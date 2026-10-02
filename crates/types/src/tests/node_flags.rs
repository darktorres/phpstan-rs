//! The flags a node carries are the recursive definitions they stand for.
use super::super::*;
use crate::Repr;

fn nodes_below(ty: &Type, out: &mut Vec<Type>) {
    out.push(ty.clone());
    ty.for_each_child(&mut |child| nodes_below(child, out));
}

fn refined_key() -> Type {
    Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::NonDecimalIntegerString),
    ]))
}

fn template() -> Type {
    Type::template_param(TemplateParam::unbounded("T".to_string()))
}

fn conditional_on_template() -> Type {
    Type::new(Repr::Conditional {
        subject: ConditionalSubject::Template("T".into()),
        target: Type::int(),
        if_true: Type::string(),
        if_false: Type::null(),
        negated: false,
    })
}

fn samples() -> Vec<Type> {
    let object = Type::object("Foo".into(), Vec::new());
    let late = Type::new(Repr::KeyOf(object.clone()));
    vec![
        object.clone(),
        template(),
        conditional_on_template(),
        Type::array(Type::unknown_array_key(), template()),
        Type::union_of(vec![Type::int(), conditional_on_template()]),
        late.clone(),
        Type::new(Repr::OffsetAccess(object.clone(), Type::int())),
        Type::new(Repr::ValueOf(object.clone())),
        Type::array(Type::string(), late),
        Type::parent_ref(),
        Type::object("Box".into(), vec![Type::parent_ref()]),
        refined_key(),
        Type::union_of(vec![Type::int(), refined_key()]),
        Type::benevolent_union_of(vec![Type::int(), refined_key()]),
        Type::object("Box".into(), vec![refined_key()]),
        Type::array(
            Type::int(),
            Type::union_of(vec![Type::null(), refined_key()]),
        ),
        Type::new(Repr::Intersection(vec![object, refined_key()])),
    ]
}

fn has_key_refinement_by_form(key: &Type) -> bool {
    match key.repr() {
        Repr::Intersection(members) => members.iter().any(|m| {
            matches!(
                m.repr(),
                Repr::Accessory(AccessoryKind::NonDecimalIntegerString)
            )
        }),
        Repr::Union(members) | Repr::BenevolentUnion(members) => {
            members.iter().any(has_key_refinement_by_form)
        }
        _ => false,
    }
}

#[test]
fn the_template_flag_is_the_recursive_definition() {
    fn recursive(ty: &Type) -> bool {
        match ty.repr() {
            Repr::TemplateParam(_) => true,
            Repr::Conditional {
                subject: ConditionalSubject::Template(_),
                ..
            } => true,
            _ => ty.any_child(recursive),
        }
    }
    for sample in samples() {
        let mut all = Vec::new();
        nodes_below(&sample, &mut all);
        for ty in all {
            assert_eq!(ty.references_template(), recursive(&ty), "{ty}");
        }
    }
}

#[test]
fn the_late_flag_is_the_recursive_definition() {
    fn recursive(ty: &Type) -> bool {
        matches!(ty.repr(), Repr::KeyOf(_) | Repr::OffsetAccess(..)) || ty.any_child(recursive)
    }
    for sample in samples() {
        let mut all = Vec::new();
        nodes_below(&sample, &mut all);
        for ty in all {
            assert_eq!(ty.contains_late(), recursive(&ty), "{ty}");
        }
    }
}

#[test]
fn the_parent_flag_is_the_recursive_definition() {
    fn recursive(ty: &Type) -> bool {
        matches!(ty.repr(), Repr::ParentRef) || ty.any_child(recursive)
    }
    for sample in samples() {
        let mut all = Vec::new();
        nodes_below(&sample, &mut all);
        for ty in all {
            assert_eq!(ty.has_parent_ref(), recursive(&ty), "{ty}");
        }
    }
}

#[test]
fn the_key_refinement_flag_is_the_form_test_and_is_not_passed_up() {
    for sample in samples() {
        let mut all = Vec::new();
        nodes_below(&sample, &mut all);
        for ty in all {
            assert_eq!(
                crate::has_key_refinement(&ty),
                has_key_refinement_by_form(&ty),
                "{ty}"
            );
        }
    }
    let boxed = Type::object("Box".into(), vec![refined_key()]);
    assert!(crate::has_key_refinement(&refined_key()));
    assert!(!crate::has_key_refinement(&boxed));
}
