//! `filter`'s own criteria-combining and suppression behavior.

use super::super::*;
use super::helpers::*;

#[test]
fn identifier_only_drops_a_matching_diagnostic_and_keeps_a_non_matching_one() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("a.php"),
        vec![
            diag("missingType.iterableValue", "no value type specified"),
            diag("argument.type", "expects int, string given"),
        ],
    );
    let entries = vec![IgnoreErrorEntry {
        identifier: Some("missingType.iterableValue".to_string()),
        ..Default::default()
    }];
    filter(&mut results, &entries);
    let remaining = &results[&PathBuf::from("a.php")];
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].identifier, "argument.type");
}

#[test]
fn identifier_and_path_are_and_combined_not_or() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("_analysis/phpstan-latte/foo.php"),
        vec![diag("missingType.iterableValue", "no value type specified")],
    );
    results.insert(
        PathBuf::from("src/Piwigo/Foo.php"),
        vec![diag("missingType.iterableValue", "no value type specified")],
    );
    let entries = vec![IgnoreErrorEntry {
        identifier: Some("missingType.iterableValue".to_string()),
        path: Some("_analysis/phpstan-latte/*".to_string()),
        ..Default::default()
    }];
    filter(&mut results, &entries);
    assert!(results[&PathBuf::from("_analysis/phpstan-latte/foo.php")].is_empty());
    assert_eq!(results[&PathBuf::from("src/Piwigo/Foo.php")].len(), 1);
}

#[test]
fn a_message_regex_only_matches_the_intended_message() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("a.latte.php"),
        vec![
            diag(
                "booleanAnd.rightAlwaysTrue",
                "Only booleans are allowed in a negated boolean, Latte\\Runtime\\Template|null given.",
            ),
            diag("booleanAnd.rightAlwaysTrue", "Only booleans are allowed in a negated boolean, int|false given."),
        ],
    );
    let entries = vec![IgnoreErrorEntry {
        message: Some(
            r"^Only booleans are allowed in a negated boolean, Latte\\Runtime\\Template\|null given\.$"
                .to_string(),
        ),
        ..Default::default()
    }];
    filter(&mut results, &entries);
    let remaining = &results[&PathBuf::from("a.latte.php")];
    assert_eq!(remaining.len(), 1);
    assert!(remaining[0].message.contains("int|false"));
}

#[test]
fn non_ignorable_diagnostics_are_never_dropped() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("a.php"),
        vec![non_ignorable_diag("class.duplicate", "duplicate class Foo")],
    );
    let entries = vec![IgnoreErrorEntry {
        identifier: Some("class.duplicate".to_string()),
        ..Default::default()
    }];
    filter(&mut results, &entries);
    assert_eq!(results[&PathBuf::from("a.php")].len(), 1);
}

#[test]
fn two_entries_each_only_catch_their_own_shape() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("_analysis/phpstan-latte/foo.php"),
        vec![
            diag("missingType.iterableValue", "no value type specified"),
            diag("variable.undefined", "undefined variable $x"),
            diag("argument.type", "expects int, string given"),
        ],
    );
    let entries = vec![
        IgnoreErrorEntry {
            identifier: Some("missingType.iterableValue".to_string()),
            path: Some("_analysis/phpstan-latte/*".to_string()),
            ..Default::default()
        },
        IgnoreErrorEntry {
            identifier: Some("variable.undefined".to_string()),
            path: Some("_analysis/phpstan-latte/*".to_string()),
            ..Default::default()
        },
    ];
    filter(&mut results, &entries);
    let remaining = &results[&PathBuf::from("_analysis/phpstan-latte/foo.php")];
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].identifier, "argument.type");
}

#[cfg(windows)]
#[test]
fn a_path_matches_in_either_separator_on_windows() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("generated\\cache\\a.php"),
        vec![diag("variable.undefined", "Undefined variable: $x")],
    );
    results.insert(
        PathBuf::from("generated\\b.php"),
        vec![diag("variable.undefined", "Undefined variable: $x")],
    );
    let entries = vec![
        IgnoreErrorEntry {
            path: Some("generated/cache/*".to_string()),
            ..Default::default()
        },
        IgnoreErrorEntry {
            path: Some("generated\\b.php".to_string()),
            ..Default::default()
        },
    ];
    filter(&mut results, &entries);
    assert!(results[&PathBuf::from("generated\\cache\\a.php")].is_empty());
    assert!(results[&PathBuf::from("generated\\b.php")].is_empty());
}

#[test]
fn a_backslash_in_a_path_pattern_is_a_separator() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("generated/cache/a.php"),
        vec![diag("variable.undefined", "Undefined variable: $x")],
    );
    let entries = vec![IgnoreErrorEntry {
        path: Some("generated\\cache\\*".to_string()),
        ..Default::default()
    }];
    filter(&mut results, &entries);
    assert!(results[&PathBuf::from("generated/cache/a.php")].is_empty());
}

#[cfg(unix)]
#[test]
fn a_backslash_in_a_path_pattern_also_matches_one_in_a_name_outside_windows() {
    let undefined = || vec![diag("variable.undefined", "Undefined variable: $x")];
    let mut results = FxHashMap::default();
    results.insert(PathBuf::from("generated\\a.php"), undefined());
    results.insert(PathBuf::from("generated/b.php"), undefined());
    let entries = vec![IgnoreErrorEntry {
        path: Some("generated\\*".to_string()),
        ..Default::default()
    }];
    filter(&mut results, &entries);
    assert!(results[&PathBuf::from("generated\\a.php")].is_empty());
    assert!(results[&PathBuf::from("generated/b.php")].is_empty());

    let mut results = FxHashMap::default();
    results.insert(PathBuf::from("generated\\a.php"), undefined());
    let entries = vec![IgnoreErrorEntry {
        path: Some("generated/*".to_string()),
        ..Default::default()
    }];
    filter(&mut results, &entries);
    assert_eq!(results[&PathBuf::from("generated\\a.php")].len(), 1);
}

#[test]
fn no_entries_at_all_changes_nothing() {
    let mut results = FxHashMap::default();
    results.insert(
        PathBuf::from("a.php"),
        vec![diag("argument.type", "expects int, string given")],
    );
    filter(&mut results, &[]);
    assert_eq!(results[&PathBuf::from("a.php")].len(), 1);
}
