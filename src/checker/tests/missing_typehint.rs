//! Phase 58: `MissingTypehintCheck`-equivalent completeness battery (real
//! PHPStan's level-6 `missingType.*` rule family) - implicit-mixed
//! parameters/properties/returns, a bare `array`/`iterable` with no value
//! type, and a bare reference to a class with its own `@template`s given
//! no type arguments. See `checker/missing_typehint.rs`'s own module doc
//! comment for what's deliberately out of scope (closures/arrow-fns, and
//! a signature-less-`callable` check).

use super::helpers::*;

#[test]
fn untyped_property_is_flagged_as_missing_a_type() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public $x;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "missingType.property");
    assert!(d[0].message.contains("C::$x"), "got: {}", d[0].message);
}

#[test]
fn explicit_mixed_property_is_not_flagged() {
    // The false-positive guard `is_explicit_mixed_text` exists for:
    // `mixed` is a real, valid level-6 type declaration, not a stand-in
    // for "no type written at all" - this project's own `Type` model
    // can't tell the two apart from the resolved type alone (see that
    // function's own doc comment), so it must be recovered from the
    // property's own source text instead.
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public mixed $x;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bare_array_property_is_flagged_for_its_missing_value_type() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public array $items;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "missingType.iterableValue");
    assert!(d[0].tip.is_some(), "expected a tip: {:?}", d[0]);
    assert!(
        d[0].message.contains("C::$items") && d[0].message.contains("array"),
        "got: {}",
        d[0].message
    );
}

#[test]
fn precisely_typed_array_property_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                /** @var array<int> */
                public array $items;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_property_with_an_explicit_mixed_value_type_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                /** @var array<string, mixed> */
                public array $items;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_parameter_with_an_explicit_mixed_value_type_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                /**
                 * @param array<string, mixed> $options
                 */
                public function f(array $options): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn array_return_type_with_an_explicit_mixed_value_type_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                /**
                 * @return array<string, mixed>
                 */
                public function f(): array { return []; }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 88: an inherited `array<string, mixed>` (no own docblock on the
// implementor at all, relying entirely on the interface's) must be
// recognized as deliberately, explicitly `mixed` the same way an own
// docblock already is - real corpus false positive: every
// `UrlServiceInterface`/`ExposesPageData`/... implementor and test
// double in Piwigo that correctly omits its own docblock.
#[test]
fn an_inherited_explicit_mixed_array_value_type_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            interface I {
                /**
                 * @param array<string, mixed> $options
                 */
                public function f(array $options): void;
            }
            class C implements I {
                public function f(array $options): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

// Phase 88: an ancestor's own genuinely, deliberately untyped `mixed`
// parameter (no native hint at all - PSR-3's real `LoggerInterface::
// log(mixed $level, ...)` shape) must still propagate its "explicit"
// provenance onto an equally-untyped override - nothing about
// backfilling a bare `Mixed` here can lose information, since the
// child's own native resolution was already `Mixed` too.
#[test]
fn an_inherited_explicit_mixed_untyped_parameter_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            interface LoggerLike {
                /**
                 * @param mixed $level
                 */
                public function log($level, string $message): void;
            }
            class SpyLogger implements LoggerLike {
                public function log($level, string $message): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn list_with_an_explicit_mixed_value_type_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                /** @var list<mixed> */
                public array $items;
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_union_member_with_no_explicit_mixed_still_flags_normally() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                /** @var array<string, int>|array $items */
                public $items;
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "missingType.iterableValue");
}

#[test]
fn class_constant_with_a_bare_array_doc_type_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                /** @var array */
                public const X = [1, 2, 3];
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "missingType.iterableValue");
    assert!(d[0].message.contains("C::X"), "got: {}", d[0].message);
}

#[test]
fn class_constant_with_no_doc_type_at_all_is_never_flagged() {
    // Matches real PHPStan's own `MissingClassConstantTypehintRule`
    // exactly: unlike property/param/return, a constant with no `@var`
    // skips this whole check outright, regardless of what its own value
    // expression happens to be.
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public const Y = [1, 2, 3];
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn untyped_method_parameter_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public function foo($x): void {}
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "missingType.parameter");
    assert!(
        d[0].message.contains("C::foo()") && d[0].message.contains("$x"),
        "got: {}",
        d[0].message
    );
}

#[test]
fn untyped_method_return_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public function foo(int $x) {}
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "missingType.return");
    assert!(d[0].message.contains("C::foo()"), "got: {}", d[0].message);
}

#[test]
fn fully_typed_method_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public function foo(int $x): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn untyped_function_parameter_and_return_are_both_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function f($x) { return $x; }
        "#,
    );
    assert_eq!(d.len(), 2, "got: {d:?}");
    let identifiers: Vec<&str> = d.iter().map(|x| x.identifier.as_str()).collect();
    assert!(identifiers.contains(&"missingType.parameter"), "got: {d:?}");
    assert!(identifiers.contains(&"missingType.return"), "got: {d:?}");
    assert!(
        d.iter().all(|x| x.message.contains("Function f()")),
        "got: {d:?}"
    );
}

#[test]
fn explicit_mixed_function_signature_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            function f(mixed $x): mixed { return $x; }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bare_reference_to_a_generic_class_is_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Box {}
            class C {
                public function foo(Box $b): void {}
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "missingType.generics");
    assert!(
        d[0].message.contains("Box") && d[0].message.contains('T'),
        "got: {}",
        d[0].message
    );
}

#[test]
fn generic_class_reference_with_type_arguments_is_not_flagged() {
    let d = all_diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Box {}
            class C {
                /**
                 * @param Box<int> $b
                 */
                public function foo(Box $b): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bare_iterator_parameter_is_not_flagged_as_a_non_generic_generic_class() {
    // `Iterator` (like `Traversable`/`IteratorAggregate`/`Generator`) is
    // real PHP's own generic-over-key/value built-in interface - real
    // PHPStan's own default config (`checkGenericIterableClasses: false`)
    // deliberately doesn't nag on a bare reference to one, since that's
    // an extremely common, otherwise-unremarkable real-code shape (e.g.
    // `function each(Iterator $it)`).
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public function foo(Iterator $it): void {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn promoted_constructor_property_is_flagged_once_as_a_parameter_not_a_property() {
    // Real PHPStan's own `MissingPropertyTypehintRule` explicitly skips a
    // promoted property (`if ($propertyReflection->isPromoted()) return
    // [];`) since its own missing typehint is already the constructor
    // parameter's - flagging it a second time, as a property too, would
    // double-report the exact same missing type under two different
    // messages.
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public function __construct(public $id) {}
            }
        "#,
    );
    assert_eq!(d.len(), 1, "got: {d:?}");
    assert_eq!(d[0].identifier, "missingType.parameter");
    assert!(d[0].message.contains("$id"), "got: {}", d[0].message);
}

#[test]
fn constructor_with_no_return_type_is_not_flagged() {
    // Real PHPStan's own reflection layer gives a hint-less `__construct`
    // an implicit `void` return type, not `mixed` - matching that means
    // the extremely common, entirely idiomatic
    // `public function __construct(int $id) {}` (no `: void`) must never
    // be flagged, even though this project's own `FunctionSig` doesn't
    // special-case `__construct`'s return type the way real PHPStan's
    // reflection does.
    let d = all_diagnostics_for(
        r#"<?php
            class C {
                public function __construct(int $id) {}
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn fully_typed_class_produces_no_missing_typehint_diagnostics() {
    let d = all_diagnostics_for(
        r#"<?php
            /**
             * @template T
             */
            class Box {
                /** @var T */
                public $value;
            }
            class C {
                /** @var array<int> */
                public const LIMITS = [1, 2, 3];

                private int $count = 0;

                /**
                 * @param Box<int> $b
                 * @return array<int>
                 */
                public function foo(Box $b): array {
                    return [$this->count];
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
