//! Type narrowing from conditionals, instanceof, is_*, isset, assert, etc.

use super::super::*;
use types::Shape;

// `str_contains`/`str_starts_with`/`str_ends_with` with a non-empty
// literal needle, mirroring real PHPStan's own
// `StrContainingTypeSpecifyingExtension`.

#[test]
fn array_key_exists_makes_an_optional_key_required_or_drops_it() {
    let shape = Type::array_shape(Shape::from_named(vec![(
        "a".to_string(),
        Type::int(),
        true,
    )]));
    assert_eq!(
        narrowing::narrow_array_shape_by_key_presence(&shape, "a", true),
        Type::array_shape(Shape::from_named(vec![(
            "a".to_string(),
            Type::int(),
            false
        )]))
    );
    assert_eq!(
        narrowing::narrow_array_shape_by_key_presence(&shape, "a", false),
        Type::array_shape(Shape::sealed(Vec::new()))
    );
}

#[test]
fn array_value_type_for_key_returns_none_for_a_missing_shape_key() {
    let shape = Type::array_shape(Shape::from_named(vec![(
        "a".to_string(),
        Type::int(),
        false,
    )]));
    assert_eq!(
        shape.offset_value_type(Some(&types::ShapeKey::from_name("b"))),
        None,
        "a shape genuinely missing the key has no value type to offer"
    );
}

// `??`/`??=`'s left side must not be checked for "undefined
// variable"/"undefined property" like an ordinary read - both are
// specifically designed to be warning-free for a variable/property
// that isn't set at all.

// A narrow slice of real PHPStan's own `ConditionalExpressionHolder` -
// correlating a second variable's narrowing to a first variable's own
// truthiness, for the single-hop case (`$ok = <condition>; if ($ok)
// {...}`), by storing the RHS condition expression and replaying it
// through the already-correct `apply_narrowing` recursion.
