use crate::ast::*;
use crate::checker::Checker;
use crate::checker::oop::visibility_word;
use crate::registry::PropertySig;
use crate::token::Span;

impl<'a> Checker<'a> {
    /// Phase 64's own still-open property-override half - real PHPStan's
    /// `OverridingPropertyRule` equivalent, scoped to the three mismatch
    /// kinds answerable with fields `PropertyDecl`/`PropertySig` already
    /// carry (`is_static`/`is_readonly`/`visibility` - all three
    /// confirmed directly via `php -r` as real, unconditional fatal
    /// errors, in whichever direction is checked below; the *other*
    /// direction of each is real, legal PHP and deliberately left
    /// unflagged). Native-type-variance (`property.nativeType` - real
    /// PHP requires a property's own type to match its ancestor's
    /// *exactly*, invariantly, unlike a method's covariant return type)
    /// is deliberately not attempted here: `PropertySig` only carries a
    /// single, already-docblock-merged `ty` (no `return_type_native`-
    /// style split the way `FunctionSig` has for return types), and
    /// comparing that merged type directly would risk the identical
    /// false-positive class `method.childReturnType`'s own doc comment
    /// above already found and fixed for methods (an ancestor's own
    /// native type re-declared identically, but with an *added* `@var`
    /// docblock refinement on just one side) - needs a `PropertySig`
    /// split first, not attempted in this pass. `#[Override]`
    /// (`property.override`/`missingOverride`) and property hooks
    /// (`notReadable`/`notWritable`/`abstractOverridingNonAbstractHook`)
    /// are separate, still-open items too. A promoted constructor
    /// property is a real property for override purposes too (confirmed
    /// directly: `class Child extends Base { public function
    /// __construct(public readonly int $x) {} }` overriding a plain
    /// `Base::$x` still fatals the identical way) - checked via the
    /// shared `check_one_property_override` helper below, once per
    /// plain declaration and once per promoted `__construct` parameter.
    /// A promoted property is never `static` (no such syntax exists),
    /// which `own_sig.is_static: false` encodes directly rather than
    /// guessing.
    pub(crate) fn check_property_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(own_sig) = info.properties.get(&p.name).cloned() else {
                continue;
            };
            self.check_property_override_attribute(fqn, &p.name, &p.attrs, p.span);
            self.check_one_property_override(fqn, &p.name, &own_sig, p.span);
        }
        for m in &c.methods {
            if m.function.name != "__construct" {
                continue;
            }
            for param in &m.function.params {
                if param.promoted_visibility.is_none() {
                    continue;
                }
                let Some(own_sig) = info.properties.get(&param.name).cloned() else {
                    continue;
                };
                self.check_property_override_attribute(fqn, &param.name, &param.attrs, param.span);
                self.check_one_property_override(fqn, &param.name, &own_sig, param.span);
            }
        }
    }

    /// Phase 64 Item 5: `OverridingPropertyRule`'s own `hasOverrideAttribute`/
    /// `filterOverrideAttribute` half for properties - the method-level
    /// equivalent already exists (`FunctionSig::has_override_attr`,
    /// `check_one_method_override`); a property needs no analogous
    /// registry field, since (unlike a method) it's never hoisted through
    /// a separate trait-merge lookup path before this check runs - `p.attrs`/
    /// `param.attrs` on the class's own textual declaration is already
    /// the complete, real answer. A `private` ancestor property doesn't
    /// count as a real override target here, mirroring `check_one_
    /// property_override`'s own identical exemption just below (an
    /// unrelated declaration, not something `#[Override]` could
    /// meaningfully claim to override).
    fn check_property_override_attribute(
        &mut self,
        fqn: &str,
        name: &str,
        attrs: &[Attribute],
        span: Span,
    ) {
        if !attrs
            .iter()
            .any(|a| crate::registry::is_override_attribute_name(&a.name))
        {
            return;
        }
        if self.registry.has_unresolvable_ancestor(fqn) {
            return;
        }
        let has_real_ancestor = self
            .registry
            .lookup_ancestor_property_with_owner(fqn, name)
            .is_some_and(|(_, sig)| sig.visibility != Visibility::Private);
        if !has_real_ancestor {
            self.report(
                fqn.to_string(),
                format!(
                    "Property {fqn}::${name} has #[\\Override] attribute, but no matching parent property exists"
                ),
                span,
                0,
                "property.override",
            );
        }
    }

    fn check_one_property_override(
        &mut self,
        fqn: &str,
        name: &str,
        own_sig: &PropertySig,
        span: Span,
    ) {
        // Phase 64 Item 3: overriding a `final` property is a real,
        // unconditional PHP fatal error (PHP 8.4+) - same "runs before
        // every other exemption below" ordering `check_one_method_
        // override`'s own final-ancestor check already uses.
        if let Some(ancestor) = self.registry.find_final_ancestor_property(fqn, name) {
            self.report(
                fqn.to_string(),
                format!("Property {fqn}::${name} overrides final property {ancestor}::${name}"),
                span,
                0,
                "property.parentPropertyFinal",
            );
        }
        let Some((ancestor_owner, ancestor_sig)) =
            self.registry.lookup_ancestor_property_with_owner(fqn, name)
        else {
            return;
        };
        // A `private` ancestor property is an unrelated declaration, not
        // a real override target - same reasoning as `check_method_
        // overrides`'s identical exemption.
        if ancestor_sig.visibility == Visibility::Private {
            return;
        }
        if ancestor_sig.is_static && !own_sig.is_static {
            self.report(
                fqn.to_string(),
                format!(
                    "Non-static property {fqn}::${name} overrides static property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.nonStatic",
            );
        } else if !ancestor_sig.is_static && own_sig.is_static {
            self.report(
                fqn.to_string(),
                format!(
                    "Static property {fqn}::${name} overrides non-static property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.static",
            );
        }
        if ancestor_sig.is_readonly && !own_sig.is_readonly {
            self.report(
                fqn.to_string(),
                format!(
                    "Readwrite property {fqn}::${name} overrides readonly property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.readWrite",
            );
        } else if !ancestor_sig.is_readonly && own_sig.is_readonly {
            self.report(
                fqn.to_string(),
                format!(
                    "Readonly property {fqn}::${name} overrides readwrite property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.readOnly",
            );
        }
        // Same two-way visibility-narrowing split `check_method_
        // overrides` already uses, one member kind over.
        let narrows_visibility = match ancestor_sig.visibility {
            Visibility::Public => own_sig.visibility != Visibility::Public,
            Visibility::Protected => own_sig.visibility == Visibility::Private,
            Visibility::Private => false,
        };
        if narrows_visibility {
            self.report(
                fqn.to_string(),
                format!(
                    "{fqn}::${name} overriding {} property {ancestor_owner}::${name} must not reduce visibility",
                    visibility_word(&ancestor_sig.visibility)
                ),
                span,
                0,
                "property.visibility",
            );
        }
        self.check_property_hook_override(fqn, name, own_sig, &ancestor_owner, ancestor_sig, span);
    }

    /// Phase 64 Item 1: `OverridingPropertyRule`'s own property-hook half -
    /// `notReadable`/`notWritable`/`abstractOverridingNonAbstractHook`,
    /// confirmed live (see this phase's own investigation notes). Gated
    /// on `ancestor_sig.is_virtual || own_sig.is_virtual` first - an
    /// ordinary, non-hooked property on either side is unconditionally
    /// both readable and writable, so nothing here could ever apply to
    /// the vast majority of properties (avoids any risk of regressing
    /// them).
    fn check_property_hook_override(
        &mut self,
        fqn: &str,
        name: &str,
        own_sig: &PropertySig,
        ancestor_owner: &str,
        ancestor_sig: &PropertySig,
        span: Span,
    ) {
        if !ancestor_sig.is_virtual && !own_sig.is_virtual {
            return;
        }
        let ancestor_readable = !ancestor_sig.is_virtual || ancestor_sig.has_get_hook;
        let own_readable = !own_sig.is_virtual || own_sig.has_get_hook;
        if ancestor_readable && !own_readable {
            self.report(
                fqn.to_string(),
                format!(
                    "Property {fqn}::${name} does not have `get` hook and is not readable, but is required to be readable by property {ancestor_owner}::${name}."
                ),
                span,
                0,
                "property.notReadable",
            );
        }
        let ancestor_writable = !ancestor_sig.is_virtual || ancestor_sig.has_set_hook;
        let own_writable = !own_sig.is_virtual || own_sig.has_set_hook;
        if ancestor_writable && !own_writable {
            self.report(
                fqn.to_string(),
                format!(
                    "Property {fqn}::${name} does not have `set` hook and is not writable, but is required to be writable by property {ancestor_owner}::${name}."
                ),
                span,
                0,
                "property.notWritable",
            );
        }
        // A hook only makes sense to compare "abstract-ness" for when
        // both sides actually declare that same hook type at all -
        // real PHP's own rule only fires on that exact overlap.
        if own_sig.is_abstract && !ancestor_sig.is_abstract {
            if own_sig.has_get_hook && ancestor_sig.has_get_hook {
                self.report(
                    fqn.to_string(),
                    format!(
                        "Cannot make non abstract method {ancestor_owner}::${name}::get() abstract in class {fqn}"
                    ),
                    span,
                    0,
                    "property.abstractOverridingNonAbstractHook",
                );
            }
            if own_sig.has_set_hook && ancestor_sig.has_set_hook {
                self.report(
                    fqn.to_string(),
                    format!(
                        "Cannot make non abstract method {ancestor_owner}::${name}::set() abstract in class {fqn}"
                    ),
                    span,
                    0,
                    "property.abstractOverridingNonAbstractHook",
                );
            }
        }
    }
}
