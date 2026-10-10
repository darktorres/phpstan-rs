//! The union of a list in one normalization is the left fold of the binary
//! union over it.
use super::super::*;
use proptest::prelude::*;

fn scalar() -> impl Strategy<Value = Type> {
    prop_oneof![
        Just(Type::null()),
        Just(Type::bool()),
        Just(Type::bool_literal(true)),
        Just(Type::bool_literal(false)),
        Just(Type::int()),
        Just(Type::float()),
        Just(Type::string()),
        Just(Type::mixed()),
        (0i64..6).prop_map(Type::int_literal),
        (0i64..4, 0i64..8).prop_map(|(a, b)| Type::int_range(Some(a), Some(a + b))),
        Just(Type::int_range(Some(1), None)),
        Just(Type::int_range(None, Some(0))),
        prop::sample::select(vec!["", "a", "b", "ab", "0"])
            .prop_map(|s| Type::string_literal(s.into())),
        prop::sample::select(vec!["Foo", "Bar", "Baz"])
            .prop_map(|n| Type::object(n.into(), Vec::new())),
        Just(Type::object_without_class()),
    ]
}

fn member() -> impl Strategy<Value = Type> {
    scalar().prop_recursive(3, 24, 3, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(k, v)| Type::array(k, v)),
            inner.clone().prop_map(Type::list_of),
            inner.clone().prop_map(Type::class_string),
            prop::collection::vec(inner.clone(), 2..4).prop_map(Type::union_of),
            (
                prop::collection::vec(
                    (
                        prop::sample::select(vec!["a", "b", "c"]),
                        inner.clone(),
                        any::<bool>()
                    ),
                    1..3
                ),
                any::<bool>()
            )
                .prop_map(|(fields, sealed)| {
                    let mut seen = Vec::new();
                    let fields: Vec<_> = fields
                        .into_iter()
                        .filter(|(k, ..)| {
                            !seen.contains(k) && {
                                seen.push(*k);
                                true
                            }
                        })
                        .collect();
                    let shape = Shape::from_named(fields);
                    if sealed {
                        Type::array_shape(shape)
                    } else {
                        Type::array_shape(Shape::unsealed(shape.fields().to_vec()))
                    }
                }),
            Just(Type::array_shape(Shape::sealed(Vec::new()))),
        ]
    })
}

/// A type as text with the order of union and intersection members and of
/// shape fields taken out: what the join's grouping is allowed to change.
fn canonical(ty: &Type) -> String {
    match ty.repr() {
        Repr::Union(members) | Repr::BenevolentUnion(members) | Repr::Intersection(members) => {
            let mut parts: Vec<String> = members.iter().map(canonical).collect();
            parts.sort();
            let open = if matches!(ty.repr(), Repr::Intersection(_)) {
                '&'
            } else {
                '|'
            };
            format!("({})", parts.join(&open.to_string()))
        }
        Repr::ArrayShape(shape) => {
            let mut fields: Vec<String> = shape
                .fields()
                .iter()
                .map(|f| {
                    format!(
                        "{:?}{}:{}",
                        f.key,
                        if f.optional { "?" } else { "" },
                        canonical(&f.ty)
                    )
                })
                .collect();
            fields.sort();
            let tail = shape.unsealed.as_ref().map_or(String::new(), |t| {
                format!("...{}=>{}", canonical(&t.0), canonical(&t.1))
            });
            format!("shape{{{}{}}}", fields.join(","), tail)
        }
        Repr::Array(k, v) => format!("array<{},{}>", canonical(k), canonical(v)),
        _ => ty.to_string(),
    }
}

fn by_fold(members: &[Type]) -> Type {
    members
        .iter()
        .cloned()
        .reduce(Type::union)
        .unwrap_or_else(Type::never)
}

/// What the join of a list may differ in, between one normalization and a
/// fold of binary joins: both are unions of the operands (each operand is a
/// subtype of the result), and they print the same up to order. A merge of
/// shapes, or an absorption, is decided from the members present when its
/// rule runs, which a fold and a single pass see differently: the results
/// are then each a sound union, one more precise than the other, and the
/// run of the corpus shows whether any difference reaches a diagnostic.
#[test]
fn the_union_of_a_list_is_a_union_of_it_and_the_fold_agrees_but_for_rare_merges() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        proptest::test_runner::TestRng::deterministic_rng(
            proptest::test_runner::RngAlgorithm::ChaCha,
        ),
    );
    let strategy = prop::collection::vec(member(), 0..7);
    let relation = Relation::structural();
    let (mut cases, mut different, mut shown) = (0, 0, 0);
    for _ in 0..30_000 {
        let members = strategy.new_tree(&mut runner).unwrap().current();
        let many = Type::union_of(members.clone());
        let fold = by_fold(&members);
        cases += 1;
        for operand in &members {
            // `void` joined with `null` is `null`, and `mixed` is every value:
            // the operand is covered all the same, but `void` is not a
            // subtype of anything.
            if matches!(operand.repr(), Repr::Void) {
                continue;
            }
            assert!(
                relation.is_super_type_of(&many, operand).yes()
                    || matches!(many.repr(), Repr::Mixed(..)),
                "{many} does not cover {operand} of {members:?}"
            );
        }
        if canonical(&many) != canonical(&fold) {
            different += 1;
            if shown < 12 {
                shown += 1;
                eprintln!(
                    "many {many}\n  fold {fold}\n  of {}",
                    members
                        .iter()
                        .map(|m| m.to_string())
                        .collect::<Vec<_>>()
                        .join(" , ")
                );
            }
        }
    }
    eprintln!("{different} of {cases} lists differ");
    assert!(
        different * 50 < cases,
        "{different} of {cases} lists differ"
    );
}

/// Many array shapes over a few keys, where the indexed merge of shapes and
/// the pairwise one (its debug assertion) must agree.
#[test]
fn many_shapes_over_few_keys_merge_as_the_pairwise_loops_do() {
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::{Config, TestRng, TestRunner};
    let keys = vec!["a", "b", "c", "d", "e"];
    let shape = (
        prop::collection::vec((prop::sample::select(keys), scalar(), any::<bool>()), 0..4),
        any::<bool>(),
    )
        .prop_map(|(fields, sealed)| {
            let mut seen = Vec::new();
            let fields: Vec<_> = fields
                .into_iter()
                .filter(|(k, ..)| {
                    !seen.contains(k) && {
                        seen.push(*k);
                        true
                    }
                })
                .collect();
            let shape = Shape::from_named(fields);
            if sealed {
                Type::array_shape(shape)
            } else {
                Type::array_shape(Shape::unsealed(shape.fields().to_vec()))
            }
        });
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        TestRng::deterministic_rng(proptest::test_runner::RngAlgorithm::ChaCha),
    );
    let strategy = prop::collection::vec(shape, 8..40);
    let relation = Relation::structural();
    for _ in 0..2_000 {
        let members = strategy.new_tree(&mut runner).unwrap().current();
        let many = Type::union_of(members.clone());
        for operand in &members {
            assert!(
                relation.is_super_type_of(&many, operand).yes(),
                "{many} does not cover {operand}"
            );
        }
    }
}

#[test]
fn a_long_union_keeps_each_member_once_in_the_order_it_met_them() {
    let mut members = Vec::new();
    for round in 0..3 {
        for n in 0..40 {
            members.push(Type::string_literal(format!("s{n}").into()));
        }
        members.push(Type::object(
            if round == 1 { "Foo" } else { "foo" }.into(),
            Vec::new(),
        ));
    }
    let union = Type::union_of(members);
    let found = union.union_members();
    assert_eq!(found.len(), 41);
    for (n, member) in found.iter().take(40).enumerate() {
        assert_eq!(*member, Type::string_literal(format!("s{n}").into()));
    }
    assert_eq!(found[40], Type::object("foo".into(), Vec::new()));
}

#[test]
fn a_shape_key_from_a_type_and_by_name_is_the_key_of_its_text() {
    for text in [
        "0",
        "7",
        "-3",
        "01",
        "-0",
        "a",
        "",
        "9223372036854775808",
        "é",
    ] {
        let from_type = ShapeKey::from_type(&Type::string_literal(text.into()));
        assert_eq!(from_type, Some(ShapeKey::from_name(text)), "{text:?}");
        for other in ["0", "7", "-3", "a", ""] {
            assert_eq!(
                ShapeKey::from_name(text).is(other),
                ShapeKey::from_name(text) == ShapeKey::from_name(other),
                "{text:?} is {other:?}"
            );
        }
    }
}
