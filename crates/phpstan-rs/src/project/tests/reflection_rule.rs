//! `reflection_rule::ReflectionRule` + `Project::run_reflection_rules`.

use super::super::*;
use crate::project::session::load_test_file;
use crate::reflection_rule::ReflectionRule;
use crate::registry::{ClassReflection, FunctionReflection};
use diagnostics::{Diagnostic, DiagnosticBuilder};

struct FlagsEveryPublicClass;
impl ReflectionRule for FlagsEveryPublicClass {
    fn check_class(&self, class: &ClassReflection, fqn: &str) -> Vec<Diagnostic> {
        let _ = class;
        vec![
            DiagnosticBuilder::new(
                format!("saw class {fqn}"),
                syntax::span::Span::default(),
                0,
                "test.sawClass",
            )
            .build(&syntax::span::LineIndex::default()),
        ]
    }
}

struct FlagsEveryFunction;
impl ReflectionRule for FlagsEveryFunction {
    fn check_function(&self, sig: &FunctionReflection, fqn: &str) -> Vec<Diagnostic> {
        let _ = sig;
        vec![
            DiagnosticBuilder::new(
                format!("saw function {fqn}"),
                syntax::span::Span::default(),
                0,
                "test.sawFunction",
            )
            .build(&syntax::span::LineIndex::default()),
        ]
    }
}

#[test]
fn a_reflection_rule_sees_every_class_the_registry_knows_about() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("a.php"),
        "<?php class Foo {} class Bar {}",
        false,
    );
    project.finish_loading();

    let rules: Vec<Box<dyn ReflectionRule>> = vec![Box::new(FlagsEveryPublicClass)];
    let diags = project.run_reflection_rules(&rules);
    assert!(diags.iter().any(|d| d.message == "saw class Foo"));
    assert!(diags.iter().any(|d| d.message == "saw class Bar"));
}

#[test]
fn a_reflection_rule_sees_every_top_level_function() {
    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("a.php"),
        "<?php function myOwnUniqueFn() {}",
        false,
    );
    project.finish_loading();

    let rules: Vec<Box<dyn ReflectionRule>> = vec![Box::new(FlagsEveryFunction)];
    let diags = project.run_reflection_rules(&rules);
    assert!(
        diags
            .iter()
            .any(|d| d.message == "saw function myOwnUniqueFn")
    );
}

#[test]
fn default_trait_methods_are_true_no_ops() {
    struct DoesNothing;
    impl ReflectionRule for DoesNothing {}

    let mut project = Project::new();
    load_test_file(
        &mut project,
        PathBuf::from("a.php"),
        "<?php class Foo {} function f() {}",
        false,
    );
    project.finish_loading();

    let rules: Vec<Box<dyn ReflectionRule>> = vec![Box::new(DoesNothing)];
    assert!(project.run_reflection_rules(&rules).is_empty());
}
