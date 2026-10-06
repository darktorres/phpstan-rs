//! `@deprecated` usage warnings, reported while the deprecation-rules
//! extension is active (`DeprecationRulesSettings::enabled`).
//! Mirrors the highest-value shapes of real PHPStan's own separate
//! `phpstan/phpstan-deprecation-rules` package: a plain function call
//! (`function.deprecated`), an instance method call (`method.deprecated`),
//! a static method call (`staticMethod.deprecated`), an instance/static
//! property access (`property.deprecated`/`staticProperty.deprecated`), a
//! class-constant access (`classConstant.deprecated`), and instantiation
//! (`new.deprecatedClass`).
//!
//! A class-level `@deprecated` cascades to every member usage - checked first, as
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
use crate::registry::{ConstantReflection, FunctionReflection, PropertyReflection};
use syntax::span::Span;

impl<'a> Checker<'a> {
    /// `function.deprecated` - a plain function call.
    pub(crate) fn check_deprecated_function_call(
        &mut self,
        fqn: &str,
        sig: &FunctionReflection,
        span: Span,
    ) {
        if !self.settings.deprecation_rules || self.current_scope_deprecated {
            return;
        }
        let Some(reason) = sig.deprecated() else {
            return;
        };
        let message = with_reason(&format!("Call to deprecated function {fqn}()."), reason);
        self.report(message, span, 0, "function.deprecated");
    }

    /// `method.deprecated`/`staticMethod.deprecated` - an instance or
    /// static method call. The declaring class's own
    /// `@deprecated` cascades to every member usage, checked first and
    /// independently of the method's own tag - matches real PHPStan's
    /// own `RestrictedDeprecatedMethodUsageExtension`, which checks
    /// `getDeclaringClass()->isDeprecated()` before the member's own tag
    /// and never stacks both diagnostics. `__toString`'s own
    /// tag gets a dedicated string-cast message/identifier instead of the
    /// generic one, but only in the own-tag branch (after the cascade
    /// branch has already had its chance to return) - matches real
    /// PHPStan's own `RestrictedDeprecatedMethodUsageExtension` special-
    /// casing order exactly.
    pub(crate) fn check_deprecated_method_call(
        &mut self,
        owner: &str,
        method_name: &str,
        sig: &FunctionReflection,
        is_static: bool,
        span: Span,
    ) {
        if !self.settings.deprecation_rules || self.current_scope_deprecated {
            return;
        }
        let kind_word = if is_static { "staticMethod" } else { "method" };
        if let Some(declaring_info) = self.registry.classes().get(owner)
            && let Some(reason) = declaring_info.deprecated()
        {
            let type_word = class_type_word(declaring_info.kind());
            let message = with_reason(
                &format!(
                    "Call to method {method_name}() of deprecated {} {owner}.",
                    type_word.to_lowercase()
                ),
                reason,
            );
            self.report(
                message,
                span,
                0,
                format!("{kind_word}.deprecated{type_word}"),
            );
            return;
        }
        let Some(reason) = sig.deprecated() else {
            return;
        };
        if method_name.eq_ignore_ascii_case("__toString") {
            // Upstream's own wording keeps the full stop before the colon.
            let message = if reason.is_empty() {
                format!("Casting class {owner} to string is deprecated.")
            } else {
                format!("Casting class {owner} to string is deprecated.:\n{reason}")
            };
            self.report(message, span, 0, "class.toStringDeprecated");
            return;
        }
        let message = with_reason(
            &format!(
                "Call to deprecated method {method_name}() of {} {owner}.",
                self.class_type_word_of(owner)
            ),
            reason,
        );
        self.report(message, span, 0, format!("{kind_word}.deprecated"));
    }

    /// `property.deprecated`/`staticProperty.deprecated` - an instance or
    /// static property access. Same declaring-class cascade
    /// as `check_deprecated_method_call` above.
    pub(crate) fn check_deprecated_property_access(
        &mut self,
        owner: &str,
        prop_name: &str,
        sig: &PropertyReflection,
        span: Span,
    ) {
        if !self.settings.deprecation_rules || self.current_scope_deprecated {
            return;
        }
        let kind_word = if sig.is_static() {
            "staticProperty"
        } else {
            "property"
        };
        if let Some(declaring_info) = self.registry.classes().get(owner)
            && let Some(reason) = declaring_info.deprecated()
        {
            let type_word = class_type_word(declaring_info.kind());
            let static_word = if sig.is_static() { "static " } else { "" };
            let message = with_reason(
                &format!(
                    "Access to {static_word}property ${prop_name} of deprecated {} {owner}.",
                    type_word.to_lowercase()
                ),
                reason,
            );
            self.report(
                message,
                span,
                0,
                format!("{kind_word}.deprecated{type_word}"),
            );
            return;
        }
        let Some(reason) = sig.deprecated() else {
            return;
        };
        let static_word = if sig.is_static() { "static " } else { "" };
        let message = with_reason(
            &format!(
                "Access to deprecated {static_word}property ${prop_name} of {} {owner}.",
                self.class_type_word_of(owner)
            ),
            reason,
        );
        self.report(message, span, 0, format!("{kind_word}.deprecated"));
    }

    /// `classConstant.deprecated` - a class-constant access. Same
    /// declaring-class cascade as the two functions above.
    pub(crate) fn check_deprecated_class_constant_access(
        &mut self,
        owner: &str,
        const_name: &str,
        sig: &ConstantReflection,
        span: Span,
    ) {
        if !self.settings.deprecation_rules || self.current_scope_deprecated {
            return;
        }
        if let Some(declaring_info) = self.registry.classes().get(owner)
            && let Some(reason) = declaring_info.deprecated()
        {
            let type_word = class_type_word(declaring_info.kind());
            let message = with_reason(
                &format!(
                    "Fetching class constant {const_name} of deprecated {} {owner}.",
                    type_word.to_lowercase()
                ),
                reason,
            );
            self.report(
                message,
                span,
                0,
                format!("classConstant.deprecated{type_word}"),
            );
            return;
        }
        let Some(reason) = &sig.deprecated() else {
            return;
        };
        let message = with_reason(
            &format!(
                "Fetching deprecated class constant {const_name} of {} {owner}.",
                self.class_type_word_of(owner)
            ),
            reason,
        );
        self.report(message, span, 0, "classConstant.deprecated");
    }

    /// `new.deprecatedClass` - instantiation (`new Foo()`) of a class whose
    /// own docblock carries `@deprecated`, worded as phpstan-deprecation-
    /// rules' `RestrictedDeprecatedClassNameUsageExtension` does.
    /// Independent of whether any of its members are individually tagged
    /// too.
    pub(crate) fn check_deprecated_instantiation(&mut self, class: &str, span: Span) {
        if !self.settings.deprecation_rules || self.current_scope_deprecated {
            return;
        }
        let Some(info) = self.registry.classes().get(class) else {
            return;
        };
        let Some(reason) = info.deprecated() else {
            return;
        };
        let message = with_reason(
            &format!("Instantiation of deprecated class {class}."),
            reason,
        );
        self.report(message, span, 0, "new.deprecatedClass");
    }
}

/// A deprecation message as phpstan-deprecation-rules words it: the
/// sentence alone, or, when the `@deprecated` tag gives a reason, the
/// sentence without its full stop, a colon and the reason on a new line.
fn with_reason(sentence: &str, reason: &str) -> String {
    if reason.is_empty() {
        sentence.to_string()
    } else {
        format!("{}:\n{reason}", sentence.trim_end_matches('.'))
    }
}

impl Checker<'_> {
    /// `ClassReflection::getClassTypeDescription()` lowercased: `class`,
    /// `interface`, `trait` or `enum`.
    fn class_type_word_of(&self, class: &str) -> String {
        self.registry
            .classes()
            .get(class)
            .map_or("class", |info| class_type_word(info.kind()))
            .to_lowercase()
    }
}
