//! `@phpstan-assert`/`-if-true`/`-if-false` tags - PHPStan's
//! custom-assertion-function convention: an accessor pair like
//! `hasTestFilesFile(): bool` (tagged `@phpstan-assert-if-true !null
//! $this->testFilesFile`) then `testFilesFile(): string`'s own body
//! guarding on it.

use super::helpers::*;

#[test]
fn assert_if_true_on_a_method_narrows_a_this_property_in_the_true_branch() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private ?string $testFilesFile = null;

                /** @phpstan-assert-if-true !null $this->testFilesFile */
                public function hasTestFilesFile(): bool {
                    return $this->testFilesFile !== null;
                }

                public function testFilesFile(): string {
                    if (!$this->hasTestFilesFile()) {
                        throw new \RuntimeException('none');
                    }
                    return $this->testFilesFile;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn without_the_assert_tag_the_same_shape_is_still_flagged() {
    // Guards against the test above passing for an unrelated reason -
    // confirms the tag itself is what's doing the narrowing.
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private ?string $testFilesFile = null;

                public function hasTestFilesFile(): bool {
                    return $this->testFilesFile !== null;
                }

                public function testFilesFile(): string {
                    if (!$this->hasTestFilesFile()) {
                        throw new \RuntimeException('none');
                    }
                    return $this->testFilesFile;
                }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "return.type"),
        "expected a return-type mismatch (nullable vs string) with no tag, got: {d:?}"
    );
}

#[test]
fn assert_if_false_on_a_method_narrows_in_the_false_branch() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                private ?string $x = null;

                /** @phpstan-assert-if-false !null $this->x */
                public function isXNull(): bool {
                    return $this->x === null;
                }

                public function needsX(): string {
                    if ($this->isXNull()) {
                        throw new \RuntimeException('none');
                    }
                    return $this->x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assert_if_true_narrows_a_parameter_by_position() {
    let d = diagnostics_for(
        r#"<?php
            class Assert {
                /** @phpstan-assert-if-true !null $value */
                public static function notNull($value): bool {
                    return $value !== null;
                }
            }
            function f(?string $x): string {
                if (!Assert::notNull($x)) {
                    throw new \RuntimeException('none');
                }
                return $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_unconditional_assert_tag_narrows_unconditionally_after_the_call() {
    let d = diagnostics_for(
        r#"<?php
            class Assert {
                /** @phpstan-assert string $value */
                public static function string($value): void {
                    if (!is_string($value)) {
                        throw new \RuntimeException('not a string');
                    }
                }
            }
            function needsString(string $s): void {}
            function f(mixed $x): void {
                Assert::string($x);
                needsString($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// The bare, unconditional `@phpstan-assert` lookup for an instance
// `MethodCall` must not require the receiver's type to be literally
// `Type::Object`, or it silently skips the narrowing for a
// `Union`/`Intersection` receiver - a value narrowed via `instanceof`
// from an unrelated interface, then called through.
#[test]
fn an_unconditional_assert_tag_narrows_through_an_intersection_receiver() {
    let d = diagnostics_for(
        r#"<?php
            interface View {}
            class Assertions {
                /** @phpstan-assert string $value */
                public function string($value): void {
                    if (!is_string($value)) {
                        throw new \RuntimeException('not a string');
                    }
                }
            }
            function needsString(string $s): void {}
            function f(View $view, mixed $x): void {
                if ($view instanceof Assertions) {
                    $view->string($x);
                    needsString($x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// `AssertTarget::ThisProp`'s "not already narrowed" fallback
// (`assert_target_key_and_current`, `checker/narrowing.rs`) must not
// match the receiver's type against `Type::Object` directly, or it
// silently falls to `Type::Mixed(false)` (discarding the property's real
// declared type entirely) for a `Union`/`Intersection` receiver -
// reachable even through the already-correct `ThisSelf`/
// branch-conditional call site above, since this fallback re-infers the
// receiver's type independently.
#[test]
fn assert_if_true_narrows_a_this_property_through_a_union_typed_receiver() {
    // Asserts the narrowed *type* directly (`assertType`), not merely the
    // absence of diagnostics - `Type::Mixed(false)` (the bug's own fallback) is
    // itself permissive enough to pass a plain `string $s` argument
    // without complaint, so a bare "no diagnostics" check would pass
    // vacuously whether or not the fix is in place.
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            class Base {
                public ?string $flag = null;
                /** @phpstan-assert-if-true !null $this->flag */
                public function isReady(): bool { return $this->flag !== null; }
            }
            class A extends Base {}
            class B extends Base {}
            function run(A|B $v): void {
                if ($v->isReady()) {
                    assertType('string', $v->flag);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_negated_assert_excludes_the_type_instead_of_narrowing_to_it() {
    let d = diagnostics_for(
        r#"<?php
            class Assert {
                /** @phpstan-assert !null $value */
                public static function notNull($value): void {
                    if ($value === null) {
                        throw new \RuntimeException('was null');
                    }
                }
            }
            function needsString(string $s): void {}
            function f(?string $x): void {
                Assert::notNull($x);
                needsString($x);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn assert_if_true_on_a_method_narrows_a_union_typed_receiver() {
    // A method tagged `@phpstan-assert-if-true A|B $this`, called on a
    // variable typed as the union of every concrete subclass - every one
    // of which inherits the identical method.
    let d = diagnostics_for(
        r#"<?php
            class Base {
                /** @phpstan-assert-if-true A|B $this */
                public function isAB(): bool { return true; }
            }
            class A extends Base { public int $x = 1; }
            class B extends Base { public int $x = 1; }
            class C extends Base {}
            function run(A|B|C $v): void {
                if ($v->isAB()) {
                    echo $v->x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn narrowing_a_union_to_a_transitively_implemented_interface_keeps_every_matching_member() {
    // An interface a concrete class implements only transitively,
    // through an intermediate abstract class, never directly - narrowing
    // a union against it must keep every member that's assignable to
    // the interface, not just ones that equal it structurally, or every
    // member's additional properties/methods are lost outright.
    let d = diagnostics_for(
        r#"<?php
            interface Marker {}
            class Base {
                /** @phpstan-assert-if-true Marker $this */
                public function isMarked(): bool { return true; }
            }
            abstract class Mid extends Base implements Marker {}
            class LeafA extends Mid { public int $x = 1; }
            class LeafB extends Mid { public int $x = 1; }
            function run(LeafA|LeafB $v): void {
                if ($v->isMarked()) {
                    echo $v->x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn without_narrowing_the_union_typed_receiver_is_still_flagged() {
    // Guards against the test above passing for an unrelated reason -
    // dropping the assert tag should bring the same diagnostic back.
    let d = diagnostics_for(
        r#"<?php
            class Base {
                public function isAB(): bool { return true; }
            }
            class A extends Base { public int $x = 1; }
            class B extends Base { public int $x = 1; }
            class C extends Base {}
            function run(A|B|C $v): void {
                if ($v->isAB()) {
                    echo $v->x;
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "property.notFound");
}

// `AssertRuleHelper`'s existence half - the tag's type text, not just
// its target, is checked for unknown classes.

#[test]
fn an_assert_tag_naming_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-assert NoSuchClass $x */
            function assertNoSuchClass($x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn an_assert_if_true_tag_naming_an_unknown_class_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @phpstan-assert-if-true NoSuchClass $x */
                public function check($x): bool { return true; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn an_assert_tag_naming_a_known_class_is_not_flagged_for_existence() {
    let d = diagnostics_for(
        r#"<?php
            class RealClass {}
            /** @phpstan-assert RealClass $x */
            function assertRealClass($x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "unexpected diagnostics: {d:?}"
    );
}

// `AssertRuleHelper`'s deeper declaration-validity checks.

#[test]
fn an_assert_target_naming_a_nonexistent_parameter_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-assert string $notAnActualParam */
            function f($x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "parameter.notFound"
            && x.message == "Assert references unknown parameter $notAnActualParam."),
        "got: {d:?}"
    );
}

#[test]
fn an_assert_target_naming_a_method_call_is_flagged_unknown_expr() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {
                /** @phpstan-assert-if-false !null $this->parseError() */
                public function isValid(): bool { return true; }
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assert.unknownExpr"
            && x.message == "Assert references unknown $this->parseError()."),
        "got: {d:?}"
    );
}

#[test]
fn an_assert_tag_naming_a_trait_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            trait T {}
            /** @phpstan-assert T $x */
            function f($x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assert.trait"),
        "got: {d:?}"
    );
}

#[test]
fn asserting_a_scalar_kind_disjoint_from_the_parameters_declared_type_is_impossible() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-assert string $x */
            function f(int $x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assert.impossibleType"
            && x.message == "Asserted type string for $x with type int can never happen."),
        "got: {d:?}"
    );
}

#[test]
fn asserting_a_type_the_parameter_is_already_declared_as_is_already_narrowed() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-assert int $x */
            function f(int $x): void {}
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "assert.alreadyNarrowedType"
                && x.message
                    == "Asserted type int for $x with type int does not narrow down the type."),
        "got: {d:?}"
    );
}

#[test]
fn a_negated_assert_of_an_already_declared_type_is_impossible() {
    // Asserting "NOT int" when the parameter is already always `int` can
    // never be true - negation flips which branch means what.
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-assert !int $x */
            function f(int $x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assert.impossibleType"
            && x.message == "Asserted negated type int for $x with type int can never happen."),
        "got: {d:?}"
    );
}

#[test]
fn a_negated_assert_disjoint_from_the_declared_type_is_already_narrowed() {
    // Asserting "NOT string" when the parameter is already always `int`
    // is always true anyway (redundant) - not impossible.
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-assert !string $x */
            function f(int $x): void {}
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assert.alreadyNarrowedType"
            && x.message
                == "Asserted negated type string for $x with type int does not narrow down the type."),
        "got: {d:?}"
    );
}

#[test]
fn asserting_a_real_narrowing_shape_is_never_flagged_impossible_or_already_narrowed() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-assert positive-int $x */
            function f(int $x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "assert.impossibleType" | "assert.alreadyNarrowedType"
        )),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn asserting_against_an_untyped_parameter_is_never_flagged_impossible_or_already_narrowed() {
    let d = diagnostics_for(
        r#"<?php
            /** @phpstan-assert string $x */
            function f($x): void {}
        "#,
    );
    assert!(
        !d.iter().any(|x| matches!(
            x.identifier.as_str(),
            "assert.impossibleType" | "assert.alreadyNarrowedType"
        )),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_psalm_assert_tag_on_a_static_assertion_method_narrows_its_argument() {
    // `webmozart/assert`'s own shape: every assertion method carries only
    // a `@psalm-assert` tag, which real PHPStan honors when no
    // `@phpstan-assert` is present.
    let d = diagnostics_for(
        r#"<?php
            class Assert {
                /** @psalm-assert string $value */
                public static function string(mixed $value): void {
                    if (!is_string($value)) {
                        throw new \InvalidArgumentException('not a string');
                    }
                }
            }

            function takesString(string $s): void {}

            function test(int|string $input): void {
                Assert::string($input);
                takesString($input);
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn without_a_psalm_assert_tag_the_same_call_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Assert {
                public static function string(mixed $value): void {
                    if (!is_string($value)) {
                        throw new \InvalidArgumentException('not a string');
                    }
                }
            }

            function takesString(string $s): void {}

            function test(int|string $input): void {
                Assert::string($input);
                takesString($input);
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "argument.type"),
        "expected argument.type without the tag, got: {d:?}"
    );
}

#[test]
fn a_psalm_param_tag_refines_a_plain_param_tag() {
    let d = diagnostics_for(
        r#"<?php
            /**
             * @param array $ids
             * @psalm-param list<int> $ids
             */
            function test(array $ids): string {
                return $ids;
            }
        "#,
    );
    assert!(
        d.iter()
            .any(|x| x.identifier == "return.type" && x.message.contains("list<int>")),
        "expected the @psalm-param type to be the one in effect, got: {d:?}"
    );
}

#[test]
fn a_single_argument_function_s_assert_if_true_narrows_its_argument() {
    let d = diagnostics_for(
        r#"<?php
use function PHPStan\Testing\assertType;
final class User {}
/** @phpstan-assert-if-true User $value */
function is_user(mixed $value): bool { return true; }
function f(mixed $b): void {
    if (is_user($b)) {
        assertType('User', $b);
    }
}
"#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "phpstanRs.assertType"),
        "{d:?}"
    );
}
