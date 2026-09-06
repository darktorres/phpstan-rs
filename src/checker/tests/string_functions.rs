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

// A top-10-bucket workflow found the identical `non-falsy-string`
// inference this file's own Concat tests above cover was missing for
// interpolated strings specifically: `ExprKind::Interpolated` returned
// bare `Type::String` unconditionally, even when one of its own parts
// was a real, non-falsy literal segment - the exact same fact
// `"prefix-" . $unknown` already proved via Concat.
#[test]
fn an_interpolated_string_with_a_non_falsy_literal_segment_narrows_to_non_falsy_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $unknown): void {
                sink("prefix-{$unknown}");
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_interpolated_string_with_no_non_falsy_part_does_not_falsely_claim_it() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $a, string $b): void {
                sink("{$a}{$b}");
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

// A top-10-bucket workflow found sprintf()/vsprintf() had zero dynamic-
// return-type handling at all - real PHPStan narrows to
// `non-falsy-string` whenever the format string's own literal (non-
// specifier) text is itself provably non-falsy, the same reasoning
// Concat/Interpolated already use.
#[test]
fn sprintf_with_a_non_falsy_literal_segment_narrows_to_non_falsy_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(int $n): void {
                sink(sprintf('value: %d', $n));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn vsprintf_with_a_non_falsy_literal_segment_narrows_to_non_falsy_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(array $args): void {
                sink(vsprintf('value: %d', $args));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_with_only_specifiers_does_not_falsely_claim_non_falsy() {
    // Every specifier could expand to an empty string, so a format
    // string with no real literal text left over can't be proven
    // non-falsy - real shape: `sprintf('%s', '')` is itself `''`.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $s): void {
                sink(sprintf('%s', $s));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn sprintf_with_a_literal_zero_remainder_does_not_falsely_claim_non_falsy() {
    // `sprintf('0%s', '')` really does produce the falsy string `"0"`.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $s): void {
                sink(sprintf('0%s', $s));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn sprintf_with_a_non_literal_format_is_unaffected() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $format, int $n): void {
                sink(sprintf($format, $n));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// A top-10-bucket workflow found var_export($x, true) typed too loosely
// - the native/stub signature is unconditionally `?string`, but a
// literal `true` second argument means it always returns the real
// representation string instead of printing to stdout and returning
// null.
#[test]
fn var_export_with_a_literal_true_flag_narrows_to_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            function f($x): string {
                return var_export($x, true);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn var_export_with_a_literal_false_flag_stays_nullable() {
    let d = diagnostics_for(
        r#"<?php
            function f($x): string {
                return var_export($x, false);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "expected a return.type mismatch (still nullable), got: {d:?}"
    );
}

#[test]
fn var_export_with_the_default_omitted_flag_stays_nullable() {
    let d = diagnostics_for(
        r#"<?php
            function f($x): string {
                return var_export($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "expected a return.type mismatch (still nullable), got: {d:?}"
    );
}

#[test]
fn var_export_with_a_non_literal_flag_stays_nullable() {
    let d = diagnostics_for(
        r#"<?php
            function f($x, bool $flag): string {
                return var_export($x, $flag);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "expected a return.type mismatch (still nullable), got: {d:?}"
    );
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
    // A dynamic (non-literal) format string, an unrecognized specifier
    // character, and a format string mixing positional and unnumbered
    // specifiers all safely fall back to no check at all - "don't know,
    // don't guess" - even though the mixed-style call is arguably
    // missing an argument (real PHP's own interaction between the two
    // styles is quirky enough not to guess at, see
    // `parse_positional_format_specifiers`'s own doc comment).
    let d = diagnostics_for(
        r#"<?php
            function f(string $fmt) {
                sprintf($fmt, "whatever");
                sprintf("%v", "x");
                sprintf('%1$s %s', "only one");
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 69: positional specifiers (`%1$s`) - a real, common PHP idiom
// for reordering arguments in localized message strings.

#[test]
fn a_purely_positional_format_string_missing_an_argument_is_flagged() {
    let d = diagnostics_for(r#"<?php sprintf('%1$s %2$s', "only one");"#);
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn a_purely_positional_format_string_with_enough_arguments_is_never_flagged() {
    let d = diagnostics_for(r#"<?php sprintf('%2$s %1$s', "a", "b");"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_repeated_position_only_needs_one_argument() {
    // `%1$s %1$s` references position 1 twice but only ever needs one
    // real argument to satisfy both.
    let d = diagnostics_for(r#"<?php sprintf('%1$s %1$s', "a");"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_gap_position_still_counts_toward_the_minimum_argument_count() {
    // `%3$s` alone still needs positions 1 and 2 to exist as real
    // arguments, even though nothing checks their own type.
    let d = diagnostics_for(r#"<?php sprintf('%3$s', "a", "b");"#);
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn a_positional_specifier_still_flags_a_bad_argument_type() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $arr) {
                sprintf('%1$d', $arr);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_position_referenced_by_two_different_specifiers_checks_both() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $arr) {
                sprintf('%1$s %1$d', $arr);
            }
        "#,
    );
    assert_eq!(
        d.iter().filter(|x| x.identifier == "argument.type").count(),
        2,
        "got: {d:?}"
    );
}

// Phase 69: wiring the existing sprintf/printf engine into
// fprintf/vsprintf/vprintf.

#[test]
fn fprintf_checks_its_format_string_as_the_second_argument() {
    let d = diagnostics_for(
        r#"<?php
            function f($stream) {
                fprintf($stream, "%s and %s", "only one");
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn vsprintf_checks_a_literal_array_argument() {
    let d = diagnostics_for(r#"<?php vsprintf('%s and %s', ["only one"]);"#);
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

#[test]
fn vprintf_checks_a_literal_array_argument_s_element_types() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $arr) {
                vprintf('%d', [$arr]);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn vsprintf_with_a_non_literal_array_argument_is_never_flagged() {
    // Don't guess - a plain `array<T>`-typed variable (not a real,
    // sequential-keyed `ArrayShape`) has no per-position types to check
    // against at all.
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string> $values */
            function f(array $values) {
                vsprintf('%s and %s', $values);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
