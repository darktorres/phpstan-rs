use crate::ast::*;
use crate::checker::Checker;
use crate::checker::oop::visibility_word;
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
    /// which `own_is_static: false` encodes directly rather than
    /// guessing.
    pub(crate) fn check_property_overrides(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(own_sig) = info.properties.get(&p.name) else {
                continue;
            };
            self.check_one_property_override(
                fqn,
                &p.name,
                own_sig.is_static,
                own_sig.is_readonly,
                &p.visibility,
                p.span,
            );
        }
        for m in &c.methods {
            if m.function.name != "__construct" {
                continue;
            }
            for param in &m.function.params {
                let Some(visibility) = &param.promoted_visibility else {
                    continue;
                };
                self.check_one_property_override(
                    fqn,
                    &param.name,
                    false,
                    param.promoted_readonly,
                    visibility,
                    param.span,
                );
            }
        }
    }

    fn check_one_property_override(
        &mut self,
        fqn: &str,
        name: &str,
        own_is_static: bool,
        own_is_readonly: bool,
        own_visibility: &Visibility,
        span: Span,
    ) {
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
        if ancestor_sig.is_static && !own_is_static {
            self.report(
                fqn.to_string(),
                format!(
                    "Non-static property {fqn}::${name} overrides static property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.nonStatic",
            );
        } else if !ancestor_sig.is_static && own_is_static {
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
        if ancestor_sig.is_readonly && !own_is_readonly {
            self.report(
                fqn.to_string(),
                format!(
                    "Readwrite property {fqn}::${name} overrides readonly property {ancestor_owner}::${name}"
                ),
                span,
                0,
                "property.readWrite",
            );
        } else if !ancestor_sig.is_readonly && own_is_readonly {
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
            Visibility::Public => *own_visibility != Visibility::Public,
            Visibility::Protected => *own_visibility == Visibility::Private,
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
    }
}
