//! Tests for the error-reporting half of an inline `@var` annotation on a
//! plain assignment statement (`checker::stmt::check_inline_var_annotation_type`) -
//! real PHPStan's `VarTagChangedExpressionTypeRule`, scoped to the single
//! assignment-statement shape.
use super::helpers::*;

#[test]
fn a_var_tag_contradicted_by_an_obviously_wrong_literal_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var int $x */
                $x = "hello";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "varTag.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_bare_var_tag_with_no_name_is_also_checked() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var int */
                $x = "hello";
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "varTag.type"),
        "got: {d:?}"
    );
}

#[test]
fn a_var_tag_referencing_an_unknown_class_is_flagged_class_not_found() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var NoSuchClass $x */
                $x = null;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_var_tag_referencing_a_real_class_is_never_flagged_class_not_found() {
    let d = diagnostics_for(
        r#"<?php
            class Foo {}
            function f(): void {
                /** @var Foo $x */
                $x = new Foo();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "class.notFound"),
        "got: {d:?}"
    );
}

#[test]
fn a_var_tag_matching_the_assigned_value_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var int $x */
                $x = 5;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "varTag.type"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_var_tag_widening_the_assigned_value_is_never_flagged() {
    // int is a real subtype of int|string - a real, common, legitimate
    // widening annotation, not a mistake.
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var int|string $x */
                $x = 5;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "varTag.type"),
        "unexpected diagnostics: {d:?}"
    );
}

// Piwigo's own `Piwigo\Category\CategoryRepository::findAvailableList`:
// `/** @var list<array<string, mixed>> $rows */
// $rows = $qb->getQuery()->getArrayResult();` - Doctrine's own
// `getArrayResult()` returns a bare, imprecise `array`, and the whole
// point of writing an inline `@var` here is to *refine* that into a
// more specific shape. A `list<array<string, mixed>>` is unconditionally
// still an array - not a mismatch, even though the refined type isn't
// itself a "supertype" of the bare one in the naive one-directional
// sense.
#[test]
fn a_var_tag_refining_a_wider_native_return_type_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var array{a: int} $x */
                $x = returnsArray();
            }
            function returnsArray(): array { return ['a' => 1]; }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "varTag.type"),
        "unexpected diagnostics: {d:?}"
    );
}

// A `foreach` header's own inline `@var` annotation (Phase 161) - the
// named form overrides the loop variable's own ordinarily-inferred
// element type for the whole body, not just validated-and-discarded
// the way `check_inline_var_annotation_type`'s own assign-only scope
// leaves it for this statement kind.
#[test]
fn a_named_var_tag_on_a_foreach_header_overrides_the_loop_variable_s_type() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(array $items): void {
                /** @var string $x */
                foreach ($items as $x) {
                    assertType('string', $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_bare_var_tag_on_a_foreach_header_resolves_to_the_loop_variable() {
    let d = diagnostics_for(
        r#"<?php
            use function PHPStan\Testing\assertType;
            function f(array $items): void {
                /** @var string */
                foreach ($items as $x) {
                    assertType('string', $x);
                }
            }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_var_tag_on_a_non_assignment_statement_is_never_checked_for_type_mismatch() {
    // Scoped deliberately to the assign shape only - a `@var` on some
    // other statement shape (foreach, static, ...) isn't validated here,
    // a disclosed scope trim.
    let d = diagnostics_for(
        r#"<?php
            function f(): void {
                /** @var int $x */
                foreach ([1, 2] as $x) {
                    echo $x;
                }
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "varTag.type"),
        "unexpected diagnostics: {d:?}"
    );
}
