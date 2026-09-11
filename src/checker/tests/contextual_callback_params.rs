//! Tests for Phase 53's contextual closure-parameter-type injection,
//! extended beyond `array_map` to `array_filter`/`usort`/`uasort`/
//! `array_walk`/`preg_replace_callback`.
use super::helpers::*;

#[test]
fn array_filter_injects_the_array_s_own_element_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /** @param int[] $ints */
            function f(array $ints): void {
                array_filter($ints, function ($x) {
                    needsString($x);
                    return true;
                });
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn array_filter_with_a_mode_argument_is_never_contextually_checked() {
    // ARRAY_FILTER_USE_KEY/USE_BOTH change the callback's own parameter
    // shape - not attempted, falls back to a plain (unhinted) check.
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $ints */
            function f(array $ints): void {
                array_filter($ints, function ($x) {
                    return true;
                }, ARRAY_FILTER_USE_KEY);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn usort_injects_the_array_s_own_element_type_for_both_params() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /** @param int[] $ints */
            function f(array $ints): void {
                usort($ints, function ($a, $b) {
                    needsString($a);
                    return $a <=> $b;
                });
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

// PHP 8.4's `array_find`/`array_find_key`/`array_any`/`array_all` all
// share the exact same `(array $array, callable $callback): ...` shape as
// `array_walk` just below - real PHPStan's own
// `ArrayFindParameterClosureTypeExtension` types all four identically
// (value, then key). Only `array_any`/`array_all` are tested here (return
// type is already correctly `bool` in the vendored stub, so the callback's
// own parameter typing is the whole gap for those two); `array_find`/
// `array_find_key`'s own missing *return*-type precision is covered in
// `array_functions.rs` instead.
#[test]
fn array_any_injects_the_array_s_own_element_and_key_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /** @param int[] $ints */
            function f(array $ints): bool {
                return array_any($ints, function ($x, $k) {
                    needsString($x);
                    return true;
                });
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn array_all_injects_the_array_s_own_element_and_key_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $i): void {}
            /** @param array<string, int> $items */
            function f(array $items): bool {
                return array_all($items, function ($value, $key) {
                    needsInt($key);
                    return true;
                });
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn array_walk_injects_both_the_element_and_key_type() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $i): void {}
            /** @param array<string, int> $items */
            function f(array $items): void {
                array_walk($items, function ($value, $key) {
                    needsInt($key);
                });
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn preg_replace_callback_injects_a_generic_matches_shape() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $i): void {}
            function f(string $s): string|null {
                return preg_replace_callback('/x/', function ($matches) {
                    needsInt($matches);
                    return $matches[0];
                }, $s);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "argument.type");
}

#[test]
fn a_correctly_typed_usort_callback_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $ints */
            function f(array $ints): void {
                usort($ints, function ($a, $b) {
                    return $a <=> $b;
                });
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
