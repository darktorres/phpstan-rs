//! The recursion gate: every library function that can call itself starts
//! with a `stack::guard` call, unless `gates/recursion-allow.txt` names it
//! with a reason.
//!
//! A definition is recursive when a chain of calls leads from it back to
//! its own name. Calls are matched by name (the method of a method call, the
//! last segment of a path call), so the graph over-approximates: a name two
//! types share links both. An over-approximation can only ask for a guard
//! that is not strictly needed, which costs a comparison; a recursion the
//! graph misses is the failure this gate exists to stop.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::modgraph::walk_dir;

const ALLOWLIST: &str = "gates/recursion-allow.txt";

/// A body's opening and closing brace, as (line, column) of each character.
type Braces = ((usize, usize), (usize, usize));

/// One function definition with the names it calls.
struct Def {
    shown: String,
    path: PathBuf,
    line: usize,
    name: String,
    calls: BTreeSet<String>,
    guarded: bool,
    /// The body's `{` and `}`, as (line, column) of the characters.
    braces: Braces,
}

/// Checks the guard; with `wrap`, first rewrites each function that lacks one
/// as `{ stack::guard(|| { body }) }`, which `cargo fmt` then lays out.
pub(crate) fn recursion(root: &Path, wrap: bool) -> Result<(), String> {
    let allowed = read_allowlist(&root.join(ALLOWLIST))?;
    let mut defs = Vec::new();
    for path in library_sources(root)? {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let ast = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let shown = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let mut collector = Collector {
            shown,
            path,
            defs: &mut defs,
        };
        collector.visit_file(&ast);
    }
    let edges = call_edges(&defs);
    let recursive = recursive_defs(&edges);
    let mut violations = Vec::new();
    let mut used = BTreeSet::new();
    let mut unguarded = Vec::new();
    for (index, def) in defs.iter().enumerate() {
        if !recursive.contains(&index) || def.guarded {
            continue;
        }
        if allowed.contains(&def.name) {
            used.insert(def.name.clone());
            continue;
        }
        unguarded.push(def);
        violations.push(format!(
            "{}:{}: `{}` can call itself and does not start with `stack::guard(`",
            def.shown, def.line, def.name
        ));
    }
    if wrap {
        return wrap_bodies(&unguarded);
    }
    for name in allowed.difference(&used) {
        violations.push(format!(
            "{ALLOWLIST}: `{name}` is allowed but no recursive function has that name"
        ));
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "recursion rules broken:\n{}",
            violations.join("\n")
        ))
    }
}

/// Rewrites each unguarded body as `{ stack::guard(|| { body }) }`. The
/// two braces are inserted in place, from the last position in each file to
/// the first, so the earlier positions stay valid.
fn wrap_bodies(unguarded: &[&Def]) -> Result<(), String> {
    let mut by_file: BTreeMap<&Path, Vec<Braces>> = BTreeMap::new();
    for def in unguarded {
        by_file
            .entry(def.path.as_path())
            .or_default()
            .push(def.braces);
    }
    for (path, braces) in by_file {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut lines: Vec<Vec<char>> = text
            .split('\n')
            .map(|line| line.chars().collect())
            .collect();
        let count = braces.len();
        // Each edit is (line, index of the character it goes before, text).
        let mut edits = Vec::new();
        for (open, close) in braces {
            if lines[open.0 - 1].get(open.1) != Some(&'{')
                || lines[close.0 - 1].get(close.1) != Some(&'}')
            {
                return Err(format!(
                    "{}:{}: the body's braces are not where the span says",
                    path.display(),
                    open.0
                ));
            }
            edits.push((close.0, close.1, "}) "));
            edits.push((open.0, open.1 + 1, " stack::guard(|| {"));
        }
        // Stable, so for an empty body the closing edit (pushed first) lands
        // before the opening one at the same index.
        edits.sort_by_key(|&(line, column, _)| std::cmp::Reverse((line, column)));
        for (line, column, insert) in edits {
            let at = &mut lines[line - 1];
            for (offset, character) in insert.chars().enumerate() {
                at.insert(column + offset, character);
            }
        }
        let text = lines
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrapped {count} bodies in {}", path.display());
    }
    Ok(())
}

/// The library source files: `crates/*/src`, minus test modules.
fn library_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    walk_dir(&root.join("crates"), &mut files)?;
    files.retain(|path| {
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        let parts: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let excluded = parts
            .iter()
            .any(|part| part == "tests" || part == "benches" || part == "tests.rs");
        parts.get(1).is_some_and(|_| parts[0] == "crates")
            && parts.iter().any(|part| part == "src")
            && !excluded
    });
    files.sort();
    Ok(files)
}

/// The names in the allowlist, one per line: `name reason`. Blank lines and
/// lines starting with `#` are ignored.
fn read_allowlist(path: &Path) -> Result<BTreeSet<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, reason)) = line.split_once(char::is_whitespace) else {
            return Err(format!("{}: `{line}` has no reason", path.display()));
        };
        if reason.trim().is_empty() {
            return Err(format!("{}: `{name}` has no reason", path.display()));
        }
        names.insert(name.to_owned());
    }
    Ok(names)
}

/// Each definition's index mapped to the indices of the definitions it can
/// reach by one call: the definitions of the same file that share a called
/// name. A Visitor's `visit_*` and the `walk_*` it calls share a file, and so
/// does a function and its recursive helpers; cross-file recursion is left to
/// the depth tests, which run every traversal on a small stack.
fn call_edges(defs: &[Def]) -> Vec<Vec<usize>> {
    let mut by_name: BTreeMap<(&str, &str), Vec<usize>> = BTreeMap::new();
    for (index, def) in defs.iter().enumerate() {
        by_name
            .entry((def.shown.as_str(), def.name.as_str()))
            .or_default()
            .push(index);
    }
    defs.iter()
        .map(|def| {
            def.calls
                .iter()
                .filter_map(|callee| by_name.get(&(def.shown.as_str(), callee.as_str())))
                .flatten()
                .copied()
                .collect()
        })
        .collect()
}

/// The definitions on a cycle of calls, or calling themselves: members of a
/// strongly connected component with more than one definition, and any
/// definition with an edge to itself. Tarjan's algorithm, with the
/// recursion over the call graph itself kept off the stack by an explicit
/// frame list.
fn recursive_defs(edges: &[Vec<usize>]) -> BTreeSet<usize> {
    let count = edges.len();
    let mut index = vec![usize::MAX; count];
    let mut low = vec![0usize; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut next = 0usize;
    let mut recursive = BTreeSet::new();
    for root in 0..count {
        if index[root] != usize::MAX {
            continue;
        }
        // Each frame is a node and the position of its next edge.
        let mut frames = vec![(root, 0usize)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(node, position)) = frames.last() {
            if let Some(&target) = edges[node].get(position) {
                frames.last_mut().expect("a frame is on the stack").1 += 1;
                if index[target] == usize::MAX {
                    index[target] = next;
                    low[target] = next;
                    next += 1;
                    stack.push(target);
                    on_stack[target] = true;
                    frames.push((target, 0));
                } else if on_stack[target] {
                    low[node] = low[node].min(index[target]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == index[node] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                let self_call = component.len() == 1 && edges[node].contains(&node);
                if component.len() > 1 || self_call {
                    recursive.extend(component);
                }
            }
        }
    }
    recursive
}

/// Collects every function definition of a file that is not test code.
struct Collector<'a> {
    shown: String,
    path: PathBuf,
    defs: &'a mut Vec<Def>,
}

impl Collector<'_> {
    fn push(&mut self, name: &syn::Ident, block: &syn::Block, span: proc_macro2::Span) {
        let mut calls = CallCollector::default();
        calls.visit_block(block);
        let whole = block.span();
        let (start, end) = (whole.start(), whole.end());
        self.defs.push(Def {
            shown: self.shown.clone(),
            path: self.path.clone(),
            line: span.start().line,
            name: name.to_string(),
            calls: calls.names,
            guarded: starts_with_guard(block),
            braces: ((start.line, start.column), (end.line, end.column - 1)),
        });
    }
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !is_test_only(&module.attrs) {
            syn::visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        if !is_test_only(&function.attrs) {
            self.push(&function.sig.ident, &function.block, function.span());
            syn::visit::visit_item_fn(self, function);
        }
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        if !is_test_only(&function.attrs) {
            self.push(&function.sig.ident, &function.block, function.span());
            syn::visit::visit_impl_item_fn(self, function);
        }
    }

    fn visit_trait_item_fn(&mut self, function: &'ast syn::TraitItemFn) {
        if let Some(block) = &function.default
            && !is_test_only(&function.attrs)
        {
            self.push(&function.sig.ident, block, function.span());
        }
        syn::visit::visit_trait_item_fn(self, function);
    }
}

/// The names a body calls that can reach a definition of the same file: a
/// function called by a single-segment or `Self`/`self`/`crate`/`super` path,
/// a method called on `self`, and the `visit_*` and `walk_*` methods of the
/// Visitor pattern. Other receivers and `Type::name` paths name another type's
/// method, which the syntax cannot resolve, so they are left out. A nested
/// function is its own definition, so its calls are not the outer body's.
#[derive(Default)]
struct CallCollector {
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for CallCollector {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let segments: Vec<String> = path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            let reachable = match segments.as_slice() {
                [_] => true,
                [first, ..] => matches!(first.as_str(), "Self" | "self" | "crate" | "super"),
                [] => false,
            };
            if let (true, Some(last)) = (reachable, segments.last()) {
                self.names.insert(last.clone());
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        let on_self = matches!(
            call.receiver.as_ref(),
            syn::Expr::Path(path) if path.path.is_ident("self")
        );
        if on_self || name.starts_with("visit_") || name.starts_with("walk_") {
            self.names.insert(name);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_item_fn(&mut self, _: &'ast syn::ItemFn) {}

    fn visit_impl_item_fn(&mut self, _: &'ast syn::ImplItemFn) {}
}

/// Whether the body opens with a `stack::guard` call, as its first statement
/// or as the `return` of its first statement.
fn starts_with_guard(block: &syn::Block) -> bool {
    let Some(syn::Stmt::Expr(expr, _)) = block.stmts.first() else {
        return false;
    };
    let expr = match expr {
        syn::Expr::Return(ret) => match ret.expr.as_deref() {
            Some(inner) => inner,
            None => return false,
        },
        other => other,
    };
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    let syn::Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    let segments: Vec<String> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    segments.len() >= 2 && segments[segments.len() - 2..] == ["stack", "guard"]
}

/// Whether the attributes make the item test code: `#[cfg(test)]` or `#[test]`.
fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        if path.is_ident("test") {
            return true;
        }
        if !path.is_ident("cfg") {
            return false;
        }
        let mut test = false;
        let _ = attr.parse_nested_meta(|meta| {
            test |= meta.path.is_ident("test");
            Ok(())
        });
        test
    })
}
