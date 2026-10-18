//! The paths a configuration names are relative to the config file, a `\`
//! in one reads as a separator and as a character of a name, and the SQLite
//! file is one of them.

use super::super::session::{ErrorPolicy, Overrides, Session};
use super::helpers::{TempDir, resolved};
use crate::registry::stub_set::Stubs;
use config::ConfigFile;
use std::path::{Path, PathBuf};

/// The session `config`, written beside the files of `dir`, resolves to,
/// rooted at `dir/proj`.
fn resolve(dir: &TempDir, config: &str, policy: ErrorPolicy) -> Result<Session, String> {
    resolve_at(dir, "proj/phpstan-rs.toml", config, policy)
}

/// `resolve` for a config file kept at `config_file`, below `dir`, while the
/// project root stays `dir/proj`.
fn resolve_at(
    dir: &TempDir,
    config_file: &str,
    config: &str,
    policy: ErrorPolicy,
) -> Result<Session, String> {
    let path = dir.write(config_file, config);
    let config = ConfigFile::load(&path, &config::Schema::new())?;
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

/// `resolve` for a config that names no `paths` of its own: it lists
/// `proj/src`, which holds one file.
fn resolve_over_src(dir: &TempDir, config: &str, policy: ErrorPolicy) -> Result<Session, String> {
    dir.write("proj/src/a.php", "<?php\n");
    resolve(dir, &format!("paths = [\"src\"]\n{config}"), policy)
}

fn listed_names(session: &Session) -> Vec<String> {
    let mut names: Vec<String> = session
        .listing()
        .iter()
        .map(|listed| {
            listed
                .path
                .as_path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

const SQLITE: &str = "[ext.sql.reflection]\ndriver = \"sqlite\"\npath = \"db/app.sqlite\"\n";

#[test]
fn a_sqlite_path_is_relative_to_the_config_file() {
    let dir = TempDir::new("configured_sqlite_relative");
    let db = dir.write("proj/db/app.sqlite", "");
    let session = resolve_over_src(&dir, SQLITE, ErrorPolicy::Fail).unwrap();
    let reflection = session.editor_settings().sql_reflection.unwrap();
    assert_eq!(
        composer::canonical::canonicalize(&reflection.path.unwrap()).unwrap(),
        composer::canonical::canonicalize(&db).unwrap()
    );
}

#[test]
fn a_sqlite_path_is_relative_to_the_config_file_and_not_to_the_root() {
    let dir = TempDir::new("configured_sqlite_beside_the_config");
    dir.write("proj/conf/src/a.php", "<?php\n");
    dir.write("proj/db/app.sqlite", "");
    let beside_the_config = dir.write("proj/conf/db/app.sqlite", "");
    let session = resolve_at(
        &dir,
        "proj/conf/phpstan-rs.toml",
        &format!("paths = [\"src\"]\n{SQLITE}"),
        ErrorPolicy::Fail,
    )
    .unwrap();
    let reflection = session.editor_settings().sql_reflection.unwrap();
    assert_eq!(
        composer::canonical::canonicalize(&reflection.path.unwrap()).unwrap(),
        composer::canonical::canonicalize(&beside_the_config).unwrap()
    );
}

#[test]
fn a_sqlite_path_that_names_no_file_is_an_error_naming_it() {
    let dir = TempDir::new("configured_sqlite_missing");
    let error = resolve_over_src(&dir, SQLITE, ErrorPolicy::Fail)
        .err()
        .unwrap();
    assert!(
        error.starts_with("sql_reflection.path ") && error.ends_with("db/app.sqlite names no file"),
        "{error}"
    );
}

#[test]
fn an_editor_keeps_going_without_the_sqlite_reflection_when_its_path_names_no_file() {
    let dir = TempDir::new("configured_sqlite_missing_degrade");
    let session = resolve_over_src(&dir, SQLITE, ErrorPolicy::Degrade).unwrap();
    assert_eq!(session.editor_settings().sql_reflection.unwrap().path, None);
    assert!(
        session
            .warnings
            .iter()
            .any(|warning| warning.ends_with("db/app.sqlite names no file")),
        "{:?}",
        session.warnings
    );
}

#[cfg(unix)]
#[test]
fn a_backslash_in_the_baseline_reads_as_a_separator() {
    let dir = TempDir::new("configured_baseline_separator");
    dir.write("proj/ci/base.json", "{}");
    let session =
        resolve_over_src(&dir, "baseline = \"ci\\\\base.json\"\n", ErrorPolicy::Fail).unwrap();
    assert_eq!(
        session.baseline_path().unwrap().file_name().unwrap(),
        "base.json"
    );
    assert_eq!(
        session
            .baseline_path()
            .unwrap()
            .parent()
            .unwrap()
            .file_name()
            .unwrap(),
        "ci"
    );
}

#[cfg(unix)]
#[test]
fn a_backslash_in_the_baseline_reads_as_a_name_when_only_that_file_exists() {
    let dir = TempDir::new("configured_baseline_name");
    dir.write("proj/ci\\base.json", "{}");
    let session =
        resolve_over_src(&dir, "baseline = \"ci\\\\base.json\"\n", ErrorPolicy::Fail).unwrap();
    assert_eq!(
        session.baseline_path().unwrap().file_name().unwrap(),
        "ci\\base.json"
    );
}

#[cfg(unix)]
#[test]
fn a_baseline_that_both_readings_name_is_an_error_naming_both() {
    let dir = TempDir::new("configured_baseline_ambiguous");
    dir.write("proj/ci/base.json", "{}");
    dir.write("proj/ci\\base.json", "{}");
    let error = resolve_over_src(&dir, "baseline = \"ci\\\\base.json\"\n", ErrorPolicy::Fail)
        .err()
        .unwrap();
    assert!(error.contains("names two files"), "{error}");
    assert!(error.contains("ci/base.json and ci\\base.json"), "{error}");
}

#[cfg(unix)]
#[test]
fn a_baseline_that_both_readings_name_is_dropped_with_a_warning_in_an_editor() {
    let dir = TempDir::new("configured_baseline_ambiguous_degrade");
    dir.write("proj/ci/base.json", "{}");
    dir.write("proj/ci\\base.json", "{}");
    let session = resolve_over_src(
        &dir,
        "baseline = \"ci\\\\base.json\"\n",
        ErrorPolicy::Degrade,
    )
    .unwrap();
    assert_eq!(session.baseline_path(), None);
    assert!(
        session
            .warnings
            .iter()
            .any(|w| w.contains("names two files"))
    );
}

#[cfg(unix)]
#[test]
fn a_backslash_in_paths_lists_the_directory_the_separator_reading_names() {
    let dir = TempDir::new("configured_paths_separator");
    dir.write("proj/src/app/a.php", "<?php\n");
    dir.write("proj/other/b.php", "<?php\n");
    let session = resolve(&dir, "paths = [\"src\\\\app\"]\n", ErrorPolicy::Fail).unwrap();
    assert_eq!(listed_names(&session), vec!["a.php".to_string()]);
}

#[cfg(unix)]
#[test]
fn a_backslash_in_paths_lists_the_directory_a_name_holds_and_the_one_a_separator_names() {
    let dir = TempDir::new("configured_paths_both");
    dir.write("proj/src/app/a.php", "<?php\n");
    dir.write("proj/src\\app/b.php", "<?php\n");
    let session = resolve(&dir, "paths = [\"src\\\\app\"]\n", ErrorPolicy::Fail).unwrap();
    assert_eq!(
        listed_names(&session),
        vec!["a.php".to_string(), "b.php".to_string()]
    );
}

#[test]
fn a_path_without_a_backslash_is_listed_as_it_is() {
    let dir = TempDir::new("configured_paths_plain");
    dir.write("proj/src/a.php", "<?php\n");
    let session = resolve(&dir, "paths = [\"src\"]\n", ErrorPolicy::Fail).unwrap();
    assert_eq!(listed_names(&session), vec!["a.php".to_string()]);
    let _: Option<PathBuf> = session.baseline_path().map(Path::to_path_buf);
}
