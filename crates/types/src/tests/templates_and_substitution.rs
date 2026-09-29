//! Template type substitution.
use super::super::*;
use crate::Repr;
use rustc_hash::FxHashMap;

#[test]
fn substitute_template_recurses_into_array_and_intersection_types() {
    // Covers Type::substitute_templates's Array and Intersection arms - not
    // exercised by substituting a bare TemplateParam or one nested in a
    // Union, only `T[]`/`array<T>` or an intersection type mentioning T.
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type::string());

    let array_of_t = Type::array(
        Type::unknown_array_key(),
        Type::new(Repr::TemplateParam(TemplateParam::unbounded(
            "T".to_string(),
        ))),
    );
    assert_eq!(
        array_of_t.substitute_templates(&bindings, &crate::Relation::structural()),
        Type::array(Type::unknown_array_key(), Type::new(Repr::String))
    );

    let intersection_with_t = Type::new(Repr::Intersection(vec![
        Type::template_param(TemplateParam::unbounded("T".to_string())),
        Type::object("Countable".into(), Vec::new()),
    ]));
    assert_eq!(
        intersection_with_t.substitute_templates(&bindings, &crate::Relation::structural()),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::object("Countable".into(), Vec::new())
        ]))
    );
}

#[test]
fn substitute_template_recurses_into_a_generic_class_reference() {
    // `Type::substitute_templates` must substitute through `Repr::Object(name,
    // args)`: `T -> int` through `Box<T>` should produce `Box<int>`,
    // not leave it unchanged.
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type::int());

    let box_of_t = Type::object(
        "Box".into(),
        vec![Type::new(Repr::TemplateParam(TemplateParam::unbounded(
            "T".to_string(),
        )))],
    );
    assert_eq!(
        box_of_t.substitute_templates(&bindings, &crate::Relation::structural()),
        Type::object("Box".into(), vec![Type::new(Repr::Int)])
    );
}

// A template param used inside one of a conditional return type's own
// branches (`@return ($x is T ? T : null)`) needs the same substitution
// as anywhere else - `subject` (a parameter name, not a `Type`) is left
// untouched.
#[test]
fn substitute_template_recurses_into_a_conditional_types_branches() {
    let mut bindings = FxHashMap::default();
    bindings.insert("T".to_string(), Type::int());

    let t = || Type::template_param(TemplateParam::unbounded("T".to_string()));
    let conditional = Type::new(Repr::Conditional {
        subject: ConditionalSubject::Parameter("x".into()),
        target: std::sync::Arc::new(t()),
        if_true: std::sync::Arc::new(t()),
        if_false: std::sync::Arc::new(Type::null()),
        negated: false,
    });
    assert_eq!(
        conditional.substitute_templates(&bindings, &crate::Relation::structural()),
        Type::new(Repr::Conditional {
            subject: ConditionalSubject::Parameter("x".into()),
            target: std::sync::Arc::new(Type::int()),
            if_true: std::sync::Arc::new(Type::int()),
            if_false: std::sync::Arc::new(Type::null()),
            negated: false,
        })
    );
}
