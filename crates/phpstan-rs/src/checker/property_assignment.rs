use crate::checker::{
    Checker, Locals, MemberKind, MemberLabel, MemberVisibilityCheck, UsageTokenKind,
};
use syntax::ast::*;
use types::php_version::PhpFeature;

impl<'a> Checker<'a> {
    pub(crate) fn check_property_assignment(
        &mut self,
        lhs: &ExprNode,
        rhs: &ExprNode,
        locals: &mut Locals,
    ) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let ExprKind::PropertyAccess(base, prop, _) = &lhs.kind else {
            return;
        };
        let Some(base) = arenas.expr(*base) else {
            return;
        };
        // `base`'s type may be a `Union`/`Intersection` (e.g. a receiver
        // narrowed via `instanceof` from an unrelated interface), not just
        // a plain `Object`; `find_property_receiver` handles that shape.
        let base_ty = self.infer_expr(base, locals);
        let Some((class, type_args, owner, prop_sig)) = self.find_property_receiver(&base_ty, prop)
        else {
            // Could be a dynamic property, which is legal (if deprecated)
            // PHP - not flagged here, only wrong types on known properties.
            return;
        };
        let prop_sig = &prop_sig;
        // `WritingToReadOnlyPropertiesRule` - a `@property-read` tag, or a
        // virtual hooked property without a `set` hook. Replaces the
        // readonly checks below for the same write.
        let can_write = self.is_member_access_visible(
            &owner,
            &self.write_visibility(prop_sig),
            prop,
            "property",
        );
        let not_writable = can_write && !prop_sig.is_writable();
        if not_writable {
            report!(self, 
                format!("Property {owner}::${prop} is not writable."),
                self.expr_span(lhs.id),
                0,
                "assign.propertyReadOnly",
            );
        } else if can_write {
            let on_this = matches!(&base.kind, ExprKind::Variable(v) if **v == *"this");
            let fact = if prop_sig.is_readonly() && !prop_sig.readonly_by_phpdoc() {
                self.native_readonly_assignment(&owner, prop, prop_sig, on_this, locals)
            } else if prop_sig.readonly_by_phpdoc() || self.registry.class_is_immutable(&owner) {
                self.phpdoc_readonly_assignment(&owner, prop, prop_sig, on_this)
            } else {
                None
            };
            if let Some((message, identifier)) = fact {
                report!(self, message, self.expr_span(lhs.id), 0, identifier);
            }
        }
        // `@internal`/`@deprecated`: `check_assignment_target`'s own
        // `PropertyAccess` arm deliberately only checks the *base* (to
        // avoid misflagging dynamic-property creation), so these checks
        // must run here for the write side instead.
        self.check_internal_property_access(&owner, prop, prop_sig, self.expr_span(lhs.id));
        self.check_restricted_property_access(&owner, prop, false, self.expr_span(lhs.id));
        self.check_deprecated_property_access(&owner, prop, prop_sig, self.expr_span(lhs.id));
        self.record_member_usage_token(
            UsageTokenKind::Property,
            &owner,
            prop_sig.deprecated().is_some(),
            prop_sig.is_internal(),
            self.expr_span(lhs.id),
        );
        // Checked against `write_visibility`, not `visibility` - PHP 8.4
        // asymmetric visibility (`public private(set) string $x;`) means a
        // write can be legal/illegal at a strictly narrower scope than a
        // read of the same property; for an ordinary property the two are
        // identical.
        let write_visibility = self.write_visibility(prop_sig);
        let set_visibility_only = write_visibility != prop_sig.visibility()
            && self.is_member_access_visible(&owner, &prop_sig.visibility(), prop, "property")
            && !self.is_member_access_visible(&owner, &write_visibility, prop, "property");
        if set_visibility_only {
            let (word, identifier) = match write_visibility {
                Visibility::Private => ("private(set)", "assign.propertyPrivateSet"),
                _ => ("protected(set)", "assign.propertyProtectedSet"),
            };
            report!(self, 
                format!("Assign to {word} property {class}::${prop}."),
                self.expr_span(lhs.id),
                0,
                identifier,
            );
        } else {
            self.check_member_visibility(MemberVisibilityCheck {
                member: MemberLabel {
                    kind: MemberKind::Property,
                    owner: &owner,
                    name: prop,
                },
                visibility: &write_visibility,
                span: self.expr_span(lhs.id),
            });
        }
        let bindings = self.class_template_bindings(&class, &type_args, &owner);
        // A PHP 8.4 `set(Type $value)` property hook can accept a wider
        // type than the property's own backing type (it does its own
        // coercion inside the hook body) - a write is checked against
        // that hook's own parameter type when one exists, not the
        // property's read type.
        let write_ty = prop_sig.set_ty().as_ref().unwrap_or(prop_sig.ty());
        let expected = write_ty.substitute_templates(&bindings, &self.relation());
        let expected = self.resolve_pseudo(&expected, &class);
        let actual = self.infer_expr(rhs, locals);
        // `check_too_wide_property_types`'s facts - scoped to
        // `write_visibility == Private`, the only visibility for which
        // every write reaches this choke point (a protected/public
        // property could be written from a subclass this project never
        // sees). A write the scope may not make fails at runtime, so it
        // assigns nothing.
        if prop_sig.write_visibility() == Visibility::Private && can_write && !not_writable {
            let writer = self
                .current_class
                .as_deref()
                .and_then(|class| self.registry.class(class).map(|(id, _, _)| id));
            let property =
                writer.and_then(|id| self.registry.property_ref_by_name(id, prop.as_ref()));
            if let (Some(writer), Some(property)) = (writer, property) {
                self.property_assign_facts
                    .entry(writer)
                    .or_default()
                    .entry(property)
                    .or_default()
                    .push(actual.clone());
            }
        }
        // See `checker::calls`' own identical comment: `rejected_from`
        // already accepts a `Mixed` actual value below real PHPStan's own
        // level 9/10, so no separate bail-out is needed (and one here
        // would wrongly skip this check at either).
        let Some(from) = self.rejected_from(&expected, &actual) else {
            return;
        };
        report_found_from!(self, 
            format!("{class}::${prop} expects {expected}, {actual} given"),
            self.expr_span(rhs.id),
            3,
            from,
            "assign.propertyType",
        );
    }

    /// Who may write `sig`'s property: its declared set visibility, or,
    /// from PHP 8.4, `protected(set)` for a public native `readonly` one.
    pub(crate) fn write_visibility(&self, sig: &crate::registry::PropertyReflection) -> Visibility {
        if sig.implicit_protected_set()
            && self
                .registry
                .php_supports_somewhere(PhpFeature::AsymmetricVisibility)
                != Some(false)
        {
            return Visibility::Protected;
        }
        sig.write_visibility()
    }

    /// `ReadOnlyPropertyAssignRule`: a native `readonly` property is
    /// assigned only in its declaring class (or, with PHP 8.4's implicit
    /// `protected(set)`, a subclass), in a constructor, `__unserialize()`
    /// or (PHP 8.3+) `__clone()`, on `$this` - and in `__clone()` once.
    /// Returns the message and identifier of a violation.
    fn native_readonly_assignment(
        &self,
        owner: &str,
        prop: &str,
        prop_sig: &crate::registry::PropertyReflection,
        on_this: bool,
        locals: &mut Locals,
    ) -> Option<(String, &'static str)> {
        let supports =
            |feature: PhpFeature| self.registry.php_supports_everywhere(feature) != Some(false);
        let out_of_class = || {
            (
                format!(
                    "Readonly property {owner}::${prop} is assigned outside of its declaring class."
                ),
                "property.readOnlyAssignOutOfClass",
            )
        };
        let Some(class) = self.current_class.as_deref() else {
            return Some(out_of_class());
        };
        if class != owner {
            let allowed_in_subclass = supports(PhpFeature::AsymmetricVisibility)
                && prop_sig.write_visibility() != Visibility::Private
                && self.registry.is_subclass_of(class, owner);
            if !allowed_in_subclass {
                return Some(out_of_class());
            }
        }
        let method = self
            .current_method
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        let in_clone =
            method == "__clone" && supports(PhpFeature::ReadonlyPropertyReinitializationOnClone);
        if syntax::ast::is_constructor_name(&method) || method == "__unserialize" || in_clone {
            if !on_this {
                return Some((
                    format!("Readonly property {owner}::${prop} is not assigned on $this."),
                    "property.readOnlyAssignNotOnThis",
                ));
            }
            if in_clone
                && locals
                    .clone_reinitialized_mut()
                    .insert(prop.to_string())
                    .is_some()
            {
                return Some((
                    format!("Readonly property {owner}::${prop} is already assigned."),
                    "assign.readOnlyProperty",
                ));
            }
            return None;
        }
        Some((
            format!("Readonly property {owner}::${prop} is assigned outside of the constructor."),
            "property.readOnlyAssignNotInConstructor",
        ))
    }

    /// `ReadOnlyByPhpDocPropertyAssignRule`: a `@readonly` property (or
    /// any property of an `@immutable` class) is assigned only in its
    /// declaring class, in a constructor or `__unserialize()`, on
    /// `$this`, unless `@phpstan-allow-private-mutation` lets the class
    /// write it anywhere.
    fn phpdoc_readonly_assignment(
        &self,
        owner: &str,
        prop: &str,
        prop_sig: &crate::registry::PropertyReflection,
        on_this: bool,
    ) -> Option<(String, &'static str)> {
        if self.current_class.as_deref() != Some(owner) {
            return Some((
                format!(
                    "@readonly property {owner}::${prop} is assigned outside of its declaring class."
                ),
                "property.readOnlyByPhpDocAssignOutOfClass",
            ));
        }
        let method = self
            .current_method
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        if syntax::ast::is_constructor_name(&method) || method == "__unserialize" {
            return (!on_this).then(|| {
                (
                    format!("@readonly property {owner}::${prop} is not assigned on $this."),
                    "property.readOnlyByPhpDocAssignNotOnThis",
                )
            });
        }
        if prop_sig.allows_private_mutation() {
            return None;
        }
        Some((
            format!("@readonly property {owner}::${prop} is assigned outside of the constructor."),
            "property.readOnlyByPhpDocAssignNotInConstructor",
        ))
    }

    /// `$x = &$obj->prop` / `$x = &Foo::$prop`: taking a reference to a
    /// property the current scope can't write (`property.assignByRef`,
    /// PHP 8.4+ asymmetric visibility), or to a readonly one
    /// (`property.readOnlyAssignByRef`/`property.readOnlyByPhpDocAssignByRef`).
    pub(crate) fn check_property_assign_by_ref(&mut self, rhs: &ExprNode, locals: &Locals) {
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let (owner, prop, sig, is_static_fetch) = match &rhs.kind {
            ExprKind::PropertyAccess(base, prop, _) => {
                let Some(base) = arenas.expr(*base) else {
                    return;
                };
                let base_ty = self.infer_expr(base, locals);
                let Some((_, _, owner, sig)) = self.find_property_receiver(&base_ty, prop) else {
                    return;
                };
                (owner, prop.to_string(), sig, false)
            }
            ExprKind::StaticPropertyFetch(target, prop) => {
                let Some(class) = target
                    .as_name()
                    .and_then(|n| self.resolve_static_class_name(n))
                else {
                    return;
                };
                let Some((owner, sig)) = self.registry.lookup_property_with_owner(&class, prop)
                else {
                    return;
                };
                (owner.clone(), prop.to_string(), sig.clone(), true)
            }
            _ => return,
        };
        let write_visibility = self.write_visibility(&sig);
        let can_write = self.is_member_access_visible(&owner, &write_visibility, &prop, "property");
        if !can_write {
            let asymmetric_supported = self
                .registry
                .php_supports_somewhere(types::php_version::PhpFeature::PropertyHooks)
                != Some(false);
            if asymmetric_supported && !is_static_fetch {
                let description = if write_visibility != sig.visibility() {
                    match write_visibility {
                        Visibility::Private => "private(set)",
                        _ => "protected(set)",
                    }
                } else if sig.visibility() == Visibility::Private {
                    "private"
                } else {
                    "protected"
                };
                report!(self, format!(
                        "Property {owner}::${prop} with {description} visibility is assigned by reference."
                    ),
                    self.expr_span(rhs.id),
                    0,
                    "property.assignByRef",
                );
            }
            return;
        }
        if sig.is_readonly()
            && !sig.readonly_by_phpdoc()
            && !sig.tag_read_only()
            && !is_static_fetch
        {
            report!(self, 
                format!("Readonly property {owner}::${prop} is assigned by reference."),
                self.expr_span(rhs.id),
                3,
                "property.readOnlyAssignByRef",
            );
        } else if sig.readonly_by_phpdoc() {
            report!(self, 
                format!("@readonly property {owner}::${prop} is assigned by reference."),
                self.expr_span(rhs.id),
                3,
                "property.readOnlyByPhpDocAssignByRef",
            );
        }
    }
}
