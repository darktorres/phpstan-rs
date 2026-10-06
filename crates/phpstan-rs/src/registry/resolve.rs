use crate::registry::NameTable;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::{Arc, OnceLock};
use syntax::name::{FoldedMap, Name};
use syntax::name::{NameMap, NameSet, OrderedNameMap};

/// Each templated ancestor's template bindings, as seen from one class.
type AncestorBindings = NameMap<FxHashMap<String, Type>>;
type DeferredClassConstantTypes = Vec<(usize, Type)>;
type ClassPropertyResults = Vec<(Name, Arc<PropertyReflection>)>;
type ClassMethodResults = Vec<(Name, FunctionReflection)>;
type ClassConstantResults = OrderedNameMap<(usize, Arc<ConstantReflection>)>;
type ResolvedClassUpdate = (
    ClassId,
    ClassReflection,
    ClassMethodResults,
    DeferredClassConstantTypes,
    ClassPropertyResults,
);
type VisibleClassConstTypes = OrderedNameMap<Type>;

use crate::registry::Registry;
use crate::registry::{
    ClassId, ClassReflection, ConstantReflection, FnId, FunctionReflection, Members, MethodEntry,
    MethodRef, PropertyReflection, substitute_type_aliases,
};
use syntax::ast::{ClassKind, Visibility};
use types::{self, Type};

impl Registry {
    /// PHP resolves class/interface/trait/enum names case-insensitively
    /// (the same rationale `Registry::lookup_function`'s doc comment
    /// gives for functions), but every internal registry graph-walk
    /// (`flatten_traits`, `flatten_phpdoc_inheritance`, the ancestor/
    /// interface-satisfaction walks in `registry/hierarchy.rs`, ...) does
    /// a plain, exact-case `self.classes.get(name)`. Rather than thread a
    /// case-insensitive fallback through every one of those call sites,
    /// this runs once, right after indexing finishes and before any of
    /// those graph-walks - a single O(classes) rewrite pass that
    /// canonicalizes every reference-shaped field on each `ClassReflection`
    /// (`extends`, `implements`, `uses`, `mixins`, each
    /// `TraitAlias::trait_name`, each `ResolvedTypeImport::
    /// source_class_fqn`) to the real declared key's own case, wherever
    /// the differently-cased reference resolves at all. A reference that
    /// still doesn't resolve after lowercasing is left untouched - a
    /// genuine typo, which the existing `class.notFound`-family checks
    /// already catch; this function only ever narrows what "resolves"
    /// means, never widens it. Every internal `self.classes.
    /// get(&current)`-style lookup stays a plain, allocation-free
    /// `HashMap` lookup, now simply correct because the stored string is
    /// always canonical. Building `canonical_by_lower` stays a single
    /// sequential pass - only the per-class rewrite below is
    /// parallelized, since it's the part whose cost actually scales with
    /// how many reference-shaped fields each class has. Each class's own
    /// rewrite touches only its own `ClassReflection` and reads only the
    /// immutable `declared_names` view of the classes as they were when the pass began - no
    /// other class is ever read or written, so `values_mut()` directly
    /// (no remove/reinsert dance) is sound to run concurrently.
    #[hotpath::measure]
    pub(crate) fn canonicalize_class_name_case(&mut self) {
        let declared = self.classes.declared_names();
        let canonicalize = |name: &mut Name| {
            if let Some(canonical) = declared.get(name)
                && canonical.as_ref() != name.as_ref()
            {
                *name = canonical.clone();
            }
        };
        let differs = |name: &Name| {
            declared
                .get(name)
                .is_some_and(|canonical| canonical.as_ref() != name.as_ref())
        };
        let names_a_differing_case = |info: &ClassReflection| {
            info.extends
                .iter()
                .chain(&info.implements)
                .chain(&info.uses)
                .chain(&info.mixins)
                .any(differs)
                || info
                    .trait_aliases
                    .iter()
                    .filter_map(|alias| alias.trait_name.as_ref())
                    .any(differs)
                || info
                    .type_imports
                    .iter()
                    .any(|import| differs(&import.source_class_fqn))
        };
        let scope = &self.scope;
        self.classes
            .iter_mut_scoped_where(scope.class_keys(), names_a_differing_case)
            .map(|(_, info)| info)
            .collect::<Vec<_>>()
            .into_par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|info| {
                for name in info
                    .extends
                    .iter_mut()
                    .chain(info.implements.iter_mut())
                    .chain(info.uses.iter_mut())
                    .chain(info.mixins.iter_mut())
                {
                    canonicalize(name);
                }
                for alias in &mut info.trait_aliases {
                    if let Some(trait_name) = &mut alias.trait_name {
                        canonicalize(trait_name);
                    }
                }
                for import in &mut info.type_imports {
                    canonicalize(&mut import.source_class_fqn);
                }
            });
    }

    /// A `use TraitName;` can reference a trait declared later in the same
    /// file (PHP resolves this at compile time, not top-to-bottom), so
    /// flattening has to be a second pass over the fully-indexed classes -
    /// a using class's own methods/properties win over a trait's.
    ///
    /// `pub(crate)` (not private) so `Project::update_file` can re-run
    /// just this pass after patching one file's contributions in place,
    /// without needing to go through `build`'s full from-scratch `index`.
    /// A trait that itself declares `use OtherTrait;` needs
    /// `OtherTrait`'s methods folded into it before a class using the
    /// first trait is processed - a single non-iterative pass only
    /// composes one level deep. Runs `flatten_traits_one_pass` to a fixed
    /// point (each pass may surface a new one-level composition a later
    /// pass can now see, e.g. a trait-of-a-trait-of-a-trait chain),
    /// bounded by the number of classes/traits registered, since a real
    /// trait-composition cycle is itself a PHP fatal error this project
    /// doesn't otherwise detect, so degrading to "leave as partially
    /// flattened" once the bound is hit is the honest answer.
    #[hotpath::measure]
    pub(crate) fn flatten_traits(&mut self) {
        let max_iterations = self.scoped_class_names().len().max(1);
        for _ in 0..max_iterations {
            if !self.flatten_traits_one_pass() {
                break;
            }
        }
    }

    /// One level of trait composition - see `flatten_traits`'s doc
    /// comment for why this needs to run more than once. Returns whether
    /// anything was actually added anywhere, so the caller knows when a
    /// fixed point has been reached.
    ///
    /// The collection loop below only ever *reads* `self.classes` (its
    /// own class, and whichever trait(s) it uses) - no write happens
    /// until the sequential apply loop afterward, so it parallelizes by
    /// reborrowing `self` as a shared `&Registry` for the duration of the
    /// parallel `filter_map` (ends before the apply loop takes `&mut
    /// self` again). Method entries reuse their shared lazy signature
    /// handles; property results reuse their reflection Arcs and constants
    /// reuse their reflection Arcs.
    fn flatten_traits_one_pass(&mut self) -> bool {
        type TraitAddition = (
            ClassId,
            Vec<(MethodRef, MethodEntry, Arc<super::Lazy<FunctionReflection>>)>,
            Vec<(Name, Arc<PropertyReflection>)>,
            Vec<(usize, Name, Arc<ConstantReflection>)>,
        );

        let class_names = self.scoped_class_names();
        let registry: &Registry = self;
        let additions: Vec<TraitAddition> = class_names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|class_name| {
                let class_id = *registry.class_ids.get(class_name)?;
                let own = &registry.classes()[class_name];
                let (uses, trait_aliases) = (&own.uses, &own.trait_aliases);
                if uses.is_empty() && trait_aliases.is_empty() {
                    return None;
                }
                // A trait can declare its own `abstract` method (the using
                // class is required to supply it) - a using class's own
                // directly-declared method for that same name (checked here,
                // before any trait's contribution is applied) always wins,
                // matching the same `or_insert` "first/own wins" semantics
                // `methods` itself already uses below. What the class has
                // of its own is not copied out either: the apply loop only
                // inserts into a vacant slot, so a member the class
                // declares is left out here instead of cloned and dropped.
                let own_method_names = &own.member_slots;
                let mut methods = Vec::new();
                let mut added_method_names = FoldedMap::default();
                let mut next_method_slot = own.member_order.len();
                let mut properties = Vec::new();
                let mut added_property_names = NameSet::default();
                let mut consts = Vec::new();
                let mut added_const_names = NameSet::default();
                let existing_const_results = registry.class_constant_results.get(&class_id);
                let mut next_const_slot =
                    own.consts.len() + existing_const_results.map_or(0, |results| results.len());
                for trait_name in uses {
                    if let Some(trait_info) = registry.classes().get(trait_name) {
                        for entry in &trait_info.member_order {
                            let name = &entry.name;
                            let Some(shared_sig) = registry.shared_own_method(trait_name, name)
                            else {
                                continue;
                            };
                            // PHP exposes an adapted alias immediately
                            // before the source method in the composed
                            // method list (ReflectionClass::getMethods).
                            for alias in trait_aliases.iter().filter(|alias| alias.method == *name)
                            {
                                let selected_trait = alias.trait_name.as_ref().or_else(|| {
                                    uses.iter().find(|candidate| {
                                        registry.classes().get(candidate).is_some_and(|info| {
                                            info.member_slots.contains_key(&alias.method)
                                        })
                                    })
                                });
                                if selected_trait == Some(trait_name)
                                    && !own_method_names.contains_key(&alias.alias)
                                    && !added_method_names.contains_key(&alias.alias)
                                {
                                    let slot = u16::try_from(next_method_slot)
                                        .expect("class method slot space exhausted");
                                    next_method_slot += 1;
                                    added_method_names.insert(alias.alias.clone(), ());
                                    methods.push((
                                        MethodRef::new(class_id, slot),
                                        MethodEntry {
                                            name: alias.alias.clone(),
                                            is_abstract: entry.is_abstract,
                                            is_final: entry.is_final,
                                            is_static: entry.is_static,
                                            is_final_by_phpdoc: entry.is_final_by_phpdoc,
                                        },
                                        Arc::clone(&shared_sig),
                                    ));
                                }
                            }
                            if own_method_names.contains_key(name)
                                || added_method_names.contains_key(name)
                            {
                                continue;
                            }
                            let slot = u16::try_from(next_method_slot)
                                .expect("class method slot space exhausted");
                            next_method_slot += 1;
                            added_method_names.insert(name.clone(), ());
                            methods.push((
                                MethodRef::new(class_id, slot),
                                MethodEntry {
                                    name: name.clone(),
                                    is_abstract: entry.is_abstract,
                                    is_final: entry.is_final,
                                    is_static: entry.is_static,
                                    is_final_by_phpdoc: entry.is_final_by_phpdoc,
                                },
                                shared_sig,
                            ));
                        }
                        for (name, declared) in &trait_info.properties {
                            if own.properties.contains_key(name)
                                || registry
                                    .class_property_results
                                    .get(&class_id)
                                    .is_some_and(|results| results.contains_key(name))
                                || !added_property_names.insert(name.clone())
                            {
                                continue;
                            }
                            let trait_id = registry.class_ids.get(trait_name).copied();
                            let property = trait_id
                                .and_then(|id| registry.class_property_results.get(&id))
                                .and_then(|results| results.get(name))
                                .unwrap_or(declared);
                            properties.push((name.clone(), Arc::clone(property)));
                        }
                        if let Some(trait_id) = registry.class_ids.get(trait_name)
                            && let Some(results) = registry.class_property_results.get(trait_id)
                        {
                            for (name, property) in results {
                                if trait_info.properties.contains_key(name)
                                    || own.properties.contains_key(name)
                                    || registry
                                        .class_property_results
                                        .get(&class_id)
                                        .is_some_and(|own_results| own_results.contains_key(name))
                                    || !added_property_names.insert(name.clone())
                                {
                                    continue;
                                }
                                properties.push((name.clone(), Arc::clone(property)));
                            }
                        }
                        // PHP 8.2+ trait constants - composed into the
                        // using class exactly like a trait's methods/
                        // properties already are just above: a `self::`
                        // reference inside a trait-defined method's body
                        // resolves against whichever class actually
                        // composes the trait (`current_class` is set to
                        // the host in `check_trait_methods`, once per
                        // host), so the constant needs to actually be
                        // reachable through that host's Registry result
                        // table and owner index for `lookup_const`/
                        // `lookup_const_with_owner` - without this, a
                        // trait's own `self::CONST` reference from
                        // inside its own body silently read as
                        // `classConstant.notFound` against every one of
                        // its hosts, since the constant was never
                        // anywhere but the trait's own, otherwise-
                        // unreachable registry entry.
                        for (name, declared) in &trait_info.consts {
                            if own.consts.contains_key(name)
                                || existing_const_results
                                    .is_some_and(|results| results.contains_key(name))
                                || !added_const_names.insert(name.clone())
                            {
                                continue;
                            }
                            let trait_id = registry.class_ids.get(trait_name).copied();
                            let constant = trait_id
                                .and_then(|id| registry.class_constant_results.get(&id))
                                .and_then(|results| results.get(name))
                                .map_or_else(
                                    || Arc::clone(declared),
                                    |(_, result)| Arc::clone(result),
                                );
                            consts.push((next_const_slot, name.clone(), constant));
                            next_const_slot += 1;
                        }
                        if let Some(trait_id) = registry.class_ids.get(trait_name)
                            && let Some(results) = registry.class_constant_results.get(trait_id)
                        {
                            for (name, (_, constant)) in results {
                                if trait_info.consts.contains_key(name)
                                    || own.consts.contains_key(name)
                                    || existing_const_results
                                        .is_some_and(|own_results| own_results.contains_key(name))
                                    || !added_const_names.insert(name.clone())
                                {
                                    continue;
                                }
                                consts.push((next_const_slot, name.clone(), Arc::clone(constant)));
                                next_const_slot += 1;
                            }
                        }
                    }
                }
                Some((class_id, methods, properties, consts))
            })
            .collect();

        let mut changed = false;
        for (class_id, methods, properties, consts) in additions {
            let Some(class_name) = self.class_name_by_id(class_id).cloned() else {
                continue;
            };
            if let Some(info) = self.classes.get_mut(&class_name) {
                for (method_ref, entry, sig) in methods {
                    debug_assert_eq!(method_ref.class(), class_id);
                    if !info.member_slots.contains_key(&entry.name) {
                        let name = entry.name.clone();
                        let inserted = info.register_member_entry(entry);
                        debug_assert!(inserted);
                        debug_assert_eq!(info.member_slots.get(&name), Some(&method_ref.slot()));
                        let methods = self.class_method_results.entry(class_id).or_default();
                        let inserted = methods.insert_shared_if_absent(name, sig);
                        debug_assert!(inserted);
                        changed = true;
                    }
                }
            }
            let own_properties = self.class_property_results.entry(class_id).or_default();
            for (name, property) in properties {
                if own_properties.contains_key(&name)
                    || self
                        .classes
                        .get(&class_name)
                        .is_some_and(|info| info.properties.contains_key(&name))
                {
                    continue;
                }
                own_properties.insert(name, property);
                changed = true;
            }
            let own_constants = self.class_constant_results.entry(class_id).or_default();
            for (slot, name, constant) in consts {
                if own_constants.contains_key(&name)
                    || self
                        .classes
                        .get(&class_name)
                        .is_some_and(|info| info.consts.contains_key(&name))
                {
                    continue;
                }
                own_constants.insert(name, (slot, constant));
                changed = true;
            }
        }
        changed
    }

    /// `@method`'s deferred insertion - `ClassReflection::method_tags`'s
    /// construction site (`from_decl`) has the full "why this can't just
    /// insert eagerly" explanation: a same-named real trait method hasn't
    /// been flattened in yet at that point, and both insertions are
    /// vacant-only, so inserting first would let the tag win by accident.
    /// Runs once, right after `flatten_traits` reaches its own fixed
    /// point (every real method, own or trait-derived, already settled)
    /// and before `flatten_phpdoc_inheritance` (irrelevant either way - a
    /// synthesized signature is always fully resolved already, never a
    /// backfill candidate) - no fixed-point loop needed, since a class's
    /// `method_tags` list is fixed the moment `from_decl` built it.
    ///
    /// Fully self-contained per class (`info.method_tags` and its member
    /// slots); signatures are returned by class id into the method result
    /// table after the parallel collection.
    #[hotpath::measure]
    pub(crate) fn resolve_method_tags(&mut self) {
        let scope = &self.scope;
        let resolved: Vec<(ClassId, Vec<(Name, FunctionReflection)>)> = self
            .classes
            .iter_mut_scoped_where(scope.class_keys(), |info| !info.method_tags.is_empty())
            .collect::<Vec<_>>()
            .into_par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|(class_name, info)| {
                let class_id = *self.class_ids.get(class_name)?;
                let mut methods = Vec::new();
                for (name, sig, is_static) in std::mem::take(&mut info.method_tags) {
                    if !info.member_slots.contains_key(&name) {
                        info.register_member_entry(MethodEntry {
                            name: name.clone(),
                            is_abstract: false,
                            is_final: false,
                            is_static,
                            is_final_by_phpdoc: false,
                        });
                        methods.push((name, sig));
                    }
                }
                Some((class_id, methods))
            })
            .collect();
        for (class_id, methods) in resolved {
            let results = self.class_method_results.entry(class_id).or_default();
            for (name, signature) in methods {
                results.insert(name, signature);
            }
        }
    }

    /// Mirrors real PHPStan's own backfill of a method's missing
    /// `@param`/`@return` from its parent class (first) or first-
    /// declared implemented interface, whenever the child's docblock
    /// lacks a tag for that position at all - unconditional and unrelated
    /// to whether the resolved type happens to be `Mixed`. Reuses
    /// `lookup_ancestor_method_with_owner`'s already-correct parent-then-
    /// implements, first-hit-wins order. Runs after `flatten_traits` (so
    /// a trait-derived method already looks like the class's own), to a
    /// fixed point exactly like `flatten_traits` itself: a multi-level
    /// chain where each level textually redeclares the method without
    /// its own docblock needs more than one pass to fully resolve.
    ///
    /// Deliberately narrow scope: methods only, no properties (real PHP
    /// interfaces can't declare them, so the existing
    /// `lookup_ancestor_property_with_owner`'s parent-only walk already
    /// covers the one real path); no trait-abstract-method fallback;
    /// ancestor conflicts aren't reconciled, first-match-wins (matching
    /// real PHPStan too); an ancestor's `private` method is never
    /// inherited from (a `private` method can't be overridden at all, so
    /// its docblock says nothing about what an unrelated same-named
    /// child method means).
    #[hotpath::measure]
    pub(crate) fn flatten_phpdoc_inheritance(&mut self) {
        // Each class's ancestor bindings, computed at most once for the
        // whole fixed point: they're built from its declared hierarchy and
        // template arguments, which the passes never change.
        let class_names = self.scoped_class_names();
        let max_iterations = class_names.len().max(1);
        let levels = self.hierarchy_levels(&class_names);
        let bindings: Vec<OnceLock<AncestorBindings>> =
            class_names.iter().map(|_| OnceLock::new()).collect();
        match levels {
            // What a class takes from its ancestors is read from their
            // signatures as they stand and written to its own alone, so
            // each level taken after the levels its ancestors are in
            // reaches the fixed point in one sweep.
            Some(levels) => {
                for level in &levels {
                    self.flatten_phpdoc_inheritance_one_pass(&class_names, level, &bindings);
                }
            }
            // A class that is its own ancestor has no first level: the
            // passes run until nothing changes.
            None => {
                let order: Vec<usize> = (0..class_names.len()).collect();
                for _ in 0..max_iterations {
                    if !self.flatten_phpdoc_inheritance_one_pass(&class_names, &order, &bindings) {
                        break;
                    }
                }
            }
        }
    }

    /// The positions in `class_names` grouped by the longest chain of
    /// ancestors among the classes each has, those with none first; `None`
    /// when a class is among its own ancestors. Parents outside
    /// `class_names` are not counted: they are not resolved again.
    fn hierarchy_levels(&self, class_names: &[Name]) -> Option<Vec<Vec<usize>>> {
        let scoped: FxHashSet<&str> = class_names.iter().map(|name| name.lower()).collect();
        let mut depth: FxHashMap<&str, usize> = FxHashMap::default();
        let mut visiting: FxHashSet<&str> = FxHashSet::default();
        // Explicit stack: a chain of ancestors can be as long as the
        // classes are many. An entry expanded holds the class it read.
        for start in class_names {
            let mut stack: Vec<(&Name, Option<&ClassReflection>)> = vec![(start, None)];
            while let Some((name, expanded)) = stack.pop() {
                let key = name.lower();
                if depth.contains_key(key) {
                    continue;
                }
                let info = match expanded {
                    Some(info) => info,
                    None => {
                        let Some(info) = self.classes.get(name) else {
                            depth.insert(key, 0);
                            continue;
                        };
                        info
                    }
                };
                let parents = || {
                    info.extends
                        .iter()
                        .chain(&info.implements)
                        .chain(&info.uses)
                        .filter(|parent| scoped.contains(parent.lower()))
                };
                if expanded.is_none() {
                    if !visiting.insert(key) {
                        return None;
                    }
                    stack.push((name, Some(info)));
                    for parent in parents() {
                        if visiting.contains(parent.lower()) {
                            return None;
                        }
                        if !depth.contains_key(parent.lower()) {
                            stack.push((parent, None));
                        }
                    }
                } else {
                    visiting.remove(key);
                    let level = parents()
                        .map(|parent| depth.get(parent.lower()).map_or(0, |d| d + 1))
                        .max()
                        .unwrap_or(0);
                    depth.insert(key, level);
                }
            }
        }
        let mut levels: Vec<Vec<usize>> = Vec::new();
        for (index, name) in class_names.iter().enumerate() {
            let level = depth.get(name.lower()).copied().unwrap_or(0);
            if levels.len() <= level {
                levels.resize_with(level + 1, Vec::new);
            }
            levels[level].push(index);
        }
        Some(levels)
    }

    /// A redeclared property inherits each hook of its parent's that it
    /// doesn't declare itself, and is virtual only when the parent's is
    /// and its own hooks never use the backing value.
    #[hotpath::measure]
    pub(crate) fn inherit_property_hooks(&mut self) {
        let class_names = self.scoped_class_names();
        let max_iterations = class_names.len().max(1);
        for _ in 0..max_iterations {
            if !self.inherit_property_hooks_one_pass() {
                break;
            }
        }
    }

    fn inherit_property_hooks_one_pass(&mut self) -> bool {
        let mut updates: Vec<(ClassId, Name, Arc<PropertyReflection>)> = Vec::new();
        for (class_name, info) in self.scoped_classes() {
            let Some(class_id) = self.class_ids.get(class_name).copied() else {
                continue;
            };
            let Some(parent) = info.extends.first() else {
                continue;
            };
            for name in self.property_names(class_name) {
                let Some(own) = self.lookup_property(class_name, name) else {
                    continue;
                };
                let Some((_, inherited)) = self.lookup_property_with_owner(parent, name) else {
                    continue;
                };
                if inherited.visibility == Visibility::Private {
                    continue;
                }
                let mut merged = PropertyReflection::clone(own);
                if !own.has_get_hook && inherited.has_get_hook {
                    merged.has_get_hook = true;
                }
                if !own.has_set_hook && inherited.has_set_hook {
                    merged.has_set_hook = true;
                    merged.set_ty = inherited.set_ty.clone();
                    merged.set_case_refs = inherited.set_case_refs.clone();
                }
                merged.is_virtual = own.is_virtual && inherited.is_virtual;
                if merged.has_get_hook != own.has_get_hook
                    || merged.has_set_hook != own.has_set_hook
                    || merged.is_virtual != own.is_virtual
                {
                    updates.push((class_id, name.clone(), Arc::new(merged)));
                }
            }
        }
        let changed = !updates.is_empty();
        for (class_id, name, merged) in updates {
            self.class_property_results
                .entry(class_id)
                .or_default()
                .insert(name, merged);
        }
        changed
    }

    /// Resolves every `value-of`/class constant reference (`types::
    /// parse_type_keyword_or_class`'s `value-of<T>`-on-a-class/enum, and
    /// `Foo::BAR` references) now that every class/enum is
    /// actually indexed - see that function's doc comment for why this
    /// can't happen at parse time. A single pass, not a fixed-point loop
    /// like `flatten_phpdoc_inheritance` above: a `value-of<value-of<X>>`-
    /// style chain isn't attempted.
    ///
    /// Each class is resolved in parallel against the classes as they are
    /// before the pass, and the results are inserted together, so no
    /// result depends on the order classes are visited in. (A class whose
    /// own docblock names `value-of<self>` on itself, where `self` is
    /// genuinely an enum, degrades to `Mixed` - the same "can't verify,
    /// don't guess" outcome every other unresolvable case here gets.)
    /// Top-level functions are resolved after, against the classes with
    /// theirs in.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_value_of(&mut self) {
        self.resolve_class_constant_references();
        let class_names = self.scoped_class_names();
        let registry: &Registry = self;
        let resolved: Vec<ResolvedClassUpdate> = class_names
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|name| {
                let id = *registry.class_ids.get(name)?;
                registry.resolved_deferred_class(name).map(
                    |(info, methods, constants, properties)| {
                        (id, info, methods, constants, properties)
                    },
                )
            })
            .collect();
        for (id, info, methods, constants, properties) in resolved {
            if let Some(name) = self.class_name_by_id(id).cloned() {
                for (slot, ty) in constants {
                    self.class_const_types.insert((id, slot), ty);
                }
                self.class_property_results
                    .entry(id)
                    .or_default()
                    .extend(properties);
                let method_results = self.class_method_results.entry(id).or_default();
                for (name, method) in methods {
                    method_results.insert(name, method);
                }
                self.classes.insert(name, info);
            }
        }
        let class_ids = &self.class_ids;
        let const_types = &self.class_const_types;
        let constant_results = &self.class_constant_results;
        let resolve = |ty: &mut Type, native: &Type, classes: &_| {
            if ty.contains_deferred() {
                let taken = std::mem::replace(ty, Type::mixed());
                *ty = resolve_deferred_value_of_with_native_fallback(
                    taken,
                    native,
                    classes,
                    class_ids,
                    const_types,
                    constant_results,
                    None,
                );
            }
        };
        let scope = &self.scope;
        for (_, sig) in self
            .functions
            .iter_mut_scoped_where(scope.function_keys(), function_has_deferred_type)
        {
            for param in &mut sig.params {
                resolve(&mut param.ty, &param.ty_native, &self.classes);
                let native = param.ty_native.clone();
                for extra in [
                    &mut param.doc_ty,
                    &mut param.out_ty,
                    &mut param.closure_this_type,
                ]
                .into_iter()
                .flatten()
                {
                    resolve(extra, &native, &self.classes);
                }
            }
            resolve(&mut sig.return_type, &sig.return_type_native, &self.classes);
            if let Some(doc_ty) = &mut sig.return_doc_ty {
                resolve(doc_ty, &sig.return_type_native, &self.classes);
            }
        }
    }

    /// `class_name`'s declaration with its deferred types resolved, when it
    /// has any, read against the classes as they are before the pass.
    fn resolved_deferred_class(
        &self,
        class_name: &Name,
    ) -> Option<(
        ClassReflection,
        ClassMethodResults,
        DeferredClassConstantTypes,
        ClassPropertyResults,
    )> {
        // Only a class with a deferred sentinel somewhere needs its
        // inherited-constant view (the overwhelming majority, stubs
        // included, have none).
        let class_id = *self.class_ids.get(class_name)?;
        let own_consts =
            match self.classes.get(class_name) {
                Some(info)
                    if class_has_deferred_type(info, self.class_method_results.get(&class_id))
                        || self.class_property_results.get(&class_id).is_some_and(
                            |properties| {
                                properties
                                    .iter()
                                    .any(|(_, prop)| property_has_deferred_type(prop))
                            },
                        )
                        || self.class_constant_results.get(&class_id).is_some_and(
                            |constants| {
                                constants
                                    .iter()
                                    .any(|(_, (_, constant))| constant.ty.contains_deferred())
                            },
                        ) =>
                {
                    visible_class_consts(
                        class_name,
                        &self.classes,
                        &self.class_ids,
                        &self.class_const_types,
                        &self.class_constant_results,
                    )
                }
                _ => return None,
            };
        let mut info = self.classes.get(class_name)?.clone();
        let class_ids = &self.class_ids;
        let const_types = &self.class_const_types;
        let constant_results = &self.class_constant_results;
        let resolve = |ty: &mut Type, native: &Type, classes: &_| {
            if ty.contains_deferred() {
                let bound = bind_class_const_self(
                    std::mem::replace(ty, Type::mixed()),
                    class_name,
                    &own_consts,
                );
                *ty = resolve_deferred_value_of_with_native_fallback(
                    bound,
                    native,
                    classes,
                    class_ids,
                    const_types,
                    constant_results,
                    Some(class_name),
                );
            }
        };
        let resolve_signature = |sig: &mut FunctionReflection| {
            for param in &mut sig.params {
                resolve(&mut param.ty, &param.ty_native, &self.classes);
                let native = param.ty_native.clone();
                for extra in [
                    &mut param.doc_ty,
                    &mut param.out_ty,
                    &mut param.closure_this_type,
                ]
                .into_iter()
                .flatten()
                {
                    resolve(extra, &native, &self.classes);
                }
            }
            resolve(&mut sig.return_type, &sig.return_type_native, &self.classes);
            if let Some(doc_ty) = &mut sig.return_doc_ty {
                resolve(doc_ty, &sig.return_type_native, &self.classes);
            }
            if let Some(out) = &mut sig.self_out {
                resolve(out, &Type::mixed(), &self.classes);
            }
        };
        let mut method_results = Vec::new();
        for name in info.methods.keys() {
            let current = info.methods.get(name).expect("method key exists");
            let mut resolved = current.clone();
            resolve_signature(&mut resolved);
            if resolved != *current {
                method_results.push((name.clone(), resolved));
            }
        }
        if let Some(results) = self.class_method_results.get(&class_id) {
            for (name, current) in results.iter() {
                let mut resolved = current.clone();
                resolve_signature(&mut resolved);
                if resolved != *current {
                    method_results.push((name.clone(), resolved));
                }
            }
        }
        let mut properties = Vec::new();
        for name in self.property_names(class_name) {
            let Some(current) = self
                .class_property_results
                .get(&class_id)
                .and_then(|results| results.get(name))
                .or_else(|| info.properties.get(name))
            else {
                continue;
            };
            if !property_has_deferred_type(current) {
                continue;
            }
            let mut prop = PropertyReflection::clone(current);
            if prop.ty.contains_deferred() {
                let ty = bind_class_const_self(
                    std::mem::replace(&mut prop.ty, Type::mixed()),
                    class_name,
                    &own_consts,
                );
                prop.ty = resolve_deferred_value_of_in_type(
                    ty,
                    &self.classes,
                    &self.class_ids,
                    &self.class_const_types,
                    &self.class_constant_results,
                    Some(class_name),
                );
            }
            if let Some(doc) = &mut prop.doc_ty
                && doc.contains_deferred()
            {
                let ty = bind_class_const_self(
                    std::mem::replace(doc, Type::mixed()),
                    class_name,
                    &own_consts,
                );
                *doc = resolve_deferred_value_of_in_type(
                    ty,
                    &self.classes,
                    &self.class_ids,
                    &self.class_const_types,
                    &self.class_constant_results,
                    Some(class_name),
                );
            }
            if let Some(set_ty) = &prop.set_ty
                && set_ty.contains_deferred()
            {
                let ty =
                    bind_class_const_self(prop.set_ty.take().unwrap(), class_name, &own_consts);
                prop.set_ty = Some(resolve_deferred_value_of_in_type(
                    ty,
                    &self.classes,
                    &self.class_ids,
                    &self.class_const_types,
                    &self.class_constant_results,
                    Some(class_name),
                ));
            }
            if prop != **current {
                properties.push((name.clone(), Arc::new(prop)));
            }
        }
        let constants = class_constant_entries(&info, self.class_constant_results.get(&class_id))
            .into_iter()
            .filter_map(|(slot, _, constant)| {
                let current = self
                    .class_const_types
                    .get(&(class_id, slot))
                    .unwrap_or(&constant.ty);
                if !current.contains_deferred() {
                    return None;
                }
                let bound = bind_class_const_self(current.clone(), class_name, &own_consts);
                let ty = resolve_deferred_value_of_in_type(
                    bound,
                    &self.classes,
                    &self.class_ids,
                    &self.class_const_types,
                    &self.class_constant_results,
                    Some(class_name),
                );
                Some((slot, ty))
            })
            .collect();
        for bound in info.template_bounds.values_mut() {
            if bound.contains_deferred() {
                let ty = bind_class_const_self(
                    std::mem::replace(bound, Type::mixed()),
                    class_name,
                    &own_consts,
                );
                *bound = resolve_deferred_value_of_in_type(
                    ty,
                    &self.classes,
                    &self.class_ids,
                    &self.class_const_types,
                    &self.class_constant_results,
                    Some(class_name),
                );
            }
        }
        // `@extends Lexer<self::T_*>`: what inherited PHPDoc binds to.
        let ancestor_args = info
            .extends_args
            .iter_mut()
            .chain(info.implements_args.values_mut().flatten());
        for arg in ancestor_args {
            if arg.contains_deferred() {
                let ty = bind_class_const_self(
                    std::mem::replace(arg, Type::mixed()),
                    class_name,
                    &own_consts,
                );
                *arg = resolve_deferred_value_of_in_type(
                    ty,
                    &self.classes,
                    &self.class_ids,
                    &self.class_const_types,
                    &self.class_constant_results,
                    Some(class_name),
                );
            }
        }
        Some((info, method_results, constants, properties))
    }

    /// Resolves every `ClassConstant` inside class constants'
    /// own types (`const B = self::A;`, `const ALL = [self::A,
    /// Other::B];` - `infer_class_const_type`'s output) before anything
    /// else reads them. Iterated to a fixpoint so a chain (`A = self::B`,
    /// `B = Other::C`, ...) resolves whatever its length, each round a
    /// read-only computation against the current map followed by an
    /// apply. A reference still unresolved once nothing changes, or after
    /// `MAX_ROUNDS` (a cycle, `A = self::B` / `B = self::A`, never stops
    /// changing), becomes `Mixed` - real PHP itself rejects such a cycle.
    fn resolve_class_constant_references(&mut self) {
        const MAX_ROUNDS: usize = 16;
        // The classes with a deferred constant type, found by the first
        // round: no later round gives another class one.
        let mut with_deferred: Option<Vec<ClassId>> = None;
        for _ in 0..MAX_ROUNDS {
            let mut updates: Vec<(ClassId, usize, Type)> = Vec::new();
            match &with_deferred {
                None => {
                    let mut found = Vec::new();
                    for (class_name, info) in self.scoped_classes() {
                        let Some(class_id) = self.class_ids.get(class_name).copied() else {
                            continue;
                        };
                        if self.constant_reference_updates(class_id, class_name, info, &mut updates)
                        {
                            found.push(class_id);
                        }
                    }
                    with_deferred = Some(found);
                }
                Some(ids) => {
                    for class_id in ids {
                        let Some(class_name) = self.class_name_by_id(*class_id).cloned() else {
                            continue;
                        };
                        if let Some(info) = self.classes.get(&class_name) {
                            self.constant_reference_updates(
                                *class_id,
                                &class_name,
                                info,
                                &mut updates,
                            );
                        }
                    }
                }
            }
            if updates.is_empty() {
                break;
            }
            for (class_id, const_slot, ty) in updates {
                self.class_const_types.insert((class_id, const_slot), ty);
            }
        }
        for class_id in with_deferred.iter().flatten().copied() {
            let Some(class_name) = self.class_name_by_id(class_id) else {
                continue;
            };
            let Some(info) = self.classes.get(class_name) else {
                continue;
            };
            let unresolved: Vec<_> =
                class_constant_entries(info, self.class_constant_results.get(&class_id))
                    .into_iter()
                    .filter_map(|(slot, _, sig)| {
                        let ty = self
                            .class_const_types
                            .get(&(class_id, slot))
                            .unwrap_or(&sig.ty);
                        ty.contains_deferred()
                            .then(|| (slot, unresolved_class_const_to_mixed(ty.clone())))
                    })
                    .collect();
            for (slot, ty) in unresolved {
                self.class_const_types.insert((class_id, slot), ty);
            }
        }
    }

    /// One round of `resolve_class_constant_references` for a class: the
    /// constants of `info` that change, pushed onto `updates`. Whether the
    /// class has a deferred constant type at all.
    fn constant_reference_updates(
        &self,
        class_id: ClassId,
        class_name: &Name,
        info: &ClassReflection,
        updates: &mut Vec<(ClassId, usize, Type)>,
    ) -> bool {
        let constants = class_constant_entries(info, self.class_constant_results.get(&class_id));
        if !constants.iter().any(|(slot, _, sig)| {
            self.class_const_types
                .get(&(class_id, *slot))
                .unwrap_or(&sig.ty)
                .contains_deferred()
        }) {
            return false;
        }
        let own_consts = visible_class_consts(
            class_name,
            &self.classes,
            &self.class_ids,
            &self.class_const_types,
            &self.class_constant_results,
        );
        for (const_slot, _, sig) in constants {
            let current = self
                .class_const_types
                .get(&(class_id, const_slot))
                .unwrap_or(&sig.ty);
            if !current.contains_deferred() {
                continue;
            }
            let step = resolve_deferred_value_of_in_type(
                bind_class_const_self(current.clone(), class_name, &own_consts),
                &self.classes,
                &self.class_ids,
                &self.class_const_types,
                &self.class_constant_results,
                None,
            );
            if step != *current {
                updates.push((class_id, const_slot, step));
            }
        }
        true
    }

    /// `@phpstan-import-type`'s deferred half - `ClassReflection::from_decl`
    /// already resolved both names (`ResolvedTypeImport`'s doc comment),
    /// but couldn't look the import up yet: the source class may not be
    /// indexed at all at that point. Mirrors `resolve_deferred_value_of`'s
    /// exact two-step shape (a read-only pass collecting every resolvable
    /// answer, then a second pass applying them) for the identical
    /// reason - looking a different class's `type_aliases` up while
    /// mutating this class's own would otherwise borrow `self.classes`
    /// two ways at once.
    ///
    /// A source class's alias is looked up under `{its own
    /// namespace}\{alias name}` - reconstructed directly from its FQN
    /// (`class_namespace`) rather than re-running that class's own name
    /// resolution, which would need its own `NameContext` (not kept
    /// around once `from_decl` returns). This matches `NameContext::
    /// resolve`'s actual behavior for a bare, unqualified name with no
    /// matching `use`-import: falls straight through to `declared_name`,
    /// i.e. plain namespace-prefixing - exactly what a `@phpstan-type`
    /// tag's bare alias name always is in practice.
    ///
    /// Not a fixed-point loop (an import that itself points at another
    /// imported - not locally declared - alias isn't chased further) -
    /// the identical "one level of indirection only" leniency
    /// `build_type_aliases`'s second pass accepts for local aliases
    /// referencing each other.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_type_imports(&mut self) {
        let mut classes_with_imports: Vec<ClassId> = Vec::new();
        let mut resolved: Vec<(ClassId, Name, Type)> = Vec::new();
        for (class_name, info) in self.scoped_classes() {
            if info.type_imports.is_empty() {
                continue;
            }
            let Some(class_id) = self.class_ids.get(class_name).copied() else {
                continue;
            };
            classes_with_imports.push(class_id);
            for imp in &info.type_imports {
                let source_key = syntax::name_resolution::qualify(
                    class_namespace(&imp.source_class_fqn),
                    &imp.source_alias_name,
                );
                if let Some(ty) = self
                    .classes
                    .get(&imp.source_class_fqn)
                    .and_then(|src| src.type_aliases.get(source_key.as_str()))
                {
                    resolved.push((class_id, imp.local_key.clone(), ty.clone()));
                }
            }
        }
        if classes_with_imports.is_empty() {
            return;
        }
        let mut by_class: FxHashMap<ClassId, NameMap<Type>> = FxHashMap::default();
        for (class_id, local_key, ty) in resolved {
            by_class.entry(class_id).or_default().insert(local_key, ty);
        }
        for class_id in classes_with_imports {
            let Some(class_name) = self.class_name_by_id(class_id).cloned() else {
                continue;
            };
            let Some(mut info) = self.classes.take(&class_name) else {
                continue;
            };
            // A class whose imports all failed to resolve (its source
            // class isn't indexed at all) still needs `type_imports`
            // cleared - the one-shot pass above already ran, so there's
            // no second chance coming; an empty `aliases` map here makes
            // every `substitute_type_aliases` call below a no-op.
            let aliases = by_class.remove(&class_id).unwrap_or_default();
            let mut resolved_const_types = Vec::new();
            let mut resolved_properties = Vec::new();
            let substitute = |ty: &mut Type| {
                let taken = std::mem::replace(ty, Type::mixed());
                *ty = substitute_type_aliases(taken, &aliases);
            };
            let substitute_signature = |sig: &mut FunctionReflection| {
                for param in &mut sig.params {
                    substitute(&mut param.ty);
                    if let Some(doc_ty) = &mut param.doc_ty {
                        substitute(doc_ty);
                    }
                }
                substitute(&mut sig.return_type);
                if let Some(doc_ty) = &mut sig.return_doc_ty {
                    substitute(doc_ty);
                }
                if let Some(out) = &mut sig.self_out {
                    substitute(out);
                }
            };
            let mut resolved_methods = Vec::new();
            for name in info.methods.keys() {
                let current = info.methods.get(name).expect("method key exists");
                let mut resolved = current.clone();
                substitute_signature(&mut resolved);
                if resolved != *current {
                    resolved_methods.push((name.clone(), resolved));
                }
            }
            if let Some(methods) = self.class_method_results.get(&class_id) {
                for (name, current) in methods.iter() {
                    let mut resolved = current.clone();
                    substitute_signature(&mut resolved);
                    if resolved != *current {
                        resolved_methods.push((name.clone(), resolved));
                    }
                }
            }
            let existing_properties = self.class_property_results.get(&class_id);
            let property_names: NameSet = info
                .properties
                .keys()
                .chain(
                    existing_properties
                        .into_iter()
                        .flat_map(|properties| properties.keys()),
                )
                .cloned()
                .collect();
            for name in property_names {
                let Some(current) = existing_properties
                    .and_then(|properties| properties.get(&name))
                    .or_else(|| info.properties.get(&name))
                else {
                    continue;
                };
                let mut prop = PropertyReflection::clone(current);
                substitute(&mut prop.ty);
                if let Some(doc) = &mut prop.doc_ty {
                    substitute(doc);
                }
                if let Some(set_ty) = &mut prop.set_ty {
                    substitute(set_ty);
                }
                if prop != **current {
                    resolved_properties.push((name, Arc::new(prop)));
                }
            }
            for (slot, _, c) in
                class_constant_entries(&info, self.class_constant_results.get(&class_id))
            {
                let current = self
                    .class_const_types
                    .get(&(class_id, slot))
                    .unwrap_or(&c.ty);
                let ty = substitute_type_aliases(current.clone(), &aliases);
                if &ty != current {
                    resolved_const_types.push((slot, ty));
                }
            }
            for (slot, ty) in resolved_const_types {
                self.class_const_types.insert((class_id, slot), ty);
            }
            self.class_property_results
                .entry(class_id)
                .or_default()
                .extend(resolved_properties);
            let method_results = self.class_method_results.entry(class_id).or_default();
            for (name, method) in resolved_methods {
                method_results.insert(name, method);
            }
            info.type_aliases.extend(aliases);
            info.type_imports.clear();
            self.classes.insert(class_name, info);
        }
    }

    /// `@mixin`'s deferred half - mirrors `resolve_deferred_type_imports`'s
    /// exact two-step shape (a read-only collection pass, then a second
    /// pass applying it): the mixin target may not be indexed yet at
    /// `ClassReflection::from_decl` time.
    ///
    /// Only a public method/property is pulled in, mirroring real
    /// PHPStan's `MixinMethodsClassReflectionExtension`/
    /// `MixinPropertiesClassReflectionExtension`, both of which resolve
    /// the mixin target from `OutOfClassScope` (no special access, the
    /// same visibility a caller anywhere outside the class itself would
    /// see) - and only when this class doesn't already declare that name
    /// for real, same "real declaration always wins" precedent
    /// `@property`/`@method` synthesis have.
    ///
    /// Not chased through a mixin's own further `@mixin`s (one level
    /// only) - the same "don't attempt a fixed-point solve for a shape
    /// not yet confirmed to occur in practice" leniency `build_
    /// type_aliases`'s alias-of-alias handling accepts.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_mixins(&mut self) {
        let mut method_additions: Vec<(
            MethodRef,
            MethodEntry,
            Arc<super::Lazy<FunctionReflection>>,
        )> = Vec::new();
        let mut property_additions: Vec<(ClassId, Name, Arc<PropertyReflection>)> = Vec::new();
        for (class_name, info) in self.scoped_classes() {
            let Some(class_id) = self.class_ids.get(class_name).copied() else {
                continue;
            };
            let mut added_method_names = FoldedMap::default();
            let own_property_names: NameSet = self.property_names(class_name).cloned().collect();
            let mut next_method_slot = info.member_order.len();
            for mixin_fqn in &info.mixins {
                let Some(mixin_info) = self.classes.get(mixin_fqn) else {
                    continue;
                };
                for entry in &mixin_info.member_order {
                    let name = &entry.name;
                    let Some(sig) = self.lookup_own_method(mixin_fqn, name) else {
                        continue;
                    };
                    if sig.visibility == Visibility::Public
                        && !info.member_slots.contains_key(name)
                        && !added_method_names.contains_key(name)
                        && let Some(shared_sig) = self.shared_own_method(mixin_fqn, name)
                    {
                        let slot = u16::try_from(next_method_slot)
                            .expect("class method slot space exhausted");
                        next_method_slot += 1;
                        added_method_names.insert(name.clone(), ());
                        method_additions.push((
                            MethodRef::new(class_id, slot),
                            MethodEntry {
                                name: name.clone(),
                                is_abstract: entry.is_abstract,
                                is_final: entry.is_final,
                                is_static: entry.is_static,
                                is_final_by_phpdoc: entry.is_final_by_phpdoc,
                            },
                            shared_sig,
                        ));
                    }
                }
                for name in self.property_names(mixin_fqn) {
                    let Some(sig) = self.lookup_property(mixin_fqn, name) else {
                        continue;
                    };
                    if sig.visibility == Visibility::Public && !own_property_names.contains(name) {
                        property_additions.push((class_id, name.clone(), Arc::new(sig.clone())));
                    }
                }
            }
        }
        for (method_ref, entry, sig) in method_additions {
            let class_id = method_ref.class();
            let Some(class_name) = self.class_name_by_id(class_id).cloned() else {
                continue;
            };
            let name = entry.name.clone();
            let mut inserted = false;
            if let Some(info) = self.classes.get_mut(&class_name)
                && !info.member_slots.contains_key(&name)
            {
                let registered = info.register_member_entry(entry);
                debug_assert!(registered);
                debug_assert_eq!(info.member_slots.get(&name), Some(&method_ref.slot()));
                inserted = true;
            }
            if inserted {
                let methods = self.class_method_results.entry(class_id).or_default();
                debug_assert!(methods.insert_shared_if_absent(name, sig));
            }
        }
        for (class_id, name, sig) in property_additions {
            let Some(class_name) = self.class_name_by_id(class_id).cloned() else {
                continue;
            };
            if self
                .classes
                .get(&class_name)
                .is_some_and(|info| info.properties.contains_key(&name))
                || self
                    .class_property_results
                    .get(&class_id)
                    .is_some_and(|properties| properties.contains_key(&name))
            {
                continue;
            }
            self.class_property_results
                .entry(class_id)
                .or_default()
                .insert(name, sig);
        }
    }

    /// The `&self` (read-only, immediate) twin of `resolve_deferred_
    /// value_of` above, for a `Type` parsed directly from a raw docblock
    /// string during checking rather than at registry-build time - by
    /// then every class/enum genuinely is already fully indexed, so this
    /// resolves a `ValueOf`/`ClassConstant` immediately, inline, with no
    /// pass needed. Every checker-side call site that parses a docblock
    /// type string independently of the registry's own
    /// `FunctionReflection::from_decl`/`ClassReflection::from_decl` pipeline (which
    /// `resolve_deferred_value_of` covers) needs this too - e.g.
    /// `Checker::resolve_type_hint`, reached through an inline `@var`
    /// annotation, would otherwise leak the raw, unresolved sentinel
    /// straight into a diagnostic message.
    pub(crate) fn resolve_deferred_value_of_now(&self, ty: Type) -> Type {
        if !ty.contains_deferred() {
            return ty;
        }
        resolve_deferred_value_of_in_type(
            ty,
            &self.classes,
            &self.class_ids,
            &self.class_const_types,
            &self.class_constant_results,
            None,
        )
    }

    /// `resolve_deferred_value_of_now`'s native-fallback-aware sibling.
    /// See `resolve_deferred_value_of_with_native_fallback`'s doc comment
    /// (the free function this wraps) for why an unresolvable
    /// `value-of<T>` needs this rather than plain
    /// `resolve_deferred_value_of_now` alone. Used by the anonymous-class
    /// param/return docblock fallback in `checker.rs`, which (like the
    /// registry's `FunctionReflection::from_decl`) reconciles its raw-parsed
    /// doc type against a native counterpart via `resolve_type` before
    /// this ever runs.
    pub(crate) fn resolve_deferred_value_of_now_with_native_fallback(
        &self,
        ty: Type,
        native: &Type,
    ) -> Type {
        if !ty.contains_deferred() {
            return ty;
        }
        resolve_deferred_value_of_with_native_fallback(
            ty,
            native,
            &self.classes,
            &self.class_ids,
            &self.class_const_types,
            &self.class_constant_results,
            None,
        )
    }

    /// `class_name`'s concrete binding for each templated ancestor
    /// (`{ancestor => {template => type}}`). Its own templates stay
    /// templates, scoped to it, through the mapping, so an inherited
    /// `list<T>` becomes `list<CMTemplate>` for a `@template-implements
    /// Factory<CMTemplate>` class rather than losing `T` to `Mixed`.
    fn ancestor_bindings(&self, class_name: &str, info: &ClassReflection) -> AncestorBindings {
        let own_args = info.own_template_args(class_name);
        self.all_ancestor_names(class_name)
            .into_iter()
            .filter(|a| self.classes.get(a).is_some_and(|c| !c.templates.is_empty()))
            .map(|a| {
                let bindings = self.class_template_bindings(class_name, &own_args, &a);
                (a, bindings)
            })
            .collect()
    }

    /// The narrower, corrected replacement for a bare `!type_mentions_
    /// foreign_scoped_template_param(...)` check at this function's
    /// backfill call sites: a class-scoped template is safe to backfill
    /// after all, provided `class_name` (the class the backfill would
    /// land on, and therefore the `owner` a later call-site
    /// `lookup_method_with_owner` will report) has a real, non-empty
    /// concrete binding for that exact scope - `ancestor_bindings`,
    /// precomputed once per `class_name` (not per-method; bindings don't
    /// depend on which method is being backfilled) by the caller as
    /// `{ancestor name => class_template_bindings(class_name, [],
    /// ancestor)}` for every `class_name` ancestor that itself declares
    /// a real `@template`. This is deliberately stricter than "is the
    /// scope structurally one of `class_name`'s ancestors at all" - a
    /// bare `implements Container` with no `<Concrete>` type argument
    /// reaches `Container` as an ancestor just as much as a real
    /// `@implements Container<Concrete>` does, but
    /// `class_template_bindings` for that erased case returns an empty
    /// map (`zip(Container.templates=["T"], type_args=[])`), which must
    /// still refuse backfill.
    pub(crate) fn template_param_scopes_all_safe_to_backfill<'b>(
        ty: &Type,
        method_name: &str,
        ancestor_bindings: &dyn Fn() -> &'b NameMap<FxHashMap<String, Type>>,
    ) -> bool {
        ty.nested_templates().iter().all(|tp| {
            tp.scope.as_deref() == Some(method_name)
                || tp.scope.as_deref() == Some(types::CALLABLE_TEMPLATE_SCOPE)
                || tp
                    .scope
                    .as_deref()
                    .is_some_and(|s| ancestor_bindings().get(s).is_some_and(|b| !b.is_empty()))
        })
    }

    /// Whether backfilling `ancestor_ty` onto a position whose own native
    /// hint alone already resolves to `native_ty` is safe: `false` only
    /// when both sides are a concrete object naming *different*
    /// classes - PHP's own native hint is authoritative about what a
    /// position's real static type is (enforced at runtime), so a
    /// same-family-but-different-class ancestor docblock type can only
    /// ever be an ancestor-scoped generic that doesn't correctly apply to
    /// *this* override, never a legitimate narrowing (a real narrowing
    /// covariant override always keeps the *same* class name, or one the
    /// child's own native hint should already name more specifically
    /// itself). Guards a case neither of the two checks above catches: a
    /// real, concrete, non-template type on both sides where the
    /// ancestor names a less specific class than the child's native
    /// hint already resolves to - backfilling it would still silently
    /// widen away the already-correct, more specific native class.
    pub(crate) fn safe_to_backfill_object_type(ancestor_ty: &Type, native_ty: &Type) -> bool {
        match (ancestor_ty.as_object(), native_ty.as_object()) {
            (Some((a, _)), Some((n, _))) => a == n,
            _ => true,
        }
    }

    /// Whether backfilling an ancestor's `Mixed` tag is safe: `false`
    /// only when the child's own native resolution (`ty_native`/
    /// `return_type_native`, computed with no docblock merge) is itself
    /// something more specific than `Mixed` - inheriting a `Mixed` tag
    /// can only ever destroy that kind of real information, never add
    /// any. When the child's native resolution is already `Mixed` too (a
    /// genuinely untyped native parameter, PSR-3's
    /// `LoggerInterface::log(mixed $level, ...)` shape), there's nothing
    /// to lose - inheriting the ancestor's `Mixed` here is exactly what
    /// correctly propagates `explicit_mixed_hint` down from wherever it
    /// was deliberately written, rather than leaving the position
    /// looking entirely undocumented.
    pub(crate) fn safe_to_backfill_mixed(ancestor_ty: &Type, native_ty: &Type) -> bool {
        !ancestor_ty.is_mixed() || native_ty.is_mixed()
    }

    /// One pass of `flatten_phpdoc_inheritance` - see its doc comment.
    /// Returns whether anything was actually changed, so the caller knows
    /// when a fixed point has been reached.
    ///
    /// Gates each backfill on the ancestor's own `has_own_doc`/
    /// `return_has_own_doc` bit, not merely on the ancestor having a type
    /// at all - real PHPStan's merge only ever pulls from an ancestor's
    /// own parsed `@param`/`@return` tag; an ancestor with no tag of its
    /// own has nothing to contribute, even though its `ty`/`return_type`
    /// field is never literally empty (it still holds whatever the
    /// native hint alone resolved to). Naively backfilling a type onto a
    /// genuine Liskov-violating override with no docblock anywhere would
    /// both hide the real override diagnostic behind a fabricated
    /// docblock type and produce a bogus `return.type` mismatch against
    /// the still-native-typed return body.
    ///
    /// A successful backfill does flip `has_own_doc`/`return_has_own_doc`
    /// to `true` - once resolved, a position counts as "documented" (own
    /// or inherited) for any further descendant's lookup, which is
    /// exactly what makes multi-level transitivity work one level per
    /// pass (an interface only two levels up from a concrete class,
    /// itself redeclaring the method with no doc of its own, only
    /// becomes a usable source on the pass after it was itself
    /// backfilled) - and, as a side effect, is what makes a resolved
    /// position permanently ineligible for re-visiting on a later pass,
    /// the same never-revisit-a-filled-slot shape
    /// `flatten_traits_one_pass`'s `Entry::Vacant` check has. The
    /// collection loop below only reads the registry - no write happens
    /// until the sequential apply loop afterward, the identical shape
    /// `flatten_traits_one_pass` parallelizes.
    fn flatten_phpdoc_inheritance_one_pass(
        &mut self,
        class_names: &[Name],
        order: &[usize],
        bindings: &[OnceLock<AncestorBindings>],
    ) -> bool {
        // What one method takes from its ancestors: the parameters by
        // position and the return type.
        type Backfill = (MethodRef, Vec<(usize, Type, bool)>, Option<(Type, bool)>);

        let registry: &Registry = self;
        let backfills: Vec<(ClassId, Vec<Backfill>)> = order
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|&index| {
                let class_name = &class_names[index];
                let class_id = *registry.class_ids.get(class_name)?;
                let mut per_class: Vec<Backfill> = Vec::new();
                let info = registry.classes().get(class_name)?;
                // What `lookup_ancestor_method_with_owner` reads, read once
                // per class and only when a method needs it: the maps of
                // its parents' methods to their owners, and the owners
                // already found (a class's methods mostly share a few).
                let mut parent_maps: Option<Vec<&FoldedMap<Name>>> = None;
                for method_name in registry.method_names(class_name) {
                    let Some(sig) = registry.lookup_own_method(class_name, method_name) else {
                        continue;
                    };
                    let needs_param = sig.params.iter().any(|p| !p.has_own_doc);
                    let needs_return = !sig.return_has_own_doc;
                    if !needs_param && !needs_return {
                        continue;
                    }
                    let parent_maps = parent_maps
                        .get_or_insert_with(|| registry.parent_method_owner_maps(info).collect());
                    if parent_maps.is_empty() {
                        // No ancestor has a method: none of the rest does.
                        break;
                    }
                    let Some(owner) = parent_maps
                        .iter()
                        .find_map(|owners| owners.get(method_name))
                    else {
                        continue;
                    };
                    let Some(ancestor_sig) = registry.lookup_own_method(owner, method_name) else {
                        continue;
                    };
                    if ancestor_sig.visibility == Visibility::Private {
                        continue;
                    }
                    // Once backfilled, this signature lives on
                    // `class_name`'s own `methods` map - a later call-site
                    // resolution reports `class_name` itself as `owner`,
                    // so `class_name`'s concrete binding for each generic
                    // ancestor is exactly what `checker::generics::
                    // substitution::substitute_class_templates` resolves
                    // at call time later. See `template_param_scopes_all_
                    // safe_to_backfill`'s doc comment for why this must be
                    // the real resolved bindings, not merely "is this
                    // ancestor name reachable at all".
                    let ancestor_bindings = || {
                        bindings[index].get_or_init(|| registry.ancestor_bindings(class_name, info))
                    };
                    // Mirrors real PHPStan's identical carve-out
                    // (`PhpDocInheritanceResolver::resolvePhpDocForMethod`):
                    // a subclass's `__construct` legitimately takes
                    // completely unrelated parameters from a native/
                    // engine ancestor's constructor (real PHP doesn't
                    // require constructor signature compatibility at
                    // all), so `RuntimeException`/`Exception`'s
                    // `@param string $message` must never backfill onto a
                    // subclass constructor with an unrelated signature.
                    // `built_in_stub_classes` (populated by
                    // `merge_stub_classes`, phpstorm-stubs' native/
                    // engine-class declarations) is this project's
                    // equivalent of `ClassReflection::isBuiltin()` - true
                    // for `Exception`/`ArrayObject`/... but not for a
                    // real vendored userland base class (Symfony's
                    // `Command`, Doctrine's console commands, ...), which
                    // must still participate in this pass normally.
                    if syntax::ast::is_constructor_name(method_name)
                        && registry.built_in_stub_classes.contains_key(owner)
                    {
                        continue;
                    }
                    let mut param_fills = Vec::new();
                    if needs_param {
                        // Real PHP never enforces constructor-signature
                        // compatibility (arity, order, or names can differ
                        // freely between a class and its parent), so the
                        // usual by-position correspondence - correct for
                        // a real, LSP-bound override, where PHP itself
                        // guarantees positions line up - cannot be
                        // trusted for `__construct`. Mirrors real
                        // PHPStan's special case
                        // (`resolveMethodPhpDocFromParentClass`'s
                        // `$lowercaseMethodName !== '__construct'` guard):
                        // match by parameter name instead.
                        if syntax::ast::is_constructor_name(method_name) {
                            for (i, p) in sig.params.iter().enumerate() {
                                if p.has_own_doc {
                                    continue;
                                }
                                if let Some(ancestor_param) =
                                    ancestor_sig.params.iter().find(|ap| ap.name == p.name)
                                    && ancestor_param.has_own_doc
                                    && Registry::safe_to_backfill_mixed(
                                        &ancestor_param.ty,
                                        &p.ty_native,
                                    )
                                    && Registry::template_param_scopes_all_safe_to_backfill(
                                        &ancestor_param.ty,
                                        method_name,
                                        &ancestor_bindings,
                                    )
                                    && Registry::safe_to_backfill_object_type(
                                        &ancestor_param.ty,
                                        &p.ty_native,
                                    )
                                {
                                    param_fills.push((
                                        i,
                                        Registry::substitute_class_scoped_ancestor_templates(
                                            &ancestor_param.ty.bind_self(owner),
                                            &ancestor_bindings,
                                        )
                                        .reconcile_with_native(p.ty_native.clone()),
                                        ancestor_param.explicit_mixed_hint,
                                    ));
                                }
                            }
                        } else {
                            for (i, p) in sig.params.iter().enumerate() {
                                if p.has_own_doc {
                                    continue;
                                }
                                if let Some(ancestor_param) = ancestor_sig.params.get(i)
                                    && ancestor_param.has_own_doc
                                    && Registry::safe_to_backfill_mixed(
                                        &ancestor_param.ty,
                                        &p.ty_native,
                                    )
                                    && Registry::template_param_scopes_all_safe_to_backfill(
                                        &ancestor_param.ty,
                                        method_name,
                                        &ancestor_bindings,
                                    )
                                    && Registry::safe_to_backfill_object_type(
                                        &ancestor_param.ty,
                                        &p.ty_native,
                                    )
                                {
                                    param_fills.push((
                                        i,
                                        Registry::substitute_class_scoped_ancestor_templates(
                                            &ancestor_param.ty.bind_self(owner),
                                            &ancestor_bindings,
                                        )
                                        .reconcile_with_native(p.ty_native.clone()),
                                        ancestor_param.explicit_mixed_hint,
                                    ));
                                }
                            }
                        }
                    }
                    // `safe_to_backfill_mixed` (both here and in the
                    // param loops above) isn't about `__construct` at
                    // all - see its doc comment.
                    // A builtin method's native return type is tentative
                    // (PHP 8.1): an override may omit it, and then has it
                    // still.
                    let return_fill = (needs_return
                        && (ancestor_sig.return_has_own_doc
                            || (sig.return_type_native.is_mixed()
                                && !ancestor_sig.return_type_native.is_mixed()
                                && registry.built_in_stub_classes.contains_key(owner)))
                        && Registry::safe_to_backfill_mixed(
                            &ancestor_sig.return_type,
                            &sig.return_type_native,
                        )
                        && Registry::template_param_scopes_all_safe_to_backfill(
                            &ancestor_sig.return_type,
                            method_name,
                            &ancestor_bindings,
                        )
                        && Registry::safe_to_backfill_object_type(
                            &ancestor_sig.return_type,
                            &sig.return_type_native,
                        ))
                    .then(|| {
                        (
                            Registry::substitute_class_scoped_ancestor_templates(
                                &ancestor_sig.return_type.bind_self(owner),
                                &ancestor_bindings,
                            )
                            .reconcile_with_native(sig.return_type_native.clone()),
                            ancestor_sig.return_explicit_mixed_hint,
                        )
                    });
                    if !param_fills.is_empty() || return_fill.is_some() {
                        let slot = *info.member_slots.get(method_name)?;
                        per_class.push((MethodRef::new(class_id, slot), param_fills, return_fill));
                    }
                }
                (!per_class.is_empty()).then_some((class_id, per_class))
            })
            .collect();

        let mut changed = false;
        for (class_id, per_class) in backfills {
            let Some(class_name) = self.class_name_by_id(class_id).cloned() else {
                continue;
            };
            let Some(info) = self.classes.get_mut(&class_name) else {
                continue;
            };
            for (method_ref, param_fills, return_fill) in per_class {
                debug_assert_eq!(method_ref.class(), class_id);
                let Some(method_name) = info
                    .member_order
                    .get(usize::from(method_ref.slot()))
                    .map(|entry| entry.name.clone())
                else {
                    continue;
                };
                let Some(mut sig) = self
                    .class_method_results
                    .get(&class_id)
                    .and_then(|methods| methods.get(&method_name))
                    .or_else(|| info.methods.get(&method_name))
                    .cloned()
                else {
                    continue;
                };
                for (i, ty, explicit_mixed_hint) in param_fills {
                    if let Some(p) = sig.params.get_mut(i) {
                        p.ty = ty;
                        p.has_own_doc = true;
                        p.explicit_mixed_hint = explicit_mixed_hint;
                        changed = true;
                    }
                }
                if let Some((ty, explicit_mixed_hint)) = return_fill {
                    sig.return_type = ty;
                    sig.return_has_own_doc = true;
                    sig.return_explicit_mixed_hint = explicit_mixed_hint;
                    changed = true;
                }
                self.class_method_results
                    .entry(class_id)
                    .or_default()
                    .insert(method_name, sig);
            }
        }
        changed
    }
}

/// The namespace a fully-qualified class name was declared in - everything
/// before the last `\`, empty for the global namespace. `Registry::
/// resolve_deferred_type_imports`'s doc comment explains why this is
/// exactly the prefix a bare `@phpstan-type` alias name declared on that
/// same class resolves against.
fn class_namespace(fqn: &str) -> &str {
    fqn.rsplit_once('\\').map_or("", |(ns, _)| ns)
}

/// The actual substitution: every `ValueOf`/`ClassConstant` in `ty`, at
/// any depth. See `Registry::resolve_deferred_value_of`'s doc comment
/// for why this exists, and `types::parse_type_keyword_or_class`'s
/// `"value-of"` arm for where a `ValueOf` is produced.
fn resolve_deferred_value_of_in_type(
    ty: Type,
    classes: &NameTable<ClassReflection>,
    class_ids: &NameTable<ClassId>,
    const_types: &FxHashMap<(ClassId, usize), Type>,
    constant_results: &FxHashMap<ClassId, ClassConstantResults>,
    excluded: Option<&str>,
) -> Type {
    ty.resolve_class_references(&DeclaredConstants {
        classes,
        class_ids,
        const_types,
        constant_results,
        excluded,
    })
}

/// The constants and enum cases of the classes indexed so far, but for
/// the one `excluded` class being resolved: a reference to it from its own
/// types degrades to `Mixed`, the same "can't verify, don't guess"
/// outcome every other unresolvable one gets.
struct DeclaredConstants<'a> {
    classes: &'a NameTable<ClassReflection>,
    class_ids: &'a NameTable<ClassId>,
    const_types: &'a FxHashMap<(ClassId, usize), Type>,
    constant_results: &'a FxHashMap<ClassId, ClassConstantResults>,
    excluded: Option<&'a str>,
}

impl DeclaredConstants<'_> {
    fn is_excluded(&self, class: &str) -> bool {
        self.excluded
            .is_some_and(|excluded| excluded.eq_ignore_ascii_case(class.trim_start_matches('\\')))
    }
}

impl types::ClassConstants for DeclaredConstants<'_> {
    fn enum_values(&self, class: &str) -> Type {
        if self.is_excluded(class) {
            return Type::mixed();
        }
        resolve_value_of_target(class, self.classes)
    }

    fn class_constants(&self, class: &Type, pattern: &str) -> Type {
        if class
            .as_object()
            .is_some_and(|(fqn, _)| self.is_excluded(fqn))
        {
            return Type::mixed();
        }
        resolve_class_const_type(
            class,
            pattern,
            self.classes,
            self.class_ids,
            self.const_types,
            self.constant_results,
        )
    }
}

/// A `ClassConstant`'s value: the union of every matching
/// constant's own type (an enum case's own case type, a literal
/// constant's literal type), mirroring real PHPStan's `TypeNodeResolver`
/// for a `ConstTypeNode` - a `*` in the name matches any run of
/// characters. Matches are ordered by name for a deterministic union
/// (real PHPStan uses declaration order, which `ClassReflection::consts` doesn't
/// keep). An unknown class, or no match, degrades to `Mixed`. Only the
/// class's own constants are consulted, not inherited ones. A `self`/
/// `static` reference has already been bound to the owning class by
/// `bind_class_const_self`, or stays unresolved (`Mixed`) where no owning
/// class exists.
fn resolve_class_const_type(
    class: &Type,
    pattern: &str,
    classes: &NameTable<ClassReflection>,
    class_ids: &NameTable<ClassId>,
    const_types: &FxHashMap<(ClassId, usize), Type>,
    constant_results: &FxHashMap<ClassId, ClassConstantResults>,
) -> Type {
    let Some((fqn, _)) = class.as_object() else {
        return Type::mixed();
    };
    let fqn = fqn.trim_start_matches('\\');
    if !classes.contains_key(fqn) {
        return Type::mixed();
    }
    matching_class_const_type(
        &visible_class_consts(fqn, classes, class_ids, const_types, constant_results),
        pattern,
    )
}

/// Whether any type `resolve_deferred_value_of` rewrites on `prop` contains
/// a deferred sentinel.
fn property_has_deferred_type(prop: &PropertyReflection) -> bool {
    prop.ty.contains_deferred()
        || prop.doc_ty.as_ref().is_some_and(Type::contains_deferred)
        || prop.set_ty.as_ref().is_some_and(Type::contains_deferred)
}

/// Whether any type `resolve_deferred_value_of` rewrites on `sig` contains
/// a deferred sentinel.
fn function_has_deferred_type(sig: &FunctionReflection) -> bool {
    sig.return_type.contains_deferred()
        || [&sig.return_doc_ty, &sig.self_out]
            .into_iter()
            .flatten()
            .any(Type::contains_deferred)
        || sig.params.iter().any(|p| {
            p.ty.contains_deferred()
                || [&p.doc_ty, &p.out_ty, &p.closure_this_type]
                    .into_iter()
                    .flatten()
                    .any(Type::contains_deferred)
        })
}

/// Whether any type `resolve_deferred_value_of` rewrites on `info`
/// contains a deferred sentinel - lets that pass skip a class outright.
fn class_has_deferred_type(
    info: &ClassReflection,
    method_results: Option<&Members<FunctionReflection>>,
) -> bool {
    info.methods.values().any(function_has_deferred_type)
        || method_results.is_some_and(|methods| methods.values().any(function_has_deferred_type))
        || info
            .extends_args
            .iter()
            .chain(info.implements_args.values().flatten())
            .any(Type::contains_deferred)
        || info
            .properties
            .values()
            .any(|p| property_has_deferred_type(p))
        || info.consts.values().any(|c| c.ty.contains_deferred())
        || info.template_bounds.values().any(Type::contains_deferred)
}

fn class_constant_entries<'a>(
    info: &'a ClassReflection,
    results: Option<&'a ClassConstantResults>,
) -> Vec<(usize, &'a Name, &'a ConstantReflection)> {
    let mut entries: Vec<_> = info
        .consts
        .iter()
        .enumerate()
        .map(|(slot, (name, constant))| (slot, name, &**constant))
        .collect();
    if let Some(results) = results {
        entries.extend(
            results
                .iter()
                .map(|(name, (slot, constant))| (*slot, name, &**constant)),
        );
    }
    entries.sort_unstable_by_key(|(slot, _, _)| *slot);
    entries
}

/// Every constant visible on `fqn` - its own, then each parent's and
/// interface's not already shadowed - the set real PHPStan's
/// `ClassReflection::getConstants()` resolves a `Foo::*` against. Walked
/// breadth-first, a class's own declaration winning over an inherited one
/// of the same name.
fn visible_class_consts(
    fqn: &str,
    classes: &NameTable<ClassReflection>,
    class_ids: &NameTable<ClassId>,
    const_types: &FxHashMap<(ClassId, usize), Type>,
    constant_results: &FxHashMap<ClassId, ClassConstantResults>,
) -> VisibleClassConstTypes {
    let mut consts = VisibleClassConstTypes::default();
    let mut seen: NameSet = NameSet::default();
    let mut queue: std::collections::VecDeque<Name> = std::collections::VecDeque::new();
    queue.push_back(Name::from(fqn));
    while let Some(name) = queue.pop_front() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(info) = classes.get(&name) else {
            continue;
        };
        let class_id = class_ids.get(&name).copied();
        let results = class_id.and_then(|id| constant_results.get(&id));
        for (slot, const_name, sig) in class_constant_entries(info, results) {
            if !consts.contains_key(const_name) {
                let ty = class_id
                    .and_then(|id| const_types.get(&(id, slot)))
                    .unwrap_or(&sig.ty);
                consts.insert(const_name.clone(), ty.clone());
            }
        }
        queue.extend(info.extends.iter().cloned());
        queue.extend(info.implements.iter().cloned());
    }
    consts
}

/// The union of every constant in `consts` whose name matches `pattern` -
/// see `resolve_class_const_type`.
// Declared types are joined while the class hierarchy is being built.
#[allow(clippy::disallowed_methods)]
fn matching_class_const_type(consts: &VisibleClassConstTypes, pattern: &str) -> Type {
    let names: Vec<&Name> = consts
        .keys()
        .filter(|name| glob_matches(pattern, name))
        .collect();
    let mut members: Vec<Type> = Vec::new();
    for name in names {
        let ty = consts[name].clone();
        if !members.contains(&ty) {
            members.push(ty);
        }
    }
    match members.len() {
        0 => Type::mixed(),
        1 => members.pop().expect("len checked"),
        _ => Type::union_of(members),
    }
}

/// `pattern` with `*` matching any (possibly empty) run of characters -
/// the only wildcard real PHPStan's constant-type syntax has.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or("");
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let remaining: Vec<&str> = parts.collect();
    let Some((last, middle)) = remaining.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        let Some(idx) = rest.find(part) else {
            return false;
        };
        rest = &rest[idx + part.len()..];
    }
    rest.ends_with(last)
}

/// Resolves every `ClassConstant` in `ty` that names `class`
/// itself (`self::X`, `static::X`, or `class`'s own name) straight from
/// `own_consts`. `resolve_deferred_value_of` takes the class being
/// rewritten out of the registry's class map while rewriting it, so
/// `resolve_deferred_value_of_in_type` can't find a class's own constants
/// there - this runs first, for the one class that's missing.
fn bind_class_const_self(ty: Type, class: &str, own_consts: &VisibleClassConstTypes) -> Type {
    bind_class_const_self_at(ty, class, own_consts, 0)
}

/// `bind_class_const_self`'s body. A substituted constant type can itself
/// hold a further own-class reference (`A = self::B`), so each
/// substitution is walked again, `depth` bounding a reference cycle
/// (`A = self::B`, `B = self::A`), which degrades to `Mixed`.
fn bind_class_const_self_at(
    ty: Type,
    class: &str,
    own_consts: &VisibleClassConstTypes,
    depth: usize,
) -> Type {
    syntax::stack::guard(|| {
        const MAX_DEPTH: usize = 16;
        ty.map_class_constants(&mut |owner, pattern| {
            let names_self = owner.is_self_reference()
                || owner
                    .as_object()
                    .is_some_and(|(fqn, _)| fqn.trim_start_matches('\\') == class);
            if !names_self {
                return None;
            }
            Some(if depth >= MAX_DEPTH {
                Type::mixed()
            } else {
                bind_class_const_self_at(
                    matching_class_const_type(own_consts, pattern),
                    class,
                    own_consts,
                    depth + 1,
                )
            })
        })
    })
}

/// Replaces every `ClassConstant` still left in `ty` with
/// `Mixed` - see `resolve_class_constant_references`.
fn unresolved_class_const_to_mixed(ty: Type) -> Type {
    ty.map_class_constants(&mut |_, _| Some(Type::mixed()))
}

/// `value-of<target_fqn>`'s resolved value, once `target_fqn`'s class/
/// enum data actually exists to consult, mirroring real PHPStan's
/// `ValueOfType::getResult()`: not an enum at all (or not a registered
/// class/enum) degrades to `Mixed`; an unbacked enum (no real `->value`
/// property, since real PHP never gives one to a non-backed enum) is
/// `Never`; a backed enum's already-built `properties["value"]`
/// (`registry/class_info.rs`, built for the enum's real `->value`
/// property) is already the exact union-of-case-literal-values type real
/// PHPStan computes independently - reused directly, not recomputed.
// Declared types are joined while the class hierarchy is being built.
#[allow(clippy::disallowed_methods)]
fn resolve_value_of_target(target_fqn: &str, classes: &NameTable<ClassReflection>) -> Type {
    // Any enum's case values, whichever enum it turns out to be.
    if is_enum_interface(target_fqn, classes, 0) {
        return Type::union_of(vec![Type::int(), Type::string()]);
    }
    let Some(info) = classes.get(target_fqn) else {
        return Type::mixed();
    };
    if info.kind != ClassKind::Enum {
        return Type::mixed();
    }
    match info.properties.get("value") {
        Some(sig) => sig.ty.clone(),
        None => Type::never(),
    }
}

/// Whether `fqn` is `UnitEnum`, `BackedEnum` or an interface extending
/// one, which only enums implement.
fn is_enum_interface(fqn: &str, classes: &NameTable<ClassReflection>, depth: usize) -> bool {
    let fqn = fqn.trim_start_matches('\\');
    if fqn.eq_ignore_ascii_case("UnitEnum") || fqn.eq_ignore_ascii_case("BackedEnum") {
        return true;
    }
    depth < 32
        && classes.get(fqn).is_some_and(|info| {
            info.kind == ClassKind::Interface
                && info
                    .extends
                    .iter()
                    .chain(&info.implements)
                    .any(|parent| is_enum_interface(parent, classes, depth + 1))
        })
}

/// Resolves `ty` (a param/return type that may contain a `ValueOf`)
/// and, if the result is bare `mixed`, falls back to
/// `native` instead when that's itself something more useful - restores
/// `function_sig::reconcile_with_native` behavior for a `value-of<T>`
/// that turns out unresolvable (`T` not a real, registered enum): without
/// this, a native type this project already trusted would be silently
/// replaced by a bare, strictly worse `Mixed`.
///
/// Only `ParameterReflection::ty`/`FunctionReflection::return_type` get this treatment -
/// they're the only two places this registry persists a native-only
/// counterpart type to fall back to (`ParameterReflection::ty_native`/
/// `FunctionReflection::return_type_native`). `PropertyReflection`/`ConstantReflection`/
/// `ClassReflection::template_bounds` have no such counterpart stored - an
/// unresolvable `value-of<T>` there stays plain `Mixed`.
fn resolve_deferred_value_of_with_native_fallback(
    ty: Type,
    native: &Type,
    classes: &NameTable<ClassReflection>,
    class_ids: &NameTable<ClassId>,
    const_types: &FxHashMap<(ClassId, usize), Type>,
    constant_results: &FxHashMap<ClassId, ClassConstantResults>,
    excluded: Option<&str>,
) -> Type {
    let resolved = resolve_deferred_value_of_in_type(
        ty,
        classes,
        class_ids,
        const_types,
        constant_results,
        excluded,
    );
    if resolved.is_mixed() && !native.is_mixed() {
        native.clone()
    } else {
        resolved
    }
}

impl ClassReflection {
    /// This class's own templates as `TemplateParam` arguments scoped to
    /// `class_name` - the identity binding, which `class_template_bindings`
    /// maps through to an ancestor without erasing them.
    pub(crate) fn own_template_args(&self, class_name: &str) -> Vec<Type> {
        self.templates
            .iter()
            .map(|name| {
                let variance = if self.covariant_templates.contains(name) {
                    types::Variance::Covariant
                } else if self.contravariant_templates.contains(name) {
                    types::Variance::Contravariant
                } else {
                    types::Variance::Invariant
                };
                Type::template_param(types::TemplateParam {
                    name: name.to_string(),
                    bound: Box::new(
                        self.template_bounds
                            .get(name)
                            .cloned()
                            .unwrap_or(Type::mixed()),
                    ),
                    variance,
                    scope: Some(class_name.to_string()),
                    default: self.template_defaults.get(name).cloned().map(Box::new),
                })
            })
            .collect()
    }
}

impl Registry {
    /// `reconcile_with_native` runs before the class hierarchy exists, so
    /// it keeps every class-named PHPDoc member. Once every class is
    /// indexed, a member the native hint provably excludes (`Schema` on a
    /// `?ValidationContext` parameter) is dropped here; when none
    /// survive, the native type stands, as PHP enforces it at runtime.
    pub(crate) fn drop_doc_members_excluded_by_native(&mut self) {
        #[derive(Clone, Copy)]
        enum SigId {
            Method(MethodRef),
            Function(FnId),
        }
        // (declaration id, parameter index or `None` for the return, corrected type)
        type Fix = (SigId, Option<usize>, Type);
        let pass = next_keep_pass();
        let registry: &Registry = self;
        let sig_fixes = |key: SigId, class: Option<&Name>, sig: &FunctionReflection| {
            let mut fixes: Vec<Fix> = Vec::new();
            for (i, p) in sig.params.iter().enumerate() {
                if let Some(ty) = registry.excluding_native(&p.ty, &p.ty_native, class, Some(pass))
                {
                    fixes.push((key, Some(i), ty));
                }
            }
            if let Some(ty) = registry.excluding_native(
                &sig.return_type,
                &sig.return_type_native,
                class,
                Some(pass),
            ) {
                fixes.push((key, None, ty));
            }
            fixes
        };
        let classes = self.scoped_classes();
        let functions = self.scoped_functions();
        let mut fixes: Vec<Fix> = classes
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map_iter(|(class, info)| {
                let Some(class_id) = registry.class_ids.get(*class).copied() else {
                    return Vec::new();
                };
                info.methods
                    .iter()
                    .filter_map(|(name, sig)| {
                        let slot = *info.member_slots.get(name)?;
                        Some(sig_fixes(
                            SigId::Method(MethodRef::new(class_id, slot)),
                            Some(class),
                            sig,
                        ))
                    })
                    .flatten()
                    .collect::<Vec<_>>()
            })
            .collect();
        fixes.par_extend(
            functions
                .par_iter()
                .with_min_len(crate::registry::PASS_CHUNK)
                .flat_map_iter(|(name, sig)| {
                    registry
                        .function_ids
                        .get(*name)
                        .copied()
                        .map(|id| sig_fixes(SigId::Function(id), None, sig))
                        .into_iter()
                        .flatten()
                }),
        );
        let property_fixes: Vec<(ClassId, Name, Arc<PropertyReflection>)> = classes
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map_iter(|(class, _info)| {
                let Some(class_id) = registry.class_ids.get(*class).copied() else {
                    return Vec::new();
                };
                registry
                    .property_names(class)
                    .filter_map(|name| {
                        let prop = registry.lookup_property(class, name)?;
                        if !prop.has_native_type {
                            return None;
                        }
                        let ty = registry.excluding_native(
                            &prop.ty,
                            &prop.ty_native,
                            Some(class),
                            Some(pass),
                        )?;
                        let mut updated = PropertyReflection::clone(prop);
                        updated.ty = ty;
                        Some((class_id, name.clone(), Arc::new(updated)))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        for (class_id, name, property) in property_fixes {
            self.class_property_results
                .entry(class_id)
                .or_default()
                .insert(name, property);
        }
        for (key, param, ty) in fixes {
            let sig = match key {
                SigId::Method(method) => {
                    let Some(class) = self.class_name_by_id(method.class()).cloned() else {
                        continue;
                    };
                    let Some(name) = self
                        .class_by_id(method.class())
                        .and_then(|info| info.member_order.get(usize::from(method.slot())))
                        .map(|entry| entry.name.clone())
                    else {
                        continue;
                    };
                    self.class_method_results
                        .get_mut(&method.class())
                        .and_then(|methods| methods.get_mut(&name))
                        .or_else(|| {
                            self.classes
                                .get_mut(&class)
                                .and_then(|info| info.methods.get_mut(&name))
                        })
                }
                SigId::Function(function) => {
                    let Some(name) = self.function_name_by_id(function).cloned() else {
                        continue;
                    };
                    self.functions.get_mut(&name)
                }
            };
            let Some(sig) = sig else { continue };
            match param {
                Some(i) => sig.params[i].ty = ty,
                None => sig.return_type = ty,
            }
        }
    }

    /// `ty` without the members `native` provably excludes, when any are
    /// excluded; `None` when nothing changes.
    fn excluding_native(
        &self,
        ty: &Type,
        native: &Type,
        class: Option<&Name>,
        pass: Option<u64>,
    ) -> Option<Type> {
        if ty == native || native.is_mixed() {
            return None;
        }
        fn bind<'t>(class: Option<&Name>, t: &'t Type) -> std::borrow::Cow<'t, Type> {
            match class {
                Some(c) if t.mentions_self_or_static() => {
                    std::borrow::Cow::Owned(t.bind_self(c).bind_static(c))
                }
                _ => std::borrow::Cow::Borrowed(t),
            }
        }
        let unbound_native = native;
        let native = bind(class, native);
        let members = ty.union_members();
        let native_members = unbound_native.union_members();
        let mut kept: Vec<Type> = Vec::new();
        let mut changed = false;
        for m in members {
            let bound = bind(class, m);
            let keep = self.keeps_member(&native, &bound, pass);
            if keep {
                kept.push(m.clone());
                continue;
            }
            changed = true;
            // A wider doc member (`Node` on a `?Class_` hint) still covers
            // the native alternatives it contains.
            for n in native_members {
                if self.is_super_type_of(&bound, &bind(class, n)) == types::TrinaryLogic::Yes
                    && !kept.contains(n)
                {
                    kept.push(n.clone());
                }
            }
        }
        if !changed {
            return None;
        }
        // Real PHPStan's `TypehintHelper::decideType`: a native
        // alternative the result doesn't cover at all is added back
        // (`?self` documented as `Schema|null` keeps its `self`).
        let covered = |kept: &[Type], n: &Type| {
            kept.iter().any(|k| {
                self.is_super_type_of(&bind(class, n), &bind(class, k)) != types::TrinaryLogic::No
            })
        };
        for n in native_members {
            if !covered(&kept, n) {
                kept.push(n.clone());
            }
        }
        Some(match kept.len() {
            0 => unbound_native.clone(),
            1 => kept.into_iter().next().unwrap(),
            _ => self.relation().union_of(kept),
        })
    }
}

impl Registry {
    /// Whether a documented member `bound` stays against the native type
    /// `native`: held by it, or not provably apart from it. Kept for the
    /// pass `pass` names, by the two types: the same pair of types comes
    /// again and again across the classes of a pass.
    fn keeps_member(&self, native: &Type, bound: &Type, pass: Option<u64>) -> bool {
        let decide = || match self.is_super_type_of(native, bound) {
            types::TrinaryLogic::Yes => true,
            types::TrinaryLogic::No => false,
            // Two fully known class names only partly overlapping: the
            // hint is what's guaranteed.
            types::TrinaryLogic::Maybe => {
                !(self.is_fully_known_class_type(bound) && self.is_fully_known_class_type(native))
            }
        };
        let Some(pass) = pass else {
            return decide();
        };
        let key = (native.identity(), bound.identity());
        let remembered = KEEP_DECISIONS.with(|memo| {
            let mut memo = memo.borrow_mut();
            if memo.0 != pass {
                *memo = (pass, FxHashMap::default());
            }
            memo.1.get(&key).map(|(_, _, keep)| *keep)
        });
        if let Some(keep) = remembered {
            return keep;
        }
        let keep = decide();
        KEEP_DECISIONS.with(|memo| {
            let mut memo = memo.borrow_mut();
            if memo.0 == pass {
                memo.1.insert(key, (native.clone(), bound.clone(), keep));
            }
        });
        keep
    }

    /// Whether `ty` is only class names (and `null`) whose whole
    /// hierarchy the registry indexed - no template, shape, or unknown
    /// class.
    fn is_fully_known_class_type(&self, ty: &Type) -> bool {
        ty.union_members().iter().all(|m| {
            m.is_null().yes()
                || m.as_object().is_some_and(|(name, _)| {
                    self.lookup_class(name).is_some()
                        && self
                            .all_ancestor_names(name)
                            .iter()
                            .all(|a| self.lookup_class(a).is_some())
                })
        })
    }
}

impl Registry {
    /// A PHPDoc name that isn't a class but spells a known global
    /// constant (`@param FTP_ASCII|FTP_BINARY $mode`) means that
    /// constant's value, as in real PHPStan's `TypeNodeResolver`
    /// (`mightBeConstant`/`tryResolveConstant`): the namespaced name
    /// first, then PHP's global fallback.
    pub(crate) fn resolve_constant_named_types(&mut self) {
        let lookup = |registry: &Registry, ty: &Type| resolve_constant_names(ty, registry);
        let registry: &Registry = self;
        let classes = self.scoped_classes();
        let functions = self.scoped_functions();
        let method_fixes: Vec<(MethodRef, FunctionReflection)> = classes
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map_iter(|(class, info)| {
                let Some(class_id) = registry.class_ids.get(*class).copied() else {
                    return Vec::new();
                };
                info.methods
                    .iter()
                    .filter_map(move |(name, sig)| {
                        let slot = *info.member_slots.get(name)?;
                        let fixed = fixed_sig(registry, sig, &lookup)?;
                        Some((MethodRef::new(class_id, slot), fixed))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let function_fixes: Vec<(FnId, FunctionReflection)> = functions
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .filter_map(|(name, sig)| {
                Some((
                    *registry.function_ids.get(*name)?,
                    fixed_sig(registry, sig, &lookup)?,
                ))
            })
            .collect();
        let property_fixes: Vec<(ClassId, Name, Arc<PropertyReflection>)> = classes
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .flat_map_iter(|(class, _info)| {
                let Some(class_id) = registry.class_ids.get(*class).copied() else {
                    return Vec::new();
                };
                registry
                    .property_names(class)
                    .filter_map(|name| {
                        let prop = registry.lookup_property(class, name)?;
                        let ty = lookup(registry, &prop.ty);
                        let doc = prop.doc_ty.as_ref().and_then(|d| lookup(registry, d));
                        (ty.is_some() || doc.is_some()).then(|| {
                            let mut updated = PropertyReflection::clone(prop);
                            if let Some(ty) = ty {
                                updated.ty = ty;
                            }
                            if doc.is_some() {
                                updated.doc_ty = doc;
                            }
                            (class_id, name.clone(), Arc::new(updated))
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        for (class_id, name, property) in property_fixes {
            self.class_property_results
                .entry(class_id)
                .or_default()
                .insert(name, property);
        }
        for (method, fixed) in method_fixes {
            let class_id = method.class();
            let Some(class) = self.class_name_by_id(class_id).cloned() else {
                continue;
            };
            let Some(name) = self
                .class_by_id(class_id)
                .and_then(|info| info.member_order.get(usize::from(method.slot())))
                .map(|entry| entry.name.clone())
            else {
                continue;
            };
            if let Some(sig) = self
                .classes
                .get_mut(&class)
                .and_then(|i| i.methods.get_mut(&name))
            {
                *sig = fixed;
            }
        }
        for (function, fixed) in function_fixes {
            let Some(name) = self.function_name_by_id(function).cloned() else {
                continue;
            };
            if let Some(sig) = self.functions.get_mut(&name) {
                *sig = fixed;
            }
        }
    }
}

/// `sig` with constant-named types resolved, when any are.
fn fixed_sig(
    registry: &Registry,
    sig: &FunctionReflection,
    lookup: &dyn Fn(&Registry, &Type) -> Option<Type>,
) -> Option<FunctionReflection> {
    let mut fixed: Option<FunctionReflection> = None;
    for (i, p) in sig.params.iter().enumerate() {
        if let Some(ty) = lookup(registry, &p.ty) {
            fixed.get_or_insert_with(|| sig.clone()).params[i].ty = ty;
        }
        if let Some(doc) = &p.doc_ty
            && let Some(ty) = lookup(registry, doc)
        {
            fixed.get_or_insert_with(|| sig.clone()).params[i].doc_ty = Some(ty);
        }
        if let Some(out) = &p.out_ty
            && let Some(ty) = lookup(registry, out)
        {
            fixed.get_or_insert_with(|| sig.clone()).params[i].out_ty = Some(ty);
        }
        if let Some(this) = &p.closure_this_type
            && let Some(ty) = lookup(registry, this)
        {
            fixed.get_or_insert_with(|| sig.clone()).params[i].closure_this_type = Some(ty);
        }
    }
    if let Some(ty) = lookup(registry, &sig.return_type) {
        fixed.get_or_insert_with(|| sig.clone()).return_type = ty;
    }
    if let Some(doc) = &sig.return_doc_ty
        && let Some(ty) = lookup(registry, doc)
    {
        fixed.get_or_insert_with(|| sig.clone()).return_doc_ty = Some(ty);
    }
    if let Some(out) = &sig.self_out
        && let Some(ty) = lookup(registry, out)
    {
        fixed.get_or_insert_with(|| sig.clone()).self_out = Some(ty);
    }
    fixed
}

/// `ty` with every constant-named `Object` resolved; `None` when there
/// is none.
fn resolve_constant_names(ty: &Type, registry: &Registry) -> Option<Type> {
    // Looked for first, without building anything: almost no type has one,
    // and rebuilding a type is interning every node of it again.
    if !ty.has_constant_looking_name() || !mentions_constant_name(ty, registry) {
        return None;
    }
    let mut changed = false;
    let out = resolve_constant_names_rec(ty, registry, &mut changed);
    changed.then_some(out)
}

/// Whether `ty` has a constant-named `Object` anywhere in it.
fn mentions_constant_name(ty: &Type, registry: &Registry) -> bool {
    if !ty.has_constant_looking_name() {
        return false;
    }
    syntax::stack::guard(|| {
        if let Some((name, args)) = ty.as_object()
            && args.is_empty()
            && constant_named_value(name, registry).is_some()
        {
            return true;
        }
        ty.any_child(|child| mentions_constant_name(child, registry))
    })
}

fn resolve_constant_names_rec(ty: &Type, registry: &Registry, changed: &mut bool) -> Type {
    if !ty.has_constant_looking_name() {
        return ty.clone();
    }
    syntax::stack::guard(|| {
        if let Some((name, args)) = ty.as_object()
            && args.is_empty()
            && let Some(value) = constant_named_type(name, registry)
        {
            *changed = true;
            return value;
        }
        ty.map_children(&mut |t| resolve_constant_names_rec(t, registry, changed))
    })
}

fn constant_named_type(name: &str, registry: &Registry) -> Option<Type> {
    constant_named_value(name, registry).cloned()
}

/// The type of the global constant a class-like name stands for, when it
/// is not a class and is spelled like a constant.
fn constant_named_value<'r>(name: &str, registry: &'r Registry) -> Option<&'r Type> {
    if !types::looks_like_constant_name(name) {
        return None;
    }
    let short = name.rsplit('\\').next().unwrap_or(name);
    // The constant is looked for first, being cheaper than a class lookup,
    // which most names the constants' table does not have never need.
    let bare = name.trim_start_matches('\\');
    let value = registry
        .global_constants()
        .get(bare)
        .or_else(|| registry.global_constants().get(short))?;
    if registry.lookup_class(name).is_some() {
        return None;
    }
    Some(value)
}

impl Registry {
    /// A top-level function signature built from one particular
    /// declaration (the registry keeps only one per name), put through
    /// the same per-signature passes as every indexed function: deferred
    /// value types, constant-named types, and native-excluded PHPDoc
    /// members.
    pub(crate) fn finalize_standalone_sig(
        &self,
        mut sig: FunctionReflection,
    ) -> FunctionReflection {
        let resolve = |ty: &mut Type, native: &Type| {
            if ty.contains_deferred() {
                let taken = std::mem::replace(ty, Type::mixed());
                *ty = resolve_deferred_value_of_with_native_fallback(
                    taken,
                    native,
                    &self.classes,
                    &self.class_ids,
                    &self.class_const_types,
                    &self.class_constant_results,
                    None,
                );
            }
        };
        for p in &mut sig.params {
            resolve(&mut p.ty, &p.ty_native);
            if let Some(doc) = &mut p.doc_ty {
                resolve(doc, &p.ty_native);
            }
        }
        resolve(&mut sig.return_type, &sig.return_type_native);
        if let Some(doc) = &mut sig.return_doc_ty {
            resolve(doc, &sig.return_type_native);
        }
        let lookup = |registry: &Registry, ty: &Type| resolve_constant_names(ty, registry);
        if let Some(fixed) = fixed_sig(self, &sig, &lookup) {
            sig = fixed;
        }
        for i in 0..sig.params.len() {
            if let Some(ty) =
                self.excluding_native(&sig.params[i].ty, &sig.params[i].ty_native, None, None)
            {
                sig.params[i].ty = ty;
            }
        }
        if let Some(ty) =
            self.excluding_native(&sig.return_type, &sig.return_type_native, None, None)
        {
            sig.return_type = ty;
        }
        sig
    }
}

/// What `Registry::keeps_member` decided in the pass it is for, by the
/// identity of the two types, which are held so that it stays theirs.
type KeepDecisions = FxHashMap<(usize, usize), (Type, Type, bool)>;

thread_local! {
    static KEEP_DECISIONS: std::cell::RefCell<(u64, KeepDecisions)> =
        std::cell::RefCell::new((0, FxHashMap::default()));
}

/// A number no pass of `drop_doc_members_excluded_by_native` has had.
fn next_keep_pass() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
