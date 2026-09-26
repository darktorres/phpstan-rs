//! The embedded stub sources - every vendored phpstorm-stubs file, PHPStan's
//! runtime source and its `.stub` overrides, each with its path - and the
//! checked-in symbol index generated from them (`cargo xtask stub-index`,
//! verified by the package's `stub_index` test).
//!
//! The index is one line per symbol, tab-separated:
//! `kind key name flags declarations extension parent interfaces value`.
//! - `kind`: `class`, `interface`, `trait`, `enum`, `function` or
//!   `constant`.
//! - `key`: the lookup key - a class or function name fully lowercased; a
//!   constant's namespace lowercased and its own name exact, unless it is
//!   case-insensitive (`flags` `ci`, a legacy `define(..., true)`), then
//!   fully lowercased.
//! - `declarations`: every declaring file, phpstorm-stubs (`c:`) before
//!   PHPStan's source (`s:`), each with its PHP-version gate as `@from-to`
//!   when one applies.
//! - `extension`: the phpstorm-stubs directory declaring it first.
//! - `parent`, `interfaces`: a class-like's ancestors as written in its
//!   first declaration, comma-separated.
//! - `value`: a constant's value as `=` and its source when every
//!   declaration agrees, `*` when they differ.

#![cfg_attr(not(test), deny(clippy::print_stdout, clippy::print_stderr))]

include!(concat!(env!("OUT_DIR"), "/stub_sources.rs"));

/// The checked-in symbol index.
pub static INDEX: &str = include_str!("../index.tsv");
