use crate::{DiagnosticBuilder, Identifier, LevelCutoff};
use syntax::span::{LineIndex, Span};

fn dummy_span() -> Span {
    Span::default()
}

fn index() -> LineIndex {
    LineIndex::new(b"<?php\n")
}

#[test]
fn diagnostic_builder_assembles_every_field() {
    let diag = DiagnosticBuilder::new(
        "something went wrong".to_string(),
        dummy_span(),
        5,
        "my.identifier",
    )
    .found_from(8)
    .found_below(9)
    .tip("try this instead")
    .non_ignorable()
    .fix(dummy_span(), "replacement text", "swap it out")
    .build(&index());

    assert_eq!(diag.message, "something went wrong");
    assert_eq!(diag.level, 5);
    assert_eq!(diag.found_from, 8);
    assert_eq!(diag.found_below, Some(9));
    assert_eq!(diag.reporting_level(), 8);
    assert_eq!(diag.identifier, "my.identifier");
    assert_eq!(diag.tips, vec!["try this instead".to_string()]);
    assert!(diag.non_ignorable);
    let fix = diag.fix.expect("expected a fix to have been set");
    assert_eq!(fix.replacement, "replacement text");
    assert_eq!(fix.description, "swap it out");
}

#[test]
fn diagnostic_builder_defaults_are_all_empty_absent_or_false() {
    let diag = DiagnosticBuilder::new(
        "plain message".to_string(),
        dummy_span(),
        0,
        "plain.identifier",
    )
    .build(&index());

    assert_eq!(diag.found_from, 0);
    assert_eq!(diag.found_below, None);
    assert!(diag.tips.is_empty());
    assert!(!diag.non_ignorable);
    assert!(diag.fix.is_none());
}

#[test]
fn several_tips_show_as_a_bulleted_list() {
    let one = DiagnosticBuilder::new("m".to_string(), dummy_span(), 0, "a.b")
        .tip("first")
        .build(&index());
    assert_eq!(one.tip_text().as_deref(), Some("first"));
    let two = DiagnosticBuilder::new("m".to_string(), dummy_span(), 0, "a.b")
        .tip("first")
        .tip("second")
        .build(&index());
    assert_eq!(two.tip_text().as_deref(), Some("• first\n• second"));
}

#[test]
fn an_identifier_is_dot_separated_alphanumeric_segments() {
    assert!(Identifier::is_valid("argument.type"));
    assert!(Identifier::is_valid("phpstanApi.class"));
    assert!(Identifier::is_valid("ignore"));
    assert!(!Identifier::is_valid(""));
    assert!(!Identifier::is_valid("argument..type"));
    assert!(!Identifier::is_valid(".type"));
    assert!(!Identifier::is_valid("argument.type."));
    assert!(!Identifier::is_valid("argument-type"));
    assert!(!Identifier::is_valid("argument type"));
}

#[test]
fn a_cutoff_shows_a_finding_at_its_rule_level_and_the_levels_it_is_made_at() {
    let finding = |found_from, found_below: Option<u8>| {
        let builder = DiagnosticBuilder::new("m", dummy_span(), 5, "a.b").found_from(found_from);
        match found_below {
            Some(below) => builder.found_below(below),
            None => builder,
        }
        .build(&index())
    };
    let everywhere = finding(0, None);
    let from_eight = finding(8, None);
    let below_nine = finding(0, Some(9));
    let shown = |cutoff: LevelCutoff| {
        [&everywhere, &from_eight, &below_nine].map(|finding| cutoff.reports(finding))
    };
    assert_eq!(shown(LevelCutoff::at(4)), [false; 3]);
    assert_eq!(shown(LevelCutoff::at(5)), [true, false, true]);
    assert_eq!(shown(LevelCutoff::at(8)), [true, true, true]);
    assert_eq!(shown(LevelCutoff::at(9)), [true, true, false]);
    assert_eq!(shown(LevelCutoff { rule: 6, found: 8 }), [true, true, true]);
}
