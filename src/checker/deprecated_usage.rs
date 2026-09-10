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
//! Phase 39.1/39.2 (grounded directly against the real, unvendored
//! `phpstan/phpstan-deprecation-rules` package's own
//! `Rules/Deprecations/*.php` sources, not guessed): a class-level
//! `@deprecated` DOES cascade to every member usage - checked first, as
//! its own separate branch, independently of whether the member itself
//! also carries `@deprecated` (real PHPStan never stacks both
//! diagnostics for the same usage). And unlike `checker::internal_usage`,
//! usage *from within* an already-deprecated function/method/class/trait
//! scope is deliberately suppressed entirely (`Checker::
//! current_scope_deprecated`) - real PHPStan's own `DeprecatedScopeHelper`
//! does the same for every rule in this family, on the reasoning that one
//! deprecated thing using another isn't worth a separate warning.
//! `check_deprecated_instantiation` needs no separate cascade branch -
//! instantiation *is* the class-level check already.

use crate::checker::{Checker, class_type_word};
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
        if !self.registry.check_deprecated_usage || self.current_scope_deprecated {
            return;
        }
        let Some(reason) = &sig.deprecated else {
            return;
        };
        let message = deprecation_message(&format!("function {fqn}()"), reason);
        self.report(label.to_string(), message, span, 0, "function.deprecated");
    }

    /// `method.deprecated`/`staticMethod.deprecated` - an instance or
    /// static method call. Phase 39.1: the *declaring class*'s own
    /// `@deprecated` cascades to every member usage, checked first and
    /// independently of the method's own tag - matches real PHPStan's
    /// own `RestrictedDeprecatedMethodUsageExtension`, which checks
    /// `getDeclaringClass()->isDeprecated()` before the member's own tag
    /// and never stacks both diagnostics. Phase 39.5: `__toString`'s own
    /// tag gets a dedicated string-cast message/identifier instead of the
    /// generic one, but only in the own-tag branch (after the cascade
    /// branch has already had its chance to return) - matches real
    /// PHPStan's own `RestrictedDeprecatedMethodUsageExtension` special-
    /// casing order exactly.
    pub(crate) fn check_deprecated_method_call(
        &mut self,
        owner: &str,
        method_name: &str,
        sig: &FunctionSig,
        is_static: bool,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_deprecated_usage || self.current_scope_deprecated {
            return;
        }
        let kind_word = if is_static { "staticMethod" } else { "method" };
        if let Some(declaring_info) = self.registry.classes.get(owner)
            && let Some(reason) = &declaring_info.deprecated
        {
            let type_word = class_type_word(declaring_info.kind);
            let static_word = if is_static { "static " } else { "" };
            let message = cascade_deprecation_message(
                &format!(
                    "Call to {static_word}method {method_name}() of deprecated {} {owner}.",
                    type_word.to_lowercase()
                ),
                reason,
            );
            self.report(
                label.to_string(),
                message,
                span,
                0,
                &format!("{kind_word}.deprecated{type_word}"),
            );
            return;
        }
        let Some(reason) = &sig.deprecated else {
            return;
        };
        if method_name.eq_ignore_ascii_case("__toString") {
            let message = if reason.is_empty() {
                format!("Casting class {owner} to string is deprecated.")
            } else {
                format!("Casting class {owner} to string is deprecated:\n{reason}")
            };
            self.report(
                label.to_string(),
                message,
                span,
                0,
                "class.toStringDeprecated",
            );
            return;
        }
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
    /// static property access. Phase 39.1: same declaring-class cascade
    /// as `check_deprecated_method_call` above.
    pub(crate) fn check_deprecated_property_access(
        &mut self,
        owner: &str,
        prop_name: &str,
        sig: &PropertySig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_deprecated_usage || self.current_scope_deprecated {
            return;
        }
        let kind_word = if sig.is_static {
            "staticProperty"
        } else {
            "property"
        };
        if let Some(declaring_info) = self.registry.classes.get(owner)
            && let Some(reason) = &declaring_info.deprecated
        {
            let type_word = class_type_word(declaring_info.kind);
            let static_word = if sig.is_static { "static " } else { "" };
            let message = cascade_deprecation_message(
                &format!(
                    "Access to {static_word}property ${prop_name} of deprecated {} {owner}.",
                    type_word.to_lowercase()
                ),
                reason,
            );
            self.report(
                label.to_string(),
                message,
                span,
                0,
                &format!("{kind_word}.deprecated{type_word}"),
            );
            return;
        }
        let Some(reason) = &sig.deprecated else {
            return;
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

    /// `classConstant.deprecated` - a class-constant access. Phase 39.1:
    /// same declaring-class cascade as the two functions above.
    pub(crate) fn check_deprecated_class_constant_access(
        &mut self,
        owner: &str,
        const_name: &str,
        sig: &ConstSig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_deprecated_usage || self.current_scope_deprecated {
            return;
        }
        if let Some(declaring_info) = self.registry.classes.get(owner)
            && let Some(reason) = &declaring_info.deprecated
        {
            let type_word = class_type_word(declaring_info.kind);
            let message = cascade_deprecation_message(
                &format!(
                    "Fetching class constant {const_name} of deprecated {} {owner}.",
                    type_word.to_lowercase()
                ),
                reason,
            );
            self.report(
                label.to_string(),
                message,
                span,
                0,
                &format!("classConstant.deprecated{type_word}"),
            );
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
        if !self.registry.check_deprecated_usage || self.current_scope_deprecated {
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

/// Phase 39.1: the declaring-class cascade's own message shape - `sentence`
/// is already a complete, period-terminated sentence (e.g. `"Call to
/// method foo() of deprecated class Bar."`), matching real PHPStan's own
/// `RestrictedDeprecated*UsageExtension` messages, which append the
/// class's own deprecation description as a *new* line (`:\n{description}`)
/// rather than a parenthetical - a different shape from `deprecation_message`'s
/// own `(reason)` suffix, which is for a *member's own* tag, not a cascade.
fn cascade_deprecation_message(sentence: &str, reason: &str) -> String {
    if reason.is_empty() {
        sentence.to_string()
    } else {
        format!("{sentence}\n{reason}")
    }
}
