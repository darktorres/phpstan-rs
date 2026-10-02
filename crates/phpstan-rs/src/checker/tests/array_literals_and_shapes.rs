//! Array literal typing, shapes, list-ness, index assignment, destructuring.

use super::super::*;
use super::helpers::*;
use types::Shape;

// Array-shape generalization, tried before the hard
// MAX_UNION_MEMBERS collapse - more shapes than it that all share the *same*
// key set merge into one shape with unioned field types instead of
// collapsing to `Mixed`. Tested directly against `generalize_by_key_
// set` itself, not via sequential `Type::union` calls: `.reduce()`
// builds the union one pairwise call at a time, which triggers
// generalization repeatedly at different *partial* states (and can
// itself separately trip the per-field union's own `MAX_UNION_
// MEMBERS` cap when the field values are too varied) - a real
// property of how `Type::union` is actually invoked, but not what
// this test is trying to isolate, which is `generalize_by_key_set`'s
// own one-shot merge behavior over an already-complete member list.
#[test]
fn generalize_by_key_set_merges_same_key_set_shapes_instead_of_collapsing() {
    // Only 2 distinct field values (alternating), so the per-field
    // union stays well under its own cap and the result is `bool`, not
    // `Mixed` - proving the merge actually folds
    // real field types together, not just picks one arbitrarily.
    let members: Vec<Type> = (0..=types::MAX_UNION_MEMBERS)
        .map(|i| {
            let value = if i % 2 == 0 {
                Type::bool_literal(true)
            } else {
                Type::bool_literal(false)
            };
            Type::array_shape(Shape::from_named(vec![("x".to_string(), value, false)]))
        })
        .collect();
    let result = generalize_by_key_set(members);
    assert_eq!(
        result.len(),
        1,
        "expected every same-key-set shape to merge into one"
    );
    match result[0].as_array_shape() {
        Some(fields) => {
            let fields = fields.named_fields();
            assert_eq!(fields.len(), 1);
            assert_eq!(fields[0].0, "x");
            assert!(!fields[0].2, "the merged field must not be optional");
            assert_eq!(
                fields[0].1,
                Type::bool(),
                "expected the alternating true and false values to fold into bool"
            );
        }
        None => panic!("expected a single merged ArrayShape, got {:?}", result[0]),
    }
}

// A mix of same- and different-key-set shapes only merges the
// matching subset - the different-key-set members pass through
// untouched, exactly as many members as went in.
#[test]
fn generalize_by_key_set_only_merges_the_matching_subset() {
    let mut members: Vec<Type> = (0..13)
        .map(|i| {
            let value = if i % 2 == 0 {
                Type::bool_literal(true)
            } else {
                Type::bool_literal(false)
            };
            Type::array_shape(Shape::from_named(vec![("x".to_string(), value, false)]))
        })
        .collect();
    members.extend((0..5).map(|i| {
        Type::array_shape(Shape::from_named(vec![(
            format!("y{i}"),
            Type::int(),
            false,
        )]))
    }));
    let result = generalize_by_key_set(members);
    // The 13 same-key-set ("x") shapes merge into 1; the 5
    // different-key-set ("y0".."y4") shapes each stay distinct and
    // untouched.
    assert_eq!(result.len(), 6, "expected 1 merged + 5 untouched members");
    let untouched_y_shapes = result
        .iter()
        .filter(|m| {
            m.as_array_shape().is_some_and(|shape| {
                shape.fields().len() == 1 && shape.fields()[0].key.to_string().starts_with('y')
            })
        })
        .count();
    assert_eq!(
        untouched_y_shapes, 5,
        "expected all 5 y-shapes to pass through unmerged"
    );
}

#[test]
fn an_offset_write_or_unset_keeps_the_variables_native_type_true() {
    let dump = type_dump_for(
        "<?php\nfunction f(string $k): void {\n    $a = [];\n    $a[$k] = 1;\n    $b = ['x' => 1];\n    unset($b['x']);\n    $c = null;\n    $c['y'] = 2;\n}\n",
    );
    let at = |line: u32| {
        dump.iter()
            .find(|l| l.starts_with(&format!("{line}:")))
            .unwrap_or_else(|| panic!("no dump for line {line}: {dump:#?}"))
            .clone()
    };
    assert!(
        at(4).contains("a: non-empty-array<(int|string), 1> (native non-empty-array)"),
        "{}",
        at(4)
    );
    assert!(at(6).contains("(native array)"), "{}", at(6));
    assert!(
        at(8).contains("c: array{y: 2} (native non-empty-array)"),
        "{}",
        at(8)
    );
}
