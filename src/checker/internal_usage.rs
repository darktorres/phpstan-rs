//! Phase 67: the `@internal` restricted-usage engine, gated behind
//! `Registry::check_internal_usage` (default off - see its own doc
//! comment). Mirrors real PHPStan's own `RestrictedInternal*UsageExtension`
//! family (`vendor/phpstan-src/src/Rules/InternalTag/`): a symbol tagged
//! `@internal` (or declared inside an `@internal`-tagged class) is usable
//! freely from anywhere sharing its own top-level ("root") namespace
//! segment - the common Composer-package-as-top-level-namespace
//! convention - and restricted everywhere else. One declared with no
//! namespace at all is usable only from within its own exact declaring
//! class.
//!
//! Covers the highest-value access shapes: a plain function call, an
//! instance/static method call, an instance/static property read, a
//! class-constant read (declaring-class-internal only - no per-constant
//! `@internal` tracking exists yet, see `ClassInfo::is_internal`'s own
//! call sites), and instantiation (`new Foo()`). Real PHPStan's own
//! `RestrictedInternalClassNameUsageExtension` additionally covers
//! `extends`/`implements`/param-and-return-type-hints/`catch`/
//! `instanceof`, and `RestrictedUsageOfDeprecatedStringCastRule` extends
//! the method-call check to an implicit `(string)`-cast-triggered
//! `__toString()` call - neither is implemented here (see ROADMAP.md's
//! own Phase 67 entry for the full list of what's still open).

use crate::ast::ClassKind;
use crate::checker::Checker;
use crate::registry::{FunctionSig, PropertySig};
use crate::token::Span;

/// `Class`/`Interface`/`Trait`/`Enum` - matches real PHPStan's own
/// `ClassReflection::getClassTypeDescription()`, used both in message
/// text (lowercased) and as an identifier suffix (as-is).
fn class_type_word(kind: ClassKind) -> &'static str {
    match kind {
        ClassKind::Class => "Class",
        ClassKind::Interface => "Interface",
        ClassKind::Trait => "Trait",
        ClassKind::Enum => "Enum",
    }
}

/// The first namespace segment before `fqn`'s own last `\`-separated
/// component (its bare name) - `None` when `fqn` has no namespace at
/// all. Mirrors real PHPStan's own `RestrictedInternalUsageHelper::
/// getRootNamespaceFromName` exactly (used for both a class's and a
/// function's own name).
fn root_namespace_segment(fqn: &str) -> Option<&str> {
    let last_sep = fqn.rfind('\\')?;
    fqn[..last_sep].split('\\').next()
}

impl<'a> Checker<'a> {
    /// Mirrors real PHPStan's own `RestrictedInternalUsageHelper::
    /// shouldBeReported` - `target_fqn` is whichever symbol's own name
    /// governs the namespace comparison (the accessed class/function
    /// itself, or the declaring class for a member access).
    fn should_report_internal_usage(&self, target_fqn: &str) -> bool {
        let current_ns = self.name_ctx.namespace();
        if current_ns.is_empty() {
            // No enclosing namespace at all: real PHP has no notion of
            // "this file's own package" to compare against, so real
            // PHPStan falls back to the narrowest possible allowance -
            // usable only from within the exact same class, and not at
            // all outside of any class.
            return match &self.current_class {
                None => true,
                Some(c) => c != target_fqn,
            };
        }
        let current_root = current_ns.split('\\').next().unwrap_or("");
        match root_namespace_segment(target_fqn) {
            None => true,
            Some(root) => root != current_root,
        }
    }

    /// `RestrictedInternalFunctionUsageExtension` - `function.internal`.
    pub(crate) fn check_internal_function_call(
        &mut self,
        fqn: &str,
        sig: &FunctionSig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_internal_usage || !sig.is_internal {
            return;
        }
        if !self.should_report_internal_usage(fqn) {
            return;
        }
        let message = match root_namespace_segment(fqn) {
            None => format!("Call to internal function {fqn}()."),
            Some(ns) => {
                format!("Call to internal function {fqn}() from outside its root namespace {ns}.")
            }
        };
        self.report(label.to_string(), message, span, 0, "function.internal");
    }

    /// `RestrictedInternalMethodUsageExtension` -
    /// `method.internal`/`staticMethod.internal`/`method.internal<Kind>`/
    /// `staticMethod.internal<Kind>`, the same four-branch shape real
    /// PHPStan's own extension has (declaring-class-internal vs
    /// method-itself-internal, crossed with namespaced vs global-
    /// namespace declaring class).
    pub(crate) fn check_internal_method_call(
        &mut self,
        owner: &str,
        method_name: &str,
        sig: &FunctionSig,
        is_static: bool,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_internal_usage {
            return;
        }
        let Some(declaring_info) = self.registry.classes.get(owner) else {
            return;
        };
        let is_method_internal = sig.is_internal;
        if !is_method_internal && !declaring_info.is_internal {
            return;
        }
        if !self.should_report_internal_usage(owner) {
            return;
        }
        let static_word = if is_static { "static " } else { "" };
        let kind_word = if is_static { "staticMethod" } else { "method" };
        let type_word = class_type_word(declaring_info.kind);
        let root_ns = root_namespace_segment(owner);
        let (message, identifier) = match (root_ns, is_method_internal) {
            (None, false) => (
                format!(
                    "Call to {static_word}method {method_name}() of internal {} {owner}.",
                    type_word.to_lowercase()
                ),
                format!("{kind_word}.internal{type_word}"),
            ),
            (None, true) => (
                format!("Call to internal {static_word}method {owner}::{method_name}()."),
                format!("{kind_word}.internal"),
            ),
            (Some(ns), false) => (
                format!(
                    "Call to {static_word}method {method_name}() of internal {} {owner} from outside its root namespace {ns}.",
                    type_word.to_lowercase()
                ),
                format!("{kind_word}.internal{type_word}"),
            ),
            (Some(ns), true) => (
                format!(
                    "Call to internal {static_word}method {owner}::{method_name}() from outside its root namespace {ns}."
                ),
                format!("{kind_word}.internal"),
            ),
        };
        self.report(label.to_string(), message, span, 0, &identifier);
    }

    /// `RestrictedInternalPropertyUsageExtension` - same four-branch
    /// shape as `check_internal_method_call` just above, "Access to"
    /// instead of "Call to", `$name` instead of `name()`.
    pub(crate) fn check_internal_property_access(
        &mut self,
        owner: &str,
        prop_name: &str,
        sig: &PropertySig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_internal_usage {
            return;
        }
        let Some(declaring_info) = self.registry.classes.get(owner) else {
            return;
        };
        let is_prop_internal = sig.is_internal;
        if !is_prop_internal && !declaring_info.is_internal {
            return;
        }
        if !self.should_report_internal_usage(owner) {
            return;
        }
        let static_word = if sig.is_static { "static " } else { "" };
        let kind_word = if sig.is_static {
            "staticProperty"
        } else {
            "property"
        };
        let type_word = class_type_word(declaring_info.kind);
        let root_ns = root_namespace_segment(owner);
        let (message, identifier) = match (root_ns, is_prop_internal) {
            (None, false) => (
                format!(
                    "Access to {static_word}property ${prop_name} of internal {} {owner}.",
                    type_word.to_lowercase()
                ),
                format!("{kind_word}.internal{type_word}"),
            ),
            (None, true) => (
                format!("Access to internal {static_word}property {owner}::${prop_name}."),
                format!("{kind_word}.internal"),
            ),
            (Some(ns), false) => (
                format!(
                    "Access to {static_word}property ${prop_name} of internal {} {owner} from outside its root namespace {ns}.",
                    type_word.to_lowercase()
                ),
                format!("{kind_word}.internal{type_word}"),
            ),
            (Some(ns), true) => (
                format!(
                    "Access to internal {static_word}property {owner}::${prop_name} from outside its root namespace {ns}."
                ),
                format!("{kind_word}.internal"),
            ),
        };
        self.report(label.to_string(), message, span, 0, &identifier);
    }

    /// `RestrictedInternalClassConstantUsageExtension`, narrowed to its
    /// declaring-class-internal half only - see this module's own doc
    /// comment for why a constant's own individual `@internal` tag isn't
    /// tracked.
    pub(crate) fn check_internal_class_constant_access(
        &mut self,
        owner: &str,
        const_name: &str,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_internal_usage {
            return;
        }
        let Some(declaring_info) = self.registry.classes.get(owner) else {
            return;
        };
        if !declaring_info.is_internal {
            return;
        }
        if !self.should_report_internal_usage(owner) {
            return;
        }
        let type_word = class_type_word(declaring_info.kind);
        let message = match root_namespace_segment(owner) {
            None => format!(
                "Access to constant {const_name} of internal {} {owner}.",
                type_word.to_lowercase()
            ),
            Some(ns) => format!(
                "Access to constant {const_name} of internal {} {owner} from outside its root namespace {ns}.",
                type_word.to_lowercase()
            ),
        };
        self.report(
            label.to_string(),
            message,
            span,
            0,
            &format!("classConstant.internal{type_word}"),
        );
    }

    /// `RestrictedInternalClassNameUsageExtension`, narrowed to
    /// instantiation (`new Foo()`) only - the single highest-value shape
    /// of the several `ClassNameUsageLocation` variants that extension
    /// covers (`extends`/`implements`/param-and-return-type-hints/
    /// `catch`/`instanceof` aren't checked here - see ROADMAP.md).
    pub(crate) fn check_internal_instantiation(&mut self, class: &str, span: Span, label: &str) {
        if !self.registry.check_internal_usage {
            return;
        }
        let Some(info) = self.registry.classes.get(class) else {
            return;
        };
        if !info.is_internal {
            return;
        }
        if !self.should_report_internal_usage(class) {
            return;
        }
        let type_word = class_type_word(info.kind);
        let message = format!(
            "Instantiation of internal {} {class}.",
            type_word.to_lowercase()
        );
        self.report(
            label.to_string(),
            message,
            span,
            0,
            &format!("new.internal{type_word}"),
        );
    }
}
