//! The labels the parser records for each function-like body and for the
//! file are the ones a source-order walk of that body finds.
use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ast::*;
use crate::name::Interner;
use crate::visit::{Visitor, any_stmt_in_body, walk_expr, walk_function_decl, walk_stmt};

/// What a walk of `stmts` in source order says: every label a `goto` names,
/// and whether some `goto` after the label names it.
fn expected(stmts: &[Stmt]) -> Vec<(String, bool)> {
    let mut seen: FxHashSet<String> = FxHashSet::default();
    let mut targets: Vec<(String, bool)> = Vec::new();
    for stmt in stmts {
        any_stmt_in_body(stmt, |s| {
            match &s.kind {
                StmtKind::Label(name) => {
                    seen.insert(name.name.to_string());
                }
                StmtKind::Goto(name) => {
                    let name = name.name.to_string();
                    let backward = seen.contains(&name);
                    match targets.iter_mut().find(|t| t.0 == name) {
                        Some(t) => t.1 |= backward,
                        None => targets.push((name, backward)),
                    }
                }
                _ => {}
            }
            false
        });
    }
    targets.sort();
    targets
}

fn recorded(labels: &[GotoLabel]) -> Vec<(String, bool)> {
    let mut v: Vec<_> = labels
        .iter()
        .map(|l| (l.name.to_string(), l.backward))
        .collect();
    v.sort();
    v
}

#[derive(Default)]
struct Check {
    bodies: usize,
    with_labels: usize,
    mismatches: Vec<String>,
}

impl Check {
    fn compare(&mut self, what: &str, stmts: &[Stmt], labels: &[GotoLabel]) {
        self.bodies += 1;
        let (want, got) = (expected(stmts), recorded(labels));
        if !want.is_empty() {
            self.with_labels += 1;
        }
        if want != got {
            self.mismatches
                .push(format!("{what}: walk {want:?}, parser {got:?}"));
        }
    }
}

impl<'ast> Visitor<'ast> for Check {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        walk_stmt(self, stmt);
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        if let Some(body) = &function.body {
            self.compare(&function.name.name, &body.stmts, &body.goto_labels);
        }
        walk_function_decl(self, function);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let ExprKind::Closure(closure) = &expr.kind {
            self.compare("closure", &closure.body, &closure.goto_labels);
        }
        walk_expr(self, expr);
    }

    fn visit_property_hooks(&mut self, hooks: &'ast PropertyHooks) {
        for hook in hooks.iter() {
            if let PropertyHookBody::Block(stmts) = &hook.body {
                self.compare("hook", stmts, &hook.goto_labels);
            }
        }
        crate::visit::walk_property_hooks(self, hooks);
    }
}

fn check_source(source: &[u8], interner: &Arc<Interner>, check: &mut Check, what: &Path) {
    let Ok(parsed) = crate::parse::parse(source, interner) else {
        return;
    };
    check.compare(
        &format!("{} (file)", what.display()),
        &parsed.stmts,
        &parsed.goto_labels,
    );
    for stmt in &parsed.stmts {
        check.visit_stmt(stmt);
    }
}

fn php_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            php_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "php") {
            out.push(path);
        }
    }
}

#[test]
fn the_labels_of_a_body_are_those_of_a_source_order_walk() {
    let interner = Arc::clone(Interner::global());
    let mut check = Check::default();
    let sources = [
        "<?php goto a; b: echo 1; a: goto b; function f() { x: goto x; goto y; y: } \
         $c = function () { goto z; z: goto z; }; \
         class C { public int $p { get { s: goto s; goto t; t: return 1; } } \
         function m() { if ($a) { goto l; } l: goto l; } }",
        "<?php function g() { if (1) { l1: } goto l1; goto l2; while (0) { l2: } }",
        "<?php function h() { $f = function () { goto in; }; goto in; in: }",
    ];
    for (i, source) in sources.iter().enumerate() {
        check_source(
            source.as_bytes(),
            &interner,
            &mut check,
            Path::new(&format!("src{i}")),
        );
    }
    assert!(check.mismatches.is_empty(), "{:#?}", check.mismatches);
    assert!(
        check.with_labels >= 8,
        "only {} bodies with a goto",
        check.with_labels
    );
}

#[test]
fn the_labels_agree_over_the_fixtures_and_the_vendor_tree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    php_files(&root.join("crates/phpstan-rs/tests/fixtures"), &mut files);
    php_files(&root.join("vendor/phpstan-src/src"), &mut files);
    php_files(&root.join("vendor/symfony/src"), &mut files);
    let interner = Arc::clone(Interner::global());
    let mut check = Check::default();
    for path in &files {
        if let Ok(source) = std::fs::read(path) {
            check_source(&source, &interner, &mut check, path);
        }
    }
    assert!(check.mismatches.is_empty(), "{:#?}", check.mismatches);
    assert!(
        check.bodies > 1000,
        "only {} bodies from {} files",
        check.bodies,
        files.len()
    );
}
