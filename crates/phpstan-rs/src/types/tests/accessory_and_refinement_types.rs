//! Accessory and refinement pseudo-types (list, class-string, int ranges, array shapes, etc.).
use super::super::*;

#[test]
fn list_is_treated_as_a_real_refinement_not_a_class_reference() {
    // Without this, "list" falls through to the generic-user-class
    // branch and gets treated as a literal class named "list".
    // `list<T>` is a real `Intersection([Array(_), Accessory(ArrayList)])`,
    // not a plain Array alias - a list's key can only ever be a
    // sequential int.
    assert_eq!(
        parse_type_string("list<int>"),
        Type::Intersection(vec![
            Type::Array(Box::new(Type::Int), Box::new(Type::Int)),
            Type::Accessory(AccessoryKind::ArrayList),
        ])
    );
    assert_eq!(
        parse_type_string("list"),
        Type::Intersection(vec![
            Type::Array(Box::new(Type::Int), Box::new(Type::Mixed(false))),
            Type::Accessory(AccessoryKind::ArrayList),
        ])
    );
}

#[test]
fn string_refinement_pseudo_types_with_no_verifiable_content_stay_plain_string() {
    // `class-string` (bare, no `<T>`) has nothing to preserve and stays
    // plain `String` - `callable-string`/`trait-string`/`interface-
    // string`/`enum-string` each now have their own real semantics
    // instead (see the tests below), rather than all five being
    // indistinguishable aliases.
    assert_eq!(parse_type_string("class-string"), Type::String);
}

#[test]
fn enum_string_is_a_class_string_bound_to_unit_enum() {
    // Mirrors real PHPStan's own `enum-string` resolution
    // (`GenericClassStringType(UnitEnum)`) - every enum this project's
    // own registry sees already synthesizes `implements UnitEnum`.
    assert_eq!(
        parse_type_string("enum-string"),
        Type::ClassString(Box::new(Type::Object("UnitEnum".to_string(), vec![])))
    );
    assert_eq!(
        parse_type_string("enum-string<Suit>"),
        Type::ClassString(Box::new(Type::Intersection(vec![
            Type::Object("Suit".to_string(), vec![]),
            Type::Object("UnitEnum".to_string(), vec![]),
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
        Type::Intersection(vec![Type::String, Type::Callable(None)])
    );
}

#[test]
fn interface_string_and_trait_string_are_class_kind_strings() {
    // This project is allowed to check things more strictly than real
    // PHPStan does (real PHPStan gives these no distinguishing semantics
    // from bare `class-string` either) - the kind is checked against the
    // real `ClassKind` at `registry::is_assignable`.
    assert_eq!(
        parse_type_string("interface-string"),
        Type::ClassKindString(crate::types::ClassKind::Interface)
    );
    assert_eq!(
        parse_type_string("trait-string"),
        Type::ClassKindString(crate::types::ClassKind::Trait)
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
        Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::LowercaseString)
        ])
    );
    assert_eq!(
        parse_type_string("uppercase-string"),
        Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::UppercaseString)
        ])
    );
    assert_eq!(
        parse_type_string("non-empty-lowercase-string"),
        Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonEmptyString),
            Type::Accessory(AccessoryKind::LowercaseString)
        ])
    );
    assert_eq!(
        parse_type_string("non-empty-uppercase-string"),
        Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonEmptyString),
            Type::Accessory(AccessoryKind::UppercaseString)
        ])
    );
}

#[test]
fn decimal_int_string_is_a_real_accessory_type() {
    assert_eq!(
        parse_type_string("decimal-int-string"),
        Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::DecimalIntegerString)
        ])
    );
}

// `int-mask<Flag::*>`/`int-mask-of<Flag::*>` expand into every
// combinable bitmask value in PHPStan, which this project can't
// enumerate without resolving the referenced class constants. Falling
// through to the generic-class branch would produce a bogus "class"
// that then fails ordinary `int` assignability checks; degrading to the
// pseudo-type's underlying kind, `Type::Int`, is a strict improvement.
#[test]
fn int_mask_pseudo_types_degrade_to_plain_int() {
    assert_eq!(parse_type_string("int-mask"), Type::Int);
    assert_eq!(
        parse_type_string("int-mask-of<Foo::*>"),
        Type::Int,
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
        Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NonEmptyString)
        ])
    );
    for name in ["non-falsy-string", "truthy-string"] {
        assert_eq!(
            parse_type_string(name),
            Type::Intersection(vec![
                Type::String,
                Type::Accessory(AccessoryKind::NonFalsyString)
            ]),
            "for {name}"
        );
    }
    assert_eq!(
        parse_type_string("literal-string"),
        Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::LiteralString)
        ])
    );
    assert_eq!(
        parse_type_string("numeric-string"),
        Type::Intersection(vec![
            Type::String,
            Type::Accessory(AccessoryKind::NumericString)
        ])
    );
}

#[test]
fn class_string_with_a_generic_argument_is_tracked_precisely() {
    assert_eq!(
        parse_type_string("class-string<Foo>"),
        Type::ClassString(Box::new(Type::Object("Foo".to_string(), Vec::new())))
    );
    // Two or more args isn't real PHPStan syntax - falls back to plain
    // string rather than guessing which one is meant.
    assert_eq!(parse_type_string("class-string<Foo, Bar>"), Type::String);
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
    let date_y_shaped = Type::Intersection(vec![
        Type::String,
        Type::Accessory(AccessoryKind::NonEmptyString),
        Type::Accessory(AccessoryKind::NumericString),
    ]);
    assert_eq!(date_y_shaped.to_string(), "non-empty-numeric-string");
}

#[test]
fn int_refinement_pseudo_types_are_real_int_ranges() {
    // Mirrors PHPStan's own standalone IntegerRangeType.
    assert_eq!(
        parse_type_string("positive-int"),
        Type::IntRange(Some(1), None)
    );
    assert_eq!(
        parse_type_string("negative-int"),
        Type::IntRange(None, Some(-1))
    );
    assert_eq!(
        parse_type_string("non-negative-int"),
        Type::IntRange(Some(0), None)
    );
    assert_eq!(
        parse_type_string("non-positive-int"),
        Type::IntRange(None, Some(0))
    );
    // Verified via dumpType(): real PHPStan represents this as a
    // union of two ranges, not its own type.
    assert_eq!(
        parse_type_string("non-zero-int"),
        Type::Union(vec![
            Type::IntRange(None, Some(-1)),
            Type::IntRange(Some(1), None),
        ])
    );
}

#[test]
fn int_range_generic_syntax_parses() {
    assert_eq!(
        parse_type_string("int<0, 255>"),
        Type::IntRange(Some(0), Some(255))
    );
    assert_eq!(
        parse_type_string("int<min, 10>"),
        Type::IntRange(None, Some(10))
    );
    assert_eq!(
        parse_type_string("int<0, max>"),
        Type::IntRange(Some(0), None)
    );
    // A malformed bound falls back to plain Int rather than
    // silently treating garbage as an unbounded side.
    assert_eq!(parse_type_string("int<garbage, 10>"), Type::Int);
}

#[test]
fn array_refinement_pseudo_types_are_real_accessory_intersections() {
    // Mirrors PHPStan's own Intersection([Array(_), Accessory(_)])
    // representation.
    assert_eq!(
        parse_type_string("non-empty-array<int>"),
        Type::Intersection(vec![
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
            Type::Accessory(AccessoryKind::NonEmptyArray),
        ])
    );
    assert_eq!(
        // A list's key can only ever be a sequential int, unlike the
        // unknown int|string default plain non-empty-array keeps above.
        parse_type_string("non-empty-list<string>"),
        Type::Intersection(vec![
            Type::Array(Box::new(Type::Int), Box::new(Type::String)),
            Type::Accessory(AccessoryKind::ArrayList),
            Type::Accessory(AccessoryKind::NonEmptyArray),
        ])
    );
    // `class-string-map<K, V>` refines `array<class-string<K>, V>` - the
    // key type is wrapped in ClassString, not discarded.
    assert_eq!(
        parse_type_string("class-string-map<Foo, int>"),
        Type::Array(
            Box::new(Type::ClassString(Box::new(Type::Object(
                "Foo".to_string(),
                Vec::new()
            )))),
            Box::new(Type::Int)
        )
    );
}

#[test]
fn array_shape_parses_required_and_optional_fields() {
    let shape = parse_type_string("array{name: string, age?: int}");
    assert_eq!(
        shape,
        Type::ArrayShape(Shape::from_named(vec![
            ("name".to_string(), Type::String, false),
            ("age".to_string(), Type::Int, true),
        ]))
    );
}

#[test]
fn array_shape_field_can_itself_be_a_union() {
    let shape = parse_type_string("array{value: int|string}");
    assert_eq!(
        shape,
        Type::ArrayShape(Shape::from_named(vec![(
            "value".to_string(),
            Type::Union(vec![Type::Int, Type::String]),
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
        Type::ArrayShape(crate::types::Shape {
            fields: vec![crate::types::ShapeField::new(
                crate::types::ShapeKey::from_name("items"),
                Type::Int,
                false
            )],
            unsealed: Some(Box::new((Type::String, Type::Mixed(false)))),
        })
    );
    // A bare `...` (no generic args) is valid too.
    let shape2 = parse_type_string("array{items: int, ...}");
    assert_eq!(
        shape2,
        Type::ArrayShape(crate::types::Shape::unsealed(vec![
            crate::types::ShapeField::new(
                crate::types::ShapeKey::from_name("items"),
                Type::Int,
                false
            ),
        ]))
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
        Type::ArrayShape(Shape::from_named(vec![
            ("0".to_string(), Type::Int, false),
            ("1".to_string(), Type::String, false),
        ]))
    );
}

#[test]
fn array_shape_with_positional_int_range_fields_parses() {
    let shape = parse_type_string("array{int<0, 255>, int<0, 255>, int<0, 255>}");
    // Each field is a real IntRange, not plain Int.
    assert_eq!(
        shape,
        Type::ArrayShape(Shape::from_named(vec![
            ("0".to_string(), Type::IntRange(Some(0), Some(255)), false),
            ("1".to_string(), Type::IntRange(Some(0), Some(255)), false),
            ("2".to_string(), Type::IntRange(Some(0), Some(255)), false),
        ]))
    );
}

#[test]
fn array_shape_mixing_explicit_int_keys_and_named_keys_still_parses_correctly() {
    let shape = parse_type_string("array{0: int, name: string}");
    assert_eq!(
        shape,
        Type::ArrayShape(Shape::from_named(vec![
            ("0".to_string(), Type::Int, false),
            ("name".to_string(), Type::String, false),
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
        Type::ArrayShape(Shape::from_named(vec![
            ("domain".to_string(), Type::String, false),
            ("plural-forms".to_string(), Type::String, false),
        ]))
    );
}

// A positional field whose own type contains a colon (a callable
// shape's return type) must not be mis-split at that internal colon.
#[test]
fn array_shape_with_a_positional_closure_field_is_not_split_at_its_own_colon() {
    let shape = parse_type_string("array{Closure(int): string, int}");
    let Type::ArrayShape(fields) = &shape else {
        panic!("expected an array shape, got {shape:?}");
    };
    let fields = fields.named_fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0, "0");
    assert_eq!(fields[0].1.to_string(), "Closure(int): string");
    assert!(!fields[0].2);
    assert_eq!(fields[1], ("1".to_string(), Type::Int, false));
}

#[test]
fn a_positional_class_constant_field_is_not_read_as_a_key() {
    let shape = parse_type_string("array{self::STRATEGY_*, non-empty-list<self::STRATEGY_*>}");
    let Type::ArrayShape(fields) = &shape else {
        panic!("expected an array shape, got {shape:?}");
    };
    let fields = fields.named_fields();
    let keys: Vec<&str> = fields.iter().map(|(k, _, _)| k.as_str()).collect();
    assert_eq!(keys, ["0", "1"]);
}
