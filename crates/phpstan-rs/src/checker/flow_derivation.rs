//! Where a derived value comes from: an expression that computes its value
//! from its operands' values (arithmetic, a cast, a string built from
//! parts, a builtin that passes its argument's value through) comes from
//! wherever they came from. Its type is given their data-flow sets here,
//! where the checker infers it, so no type operation has to.
//!
//! An operation whose `int` result can leave the `int` range gives the
//! float PHP gives there a node of its own (`FlowNode::Overflow`), which
//! goes on with that float, and only with it, to where it is used.

use syntax::ast::*;
use syntax::span::Span;
use types::{ArithmeticOp, Flow, Type};

use crate::checker::flow_nodes::{FlowNode, Owner, Site, flow_of, nodes_of, sources_and_overflows};
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
    /// derives its value from, or of the place it reads it from.
    pub(crate) fn with_derived_flow(&self, e: &Expr, ty: Type, locals: &Locals) -> Type {
        let flow = self.derived_flow(e, &ty, locals);
        let overflow = self.overflow_site(e, &ty, locals);
        with_flows(ty, &flow, &overflow)
    }

    /// `after`, the value `$x++`/`$x--` at `span` leaves from `before`,
    /// with where `before` came from, and the step as an operation that can
    /// leave the `int` range.
    pub(crate) fn with_step_flow(&self, span: Span, op: &str, before: &Type, after: Type) -> Type {
        let (rest, overflowed) = before.split_overflow_float();
        let overflow = if !overflowed && after.split_overflow_float().1 {
            self.overflow_node(span, op, &[rest])
        } else {
            Flow::default()
        };
        with_flows(after, &before.deep_flow(), &overflow)
    }

    /// The node of the operation `e`, when its result can leave the `int`
    /// range and its operands' `int` values alone can make it.
    fn overflow_site(&self, e: &Expr, ty: &Type, locals: &Locals) -> Flow {
        if !ty.split_overflow_float().1 {
            return Flow::default();
        }
        let operand = |operand: &Expr| self.infer_expr(operand, locals).split_overflow_float();
        match &e.kind {
            ExprKind::Binary(op, l, r) | ExprKind::CompoundAssign(op, l, r) => {
                let Some((arithmetic, symbol)) = overflowing_op(*op) else {
                    return Flow::default();
                };
                let ((l, l_overflowed), (r, r_overflowed)) = (operand(l), operand(r));
                let fresh = !(l_overflowed || r_overflowed)
                    || Type::binary_operation(arithmetic, &l, &r)
                        .split_overflow_float()
                        .1;
                if fresh {
                    self.overflow_node(e.span, symbol, &[l, r])
                } else {
                    Flow::default()
                }
            }
            ExprKind::Unary(UnaryOp::Neg, inner) => match operand(inner) {
                (inner, false) => self.overflow_node(e.span, "-", &[inner]),
                (inner, true) if inner.clone().negate().split_overflow_float().1 => {
                    self.overflow_node(e.span, "-", &[inner])
                }
                _ => Flow::default(),
            },
            ExprKind::Call(callee, args)
                if matches!(&callee.kind, ExprKind::Name(name)
                if self.registry.resolve_function(name).is_some_and(|(resolved, sig)| {
                    sig.is_stub() && resolved.trim_start_matches('\\').eq_ignore_ascii_case("abs")
                })) =>
            {
                match args.first().map(|arg| operand(&arg.value)) {
                    Some((inner, false)) => self.overflow_node(e.span, "abs()", &[inner]),
                    _ => Flow::default(),
                }
            }
            _ => Flow::default(),
        }
    }

    /// The node of the operation `op` at `span` over `operands`.
    fn overflow_node(&self, span: Span, op: &str, operands: &[Type]) -> Flow {
        flow_of(FlowNode::Overflow {
            site: self.flow_site(span),
            op: op.into(),
            line: self.line_index.line_col(span.start).line,
            real: operands.iter().all(magnitude_known),
        })
    }

    fn derived_flow(&self, e: &Expr, ty: &Type, locals: &Locals) -> Flow {
        let of = |operand: &Expr| self.infer_expr(operand, locals).deep_flow();
        let of_args = |args: &[Arg]| {
            args.iter()
                .fold(Flow::default(), |acc, arg| acc.union(&of(&arg.value)))
        };
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
                ExprKind::Name(name) => match self.registry.resolve_function(name) {
                    Some(_) if self.passes_values_through(name) => of_args(args),
                    Some((resolved, sig)) if self.is_source_function(resolved, sig.is_stub()) => {
                        flow_of(FlowNode::Env(
                            format!("{}()", resolved.trim_start_matches('\\')).into(),
                        ))
                    }
                    Some((_, sig)) if sig.is_stub() => Flow::default(),
                    Some((resolved, _)) => {
                        self.call_flow(e.span, format!("{}()", resolved.trim_start_matches('\\')))
                    }
                    None => self.call_flow(e.span, format!("{name}()")),
                },
                _ => self.call_flow(e.span, "{dynamic}()".to_string()),
            },
            ExprKind::MethodCall(base, method, ..) => {
                let receiver = self.infer_expr(base, locals);
                let classes = receiver.object_class_names();
                if let Some(source) = classes
                    .iter()
                    .find_map(|class| self.source_method_class(class, &method.name))
                {
                    return flow_of(FlowNode::Env(format!("{source}::{}()", method.name).into()));
                }
                let builtin = !classes.is_empty()
                    && classes.iter().all(|class| {
                        self.registry
                            .lookup_method_with_owner(class, &method.name)
                            .is_some_and(|(owner, _)| self.registry.is_builtin_class(&owner))
                    });
                if builtin {
                    Flow::default()
                } else {
                    self.call_flow(e.span, format!("->{}()", method.name))
                }
            }
            ExprKind::DynamicMethodCall(..) => self.call_flow(e.span, "->{dynamic}()".to_string()),
            ExprKind::StaticCall(target, method, _) => {
                match self.infer_static_target_class(target, locals) {
                    Some(class)
                        if self
                            .registry
                            .lookup_method_with_owner(&class, &method.name)
                            .is_some_and(|(owner, _)| self.registry.is_builtin_class(&owner)) =>
                    {
                        Flow::default()
                    }
                    class => self.call_flow(
                        e.span,
                        format!(
                            "{}::{}()",
                            class.as_deref().unwrap_or("{dynamic}"),
                            method.name
                        ),
                    ),
                }
            }
            ExprKind::DynamicStaticCall(..) => {
                self.call_flow(e.span, "{dynamic}::{dynamic}()".to_string())
            }
            ExprKind::PropertyAccess(base, prop, _) if !ty.has_flow() => self
                .infer_expr(base, locals)
                .object_class_names()
                .iter()
                .filter_map(|class| self.property_flow(class, &prop.name))
                .fold(Flow::default(), |acc, flow| acc.union(&flow)),
            ExprKind::StaticPropertyFetch(target, prop) if !ty.has_flow() => self
                .infer_static_target_class(target, locals)
                .and_then(|class| self.property_flow(&class, &prop.name))
                .unwrap_or_default(),
            _ => Flow::default(),
        }
    }

    /// Where `span` is in the file being checked.
    pub(crate) fn flow_site(&self, span: Span) -> Site {
        let file = self
            .current_file_absolute
            .as_ref()
            .or(self.current_file.as_ref())
            .map(|path| path.to_string_lossy().into())
            .unwrap_or_else(|| "".into());
        Site {
            file,
            offset: span.start,
        }
    }

    /// The set of the parameter `name` of `owner`.
    pub(crate) fn param_flow(&self, owner: Owner, name: &str) -> Flow {
        flow_of(FlowNode::Param {
            owner,
            name: name.into(),
        })
    }

    fn call_flow(&self, span: Span, callee: String) -> Flow {
        flow_of(FlowNode::Call {
            site: self.flow_site(span),
            callee: callee.into(),
        })
    }

    /// The set of the property `name` read from a `class`: the class that
    /// declares it, unless a builtin class does.
    fn property_flow(&self, class: &str, name: &str) -> Option<Flow> {
        let (owner, _) = self.registry.lookup_property_with_owner(class, name)?;
        (!self.registry.is_builtin_class(&owner)).then(|| {
            flow_of(FlowNode::Property {
                class: owner.as_ref().into(),
                name: name.into(),
            })
        })
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

/// `ty` with `flow`'s sources on its scalar parts, and its operations that
/// can leave the `int` range, with `overflow`, on its overflow floats.
fn with_flows(ty: Type, flow: &Flow, overflow: &Flow) -> Type {
    let (sources, carried) = sources_and_overflows(flow);
    let ty = if sources.is_empty() {
        ty
    } else {
        ty.with_flow(&sources)
    };
    ty.with_overflow_float_flow(&carried.union(overflow))
}

/// Whether how large `operand` can be is known where it is used: it comes
/// from the environment, which can make it anything its type allows, or
/// from nowhere else and each of its values is one known value or a
/// bounded range. A value a caller, a property or a call brings, or an
/// unbounded one whose source isn't known, is as large as they make it.
pub(crate) fn magnitude_known(operand: &Type) -> bool {
    let (sources, _) = sources_and_overflows(&operand.deep_flow());
    if !sources.is_empty() {
        return nodes_of(&sources).iter().all(FlowNode::is_env);
    }
    operand.union_members().iter().all(|member| {
        member.is_single_value()
            || member
                .int_bounds()
                .is_some_and(|(min, max)| min.is_some() && max.is_some())
    })
}

/// The arithmetic `op` is, and how it is written, when its `int` result
/// can leave the `int` range: a shift wraps and a division gives a float of
/// its own.
fn overflowing_op(op: BinOp) -> Option<(ArithmeticOp, &'static str)> {
    match op {
        BinOp::Add => Some((ArithmeticOp::Add, "+")),
        BinOp::Sub => Some((ArithmeticOp::Sub, "-")),
        BinOp::Mul => Some((ArithmeticOp::Mul, "*")),
        BinOp::Pow => Some((ArithmeticOp::Pow, "**")),
        _ => None,
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
