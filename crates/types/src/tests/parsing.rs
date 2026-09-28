//! Parsing PHPDoc type strings into `Type` values.
use super::super::*;
use super::is_subtype;
use crate::Repr;

#[test]
fn nullable_parses_as_union_with_null() {
    assert_eq!(
        parse_type_string("?string"),
        Type(Repr::Union(vec![Type::string(), Type::null()]))
    );
}

#[test]
fn union_parses_flat() {
    assert_eq!(
        parse_type_string("int|string"),
        Type(Repr::Union(vec![Type::int(), Type::string()]))
    );
}

// `Mixed` already includes every other possible value, so a stray
// `False` (or `Null`) member sitting alongside it fails an ordinary
// argument-compatibility check on its own, even though the whole union
// is vacuously satisfiable.
#[test]
fn a_union_containing_mixed_collapses_to_plain_mixed() {
    assert_eq!(parse_type_string("mixed|false"), Type::mixed());
    assert_eq!(parse_type_string("mixed|null"), Type::mixed());
}

#[test]
fn a_leading_nullable_mark_on_mixed_still_collapses_to_plain_mixed() {
    // A different code path from the pipe-separated `mixed|null` case
    // just above: `?mixed`'s nullable prefix is stripped and re-wrapped
    // around the already-parsed inner type at the very end of
    // `parse_type_string`, after the pipe-union collapse above has
    // already run - unconditionally re-wrapping with `Repr::Null`
    // regardless of what the inner type turned out to be, building
    // `Union([Mixed, Null])` for this one shape with nothing to ever
    // collapse it back down.
    assert_eq!(parse_type_string("?mixed"), Type::mixed());
}

#[test]
fn an_ordinary_union_with_no_mixed_member_is_unaffected() {
    assert_eq!(
        parse_type_string("int|string"),
        Type(Repr::Union(vec![Type::int(), Type::string()]))
    );
}

#[test]
fn quoted_literal_string_union_members_are_string_literals_not_class_names() {
    // Without recognizing the quotes, `'ASC'` falls through to the
    // generic-class branch and becomes a literal class named `'ASC'`
    // (quotes included), then gets nonsensically namespace-qualified
    // once that machinery runs.
    assert_eq!(
        parse_type_string("'ASC'|'DESC'"),
        Type(Repr::Union(vec![
            Type::string_literal("ASC".into()),
            Type::string_literal("DESC".into()),
        ]))
    );
    assert_eq!(
        parse_type_string("\"foo\""),
        Type::string_literal("foo".into())
    );
}

#[test]
fn intersection_parses_flat() {
    assert_eq!(
        parse_type_string("Countable&Iterator"),
        Type(Repr::Intersection(vec![
            Type::object("Countable".into(), Vec::new()),
            Type::object("Iterator".into(), Vec::new())
        ]))
    );
}

#[test]
fn a_leading_backslash_is_preserved_by_the_bare_parser() {
    // parse_type_string alone has no namespace context - it's not this
    // layer's job to decide what an absolute reference resolves to,
    // only to preserve the fact that it *is* absolute for
    // registry::resolve_names_in_type to act on later. Stripping the
    // backslash here would silently mis-qualify every absolute
    // reference as if it were relative - see registry.rs's
    // `a_leading_backslash_in_a_native_type_hint_is_not_re_namespaced`.
    assert_eq!(
        parse_type_string("\\App\\Foo"),
        Type::object(("\\App\\Foo".to_string()).into(), Vec::new())
    );
}

#[test]
fn phpdoc_array_shorthand_parses() {
    assert_eq!(
        parse_type_string("int[]"),
        Type::array(Type::benevolent_array_key(), Type(Repr::Int))
    );
    assert_eq!(
        parse_type_string("string[][]"),
        Type::array(
            Type::benevolent_array_key(),
            Type(Repr::Array(
                Box::new(Type::benevolent_array_key()),
                Box::new(Type(Repr::String))
            ))
        )
    );
}

// Array-shorthand form where the element type itself is a parenthesized
// union/intersection, not a single bare name - must not fall through to
// the generic class-name branch and get nonsensically
// namespace-qualified.
#[test]
fn phpdoc_array_shorthand_with_a_parenthesized_union_element_type_parses() {
    assert_eq!(
        parse_type_string("(string|int)[]"),
        Type::array(
            Type::benevolent_array_key(),
            Type(Repr::Union(vec![Type(Repr::String), Type(Repr::Int)]))
        )
    );
}

#[test]
fn phpdoc_array_shorthand_with_a_parenthesized_intersection_element_type_parses() {
    assert_eq!(
        parse_type_string("(Foo&Bar)[]"),
        Type::array(
            Type::benevolent_array_key(),
            Type(Repr::Intersection(vec![
                Type(Repr::Object("Foo".into(), Vec::new())),
                Type(Repr::Object("Bar".into(), Vec::new()))
            ]))
        )
    );
}

// `try_parse_array_shape`/`try_parse_object_shape` must be tried against
// more than just the original, un-stripped string - a shape wrapped in
// an array-shorthand suffix never ends with the shape's own closing `}`
// at that point (it ends with `[]`), so it would fall straight through
// to the generic class-name branch and mangle the shape's literal text
// into a bogus, namespace-qualified "class name".
#[test]
fn a_positional_array_shape_wrapped_in_array_shorthand_parses() {
    assert_eq!(
        parse_type_string("array{int, string, string}[]"),
        Type::array(
            Type::benevolent_array_key(),
            Type(Repr::ArrayShape(Shape::from_named(vec![
                ("0".to_string(), Type(Repr::Int), false),
                ("1".to_string(), Type(Repr::String), false),
                ("2".to_string(), Type(Repr::String), false),
            ])))
        )
    );
}

// `canonical_int_key` implements PHP's array-key canonicalization rule -
// a negative key canonicalizes to a real int key, but a leading-zero
// string does not (PHP keeps `"01"` as a string key), unlike a bare
// `str::parse::<i64>()` (`"01".parse::<i64>() == Ok(1)` - Rust's parser
// is lenient about leading zeros/`"-0"` in a way PHP's canonicalization
// isn't).

#[test]
fn canonical_int_key_accepts_zero_and_ordinary_positive_and_negative_integers() {
    assert_eq!(canonical_int_key("0"), Some(0));
    assert_eq!(canonical_int_key("5"), Some(5));
    assert_eq!(canonical_int_key("-5"), Some(-5));
    assert_eq!(canonical_int_key("123456789"), Some(123456789));
}

#[test]
fn canonical_int_key_rejects_leading_zeros_and_negative_zero() {
    assert_eq!(canonical_int_key("01"), None);
    assert_eq!(canonical_int_key("00"), None);
    assert_eq!(canonical_int_key("-0"), None);
    assert_eq!(canonical_int_key("-01"), None);
}

#[test]
fn canonical_int_key_rejects_non_numeric_and_out_of_range_strings() {
    assert_eq!(canonical_int_key(""), None);
    assert_eq!(canonical_int_key("foo"), None);
    assert_eq!(canonical_int_key("1.5"), None);
    assert_eq!(canonical_int_key("+1"), None);
    // Overflows i64 - stays a string key, matching real PHP on a 64-bit
    // build (the value doesn't fit the platform integer range).
    assert_eq!(canonical_int_key("99999999999999999999"), None);
}

#[test]
fn a_list_shape_parses_identically_to_the_array_shape_spelling() {
    // Real shape: `Psl\Type\Internal\MutableMapType`'s own
    // `list<list{Tk, Tv}>` - the bare, unnamed-tuple `list{...}` spelling
    // real PHPStan's own `phpdoc-parser` recognizes
    // (`ArrayShapeNode::KIND_LIST`) alongside `array{...}`, previously
    // unhandled here and silently mangled into a bogus, namespace-
    // qualified "class name" instead.
    assert_eq!(
        parse_type_string("list{int, string}"),
        Type::array_shape(Shape::from_named(vec![
            ("0".to_string(), Type(Repr::Int), false),
            ("1".to_string(), Type(Repr::String), false),
        ]))
    );
}

#[test]
fn a_non_empty_array_shape_and_a_non_empty_list_shape_both_parse() {
    assert_eq!(
        parse_type_string("non-empty-array{a: int}"),
        Type::array_shape(Shape::from_named(vec![(
            "a".to_string(),
            Type(Repr::Int),
            false
        )]))
    );
    assert_eq!(
        parse_type_string("non-empty-list{int}"),
        Type::array_shape(Shape::from_named(vec![(
            "0".to_string(),
            Type(Repr::Int),
            false
        )]))
    );
}

#[test]
fn a_class_name_merely_starting_with_list_is_not_mistaken_for_a_list_shape() {
    assert_eq!(
        parse_type_string("ListNode"),
        Type::object("ListNode".into(), Vec::new())
    );
}

#[test]
fn a_negative_shape_key_parses_as_a_real_int_literal() {
    assert_eq!(
        parse_type_string("array{-5: int}"),
        Type::array_shape(Shape::from_named(vec![(
            "-5".to_string(),
            Type(Repr::Int),
            false
        )]))
    );
}

#[test]
fn a_leading_zero_shape_key_stays_a_string_literal_not_an_int() {
    assert_eq!(
        parse_type_string("array{'01': int}"),
        Type::array_shape(Shape::from_named(vec![(
            "01".to_string(),
            Type(Repr::Int),
            false
        )]))
    );
    // The key itself isn't the assertion - `shape_key_literal` (the
    // Repr::IntLiteral/StringLiteral choice `value-of<T>`/`key-of<T>` and
    // key-access narrowing consume) is; confirmed via the dedicated
    // `canonical_int_key` unit tests below instead, since `ArrayShape`
    // itself only ever stores the raw key string, not which literal kind
    // it canonicalizes to.
}

#[test]
fn an_explicit_zero_key_collides_with_and_overwrites_an_earlier_positional_field() {
    // `try_parse_array_shape` must merge two shape fields whose keys
    // canonicalize to the same value, not silently produce two separate
    // fields both keyed "0".
    assert_eq!(
        parse_type_string("array{int, 0: string}"),
        Type::array_shape(Shape::from_named(vec![(
            "0".to_string(),
            Type(Repr::String),
            false
        )]))
    );
}

#[test]
fn a_literal_duplicate_shape_key_is_merged_last_one_wins() {
    assert_eq!(
        parse_type_string("array{foo: int, foo: string}"),
        Type::array_shape(Shape::from_named(vec![(
            "foo".to_string(),
            Type(Repr::String),
            false
        )]))
    );
}

#[test]
fn a_keyed_array_shape_wrapped_in_array_shorthand_parses() {
    assert_eq!(
        parse_type_string("array{a: int}[]"),
        Type::array(
            Type::benevolent_array_key(),
            Type(Repr::ArrayShape(Shape::from_named(vec![(
                "a".to_string(),
                Type(Repr::Int),
                false
            )])))
        )
    );
}

#[test]
fn an_object_shape_wrapped_in_array_shorthand_parses() {
    assert_eq!(
        parse_type_string("object{a: int}[]"),
        Type::array(
            Type::benevolent_array_key(),
            Type(Repr::ObjectShape(Shape::from_named(vec![(
                "a".to_string(),
                Type(Repr::Int),
                false
            )])))
        )
    );
}

// `OffsetAccessType` - `SomeShape['key']` extracts one field's value
// type.
#[test]
fn offset_access_type_on_an_array_shape_extracts_the_field_type() {
    assert_eq!(
        parse_type_string("array{name: string, age: int}['age']"),
        Type::int()
    );
}

#[test]
fn offset_access_type_accepts_a_bare_unquoted_key() {
    assert_eq!(
        parse_type_string("array{0: string, 1: int}[1]"),
        Type::int()
    );
}

#[test]
fn offset_access_type_on_a_missing_key_degrades_to_mixed() {
    assert_eq!(
        parse_type_string("array{name: string}['missing']"),
        Type::mixed()
    );
}

#[test]
fn offset_access_type_on_a_plain_array_uses_its_uniform_value_type() {
    assert_eq!(
        parse_type_string("array<string, int>['anything']"),
        Type::int()
    );
}

#[test]
fn an_empty_bracket_after_offset_access_still_parses_as_array_shorthand() {
    // `SomeShape['key'][]` - the outer `[]` is the ordinary array-
    // shorthand suffix, not another offset access (its key part would
    // be empty).
    assert_eq!(
        parse_type_string("array{age: int}['age'][]"),
        Type::array(Type::benevolent_array_key(), Type(Repr::Int))
    );
}

#[test]
fn offset_access_type_recurses_through_a_union() {
    assert_eq!(
        parse_type_string("(array{a: int}|array{a: string})['a']"),
        Type(Repr::Union(vec![Type::int(), Type::string()]))
    );
}

#[test]
fn dollar_this_phpdoc_pseudo_type_is_treated_as_static() {
    // `@return $this` is a real, documented PHPStan/Psalm convention
    // (real example: amphp/amp's Interval::disable() and others use
    // exactly this) - identical in meaning to `@return static`, not a
    // variable reference. Without this it fell through to the
    // generic-user-class branch and became a literal class named
    // "$this", then got nonsensically namespace-qualified once that
    // existed (`Amp\$this`).
    assert_eq!(parse_type_string("$this"), Type::this());
}

#[test]
fn class_constant_reference_types_parse_to_a_class_constant() {
    // `self::RFC_*` / `Foo::BAR` (PHPStan's constant-type convention,
    // real example: monolog/monolog's own `@param self::RFC_* $rfc`)
    // look like a class name syntactically but aren't one. They parse to
    // a `Repr::ClassConstant` carrying the class reference and the name
    // pattern, resolved against real constants once the registry exists
    // (`registry::resolve`'s `resolve_class_const_type`) - never a
    // literal class named "self::RFC_*".
    let sentinel = |class: Type, pattern: &str| {
        Type(Repr::ClassConstant {
            class: Box::new(class),
            pattern: pattern.to_string(),
        })
    };
    assert_eq!(
        parse_type_string("self::RFC_*"),
        sentinel(Type::self_ref(Vec::new()), "RFC_*")
    );
    assert_eq!(
        parse_type_string("Foo::BAR"),
        sentinel(Type::object("Foo".into(), vec![]), "BAR")
    );
    // `Foo::class` is a class-string, not a constant reference.
    assert_eq!(
        parse_type_string("Foo::class"),
        Type::class_string(Type(Repr::Object("Foo".into(), vec![])))
    );
    // The array-wrapping machinery still applies around it.
    assert_eq!(
        parse_type_string("self::RFC_*[]"),
        Type::array(
            Type::benevolent_array_key(),
            sentinel(Type(Repr::SelfRef(Vec::new())), "RFC_*")
        )
    );
}

#[test]
fn generic_array_with_one_type_arg_parses_as_value_type() {
    // `array<V>` (one generic arg) builds a real `BenevolentUnionType`
    // key, exactly like the `V[]`-shorthand suffix - real PHPStan's own
    // `resolveGenericTypeNode` uses the identical construction for both
    // (confirmed reading it directly), unlike bare `array` (no generic
    // args at all) or `array<K, V>` (2 args, explicit key), neither of
    // which get the benevolent marker.
    assert_eq!(
        parse_type_string("array<int>"),
        Type::array(Type::benevolent_array_key(), Type(Repr::Int))
    );
}

#[test]
fn generic_array_with_key_and_value_uses_both() {
    // The key argument is a real part of the resulting Repr::Array, not
    // silently discarded.
    assert_eq!(
        parse_type_string("array<string, int>"),
        Type::array(Type(Repr::String), Type(Repr::Int))
    );
}

#[test]
fn generic_user_class_keeps_base_name_and_type_args() {
    assert_eq!(
        parse_type_string("Collection<User>"),
        Type::object(
            "Collection".into(),
            vec![Type(Repr::Object("User".into(), Vec::new()))]
        )
    );
}

// PHPStan's call-site variance annotation syntax (`covariant Foo`/
// `contravariant Foo`/`*`) inside a generic argument list. Direct
// parser-level assertions, independent of any downstream consumer, since
// checker-level tests exercising this (`checker::tests::
// generics_and_templates`) can pass "by accident" if parsing regresses
// to treating the whole annotated argument as one bogus, unrelated class
// name: `Collection<contravariant T>` degrading to a literal class named
// `"contravariant T"` would still produce zero diagnostics downstream,
// for an entirely different (and wrong) reason.

#[test]
fn generic_arg_with_covariant_marker_parses_to_a_variance_arg() {
    assert_eq!(
        parse_type_string("Collection<covariant Foo>"),
        Type::object(
            "Collection".into(),
            vec![Type(Repr::VarianceArg(
                CallSiteVariance::Covariant,
                Box::new(Type(Repr::Object("Foo".into(), Vec::new())))
            ))]
        )
    );
}

#[test]
fn generic_arg_with_contravariant_marker_parses_to_a_variance_arg() {
    assert_eq!(
        parse_type_string("Collection<contravariant Foo>"),
        Type::object(
            "Collection".into(),
            vec![Type(Repr::VarianceArg(
                CallSiteVariance::Contravariant,
                Box::new(Type(Repr::Object("Foo".into(), Vec::new())))
            ))]
        )
    );
}

#[test]
fn generic_arg_with_bivariant_marker_parses_to_a_variance_arg() {
    assert_eq!(
        parse_type_string("Collection<*>"),
        Type::object(
            "Collection".into(),
            vec![Type(Repr::VarianceArg(
                CallSiteVariance::Bivariant,
                Box::new(Type(Repr::Mixed(false)))
            ))]
        )
    );
}

#[test]
fn a_call_site_variance_marker_survives_nested_inside_the_wrapped_type() {
    // The wrapped type itself can be an arbitrary nested generic, not
    // just a bare class reference - `parse_generic_arg` recurses through
    // the ordinary `parse_type_string` pipeline for it.
    assert_eq!(
        parse_type_string("Collection<covariant Box<int>>"),
        Type::object(
            "Collection".into(),
            vec![Type(Repr::VarianceArg(
                CallSiteVariance::Covariant,
                Box::new(Type(Repr::Object("Box".into(), vec![Type(Repr::Int)])))
            ))]
        )
    );
}

#[test]
fn union_inside_generic_args_is_not_a_top_level_union() {
    // Without depth-aware splitting, the `|` inside `<>` would be
    // wrongly treated as splitting the whole string.
    assert_eq!(
        parse_type_string("Collection<int|string>"),
        Type::object(
            "Collection".into(),
            vec![Type(Repr::Union(vec![Type(Repr::Int), Type(Repr::String)]))]
        )
    );
}

#[test]
fn top_level_union_around_a_generic_still_splits_correctly() {
    assert_eq!(
        parse_type_string("Collection<int>|null"),
        Type(Repr::Union(vec![
            Type::object("Collection".into(), vec![Type(Repr::Int)]),
            Type::null()
        ]))
    );
}

#[test]
fn dnf_type_group_resolves_to_a_real_intersection_inside_the_union() {
    let ty = parse_type_string("(Countable&Iterator)|null");
    let Type(Repr::Union(members)) = &ty else {
        panic!("expected a top-level union");
    };
    assert_eq!(members.len(), 2);
    assert!(matches!(&members[0], Type(Repr::Intersection(parts)) if parts.len() == 2));
    assert_eq!(members[1], Type::null());
}

// PHPStan's conditional return type syntax (`@return ($action is 'send'
// ? list<string> : list<UserMailNotification>)`) must not fall into the
// DNF-group unwrap above and get mangled into a nonsense,
// namespace-qualified "class name". A bare `$param` subject parses into
// a lazily-resolved `Repr::Conditional` instead of eagerly unioning both
// branches at parse time - `resolve_conditional` (wired into
// `Checker::infer_call_return_type`) picks the real branch per call site
// once the bound argument type is known.
#[test]
fn conditional_return_type_syntax_parses_into_a_real_conditional_type() {
    let ty = parse_type_string("($action is 'send' ? list<string> : list<int>)");
    let Type(Repr::Conditional {
        subject,
        target,
        if_true,
        if_false,
        negated,
        template_subject,
    }) = &ty
    else {
        panic!("expected a real Conditional type, got {ty:?}");
    };
    assert!(!template_subject);
    assert_eq!(subject, "action");
    assert_eq!(**target, Type::string_literal("send".into()));
    assert_eq!(**if_true, parse_type_string("list<string>"));
    assert_eq!(**if_false, parse_type_string("list<int>"));
    assert!(!negated);
}

#[test]
fn conditional_return_type_syntax_with_is_not_sets_negated() {
    let ty = parse_type_string("($action is not 'send' ? list<string> : list<int>)");
    let Type(Repr::Conditional {
        subject, negated, ..
    }) = &ty
    else {
        panic!("expected a real Conditional type, got {ty:?}");
    };
    assert_eq!(subject, "action");
    assert!(*negated);
}

// PHPStan also allows a non-parameter type-expression subject (e.g. a
// template name) - not modeled here, so this falls back to the eager
// "union both branches" leniency rather than guessing at a shape it
// can't represent.
#[test]
fn conditional_return_type_syntax_with_a_template_subject_keeps_its_condition() {
    let ty = parse_type_string("(T is 'send' ? list<string> : list<int>)");
    let Type(Repr::Conditional {
        subject,
        template_subject,
        ..
    }) = &ty
    else {
        panic!("expected a template-subject conditional, got {ty:?}");
    };
    assert_eq!(subject, "T");
    assert!(*template_subject);
}

// A docblock-parsed `list<int>` and one built at runtime are the same
// type, keyed on `int<0, max>` either way.
#[test]
fn a_docblock_parsed_list_is_a_mutual_subtype_of_the_runtime_inferred_shape() {
    let docblock_list = parse_type_string("list<int>");
    let runtime_list = Type::list_of(Type::int());
    assert_eq!(docblock_list, runtime_list);
    assert!(is_subtype(&runtime_list, &docblock_list));
    assert!(is_subtype(&docblock_list, &runtime_list));
}

#[test]
fn a_docblock_parsed_non_empty_list_is_a_mutual_subtype_of_the_runtime_inferred_shape() {
    let docblock_list = parse_type_string("non-empty-list<int>");
    let runtime_list = Type::intersection_of(vec![
        Type::list_of(Type::int()),
        Type::accessory(AccessoryKind::NonEmptyArray),
    ]);
    assert_eq!(docblock_list, runtime_list);
    assert!(is_subtype(&runtime_list, &docblock_list));
    assert!(is_subtype(&docblock_list, &runtime_list));
}

// The `NonEmptyArray` half of the same rule: a plain (possibly-empty)
// runtime `list<int>` must NOT satisfy a docblock-declared
// `non-empty-list<int>` - only the element-type comparison is skipped
// for keys, not the non-emptiness guarantee itself.
#[test]
fn a_plain_list_is_not_a_subtype_of_a_non_empty_list_of_the_same_element_type() {
    let docblock_non_empty = parse_type_string("non-empty-list<int>");
    let runtime_plain_list = Type::list_of(Type::int());
    assert!(!is_subtype(&runtime_plain_list, &docblock_non_empty));
}

// A `Closure(self):T`-shaped callable-shape doc member must not fall
// through to the generic class-name branch and get mangled into a
// bogus, namespace-qualified "class name" - it parses into a real,
// shaped `Repr::Callable(Some(sig))` instead of degrading to the bare
// marker. `self`/`T` stay unresolved `Object` references here (only
// `parse_type_string` itself runs in this test, not the later
// name-resolution/template-marking passes), but the structural shape
// itself is real.
#[test]
fn closure_shape_syntax_parses_into_a_real_shape() {
    assert_eq!(
        parse_type_string("Closure(self):T").to_string(),
        "Closure(self): T"
    );
    assert_eq!(
        parse_type_string("Closure(int, string): bool").to_string(),
        "Closure(int, string): bool"
    );
    assert_eq!(
        parse_type_string("callable(int): void").to_string(),
        "callable(int): void"
    );
}

#[test]
fn closure_shape_syntax_still_works_inside_a_union() {
    let ty = parse_type_string("Closure(self):T|null");
    let Type(Repr::Union(members)) = &ty else {
        panic!("expected a top-level union, got {ty:?}");
    };
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].to_string(), "Closure(self): T");
    assert_eq!(members[1], Type::null());
}

#[test]
fn a_class_name_merely_starting_with_closure_is_not_mistaken_for_the_callable_shape() {
    // `strip_prefix("closure")` alone would wrongly match a real
    // class like `ClosureFactory` too - only a literal `(` right
    // after `closure`/`callable` (ignoring case) counts.
    assert_eq!(
        parse_type_string("ClosureFactory"),
        Type::object("ClosureFactory".into(), Vec::new())
    );
}

// A fully-qualified `\Closure(CancelledException)` (leading `\`) must
// not defeat the callable-shape check (`"\closure(...".starts_with(
// "closure(")` is false), falling through to the generic-class branch
// and mangling into a bogus `Repr::Object("Closure(CancelledException)",
// [])` "class".
//
// `\Closure(CancelledException)` itself has no trailing `: ReturnType` -
// a mandatory piece of the grammar - so it still degrades to the bare
// marker; `\Closure(int, string): bool` does have one, so it parses into
// a real shape instead.
#[test]
fn fully_qualified_closure_shape_syntax_with_no_return_type_degrades_to_plain_callable() {
    assert_eq!(
        parse_type_string("\\Closure(CancelledException)"),
        Type::callable(None)
    );
}

#[test]
fn fully_qualified_closure_shape_syntax_with_a_return_type_parses_into_a_real_shape() {
    assert_eq!(
        parse_type_string("\\Closure(int, string): bool").to_string(),
        "Closure(int, string): bool"
    );
}

// A bracket-shaped character *inside* one quoted union member
// (`'->'|'::'`, the `>` in `'->'`) must not be read as a real closing
// bracket by `split_top_level`, or it corrupts the depth counter and
// swallows the `|` separator for the rest of the string, mangling the
// whole thing into a single nonsensical `Repr::StringLiteral`.
#[test]
fn a_quoted_literal_containing_a_bracket_shaped_character_does_not_corrupt_the_union_split() {
    assert_eq!(
        parse_type_string("'->'|'::'"),
        Type(Repr::Union(vec![
            Type::string_literal("->".into()),
            Type::string_literal("::".into()),
        ]))
    );
}

#[test]
fn a_quoted_literal_containing_a_bracket_shaped_character_inside_an_array_shape() {
    assert_eq!(
        parse_type_string("array{type?: '->'|'::'}"),
        Type::array_shape(Shape::from_named(vec![(
            "type".to_string(),
            Type(Repr::Union(vec![
                Type(Repr::StringLiteral("->".into())),
                Type(Repr::StringLiteral("::".into())),
            ])),
            true,
        )]))
    );
}
