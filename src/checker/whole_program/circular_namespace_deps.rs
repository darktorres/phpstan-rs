//! ROADMAP Phase 86's B3: flag a *namespace-level* structural
//! dependency cycle - namespace `A` has a class that `extends`/
//! `implements`/`use`s a class in namespace `B`, and namespace `B` (via
//! some other, possibly unrelated, class pair) has an edge back into
//! `A`. Real PHPStan has no such rule; this project's own closed-world
//! `Registry` makes the graph available almost for free, since every
//! `ClassInfo.extends`/`.implements`/`.uses` entry is already resolved
//! to its target's real canonical FQN (`ctx.resolve` at declaration
//! time) - no new indexing pass needed at all, unlike Phase 86's B2
//! (`dead_code`), which needed a real (if cheap) reference index.
//!
//! Deliberately narrower than the backlog's own original sketch (Tarjan's
//! SCC over `Project.dependents`, "the already-live reverse-reference
//! graph"): that graph is *any* reference (a type hint, a static call, an
//! `instanceof`, ...), not just structural composition, and - as found
//! implementing B2 - its own raw, unresolved keys aren't precise enough
//! for a specific, attributable finding like this one either. Restricting
//! to `extends`/`implements`/`uses` trades completeness (a cycle formed
//! purely through method-body calls or type hints is invisible here) for
//! a check that's both fully precise (zero false edges: every edge is a
//! real, already-canonical structural fact) and effectively free (a
//! single pass over already-built `Registry` data, no dependency on
//! `check_dead_code`'s own Tier-1 index at all). A namespace with itself
//! as its own only "cycle" (every class in a namespace referencing a
//! sibling in the same namespace) is never reported - only a same-
//! namespace edge is skipped when building the graph in the first place,
//! so a namespace never gets a self-loop from its own internal structure.

use rustc_hash::FxHashMap;

use crate::registry::Registry;

/// Everything before a fully-qualified name's last `\` - the empty
/// string for a global-namespace name (a real, if unusual, node in the
/// graph like any other).
fn namespace_of(fqn: &str) -> &str {
    fqn.rsplit_once('\\').map(|(ns, _)| ns).unwrap_or("")
}

/// One namespace-level cycle: every namespace in it, sorted for a
/// deterministic report order, plus one real, reportable class - the
/// alphabetically-first class (by FQN) whose own `extends`/`implements`/
/// `uses` crosses from the alphabetically-first namespace pair inside
/// this cycle - the caller resolves its own declaration site itself
/// (this function stays unaware of files/spans/`Diagnostic` entirely,
/// mirroring `dead_enum_cases`/`dead_code`'s own module-privacy design).
pub(crate) struct NamespaceCycle {
    pub(crate) namespaces: Vec<String>,
    pub(crate) representative_class: String,
}

pub(crate) fn find_namespace_cycles(registry: &Registry) -> Vec<NamespaceCycle> {
    let mut graph: FxHashMap<String, Vec<String>> = FxHashMap::default();
    // The alphabetically-first class contributing each distinct
    // namespace-pair edge - used to pick a cycle's own representative
    // once its member namespaces are known.
    let mut edge_representative: FxHashMap<(String, String), String> = FxHashMap::default();

    for (fqn, info) in &registry.classes {
        let from_ns = namespace_of(fqn).to_string();
        let targets = info
            .extends
            .iter()
            .chain(info.implements.iter())
            .chain(info.uses.iter());
        for target in targets {
            let to_ns = namespace_of(target).to_string();
            if from_ns == to_ns {
                continue;
            }
            let edges = graph.entry(from_ns.clone()).or_default();
            if !edges.contains(&to_ns) {
                edges.push(to_ns.clone());
            }
            edge_representative
                .entry((from_ns.clone(), to_ns))
                .and_modify(|existing| {
                    if fqn.as_ref() < existing.as_str() {
                        *existing = fqn.to_string();
                    }
                })
                .or_insert_with(|| fqn.to_string());
        }
    }

    let sccs = tarjan_sccs(&graph);
    let mut cycles = Vec::new();
    for mut namespaces in sccs {
        if namespaces.len() < 2 {
            continue;
        }
        namespaces.sort();
        let representative_class = namespaces
            .iter()
            .flat_map(|from| {
                namespaces.iter().filter_map(|to| {
                    edge_representative
                        .get(&(from.clone(), to.clone()))
                        .cloned()
                })
            })
            .min()
            .expect("a cycle of 2+ namespaces always has at least one contributing edge");
        cycles.push(NamespaceCycle {
            namespaces,
            representative_class,
        });
    }
    cycles.sort_by(|a, b| a.namespaces.cmp(&b.namespaces));
    cycles
}

/// Standard recursive Tarjan's strongly-connected-components algorithm.
/// Namespace counts are small (nowhere near deep enough to risk a stack
/// overflow the way this project's own unguarded-recursive AST walk
/// needed a large worker stack for), so recursion is fine here.
fn tarjan_sccs(graph: &FxHashMap<String, Vec<String>>) -> Vec<Vec<String>> {
    struct State<'a> {
        graph: &'a FxHashMap<String, Vec<String>>,
        counter: usize,
        stack: Vec<String>,
        on_stack: rustc_hash::FxHashSet<String>,
        index: FxHashMap<String, usize>,
        lowlink: FxHashMap<String, usize>,
        sccs: Vec<Vec<String>>,
    }

    impl State<'_> {
        fn visit(&mut self, v: &str) {
            let idx = self.counter;
            self.counter += 1;
            self.index.insert(v.to_string(), idx);
            self.lowlink.insert(v.to_string(), idx);
            self.stack.push(v.to_string());
            self.on_stack.insert(v.to_string());

            if let Some(neighbors) = self.graph.get(v) {
                for w in neighbors.clone() {
                    if !self.index.contains_key(&w) {
                        self.visit(&w);
                        let w_low = self.lowlink[&w];
                        let v_low = self.lowlink[v];
                        if w_low < v_low {
                            self.lowlink.insert(v.to_string(), w_low);
                        }
                    } else if self.on_stack.contains(&w) {
                        let w_idx = self.index[&w];
                        let v_low = self.lowlink[v];
                        if w_idx < v_low {
                            self.lowlink.insert(v.to_string(), w_idx);
                        }
                    }
                }
            }

            if self.lowlink[v] == self.index[v] {
                let mut component = Vec::new();
                loop {
                    let w = self.stack.pop().expect("v itself is always on the stack");
                    self.on_stack.remove(&w);
                    let is_v = w == v;
                    component.push(w);
                    if is_v {
                        break;
                    }
                }
                self.sccs.push(component);
            }
        }
    }

    let mut state = State {
        graph,
        counter: 0,
        stack: Vec::new(),
        on_stack: rustc_hash::FxHashSet::default(),
        index: FxHashMap::default(),
        lowlink: FxHashMap::default(),
        sccs: Vec::new(),
    };
    let mut nodes: Vec<&String> = graph.keys().collect();
    nodes.sort();
    for node in nodes {
        if !state.index.contains_key(node) {
            state.visit(node);
        }
    }
    state.sccs
}
