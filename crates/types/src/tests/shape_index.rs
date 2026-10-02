//! `Shape`'s key index: a shape of many fields answers `field`, `set`,
//! `push` and `retain` as a plain list of its fields does.
use super::super::*;

/// The shape's fields as `(key text, type, optional)`.
fn listed(shape: &Shape) -> Vec<(String, Type, bool)> {
    shape
        .fields()
        .iter()
        .map(|f| (f.key.to_string(), f.ty.clone(), f.optional))
        .collect()
}

/// A repeatable sequence of keys over a small alphabet, so keys repeat.
fn keys(count: usize, alphabet: u64) -> Vec<ShapeKey> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let n = (state >> 33) % alphabet;
            if n.is_multiple_of(3) {
                ShapeKey::Int(n as i64)
            } else {
                ShapeKey::from_name(&format!("k{n}"))
            }
        })
        .collect()
}

/// What `set` does to a list of fields.
fn model_set(model: &mut Vec<ShapeField>, field: ShapeField) {
    match model.iter_mut().find(|f| f.key == field.key) {
        Some(existing) => *existing = field,
        None => model.push(field),
    }
}

#[test]
fn set_replaces_in_place_and_appends_new_keys_for_any_size() {
    for alphabet in [5u64, 15, 16, 17, 40, 300] {
        let mut shape = Shape::sealed(Vec::new());
        let mut model: Vec<ShapeField> = Vec::new();
        for (i, key) in keys(600, alphabet).into_iter().enumerate() {
            let ty = if i % 2 == 0 {
                Type::int_literal(i as i64)
            } else {
                Type::string()
            };
            let field = ShapeField::new(key, ty, i % 5 == 0);
            model_set(&mut model, field.clone());
            shape.set(field);
            assert_eq!(shape.fields(), model.as_slice(), "alphabet {alphabet}");
        }
        for key in keys(200, alphabet * 2) {
            assert_eq!(
                shape.field(&key),
                model.iter().find(|f| f.key == key),
                "{key}"
            );
        }
    }
}

#[test]
fn a_copy_does_not_carry_the_index_of_the_shape_it_was_made_from() {
    let mut shape = Shape::sealed(Vec::new());
    for n in 0..40 {
        shape.push(ShapeField::new(
            ShapeKey::from_name(&format!("k{n}")),
            Type::int(),
            false,
        ));
    }
    // Built by the lookup, then copied.
    assert!(shape.field(&ShapeKey::from_name("k39")).is_some());
    let mut copy = shape.clone();
    copy.push(ShapeField::new(
        ShapeKey::from_name("added"),
        Type::string(),
        true,
    ));
    copy.set(ShapeField::new(
        ShapeKey::from_name("k0"),
        Type::null(),
        false,
    ));
    assert_eq!(shape.fields().len(), 40);
    assert!(shape.field(&ShapeKey::from_name("added")).is_none());
    assert_eq!(
        shape
            .field(&ShapeKey::from_name("k0"))
            .map(|f| f.ty.clone()),
        Some(Type::int())
    );
    assert!(copy.field(&ShapeKey::from_name("added")).is_some());
    assert_eq!(
        copy.field(&ShapeKey::from_name("k0")).map(|f| f.ty.clone()),
        Some(Type::null())
    );
}

#[test]
fn retain_leaves_no_stale_position_behind() {
    let mut shape = Shape::sealed(Vec::new());
    for n in 0..50 {
        shape.push(ShapeField::new(
            ShapeKey::Int(n),
            Type::int_literal(n),
            n % 2 == 0,
        ));
    }
    assert!(shape.field(&ShapeKey::Int(49)).is_some());
    shape.retain(|f| !f.optional);
    assert_eq!(shape.fields().len(), 25);
    for n in 0..50 {
        assert_eq!(
            shape.field(&ShapeKey::Int(n)).is_some(),
            n % 2 == 1,
            "key {n}"
        );
    }
    // A key that was removed can be added again at the end.
    shape.set(ShapeField::new(ShapeKey::Int(0), Type::null(), false));
    assert_eq!(
        shape.fields().last().map(|f| f.key.clone()),
        Some(ShapeKey::Int(0))
    );
    assert!(shape.field(&ShapeKey::Int(0)).is_some());
}

#[test]
fn equality_and_the_listing_ignore_whether_the_index_was_made() {
    let build = || {
        let mut shape = Shape::sealed(Vec::new());
        for n in 0..30 {
            shape.push(ShapeField::new(ShapeKey::Int(n), Type::int(), false));
        }
        shape
    };
    let (a, b) = (build(), build());
    assert!(a.field(&ShapeKey::Int(29)).is_some());
    assert_eq!(a, b);
    assert_eq!(listed(&a), listed(&b));
}

#[test]
fn merge_field_folds_into_the_field_of_that_key() {
    let mut shape = Shape::sealed(Vec::new());
    for n in 0..20 {
        shape.push(ShapeField::new(ShapeKey::Int(n), Type::int(), false));
    }
    shape.merge_field(
        ShapeField::new(ShapeKey::Int(7), Type::string(), true),
        |g, f| {
            g.ty = Type::union(g.ty.clone(), f.ty);
            g.optional |= f.optional;
        },
    );
    shape.merge_field(
        ShapeField::new(ShapeKey::Int(99), Type::null(), false),
        |_, _| unreachable!("no field has this key"),
    );
    assert_eq!(shape.fields().len(), 21);
    let seven = shape.field(&ShapeKey::Int(7)).expect("it is there");
    assert!(seven.optional);
    assert_eq!(seven.ty, Type::union(Type::int(), Type::string()));
}

#[test]
fn the_remembered_key_and_value_types_follow_every_change_of_the_shape() {
    // A copy starts with nothing remembered, so it answers from the fields.
    fn agrees(shape: &Shape, step: &str) {
        let fresh = shape.clone();
        assert_eq!(shape.key_type(), fresh.key_type(), "key type after {step}");
        assert_eq!(
            shape.value_type(),
            fresh.value_type(),
            "value type after {step}"
        );
    }
    let mut shape = Shape::sealed(Vec::new());
    agrees(&shape, "nothing");
    assert_eq!(shape.value_type(), Type::never());
    for n in 0..24 {
        // Asked before each change, so there is something to go stale.
        let _ = (shape.key_type(), shape.value_type());
        shape.push(ShapeField::new(
            ShapeKey::from_name(&format!("k{n}")),
            Type::int_literal(n),
            false,
        ));
        agrees(&shape, "push");
    }
    let _ = (shape.key_type(), shape.value_type());
    shape.set(ShapeField::new(
        ShapeKey::from_name("k3"),
        Type::string(),
        true,
    ));
    agrees(&shape, "set");
    let _ = (shape.key_type(), shape.value_type());
    shape.update_fields(|_, ty, _| *ty = Type::union(ty.clone(), Type::null()));
    agrees(&shape, "update_fields");
    let _ = (shape.key_type(), shape.value_type());
    if let Some(ty) = shape.field_type_at_mut(0) {
        *ty = Type::float();
    }
    agrees(&shape, "field_type_at_mut");
    let _ = (shape.key_type(), shape.value_type());
    shape.retain(|f| f.key != ShapeKey::from_name("k5"));
    agrees(&shape, "retain");
    let _ = (shape.key_type(), shape.value_type());
    shape.merge_field(
        ShapeField::new(ShapeKey::from_name("k1"), Type::bool(), false),
        |g, f| g.ty = Type::union(g.ty.clone(), f.ty),
    );
    agrees(&shape, "merge_field");
    // The tail is a public field: changed in place under a remembered type.
    let _ = (shape.key_type(), shape.value_type());
    shape.unsealed = Some(Box::new((Type::string(), Type::bool())));
    agrees(&shape, "a tail set in place");
    assert!(
        shape
            .value_type()
            .union_members()
            .iter()
            .any(|m| *m == Type::bool())
    );
}
