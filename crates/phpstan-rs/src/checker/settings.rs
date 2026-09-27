//! The engine's keys, and what the checks are configured with.

use serde::Deserialize;

/// Top-level keys.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct EngineSettings {
    /// PHPStan's `dynamicConstantNames`: global (`NAME`) or class
    /// (`Class::NAME`) constants whose declared value differs between
    /// environments, so their literal value is never trusted. A plain list
    /// reads each one as its value's general type; a table gives the type
    /// explicitly, and a declared value it doesn't accept is reported; a
    /// list of entries mixes both.
    ///
    /// ```toml
    /// dynamic_constant_names = ["DEBUG", "App\\Settings::LEVEL"]
    /// # or
    /// [dynamic_constant_names]
    /// DEBUG = "bool"
    /// # or
    /// dynamic_constant_names = [{ name = "DEBUG" }, { name = "MODE", type = "'dev'|'prod'" }]
    /// ```
    pub dynamic_constant_names: Option<DynamicConstantNames>,
}

impl config::Section for EngineSettings {
    const PATH: &'static [&'static str] = &[];
}

/// `EngineSettings::dynamic_constant_names`' accepted shapes: a list of
/// names, a name-to-type table, or a list of entries mixing both.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DynamicConstantNames {
    Names(Vec<String>),
    Typed(std::collections::BTreeMap<String, String>),
    Entries(Vec<DynamicConstantEntry>),
}

/// One `dynamic_constant_names` entry: a constant (`NAME` or
/// `Class::NAME`), optionally with the type its value has.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DynamicConstantEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Option<String>,
}

impl DynamicConstantNames {
    /// Every entry as `(name, explicit type)`.
    pub fn entries(&self) -> Vec<(String, Option<String>)> {
        match self {
            DynamicConstantNames::Names(names) => names.iter().map(|n| (n.clone(), None)).collect(),
            DynamicConstantNames::Typed(map) => map
                .iter()
                .map(|(n, t)| (n.clone(), Some(t.clone())))
                .collect(),
            DynamicConstantNames::Entries(entries) => entries
                .iter()
                .map(|e| (e.name.clone(), e.ty.clone()))
                .collect(),
        }
    }
}

impl EngineSettings {
    /// `dynamic_constant_names` as `(name, explicit type)` entries.
    pub fn dynamic_constant_entries(&self) -> Vec<(String, Option<String>)> {
        self.dynamic_constant_names
            .as_ref()
            .map(DynamicConstantNames::entries)
            .unwrap_or_default()
    }
}

/// What the checks are configured with: each owner's settings, every
/// extension's checks resolved against whether it's active.
#[derive(Debug, Default)]
pub struct CheckSettings {
    pub rule_levels: crate::checker::rule_level_helper::RuleLevelHelper,
    pub constants: crate::checker::constant_resolver::ConstantResolver,
    pub rules: crate::rules_settings::RulesSettings,
    /// Whether the project consumes PHPStan as a library, which the
    /// PHPStan-API rules apply to.
    pub phpstan_api: bool,
    pub extras: crate::ext::extras::ExtrasSettings<bool>,
    pub strict_rules: crate::ext::strict_rules::StrictRulesSettings<bool>,
    pub shipmonk: crate::ext::shipmonk::ShipmonkSettings<bool>,
    pub voku: crate::ext::voku::VokuSettings<bool>,
    pub slam: crate::ext::slam::SlamSettings<bool>,
    pub psr3: crate::ext::psr3::Psr3Settings<bool>,
    /// `psr3.psr3_context_key_pattern`, compiled.
    pub psr3_context_key_pattern: Option<regex::Regex>,
    pub sidz: crate::ext::sidz::SidzSettings<bool>,
    pub phpunit: crate::ext::phpunit::PhpunitSettings<bool>,
    /// Whether phpstan/phpstan-deprecation-rules is active.
    pub deprecation_rules: bool,
    pub restricted_symbols: crate::restricted_symbols::RestrictedSymbols,
    /// Whether phpstan/phpstan-doctrine is active: Doctrine ORM/DBAL
    /// result types are inferred only then.
    pub doctrine: bool,
    /// The project's DQL functions: name (lowercased, as DQL looks
    /// functions up case-insensitively) to category.
    pub doctrine_custom_functions:
        rustc_hash::FxHashMap<String, crate::doctrine_functions::DoctrineFunctionKind>,
    /// Whether the php-language-extensions extension is active: its
    /// access attributes are enforced only then.
    pub language_extensions: bool,
    /// Whether the Pest extension is active: Pest's test closures bind
    /// `$this` only then.
    pub pest: bool,
}
