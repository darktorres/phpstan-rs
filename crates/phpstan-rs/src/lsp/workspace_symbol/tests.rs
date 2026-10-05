//! Unit tests for `workspace_symbols`, built against a real multi-file
//! `Project` loaded through a `Session` (`project::test_support::Files`).

use super::*;
use lsp_types::SymbolKind;

fn project(files: &[(&str, &str)]) -> Project {
    let mut loaded = crate::project::test_support::Files::new();
    for (name, src) in files {
        loaded = loaded.context(name, src);
    }
    loaded.load()
}

fn names<'a>(matches: &'a [Match<'a>]) -> Vec<&'a str> {
    matches.iter().map(|m| m.name.as_str()).collect()
}

#[test]
fn a_class_matches_by_case_insensitive_substring() {
    let p = project(&[("a.php", "<?php class FooBar {}")]);
    let matches = workspace_symbols(&p, "oob");
    assert_eq!(names(&matches), vec!["FooBar"]);
    assert_eq!(matches[0].kind, SymbolKind::CLASS);
}

#[test]
fn a_top_level_function_matches_and_reports_no_container() {
    let p = project(&[("a.php", "<?php function helperA() {}")]);
    let matches = workspace_symbols(&p, "helper");
    assert_eq!(names(&matches), vec!["helperA"]);
    assert_eq!(matches[0].kind, SymbolKind::FUNCTION);
    assert_eq!(matches[0].container_name, None);
}

#[test]
fn a_method_reports_its_declaring_class_fqn_as_container_name() {
    let p = project(&[("a.php", "<?php class Foo { function doStuff() {} }")]);
    let matches = workspace_symbols(&p, "dostuff");
    assert_eq!(names(&matches), vec!["doStuff"]);
    assert_eq!(matches[0].kind, SymbolKind::METHOD);
    assert_eq!(matches[0].container_name, Some("Foo".to_string()));
}

#[test]
fn a_property_and_a_const_get_distinct_kinds() {
    let p = project(&[(
        "a.php",
        "<?php class Foo { const BAR = 1; public int $baz; }",
    )]);
    let bar = workspace_symbols(&p, "bar");
    assert_eq!(names(&bar), vec!["BAR"]);
    assert_eq!(bar[0].kind, SymbolKind::CONSTANT);

    let baz = workspace_symbols(&p, "baz");
    assert_eq!(names(&baz), vec!["baz"]);
    assert_eq!(baz[0].kind, SymbolKind::PROPERTY);
}

#[test]
fn an_enum_case_is_reported_as_enum_member_not_constant() {
    let p = project(&[("a.php", "<?php enum Suit: string { case Hearts = 'H'; }")]);
    let matches = workspace_symbols(&p, "hearts");
    assert_eq!(names(&matches), vec!["Hearts"]);
    assert_eq!(matches[0].kind, SymbolKind::ENUM_MEMBER);
}

#[test]
fn an_interface_and_a_trait_report_their_own_class_kind() {
    let p = project(&[("a.php", "<?php interface Greets {} trait Helper {}")]);
    let greets = workspace_symbols(&p, "greets");
    assert_eq!(greets[0].kind, SymbolKind::INTERFACE);
    let helper = workspace_symbols(&p, "helper");
    assert_eq!(helper[0].kind, SymbolKind::CLASS);
}

#[test]
fn a_namespaced_class_reports_its_namespace_as_container_name() {
    let p = project(&[("a.php", "<?php namespace App; class Foo {}")]);
    let matches = workspace_symbols(&p, "foo");
    assert_eq!(names(&matches), vec!["Foo"]);
    assert_eq!(matches[0].container_name, Some("App".to_string()));
}

#[test]
fn a_query_that_only_matches_as_a_subsequence_still_matches() {
    let p = project(&[("a.php", "<?php class HelperClass {}")]);
    let matches = workspace_symbols(&p, "hlpcls");
    assert_eq!(names(&matches), vec!["HelperClass"]);
}

#[test]
fn a_query_with_no_match_at_all_returns_nothing() {
    let p = project(&[("a.php", "<?php class Foo {}")]);
    assert!(workspace_symbols(&p, "zzzzz").is_empty());
}

#[test]
fn prefix_matches_rank_above_substring_matches() {
    let p = project(&[("a.php", "<?php class FooBar {} class XxxFoo {}")]);
    let matches = workspace_symbols(&p, "foo");
    assert_eq!(matches[0].name, "FooBar");
}

#[test]
fn an_empty_query_matches_every_declared_symbol() {
    let p = project(&[("a.php", "<?php class Foo {} function bar() {} class Baz {}")]);
    let matches = workspace_symbols(&p, "");
    let mut found = names(&matches);
    found.sort_unstable();
    assert_eq!(found, vec!["Baz", "Foo", "bar"]);
}

#[test]
fn results_are_capped_at_the_maximum() {
    let mut src = String::from("<?php ");
    for i in 0..300 {
        src.push_str(&format!("function f{i}() {{}} "));
    }
    let p = project(&[("a.php", &src)]);
    let matches = workspace_symbols(&p, "");
    assert_eq!(matches.len(), 256);
}

#[test]
fn a_method_and_a_property_with_the_same_name_are_both_listed() {
    let p = project(&[(
        "a.php",
        "<?php class Foo { public $bar; function bar() {} }",
    )]);
    let matches = workspace_symbols(&p, "bar");
    let mut kinds: Vec<SymbolKind> = matches.iter().map(|m| m.kind).collect();
    kinds.sort_by_key(|k| format!("{k:?}"));
    assert_eq!(kinds, vec![SymbolKind::METHOD, SymbolKind::PROPERTY]);
}

#[test]
fn an_inherited_member_is_listed_under_its_declaring_class_only() {
    let p = project(&[(
        "a.php",
        "<?php class P { function run() {} } class C extends P {}",
    )]);
    let matches = workspace_symbols(&p, "run");
    assert_eq!(names(&matches), vec!["run"]);
    assert_eq!(matches[0].container_name, Some("P".to_string()));
}

#[test]
fn a_namespaced_classs_member_reports_the_owners_full_fqn_as_container_name() {
    let p = project(&[(
        "a.php",
        "<?php namespace App; class Foo { function doStuff() {} }",
    )]);
    let matches = workspace_symbols(&p, "dostuff");
    assert_eq!(names(&matches), vec!["doStuff"]);
    assert_eq!(
        matches[0].container_name,
        Some("App\\Foo".to_string()),
        "must be the owner's full FQN, not its short name"
    );
}
