//! `cannot_contain` rules a pair out only where the relation that knows no
//! classes would not say that the one holds the other.
use super::super::*;
use super::shape_relation::{Lcg, shape};
use crate::union::cannot_contain;

fn pool() -> Vec<Type> {
    let object = |name: &str| Type::object(name.into(), Vec::new());
    let generic = |name: &str| Type::object(name.into(), vec![Type::int()]);
    let class_string = |inner: Type| Type::new(Repr::ClassString(inner));
    let mut types = vec![
        Type::mixed(),
        Type::never(),
        Type::null(),
        Type::bool(),
        Type::int(),
        Type::int_literal(3),
        Type::int_range(Some(1), Some(5)),
        Type::float(),
        Type::string(),
        Type::string_literal("a".into()),
        Type::object_without_class(),
        object("Foo"),
        object("foo"),
        object("Bar"),
        object("Closure"),
        object("Traversable"),
        object("Iterator"),
        object("Stringable"),
        generic("Foo"),
        generic("Bar"),
        class_string(object("Foo")),
        class_string(object("Bar")),
        class_string(object("foo")),
        class_string(Type::object_without_class()),
        class_string(Type::mixed()),
        class_string(Type::template_param(crate::TemplateParam::unbounded(
            "T".to_string(),
        ))),
        Type::enum_case("Suit".into(), "Hearts".into()),
        Type::array(Type::int(), Type::string()),
        Type::list_of(Type::int()),
        Type::union_of(vec![Type::int(), Type::string()]),
        Type::callable(None),
        Type::template_param(crate::TemplateParam::unbounded("T".to_string())),
    ];
    let mut rng = Lcg(0xABCD_EF01_2345_6789);
    for _ in 0..40 {
        types.push(Type::new(Repr::ArrayShape(shape(&mut rng, 1).into())));
    }
    types
}

#[test]
fn a_pair_ruled_out_is_one_the_relation_does_not_hold() {
    let relation = Relation::structural();
    let types = pool();
    let mut ruled_out = 0;
    for container in &types {
        for member in &types {
            if cannot_contain(container, member) {
                ruled_out += 1;
                assert!(
                    !relation.is_super_type_of(container, member).yes(),
                    "{container} over {member}"
                );
            }
        }
    }
    assert!(ruled_out > 500, "{ruled_out}");
}
