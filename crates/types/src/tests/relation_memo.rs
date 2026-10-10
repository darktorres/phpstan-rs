//! The relation memos keep an answer under the generation of the hierarchy
//! that gave it.

use rustc_hash::FxHashMap;
use syntax::name::Name;

use crate::{
    CallableSig, ClassFacts, ClassHierarchy, ClassKind, Relation, TrinaryLogic, Type, Variance,
};

/// Classes `P` and `S`; `S` extends `P` only when `extends`, and its
/// `__toString` returns `returns`.
struct Hierarchy {
    generation: u64,
    extends: bool,
    returns: Type,
}

impl ClassHierarchy for Hierarchy {
    fn generation(&self) -> Option<u64> {
        Some(self.generation)
    }

    fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        class.eq_ignore_ascii_case(of) || (self.extends && class == "S" && of == "P")
    }

    fn find_class(&self, name: &str) -> Option<ClassFacts<'_>> {
        matches!(name, "P" | "S").then_some(ClassFacts {
            kind: ClassKind::Class,
            is_final: false,
            templates: &[],
        })
    }

    fn ancestor_names(&self, class: &str) -> Vec<Name> {
        if self.extends && class == "S" {
            vec![Name::from("P")]
        } else {
            Vec::new()
        }
    }

    fn has_method(&self, class: &str, method: &str) -> bool {
        class == "S" && method.eq_ignore_ascii_case("__toString")
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

    fn method_return_type(&self, class: &str, method: &str) -> Option<Type> {
        (class == "S" && method.eq_ignore_ascii_case("__toString")).then(|| self.returns.clone())
    }

    fn property_type(&self, _: &str, _: &str) -> Option<Type> {
        None
    }

    fn slot_variance(&self, _: &str, _: usize) -> Option<Variance> {
        None
    }

    fn ancestor_template_bindings(&self, _: &str, _: &[Type], _: &str) -> FxHashMap<String, Type> {
        FxHashMap::default()
    }
}

fn hierarchies() -> (Hierarchy, Hierarchy) {
    (
        Hierarchy {
            generation: 9_000_001,
            extends: true,
            returns: Type::string_literal("a".into()),
        },
        Hierarchy {
            generation: 9_000_002,
            extends: false,
            returns: Type::string_literal("b".into()),
        },
    )
}

#[test]
fn a_rejection_is_the_one_of_the_generation_asked() {
    let (extending, separate) = hierarchies();
    let p = Type::object("P".into(), Vec::new());
    let s = Type::object("S".into(), Vec::new());
    let relation = |hierarchy| Relation {
        hierarchy,
        strict_mixed_arrays: false,
        check_benevolent_union_types: false,
    };
    for _ in 0..3 {
        assert_eq!(relation(&extending).rejected_from(&p, &s), None);
        assert!(relation(&separate).rejected_from(&p, &s).is_some());
    }
}

#[test]
fn a_rejection_depends_on_the_relation_settings() {
    let (hierarchy, _) = hierarchies();
    let loose = Relation {
        hierarchy: &hierarchy,
        strict_mixed_arrays: false,
        check_benevolent_union_types: false,
    };
    let strict = Relation {
        hierarchy: &hierarchy,
        strict_mixed_arrays: true,
        check_benevolent_union_types: false,
    };
    let accepting = Type::array(Type::int(), Type::string());
    let accepted = Type::array(Type::int(), Type::mixed());
    let asked = |relation: &Relation| {
        let first = relation.rejected_from(&accepting, &accepted);
        assert_eq!(relation.rejected_from(&accepting, &accepted), first);
        first
    };
    let (loose_answer, strict_answer) = (asked(&loose), asked(&strict));
    assert_ne!(loose_answer, strict_answer);
}

#[test]
fn a_string_conversion_is_the_one_of_the_generation_asked() {
    let (a, b) = hierarchies();
    let s = Type::object("S".into(), Vec::new());
    for _ in 0..3 {
        assert_eq!(s.to_string_type(&a), Some(Type::string_literal("a".into())));
        assert_eq!(s.to_string_type(&b), Some(Type::string_literal("b".into())));
    }
}
