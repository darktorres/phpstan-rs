//! sidz/phpstan-rules: magic numbers.

use serde::Deserialize;

/// `[ext.sidz]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct SidzSettings<T = Option<bool>> {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: T,
    /// A bare numeric literal outside a constant declaration, in any of 13
    /// positions (`magicNumber.*`).
    pub check_magic_numbers: T,
    /// Numbers `check_magic_numbers` accepts anywhere (default `[0, 1]`).
    pub magic_number_exemptions: Option<Vec<ConfigNumber>>,
    /// Whether `check_magic_numbers` leaves numeric strings alone.
    pub magic_numbers_ignore_numeric_strings: bool,
}

impl config::Section for SidzSettings {
    const PATH: &'static [&'static str] = &["ext", "sidz"];
}

impl config::Extension for SidzSettings {
    const PACKAGES: &'static [&'static str] = &["sidz/phpstan-rules"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl SidzSettings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> SidzSettings<bool> {
        SidzSettings {
            enabled: active,
            check_magic_numbers: self.check_magic_numbers.unwrap_or(active),
            magic_number_exemptions: self.magic_number_exemptions,
            magic_numbers_ignore_numeric_strings: self.magic_numbers_ignore_numeric_strings,
        }
    }
}

/// A TOML integer or float.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum ConfigNumber {
    Int(i64),
    Float(f64),
}

impl ConfigNumber {
    pub fn value(self) -> f64 {
        match self {
            ConfigNumber::Int(i) => i as f64,
            ConfigNumber::Float(f) => f,
        }
    }
}
