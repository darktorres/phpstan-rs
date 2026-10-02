//! `bind_self`, `bind_static` and `bind_parent`, and the flags that let a
//! type without those references be returned as it is.
use super::super::*;
use crate::Repr;

fn same_node(a: &Type, b: &Type) -> bool {
    std::ptr::eq(&*a.0, &*b.0)
}

fn nested(reference: Type) -> Type {
    // A reference at several depths: an array value, a union member, an
    // object's argument, and a callable's parameter and return.
    Type::union_of(vec![
        Type::array(Type::unknown_array_key(), reference.clone()),
        Type::object("Box".into(), vec![reference.clone()]),
        Type::callable(Some(std::sync::Arc::new(CallableSig {
            params: vec![CallableParam {
                ty: reference.clone(),
                variadic: false,
                optional: false,
                by_ref: false,
            }],
            return_type: Box::new(reference),
            templates: Vec::new(),
        }))),
    ])
}

#[test]
fn a_type_with_none_of_the_references_is_returned_as_it_is() {
    let plain = nested(Type::object("Foo".into(), Vec::new()));
    assert!(same_node(&plain.bind_self("Bar"), &plain));
    assert!(same_node(&plain.bind_static("Bar"), &plain));
    assert!(same_node(&plain.bind_parent("Bar"), &plain));
}

#[test]
fn each_reference_is_bound_at_every_depth_and_only_its_own_kind() {
    let bar = || Type::object("Bar".into(), Vec::new());
    let with_self = nested(Type::self_ref(Vec::new()));
    assert_eq!(with_self.bind_self("Bar"), nested(bar()));
    assert!(same_node(&with_self.bind_static("Bar"), &with_self));
    assert!(same_node(&with_self.bind_parent("Bar"), &with_self));

    let with_static = nested(Type::static_ref(Vec::new()));
    assert_eq!(with_static.bind_static("Bar"), nested(bar()));
    assert!(same_node(&with_static.bind_self("Bar"), &with_static));

    let with_this = nested(Type::this());
    assert_eq!(with_this.bind_static("Bar"), nested(bar()));
    assert!(same_node(&with_this.bind_self("Bar"), &with_this));

    let with_parent = nested(Type::parent_ref());
    assert_eq!(with_parent.bind_parent("Bar"), nested(bar()));
    assert!(same_node(&with_parent.bind_self("Bar"), &with_parent));
}

#[test]
fn a_reference_in_a_template_bound_or_a_shape_is_found() {
    let bounded = Type::new(Repr::TemplateParam(TemplateParam {
        bound: Box::new(Type::self_ref(Vec::new())),
        ..TemplateParam::unbounded("T".to_string())
    }));
    let bound_of = |ty: &Type| match ty.repr() {
        Repr::TemplateParam(tp) => (*tp.bound).clone(),
        other => panic!("expected a template, got {other:?}"),
    };
    assert_eq!(
        bound_of(&bounded.bind_self("Bar")),
        Type::object("Bar".into(), Vec::new())
    );
    assert!(same_node(&bounded.bind_static("Bar"), &bounded));
}

#[test]
fn the_deferred_flag_is_the_recursive_definition() {
    fn recursive(ty: &Type) -> bool {
        matches!(
            ty.repr(),
            Repr::ValueOf(_) | Repr::KeyOf(_) | Repr::OffsetAccess(..) | Repr::ClassConstant { .. }
        ) || ty.any_child(recursive)
    }
    let key_of = Type::new(Repr::KeyOf(Type::object("Foo".into(), Vec::new())));
    let plain = Type::object("Foo".into(), Vec::new());
    for ty in [
        plain.clone(),
        key_of.clone(),
        nested(plain.clone()),
        nested(key_of.clone()),
        Type::array(Type::unknown_array_key(), key_of.clone()),
        Type::union_of(vec![Type::int(), Type::new(Repr::ValueOf(plain.clone()))]),
    ] {
        assert_eq!(ty.contains_deferred(), recursive(&ty), "{ty}");
    }
    assert!(key_of.contains_deferred() && !plain.contains_deferred());
}
