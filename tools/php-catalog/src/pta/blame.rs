//! Where the large sets come from. Each node's main source is the node
//! whose edge passed it most of its locations, found by a majority vote
//! weighted by what each edge passed; followed back, the sources lead to
//! the node that gathered the set from many, and the large sets are
//! grouped by it.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::PathBuf;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{NONE, NodeId, Pta, TrId};

/// How many locations a node holds to be among the large.
const LARGE: u32 = 2048;
/// How many of the gathering nodes the report lists.
const ROOTS_SHOWN: usize = 40;
/// How many of the gathering nodes the file lists the sources of.
const ROOTS_WRITTEN: usize = 400;
/// How many of a gathering node's largest sources are listed.
const SOURCES_SHOWN: usize = 8;
/// How many names of a node are printed or written.
const NAMES_SHOWN: usize = 3;

pub(crate) struct Blame {
    /// Where the sources of the large sets are written.
    path: PathBuf,
    /// Each node's candidate source and the count of its vote.
    votes: Vec<(NodeId, u32)>,
}

impl Blame {
    pub(crate) fn new(path: PathBuf) -> Blame {
        Blame {
            path,
            votes: Vec::new(),
        }
    }
}

/// A gathering node's sources: each source's node, its size and the move
/// of its edge.
type Sources = Vec<(u32, NodeId, TrId)>;

impl Pta<'_> {
    /// Notes the locations an edge from one node passed another.
    pub(crate) fn vote(&mut self, from: NodeId, to: NodeId, gained: u32) {
        let count = self.rep.len();
        let Some(blame) = self.blame.as_deref_mut() else {
            return;
        };
        if gained == 0 || from == to {
            return;
        }
        if blame.votes.len() < count {
            blame.votes.resize(count, (NONE, 0));
        }
        let (candidate, votes) = &mut blame.votes[to as usize];
        if *candidate == from {
            *votes = votes.saturating_add(gained);
        } else if *votes >= gained {
            *votes -= gained;
        } else {
            *candidate = from;
            *votes = gained - *votes;
        }
    }

    /// Keeps, for a node merged into another, the votes of both.
    pub(crate) fn merge_votes(&mut self, into: NodeId, merged: NodeId) {
        let Some(blame) = self.blame.as_deref_mut() else {
            return;
        };
        let get = |node: NodeId| blame.votes.get(node as usize).copied().unwrap_or((NONE, 0));
        let ((a, a_votes), (b, b_votes)) = (get(into), get(merged));
        let merged_vote = if a == b {
            (a, a_votes.saturating_add(b_votes))
        } else if a_votes >= b_votes {
            (a, a_votes - b_votes)
        } else {
            (b, b_votes - a_votes)
        };
        if let Some(vote) = blame.votes.get_mut(into as usize) {
            *vote = merged_vote;
        }
    }

    fn set_size(&self, node: NodeId) -> u32 {
        self.sets.len(self.pts[self.find_const(node) as usize])
    }

    /// The node a node's set was gathered at: its source while the source
    /// passed it most of its locations, followed back.
    fn gathered_at(
        &self,
        votes: &[(NodeId, u32)],
        node: NodeId,
        roots: &mut FxHashMap<NodeId, NodeId>,
    ) -> NodeId {
        let mut path = Vec::new();
        let mut on_path = FxHashSet::default();
        let mut at = node;
        let root = loop {
            if let Some(&root) = roots.get(&at) {
                break root;
            }
            path.push(at);
            on_path.insert(at);
            let (candidate, count) = votes.get(at as usize).copied().unwrap_or((NONE, 0));
            let next = if candidate == NONE {
                NONE
            } else {
                self.find_const(candidate)
            };
            // The source passed at least five eighths of what the node
            // gained where its vote holds a quarter of the node's set.
            if next == NONE
                || on_path.contains(&next)
                || u64::from(count) * 4 < u64::from(self.set_size(at))
            {
                break at;
            }
            at = next;
        };
        for at in path {
            roots.insert(at, root);
        }
        root
    }

    /// The gathering nodes of the large sets, with how many each gathers,
    /// and their largest sources, in the report and, whole, in the file.
    pub(crate) fn report_blame(&mut self, out: &mut String) {
        let Some(blame) = self.blame.take() else {
            return;
        };
        let large: Vec<NodeId> = (0..self.rep.len() as NodeId)
            .filter(|&node| self.find_const(node) == node && self.set_size(node) >= LARGE)
            .collect();
        let mut roots: FxHashMap<NodeId, NodeId> = FxHashMap::default();
        let mut gathers: FxHashMap<NodeId, u32> = FxHashMap::default();
        for &node in &large {
            let root = self.gathered_at(&blame.votes, node, &mut roots);
            *gathers.entry(root).or_default() += 1;
        }
        let mut ranked: Vec<(u32, NodeId)> = gathers.iter().map(|(&root, &n)| (n, root)).collect();
        ranked.sort_unstable_by_key(|&(n, root)| std::cmp::Reverse((n, root)));
        let listed: FxHashSet<NodeId> = ranked
            .iter()
            .take(ROOTS_WRITTEN)
            .map(|&(_, root)| root)
            .collect();
        let mut sources: FxHashMap<NodeId, Sources> = FxHashMap::default();
        for from in 0..self.rep.len() as NodeId {
            let source = self.find_const(from);
            for &(to, tr) in &self.succ[from as usize] {
                let to = self.find_const(to);
                if to == source || !listed.contains(&to) {
                    continue;
                }
                let found = sources.entry(to).or_default();
                found.push((self.set_size(source), source, tr));
                if found.len() > 8 * SOURCES_SHOWN {
                    keep_largest(found);
                }
            }
        }
        for found in sources.values_mut() {
            keep_largest(found);
        }
        let mut shown: FxHashSet<NodeId> = FxHashSet::default();
        for &(_, root) in ranked.iter().take(ROOTS_SHOWN) {
            shown.insert(root);
            shown.extend(
                sources
                    .get(&root)
                    .into_iter()
                    .flatten()
                    .map(|&(_, node, _)| node),
            );
        }
        let names = self.node_names(&shown);
        let name = |node: NodeId| {
            let names = names.get(&node).map(Vec::as_slice).unwrap_or_default();
            let mut text = names
                .iter()
                .take(NAMES_SHOWN)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ");
            if names.len() > NAMES_SHOWN {
                let _ = write!(text, " (and {} more)", names.len() - NAMES_SHOWN);
            }
            text
        };
        let _ = writeln!(
            out,
            "where the {} sets of {LARGE} locations or more come from, gathered at {} nodes:",
            large.len(),
            ranked.len()
        );
        for &(n, root) in ranked.iter().take(ROOTS_SHOWN) {
            let (candidate, count) = blame.votes.get(root as usize).copied().unwrap_or((NONE, 0));
            let _ = writeln!(
                out,
                "  {n} sets from {} locations at node {root}: {}",
                self.set_size(root),
                name(root)
            );
            let _ = writeln!(
                out,
                "      its vote: node {} by {count}",
                if candidate == NONE {
                    "none".to_owned()
                } else {
                    self.find_const(candidate).to_string()
                }
            );
            for &(size, source, tr) in sources.get(&root).into_iter().flatten() {
                let _ = writeln!(
                    out,
                    "      {size} from node {source} moved {:?}: {}",
                    self.trs[tr as usize],
                    name(source)
                );
            }
        }
        if let Err(e) = self.write_blame(&blame, &roots, &ranked, &sources) {
            let _ = writeln!(out, "the sources were not written: {e}");
        }
    }

    /// Writes each node a large set's path passes with its source and the
    /// node it was gathered at, the gathering nodes with their sources,
    /// and the names of all of them, a line each.
    fn write_blame(
        &self,
        blame: &Blame,
        roots: &FxHashMap<NodeId, NodeId>,
        ranked: &[(u32, NodeId)],
        sources: &FxHashMap<NodeId, Sources>,
    ) -> std::io::Result<()> {
        let mut file = BufWriter::new(File::create(&blame.path)?);
        writeln!(
            file,
            "# node <node> <size> <source> <votes> <gathered at>; gathers <node> <sets>; \
             source <gathering node> <node> <size> <move>; name <node> <text>"
        )?;
        let mut nodes: Vec<(NodeId, NodeId)> = roots.iter().map(|(&n, &r)| (n, r)).collect();
        nodes.sort_unstable();
        for &(node, root) in &nodes {
            let (candidate, count) = blame.votes.get(node as usize).copied().unwrap_or((NONE, 0));
            let candidate = if candidate == NONE {
                NONE
            } else {
                self.find_const(candidate)
            };
            writeln!(
                file,
                "node\t{node}\t{}\t{candidate}\t{count}\t{root}",
                self.set_size(node)
            )?;
        }
        for &(n, root) in ranked {
            writeln!(file, "gathers\t{root}\t{n}")?;
        }
        let mut named: FxHashSet<NodeId> = roots.keys().copied().collect();
        for &(_, root) in ranked.iter().take(ROOTS_WRITTEN) {
            for &(size, source, tr) in sources.get(&root).into_iter().flatten() {
                writeln!(
                    file,
                    "source\t{root}\t{source}\t{size}\t{:?}",
                    self.trs[tr as usize]
                )?;
                named.insert(source);
            }
        }
        let mut written: FxHashMap<NodeId, usize> = FxHashMap::default();
        let mut result = Ok(());
        self.visit_names(true, &mut |node, text| {
            if result.is_err() || !named.contains(&node) {
                return;
            }
            let count = written.entry(node).or_default();
            if *count < NAMES_SHOWN {
                *count += 1;
                result = writeln!(file, "name\t{node}\t{}", text());
            }
        });
        result?;
        file.flush()
    }
}

/// Keeps a gathering node's largest sources, each once.
fn keep_largest(found: &mut Sources) {
    found.sort_unstable_by_key(|&(size, node, tr)| std::cmp::Reverse((size, node, tr)));
    found.dedup_by_key(|&mut (_, node, _)| node);
    found.truncate(SOURCES_SHOWN);
}
