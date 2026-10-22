//! Where a derived value comes from: an expression that computes its value
//! from its operands' values (arithmetic, a cast, a string built from
//! parts, a builtin that passes its argument's value through) comes from
//! wherever they came from. Its type is given their data-flow sets here,
//! where the checker infers it, so no type operation has to.

use syntax::ast::*;
use types::{Flow, Type};

use crate::checker::{Checker, Locals};

/// Builtins whose result is made of their arguments' values: converted,
/// trimmed, cut, combined, decoded. One that measures its argument
/// (`strlen`, `count`) is not here: a size is bounded by what the process
/// holds, not chosen by where the value came from.
const PASS_THROUGH_BUILTINS: &[&str] = &[
    "abs",
    "array_product",
    "array_sum",
    "base64_decode",
    "bin2hex",
    "bindec",
    "boolval",
    "ceil",
    "explode",
    "floatval",
    "floor",
    "fmod",
    "hex2bin",
    "hexdec",
    "html_entity_decode",
    "htmlspecialchars_decode",
    "implode",
    "intdiv",
    "intval",
    "join",
    "json_decode",
    "lcfirst",
    "ltrim",
    "max",
    "min",
    "octdec",
    "rawurldecode",
    "round",
    "rtrim",
    "sprintf",
    "str_pad",
    "str_repeat",
    "str_replace",
    "str_split",
    "strrev",
    "strtolower",
    "strtoupper",
    "strval",
    "substr",
    "trim",
    "ucfirst",
    "ucwords",
    "urldecode",
    "vsprintf",
];

impl Checker<'_> {
    /// `ty`, the type of `e`, with the data-flow sets of the operands `e`
    /// derives its value from.
    pub(crate) fn with_derived_flow(&self, e: &Expr, ty: Type, locals: &Locals) -> Type {
        let flow = self.derived_flow(e, locals);
        if flow.is_empty() {
            ty
        } else {
            ty.with_flow(&flow)
        }
    }

    fn derived_flow(&self, e: &Expr, locals: &Locals) -> Flow {
        let of = |operand: &Expr| self.infer_expr(operand, locals).deep_flow();
        match &e.kind {
            ExprKind::Binary(op, l, r) if derives(*op) => of(l).union(&of(r)),
            ExprKind::Unary(op, inner) if *op != UnaryOp::Not => of(inner),
            ExprKind::Cast(spelling, inner)
                if !matches!(
                    spelling,
                    CastSpelling::Bool
                        | CastSpelling::Boolean
                        | CastSpelling::Unset
                        | CastSpelling::Void
                        | CastSpelling::Object
                ) =>
            {
                of(inner)
            }
            ExprKind::Interpolated(parts) => parts
                .iter()
                .fold(Flow::default(), |acc, part| acc.union(&of(part))),
            ExprKind::CompoundAssign(op, target, value) if derives(*op) => {
                of(target).union(&of(value))
            }
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) if self.passes_values_through(name) => args
                    .iter()
                    .fold(Flow::default(), |acc, arg| acc.union(&of(&arg.value))),
                _ => Flow::default(),
            },
            _ => Flow::default(),
        }
    }

    /// Whether `name` calls a builtin whose result is made of its
    /// arguments' values.
    fn passes_values_through(&self, name: &QualifiedName) -> bool {
        let Some((resolved, sig)) = self.registry.resolve_function(name) else {
            return false;
        };
        sig.is_stub()
            && PASS_THROUGH_BUILTINS
                .binary_search(
                    &resolved
                        .trim_start_matches('\\')
                        .to_ascii_lowercase()
                        .as_str(),
                )
                .is_ok()
    }
}

/// Whether `op` computes its value from its operands' values: arithmetic,
/// bitwise, a shift, concatenation. A comparison gives a `bool`, and `??`
/// one of its operands, which keeps its own set.
fn derives(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::Concat
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
    )
}
