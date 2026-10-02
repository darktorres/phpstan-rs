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
        target: t(),
        if_true: t(),
        if_false: Type::null(),
        negated: false,
    });
    assert_eq!(
        conditional.substitute_templates(&bindings, &crate::Relation::structural()),
        Type::new(Repr::Conditional {
            subject: ConditionalSubject::Parameter("x".into()),
            target: Type::int(),
            if_true: Type::int(),
            if_false: Type::null(),
            negated: false,
        })
    );
}

#[test]
fn renaming_class_names_leaves_a_template_placeholders_bound_as_declared() {
    let placeholder = Type::template_param(TemplateParam {
        bound: Box::new(Type::object("Lib\\Repo".into(), Vec::new())),
        ..TemplateParam::unbounded("R".to_string())
    });
    let written = Type::object("Box".into(), vec![placeholder.clone()]);
    let renamed = written.map_class_names(&mut |name| format!("App\\{name}").as_str().into());
    assert_eq!(renamed, Type::object("App\\Box".into(), vec![placeholder]));
}

#[test]
fn leaving_a_type_without_templates_alone_is_what_walking_it_gives() {
    use crate::ConditionalSubject;
    use crate::templates::substitute_with;
    let t = || Type::template_param(TemplateParam::unbounded("T".to_string()));
    let u = || Type::template_param(TemplateParam::unbounded("U".to_string()));
    let list_of_int = Type::array(Type::unknown_array_key(), Type::int());
    let shape = |ty: Type| Type::array_shape(Shape::from_named([("a", ty, false)]));
    let types = [
        Type::int(),
        Type::string(),
        Type::union_of(vec![Type::int(), Type::null()]),
        list_of_int.clone(),
        shape(Type::int()),
        Type::object("Box".into(), vec![Type::int()]),
        Type::object("Box".into(), vec![t()]),
        t(),
        Type::union_of(vec![t(), Type::null()]),
        Type::array(Type::unknown_array_key(), t()),
        shape(t()),
        shape(shape(t())),
        Type::object("Box".into(), vec![Type::object("Box".into(), vec![u()])]),
        Type::class_string(t()),
        Type::value_of(list_of_int.clone()),
        Type::key_of(list_of_int.clone()),
        Type::key_of(Type::array(Type::unknown_array_key(), t())),
        Type::offset_access(shape(Type::int()), Type::string_literal("a".into())),
        Type::offset_access(shape(t()), Type::string_literal("a".into())),
        Type::union_of(vec![Type::int(), Type::key_of(list_of_int.clone())]),
        Type::object("Box".into(), vec![Type::value_of(list_of_int)]),
        Type::conditional(
            ConditionalSubject::Template("T".to_string().into()),
            Type::int(),
            Type::string(),
            Type::null(),
            false,
        ),
        Type::object(
            "Box".into(),
            vec![Type::conditional(
                ConditionalSubject::Template("U".to_string().into()),
                Type::int(),
                t(),
                Type::int(),
                true,
            )],
        ),
        Type::callable(None),
    ];
    let mut bound: FxHashMap<String, Type> = FxHashMap::default();
    bound.insert("T".to_string(), Type::int());
    let sets = [
        FxHashMap::default(),
        bound,
        FxHashMap::from_iter([
            ("T".to_string(), Type::string()),
            ("U".to_string(), Type::bool()),
        ]),
    ];
    let relation = crate::Relation::structural();
    for ty in &types {
        for bindings in &sets {
            for to_bound in [false, true] {
                let pruned =
                    substitute_with(ty, bindings, &|_| true, &|_| to_bound, &relation, true);
                let walked =
                    substitute_with(ty, bindings, &|_| true, &|_| to_bound, &relation, false);
                assert_eq!(
                    pruned, walked,
                    "{ty} with {bindings:?} (to bounds: {to_bound})"
                );
            }
        }
    }
}

#[test]
fn a_parameter_without_templates_binds_nothing() {
    let relation = crate::Relation::structural();
    let params = [
        Type::int(),
        Type::array(Type::unknown_array_key(), Type::int()),
        Type::object("Box".into(), vec![Type::int()]),
        Type::union_of(vec![Type::int(), Type::null()]),
    ];
    let args = [
        Type::int(),
        Type::array(Type::unknown_array_key(), Type::string()),
        Type::object("Box".into(), vec![Type::string()]),
        Type::union_of(vec![Type::int(), Type::string()]),
    ];
    for param in &params {
        for arg in &args {
            let mut bindings = FxHashMap::default();
            relation.infer_template_bindings(param, arg, &mut bindings);
            assert!(bindings.is_empty(), "{param} against {arg}");
        }
    }
}
