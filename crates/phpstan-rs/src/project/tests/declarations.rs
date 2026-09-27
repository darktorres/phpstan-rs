//! Where the registry records each declaration: its file and name span,
//! with no retained source needed, kept current as files are removed and
//! updated.

use super::super::*;

#[test]
fn classes_and_members_are_recorded_without_retain_source() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class Foo { public function bar(): void {} }",
        )
        .unwrap();
    project.finish_loading();

    let registry = project.registry();
    let (path, span) = registry.class_declaration("Foo").expect("Foo");
    assert_eq!(path, Path::new("a.php"));
    assert_eq!((span.start.line, span.start.col), (1, 13));
    let (path, _) = registry.method_declaration("Foo", "bar").expect("bar");
    assert_eq!(path, Path::new("a.php"));
    assert_eq!(
        registry.classes["Foo"].methods()["bar"]
            .provenance()
            .declaring_class(),
        Some("Foo")
    );
}

#[test]
fn an_inherited_member_keeps_its_declaring_class() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            "<?php class P { public function run(): void {} }\nclass C extends P {}",
        )
        .unwrap();
    project.finish_loading();

    let (_, sig) = project
        .registry()
        .lookup_method_with_owner("C", "run")
        .expect("run");
    assert_eq!(sig.provenance().declaring_class(), Some("P"));
}

#[test]
fn a_builtin_records_its_extension_and_no_file() {
    let project = Project::new();
    let info = &project.registry().classes["ArrayObject"];
    assert!(info.provenance().is_builtin());
    assert_eq!(info.provenance().extension_name(), Some("SPL"));
    assert_eq!(info.provenance().file(), None);
}

#[test]
fn removing_a_file_drops_its_declarations() {
    let mut project = Project::new();
    let path = PathBuf::from("a.php");
    project
        .load_file(path.clone(), "<?php class Foo {}")
        .unwrap();
    project.finish_loading();
    assert!(project.registry().class_declaration("Foo").is_some());

    project.remove_files(&[path]);
    assert!(project.registry().class_declaration("Foo").is_none());
}

#[test]
fn updating_a_file_records_its_new_declarations() {
    let mut project = Project::new();
    let path = PathBuf::from("a.php");
    project
        .load_file(path.clone(), "<?php class Foo {}")
        .unwrap();
    project.finish_loading();
    assert!(project.registry().class_declaration("Foo").is_some());

    project
        .update_file(&path, "<?php class Bar {}")
        .expect("update should succeed");
    assert!(project.registry().class_declaration("Foo").is_none());
    assert!(project.registry().class_declaration("Bar").is_some());
}
