//! Declaration-level checks run once per function-like (function,
//! method, closure, arrow function): constructor property promotion
//! legality (`InvalidPromotedPropertiesRule` - PHP 8.0+, only on a
//! non-abstract constructor, never variadic, `final` promotion PHP
//! 8.5+), parameter-list sanity (`InvalidParameterNameRule`,
//! `RedefinedParametersRule`, `VariadicParametersDeclarationRule`), and
//! `ReturnNullsafeByRefRule`.
//!
//! A whole-file walk of its own rather than a hook inside
//! `check_function`: a trait's methods are checked once per using host
//! there, which would repeat each diagnostic per host.

use crate::checker::calls::contains_nullsafe_access;
use crate::checker::type_declarations::TypePosition;
use crate::checker::{Checker, Locals};
use crate::registry::class_decl_name;
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::{any_child_expr, child_stmts, stmt_exprs};
use types::Type;
use types::php_version::PhpFeature;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromotionSite {
    NotConstructor,
    AbstractConstructor,
    Constructor,
}

/// Where a function-like's `return` values come from, for
/// `ReturnNullsafeByRefRule`.
enum ReturnSource<'s> {
    Body(&'s [Stmt]),
    Arrow(&'s Expr),
}

const SUPERGLOBALS: [&str; 9] = [
    "GLOBALS", "_SERVER", "_GET", "_POST", "_FILES", "_COOKIE", "_SESSION", "_REQUEST", "_ENV",
];

/// Every `return <expr>` directly in `body`, not inside a nested
/// function-like or class declaration.
fn collect_return_exprs<'s>(body: &'s [Stmt], out: &mut Vec<&'s Expr>) {
    for stmt in body {
        if let StmtKind::Return(Some(e)) = &stmt.kind {
            out.push(e);
        }
        for child in child_stmts(stmt) {
            collect_return_exprs(std::slice::from_ref(child), out);
        }
    }
}

fn is_promoted(p: &Param) -> bool {
    p.promoted_visibility.is_some() || p.hooks.is_some()
}

impl<'a> Checker<'a> {
    /// Entry point for one top-level statement of `check_program`'s own
    /// loop, so class names resolve against the positional `name_ctx`.
    pub(crate) fn check_function_like_declarations_in_stmt(&mut self, stmt: &Stmt, label: &str) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => {
                self.check_function_like_declaration(
                    &f.params,
                    (
                        f.return_type.as_ref().map(TypeHint::to_string).as_deref(),
                        f.return_type.as_ref().map(|h| h.span).unwrap_or(f.span),
                    ),
                    PromotionSite::NotConstructor,
                    f.by_ref,
                    Some(ReturnSource::Body(&f.body)),
                );
                self.walk_promoted_in_body(&f.body, &f.name);
            }
            StmtKind::ClassDecl(c) => {
                let fqn = self.name_ctx.declared_name(class_decl_name(c));
                let previous_class = self.current_class.replace(fqn.clone());
                self.check_promoted_in_methods(&fqn, c.kind, &c.methods);
                self.walk_promoted_in_property_hooks(&fqn, &c.properties);
                self.current_class = previous_class;
            }
            StmtKind::EnumDecl(e) => {
                let fqn = self.name_ctx.declared_name(&e.name);
                let previous_class = self.current_class.replace(fqn.clone());
                self.check_promoted_in_methods(&fqn, ClassKind::Enum, &e.methods);
                self.current_class = previous_class;
            }
            _ => {
                for e in stmt_exprs(stmt) {
                    self.walk_promoted_in_expr(e, label);
                }
                for child in child_stmts(stmt) {
                    self.check_function_like_declarations_in_stmt(child, label);
                }
            }
        }
    }

    fn walk_promoted_in_body(&mut self, body: &[Stmt], label: &str) {
        for stmt in body {
            self.check_function_like_declarations_in_stmt(stmt, label);
        }
    }

    fn walk_promoted_in_expr(&mut self, e: &Expr, label: &str) {
        match &e.kind {
            ExprKind::Closure {
                params,
                body,
                by_ref,
                return_type,
                ..
            } => {
                self.check_function_like_declaration(
                    params,
                    (
                        return_type.as_deref().map(TypeHint::to_string).as_deref(),
                        e.span,
                    ),
                    PromotionSite::NotConstructor,
                    *by_ref,
                    Some(ReturnSource::Body(body)),
                );
                self.check_literal_default_parameter_types(params, "anonymous function");
                self.walk_promoted_in_body(body, label);
            }
            ExprKind::ArrowFn {
                params,
                body,
                by_ref,
                return_type,
                ..
            } => {
                self.check_function_like_declaration(
                    params,
                    (
                        return_type.as_deref().map(TypeHint::to_string).as_deref(),
                        e.span,
                    ),
                    PromotionSite::NotConstructor,
                    *by_ref,
                    Some(ReturnSource::Arrow(body)),
                );
                self.check_literal_default_parameter_types(params, "arrow function");
                self.walk_promoted_in_expr(body, label);
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                for a in args {
                    self.walk_promoted_in_expr(&a.value, label);
                }
                let registry_name = self.anonymous_class_names.get(&e.span).cloned();
                let previous_class =
                    std::mem::replace(&mut self.current_class, registry_name.clone());
                self.check_promoted_in_methods(class_decl_name(decl), decl.kind, &decl.methods);
                self.walk_promoted_in_property_hooks(class_decl_name(decl), &decl.properties);
                self.current_class = previous_class;
            }
            _ => {
                any_child_expr(e, &mut |child| {
                    self.walk_promoted_in_expr(child, label);
                    false
                });
            }
        }
    }

    fn check_promoted_in_methods(&mut self, fqn: &str, kind: ClassKind, methods: &[MethodDecl]) {
        for m in methods {
            let label = format!("{fqn}::{}", m.function.name);
            let is_constructor = m.function.name.eq_ignore_ascii_case("__construct")
                || (kind == ClassKind::Trait
                    && self.trait_method_aliased_as_constructor(fqn, &m.function.name));
            let site = if !is_constructor {
                PromotionSite::NotConstructor
            } else if m.is_abstract || kind == ClassKind::Interface {
                PromotionSite::AbstractConstructor
            } else {
                PromotionSite::Constructor
            };
            let returns = m
                .function
                .has_body
                .then_some(ReturnSource::Body(&m.function.body));
            self.check_function_like_declaration(
                &m.function.params,
                (
                    m.function
                        .return_type
                        .as_ref()
                        .map(TypeHint::to_string)
                        .as_deref(),
                    m.function
                        .return_type
                        .as_ref()
                        .map(|h| h.span)
                        .unwrap_or(m.function.span),
                ),
                site,
                m.function.by_ref,
                returns,
            );
            self.walk_promoted_in_body(&m.function.body, &label);
        }
    }

    fn walk_promoted_in_property_hooks(&mut self, fqn: &str, properties: &[PropertyDecl]) {
        for p in properties {
            let Some(hooks) = &p.hooks else {
                continue;
            };
            let label = format!("{fqn}::${}", p.name);
            if let Some((Some(param), _)) = &hooks.set {
                self.check_parameter_name(param);
            }
            let bodies = hooks
                .get
                .iter()
                .chain(hooks.set.iter().map(|(_, body)| body));
            for body in bodies {
                match body {
                    PropertyHookBody::Arrow(e) => self.walk_promoted_in_expr(e, &label),
                    PropertyHookBody::Block(stmts) => self.walk_promoted_in_body(stmts, &label),
                    PropertyHookBody::Abstract => {}
                }
            }
        }
    }

    /// Whether some class using `trait_fqn` imports its `method` under
    /// the name `__construct` (`use T { init as __construct; }`).
    fn trait_method_aliased_as_constructor(&self, trait_fqn: &str, method: &str) -> bool {
        self.registry
            .classes_using_trait(trait_fqn)
            .iter()
            .filter_map(|host| self.registry.classes.get(host))
            .flat_map(|info| info.trait_aliases().iter())
            .any(|a| {
                a.alias.eq_ignore_ascii_case("__construct")
                    && a.method.eq_ignore_ascii_case(method)
                    && a.trait_name
                        .as_deref()
                        .is_none_or(|t| t.eq_ignore_ascii_case(trait_fqn))
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn check_function_like_declaration(
        &mut self,
        params: &[Param],
        return_type: (Option<&str>, Span),
        site: PromotionSite,
        by_ref: bool,
        returns: Option<ReturnSource>,
    ) {
        self.check_promoted_params(params, site);
        for p in params {
            self.check_type_declaration(
                p.type_hint.as_ref().map(TypeHint::to_string).as_deref(),
                TypePosition::Parameter,
                p.type_hint.as_ref().map(|h| h.span).unwrap_or(p.span),
            );
        }
        self.check_type_declaration(return_type.0, TypePosition::Return, return_type.1);
        if let Some(ReturnSource::Body(body)) = &returns {
            self.check_statement_keywords(body);
        }
        let mut seen: Vec<&str> = Vec::new();
        for (i, p) in params.iter().enumerate() {
            self.check_parameter_name(p);
            if seen.contains(&p.name.as_ref()) {
                self.report_declaration(
                    &format!("Redefinition of parameter ${}.", p.name),
                    p.span,
                    "parameter.duplicate",
                );
            } else {
                seen.push(&p.name);
            }
            if p.variadic && i + 1 != params.len() {
                self.report_declaration(
                    "Only the last parameter can be variadic.",
                    p.span,
                    "parameter.variadicNotLast",
                );
            }
        }
        if by_ref {
            let mut return_exprs = Vec::new();
            match returns {
                Some(ReturnSource::Body(body)) => collect_return_exprs(body, &mut return_exprs),
                Some(ReturnSource::Arrow(e)) => return_exprs.push(e),
                None => {}
            }
            for e in return_exprs {
                if contains_nullsafe_access(e) {
                    self.report_declaration(
                        "Nullsafe cannot be returned by reference.",
                        e.span,
                        "nullsafe.byRef",
                    );
                }
            }
        }
    }

    /// `IncompatibleClosureDefaultParameterTypeRule`/
    /// `IncompatibleArrowFunctionDefaultParameterTypeRule` - a closure or
    /// arrow function has no `FunctionReflection`, so each parameter's native
    /// hint is resolved here. A `null` default makes a typed parameter
    /// implicitly nullable, and a default of unknown type is always
    /// accepted, so neither is a mismatch.
    fn check_literal_default_parameter_types(&mut self, params: &[Param], kind: &str) {
        for (i, p) in params.iter().enumerate() {
            let Some(default) = &p.default else {
                continue;
            };
            if p.variadic || p.type_hint.is_none() {
                continue;
            }
            if matches!(default.kind, ExprKind::Literal(Literal::Null)) {
                continue;
            }
            let param_ty =
                self.resolve_type_hint(p.type_hint.as_ref().map(TypeHint::to_string).as_deref());
            let param_ty = match &param_ty {
                Type::TemplateParam(tp) => tp.bound.as_ref().clone(),
                other => other.clone(),
            };
            let param_ty = match &self.current_class {
                Some(class) => self.resolve_pseudo(&param_ty, &class.clone()),
                None => param_ty,
            };
            let default_ty = self.infer_expr(default, &Locals::default());
            if matches!(default_ty, Type::Mixed(_)) || self.accepts(&param_ty, &default_ty).yes() {
                continue;
            }
            self.report(format!(
                    "Default value of the parameter #{} ${} ({default_ty}) of {kind} is incompatible with type {param_ty}.",
                    i + 1,
                    p.name,
                ),
                default.span,
                2,
                "parameter.defaultValue",
            );
        }
    }

    fn check_parameter_name(&mut self, p: &Param) {
        if SUPERGLOBALS.contains(&p.name.as_ref()) {
            self.report_declaration(
                &format!(
                    "Superglobal variable ${} cannot be used as a parameter.",
                    p.name
                ),
                p.span,
                "parameter.superglobal",
            );
        } else if p.name.as_ref() == "this" {
            self.report_declaration("Cannot use $this as parameter.", p.span, "parameter.this");
        }
    }

    fn check_promoted_params(&mut self, params: &[Param], site: PromotionSite) {
        let Some(first) = params.iter().find(|p| is_promoted(p)) else {
            return;
        };
        let unavailable =
            |feature: PhpFeature| self.registry.php_supports_everywhere(feature) == Some(false);
        let first_span = first.span;
        if unavailable(PhpFeature::PromotedProperties) {
            self.report_declaration(
                "Promoted properties are supported only on PHP 8.0 and later.",
                first_span,
                "property.promotedNotSupported",
            );
            return;
        }
        match site {
            PromotionSite::NotConstructor => {
                self.report_declaration(
                    "Promoted properties can be in constructor only.",
                    first_span,
                    "property.invalidPromoted",
                );
                return;
            }
            PromotionSite::AbstractConstructor => {
                self.report_declaration(
                    "Promoted properties are not allowed in abstract constructors.",
                    first_span,
                    "property.invalidPromoted",
                );
                return;
            }
            PromotionSite::Constructor => {}
        }
        let final_unavailable = unavailable(PhpFeature::FinalPromotedProperties);
        for p in params.iter().filter(|p| p.promoted_visibility.is_some()) {
            if p.promoted_final && final_unavailable {
                self.report_declaration(
                    "Final promoted properties are supported only on PHP 8.5 and later.",
                    p.span,
                    "property.invalidPromoted",
                );
            }
            if p.variadic {
                self.report_declaration(
                    &format!(
                        "Promoted property parameter ${} can not be variadic.",
                        p.name
                    ),
                    p.span,
                    "property.invalidPromoted",
                );
            }
        }
    }

    fn report_declaration(&mut self, message: &str, span: Span, identifier: &'static str) {
        self.report_builder(
            DiagnosticBuilder::new(message.to_string(), span, 0, identifier).non_ignorable(),
        );
    }
}
