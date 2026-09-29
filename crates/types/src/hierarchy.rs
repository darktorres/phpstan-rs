//! What a type relation needs to know about classes; `Registry`
//! implements it.

use rustc_hash::FxHashMap;
use syntax::ast::ClassKind;

use crate::{CallSiteVariance, CallableSig, Name, Repr, TrinaryLogic, Type, Variance};

/// A class as a type relation sees it.
pub struct ClassFacts<'a> {
    pub kind: ClassKind,
    pub is_final: bool,
    /// Its own `@template` names, in declaration order.
    pub templates: &'a [Name],
}

pub trait ClassHierarchy {
    /// Whether `class` is `of`, or extends or implements it.
    fn is_subclass_of(&self, class: &str, of: &str) -> bool;

    /// `name`'s facts, looked up as written and then case-insensitively.
    fn find_class(&self, name: &str) -> Option<ClassFacts<'_>>;

    /// Every ancestor's name: parents, interfaces and used traits.
    fn ancestor_names(&self, class: &str) -> Vec<Name>;

    fn has_method(&self, class: &str, method: &str) -> bool;

    /// Whether `class`'s method `method` is static.
    fn is_static_method(&self, class: &str, method: &str) -> bool;

    /// `class`'s method `method` as a callable: its parameters and
    /// return type.
    fn method_signature(&self, class: &str, method: &str) -> Option<CallableSig>;

    /// Whether a function named `name` exists (compared
    /// case-insensitively); `Maybe` when functions aren't known.
    fn function_exists(&self, name: &str) -> TrinaryLogic;

    /// The declared return type of `class`'s method `method`.
    fn method_return_type(&self, class: &str, method: &str) -> Option<Type>;

    /// The declared type of `class`'s property `property`.
    fn property_type(&self, class: &str, property: &str) -> Option<Type>;

    /// The variance `class` declares for its template slot `index`:
    /// `Invariant` when it declares none, `None` when `class` or the
    /// slot is unknown.
    fn slot_variance(&self, class: &str, index: usize) -> Option<Variance>;

    /// What `ancestor`'s templates are bound to in `class<args>`.
    fn ancestor_template_bindings(
        &self,
        class: &str,
        args: &[Type],
        ancestor: &str,
    ) -> FxHashMap<String, Type>;
}

/// The variance that governs one generic argument slot: a call-site
/// annotation written on `arg` wins; otherwise `class`'s declared
/// variance for the slot. `None` is `*` (never checked) or an unknown
/// slot.
pub(crate) fn effective_slot_variance(
    hierarchy: &dyn ClassHierarchy,
    class: &str,
    index: usize,
    arg: &Type,
) -> Option<Variance> {
    match arg.repr() {
        Repr::VarianceArg(CallSiteVariance::Covariant, _) => Some(Variance::Covariant),
        Repr::VarianceArg(CallSiteVariance::Contravariant, _) => Some(Variance::Contravariant),
        Repr::VarianceArg(CallSiteVariance::Bivariant, _) => None,
        _ => hierarchy.slot_variance(class, index),
    }
}

/// A hierarchy that knows no classes: a class is only itself. The
/// relations over it compare classes by name alone.
pub struct NoClasses;

impl ClassHierarchy for NoClasses {
    fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        class.eq_ignore_ascii_case(of)
    }

    fn find_class(&self, _: &str) -> Option<ClassFacts<'_>> {
        None
    }

    fn ancestor_names(&self, _: &str) -> Vec<Name> {
        Vec::new()
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

    fn slot_variance(&self, _: &str, _: usize) -> Option<Variance> {
        None
    }

    fn ancestor_template_bindings(&self, _: &str, _: &[Type], _: &str) -> FxHashMap<String, Type> {
        FxHashMap::default()
    }
}
