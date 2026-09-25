//! Type narrowing from conditionals, instanceof, is_*, isset, assert, etc.

use super::super::*;
use super::helpers::*;
use types::Shape;

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

// `str_contains`/`str_starts_with`/`str_ends_with` with a non-empty
// literal needle, mirroring real PHPStan's own
// `StrContainingTypeSpecifyingExtension`.

#[test]
fn array_key_exists_on_a_merely_optional_key_narrows_neither_branch() {
    let shape = Type::ArrayShape(Shape::from_named(vec![("a".to_string(), Type::Int, true)]));
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
fn array_value_type_for_key_returns_none_for_a_missing_shape_key() {
    let shape = Type::ArrayShape(Shape::from_named(vec![("a".to_string(), Type::Int, false)]));
    assert_eq!(
        narrowing::array_value_type_for_key(&shape, "b"),
        None,
        "a shape genuinely missing the key has no value type to offer"
    );
}

// `??`/`??=`'s left side must not be checked for "undefined
// variable"/"undefined property" like an ordinary read - both are
// specifically designed to be warning-free for a variable/property
// that isn't set at all.

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

// A narrow slice of real PHPStan's own `ConditionalExpressionHolder` -
// correlating a second variable's narrowing to a first variable's own
// truthiness, for the single-hop case (`$ok = <condition>; if ($ok)
// {...}`), by storing the RHS condition expression and replaying it
// through the already-correct `apply_narrowing` recursion.

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
