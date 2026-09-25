use crate::ast::*;
use crate::checker::{Checker, Locals, MemberVisibilityCheck, UsageTokenKind};
use crate::php_version::PhpFeature;
use crate::types;

impl<'a> Checker<'a> {
    pub(crate) fn check_property_assignment(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        locals: &mut Locals,
        label: &str,
    ) {
        let ExprKind::PropertyAccess(base, prop, _) = &lhs.kind else {
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
            self.report(
                label.to_string(),
                format!("Property {owner}::${prop} is not writable."),
                lhs.span,
                0,
                "assign.propertyReadOnly",
            );
        } else if can_write {
            let on_this = matches!(&base.kind, ExprKind::Variable(v) if **v == *"this");
            let fact = if prop_sig.is_readonly && !prop_sig.readonly_by_phpdoc {
                self.native_readonly_assignment(&owner, prop, prop_sig, on_this, locals)
            } else if prop_sig.readonly_by_phpdoc || self.registry.class_is_immutable(&owner) {
                self.phpdoc_readonly_assignment(&owner, prop, prop_sig, on_this)
            } else {
                None
            };
            if let Some((message, identifier)) = fact {
                self.report(label.to_string(), message, lhs.span, 0, identifier);
            }
        }
        // `@internal`/`@deprecated`: `check_assignment_target`'s own
        // `PropertyAccess` arm deliberately only checks the *base* (to
        // avoid misflagging dynamic-property creation), so these checks
        // must run here for the write side instead.
        self.check_internal_property_access(&owner, prop, prop_sig, lhs.span, label);
        self.check_restricted_property_access(&owner, prop, false, lhs.span, label);
        self.check_deprecated_property_access(&owner, prop, prop_sig, lhs.span, label);
        self.record_member_usage_token(
            UsageTokenKind::Property,
            &owner,
            prop_sig.deprecated.is_some(),
            prop_sig.is_internal,
            lhs.span,
        );
        // Checked against `write_visibility`, not `visibility` - PHP 8.4
        // asymmetric visibility (`public private(set) string $x;`) means a
        // write can be legal/illegal at a strictly narrower scope than a
        // read of the same property; for an ordinary property the two are
        // identical.
        let write_visibility = self.write_visibility(prop_sig);
        let set_visibility_only = write_visibility != prop_sig.visibility
            && self.is_member_access_visible(&owner, &prop_sig.visibility, prop, "property")
            && !self.is_member_access_visible(&owner, &write_visibility, prop, "property");
        if set_visibility_only {
            let (word, identifier) = match write_visibility {
                Visibility::Private => ("private(set)", "assign.propertyPrivateSet"),
                _ => ("protected(set)", "assign.propertyProtectedSet"),
            };
            self.report(
                label.to_string(),
                format!("Assign to {word} property {class}::${prop}."),
                lhs.span,
                0,
                identifier,
            );
        } else {
            self.check_member_visibility(MemberVisibilityCheck {
                kind: "property",
                owner: &owner,
                member_name: prop,
                visibility: &write_visibility,
                describe: &format!("property {owner}::${prop}"),
                span: lhs.span,
                label,
            });
        }
        let bindings = self.class_template_bindings(&class, &type_args, &owner);
        // A PHP 8.4 `set(Type $value)` property hook can accept a wider
        // type than the property's own backing type (it does its own
        // coercion inside the hook body) - a write is checked against
        // that hook's own parameter type when one exists, not the
        // property's read type.
        let write_ty = prop_sig.set_ty.as_ref().unwrap_or(&prop_sig.ty);
        let expected = types::substitute_template(write_ty, &bindings);
        let expected = self.resolve_pseudo(&expected, &class);
        let actual = self.infer_expr(rhs, locals);
        // `check_too_wide_property_types`'s own accumulation half - scoped
        // to `write_visibility == Private` since that's the only
        // visibility for which every real write is guaranteed to reach
        // this exact choke point (a protected/public property could be
        // written from a subclass this project never sees, so "every
        // write we've seen" would be an unsound undercount for anything
        // wider than private).
        if prop_sig.write_visibility == Visibility::Private {
            self.property_assign_facts
                .entry(crate::intern::intern(&owner))
                .or_default()
                .entry(crate::intern::intern(&prop.name))
                .or_default()
                .push(actual.clone());
        }
        // See `checker::calls`' own identical comment: `accepts_at_
        // configured_level` already accepts a `Mixed` actual value
        // unconditionally below real PHPStan's own level 9/10, so no
        // separate bail-out is needed (and one here would wrongly skip
        // this check once either is enabled).
        if self
            .registry
            .accepts_at_configured_level(&actual, &expected)
        {
            return;
        }
        self.report(
            label.to_string(),
            format!("{class}::${prop} expects {expected}, {actual} given"),
            rhs.span,
            3,
            "assign.propertyType",
        );
    }

    /// Who may write `sig`'s property: its declared set visibility, or,
    /// from PHP 8.4, `protected(set)` for a public native `readonly` one.
    pub(crate) fn write_visibility(&self, sig: &crate::registry::PropertySig) -> Visibility {
        if sig.implicit_protected_set
            && self
                .registry
                .php_version
                .is_none_or(|v| PhpFeature::AsymmetricVisibility.is_available(v))
        {
            return Visibility::Protected;
        }
        sig.write_visibility.clone()
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
        prop_sig: &crate::registry::PropertySig,
        on_this: bool,
        locals: &mut Locals,
    ) -> Option<(String, &'static str)> {
        let supports = |feature: PhpFeature| {
            self.registry
                .php_version
                .is_none_or(|v| feature.is_available(v))
        };
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
                && prop_sig.write_visibility != Visibility::Private
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
        if method == "__construct" || method == "__unserialize" || in_clone {
            if !on_this {
                return Some((
                    format!("Readonly property {owner}::${prop} is not assigned on $this."),
                    "property.readOnlyAssignNotOnThis",
                ));
            }
            if in_clone
                && locals
                    .clone_reinitialized
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
        prop_sig: &crate::registry::PropertySig,
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
        if method == "__construct" || method == "__unserialize" {
            return (!on_this).then(|| {
                (
                    format!("@readonly property {owner}::${prop} is not assigned on $this."),
                    "property.readOnlyByPhpDocAssignNotOnThis",
                )
            });
        }
        if prop_sig.allows_private_mutation {
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
    pub(crate) fn check_property_assign_by_ref(
        &mut self,
        rhs: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let (owner, prop, sig, is_static_fetch) = match &rhs.kind {
            ExprKind::PropertyAccess(base, prop, _) => {
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
                .php_version
                .is_none_or(|v| crate::php_version::PhpFeature::PropertyHooks.is_available(v));
            if asymmetric_supported && !is_static_fetch {
                let description = if write_visibility != sig.visibility {
                    match write_visibility {
                        Visibility::Private => "private(set)",
                        _ => "protected(set)",
                    }
                } else if sig.visibility == Visibility::Private {
                    "private"
                } else {
                    "protected"
                };
                self.report(
                    label.to_string(),
                    format!(
                        "Property {owner}::${prop} with {description} visibility is assigned by reference."
                    ),
                    rhs.span,
                    0,
                    "property.assignByRef",
                );
            }
            return;
        }
        if sig.is_readonly && !sig.readonly_by_phpdoc && !sig.tag_read_only && !is_static_fetch {
            self.report(
                label.to_string(),
                format!("Readonly property {owner}::${prop} is assigned by reference."),
                rhs.span,
                3,
                "property.readOnlyAssignByRef",
            );
        } else if sig.readonly_by_phpdoc {
            self.report(
                label.to_string(),
                format!("@readonly property {owner}::${prop} is assigned by reference."),
                rhs.span,
                3,
                "property.readOnlyByPhpDocAssignByRef",
            );
        }
    }
}
