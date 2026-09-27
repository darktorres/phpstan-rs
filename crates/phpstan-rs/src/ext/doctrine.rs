//! phpstan/phpstan-doctrine.

use serde::Deserialize;

use crate::doctrine_functions::DoctrineCustomFunctionEntry;

/// `[ext.doctrine]`: a project's own DQL function registrations.
///
/// ```toml
/// [[ext.doctrine.custom_functions]]
/// name = "DATE_FORMAT_YEAR_MONTH"
/// kind = "string"
/// ```
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct DoctrineSettings {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: Option<bool>,
    pub custom_functions: Vec<DoctrineCustomFunctionEntry>,
}

impl config::Section for DoctrineSettings {
    const PATH: &'static [&'static str] = &["ext", "doctrine"];
}

impl config::Extension for DoctrineSettings {
    const PACKAGES: &'static [&'static str] = &["phpstan/phpstan-doctrine"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}
