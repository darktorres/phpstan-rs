//! Real PHPStan's `InvalidAssignVarRule` - `nullsafe.assign` (a nullsafe
//! operator anywhere in an assignment's own left-hand side) and
//! `assign.invalidExpr` (a left-hand side that isn't a real PHP lvalue
//! at all).

use super::helpers::*;

#[test]
fn a_nullsafe_property_access_directly_on_the_left_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo { public ?Foo $bar = null; }
            function test(?Foo $f) {
                $f?->bar = new Foo();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "nullsafe.assign"),
        "got: {d:?}"
    );
}

#[test]
fn a_nullsafe_method_call_directly_on_the_left_is_flagged() {
    // Not a real lvalue either way (`method.nonObject`/parse-shape
    // aside) - real PHPStan's own rule still fires on it independently,
    // since `containsNullSafe` only asks about the nullsafe operator's
    // own position, not overall assignability.
    let d = diagnostics_for(
        r#"<?php
            class Foo { public ?Foo $bar = null; }
            function test(?Foo $f) {
                $f?->bar() = new Foo();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "nullsafe.assign"),
        "got: {d:?}"
    );
}

#[test]
fn a_nullsafe_operator_nested_deeper_in_the_left_hand_side_is_still_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo { public ?Foo $bar = null; public ?Foo $baz = null; }
            function test(?Foo $f) {
                $f?->bar->baz = new Foo();
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "nullsafe.assign"),
        "got: {d:?}"
    );
}

#[test]
fn a_nullsafe_operator_inside_a_destructuring_target_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo { public ?Foo $bar = null; }
            function test(?Foo $f, array $x) {
                [$f?->bar] = $x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "nullsafe.assign"),
        "got: {d:?}"
    );
}

#[test]
fn an_ordinary_non_nullsafe_property_assignment_is_never_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo { public ?Foo $bar = null; }
            function test(Foo $f) {
                $f->bar = new Foo();
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "nullsafe.assign"),
        "unexpected diagnostics: {d:?}"
    );
}

#[test]
fn a_compound_assignment_to_a_nullsafe_target_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            class Foo { public int $bar = 0; }
            function test(?Foo $f) {
                $f?->bar += 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "nullsafe.assign"),
        "got: {d:?}"
    );
}

#[test]
fn a_call_result_on_the_left_is_flagged_as_not_assignable() {
    let d = diagnostics_for(
        r#"<?php
            function foo(): int { return 1; }
            function test() {
                foo() = 1;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assign.invalidExpr"),
        "got: {d:?}"
    );
}

#[test]
fn a_literal_on_the_left_is_flagged_as_not_assignable() {
    let d = diagnostics_for(
        r#"<?php
            function test() {
                1 = 2;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assign.invalidExpr"),
        "got: {d:?}"
    );
}

#[test]
fn a_non_assignable_expression_nested_inside_a_destructuring_target_is_flagged() {
    let d = diagnostics_for(
        r#"<?php
            function foo(): int { return 1; }
            function test(array $x) {
                [foo()] = $x;
            }
        "#,
    );
    assert!(
        d.iter().any(|x| x.identifier == "assign.invalidExpr"),
        "got: {d:?}"
    );
}

#[test]
fn ordinary_assignable_targets_are_never_flagged_as_invalid() {
    let d = diagnostics_for(
        r#"<?php
            class Foo { public int $bar = 0; public static int $baz = 0; }
            function test(Foo $f, array $x, string $name) {
                $a = 1;
                $f->bar = 1;
                $x['k'] = 1;
                $x[] = 1;
                Foo::$baz = 1;
                $f->{$name} = 1;
                [$a, $f->bar] = $x;
            }
        "#,
    );
    assert!(
        !d.iter().any(|x| x.identifier == "assign.invalidExpr"),
        "unexpected diagnostics: {d:?}"
    );
}
