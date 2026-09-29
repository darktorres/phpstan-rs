//! Stable synthetic naming for `new class { ... }` (anonymous class)
//! expressions, so one can be indexed into the registry under a real name
//! the same way any other class is - mirrors real PHPStan's own
//! `AnonymousClassNameHelper`/`AnonymousClassVisitor` exactly
//! (`vendor/phpstan-src/src/Broker/AnonymousClassNameHelper.php`,
//! `vendor/phpstan-src/src/Parser/AnonymousClassVisitor.php`):
//! `"AnonymousClass" . md5("{file}:{line}[:{line_index}]")`, where
//! `line_index` is a 1-based, document-order position among every
//! anonymous class starting on that exact line - present only when more
//! than one shares a line, absent (not `0`) otherwise, since it changes
//! the hashed string.
//!
//! `collect_anonymous_classes` is the one place that computes these names
//! at all - both the registry entry (`BuiltDecl`) and the checker's own
//! inference site (`checker::expr::ExprKind::NewAnonymousClass`) get the
//! identical name because the checker never recomputes it: it just looks
//! its own expression's span up in the `Span -> name` map this also
//! returns, populated once, here, at parse time. This sidesteps the two
//! independent-computation-sites-must-agree risk a design that
//! recomputed the name at both places would carry.
//!
//! Unlike `registry::build_declarations` (top-level declarations only),
//! this walks every function/method/closure/arrow-fn body too - an
//! anonymous class can appear anywhere an expression can.

use crate::registry::class_info::MemberSpans;
use crate::registry::provenance::Origin;
use rustc_hash::FxHashMap;

use crate::registry::{BuiltDecl, ClassReflection};
use syntax::ast::*;
use syntax::name_resolution::{NameContext, for_each_in_context};
use syntax::token::Span;
use syntax::visit::{Visitor, walk_expr};

/// A class declaration's name, or `class@anonymous` for an anonymous
/// class.
pub(crate) fn class_decl_name(c: &ClassDecl) -> &str {
    c.name.as_deref().unwrap_or("class@anonymous")
}

/// The fully-qualified name a class declaration declares;
/// `class@anonymous` for an anonymous class.
pub(crate) fn class_decl_fqn(c: &ClassDecl) -> &str {
    c.fq_name().unwrap_or("class@anonymous")
}

pub(crate) fn synthetic_name(
    relative_file: &str,
    start_line: u32,
    line_index: Option<u32>,
) -> String {
    let hashed = match line_index {
        Some(i) => format!("{relative_file}:{start_line}:{i}"),
        None => format!("{relative_file}:{start_line}"),
    };
    format!("AnonymousClass{:x}", md5::compute(hashed.as_bytes()))
}

/// See the module doc comment. `relative_file` is whatever string this
/// project already uses to identify the file elsewhere (the same `Path`
/// `Project::check_file`'s own caller prints diagnostics against) - only
/// needs to be stable per file across one analysis run, not to match real
/// PHPStan's own path convention exactly, since nothing compares the two.
/// Each class is recorded as declared at its `new class` expression in
/// `origin`.
pub(crate) fn collect_anonymous_classes(
    stmts: &[Stmt],
    relative_file: &str,
    origin: &Origin,
) -> (Vec<BuiltDecl>, FxHashMap<Span, String>) {
    // Each class with the context of the statement it is in.
    let mut contexts: Vec<NameContext> = Vec::new();
    let mut found: Vec<(&Expr, &ClassDecl, usize)> = Vec::new();
    let mut in_stmt = Found(Vec::new());
    for_each_in_context(stmts, |stmt, ctx| {
        in_stmt.visit_stmt(stmt);
        if !in_stmt.0.is_empty() {
            contexts.push(ctx.clone());
            let at = contexts.len() - 1;
            found.extend(in_stmt.0.drain(..).map(|(expr, decl)| (expr, decl, at)));
        }
    });

    let mut per_line: FxHashMap<u32, u32> = FxHashMap::default();
    for (expr, _, _) in &found {
        *per_line.entry(expr.span.start.line).or_insert(0) += 1;
    }

    let mut seen_on_line: FxHashMap<u32, u32> = FxHashMap::default();
    let mut decls = Vec::new();
    let mut names = FxHashMap::default();
    for (expr, decl, at) in found {
        let line = expr.span.start.line;
        let line_index = if per_line[&line] > 1 {
            let counter = seen_on_line.entry(line).or_insert(0);
            *counter += 1;
            Some(*counter)
        } else {
            None
        };
        let name = synthetic_name(relative_file, line, line_index);
        names.insert(expr.span, name.clone());
        let name: std::sync::Arc<str> = std::sync::Arc::from(name);
        let mut info = ClassReflection::from_decl(decl, &contexts[at]);
        info.is_anonymous = true;
        info.record_provenance(origin, &name, expr.span, &MemberSpans::of_class(decl));
        // Not `intern()`: every anonymous class in the whole corpus gets
        // its own unique md5-hashed name (file+line+index), so there is
        // no repeated text here to deduplicate - only a permanent,
        // never-evicted interner-table entry to avoid.
        decls.push(BuiltDecl::Class {
            name,
            conditional: false,
            info,
        });
    }
    (decls, names)
}

/// Every anonymous class, in source order, wherever it is.
struct Found<'a>(Vec<(&'a Expr, &'a ClassDecl)>);

impl<'a> Visitor<'a> for Found<'a> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let ExprKind::NewAnonymousClass(decl, _) = &expr.kind {
            self.0.push((expr, decl));
        }
        walk_expr(self, expr);
    }
}
