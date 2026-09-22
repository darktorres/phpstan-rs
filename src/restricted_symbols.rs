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
//! `allow_in`/`allow_except_in` (path-scoped) and `allow_in_class_with_
//! attributes`/`allow_except_in_class_with_attributes`/`allow_in_methods_
//! with_attributes`/`allow_except_in_methods_with_attributes` (attribute-
//! context-scoped) mirror the real `spaze` package's own `allowIn`/
//! `allowExceptIn`/`allowInClassWithAttributes`/etc. (confirmed directly
//! against its real docs, not recalled knowledge) - a deliberately
//! narrower slice of its much larger filter DSL (no `allowInClassWith
//! MethodAttributes`, `allowInParamTypes`/`allowInReturnType`, `allowInUse`,
//! or parameter-value conditions). Path-scoping is a post-filter
//! (`filter_by_path` below), not a `Checker`-level field - see that
//! function's own doc comment for why. Attribute-context matching is
//! checked inline instead, against `Checker::current_class_attrs`/
//! `current_method_attrs` (`checker/restricted_symbol_usage.rs`), since
//! (unlike the file path) the checker already has that context on hand
//! at report time.
//!
//! Still explicitly out of scope: its namespace/enum/superglobal/
//! keyword-specific siblings, each a materially different detection shape
//! needing its own separate investigation.

use crate::checker::Diagnostic;
use std::collections::HashMap;
use std::path::PathBuf;

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
    /// `fnmatch()`-style patterns (`*` matches `/` too, mirroring
    /// `ignore_errors::glob_matches`' own identical semantics) against a
    /// diagnostic's own already-relative file path - a match allows
    /// (drops) it. Empty means no path-based allowance at all.
    #[serde(default)]
    pub allow_in: Vec<String>,
    /// The inverse of `allow_in`: when non-empty, the item is reported
    /// only when the path *does* match one of these patterns, and allowed
    /// (dropped) otherwise - mirrors real `allowExceptIn`/`disallowIn`.
    #[serde(default)]
    pub allow_except_in: Vec<String>,
    /// `fnmatch()`-style patterns against an attribute name - a match
    /// against any attribute on the *enclosing class* of the reported
    /// usage allows it. Checked inline (`Checker::current_class_attrs`),
    /// not via the path post-filter.
    #[serde(default)]
    pub allow_in_class_with_attributes: Vec<String>,
    /// The inverse of `allow_in_class_with_attributes`: when non-empty,
    /// the item is reported only when the enclosing class *does* carry a
    /// matching attribute, and allowed otherwise.
    #[serde(default)]
    pub allow_except_in_class_with_attributes: Vec<String>,
    /// Same as `allow_in_class_with_attributes`, checked against the
    /// enclosing method/function's own attributes instead
    /// (`Checker::current_method_attrs`).
    #[serde(default)]
    pub allow_in_methods_with_attributes: Vec<String>,
    /// The inverse of `allow_in_methods_with_attributes`.
    #[serde(default)]
    pub allow_except_in_methods_with_attributes: Vec<String>,
    /// `fnmatch()`-style patterns against an attribute name - a match
    /// against *any* method in the enclosing class (including the one
    /// the restricted usage itself lives in, but never the class's own
    /// attributes) allows it. Confirmed distinct from `allow_in_methods_
    /// with_attributes` above (current method only) against the real
    /// `spaze/phpstan-disallowed-calls` docs directly.
    #[serde(default)]
    pub allow_in_class_with_method_attributes: Vec<String>,
    /// The inverse of `allow_in_class_with_method_attributes`.
    #[serde(default)]
    pub allow_except_in_class_with_method_attributes: Vec<String>,
    /// `Class`-kind only: a plain, class-shaped `use Foo;` import is not
    /// itself a type hint (real PHPStan's own `ClassNameCheck::
    /// checkClassNames`, called with a `null` usage location for this
    /// exact shape, never calls `hasClass` there - but a `Class`-kind ban
    /// *does* still report it as its own, separate occurrence there by
    /// default, confirmed against the real `spaze/phpstan-disallowed-
    /// calls` docs: "the `use` line will still be reported as a separate
    /// occurrence"). `true` here suppresses exactly that one occurrence,
    /// leaving every other detection surface (`new`/`instanceof`/
    /// `catch`/`extends`/`implements`/param-and-return-type-hints)
    /// unaffected, mirroring the real `allowInUse` key exactly.
    #[serde(default)]
    pub allow_in_use: bool,
    /// `Class`-kind only: `true` allows this class specifically when
    /// used as a parameter type hint, on top of the ordinary "disallowed
    /// everywhere" default - purely additive, unlike its own
    /// `allow_except_in_param_types` inverse just below.
    #[serde(default)]
    pub allow_in_param_types: bool,
    /// `Class`-kind only, alias `disallowInParamTypes` in the real
    /// package (this project picks one canonical spelling, matching
    /// every other `allow_except_in_*` field here having no separate
    /// `disallow_in_*` alias either): unlike every other allow field in
    /// this struct, `true` here *inverts the entry's own default
    /// entirely* - the class becomes allowed everywhere except
    /// specifically as a parameter type hint, where it's still reported
    /// (confirmed against the real docs' own worked example: a `use` of
    /// the class, `new`-ing it, and using it as a *return* type all
    /// become allowed once this is set).
    #[serde(default)]
    pub allow_except_in_param_types: bool,
    /// `allow_in_param_types`'s return-type sibling.
    #[serde(default)]
    pub allow_in_return_type: bool,
    /// `allow_except_in_param_types`'s return-type sibling - the same
    /// default-inverting behavior, just for the return-type-hint
    /// position instead.
    #[serde(default)]
    pub allow_except_in_return_type: bool,
    /// `fnmatch()`-style patterns against the *calling* function or
    /// method's own `Class::method()`/`function()`-shaped name (real
    /// `allowInFunctions`, alias `allowInMethods` - this project picks
    /// one canonical spelling, the same call every other alias-bearing
    /// field here already made) - a match allows the item when called
    /// from that context, on top of the ordinary "disallowed
    /// everywhere" default.
    #[serde(default)]
    pub allow_in_methods: Vec<String>,
    /// The inverse of `allow_in_methods` (real `allowExceptInFunctions`,
    /// aliases `allowExceptInMethods`/`disallowInFunctions`/
    /// `disallowInMethods`): unlike `allow_in_methods`, a match here
    /// *inverts the entry's own default* - allowed everywhere except
    /// when called from a matching context, where it's still reported
    /// (confirmed against the real docs' own worked example: `Controller::
    /// redirect()` stays allowed everywhere, reported only when called
    /// from a `Controller\Foo\Bar\*::__construct()`-matching method).
    #[serde(default)]
    pub allow_except_in_methods: Vec<String>,
    /// `fnmatch()`-style patterns against the current class's own name
    /// *and* every transitive `extends`/`implements` ancestor's name
    /// (real `allowInInstanceOf`, named after PHP's `instanceof`
    /// operator) - a match allows the item when used inside a class
    /// that *is*, extends, or implements a matching name, on top of the
    /// ordinary "disallowed everywhere" default.
    #[serde(default)]
    pub allow_in_instance_of: Vec<String>,
    /// The inverse of `allow_in_instance_of` (real
    /// `allowExceptInInstanceOf`, alias `disallowInInstanceOf`): a match
    /// *inverts the entry's own default* - allowed everywhere except
    /// inside a matching class hierarchy, where it's still reported.
    #[serde(default)]
    pub allow_except_in_instance_of: Vec<String>,
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

/// The two metadata keys `checker::restricted_symbol_usage`'s own
/// `report_restricted` attaches to a diagnostic when its entry has real
/// `allow_in`/`allow_except_in` patterns - read back here, not re-looked-
/// up against the registry, since this runs as a post-filter over already-
/// produced diagnostics with no `Checker`/`Registry` in scope at all.
const ALLOW_IN_METADATA_KEY: &str = "restrictedSymbol.allowIn";
const ALLOW_EXCEPT_IN_METADATA_KEY: &str = "restrictedSymbol.allowExceptIn";

/// Path-scoped `allow_in`/`allow_except_in` (Phase 67's own DSL
/// extension) - a **post-filter** over already-produced diagnostics,
/// mirroring `ignore_errors::filter`'s own precedent exactly, rather than
/// a new `Checker`-level "current file path" field: `Checker::new` has 31
/// call sites across 6 files, and the ordinary per-file checking walk
/// has no other need to know its own path at all. Scoped to `restrictedSymbol.*`
/// identifiers only - every other diagnostic family is untouched.
pub fn filter_by_path(results: &mut HashMap<PathBuf, Vec<Diagnostic>>) {
    for (file, diagnostics) in results.iter_mut() {
        let path = file.display().to_string();
        diagnostics.retain(|d| !is_allowed_by_path(d, &path));
    }
}

fn is_allowed_by_path(d: &Diagnostic, path: &str) -> bool {
    if !d.identifier.starts_with("restrictedSymbol.") {
        return false;
    }
    let allow_in = string_array_metadata(d, ALLOW_IN_METADATA_KEY);
    if allow_in
        .iter()
        .any(|p| crate::ignore_errors::glob_matches(p, path))
    {
        return true;
    }
    let allow_except_in = string_array_metadata(d, ALLOW_EXCEPT_IN_METADATA_KEY);
    if !allow_except_in.is_empty()
        && !allow_except_in
            .iter()
            .any(|p| crate::ignore_errors::glob_matches(p, path))
    {
        return true;
    }
    false
}

fn string_array_metadata(d: &Diagnostic, key: &str) -> Vec<String> {
    d.metadata
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
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
            allow_in: Vec::new(),
            allow_except_in: Vec::new(),
            allow_in_class_with_attributes: Vec::new(),
            allow_except_in_class_with_attributes: Vec::new(),
            allow_in_methods_with_attributes: Vec::new(),
            allow_except_in_methods_with_attributes: Vec::new(),
            allow_in_class_with_method_attributes: Vec::new(),
            allow_except_in_class_with_method_attributes: Vec::new(),
            allow_in_use: false,
            allow_in_param_types: false,
            allow_except_in_param_types: false,
            allow_in_return_type: false,
            allow_except_in_return_type: false,
            allow_in_methods: Vec::new(),
            allow_except_in_methods: Vec::new(),
            allow_in_instance_of: Vec::new(),
            allow_except_in_instance_of: Vec::new(),
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

    fn restricted_diagnostic(
        allow_in: &[&str],
        allow_except_in: &[&str],
    ) -> crate::checker::Diagnostic {
        let mut metadata = std::collections::HashMap::new();
        if !allow_in.is_empty() {
            metadata.insert(
                ALLOW_IN_METADATA_KEY.to_string(),
                serde_json::json!(allow_in),
            );
        }
        if !allow_except_in.is_empty() {
            metadata.insert(
                ALLOW_EXCEPT_IN_METADATA_KEY.to_string(),
                serde_json::json!(allow_except_in),
            );
        }
        crate::checker::Diagnostic {
            location: "f".to_string(),
            message: "restricted".to_string(),
            span: crate::token::Span::default(),
            level: 0,
            identifier: "restrictedSymbol.function".to_string(),
            tip: None,
            non_ignorable: false,
            metadata,
            fix: None,
        }
    }

    #[test]
    fn a_matching_allow_in_pattern_drops_the_diagnostic() {
        assert!(is_allowed_by_path(
            &restricted_diagnostic(&["tests/*.php"], &[]),
            "tests/Foo.php"
        ));
    }

    #[test]
    fn a_non_matching_allow_in_pattern_keeps_the_diagnostic() {
        assert!(!is_allowed_by_path(
            &restricted_diagnostic(&["tests/*.php"], &[]),
            "src/Foo.php"
        ));
    }

    #[test]
    fn allow_except_in_only_reports_inside_the_matching_paths() {
        let d = restricted_diagnostic(&[], &["legacy/*.php"]);
        assert!(!is_allowed_by_path(&d, "legacy/Foo.php"));
        assert!(is_allowed_by_path(&d, "src/Foo.php"));
    }

    #[test]
    fn a_non_restricted_symbol_diagnostic_is_never_touched_by_the_path_filter() {
        let mut d = restricted_diagnostic(&["tests/*.php"], &[]);
        d.identifier = "argument.type".to_string();
        assert!(!is_allowed_by_path(&d, "src/Foo.php"));
    }

    #[test]
    fn filter_by_path_drops_only_the_allowed_entries() {
        let mut results: HashMap<PathBuf, Vec<crate::checker::Diagnostic>> = HashMap::new();
        results.insert(
            PathBuf::from("tests/Foo.php"),
            vec![restricted_diagnostic(&["tests/*.php"], &[])],
        );
        results.insert(
            PathBuf::from("src/Foo.php"),
            vec![restricted_diagnostic(&["tests/*.php"], &[])],
        );
        filter_by_path(&mut results);
        assert!(results[&PathBuf::from("tests/Foo.php")].is_empty());
        assert_eq!(results[&PathBuf::from("src/Foo.php")].len(), 1);
    }
}
