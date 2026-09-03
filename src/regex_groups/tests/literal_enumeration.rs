//! Literal enumeration: which capture groups can be reduced to a finite set of literal string values.

use super::super::*;
use super::helpers::*;
#[test]
fn a_plain_literal_group_is_fully_enumerable() {
    let groups = parse_capture_groups("/(cat)/").unwrap().groups;
    assert_eq!(literals(&groups), vec![Some(vec!["cat".to_string()])]);
}

#[test]
fn an_alternation_of_literals_unions_their_values() {
    let groups = parse_capture_groups("/(cat|dog)/").unwrap().groups;
    assert_eq!(
        literals(&groups),
        vec![Some(vec!["cat".to_string(), "dog".to_string()])]
    );
}

#[test]
fn q_e_contributes_its_own_verbatim_text() {
    let groups = parse_capture_groups(r"/(\Qa.b\E)/").unwrap().groups;
    assert_eq!(literals(&groups), vec![Some(vec!["a.b".to_string()])]);
}

#[test]
fn an_escaped_punctuation_character_is_literal() {
    let groups = parse_capture_groups(r"/(a\.b)/").unwrap().groups;
    assert_eq!(literals(&groups), vec![Some(vec!["a.b".to_string()])]);
}

#[test]
fn a_bounded_quantifier_repeats_a_literal_atom() {
    let groups = parse_capture_groups("/(a{2})/").unwrap().groups;
    assert_eq!(literals(&groups), vec![Some(vec!["aa".to_string()])]);
}

#[test]
fn a_bounded_range_quantifier_enumerates_every_repeat_count() {
    let groups = parse_capture_groups("/(a{1,2})/").unwrap().groups;
    let mut values = literals(&groups)[0].clone().unwrap();
    values.sort();
    assert_eq!(values, vec!["a".to_string(), "aa".to_string()]);
}

#[test]
fn the_i_modifier_disables_literal_tracking_but_not_group_counting() {
    let groups = parse_capture_groups("/(cat)/i").unwrap().groups;
    assert_eq!(numbers_and_names(&groups), vec![(1, None, false)]);
    assert_eq!(literals(&groups), vec![None]);
}

#[test]
fn a_character_class_is_never_literal_tracked() {
    let groups = parse_capture_groups("/([a])/").unwrap().groups;
    assert_eq!(literals(&groups), vec![None]);
}

#[test]
fn a_wildcard_dot_is_never_literal_tracked() {
    let groups = parse_capture_groups("/(a.b)/").unwrap().groups;
    assert_eq!(literals(&groups), vec![None]);
}

#[test]
fn anchors_contribute_the_empty_string_not_a_bail() {
    let groups = parse_capture_groups("/(a^b)/").unwrap().groups;
    assert_eq!(literals(&groups), vec![Some(vec!["ab".to_string()])]);
}

#[test]
fn an_unbounded_quantifier_disables_literal_tracking_for_that_atom() {
    let groups = parse_capture_groups("/(a+)/").unwrap().groups;
    assert_eq!(literals(&groups), vec![None]);
}

#[test]
fn an_unrecognized_escape_disables_literal_tracking_for_that_atom() {
    let groups = parse_capture_groups(r"/(a\db)/").unwrap().groups;
    assert_eq!(literals(&groups), vec![None]);
}

#[test]
fn a_bounded_quantifier_directly_after_a_quoted_block_bails_that_span() {
    let groups = parse_capture_groups(r"/(\Qab\E{2})/").unwrap().groups;
    assert_eq!(literals(&groups), vec![None]);
}

#[test]
fn a_group_repeated_more_than_once_externally_loses_its_own_literal_value() {
    let groups = parse_capture_groups("/(?:(a)){2,}/").unwrap().groups;
    assert_eq!(literals(&groups), vec![None]);
    assert_eq!(numbers_and_names(&groups), vec![(1, None, false)]);
}

#[test]
fn a_lookahead_contributes_the_empty_string_not_its_own_interior() {
    let groups = parse_capture_groups("/(a(?=b))/").unwrap().groups;
    assert_eq!(literals(&groups), vec![Some(vec!["a".to_string()])]);
}

#[test]
fn a_capturing_group_inside_a_lookahead_still_gets_its_own_correct_value() {
    let groups = parse_capture_groups("/(?=(a))/").unwrap().groups;
    assert_eq!(literals(&groups), vec![Some(vec!["a".to_string()])]);
}

#[test]
fn the_literal_enum_limit_degrades_one_span_without_failing_the_whole_parse() {
    // 6 branches of 2 chars each, repeated 6 times -> 6^6 = 46656
    // combinations, well past LITERAL_ENUM_LIMIT (32).
    let groups = parse_capture_groups("/((?:aa|bb|cc|dd|ee|ff){6})(x)/")
        .unwrap()
        .groups;
    assert_eq!(
        numbers_and_names(&groups),
        vec![(1, None, false), (2, None, false)]
    );
    assert_eq!(literals(&groups)[0], None);
    assert_eq!(literals(&groups)[1], Some(vec!["x".to_string()]));
}
