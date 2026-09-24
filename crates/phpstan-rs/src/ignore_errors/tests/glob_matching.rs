//! `glob_matches`'s own wildcard semantics.

use super::super::*;

#[test]
fn a_pattern_with_no_wildcard_requires_an_exact_match() {
    assert!(glob_matches(
        "tests/Unit/Db/BatchWriterTest.php",
        "tests/Unit/Db/BatchWriterTest.php"
    ));
    assert!(!glob_matches(
        "tests/Unit/Db/BatchWriterTest.php",
        "tests/Unit/Db/OtherTest.php"
    ));
}

#[test]
fn a_trailing_star_matches_everything_under_that_directory() {
    assert!(glob_matches(
        "_analysis/phpstan-latte/*",
        "_analysis/phpstan-latte/foo.php"
    ));
    // Real PHPStan's own matcher (fnmatch() without FNM_PATHNAME) lets a
    // bare `*` cross directory boundaries too - confirmed this project
    // must match that, not stop at the first `/`.
    assert!(glob_matches(
        "_analysis/phpstan-latte/*",
        "_analysis/phpstan-latte/sub/foo.php"
    ));
    assert!(!glob_matches(
        "_analysis/phpstan-latte/*",
        "_analysis/other/foo.php"
    ));
}

#[test]
fn a_star_crosses_slash_boundaries_at_any_depth() {
    assert!(glob_matches("*/Event/*", "src/Piwigo/Event/Foo.php"));
    assert!(glob_matches("*/Event/*", "a/Event/Foo.php"));
    // The pattern's own literal `/` before "Event" genuinely requires a
    // directory segment ahead of it - a bare top-level "Event/Foo.php"
    // has no `/` there to match, exactly like real `fnmatch()`.
    assert!(!glob_matches("*/Event/*", "Event/Foo.php"));
    assert!(!glob_matches("*/Event/*", "src/Piwigo/NotEvent/Foo.php"));
}

#[test]
fn a_leading_star_matches_any_prefix() {
    assert!(glob_matches("*Test.php", "tests/Unit/FooTest.php"));
    assert!(!glob_matches("*Test.php", "tests/Unit/Foo.php"));
}
