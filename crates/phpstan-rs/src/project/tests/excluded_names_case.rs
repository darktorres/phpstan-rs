//! The directory names a walk leaves out are compared with the names it lists
//! by the case rule of the root's directory: any case of their letters where
//! the directory folds case, the exact spelling elsewhere. The rule is
//! injected, so a case sensitive disk shows what a folding one gives.

use super::super::walk::{Excluded, Role, Unstreamed, Walk};
use super::super::{DEFAULT_SCAN_EXCLUDED_DIR_NAMES, path_under_excluded_dir};
use super::helpers::{TempDir, resolved};
use diagnostics::CaseRule;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

static NO_PACKAGES: LazyLock<composer::PackageIndex> =
    LazyLock::new(composer::PackageIndex::default);

/// The files a walk of `root` lists, with `excluded` left out, compared under
/// `case`, relative to `root` and sorted.
fn listed(root: &Path, excluded: &[&str], case: CaseRule) -> Vec<PathBuf> {
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES).with_path_case(case);
    walk.add(root, Role::Analysed, excluded);
    let mut files: Vec<PathBuf> = walk
        .finish(&crate::project::worker_pool(), &Unstreamed)
        .files
        .into_iter()
        .map(|listed| {
            listed
                .path
                .as_path()
                .strip_prefix(root)
                .unwrap()
                .to_path_buf()
        })
        .collect();
    files.sort();
    files
}

fn tree(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new(name);
    dir.write("proj/src/a.php", "<?php\n");
    dir.write("proj/Vendor/x.php", "<?php\n");
    dir.write("proj/Node_Modules/y.php", "<?php\n");
    let root = resolved(&dir).join("proj");
    (dir, root)
}

#[test]
fn a_folded_rule_leaves_out_a_directory_named_in_another_case() {
    let (_dir, root) = tree("excluded_folded");
    assert_eq!(
        listed(&root, &["vendor", "node_modules"], CaseRule::Folded),
        vec![PathBuf::from("src/a.php")]
    );
}

#[test]
fn an_exact_rule_leaves_out_a_directory_by_its_own_spelling_only() {
    let (_dir, root) = tree("excluded_exact");
    assert_eq!(
        listed(&root, &["vendor", "node_modules"], CaseRule::Exact),
        vec![
            PathBuf::from("Node_Modules/y.php"),
            PathBuf::from("Vendor/x.php"),
            PathBuf::from("src/a.php"),
        ]
    );
}

#[test]
fn an_excluded_name_written_in_capitals_is_found_under_a_folded_rule() {
    let (_dir, root) = tree("excluded_capitals");
    assert_eq!(
        listed(&root, &["VENDOR"], CaseRule::Folded),
        vec![
            PathBuf::from("Node_Modules/y.php"),
            PathBuf::from("src/a.php")
        ]
    );
}

#[test]
fn excluded_names_compare_by_the_rule() {
    let names = ["vendor", ".git"];
    let folded = Excluded::new(&names, CaseRule::Folded);
    let exact = Excluded::new(&names, CaseRule::Exact);
    assert!(folded.excludes(OsStr::new("Vendor")));
    assert!(folded.excludes(OsStr::new(".GIT")));
    assert!(!folded.excludes(OsStr::new("vendors")));
    assert!(exact.excludes(OsStr::new("vendor")));
    assert!(!exact.excludes(OsStr::new("Vendor")));
    assert!(!Excluded::default().excludes(OsStr::new("vendor")));
    assert!(Excluded::default().is_empty());
}

#[test]
fn a_path_below_a_default_excluded_directory_is_under_it_by_the_rule() {
    let under = |case| {
        path_under_excluded_dir(
            Path::new(".GIT/hooks/a.php"),
            None,
            Excluded::new(DEFAULT_SCAN_EXCLUDED_DIR_NAMES, case),
            &|_: &Path| false,
            &[],
        )
    };
    assert!(under(CaseRule::Folded));
    assert!(!under(CaseRule::Exact));
}

#[test]
fn a_path_below_a_composer_directory_is_under_it_only_where_the_walk_leaves_composer_out() {
    let composer = |dir: &Path| dir == Path::new("/proj/libs");
    let leaving_out =
        Excluded::new(DEFAULT_SCAN_EXCLUDED_DIR_NAMES, CaseRule::Exact).leaving_out_composer();
    let path = Path::new("/proj/libs/acme/a.php");
    let root = Some(Path::new("/proj"));
    assert!(path_under_excluded_dir(
        path,
        root,
        leaving_out,
        &composer,
        &[]
    ));
    assert!(!path_under_excluded_dir(
        path,
        root,
        Excluded::new(DEFAULT_SCAN_EXCLUDED_DIR_NAMES, CaseRule::Exact),
        &composer,
        &[],
    ));
    // The root itself is never asked: a walk of `libs` lists its files.
    assert!(!path_under_excluded_dir(
        Path::new("/proj/libs/a.php"),
        Some(Path::new("/proj/libs")),
        leaving_out,
        &composer,
        &[],
    ));
}
