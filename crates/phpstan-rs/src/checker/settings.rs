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
    /// The platforms the code runs on. Without it, any platform PHP runs
    /// on, so a string's length and the clock have no bound below the
    /// `int` range; listing them bounds what every listed one bounds.
    ///
    /// ```toml
    /// [deployment]
    /// arch = ["x86_64", "aarch64"]
    /// os = ["linux"]
    /// ```
    pub deployment: Option<Deployment>,
}

/// `EngineSettings::deployment`: the architectures and operating systems
/// the code runs on; an empty list is any.
#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Deployment {
    pub arch: Vec<Arch>,
    pub os: Vec<Os>,
}

/// A 64-bit architecture PHP runs on.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    X86_64,
    Aarch64,
    Ppc64,
    Riscv64,
    S390x,
}

impl Arch {
    /// The size of a process's address space, which no string outgrows:
    /// 5-level paging on x86-64 and Sv57 on RISC-V give 2^56, 52-bit
    /// addresses on AArch64 and 4 PB on ppc64 give 2^52. s390x gives the
    /// whole 64 bits (`TASK_SIZE_MAX` is `-PAGE_SIZE`), so no bound.
    fn address_space(self) -> Option<i64> {
        match self {
            Arch::X86_64 | Arch::Riscv64 => Some(1 << 56),
            Arch::Aarch64 | Arch::Ppc64 => Some(1 << 52),
            Arch::S390x => None,
        }
    }
}

/// An operating system PHP runs on.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Linux,
    Windows,
    Macos,
    Freebsd,
}

impl Os {
    /// The seconds `time()` can give, least and most. Linux sets no clock
    /// before the epoch nor past `KTIME_SEC_MAX`; Windows keeps time as
    /// 100 ns ticks from 1601 in an `i64`. macOS takes any non-negative
    /// second, and FreeBSD any with `debug.allow_insane_settime`.
    fn clock(self) -> Option<(i64, i64)> {
        match self {
            Os::Linux => Some((0, i64::MAX / 1_000_000_000)),
            Os::Windows => Some((-11_644_473_600, i64::MAX / 10_000_000 - 11_644_473_600)),
            Os::Macos | Os::Freebsd => None,
        }
    }
}

/// What the deployment's platforms bound.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PlatformLimits {
    /// The most bytes a string holds.
    pub string_length: Option<i64>,
    /// The least and most seconds `time()` gives.
    pub clock: Option<(i64, i64)>,
}

impl Deployment {
    /// The bounds every listed platform keeps.
    pub fn limits(&self) -> PlatformLimits {
        let string_length = if self.arch.is_empty() {
            None
        } else {
            self.arch
                .iter()
                .map(|a| a.address_space())
                .try_fold(0, |most, space| space.map(|s| most.max(s - 1)))
        };
        let clock = if self.os.is_empty() {
            None
        } else {
            self.os
                .iter()
                .map(|o| o.clock())
                .try_fold((i64::MAX, i64::MIN), |(lo, hi), clock| {
                    clock.map(|(l, h)| (lo.min(l), hi.max(h)))
                })
        };
        PlatformLimits {
            string_length,
            clock,
        }
    }
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
    /// How the letters of configured path text compare with a path's: as the
    /// project root's directory compares names. The restricted symbols'
    /// `allow_in` zones and the include rule's guarded paths are compared so.
    pub path_case: diagnostics::CaseRule,
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
    /// What the deployment's platforms bound (`EngineSettings::deployment`).
    pub platform: PlatformLimits,
}
