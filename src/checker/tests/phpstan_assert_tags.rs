//! `@phpstan-assert`/`-if-true`/`-if-false` tags - real PHPStan's
//! custom-assertion-function convention. Real corpus shape: phpunit's
//! own accessor pair `hasTestFilesFile(): bool` (tagged `@phpstan-assert
//! -if-true !null $this->testFilesFile`) then `testFilesFile(): string`'s
//! own body guarding on it.

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

// Real bug found and fixed this session: the bare, unconditional
// `@phpstan-assert` lookup for an instance `MethodCall` required the
// receiver's own type to be literally `Type::Object`, silently skipping
// the narrowing for a `Union`/`Intersection` receiver - a value narrowed
// via `instanceof` from an unrelated interface, then called through.
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

// Real bug found and fixed this session: `AssertTarget::ThisProp`'s own
// "not already narrowed" fallback (`assert_target_key_and_current`,
// `checker/narrowing.rs`) matched the receiver's own type against
// `Type::Object` directly, silently falling to `Type::Mixed` (discarding
// the property's real declared type entirely) for a `Union`/
// `Intersection` receiver - reachable even through the *already*-correct
// `ThisSelf`/branch-conditional call site above, since this fallback
// re-infers the receiver's own type independently.
#[test]
fn assert_if_true_narrows_a_this_property_through_a_union_typed_receiver() {
    // Asserts the narrowed *type* directly (`assertType`), not merely the
    // absence of diagnostics - `Type::Mixed` (the bug's own fallback) is
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
    // Real corpus shape: Doctrine ORM's own `AssociationMapping::
    // isToOneOwningSide(): bool` (`@phpstan-assert-if-true
    // OneToOneOwningSideMapping|ManyToOneAssociationMapping $this`),
    // called on a variable typed as the union of every concrete mapping
    // subclass - every one of which inherits the identical method.
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
    // Real corpus shape: Doctrine ORM's own `@phpstan-assert-if-true
    // ToOneAssociationMapping $this` (an interface every concrete mapping
    // class implements only *transitively*, through an intermediate
    // abstract class, never directly) - narrowing a union against it
    // must keep every member that's *assignable* to the interface, not
    // just ones that equal it structurally, or every member's own
    // additional properties/methods are lost outright.
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
