//! The roles a session's load gives its files: each file once, by the
//! strongest root that contains it.

use super::super::session::{Overrides, Policy, Session};
use super::super::*;
use super::helpers::TempDir;

fn session(dir: &TempDir, overrides: Overrides) -> Session {
    let schema = crate::app::config_schema();
    let config = config::ConfigFile::parse("", &schema).unwrap();
    Session::resolve(
        &config,
        None,
        dir.0.clone(),
        Overrides {
            no_composer: true,
            ..overrides
        },
        Policy::Fail,
        worker_pool(),
    )
    .unwrap()
}

fn tree() -> TempDir {
    let dir = TempDir::new("session_roles");
    dir.write("src/A.php", "<?php class A {}");
    dir.write("src/sub/B.php", "<?php class B {}");
    dir.write("lib/C.php", "<?php class C {}");
    dir.write("stubs/D.php", "<?php class D {}");
    dir
}

fn paths(dirs: &[&str]) -> Vec<PathBuf> {
    dirs.iter().map(PathBuf::from).collect()
}

/// Each loaded file with whether it is analysed, by path.
fn roles_of(project: &Project) -> Vec<(String, bool)> {
    let mut files: Vec<(String, bool)> = project
        .files
        .iter()
        .map(|(path, state)| (path.display().to_string(), state.analyzed))
        .collect();
    files.sort();
    files
}

#[test]
fn paths_that_overlap_load_each_file_once() {
    let dir = tree();
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src", "./src", "src/sub", "src/A.php"]),
            ..Overrides::default()
        },
    );
    let loaded = session.load().unwrap();
    assert_eq!(
        roles_of(&loaded.project),
        [
            ("src/A.php".to_string(), true),
            ("src/sub/B.php".to_string(), true)
        ]
    );
    assert_eq!(loaded.project.next_load_order, 2);
}

#[test]
fn a_file_both_analysed_and_context_is_analysed_once() {
    let dir = tree();
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            scan_paths: paths(&["src", "lib", "."]),
            ..Overrides::default()
        },
    );
    let loaded = session.load().unwrap();
    let files = roles_of(&loaded.project);
    assert_eq!(
        files
            .iter()
            .filter(|(path, _)| path.ends_with("A.php"))
            .count(),
        1,
        "{files:?}"
    );
    for (path, analysed) in &files {
        assert_eq!(*analysed, path.contains("src/"), "{path}");
    }
    assert_eq!(
        files
            .iter()
            .filter(|(path, analysed)| path.ends_with("lib/C.php") && !analysed)
            .count(),
        1,
        "{files:?}"
    );
    let mut places: Vec<u64> = loaded
        .project
        .files
        .values()
        .map(|state| state.load_order)
        .collect();
    places.sort_unstable();
    assert_eq!(places, (0..places.len() as u64).collect::<Vec<_>>());
}

#[test]
fn the_session_classifies_a_file_the_load_never_listed() {
    let dir = tree();
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            scan_paths: paths(&["lib"]),
            stub_paths: paths(&["stubs"]),
            ..Overrides::default()
        },
    );
    let analysed = session.classify(Path::new("src/new/E.php"));
    assert_eq!(analysed.role, super::super::roles::Role::Analysed);
    assert!(!analysed.is_stub);
    let context = session.classify(Path::new("./lib/../lib/F.php"));
    assert_eq!(context.role, super::super::roles::Role::Context);
    let stub = session.classify(Path::new("stubs/G.php"));
    assert_eq!(stub.role, super::super::roles::Role::Context);
    assert!(stub.is_stub);
    let loaded = session.load().unwrap();
    assert_eq!(
        loaded.project.classify(Path::new("src/new/E.php")),
        analysed
    );
    assert_eq!(loaded.project.classify(Path::new("stubs/G.php")), stub);
}

#[test]
fn the_listing_is_the_set_of_files_the_load_reads() {
    let dir = tree();
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            scan_paths: paths(&["lib"]),
            stub_paths: paths(&["stubs"]),
            ..Overrides::default()
        },
    );
    let (analysed, context) = session.listing();
    let loaded = session.load().unwrap();
    let mut listed: Vec<String> = analysed
        .iter()
        .chain(&context)
        .map(|path| path.display().to_string())
        .collect();
    listed.sort();
    let loaded_paths: Vec<String> = roles_of(&loaded.project)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(listed, loaded_paths);
    assert!(
        analysed
            .iter()
            .all(|path| loaded.project.files[path].analyzed)
    );
    assert!(
        context
            .iter()
            .all(|path| !loaded.project.files[path].analyzed)
    );
}

#[test]
fn an_invalid_setting_fails_one_policy_and_is_taken_as_unset_by_the_other() {
    let dir = tree();
    let schema = crate::app::config_schema();
    let config = config::ConfigFile::parse("php_version = \"not-a-version\"\n", &schema).unwrap();
    let resolve = |policy| {
        Session::resolve(
            &config,
            None,
            dir.0.clone(),
            Overrides {
                no_composer: true,
                ..Overrides::default()
            },
            policy,
            worker_pool(),
        )
    };
    assert!(resolve(Policy::Fail).is_err());
    let degraded = resolve(Policy::Degrade).unwrap();
    assert!(!degraded.warnings.is_empty());
}
