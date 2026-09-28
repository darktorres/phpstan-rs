//! Bare constant references. At level 1, `ConstantRule`
//! (`constant.notFound`) flags a global constant no `const`/`define()`
//! anywhere in the project and no stub declares, except where a
//! `defined('NAME')` test has proven it exists (`Locals::
//! guarded_constants`).
//! `MagicConstantContextRule` flags a magic constant used where it's
//! always empty.

use rustc_hash::FxHashSet;

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::{any_child_expr, child_stmts, stmt_exprs};

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
    /// Spans of `__FUNCTION__`/`__METHOD__` inside a closure or arrow
    /// function, where they name `{closure}`.
    pub(crate) magic_in_closure: FxHashSet<Span>,
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
            for e in stmt_exprs(stmt) {
                collect_in_expr(e, in_closure, out);
            }
            for child in child_stmts(stmt) {
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
            any_child_expr(e, &mut |child| {
                collect_in_expr(child, in_closure, out);
                false
            });
        }
    }
}

impl<'a> Checker<'a> {
    /// Called for a bare constant name that resolved to no known
    /// constant.
    pub(crate) fn check_unknown_global_constant(
        &mut self,
        name: &str,
        locals: &crate::checker::Locals,
        span: Span,
    ) {
        let bare = name.trim_start_matches('\\');
        let unqualified = bare.rsplit('\\').next().unwrap_or(bare);
        if ["true", "false", "null"]
            .iter()
            .any(|kw| unqualified.eq_ignore_ascii_case(kw))
            || MAGIC_CONSTANTS
                .iter()
                .any(|m| unqualified.eq_ignore_ascii_case(m))
        {
            return;
        }
        let (primary, fallback) = self.name_ctx.resolve_constant_candidates(name);
        let guarded = |n: &str| locals.guarded_constants.contains(n);
        if guarded(bare)
            || guarded(primary.trim_start_matches('\\'))
            || fallback.as_deref().is_some_and(guarded)
        {
            return;
        }
        self.report(
            format!("Constant {bare} not found."),
            span,
            1,
            "constant.notFound",
        );
    }

    /// `MagicConstantContextRule`: `__CLASS__` outside a class,
    /// `__TRAIT__` outside a trait, `__FUNCTION__`/`__METHOD__` outside
    /// any function-like, `__NAMESPACE__` in the global namespace.
    pub(crate) fn check_magic_constant_context(&mut self, name: &str, span: Span) {
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
            format!("Magic constant {upper} {message}."),
            span,
            0,
            identifier,
        );
    }

    /// `ValueAssignedToDefineRule`/`ValueAssignedToGlobalConstantRule`
    /// (`constant.defineValue`/`constant.value`, level 2): a declared
    /// value the constant's configured `dynamicConstantNames` type
    /// doesn't accept.
    pub(crate) fn check_configured_constant_value(
        &mut self,
        class: Option<&str>,
        name: &str,
        value: &types::Type,
        identifier: &'static str,
        span: Span,
    ) {
        let Some(configured) = self
            .settings
            .constants
            .configured_type(class, name)
            .cloned()
        else {
            return;
        };
        if matches!(value, types::Type::Mixed(false)) || self.accepts(&configured, value).yes() {
            return;
        }
        let display = match class {
            Some(class) => format!("{class}::{name}"),
            None => name.trim_start_matches('\\').to_string(),
        };
        self.report(format!(
                "Configuration defined type for constant {display} ({configured}) does not accept value {value}."
            ),
            span,
            2,
            identifier,
        );
    }

    /// `define('NAME', $value)` against a configured type.
    pub(crate) fn check_define_value(
        &mut self,
        args: &[Arg],
        locals: &crate::checker::Locals,
        span: Span,
    ) {
        let [name_arg, value_arg, ..] = args else {
            return;
        };
        let types::Type::StringLiteral(name) = self.infer_expr(&name_arg.value, locals) else {
            return;
        };
        let value = self.infer_expr(&value_arg.value, locals);
        let Some(name) = name.as_str() else {
            return;
        };
        self.check_configured_constant_value(None, name, &value, "constant.defineValue", span);
    }

    /// `DynamicClassConstantFetchRule`: `Foo::{$expr}` needs PHP 8.3+
    /// (`classConstant.dynamicFetch`), and its name must be a string
    /// (`classConstant.nameType`; level 7 when only some union members
    /// aren't).
    pub(crate) fn check_dynamic_class_constant_fetch(&mut self, name_ty: &types::Type, span: Span) {
        use types::Type;
        if self
            .registry
            .php_supports_everywhere(types::php_version::PhpFeature::DynamicClassConstantFetch)
            == Some(false)
        {
            self.report_builder(
                DiagnosticBuilder::new("Fetching class constants with a dynamic name is supported only on PHP 8.3 and later."
                        .to_string(),
                    span,
                    0,
                    "classConstant.dynamicFetch",
                )
                .non_ignorable(),
            );
            return;
        }
        let is_string = |t: &Type| {
            matches!(
                t,
                Type::String
                    | Type::StringLiteral(_)
                    | Type::ClassString(_)
                    | Type::ClassKindString(_)
            ) || matches!(t, Type::Intersection(ms) if ms.iter().any(|m| matches!(m, Type::String)))
        };
        let is_unknown =
            |t: &Type| matches!(t, Type::Mixed(_) | Type::Never | Type::TemplateParam(_));
        let level = match name_ty {
            t if is_string(t) || is_unknown(t) => return,
            Type::Union(members) if members.iter().any(is_unknown) => return,
            Type::Union(members) if members.iter().any(is_string) => 7,
            _ => 0,
        };
        self.report(
            format!("Class constant name in dynamic fetch can only be a string, {name_ty} given."),
            span,
            level,
            "classConstant.nameType",
        );
    }
}
