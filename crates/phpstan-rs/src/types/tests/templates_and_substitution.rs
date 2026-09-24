//! Template type substitution.
use super::super::*;
use rustc_hash::FxHashMap;

#[test]
fn substitute_template_recurses_into_array_and_intersection_types() {
    // Covers substitute_template's Array and Intersection arms - not
    // exercised by substituting a bare TemplateParam or one nested in a
    // Union, only `T[]`/`array<T>` or an intersection type mentioning T.
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type::String);

    let array_of_t = Type::Array(
        Box::new(Type::unknown_array_key()),
        Box::new(Type::TemplateParam(TemplateParam::unbounded(
            "T".to_string(),
        ))),
    );
    assert_eq!(
        substitute_template(&array_of_t, &bindings),
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::String))
    );

    let intersection_with_t = Type::Intersection(vec![
        Type::TemplateParam(TemplateParam::unbounded("T".to_string())),
        Type::Object("Countable".to_string(), Vec::new()),
    ]);
    assert_eq!(
        substitute_template(&intersection_with_t, &bindings),
        Type::Intersection(vec![
            Type::String,
            Type::Object("Countable".to_string(), Vec::new())
        ])
    );
}

#[test]
fn substitute_template_recurses_into_a_generic_class_reference() {
    // `substitute_template` must substitute through `Type::Object(name,
    // args)`: `T -> int` through `Box<T>` should produce `Box<int>`,
    // not leave it unchanged.
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type::Int);

    let box_of_t = Type::Object(
        "Box".to_string(),
        vec![Type::TemplateParam(TemplateParam::unbounded(
            "T".to_string(),
        ))],
    );
    assert_eq!(
        substitute_template(&box_of_t, &bindings),
        Type::Object("Box".to_string(), vec![Type::Int])
    );
}

// A template param used inside one of a conditional return type's own
// branches (`@return ($x is T ? T : null)`) needs the same substitution
// as anywhere else - `subject` (a raw parameter name, not a `Type`) is
// left untouched.
#[test]
fn substitute_template_recurses_into_a_conditional_types_branches() {
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type::Int);

    let t = || Type::TemplateParam(TemplateParam::unbounded("T".to_string()));
    let conditional = Type::Conditional {
        subject: "x".to_string(),
        target: Box::new(t()),
        if_true: Box::new(t()),
        if_false: Box::new(Type::Null),
        negated: false,
        template_subject: false,
    };
    assert_eq!(
        substitute_template(&conditional, &bindings),
        Type::Conditional {
            subject: "x".to_string(),
            target: Box::new(Type::Int),
            if_true: Box::new(Type::Int),
            if_false: Box::new(Type::Null),
            negated: false,
            template_subject: false,
        }
    );
}
