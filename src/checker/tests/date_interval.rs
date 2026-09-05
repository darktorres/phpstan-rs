//! Phase 78 (partial slice): `DateInterval`'s own ISO-8601 duration-
//! string constructor validation. Every case here was confirmed
//! directly against real `php` (8.5) before being encoded - see
//! `src/checker/date_interval.rs`'s own module doc for the full
//! grammar-derivation notes.

use super::helpers::*;

fn is_malformed(spec: &str) -> bool {
    let d = diagnostics_for(&format!(r#"<?php new DateInterval('{spec}');"#));
    d.iter().any(|x| x.identifier == "argument.type")
}

#[test]
fn valid_designator_specs_are_never_flagged() {
    for spec in [
        "P1Y",
        "P1Y2M3D",
        "PT1H",
        "P1YT1H",
        "P1W",
        "P1Y2W",
        "P1Y2M3DT4H5M6S",
        "P0D",
        "P1Y2M3W4DT5H6M7S",
        "P1M2W",
        "P1W2D",
        "P2M1W",
        "P01Y",
        "P1Y2M3W",
        "PT1H2M3S",
        "P10000Y",
    ] {
        assert!(!is_malformed(spec), "{spec} should be valid");
    }
}

#[test]
fn malformed_designator_specs_are_flagged() {
    for spec in [
        "P", "PT", "P1", "P1D2Y", "PY", "P1.5Y", "P-1Y", "P1YY", "P1S", "PT1D", "P1DT", "P1D2W",
        "P1W2M", "P1Y1Y", "P+1Y", "PT1M2H", "PT1S2M",
    ] {
        assert!(is_malformed(spec), "{spec} should be malformed");
    }
}

#[test]
fn lowercase_p_is_flagged() {
    // Case-sensitive: PHP itself rejects a lowercase `p1y`.
    assert!(is_malformed("p1y"));
}

#[test]
fn the_alternate_iso_datetime_form_is_recognized() {
    assert!(!is_malformed("P0001-02-03T04:05:06"));
    assert!(!is_malformed("P0000-00-01T00:00:00"));
}

#[test]
fn the_iso_datetime_form_requires_exact_digit_widths_and_a_time_suffix() {
    for spec in ["P2018-01-01", "P1-1-1", "P01-02-03T04:05:06"] {
        assert!(is_malformed(spec), "{spec} should be malformed");
    }
}

#[test]
fn whitespace_around_a_valid_spec_is_tolerated() {
    assert!(!is_malformed(" P1Y"));
    assert!(!is_malformed("P1Y "));
}

#[test]
fn a_computed_spec_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $spec): void {
                new DateInterval($spec);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_named_or_spread_argument_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            new DateInterval(duration: 'garbage');
            $args = ['garbage'];
            new DateInterval(...$args);
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn other_classes_own_constructors_are_never_flagged_by_this_check() {
    let d = diagnostics_for(
        r#"<?php
            class DateIntervalLookalike {
                public function __construct(string $x) {}
            }
            new DateIntervalLookalike('garbage');
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
        "unexpected diagnostics: {d:?}"
    );
}
