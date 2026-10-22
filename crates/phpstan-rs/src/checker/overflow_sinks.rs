//! Where the float an `int` overflow gives is used as an `int`: a typed
//! parameter, return or property throws a `TypeError` for it, and the
//! operations that convert it to an `int` give `PHP_INT_MIN` with a
//! warning. Each operation that can have left the `int` range on the way,
//! and whose operands alone can make it, is reported there.

use std::sync::Arc;

use syntax::ast::{BinOp, Expr, ExprKind};
use syntax::span::Span;
use types::Type;

use crate::checker::flow_nodes::{FlowNode, nodes_of, sources_and_overflows};
use crate::checker::{Checker, Locals};

impl Checker<'_> {
    /// Whether a value of type `target` can't be the float: PHP converts
    /// it to an `int` there, and throws a `TypeError` for one past the
    /// `int` range. Without `strict_types` a float can still become a
    /// `string` or a `bool`.
    pub(crate) fn rejects_overflow_float(&self, target: &Type) -> bool {
        !target.is_int().no()
            && !self.rule_level_accepts(target, &Type::float()).yes()
            && (self.strict_types
                || !target
                    .union_members()
                    .iter()
                    .any(|m| m.is_string().yes() || m.is_bool().yes()))
    }

    /// Reports `value`'s overflow at `span`, where `sink` says what PHP
    /// does with it.
    pub(crate) fn report_overflow_into(
        &mut self,
        value: &Type,
        span: Span,
        sink: impl FnOnce() -> String,
    ) {
        let flow = value.overflow_float_flow();
        if flow.is_empty() {
            return;
        }
        let (sources, overflows) = sources_and_overflows(&flow);
        let mut sites: Vec<(u32, Arc<str>)> = nodes_of(&overflows)
            .into_iter()
            .filter_map(|node| match node {
                FlowNode::Overflow {
                    op,
                    line,
                    real: true,
                    ..
                } => Some((line, op)),
                _ => None,
            })
            .collect();
        if sites.is_empty() {
            return;
        }
        sites.sort();
        sites.dedup();
        let mut environment: Vec<Arc<str>> = nodes_of(&sources)
            .into_iter()
            .filter_map(|node| match node {
                FlowNode::Env(name) => Some(name),
                _ => None,
            })
            .collect();
        environment.sort();
        environment.dedup();
        let operations = sites
            .iter()
            .map(|(line, op)| format!("{op} on line {line}"))
            .collect::<Vec<_>>()
            .join(", ");
        let from = if environment.is_empty() {
            String::new()
        } else {
            format!(" with a value from {}", environment.join(", "))
        };
        self.report(
            format!(
                "The result of {operations} can leave the int range{from}, and {}.",
                sink()
            ),
            span,
            5,
            "arithmetic.intOverflow",
        );
    }

    /// Reports `value`'s overflow where `what` converts it to an `int`.
    pub(crate) fn report_overflow_converted(&mut self, value: &Type, span: Span, what: &str) {
        self.report_overflow_into(value, span, || {
            format!("{what} converts the float to PHP_INT_MIN with a warning")
        });
    }

    /// Reports the overflow of an argument a builtin converts to an `int`
    /// without a declared `int` parameter: `intval()`'s value, and the
    /// variable `settype()` gives the type `int`. `resolved` is the call's
    /// arguments by parameter.
    pub(crate) fn check_overflow_converting_builtin(
        &mut self,
        function: &str,
        resolved: &[Option<&Expr>],
        locals: &Locals,
    ) {
        let function = function.trim_start_matches('\\');
        let converted = if function.eq_ignore_ascii_case("intval") {
            resolved.first().copied().flatten()
        } else if function.eq_ignore_ascii_case("settype")
            && let Some(Some(type_name)) = resolved.get(1)
            && self
                .infer_expr(type_name, locals)
                .constant_strings_text()
                .iter()
                .any(|t| t.eq_ignore_ascii_case("int") || t.eq_ignore_ascii_case("integer"))
        {
            resolved.first().copied().flatten()
        } else {
            None
        };
        if let Some(arg) = converted {
            let ty = self.infer_expr(arg, locals);
            self.report_overflow_converted(&ty, arg.span, &format!("{function}()"));
        }
    }

    /// Reports `value`'s overflow where it is written to `target`: a
    /// property with a native type and no `set` hook throws a `TypeError`
    /// for it.
    pub(crate) fn check_overflow_into_target(
        &mut self,
        target: &Expr,
        value: &Type,
        span: Span,
        locals: &Locals,
    ) {
        if value.overflow_float_flow().is_empty() {
            return;
        }
        let property = match &target.kind {
            ExprKind::PropertyAccess(base, prop, _) => {
                let base_ty = self.infer_expr(base, locals);
                self.find_property_receiver(&base_ty, prop)
                    .map(|(_, _, owner, sig)| (owner, prop.name.clone(), sig))
            }
            ExprKind::StaticPropertyFetch(class, prop) => self
                .infer_static_target_class(class, locals)
                .and_then(|class| self.registry.lookup_property_with_owner(&class, prop))
                .map(|(owner, sig)| (owner, prop.name.clone(), sig.clone())),
            _ => None,
        };
        let Some((owner, name, sig)) = property else {
            return;
        };
        if sig.has_set_hook()
            || !sig
                .native_ty()
                .is_some_and(|native| self.rejects_overflow_float(native))
        {
            return;
        }
        self.report_overflow_into(value, span, || {
            format!("assigning the float to {owner}::${name} throws a TypeError")
        });
    }
}

/// How the operator `op` is written, when it converts its operands to
/// `int`: `%`, the bitwise operators and the shifts.
pub(crate) fn converting_operator(op: BinOp) -> Option<&'static str> {
    match op {
        BinOp::Mod => Some("`%`"),
        BinOp::BitAnd => Some("`&`"),
        BinOp::BitOr => Some("`|`"),
        BinOp::BitXor => Some("`^`"),
        BinOp::Shl => Some("`<<`"),
        BinOp::Shr => Some("`>>`"),
        _ => None,
    }
}
