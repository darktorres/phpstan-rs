//! What a file's check records of where values go, for judging whether an
//! operation can leave the `int` range (`flow_eval`): what each derived
//! value is computed from, what each call passes and to what, what each
//! write, `return` and default stores, and where the float of an overflow
//! meets a place that takes only an `int`. One file's facts judge what that
//! file alone shows; a project joins every file's.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use syntax::span::Span;
use types::{Flow, Type};

use crate::checker::Checker;
use crate::checker::flow_nodes::{self, FlowNode, Owner, Site, sources_and_overflows};

/// The least and greatest `int` a value converts to, `None` for an end the
/// `int` range bounds.
pub(crate) type Range = (Option<i64>, Option<i64>);

/// A value where it is used: the `int` range its type gives there (`None`
/// when the type says nothing of it) and where it comes from.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ValueFact {
    pub(crate) range: Option<Range>,
    pub(crate) flow: Flow,
}

impl ValueFact {
    /// Both: what either can be.
    fn join(&self, other: &ValueFact) -> ValueFact {
        ValueFact {
            range: join_ranges(self.range, other.range),
            flow: self.flow.union(&other.flow),
        }
    }
}

/// The range covering both, `None` (no bound known) when either is.
pub(crate) fn join_ranges(a: Option<Range>, b: Option<Range>) -> Option<Range> {
    let ((a_lo, a_hi), (b_lo, b_hi)) = (a?, b?);
    Some((
        a_lo.zip(b_lo).map(|(x, y)| x.min(y)),
        a_hi.zip(b_hi).map(|(x, y)| x.max(y)),
    ))
}

/// How a derived value is computed from its operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DerivedOp {
    Add,
    Sub,
    Mul,
    Pow,
    Neg,
    BitNot,
    Abs,
    /// `/`, whose `int` result is the exact quotient.
    Div,
    IntDiv,
    Mod,
    Shl,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
    Min,
    Max,
    /// `(int)` of a float: the float's integer part, or for one past the
    /// `int` range, that part wrapped into it. Either way it is at most
    /// the largest float below 2^63.
    FloatToInt,
    /// A transformation whose result's size its operands' sizes don't
    /// bound (a string built from parts, a decoded value): only whether
    /// the environment chooses it carries over.
    Opaque,
}

impl DerivedOp {
    /// How the operation is written in a message.
    pub(crate) fn symbol(self) -> &'static str {
        match self {
            DerivedOp::Add => "+",
            DerivedOp::Sub => "-",
            DerivedOp::Mul => "*",
            DerivedOp::Pow => "**",
            DerivedOp::Neg => "-",
            DerivedOp::BitNot => "~",
            DerivedOp::Abs => "abs()",
            DerivedOp::Div => "/",
            DerivedOp::IntDiv => "intdiv()",
            DerivedOp::Mod => "%",
            DerivedOp::Shl => "<<",
            DerivedOp::Shr => ">>",
            DerivedOp::BitAnd => "&",
            DerivedOp::BitOr => "|",
            DerivedOp::BitXor => "^",
            DerivedOp::Min => "min()",
            DerivedOp::Max => "max()",
            DerivedOp::FloatToInt => "(int)",
            DerivedOp::Opaque => "…",
        }
    }
}

/// What a derived value is computed from, as every check of its expression
/// saw it.
#[derive(Clone, Debug)]
pub(crate) struct DerivedFact {
    pub(crate) op: DerivedOp,
    pub(crate) operands: Vec<ValueFact>,
    /// The operands the first check of the expression saw: in a loop, as
    /// they are before its first iteration.
    pub(crate) first: Vec<ValueFact>,
    /// The `int` range the result's type gives.
    pub(crate) result: Option<Range>,
    /// How many times one run of the function can compute it: the
    /// iterations of the loops around it, `None` when unbounded.
    pub(crate) executions: Option<i64>,
    /// The function computing it.
    pub(crate) owner: Arc<str>,
}

impl DerivedFact {
    fn join(&mut self, other: DerivedFact) {
        if self.operands.len() == other.operands.len() {
            for (mine, theirs) in self.operands.iter_mut().zip(&other.operands) {
                *mine = mine.join(theirs);
            }
        } else {
            self.op = DerivedOp::Opaque;
            self.operands.extend(other.operands);
        }
        self.result = join_ranges(self.result, other.result);
        self.executions = match (self.executions, other.executions) {
            (Some(a), Some(b)) => Some(a.max(b)),
            _ => None,
        };
    }
}

/// What a call calls.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CallTarget {
    /// A function, by its declared name.
    Function(Arc<str>),
    /// A method of `class`, the type of the receiver; `dispatch` when an
    /// override in a subclass can be what runs.
    Method {
        class: Arc<str>,
        method: Arc<str>,
        dispatch: bool,
    },
}

/// One call: where it is, what it calls, and what it passes to each
/// parameter, by position. A parameter it passes nothing gets its default.
#[derive(Clone, Debug)]
pub(crate) struct CallFact {
    pub(crate) site: Site,
    pub(crate) target: CallTarget,
    /// The object a method runs on, its `$this`.
    pub(crate) receiver: Option<ValueFact>,
    pub(crate) args: Vec<(usize, ValueFact)>,
    /// From this position on, an unpacked argument passes what it holds.
    pub(crate) unpacked_from: Option<(usize, ValueFact)>,
    /// The function making the call.
    pub(crate) caller: Arc<str>,
}

/// A function's parameters, in order: their nodes, and the position of the
/// variadic one.
#[derive(Clone, Debug)]
pub(crate) struct ParamList {
    pub(crate) class: Option<Arc<str>>,
    pub(crate) method: Arc<str>,
    pub(crate) params: Vec<u32>,
    pub(crate) variadic: Option<usize>,
}

/// A read of a property of an object: the object, and the property's
/// nodes (one per class the object can be).
#[derive(Clone, Debug)]
pub(crate) struct FieldRead {
    pub(crate) receiver: ValueFact,
    pub(crate) properties: Vec<u32>,
    pub(crate) result: Option<Range>,
    /// The function reading it.
    pub(crate) owner: Arc<str>,
}

/// A write to a property of an object: the object (`None` for a default,
/// which every object of the class starts with), the property's node, and
/// what is written.
#[derive(Clone, Debug)]
pub(crate) struct FieldWrite {
    pub(crate) receiver: Option<ValueFact>,
    pub(crate) property: u32,
    pub(crate) value: ValueFact,
    /// The function writing it; `None` for a default.
    pub(crate) owner: Option<Arc<str>>,
}

/// Where the float of an overflow is used as an `int`.
#[derive(Clone, Debug)]
pub(crate) struct SinkFact {
    pub(crate) span: Span,
    pub(crate) kind: SinkKind,
    /// The function the use is in.
    pub(crate) owner: Arc<str>,
}

#[derive(Clone, Debug)]
pub(crate) enum SinkKind {
    /// The float of the operations `overflows` (their `Overflow` nodes)
    /// reaches `sink`, which says what PHP does with it. A `strict_types`
    /// typed target rejects `every_float`; anywhere else a float that is a
    /// whole number in the `int` range converts to that `int`.
    Float {
        overflows: Vec<u32>,
        sink: String,
        every_float: bool,
    },
    /// `intdiv()` of these: `PHP_INT_MIN` by `-1` throws.
    IntDiv {
        dividend: ValueFact,
        divisor: ValueFact,
    },
}

/// Everything a check records.
#[derive(Clone, Debug, Default)]
pub(crate) struct FlowFacts {
    /// By the number of the `Derived` node.
    pub(crate) derived: FxHashMap<u32, DerivedFact>,
    pub(crate) calls: Vec<CallFact>,
    /// What a property, global, static or `return` is given, by its node.
    pub(crate) writes: Vec<(u32, ValueFact)>,
    /// A parameter's default, by its node.
    pub(crate) defaults: Vec<(u32, ValueFact)>,
    /// By the number of the `Field` node.
    pub(crate) field_reads: FxHashMap<u32, FieldRead>,
    pub(crate) field_writes: Vec<FieldWrite>,
    /// The function each `Alloc` node's `new` is in, by the node.
    pub(crate) allocs: FxHashMap<u32, Arc<str>>,
    /// By the key of the function (`owner_key`).
    pub(crate) params: FxHashMap<Arc<str>, ParamList>,
    pub(crate) sinks: Vec<SinkFact>,
}

/// How far the lists of `FlowFacts` reached, to drop what a discarded pass
/// added.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FactsMark {
    calls: usize,
    writes: usize,
    defaults: usize,
    field_writes: usize,
    sinks: usize,
}

impl FlowFacts {
    pub(crate) fn mark(&self) -> FactsMark {
        FactsMark {
            calls: self.calls.len(),
            writes: self.writes.len(),
            defaults: self.defaults.len(),
            field_writes: self.field_writes.len(),
            sinks: self.sinks.len(),
        }
    }

    pub(crate) fn rewind_to(&mut self, mark: FactsMark) {
        self.calls.truncate(mark.calls);
        self.writes.truncate(mark.writes);
        self.defaults.truncate(mark.defaults);
        self.field_writes.truncate(mark.field_writes);
        self.sinks.truncate(mark.sinks);
    }
}

/// The key a function's facts are found by: its name, lowercased, without
/// a leading backslash; a method as `class::method`; a closure by where it
/// is written.
pub(crate) fn owner_key(owner: &Owner) -> Arc<str> {
    match owner {
        Owner::Named(name) => function_key(name),
        Owner::Closure(site) => format!("{{closure}}@{}:{}", site.file, site.offset).into(),
    }
}

/// `owner_key` of the function or method named `name`.
pub(crate) fn function_key(name: &str) -> Arc<str> {
    name.trim_start_matches('\\').to_ascii_lowercase().into()
}

impl Checker<'_> {
    /// `ty` where it is used, as a fact.
    ///
    /// An object takes no part in integer arithmetic (operators either
    /// throw or are overloaded), so it bounds nothing.
    pub(crate) fn value_fact(&self, ty: &Type) -> ValueFact {
        let (int_part, _) = ty.split_overflow_float();
        let range = if int_part.is_object().yes() {
            None
        } else {
            int_part
                .to_integer(self.registry)
                .and_then(|int| int.int_hull())
        };
        let (flow, _) = sources_and_overflows(&ty.deep_flow());
        ValueFact { range, flow }
    }

    /// Records that the `Derived` node `node` is `op` of `operands`, giving
    /// `result`.
    pub(crate) fn record_derived(
        &self,
        node: u32,
        op: DerivedOp,
        operands: Vec<ValueFact>,
        result: &Type,
    ) {
        let fact = DerivedFact {
            op,
            first: operands.clone(),
            operands,
            result: result.split_overflow_float().0.int_hull(),
            executions: self.executions_here(),
            owner: owner_key(&self.current_function_owner),
        };
        let mut facts = self.flow_facts.borrow_mut();
        match facts.derived.get_mut(&node) {
            Some(known) => known.join(fact),
            None => {
                facts.derived.insert(node, fact);
            }
        }
    }

    /// How many times one run of the current function can run the
    /// statement being checked: the product of its loops' iterations.
    fn executions_here(&self) -> Option<i64> {
        self.loop_iteration_bounds
            .iter()
            .try_fold(1_i64, |acc, bound| {
                bound.map(|bound| acc.saturating_mul(bound.max(1)))
            })
    }

    /// Records the call at `span` to `target`, on `receiver`, passing `args`
    /// by position.
    pub(crate) fn record_call(
        &self,
        span: Span,
        target: CallTarget,
        receiver: Option<&Type>,
        args: Vec<(usize, ValueFact)>,
        unpacked_from: Option<(usize, ValueFact)>,
    ) {
        let fact = CallFact {
            site: self.flow_site(span),
            target,
            receiver: receiver.map(|r| self.value_fact(r)),
            args,
            unpacked_from,
            caller: owner_key(&self.current_function_owner),
        };
        self.flow_facts.borrow_mut().calls.push(fact);
    }

    /// Records that `node` (a property, global, static or `return`) is
    /// given `value`.
    pub(crate) fn record_write(&self, node: FlowNode, value: &Type) {
        let fact = self.value_fact(value);
        self.flow_facts
            .borrow_mut()
            .writes
            .push((flow_nodes::id(node), fact));
    }

    /// The `Field` node of the read of the property `name` at `span` from
    /// `receiver`, whose nodes are `properties`, giving `result`.
    pub(crate) fn record_field_read(
        &self,
        span: Span,
        name: &str,
        receiver: &Type,
        properties: Vec<FlowNode>,
        result: &Type,
    ) -> u32 {
        let node = flow_nodes::id(FlowNode::Field {
            site: self.flow_site(span),
            name: name.into(),
            line: self.line_index.line_col(span.start).line,
        });
        let read = FieldRead {
            receiver: self.value_fact(receiver),
            properties: properties.into_iter().map(flow_nodes::id).collect(),
            result: result.split_overflow_float().0.int_hull(),
            owner: owner_key(&self.current_function_owner),
        };
        let mut facts = self.flow_facts.borrow_mut();
        match facts.field_reads.get_mut(&node) {
            Some(known) => {
                known.receiver = known.receiver.join(&read.receiver);
                known.result = join_ranges(known.result, read.result);
                for property in read.properties {
                    if !known.properties.contains(&property) {
                        known.properties.push(property);
                    }
                }
            }
            None => {
                facts.field_reads.insert(node, read);
            }
        }
        node
    }

    /// Records that `value` is written to the property `property` of
    /// `receiver` (of every object of its class, for a default), and to the
    /// property as every object has it.
    pub(crate) fn record_field_write(
        &self,
        receiver: Option<&Type>,
        property: FlowNode,
        value: &Type,
    ) {
        let owner = receiver.map(|_| owner_key(&self.current_function_owner));
        self.push_field_write(receiver, property, value, owner);
    }

    fn push_field_write(
        &self,
        receiver: Option<&Type>,
        property: FlowNode,
        value: &Type,
        owner: Option<Arc<str>>,
    ) {
        let write = FieldWrite {
            receiver: receiver.map(|r| self.value_fact(r)),
            property: flow_nodes::id(property.clone()),
            value: self.value_fact(value),
            owner,
        };
        self.flow_facts.borrow_mut().field_writes.push(write);
        self.record_write(property, value);
    }

    /// Records the parameters of `owner`, in order, with their defaults.
    pub(crate) fn record_params(
        &self,
        owner: &Owner,
        class: Option<&str>,
        method: &str,
        params: &[(&str, Option<Type>, bool)],
    ) {
        let mut nodes = Vec::with_capacity(params.len());
        let mut variadic = None;
        let mut defaults = Vec::new();
        for (i, (name, default, is_variadic)) in params.iter().enumerate() {
            let node = flow_nodes::id(FlowNode::Param {
                owner: owner.clone(),
                name: (*name).into(),
            });
            nodes.push(node);
            if *is_variadic {
                variadic = Some(i);
            }
            if let Some(default) = default {
                defaults.push((node, self.value_fact(default)));
            }
        }
        let mut facts = self.flow_facts.borrow_mut();
        facts.defaults.extend(defaults);
        facts.params.insert(
            owner_key(owner),
            ParamList {
                class: class.map(Into::into),
                method: method.into(),
                params: nodes,
                variadic,
            },
        );
    }

    /// Records that the assignment to `target` writes `value` to a
    /// property, a global or a static: an element written to one writes to
    /// it too.
    pub(crate) fn record_target_write(
        &self,
        target: &syntax::ast::Expr,
        value: &Type,
        locals: &crate::checker::Locals,
    ) {
        use syntax::ast::{Expr, ExprKind, Literal};
        let mut base = target;
        let mut first_key: Option<&Expr> = None;
        while let ExprKind::Index(inner, key) = &base.kind {
            first_key = key.as_deref();
            base = inner;
        }
        let mut nodes: Vec<FlowNode> = Vec::new();
        match &base.kind {
            ExprKind::PropertyAccess(object, prop, _) => {
                let receiver = self.infer_expr(object, locals);
                for class in receiver.object_class_names() {
                    if let Some(property) = self.property_node(class, &prop.name) {
                        self.record_field_write(Some(&receiver), property, value);
                    }
                }
            }
            ExprKind::StaticPropertyFetch(class, prop) => {
                if let Some(class) = self.infer_static_target_class(class, locals) {
                    nodes.extend(self.property_node(&class, &prop.name));
                }
            }
            ExprKind::Variable(name) => {
                let name = name.as_str();
                match name {
                    "GLOBALS" => {
                        nodes.push(FlowNode::Global("GLOBALS".into()));
                        if let Some(Expr {
                            kind: ExprKind::Literal(Literal::String(key)),
                            ..
                        }) = first_key
                            && let Some(key) = key.as_str()
                        {
                            nodes.push(FlowNode::Global(key.into()));
                        }
                    }
                    "_SESSION" => nodes.push(FlowNode::Global("_SESSION".into())),
                    _ => {
                        let global = match &self.global_scope {
                            crate::checker::global_writes::Scope::Top => true,
                            crate::checker::global_writes::Scope::Function(imports) => {
                                imports.contains(name)
                            }
                            crate::checker::global_writes::Scope::Other => false,
                        };
                        if global {
                            nodes.push(FlowNode::Global(name.into()));
                            nodes.push(FlowNode::Global("GLOBALS".into()));
                        } else if self.current_function_statics.contains(name) {
                            nodes.push(FlowNode::Static {
                                owner: self.current_function_owner.clone(),
                                name: name.into(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        for node in nodes {
            self.record_write(node, value);
        }
    }

    /// The node of the property `name` read from a `class`: the class that
    /// declares it, unless a builtin class does.
    pub(crate) fn property_node(&self, class: &str, name: &str) -> Option<FlowNode> {
        let (owner, _) = self.registry.lookup_property_with_owner(class, name)?;
        (!self.registry.is_builtin_class(&owner)).then(|| FlowNode::Property {
            class: owner.as_ref().into(),
            name: name.into(),
        })
    }

    /// Records that the constructor parameter `param` of `class`, of type
    /// `ty`, is written to the property it promotes to.
    pub(crate) fn record_promotion(
        &self,
        class: Option<&str>,
        param: &syntax::ast::Param,
        ty: &Type,
    ) {
        if let (Some(class), Some(_)) = (class, &param.promotion) {
            let constructor = Owner::Named(format!("{class}::__construct").into());
            let this = Type::object(class.into(), Vec::new())
                .with_flow(&flow_nodes::flow_of(FlowNode::This(constructor.clone())));
            self.push_field_write(
                Some(&this),
                FlowNode::Property {
                    class: class.into(),
                    name: param.name.as_ref().into(),
                },
                ty,
                Some(owner_key(&constructor)),
            );
        }
    }

    /// Records where an overflow's float, or an `intdiv()`, is used.
    pub(crate) fn record_sink(&self, span: Span, kind: SinkKind) {
        let owner = owner_key(&self.current_function_owner);
        let mut facts = self.flow_facts.borrow_mut();
        // A loop's body is checked again until its types settle: each
        // check of a use adds what it saw to the first.
        let earlier = facts.sinks.iter_mut().find(|sink| {
            sink.span == span
                && sink.owner == owner
                && std::mem::discriminant(&sink.kind) == std::mem::discriminant(&kind)
        });
        let Some(earlier) = earlier else {
            facts.sinks.push(SinkFact { span, kind, owner });
            return;
        };
        match (&mut earlier.kind, kind) {
            (
                SinkKind::Float {
                    overflows,
                    every_float,
                    ..
                },
                SinkKind::Float {
                    overflows: more,
                    every_float: also,
                    ..
                },
            ) => {
                overflows.extend(more);
                overflows.sort_unstable();
                overflows.dedup();
                *every_float |= also;
            }
            (
                SinkKind::IntDiv { dividend, divisor },
                SinkKind::IntDiv {
                    dividend: more_dividend,
                    divisor: more_divisor,
                },
            ) => {
                *dividend = dividend.join(&more_dividend);
                *divisor = divisor.join(&more_divisor);
            }
            _ => {}
        }
    }

    /// `flow` with each value computed in this check replaced by what it is
    /// computed from, as far back as this check recorded.
    pub(crate) fn computed_from(&self, flow: &Flow) -> Flow {
        let facts = self.flow_facts.borrow();
        let mut seen: rustc_hash::FxHashSet<u32> = rustc_hash::FxHashSet::default();
        let mut stack: Vec<u32> = flow.nodes().to_vec();
        let mut out: Vec<u32> = Vec::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            if let Some(fact) = facts.derived.get(&node) {
                for operand in fact.operands.iter().chain(&fact.first) {
                    stack.extend(operand.flow.nodes().iter().copied());
                }
            } else if let Some(read) = facts.field_reads.get(&node) {
                out.extend(read.properties.iter().copied());
            } else {
                out.push(node);
            }
        }
        Flow::of(out)
    }

    /// This check's facts.
    pub(crate) fn take_flow_facts(&mut self) -> FlowFacts {
        std::mem::take(&mut *self.flow_facts.borrow_mut())
    }
}
