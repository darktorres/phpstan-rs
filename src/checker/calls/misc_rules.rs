use crate::ast::*;
use crate::checker::calls::is_language_construct;
use crate::checker::{Checker, DiagnosticBuilder, Locals, narrow_type_excluding};
use crate::token::Span;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// Phase 37: the statement half of real PHPStan's own
    /// `CallToFunctionStatementWithNoDiscardRule`/
    /// `CallToMethodStatementWithNoDiscardRule`/
    /// `CallToStaticMethodStatementWithNoDiscardRule` - a bare call
    /// (`foo();`, `$obj->foo();`, `Foo::bar();`) sitting on its own line
    /// as a whole statement, with its return value neither assigned,
    /// returned, nor otherwise consumed, is flagged when the resolved
    /// function/method's own signature carries PHP 8.5's `#[\NoDiscard]`
    /// marker (`FunctionSig::no_discard`, Phase 37's own registry-side
    /// half). Called once per `StmtKind::Expression`, on the statement's
    /// own top-level expression only - a `NoDiscard` call anywhere else
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
    pub(crate) fn check_no_discard_statement(&mut self, e: &Expr, locals: &Locals, label: &str) {
        let (sig, callee_desc, identifier) = match &e.kind {
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return;
                };
                if is_language_construct(name) {
                    return;
                }
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
                // Real bug found and fixed this session: this used to
                // require `recv_ty` to be literally `Type::Object`,
                // silently skipping the check for a `Union`/
                // `Intersection` receiver.
                let Some((owner, sig)) = self.find_method_owner(&recv_ty, method) else {
                    return;
                };
                (
                    sig,
                    format!("{owner}::{method}()"),
                    "method.resultDiscarded",
                )
            }
            ExprKind::StaticCall(class_name, method, _) => {
                let Some(class) = self.resolve_static_class_name(class_name) else {
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
        if !sig.no_discard {
            return;
        }
        self.report_builder(
            DiagnosticBuilder::new(
                label.to_string(),
                format!("Call to {callee_desc} on a separate line discards its return value."),
                e.span,
                0,
                identifier,
            )
            .non_ignorable(),
        );
    }

    /// Phase 71: `RandomIntParametersRule`, narrowed to its plain-
    /// literal-arguments case (real PHPStan's own version also handles
    /// an `IntegerRangeType`-typed argument - this project has no such
    /// type, so a non-literal argument is silently skipped, the usual
    /// "don't guess" leniency). Confirmed directly via `php`:
    /// `random_int(10, 1)` throws a real, unconditional `ValueError` at
    /// call time ("Argument #1 ($min) must be less than or equal to
    /// argument #2 ($max)") whenever `$min` is greater than `$max` -
    /// `$min == $max` is fine (a degenerate but legal single-value
    /// range).
    pub(crate) fn check_random_int_call(&mut self, args: &[Arg], locals: &Locals, label: &str) {
        if args.len() < 2 {
            return;
        }
        let min_ty = self.infer_expr(&args[0].value, locals);
        let max_ty = self.infer_expr(&args[1].value, locals);
        if let (Type::IntLiteral(min), Type::IntLiteral(max)) = (min_ty, max_ty)
            && min > max
        {
            self.report(
                label.to_string(),
                format!(
                    "random_int(): parameter #1 ($min = {min}) must be less than or equal to parameter #2 ($max = {max})"
                ),
                args[0].value.span,
                0,
                "argument.type",
            );
        }
    }

    /// Phase 71: `DefineParametersRule` - `define()`'s own third
    /// `$case_insensitive` argument stopped doing anything as of PHP
    /// 8.0 (case-insensitive constants were removed entirely), confirmed
    /// directly via `php`: `define("FOO", 1, true)` on PHP 8.5 emits
    /// "Argument #3 ($case_insensitive) is ignored since declaration of
    /// case-insensitive constants is no longer supported" as a real
    /// runtime `E_WARNING` - the exact message text real PHPStan's own
    /// rule uses. Gated on `Registry::php_version` being configured *and*
    /// at least 8.0 (matching real PHPStan's own version-aware
    /// behavior): with no configured version, this project has nothing
    /// to check against, matching every other `PhpFeature`-gated rule's
    /// own "unset means unconfigured, stay silent" convention.
    pub(crate) fn check_define_third_argument(&mut self, args: &[Arg], span: Span, label: &str) {
        let Some(version) = self.registry.php_version else {
            return;
        };
        if crate::php_version::PhpFeature::CaseInsensitiveConstantsRemoved.is_available(version) {
            return;
        }
        if args.len() < 3 {
            return;
        }
        self.report(
            label.to_string(),
            "Argument #3 ($case_insensitive) is ignored since declaration of case-insensitive constants is no longer supported.".to_string(),
            span,
            0,
            "argument.unused",
        );
    }
}
