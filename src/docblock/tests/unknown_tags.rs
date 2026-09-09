//! Tests for `unknown_phpstan_tags` - real PHPStan's own
//! `InvalidPHPStanDocTagRule` whitelist, transcribed.
use super::super::*;

#[test]
fn a_known_tag_is_never_flagged() {
    assert_eq!(
        unknown_phpstan_tags("/** @phpstan-var int $x */"),
        Vec::<String>::new()
    );
}

#[test]
fn a_typo_d_tag_is_flagged() {
    assert_eq!(
        unknown_phpstan_tags("/** @phpstan-parm int $x */"),
        vec!["parm".to_string()]
    );
}

#[test]
fn multiple_unknown_tags_are_all_reported() {
    assert_eq!(
        unknown_phpstan_tags(
            r#"/**
             * @phpstan-parm int $x
             * @phpstan-retrn int
             */"#
        ),
        vec!["parm".to_string(), "retrn".to_string()]
    );
}

#[test]
fn a_bare_non_phpstan_tag_is_never_flagged() {
    assert_eq!(
        unknown_phpstan_tags("/** @param int $x */"),
        Vec::<String>::new()
    );
}

#[test]
fn a_doc_with_no_tags_at_all_is_never_flagged() {
    assert_eq!(
        unknown_phpstan_tags("/** Just a plain description. */"),
        Vec::<String>::new()
    );
}
