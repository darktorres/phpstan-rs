//! preg_match/preg_match_all/preg_replace capture-group and result typing.

use super::helpers::*;

#[test]
fn preg_replace_narrows_to_string_or_null_when_the_subject_is_a_string() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $subject): void {
                $result = preg_replace('/a/', 'b', $subject);
                if ($result !== null) {
                    strlen($result);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Round 11 of the Piwigo corpus cleanup: `preg_replace`/`str_replace`'s
// subject-dependent special case gated on a bare `== Type::String`,
// which stopped matching the instant Phase 14 made a narrowed
// "provably non-empty" string a real `Intersection([String,
// Accessory(NonEmptyString)])` instead of plain `String` - the exact
// shape `Lang::load()`'s own `if ($source_file === '') { return
// false; }` guard produces for every use of `$source_file` afterward.
#[test]
fn preg_replace_narrows_to_string_when_the_subject_is_a_narrowed_non_empty_string() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $subject): string {
                if ($subject === '') {
                    return '';
                }
                $result = preg_replace('/a/', 'b', $subject);
                if ($result === null) {
                    return '';
                }
                return $result;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 12 Finding A: preg_match's by-ref $matches shape - a real
// PCRE-structure parser (never a matching engine), scoped to the
// common 3-argument, literal-pattern form.
#[test]
fn preg_match_required_group_is_accepted_as_a_string_parameter() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches);
                needsString($matches[1]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Since Finding E, `$matches`'s own *unconditional* type (right after
// the call, with no narrowing) is a `Union` with the empty (no-match)
// shape - reading `$matches[1]` there collapses to `Mixed` (traced
// through in the plan itself: the empty-shape member's own missing-
// key fallback is `Mixed`, and `union_types` unconditionally absorbs
// into it), so the test above alone no longer discriminates "typed as
// a real `string`" from "leniently accepted because it's `Mixed`."
// This one narrows first (Finding E's own `preg_match_condition`
// recognizer), proving `$matches[1]` really is a precise `string` in
// the one context that matters in practice - passing something
// genuinely incompatible (an array) would be flagged if it weren't.
#[test]
fn preg_match_required_group_is_precisely_typed_once_the_match_is_confirmed() {
    let d = diagnostics_for(
        r#"<?php
            function needsArray(array $a): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?/', $subject, $matches)) {
                    needsArray($matches[1]);
                }
            }
        "#,
    );
    // Phase 12 (literal enumeration): group 1's own content is the
    // constant `a`, so it's now typed as the literal `'a'`, not
    // generic `string` - even more precise than this test's own
    // original intent (proving precise typing at all, not just
    // Mixed-leniency), so the check just needs to name the sharper
    // type.
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsArray") && x.message.contains("'a'")),
        "expected an argument-type mismatch naming 'a', got: {d:?}"
    );
}

#[test]
fn preg_match_reading_a_genuinely_absent_group_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches);
                echo $matches[5];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("no key '5'")),
        "expected a 'no such key' diagnostic, got: {d:?}"
    );
}

// A "reads $matches[1]/[5] in the impossible branch" version of this
// test wouldn't actually discriminate whether narrowing happened:
// `infer_expr`'s `Index` arm falls back to `Mixed` for `Type::Never`
// just as readily as for a real `ArrayShape`, so no diagnostic either
// way. Passing `$matches` itself to a parameter requiring an
// incompatible type is the real, discriminating test - `Never` is
// vacuously assignable to anything (so no diagnostic proves the
// branch really was narrowed to it), while an ordinary `ArrayShape`
// is not (so a diagnostic there proves the optional case correctly
// *isn't* over-optimistically narrowed).
#[test]
fn preg_match_array_key_exists_false_branch_narrows_a_required_group_to_never() {
    // Nested inside a truthy `preg_match(...)` check (Finding E's own
    // `preg_match_condition` narrowing) rather than tested in
    // isolation: since Finding E, $matches's own *unconditional* type
    // right after the call is a `Union` with the empty (no-match)
    // shape, and array_key_exists's own false-branch narrowing
    // correctly leaves that empty-shape Union member untouched (it's
    // entirely consistent with key '1' being absent) - so, without
    // first narrowing away the no-match possibility, the overall
    // result is genuinely `array{}`, not `Never`. The precise,
    // non-Union full shape (and therefore a real `Never` narrowing
    // for a required key) only exists once the match is confirmed.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?/', $subject, $matches)) {
                    if (! array_key_exists('1', $matches)) {
                        needsInt($matches);
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_array_key_exists_false_branch_does_not_narrow_an_optional_group() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches);
                if (! array_key_exists('2', $matches)) {
                    needsInt($matches);
                }
            }
        "#,
    );
    assert!(
        !d.is_empty(),
        "expected a diagnostic proving the optional group wasn't narrowed to Never, got none"
    );
}

// --- Task 1c: trailing-vs-interior optional field presence/value ---

#[test]
fn preg_match_an_interior_optional_group_is_always_present_not_optional() {
    // Group 2 is optional (via `?`) but *interior* - group 3, after
    // it, is required, so real PCRE can never omit $matches[2]
    // entirely (confirmed live: `preg_match('/(a)(b)?(c)/', 'ac', $m)`
    // gives `$m = [0=>'ac', 1=>'a', 2=>'', 3=>'c']`). Using the same
    // established technique as `preg_match_array_key_exists_false_
    // branch_narrows_a_required_group_to_never` above: if the false
    // branch of `array_key_exists('2', ...)` narrows `$matches` to
    // `Never` (vacuously assignable to anything), the field is
    // correctly modeled as always-present.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?(c)/', $subject, $matches)) {
                    if (! array_key_exists('2', $matches)) {
                        needsInt($matches);
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_a_trailing_optional_group_keeps_its_own_optional_field() {
    // Same shape as the interior case above, but group 2 here really
    // is the pattern's own last group (trailing) - today's existing,
    // unaffected-by-Task-1c behavior: the field can genuinely be
    // absent, so `array_key_exists`'s false branch must *not* narrow
    // `$matches` to `Never`.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a)(b)?/', $subject, $matches)) {
                    if (! array_key_exists('2', $matches)) {
                        needsInt($matches);
                    }
                }
            }
        "#,
    );
    assert!(
        !d.is_empty(),
        "expected a diagnostic proving the trailing optional group wasn't narrowed to Never, got none"
    );
}

#[test]
fn preg_match_named_group_is_readable_by_number_and_by_name() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(?<year>\d+)/', $subject, $matches);
                needsString($matches[1]);
                needsString($matches['year']);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// See the identical reasoning on
// `preg_match_required_group_is_precisely_typed_once_the_match_is_confirmed`
// above - narrows first, so this actually proves precise typing by
// both number and name, not just Mixed-leniency.
#[test]
fn preg_match_named_group_is_precisely_typed_by_number_and_name_once_confirmed() {
    let d = diagnostics_for(
        r#"<?php
            function needsArray(array $a): void {}
            function f($subject): void {
                if (preg_match('/(?<year>\d+)/', $subject, $matches)) {
                    needsArray($matches[1]);
                    needsArray($matches['year']);
                }
            }
        "#,
    );
    assert_eq!(
        d.iter()
            .filter(|x| x.message.contains("needsArray") && x.message.contains("string"))
            .count(),
        2,
        "expected two argument-type mismatches naming string, got: {d:?}"
    );
}

// --- Phase 12: literal enumeration for constant-only groups ---

#[test]
fn preg_match_a_constant_alternation_group_is_typed_as_a_literal_union() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(cat|dog)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.contains("'cat'|'dog'")),
        "expected an argument-type mismatch naming 'cat'|'dog', got: {d:?}"
    );
}

// `preg_match` itself can't be used to test this composition:
// reading through a flagged call's `$matches` always collapses to
// `Mixed` (Finding E's own narrowing is scoped to no-flags callers
// only, so a flagged call's `$matches` stays the unconditional
// `Union` even inside a confirmed-matched `if` - see
// `preg_match_offset_capture_types_each_entry_as_a_string_int_tuple`'s
// own comment above). `preg_match_all` never has that Union at all
// (Finding D), so it's the only way to observe the composed tuple
// shape directly.
#[test]
fn preg_match_all_a_constant_group_combined_with_offset_capture_only_narrows_the_text_slot() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(cat)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                needsInt($matches[1][0][0]);
                needsInt($matches[1][0][1]);
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected exactly one mismatch (the text slot only), got: {d:?}"
    );
    assert!(
        d[0].message.contains("needsInt") && d[0].message.contains("'cat'"),
        "expected the text slot to be flagged as 'cat', got: {d:?}"
    );
}

// Phase 72: `\d+` is neither a literal sequence nor a top-level
// alternation, but it *is* now provably a non-empty, digits-only group
// (the `RegexGroupFallback::NumericString` tri-state fallback) - refined
// to `numeric-string`, not a wrong or guessed literal, and strictly more
// precise than the plain `string` this used to degrade to.
#[test]
fn preg_match_a_digit_only_group_is_typed_as_numeric_string() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(\d+)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.ends_with("numeric-string given")),
        "expected a numeric-string mismatch, got: {d:?}"
    );
}

// A group with a genuinely mixed, non-digit-only character class -
// `RegexGroupFallback::NonEmptyString` instead: still refined, just to
// the weaker of the two fallbacks.
#[test]
fn preg_match_a_non_digit_non_empty_group_is_typed_as_non_empty_string() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/([a-z]+)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
    );
    assert!(
        d.iter().any(
            |x| x.message.contains("needsInt") && x.message.ends_with("non-empty-string given")
        ),
        "expected a non-empty-string mismatch, got: {d:?}"
    );
}

// A group that could legitimately be empty (`\d*`) gets neither
// fallback - false-positive guard: this must NOT be typed as
// `numeric-string` (an empty string genuinely isn't one), so it stays
// plain `string`, same as before this phase.
#[test]
fn preg_match_a_possibly_empty_digit_group_stays_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(\d*)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.ends_with("string given")),
        "expected a plain string mismatch (not numeric-string), got: {d:?}"
    );
}

// A digit-only group already covered by *exact* literal enumeration
// (Task 1a) must keep using that, never the coarser tri-state fallback -
// false-positive guard against the fallback accidentally taking
// priority over a strictly more precise, already-working mechanism.
#[test]
fn preg_match_a_literal_digit_group_keeps_its_exact_literal_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(1|2)/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.ends_with("'1'|'2' given")),
        "expected the exact literal union '1'|'2', got: {d:?}"
    );
}

// --- Task 1b: tagged unions ---
//
// Reading `$matches[N]` directly from the tagged `Union` collapses to
// `Mixed` (the "without" variant genuinely lacks key N, and a `Union`
// read already correctly falls back to `Mixed` whenever any member
// lacks the key - established, correct behavior, not something to
// work around). `array_key_exists('N', $matches)` narrowing
// (`narrow_array_shape_by_key_presence`) is what actually drops the
// non-matching member first, leaving a plain (non-`Union`) shape
// behind - the same mechanism these tests use to observe the tagged
// union's own internal shape precisely.

#[test]
fn preg_match_single_optional_group_with_variant_keeps_the_nested_descendant() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(a(b))?/', $subject, $matches)) {
                    if (array_key_exists('1', $matches)) {
                        needsInt($matches[1]);
                        needsInt($matches[2]);
                    }
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.contains("'ab'")),
        "expected the with-variant's group 1 to be 'ab', got: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.contains("'b'")),
        "expected the with-variant's nested group 2 to be 'b', got: {d:?}"
    );
}

#[test]
fn preg_match_single_optional_group_without_variant_is_bare() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                if (preg_match('/(a(b))?/', $subject, $matches)) {
                    if (!array_key_exists('1', $matches)) {
                        echo $matches[1];
                        echo $matches[2];
                    }
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("no key '1'")),
        "expected the without-variant to lack key 1, got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.message.contains("no key '2'")),
        "expected the without-variant to also lack the nested descendant's own key 2, got: {d:?}"
    );
}

#[test]
fn preg_match_two_optional_top_level_groups_stay_on_the_flat_path() {
    // Two top-level groups fails the "exactly one" gate - stays flat
    // (an `optional` field), not tagged. `array_key_exists` on a
    // *flat* shape's own optional field narrows without ever
    // touching a `Union` at all, so a diagnostic here would only
    // appear if this pattern were wrongly treated as a single
    // optional group.
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match('/(a)?(b)?/', $subject, $matches);
                echo $matches[1];
                echo $matches[2];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_a_top_level_alternation_forces_earlier_siblings_and_removes_later_ones() {
    // `array_key_exists('2', ...)` alone isn't enough to isolate the
    // "dog" branch specifically: field 2 is *also* present (forced
    // `''`) in the "bird" branch's own variant, as an *earlier*
    // sibling of group 3 - real, confirmed behavior, not a test bug
    // to work around by accident. Combined with `!array_key_exists
    // ('3', ...)` (true only for "cat" and "dog", since "bird" is
    // the only variant with field 3 present), the pair together
    // isolates "dog" specifically.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(cat)|(dog)|(bird)/', $subject, $matches)) {
                    if (array_key_exists('2', $matches) && !array_key_exists('3', $matches)) {
                        needsInt($matches[1]); // earlier sibling - forced ''
                        needsInt($matches[2]); // this branch's own group
                        echo $matches[3]; // later sibling - removed entirely
                    }
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.contains("''")),
        "expected the earlier sibling to be forced to '', got: {d:?}"
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.contains("'dog'")),
        "expected this branch's own group to be 'dog', got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.message.contains("no key '3'")),
        "expected the later sibling to be entirely removed, got: {d:?}"
    );
}

#[test]
fn preg_match_a_zero_group_alternation_branch_adds_the_bare_variant() {
    // 2 top-level groups rules out the single-optional-group
    // dispatch; the alternation has 3 branches but only 2 groups
    // (`x` has none) - matching it produces $matches with only
    // field "0", a real possibility this test confirms is present
    // by narrowing array_key_exists('1', ...) to false and checking
    // the surviving shape is genuinely bare.
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                if (preg_match('/(cat)|(dog)|x/', $subject, $matches)) {
                    if (!array_key_exists('1', $matches) && !array_key_exists('2', $matches)) {
                        echo $matches[1];
                    }
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("no key '1'")),
        "expected the bare (zero-group-branch) variant to lack key 1, got: {d:?}"
    );
}

#[test]
fn preg_match_an_alternation_wrapped_in_an_optional_quantifier_still_adds_the_bare_variant() {
    // Every branch already has a group (no branch-count mismatch) -
    // the bare variant here can only come from
    // `alternation_wrapped_in_optional_quantifier`.
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                if (preg_match('/(?:(cat)|(dog))?/', $subject, $matches)) {
                    if (!array_key_exists('1', $matches) && !array_key_exists('2', $matches)) {
                        echo $matches[1];
                    }
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("no key '1'")),
        "expected the bare (whole-alternation-optional) variant to lack key 1, got: {d:?}"
    );
}

#[test]
fn preg_match_unmatched_as_null_stays_flat_for_the_single_optional_group_case() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match('/(a(b))?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                echo $matches[1];
                echo $matches[2];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_unmatched_as_null_stays_flat_for_the_alternation_case() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match('/(cat)|(dog)/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                echo $matches[1];
                echo $matches[2];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_with_a_dynamic_pattern_stays_mixed() {
    let d = diagnostics_for(
        r#"<?php
            function f($pattern, $subject): void {
                preg_match($pattern, $subject, $matches);
                echo $matches[99];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_with_an_unrecognized_flags_expression_stays_mixed() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, SOME_UNDEFINED_FLAG);
                echo $matches[99];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 12 Finding B: PREG_OFFSET_CAPTURE - confirmed against a real
// PHP interpreter that every present entry becomes a real [string,
// int] tuple (the match text and its byte offset), not a bare
// string. Passes leniently either way - Finding E's own
// `preg_match_condition` narrowing is deliberately scoped to
// no-`$flags` callers only, so a flagged call's `$matches` stays a
// `Union` even inside a confirmed-matched `if`, and reading through
// it collapses to `Mixed` (this project's usual "don't know, don't
// flag" leniency) rather than the real tuple shape. The genuinely
// discriminating check for a flagged call is field *presence*
// through the `Union` (Part 1's own real value), covered by the
// dedicated test just below.
#[test]
fn preg_match_offset_capture_types_each_entry_as_a_string_int_tuple() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                needsString($matches[1][0]);
                needsInt($matches[1][1]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_offset_capture_still_flags_a_genuinely_absent_group_through_the_union() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match('/(a)/', $subject, $matches, PREG_OFFSET_CAPTURE);
                echo $matches[99];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("no key '99'")),
        "expected a 'no such key' diagnostic even through the Union, got: {d:?}"
    );
}

// Phase 12 Finding C: PREG_UNMATCHED_AS_NULL - confirmed against a
// real PHP interpreter that a non-participating optional group
// becomes `null` in the array rather than an entirely absent key.
// Both tests below now pass leniently either way, the same reason as
// `preg_match_offset_capture_types_each_entry_as_a_string_int_tuple`
// just above: Finding E's own narrowing is scoped to no-`$flags`
// callers, so a `PREG_UNMATCHED_AS_NULL`-flagged call's `$matches`
// stays a `Union` with no narrowed, precise-typed context to read it
// from - reading through it collapses to `Mixed` regardless of the
// real nullable-vs-plain distinction this shape-building logic
// actually produces. **A real, disclosed test gap, not a false
// claim of coverage elsewhere**: `preg_match_matches_shape` itself
// (`checker/expr.rs`) has no dedicated unit test of its own - this
// project's established convention for this family of per-function
// shape helpers (see `curl_getinfo_component_type`'s identical lack
// of one) is to verify them only through `diagnostics_for`-based
// integration tests like these, which is exactly what Finding E's
// own narrowing scope now prevents from discriminating this one
// specific distinction. Left open rather than papered over with a
// test that would pass without truly exercising anything.
#[test]
fn preg_match_unmatched_as_null_makes_an_optional_group_nullable_but_always_present() {
    let d = diagnostics_for(
        r#"<?php
            function needsNullableString(?string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                // Reading the field at all is fine (no "no such key") -
                // the field being present is still correctly preserved
                // through the Union.
                needsNullableString($matches[2]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_unmatched_as_null_does_not_widen_a_required_group() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match('/(a)(b)?/', $subject, $matches, PREG_UNMATCHED_AS_NULL);
                needsString($matches[1]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 12 Finding D: preg_match_all - both PREG_PATTERN_ORDER (the
// real default) and PREG_SET_ORDER, both confirmed against a real
// PHP interpreter before implementing.
#[test]
fn preg_match_all_pattern_order_types_each_group_as_an_array_of_strings() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches);
                needsInt($matches[1]);
            }
        "#,
    );
    // Phase 12 (literal enumeration): group 1's own content is the
    // constant `a`, so its per-match array element is now the
    // literal `'a'`, not generic `string`. Corpus round: each group's
    // own match array is itself always a real `list<T>` (verified via
    // dumpType()), not merely `array<T>`.
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.contains("list<'a'>")),
        "expected an argument-type mismatch naming list<'a'>, got: {d:?}"
    );
}

#[test]
fn preg_match_all_pattern_order_always_populates_every_key_never_optional() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches);
                // Group 2 is optional in a single match, but
                // PREG_PATTERN_ORDER always includes its own key
                // (possibly as an empty array) - reading it should never
                // be flagged as "no such key".
                echo count($matches[2]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_all_set_order_reuses_the_single_match_shape_per_entry() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches, PREG_SET_ORDER);
                foreach ($matches as $oneMatch) {
                    needsString($oneMatch[1]);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_all_set_order_reading_a_genuinely_absent_group_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match_all('/(a)(b)?/', $subject, $matches, PREG_SET_ORDER);
                foreach ($matches as $oneMatch) {
                    echo $oneMatch[5];
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("no key '5'")),
        "expected a 'no such key' diagnostic, got: {d:?}"
    );
}

#[test]
fn preg_match_all_set_order_composes_with_offset_capture() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                preg_match_all('/(a)/', $subject, $matches, PREG_SET_ORDER | PREG_OFFSET_CAPTURE);
                foreach ($matches as $oneMatch) {
                    needsInt($oneMatch[1][1]);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_all_with_a_pattern_this_parser_bails_on_stays_mixed() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match_all('/(?(1)a|b)/', $subject, $matches);
                echo $matches[99];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_match_with_a_pattern_this_parser_bails_on_stays_mixed() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match('/(?(1)a|b)/', $subject, $matches);
                echo $matches[99];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn known_by_ref_output_param_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $text) {
                if (preg_match('/(\d+)/', $text, $matches)) {
                    return $matches[1];
                }
                return null;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// --- Phase 72 ---

// (b): a statically-foldable `Concat` pattern argument is now resolved
// by `literal_string_value` itself, so `preg_match`'s own `$matches`
// shape inference (which already only ever looked at
// `literal_string_value(pattern_arg)`) picks it up for free.
#[test]
fn preg_match_understands_a_statically_foldable_concatenated_pattern() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(' . 'cat|dog' . ')/', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("needsInt") && x.message.contains("'cat'|'dog'")),
        "expected an argument-type mismatch naming 'cat'|'dog', got: {d:?}"
    );
}

// False-positive guard: a pattern concatenated with a genuine variable
// operand can never be folded (no access to inferred types here) - the
// existing "dynamic pattern stays untouched" behavior, unchanged.
#[test]
fn preg_match_still_skips_a_pattern_concatenated_with_a_real_variable() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $prefix, $subject): void {
                preg_match($prefix . '(\d+)/', $subject, $matches);
                echo $matches[99];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// (a): the `x` (free-spacing) modifier no longer bails the whole parse -
// unescaped whitespace and `#`-comments between tokens are skipped, the
// same way real PCRE's own free-spacing mode does (confirmed live
// against a real PHP interpreter).
#[test]
fn preg_match_understands_a_free_spacing_pattern_with_comments() {
    // The pattern has to appear directly as the call argument, not
    // through an intermediate variable - `literal_string_value` only
    // ever looks at the raw `Expr` passed to it, with no access to
    // inferred types/`Locals` (same reasoning as its own `Concat`
    // folding, see `checker::assignment::literal_string_value`'s own
    // doc comment).
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/
                    (\d+)   # the leading number
                    \s*
                    ([a-z]+) # the trailing word
                /x', $subject, $matches)) {
                    needsInt($matches[1]);
                    needsInt($matches[2]);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 2, "got: {d:?}");
    assert!(
        d[0].message.contains("needsInt") && d[0].message.ends_with("numeric-string given"),
        "expected group 1 as numeric-string, got: {d:?}"
    );
    assert!(
        d[1].message.contains("needsInt") && d[1].message.ends_with("non-empty-string given"),
        "expected group 2 as non-empty-string, got: {d:?}"
    );
}

// False-positive guard: whitespace *inside* a character class stays
// literal even under `/x` - real PCRE fact - so this pattern's own
// class genuinely only matches `'a'`/`' '`/`'b'`, never merges into one
// bigger, wrongly-skipped-whitespace class.
#[test]
fn preg_match_free_spacing_mode_still_treats_class_whitespace_as_literal() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/([a b])/x', $subject, $matches)) {
                    needsInt($matches[1]);
                }
            }
        "#,
    );
    // Not digits-only (contains a literal space and letters) and not
    // provably non-empty via any *other* mechanism this scanner tracks
    // beyond the class's own known min_len=1 - so it *is* non-empty,
    // just never a numeric-string.
    assert!(
        d.iter().any(
            |x| x.message.contains("needsInt") && x.message.ends_with("non-empty-string given")
        ),
        "expected non-empty-string (never numeric-string), got: {d:?}"
    );
}

// (a): `(?|...)` branch-reset - the classic idiom (each branch captures
// something, reusing the same public group number) still can't be
// shape-typed (see `parse_capture_groups`'s own doc comment on why),
// but no longer bails the *whole* parse the way it did before this
// phase - `$matches` just stays the generic widened fallback, same as
// any other call this project doesn't confidently understand, rather
// than every other `preg_match` in the same function file getting no
// benefit from this scanner at all.
#[test]
fn preg_match_branch_reset_with_reused_numbers_safely_stays_untyped() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject): void {
                preg_match('/(?|(\d+)|([a-z]+))/', $subject, $matches);
                echo $matches[1];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// (a): the narrower, safely-resolvable shape of branch-reset - only one
// branch (of several) captures anything at all, so no number is ever
// reused - types correctly, including a group declared *after* the
// construct renumbering correctly (confirmed live against a real PHP
// interpreter: `(d)` really is group 2 here, not 3, since no branch of
// `(?|...)` reaches higher than group 1).
#[test]
fn preg_match_branch_reset_with_only_one_capturing_branch_types_correctly() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function f($subject): void {
                if (preg_match('/(?|(\d+)|x|y)([a-z]+)/', $subject, $matches)) {
                    needsInt($matches[1]);
                    needsInt($matches[2]);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 2, "got: {d:?}");
    // Group 1 is the branch-reset's own `\d+` - real PCRE fact:
    // reachable via any of the 3 branches (only one of which actually
    // captures), so it's optional at the shape level and backfilled with
    // `''` when a groupless branch ("x"/"y") is the one that matched -
    // `numeric-string` names its own non-empty contribution either way.
    assert!(
        d[0].message.contains("needsInt") && d[0].message.contains("numeric-string"),
        "expected group 1 (the branch-reset's own \\d+) to mention numeric-string, got: {d:?}"
    );
    assert!(
        d[1].message.contains("needsInt") && d[1].message.ends_with("non-empty-string given"),
        "expected group 2 ([a-z]+, renumbered correctly to 2) as non-empty-string, got: {d:?}"
    );
}
