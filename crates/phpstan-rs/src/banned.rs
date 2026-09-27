//! Opt-in bans on language constructs (`banned_constructs`) and native
//! types (`banned_types`), checked by `Project::check_banned`, and the
//! built-in `restricted_symbols` presets (`restricted_symbol_presets`).

use crate::restricted_symbols::RestrictedSymbolEntry;

/// `[rules.banned]`: language constructs (`BannedConstruct::ALL` names)
/// and native types or classes to report wherever they're used.
#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct BannedSettings {
    pub constructs: Vec<String>,
    pub types: Vec<String>,
}

impl config::Section for BannedSettings {
    const PATH: &'static [&'static str] = &["rules", "banned"];
}

/// A construct `banned_constructs` can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BannedConstruct {
    Eval,
    Isset,
    Empty,
    Compact,
    Extract,
    Exit,
    ErrorSuppression,
    Switch,
    Goto,
    Global,
    NamedArguments,
    AssignByReference,
    ByReferenceParameters,
    ByReferenceReturns,
    ShortTernary,
    LooseComparison,
    ArrayAppend,
    Backticks,
    IgnoreComments,
    Superglobals,
    VariableVariables,
    DynamicMemberAccess,
}

impl BannedConstruct {
    pub const ALL: [(&'static str, BannedConstruct); 22] = [
        ("eval", BannedConstruct::Eval),
        ("isset", BannedConstruct::Isset),
        ("empty", BannedConstruct::Empty),
        ("compact", BannedConstruct::Compact),
        ("extract", BannedConstruct::Extract),
        ("exit", BannedConstruct::Exit),
        ("error-suppression", BannedConstruct::ErrorSuppression),
        ("switch", BannedConstruct::Switch),
        ("goto", BannedConstruct::Goto),
        ("global", BannedConstruct::Global),
        ("named-arguments", BannedConstruct::NamedArguments),
        ("assign-by-reference", BannedConstruct::AssignByReference),
        (
            "by-reference-parameters",
            BannedConstruct::ByReferenceParameters,
        ),
        ("by-reference-returns", BannedConstruct::ByReferenceReturns),
        ("short-ternary", BannedConstruct::ShortTernary),
        ("loose-comparison", BannedConstruct::LooseComparison),
        ("array-append", BannedConstruct::ArrayAppend),
        ("backticks", BannedConstruct::Backticks),
        ("ignore-comments", BannedConstruct::IgnoreComments),
        ("superglobals", BannedConstruct::Superglobals),
        ("variable-variables", BannedConstruct::VariableVariables),
        (
            "dynamic-member-access",
            BannedConstruct::DynamicMemberAccess,
        ),
    ];

    pub fn parse(name: &str) -> Result<BannedConstruct, String> {
        BannedConstruct::ALL
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, c)| *c)
            .ok_or_else(|| {
                let known: Vec<&str> = BannedConstruct::ALL.iter().map(|(n, _)| *n).collect();
                format!(
                    "unknown banned construct \"{name}\"; known: {}",
                    known.join(", ")
                )
            })
    }

    /// The diagnostic identifier, `banned.<name in camelCase>`.
    pub fn identifier(self) -> String {
        let name = BannedConstruct::ALL
            .iter()
            .find(|(_, c)| *c == self)
            .map_or("", |(n, _)| *n);
        let mut out = String::from("banned.");
        let mut upper = false;
        for c in name.chars() {
            if c == '-' {
                upper = true;
            } else if upper {
                out.push(c.to_ascii_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        }
        out
    }
}

/// Native type names `banned_types` understands, besides class names.
pub const BANNABLE_KEYWORD_TYPES: [&str; 9] = [
    "float", "int", "string", "bool", "array", "mixed", "object", "callable", "iterable",
];

/// The byte-oriented string functions with `mb_*` counterparts.
const MULTIBYTE_STRING_FUNCTIONS: [&str; 12] = [
    "strlen",
    "strpos",
    "strrpos",
    "stripos",
    "strripos",
    "substr",
    "strtolower",
    "strtoupper",
    "strstr",
    "stristr",
    "strrchr",
    "substr_count",
];

/// The `restricted_symbols` entries a named preset expands to.
pub fn restricted_symbol_preset(name: &str) -> Result<Vec<RestrictedSymbolEntry>, String> {
    match name {
        "multibyte-strings" => Ok(MULTIBYTE_STRING_FUNCTIONS
            .iter()
            .map(|function| {
                let json = format!(
                    r#"{{"kind": "function", "name": "{function}", "message": "use mb_{function}() instead; {function}() counts bytes, not characters"}}"#
                );
                serde_json::from_str(&json).expect("preset entries are well-formed")
            })
            .collect()),
        other => Err(format!(
            "unknown restricted_symbols preset \"{other}\"; known: multibyte-strings"
        )),
    }
}
