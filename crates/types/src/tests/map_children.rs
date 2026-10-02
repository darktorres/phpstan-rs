//! `Type::map_children` gives back the very node when no child changed.
use super::super::*;
use std::sync::Arc;

fn samples() -> Vec<Type> {
    let object = |name: &str, args: Vec<Type>| Type::object(name.into(), args);
    let template = Type::template_param(TemplateParam::unbounded("T".to_string()));
    vec![
        Type::array(Type::string(), Type::int()),
        Type::class_string(object("Foo", Vec::new())),
        object("Box", vec![Type::int(), template.clone()]),
        Type::self_ref(vec![Type::int()]),
        Type::static_ref(vec![template.clone()]),
        Type::array_shape(Shape::from_named(vec![
            ("a", Type::int(), false),
            ("b", Type::string(), true),
        ])),
        Type::object_shape(Shape::from_named(vec![("a", Type::int(), false)])),
        Type::variance_arg(CallSiteVariance::Covariant, Type::int()),
        Type::new(Repr::Intersection(vec![
            object("A", Vec::new()),
            object("B", Vec::new()),
        ])),
        Type::has_offset_value(ShapeKey::Str("k".into()), Type::int()),
        Type::reflected_member(object("Foo", Vec::new()), "bar".into()),
        Type::value_of(object("Foo", Vec::new())),
        Type::key_of(template.clone()),
        Type::offset_access(template.clone(), Type::int()),
        Type::class_constant(object("Foo", Vec::new()), "BAR_*".into()),
        Type::new(Repr::Conditional {
            subject: ConditionalSubject::Template("T".into()),
            target: Type::int(),
            if_true: Type::string(),
            if_false: Type::null(),
            negated: false,
        }),
        template,
        Type::int(),
    ]
}

#[test]
fn an_identity_mapping_gives_back_the_same_node() {
    for ty in samples() {
        let mapped = ty.map_children(&mut |t| t.clone());
        assert!(Arc::ptr_eq(&ty.0, &mapped.0), "{ty}");
    }
}

#[test]
fn a_changed_child_is_rebuilt_and_an_unchanged_one_kept() {
    let object = Type::object("Box".into(), vec![Type::int(), Type::string()]);
    // Only `string` becomes `null`.
    let mapped = object.map_children(&mut |t| {
        if t.to_string() == "string" {
            Type::null()
        } else {
            t.clone()
        }
    });
    assert_eq!(
        mapped,
        Type::object("Box".into(), vec![Type::int(), Type::null()])
    );
    assert!(!Arc::ptr_eq(&mapped.0, &object.0));
    let shape = Type::array_shape(Shape::from_named(vec![
        ("a", Type::int(), false),
        ("b", Type::string(), false),
    ]));
    let mapped = shape.map_children(&mut |t| {
        if t.to_string() == "int" {
            Type::float()
        } else {
            t.clone()
        }
    });
    assert_eq!(
        mapped,
        Type::array_shape(Shape::from_named(vec![
            ("a", Type::float(), false),
            ("b", Type::string(), false),
        ]))
    );
}

#[test]
fn the_mapping_function_is_called_on_every_child_in_order() {
    let object = Type::object(
        "Box".into(),
        vec![Type::int(), Type::string(), Type::bool()],
    );
    let mut seen = Vec::new();
    object.map_children(&mut |t| {
        seen.push(t.to_string());
        if t.to_string() == "int" {
            Type::null()
        } else {
            t.clone()
        }
    });
    assert_eq!(seen, ["int", "string", "bool"]);
}
