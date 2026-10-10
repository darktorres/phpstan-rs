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
use syntax::span::Span;
use syntax::visit::arena::{self, Visitor};

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

/// The bit of `FileFacts::mentions` a file that names `__FUNCTION__` or
/// `__METHOD__` has: only such a file has one to find in a closure.
pub(crate) const MENTIONS_MAGIC: u32 = 1 << 5;

/// The spellings that set `MENTIONS_MAGIC`.
pub(crate) const WATCH: &[(&str, u32)] = &[
    ("__function__", MENTIONS_MAGIC),
    ("__method__", MENTIONS_MAGIC),
];

/// Per-file facts the bare-constant checks need, gathered once.
#[derive(Default)]
pub(crate) struct ConstantFacts {
    /// Spans of `__FUNCTION__`/`__METHOD__` inside a closure or arrow
    /// function, where they name `{closure}`.
    pub(crate) magic_in_closure: FxHashSet<Span>,
}

pub(crate) fn constant_facts(arenas: &NodeArenas, program: &[StmtId]) -> ConstantFacts {
    let mut collector = Collector {
        out: ConstantFacts::default(),
        in_closure: false,
    };
    arena::walk_stmts(&mut collector, arenas, program);
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
    fn body(&mut self, arenas: &NodeArenas, body: &[StmtId], in_closure: bool) {
        let outer = std::mem::replace(&mut self.in_closure, in_closure);
        arena::walk_stmts(self, arenas, body);
        self.in_closure = outer;
    }
}

impl Visitor for Collector {
    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        match &stmt.kind {
            StmtKind::FunctionDecl(id) => {
                if let Some(function) = arenas.function(*id) {
                    self.body(arenas, function.body_stmts(arenas), false);
                }
            }
            StmtKind::ClassDecl(c) => {
                let c = arenas.class_decl(c.clone());
                if let Some(def) = arenas.class(c.def) {
                    for m in arenas.methods(def.methods.clone()) {
                        self.body(arenas, m.function.body_stmts(arenas), false);
                    }
                }
            }
            _ => arena::walk_stmt(self, arenas, id),
        }
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Name(name)
                if self.in_closure
                    && ["__FUNCTION__", "__METHOD__"].iter().any(|m| {
                        arenas
                            .qualified_name(name)
                            .is_some_and(|name| name.eq_ignore_ascii_case(m))
                    }) =>
            {
                self.out.magic_in_closure.insert(
                    arenas
                        .expr_span(e.id)
                        .expect("expression has a parallel span"),
                );
            }
            ExprKind::Closure(id) => {
                if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*id) {
                    self.body(arenas, arenas.stmt_ids(&closure.body), true);
                }
            }
            ExprKind::ArrowFn(id) => {
                let Some(ClosurePayload::ArrowFn(arrow)) = arenas.closure(*id) else {
                    return;
                };
                let outer = std::mem::replace(&mut self.in_closure, true);
                self.visit_expr(arenas, arrow.body);
                self.in_closure = outer;
            }
            ExprKind::NewAnonymousClass(id) => {
                let Some(class) = arenas.anonymous_class(*id) else {
                    return;
                };
                for a in arenas.args(class.args.clone()) {
                    self.visit_expr(arenas, a.value);
                }
                if let Some(def) = arenas.class(class.def) {
                    for m in arenas.methods(def.methods.clone()) {
                        self.body(arenas, m.function.body_stmts(arenas), false);
                    }
                }
            }
            _ => arena::walk_expr(self, arenas, id),
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
        let guarded = |n: &str| locals.guarded_constants().contains(n);
        if guarded(bare) || guarded(primary) || fallback.is_some_and(guarded) {
            return;
        }
        report!(
            self,
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
        report!(
            self,
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
        report!(
            self,
            format!(
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
        arenas: &NodeArenas,
        args: &[Arg],
        locals: &crate::checker::Locals,
        span: Span,
    ) {
        let [name_arg, value_arg, ..] = args else {
            return;
        };
        let Some(name_arg) = arenas.expr(name_arg.value) else {
            return;
        };
        let name_ty = self.infer_expr(name_arg, locals);
        let Some(name) = name_ty.string_literal_value() else {
            return;
        };
        let Some(value_arg) = arenas.expr(value_arg.value) else {
            return;
        };
        let value = self.infer_expr(value_arg, locals);
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
            report_builder!(self, 
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
        let is_unknown = |t: &Type| t.is_mixed() || t.is_never() || t.is_template() || t.is_error();
        if name_ty.union_members().iter().any(is_unknown) {
            return;
        }
        let Some(level) = self.failing_level(name_ty, 0, |m| !m.is_string().yes()) else {
            return;
        };
        report!(
            self,
            format!("Class constant name in dynamic fetch can only be a string, {name_ty} given."),
            span,
            level,
            "classConstant.nameType",
        );
    }
}
