use crate::checker::{Checker, Locals, narrow_type_excluding};
use diagnostics::DiagnosticBuilder;
use syntax::ast::*;
use syntax::token::Span;
use types::Type;

impl<'a> Checker<'a> {
    /// The statement half of real PHPStan's own
    /// `CallToFunctionStatementWithNoDiscardRule`/
    /// `CallToMethodStatementWithNoDiscardRule`/
    /// `CallToStaticMethodStatementWithNoDiscardRule` - a bare call
    /// (`foo();`, `$obj->foo();`, `Foo::bar();`) sitting on its own line
    /// as a whole statement, with its return value neither assigned,
    /// returned, nor otherwise consumed, is flagged when the resolved
    /// function/method's own signature carries PHP 8.5's `#[\NoDiscard]`
    /// marker (`FunctionReflection::no_discard`). Called once per
    /// `StmtKind::Expression`, on the statement's own top-level
    /// expression only - a `NoDiscard` call anywhere else
    /// (an argument, the right side of `=`, one operand of a larger
    /// expression) already has its result consumed by whatever it's
    /// nested inside, so it's not this rule's concern at all. Real
    /// PHPStan's own rule also special-cases a `(void)`-cast call as a
    /// separate, weaker diagnostic (still discarding, but explicitly
    /// acknowledged) - not modeled here, since this project's own
    /// `Cast` handling has no reason to distinguish `(void)` from any
    /// other cast target yet; a `(void)`-cast `NoDiscard` call is simply
    /// not flagged at all, the safe (false-negative, never false-
    /// positive) direction to skip in.
    pub(crate) fn check_no_discard_statement(&mut self, e: &Expr, locals: &Locals) {
        let (sig, callee_desc, identifier) = match &e.kind {
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return;
                };
                let (primary, fallback) = self.name_ctx.resolve_function_candidates(name);
                let Some((resolved_name, sig)) = self
                    .registry
                    .lookup_function(&primary)
                    .map(|sig| (primary.as_str(), sig))
                    .or_else(|| {
                        fallback
                            .as_deref()
                            .and_then(|g| self.registry.lookup_function(g).map(|sig| (g, sig)))
                    })
                else {
                    return;
                };
                (
                    sig,
                    format!("{resolved_name}()"),
                    "function.resultDiscarded",
                )
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                let recv_ty = narrow_type_excluding(&self.infer_expr(recv, locals), &Type::Null);
                let Some((owner, sig)) = self.find_method_owner(&recv_ty, method) else {
                    return;
                };
                (
                    sig,
                    format!("{owner}::{method}()"),
                    "method.resultDiscarded",
                )
            }
            ExprKind::StaticCall(target, method, _) => {
                let Some(class) = self.resolve_static_class_name(target.as_name().unwrap_or("?"))
                else {
                    return;
                };
                let Some(sig) = self.registry.lookup_method(&class, method) else {
                    return;
                };
                (
                    sig,
                    format!("{class}::{method}()"),
                    "staticMethod.resultDiscarded",
                )
            }
            _ => return,
        };
        if !sig.no_discard() && !(sig.must_use_result() && self.settings.language_extensions) {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                format!("Call to {callee_desc} on a separate line discards its return value."),
                e.span,
                0,
                identifier,
            )
            .non_ignorable(),
        );
    }

    /// `RandomIntParametersRule`, narrowed to its plain-literal-arguments
    /// case (real PHPStan's own version also handles an
    /// `IntegerRangeType`-typed argument - this project has no such
    /// type, so a non-literal argument is silently skipped). Real PHP:
    /// `random_int(10, 1)` throws an unconditional `ValueError` at call
    /// time whenever `$min` is greater than `$max` - `$min == $max` is
    /// fine (a degenerate but legal single-value range).
    pub(crate) fn check_random_int_call(&mut self, args: &[Arg], locals: &Locals) {
        if args.len() < 2 {
            return;
        }
        let min_ty = self.infer_expr(&args[0].value, locals);
        let max_ty = self.infer_expr(&args[1].value, locals);
        if let (Type::IntLiteral(min), Type::IntLiteral(max)) = (min_ty, max_ty)
            && min > max
        {
            self.report(format!(
                    "random_int(): parameter #1 ($min = {min}) must be less than or equal to parameter #2 ($max = {max})"
                ),
                args[0].value.span,
                0,
                "argument.type",
            );
        }
    }

    /// `DefineParametersRule` - `define()`'s third `$case_insensitive`
    /// argument stopped doing anything as of PHP 8.0 (case-insensitive
    /// constants were removed entirely); real PHP emits "Argument #3
    /// ($case_insensitive) is ignored since declaration of
    /// case-insensitive constants is no longer supported" as a runtime
    /// `E_WARNING` - the exact message text real PHPStan's rule uses.
    /// Gated on `Registry::php_version` being configured and at least
    /// 8.0: with no configured version, this project has nothing to
    /// check against, matching every other `PhpFeature`-gated rule's
    /// "unset means unconfigured, stay silent" convention.
    pub(crate) fn check_define_third_argument(&mut self, args: &[Arg], span: Span) {
        if self.registry.php_supports_everywhere(
            types::php_version::PhpFeature::CaseInsensitiveConstantsRemoved,
        ) != Some(false)
        {
            return;
        }
        if args.len() < 3 {
            return;
        }
        self.report("Argument #3 ($case_insensitive) is ignored since declaration of case-insensitive constants is no longer supported.".to_string(),
            span,
            0,
            "argument.unused",
        );
    }
}
