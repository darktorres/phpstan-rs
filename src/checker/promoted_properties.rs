//! Constructor property promotion legality, mirroring real PHPStan's
//! `InvalidPromotedPropertiesRule`: promotion needs PHP 8.0+, is only
//! allowed on a non-abstract constructor, never on a variadic parameter,
//! and `final` promotion needs PHP 8.5+.
//!
//! A whole-file walk of its own rather than a hook inside
//! `check_function`: a trait's methods are checked once per using host
//! there, which would repeat each diagnostic per host.

use crate::ast::*;
use crate::ast_walk::{any_expr_child, stmt_children, stmt_own_exprs};
use crate::checker::{Checker, DiagnosticBuilder};
use crate::php_version::PhpFeature;
use crate::token::Span;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PromotionSite {
    NotConstructor,
    AbstractConstructor,
    Constructor,
}

fn is_promoted(p: &Param) -> bool {
    p.promoted_visibility.is_some() || p.hooks.is_some()
}

impl<'a> Checker<'a> {
    /// Entry point for one top-level statement of `check_program`'s own
    /// loop, so class names resolve against the positional `name_ctx`.
    pub(crate) fn check_promoted_properties_in_stmt(&mut self, stmt: &Stmt, label: &str) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => {
                self.check_promoted_params(&f.params, PromotionSite::NotConstructor, &f.name);
                self.walk_promoted_in_body(&f.body, &f.name);
            }
            StmtKind::ClassDecl(c) => {
                let fqn = self.name_ctx.declared_name(&c.name);
                self.check_promoted_in_methods(&fqn, c.kind, &c.methods);
            }
            StmtKind::EnumDecl(e) => {
                let fqn = self.name_ctx.declared_name(&e.name);
                self.check_promoted_in_methods(&fqn, ClassKind::Enum, &e.methods);
            }
            _ => {
                for e in stmt_own_exprs(stmt) {
                    self.walk_promoted_in_expr(e, label);
                }
                for child in stmt_children(stmt) {
                    self.check_promoted_properties_in_stmt(child, label);
                }
            }
        }
    }

    fn walk_promoted_in_body(&mut self, body: &[Stmt], label: &str) {
        for stmt in body {
            self.check_promoted_properties_in_stmt(stmt, label);
        }
    }

    fn walk_promoted_in_expr(&mut self, e: &Expr, label: &str) {
        match &e.kind {
            ExprKind::Closure { params, body, .. } => {
                self.check_promoted_params(params, PromotionSite::NotConstructor, label);
                self.walk_promoted_in_body(body, label);
            }
            ExprKind::ArrowFn { params, body, .. } => {
                self.check_promoted_params(params, PromotionSite::NotConstructor, label);
                self.walk_promoted_in_expr(body, label);
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                for a in args {
                    self.walk_promoted_in_expr(&a.value, label);
                }
                self.check_promoted_in_methods(&decl.name, decl.kind, &decl.methods);
            }
            _ => {
                any_expr_child(e, &mut |child| {
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
            self.check_promoted_params(&m.function.params, site, &label);
            self.walk_promoted_in_body(&m.function.body, &label);
        }
    }

    /// Whether some class using `trait_fqn` imports its `method` under
    /// the name `__construct` (`use T { init as __construct; }`).
    fn trait_method_aliased_as_constructor(&self, trait_fqn: &str, method: &str) -> bool {
        self.registry
            .classes_using_trait(trait_fqn)
            .iter()
            .filter_map(|host| self.registry.classes.get(host))
            .flat_map(|info| info.trait_aliases.iter())
            .any(|a| {
                a.alias.eq_ignore_ascii_case("__construct")
                    && a.method.eq_ignore_ascii_case(method)
                    && a.trait_name
                        .as_deref()
                        .is_none_or(|t| t.eq_ignore_ascii_case(trait_fqn))
            })
    }

    fn check_promoted_params(&mut self, params: &[Param], site: PromotionSite, label: &str) {
        let Some(first) = params.iter().find(|p| is_promoted(p)) else {
            return;
        };
        let version = self.registry.php_version;
        let unavailable = |feature: PhpFeature| version.is_some_and(|v| !feature.is_available(v));
        let first_span = first.span;
        if unavailable(PhpFeature::PromotedProperties) {
            self.report_promotion(
                label,
                "Promoted properties are supported only on PHP 8.0 and later.",
                first_span,
                "property.promotedNotSupported",
            );
            return;
        }
        match site {
            PromotionSite::NotConstructor => {
                self.report_promotion(
                    label,
                    "Promoted properties can be in constructor only.",
                    first_span,
                    "property.invalidPromoted",
                );
                return;
            }
            PromotionSite::AbstractConstructor => {
                self.report_promotion(
                    label,
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
                self.report_promotion(
                    label,
                    "Final promoted properties are supported only on PHP 8.5 and later.",
                    p.span,
                    "property.invalidPromoted",
                );
            }
            if p.variadic {
                self.report_promotion(
                    label,
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

    fn report_promotion(&mut self, label: &str, message: &str, span: Span, identifier: &str) {
        self.report_builder(
            DiagnosticBuilder::new(label.to_string(), message.to_string(), span, 0, identifier)
                .non_ignorable(),
        );
    }
}
