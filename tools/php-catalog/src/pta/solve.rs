//! Solving the constraints: each node passes on only the pointers it
//! gained since it last did, in rounds ordered by the graph's topological
//! order. A copy edge between nodes holding the same pointers is checked
//! for a cycle, which is merged; at times the whole graph's cycles are
//! found, merged where their shifts agree, and else widened to the
//! offsets their shifts reach.

use rustc_hash::{FxHashMap, FxHashSet};

use super::bitmap::{EMPTY, SetId};
use super::memory::Mirror;
use super::{COPY, Cons, LocId, NONE, NodeId, Phase, Pta, TrId, Transform, gcd};

/// How many nodes a check for a cycle visits at most.
const LCD_LIMIT: usize = 4096;
/// How many nodes are processed between checks of the memory in use.
const MEMORY_CHECK: u64 = 1 << 12;
/// How many successors a node has at most for a new edge to be looked
/// for among all of them.
const SCAN_SUCC: usize = 8;

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SolveStats {
    pub(crate) rounds: u64,
    pub(crate) processed: u64,
    pub(crate) lcd_checks: u64,
    pub(crate) lcd_merged: u64,
    pub(crate) passes: u64,
    pub(crate) pass_merged: u64,
    pub(crate) widened_cycles: u64,
    pub(crate) widened_edges: u64,
    pub(crate) memo_clears: u64,
    pub(crate) compactions: u64,
}

impl Pta<'_> {
    // The graph.

    /// An edge passing `from`'s pointers to `to`, moved; while solving,
    /// with what `from` already passed on.
    pub(crate) fn add_edge(&mut self, from: NodeId, to: NodeId, tr: TrId) {
        if from == NONE || to == NONE {
            return;
        }
        let (from, to) = (self.find(from), self.find(to));
        let tr = if from == to {
            // A node moving its own pointers holds them at every multiple
            // of the move.
            let t = self.trs[tr as usize];
            let closed = if t.any {
                Transform::ANY
            } else {
                Transform {
                    bound: t.bound,
                    ..Transform::linear(0, gcd(t.add.unsigned_abs(), u64::from(t.stride)))
                }
            };
            if closed == Transform::IDENTITY {
                return;
            }
            self.tr(closed)
        } else {
            tr
        };
        if !self.push_succ(from, (to, tr)) {
            return;
        }
        if self.phase == Phase::Solve {
            let done = self.done[from as usize];
            if done != EMPTY {
                let traced = self.traced_in(done);
                self.note_arrivals(from, to, traced);
                let moved = self.gep(done, tr);
                let watched = self.growth_watch(to);
                self.add_set(to, moved);
                self.note_growth(from, to, tr, moved, watched);
            }
        }
    }

    /// Adds an edge to a node's successors, unless it is the last one added
    /// or, while there are few, any of them: false where it is. The list is
    /// sorted and rid of repeats whenever it doubles, so that it holds no
    /// more than twice its edges; an edge added again only passes on what
    /// its target holds already.
    fn push_succ(&mut self, from: NodeId, edge: (NodeId, TrId)) -> bool {
        let at = from as usize;
        let succ = &mut self.succ[at];
        if succ.last() == Some(&edge) || (succ.len() <= SCAN_SUCC && succ.contains(&edge)) {
            return false;
        }
        succ.push(edge);
        self.edge_count += 1;
        if succ.len() > 2 * (self.succ_sorted[at] as usize).max(SCAN_SUCC) {
            let before = succ.len();
            succ.sort_unstable();
            succ.dedup();
            self.edge_count -= before - succ.len();
            self.succ_sorted[at] = succ.len() as u32;
        }
        true
    }

    pub(crate) fn add_initial(&mut self, node: NodeId, loc: LocId) {
        if node == NONE {
            return;
        }
        let node = self.find(node);
        self.note_initial(node, loc);
        let set = self.sets.insert(self.pts[node as usize], loc);
        if set != self.pts[node as usize] {
            self.pts[node as usize] = set;
            self.enqueue(node);
        }
    }

    fn add_set(&mut self, node: NodeId, set: SetId) {
        if set == EMPTY {
            return;
        }
        let node = self.find(node);
        let old = self.pts[node as usize];
        let union = self.sets.union(old, set);
        if union != old {
            self.pts[node as usize] = union;
            self.enqueue(node);
        }
    }

    /// A constraint on a node's pointers; while solving, brought into
    /// effect on what the node already passed on.
    pub(crate) fn add_cons(&mut self, node: NodeId, cons: Cons) {
        if node == NONE {
            return;
        }
        let node = self.find(node);
        if self.phase == Phase::Solve {
            let done = self.done[node as usize];
            if done != EMPTY {
                for loc in self.sets.elements(done) {
                    self.apply_cons(&cons, loc);
                }
            }
        }
        self.cons.entry(node).or_default().push(cons);
    }

    fn enqueue(&mut self, node: NodeId) {
        if self.phase != Phase::Solve {
            return;
        }
        let at = node as usize;
        if !self.queued[at] {
            self.queued[at] = true;
            self.next.push(node);
        }
    }

    /// Merges `b` into `a`: one node holding both's pointers, edges and
    /// constraints, passing on again what only one of them passed on.
    pub(crate) fn merge(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let (a, b) = (self.find(a), self.find(b));
        if a == b {
            return a;
        }
        let (au, bu) = (a as usize, b as usize);
        self.note_merge(a, b);
        self.merge_votes(a, b);
        self.rep[bu] = a;
        self.pts[au] = self.sets.union(self.pts[au], self.pts[bu]);
        self.done[au] = self.sets.intersection(self.done[au], self.done[bu]);
        self.pts[bu] = EMPTY;
        self.done[bu] = EMPTY;
        let succ = std::mem::take(&mut self.succ[bu]);
        self.succ[au].extend(succ);
        if let Some(cons) = self.cons.remove(&b) {
            self.cons.entry(a).or_default().extend(cons);
        }
        self.indirect[au] |= self.indirect[bu];
        if self.phase == Phase::Solve {
            self.rank[au] = self.rank[au].min(self.rank[bu]);
            if self.pts[au] != self.done[au] {
                self.enqueue(a);
            }
        }
        a
    }

    /// A set's locations moved.
    pub(crate) fn gep(&mut self, set: SetId, tr: TrId) -> SetId {
        if tr == COPY || set == EMPTY {
            return set;
        }
        if let Some(&moved) = self.gep_memo.get(&(tr, set)) {
            return moved;
        }
        let t = self.trs[tr as usize];
        let mut out = Vec::with_capacity(self.sets.len(set) as usize);
        for loc in self.sets.elements(set) {
            out.push(self.moved_loc(loc, t));
        }
        let moved = self.sets.intern_unsorted(&mut out);
        self.gep_memo.insert((tr, set), moved);
        moved
    }

    /// A constraint brought into effect on a location its node points to.
    pub(crate) fn apply_cons(&mut self, cons: &Cons, loc: LocId) {
        let (obj, off) = self.locs[loc as usize];
        let window = self.loc_windows[loc as usize];
        match *cons {
            Cons::Load {
                tr,
                width,
                ctx,
                dst,
            } => {
                if self.objects[obj as usize].is_constant() {
                    return;
                }
                let (off, window) = self.trs[tr as usize].apply_in(off, window);
                let reader = self.reader(obj, off, ctx, width, window);
                self.add_edge(reader, dst, COPY);
            }
            Cons::Store {
                tr,
                width,
                part,
                src,
            } => {
                if !self.objects[obj as usize].takes_writes() {
                    return;
                }
                let (off, window) = self.trs[tr as usize].apply_in(off, window);
                let cell = self.cell(obj, off, part, width, window);
                self.add_edge(src, cell, COPY);
            }
            Cons::Call { site } => self.call_target(site, loc),
            Cons::Unpaired { key, values } => self.leave_unpaired(obj, key, values),
            Cons::Gather {
                tr,
                len,
                preserve,
                dst,
            } => {
                let (src_off, src_window) = self.trs[tr as usize].apply_in(off, window);
                let dst_window = self.loc_windows[dst as usize];
                let (dst, dst_off) = self.locs[dst as usize];
                self.add_mirror(
                    dst,
                    Mirror {
                        src: obj,
                        src_off,
                        src_window,
                        len,
                        dst_off,
                        dst_window,
                        preserve,
                    },
                );
            }
            Cons::Scatter {
                tr,
                len,
                preserve,
                src,
            } => {
                let (dst_off, dst_window) = self.trs[tr as usize].apply_in(off, window);
                let src_window = self.loc_windows[src as usize];
                let (src, src_off) = self.locs[src as usize];
                self.add_mirror(
                    obj,
                    Mirror {
                        src,
                        src_off,
                        src_window,
                        len,
                        dst_off,
                        dst_window,
                        preserve,
                    },
                );
            }
        }
    }

    // Solving.

    pub(crate) fn solve(&mut self) -> Result<(), String> {
        for node in 0..self.rep.len() as NodeId {
            if self.rep[node as usize] == node && self.pts[node as usize] != EMPTY {
                self.enqueue(node);
            }
        }
        self.pass();
        let mut since_check = 0u64;
        while !self.next.is_empty() {
            if self.edge_count >= 2 * self.pass_edges.max(1 << 16)
                || self.locs.len() >= self.pass_locs + (self.pass_locs / 4).max(1 << 14)
            {
                self.pass();
            }
            let mut round = std::mem::take(&mut self.next);
            for &node in &round {
                self.queued[node as usize] = false;
            }
            for node in &mut round {
                *node = self.find(*node);
            }
            round.sort_unstable_by_key(|&node| (self.rank[node as usize], node));
            round.dedup();
            self.stats.solve.rounds += 1;
            for node in round {
                let node = self.find(node);
                self.process(node);
                self.stats.solve.processed += 1;
                since_check += 1;
                if since_check >= MEMORY_CHECK || self.over_cap.is_some() {
                    since_check = 0;
                    self.check_memory()?;
                    self.trim_memos();
                }
                while let Some((from, to)) = self.lcd.pop() {
                    self.check_cycle(from, to);
                }
            }
        }
        Ok(())
    }

    /// Passes on what a node gained: through its constraints and its
    /// edges.
    fn process(&mut self, node: NodeId) {
        let at = node as usize;
        let pts = self.pts[at];
        let done = self.done[at];
        if pts == done {
            return;
        }
        let delta = self.sets.difference(pts, done);
        self.done[at] = pts;
        let traced = self.traced_in(delta);
        if let Some(cons) = self.cons.remove(&node) {
            let locs = self.sets.elements(delta);
            for c in &cons {
                for &loc in &locs {
                    self.apply_cons(c, loc);
                }
            }
            let node = self.find(node);
            let mut back = cons;
            if let Some(added) = self.cons.remove(&node) {
                back.extend(added);
            }
            self.cons.insert(node, back);
        }
        let node = self.find(node);
        let at = node as usize;
        let succ = std::mem::take(&mut self.succ[at]);
        for &(to, tr) in &succ {
            let to = self.find(to);
            if to == node && tr == COPY {
                continue;
            }
            if tr == COPY && self.pts[to as usize] == pts && !self.lcd_checked.contains(&(node, to))
            {
                self.lcd.push((node, to));
            }
            self.note_arrivals(node, to, traced);
            let moved = self.gep(delta, tr);
            let watched = self.growth_watch(to);
            self.add_set(to, moved);
            self.note_growth(node, to, tr, moved, watched);
        }
        let added = std::mem::take(&mut self.succ[at]);
        self.succ[at] = succ;
        self.succ[at].extend(added);
    }

    /// Checks, once for each edge, whether an edge between nodes holding
    /// the same pointers closes a cycle of copies; merges it where it
    /// does.
    fn check_cycle(&mut self, from: NodeId, to: NodeId) {
        if !self.lcd_checked.insert((from, to)) {
            return;
        }
        self.stats.solve.lcd_checks += 1;
        let (from, to) = (self.find(from), self.find(to));
        if from == to {
            return;
        }
        let Some(cycle) = self.copy_cycle(to, from) else {
            return;
        };
        let mut into = from;
        for node in cycle {
            into = self.merge(into, node);
            self.stats.solve.lcd_merged += 1;
        }
    }

    /// The nodes of the cycle of copy edges through `start` and `target`,
    /// where a search of at most `LCD_LIMIT` nodes from `start` finds one.
    fn copy_cycle(&mut self, start: NodeId, target: NodeId) -> Option<Vec<NodeId>> {
        let mut index: FxHashMap<NodeId, (u32, u32)> = FxHashMap::default();
        let mut stack: Vec<NodeId> = Vec::new();
        let mut on_stack: FxHashSet<NodeId> = FxHashSet::default();
        let mut call: Vec<(NodeId, usize)> = Vec::new();
        let mut counter = 0u32;
        index.insert(start, (0, 0));
        counter += 1;
        stack.push(start);
        on_stack.insert(start);
        call.push((start, 0));
        while let Some(&(v, i)) = call.last() {
            let next = self.succ[v as usize].get(i).copied();
            match next {
                Some((w, tr)) => {
                    if let Some(top) = call.last_mut() {
                        top.1 += 1;
                    }
                    if tr != COPY {
                        continue;
                    }
                    let w = self.find(w);
                    match index.get(&w) {
                        None => {
                            if index.len() >= LCD_LIMIT {
                                return None;
                            }
                            index.insert(w, (counter, counter));
                            counter += 1;
                            stack.push(w);
                            on_stack.insert(w);
                            call.push((w, 0));
                        }
                        Some(&(w_index, _)) => {
                            if on_stack.contains(&w)
                                && let Some(entry) = index.get_mut(&v)
                            {
                                entry.1 = entry.1.min(w_index);
                            }
                        }
                    }
                }
                None => {
                    call.pop();
                    let (v_index, v_low) = index[&v];
                    if let Some(&(parent, _)) = call.last()
                        && let Some(entry) = index.get_mut(&parent)
                    {
                        entry.1 = entry.1.min(v_low);
                    }
                    if v_low == v_index {
                        let mut component = Vec::new();
                        while let Some(w) = stack.pop() {
                            on_stack.remove(&w);
                            component.push(w);
                            if w == v {
                                break;
                            }
                        }
                        if component.len() > 1 && component.contains(&target) {
                            return Some(component);
                        }
                    }
                }
            }
        }
        None
    }

    /// Forgets the results remembered once they grow past their bounds,
    /// and drops the sets no node holds once the sets stored have doubled.
    fn trim_memos(&mut self) {
        let compact = self.sets.count() >= 2 * self.live_sets.max(1 << 20);
        if compact || self.gep_memo.len() > 1 << 23 || self.sets.memo_len() > 1 << 24 {
            self.gep_memo = FxHashMap::default();
            self.sets.clear_memos();
            self.stats.solve.memo_clears += 1;
        }
        if compact {
            let (pts, done) = (&mut self.pts, &mut self.done);
            let mut traced = self.trace.as_deref_mut().map(|trace| &mut trace.locs);
            self.sets.compact(&mut |visit| {
                for set in pts.iter_mut().chain(done.iter_mut()) {
                    visit(set);
                }
                if let Some(locs) = traced.as_deref_mut() {
                    visit(locs);
                }
            });
            self.live_sets = self.sets.count();
            self.stats.solve.compactions += 1;
        }
    }

    // The whole graph's cycles.

    /// Each node's edges to the current nodes, once each.
    pub(crate) fn normalize_edges(&mut self) {
        let mut count = 0;
        for node in 0..self.rep.len() {
            if self.rep[node] != node as NodeId {
                self.succ[node] = Vec::new();
                self.succ_sorted[node] = 0;
                continue;
            }
            let mut succ = std::mem::take(&mut self.succ[node]);
            for edge in &mut succ {
                edge.0 = self.find(edge.0);
            }
            succ.retain(|&(to, tr)| !(to == node as NodeId && tr == COPY));
            succ.sort_unstable();
            succ.dedup();
            succ.shrink_to_fit();
            count += succ.len();
            self.succ_sorted[node] = succ.len() as u32;
            self.succ[node] = succ;
        }
        self.edge_count = count;
    }

    /// The strongly connected components of the graph, in the order they
    /// complete (each before the ones reaching it): the nodes of each,
    /// and where each starts.
    pub(crate) fn components_of_graph(&self) -> (Vec<NodeId>, Vec<u32>) {
        let count = self.rep.len();
        let mut index = vec![u32::MAX; count];
        let mut low = vec![0u32; count];
        let mut on_stack = vec![false; count];
        let mut stack: Vec<NodeId> = Vec::new();
        let mut call: Vec<(NodeId, u32)> = Vec::new();
        let mut members: Vec<NodeId> = Vec::new();
        let mut starts: Vec<u32> = Vec::new();
        let mut counter = 0u32;
        for root in 0..count {
            if self.rep[root] != root as NodeId || index[root] != u32::MAX {
                continue;
            }
            index[root] = counter;
            low[root] = counter;
            counter += 1;
            stack.push(root as NodeId);
            on_stack[root] = true;
            call.push((root as NodeId, 0));
            while let Some(&(v, i)) = call.last() {
                let vu = v as usize;
                if let Some(&(w, _)) = self.succ[vu].get(i as usize) {
                    if let Some(top) = call.last_mut() {
                        top.1 += 1;
                    }
                    let wu = self.find_const(w) as usize;
                    if index[wu] == u32::MAX {
                        index[wu] = counter;
                        low[wu] = counter;
                        counter += 1;
                        stack.push(wu as NodeId);
                        on_stack[wu] = true;
                        call.push((wu as NodeId, 0));
                    } else if on_stack[wu] {
                        low[vu] = low[vu].min(index[wu]);
                    }
                } else {
                    call.pop();
                    if let Some(&(parent, _)) = call.last() {
                        let pu = parent as usize;
                        low[pu] = low[pu].min(low[vu]);
                    }
                    if low[vu] == index[vu] {
                        starts.push(members.len() as u32);
                        while let Some(w) = stack.pop() {
                            on_stack[w as usize] = false;
                            members.push(w);
                            if w == v {
                                break;
                            }
                        }
                    }
                }
            }
        }
        starts.push(members.len() as u32);
        (members, starts)
    }

    /// Finds the graph's cycles: merges the nodes of a cycle that its
    /// shifts move by the same distance from each other, widens the edges
    /// of a cycle whose shifts grow its offsets to the offsets they reach,
    /// and ranks the nodes in topological order. An edge to any offset
    /// ends a pointer's growth, so the cycles are those of the other edges.
    pub(crate) fn pass(&mut self) {
        self.stats.solve.passes += 1;
        self.normalize_edges();
        let (members, starts) = self.components_of_graph();
        let components = starts.len() - 1;
        let mut local: FxHashMap<NodeId, u32> = FxHashMap::default();
        let mut widened = false;
        let mut merged = false;
        for c in 0..components {
            let nodes = &members[starts[c] as usize..starts[c + 1] as usize];
            if nodes.len() < 2 {
                continue;
            }
            local.clear();
            for (i, &node) in nodes.iter().enumerate() {
                local.insert(node, i as u32);
            }
            let adjacency: Vec<Vec<u32>> = nodes
                .iter()
                .map(|&u| {
                    self.succ[u as usize]
                        .iter()
                        .filter(|&&(_, tr)| !self.trs[tr as usize].any)
                        .filter_map(|(v, _)| local.get(v).copied())
                        .collect()
                })
                .collect();
            let (cycles, cycle_starts) = strong_components(&adjacency);
            for k in 0..cycle_starts.len() - 1 {
                let cycle: Vec<NodeId> = cycles
                    [cycle_starts[k] as usize..cycle_starts[k + 1] as usize]
                    .iter()
                    .map(|&i| nodes[i as usize])
                    .collect();
                if cycle.len() < 2 {
                    continue;
                }
                match self.cycle_period(&cycle) {
                    (potential, 0) => {
                        let mut by_potential: FxHashMap<i64, NodeId> = FxHashMap::default();
                        for &node in &cycle {
                            let p = potential[&node];
                            match by_potential.get(&p) {
                                Some(&into) => {
                                    let into = self.merge(into, node);
                                    by_potential.insert(p, into);
                                    self.stats.solve.pass_merged += 1;
                                    merged = true;
                                }
                                None => {
                                    by_potential.insert(p, node);
                                }
                            }
                        }
                    }
                    (_, period) => {
                        self.widen(&cycle, period);
                        widened = true;
                    }
                }
            }
        }
        if merged || widened {
            self.normalize_edges();
        }
        let (members, starts) = if merged {
            self.components_of_graph()
        } else {
            (members, starts)
        };
        let components = starts.len() - 1;
        for c in 0..components {
            let rank = (components - 1 - c) as u32;
            for &node in &members[starts[c] as usize..starts[c + 1] as usize] {
                self.rank[node as usize] = rank;
            }
        }
        self.pass_edges = self.edge_count;
        self.pass_locs = self.locs.len();
    }

    /// Each node's distance from the first along a cycle's edges, none of
    /// them to any offset, and the gcd of the distances its cycles add and
    /// of its strides: 0 where every way round adds nothing.
    fn cycle_period(&self, cycle: &[NodeId]) -> (FxHashMap<NodeId, i64>, u64) {
        let inside: FxHashSet<NodeId> = cycle.iter().copied().collect();
        let mut potential: FxHashMap<NodeId, i64> = FxHashMap::default();
        potential.insert(cycle[0], 0);
        let mut work = vec![cycle[0]];
        let mut period = 0u64;
        while let Some(u) = work.pop() {
            let pu = potential[&u];
            for &(v, tr) in &self.succ[u as usize] {
                let t = self.trs[tr as usize];
                if t.any || !inside.contains(&v) {
                    continue;
                }
                period = gcd(period, u64::from(t.stride));
                let reached = pu.saturating_add(t.add);
                match potential.get(&v) {
                    None => {
                        potential.insert(v, reached);
                        work.push(v);
                    }
                    Some(&pv) => period = gcd(period, reached.abs_diff(pv)),
                }
            }
        }
        (potential, period)
    }

    /// Widens the edges inside a cycle that grows its offsets to the
    /// offsets a multiple of its period apart.
    fn widen(&mut self, cycle: &[NodeId], period: u64) {
        self.stats.solve.widened_cycles += 1;
        let inside: FxHashSet<NodeId> = cycle.iter().copied().collect();
        for &u in cycle {
            let succ = std::mem::take(&mut self.succ[u as usize]);
            let mut rewritten = Vec::with_capacity(succ.len());
            for (v, tr) in succ {
                let t = self.trs[tr as usize];
                if t.any || !inside.contains(&v) {
                    rewritten.push((v, tr));
                    continue;
                }
                let wide = Transform {
                    bound: t.bound,
                    ..Transform::linear(t.add, gcd(u64::from(t.stride), period))
                };
                if wide == t {
                    rewritten.push((v, tr));
                    continue;
                }
                let wide = self.tr(wide);
                rewritten.push((v, wide));
                self.stats.solve.widened_edges += 1;
                let done = self.done[u as usize];
                if done != EMPTY {
                    let moved = self.gep(done, wide);
                    self.add_set(v, moved);
                }
            }
            self.succ[u as usize] = rewritten;
        }
    }
}

/// The strongly connected components of a graph given by each vertex's
/// successors, in the order they complete (each before the ones reaching
/// it): the vertices of each, and where each starts.
pub(crate) fn strong_components(adjacency: &[Vec<u32>]) -> (Vec<u32>, Vec<u32>) {
    let count = adjacency.len();
    let mut index = vec![u32::MAX; count];
    let mut low = vec![0u32; count];
    let mut on_stack = vec![false; count];
    let mut stack: Vec<u32> = Vec::new();
    let mut call: Vec<(u32, usize)> = Vec::new();
    let mut members: Vec<u32> = Vec::with_capacity(count);
    let mut starts: Vec<u32> = Vec::new();
    let mut counter = 0u32;
    for root in 0..count {
        if index[root] != u32::MAX {
            continue;
        }
        index[root] = counter;
        low[root] = counter;
        counter += 1;
        stack.push(root as u32);
        on_stack[root] = true;
        call.push((root as u32, 0));
        while let Some(&(v, i)) = call.last() {
            let vu = v as usize;
            if let Some(&w) = adjacency[vu].get(i) {
                if let Some(top) = call.last_mut() {
                    top.1 += 1;
                }
                let wu = w as usize;
                if index[wu] == u32::MAX {
                    index[wu] = counter;
                    low[wu] = counter;
                    counter += 1;
                    stack.push(w);
                    on_stack[wu] = true;
                    call.push((w, 0));
                } else if on_stack[wu] {
                    low[vu] = low[vu].min(index[wu]);
                }
                continue;
            }
            call.pop();
            if let Some(&(parent, _)) = call.last() {
                let pu = parent as usize;
                low[pu] = low[pu].min(low[vu]);
            }
            if low[vu] == index[vu] {
                starts.push(members.len() as u32);
                while let Some(w) = stack.pop() {
                    on_stack[w as usize] = false;
                    members.push(w);
                    if w == v {
                        break;
                    }
                }
            }
        }
    }
    starts.push(members.len() as u32);
    (members, starts)
}
