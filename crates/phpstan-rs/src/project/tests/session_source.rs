//! The files a session reads in place of the disk's: an overlay.

use super::super::session::{Loaded, Overrides, Policy, Session};
use super::super::source::ContentHash;
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

fn paths(dirs: &[&str]) -> Vec<PathBuf> {
    dirs.iter().map(PathBuf::from).collect()
}

#[test]
fn an_overlay_file_is_read_in_place_of_the_disks() {
    let dir = TempDir::new("source_in_place");
    dir.write("src/A.php", "<?php class OnDisk {}");
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            ..Overrides::default()
        },
    )
    .with_overlay([(
        PathBuf::from("src/A.php"),
        b"<?php class InOverlay {}".to_vec(),
    )]);
    let loaded = session.load().unwrap();
    assert!(loaded.project.registry().classes.contains_key("InOverlay"));
    assert!(!loaded.project.registry().classes.contains_key("OnDisk"));
    let path = Path::new("src/A.php");
    assert_eq!(
        loaded.project.content_hash(path),
        Some(ContentHash::of(b"<?php class InOverlay {}"))
    );
}

#[test]
fn an_overlay_file_the_disk_does_not_have_is_listed_and_loaded_under_its_role() {
    let dir = TempDir::new("source_only_overlay");
    dir.write("src/A.php", "<?php class A {}");
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            scan_paths: paths(&["lib"]),
            ..Overrides::default()
        },
    )
    .with_overlay([
        (PathBuf::from("src/sub/B.php"), b"<?php class B {}".to_vec()),
        (PathBuf::from("lib/C.php"), b"<?php class C {}".to_vec()),
        (PathBuf::from("src/notes.txt"), b"not php".to_vec()),
    ]);
    let (analysed, context) = session.listing();
    assert_eq!(analysed, paths(&["src/A.php", "src/sub/B.php"]));
    assert_eq!(context, paths(&["lib/C.php"]));
    let loaded = session.load().unwrap();
    for (name, analysed) in [("A", true), ("B", true), ("C", false)] {
        assert!(loaded.project.registry().classes.contains_key(name));
        let found: Vec<bool> = loaded
            .project
            .files
            .iter()
            .filter(|(path, _)| path.ends_with(format!("{name}.php")))
            .map(|(_, state)| state.analyzed)
            .collect();
        assert_eq!(found, [analysed], "{name}");
    }
}

#[test]
fn an_overlay_file_is_listed_once_when_the_disk_has_it_too() {
    let dir = TempDir::new("source_listed_once");
    dir.write("src/A.php", "<?php class A {}");
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            ..Overrides::default()
        },
    )
    .with_overlay([(PathBuf::from("src/A.php"), b"<?php class A2 {}".to_vec())]);
    let (analysed, _) = session.listing();
    assert_eq!(analysed, paths(&["src/A.php"]));
}

#[test]
fn a_file_root_that_only_the_overlay_holds_is_loaded() {
    let dir = TempDir::new("source_file_root");
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["virtual.php"]),
            ..Overrides::default()
        },
    )
    .with_overlay([(PathBuf::from("virtual.php"), b"<?php class V {}".to_vec())]);
    let loaded = session.load().unwrap();
    assert!(loaded.project.registry().classes.contains_key("V"));
}

#[test]
fn files_are_added_to_a_project_that_is_loaded_through_the_session() {
    let dir = TempDir::new("source_load_into");
    dir.write("src/A.php", "<?php class A {}");
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            ..Overrides::default()
        },
    )
    .with_overlay([(PathBuf::from("src/B.php"), b"<?php class B {}".to_vec())]);
    let mut loaded = session.load().unwrap();
    let added = session.load_into(
        &mut loaded.project,
        &[],
        &[PathBuf::from("src/C.php"), PathBuf::from("src/Missing.php")],
    );
    assert_eq!(added.skipped_analyze, 2);
    assert!(
        loaded
            .project
            .content_hash(Path::new("src/B.php"))
            .is_some()
    );
    assert_eq!(loaded.project.next_load_order, 2);
}

#[test]
fn a_listing_after_the_first_lists_the_directories_again() {
    let dir = TempDir::new("source_relisted");
    dir.write("src/A.php", "<?php class A {}");
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            ..Overrides::default()
        },
    );
    assert_eq!(session.listing().0, paths(&["src/A.php"]));
    dir.write("src/B.php", "<?php class B {}");
    assert_eq!(session.listing().0, paths(&["src/A.php", "src/B.php"]));
}

#[test]
fn a_classmap_file_in_a_psr4_directory_is_listed_once() {
    let dir = TempDir::new("source_psr4_once");
    dir.write("composer.json", "{}");
    dir.write("src/App.php", "<?php class App {}");
    dir.write("vendor/acme/lib/src/A.php", "<?php class A {}");
    dir.write("vendor/acme/lib/Top.php", "<?php class Top {}");
    dir.write(
        "vendor/composer/autoload_classmap.php",
        "<?php\n$vendorDir = dirname(__DIR__);\nreturn array(\n    'A' => $vendorDir . '/acme/lib/src/A.php',\n    'Top' => $vendorDir . '/acme/lib/Top.php',\n);\n",
    );
    dir.write(
        "vendor/composer/autoload_psr4.php",
        "<?php\n$vendorDir = dirname(__DIR__);\nreturn array(\n    'Acme\\\\' => array($vendorDir . '/acme/lib/src'),\n);\n",
    );
    let schema = crate::app::config_schema();
    let config = config::ConfigFile::parse("", &schema).unwrap();
    let session = Session::resolve(
        &config,
        None,
        dir.0.clone(),
        Overrides {
            paths: paths(&["src"]),
            ..Overrides::default()
        },
        Policy::Fail,
        worker_pool(),
    )
    .unwrap();
    let (analysed, context) = session.listing();
    assert_eq!(analysed, paths(&["src/App.php"]));
    let names: Vec<&str> = context
        .iter()
        .filter_map(|path| path.file_name()?.to_str())
        .collect();
    assert_eq!(names, ["Top.php", "A.php"]);
}

#[test]
fn a_load_that_parses_as_it_walks_loads_what_a_load_of_the_listing_does() {
    let dir = TempDir::new("source_parsed_as_walked");
    dir.write("src/A.php", "<?php class A {}");
    dir.write("src/deep/B.php", "<?php class B {}");
    dir.write("src/Broken.php", "<?php class {");
    dir.write("lib/C.php", "<?php class C {}");
    dir.write("lib/more/D.php", "<?php class D {}");
    let session = session(
        &dir,
        Overrides {
            paths: paths(&["src"]),
            scan_paths: paths(&["lib", "."]),
            ..Overrides::default()
        },
    );
    let walked = session.load().unwrap();
    let (analysed, context) = session.listing();
    let listed = session.load_listed(false, &context, analysed.clone());

    let classes = |loaded: &Loaded| {
        let mut names: Vec<String> = loaded
            .project
            .registry()
            .classes
            .keys()
            .map(|name| name.to_string())
            .collect();
        names.sort();
        names
    };
    for name in ["A", "B", "C", "D"] {
        assert!(classes(&walked).contains(&name.to_string()), "{name}");
    }
    assert_eq!(classes(&walked), classes(&listed));
    for path in analysed.iter().chain(&context) {
        assert_eq!(
            walked.project.content_hash(path),
            listed.project.content_hash(path),
            "{}",
            path.display()
        );
    }
    assert_eq!(
        walked.project.next_load_order,
        listed.project.next_load_order
    );
    assert_eq!(walked.lex_warnings, listed.lex_warnings);
}
