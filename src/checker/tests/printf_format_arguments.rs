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

// Phase 69: real PHPStan's `PrintfParametersRule` flags `sprintf`/
// `printf` unconditionally at level 0 for an exact placeholder/argument
// count mismatch - "too many" is a real, reachable bug (e.g. a stray
// extra argument left over from a refactor), not something real PHP's
// runtime leniency should suppress in static analysis.
#[test]
fn sprintf_flags_too_many_arguments_for_its_own_placeholders() {
    let d = diagnostics_for(r#"<?php sprintf("%s", "a", "b", "c");"#);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("expects 1 argument(s), 3 given"),
        "got: {d:?}"
    );
}

#[test]
fn printf_flags_too_many_arguments_for_its_own_placeholders() {
    let d = diagnostics_for(r#"<?php printf("%s", "a", "b");"#);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message.contains("expects 1 argument(s), 2 given"),
        "got: {d:?}"
    );
}

// `fprintf` has no backing real-PHPStan rule at all (absent from both
// `PrintfParametersRule::FORMAT_ARGUMENT_POSITIONS` and
// `PrintfArrayParametersRule`'s name list) - this project's own support
// for it is a disclosed extension, kept deliberately conservative at
// "too few only" so it never goes stricter than anything real PHPStan
// actually verified.
#[test]
fn fprintf_still_does_not_flag_extra_arguments() {
    let d = diagnostics_for(
        r#"<?php
            function f($stream) {
                fprintf($stream, "%s", "a", "b", "c");
            }
        "#,
    );
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

// Phase 50: a non-trailing optional field means the shape is only
// `Maybe`-a-list (real PHPStan's own `ConstantArrayTypeBuilder`
// semantics) - if the optional field is genuinely absent, the remaining
// keys aren't sequential from zero at all. vsprintf/vprintf's own
// arg-count check must stay silent here (favoring a false negative over
// a false positive), not assume the shape is list-like the way it used
// to (key-index matching alone, ignoring each field's own `optional`
// flag).

#[test]
fn vsprintf_with_a_non_trailing_optional_shape_field_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{0: string, 1?: int, 2: string} $args */
            function f(array $args) {
                vsprintf('%s %d %s', $args);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "arguments.count"),
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

// Phase 69: vsprintf/vprintf now require an exact count match too,
// mirroring real PHPStan's own `PrintfArrayParametersRule` (which
// compares the placeholder count against the literal array's own size
// with `===`, not just a "too few" lower bound).
#[test]
fn vsprintf_flags_too_many_array_elements_for_its_own_placeholders() {
    let d = diagnostics_for(r#"<?php vsprintf('%s', ["a", "b"]);"#);
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
        "got: {d:?}"
    );
}

// Phase 69: `%h`/`%H` (locale-independent decimal-point float formatting)
// are real PHP 8.0+ specifiers. With no target version configured, they're
// accepted (this project's "no target configured, don't restrict"
// convention) - and, like the other float-bucket specifiers, flagged
// against an array/object argument.
#[test]
fn sprintf_accepts_h_specifier_with_no_target_version_configured() {
    let d = diagnostics_for(r#"<?php sprintf("%h", 3.5);"#);
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_flags_an_array_for_the_h_specifier() {
    let d = diagnostics_for(
        r#"<?php
            function f(array $arr) {
                sprintf("%H", $arr);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn sprintf_rejects_h_specifier_below_php_8_0() {
    // Below the real threshold, `%h` isn't a recognized PHP specifier at
    // all - the whole format string parse bails ("don't guess"), so no
    // diagnostic at all rather than a wrong one.
    let d = diagnostics_for_with(r#"<?php sprintf("%h and %s", 3.5, "only one");"#, |r| {
        r.php_version = Some((7, 4))
    });
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn sprintf_flags_h_specifier_count_mismatch_on_php_8_0_or_later() {
    let d = diagnostics_for_with(r#"<?php sprintf("%h and %h", 3.5);"#, |r| {
        r.php_version = Some((8, 0))
    });
    assert!(
        d.iter().any(|x| x.identifier == "arguments.count"),
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

// Phase 69: opt-in strict printf placeholder-type matching
// (`check_strict_printf_placeholder_types`) - off by default, so none of
// these are flagged without the flag explicitly enabled.

#[test]
fn a_float_for_d_specifier_is_never_flagged_in_default_castable_mode() {
    let d = diagnostics_for(
        r#"<?php
            function f(float $x) {
                sprintf("%d", $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_float_for_d_specifier_is_flagged_in_strict_mode() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(float $x) {
                sprintf("%d", $x);
            }
        "#,
        |r| r.check_strict_printf_placeholder_types = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn an_int_for_f_specifier_is_still_accepted_in_strict_mode() {
    // Real PHPStan's own `FloatType::accepts` treats int as acceptable
    // (PHP implicitly widens int to float) - strict mode must not flag
    // this, only the genuinely wrong cases.
    let d = diagnostics_for_with(
        r#"<?php
            function f(int $x) {
                sprintf("%f", $x);
            }
        "#,
        |r| r.check_strict_printf_placeholder_types = true,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_decimal_int_string_literal_is_accepted_for_d_in_strict_mode() {
    let d = diagnostics_for_with(r#"<?php sprintf("%d", "123");"#, |r| {
        r.check_strict_printf_placeholder_types = true
    });
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_non_numeric_string_literal_is_flagged_for_d_in_strict_mode() {
    let d = diagnostics_for_with(r#"<?php sprintf("%d", "abc");"#, |r| {
        r.check_strict_printf_placeholder_types = true
    });
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_bool_for_s_specifier_is_flagged_in_strict_mode_but_not_by_default() {
    let default_mode = diagnostics_for(
        r#"<?php
            function f(bool $x) {
                sprintf("%s", $x);
            }
        "#,
    );
    assert!(default_mode.is_empty(), "unexpected: {default_mode:?}");

    let strict_mode = diagnostics_for_with(
        r#"<?php
            function f(bool $x) {
                sprintf("%s", $x);
            }
        "#,
        |r| r.check_strict_printf_placeholder_types = true,
    );
    assert!(
        strict_mode.iter().any(|x| x.identifier == "argument.type"),
        "got: {strict_mode:?}"
    );
}

#[test]
fn a_null_for_s_specifier_is_still_accepted_in_strict_mode() {
    // Real PHPStan's own strict-mode union explicitly includes `null`
    // ("consistency with phpstan-strict-rules") despite the surrounding
    // "don't accept null or bool" comment applying only to bool.
    let d = diagnostics_for_with(
        r#"<?php
            function f(?string $x) {
                sprintf("%s", null);
            }
        "#,
        |r| r.check_strict_printf_placeholder_types = true,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
