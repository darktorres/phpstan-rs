//! The early exits on a node's flags and the answers a node keeps give
//! what the computations they stand for give.
use super::super::*;
use crate::{Repr, without_shortcuts};
use proptest::prelude::*;

pub(super) fn leaf() -> impl Strategy<Value = Type> {
    prop_oneof![
        Just(Type::mixed()),
        Just(Type::never()),
        Just(Type::null()),
        Just(Type::bool()),
        Just(Type::bool_literal(true)),
        Just(Type::bool_literal(false)),
        Just(Type::int()),
        Just(Type::float()),
        Just(Type::string()),
        Just(Type::object_without_class()),
        any::<i64>().prop_map(Type::int_literal),
        "[a-z]{0,4}".prop_map(|s| Type::string_literal(s.into())),
        "[A-C][a-z]{0,3}".prop_map(|n| Type::object(n.into(), Vec::new())),
        Just(Type::template_param(TemplateParam::unbounded(
            "T".to_string()
        ))),
    ]
}

pub(super) fn any_type() -> impl Strategy<Value = Type> {
    leaf().prop_recursive(4, 40, 4, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(k, v)| Type::array(k, v)),
            inner.clone().prop_map(Type::class_string),
            inner.clone().prop_map(Type::list_of),
            prop::collection::vec(inner.clone(), 2..4).prop_map(Type::union_of),
            prop::collection::vec(inner.clone(), 2..3)
                .prop_map(|members| Type::new(Repr::Intersection(members))),
            (
                "[A-C][a-z]{0,3}",
                prop::collection::vec(inner.clone(), 1..3)
            )
                .prop_map(|(n, args)| Type::object(n.into(), args)),
            (
                prop_oneof![
                    Just(ConditionalSubject::Parameter("a".into())),
                    Just(ConditionalSubject::Parameter("b".into())),
                    Just(ConditionalSubject::Template("T".into())),
                ],
                inner.clone(),
                inner.clone(),
                inner.clone(),
                any::<bool>(),
            )
                .prop_map(|(subject, target, if_true, if_false, negated)| {
                    Type::conditional(subject, target, if_true, if_false, negated)
                }),
            (inner.clone(), inner.clone(), prop::bool::ANY).prop_map(|(param, ret, templated)| {
                let template = TemplateParam {
                    scope: Some(CALLABLE_TEMPLATE_SCOPE.to_string()),
                    ..TemplateParam::unbounded("U".to_string())
                };
                Type::callable(Some(Arc::new(CallableSig {
                    params: vec![CallableParam {
                        ty: param,
                        variadic: false,
                        optional: false,
                        by_ref: false,
                    }],
                    return_type: Box::new(ret),
                    templates: if templated {
                        vec![template]
                    } else {
                        Vec::new()
                    },
                })))
            }),
        ]
    })
}

fn predicates(ty: &Type) -> Vec<TrinaryLogic> {
    let mut answers = vec![
        ty.is_null(),
        ty.is_true(),
        ty.is_false(),
        ty.is_bool(),
        ty.is_int(),
        ty.is_float(),
        ty.is_string(),
        ty.is_array(),
        ty.is_object(),
        ty.is_scalar(),
        ty.is_list(),
    ];
    for kind in [
        AccessoryKind::NonEmptyString,
        AccessoryKind::NonFalsyString,
        AccessoryKind::LiteralString,
        AccessoryKind::NumericString,
        AccessoryKind::LowercaseString,
        AccessoryKind::UppercaseString,
        AccessoryKind::DecimalIntegerString,
    ] {
        answers.push(ty.is_refined_string(kind));
    }
    answers
}

fn rewrites(ty: &Type) -> (Type, Type, Type) {
    let (int, string) = (Type::int(), Type::string());
    let argument = |name: &str| match name {
        "a" => Some(&int),
        "b" => Some(&string),
        _ => None,
    };
    let resolved = ty.resolve_conditional(&argument, &Relation::structural());
    let renamed = ty.map_class_names(&mut |name| format!("N\\{name}").as_str().into());
    let replaced = ty.replace_objects(&mut |name| (name.as_ref() == "Ab").then(Type::int));
    (resolved, renamed, replaced)
}

proptest! {
    #[test]
    fn a_type_answers_alike_with_the_shortcuts_off(ty in any_type()) {
        let on = (predicates(&ty), rewrites(&ty));
        // A second ask reads what the first one kept.
        prop_assert_eq!(&on, &(predicates(&ty), rewrites(&ty)));
        let off = without_shortcuts(|| (predicates(&ty), rewrites(&ty)));
        prop_assert_eq!(on, off);
    }
}

fn contains(ty: &Type, pred: &dyn Fn(&Type) -> bool) -> bool {
    let mut found = pred(ty);
    ty.for_each_child(&mut |child| found |= contains(child, pred));
    found
}

proptest! {
    #[test]
    fn the_flags_are_the_recursive_definitions(ty in any_type()) {
        let conditional = contains(&ty, &|t| matches!(t.repr(), Repr::Conditional { .. }));
        let object = contains(&ty, &|t| {
            matches!(t.repr(), Repr::Object(..))
                || matches!(t.repr(), Repr::Callable(Some(sig)) if !sig.templates.is_empty())
        });
        let param = contains(&ty, &|t| {
            matches!(
                t.repr(),
                Repr::Conditional { subject: ConditionalSubject::Parameter(_), .. }
            )
        });
        prop_assert_eq!(ty.0.flags & crate::HAS_CONDITIONAL != 0, conditional);
        prop_assert_eq!(ty.0.flags & crate::HAS_OBJECT != 0, object);
        prop_assert_eq!(ty.0.flags & crate::HAS_PARAM_CONDITIONAL != 0, param);
    }
}

#[test]
fn a_type_without_a_conditional_or_an_object_is_returned_as_it_is() {
    let ty = Type::array(
        Type::int(),
        Type::union_of(vec![Type::string(), Type::null()]),
    );
    let bindings = |_: &str| None;
    let relation = Relation::structural();
    assert_eq!(ty.0.flags & (crate::HAS_CONDITIONAL | crate::HAS_OBJECT), 0);
    assert!(Arc::ptr_eq(
        &ty.resolve_conditional(&bindings, &relation).0,
        &ty.0
    ));
    assert!(Arc::ptr_eq(
        &ty.map_class_names(&mut |n| n.clone()).0,
        &ty.0
    ));
    assert!(Arc::ptr_eq(&ty.replace_objects(&mut |_| None).0, &ty.0));
}

#[test]
fn a_node_holds_no_more_than_its_fields_and_the_padding_of_one_word() {
    // `Repr`, the two hashes, the flags and the 64 bits of answers.
    let repr = std::mem::size_of::<Repr>();
    assert!(std::mem::size_of::<crate::Node>() <= repr + 8 + 8 + 8 + 8);
}
