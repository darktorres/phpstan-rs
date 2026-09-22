//! `instanceof` checks whose outcome is already decided by the checked
//! expression's type, mirroring real PHPStan's `ImpossibleInstanceOfRule`:
//! `instanceof.alwaysTrue`/`.alwaysFalse`, and `instanceof.invalidExprType`
//! for a dynamic class-name operand that can't be a string or an object.

use crate::ast::{ClassKind, Expr, InstanceofTarget};
use crate::checker::expr::type_can_never_be_object;
use crate::checker::instantiation::type_can_never_name_a_class;
use crate::checker::{Checker, Locals};
use crate::token::Span;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// `is_exempt_last_condition` is the "last `elseif` with no `else`"
    /// position, where an always-true check acts as an implicit `else`
    /// and isn't reported.
    pub(crate) fn check_impossible_instanceof(
        &mut self,
        inner: &Expr,
        target: &InstanceofTarget,
        span: Span,
        locals: &Locals,
        label: &str,
        is_exempt_last_condition: bool,
    ) {
        let expr_ty = self.infer_expr(inner, locals);
        // A dynamic `class-string<C>` target names `C` or any subclass of
        // it, so only "never" carries over, or "always" when `C` is final.
        let (class, only_subclass_known) = match target {
            InstanceofTarget::Expr(target_expr) => {
                let target_ty = self.infer_expr(target_expr, locals);
                if type_can_never_name_a_class(&target_ty) {
                    self.report(
                        label.to_string(),
                        format!(
                            "Instanceof between {expr_ty} and {target_ty} results in an error."
                        ),
                        span,
                        0,
                        "instanceof.invalidExprType",
                    );
                    return;
                }
                match target_ty {
                    Type::StringLiteral(name) => (name, false),
                    Type::ClassString(inner_ty) => match *inner_ty {
                        Type::Object(name, _) => (name, true),
                        _ => return,
                    },
                    _ => return,
                }
            }
            InstanceofTarget::Name(name, _) => {
                if name.eq_ignore_ascii_case("static") {
                    return;
                }
                let Type::Object(class, _) = self.resolve_name_expr(name) else {
                    return;
                };
                (class, false)
            }
        };
        let Some((class, class_info)) = self.registry.lookup_class(class.trim_start_matches('\\'))
        else {
            return;
        };
        let class_is_final = class_info.is_final || class_info.kind == ClassKind::Enum;
        let class = class.to_string();
        let Some(always) = self.instanceof_verdict(&expr_ty, &class) else {
            return;
        };
        if always && only_subclass_known && !class_is_final {
            return;
        }
        if always && is_exempt_last_condition {
            return;
        }
        self.report(
            label.to_string(),
            format!("Instanceof between {expr_ty} and {class} will always evaluate to {always}."),
            span,
            4,
            if always {
                "instanceof.alwaysTrue"
            } else {
                "instanceof.alwaysFalse"
            },
        );
    }

    /// Whether a value of type `ty` is always (`Some(true)`), never
    /// (`Some(false)`), or only sometimes (`None`) an instance of the
    /// registered class-like `target`.
    fn instanceof_verdict(&self, ty: &Type, target: &str) -> Option<bool> {
        match ty {
            Type::Object(class, _) => self.class_instanceof_verdict(class, target),
            Type::Union(members) => {
                let mut verdicts = members.iter().map(|m| self.instanceof_verdict(m, target));
                let first = verdicts.next()??;
                verdicts.all(|v| v == Some(first)).then_some(first)
            }
            Type::Intersection(members) => {
                let verdicts: Vec<_> = members
                    .iter()
                    .filter_map(|m| self.instanceof_verdict(m, target))
                    .collect();
                if verdicts.contains(&true) {
                    Some(true)
                } else if verdicts.contains(&false) {
                    Some(false)
                } else {
                    None
                }
            }
            Type::TemplateParam(param) => self.instanceof_verdict(&param.bound, target),
            Type::Never => None,
            other if type_can_never_be_object(other) => Some(false),
            _ => None,
        }
    }

    fn class_instanceof_verdict(&self, class: &str, target: &str) -> Option<bool> {
        let (class, info) = self.registry.lookup_class(class)?;
        let (_, target_info) = self.registry.lookup_class(target)?;
        if self.registry.is_subclass_of(class, target) {
            return Some(true);
        }
        if target_info.kind == ClassKind::Trait {
            return Some(false);
        }
        if self.registry.is_subclass_of(target, class)
            || self.registry.has_unresolvable_ancestor(class)
        {
            return None;
        }
        let class_is_final = info.is_final || info.kind == ClassKind::Enum;
        let target_is_final = target_info.is_final || target_info.kind == ClassKind::Enum;
        let both_classes = info.kind == ClassKind::Class && target_info.kind == ClassKind::Class;
        (class_is_final || target_is_final || both_classes).then_some(false)
    }
}
