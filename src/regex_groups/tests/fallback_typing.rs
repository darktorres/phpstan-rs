//! `RegexGroupFallback`'s own tri-state fallback for a group whose
//! body isn't exactly literal-enumerable.

use super::super::*;
use super::helpers::fallbacks;

fn parse(pattern: &str) -> Vec<RegexGroupFallback> {
    fallbacks(
        &parse_capture_groups(pattern)
            .expect("pattern should parse")
            .groups,
    )
}

#[test]
fn a_digit_escape_repeated_is_numeric_string() {
    assert_eq!(parse("/(\\d+)/"), vec![RegexGroupFallback::NumericString]);
}

#[test]
fn a_digit_only_bracket_class_is_numeric_string() {
    assert_eq!(parse("/([0-9]+)/"), vec![RegexGroupFallback::NumericString]);
    assert_eq!(parse("/([0-9])/"), vec![RegexGroupFallback::NumericString]);
    // A subrange is still digit-safe, even though it doesn't cover all
    // ten digits - `digits_only` only ever asserts what it *can*
    // produce, never that it covers every digit.
    assert_eq!(parse("/([3-7]+)/"), vec![RegexGroupFallback::NumericString]);
    // A list of individual digits (no range syntax at all).
    assert_eq!(
        parse("/([13579]+)/"),
        vec![RegexGroupFallback::NumericString]
    );
    // `[:digit:]` POSIX class.
    assert_eq!(
        parse("/([[:digit:]]+)/"),
        vec![RegexGroupFallback::NumericString]
    );
}

#[test]
fn a_non_digit_character_class_is_non_empty_string_not_numeric() {
    assert_eq!(
        parse("/([a-z]+)/"),
        vec![RegexGroupFallback::NonEmptyString]
    );
}

#[test]
fn a_negated_class_is_never_provably_digit_only() {
    // `[^a-z]` *could* match a digit, but could just as easily match
    // anything else that isn't `a`-`z` - never provably digits-only,
    // even though it's still provably non-empty.
    assert_eq!(
        parse("/([^a-z]+)/"),
        vec![RegexGroupFallback::NonEmptyString]
    );
}

#[test]
fn a_group_whose_own_body_could_match_empty_gets_no_fallback() {
    // The `*` is *inside* the group here - the group's own body can
    // legitimately capture `""` (zero repetitions), so its own fallback
    // (describing what it looks like *when it does capture something*
    // has no bearing on that - never safe to claim non-empty, let alone
    // numeric.
    assert_eq!(parse("/(\\d*)/"), vec![RegexGroupFallback::Unknown]);
    assert_eq!(parse("/([a-z]*)/"), vec![RegexGroupFallback::Unknown]);
}

#[test]
fn a_group_optionally_quantified_from_outside_keeps_its_own_body_fallback() {
    // The `?` here is *outside* `(a)` - it governs whether the group
    // participates *at all* (handled separately, by
    // `checker::expr::preg_shapes::preg_match_matches_shape`'s own
    // optional/backfill logic), not what the group's body itself looks
    // like on the iterations where it *does* run - `(a)`'s own body is
    // still unconditionally `"a"`, same as `(a)` alone. Mirrors
    // `literal_values`'s own pre-existing behavior for the same pattern
    // shape (`optional_group_via_question_mark` in
    // `group_numbering_and_naming`): optionality and the group's own
    // per-capture shape are orthogonal.
    assert_eq!(parse("/(a)?/"), vec![RegexGroupFallback::NonEmptyString]);
}

#[test]
fn a_wildcard_or_unrecognized_escape_is_non_empty_but_not_digit_only() {
    // `.` always consumes exactly one (unknown) character.
    assert_eq!(parse("/(.+)/"), vec![RegexGroupFallback::NonEmptyString]);
    // `\w` is a known-length, known-not-digit-only escape class.
    assert_eq!(parse("/(\\w+)/"), vec![RegexGroupFallback::NonEmptyString]);
}

#[test]
fn a_literal_enumerable_group_does_not_need_the_fallback_but_still_computes_one() {
    // `literal_values` already wins in `group_leaf_type` - this just
    // confirms the fallback computation itself doesn't get confused by
    // an all-digit *literal* alternation (still correctly `NumericString`
    // even though `checker::expr::preg_shapes::group_leaf_type` will
    // never actually consult it here).
    let parsed = parse_capture_groups("/(1|2)/").unwrap();
    assert_eq!(
        parsed.groups[0].literal_values,
        Some(vec!["1".to_string(), "2".to_string()])
    );
    assert_eq!(parsed.groups[0].fallback, RegexGroupFallback::NumericString);
}

#[test]
fn word_boundary_and_other_zero_width_escapes_do_not_break_digit_only_ness() {
    // `\b` never contributes a character - vacuously digit-safe, so it
    // must not drag a following/preceding `\d+`'s own fallback down to
    // merely non-empty.
    assert_eq!(
        parse("/(\\d+\\b)/"),
        vec![RegexGroupFallback::NumericString]
    );
    assert_eq!(
        parse("/(\\b\\d+)/"),
        vec![RegexGroupFallback::NumericString]
    );
}

#[test]
fn a_quoted_block_of_all_digits_is_numeric_string() {
    assert_eq!(
        parse("/(\\Q123\\E)/"),
        vec![RegexGroupFallback::NumericString]
    );
    assert_eq!(
        parse("/(\\Qabc\\E)/"),
        vec![RegexGroupFallback::NonEmptyString]
    );
}

#[test]
fn case_insensitivity_never_affects_the_fallback() {
    // Digits (and non-emptiness) are unaffected by case folding - only
    // exact literal-value enumeration needs to bail under `/i`.
    assert_eq!(parse("/(\\d+)/i"), vec![RegexGroupFallback::NumericString]);
    assert_eq!(
        parse("/([a-z]+)/i"),
        vec![RegexGroupFallback::NonEmptyString]
    );
}

#[test]
fn a_group_repeated_more_than_once_externally_keeps_its_own_fallback() {
    // Unlike `literal_values` (deliberately cleared for a group repeated
    // from outside, mirroring real PHPStan's own choice), the fallback
    // reflects the group's *own body* shape, which a repeated capture
    // still has on every individual iteration (real PCRE fact: each
    // repetition still only ever captures one digit).
    let parsed = parse_capture_groups("/(\\d){2,3}/").unwrap();
    assert_eq!(parsed.groups[0].literal_values, None);
    assert_eq!(parsed.groups[0].fallback, RegexGroupFallback::NumericString);
}
