//! Group numbering and naming: numeric indices, `?<name>`/`?'name'`, optionality, and malformed/bailing patterns.

use super::super::*;
use super::helpers::*;
#[test]
fn bare_numbered_groups_are_required() {
    let groups = parse_capture_groups(r"/(\d+)-(\d+)/").unwrap().groups;
    assert_eq!(
        numbers_and_names(&groups),
        vec![(1, None, false), (2, None, false)]
    );
}

#[test]
fn named_groups_capture_both_number_and_name() {
    let groups = parse_capture_groups(r"/(?<year>\d+)-(?<month>\d+)/")
        .unwrap()
        .groups;
    assert_eq!(
        numbers_and_names(&groups),
        vec![(1, Some("year"), false), (2, Some("month"), false)]
    );
}

#[test]
fn optional_group_via_question_mark() {
    let groups = parse_capture_groups("/(a)(b)?/").unwrap().groups;
    assert_eq!(
        numbers_and_names(&groups),
        vec![(1, None, false), (2, None, true)]
    );
}

#[test]
fn optional_group_via_alternation() {
    let groups = parse_capture_groups("/(a)|(b)/").unwrap().groups;
    assert_eq!(
        numbers_and_names(&groups),
        vec![(1, None, true), (2, None, true)]
    );
}

#[test]
fn optionality_propagates_through_a_non_capturing_wrapper() {
    let groups = parse_capture_groups("/(?:(a))?/").unwrap().groups;
    assert_eq!(numbers_and_names(&groups), vec![(1, None, true)]);
}

#[test]
fn p_angle_and_quote_named_group_syntax_are_equivalent() {
    let angle = parse_capture_groups("/(?P<x>a)/").unwrap().groups;
    let quote = parse_capture_groups("/(?'x'a)/").unwrap().groups;
    assert_eq!(numbers_and_names(&angle), vec![(1, Some("x"), false)]);
    assert_eq!(numbers_and_names(&quote), vec![(1, Some("x"), false)]);
}

#[test]
fn a_leading_bracket_and_a_posix_class_dont_miscount_group_boundaries() {
    let groups = parse_capture_groups("/[]a[:digit:]]+/").unwrap().groups;
    assert!(groups.is_empty());
}

#[test]
fn a_malformed_posix_looking_construct_nested_in_a_class_falls_back_to_literal() {
    // The `[:` lookahead only ever triggers on an inner `[` while a
    // class is already open, since real POSIX syntax `[:name:]` is
    // itself bracket-delimited - a bare top-level `/[:foo](x)/` never
    // reaches this code path at all.
    let groups = parse_capture_groups("/[[:foo](x)/").unwrap().groups;
    assert_eq!(numbers_and_names(&groups), vec![(1, None, false)]);
}

#[test]
fn q_e_literal_block_does_not_open_a_group() {
    let groups = parse_capture_groups(r"/\Q(\E/").unwrap().groups;
    assert!(groups.is_empty());
}

#[test]
fn a_literal_brace_not_forming_a_quantifier_is_plain_text() {
    let groups = parse_capture_groups("/a{b/").unwrap().groups;
    assert!(groups.is_empty());
}

#[test]
fn duplicate_names_on_alternation_branches_dont_crash() {
    let groups = parse_capture_groups("/(?<x>a)|(?<x>b)/").unwrap().groups;
    assert_eq!(
        numbers_and_names(&groups),
        vec![(1, Some("x"), true), (2, Some("x"), true)]
    );
}

#[test]
fn a_conditional_group_bails() {
    assert!(parse_capture_groups("/(?(1)a|b)/").is_none());
}

#[test]
fn the_j_modifier_still_bails_but_x_no_longer_does() {
    // Phase 72: `x` (free-spacing mode) is now supported - it only
    // changes tokenization, which `Scanner::skip_free_spacing` handles
    // inline - so a pattern that's otherwise fully understood no longer
    // bails just for carrying it. `J` (duplicate named groups) still
    // bails; see `regex_groups::tests::x_modifier` for `x`'s own
    // dedicated coverage.
    assert!(parse_capture_groups("/(a)/x").is_some());
    assert!(parse_capture_groups("/(a)/J").is_none());
}

#[test]
fn malformed_patterns_bail() {
    assert!(parse_capture_groups("/(a/").is_none());
    assert!(parse_capture_groups("/").is_none());
    assert!(parse_capture_groups("").is_none());
}

#[test]
fn delimiter_variety_all_strip_correctly() {
    assert!(parse_capture_groups("/(a)/").is_some());
    assert!(parse_capture_groups("#(a)#").is_some());
    assert!(parse_capture_groups("~(a)~").is_some());
    assert!(parse_capture_groups("(a)").is_some());
    assert!(parse_capture_groups("{a}").is_some());
    assert!(parse_capture_groups("[a]").is_some());
    assert!(parse_capture_groups("<a>").is_some());
}

#[test]
fn the_real_qdaterangescope_pattern_produces_the_expected_shape() {
    // src/Piwigo/Search/QDateRangeScope.php:51 - a year/month/day
    // date parser. Group 1 (year) required; groups 2 and 3
    // (month/day) each optional via their own trailing `?`.
    let groups = parse_capture_groups(
        "/([0-9]{4})-?((?:1[0-2])|(?:0?[1-9]))?-?((?:(?:[1-3][0-9])|(?:0?[1-9])))?/",
    )
    .unwrap()
    .groups;
    assert_eq!(
        numbers_and_names(&groups),
        vec![(1, None, false), (2, None, true), (3, None, true)]
    );
}

#[test]
fn pathological_input_does_not_hang_or_overflow() {
    let long_flat = format!("/{}/", "a".repeat(5000));
    assert!(parse_capture_groups(&long_flat).is_some());

    let mut deeply_nested = String::from("/");
    for _ in 0..500 {
        deeply_nested.push_str("(?:");
    }
    deeply_nested.push('a');
    for _ in 0..500 {
        deeply_nested.push(')');
    }
    deeply_nested.push('/');
    // Exceeds MAX_GROUP_DEPTH - must bail cleanly, not overflow.
    assert!(parse_capture_groups(&deeply_nested).is_none());
}
