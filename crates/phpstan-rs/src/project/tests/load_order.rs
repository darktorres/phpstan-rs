//! A file's place in the load order, and the role a file an edit adds gets:
//! what a load gives each file.

use super::super::roles::FileRoles;
use super::super::test_support::Files;
use super::super::*;

fn project() -> Project {
    let mut project = Project::new();
    project.apply_settings(ProjectSettings {
        roles: FileRoles::default().with_analysed(Path::new("/w/src")),
        keeps_as_built: true,
        ..ProjectSettings::default()
    });
    project
}

/// Every loaded file with its place in the load order, by place.
fn places(project: &Project) -> Vec<(String, u64)> {
    let mut places: Vec<(String, u64)> = project
        .files
        .iter()
        .map(|(path, state)| (path.display().to_string(), state.load_order))
        .collect();
    places.sort_by_key(|(_, place)| *place);
    places
}

/// A project that loads `context`, then `analysed`.
fn loaded(context: &[&str], analysed: &[&str]) -> Project {
    let mut files = Files::new().settings(ProjectSettings {
        roles: FileRoles::default().with_analysed(Path::new("/w/src")),
        keeps_as_built: true,
        ..ProjectSettings::default()
    });
    for path in context {
        files = files.context(path, "<?php function f() {}");
    }
    for path in analysed {
        files = files.analysed(path, "<?php function g() {}");
    }
    files.load()
}

#[test]
fn a_file_added_takes_the_place_a_load_gives_it() {
    let mut project = loaded(
        &["/w/vendor/b.php", "/w/vendor/d.php"],
        &["/w/src/a.php", "/w/src/c.php"],
    );
    project
        .update_file(Path::new("/w/src/b.php"), "<?php function h() {}")
        .unwrap();
    project
        .update_file(Path::new("/w/vendor/c.php"), "<?php function i() {}")
        .unwrap();
    project
        .update_file(Path::new("/w/src/d.php"), "<?php function j() {}")
        .unwrap();

    let fresh = loaded(
        &["/w/vendor/b.php", "/w/vendor/c.php", "/w/vendor/d.php"],
        &[
            "/w/src/a.php",
            "/w/src/b.php",
            "/w/src/c.php",
            "/w/src/d.php",
        ],
    );
    assert_eq!(places(&project), places(&fresh));
    assert_eq!(project.next_load_order, fresh.next_load_order);
}

#[test]
fn a_file_removed_closes_its_place() {
    let mut project = loaded(
        &["/w/vendor/a.php", "/w/vendor/b.php"],
        &["/w/src/a.php", "/w/src/b.php", "/w/src/c.php"],
    );
    project.remove_files(&[
        PathBuf::from("/w/vendor/a.php"),
        PathBuf::from("/w/src/b.php"),
        PathBuf::from("/w/src/missing.php"),
    ]);

    let fresh = loaded(&["/w/vendor/b.php"], &["/w/src/a.php", "/w/src/c.php"]);
    assert_eq!(places(&project), places(&fresh));
    assert_eq!(project.next_load_order, 3);
}

#[test]
fn a_file_removed_and_added_back_returns_to_its_place() {
    let mut project = loaded(&["/w/vendor/a.php"], &["/w/src/a.php", "/w/src/b.php"]);
    let before = places(&project);
    project.remove_file(Path::new("/w/src/a.php"));
    project
        .update_file(Path::new("/w/src/a.php"), "<?php function g() {}")
        .unwrap();
    assert_eq!(places(&project), before);
}

#[test]
fn an_edited_file_keeps_its_place() {
    let mut project = loaded(&["/w/vendor/a.php"], &["/w/src/a.php", "/w/src/b.php"]);
    let before = places(&project);
    project
        .update_file(Path::new("/w/src/a.php"), "<?php function g2() {}")
        .unwrap();
    assert_eq!(places(&project), before);
}

#[test]
fn a_file_added_is_analysed_under_an_analysed_root_and_context_elsewhere() {
    let mut project = project();
    for (path, analysed) in [
        ("/w/src/a/b.php", true),
        ("/w/src/vendor/x.php", true),
        ("/w/vendor/x/y.php", false),
        ("/elsewhere/z.php", false),
    ] {
        project
            .update_file(Path::new(path), "<?php function k() {}")
            .unwrap();
        assert_eq!(
            project.files.get(Path::new(path)).unwrap().analyzed,
            analysed,
            "{path}"
        );
    }
}

#[test]
fn a_file_added_by_a_spelling_from_the_working_directory_takes_the_absolute_roots_role() {
    let mut project = Project::new();
    project.apply_settings(ProjectSettings {
        working_dir: Some(PathBuf::from(".")),
        absolute_root: Some(PathBuf::from("/w")),
        roles: FileRoles::default().with_analysed(Path::new("/w/src")),
        keeps_as_built: true,
        ..ProjectSettings::default()
    });
    for (path, analysed) in [
        ("src/a.php", true),
        ("./src/b.php", true),
        ("lib/c.php", false),
    ] {
        project
            .update_file(Path::new(path), "<?php function k() {}")
            .unwrap();
        assert_eq!(
            project.files.get(Path::new(path)).unwrap().analyzed,
            analysed,
            "{path}"
        );
    }
}
