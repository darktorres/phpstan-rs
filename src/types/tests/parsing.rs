//! Parsing PHPDoc type strings into `Type` values.
use super::super::*;

#[test]
fn nullable_parses_as_union_with_null() {
    assert_eq!(
        parse_type_string("?string"),
        Type::Union(vec![Type::String, Type::Null])
    );
}

#[test]
fn union_parses_flat() {
    assert_eq!(
        parse_type_string("int|string"),
        Type::Union(vec![Type::Int, Type::String])
    );
}

// Phase 32 Finding H, real shape: `end()`'s real stub is `function
// end(object|array &$array): mixed {}` with a never-updated
// `@return mixed|false` doc comment - `Mixed` already includes every
// other possible value, so a stray `False` (or `Null`) member sitting
// alongside it fails an ordinary argument-compatibility check on its
// own, even though the whole union is vacuously satisfiable.
#[test]
fn a_union_containing_mixed_collapses_to_plain_mixed() {
    assert_eq!(parse_type_string("mixed|false"), Type::Mixed);
    assert_eq!(parse_type_string("mixed|null"), Type::Mixed);
}

#[test]
fn an_ordinary_union_with_no_mixed_member_is_unaffected() {
    assert_eq!(
        parse_type_string("int|string"),
        Type::Union(vec![Type::Int, Type::String])
    );
}

#[test]
fn quoted_literal_string_union_members_are_string_literals_not_class_names() {
    // Real, previously-live bug found via real code in this
    // project's own verification corpus (`@param 'ASC'|'DESC' $dir`,
    // a real, documented PHPStan/Psalm literal-string-union
    // convention): without recognizing the quotes, `'ASC'` fell
    // through to the generic-class branch and became a literal
    // class named `'ASC'` (quotes included), then got nonsensically
    // namespace-qualified once that machinery ran (`Piwigo\Db\'ASC'`).
    assert_eq!(
        parse_type_string("'ASC'|'DESC'"),
        Type::Union(vec![
            Type::StringLiteral("ASC".to_string()),
            Type::StringLiteral("DESC".to_string()),
        ])
    );
    assert_eq!(
        parse_type_string("\"foo\""),
        Type::StringLiteral("foo".to_string())
    );
}

#[test]
fn intersection_parses_flat() {
    assert_eq!(
        parse_type_string("Countable&Iterator"),
        Type::Intersection(vec![
            Type::Object("Countable".to_string(), Vec::new()),
            Type::Object("Iterator".to_string(), Vec::new())
        ])
    );
}

#[test]
fn a_leading_backslash_is_preserved_by_the_bare_parser() {
    // parse_type_string alone has no namespace context - it's not
    // this layer's job to decide what an absolute reference resolves
    // to, only to preserve the fact that it *is* absolute for
    // registry::resolve_names_in_type to act on later. Stripping the
    // backslash here (the old, buggy behavior this test used to
    // assert) permanently lost that distinction before namespace
    // resolution ever ran, silently mis-qualifying every absolute
    // reference as if it were relative - see
    // registry.rs's own `a_leading_backslash_in_a_native_type_hint_is_not_re_namespaced`
    // for the real, end-to-end regression this caused.
    assert_eq!(
        parse_type_string("\\App\\Foo"),
        Type::Object("\\App\\Foo".to_string(), Vec::new())
    );
}

#[test]
fn phpdoc_array_shorthand_parses() {
    assert_eq!(
        parse_type_string("int[]"),
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int))
    );
    assert_eq!(
        parse_type_string("string[][]"),
        Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(Type::String)
            ))
        )
    );
}

// Phase 53: a real corpus check (guzzlehttp/psr7's own `@param
// (string|int)[] $keys`) found this array-shorthand form - the element
// type itself is a parenthesized union/intersection, not a single bare
// name - fell through to the generic class-name branch and got
// nonsensically namespace-qualified, the same bug class `$this`/
// `self::RFC_*` above already had their own dedicated fixes for.
#[test]
fn phpdoc_array_shorthand_with_a_parenthesized_union_element_type_parses() {
    assert_eq!(
        parse_type_string("(string|int)[]"),
        Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type::Union(vec![Type::String, Type::Int]))
        )
    );
}

#[test]
fn phpdoc_array_shorthand_with_a_parenthesized_intersection_element_type_parses() {
    assert_eq!(
        parse_type_string("(Foo&Bar)[]"),
        Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type::Intersection(vec![
                Type::Object("Foo".to_string(), Vec::new()),
                Type::Object("Bar".to_string(), Vec::new())
            ]))
        )
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
    assert_eq!(parse_type_string("$this"), Type::StaticRef);
}

#[test]
fn class_constant_reference_types_degrade_to_mixed() {
    // `self::RFC_*` / `Foo::BAR` (PHPStan's wildcard-constant
    // convention, real example: monolog/monolog's own
    // `@param self::RFC_* $rfc`) look like a class name syntactically
    // but aren't one - resolving them for real would mean looking up
    // the referenced constant('s value), which this project doesn't
    // model. Without this check they fell through to the
    // generic-class branch and became a literal class literally
    // named "self::RFC_*", then got nonsensically namespace-qualified
    // once that existed (`Monolog\Handler\self::RFC*`).
    assert_eq!(parse_type_string("self::RFC_*"), Type::Mixed);
    assert_eq!(parse_type_string("Foo::BAR"), Type::Mixed);
    // The array-wrapping/nullable machinery around parse_type_atom
    // still has to apply correctly around this degraded case.
    assert_eq!(
        parse_type_string("self::RFC_*[]"),
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed))
    );
}

#[test]
fn generic_array_with_one_type_arg_parses_as_value_type() {
    assert_eq!(
        parse_type_string("array<int>"),
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int))
    );
}

#[test]
fn generic_array_with_key_and_value_uses_both() {
    // Phase 50: the key argument used to be silently discarded - now
    // it's a real part of the resulting Type::Array.
    assert_eq!(
        parse_type_string("array<string, int>"),
        Type::Array(Box::new(Type::String), Box::new(Type::Int))
    );
}

#[test]
fn generic_user_class_keeps_base_name_and_type_args() {
    assert_eq!(
        parse_type_string("Collection<User>"),
        Type::Object(
            "Collection".to_string(),
            vec![Type::Object("User".to_string(), Vec::new())]
        )
    );
}

// Phase 54: real PHPStan's own call-site variance annotation syntax
// (`covariant Foo`/`contravariant Foo`/`*`) inside a generic argument
// list - confirmed real and current against `phpdoc-parser`'s own
// `GenericTypeNode` in `vendor/phpstan-src`. Direct parser-level
// assertions, independent of any downstream consumer, since the
// checker-level tests exercising this (`checker::tests::
// generics_and_templates`) can pass "by accident" if parsing regresses
// to treating the whole annotated argument as one bogus, unrelated
// class name - a real gap this test file's own revert-verification
// found: `Collection<contravariant T>` degrading to a literal class
// named `"contravariant T"` still produced zero diagnostics downstream,
// for an entirely different (and wrong) reason.

#[test]
fn generic_arg_with_covariant_marker_parses_to_the_sentinel() {
    assert_eq!(
        parse_type_string("Collection<covariant Foo>"),
        Type::Object(
            "Collection".to_string(),
            vec![Type::Object(
                "$$covariant".to_string(),
                vec![Type::Object("Foo".to_string(), Vec::new())]
            )]
        )
    );
}

#[test]
fn generic_arg_with_contravariant_marker_parses_to_the_sentinel() {
    assert_eq!(
        parse_type_string("Collection<contravariant Foo>"),
        Type::Object(
            "Collection".to_string(),
            vec![Type::Object(
                "$$contravariant".to_string(),
                vec![Type::Object("Foo".to_string(), Vec::new())]
            )]
        )
    );
}

#[test]
fn generic_arg_with_bivariant_marker_parses_to_the_sentinel() {
    assert_eq!(
        parse_type_string("Collection<*>"),
        Type::Object(
            "Collection".to_string(),
            vec![Type::Object("$$bivariant".to_string(), Vec::new())]
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
        Type::Object(
            "Collection".to_string(),
            vec![Type::Object(
                "$$covariant".to_string(),
                vec![Type::Object("Box".to_string(), vec![Type::Int])]
            )]
        )
    );
}

#[test]
fn union_inside_generic_args_is_not_a_top_level_union() {
    // Without depth-aware splitting, the `|` inside `<>` would be
    // wrongly treated as splitting the whole string.
    assert_eq!(
        parse_type_string("Collection<int|string>"),
        Type::Object(
            "Collection".to_string(),
            vec![Type::Union(vec![Type::Int, Type::String])]
        )
    );
}

#[test]
fn top_level_union_around_a_generic_still_splits_correctly() {
    assert_eq!(
        parse_type_string("Collection<int>|null"),
        Type::Union(vec![
            Type::Object("Collection".to_string(), vec![Type::Int]),
            Type::Null
        ])
    );
}

#[test]
fn dnf_type_group_resolves_to_a_real_intersection_inside_the_union() {
    let ty = parse_type_string("(Countable&Iterator)|null");
    let Type::Union(members) = ty else {
        panic!("expected a top-level union");
    };
    assert_eq!(members.len(), 2);
    assert!(matches!(&members[0], Type::Intersection(parts) if parts.len() == 2));
    assert_eq!(members[1], Type::Null);
}

// A round of Piwigo-corpus bucketing found real PHPStan's own
// conditional return type syntax (`@return ($action is 'send' ?
// list<string> : list<UserMailNotification>)`,
// NotificationByMailSender's own real shape) fell into the DNF-group
// unwrap right above and got mangled into a nonsense, namespace-
// qualified "class name" - this project can't evaluate the
// condition per call site, so it takes the union of both branches
// instead, matching this project's usual "can't evaluate, don't
// guess which one" leniency.
#[test]
fn conditional_return_type_syntax_unions_both_branches() {
    let ty = parse_type_string("($action is 'send' ? list<string> : list<int>)");
    let Type::Union(members) = ty else {
        panic!("expected a union of both branches, got {ty:?}");
    };
    assert_eq!(members.len(), 2);
}

#[test]
fn conditional_return_type_syntax_collapses_identical_branches() {
    let ty = parse_type_string("($action is 'send' ? list<string> : list<string>)");
    assert_eq!(ty, parse_type_string("list<string>"));
}

// A docblock-parsed `list<int>` builds its key as a literal `Type::Int`
// (needed so a later marker-stripping op, e.g. `array_diff()`'s own
// result, still shows a known `array<int, T>` rather than collapsing to
// a keyless `array<T>` - verified via dumpType()), while every
// runtime-inferred list this project builds (`array_values()`, `sort()`,
// ...) uses the generic `unknown_array_key()` sentinel for that same
// position instead (deliberately - a list's key is always int *by
// definition*, so `upgrade_array_to_list`'s own doc comment treats
// encoding it twice as redundant). Both printed identically as
// `list<int>` (`describe_accessory_intersection` hides a list's key
// unconditionally) but weren't structurally `==`-equal, and worse,
// `is_subtype`'s own generic `Array` key check is directionally
// asymmetric between the two representations
// (`is_subtype(unknown_array_key(), Int)` is false, the reverse true),
// so a genuine runtime `list<int>` failed to satisfy an otherwise-
// identical docblock-declared `list<int>` parameter/return type.
// Corpus-verified via `SearchService::getRegularSearchResults()`
// printing a real return-type mismatch as `list<int>|list<int>|array|
// array{}` - the two `list<int>` members were exactly this
// docblock-vs-runtime pair, which `union_types`'s own dedup (via
// `types_equal_for_union`) now also collapses into one.
#[test]
fn a_docblock_parsed_list_is_a_mutual_subtype_of_the_runtime_inferred_shape() {
    let docblock_list = parse_type_string("list<int>");
    let runtime_list = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
        Type::Accessory(AccessoryKind::ArrayList),
    ]);
    assert_ne!(
        docblock_list, runtime_list,
        "still two distinct representations"
    );
    assert!(is_subtype(&runtime_list, &docblock_list));
    assert!(is_subtype(&docblock_list, &runtime_list));
    assert!(types_equal_for_union(&docblock_list, &runtime_list));
}

#[test]
fn a_docblock_parsed_non_empty_list_is_a_mutual_subtype_of_the_runtime_inferred_shape() {
    let docblock_list = parse_type_string("non-empty-list<int>");
    let runtime_list = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
        Type::Accessory(AccessoryKind::ArrayList),
        Type::Accessory(AccessoryKind::NonEmptyArray),
    ]);
    assert!(is_subtype(&runtime_list, &docblock_list));
    assert!(is_subtype(&docblock_list, &runtime_list));
    assert!(types_equal_for_union(&docblock_list, &runtime_list));
}

// The `NonEmptyArray` half of the same rule: a plain (possibly-empty)
// runtime `list<int>` must NOT satisfy a docblock-declared
// `non-empty-list<int>` - only the element-type comparison is skipped
// for keys, not the non-emptiness guarantee itself.
#[test]
fn a_plain_list_is_not_a_subtype_of_a_non_empty_list_of_the_same_element_type() {
    let docblock_non_empty = parse_type_string("non-empty-list<int>");
    let runtime_plain_list = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
        Type::Accessory(AccessoryKind::ArrayList),
    ]);
    assert!(!is_subtype(&runtime_plain_list, &docblock_non_empty));
}

// Phase 30, real shape: Doctrine DBAL's own `@param Closure(self):T
// $func` (`Connection::transactional()`) - the callable-shape syntax
// wasn't recognized at all, so the whole literal string
// `"Closure(self):T"` fell through to the generic class-name branch
// and got mangled into a bogus, namespace-qualified "class name"
// once resolved against the current file's own namespace.
#[test]
fn closure_shape_syntax_degrades_to_plain_callable() {
    assert_eq!(parse_type_string("Closure(self):T"), Type::Callable);
    assert_eq!(
        parse_type_string("Closure(int, string): bool"),
        Type::Callable
    );
    assert_eq!(parse_type_string("callable(int): void"), Type::Callable);
}

#[test]
fn closure_shape_syntax_still_works_inside_a_union() {
    let ty = parse_type_string("Closure(self):T|null");
    let Type::Union(members) = ty else {
        panic!("expected a top-level union, got {ty:?}");
    };
    assert_eq!(members, vec![Type::Callable, Type::Null]);
}

#[test]
fn a_class_name_merely_starting_with_closure_is_not_mistaken_for_the_callable_shape() {
    // `strip_prefix("closure")` alone would wrongly match a real
    // class like `ClosureFactory` too - only a literal `(` right
    // after `closure`/`callable` (ignoring case) counts.
    assert_eq!(
        parse_type_string("ClosureFactory"),
        Type::Object("ClosureFactory".to_string(), Vec::new())
    );
}

// Real corpus shape: amphp/amp's own `@param \Closure(CancelledException)
// $callback` - fully qualified with a leading `\`, which used to defeat
// the callable-shape check entirely (`"\closure(...".starts_with(
// "closure(")` is false), falling through to the generic-class branch
// and mangling into a bogus `Type::Object("Closure(CancelledException)",
// [])` "class" (reported as a false `class.notFound`).
#[test]
fn fully_qualified_closure_shape_syntax_also_degrades_to_plain_callable() {
    assert_eq!(
        parse_type_string("\\Closure(CancelledException)"),
        Type::Callable
    );
    assert_eq!(
        parse_type_string("\\Closure(int, string): bool"),
        Type::Callable
    );
}

// Real corpus shape: PHPUnit's own literal-string union `'->'|'::' `
// (documenting a real property-access-vs-static-call operator) - a
// bracket-shaped character *inside* one quoted member (the `>` in
// `'->'.`) used to be read as a real closing bracket by `split_top_level`,
// permanently corrupting its own depth counter and swallowing the `|`
// separator for the rest of the string entirely, mangling the whole
// thing into the single, nonsensical `Type::StringLiteral("->'|'::")`.
#[test]
fn a_quoted_literal_containing_a_bracket_shaped_character_does_not_corrupt_the_union_split() {
    assert_eq!(
        parse_type_string("'->'|'::'"),
        Type::Union(vec![
            Type::StringLiteral("->".to_string()),
            Type::StringLiteral("::".to_string()),
        ])
    );
}

#[test]
fn a_quoted_literal_containing_a_bracket_shaped_character_inside_an_array_shape() {
    assert_eq!(
        parse_type_string("array{type?: '->'|'::'}"),
        Type::ArrayShape(vec![(
            "type".to_string(),
            Type::Union(vec![
                Type::StringLiteral("->".to_string()),
                Type::StringLiteral("::".to_string()),
            ]),
            true,
        )])
    );
}
