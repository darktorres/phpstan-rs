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
    /// The platforms the code runs on, how much memory a process may
    /// hold, and how long it runs. Without operating systems, any PHP runs
    /// on, so the clock has no bound below the `int` range; listing them
    /// bounds it as every listed one does. A string is shorter than
    /// `memory_limit` (PHP's setting, as its ini file spells it; `-1` or
    /// unset is none), than the address space of every listed
    /// architecture, and than 2^56 bytes, which no process holds. A process
    /// runs under `max_run_seconds` (a year unless set), which bounds how
    /// far a loop counter gets.
    ///
    /// ```toml
    /// [deployment]
    /// arch = ["x86_64", "aarch64"]
    /// os = ["linux"]
    /// memory_limit = "512M"
    /// max_run_seconds = 86400
    /// ```
    pub deployment: Option<Deployment>,
}

/// `EngineSettings::deployment`: the architectures and operating systems
/// the code runs on (an empty list is any), the memory a process may hold,
/// and the longest it runs.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Deployment {
    pub arch: Vec<Arch>,
    pub os: Vec<Os>,
    pub memory_limit: Option<MemoryLimit>,
    pub max_run_seconds: i64,
}

impl Default for Deployment {
    fn default() -> Self {
        Deployment {
            arch: Vec::new(),
            os: Vec::new(),
            memory_limit: None,
            max_run_seconds: 365 * 24 * 60 * 60,
        }
    }
}

/// The most bytes a process holds: 2^56, the address space of the largest
/// machines PHP runs on, and more memory than any is built with.
const MOST_BYTES_HELD: i64 = 1 << 56;

/// PHP's `memory_limit`: a number of bytes, or none.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(try_from = "MemoryLimitSpelling")]
pub struct MemoryLimit(Option<i64>);

#[derive(Deserialize)]
#[serde(untagged)]
enum MemoryLimitSpelling {
    Bytes(i64),
    Text(String),
}

impl TryFrom<MemoryLimitSpelling> for MemoryLimit {
    type Error = String;

    /// As PHP reads its ini setting: a number of bytes, with `K`, `M` or
    /// `G` for 2^10, 2^20 or 2^30 of them; `-1` is none.
    fn try_from(spelling: MemoryLimitSpelling) -> Result<Self, String> {
        let bytes = match spelling {
            MemoryLimitSpelling::Bytes(n) => n,
            MemoryLimitSpelling::Text(text) => {
                let text = text.trim();
                let (digits, shift) = match text.chars().last().map(|c| c.to_ascii_lowercase()) {
                    Some('k') => (&text[..text.len() - 1], 10),
                    Some('m') => (&text[..text.len() - 1], 20),
                    Some('g') => (&text[..text.len() - 1], 30),
                    _ => (text, 0),
                };
                let n: i64 = digits
                    .trim()
                    .parse()
                    .map_err(|_| format!("invalid memory_limit `{text}`"))?;
                if n == -1 {
                    n
                } else {
                    n.checked_mul(1 << shift)
                        .ok_or_else(|| format!("memory_limit `{text}` is too large"))?
                }
            }
        };
        match bytes {
            -1 => Ok(MemoryLimit(None)),
            n if n > 0 => Ok(MemoryLimit(Some(n))),
            _ => Err(format!("memory_limit must be positive or -1, not {bytes}")),
        }
    }
}

/// The most updates a chain of dependent `int` operations makes in a
/// second: one a cycle at 2^34 Hz, about twice the fastest clock a CPU
/// has run at. PHP runs each loop iteration, so each takes a cycle at
/// least.
const UPDATES_PER_SECOND: i64 = 1 << 34;

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

/// What the deployment bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlatformLimits {
    /// The most bytes a string holds.
    pub string_length: i64,
    /// The least and most seconds `time()` gives.
    pub clock: Option<(i64, i64)>,
    /// The most iterations loops run in one process.
    pub loop_iterations: Option<i64>,
}

impl Default for PlatformLimits {
    fn default() -> Self {
        Deployment::default().limits()
    }
}

impl Deployment {
    /// The bounds every listed platform keeps.
    pub fn limits(&self) -> PlatformLimits {
        let address_space = if self.arch.is_empty() {
            None
        } else {
            self.arch
                .iter()
                .map(|a| a.address_space())
                .try_fold(0, |most, space| space.map(|s| most.max(s)))
        };
        let bytes = [
            Some(MOST_BYTES_HELD),
            address_space,
            self.memory_limit.and_then(|limit| limit.0),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(MOST_BYTES_HELD);
        let string_length = bytes - 1;
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
            loop_iterations: self
                .max_run_seconds
                .checked_mul(UPDATES_PER_SECOND)
                .filter(|n| *n > 0),
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
    /// What the deployment bounds (`EngineSettings::deployment`).
    pub platform: PlatformLimits,
}
