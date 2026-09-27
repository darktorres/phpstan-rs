//! dave-liddament/phpstan-php-language-extensions: the access attributes
//! (`Friend`, `NamespaceVisibility`, `TestTag`, `RestrictTraitTo`,
//! `InjectableVersion`, `MustUseResult`, ...).

use serde::Deserialize;

/// `[ext.language-extensions]`.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct LanguageExtensionsSettings {
    /// Whether the extension is active; unset, whether one of its
    /// packages is installed.
    pub enabled: Option<bool>,
}

impl config::Section for LanguageExtensionsSettings {
    const PATH: &'static [&'static str] = &["ext", "language-extensions"];
}

impl config::Extension for LanguageExtensionsSettings {
    const PACKAGES: &'static [&'static str] = &["dave-liddament/phpstan-php-language-extensions"];

    fn enabled(&self) -> Option<bool> {
        self.enabled
    }
}
