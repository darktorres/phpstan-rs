//! Bare constant references. At level 1, `ConstantRule`
//! (`constant.notFound`) flags a global constant no `const`/`define()`
//! anywhere in the project and no stub declares; one the same file tests
//! with `defined('NAME')` is left alone, since that guard is how code
//! reads a constant supplied from outside the analysed files.
//! `MagicConstantContextRule` flags a magic constant used where it's
//! always empty.

use std::collections::HashSet;

use crate::ast::*;
use crate::ast_walk::{any_expr_child, stmt_children, stmt_own_exprs};
use crate::checker::Checker;
use crate::token::Span;

const MAGIC_CONSTANTS: [&str; 10] = [
    "__LINE__",
    "__FILE__",
    "__DIR__",
    "__FUNCTION__",
    "__CLASS__",
    "__TRAIT__",
    "__METHOD__",
    "__NAMESPACE__",
    "__PROPERTY__",
    "__COMPILER_HALT_OFFSET__",
];

/// Per-file facts the bare-constant checks need, gathered once.
#[derive(Default)]
pub(crate) struct ConstantFacts {
    /// Every constant name tested with a literal `defined('NAME')`.
    pub(crate) defined_guarded: HashSet<String>,
    /// Spans of `__FUNCTION__`/`__METHOD__` inside a closure or arrow
    /// function, where they name `{closure}`.
    pub(crate) magic_in_closure: HashSet<Span>,
}

pub(crate) fn constant_facts(program: &[Stmt]) -> ConstantFacts {
    let mut out = ConstantFacts::default();
    for stmt in program {
        collect_in_stmt(stmt, false, &mut out);
    }
    out
}

fn collect_in_stmt(stmt: &Stmt, in_closure: bool, out: &mut ConstantFacts) {
    match &stmt.kind {
        StmtKind::FunctionDecl(f) => collect_in_body(&f.body, false, out),
        StmtKind::ClassDecl(c) => {
            for m in &c.methods {
                collect_in_body(&m.function.body, false, out);
            }
        }
        StmtKind::EnumDecl(e) => {
            for m in &e.methods {
                collect_in_body(&m.function.body, false, out);
            }
        }
        _ => {
            for e in stmt_own_exprs(stmt) {
                collect_in_expr(e, in_closure, out);
            }
            for child in stmt_children(stmt) {
                collect_in_stmt(child, in_closure, out);
            }
        }
    }
}

fn collect_in_body(body: &[Stmt], in_closure: bool, out: &mut ConstantFacts) {
    for stmt in body {
        collect_in_stmt(stmt, in_closure, out);
    }
}

fn collect_in_expr(e: &Expr, in_closure: bool, out: &mut ConstantFacts) {
    match &e.kind {
        ExprKind::Name(name)
            if in_closure
                && ["__FUNCTION__", "__METHOD__"]
                    .iter()
                    .any(|m| name.eq_ignore_ascii_case(m)) =>
        {
            out.magic_in_closure.insert(e.span);
        }
        ExprKind::Call(callee, args) => {
            if let ExprKind::Name(name) = &callee.kind
                && name
                    .trim_start_matches('\\')
                    .eq_ignore_ascii_case("defined")
                && let Some(arg) = args.first()
                && let ExprKind::Literal(Literal::String(constant)) = &arg.value.kind
            {
                out.defined_guarded
                    .insert(constant.trim_start_matches('\\').to_string());
            }
            collect_in_expr(callee, in_closure, out);
            for a in args {
                collect_in_expr(&a.value, in_closure, out);
            }
        }
        ExprKind::Closure { body, .. } => collect_in_body(body, true, out),
        ExprKind::ArrowFn { body, .. } => collect_in_expr(body, true, out),
        ExprKind::NewAnonymousClass(decl, args) => {
            for a in args {
                collect_in_expr(&a.value, in_closure, out);
            }
            for m in &decl.methods {
                collect_in_body(&m.function.body, false, out);
            }
        }
        _ => {
            any_expr_child(e, &mut |child| {
                collect_in_expr(child, in_closure, out);
                false
            });
        }
    }
}

impl<'a> Checker<'a> {
    /// Called for a bare constant name that resolved to no known
    /// constant.
    pub(crate) fn check_unknown_global_constant(&mut self, name: &str, span: Span, label: &str) {
        let bare = name.trim_start_matches('\\');
        let unqualified = bare.rsplit('\\').next().unwrap_or(bare);
        if ["true", "false", "null", "exit", "die"]
            .iter()
            .any(|kw| unqualified.eq_ignore_ascii_case(kw))
            || MAGIC_CONSTANTS
                .iter()
                .any(|m| unqualified.eq_ignore_ascii_case(m))
        {
            return;
        }
        let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
        let guarded = |n: &str| self.constant_facts.defined_guarded.contains(n);
        if guarded(bare)
            || guarded(primary.trim_start_matches('\\'))
            || fallback.as_deref().is_some_and(guarded)
        {
            return;
        }
        self.report(
            label.to_string(),
            format!("Constant {bare} not found."),
            span,
            1,
            "constant.notFound",
        );
    }

    /// `MagicConstantContextRule`: `__CLASS__` outside a class,
    /// `__TRAIT__` outside a trait, `__FUNCTION__`/`__METHOD__` outside
    /// any function-like, `__NAMESPACE__` in the global namespace.
    pub(crate) fn check_magic_constant_context(&mut self, name: &str, span: Span, label: &str) {
        let upper = name.trim_start_matches('\\').to_ascii_uppercase();
        let (message, identifier) = match upper.as_str() {
            "__CLASS__" if self.current_class.is_none() => (
                "is always empty outside a class",
                "magicConstant.outOfClass",
            ),
            "__TRAIT__" if self.current_trait.is_none() => (
                "is always empty outside a trait",
                "magicConstant.outOfTrait",
            ),
            "__FUNCTION__" | "__METHOD__"
                if self.current_method.is_none()
                    && !self.constant_facts.magic_in_closure.contains(&span) =>
            {
                (
                    "is always empty outside a function",
                    "magicConstant.outOfFunction",
                )
            }
            "__NAMESPACE__" if self.name_ctx.namespace().is_empty() => (
                "is always empty in global namespace",
                "magicConstant.outOfNamespace",
            ),
            _ => return,
        };
        self.report(
            label.to_string(),
            format!("Magic constant {upper} {message}."),
            span,
            0,
            identifier,
        );
    }
}
