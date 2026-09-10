//! Phase 78: `DateTimeInterface::format()`/`DateInterval::format()`
//! return-type wiring, `DateTimeInterface::diff()` narrowing, and
//! `SimpleXMLElement::asXML()`'s real arity-dependent overload split -
//! all verified directly against real PHPStan source
//! (`DateFunctionReturnTypeHelper`, `DateIntervalFormatReturnTypeHelper`,
//! `SimpleXMLElementAsXMLMethodReturnTypeExtension`).

use super::helpers::*;

#[test]
fn datetime_format_with_an_all_numeric_literal_format_narrows_to_a_numeric_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $s */
            function sink(string $s): void {}
            function f(\DateTime $dt): void {
                sink($dt->format('Ymd'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn datetimeimmutable_format_also_gets_the_same_wiring() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $s */
            function sink(string $s): void {}
            function f(\DateTimeImmutable $dt): void {
                sink($dt->format('His'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn datetime_format_with_a_non_numeric_literal_format_is_still_non_empty_but_not_numeric() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-string $s */
            function sink(string $s): void {}
            function f(\DateTime $dt): void {
                sink($dt->format('lY'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let numeric_sink = diagnostics_for(
        r#"<?php
            /** @param numeric-string $s */
            function sink(string $s): void {}
            function f(\DateTime $dt): void {
                sink($dt->format('lY'));
            }
        "#,
    );
    assert!(
        numeric_sink.iter().any(|x| x.identifier == "argument.type"),
        "got: {numeric_sink:?}"
    );
}

#[test]
fn datetime_format_with_a_non_literal_format_keeps_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $s */
            function sink(string $s): void {}
            function f(\DateTime $dt, string $fmt): void {
                sink($dt->format($fmt));
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

// DateInterval::format() - fixed-width specifiers (%Y/%M/%D/%H/%I/%S/%F,
// %R, %a) always produce non-falsy text, genuinely for every real
// interval magnitude (see `date_interval_format_return_type`'s own doc
// comment for why this is sound, not just an approximation).

#[test]
fn dateinterval_format_with_a_fixed_width_specifier_narrows_to_non_falsy_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $s */
            function sink(string $s): void {}
            function f(\DateInterval $i): void {
                sink($i->format('%Y years'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn dateinterval_format_with_only_a_bare_d_specifier_is_not_narrowed_to_non_falsy() {
    // Real PHP: a zero-day interval's `%d` is the literal "0" - never
    // non-falsy, matching this project's representative-substitution
    // check exactly.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $s */
            function sink(string $s): void {}
            function f(\DateInterval $i): void {
                sink($i->format('%d'));
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );

    let non_empty_sink = diagnostics_for(
        r#"<?php
            /** @param non-empty-string $s */
            function sink(string $s): void {}
            function f(\DateInterval $i): void {
                sink($i->format('%d'));
            }
        "#,
    );
    assert!(
        non_empty_sink.is_empty(),
        "unexpected diagnostics: {non_empty_sink:?}"
    );
}

#[test]
fn dateinterval_format_with_an_unrecognized_specifier_keeps_plain_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-string $s */
            function sink(string $s): void {}
            function f(\DateInterval $i): void {
                sink($i->format('%Q'));
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

// DateTimeInterface::diff() narrows to DateInterval&object{days: int}.

#[test]
fn datetime_diff_result_days_property_is_int() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int $n */
            function sink(int $n): void {}
            function f(\DateTime $a, \DateTime $b): void {
                $diff = $a->diff($b);
                sink($diff->days);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn datetime_diff_result_is_still_a_real_dateinterval() {
    let d = diagnostics_for(
        r#"<?php
            function sink(\DateInterval $i): void {}
            function f(\DateTime $a, \DateTime $b): void {
                sink($a->diff($b));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// SimpleXMLElement::asXML() - exactly one argument (a filename) returns
// bool; zero arguments returns string|false. (The design doc's own
// paraphrase had this backwards - verified directly against real
// PHPStan's own SimpleXMLElementAsXMLMethodReturnTypeExtension source.)

#[test]
fn simplexmlelement_asxml_with_no_arguments_is_string_or_false() {
    let d = diagnostics_for(
        r#"<?php
            function sink(string|false $s): void {}
            function f(\SimpleXMLElement $x): void {
                sink($x->asXML());
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn simplexmlelement_asxml_with_no_arguments_is_not_plain_bool() {
    let d = diagnostics_for(
        r#"<?php
            function sink(bool $b): void {}
            function f(\SimpleXMLElement $x): void {
                sink($x->asXML());
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "got: {d:?}"
    );
}

#[test]
fn simplexmlelement_asxml_with_a_filename_argument_is_bool() {
    let d = diagnostics_for(
        r#"<?php
            function sink(bool $b): void {}
            function f(\SimpleXMLElement $x): void {
                sink($x->asXML('/tmp/out.xml'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
