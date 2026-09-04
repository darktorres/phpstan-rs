//! Phase 72 (a-iii): the `x` (free-spacing) modifier.

use super::super::*;
use super::helpers::{fallbacks, numbers_and_names};

#[test]
fn whitespace_between_atoms_is_skipped() {
    let parsed = parse_capture_groups("/(a) (b)/x").expect("should parse");
    assert_eq!(
        numbers_and_names(&parsed.groups),
        vec![(1, None, false), (2, None, false)]
    );
}

#[test]
fn a_comment_between_atoms_is_skipped() {
    let parsed = parse_capture_groups("/(a) # a comment\n (b)/x").expect("should parse");
    assert_eq!(
        numbers_and_names(&parsed.groups),
        vec![(1, None, false), (2, None, false)]
    );
}

#[test]
fn whitespace_between_an_atom_and_its_own_quantifier_is_skipped() {
    let parsed = parse_capture_groups("/(\\d) {2,3}/x").expect("should parse");
    assert_eq!(
        fallbacks(&parsed.groups),
        vec![RegexGroupFallback::NumericString]
    );
}

#[test]
fn whitespace_around_an_alternation_bar_is_skipped_on_both_sides() {
    // Confirmed live against a real PHP interpreter: group 1 really is
    // exactly `"cat"`/`"dog"`, with *no* leftover space on either side -
    // `skip_free_spacing`'s own top-of-loop call in `scan_branch` runs
    // right before the `|`/`)`/EOF break check, so the space right
    // before `|` is consumed before that branch ever "closes", and the
    // space right after is consumed by the next branch's own first
    // iteration - neither ever becomes a literal atom.
    let parsed = parse_capture_groups("/(cat | dog)/x").expect("should parse");
    assert_eq!(numbers_and_names(&parsed.groups), vec![(1, None, false)]);
    assert_eq!(
        parsed.groups[0].literal_values,
        Some(vec!["cat".to_string(), "dog".to_string()])
    );
}

#[test]
fn whitespace_stays_literal_inside_a_character_class() {
    // Real PCRE fact: `x` never affects a `[...]` class's own interior.
    let parsed = parse_capture_groups("/([a b])/x").expect("should parse");
    assert_eq!(parsed.groups.len(), 1);
    // Not digit-only (contains a literal space and letters), but still
    // provably non-empty (a well-formed class always matches exactly
    // one character).
    assert_eq!(
        fallbacks(&parsed.groups),
        vec![RegexGroupFallback::NonEmptyString]
    );
}

#[test]
fn a_multiline_documented_pattern_parses_correctly() {
    let pattern = "/
        (\\d+)   # the leading number
        \\s*
        ([a-z]+) # the trailing word
    /x";
    let parsed = parse_capture_groups(pattern).expect("should parse");
    assert_eq!(
        numbers_and_names(&parsed.groups),
        vec![(1, None, false), (2, None, false)]
    );
    assert_eq!(
        fallbacks(&parsed.groups),
        vec![
            RegexGroupFallback::NumericString,
            RegexGroupFallback::NonEmptyString
        ]
    );
}

#[test]
fn the_j_modifier_still_bails_even_combined_with_x() {
    assert!(parse_capture_groups("/(?<x>a) (?<x>b)/xJ").is_none());
}

#[test]
fn a_pattern_without_the_x_modifier_still_treats_whitespace_as_literal() {
    // False-positive guard: `x` support must not leak into patterns that
    // never asked for it.
    let parsed = parse_capture_groups("/(a) (b)/").expect("should parse");
    assert_eq!(parsed.groups[0].literal_values, Some(vec!["a".to_string()]));
}
