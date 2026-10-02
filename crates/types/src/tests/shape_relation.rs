//! `shape_super_type_of` and `is_keys_superset`, which settle keys before
//! comparing values and stop at the first answer that rules the relation
//! out, answer as the comparisons that go field by field, in order, do.
use super::super::*;

/// Every field's answer collected, the weakest of them the result.
fn collected(relation: &Relation, sup: &Shape, sub: &Shape) -> TrinaryLogic {
    let mut results = Vec::new();
    for sup_field in sup.fields() {
        match sub.field(&sup_field.key) {
            Some(sub_field) => {
                let value = relation.is_super_type_of(&sup_field.ty, &sub_field.ty);
                results.push(if sub_field.optional && !sup_field.optional {
                    match value {
                        TrinaryLogic::No => TrinaryLogic::No,
                        _ => TrinaryLogic::Maybe,
                    }
                } else {
                    value
                });
            }
            None if sup_field.optional => {}
            None if sub.is_sealed() => results.push(TrinaryLogic::No),
            None => results.push(TrinaryLogic::Maybe),
        }
    }
    if sup.is_sealed() {
        for sub_field in sub.fields() {
            if sup.field(&sub_field.key).is_none() {
                results.push(if sub_field.optional {
                    TrinaryLogic::Maybe
                } else {
                    TrinaryLogic::No
                });
            }
        }
    }
    if results.contains(&TrinaryLogic::No) {
        TrinaryLogic::No
    } else if results.contains(&TrinaryLogic::Maybe) {
        TrinaryLogic::Maybe
    } else {
        TrinaryLogic::Yes
    }
}

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

fn value(rng: &mut Lcg, depth: u32) -> Type {
    match rng.below(if depth == 0 { 8 } else { 10 }) {
        0 => Type::int(),
        1 => Type::string(),
        2 => Type::int_literal(rng.below(3) as i64),
        3 => Type::mixed(),
        4 => Type::never(),
        5 => Type::null(),
        6 => Type::union(Type::int(), Type::null()),
        7 => Type::string_literal("a".into()),
        8 => Type::new(Repr::ArrayShape(shape(rng, depth - 1).into())),
        _ => Type::array(Type::int(), value(rng, depth - 1)),
    }
}

fn shape(rng: &mut Lcg, depth: u32) -> Shape {
    let mut fields: Vec<ShapeField> = Vec::new();
    for _ in 0..rng.below(5) {
        let key = ShapeKey::from_name(&format!("k{}", rng.below(5)));
        if fields.iter().all(|f| f.key != key) {
            let ty = value(rng, depth);
            fields.push(ShapeField::new(key, ty, rng.below(3) == 0));
        }
    }
    if rng.below(3) == 0 {
        Shape::unsealed(fields)
    } else {
        Shape::sealed(fields)
    }
}

#[test]
fn shapes_relate_as_the_collected_comparison_does() {
    let relation = Relation::structural();
    let mut rng = Lcg(0xC0FF_EE00_1234_5678);
    let (mut yes, mut maybe, mut no) = (0, 0, 0);
    for _ in 0..20_000 {
        let sup = shape(&mut rng, 2);
        let sub = shape(&mut rng, 2);
        let expected = collected(&relation, &sup, &sub);
        let got = relation.is_super_type_of(
            &Type::new(Repr::ArrayShape(sup.clone().into())),
            &Type::new(Repr::ArrayShape(sub.clone().into())),
        );
        match expected {
            TrinaryLogic::Yes => yes += 1,
            TrinaryLogic::Maybe => maybe += 1,
            TrinaryLogic::No => no += 1,
        }
        assert_eq!(got, expected, "{sup:?} over {sub:?}");
    }
    assert!(yes > 100 && maybe > 100 && no > 100, "{yes} {maybe} {no}");
}

/// `is_keys_superset`, comparing each key's values as it finds the key.
fn keys_superset_in_order(relation: &Relation, this: &Shape, other: &Shape) -> bool {
    fn legacy(relation: &Relation, this: &Shape, other: &Shape) -> bool {
        if this.fields().len() < other.fields().len() {
            return false;
        }
        if other.fields().is_empty() {
            return this.fields().is_empty();
        }
        let mut fail_on_different_value =
            this.fields().len() != other.fields().len() || this.fields().len() < 2;
        for other_field in other.fields() {
            let Some(this_field) = this.field(&other_field.key) else {
                return false;
            };
            if !relation
                .is_super_type_of(&other_field.ty, &this_field.ty)
                .no()
            {
                continue;
            }
            if fail_on_different_value {
                return false;
            }
            fail_on_different_value = true;
        }
        this.fields()
            .iter()
            .filter(|f| !f.optional && other.field(&f.key).is_none())
            .count()
            <= 1
    }
    if other.fields().is_empty() && other.is_sealed() {
        return this.fields().iter().all(|f| f.optional);
    }
    let tail_absorbs = |tail_owner: &Shape, fields: &Shape| {
        let Some(tail) = tail_owner.unsealed.as_deref() else {
            return false;
        };
        fields.fields().iter().filter(|f| !f.optional).all(|f| {
            !relation.is_super_type_of(&tail.0, &f.key.to_type()).no()
                && !relation.is_super_type_of(&tail.1, &f.ty).no()
        })
    };
    match (this.is_sealed(), other.is_sealed()) {
        (false, false) => {
            legacy(relation, this, other) || tail_absorbs(this, other) || tail_absorbs(other, this)
        }
        (false, true) => legacy(relation, this, other) || tail_absorbs(this, other),
        (true, false) => legacy(relation, this, other) || tail_absorbs(other, this),
        (true, true) => legacy(relation, this, other),
    }
}

#[test]
fn keys_superset_answers_as_the_in_order_comparison_does() {
    let relation = Relation::structural();
    let mut rng = Lcg(0x1357_9BDF_0246_8ACE);
    let (mut yes, mut no) = (0, 0);
    for _ in 0..30_000 {
        let this = shape(&mut rng, 1);
        let other = shape(&mut rng, 1);
        let expected = keys_superset_in_order(&relation, &this, &other);
        if expected {
            yes += 1;
        } else {
            no += 1;
        }
        assert_eq!(
            crate::union::is_keys_superset(&relation, &this, &other),
            expected,
            "{this:?} over {other:?}"
        );
    }
    assert!(yes > 200 && no > 200, "{yes} {no}");
}
