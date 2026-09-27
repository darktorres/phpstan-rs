//! The extensions' own config sections, one module per extension crate
//! to be, and which extensions are active.
//!
//! An extension is active when its table says `enabled = true`, or, with
//! `enabled` unset, when one of its packages is installed in the project's
//! composer root, as PHPStan's extension installer would load it. An
//! active extension's checks default to on; each can still be turned off
//! in its table.

pub mod deprecation_rules;
pub mod disallowed_calls;
pub mod doctrine;
pub mod extras;
pub mod language_extensions;
pub mod mockstan;
pub mod pest;
pub mod phpunit;
pub mod psr3;
pub mod shipmonk;
pub mod sidz;
pub mod slam;
pub mod sql;
pub mod strict_rules;
pub mod voku;

use config::ConfigFile;

/// Which extensions are active.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ActiveExtensions {
    pub extras: bool,
    pub strict_rules: bool,
    pub deprecation_rules: bool,
    pub shipmonk: bool,
    pub voku: bool,
    pub slam: bool,
    pub psr3: bool,
    pub sidz: bool,
    pub phpunit: bool,
    pub pest: bool,
    pub mockstan: bool,
    pub doctrine: bool,
    pub language_extensions: bool,
}

impl ActiveExtensions {
    /// Every extension's activation in `config`, `installed` telling
    /// whether a package is installed.
    pub fn resolve(
        config: &ConfigFile,
        installed: &dyn Fn(&str) -> bool,
    ) -> Result<ActiveExtensions, String> {
        fn active<E: config::Extension>(
            config: &ConfigFile,
            installed: &dyn Fn(&str) -> bool,
        ) -> Result<bool, String> {
            Ok(config.section::<E>()?.is_active(installed))
        }
        Ok(ActiveExtensions {
            extras: active::<extras::ExtrasSettings>(config, installed)?,
            strict_rules: active::<strict_rules::StrictRulesSettings>(config, installed)?,
            deprecation_rules: active::<deprecation_rules::DeprecationRulesSettings>(
                config, installed,
            )?,
            shipmonk: active::<shipmonk::ShipmonkSettings>(config, installed)?,
            voku: active::<voku::VokuSettings>(config, installed)?,
            slam: active::<slam::SlamSettings>(config, installed)?,
            psr3: active::<psr3::Psr3Settings>(config, installed)?,
            sidz: active::<sidz::SidzSettings>(config, installed)?,
            phpunit: active::<phpunit::PhpunitSettings>(config, installed)?,
            pest: active::<pest::PestSettings>(config, installed)?,
            mockstan: active::<mockstan::MockstanSettings>(config, installed)?,
            doctrine: active::<doctrine::DoctrineSettings>(config, installed)?,
            language_extensions: active::<language_extensions::LanguageExtensionsSettings>(
                config, installed,
            )?,
        })
    }
}

/// Whether `package` is installed in `manifest`'s root; none is without
/// one.
pub fn installed_in(
    manifest: Option<&composer::manifest::ComposerManifest>,
    package: &str,
) -> bool {
    manifest.is_some_and(|m| m.installed_package(package).is_some())
}

#[cfg(test)]
mod tests;
