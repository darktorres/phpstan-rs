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
