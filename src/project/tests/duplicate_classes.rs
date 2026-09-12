//! Phase 83: `checker::duplicate_classes::check_duplicate_class_declarations`,
//! opt-in (unlike `duplicate_functions`'s own always-on equivalent - see
//! `registry::Registry::check_duplicate_class_declarations`'s own doc
//! comment for why) - a top-level class-like declared more than once,
//! unconditionally, across the whole project.

use super::super::*;

#[test]
fn two_files_each_unconditionally_declaring_the_same_class_are_both_flagged() {
    let mut project = Project::new();
    project.set_check_duplicate_class_declarations(true);
    project
        .load_file(PathBuf::from("a.php"), "<?php class Foo {}")
        .unwrap();
    project
        .load_file(PathBuf::from("b.php"), "<?php class Foo {}")
        .unwrap();
    project.finish_loading();

    let a_diags = project.check_file(&PathBuf::from("a.php"));
    let b_diags = project.check_file(&PathBuf::from("b.php"));
    assert!(
        a_diags.iter().any(|d| d.identifier == "class.duplicate"),
        "got: {a_diags:?}"
    );
    assert!(
        b_diags.iter().any(|d| d.identifier == "class.duplicate"),
        "got: {b_diags:?}"
    );
    let msg = &a_diags
        .iter()
        .find(|d| d.identifier == "class.duplicate")
        .unwrap()
        .message;
    assert!(msg.contains("a.php") && msg.contains("b.php"), "got: {msg}");
}

#[test]
fn the_check_is_silent_when_not_opted_in() {
    let mut project = Project::new();
    project
        .load_file(PathBuf::from("a.php"), "<?php class Foo {}")
        .unwrap();
    project
        .load_file(PathBuf::from("b.php"), "<?php class Foo {}")
        .unwrap();
    project.finish_loading();

    let a_diags = project.check_file(&PathBuf::from("a.php"));
    assert!(
        !a_diags.iter().any(|d| d.identifier == "class.duplicate"),
        "got: {a_diags:?}"
    );
}

#[test]
fn each_class_like_kind_gets_its_own_identifier() {
    for (src, identifier) in [
        ("<?php interface Foo {}", "interface.duplicate"),
        ("<?php trait Foo {}", "trait.duplicate"),
        ("<?php enum Foo {}", "enum.duplicate"),
    ] {
        let mut project = Project::new();
        project.set_check_duplicate_class_declarations(true);
        project.load_file(PathBuf::from("a.php"), src).unwrap();
        project.load_file(PathBuf::from("b.php"), src).unwrap();
        project.finish_loading();

        let diags = project.check_file(&PathBuf::from("a.php"));
        assert!(
            diags.iter().any(|d| d.identifier == identifier),
            "src {src:?}, got: {diags:?}"
        );
    }
}

#[test]
fn a_class_exists_guarded_declaration_duplicated_across_two_files_is_not_flagged() {
    let mut project = Project::new();
    project.set_check_duplicate_class_declarations(true);
    let src = "<?php if (!class_exists(Foo::class)) { class Foo {} }";
    project.load_file(PathBuf::from("a.php"), src).unwrap();
    project.load_file(PathBuf::from("b.php"), src).unwrap();
    project.finish_loading();

    let a_diags = project.check_file(&PathBuf::from("a.php"));
    assert!(
        !a_diags.iter().any(|d| d.identifier == "class.duplicate"),
        "got: {a_diags:?}"
    );
}

#[test]
fn an_anonymous_class_never_appears_in_the_site_table() {
    let mut project = Project::new();
    project.set_check_duplicate_class_declarations(true);
    let src = "<?php function make() { return new class {}; }";
    project.load_file(PathBuf::from("a.php"), src).unwrap();
    project.load_file(PathBuf::from("b.php"), src).unwrap();
    project.finish_loading();

    let a_diags = project.check_file(&PathBuf::from("a.php"));
    assert!(
        !a_diags.iter().any(|d| d.identifier == "class.duplicate"),
        "got: {a_diags:?}"
    );
}

/// The Imagick-shaped scenario: a real class (the "extension" version)
/// and a vendored override stub (`is_stub: true`) declaring the identical
/// name - the stub must never be flagged, and must never count toward
/// flagging the real declaration either, so exactly zero `class.
/// duplicate` diagnostics come out of either file.
#[test]
fn a_stub_tagged_site_is_never_flagged_and_never_counts_toward_flagging_the_real_declaration() {
    let mut project = Project::new();
    project.set_check_duplicate_class_declarations(true);

    let real_path = PathBuf::from("real.php");
    let real_src = "<?php class Imagick {}";
    let real_parsed = parse_file(&real_path, real_src, false).unwrap();
    project.insert_parsed_file(real_path.clone(), real_parsed, real_src.to_string(), true);

    let stub_path = PathBuf::from("Imagick.stub.php");
    let stub_src = "<?php class Imagick {}";
    let stub_parsed = parse_file(&stub_path, stub_src, true).unwrap();
    project.insert_parsed_file(stub_path.clone(), stub_parsed, stub_src.to_string(), false);

    project.finish_loading();

    let real_diags = project.check_file(&real_path);
    let stub_diags = project.check_file(&stub_path);
    assert!(
        !real_diags.iter().any(|d| d.identifier == "class.duplicate"),
        "got: {real_diags:?}"
    );
    assert!(
        !stub_diags.iter().any(|d| d.identifier == "class.duplicate"),
        "got: {stub_diags:?}"
    );
}

/// Negative control for the stub test above: two *non*-stub-tagged sites
/// for the same name must still be flagged - proves the stub exclusion
/// above is really testing the `is_stub` field, not some other reason
/// the pair went silent.
#[test]
fn two_non_stub_sites_for_the_same_name_are_still_flagged() {
    let mut project = Project::new();
    project.set_check_duplicate_class_declarations(true);

    let a_path = PathBuf::from("a.php");
    let a_src = "<?php class Imagick {}";
    let a_parsed = parse_file(&a_path, a_src, false).unwrap();
    project.insert_parsed_file(a_path.clone(), a_parsed, a_src.to_string(), true);

    let b_path = PathBuf::from("b.php");
    let b_src = "<?php class Imagick {}";
    let b_parsed = parse_file(&b_path, b_src, false).unwrap();
    project.insert_parsed_file(b_path.clone(), b_parsed, b_src.to_string(), true);

    project.finish_loading();

    let a_diags = project.check_file(&a_path);
    assert!(
        a_diags.iter().any(|d| d.identifier == "class.duplicate"),
        "got: {a_diags:?}"
    );
}

#[test]
fn an_update_file_edit_introduces_then_removes_a_duplicate() {
    let mut project = Project::new();
    project.set_check_duplicate_class_declarations(true);
    project
        .load_file(PathBuf::from("a.php"), "<?php class Foo {}")
        .unwrap();
    project.finish_loading();

    let results = project
        .update_file(&PathBuf::from("b.php"), "<?php class Foo {}")
        .unwrap();
    assert!(
        results
            .values()
            .flatten()
            .any(|d| d.identifier == "class.duplicate"),
        "got: {results:?}"
    );

    let results = project
        .update_file(&PathBuf::from("b.php"), "<?php class Bar {}")
        .unwrap();
    assert!(
        !results
            .values()
            .flatten()
            .any(|d| d.identifier == "class.duplicate"),
        "got: {results:?}"
    );
}
