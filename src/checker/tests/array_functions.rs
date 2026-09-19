//! Built-in array_* function return type inference (map/filter/reduce/merge/etc).

use super::helpers::*;

// array_map/array_filter return-type precision. Each test proves the
// inferred type is real, not just "still Mixed and therefore
// compatible with everything" - is_assignable's own Array(Mixed)
// leniency means an imprecise result would silently satisfy any
// declared element type, so the discriminating check here is a call
// the real, precise type must actually fail.

#[test]
fn array_map_infers_the_real_element_type_from_an_untyped_arrow_fn_param() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            $mapped = array_map(fn($x) => $x * 2, [1, 2, 3]);
            needsInts($mapped);
            needsStrings($mapped);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsStrings"), "got: {d:?}");
    assert!(
        d[0].message
            .contains("expects array<(int|string), string>, array<int> given"),
        "got: {d:?}"
    );
}

#[test]
fn array_map_infers_the_real_element_type_from_an_untyped_closure_param() {
    // Same as the arrow-fn test, but the common
    // `function($x) { return ...; }` single-return-statement body
    // shape instead.
    let d = diagnostics_for(
        r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            $mapped = array_map(function ($x) { return $x * 2; }, [1, 2, 3]);
            needsStrings($mapped);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message
            .contains("expects array<(int|string), string>, array<int> given"),
        "got: {d:?}"
    );
}

#[test]
fn array_map_trusts_an_explicit_closure_return_type_hint_over_the_array_element_type() {
    // The closure's own declared return type wins, independent of
    // what the array's element type would otherwise suggest.
    let d = diagnostics_for(
        r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            $mapped = array_map(fn(int $x): string => (string) $x, [1, 2, 3]);
            needsStrings($mapped);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_map_prefers_the_closures_own_more_precise_inferred_return_over_its_hint() {
    // `DbMaintenanceRepositoryTest`'s own real shape: a closure literal's
    // declared `: string` hint isn't the most precise answer when its own
    // body actually returns something narrower - unlike an ordinary named
    // function/method (already reconciled elsewhere), a closure/arrow-fn
    // literal's signature used to trust the bare hint unconditionally.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-string[] $xs */
            function needsNonEmptyStrings(array $xs): void {}
            $mapped = array_map(static function (int $x): string {
                return "not-empty";
            }, [1, 2, 3]);
            needsNonEmptyStrings($mapped);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_map_falls_back_to_the_hint_when_the_bodys_inference_is_less_precise() {
    // The exact mirror of the test above: `alwaysMixed()` gives the
    // closure's own body inference nothing better than `Mixed`, so the
    // declared `: string` hint must still win, not get discarded to
    // `Mixed` (which `is_assignable` would trivially treat as
    // compatible with anything, silently losing the hint's own real
    // precision).
    let d = diagnostics_for(
        r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function alwaysMixed(): mixed {
                return 1;
            }
            $mapped = array_map(static function (int $x): string {
                return alwaysMixed();
            }, [1, 2, 3]);
            needsStrings($mapped);
            needsInts($mapped);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsInts"), "got: {d:?}");
}

#[test]
fn array_map_still_trusts_the_hint_for_a_multi_statement_closure_body() {
    // A real regression, caught by this project's own corpus run: only a
    // single-implicit-return-statement body is inferred at all (`infer_
    // closure_return_with_param_overrides`'s own doc comment) - a real
    // corpus shape (`ExtensionRepository::findUserIdsByTheme`'s own
    // `if (! is_numeric(...)) { throw ...; } return ...;` guard-then-
    // return body) has two statements, not one. The declared `: string`
    // hint must still be used for *this* shape too - discarding it
    // entirely (rather than falling back to it, the same as no body
    // inference being attempted at all) silently lost `array_map()`'s
    // own list-ness preservation, not just element-type precision.
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $xs */
            function needsListOfStrings(array $xs): void {}
            /** @return list<int> */
            function ids(): array { return [1, 2, 3]; }
            $mapped = array_map(static function (int $x): string {
                if ($x < 0) {
                    throw new \RuntimeException('negative');
                }
                return (string) $x;
            }, ids());
            needsListOfStrings($mapped);
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A first-class callable (`intval(...)`, PHP 8.1) used as array_map's
// own callback must be recognized, or it discards both the mapped
// element type and any list-ness/non-emptiness the input already had.
#[test]
fn array_map_resolves_a_first_class_callable_to_a_native_function() {
    let d = diagnostics_for(
        r#"<?php
            /** @return non-empty-list<int> */
            function f(): array {
                return array_map(intval(...), explode(',', '1,2,3'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_map_resolves_a_first_class_callable_to_a_static_method() {
    let d = diagnostics_for(
        r#"<?php
            class TagId16 {
                private function __construct(public int $value) {}
                public static function from(int $value): self { return new self($value); }
            }
            /** @param list<int> $ids
             *  @return list<TagId16>
             */
            function f(array $ids): array {
                return array_map(TagId16::from(...), $ids);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A static factory returning `self` (`TagId::tryFrom(...): ?self`) must
// not resolve to the literal, un-substituted `self|null` instead of the
// concrete class - every other call site that resolves a method's
// return type runs it through resolve_pseudo afterward, and this one
// needs the same treatment.
#[test]
fn array_map_resolves_a_first_class_callables_own_self_return_type() {
    let d = diagnostics_for(
        r#"<?php
            class TagId17 {
                private function __construct(public int $value) {}
                public static function tryFrom(mixed $value): ?self {
                    return is_int($value) ? new self($value) : null;
                }
            }
            /** @param list<mixed> $values
             *  @return list<TagId17|null>
             */
            function f(array $values): array {
                return array_map(TagId17::tryFrom(...), $values);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The `ExprKind::MethodCall` arm resolving a first-class callable's own
// signature must not require the receiver's own type to be literally
// `Type::Object`, or it silently skips (and discards the mapped element
// type entirely, the same regression the tests above guard for a
// `Call`/`StaticCall` callable) a `Union`/`Intersection` receiver.
#[test]
fn array_map_resolves_a_first_class_callable_to_an_instance_method_on_a_union_typed_receiver() {
    let d = diagnostics_for(
        r#"<?php
            class TagId18 {
                private function __construct(public int $value) {}
                public function from(int $value): self { return new self($value); }
            }
            class OtherTagId18 {
                private function __construct(public int $value) {}
                public function from(int $value): self { return new self($value); }
            }
            /** @param list<int> $ids
             *  @return list<TagId18>
             */
            function f(TagId18|OtherTagId18 $first, array $ids): array {
                return array_map($first->from(...), $ids);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// PHP 8.4's `array_find`/`array_find_key`/`array_any`/`array_all` need
// dedicated handling - the vendored stub declares `array_find`/
// `array_find_key` `mixed`, matching real PHPStan's own dedicated
// `ArrayFind{,Key}FunctionReturnTypeExtension` fallback absence, so
// without it a call like `array_find($cachedSections, fn
// ($knownSection) => ...)` loses its element type's own precision down
// to `mixed`. `array_any`/`array_all` already declare `bool` correctly
// in the stub - their own gap is the callback's own parameter type, not
// the return.

#[test]
fn array_find_infers_the_element_type_or_null() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string $x */
            function needsString($x): void {}
            /** @param int[] $xs */
            function f(array $xs): void {
                needsString(array_find($xs, fn($x) => $x > 0));
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"
            && x.message.contains("expects string, int|null given")),
        "got: {d:?}"
    );
}

#[test]
fn array_find_key_infers_the_key_type_or_null() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string $x */
            function needsString($x): void {}
            /** @param list<int> $xs */
            function f(array $xs): void {
                needsString(array_find_key($xs, fn($x, $k) => $x > 0));
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"
            && x.message.contains("expects string, int|null given")),
        "got: {d:?}"
    );
}

#[test]
fn array_filter_preserves_the_input_array_s_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param int[] $xs */
            function f(array $xs) {
                $filtered = array_filter($xs, fn($x) => $x > 0);
                needsInts($filtered);
                needsStrings($filtered);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message
            .contains("expects array<(int|string), string>, array<(int|string), int> given"),
        "got: {d:?}"
    );
}

#[test]
fn array_reduce_infers_the_callback_s_own_return_type() {
    // The result of a fold is ultimately whatever the callback's own
    // return type is (each call's return becomes the next call's
    // $carry) - here that's Int, so passing it where a string is
    // expected must be flagged, proving this isn't just Mixed.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            function needsString(string $s): void {}
            $sum = array_reduce([1, 2, 3], fn($carry, $item) => $carry + $item, 0);
            needsInt($sum);
            needsString($sum);
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsString"), "got: {d:?}");
}

#[test]
fn array_reduce_binds_item_to_the_array_s_own_element_type() {
    // Returning $item directly (ignoring $carry) isolates the other
    // half of the binding - the array's own element type, not just
    // $initial's.
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function f(array $xs) {
                $last = array_reduce($xs, fn($carry, $item) => $item, 0);
                needsString($last);
            }
            function needsString(string $s): void {}
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsString"), "got: {d:?}");
}

#[test]
fn array_reduce_with_no_initial_value_binds_carry_to_null() {
    // Real PHP defaults $initial to null when omitted - returning
    // $carry directly (ignoring $item) isolates exactly what it was
    // bound to; Null isn't assignable to string, so this must be
    // flagged, proving it wasn't left as an unbound Mixed (which
    // would silently pass any check).
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function f(array $xs) {
                $result = array_reduce($xs, fn($carry, $item) => $carry);
                needsString($result);
            }
            function needsString(string $s): void {}
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsString"), "got: {d:?}");
}

#[test]
fn array_combine_infers_the_values_array_s_own_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param string[] $keys
             *  @param int[] $vals */
            function f(array $keys, array $vals) {
                $combined = array_combine($keys, $vals);
                needsInts($combined);
                needsStrings($combined);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsStrings"), "got: {d:?}");
}

// The native stub's own `@return int[]|string[]` (a whole-array
// either-or, not a single homogeneous key type) must not always win
// when the argument's own shape is knowable, or it prints as the
// confusing `array<int>|array<string>`.
#[test]
fn array_keys_infers_string_keys_from_a_known_array_shape() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            function f() {
                $insert = ['id' => 1, 'galleries_url' => 'x'];
                needsStrings(array_keys($insert));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The 2-/3-arg `$search_value`/`$strict` form must not fall through to
// the bare stub's own useless `int[]|string[]` answer just because this
// arm only gates on `args.len() == 1` - `$search_value`/`$strict` only
// ever narrow which keys survive, never the key type itself, confirmed
// via `dumpType()` against real PHPStan, which returns the identical
// `list<string>` for the 3-arg form here that it does for the bare
// 1-arg form.
#[test]
fn array_keys_with_a_search_value_still_infers_the_real_key_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, string> $byLower */
            function f(array $byLower, string $needle): string {
                $found = array_keys($byLower, $needle, true);
                return $found[0];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_keys_infers_int_keys_when_every_key_is_canonically_numeric() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function f() {
                $list = ['x', 'y', 'z'];
                needsInts(array_keys($list));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_keys_unions_int_and_string_for_a_genuinely_mixed_shape() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function needsInts(array $xs): void {}
            function f() {
                $mixed = [0 => 'a', 'name' => 'b'];
                needsInts(array_keys($mixed));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsInts"), "got: {d:?}");
}

// A plain, non-literal `array<string, V>` argument (not a literal
// shape, so the dedicated case above doesn't apply) must not fall
// through to the native stub's own literal `@return int[]|string[]`
// verbatim, or it surfaces `int|string` for every reader of a key.
#[test]
fn array_keys_on_a_plain_typed_array_falls_back_to_a_mixed_key_type_not_int_or_string() {
    // `array_keys()`'s own key-type propagation only trusts a literal
    // `Type::ArrayShape`'s key type, not a plain, non-shape
    // `Type::Array`'s - this project's dynamic-key-write widening
    // doesn't track a plain array's own key type precisely enough to
    // propagate safely (see `is_or_wraps_array_shape`'s own doc
    // comment). A plain `array<string, string>`-typed argument
    // therefore still degrades to `Mixed`-keyed here.
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, string> $classMap */
            function f(array $classMap): void {
                foreach (array_keys($classMap) as $symbol) {
                    str_starts_with($symbol, 'x');
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_keys_on_a_genuinely_untyped_array_falls_back_to_mixed() {
    let d = diagnostics_for(
        r#"<?php
            function needsStrings(string ...$xs): void {}
            function f(array $anything): void {
                needsStrings(...array_keys($anything));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// array_keys() always returns a real list (sequentially re-keyed from
// zero, verified via dumpType()) regardless of whether the input's own
// keys were already sequential.
#[test]
fn array_keys_result_is_a_real_list_not_just_a_plain_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $xs */
            function needsList(array $xs): void {}
            /** @param array<string, callable> $filters */
            function f(array $filters): void {
                needsList(array_keys($filters));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_fill_and_array_fill_keys_infer_the_fill_value_s_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            function f(array $keys) {
                $a = array_fill(0, 5, 42);
                $b = array_fill_keys($keys, 42);
                needsStrings($a);
                needsStrings($b);
            }
        "#,
    );
    assert_eq!(d.len(), 2, "got: {d:?}");
    assert!(d.iter().all(|x| x.message.contains("needsStrings")));
}

// Matches real PHPStan's own `ArrayFillFunctionReturnTypeExtension`:
// `array_fill()`'s own result is `list`-accessory whenever the start
// index is provably `0`, and `non-empty`-accessory whenever the count
// is provably `>= 1`.
#[test]
fn array_fill_marks_list_and_non_empty_accessories_when_provable() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(int $n): void {
                assertType('list<0>', array_fill(0, $n, 0));
                assertType('non-empty-list<0>', array_fill(0, 5, 0));
                assertType('array<0>', array_fill(2, $n, 0));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// array_fill_keys()'s own key-type precision - $keys' own elements
// become the result's real keys, not the generic unknown_array_key()
// default.

#[test]
fn array_fill_keys_infers_the_key_type_from_the_keys_array() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $keys */
            function f(array $keys): void {
                assertType('array<string, 42>', array_fill_keys($keys, 42));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// array_search()'s own key-type precision, one level less precise than
// the pre-existing "definitely a list" case just above - a haystack
// with a confidently-known key type (not just "definitely a list")
// narrows the found-key half of the result the same way.

#[test]
fn array_search_narrows_to_the_haystack_s_own_known_key_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $haystack */
            function f(array $haystack): void {
                assertType('string|false', array_search(1, $haystack));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_search_stays_generic_for_an_unknown_key_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(array $haystack): void {
                assertType('int|string|false', array_search(1, $haystack));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// array_intersect_key() - every surviving key/value pair's own type
// comes straight from the first argument; only which keys survive is
// narrowed by the rest. List-ness never survives (a gap can open up
// anywhere).

#[test]
fn array_intersect_key_preserves_the_first_array_s_own_key_and_value_types() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array<string, int> $a
             * @param array<string, bool> $b
             */
            function f(array $a, array $b): void {
                assertType('array<string, int>', array_intersect_key($a, $b));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The identical `array_intersect_key` shape, generalized to every
// other `array_diff*`/`array_udiff*`/`array_intersect*`/
// `array_uintersect*` sibling.

#[test]
fn array_diff_preserves_the_first_array_s_own_value_type_and_loses_list_ness() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<int> $xs */
            function f(array $xs, array $other): void {
                assertType('array<int, int>', array_diff($xs, $other));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_diff_key_preserves_the_first_array_s_own_key_and_value_types() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array<string, int> $a
             * @param array<string, bool> $b
             */
            function f(array $a, array $b): void {
                assertType('array<string, int>', array_diff_key($a, $b));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A single spread argument (`array_intersect(...$arrays)`) has exactly
// one AST-level `Arg`, so a plain `args.len() >= 2` gate would fail
// outright and fall all the way through to a bare `Mixed`/`array`
// fallback - unlike `array_merge`'s own sibling arm, which already
// unwraps a spread source one level to reach the arrays actually being
// merged/diffed/intersected.
#[test]
fn array_intersect_and_array_diff_handle_a_single_spread_argument() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, list<int>> $byKey */
            function f(array $byKey): void {
                assertType('array<int, int>', array_intersect(...array_values($byKey)));
                assertType('array<int, int>', array_diff(...array_values($byKey)));
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn array_udiff_preserves_the_first_array_s_own_value_type() {
    // Only asserts the return-type shape this test is about - a
    // separate, pre-existing stub imprecision (phpstorm-stubs' own real
    // `array_udiff` declares its PHP 8.0+ `...$rest` tail with a bare
    // `@param array ...$rest` docblock, with no way to express "0+
    // arrays then one trailing callback") separately still flags the
    // 3rd argument itself - unrelated to this check, not fixed here.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array<string, int> $a
             * @param array<string, int> $b
             */
            function f(array $a, array $b): void {
                assertType('array<string, int>', array_udiff($a, $b, fn($x, $y) => $x <=> $y));
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpstanRs.assertType"),
        "got: {d:?}"
    );
}

#[test]
fn array_intersect_preserves_the_first_array_s_own_value_type_and_loses_list_ness() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<int> $xs */
            function f(array $xs, array $other): void {
                assertType('array<int, int>', array_intersect($xs, $other));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_uintersect_assoc_preserves_the_first_array_s_own_key_and_value_types() {
    // Same caveat as `array_udiff` just above - only the return-type
    // shape is asserted here.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array<string, int> $a
             * @param array<string, int> $b
             */
            function f(array $a, array $b): void {
                assertType(
                    'array<string, int>',
                    array_uintersect_assoc($a, $b, fn($x, $y) => $x <=> $y)
                );
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpstanRs.assertType"),
        "got: {d:?}"
    );
}

// array_flip() swaps keys and values - list-ness never survives (the new
// keys come from the old values, essentially never already 0, 1, 2,
// ...) but non-emptiness does.

#[test]
fn array_flip_swaps_key_and_value_types() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $xs */
            function f(array $xs): void {
                assertType('array<string, int>', array_flip($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_flip_preserves_non_emptiness_but_not_list_ness() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('non-empty-array<string, int>', array_flip($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The internal-array-pointer family - reset/current/end/key/
// array_key_first/array_key_last/next/prev.

// Each pointer function still gets its own dedicated `$xs`/function
// below, purely for test isolation - not because of the stale widening
// concern this comment used to describe. `reset`/`end`/`next`/`prev`
// are declared by-ref in their own real stub (`object|array &$array` -
// none of them actually mutate the array's *value*, only its internal
// pointer), and this project's own generic by-ref-argument-widening
// used to apply anyway, discarding the argument's own precise type down
// to the stub's coarse declared one - confirmed a real, corpus-relevant
// bug (`Checker::calls`'s own by-ref-widening loop now exempts them the
// same way the `sort()`/`asort()` families already are, see its own doc
// comment) - see `reset_current_end_preserve_the_arguments_own_precise_type_afterward`
// below for the regression test.

#[test]
fn reset_reports_the_element_type_when_provably_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('string', reset($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn current_reports_the_element_type_when_provably_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('string', current($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn end_reports_the_element_type_when_provably_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('string', end($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reset_current_end_report_false_when_provably_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = [];
                $b = [];
                $c = [];
                assertType('false', reset($a));
                assertType('false', current($b));
                assertType('false', end($c));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reset_unions_with_false_when_emptiness_is_unknown() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $xs */
            function f(array $xs): void {
                assertType('string|false', reset($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The real corpus-confirmed regression: `Checker::calls`'s own generic
// by-ref-argument widening (`param.by_ref` gated) used to apply to
// `reset`/`end`/`next`/`prev`'s own declared `object|array &$array`
// stub param unconditionally, discarding the argument's real, precise
// type - confirmed via `dumpType()` against real PHPStan that it stays
// completely unchanged afterward, including through a shape real
// PHPStan verifiably keeps well past this project's own union-member
// cap (a many-independently-conditional-field accumulator, each field
// still a real `list<int>`, never collapsed into one shape the way real
// PHPStan's own `TypeCombinator` would).
#[test]
fn reset_current_end_preserve_the_arguments_own_precise_type_afterward() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param list<int> $tags
             * @param list<int> $custom
             */
            function f(bool $hasTags, bool $hasCustom, array $tags, array $custom): void {
                $imageIdsForFilter = [];
                if ($hasTags) {
                    $imageIdsForFilter['tags'] = $tags;
                }
                if ($hasCustom) {
                    $imageIdsForFilter['custom'] = $custom;
                }
                if ($imageIdsForFilter !== []) {
                    $first = reset($imageIdsForFilter);
                    assertType('list<int>', $first);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn next_always_unions_with_false_even_when_provably_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('string|false', next($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn prev_always_unions_with_false_even_when_provably_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $xs */
            function f(array $xs): void {
                assertType('string|false', prev($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn key_reports_the_key_type_when_provably_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-array<string, int> $xs */
            function f(array $xs): void {
                assertType('string', key($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn key_unions_with_null_when_emptiness_is_unknown() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, int> $xs */
            function f(array $xs): void {
                assertType('string|null', key($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_first_and_last_report_the_key_type_when_provably_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-array<string, int> $xs */
            function f(array $xs): void {
                assertType('string', array_key_first($xs));
                assertType('string', array_key_last($xs));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_first_reports_a_real_key_type_for_a_literal_array_shape() {
    // `array_key_type` handles `Type::ArrayShape` directly - a real
    // union of each field's own key kind, not a generic `int|string`
    // fallback. `types::shape_key_type` contributes each int-canonical
    // key's real `IntLiteral`, not a widened `Type::Int`, making this
    // even more precise: `0|1|2`, not just `int` - `array_key_first`
    // has no per-call "which key is really first" tracking of its own
    // (it reuses the same aggregate key union `key()`/`array_key_last`
    // do), so a real union of every known key is the best this project's
    // own architecture gets here, still strictly more precise than `int`.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                assertType('string', array_key_first(['a' => 1, 'b' => 2]));
                assertType('0|1|2', array_key_first([1, 2, 3]));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn key_and_array_key_first_report_null_when_provably_empty() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $a = [];
                assertType('null', key($a));
                assertType('null', array_key_first([]));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_chunk_wraps_the_original_element_type_in_an_extra_array_layer() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param int[] $xs */
            function f(array $xs) {
                $chunks = array_chunk($xs, 2);
                needsStrings($chunks);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(
        d[0].message
            .contains("expects string[], array<array<int>> given")
            || d[0]
                .message
                .contains("expects array<string>, array<array<int>> given")
            || d[0]
                .message
                .contains("expects array<(int|string), string>, array<array<int>> given"),
        "got: {d:?}"
    );
}

#[test]
fn array_column_extracts_a_known_shape_field_s_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string[] $xs */
            function needsStrings(array $xs): void {}
            /** @param array<array{id: int, name: string}> $rows */
            function f(array $rows) {
                $ids = array_column($rows, 'id');
                needsStrings($ids);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsStrings"), "got: {d:?}");
}

#[test]
fn array_column_falls_back_safely_without_a_known_shape_or_literal_key() {
    // Neither a plain, unshaped array<int> element type nor a
    // dynamic (non-literal) column key can be resolved to a
    // specific field type - both must fall through to the existing
    // generic behavior without panicking or guessing wrong.
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function f(array $xs, string $dynamicKey) {
                $a = array_column($xs, 'whatever');
                /** @param array<array{id: int}> $rows */
                function g(array $rows, string $key) {
                    $b = array_column($rows, $key);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_column_on_an_unshaped_row_array_still_produces_a_list() {
    // A raw DB-fetch row type (`array<string, mixed>`, no shape at all)
    // still has array_column()'s own real list-ness (a fresh, always-
    // sequential result array) even though there's no specific field
    // type to extract - the row's own uniform (here Mixed) element type
    // is the best available answer, and `Array(Mixed)`'s own existing
    // leniency lets that satisfy a more specific declared element type.
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $x */
            function wantsList(array $x): void {}
            /** @param list<array<string, mixed>> $rows */
            function f(array $rows): void {
                wantsList(array_column($rows, 'version'));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_column_with_an_index_key_does_not_gain_list_ness() {
    // The 3-argument form uses $index_key's own column values as the
    // result's keys - not necessarily sequential integers - so it must
    // not gain the same list marker the 2-argument form does.
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $x */
            function wantsList(array $x): void {}
            /** @param list<array{id: string, name: string}> $rows */
            function f(array $rows): void {
                wantsList(array_column($rows, 'name', 'id'));
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn array_search_on_a_list_haystack_narrows_to_int_or_false() {
    // A list's own keys are always sequential integers by definition,
    // so searching one can
    // only ever find an `int` key (or `false`), never `string` - the
    // real stub's own unconditional `string|int|false` return type
    // doesn't know this.
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $haystack */
            function indexOf(array $haystack, string $needle): int {
                $index = array_search($needle, $haystack, true);
                if ($index === false) {
                    throw new RuntimeException('not found');
                }
                return $index;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_search_on_a_non_list_haystack_keeps_todays_generic_union() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, string> $haystack */
            function indexOf(array $haystack, string $needle): int {
                $index = array_search($needle, $haystack, true);
                if ($index === false) {
                    throw new RuntimeException('not found');
                }
                return $index;
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the generic string|int union to still apply: {d:?}"
    );
}

#[test]
fn array_map_array_filter_and_array_reduce_fall_back_safely_for_shapes_they_do_not_special_case() {
    // A non-closure callback (a variable, not introspectable), a
    // 3+-arg multi-array call, a callback with more than one param,
    // array_filter on an argument this checker has no specific type
    // for (Mixed), and array_reduce with a callback whose arity
    // doesn't match (carry, item) all safely fall through to the
    // existing, imprecise (but correct) generic behavior - no panic,
    // no false positive. Untyped params (Mixed) throughout,
    // deliberately, so this only exercises the fallback paths
    // themselves, not check_args's own separate, unrelated
    // argument-type checking.
    let d = diagnostics_for(
        r#"<?php
            function f(callable $cb, array $a, array $b) {
                $x = array_map($cb, $a);
                $y = array_map(fn($p, $q) => $p + $q, $a, $b);
                $z = array_reduce($a, fn($only_one_param) => $only_one_param);
            }
            function g($m) {
                $w = array_filter($m);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_filter_called_with_zero_arguments_does_not_panic() {
    // A syntactically valid but semantically invalid 0-arg
    // array_filter() call parses fine and must not be blindly indexed
    // into (an out-of-bounds panic otherwise).
    let d = diagnostics_for(r#"<?php array_filter();"#);
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("expects at least 1"), "got: {d:?}");
}

// The generic by-ref-widening rule above must not discard real
// precision for the sort() family by falling into the generic "trust
// the declared param type" widening, which is only the plain
// `array &$array` stub for every one of these: sort/rsort/
// usort/shuffle always reindex to a real list; asort/arsort/ksort/
// krsort/uasort/uksort/natsort/natcasesort preserve the exact key
// structure unchanged.
#[test]
fn sort_reindexes_an_already_typed_array_to_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, int> $x
             *  @return list<int>
             */
            function f(array $x): array {
                sort($x);
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn usort_and_shuffle_also_reindex_to_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, int> $a
             *  @return list<int>
             */
            function f(array $a): array {
                usort($a, static fn (int $x, int $y): int => $x <=> $y);
                return $a;
            }
            /** @param array<string, int> $b
             *  @return list<int>
             */
            function g(array $b): array {
                shuffle($b);
                return $b;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn asort_and_ksort_preserve_the_exact_type_unchanged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{a: int, b: string} $x
             *  @return array{a: int, b: string}
             */
            function f(array $x): array {
                asort($x);
                return $x;
            }
            /** @param array{a: int, b: string} $y
             *  @return array{a: int, b: string}
             */
            function g(array $y): array {
                ksort($y);
                return $y;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_values_always_narrows_to_a_list_even_from_a_non_list_input() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, int> $x
             *  @return list<int>
             */
            function f($x): array {
                return array_values($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_filter_array_combine_and_array_unique_no_longer_claim_the_result_is_still_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingFilter($x): array {
                return array_filter($x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingCombine($x): array {
                return array_combine($x, $x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function usingUnique($x): array {
                return array_unique($x);
            }
        "#,
    );
    assert_eq!(d.len(), 3, "got: {d:?}");
}

// `array_unique`'s own list-ness-loss above doesn't extend to non-
// emptiness: real PHP only ever removes *duplicates* (always keeping
// at least the first occurrence of each distinct value), so a non-empty
// input can never come out empty - confirmed via `dumpType()` against
// real PHPStan, unlike `array_diff`/`array_intersect`, which really can
// empty a non-empty input and correctly don't get this same treatment.
#[test]
fn array_unique_preserves_non_emptiness_but_not_list_ness() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<int> $x */
            function f(array $x): void {
                assertType('non-empty-array<int, int>', array_unique($x));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `strip_accessory_markers` must strip through a `Union` branch too
// (the real shape a loop-conditionally-appended array literal gets,
// e.g. `non-empty-list<T>|array{}`), not just a bare `Intersection`, or
// `array_filter()`'s own 2-argument, non-null/non-instanceof-callback
// case wrongly still claims list-ness through the `Union`.
#[test]
fn array_filter_with_an_arbitrary_callback_strips_list_ness_through_a_union() {
    let d = diagnostics_for(
        r#"<?php
            /** @return list<int> */
            function build(array $rows): array {
                $x = [];
                foreach ($rows as $row) {
                    if ($row) {
                        $x[] = $row;
                    }
                }
                return array_filter($x, is_int(...));
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("return type mismatch")),
        "expected array_filter's own list marker to be stripped through the Union, got: {d:?}"
    );
}

// Real PHPStan's own array_filter() extension recognizes an explicit
// null-check callback (`fn ($v) => $v !== null`) and strips Null from
// the retained element type, not just its usual accessory-marker
// stripping - verified via dumpType().
#[test]
fn array_filter_with_an_explicit_null_check_arrow_fn_strips_null_from_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<int|null> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, static fn (?int $v): bool => $v !== null);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_filter_with_an_explicit_null_check_closure_strips_null_from_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<int|null> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, static function (?int $v): bool {
                    return $v !== null;
                });
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_filter_with_an_unrelated_callback_does_not_strip_null() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<int|null> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, static fn (?int $v): bool => $v !== 0);
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected null to still survive an unrelated filter: {d:?}"
    );
}

// Real PHPStan's own other recognized array_filter() extension: an
// instanceof-shaped filter callback (`fn ($v): bool => $v instanceof
// Target`) narrows the retained element type to Target specifically,
// not just strips Null - verified via dumpType().
#[test]
fn array_filter_with_an_instanceof_callback_narrows_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            class TagId18 {}
            class OtherId18 {}
            /** @param array<TagId18|OtherId18|null> $x
             *  @return array<TagId18>
             */
            function f(array $x): array {
                return array_filter($x, static fn (mixed $v): bool => $v instanceof TagId18);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `array_filter`'s third recognized callback shape: a native
// `is_string`/`is_int`/`is_float`/`is_bool`/`is_null` type-check
// predicate (including the first-class-callable form,
// `array_filter($x, is_string(...))`). Verified via dumpType() that
// real PHPStan narrows the retained element type to exactly that type,
// the same way an `instanceof`-shaped callback already narrows to a
// class above.
#[test]
fn array_filter_with_a_first_class_callable_is_string_predicate_narrows_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<mixed> $x
             *  @return array<string>
             */
            function f(array $x): array {
                return array_filter($x, is_string(...));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The same predicate also narrows when spelled as an arrow-fn/closure
// wrapper, matching the sibling instanceof-callback recognizer's own
// coverage of both shapes.
#[test]
fn array_filter_with_an_is_int_arrow_fn_predicate_narrows_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<mixed> $x
             *  @return array<int>
             */
            function f(array $x): array {
                return array_filter($x, fn (mixed $v): bool => is_int($v));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_filter_with_an_is_bool_closure_predicate_narrows_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<mixed> $x
             *  @return array<bool>
             */
            function f(array $x): array {
                return array_filter($x, function (mixed $v): bool {
                    return is_bool($v);
                });
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real PHP's no-callback array_filter() default drops every falsy
// element, exactly what narrow_type_excluding_falsy (already shared
// with `!empty($x)`'s own narrowing) already computes - not just
// stripping accessory markers and leaving the element type unchanged.
// Verified via dumpType() (list<TagId|null> -> list<TagId>;
// list<int|null> -> a real non-zero-int, stripping both 0 and null,
// not just null).
#[test]
fn array_filter_with_no_callback_drops_every_falsy_element() {
    let d = diagnostics_for(
        r#"<?php
            class TagId19 {}
            /** @param array<TagId19|null> $x
             *  @return array<TagId19>
             */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `array_filter([$sourceId, $copyId])` where `$copyId` is `?GroupId`
// must not keep field `1` typed `GroupId|null` and unconditionally
// required, rejecting a `list<GroupId>`-typed sink - real PHP's
// no-callback filtering really can (and does, when `$copyId` is null)
// drop that key entirely.
#[test]
fn array_filter_with_no_callback_on_a_shape_drops_a_provably_null_field() {
    let d = diagnostics_for(
        r#"<?php
            class GroupId {}
            /** @param list<GroupId> $ids */
            function sink(array $ids): void {}
            function f(GroupId $sourceId, ?GroupId $copyId): void {
                sink(array_filter([$sourceId, $copyId]));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A field that's provably *always* falsy (a literal `null` in the
// shape, not merely possibly-null) must be dropped from the shape
// entirely, not just marked optional.
#[test]
fn array_filter_with_no_callback_on_a_shape_drops_an_always_falsy_field_entirely() {
    let d = diagnostics_for(
        r#"<?php
            class GroupId {}
            /** @param array{0: GroupId} $ids */
            function sink(array $ids): void {}
            function f(GroupId $sourceId): void {
                // Position 1 is a literal `null` - always falsy, so
                // array_filter's own result can never carry key 1 at
                // all, matching the exact `array{0: GroupId}` shape
                // `sink()` requires (not `array{0: GroupId, 1?: mixed}`).
                sink(array_filter([$sourceId, null]));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_slice_and_array_reverse_preserve_list_ness_only_with_default_preserve_keys() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x
             *  @return list<int>
             */
            function sliceDefault($x): array {
                return array_slice($x, 1);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function reverseDefault($x): array {
                return array_reverse($x);
            }
            /** @param list<int> $x
             *  @return list<int>
             */
            function slicePreserveKeys($x): array {
                return array_slice($x, 1, null, true);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].location.contains("slicePreserveKeys"), "got: {d:?}");
}

// array_merge() combining several list<T>-typed arguments. Verified
// against real PHPStan's own dumpType(): merging only list-like
// arguments stays a list; merging in even one plain array<T> degrades
// list-ness for the whole result but keeps the element-type union.
#[test]
fn array_merge_of_two_lists_stays_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $a
             *  @param list<string> $b
             *  @return list<string>
             */
            function f($a, $b): array {
                return array_merge($a, $b);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_merge_of_a_list_and_a_plain_array_degrades_to_a_plain_array_not_a_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $a
             *  @param array<string> $b
             *  @return list<string>
             */
            function f($a, $b): array {
                return array_merge($a, $b);
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the list-ness mismatch to still be flagged: {d:?}"
    );
}

// Real, previously-live false positive found while reviewing a raw
// corpus scoped-run finding (SearchService.php/SectionPopulator.php,
// both going through array_merge with a plain, gapped-but-int-keyed
// array feeding a `list`-typed parameter elsewhere): the *old* check
// here (`is_list_like` alone) wrongly treated an unmarked int-keyed
// argument (`array<int, string>`, list-ness never proven, only never
// disproven) as breaking list-ness. Real PHP's own `array_merge`
// always renumbers every argument's *integer* keys sequentially and
// discards their original values - only a possible *string* key
// actually survives unrenumbered - so the merged result genuinely is
// a list here regardless of whether `$a` itself was already list-
// shaped. Verified against the real vendored `phpstan.phar`: identical
// `list<string>` answer (not `non-empty-list` - `$b` isn't provably
// non-empty either).
#[test]
fn array_merge_of_an_int_keyed_non_list_array_and_a_list_is_a_list() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<int, string> $a
             *  @param list<string> $b
             */
            function f(array $a, array $b): void {
                assertType('list<string>', array_merge($a, $b));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_merge_with_a_literal_array_argument_produces_a_non_empty_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $a
             *  @return non-empty-list<string>
             */
            function f($a): array {
                return array_merge(['x', 'y'], $a);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `array_merge(...$x)` - a spread argument, not a spread array-literal
// item - must not be treated as if the whole array-of-arrays were one
// merge argument, which would give list<list<int>> instead of
// list<int>. Verified via dumpType().
#[test]
fn array_merge_of_a_spread_argument_flattens_one_level() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, list<int>> $byWord
             *  @return list<int>
             */
            function f(array $byWord): array {
                return array_merge(...array_values($byWord));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Merging a property's own default `array{...}` shape with a
// caller-supplied options shape via array_merge(), then reassigning
// the property with the merged result -
// `$this->options = array_merge($this->options, $options)`. Verified
// against real PHPStan's own dumpType(): merging two known shapes does
// the same key-renumbering/overwrite merge a spread array literal
// does, producing a precise merged shape rather than degrading to a
// generic array.
#[test]
fn array_merge_of_two_known_shapes_produces_a_precise_merged_shape() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                /** @var array{a?: int, b?: string} */
                private array $opts = ['a' => 1, 'b' => 'x'];

                /** @param array{a?: int, b?: string} $x */
                public function __construct(array $x) {
                    $this->opts = array_merge($this->opts, $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Merging a known shape with a plain, string-keyed array (e.g.
// `array_merge($defaults, $options)`) stays field-aware instead of
// degrading straight to a generic array - every known field
// name is kept, broadened by the plain array's own element type, and the
// result is marked unsealed since the plain array may also contribute
// unknown keys of its own.
#[test]
fn array_merge_of_a_known_shape_and_a_plain_string_keyed_array_keeps_known_field_names() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a: int} $defaults
             *  @param array<string, string> $options
             */
            function f(array $defaults, array $options): void {
                assertType('array{a: int|string, ...}', array_merge($defaults, $options));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Negative/no-regression case: a shape carrying a canonical int key
// never reaches the new field-aware path (int-key renumbering isn't
// attempted here) - still falls back to today's existing generic
// fallback behavior, unchanged (`non-empty-array<V>`, since the
// generic branch's own pre-existing `any_non_empty` flag picks up the
// shape argument's own guaranteed non-emptiness).
#[test]
fn array_merge_of_an_int_keyed_shape_and_a_plain_array_is_unaffected() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array{0: int, a: string} $shape
             *  @param array<string, string> $options
             */
            function f(array $shape, array $options): void {
                assertType('non-empty-array<int|string>', array_merge($shape, $options));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A plain array whose own key type isn't confidently `string` (here,
// unknown/int-or-string) also stays outside the new path, for the same
// int-key-renumbering reason.
#[test]
fn array_merge_of_a_shape_and_a_plain_array_with_an_unknown_key_type_is_unaffected() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a: int} $shape
             *  @param array<int|string, string> $options
             */
            function f(array $shape, array $options): void {
                assertType('non-empty-array<int|string>', array_merge($shape, $options));
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `array_element_type`/`is_array_like`/`is_list_like`/
// `is_known_non_empty_array` need a `Type::Union` arm - the single most
// common way a real variable's type actually arrives here: a
// control-flow merge (`$x = []; if (...) { foreach (...) { $x[] = ...;
// } }` leaves `$x` a Union of the "0 iterations" `array{}` branch and
// the "≥1" real list branch). Without it, `sort()`, `array_merge()`,
// `foreach`, etc. would silently stop recognizing such a value as an
// array at all - the same class of regression already guarded for
// `Intersection`.
#[test]
fn sort_still_reindexes_to_a_list_when_the_input_is_a_union_from_a_conditional_append() {
    let d = diagnostics_for(
        r#"<?php
            /** @return list<string> */
            function f(bool $flag): array {
                $x = [];
                if ($flag) {
                    foreach (['a', 'b'] as $s) {
                        $x[] = $s;
                    }
                }
                sort($x);
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_by_ref_parameter_declared_wider_than_array_falls_back_to_mixed_not_the_full_union() {
    // An already-array-typed `array $array` parameter passed into
    // `array_walk($array, ...)`, whose real phpstorm-stubs signature is
    // `array_walk(array|object &$array, ...)` - a union wider than
    // what an already-array-typed argument could ever actually become
    // (array_walk never turns an array into an object).
    // Blindly widening to the *full* declared type here regresses
    // real precision (the return type below would start looking
    // possibly-object); only a plain `Type::Array`/`Type::ArrayShape`
    // declared type is trusted, everything else falls back to the
    // same safe `Mixed` the undefined-variable case already uses.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<int|string, mixed> $array
             * @return array<int|string, string>
             */
            function test(array $array): array {
                array_walk($array, function (mixed &$value): void {
                    $value = (string) $value;
                });
                return $array;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_map_over_a_union_from_a_conditional_append_still_produces_a_list() {
    // Same shape as sort()'s own identical test above, but through
    // array_map's own list-preserving `rewrap_with_accessory_markers` -
    // must recognize a `Union`-shaped input too, not just
    // `Intersection`, or it silently drops list-ness the moment the
    // input comes from a control-flow merge (`array{}|list<T>`) instead
    // of a single literal/return.
    let d = diagnostics_for(
        r#"<?php
            /** @return list<int> */
            function f(bool $flag): array {
                $x = [];
                if ($flag) {
                    foreach (['1', '2'] as $s) {
                        $x[] = $s;
                    }
                }
                return array_map(intval(...), $x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn range_with_int_bounds_is_a_non_empty_list_of_int() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function wantsList(array $x): void {}
            wantsList(range(1, 10));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn range_with_single_char_string_bounds_is_a_non_empty_list_of_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<string> $x */
            function wantsList(array $x): void {}
            wantsList(range('a', 'z'));
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_split_with_a_non_literal_pattern_is_a_list_of_string_or_false() {
    // A non-literal pattern/subject still splits into sequential
    // integer keys - phpstorm-stubs' own bare `string[]|false` return
    // type gives up that list-ness entirely.
    let d = diagnostics_for(
        r#"<?php
            /** @return list<string> */
            function split(string $pattern, string $subject): array {
                $list = preg_split($pattern, $subject, -1, PREG_SPLIT_NO_EMPTY);
                if ($list === false) {
                    throw new Exception('failed');
                }
                return $list;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn preg_split_without_no_empty_has_a_plain_string_element_type() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function wantsIntList(array $x): void {}
            function f(string $pattern, string $subject): void {
                $list = preg_split($pattern, $subject);
                if ($list === false) {
                    throw new Exception('failed');
                }
                wantsIntList($list);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn array_unshift_on_an_already_list_like_argument_preserves_list_ness() {
    let d = diagnostics_for(
        r#"<?php
            /** @return list<string> */
            function f(string $head): array {
                $chain = [];
                array_unshift($chain, $head);
                return $chain;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_push_on_a_non_list_argument_does_not_falsely_gain_list_ness() {
    // A real array_push on an arbitrary-keyed array doesn't turn it
    // into a list - only appends fresh integer keys past the existing
    // max, so this must fall through to the generic widening rather
    // than the array_push/array_unshift special case.
    let d = diagnostics_for(
        r#"<?php
            /** @return list<string> */
            function f(): array {
                $x = ['a' => 'foo'];
                array_push($x, 'bar');
                return $x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("return type mismatch")),
        "expected a return-type mismatch, got: {d:?}"
    );
}

#[test]
fn array_shift_on_a_list_preserves_list_ness_but_not_non_emptiness() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param list<int> $x
             * @return list<int>
             */
            function f(array $x): array {
                array_shift($x);
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `array_shift`/`array_pop`'s own by-ref argument widening (just above)
// correctly drops non-emptiness from the array afterward, but must not
// also leak into this expression's own returned value - `array_shift`
// removing the one, definitely-present element of a provably non-empty
// array (`explode()`'s own always-non-empty result) must not look like
// it could return `null`, since the pre-call array's own known
// non-emptiness guarantees the removed/returned element is real.
#[test]
fn array_shift_on_a_provably_non_empty_array_never_returns_null() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f(string $s): void {
                $parts = explode("\x00", $s);
                $first = array_shift($parts);
                needsString($first);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "argument.type"),
        "unexpected diagnostics: {d:?}"
    );
}

// `ArrayFilterRule`/`ArrayValuesRule` - a no-op call, cheap to detect
// once precise element/list-ness inference already exists.

#[test]
fn array_filter_on_an_empty_array_literal_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return array_filter([]);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayFilter.empty"),
        "got: {d:?}"
    );
}

#[test]
fn array_filter_on_an_ambiguous_element_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arrayFilter.")),
        "unexpected diagnostics: {d:?}"
    );
}

// `arrayFilter.same`/`arrayFilter.alwaysEmpty` - unconditional,
// matching real PHPStan's own unconditional level-5 default.

#[test]
fn array_filter_on_a_list_of_objects_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param list<Foo> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayFilter.same"),
        "got: {d:?}"
    );
}

#[test]
fn array_filter_on_a_list_of_true_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<true> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayFilter.same"),
        "got: {d:?}"
    );
}

#[test]
fn array_filter_on_a_list_of_false_is_flagged_always_empty() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<false> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayFilter.alwaysEmpty"),
        "got: {d:?}"
    );
}

#[test]
fn array_filter_on_a_list_of_null_is_flagged_always_empty() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<null> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayFilter.alwaysEmpty"),
        "got: {d:?}"
    );
}

#[test]
fn array_filter_on_a_bare_int_element_type_is_never_flagged() {
    // A bare `int` element type must stay ambiguous (an `int` genuinely
    // can be exactly `0`), not "provably never falsy" - comparing
    // `narrow_type_excluding_falsy`'s output against the original would
    // wrongly treat it as never-falsy, since that function (built for a
    // different purpose, condition-narrowing) leaves a bare
    // `Int`/`Float`/`Bool` unchanged rather than marking it ambiguous.
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arrayFilter.")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn array_filter_on_a_bare_bool_element_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<bool> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arrayFilter.")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn array_filter_on_a_union_mixing_a_falsy_and_a_truthy_member_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param list<Foo|null> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arrayFilter.")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn array_filter_on_a_positive_int_range_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<positive-int> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayFilter.same"),
        "got: {d:?}"
    );
}

#[test]
fn array_filter_on_a_map_of_lists_is_never_flagged_a_list_can_be_empty() {
    // `list<T>` only promises sequential int keys, never non-emptiness -
    // `[]` is a perfectly real, empty (falsy) `list<T>`.
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, list<string>> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arrayFilter.")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn array_filter_on_a_map_of_non_empty_lists_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, non-empty-list<string>> $x */
            function f(array $x): array {
                return array_filter($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayFilter.same"),
        "got: {d:?}"
    );
}

#[test]
fn array_filter_with_a_callback_is_never_flagged() {
    // Real PHPStan itself doesn't attempt to analyze an explicit
    // callback's own logic either.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param list<Foo> $x */
            function f(array $x): array {
                return array_filter($x, fn ($v) => true);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arrayFilter.")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn array_values_on_an_empty_array_literal_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return array_values([]);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayValues.empty"),
        "got: {d:?}"
    );
}

#[test]
fn array_values_on_an_already_list_shaped_array_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function f(array $x): array {
                return array_values($x);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "arrayValues.list"),
        "got: {d:?}"
    );
}

#[test]
fn array_values_on_a_non_list_array_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<string, int> $x */
            function f(array $x): array {
                return array_values($x);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arrayValues.")),
        "unexpected diagnostics: {d:?}"
    );
}

// A by-ref `use (&$values)` capture mutated inside a closure passed to
// `preg_replace_callback()`. A single-pass check would see the closure
// body run exactly once, ending with a real list - real PHP (and real
// PHPStan, confirmed via `dumpType()`) instead treats the callback as
// running zero-to-many times, each run seeing the *previous* run's own
// effect as its entry, converging to a bare `array<mixed>` - not already
// list-shaped, so the `array_values()` call is doing real work, not a
// redundant no-op.
//
// Deliberately an ordinary `if`/`else` here, not an early-`return`ing
// guard clause: `SearchService::positionalCondition`'s own real shape
// mutates `$values` right before a `return` inside the spread-merge
// branch - `check_if`'s own `then_terminates` handling correctly
// discards a terminated branch's `Locals` for what code *after* the
// `if` sees (nothing ever reaches there via that path), but that also
// means a by-ref-captured variable's own mutation made just before the
// `return` never reaches this convergence loop's own end-of-pass read at
// all, unlike a real PHP invocation (where the mutation still takes
// effect in the caller before returning). Confirmed via `dumpType()`
// that real PHPStan still answers the same bare `array` for that exact
// shape too - a further, deeper gap this fix doesn't close, tracked
// separately, not asserted here.
#[test]
fn array_values_after_a_byref_closure_mutated_across_multiple_preg_replace_callback_matches_is_not_flagged()
 {
    let d = diagnostics_for(
        r#"<?php
            function f(string $pattern, string $subject, array $value): array {
                $values = [];
                preg_replace_callback($pattern, static function (array $matches) use (&$values, $value): string {
                    if ($matches[1] ?? null) {
                        $values = [...$values, ...$value];
                    } else {
                        $values[] = $matches[0];
                    }

                    return '?';
                }, $subject);

                return array_values($values);
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier.starts_with("arrayValues.")),
        "unexpected diagnostics: {d:?}"
    );
}

// ArrayPopFunctionReturnTypeExtension/ArrayShiftFunctionReturnTypeExtension:
// investigated and declined - see the long-form doc comment on the
// (reverted) match arm in `builtin_calls.rs` for the two compounding
// reasons (an unsound "provably non-empty" half, plus noise even from
// the safe, always-nullable fallback alone).

// `union_types` must merge two structurally different `Array(key, val)`
// members into one - a dynamic-key index-assignment inside a loop's
// `if`-branch, then `continue`, plus its own fall-through path
// assigning a different value type to the same array, must produce one
// precise `array<(int|string), Foo|false>`, not four separate,
// redundant `array<...>` union members. The key displays as `(int|string)`
// (parenthesized, benevolent) rather than the bare unknown-key shorthand
// since `$key`'s own type here isn't confidently `int` or `string` -
// `dynamic_write_key_type`'s own fallback, mirroring real PHPStan's
// `MixedType`/`UnionType::toArrayKey()`.
//
// `$items` here is a plain `array` parameter with no non-emptiness
// guarantee, so the loop may genuinely run zero times - `$result`'s own
// merge does include `[]` (`array{}`) as a real alternative, matching
// real PHPStan's own `ForeachHandler::processStmt` merging it back in
// whenever `isIterableAtLeastOnce()` isn't a definite "yes" - but that
// merged-in `array{}` must not survive as its own visible union member
// once a general `Array(key, val)` member (`array<(int|string), false|Foo>`
// here) is also present: it's a redundant subtype (an empty array
// vacuously satisfies "every key/value is `key`/`val`-typed" for any
// `key`/`val`), and `union_types` drops it the same way real PHPStan's
// own `TypeCombinator::union()` does - confirmed via `\PHPStan\dumpType()`
// against the real `phpstan/phpstan` phar for this identical shape
// (`$counter = []; foreach (...) { $counter[$k] = ...; }`'s own
// post-loop type is the unqualified `array<...>`, no `array{}`
// alternative visible at all).
#[test]
fn dynamic_key_writes_across_an_if_continue_branch_and_its_fallthrough_merge_into_one_array_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Foo {}
            function f(array $items): void {
                $result = [];
                foreach ($items as $key => $flag) {
                    if ($flag) {
                        $result[$key] = new Foo();
                        continue;
                    }
                    $result[$key] = false;
                }
                assertType('array<(int|string), false|Foo>', $result);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// A dynamic-key read against an array built via dynamic-key writes
// inside a `foreach` (that might run zero times) must not spuriously
// infer `|null`, since the merged `array{}` alternative is a redundant
// subtype of the general `Array(key, val)` the loop body actually
// produces - matches real PHPStan's own `dumpType()` output for this
// identical shape.
#[test]
fn a_dynamic_key_read_after_a_foreach_populated_array_is_not_spuriously_nullable() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $keys */
            function f(array $keys, string $someKey): void {
                $counter = [];
                foreach ($keys as $k) {
                    $counter[$k] = 1;
                }
                assertType('1', $counter[$someKey]);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Sibling case confirming the fix is genuinely scoped to a redundant
// empty-shape-vs-array subtype relationship, not a blanket "loops never
// keep their zero-iteration possibility" rule: a whole-variable
// reassignment (not a dynamic-key write) must still keep its `|null`
// alternative, since `Null` is not a subtype of `String` (this
// project's own union member ordering displays it as `null|string`, a
// cosmetic difference only).
#[test]
fn a_whole_variable_reassigned_inside_a_foreach_still_keeps_its_null_alternative() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $rows */
            function f(array $rows): void {
                $file_path = null;
                foreach ($rows as $row) {
                    $file_path = $row;
                }
                assertType('null|string', $file_path);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// `array_splice()`'s own by-ref effect on its first argument - previously
// unhandled (fell through to the generic declared-param-type widening,
// discarding list-ness/element-type down to a bare `array`). A provably
// non-empty literal replacement alone proves the result non-empty,
// independent of $offset/$length/the array's own prior state - real PHP
// always clamps the removal count to at most count($array), so
// count(after) = count(before) - removed + count(replacement) >=
// count(replacement) unconditionally (confirmed against real PHPStan's
// own ArrayType::spliceArray(), which uses the identical rule).
#[test]
fn array_splice_with_a_non_empty_literal_replacement_yields_a_non_empty_list() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param list<string> $x */
            function f(array $x): void {
                array_splice($x, 0, 1, ['new']);
                assertType('non-empty-list<string>', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn array_splice_with_no_replacement_preserves_list_ness_but_drops_non_emptiness() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param non-empty-list<string> $x */
            function f(array $x): void {
                array_splice($x, 0, 1);
                assertType('list<string>', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn array_splice_with_an_unknown_typed_replacement_stays_conservative() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param list<string> $x
             * @param array<mixed> $replacement
             */
            function f(array $x, array $replacement): void {
                array_splice($x, 0, 1, $replacement);
                assertType('list', $x);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

#[test]
fn array_splice_on_a_non_list_argument_does_not_falsely_gain_list_ness() {
    // Mirrors `array_push_on_a_non_list_argument_does_not_falsely_gain_list_ness`
    // - a real array_splice on an arbitrary-keyed array doesn't turn it
    // into a list, so this must fall through to the generic widening
    // rather than the new special case.
    let d = diagnostics_for(
        r#"<?php
            /** @return list<string> */
            function f(): array {
                $x = ['a' => 'foo'];
                array_splice($x, 0, 1, ['bar']);
                return $x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("return type mismatch")),
        "expected a return-type mismatch, got: {d:?}"
    );
}

// The exact `SearchService::qsearchGetTextTokenSearchSql` corpus shape:
// a by-ref `array_splice()` call with a non-empty literal replacement,
// whose result is then returned against a declared `list<string>` slot
// of a tuple return type.
#[test]
fn array_splice_resolves_the_qsearch_get_text_token_search_sql_shape() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param list<string> $clauses
             * @param list<string> $values
             * @return array{0: list<string>, 1: list<string>}
             */
            function f(array $clauses, array $values): array {
                array_splice($values, 0, 0, ['tsv_search @@ to_tsquery(?)']);
                return [$clauses, $values];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
