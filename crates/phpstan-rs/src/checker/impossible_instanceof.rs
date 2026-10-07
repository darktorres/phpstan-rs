//! `instanceof` checks whose outcome is already decided by the checked
//! expression's type, mirroring real PHPStan's `ImpossibleInstanceOfRule`:
//! `instanceof.alwaysTrue`/`.alwaysFalse`, and `instanceof.invalidExprType`
//! for a dynamic class-name operand that can't be a string or an object.

use crate::checker::{Checker, Locals};
use syntax::ast::{ClassKind, ExprNode, InstanceofTarget};
use syntax::span::Span;
use types::Type;

impl<'a> Checker<'a> {
    /// `is_exempt_last_condition` is the "last `elseif` with no `else`"
    /// position, where an always-true check acts as an implicit `else`
    /// and isn't reported.
    pub(crate) fn check_impossible_instanceof(
        &mut self,
        inner: &ExprNode,
        target: &InstanceofTarget,
        span: Span,
        locals: &Locals,
        is_exempt_last_condition: bool,
    ) {
        let expr_ty = self.infer_expr(inner, locals);
        if let InstanceofTarget::Expr(target_expr) = target {
            let target_ty = self.infer_expr(target_expr, locals);
            if self.relation().can_name_a_class(&target_ty).no() {
                self.report(
                    format!("Instanceof between {expr_ty} and {target_ty} results in an error."),
                    span,
                    0,
                    "instanceof.invalidExprType",
                );
                return;
            }
        }
        let Some((always, class)) = self.instanceof_outcome(&expr_ty, target, locals) else {
            return;
        };
        if always && is_exempt_last_condition {
            return;
        }
        self.report(
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

    /// Whether a value of `expr_ty` is always (`Some(true)`) or never an
    /// instance of `target`, and the class it names; `None` when it only
    /// sometimes is, or the target isn't a known class. A dynamic
    /// `class-string<C>` target, like `static`, names `C` or any subclass
    /// of it, so only "never" carries over, or "always" when `C` is final.
    pub(crate) fn instanceof_outcome(
        &self,
        expr_ty: &Type,
        target: &InstanceofTarget,
        locals: &Locals,
    ) -> Option<(bool, String)> {
        let (class, only_subclass_known) = match target {
            InstanceofTarget::Expr(target_expr) => {
                let target_ty = self.infer_expr(target_expr, locals);
                if let Some(name) = target_ty.string_literal_value() {
                    (name.to_string(), false)
                } else {
                    (target_ty.class_string_class()?.to_string(), true)
                }
            }
            // `static` names the current class or any subclass of it.
            InstanceofTarget::Name(name) if name.eq_ignore_ascii_case("static") => {
                (self.current_class.clone()?, true)
            }
            InstanceofTarget::Name(name) => {
                let resolved = self.resolve_name_expr(name);
                (resolved.as_object()?.0.to_string(), false)
            }
        };
        let (class, class_info) = self.registry.lookup_class(class.trim_start_matches('\\'))?;
        let class_is_final = class_info.is_final() || class_info.kind() == ClassKind::Enum;
        let class = class.to_string();
        let always = self.instanceof_verdict(expr_ty, &class)?;
        if always && only_subclass_known && !class_is_final {
            return None;
        }
        Some((always, class))
    }

    /// Whether a value of type `ty` is always (`Some(true)`), never
    /// (`Some(false)`), or only sometimes (`None`) an instance of the
    /// registered class-like `target`.
    fn instanceof_verdict(&self, ty: &Type, target: &str) -> Option<bool> {
        syntax::stack::guard(|| {
            if ty.is_never() {
                return None;
            }
            if let Some((class, _)) = ty.as_object() {
                return self.class_instanceof_verdict(class, target);
            }
            if let Some(bound) = ty.template_bound() {
                return self.instanceof_verdict(bound, target);
            }
            let members = ty.union_members();
            if members.len() > 1 {
                let mut verdicts = members.iter().map(|m| self.instanceof_verdict(m, target));
                let first = verdicts.next()??;
                return verdicts.all(|v| v == Some(first)).then_some(first);
            }
            let parts = ty.intersection_members();
            if parts.len() > 1 {
                let verdicts: Vec<_> = parts
                    .iter()
                    .filter_map(|m| self.instanceof_verdict(m, target))
                    .collect();
                return if verdicts.contains(&true) {
                    Some(true)
                } else if verdicts.contains(&false) {
                    Some(false)
                } else {
                    None
                };
            }
            self.relation()
                .can_access_properties(ty)
                .no()
                .then_some(false)
        })
    }

    fn class_instanceof_verdict(&self, class: &str, target: &str) -> Option<bool> {
        let (class, info) = self.registry.lookup_class(class)?;
        let (_, target_info) = self.registry.lookup_class(target)?;
        if self.registry.is_subclass_of(class, target) {
            return Some(true);
        }
        if target_info.kind() == ClassKind::Trait {
            return Some(false);
        }
        if self.registry.is_subclass_of(target, class)
            || self.registry.has_unresolvable_ancestor(class)
        {
            return None;
        }
        let class_is_final = info.is_final() || info.kind() == ClassKind::Enum;
        let target_is_final = target_info.is_final() || target_info.kind() == ClassKind::Enum;
        let both_classes =
            info.kind() == ClassKind::Class && target_info.kind() == ClassKind::Class;
        (class_is_final || target_is_final || both_classes).then_some(false)
    }
}
