//! Accessory and refinement pseudo-types (list, class-string, int ranges, array shapes, etc.).
use super::super::*;

#[test]
fn list_is_treated_as_a_real_refinement_not_a_class_reference() {
    // A real, widely-used PHPStan-ecosystem pseudo-type (nikic/php-parser's
    // own `@return list<ClassMethod>` is exactly this shape) - without
    // this, "list" fell through to the generic-user-class branch and
    // got treated as a literal class named "list". Phase 14: now a
    // real Intersection([Array(_), Accessory(ArrayList)]), not a
    // plain Array alias.
    // Phase 50: a list's key can only ever be a sequential int.
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
            Type::Array(Box::new(Type::Int), Box::new(Type::Mixed)),
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
fn interface_string_and_trait_string_are_class_strings_bound_to_an_internal_kind_sentinel() {
    // This project is allowed to check things more strictly than real
    // PHPStan does (real PHPStan gives these no distinguishing semantics
    // from bare `class-string` either) - these route through
    // `Type::ClassString` bound to an internal `$$interface`/`$$trait`
    // sentinel, checked against the real `ClassKind` at
    // `registry::is_assignable`'s own `ClassString` arm.
    assert_eq!(
        parse_type_string("interface-string"),
        Type::ClassString(Box::new(Type::Object("$$interface".to_string(), vec![])))
    );
    assert_eq!(
        parse_type_string("trait-string"),
        Type::ClassString(Box::new(Type::Object("$$trait".to_string(), vec![])))
    );
}

// A top-10-bucket corpus round found `lowercase-string`/`uppercase-
// string`/`non-empty-lowercase-string`/`non-empty-uppercase-string`
// missing from the list above entirely - real PHPStan/Psalm pseudo-
// types (real corpus shape: `symfony/polyfill-php84`'s own `Php84.php`,
// `amphp/hpack`'s own header-name typing) that fell through to the
// generic-user-class branch and got namespace-qualified into a bogus
// "class" (e.g. `Psl\HPACK\non-empty-lowercase-string`), which then
// failed every ordinary `string`-typed assignability check.
#[test]
fn case_refinement_string_pseudo_types_also_stay_plain_string() {
    for name in [
        "lowercase-string",
        "uppercase-string",
        "non-empty-lowercase-string",
        "non-empty-uppercase-string",
    ] {
        assert_eq!(parse_type_string(name), Type::String, "for {name}");
    }
}

// Same corpus round: `int-mask<Flag::*>`/`int-mask-of<Flag::*>` -
// real PHPStan expands these into every combinable bitmask value, which
// this project can't enumerate without resolving the referenced class
// constants. Falling through to the generic-class branch produced a
// bogus `Composer\Pcre\int-mask<Composer\Pcre\PREG_UNMATCHED_AS_NULL>`-
// style "class" that then failed ordinary `int` assignability checks -
// a real corpus false positive. Degrading to the pseudo-type's own real
// underlying kind, `Type::Int`, is a strict improvement.
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
    // Phase 14: real, common PHPStan/Psalm-ecosystem refinement types
    // (real volume seen in a live corpus: 3402 non-empty-string
    // occurrences alone) - each is a literal-provable fact about a
    // string's content, mirroring real PHPStan's own
    // Intersection([StringType, AccessoryXType]) representation.
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

#[test]
fn int_refinement_pseudo_types_are_real_int_ranges() {
    // Phase 14: each is a real IntRange now, mirroring real
    // PHPStan's own standalone IntegerRangeType.
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
    // Phase 14: real Intersection([Array(_), Accessory(_)]) types
    // now, mirroring real PHPStan's own representation.
    assert_eq!(
        parse_type_string("non-empty-array<int>"),
        Type::Intersection(vec![
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
            Type::Accessory(AccessoryKind::NonEmptyArray),
        ])
    );
    assert_eq!(
        // Phase 50: a list's key can only ever be a sequential int - a
        // real precision win, not just arity-matching the unknown
        // int|string default plain non-empty-array keeps above.
        parse_type_string("non-empty-list<string>"),
        Type::Intersection(vec![
            Type::Array(Box::new(Type::Int), Box::new(Type::String)),
            Type::Accessory(AccessoryKind::ArrayList),
            Type::Accessory(AccessoryKind::NonEmptyArray),
        ])
    );
    // Doctrine's own refinement of `array<class-string<TKey>, TValue>`
    // - Phase 50: the key type is real now (wrapped in ClassString, per
    // this pseudo-type's own documented meaning), not discarded.
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
        Type::ArrayShape(vec![
            ("name".to_string(), Type::String, false),
            ("age".to_string(), Type::Int, true),
        ])
    );
}

#[test]
fn array_shape_field_can_itself_be_a_union() {
    let shape = parse_type_string("array{value: int|string}");
    assert_eq!(
        shape,
        Type::ArrayShape(vec![(
            "value".to_string(),
            Type::Union(vec![Type::Int, Type::String]),
            false
        )])
    );
}

#[test]
fn array_shape_with_an_unsealed_marker_still_parses_its_real_fields() {
    // `...<K, V>` (PHPStan/Psalm's "this shape may carry other,
    // unlisted keys too" marker) has no `:` in it - found on real code
    // that this used to make the field loop's `?` propagate a None out
    // of the whole function, silently failing the ENTIRE shape parse
    // (falling through to treating `array{...}` as a literal, then
    // namespace-mangled, class name) over one unmodeled marker,
    // discarding every real field alongside it.
    //
    // Phase 50 follow-up: the marker itself is now recorded as a real
    // sentinel field (`("...".to_string(), Type::Mixed, false)`, see
    // `types::is_unsealed_shape`'s own doc comment) instead of being
    // silently discarded - consumed by `checker::narrowing`'s "never
    // prove an unsealed shape's key is absent" gate and `checker::
    // assignment`'s matching `isset()`/access-existence leniency.
    let shape = parse_type_string("array{items: int, ...<string, mixed>}");
    assert_eq!(
        shape,
        Type::ArrayShape(vec![
            ("items".to_string(), Type::Int, false),
            ("...".to_string(), Type::Mixed, false),
        ])
    );
    // A bare `...` (no generic args) is valid too.
    let shape2 = parse_type_string("array{items: int, ...}");
    assert_eq!(
        shape2,
        Type::ArrayShape(vec![
            ("items".to_string(), Type::Int, false),
            ("...".to_string(), Type::Mixed, false),
        ])
    );
}

// Phase 33 Finding 3: a positional (unkeyed) field - real, valid
// PHPStan/Psalm tuple syntax - used to fail the *entire* shape's parse
// (`split_once(':')?` had nothing to split), falling through to a
// bogus, unparsed class-name `Object`. Numbered the same way
// `infer_array_literal_type` numbers a real array literal's own
// positional elements, so both end up structurally identical.
#[test]
fn array_shape_with_positional_fields_numbers_them_like_a_real_array_literal() {
    let shape = parse_type_string("array{int, string}");
    assert_eq!(
        shape,
        Type::ArrayShape(vec![
            ("0".to_string(), Type::Int, false),
            ("1".to_string(), Type::String, false),
        ])
    );
}

// The real `idcUploadSizedPhoto` shape this finding exists for.
#[test]
fn array_shape_with_positional_int_range_fields_parses() {
    let shape = parse_type_string("array{int<0, 255>, int<0, 255>, int<0, 255>}");
    // Phase 14: each field is a real IntRange now, not plain Int.
    assert_eq!(
        shape,
        Type::ArrayShape(vec![
            ("0".to_string(), Type::IntRange(Some(0), Some(255)), false),
            ("1".to_string(), Type::IntRange(Some(0), Some(255)), false),
            ("2".to_string(), Type::IntRange(Some(0), Some(255)), false),
        ])
    );
}

#[test]
fn array_shape_mixing_explicit_int_keys_and_named_keys_still_parses_correctly() {
    let shape = parse_type_string("array{0: int, name: string}");
    assert_eq!(
        shape,
        Type::ArrayShape(vec![
            ("0".to_string(), Type::Int, false),
            ("name".to_string(), Type::String, false),
        ])
    );
}

// Regression guard: Translator::toDictionaryEntry's real
// @return array{domain: string, plural-forms: string, ...} - found by
// the full corpus re-check after this fix first shipped, since a
// hyphenated bare key was already accepted before any key-shape
// validation existed at all, and `looks_like_shape_key`'s first
// version wrongly rejected it, misclassifying the field as
// positional.
#[test]
fn array_shape_with_a_hyphenated_bare_key_still_parses_as_keyed() {
    let shape = parse_type_string("array{domain: string, plural-forms: string}");
    assert_eq!(
        shape,
        Type::ArrayShape(vec![
            ("domain".to_string(), Type::String, false),
            ("plural-forms".to_string(), Type::String, false),
        ])
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
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0, "0");
    assert_eq!(fields[0].1.to_string(), "callable(int): string");
    assert!(!fields[0].2);
    assert_eq!(fields[1], ("1".to_string(), Type::Int, false));
}
