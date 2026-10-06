//! `property.defaultValue` - real PHPStan's own `Rules/Properties/
//! DefaultValueTypesAssignedToPropertiesRule.php` (level 3): a
//! property's own default value expression's type must be assignable to
//! its declared (writable) type.

use crate::checker::Checker;
use syntax::ast::*;

impl<'a> Checker<'a> {
    /// Mirrors real PHPStan's own `getWritableType()` (`set_ty` when a
    /// PHP 8.4 `set(Type $value)` hook declares its own, wider accepted
    /// type, `ty` otherwise - `PropertyReflection::set_ty`'s own doc comment).
    /// The untyped-property exemption below mirrors real PHPStan's own
    /// `!hasNativeType()` special case exactly: only a literal `null`
    /// default is exempted, checked against the raw AST `type_hint`
    /// (never the resolved `Type`, which may already carry a `@var`-tag
    /// type even with no *native* hint at all) - real PHP quietly
    /// defaults every untyped property to `null` regardless of what its
    /// own `@var` tag claims, so an explicit `= null` on one is always
    /// legal no matter how narrow that tag's own type is. Any other
    /// default on an untyped property is still checked normally, against
    /// whatever type `ty`/`set_ty` resolved to (its own `@var` tag, or
    /// `Mixed` with none).
    pub(crate) fn check_default_property_types(&mut self, fqn: &str, c: &ClassDef) {
        for p in &c.properties {
            let Some(default) = &p.default else {
                continue;
            };
            let Some(prop_sig) = self.registry.lookup_property(fqn, p.name.as_ref()) else {
                continue;
            };
            if p.type_hint.is_none()
                && matches!(
                    default,
                    Expr {
                        kind: ExprKind::Literal(Literal::Null),
                        ..
                    }
                )
            {
                continue;
            }
            let property_ty = prop_sig.set_ty().as_ref().unwrap_or(prop_sig.ty());
            // `self::`/`static::` in the default resolve against the
            // declaring class, which isn't otherwise the current class
            // outside a method body.
            let previous_class = self.current_class.replace(fqn.to_string());
            let default_ty = self.infer_expr(default, &self.new_locals());
            self.current_class = previous_class;
            if self.accepts(property_ty, &default_ty).yes() {
                continue;
            }
            let kind = if prop_sig.is_static() {
                "Static property"
            } else {
                "Property"
            };
            self.report(format!(
                    "{kind} {fqn}::${} ({property_ty}) does not accept default value of type {default_ty}.",
                    p.name,
                ),
                default.span,
                3,
                "property.defaultValue",
            );
        }
    }
}
