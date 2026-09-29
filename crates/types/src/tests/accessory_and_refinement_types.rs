//! Accessory and refinement pseudo-types (list, class-string, int ranges, array shapes, etc.).
use super::super::*;
use super::is_subtype;
use crate::Repr;

#[test]
fn list_is_treated_as_a_real_refinement_not_a_class_reference() {
    // Without this, "list" falls through to the generic-user-class
    // branch and gets treated as a literal class named "list".
    // A list is an array refined by the list accessory, keyed on
    // `int<0, max>` - the one encoding every list has, parsed or built.
    assert_eq!(
        parse_type_string("list<int>"),
        Type::new(Repr::Intersection(vec![
            Type::array(Type::int_range(Some(0), None), Type::int()),
            Type::accessory(AccessoryKind::ArrayList),
        ]))
    );
    assert_eq!(parse_type_string("list<int>"), Type::list_of(Type::int()));
    assert_eq!(parse_type_string("list"), Type::list_of(Type::mixed()));
}

#[test]
fn string_refinement_pseudo_types_with_no_verifiable_content_stay_plain_string() {
    // `class-string` (bare, no `<T>`) has nothing to preserve and stays
    // plain `String` - `callable-string`/`trait-string`/`interface-
    // string`/`enum-string` each now have their own real semantics
    // instead (see the tests below), rather than all five being
    // indistinguishable aliases.
    assert_eq!(parse_type_string("class-string"), Type::string());
}

#[test]
fn enum_string_is_a_class_string_bound_to_unit_enum() {
    // Mirrors real PHPStan's own `enum-string` resolution
    // (`GenericClassStringType(UnitEnum)`) - every enum this project's
    // own registry sees already synthesizes `implements UnitEnum`.
    assert_eq!(
        parse_type_string("enum-string"),
        Type::class_string(Type::new(Repr::Object("UnitEnum".into(), vec![])))
    );
    assert_eq!(
        parse_type_string("enum-string<Suit>"),
        Type::class_string(Type::new(Repr::Intersection(vec![
            Type::new(Repr::Object("Suit".into(), vec![])),
            Type::new(Repr::Object("UnitEnum".into(), vec![])),
        ])))
    );
}

#[test]
fn callable_string_is_a_string_and_callable_intersection() {
    // Mirrors real PHPStan's own `callable-string` resolution
    // (`IntersectionType([StringType, CallableType])`) - architecturally
    // unrelated to the `class-string` family despite the shared name.
    assert_eq!(
        parse_type_string("callable-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::callable(None)
        ]))
    );
}

#[test]
fn interface_string_and_trait_string_are_class_kind_strings() {
    // This project is allowed to check things more strictly than real
    // PHPStan does (real PHPStan gives these no distinguishing semantics
    // from bare `class-string` either) - the kind is checked against the
    // real `ClassKind` at `registry::accepts`.
    assert_eq!(
        parse_type_string("interface-string"),
        Type::new(Repr::ClassKindString(crate::ClassKind::Interface))
    );
    assert_eq!(
        parse_type_string("trait-string"),
        Type::new(Repr::ClassKindString(crate::ClassKind::Trait))
    );
}

// `lowercase-string`/`uppercase-string`/`non-empty-lowercase-string`/
// `non-empty-uppercase-string` must not fall through to the
// generic-user-class branch and get namespace-qualified into a bogus
// "class", which then fails every ordinary `string`-typed assignability
// check. Unlike `class-string`, case-content genuinely is something
// this project can prove and produce, so these route through the same
// `Intersection([String, Accessory(_)])` machinery as
// `non-empty-string`/`numeric-string`.
#[test]
fn case_refinement_string_pseudo_types_are_now_real_accessory_types() {
    assert_eq!(
        parse_type_string("lowercase-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::LowercaseString)
        ]))
    );
    assert_eq!(
        parse_type_string("uppercase-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::UppercaseString)
        ]))
    );
    assert_eq!(
        parse_type_string("non-empty-lowercase-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString),
            Type::accessory(AccessoryKind::LowercaseString)
        ]))
    );
    assert_eq!(
        parse_type_string("non-empty-uppercase-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString),
            Type::accessory(AccessoryKind::UppercaseString)
        ]))
    );
}

#[test]
fn decimal_int_string_is_a_real_accessory_type() {
    assert_eq!(
        parse_type_string("decimal-int-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::DecimalIntegerString)
        ]))
    );
}

// `int-mask<Flag::*>`/`int-mask-of<Flag::*>` expand into every
// combinable bitmask value in PHPStan, which this project can't
// enumerate without resolving the referenced class constants. Falling
// through to the generic-class branch would produce a bogus "class"
// that then fails ordinary `int` assignability checks; degrading to the
// pseudo-type's underlying kind, `Repr::Int`, is a strict improvement.
#[test]
fn int_mask_pseudo_types_degrade_to_plain_int() {
    assert_eq!(parse_type_string("int-mask"), Type::int());
    assert_eq!(
        parse_type_string("int-mask-of<Foo::*>"),
        Type::int(),
        "the generic argument is parsed and discarded, not left unconsumed"
    );
}

#[test]
fn string_refinement_pseudo_types_with_verifiable_content_are_real_accessory_types() {
    // Each is a literal-provable fact about a string's content,
    // mirroring PHPStan's own Intersection([StringType, AccessoryXType])
    // representation.
    assert_eq!(
        parse_type_string("non-empty-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NonEmptyString)
        ]))
    );
    for name in ["non-falsy-string", "truthy-string"] {
        assert_eq!(
            parse_type_string(name),
            Type::new(Repr::Intersection(vec![
                Type::string(),
                Type::accessory(AccessoryKind::NonFalsyString)
            ])),
            "for {name}"
        );
    }
    assert_eq!(
        parse_type_string("literal-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::LiteralString)
        ]))
    );
    assert_eq!(
        parse_type_string("numeric-string"),
        Type::new(Repr::Intersection(vec![
            Type::string(),
            Type::accessory(AccessoryKind::NumericString)
        ]))
    );
}

#[test]
fn class_string_with_a_generic_argument_is_tracked_precisely() {
    assert_eq!(
        parse_type_string("class-string<Foo>"),
        Type::class_string(Type::new(Repr::Object("Foo".into(), Vec::new())))
    );
    // Two or more args isn't real PHPStan syntax - falls back to plain
    // string rather than guessing which one is meant.
    assert_eq!(parse_type_string("class-string<Foo, Bar>"), Type::string());
}

// `describe_accessory_intersection` uses a generic ordered-parts join
// rather than a fixed tuple match, so any combination (e.g.
// `non-empty-numeric-string`) composes correctly, not just a hardcoded
// set of singleton cases.
#[test]
fn accessory_intersection_display_composes_every_combination() {
    assert_eq!(
        parse_type_string("non-empty-string").to_string(),
        "non-empty-string"
    );
    assert_eq!(
        parse_type_string("non-falsy-string").to_string(),
        "non-falsy-string"
    );
    assert_eq!(
        parse_type_string("literal-string").to_string(),
        "literal-string"
    );
    assert_eq!(
        parse_type_string("numeric-string").to_string(),
        "numeric-string"
    );
    assert_eq!(
        parse_type_string("lowercase-string").to_string(),
        "lowercase-string"
    );
    assert_eq!(
        parse_type_string("uppercase-string").to_string(),
        "uppercase-string"
    );
    assert_eq!(
        parse_type_string("decimal-int-string").to_string(),
        "decimal-int-string"
    );
    assert_eq!(
        parse_type_string("non-empty-lowercase-string").to_string(),
        "non-empty-lowercase-string"
    );
    assert_eq!(
        parse_type_string("non-empty-uppercase-string").to_string(),
        "non-empty-uppercase-string"
    );
    let date_y_shaped = Type::new(Repr::Intersection(vec![
        Type::string(),
        Type::accessory(AccessoryKind::NonEmptyString),
        Type::accessory(AccessoryKind::NumericString),
    ]));
    assert_eq!(date_y_shaped.to_string(), "non-empty-numeric-string");
}

#[test]
fn int_refinement_pseudo_types_are_real_int_ranges() {
    // Mirrors PHPStan's own standalone IntegerRangeType.
    assert_eq!(
        parse_type_string("positive-int"),
        Type::int_range(Some(1), None)
    );
    assert_eq!(
        parse_type_string("negative-int"),
        Type::int_range(None, Some(-1))
    );
    assert_eq!(
        parse_type_string("non-negative-int"),
        Type::int_range(Some(0), None)
    );
    assert_eq!(
        parse_type_string("non-positive-int"),
        Type::int_range(None, Some(0))
    );
    // Verified via dumpType(): real PHPStan represents this as a
    // union of two ranges, not its own type.
    assert_eq!(
        parse_type_string("non-zero-int"),
        Type::new(Repr::Union(vec![
            Type::int_range(None, Some(-1)),
            Type::int_range(Some(1), None),
        ]))
    );
}

#[test]
fn int_range_generic_syntax_parses() {
    assert_eq!(
        parse_type_string("int<0, 255>"),
        Type::int_range(Some(0), Some(255))
    );
    assert_eq!(
        parse_type_string("int<min, 10>"),
        Type::int_range(None, Some(10))
    );
    assert_eq!(
        parse_type_string("int<0, max>"),
        Type::int_range(Some(0), None)
    );
    // A malformed bound falls back to plain Int rather than
    // silently treating garbage as an unbounded side.
    assert_eq!(parse_type_string("int<garbage, 10>"), Type::int());
}

#[test]
fn array_refinement_pseudo_types_are_real_accessory_intersections() {
    // Mirrors PHPStan's own Intersection([Array(_), Accessory(_)])
    // representation.
    assert_eq!(
        parse_type_string("non-empty-array<int>"),
        Type::new(Repr::Intersection(vec![
            Type::array(Type::unknown_array_key(), Type::new(Repr::Int)),
            Type::accessory(AccessoryKind::NonEmptyArray),
        ]))
    );
    assert_eq!(
        // A list keys on `int<0, max>`, unlike the unknown `int|string`
        // default plain non-empty-array keeps above.
        parse_type_string("non-empty-list<string>"),
        Type::new(Repr::Intersection(vec![
            Type::array(Type::int_range(Some(0), None), Type::new(Repr::String)),
            Type::accessory(AccessoryKind::ArrayList),
            Type::accessory(AccessoryKind::NonEmptyArray),
        ]))
    );
    // `class-string-map<K, V>` refines `array<class-string<K>, V>` - the
    // key type is wrapped in ClassString, not discarded.
    assert_eq!(
        parse_type_string("class-string-map<Foo, int>"),
        Type::array(
            Type::new(Repr::ClassString(Type::new(Repr::Object(
                "Foo".into(),
                Vec::new()
            )))),
            Type::new(Repr::Int)
        )
    );
}

#[test]
fn array_shape_parses_required_and_optional_fields() {
    let shape = parse_type_string("array{name: string, age?: int}");
    assert_eq!(
        shape,
        Type::array_shape(Shape::from_named(vec![
            ("name".to_string(), Type::new(Repr::String), false),
            ("age".to_string(), Type::new(Repr::Int), true),
        ]))
    );
}

#[test]
fn array_shape_field_can_itself_be_a_union() {
    let shape = parse_type_string("array{value: int|string}");
    assert_eq!(
        shape,
        Type::array_shape(Shape::from_named(vec![(
            "value".to_string(),
            Type::new(Repr::Union(vec![
                Type::new(Repr::Int),
                Type::new(Repr::String)
            ])),
            false
        )]))
    );
}

#[test]
fn array_shape_with_an_unsealed_marker_still_parses_its_real_fields() {
    // `...<K, V>` (PHPStan/Psalm's "this shape may carry other,
    // unlisted keys too" marker) has no `:` in it, so the field loop's
    // `?` must not propagate a None out of the whole function and
    // silently fail the ENTIRE shape parse over one unmodeled marker.
    // The marker unseals the shape, with the tail's written key and value
    // types, consumed by `checker::narrowing`'s "never prove an unsealed
    // shape's key is absent" gate and `checker::assignment`'s matching
    // `isset()`/access-existence leniency.
    let shape = parse_type_string("array{items: int, ...<string, mixed>}");
    assert_eq!(
        shape,
        Type::array_shape(crate::Shape {
            fields: vec![crate::ShapeField::new(
                crate::ShapeKey::from_name("items"),
                Type::new(Repr::Int),
                false
            )],
            unsealed: Some(Box::new((
                Type::new(Repr::String),
                Type::new(Repr::Mixed(false, None))
            ))),
            next_index: None,
        })
    );
    // A bare `...` (no generic args) is valid too.
    let shape2 = parse_type_string("array{items: int, ...}");
    assert_eq!(
        shape2,
        Type::array_shape(crate::Shape::unsealed(vec![crate::ShapeField::new(
            crate::ShapeKey::from_name("items"),
            Type::new(Repr::Int),
            false
        ),]))
    );
}

// A positional (unkeyed) field must not fail the entire shape's parse
// (`split_once(':')?` has nothing to split), falling through to a bogus,
// unparsed class-name `Object`. Numbered the same way
// `infer_array_literal_type` numbers an array literal's own positional
// elements, so both end up structurally identical.
#[test]
fn array_shape_with_positional_fields_numbers_them_like_a_real_array_literal() {
    let shape = parse_type_string("array{int, string}");
    assert_eq!(
        shape,
        Type::array_shape(Shape::from_named(vec![
            ("0".to_string(), Type::new(Repr::Int), false),
            ("1".to_string(), Type::new(Repr::String), false),
        ]))
    );
}

#[test]
fn array_shape_with_positional_int_range_fields_parses() {
    let shape = parse_type_string("array{int<0, 255>, int<0, 255>, int<0, 255>}");
    // Each field is a real IntRange, not plain Int.
    assert_eq!(
        shape,
        Type::array_shape(Shape::from_named(vec![
            (
                "0".to_string(),
                Type::new(Repr::IntRange(Some(0), Some(255))),
                false
            ),
            (
                "1".to_string(),
                Type::new(Repr::IntRange(Some(0), Some(255))),
                false
            ),
            (
                "2".to_string(),
                Type::new(Repr::IntRange(Some(0), Some(255))),
                false
            ),
        ]))
    );
}

#[test]
fn array_shape_mixing_explicit_int_keys_and_named_keys_still_parses_correctly() {
    let shape = parse_type_string("array{0: int, name: string}");
    assert_eq!(
        shape,
        Type::array_shape(Shape::from_named(vec![
            ("0".to_string(), Type::new(Repr::Int), false),
            ("name".to_string(), Type::new(Repr::String), false),
        ]))
    );
}

// A hyphenated bare key must not be rejected by `looks_like_shape_key`,
// misclassifying the field as positional.
#[test]
fn array_shape_with_a_hyphenated_bare_key_still_parses_as_keyed() {
    let shape = parse_type_string("array{domain: string, plural-forms: string}");
    assert_eq!(
        shape,
        Type::array_shape(Shape::from_named(vec![
            ("domain".to_string(), Type::new(Repr::String), false),
            ("plural-forms".to_string(), Type::new(Repr::String), false),
        ]))
    );
}

// A positional field whose own type contains a colon (a callable
// shape's return type) must not be mis-split at that internal colon.
#[test]
fn array_shape_with_a_positional_closure_field_is_not_split_at_its_own_colon() {
    let shape = parse_type_string("array{Closure(int): string, int}");
    let Repr::ArrayShape(fields) = shape.repr() else {
        panic!("expected an array shape, got {shape:?}");
    };
    let fields = fields.named_fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0, "0");
    assert_eq!(fields[0].1.to_string(), "Closure(int): string");
    assert!(!fields[0].2);
    assert_eq!(fields[1], ("1".to_string(), Type::int(), false));
}

#[test]
fn a_positional_class_constant_field_is_not_read_as_a_key() {
    let shape = parse_type_string("array{self::STRATEGY_*, non-empty-list<self::STRATEGY_*>}");
    let Repr::ArrayShape(fields) = shape.repr() else {
        panic!("expected an array shape, got {shape:?}");
    };
    let fields = fields.named_fields();
    let keys: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
    assert_eq!(keys, ["0", "1"]);
}

#[test]
fn a_numeric_string_is_never_empty() {
    assert!(is_subtype(
        &parse_type_string("numeric-string"),
        &parse_type_string("non-empty-string")
    ));
    assert!(is_subtype(
        &parse_type_string("decimal-int-string"),
        &parse_type_string("non-empty-string")
    ));
    assert!(!is_subtype(
        &parse_type_string("numeric-string"),
        &parse_type_string("non-falsy-string")
    ));
}

#[test]
fn an_intersection_of_arrays_is_one_array_with_every_refinement() {
    let expected = parse_type_string("non-empty-list<int>");
    for written in [
        "list<int>&non-empty-array",
        "non-empty-array&list<int>",
        "list<int>&non-empty-list<int>",
    ] {
        let ty = parse_type_string(written);
        assert!(
            is_subtype(&ty, &expected) && is_subtype(&expected, &ty),
            "{written} parsed as {ty}"
        );
        assert_eq!(ty.to_string(), "non-empty-list<int>", "{written}");
    }
}

#[test]
fn an_integer_range_absorbs_the_constants_and_ranges_touching_it() {
    let range = |min, max| Type::int_range(min, max);
    assert_eq!(
        Type::union(Type::int_literal(0), range(Some(1), None)),
        range(Some(0), None)
    );
    assert_eq!(
        Type::union(range(Some(1), Some(5)), range(Some(6), Some(9))),
        range(Some(1), Some(9))
    );
    assert_eq!(
        Type::union(range(None, Some(0)), range(Some(1), None)),
        Type::int()
    );
    assert_eq!(
        Type::union(range(Some(3), Some(5)), Type::int_literal(1)),
        Type::new(Repr::Union(vec![
            range(Some(3), Some(5)),
            Type::int_literal(1)
        ]))
    );
    assert_eq!(
        Type::union(Type::int_literal(1), Type::int_literal(2)),
        Type::new(Repr::Union(vec![
            Type::int_literal(1),
            Type::int_literal(2)
        ]))
    );
    assert_eq!(parse_type_string("0|positive-int"), range(Some(0), None));
}

#[test]
fn an_offset_an_intersection_records_is_surely_there() {
    let key = || Type::string_literal("k".into());
    let base = || Type::array(Type::string(), Type::int());
    let with_offset = Type::new(Repr::Intersection(vec![
        base(),
        Type::has_offset(ShapeKey::Str("k".into())),
    ]));
    assert_eq!(with_offset.has_offset_value_type(&key()), TrinaryLogic::Yes);
    let with_value = Type::new(Repr::Intersection(vec![
        base(),
        Type::has_offset_value(ShapeKey::Str("k".into()), Type::int()),
    ]));
    assert_eq!(with_value.has_offset_value_type(&key()), TrinaryLogic::Yes);
    assert_eq!(
        with_value.has_offset_value_type(&Type::string_literal("other".into())),
        TrinaryLogic::Maybe
    );
    let template = Type::template_param(TemplateParam {
        bound: Box::new(Type::array_shape(Shape::sealed(vec![ShapeField::new(
            ShapeKey::Str("k".into()),
            Type::int(),
            false,
        )]))),
        ..TemplateParam::unbounded("T".to_string())
    });
    assert_eq!(template.has_offset_value_type(&key()), TrinaryLogic::Yes);
}

#[test]
fn an_offset_of_several_literal_keys_reads_those_fields_joined() {
    let shape = Type::array_shape(Shape::from_named(vec![
        ("sql", Type::string(), false),
        ("dql", Type::string(), false),
        ("labels", Type::null(), false),
    ]));
    let key = Type::union(
        Type::string_literal("sql".into()),
        Type::string_literal("dql".into()),
    );
    assert_eq!(shape.offset_value_type_at(&key), Some(Type::string()));
    assert_eq!(
        shape.offset_value_type_at(&Type::string()),
        Some(Type::union(Type::string(), Type::null()))
    );
}
