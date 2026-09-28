//! Array literal typing, shapes, list-ness, index assignment, destructuring.

use super::super::*;
use types::Shape;

// Array-shape generalization, tried before the hard
// MAX_UNION_MEMBERS collapse - 13 shapes that all share the *same*
// key set merge into one shape with unioned field types instead of
// collapsing to `Mixed`. Tested directly against `generalize_by_key_
// set` itself, not via sequential `union_types` calls: `.reduce()`
// builds the union one pairwise call at a time, which triggers
// generalization repeatedly at different *partial* states (and can
// itself separately trip the per-field union's own `MAX_UNION_
// MEMBERS` cap when the field values are too varied) - a real
// property of how `union_types` is actually invoked, but not what
// this test is trying to isolate, which is `generalize_by_key_set`'s
// own one-shot merge behavior over an already-complete member list.
#[test]
fn generalize_by_key_set_merges_same_key_set_shapes_instead_of_collapsing() {
    // Only 2 distinct field values (alternating), so the per-field
    // union stays well under its own cap and the result is `bool`, not
    // `Mixed` - proving the merge actually folds
    // real field types together, not just picks one arbitrarily.
    let members: Vec<Type> = (0..13)
        .map(|i| {
            let value = if i % 2 == 0 { Type::True } else { Type::False };
            Type::ArrayShape(Shape::from_named(vec![("x".to_string(), value, false)]))
        })
        .collect();
    let result = generalize_by_key_set(members);
    assert_eq!(
        result.len(),
        1,
        "expected every same-key-set shape to merge into one"
    );
    match &result[0] {
        Type::ArrayShape(fields) => {
            let fields = fields.named_fields();
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "x");
            assert!(!fields[0].2, "the merged field must not be optional");
            assert_eq!(
                fields[0].1,
                Type::Bool,
                "expected the alternating true and false values to fold into bool"
            );
        }
        other => panic!("expected a single merged ArrayShape, got {other:?}"),
    }
}

// A mix of same- and different-key-set shapes only merges the
// matching subset - the different-key-set members pass through
// untouched, exactly as many members as went in.
#[test]
fn generalize_by_key_set_only_merges_the_matching_subset() {
    let mut members: Vec<Type> = (0..13)
        .map(|i| {
            let value = if i % 2 == 0 { Type::True } else { Type::False };
            Type::ArrayShape(Shape::from_named(vec![("x".to_string(), value, false)]))
        })
        .collect();
    members.extend(
        (0..5).map(|i| {
            Type::ArrayShape(Shape::from_named(vec![(format!("y{i}"), Type::Int, false)]))
        }),
    );
    let result = generalize_by_key_set(members);
    // The 13 same-key-set ("x") shapes merge into 1; the 5
    // different-key-set ("y0".."y4") shapes each stay distinct and
    // untouched.
    assert_eq!(result.len(), 6, "expected 1 merged + 5 untouched members");
    let untouched_y_shapes = result
            .iter()
            .filter(|m| matches!(m, Type::ArrayShape(shape) if shape.fields.len() == 1 && shape.fields[0].key.to_string().starts_with('y')))
            .count();
    assert_eq!(
        untouched_y_shapes, 5,
        "expected all 5 y-shapes to pass through unmerged"
    );
}
