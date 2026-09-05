//! Phase 39: `@deprecated` usage warnings, gated behind `Registry::
//! check_deprecated_usage` (default off - see its own doc comment).
//! Mirrors the highest-value shapes of real PHPStan's own separate
//! `phpstan/phpstan-deprecation-rules` package: a plain function call
//! (`function.deprecated`), an instance method call (`method.deprecated`),
//! and a static method call (`staticMethod.deprecated`).
//!
//! Unlike `checker::internal_usage`, there's no "declaring class also
//! counts" fallback and no namespace-based allowance - `@deprecated` isn't
//! about *where* the caller is, only *whether* the callee was tagged, so
//! every real usage from anywhere (including the declaring class's own
//! body) is reported. This project could not confirm against real
//! PHPStan's own source whether a class-level `@deprecated` implicitly
//! deprecates every member too (the actual detection logic lives in
//! `ondrejmirtes/better-reflection`, not vendored here) - left unclaimed
//! rather than guessed, matching `FunctionSig::deprecated`'s own doc
//! comment.

use crate::checker::Checker;
use crate::registry::FunctionSig;
use crate::token::Span;

impl<'a> Checker<'a> {
    /// `function.deprecated` - a plain function call.
    pub(crate) fn check_deprecated_function_call(
        &mut self,
        fqn: &str,
        sig: &FunctionSig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_deprecated_usage {
            return;
        }
        let Some(reason) = &sig.deprecated else {
            return;
        };
        let message = deprecation_message(&format!("function {fqn}()"), reason);
        self.report(label.to_string(), message, span, 0, "function.deprecated");
    }

    /// `method.deprecated`/`staticMethod.deprecated` - an instance or
    /// static method call.
    pub(crate) fn check_deprecated_method_call(
        &mut self,
        owner: &str,
        method_name: &str,
        sig: &FunctionSig,
        is_static: bool,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_deprecated_usage {
            return;
        }
        let Some(reason) = &sig.deprecated else {
            return;
        };
        let kind_word = if is_static { "staticMethod" } else { "method" };
        let message = deprecation_message(&format!("method {owner}::{method_name}()"), reason);
        self.report(
            label.to_string(),
            message,
            span,
            0,
            &format!("{kind_word}.deprecated"),
        );
    }
}

/// `subject` is already `"function foo()"`/`"method Foo::bar()"` -
/// `reason` is `FunctionSig::deprecated`'s own `Some(String)` payload,
/// possibly empty (a bare `@deprecated` with no reason given).
fn deprecation_message(subject: &str, reason: &str) -> String {
    if reason.is_empty() {
        format!("Call to deprecated {subject}.")
    } else {
        format!("Call to deprecated {subject} ({reason}).")
    }
}
