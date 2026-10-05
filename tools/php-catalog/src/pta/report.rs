//! The call graph's components, each loaded from the store callees first
//! as the summaries will read them, and what the analysis found.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::time::Instant;

use rustc_hash::{FxHashMap, FxHashSet};

use super::bitmap::EMPTY;
use super::solve::strong_components;
use super::tags::OTHER;
use super::trace::NodeKind;
use super::{
    DefId, FnId, Later, LocId, NodeId, ObjId, ObjKind, Off, Pta, SiteId, SiteKind, StoreAt,
    memory_use, off_sub,
};
use crate::ir::{Callee, Function, Stmt};
use crate::store::Store;

/// The call graph's components as loaded.
pub(crate) struct Components {
    pub(crate) count: usize,
    /// How many components have each size, by a bucket of sizes.
    pub(crate) sizes: BTreeMap<usize, usize>,
    pub(crate) largest: Vec<DefId>,
    pub(crate) largest_bytes: u64,
    pub(crate) bytes: u64,
    pub(crate) seconds: f64,
    /// The memory in use at its highest while one component was loaded,
    /// over what was in use before.
    pub(crate) most_loaded: u64,
}

/// A count's bucket for a histogram: 0, 1, then powers of 4.
fn bucket(count: usize) -> usize {
    match count {
        0 => 0,
        1 => 1,
        _ => {
            let mut bound = 4;
            while bound < count {
                bound *= 4;
            }
            bound
        }
    }
}

fn histogram(out: &mut String, title: &str, sizes: &BTreeMap<usize, usize>) {
    let _ = writeln!(out, "{title}:");
    let mut low = 0;
    for (&bound, &count) in sizes {
        let label = match bound {
            0 => "0".to_owned(),
            1 => "1".to_owned(),
            _ => format!("{}-{bound}", low + 1),
        };
        low = bound;
        let _ = writeln!(out, "  {label:>16}: {count}");
    }
}

impl Pta<'_> {
    /// Loads the definitions component by component of the call graph the
    /// analysis bound, callees first.
    pub(crate) fn components(&mut self, store: &Store) -> Result<Components, String> {
        let start = Instant::now();
        let count = self.defs.len();
        let mut callees: Vec<Vec<DefId>> = vec![Vec::new(); count];
        let mut edges: Vec<(DefId, DefId)> = self.call_edges.iter().copied().collect();
        edges.sort_unstable();
        for (from, to) in edges {
            callees[from as usize].push(to);
        }
        let mut components = Components {
            count: 0,
            sizes: BTreeMap::new(),
            largest: Vec::new(),
            largest_bytes: 0,
            bytes: 0,
            seconds: 0.0,
            most_loaded: 0,
        };
        let (members, starts) = strong_components(&callees);
        for c in 0..starts.len() - 1 {
            let component = &members[starts[c] as usize..starts[c + 1] as usize];
            self.load_component(store, component, &mut components)?;
        }
        components.seconds = start.elapsed().as_secs_f64();
        Ok(components)
    }

    fn load_component(
        &mut self,
        store: &Store,
        component: &[DefId],
        components: &mut Components,
    ) -> Result<(), String> {
        let (before, _) = memory_use();
        let mut bodies: Vec<Function> = Vec::with_capacity(component.len());
        let mut bytes = 0;
        for &def in component {
            let hash = self.defs[def as usize].hash;
            bodies.push(store.get(hash)?);
            bytes += store.size(hash);
        }
        if component.len() > 1 {
            let (now, _) = memory_use();
            components.most_loaded = components.most_loaded.max(now.saturating_sub(before));
        }
        drop(bodies);
        components.count += 1;
        components.bytes += bytes;
        *components.sizes.entry(bucket(component.len())).or_default() += 1;
        if component.len() > components.largest.len() {
            components.largest = component.to_vec();
            components.largest_bytes = bytes;
        }
        if components.count.is_multiple_of(4096) {
            self.check_memory()?;
        }
        Ok(())
    }

    /// The functions a site may call.
    fn site_functions(&self, site: SiteId) -> Vec<FnId> {
        self.sites[site as usize]
            .callee
            .map_or_else(Vec::new, |callee| self.functions(callee))
    }

    pub(crate) fn describe(&self, site: SiteId) -> String {
        let site = &self.sites[site as usize];
        match site.caller {
            Some(inst) => {
                let def = &self.defs[self.insts[inst as usize].def as usize];
                format!(
                    "{}:{} {}",
                    def.file, site.line, self.fn_names[def.fn_id as usize]
                )
            }
            None => format!("{:?}", site.kind),
        }
    }

    /// The callee expression of a call through a pointer, as the source
    /// writes it.
    fn callee_text(&self, store: &Store, site: SiteId) -> String {
        let site = &self.sites[site as usize];
        let Some(inst) = site.caller else {
            return String::new();
        };
        let def = &self.defs[self.insts[inst as usize].def as usize];
        let Ok(function) = store.get::<Function>(def.hash) else {
            return String::new();
        };
        for block in &function.blocks {
            for stmt in &block.stmts {
                if let Stmt::Call {
                    callee: Callee::Indirect(expr),
                    line,
                    ..
                } = stmt
                    && *line == site.line
                {
                    return crate::print::expr(&function, expr);
                }
            }
        }
        String::new()
    }

    /// The locations a store writes, as objects and offsets.
    fn store_targets(&self, at: StoreAt, out: &mut Vec<(ObjId, Off)>) {
        match at {
            StoreAt::Loc(loc) => out.push(self.locs[loc as usize]),
            StoreAt::Node(node, tr) => {
                let t = self.trs[tr as usize];
                let set = self.pts[self.find_const(node) as usize];
                let mut locs: Vec<LocId> = Vec::new();
                self.sets.extend_into(set, &mut locs);
                for loc in locs {
                    let (obj, off) = self.locs[loc as usize];
                    out.push((obj, t.apply(off)));
                }
            }
        }
    }

    /// The writes not through a key's members that may write a byte some
    /// write of a key's writes: their count, and some of them.
    fn tag_overlaps(&self) -> (usize, usize, Vec<String>) {
        let mut positions: FxHashMap<ObjId, Vec<Off>> = FxHashMap::default();
        let mut targets = Vec::new();
        for &(at, byte) in &self.tag_writes {
            targets.clear();
            self.store_targets(at, &mut targets);
            for &(obj, off) in &targets {
                let position = super::off_add(off, Off::Exact(i64::from(byte)));
                let list = positions.entry(obj).or_default();
                if !list.contains(&position) {
                    list.push(position);
                }
            }
        }
        let mut count = 0;
        let mut objects: FxHashSet<ObjId> = FxHashSet::default();
        let mut examples = Vec::new();
        for store in &self.plain_stores {
            targets.clear();
            self.store_targets(store.at, &mut targets);
            let width = i64::from(store.width);
            let hit = targets.iter().find(|&&(obj, off)| {
                positions.get(&obj).is_some_and(|list| {
                    list.iter().any(|&position| match off_sub(position, off) {
                        Off::Exact(d) => (0..width).contains(&d),
                        Off::Stride { rem, .. } => i64::from(rem) < width,
                        Off::Any => true,
                    })
                })
            });
            if let Some(&(obj, _)) = hit {
                count += 1;
                objects.insert(obj);
                if examples.len() < 40 {
                    let def = &self.defs[self.insts[store.inst as usize].def as usize];
                    examples.push(format!(
                        "{}:{} {} ({} bytes, into {})",
                        def.file,
                        store.line,
                        self.fn_names[def.fn_id as usize],
                        store.width,
                        self.object_name(obj)
                    ));
                }
            }
        }
        (count, objects.len(), examples)
    }

    pub(crate) fn object_name(&self, obj: ObjId) -> String {
        match self.objects[obj as usize].kind {
            ObjKind::Function(f) => format!("code of {}", self.fn_names[f as usize]),
            ObjKind::Var(inst, var) => {
                let def = &self.defs[self.insts[inst as usize].def as usize];
                format!("variable {var} of {}", self.fn_names[def.fn_id as usize])
            }
            ObjKind::Return(inst) | ObjKind::VarArgs(inst) => {
                let def = &self.defs[self.insts[inst as usize].def as usize];
                format!(
                    "{:?} of {}",
                    self.objects[obj as usize].kind, self.fn_names[def.fn_id as usize]
                )
            }
            ObjKind::Global(global) => format!("global {}", self.global_names[global as usize]),
            ObjKind::Heap(site) => format!("heap of {}", self.describe(site)),
            ObjKind::Literal => "string literals".to_owned(),
            ObjKind::Library(library) => format!("code of library {}", self.library_name(library)),
            ObjKind::LibOwn(library, class) => format!(
                "library {}'s own memory ({})",
                self.library_name(library),
                self.class_name(class)
            ),
            ObjKind::Outside => "outside".to_owned(),
            ObjKind::Copy(copy) => match self.copy_sites[copy as usize] {
                (Some(inst), line) => {
                    let def = &self.defs[self.insts[inst as usize].def as usize];
                    format!(
                        "copy at {}:{line} {}",
                        def.file, self.fn_names[def.fn_id as usize]
                    )
                }
                (None, _) => format!("copy {copy}"),
            },
        }
    }

    /// The functions the C library keeps for later, by when they run.
    fn kept(&self) -> BTreeMap<Later, Vec<FnId>> {
        let mut kept: BTreeMap<Later, FxHashSet<FnId>> = BTreeMap::new();
        for (i, site) in self.sites.iter().enumerate() {
            if let SiteKind::Later(when) = site.kind {
                kept.entry(when)
                    .or_default()
                    .extend(self.site_functions(i as SiteId));
            }
        }
        for &(when, node) in &self.kept_members {
            kept.entry(when).or_default().extend(self.functions(node));
        }
        kept.into_iter()
            .map(|(when, functions)| {
                let mut functions: Vec<FnId> = functions.into_iter().collect();
                functions.sort_unstable_by(|a, b| {
                    self.fn_names[*a as usize].cmp(&self.fn_names[*b as usize])
                });
                (when, functions)
            })
            .collect()
    }

    /// What the analysis found, or reached where it stopped short of the
    /// components.
    pub(crate) fn report(&mut self, store: &Store, components: Option<&Components>) -> String {
        let mut out = String::new();
        self.normalize_edges();
        self.report_sizes(&mut out);
        self.report_trace(&mut out);
        self.report_blame(&mut out);
        self.report_calls(&mut out, store);
        self.report_tags(&mut out);
        if let Some(components) = components {
            self.report_components(&mut out, components);
        }
        self.report_libraries(&mut out);
        out
    }

    fn report_sizes(&mut self, out: &mut String) {
        let reps: Vec<NodeId> = (0..self.rep.len() as NodeId)
            .filter(|&node| self.rep[node as usize] == node)
            .collect();
        let mut elements = 0u64;
        let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
        for &node in &reps {
            let set = self.pts[node as usize];
            if set == EMPTY {
                continue;
            }
            let len = self.sets.len(set) as usize;
            elements += len as u64;
            *sizes.entry(bucket(len)).or_default() += 1;
        }
        let (mut cells, mut readers, mut mirrors, mut waiting) = (0, 0, 0, 0);
        for object in &self.objects {
            if let Some(memory) = &object.memory {
                cells += memory.cell_count();
                readers += memory.reader_count();
                mirrors += memory.mirrors.len();
                if !memory.read {
                    waiting += memory.mirrors.len();
                }
            }
        }
        let cons: usize = self.cons.values().map(Vec::len).sum();
        let _ = writeln!(
            out,
            "points-to: {} nodes ({} after merging, {} generated), {} edges, {cons} constraints, {} objects, {} locations, {cells} cells, {readers} readers, {mirrors} copies of bytes ({waiting} into objects nothing reads), {} transforms",
            self.rep.len(),
            reps.len(),
            self.stats.nodes_generated,
            self.edge_count,
            self.objects.len(),
            self.locs.len(),
            self.trs.len()
        );
        let _ = writeln!(
            out,
            "sets: {} distinct, {elements} elements over the nodes, {} MiB stored, {} results remembered ({} MiB), {} moves remembered",
            self.sets.count(),
            self.sets.arena_bytes() >> 20,
            self.sets.memo_len(),
            self.sets.memo_bytes() >> 20,
            self.gep_memo.len()
        );
        let (_, peak) = memory_use();
        self.stats.peak_rss = self.stats.peak_rss.max(peak);
        let times: Vec<String> = self
            .stats
            .seconds
            .iter()
            .map(|(what, seconds)| format!("{what} {seconds:.1} s"))
            .collect();
        let _ = writeln!(
            out,
            "peak memory {} MiB; {}",
            self.stats.peak_rss >> 20,
            times.join(", ")
        );
        let _ = writeln!(
            out,
            "bodies: {} read ({} MiB), {} definitions not read whole, {} wrappers analysed again for {} calls ({} past the cap), {} accessors, {} functions returning a few constants, {} functions that may write some tag and {} any, {} functions without effects",
            self.stats.bodies,
            self.stats.body_bytes >> 20,
            self.defs.iter().filter(|def| def.unread).count(),
            self.stats.wrappers,
            self.stats.clones,
            self.stats.clones_refused,
            self.stats.accessors,
            self.stats.constant_returns,
            self.stats.key_writers,
            self.stats.any_key_writers,
            self.stats.pure
        );
        let hvn = &self.stats.hvn;
        let _ = writeln!(
            out,
            "offline: {} nodes, {} merged, {} holding nothing, {} labels, {} stores of nothing dropped",
            hvn.nodes, hvn.merged, hvn.empty, hvn.labels, hvn.stores_dropped
        );
        let solve = &self.stats.solve;
        let _ = writeln!(
            out,
            "solving: {} rounds, {} nodes processed, {} cycle checks merging {}, {} passes merging {} and widening {} cycles ({} edges), memos cleared {} times, sets no node holds dropped {} times",
            solve.rounds,
            solve.processed,
            solve.lcd_checks,
            solve.lcd_merged,
            solve.passes,
            solve.pass_merged,
            solve.widened_cycles,
            solve.widened_edges,
            solve.memo_clears,
            solve.compactions
        );
        histogram(out, "locations per node", &sizes);
        self.report_largest_set(out, &reps);
        self.report_edges(out);
        let mut largest: Vec<(usize, usize, ObjId)> = self
            .objects
            .iter()
            .enumerate()
            .filter_map(|(obj, object)| {
                let memory = object.memory.as_deref()?;
                Some((memory.cell_count(), memory.reader_count(), obj as ObjId))
            })
            .collect();
        largest.sort_unstable_by(|a, b| b.cmp(a));
        let _ = writeln!(out, "objects with the most cells:");
        for &(cells, readers, obj) in largest.iter().take(16) {
            let _ = writeln!(
                out,
                "  {cells:8} cells {readers:8} readers  {}",
                self.object_name(obj)
            );
        }
        largest.sort_unstable_by_key(|&(cells, readers, obj)| {
            std::cmp::Reverse((readers, cells, obj))
        });
        let mut copied_to: FxHashMap<ObjId, (usize, FxHashSet<ObjId>)> = FxHashMap::default();
        for (dst, object) in self.objects.iter().enumerate() {
            for mirror in object.memory.iter().flat_map(|memory| &memory.mirrors) {
                let entry = copied_to.entry(mirror.src).or_default();
                entry.0 += 1;
                entry.1.insert(dst as ObjId);
            }
        }
        let _ = writeln!(
            out,
            "objects with the most readers (loose readers; copies in: sources, shifts; copies out: destinations):"
        );
        for &(cells, readers, obj) in largest.iter().take(16) {
            let memory = self.objects[obj as usize].memory.as_deref();
            let mirrors = memory.map_or(&[][..], |memory| &memory.mirrors[..]);
            let sources: FxHashSet<ObjId> = mirrors.iter().map(|mirror| mirror.src).collect();
            let shifts: FxHashSet<Off> = mirrors
                .iter()
                .map(|mirror| off_sub(mirror.dst_off, mirror.src_off))
                .collect();
            let (out_copies, out_dsts) = copied_to
                .get(&obj)
                .map_or((0, 0), |(count, dsts)| (*count, dsts.len()));
            let _ = writeln!(
                out,
                "  {cells:8} cells {readers:8} readers ({} loose; {} in: {} sources, {} shifts; {out_copies} out: {out_dsts})  {}",
                memory.map_or(0, |memory| memory.loose_readers.len()),
                mirrors.len(),
                sources.len(),
                shifts.len(),
                self.object_name(obj)
            );
            let mut by_source: FxHashMap<ObjId, usize> = FxHashMap::default();
            for mirror in mirrors {
                *by_source.entry(mirror.src).or_default() += 1;
            }
            let mut by_source: Vec<(usize, ObjId)> =
                by_source.into_iter().map(|(src, n)| (n, src)).collect();
            by_source.sort_unstable_by_key(|&(n, src)| std::cmp::Reverse((n, src)));
            for &(n, src) in by_source.iter().take(3) {
                let _ = writeln!(out, "      {n:8} from {}", self.object_name(src));
            }
        }
        let _ = writeln!(
            out,
            "cells copies moved: {} at constant offsets, {} not; {} copies widened round cycles",
            self.stats.copy_cells.0, self.stats.copy_cells.1, self.stats.widened_copies
        );
        self.report_strides(out);
        if !self.stats.unknown.is_empty() {
            let _ = writeln!(out, "values the lowering could not give:");
            for (why, count) in &self.stats.unknown {
                let _ = writeln!(out, "  {count:8} {why}");
            }
        }
    }

    /// What the largest set holds: its objects by kind, and the objects
    /// it holds at the most offsets.
    fn report_largest_set(&self, out: &mut String, reps: &[NodeId]) {
        let Some(&node) = reps
            .iter()
            .max_by_key(|&&node| self.sets.len(self.pts[node as usize]))
        else {
            return;
        };
        let locs = self.sets.elements(self.pts[node as usize]);
        let mut offsets: FxHashMap<ObjId, usize> = FxHashMap::default();
        for &loc in &locs {
            *offsets.entry(self.locs[loc as usize].0).or_default() += 1;
        }
        let mut kinds: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
        for (&obj, &n) in &offsets {
            let kind = kind_word(&self.objects[obj as usize].kind);
            let entry = kinds.entry(kind).or_default();
            entry.0 += 1;
            entry.1 += n;
        }
        let _ = writeln!(
            out,
            "the largest set: {} locations of {} objects",
            locs.len(),
            offsets.len()
        );
        for (kind, (objects, locations)) in &kinds {
            let _ = writeln!(out, "  {objects:8} {kind} at {locations} locations");
        }
        let mut most: Vec<(usize, ObjId)> = offsets.into_iter().map(|(obj, n)| (n, obj)).collect();
        most.sort_unstable_by_key(|&(n, obj)| std::cmp::Reverse((n, obj)));
        for &(n, obj) in most.iter().take(8) {
            let _ = writeln!(out, "  {n:8} offsets of {}", self.object_name(obj));
        }
    }

    /// The edges by the kinds of the nodes they join, and the nodes with
    /// the most edges out and in.
    fn report_edges(&self, out: &mut String) {
        const SHOWN: usize = 12;
        let kinds = self.node_kinds();
        let mut ins = vec![0u32; self.rep.len()];
        let mut by_kind: BTreeMap<(NodeKind, NodeKind), u64> = BTreeMap::new();
        let mut outs: Vec<(u32, NodeId)> = Vec::new();
        for from in 0..self.rep.len() {
            if self.rep[from] as usize != from || self.succ[from].is_empty() {
                continue;
            }
            outs.push((self.succ[from].len() as u32, from as NodeId));
            for &(to, _) in &self.succ[from] {
                let to = self.find_const(to) as usize;
                ins[to] += 1;
                *by_kind.entry((kinds[from], kinds[to])).or_default() += 1;
            }
        }
        outs.sort_unstable_by_key(|&(n, node)| std::cmp::Reverse((n, node)));
        outs.truncate(SHOWN);
        let mut most_in: Vec<(u32, NodeId)> = ins
            .iter()
            .enumerate()
            .filter(|&(_, &n)| n > 0)
            .map(|(node, &n)| (n, node as NodeId))
            .collect();
        most_in.sort_unstable_by_key(|&(n, node)| std::cmp::Reverse((n, node)));
        most_in.truncate(SHOWN);
        let wanted: FxHashSet<NodeId> = outs
            .iter()
            .chain(most_in.iter())
            .map(|&(_, node)| node)
            .collect();
        let names = self.node_names(&wanted);
        let mut by_kind: Vec<((NodeKind, NodeKind), u64)> = by_kind.into_iter().collect();
        by_kind.sort_unstable_by_key(|&(kinds, n)| std::cmp::Reverse((n, kinds)));
        let _ = writeln!(out, "edges by the nodes they join:");
        for ((from, to), n) in by_kind.iter().take(24) {
            let _ = writeln!(out, "  {n:12} {from:?} -> {to:?}");
        }
        for (title, list) in [("most edges out", &outs), ("most edges in", &most_in)] {
            let _ = writeln!(out, "nodes with the {title}:");
            for &(n, node) in list.iter() {
                let shown: Vec<&str> = names
                    .get(&node)
                    .map(|names| names.iter().take(4).map(String::as_str).collect())
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "  {n:10} [{}] {:?} {}",
                    self.sets.len(self.pts[node as usize]),
                    kinds[node as usize],
                    shown.join(" | ")
                );
            }
        }
    }

    fn report_calls(&self, out: &mut String, store: &Store) {
        let mut indirect = 0;
        let mut unresolved = Vec::new();
        let mut into_libraries = 0;
        let mut data = 0;
        let mut anything = 0;
        let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
        let mut widest: Vec<(usize, SiteId)> = Vec::new();
        for (i, site) in self.sites.iter().enumerate() {
            if site.kind != SiteKind::Indirect {
                continue;
            }
            indirect += 1;
            let functions = self.site_functions(i as SiteId);
            let libraries = site
                .bound
                .iter()
                .filter(|target| matches!(target, super::Target::Library(_)))
                .count();
            if functions.is_empty() && libraries == 0 {
                unresolved.push(i as SiteId);
            }
            if libraries > 0 {
                into_libraries += 1;
            }
            data += usize::from(site.data);
            anything += usize::from(site.anything);
            *sizes.entry(bucket(functions.len())).or_default() += 1;
            widest.push((functions.len(), i as SiteId));
        }
        let _ = writeln!(
            out,
            "calls through pointers: {indirect}, {} with no target, {into_libraries} into libraries, {data} maybe through data, {anything} maybe through bytes from outside",
            unresolved.len()
        );
        histogram(out, "targets per call through a pointer", &sizes);
        let _ = writeln!(out, "with no target:");
        for &site in unresolved.iter().take(40) {
            let _ = writeln!(
                out,
                "  {}  {}",
                self.callee_text(store, site),
                self.describe(site)
            );
        }
        widest.sort_unstable_by(|a, b| b.cmp(a));
        let _ = writeln!(out, "with the most targets:");
        let mut shown: FxHashSet<String> = FxHashSet::default();
        for &(count, site) in &widest {
            if shown.len() >= 40 {
                break;
            }
            let callee = self.callee_text(store, site);
            if shown.insert(callee.clone()) {
                let _ = writeln!(out, "  {count:6} {callee}  ({})", self.describe(site));
            }
        }
    }

    fn report_tags(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "tags: {} keys, {} parts and contexts; variable arguments read as {} classes of types",
            self.tags.keys.len(),
            self.tag_sets.len() - 1,
            self.variadic.classes()
        );
        // The objects writes of each key's byte that are not paired may
        // write, and those of them where they may write every value.
        let mut unpaired_in = vec![(0usize, 0usize); self.tags.keys.len()];
        for object in &self.objects {
            for &(key, values) in object.memory.iter().flat_map(|memory| &memory.unpaired) {
                let full = self.tags.keys[key as usize].full();
                let counts = &mut unpaired_in[key as usize];
                counts.0 += 1;
                if values & full == full {
                    counts.1 += 1;
                }
            }
        }
        for (id, key) in self.tags.keys.iter().enumerate() {
            let counts = &key.counts;
            let mut state = if key.unclean_values == 0 && key.unsplit_by.is_empty() {
                "clean".to_owned()
            } else {
                format!(
                    "writes not paired in {} objects, of every value in {}",
                    unpaired_in[id].0, unpaired_in[id].1
                )
            };
            for bit in (0..64).map(|i| 1u64 << i) {
                let by: Vec<String> = key
                    .unclean_by
                    .iter()
                    .filter(|(b, ..)| *b == bit)
                    .map(|(_, at, rooted)| {
                        format!("{at}{}", if *rooted { "" } else { " (not rooted)" })
                    })
                    .collect();
                if !by.is_empty() {
                    let _ = write!(state, "\n      {} by {}", key.describe(bit), by.join(", "));
                }
            }
            if !key.unsplit_by.is_empty() {
                let _ = write!(
                    state,
                    "\n      values not known by {}",
                    key.unsplit_by.join(", ")
                );
            }
            let _ = writeln!(
                out,
                "  {} byte {} region {}..{} ({} values{}): {state}",
                key.record,
                key.tag,
                key.region.0,
                key.region.1,
                key.domain.len(),
                if key.full() & OTHER != 0 {
                    " and others"
                } else {
                    ""
                }
            );
            let _ = writeln!(
                out,
                "    {} writes of the byte ({} copied, {} constant unpaired, {} unpaired, {} not rooted), {} of the region ({} copies, {} parts found ahead, {} parts), {} contexts",
                counts.tag_writes,
                counts.correlated,
                counts.unpaired_constant,
                counts.unpaired,
                counts.unrooted,
                counts.region_writes,
                counts.copies,
                counts.searched,
                counts.parts,
                counts.contexts
            );
        }
        let (count, objects, examples) = self.tag_overlaps();
        let _ = writeln!(
            out,
            "writes not through a key's members that may write a key's byte: {count}, into {objects} objects"
        );
        for example in examples {
            let _ = writeln!(out, "  {example}");
        }
    }

    fn report_components(&self, out: &mut String, components: &Components) {
        let largest: Vec<&str> = components
            .largest
            .iter()
            .take(12)
            .map(|&def| &*self.fn_names[self.defs[def as usize].fn_id as usize])
            .collect();
        let _ = writeln!(
            out,
            "call graph: {} definitions, {} calls between them, {} components loaded callees first ({} MiB in {:.1} s), the largest {} definitions ({} KiB, at most {} MiB in use for one): {}",
            self.defs.len(),
            self.call_edges.len(),
            components.count,
            components.bytes >> 20,
            components.seconds,
            components.largest.len(),
            components.largest_bytes >> 10,
            components.most_loaded >> 20,
            largest.join(" ")
        );
        histogram(out, "definitions per component", &components.sizes);
    }

    fn report_libraries(&self, out: &mut String) {
        let _ = writeln!(out, "functions without a source and not in the tables:");
        let mut by_library: BTreeMap<&str, Vec<(&str, usize)>> = BTreeMap::new();
        for (name, (libraries, calls)) in &self.unmodelled {
            for &library in libraries {
                by_library
                    .entry(self.library_name(library))
                    .or_default()
                    .push((name, *calls));
            }
        }
        for (library, functions) in &by_library {
            let _ = writeln!(out, "  {library}: {}", functions.len());
            let list: Vec<String> = functions
                .iter()
                .map(|(name, calls)| format!("{name}×{calls}"))
                .collect();
            for chunk in list.chunks(8) {
                let _ = writeln!(out, "    {}", chunk.join(" "));
            }
        }
        let _ = writeln!(
            out,
            "functions each library may call (the classes it holds; the locations it gives at void):"
        );
        let mut libraries: Vec<super::LibId> = self.libs.keys().copied().collect();
        libraries.sort_unstable();
        for library in libraries {
            let state = &self.libs[&library];
            let calls = state
                .held
                .iter()
                .find(|((class, _), _)| {
                    self.class_info(*class).to == super::library::Pointee::Function
                })
                .map_or(super::NONE, |(_, &node)| node);
            let given = state
                .gives
                .iter()
                .find(|(class, _)| {
                    let info = self.class_info(**class);
                    info.to == super::library::Pointee::Void && info.konst
                })
                .or_else(|| {
                    state.gives.iter().find(|(class, _)| {
                        self.class_info(**class).to == super::library::Pointee::Void
                    })
                })
                .map_or(0, |(_, &node)| self.points_to(node).len());
            let mut functions: Vec<&str> = if calls == super::NONE {
                Vec::new()
            } else {
                self.functions(calls)
                    .into_iter()
                    .map(|f| &*self.fn_names[f as usize])
                    .collect()
            };
            functions.sort_unstable();
            let _ = writeln!(
                out,
                "  {}: {} ({} classes; {} locations)",
                self.library_name(library),
                functions.len(),
                state.held.len(),
                given
            );
            if !functions.is_empty() {
                let shown: Vec<&str> = functions.iter().take(24).copied().collect();
                let _ = writeln!(out, "    {}", shown.join(" "));
            }
        }
        let _ = writeln!(out, "kept for later:");
        for (when, functions) in self.kept() {
            let names: Vec<&str> = functions
                .iter()
                .map(|&f| &*self.fn_names[f as usize])
                .collect();
            let _ = writeln!(out, "  {when:?}: {}", names.join(" "));
        }
    }
}

/// The word for objects of a kind.
pub(crate) fn kind_word(kind: &ObjKind) -> &'static str {
    match kind {
        ObjKind::Function(_) => "functions",
        ObjKind::Var(..) => "variables",
        ObjKind::Return(_) => "results",
        ObjKind::VarArgs(_) => "variable arguments",
        ObjKind::Global(_) => "globals",
        ObjKind::Heap(_) => "heap",
        ObjKind::Literal => "string literals",
        ObjKind::Library(_) | ObjKind::LibOwn(..) => "libraries",
        ObjKind::Outside => "outside",
        ObjKind::Copy(_) => "copies",
    }
}
