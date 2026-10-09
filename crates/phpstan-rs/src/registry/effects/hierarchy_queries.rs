//! The hierarchy queries effect resolution runs, on small class hierarchies:
//! what a call may reach through overrides, `__call`, `@method` tags and a
//! builtin's methods. Results are compared as sorted `class::member` names,
//! and with the direct versions in `reference`.

use super::*;
use syntax::{lexer::Lexer, parser::Parser};

fn registry(source: &str) -> Registry {
    let tokens = Lexer::new(source).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    Registry::build(&program.arenas, program.statements())
}

fn names(keys: impl IntoIterator<Item = DeclKey>) -> Vec<String> {
    let mut out: Vec<String> = keys
        .into_iter()
        .map(|key| match key.class {
            Some(class) => format!("{class}::{}", key.member),
            None => key.member,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

struct NoContext;

impl CallContext for NoContext {
    fn class(&self) -> Option<&str> {
        None
    }
    fn file(&self) -> Option<FileId> {
        None
    }
    fn receiver(&self, _: &Receiver) -> (Holder, bool) {
        (Holder::Unknown, false)
    }
    fn argument(&self, _: &ArgAt, _: Option<&Place>) -> Holder {
        Holder::Unknown
    }
    fn runs_written(&self, _: Written<'_>, _: &Transfer) -> bool {
        false
    }
    fn operand(&self, _: Option<&[Origin]>) -> (Holder, bool) {
        (Holder::Unknown, false)
    }
    fn rebindable(&self) -> bool {
        false
    }
}

fn target_names(targets: impl IntoIterator<Item = (DeclKey, Transfer)>) -> Vec<String> {
    let mut out: Vec<String> = targets
        .into_iter()
        .map(|(key, transfer)| {
            let key = names([key]).remove(0);
            format!("{key} {transfer:?}")
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn implicit(registry: &Registry, holder: &Holder, operation: &Operation) -> Vec<String> {
    let reach = Reach {
        registry,
        ctx: &NoContext,
    };
    let (mut found, mut expected) = (TargetSet::default(), Vec::new());
    reach.implicit_targets(holder, false, operation, &mut found, &mut Vec::new());
    reach.reference_implicit_targets(holder, false, operation, &mut expected, &mut Vec::new());
    let (found, expected) = (target_names(found.into_vec()), target_names(expected));
    assert_eq!(found, expected);
    found
}

fn classes(names: &[&str]) -> Holder {
    Holder::Classes(names.iter().map(|name| Name::from(*name)).collect())
}

const SHAPES: &str = r#"<?php
    interface Shape { public function area(): float; }
    abstract class Base implements Shape {
        public function area(): float { return 0.0; }
        public function name(): string { return 'base'; }
    }
    class Circle extends Base { public function area(): float { return 1.0; } }
    class Square extends Base {}
    final class Tiny extends Base { public function name(): string { return 't'; } }
    class Magic { public function __call($name, $arguments) {} }
    class SubMagic extends Magic { public function run() {} }
"#;

#[test]
fn an_overridable_method_reaches_each_override_below() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.implementations("Base", "name").to_vec()),
        ["base::name", "tiny::name"]
    );
    assert_eq!(
        names(registry.implementations("Base", "area").to_vec()),
        ["base::area", "circle::area"]
    );
}

#[test]
fn a_method_nothing_overrides_reaches_only_itself() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.implementations("Circle", "area").to_vec()),
        ["circle::area"]
    );
    assert_eq!(
        names(registry.implementations("Tiny", "name").to_vec()),
        ["tiny::name"]
    );
}

#[test]
fn a_missing_method_reaches_the_call_method_of_each_class() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.implementations("Magic", "missing").to_vec()),
        ["magic::__call"]
    );
    assert!(registry.implementations("Square", "missing").is_empty());
}

#[test]
fn a_name_on_an_unknown_object_reaches_every_declaration_and_every_call_method() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.methods_named("area").to_vec()),
        ["base::area", "circle::area", "magic::__call", "shape::area"]
    );
    assert_eq!(
        names(registry.methods_named("NAME").to_vec()),
        ["base::name", "magic::__call", "tiny::name"]
    );
}

#[test]
fn the_overrides_of_a_builtin_are_taken_as_the_receiver_resolves_them() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.overrides_of_builtin("Base", "Shape").to_vec()),
        ["base::area", "circle::area"]
    );
    assert_eq!(
        names(registry.overrides_of_builtin("Circle", "Shape").to_vec()),
        ["circle::area"]
    );
    assert!(registry.overrides_of_builtin("Base", "Missing").is_empty());
}

const GRAPH: &str = r#"<?php
    interface Root { public function root(): int; }
    interface Left extends Root {}
    interface Right extends Root {}
    class Both implements Left, Right { public function root(): int { return 1; } }
    class SubBoth extends Both { public function root(): int { return 2; } }
    trait Greets { public function greet(): string { return 'hi'; } }
    class Greeter { use Greets; }
    class LoudGreeter extends Greeter { public function greet(): string { return 'HI'; } }
    /** @method int ghost() */
    class Tagged { public function __call($name, $arguments) {} }
    class Row implements IteratorAggregate {
        public function getIterator(): Iterator { return new RowIt(); }
    }
    class RowIt implements Iterator {
        public function current(): mixed { return 1; }
        public function key(): mixed { return 1; }
        public function next(): void {}
        public function rewind(): void {}
        public function valid(): bool { return false; }
    }
    class Mixed_Case { public function DoThing() {} }
    class sub_case extends MIXED_CASE { public function DOTHING() {} }
"#;

fn all_implementations(registry: &Registry, class: &str, method: &str) -> Vec<String> {
    let found = names(registry.implementations(class, method).to_vec());
    assert_eq!(
        found,
        names(registry.reference_implementations(class, method))
    );
    found
}

#[test]
fn an_interface_diamond_reaches_each_implementation_once() {
    let registry = registry(GRAPH);
    assert_eq!(
        all_implementations(&registry, "Root", "root"),
        ["both::root", "root::root", "subboth::root"]
    );
    assert_eq!(
        all_implementations(&registry, "Left", "root"),
        ["both::root", "root::root", "subboth::root"]
    );
    assert_eq!(
        all_implementations(&registry, "Both", "root"),
        ["both::root", "subboth::root"]
    );
}

#[test]
fn a_trait_method_is_reached_as_the_using_class_declares_it() {
    let registry = registry(GRAPH);
    let found = all_implementations(&registry, "Greeter", "greet");
    assert!(
        found.contains(&"loudgreeter::greet".to_string()),
        "{found:?}"
    );
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(
        all_implementations(&registry, "LoudGreeter", "greet"),
        ["loudgreeter::greet"]
    );
}

#[test]
fn a_method_only_a_tag_declares_reaches_the_call_method() {
    let registry = registry(GRAPH);
    assert_eq!(
        all_implementations(&registry, "Tagged", "ghost"),
        ["tagged::__call"]
    );
    let found = names(registry.methods_named("GHOST").to_vec());
    assert_eq!(found, names(registry.reference_methods_named("GHOST")));
    assert_eq!(found, ["tagged::__call"]);
}

#[test]
fn method_names_differing_in_case_are_the_same_method() {
    let registry = registry(GRAPH);
    let found = names(registry.methods_named("dothing").to_vec());
    assert_eq!(found, names(registry.reference_methods_named("dothing")));
    assert_eq!(
        found,
        ["mixed_case::dothing", "sub_case::dothing", "tagged::__call"]
    );
    assert_eq!(
        all_implementations(&registry, "MIXED_CASE", "DOTHING"),
        ["mixed_case::dothing", "sub_case::dothing"]
    );
}

#[test]
fn every_method_of_a_class_and_the_classes_below_it() {
    let registry = registry(GRAPH);
    let reach = Reach {
        registry: &registry,
        ctx: &NoContext,
    };
    for class in ["Both", "Root", "Greeter", "Row", "Missing"] {
        assert_eq!(
            names(reach.every_method(class).to_vec()),
            names(reach.reference_every_method(class)),
            "{class}"
        );
    }
    assert_eq!(
        names(reach.every_method("Both").to_vec()),
        ["both::root", "subboth::root"]
    );
    assert!(reach.every_method("Missing").is_empty());
}

#[test]
fn the_overrides_of_an_interface_are_the_receivers_methods() {
    let registry = registry(GRAPH);
    for (receiver, builtin) in [
        ("Both", "Root"),
        ("SubBoth", "Root"),
        ("Left", "Root"),
        ("Row", "Root"),
        ("Both", "Missing"),
    ] {
        assert_eq!(
            names(registry.overrides_of_builtin(receiver, builtin).to_vec()),
            names(registry.reference_overrides_of_builtin(receiver, builtin)),
            "{receiver} {builtin}"
        );
    }
    assert_eq!(
        names(registry.overrides_of_builtin("Both", "Root").to_vec()),
        ["both::root", "subboth::root"]
    );
    assert_eq!(
        names(registry.overrides_of_builtin("SubBoth", "Root").to_vec()),
        ["subboth::root"]
    );
}

#[test]
fn an_aggregate_is_iterated_through_the_iterator_it_gives() {
    let registry = registry(GRAPH);
    let found = implicit(
        &registry,
        &classes(&["Row"]),
        &Operation::Iterate { keys: true },
    );
    assert!(
        found
            .iter()
            .any(|name| name.starts_with("row::getiterator")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|name| name.starts_with("rowit::current")),
        "{found:?}"
    );
    let found = implicit(
        &registry,
        &classes(&["RowIt"]),
        &Operation::Iterate { keys: false },
    );
    assert!(
        found.iter().all(|name| !name.starts_with("rowit::key")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|name| name.starts_with("rowit::valid")),
        "{found:?}"
    );
}

#[test]
fn an_operation_on_an_unknown_object_reaches_every_class_with_the_hook() {
    let registry = registry(SHAPES);
    let found = implicit(&registry, &Holder::Unknown, &Operation::String);
    assert!(
        found.iter().all(|name| name.contains("__tostring")),
        "{found:?}"
    );
    implicit(&registry, &Holder::Unknown, &Operation::Destroy);
    implicit(
        &registry,
        &Holder::Made(None),
        &Operation::Iterate { keys: true },
    );
}

#[test]
fn an_operation_on_classes_reaches_the_hooks_of_each_class_and_those_below() {
    let registry = registry(GRAPH);
    for operation in [
        Operation::String,
        Operation::Destroy,
        Operation::Clone { with: true },
        Operation::Element(Access::Read),
        Operation::Property(None, Access::Write),
    ] {
        for holder in [
            classes(&["Both"]),
            classes(&["Greeter", "Tagged"]),
            Holder::Made(Some(vec![Name::from("Both")])),
        ] {
            implicit(&registry, &holder, &operation);
        }
    }
}

#[test]
fn a_target_set_keeps_each_target_once_in_the_order_added() {
    let mut set = TargetSet::default();
    let mut expected: Vec<(DeclKey, Transfer)> = Vec::new();
    for round in 0..3 {
        for n in 0..40 {
            let key = method_key(&Name::from(format!("C{n}")), "m");
            let transfer = if n % 3 == 0 {
                Transfer::Same
            } else {
                Transfer::Other(None)
            };
            set.push(key.clone(), transfer.clone());
            if round == 0 {
                expected.push((key, transfer));
            }
        }
    }
    // A name in another case is the same declaration.
    set.push(method_key(&Name::from("c1"), "M"), Transfer::Other(None));
    assert_eq!(set.into_vec(), expected);
}

#[test]
fn an_answer_is_computed_once_and_dropped_when_the_graph_can_change() {
    let mut registry = registry(SHAPES);
    let first = registry.implementations("Shape", "area");
    assert!(Arc::ptr_eq(
        &first,
        &registry.implementations("sHAPE", "AREA")
    ));
    assert!(Arc::ptr_eq(
        &registry.subclasses_shared("Shape"),
        &registry.subclasses_shared("shape")
    ));
    registry.advance_relation_generation();
    let second = registry.implementations("Shape", "area");
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(*first, *second);
}
