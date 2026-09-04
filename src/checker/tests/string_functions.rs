//! Built-in string function return type inference (str_replace, sprintf, explode, etc).

use super::helpers::*;

// Phase 23 Finding 3: str_replace/preg_replace's real stub return
// types are unconditionally unioned with `array`, even though the
// function only returns an array when $subject itself is one.
#[test]
fn str_replace_narrows_to_string_when_the_subject_is_a_string() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $subject): string {
                return str_replace(['a', 'b'], ['c', 'd'], $subject);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Round 12: verified via dumpType() - concatenating any string with a
// real non-falsy string literal (on either side) always produces a
// non-falsy-string, regardless of the other side's own type. Real
// corpus shape: `sys_get_temp_dir() . '/'` fed straight into
// PHPUnit's `assertStringStartsWith(non-empty-string $prefix, ...)`.
#[test]
fn concatenating_a_non_falsy_string_literal_on_the_right_narrows_to_non_falsy_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $unknown): void {
                sink($unknown . '/');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn concatenating_a_non_falsy_string_literal_on_the_left_narrows_to_non_falsy_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $unknown): void {
                sink('.' . $unknown);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn concatenating_two_unknown_strings_does_not_falsely_claim_non_falsy() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $a, string $b): void {
                sink($a . $b);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn str_replace_falls_back_to_the_generic_path_for_a_non_string_subject() {
    // Scoped to "subject is exactly Type::String" - a Mixed subject
    // (or a real array one) must keep today's existing, unaffected
    // generic-path behavior rather than the new special case firing.
    let d = diagnostics_for(
        r#"<?php
            function f($subject) {
                $result = str_replace('a', 'b', $subject);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 32 Finding E: same subject-dependent-return-type family as
// `str_replace` above, missed by Phase 23/31 - `substr_replace`'s own
// subject is argument index 0, not 2.
#[test]
fn substr_replace_narrows_to_string_when_the_subject_is_a_string() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $subject): string {
                return substr_replace($subject, 'x', 0, 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn substr_replace_falls_back_to_the_generic_path_for_a_non_string_subject() {
    let d = diagnostics_for(
        r#"<?php
            function f($subject) {
                $result = substr_replace($subject, 'x', 0, 1);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The vendored stub's own `@return string[]|false` reflects PHP 7's
// behavior for an empty $separator - PHP 8+ throws a ValueError there
// instead, so real PHPStan drops |false entirely and always answers
// non-empty-list<string>, verified via dumpType() (even for a
// non-literal $separator). Real Piwigo code
// (GeneratedFileErrorMapper::lines) relies on exactly this.
#[test]
fn explode_always_returns_a_non_empty_list_never_false() {
    let d = diagnostics_for(
        r#"<?php
            /** @return non-empty-list<string> */
            function f(string $sep, string $s): array {
                return explode($sep, $s);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Only the array-stripping half of mb_convert_encoding()'s own real
// (more involved) dynamic extension is implemented - evidenced via
// `CharsetHelper::convertCharset`'s own real shape.
#[test]
fn mb_convert_encoding_narrows_to_string_or_false_when_the_subject_is_a_string() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $subject): string|false {
                return mb_convert_encoding($subject, 'UTF-8', 'ISO-8859-1');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_flags_too_few_arguments_for_its_own_placeholders() {
    let d = diagnostics_for(r#"<?php sprintf("%s and %s", "only one");"#);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("expects 2 argument(s), 1 given"),
        "got: {d:?}"
    );
}

// Phase 72: `literal_string_value` now folds a statically-foldable
// `Concat` chain (every operand a literal, or a parenthesized grouping
// of one) into one owned string, so a format string built by
// concatenating nothing but literals is understood exactly the same as
// if it had been written as one literal to begin with.
#[test]
fn sprintf_understands_a_statically_foldable_concatenated_format_string() {
    let d = diagnostics_for(r#"<?php sprintf('%s and ' . '%s', "only one");"#);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("expects 2 argument(s), 1 given"),
        "got: {d:?}"
    );
}

// False-positive guard: a concatenation with a genuine variable operand
// isn't statically foldable at all (this helper never has access to
// inferred types/`Locals`, only the raw `Expr`) - stays the same "don't
// know, don't guess" skip as any other dynamic format string, never a
// wrong guess at the placeholder count.
#[test]
fn sprintf_still_skips_a_concatenation_with_a_real_variable_operand() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $suffix) {
                sprintf('%s and ' . $suffix, "only one");
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_does_not_flag_extra_arguments_real_php_just_ignores() {
    let d = diagnostics_for(r#"<?php sprintf("%s", "a", "b", "c");"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_flags_an_array_passed_for_any_specifier() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $arr) {
                sprintf("%d", $arr);
                sprintf("%s", $arr);
            }
        "#,
    );
    assert_eq!(d.len(), 2, "got: {d:?}");
    assert!(d.iter().all(|x| x.message.contains("does not accept")));
}

#[test]
fn sprintf_flags_an_object_for_a_numeric_specifier_but_not_a_string_one() {
    // Real PHP's %s accepts anything with __toString (extremely
    // common), so objects are only flagged against a numeric
    // specifier, which no object can legitimately satisfy.
    let d = diagnostics_for(
        r#"<?php
            class Money {}
            function f(Money $m) {
                sprintf("%d", $m);
                sprintf("%s", $m);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("%d"), "got: {d:?}");
}

#[test]
fn sprintf_and_printf_accept_ordinary_scalar_arguments() {
    let d = diagnostics_for(
        r#"<?php
            sprintf("%s is %d years old, %.2f%% sure", "Bob", 30, 99.5);
            printf("%x", 255);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_skips_the_check_entirely_for_shapes_it_does_not_confidently_understand() {
    // A positional specifier (%1$s), a dynamic (non-literal) format
    // string, and an unrecognized specifier character all safely
    // fall back to no check at all - "don't know, don't guess" - even
    // though the positional-args call below is actually missing an
    // argument in spirit (it repeats the same one), and the
    // unknown-specifier call is arguably malformed PHP.
    let d = diagnostics_for(
        r#"<?php
            function f(string $fmt) {
                sprintf('%1$s %2$s', "only one");
                sprintf($fmt, "whatever");
                sprintf("%v", "x");
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
