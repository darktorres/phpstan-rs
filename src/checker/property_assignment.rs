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
        // A `readonly` property may only be initialized once, from within
        // the declaring class's own scope - in practice its constructor.
        // Flags every write outside the literal syntactic body of the
        // declaring class's own constructor rather than attempting real
        // "already initialized" dataflow; a promoted constructor parameter
        // never reaches this check (no `ExprKind::Assign` node exists for
        // it), so it counts as the allowed first write automatically.
        //
        // PHP 8.3 allows reassigning a `readonly` property once from
        // within the declaring class's own `__clone()` (a "deep clone"
        // idiom), gated on the configured `php_version` being `>= 8.3`
        // (`PhpFeature::ReadonlyPropertyReinitializationOnClone`); without
        // a configured version, `__clone()` is treated like any other
        // non-constructor method, since reassigning on PHP < 8.3 is a
        // real runtime error.
        let in_clone_reinit = self.current_class.as_deref() == Some(owner.as_str())
            && self.current_method.as_deref() == Some("__clone")
            && self.registry.php_version.is_some_and(|v| {
                crate::php_version::PhpFeature::ReadonlyPropertyReinitializationOnClone
                    .is_available(v)
            });
        // `@phpstan-immutable`/`@phpstan-readonly` on the *class* (not the
        // property) makes every property of the class - and any subclass,
        // via `class_is_immutable`'s `extends`-walk - behave as if
        // individually `readonly`.
        if (prop_sig.is_readonly || self.registry.class_is_immutable(&owner))
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
        // `@internal`/`@deprecated`: `check_assignment_target`'s own
        // `PropertyAccess` arm deliberately only checks the *base* (to
        // avoid misflagging dynamic-property creation), so these checks
        // must run here for the write side instead.
        self.check_internal_property_access(&owner, prop, prop_sig, lhs.span, label);
        self.check_restricted_property_access(&owner, prop, false, lhs.span, label);
        self.check_deprecated_property_access(&owner, prop, prop_sig, lhs.span, label);
        // Checked against `write_visibility`, not `visibility` - PHP 8.4
        // asymmetric visibility (`public private(set) string $x;`) means a
        // write can be legal/illegal at a strictly narrower scope than a
        // read of the same property; for an ordinary property the two are
        // identical.
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
        if actual == Type::Mixed
            || self
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
}
