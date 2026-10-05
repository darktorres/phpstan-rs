//! `Sampler`: the values of a declared type, each one the type accepts.

use super::*;
use crate::project::Project;

fn project() -> Project {
    crate::project::test_support::Files::new()
        .analysed(
            "a.php",
            r#"<?php
interface Shape {}
final class Circle implements Shape { public function __construct(public int $r) {} }
abstract class Base {}
final class Leaf extends Base {}
enum Suit: string { case Hearts = 'h'; case Spades = 's'; }
"#,
        )
        .load()
}

/// The PHP of each value sampled for the type `written` reads as.
fn sampled(project: &Project, written: &str) -> Vec<String> {
    let ty = types::parse_type_string(written);
    let registry = project.registry();
    let samples = Sampler::new(registry).samples(&ty);
    let relation = registry.relation();
    for sample in &samples {
        assert!(
            relation.is_super_type_of(&ty, &sample.ty).yes(),
            "{written} does not accept {}",
            sample.php
        );
    }
    samples.into_iter().map(|s| s.php).collect()
}

#[test]
fn scalars_are_the_pool_s_values_the_type_accepts() {
    let project = project();
    assert_eq!(sampled(&project, "int<3, 9>"), ["3", "9", "6", "7"]);
    assert_eq!(
        sampled(&project, "non-empty-string"),
        [
            "\"0\"",
            "\"a\"",
            "\"stdClass\"",
            "\"strlen\"",
            "\"1.5\"",
            "\"1\""
        ]
    );
    assert_eq!(sampled(&project, "'x'|'y'"), ["\"x\"", "\"y\""]);
    assert_eq!(sampled(&project, "?bool"), ["true", "null", "false"]);
}

#[test]
fn arrays_are_built_from_their_key_and_value_types() {
    let project = project();
    assert_eq!(
        sampled(&project, "list<int>"),
        ["[]", "[0 => 0]", "[0 => 0, 1 => 1]"]
    );
    assert_eq!(
        sampled(&project, "non-empty-array<string, bool>"),
        ["[\"\" => true]", "[\"\" => true, \"a\" => false]"]
    );
    assert_eq!(
        sampled(&project, "array{a: int, b?: string}"),
        ["[\"a\" => 0, \"b\" => \"\"]", "[\"a\" => 0]"]
    );
}

#[test]
fn objects_are_made_through_their_constructor_or_a_concrete_descendant() {
    let project = project();
    assert_eq!(sampled(&project, "Circle"), ["new \\Circle(0)"]);
    assert_eq!(sampled(&project, "Shape"), ["new \\Circle(0)"]);
    assert_eq!(sampled(&project, "Base"), ["new \\Leaf()"]);
    assert_eq!(
        sampled(&project, "Suit"),
        ["\\Suit::Hearts", "\\Suit::Spades"]
    );
    assert_eq!(sampled(&project, "class-string<Base>"), ["\\Leaf::class"]);
}

#[test]
fn a_callable_is_a_closure_returning_a_value_of_its_return_type() {
    let project = project();
    assert_eq!(
        sampled(&project, "callable(int): string"),
        ["static function (...$__a) { return \"\"; }"]
    );
}

#[test]
fn an_enum_case_type_is_its_case_and_an_object_shape_an_object_with_its_fields() {
    let project = project();
    let case = Type::enum_case("Suit".into(), "Spades".into());
    let samples: Vec<String> = Sampler::new(project.registry())
        .samples(&case)
        .into_iter()
        .map(|s| s.php)
        .collect();
    assert_eq!(samples, ["\\Suit::Spades"]);
    assert_eq!(
        sampled_unchecked(&project, "object{name: string, age?: int}"),
        [
            "(object) [\"name\" => \"\", \"age\" => 0]",
            "(object) [\"name\" => \"\"]"
        ]
    );
}

/// `sampled` for a type whose values are of it by construction, which the
/// relation does not tell of an object with properties.
fn sampled_unchecked(project: &Project, written: &str) -> Vec<String> {
    let ty = types::parse_type_string(written);
    Sampler::new(project.registry())
        .samples(&ty)
        .into_iter()
        .map(|s| s.php)
        .collect()
}
