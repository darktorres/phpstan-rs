//! Type narrowing from conditionals, instanceof, is_*, isset, assert, etc.

use super::super::*;
use super::helpers::*;

// `$pos !== false || $this->ierror('Missing .');` where `ierror()`
// has a real PHP `never` return type.
#[test]
fn a_bare_or_statement_with_a_never_returning_right_side_narrows_the_left() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                function ierror(string $msg): never {
                    throw new Exception($msg);
                }
                function f(string $req): string {
                    $pos = strrpos($req, '.');
                    $pos !== false || $this->ierror('Missing .');
                    return substr($req, $pos);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_or_statement_does_not_narrow_when_the_right_side_isnt_never() {
    let d = diagnostics_for(
        r#"<?php
            class C {
                function regular(): bool {
                    return true;
                }
                function f(string $req): string {
                    $pos = strrpos($req, '.');
                    $pos !== false || $this->regular();
                    return substr($req, $pos);
                }
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected $pos to stay unnarrowed (still int|false): {d:?}"
    );
}

// `is_array()`'s registered target is generic `Type::Array(Mixed)`;
// excluding it must also exclude a specific-element `Array(String)`
// member, not just an exact `Array(Mixed)` match, or the full
// `string|array<string>` type would still flow into `explode()`'s
// plain-`string` argument.
#[test]
fn is_array_false_branch_excludes_a_specific_element_array_member_too() {
    let d = diagnostics_for(
        r#"<?php
            /** @param string|array<string> $rawTags */
            function f($rawTags): int {
                if (! is_array($rawTags)) {
                    $rawTags = explode(',', $rawTags);
                }
                return count($rawTags);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A ternary's own `union_types(String, StringLiteral(""))` must
// collapse to plain `String`, or `$rewritten` would never match
// `str_replace`'s subject-narrowing `== Type::String` check again.
#[test]
fn str_replace_narrows_a_subject_built_from_a_ternarys_string_and_string_literal_branches() {
    let d = diagnostics_for(
        r#"<?php
            function f($rewritten): string {
                $rewritten = is_string($rewritten) ? $rewritten : '';
                $rewritten = str_replace('//', '/', $rewritten);
                return strtoupper($rewritten);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `narrow_type_excluding` must exclude each of `is_scalar`'s 4-member
// target individually, not filter by exact structural equality against
// the whole union - no real member is ever equal to the whole 4-member
// union, so `!is_scalar($x)` would never exclude anything.
#[test]
fn is_scalar_false_branch_actually_excludes_the_scalar_members() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|array $x */
            function f($x): array {
                if (! is_scalar($x)) {
                    return $x;
                }
                return [];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `apply_phpunit_assertion_narrowing` shares the same requirement as
// `is_array()` above: `assertIsArray($x)` with `$x: array<string>|int`
// must keep the specific element type, not fall back to generic `array`.
#[test]
fn assert_is_array_keeps_a_specific_element_type_not_the_generic_array() {
    let d = diagnostics_for(
        r#"<?php
            class T {
                /** @param array<string>|int $x */
                function f($x): array {
                    self::assertIsArray($x);
                    return $x;
                }
                static function assertIsArray($x): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn guard_clause_narrows_nullable_param_after_early_return() {
    // The single most common narrowing idiom in real PHP: without
    // recognizing that the `if` branch terminates, the narrowed-to-null
    // state from that branch would incorrectly leak past the guard.
    let d = diagnostics_for(
        r#"<?php
            function greet(?string $name): string {
                if ($name === null) {
                    return "stranger";
                }
                return $name;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn guard_clause_narrows_in_else_branch_too() {
    let d = diagnostics_for(
        r#"<?php
            function greet(?string $name): string {
                if ($name === null) {
                    $name = "stranger";
                } else {
                    // $name narrowed to string here already, this is a no-op
                }
                return $name;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_string_narrowing_catches_misuse_inside_the_guarded_branch() {
    // Before narrowing, an untyped param stays Mixed forever and this
    // bug would have been silently skipped rather than caught.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function process($value) {
                if (is_string($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("needsInt"));
}

#[test]
fn is_callable_narrowing_catches_misuse_inside_the_guarded_branch() {
    // `is_callable` fits `narrowing_type_for_predicate`'s table: a
    // single-arg call narrowing a bare variable to one concrete Type.
    // Other predicates like `class_exists`/`function_exists`/
    // `in_array`/`array_key_exists` don't fit - they're either 2-arg
    // calls or don't narrow a value's type the way `is_string`/
    // `is_array` do. `method_exists`/`property_exists` instead use a
    // structurally different mechanism (`Locals::guarded_methods`/
    // `guarded_properties`, mirroring `guarded_functions` - see the
    // tests below), since they assert an independent capability fact
    // about the receiver rather than narrowing its own type.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function process($value) {
                if (is_callable($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsInt"), "got: {d:?}");
}

// Mirrors real PHPStan's own `MethodExistsTypeSpecifyingExtension`
// (`Accessory\HasMethodType`): `if (method_exists($image, 'destroy'))
// { $image->destroy(); }` must not flag a call to a method not on the
// receiver's own declared type.
#[test]
fn method_exists_guards_a_call_to_a_method_not_on_the_declared_type() {
    let d = diagnostics_for(
        r#"<?php
            interface ImageInterface1 {}
            class ImageGd1 implements ImageInterface1 {
                public function destroy(): bool { return true; }
            }
            function f(ImageInterface1 $image): void {
                if (method_exists($image, 'destroy')) {
                    $image->destroy();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn property_exists_guards_an_access_to_a_property_not_on_the_declared_type() {
    let d = diagnostics_for(
        r#"<?php
            interface HasBag1 {}
            class RealBag1 implements HasBag1 {
                public string $bag = '';
            }
            function f(HasBag1 $x): void {
                if (property_exists($x, 'bag')) {
                    echo $x->bag;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The guard is name-specific, not a blanket suppression of every
// missing-member diagnostic on the receiver.
#[test]
fn method_exists_with_a_different_name_does_not_guard_an_unrelated_call() {
    let d = diagnostics_for(
        r#"<?php
            interface ImageInterface2 {}
            function f(ImageInterface2 $image): void {
                if (method_exists($image, 'destroy')) {
                    $image->somethingElse();
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.notFound"),
        "got: {d:?}"
    );
}

// The guard is genuinely Locals-scoped (per apply_narrowing/merge_scopes),
// not accidentally promoted to Checker-wide state - it must not leak
// past the branch that proved it.
#[test]
fn method_exists_guard_does_not_leak_past_the_branch() {
    let d = diagnostics_for(
        r#"<?php
            interface ImageInterface3 {}
            function f(ImageInterface3 $image): void {
                if (method_exists($image, 'destroy')) {
                    $image->destroy();
                }
                $image->destroy();
            }
        "#,
    );
    assert_eq!(
        d.iter()
            .filter(|x| x.identifier == "method.notFound")
            .count(),
        1,
        "got: {d:?}"
    );
}

// Mirrors guarded_functions's own already-tested && composition - the
// right operand of && is checked against the left operand's own
// narrowed locals.
#[test]
fn method_exists_composes_with_the_right_side_of_an_and_expression() {
    let d = diagnostics_for(
        r#"<?php
            interface ImageInterface4 {}
            function f(ImageInterface4 $image): void {
                if (method_exists($image, 'destroy') && $image->destroy()) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Exercises the guard set's own merge_scopes intersection: only one
// branch of an if/else proves the guard, so it must not survive past
// the merge point.
#[test]
fn method_exists_guard_from_only_one_branch_does_not_survive_the_merge() {
    let d = diagnostics_for(
        r#"<?php
            interface ImageInterface5 {}
            function f(ImageInterface5 $image, bool $flag): void {
                if ($flag) {
                    if (method_exists($image, 'destroy')) {
                    }
                } else {
                }
                $image->destroy();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.notFound"),
        "got: {d:?}"
    );
}

// `proc_open()`/`fsockopen()` return `resource|false`, and code
// commonly guards them with `if (! is_resource($x)) { throw ...; }`
// before `fclose`/`fwrite`/`fread`/`proc_close`/etc. - `is_resource`
// must be in `narrowing_type_for_predicate`'s table.
#[test]
fn is_resource_narrowing_excludes_false() {
    let d = diagnostics_for(
        r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $proc = open();
                if (! is_resource($proc)) {
                    throw new Exception('failed');
                }
                fclose($proc);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_scalar_narrows_to_the_four_scalar_types() {
    // Unlike is_numeric (deliberately not narrowed at all - real
    // PHPStan's own int|float|numeric-string result has no
    // representation here), is_scalar's real set is exactly
    // int|float|string|bool, with no such approximation gap.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function process(mixed $value) {
                if (is_scalar($value)) {
                    return needsInt($value);
                }
                return 0;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("needsInt"), "got: {d:?}");
}

// `count()` on anything but `array|Countable` is a real PHP 8+
// `TypeError`, so `is_countable` must narrow via
// `narrowing_type_for_predicate` before a following `count()` call.
// `array|null` (not a hardcoded `null` literal) - a provably-`null`
// subject would now also be a genuine `function.impossibleType` finding
// of its own (`is_countable(null)` is unconditionally `false`), which
// isn't what this test means to exercise.
#[test]
fn is_countable_narrows_a_possibly_null_value_before_count() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array<int>|null $tags */
            function f($tags): int {
                if (is_countable($tags) and count($tags) > 1) {
                    return count($tags);
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_countable_false_branch_excludes_array_and_countable() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|array<int> $x */
            function f($x): int {
                if (! is_countable($x)) {
                    return $x;
                }
                return count($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Narrowing sources for the accessory refinement types, each verified
// against real PHPStan's own dumpType().
#[test]
fn not_equal_empty_string_narrows_to_non_empty_string() {
    let d = diagnostics_for(
        r#"<?php
            function needsNonEmpty(): void {}
            /** @param non-empty-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if ($x !== '') {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `$x !== []` (the array equivalent of the string idiom just above)
// needs `is_known_single_value` to recognize an empty array literal
// as exhaustively single-valued, the same way it already does a
// literal string/int.
#[test]
fn not_equal_empty_array_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink(array $x): void {}
            /** @param array<int> $x */
            function f(array $x): void {
                if ($x !== []) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn not_equal_empty_array_narrows_an_already_typed_list_to_non_empty() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-list<int> $x */
            function sink(array $x): void {}
            /** @param list<int> $x */
            function f(array $x): void {
                if ($x !== []) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn not_empty_on_a_string_narrows_to_non_falsy_string_not_merely_non_empty() {
    // empty() is also false for '0' - real PHPStan narrows !empty($x)
    // to non-falsy-string, not just non-empty-string.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!empty($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A redundant nested `!empty($x)` guard on an already-`non-empty-string`
// value must still upgrade it to the strictly narrower `non-falsy-string`
// (excludes the literal `"0"` too), same as the bare-string case above -
// mirrors `is_numeric_upgrades_an_already_non_empty_string_to_numeric_
// string_too`'s own "stack onto an already-refined string" shape.
#[test]
fn not_empty_upgrades_an_already_non_empty_string_to_non_falsy_string_too() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            /** @param non-empty-string $x */
            function f($x): void {
                if (!empty($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Negative control: without the redundant guard, a plain `non-empty-string`
// must still be rejected by a `non-falsy-string`-typed sink - proves the
// fix above is additive (only narrows inside the guard), not a blanket
// relaxation of `non-empty-string` into `non-falsy-string` everywhere.
#[test]
fn a_non_empty_string_without_an_empty_guard_is_still_rejected_by_a_non_falsy_string_sink() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            /** @param non-empty-string $x */
            function f($x): void {
                sink($x);
            }
        "#,
    );
    assert!(!d.is_empty(), "expected a diagnostic, got none");
}

#[test]
fn not_empty_on_an_array_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (!empty($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn count_greater_than_zero_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (count($x) > 0) {
                    sink($x);
                }
            }
            function g(array $x): void {
                if (count($x) >= 1) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `upgrade_array_to_non_empty` needs a `Type::Union` arm: a loop-built
// array whose type is a `Union` including an empty-`ArrayShape`
// alternative (the loop might never have run, leaving the array at its
// pre-loop `[]`) must have that alternative stripped once
// `count($x) > 0` disproves it, not fall through unchanged.
#[test]
fn count_greater_than_zero_narrows_a_union_with_an_empty_shape_alternative() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $items): void {
                $collected = [];
                foreach ($items as $item) {
                    $collected[] = $item;
                }
                if (count($collected) > 0) {
                    sink($collected);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `if (count($x) < 1) { bail(); }` (a `never`-typed early-exit idiom)
// proves the exact same "`$x` is non-empty" fact on the surviving path
// as the direct `count($x) > 0` form above - just phrased as the false
// branch of the mirrored comparison.
#[test]
fn count_less_than_one_in_a_terminating_branch_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function bail(): never { throw new \Exception(); }
            function f(array $x): void {
                if (count($x) < 1) {
                    bail();
                }
                sink($x);
            }
            function g(array $x): void {
                if (count($x) <= 0) {
                    bail();
                }
                sink($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Reversed operand order: `0 < count($x)`/`1 <= count($x)` prove the
// identical fact `count($x) > 0`/`count($x) >= 1` already do.
#[test]
fn reversed_count_comparison_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (0 < count($x)) {
                    sink($x);
                }
            }
            function g(array $x): void {
                if (1 <= count($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Reversed operand order for the terminating-branch mirror too: `1 >
// count($x)`/`0 >= count($x)` prove the identical fact `count($x) < 1`/
// `count($x) <= 0` already do.
#[test]
fn reversed_count_comparison_in_a_terminating_branch_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function bail(): never { throw new \Exception(); }
            function f(array $x): void {
                if (1 > count($x)) {
                    bail();
                }
                sink($x);
            }
            function g(array $x): void {
                if (0 >= count($x)) {
                    bail();
                }
                sink($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A bare `count($x)` truthiness check (no explicit `> 0`/`>= 1`
// comparison) - the same "provably non-empty" fact the comparison form
// above already proves.
#[test]
fn a_bare_count_truthiness_check_narrows_to_non_empty_array() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (count($x)) {
                    sink($x);
                }
            }
            function g(array $x): void {
                while (count($x)) {
                    sink($x);
                    break;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `array_is_list($x)` - a common `is_array($x) && array_is_list($x)`
// idiom.

#[test]
fn array_is_list_narrows_to_a_real_list() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (array_is_list($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_is_list_composes_with_the_is_array_guard() {
    let d = diagnostics_for(
        r#"<?php
            /** @param list<mixed> $x */
            function sink($x): void {}
            function f(mixed $x): void {
                if (is_array($x) && array_is_list($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn failing_array_is_list_proves_nothing_more_specific() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (array_is_list($x)) {
                } else {
                    sink($x);
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "the false branch shouldn't be narrowed to non-empty at all: {d:?}"
    );
}

// `str_contains`/`str_starts_with`/`str_ends_with` with a non-empty
// literal needle, mirroring real PHPStan's own
// `StrContainingTypeSpecifyingExtension`.

#[test]
fn str_contains_with_a_literal_needle_narrows_the_haystack_to_non_falsy_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (str_contains($x, 'foo')) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn str_starts_with_and_str_ends_with_also_narrow_the_haystack() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $a, string $b): void {
                if (str_starts_with($a, 'foo')) {
                    sink($a);
                }
                if (str_ends_with($b, 'bar')) {
                    sink($b);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn str_contains_with_a_literal_zero_needle_narrows_to_non_empty_not_non_falsy() {
    // `"0"` is itself falsy - a haystack containing it can't be proven
    // non-falsy (it could be exactly `"0"`), only non-empty.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (str_contains($x, '0')) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "a '0'-containing haystack isn't provably non-falsy: {d:?}"
    );
}

#[test]
fn str_contains_composes_with_a_negated_guard_clause() {
    // `if (!str_contains($x, 'foo')) { return; }` - only the code past
    // the guard should see the upgrade.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!str_contains($x, 'foo')) {
                    return;
                }
                sink($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn str_contains_with_a_non_literal_needle_narrows_nothing() {
    let d = diagnostics_for(
        r#"<?php
            /** @param non-falsy-string $x */
            function sink($x): void {}
            function f(string $x, string $needle): void {
                if (str_contains($x, $needle)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "a non-literal needle shouldn't prove anything about the haystack: {d:?}"
    );
}

#[test]
fn a_guard_clause_on_a_negated_bare_count_still_narrows_past_it() {
    // `!count($x)` composes through the existing `Unary(Not)` arm
    // (`apply_narrowing(inner, locals, !branch_true)`) - the guard's
    // own true branch (`count($x)` false, i.e. genuinely empty) isn't
    // narrowed by this arm at all (it requires `branch_true`), but the
    // surviving continuation past a terminating guard correctly ends up
    // narrowing `count($x)`'s own true branch instead, same as any
    // other guard-clause idiom already composes.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-array<int> $x */
            function sink($x): void {}
            function f(array $x): void {
                if (!count($x)) {
                    return;
                }
                sink($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `is_iterable()` narrows to `array|Traversable`, real PHP's own
// native `iterable` type hint's own definition.
#[test]
fn is_iterable_narrows_a_mixed_typed_value_to_array_or_traversable() {
    let d = diagnostics_for(
        r#"<?php
            function needsIterable(iterable $x): void {}
            function test(mixed $x): void {
                if (is_iterable($x)) {
                    needsIterable($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A fully-qualified predicate call (`\is_array($x)`, a common
// defensive-global-namespace style) keeps its leading backslash in
// `ExprKind::Name`, so `apply_narrowing`'s own dispatch must normalize
// it before the bare string comparison, unlike ordinary call resolution
// (`name_ctx::resolve_function_candidates`), which already handles
// this correctly elsewhere.
#[test]
fn a_fully_qualified_predicate_call_still_narrows() {
    let d = diagnostics_for(
        r#"<?php
            function needsArray(array $x): void {}
            function test(mixed $value): void {
                if (is_iterable($value)) {
                    if (\is_array($value)) {
                        needsArray($value);
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_numeric_narrows_a_plain_string_to_numeric_string() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (is_numeric($x)) {
                    sink($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_numeric_false_branch_leaves_a_string_unchanged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            function f(string $x): void {
                if (!is_numeric($x)) {
                    return;
                }
                sink($x);
            }
            function g(string $x): void {
                if (is_numeric($x)) {
                    return;
                }
                sink($x);
            }
        "#,
    );
    // g() must still be flagged: the false branch is plain string,
    // not numeric-string.
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// `is_numeric()`'s upgrade must stack onto an already-refined string
// (`non-empty-string` here, e.g. from a `preg_match()` capture group),
// not silently no-op just because it wasn't a bare `string`.
#[test]
fn is_numeric_upgrades_an_already_non_empty_string_to_numeric_string_too() {
    let d = diagnostics_for(
        r#"<?php
            /** @param numeric-string $x */
            function sink($x): void {}
            /** @param non-empty-string $x */
            function f($x): void {
                if (!is_numeric($x)) {
                    throw new \RuntimeException('not numeric');
                }
                sink($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_narrows_to_the_checked_class() {
    let d = diagnostics_for(
        r#"<?php
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if ($animal instanceof Cat) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_narrows_to_the_checked_class_in_a_namespaced_file() {
    // The bare-name twin above passes trivially even if namespace
    // resolution were completely broken, since an empty namespace
    // makes FQN == bare name. This is the version that actually
    // exercises it: `useCat(Cat $c)`'s param type is resolved to an
    // FQN at *registry-build* time (resolve_names_in_type), while the
    // `instanceof Cat` narrowing resolves it separately at *check*
    // time (name_ctx.resolve) - both need to land on the exact same
    // string for is_assignable's `a == b` class-name comparison to
    // ever succeed.
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if ($animal instanceof Cat) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `instanceof`'s true branch must not unconditionally replace the
// checked variable's type with just the target class/interface,
// discarding whatever it was already known to be - correct for the
// common down-cast (a known parent narrowed to a known child) or
// nullable-narrowing (`?Foo` narrowed to `Foo`) shape, but wrong when
// the target is an unrelated sibling interface: a parameter passing
// through several independent, sequential `if ($x instanceof
// SomeOtherInterface)` checks must still be usable as its original
// type afterward, not collapse into a nonsensical accumulated union of
// bare, unrelated interfaces.
#[test]
fn instanceof_against_an_unrelated_interface_keeps_the_original_type_too() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            interface HasPageAssets {}
            function needsView(View $v): void {}
            function f(View $view): void {
                if ($view instanceof HasPageAssets) {
                    // no-op
                }
                needsView($view);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_against_an_unrelated_interface_also_narrows_to_the_new_interface() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            interface HasPageAssets { public function pageAssets(): array; }
            function f(View $view): void {
                if ($view instanceof HasPageAssets) {
                    $view->pageAssets();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The `MethodCall` case just above only exercises `check_expr`'s own
// separate MethodCall arm (diagnostics/narrowing purposes) -
// `infer_expr` (the core type-*inference* path, used whenever a method
// call's own return type needs computing without also running full
// diagnostics) needs the identical `Union`/`Intersection` handling too,
// or it silently collapses the whole call's return type to
// `Type::Mixed(false)`.
#[test]
fn a_method_call_on_an_intersection_receiver_infers_its_real_return_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            interface View {}
            class HasValue { public function value(): int { return 1; } }
            function f(View $view): void {
                if ($view instanceof HasValue) {
                    assertType('int', $view->value());
                    $x = $view->value() + 1;
                    assertType('int', $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `narrow_instanceof_true` always builds `Intersection([original_type,
// target_type])` - the broader, pre-existing type first, the
// just-`instanceof`-checked, narrower type last. Member lookup must
// search in reverse order, checking the narrower, just-checked type
// first: searching forward would let a same-named method declared on
// both the broader original type and the narrower target resolve to
// the broader type's own possibly-less-precise signature, producing a
// bogus argument-type mismatch when the narrower type overrides it
// more precisely.
#[test]
fn a_method_declared_on_both_intersection_members_prefers_the_narrower_types_own_override() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            interface View { public function value(): int|string; }
            class HasValue { public function value(): int { return 1; } }
            function f(View $view): void {
                if ($view instanceof HasValue) {
                    assertType('int', $view->value());
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `check_expr`'s own separate `MethodCall` arm (not `infer_expr`'s,
// covered just above) - `check_expr`'s own `recv_class` extraction is
// also used to resolve
// the method's real signature/owner for argument-type checking and
// visibility/`@internal`/`@deprecated` - it must prefer the narrower,
// just-checked member over the broader one, the same reverse-search
// requirement as `infer_expr` above. `assertType` alone can't observe
// this - it reads `infer_expr`'s own result, not `check_expr`'s - so
// this test instead checks a `check_expr`-only side effect: a
// same-named method declared on both intersection members with
// different visibility. `HasValue::value()` (the narrower, correctly-
// preferred member) is private; `View::value()` (the broader member)
// is public.
#[test]
fn a_method_call_through_an_intersection_checks_visibility_against_the_narrower_types_own_override()
{
    let d = diagnostics_for(
        r#"<?php
            interface View { public function value(): int; }
            class HasValue {
                private function value(): int { return 1; }
                public function useIt(): int { return $this->value(); }
            }
            function f(View $view): void {
                if ($view instanceof HasValue) {
                    $view->value();
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "method.private"),
        "got: {d:?}"
    );
}

// Same refinement as the property-read test below: a `Union`-typed
// method call unions every resolving member's own return type together,
// rather than just using whichever member happened to come first.
#[test]
fn a_method_call_on_a_union_of_two_classes_unions_their_return_types() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class ReturnsInt { public function get(): int { return 1; } }
            class ReturnsString { public function get(): string { return "x"; } }
            function f(ReturnsInt|ReturnsString $v): void {
                assertType('int|string', $v->get());
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `check_expr`'s/`infer_expr`'s own `PropertyAccess` arms need a
// `Union`/`Intersection` arm, not just `Object`/`EnumCase`/`ObjectShape`
// - a property access (unlike the `MethodCall` case just above) on the
// same kind of `View&HasPageAssets` intersection must not fall
// straight to `Type::Mixed(false)` just because the receiver was narrowed via
// `instanceof` from an interface.
#[test]
fn instanceof_against_an_unrelated_class_also_resolves_a_property_on_the_intersection() {
    // Interfaces can't declare properties in real PHP, so the
    // property-bearing side here is a plain, otherwise-unrelated class.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            interface View {}
            class ClassMethod { public array $stmts = []; }
            function f(View $view): void {
                if ($view instanceof ClassMethod) {
                    assertType('array', $view->stmts);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A `Union`-typed property access needs different handling than an
// `Intersection`'s "first member found wins" shortcut - the value is
// only ever one of `A`/`B` at a time, so two members declaring the
// property with
// different types genuinely means the read's own type is the union of
// both, not just whichever member happened to come first.
#[test]
fn a_property_read_through_a_union_of_two_classes_unions_their_declared_types() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class HasIntFlag { public int $flag = 1; }
            class HasStringFlag { public string $flag = "x"; }
            function f(HasIntFlag|HasStringFlag $v): void {
                assertType('int|string', $v->flag);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The visibility/`@internal`/`@deprecated` checks need a `Union`/
// `Intersection` case too, not just `Type::Object` directly, matching
// `MethodCall`'s own identical checks, which already extract a
// representative class from a `Union`/`Intersection` receiver.
// Deliberately verifies the property lives on the second intersection
// member (`Foo`, not the first-checked `View`) - a plain "first Object
// member found" shortcut would still miss this.
#[test]
fn instanceof_against_an_unrelated_class_still_flags_a_private_property_on_the_intersection() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            class Foo {
                private int $secret = 1;
                public function useIt(): int { return $this->secret; }
            }
            function f(View $view): void {
                if ($view instanceof Foo) {
                    echo $view->secret;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.private"),
        "got: {d:?}"
    );
}

// The write-side counterpart of the test above: `check_property_
// assignment` (`checker/assignment.rs`) needs the same `Union`/
// `Intersection` handling, or it silently skips every one of its checks
// (visibility, readonly, `@internal`/`@deprecated`, and the assigned
// value's own type) for such a target. Both the visibility violation
// and the type mismatch are checked in one test since both come from
// the same code path.
#[test]
fn instanceof_against_an_unrelated_class_still_checks_a_write_to_a_property_on_the_intersection() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            class Foo {
                private int $secret = 1;
                public function useIt(): int { return $this->secret; }
            }
            function f(View $view): void {
                if ($view instanceof Foo) {
                    $view->secret = "not an int";
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "property.private"),
        "got: {d:?}"
    );
    assert!(
        d.iter().any(|x| x.identifier == "assign.propertyType"),
        "got: {d:?}"
    );
}

// A nullable property/variable checked against an unrelated interface:
// `?Socket instanceof ResourceStream` must not fall all the way through
// to "just trust the checked class" (losing `Socket` entirely, not
// merely its own nullability) just because `is_object_like` sees the
// still-nullable `Union([Socket, Null])` - `null` needs dropping first
// (a true `instanceof` can never itself be `null`), not treated as
// disqualifying the rest of the union from being object-like.
#[test]
fn instanceof_against_an_unrelated_interface_on_a_nullable_type_keeps_the_original_type_too() {
    let d = diagnostics_for(
        r#"<?php
            namespace Amp;
            interface ResourceStream { public function unreference(): void; }
            interface Socket { public function read(): ?string; }
            function f(?Socket $socket): void {
                if ($socket instanceof ResourceStream) {
                    $socket->unreference();
                }
                $socket?->read();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// When the pre-narrowing type is something that could never actually be
// `instanceof` anything at all (`Null`, a scalar, ... - itself a sign of
// a separate upstream inference gap, not narrowed here), building a
// real Intersection out of it produces nonsense like `null&ImageId` -
// worse than simply trusting the checked class alone. Falls back to
// that whenever the pre-narrowing type isn't a plain, single
// already-known class/interface.
#[test]
fn instanceof_true_on_a_provably_non_object_type_does_not_build_a_nonsense_intersection() {
    let d = diagnostics_for(
        r#"<?php
            class Widget15 { public int $value = 0; }
            /** @param null $x */
            function f($x): int {
                if ($x instanceof Widget15) {
                    return $x->value;
                }
                return 0;
            }
        "#,
    );
    assert!(
        d.iter().all(|x| x.identifier.starts_with("instanceof.")),
        "unexpected diagnostics: {d:?}"
    );
}

// Three independent, sequential `instanceof` checks on the same
// parameter: after the first check's own if/no-else merge turns the
// variable into a real Union of object-like branches, a second
// `instanceof` check against an unrelated interface must still combine
// correctly, not fall back to bare-trust-the-target the moment current
// stops being a single plain Object.
#[test]
fn a_second_independent_instanceof_check_still_combines_after_the_first_ones_own_merge() {
    let d = diagnostics_for(
        r#"<?php
            interface View2 {}
            interface HasPageAssets2 {}
            interface ExposesPageData2 { public function exposedPageData(): array; }
            function needsView(View2 $v): void {}
            function f(View2 $view): void {
                if ($view instanceof HasPageAssets2) {
                    // no-op
                }
                if ($view instanceof ExposesPageData2) {
                    $view->exposedPageData();
                }
                needsView($view);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_narrowing_down_a_class_hierarchy_still_replaces_the_type_entirely() {
    // The common down-cast case must keep working exactly as before -
    // no spurious Cat&Animal intersection when Cat already IS-A
    // Animal.
    let d = diagnostics_for(
        r#"<?php
            class Animal {}
            class Cat extends Animal { public function meow(): string { return "meow"; } }
            function f(Animal $animal): string {
                if ($animal instanceof Cat) {
                    return $animal->meow();
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_false_branch_drops_the_checked_class_from_a_union() {
    let d = diagnostics_for(
        r#"<?php
            class Mysqli {}
            class PgConnection {}
            function needsPgConnection(PgConnection $c): void {}
            function dbEscape(Mysqli|PgConnection $db): void {
                if ($db instanceof Mysqli) {
                    return;
                }
                needsPgConnection($db);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_false_branch_of_the_exact_declared_class_is_dead_code() {
    let d = diagnostics_for(
        r#"<?php
            class Cat {}
            function needsInt(int $n): void {}
            function handle(Cat $c): void {
                if (! ($c instanceof Cat)) {
                    needsInt($c);
                }
            }
        "#,
    );
    assert!(
        d.iter().all(|x| x.identifier.starts_with("instanceof.")),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn instanceof_false_branch_against_an_unrelated_class_leaves_the_type_untouched() {
    let d = diagnostics_for(
        r#"<?php
            class Cat {}
            class Dog {}
            function needsCat(Cat $c): void {}
            function handle(Cat|Dog $animal): void {
                if ($animal instanceof Dog) {
                    return;
                }
                needsCat($animal);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `while ($x instanceof IteratorAggregate) { $x = $x->getIterator(); }`
// - PHP's own `Traversable` is a marker interface userland code can
// never implement directly, only through one of its exactly two direct
// subinterfaces (`Iterator`, `IteratorAggregate`), so `narrow_type_
// excluding` needs a special case: excluding one after the loop
// structurally proves the other, and the loop-exit type must not stay
// bare `Traversable` (which declares no methods of its own), wrongly
// flagging every one of `rewind()`/`valid()`/`current()` as undefined.
#[test]
fn a_while_loop_excluding_iterator_aggregate_narrows_traversable_to_iterator() {
    let d = diagnostics_for(
        r#"<?php
            function handle(Traversable $x): void {
                while ($x instanceof IteratorAggregate) {
                    $x = $x->getIterator();
                }
                $x->rewind();
                if ($x->valid()) {
                    $x->current();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `in_array_target_type` must not require every haystack element to be
// a plain `ExprKind::Literal`: a haystack mixing literals with a bare
// empty-array literal (`[]`, parsed as `ExprKind::ArrayLiteral`, not
// `Literal`) must not bail the whole function to `None`, silently
// narrowing none of the haystack's values, even though `[]` is exactly
// as statically provable a single exact type (`Type::ArrayShape(vec![])`)
// as any scalar literal.
#[test]
fn in_array_strict_with_an_empty_array_literal_in_the_haystack_still_narrows() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function f(array|string|null $raw): void {
                if (! in_array($raw, [null, '0', '', []], true)) {
                    if (! is_array($raw)) {
                        needsString($raw);
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn in_array_strict_narrows_the_true_branch_to_the_haystacks_literal_union() {
    // `$post['status'] ?? null` infers Mixed (an unshaped `array`'s own
    // element type; see `widen_coalesce_result`'s own `?? null` no-op
    // case), so this exercises `narrow_type_to`'s non-Union fallback
    // too.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<array-key, mixed> $post
             */
            function fromArray(array $post): string {
                $status_raw = $post['status'] ?? null;
                $status = in_array($status_raw, ['public', 'private'], true) ? $status_raw : '';
                return $status;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `$x ?? null` is a real no-op - if `$x` is null the result is null
// either way, and otherwise it's exactly `$x` - so its own type must
// be exactly `$x`'s own (unnarrowed) type, not the literal `Type::Null`,
// even when the right side is a bare `null`. Otherwise an unshaped
// array read (`$post['selection'] ?? null`, `$post: array` with no
// shape, hence `Mixed`) would collapse to provably-null, so a later
// `is_array(...)` check on it - genuinely checking a real `Mixed`
// value - would never have a chance to actually narrow anything.
#[test]
fn coalesce_with_a_null_fallback_keeps_the_left_sides_own_type() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<array-key, mixed> $post
             * @param array<mixed> $default
             */
            function f(array $post, array $default): array {
                $selection_raw = $post['selection'] ?? null;
                return is_array($selection_raw) ? $selection_raw : $default;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The no-op reasoning above only actually holds for a bare variable on
// the left - an array-index access whose own value type is concrete
// and non-nullable (unlike the Mixed case above) must still have
// `Null` unioned back in, since `infer_expr` for a plain
// `array<int, string>`-typed array's index access has no notion of "the
// key might not exist" at all, the exact real-PHP case `?? null` exists
// to guard against. Otherwise `$val = $data[$index] ?? null; return
// $val ?? '';` would flag the second, genuinely-needed `?? ''` as
// checking an already-non-nullable variable.
#[test]
fn coalesce_with_a_null_fallback_still_unions_null_for_a_non_nullable_array_access() {
    let d = diagnostics_for(
        r#"<?php
            /** @return array<int, string> */
            function group(): array { return []; }
            function fromLangArray(int $index): string {
                $data = group();
                $val = $data[$index] ?? null;
                return $val ?? '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn in_array_without_the_strict_flag_narrows_nothing() {
    let d = diagnostics_for(
        r#"<?php
            function fromArray(?string $status_raw): string {
                $status = in_array($status_raw, ['public', 'private']) ? $status_raw : '';
                return $status;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("null"), "got: {d:?}");
}

#[test]
fn in_array_strict_with_a_non_literal_haystack_element_narrows_to_the_element_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function fromArray(?string $status_raw, string $other): string {
                if (in_array($status_raw, [$other, 'public'], true)) {
                    assertType('string', $status_raw);
                } else {
                    assertType('string|null', $status_raw);
                }
                $status = in_array($status_raw, [$other, 'public'], true) ? $status_raw : '';
                return $status;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_exists_true_branch_keeps_only_shapes_declaring_the_key() {
    let d = diagnostics_for(
        r#"<?php
            function needsBValue(string $b): void {}
            /**
             * @param array{a: int} $withA
             * @param array{b: string} $withB
             */
            function handle(array $withA, array $withB, bool $cond): void {
                $x = $cond ? $withA : $withB;
                if (array_key_exists('b', $x)) {
                    needsBValue($x['b']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_exists_false_branch_keeps_only_shapes_lacking_the_key() {
    let d = diagnostics_for(
        r#"<?php
            function needsAValue(int $a): void {}
            /**
             * @param array{a: int} $withA
             * @param array{b: string} $withB
             */
            function handle(array $withA, array $withB, bool $cond): void {
                $x = $cond ? $withA : $withB;
                if (! array_key_exists('b', $x)) {
                    needsAValue($x['a']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The dispatch above must not require a literal `Literal::String` key
// specifically: `array_key_exists(0, $x)` (an int-literal key - just as
// common a shape as a string-literal one, e.g. a tuple/numeric-shape
// check) must also trigger this narrowing, excluding a `Union` member
// that couldn't possibly have offset `0` in the first place (it only
// has string-named fields).
#[test]
fn array_key_exists_with_an_int_literal_key_also_narrows() {
    let d = diagnostics_for(
        r#"<?php
            function needsPair(string $a, string $b): void {}
            /**
             * @param array{0: string, 1: string} $pair
             * @param array{range: array{0: int, 1: int}} $range
             */
            function handle(array $pair, array $range, bool $cond): void {
                $x = $cond ? $pair : $range;
                if (array_key_exists(0, $x)) {
                    needsPair($x[0], $x[1]);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_exists_on_a_merely_optional_key_narrows_neither_branch() {
    let shape = Type::ArrayShape(vec![("a".to_string(), Type::Int, true)]);
    assert_eq!(
        narrowing::narrow_array_shape_by_key_presence(&shape, "a", true),
        shape
    );
    assert_eq!(
        narrowing::narrow_array_shape_by_key_presence(&shape, "a", false),
        shape
    );
}

#[test]
fn array_key_exists_on_a_plain_array_leaves_the_type_untouched() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<string, int> $x
             */
            function handle(array $x): void {
                if (array_key_exists('a', $x)) {
                    $y = $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `array_key_exists`'s own `HasOffsetValueType` equivalent - narrowing
// the specific element access (`$x['key']`), not just `$x`'s own
// overall type, via this project's existing composite `narrowing_key`
// mechanism rather than a new accessory-type hierarchy.
#[test]
fn array_key_exists_on_a_plain_array_narrows_the_specific_element_access() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $n): void {}
            /**
             * @param array<string, int> $x
             */
            function handle(array $x): void {
                if (array_key_exists('a', $x)) {
                    needsInt($x['a']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_key_exists_true_branch_element_access_still_catches_a_real_type_mismatch() {
    // Regression guard: the narrowing above must be precise, not a
    // blanket "never flag $x[key] again" escape hatch - `int` is still
    // the wrong type for a `string`-expecting parameter.
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, int> $x
             */
            function handle(array $x): void {
                if (array_key_exists('a', $x)) {
                    needsString($x['a']);
                }
            }
        "#,
    );
    assert!(!d.is_empty(), "expected a type mismatch, got none");
}

#[test]
fn array_value_type_for_key_returns_none_for_a_missing_shape_key() {
    let shape = Type::ArrayShape(vec![("a".to_string(), Type::Int, false)]);
    assert_eq!(
        narrowing::array_value_type_for_key(&shape, "b"),
        None,
        "a shape genuinely missing the key has no value type to offer"
    );
}

#[test]
fn is_string_narrows_a_variable_subscript_array_index_expression() {
    let d = diagnostics_for(
        r#"<?php
            class Translator {
                /** @var array<string, string|array<int, string>> */
                private array $mirror = [];

                public function translate(string $key): string {
                    $val = $key;
                    if (is_string($this->mirror[$key])) {
                        $val = $this->mirror[$key];
                    }
                    return $val;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_the_subscript_variable_invalidates_the_narrowed_entry() {
    // The soundness case this finding exists for. `is_int(...)`'s own
    // non-`Union` narrowing fallback (`narrow_type_to`'s `_ =>
    // target.clone()`) makes the *stale* entry a confidently-wrong
    // `int` rather than the correct, unprovable `mixed` a fresh read
    // would give - `needsString` (not `needsInt`) exposes the
    // difference: a surviving stale `int` would wrongly fail it,
    // while a correctly-purged `mixed` is leniently never flagged at
    // all (this project's own "don't guess" rule for `Mixed`
    // arguments), so the fix is confirmed by the diagnostic
    // *disappearing*, not by one appearing.
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr, string $key, string $other): void {
                if (is_int($arr[$key])) {
                    $key = $other;
                    needsString($arr[$key]);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_an_intermediate_property_link_invalidates_a_deeper_narrowed_entry() {
    // Reassigning $foo->bar to a fresh literal that *keeps* key 'k'
    // (just with a different value type) sidesteps the unrelated
    // "Offset ... does not exist" check (`check_offset_existence` runs
    // unconditionally, independent of narrowing) while still
    // exercising this fix directly: without it, the stale
    // `int`-narrowed `"foo->bar[k]"` entry would still win over the
    // real, freshly-reassigned `'newstring'`, wrongly reporting no
    // mismatch at all against `needsInt`.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @var array<string, mixed> */
                public array $bar = [];
            }
            function needsInt(int $n): void {}
            function handle(Foo $foo): void {
                if (is_int($foo->bar['k'])) {
                    $foo->bar = ['k' => 'newstring'];
                    needsInt($foo->bar['k']);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("newstring"), "got: {d:?}");
}

#[test]
fn a_by_ref_call_argument_invalidates_a_stale_narrowed_index_entry() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array<string, mixed> $arr
             */
            function widenIt(array &$arr): void {}
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                if (is_int($arr['k'])) {
                    widenIt($arr);
                    needsString($arr['k']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_an_unrelated_variable_does_not_purge_narrowing() {
    // The mirror-image control for the three tests above: since a
    // correctly-purged `mixed` is leniently never flagged, "no
    // diagnostic" alone wouldn't prove anything was preserved -
    // asserting the mismatch *still fires* is what proves the real
    // `int` narrowing survived the unrelated assignment intact.
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr, string $key): void {
                if (is_int($arr[$key])) {
                    $unrelated = 5;
                    needsString($arr[$key]);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int"), "got: {d:?}");
}

#[test]
fn or_of_same_key_predicates_narrows_the_true_branch_to_their_union() {
    let d = diagnostics_for(
        r#"<?php
            function needsIntOrString(int|string $x): void {}
            /**
             * @param array<string, mixed> $chronology_date
             */
            function handle(array $chronology_date): void {
                $year = $chronology_date['year'] ?? null;
                $year = is_int($year) || is_string($year) ? $year : 0;
                needsIntOrString($year);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_or_of_instanceof_checks_on_the_same_variable_narrows_to_their_union() {
    // `or_chain_narrowing_target` needs a leaf for `instanceof`: without
    // one, `$node instanceof A || $node instanceof B ? (string) $node :
    // ...` falls all the way through to "no narrowing," and the cast is
    // checked against the un-narrowed base class instead of the two
    // Stringable subclasses proven by the condition.
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Named extends Base { public function __toString(): string { return "n"; } }
            class Ident extends Base { public function __toString(): string { return "i"; } }
            function test(Base $node): string {
                return $node instanceof Named || $node instanceof Ident
                    ? (string) $node
                    : "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_three_way_or_of_same_key_predicates_still_narrows() {
    let d = diagnostics_for(
        r#"<?php
            function needsIntOrStringOrBool(int|string|bool $x): void {}
            /**
             * @param array<string, mixed> $arr
             */
            function handle(array $arr): void {
                $x = $arr['x'] ?? null;
                $x = is_int($x) || is_string($x) || is_bool($x) ? $x : 0;
                needsIntOrStringOrBool($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn or_of_predicates_on_different_keys_narrows_neither() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): void {}
            function handle(?int $x, ?string $y): void {
                $x = is_int($x) || is_string($y) ? $x : 0;
                needsString($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn or_of_and_of_negated_predicates_on_different_keys_narrows_each_independently() {
    // Each `||` leaf is `!isA($v) && !isB($v)` for a different `$v` -
    // De Morgan on the outer `||`'s false branch decomposes to one
    // independent `&&`-false-branch narrowing per leaf, each touching
    // only its own key, so no cross-key reasoning is needed here.
    let d = diagnostics_for(
        r#"<?php
            function wantsThree(int|string $a, int|string $b, bool|int $c): void {}
            function handle(mixed $commentId, mixed $imageId, mixed $validated): void {
                if ((! is_int($commentId) && ! is_string($commentId))
                    || (! is_int($imageId) && ! is_string($imageId))
                    || (! is_bool($validated) && ! is_int($validated))) {
                    return;
                }
                wantsThree($commentId, $imageId, $validated);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assert_instanceof_narrows_for_the_rest_of_the_block() {
    // `assert($node instanceof FuncCall); $node->args` - real PHPStan
    // documents support for narrowing on `assert()` the same as `if`.
    let d = diagnostics_for(
        r#"<?php
            class Node {}
            class FuncCall extends Node { public array $args = []; }
            function mutate(Node $node) {
                assert($node instanceof FuncCall);
                return $node->args;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A fully-qualified `\assert(...)` call (a common defensive-global-
// namespace style) must be treated as a narrowing statement too, not
// just its bare `assert(...)` spelling. `apply_narrowing`'s own
// `ExprKind::Call` arm already strips a leading backslash before
// comparing against `is_string`/`is_array`/etc, but the statement-level
// `assert(...)` recognition needs the same handling.
#[test]
fn a_fully_qualified_assert_call_still_narrows() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function f(int|string $input) {
                \assert(\is_int($input));
                needsInt($input);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assert_without_a_matching_narrowing_shape_is_a_harmless_no_op() {
    // apply_narrowing already no-ops on condition shapes it doesn't
    // understand - this just confirms assert()'s own new call site
    // doesn't crash or misbehave for one (a plain boolean variable).
    let d = diagnostics_for(
        r#"<?php
            function check(bool $ok) {
                assert($ok);
                return $ok;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real PHPUnit tests commonly narrow via assertion calls instead of an
// `if`-guard - `$output = ob_get_clean(); self::assertIsString($output);`
// - real PHPStan documents support for exactly this via its own
// `phpstan/phpstan-phpunit` extension, which this project doesn't
// depend on, so it's replicated directly. Matches the same "statement
// narrows unconditionally" shape `assert($cond)` already gets above.
#[test]
fn phpunit_assert_is_string_narrows_for_the_rest_of_the_block() {
    let d = diagnostics_for(
        r#"<?php
            class TestCase {
                public static function assertIsString($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): string {
                    $output = fetch();
                    self::assertIsString($output);
                    return $output;
                }
            }
            /** @return string|false */
            function fetch() {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `assertIsResource`'s own narrowing target is a
// `Type::Object("resource", ...)` (this project's fixed pseudo-type
// sentinel for PHP's native resource type, unlike every sibling
// `assertIs*` target which narrows to a bare native type) -
// `registry::is_assignable`'s own "an unresolved `@phpstan-type`-alias
// object name, stay lenient" arm (`assignability.rs`) must not also
// match `resource` (it's never a key in `self.classes` either, exactly
// like a genuinely-unresolvable alias), or `narrow_type_to`'s own
// `Union` arm would wrongly keep `Type::False` as "still possibly
// assignable to `resource`" and never actually strip it.
#[test]
fn phpunit_assert_is_resource_narrows_away_false() {
    let d = diagnostics_for(
        r#"<?php
            class TestCase {
                public static function assertIsResource($actual): void {}
            }
            class MyTest extends TestCase {
                /** @param resource $r */
                private function needsResource($r): void {}
                public function testX(): void {
                    $output = fetch();
                    self::assertIsResource($output);
                    $this->needsResource($output);
                }
            }
            /** @return resource|false */
            function fetch() {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpunit_assert_not_false_narrows_a_resource_for_the_rest_of_the_block() {
    let d = diagnostics_for(
        r#"<?php
            class TestCase {
                public static function assertNotFalse($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): void {
                    $handle = open();
                    self::assertNotFalse($handle);
                    fclose($handle);
                }
            }
            /** @return resource|false */
            function open() {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpunit_assert_not_null_narrows_via_an_instance_method_call_too() {
    // Same idiom via `$this->assertNotNull(...)` (PHPUnit's methods
    // are static, but PHP allows calling a static method through an
    // instance too, and real test code does this constantly).
    let d = diagnostics_for(
        r#"<?php
            class TestCase {
                public static function assertNotNull($actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(): string {
                    $value = fetch();
                    $this->assertNotNull($value);
                    return $value;
                }
            }
            /** @return ?string */
            function fetch() {}
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn phpunit_assert_instance_of_narrows_to_the_expected_class() {
    let d = diagnostics_for(
        r#"<?php
            class Node {}
            class FuncCall extends Node { public array $args = []; }
            class TestCase {
                public static function assertInstanceOf($expected, $actual): void {}
            }
            class MyTest extends TestCase {
                public function testX(Node $node): array {
                    self::assertInstanceOf(FuncCall::class, $node);
                    return $node->args;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn narrowing_a_generated_stub_union_return_type_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function needsString(string $s): string { return $s; }
            function f() {
                $encoded = json_encode(["a" => 1]);
                if ($encoded !== false) {
                    return needsString($encoded);
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn short_circuit_and_narrows_its_right_operand_and_the_enclosing_if_body() {
    // A common AST-visitor idiom: narrow a property chain via
    // `instanceof`, then read a property only the narrowed-to
    // subtype has - both within the same `&&` (needs check_expr's
    // short-circuit-aware traversal) and in the enclosing `if`'s body
    // (needs apply_narrowing to compose `&&`'s operands, not just
    // handle a single top-level condition shape).
    let and_operand = diagnostics_for(
        r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                return $arg->value instanceof Variable && $arg->value->name;
            }
        "#,
    );
    assert!(and_operand.is_empty(), "unexpected: {and_operand:?}");

    let if_body = diagnostics_for(
        r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                if ($arg->value instanceof Variable) {
                    echo $arg->value->name;
                    echo $arg->value->totallyMadeUp;
                }
            }
        "#,
    );
    assert!(
        if_body
            .iter()
            .any(|x| x.message.contains("Variable::$totallyMadeUp")),
        "expected the genuinely-undefined property to still be caught: {if_body:?}"
    );
}

#[test]
fn or_short_circuit_narrows_its_right_operand_on_the_false_branch() {
    // `||`'s right side only runs when the left is false - the mirror
    // image of `&&`'s "both true" composition.
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x) {
                return $x === null || strlen($x) > 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A parser-level requirement (not `merge_new_variable_definitions`'s
// own mechanism, already proven correct by the by-ref-out-param test
// right below this one) - a plain `$x = expr` assignment nested
// unparenthesized as `&&`'s/`||`'s own RHS must parse as real PHP's
// own `$cond && ($x = expr)`, not `($cond && $x) = expr`, or `$x`
// never looks assigned to anything downstream, regardless of how
// correct the checker's own narrowing/definite-assignment merge logic
// already is.
#[test]
fn an_assignment_nested_as_and_s_rhs_defines_the_variable_in_the_if_body() {
    let d = diagnostics_for(
        r#"<?php
            class NotFoundException {}
            function findNamespace(string $name): string { return $name; }
            function f(object $e, string $name): void {
                if ($e instanceof NotFoundException && $namespace = findNamespace($name)) {
                    echo $namespace;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_assignment_nested_as_or_s_rhs_defines_the_variable_afterward() {
    // Mirrors `Binary(Or, ...)`'s own existing `merge_new_variable_
    // definitions` policy exactly (same as `preg_match`'s by-ref
    // `$matches` test below already exercises for `&&`) - a new
    // variable defined by evaluating the right side is merged back into
    // `locals` unconditionally once the whole `||` expression is done,
    // regardless of which side actually ran. This test only needs to
    // confirm the parser now hands that mechanism a real `Assign` node
    // to find at all.
    let d = diagnostics_for(
        r#"<?php
            function f(?string $name): void {
                $name === null || $resolved = $name;
                echo $resolved;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_by_ref_out_param_defined_on_the_right_of_and_is_readable_afterward() {
    // `$matches` is `preg_match`'s by-ref out-param, defined as a side
    // effect of evaluating the right operand of a chained `and`/`&&`.
    // `check_expr`'s `Binary(And, l, r)` arm must check `r` against the
    // real `locals`, not a clone that gets discarded once the
    // expression finishes, or this definition never reaches the real
    // `locals` the enclosing `if`'s body sees. `mixed`, not `string` -
    // an already-`string`-typed subject makes the `is_string` guard
    // itself a genuine `function.alreadyNarrowedType` finding, which
    // isn't what this test means to exercise.
    let d = diagnostics_for(
        r#"<?php
            function f(mixed $subject): void {
                if (is_string($subject) and (bool) preg_match('/(\d+)/', $subject, $matches)) {
                    echo $matches[0];
                    echo $matches[1];
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_narrowing_inside_and_does_not_leak_into_the_else_branch() {
    // A naive "merge every newly-inserted key back into locals" (rather
    // than only real variable names) would insert `apply_narrowing`'s
    // own synthetic `"arg->value"` narrowing key into the real `locals`
    // before the `if`'s then/else split even runs - contaminating the
    // `else` branch (where the condition was false, so `$arg->value` is
    // NOT known to be a `Variable`) with the narrowed-to type. A test
    // that only covers the by-ref-in-`&&` shape above would still pass
    // under that broken version and would not catch this.
    let d = diagnostics_for(
        r#"<?php
            class Base {}
            class Variable extends Base { public string $name = ""; }
            class Arg { public Base $value; }
            function f(Arg $arg) {
                if ($arg->value instanceof Variable && $arg->value->name !== '') {
                    echo 'narrowed';
                } else {
                    echo $arg->value->name;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("Base::$name")),
        "expected the else branch to still see $arg->value as Base (undefined $name), got: {d:?}"
    );
}

#[test]
fn is_string_narrows_a_literal_array_index_expression() {
    // `narrowing_key` (and every `apply_narrowing` call site consuming
    // it) must not require a bare `ExprKind::Variable` directly:
    // `is_string(...)` must also narrow a literal-key array-index
    // expression the way it already narrows a property chain.
    let d = diagnostics_for(
        r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if (is_string($info['code'])) {
                    return strtoupper($info['code']);
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `check_expr`'s `Index` arm must consult the narrowed `locals` state,
// unlike `infer_expr`'s `Index` arm, and unlike `check_expr`'s own
// `PropertyAccess` arm just above it in the same match. A narrowed
// array-index expression passed as a *function argument* (checked via
// `infer_expr` for the argument-type comparison, the shape the test
// just above this one exercises) already worked; a bare `return
// $x['k'];` reading the same narrowed expression directly did not.
#[test]
fn is_string_narrows_a_literal_array_index_expression_read_back_via_a_bare_return() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if (is_string($info['code'])) {
                    return $info['code'];
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A ternary yielding `false|int` on both branches, then
// `if (! is_numeric($user_id)) { return false; }` before a call
// expecting a plain `int`.
#[test]
fn is_numeric_narrows_the_true_branch_by_excluding_bool_and_null() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|false $x */
            function f($x): int {
                if (is_numeric($x)) {
                    return $x;
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The unsoundness a shared-target table entry would have introduced
// (see this arm's own doc comment): `is_numeric`'s true branch must
// *not* drop `String`, since a numeric-looking string still satisfies
// it.
#[test]
fn is_numeric_leaves_string_untouched_in_the_true_branch() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|string $x */
            function f($x): int|string {
                if (is_numeric($x)) {
                    return $x;
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_numeric_false_branch_narrows_by_excluding_int_and_float() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int|string $x */
            function f($x): string {
                if (! is_numeric($x)) {
                    return $x;
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn equality_narrows_a_literal_array_index_expression() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{code?: string|null} $info */
            function f(array $info): string {
                if ($info['code'] !== null) {
                    return strtoupper($info['code']);
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_string_narrows_an_assignment_nested_in_its_own_argument() {
    // `narrowing_key` must not return `None` for an Assign expression:
    // the extremely common "assign-then-check" idiom
    // (`is_string($result = fetch())`) must be narrowed, even though
    // the assignment itself already defines `$result` correctly.
    let d = diagnostics_for(
        r#"<?php
            function fetch(): string|false { return 'x'; }
            function f(): string {
                if (is_string($result = fetch())) {
                    return $result;
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `while (($file = readdir($dir)) !== false) { ... }` - real PHP
// requires the parens here (`=` binds looser than `!==`, so `$file =
// readdir($dir) !== false` would parse as `$file = (readdir($dir) !==
// false)` instead), so the extremely common "assign-then-check" idiom
// is almost always wrapped in a `Grouping` node - `narrowing_key` must
// see through it to reach the `Assign` arm underneath.
#[test]
fn is_string_narrows_a_parenthesized_assignment_in_a_while_condition() {
    let d = diagnostics_for(
        r#"<?php
            function readdir_(): string|false { return 'x'; }
            function f(): void {
                while (($file = readdir_()) !== false) {
                    preg_match('/x/', $file);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `if (false !== $pos = strpos($path, '://')) { ... }` - no parens
// around the assignment at all, relying on real PHP's own grammar
// letting `variable '=' expr` nest directly as a comparison operator's
// operand (parser-level, `parse_assignment_tail`). This exercises the
// narrowing half specifically - the existing `ExprKind::Assign` arm in
// `narrowing_key` already covers it once the parser hands it the right
// AST shape.
#[test]
fn identity_comparison_narrows_an_unparenthesized_assignment_nested_in_an_if_condition() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $path): string {
                if (false !== $pos = strpos($path, '://')) {
                    return substr($path, 0, $pos + 3);
                }
                return '';
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A fully-qualified reference to a global keyword-name
// (`if (\false === $x = strrpos(...))`, e.g. from a namespace-prefixing
// code transform that rewrites unqualified `true`/`false`/`null` to
// their fully-qualified form). A fully-qualified `\false` parses as a
// plain `ExprKind::Name`, not the `Literal(Bool(false))` a bare `false`
// gets via `parse_primary`'s own `Keyword::False` match, so it must not
// silently read as `Type::Mixed(false)` and fall into
// `apply_equality_narrowing`'s own `target_ty == Type::Mixed(false)` bail-out,
// skipping the guard's own narrowing entirely.
#[test]
fn identity_comparison_narrows_through_a_fully_qualified_false_literal() {
    let d = diagnostics_for(
        r#"<?php
            function f(string $path): string {
                if (\false === $pos = strpos($path, '://')) {
                    return '';
                }
                return substr($path, 0, $pos);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn fully_qualified_true_false_and_null_infer_as_their_own_literal_types() {
    let d = diagnostics_for(
        r#"<?php
            function needsTrue(true $x): void {}
            function needsFalse(false $x): void {}
            function needsNull(null $x): void {}
            function test(): void {
                needsTrue(\true);
                needsFalse(\false);
                needsNull(\null);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `if (! (bool) $fp) { throw ...; }` - a common, explicit style for a
// truthiness guard. `(bool)` doesn't change which values are
// truthy/falsy, so `apply_narrowing` needs an arm for `Cast` too, or
// the guard is never recognized.
#[test]
fn a_bool_cast_truthiness_guard_narrows_like_a_bare_variable_check() {
    let d = diagnostics_for(
        r#"<?php
            /** @return resource|false */
            function open() {}
            function f(): void {
                $fp = open();
                if (! (bool) $fp) {
                    throw new Exception('failed');
                }
                fclose($fp);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `is_int($decoded['status'] ?? null)` - `narrowing_key` needs an arm
// for a `??`-wrapped expression too, or this common
// "narrow-an-array-element-that-might-be-absent" idiom never narrows
// `$decoded['status']` itself.
#[test]
fn is_int_narrows_through_a_null_coalesce_fallback() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{status?: int|null} $decoded */
            function f(array $decoded): int {
                if (is_int($decoded['status'] ?? null)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The unsoundness this fix's own scoping avoids: unlike `?? null`,
// `is_int($decoded['status'] ?? 0)` can be true purely because the
// fallback `0` itself is an int, even when `$decoded['status']` is
// absent/`null` - narrowing it to `int` there would be a real, new
// false negative, so the coalesce arm must not fire for this
// fallback.
#[test]
fn is_int_does_not_narrow_through_a_non_null_coalesce_fallback() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{status?: int|string} $decoded */
            function f(array $decoded): int {
                if (is_int($decoded['status'] ?? 0)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected $decoded['status'] to stay unnarrowed (still int|string): {d:?}"
    );
}

// The same boundary, checked against a non-literal fallback too - not
// just "a fallback that happens to be non-zero."
#[test]
fn is_int_does_not_narrow_through_a_variable_coalesce_fallback() {
    let d = diagnostics_for(
        r#"<?php
            /** @param array{status?: int|string} $decoded */
            function f(array $decoded, int $default): int {
                if (is_int($decoded['status'] ?? $default)) {
                    return $decoded['status'];
                }
                return 0;
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected $decoded['status'] to stay unnarrowed (still int|string): {d:?}"
    );
}

// `apply_equality_narrowing`'s exclusion branch must not use the other
// side's own static type (`Bool`, a general type covering both
// `true`/`false`, not one specific value) the same way it uses a
// genuine single-value exclusion (`Null`/`True`/`False`/a literal), or
// it wrongly collapses an already-known `bool` to `Type::Never` the
// moment it's compared `!==` against another `bool`-typed (but not
// literally known) value.
#[test]
fn not_identical_to_a_non_literal_bool_does_not_collapse_the_narrowed_type_to_never() {
    let d = diagnostics_for(
        r#"<?php
            function test(?bool $a, bool $b): void {
                $update = [];
                if ($a !== null && $a !== $b) {
                    $update['flag'] = $a;
                }
                echo $update['flag'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `narrow_type_to`'s own non-Union fallback must not unconditionally
// return the comparison target's type, discarding a narrower
// `original` - a definite `int` compared `===` against an `int|null`
// value inside an unrelated `if`/`else` that never reassigns it must
// not have its own type widened to `int|null` on the true branch,
// corrupting every downstream use for the rest of the function.
#[test]
fn identical_to_a_broader_typed_value_does_not_widen_the_narrower_side() {
    let d = diagnostics_for(
        r#"<?php
            class ImageId {
                public static function from(int $value): self { return new self(); }
            }
            function getRefererImageId(): ?int { return null; }
            function f(int $image_id): void {
                if (getRefererImageId() === $image_id) {
                    // no-op - neither branch reassigns $image_id
                }
                ImageId::from($image_id);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `check_expr`'s own `Ternary` arm must not write
// `merge_scopes(then_locals, else_locals)` back into the outer locals
// - a ternary's own condition narrowing a param (`$collected` here)
// down to the literal empty shape on its true branch must not get
// permanently unioned into `$collected`'s real, tracked type for
// everything checked afterward, including a second, independent
// evaluation of the exact same sub-expression, or a later
// `preg_replace`'s own subject-type check would see the corrupted
// union instead of `$collected[0]`'s real `string` type, losing its
// precise return type.
#[test]
fn a_ternarys_own_condition_narrowing_does_not_leak_past_the_ternary() {
    let d = diagnostics_for(
        r#"<?php
            class Holder {
                public ?string $captured = null;

                /** @param list<string> $collected */
                public function run(array $collected): void {
                    $this->captured = $collected === [] ? null : preg_replace('/^x/', '', $collected[0]);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_a_property_after_narrowing_it_does_not_leave_the_stale_narrowed_type_behind() {
    // A lazy-init idiom: `if ($this->entries === null) { $this->entries
    // = [...]; } return $this->entries;`. `apply_equality_narrowing`
    // correctly narrows `$this->entries` to `null` for the condition's
    // own branch, but `bind_assignment_target` needs a `PropertyAccess`
    // arm too, or the reassignment inside the `if` body never updates
    // that same synthetic narrowing-key entry - `infer_expr`'s own
    // "check narrowed key first" fast path would keep reading back the
    // stale `null` instead of the real, newly-assigned array, making
    // the return type look nullable when it never could be.
    let d = diagnostics_for(
        r#"<?php
            class Cache {
                /** @var array<string>|null */
                private ?array $entries = null;

                /** @return array<string> */
                private function entries(): array {
                    if ($this->entries === null) {
                        $this->entries = ['a', 'b'];
                    }
                    return $this->entries;
                }

                /** @return array<string> */
                public function get(): array {
                    return $this->entries();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn instanceof_self_narrows_to_the_real_enclosing_class_not_a_literal_self() {
    // `if ($x instanceof self)` - `apply_narrowing`'s instanceof
    // handling must not treat "self"/"parent"/"static" as ordinary
    // bare class names to resolve via `name_ctx.resolve` directly;
    // it needs to go through `resolve_pseudo` against the enclosing
    // class the way every other self/parent/static site already does.
    // In a namespaced file getting this wrong narrows to a nonsensical
    // class name ("App\self") instead of the real one ("App\Box") -
    // which looks like no diagnostics at all below (a bogus,
    // not-in-the-registry class silently suppresses the undefined-
    // method check), not a wrong one, so
    // this asserts a diagnostic *does* appear, to actually
    // distinguish correct narrowing from the old silent failure.
    let d = diagnostics_for(
        r#"<?php
            namespace App;
            class Box {
                public function useSame($other): string {
                    if ($other instanceof self) {
                        return $other->totallyMadeUp();
                    }
                    return "";
                }
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("App\\Box::totallyMadeUp")),
        "expected narrowing to resolve to the real class App\\Box, got: {d:?}"
    );
}

#[test]
fn negated_instanceof_guard_clause_narrows_past_the_if() {
    // `if (! $x instanceof Y) { throw ...; }` must narrow $x to Y for
    // the code after the guard - depends on `!` correctly binding
    // looser than `instanceof` at the parser level, not parsing as
    // `(!$x) instanceof Y`, which would silently narrow nothing.
    let d = diagnostics_for(
        r#"<?php
            interface Base {}
            interface Specific extends Base {
                public function specificMethod(): int;
            }
            function make(): Base { return new class implements Specific {
                public function specificMethod(): int { return 1; }
            }; }
            function f() {
                $x = make();
                if (! $x instanceof Specific) {
                    throw new \Exception('nope');
                }
                return $x->specificMethod();
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_inline_var_annotation_on_a_later_statement_narrows_a_generically_typed_read() {
    // `$file` comes back from `current()`'s own generic (untyped, so
    // `mixed`) return, an inline `/** @var FakeFileInfo9921 $file */`
    // sits on an unrelated `if` statement that merely reads it, and
    // code after that `if` calls a method only `FakeFileInfo9921` has.
    // Without the annotation, `$file` stays `mixed` and this method
    // call is never checked at all (this project's usual mixed-
    // argument/mixed-receiver leniency) - so a real "undefined method"
    // diagnostic here proves the annotation actually took effect, not
    // just that nothing was flagged.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): void {
                $file = $it->current();
                /** @var FakeFileInfo9921 $file */
                if (true) {
                }
                $file->missingMethod();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined method")),
        "expected an undefined-method diagnostic, got: {d:?}"
    );
}

#[test]
fn a_bare_inline_var_annotation_overrides_the_immediately_following_assignment() {
    // The bug found during adversarial validation: applying the
    // assertion only *before* dispatch would have this immediately
    // clobbered by the assignment's own natural (mixed) inference,
    // since `bind_assignment_target` runs inside the very same
    // `check_stmt` call. A `return type mismatch` naming FakeFileInfo9921
    // proves the after-dispatch reapplication actually won.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): string {
                /** @var FakeFileInfo9921 */
                $file = $it->current();
                return $file;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("return type mismatch")
            && x.message.contains("FakeFileInfo9921")),
        "expected a return-type mismatch naming FakeFileInfo9921, got: {d:?}"
    );
}

#[test]
fn a_named_inline_var_annotation_on_its_own_assignment_also_overrides_it() {
    // Same bug, explicitly-named form - proving the fix isn't
    // bare-form-specific.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            class Iter {
                public function current() { return null; }
            }
            function f(Iter $it): string {
                /** @var FakeFileInfo9921 $file */
                $file = $it->current();
                return $file;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("return type mismatch")
            && x.message.contains("FakeFileInfo9921")),
        "expected a return-type mismatch naming FakeFileInfo9921, got: {d:?}"
    );
}

#[test]
fn an_inline_var_override_is_invalidated_by_a_later_real_reassignment() {
    // The override correctly gets invalidated by a later real
    // reassignment of the same variable (purge-on-reassignment).
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            function needsInt(int $n): void {}
            function f($it): void {
                $file = $it;
                /** @var FakeFileInfo9921 $file */
                if ($it) {
                }
                $file = 5;
                needsInt($file);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_inline_var_annotation_preceding_a_foreach_binding_two_names_is_a_no_op() {
    // Scope guard for `statement_defined_variable_names`'s own
    // restriction: a bare (nameless) tag can only infer its target
    // when the statement it precedes has exactly one candidate -
    // a `foreach` binding both a key and a value has two, so this is
    // left unguessed rather than assuming either one.
    let d = diagnostics_for(
        r#"<?php
            class FakeFileInfo9921 {}
            function f(array $items): string {
                /** @var FakeFileInfo9921 */
                foreach ($items as $k => $v) {
                    return $v;
                }
                return "fallback";
            }
        "#,
    );
    // Real PHPStan also counts the iterated variable as a candidate, so
    // an unnamed tag here is `varTag.noVariable` - not this test's subject.
    let d: Vec<_> = d
        .into_iter()
        .filter(|x| x.identifier != "varTag.noVariable")
        .collect();
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_doc_comment_with_no_var_tag_is_a_no_op() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                /** Just a description, no @var tag here. */
                $x = "not an int";
                return 1;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A bare truthiness check (`if ($x)` / `if (!$x)`) and `empty($x)` must
// both apply narrowing.

#[test]
fn negated_truthy_guard_narrows_past_the_if() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                if (!$x) {
                    return "empty";
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn positive_truthy_guard_narrows_inside_the_if() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                if ($x) {
                    return $x;
                }
                return "default";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn positive_truthy_guard_does_not_narrow_the_false_branch() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                if ($x) {
                    return "yes";
                }
                return $x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn empty_guard_narrows_past_the_if() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                if (empty($x)) {
                    return "empty";
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `isset($x)`'s truthy branch narrows away `Null`, the single most
// common PHP nullability idiom.
#[test]
fn isset_guard_narrows_null_away_in_the_truthy_branch() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): int {
                if (isset($x)) {
                    return strlen($x);
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_guard_narrows_a_property_access_too() {
    let d = diagnostics_for(
        r#"<?php
            class Box { public ?string $value = null; }
            function f(Box $box): int {
                if (isset($box->value)) {
                    return strlen($box->value);
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `isset($this->manager, $this->namespace)` - real PHP's `isset()` is
// genuinely variadic (`isset($a, $b)` means `isset($a) && isset($b)`,
// every argument must be set), so the narrowing dispatch above must
// not only match a single-argument call (`[single_arg]`'s own slice
// pattern), or it silently narrows none of a multi-argument isset()'s
// arguments - not just the ones past the first.
#[test]
fn a_multi_argument_isset_narrows_every_argument_in_the_truthy_branch() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $a, ?string $b): int {
                if (isset($a, $b)) {
                    return strlen($a) + strlen($b);
                }
                return 0;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A static property (`self::$prop !== null`) needs a narrowing key
// too, not just an instance property's `->` chain.
#[test]
fn a_static_property_is_narrowed_by_an_equality_check_too() {
    let d = diagnostics_for(
        r#"<?php
            class Cache1 {
                private static ?string $value = null;
                public static function get(): string {
                    if (self::$value !== null) {
                        return self::$value;
                    }
                    return self::$value = 'computed';
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reassigning_a_static_property_after_narrowing_it_does_not_leave_the_stale_narrowed_type_behind()
{
    let d = diagnostics_for(
        r#"<?php
            class Cache2 {
                private static ?string $value = null;
                public static function reset(): int {
                    if (self::$value !== null) {
                        self::$value = null;
                        return strlen(self::$value);
                    }
                    return 0;
                }
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "expected the stale narrowed type to be purged: {d:?}"
    );
}

#[test]
fn isset_false_branch_is_not_narrowed() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): int {
                if (isset($x)) {
                    return 0;
                }
                return strlen($x);
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// ternary/switch/match all need condition-based narrowing, for any
// condition shape.

#[test]
fn ternary_narrows_both_branches() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return $x === null ? "default" : $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return $x !== null ? $x : "default";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let wrong = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return $x === null ? $x : "default";
            }
        "#,
    );
    assert_eq!(wrong.len(), 1, "got: {wrong:?}");
}

// `$handle = $filename !== null ? @fopen($filename, 'wb') : false;` -
// `fopen()`'s own type is already a Union (`resource|false`);
// combining it with the ternary's other branch must flatten that
// nested union, or a later `!== false` narrowing on the outer union
// can't reach the `false` still hiding inside it.
#[test]
fn ternary_flattens_a_branch_that_is_itself_already_a_union() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $filename): void {
                $handle = $filename !== null ? @fopen($filename, 'wb') : false;
                if ($handle !== false) {
                    fclose($handle);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn switch_default_narrows_past_a_terminating_case() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                        return "was-null";
                    default:
                        return $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let wrong = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                        return $x;
                    default:
                        return "not null";
                }
            }
        "#,
    );
    assert_eq!(wrong.len(), 1, "got: {wrong:?}");
}

#[test]
fn switch_fallthrough_case_does_not_leak_its_own_narrowing_past_a_later_terminating_case() {
    // An empty `case null: case "": ...` fallthrough group's optimistic
    // narrowing (subject === null) must not leak past the second case
    // (which itself terminates) into `default`.
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                    case "":
                        return "empty-ish";
                    default:
                        return $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `exits_function_or_loop`'s `Try` handling: a switch's own last case
// ending in a `try`/`catch` that unconditionally exits (both the `try`
// body and every `catch` return/throw) must not fall through its
// catch-all `_ => false` arm, wrongly concluding that case's own
// locals "reach after the switch" (`reaches_after_switch`'s
// `is_last && !exits_function_or_loop` arm) - merging that case's
// locals (where `$result` was never assigned) into the switch's real
// exit state (`case null`'s, where it was) would make `$result` look
// only maybe-defined after the switch.
#[test]
fn switch_last_case_ending_in_a_try_catch_does_not_reach_after_the_switch() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                switch ($x) {
                    case null:
                        $result = "was-null";
                        break;
                    default:
                        try {
                            return "";
                        } catch (\Throwable $e) {
                            throw $e;
                        }
                }
                return $result;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn match_true_pattern_narrows_each_arm_and_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return match (true) {
                    $x === null => "default",
                    default => $x,
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    let wrong = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return match (true) {
                    $x === null => $x,
                    default => "not null",
                };
            }
        "#,
    );
    assert_eq!(wrong.len(), 1, "got: {wrong:?}");
}

#[test]
fn match_on_a_real_subject_narrows_via_implicit_equality() {
    let d = diagnostics_for(
        r#"<?php
            function f(?string $x): string {
                return match ($x) {
                    null => "default",
                    default => $x,
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn match_unhandled_flags_an_enum_case_missing_no_default() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit { case Hearts; case Diamonds; case Clubs; case Spades; }
            function color(Suit $s): string {
                return match ($s) {
                    Suit::Hearts, Suit::Diamonds => "red",
                    Suit::Clubs => "black",
                };
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "match.unhandled" && x.message.contains("Suit::Spades")),
        "got: {d:?}"
    );
}

#[test]
fn match_unhandled_is_never_flagged_when_every_enum_case_is_covered() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit { case Hearts; case Diamonds; case Clubs; case Spades; }
            function color(Suit $s): string {
                return match ($s) {
                    Suit::Hearts, Suit::Diamonds => "red",
                    Suit::Clubs, Suit::Spades => "black",
                };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.unhandled"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn match_unhandled_is_never_flagged_when_a_nullable_enum_is_fully_covered() {
    // Regression: `expand_enum_cases` must recurse into a `Union`
    // member-by-member - a bare enum `Object` sitting directly inside
    // `Status|null` (not yet expanded to its own cases) can never be
    // proven redundant against an individual `EnumCase` exclusion,
    // only a real per-case member already sitting in the union can.
    let d = diagnostics_for(
        r#"<?php
            enum Status { case Active; case Inactive; }
            function f(Status|null $s): string {
                return match ($s) {
                    Status::Active => "active",
                    Status::Inactive => "inactive",
                    null => "none",
                };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.unhandled"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn match_unhandled_flags_a_bool_subject_missing_the_false_arm() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $b): string {
                return match ($b) {
                    true => "yes",
                };
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "match.unhandled" && x.message.contains("false")),
        "got: {d:?}"
    );
}

#[test]
fn match_unhandled_is_never_flagged_when_both_bool_arms_are_covered() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $b): string {
                return match ($b) {
                    true => "yes",
                    false => "no",
                };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.unhandled"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn match_unhandled_is_never_flagged_when_a_default_arm_is_present() {
    let d = diagnostics_for(
        r#"<?php
            enum Suit { case Hearts; case Diamonds; case Clubs; case Spades; }
            function color(Suit $s): string {
                return match ($s) {
                    Suit::Hearts => "red",
                    default => "other",
                };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.unhandled"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn match_unhandled_flags_an_unbounded_int_subject_covered_by_finitely_many_literals() {
    // No finite set of int literal arms can ever exhaust `int` itself -
    // matches real PHPStan's own identical behavior here.
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): string {
                return match ($n) {
                    1 => "one",
                    2 => "two",
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "match.unhandled"),
        "got: {d:?}"
    );
}

#[test]
fn match_unhandled_proves_a_match_true_idiom_exhaustive_via_range_narrowing() {
    // Real PHPStan's own mechanism for this shape: the *last* arm's own
    // condition, narrowed by every earlier arm having been false, is
    // already tautologically true - ordinary int-range narrowing
    // (`condition_is_provably_true`, the same machinery an ordinary
    // `if ($x > 5) {} elseif ($x <= 5) {}` chain already uses to report
    // `smallerOrEqual.alwaysTrue`) proves this directly, no boolean-SAT
    // solving needed.
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): string {
                return match (true) {
                    $x > 5 => "big",
                    $x <= 5 => "small",
                };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.unhandled"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn match_unhandled_still_flags_a_genuinely_non_exhaustive_match_true_idiom() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): string {
                return match (true) {
                    $x > 5 => "big",
                    $x > 10 => "bigger",
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "match.unhandled"),
        "got: {d:?}"
    );
}

#[test]
fn match_unhandled_is_exempted_when_wrapped_in_a_try_catching_unhandled_match_error() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): string {
                try {
                    return match ($n) {
                        1 => "one",
                    };
                } catch (\UnhandledMatchError $e) {
                    return "caught";
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.unhandled"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn match_unhandled_is_exempted_when_caught_via_a_broader_throwable_type() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): string {
                try {
                    return match ($n) {
                        1 => "one",
                    };
                } catch (\Throwable $e) {
                    return "caught";
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.unhandled"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn match_unhandled_is_never_exempted_by_an_unrelated_catch_type() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $n): string {
                try {
                    return match ($n) {
                        1 => "one",
                    };
                } catch (\RuntimeException $e) {
                    return "caught";
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "match.unhandled"),
        "got: {d:?}"
    );
}

#[test]
fn match_unhandled_is_exempted_by_an_enclosing_throws_tag() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @throws \UnhandledMatchError
             */
            function f(int $n): string {
                return match ($n) {
                    1 => "one",
                };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.unhandled"),
        "unexpected diagnostics: {d:?}"
    );
}

// `match.alwaysTrue`/`match.alwaysFalse` - real PHPStan's own
// `MatchExpressionRule` dead-arm half, the sibling of `match.unhandled`'s
// own missing-arm half above.

#[test]
fn a_dead_arm_condition_already_excluded_by_an_earlier_multi_condition_arm_is_always_false() {
    let d = diagnostics_for(
        r#"<?php
            enum Status { case A; case B; case C; }
            function f(Status $s): string {
                return match ($s) {
                    Status::A, Status::B, Status::C => 'covers everything',
                    Status::A => 'dead code',
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "match.alwaysFalse"),
        "got: {d:?}"
    );
}

#[test]
fn a_bool_arm_exhaustive_before_a_trailing_default_is_always_true() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $b): string {
                return match ($b) {
                    true => 'a',
                    false => 'b',
                    default => 'unreachable',
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "match.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_bool_arm_exhaustive_with_no_trailing_default_is_never_flagged_as_the_last_arm() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $b): string {
                return match ($b) {
                    true => 'a',
                    false => 'b',
                };
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "match.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn a_literal_arm_repeated_after_an_already_narrowed_single_value_subject_is_always_true() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                $x = 1;
                return match ($x) {
                    1 => 'always true but followed by default',
                    default => 'unreachable',
                };
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "match.alwaysTrue"),
        "got: {d:?}"
    );
}

#[test]
fn an_ordinary_unbounded_subject_match_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(int $x): string {
                return match ($x) {
                    1 => 'a',
                    2 => 'b',
                    default => 'c',
                };
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "match.alwaysTrue" || x.identifier == "match.alwaysFalse"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_single_arm_covering_every_enum_case_individually_is_never_flagged_per_condition() {
    // Each condition in a multi-condition arm is checked independently
    // (`$matchCondition === $rawCondition` for just that one raw
    // condition) - real PHPStan's own per-condition reasoning can't see
    // that the arm's *combined* OR exhausts every case, so no single
    // condition is itself "always true," matching real PHPStan exactly
    // (that combined-exhaustiveness question belongs to `match.unhandled`
    // alone, not this check).
    let d = diagnostics_for(
        r#"<?php
            enum Status { case A; case B; case C; }
            function f(Status $s): string {
                return match ($s) {
                    Status::A, Status::B, Status::C => 'covers everything',
                };
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "match.alwaysTrue" || x.identifier == "match.alwaysFalse"),
        "unexpected diagnostics: {d:?}"
    );
}

// `??`/`??=`'s left side must not be checked for "undefined
// variable"/"undefined property" like an ordinary read - both are
// specifically designed to be warning-free for a variable/property
// that isn't set at all.

#[test]
fn coalesce_does_not_flag_a_conditionally_assigned_variable_as_undefined() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $c): string {
                if ($c) { $result = "computed"; }
                return $result ?? "default";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_does_not_flag_an_undefined_array_key_root_variable() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                return $config['key'] ?? "default";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_widens_to_the_left_side_s_array_type_when_the_fallback_is_a_narrower_shape() {
    // `$details = $row->details ?? [];` where `$row->details` is
    // `public ?array $details` must not collapse the whole expression's
    // type to the literal empty `[]` shape from the fallback alone,
    // discarding that `$row->details`'s own real, non-empty-capable
    // `?array` type could hold arbitrary keys - otherwise every later
    // `$details['key']` read looks like a typo.
    let d = diagnostics_for(
        r#"<?php
            class Row { public ?array $details; }
            function f(Row $row): void {
                $details = $row->details ?? [];
                echo $details['config_section'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_still_collapses_to_the_fallback_for_an_ordinary_mixed_left_side() {
    // The common, currently-precise case: when the left side's real
    // type isn't an informative `Type::Array`, `??`'s result stays
    // exactly the fallback's own (here, literal string) type - not
    // blanket `Mixed` from unioning in `$x`'s own type.
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): int { return $x; }
            function f($x): int {
                return needsInt($x ?? 0);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_does_not_flag_an_undeclared_instance_property() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f(Foo $x): string { return $x->missing ?? "default"; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn coalesce_still_flags_an_undeclared_static_property() {
    // isset()/?? silence a suppressible *notice* - an undeclared
    // *static* property is a real, non-suppressible PHP fatal error,
    // and must still be flagged under `??`.
    let d = diagnostics_for(
        r#"<?php
            class Bar {}
            function f(): string { return Bar::$missing ?? "default"; }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn coalesce_assign_does_not_flag_its_own_left_side_as_undefined() {
    let d = diagnostics_for(
        r#"<?php
            function f(): string {
                $x ??= "default";
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Loose equality (`==`) must not narrow exactly like strict equality
// (`===`), which would discard every other value PHP's own
// type-juggling rules also match.

#[test]
fn loose_equality_does_not_narrow_to_the_literal_compared_to_value() {
    // `$x == 0` also matches the real bool `false` (PHP's own
    // `false == 0` rule) - narrowing exactly to the literal `0`
    // would incorrectly discard that possibility. This project's own
    // separate, deliberate choice to analyze every file as if
    // strict_types were always active means passing the
    // *unnarrowed* `bool|int` union to a `bool`-only parameter can
    // still legitimately be flagged on its own terms - what this
    // test actually guards against is over-narrowing to the wrong,
    // overly-specific literal value.
    let d = diagnostics_for(
        r#"<?php
            function g(bool $flag): void {}
            function f(bool|int $x): void {
                if ($x == 0) {
                    g($x);
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.message.contains("'0' given") || x.message.contains(", 0 given")),
        "still over-narrowing to the literal compared-to value: {d:?}"
    );
}

#[test]
fn strict_equality_still_narrows() {
    let d = diagnostics_for(
        r#"<?php
            function h(int $y): void {}
            function f(?int $x): void {
                if ($x === null) {
                    return;
                }
                h($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A `while`/`do-while` loop must apply its negated condition as
// narrowing after exit, and a `for` loop's body must not be checked as
// if it always executes exactly once - it needs a "may run zero times"
// merge too.

#[test]
fn while_loop_narrows_the_negated_condition_after_exit() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                $x = null;
                while ($x === null) {
                    $x = 5;
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `while (true) { ...; break; }` never exits via `cond` going false (it
// can't - the condition is a literal `true`) - the only way past it is
// the `break`, so a variable the body unconditionally defines before
// breaking must be visible afterward, not merged with a "loop ran zero
// times" possibility that can't actually happen.
#[test]
fn a_while_true_loop_defines_a_variable_reachable_only_via_break() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                while (true) {
                    $y = 1;
                    break;
                }
                return $y;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_while_1_loop_is_treated_as_the_identical_truthy_literal() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                while (1) {
                    $y = 1;
                    break;
                }
                return $y;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A `while (true)` with more than one break-reached state must merge
// them together, not just take the first one found.
#[test]
fn a_while_true_loop_merges_multiple_break_reached_states() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $flag): int {
                while (true) {
                    if ($flag) {
                        $y = 1;
                        break;
                    }
                    $y = 2;
                    break;
                }
                return $y;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A while loop whose condition is truthy but not a literal (a variable,
// a function call, ...) must still get the ordinary "may run zero
// times" treatment - `is_truthy_literal` deliberately only recognizes a
// bare literal, not anything that merely evaluates to one.
#[test]
fn a_while_loop_over_a_non_literal_truthy_condition_is_unaffected() {
    let d = diagnostics_for(
        r#"<?php
            function f(bool $cond): int {
                while ($cond) {
                    $y = 1;
                    break;
                }
                return $y;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.message.contains("undefined variable")),
        "expected $y to still look possibly-undefined (loop may run zero \
         times), got: {d:?}"
    );
}

#[test]
fn do_while_loop_narrows_the_negated_condition_after_exit() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                $x = null;
                do {
                    $x = 5;
                } while ($x === null);
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn for_loop_narrows_the_negated_condition_after_exit() {
    let d = diagnostics_for(
        r#"<?php
            function f(): int {
                for ($x = null; $x === null;) {
                    $x = 5;
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The nullsafe operator (?->) and plain -> must not produce identical
// AST nodes, or `infer_expr` never knows a nullsafe access's result
// could be null.

#[test]
fn nullsafe_method_call_result_is_implicitly_nullable() {
    let d = diagnostics_for(
        r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(?Owner $obj): void { needsFoo($obj?->getFoo()); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("Foo|null"), "got: {d:?}");
}

#[test]
fn nullsafe_property_access_result_is_implicitly_nullable() {
    let d = diagnostics_for(
        r#"<?php
            class Owner { public ?Foo $foo = null; }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(?Owner $obj): void { needsFoo($obj?->foo); }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

#[test]
fn nullsafe_result_accepted_by_a_nullable_parameter_is_not_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsMaybeFoo(?Foo $x): void {}
            function test(?Owner $obj): void { needsMaybeFoo($obj?->getFoo()); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn plain_arrow_on_a_non_nullable_receiver_is_unaffected() {
    let d = diagnostics_for(
        r#"<?php
            class Owner { public function getFoo(): Foo { return new Foo(); } }
            class Foo {}
            function needsFoo(Foo $x): void {}
            function test(Owner $obj): void { needsFoo($obj->getFoo()); }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Reassigning a variable inside a middle `&&` conjunct must be visible
// to a later conjunct in the same condition, not just to code after
// the whole `if`. `apply_narrowing`'s own `Binary(And, ...)`
// composition re-derives each operand's effect independently of
// whatever `check_expr` already computed while checking it for
// diagnostics, so it needs a dedicated `ExprKind::Assign` arm there, or
// the reassignment is invisible to `$r->isPublic()` two conjuncts
// later - `$r` still looking like its pre-assignment `ReflectionClass`,
// wrongly flagging `isPublic()` (a real `ReflectionMethod` method) as
// undefined.
#[test]
fn a_reassignment_in_a_middle_and_conjunct_is_visible_to_a_later_conjunct() {
    let d = diagnostics_for(
        r#"<?php
            function f(ReflectionClass $r): void {
                if ($r->hasMethod('__call') && ($r = $r->getMethod('__call')) && $r->isPublic()) {
                    echo "ok";
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

// Same shape, `||` instead of `&&` - a later disjunct still needs to see
// an earlier one's reassignment (relevant once the earlier disjunct's
// own condition is false and control reaches the assignment at all).
#[test]
fn a_reassignment_in_a_middle_or_conjunct_is_visible_to_a_later_conjunct() {
    let d = diagnostics_for(
        r#"<?php
            function f(?ReflectionClass $r): void {
                if ($r === null || ($r = $r->getMethod('x')) || $r->isPublic()) {
                    echo "ok";
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "method.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

// `<`/`>`/`<=`/`>=` against a literal narrows a plain/ranged int
// variable's own range - scoped to a single literal bound and a
// numeric-only current type (see `comparison_narrowed_bounds`'s own
// doc comment).

#[test]
fn less_than_a_literal_narrows_the_upper_bound_exclusive() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 9> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if ($x < 10 && $x >= 0) {
                    needsNarrower($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn less_than_or_equal_to_a_literal_narrows_the_upper_bound_inclusive() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 10> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if ($x <= 10 && $x >= 0) {
                    needsNarrower($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");

    // Proves the bound is exact, not just "some non-negative int" -
    // one narrower than the real result must still be rejected.
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, 9> $x */
            function needsTighter(int $x): void {}
            function test(int $x): void {
                if ($x <= 10 && $x >= 0) {
                    needsTighter($x);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int<0, 10>"), "got: {d:?}");
}

#[test]
fn greater_than_a_literal_narrows_the_lower_bound_exclusive() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<1, max> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if ($x > 0) {
                    needsNarrower($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn greater_than_or_equal_to_a_literal_narrows_the_lower_bound_inclusive() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, max> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if ($x >= 0) {
                    needsNarrower($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_literal_on_the_left_is_narrowed_via_the_mirrored_operator() {
    // `0 <= $x` means `$x >= 0`, not `$x <= 0` - proves the operand-order
    // swap actually mirrors the operator, not just relabels it.
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, max> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if (0 <= $x) {
                    needsNarrower($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn the_false_branch_of_a_comparison_narrows_via_the_negated_outcome() {
    // `!($x < 10)` means `$x >= 10`.
    let d = diagnostics_for(
        r#"<?php
            /** @param int<10, max> $x */
            function needsNarrower(int $x): void {}
            function test(int $x): void {
                if ($x < 10) {
                    return;
                }
                needsNarrower($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn comparison_against_a_non_literal_bound_is_never_narrowed() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, max> $x */
            function needsNarrower(int $x): void {}
            function test(int $x, int $limit): void {
                if ($x < $limit) {
                    needsNarrower($x);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert!(d[0].message.contains("int given"), "got: {d:?}");
}

#[test]
fn comparison_on_a_non_numeric_current_type_is_never_narrowed() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int<0, max> $x */
            function needsNarrower(int|string $x): void {}
            function test(int|string $x): void {
                if ($x < 10) {
                    needsNarrower($x);
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// A narrow slice of real PHPStan's own `ConditionalExpressionHolder` -
// correlating a second variable's narrowing to a first variable's own
// truthiness, for the single-hop case (`$ok = <condition>; if ($ok)
// {...}`), by storing the RHS condition expression and replaying it
// through the already-correct `apply_narrowing` recursion.

#[test]
fn bool_variable_alias_from_instanceof_narrows_on_branch() {
    let d = diagnostics_for(
        r#"<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function f(Base1 $x): void {
                $isFoo = $x instanceof Foo1;
                if ($isFoo) {
                    $x->fooOnlyMethod();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bool_variable_alias_negated_branch_narrows_the_false_side() {
    let d = diagnostics_for(
        r#"<?php
            class Base1 {}
            function sink1(Base1 $x): void {}
            function f(?Base1 $x): void {
                $isNull = $x === null;
                if (!$isNull) {
                    sink1($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn plain_alias_assignment_correlates_truthiness() {
    let d = diagnostics_for(
        r#"<?php
            function sink2(int $x): void {}
            function f(?int $x): void {
                $y = $x;
                if ($y) {
                    sink2($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Regression guard against unsoundness: reassigning the *aliased*
// variable between the alias's own creation and the branch that
// replays it must invalidate the alias.
#[test]
fn condition_alias_invalidated_by_reassigning_the_aliased_variable() {
    let d = diagnostics_for(
        r#"<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function other1(): Base1 { return new Base1(); }
            function f(Base1 $x): void {
                $ok = $x instanceof Foo1;
                $x = other1();
                if ($ok) {
                    $x->fooOnlyMethod();
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// Reassigning the *boolean* alias variable itself (not its target) must
// also invalidate the alias.
#[test]
fn condition_alias_invalidated_by_reassigning_the_bool_variable_itself() {
    let d = diagnostics_for(
        r#"<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function otherBool1(): bool { return false; }
            function f(Base1 $x): void {
                $ok = $x instanceof Foo1;
                $ok = otherBool1();
                if ($ok) {
                    $x->fooOnlyMethod();
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// Exercises the `Rc::ptr_eq` merge path: an unrelated branch that
// touches neither the alias variable nor its target must not disturb
// the alias.
#[test]
fn condition_alias_survives_an_unrelated_branch_merge() {
    let d = diagnostics_for(
        r#"<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function unrelatedCond1(): bool { return true; }
            function doSomething1(): void {}
            function f(Base1 $x): void {
                $ok = $x instanceof Foo1;
                if (unrelatedCond1()) {
                    doSomething1();
                }
                if ($ok) {
                    $x->fooOnlyMethod();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Exercises the merge correctly *dropping* the alias when only one side
// of a branch reassigns the target.
#[test]
fn condition_alias_does_not_survive_a_branch_that_only_sometimes_reassigns() {
    let d = diagnostics_for(
        r#"<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function unrelatedCond2(): bool { return true; }
            function other2(): Base1 { return new Base1(); }
            function f(Base1 $x): void {
                $ok = $x instanceof Foo1;
                if (unrelatedCond2()) {
                    $x = other2();
                }
                if ($ok) {
                    $x->fooOnlyMethod();
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
}

// Regression guard for the self-reference guard in
// `Checker::record_condition_alias` - must not infinite-loop or
// misbehave.
#[test]
fn condition_alias_self_reference_guard() {
    let d = diagnostics_for(
        r#"<?php
            class Base1 {}
            class Foo1 extends Base1 {
                public function fooOnlyMethod(): void {}
            }
            function f(Base1 $x): void {
                $x = $x instanceof Foo1;
                if ($x) {
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `record_branch_implied_facts`/`apply_equality_narrowing`'s own replay
// hook: a discriminant tested a second time, with no lexical
// relationship to the branch that first proved the correlation, still
// recalls that the other variable was set.
#[test]
fn a_discriminant_retested_later_implies_a_previously_set_variable_is_non_null() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum ConnectedWith {
                case ApiKey;
                case Password;
            }
            function f(ConnectedWith $valid_key): void {
                $secret_key = null;
                if ($valid_key === ConnectedWith::ApiKey) {
                    $secret_key = "abc";
                }
                echo "unrelated code in between";
                if ($valid_key === ConnectedWith::ApiKey) {
                    assertType("'abc'", $secret_key);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_different_discriminant_value_does_not_imply_the_correlated_variable() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum ConnectedWith {
                case ApiKey;
                case Password;
            }
            function f(ConnectedWith $valid_key): void {
                $secret_key = null;
                if ($valid_key === ConnectedWith::ApiKey) {
                    $secret_key = "abc";
                }
                if ($valid_key === ConnectedWith::Password) {
                    // Nothing narrows $secret_key here - the recorded
                    // fact is keyed to the ApiKey discriminant
                    // specifically, not a general "valid_key was
                    // compared" signal - so it stays whatever the first
                    // if's own ordinary merge already produced.
                    assertType("'abc'|null", $secret_key);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `record_branch_implied_facts`'s own boolean-flag-proxy subject -
// `AlbumsPageRenderer::render`'s own real shape: the *tested* condition
// (`str_starts_with(...)`) is never re-evaluated verbatim anywhere else,
// but a plain local flag it sets to a literal `true` inside that same
// branch (`$orderByDate`) is re-tested bare later, with no lexical
// relationship to the original `if` - a real proxy for "that branch ran."
#[test]
fn a_flag_literal_assigned_inside_a_branch_is_a_proxy_discriminant() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $orderByField): void {
                $orderByDate = false;
                if (str_starts_with($orderByField, 'date_')) {
                    $orderByDate = true;
                    $refDates = [1, 2, 3];
                }
                echo "unrelated code in between";
                if ($orderByDate) {
                    assertType('array{0: 1, 1: 2, 2: 3}', $refDates);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `record_branch_implied_facts`'s own concrete-object-proxy subject,
// replayed via `replay_object_discriminant_facts` from `instanceof` -
// `UploadService::addUploadedFile`'s own real shape: a variable that
// became a concrete object specifically inside the branch, later
// re-tested via `instanceof` rather than bare truthiness.
#[test]
fn a_variable_assigned_a_concrete_object_inside_a_branch_is_a_proxy_discriminant() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Connection {}
            function f(bool $shouldLock): void {
                $conn = null;
                $name = null;
                if ($shouldLock) {
                    $conn = new Connection();
                    $name = "lockname";
                }
                echo "unrelated code in between";
                if ($conn instanceof Connection) {
                    assertType("'lockname'", $name);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `record_branch_implied_facts`'s own enum-case-proxy subject (the
// `is_enum_case_proxy` counterpart of the concrete-object-proxy case above)
// - `AuthService::authKeyLogin`'s own real shape: a variable set to an enum
// case (never previously introduced as a `subjects` candidate) correlates
// with another variable set alongside it in the same branch, replayed via
// an equality comparison several statements later after intervening
// unrelated code.
#[test]
fn a_variable_assigned_an_enum_case_inside_a_branch_is_a_proxy_discriminant() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            enum ConnectedWith {
                case AuthKey;
                case ApiKey;
            }
            function f(bool $isApiKey): void {
                $validKey = null;
                $secretKey = null;
                if ($isApiKey) {
                    $validKey = ConnectedWith::ApiKey;
                    $secretKey = "s3cr3t";
                }
                echo "unrelated code in between";
                if ($validKey === ConnectedWith::ApiKey) {
                    assertType("'s3cr3t'", $secretKey);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Real corpus regression from the `is_enum_case_proxy` addition above -
// `League\Uri\QueryString::composeRecursive`'s own shape: a variable
// (`$value`) whose own `instanceof`-narrowed type is unrelated to a
// *different* discriminant (`$mode`) that also happens to become an enum
// case within the same branch. `$value`'s own name appears directly in
// this branch's own condition, so it must never become an implied-fact
// candidate for `$mode`'s discriminant - on a second loop pass, replaying
// a stale fact here would overwrite `$value`'s freshly re-narrowed
// `instanceof` type with the first pass's own leftover `array` value,
// right before the real `get_object_vars()` call that needs the object
// type.
#[test]
fn a_variable_named_in_its_own_branch_condition_is_never_an_implied_fact_for_an_unrelated_discriminant()
 {
    let d = diagnostics_for(
        r#"<?php
            enum Mode {
                case Compatible;
                case Lenient;
            }
            interface BackedEnum {}
            interface UnitEnum {}
            /** @param list<mixed> $data */
            function f(Mode $mode, array $data): void {
                foreach ($data as $value) {
                    if ($value instanceof BackedEnum) {
                        if (Mode::Compatible !== $mode) {
                            continue;
                        }
                        $value = get_object_vars($value);
                    }
                    if ($value instanceof UnitEnum) {
                        if (Mode::Lenient === $mode) {
                            continue;
                        }
                        $value = get_object_vars($value);
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `record_branch_implied_facts`'s own facts surviving into a closure body
// (Phase 175) - `RateService::rate`'s own real shape: an outer `if
// ($userAnonymous) { $savedAnonymousId = ...; } else { $savedAnonymousId =
// null; }`, then a `use (...)` closure re-testing `$userAnonymous` as part
// of a compound condition. `fresh_scope()` starts a closure's own
// `branch_implied_facts` empty, so without carrying the relevant facts
// across at closure-construction time, this never replays.
#[test]
fn a_discriminant_fact_survives_into_a_use_capturing_closure() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function transactional(callable $cb): void {
                $cb();
            }
            function f(bool $userAnonymous, string $anonymousId): void {
                if ($userAnonymous) {
                    $savedAnonymousId = "abc";
                } else {
                    $savedAnonymousId = null;
                }
                transactional(function () use ($userAnonymous, $anonymousId, $savedAnonymousId): void {
                    if ($userAnonymous && $anonymousId !== $savedAnonymousId) {
                        assertType("'abc'", $savedAnonymousId);
                    }
                });
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_implied_fact_naming_an_uncaptured_variable_is_dropped_at_the_closure_boundary() {
    // The mirror-image safety check: if `$other` (never captured) still
    // somehow got inserted into the closure's own `Locals` via a replayed
    // fact, this would wrongly suppress a real `variable.undefined` here
    // instead of reporting it.
    let d = diagnostics_for(
        r#"<?php
            function transactional(callable $cb): void {
                $cb();
            }
            function f(bool $flag): void {
                if ($flag) {
                    $other = "abc";
                }
                transactional(function () use ($flag): void {
                    if ($flag) {
                        echo $other;
                    }
                });
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "variable.undefined"),
        "got: {d:?}"
    );
}

// `is_a()` narrowing - the runtime-value-spelled twin of `instanceof`,
// reusing its identical narrowing primitives.
#[test]
fn is_a_with_a_class_const_target_narrows_like_instanceof() {
    let d = diagnostics_for(
        r#"<?php
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if (is_a($animal, Cat::class)) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn is_a_with_a_string_literal_target_narrows_like_instanceof() {
    let d = diagnostics_for(
        r#"<?php
            class Cat { public function meow(): string { return "meow"; } }
            function useCat(Cat $c): string { return $c->meow(); }
            function handle($animal) {
                if (is_a($animal, 'Cat')) {
                    return useCat($animal);
                }
                return "";
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A compound `||` condition assigned to a flag variable, then reused
// later - `condition_alias_shape` now accepts a `Binary(And|Or, ...)`
// RHS when both sides share the same narrowing target, and the replay
// needs no new narrowing logic at all: `apply_narrowing`'s own
// existing `Or ... if !branch_true` arm already recurses into each
// disjunct independently.
#[test]
fn a_flag_holding_a_compound_or_equality_condition_narrows_on_replay() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(?int $parentId): void {
                $parentIdIsEmpty = $parentId === null || $parentId === 0;
                if (! $parentIdIsEmpty) {
                    assertType('int', $parentId);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_flag_holding_a_compound_condition_spanning_two_subjects_narrows_both_on_replay() {
    // `PasswordController::processVerificationCode`'s own real shape -
    // unlike the single-subject case above, `$isUserFound`/`$email` are
    // two genuinely different variables, and the first leaf is negated
    // (`! $isUserFound`, not a bare equality/predicate call).
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(bool $isUserFound, ?string $email): void {
                $skipMail = ! $isUserFound || $email === null || $email === '';
                if (! $skipMail) {
                    assertType('non-empty-string', $email);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn rebinding_either_subject_of_a_multi_target_flag_alias_invalidates_it() {
    // If rebinding `$email` didn't purge the alias, `if (! $skipMail)`
    // would incorrectly replay the *stale* `$email === null` narrowing
    // and claim `$email` excludes `null` here, when it's really just
    // whatever `$other` holds.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(bool $isUserFound, ?string $email, ?string $other): void {
                $skipMail = ! $isUserFound || $email === null;
                $email = $other;
                if (! $skipMail) {
                    assertType('string|null', $email);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_flag_holding_a_compound_and_inequality_condition_narrows_on_replay() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(?int $x): void {
                $flag = $x !== null && $x !== 0;
                if ($flag) {
                    assertType('int', $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The direct-condition form (no intermediate flag variable) of the
// same shape, via `or_chain_narrowing_target`'s own new equality leaf.
#[test]
fn an_or_chain_of_equality_comparisons_narrows_directly_to_the_union() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(int|string|bool $x): void {
                if ($x === 1 || $x === "a") {
                    assertType("1|'a'", $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A nullsafe chain proven non-null implies its receiver is non-null too.
#[test]
fn a_directly_tested_nullsafe_method_call_implies_the_receiver_is_non_null() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class A { public function b(): string { return "x"; } }
            function f(?A $a): void {
                if ($a?->b() !== null) {
                    assertType('A', $a);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_assigned_then_tested_nullsafe_property_access_implies_the_receiver_is_non_null() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class A { public ?string $b = null; }
            function f(?A $a): void {
                $x = $a?->b;
                if ($x !== null) {
                    assertType('A', $a);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Same-condition-expression recurrence (Phase 170) - a ternary reusing
// the identical bare-variable condition its own subject's assignment
// was gated on, one line further down.
#[test]
fn a_ternary_reusing_an_earlier_if_s_identical_condition_inherits_its_effect() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(bool $sendDetail): void {
                $news = null;
                if ($sendDetail) {
                    $news = "abc";
                }
                $result = $sendDetail ? $news : null;
                assertType("'abc'|null", $result);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `isset($image_id)` re-checked a second time, as a fresh `if`/`else` -
// the second `else` branch inherits the first `else` branch's own
// effect.
#[test]
fn isset_re_checked_a_second_time_inherits_the_first_else_branch_s_effect() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(): void {
                $dbnow = null;
                if (isset($image_id)) {
                } else {
                    $dbnow = "now";
                }
                echo "unrelated code in between";
                if (isset($image_id)) {
                } else {
                    assertType("'now'", $dbnow);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Disjunctive syllogism (Phase 162): `if ($a || $b) { if ($b) {} else {
// /* $a must be true */ } }`.
#[test]
fn disjunctive_syllogism_narrows_the_other_disjunct_true() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(?string $exif, ?string $exif2): void {
                if ((bool) $exif || (bool) $exif2) {
                    if ((bool) $exif2) {
                    } else {
                        assertType('non-falsy-string', $exif);
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn disjunctive_syllogism_does_not_fire_outside_a_pending_or_disjunction() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(?string $exif, ?string $exif2): void {
                if ((bool) $exif2) {
                } else {
                    // No enclosing `||` pushed anything - $exif stays
                    // whatever it already was, not narrowed to non-null.
                    assertType('string|null', $exif);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A flag variable set unconditionally *inside* an `if`'s own body (`if
// (A || B) { $flag = true; }`, not a direct `$flag = A || B;` assignment
// expression) still correlates with the original condition later -
// confirmed real shape:
// `ImageDerivativeController::__invoke`'s own `$need_generate = false;
// if ($derivative_mtime === false or $derivative_mtime < $src_mtime or
// ...) { $need_generate = true; } ...; if (! $need_generate) { ...
// gmdate(..., $derivative_mtime) ... }`.
#[test]
fn a_flag_set_inside_an_if_bodys_own_statement_correlates_with_the_original_condition() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): void {}
            function f(int|false $mtime, int $threshold): void {
                $need_generate = false;
                if ($mtime === false or $mtime < $threshold) {
                    $need_generate = true;
                }
                if (! $need_generate) {
                    needsInt($mtime);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The same shape, but with a bare (brace-less) single-statement body -
// `record_if_body_flag_alias` must unwrap both forms identically.
#[test]
fn a_flag_set_inside_a_brace_less_if_body_also_correlates() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): void {}
            function f(int|false $mtime): void {
                $need_generate = false;
                if ($mtime === false) $need_generate = true;
                if (! $need_generate) {
                    needsInt($mtime);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Negative guard against over-firing: a branch that sets the flag
// *alongside* another real effect isn't provably "this flag's value is
// fully determined by the condition alone" - no alias should be
// recorded, so the later read stays exactly as imprecise as it already
// was (still no false claim, just no improvement).
#[test]
fn a_flag_set_alongside_another_effect_in_the_same_branch_is_not_aliased() {
    let d = diagnostics_for(
        r#"<?php
            function needsInt(int $x): void {}
            function sideEffect(): void {}
            function f(int|false $mtime): void {
                $need_generate = false;
                if ($mtime === false) {
                    $need_generate = true;
                    sideEffect();
                }
                if (! $need_generate) {
                    needsInt($mtime);
                }
            }
        "#,
    );
    assert_eq!(
        d.len(),
        1,
        "the flag-only-effect restriction should leave this unimproved: {d:?}"
    );
}

// Regression test for `Locals::may_have_member_narrowing` (see
// `check_expr`/`infer_expr`'s `PropertyAccess`/`Index`/`StaticAccess`
// arms): a property-chain narrowing entry (`narrowing_key`'s own
// `"a->value"` synthetic key) is written independently inside each side
// of an `if`/`else` (via each branch's own `is_string(...)`-guarded early
// return, so the outer `if`/`else` itself doesn't terminate on either
// side and genuinely goes through `merge_scopes`'s cross-branch join,
// not just "the surviving branch's own locals"), then read back once
// past the merge. `may_have_member_narrowing` must still read `true`
// after the merge - `merge_scopes`'s own `types`-merging loop routes the
// surviving entry through `Locals::insert_with_provenance`, which is one
// of the three places that flag gets set - or this would silently regress
// to the pre-fix behavior of never reading the narrowed type back at all.
#[test]
fn a_property_narrowed_independently_in_both_if_and_else_branches_survives_the_merge() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Box { public string|int $value = 0; }
            function f(Box $a, bool $cond): void {
                if ($cond) {
                    if (!is_string($a->value)) {
                        return;
                    }
                } else {
                    if (!is_string($a->value)) {
                        return;
                    }
                }
                assertType('string', $a->value);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `container_field_facts` (`checker.rs`) - a by-ref `foreach`'s own
// per-field writes are now visible after the loop, through *any* later
// variable bound from the same container, not just the by-ref alias
// itself. Mirrors the exact `ConfigurationSubController::processSizes`
// corpus shape: a by-ref foreach writes several fields (some
// unconditionally, some across both arms of an `if`/`else`), an
// *intervening*, unrelated loop runs (exercising `merge_scopes`'s own
// intersection rule for this fact - the real target has exactly this
// shape), then a *separate*, later, non-by-ref `foreach` reads the
// fields back through a differently-named value variable.
#[test]
fn a_by_ref_foreachs_per_field_writes_are_visible_through_a_later_differently_named_read() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function f(array $rows): void {
                foreach ($rows as $type => &$row) {
                    if ($row['flag_a'] = ($type === 'x' ? true : false)) {
                        $row['flag_c'] = 1;
                    } else {
                        $row['flag_c'] = 2;
                    }
                    $row['flag_b'] = true;
                }
                unset($row);

                foreach (['a', 'b'] as $t) {
                    $tmp = 1;
                }

                foreach ($rows as $t2 => $prow) {
                    assertType('true|false', $prow['flag_a']);
                    assertType('true', $prow['flag_b']);
                    assertType('1|2', $prow['flag_c']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// The other real-world binding shape: a plain `$x = $container[$dyn];`
// assignment (not a `foreach` value), mirroring
// `ConfigurationSubController.php:1076`'s own
// `$pderivative = $pderivatives[$type];` read.
#[test]
fn a_plain_assignment_from_a_dynamic_key_read_also_sees_the_container_field_facts() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function f(array $rows, string $key): void {
                foreach ($rows as $type => &$row) {
                    $row['flag_b'] = true;
                }
                unset($row);

                $one = $rows[$key];
                assertType('true', $one['flag_b']);
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Negative test: a field set in only *one* arm of an `if`/`else`, with no
// corresponding write in the other arm, must not get a false-precision
// fact - `merge_scopes`'s own existing, already-correct "present in only
// one branch gets dropped" rule for `types` entries already guarantees
// this (the harvest only ever reads what's still present in `pass_locals`
// by the time it runs), but this test exercises that guarantee through
// this specific mechanism rather than assuming it holds.
#[test]
fn a_field_set_in_only_one_branch_does_not_get_a_false_precision_fact() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function f(array $rows): void {
                foreach ($rows as $type => &$row) {
                    if ($type === 'x') {
                        $row['maybe'] = true;
                    }
                }
                unset($row);

                foreach ($rows as $t2 => $prow) {
                    assertType('string|int|bool|null', $prow['maybe']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Invalidation: the container is reassigned wholesale between the by-ref
// loop and the later read - the stale fact must not survive that rebind.
#[test]
fn reassigning_the_container_wholesale_invalidates_its_recorded_field_facts() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array<string, array<string, string|int|bool|null>> $rows
             * @param array<string, array<string, string|int|bool|null>> $other
             */
            function f(array $rows, array $other): void {
                foreach ($rows as $type => &$row) {
                    $row['flag_b'] = true;
                }
                unset($row);

                $rows = $other;

                foreach ($rows as $t2 => $prow) {
                    assertType('string|int|bool|null', $prow['flag_b']);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// Invalidation: the container is passed by reference to an unrecognized
// function between the by-ref loop and the later read - this goes
// through `Locals::insert`, not `declare`, so it needs
// `purge_stale_narrowing_for`'s own call at that by-ref-widening site
// (`checker/calls.rs`) to correctly drop the stale fact too.
#[test]
fn passing_the_container_by_reference_to_an_unrecognized_function_invalidates_its_field_facts() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function f(array $rows): void {
                foreach ($rows as $type => &$row) {
                    $row['flag_b'] = true;
                }
                unset($row);

                mutateSomehow($rows);

                foreach ($rows as $t2 => $prow) {
                    assertType('string|int|bool|null', $prow['flag_b']);
                }
            }

            /** @param array<string, array<string, string|int|bool|null>> $rows */
            function mutateSomehow(array &$rows): void {}
        "#,
    );
    assert!(d.is_empty(), "assertType() mismatch(es): {d:?}");
}

// `DiscriminantArrayFactsMap`/`Checker::active_discriminant` (`checker.rs`)
// - `write_index_into`'s own generic `Type::Union` handling
// (`checker/assignment.rs`) applies an append write to *every* member of
// an already-diverged union uniformly, including a member produced by a
// *different* loop iteration's *other* `if`/`else` branch - not a
// `merge_scopes` flattening bug (confirmed by direct trace: `union_types`
// never merges two different `Array`/`Intersection` element types
// together on its own). This mechanism separately tracks each branch's
// own accumulator, consulted only when checking a `return` statement
// against a `Type::Conditional` declared type that the ordinary
// (still-corrupted) tracked type would otherwise fail.
#[test]
fn a_branch_gated_accumulator_satisfies_its_own_conditional_return_type() {
    let d = diagnostics_for(
        r#"<?php
            class UserMailNotification {}
            /**
             * @param list<UserMailNotification> $dataUsers
             * @return ($isActionSend is true ? list<string> : list<UserMailNotification>)
             */
            function f(array $dataUsers, bool $isActionSend): array {
                $returnList = [];
                foreach ($dataUsers as $nbmUser) {
                    if ($isActionSend) {
                        $returnList[] = 'checkKey';
                    } else {
                        $returnList[] = $nbmUser;
                    }
                }
                return $returnList;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Negative control: a push that happens *unconditionally* (no recognized
// `if`/`else` discriminant at all) never populates `discriminant_array_facts`,
// so a genuine, real mismatch must still be flagged exactly as before -
// this mechanism only ever *suppresses* a diagnostic when it has real
// per-branch evidence to do so, never blanket-tolerates a mismatched
// conditional return.
#[test]
fn an_unconditionally_mixed_accumulator_still_fails_its_conditional_return_type() {
    let d = diagnostics_for(
        r#"<?php
            class UserMailNotification {}
            /**
             * @param list<UserMailNotification> $dataUsers
             * @return ($isActionSend is true ? list<string> : list<UserMailNotification>)
             */
            function f(array $dataUsers, bool $isActionSend): array {
                $returnList = [];
                foreach ($dataUsers as $nbmUser) {
                    $returnList[] = 'checkKey';
                    $returnList[] = $nbmUser;
                }
                return $returnList;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "expected a return.type diagnostic, got: {d:?}"
    );
}

// `Registry::is_assignable`'s own new `Type::Conditional` arm
// (`registry/assignability.rs`), independent of the discriminant
// mechanism above: without it, a plain `non-empty-list<string>`-typed
// return value (an `Intersection`, decomposed into its own individual
// `Array`/`Accessory(ArrayList)`/`Accessory(NonEmptyArray)` members by
// this function's own generic `(Intersection, _) => any(...)` rule
// *before* `sup`'s own `Conditional` shape is ever inspected) was
// wrongly rejected even for a function that never touches this
// mechanism at all - any `Type::Conditional`-returning function with a
// list-typed return value had this gap.
#[test]
fn a_plain_list_typed_return_satisfies_a_conditional_return_type_with_no_discriminant_tracking() {
    let d = diagnostics_for(
        r#"<?php
            /** @return ($flag is true ? list<string> : list<int>) */
            function f(bool $flag): array {
                return ['a', 'b'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_negated_is_object_removes_every_class_member() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class TV {}
            enum E { case A; }
            /** @param TV|E|list<int>|string $x */
            function f($x): void {
                if (!is_object($x)) {
                    assertType('list<int>|string', $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_later_match_true_arm_sees_every_condition_of_a_multi_condition_arm_false() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param int|string|null $x */
            function f($x): int {
                return match (true) {
                    is_int($x), is_null($x) => 1,
                    default => (function () use ($x) { assertType('string', $x); return 2; })(),
                };
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_inline_var_tag_on_an_assignment_applies_after_the_right_hand_side() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<mixed> $sel */
            function f(array $sel): void {
                /** @var list<int|string> $sel */
                $sel = array_values(array_filter(
                    $sel,
                    static fn ($item): bool => is_int($item) || is_string($item),
                ));
                assertType('list<int|string>', $sel);
            }
            function g(): void {
                /** @var int $a */
                [$a, $b] = [rand() ? 1 : 'x', 2];
                assertType('int', $a);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn branch_implied_facts_do_not_survive_rebinding_the_variable_they_describe() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Expr {}
            class Arr extends Expr {}
            class Arg {
                public Expr $value;
                public function __construct() { $this->value = new Expr(); }
            }
            class Call {
                /** @var array<Arg|int> */
                public array $args = [];
            }
            /** @param list<Call> $calls */
            function objects(array $calls): void {
                $seen = [];
                foreach ($calls as $call) {
                    if (! $call->args[0] instanceof Arg || ! $call->args[0]->value instanceof Arr) {
                        continue;
                    }
                    $seen[] = 1;
                }
            }
            /** @param list<array<string, mixed>> $rows */
            function flags(array $rows, bool $b): void {
                $out = [];
                foreach ($rows as $row) {
                    if ($b) {
                        $row['name'] = 'y';
                    } else {
                        assertType('mixed', $row['name']);
                        $row['name'] = strip_tags('x');
                    }
                    $out[] = $row;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn stacked_switch_cases_see_every_label_they_can_be_entered_by() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array<string, mixed> $s */
            function f(string $a, array $s): void {
                switch ($a) {
                    case 'a':
                    case 'b':
                        assertType("'a'|'b'", $a);
                        break;
                    case 'moderate':
                        $s['p'] = [];
                        // no break
                    case 'validate':
                        assertType("'moderate'|'validate'", $a);
                        if (! isset($s['p']) || ! is_array($s['p'])) {
                            $s['p'] = [];
                        }
                        break;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn isset_on_an_offset_narrows_the_container_too() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /** @param array{a?: int}|false $x */
            function f($x, string $u): void {
                if (isset($x['a'])) {
                    assertType('array{a: int}', $x);
                }
                $parts = parse_url($u);
                if (isset($parts['port'])) {
                    echo $parts['port'];
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn strict_in_array_narrows_against_a_constant_haystack() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            final class C {
                private const array ACTIONS = ['a', 'b'];
                /** @param list<int> $ids */
                public function f(string $s, int $n, array $ids): void {
                    if (! in_array($s, self::ACTIONS, true)) {
                        return;
                    }
                    assertType("'a'|'b'", $s);
                    if (in_array($n, $ids, true)) {
                        assertType('int', $n);
                    }
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_dynamic_key_write_invalidates_narrowed_sibling_entries() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(string $k, int $v): void {
                $r = ['qs' => ['q' => 'x']];
                $r['qs']['z'] = 1;
                $r[$k] = $v;
                assertType("int|array{q: 'x', z: 1}", $r['qs']);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_write_makes_an_optional_key_present_and_isset_adds_an_unlisted_one() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            /**
             * @param array{usage?: int, region?: string|null} $cache
             * @param array<string, int|null> $counts
             */
            function f(array $cache, array $counts, int $d): void {
                if (! isset($cache['usage'])) {
                    $cache['usage'] = $d;
                }
                assertType('array{usage: int, region?: string|null}', $cache);
                if (isset($counts['x'])) {
                    assertType("array<string, int|null>&hasOffsetValue('x', int)", $counts);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_nameless_var_tag_on_a_return_types_the_returned_value() {
    let d = diagnostics_for(
        r#"<?php
            /** @return non-empty-string */
            function f(string $s): string {
                /** @var non-empty-string */
                return strtolower($s);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
