//! `DateTime`/`DateTimeImmutable`'s own constructor-string validation.
//! Every case here was checked directly against real `php` (8.5) - see
//! `src/php_date_parser.rs`'s own module doc for the full
//! grammar-derivation notes.

use super::helpers::*;

fn is_malformed(class: &str, spec: &str) -> bool {
    let d = diagnostics_for(&format!(r#"<?php new {class}('{spec}');"#));
    d.iter().any(|x| x.identifier == "argument.type")
}

#[test]
fn valid_absolute_and_relative_strings_are_never_flagged() {
    for spec in [
        "2024-01-15",
        "2024-01-15T10:30:00",
        "2024-01-15 10:30:00",
        "2024-01-15T10:30:00Z",
        "2024-01-15T10:30:00+02:00",
        "@1700000000",
        "+1 day",
        "-30 days",
        "1 fortnight",
        "next monday",
        "last day of this month",
        "now",
        "today",
        "tomorrow",
        "midnight",
    ] {
        assert!(!is_malformed("DateTime", spec), "{spec} should be valid");
        assert!(
            !is_malformed("DateTimeImmutable", spec),
            "{spec} should be valid"
        );
    }
}

#[test]
fn conclusively_malformed_strings_are_flagged() {
    for spec in ["2024-13-01", "2024-01-32", "2024-01-15T25:00:00"] {
        assert!(is_malformed("DateTime", spec), "{spec} should be malformed");
    }
}

#[test]
fn an_unrecognized_shape_is_never_flagged() {
    // Not a shape this port recognizes at all (natural-language month
    // names) - stays silent rather than risk a false positive against
    // one of PHP's many uncovered formats.
    assert!(!is_malformed("DateTime", "January 15, 2024"));
}

#[test]
fn a_computed_string_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $spec): void {
                new DateTime($spec);
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
            new DateTime(datetime: '2024-13-01');
            $args = ['2024-13-01'];
            new DateTime(...$args);
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
            class DateTimeLookalike {
                public function __construct(string $x) {}
            }
            new DateTimeLookalike('2024-13-01');
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
        "unexpected diagnostics: {d:?}"
    );
}
