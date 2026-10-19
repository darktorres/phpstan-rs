//! Which vendor directories are Composer's, as the degrade walk and the
//! classification see them. A directory is Composer's when its parent's
//! manifest names it as the vendor directory, or when Composer has written
//! into it, wherever it lies and whatever it is called. A directory with
//! neither is the project's own code, `lib/vendor` included.

use super::super::Project;
use super::super::session::{
    ErrorPolicy, Overrides, Session, classify_unlisted, unlisted_role, with_unlisted_composer,
};
use super::super::walk::Role;
use super::helpers::{TempDir, resolved};
use super::installer_paths::composed;
use crate::registry::stub_set::Stubs;
use crate::test_links::link_dir;
use config::ConfigFile;
use diagnostics::CaseRule;
use std::path::{Path, PathBuf};

const INSTALLED: &str = r#"{"packages": []}"#;

/// A project with one analysed file and no manifest.
fn project(name: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new(name);
    dir.write("proj/src/a.php", "<?php class A {}");
    let root = resolved(&dir).join("proj");
    (dir, root)
}

/// The files the session's degrade walk lists, relative to `root` and sorted.
fn analysed(session: &Session, root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = session
        .listing()
        .iter()
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

fn role_of(session: &Session, path: &Path) -> Role {
    session.classify(path).role
}

#[test]
fn a_vendor_directory_no_manifest_or_composer_mark_names_is_analysed() {
    let (dir, root) = project("composer_vendor_plain");
    dir.write("proj/lib/vendor/x.php", "<?php class X {}");
    let session = composed(&root);
    assert_eq!(
        analysed(&session, &root),
        vec![
            PathBuf::from("lib/vendor/x.php"),
            PathBuf::from("src/a.php"),
        ]
    );
    assert_eq!(
        role_of(&session, &root.join("lib/vendor/x.php")),
        Role::Analysed
    );
}

#[test]
fn a_composer_named_directory_without_installed_packages_is_analysed() {
    let (dir, root) = project("composer_vendor_named");
    dir.write("proj/lib/vendor/composer/autoload.php", "<?php");
    let session = composed(&root);
    assert!(analysed(&session, &root).contains(&PathBuf::from("lib/vendor/composer/autoload.php")));
}

#[test]
fn a_vendor_directory_beside_a_manifest_that_names_another_is_analysed() {
    let (dir, root) = project("composer_vendor_other");
    dir.write(
        "proj/composer.json",
        r#"{"config": {"vendor-dir": "libs"}}"#,
    );
    dir.write("proj/vendor/own.php", "<?php class Own {}");
    let session = composed(&root);
    assert!(analysed(&session, &root).contains(&PathBuf::from("vendor/own.php")));
    assert_eq!(
        role_of(&session, &root.join("vendor/own.php")),
        Role::Analysed
    );
}

#[test]
fn a_vendor_directory_the_manifest_names_is_context_before_composer_writes_it() {
    let (dir, root) = project("composer_vendor_unbuilt");
    dir.write("proj/composer.json", "{}");
    dir.write("proj/vendor/acme/new.php", "<?php class New {}");
    let session = composed(&root);
    assert_eq!(
        role_of(&session, &root.join("vendor/acme/new.php")),
        Role::Context
    );
    assert_eq!(analysed(&session, &root), vec![PathBuf::from("src/a.php")]);
}

#[test]
fn a_configured_vendor_directory_is_context_before_composer_writes_it() {
    let (dir, root) = project("composer_vendor_configured");
    dir.write(
        "proj/composer.json",
        r#"{"config": {"vendor-dir": "libs"}}"#,
    );
    dir.write("proj/libs/acme/new.php", "<?php class New {}");
    let session = composed(&root);
    assert_eq!(
        role_of(&session, &root.join("libs/acme/new.php")),
        Role::Context
    );
    assert_eq!(analysed(&session, &root), vec![PathBuf::from("src/a.php")]);
}

#[test]
fn nested_projects_vendor_directories_are_left_out_with_and_without_a_manifest() {
    let (dir, root) = project("composer_vendor_nested");
    dir.write("proj/composer.json", "{}");
    dir.write("proj/packages/a/composer.json", "{}");
    dir.write("proj/packages/a/src/a.php", "<?php class A {}");
    dir.write("proj/packages/a/vendor/acme/x.php", "<?php class X {}");
    dir.write("proj/packages/b/src/b.php", "<?php class B {}");
    dir.write("proj/packages/b/vendor/composer/installed.json", INSTALLED);
    dir.write("proj/packages/b/vendor/acme/y.php", "<?php class Y {}");
    let session = composed(&root);
    assert_eq!(
        analysed(&session, &root),
        vec![
            PathBuf::from("packages/a/src/a.php"),
            PathBuf::from("packages/b/src/b.php"),
            PathBuf::from("src/a.php"),
        ]
    );
    assert_eq!(
        role_of(&session, &root.join("packages/a/vendor/acme/x.php")),
        Role::Context
    );
    assert_eq!(
        role_of(&session, &root.join("packages/b/vendor/acme/y.php")),
        Role::Context
    );
}

#[test]
fn a_shared_vendor_directory_outside_the_nested_subtrees_is_left_out() {
    let (dir, root) = project("composer_vendor_shared");
    dir.write("proj/vendor/composer/installed.json", INSTALLED);
    dir.write("proj/vendor/acme/z.php", "<?php class Z {}");
    dir.write(
        "proj/packages/a/composer.json",
        r#"{"config": {"vendor-dir": "../../vendor"}}"#,
    );
    dir.write("proj/packages/a/src/a.php", "<?php class A {}");
    let session = composed(&root);
    assert_eq!(
        analysed(&session, &root),
        vec![
            PathBuf::from("packages/a/src/a.php"),
            PathBuf::from("src/a.php"),
        ]
    );
    assert_eq!(
        role_of(&session, &root.join("vendor/acme/z.php")),
        Role::Context
    );
}

#[test]
fn a_build_copy_and_a_stale_install_are_left_out() {
    let (dir, root) = project("composer_vendor_copies");
    dir.write(
        "proj/composer.json",
        r#"{"config": {"vendor-dir": "libs"}}"#,
    );
    dir.write("proj/build/vendor/composer/installed.json", INSTALLED);
    dir.write("proj/build/vendor/acme/b.php", "<?php class B {}");
    dir.write("proj/vendor/composer/installed.json", INSTALLED);
    dir.write("proj/vendor/acme/s.php", "<?php class S {}");
    let session = composed(&root);
    assert_eq!(analysed(&session, &root), vec![PathBuf::from("src/a.php")]);
    assert_eq!(
        role_of(&session, &root.join("build/vendor/acme/b.php")),
        Role::Context
    );
    assert_eq!(
        role_of(&session, &root.join("vendor/acme/s.php")),
        Role::Context
    );
}

#[test]
fn a_link_to_a_composer_vendor_directory_is_not_followed() {
    let (dir, root) = project("composer_vendor_link");
    dir.write("proj/vendor/composer/installed.json", INSTALLED);
    dir.write("proj/vendor/acme/v.php", "<?php class V {}");
    link_dir(root.join("vendor"), root.join("lnk"));
    let session = composed(&root);
    assert_eq!(analysed(&session, &root), vec![PathBuf::from("src/a.php")]);
}

#[test]
fn a_vendor_named_ancestor_without_composer_marks_is_not_context_when_unlisted() {
    let dir = TempDir::new("composer_vendor_ancestor");
    dir.write("vendor/proj/src/a.php", "<?php class A {}");
    let base = resolved(&dir);
    let path = base.join("vendor/proj/src/a.php");
    let session = composed(&base.join("vendor/proj"));
    assert_eq!(role_of(&session, &path), Role::Analysed);
    assert_eq!(classify_unlisted(&path).role, Role::Analysed);
}

#[test]
fn a_never_seen_path_is_classified_alike_by_the_session_and_unlisted() {
    let (dir, root) = project("composer_vendor_agree");
    dir.write("proj/composer.json", "{}");
    dir.write("proj/vendor/composer/installed.json", INSTALLED);
    dir.write("proj/packages/a/composer.json", "{}");
    dir.write("proj/lib/vendor/x.php", "<?php class X {}");
    dir.write("proj/.git/HEAD", "ref: refs/heads/main");
    let session = composed(&root);
    let cases = [
        ("vendor/acme/new.php", Role::Context),
        ("packages/a/vendor/new.php", Role::Context),
        ("lib/vendor/new.php", Role::Analysed),
        (".git/HEAD", Role::Context),
        ("src/new.php", Role::Analysed),
    ];
    for (rel, expected) in cases {
        let path = root.join(rel);
        let by_session = role_of(&session, &path);
        let by_root = with_unlisted_composer(|composer| {
            unlisted_role(
                &path,
                Some(&root),
                CaseRule::Exact,
                crate::project::walk::PathCut::None,
                composer,
                &[],
            )
        });
        let by_unlisted = classify_unlisted(&path).role;
        assert_eq!(by_session, expected, "session: {rel}");
        assert_eq!(by_root, expected, "unlisted under the root: {rel}");
        assert_eq!(by_unlisted, expected, "unlisted: {rel}");
    }
}

/// The project at `root`, loaded with Composer as the language server loads it.
fn composed_project(root: &Path) -> Project {
    let config_path = root.join("phpstan-rs.toml");
    std::fs::write(&config_path, "").unwrap();
    let config = ConfigFile::load(&config_path, &config::Schema::new()).unwrap();
    let session = Session::resolve(
        &config,
        config.dir().map(Path::to_path_buf),
        root.to_path_buf(),
        Overrides {
            paths: vec![PathBuf::new()],
            ..Overrides::default()
        },
        Stubs::shared().clone(),
        ErrorPolicy::Fail,
    )
    .unwrap();
    let mut project = Project::new();
    project.apply_settings(session.editor_settings());
    session.load_into(&mut project).unwrap();
    project
}

/// The paths the include rule reports as missing in `file` of the project at
/// `root`, loaded with Composer.
fn missing_includes(root: &Path, file: &Path) -> Vec<String> {
    composed_project(root)
        .check_file(file)
        .iter()
        .filter(|d| d.identifier.ends_with(".fileNotFound"))
        .map(|d| d.message.clone())
        .collect()
}

/// Two includes of the project's `libs` directory, the first of a file that
/// the project's autoloader writes and the second of a file that is not there.
const LIBS_INCLUDES: &str = "<?php\nrequire __DIR__ . '/../libs/autoload.php';\nrequire __DIR__ . '/../libs/missing.php';\n";

#[test]
fn an_include_below_an_uninstalled_vendor_directory_named_by_the_manifest_is_not_judged() {
    let (dir, root) = project("composer_vendor_include_unbuilt");
    dir.write(
        "proj/composer.json",
        r#"{"config": {"vendor-dir": "libs"}}"#,
    );
    dir.write("proj/src/a.php", LIBS_INCLUDES);
    let missing = missing_includes(&root, &root.join("src/a.php"));
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn an_include_below_an_installed_vendor_directory_named_by_the_manifest_is_judged() {
    let (dir, root) = project("composer_vendor_include_installed");
    dir.write(
        "proj/composer.json",
        r#"{"config": {"vendor-dir": "libs"}}"#,
    );
    dir.write("proj/libs/composer/installed.json", INSTALLED);
    dir.write("proj/libs/autoload.php", "<?php");
    dir.write("proj/src/a.php", LIBS_INCLUDES);
    let missing = missing_includes(&root, &root.join("src/a.php"));
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(missing[0].contains("missing.php"), "{missing:?}");
}

#[test]
fn an_edit_of_a_path_the_project_has_not_seen_is_analysed_as_the_session_classifies_it() {
    let cases = [
        ("vendor/acme/new.php", false),
        ("lib/vendor/new.php", true),
        ("src/new.php", true),
    ];
    for (rel, analysed) in cases {
        let (dir, root) = project("composer_vendor_edit");
        dir.write("proj/composer.json", "{}");
        dir.write("proj/vendor/composer/installed.json", INSTALLED);
        let session = composed(&root);
        let path = root.join(rel);
        assert_eq!(
            role_of(&session, &path) == Role::Analysed,
            analysed,
            "session: {rel}"
        );

        // A class that extends a missing one is reported only in an analysed
        // file.
        let mut project = composed_project(&root);
        let affected = project
            .update_file(&path, "<?php class New extends Missing {}")
            .unwrap();
        // The editor publishes the diagnostics of analysed files only, so a
        // context file's own findings are not what it shows.
        let published = project.analyzed_files();
        let reported = affected
            .iter()
            .filter(|(id, _)| published.contains(id))
            .flat_map(|(_, diags)| diags)
            .any(|d| d.identifier.ends_with("class.notFound"));
        assert_eq!(reported, analysed, "edit: {rel}");
    }
}
