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
use syntax::visit::{Visitor, walk_expr, walk_stmt, walk_stmts};

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
    let mut collector = Collector {
        out: ConstantFacts::default(),
        in_closure: false,
    };
    walk_stmts(&mut collector, program);
    collector.out
}

/// Walks a file noting where it is inside a closure or arrow function. A
/// declaration's body is outside any closure; its parameter defaults,
/// property defaults and constants are not visited.
struct Collector {
    out: ConstantFacts,
    in_closure: bool,
}

impl Collector {
    fn body(&mut self, body: &[Stmt], in_closure: bool) {
        let outer = std::mem::replace(&mut self.in_closure, in_closure);
        walk_stmts(self, body);
        self.in_closure = outer;
    }
}

impl<'ast> Visitor<'ast> for Collector {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => self.body(&f.body, false),
            StmtKind::ClassDecl(c) => {
                for m in &c.methods {
                    self.body(&m.function.body, false);
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Name(name)
                if self.in_closure
                    && ["__FUNCTION__", "__METHOD__"]
                        .iter()
                        .any(|m| name.eq_ignore_ascii_case(m)) =>
            {
                self.out.magic_in_closure.insert(e.span);
            }
            ExprKind::Closure { body, .. } => self.body(body, true),
            ExprKind::ArrowFn { body, .. } => {
                let outer = std::mem::replace(&mut self.in_closure, true);
                self.visit_expr(body);
                self.in_closure = outer;
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                for a in args {
                    self.visit_expr(&a.value);
                }
                for m in &decl.methods {
                    self.body(&m.function.body, false);
                }
            }
            _ => walk_expr(self, e),
        })
    }
}

impl<'a> Checker<'a> {
    /// Called for a bare constant name that resolved to no known
    /// constant.
    pub(crate) fn check_unknown_global_constant(
        &mut self,
        name: &QualifiedName,
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
        let (primary, fallback) = name.candidates();
        let guarded = |n: &str| locals.guarded_constants.contains(n);
        if guarded(bare) || guarded(primary) || fallback.is_some_and(guarded) {
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
        if value.is_implicit_mixed() || self.accepts(&configured, value).yes() {
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
        let name_ty = self.infer_expr(&name_arg.value, locals);
        let Some(name) = name_ty.string_literal_value() else {
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
        let is_unknown = |t: &Type| t.is_mixed() || (t.is_never() || t.is_template());
        if name_ty.union_members().iter().any(is_unknown) {
            return;
        }
        let Some(level) = self.failing_level(name_ty, 0, |m| !m.is_string().yes()) else {
            return;
        };
        self.report(
            format!("Class constant name in dynamic fetch can only be a string, {name_ty} given."),
            span,
            level,
            "classConstant.nameType",
        );
    }
}
