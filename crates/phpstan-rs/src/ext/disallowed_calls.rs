//! spaze/phpstan-disallowed-calls: declarative restricted symbols.

use serde::Deserialize;

use crate::restricted_symbols::RestrictedSymbolEntry;

/// `[ext.disallowed-calls]`:
///
/// ```toml
/// [[ext.disallowed-calls.restricted_symbols]]
/// kind = "staticMethod"
/// class = "Legacy\\Db"
/// name = "query"
/// message = "use the Db\\Connection service instead"
/// ```
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct DisallowedCallsSettings {
    pub restricted_symbols: Vec<RestrictedSymbolEntry>,
    /// Built-in sets to add (`multibyte-strings`).
    pub restricted_symbol_presets: Vec<String>,
}

impl config::Section for DisallowedCallsSettings {
    const PATH: &'static [&'static str] = &["ext", "disallowed-calls"];
}
