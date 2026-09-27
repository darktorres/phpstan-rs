//! voku/phpstan-rules.

use serde::Deserialize;

/// `[ext.voku]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct VokuSettings<T = Option<bool>> {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: T,
    /// A string operand combined with a non-string one, or an array with a non-array (`binaryOp.*`, `assignOp.*`).
    pub check_mixed_type_operands: T,
}

impl config::Section for VokuSettings {
    const PATH: &'static [&'static str] = &["ext", "voku"];
}

impl config::Extension for VokuSettings {
    const PACKAGES: &'static [&'static str] = &["voku/phpstan-rules"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl VokuSettings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> VokuSettings<bool> {
        VokuSettings {
            enabled: active,
            check_mixed_type_operands: self.check_mixed_type_operands.unwrap_or(active),
        }
    }
}
