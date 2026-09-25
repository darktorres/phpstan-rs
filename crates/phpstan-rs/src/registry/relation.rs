//! The registry as the class hierarchy the type relations read, and its
//! shorthands for them.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::registry::Registry;
use crate::types::{ClassFacts, ClassHierarchy, Relation, TrinaryLogic, Type, Variance};

impl ClassHierarchy for Registry {
    fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        Registry::is_subclass_of(self, class, of)
    }

    fn find_class(&self, name: &str) -> Option<ClassFacts<'_>> {
        self.lookup_class(name).map(|(_, info)| ClassFacts {
            kind: info.kind,
            is_final: info.is_final,
            templates: &info.templates,
        })
    }

    fn class_exactly(&self, name: &str) -> Option<ClassFacts<'_>> {
        self.classes.get(name).map(|info| ClassFacts {
            kind: info.kind,
            is_final: info.is_final,
            templates: &info.templates,
        })
    }

    fn ancestor_names(&self, class: &str) -> Vec<Arc<str>> {
        self.all_ancestor_names(class).into_iter().collect()
    }

    fn has_method(&self, class: &str, method: &str) -> bool {
        self.lookup_method(class, method).is_some()
    }

    fn method_return_type(&self, class: &str, method: &str) -> Option<Type> {
        self.lookup_method(class, method)
            .map(|sig| sig.return_type.clone())
    }

    fn property_type(&self, class: &str, property: &str) -> Option<Type> {
        self.lookup_property(class, property).map(|p| p.ty.clone())
    }

    fn slot_variance(&self, class: &str, index: usize) -> Option<Variance> {
        Registry::slot_variance(self, class, index)
    }

    fn ancestor_template_bindings(
        &self,
        class: &str,
        args: &[Type],
        ancestor: &str,
    ) -> FxHashMap<String, Type> {
        self.class_template_bindings(class, args, ancestor)
    }
}

impl Registry {
    /// The type relations against this registry's classes and settings.
    pub fn relation(&self) -> Relation<'_> {
        Relation {
            hierarchy: self,
            strict_mixed_arrays: self.strict_mixed_arrays,
            configured_level: self.configured_level,
            check_benevolent_union_types: self.check_benevolent_union_types,
        }
    }

    pub fn is_assignable(&self, sub: &Type, sup: &Type) -> bool {
        self.relation().is_assignable(sub, sup)
    }

    pub fn accepts_at_configured_level(&self, sub: &Type, sup: &Type) -> bool {
        self.relation().accepts_at_configured_level(sub, sup)
    }

    pub fn strict_mixed_receiver_level(&self, ty: &Type) -> Option<u8> {
        self.relation().strict_mixed_receiver_level(ty)
    }

    pub fn is_super_type_of(&self, sup: &Type, sub: &Type) -> TrinaryLogic {
        self.relation().is_super_type_of(sup, sub)
    }

    pub fn absorb_union_subtypes(&self, ty: &Type) -> Type {
        self.relation().absorb_union_subtypes(ty)
    }

    pub(crate) fn identical_result(&self, left: &Type, right: &Type) -> Option<bool> {
        self.relation().identical_result(left, right)
    }
}
