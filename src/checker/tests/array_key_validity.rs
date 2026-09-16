//! Array-key legality: illegal key types for array-literal/dim-fetch access,
//! and duplicate/colliding array-literal keys.

use super::helpers::*;

// An array literal key must be a type PHP itself accepts as an array key
// at runtime.

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
    // PHP silently coerces these at runtime - not a hard TypeError the
    // way an array/object key is. No php_version configured here, so the
    // float/null deprecation checks below stay silent too, matching the
    // "unconfigured means don't check" convention for every other
    // PhpFeature-gated rule.
    let d = diagnostics_for(
        r#"<?php
            function test(bool $b, float $f, ?string $n): array {
                return [$b => 1, $f => 2, $n => 3];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// With a target php_version configured, additionally exclude a float
// key on PHP >= 8.1 and a null key on PHP >= 8.5.

#[test]
fn a_float_array_literal_key_is_flagged_on_php_8_1_or_later() {
    let d = diagnostics_for_with(
        r#"<?php
            function test(float $f): array {
                return [$f => 1];
            }
        "#,
        |r| r.php_version = Some((8, 1)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.invalidKey"
            && x.message.contains("Invalid array key type float")),
        "got: {d:?}"
    );
}

#[test]
fn a_float_array_literal_key_is_never_flagged_below_php_8_1() {
    let d = diagnostics_for_with(
        r#"<?php
            function test(float $f): array {
                return [$f => 1];
            }
        "#,
        |r| r.php_version = Some((8, 0)),
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_null_array_literal_key_is_flagged_on_php_8_5_or_later() {
    // A bare `null` literal, not `?string` - a *union* including a legal
    // member (`string`) would bail out of the whole check entirely (this
    // project's own "at least one member legal, stay silent" leniency),
    // so this needs a definite, non-union `Type::Null` to exercise the
    // new arm at all.
    let d = diagnostics_for_with(
        r#"<?php
            function test(): array {
                return [null => 1];
            }
        "#,
        |r| r.php_version = Some((8, 5)),
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.invalidKey"
            && x.message.contains("Invalid array key type null")),
        "got: {d:?}"
    );
}

#[test]
fn a_null_array_literal_key_is_never_flagged_below_php_8_5() {
    let d = diagnostics_for_with(
        r#"<?php
            function test(): array {
                return [null => 1];
            }
        "#,
        |r| r.php_version = Some((8, 4)),
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

// `offsetAccess.invalidOffset` applies the exact same legal-key-type
// check as `array.invalidKey` above, to a dim-fetch's own dimension
// expression (`$arr[$key]`) instead of a literal's key. A used generic
// template parameter must stay `Type::TemplateParam` here, not leak
// through as a bogus namespace-qualified class reference.

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
    // A still-generic `T` reaching this check (not yet substituted with
    // a concrete type) must stay leniently unflagged, not misread as a
    // bogus, namespace-qualified class reference.
    // `invalid_array_key_description` has no arm for `Type::
    // TemplateParam` at all, so it falls to the safe "don't know"
    // catch-all.
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
    // `@phpstan-import-type Name from Class` (importing a `@phpstan-type`
    // alias declared on a different class) is a distinct, separate tag
    // this project doesn't parse (`DocBlock::type_aliases`'s own doc
    // comment, `src/docblock.rs`) - the unresolved alias name falls
    // through as a bogus, unregistered `Object`, which this check must
    // treat as "don't know," not a confirmed-illegal key type. The
    // separate `class.notFound` existence check is expected to fire on
    // it independently; only `offsetAccess.invalidOffset` must not.
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
    // A class-level `@template T` referenced inside an inline `/** @var
    // Type<T> $type */` annotation is parsed independently of the
    // registry-build pipeline (where `mark_template_params` always runs
    // before name resolution), so `T` must still be recognized here, not
    // treated as indistinguishable from a genuine, unqualified class
    // reference and resolved (then flagged) as one.
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
    // A top-level function's `@template Tk of array-key` referenced via
    // `/** @var Tk $key */` - no enclosing class at all, so the
    // class-level fix alone doesn't cover it; needs `Checker::
    // current_function_template_params` too. The `namespace` statement
    // is deliberate, not incidental: a global-namespace function's bare
    // name already equals its FQN, so a fixture without one wouldn't
    // actually exercise `current_function_template_params`'s fix
    // (`Registry::lookup_function` needs the real FQN,
    // `self.name_ctx.declared_name(...)` - a plain bare-name lookup
    // silently finds nothing for any namespaced function).
    //
    // `$keyFunc`'s own `(Closure(Tv): ?Tk)` type is copied verbatim from
    // the real signature too (not simplified to a bare `\Closure`) - `Tk`
    // is only ever referenced there, nested inside the closure's return-
    // type slot, never as a parameter's bare top-level type, so the
    // template-in-parameter scan must recurse into `Type::Callable` too,
    // not just an ordinary class's generic argument list.
    let d = diagnostics_for(
        r#"<?php
            namespace Psl\Dict;
            /**
             * @template Tk of array-key
             * @template Tv
             * @param iterable<Tv> $values
             * @param (Closure(Tv): ?Tk) $keyFunc
             * @return array<Tk, list<Tv>>
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

// `array.duplicateKey` - a later literal key silently overwrites an
// earlier one at runtime, almost always a copy-paste bug.

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
fn a_duplicate_negative_int_key_in_an_array_literal_is_flagged() {
    // A negative key must be recognized as a real, duplicate-checkable
    // int key, not rejected outright for its leading `-`.
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return [-5 => 'a', -3 => 'b', -5 => 'c'];
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

// Key resolution goes through `infer_expr`, so it's not limited to a
// bare AST literal - a class constant, a `define()`d global constant, or
// a literal bool/null key are all statically-known-scalar keys.

#[test]
fn a_duplicate_key_via_a_class_constant_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class C { const KEY = 'a'; }
            function f(): array {
                return [C::KEY => 1, 'a' => 2];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn a_resolved_int_key_via_a_class_constant_advances_the_next_auto_index() {
    // C::KEY resolves to the int key 0 - the following bare item must
    // correctly take key 1, not (wrongly) 0 too, so this must stay
    // unflagged. A version of this widened resolution that forgot to
    // advance `next_index` for a class-constant-resolved key would
    // wrongly see a collision here.
    let d = diagnostics_for(
        r#"<?php
            class C { const KEY = 0; }
            function f(): array {
                return [C::KEY => 'x', 'y'];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_duplicate_key_via_a_defined_global_constant_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            define('KEY', 'a');
            function f(): array {
                return [KEY => 1, 'a' => 2];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn a_true_key_collides_with_the_int_key_one() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return [true => 'a', 1 => 'b'];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn a_false_key_collides_with_the_int_key_zero() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return [false => 'a', 0 => 'b'];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn a_null_key_collides_with_the_empty_string_key() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return [null => 'a', '' => 'b'];
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "got: {d:?}"
    );
}

#[test]
fn a_class_constant_key_that_does_not_infer_to_a_scalar_still_suppresses_checking() {
    // The widened resolution only understands `infer_expr` results that
    // land on `IntLiteral`/`StringLiteral`/`True`/`False`/`Null` - a class
    // constant whose own value isn't statically known that precisely
    // (here, a bare `self::class`, a `ClassString`) still bails on the
    // whole rest of the literal, the same "don't guess" leniency as any
    // other unresolvable key.
    let d = diagnostics_for(
        r#"<?php
            class C { const KEY = self::class; }
            function f(): array {
                return ['a' => 1, C::KEY => 2, 'a' => 3];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "array.duplicateKey"),
        "unexpected diagnostics: {d:?}"
    );
}

// Opt-in array-key cast warnings (PHASE_BACKLOG Phase 103's Z1,
// `array.keyCast`/`offsetAccess.keyCast`) - off by default.

#[test]
fn array_key_cast_is_off_by_default() {
    let d = diagnostics_for(
        r#"<?php
            function f(): array {
                return [true => 1, '1' => 2];
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "array.keyCast"),
        "got: {d:?}"
    );
}

#[test]
fn a_bool_literal_array_key_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): array {
                return [true => 1];
            }
        "#,
        |r| r.check_array_key_cast = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.keyCast"),
        "got: {d:?}"
    );
}

#[test]
fn a_null_literal_array_key_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): array {
                return [null => 1];
            }
        "#,
        |r| r.check_array_key_cast = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.keyCast"),
        "got: {d:?}"
    );
}

#[test]
fn a_float_literal_array_key_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): array {
                return [2.5 => 1];
            }
        "#,
        |r| r.check_array_key_cast = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.keyCast"),
        "got: {d:?}"
    );
}

#[test]
fn a_canonical_int_string_array_key_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): array {
                return ['1' => 1];
            }
        "#,
        |r| r.check_array_key_cast = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "array.keyCast"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_canonical_string_array_key_is_not_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): array {
                return ['08' => 1, '+1' => 2, '1.2' => 3];
            }
        "#,
        |r| r.check_array_key_cast = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "array.keyCast"),
        "got: {d:?}"
    );
}

#[test]
fn a_plain_int_array_key_is_not_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(): array {
                return [1 => 'a'];
            }
        "#,
        |r| r.check_array_key_cast = true,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "array.keyCast"),
        "got: {d:?}"
    );
}

#[test]
fn a_dim_fetch_with_a_cast_key_is_flagged_when_enabled() {
    let d = diagnostics_for_with(
        r#"<?php
            function f(array $a): mixed {
                return $a[true];
            }
        "#,
        |r| r.check_array_key_cast = true,
    );
    assert!(
        d.iter().any(|x| x.identifier == "offsetAccess.keyCast"),
        "got: {d:?}"
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
