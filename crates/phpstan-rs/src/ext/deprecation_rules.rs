//! phpstan/phpstan-deprecation-rules.

use serde::Deserialize;

/// `[ext.deprecation-rules]`.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct DeprecationRulesSettings {
    /// Reports uses of deprecated symbols, directives and syntax; unset,
    /// whether one of the extension's packages is installed.
    pub enabled: Option<bool>,
}

impl config::Section for DeprecationRulesSettings {
    const PATH: &'static [&'static str] = &["ext", "deprecation-rules"];
}

impl config::Extension for DeprecationRulesSettings {
    const PACKAGES: &'static [&'static str] = &["phpstan/phpstan-deprecation-rules"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}
