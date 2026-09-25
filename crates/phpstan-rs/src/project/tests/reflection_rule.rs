//! `reflection_rule::ReflectionRule` + `Project::run_reflection_rules`.

use super::super::*;
use crate::diagnostic::Diagnostic;
use crate::reflection_rule::ReflectionRule;
use crate::registry::{ClassInfo, FunctionSig};

struct FlagsEveryPublicClass;
impl ReflectionRule for FlagsEveryPublicClass {
    fn check_class(&self, class: &ClassInfo, fqn: &str) -> Vec<Diagnostic> {
        let _ = class;
        vec![Diagnostic {
            location: fqn.to_string(),
            message: format!("saw class {fqn}"),
            span: syntax::token::Span::default(),
            level: 0,
            identifier: "test.sawClass".to_string(),
            tip: None,
            non_ignorable: false,
            metadata: rustc_hash::FxHashMap::default(),
            fix: None,
        }]
    }
}

struct FlagsEveryFunction;
impl ReflectionRule for FlagsEveryFunction {
    fn check_function(&self, sig: &FunctionSig, fqn: &str) -> Vec<Diagnostic> {
        let _ = sig;
        vec![Diagnostic {
            location: fqn.to_string(),
            message: format!("saw function {fqn}"),
            span: syntax::token::Span::default(),
            level: 0,
            identifier: "test.sawFunction".to_string(),
            tip: None,
            non_ignorable: false,
            metadata: rustc_hash::FxHashMap::default(),
            fix: None,
        }]
    }
}

#[test]
fn a_reflection_rule_sees_every_class_the_registry_knows_about() {
    let mut project = Project::new();
    project
        .load_file(PathBuf::from("a.php"), "<?php class Foo {} class Bar {}")
        .unwrap();
    project.finish_loading();

    let rules: Vec<Box<dyn ReflectionRule>> = vec![Box::new(FlagsEveryPublicClass)];
    let diags = project.run_reflection_rules(&rules);
    assert!(diags.iter().any(|d| d.message == "saw class Foo"));
    assert!(diags.iter().any(|d| d.message == "saw class Bar"));
}

#[test]
fn a_reflection_rule_sees_every_top_level_function() {
    let mut project = Project::new();
    project
        .load_file(PathBuf::from("a.php"), "<?php function myOwnUniqueFn() {}")
        .unwrap();
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
    project
        .load_file(PathBuf::from("a.php"), "<?php class Foo {} function f() {}")
        .unwrap();
    project.finish_loading();

    let rules: Vec<Box<dyn ReflectionRule>> = vec![Box::new(DoesNothing)];
    assert!(project.run_reflection_rules(&rules).is_empty());
}
