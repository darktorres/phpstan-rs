//! Phase 67: checker-side consumption of a project's own configured
//! `restricted_symbols` list (`crate::restricted_symbols::Restricted
//! Symbols`) - sibling of `internal_usage.rs`, reusing the identical
//! call sites (owner/member/span/label are already resolved there for
//! the `@internal` checks), one function per event, each opening with a
//! fast `is_empty()` bail.

use crate::ast::ClassKind;
use crate::checker::Checker;
use crate::checker::internal_usage::class_type_word;
use crate::token::Span;

impl<'a> Checker<'a> {
    pub(crate) fn check_restricted_function_call(&mut self, fqn: &str, span: Span, label: &str) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        let bare = fqn.rsplit('\\').next().unwrap_or(fqn);
        let Some(entry) = self.registry.restricted_symbols.function(bare) else {
            return;
        };
        self.report(
            label.to_string(),
            format!("Call to restricted function {bare}(): {}", entry.message),
            span,
            0,
            "restrictedSymbol.function",
        );
    }

    pub(crate) fn check_restricted_method_call(
        &mut self,
        owner: &str,
        method: &str,
        is_static: bool,
        span: Span,
        label: &str,
    ) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        let entry = if is_static {
            self.registry
                .restricted_symbols
                .static_method(owner, method)
        } else {
            self.registry.restricted_symbols.method(owner, method)
        };
        let Some(entry) = entry else { return };
        let static_word = if is_static { "static " } else { "" };
        let identifier = if is_static {
            "restrictedSymbol.staticMethod"
        } else {
            "restrictedSymbol.method"
        };
        self.report(
            label.to_string(),
            format!(
                "Call to restricted {static_word}method {owner}::{method}(): {}",
                entry.message
            ),
            span,
            0,
            identifier,
        );
    }

    pub(crate) fn check_restricted_property_access(
        &mut self,
        owner: &str,
        prop: &str,
        is_static: bool,
        span: Span,
        label: &str,
    ) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        let entry = if is_static {
            self.registry
                .restricted_symbols
                .static_property(owner, prop)
        } else {
            self.registry.restricted_symbols.property(owner, prop)
        };
        let Some(entry) = entry else { return };
        let static_word = if is_static { "static " } else { "" };
        let identifier = if is_static {
            "restrictedSymbol.staticProperty"
        } else {
            "restrictedSymbol.property"
        };
        self.report(
            label.to_string(),
            format!(
                "Access to restricted {static_word}property {owner}::${prop}: {}",
                entry.message
            ),
            span,
            0,
            identifier,
        );
    }

    pub(crate) fn check_restricted_class_constant_access(
        &mut self,
        owner: &str,
        const_name: &str,
        span: Span,
        label: &str,
    ) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        let Some(entry) = self
            .registry
            .restricted_symbols
            .class_constant(owner, const_name)
        else {
            return;
        };
        self.report(
            label.to_string(),
            format!(
                "Access to restricted constant {owner}::{const_name}: {}",
                entry.message
            ),
            span,
            0,
            "restrictedSymbol.classConstant",
        );
    }

    /// Shared by every native class-name-usage location a `Class`-kind
    /// ban covers (instantiation/`instanceof`/`catch`/`extends`/
    /// `implements`/param-and-return-type-hints) - mirrors `internal_
    /// usage.rs`'s own `check_internal_class_name_usage` shape.
    pub(crate) fn check_restricted_class_name_usage(
        &mut self,
        class: &str,
        verb_phrase: &str,
        span: Span,
        label: &str,
    ) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        let Some(entry) = self.registry.restricted_symbols.class(class) else {
            return;
        };
        let type_word = self
            .registry
            .classes
            .get(class)
            .map(|info| class_type_word(info.kind))
            .unwrap_or_else(|| class_type_word(ClassKind::Class));
        self.report(
            label.to_string(),
            format!(
                "{verb_phrase} restricted {} {class}: {}",
                type_word.to_lowercase(),
                entry.message
            ),
            span,
            0,
            "restrictedSymbol.class",
        );
    }
}
