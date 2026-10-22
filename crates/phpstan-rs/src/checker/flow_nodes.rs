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
    /// A property of a class, as the writes to it leave it.
    Property { class: Arc<str>, name: Arc<str> },
    /// A global variable, as the code that writes it leaves it.
    Global(Arc<str>),
    /// A static variable, as the earlier calls leave it.
    Static { owner: Owner, name: Arc<str> },
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
            FlowNode::Property { class, name } => format!("property:{class}::${name}"),
            FlowNode::Global(name) => format!("global:${name}"),
            FlowNode::Static { owner, name } => format!("static:{owner}::${name}"),
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
