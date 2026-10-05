//! How chosen objects reach the nodes holding them: for each node and
//! each object traced, the node its first pointer to the object came
//! from, so that the path from where the object's address is taken to a
//! node holding it can be told.

use std::fmt::Write;

use rustc_hash::{FxHashMap, FxHashSet};

use super::bitmap::{EMPTY, SetId};
use super::{COPY, LoadKey, LocId, MEMORY, NONE, NodeId, ObjId, Pta, TrId};

/// How many nodes of a path the report prints at most.
const PATH_LIMIT: usize = 400;
/// How many names of a node the report prints.
const NAMES_SHOWN: usize = 8;
/// How many of an object's first arrivals the report lists.
const ARRIVALS_SHOWN: usize = 48;
/// How many nodes a name asked about the paths are traced into.
const AT_SHOWN: usize = 4;
/// How many locations a node holds to be among the large.
const LARGE: u32 = 2048;
/// How many of the first nodes to grow large the report lists.
const GROWN_SHOWN: usize = 40;

/// A path traced into a node: what the node is, the path, and whether it
/// is whole.
type Traced = (String, Vec<NodeId>, bool);

/// What the report traces: the objects, by texts their names hold, and
/// the nodes the paths are traced into besides the one holding the most.
#[derive(Clone, Default)]
pub(crate) struct Asked {
    /// The texts an object's name holds to be traced.
    pub(crate) objects: Vec<String>,
    /// The texts of the calls through pointers whose callees the paths
    /// are traced into.
    pub(crate) into: Vec<String>,
    /// The texts of the names of the nodes the paths are traced into.
    pub(crate) at: Vec<String>,
    /// Where the sources of the large sets are written.
    pub(crate) blame: Option<std::path::PathBuf>,
}

pub(crate) struct Trace {
    asked: Asked,
    /// Whether each object asked about is traced.
    known: FxHashMap<ObjId, bool>,
    pub(crate) objects: Vec<ObjId>,
    /// The locations of the traced objects.
    pub(crate) locs: SetId,
    /// The locations made since the traced ones were last gathered.
    pending: Vec<LocId>,
    /// The node each traced object first reached a node from; `NONE` for
    /// a pointer the node holds from the start.
    came: FxHashMap<(NodeId, ObjId), NodeId>,
    /// Each traced object's first arrivals in order: the node it came
    /// from (`NONE` for a pointer held from the start) and the node it
    /// reached.
    arrivals: FxHashMap<ObjId, Vec<(NodeId, NodeId)>>,
    /// The first nodes to hold `LARGE` locations, in order.
    grown: Vec<Grown>,
}

/// A node grown large through an edge: the nodes the edge joins, its move,
/// what it passed on, and how many nodes had been processed.
struct Grown {
    from: NodeId,
    to: NodeId,
    tr: TrId,
    gained: String,
    processed: u64,
}

impl Trace {
    pub(crate) fn new(asked: Asked) -> Trace {
        Trace {
            asked,
            known: FxHashMap::default(),
            objects: Vec::new(),
            locs: EMPTY,
            pending: Vec::new(),
            came: FxHashMap::default(),
            arrivals: FxHashMap::default(),
            grown: Vec::new(),
        }
    }

    /// Notes an object reaching a node from another, where it had not
    /// reached it before.
    fn arrive(&mut self, from: NodeId, to: NodeId, obj: ObjId) {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.came.entry((to, obj)) {
            entry.insert(from);
            let arrivals = self.arrivals.entry(obj).or_default();
            if arrivals.len() < ARRIVALS_SHOWN {
                arrivals.push((from, to));
            }
        }
    }
}

impl Pta<'_> {
    /// Notes a location made, to be told traced or not when next asked.
    pub(crate) fn trace_loc(&mut self, loc: LocId) {
        if let Some(trace) = self.trace.as_deref_mut() {
            trace.pending.push(loc);
        }
    }

    /// Whether an object is traced: its name holds one of the texts.
    fn traced(&mut self, obj: ObjId) -> bool {
        let Some(trace) = self.trace.as_deref() else {
            return false;
        };
        if let Some(&known) = trace.known.get(&obj) {
            return known;
        }
        let name = self.object_name(obj);
        let Some(trace) = self.trace.as_deref_mut() else {
            return false;
        };
        let traced = trace
            .asked
            .objects
            .iter()
            .any(|pattern| name.contains(pattern.as_str()));
        trace.known.insert(obj, traced);
        if traced {
            trace.objects.push(obj);
        }
        traced
    }

    /// Gathers the traced objects' locations made since last asked.
    fn gather_traced(&mut self) {
        let Some(trace) = self.trace.as_deref_mut() else {
            return;
        };
        let pending = std::mem::take(&mut trace.pending);
        for loc in pending {
            let obj = self.locs[loc as usize].0;
            if self.traced(obj)
                && let Some(trace) = self.trace.as_deref_mut()
            {
                trace.locs = self.sets.insert(trace.locs, loc);
            }
        }
    }

    /// The traced objects' locations a set holds.
    pub(crate) fn traced_in(&mut self, set: SetId) -> SetId {
        if self.trace.is_none() || set == EMPTY {
            return EMPTY;
        }
        self.gather_traced();
        let Some(trace) = self.trace.as_deref() else {
            return EMPTY;
        };
        let locs = trace.locs;
        self.sets.intersection(set, locs)
    }

    /// Notes the traced objects of a set as reaching `to` from `from`,
    /// where they had not reached it before.
    pub(crate) fn note_arrivals(&mut self, from: NodeId, to: NodeId, traced: SetId) {
        if traced == EMPTY {
            return;
        }
        let to = self.find(to);
        let mut objects: Vec<ObjId> = self
            .sets
            .elements(traced)
            .into_iter()
            .map(|loc| self.locs[loc as usize].0)
            .collect();
        objects.dedup();
        if let Some(trace) = self.trace.as_deref_mut() {
            for obj in objects {
                trace.arrive(from, to, obj);
            }
        }
    }

    /// Notes a pointer a node holds from the start.
    pub(crate) fn note_initial(&mut self, node: NodeId, loc: LocId) {
        if self.trace.is_none() {
            return;
        }
        let obj = self.locs[loc as usize].0;
        if self.traced(obj)
            && let Some(trace) = self.trace.as_deref_mut()
        {
            trace.locs = self.sets.insert(trace.locs, loc);
            trace.arrive(NONE, node, obj);
        }
    }

    /// The size of a node's set, where the trace watches it grow large or
    /// the sources of the sets are asked.
    pub(crate) fn growth_watch(&self, node: NodeId) -> Option<u32> {
        let growing = self
            .trace
            .as_deref()
            .is_some_and(|trace| trace.grown.len() < GROWN_SHOWN);
        if !growing && self.blame.is_none() {
            return None;
        }
        Some(self.sets.len(self.pts[self.find_const(node) as usize]))
    }

    /// Notes what an edge passing on a set added to a watched node, and
    /// the node growing large through it.
    pub(crate) fn note_growth(
        &mut self,
        from: NodeId,
        to: NodeId,
        tr: TrId,
        passed: SetId,
        watched: Option<u32>,
    ) {
        let Some(before) = watched else {
            return;
        };
        let (from, to) = (self.find(from), self.find(to));
        let held = self.sets.len(self.pts[to as usize]);
        self.vote(from, to, held.saturating_sub(before));
        let growing = self
            .trace
            .as_deref()
            .is_some_and(|trace| trace.grown.len() < GROWN_SHOWN);
        if !growing || before >= LARGE || held < LARGE {
            return;
        }
        let gained = self.set_summary(passed);
        let processed = self.stats.solve.processed;
        if let Some(trace) = self.trace.as_deref_mut() {
            trace.grown.push(Grown {
                from,
                to,
                tr,
                gained,
                processed,
            });
        }
    }

    /// A set's objects by kind, and those it holds at the most offsets.
    fn set_summary(&self, set: SetId) -> String {
        let mut offsets: FxHashMap<ObjId, usize> = FxHashMap::default();
        for loc in self.sets.elements(set) {
            *offsets.entry(self.locs[loc as usize].0).or_default() += 1;
        }
        let mut kinds: std::collections::BTreeMap<&'static str, usize> = Default::default();
        for &obj in offsets.keys() {
            *kinds
                .entry(super::report::kind_word(&self.objects[obj as usize].kind))
                .or_default() += 1;
        }
        let mut most: Vec<(usize, ObjId)> = offsets.iter().map(|(&obj, &n)| (n, obj)).collect();
        most.sort_unstable_by_key(|&(n, obj)| std::cmp::Reverse((n, obj)));
        let kinds: Vec<String> = kinds
            .iter()
            .map(|(kind, n)| format!("{n} {kind}"))
            .collect();
        let most: Vec<String> = most
            .iter()
            .take(4)
            .map(|&(n, obj)| format!("{} ({n})", self.object_name(obj)))
            .collect();
        format!(
            "{} locations: {}; most: {}",
            self.sets.len(set),
            kinds.join(", "),
            most.join(", ")
        )
    }

    /// Keeps, for a node merged into another, how the traced objects
    /// reached it.
    pub(crate) fn note_merge(&mut self, into: NodeId, merged: NodeId) {
        let Some(trace) = self.trace.as_deref_mut() else {
            return;
        };
        for &obj in &trace.objects {
            if let Some(&from) = trace.came.get(&(merged, obj)) {
                trace.came.entry((into, obj)).or_insert(from);
            }
        }
    }

    /// For each traced object: how many nodes it reached, the first it
    /// reached in order, and the paths by which it reached the one of
    /// them holding the most, the nodes of the names asked about and the
    /// callees of the calls asked about.
    pub(crate) fn report_trace(&mut self, out: &mut String) {
        if self.trace.is_none() {
            return;
        }
        self.gather_traced();
        let Some(trace) = self.trace.take() else {
            return;
        };
        let mut reached: FxHashMap<ObjId, FxHashSet<NodeId>> = FxHashMap::default();
        for &(node, obj) in trace.came.keys() {
            reached
                .entry(obj)
                .or_default()
                .insert(self.find_const(node));
        }
        let calls: Vec<(String, NodeId)> = self
            .sites
            .iter()
            .enumerate()
            .filter_map(|(i, site)| {
                let callee = site.callee?;
                let text = self.describe(i as super::SiteId);
                trace
                    .asked
                    .into
                    .iter()
                    .any(|into| text.contains(into.as_str()))
                    .then(|| (format!("the callee of {text}"), self.find_const(callee)))
            })
            .collect();
        let mut named: Vec<(usize, String, NodeId)> = Vec::new();
        if !trace.asked.at.is_empty() {
            let mut seen: FxHashSet<(usize, NodeId)> = FxHashSet::default();
            self.visit_names(true, &mut |node, text| {
                let text = text();
                for (i, at) in trace.asked.at.iter().enumerate() {
                    if text.contains(at.as_str()) && seen.insert((i, node)) {
                        named.push((i, text.clone(), node));
                    }
                }
            });
        }
        let mut paths: Vec<(ObjId, usize, Vec<Traced>)> = Vec::new();
        for &obj in &trace.objects {
            let nodes = reached.remove(&obj).unwrap_or_default();
            let mut targets: Vec<(String, NodeId)> = Vec::new();
            if let Some(&most) = nodes
                .iter()
                .max_by_key(|&&node| (self.sets.len(self.pts[node as usize]), node))
            {
                targets.push(("the node holding the most".to_owned(), most));
            }
            let mut shown = vec![0; trace.asked.at.len()];
            for (i, label, node) in &named {
                if nodes.contains(node) && shown[*i] < AT_SHOWN {
                    shown[*i] += 1;
                    targets.push((label.clone(), *node));
                }
            }
            for (label, callee) in &calls {
                if nodes.contains(callee) {
                    targets.push((label.clone(), *callee));
                }
            }
            let traced = targets
                .into_iter()
                .map(|(label, target)| {
                    let (path, whole) = self.trace_path(&trace, obj, target);
                    (label, path, whole)
                })
                .collect();
            paths.push((obj, nodes.len(), traced));
        }
        let mut wanted: FxHashSet<NodeId> = paths
            .iter()
            .flat_map(|(_, _, traced)| traced.iter().flat_map(|(_, path, _)| path.iter().copied()))
            .collect();
        for arrivals in trace.arrivals.values() {
            for &(from, to) in arrivals {
                wanted.insert(self.find_const(to));
                if from != NONE {
                    wanted.insert(self.find_const(from));
                }
            }
        }
        let names = self.node_names(&wanted);
        let _ = writeln!(out, "traced objects:");
        for (obj, count, traced) in paths {
            let _ = writeln!(out, "  {} reaches {count} nodes", self.object_name(obj));
            let _ = writeln!(out, "   the first nodes it reached:");
            for &(from, to) in trace.arrivals.get(&obj).map_or(&[][..], Vec::as_slice) {
                let to = self.find_const(to);
                if from == NONE {
                    let _ = writeln!(out, "    {}, from the start", self.node_row(to, &names));
                    continue;
                }
                let from = self.find_const(from);
                let _ = writeln!(
                    out,
                    "    {}{}, from {from}",
                    self.node_row(to, &names),
                    self.moved(from, to)
                );
            }
            for (label, path, whole) in traced {
                let _ = writeln!(
                    out,
                    "   into {label}{}:",
                    if whole {
                        ""
                    } else {
                        ", the path cut where it closes a cycle"
                    }
                );
                self.print_path(out, &path, &names);
            }
        }
        if !trace.grown.is_empty() {
            let wanted: FxHashSet<NodeId> = trace
                .grown
                .iter()
                .flat_map(|grown| [grown.from, grown.to])
                .collect();
            let own = self.names_of(false, &wanted);
            let row = |node: NodeId| {
                let names = own.get(&node).map_or(&[][..], Vec::as_slice);
                let shown: Vec<&str> = names.iter().take(NAMES_SHOWN).map(String::as_str).collect();
                format!(
                    "{node} {}{}",
                    if shown.is_empty() {
                        "(unnamed)".to_owned()
                    } else {
                        shown.join(" | ")
                    },
                    if names.len() > NAMES_SHOWN {
                        format!(" | {} more", names.len() - NAMES_SHOWN)
                    } else {
                        String::new()
                    }
                )
            };
            let _ = writeln!(out, "the first nodes to hold {LARGE} locations:");
            for grown in &trace.grown {
                let _ = writeln!(
                    out,
                    "  after {} nodes processed: {}",
                    grown.processed,
                    row(grown.to)
                );
                let moved = if grown.tr == COPY {
                    String::new()
                } else {
                    format!(" moved {:?}", self.trs[grown.tr as usize])
                };
                let _ = writeln!(out, "    from {}{moved}", row(grown.from));
                let _ = writeln!(out, "    gaining {}", grown.gained);
            }
        }
        self.trace = Some(trace);
    }

    /// The path by which an object first reached a node, from where it
    /// started; and whether it is whole, not cut where it closes a cycle.
    fn trace_path(&self, trace: &Trace, obj: ObjId, target: NodeId) -> (Vec<NodeId>, bool) {
        let mut path = vec![target];
        let mut seen: FxHashSet<NodeId> = FxHashSet::default();
        seen.insert(target);
        let mut cur = target;
        let mut whole = false;
        while path.len() < PATH_LIMIT {
            match trace.came.get(&(cur, obj)) {
                Some(&NONE) => {
                    whole = true;
                    break;
                }
                Some(&from) => {
                    let from = self.find_const(from);
                    if !seen.insert(from) {
                        break;
                    }
                    path.push(from);
                    cur = from;
                }
                None => break,
            }
        }
        path.reverse();
        (path, whole)
    }

    fn print_path(
        &self,
        out: &mut String,
        path: &[NodeId],
        names: &FxHashMap<NodeId, Vec<String>>,
    ) {
        let mut prev: Option<NodeId> = None;
        for &node in path {
            let moved = prev.map_or(String::new(), |from| self.moved(from, node));
            let row = self.node_row(node, names);
            let _ = match row.split_once(' ') {
                Some((head, rest)) if !moved.is_empty() => {
                    writeln!(out, "    {head}{moved} {rest}")
                }
                _ => writeln!(out, "    {row}"),
            };
            prev = Some(node);
        }
    }

    /// How an edge from one node to another moves the pointers it
    /// carries; nothing for a plain copy or no edge.
    fn moved(&self, from: NodeId, to: NodeId) -> String {
        self.succ[from as usize]
            .iter()
            .find(|&&(next, _)| self.find_const(next) == to)
            .map_or(String::new(), |&(_, tr)| {
                if tr == COPY {
                    String::new()
                } else {
                    format!(" moved {:?}", self.trs[tr as usize])
                }
            })
    }

    /// A node, the size of its set and its names.
    fn node_row(&self, node: NodeId, names: &FxHashMap<NodeId, Vec<String>>) -> String {
        let own = names.get(&node);
        let shown: Vec<&str> = own
            .map(|names| names.iter().take(NAMES_SHOWN).map(String::as_str).collect())
            .unwrap_or_default();
        let more = own.map_or(0, |names| names.len().saturating_sub(NAMES_SHOWN));
        format!(
            "{:>9} [{}] {}{}",
            node,
            self.sets.len(self.pts[node as usize]),
            if shown.is_empty() {
                "(unnamed)".to_owned()
            } else {
                shown.join(" | ")
            },
            if more > 0 {
                format!(" | {more} more")
            } else {
                String::new()
            }
        )
    }

    /// What each wanted node stands for: the variables, results, cells,
    /// readers and places of libraries and calls merged into it.
    pub(crate) fn node_names(&self, wanted: &FxHashSet<NodeId>) -> FxHashMap<NodeId, Vec<String>> {
        self.names_of(true, wanted)
    }

    /// The names of each wanted node, with those merged into it where
    /// `merged` holds, else its own.
    fn names_of(&self, merged: bool, wanted: &FxHashSet<NodeId>) -> FxHashMap<NodeId, Vec<String>> {
        let mut names: FxHashMap<NodeId, Vec<String>> = FxHashMap::default();
        self.visit_names(merged, &mut |node, text| {
            if wanted.contains(&node) {
                names.entry(node).or_default().push(text());
            }
        });
        names
    }

    /// Gives each name of each node, with the node it was merged into
    /// where `merged` holds.
    pub(crate) fn visit_names(
        &self,
        merged: bool,
        visit: &mut dyn FnMut(NodeId, &dyn Fn() -> String),
    ) {
        let mut name = |node: NodeId, text: &dyn Fn() -> String| {
            if node != NONE {
                visit(if merged { self.find_const(node) } else { node }, text);
            }
        };
        for inst in &self.insts {
            let def = &self.defs[inst.def as usize];
            let function = &self.fn_names[def.fn_id as usize];
            for (var, &slot) in def.slots.iter().enumerate() {
                if slot & MEMORY == 0 {
                    name(inst.registers + slot, &|| {
                        format!("variable {var} of {function}")
                    });
                }
            }
            name(inst.ret_node, &|| format!("result of {function}"));
        }
        for (obj, object) in self.objects.iter().enumerate() {
            let Some(memory) = object.memory.as_deref() else {
                continue;
            };
            let obj = obj as ObjId;
            let object_name = || self.object_name(obj);
            for (&(at, tag, width), &node) in &memory.exact_cells {
                name(node, &|| {
                    format!(
                        "cell of {} +{at} ({width} bytes, part {tag})",
                        object_name()
                    )
                });
            }
            for cell in &memory.loose_cells {
                name(cell.node, &|| {
                    format!(
                        "cell of {} {:?} ({} bytes, part {}, window {:?})",
                        object_name(),
                        cell.off,
                        cell.width,
                        cell.tag,
                        cell.window
                    )
                });
            }
            for (&(at, tag, width), &node) in &memory.exact_readers {
                name(node, &|| {
                    format!(
                        "reader of {} +{at} ({width} bytes, context {tag})",
                        object_name()
                    )
                });
            }
            for reader in &memory.loose_readers {
                name(reader.node, &|| {
                    format!(
                        "reader of {} {:?} ({} bytes, context {}, window {:?})",
                        object_name(),
                        reader.off,
                        reader.width,
                        reader.tag,
                        reader.window
                    )
                });
            }
            name(memory.any_out, &|| {
                format!("every cell of {} at any offset", object_name())
            });
            name(memory.any_in, &|| {
                format!("the cells of {} at any offset", object_name())
            });
        }
        for (&library, state) in &self.libs {
            let library = self.library_name(library);
            for (&(class, extent), &node) in &state.held {
                name(node, &|| {
                    format!(
                        "what {library} holds at {} ({extent:?})",
                        self.class_name(class)
                    )
                });
            }
            for (&class, &node) in &state.gives {
                name(node, &|| {
                    format!("what {library} gives at {}", self.class_name(class))
                });
            }
        }
        for (&loc, &node) in &self.const_nodes {
            let (obj, off) = self.locs[loc as usize];
            name(node, &|| {
                format!("the address of {} {off:?}", self.object_name(obj))
            });
        }
        for (&(base, tr), &node) in &self.shifted {
            name(node, &|| {
                format!(
                    "node {} moved {:?}",
                    self.find_const(base),
                    self.trs[tr as usize]
                )
            });
        }
        for (key, &node) in &self.loads {
            match *key {
                LoadKey::Node(base, tr, width, ctx) => name(node, &|| {
                    format!(
                        "{width} bytes read through node {} moved {:?} (context {ctx})",
                        self.find_const(base),
                        self.trs[tr as usize]
                    )
                }),
                LoadKey::Loc(loc, width, ctx) => {
                    let (obj, off) = self.locs[loc as usize];
                    name(node, &|| {
                        format!(
                            "{width} bytes read at {} {off:?} (context {ctx})",
                            self.object_name(obj)
                        )
                    });
                }
            }
        }
        for (i, site) in self.sites.iter().enumerate() {
            let site_id = i as super::SiteId;
            if let Some(callee) = site.callee {
                name(callee, &|| format!("callee of {}", self.describe(site_id)));
            }
            for (a, arg) in site.args.iter().enumerate() {
                name(arg.val, &|| {
                    format!("argument {a} of {}", self.describe(site_id))
                });
            }
        }
        name(self.outside_node, &|| "the outside value".to_owned());
        for &(when, node) in &self.kept_members {
            name(node, &|| format!("functions kept for {when:?}"));
        }
    }

    /// What each node is, by the first of the kinds merged into it found.
    pub(crate) fn node_kinds(&self) -> Vec<NodeKind> {
        let mut kinds = vec![NodeKind::Other; self.rep.len()];
        let mut mark = |node: NodeId, kind: NodeKind| {
            if node == NONE {
                return;
            }
            let at = self.find_const(node) as usize;
            if kinds[at] == NodeKind::Other {
                kinds[at] = kind;
            }
        };
        for object in &self.objects {
            let Some(memory) = object.memory.as_deref() else {
                continue;
            };
            for &node in memory.exact_cells.values() {
                mark(node, NodeKind::Cell);
            }
            for cell in &memory.loose_cells {
                mark(cell.node, NodeKind::LooseCell);
            }
            for &node in memory.exact_readers.values() {
                mark(node, NodeKind::Reader);
            }
            for reader in &memory.loose_readers {
                mark(reader.node, NodeKind::LooseReader);
            }
            mark(memory.any_out, NodeKind::AnyCells);
            mark(memory.any_in, NodeKind::AnyCells);
        }
        for inst in &self.insts {
            let def = &self.defs[inst.def as usize];
            for &slot in def.slots.iter() {
                if slot & MEMORY == 0 {
                    mark(inst.registers + slot, NodeKind::Register);
                }
            }
            mark(inst.ret_node, NodeKind::Result);
        }
        for state in self.libs.values() {
            for &node in state.held.values().chain(state.gives.values()) {
                mark(node, NodeKind::Library);
            }
        }
        for &node in self.const_nodes.values() {
            mark(node, NodeKind::Address);
        }
        for &node in self.loads.values() {
            mark(node, NodeKind::Read);
        }
        for &node in self.shifted.values() {
            mark(node, NodeKind::Moved);
        }
        for site in &self.sites {
            for arg in &site.args {
                mark(arg.val, NodeKind::Argument);
            }
        }
        kinds
    }
}

/// What a node is, for the report of the graph's edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum NodeKind {
    Other,
    Cell,
    LooseCell,
    Reader,
    LooseReader,
    AnyCells,
    Register,
    Result,
    Library,
    Address,
    Read,
    Moved,
    Argument,
}
