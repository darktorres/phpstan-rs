//! Every case here was checked against a live PHP 8.5 interpreter
//! (`php -r`) before being pinned - see this module's own doc comment.

use super::validate_pattern;

#[test]
fn a_well_formed_pattern_is_valid() {
    assert_eq!(validate_pattern("/foo/i"), None);
    assert_eq!(validate_pattern("#foo#"), None);
    assert_eq!(validate_pattern("(foo)i"), None);
    assert_eq!(validate_pattern("{foo}"), None);
}

#[test]
fn an_alphanumeric_or_backslash_or_nul_delimiter_is_rejected() {
    for pattern in ["1abc1", "aabca"] {
        assert!(validate_pattern(pattern).is_some(), "for {pattern:?}");
    }
    assert!(validate_pattern("\\abc\\").is_some());
}

#[test]
fn a_missing_closing_delimiter_is_rejected() {
    assert!(validate_pattern("/abc").is_some());
}

#[test]
fn an_unknown_modifier_is_rejected() {
    assert!(validate_pattern("/abc/y").is_some());
}

#[test]
fn every_real_php_modifier_is_accepted() {
    for m in [
        'i', 'm', 'n', 'r', 's', 'u', 'x', 'A', 'D', 'J', 'S', 'U', 'X',
    ] {
        assert_eq!(
            validate_pattern(&format!("/x/{m}")),
            None,
            "for modifier {m}"
        );
    }
}

#[test]
fn a_genuinely_unclosed_group_fails_the_real_pcre2_compile_step() {
    let err = validate_pattern("/(unclosed/");
    assert!(err.is_some());
    assert!(
        err.as_deref().unwrap().contains("Compilation failed"),
        "got: {err:?}"
    );
}

#[test]
fn duplicate_named_groups_require_the_j_modifier() {
    assert!(validate_pattern("/(?<a>x)|(?<a>y)/").is_some());
    assert_eq!(validate_pattern("/(?<a>x)|(?<a>y)/J"), None);
}
