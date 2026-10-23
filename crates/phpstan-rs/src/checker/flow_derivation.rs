//! Where a derived value comes from. An expression that copies, converts or
//! reads a value (a cast, `trim()`, an array read) comes from wherever that
//! value came from; one that computes a value from its operands
//! (arithmetic, a string built from parts) is a node of its own, whose
//! operands are recorded where it is computed. Its type is given these
//! data-flow sets here, where the checker infers it, so no type operation
//! has to.
//!
//! An operation whose `int` result can leave the `int` range gives the
//! float PHP gives there a node of its own (`FlowNode::Overflow`), which
//! goes on with that float, and only with it, to where it is used.

use syntax::ast::*;
use syntax::span::Span;
use types::{ArithmeticOp, Flow, Type};

use crate::checker::flow_facts::DerivedOp;
use crate::checker::flow_nodes::{
    self, Carry, FlowNode, MOST_STEPS, Owner, Site, Step, flow_of, sources_and_overflows,
};
use crate::checker::{Checker, Locals};

/// Builtins whose result is their argument's value, converted, trimmed or
/// with its letters' case changed: a number in it stays that number. One
/// that measures its argument (`strlen`, `count`) is in neither list: a size
/// is bounded by what the process holds, not chosen by where the value came
/// from.
const IDENTITY_BUILTINS: &[&str] = &[
    "boolval",
    "ceil",
    "floatval",
    "floor",
    "intval",
    "lcfirst",
    "ltrim",
    "round",
    "rtrim",
    "strtolower",
    "strtoupper",
    "strval",
    "trim",
    "ucfirst",
    "ucwords",
];

/// Builtins whose result is made of their arguments' values in a way their
/// sizes don't bound: cut, combined, decoded, repeated.
const PASS_THROUGH_BUILTINS: &[&str] = &[
    "abs",
    "array_product",
    "array_sum",
    "base64_decode",
    "bin2hex",
    "bindec",
    "explode",
    "fmod",
    "hex2bin",
    "hexdec",
    "html_entity_decode",
    "htmlspecialchars_decode",
    "implode",
    "intdiv",
    "join",
    "json_decode",
    "max",
    "min",
    "octdec",
    "rawurldecode",
    "sprintf",
    "str_pad",
    "str_repeat",
    "str_replace",
    "str_split",
    "strrev",
    "substr",
    "urldecode",
    "vsprintf",
];

impl Checker<'_> {
    /// `ty`, the type of `e`, with where its value comes from: a value `e`
    /// computes from its operands is a `Derived` node of its own, recording
    /// them (`flow_facts`); one it copies, converts or reads keeps the sets
    /// of what it reads.
    pub(crate) fn with_derived_flow(&self, e: &Expr, ty: Type, locals: &Locals) -> Type {
        if let Some((op, symbol, operands)) = self.derivation(e, locals) {
            return self.with_derived_node(e.span, op, symbol, &operands, ty);
        }
        let flow = self.derived_flow(e, &ty, locals);
        with_flows(ty, &flow, &Flow::default())
    }

    /// `after`, the value `$x++`/`$x--` at `span` leaves from `before`.
    pub(crate) fn with_step_flow(&self, span: Span, op: &str, before: &Type, after: Type) -> Type {
        let derived = if op == "--" {
            DerivedOp::Sub
        } else {
            DerivedOp::Add
        };
        self.with_derived_node(
            span,
            derived,
            op,
            &[before.clone(), Type::int_literal(1)],
            after,
        )
    }

    /// `ty`, what `op` (written `symbol`) at `span` computes from
    /// `operands`: the `Derived` node of the site when an operand comes
    /// from somewhere or the result can leave the `int` range, which then
    /// gives its float an `Overflow` node too; the floats of the operations
    /// the operands went through go on with it.
    pub(crate) fn with_derived_node(
        &self,
        span: Span,
        op: DerivedOp,
        symbol: &str,
        operands: &[Type],
        ty: Type,
    ) -> Type {
        let fresh = overflows_here(op, operands, &ty);
        let ty = without_overflow_nodes(ty);
        let mut sourced = false;
        let mut carried = Flow::default();
        for (i, operand) in operands.iter().enumerate() {
            let (sources, overflows) = sources_and_overflows(&operand.deep_flow());
            sourced |= !sources.is_empty();
            carried = carried.union(&carried_through(op, i, operands, &overflows));
        }
        if !sourced && !fresh {
            return with_flows(ty, &Flow::default(), &carried);
        }
        let site = self.flow_site(span);
        let line = self.line_index.line_col(span.start).line;
        let node = flow_nodes::id(FlowNode::Derived {
            site: site.clone(),
            op: symbol.into(),
            line,
        });
        self.record_derived(
            node,
            op,
            operands.iter().map(|o| self.value_fact(o)).collect(),
            &ty,
        );
        if fresh {
            carried = carried.union(&flow_of(FlowNode::Overflow {
                site,
                op: symbol.into(),
                line,
                carry: Carry::Direct,
            }));
        }
        let flow = if sourced {
            Flow::of([node])
        } else {
            Flow::default()
        };
        with_flows(ty, &flow, &carried)
    }

    /// How `e` computes its value from its operands, when it isn't a copy,
    /// a conversion or a read of one: the operation, how it is written, and
    /// the operands' types. An assignment's own operation is derived where
    /// it is checked, from the operands as they were before it.
    fn derivation(
        &self,
        e: &Expr,
        locals: &Locals,
    ) -> Option<(DerivedOp, &'static str, Vec<Type>)> {
        let of = |operand: &Expr| self.infer_expr(operand, locals);
        match &e.kind {
            ExprKind::Binary(op, l, r) => {
                let derived = binary_derived_op(*op)?;
                Some((derived, binary_symbol(*op), vec![of(l), of(r)]))
            }
            ExprKind::Unary(UnaryOp::Neg, inner) => Some((DerivedOp::Neg, "-", vec![of(inner)])),
            ExprKind::Unary(UnaryOp::BitNot, inner) => {
                Some((DerivedOp::BitNot, "~", vec![of(inner)]))
            }
            ExprKind::Cast(CastSpelling::Int | CastSpelling::Integer, inner)
                if of(inner).is_float().yes() =>
            {
                Some((DerivedOp::FloatToInt, "(int)", vec![of(inner)]))
            }
            ExprKind::Interpolated(parts) => {
                Some((DerivedOp::Opaque, "\"…\"", parts.iter().map(of).collect()))
            }
            ExprKind::Call(callee, args) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return None;
                };
                let (resolved, sig) = self.registry.resolve_function(name)?;
                if !sig.is_stub() {
                    return None;
                }
                let function = resolved.trim_start_matches('\\').to_ascii_lowercase();
                let operands: Vec<Type> = args.iter().map(|arg| of(&arg.value)).collect();
                let spread = args.iter().any(|arg| arg.spread);
                let derived = match function.as_str() {
                    "abs" if !spread => DerivedOp::Abs,
                    "intval"
                        if !spread
                            && args.len() == 1
                            && operands.first().is_some_and(|o| o.is_float().yes()) =>
                    {
                        DerivedOp::FloatToInt
                    }
                    "intdiv" if !spread && args.len() == 2 => DerivedOp::IntDiv,
                    "min" if !spread && args.len() >= 2 => DerivedOp::Min,
                    "max" if !spread && args.len() >= 2 => DerivedOp::Max,
                    _ if IDENTITY_BUILTINS.binary_search(&function.as_str()).is_ok() => {
                        return None;
                    }
                    _ if PASS_THROUGH_BUILTINS
                        .binary_search(&function.as_str())
                        .is_ok() =>
                    {
                        DerivedOp::Opaque
                    }
                    _ => return None,
                };
                let symbol = match derived {
                    DerivedOp::Opaque => "…()",
                    other => other.symbol(),
                };
                Some((derived, symbol, operands))
            }
            _ => None,
        }
    }

    fn derived_flow(&self, e: &Expr, ty: &Type, locals: &Locals) -> Flow {
        let of = |operand: &Expr| self.infer_expr(operand, locals).deep_flow();
        let of_args = |args: &[Arg]| {
            args.iter()
                .fold(Flow::default(), |acc, arg| acc.union(&of(&arg.value)))
        };
        match &e.kind {
            ExprKind::Unary(
                UnaryOp::Plus
                | UnaryOp::PreInc
                | UnaryOp::PreDec
                | UnaryOp::PostInc
                | UnaryOp::PostDec,
                inner,
            ) => of(inner),
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
            ExprKind::Call(callee, args) => match &callee.kind {
                ExprKind::Name(name) => match self.registry.resolve_function(name) {
                    Some(_) if self.converts_its_argument(name) => of_args(args),
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
            ExprKind::PropertyAccess(base, prop, _) if !ty.has_flow() => {
                let receiver = self.infer_expr(base, locals);
                let properties: Vec<FlowNode> = receiver
                    .object_class_names()
                    .iter()
                    .filter_map(|class| self.property_node(class, &prop.name))
                    .collect();
                if properties.is_empty() {
                    Flow::default()
                } else {
                    Flow::of(
                        [self.record_field_read(e.span, &prop.name, &receiver, properties, ty)],
                    )
                }
            }
            ExprKind::New(..) | ExprKind::NewAnonymousClass(_) => self.alloc_flow(e.span),
            ExprKind::StaticPropertyFetch(target, prop) if !ty.has_flow() => self
                .infer_static_target_class(target, locals)
                .and_then(|class| self.property_flow(&class, &prop.name))
                .unwrap_or_default(),
            _ => Flow::default(),
        }
    }

    /// Where `span` is in the file being checked.
    pub(crate) fn flow_site(&self, span: Span) -> Site {
        Site {
            file: self.flow_file.clone(),
            offset: span.start,
        }
    }

    /// The set of the object `new` makes at `span`.
    pub(crate) fn alloc_flow(&self, span: Span) -> Flow {
        let node = flow_nodes::id(FlowNode::Alloc {
            site: self.flow_site(span),
            line: self.line_index.line_col(span.start).line,
        });
        self.flow_facts.borrow_mut().allocs.insert(
            node,
            crate::checker::flow_facts::owner_key(&self.current_function_owner),
        );
        Flow::of([node])
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

    /// The set of the property `name` read from a `class`.
    fn property_flow(&self, class: &str, name: &str) -> Option<Flow> {
        self.property_node(class, name).map(flow_of)
    }

    /// Whether `name` calls a builtin whose result is its argument's value,
    /// converted or trimmed.
    fn converts_its_argument(&self, name: &QualifiedName) -> bool {
        let Some((resolved, sig)) = self.registry.resolve_function(name) else {
            return false;
        };
        sig.is_stub()
            && IDENTITY_BUILTINS
                .binary_search(
                    &resolved
                        .trim_start_matches('\\')
                        .to_ascii_lowercase()
                        .as_str(),
                )
                .is_ok()
    }
}

/// `ty` without the `Overflow` nodes its floats carry: a result whose type
/// an operation built from its operands' types has theirs, which the
/// operation passes on itself (`carried_through`).
fn without_overflow_nodes(ty: Type) -> Type {
    let (_, overflows) = sources_and_overflows(&ty.overflow_float_flow());
    overflows
        .nodes()
        .iter()
        .fold(ty, |ty, &node| ty.without_flow_node(node))
}

/// The overflow floats of `overflows`, those of operand `i` of `op`, as
/// `op` passes them on: each a step from the float it was, when the
/// other operands are `int`s and the step is one of `Step`'s.
fn carried_through(op: DerivedOp, i: usize, operands: &[Type], overflows: &Flow) -> Flow {
    if overflows.is_empty() {
        return Flow::default();
    }
    let step = step_of(op, i, operands);
    let carried = overflows.nodes().iter().map(|&id| {
        let Some(FlowNode::Overflow {
            site,
            op: symbol,
            line,
            carry,
        }) = flow_nodes::node(id)
        else {
            return id;
        };
        let carry = match (carry, step) {
            (Carry::Changed, _) => Carry::Changed,
            (_, Some(Passed::Unchanged)) => return id,
            (_, Some(Passed::By(step, with))) if flow_nodes::steps(id) < MOST_STEPS => {
                Carry::Step {
                    step,
                    with,
                    from: id,
                }
            }
            _ => Carry::Changed,
        };
        flow_nodes::id(FlowNode::Overflow {
            site,
            op: symbol,
            line,
            carry,
        })
    });
    Flow::of(carried)
}

/// How a float passes through an operation (`step_of`).
#[derive(Clone, Copy)]
enum Passed {
    /// It is the result where it is, unchanged: a `max()` or `min()` beside
    /// a value that may not be an `int`.
    Unchanged,
    /// By this step, with an `int` within this range.
    By(Step, (i64, i64)),
}

/// How the float of operand `i` of `op` passes through it: `None` when
/// the step isn't followed (`Carry::Changed`).
fn step_of(op: DerivedOp, i: usize, operands: &[Type]) -> Option<Passed> {
    let ints = |ty: &Type| {
        ty.int_hull()
            .map(|(lo, hi)| (lo.unwrap_or(i64::MIN), hi.unwrap_or(i64::MAX)))
    };
    let other = || match operands {
        [a, b] => ints(if i == 0 { b } else { a }),
        _ => None,
    };
    let others = || {
        operands
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .map(|(_, ty)| ints(ty))
    };
    let by = |step, with| Some(Passed::By(step, with));
    match op {
        DerivedOp::Add => by(Step::Add, other()?),
        DerivedOp::Sub if i == 0 => by(Step::Sub, other()?),
        DerivedOp::Sub => by(Step::SubFrom, other()?),
        DerivedOp::Mul => by(Step::Mul, other()?),
        DerivedOp::Div if i == 0 => by(Step::Div, other()?),
        DerivedOp::Neg => by(Step::Neg, (0, 0)),
        DerivedOp::Abs => by(Step::Abs, (0, 0)),
        DerivedOp::Max => match others().flatten().map(|(lo, _)| lo).max() {
            Some(least) => by(Step::Max, (least, least)),
            None => Some(Passed::Unchanged),
        },
        DerivedOp::Min => match others().flatten().map(|(_, hi)| hi).min() {
            Some(most) => by(Step::Min, (most, most)),
            None => Some(Passed::Unchanged),
        },
        _ => None,
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

/// Whether the float in `ty`, what `op` computes from `operands`, is one
/// this operation gives: its operands' `int` parts alone can leave the
/// `int` range, not only a float an operand already was.
fn overflows_here(op: DerivedOp, operands: &[Type], ty: &Type) -> bool {
    if !ty.split_overflow_float().1 {
        return false;
    }
    let parts: Vec<(Type, bool)> = operands.iter().map(Type::split_overflow_float).collect();
    let none_overflowed = parts.iter().all(|(_, overflowed)| !overflowed);
    let arithmetic = match op {
        DerivedOp::Add => ArithmeticOp::Add,
        DerivedOp::Sub => ArithmeticOp::Sub,
        DerivedOp::Mul => ArithmeticOp::Mul,
        DerivedOp::Pow => ArithmeticOp::Pow,
        DerivedOp::Neg => {
            return none_overflowed
                || parts
                    .first()
                    .is_some_and(|(inner, _)| inner.clone().negate().split_overflow_float().1);
        }
        DerivedOp::Abs => return none_overflowed,
        _ => return false,
    };
    match parts.as_slice() {
        [(l, _), (r, _)] => {
            none_overflowed
                || Type::binary_operation(arithmetic, l, r)
                    .split_overflow_float()
                    .1
        }
        _ => false,
    }
}

/// The derivation of the binary operator `op`, when it computes its
/// value from both operands: arithmetic, bitwise, a shift, concatenation.
/// A comparison gives a `bool`, and `??` one of its operands, which keeps
/// its own set.
pub(crate) fn binary_derived_op(op: BinOp) -> Option<DerivedOp> {
    Some(match op {
        BinOp::Add => DerivedOp::Add,
        BinOp::Sub => DerivedOp::Sub,
        BinOp::Mul => DerivedOp::Mul,
        BinOp::Pow => DerivedOp::Pow,
        BinOp::Div => DerivedOp::Div,
        BinOp::Mod => DerivedOp::Mod,
        BinOp::Shl => DerivedOp::Shl,
        BinOp::Shr => DerivedOp::Shr,
        BinOp::BitAnd => DerivedOp::BitAnd,
        BinOp::BitOr => DerivedOp::BitOr,
        BinOp::BitXor => DerivedOp::BitXor,
        BinOp::Concat => DerivedOp::Opaque,
        _ => return None,
    })
}

/// How the binary operator `op` is written.
pub(crate) fn binary_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Pow => "**",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Concat => ".",
        _ => "?",
    }
}
