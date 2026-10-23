//! Whether an operation can leave the `int` range, judged from what its
//! operands are computed from, as one file's facts or a whole project's
//! show it (`flow_facts`).
//!
//! Every node gets a value: what is known of the `int`s it holds. A value
//! the environment chooses can be anything its type allows where it is
//! used; one computed from others is computed from theirs; a parameter
//! holds what its calls pass, a property, global or static what is written
//! to it, a call what its callee returns. A value nothing known gives is
//! unknown, and an operation on one is not judged: what callers outside
//! the checked code pass is not known. A loop or a cycle of writes that
//! adds the same amounts again and again grows by them as many times as it
//! can run: its loops' iterations, or the process's whole run for one
//! that goes through a property, a global, a static or a call.
//!
//! An operation is then judged in each context a chain of calls gives its
//! function, up to a depth: two parameters take their values from the
//! same call.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::checker::flow_facts::{
    CallFact, CallTarget, DerivedFact, DerivedOp, FieldRead, FieldWrite, FlowFacts, ParamList,
    SinkFact, SinkKind, ValueFact, function_key, owner_key,
};
use crate::checker::flow_nodes::{self, Carry, FlowNode, Step};
use crate::registry::Registry;

const INT_MIN: i128 = i64::MIN as i128;
const INT_MAX: i128 = i64::MAX as i128;
/// How many times a cycle's operations are run before what they still
/// add is carried on for the rest of the times the cycle can run.
const CYCLE_ROUNDS: usize = 128;
/// How many times they are run after that at most, for an operation that
/// takes back part of it (`%`, `>>`, `&`).
const NARROWING_ROUNDS: usize = 128;
/// The largest `int` a float converts to: the largest float below 2^63.
const LARGEST_INT_OF_FLOAT: i128 = (1 << 63) - (1 << 10);

/// How many calls deep the contexts of an operation go.
const CONTEXT_DEPTH: usize = 4;

/// How many contexts of one operation are tried before the joined values
/// of every caller decide.
const CONTEXT_BUDGET: usize = 512;

/// What is known of the `int`s a value holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    /// Nothing: where it comes from isn't known, or nothing reaches it.
    Unknown,
    /// Within this range (as `int`s).
    Within(i128, i128),
    /// Anything its type allows: the environment chooses it.
    Chosen,
}

impl Value {
    fn join(self, other: Value) -> Value {
        match (self, other) {
            (Value::Unknown, v) | (v, Value::Unknown) => v,
            (Value::Chosen, _) | (_, Value::Chosen) => Value::Chosen,
            (Value::Within(a, b), Value::Within(c, d)) => Value::Within(a.min(c), b.max(d)),
        }
    }

    /// The value where its type gives it `range`.
    fn within(self, range: Option<(Option<i64>, Option<i64>)>) -> Value {
        let Some((lo, hi)) = range else {
            return self;
        };
        let (lo, hi) = (
            lo.map_or(INT_MIN, i128::from),
            hi.map_or(INT_MAX, i128::from),
        );
        match self {
            Value::Unknown => Value::Unknown,
            Value::Chosen => Value::Within(lo, hi),
            Value::Within(a, b) => {
                let (a, b) = (a.max(lo), b.min(hi));
                if a > b {
                    Value::Unknown
                } else {
                    Value::Within(a, b)
                }
            }
        }
    }

    /// The `int`s an operation reads it as: anything for a chosen value.
    fn interval(self) -> Option<(i128, i128)> {
        match self {
            Value::Unknown => None,
            Value::Within(a, b) => Some((a, b)),
            Value::Chosen => Some((INT_MIN, INT_MAX)),
        }
    }

    /// What both values hold.
    fn meet(self, other: Value) -> Value {
        match (self, other) {
            (Value::Unknown, _) | (_, Value::Unknown) => Value::Unknown,
            (Value::Chosen, v) | (v, Value::Chosen) => v,
            (Value::Within(a, b), Value::Within(c, d)) => {
                let (lo, hi) = (a.max(c), b.min(d));
                if lo > hi {
                    Value::Unknown
                } else {
                    Value::Within(lo, hi)
                }
            }
        }
    }

    /// The value after `times` more runs that each move its ends as far
    /// as the last one moved them from `before`.
    fn carried_on(self, before: Value, times: i128) -> Value {
        match (self, before) {
            (Value::Within(lo, hi), Value::Within(a, b)) => Value::Within(
                lo.saturating_sub((a - lo).max(0).saturating_mul(times)),
                hi.saturating_add((hi - b).max(0).saturating_mul(times)),
            ),
            (Value::Within(..), Value::Unknown) => Value::Within(INT_MIN, INT_MAX),
            (value, _) => value,
        }
    }

    /// The value with its ends held to the `int` range.
    fn clamped(self) -> Value {
        match self {
            Value::Within(a, b) => {
                Value::Within(a.clamp(INT_MIN, INT_MAX), b.clamp(INT_MIN, INT_MAX))
            }
            other => other,
        }
    }
}

/// The objects a value can be: where each was made, and whether it can be
/// one made where the checked code doesn't show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Objects {
    allocs: Vec<Object>,
    open: bool,
    /// Too many places make it to tell them apart (`MOST_OBJECTS`).
    any: bool,
}

impl Objects {
    fn open() -> Objects {
        Objects {
            allocs: Vec::new(),
            open: true,
            any: false,
        }
    }

    fn join(&mut self, other: &Objects) {
        self.open |= other.open;
        if other.is_any() {
            *self = Objects::any();
            return;
        }
        if other.allocs.is_empty() || self.is_any() {
            return;
        }
        let mut merged = Vec::with_capacity(self.allocs.len() + other.allocs.len());
        let (mut i, mut j) = (0, 0);
        while i < self.allocs.len() && j < other.allocs.len() {
            let (a, b) = (self.allocs[i], other.allocs[j]);
            merged.push(a.min(b));
            i += usize::from(a <= b);
            j += usize::from(b <= a);
        }
        merged.extend_from_slice(&self.allocs[i..]);
        merged.extend_from_slice(&other.allocs[j..]);
        if merged.len() > MOST_OBJECTS {
            *self = Objects::any();
        } else {
            self.allocs = merged;
        }
    }

    /// Any object: too many places make it to tell them apart, so it is
    /// read as an object made where the checked code doesn't show.
    fn any() -> Objects {
        Objects {
            allocs: Vec::new(),
            open: true,
            any: true,
        }
    }

    fn is_any(&self) -> bool {
        self.any
    }

    /// The places that made the object, when they are told apart.
    fn known(&self) -> &[Object] {
        &self.allocs
    }
}

/// How many places can make the objects a value can be before they are no
/// longer told apart.
const MOST_OBJECTS: usize = 32;

/// The methods the facts declare, by lowercase name: each declaring class
/// and the method's key.
type MethodIndex = FxHashMap<Arc<str>, Vec<(Arc<str>, Arc<str>)>>;

/// The context of a node that no function owns, or of a node of a function
/// nothing in the checked code calls.
const NO_CALL: u32 = u32::MAX;
/// Every call of the node's function at once.
const EVERY_CALL: u32 = u32::MAX - 1;

/// What the graph gives a value: a node in one context, the call of its
/// function it runs in (`NO_CALL`, `EVERY_CALL`, or the call's number), or
/// a property of one object (`Graph::cells`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Key {
    Node(u32, u32),
    Cell(u32),
}

/// An object: the `Alloc` node of the `new` that made it, and the call of
/// the function that `new` is in (`object`), or one made where the
/// checked code doesn't show (`OPEN_OBJECT`).
type Object = u64;
const OPEN_OBJECT: Object = u64::MAX;

fn object(alloc: u32, call: u32) -> Object {
    (u64::from(alloc) << 32) | u64::from(call)
}

/// What a value is read from: a fact as a function sees it in one of its
/// contexts (no function, for a fact every owned node of which is read
/// joined over its calls), or a key.
#[derive(Clone, Copy)]
enum Input<'f> {
    Fact(&'f ValueFact, Option<u32>, u32),
    Key(Key),
}

/// A call in a chain of calls: the context of the function it runs (a
/// call, or a call paired with its caller's context), and the function.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Frame {
    context: u32,
    callee: u32,
}

/// How deep the contexts paired with their caller's go
/// (`Graph::paired`).
const MOST_PAIRED: usize = 3;

/// How many paired contexts a function gets; calls past them run in the
/// context joined over their caller's.
const MOST_CONTEXTS: usize = 64;

/// Every file's facts, joined, and the values of their nodes. A node of a
/// function has a value in each call of the function (its context): a
/// parameter holds what that call passes, joined over the calls of the
/// caller; an object a `new` makes is one object for each call of the
/// function the `new` is in; a call's result is what its callee returns
/// in that call.
pub(crate) struct Graph<'r, 'f> {
    registry: &'r Registry,
    run_total: Option<i64>,
    nodes: FxHashMap<u32, FlowNode>,
    derived: FxHashMap<u32, DerivedFact>,
    writes: FxHashMap<u32, Vec<&'f ValueFact>>,
    defaults: FxHashMap<u32, Vec<&'f ValueFact>>,
    params: FxHashMap<Arc<str>, ParamList>,
    /// The functions, by number, and their numbers.
    owners: Vec<Arc<str>>,
    owner_ids: FxHashMap<Arc<str>, u32>,
    /// The function each node of a function is in.
    node_owner: FxHashMap<u32, u32>,
    /// The contexts of each function, or `NO_CALL` alone: a call of it,
    /// or a call paired with a context of its caller (`paired`).
    contexts: Vec<Vec<u32>>,
    /// The contexts numbered after the calls: a call on a new object or
    /// on `$this`, in one context of the function making it. The object
    /// is one of that context's, so the call runs with what that context
    /// passes, not with every context's.
    paired: Vec<(usize, u32)>,
    paired_ids: FxHashMap<(usize, u32), u32>,
    /// A parameter's function and position.
    param_of: FxHashMap<u32, (u32, usize)>,
    /// A function's `Return` node.
    returns: FxHashMap<u32, u32>,
    calls: Vec<&'f CallFact>,
    /// The function making each call.
    callers: Vec<u32>,
    /// What each call can run.
    targets: Vec<Vec<u32>>,
    /// The calls made at each call site.
    calls_at: FxHashMap<flow_nodes::Site, Vec<usize>>,
    field_reads: FxHashMap<u32, &'f FieldRead>,
    /// By the property's node.
    field_writes: FxHashMap<u32, Vec<&'f FieldWrite>>,
    /// The bound `x - ((x >> k) << k)` and `x - (((x + c) >> k) << k)` put
    /// on a subtraction, by its `Derived` node: what is left of `x` below
    /// bit `k`.
    low_bits: FxHashMap<u32, (i128, i128)>,
    /// The properties of objects, by number: the object and the
    /// property's node.
    cells: std::cell::RefCell<Vec<(Object, u32)>>,
    cell_ids: std::cell::RefCell<FxHashMap<(Object, u32), u32>>,
    /// The objects each key can be, once worked out (`solve_objects`).
    objects: FxHashMap<Key, Objects>,
    solved: FxHashSet<Key>,
    /// The keys whose objects are worked out from each key's.
    object_readers: FxHashMap<Key, Vec<Key>>,
    object_edges: FxHashSet<(Key, Key)>,
    /// What each key's value is made of.
    deps: FxHashMap<Key, Vec<Key>>,
    /// The keys in a cycle with others.
    cyclic: FxHashSet<Key>,
    base: FxHashMap<Key, Value>,
}

impl<'r, 'f> Graph<'r, 'f> {
    /// The graph of `facts`, with its nodes' values. `run_total` bounds how
    /// many steps a run takes (`PlatformLimits::loop_iterations`).
    pub(crate) fn new(
        registry: &'r Registry,
        run_total: Option<i64>,
        facts: impl IntoIterator<Item = &'f FlowFacts>,
    ) -> Graph<'r, 'f> {
        let mut graph = Graph {
            registry,
            run_total,
            nodes: FxHashMap::default(),
            derived: FxHashMap::default(),
            writes: FxHashMap::default(),
            defaults: FxHashMap::default(),
            params: FxHashMap::default(),
            owners: Vec::new(),
            owner_ids: FxHashMap::default(),
            node_owner: FxHashMap::default(),
            contexts: Vec::new(),
            paired: Vec::new(),
            paired_ids: FxHashMap::default(),
            param_of: FxHashMap::default(),
            returns: FxHashMap::default(),
            calls: Vec::new(),
            callers: Vec::new(),
            targets: Vec::new(),
            calls_at: FxHashMap::default(),
            field_reads: FxHashMap::default(),
            field_writes: FxHashMap::default(),
            low_bits: FxHashMap::default(),
            cells: std::cell::RefCell::default(),
            cell_ids: std::cell::RefCell::default(),
            objects: FxHashMap::default(),
            solved: FxHashSet::default(),
            object_readers: FxHashMap::default(),
            object_edges: FxHashSet::default(),
            deps: FxHashMap::default(),
            cyclic: FxHashSet::default(),
            base: FxHashMap::default(),
        };
        let mut sinks: Vec<&'f SinkFact> = Vec::new();
        let mut allocs: Vec<(u32, &'f Arc<str>)> = Vec::new();
        for facts in facts {
            sinks.extend(&facts.sinks);
            for (&node, fact) in &facts.derived {
                match graph.derived.get_mut(&node) {
                    Some(known) => known.join_from(fact),
                    None => {
                        graph.derived.insert(node, fact.clone());
                    }
                }
            }
            for (node, fact) in &facts.writes {
                graph.writes.entry(*node).or_default().push(fact);
            }
            for (node, fact) in &facts.defaults {
                graph.defaults.entry(*node).or_default().push(fact);
            }
            for (key, list) in &facts.params {
                graph.params.insert(key.clone(), list.clone());
            }
            graph.calls.extend(facts.calls.iter());
            for (&node, read) in &facts.field_reads {
                graph.field_reads.insert(node, read);
            }
            for write in &facts.field_writes {
                graph
                    .field_writes
                    .entry(write.property)
                    .or_default()
                    .push(write);
            }
            allocs.extend(facts.allocs.iter().map(|(&node, owner)| (node, owner)));
        }
        graph.index(&allocs);
        // Only what the uses of overflow floats and the `intdiv()` calls
        // are made of is judged.
        let mut roots: Vec<Key> = Vec::new();
        for sink in &sinks {
            let owner = graph.owner_ids.get(&sink.owner).copied();
            match &sink.kind {
                SinkKind::Float { overflows, .. } => {
                    for derived in overflows.iter().filter_map(|&o| derived_of(o)) {
                        let owner = graph.owner_of(derived);
                        for &call in graph.contexts_of(owner) {
                            roots.push(Key::Node(derived, call));
                        }
                    }
                }
                SinkKind::IntDiv { dividend, divisor } => {
                    for &call in graph.contexts_of(owner) {
                        roots.extend(graph.fact_keys(dividend, owner, call));
                        roots.extend(graph.fact_keys(divisor, owner, call));
                    }
                }
            }
        }
        let mut reached: FxHashSet<Key> = FxHashSet::default();
        while let Some(key) = roots.pop() {
            if !graph.has_key(key) || !reached.insert(key) {
                continue;
            }
            let deps = graph.compute_dependencies(key);
            roots.extend(&deps);
            graph.deps.insert(key, deps);
        }
        let mut keys: Vec<Key> = reached.into_iter().collect();
        keys.sort_unstable();
        graph.settle(&keys);
        graph
    }

    fn node(&self, id: u32) -> Option<&FlowNode> {
        self.nodes.get(&id)
    }

    fn has_key(&self, key: Key) -> bool {
        match key {
            Key::Node(node, _) => self.nodes.contains_key(&node),
            Key::Cell(_) => true,
        }
    }

    fn owner_id(&mut self, key: &Arc<str>) -> u32 {
        if let Some(&id) = self.owner_ids.get(key) {
            return id;
        }
        let id = u32::try_from(self.owners.len()).unwrap_or(NO_CALL);
        self.owners.push(key.clone());
        self.owner_ids.insert(key.clone(), id);
        id
    }

    /// The function `node` is in, when it is a node of one.
    fn owner_of(&self, node: u32) -> Option<u32> {
        self.node_owner.get(&node).copied()
    }

    /// The contexts of the function `owner`: its calls, or `NO_CALL`.
    fn contexts_of(&self, owner: Option<u32>) -> &[u32] {
        match owner {
            Some(owner) => self
                .contexts
                .get(owner as usize)
                .map_or(&[NO_CALL][..], Vec::as_slice),
            None => &[NO_CALL],
        }
    }

    /// The context reading a node of `owner` joined over its calls.
    fn every_call(&self, owner: u32) -> u32 {
        if self.contexts_of(Some(owner)) == [NO_CALL] {
            NO_CALL
        } else {
            EVERY_CALL
        }
    }

    /// The key of `node` as the function `owner` reads it in its context
    /// `call`: a node of `owner` in that call, a node no function owns as
    /// it is, a node of another function joined over that one's calls.
    fn key_at(&self, node: u32, owner: Option<u32>, call: u32) -> Key {
        match self.owner_of(node) {
            None => Key::Node(node, NO_CALL),
            Some(own) if Some(own) == owner => Key::Node(node, call),
            Some(own) => Key::Node(node, self.every_call(own)),
        }
    }

    fn fact_keys(&self, fact: &ValueFact, owner: Option<u32>, call: u32) -> Vec<Key> {
        fact.flow
            .nodes()
            .iter()
            .map(|&node| self.key_at(node, owner, call))
            .collect()
    }

    /// The number of the property `property` of `object`.
    fn cell(&self, object: Object, property: u32) -> Key {
        if let Some(&id) = self.cell_ids.borrow().get(&(object, property)) {
            return Key::Cell(id);
        }
        let mut cells = self.cells.borrow_mut();
        let id = u32::try_from(cells.len()).unwrap_or(u32::MAX);
        cells.push((object, property));
        self.cell_ids.borrow_mut().insert((object, property), id);
        Key::Cell(id)
    }

    fn cell_of(&self, id: u32) -> (Object, u32) {
        self.cells.borrow()[id as usize]
    }

    /// Learns every node's kind and function, and indexes parameters,
    /// returns and calls.
    fn index(&mut self, allocs: &[(u32, &Arc<str>)]) {
        let mut ids: FxHashSet<u32> = FxHashSet::default();
        let add_flow = |ids: &mut FxHashSet<u32>, fact: &ValueFact| {
            ids.extend(fact.flow.nodes().iter().copied());
        };
        for (&node, fact) in &self.derived {
            ids.insert(node);
            for operand in fact.operands.iter().chain(&fact.first) {
                add_flow(&mut ids, operand);
            }
        }
        for (&node, facts) in self.writes.iter().chain(&self.defaults) {
            ids.insert(node);
            for fact in facts {
                add_flow(&mut ids, fact);
            }
        }
        for list in self.params.values() {
            ids.extend(list.params.iter().copied());
        }
        for call in &self.calls {
            for (_, fact) in &call.args {
                add_flow(&mut ids, fact);
            }
            if let Some((_, fact)) = &call.unpacked_from {
                add_flow(&mut ids, fact);
            }
            if let Some(fact) = &call.receiver {
                add_flow(&mut ids, fact);
            }
        }
        for (&node, read) in &self.field_reads {
            ids.insert(node);
            ids.extend(read.properties.iter().copied());
            add_flow(&mut ids, &read.receiver);
        }
        for (&property, writes) in &self.field_writes {
            ids.insert(property);
            for write in writes {
                add_flow(&mut ids, &write.value);
                if let Some(receiver) = &write.receiver {
                    add_flow(&mut ids, receiver);
                }
            }
        }
        for id in ids {
            if let Some(node) = flow_nodes::node(id) {
                self.nodes.insert(id, node);
            }
        }
        let keys: Vec<Arc<str>> = self.params.keys().cloned().collect();
        for key in keys {
            let owner = self.owner_id(&key);
            let params = self.params[&key].params.clone();
            for (i, param) in params.into_iter().enumerate() {
                self.param_of.insert(param, (owner, i));
                self.node_owner.insert(param, owner);
            }
        }
        let derived: Vec<(u32, Arc<str>)> = self
            .derived
            .iter()
            .map(|(&node, fact)| (node, fact.owner.clone()))
            .collect();
        for (node, owner) in derived {
            let owner = self.owner_id(&owner);
            self.node_owner.insert(node, owner);
        }
        let reads: Vec<(u32, Arc<str>)> = self
            .field_reads
            .iter()
            .map(|(&node, read)| (node, read.owner.clone()))
            .collect();
        for (node, owner) in reads {
            let owner = self.owner_id(&owner);
            self.node_owner.insert(node, owner);
        }
        for &(node, owner) in allocs {
            let owner = self.owner_id(owner);
            self.node_owner.insert(node, owner);
        }
        let named: Vec<(u32, FlowNode)> = self
            .nodes
            .iter()
            .filter(|(_, node)| matches!(node, FlowNode::Return(_) | FlowNode::This(_)))
            .map(|(&id, node)| (id, node.clone()))
            .collect();
        for (id, node) in named {
            match node {
                FlowNode::Return(owner) => {
                    let owner = self.owner_id(&owner_key(&owner));
                    self.returns.insert(owner, id);
                    self.node_owner.insert(id, owner);
                }
                FlowNode::This(owner) => {
                    let owner = self.owner_id(&owner_key(&owner));
                    self.node_owner.insert(id, owner);
                }
                _ => {}
            }
        }
        let mut by_method: MethodIndex = FxHashMap::default();
        for (key, list) in &self.params {
            if let Some(class) = &list.class {
                by_method
                    .entry(list.method.to_ascii_lowercase().into())
                    .or_default()
                    .push((class.clone(), key.clone()));
            }
        }
        for methods in by_method.values_mut() {
            methods.sort();
        }
        let mut resolved_targets: FxHashMap<&CallTarget, Vec<Arc<str>>> = FxHashMap::default();
        let mut calls_into: Vec<Vec<u32>> = Vec::new();
        let calls = self.calls.clone();
        for (i, call) in calls.iter().enumerate() {
            let resolved = resolved_targets
                .entry(&call.target)
                .or_insert_with(|| self.resolve(&call.target, &by_method))
                .clone();
            let caller = self.owner_id(&call.caller);
            self.callers.push(caller);
            let targets: Vec<u32> = resolved.iter().map(|t| self.owner_id(t)).collect();
            for &target in &targets {
                let target = target as usize;
                if calls_into.len() <= target {
                    calls_into.resize(target + 1, Vec::new());
                }
                calls_into[target].push(u32::try_from(i).unwrap_or(NO_CALL));
            }
            self.targets.push(targets);
            self.calls_at.entry(call.site.clone()).or_default().push(i);
        }
        let call_nodes: Vec<(u32, flow_nodes::Site)> = self
            .nodes
            .iter()
            .filter_map(|(&id, node)| match node {
                FlowNode::Call { site, .. } => Some((id, site.clone())),
                _ => None,
            })
            .collect();
        for (id, site) in call_nodes {
            if let Some(&call) = self.calls_at.get(&site).and_then(|calls| calls.first()) {
                self.node_owner.insert(id, self.callers[call]);
            }
        }
        calls_into.resize(self.owners.len(), Vec::new());
        let follows: Vec<bool> = (0..self.calls.len())
            .map(|call| self.follows_caller(call))
            .collect();
        let mut calls_from: Vec<Vec<usize>> = vec![Vec::new(); self.owners.len()];
        for call in (0..self.calls.len()).filter(|&call| follows[call]) {
            calls_from[self.callers[call] as usize].push(call);
        }
        let mut contexts: Vec<FxHashSet<u32>> = vec![FxHashSet::default(); self.owners.len()];
        let mut paired_in: Vec<usize> = vec![0; self.owners.len()];
        let mut work: Vec<(u32, u32)> = Vec::new();
        for (owner, calls) in calls_into.iter().enumerate() {
            let owner = u32::try_from(owner).unwrap_or(NO_CALL);
            if calls.is_empty() {
                work.push((owner, NO_CALL));
            }
            for &call in calls.iter().filter(|&&call| !follows[call as usize]) {
                work.push((owner, call));
            }
        }
        loop {
            while let Some((owner, context)) = work.pop() {
                if !contexts[owner as usize].insert(context) {
                    continue;
                }
                for &call in &calls_from[owner as usize] {
                    let depth = self.pair_depth(context);
                    for &target in &self.targets[call].clone() {
                        let target_contexts = &mut paired_in[target as usize];
                        let made = if depth < MOST_PAIRED && *target_contexts < MOST_CONTEXTS {
                            *target_contexts += 1;
                            self.pair(call, context)
                        } else {
                            u32::try_from(call).unwrap_or(NO_CALL)
                        };
                        work.push((target, made));
                    }
                }
            }
            // A cycle of calls none of which is reached from outside it
            // runs as if nothing called it.
            let unreached: Vec<u32> = (0..self.owners.len())
                .filter(|&owner| contexts[owner].is_empty())
                .filter_map(|owner| u32::try_from(owner).ok())
                .collect();
            let Some(&owner) = unreached.first() else {
                break;
            };
            work.push((owner, NO_CALL));
        }
        self.contexts = contexts
            .into_iter()
            .map(|set| {
                let mut list: Vec<u32> = set.into_iter().collect();
                list.sort_unstable();
                list
            })
            .collect();
        self.index_low_bits();
    }

    /// Whether the call `call` is on an object its caller made or on its
    /// caller's `$this`: one that tells the caller's contexts apart.
    fn follows_caller(&self, call: usize) -> bool {
        let caller = self.callers[call];
        let Some(receiver) = &self.calls[call].receiver else {
            return false;
        };
        !receiver.flow.is_empty()
            && receiver.flow.nodes().iter().all(|&node| {
                matches!(
                    self.node(node),
                    Some(FlowNode::Alloc { .. } | FlowNode::This(_))
                ) && self.owner_of(node) == Some(caller)
            })
    }

    /// The context of `call` paired with its caller's `context`.
    fn pair(&mut self, call: usize, context: u32) -> u32 {
        if let Some(&id) = self.paired_ids.get(&(call, context)) {
            return id;
        }
        let id = u32::try_from(self.calls.len() + self.paired.len()).unwrap_or(NO_CALL);
        self.paired.push((call, context));
        self.paired_ids.insert((call, context), id);
        id
    }

    /// How many pairings `context` goes through.
    fn pair_depth(&self, mut context: u32) -> usize {
        let mut depth = 0;
        while let Some(&(_, outer)) = self.paired_of(context) {
            depth += 1;
            context = outer;
        }
        depth
    }

    fn paired_of(&self, context: u32) -> Option<&(usize, u32)> {
        (context as usize)
            .checked_sub(self.calls.len())
            .and_then(|i| self.paired.get(i))
    }

    /// The call `context` is a run of.
    fn call_of(&self, context: u32) -> usize {
        match self.paired_of(context) {
            Some(&(call, _)) => call,
            None => context as usize,
        }
    }

    /// The context of its caller a run in `context` is made from: the
    /// one it is paired with, or every one.
    fn caller_context(&self, context: u32) -> u32 {
        match self.paired_of(context) {
            Some(&(_, outer)) => outer,
            None => self.every_call(self.callers[context as usize]),
        }
    }

    /// The context of the callee that `call`, made in its caller's
    /// `context`, runs in.
    fn callee_context(&self, call: usize, context: u32) -> u32 {
        self.paired_ids
            .get(&(call, context))
            .copied()
            .unwrap_or(u32::try_from(call).unwrap_or(NO_CALL))
    }

    /// Finds the subtractions that keep what is below a bit of a value
    /// (`low_bits`).
    fn index_low_bits(&mut self) {
        let single = |fact: &ValueFact| match fact.flow.nodes() {
            [node] => Some(*node),
            _ => None,
        };
        let constant = |fact: &ValueFact| match fact.range {
            Some((Some(lo), Some(hi))) if lo == hi && fact.flow.is_empty() => Some(i128::from(lo)),
            _ => None,
        };
        let mut found = Vec::new();
        for (&node, fact) in &self.derived {
            let (DerivedOp::Sub, [value, taken]) = (fact.op, fact.operands.as_slice()) else {
                continue;
            };
            let Some(shl) = single(taken).and_then(|n| self.derived.get(&n)) else {
                continue;
            };
            let (DerivedOp::Shl, [shifted, by]) = (shl.op, shl.operands.as_slice()) else {
                continue;
            };
            let Some(shr) = single(shifted).and_then(|n| self.derived.get(&n)) else {
                continue;
            };
            let (DerivedOp::Shr, [from, back]) = (shr.op, shr.operands.as_slice()) else {
                continue;
            };
            let (Some(k), Some(k_back)) = (constant(by), constant(back)) else {
                continue;
            };
            if k != k_back || !(1..=62).contains(&k) || value.flow.is_empty() {
                continue;
            }
            let offset = if from.flow == value.flow {
                Some(0)
            } else {
                single(from)
                    .and_then(|n| self.derived.get(&n))
                    .and_then(|add| match (add.op, add.operands.as_slice()) {
                        (DerivedOp::Add, [x, c]) if x.flow == value.flow => constant(c),
                        (DerivedOp::Add, [c, x]) if x.flow == value.flow => constant(c),
                        _ => None,
                    })
            };
            if let Some(c) = offset {
                found.push((node, (-c, (1_i128 << k) - 1 - c)));
            }
        }
        self.low_bits.extend(found);
    }

    /// The functions `target` can run, by key: a function, or a method as
    /// the receiver's class finds it and, when dispatched, each override of
    /// it in a subclass.
    fn resolve(&self, target: &CallTarget, by_method: &MethodIndex) -> Vec<Arc<str>> {
        match target {
            CallTarget::Function(name) => vec![function_key(name)],
            CallTarget::Method {
                class,
                method,
                dispatch,
            } => {
                let mut out = Vec::new();
                if let Some((owner, _)) = self.registry.lookup_method_with_owner(class, method) {
                    let declared = self.registry.declared_method_name(&owner, method);
                    out.push(function_key(&format!("{owner}::{declared}")));
                }
                if *dispatch
                    && let Some(overrides) = by_method.get(method.to_ascii_lowercase().as_str())
                {
                    for (declaring, key) in overrides {
                        if self.registry.is_subclass_of(declaring, class) && !out.contains(key) {
                            out.push(key.clone());
                        }
                    }
                }
                out
            }
        }
    }

    /// What the run of `owner` in `context` is passed for its parameter at
    /// `index` (the node `param`): the call's arguments there, as the
    /// caller reads them in the context the run is made from, or the
    /// default, read by `owner` in that run, when it passes none.
    fn args_for(&self, context: u32, owner: u32, index: usize, param: u32) -> Vec<Input<'f>> {
        let call = self.call_of(context);
        let fact: &'f CallFact = self.calls[call];
        let caller = self.callers[call];
        let made_in = self.caller_context(context);
        let variadic = self
            .owners
            .get(owner as usize)
            .and_then(|key| self.params.get(key))
            .and_then(|list| list.variadic);
        let mut out: Vec<Input<'f>> = fact
            .args
            .iter()
            .filter(|(at, _)| *at == index || variadic.is_some_and(|v| v == index && *at >= v))
            .map(|(_, value)| Input::Fact(value, Some(caller), made_in))
            .collect();
        if let Some((from, value)) = &fact.unpacked_from
            && index >= *from
        {
            out.push(Input::Fact(value, Some(caller), made_in));
        }
        if out.is_empty() {
            for &default in self.defaults.get(&param).into_iter().flatten() {
                out.push(Input::Fact(default, Some(owner), context));
            }
        }
        out
    }

    /// The writes that reach the property of `object` with the node
    /// `property`: each with the function writing it and that function's
    /// context. `read` collects the keys whose objects this depends on.
    fn cell_writes(&self, object: Object, property: u32, read: &mut Vec<Key>) -> Vec<Input<'f>> {
        let mut out = Vec::new();
        for &write in self.field_writes.get(&property).into_iter().flatten() {
            let write: &'f FieldWrite = write;
            let (Some(receiver), Some(owner)) = (&write.receiver, &write.owner) else {
                out.push(Input::Fact(&write.value, None, NO_CALL));
                continue;
            };
            let Some(&owner) = self.owner_ids.get(owner) else {
                continue;
            };
            for &call in self.contexts_of(Some(owner)) {
                let on = self.objects_of_fact(receiver, Some(owner), call, read);
                let reaches = if object == OPEN_OBJECT {
                    on.open
                } else {
                    on.is_any() || on.known().binary_search(&object).is_ok()
                };
                if reaches {
                    out.push(Input::Fact(&write.value, Some(owner), call));
                }
            }
        }
        out
    }

    /// The keys the read `node` of `owner` in `call` takes its value from:
    /// the property of each object its receiver can be.
    fn read_keys(&self, node: u32, call: u32, read: &mut Vec<Key>) -> Vec<Key> {
        let Some(fact) = self.field_reads.get(&node) else {
            return Vec::new();
        };
        let owner = self.owner_of(node);
        let on = self.objects_of_fact(&fact.receiver, owner, call, read);
        let mut out = Vec::new();
        for &property in &fact.properties {
            if on.is_any() {
                out.push(Key::Node(property, NO_CALL));
                continue;
            }
            for &object in on.known() {
                out.push(self.cell(object, property));
            }
            if on.open {
                out.push(self.cell(OPEN_OBJECT, property));
            }
        }
        out
    }

    /// What `key`'s value is read from, for a key that isn't an operation
    /// or a source.
    fn inputs(&self, key: Key, read: &mut Vec<Key>) -> Vec<Input<'f>> {
        let (node, call) = match key {
            Key::Cell(id) => {
                let (object, property) = self.cell_of(id);
                return self.cell_writes(object, property, read);
            }
            Key::Node(node, call) => (node, call),
        };
        if call == EVERY_CALL {
            return self
                .contexts_of(self.owner_of(node))
                .iter()
                .map(|&c| Input::Key(Key::Node(node, c)))
                .collect();
        }
        match self.node(node) {
            Some(FlowNode::Param { .. }) => match self.param_of.get(&node) {
                Some(&(owner, index)) if call != NO_CALL => self.args_for(call, owner, index, node),
                _ => Vec::new(),
            },
            Some(FlowNode::Field { .. }) => self
                .read_keys(node, call, read)
                .into_iter()
                .map(Input::Key)
                .collect(),
            Some(FlowNode::Call { site, .. }) => {
                let mut out = Vec::new();
                for &made in self.calls_at.get(site).into_iter().flatten() {
                    let runs_in = self.callee_context(made, call);
                    for target in &self.targets[made] {
                        if let Some(&ret) = self.returns.get(target) {
                            out.push(Input::Key(Key::Node(ret, runs_in)));
                        }
                    }
                }
                out
            }
            Some(FlowNode::Return(_)) => {
                let owner = self.owner_of(node);
                self.writes
                    .get(&node)
                    .into_iter()
                    .flatten()
                    .map(|&fact| Input::Fact(fact, owner, call))
                    .collect()
            }
            Some(FlowNode::Property { .. } | FlowNode::Global(_) | FlowNode::Static { .. }) => self
                .writes
                .get(&node)
                .into_iter()
                .flatten()
                .map(|&fact| Input::Fact(fact, None, NO_CALL))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn input_keys(&self, input: Input<'f>) -> Vec<Key> {
        match input {
            Input::Fact(fact, owner, call) => self.fact_keys(fact, owner, call),
            Input::Key(key) => vec![key],
        }
    }

    /// Works out the objects of `roots` and of every key they are worked
    /// out from, adding what goes around each cycle until nothing new
    /// does.
    fn solve_objects(&mut self, roots: &[Key]) {
        let mut queue: std::collections::VecDeque<Key> = std::collections::VecDeque::new();
        let mut queued: FxHashSet<Key> = FxHashSet::default();
        for &root in roots {
            if !self.solved.contains(&root) && queued.insert(root) {
                queue.push_back(root);
            }
        }
        while let Some(key) = queue.pop_front() {
            queued.remove(&key);
            let mut read = Vec::new();
            let mut objects = self.objects_from(key, &mut read);
            for from in read {
                if self.object_edges.insert((from, key)) {
                    self.object_readers.entry(from).or_default().push(key);
                }
                if !self.solved.contains(&from) && queued.insert(from) {
                    queue.push_back(from);
                }
            }
            let first = self.solved.insert(key);
            if let Some(known) = self.objects.get(&key) {
                objects.join(known);
                if !first && *known == objects {
                    continue;
                }
            }
            self.objects.insert(key, objects);
            for &reader in self.object_readers.get(&key).into_iter().flatten() {
                if queued.insert(reader) {
                    queue.push_back(reader);
                }
            }
        }
    }

    /// The objects `key` can be, from what it is made of now; `read`
    /// collects the keys that is.
    fn objects_from(&self, key: Key, read: &mut Vec<Key>) -> Objects {
        let mut out = Objects::default();
        let mut joined_any = false;
        let mut join = |out: &mut Objects, objects: Objects| {
            joined_any = true;
            out.join(&objects);
        };
        if let Key::Node(node, call) = key {
            match self.node(node) {
                Some(FlowNode::Alloc { .. }) if call != EVERY_CALL => {
                    return Objects {
                        allocs: vec![object(node, call)],
                        open: false,
                        any: false,
                    };
                }
                Some(FlowNode::Derived { .. } | FlowNode::Env(_)) => return Objects::default(),
                Some(FlowNode::This(_)) if call == NO_CALL => return Objects::open(),
                Some(FlowNode::This(_)) if call != EVERY_CALL => {
                    let made = self.call_of(call);
                    let caller = self.callers[made];
                    return match &self.calls[made].receiver {
                        Some(receiver) => self.objects_of_fact(
                            receiver,
                            Some(caller),
                            self.caller_context(call),
                            read,
                        ),
                        None => Objects::open(),
                    };
                }
                Some(FlowNode::Param { .. }) if call == NO_CALL => return Objects::open(),
                _ => {}
            }
        }
        for input in self.inputs(key, read) {
            let objects = match input {
                Input::Fact(fact, owner, call) => self.objects_of_fact(fact, owner, call, read),
                Input::Key(key) => {
                    read.push(key);
                    self.object_value(key)
                }
            };
            join(&mut out, objects);
        }
        if !joined_any {
            out.open = true;
        }
        out
    }

    fn object_value(&self, key: Key) -> Objects {
        self.objects.get(&key).cloned().unwrap_or_default()
    }

    /// The objects `fact` can be as `owner` reads it in `call`.
    fn objects_of_fact(
        &self,
        fact: &ValueFact,
        owner: Option<u32>,
        call: u32,
        read: &mut Vec<Key>,
    ) -> Objects {
        if fact.flow.is_empty() {
            return Objects::open();
        }
        let mut out = Objects::default();
        for &node in fact.flow.nodes() {
            let key = self.key_at(node, owner, call);
            read.push(key);
            out.join(&self.object_value(key));
        }
        out
    }

    /// The keys `key`'s value is made of, once the objects it reads are
    /// worked out.
    fn compute_dependencies(&mut self, key: Key) -> Vec<Key> {
        if matches!(key, Key::Cell(_))
            || matches!(key, Key::Node(node, call)
                if call != EVERY_CALL && matches!(self.node(node), Some(FlowNode::Field { .. })))
        {
            self.solve_objects(&[key]);
        }
        let mut out: Vec<Key> = Vec::new();
        match key {
            Key::Node(node, call)
                if call != EVERY_CALL
                    && matches!(self.node(node), Some(FlowNode::Derived { .. })) =>
            {
                if let Some(fact) = self.derived.get(&node) {
                    let owner = self.owner_of(node);
                    for operand in fact.operands.iter().chain(&fact.first) {
                        out.extend(self.fact_keys(operand, owner, call));
                    }
                }
            }
            _ => {
                let mut read = Vec::new();
                for input in self.inputs(key, &mut read) {
                    out.extend(self.input_keys(input));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn dependencies(&self, key: Key) -> &[Key] {
        self.deps.get(&key).map_or(&[], Vec::as_slice)
    }

    /// Gives every key its value: the cycles of the graph one at a time,
    /// each after the keys it is made of.
    fn settle(&mut self, keys: &[Key]) {
        for component in self.components(keys) {
            let cyclic =
                component.len() > 1 || self.dependencies(component[0]).contains(&component[0]);
            if cyclic {
                self.cyclic.extend(component.iter().copied());
                self.settle_cycle(&component);
            } else {
                let value = self.transfer(component[0]);
                self.base.insert(component[0], value);
            }
        }
    }

    /// The strongly connected components of the graph over `keys`, each
    /// after the components it depends on (Tarjan's algorithm, iterative).
    fn components(&self, keys: &[Key]) -> Vec<Vec<Key>> {
        let mut index: FxHashMap<Key, usize> = FxHashMap::default();
        let mut low: FxHashMap<Key, usize> = FxHashMap::default();
        let mut on_stack: FxHashSet<Key> = FxHashSet::default();
        let mut stack: Vec<Key> = Vec::new();
        let mut out: Vec<Vec<Key>> = Vec::new();
        let mut next = 0usize;
        for &root in keys {
            if index.contains_key(&root) {
                continue;
            }
            let mut work: Vec<(Key, usize)> = Vec::new();
            index.insert(root, next);
            low.insert(root, next);
            next += 1;
            stack.push(root);
            on_stack.insert(root);
            work.push((root, 0));
            while let Some((key, at)) = work.last_mut() {
                let key = *key;
                let deps = self.dependencies(key);
                if *at < deps.len() {
                    let dep = deps[*at];
                    *at += 1;
                    if !self.deps.contains_key(&dep) {
                        continue;
                    }
                    if let std::collections::hash_map::Entry::Vacant(slot) = index.entry(dep) {
                        slot.insert(next);
                        low.insert(dep, next);
                        next += 1;
                        stack.push(dep);
                        on_stack.insert(dep);
                        work.push((dep, 0));
                    } else if on_stack.contains(&dep) {
                        let l = low[&key].min(index[&dep]);
                        low.insert(key, l);
                    }
                    continue;
                }
                work.pop();
                if let Some((parent, _)) = work.last() {
                    let l = low[parent].min(low[&key]);
                    low.insert(*parent, l);
                }
                if low[&key] == index[&key] {
                    let mut component = Vec::new();
                    while let Some(member) = stack.pop() {
                        on_stack.remove(&member);
                        component.push(member);
                        if member == key {
                            break;
                        }
                    }
                    component.sort_unstable();
                    out.push(component);
                }
            }
        }
        out
    }

    /// What the operation `node` of `owner` gives on `operands`: its
    /// result, held to what is left below a bit when it keeps that
    /// (`low_bits`).
    fn derived_value(&self, node: u32, fact: &DerivedFact, operands: &[Value]) -> Value {
        let value = compute(fact.op, operands).0.within(fact.result);
        match (value, self.low_bits.get(&node)) {
            (Value::Within(..) | Value::Chosen, Some(&(lo, hi))) => {
                value.meet(Value::Within(lo, hi))
            }
            _ => value,
        }
    }

    /// `key`'s value from the values of what it is made of.
    fn transfer(&self, key: Key) -> Value {
        if let Key::Node(node, call) = key
            && call != EVERY_CALL
        {
            match self.node(node) {
                Some(FlowNode::Env(_)) => return Value::Chosen,
                Some(FlowNode::Derived { .. }) => {
                    let Some(fact) = self.derived.get(&node) else {
                        return Value::Unknown;
                    };
                    let owner = self.owner_of(node);
                    let operands: Vec<Value> = fact
                        .operands
                        .iter()
                        .map(|o| self.eval_fact(o, owner, call))
                        .collect();
                    return self.derived_value(node, fact, &operands);
                }
                _ => {}
            }
        }
        let mut read = Vec::new();
        let value = self
            .inputs(key, &mut read)
            .into_iter()
            .fold(Value::Unknown, |acc, input| {
                acc.join(self.eval_input(input))
            });
        self.held_to_type(key, value)
    }

    /// `value` held to the range the type of a property read gives it.
    fn held_to_type(&self, key: Key, value: Value) -> Value {
        match key {
            Key::Node(node, call) if call != EVERY_CALL => match self.field_reads.get(&node) {
                Some(read) => value.within(read.result),
                None => value,
            },
            _ => value,
        }
    }

    fn base_of(&self, key: Key) -> Value {
        self.base.get(&key).copied().unwrap_or(Value::Unknown)
    }

    fn eval_input(&self, input: Input<'f>) -> Value {
        match input {
            Input::Fact(fact, owner, call) => self.eval_fact(fact, owner, call),
            Input::Key(key) => self.base_of(key),
        }
    }

    /// What `fact` is as the function `owner` reads it in `call`.
    fn eval_fact(&self, fact: &ValueFact, owner: Option<u32>, call: u32) -> Value {
        self.eval_fact_with(fact, owner, call, &|key| self.base_of(key))
    }

    fn eval_fact_with(
        &self,
        fact: &ValueFact,
        owner: Option<u32>,
        call: u32,
        value_of: &dyn Fn(Key) -> Value,
    ) -> Value {
        if fact.flow.is_empty() {
            return match fact.range {
                Some((Some(lo), Some(hi))) => Value::Within(lo.into(), hi.into()),
                _ => Value::Unknown,
            };
        }
        fact.flow
            .nodes()
            .iter()
            .fold(Value::Unknown, |acc, &node| {
                acc.join(value_of(self.key_at(node, owner, call)))
            })
            .within(fact.range)
    }

    /// The values of the keys of a cycle. When each operation in it adds
    /// or subtracts an amount from outside it to what goes around, every
    /// key holds what comes in from outside moved by those amounts as many
    /// times as the cycle can run. A cycle the environment feeds holds
    /// what it chooses. Any other runs its operations as many times as it
    /// can run (`iterate_cycle`).
    fn settle_cycle(&mut self, members: &[Key]) {
        let inside: FxHashSet<Key> = members.iter().copied().collect();
        let outside = |fact: &ValueFact, owner: Option<u32>, call: u32| -> Value {
            let keys = self.fact_keys(fact, owner, call);
            if !keys.is_empty() && keys.iter().all(|k| inside.contains(k)) {
                return Value::Unknown;
            }
            self.eval_fact_with(fact, owner, call, &|key| {
                if inside.contains(&key) {
                    Value::Unknown
                } else {
                    self.base_of(key)
                }
            })
        };
        let touches = |fact: &ValueFact, owner: Option<u32>, call: u32| {
            self.fact_keys(fact, owner, call)
                .iter()
                .any(|k| inside.contains(k))
        };
        let mut entry = Value::Unknown;
        let mut step_lo: i128 = 0;
        let mut step_hi: i128 = 0;
        let mut linear = true;
        let mut persistent = false;
        let mut executions: Option<i64> = Some(1);
        for &member in members {
            if let Key::Node(node, call) = member
                && call != EVERY_CALL
                && let Some(FlowNode::Derived { .. }) = self.node(node)
            {
                let Some(fact) = self.derived.get(&node) else {
                    continue;
                };
                let owner = self.owner_of(node);
                executions = match (executions, fact.executions) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    _ => None,
                };
                let cyclic: Vec<usize> = (0..fact.operands.len())
                    .filter(|&i| touches(&fact.operands[i], owner, call))
                    .collect();
                let (cycle_at, step_at, negate) = match (fact.op, cyclic.as_slice()) {
                    (DerivedOp::Add, [0]) => (0, 1, false),
                    (DerivedOp::Add, [1]) => (1, 0, false),
                    (DerivedOp::Sub, [0]) => (0, 1, true),
                    _ => {
                        linear = false;
                        continue;
                    }
                };
                entry = entry.join(outside(&fact.operands[cycle_at], owner, call));
                if let Some(first) = fact.first.get(cycle_at)
                    && !touches(first, owner, call)
                {
                    entry = entry.join(self.eval_fact(first, owner, call));
                }
                match self
                    .eval_fact(&fact.operands[step_at], owner, call)
                    .interval()
                {
                    Some((lo, hi)) => {
                        let (lo, hi) = if negate { (-hi, -lo) } else { (lo, hi) };
                        step_lo += lo.min(0);
                        step_hi += hi.max(0);
                    }
                    None => linear = false,
                }
                continue;
            }
            // Anything else in a cycle is a parameter, a call's result, a
            // property, a global, a static or a value joined over calls:
            // what goes around it lasts from one call to the next.
            persistent = true;
            let mut read = Vec::new();
            for input in self.inputs(member, &mut read) {
                entry = entry.join(match input {
                    Input::Fact(fact, owner, call) => outside(fact, owner, call),
                    Input::Key(key) if inside.contains(&key) => Value::Unknown,
                    Input::Key(key) => self.base_of(key),
                });
            }
        }
        // A cycle no loop bounds runs at most as often as the run lets it.
        let executions = match executions {
            _ if persistent => self.run_total,
            Some(times) => Some(self.run_total.map_or(times, |run| times.min(run))),
            None => self.run_total,
        };
        let value = match entry {
            Value::Chosen => Value::Chosen,
            Value::Within(lo, hi) if linear => {
                let times = executions.map(i128::from);
                let lo = match times {
                    _ if step_lo == 0 => lo,
                    Some(times) => lo.saturating_add(times.saturating_mul(step_lo)),
                    None => INT_MIN - 1,
                };
                let hi = match times {
                    _ if step_hi == 0 => hi,
                    Some(times) => hi.saturating_add(times.saturating_mul(step_hi)),
                    None => INT_MAX + 1,
                };
                Value::Within(lo, hi).clamped()
            }
            _ => {
                self.iterate_cycle(members, executions);
                return;
            }
        };
        for &member in members {
            let own = match member {
                Key::Node(node, call) if call != EVERY_CALL => match self.derived.get(&node) {
                    Some(fact) => value.within(fact.result),
                    None => self.held_to_type(member, value),
                },
                _ => value,
            };
            self.base.insert(member, own);
        }
    }

    /// Runs the operations of a cycle on what comes into it as many times
    /// as the cycle can run (`executions`), or until no value changes,
    /// `CYCLE_ROUNDS` times at most. A value still changing then goes on
    /// by what the last run added for each run left, and the operations
    /// are run again on that for as long as it narrows it
    /// (`NARROWING_ROUNDS` at most): an operation that takes back part of
    /// what goes around (`%`, `>>`, `&`) holds it to what it allows.
    fn iterate_cycle(&mut self, members: &[Key], executions: Option<i64>) {
        for &member in members {
            self.base.insert(member, Value::Unknown);
        }
        let times = executions.map_or(i128::MAX, i128::from).max(1);
        let rounds = usize::try_from(times).map_or(CYCLE_ROUNDS, |t| t.min(CYCLE_ROUNDS));
        let mut before = self.values_of(members);
        for round in 1..=rounds {
            before = self.values_of(members);
            if !self.cycle_round(members, Value::join) {
                return;
            }
            if i128::try_from(round).is_ok_and(|round| round >= times) {
                return;
            }
        }
        let left = times.saturating_sub(i128::try_from(rounds).unwrap_or(i128::MAX));
        for (&member, before) in members.iter().zip(before) {
            let value = self.base_of(member).carried_on(before, left).clamped();
            self.base.insert(member, value);
        }
        for _ in 0..NARROWING_ROUNDS {
            if !self.cycle_round(members, Value::meet) {
                break;
            }
        }
    }

    /// Runs each member of a cycle once, taking `next` of what it had and
    /// what it holds now; whether a value changed.
    fn cycle_round(&mut self, members: &[Key], next: fn(Value, Value) -> Value) -> bool {
        let mut changed = false;
        for &member in members {
            let old = self.base_of(member);
            let new = next(old, self.cycle_transfer(member).clamped());
            if new != old {
                self.base.insert(member, new);
                changed = true;
            }
        }
        changed
    }

    fn values_of(&self, members: &[Key]) -> Vec<Value> {
        members.iter().map(|&member| self.base_of(member)).collect()
    }

    /// What the cycle member `key` holds from the values its operands have
    /// now, and, for an operation, from the operands it had the first time
    /// it was checked (what comes into the cycle).
    fn cycle_transfer(&self, key: Key) -> Value {
        let value = self.transfer(key);
        match key {
            Key::Node(node, call) if call != EVERY_CALL => match self.derived.get(&node) {
                Some(fact) if fact.first.len() == fact.operands.len() => {
                    let owner = self.owner_of(node);
                    let first: Vec<Value> = fact
                        .first
                        .iter()
                        .map(|o| self.eval_fact(o, owner, call))
                        .collect();
                    value.join(self.derived_value(node, fact, &first))
                }
                _ => value,
            },
            _ => value,
        }
    }

    /// The context in which the operation of the `Derived` node `node` can
    /// give a float its sink rejects (`leaves`), when it can: one of its
    /// function's calls, as a chain of calls into it shows it.
    fn overflows(&self, node: u32, arrival: &Arrival) -> Option<u32> {
        let fact = self.derived.get(&node)?;
        let owner = self.owner_of(node);
        let leaves = |values: &[Value]| leaves(fact.op, values, arrival);
        for &call in self.contexts_of(owner) {
            let base: Vec<Value> = fact
                .operands
                .iter()
                .map(|o| self.eval_fact(o, owner, call))
                .collect();
            if !leaves(&base) {
                continue;
            }
            if call == NO_CALL || self.cyclic.contains(&Key::Node(node, call)) {
                return Some(call);
            }
            let holds = self.in_some_context(owner, call, |graph, chain| {
                let mut memo = FxHashMap::default();
                let values: Vec<Value> = fact
                    .operands
                    .iter()
                    .map(|o| graph.eval_in(o, chain, &mut memo))
                    .collect();
                leaves(&values)
            });
            if holds {
                return Some(call);
            }
        }
        None
    }

    /// Whether `holds` is true in some chain of calls that starts with the
    /// run of `owner` in `context` and goes on through its callers, or of
    /// the values of that run alone when there are too many to try.
    fn in_some_context(
        &self,
        owner: Option<u32>,
        context: u32,
        holds: impl Fn(&Self, &[Frame]) -> bool,
    ) -> bool {
        let (Some(owner), true) = (owner, context != NO_CALL && context != EVERY_CALL) else {
            return holds(self, &[]);
        };
        let mut chains: Vec<Vec<Frame>> = vec![chain_start(owner, context)];
        for _ in 1..CONTEXT_DEPTH {
            let mut longer: Vec<Vec<Frame>> = Vec::new();
            let mut grew = false;
            for chain in chains {
                let last = chain.last().map_or(context, |frame| frame.context);
                let into = self.callers[self.call_of(last)];
                let outer = match self.paired_of(last) {
                    Some(&(_, outer)) => vec![outer],
                    None => self.contexts_of(Some(into)).to_vec(),
                };
                if outer == [NO_CALL] {
                    longer.push(chain);
                    continue;
                }
                grew = true;
                for next_context in outer {
                    let mut next = chain.clone();
                    next.push(Frame {
                        context: next_context,
                        callee: into,
                    });
                    longer.push(next);
                }
                if longer.len() > CONTEXT_BUDGET {
                    return holds(self, &chain_start(owner, context));
                }
            }
            chains = longer;
            if !grew {
                break;
            }
        }
        chains.iter().any(|chain| holds(self, chain))
    }

    /// What `fact` is in the chain of calls `chain`, innermost first.
    fn eval_in(
        &self,
        fact: &ValueFact,
        chain: &[Frame],
        memo: &mut FxHashMap<(u32, usize), Value>,
    ) -> Value {
        let Some(frame) = chain.first() else {
            return self.eval_fact(fact, None, NO_CALL);
        };
        if fact.flow.is_empty() {
            return self.eval_fact(fact, Some(frame.callee), frame.context);
        }
        let mut acc = Value::Unknown;
        for &node in fact.flow.nodes() {
            acc = acc.join(self.value_in(node, chain, memo));
        }
        acc.within(fact.range)
    }

    /// What `node` is in `chain`: a parameter of the innermost call's
    /// function holds what that call passes as the next call in the chain
    /// gives it, a value computed in that function is computed from those;
    /// anything else holds its value in the innermost call.
    fn value_in(
        &self,
        node: u32,
        chain: &[Frame],
        memo: &mut FxHashMap<(u32, usize), Value>,
    ) -> Value {
        let frame = &chain[0];
        let key = self.key_at(node, Some(frame.callee), frame.context);
        if self.owner_of(node) != Some(frame.callee) {
            return self.base_of(key);
        }
        if let Some(&known) = memo.get(&(node, chain.len())) {
            return known;
        }
        let value = match self.node(node) {
            Some(FlowNode::Param { .. }) => match self.param_of.get(&node) {
                Some(&(owner, index)) => self
                    .args_for(frame.context, owner, index, node)
                    .into_iter()
                    .fold(Value::Unknown, |acc, input| {
                        acc.join(match input {
                            Input::Fact(fact, Some(by), _) if by != owner && chain.len() > 1 => {
                                self.eval_in(fact, &chain[1..], memo)
                            }
                            other => self.eval_input(other),
                        })
                    }),
                None => self.base_of(key),
            },
            Some(FlowNode::Derived { .. }) if !self.cyclic.contains(&key) => {
                match self.derived.get(&node) {
                    Some(fact) => {
                        let operands: Vec<Value> = fact
                            .operands
                            .iter()
                            .map(|o| self.eval_in(o, chain, memo))
                            .collect();
                        self.derived_value(node, fact, &operands)
                    }
                    None => self.base_of(key),
                }
            }
            _ => self.base_of(key),
        };
        memo.insert((node, chain.len()), value);
        value
    }

    /// The finding for a use of the floats of `overflows`, when one of
    /// their operations can leave the `int` range: the operations, and
    /// where their values come from.
    pub(crate) fn judge(&self, sink: &SinkFact) -> Option<Overflow> {
        match &sink.kind {
            SinkKind::Float {
                overflows,
                sink,
                every_float,
            } => {
                let mut sites: Vec<(u32, Arc<str>, u32, u32)> = Vec::new();
                for &overflow in overflows {
                    let Some(FlowNode::Overflow { op, line, .. }) = flow_nodes::node(overflow)
                    else {
                        continue;
                    };
                    let Some(derived) = derived_of(overflow) else {
                        continue;
                    };
                    let arrival = Arrival::of(overflow, *every_float);
                    if let Some(call) = self.overflows(derived, &arrival) {
                        sites.push((line, op, derived, call));
                    }
                }
                if sites.is_empty() {
                    return None;
                }
                sites.sort();
                sites.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
                let operations = sites
                    .iter()
                    .map(|(line, op, _, _)| format!("{op} on line {line}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut start = Vec::new();
                for &(_, _, node, call) in &sites {
                    if let Some(fact) = self.derived.get(&node) {
                        let owner = self.owner_of(node);
                        for operand in &fact.operands {
                            start.extend(self.fact_keys(operand, owner, call));
                        }
                    }
                }
                let from = self.describe_sources(start);
                Some(Overflow {
                    environment: !from.is_empty(),
                    message: format!(
                        "The result of {operations} can leave the int range{from}, and {sink}."
                    ),
                })
            }
            SinkKind::IntDiv { dividend, divisor } => {
                let can = |dividend: Value, divisor: Value| {
                    dividend.interval().is_some_and(|(lo, _)| lo == INT_MIN)
                        && divisor
                            .interval()
                            .is_some_and(|(lo, hi)| lo <= -1 && -1 <= hi)
                };
                let owner = self.owner_ids.get(&sink.owner).copied();
                let found = self.contexts_of(owner).iter().copied().find(|&call| {
                    can(
                        self.eval_fact(dividend, owner, call),
                        self.eval_fact(divisor, owner, call),
                    ) && self.in_some_context(owner, call, |graph, chain| {
                        let mut memo = FxHashMap::default();
                        if chain.is_empty() {
                            return can(
                                graph.eval_fact(dividend, owner, call),
                                graph.eval_fact(divisor, owner, call),
                            );
                        }
                        can(
                            graph.eval_in(dividend, chain, &mut memo),
                            graph.eval_in(divisor, chain, &mut memo),
                        )
                    })
                })?;
                let mut start = self.fact_keys(dividend, owner, found);
                start.extend(self.fact_keys(divisor, owner, found));
                let from = self.describe_sources(start);
                Some(Overflow {
                    environment: !from.is_empty(),
                    message: format!(
                        "intdiv() can divide PHP_INT_MIN by -1{from}, whose result is past the int range: an ArithmeticError."
                    ),
                })
            }
        }
    }

    /// ` with a value from …` naming the environment the values at `start`
    /// come from, and the parameters, properties, calls and variables they
    /// go through on the way; empty when none comes from the environment.
    fn describe_sources(&self, mut start: Vec<Key>) -> String {
        let mut seen: FxHashSet<Key> = FxHashSet::default();
        let mut queue: std::collections::VecDeque<(Key, Option<usize>)> =
            std::collections::VecDeque::new();
        let mut parents: Vec<(Key, Option<usize>)> = Vec::new();
        start.sort_by_key(|&k| self.label(k));
        for key in start {
            if seen.insert(key) {
                queue.push_back((key, None));
            }
        }
        let mut environment: Vec<String> = Vec::new();
        let mut path: Option<Vec<String>> = None;
        while let Some((key, parent)) = queue.pop_front() {
            if self.base_of(key) == Value::Unknown {
                continue;
            }
            parents.push((key, parent));
            let at = parents.len() - 1;
            if let Key::Node(node, _) = key
                && let Some(FlowNode::Env(name)) = self.node(node)
            {
                environment.push(name.to_string());
                if path.is_none() {
                    let mut steps: Vec<String> = Vec::new();
                    let mut cursor = parents[at].1;
                    while let Some(i) = cursor {
                        if let Some(step) = self.step_name(parents[i].0)
                            && steps.last() != Some(&step)
                        {
                            steps.push(step);
                        }
                        cursor = parents[i].1;
                    }
                    path = Some(steps);
                }
                continue;
            }
            let mut deps = self.dependencies(key).to_vec();
            deps.sort_by_key(|&k| self.label(k));
            for dep in deps {
                if seen.insert(dep) {
                    queue.push_back((dep, Some(at)));
                }
            }
        }
        if environment.is_empty() {
            return String::new();
        }
        environment.sort();
        environment.dedup();
        let mut out = format!(" with a value from {}", environment.join(", "));
        if let Some(steps) = path.filter(|steps| !steps.is_empty()) {
            out.push_str(" through ");
            out.push_str(&steps.join(", "));
        }
        out
    }

    /// A stable order for keys: how a message would name their nodes.
    fn label(&self, key: Key) -> String {
        match key {
            Key::Node(node, call) => match self.node(node) {
                Some(n) => format!("{n:?}#{call}"),
                None => String::new(),
            },
            Key::Cell(id) => {
                let (object, property) = self.cell_of(id);
                format!("{:?}#{object}", self.node(property))
            }
        }
    }

    /// How a message names a place a value goes through.
    fn step_name(&self, key: Key) -> Option<String> {
        let node = match key {
            Key::Node(node, _) => node,
            Key::Cell(id) => self.cell_of(id).1,
        };
        match self.node(node)? {
            FlowNode::Param { owner, name } => Some(format!("parameter ${name} of {owner}()")),
            FlowNode::Property { class, name } => Some(format!("{class}::${name}")),
            FlowNode::Field { .. } => self
                .field_reads
                .get(&node)?
                .properties
                .iter()
                .find_map(|&property| self.step_name(Key::Node(property, NO_CALL))),
            FlowNode::Call { callee, .. } => Some(format!("the result of {callee}")),
            FlowNode::Global(name) => Some(format!("global ${name}")),
            FlowNode::Static { owner, name } => Some(format!("static ${name} of {owner}()")),
            _ => None,
        }
    }
}

/// An operation that can leave the `int` range, as `Graph::judge` finds
/// it: the message, and whether the environment chooses the values that
/// take it there.
pub(crate) struct Overflow {
    pub(crate) message: String,
    pub(crate) environment: bool,
}

impl Overflow {
    /// The identifier it is reported with, when it is reported:
    /// `arithmetic.intOverflow` when the environment drives it,
    /// `arithmetic.intOverflowGrowth` when values grow there on their own
    /// and `growth` reports those.
    pub(crate) fn identifier(&self, growth: bool) -> Option<&'static str> {
        if self.environment {
            Some("arithmetic.intOverflow")
        } else if growth {
            Some("arithmetic.intOverflowGrowth")
        } else {
            None
        }
    }
}

/// The chain of the run of `owner` in `context` alone.
fn chain_start(owner: u32, context: u32) -> Vec<Frame> {
    vec![Frame {
        context,
        callee: owner,
    }]
}

impl DerivedFact {
    /// `other`'s operands and bounds joined into these.
    fn join_from(&mut self, other: &DerivedFact) {
        if self.operands.len() == other.operands.len() {
            for (mine, theirs) in self.operands.iter_mut().zip(&other.operands) {
                mine.range = crate::checker::flow_facts::join_ranges(mine.range, theirs.range);
                mine.flow = mine.flow.union(&theirs.flow);
            }
        }
        self.result = crate::checker::flow_facts::join_ranges(self.result, other.result);
        self.executions = match (self.executions, other.executions) {
            (Some(a), Some(b)) => Some(a.max(b)),
            _ => None,
        };
    }
}

/// The `Derived` node of the operation whose float the `Overflow` node
/// `overflow` is.
fn derived_of(overflow: u32) -> Option<u32> {
    let FlowNode::Overflow { site, op, line, .. } = flow_nodes::node(overflow)? else {
        return None;
    };
    Some(flow_nodes::id(FlowNode::Derived { site, op, line }))
}

/// What `op` gives from `operands` (as `int`s, held to the `int` range),
/// and whether its exact result can leave the `int` range.
fn compute(op: DerivedOp, operands: &[Value]) -> (Value, bool) {
    if operands.contains(&Value::Unknown) {
        return (Value::Unknown, false);
    }
    if op == DerivedOp::Opaque {
        let chosen = operands.contains(&Value::Chosen);
        return (
            if chosen {
                Value::Chosen
            } else {
                Value::Unknown
            },
            false,
        );
    }
    let intervals: Vec<(i128, i128)> = operands.iter().filter_map(|v| v.interval()).collect();
    let exact = match (op, intervals.as_slice()) {
        (DerivedOp::Add, [(a, b), (c, d)]) => Some((a + c, b + d)),
        (DerivedOp::Sub, [(a, b), (c, d)]) => Some((a - d, b - c)),
        (DerivedOp::Mul, [(a, b), (c, d)]) => Some(hull([a * c, a * d, b * c, b * d])),
        (DerivedOp::Pow, [base, exponent]) => pow(*base, *exponent),
        (DerivedOp::Neg, [(a, b)]) => Some((-b, -a)),
        (DerivedOp::Abs, [(a, b)]) => Some(if *a >= 0 {
            (*a, *b)
        } else if *b <= 0 {
            (-b, -a)
        } else {
            (0, (-a).max(*b))
        }),
        (DerivedOp::BitNot, [(a, b)]) => Some((-b - 1, -a - 1)),
        (DerivedOp::Div | DerivedOp::IntDiv, [dividend, divisor]) => divide(*dividend, *divisor),
        (DerivedOp::Mod, [dividend, divisor]) => modulo(*dividend, *divisor),
        (DerivedOp::Shl, [value, by]) => shift_left(*value, *by),
        (DerivedOp::Shr, [value, by]) => shift_right(*value, *by),
        (DerivedOp::BitAnd, [(a, b), (c, d)]) => Some(match (*a >= 0, *c >= 0) {
            (true, true) => (0, (*b).min(*d)),
            (true, false) => (0, *b),
            (false, true) => (0, *d),
            (false, false) => (INT_MIN, (*b).max(*d)),
        }),
        (DerivedOp::BitOr | DerivedOp::BitXor, [(a, b), (c, d)]) => Some(if *a >= 0 && *c >= 0 {
            let top = (*b).max(*d);
            let bits = 128 - top.leading_zeros();
            (0, (1_i128 << bits) - 1)
        } else {
            (INT_MIN, INT_MAX)
        }),
        (DerivedOp::Min, all) if !all.is_empty() => Some((
            all.iter().map(|i| i.0).min().unwrap_or(0),
            all.iter().map(|i| i.1).min().unwrap_or(0),
        )),
        (DerivedOp::Max, all) if !all.is_empty() => Some((
            all.iter().map(|i| i.0).max().unwrap_or(0),
            all.iter().map(|i| i.1).max().unwrap_or(0),
        )),
        (DerivedOp::FloatToInt, [(a, b)]) => Some((
            (*a).clamp(INT_MIN, LARGEST_INT_OF_FLOAT),
            (*b).clamp(INT_MIN, LARGEST_INT_OF_FLOAT),
        )),
        _ => None,
    };
    match exact {
        Some((lo, hi)) => {
            let leaves = matches!(
                op,
                DerivedOp::Add
                    | DerivedOp::Sub
                    | DerivedOp::Mul
                    | DerivedOp::Pow
                    | DerivedOp::Neg
                    | DerivedOp::Abs
            ) && (lo < INT_MIN || hi > INT_MAX);
            (Value::Within(lo, hi).clamped(), leaves)
        }
        None => (Value::Within(INT_MIN, INT_MAX), false),
    }
}

/// How the float of an overflow arrives where it is used, and what is
/// rejected there.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Arrival {
    /// Every float, not only one a double doesn't convert to an `int`
    /// from: a `strict_types` typed target, or a float computed from by
    /// steps that aren't followed.
    every: bool,
    /// The steps it went through from its operation, in order.
    steps: Vec<(Step, (i64, i64))>,
}

impl Arrival {
    /// How the float of the `Overflow` node `overflow` arrives at a use
    /// that rejects `every_float`.
    fn of(overflow: u32, every_float: bool) -> Arrival {
        let mut arrival = Arrival {
            every: every_float,
            steps: Vec::new(),
        };
        let mut node = overflow;
        while let Some(FlowNode::Overflow { carry, .. }) = flow_nodes::node(node) {
            match carry {
                Carry::Step { step, with, from } => {
                    arrival.steps.push((step, with));
                    node = from;
                }
                Carry::Changed => {
                    arrival.every = true;
                    break;
                }
                Carry::Direct => break,
            }
        }
        arrival.steps.reverse();
        arrival
    }
}

/// `-2^63`, the least double that converts to an `int`; `2^63` is the
/// least past them.
const LEAST_CONVERTED: f64 = -9_223_372_036_854_775_808.0;

/// Whether `op` of `operands` can give a float that `arrival` takes to
/// where it is rejected. PHP gives the float only where the exact result
/// is past the `int` range, and computes it in doubles from the operands
/// converted to doubles: rounding is monotone, so the doubles it can be,
/// below the range and above it, reach from `∓2^63` to those at the ends
/// of the operands' ranges that are past it. Each step computes from
/// them as PHP does. With `every` any float that arrives is rejected,
/// else one past the range a double converts to an `int` within
/// (`-2^63` converts, `2^63` doesn't).
fn leaves(op: DerivedOp, operands: &[Value], arrival: &Arrival) -> bool {
    if !compute(op, operands).1 {
        return false;
    }
    let intervals: Vec<(i128, i128)> = operands.iter().filter_map(|v| v.interval()).collect();
    let double = |n: i128| n as f64;
    let past =
        |(exact, double): (i128, f64)| (!(INT_MIN..=INT_MAX).contains(&exact)).then_some(double);
    let ends: Vec<f64> = match (op, intervals.as_slice()) {
        (DerivedOp::Add, [(a, b), (c, d)]) => [
            (a + c, double(*a) + double(*c)),
            (b + d, double(*b) + double(*d)),
        ]
        .into_iter()
        .filter_map(past)
        .collect(),
        (DerivedOp::Sub, [(a, b), (c, d)]) => [
            (a - d, double(*a) - double(*d)),
            (b - c, double(*b) - double(*c)),
        ]
        .into_iter()
        .filter_map(past)
        .collect(),
        (DerivedOp::Mul, [(a, b), (c, d)]) => [(a, c), (a, d), (b, c), (b, d)]
            .into_iter()
            .map(|(x, y)| (x * y, double(*x) * double(*y)))
            .filter_map(past)
            .collect(),
        (DerivedOp::Neg, [(a, b)]) => [(-a, -double(*a)), (-b, -double(*b))]
            .into_iter()
            .filter_map(past)
            .collect(),
        (DerivedOp::Abs, [(a, b)]) => [(a.abs(), double(*a).abs()), (b.abs(), double(*b).abs())]
            .into_iter()
            .filter_map(past)
            .collect(),
        (DerivedOp::Pow, [(b_lo, b_hi), (e_lo, e_hi)]) => {
            let mut bases = vec![*b_lo, *b_hi];
            bases.extend([-1, 0, 1].into_iter().filter(|b| b_lo <= b && b <= b_hi));
            let e_lo = (*e_lo).max(0);
            let e_hi = (*e_hi).max(0);
            let mut exponents = vec![e_lo, e_hi];
            if e_lo < e_hi {
                exponents.extend([e_lo + 1, e_hi - 1]);
            }
            let clamp = |n: i128| i64::try_from(n.clamp(INT_MIN, INT_MAX)).unwrap_or(0);
            bases
                .iter()
                .flat_map(|&b| exponents.iter().map(move |&e| (b, e)))
                .filter_map(|(b, e)| php_pow(clamp(b), clamp(e)).err())
                .collect()
        }
        _ => vec![f64::NEG_INFINITY, f64::INFINITY],
    };
    let below = ends
        .iter()
        .filter(|&&d| d <= LEAST_CONVERTED)
        .fold(None, |least: Option<f64>, &d| {
            Some(least.map_or(d, |l| l.min(d)))
        });
    let above = ends
        .iter()
        .filter(|&&d| d > LEAST_CONVERTED)
        .fold(None, |most: Option<f64>, &d| {
            Some(most.map_or(d, |m| m.max(d)))
        });
    let mut parts: Vec<(f64, f64)> = below
        .map(|least| (least, LEAST_CONVERTED))
        .into_iter()
        .chain(above.map(|most| (-LEAST_CONVERTED, most)))
        .collect();
    for &(step, with) in &arrival.steps {
        parts = parts
            .into_iter()
            .filter_map(|part| step_doubles(step, with, part))
            .collect();
    }
    parts
        .iter()
        .any(|&(lo, hi)| arrival.every || lo < LEAST_CONVERTED || hi >= -LEAST_CONVERTED)
}

/// The doubles `step` gives from those in `lo..=hi` and an `int` within
/// `with`, as PHP computes them: `None` when none goes on (a `max()` or
/// `min()` that takes another value, a division by zero). A bound that
/// can't be told (`INF - INF`) leaves it unbounded.
fn step_doubles(step: Step, (w_lo, w_hi): (i64, i64), (lo, hi): (f64, f64)) -> Option<(f64, f64)> {
    let (w_lo, w_hi) = (w_lo as f64, w_hi as f64);
    let hull = |values: &[f64]| {
        if values.iter().any(|v| v.is_nan()) {
            return (f64::NEG_INFINITY, f64::INFINITY);
        }
        (
            values.iter().copied().fold(f64::INFINITY, f64::min),
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    };
    Some(match step {
        Step::Add => hull(&[lo + w_lo, hi + w_hi]),
        Step::Sub => hull(&[lo - w_hi, hi - w_lo]),
        Step::SubFrom => hull(&[w_lo - hi, w_hi - lo]),
        Step::Mul => hull(&[lo * w_lo, lo * w_hi, hi * w_lo, hi * w_hi]),
        Step::Div => {
            let mut divisors: Vec<f64> = [w_lo, w_hi, -1.0, 1.0]
                .into_iter()
                .filter(|&d| d != 0.0 && w_lo <= d && d <= w_hi)
                .collect();
            if divisors.is_empty() {
                return None;
            }
            divisors.dedup();
            let quotients: Vec<f64> = divisors.iter().flat_map(|&d| [lo / d, hi / d]).collect();
            hull(&quotients)
        }
        Step::Neg => (-hi, -lo),
        Step::Abs if lo >= 0.0 => (lo, hi),
        Step::Abs if hi <= 0.0 => (-hi, -lo),
        Step::Abs => (0.0, (-lo).max(hi)),
        Step::Max if hi < w_lo => return None,
        Step::Max => (lo.max(w_lo), hi),
        Step::Min if lo > w_hi => return None,
        Step::Min => (lo, hi.min(w_hi)),
    })
}

/// PHP's `**` of the `int`s `base` and `exponent >= 0`
/// (`pow_function_base`): the `int` while it stays in range, else the
/// double it gives from where it left it.
fn php_pow(base: i64, exponent: i64) -> Result<i64, f64> {
    if exponent == 0 {
        return Ok(1);
    }
    if base == 0 {
        return Ok(0);
    }
    let (mut l1, mut l2, mut i) = (1_i64, base, exponent);
    while i >= 1 {
        if i % 2 == 1 {
            i -= 1;
            match l1.checked_mul(l2) {
                Some(product) => l1 = product,
                None => {
                    let product = l1 as f64 * l2 as f64;
                    return Err(product * (l2 as f64).powf(i as f64));
                }
            }
        } else {
            i /= 2;
            match l2.checked_mul(l2) {
                Some(square) => l2 = square,
                None => {
                    let square = l2 as f64 * l2 as f64;
                    return Err(l1 as f64 * square.powf(i as f64));
                }
            }
        }
    }
    Ok(l1)
}

fn hull<const N: usize>(values: [i128; N]) -> (i128, i128) {
    (
        values.iter().copied().min().unwrap_or(0),
        values.iter().copied().max().unwrap_or(0),
    )
}

/// `base ** exponent` over the ranges, for a non-negative exponent: the
/// powers at the ends of the base's range and of `-1`, `0` and `1` when
/// it holds them, at the ends of the exponent's range and one past its
/// least (the other parity), held to `±2^127`.
fn pow((b_lo, b_hi): (i128, i128), (e_lo, e_hi): (i128, i128)) -> Option<(i128, i128)> {
    if e_hi < 0 {
        return Some((-1, 1));
    }
    let e_lo = e_lo.max(0);
    let mut bases = vec![b_lo, b_hi];
    for b in [-1, 0, 1] {
        if b_lo <= b && b <= b_hi {
            bases.push(b);
        }
    }
    let mut exponents = vec![e_lo, e_hi];
    if e_lo < e_hi {
        exponents.push(e_lo + 1);
        exponents.push(e_hi - 1);
    }
    let mut lo = i128::MAX;
    let mut hi = i128::MIN;
    for &b in &bases {
        for &e in &exponents {
            let value = match u32::try_from(e) {
                Ok(e) => b.checked_pow(e).unwrap_or(if b < 0 && e % 2 == 1 {
                    i128::MIN
                } else {
                    i128::MAX
                }),
                Err(_) if b.abs() <= 1 => b.pow(if e % 2 == 0 { 2 } else { 1 }),
                Err(_) => {
                    if b < 0 && e % 2 == 1 {
                        i128::MIN
                    } else {
                        i128::MAX
                    }
                }
            };
            lo = lo.min(value);
            hi = hi.max(value);
        }
    }
    Some((lo, hi))
}

/// The quotient over the ranges, truncated toward zero; `None` when the
/// divisor can only be `0`.
fn divide((n_lo, n_hi): (i128, i128), (d_lo, d_hi): (i128, i128)) -> Option<(i128, i128)> {
    let mut divisors = Vec::new();
    if d_lo <= -1 {
        divisors.push(d_lo);
        divisors.push(d_hi.min(-1));
    }
    if d_hi >= 1 {
        divisors.push(d_lo.max(1));
        divisors.push(d_hi);
    }
    if divisors.is_empty() {
        return None;
    }
    let mut lo = i128::MAX;
    let mut hi = i128::MIN;
    for &d in &divisors {
        for n in [n_lo, n_hi] {
            let q = n / d;
            lo = lo.min(q);
            hi = hi.max(q);
        }
    }
    Some((lo, hi))
}

/// The remainder over the ranges: smaller than the divisor's size, with
/// the dividend's sign.
fn modulo((n_lo, n_hi): (i128, i128), (d_lo, d_hi): (i128, i128)) -> Option<(i128, i128)> {
    let size = d_lo.abs().max(d_hi.abs());
    if size == 0 {
        return None;
    }
    let most = size - 1;
    Some(if n_lo >= 0 {
        (0, n_hi.min(most))
    } else if n_hi <= 0 {
        (n_lo.max(-most), 0)
    } else {
        (n_lo.max(-most), n_hi.min(most))
    })
}

/// `value << by`: exact when no bit leaves the `int`, any `int` otherwise
/// (PHP wraps).
fn shift_left((v_lo, v_hi): (i128, i128), (s_lo, s_hi): (i128, i128)) -> Option<(i128, i128)> {
    if s_hi < 0 {
        return None;
    }
    let (s_lo, s_hi) = (s_lo.max(0), s_hi);
    if s_hi >= 64 {
        return Some((INT_MIN, INT_MAX));
    }
    let corners = [v_lo << s_lo, v_lo << s_hi, v_hi << s_lo, v_hi << s_hi];
    let (lo, hi) = hull(corners);
    Some(if lo < INT_MIN || hi > INT_MAX {
        (INT_MIN, INT_MAX)
    } else {
        (lo, hi)
    })
}

/// `value >> by`, an arithmetic shift; one of 64 or more gives `0` or `-1`.
fn shift_right((v_lo, v_hi): (i128, i128), (s_lo, s_hi): (i128, i128)) -> Option<(i128, i128)> {
    if s_hi < 0 {
        return None;
    }
    let shift = |v: i128, s: i128| {
        if s >= 64 {
            if v < 0 { -1 } else { 0 }
        } else {
            v >> s
        }
    };
    let s_lo = s_lo.max(0);
    Some(hull([
        shift(v_lo, s_lo),
        shift(v_lo, s_hi),
        shift(v_hi, s_lo),
        shift(v_hi, s_hi),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made_at(allocs: &[Object]) -> Objects {
        Objects {
            allocs: allocs.to_vec(),
            open: false,
            any: false,
        }
    }

    #[test]
    fn joining_any_object_gives_any_object() {
        let mut objects = made_at(&[1, 2]);
        objects.join(&Objects::any());
        assert_eq!(objects, Objects::any());
    }

    #[test]
    fn joining_more_places_than_are_told_apart_gives_any_object() {
        let mut objects = made_at(&(0..20).collect::<Vec<_>>());
        objects.join(&made_at(&(10..40).collect::<Vec<_>>()));
        assert_eq!(objects, Objects::any());
    }

    #[test]
    fn joining_keeps_each_place_once_in_order() {
        let mut objects = made_at(&[1, 3, 5]);
        objects.join(&made_at(&[2, 3, 6]));
        assert_eq!(objects, made_at(&[1, 2, 3, 5, 6]));
    }

    fn at(n: i128) -> Value {
        Value::Within(n, n)
    }

    const COERCED: Arrival = Arrival {
        every: false,
        steps: Vec::new(),
    };

    fn through(every: bool, steps: &[(Step, (i64, i64))]) -> Arrival {
        Arrival {
            every,
            steps: steps.to_vec(),
        }
    }

    #[test]
    fn a_subtraction_just_below_the_int_range_gives_a_float_that_converts() {
        let sub = |a: i128, b: i128| leaves(DerivedOp::Sub, &[at(a), at(b)], &COERCED);
        assert!(!sub(INT_MIN, 3));
        assert!(!sub(INT_MIN, 1024));
        assert!(sub(INT_MIN, 1025));
        assert!(sub(INT_MIN + 500, 1524));
        assert!(!sub(INT_MIN + 600, 1624));
        assert!(sub(INT_MAX, -1));
        let strict = through(true, &[]);
        assert!(leaves(DerivedOp::Sub, &[at(INT_MIN), at(3)], &strict));
    }

    #[test]
    fn only_the_end_past_the_int_range_gives_a_float() {
        let operands = [Value::Within(INT_MIN, INT_MAX), at(1)];
        assert!(!leaves(DerivedOp::Sub, &operands, &COERCED));
        assert!(leaves(DerivedOp::Add, &operands, &COERCED));
    }

    #[test]
    fn a_float_computed_from_is_followed_as_php_computes_it() {
        let after = |step| {
            leaves(
                DerivedOp::Sub,
                &[at(INT_MIN), at(3)],
                &through(false, &[step]),
            )
        };
        assert!(!after((Step::Sub, (3, 3))));
        assert!(after((Step::Mul, (-1, -1))));
        assert!(after((Step::Sub, (4096, 4096))));
        assert!(!after((Step::Div, (2, 2))));
        assert!(after((Step::Abs, (0, 0))));
    }

    #[test]
    fn max_and_min_beside_an_int_keep_a_float_only_past_it() {
        let operands = [Value::Within(INT_MIN, INT_MAX), at(4096)];
        let max = through(true, &[(Step::Max, (1, 1))]);
        let min = through(true, &[(Step::Min, (1, 1))]);
        assert!(!leaves(DerivedOp::Sub, &operands, &max));
        assert!(leaves(DerivedOp::Sub, &operands, &min));
        assert!(leaves(DerivedOp::Add, &operands, &max));
        assert!(!leaves(DerivedOp::Add, &operands, &min));
    }

    #[test]
    fn a_float_computed_from_by_steps_not_followed_is_judged_by_its_exact_result() {
        let changed = through(true, &[]);
        assert!(leaves(DerivedOp::Sub, &[at(INT_MIN), at(3)], &changed));
    }

    #[test]
    fn negating_or_taking_the_absolute_value_of_php_int_min_gives_2_63() {
        assert!(leaves(DerivedOp::Neg, &[at(INT_MIN)], &COERCED));
        assert!(leaves(DerivedOp::Abs, &[at(INT_MIN)], &COERCED));
    }

    #[test]
    fn a_power_is_computed_as_php_computes_it() {
        assert_eq!(php_pow(2, 62), Ok(1 << 62));
        assert_eq!(php_pow(-2, 63), Ok(i64::MIN));
        assert_eq!(php_pow(2, 63), Err(2.0_f64.powi(63)));
        assert_eq!(php_pow(3, 0), Ok(1));
        assert!(leaves(DerivedOp::Pow, &[at(2), at(63)], &COERCED));
        assert!(!leaves(DerivedOp::Pow, &[at(-2), at(63)], &COERCED));
    }
}
