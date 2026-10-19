//! The directories of packages installed outside the vendor directory (a custom
//! installer's `web/core`) are Composer's: a walk that leaves Composer's
//! directories out leaves these out, a link into one is not followed, and the
//! session classifies a file below one as context, whether or not the walk
//! listed it. A walk that does not leave Composer out lists them as it lists
//! any directory.

use super::super::path_under_excluded_dir;
use super::super::session::{ErrorPolicy, Overrides, Session};
use super::super::walk::{Excluded, Role, Unstreamed, Walk, is_seed};
use super::helpers::{TempDir, resolved};
use crate::registry::stub_set::Stubs;
use crate::test_links::link_dir;
use config::ConfigFile;
use diagnostics::CaseRule;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

static NO_PACKAGES: LazyLock<composer::PackageIndex> =
    LazyLock::new(composer::PackageIndex::default);

/// The project tree: `web/core` is installed by a custom installer, and
/// `web/modules/custom` is the project's own code.
fn tree(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new(name);
    dir.write("proj/src/Bar.php", "<?php class Bar {}");
    dir.write(
        "proj/web/core/lib/Drupal/Core/Foo.php",
        "<?php class Foo {}",
    );
    dir.write("proj/web/modules/custom/mine.php", "<?php class Mine {}");
    let root = resolved(&dir).join("proj");
    (dir, root)
}

/// The files a walk of `root` lists, relative to `root` and sorted. The walk
/// knows `seeds`, the directories of packages installed elsewhere, and leaves
/// Composer's directories out when `leave_out` is set.
fn walked(root: &Path, seeds: &[PathBuf], leave_out: bool) -> Vec<PathBuf> {
    let vendor = root.join("vendor");
    let composer = |dir: &Path| dir == vendor.as_path();
    let mut walk = Walk::new(Arc::default(), &NO_PACKAGES)
        .with_composer(&composer)
        .with_seeds(Arc::from(seeds));
    if leave_out {
        walk.add_leaving_out_composer(root, Role::Analysed, &[]);
    } else {
        walk.add(root, Role::Analysed, &[]);
    }
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

#[test]
fn a_composer_walk_leaves_out_the_directory_of_a_package_installed_elsewhere() {
    let (_dir, root) = tree("installer_paths_walk");
    let seeds = [root.join("web/core")];
    assert_eq!(
        walked(&root, &seeds, true),
        vec![
            PathBuf::from("src/Bar.php"),
            PathBuf::from("web/modules/custom/mine.php"),
        ]
    );
}

#[test]
fn a_configured_walk_lists_a_package_installed_elsewhere_as_any_directory() {
    let (_dir, root) = tree("installer_paths_configured");
    let seeds = [root.join("web/core")];
    assert_eq!(
        walked(&root, &seeds, false),
        vec![
            PathBuf::from("src/Bar.php"),
            PathBuf::from("web/core/lib/Drupal/Core/Foo.php"),
            PathBuf::from("web/modules/custom/mine.php"),
        ]
    );
}

#[test]
fn a_composer_walk_does_not_follow_a_link_into_a_package_installed_elsewhere() {
    let (_dir, root) = tree("installer_paths_link");
    link_dir(root.join("web/core/lib"), root.join("link"));
    let seeds = [root.join("web/core")];
    assert_eq!(
        walked(&root, &seeds, true),
        vec![
            PathBuf::from("src/Bar.php"),
            PathBuf::from("web/modules/custom/mine.php"),
        ]
    );
}

#[test]
fn a_seed_is_matched_by_the_case_rule_of_its_root() {
    let seeds = [PathBuf::from("/proj/web/core")];
    assert!(is_seed(
        &seeds,
        CaseRule::Folded,
        Path::new("/PROJ/Web/CORE")
    ));
    assert!(!is_seed(
        &seeds,
        CaseRule::Exact,
        Path::new("/PROJ/Web/CORE")
    ));
    assert!(is_seed(
        &seeds,
        CaseRule::Exact,
        Path::new("/proj/web/core")
    ));
    assert!(!is_seed(
        &seeds,
        CaseRule::Exact,
        Path::new("/proj/web/core/lib")
    ));
    assert!(!is_seed(&[], CaseRule::Folded, Path::new("/proj/web/core")));
}

#[test]
fn a_seed_is_found_among_several_by_its_exact_spelling() {
    let seeds = [
        PathBuf::from("/proj/a"),
        PathBuf::from("/proj/web/core"),
        PathBuf::from("/proj/z"),
    ];
    for seed in &seeds {
        assert!(is_seed(&seeds, CaseRule::Exact, seed));
    }
    assert!(!is_seed(&seeds, CaseRule::Exact, Path::new("/proj/b")));
    assert!(!is_seed(&seeds, CaseRule::Exact, Path::new("/proj/web")));
}

#[test]
fn a_path_below_a_seed_is_under_it_only_where_the_walk_leaves_composer_out() {
    let seeds = [PathBuf::from("/proj/web/core")];
    let path = Path::new("/proj/web/core/lib/Foo.php");
    let root = Some(Path::new("/proj"));
    let never = |_: &Path| false;
    assert!(path_under_excluded_dir(
        path,
        root,
        Excluded::new(&[], CaseRule::Exact).leaving_out_composer(),
        &never,
        &seeds,
    ));
    assert!(!path_under_excluded_dir(
        path,
        root,
        Excluded::new(&[], CaseRule::Exact),
        &never,
        &seeds,
    ));
    assert!(!path_under_excluded_dir(
        Path::new("/proj/web/modules/custom/mine.php"),
        root,
        Excluded::new(&[], CaseRule::Exact).leaving_out_composer(),
        &never,
        &seeds,
    ));
}

/// A Composer project whose `vendor/composer/installed.json` lists `drupal/core`
/// at `web/core`, as Composer writes a custom installer's package, beside
/// `acme/lib` in the vendor directory.
fn composer_tree(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new(name);
    dir.write(
        "proj/composer.json",
        r#"{"name": "acme/site", "extra": {"installer-paths": {"web/core": ["drupal/core"]}}}"#,
    );
    dir.write(
        "proj/vendor/composer/installed.json",
        r#"{"packages": [
            {"name": "drupal/core", "install-path": "../../web/core"},
            {"name": "acme/lib", "install-path": "../acme/lib"}
        ]}"#,
    );
    dir.write("proj/vendor/acme/lib/src/Baz.php", "<?php class Baz {}");
    dir.write(
        "proj/web/core/lib/Drupal/Core/Foo.php",
        "<?php class Foo {}",
    );
    dir.write("proj/web/modules/custom/mine.php", "<?php class Mine {}");
    dir.write("proj/src/Bar.php", "<?php class Bar {}");
    let root = resolved(&dir).join("proj");
    (dir, root)
}

/// A session over `root` with Composer read, in the editor's degrade mode.
pub(super) fn composed(root: &Path) -> Session {
    Session::resolve(
        &ConfigFile::empty(),
        None,
        root.to_path_buf(),
        Overrides::default(),
        Stubs::shared().clone(),
        ErrorPolicy::Degrade,
    )
    .unwrap()
}

#[test]
fn the_session_classifies_a_file_below_a_package_installed_elsewhere_as_context() {
    let (dir, root) = composer_tree("installer_paths_session");
    let session = composed(&root);

    let listed: Vec<PathBuf> = session
        .listing()
        .iter()
        .map(|listed| listed.path.as_path().to_path_buf())
        .collect();
    assert!(listed.contains(&root.join("src/Bar.php")));
    assert!(listed.contains(&root.join("web/modules/custom/mine.php")));
    assert!(
        !listed
            .iter()
            .any(|path| path.starts_with(root.join("web/core")))
    );
    assert!(
        !listed
            .iter()
            .any(|path| path.starts_with(root.join("vendor")))
    );

    assert_eq!(
        session
            .classify(&root.join("web/core/lib/Drupal/Core/Foo.php"))
            .role,
        Role::Context
    );
    assert_eq!(
        session.classify(&root.join("src/Bar.php")).role,
        Role::Analysed
    );
    assert_eq!(
        session
            .classify(&root.join("web/modules/custom/mine.php"))
            .role,
        Role::Analysed
    );
    assert_eq!(
        session
            .classify(&root.join("vendor/acme/lib/src/Baz.php"))
            .role,
        Role::Context
    );

    dir.write(
        "proj/web/core/lib/Drupal/Core/New.php",
        "<?php class New {}",
    );
    assert_eq!(
        session
            .classify(&root.join("web/core/lib/Drupal/Core/New.php"))
            .role,
        Role::Context
    );
    // A directory that `installed.json` does not list is the project's own
    // code until the session is resolved again.
    dir.write("proj/web/modules/contrib/stale/stale.php", "<?php");
    assert_eq!(
        session
            .classify(&root.join("web/modules/contrib/stale/stale.php"))
            .role,
        Role::Analysed
    );
}
