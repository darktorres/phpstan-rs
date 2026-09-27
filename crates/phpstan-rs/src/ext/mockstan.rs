//! PHPUnit mock-builder hygiene.

use serde::Deserialize;

/// `[ext.mockstan]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct MockstanSettings<T = Option<bool>> {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: T,
    /// Mock-builder misuse (`mock.*`).
    pub check_mock_hygiene: T,
    /// Classes tests must not mock (`mock.bannedClass`).
    pub mock_banned_classes: Vec<String>,
}

impl config::Section for MockstanSettings {
    const PATH: &'static [&'static str] = &["ext", "mockstan"];
}

impl config::Extension for MockstanSettings {
    const PACKAGES: &'static [&'static str] = &["rector/mockstan"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl MockstanSettings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> MockstanSettings<bool> {
        MockstanSettings {
            enabled: active,
            check_mock_hygiene: self.check_mock_hygiene.unwrap_or(active),
            mock_banned_classes: self.mock_banned_classes,
        }
    }
}
