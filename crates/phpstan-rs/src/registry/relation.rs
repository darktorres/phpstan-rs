//! The registry as the class hierarchy the type relations read, and its
//! shorthands for them.

use syntax::name::Name;

use rustc_hash::FxHashMap;

use crate::registry::Registry;
use types::{
    AcceptsResult, CallableParam, CallableSig, ClassFacts, ClassHierarchy, Relation, TrinaryLogic,
    Type, Variance,
};

/// Every class name is looked up as PHP resolves it, case-insensitively:
/// a name the relations carry keeps its spelling as written.
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

    fn ancestor_names(&self, class: &str) -> Vec<Name> {
        self.all_ancestor_names(self.canonical_class_name(class))
            .into_iter()
            .collect()
    }

    fn has_method(&self, class: &str, method: &str) -> bool {
        self.lookup_method(self.canonical_class_name(class), method)
            .is_some()
    }

    fn is_static_method(&self, class: &str, method: &str) -> bool {
        let class = self.canonical_class_name(class);
        self.lookup_method_with_owner(class, method)
            .and_then(|(owner, _)| self.classes.get(&owner))
            .is_some_and(|info| {
                info.static_methods()
                    .iter()
                    .any(|m| m.eq_ignore_ascii_case(method))
            })
    }

    fn method_signature(&self, class: &str, method: &str) -> Option<CallableSig> {
        let sig = self.lookup_method(self.canonical_class_name(class), method)?;
        Some(CallableSig {
            params: sig
                .params()
                .iter()
                .map(|p| CallableParam {
                    ty: p.ty().clone(),
                    variadic: p.variadic(),
                    optional: p.has_default(),
                    by_ref: p.by_ref(),
                })
                .collect(),
            return_type: Box::new(sig.return_type().clone()),
            templates: Vec::new(),
        })
    }

    fn function_exists(&self, name: &str) -> TrinaryLogic {
        TrinaryLogic::from_bool(
            self.lookup_function(name.trim_start_matches('\\'))
                .is_some(),
        )
    }

    fn method_return_type(&self, class: &str, method: &str) -> Option<Type> {
        self.lookup_method(self.canonical_class_name(class), method)
            .map(|sig| sig.return_type.clone())
    }

    fn property_type(&self, class: &str, property: &str) -> Option<Type> {
        self.lookup_property(self.canonical_class_name(class), property)
            .map(|p| p.ty.clone())
    }

    fn slot_variance(&self, class: &str, index: usize) -> Option<Variance> {
        Registry::slot_variance(self, self.canonical_class_name(class), index)
    }

    fn ancestor_template_bindings(
        &self,
        class: &str,
        args: &[Type],
        ancestor: &str,
    ) -> FxHashMap<String, Type> {
        self.class_template_bindings(
            self.canonical_class_name(class),
            args,
            self.canonical_class_name(ancestor),
        )
    }
}

impl Registry {
    /// `name` in its declared spelling when the class is known, as
    /// written otherwise.
    pub fn canonical_class_name<'a>(&'a self, name: &'a str) -> &'a str {
        self.lookup_class(name)
            .map_or(name, |(declared, _)| declared)
    }

    /// The type relations against this registry's classes, at their
    /// default leniency; the checker's `relation` applies the configured
    /// strictness.
    pub fn relation(&self) -> Relation<'_> {
        Relation {
            hierarchy: self,
            strict_mixed_arrays: false,
            check_benevolent_union_types: false,
        }
    }

    pub fn accepts(&self, accepting: &Type, accepted: &Type) -> AcceptsResult {
        self.relation().accepts(accepting, accepted)
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
