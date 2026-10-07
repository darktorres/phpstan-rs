//! Stable synthetic naming for `new class { ... }` (anonymous class)
//! expressions, so one can be indexed into the registry under a real name
//! the same way any other class is: `class@anonymous/{file}:{line}`, with
//! `:{line_index}` appended when more than one anonymous class starts on
//! that line (`line_index` is 1-based, in document order). No declared
//! class can have that name, and it is how messages and types show the
//! class.
//!
//! `name_anonymous_classes` computes these names once, right after
//! parsing, and stores each in the file owner's side table keyed by its
//! `AnonymousClassId`, so the parsed syntax tree stays immutable.
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
use syntax::name_resolution::NameContext;
use syntax::visit::arena::{self, Visitor as ArenaVisitor};

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
/// `relative_file` is whatever string this project already uses to
/// identify the file elsewhere (the same `Path` `Project::check_file`'s
/// own caller prints diagnostics against) - only needs to be stable per
/// file across one analysis run, not to match real PHPStan's own path
/// convention exactly, since nothing compares the two.
pub(crate) fn name_anonymous_classes(
    arenas: &mut NodeArenas,
    stmts: &syntax::ast::StmtIdRange,
    relative_file: &str,
    line_index: &LineIndex,
) {
    let mut lines = Lines(Vec::new(), line_index);
    let stmts = arenas.stmt_ids(stmts).to_vec();
    arena::walk_stmts(&mut lines, arenas, &stmts);
    if lines.0.is_empty() {
        return;
    }
    let mut per_line: FxHashMap<u32, u32> = FxHashMap::default();
    for (_, line) in &lines.0 {
        *per_line.entry(*line).or_insert(0) += 1;
    }
    let mut seen_on_line: FxHashMap<u32, u32> = FxHashMap::default();
    for (class_id, line) in lines.0 {
        let line_index = if per_line.get(&line).is_some_and(|n| *n > 1) {
            let counter = seen_on_line.entry(line).or_insert(0);
            *counter += 1;
            Some(*counter)
        } else {
            None
        };
        arenas.set_anonymous_class_name(
            class_id,
            Name::from(synthetic_name(relative_file, line, line_index)),
        );
    }
}

/// The registry entry of every named anonymous class in `stmts`, each
/// recorded as declared at its `new class` expression in `origin`.
pub(crate) fn anonymous_class_decls(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    origin: &Origin,
) -> Vec<BuiltDecl> {
    let mut decls = Vec::new();
    let mut ctx = NameContext::default();
    for &id in stmts {
        let Some(stmt) = arenas.stmt(id) else {
            continue;
        };
        match &stmt.kind {
            StmtKind::Namespace(name) => ctx.set_namespace(name),
            StmtKind::Use(imports) => ctx.add_uses(arenas.use_imports(imports)),
            _ => {}
        }
        let mut found = Found(Vec::new());
        found.visit_stmt(arenas, id);
        for (expr_id, class_id) in found.0 {
            let Some(class) = arenas.anonymous_class(class_id) else {
                continue;
            };
            let Some(name) = arenas.anonymous_class_name(class_id) else {
                continue;
            };
            let Some(def) = arenas.class(class.def) else {
                continue;
            };
            let mut info = ClassReflection::from_def(arenas, name, def, &ctx);
            info.is_anonymous = true;
            let Some(span) = arenas.expr_span(expr_id) else {
                continue;
            };
            info.record_provenance(origin, name, span, &MemberSpans::of_class(arenas, def));
            decls.push(BuiltDecl::Class {
                name: name.clone(),
                conditional: false,
                guard: None,
                info: std::sync::Arc::new(info),
                namespace: Name::from(ctx.namespace()),
            });
        }
    }
    decls
}

struct Found(Vec<(ExprId, AnonymousClassId)>);

impl ArenaVisitor for Found {
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        if let Some(expr) = arenas.expr(id)
            && let ExprKind::NewAnonymousClass(class_id) = &expr.kind
        {
            self.0.push((id, *class_id));
        }
        arena::walk_expr(self, arenas, id);
    }
}

/// The start line of every anonymous class, in source order.
struct Lines<'i>(Vec<(AnonymousClassId, u32)>, &'i LineIndex);

impl ArenaVisitor for Lines<'_> {
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        if let Some(expr) = arenas.expr(id)
            && let ExprKind::NewAnonymousClass(class_id) = &expr.kind
        {
            self.0.push((
                *class_id,
                self.1.line(
                    arenas
                        .expr_span(expr.id)
                        .expect("anonymous-class expression has a parallel span")
                        .start,
                ),
            ));
        }
        arena::walk_expr(self, arenas, id);
    }
}
