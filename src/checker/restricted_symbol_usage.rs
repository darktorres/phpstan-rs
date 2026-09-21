//! Checker-side consumption of a project's own configured
//! `restricted_symbols` list (`crate::restricted_symbols::Restricted
//! Symbols`) - reuses the same call sites as `internal_usage.rs`'s
//! `@internal` checks, one function per event, each opening with a fast
//! `is_empty()` bail.

use crate::ast::{ClassKind, FunctionDecl, Param};
use crate::checker::Checker;
use crate::checker::internal_usage::class_type_word;
use crate::checker::internal_usage::collect_referenced_class_names;
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
    /// `implements`/param-and-return-type-hints).
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

    /// Closes a real, live enforcement gap: the `Class` kind's own ban
    /// was never actually checked against an `extends`/`implements`
    /// clause at all (only `instanceof`/`new` called `check_restricted_
    /// class_name_usage`) - mirrors `check_internal_inheritance`'s own
    /// shape exactly (same already-resolved `ClassInfo::extends`/
    /// `implements` fields, same whole-class-declaration span, same two
    /// call sites), minus its trait-`use` half - a banned class named in
    /// a trait's own `use` clause isn't one of the `Class`-kind ban's own
    /// listed detection surfaces.
    pub(crate) fn check_restricted_class_inheritance(&mut self, fqn: &str, span: Span) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let (extends, implements) = (info.extends.clone(), info.implements.clone());
        for parent in &extends {
            self.check_restricted_class_name_usage(parent, &format!("{fqn} extends"), span, fqn);
        }
        for iface in &implements {
            self.check_restricted_class_name_usage(iface, &format!("{fqn} implements"), span, fqn);
        }
    }

    /// `check_internal_param_and_return_types`'s own restricted-symbols
    /// sibling - named functions/methods only, delegating to `check_
    /// restricted_param_and_return_types_raw` below.
    pub(crate) fn check_restricted_param_and_return_types(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        label: &str,
    ) {
        self.check_restricted_param_and_return_types_raw(
            &f.params,
            f.return_type.as_deref(),
            f.return_type_span.unwrap_or(f.span),
            class_name,
            label,
        );
    }

    /// `param`/`return` native type-hint coverage - the third and last
    /// missing piece of the `Class` kind's own listed detection surface.
    /// Mirrors `check_internal_param_and_return_types_raw`'s exact
    /// shape (same native-type-hint-only resolution, same recursive
    /// `Union`/`Intersection` class-name collection via the shared
    /// `collect_referenced_class_names`), called from the identical two
    /// call sites (`checker/expr.rs`'s `Closure`/`ArrowFn` literal-
    /// signature checking and the named-declaration path both already
    /// reach through `check_internal_param_and_return_types_raw`).
    pub(crate) fn check_restricted_param_and_return_types_raw(
        &mut self,
        params: &[Param],
        return_type: Option<&str>,
        return_type_fallback_span: Span,
        class_name: Option<&str>,
        label: &str,
    ) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        for p in params {
            let ty = self.resolve_type_hint(p.type_hint.as_deref());
            let ty = match class_name {
                Some(c) => self.resolve_pseudo(&ty, c),
                None => ty,
            };
            let mut classes = Vec::new();
            collect_referenced_class_names(&ty, &mut classes);
            for class in classes {
                self.check_restricted_class_name_usage(
                    &class,
                    &format!("Parameter ${} references", p.name),
                    p.type_hint_span.unwrap_or(p.name_span),
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
                self.check_restricted_class_name_usage(
                    &class,
                    "Return type references",
                    return_type_fallback_span,
                    label,
                );
            }
        }
    }
}
