//! Structural shape: top-level flags, alternation identity/branch attribution, and how optionality does or doesn't propagate.

use super::super::*;
use super::helpers::*;
#[test]
fn top_level_is_correct_for_a_bare_a_wrapped_and_a_doubly_nested_group() {
    let bare = parse_capture_groups("/(a)/").unwrap().groups;
    assert_eq!(top_levels(&bare), vec![true]);

    let wrapped = parse_capture_groups("/(?:(a))/").unwrap().groups;
    assert_eq!(top_levels(&wrapped), vec![true]);

    let nested = parse_capture_groups("/((a))/").unwrap().groups;
    assert_eq!(top_levels(&nested), vec![true, false]);
}

#[test]
fn alternation_identifies_the_shared_id_and_distinct_branch_indices() {
    let groups = parse_capture_groups("/(a)|(b)|(c)/").unwrap().groups;
    let ids: Vec<u32> = groups.iter().map(|g| g.alternation.unwrap().0).collect();
    let branches: Vec<u32> = groups.iter().map(|g| g.alternation.unwrap().1).collect();
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[1], ids[2]);
    assert_eq!(branches, vec![0, 1, 2]);
}

#[test]
fn a_nested_alternations_own_descendants_are_attributed_to_the_inner_one() {
    // Group 1 wraps a nested alternation (groups 2, 3); group 4 is
    // the outer alternation's other branch. Groups 2/3 must belong
    // to the *inner* alternation, not the outer one, even though
    // they're discovered while scanning the outer alternation's own
    // branch 0.
    let groups = parse_capture_groups("/((a)|(b))|(c)/").unwrap().groups;
    let outer_id = groups[0].alternation.unwrap().0;
    let outer_other_id = groups[3].alternation.unwrap().0;
    assert_eq!(outer_id, outer_other_id);
    let inner_id_a = groups[1].alternation.unwrap().0;
    let inner_id_b = groups[2].alternation.unwrap().0;
    assert_eq!(inner_id_a, inner_id_b);
    assert_ne!(inner_id_a, outer_id);
}

#[test]
fn in_optional_quantification_does_not_propagate_through_a_non_capturing_wrapper() {
    let direct = parse_capture_groups("/(a)?/").unwrap().groups;
    assert!(direct[0].in_optional_quantification);

    // A real, non-obvious correction: real PHPStan's own flag resets
    // at every non-quantification node, so it must be `false` here,
    // not `true` - confirmed against real PHPStan directly (see
    // `RegexCaptureGroup::in_optional_quantification`'s own doc
    // comment).
    let wrapped = parse_capture_groups("/(?:(a))?/").unwrap().groups;
    assert!(!wrapped[0].in_optional_quantification);
}

#[test]
fn alternation_wrapped_in_optional_quantifier_is_set_even_when_no_group_is_directly_quantified() {
    let parsed = parse_capture_groups("/(?:(a)|(b))?/").unwrap();
    assert!(!parsed.groups[0].in_optional_quantification);
    assert!(!parsed.groups[1].in_optional_quantification);
    let alt_id = parsed.groups[0].alternation.unwrap().0;
    assert!(
        parsed
            .alternation_wrapped_in_optional_quantifier
            .contains(&alt_id)
    );
}

#[test]
fn alternation_branch_counts_reflects_every_branch_including_groupless_ones() {
    let parsed = parse_capture_groups("/(a)|(b)|x/").unwrap();
    let alt_id = parsed.groups[0].alternation.unwrap().0;
    assert_eq!(
        parsed
            .alternation_branch_counts
            .iter()
            .find(|(id, _)| *id == alt_id)
            .map(|(_, n)| *n),
        Some(3)
    );
}
