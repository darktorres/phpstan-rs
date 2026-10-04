//! Flags a *namespace-level* structural
//! dependency cycle - namespace `A` has a class that `extends`/
//! `implements`/`use`s a class in namespace `B`, and namespace `B` (via
//! some other, possibly unrelated, class pair) has an edge back into
//! `A`. Real PHPStan has no such rule; each project file's declarations,
//! as built, make the graph available almost for free, since every
//! `ClassReflection.extends`/`.implements`/`.uses` entry is already resolved
//! to its target's FQN (by name resolution) - no new indexing pass needed at all, unlike
//! `dead_code`, which needed a real (if cheap) reference index.
//!
//! Deliberately narrower than Tarjan's SCC over `Project.dependents`
//! (the reverse-reference graph): that graph is *any* reference (a type
//! hint, a static call, an `instanceof`, ...), not just structural
//! composition, and its raw, unresolved keys aren't precise enough
//! for a specific, attributable finding like this one either. Restricting
//! to `extends`/`implements`/`uses` trades completeness (a cycle formed
//! purely through method-body calls or type hints is invisible here) for
//! a check that's both fully precise (zero false edges: every edge is a
//! real, already-resolved structural fact) and effectively free (a
//! single pass over the declarations as built, no dependency on
//! `check_dead_code`'s own Tier-1 index at all). A namespace with itself
//! as its own only "cycle" (every class in a namespace referencing a
//! sibling in the same namespace) is never reported - only a same-
//! namespace edge is skipped when building the graph in the first place,
//! so a namespace never gets a self-loop from its own internal structure.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::project::whole_program::Declaration;
use crate::registry::ClassReflection;

/// Everything before a fully-qualified name's last `\` - the empty
/// string for a global-namespace name (a real, if unusual, node in the
/// graph like any other).
fn namespace_of(fqn: &str) -> &str {
    fqn.rsplit_once('\\').map(|(ns, _)| ns).unwrap_or("")
}

/// A class-like declaration of a project file, as the graph reads it: its
/// own `extends`/`implements`/`uses`, from the namespace it is declared
/// in (an anonymous class's name has none).
pub(crate) struct ClassDeclaration<'a> {
    pub(crate) declaration: Declaration<'a>,
    pub(crate) namespace: &'a str,
    pub(crate) info: &'a ClassReflection,
    pub(crate) analysed: bool,
}

/// One namespace-level cycle: every namespace in it, sorted for a
/// deterministic report order, and the analysed declaration it is
/// reported at - the first, by name and then place, whose own `extends`/
/// `implements`/`uses` is an edge of the cycle.
pub(crate) struct NamespaceCycle<'a> {
    pub(crate) namespaces: Vec<String>,
    pub(crate) representative: Declaration<'a>,
}

/// The order a cycle's representative is chosen in.
fn representative_key<'a>(declaration: &Declaration<'a>) -> (&'a str, &'a std::path::Path, u32) {
    (declaration.name, declaration.path, declaration.span.start)
}

/// The cycles among the namespaces of `declarations`, every class-like
/// declaration of a project file, analysed or context: a cycle can pass
/// through vendor code. Namespaces are compared without case, as PHP
/// compares them, and each is spelled as its least declaration spells it.
/// An edge to a class no project file declares and `is_builtin_class`
/// knows goes: a builtin depends on no project code, so an edge to one
/// only joins the global namespace. A cycle no analysed declaration
/// contributes an edge to is vendor code's, and is not returned.
pub(crate) fn find_namespace_cycles<'a>(
    declarations: &[ClassDeclaration<'a>],
    is_builtin_class: impl Fn(&str) -> bool,
) -> Vec<NamespaceCycle<'a>> {
    let declared: FxHashSet<String> = declarations
        .iter()
        .map(|d| d.declaration.name.to_ascii_lowercase())
        .collect();
    let mut spelling: FxHashMap<String, &'a str> = FxHashMap::default();
    let mut graph: FxHashMap<String, Vec<String>> = FxHashMap::default();
    // The first analysed declaration contributing each distinct
    // namespace-pair edge.
    let mut edge_representative: FxHashMap<(String, String), Declaration<'a>> =
        FxHashMap::default();

    for d in declarations {
        let from_ns = d.namespace.to_ascii_lowercase();
        spelling
            .entry(from_ns.clone())
            .and_modify(|least| {
                if d.namespace < *least {
                    *least = d.namespace;
                }
            })
            .or_insert(d.namespace);
        let targets = d
            .info
            .extends()
            .iter()
            .chain(d.info.implements().iter())
            .chain(d.info.uses().iter());
        for target in targets {
            if !declared.contains(&target.to_ascii_lowercase()) && is_builtin_class(target) {
                continue;
            }
            let to_ns = namespace_of(target).to_ascii_lowercase();
            if from_ns == to_ns {
                continue;
            }
            let edges = graph.entry(from_ns.clone()).or_default();
            if !edges.contains(&to_ns) {
                edges.push(to_ns.clone());
            }
            if d.analysed {
                edge_representative
                    .entry((from_ns.clone(), to_ns))
                    .and_modify(|existing| {
                        if representative_key(&d.declaration) < representative_key(existing) {
                            *existing = d.declaration;
                        }
                    })
                    .or_insert(d.declaration);
            }
        }
    }

    let sccs = tarjan_sccs(&graph);
    let mut cycles = Vec::new();
    for members in sccs {
        if members.len() < 2 {
            continue;
        }
        let Some(representative) = members
            .iter()
            .flat_map(|from| {
                members
                    .iter()
                    .filter_map(|to| edge_representative.get(&(from.clone(), to.clone())))
            })
            .min_by_key(|declaration| representative_key(declaration))
            .copied()
        else {
            continue;
        };
        // Every namespace on a cycle has an edge out, so a declaration in
        // it.
        let mut namespaces: Vec<String> = members
            .iter()
            .map(|key| spelling.get(key).map_or(key.as_str(), |s| s).to_string())
            .collect();
        namespaces.sort();
        cycles.push(NamespaceCycle {
            namespaces,
            representative,
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
