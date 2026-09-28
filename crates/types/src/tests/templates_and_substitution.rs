//! Template type substitution.
use super::super::*;
use crate::Repr;
use rustc_hash::FxHashMap;

#[test]
fn substitute_template_recurses_into_array_and_intersection_types() {
    // Covers substitute_template's Array and Intersection arms - not
    // exercised by substituting a bare TemplateParam or one nested in a
    // Union, only `T[]`/`array<T>` or an intersection type mentioning T.
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type(Repr::String));

    let array_of_t = Type(Repr::Array(
        Box::new(Type::unknown_array_key()),
        Box::new(Type(Repr::TemplateParam(TemplateParam::unbounded(
            "T".to_string(),
        )))),
    ));
    assert_eq!(
        substitute_template(&array_of_t, &bindings),
        Type(Repr::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type(Repr::String))
        ))
    );

    let intersection_with_t = Type(Repr::Intersection(vec![
        Type(Repr::TemplateParam(TemplateParam::unbounded(
            "T".to_string(),
        ))),
        Type(Repr::Object("Countable".into(), Vec::new())),
    ]));
    assert_eq!(
        substitute_template(&intersection_with_t, &bindings),
        Type(Repr::Intersection(vec![
            Type(Repr::String),
            Type(Repr::Object("Countable".into(), Vec::new()))
        ]))
    );
}

#[test]
fn substitute_template_recurses_into_a_generic_class_reference() {
    // `substitute_template` must substitute through `Type::Object(name,
    // args)`: `T -> int` through `Box<T>` should produce `Box<int>`,
    // not leave it unchanged.
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type(Repr::Int));

    let box_of_t = Type(Repr::Object(
        "Box".into(),
        vec![Type(Repr::TemplateParam(TemplateParam::unbounded(
            "T".to_string(),
        )))],
    ));
    assert_eq!(
        substitute_template(&box_of_t, &bindings),
        Type(Repr::Object("Box".into(), vec![Type(Repr::Int)]))
    );
}

// A template param used inside one of a conditional return type's own
// branches (`@return ($x is T ? T : null)`) needs the same substitution
// as anywhere else - `subject` (a raw parameter name, not a `Type`) is
// left untouched.
#[test]
fn substitute_template_recurses_into_a_conditional_types_branches() {
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type(Repr::Int));

    let t = || {
        Type(Repr::TemplateParam(TemplateParam::unbounded(
            "T".to_string(),
        )))
    };
    let conditional = Type(Repr::Conditional {
        subject: "x".to_string(),
        target: Box::new(t()),
        if_true: Box::new(t()),
        if_false: Box::new(Type(Repr::Null)),
        negated: false,
        template_subject: false,
    });
    assert_eq!(
        substitute_template(&conditional, &bindings),
        Type(Repr::Conditional {
            subject: "x".to_string(),
            target: Box::new(Type(Repr::Int)),
            if_true: Box::new(Type(Repr::Int)),
            if_false: Box::new(Type(Repr::Null)),
            negated: false,
            template_subject: false,
        })
    );
}
