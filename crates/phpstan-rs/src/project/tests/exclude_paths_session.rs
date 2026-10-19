//! `exclude_paths` as a session applies it: what a configuration leaves out of
//! the analysis is still scanned, what it leaves out of both is not listed,
//! and an entry that names nothing is reported.

use super::super::session::{ErrorPolicy, Overrides, Session};
use super::super::walk::Role;
use super::helpers::{TempDir, resolved};
use crate::registry::stub_set::Stubs;
use config::ConfigFile;
use std::path::{Path, PathBuf};

/// The session `config` (the project's `phpstan-rs.toml`) resolves to, over
/// the project directory `proj` in `dir`.
fn session(dir: &TempDir, config: &str, policy: ErrorPolicy) -> Result<Session, String> {
    let path = dir.write("proj/phpstan-rs.toml", config);
    let config = ConfigFile::load(&path, &crate::app::config_schema())?;
    Session::resolve(
        &config,
        config.dir().map(Path::to_path_buf),
        resolved(dir).join("proj"),
        Overrides {
            no_composer: true,
            ..Overrides::default()
        },
        Stubs::shared().clone(),
        policy,
    )
}

/// Each file the session lists, relative to `proj`, with its role, sorted.
fn listed(session: &Session, dir: &TempDir) -> Vec<(PathBuf, Role)> {
    let proj = resolved(dir).join("proj");
    let mut files: Vec<(PathBuf, Role)> = session
        .listing()
        .iter()
        .map(|listed| {
            let rel = listed.path.as_path().strip_prefix(&proj).unwrap();
            (rel.to_path_buf(), listed.role)
        })
        .collect();
    files.sort();
    files
}

#[test]
fn a_path_left_out_of_the_analysis_is_still_scanned_and_one_left_out_of_both_is_not_listed() {
    let dir = TempDir::new("exclude_session_roles");
    dir.write("proj/src/a.php", "<?php\n");
    dir.write("proj/gen/x.php", "<?php\n");
    dir.write("proj/cache/y.php", "<?php\n");
    dir.write("proj/.git/hooks/h.php", "<?php\n");
    let session = session(
        &dir,
        "[exclude_paths]\nanalyse = [\"gen\"]\nanalyse_and_scan = [\"cache\"]\n",
        ErrorPolicy::Degrade,
    )
    .unwrap();
    assert_eq!(
        listed(&session, &dir),
        vec![
            (PathBuf::from("gen/x.php"), Role::Context),
            (PathBuf::from("src/a.php"), Role::Analysed),
        ]
    );
}

#[test]
fn a_flat_list_leaves_paths_out_of_the_analysis_and_the_scan() {
    let dir = TempDir::new("exclude_session_flat");
    dir.write("proj/src/a.php", "<?php\n");
    dir.write("proj/gen/x.php", "<?php\n");
    let session = session(&dir, "exclude_paths = [\"gen\"]\n", ErrorPolicy::Degrade).unwrap();
    assert_eq!(
        listed(&session, &dir),
        vec![(PathBuf::from("src/a.php"), Role::Analysed)]
    );
}

#[test]
fn the_degrade_floor_leaves_out_a_package_directory_of_a_root_with_no_paths() {
    let dir = TempDir::new("exclude_session_floor");
    dir.write("proj/src/a.php", "<?php\n");
    dir.write("proj/node_modules/p/x.php", "<?php\n");
    dir.write("proj/src/node_modules/q.php", "<?php\n");
    let session = session(&dir, "", ErrorPolicy::Degrade).unwrap();
    assert_eq!(
        listed(&session, &dir),
        vec![(PathBuf::from("src/a.php"), Role::Analysed)]
    );
}

#[test]
fn an_entry_that_names_nothing_is_an_error_and_the_others_still_apply() {
    let dir = TempDir::new("exclude_session_missing");
    dir.write("proj/src/a.php", "<?php\n");
    dir.write("proj/gen/x.php", "<?php\n");
    let config = "paths = [\"src\"]\nexclude_paths = [\"nope\", \"gen\"]\n";
    let error = match session(&dir, config, ErrorPolicy::Fail) {
        Ok(_) => panic!("a missing literal path is not an error"),
        Err(error) => error,
    };
    assert!(error.contains("nope"), "{error}");

    let session = session(&dir, config, ErrorPolicy::Degrade).unwrap();
    assert!(
        session.warnings.iter().any(|w| w.contains("nope")),
        "{:?}",
        session.warnings
    );
    assert_eq!(
        listed(&session, &dir),
        vec![(PathBuf::from("src/a.php"), Role::Analysed)]
    );
}
