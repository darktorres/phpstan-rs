//! Phase 39: `@deprecated` usage warnings, gated behind `Registry::
//! check_deprecated_usage` (default off - see its own doc comment).
//! Mirrors the highest-value shapes of real PHPStan's own separate
//! `phpstan/phpstan-deprecation-rules` package: a plain function call
//! (`function.deprecated`), an instance method call (`method.deprecated`),
//! a static method call (`staticMethod.deprecated`), an instance/static
//! property access (`property.deprecated`/`staticProperty.deprecated`), a
//! class-constant access (`classConstant.deprecated`), and instantiation
//! (`new.deprecated`).
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
//! comment. `check_deprecated_instantiation` is a separate, unrelated
//! question (is the *class itself* tagged), not an instance of that
//! unclaimed fallback.

use crate::checker::Checker;
use crate::registry::{ConstSig, FunctionSig, PropertySig};
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

    /// `property.deprecated`/`staticProperty.deprecated` - an instance or
    /// static property access.
    pub(crate) fn check_deprecated_property_access(
        &mut self,
        owner: &str,
        prop_name: &str,
        sig: &PropertySig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_deprecated_usage {
            return;
        }
        let Some(reason) = &sig.deprecated else {
            return;
        };
        let kind_word = if sig.is_static {
            "staticProperty"
        } else {
            "property"
        };
        let message = deprecation_message(&format!("property {owner}::${prop_name}"), reason);
        self.report(
            label.to_string(),
            message,
            span,
            0,
            &format!("{kind_word}.deprecated"),
        );
    }

    /// `classConstant.deprecated` - a class-constant access.
    pub(crate) fn check_deprecated_class_constant_access(
        &mut self,
        owner: &str,
        const_name: &str,
        sig: &ConstSig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_deprecated_usage {
            return;
        }
        let Some(reason) = &sig.deprecated else {
            return;
        };
        let message = deprecation_message(&format!("constant {owner}::{const_name}"), reason);
        self.report(
            label.to_string(),
            message,
            span,
            0,
            "classConstant.deprecated",
        );
    }

    /// `new.deprecated` - instantiation (`new Foo()`) of a class whose own
    /// docblock carries `@deprecated`. Independent of whether any of its
    /// members are individually tagged too.
    pub(crate) fn check_deprecated_instantiation(&mut self, class: &str, span: Span, label: &str) {
        if !self.registry.check_deprecated_usage {
            return;
        }
        let Some(info) = self.registry.classes.get(class) else {
            return;
        };
        let Some(reason) = &info.deprecated else {
            return;
        };
        let message = if reason.is_empty() {
            format!("Instantiation of deprecated class {class}.")
        } else {
            format!("Instantiation of deprecated class {class} ({reason}).")
        };
        self.report(label.to_string(), message, span, 0, "new.deprecated");
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
