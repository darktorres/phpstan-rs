//! A scope that keeps the answers of the relations over a hierarchy
//! answers as the relation does without one, and keeps none of them for
//! another hierarchy.
use rustc_hash::FxHashMap;
use syntax::name::Name;

use super::comparison::MatrixClasses;
use crate::{
    CallableSig, ClassFacts, ClassHierarchy, Relation, TrinaryLogic, Type, Variance,
    relation_memo_scope,
};

/// `MatrixClasses`, with `Q` extending `P` when the flag is set. A
/// hierarchy takes up room, as a registry does, so that two of them are two
/// addresses.
struct MatrixWithQ(MatrixClasses, bool);

impl ClassHierarchy for MatrixWithQ {
    fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        (self.1 && class == "Q" && of == "P") || self.0.is_subclass_of(class, of)
    }
    fn find_class(&self, name: &str) -> Option<ClassFacts<'_>> {
        self.0.find_class(name)
    }
    fn ancestor_names(&self, class: &str) -> Vec<Name> {
        if self.1 && class == "Q" {
            vec![Name::from("P")]
        } else {
            Vec::new()
        }
    }
    fn has_method(&self, class: &str, method: &str) -> bool {
        self.0.has_method(class, method)
    }
    fn is_static_method(&self, class: &str, method: &str) -> bool {
        self.0.is_static_method(class, method)
    }
    fn method_signature(&self, class: &str, method: &str) -> Option<CallableSig> {
        self.0.method_signature(class, method)
    }
    fn function_exists(&self, name: &str) -> TrinaryLogic {
        self.0.function_exists(name)
    }
    fn method_return_type(&self, class: &str, method: &str) -> Option<Type> {
        self.0.method_return_type(class, method)
    }
    fn property_type(&self, class: &str, property: &str) -> Option<Type> {
        self.0.property_type(class, property)
    }
    fn slot_variance(&self, class: &str, index: usize) -> Option<Variance> {
        self.0.slot_variance(class, index)
    }
    fn ancestor_template_bindings(
        &self,
        class: &str,
        args: &[Type],
        ancestor: &str,
    ) -> FxHashMap<String, Type> {
        self.0.ancestor_template_bindings(class, args, ancestor)
    }
}

fn types() -> Vec<Type> {
    let object = |name: &str| Type::object(name.into(), Vec::new());
    vec![
        object("P"),
        object("Q"),
        object("S"),
        Type::enum_case("E".into(), "A".into()),
        Type::union_of(vec![object("P"), Type::null()]),
        Type::object_without_class(),
        Type::int(),
        Type::mixed(),
    ]
}

fn answers(relation: &Relation, types: &[Type]) -> Vec<TrinaryLogic> {
    types
        .iter()
        .flat_map(|sup| types.iter().map(|sub| relation.is_super_type_of(sup, sub)))
        .collect()
}

fn unscoped(hierarchy: &dyn ClassHierarchy, types: &[Type]) -> Vec<TrinaryLogic> {
    let relation = Relation {
        hierarchy,
        strict_mixed_arrays: false,
        configured_level: None,
        check_benevolent_union_types: false,
    };
    types
        .iter()
        .flat_map(|sup| {
            types
                .iter()
                .map(|sub| relation.is_super_type_of_by_form(sup, sub))
        })
        .collect()
}

#[test]
fn scoped_answers_are_the_relations_own_for_each_hierarchy() {
    let types = types();
    let plain = MatrixWithQ(MatrixClasses, false);
    let extends = MatrixWithQ(MatrixClasses, true);
    let relation = |h| Relation {
        hierarchy: h,
        strict_mixed_arrays: false,
        configured_level: None,
        check_benevolent_union_types: false,
    };
    let (plain_expected, extends_expected) = (unscoped(&plain, &types), unscoped(&extends, &types));
    assert_ne!(plain_expected, extends_expected);
    for _ in 0..2 {
        {
            let _scope = relation_memo_scope(&plain);
            assert_eq!(answers(&relation(&plain), &types), plain_expected);
            assert_eq!(answers(&relation(&plain), &types), plain_expected);
            // Another hierarchy asked inside the scope is not answered
            // from it.
            assert_eq!(answers(&relation(&extends), &types), extends_expected);
        }
        {
            let _scope = relation_memo_scope(&extends);
            assert_eq!(answers(&relation(&extends), &types), extends_expected);
            {
                let _inner = relation_memo_scope(&plain);
                assert_eq!(answers(&relation(&plain), &types), plain_expected);
            }
            assert_eq!(answers(&relation(&extends), &types), extends_expected);
        }
        assert_eq!(answers(&relation(&plain), &types), plain_expected);
    }
}
