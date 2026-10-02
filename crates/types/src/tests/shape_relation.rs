//! `shape_super_type_of`, which settles keys before comparing values and
//! stops at the first `No`, answers as the field-by-field comparison that
//! collects every field's answer first.
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
