use crate::checker::narrowing::mirror_comparison;
use crate::checker::{Checker, Locals, PregCall, preg_call, preg_result_outcome, preg_result_type};
use crate::regex_groups;
use syntax::ast::*;
use types::{AccessoryKind, PhpVersion, Type};

impl Checker<'_> {
    /// Narrows a preg call's `$matches` and subject when `cond` is the
    /// call itself, by its truthiness.
    pub(super) fn apply_preg_result_narrowing(
        &self,
        cond: &ExprNode,
        locals: &mut Locals,
        branch_true: bool,
    ) {
        let Some(arenas) = self.syntax_arenas() else {
            return;
        };
        if let Some(call) = preg_call(arenas, cond.id) {
            let result = preg_result_type(call.all);
            let result = if branch_true {
                result.without_falsy()
            } else {
                result.only_falsy(&self.relation())
            };
            self.narrow_preg_outcome(&call, &result, locals);
        }
    }

    /// `l op r` was `holds`, where a side is a preg call or a variable
    /// whose condition alias is one: the comparison applied to the
    /// call's result type, as any comparison narrows, so every operator
    /// and operand order works.
    pub(super) fn apply_preg_comparison(
        &self,
        op: &BinOp,
        l: &ExprNode,
        r: &ExprNode,
        locals: &mut Locals,
        holds: bool,
    ) {
        let arenas = self
            .syntax_arenas()
            .expect("condition aliases require the syntax tree owner");
        for (side, other, on_left) in [(l, r, true), (r, l, false)] {
            let (call, result) = if let Some(call) = preg_call(arenas, side.id) {
                let result = preg_result_type(call.all);
                (call, result)
            } else if let Some(ExprNode {
                kind: ExprKind::Variable(name),
                ..
            }) = arenas.peeled(side.id).and_then(|id| arenas.expr(id))
                && let Some((_, stored)) = locals.condition_aliases().get(&locals.var_key(name))
                && let Some(stored) = arenas.expr(*stored)
                && let Some(call) = preg_call(arenas, stored.id)
            {
                (call, self.infer_expr(side, locals))
            } else {
                continue;
            };
            let other = self.infer_expr(other, locals);
            let op = if on_left { *op } else { mirror_comparison(op) };
            if let Some(result) = self.compared_result(&result, &op, &other, holds) {
                self.narrow_preg_outcome(&call, &result, locals);
            }
        }
    }

    /// `result` where `result op other` is `holds`.
    fn compared_result(
        &self,
        result: &Type,
        op: &BinOp,
        other: &Type,
        holds: bool,
    ) -> Option<Type> {
        let version = self.comparison_php_version();
        Some(match op {
            BinOp::Identical | BinOp::NotIdentical => {
                if matches!(op, BinOp::Identical) == holds {
                    self.relation().narrow(result, other)
                } else if other.is_single_value() {
                    self.exclude(result, other)
                } else {
                    result.clone()
                }
            }
            BinOp::Eq | BinOp::NotEq => self.relation().narrow_loose(
                result,
                other,
                matches!(op, BinOp::Eq) == holds,
                version,
            ),
            _ => {
                let ordering = crate::checker::operators::ordering_op(op)?;
                self.relation()
                    .narrow_ordering(result, ordering, other, holds, version)
            }
        })
    }

    /// `$matches` as the call leaves it for `result`, and, when the call
    /// surely matched, its subject: a `non-empty-string` for a pattern
    /// whose match is never empty, and a numeric one when the match is
    /// the whole subject and holds only digits (a final newline the end
    /// anchor allows is numeric from PHP 8 on).
    pub(super) fn narrow_preg_outcome(
        &self,
        call: &PregCall<'_>,
        result: &Type,
        locals: &mut Locals,
    ) {
        let Some(outcome) = preg_result_outcome(result) else {
            return;
        };
        if let Some(matches) = call.matches
            && let Some(key) = self.key_of_expr(locals, matches)
            && let Some(ty) = self.preg_matches_type(call, outcome, locals)
        {
            locals.insert(key, ty);
        }
        if !outcome.matched() {
            return;
        }
        let Some(subject) = call.subject else {
            return;
        };
        let Some(key) = self.key_of_expr(locals, subject) else {
            return;
        };
        let Some(parsed) = self
            .constant_string_value(call.pattern, locals)
            .and_then(|p| regex_groups::shared_capture_groups(&p))
        else {
            return;
        };
        if parsed.min_len == 0 {
            return;
        }
        let from_start = call
            .offset
            .is_none_or(|o| self.infer_expr(o, locals).int_literal_value() == Some(0));
        let newline_is_numeric = !parsed.final_newline
            || self
                .registry
                .php_version_min()
                .is_some_and(|v| v >= PhpVersion::new(8, 0));
        let target = Type::string().with_string_accessory(AccessoryKind::NonEmptyString);
        let target =
            if parsed.fully_anchored && parsed.digits_only && from_start && newline_is_numeric {
                target.with_string_accessory(AccessoryKind::NumericString)
            } else {
                target
            };
        let current = self.current_narrowed_type(subject, key, locals);
        locals.insert(key, self.refine_string_members(&current, &target, true));
    }
}
