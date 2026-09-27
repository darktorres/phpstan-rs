//! slam/phpstan-extensions.

use serde::Deserialize;

/// `[ext.slam]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct SlamSettings<T = Option<bool>> {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: T,
    /// Code reading the wall clock implicitly (`currentTime.*`).
    pub check_current_time_dependence: T,
}

impl config::Section for SlamSettings {
    const PATH: &'static [&'static str] = &["ext", "slam"];
}

impl config::Extension for SlamSettings {
    const PACKAGES: &'static [&'static str] = &["slam/phpstan-extensions"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl SlamSettings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> SlamSettings<bool> {
        SlamSettings {
            enabled: active,
            check_current_time_dependence: self.check_current_time_dependence.unwrap_or(active),
        }
    }
}
