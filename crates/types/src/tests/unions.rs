//! `Type::union`: which members merge and which stay apart.

use crate::{Repr, Shape, ShapeField, ShapeKey, Type};

fn shape(fields: &[(&str, Type, bool)]) -> Type {
    Type::array_shape(Shape::sealed(
        fields
            .iter()
            .map(|(k, t, o)| ShapeField::new(ShapeKey::from_name(k), t.clone(), *o))
            .collect(),
    ))
}

fn lit(s: &str) -> Type {
    Type::string_literal(s.into())
}

#[test]
fn a_tagged_union_of_shapes_stays_two_members() {
    let a = shape(&[("t", lit("a"), false), ("v", Type::int_literal(1), false)]);
    let b = shape(&[("t", lit("b"), false), ("v", lit("s"), false)]);
    assert_eq!(
        Type::union(a.clone(), b.clone()),
        Type::new(Repr::Union(vec![a, b]))
    );
}

#[test]
fn shapes_differing_only_in_their_tag_merge() {
    let a = shape(&[("t", lit("a"), false), ("v", Type::int(), false)]);
    let b = shape(&[("t", lit("b"), false), ("v", Type::int(), false)]);
    assert_eq!(
        Type::union(a, b),
        shape(&[
            ("t", Type::new(Repr::Union(vec![lit("a"), lit("b")])), false),
            ("v", Type::int(), false)
        ])
    );
}

#[test]
fn an_empty_array_stays_apart_from_a_shape_with_a_required_key() {
    let empty = shape(&[]);
    let one = Type::array_shape(Shape::sealed(vec![ShapeField::new(
        ShapeKey::Int(0),
        lit("x"),
        false,
    )]));
    assert_eq!(
        Type::union(empty.clone(), one.clone()),
        Type::new(Repr::Union(vec![empty, one]))
    );
}

#[test]
fn an_all_optional_shape_absorbs_the_empty_array() {
    let empty = shape(&[]);
    let optional = shape(&[("a", Type::int(), true)]);
    assert_eq!(Type::union(empty, optional.clone()), optional);
}

#[test]
fn a_general_array_absorbs_every_shape() {
    let general = Type::array(Type::new(Repr::String), Type::new(Repr::Int));
    let s = shape(&[("a", Type::int_literal(1), false)]);
    assert_eq!(
        Type::union(general, s),
        Type::array(Type::new(Repr::String), Type::new(Repr::Int))
    );
}

#[test]
fn never_true_and_false_fold_away() {
    assert_eq!(
        Type::union(Type::int_literal(1), Type::never()),
        Type::int_literal(1)
    );
    assert_eq!(
        Type::union(Type::bool_literal(true), Type::bool_literal(false)),
        Type::bool()
    );
    assert_eq!(
        Type::union(
            Type::new(Repr::Union(vec![Type::bool(), Type::int()])),
            Type::bool_literal(false)
        ),
        Type::new(Repr::Union(vec![Type::bool(), Type::int()]))
    );
}

#[test]
fn a_union_keeps_an_explicit_mixed_explicit() {
    assert!(matches!(
        (Type::union(Type::explicit_mixed(), Type::null())).repr(),
        Repr::Mixed(true, None)
    ));
    assert!(matches!(
        (Type::union(Type::mixed(), Type::null())).repr(),
        Repr::Mixed(false, None)
    ));
}

#[test]
fn a_shape_with_two_more_required_keys_stays_apart() {
    let key = |i: i64, t: Type| ShapeField::new(ShapeKey::Int(i), t, false);
    let bare = Type::array_shape(Shape::sealed(vec![key(0, Type::new(Repr::String))]));
    let with = Type::array_shape(Shape::sealed(vec![
        key(0, Type::new(Repr::String)),
        key(1, lit("ab")),
        key(2, lit("b")),
    ]));
    assert_eq!(
        Type::union(with.clone(), bare.clone()),
        Type::new(Repr::Union(vec![with, bare]))
    );
}

#[test]
fn an_error_absorbs_a_union() {
    assert_eq!(Type::union(Type::error(), Type::int()), Type::error());
    assert_eq!(
        Type::union(
            Type::new(Repr::Union(vec![Type::int(), Type::error()])),
            Type::string()
        ),
        Type::error()
    );
}

mod past_the_cap {
    use crate::{
        CallableSig, ClassFacts, ClassHierarchy, ClassKind, MAX_UNION_MEMBERS, Relation, Shape,
        ShapeField, ShapeKey, TrinaryLogic, Type,
    };
    use rustc_hash::FxHashMap;
    use syntax::name::Name;

    /// `C0`..`C69` implement `I`, the even ones `J` too, all use the trait
    /// `T`; `D0`..`D69` extend nothing; `Collection` has one template.
    struct Classes {
        templates: Vec<crate::TemplateParam>,
    }

    static CLASSES: std::sync::LazyLock<Classes> = std::sync::LazyLock::new(|| Classes {
        templates: vec![crate::TemplateParam::unbounded("V".to_string())],
    });

    impl ClassHierarchy for Classes {
        fn is_subclass_of(&self, class: &str, of: &str) -> bool {
            class.eq_ignore_ascii_case(of)
                || self.ancestor_names(class).iter().any(|a| a.as_ref() == of)
        }

        fn find_class(&self, name: &str) -> Option<ClassFacts<'_>> {
            let kind = match name {
                "I" | "J" => ClassKind::Interface,
                "T" => ClassKind::Trait,
                _ => ClassKind::Class,
            };
            Some(ClassFacts {
                kind,
                is_final: false,
                templates: if name == "Collection" {
                    &self.templates
                } else {
                    &[]
                },
            })
        }

        fn ancestor_names(&self, class: &str) -> Vec<Name> {
            let Some(n) = class.strip_prefix('C').and_then(|n| n.parse::<u32>().ok()) else {
                return Vec::new();
            };
            let mut names = vec![Name::from("I"), Name::from("T")];
            if n % 2 == 0 {
                names.push(Name::from("J"));
            }
            names
        }

        fn has_method(&self, _: &str, _: &str) -> bool {
            false
        }

        fn is_static_method(&self, _: &str, _: &str) -> bool {
            false
        }

        fn method_signature(&self, _: &str, _: &str) -> Option<CallableSig> {
            None
        }

        fn function_exists(&self, _: &str) -> TrinaryLogic {
            TrinaryLogic::Maybe
        }

        fn method_return_type(&self, _: &str, _: &str) -> Option<Type> {
            None
        }

        fn property_type(&self, _: &str, _: &str) -> Option<Type> {
            None
        }

        fn ancestor_template_bindings(
            &self,
            class: &str,
            args: &[Type],
            ancestor: &str,
        ) -> FxHashMap<String, Type> {
            let mut bindings = FxHashMap::default();
            if class == ancestor && class == "Collection" {
                bindings.insert(
                    "V".to_string(),
                    args.first().cloned().unwrap_or_else(Type::mixed),
                );
            }
            bindings
        }
    }

    fn relation() -> Relation<'static> {
        Relation {
            hierarchy: &*CLASSES,
            ..Relation::structural()
        }
    }

    fn objects(prefix: &str, count: usize, step: usize) -> Vec<Type> {
        (0..count)
            .map(|i| Type::object(format!("{prefix}{}", i * step).into(), Vec::new()))
            .collect()
    }

    #[test]
    fn classes_sharing_an_interface_fold_to_it() {
        let odd: Vec<Type> = (0..MAX_UNION_MEMBERS + 1)
            .map(|i| Type::object(format!("C{}", 2 * i + 1).into(), Vec::new()))
            .collect();
        assert_eq!(
            relation().union_of(odd),
            Type::object("I".into(), Vec::new())
        );
    }

    #[test]
    fn two_interfaces_in_common_give_their_intersection() {
        let even = objects("C", MAX_UNION_MEMBERS + 1, 2);
        assert_eq!(
            relation().union_of(even),
            Type::intersection_of(vec![
                Type::object("I".into(), Vec::new()),
                Type::object("J".into(), Vec::new()),
            ])
        );
    }

    #[test]
    fn no_common_ancestor_gives_object() {
        let unrelated = objects("D", MAX_UNION_MEMBERS + 1, 1);
        assert_eq!(relation().union_of(unrelated), Type::object_without_class());
    }

    #[test]
    fn without_the_hierarchy_objects_fold_to_object() {
        let odd = objects("C", MAX_UNION_MEMBERS + 1, 1);
        assert_eq!(Type::union_of(odd), Type::object_without_class());
    }

    #[test]
    fn instantiations_of_one_generic_class_join_their_arguments() {
        let mut members = vec![
            Type::object("Collection".into(), vec![Type::int()]),
            Type::object("Collection".into(), vec![Type::string()]),
        ];
        members.extend(objects("D", MAX_UNION_MEMBERS - 1, 1));
        let joined = relation().union_of(members);
        assert_eq!(joined.union_members().len(), MAX_UNION_MEMBERS);
        assert!(
            joined.union_members().contains(&Type::object(
                "Collection".into(),
                vec![Type::union(Type::int(), Type::string())]
            )),
            "got {joined}"
        );
    }

    #[test]
    fn literals_fold_to_their_hull_and_common_refinements() {
        let ints: Vec<Type> = (0..MAX_UNION_MEMBERS as i64 + 1)
            .map(|i| Type::int_literal(i * 2))
            .collect();
        assert_eq!(
            Type::union_of(ints),
            Type::int_range(Some(0), Some(2 * MAX_UNION_MEMBERS as i64))
        );
        let strings: Vec<Type> = (0..MAX_UNION_MEMBERS + 1)
            .map(|i| Type::string_literal(format!("k{i}").into()))
            .collect();
        assert_eq!(
            Type::union_of(strings).to_string(),
            "non-falsy-lowercase-literal-non-decimal-int-string"
        );
    }

    #[test]
    fn shapes_with_different_keys_fold_to_one_with_optional_fields() {
        let shapes: Vec<Type> = (0..MAX_UNION_MEMBERS + 1)
            .map(|i| {
                Type::array_shape(Shape::sealed(vec![ShapeField::new(
                    ShapeKey::from_name(&format!("k{i}")),
                    Type::int(),
                    false,
                )]))
            })
            .collect();
        let folded = Type::union_of(shapes);
        let shape = folded.as_array_shape().expect("one shape");
        assert_eq!(shape.fields().len(), MAX_UNION_MEMBERS + 1);
        assert!(shape.fields().iter().all(|f| f.optional));
    }
}
