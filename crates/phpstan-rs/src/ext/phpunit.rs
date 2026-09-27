//! phpstan/phpstan-phpunit and the Pest equivalent.

use serde::Deserialize;

/// `[ext.phpunit]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct PhpunitSettings<T = Option<bool>> {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: T,
    /// A test with no assertion (`test.risky`).
    pub check_risky_tests: T,
}

impl config::Section for PhpunitSettings {
    const PATH: &'static [&'static str] = &["ext", "phpunit"];
}

impl config::Extension for PhpunitSettings {
    const PACKAGES: &'static [&'static str] = &["phpstan/phpstan-phpunit"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl PhpunitSettings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> PhpunitSettings<bool> {
        PhpunitSettings {
            enabled: active,
            check_risky_tests: self.check_risky_tests.unwrap_or(active),
        }
    }
}
