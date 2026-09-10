//! Array-key legality: illegal key types for array-literal/dim-fetch access,
//! and duplicate/colliding array-literal keys.

use super::helpers::*;

// Phase 50: InvalidKeyInArrayItemRule - an array literal key must be a
// type PHP itself accepts as an array key at runtime.

#[test]
fn an_array_literal_key_typed_as_an_object_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(Foo $obj): array {
                return [$obj => 1];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "array.invalidKey");
    assert!(
        d[0].message.contains("Invalid array key type Foo"),
        "got: {d:?}"
    );
}

#[test]
fn an_array_literal_key_typed_as_an_array_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $xs */
            function test(array $xs): array {
                return [$xs => 1];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "array.invalidKey");
}

#[test]
fn ordinary_int_and_string_array_literal_keys_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function test(int $i, string $s): array {
                return [$i => 1, $s => 2, 3 => 'a', 'k' => 'b'];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bool_float_and_null_array_literal_keys_are_not_flagged_php_coerces_them() {
    // PHP silently coerces these at runtime (matches real PHPStan's own
    // default, non-strict AllowedArrayKeysTypes) - not a hard TypeError
    // the way an array/object key is.
    let d = diagnostics_for(
        r#"<?php
            function test(bool $b, float $f, ?string $n): array {
                return [$b => 1, $f => 2, $n => 3];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_mixed_typed_array_literal_key_is_not_flagged_dont_guess() {
    let d = diagnostics_for(
        r#"<?php
            function test(mixed $m): array {
                return [$m => 1];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_union_key_type_mixing_a_legal_and_illegal_member_is_not_flagged() {
    // This project has no tri-state "possibly invalid" - only a key
    // type that's confidently, entirely illegal is flagged.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test(int|Foo $k): array {
                return [$k => 1];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_refined_string_array_literal_key_is_not_flagged() {
    // An accessory-refined string (literal-string, non-empty-string,
    // ...) is still a plain legal string key - only an Array-shaped
    // base type is illegal.
    let d = diagnostics_for(
        r#"<?php
            /** @param non-empty-string $k */
            function test(string $k): array {
                return [$k => 1];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 50 (reattempt): `InvalidKeyInArrayDimFetchRule` -
// `offsetAccess.invalidOffset`, the exact same legal-key-type check as
// `InvalidKeyInArrayItemRule` above, applied to a dim-fetch's own
// dimension expression (`$arr[$key]`) instead of a literal's own key. A
// first attempt at this was built, corpus-verified, then reverted after
// finding +31 false positives, all traced to a used generic template
// parameter leaking through as a bogus namespace-qualified class
// reference instead of staying `Type::TemplateParam` - see ROADMAP.md's
// own **Reverted** section.

#[test]
fn a_dim_fetch_with_an_object_typed_key_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param int[] $arr */
            function test(array $arr, Foo $key) {
                return $arr[$key];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "offsetAccess.invalidOffset");
    assert!(
        d[0].message.contains("Invalid array key type Foo"),
        "got: {d:?}"
    );
}

#[test]
fn a_dim_fetch_write_with_an_object_typed_key_is_also_flagged() {
    // Unlike `check_shape_key_access` (which deliberately skips the
    // write side - auto-vivification), this rule fires just as much on
    // a write: the key's own type is exactly as illegal regardless of
    // whether the array grows.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /** @param int[] $arr */
            function test(array $arr, Foo $key): void {
                $arr[$key] = 1;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "offsetAccess.invalidOffset");
}

#[test]
fn ordinary_int_and_string_dim_fetch_keys_are_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @param int[] $arr */
            function test(array $arr, int $i, string $s) {
                return [$arr[$i], $arr[$s], $arr[3], $arr['k']];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_dim_fetch_on_a_non_confidently_array_base_is_never_flagged() {
    // `Mixed`, or anything else this project can't confidently prove is
    // an array at all, is "don't know" - matches this project's own
    // leniency elsewhere rather than real PHPStan's own tri-state
    // `isArray()` grading.
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function test($arr, Foo $key) {
                return $arr[$key];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_dim_fetch_on_a_union_of_arrays_is_still_checked() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            /**
             * @param int[]|string[] $arr
             */
            function test(array $arr, Foo $key) {
                return $arr[$key];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "offsetAccess.invalidOffset");
}

#[test]
fn an_unbound_generic_template_parameter_used_as_a_dim_fetch_key_is_never_flagged() {
    // Regression test for the exact real-corpus shape that caused the
    // original revert: `amphp/sync`'s own `PriorityQueue<T of
    // int|string>` - a still-generic `T` reaching this check (not yet
    // substituted with a concrete type) must stay leniently unflagged,
    // not misread as a bogus, namespace-qualified class reference.
    // `invalid_array_key_description` has no arm for `Type::
    // TemplateParam` at all, so it already falls to the safe "don't
    // know" catch-all - this pins that behavior down for this exact
    // shape rather than trusting it implicitly.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template T of int|string
             */
            class PriorityQueue {
                /** @var array<T, mixed> */
                private array $heap = [];

                /** @param T $priority */
                public function insert($priority, $value): void {
                    $this->heap[$priority] = $value;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unrecognized_phpstan_import_type_alias_used_as_a_dim_fetch_key_is_never_flagged() {
    // Regression test for a real, corpus-found residual of a pre-
    // existing, disclosed gap: `@phpstan-import-type Name from Class`
    // (importing a `@phpstan-type` alias declared on a *different*
    // class) is "a distinct, separate tag this project doesn't parse"
    // (`DocBlock::type_aliases`'s own doc comment, `src/docblock.rs`) -
    // the unresolved alias name falls through as a bogus, unregistered
    // `Object`, which this check must treat as "don't know," not a
    // confirmed-illegal key type. Real corpus shapes: `phpunit/php-code-
    // coverage`'s own `TestIndexType`, `php-cs-fixer`'s own `_ImportType`.
    //
    // `TestIndexType` genuinely isn't a registered class here (this
    // project doesn't parse `@phpstan-import-type` at all, so nothing
    // ever defines it) - Phase 48's own, separate, correct
    // `class.notFound` existence check is expected to fire on it
    // independently; only `offsetAccess.invalidOffset` must not.
    let d = diagnostics_for(
        r#"<?php
            class Holder {
                /** @phpstan-import-type TestIndexType from Holder */
                /** @var array<TestIndexType, int> */
                private array $hit = [];

                /** @param TestIndexType $testIndex */
                public function recordHit($testIndex): void {
                    $this->hit[$testIndex] = 1;
                }
            }
        "#,
    );
    assert!(
        !d.iter()
            .any(|x| x.identifier == "offsetAccess.invalidOffset"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_class_level_template_referenced_inside_an_inline_var_annotation_is_never_flagged() {
    // Regression test for the *real*, precise root cause behind the
    // original revert (see `Checker::resolve_type_hint`'s own doc
    // comment): `php-standard-library`'s own `SetType`/`MapType`/
    // `MutableSetType`, each with an inline `/** @var Type\Type<T>
    // $type */` referencing the enclosing class's own `@template T` -
    // parsed independently of the registry-build pipeline (where
    // `mark_template_params` always runs *before* name resolution), so
    // without this fix `T` was indistinguishable from a genuine,
    // unqualified class reference and got resolved (then flagged) as
    // one.
    let d = diagnostics_for(
        r#"<?php
            /**
             * @template-covariant T
             */
            interface TypeInterface {
                /** @return T */
                public function coerce(mixed $value): mixed;
            }
            /**
             * @template T of array-key
             */
            final class SetType {
                /** @param TypeInterface<T> $type */
                public function __construct(private TypeInterface $type) {}

                public function coerce(mixed $value): void {
                    /** @var TypeInterface<T> $type */
                    $type = $this->type;
                    $v = $type->coerce($value);
                    $set = [];
                    $set[$v] = $v;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_top_level_function_own_template_referenced_inside_an_inline_var_annotation_is_never_flagged() {
    // Real corpus shape found while verifying the class-level fix just
    // above against the full vendor corpus: `php-standard-library`'s own
    // top-level `Psl\Dict\group_by()`, `@template Tk of array-key`,
    // `/** @var Tk $key */` referencing it directly - no enclosing class
    // at all, so the class-level fix alone doesn't cover it; needs
    // `Checker::current_function_template_params` too. The `namespace`
    // statement is deliberate, not incidental: a *global*-namespace
    // function's own bare name already equals its FQN, so a fixture
    // without one doesn't actually exercise `current_function_template_
    // params`'s own real fix (`Registry::lookup_function` needs the
    // real FQN, `self.name_ctx.declared_name(...)` - a plain bare-name
    // lookup silently finds nothing for any namespaced function, which
    // is the overwhelmingly common real shape) - confirmed the hard way,
    // this exact test passed even before that fix when written without
    // one.
    let d = diagnostics_for(
        r#"<?php
            namespace Psl\Dict;
            /**
             * @template Tk of array-key
             * @param iterable<mixed> $values
             * @return array<Tk, mixed>
             */
            function group_by(iterable $values, \Closure $keyFunc): array {
                $result = [];
                foreach ($values as $value) {
                    $key = $keyFunc($value);
                    /** @var Tk $key */
                    $result[$key] = $value;
                }
                return $result;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 68: `array.duplicateKey` - a later literal key silently
// overwrites an earlier one at runtime, almost always a copy-paste bug.

#[test]
fn a_duplicate_string_key_in_an_array_literal_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return ['a' => 1, 'b' => 2, 'a' => 3];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn a_duplicate_int_key_in_an_array_literal_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return [1 => 'a', 2 => 'b', 1 => 'c'];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn an_explicit_int_key_colliding_with_an_implicit_one_is_flagged() {
    // The first unkeyed element claims key `0` - a later explicit
    // `0 => ...` collides with it.
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return ['a', 0 => 'b'];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn a_numeric_string_key_colliding_with_its_int_twin_is_flagged() {
    // Real PHP coerces a numeric-string key to the same int key it
    // names - `'5'` and `5` collide.
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return ['5' => 'a', 5 => 'b'];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn an_array_literal_with_all_unique_keys_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return ['a' => 1, 'b' => 2, 3 => 'c', 'd'];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_spread_array_literal_is_never_flagged_for_duplicate_keys() {
    // Not guessed at - a spread source's own key set isn't known
    // statically in general.
    let d = diagnostics_for(
        r#"<?php
            function f(array $x): array {
                return ['a' => 1, ...$x, 'a' => 2];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_computed_key_suppresses_further_duplicate_key_checking() {
    // Don't guess: once a key can't be statically resolved, nothing
    // past it can be soundly checked either (it could coincide with
    // any later key).
    let d = diagnostics_for(
        r#"<?php
            function f(string $k): array {
                return ['a' => 1, $k => 2, 'a' => 3];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "unexpected diagnostics: {d:?}"
    );
}
