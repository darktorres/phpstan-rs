//! The nested Composer projects of a degrade root (a monorepo's packages, a
//! `tools/cs/` with a vendor directory of its own) are roots before the walk:
//! their vendor classes are context to their own source, their vendor
//! directories are left out of the analysed walk, and a nested project with no
//! vendor directory is not judged for the includes of it. `--no-composer` runs
//! none of this.

use super::super::Project;
use super::super::exclude_paths::Exclusions;
use super::super::session::{ErrorPolicy, Overrides, Session};
use super::super::walk::PathCut;
use super::super::walk::{Excluded, Role, Walk};
use super::helpers::{TempDir, resolved};
use crate::registry::stub_set::Stubs;
use crate::test_links::link_dir;
use config::ConfigFile;
use diagnostics::CaseRule;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

static NO_PACKAGES: LazyLock<composer::PackageIndex> =
    LazyLock::new(composer::PackageIndex::default);

/// The `autoload_static.php` a nested project's Composer writes: one class,
/// `Cs\Fixer`, under the nested vendor directory.
const CS_STATIC: &str = r#"<?php
namespace Composer\Autoload;

class ComposerStaticInitCs
{
    public static $classMap = array (
        'Cs\\Fixer' => __DIR__ . '/..' . '/cs/src/Fixer.php',
    );
}
"#;

/// A project with a root manifest, and a nested project `tools/cs` whose
/// installed vendor directory holds the class its script uses.
fn monorepo(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new(name);
    dir.write("proj/composer.json", "{}");
    dir.write("proj/src/Bar.php", "<?php class Bar {}");
    dir.write("proj/tools/cs/composer.json", "{}");
    dir.write(
        "proj/tools/cs/vendor/composer/installed.json",
        r#"{"packages": []}"#,
    );
    dir.write(
        "proj/tools/cs/vendor/composer/autoload_static.php",
        CS_STATIC,
    );
    dir.write(
        "proj/tools/cs/vendor/cs/src/Fixer.php",
        "<?php namespace Cs; class Fixer {}",
    );
    dir.write("proj/tools/cs/run.php", "<?php new \\Cs\\Fixer();");
    let root = resolved(&dir).join("proj");
    (dir, root)
}

/// The session of the editor's degrade mode over `root`, with Composer read
/// unless `overrides` says otherwise.
fn degraded_session(root: &Path, overrides: Overrides) -> Session {
    Session::resolve(
        &ConfigFile::empty(),
        None,
        root.to_path_buf(),
        overrides,
        Stubs::shared().clone(),
        ErrorPolicy::Degrade,
    )
    .unwrap()
}

fn degraded_project(root: &Path, overrides: Overrides) -> Project {
    let session = degraded_session(root, overrides);
    let mut project = Project::new();
    project.apply_settings(session.editor_settings());
    session.load_into(&mut project).unwrap();
    project
}

/// The paths the degrade walk lists, sorted.
fn analysed(session: &Session) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = session
        .listing()
        .iter()
        .map(|listed| listed.path.as_path().to_path_buf())
        .collect();
    files.sort();
    files
}

#[test]
fn a_nested_project_s_script_sees_the_classes_of_its_own_vendor_directory() {
    let (_dir, root) = monorepo("nested_context");
    let project = degraded_project(&root, Overrides::default());
    let diagnostics = project.check_file(&root.join("tools/cs/run.php"));
    assert!(
        diagnostics
            .iter()
            .all(|d| !d.identifier.ends_with("class.notFound")),
        "{diagnostics:?}"
    );
}

#[test]
fn with_no_composer_the_nested_script_has_its_class_not_found() {
    let (_dir, root) = monorepo("nested_no_composer");
    let overrides = Overrides {
        no_composer: true,
        ..Overrides::default()
    };
    let project = degraded_project(&root, overrides);
    let diagnostics = project.check_file(&root.join("tools/cs/run.php"));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.identifier.ends_with("class.notFound")),
        "{diagnostics:?}"
    );
}

#[test]
fn a_nested_project_s_vendor_directory_is_not_analysed() {
    let (_dir, root) = monorepo("nested_vendor_walk");
    let session = degraded_session(&root, Overrides::default());
    let vendor = root.join("tools/cs/vendor/cs/src/Fixer.php");
    assert!(analysed(&session).contains(&vendor));
    assert_eq!(session.classify(&vendor).role, Role::Context);
    assert_eq!(
        session.classify(&root.join("tools/cs/run.php")).role,
        Role::Analysed
    );
}

#[test]
fn a_nested_project_without_a_vendor_directory_is_not_judged_for_its_vendor_includes() {
    let (dir, root) = monorepo("nested_unbuilt");
    dir.write("proj/tools/tool/composer.json", "{}");
    dir.write(
        "proj/tools/tool/main.php",
        "<?php\nrequire __DIR__ . '/vendor/autoload.php';\nrequire __DIR__ . '/missing.php';\n",
    );
    let project = degraded_project(&root, Overrides::default());
    let missing: Vec<String> = project
        .check_file(&root.join("tools/tool/main.php"))
        .iter()
        .filter(|d| d.identifier.ends_with(".fileNotFound"))
        .map(|d| d.message.clone())
        .collect();
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(missing[0].contains("missing.php"), "{missing:?}");
}

#[test]
fn the_scan_lists_the_manifests_the_walk_enters_once_each() {
    let (dir, root) = monorepo("nested_scan");
    dir.write("proj/packages/a/composer.json", "{}");
    // Below a vendor directory the walk does not enter, and below an excluded
    // name, so neither is a root.
    dir.write("proj/packages/a/vendor/x/composer.json", "{}");
    dir.write("proj/node_modules/p/composer.json", "{}");
    link_dir(root.join("tools/cs"), root.join("packages/b"));
    let composer = |dir: &Path| dir.ends_with("vendor");
    let floor = Exclusions::default().with_floor();
    let found = Walk::new(Arc::default(), &NO_PACKAGES)
        .with_composer(&composer)
        .manifest_dirs(
            &root,
            &Excluded::new(
                PathCut::Analysis(std::sync::Arc::new(floor)),
                CaseRule::Exact,
            )
            .leaving_out_composer(),
        );
    assert_eq!(
        found,
        vec![root.join("packages/a"), root.join("tools/cs")],
        "the root's own manifest is not a nested root"
    );
}
