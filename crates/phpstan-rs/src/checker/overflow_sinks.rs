//! Where the float an `int` overflow gives is used as an `int`: a typed
//! parameter, return or property throws a `TypeError` for it, and the
//! operations that convert it to an `int` wrap it into the `int` range
//! with a warning. Without `strict_types` both take a float that is a
//! whole number in the `int` range: PHP computes the overflow in doubles,
//! and one just below `PHP_INT_MIN` rounds to `-2^63`. Each
//! operation that can have left the `int` range on the way is reported
//! there, when what its operands are computed from can make it
//! (`flow_eval`).

use syntax::ast::{BinOp, Expr, ExprKind};
use syntax::span::Span;
use types::Type;

use crate::checker::flow_eval::Graph;
use crate::checker::flow_facts::SinkKind;
use crate::checker::flow_nodes::sources_and_overflows;
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

    /// Records `value`'s overflow at `span`, a typed target, where `sink`
    /// says what PHP does with it; it is reported when one of the
    /// operations its float comes from can leave the `int` range
    /// (`judge_overflow_sinks`).
    pub(crate) fn report_overflow_into(
        &mut self,
        value: &Type,
        span: Span,
        sink: impl FnOnce() -> String,
    ) {
        let every_float = self.strict_types;
        self.record_overflow_float(value, span, every_float, sink);
    }

    /// Records `value`'s overflow where `what` converts it to an `int`.
    pub(crate) fn report_overflow_converted(&mut self, value: &Type, span: Span, what: &str) {
        self.record_overflow_float(value, span, false, || {
            format!("{what} wraps the float into the int range with a warning")
        });
    }

    /// Records `value`'s overflow at `span`; `every_float` when PHP
    /// rejects any float there.
    fn record_overflow_float(
        &mut self,
        value: &Type,
        span: Span,
        every_float: bool,
        sink: impl FnOnce() -> String,
    ) {
        let flow = value.overflow_float_flow();
        if flow.is_empty() {
            return;
        }
        let (_, overflows) = sources_and_overflows(&flow);
        let overflows: Vec<u32> = overflows.nodes().to_vec();
        if overflows.is_empty() {
            return;
        }
        self.record_sink(
            span,
            SinkKind::Float {
                overflows,
                sink: sink(),
                every_float,
            },
        );
    }

    /// Records the overflow of an argument a builtin converts to an `int`
    /// without a declared `int` parameter (`intval()`'s value, and the
    /// variable `settype()` gives the type `int`), and `intdiv()`, which
    /// throws for `PHP_INT_MIN` by `-1`. `resolved` is the call's arguments
    /// by parameter.
    pub(crate) fn check_overflow_in_builtin(
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
        if function.eq_ignore_ascii_case("intdiv")
            && let (Some(Some(dividend)), Some(Some(divisor))) = (resolved.first(), resolved.get(1))
        {
            let dividend_ty = self.infer_expr(dividend, locals);
            let divisor_ty = self.infer_expr(divisor, locals);
            let (dividend_int, _) = dividend_ty.split_overflow_float();
            if intdiv_can_overflow(&dividend_int, &divisor_ty) {
                let kind = SinkKind::IntDiv {
                    dividend: self.value_fact(&dividend_int),
                    divisor: self.value_fact(&divisor_ty),
                };
                self.record_sink(dividend.span.to(divisor.span), kind);
            }
        }
    }

    /// Reports each use of an overflow's float, and each `intdiv()`, this
    /// check recorded, judged from its own facts alone.
    pub(crate) fn judge_overflow_sinks(&mut self) {
        let facts = self.flow_facts.borrow();
        if facts.sinks.is_empty() {
            return;
        }
        let graph = Graph::new(
            self.registry,
            self.settings.platform.loop_iterations,
            std::iter::once(&*facts),
        );
        let growth = self.settings.extras.check_int_overflow_growth;
        let found: Vec<(Span, String, &'static str)> = facts
            .sinks
            .iter()
            .filter_map(|sink| {
                let overflow = graph.judge(sink)?;
                let identifier = overflow.identifier(growth)?;
                Some((sink.span, overflow.message, identifier))
            })
            .collect();
        drop(facts);
        for (span, message, identifier) in found {
            self.report(message, span, 5, identifier);
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

/// Whether `intdiv()` of a `dividend` by a `divisor` can be
/// `PHP_INT_MIN` divided by `-1`, whose result is past the `int` range: an
/// `ArithmeticError`. A value that isn't an `int` is converted, and can
/// be either.
pub(crate) fn intdiv_can_overflow(dividend: &Type, divisor: &Type) -> bool {
    let can_be = |ty: &Type, n: i64| {
        ty.union_members().iter().any(|m| match m.int_bounds() {
            Some((min, max)) => min.is_none_or(|min| min <= n) && max.is_none_or(|max| n <= max),
            None => !m.is_int().no() || m.is_mixed() || !m.is_string().no() || !m.is_float().no(),
        })
    };
    can_be(dividend, i64::MIN) && can_be(divisor, -1)
}
