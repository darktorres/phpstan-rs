//! `(?|...)` branch-reset support.

use super::super::*;
use super::helpers::numbers_and_names;

#[test]
fn a_branch_reset_where_every_branch_captures_safely_bails() {
    // The classic idiom: every branch captures something, so the same
    // public number is genuinely reused - `parse_capture_groups` bails
    // rather than risk a wrong (duplicate-keyed) shape. See its own doc
    // comment for the full reasoning, backed by live PHP verification.
    assert!(parse_capture_groups("/(?|(\\d+)|([a-z]+))/").is_none());
}

#[test]
fn a_branch_reset_where_only_one_branch_captures_resolves_with_no_bail() {
    // Only "a" ever captures anything - "b"/"c" have no groups at all,
    // so no number is ever reused.
    let parsed = parse_capture_groups("/(?|(a)|b|c)/").expect("should not bail");
    assert_eq!(numbers_and_names(&parsed.groups), vec![(1, None, true)]);
}

#[test]
fn a_group_declared_after_a_branch_reset_renumbers_from_the_highest_number_reached() {
    // `preg_match('/(?|(a)(b)|(c))(d)/', 'abd', $m)` gives
    // `$m = [0=>'abd',1=>'a',2=>'b',3=>'d']` - `(d)` really is group 3,
    // not 2, since branch 1 alone reached as high as group 2. This
    // pattern collides (both branches reach number 1), so the whole
    // parse still bails; the `next_number` restoration is exercised via
    // a non-colliding variant just below.
    assert!(parse_capture_groups("/(?|(a)(b)|(c))(d)/").is_none());
}

#[test]
fn renumbering_after_a_non_colliding_branch_reset_is_correct() {
    // Only branch "a" captures (reaching group 1); "b"/"c" don't, so
    // nothing collides, and `(d)` correctly continues from 2, not 1.
    let parsed = parse_capture_groups("/(?|(a)|b|c)(d)/").expect("should not bail");
    assert_eq!(
        numbers_and_names(&parsed.groups),
        vec![(1, None, true), (2, None, false)]
    );
}

#[test]
fn a_branch_reset_named_group_still_gets_reused_numbering() {
    let parsed = parse_capture_groups("/(?|(?<x>a)|b|c)/").expect("should not bail");
    assert_eq!(
        numbers_and_names(&parsed.groups),
        vec![(1, Some("x"), true)]
    );
}

#[test]
fn a_nested_branch_reset_group_does_not_crash_and_bails_when_it_collides() {
    assert!(parse_capture_groups("/x(?|(\\d+)|([a-z]+))y/").is_none());
}

#[test]
fn a_deeply_nested_group_inside_one_branch_reset_branch_is_unaffected_when_no_collision() {
    let parsed = parse_capture_groups("/(?|(a(b))|c)/").expect("should not bail");
    // `(a(b))` is group 1, `(b)` nested inside it is group 2 - no
    // collision (branch "c" has no groups at all).
    assert_eq!(
        numbers_and_names(&parsed.groups),
        vec![(1, None, true), (2, None, true)]
    );
}

#[test]
fn an_ordinary_pattern_with_no_branch_reset_is_completely_unaffected() {
    let parsed = parse_capture_groups("/(a)(b)/").expect("should parse");
    assert_eq!(
        numbers_and_names(&parsed.groups),
        vec![(1, None, false), (2, None, false)]
    );
}
