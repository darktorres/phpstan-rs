//! `Diagnostic` carries `identifier`/`tip`/`non_ignorable`/
//! `fix` fields, and `DiagnosticBuilder` is the fluent way to assemble
//! one. These tests exercise both halves: that real checker call sites
//! actually populate `identifier` (and, for the couple of call sites
//! that attach one, `tip`) correctly, and that `DiagnosticBuilder`
//! itself behaves as a plain, independently-testable unit.

use crate::diagnostic::DiagnosticBuilder;
use syntax::token::{Position, Span};

fn dummy_span() -> Span {
    Span {
        start: Position { line: 1, col: 1 },
        end: Position { line: 1, col: 1 },
    }
}

#[test]
fn diagnostic_builder_assembles_every_field() {
    // A direct, checker-independent unit test of the builder itself -
    // the Rust equivalent of real PHPStan's own
    // `RuleErrorBuilder::message(...)->identifier(...)->tip(...)->build()`
    // chain.
    let diag = DiagnosticBuilder::new(
        "something went wrong".to_string(),
        dummy_span(),
        5,
        "my.identifier",
    )
    .tip("try this instead")
    .non_ignorable()
    .fix(dummy_span(), "replacement text", "swap it out")
    .build();

    assert_eq!(diag.message, "something went wrong");
    assert_eq!(diag.level, 5);
    assert_eq!(diag.identifier, "my.identifier");
    assert_eq!(diag.tip.as_deref(), Some("try this instead"));
    assert!(diag.non_ignorable);
    let fix = diag.fix.expect("expected a fix to have been set");
    assert_eq!(fix.replacement, "replacement text");
    assert_eq!(fix.description, "swap it out");
}

// PHP 8.5's `#[\NoDiscard]` marker - `Checker::check_no_discard_statement`,
// mirroring real PHPStan's own `CallToFunctionStatementWithNoDiscardRule`/
// `CallToMethodStatementWithNoDiscardRule`/
// `CallToStaticMethodStatementWithNoDiscardRule`.

#[test]
fn diagnostic_builder_defaults_are_all_empty_absent_or_false() {
    // The other half of the above: a builder with only the required
    // fields set should never spuriously populate any of the optional
    // ones.
    let diag = DiagnosticBuilder::new(
        "plain message".to_string(),
        dummy_span(),
        0,
        "plain.identifier",
    )
    .build();

    assert!(diag.tip.is_none());
    assert!(!diag.non_ignorable);
    assert!(diag.fix.is_none());
}
