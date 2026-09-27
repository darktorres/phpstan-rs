//! pestphp/pest-plugin-phpstan: `$this` inside Pest's test closures.

use serde::Deserialize;

/// `[ext.pest]`.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct PestSettings {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: Option<bool>,
}

impl config::Section for PestSettings {
    const PATH: &'static [&'static str] = &["ext", "pest"];
}

impl config::Extension for PestSettings {
    const PACKAGES: &'static [&'static str] = &["pestphp/pest-plugin-phpstan"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}
