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

#[test]
fn a_path_pattern_reads_a_backslash_as_a_separator_wherever_it_stands() {
    assert!(path_glob_matches(
        "src\\Legacy\\Foo.php",
        "src/Legacy/Foo.php",
        CaseRule::Exact
    ));
    assert!(path_glob_matches(
        "src\\*",
        "src/Legacy/Foo.php",
        CaseRule::Exact
    ));
    assert!(path_glob_matches(
        "*\\Event\\*",
        "src/Piwigo/Event/Foo.php",
        CaseRule::Exact
    ));
    assert!(path_glob_matches(
        "*\\Foo.php",
        "src/Legacy/Foo.php",
        CaseRule::Exact
    ));
    assert!(path_glob_matches("*\\é\\*", "a/é/b.php", CaseRule::Exact));
    assert!(!path_glob_matches(
        "*\\Event\\*",
        "src/Piwigo/NotEvent/Foo.php",
        CaseRule::Exact
    ));
    assert!(!path_glob_matches(
        "src\\*",
        "srcx/Foo.php",
        CaseRule::Exact
    ));
    assert!(!path_glob_matches("src/*", "src\\Foo.php", CaseRule::Exact));
}

#[test]
fn a_name_pattern_keeps_a_backslash_as_it_is() {
    assert!(glob_matches("app\\models\\*", "app\\models\\user"));
    assert!(!glob_matches("app\\models\\*", "app/models/user"));
}

#[test]
fn a_folded_rule_matches_a_pattern_whatever_the_case_of_its_letters() {
    assert!(path_glob_matches("Src/*", "src/Foo.php", CaseRule::Folded));
    assert!(path_glob_matches("src/*", "SRC/foo.php", CaseRule::Folded));
    assert!(path_glob_matches(
        "*\\Event\\*.PHP",
        "src/event/foo.php",
        CaseRule::Folded
    ));
    assert!(path_glob_matches("é/*", "É/a.php", CaseRule::Folded));
    assert!(!path_glob_matches("Src/*", "lib/Foo.php", CaseRule::Folded));
    assert!(!path_glob_matches(
        "src/*",
        "srcx/Foo.php",
        CaseRule::Folded
    ));
}

#[test]
fn an_exact_rule_matches_a_pattern_by_the_case_of_its_letters_only() {
    assert!(!path_glob_matches("Src/*", "src/Foo.php", CaseRule::Exact));
    assert!(path_glob_matches("Src/*", "Src/Foo.php", CaseRule::Exact));
}
