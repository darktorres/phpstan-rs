//! The hierarchy queries effect resolution runs, on small class hierarchies:
//! what a call may reach through overrides, `__call`, `@method` tags and a
//! builtin's methods. Results are compared as sorted `class::member` names.

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
        names(registry.implementations("Base", "name")),
        ["base::name", "tiny::name"]
    );
    assert_eq!(
        names(registry.implementations("Base", "area")),
        ["base::area", "circle::area"]
    );
}

#[test]
fn a_method_nothing_overrides_reaches_only_itself() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.implementations("Circle", "area")),
        ["circle::area"]
    );
    assert_eq!(
        names(registry.implementations("Tiny", "name")),
        ["tiny::name"]
    );
}

#[test]
fn a_missing_method_reaches_the_call_method_of_each_class() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.implementations("Magic", "missing")),
        ["magic::__call"]
    );
    assert!(registry.implementations("Square", "missing").is_empty());
}

#[test]
fn a_name_on_an_unknown_object_reaches_every_declaration_and_every_call_method() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.methods_named("area").unwrap()),
        ["base::area", "circle::area", "magic::__call", "shape::area"]
    );
    assert_eq!(
        names(registry.methods_named("NAME").unwrap()),
        ["base::name", "magic::__call", "tiny::name"]
    );
}

#[test]
fn the_overrides_of_a_builtin_are_taken_as_the_receiver_resolves_them() {
    let registry = registry(SHAPES);
    assert_eq!(
        names(registry.overrides_of_builtin("Base", "Shape")),
        ["base::area", "circle::area"]
    );
    assert_eq!(
        names(registry.overrides_of_builtin("Circle", "Shape")),
        ["circle::area"]
    );
    assert!(registry.overrides_of_builtin("Base", "Missing").is_empty());
}
