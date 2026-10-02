//! The types with nothing in them, which share one node each.
use super::super::*;
use crate::intern::Constant;

#[test]
fn every_constant_is_the_node_its_form_interns_to() {
    for constant in Constant::ALL {
        let form = constant.repr();
        assert_eq!(Constant::of(&form), Some(constant), "{form:?}");
        let built = Type::new(form);
        assert!(
            Arc::ptr_eq(&built.0, &Type::constant(constant).0),
            "{constant:?}"
        );
    }
}

#[test]
fn the_constructors_return_the_shared_nodes() {
    let pairs = [
        (Type::mixed(), Constant::Mixed),
        (Type::explicit_mixed(), Constant::ExplicitMixed),
        (Type::error(), Constant::Error),
        (Type::never(), Constant::Never),
        (Type::void(), Constant::Void),
        (Type::null(), Constant::Null),
        (Type::bool(), Constant::Bool),
        (Type::bool_literal(true), Constant::True),
        (Type::bool_literal(false), Constant::False),
        (Type::int(), Constant::Int),
        (Type::float(), Constant::Float),
        (Type::string(), Constant::String),
        (Type::object_without_class(), Constant::ObjectWithoutClass),
        (Type::parent_ref(), Constant::ParentRef),
        (Type::this(), Constant::This),
    ];
    for (built, constant) in pairs {
        assert!(Arc::ptr_eq(&built.0, &Type::constant(constant).0));
    }
    assert!(Arc::ptr_eq(&Type::int_range(None, None).0, &Type::int().0));
    assert!(Arc::ptr_eq(
        &Type::int_range(Some(2), Some(1)).0,
        &Type::never().0
    ));
}

#[test]
fn a_refined_string_probe_is_string_with_its_own_accessory() {
    for kind in [
        AccessoryKind::NonEmptyString,
        AccessoryKind::NonFalsyString,
        AccessoryKind::LiteralString,
        AccessoryKind::NumericString,
        AccessoryKind::ArrayList,
        AccessoryKind::NonEmptyArray,
        AccessoryKind::LowercaseString,
        AccessoryKind::UppercaseString,
        AccessoryKind::DecimalIntegerString,
        AccessoryKind::NonDecimalIntegerString,
    ] {
        let built = Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(kind),
        ]));
        assert!(Arc::ptr_eq(
            &crate::queries::refined_string_probe(kind).0,
            &built.0
        ));
    }
}
