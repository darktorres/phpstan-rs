use crate::checker::{Checker, Locals};
use crate::registry::FunctionReflection;
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
        // The callee's description is formatted only for a call that is
        // reported: the attribute is tested first, on the call statements
        // that mostly have none.
        match &e.kind {
            ExprKind::Call(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return;
                };
                let Some((resolved_name, sig)) = self.registry.resolve_function(name) else {
                    return;
                };
                if self.discards_result_flagged(sig) {
                    self.report_discarded(
                        format!("{resolved_name}()"),
                        e.span,
                        "function.resultDiscarded",
                    );
                }
            }
            ExprKind::MethodCall(recv, method, _, _) => {
                // The callee the call's own check resolved, when it did.
                let resolved = self
                    .statement_method_call
                    .as_ref()
                    .filter(|call| call.id == e.id)
                    .map(|call| call.owner.clone())
                    .and_then(|owner| {
                        let (_, sig) = self.lookup_method_with_owner(&owner, method)?;
                        Some((owner, sig))
                    });
                let Some((owner, sig)) = resolved.or_else(|| {
                    let recv_ty = self.infer_expr(recv, locals).without(&Type::null());
                    self.find_method_owner(&recv_ty, method)
                }) else {
                    return;
                };
                if self.discards_result_flagged(sig) {
                    self.report_discarded(
                        format!("{owner}::{method}()"),
                        e.span,
                        "method.resultDiscarded",
                    );
                }
            }
            ExprKind::StaticCall(target, method, _) => {
                let Some(class) = target
                    .as_name()
                    .and_then(|name| self.resolve_static_class_name(name))
                else {
                    return;
                };
                let Some(sig) = self.registry.lookup_method(&class, method) else {
                    return;
                };
                if self.discards_result_flagged(sig) {
                    self.report_discarded(
                        format!("{class}::{method}()"),
                        e.span,
                        "staticMethod.resultDiscarded",
                    );
                }
            }
            _ => {}
        }
    }

    /// Whether `sig` marks its result as one not to discard.
    fn discards_result_flagged(&self, sig: &FunctionReflection) -> bool {
        sig.no_discard() || (sig.must_use_result() && self.settings.language_extensions)
    }

    fn report_discarded(
        &mut self,
        callee: String,
        span: syntax::token::Span,
        identifier: &'static str,
    ) {
        self.report_builder(
            DiagnosticBuilder::new(
                format!("Call to {callee} on a separate line discards its return value."),
                span,
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
        if let (Some(min), Some(max)) = (min_ty.int_literal_value(), max_ty.int_literal_value())
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

    /// Builtins that turn values into keys, called as PHP rejects:
    /// `array_combine` with arrays whose sizes surely differ (a
    /// `ValueError` on PHP 8, `false` before), and `array_flip` /
    /// `array_count_values` over values that aren't ints or strings, each
    /// skipped with a warning.
    pub(crate) fn check_value_to_key_call(&mut self, name: &str, args: &[Arg], locals: &Locals) {
        if args.iter().any(|a| a.spread || a.name.is_some()) {
            return;
        }
        match (name, args) {
            ("array_combine", [keys, values, ..]) => {
                let keys_ty = self.infer_expr(&keys.value, locals);
                let values_ty = self.infer_expr(&values.value, locals);
                if let (Some((kl, kh)), Some((vl, vh))) =
                    (keys_ty.count_range(), values_ty.count_range())
                    && (kh.is_some_and(|h| h < vl) || vh.is_some_and(|h| h < kl))
                {
                    self.report(
                        format!(
                            "array_combine() expects arrays of the same size, {keys_ty} and {values_ty} given."
                        ),
                        values.value.span,
                        3,
                        "function.arraySizeMismatch",
                    );
                }
            }
            ("array_flip" | "array_count_values", [array, ..]) => {
                let Some(values) = self.infer_expr(&array.value, locals).array_value_type() else {
                    return;
                };
                let keyable = self.relation().union(Type::int(), Type::string());
                if let Some(level) = self.failing_level(&values, 3, |m| {
                    self.relation().is_super_type_of(&keyable, m).no()
                }) {
                    let verb = if name == "array_flip" {
                        "flip"
                    } else {
                        "count"
                    };
                    self.report(
                        format!(
                            "{name}() can only {verb} int and string values, {values} given; others are skipped."
                        ),
                        array.value.span,
                        level,
                        "function.unkeyableValue",
                    );
                }
            }
            _ => {}
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
