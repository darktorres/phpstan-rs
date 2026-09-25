//! The `@internal` restricted-usage engine, gated behind
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
//! class-constant read (both a constant's own individual `@internal` tag
//! and its declaring class's), instantiation (`new Foo()`), `instanceof`,
//! a `catch` clause's own caught type, `extends`/`implements`/trait-use,
//! a named function/method's own and a closure/arrow-fn literal's own
//! native param/return type hints, first-class-callables, the implicit
//! `__toString()` call an echo/print/string-cast/interpolation triggers,
//! and every PHPDoc-tag-typed position real
//! `RestrictedInternalClassNameUsageExtension` also covers (`@var`/
//! `@assert`/`@method`/`@mixin`/`@property`/`@phpstan-require-extends`/
//! `@phpstan-require-implements`/`@phpstan-sealed`/`@template` bound-and-
//! default) via `check_type_internal_usage`, reusing the `case_refs`
//! threading network. Not covered: attribute usage (real PHPStan's own
//! extension checks this too, but no case-tracking plumbing exists for
//! attribute-usage positions in this project yet).

use crate::checker::Checker;
use crate::registry::{ConstSig, FunctionSig, PropertySig};
use crate::types::Type;
use syntax::ast::TypeHint;
use syntax::ast::{ClassKind, FunctionDecl, Param};
use syntax::token::Span;

/// `Class`/`Interface`/`Trait`/`Enum` - matches real PHPStan's own
/// `ClassReflection::getClassTypeDescription()`, used both in message
/// text (lowercased) and as an identifier suffix (as-is).
pub(crate) fn class_type_word(kind: ClassKind) -> &'static str {
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
    pub(crate) fn should_report_internal_usage(&self, target_fqn: &str) -> bool {
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
        self.check_access_attributes(
            owner,
            method_name,
            sig.visibility == syntax::ast::Visibility::Public,
            span,
            label,
        );
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

    /// `RestrictedInternalClassConstantUsageExtension`: a constant's
    /// own individual `@internal` tag is tracked too, mirroring
    /// `check_internal_property_access`'s identical "own tag OR
    /// declaring class's tag" convention exactly.
    pub(crate) fn check_internal_class_constant_access(
        &mut self,
        owner: &str,
        const_name: &str,
        sig: &ConstSig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.check_internal_usage {
            return;
        }
        let Some(declaring_info) = self.registry.classes.get(owner) else {
            return;
        };
        let is_const_internal = sig.is_internal;
        if !is_const_internal && !declaring_info.is_internal {
            return;
        }
        if !self.should_report_internal_usage(owner) {
            return;
        }
        let type_word = class_type_word(declaring_info.kind);
        let root_ns = root_namespace_segment(owner);
        let (message, identifier) = match (root_ns, is_const_internal) {
            (None, false) => (
                format!(
                    "Access to constant {const_name} of internal {} {owner}.",
                    type_word.to_lowercase()
                ),
                format!("classConstant.internal{type_word}"),
            ),
            (None, true) => (
                format!("Access to internal constant {owner}::{const_name}."),
                "classConstant.internal".to_string(),
            ),
            (Some(ns), false) => (
                format!(
                    "Access to constant {const_name} of internal {} {owner} from outside its root namespace {ns}.",
                    type_word.to_lowercase()
                ),
                format!("classConstant.internal{type_word}"),
            ),
            (Some(ns), true) => (
                format!(
                    "Access to internal constant {owner}::{const_name} from outside its root namespace {ns}."
                ),
                "classConstant.internal".to_string(),
            ),
        };
        self.report(label.to_string(), message, span, 0, &identifier);
    }

    /// `RestrictedInternalClassNameUsageExtension`, narrowed to
    /// instantiation (`new Foo()`) - one of the several
    /// `ClassNameUsageLocation` variants that extension covers
    /// (`extends`/`implements`/param-and-return-type-hints/first-class-
    /// callables aren't checked here).
    pub(crate) fn check_internal_instantiation(&mut self, class: &str, span: Span, label: &str) {
        if let Some((canonical, _)) = self.registry.lookup_class(class) {
            let canonical = canonical.to_string();
            let is_public = self
                .registry
                .lookup_method(&canonical, "__construct")
                .is_none_or(|sig| sig.visibility == syntax::ast::Visibility::Public);
            self.check_access_attributes(&canonical, "__construct", is_public, span, label);
        }
        self.check_internal_class_name_usage(class, "Instantiation of", "new", span, label);
    }

    /// `RestrictedInternalClassNameUsageExtension`, narrowed to
    /// `instanceof`'s own target class.
    pub(crate) fn check_internal_instanceof(&mut self, class: &str, span: Span, label: &str) {
        self.check_internal_class_name_usage(
            class,
            "Instanceof references",
            "instanceof",
            span,
            label,
        );
    }

    /// `RestrictedInternalClassNameUsageExtension`, narrowed to a `catch`
    /// clause's own caught type - fires independently of, and in addition
    /// to, `catch.notThrowable` (a real Throwable-implementing class can
    /// still be `@internal`).
    pub(crate) fn check_internal_catch(&mut self, class: &str, span: Span, label: &str) {
        self.check_internal_class_name_usage(class, "Catching", "catch", span, label);
    }

    /// Shared by `check_internal_instantiation`/`check_internal_instanceof`/
    /// `check_internal_catch` - the three `ClassNameUsageLocation` variants
    /// with no member/receiver context of their own (unlike the method/
    /// property/constant checks above, a class-name-only reference has
    /// nothing to check beyond the class's own `is_internal` bit), so real
    /// PHPStan's own per-location message text differs only in its leading
    /// verb phrase and identifier prefix - both threaded in by the caller.
    /// `RestrictedInternalClassNameUsageExtension`'s
    /// own PHPDoc-tag-position coverage (`@var`/`@assert`/`@method`/
    /// `@mixin`/`@property`/`@phpstan-require-extends`/`@phpstan-require-
    /// implements`/`@phpstan-sealed`/`@template` bound-and-default) - real
    /// PHPStan checks `@internal` at every position a class name can
    /// appear, not just the native-syntax positions the rest of this file
    /// covers. Reuses the `case_refs` threading network
    /// (`collect_class_name_case_refs`, populated once at registry-build
    /// time for exactly these same positions for a *different* check,
    /// `class.nameCase`) instead of building new call-site plumbing -
    /// each `(raw, resolved)` pair's `resolved` half is already the
    /// fully-qualified name this check needs.
    pub(crate) fn check_type_internal_usage(
        &mut self,
        case_refs: &[(std::sync::Arc<str>, std::sync::Arc<str>)],
        label: &str,
        span: Span,
    ) {
        if !self.registry.check_internal_usage {
            return;
        }
        for (_, resolved) in case_refs {
            self.check_internal_class_name_usage(resolved, "Referencing", "type", span, label);
        }
    }

    fn check_internal_class_name_usage(
        &mut self,
        class: &str,
        verb_phrase: &str,
        identifier_prefix: &str,
        span: Span,
        label: &str,
    ) {
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
            "{verb_phrase} internal {} {class}.",
            type_word.to_lowercase()
        );
        self.report(
            label.to_string(),
            message,
            span,
            0,
            &format!("{identifier_prefix}.internal{type_word}"),
        );
    }

    /// `RestrictedInternalClassNameUsageExtension`'s
    /// `extends`/`implements`/trait-use half (`use SomeTrait;` is covered
    /// by the identical real-PHPStan mechanism -
    /// `ExistingClassInTraitUseRule` feeding the same shared
    /// `ClassNameCheck`). Mirrors `check_class_references_exist`'s own shape exactly: same
    /// already-resolved `ClassInfo::extends`/`implements`/`uses` fields,
    /// same whole-class-declaration span, same two call sites
    /// (`check_class_references_exist` doesn't run for a nested/
    /// conditionally-declared class either, so neither does this).
    pub(crate) fn check_internal_inheritance(&mut self, fqn: &str, kind: ClassKind, span: Span) {
        if !self.registry.check_internal_usage {
            return;
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let (extends, implements, uses) = (
            info.extends.clone(),
            info.implements.clone(),
            info.uses.clone(),
        );
        let subject = class_type_word(kind);
        let extends_prefix = if kind == ClassKind::Interface {
            "interface.extends"
        } else {
            "class.extends"
        };
        for parent in &extends {
            self.report_internal_extends_or_implements(
                fqn,
                subject,
                parent,
                "extends",
                extends_prefix,
                span,
            );
        }
        let implements_prefix = if kind == ClassKind::Enum {
            "enum.implements"
        } else {
            "class.implements"
        };
        for iface in &implements {
            self.report_internal_extends_or_implements(
                fqn,
                subject,
                iface,
                "implements",
                implements_prefix,
                span,
            );
        }
        for used_trait in &uses {
            self.report_internal_trait_use(fqn, used_trait, span);
        }
    }

    fn report_internal_extends_or_implements(
        &mut self,
        fqn: &str,
        subject: &str,
        target: &str,
        verb: &str,
        id_prefix: &str,
        span: Span,
    ) {
        let Some(info) = self.registry.classes.get(target) else {
            return;
        };
        if !info.is_internal || !self.should_report_internal_usage(target) {
            return;
        }
        let type_word = class_type_word(info.kind);
        self.report(
            fqn.to_string(),
            format!(
                "{subject} {fqn} {verb} internal {} {target}.",
                type_word.to_lowercase()
            ),
            span,
            0,
            &format!("{id_prefix}Internal{type_word}"),
        );
    }

    fn report_internal_trait_use(&mut self, fqn: &str, target: &str, span: Span) {
        let Some(info) = self.registry.classes.get(target) else {
            return;
        };
        if !info.is_internal || !self.should_report_internal_usage(target) {
            return;
        }
        let type_word = class_type_word(info.kind);
        self.report(
            fqn.to_string(),
            format!(
                "Usage of internal {} {target} in class {fqn}.",
                type_word.to_lowercase()
            ),
            span,
            0,
            &format!("traitUse.internal{type_word}"),
        );
    }

    /// `param`/`return` native type-hints for the
    /// `@internal` class-name-usage check - operates on each param's and
    /// the return's native type-hint text only (`resolve_type_hint`,
    /// not the docblock-merged type any `sig`-based lookup would give),
    /// matching real PHPStan's own `FunctionDefinitionCheck` exactly
    /// (there is no `ClassNameUsageLocation` for a `@param`/`@return`
    /// PHPDoc tag's own type at all). Named functions/methods only - see
    /// `check_internal_param_and_return_types_raw` for the shared core
    /// this and the closure/arrow-fn call sites both delegate to.
    pub(crate) fn check_internal_param_and_return_types(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        label: &str,
    ) {
        self.check_internal_param_and_return_types_raw(
            &f.params,
            f.return_type.as_ref().map(TypeHint::to_string).as_deref(),
            f.return_type.as_ref().map(|h| h.span).unwrap_or(f.span),
            class_name.is_some(),
            class_name,
            label,
        );
    }

    /// The real body of `check_internal_param_and_return_types` above,
    /// generalized to take a bare `params`/`return_type` pair instead of a
    /// whole `&FunctionDecl` - `ExprKind::Closure`/`ArrowFn` (`checker/
    /// expr.rs`) share this exact shape but aren't `FunctionDecl`s (no
    /// `FunctionDecl` wrapper exists for a closure literal at all), so
    /// this needs to run for them too: `function (InternalClass $f):
    /// InternalClass {}` must be flagged the same way the identical
    /// named-function signature is. `is_method` only picks the
    /// message's own "method"/"function" word - a closure/arrow-fn uses
    /// neither (real PHPStan has no separate rule for this shape at all
    /// to match wording against), so callers outside a class-scoped named
    /// declaration pass `false` and get "function" wording, matching this
    /// project's own existing closure-label convention elsewhere
    /// (`"{label} (closure)"`, already folded into `label` by the caller).
    pub(crate) fn check_internal_param_and_return_types_raw(
        &mut self,
        params: &[Param],
        return_type: Option<&str>,
        return_type_fallback_span: Span,
        is_method: bool,
        class_name: Option<&str>,
        label: &str,
    ) {
        if !self.registry.check_internal_usage {
            return;
        }
        let context = if is_method {
            format!("method {label}()")
        } else {
            format!("function {label}()")
        };
        for p in params {
            let ty =
                self.resolve_type_hint(p.type_hint.as_ref().map(TypeHint::to_string).as_deref());
            let ty = match class_name {
                Some(c) => self.resolve_pseudo(&ty, c),
                None => ty,
            };
            let mut classes = Vec::new();
            collect_referenced_class_names(&ty, &mut classes);
            for class in classes {
                self.report_internal_param_or_return(
                    &class,
                    &format!("Parameter ${}", p.name),
                    &context,
                    "parameter",
                    p.type_hint.as_ref().map(|h| h.span).unwrap_or(p.name_span),
                    label,
                );
            }
        }
        if return_type.is_some() {
            let ty = self.resolve_type_hint(return_type);
            let ty = match class_name {
                Some(c) => self.resolve_pseudo(&ty, c),
                None => ty,
            };
            let mut classes = Vec::new();
            collect_referenced_class_names(&ty, &mut classes);
            for class in classes {
                self.report_internal_param_or_return(
                    &class,
                    "Return type",
                    &context,
                    "return",
                    return_type_fallback_span,
                    label,
                );
            }
        }
    }

    fn report_internal_param_or_return(
        &mut self,
        class: &str,
        subject: &str,
        context: &str,
        id_prefix: &str,
        span: Span,
        label: &str,
    ) {
        let Some(info) = self.registry.classes.get(class) else {
            return;
        };
        if !info.is_internal || !self.should_report_internal_usage(class) {
            return;
        }
        let type_word = class_type_word(info.kind);
        self.report(
            label.to_string(),
            format!(
                "{subject} of {context} has typehint with internal {} {class}.",
                type_word.to_lowercase()
            ),
            span,
            0,
            &format!("{id_prefix}.internal{type_word}"),
        );
    }
}

/// Sibling of `checker::resolve::collect_unknown_class_names`, minus the
/// existence check: native PHP type-hint syntax can only ever produce
/// `Object`/`Union`/`Intersection` (no `Array`/`ClassString`/`ArrayShape`/
/// `ObjectShape` - those are PHPDoc-only constructs `resolve_type_hint`
/// never emits from a bare native hint string), so the walk only needs
/// those two arms.
pub(crate) fn collect_referenced_class_names(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Object(name, _) => {
            if !out.iter().any(|n| n == name) {
                out.push(name.clone().to_string());
            }
        }
        Type::Union(members) | Type::Intersection(members) => {
            for m in members {
                collect_referenced_class_names(m, out);
            }
        }
        _ => {}
    }
}
