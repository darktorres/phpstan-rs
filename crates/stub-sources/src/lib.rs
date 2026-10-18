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

#[cfg(test)]
mod embed;

#[cfg(test)]
mod tests {
    use super::embed::{slash_spelling, with_lf_breaks};
    use super::*;
    use std::path::Path;

    #[test]
    fn a_path_is_spelled_with_slashes() {
        let relative = Path::new("standard").join("sub").join("basic.php");
        assert_eq!(slash_spelling(&relative), "standard/sub/basic.php");
        assert_eq!(slash_spelling(Path::new("basic.php")), "basic.php");
        assert_eq!(slash_spelling(Path::new("")), "");
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_is_spelled_with_slashes() {
        assert_eq!(
            slash_spelling(Path::new(r"standard\sub\basic.php")),
            "standard/sub/basic.php"
        );
        assert_eq!(
            slash_spelling(Path::new("standard/sub\\basic.php")),
            "standard/sub/basic.php"
        );
    }

    #[test]
    fn a_source_written_with_crlf_is_embedded_with_lf() {
        assert_eq!(with_lf_breaks(b"a\r\nb\r\n"), Some(b"a\nb\n".to_vec()));
        assert_eq!(with_lf_breaks(b"a\n\r\nb\r"), Some(b"a\n\nb\r".to_vec()));
        assert_eq!(with_lf_breaks(b"\r\r\n"), Some(b"\r\n".to_vec()));
        assert_eq!(with_lf_breaks(b"a\nb\n"), None);
        assert_eq!(with_lf_breaks(b"a\rb"), None);
        assert_eq!(with_lf_breaks(b"a\r"), None);
        assert_eq!(with_lf_breaks(b""), None);
    }

    #[test]
    fn the_embedded_paths_and_sources_are_what_every_platform_embeds() {
        assert!(!PHPSTORM_STUBS_SOURCES.is_empty() && !PHPSTAN_SRC_SOURCES.is_empty());
        assert_eq!(PHPSTORM_STUBS_PATHS.len(), PHPSTORM_STUBS_SOURCES.len());
        assert_eq!(
            PHPSTORM_STUBS_EXTENSIONS.len(),
            PHPSTORM_STUBS_SOURCES.len()
        );
        assert_eq!(PHPSTAN_SRC_PATHS.len(), PHPSTAN_SRC_SOURCES.len());
        for path in PHPSTORM_STUBS_PATHS.iter().chain(PHPSTAN_SRC_PATHS) {
            assert!(!path.contains('\\'), "{path}");
            assert!(path.ends_with(".php"), "{path}");
        }
        let sources = PHPSTORM_STUBS_SOURCES
            .iter()
            .chain(PHPSTAN_SRC_SOURCES)
            .chain(PHPSTAN_STUB_SOURCES);
        for source in sources {
            assert!(!source.contains("\r\n"));
        }
    }
}
