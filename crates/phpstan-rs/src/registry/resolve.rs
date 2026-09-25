use indexmap::IndexMap;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

use crate::registry::Registry;
use crate::registry::function_sig;
use crate::registry::{ClassInfo, ConstSig, FunctionSig, PropertySig, substitute_type_aliases};
use crate::types::{self, Type};
use syntax::ast::{ClassKind, Visibility};

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
    /// canonicalizes every reference-shaped field on each `ClassInfo`
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
    /// rewrite touches only its own `ClassInfo` and reads only the
    /// immutable, already-fully-built `canonical_by_lower` table - no
    /// other class is ever read or written, so `values_mut()` directly
    /// (no remove/reinsert dance) is sound to run concurrently.
    #[hotpath::measure]
    pub(crate) fn canonicalize_class_name_case(&mut self) {
        let mut canonical_by_lower: FxHashMap<String, Arc<str>> = FxHashMap::default();
        for key in self.classes.keys() {
            canonical_by_lower
                .entry(key.to_ascii_lowercase())
                .or_insert_with(|| key.clone());
        }
        // Persisted as `self.classes_lowercase` too - `Registry::
        // lookup_class`'s doc comment explains why its linear-scan
        // fallback uses this instead of a fresh scan on every call. Built
        // fresh every time this pass runs, so it can never go stale
        // independently of this pass's own call sites.
        self.classes_lowercase = canonical_by_lower
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let canonicalize = |name: &mut Arc<str>| {
            if let Some(canonical) = canonical_by_lower.get(&name.to_ascii_lowercase())
                && canonical.as_ref() != name.as_ref()
            {
                *name = canonical.clone();
            }
        };
        self.classes
            .values_mut()
            .collect::<Vec<_>>()
            .into_par_iter()
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
        let max_iterations = self.classes.len().max(1);
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
    /// self` again) - the actual cost here (cloning every trait method/
    /// property signature into `methods`/`properties`) is what scales
    /// with corpus size, not the cheap, still-sequential apply step.
    fn flatten_traits_one_pass(&mut self) -> bool {
        type TraitAddition = (
            Arc<str>,
            Vec<(Arc<str>, FunctionSig)>,
            Vec<(Arc<str>, PropertySig)>,
            Vec<(Arc<str>, ConstSig)>,
            FxHashSet<Arc<str>>,
            FxHashSet<Arc<str>>,
        );

        let class_names: Vec<Arc<str>> = self.classes.keys().cloned().collect();
        let registry: &Registry = self;
        let additions: Vec<TraitAddition> = class_names
            .par_iter()
            .filter_map(|class_name| {
                let uses = registry.classes[class_name].uses.clone();
                let trait_aliases = registry.classes[class_name].trait_aliases.clone();
                if uses.is_empty() && trait_aliases.is_empty() {
                    return None;
                }
                // A trait can declare its own `abstract` method (the using
                // class is required to supply it) - a using class's own
                // directly-declared method for that same name (checked here,
                // before any trait's contribution is applied) always wins,
                // matching the same `or_insert` "first/own wins" semantics
                // `methods` itself already uses below.
                let own_method_names: FxHashSet<Arc<str>> = registry.classes[class_name]
                    .methods
                    .keys()
                    .cloned()
                    .collect();
                let mut methods = Vec::new();
                let mut properties = Vec::new();
                let mut consts = Vec::new();
                let mut abstract_from_traits: FxHashSet<Arc<str>> = FxHashSet::default();
                // `static_methods` needs the identical trait-flattening
                // treatment `abstract_from_traits` gets below - without
                // it, a using class's own `static_methods` set would
                // never learn about a trait-supplied static method it
                // doesn't also redeclare itself, even though that
                // method's (correctly flattened-in) `FunctionSig` is
                // right there in `info.methods`.
                let mut static_from_traits: FxHashSet<Arc<str>> = FxHashSet::default();
                for trait_name in &uses {
                    if let Some(trait_info) = registry.classes.get(trait_name) {
                        for (name, sig) in &trait_info.methods {
                            methods.push((name.clone(), sig.clone()));
                            if trait_info.abstract_methods.contains(name)
                                && !own_method_names.contains(name)
                            {
                                abstract_from_traits.insert(name.clone());
                            }
                            if trait_info.static_methods.contains(name)
                                && !own_method_names.contains(name)
                            {
                                static_from_traits.insert(name.clone());
                            }
                        }
                        properties.extend(
                            trait_info
                                .properties
                                .iter()
                                .map(|(n, s)| (n.clone(), s.clone())),
                        );
                        // PHP 8.2+ trait constants - composed into the
                        // using class exactly like a trait's methods/
                        // properties already are just above: a `self::`
                        // reference inside a trait-defined method's body
                        // resolves against whichever class actually
                        // composes the trait (`current_class` is set to
                        // the host in `check_trait_methods`, once per
                        // host), so the constant needs to actually be
                        // reachable through that host's own `consts` map
                        // for `Registry::lookup_const`/`lookup_const_
                        // with_owner` to ever find it - without this, a
                        // trait's own `self::CONST` reference from
                        // inside its own body silently read as
                        // `classConstant.notFound` against every one of
                        // its hosts, since the constant was never
                        // anywhere but the trait's own, otherwise-
                        // unreachable registry entry.
                        consts.extend(
                            trait_info
                                .consts
                                .iter()
                                .map(|(n, s)| (n.clone(), s.clone())),
                        );
                    }
                }
                // `use A, B { B::hello as helloB; }` - registers the
                // trait method under its new alias too, alongside its
                // original name (both stay callable in real PHP). A bare
                // `method as newName;` with no `TraitName::` prefix (real,
                // valid PHP when the name is unambiguous among the traits
                // used) resolves against the first used trait that
                // declares it.
                for alias in &trait_aliases {
                    let source_sig = match &alias.trait_name {
                        Some(t) => registry
                            .classes
                            .get(t)
                            .and_then(|i| i.methods.get(&alias.method)),
                        None => uses
                            .iter()
                            .find_map(|t| registry.classes.get(t)?.methods.get(&alias.method)),
                    };
                    if let Some(sig) = source_sig {
                        methods.push((alias.alias.clone(), sig.clone()));
                    }
                }
                Some((
                    class_name.clone(),
                    methods,
                    properties,
                    consts,
                    abstract_from_traits,
                    static_from_traits,
                ))
            })
            .collect();

        let mut changed = false;
        for (class_name, methods, properties, consts, abstract_from_traits, static_from_traits) in
            additions
        {
            if let Some(info) = self.classes.get_mut(&class_name) {
                for (name, sig) in methods {
                    if let std::collections::hash_map::Entry::Vacant(slot) =
                        info.methods.entry(name)
                    {
                        slot.insert(sig);
                        changed = true;
                    }
                }
                for (name, sig) in properties {
                    if let std::collections::hash_map::Entry::Vacant(slot) =
                        info.properties.entry(name)
                    {
                        slot.insert(sig);
                        changed = true;
                    }
                }
                for (name, sig) in consts {
                    if let indexmap::map::Entry::Vacant(slot) = info.consts.entry(name) {
                        slot.insert(sig);
                        changed = true;
                    }
                }
                let before = info.abstract_methods.len();
                info.abstract_methods.extend(abstract_from_traits);
                if info.abstract_methods.len() != before {
                    changed = true;
                }
                let before = info.static_methods.len();
                info.static_methods.extend(static_from_traits);
                if info.static_methods.len() != before {
                    changed = true;
                }
            }
        }
        changed
    }

    /// `@method`'s deferred insertion - `ClassInfo::method_tags`'s
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
    /// Fully self-contained per class (`info.method_tags` -> `info.
    /// methods`, on that same `info` only) - no other class is ever read
    /// or written, so this parallelizes with no read phase to separate
    /// out first, unlike `flatten_traits`/`flatten_phpdoc_inheritance`
    /// below (which do need one, since they read other classes).
    #[hotpath::measure]
    pub(crate) fn resolve_method_tags(&mut self) {
        self.classes
            .values_mut()
            .collect::<Vec<_>>()
            .into_par_iter()
            .for_each(|info| {
                for (name, sig, is_static) in std::mem::take(&mut info.method_tags) {
                    if let std::collections::hash_map::Entry::Vacant(slot) =
                        info.methods.entry(name.clone())
                    {
                        slot.insert(sig);
                        if is_static {
                            info.static_methods.insert(name);
                        }
                    }
                }
            });
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
        let max_iterations = self.classes.len().max(1);
        for _ in 0..max_iterations {
            if !self.flatten_phpdoc_inheritance_one_pass() {
                break;
            }
        }
    }

    /// Resolves every `Type::ValueOf`/`Type::ClassConstant` (`types::
    /// parse_type_keyword_or_class`'s `value-of<T>`-on-a-class/enum, and
    /// `Foo::BAR` references) now that every class/enum is
    /// actually indexed - see that function's doc comment for why this
    /// can't happen at parse time. A single pass, not a fixed-point loop
    /// like `flatten_phpdoc_inheritance` above: a `value-of<value-of<X>>`-
    /// style chain isn't attempted.
    ///
    /// Every class is taken out of `self.classes` before being mutated
    /// (`HashMap::remove`, reinserted at the end of its own iteration),
    /// not the collect-then-apply shape `flatten_phpdoc_inheritance_
    /// one_pass` needs - that one's shape exists to let a fixed-point
    /// loop read an ancestor's still-unmodified signature while
    /// backfilling a descendant's; this pass only needs to read some
    /// other class/enum's `properties["value"]` while rewriting the
    /// current one's types, and temporarily removing the current entry
    /// sidesteps borrowing `self.classes` both ways at once. (A class
    /// whose own docblock names `value-of<self>` on itself, where `self`
    /// is genuinely an enum, degrades to `Mixed` while it's temporarily
    /// removed - the same "can't verify, don't guess" outcome every
    /// other unresolvable case here gets.) Top-level functions
    /// (`self.functions`) need no such dance at all - mutating one
    /// function's signature while reading `self.classes` for a lookup
    /// borrows two disjoint fields of `self`, which the borrow checker
    /// already allows without help.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_value_of(&mut self) {
        self.resolve_class_constant_references();
        let class_names: Vec<Arc<str>> = self.classes.keys().cloned().collect();
        for class_name in class_names {
            // Only a class with a deferred sentinel somewhere needs its
            // inherited-constant view (the overwhelming majority, stubs
            // included, have none).
            let own_consts = match self.classes.get(&class_name) {
                Some(info) if class_has_deferred_type(info) => {
                    visible_class_consts(&class_name, &self.classes)
                }
                Some(_) => continue,
                None => continue,
            };
            let Some(mut info) = self.classes.remove(&class_name) else {
                continue;
            };
            let resolve = |ty: &mut Type, native: &Type, classes: &_| {
                if type_contains_deferred_value_of(ty) {
                    let bound = bind_class_const_self(
                        std::mem::replace(ty, Type::Mixed(false)),
                        &class_name,
                        &own_consts,
                    );
                    *ty = resolve_deferred_value_of_with_native_fallback(bound, native, classes);
                }
            };
            for sig in info.methods.values_mut() {
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
                    resolve(out, &Type::Mixed(false), &self.classes);
                }
            }
            for prop in info.properties.values_mut() {
                if type_contains_deferred_value_of(&prop.ty) {
                    let ty = bind_class_const_self(
                        std::mem::replace(&mut prop.ty, Type::Mixed(false)),
                        &class_name,
                        &own_consts,
                    );
                    prop.ty = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
                if let Some(doc) = &mut prop.doc_ty
                    && type_contains_deferred_value_of(doc)
                {
                    let ty = bind_class_const_self(
                        std::mem::replace(doc, Type::Mixed(false)),
                        &class_name,
                        &own_consts,
                    );
                    *doc = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
                if let Some(set_ty) = &prop.set_ty
                    && type_contains_deferred_value_of(set_ty)
                {
                    let ty = bind_class_const_self(
                        prop.set_ty.take().unwrap(),
                        &class_name,
                        &own_consts,
                    );
                    prop.set_ty = Some(resolve_deferred_value_of_in_type(ty, &self.classes));
                }
            }
            for c in info.consts.values_mut() {
                if type_contains_deferred_value_of(&c.ty) {
                    let ty = bind_class_const_self(
                        std::mem::replace(&mut c.ty, Type::Mixed(false)),
                        &class_name,
                        &own_consts,
                    );
                    c.ty = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            for bound in info.template_bounds.values_mut() {
                if type_contains_deferred_value_of(bound) {
                    let ty = bind_class_const_self(
                        std::mem::replace(bound, Type::Mixed(false)),
                        &class_name,
                        &own_consts,
                    );
                    *bound = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            // `@extends Lexer<self::T_*>`: what inherited PHPDoc binds to.
            let ancestor_args = info
                .extends_args
                .iter_mut()
                .chain(info.implements_args.values_mut().flatten());
            for arg in ancestor_args {
                if type_contains_deferred_value_of(arg) {
                    let ty = bind_class_const_self(
                        std::mem::replace(arg, Type::Mixed(false)),
                        &class_name,
                        &own_consts,
                    );
                    *arg = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            self.classes.insert(class_name, info);
        }
        let resolve = |ty: &mut Type, native: &Type, classes: &_| {
            if type_contains_deferred_value_of(ty) {
                let taken = std::mem::replace(ty, Type::Mixed(false));
                *ty = resolve_deferred_value_of_with_native_fallback(taken, native, classes);
            }
        };
        for sig in self.functions.values_mut() {
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
        for _ in 0..MAX_ROUNDS {
            let mut updates: Vec<(Arc<str>, Arc<str>, Type)> = Vec::new();
            for (class_name, info) in &self.classes {
                if !info
                    .consts
                    .values()
                    .any(|sig| type_contains_deferred_value_of(&sig.ty))
                {
                    continue;
                }
                let own_consts = visible_class_consts(class_name, &self.classes);
                for (const_name, sig) in &info.consts {
                    if !type_contains_deferred_value_of(&sig.ty) {
                        continue;
                    }
                    let step = resolve_deferred_value_of_in_type(
                        bind_class_const_self(sig.ty.clone(), class_name, &own_consts),
                        &self.classes,
                    );
                    if step != sig.ty {
                        updates.push((class_name.clone(), const_name.clone(), step));
                    }
                }
            }
            if updates.is_empty() {
                break;
            }
            for (class_name, const_name, ty) in updates {
                if let Some(sig) = self
                    .classes
                    .get_mut(&class_name)
                    .and_then(|info| info.consts.get_mut(&const_name))
                {
                    sig.ty = ty;
                }
            }
        }
        for info in self.classes.values_mut() {
            for sig in info.consts.values_mut() {
                if type_contains_deferred_value_of(&sig.ty) {
                    sig.ty = unresolved_class_const_to_mixed(std::mem::replace(
                        &mut sig.ty,
                        Type::Mixed(false),
                    ));
                }
            }
        }
    }

    /// `@phpstan-import-type`'s deferred half - `ClassInfo::from_decl`
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
        let mut classes_with_imports: Vec<Arc<str>> = Vec::new();
        let mut resolved: Vec<(Arc<str>, Arc<str>, Type)> = Vec::new();
        for (class_name, info) in &self.classes {
            if info.type_imports.is_empty() {
                continue;
            }
            classes_with_imports.push(class_name.clone());
            for imp in &info.type_imports {
                let source_key = declared_name_in_namespace(
                    class_namespace(&imp.source_class_fqn),
                    &imp.source_alias_name,
                );
                if let Some(ty) = self
                    .classes
                    .get(&imp.source_class_fqn)
                    .and_then(|src| src.type_aliases.get(source_key.as_str()))
                {
                    resolved.push((class_name.clone(), imp.local_key.clone(), ty.clone()));
                }
            }
        }
        if classes_with_imports.is_empty() {
            return;
        }
        let mut by_class: FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>> = FxHashMap::default();
        for (class_name, local_key, ty) in resolved {
            by_class
                .entry(class_name)
                .or_default()
                .insert(local_key, ty);
        }
        for class_name in classes_with_imports {
            let Some(mut info) = self.classes.remove(&class_name) else {
                continue;
            };
            // A class whose imports all failed to resolve (its source
            // class isn't indexed at all) still needs `type_imports`
            // cleared - the one-shot pass above already ran, so there's
            // no second chance coming; an empty `aliases` map here makes
            // every `substitute_type_aliases` call below a no-op.
            let aliases = by_class.remove(&class_name).unwrap_or_default();
            let substitute = |ty: &mut Type| {
                let taken = std::mem::replace(ty, Type::Mixed(false));
                *ty = substitute_type_aliases(taken, &aliases);
            };
            for sig in info.methods.values_mut() {
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
            }
            for prop in info.properties.values_mut() {
                let ty = std::mem::replace(&mut prop.ty, Type::Mixed(false));
                prop.ty = substitute_type_aliases(ty, &aliases);
                if let Some(doc) = &mut prop.doc_ty {
                    substitute(doc);
                }
                if let Some(set_ty) = prop.set_ty.take() {
                    prop.set_ty = Some(substitute_type_aliases(set_ty, &aliases));
                }
            }
            for c in info.consts.values_mut() {
                let ty = std::mem::replace(&mut c.ty, Type::Mixed(false));
                c.ty = substitute_type_aliases(ty, &aliases);
            }
            info.type_aliases.extend(aliases);
            info.type_imports.clear();
            self.classes.insert(class_name, info);
        }
    }

    /// `@mixin`'s deferred half - mirrors `resolve_deferred_type_imports`'s
    /// exact two-step shape (a read-only collection pass, then a second
    /// pass applying it): the mixin target may not be indexed yet at
    /// `ClassInfo::from_decl` time.
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
        let mut method_additions: Vec<(Arc<str>, Arc<str>, FunctionSig)> = Vec::new();
        let mut property_additions: Vec<(Arc<str>, Arc<str>, PropertySig)> = Vec::new();
        for (class_name, info) in &self.classes {
            for mixin_fqn in &info.mixins {
                let Some(mixin_info) = self.classes.get(mixin_fqn) else {
                    continue;
                };
                for (name, sig) in &mixin_info.methods {
                    if sig.visibility == Visibility::Public && !info.methods.contains_key(name) {
                        method_additions.push((class_name.clone(), name.clone(), sig.clone()));
                    }
                }
                for (name, sig) in &mixin_info.properties {
                    if sig.visibility == Visibility::Public && !info.properties.contains_key(name) {
                        property_additions.push((class_name.clone(), name.clone(), sig.clone()));
                    }
                }
            }
        }
        for (class_name, name, sig) in method_additions {
            if let Some(info) = self.classes.get_mut(&class_name) {
                info.methods.entry(name).or_insert(sig);
            }
        }
        for (class_name, name, sig) in property_additions {
            if let Some(info) = self.classes.get_mut(&class_name) {
                info.properties.entry(name).or_insert(sig);
            }
        }
    }

    /// The `&self` (read-only, immediate) twin of `resolve_deferred_
    /// value_of` above, for a `Type` parsed directly from a raw docblock
    /// string during checking rather than at registry-build time - by
    /// then every class/enum genuinely is already fully indexed, so this
    /// resolves a `ValueOf`/`ClassConstant` immediately, inline, with no
    /// pass needed. Every checker-side call site that parses a docblock
    /// type string independently of the registry's own
    /// `FunctionSig::from_decl`/`ClassInfo::from_decl` pipeline (which
    /// `resolve_deferred_value_of` covers) needs this too - e.g.
    /// `Checker::resolve_type_hint`, reached through an inline `@var`
    /// annotation, would otherwise leak the raw, unresolved sentinel
    /// straight into a diagnostic message.
    pub(crate) fn resolve_deferred_value_of_now(&self, ty: Type) -> Type {
        if !type_contains_deferred_value_of(&ty) {
            return ty;
        }
        resolve_deferred_value_of_in_type(ty, &self.classes)
    }

    /// `resolve_deferred_value_of_now`'s native-fallback-aware sibling.
    /// See `resolve_deferred_value_of_with_native_fallback`'s doc comment
    /// (the free function this wraps) for why an unresolvable
    /// `value-of<T>` needs this rather than plain
    /// `resolve_deferred_value_of_now` alone. Used by the anonymous-class
    /// param/return docblock fallback in `checker.rs`, which (like the
    /// registry's `FunctionSig::from_decl`) reconciles its raw-parsed
    /// doc type against a native counterpart via `resolve_type` before
    /// this ever runs.
    pub(crate) fn resolve_deferred_value_of_now_with_native_fallback(
        &self,
        ty: Type,
        native: &Type,
    ) -> Type {
        if !type_contains_deferred_value_of(&ty) {
            return ty;
        }
        resolve_deferred_value_of_with_native_fallback(ty, native, &self.classes)
    }

    /// Whether `ty` mentions a `Type::TemplateParam` scoped anywhere
    /// other than `method_name` - mirrors `function_sig::
    /// mark_template_params`'s recursive-case list (`Object`'s type
    /// args, `Array`'s key/value, `ClassString`, `Union`/`Intersection`,
    /// `ArrayShape`/`ObjectShape` fields). `flatten_phpdoc_inheritance_
    /// one_pass` uses this to refuse backfilling a foreign-scoped
    /// template-parameterized ancestor type - genuinely unsafe to
    /// backfill because it's scoped to the ancestor's own enclosing
    /// class (meaningful only via unmodeled `@extends Base<Concrete>`
    /// cross-inheritance binding, which this project doesn't model).
    ///
    /// As opposed to a `@template` declared directly on the ancestor
    /// method itself (`function_sig::from_decl`'s `build_template_params(d,
    /// &f.name, ctx)` stamps `scope: Some(f.name)` for exactly this case,
    /// vs. `class_info.rs`'s `class_scope = ctx.declared_name(&c.name)`
    /// for a class-level one - an FQN vs. a bare method name,
    /// structurally distinguishable). A method-scoped template is safe
    /// to copy through verbatim: `types::bind_template_param`/
    /// `substitute_template`'s call-site binding is purely name-keyed
    /// and blind to which class's `FunctionSig` a signature came from.
    /// Real PHPStan draws the identical line:
    /// `ResolvedPhpDocBlock::resolveTemplateTypeInTag` substitutes an
    /// ancestor tag's type through
    /// `$parentClass->getActiveTemplateTypeMap()`, which only ever holds
    /// class-level bindings - a method-scoped `@template` is never a
    /// member of it, so that substitution is a no-op for this shape, and
    /// real PHPStan's own merge leaves it untouched too.
    pub(crate) fn type_mentions_foreign_scoped_template_param(
        ty: &Type,
        method_name: &str,
    ) -> bool {
        match ty {
            Type::TemplateParam(tp) => !matches!(
                tp.scope.as_deref(),
                Some(s) if s == method_name || s == crate::types::CALLABLE_TEMPLATE_SCOPE
            ),
            _ => ty.any_child(|t| {
                Registry::type_mentions_foreign_scoped_template_param(t, method_name)
            }),
        }
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
    pub(crate) fn template_param_scopes_all_safe_to_backfill(
        ty: &Type,
        method_name: &str,
        ancestor_bindings: &FxHashMap<Arc<str>, FxHashMap<String, Type>>,
    ) -> bool {
        match ty {
            Type::TemplateParam(tp) => {
                tp.scope.as_deref() == Some(method_name)
                    || tp.scope.as_deref() == Some(crate::types::CALLABLE_TEMPLATE_SCOPE)
                    || tp
                        .scope
                        .as_deref()
                        .is_some_and(|s| ancestor_bindings.get(s).is_some_and(|b| !b.is_empty()))
            }
            _ => !ty.any_child(|t| {
                !Registry::template_param_scopes_all_safe_to_backfill(
                    t,
                    method_name,
                    ancestor_bindings,
                )
            }),
        }
    }

    /// Whether backfilling `ancestor_ty` onto a position whose own native
    /// hint alone already resolves to `native_ty` is safe: `false` only
    /// when both sides are a concrete `Type::Object` naming *different*
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
        match (ancestor_ty, native_ty) {
            (Type::Object(a, _), Type::Object(n, _)) => a == n,
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
        *ancestor_ty != Type::Mixed(false) || *native_ty == Type::Mixed(false)
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
    fn flatten_phpdoc_inheritance_one_pass(&mut self) -> bool {
        type Backfill = (
            Arc<str>,
            Arc<str>,
            Vec<(usize, Type, bool)>,
            Option<(Type, bool)>,
        );

        let class_names: Vec<Arc<str>> = self.classes.keys().cloned().collect();
        let registry: &Registry = self;
        let backfills: Vec<Backfill> = class_names
            .par_iter()
            .flat_map(|class_name| {
                let mut per_class: Vec<Backfill> = Vec::new();
                let Some(info) = registry.classes.get(class_name) else {
                    return per_class;
                };
                for (method_name, sig) in &info.methods {
                    let needs_param = sig.params.iter().any(|p| !p.has_own_doc);
                    let needs_return = !sig.return_has_own_doc;
                    if !needs_param && !needs_return {
                        continue;
                    }
                    let Some((owner, ancestor_sig)) =
                        registry.lookup_ancestor_method_with_owner(class_name, method_name)
                    else {
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
                    // at call time later - computed once per method
                    // (bindings don't depend on which method's signature
                    // is being backfilled), not per backfill call site
                    // below. See `template_param_scopes_all_safe_to_
                    // backfill`'s doc comment for why this must be the
                    // real resolved bindings, not merely "is this
                    // ancestor name reachable at all.""
                    // `class_name`'s own templates stay templates (scoped
                    // to it) through the ancestor mapping, so an inherited
                    // `list<T>` becomes `list<CMTemplate>` for a
                    // `@template-implements Factory<CMTemplate>` class
                    // rather than losing `T` to `Mixed`.
                    let own_args = info.own_template_args(class_name);
                    let ancestor_bindings: FxHashMap<Arc<str>, FxHashMap<String, Type>> = registry
                        .all_ancestor_names(class_name)
                        .into_iter()
                        .filter(|a| {
                            registry
                                .classes
                                .get(a)
                                .is_some_and(|c| !c.templates.is_empty())
                        })
                        .map(|a| {
                            let bindings =
                                registry.class_template_bindings(class_name, &own_args, &a);
                            (a, bindings)
                        })
                        .collect();
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
                    if method_name.as_ref() == "__construct"
                        && registry.built_in_stub_classes.contains(&owner)
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
                        if method_name.as_ref() == "__construct" {
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
                                        function_sig::reconcile_with_native(
                                            Registry::substitute_class_scoped_ancestor_templates(
                                                &bind_self_ref(&ancestor_param.ty, &owner),
                                                &ancestor_bindings,
                                            ),
                                            p.ty_native.clone(),
                                        ),
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
                                        function_sig::reconcile_with_native(
                                            Registry::substitute_class_scoped_ancestor_templates(
                                                &bind_self_ref(&ancestor_param.ty, &owner),
                                                &ancestor_bindings,
                                            ),
                                            p.ty_native.clone(),
                                        ),
                                        ancestor_param.explicit_mixed_hint,
                                    ));
                                }
                            }
                        }
                    }
                    // `safe_to_backfill_mixed` (both here and in the
                    // param loops above) isn't about `__construct` at
                    // all - see its doc comment.
                    let return_fill = (needs_return
                        && ancestor_sig.return_has_own_doc
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
                            function_sig::reconcile_with_native(
                                Registry::substitute_class_scoped_ancestor_templates(
                                    &bind_self_ref(&ancestor_sig.return_type, &owner),
                                    &ancestor_bindings,
                                ),
                                sig.return_type_native.clone(),
                            ),
                            ancestor_sig.return_explicit_mixed_hint,
                        )
                    });
                    if !param_fills.is_empty() || return_fill.is_some() {
                        per_class.push((
                            class_name.clone(),
                            method_name.clone(),
                            param_fills,
                            return_fill,
                        ));
                    }
                }
                per_class
            })
            .collect();

        let mut changed = false;
        for (class_name, method_name, param_fills, return_fill) in backfills {
            if let Some(sig) = self
                .classes
                .get_mut(&class_name)
                .and_then(|info| info.methods.get_mut(&method_name))
            {
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

/// `NameContext::declared_name`'s exact logic, replicated for a namespace
/// string reconstructed after the fact (`class_namespace`) rather than a
/// live `NameContext` - no leading `\` for the global namespace, which a
/// naive `format!("{ns}\\{bare}")` would always insert: an alias declared
/// with no namespace at all keys its `type_aliases` entry under the bare
/// name, never `"\\Name"`.
fn declared_name_in_namespace(namespace: &str, bare: &str) -> String {
    if namespace.is_empty() {
        bare.to_string()
    } else {
        format!("{namespace}\\{bare}")
    }
}

/// Cheap, read-only pre-check for `Registry::resolve_deferred_value_of`:
/// does `ty` contain a `ValueOf`/`ClassConstant` anywhere in its tree - lets
/// every call site skip the much more expensive rebuild-with-clone
/// `resolve_deferred_value_of_in_type` does for the overwhelming majority
/// of types, which never contain it.
pub(crate) fn type_contains_deferred_value_of(ty: &Type) -> bool {
    match ty {
        Type::ValueOf(_) | Type::KeyOf(_) | Type::OffsetAccess(..) | Type::ClassConstant { .. } => {
            true
        }
        _ => ty.any_child(type_contains_deferred_value_of),
    }
}

/// The actual substitution: every `ValueOf`/`ClassConstant` in `ty`, at
/// any depth. See `Registry::resolve_deferred_value_of`'s doc comment
/// for why this exists, and `types::parse_type_keyword_or_class`'s
/// `"value-of"` arm for where a `ValueOf` is produced.
fn resolve_deferred_value_of_in_type(ty: Type, classes: &FxHashMap<Arc<str>, ClassInfo>) -> Type {
    match ty {
        Type::ValueOf(target) => match *target {
            target @ Type::ClassConstant { .. } => {
                value_of_type(resolve_deferred_value_of_in_type(target, classes))
            }
            Type::Object(target_fqn, _) => resolve_value_of_target(&target_fqn, classes),
            _ => Type::Mixed(false),
        },
        Type::ClassConstant { class, pattern } => {
            resolve_class_const_type(&class, &pattern, classes)
        }
        late @ (Type::KeyOf(_) | Type::OffsetAccess(..)) => crate::types::settle(
            late.map_children(&mut |t| resolve_deferred_value_of_in_type(t.clone(), classes)),
        ),
        other => other.map_children(&mut |t| resolve_deferred_value_of_in_type(t.clone(), classes)),
    }
}

/// A `ClassConstant`'s value: the union of every matching
/// constant's own type (an enum case's `Type::EnumCase`, a literal
/// constant's literal type), mirroring real PHPStan's `TypeNodeResolver`
/// for a `ConstTypeNode` - a `*` in the name matches any run of
/// characters. Matches are ordered by name for a deterministic union
/// (real PHPStan uses declaration order, which `ClassInfo::consts` doesn't
/// keep). An unknown class, or no match, degrades to `Mixed`. Only the
/// class's own constants are consulted, not inherited ones. A `self`/
/// `static` reference has already been bound to the owning class by
/// `bind_class_const_self`, or stays unresolved (`Mixed`) where no owning
/// class exists.
fn resolve_class_const_type(
    class: &Type,
    pattern: &str,
    classes: &FxHashMap<Arc<str>, ClassInfo>,
) -> Type {
    let Type::Object(fqn, _) = class else {
        return Type::Mixed(false);
    };
    let fqn = fqn.trim_start_matches('\\');
    if !classes.contains_key(fqn) {
        return Type::Mixed(false);
    }
    matching_class_const_type(&visible_class_consts(fqn, classes), pattern)
}

/// Whether any type `resolve_deferred_value_of` rewrites on `info`
/// contains a deferred sentinel - lets that pass skip a class outright.
fn class_has_deferred_type(info: &ClassInfo) -> bool {
    info.methods.values().any(|sig| {
        type_contains_deferred_value_of(&sig.return_type)
            || [&sig.return_doc_ty, &sig.self_out]
                .into_iter()
                .flatten()
                .any(type_contains_deferred_value_of)
            || sig.params.iter().any(|p| {
                type_contains_deferred_value_of(&p.ty)
                    || [&p.doc_ty, &p.out_ty, &p.closure_this_type]
                        .into_iter()
                        .flatten()
                        .any(type_contains_deferred_value_of)
            })
    }) || info
        .extends_args
        .iter()
        .chain(info.implements_args.values().flatten())
        .any(type_contains_deferred_value_of)
        || info.properties.values().any(|p| {
            type_contains_deferred_value_of(&p.ty)
                || p.doc_ty
                    .as_ref()
                    .is_some_and(type_contains_deferred_value_of)
                || p.set_ty
                    .as_ref()
                    .is_some_and(type_contains_deferred_value_of)
        })
        || info
            .consts
            .values()
            .any(|c| type_contains_deferred_value_of(&c.ty))
        || info
            .template_bounds
            .values()
            .any(type_contains_deferred_value_of)
}

/// Every constant visible on `fqn` - its own, then each parent's and
/// interface's not already shadowed - the set real PHPStan's
/// `ClassReflection::getConstants()` resolves a `Foo::*` against. Walked
/// breadth-first, a class's own declaration winning over an inherited one
/// of the same name.
fn visible_class_consts(
    fqn: &str,
    classes: &FxHashMap<Arc<str>, ClassInfo>,
) -> IndexMap<Arc<str>, ConstSig> {
    let mut consts: IndexMap<Arc<str>, ConstSig> = IndexMap::new();
    let mut seen: FxHashSet<Arc<str>> = FxHashSet::default();
    let mut queue: std::collections::VecDeque<Arc<str>> = std::collections::VecDeque::new();
    queue.push_back(Arc::from(fqn));
    while let Some(name) = queue.pop_front() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(info) = classes.get(&name) else {
            continue;
        };
        for (const_name, sig) in &info.consts {
            consts
                .entry(const_name.clone())
                .or_insert_with(|| sig.clone());
        }
        queue.extend(info.extends.iter().cloned());
        queue.extend(info.implements.iter().cloned());
    }
    consts
}

/// The union of every constant in `consts` whose name matches `pattern` -
/// see `resolve_class_const_type`.
fn matching_class_const_type(consts: &IndexMap<Arc<str>, ConstSig>, pattern: &str) -> Type {
    let names: Vec<&Arc<str>> = consts
        .keys()
        .filter(|name| glob_matches(pattern, name))
        .collect();
    let mut members: Vec<Type> = Vec::new();
    for name in names {
        let ty = consts[name].ty.clone();
        if !members.contains(&ty) {
            members.push(ty);
        }
    }
    match members.len() {
        0 => Type::Mixed(false),
        1 => members.pop().expect("len checked"),
        _ => Type::Union(members),
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

/// `value-of<T>` for an already-resolved `T` that isn't a bare enum
/// reference (`value-of<Foo::VALUES>`, a constant holding an array): the
/// array's value type, the union of an array shape's real fields, or
/// `Mixed` for anything else.
fn value_of_type(ty: Type) -> Type {
    match ty {
        Type::Array(_, value) => *value,
        // `list<V>`/`non-empty-array<V>` - the `Array` member carries `V`.
        Type::Intersection(members) => members
            .into_iter()
            .find(|m| matches!(m, Type::Array(..)))
            .map_or(Type::Mixed(false), value_of_type),
        Type::ArrayShape(shape) => {
            let mut members: Vec<Type> = Vec::new();
            for f in shape.fields {
                if !members.contains(&f.ty) {
                    members.push(f.ty);
                }
            }
            match members.len() {
                0 => Type::Never,
                1 => members.pop().expect("len checked"),
                _ => Type::Union(members),
            }
        }
        _ => Type::Mixed(false),
    }
}

/// Resolves every `ClassConstant` in `ty` that names `class`
/// itself (`self::X`, `static::X`, or `class`'s own name) straight from
/// `own_consts`. `resolve_deferred_value_of` takes the class being
/// rewritten out of the registry's class map while rewriting it, so
/// `resolve_deferred_value_of_in_type` can't find a class's own constants
/// there - this runs first, for the one class that's missing.
fn bind_class_const_self(ty: Type, class: &str, own_consts: &IndexMap<Arc<str>, ConstSig>) -> Type {
    bind_class_const_self_at(ty, class, own_consts, 0)
}

/// `bind_class_const_self`'s body. A substituted constant type can itself
/// hold a further own-class reference (`A = self::B`), so each
/// substitution is walked again, `depth` bounding a reference cycle
/// (`A = self::B`, `B = self::A`), which degrades to `Mixed`.
fn bind_class_const_self_at(
    ty: Type,
    class: &str,
    own_consts: &IndexMap<Arc<str>, ConstSig>,
    depth: usize,
) -> Type {
    const MAX_DEPTH: usize = 16;
    let recurse = |t: Type| bind_class_const_self_at(t, class, own_consts, depth);
    match ty {
        Type::ClassConstant {
            class: owner,
            pattern,
        } => {
            let names_self = match &*owner {
                Type::SelfRef(_) | Type::StaticRef(_) | Type::This => true,
                Type::Object(fqn, _) => fqn.trim_start_matches('\\') == class,
                _ => false,
            };
            match names_self {
                true if depth >= MAX_DEPTH => Type::Mixed(false),
                true => bind_class_const_self_at(
                    matching_class_const_type(own_consts, &pattern),
                    class,
                    own_consts,
                    depth + 1,
                ),
                false => Type::ClassConstant {
                    class: owner,
                    pattern,
                },
            }
        }
        other => other.map_children(&mut |t| recurse(t.clone())),
    }
}

/// Replaces every `ClassConstant` still left in `ty` with
/// `Mixed` - see `resolve_class_constant_references`.
fn unresolved_class_const_to_mixed(ty: Type) -> Type {
    match ty {
        Type::ClassConstant { .. } => Type::Mixed(false),
        other => other.map_children(&mut |t| unresolved_class_const_to_mixed(t.clone())),
    }
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
fn resolve_value_of_target(target_fqn: &str, classes: &FxHashMap<Arc<str>, ClassInfo>) -> Type {
    let Some(info) = classes.get(target_fqn) else {
        return Type::Mixed(false);
    };
    if info.kind != ClassKind::Enum {
        return Type::Mixed(false);
    }
    match info.properties.get("value") {
        Some(sig) => sig.ty.clone(),
        None => Type::Never,
    }
}

/// Resolves `ty` (a param/return type that may contain a `ValueOf`)
/// and, if the result is bare `Type::Mixed(false)`, falls back to
/// `native` instead when that's itself something more useful - restores
/// `function_sig::reconcile_with_native` behavior for a `value-of<T>`
/// that turns out unresolvable (`T` not a real, registered enum): without
/// this, a native type this project already trusted would be silently
/// replaced by a bare, strictly worse `Mixed`.
///
/// Only `ParamSig::ty`/`FunctionSig::return_type` get this treatment -
/// they're the only two places this registry persists a native-only
/// counterpart type to fall back to (`ParamSig::ty_native`/
/// `FunctionSig::return_type_native`). `PropertySig`/`ConstSig`/
/// `ClassInfo::template_bounds` have no such counterpart stored - an
/// unresolvable `value-of<T>` there stays plain `Mixed`.
fn resolve_deferred_value_of_with_native_fallback(
    ty: Type,
    native: &Type,
    classes: &FxHashMap<Arc<str>, ClassInfo>,
) -> Type {
    let resolved = resolve_deferred_value_of_in_type(ty, classes);
    if resolved == Type::Mixed(false) && *native != Type::Mixed(false) {
        native.clone()
    } else {
        resolved
    }
}

impl ClassInfo {
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
                Type::TemplateParam(types::TemplateParam {
                    name: name.to_string(),
                    bound: Box::new(
                        self.template_bounds
                            .get(name)
                            .cloned()
                            .unwrap_or(Type::Mixed(false)),
                    ),
                    variance,
                    scope: Some(class_name.to_string()),
                    default: self.template_defaults.get(name).cloned().map(Box::new),
                })
            })
            .collect()
    }
}

/// `self` in a type inherited from `owner`'s PHPDoc means `owner`, the
/// declaring class, wherever the type is later read; `static` keeps
/// its late binding.
fn bind_self_ref(ty: &Type, owner: &str) -> Type {
    match ty {
        Type::SelfRef(args) => Type::Object(
            owner.into(),
            args.iter().map(|a| bind_self_ref(a, owner)).collect(),
        ),
        other => other.map_children(&mut |t| bind_self_ref(t, owner)),
    }
}

impl Registry {
    /// `reconcile_with_native` runs before the class hierarchy exists, so
    /// it keeps every class-named PHPDoc member. Once every class is
    /// indexed, a member the native hint provably excludes (`Schema` on a
    /// `?ValidationContext` parameter) is dropped here; when none
    /// survive, the native type stands, as PHP enforces it at runtime.
    pub(crate) fn drop_doc_members_excluded_by_native(&mut self) {
        // (class, method or function, parameter index or `None` for the
        // return, corrected type)
        type Fix = (Option<Arc<str>>, Arc<str>, Option<usize>, Type);
        let mut fixes: Vec<Fix> = Vec::new();
        let mut visit = |class: Option<&Arc<str>>, name: &Arc<str>, sig: &FunctionSig| {
            for (i, p) in sig.params.iter().enumerate() {
                if let Some(ty) = self.excluding_native(&p.ty, &p.ty_native, class) {
                    fixes.push((class.cloned(), name.clone(), Some(i), ty));
                }
            }
            if let Some(ty) =
                self.excluding_native(&sig.return_type, &sig.return_type_native, class)
            {
                fixes.push((class.cloned(), name.clone(), None, ty));
            }
        };
        for (class, info) in &self.classes {
            for (name, sig) in &info.methods {
                visit(Some(class), name, sig);
            }
        }
        for (name, sig) in &self.functions {
            visit(None, name, sig);
        }
        let mut property_fixes = Vec::new();
        for (class, info) in &self.classes {
            for (name, prop) in &info.properties {
                if prop.has_native_type
                    && let Some(ty) = self.excluding_native(&prop.ty, &prop.ty_native, Some(class))
                {
                    property_fixes.push((class.clone(), name.clone(), ty));
                }
            }
        }
        for (class, name, ty) in property_fixes {
            if let Some(prop) = self
                .classes
                .get_mut(&class)
                .and_then(|i| i.properties.get_mut(&name))
            {
                prop.ty = ty;
            }
        }
        for (class, name, param, ty) in fixes {
            let sig = match &class {
                Some(c) => self
                    .classes
                    .get_mut(c)
                    .and_then(|info| info.methods.get_mut(&name)),
                None => self.functions.get_mut(&name),
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
    fn excluding_native(&self, ty: &Type, native: &Type, class: Option<&Arc<str>>) -> Option<Type> {
        if ty == native || matches!(native, Type::Mixed(_)) {
            return None;
        }
        let bind = |t: &Type| match class {
            Some(c) => bind_pseudo_refs(t, c),
            None => t.clone(),
        };
        let unbound_native = native;
        let native = bind(native);
        let members: Vec<&Type> = match ty {
            Type::Union(ms) => ms.iter().collect(),
            other => vec![other],
        };
        let native_members: Vec<&Type> = match unbound_native {
            Type::Union(ms) => ms.iter().collect(),
            other => vec![other],
        };
        let mut kept: Vec<Type> = Vec::new();
        let mut changed = false;
        for m in &members {
            let bound = bind(m);
            let keep = match self.is_super_type_of(&native, &bound) {
                super::Trinary::Yes => true,
                super::Trinary::No => false,
                // Two fully known class names only partly overlapping: the
                // hint is what's guaranteed.
                super::Trinary::Maybe => {
                    !(self.is_fully_known_class_type(&bound)
                        && self.is_fully_known_class_type(&native))
                }
            };
            if keep {
                kept.push((*m).clone());
                continue;
            }
            changed = true;
            // A wider doc member (`Node` on a `?Class_` hint) still covers
            // the native alternatives it contains.
            for n in &native_members {
                if self.is_super_type_of(&bound, &bind(n)) == super::Trinary::Yes
                    && !kept.contains(n)
                {
                    kept.push((*n).clone());
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
            kept.iter()
                .any(|k| self.is_super_type_of(&bind(n), &bind(k)) != super::Trinary::No)
        };
        for n in &native_members {
            if !covered(&kept, n) {
                kept.push((*n).clone());
            }
        }
        Some(match kept.len() {
            0 => unbound_native.clone(),
            1 => kept.into_iter().next().unwrap(),
            _ => Type::Union(kept),
        })
    }
}

impl Registry {
    /// Whether `ty` is only class names (and `null`) whose whole
    /// hierarchy the registry indexed - no template, shape, or unknown
    /// class.
    fn is_fully_known_class_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Null => true,
            Type::Object(name, _) => {
                self.lookup_class(name).is_some()
                    && self
                        .all_ancestor_names(name)
                        .iter()
                        .all(|a| self.lookup_class(a).is_some())
            }
            Type::Union(members) => members.iter().all(|m| self.is_fully_known_class_type(m)),
            _ => false,
        }
    }
}

/// `self`/`static` as `class` - only for comparing against a native hint;
/// the stored type keeps its late-bound `static`.
fn bind_pseudo_refs(ty: &Type, class: &str) -> Type {
    match ty {
        Type::SelfRef(args) | Type::StaticRef(args) => Type::Object(
            class.into(),
            args.iter().map(|a| bind_pseudo_refs(a, class)).collect(),
        ),
        Type::This => Type::Object(class.into(), Vec::new()),
        other => other.map_children(&mut |t| bind_pseudo_refs(t, class)),
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
        let mut method_fixes = Vec::new();
        for (class, info) in &self.classes {
            for (name, sig) in &info.methods {
                if let Some(fixed) = fixed_sig(self, sig, &lookup) {
                    method_fixes.push((class.clone(), name.clone(), fixed));
                }
            }
        }
        let mut function_fixes = Vec::new();
        for (name, sig) in &self.functions {
            if let Some(fixed) = fixed_sig(self, sig, &lookup) {
                function_fixes.push((name.clone(), fixed));
            }
        }
        let mut property_fixes = Vec::new();
        for (class, info) in &self.classes {
            for (name, prop) in &info.properties {
                let ty = lookup(self, &prop.ty);
                let doc = prop.doc_ty.as_ref().and_then(|d| lookup(self, d));
                if ty.is_some() || doc.is_some() {
                    property_fixes.push((class.clone(), name.clone(), ty, doc));
                }
            }
        }
        for (class, name, ty, doc) in property_fixes {
            if let Some(prop) = self
                .classes
                .get_mut(&class)
                .and_then(|i| i.properties.get_mut(&name))
            {
                if let Some(ty) = ty {
                    prop.ty = ty;
                }
                if doc.is_some() {
                    prop.doc_ty = doc;
                }
            }
        }
        for (class, name, fixed) in method_fixes {
            if let Some(sig) = self
                .classes
                .get_mut(&class)
                .and_then(|i| i.methods.get_mut(&name))
            {
                *sig = fixed;
            }
        }
        for (name, fixed) in function_fixes {
            if let Some(sig) = self.functions.get_mut(&name) {
                *sig = fixed;
            }
        }
    }
}

/// `sig` with constant-named types resolved, when any are.
fn fixed_sig(
    registry: &Registry,
    sig: &FunctionSig,
    lookup: &dyn Fn(&Registry, &Type) -> Option<Type>,
) -> Option<FunctionSig> {
    let mut fixed: Option<FunctionSig> = None;
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
    let mut changed = false;
    let out = resolve_constant_names_rec(ty, registry, &mut changed);
    changed.then_some(out)
}

fn resolve_constant_names_rec(ty: &Type, registry: &Registry, changed: &mut bool) -> Type {
    if let Type::Object(name, args) = ty
        && args.is_empty()
        && let Some(value) = constant_named_type(name, registry)
    {
        *changed = true;
        return value;
    }
    ty.map_children(&mut |t| resolve_constant_names_rec(t, registry, changed))
}

fn constant_named_type(name: &str, registry: &Registry) -> Option<Type> {
    let short = name.rsplit('\\').next().unwrap_or(name);
    let looks_constant = short
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
        && short
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if !looks_constant || registry.lookup_class(name).is_some() {
        return None;
    }
    let name = name.trim_start_matches('\\');
    registry
        .global_constants
        .get(name)
        .or_else(|| registry.global_constants.get(short))
        .cloned()
}

impl Registry {
    /// A top-level function signature built from one particular
    /// declaration (the registry keeps only one per name), put through
    /// the same per-signature passes as every indexed function: deferred
    /// value types, constant-named types, and native-excluded PHPDoc
    /// members.
    pub(crate) fn finalize_standalone_sig(&self, mut sig: FunctionSig) -> FunctionSig {
        let resolve = |ty: &mut Type, native: &Type| {
            if type_contains_deferred_value_of(ty) {
                let taken = std::mem::replace(ty, Type::Mixed(false));
                *ty = resolve_deferred_value_of_with_native_fallback(taken, native, &self.classes);
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
                self.excluding_native(&sig.params[i].ty, &sig.params[i].ty_native, None)
            {
                sig.params[i].ty = ty;
            }
        }
        if let Some(ty) = self.excluding_native(&sig.return_type, &sig.return_type_native, None) {
            sig.return_type = ty;
        }
        sig
    }
}
