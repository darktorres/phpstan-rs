//! shipmonk/phpstan-rules.

use serde::Deserialize;

/// `[ext.shipmonk]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ShipmonkSettings<T = Option<bool>> {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: T,
    /// `<`/`>`/`<=`/`>=`/`<=>` only between comparable types (`comparison.*`).
    pub check_comparable_types: T,
    /// `isset()`/`empty()` on objects, and possibly-null values silently coerced.
    pub check_null_coercion: T,
    /// Enum branching that defeats `match` exhaustiveness.
    pub check_enum_exhaustiveness: T,
}

impl config::Section for ShipmonkSettings {
    const PATH: &'static [&'static str] = &["ext", "shipmonk"];
}

impl config::Extension for ShipmonkSettings {
    const PACKAGES: &'static [&'static str] = &["shipmonk/phpstan-rules"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl ShipmonkSettings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> ShipmonkSettings<bool> {
        ShipmonkSettings {
            enabled: active,
            check_comparable_types: self.check_comparable_types.unwrap_or(active),
            check_null_coercion: self.check_null_coercion.unwrap_or(active),
            check_enum_exhaustiveness: self.check_enum_exhaustiveness.unwrap_or(active),
        }
    }
}
