//! A purely declarative, config-driven "restricted symbols" list - the
//! closest buildable analog, for this project's own constraints (no
//! NEON, no code-plugin mechanism), to real PHPStan's
//! `RestrictedInternal*UsageExtension` family and the real, independently
//! popular third-party `spaze/phpstan-disallowed-calls` package, which
//! implements exactly this shape: a plain parameter list banning specific
//! functions/methods/properties/constants/classes project-wide with a
//! custom message, zero plugin-class-writing required.
//!
//! Config-file-only (no CLI flag) - a structured list doesn't fit a
//! boolean/string CLI flag, the same reasoning `Config::ignore_errors`
//! already established. Unlike `ignore_errors` (a post-hoc filter over
//! already-produced diagnostics), this compiles into a `Registry`-held
//! index consulted *during* the check pass - `check_internal_usage`'s own
//! registry-field wiring shape is the real precedent here, not
//! `ignore_errors`'s.
//!
//! Explicitly out of scope for this pass: the real `spaze` package's much
//! larger allow/disallow-in filter DSL (attribute-context matching, call-
//! site-scope-path matching) - no such mechanism exists anywhere in this
//! checker to build on yet, a genuine missing-prerequisite gap; and its
//! namespace/enum/superglobal/keyword-specific siblings, each a
//! materially different detection shape needing its own investigation.

use serde::Deserialize;

use rustc_hash::FxHashMap;

/// PHP's own case-(in)sensitivity rules govern the lookup-key casing
/// below: function/method/static-method/class names are case-insensitive
/// in PHP, but property/static-property/class-constant names are
/// case-sensitive (exactly like class constants) - only the first four
/// kinds get lowercased keys.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum RestrictedSymbolKind {
    Function,
    Method,
    StaticMethod,
    Property,
    StaticProperty,
    ClassConstant,
    Class,
}

/// One `[[restricted_symbols]]` entry.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RestrictedSymbolEntry {
    pub kind: RestrictedSymbolKind,
    /// The declaring class FQN - required for every kind except
    /// `Function`/`Class` (silently inert if given where it doesn't apply
    /// or missing where it's required, the same lenient "a malformed
    /// entry never matches" posture `IgnoreErrorEntry`'s own malformed-
    /// regex handling already established).
    pub class: Option<String>,
    pub name: String,
    pub message: String,
}

/// A compiled, case-folded index over a project's own configured
/// `restricted_symbols` list - built once (`RestrictedSymbols::compile`),
/// consulted at every real call/access site via `checker::restricted_
/// symbol_usage`'s own fast-bail functions. Empty by default
/// (`#[derive(Default)]`), so an unconfigured project pays zero lookup
/// cost beyond one `is_empty()` check per site.
#[derive(Debug, Default)]
pub struct RestrictedSymbols {
    functions: FxHashMap<String, RestrictedSymbolEntry>,
    // Real PHPStan/`spaze`'s own `Class` kind bans a class from every
    // native class-name-usage location (instantiation/instanceof/catch/
    // extends/implements/param-and-return-type-hints) - mirrors
    // `internal_usage.rs`'s own `check_internal_class_name_usage`/
    // `check_internal_inheritance`/`check_internal_param_and_return_
    // types_raw` threading, the exact call-site list a `Class`-kind ban
    // needs.
    classes: FxHashMap<String, RestrictedSymbolEntry>,
    methods: FxHashMap<(String, String), RestrictedSymbolEntry>,
    static_methods: FxHashMap<(String, String), RestrictedSymbolEntry>,
    properties: FxHashMap<(String, String), RestrictedSymbolEntry>,
    static_properties: FxHashMap<(String, String), RestrictedSymbolEntry>,
    class_constants: FxHashMap<(String, String), RestrictedSymbolEntry>,
}

impl RestrictedSymbols {
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
            && self.classes.is_empty()
            && self.methods.is_empty()
            && self.static_methods.is_empty()
            && self.properties.is_empty()
            && self.static_properties.is_empty()
            && self.class_constants.is_empty()
    }

    pub fn compile(entries: &[RestrictedSymbolEntry]) -> Self {
        let mut out = RestrictedSymbols::default();
        for entry in entries {
            match entry.kind {
                RestrictedSymbolKind::Function => {
                    out.functions
                        .insert(entry.name.to_lowercase(), entry.clone());
                }
                RestrictedSymbolKind::Class => {
                    out.classes.insert(entry.name.to_lowercase(), entry.clone());
                }
                RestrictedSymbolKind::Method => {
                    if let Some(class) = &entry.class {
                        out.methods.insert(
                            (class.to_lowercase(), entry.name.to_lowercase()),
                            entry.clone(),
                        );
                    }
                }
                RestrictedSymbolKind::StaticMethod => {
                    if let Some(class) = &entry.class {
                        out.static_methods.insert(
                            (class.to_lowercase(), entry.name.to_lowercase()),
                            entry.clone(),
                        );
                    }
                }
                RestrictedSymbolKind::Property => {
                    if let Some(class) = &entry.class {
                        // Property names are case-sensitive in PHP - only
                        // the class-name half is lowercased.
                        out.properties
                            .insert((class.to_lowercase(), entry.name.clone()), entry.clone());
                    }
                }
                RestrictedSymbolKind::StaticProperty => {
                    if let Some(class) = &entry.class {
                        out.static_properties
                            .insert((class.to_lowercase(), entry.name.clone()), entry.clone());
                    }
                }
                RestrictedSymbolKind::ClassConstant => {
                    if let Some(class) = &entry.class {
                        // Class-constant names are case-sensitive too.
                        out.class_constants
                            .insert((class.to_lowercase(), entry.name.clone()), entry.clone());
                    }
                }
            }
        }
        out
    }

    pub(crate) fn function(&self, name: &str) -> Option<&RestrictedSymbolEntry> {
        self.functions.get(&name.to_lowercase())
    }

    pub(crate) fn class(&self, name: &str) -> Option<&RestrictedSymbolEntry> {
        self.classes.get(&name.to_lowercase())
    }

    pub(crate) fn method(&self, class: &str, name: &str) -> Option<&RestrictedSymbolEntry> {
        self.methods
            .get(&(class.to_lowercase(), name.to_lowercase()))
    }

    pub(crate) fn static_method(&self, class: &str, name: &str) -> Option<&RestrictedSymbolEntry> {
        self.static_methods
            .get(&(class.to_lowercase(), name.to_lowercase()))
    }

    pub(crate) fn property(&self, class: &str, name: &str) -> Option<&RestrictedSymbolEntry> {
        self.properties
            .get(&(class.to_lowercase(), name.to_string()))
    }

    pub(crate) fn static_property(
        &self,
        class: &str,
        name: &str,
    ) -> Option<&RestrictedSymbolEntry> {
        self.static_properties
            .get(&(class.to_lowercase(), name.to_string()))
    }

    pub(crate) fn class_constant(&self, class: &str, name: &str) -> Option<&RestrictedSymbolEntry> {
        self.class_constants
            .get(&(class.to_lowercase(), name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: RestrictedSymbolKind, class: Option<&str>, name: &str) -> RestrictedSymbolEntry {
        RestrictedSymbolEntry {
            kind,
            class: class.map(str::to_string),
            name: name.to_string(),
            message: "do not use this".to_string(),
        }
    }

    #[test]
    fn function_and_class_lookups_are_case_insensitive() {
        let symbols =
            RestrictedSymbols::compile(&[entry(RestrictedSymbolKind::Function, None, "var_dump")]);
        assert!(symbols.function("VAR_DUMP").is_some());
        assert!(symbols.function("var_dump").is_some());
        assert!(symbols.function("other").is_none());
    }

    #[test]
    fn property_lookups_are_case_sensitive_on_the_member_name() {
        let symbols = RestrictedSymbols::compile(&[entry(
            RestrictedSymbolKind::Property,
            Some("Legacy\\Db"),
            "password",
        )]);
        assert!(symbols.property("Legacy\\Db", "password").is_some());
        assert!(symbols.property("legacy\\db", "password").is_some());
        assert!(symbols.property("Legacy\\Db", "Password").is_none());
    }

    #[test]
    fn class_constant_lookups_are_case_sensitive_on_the_member_name() {
        let symbols = RestrictedSymbols::compile(&[entry(
            RestrictedSymbolKind::ClassConstant,
            Some("Legacy\\Db"),
            "VERSION",
        )]);
        assert!(symbols.class_constant("Legacy\\Db", "VERSION").is_some());
        assert!(symbols.class_constant("Legacy\\Db", "version").is_none());
    }

    #[test]
    fn an_entry_missing_its_required_class_is_silently_inert() {
        let symbols =
            RestrictedSymbols::compile(&[entry(RestrictedSymbolKind::Method, None, "query")]);
        assert!(symbols.is_empty());
    }

    #[test]
    fn empty_config_produces_an_empty_index() {
        assert!(RestrictedSymbols::default().is_empty());
    }
}
