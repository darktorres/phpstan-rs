//! `textDocument/semanticTokens/full`'s own accumulator - flags every
//! `@deprecated`/`@internal` usage site independent of whether this
//! project's own `check_deprecated_usage` gate/internal-usage extension
//! is even enabled, so a client gets push-highlighting regardless of
//! diagnostic configuration.
//!
//! Each `record_*_usage_token` entry point deliberately re-derives the
//! exact same gating the corresponding `check_internal_*`/
//! `check_deprecated_*` function applies - `should_report_internal_usage`
//! (the namespace-root comparison that makes an in-package `@internal`
//! access legal and silent) and `current_scope_deprecated` (deprecated-
//! calling-deprecated inside an already-legacy scope isn't worth its own
//! warning) - rather than firing off the raw `is_internal`/
//! `deprecated.is_some()` booleans alone, which would light up every
//! ordinary, sanctioned in-package/in-scope access identically to a
//! genuine cross-package/live-code violation.
//!
//! `record_usage_semantic_token` itself stays private: the three
//! `record_*_usage_token` wrappers are the only entry points, so a
//! future call site can't reintroduce that bug by hand-rolling its own
//! boolean computation and calling the raw primitive directly. Do not
//! widen its visibility.

use crate::checker::Checker;
use crate::registry::FunctionSig;
use crate::token::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageTokenKind {
    Function,
    Method,
    Property,
    ClassConstant,
    Class,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct UsageSemanticToken {
    pub span: Span,
    pub kind: UsageTokenKind,
    pub deprecated: bool,
    pub internal: bool,
}

impl<'a> Checker<'a> {
    /// Single choke point: drops (does not truncate) any span crossing a
    /// line boundary - LSP's semantic-tokens wire format requires every
    /// token to sit on one line, but a member access's own `e.span`
    /// covers the whole access/call expression, which can span a
    /// multi-line argument list or fluent receiver.
    fn record_usage_semantic_token(
        &mut self,
        kind: UsageTokenKind,
        span: Span,
        deprecated: bool,
        internal: bool,
    ) {
        if (!deprecated && !internal) || span.start.line != span.end.line {
            return;
        }
        self.semantic_tokens.push(UsageSemanticToken {
            span,
            kind,
            deprecated,
            internal,
        });
    }

    /// Plain function call / its first-class-callable twin. Reuses
    /// `should_report_internal_usage(fqn)` - the exact same namespace-
    /// root gate `check_internal_function_call` applies - so an
    /// in-package call to your own `@internal` function never lights up.
    pub(crate) fn record_function_usage_token(&mut self, fqn: &str, sig: &FunctionSig, span: Span) {
        let deprecated = !self.current_scope_deprecated && sig.deprecated.is_some();
        let internal = sig.is_internal && self.should_report_internal_usage(fqn);
        self.record_usage_semantic_token(UsageTokenKind::Function, span, deprecated, internal);
    }

    /// method/staticMethod/property/staticProperty/classConstant shapes.
    /// Deprecated: own tag OR declaring class's cascade tag (matches
    /// `check_deprecated_method_call`/etc.'s cascade-first shape), gated
    /// on `current_scope_deprecated` exactly like those functions.
    /// Internal: `check_internal_method_call`/`_property_access`/
    /// `_class_constant_access` all *require* `registry.classes.get(owner)`
    /// to succeed at all before considering either tag (an unresolved/
    /// external declaring class never reports, even when the member's
    /// own `sig.is_internal` is true) - mirrored here via the
    /// `class.is_some_and` wrapping the whole own-tag-or-cascade OR, not
    /// just the cascade half. Deprecated has no such requirement
    /// (`check_deprecated_*` checks the own tag regardless of whether the
    /// registry lookup for the cascade succeeds), so its computation is
    /// NOT wrapped the same way.
    pub(crate) fn record_member_usage_token(
        &mut self,
        kind: UsageTokenKind,
        owner: &str,
        own_deprecated: bool,
        own_internal: bool,
        span: Span,
    ) {
        let class = self.registry.classes.get(owner);
        let deprecated = !self.current_scope_deprecated
            && (own_deprecated || class.is_some_and(|c| c.deprecated.is_some()));
        let internal = class.is_some_and(|c| {
            (own_internal || c.is_internal) && self.should_report_internal_usage(owner)
        });
        self.record_usage_semantic_token(kind, span, deprecated, internal);
    }

    /// `new Foo()` - mirrors `check_deprecated_instantiation`/
    /// `check_internal_instantiation` exactly: both require the registry
    /// lookup itself to succeed; internal additionally requires
    /// `should_report_internal_usage(class)`.
    pub(crate) fn record_instantiation_usage_token(&mut self, class: &str, span: Span) {
        let info = self.registry.classes.get(class);
        let deprecated =
            !self.current_scope_deprecated && info.is_some_and(|i| i.deprecated.is_some());
        let internal =
            info.is_some_and(|i| i.is_internal) && self.should_report_internal_usage(class);
        self.record_usage_semantic_token(UsageTokenKind::Class, span, deprecated, internal);
    }
}
