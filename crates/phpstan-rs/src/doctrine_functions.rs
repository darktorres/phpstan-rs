//! A purely declarative, config-driven list of a project's own custom
//! DQL function registrations - a
//! `Doctrine\ORM\Query\AST\Functions\FunctionNode` subclass registered
//! via `Configuration::addCustomStringFunction()`/
//! `addCustomNumericFunction()`/`addCustomDatetimeFunction()` (e.g.
//! `DATE_FORMAT_YEAR_MONTH`) has no return type this project can
//! derive from the DQL parser alone - **this is true of real PHPStan's
//! own `phpstan-doctrine` too**: it requires the identical shape of
//! manually-declared config (`customNumericFunctions`/
//! `customStringFunctions`/`customDatetimeFunctions` in a project's own
//! `doctrine.neon`) precisely because it can't infer this either, so
//! mirroring that with a small, declarative config surface here (rather
//! than skipping these functions, or guessing) matches the real
//! reference implementation's own architecture, not just its precision.
//!
//! Config-file-only (no CLI flag) - a structured list doesn't fit a
//! boolean/string CLI flag, the same reasoning `Config::ignore_errors`/
//! `restricted_symbols` already established.

use serde::Deserialize;

/// Which of DQL's three custom-function categories a project's own
/// registration belongs to - mirrors real `phpstan-doctrine`'s own
/// `customNumericFunctions`/`customStringFunctions`/
/// `customDatetimeFunctions` config keys exactly, including their own
/// per-category result type: `Numeric` resolves to `int|float` (real
/// SQL numeric expressions are driver/value-dependent between the two,
/// matching how this project's own aggregate-function support already
/// leaves `SUM`/`AVG` to the operand's own type rather than picking
/// one); `String`/`Datetime` both resolve to plain `string` - a DQL
/// scalar result is always a raw, unhydrated database value, never a
/// real `DateTime` object, regardless of which category a function's
/// own semantic *meaning* falls into.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum DoctrineFunctionKind {
    Numeric,
    String,
    Datetime,
}

/// One `[[doctrine_custom_functions]]` entry.
///
/// ```toml
/// [[doctrine_custom_functions]]
/// name = "DATE_FORMAT_YEAR_MONTH"
/// kind = "string"
/// ```
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DoctrineCustomFunctionEntry {
    /// The DQL function name exactly as registered (case-insensitive at
    /// lookup time, matching real Doctrine's own DQL parser).
    pub name: String,
    pub kind: DoctrineFunctionKind,
}
