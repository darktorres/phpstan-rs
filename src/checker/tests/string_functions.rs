//! Built-in string function return type inference (str_replace, sprintf, explode, etc).

use super::helpers::*;

// `str_replace`/`preg_replace`'s native stub return types are
// unconditionally unioned with `array`, even though the function only
// returns an array when $subject itself is one.
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

// Concatenating any string with a non-falsy string literal (on either
// side) always produces a non-falsy-string, regardless of the other
// side's type.
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

// The identical `non-falsy-string` inference this file's Concat tests
// above cover must also apply to interpolated strings:
// `ExprKind::Interpolated` must not return bare `Type::String`
// unconditionally when one of its parts is a non-falsy literal segment -
// the same fact `"prefix-" . $unknown` already proves via Concat.
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

// sprintf()/vsprintf() must narrow to `non-falsy-string` whenever the
// format string's literal (non-specifier) text is itself provably
// non-falsy, the same reasoning Concat/Interpolated already use.
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

// `var_export($x, true)` must not stay typed as the native/stub
// signature's unconditional `?string` - a literal `true` second argument
// means it always returns the representation string instead of printing
// to stdout and returning null.
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

// Same subject-dependent-return-type family as `str_replace` above -
// `substr_replace`'s subject is argument index 0, not 2.
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

// `str_repeat()` must not fall back to a bare `string` (matching the
// native stub) with no special-case handling. Mirrors PHPStan's
// `StrRepeatFunctionReturnTypeExtension`: constant-folds a small
// literal-input/literal-count call outright, and otherwise still marks
// the result `non-falsy-string` when the input is itself non-falsy and
// the count
// is provably `>= 1`.
#[test]
fn str_repeat_folds_small_literals_and_marks_non_falsy_when_provable() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(int $n): void {
                assertType('non-falsy-string', str_repeat(' ', 300));
                assertType("'abab'", str_repeat('ab', 2));
                assertType("''", str_repeat(' ', 0));
                assertType('string', str_repeat(' ', $n));
                assertType("''", str_repeat('', 5));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}
