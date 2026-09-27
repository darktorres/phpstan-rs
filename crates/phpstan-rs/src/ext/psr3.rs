//! PSR-3 logging (staabm/phpstan-psr3, sfp-phpstan-psr-log).

use serde::Deserialize;

/// `[ext.psr3]`: each check defaults to whether the extension is active.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Psr3Settings<T = Option<bool>> {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: T,
    /// Logger call hygiene: placeholders, message, level and context
    /// (`psr3.*`).
    pub check_psr3_logging: T,
    /// A logger call without an `exception` context key while a
    /// Throwable variable is in scope (`psr3.contextMissingException`).
    pub check_psr3_exception_context: T,
    /// A regular expression (Rust `regex` syntax) every context key must
    /// match, under `check_psr3_logging`.
    pub psr3_context_key_pattern: Option<String>,
}

impl config::Section for Psr3Settings {
    const PATH: &'static [&'static str] = &["ext", "psr3"];
}

impl config::Extension for Psr3Settings {
    const PACKAGES: &'static [&'static str] = &[
        "staabm/phpstan-psr3",
        "struggle-for-php/sfp-phpstan-psr-log",
    ];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}

impl Psr3Settings {
    /// Each check resolved against whether the extension is `active`.
    pub fn resolve(self, active: bool) -> Psr3Settings<bool> {
        Psr3Settings {
            enabled: active,
            check_psr3_logging: self.check_psr3_logging.unwrap_or(active),
            check_psr3_exception_context: self.check_psr3_exception_context.unwrap_or(active),
            psr3_context_key_pattern: self.psr3_context_key_pattern,
        }
    }
}

impl<T> Psr3Settings<T> {
    /// `psr3_context_key_pattern` compiled.
    pub fn context_key_pattern(&self) -> Result<Option<regex::Regex>, String> {
        self.psr3_context_key_pattern
            .as_deref()
            .map(regex::Regex::new)
            .transpose()
            .map_err(|e| format!("invalid ext.psr3.psr3_context_key_pattern: {e}"))
    }
}
