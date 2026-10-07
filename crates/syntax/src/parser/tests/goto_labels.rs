//! The labels the parser records for each function-like body and for the
//! file are the ones a source-order walk of that body finds.
use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::visit::arena::{self, Visitor};

/// What a walk of `stmts` in source order says: every label a `goto` names,
/// and whether some `goto` after the label names it.
fn expected(arenas: &NodeArenas, stmts: &[StmtId]) -> Vec<(String, bool)> {
    struct Gotos {
        seen: FxHashSet<String>,
        targets: Vec<(String, bool)>,
    }
    impl Visitor for Gotos {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            let Some(stmt) = arenas.stmt(id) else {
                return;
            };
            match &stmt.kind {
                StmtKind::Label(name) => {
                    self.seen.insert(name.name.to_string());
                }
                StmtKind::Goto(name) => {
                    let name = name.name.to_string();
                    let backward = self.seen.contains(&name);
                    match self.targets.iter_mut().find(|target| target.0 == name) {
                        Some(target) => target.1 |= backward,
                        None => self.targets.push((name, backward)),
                    }
                }
                _ => {}
            }
            arena::walk_stmt(self, arenas, id);
        }

        // Expression bodies, nested functions, and classes have their own
        // goto scopes. The parser records those separately.
        fn visit_expr(&mut self, _arenas: &NodeArenas, _id: ExprId) {}

        fn visit_function_decl(&mut self, _arenas: &NodeArenas, _function: &FunctionDecl) {}

        fn visit_class_def(&mut self, _arenas: &NodeArenas, _class: &ClassDef) {}
    }

    let mut gotos = Gotos {
        seen: FxHashSet::default(),
        targets: Vec::new(),
    };
    arena::walk_stmts(&mut gotos, arenas, stmts);
    gotos.targets.sort();
    gotos.targets
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
    fn compare(&mut self, arenas: &NodeArenas, what: &str, stmts: &[StmtId], labels: &[GotoLabel]) {
        self.bodies += 1;
        let (want, got) = (expected(arenas, stmts), recorded(labels));
        if !want.is_empty() {
            self.with_labels += 1;
        }
        if want != got {
            self.mismatches
                .push(format!("{what}: walk {want:?}, parser {got:?}"));
        }
    }
}

impl Visitor for Check {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        arena::walk_stmt(self, arenas, id);
    }

    fn visit_function_decl(&mut self, arenas: &NodeArenas, function: &FunctionDecl) {
        if let Some(body) = &function.body {
            self.compare(
                arenas,
                &function.name.name,
                arenas.stmt_ids(&body.stmts),
                &body.goto_labels,
            );
        }
        arena::walk_function_decl(self, arenas, function);
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(expr) = arenas.expr(id) else {
            return;
        };
        if let ExprKind::Closure(closure) = &expr.kind {
            if let Some(crate::ast::ClosurePayload::Closure(closure)) = arenas.closure(*closure) {
                self.compare(
                    arenas,
                    "closure",
                    arenas.stmt_ids(&closure.body),
                    &closure.goto_labels,
                );
            }
        }
        arena::walk_expr(self, arenas, id);
    }

    fn visit_property_hooks(&mut self, arenas: &NodeArenas, hooks: &PropertyHooks) {
        for hook in hooks.iter() {
            if let PropertyHookBody::Block(stmts) = &hook.body {
                self.compare(arenas, "hook", arenas.stmt_ids(stmts), &hook.goto_labels);
            }
        }
        arena::walk_property_hooks(self, arenas, hooks);
    }
}

fn check_source(source: &[u8], check: &mut Check, what: &Path) {
    let Ok(parsed) = crate::parse::parse(source) else {
        return;
    };
    check.compare(
        &parsed.arenas,
        &format!("{} (file)", what.display()),
        parsed.statements(),
        &parsed.goto_labels,
    );
    arena::walk_stmts(&mut *check, &parsed.arenas, parsed.statements());
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
        check_source(source.as_bytes(), &mut check, Path::new(&format!("src{i}")));
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
    let mut check = Check::default();
    for path in &files {
        if let Ok(source) = std::fs::read(path) {
            check_source(&source, &mut check, path);
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
