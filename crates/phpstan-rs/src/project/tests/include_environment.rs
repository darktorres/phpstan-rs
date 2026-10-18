//! What the include rule is given: the configured include path and working
//! directories, the values the analysed files set the include path to, and
//! the directories scripts run from. An edit that moves any of them
//! re-checks every analysed file that includes something; any other edit
//! does not.

use super::super::session::{ErrorPolicy, Overrides, Session};
use super::helpers::{TempDir, resolved};
use crate::registry::stub_set::Stubs;
use config::ConfigFile;
use std::path::{Path, PathBuf};

use super::super::*;

/// A project rooted at `dir/proj`, configured with `config` and loaded as the
/// language server loads one, so that it can be edited.
fn loaded(dir: &TempDir, config: &str) -> Project {
    let path = dir.write("proj/phpstan-rs.toml", config);
    let config = ConfigFile::load(&path, &config::Schema::new()).unwrap();
    let session = Session::resolve(
        &config,
        config.dir().map(Path::to_path_buf),
        resolved(dir).join("proj"),
        Overrides {
            paths: vec![PathBuf::new()],
            no_composer: true,
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

/// The paths the include rule reports as missing in `file`, as the messages
/// write them.
fn missing_in(project: &Project, file: &Path) -> Vec<String> {
    project
        .check_file(file)
        .iter()
        .filter(|d| d.identifier.ends_with(".fileNotFound"))
        .map(|d| {
            let start = d.message.find('"').unwrap() + 1;
            let end = d.message.rfind('"').unwrap();
            d.message[start..end].to_string()
        })
        .collect()
}

/// The names of the files that `affected` has numbers of.
fn names(project: &Project, affected: impl IntoIterator<Item = FileId>) -> Vec<String> {
    let mut names: Vec<String> = affected
        .into_iter()
        .map(|id| {
            project
                .path_of(id)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// The names of the files that an edit of `path` to `source` re-checked.
fn edit(project: &mut Project, path: &Path, source: &str) -> Vec<String> {
    let affected = project.update_file(path, source).unwrap();
    names(project, affected.into_keys())
}

const SETS_THE_INCLUDE_PATH: &str = "<?php\nset_include_path(__DIR__ . '/lib');\n";

#[test]
fn the_configured_include_path_and_working_directories_reach_the_rule() {
    let dir = TempDir::new("include_env_configured");
    dir.write(
        "proj/main.php",
        "<?php\nrequire 'present.php';\nrequire 'gone.php';\n",
    );
    dir.write("proj/lib/present.php", "<?php\n");
    let project = loaded(&dir, "include_path = \"lib\"\nworking_dirs = [\".\"]\n");
    let main = resolved(&dir).join("proj/main.php");
    assert_eq!(missing_in(&project, &main), ["gone.php"]);
}

#[test]
fn without_an_include_path_a_plain_name_is_not_judged() {
    let dir = TempDir::new("include_env_undeclared");
    dir.write("proj/main.php", "<?php\nrequire 'gone.php';\n");
    let project = loaded(&dir, "");
    let main = resolved(&dir).join("proj/main.php");
    assert!(missing_in(&project, &main).is_empty());
}

#[test]
fn an_edit_that_sets_the_include_path_re_checks_the_files_that_include_and_clears_the_reports() {
    let dir = TempDir::new("include_env_edit");
    dir.write("proj/main.php", "<?php\nrequire 'present.php';\n");
    dir.write("proj/second.php", "<?php\nrequire 'present.php';\n");
    dir.write("proj/other.php", "<?php\n");
    dir.write("proj/bootstrap.php", "<?php\n");
    dir.write("proj/lib/present.php", "<?php\n");
    let mut project = loaded(&dir, "include_path = \".\"\nworking_dirs = [\".\"]\n");
    let root = resolved(&dir).join("proj");
    let (main, bootstrap) = (root.join("main.php"), root.join("bootstrap.php"));
    assert_eq!(missing_in(&project, &main), ["present.php"]);

    // `other.php` and `present.php` include nothing, so nothing they report
    // can change.
    let rechecked = edit(&mut project, &bootstrap, SETS_THE_INCLUDE_PATH);
    assert_eq!(rechecked, ["bootstrap.php", "main.php", "second.php"]);
    assert!(missing_in(&project, &main).is_empty());

    // The same value again moves nothing, so nothing else is re-checked.
    let again = format!("{SETS_THE_INCLUDE_PATH}\n");
    assert_eq!(edit(&mut project, &bootstrap, &again), ["bootstrap.php"]);

    // Taking the value out brings the report back.
    let rechecked = edit(&mut project, &bootstrap, "<?php\n");
    assert!(rechecked.contains(&"main.php".to_string()), "{rechecked:?}");
    assert_eq!(missing_in(&project, &main), ["present.php"]);
}

#[test]
fn an_edit_that_moves_the_working_directory_re_checks_the_files_that_include() {
    let dir = TempDir::new("include_env_chdir");
    dir.write("proj/main.php", "<?php\nrequire './present.php';\n");
    dir.write("proj/other.php", "<?php\n");
    dir.write("proj/bootstrap.php", "<?php\n");
    dir.write("proj/lib/present.php", "<?php\n");
    let mut project = loaded(&dir, "include_path = \".\"\nworking_dirs = [\".\"]\n");
    let root = resolved(&dir).join("proj");
    let (main, bootstrap) = (root.join("main.php"), root.join("bootstrap.php"));
    assert_eq!(missing_in(&project, &main), ["./present.php"]);

    // Into a directory that holds it.
    let rechecked = edit(
        &mut project,
        &bootstrap,
        "<?php\nchdir(__DIR__ . '/lib');\n",
    );
    assert_eq!(rechecked, ["bootstrap.php", "main.php"]);
    assert!(missing_in(&project, &main).is_empty());

    // Into one that cannot be named, where anything relative may be.
    edit(&mut project, &bootstrap, "<?php\nchdir($argv[1]);\n");
    assert!(missing_in(&project, &main).is_empty());

    // Taking the call out brings the report back.
    edit(&mut project, &bootstrap, "<?php\n");
    assert_eq!(missing_in(&project, &main), ["./present.php"]);
}

#[test]
fn an_edit_of_a_file_that_does_not_set_the_include_path_re_checks_only_itself() {
    let dir = TempDir::new("include_env_unrelated_edit");
    dir.write("proj/main.php", "<?php\nrequire 'present.php';\n");
    dir.write("proj/other.php", "<?php\n");
    let mut project = loaded(&dir, "include_path = \".\"\nworking_dirs = [\".\"]\n");
    let other = resolved(&dir).join("proj/other.php");
    assert_eq!(
        edit(&mut project, &other, "<?php\n$a = 1;\n"),
        ["other.php"]
    );
}

#[test]
fn removing_the_file_that_sets_the_include_path_re_checks_the_files_that_include() {
    let dir = TempDir::new("include_env_removal");
    dir.write("proj/main.php", "<?php\nrequire 'present.php';\n");
    dir.write("proj/other.php", "<?php\n");
    dir.write("proj/bootstrap.php", SETS_THE_INCLUDE_PATH);
    dir.write("proj/lib/present.php", "<?php\n");
    let mut project = loaded(&dir, "include_path = \".\"\nworking_dirs = [\".\"]\n");
    let root = resolved(&dir).join("proj");
    let main = root.join("main.php");
    assert!(missing_in(&project, &main).is_empty());

    let affected = project.remove_file(&root.join("bootstrap.php"));
    let rechecked = names(&project, affected.into_keys());
    assert_eq!(rechecked, ["bootstrap.php", "main.php"]);
    assert_eq!(missing_in(&project, &main), ["present.php"]);
}

#[test]
fn a_value_that_cannot_be_computed_opens_the_include_path_until_it_is_edited_out() {
    let dir = TempDir::new("include_env_open");
    dir.write("proj/main.php", "<?php\nrequire 'gone.php';\n");
    dir.write(
        "proj/bootstrap.php",
        "<?php\nset_include_path($_SERVER['LIB']);\n",
    );
    let mut project = loaded(&dir, "include_path = \".\"\nworking_dirs = [\".\"]\n");
    let root = resolved(&dir).join("proj");
    let main = root.join("main.php");
    assert!(missing_in(&project, &main).is_empty());

    edit(&mut project, &root.join("bootstrap.php"), "<?php\n");
    assert_eq!(missing_in(&project, &main), ["gone.php"]);
}

#[test]
fn a_new_directory_among_the_scripts_re_checks_the_files_that_include_when_working_directories_are_undeclared()
 {
    let dir = TempDir::new("include_env_script_dirs");
    dir.write("proj/main.php", "<?php\nrequire './helper.php';\n");
    let mut project = loaded(&dir, "include_path = \".\"\n");
    let root = resolved(&dir).join("proj");
    let main = root.join("main.php");
    assert_eq!(missing_in(&project, &main), ["./helper.php"]);

    // A script in `sub` runs there, where `./helper.php` is its sibling.
    dir.write("proj/sub/helper.php", "<?php\n");
    let rechecked = edit(&mut project, &root.join("sub/helper.php"), "<?php\n");
    assert!(rechecked.contains(&"main.php".to_string()), "{rechecked:?}");
    assert!(missing_in(&project, &main).is_empty());
}

#[test]
fn a_new_file_in_a_directory_scripts_already_run_from_re_checks_only_itself() {
    let dir = TempDir::new("include_env_same_dirs");
    dir.write("proj/main.php", "<?php\nrequire './helper.php';\n");
    let mut project = loaded(&dir, "include_path = \".\"\n");
    let root = resolved(&dir).join("proj");
    dir.write("proj/added.php", "<?php\n");
    assert_eq!(
        edit(&mut project, &root.join("added.php"), "<?php\n"),
        ["added.php"]
    );
}
