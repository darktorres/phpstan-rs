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
//! or parameter-value conditions). Path zones
//! (`RestrictedSymbolEntry::allowed_at_path`) and attribute-context
//! matching are both checked where the usage is reported
//! (`checker/restricted_symbol_usage.rs`).
//!
//! Still explicitly out of scope: its namespace/enum/superglobal/
//! keyword-specific siblings, each a materially different detection shape
//! needing its own separate investigation.

use rustc_hash::FxHashMap;

use serde::Deserialize;

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

/// A scalar literal to match a call argument's own already-inferred
/// literal type against (real `value:` directive) - only scalars are
/// supported (confirmed against the real docs: "only scalar values are
/// supported with the `value` configuration directive"), matched
/// against `Type::{True,False,IntLiteral,FloatLiteral,StringLiteral}`.
/// `#[serde(untagged)]` picks whichever variant the configured TOML
/// value's own type already is - no separate tag needed.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ParamScalarValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

/// One `{position, name, value|typeString|classPattern}` parameter
/// condition (real docs: "if you don't specify `name`, the named
/// parameter will not be found... vice versa... only the `name` key
/// matches the positional one"). `position` is 1-indexed, counting only
/// non-named, non-spread arguments - matches the real package's own
/// argument-resolution semantics directly against the call site, not
/// against the callee's own declared parameter names (a named
/// condition needs nothing more than `arg.name == configured_name`).
/// Only one of `value`/`type_string`/`class_pattern` is meaningful at
/// once - `type_string` takes precedence over `value` if both are set
/// (confirmed against the real docs), `class_pattern` over `type_string`.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct ParamCondition {
    pub position: Option<u32>,
    pub name: Option<String>,
    pub value: Option<ParamScalarValue>,
    pub type_string: Option<String>,
    pub class_pattern: Option<String>,
}

/// `ParamCondition`'s own presence-only sibling, for the real `*AnyValue`
/// directive family ("don't care about the parameter's value but want
/// to make sure the parameter is passed").
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct ParamRef {
    pub position: Option<u32>,
    pub name: Option<String>,
}

/// `ParamCondition`'s own bitmask sibling, for the real `ParamFlags`
/// directive family - matches when the argument's own resolved integer
/// value has every bit of `value` (a single flag) set, or every bit of
/// *any one* member of `type_string`'s own parsed union set (real docs:
/// "if you want to (dis)allow a parameter when either the flag `1` or
/// `2` is set, use `typeString: 1 | 2`" - note this `|` is a PHPDoc
/// union, not PHP's own bitwise-or operator). No `class_pattern`
/// sibling here - a flag mask is always an integer, never an object.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct ParamFlagsCondition {
    pub position: Option<u32>,
    pub name: Option<String>,
    pub value: Option<i64>,
    pub type_string: Option<String>,
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
    /// diagnostic's own already-relative file path, spelled with `/`
    /// between its names on every platform (`diagnostics::report_path`; a
    /// `\` in a pattern is a separator too, so a configuration written on
    /// Windows names the same files elsewhere,
    /// `ignore_errors::path_glob_matches`) - a match allows (drops) it.
    /// Empty means no path-based allowance at all.
    #[serde(default)]
    pub allow_in: Vec<String>,
    /// The inverse of `allow_in`: when non-empty, the item is reported
    /// only when the path *does* match one of these patterns, and allowed
    /// (dropped) otherwise - mirrors real `allowExceptIn`/`disallowIn`.
    #[serde(default)]
    pub allow_except_in: Vec<String>,
    /// `fnmatch()`-style patterns against an attribute name - a match
    /// against any attribute on the *enclosing class* of the reported
    /// usage allows it. Checked against `Checker::current_class_attrs`.
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
    /// `Function`/`Method`/`StaticMethod` kinds only (the other four
    /// have no call-site arguments to condition on): real
    /// `allowParamsAnywhere` - allows a call anywhere in the codebase
    /// when it's made with *every* listed condition satisfied (an
    /// AND across the list, confirmed against the real docs' own
    /// `log()` worked example: "calls are allowed when called with all
    /// parameters listed"), on top of the ordinary "disallowed
    /// everywhere" default.
    #[serde(default)]
    pub allow_params_anywhere: Vec<ParamCondition>,
    /// `allow_params_anywhere`'s presence-only sibling (real
    /// `allowParamsAnywhereAnyValue`) - every listed parameter must be
    /// passed, with any value.
    #[serde(default)]
    pub allow_params_anywhere_any_value: Vec<ParamRef>,
    /// The inverse of `allow_params_anywhere` (real
    /// `allowExceptParamsAnywhere`, alias `disallowParamsAnywhere`):
    /// unlike the additive field above, a full match here *inverts the
    /// entry's own default* - allowed everywhere except a call made
    /// with every listed condition satisfied, where it's still
    /// reported (confirmed against the real docs' `hash()`/`foo()`
    /// worked examples).
    #[serde(default)]
    pub allow_except_params_anywhere: Vec<ParamCondition>,
    /// `allow_except_params_anywhere`'s presence-only sibling (real
    /// `allowExceptParamsAnyValue`, alias `disallowParamsAnyValue`) -
    /// disallows only when every listed parameter is passed, with any
    /// value (confirmed against the real docs' `waldo()` example:
    /// `waldo('foo')`, with only one of two listed parameters present,
    /// stays allowed).
    #[serde(default)]
    pub allow_except_params_any_value: Vec<ParamRef>,
    /// `allow_except_params_anywhere`'s case-insensitive sibling (real
    /// `allowExceptCaseInsensitiveParams`, alias
    /// `disallowCaseInsensitiveParams`) - identical default-inverting
    /// shape, except a `value:` string condition matches regardless of
    /// case (confirmed against the real docs' `hash('md5', ...)`
    /// example - only `value:`-shaped conditions are affected;
    /// `type_string`/`class_pattern` matching is unrelated to string
    /// casing).
    #[serde(default)]
    pub allow_except_case_insensitive_params: Vec<ParamCondition>,
    /// Narrows whichever of `allow_in_instance_of`/`allow_except_in_
    /// instance_of`/`allow_in_methods`/`allow_except_in_methods`/
    /// `allow_in`/`allow_except_in` this same entry also configures
    /// (real `allowParamsInAllowed`: "the parameter condition is
    /// applied to the location matched by the accompanying allow
    /// directive"). Purely additive on top of an `allow_in_*`-style
    /// zone (both the zone match *and* every listed condition must
    /// hold to be allowed); combined with an `allow_except_in_*`-style
    /// zone instead, a match lets the call through *inside* that zone
    /// (confirmed against the real docs' `Controller::handleRequest()`
    /// worked example).
    #[serde(default)]
    pub allow_params_in_allowed: Vec<ParamCondition>,
    /// `allow_params_in_allowed`'s presence-only sibling.
    #[serde(default)]
    pub allow_params_in_allowed_any_value: Vec<ParamRef>,
    /// The inverse of `allow_params_in_allowed` (real
    /// `allowExceptParamsInAllowed`, alias `disallowParamsInAllowed`):
    /// combined with an `allow_in_*`-style zone, a match *disallows* the
    /// call inside the zone instead of allowing it (confirmed against
    /// the real docs' `views/`+`waldo()` worked example: allowed in
    /// `views/*` unless the 2nd parameter is `'quux'`); combined with an
    /// `allow_except_in_*`-style zone, a match keeps the call disallowed
    /// inside the zone, same as having no param condition at all
    /// (confirmed against the real docs' `HandlerInterface`+`dispatch()`
    /// worked example).
    #[serde(default)]
    pub allow_except_params_in_allowed: Vec<ParamCondition>,
    /// `allow_params_in_allowed`'s bitmask sibling (real
    /// `allowParamFlagsInAllowed`).
    #[serde(default)]
    pub allow_param_flags_in_allowed: Vec<ParamFlagsCondition>,
    /// `allow_params_anywhere`'s bitmask sibling (real
    /// `allowParamFlagsAnywhere`).
    #[serde(default)]
    pub allow_param_flags_anywhere: Vec<ParamFlagsCondition>,
    /// `allow_except_params_in_allowed`'s bitmask sibling (real
    /// `allowExceptParamFlagsInAllowed`, alias
    /// `disallowParamFlagsInAllowed`).
    #[serde(default)]
    pub allow_except_param_flags_in_allowed: Vec<ParamFlagsCondition>,
    /// `allow_except_params_anywhere`'s bitmask sibling (real
    /// `allowExceptParamFlags`, alias `disallowParamFlags`).
    #[serde(default)]
    pub allow_except_param_flags: Vec<ParamFlagsCondition>,
}

impl RestrictedSymbolEntry {
    /// Whether this entry configures any `*ParamsInAllowed`/
    /// `*ParamFlagsInAllowed` field at all -
    /// `checker::restricted_symbol_usage`'s own unconditional zone
    /// checks (`is_allowed_by_calling_context`/`is_allowed_by_
    /// instance_of_context`) defer to the params-aware combined check
    /// instead of evaluating `allow_in_methods`/`allow_in_instance_of`/
    /// their `allow_except_in_*` siblings on their own whenever this is
    /// `true`, so the same zone isn't evaluated twice with two
    /// different (param-unaware vs. param-aware) answers.
    pub(crate) fn has_params_in_allowed_directive(&self) -> bool {
        !self.allow_params_in_allowed.is_empty()
            || !self.allow_params_in_allowed_any_value.is_empty()
            || !self.allow_except_params_in_allowed.is_empty()
            || !self.allow_param_flags_in_allowed.is_empty()
            || !self.allow_except_param_flags_in_allowed.is_empty()
    }
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

impl RestrictedSymbolEntry {
    /// Whether the path-scoped `allow_in`/`allow_except_in` zones allow
    /// this entry in the file at `path` (spelled relative to the project
    /// root, as `diagnostics::report_path` spells it). `params_gate` is the
    /// already-resolved
    /// `allow_params_in_allowed`/`allow_except_params_in_allowed` verdict
    /// at the call site, when the entry configures either: it narrows a
    /// matched zone the same way it narrows the instance-of/methods
    /// zones, and an entry without one keeps the zone's unconditional
    /// behavior.
    pub(crate) fn allowed_at_path(&self, path: &str, params_gate: Option<bool>) -> bool {
        let matches = |p: &String| crate::ignore_errors::path_glob_matches(p, path);
        if self.allow_in.iter().any(matches) {
            return params_gate.unwrap_or(true);
        }
        if !self.allow_except_in.is_empty() {
            if !self.allow_except_in.iter().any(matches) {
                return true;
            }
            return params_gate.unwrap_or(false);
        }
        false
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
            allow_params_anywhere: Vec::new(),
            allow_params_anywhere_any_value: Vec::new(),
            allow_except_params_anywhere: Vec::new(),
            allow_except_params_any_value: Vec::new(),
            allow_except_case_insensitive_params: Vec::new(),
            allow_params_in_allowed: Vec::new(),
            allow_params_in_allowed_any_value: Vec::new(),
            allow_except_params_in_allowed: Vec::new(),
            allow_param_flags_in_allowed: Vec::new(),
            allow_param_flags_anywhere: Vec::new(),
            allow_except_param_flags_in_allowed: Vec::new(),
            allow_except_param_flags: Vec::new(),
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

    fn zoned(allow_in: &[&str], allow_except_in: &[&str]) -> RestrictedSymbolEntry {
        let mut e = entry(RestrictedSymbolKind::Function, None, "var_dump");
        e.allow_in = allow_in.iter().map(|p| p.to_string()).collect();
        e.allow_except_in = allow_except_in.iter().map(|p| p.to_string()).collect();
        e
    }

    #[test]
    fn a_matching_allow_in_pattern_allows_the_entry() {
        assert!(zoned(&["tests/*.php"], &[]).allowed_at_path("tests/Foo.php", None));
    }

    #[test]
    fn a_non_matching_allow_in_pattern_keeps_the_entry_restricted() {
        assert!(!zoned(&["tests/*.php"], &[]).allowed_at_path("src/Foo.php", None));
    }

    #[test]
    fn allow_except_in_only_reports_inside_the_matching_paths() {
        let e = zoned(&[], &["legacy/*.php"]);
        assert!(!e.allowed_at_path("legacy/Foo.php", None));
        assert!(e.allowed_at_path("src/Foo.php", None));
    }

    #[test]
    fn an_entry_without_path_zones_is_never_allowed_by_path() {
        assert!(!zoned(&[], &[]).allowed_at_path("src/Foo.php", None));
    }

    #[test]
    fn a_backslash_in_a_zone_is_a_separator() {
        assert!(zoned(&["tests\\*.php"], &[]).allowed_at_path("tests/Foo.php", None));
        let e = zoned(&[], &["legacy\\*"]);
        assert!(!e.allowed_at_path("legacy/a.php", None));
        assert!(e.allowed_at_path("src/a.php", None));
    }

    #[test]
    fn params_in_allowed_gate_narrows_an_allow_in_match() {
        let e = zoned(&["views/*"], &[]);
        assert!(e.allowed_at_path("views/Foo.php", Some(true)));
        assert!(!e.allowed_at_path("views/Foo.php", Some(false)));
    }

    #[test]
    fn params_in_allowed_gate_is_ignored_when_the_allow_in_path_does_not_match() {
        assert!(!zoned(&["views/*"], &[]).allowed_at_path("src/Foo.php", Some(true)));
    }

    #[test]
    fn params_in_allowed_gate_narrows_an_allow_except_in_match() {
        let e = zoned(&[], &["legacy/*.php"]);
        assert!(e.allowed_at_path("legacy/Foo.php", Some(true)));
        assert!(!e.allowed_at_path("legacy/Foo.php", Some(false)));
    }

    #[test]
    fn params_in_allowed_gate_is_irrelevant_outside_an_allow_except_in_zone() {
        assert!(zoned(&[], &["legacy/*.php"]).allowed_at_path("src/Foo.php", Some(false)));
    }
}
