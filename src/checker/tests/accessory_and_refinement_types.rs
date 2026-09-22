//! Union collapsing, readonly/property hooks, literal/accessory refinement types.

use super::super::*;
use super::helpers::*;
use crate::types::AccessoryKind;

// Exact type equality is the wrong tool for "is this the kind of thing
// being narrowed" - both `narrow_type_to`/`narrow_type_excluding`
// (member filtering) and `union_types` (member dedup at construction
// time) must not use plain `==`/`!=`.

#[test]
fn union_types_collapses_a_string_literal_into_an_already_present_string() {
    assert_eq!(
        union_types(Type::String, Type::StringLiteral("".to_string())),
        Type::String
    );
}

#[test]
fn union_types_collapses_an_int_literal_into_an_already_present_int() {
    assert_eq!(union_types(Type::Int, Type::IntLiteral(0)), Type::Int);
}

// The float-widening pitfall this fix deliberately avoids: `is_subtype
// (IntLiteral(0), Float)` is `true` (PHP's int-to-float argument
// coercion), but that's an *assignability* fact, not a "this value's
// representation is already covered" one - an `IntLiteral` sitting
// next to an unrelated `Float` member must not be silently dropped.
#[test]
fn union_types_does_not_collapse_an_int_literal_into_an_unrelated_float() {
    assert_eq!(
        union_types(Type::Float, Type::IntLiteral(0)),
        Type::Union(vec![Type::Float, Type::IntLiteral(0)])
    );
}

// The same absorption idea, for an accessory-refined string/array and an
// IntRange - an unknown branch genuinely might be empty/out of range, so
// the refined member can't survive next to its own separately-present,
// unrefined base.
#[test]
fn union_types_collapses_a_non_empty_string_into_an_already_present_string() {
    let non_empty = Type::Intersection(vec![
        Type::String,
        Type::Accessory(AccessoryKind::NonEmptyString),
    ]);
    assert_eq!(union_types(Type::String, non_empty), Type::String);
}

#[test]
fn union_types_collapses_a_list_into_an_already_present_array() {
    let list_ty = Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
        Type::Accessory(AccessoryKind::ArrayList),
    ]);
    assert_eq!(
        union_types(
            Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int)),
            list_ty
        ),
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Int))
    );
}

#[test]
fn union_types_collapses_an_int_range_into_an_already_present_int() {
    assert_eq!(
        union_types(Type::Int, Type::IntRange(Some(1), None)),
        Type::Int
    );
}

#[test]
fn union_types_collapses_a_float_literal_into_an_already_present_float() {
    assert_eq!(
        union_types(Type::Float, Type::FloatLiteral(1.5)),
        Type::Float
    );
}

#[test]
fn union_types_does_not_collapse_a_real_class_intersection() {
    // Guards the absorption fix above: a genuine, unrelated multi-class
    // intersection (no Accessory member at all) must never be treated
    // as "more specific than its own base type" - Countable&Iterator
    // sitting next to bare Countable in the same union are both real,
    // independent possibilities.
    let intersection = Type::Intersection(vec![
        Type::Object("Countable".to_string(), Vec::new()),
        Type::Object("Iterator".to_string(), Vec::new()),
    ]);
    assert_eq!(
        union_types(
            Type::Object("Countable".to_string(), Vec::new()),
            intersection.clone()
        ),
        Type::Union(vec![
            Type::Object("Countable".to_string(), Vec::new()),
            intersection
        ])
    );
}

// `merge_scopes` (one `union_types` call per `if`/`else`) and
// `write_index_into`'s own `Union` arm both feed into `union_types`,
// which must cap how many distinct members it accumulates - a function
// with many sequential branches assigning genuinely distinct array
// shapes to the same variable can otherwise grow that variable's union
// member count combinatorially. Simulates that shape directly: 20
// pairwise-distinct one-field `ArrayShape`s folded through
// `union_types` one at a time, exactly the access pattern `merge_scopes`
// uses across many sequential branches.
#[test]
fn union_types_caps_runaway_member_growth_instead_of_hanging() {
    let result = (0..20)
        .map(|i| Type::ArrayShape(vec![(i.to_string(), Type::Int, false)]))
        .reduce(union_types)
        .unwrap();
    assert_eq!(
        result,
        Type::Mixed(false),
        "expected the cap to collapse a >12-member union to Mixed, got {result:?}"
    );
}

// The cap must not misfire on the common, small case real code
// actually produces - a handful of genuinely distinct branch types
// stays a precise `Union`, not `Mixed`.
#[test]
fn union_types_below_the_cap_still_builds_a_precise_union() {
    let result = [Type::Int, Type::String, Type::Bool, Type::Null]
        .into_iter()
        .reduce(union_types)
        .unwrap();
    assert_eq!(
        result,
        Type::Union(vec![Type::Int, Type::String, Type::Bool, Type::Null])
    );
}

// The two tests above exercise `generalize_by_key_set` directly;
// this one confirms `union_types` itself actually calls it - built as
// a single `union_types(pre_built_union_of_12, one_more_shape)` call
// (not a `.reduce()` chain) specifically so only *one* generalization
// pass happens, matching what `generalize_by_key_set`'s own
// standalone tests already prove that pass does.
#[test]
fn union_types_itself_generalizes_before_collapsing_to_mixed() {
    let first_twelve = Type::Union(
        (0..12)
            .map(|i| {
                let value = if i % 2 == 0 { Type::True } else { Type::False };
                Type::ArrayShape(vec![("x".to_string(), value, false)])
            })
            .collect(),
    );
    // A genuinely new value, distinct from every one of the first 12
    // (which only alternate True/False) - `union_types`'s own
    // existing dedup (`!members.contains(&other)`) would otherwise
    // silently no-op adding a 13th member that already exists,
    // masking whether generalization ran at all.
    let thirteenth = Type::ArrayShape(vec![("x".to_string(), Type::Null, false)]);
    let result = union_types(first_twelve, thirteenth);
    match result {
        Type::ArrayShape(fields) => {
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "x");
        }
        other => panic!(
            "expected union_types to generalize down to one merged ArrayShape, got {other:?}"
        ),
    }
}

#[test]
fn a_literal_argument_satisfies_a_literal_string_parameter_a_plain_string_does_not() {
    let d = diagnostics_for(
        r#"<?php
            /** @param literal-string $x */
            function sink($x): void {}
            function f(): void {
                sink("hello");
            }
            function g(string $x): void {
                sink($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn writing_a_readonly_property_from_outside_its_declaring_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Point1 {
                public function __construct(public readonly int $x, public readonly int $y) {}
            }
            function mutateReadonly(Point1 $p): void { $p->x = 5; }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("Point1::$x is readonly")),
        "expected a readonly-write diagnostic, got: {d:?}"
    );
}

#[test]
fn writing_a_readonly_property_from_a_non_constructor_method_of_its_own_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Immutable {
                public function __construct(public readonly int $v) {}
                public function bump(): void { $this->v = $this->v + 1; }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("Immutable::$v is readonly")),
        "expected a readonly-write diagnostic, got: {d:?}"
    );
}

#[test]
fn a_promoted_readonly_property_is_not_flagged_by_its_own_promotion() {
    let d = diagnostics_for(
        r#"<?php
            class Point1 {
                public function __construct(public readonly int $x, public readonly int $y) {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn writing_a_readonly_property_inside_its_own_constructor_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Widget14 {
                public readonly int $a;
                public function __construct() { $this->a = 1; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A PHP 8.4 property hook's `set(Type $value)` can deliberately accept a
// wider type than the property's own backing/read type, coercing
// internally before the real assignment - a write must be checked
// against the hook's parameter type, not the property's declared type.
#[test]
fn a_property_hooks_wider_set_parameter_type_is_the_real_write_sink() {
    let d = diagnostics_for(
        r#"<?php
            class Config1 {
                /** @var list<string> */
                public array $names = ['a'] {
                    set(array $value) {
                        $this->names = array_values($value);
                    }
                }
            }
            function f(Config1 $c, array $raw): void {
                $c->names = $raw;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_property_hooks_narrower_set_parameter_type_is_still_enforced() {
    let d = diagnostics_for(
        r#"<?php
            class Config2 {
                public int $level = 0 {
                    set(int $value) {
                        $this->level = $value;
                    }
                }
            }
            function f(Config2 $c, string $bad): void {
                $c->level = $bad;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn writing_an_ordinary_mutable_property_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Ok3 {
                public int $mutable = 0;
                public function change(): void { $this->mutable = 5; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `iterable` must not be parsed as plain `array`, silently dropping its
// `|Traversable` half.

#[test]
fn iterable_accepts_a_real_traversable_not_just_an_array() {
    let d = diagnostics_for(
        r#"<?php
            function f(iterable $items): void {
                foreach ($items as $item) { echo $item; }
            }
            function test(): void {
                f(new ArrayIterator([1, 2, 3]));
                f([1, 2, 3]);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn iterable_still_rejects_a_genuinely_non_iterable_value() {
    let d = diagnostics_for(
        r#"<?php
            function f(iterable $items): void {}
            function test(): void { f("not iterable"); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn iterable_traversable_half_is_not_namespace_mangled() {
    // The synthesized `Traversable` half of `iterable` is this
    // project's own internal desugaring, not something the programmer
    // wrote as a class reference - it must stay global regardless of
    // the current namespace, the same way the native `object` hint
    // already does.
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            function f(iterable $items): void {
                foreach ($items as $item) { echo $item; }
            }
            function test(): void { f(new \ArrayIterator([1, 2, 3])); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// array-key/scalar/numeric/resource pseudo-types must be recognized, not
// silently fall through to a bogus literal class reference.

#[test]
fn array_key_accepts_both_string_and_int() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array-key $key */
            function f($key): void {}
            function test(): void { f("hello"); f(42); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_still_rejects_a_genuinely_wrong_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array-key $key */
            function f($key): void {}
            function test(): void { f([1, 2]); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn resource_pseudo_type_matches_across_namespaces() {
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            /** @param resource $handle */
            function useHandle($handle): void {}

            namespace {
                /** @param resource $h */
                function passthrough($h): void { \App\useHandle($h); }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// PHPStan-style literal-integer union types (`1|2|3`) must not reject
// every value, including genuinely valid ones - a bare integer in a
// type string must not fall through to a bogus literal-class-reference
// fallback.

#[test]
fn literal_integer_union_type_accepts_a_genuinely_valid_value() {
    let d = diagnostics_for(
        r#"<?php
            /** @param 1|2|3 $level */
            function f($level): void {}
            function test(): void { f(2); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn literal_integer_union_type_still_rejects_a_genuinely_invalid_value() {
    let d = diagnostics_for(
        r#"<?php
            /** @param 1|2|3 $level */
            function f($level): void {}
            function test(): void { f(5); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// String accessory case-tracking, full concat algorithm.

#[test]
fn strtolower_of_a_literal_constant_folds() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            assertType("'abc'", strtolower('ABC'));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn strtolower_of_a_plain_string_produces_lowercase_string() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('lowercase-string', strtolower($x));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn mb_convert_case_with_a_resolvable_upper_mode_produces_uppercase_string() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $x): void {
                assertType('uppercase-string', mb_convert_case($x, MB_CASE_UPPER));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn two_literal_strings_concatenated_constant_fold() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            assertType("'ab'", 'a' . 'b');
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_literal_string_typed_parameter_still_accepts_a_concat_of_two_literals() {
    // `LiteralString` must have a producer for `BinOp::Concat` - it must
    // not collapse to bare `string` even when both operands are
    // literals, which a `literal-string`-typed callee would then
    // correctly reject.
    let d = diagnostics_for(
        r#"<?php
            /** @param literal-string $s */
            function needs_literal(string $s): void {}
            needs_literal('a' . 'b');
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_lowercase_string_phpdoc_parameter_rejects_a_plain_unproven_string() {
    // `lowercase-string` must not degrade to plain `string`, or this
    // call is wrongly accepted.
    let d = diagnostics_for(
        r#"<?php
            /** @param lowercase-string $s */
            function needs_lowercase(string $s): void {}
            function f(string $x): void {
                needs_lowercase($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn a_lowercase_string_phpdoc_parameter_accepts_a_proven_lowercase_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param lowercase-string $s */
            function needs_lowercase(string $s): void {}
            function f(string $x): void {
                needs_lowercase(strtolower($x));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn concatenating_an_int_left_operand_with_a_pure_digit_right_literal_is_numeric_string() {
    // Isolates the `NumericString` propagation specifically (rather than
    // pinning the full composed type string, which also picks up
    // `NonFalsyString`/`NonEmptyString` from the same concat) via a
    // `numeric-string`-typed receiver.
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $s */
            function needs_numeric_string(string $s): void {}
            function f(int $x): void {
                needs_numeric_string($x . '5');
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn concatenating_an_int_left_operand_with_a_negative_signed_right_literal_withholds_numeric_string()
{
    // The one real false-positive-risk edge case this phase's own
    // validation predicate exists for: real PHPStan's own `int`-left
    // validation callback (`!str_starts_with($value, '-')`) excludes a
    // right constant that is itself negative-signed - concatenating an
    // arbitrary (possibly already-negative) int with another negative-
    // signed piece is not reliably numeric-looking.
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $s */
            function needs_numeric_string(string $s): void {}
            function f(int $x): void {
                needs_numeric_string($x . '-5');
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn a_float_literal_concatenated_with_a_string_literal_narrows_via_the_composed_accessories() {
    // This project's own inference has never given a bare float literal
    // expression a precise `Type::FloatLiteral` (only `Literal::Int`
    // does, `checker/expr.rs`'s own `ExprKind::Literal` arm collapses
    // `Literal::Float` straight to plain `Type::Float`) - so `1.5` here
    // reaches `resolve_concat_type` as `Type::Float`, not a literal, and
    // the result is the composed-accessory answer for that shape
    // (`Float`'s own `toString()` projection carries no non-empty/non-
    // falsy accessory on its own, so only the literal `'-suffix'`
    // operand's own non-falsiness survives the AND/OR composition).
    // `format_php_float`'s own real algorithm is covered directly by
    // `checker::narrowing::accessory_range`'s own unit tests instead,
    // where a real `Type::FloatLiteral` value is easy to construct by
    // hand.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            assertType('non-falsy-string', 1.5 . '-suffix');
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `Type::benevolent_array_key()` - real PHPStan's own `BenevolentUnionType`
// marker, used for a PHPDoc `Type[]`-shorthand/single-generic-arg
// `array<V>` key and the array-union `+` operator's own "neither side
// had a more precise key" fallback. `RuleLevelHelper::accepts()` treats
// it specially: accepted if *any* member matches, not only when *all*
// do - the real difference a plain, strict `int|string` union doesn't
// get. Confirmed via `dumpType()` against real PHPStan: a `@var
// DisplayBlock[]` foreach-key passed to a `string`-typed parameter
// produces zero errors.
#[test]
fn a_type_shorthand_array_key_is_accepted_by_a_string_typed_parameter() {
    let d = diagnostics_for(
        r#"<?php
            class DisplayBlock {}
            function hideBlock(string $block_id): void {}
            /** @param DisplayBlock[] $blocks */
            function apply(array $blocks): void {
                foreach ($blocks as $id => $block) {
                    hideBlock($id);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The same leniency for the single-generic-arg `array<V>` spelling of
// the identical shorthand - real PHPStan's own
// `TypeNodeResolver::resolveGenericTypeNode` uses the identical
// `BenevolentUnionType` construction for both forms.
#[test]
fn a_generic_one_arg_array_key_is_accepted_by_a_string_typed_parameter() {
    let d = diagnostics_for(
        r#"<?php
            function hideBlock(string $block_id): void {}
            /** @param array<int> $blocks */
            function apply(array $blocks): void {
                foreach ($blocks as $id => $block) {
                    hideBlock($id);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A plain, strict `int|string` union (not benevolent) is still held to
// the ordinary strict rule - only the marker itself grants the
// leniency, not "any union that happens to equal `int|string`".
#[test]
fn a_plain_non_benevolent_int_or_string_key_is_still_rejected_by_a_string_typed_parameter() {
    let d = diagnostics_for(
        r#"<?php
            function hideBlock(string $block_id): void {}
            /** @param array<int|string, int> $blocks */
            function apply(array $blocks): void {
                foreach ($blocks as $id => $block) {
                    hideBlock($id);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// The benevolent marker survives `array_flip()`'s own key/value swap -
// `array_key_type`/`array_element_type` return whatever `Type` is
// stored verbatim, so a marker sitting in the key position travels to
// the value position unchanged, with no dedicated `array_flip` handling
// needed.
#[test]
fn array_flip_carries_the_benevolent_marker_from_key_to_value_position() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /** @param array<int> $items */
            function f(array $items): void {
                $flipped = array_flip($items);
                foreach ($flipped as $k => $v) {
                    needsString($v);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The `+` operator's own real implementation
// (`InitializerExprTypeResolver::getPlusType`) keeps a real
// `BenevolentUnionType` fallback too when neither side has a more
// precise key, confirmed reading it directly - two `Type[]`-shorthand-
// keyed arrays unioned via `+` still accept an `int`-typed key read
// against a `string`-typed parameter.
#[test]
fn the_plus_operator_s_fallback_key_type_is_also_benevolent() {
    let d = diagnostics_for(
        r#"<?php
            function hideBlock(string $block_id): void {}
            /** @param array<int> $a
             *  @param array<int> $b
             */
            function f(array $a, array $b): void {
                $combined = $a + $b;
                foreach ($combined as $id => $v) {
                    hideBlock($id);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
