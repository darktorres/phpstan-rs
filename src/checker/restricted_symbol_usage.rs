//! Checker-side consumption of a project's own configured
//! `restricted_symbols` list (`crate::restricted_symbols::Restricted
//! Symbols`) - reuses the same call sites as `internal_usage.rs`'s
//! `@internal` checks, one function per event, each opening with a fast
//! `is_empty()` bail.

use crate::ast::{ClassKind, FunctionDecl, Param};
use crate::checker::internal_usage::class_type_word;
use crate::checker::internal_usage::collect_referenced_class_names;
use crate::checker::{Checker, DiagnosticBuilder};
use crate::ignore_errors::glob_matches;
use crate::restricted_symbols::RestrictedSymbolEntry;
use crate::token::Span;

impl<'a> Checker<'a> {
    /// Every `check_restricted_*` report call routes through here -
    /// centralizes both halves of Phase 67's own filter-DSL extension
    /// (confirmed against the real `spaze/phpstan-disallowed-calls`
    /// docs): attribute-context matching (`allow_in_class_with_
    /// attributes`/`allow_in_methods_with_attributes` and their
    /// `allow_except_*` inverses) is checked inline, right here, against
    /// `current_class_attrs`/`current_method_attrs` - the checker already
    /// has that context on hand at report time, unlike the file path
    /// (path-scoped `allow_in`/`allow_except_in` are instead attached as
    /// diagnostic metadata and resolved by a later post-filter,
    /// `restricted_symbols::filter_by_path` - see that function's own doc
    /// comment for why).
    fn report_restricted(
        &mut self,
        entry: &RestrictedSymbolEntry,
        label: &str,
        message: String,
        span: Span,
        identifier: &'static str,
    ) {
        if self.is_allowed_by_attribute_context(entry) {
            return;
        }
        let mut builder = DiagnosticBuilder::new(label.to_string(), message, span, 0, identifier);
        if !entry.allow_in.is_empty() {
            builder = builder.metadata(
                "restrictedSymbol.allowIn",
                serde_json::json!(entry.allow_in),
            );
        }
        if !entry.allow_except_in.is_empty() {
            builder = builder.metadata(
                "restrictedSymbol.allowExceptIn",
                serde_json::json!(entry.allow_except_in),
            );
        }
        self.report_builder(builder);
    }

    /// `allow_in_class_with_attributes`/`allow_except_in_class_with_
    /// attributes` and their method-level siblings, evaluated in the
    /// same "first genuine allow wins" order the real package's own
    /// broader DSL uses (see `restricted_symbols.rs`'s own module doc
    /// comment) - class-level checked before method-level, `allow_in_*`
    /// before its own `allow_except_in_*` inverse. Attribute-name
    /// matching is case-insensitive (`glob_matches` itself isn't, so both
    /// sides are lowercased first), matching PHP's own class-name
    /// case-insensitivity - an attribute name is, after all, a class
    /// name.
    fn is_allowed_by_attribute_context(&self, entry: &RestrictedSymbolEntry) -> bool {
        let matches_any = |patterns: &[String], attrs: &[std::sync::Arc<str>]| {
            patterns.iter().any(|p| {
                let p = p.to_lowercase();
                attrs.iter().any(|a| glob_matches(&p, &a.to_lowercase()))
            })
        };
        if matches_any(
            &entry.allow_in_class_with_attributes,
            &self.current_class_attrs,
        ) {
            return true;
        }
        if !entry.allow_except_in_class_with_attributes.is_empty()
            && !matches_any(
                &entry.allow_except_in_class_with_attributes,
                &self.current_class_attrs,
            )
        {
            return true;
        }
        if matches_any(
            &entry.allow_in_methods_with_attributes,
            &self.current_method_attrs,
        ) {
            return true;
        }
        if !entry.allow_except_in_methods_with_attributes.is_empty()
            && !matches_any(
                &entry.allow_except_in_methods_with_attributes,
                &self.current_method_attrs,
            )
        {
            return true;
        }
        false
    }

    pub(crate) fn check_restricted_function_call(&mut self, fqn: &str, span: Span, label: &str) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        let bare = fqn.rsplit('\\').next().unwrap_or(fqn);
        let Some(entry) = self.registry.restricted_symbols.function(bare) else {
            return;
        };
        let entry = entry.clone();
        self.report_restricted(
            &entry,
            label,
            format!("Call to restricted function {bare}(): {}", entry.message),
            span,
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
        let entry = entry.clone();
        let static_word = if is_static { "static " } else { "" };
        let identifier = if is_static {
            "restrictedSymbol.staticMethod"
        } else {
            "restrictedSymbol.method"
        };
        self.report_restricted(
            &entry,
            label,
            format!(
                "Call to restricted {static_word}method {owner}::{method}(): {}",
                entry.message
            ),
            span,
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
        let entry = entry.clone();
        let static_word = if is_static { "static " } else { "" };
        let identifier = if is_static {
            "restrictedSymbol.staticProperty"
        } else {
            "restrictedSymbol.property"
        };
        self.report_restricted(
            &entry,
            label,
            format!(
                "Access to restricted {static_word}property {owner}::${prop}: {}",
                entry.message
            ),
            span,
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
        let entry = entry.clone();
        self.report_restricted(
            &entry,
            label,
            format!(
                "Access to restricted constant {owner}::{const_name}: {}",
                entry.message
            ),
            span,
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
        let entry = entry.clone();
        let type_word = self
            .registry
            .classes
            .get(class)
            .map(|info| class_type_word(info.kind))
            .unwrap_or_else(|| class_type_word(ClassKind::Class));
        self.report_restricted(
            &entry,
            label,
            format!(
                "{verb_phrase} restricted {} {class}: {}",
                type_word.to_lowercase(),
                entry.message
            ),
            span,
            "restrictedSymbol.class",
        );
    }

    /// The `use`-import detection surface a `Class`-kind ban's own
    /// listed locations never covered at all until now - a real,
    /// confirmed gap: `checker::imports::check_use_class_imports`
    /// already checks a class-shaped `use Foo;` for `class.prefixed`,
    /// but never for a restricted-symbols ban. Kept separate from
    /// `check_restricted_class_name_usage` (rather than threading a new
    /// parameter through its 6 other call sites) since `allow_in_use`
    /// is the one allow-mechanism scoped to exactly this single
    /// location, not shared by any of the others.
    pub(crate) fn check_restricted_class_use_import(&mut self, class: &str, span: Span) {
        if self.registry.restricted_symbols.is_empty() {
            return;
        }
        let Some(entry) = self.registry.restricted_symbols.class(class) else {
            return;
        };
        if entry.allow_in_use {
            return;
        }
        let entry = entry.clone();
        let type_word = self
            .registry
            .classes
            .get(class)
            .map(|info| class_type_word(info.kind))
            .unwrap_or_else(|| class_type_word(ClassKind::Class));
        self.report_restricted(
            &entry,
            "<main>",
            format!(
                "Importing restricted {} {class}: {}",
                type_word.to_lowercase(),
                entry.message
            ),
            span,
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
