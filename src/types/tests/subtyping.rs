//! Subtype relation checks between `Type` values.
use super::super::*;

#[test]
fn class_string_is_interchangeable_with_plain_string_both_ways() {
    let cs = Type::ClassString(Box::new(Type::Object("Foo".to_string(), Vec::new())));
    assert!(is_subtype(&cs, &Type::String));
    assert!(is_subtype(&Type::String, &cs));
    assert!(is_subtype(&Type::StringLiteral("Foo".to_string()), &cs));
    assert!(is_subtype(&cs, &cs));
    let cs_other = Type::ClassString(Box::new(Type::Object("Bar".to_string(), Vec::new())));
    assert!(!is_subtype(&cs, &cs_other));
}

#[test]
fn array_shape_is_subtype_of_bare_array_but_not_the_reverse() {
    let shape = parse_type_string("array{name: string}");
    assert!(is_subtype(
        &shape,
        &Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed))
    ));
    assert!(!is_subtype(
        &Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
        &shape
    ));
}

// Phase 50: on top of the pre-existing "only when the array's own value
// type is Mixed" rule (no per-field value comparison happens here at all -
// that's `registry::is_assignable`'s own, separate job), a shape's own
// real aggregate key type must also be a subtype of a precisely-keyed
// `Type::Array`'s own declared key type.

#[test]
fn a_string_keyed_shape_is_a_subtype_of_a_matching_string_keyed_mixed_valued_array() {
    let shape = parse_type_string("array{name: string}");
    assert!(is_subtype(
        &shape,
        &Type::Array(Box::new(Type::String), Box::new(Type::Mixed))
    ));
}

#[test]
fn a_string_keyed_shape_is_not_a_subtype_of_an_int_keyed_mixed_valued_array() {
    let shape = parse_type_string("array{name: string}");
    assert!(!is_subtype(
        &shape,
        &Type::Array(Box::new(Type::Int), Box::new(Type::Mixed))
    ));
}

#[test]
fn literal_is_subtype_of_its_general_type() {
    assert!(is_subtype(&Type::IntLiteral(5), &Type::Int));
    assert!(!is_subtype(&Type::Int, &Type::IntLiteral(5)));
}

#[test]
fn everything_is_subtype_of_mixed() {
    assert!(is_subtype(&Type::String, &Type::Mixed));
    assert!(is_subtype(
        &Type::Object("Foo".to_string(), Vec::new()),
        &Type::Mixed
    ));
}

#[test]
fn an_unbounded_template_param_is_still_unconditionally_compatible_either_way() {
    // Phase 54: the extremely common bound-free `@template T` must keep
    // `is_subtype`'s old blanket leniency exactly as before - an
    // undeclared bound is `Mixed`, and `is_subtype(Mixed, sup)` is only
    // ever true when `sup` is `Mixed` too, so without this the new
    // bound-aware `sub` arm would turn every one of these into a brand
    // new false positive.
    let t = Type::TemplateParam(TemplateParam::unbounded("T".to_string()));
    assert!(is_subtype(&t, &Type::String));
    assert!(is_subtype(&t, &Type::Object("Foo".to_string(), Vec::new())));
    assert!(is_subtype(&Type::String, &t));
    assert!(is_subtype(&Type::Object("Foo".to_string(), Vec::new()), &t));
}

#[test]
fn a_bounded_template_param_is_a_subtype_of_whatever_its_bound_satisfies() {
    // A `T of int` is guaranteed to be an `int` (or narrower) however
    // it's eventually instantiated, so it's a subtype of anything `int`
    // itself is a subtype of - `float`, in particular, via real PHP's
    // own int-to-float widening rule.
    let t = Type::TemplateParam(TemplateParam {
        name: "T".to_string(),
        bound: Box::new(Type::Int),
        variance: Variance::Invariant,
        scope: None,
        default: None,
    });
    assert!(is_subtype(&t, &Type::Int));
    assert!(is_subtype(&t, &Type::Float));
    assert!(!is_subtype(&t, &Type::String));
}

// Real corpus false positive: doctrine/collections' own `@template TKey
// of array-key` (bound `int|string`, a Union) - `is_subtype`'s own
// generic `(_, Union(members)) => any(is_subtype(sub, m))` arm used to
// run *before* the dedicated `TemplateParam` arm below, splitting the
// *target* union member-by-member (`is_subtype(TKey, Int)` and
// `is_subtype(TKey, String)`, each correctly false on its own) instead
// of comparing `TKey`'s own union bound against the target as a whole.
#[test]
fn a_template_param_whose_own_bound_is_a_union_is_a_subtype_of_that_same_union() {
    let t = Type::TemplateParam(TemplateParam {
        name: "TKey".to_string(),
        bound: Box::new(Type::Union(vec![Type::Int, Type::String])),
        variance: Variance::Invariant,
        scope: None,
        default: None,
    });
    assert!(is_subtype(&t, &Type::Union(vec![Type::Int, Type::String])));
    assert!(!is_subtype(&t, &Type::Int));
    assert!(!is_subtype(&t, &Type::String));
}

#[test]
fn a_bounded_template_param_still_accepts_anything_as_a_sink() {
    // The reverse direction (T used as `sup`, the assignment target)
    // stays maximally lenient regardless of any declared bound: knowing
    // a value satisfies T's *upper* bound doesn't prove it satisfies
    // whatever T is actually instantiated to.
    let t = Type::TemplateParam(TemplateParam {
        name: "T".to_string(),
        bound: Box::new(Type::Int),
        variance: Variance::Invariant,
        scope: None,
        default: None,
    });
    assert!(is_subtype(&Type::String, &t));
}

#[test]
fn never_is_subtype_of_everything() {
    assert!(is_subtype(&Type::Never, &Type::String));
    assert!(is_subtype(&Type::Never, &Type::Mixed));
}

#[test]
fn union_subtype_rules() {
    let small = Type::Union(vec![Type::Int, Type::String]);
    let big = Type::Union(vec![Type::Int, Type::String, Type::Bool]);
    assert!(is_subtype(&small, &big));
    assert!(!is_subtype(&big, &small));
    assert!(is_subtype(&Type::String, &small));
}

#[test]
fn intersection_subtype_rules() {
    // Coverage measurement found both Intersection arms of is_subtype
    // had zero executions (registry::is_assignable's own, separate
    // Intersection handling was covered independently).
    let both = Type::Intersection(vec![Type::Int, Type::String]);
    // sub is the intersection: satisfies any one of its members.
    assert!(is_subtype(&both, &Type::Int));
    assert!(is_subtype(&both, &Type::String));
    assert!(!is_subtype(&both, &Type::Bool));
    // sup is the intersection: must satisfy every member at once - a
    // plain Int alone doesn't also satisfy "and a String".
    assert!(!is_subtype(&Type::Int, &both));
}

#[test]
fn true_and_false_literals_are_subtypes_of_bool() {
    assert!(is_subtype(&Type::True, &Type::Bool));
    assert!(is_subtype(&Type::False, &Type::Bool));
}

#[test]
fn int_range_subtype_rules() {
    let positive = Type::IntRange(Some(1), None);
    assert!(is_subtype(&Type::IntLiteral(5), &positive));
    assert!(!is_subtype(&Type::IntLiteral(0), &positive));
    assert!(!is_subtype(&Type::IntLiteral(-1), &positive));
    assert!(is_subtype(&positive, &Type::Int));
    assert!(is_subtype(&positive, &Type::Float));
    assert!(!is_subtype(&Type::Int, &positive));

    let narrow = Type::IntRange(Some(1), Some(10));
    assert!(is_subtype(&narrow, &positive));
    assert!(!is_subtype(&positive, &narrow));
    let unbounded_below = Type::IntRange(None, Some(10));
    assert!(!is_subtype(&unbounded_below, &positive));
}

#[test]
fn string_accessory_subtype_rules() {
    let non_empty = Type::Intersection(vec![
        Type::String,
        Type::Accessory(AccessoryKind::NonEmptyString),
    ]);
    let non_falsy = Type::Intersection(vec![
        Type::String,
        Type::Accessory(AccessoryKind::NonFalsyString),
    ]);
    assert!(is_subtype(
        &Type::StringLiteral("a".to_string()),
        &non_empty
    ));
    assert!(!is_subtype(&Type::StringLiteral(String::new()), &non_empty));
    assert!(is_subtype(
        &Type::StringLiteral("0".to_string()),
        &non_empty
    ));
    assert!(!is_subtype(
        &Type::StringLiteral("0".to_string()),
        &non_falsy
    ));
    assert!(is_subtype(
        &Type::StringLiteral("a".to_string()),
        &non_falsy
    ));
    // Every non-falsy string is non-empty, not vice versa.
    assert!(is_subtype(&non_falsy, &non_empty));
    assert!(!is_subtype(&non_empty, &non_falsy));

    let literal = Type::Intersection(vec![
        Type::String,
        Type::Accessory(AccessoryKind::LiteralString),
    ]);
    assert!(is_subtype(
        &Type::StringLiteral("anything".to_string()),
        &literal
    ));
    assert!(!is_subtype(&Type::String, &literal));

    let numeric = Type::Intersection(vec![
        Type::String,
        Type::Accessory(AccessoryKind::NumericString),
    ]);
    assert!(is_subtype(
        &Type::StringLiteral("123".to_string()),
        &numeric
    ));
    assert!(is_subtype(
        &Type::StringLiteral(" -1.5e3 ".to_string()),
        &numeric
    ));
    assert!(!is_subtype(
        &Type::StringLiteral("abc".to_string()),
        &numeric
    ));

    // A plain, unprovable String never satisfies any of these.
    assert!(!is_subtype(&Type::String, &non_empty));
}

#[test]
fn array_accessory_subtype_rules() {
    // Scoped to `Array(Mixed)` as the intersection's own base member
    // deliberately: `is_subtype`'s own pre-existing, unrelated
    // `(ArrayShape(_), Array(b)) => matches!(**b, Mixed)` rule is
    // already conservative about a *specific*-element `Array(b)`
    // (that per-field check is `is_assignable`'s own job, not
    // `is_subtype`'s) - using `Array(Mixed)` here isolates the new
    // accessory logic from that pre-existing, unrelated asymmetry.
    let list_ty = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
        Type::Accessory(AccessoryKind::ArrayList),
    ]);
    let non_empty_ty = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
        Type::Accessory(AccessoryKind::NonEmptyArray),
    ]);
    let list_shape = Type::ArrayShape(vec![
        ("0".to_string(), Type::IntLiteral(1), false),
        ("1".to_string(), Type::IntLiteral(2), false),
    ]);
    assert!(is_subtype(&list_shape, &list_ty));
    assert!(is_subtype(&list_shape, &non_empty_ty));

    let gapped_shape = Type::ArrayShape(vec![
        ("0".to_string(), Type::IntLiteral(1), false),
        ("2".to_string(), Type::IntLiteral(2), false),
    ]);
    assert!(!is_subtype(&gapped_shape, &list_ty));
    assert!(is_subtype(&gapped_shape, &non_empty_ty));

    let empty_shape = Type::ArrayShape(vec![]);
    assert!(!is_subtype(&empty_shape, &non_empty_ty));

    // Real PHPStan's own confirmed strictness: a plain Array(_),
    // even with a known element type, never satisfies list<T>/
    // non-empty-array<T> - only a real, provable ArrayShape does.
    assert!(!is_subtype(
        &Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
        &list_ty
    ));
    assert!(!is_subtype(
        &Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
        &non_empty_ty
    ));
}

#[test]
fn a_shape_with_a_non_trailing_optional_field_is_not_a_definite_list() {
    // Phase 50: `array{0: int, 1?: string, 2: int}` is only `Maybe`-a-list
    // in real PHPStan (if the optional field 1 is genuinely absent, keys
    // {0, 2} aren't sequential) - `is_list_like`/`shape_fields_are_list_
    // like` used to ignore each field's own `optional` flag entirely and
    // answer `true` unconditionally here.
    let list_ty = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
        Type::Accessory(AccessoryKind::ArrayList),
    ]);
    let non_trailing_optional = Type::ArrayShape(vec![
        ("0".to_string(), Type::Int, false),
        ("1".to_string(), Type::String, true),
        ("2".to_string(), Type::Int, false),
    ]);
    assert!(!is_subtype(&non_trailing_optional, &list_ty));

    // A *trailing* run of optional fields is fine - the shape is still a
    // definite list either way (whether the optional suffix is present or
    // not, the present keys are always sequential from zero).
    let trailing_optional = Type::ArrayShape(vec![
        ("0".to_string(), Type::Int, false),
        ("1".to_string(), Type::String, true),
        ("2".to_string(), Type::Int, true),
    ]);
    assert!(is_subtype(&trailing_optional, &list_ty));
}

#[test]
fn nullable_string_accepts_null_and_string() {
    let nullable_string = parse_type_string("?string");
    assert!(is_subtype(&Type::Null, &nullable_string));
    assert!(is_subtype(&Type::String, &nullable_string));
    assert!(!is_subtype(&Type::Int, &nullable_string));
}

#[test]
fn array_subtyping_is_covariant_on_value_type() {
    let ints = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int));
    let mixed_arr = Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed));
    assert!(is_subtype(&ints, &mixed_arr));
    assert!(!is_subtype(&mixed_arr, &ints));
}

// Phase 80 follow-up: an unresolved `Type::Conditional` falls back to
// comparing against `Union([if_true, if_false])` on either side, mirroring
// the existing `Union` arms exactly.
#[test]
fn an_unresolved_conditional_as_sub_requires_both_branches_to_satisfy_sup() {
    let conditional = parse_type_string("($x is int ? int : string)");
    assert!(is_subtype(&conditional, &parse_type_string("int|string")));
    assert!(!is_subtype(&conditional, &Type::Int));
}

#[test]
fn an_unresolved_conditional_as_sup_is_satisfied_by_either_branch() {
    let conditional = parse_type_string("($x is int ? int : string)");
    assert!(is_subtype(&Type::Int, &conditional));
    assert!(is_subtype(&Type::String, &conditional));
    assert!(!is_subtype(&Type::Bool, &conditional));
}
