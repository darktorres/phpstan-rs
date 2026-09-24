//! `Project::decl_locations()` - unlike Tier-1 `use_sites`/
//! `FileState.source`, this needs no retained source text at all
//! (`DeclLocations::index` takes only `&ast`/`&path`), so it's populated
//! unconditionally regardless of `retain_source` - including in the
//! default, batch-`check-project` shape (`retain_source` left `false`),
//! which is exactly what `Project::check_whole_program` needs to resolve
//! a whole-program fact's own declaring file/span.

use super::super::*;

#[test]
fn classes_and_members_are_indexed_without_retain_source() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project.finish_loading();

    let locs = project.decl_locations();
    assert!(locs.classes.contains_key("Foo"), "got: {:?}", locs.classes);
    assert!(
        locs.members
            .contains_key(&("Foo".to_string(), "bar".to_string())),
        "got: {:?}",
        locs.members
    );
}

#[test]
fn removing_a_file_drops_its_decl_locations() {
    let mut project = Project::new();
    let path = PathBuf::from("a.php");
    project
        .load_file(path.clone(), "<?php class Foo {}")
        .unwrap();
    project.finish_loading();
    assert!(project.decl_locations().classes.contains_key("Foo"));

    project.remove_files(&[path]);
    assert!(!project.decl_locations().classes.contains_key("Foo"));
}

#[test]
fn updating_a_file_reindexes_its_decl_locations() {
    let mut project = Project::new();
    let path = PathBuf::from("a.php");
    project
        .load_file(path.clone(), "<?php class Foo {}")
        .unwrap();
    project.finish_loading();
    assert!(project.decl_locations().classes.contains_key("Foo"));

    project
        .update_file(&path, "<?php class Bar {}")
        .expect("update should succeed");
    assert!(!project.decl_locations().classes.contains_key("Foo"));
    assert!(project.decl_locations().classes.contains_key("Bar"));
}
