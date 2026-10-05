//! The nodes that hold the same pointers, found before solving and merged
//! (offline variable substitution by hash-based value numbering): each
//! node is labelled by the locations it starts with, its predecessors'
//! labels and a label of its own where it gains pointers only while
//! solving, the labels of a moved node's pointers by what they are moved
//! from and how. Nodes of one label hold the same pointers; a node with
//! none holds none.

use rustc_hash::{FxHashMap, FxHashSet};

use super::bitmap::{EMPTY, SetId, Sets};
use super::{COPY, Cons, NodeId, Pta, TrId};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct HvnStats {
    pub(crate) nodes: usize,
    pub(crate) merged: usize,
    pub(crate) empty: usize,
    pub(crate) labels: usize,
    pub(crate) stores_dropped: usize,
}

impl Pta<'_> {
    pub(crate) fn hvn(&mut self) {
        self.normalize_edges();
        let (members, starts) = self.components_of_graph();
        let count = self.rep.len();
        let mut labels = Sets::new();
        let mut fresh = self.locs.len() as u32;
        let mut new_label = || {
            let label = fresh;
            fresh += 1;
            label
        };
        let mut incoming: Vec<SetId> = vec![EMPTY; count];
        let mut label: Vec<SetId> = vec![EMPTY; count];
        let mut moved: FxHashMap<(SetId, TrId), SetId> = FxHashMap::default();
        let mut component_of: FxHashSet<NodeId> = FxHashSet::default();
        // The components complete each before the ones reaching it: read
        // backwards, each comes after all that reach it.
        for c in (0..starts.len() - 1).rev() {
            let nodes = &members[starts[c] as usize..starts[c + 1] as usize];
            component_of.clear();
            for &node in nodes {
                component_of.insert(node);
            }
            let shifts_inside = nodes.len() > 1
                && nodes.iter().any(|&u| {
                    self.succ[u as usize]
                        .iter()
                        .any(|&(v, tr)| tr != COPY && component_of.contains(&v))
                });
            let mut union = EMPTY;
            let mut open = false;
            for &node in nodes {
                let start = self.sets.elements(self.pts[node as usize]);
                let start = labels.intern_sorted(&start);
                union = labels.union(union, start);
                union = labels.union(union, incoming[node as usize]);
                open |= self.indirect[node as usize];
            }
            if shifts_inside {
                for &node in nodes {
                    label[node as usize] = if union == EMPTY && !open {
                        EMPTY
                    } else {
                        let own = new_label();
                        labels.singleton(own)
                    };
                }
            } else {
                if open {
                    let own = new_label();
                    let own = labels.singleton(own);
                    union = labels.union(union, own);
                }
                for &node in nodes {
                    label[node as usize] = union;
                }
            }
            for &u in nodes {
                let from = label[u as usize];
                if from == EMPTY {
                    continue;
                }
                for &(v, tr) in &self.succ[u as usize] {
                    if component_of.contains(&v) {
                        continue;
                    }
                    let given = if tr == COPY {
                        from
                    } else {
                        match moved.get(&(from, tr)) {
                            Some(&given) => given,
                            None => {
                                let own = new_label();
                                let given = labels.singleton(own);
                                moved.insert((from, tr), given);
                                given
                            }
                        }
                    };
                    let v = v as usize;
                    incoming[v] = labels.union(incoming[v], given);
                }
            }
        }
        drop(incoming);
        let mut stats = HvnStats {
            nodes: members.len(),
            labels: labels.count(),
            ..HvnStats::default()
        };
        let mut by_label: FxHashMap<SetId, NodeId> = FxHashMap::default();
        let mut empty = vec![false; count];
        for &node in &members {
            let l = label[node as usize];
            if l == EMPTY {
                empty[node as usize] = true;
                stats.empty += 1;
                continue;
            }
            match by_label.get(&l) {
                Some(&into) => {
                    let into = self.merge(into, node);
                    by_label.insert(l, into);
                    stats.merged += 1;
                }
                None => {
                    by_label.insert(l, node);
                }
            }
        }
        for node in 0..count {
            if empty[node] && self.rep[node] == node as NodeId {
                self.succ[node] = Vec::new();
                self.cons.remove(&(node as NodeId));
            }
        }
        for cons in self.cons.values_mut() {
            let before = cons.len();
            cons.retain(|c| match *c {
                Cons::Store { src, .. } => !empty[src as usize],
                _ => true,
            });
            stats.stores_dropped += before - cons.len();
        }
        self.cons.retain(|_, cons| !cons.is_empty());
        self.normalize_edges();
        self.stats.hvn = stats;
    }
}
