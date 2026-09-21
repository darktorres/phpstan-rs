//! `checker::whole_program::renamed_named_arguments` (Phase 82), wired
//! through `Project::check_whole_program` - opt-in, off by default (see
//! `registry::Registry::check_renamed_named_arguments`'s own doc
//! comment). Every fixture loads via `load_analyzed_file`, not
//! `load_file` - see `dead_enum_cases`'s own module doc comment for why.

use super::super::*;

fn project_with_check_enabled() -> Project {
    let mut project = Project::new();
    project.set_check_renamed_named_arguments(true);
    project
}

const FIXTURE: &str = r#"<?php
    class Base {
        public function f(int $old): void {}
    }
    class Sub extends Base {
        public function f(int $new): void {}
    }
    function call_it(Base $b): void {
        $b->f(old: 1);
    }
"#;

#[test]
fn off_by_default() {
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("a.php"), FIXTURE)
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    assert!(results.is_empty(), "got: {results:?}");
}

#[test]
fn a_named_argument_call_colliding_with_a_renamed_override_is_flagged_when_enabled() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project.load_analyzed_file(path.clone(), FIXTURE).unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).expect("expected findings for a.php");
    assert!(
        diags
            .iter()
            .any(|d| d.identifier == "argument.parameterRenamedInSubtype"
                && d.message.contains("Base::f")
                && d.message.contains("$old")
                && d.message.contains("Sub")
                && d.message.contains("$new")),
        "got: {diags:?}"
    );
}

#[test]
fn a_call_using_the_still_current_parameter_name_is_not_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            r#"<?php
                class Base {
                    public function f(int $x): void {}
                }
                class Sub extends Base {
                    public function f(int $x): void {}
                }
                function call_it(Base $b): void {
                    $b->f(x: 1);
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags
            .iter()
            .any(|d| d.identifier == "argument.parameterRenamedInSubtype"),
        "got: {diags:?}"
    );
}

#[test]
fn a_call_against_a_final_class_is_never_flagged() {
    // A final class's own method can never actually dispatch to a
    // different override at runtime - no rename collision is possible.
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            r#"<?php
                final class Base {
                    public function f(int $old): void {}
                }
                function call_it(Base $b): void {
                    $b->f(old: 1);
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags
            .iter()
            .any(|d| d.identifier == "argument.parameterRenamedInSubtype"),
        "got: {diags:?}"
    );
}

#[test]
fn a_call_against_a_private_method_is_never_flagged() {
    let mut project = project_with_check_enabled();
    let path = PathBuf::from("a.php");
    project
        .load_analyzed_file(
            path.clone(),
            r#"<?php
                class Base {
                    private function f(int $old): void {
                        $this->f(old: 1);
                    }
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let results = project.check_whole_program();
    let diags = results.get(&path).cloned().unwrap_or_default();
    assert!(
        !diags
            .iter()
            .any(|d| d.identifier == "argument.parameterRenamedInSubtype"),
        "got: {diags:?}"
    );
}
