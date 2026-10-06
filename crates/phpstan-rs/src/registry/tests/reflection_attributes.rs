//! Attributes as reflection exposes them: names resolved, constant
//! arguments evaluated.

use super::helpers::*;
use crate::registry::{AttributeValue, ReflectionAttribute};

fn class_attributes(src: &str, class: &str) -> Vec<ReflectionAttribute> {
    build_registry(src)
        .class_by_name(class)
        .unwrap()
        .attributes
        .clone()
}

#[test]
fn names_resolve_through_imports_and_the_namespace() {
    let attributes = class_attributes(
        "<?php namespace App; use Lib\\Route; #[Route] #[Marker] #[\\Global\\Tag] class C {}",
        "App\\C",
    );
    let names: Vec<&str> = attributes.iter().map(|a| a.name()).collect();
    assert_eq!(names, vec!["Lib\\Route", "App\\Marker", "Global\\Tag"]);
    assert_eq!(attributes[0].short_name(), "Route");
}

#[test]
fn constant_arguments_are_evaluated_and_others_are_not() {
    let attributes = class_attributes(
        "<?php use Lib\\Target; #[A(1, -2.5, 'x', true, null, Target::class, Target::FOO, PHP_EOL, [1, 'k' => 2], Target::A | Target::B, $x)] class C {}",
        "C",
    );
    let values: Vec<&AttributeValue> = attributes[0].arguments().iter().map(|a| &a.value).collect();
    assert_eq!(values[0], &AttributeValue::Int(1));
    assert_eq!(values[1], &AttributeValue::Float(-2.5));
    assert_eq!(values[2], &AttributeValue::String("x".into()));
    assert_eq!(values[3], &AttributeValue::Bool(true));
    assert_eq!(values[4], &AttributeValue::Null);
    assert_eq!(values[5], &AttributeValue::ClassName("Lib\\Target".into()));
    assert_eq!(
        values[6],
        &AttributeValue::ClassConstant {
            class: "Lib\\Target".into(),
            name: "FOO".into()
        }
    );
    assert_eq!(values[7], &AttributeValue::Constant("PHP_EOL".into()));
    assert_eq!(
        values[8],
        &AttributeValue::Array(vec![
            (None, AttributeValue::Int(1)),
            (
                Some(AttributeValue::String("k".into())),
                AttributeValue::Int(2)
            ),
        ])
    );
    assert!(matches!(values[9], AttributeValue::BitOr(operands) if operands.len() == 2));
    assert_eq!(values[10], &AttributeValue::Unsupported);
}

#[test]
fn a_named_argument_is_found_by_name_and_a_positional_one_by_position() {
    let attributes = class_attributes("<?php #[A('first', flags: 3)] class C {}", "C");
    assert_eq!(
        attributes[0].argument(0, "value"),
        Some(&AttributeValue::String("first".into()))
    );
    assert_eq!(
        attributes[0].argument(1, "flags"),
        Some(&AttributeValue::Int(3))
    );
    assert_eq!(attributes[0].argument(2, "other"), None);
}

#[test]
fn members_and_parameters_carry_their_own_attributes() {
    let registry = build_registry(
        "<?php class C { #[P] public $p; #[K] const K = 1; #[M] function m(#[Q] $q) {} }",
    );
    let info = &registry.class_by_name("C").unwrap();
    assert_eq!(info.properties["p"].attributes[0].name(), "P");
    assert_eq!(info.consts["K"].attributes[0].name(), "K");
    let method = &info.methods["m"];
    assert_eq!(method.attributes[0].name(), "M");
    assert_eq!(method.params[0].attributes[0].name(), "Q");
}
