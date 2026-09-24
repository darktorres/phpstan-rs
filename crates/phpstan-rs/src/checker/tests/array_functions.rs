//! Built-in array_* function return type inference (map/filter/reduce/merge/etc).

use super::helpers::*;

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
