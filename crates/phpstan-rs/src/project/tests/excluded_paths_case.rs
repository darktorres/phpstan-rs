//! The paths a configuration leaves out are compared with the paths a walk
//! lists by the case rule of the root's directory: any case of their letters
//! where the directory folds case, the exact spelling elsewhere. The rule is
//! injected, so a case sensitive disk shows what a folding one gives.

use super::super::exclude_paths::Exclusions;
use super::super::path_under_excluded_dir;
use super::super::walk::{Excluded, PathCut, Role, Unstreamed, Walk};
use super::helpers::{TempDir, resolved};
use diagnostics::CaseRule;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

static NO_PACKAGES: LazyLock<composer::PackageIndex> =
    LazyLock::new(composer::PackageIndex::default);

/// The exclusions that leave out each of `names` below `root`, at any depth. A
/// name needs no file on disk for its spelling to be compared.
fn leaving_out(names: &[&str]) -> Exclusions {
    Exclusions::anywhere(
        names
            .iter()
            .flat_map(|name| [name.to_string(), format!("*/{name}")]),
    )
}

/// The files a walk of `root` lists, with the paths `names` leave out, compared
/// under `case`, relative to `root` and sorted.
fn listed(root: &Path, names: &[&str], case: CaseRule) -> Vec<PathBuf> {
    let exclusions = leaving_out(names);
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES).with_path_case(case);
    walk.add(
        root,
        Role::Analysed,
        PathCut::Analysis(Arc::new(exclusions)),
    );
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
fn excluded_paths_compare_by_the_rule() {
    let exclusions = leaving_out(&["vendor", ".git"]);
    let folded = |path: &str| exclusions.excludes_scan(Path::new(path), CaseRule::Folded, None);
    let exact = |path: &str| exclusions.excludes_scan(Path::new(path), CaseRule::Exact, None);
    assert!(folded("/proj/Vendor"));
    assert!(folded("/proj/.GIT"));
    assert!(!folded("/proj/vendors"));
    assert!(exact("/proj/vendor"));
    assert!(!exact("/proj/Vendor"));
    assert!(Exclusions::default().is_empty());
    assert!(!Exclusions::default().excludes_scan(Path::new("/proj/vendor"), CaseRule::Exact, None));
}

#[test]
fn a_path_below_a_degrade_floor_directory_is_under_it_by_the_rule() {
    let floor = Arc::new(Exclusions::default().with_floor());
    let under = |case| {
        path_under_excluded_dir(
            Path::new("/proj/.GIT/hooks/a.php"),
            None,
            Excluded::new(PathCut::Analysis(Arc::clone(&floor)), case),
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
    let leaving_out = Excluded::new(PathCut::None, CaseRule::Exact).leaving_out_composer();
    let path = Path::new("/proj/libs/acme/a.php");
    let root = Some(Path::new("/proj"));
    assert!(path_under_excluded_dir(
        path,
        root,
        leaving_out.clone(),
        &composer,
        &[]
    ));
    assert!(!path_under_excluded_dir(
        path,
        root,
        Excluded::new(PathCut::None, CaseRule::Exact),
        &composer,
        &[],
    ));
    // The root itself is never asked: a walk of `libs` lists its files.
    assert!(!path_under_excluded_dir(
        Path::new("/proj/libs/a.php"),
        Some(Path::new("/proj/libs")),
        leaving_out.clone(),
        &composer,
        &[],
    ));
}
