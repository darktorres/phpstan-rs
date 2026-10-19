//! `exclude_paths`: how a configuration's entries read, and which paths they
//! match.

use super::super::exclude_paths::{Entry, ExcludePaths, Exclusions};
use super::super::session::SessionSettings;
use super::helpers::{TempDir, resolved};
use config::ConfigFile;
use diagnostics::CaseRule;
use std::path::PathBuf;

/// The `exclude_paths` setting `text` (a config file's text) reads as.
fn setting(text: &str) -> ExcludePaths {
    let schema = config::Schema::new().section::<SessionSettings>();
    ConfigFile::parse(text, &schema)
        .unwrap()
        .section::<SessionSettings>()
        .unwrap()
        .exclude_paths
}

/// The exclusions `text` names, resolved against `dir`, with the errors.
fn resolve(dir: &TempDir, text: &str) -> (Exclusions, Vec<String>) {
    let base = resolved(dir);
    Exclusions::resolve(&setting(text), &base, &|path: &std::path::Path| {
        path.exists()
    })
}

/// `rel` below `dir`, resolved.
fn path(dir: &TempDir, rel: &str) -> PathBuf {
    resolved(dir).join(rel)
}

#[test]
fn both_spellings_parse() {
    assert_eq!(
        setting("exclude_paths = [\"a\", { path = \"b\", optional = true }]"),
        ExcludePaths::List(vec![
            Entry::Plain("a".into()),
            Entry::Table {
                path: "b".into(),
                optional: true,
            },
        ])
    );
    assert_eq!(
        setting("[exclude_paths]\nanalyse = [\"a\"]\nanalyse_and_scan = [\"b\"]\n"),
        ExcludePaths::Table {
            analyse: vec![Entry::Plain("a".into())],
            analyse_and_scan: vec![Entry::Plain("b".into())],
        }
    );
    assert_eq!(setting(""), ExcludePaths::default());
}

#[test]
fn a_directory_entry_leaves_out_what_is_below_it_and_not_its_neighbours() {
    let dir = TempDir::new("exclude_prefix");
    dir.write("gen/x.php", "<?php");
    dir.write("gen-old/y.php", "<?php");
    dir.write("keep.php", "<?php");
    let (exclusions, errors) = resolve(&dir, "exclude_paths = [\"gen\", \"keep.php\"]");
    assert!(errors.is_empty(), "{errors:?}");
    let case = CaseRule::Exact;
    assert!(exclusions.excludes_scan(&path(&dir, "gen"), case, None));
    assert!(exclusions.excludes_scan(&path(&dir, "gen/x.php"), case, None));
    assert!(!exclusions.excludes_scan(&path(&dir, "gen-old/y.php"), case, None));
    assert!(exclusions.excludes_scan(&path(&dir, "keep.php"), case, None));
}

#[test]
fn analyse_leaves_a_path_out_of_the_analysis_but_not_the_scan() {
    let dir = TempDir::new("exclude_analyse");
    dir.write("gen/x.php", "<?php");
    dir.write("cache/y.php", "<?php");
    let (exclusions, errors) = resolve(
        &dir,
        "[exclude_paths]\nanalyse = [\"gen\"]\nanalyse_and_scan = [\"cache\"]\n",
    );
    assert!(errors.is_empty(), "{errors:?}");
    let case = CaseRule::Exact;
    assert!(exclusions.excludes_analysis(&path(&dir, "gen/x.php"), case, None));
    assert!(!exclusions.excludes_scan(&path(&dir, "gen/x.php"), case, None));
    assert!(exclusions.excludes_analysis(&path(&dir, "cache/y.php"), case, None));
    assert!(exclusions.excludes_scan(&path(&dir, "cache/y.php"), case, None));
}

#[test]
fn a_flat_list_leaves_paths_out_of_the_analysis_and_the_scan() {
    let dir = TempDir::new("exclude_flat");
    dir.write("gen/x.php", "<?php");
    let (exclusions, errors) = resolve(&dir, "exclude_paths = [\"gen\"]");
    assert!(errors.is_empty(), "{errors:?}");
    assert!(exclusions.excludes_scan(&path(&dir, "gen/x.php"), CaseRule::Exact, None));
}

#[test]
fn a_pattern_crosses_directories_and_matches_its_directory() {
    let dir = TempDir::new("exclude_glob");
    dir.write("x/y/Event/z.php", "<?php");
    dir.write("vendor-old/a.php", "<?php");
    dir.write("vendor/a.php", "<?php");
    let (exclusions, errors) = resolve(&dir, "exclude_paths = [\"*/Event\", \"vendor-*\"]");
    assert!(errors.is_empty(), "{errors:?}");
    let case = CaseRule::Exact;
    assert!(exclusions.excludes_scan(&path(&dir, "x/y/Event/z.php"), case, None));
    assert!(exclusions.excludes_scan(&path(&dir, "vendor-old/a.php"), case, None));
    assert!(!exclusions.excludes_scan(&path(&dir, "vendor/a.php"), case, None));
}

#[test]
fn a_missing_literal_path_is_an_error_and_an_optional_one_is_not() {
    let dir = TempDir::new("exclude_missing");
    dir.write("gen/x.php", "<?php");
    let (exclusions, errors) = resolve(&dir, "exclude_paths = [\"nope\", \"gen\"]");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("nope"), "{errors:?}");
    assert!(exclusions.excludes_scan(&path(&dir, "gen/x.php"), CaseRule::Exact, None));

    let (exclusions, errors) = resolve(
        &dir,
        "exclude_paths = [{ path = \"nope\", optional = true }]",
    );
    assert!(errors.is_empty(), "{errors:?}");
    assert!(!exclusions.excludes_scan(&path(&dir, "gen/x.php"), CaseRule::Exact, None));
}

#[test]
fn an_empty_entry_is_an_error() {
    let dir = TempDir::new("exclude_empty");
    let (_, errors) = resolve(&dir, "exclude_paths = [\"\"]");
    assert_eq!(errors.len(), 1, "{errors:?}");
}

#[test]
fn letters_compare_as_the_root_compares_names() {
    let dir = TempDir::new("exclude_case");
    dir.write("Gen/x.php", "<?php");
    let (exclusions, errors) = resolve(&dir, "exclude_paths = [\"Gen\"]");
    assert!(errors.is_empty(), "{errors:?}");
    let file = path(&dir, "gen/x.php");
    assert!(!exclusions.excludes_scan(&file, CaseRule::Exact, None));
    assert!(exclusions.excludes_scan(&file, CaseRule::Folded, None));
}
