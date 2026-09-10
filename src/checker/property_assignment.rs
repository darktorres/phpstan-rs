use crate::ast::*;
use crate::checker::{Checker, DiagnosticBuilder, Locals, MemberVisibilityCheck};
use crate::types::{self, Type};

impl<'a> Checker<'a> {
    pub(crate) fn check_property_assignment(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        locals: &Locals,
        label: &str,
    ) {
        let ExprKind::PropertyAccess(base, prop, _) = &lhs.kind else {
            return;
        };
        // Real bug found and fixed this session: this used to require
        // `base`'s own type to be literally `Type::Object`, silently
        // skipping *every* check below (readonly, visibility, `@internal`/
        // `@deprecated`, and the assigned value's own type) whenever it
        // was a `Union`/`Intersection` instead - the exact same shape the
        // read side (`ExprKind::PropertyAccess` above) had, and just as
        // reachable here: a receiver narrowed via `instanceof` from an
        // unrelated interface, then written to.
        let base_ty = self.infer_expr(base, locals);
        let Some((class, type_args, owner, prop_sig)) = self.find_property_receiver(&base_ty, prop)
        else {
            // Not a declared property: could be a dynamic property, which
            // is legal (if deprecated) PHP - not flagged here, only wrong
            // types on properties we do know about.
            return;
        };
        let prop_sig = &prop_sig;
        // Finding 47: a `readonly` property may only be initialized once,
        // from within the scope of the class that declares it - in
        // practice, almost always its own constructor. Pragmatic first
        // cut (per the plan's own text): flag every write outside the
        // literal syntactic body of the declaring class's own
        // constructor, rather than attempting real "has this exact
        // property already been initialized" dataflow - a promoted
        // constructor parameter never reaches this check at all (no
        // `ExprKind::Assign` node exists for it in source), so it counts
        // as the allowed first write automatically.
        //
        // Real corpus find (`cyclonedx/cyclonedx-library`'s own
        // `Component::__clone()`): PHP 8.3 added a real, deliberate
        // exception for exactly this shape - reassigning a `readonly`
        // property once from within the declaring class's own
        // `__clone()` (`ReadOnlyPropertyAssignRule`'s own `$inClone`
        // gate, confirmed directly against `vendor/phpstan-src`) - a
        // "deep clone" idiom, not a bug. Gated on the configured
        // `php_version` being `>= 8.3` (`PhpFeature::
        // ReadonlyPropertyReinitializationOnClone`), matching real
        // PHPStan's own identical version check exactly - unset or
        // older stays silent-*into-flagging*, not the other way around:
        // reassigning here on PHP < 8.3 is a real, unconditional runtime
        // error, so the safe default without a configured version is to
        // keep treating `__clone()` the same as any other non-
        // constructor method.
        let in_clone_reinit = self.current_class.as_deref() == Some(owner.as_str())
            && self.current_method.as_deref() == Some("__clone")
            && self.registry.php_version.is_some_and(|v| {
                crate::php_version::PhpFeature::ReadonlyPropertyReinitializationOnClone
                    .is_available(v)
            });
        if prop_sig.is_readonly
            && !in_clone_reinit
            && (self.current_class.as_deref() != Some(owner.as_str())
                || self.current_method.as_deref() != Some("__construct"))
        {
            self.report_builder(
                DiagnosticBuilder::new(
                    label.to_string(),
                    format!("{owner}::${prop} is readonly and cannot be modified here"),
                    lhs.span,
                    0,
                    "property.readOnlyAssignNotInConstructor",
                )
                .tip("readonly properties can only be written once, from inside the declaring class's own constructor"),
            );
        }
        // Phase 67/Phase 39 follow-up: `@internal`/`@deprecated` - the
        // read side (`ExprKind::PropertyAccess` in `checker/expr.rs`)
        // already covers both; a plain write reached this function
        // directly instead (`check_assignment_target`'s own
        // `PropertyAccess` arm deliberately only checks the *base*, to
        // avoid misflagging dynamic-property creation), so neither check
        // ever ran for a write. Confirmed via the exact same corpus this
        // project already used for the read side - see ROADMAP.md.
        self.check_internal_property_access(&owner, prop, prop_sig, lhs.span, label);
        self.check_deprecated_property_access(&owner, prop, prop_sig, lhs.span, label);
        // Phase 57: visibility - shares the same "property.private"/
        // "property.protected" identifiers a read gets (`ExprKind::
        // PropertyAccess`, `checker/expr.rs`), matching real PHPStan's
        // own `AccessPropertiesCheck::check()`, which both the read rule
        // and this project's own write-side equivalent delegate to
        // identically. Checked against `write_visibility`, not
        // `visibility` - PHP 8.4 asymmetric visibility
        // (`public private(set) string $x;`) means a write can be
        // legal/illegal at a strictly narrower scope than a read of the
        // exact same property; for an ordinary, symmetric property the
        // two are identical, so this is a no-op change for every case
        // that isn't asymmetric.
        self.check_member_visibility(MemberVisibilityCheck {
            kind: "property",
            owner: &owner,
            member_name: prop,
            visibility: &prop_sig.write_visibility,
            describe: &format!("property {owner}::${prop}"),
            span: lhs.span,
            label,
        });
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
        if actual == Type::Mixed || self.registry.is_assignable(&actual, &expected) {
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
}
