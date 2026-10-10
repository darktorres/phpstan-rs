use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::sync::Arc;
use syntax::name::NameSet;
use syntax::name::{FoldKey, FoldedMap, Name, NameMap};
use syntax::stats::{self, Counter};

use rayon::prelude::*;

use crate::registry::effects::KeyName;
use crate::registry::name_table::TableKey;
use crate::registry::{
    ClassId, ClassReflection, ConstantReflection, FunctionReflection, MethodEntry,
    PropertyReflection,
};
use crate::registry::{Lazy, NameTable, Registry};
use syntax::ast::ClassKind;
use types::Type;

/// Method entries added by finishing passes. Declaration entries remain on
/// `ClassReflection`; these host-local entries extend its slot space.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MemberTopology {
    pub(crate) entries: Vec<MethodEntry>,
    pub(crate) slots: FoldedMap<u16>,
}

impl MemberTopology {
    pub(crate) fn add(&mut self, declaration_len: usize, entry: MethodEntry) -> Option<u16> {
        if self.slots.contains_key(&entry.name) {
            return None;
        }
        let slot = u16::try_from(declaration_len + self.entries.len())
            .expect("class method slot space exhausted");
        self.slots.insert(entry.name.clone(), slot);
        self.entries.push(entry);
        Some(slot)
    }
}

/// The resolved extends/implements closure for one declaration. Registered
/// ancestors use stable declaration IDs; unresolved direct names are retained
/// so PHP's name-based `is_subclass_of` behavior survives missing symbols.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AncestorIds {
    ids: FxHashSet<ClassId>,
    unresolved: NameSet,
}

impl AncestorIds {
    pub(crate) fn from_names(registry: &Registry, names: &NameSet) -> Self {
        let mut closure = Self::default();
        for name in names {
            if let Some(id) = registry.class_ids.get(name) {
                closure.ids.insert(*id);
            } else {
                closure.unresolved.insert(name.clone());
            }
        }
        closure
    }

    pub(crate) fn names(&self, registry: &Registry) -> NameSet {
        let mut names = self.unresolved.clone();
        for id in &self.ids {
            if let Some(name) = registry.class_name_by_id(*id) {
                names.insert(name.clone());
            }
        }
        names
    }

    pub(crate) fn with_generation(mut self, generation: u32) -> Self {
        self.ids = self
            .ids
            .into_iter()
            .map(|id| id.with_generation(generation))
            .collect();
        self
    }
}

/// What owns the members of one class, by kind: for each method, property
/// and constant name, the class (or ancestor) that declares it, found
/// walking `extends` and `implements` from the class itself. A map is
/// decoded when first read, so a lookup of methods does not decode the
/// other two.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ClassOwners {
    // Method names ignore case, as PHP's do: one entry for each, by the
    // spelling of the most derived class that declares it.
    pub(crate) methods: Lazy<FoldedMap<Name>>,
    pub(crate) properties: Lazy<NameMap<Name>>,
    pub(crate) consts: Lazy<NameMap<Name>>,
}

impl PartialEq for ClassOwners {
    fn eq(&self, other: &ClassOwners) -> bool {
        self.methods.get() == other.methods.get()
            && self.properties.get() == other.properties.get()
            && self.consts.get() == other.consts.get()
    }
}

impl Registry {
    /// Ensure a class has stable exact-case property slots. Declared
    /// properties are installed first; synthesized results append after them.
    pub(crate) fn ensure_property_topology(&mut self, class: super::ClassId) -> bool {
        let Some(declaration) = self.class_by_id(class) else {
            return false;
        };
        let mut declared: Vec<Name> = declaration.properties().keys().cloned().collect();
        declared.sort_unstable();
        let topology = self.class_property_topology.entry(class).or_default();
        for name in declared {
            if topology.slot(&name).is_none() {
                topology.append(name);
            }
        }
        true
    }

    pub(crate) fn property_ref_by_name(
        &self,
        class: super::ClassId,
        property: &str,
    ) -> Option<super::PropertyRef> {
        self.debug_check_id_generation(class.generation());
        let slot = self.class_property_topology.get(&class)?.slot(property)?;
        Some(super::PropertyRef::new(class, slot))
    }

    pub(crate) fn property_name_by_ref(&self, property: super::PropertyRef) -> Option<&Name> {
        self.debug_check_id_generation(property.class().generation());
        self.class_property_topology
            .get(&property.class())?
            .name(property.slot())
    }

    pub(crate) fn property_result_by_name(
        &self,
        class: super::ClassId,
        property: &str,
    ) -> Option<&PropertyReflection> {
        self.property_result_shared_by_name(class, property)
            .map(Arc::as_ref)
    }

    pub(crate) fn property_result_shared_by_name(
        &self,
        class: super::ClassId,
        property: &str,
    ) -> Option<&Arc<PropertyReflection>> {
        self.deferred_class_result(class)
            .and_then(|results| {
                results
                    .properties
                    .iter()
                    .find(|(name, _)| name.as_str() == property)
                    .map(|(_, property)| property)
            })
            .or_else(|| self.property_result_raw_shared_by_name(class, property))
    }

    pub(crate) fn property_result_raw_shared_by_name(
        &self,
        class: super::ClassId,
        property: &str,
    ) -> Option<&Arc<PropertyReflection>> {
        let slot = self.property_ref_by_name(class, property)?;
        self.class_property_results.get(&slot)
    }

    pub(crate) fn property_results_for_class(
        &self,
        class: super::ClassId,
    ) -> impl Iterator<Item = (super::PropertyRef, &Arc<PropertyReflection>)> {
        let resolved = self
            .deferred_class_result(class)
            .into_iter()
            .flat_map(move |results| {
                results
                    .properties
                    .iter()
                    .filter_map(move |(name, property)| {
                        self.property_ref_by_name(class, name)
                            .map(|reference| (reference, property))
                    })
            });
        resolved.chain(self.class_property_results.of_class(class))
    }

    pub(crate) fn raw_property_results_for_class(
        &self,
        class: super::ClassId,
    ) -> impl Iterator<Item = (super::PropertyRef, &Arc<PropertyReflection>)> {
        self.class_property_results.of_class(class)
    }

    /// Drops the memoized deferred result of `class`: it was resolved from
    /// the overlay as it stood, and a write to that overlay makes it stale.
    pub(crate) fn invalidate_deferred_class_result(&mut self, class: super::ClassId) {
        if let Some(cell) = self.deferred_class_results.get_mut(class) {
            *cell = std::sync::OnceLock::new();
        }
    }

    pub(crate) fn set_property_result(
        &mut self,
        class: super::ClassId,
        name: Name,
        property: Arc<PropertyReflection>,
    ) {
        self.invalidate_deferred_class_result(class);
        assert!(
            self.ensure_property_topology(class),
            "property result class is active"
        );
        let topology = self.class_property_topology.get_mut(&class).unwrap();
        let slot = topology
            .slot(&name)
            .or_else(|| topology.append(name.clone()))
            .expect("property slot exists after append");
        self.class_property_results
            .insert(super::PropertyRef::new(class, slot), property);
    }

    /// All declared and pass-added member entries in host-local slot order.
    pub(crate) fn member_entries(&self, class: &str) -> impl Iterator<Item = &MethodEntry> {
        self.class(class)
            .map(|(_, _, info)| info)
            .into_iter()
            .flat_map(|info| info.member_order.iter())
            .chain(
                self.class_ids
                    .get(class)
                    .and_then(|id| self.class_member_topology.get(id))
                    .into_iter()
                    .flat_map(|topology| topology.entries.iter()),
            )
    }

    pub(crate) fn member_slot<K: TableKey + ?Sized, M: FoldKey + ?Sized>(
        &self,
        class: &K,
        method: &M,
    ) -> Option<u16> {
        self.class(class)
            .and_then(|(_, _, info)| info.member_slots.get(method).copied())
            .or_else(|| {
                self.class_ids
                    .get(class)
                    .and_then(|id| self.class_member_topology.get(id))
                    .and_then(|topology| topology.slots.get(method).copied())
            })
    }

    pub(crate) fn member_entry_by_ref(&self, method: super::MethodRef) -> Option<&MethodEntry> {
        let info = self.class_by_id(method.class())?;
        let slot = usize::from(method.slot());
        if let Some(entry) = info.member_order.get(slot) {
            return Some(entry);
        }
        self.class_member_topology
            .get(&method.class())?
            .entries
            .get(slot.checked_sub(info.member_order.len())?)
    }

    pub(crate) fn add_member_entry(
        &mut self,
        class: ClassId,
        entry: MethodEntry,
    ) -> Option<super::MethodRef> {
        let declaration_len = self.class_by_id(class)?.member_order.len();
        if self
            .class_by_id(class)?
            .member_slots
            .contains_key(&entry.name)
        {
            return None;
        }
        let slot = self
            .class_member_topology
            .entry(class)
            .or_default()
            .add(declaration_len, entry)?;
        Some(super::MethodRef::new(class, slot))
    }

    pub(crate) fn method_result(&self, method: super::MethodRef) -> Option<&FunctionReflection> {
        self.deferred_class_result(method.class())
            .and_then(|results| results.method(method))
            .or_else(|| {
                self.class_method_results
                    .get(&method)
                    .map(|signature| signature.get())
            })
    }

    pub(crate) fn method_results_for_class(
        &self,
        class: super::ClassId,
    ) -> impl Iterator<Item = (super::MethodRef, &FunctionReflection)> {
        let resolved = self
            .deferred_class_result(class)
            .into_iter()
            .flat_map(move |results| {
                results
                    .methods
                    .iter()
                    .map(|(method, signature)| (*method, signature))
            });
        resolved.chain(
            self.class_method_results
                .of_class(class)
                .map(|(method, signature)| (method, signature.get())),
        )
    }

    pub(crate) fn raw_method_results_for_class(
        &self,
        class: super::ClassId,
    ) -> impl Iterator<Item = (super::MethodRef, &FunctionReflection)> {
        self.class_method_results
            .of_class(class)
            .map(|(method, signature)| (method, signature.get()))
    }

    pub(crate) fn set_method_result(
        &mut self,
        method: super::MethodRef,
        signature: FunctionReflection,
    ) {
        self.invalidate_deferred_class_result(method.class());
        self.class_method_results
            .insert(method, std::sync::Arc::new(super::Lazy::decoded(signature)));
    }

    pub(crate) fn set_shared_method_result(
        &mut self,
        method: super::MethodRef,
        signature: std::sync::Arc<super::Lazy<FunctionReflection>>,
    ) -> bool {
        self.invalidate_deferred_class_result(method.class());
        self.class_method_results
            .insert_if_absent(method, signature)
    }

    pub(crate) fn method_is_abstract<K: TableKey + ?Sized, M: FoldKey + ?Sized>(
        &self,
        class: &K,
        method: &M,
    ) -> bool {
        self.member_slot(class, method)
            .and_then(|slot| {
                self.class_ids
                    .get(class)
                    .copied()
                    .and_then(|id| self.member_entry_by_ref(super::MethodRef::new(id, slot)))
            })
            .is_some_and(|entry| entry.is_abstract)
    }

    pub(crate) fn method_is_static<K: TableKey + ?Sized>(&self, class: &K, method: &str) -> bool {
        self.member_slot(class, method)
            .and_then(|slot| {
                self.class_ids
                    .get(class)
                    .copied()
                    .and_then(|id| self.member_entry_by_ref(super::MethodRef::new(id, slot)))
            })
            .is_some_and(|entry| entry.is_static)
    }

    pub(crate) fn method_is_final(&self, class: &str, method: &str) -> bool {
        self.member_slot(class, method)
            .and_then(|slot| {
                self.class_ids
                    .get(class)
                    .copied()
                    .and_then(|id| self.member_entry_by_ref(super::MethodRef::new(id, slot)))
            })
            .is_some_and(|entry| entry.is_final)
    }

    pub(crate) fn method_is_final_by_phpdoc(&self, class: &str, method: &str) -> bool {
        self.member_slot(class, method)
            .and_then(|slot| {
                self.class_ids
                    .get(class)
                    .copied()
                    .and_then(|id| self.member_entry_by_ref(super::MethodRef::new(id, slot)))
            })
            .is_some_and(|entry| entry.is_final_by_phpdoc)
    }

    /// The owners of `class`'s method names, see `ClassOwners`.
    pub(crate) fn method_owners<K: TableKey + ?Sized>(
        &self,
        class: &K,
    ) -> Option<&FoldedMap<Name>> {
        self.class_ids
            .get(class)
            .and_then(|id| self.member_owners.get(id))
            .map(|owners| owners.methods.get())
    }

    /// The owners of `class`'s property names.
    pub(crate) fn property_owners<K: TableKey + ?Sized>(
        &self,
        class: &K,
    ) -> Option<&NameMap<Name>> {
        self.class_ids
            .get(class)
            .and_then(|id| self.member_owners.get(id))
            .map(|owners| owners.properties.get())
    }

    /// The owners of `class`'s constant names.
    pub(crate) fn const_owners<K: TableKey + ?Sized>(&self, class: &K) -> Option<&NameMap<Name>> {
        self.class_ids
            .get(class)
            .and_then(|id| self.member_owners.get(id))
            .map(|owners| owners.consts.get())
    }

    /// Does `class` derive from or implement `of`, directly or
    /// transitively? A single `HashSet::contains` against
    /// `self.ancestors_by_id` (see `precompute_ancestor_names`) - cheaper
    /// than a fresh `extends`+`implements` DFS on every one of this
    /// function's dozens of read-only, post-load call sites.
    /// Whether `class` is `of` or extends or implements it, both compared
    /// as PHP compares class names, case-insensitively.
    pub fn is_subclass_of<K: TableKey + ?Sized>(&self, class: &K, of: &str) -> bool {
        if class.with_key(|class| class.eq_ignore_ascii_case(of)) {
            return true;
        }
        let Some(ancestors) = self
            .class_ids
            .get(class)
            .and_then(|id| self.ancestors_by_id.get(id))
        else {
            return false;
        };
        if let Some((id, _, _)) = self.class(of) {
            ancestors.ids.contains(&id)
        } else {
            ancestors.unresolved.contains(of)
        }
    }

    /// The extends/implements closure for a declaration, addressed through
    /// its stable class ID. Names are reconstructed only at this API
    /// boundary for consumers that still need class names.
    pub(crate) fn ancestor_names_of(&self, class: &str) -> Option<NameSet> {
        let id = self.class_ids.get(class)?;
        self.ancestors_by_id
            .get(id)
            .map(|ancestors| ancestors.names(self))
    }

    /// The exact `extends`+`implements`-only transitive closure
    /// `is_subclass_of` needs - NOT `all_ancestor_names`'s superset
    /// (which also chains `uses`, a trait relationship
    /// `instanceof`/`is_subclass_of` never recognizes). `class` itself
    /// is never included (matching `is_subclass_of`'s own separate
    /// `class == of` check).
    fn class_extends_implements_closure(&self, class: &Name) -> AncestorIds {
        // The names walked are borrowed from the classes (or `class`), and
        // one is copied only the first time it is found.
        let mut seen: FxHashSet<Name> = FxHashSet::default();
        let mut stack: Vec<Name> = vec![class.clone()];
        let mut result = AncestorIds::default();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(references) = self.class_references_by_name(&current) else {
                continue;
            };
            for parent in references.extends.iter().chain(references.implements) {
                if let Some(id) = self.class_ids.get(parent) {
                    result.ids.insert(*id);
                } else {
                    result.unresolved.insert(parent.clone());
                }
                stack.push(parent.clone());
            }
        }
        result
    }

    /// Whether a name under `PHPStan\` stands among what `class` extends,
    /// implements or uses, transitively, a trait's own `uses` included: what
    /// `all_ancestor_names` collects, stopping at the first.
    fn class_reaches_phpstan_name(&self, class: &Name) -> bool {
        let mut seen: FxHashSet<Name> = FxHashSet::default();
        let mut stack: Vec<Name> = vec![class.clone()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(references) = self.class_references_by_name(&current) else {
                continue;
            };
            for parent in references
                .extends
                .iter()
                .chain(references.implements)
                .chain(references.uses)
            {
                if crate::registry::is_phpstan_namespace(parent) {
                    return true;
                }
                stack.push(parent.clone());
            }
        }
        false
    }

    /// Rebuilds the ID-addressed PHPStan-ancestry verdicts after loading a
    /// cached registry, whose declaration IDs are assigned when it freezes.
    pub(crate) fn rebuild_phpstan_ancestry_ids(&mut self) {
        let class_names = self.scoped_class_names();
        let registry: &Registry = self;
        self.phpstan_ancestry = class_names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|name| {
                registry
                    .class_reaches_phpstan_name(name)
                    .then(|| registry.class_ids.get(name).copied())
                    .flatten()
            })
            .collect();
    }

    /// Whether `class` has a name under `PHPStan\` among what it extends,
    /// implements or uses, transitively: the gate of the rules that read
    /// the PHPStan API's use from a class that is part of an extension.
    /// Known for the classes the passes have been over, which is every
    /// class a project declares.
    pub(crate) fn class_has_phpstan_ancestry(&self, class: &str) -> bool {
        self.class(class)
            .is_some_and(|(id, ..)| self.phpstan_ancestry.contains(&id))
    }

    /// Rebuilds `self.ancestors_by_id` (see `is_subclass_of`) from
    /// scratch over every registered class - call this any time the
    /// class graph could have changed, so `is_subclass_of` can always
    /// trust this map is current. The results are collected directly into
    /// the ID-keyed table.
    #[hotpath::measure]
    pub fn precompute_ancestor_names(&mut self) {
        let class_names = self.scoped_class_names();
        let registry: &Registry = self;
        let ancestor_names: FxHashMap<_, _> = class_names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|name| {
                Some((
                    *registry.class_ids.get(name)?,
                    (
                        registry.class_extends_implements_closure(name),
                        registry.class_reaches_phpstan_name(name),
                    ),
                ))
            })
            .collect();
        if self.scope.is_all() {
            self.phpstan_ancestry = ancestor_names
                .par_iter()
                .filter_map(|(id, (_, reaches))| reaches.then_some(*id))
                .collect();
            self.ancestors_by_id = ancestor_names
                .into_par_iter()
                .map(|(id, (ancestors, _))| (id, ancestors))
                .collect();
            self.subtype_children = NameTable::default();
            self.trait_users = NameTable::default();
            self.recorded_parents = NameTable::default();
        } else {
            let active_ids: FxHashSet<ClassId> = self.class_ids.values().copied().collect();
            self.phpstan_ancestry.retain(|id| {
                active_ids.contains(id)
                    && !ancestor_names.get(id).is_some_and(|(_, reaches)| !reaches)
            });
            self.ancestors_by_id.retain(|id, _| active_ids.contains(id));
            for name in self.scoped_removed_class_keys() {
                self.unrecord_parents(&name);
            }
            self.phpstan_ancestry.par_extend(
                ancestor_names
                    .par_iter()
                    .filter_map(|(id, (_, reaches))| reaches.then_some(*id)),
            );
            self.ancestors_by_id.par_extend(
                ancestor_names
                    .into_par_iter()
                    .map(|(id, (ancestors, _))| (id, ancestors)),
            );
        }
        for name in class_names.iter() {
            let Some(references) = self.class_references_by_name(name) else {
                self.unrecord_parents(name);
                continue;
            };
            // Recorded already, under this spelling, with these parents.
            if self.recorded_parents.get_with_name(name).is_some_and(
                |(declared, (supertypes, traits))| {
                    declared == name
                        && supertypes
                            .iter()
                            .eq(references.extends.iter().chain(references.implements))
                        && traits.as_slice() == references.uses
                },
            ) {
                continue;
            }
            let supertypes: Vec<Name> = references
                .extends
                .iter()
                .chain(references.implements)
                .cloned()
                .collect();
            let traits: Vec<Name> = references.uses.to_vec();
            self.unrecord_parents(name);
            for parent in &supertypes {
                self.subtype_children
                    .get_or_insert_with(parent.clone(), NameSet::default)
                    .insert(name.clone());
            }
            for used in &traits {
                self.trait_users
                    .get_or_insert_with(used.clone(), NameSet::default)
                    .insert(name.clone());
            }
            self.recorded_parents
                .insert(name.clone(), (supertypes, traits));
        }
    }

    /// Takes `class` out of the subtype and trait-user indexes. It is in
    /// them under the spelling it was recorded with, which the record keeps.
    fn unrecord_parents(&mut self, class: &str) {
        let Some((declared, (supertypes, traits))) = self.recorded_parents.get_with_name(class)
        else {
            return;
        };
        for (index, parents) in [
            (&mut self.subtype_children, supertypes),
            (&mut self.trait_users, traits),
        ] {
            for parent in parents {
                if let Some(children) = index.get_mut(parent) {
                    children.remove(declared);
                    if children.is_empty() {
                        index.remove(parent);
                    }
                }
            }
        }
        self.recorded_parents.remove(class);
    }

    /// Every class-like extending or implementing `class`, directly or
    /// transitively - `is_subclass_of`'s converse, without `class`.
    pub(crate) fn subclasses(&self, class: &(impl KeyName + ?Sized)) -> Vec<Name> {
        self.descendants(class, false).to_vec()
    }

    /// `subclasses`, computed once for the graph as it is.
    pub(crate) fn subclasses_shared(&self, class: &(impl KeyName + ?Sized)) -> Arc<[Name]> {
        self.descendants(class, false)
    }

    /// `subclasses`, and every class-like using `class` as a trait or
    /// descending from one that does - `all_ancestor_names`'s converse.
    pub(crate) fn inheritors(&self, class: &(impl KeyName + ?Sized)) -> Vec<Name> {
        self.descendants(class, true).to_vec()
    }

    fn descendants(&self, class: &(impl KeyName + ?Sized), with_traits: bool) -> Arc<[Name]> {
        let mut computed = false;
        let below =
            self.reach_memo
                .descendants
                .get_or_compute((class.lowered_name(), with_traits), || {
                    computed = true;
                    self.walk_descendants(class.text(), with_traits)
                });
        stats::count(if computed {
            Counter::DescendantsMiss
        } else {
            Counter::DescendantsHit
        });
        below
    }

    /// The classes below `class`, found by walking the graph.
    pub(crate) fn walk_descendants(&self, class: &str, with_traits: bool) -> Arc<[Name]> {
        let mut seen: FxHashSet<Name> = FxHashSet::default();
        seen.insert(class.lowered_name());
        let mut result = Vec::new();
        let mut stack = vec![Name::from(class)];
        while let Some(current) = stack.pop() {
            let children = self.subtype_children.get(&current).into_iter().flatten();
            let users = with_traits
                .then(|| self.trait_users.get(&current))
                .flatten()
                .into_iter()
                .flatten();
            for child in children.chain(users) {
                if seen.insert(child.lowered().clone()) {
                    result.push(child.clone());
                    stack.push(child.clone());
                }
            }
        }
        result.into()
    }

    /// Is `class` (or any ancestor it `extends`) tagged
    /// `@phpstan-immutable` (or `@phpstan-readonly` - both fold into
    /// `ClassReflection::is_immutable`)? Query-time, `extends`-only, mirroring
    /// real PHPStan's own `ClassReflection::isImmutable()` (walks
    /// `getParentClass()` only, never `implements`/`uses`).
    pub(crate) fn class_is_immutable(&self, class: &str) -> bool {
        let mut seen = FxHashSet::default();
        let mut stack = vec![Name::from(class)];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some((_, _, info)) = self.class(&current) else {
                continue;
            };
            if info.is_immutable {
                return true;
            }
            if let Some(references) = self.class_references_by_name(&current) {
                stack.extend(references.extends.iter().cloned());
            }
        }
        false
    }

    /// Every interface/trait `class` transitively inherits from -
    /// `extends`/`implements` *and* `uses`, which `is_subclass_of`
    /// doesn't chain (a used trait isn't itself a supertype
    /// `instanceof` recognizes). `class` itself is never included.
    /// Needed by `checker::oop::check_require_tags`: real PHPStan's
    /// `getInterfaces()`/`getTraits(true)` are both transitive the
    /// identical way.
    pub(crate) fn all_ancestor_names(&self, class: &str) -> NameSet {
        let mut seen: FxHashSet<Name> = FxHashSet::default();
        let mut stack: Vec<Name> = vec![Name::from(class)];
        let mut result = NameSet::default();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(references) = self.class_references_by_name(&current) else {
                continue;
            };
            for parent in references
                .extends
                .iter()
                .chain(references.implements)
                .chain(references.uses)
            {
                if !result.contains(parent) {
                    result.insert(parent.clone());
                }
                stack.push(parent.clone());
            }
        }
        result
    }

    /// Every real class/enum that ends up composing `trait_fqn`,
    /// walking past any intermediate *trait* user - `flatten_traits`
    /// merges a used trait's methods into the *user's* method map, so a
    /// trait used only by another trait (`trait B { ... } trait A {
    /// use B; } class X { use A; }`) still needs `B` checked against
    /// `X`, the real, final host.
    pub(crate) fn classes_using_trait(&self, trait_fqn: &str) -> Vec<Name> {
        let mut result = Vec::new();
        let mut stack = vec![Name::from(trait_fqn)];
        let mut seen = FxHashSet::default();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            for name in self.trait_users.get(&current).into_iter().flatten() {
                if self
                    .class(name)
                    .is_some_and(|(_, _, info)| info.kind == ClassKind::Trait)
                {
                    stack.push(name.clone());
                } else {
                    result.push(name.clone());
                }
            }
        }
        result.sort();
        result
    }

    /// The exhaustive BFS build `precompute_member_owners` runs once
    /// per class. Walks `class`'s own `extends` (*and* `implements`, so
    /// an interface-typed receiver's own multi-parent `extends` chain
    /// and a class's declared-but-not-overridden interface methods both
    /// resolve) ancestor graph, in the same order `is_subclass_of` does,
    /// recording every method/property/const name found under the
    /// *closest* (first-visited-by-BFS) declaring ancestor - `class`
    /// itself included, checked before any parent. A single-parent-only
    /// walk (`extends.first()`) would be wrong for interfaces
    /// specifically, since unlike a class, PHP lets an interface
    /// `extends` more than one parent at once (real shape:
    /// league/flysystem's
    /// `FilesystemOperator extends FilesystemReader, FilesystemWriter`).
    ///
    /// Fused: builds all three (methods/properties/consts) in a single
    /// walk rather than three independent ones, since the BFS topology
    /// never depends on which member kind is being extracted - only on
    /// `class`'s own `extends`/`implements`. `contains_key` guards
    /// (rather than `entry(name.to_string()).or_insert_with(...)`) avoid
    /// allocating a String key on a name a closer ancestor already
    /// claimed.
    #[allow(clippy::type_complexity)]
    fn own_owner_names(&self, class: &Name) -> (FoldedMap<Name>, NameMap<Name>, NameMap<Name>) {
        let mut method_owner = FoldedMap::default();
        let mut property_owner = NameMap::default();
        let mut const_owner = NameMap::default();
        let mut seen: FxHashSet<Name> = FxHashSet::default();
        let mut queue: VecDeque<Name> = VecDeque::new();
        queue.push_back(class.clone());
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some((id, _, info)) = self.class(current.as_str()) else {
                continue;
            };
            for name in self.method_names(current.as_str()) {
                if !method_owner.contains_key(name) {
                    method_owner.insert(name.clone(), current.clone());
                }
            }
            for name in info.properties.keys() {
                if !property_owner.contains_key(name) {
                    property_owner.insert(name.clone(), current.clone());
                }
            }
            for (property, _) in self.property_results_for_class(id) {
                if let Some(name) = self.property_name_by_ref(property)
                    && !property_owner.contains_key(name)
                {
                    property_owner.insert(name.clone(), current.clone());
                }
            }
            for name in info.consts.keys() {
                if !const_owner.contains_key(name) {
                    const_owner.insert(name.clone(), current.clone());
                }
            }
            for (_, name, _) in self.class_constant_results_for_class(id) {
                if !const_owner.contains_key(name) {
                    const_owner.insert(name.clone(), current.clone());
                }
            }
            if let Some(references) = self.class_references_by_name(current.as_str()) {
                for parent in references.extends.iter().chain(references.implements) {
                    queue.push_back(parent.clone());
                }
            }
        }
        (method_owner, property_owner, const_owner)
    }

    /// Rebuilds every `*_owner_by_class` map (see their own doc
    /// comments on `Registry`) from scratch over every registered class,
    /// call this any time the class graph could have changed (called
    /// from the identical settle points `precompute_ancestor_names` is:
    /// `Registry::index_and_flatten`/`build`, `Project::finish_loading`/
    /// `update_file`). Avoids a fresh BFS on every
    /// `lookup_method`/`lookup_property`/`lookup_const`/their
    /// `_with_owner`/`lookup_ancestor_*` call.
    /// `find_method_signature_prototype`'s own hand-ordered
    /// interface/trait/extends priority walk is untouched, since it only
    /// calls `lookup_method_with_owner` as a sub-step, so it benefits
    /// automatically.
    #[hotpath::measure]
    pub fn precompute_member_owners(&mut self) {
        let class_names = self.scoped_class_names();
        let registry: &Registry = self;

        let own_by_class: FxHashMap<_, _> = class_names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|name| {
                let id = *registry.class_ids.get(name)?;
                let (methods, properties, consts) = registry.own_owner_names(name);
                Some((
                    id,
                    ClassOwners {
                        methods: Lazy::decoded(methods),
                        properties: Lazy::decoded(properties),
                        consts: Lazy::decoded(consts),
                    },
                ))
            })
            .collect();
        if self.scope.is_all() {
            self.member_owners = own_by_class;
        } else {
            let active_ids: FxHashSet<ClassId> = self.class_ids.values().copied().collect();
            self.member_owners.retain(|id, _| active_ids.contains(id));
            self.member_owners.par_extend(own_by_class);
        }
    }

    /// Brings the member-owner tables up to date with the members
    /// `resolve_deferred_mixins` just added: rebuilds them for the scope's
    /// classes that have a class with `@mixin`s in the scope somewhere in
    /// their hierarchy, itself included - the only ones whose members
    /// changed since `precompute_member_owners` last ran.
    pub(crate) fn precompute_member_owners_after_mixins(&mut self) {
        let class_names = self.scoped_class_names();
        let with_mixins: FxHashSet<String> = class_names
            .iter()
            .filter(|name| {
                self.class_references_by_name(name)
                    .is_some_and(|references| !references.mixins.is_empty())
            })
            .map(|name| name.to_ascii_lowercase())
            .collect();
        if with_mixins.is_empty() {
            return;
        }
        let affected: Vec<&Name> = class_names
            .iter()
            .filter(|name| {
                let mut seen = FxHashSet::default();
                let mut queue: Vec<Name> = vec![(*name).clone()];
                while let Some(current) = queue.pop() {
                    if with_mixins.contains(current.lower()) {
                        return true;
                    }
                    if !seen.insert(current.clone()) {
                        continue;
                    }
                    if let Some(references) = self.class_references_by_name(&current) {
                        queue.extend(
                            references
                                .extends
                                .iter()
                                .chain(references.implements)
                                .cloned(),
                        );
                    }
                }
                false
            })
            .collect();
        let scope = crate::registry::scope::Scope::of(
            affected.iter().map(|name| name.as_ref()),
            std::iter::empty(),
        );
        let outer = std::mem::replace(&mut self.scope, scope);
        self.precompute_member_owners();
        self.scope = outer;
    }

    /// Looks up a method, walking the extends/implements ancestor graph
    /// if not declared directly on `class`. Traits aren't modeled here as
    /// a distinct concept - `flatten_traits` already merges a trait's
    /// members directly into the using class's own `methods` map, so they
    /// show up as if declared there.
    ///
    /// A single two-level `HashMap` lookup against `self.method_owner_by_
    /// class` (see `precompute_member_owners`'s own doc comment for what
    /// this replaced and why).
    /// `method` as `owner` declares it (method names resolve
    /// case-insensitively; messages spell them as declared).
    pub(crate) fn declared_method_name(&self, owner: &str, method: &str) -> String {
        self.method_names(owner)
            .find(|name| name.eq_ignore_ascii_case(method))
            .map(ToString::to_string)
            .unwrap_or_else(|| method.to_string())
    }

    /// Iterates the methods this class declares or composes, in its local
    /// method order. Finishing-pass entries live in the topology result.
    pub fn method_names<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a Name> + 'a {
        self.member_entries(class).map(|entry| &entry.name)
    }

    pub(crate) fn lookup_own_method<K: TableKey + ?Sized, M: FoldKey + ?Sized>(
        &self,
        class: &K,
        method: &M,
    ) -> Option<&FunctionReflection> {
        // Three tables are asked for the name: its hash is read once.
        let method = &syntax::name::Hashed::new(method);
        let (_, id) = self.class_ids.get_with_name(class)?;
        let id = *id;
        let info = self.class_by_id(id);
        let slot = info
            .and_then(|info| info.member_slots.get(method).copied())
            .or_else(|| {
                self.class_member_topology
                    .get(&id)
                    .and_then(|topology| topology.slots.get(method).copied())
            });
        if let Some(result) =
            slot.and_then(|slot| self.method_result(super::MethodRef::new(id, slot)))
        {
            return Some(result);
        }
        info?.methods.get(method)
    }

    pub(crate) fn shared_own_method(
        &self,
        class: &str,
        method: &str,
    ) -> Option<std::sync::Arc<super::Lazy<FunctionReflection>>> {
        if let Some(result) = self
            .class_ids
            .get(class)
            .copied()
            .zip(self.member_slot(class, method))
            .and_then(|(class, slot)| {
                self.class_method_results
                    .get(&super::MethodRef::new(class, slot))
                    .cloned()
            })
        {
            return Some(result);
        }
        self.class(class)?.2.methods.shared_get(method)
    }

    pub(crate) fn method_by_ref(&self, method: super::MethodRef) -> Option<&FunctionReflection> {
        self.method_result(method).or_else(|| {
            let class = self.class_name_by_id(method.class())?;
            let name = self.member_entry_by_ref(method)?.name.as_ref();
            self.class(class)?.2.methods.get(name)
        })
    }

    pub fn lookup_method<K: TableKey + ?Sized, M: FoldKey + ?Sized>(
        &self,
        class: &K,
        method: &M,
    ) -> Option<&FunctionReflection> {
        // Real PHP resolves method calls case-insensitively, the same as
        // class and function names: the owners hold one entry for each
        // method, by the most derived declaration's spelling.
        let (declared, owner) = self.method_owners(class)?.get_key_value(method)?;
        self.lookup_own_method(owner, declared)
    }

    /// The real declared spelling `lookup_method`/`lookup_method_with_
    /// owner` would resolve `method` to, for comparing against the
    /// as-written call-site text.
    pub(crate) fn lookup_method_canonical_name(&self, class: &str, method: &str) -> Option<&str> {
        let (name, _) = self.method_owners(class)?.get_key_value(method)?;
        Some(name.as_ref())
    }

    /// Every method name callable on `class` (own + inherited), for
    /// building a "did you mean?" suggestion at a `method.notFound` site -
    /// just the already-precomputed `method_owner_by_class` key set, no
    /// extra ancestor walk needed.
    pub(crate) fn all_method_names(&self, class: &str) -> impl Iterator<Item = &str> {
        self.method_owners(class)
            .into_iter()
            .flat_map(|m| m.keys().map(|s| s.as_ref()))
    }

    /// `all_method_names`'s own property counterpart, for
    /// `textDocument/completion`'s member-completion candidate list.
    pub(crate) fn all_property_names(&self, class: &str) -> impl Iterator<Item = &str> {
        self.property_owners(class)
            .into_iter()
            .flat_map(|m| m.keys().map(|s| s.as_ref()))
    }

    /// `all_method_names`'s own const counterpart (an enum's cases are
    /// folded into this same map - see `ClassReflection::consts`'s own doc
    /// comment).
    pub(crate) fn all_const_names(&self, class: &str) -> impl Iterator<Item = &str> {
        self.const_owners(class)
            .into_iter()
            .flat_map(|m| m.keys().map(|s| s.as_ref()))
    }

    /// The spelling an ancestor of `class` declares `method` with, when it
    /// differs from `method`'s: each parent's owners name a method by the
    /// spelling of the most derived declaration in its ancestry.
    pub(crate) fn find_case_mismatched_ancestor_method(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(Name, Name)> {
        self.parent_method_owner_maps(class).find_map(|owners| {
            let (name, owner) = owners.get_key_value(method)?;
            (name.as_ref() != method).then(|| (owner.clone(), name.clone()))
        })
    }

    /// Same as `lookup_method`, but also returns the declaring ancestor's
    /// name - needed wherever a caller goes on to do class-level generic
    /// substitution (`checker::class_template_bindings`),
    /// since a `@template` name is scoped to the class that declares it,
    /// not globally unique - substituting an inherited method/property's
    /// `TemplateParam`s correctly requires knowing which ancestor's own
    /// template scope they belong to, not just the value found.
    pub fn lookup_method_with_owner<K: TableKey + ?Sized, M: FoldKey + ?Sized>(
        &self,
        class: &K,
        method: &M,
    ) -> Option<(Name, &FunctionReflection)> {
        // See `lookup_method`.
        let (declared, owner) = self.method_owners(class)?.get_key_value(method)?;
        let sig = self.lookup_own_method(owner, declared)?;
        Some((owner.clone(), sig))
    }

    /// The own-owner maps of the direct parents of `info`, in the order a
    /// lookup of an ancestor's member tries them: its `extends` and then
    /// its `implements`, each parent's map covering that parent's whole
    /// ancestry. For each parent *in order*, the parent's *entire*
    /// ancestor chain is tried before the next parent, which is the
    /// priority `lookup_ancestor_method_with_owner` is specified by -
    /// not a single walk merged across all the parents at once.
    pub(crate) fn parent_method_owner_maps<'a: 'i, 'i>(
        &'a self,
        class: &'i str,
    ) -> impl Iterator<Item = &'a FoldedMap<Name>> + 'i {
        self.class_references_by_name(class)
            .into_iter()
            .flat_map(|references| references.extends.iter().chain(references.implements))
            .filter_map(|parent| self.method_owners(&parent))
    }

    /// The ancestor of `info` that owns its method `method`, the first
    /// parent in `parent_method_owner_maps` order that has one.
    pub(crate) fn ancestor_method_owner(&self, class: &str, method: &str) -> Option<&Name> {
        self.parent_method_owner_maps(class)
            .find_map(|owners| owners.get(method))
    }

    /// The ancestor of `info` that owns its property `prop`: only
    /// `extends` is tried, since real PHP interfaces can't declare
    /// properties at all.
    pub(crate) fn ancestor_property_owner(&self, class: &str, prop: &str) -> Option<&Name> {
        self.class_references_by_name(class)
            .into_iter()
            .flat_map(|references| references.extends.iter())
            .filter_map(|parent| self.property_owners(&parent))
            .find_map(|owners| owners.get(prop))
    }

    /// Same as `lookup_method_with_owner`, but starts the walk at
    /// `class`'s own parents rather than `class` itself - used for
    /// override-compatibility checking, which needs the signature a
    /// method is overriding, not the overriding one itself.
    pub fn lookup_ancestor_method_with_owner(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(Name, &FunctionReflection)> {
        let owner = self.ancestor_method_owner(class, method)?;
        let sig = self.lookup_own_method(owner, method)?;
        Some((owner.clone(), sig))
    }

    /// A faithful port of real PHPStan's own
    /// `MethodPrototypeFinder::findPrototype` priority order - used
    /// **only** by `check_one_method_override`'s Liskov-compatibility
    /// ancestor lookup, not a replacement for
    /// `lookup_ancestor_method_with_owner`'s other call sites
    /// (runtime-dispatch/purity questions). A plain
    /// `extends.iter().chain(implements.iter())` walk would get this
    /// wrong: an *immediately*-implemented interface's own native
    /// declaration always wins over an `extends` parent's, and a used
    /// trait's **abstract** declaration wins over the `extends` parent
    /// too when no immediate interface declares the method.
    ///
    /// Returns `(owner, sig, check_visibility)` - `check_visibility` is
    /// `false` **only** for a trait-abstract-sourced prototype (step 2
    /// below), mirroring real `MethodPrototypeFinder::findPrototype`'s
    /// three-element-tuple return: its interface and `extends`-parent
    /// branches both return `true`, but its trait branch returns
    /// `false`. This matters because real PHP does **not** enforce
    /// visibility-widening compatibility between a trait's `abstract`
    /// method declaration and the concrete method that fulfills it
    /// (`trait T { abstract protected function f(); } class C { use T;
    /// private function f() {} }` compiles with no error) - a real,
    /// unconditional fatal error for the interface/extends cases, but
    /// not this one (real shape: Symfony's own
    /// `AmpResponse::schedule()` (`private`) fulfilling
    /// `TransportResponseTrait`'s own
    /// `abstract protected static function schedule()`).
    pub fn find_method_signature_prototype(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(Name, &FunctionReflection, bool)> {
        let references = self.class_references_by_name(class)?;
        // 1. Immediately-implemented interfaces - walks each interface's
        //    *own* ancestor chain too (`lookup_method_with_owner`, not a
        //    bare `.methods.get`), not just its own direct declaration
        //    (real shape: Piwigo's own
        //    `ExtensionInterface extends SubscriberInterface`, where
        //    `subscribedEvents()` is declared only on the *grandparent*
        //    interface).
        for iface in references.implements {
            if let Some((owner, sig)) = self.lookup_method_with_owner(iface, method) {
                return Some((owner, sig, true));
            }
        }
        // 2. A used trait's own abstract declaration - unconditional,
        //    matching this file's other override-compatibility checks
        //    (real PHPStan's own equivalent gate,
        //    `supportsAbstractTraitMethods`, is PHP 8.0+, a bar every
        //    realistic target version already clears).
        for used in references.uses {
            if self
                .class(used)
                .is_some_and(|_| self.method_is_abstract(used, method))
                && let Some(sig) = self.lookup_own_method(used, method)
            {
                return Some((used.clone(), sig, false));
            }
        }
        // 3. Ordinary `extends` chain (existing, already-correct
        //    recursive walk).
        for parent in references.extends {
            if let Some((owner, sig)) = self.lookup_method_with_owner(parent, method) {
                return Some((owner, sig, true));
            }
        }
        None
    }

    /// `@phpstan-consistent-constructor`'s own ancestor walk,
    /// transcribed from real PHPStan's
    /// `ConsistentConstructorHelper::findConsistentConstructor`: starting
    /// at `class`'s own *parent* (not `class` itself - the real rule's
    /// call site already has the child's own method in hand and is
    /// looking for what it's being held *to*), walks up `extends` until
    /// an ancestor tagged `@phpstan-consistent-constructor` is found,
    /// returning that ancestor's own `__construct` - or `None` if the
    /// chain ends (or cycles - bounded by the registered class ID count, same
    /// guard `Registry::flatten_traits`'s cycle protection uses) with no
    /// tagged ancestor at all, meaning no check applies, real
    /// PHP's own default.
    ///
    /// Deliberately doesn't synthesize a zero-parameter dummy constructor
    /// for a tagged ancestor with no `__construct` of its own the way real
    /// PHPStan's `DummyConstructorReflection` does - every `FunctionReflection`
    /// this project returns is borrowed from registry declaration/result
    /// storage, and a class that bothers tagging itself
    /// `@phpstan-consistent-constructor` at
    /// all almost always also declares the constructor it's pinning down
    /// (the entire point of the tag), so this narrower scope isn't
    /// expected to leave real value on the table.
    pub(crate) fn find_consistent_constructor_ancestor(
        &self,
        class: &str,
    ) -> Option<(Name, &FunctionReflection)> {
        let mut current = self
            .class_references_by_name(class)?
            .extends
            .first()?
            .clone();
        for _ in 0..self.class_ids.len().max(1) {
            let (_, _, info) = self.class(&current)?;
            if info.is_consistent_constructor {
                return self
                    .lookup_own_method(&current, "__construct")
                    .map(|sig| (current.clone(), sig));
            }
            current = self
                .class_references_by_name(&current)?
                .extends
                .first()?
                .clone();
        }
        None
    }

    /// Whether `class` directly `use`s a trait that declares a method
    /// named `method` *abstract* - used only by `#[\Override]`-attribute
    /// checking, which needs to treat implementing a *trait's own*
    /// abstract method as a real override too: a class implementing a
    /// trait's `abstract protected function twist()` with `#[\Override]`
    /// is real, legal PHP (real shape:
    /// `Psl\RandomSequence\MersenneTwisterSequence::twist()` overriding
    /// `Internal\MersenneTwisterTrait`'s own abstract declaration this
    /// exact way). Deliberately checks the trait's `abstract_methods`
    /// set, not merely "does the trait have a method of this name at
    /// all" - a trait supplying a *concrete* method the using class
    /// never separately redeclares is a different case, with its own
    /// already-correct handling: `check_method_overrides`'s own
    /// trait-derived-method sweep re-runs this same function keyed on
    /// the using class, so a genuinely ancestor-less concrete trait
    /// method still fatals there exactly as real PHP does. Deliberately
    /// narrower than folding `uses` into
    /// `lookup_ancestor_method_with_owner`'s general walk, which would
    /// also change every *other* check in `check_one_method_override`
    /// (Liskov parameter/return-type comparisons) in unverified ways;
    /// this only answers the one yes/no question the override-attribute
    /// check needs.
    pub fn trait_declares_abstract_method(&self, class: &str, method: &str) -> bool {
        let Some(references) = self.class_references_by_name(class) else {
            return false;
        };
        references.uses.iter().any(|used_trait| {
            self.class(used_trait)
                .is_some_and(|_| self.method_is_abstract(used_trait, method))
        })
    }

    /// Whether `class`'s own `extends`/`implements` ancestor chain
    /// includes a name this project's registry has no declaration for
    /// at all, at any depth - used only by `#[\Override]`-attribute
    /// checking, which must never confidently claim "no matching parent
    /// method exists" when the real reason nothing was found is a
    /// missing dependency rather than a genuine absence (real shape:
    /// `CycloneDX\Composer\Plugin implements ...
    /// Composer\Plugin\PluginInterface`, unvendored, so this registry
    /// has no way to know whether the real `activate()` being overridden
    /// is declared there). Every other check in
    /// `check_one_method_override` stays lenient-by-omission (an
    /// unresolved ancestor yields no comparison, same as a genuinely
    /// absent one) - only this check's polarity (firing exactly when
    /// *nothing* is found) makes the unresolved-vs-absent distinction
    /// matter.
    pub fn has_unresolvable_ancestor(&self, class: &str) -> bool {
        let Some(references) = self.class_references_by_name(class) else {
            return false;
        };
        let mut seen = FxHashSet::default();
        let mut queue: VecDeque<Name> = references
            .extends
            .iter()
            .chain(references.implements)
            .cloned()
            .collect();
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(_) = self.class(&current) else {
                return true;
            };
            if let Some(references) = self.class_references_by_name(&current) {
                queue.extend(
                    references
                        .extends
                        .iter()
                        .chain(references.implements)
                        .cloned(),
                );
            }
        }
        false
    }

    /// Same idea as `lookup_ancestor_method_with_owner`, but walking
    /// only `extends` - real PHP interfaces can't declare properties, so
    /// there's no `implements` side to check.
    pub fn lookup_ancestor_property_with_owner(
        &self,
        class: &str,
        prop: &str,
    ) -> Option<(Name, &PropertyReflection)> {
        let owner = self.ancestor_property_owner(class, prop)?;
        let sig = self
            .class_ids
            .get(&owner)
            .and_then(|id| self.property_result_by_name(*id, prop))
            .or_else(|| {
                self.class(owner)?
                    .2
                    .properties
                    .get(prop)
                    .map(|property| &**property)
            })?;
        Some((owner.clone(), sig))
    }

    /// Real PHP forbids overriding a method declared `final` anywhere
    /// in the ancestor chain, no matter how many intervening subclasses
    /// sit between the `final` declaration and this one. Walks only
    /// `extends` - an interface's methods have no bodies to protect.
    /// Trait methods are present in the consuming class's member entries,
    /// so a child sees the trait's final flag on that class. Returns the name of the ancestor
    /// that declared it final, plus whether that was via `@final`
    /// (docblock, PHPStan-only convention, real PHP doesn't enforce it)
    /// rather than the real keyword - checked together, nearest-ancestor
    /// first, real keyword winning over `@final` when a single ancestor
    /// has both (mirrors real PHPStan's own `OverridingMethodRule`
    /// if/elseif order: `method.parentMethodFinal` vs.
    /// `method.parentMethodFinalByPhpDoc`).
    pub fn find_final_ancestor_method(&self, class: &str, method: &str) -> Option<(Name, bool)> {
        let mut seen = FxHashSet::default();
        let mut stack: Vec<Name> = self.class_references_by_name(class)?.extends.to_vec();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(_) = self.class(&current) else {
                continue;
            };
            if self.method_is_final(&current, method) {
                return Some((current, false));
            }
            if self.method_is_final_by_phpdoc(&current, method) {
                return Some((current, true));
            }
            if let Some(references) = self.class_references_by_name(&current) {
                stack.extend(references.extends.iter().cloned());
            }
        }
        None
    }

    /// `find_final_ancestor_method`'s property-side
    /// mirror - PHP 8.4's own `final` property modifier. Walks only
    /// `extends`, same reasoning as the method version (properties have
    /// no `implements`/interface side at all. See that function's own doc comment
    /// for the `@final`/keyword priority the returned `bool` encodes.
    pub fn find_final_ancestor_property(
        &self,
        class: &str,
        property: &str,
    ) -> Option<(Name, bool)> {
        let mut seen = FxHashSet::default();
        let mut stack: Vec<Name> = self.class_references_by_name(class)?.extends.to_vec();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some((_, _, current_info)) = self.class(&current) else {
                continue;
            };
            let property_result = self
                .class_ids
                .get(&current)
                .and_then(|id| self.property_result_by_name(*id, property));
            if property_result.is_some_and(|property| property.is_final())
                || current_info.is_final_property(property)
            {
                return Some((current, false));
            }
            if property_result.is_some_and(|property| property.is_final_by_phpdoc())
                || current_info.is_final_property_by_phpdoc(property)
            {
                return Some((current, true));
            }
            if let Some(references) = self.class_references_by_name(&current) {
                stack.extend(references.extends.iter().cloned());
            }
        }
        None
    }

    /// Every interface (used by `MethodThrowTypeCovarianceRule`)
    /// anywhere in `class`'s own transitive ancestry (its own `implements`,
    /// any ancestor class's own `implements`, and any of those
    /// interfaces' own `extends` chain) that itself declares `method` -
    /// mirrors real PHPStan's own `ClassReflection::getInterfaces()`
    /// closely enough for this purpose (a plain BFS, not attempting its
    /// full internal dedup/priority machinery).
    pub(crate) fn all_interfaces_declaring_method(
        &self,
        class: &str,
        method: &str,
    ) -> Vec<(Name, &FunctionReflection)> {
        let mut seen_classes = FxHashSet::default();
        let mut class_queue: VecDeque<Name> = VecDeque::new();
        class_queue.push_back(Name::from(class));
        let mut seen_ifaces = FxHashSet::default();
        let mut result = Vec::new();
        while let Some(c) = class_queue.pop_front() {
            if !seen_classes.insert(c.clone()) {
                continue;
            }
            let Some(_) = self.class(&c) else {
                continue;
            };
            let Some(references) = self.class_references_by_name(&c) else {
                continue;
            };
            class_queue.extend(references.extends.iter().cloned());
            let mut iface_queue: VecDeque<Name> = references.implements.iter().cloned().collect();
            while let Some(iface) = iface_queue.pop_front() {
                if !seen_ifaces.insert(iface.clone()) {
                    continue;
                }
                let Some(_) = self.class(&iface) else {
                    continue;
                };
                if let Some(sig) = self.lookup_own_method(&iface, method) {
                    result.push((iface.clone(), sig));
                }
                if let Some(references) = self.class_references_by_name(&iface) {
                    iface_queue.extend(references.extends.iter().cloned());
                }
            }
        }
        result
    }

    /// Every method name `class` must provide a concrete (non-abstract)
    /// implementation of somewhere in its own hierarchy: every abstract
    /// method declared by an ancestor class, every method declared by an
    /// ancestor interface (interface methods have no body at all, so
    /// every one of them is implicitly abstract - real PHP doesn't
    /// require/allow the `abstract` keyword there), and every abstract
    /// method declared by a trait any of those classes `use`s -
    /// `flatten_traits` already folds a trait's own abstract method
    /// entries into each using class (unless the using class supplies
    /// its own concrete override), so a plain walk of method entries
    /// here already covers it, with no separate trait-specific case
    /// needed.
    pub fn required_abstract_methods(&self, class: &str) -> Vec<Name> {
        let mut required = FxHashSet::default();
        let mut seen = FxHashSet::default();
        let mut stack = vec![Name::from(class)];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some((_, _, info)) = self.class(&current) else {
                continue;
            };
            if info.kind == ClassKind::Interface {
                // A purely magic `@method` tag (see `FunctionReflection::is_magic`)
                // documents a capability, not a requirement - real PHP
                // has no way to express "this interface method is
                // optional," so an interface's real, textually-declared
                // methods are still all unconditionally required.
                required.extend(self.member_entries(&current).filter_map(|entry| {
                    self.lookup_own_method(&current, &entry.name)
                        .filter(|sig| !sig.is_magic)
                        .map(|_| entry.name.clone())
                }));
            } else {
                required.extend(
                    self.member_entries(&current)
                        .filter(|entry| entry.is_abstract)
                        .map(|entry| entry.name.clone()),
                );
            }
            if let Some(references) = self.class_references_by_name(&current) {
                for parent in references.extends.iter().chain(references.implements) {
                    stack.push(parent.clone());
                }
            }
        }
        let mut required: Vec<Name> = required.into_iter().collect();
        required.sort();
        required
    }

    /// A required method (see `required_abstract_methods`) counts as
    /// implemented only if *some* reachable ancestor provides a concrete
    /// one, not necessarily the *closest* ancestor with any entry.
    /// Deliberately doesn't reuse `lookup_method_with_owner`'s
    /// closest-match BFS: a class can `implements Throwable` directly
    /// while its real concrete `getCode()` etc. only live two hops
    /// further up an `extends` chain (`BaseException` -> real
    /// `Exception`) - the closest node with any entry is the
    /// interface's own bodyless declaration, even though a real
    /// implementation is reachable elsewhere.
    pub fn has_concrete_implementation(&self, class: &str, method: &str) -> bool {
        let mut seen = FxHashSet::default();
        let mut stack = vec![Name::from(class)];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some((_, _, info)) = self.class(&current) else {
                continue;
            };
            if info.kind != ClassKind::Interface
                && self.lookup_own_method(&current, method).is_some()
                && !self.method_is_abstract(&current, method)
            {
                return true;
            }
            if let Some(references) = self.class_references_by_name(&current) {
                for parent in references.extends.iter().chain(references.implements) {
                    stack.push(parent.clone());
                }
            }
        }
        false
    }

    /// Looks up a property, walking the ancestor graph if not declared
    /// directly on `class` (covers inherited properties, including ones
    /// introduced via constructor promotion). A single two-level
    /// `HashMap` lookup against `self.property_owner_by_class` - see
    /// `precompute_member_owners`'s own doc comment.
    pub fn lookup_property<K: TableKey + ?Sized>(
        &self,
        class: &K,
        prop: &str,
    ) -> Option<&PropertyReflection> {
        let owner = self.property_owners(class)?.get(prop)?;
        let id = self.class_ids.get(owner)?;
        self.property_result_by_name(*id, prop)
            .or_else(|| self.class_by_id(*id)?.properties.get(prop).map(|p| &**p))
    }

    /// Iterates a class's declared properties, including composed and
    /// resolved entries held in the Registry result table. A result that
    /// replaces a declaration keeps its place in the declaration map; only
    /// result-only names are appended.
    pub fn property_names<'a>(&'a self, class: &str) -> impl Iterator<Item = &'a Name> + 'a {
        let declared = self.class(class).map(|(_, _, info)| &info.properties);
        let results = self.class_ids.get(class).copied().map(|id| {
            self.property_results_for_class(id)
                .filter_map(move |(property, _)| self.property_name_by_ref(property))
        });
        declared
            .into_iter()
            .flat_map(|properties| properties.keys())
            .chain(
                results.into_iter().flatten().filter(move |name| {
                    !declared.is_some_and(|declared| declared.contains_key(*name))
                }),
            )
    }

    /// Same as `lookup_property`, but also returns the declaring
    /// ancestor's name - class-level generic substitution needs it the
    /// same way `lookup_method_with_owner`'s own doc comment explains.
    pub fn lookup_property_with_owner<K: TableKey + ?Sized>(
        &self,
        class: &K,
        prop: &str,
    ) -> Option<(Name, &PropertyReflection)> {
        let owner = self.property_owners(class)?.get(prop)?;
        let id = self.class_ids.get(owner)?;
        let sig = self
            .property_result_by_name(*id, prop)
            .or_else(|| self.class_by_id(*id)?.properties.get(prop).map(|p| &**p))?;
        Some((owner.clone(), sig))
    }

    /// Looks up a class constant, walking the ancestor graph if not
    /// declared directly on `class` - mirrors `lookup_property`.
    pub fn lookup_const(&self, class: &str, const_name: &str) -> Option<&ConstantReflection> {
        let owner = self.const_owners(class)?.get(const_name)?;
        let result = self
            .class_ids
            .get(owner)
            .and_then(|id| self.class_constant_result_by_name(*id, const_name))
            .map(|constant| &**constant);
        result.or_else(|| self.class(owner)?.2.consts.get(const_name).map(|c| &**c))
    }

    /// Iterates a class's declared constants, including constants composed
    /// from traits and held as Registry results.
    pub fn constant_names<'a>(&'a self, class: &str) -> impl Iterator<Item = &'a Name> + 'a {
        let declared = self.class(class).map(|(_, _, info)| &info.consts);
        let results = self.class_ids.get(class).copied();
        declared
            .into_iter()
            .flat_map(|constants| constants.keys())
            .chain(results.into_iter().flat_map(move |id| {
                self.class_constant_results_for_class(id)
                    .map(|(_, name, _)| name)
                    .filter(move |name| {
                        !declared.is_some_and(|declared| declared.contains_key(*name))
                    })
            }))
    }

    /// Looks up a class constant and returns its stable slot in the
    /// declaring class, walking the ancestor graph if needed. The slot is
    /// suitable for ID-addressed pass results while the declaration stays
    /// in insertion order.
    pub fn lookup_const_with_owner_and_slot(
        &self,
        class: &str,
        const_name: &str,
    ) -> Option<(Name, usize, &ConstantReflection)> {
        let owner = self.const_owners(class)?.get(const_name)?;
        let id = self.class_ids.get(owner).copied()?;
        let info = self.class_by_id(id)?;
        let result_slot = self.class_constant_slot_by_name(id, const_name);
        let result = result_slot.and_then(|slot| self.class_constant_results.get(&(id, slot)));
        let (slot, declared) = result
            .map(|constant| {
                (
                    result_slot.expect("constant result has a slot"),
                    &**constant,
                )
            })
            .or_else(|| {
                let slot = info.consts.get_index(const_name)?;
                Some((slot, &**info.consts.get_index_value(slot)?))
            })?;
        let constant = declared;
        Some((owner.clone(), slot, constant))
    }

    /// Looks up a constant's effective value type and its declaring class.
    /// A resolved ID-and-slot result wins when available; the reflection's
    /// type remains the fallback for raw declarations and older cache data.
    pub fn lookup_const_type_with_owner(
        &self,
        class: &str,
        const_name: &str,
    ) -> Option<(Name, &Type)> {
        let (owner, slot, constant) = self.lookup_const_with_owner_and_slot(class, const_name)?;
        let resolved = self
            .class_ids
            .get(&owner)
            .and_then(|id| self.class_const_type_result(*id, slot));
        Some((owner, resolved.unwrap_or_else(|| constant.ty())))
    }

    /// Looks up a constant's effective value type.
    pub fn lookup_const_type(&self, class: &str, const_name: &str) -> Option<&Type> {
        self.lookup_const_type_with_owner(class, const_name)
            .map(|(_, ty)| ty)
    }

    /// Same as `lookup_const`, but also returns the declaring ancestor's
    /// name - the `@internal`-usage check needs it to know *which*
    /// class's own `@internal` tag (if any) actually governs a
    /// `Class::CONST` access, the same way
    /// `lookup_method_with_owner`/`lookup_property_with_owner` already
    /// do for their own members.
    pub fn lookup_const_with_owner(
        &self,
        class: &str,
        const_name: &str,
    ) -> Option<(Name, &ConstantReflection)> {
        let owner = self.const_owners(class)?.get(const_name)?;
        let constant = self
            .class_ids
            .get(owner)
            .and_then(|id| self.class_constant_result_by_name(*id, const_name))
            .map(|constant| &**constant)
            .or_else(|| self.class(owner)?.2.consts.get(const_name).map(|c| &**c))?;
        Some((owner.clone(), constant))
    }

    /// A backed enum case's own literal backing value, if this
    /// checker was able to prove one (see `ClassReflection::enum_case_values`'s
    /// own doc comment) - not hierarchy-walked, since an enum can't
    /// extend anything for a case to inherit from in the first place.
    pub fn enum_case_value(&self, class: &str, case: &str) -> Option<&Type> {
        self.class(class)?.2.enum_case_values.get(case)
    }

    /// The matching case (if any, and if provable) for a
    /// literal argument passed to a backed enum's own `::from()`/
    /// `::tryFrom()` - a reverse lookup over `enum_case_value` above,
    /// since PHP itself only stores value -> case at runtime, never the
    /// other way around.
    pub fn enum_case_for_value(&self, class: &str, value: &Type) -> Option<&str> {
        let (_, _, info) = self.class(class)?;
        info.enum_case_values
            .iter()
            .find(|(_, v)| *v == value)
            .map(|(name, _)| name.as_ref())
    }

    /// Whether `name` is a function a stub declares (a PHP builtin or
    /// extension function) and no analysed code redeclares.
    pub(crate) fn is_builtin_function(&self, name: &str) -> bool {
        self.stub_provided_functions
            .contains(super::function_key(name).as_ref())
    }

    /// Whether `class` is a class a stub declares (a PHP builtin or
    /// extension class).
    pub(crate) fn is_builtin_class<K: TableKey + ?Sized>(&self, class: &K) -> bool {
        self.built_in_stub_classes.contains_key(class)
    }

    /// The function `name` names, compared case-insensitively as PHP does.
    pub fn lookup_function<K: TableKey + ?Sized>(&self, name: &K) -> Option<&FunctionReflection> {
        self.function(name).map(|(_, _, declaration)| declaration)
    }

    /// The function a call to `name` reaches, with the name it's declared
    /// under: the namespaced candidate when that's declared, else the
    /// global one PHP falls back to.
    pub fn resolve_function<'n>(
        &self,
        name: &'n syntax::ast::QualifiedName,
    ) -> Option<(&'n str, &FunctionReflection)> {
        let (primary, fallback) = name.candidates();
        self.lookup_function(primary)
            .map(|sig| (primary, sig))
            .or_else(|| fallback.and_then(|g| self.lookup_function(g).map(|sig| (g, sig))))
    }

    /// The name of the function a call to `name` reaches: the declared
    /// candidate, else the global name PHP falls back to for an
    /// unqualified name in a namespace.
    pub fn called_function_name<'n>(&self, name: &'n syntax::ast::QualifiedName) -> &'n str {
        match self.resolve_function(name) {
            Some((resolved, _)) => resolved,
            None => {
                let (primary, fallback) = name.candidates();
                fallback.unwrap_or(primary)
            }
        }
    }

    /// `lookup_function`'s canonical-name-returning sibling, mirroring
    /// `lookup_class`'s `(&str, &Info)` shape - `FunctionReflection` carries no
    /// `name` field, so `check_function_name_case`'s
    /// `function.nameCase` diagnostic needs this to report the real
    /// declared casing rather than just presence.
    pub(crate) fn lookup_function_with_name(
        &self,
        name: &str,
    ) -> Option<(&str, &FunctionReflection)> {
        self.function(name)
            .map(|(_, declared, sig)| (declared.as_str(), sig))
    }

    /// `lookup_function`'s class-side mirror - real PHP resolves
    /// class/interface/trait/enum names case-insensitively too. Returns
    /// the *canonical* declared key alongside the info so a caller can
    /// rebind its own local variable to it (a fresh-per-check-time
    /// lookup, not a structural registry field - see
    /// `canonicalize_class_name_case` for why those two need different
    /// treatment).
    pub fn lookup_class(&self, name: &str) -> Option<(&str, &ClassReflection)> {
        self.class(name)
            .map(|(_, declared, info)| (declared.as_str(), info))
    }

    /// Whether a native intersection type's own members (`Foo&Bar&...`)
    /// could ever be satisfied by some real value - real PHPStan's own
    /// `UnresolvableTypeHelper` reference semantics (`FunctionDefinitionCheck`'s
    /// `parameter.unresolvableNativeType`/`return.unresolvableNativeType`).
    /// Checks every pair; the whole intersection is impossible the
    /// moment any one pair is. Deliberately conservative, matching this
    /// project's own "don't know, don't guess" precedent everywhere
    /// else: an unrelated pair where either side isn't a class this
    /// registry actually knows is never treated as impossible (a real
    /// but unregistered class - a PHAR-bundled dependency, say - could
    /// always turn out to satisfy both).
    pub(crate) fn is_intersection_possible(&self, members: &[String]) -> bool {
        for (i, a) in members.iter().enumerate() {
            for b in &members[i + 1..] {
                if !self.pair_possible_in_intersection(a, b) {
                    return false;
                }
            }
        }
        true
    }

    /// One pair's own half of `is_intersection_possible`. Two rules,
    /// both grounded in real PHP single-inheritance semantics rather
    /// than guessed at: (1) two distinct, registered `ClassKind::Class`
    /// members with no `extends`/`implements` relationship between them
    /// can never coexist - a real object has exactly one concrete class
    /// identity, so two unrelated concrete classes are never satisfiable
    /// together regardless of `final`-ness; (2) a `final` class paired
    /// with an interface it doesn't itself implement is also impossible -
    /// `final` forecloses the one remaining way that combination could
    /// ever become true (a not-yet-written subclass implementing the
    /// interface too). Every other combination (two interfaces; a
    /// non-final class with an interface it doesn't yet implement; one
    /// side genuinely unresolvable) stays "possible" - a real subclass,
    /// or a class this registry simply doesn't know about, could always
    /// satisfy it.
    fn pair_possible_in_intersection(&self, a: &str, b: &str) -> bool {
        if a.eq_ignore_ascii_case(b) || self.is_subclass_of(a, b) || self.is_subclass_of(b, a) {
            return true;
        }
        let Some((_, _, a_info)) = self.class(a) else {
            return true;
        };
        let Some((_, _, b_info)) = self.class(b) else {
            return true;
        };
        use syntax::ast::ClassKind::{Class, Interface};
        match (a_info.kind, b_info.kind) {
            (Class, Class) => false,
            (Class, Interface) => !a_info.is_final,
            (Interface, Class) => !b_info.is_final,
            _ => true,
        }
    }
}

syntax::canonical_struct!(ClassOwners {
    methods,
    properties,
    consts,
});
