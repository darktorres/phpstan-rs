//! A configured root on a directory that folds case is spelled as the
//! listings of its directories spell it. The rule is injected, so a case
//! sensitive disk shows what a folding one gives.

use super::super::path_spelling::as_listed;
use super::helpers::{TempDir, resolved};
use composer::dir_listings::DirListings;
use diagnostics::CaseRule;
use std::path::{Path, PathBuf};

fn listed(dir: &TempDir, path: &str, case: CaseRule) -> PathBuf {
    as_listed(
        &resolved(dir),
        Path::new(path),
        case,
        &DirListings::default(),
    )
}

#[test]
fn an_exact_rule_gives_the_path_as_it_is() {
    let dir = TempDir::new("spelling_exact");
    dir.write("src/Http/Router.php", "<?php\n");
    assert_eq!(
        listed(&dir, "SRC/http", CaseRule::Exact),
        PathBuf::from("SRC/http")
    );
}

#[test]
fn a_folded_rule_spells_each_name_as_its_directory_lists_it() {
    let dir = TempDir::new("spelling_folded");
    dir.write("src/Http/Router.php", "<?php\n");
    assert_eq!(
        listed(&dir, "SRC/http/router.php", CaseRule::Folded),
        PathBuf::from("src/Http/Router.php")
    );
    assert_eq!(
        listed(&dir, "src/Http", CaseRule::Folded),
        PathBuf::from("src/Http")
    );
}

#[test]
fn an_absolute_path_below_the_base_keeps_the_base_and_is_spelled_below_it() {
    let dir = TempDir::new("spelling_absolute");
    dir.write("src/Http/Router.php", "<?php\n");
    let base = resolved(&dir);
    assert_eq!(
        as_listed(
            &base,
            &base.join("Src/HTTP"),
            CaseRule::Folded,
            &DirListings::default()
        ),
        base.join("src/Http")
    );
}

#[test]
fn an_absolute_path_outside_the_base_is_as_it_is() {
    let dir = TempDir::new("spelling_outside");
    dir.write("src/a.php", "<?php\n");
    let elsewhere = if cfg!(windows) {
        "C:\\Other\\SRC"
    } else {
        "/other/SRC"
    };
    assert_eq!(
        as_listed(
            &resolved(&dir),
            Path::new(elsewhere),
            CaseRule::Folded,
            &DirListings::default()
        ),
        PathBuf::from(elsewhere)
    );
}

#[test]
fn a_name_the_listing_does_not_hold_ends_the_spelling() {
    let dir = TempDir::new("spelling_missing");
    dir.write("src/Http/Router.php", "<?php\n");
    assert_eq!(
        listed(&dir, "SRC/Missing/Router.PHP", CaseRule::Folded),
        PathBuf::from("src/Missing/Router.PHP")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_name_held_in_two_cases_is_the_exact_one_or_none() {
    let dir = TempDir::new("spelling_two_cases");
    dir.write("a/x.php", "<?php\n");
    dir.write("A/y.php", "<?php\n");
    // Two entries that differ only in case: the directory keeps case. A name
    // that one of them has exactly is that one, and no other spelling is
    // guessed at.
    assert!(resolved(&dir).join("a").exists() && resolved(&dir).join("A").exists());
    assert_eq!(listed(&dir, "a", CaseRule::Folded), PathBuf::from("a"));
    assert_eq!(listed(&dir, "A", CaseRule::Folded), PathBuf::from("A"));
    assert_eq!(
        listed(&dir, "a/X.php", CaseRule::Folded),
        PathBuf::from("a/x.php")
    );
}

#[test]
fn a_component_that_is_not_a_name_ends_the_spelling() {
    let dir = TempDir::new("spelling_parent");
    dir.write("src/Http/Router.php", "<?php\n");
    dir.write("lib/Util.php", "<?php\n");
    assert_eq!(
        listed(&dir, "SRC/../LIB/util.php", CaseRule::Folded),
        PathBuf::from("src/../LIB/util.php")
    );
}

#[cfg(unix)]
#[test]
fn a_link_ends_the_spelling() {
    let dir = TempDir::new("spelling_link");
    dir.write("real/Inner/a.php", "<?php\n");
    std::os::unix::fs::symlink(resolved(&dir).join("real"), resolved(&dir).join("Link")).unwrap();
    assert_eq!(
        listed(&dir, "link/inner/a.php", CaseRule::Folded),
        PathBuf::from("Link/inner/a.php")
    );
}
