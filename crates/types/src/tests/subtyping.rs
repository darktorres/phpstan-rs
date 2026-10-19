//! Subtype relation checks between `Type` values.
use super::super::*;
use super::{accepts, is_subtype};
use crate::Repr;

#[test]
fn class_string_is_interchangeable_with_plain_string_both_ways() {
    let cs = Type::class_string(Type::new(Repr::Object("Foo".into(), Vec::new())));
    assert!(is_subtype(&cs, &Type::string()));
    // A plain string may name a class: accepted, never provably one.
    assert!(!is_subtype(&Type::string(), &cs));
    assert!(accepts(&cs, &Type::string()));
    assert!(accepts(&cs, &Type::string_literal("Foo".into())));
    assert!(is_subtype(&cs, &cs));
    let cs_other = Type::class_string(Type::new(Repr::Object("Bar".into(), Vec::new())));
    assert!(!is_subtype(&cs, &cs_other));
}

#[test]
fn array_shape_is_subtype_of_bare_array_but_not_the_reverse() {
    let shape = parse_type_string("array{name: string}");
    assert!(is_subtype(
        &shape,
        &Type::array(
            Type::unknown_array_key(),
            Type::new(Repr::Mixed(MixedKind::Implicit, None))
        )
    ));
    assert!(!is_subtype(
        &Type::array(
            Type::unknown_array_key(),
            Type::new(Repr::Mixed(MixedKind::Implicit, None))
        ),
        &shape
    ));
}

// On top of the "only when the array's own value type is Mixed" rule (no
// per-field value comparison happens here at all - that's
// `registry::accepts`'s own, separate job), a shape's aggregate key
// type must also be a subtype of a precisely-keyed `Repr::Array`'s
// declared key type.

#[test]
fn a_string_keyed_shape_is_a_subtype_of_a_matching_string_keyed_mixed_valued_array() {
    let shape = parse_type_string("array{name: string}");
    assert!(is_subtype(
        &shape,
        &Type::array(
            Type::new(Repr::String),
            Type::new(Repr::Mixed(MixedKind::Implicit, None))
        )
    ));
}

#[test]
fn a_string_keyed_shape_is_not_a_subtype_of_an_int_keyed_mixed_valued_array() {
    let shape = parse_type_string("array{name: string}");
    assert!(!is_subtype(
        &shape,
        &Type::array(
            Type::new(Repr::Int),
            Type::new(Repr::Mixed(MixedKind::Implicit, None))
        )
    ));
}

#[test]
fn literal_is_subtype_of_its_general_type() {
    assert!(is_subtype(&Type::int_literal(5), &Type::int()));
    assert!(!is_subtype(&Type::int(), &Type::int_literal(5)));
}

#[test]
fn everything_is_subtype_of_mixed() {
    assert!(is_subtype(&Type::string(), &Type::mixed()));
    assert!(is_subtype(
        &Type::object("Foo".into(), Vec::new()),
        &Type::mixed()
    ));
}

#[test]
fn an_unbounded_template_param_takes_anything_but_is_as_unknown_as_mixed_when_given() {
    // An unbounded `@template T` slot takes any value. A `T` given is
    // unknown: plain `accepts` refuses it where a specific type is
    // declared, and the rule-level check lets it through below level 9,
    // as it does an explicit `mixed`.
    let t = Type::template_param(TemplateParam::unbounded("T".to_string()));
    assert!(accepts(&t, &Type::string()));
    assert!(accepts(&t, &Type::object("Foo".into(), Vec::new())));
    assert!(!accepts(&Type::string(), &t));
    assert!(!accepts(&Type::int(), &t));
    assert!(
        crate::Relation::structural()
            .rule_level_accepts(&Type::string(), &t)
            .yes()
    );
    assert!(!is_subtype(&t, &Type::string()));
    assert!(!is_subtype(&Type::string(), &t));
}

#[test]
fn a_bounded_template_param_is_a_subtype_of_whatever_its_bound_satisfies() {
    // A `T of int` is guaranteed to be an `int` (or narrower) however
    // it's eventually instantiated, so it's a subtype of anything `int`
    // itself is a subtype of; a `float` slot also accepts it, via real
    // PHP's own int-to-float widening rule.
    let t = Type::template_param(TemplateParam {
        name: "T".to_string(),
        bound: Box::new(Type::new(Repr::Int)),
        variance: Variance::Invariant,
        scope: None,
        default: None,
    });
    assert!(is_subtype(&t, &Type::int()));
    assert!(!is_subtype(&t, &Type::float()));
    assert!(accepts(&Type::float(), &t));
    assert!(!is_subtype(&t, &Type::string()));
}

// A `@template TKey of array-key` (bound `int|string`, a Union) must be
// compared via the dedicated `TemplateParam` arm, not the generic `(_,
// Union(members)) => any(subtype(sub, m))` arm - the latter splits
// the target union member-by-member (`subtype(TKey, Int)` and
// `subtype(TKey, String)`, each correctly false on its own) instead
// of comparing `TKey`'s union bound against the target as a whole.
#[test]
fn a_template_param_whose_own_bound_is_a_union_is_a_subtype_of_that_same_union() {
    let t = Type::template_param(TemplateParam {
        name: "TKey".to_string(),
        bound: Box::new(Type::new(Repr::Union(vec![
            Type::new(Repr::Int),
            Type::new(Repr::String),
        ]))),
        variance: Variance::Invariant,
        scope: None,
        default: None,
    });
    assert!(is_subtype(
        &t,
        &Type::new(Repr::Union(vec![Type::int(), Type::string()]))
    ));
    assert!(!is_subtype(&t, &Type::int()));
    assert!(!is_subtype(&t, &Type::string()));
}

#[test]
fn a_bounded_template_param_still_accepts_anything_as_a_sink() {
    // The reverse direction (T used as `sup`, the assignment target)
    // is accepted regardless of any declared bound, but never provable:
    // knowing a value satisfies T's *upper* bound doesn't prove it
    // satisfies whatever T is actually instantiated to.
    let t = Type::template_param(TemplateParam {
        name: "T".to_string(),
        bound: Box::new(Type::new(Repr::Int)),
        variance: Variance::Invariant,
        scope: None,
        default: None,
    });
    assert!(accepts(&t, &Type::string()));
    assert!(!is_subtype(&Type::string(), &t));
}

#[test]
fn never_is_subtype_of_everything() {
    assert!(is_subtype(&Type::never(), &Type::string()));
    assert!(is_subtype(&Type::never(), &Type::mixed()));
}

#[test]
fn union_subtype_rules() {
    let small = Type::new(Repr::Union(vec![Type::int(), Type::string()]));
    let big = Type::new(Repr::Union(vec![Type::int(), Type::string(), Type::bool()]));
    assert!(is_subtype(&small, &big));
    assert!(!is_subtype(&big, &small));
    assert!(is_subtype(&Type::string(), &small));
}

#[test]
fn intersection_subtype_rules() {
    // Coverage measurement found both Intersection arms of subtype
    // had zero executions (registry::accepts's own, separate
    // Intersection handling was covered independently).
    let both = Type::new(Repr::Intersection(vec![Type::int(), Type::string()]));
    // sub is the intersection: satisfies any one of its members.
    assert!(is_subtype(&both, &Type::int()));
    assert!(is_subtype(&both, &Type::string()));
    assert!(!is_subtype(&both, &Type::bool()));
    // sup is the intersection: must satisfy every member at once - a
    // plain Int alone doesn't also satisfy "and a String".
    assert!(!is_subtype(&Type::int(), &both));
}

#[test]
fn true_and_false_literals_are_subtypes_of_bool() {
    assert!(is_subtype(&Type::bool_literal(true), &Type::bool()));
    assert!(is_subtype(&Type::bool_literal(false), &Type::bool()));
}

#[test]
fn int_range_subtype_rules() {
    let positive = Type::int_range(Some(1), None);
    assert!(is_subtype(&Type::int_literal(5), &positive));
    assert!(!is_subtype(&Type::int_literal(0), &positive));
    assert!(!is_subtype(&Type::int_literal(-1), &positive));
    assert!(is_subtype(&positive, &Type::int()));
    assert!(!is_subtype(&positive, &Type::float()));
    assert!(accepts(&Type::float(), &positive));
    assert!(!is_subtype(&Type::int(), &positive));

    let narrow = Type::int_range(Some(1), Some(10));
    assert!(is_subtype(&narrow, &positive));
    assert!(!is_subtype(&positive, &narrow));
    let unbounded_below = Type::int_range(None, Some(10));
    assert!(!is_subtype(&unbounded_below, &positive));
}

#[test]
fn string_accessory_subtype_rules() {
    let non_empty = Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::NonEmptyString),
    ]));
    let non_falsy = Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::NonFalsyString),
    ]));
    assert!(is_subtype(&Type::string_literal("a".into()), &non_empty));
    assert!(!is_subtype(
        &Type::string_literal(PhpString::default()),
        &non_empty
    ));
    assert!(is_subtype(&Type::string_literal("0".into()), &non_empty));
    assert!(!is_subtype(&Type::string_literal("0".into()), &non_falsy));
    assert!(is_subtype(&Type::string_literal("a".into()), &non_falsy));
    // Every non-falsy string is non-empty, not vice versa.
    assert!(is_subtype(&non_falsy, &non_empty));
    assert!(!is_subtype(&non_empty, &non_falsy));

    let literal = Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::LiteralString),
    ]));
    assert!(is_subtype(
        &Type::string_literal("anything".into()),
        &literal
    ));
    assert!(!is_subtype(&Type::string(), &literal));

    let numeric = Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::NumericString),
    ]));
    assert!(is_subtype(&Type::string_literal("123".into()), &numeric));
    assert!(is_subtype(
        &Type::string_literal(" -1.5e3 ".into()),
        &numeric
    ));
    assert!(!is_subtype(&Type::string_literal("abc".into()), &numeric));

    // A plain, unprovable String never satisfies any of these.
    assert!(!is_subtype(&Type::string(), &non_empty));
}

#[test]
fn array_accessory_subtype_rules() {
    // Scoped to `Array(Mixed)` as the intersection's own base member
    // deliberately: `is_super_type_of`'s own pre-existing, unrelated
    // `(ArrayShape(_), Array(b)) => matches!(**b, Mixed)` rule is
    // already conservative about a *specific*-element `Array(b)`
    // (that per-field check is `accepts`'s own job, not
    // `is_super_type_of`'s) - using `Array(Mixed)` here isolates the new
    // accessory logic from that pre-existing, unrelated asymmetry.
    let list_ty = Type::new(Repr::Intersection(vec![
        Type::array(
            Type::unknown_array_key(),
            Type::new(Repr::Mixed(MixedKind::Implicit, None)),
        ),
        Type::accessory(AccessoryKind::ArrayList),
    ]));
    let non_empty_ty = Type::new(Repr::Intersection(vec![
        Type::array(
            Type::unknown_array_key(),
            Type::new(Repr::Mixed(MixedKind::Implicit, None)),
        ),
        Type::accessory(AccessoryKind::NonEmptyArray),
    ]));
    let list_shape = Type::array_shape(Shape::from_named(vec![
        ("0".to_string(), Type::new(Repr::IntLiteral(1)), false),
        ("1".to_string(), Type::new(Repr::IntLiteral(2)), false),
    ]));
    assert!(is_subtype(&list_shape, &list_ty));
    assert!(is_subtype(&list_shape, &non_empty_ty));

    let gapped_shape = Type::array_shape(Shape::from_named(vec![
        ("0".to_string(), Type::new(Repr::IntLiteral(1)), false),
        ("2".to_string(), Type::new(Repr::IntLiteral(2)), false),
    ]));
    assert!(!is_subtype(&gapped_shape, &list_ty));
    assert!(is_subtype(&gapped_shape, &non_empty_ty));

    let empty_shape = Type::array_shape(Shape::sealed(Vec::new()));
    assert!(!is_subtype(&empty_shape, &non_empty_ty));

    // Real PHPStan's own confirmed strictness: a plain Array(_),
    // even with a known element type, never satisfies list<T>/
    // non-empty-array<T> - only a real, provable ArrayShape does.
    assert!(!is_subtype(
        &Type::array(Type::unknown_array_key(), Type::new(Repr::Int)),
        &list_ty
    ));
    assert!(!is_subtype(
        &Type::array(Type::unknown_array_key(), Type::new(Repr::Int)),
        &non_empty_ty
    ));
}

#[test]
fn a_shape_with_a_non_trailing_optional_field_is_not_a_definite_list() {
    // `array{0: int, 1?: string, 2: int}` is only `Maybe`-a-list (if the
    // optional field 1 is genuinely absent, keys {0, 2} aren't
    // sequential) - `is_list_like`/`Shape::is_list` must not
    // ignore each field's `optional` flag and answer `true`
    // unconditionally here.
    let list_ty = Type::new(Repr::Intersection(vec![
        Type::array(
            Type::unknown_array_key(),
            Type::new(Repr::Mixed(MixedKind::Implicit, None)),
        ),
        Type::accessory(AccessoryKind::ArrayList),
    ]));
    let non_trailing_optional = Type::array_shape(Shape::from_named(vec![
        ("0".to_string(), Type::new(Repr::Int), false),
        ("1".to_string(), Type::new(Repr::String), true),
        ("2".to_string(), Type::new(Repr::Int), false),
    ]));
    assert!(!is_subtype(&non_trailing_optional, &list_ty));

    // A *trailing* run of optional fields is fine - the shape is still a
    // definite list either way (whether the optional suffix is present or
    // not, the present keys are always sequential from zero).
    let trailing_optional = Type::array_shape(Shape::from_named(vec![
        ("0".to_string(), Type::new(Repr::Int), false),
        ("1".to_string(), Type::new(Repr::String), true),
        ("2".to_string(), Type::new(Repr::Int), true),
    ]));
    assert!(is_subtype(&trailing_optional, &list_ty));
}

#[test]
fn nullable_string_accepts_null_and_string() {
    let nullable_string = parse_type_string("?string");
    assert!(is_subtype(&Type::null(), &nullable_string));
    assert!(is_subtype(&Type::string(), &nullable_string));
    assert!(!is_subtype(&Type::int(), &nullable_string));
}

#[test]
fn array_subtyping_is_covariant_on_value_type() {
    let ints = Type::array(Type::unknown_array_key(), Type::new(Repr::Int));
    let mixed_arr = Type::array(
        Type::unknown_array_key(),
        Type::new(Repr::Mixed(MixedKind::Implicit, None)),
    );
    assert!(is_subtype(&ints, &mixed_arr));
    assert!(!is_subtype(&mixed_arr, &ints));
}

// An unresolved `Repr::Conditional` falls back to comparing against
// `Union([if_true, if_false])` on either side, mirroring the existing
// `Union` arms exactly.
#[test]
fn an_unresolved_conditional_as_sub_requires_both_branches_to_satisfy_sup() {
    let conditional = parse_type_string("($x is int ? int : string)");
    assert!(is_subtype(&conditional, &parse_type_string("int|string")));
    assert!(!is_subtype(&conditional, &Type::int()));
}

#[test]
fn an_unresolved_conditional_as_sup_is_satisfied_by_either_branch() {
    let conditional = parse_type_string("($x is int ? int : string)");
    assert!(is_subtype(&Type::int(), &conditional));
    assert!(is_subtype(&Type::string(), &conditional));
    assert!(!is_subtype(&Type::bool(), &conditional));
}

// Literal-proving arms + implication arms for
// `LowercaseString`/`UppercaseString`/`DecimalIntegerString`.

fn lowercase_string() -> Type {
    Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::LowercaseString),
    ]))
}
fn uppercase_string() -> Type {
    Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::UppercaseString),
    ]))
}
fn decimal_int_string() -> Type {
    Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::DecimalIntegerString),
    ]))
}

#[test]
fn a_lowercase_literal_proves_lowercase_string() {
    assert!(is_subtype(
        &Type::string_literal("abc".into()),
        &lowercase_string()
    ));
    assert!(is_subtype(
        &Type::string_literal("abc123".into()),
        &lowercase_string()
    ));
    assert!(!is_subtype(
        &Type::string_literal("Abc".into()),
        &lowercase_string()
    ));
    // No letters at all - vacuously lowercase, matching real
    // `strtolower($v) === $v`.
    assert!(is_subtype(
        &Type::string_literal("123".into()),
        &lowercase_string()
    ));
}

#[test]
fn an_uppercase_literal_proves_uppercase_string() {
    assert!(is_subtype(
        &Type::string_literal("ABC".into()),
        &uppercase_string()
    ));
    assert!(!is_subtype(
        &Type::string_literal("Abc".into()),
        &uppercase_string()
    ));
    assert!(is_subtype(
        &Type::string_literal("123".into()),
        &uppercase_string()
    ));
}

#[test]
fn a_canonical_decimal_integer_literal_proves_decimal_integer_string() {
    for s in ["0", "1", "1234", "-1"] {
        assert!(
            is_subtype(
                &Type::string_literal(s.to_string().into()),
                &decimal_int_string()
            ),
            "{s} should be a decimal-int-string"
        );
    }
    // Leading zero, explicit sign, `-0`, empty, and a non-integer numeric
    // string are all excluded - the exact real `ConstantStringType::
    // isDecimalIntegerString`'s own confirmed edge cases.
    for s in ["007", "+1", "-0", "", "1.0", "1,3", "foo"] {
        assert!(
            !is_subtype(
                &Type::string_literal(s.to_string().into()),
                &decimal_int_string()
            ),
            "{s} should NOT be a decimal-int-string"
        );
    }
}

#[test]
fn a_decimal_integer_string_implies_numeric_lowercase_uppercase_and_non_empty() {
    let decimal = decimal_int_string();
    assert!(is_subtype(
        &decimal,
        &Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NumericString)
        ]))
    ));
    assert!(is_subtype(&decimal, &lowercase_string()));
    assert!(is_subtype(&decimal, &uppercase_string()));
    assert!(is_subtype(
        &decimal,
        &Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString)
        ]))
    ));
    // The reverse doesn't hold - an ordinary numeric string isn't
    // necessarily a decimal integer string (e.g. `"1.5"`).
    assert!(!is_subtype(
        &Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NumericString)
        ])),
        &decimal
    ));
}

#[test]
fn class_names_are_one_class_whatever_their_case() {
    use std::hash::{BuildHasher, BuildHasherDefault};
    let hash = |t: &Type| BuildHasherDefault::<rustc_hash::FxHasher>::default().hash_one(t);
    let written = Type::object("foo".into(), Vec::new());
    let declared = Type::object("Foo".into(), Vec::new());
    assert_eq!(written, declared);
    assert_eq!(hash(&written), hash(&declared));
    assert!(is_subtype(&written, &declared));
    assert!(is_subtype(&declared, &written));
    assert_eq!(
        Type::enum_case("suit".into(), "Hearts".into()),
        Type::enum_case("Suit".into(), "Hearts".into())
    );
    assert_ne!(
        Type::enum_case("Suit".into(), "hearts".into()),
        Type::enum_case("Suit".into(), "Hearts".into())
    );
}

#[test]
fn property_names_keep_their_case_method_names_do_not() {
    assert_ne!(
        Type::has_property("name".into()),
        Type::has_property("Name".into())
    );
    assert_eq!(
        Type::has_method("run".into()),
        Type::has_method("Run".into())
    );
}

#[test]
fn an_error_accepts_and_is_accepted_by_everything() {
    assert!(accepts(&Type::int(), &Type::error()));
    assert!(accepts(&Type::error(), &Type::string()));
    assert!(
        crate::Relation::structural()
            .rule_level_accepts(&Type::int(), &Type::error())
            .yes()
    );
    assert!(is_subtype(&Type::int(), &Type::error()));
    assert_eq!(Type::error().to_string(), "*ERROR*");
}
