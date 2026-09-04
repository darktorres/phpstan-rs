//! Phase 40: flags a third-party project's own references into real
//! PHPStan's `PHPStan\*` namespace that aren't marked `@api` (or, for an
//! `extends`/`implements`/trait `use`, specifically marked
//! `@api-do-not-implement`) - PHPStan's own documented backward-
//! compatibility promise (see `vendor/phpstan-src`'s own `CLAUDE.md`:
//! "Code marked with `@api` must not break backward compatibility... Any
//! code without `@api` may change in minor versions without notice").
//! Mirrors real PHPStan's own `Rules\Api\*` rule family
//! (`ApiInstantiationRule`, `ApiStaticCallRule`, `ApiClassConstFetchRule`,
//! `ApiInstanceofRule`, `ApiClassExtendsRule`, `ApiClassImplementsRule`,
//! `ApiInterfaceExtendsRule`, `ApiTraitUseRule`, plus a syntactically-typed
//! subset of `ApiMethodCallRule` - see below) - this is the mirror-image
//! feature to Phase 18's future extension-API surface (phpstan-rs
//! protecting *its own* API), not that feature itself: this checks code
//! that *consumes* real PHPStan as a Composer library.
//!
//! Only ever called when `composer::detect_phpstan_dependency` found a
//! real `phpstan/phpstan`/`phpstan/extension-installer` dependency in the
//! analyzed project's own `composer.json` (`Project::check_file`) - a
//! project with no such dependency has no backward-compatibility promise
//! to check anything against, and (mirroring the real corpus reasoning
//! `pest_config`'s own module doc gives for its equivalent gate) running
//! this unconditionally would risk flagging a project's own, unrelated
//! `PHPStan\`-namespaced code with a diagnostic that means nothing to it.
//!
//! **Namespace signal only, deliberately not also a vendor-path check**:
//! real PHPStan's own `ApiRuleHelper::isPhpStanCode` additionally confirms
//! the referenced declaration's own file lives under a `vendor/`
//! directory, specifically so analyzing phpstan-src's own source against
//! itself doesn't flag its *own* internal, same-namespace references. This
//! project's registry doesn't track a per-class declaring file path at
//! all (`registry::ClassInfo` has never needed one), and the existing
//! vendor-context split (`composer::discover_vendor_context`,
//! `main.rs::run_check_project`'s own `context_files`/`analyze_files`
//! separation) already guarantees a real vendor-loaded `PHPStan\*` class
//! is never itself among the files this check - or any check - reports
//! diagnostics *on*; it only ever needs to know whether such a class is
//! `@api` when some *other*, actually-analyzed file references it. A
//! first-party project declaring its own classes directly under the
//! `PHPStan\` namespace without actually being PHPStan itself is exactly
//! the kind of pathological self-inflicted collision `ApiRuleHelper`'s
//! path check exists to rule out for phpstan-src's own corpus - real,
//! but not something any corpus evidence available to this project
//! justifies building the extra registry plumbing for.
//!
//! **Reference kinds covered** (each only against a name the registry
//! actually has an entry for - an unresolved/typo'd name already gets its
//! own, more meaningful undefined-class diagnostic elsewhere, and isn't
//! double-reported here): `new X(...)`, `X::staticMethod()`, `X::CONST`
//! (but not `X::class` - see below), `$x instanceof X`, `extends X`,
//! `implements X`, `use X;` (trait), plus one syntactically-typed slice of
//! instance method calls: `$scope->getType(...)` where `$scope` is a
//! directly-typed parameter (or closure/arrow-fn `use`-captured variable)
//! of the enclosing function - not general receiver-type inference (this
//! project's full `Checker` does that, but only mid-traversal, with no
//! per-node type retained afterward for a second, separate pass to reuse
//! cheaply). A call through a constructor-promoted/injected property
//! (`$this->apiRuleHelper->isPhpStanCode(...)`, a real, common shape in
//! extension code doing its own internal delegation) is a known,
//! disclosed gap this first pass doesn't cover.
//!
//! `X::class` is deliberately never flagged: real PHPStan's own
//! `ApiClassConstFetchRule` does flag it, but only after also checking
//! every one of the class's own methods for a stray `@api` in their doc
//! comment (a special case this project has no equivalent machinery for -
//! method-level `@api` isn't parsed into `registry::FunctionSig` at all,
//! only the class-level tag is, via `registry::ClassInfo::is_api`).
//! Flagging bare `SomeExtensionInterface::class` - an extremely common,
//! benign idiom for naming a class in a DI container registration or
//! `#[RegisteredRule]`-style attribute, not actually "using" its API
//! surface - without that nuance would be pure noise.

use std::collections::HashMap;

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::checker::Diagnostic;
use crate::registry::{NameContext, Registry};
use crate::token::Span;

/// `$variable-name -> resolved fully-qualified class name` for whichever
/// of the enclosing function/method/closure/arrow-fn's own parameters (or
/// closure `use`-captures) carry a single, unambiguous native class type
/// hint - see `param_types_with`'s own doc comment for exactly what
/// counts. Not filtered to `PHPStan\*` at collection time (unlike every
/// other reference kind's own `resolve_if_phpstan` check) - collection
/// stays reusable/simple, and every lookup site re-checks the namespace
/// itself.
type ParamTypes = HashMap<String, String>;

pub(crate) fn check_phpstan_api_usage(program: &[Stmt], registry: &Registry) -> Vec<Diagnostic> {
    // Same "last `namespace` seen wins, brace-delimited multi-namespace
    // files are rare enough not to bother" simplification
    // `Checker::check_program` itself already makes for exactly the same
    // reason (see its own comment there).
    let ctx = NameContext::build(program);
    let mut walker = Walker {
        registry,
        diagnostics: Vec::new(),
    };
    let empty = ParamTypes::new();
    for stmt in program {
        walker.check_stmt(stmt, &ctx, &empty, "<main>");
    }
    walker.diagnostics
}

/// Case-insensitive `PHPStan\`-prefix match (mirrors real PHPStan's own
/// `ApiRuleHelper::isPhpStanName`'s `stripos($namespace, 'PHPStan\\') ===
/// 0`), excluding the two real, separately-maintained third-party
/// libraries PHPStan bundles under its own namespace umbrella
/// (`phpstan/phpdoc-parser`, `ondrejmirtes/better-reflection`) rather than
/// actual PHPStan API surface - excluded via a case-*sensitive* prefix
/// match, deliberately mirroring `ApiRuleHelper`'s own `str_starts_with`
/// exactly (no case-insensitivity exemption for either sub-namespace).
fn is_phpstan_namespace(fqn: &str) -> bool {
    fqn.len() >= 8
        && fqn.as_bytes()[..8].eq_ignore_ascii_case(b"PHPStan\\")
        && !fqn.starts_with("PHPStan\\PhpDocParser\\")
        && !fqn.starts_with("PHPStan\\BetterReflection\\")
}

/// A single class-shaped native type hint (`Param::type_hint`) resolved to
/// a fully-qualified name, or `None` for anything not simple/unambiguous
/// enough to trust: no hint at all, a union/intersection (`Foo|Bar`,
/// `A&B`), or an empty string. A leading `?` (nullable) is stripped first -
/// `?Scope $scope` still names `Scope` unambiguously. No primitive/pseudo-
/// type keyword filter is needed here: `is_phpstan_namespace` already
/// rejects anything a keyword like `self`/`static`/`array`/`int` could
/// ever resolve to, so a keyword slipping through this function costs
/// nothing.
fn resolved_param_class(p: &Param, ctx: &NameContext) -> Option<String> {
    let hint = p.type_hint.as_deref()?;
    let bare = hint.strip_prefix('?').unwrap_or(hint);
    if bare.is_empty() || bare.contains('|') || bare.contains('&') {
        return None;
    }
    Some(ctx.resolve(bare))
}

/// Builds a fresh `ParamTypes` map for one function-like body: every entry
/// from `base` not shadowed by one of `params`' own names, plus a fresh
/// entry (or an explicit removal, if the param's own hint isn't a simple
/// class reference) for each of `params` themselves. Shared by
/// `check_function` (called with an empty `base` - an ordinary function/
/// method has no enclosing lexical scope to inherit from) and the
/// `Closure`/`ArrowFn` arms of `check_expr` (called with whatever the
/// enclosing scope already resolved, filtered down first for `Closure`'s
/// own explicit `use (...)` capture list - PHP's real capture rule for a
/// full closure, unlike an arrow function's own implicit
/// capture-everything-referenced rule, which is why `ArrowFn` passes its
/// *entire* enclosing map through unfiltered instead).
fn param_types_with(base: &ParamTypes, params: &[Param], ctx: &NameContext) -> ParamTypes {
    let mut out = base.clone();
    for p in params {
        out.remove(&p.name);
        if let Some(fqn) = resolved_param_class(p, ctx) {
            out.insert(p.name.clone(), fqn);
        }
    }
    out
}

struct Walker<'a> {
    registry: &'a Registry,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Walker<'a> {
    fn resolve_if_phpstan(&self, name: &str, ctx: &NameContext) -> Option<String> {
        let fqn = ctx.resolve(name);
        is_phpstan_namespace(&fqn).then_some(fqn)
    }

    /// "Referencing this at all requires `@api`" - covers every reference
    /// kind except the inheritance-shaped ones (`report_inheritance`
    /// below handles those, since an `@api-do-not-implement` class is
    /// still perfectly fine to *call*, just not to `extends`/
    /// `implements`/`use`).
    /// `identifier` matches real PHPStan's own `Rules\Api\*` identifier
    /// strings one-for-one (see e.g. `ApiMethodCallRule`'s
    /// `phpstanApi.method`) - threaded in by each call site below rather
    /// than derived here, since the identifier depends on the reference
    /// *kind* (method call vs. instantiation vs. instanceof, ...), not on
    /// anything `report_not_covered` itself knows.
    fn report_not_covered(
        &mut self,
        fqn: &str,
        action: &str,
        span: Span,
        label: &str,
        identifier: &str,
    ) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.is_api {
            return;
        }
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!(
                "{action} is not covered by PHPStan's backward compatibility promise (missing @api on {fqn})"
            ),
            span,
            level: 0,
            identifier: identifier.to_string(),
            tip: None,
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    fn report_inheritance(
        &mut self,
        fqn: &str,
        action: &str,
        span: Span,
        label: &str,
        identifier: &str,
    ) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.is_api_do_not_implement {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{action} is not supported: {fqn} is marked @api-do-not-implement and may gain new members in a minor version"
                ),
                span,
                level: 0,
                identifier: identifier.to_string(),
                tip: None,
                non_ignorable: false,
                metadata: HashMap::new(),
                fix: None,
            });
            return;
        }
        if !info.is_api {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{action} is not covered by PHPStan's backward compatibility promise (missing @api on {fqn})"
                ),
                span,
                level: 0,
                identifier: identifier.to_string(),
                tip: None,
                non_ignorable: false,
                metadata: HashMap::new(),
                fix: None,
            });
        }
    }

    fn check_function(&mut self, f: &FunctionDecl, ctx: &NameContext, label: &str) {
        let param_types = param_types_with(&ParamTypes::new(), &f.params, ctx);
        for s in &f.body {
            self.check_stmt(s, ctx, &param_types, label);
        }
        for p in &f.params {
            if let Some(d) = &p.default {
                self.check_expr(d, ctx, &param_types, label);
            }
        }
    }

    fn check_class(&mut self, c: &ClassDecl, ctx: &NameContext) {
        let fqn = ctx.declared_name(&c.name);
        for parent in &c.extends {
            if let Some(pfqn) = self.resolve_if_phpstan(parent, ctx) {
                let action = format!("extending {pfqn}");
                self.report_inheritance(&pfqn, &action, c.span, &fqn, "phpstanApi.class");
            }
        }
        for iface in &c.implements {
            if let Some(ifqn) = self.resolve_if_phpstan(iface, ctx) {
                let action = format!("implementing {ifqn}");
                self.report_inheritance(&ifqn, &action, c.span, &fqn, "phpstanApi.interface");
            }
        }
        for used in &c.uses {
            if let Some(ufqn) = self.resolve_if_phpstan(used, ctx) {
                let action = format!("using trait {ufqn}");
                self.report_inheritance(&ufqn, &action, c.span, &fqn, "phpstanApi.trait");
            }
        }
        let empty = ParamTypes::new();
        for p in &c.properties {
            if let Some(d) = &p.default {
                self.check_expr(d, ctx, &empty, &fqn);
            }
        }
        for cst in &c.consts {
            self.check_expr(&cst.value, ctx, &empty, &fqn);
        }
        for m in &c.methods {
            let method_label = format!("{fqn}::{}", m.function.name);
            self.check_function(&m.function, ctx, &method_label);
        }
    }

    fn check_enum(&mut self, e: &EnumDecl, ctx: &NameContext) {
        let fqn = ctx.declared_name(&e.name);
        for iface in &e.implements {
            if let Some(ifqn) = self.resolve_if_phpstan(iface, ctx) {
                let action = format!("implementing {ifqn}");
                self.report_inheritance(&ifqn, &action, e.span, &fqn, "phpstanApi.interface");
            }
        }
        for used in &e.uses {
            if let Some(ufqn) = self.resolve_if_phpstan(used, ctx) {
                let action = format!("using trait {ufqn}");
                self.report_inheritance(&ufqn, &action, e.span, &fqn, "phpstanApi.trait");
            }
        }
        let empty = ParamTypes::new();
        for cst in &e.consts {
            self.check_expr(&cst.value, ctx, &empty, &fqn);
        }
        for case in &e.cases {
            if let Some(v) = &case.value {
                self.check_expr(v, ctx, &empty, &fqn);
            }
        }
        for m in &e.methods {
            let method_label = format!("{fqn}::{}", m.function.name);
            self.check_function(&m.function, ctx, &method_label);
        }
    }

    /// Mirrors `deps::collect_stmt`'s exact shape - see that function's
    /// own doc comment for why the recursion is split this way
    /// (`stmt_children` for nested *statements*, this match for a
    /// variant's own non-statement fields, with `FunctionDecl`/
    /// `ClassDecl`/`EnumDecl` needing both since they have their own
    /// dedicated declaration-walker instead of being flattened into an
    /// ordinary statement list).
    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        ctx: &NameContext,
        param_types: &ParamTypes,
        label: &str,
    ) {
        match &stmt.kind {
            StmtKind::Block(_)
            | StmtKind::InlineHtml(_)
            | StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Namespace(_)
            | StmtKind::Use(_)
            | StmtKind::Global(_)
            | StmtKind::Goto(_)
            | StmtKind::Label(_) => {}
            StmtKind::Expression(e) | StmtKind::Throw(e) => {
                self.check_expr(e, ctx, param_types, label)
            }
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.check_expr(e, ctx, param_types, label);
                }
            }
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    self.check_expr(e, ctx, param_types, label);
                }
            }
            StmtKind::If { cond, .. }
            | StmtKind::While { cond, .. }
            | StmtKind::DoWhile { cond, .. } => {
                self.check_expr(cond, ctx, param_types, label);
            }
            StmtKind::For {
                init, cond, step, ..
            } => {
                for e in init.iter().chain(cond).chain(step) {
                    self.check_expr(e, ctx, param_types, label);
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                ..
            } => {
                self.check_expr(subject, ctx, param_types, label);
                if let Some(k) = key {
                    self.check_expr(k, ctx, param_types, label);
                }
                self.check_expr(value, ctx, param_types, label);
            }
            StmtKind::FunctionDecl(f) => self.check_function(f, ctx, &f.name),
            StmtKind::ClassDecl(c) => self.check_class(c, ctx),
            StmtKind::EnumDecl(e) => self.check_enum(e, ctx),
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.check_expr(&c.value, ctx, param_types, label);
                }
            }
            StmtKind::StaticVar(vars) => {
                for (_, d) in vars {
                    if let Some(e) = d {
                        self.check_expr(e, ctx, param_types, label);
                    }
                }
            }
            StmtKind::Switch { subject, cases } => {
                self.check_expr(subject, ctx, param_types, label);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.check_expr(t, ctx, param_types, label);
                    }
                }
            }
            StmtKind::Try { .. } => {}
        }
        for child in stmt_children(stmt) {
            self.check_stmt(child, ctx, param_types, label);
        }
    }

    /// Mirrors `deps::collect_expr`'s exact shape - see that function's
    /// own doc comment. Each name-bearing reference kind is checked
    /// directly against `self.registry`'s `@api`/`@api-do-not-implement`
    /// state (never generically recursed into via `expr_children` a
    /// second time for the same span - every arm below either fully
    /// covers its own children by hand, matching what `expr_children`
    /// would otherwise return, or falls through to the catch-all).
    fn check_expr(&mut self, e: &Expr, ctx: &NameContext, param_types: &ParamTypes, label: &str) {
        match &e.kind {
            ExprKind::New(callee, args) => {
                if let ExprKind::Name(n) = &callee.kind {
                    if let Some(fqn) = self.resolve_if_phpstan(n, ctx) {
                        let action = format!("instantiating {fqn}");
                        self.report_not_covered(
                            &fqn,
                            &action,
                            e.span,
                            label,
                            "phpstanApi.constructor",
                        );
                    }
                } else {
                    self.check_expr(callee, ctx, param_types, label);
                }
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            ExprKind::StaticCall(class_name, method, args) => {
                if let Some(fqn) = self.resolve_if_phpstan(class_name, ctx) {
                    let action = format!("calling {fqn}::{method}()");
                    self.report_not_covered(&fqn, &action, e.span, label, "phpstanApi.method");
                }
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            // `X::class` is deliberately exempt - see this module's own
            // doc comment for why.
            ExprKind::StaticAccess(class_name, member) if member != "class" => {
                if let Some(fqn) = self.resolve_if_phpstan(class_name, ctx) {
                    let action = format!("accessing {fqn}::{member}");
                    self.report_not_covered(
                        &fqn,
                        &action,
                        e.span,
                        label,
                        "phpstanApi.classConstant",
                    );
                }
            }
            ExprKind::MethodCall(recv, method, args, _) => {
                if let ExprKind::Variable(name) = &recv.kind
                    && let Some(fqn) = param_types.get(name)
                    && is_phpstan_namespace(fqn)
                {
                    let action = format!("calling {fqn}::{method}()");
                    self.report_not_covered(fqn, &action, e.span, label, "phpstanApi.method");
                }
                self.check_expr(recv, ctx, param_types, label);
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            ExprKind::Instanceof(inner, InstanceofTarget::Name(n)) => {
                if let Some(fqn) = self.resolve_if_phpstan(n, ctx) {
                    let action = format!("instanceof {fqn}");
                    self.report_not_covered(
                        &fqn,
                        &action,
                        e.span,
                        label,
                        "phpstanApi.instanceofAssumption",
                    );
                }
                self.check_expr(inner, ctx, param_types, label);
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                self.check_class(decl, ctx);
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            ExprKind::Closure {
                params, uses, body, ..
            } => {
                let captured: ParamTypes = uses
                    .iter()
                    .filter_map(|u| {
                        param_types
                            .get(&u.name)
                            .map(|ty| (u.name.clone(), ty.clone()))
                    })
                    .collect();
                let nested = param_types_with(&captured, params, ctx);
                for s in body {
                    self.check_stmt(s, ctx, &nested, label);
                }
            }
            ExprKind::ArrowFn { params, body, .. } => {
                let nested = param_types_with(param_types, params, ctx);
                self.check_expr(body, ctx, &nested, label);
            }
            _ => {
                for child in expr_children(e) {
                    self.check_expr(child, ctx, param_types, label);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
