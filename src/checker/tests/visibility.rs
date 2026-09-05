//! Phase 57: `public`/`private`/`protected` access-control enforcement
//! for property reads/writes, method calls, and static method calls.

use super::super::Diagnostic;
use super::helpers::*;

/// These fixtures are deliberately minimal (a bare private property/
/// method with no other legitimate use anywhere in the file) - real,
/// unrelated Phase 59 unused-symbol diagnostics (`property.onlyWritten`,
/// `method.unused`, ...) fire on them too, which isn't what these tests
/// are about. Filters down to the visibility-family identifiers this
/// file actually exercises.
fn visibility_diagnostics(src: &str) -> Vec<Diagnostic> {
    diagnostics_for(src)
        .into_iter()
        .filter(|d| {
            d.identifier.starts_with("property.private")
                || d.identifier.starts_with("property.protected")
                || d.identifier.starts_with("method.private")
                || d.identifier.starts_with("method.protected")
                || d.identifier.starts_with("staticMethod.private")
                || d.identifier.starts_with("staticMethod.protected")
                || d.identifier.starts_with("classConstant.private")
                || d.identifier.starts_with("classConstant.protected")
                || d.identifier
                    .starts_with("staticClassAccess.privateConstant")
        })
        .collect()
}

#[test]
fn reading_a_private_property_from_outside_its_class_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private int $x = 1;
            }
            function test(Foo $f): int {
                return $f->x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "property.private");
}

#[test]
fn reading_a_private_property_from_inside_its_own_class_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private int $x = 1;
                public function getX(): int {
                    return $this->x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reading_a_private_property_from_a_subclass_is_flagged() {
    // Real PHP: `private` is never inherited/visible to a subclass,
    // unlike `protected`.
    let d = visibility_diagnostics(
        r#"<?php
            class Base {
                private int $x = 1;
            }
            class Child extends Base {
                public function getX(): int {
                    return $this->x;
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "property.private");
}

#[test]
fn reading_a_protected_property_from_a_subclass_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Base {
                protected int $x = 1;
            }
            class Child extends Base {
                public function getX(): int {
                    return $this->x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reading_a_protected_property_from_an_unrelated_class_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                protected int $x = 1;
            }
            class Unrelated {
                public function test(Foo $f): int {
                    return $f->x;
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "property.protected");
}

#[test]
fn reading_a_public_property_from_anywhere_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                public int $x = 1;
            }
            function test(Foo $f): int {
                return $f->x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn writing_a_private_property_from_outside_its_class_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private int $x = 1;
            }
            function test(Foo $f): void {
                $f->x = 2;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "property.private");
}

#[test]
fn writing_a_private_property_from_inside_its_own_class_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private int $x = 1;
                public function setX(int $x): void {
                    $this->x = $x;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_a_private_method_from_outside_its_class_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private function secret(): int { return 1; }
            }
            function test(Foo $f): int {
                return $f->secret();
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "method.private");
}

#[test]
fn calling_a_private_method_from_inside_its_own_class_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private function secret(): int { return 1; }
                public function reveal(): int {
                    return $this->secret();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_a_protected_method_from_a_subclass_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Base {
                protected function helper(): int { return 1; }
            }
            class Child extends Base {
                public function reveal(): int {
                    return $this->helper();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_a_private_static_method_from_outside_its_class_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private static function secret(): int { return 1; }
            }
            function test(): int {
                return Foo::secret();
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "staticMethod.private");
}

#[test]
fn calling_a_private_static_method_from_inside_its_own_class_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private static function secret(): int { return 1; }
                public static function reveal(): int {
                    return self::secret();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn calling_a_protected_static_method_from_a_subclass_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Base {
                protected static function helper(): int { return 1; }
            }
            class Child extends Base {
                public static function reveal(): int {
                    return static::helper();
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A real corpus check (amphp/http-server's own `Http1Driver`/
// `Http2Driver`, siblings under a common `abstract class
// AbstractHttpDriver { abstract protected function write(); }`) found
// this shape wrongly flagged: `owner` (whichever class's own lookup
// resolved the *concrete* override, `Http2Driver` here) isn't
// necessarily where the member's own access-control contract actually
// originates - `current_class`'s own hierarchy independently having a
// same-named member (via the shared abstract ancestor) proves the
// contract is shared too.
#[test]
fn calling_a_protected_method_on_a_sibling_class_via_a_shared_abstract_ancestor_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            abstract class Base {
                abstract protected function write(): void;
            }
            class A extends Base {
                private ?B $b = null;
                protected function write(): void {
                    if ($this->b) {
                        $this->b->write();
                    }
                }
            }
            class B extends Base {
                protected function write(): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Pest's own `$this`-binding (`uses(X::class)` + `it(...)`) grants the
// same private/protected access real `Closure::bind`'s own third
// argument does - a real corpus check (Piwigo's own PHPUnit/Pest test
// suite calling an inherited protected `TestCase::expectErrorLog()`)
// found this wrongly flagged before this call site started passing a
// scope class through alongside `$this`'s own type.
#[test]
fn calling_a_protected_method_from_inside_a_pest_bound_closure_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function uses($class) {}
            function it($description, $callback) {}
            class RealTestBase {
                protected function helper(): int { return 1; }
            }
            uses(RealTestBase::class);
            it('does something', function (): int {
                return $this->helper();
            });
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// A promoted constructor property's own visibility is enforced the same
// way an ordinary declared property's is.
#[test]
fn reading_a_promoted_private_constructor_property_from_outside_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                public function __construct(private int $x) {}
            }
            function test(Foo $f): int {
                return $f->x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "property.private");
}

// Phase 57's own still-open item: a class constant's own `public`/
// `private`/`protected` modifier (PHP 7.1+) gets the identical
// enforcement a property/method already does.

#[test]
fn reading_a_private_constant_from_outside_its_class_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private const X = 1;
            }
            function test(): int {
                return Foo::X;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "classConstant.private");
}

#[test]
fn reading_a_private_constant_from_inside_its_own_class_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private const X = 1;
                public function getX(): int {
                    return self::X;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reading_a_private_constant_from_a_subclass_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Base {
                private const X = 1;
            }
            class Child extends Base {
                public function getX(): int {
                    return self::X;
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "classConstant.private");
}

#[test]
fn reading_a_protected_constant_from_a_subclass_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Base {
                protected const X = 1;
            }
            class Child extends Base {
                public function getX(): int {
                    return self::X;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn reading_a_protected_constant_from_an_unrelated_class_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                protected const X = 1;
            }
            function test(): int {
                return Foo::X;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "classConstant.protected");
}

#[test]
fn reading_a_public_constant_from_anywhere_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                public const X = 1;
                const Y = 2;
            }
            function test(): int {
                return Foo::X + Foo::Y;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_enum_case_fetch_is_never_flagged_for_visibility() {
    // Enum cases have no visibility syntax in real PHP at all.
    let d = visibility_diagnostics(
        r#"<?php
            enum Suit {
                case Hearts;
            }
            function test(): Suit {
                return Suit::Hearts;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 57 follow-up: `AccessPrivateConstantThroughStaticRule` -
// `static::` performs late static binding, which can resolve to a
// different class at runtime than the one lexically containing the
// code, so a `private` constant reached this way is unsafe even from
// inside its own declaring class - confirmed directly via `php -r`.

#[test]
fn a_private_constant_accessed_through_static_is_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private const X = 1;
                public static function get(): int {
                    return static::X;
                }
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "staticClassAccess.privateConstant");
}

#[test]
fn a_private_constant_accessed_through_static_in_a_final_class_is_never_flagged() {
    // A `final` class can never have a subclass at all, so `static::`
    // there can never actually differ from the literal declaring class -
    // matches real PHPStan's own identical exemption.
    let d = visibility_diagnostics(
        r#"<?php
            final class Foo {
                private const X = 1;
                public static function get(): int {
                    return static::X;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_private_constant_accessed_through_self_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private const X = 1;
                public static function get(): int {
                    return self::X;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_public_constant_accessed_through_static_is_never_flagged() {
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                public const X = 1;
                public static function get(): int {
                    return static::X;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_protected_constant_accessed_through_static_is_never_flagged() {
    // Only `private` is unsafe this way - a subclass overriding a
    // `protected` constant is still legally accessible from the
    // declaring class's own perspective either way.
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                protected const X = 1;
                public static function get(): int {
                    return static::X;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_private_constant_accessed_through_a_named_class_is_never_flagged_for_the_static_rule() {
    // Only the literal `static` keyword triggers this rule - `Foo::X`
    // (even from inside `Foo` itself) is never late-static-bound.
    let d = visibility_diagnostics(
        r#"<?php
            class Foo {
                private const X = 1;
                public static function get(): int {
                    return Foo::X;
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
