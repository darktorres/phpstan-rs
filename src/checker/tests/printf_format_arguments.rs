//! sprintf/printf-family format-string argument validation: placeholder
//! counts and argument types.

use super::helpers::*;

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

// Phase 69: `*` width/precision sub-placeholders each consume their own
// argument, in order, before the specifier's own value argument.
#[test]
fn sprintf_width_star_consumes_int_argument() {
    let d = diagnostics_for(r#"<?php sprintf("%*d", 5, 42);"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_width_star_rejects_non_int_argument() {
    let d = diagnostics_for(r#"<?php sprintf("%*d", "five", 42);"#);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("width"), "got: {d:?}");
}

#[test]
fn sprintf_precision_star_consumes_int_argument() {
    let d = diagnostics_for(r#"<?php sprintf("%.*f", 2, 3.14159);"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_width_and_precision_star_combined() {
    let d = diagnostics_for(r#"<?php sprintf("%*.*f", 10, 2, 3.14159);"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_positional_with_star_stays_unchecked() {
    // Deliberately not supported - see `parse_positional_format_specifiers`'s
    // own doc comment on real PHP's quirky positional/dynamic interaction.
    let d = diagnostics_for(r#"<?php sprintf('%1$*d', "not an int at all");"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 69: `sscanf`/`fscanf`'s 3+-arg by-ref-output-variable form.
#[test]
fn sscanf_byref_count_matches_placeholders_no_report() {
    let d = diagnostics_for(
        r#"<?php
            $a = 0; $b = '';
            sscanf("1 x", "%d %s", $a, $b);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sscanf_byref_count_mismatch_reports_argument_sscanf() {
    let d = diagnostics_for(
        r#"<?php
            $a = 0;
            sscanf("1 x", "%d %s", $a);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.sscanf");
}

#[test]
fn fscanf_byref_count_mismatch_reports_argument_fscanf() {
    let d = diagnostics_for(
        r#"<?php
            /** @param resource $stream */
            function f($stream) {
                $a = 0; $b = ''; $c = 0;
                fscanf($stream, "%d %s", $a, $b, $c);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.fscanf");
}

#[test]
fn sscanf_two_arg_form_not_affected() {
    let d = diagnostics_for(r#"<?php sscanf("1 x", "%d %s");"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sscanf_non_literal_format_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $fmt) {
                $a = 0;
                sscanf("1 x", $fmt, $a);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sscanf_spread_argument_stays_silent() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<mixed> $vars */
            function f(array $vars) {
                sscanf("1 x", "%d %s", ...$vars);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
