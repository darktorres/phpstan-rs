//! The data-flow nodes a type's `Flow` names: where a value can come from.
//! One numbering serves every file checked in the process, so the facts
//! files record about the same function or source name the same nodes.

use std::sync::{Arc, LazyLock, Mutex};

use rustc_hash::FxHashMap;
use types::Flow;

/// A place a value comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FlowNode {
    /// The environment: request data, the command line, environment
    /// variables, what the deployment's files and databases hold. The name
    /// says which (`$_GET`, `getenv`).
    Env(Arc<str>),
    /// A parameter, as the callers pass it.
    Param { owner: Owner, name: Arc<str> },
    /// What one call returns, at the site that makes it; `callee` names
    /// what it calls.
    Call { site: Site, callee: Arc<str> },
    /// What a function returns, as its `return` statements give it.
    Return(Owner),
    /// A value an operation computes from other values (arithmetic, a
    /// string built from parts, a builtin that transforms its argument):
    /// what it is computed from is recorded where it is computed
    /// (`flow_facts::DerivedFact`).
    Derived { site: Site, op: Arc<str>, line: u32 },
    /// A property of a class, as the writes to it leave it: of every
    /// object of the class, for a read whose object isn't known.
    Property { class: Arc<str>, name: Arc<str> },
    /// A read of the property `name` at a site: what the writes to the
    /// objects its receiver can be leave there (`flow_facts::FieldRead`).
    Field {
        site: Site,
        name: Arc<str>,
        line: u32,
    },
    /// The object `new` makes at a site.
    Alloc { site: Site, line: u32 },
    /// `$this` in a method: the receiver of its calls.
    This(Owner),
    /// A global variable, as the code that writes it leaves it.
    Global(Arc<str>),
    /// A static variable, as the earlier calls leave it.
    Static { owner: Owner, name: Arc<str> },
    /// An operation whose `int` result can leave the `int` range, carried
    /// by the float it gives there, and how that float got here. Whether
    /// it can is judged from what its operands are computed from
    /// (`flow_eval`), as the `Derived` node of the same site records it.
    Overflow {
        site: Site,
        op: Arc<str>,
        line: u32,
        carry: Carry,
    },
}

/// How the float of an overflow reaches a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Carry {
    /// As its operation gave it.
    Direct,
    /// Computed by `step` from the float of the `Overflow` node `from` and
    /// an `int` within `with`, as PHP computes it, in doubles.
    Step {
        step: Step,
        with: (i64, i64),
        from: u32,
    },
    /// Computed from by more steps than are followed, or by one that
    /// isn't.
    Changed,
}

/// An operation the float of an overflow goes through (`Carry::Step`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Step {
    /// The float plus the `int`.
    Add,
    /// The float minus the `int`.
    Sub,
    /// The `int` minus the float.
    SubFrom,
    /// The float times the `int`.
    Mul,
    /// The float divided by the `int`.
    Div,
    Neg,
    Abs,
    /// `max()` beside `int`s at least the `with`'s least: the float only
    /// where it is above it.
    Max,
    /// `min()` beside `int`s at most the `with`'s greatest: the float only
    /// where it is below it.
    Min,
}

/// How many steps of an overflow's float are followed (`Carry::Step`).
pub(crate) const MOST_STEPS: usize = 4;

/// How many steps the float of the `Overflow` node `overflow` went
/// through.
pub(crate) fn steps(mut overflow: u32) -> usize {
    let mut count = 0;
    while let Some(FlowNode::Overflow {
        carry: Carry::Step { from, .. },
        ..
    }) = node(overflow)
    {
        count += 1;
        overflow = from;
    }
    count
}

/// The function a parameter or a static variable belongs to: a function
/// by its qualified name, a method as `Class::method`, a closure by where
/// it is written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Owner {
    Named(Arc<str>),
    Closure(Site),
}

/// A place in the code: a file and an offset in it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Site {
    pub(crate) file: Arc<str>,
    pub(crate) offset: u32,
}

impl FlowNode {
    pub(crate) fn env(name: &str) -> FlowNode {
        FlowNode::Env(name.into())
    }

    /// How `assertFlow` writes the node.
    fn describe(&self) -> String {
        match self {
            FlowNode::Env(name) => format!("env:{name}"),
            FlowNode::Param { owner, name } => format!("param:{owner}::${name}"),
            FlowNode::Call { callee, .. } => format!("call:{callee}"),
            FlowNode::Return(owner) => format!("return:{owner}"),
            FlowNode::Derived { op, line, .. } => format!("derived:{op}@{line}"),
            FlowNode::Property { class, name } => format!("property:{class}::${name}"),
            FlowNode::Field { name, line, .. } => format!("field:->{name}@{line}"),
            FlowNode::Alloc { line, .. } => format!("new@{line}"),
            FlowNode::This(owner) => format!("this:{owner}"),
            FlowNode::Global(name) => format!("global:${name}"),
            FlowNode::Static { owner, name } => format!("static:{owner}::${name}"),
            FlowNode::Overflow {
                op, line, carry, ..
            } => match carry {
                Carry::Direct => format!("overflow:{op}@{line}"),
                Carry::Step { from, .. } => {
                    format!("overflow:{op}@{line} then {} steps", steps(*from) + 1)
                }
                Carry::Changed => format!("overflow:{op}@{line} changed"),
            },
        }
    }
}

impl std::fmt::Display for Owner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Owner::Named(name) => f.write_str(name),
            Owner::Closure(_) => f.write_str("{closure}"),
        }
    }
}

#[derive(Default)]
struct Table {
    ids: FxHashMap<FlowNode, u32>,
    nodes: Vec<FlowNode>,
}

static TABLE: LazyLock<Mutex<Table>> = LazyLock::new(Mutex::default);

fn table() -> std::sync::MutexGuard<'static, Table> {
    TABLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The number of `node`.
pub(crate) fn id(node: FlowNode) -> u32 {
    let mut table = table();
    if let Some(&id) = table.ids.get(&node) {
        return id;
    }
    let id = u32::try_from(table.nodes.len()).expect("fewer than 2^32 data-flow nodes");
    table.nodes.push(node.clone());
    table.ids.insert(node, id);
    id
}

/// The node numbered `id`.
pub(crate) fn node(id: u32) -> Option<FlowNode> {
    table().nodes.get(id as usize).cloned()
}

/// `flow` split into where its value comes from and the operations that
/// can have left the `int` range on the way.
pub(crate) fn sources_and_overflows(flow: &Flow) -> (Flow, Flow) {
    if flow.is_empty() {
        return (Flow::default(), Flow::default());
    }
    let table = table();
    let (overflows, sources): (Vec<u32>, Vec<u32>) = flow.nodes().iter().partition(|&&id| {
        matches!(
            table.nodes.get(id as usize),
            Some(FlowNode::Overflow { .. })
        )
    });
    (Flow::of(sources), Flow::of(overflows))
}

/// The set of `node` alone.
pub(crate) fn flow_of(node: FlowNode) -> Flow {
    Flow::of([id(node)])
}

/// `flow`'s nodes as `assertFlow` writes them: sorted, comma-separated.
pub(crate) fn describe(flow: &Flow) -> String {
    let mut names: Vec<String> = flow
        .nodes()
        .iter()
        .filter_map(|&id| node(id))
        .map(|n| n.describe())
        .collect();
    names.sort();
    names.join(", ")
}
