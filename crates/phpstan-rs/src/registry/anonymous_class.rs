//! Stable synthetic naming for `new class { ... }` (anonymous class)
//! expressions, so one can be indexed into the registry under a real name
//! the same way any other class is: `class@anonymous/{file}:{line}`, with
//! `:{line_index}` appended when more than one anonymous class starts on
//! that line (`line_index` is 1-based, in document order). No declared
//! class can have that name, and it is how messages and types show the
//! class.
//!
//! `name_anonymous_classes` computes these names once, right after
//! parsing, and stores each on its `AnonymousClass` node, so the registry
//! entry and everything that reads the AST agree by construction.
//!
//! Unlike `registry::build_declarations` (top-level declarations only),
//! this walks every function/method/closure/arrow-fn body too - an
//! anonymous class can appear anywhere an expression can.

use crate::registry::class_info::MemberSpans;
use crate::registry::provenance::Origin;
use rustc_hash::FxHashMap;
use syntax::name::Name;
use syntax::span::LineIndex;

use crate::registry::{BuiltDecl, ClassReflection};
use syntax::ast::*;
use syntax::name_resolution::{NameContext, for_each_in_context};
use syntax::visit::{Visitor, VisitorMut, walk_expr, walk_expr_mut};

pub(crate) fn synthetic_name(
    relative_file: &str,
    start_line: u32,
    line_index: Option<u32>,
) -> String {
    match line_index {
        Some(i) => format!("class@anonymous/{relative_file}:{start_line}:{i}"),
        None => format!("class@anonymous/{relative_file}:{start_line}"),
    }
}

/// Names every anonymous class in `stmts` (see the module doc comment).
/// `relative_file` is the file as reports name it, relative to the root
/// with `/` between its names on every platform (`diagnostics::report_path`),
/// as PHPStan's own `class@anonymous` names spell it, so a message that
/// names the class reads the same on every platform.
pub(crate) fn name_anonymous_classes(
    stmts: &mut [Stmt],
    relative_file: &str,
    line_index: &LineIndex,
) {
    let mut lines = Lines(Vec::new(), line_index);
    for stmt in stmts.iter() {
        lines.visit_stmt(stmt);
    }
    if lines.0.is_empty() {
        return;
    }
    let mut per_line: FxHashMap<u32, u32> = FxHashMap::default();
    for line in &lines.0 {
        *per_line.entry(*line).or_insert(0) += 1;
    }
    let mut namer = Namer {
        relative_file,
        line_index,
        per_line,
        seen_on_line: FxHashMap::default(),
    };
    for stmt in stmts.iter_mut() {
        namer.visit_stmt(stmt);
    }
}

/// The registry entry of every named anonymous class in `stmts`, each
/// recorded as declared at its `new class` expression in `origin`.
pub(crate) fn anonymous_class_decls(stmts: &[Stmt], origin: &Origin) -> Vec<BuiltDecl> {
    let mut decls = Vec::new();
    let mut in_stmt = Found(Vec::new());
    for_each_in_context(stmts, |stmt, ctx: &NameContext| {
        in_stmt.visit_stmt(stmt);
        for (expr, class) in in_stmt.0.drain(..) {
            let Some(name) = &class.name else {
                continue;
            };
            let mut info = ClassReflection::from_def(name, &class.def, ctx);
            info.is_anonymous = true;
            info.record_provenance(origin, name, expr.span, &MemberSpans::of_class(&class.def));
            decls.push(BuiltDecl::Class {
                name: name.clone(),
                conditional: false,
                guard: None,
                info: std::sync::Arc::new(info),
            });
        }
    });
    decls
}

/// The start line of every anonymous class, in source order.
struct Lines<'i>(Vec<u32>, &'i LineIndex);

impl<'a> Visitor<'a> for Lines<'_> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let ExprKind::NewAnonymousClass(_) = &expr.kind {
            self.0.push(self.1.line(expr.span.start));
        }
        walk_expr(self, expr);
    }
}

/// Names each anonymous class in the order `Lines` found them.
struct Namer<'a> {
    relative_file: &'a str,
    line_index: &'a LineIndex,
    per_line: FxHashMap<u32, u32>,
    seen_on_line: FxHashMap<u32, u32>,
}

impl VisitorMut for Namer<'_> {
    fn visit_expr(&mut self, expr: &mut Expr) {
        if let ExprKind::NewAnonymousClass(class) = &mut expr.kind {
            let line = self.line_index.line(expr.span.start);
            let line_index = if self.per_line.get(&line).is_some_and(|n| *n > 1) {
                let counter = self.seen_on_line.entry(line).or_insert(0);
                *counter += 1;
                Some(*counter)
            } else {
                None
            };
            class.name = Some(Name::from(synthetic_name(
                self.relative_file,
                line,
                line_index,
            )));
        }
        walk_expr_mut(self, expr);
    }
}

/// Every anonymous class, in source order, wherever it is.
struct Found<'a>(Vec<(&'a Expr, &'a AnonymousClass)>);

impl<'a> Visitor<'a> for Found<'a> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let ExprKind::NewAnonymousClass(class) = &expr.kind {
            self.0.push((expr, class));
        }
        walk_expr(self, expr);
    }
}
