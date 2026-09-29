use crate::registry::NameTable;
use indexmap::IndexMap;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::OnceLock;
use syntax::name::Name;

/// Each templated ancestor's template bindings, as seen from one class.
type AncestorBindings = FxHashMap<Name, FxHashMap<String, Type>>;

use crate::registry::Registry;
use crate::registry::{
    ClassReflection, ConstantReflection, FunctionReflection, PropertyReflection,
    substitute_type_aliases,
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
    /// immutable, already-fully-built `canonical_by_lower` table - no
    /// other class is ever read or written, so `values_mut()` directly
    /// (no remove/reinsert dance) is sound to run concurrently.
    #[hotpath::measure]
    pub(crate) fn canonicalize_class_name_case(&mut self) {
        let mut canonical_by_lower: FxHashMap<String, Name> = FxHashMap::default();
        for key in self.classes.keys() {
            canonical_by_lower
                .entry(key.to_ascii_lowercase())
                .or_insert_with(|| key.clone());
        }
        let canonicalize = |name: &mut Name| {
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
            Name,
            Vec<(Name, FunctionReflection)>,
            Vec<(Name, PropertyReflection)>,
            Vec<(Name, ConstantReflection)>,
            FxHashSet<Name>,
            FxHashSet<Name>,
        );

        let class_names: Vec<Name> = self.classes.keys().cloned().collect();
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
                let own_method_names: FxHashSet<Name> = registry.classes[class_name]
                    .methods
                    .keys()
                    .cloned()
                    .collect();
                let mut methods = Vec::new();
                let mut properties = Vec::new();
                let mut consts = Vec::new();
                let mut abstract_from_traits: FxHashSet<Name> = FxHashSet::default();
                // `static_methods` needs the identical trait-flattening
                // treatment `abstract_from_traits` gets below - without
                // it, a using class's own `static_methods` set would
                // never learn about a trait-supplied static method it
                // doesn't also redeclare itself, even though that
                // method's (correctly flattened-in) `FunctionReflection` is
                // right there in `info.methods`.
                let mut static_from_traits: FxHashSet<Name> = FxHashSet::default();
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
        // Each class's ancestor bindings, computed at most once for the
        // whole fixed point: they're built from its declared hierarchy and
        // template arguments, which the passes never change.
        let bindings: FxHashMap<Name, OnceLock<AncestorBindings>> = self
            .classes
            .keys()
            .map(|name| (name.clone(), OnceLock::new()))
            .collect();
        let max_iterations = self.classes.len().max(1);
        for _ in 0..max_iterations {
            if !self.flatten_phpdoc_inheritance_one_pass(&bindings) {
                break;
            }
        }
    }

    /// A redeclared property inherits each hook of its parent's that it
    /// doesn't declare itself, and is virtual only when the parent's is
    /// and its own hooks never use the backing value.
    #[hotpath::measure]
    pub(crate) fn inherit_property_hooks(&mut self) {
        let max_iterations = self.classes.len().max(1);
        for _ in 0..max_iterations {
            if !self.inherit_property_hooks_one_pass() {
                break;
            }
        }
    }

    fn inherit_property_hooks_one_pass(&mut self) -> bool {
        let mut updates: Vec<(Name, Name, PropertyReflection)> = Vec::new();
        for (class_name, info) in &self.classes {
            let Some(parent) = info.extends.first() else {
                continue;
            };
            for (name, own) in &info.properties {
                let Some((_, inherited)) = self.lookup_property_with_owner(parent, name) else {
                    continue;
                };
                if inherited.visibility == Visibility::Private {
                    continue;
                }
                let mut merged = own.clone();
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
                    updates.push((class_name.clone(), name.clone(), merged));
                }
            }
        }
        let changed = !updates.is_empty();
        for (class_name, name, merged) in updates {
            if let Some(info) = self.classes.get_mut(&class_name) {
                info.properties.insert(name, merged);
            }
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
        let class_names: Vec<Name> = self.classes.keys().cloned().collect();
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
                if ty.contains_deferred() {
                    let bound = bind_class_const_self(
                        std::mem::replace(ty, Type::mixed()),
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
                    resolve(out, &Type::mixed(), &self.classes);
                }
            }
            for prop in info.properties.values_mut() {
                if prop.ty.contains_deferred() {
                    let ty = bind_class_const_self(
                        std::mem::replace(&mut prop.ty, Type::mixed()),
                        &class_name,
                        &own_consts,
                    );
                    prop.ty = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
                if let Some(doc) = &mut prop.doc_ty
                    && doc.contains_deferred()
                {
                    let ty = bind_class_const_self(
                        std::mem::replace(doc, Type::mixed()),
                        &class_name,
                        &own_consts,
                    );
                    *doc = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
                if let Some(set_ty) = &prop.set_ty
                    && set_ty.contains_deferred()
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
                if c.ty.contains_deferred() {
                    let ty = bind_class_const_self(
                        std::mem::replace(&mut c.ty, Type::mixed()),
                        &class_name,
                        &own_consts,
                    );
                    c.ty = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            for bound in info.template_bounds.values_mut() {
                if bound.contains_deferred() {
                    let ty = bind_class_const_self(
                        std::mem::replace(bound, Type::mixed()),
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
                if arg.contains_deferred() {
                    let ty = bind_class_const_self(
                        std::mem::replace(arg, Type::mixed()),
                        &class_name,
                        &own_consts,
                    );
                    *arg = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            self.classes.insert(class_name, info);
        }
        let resolve = |ty: &mut Type, native: &Type, classes: &_| {
            if ty.contains_deferred() {
                let taken = std::mem::replace(ty, Type::mixed());
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
            let mut updates: Vec<(Name, Name, Type)> = Vec::new();
            for (class_name, info) in &self.classes {
                if !info.consts.values().any(|sig| sig.ty.contains_deferred()) {
                    continue;
                }
                let own_consts = visible_class_consts(class_name, &self.classes);
                for (const_name, sig) in &info.consts {
                    if !sig.ty.contains_deferred() {
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
                if sig.ty.contains_deferred() {
                    sig.ty = unresolved_class_const_to_mixed(std::mem::replace(
                        &mut sig.ty,
                        Type::mixed(),
                    ));
                }
            }
        }
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
        let mut classes_with_imports: Vec<Name> = Vec::new();
        let mut resolved: Vec<(Name, Name, Type)> = Vec::new();
        for (class_name, info) in &self.classes {
            if info.type_imports.is_empty() {
                continue;
            }
            classes_with_imports.push(class_name.clone());
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
                    resolved.push((class_name.clone(), imp.local_key.clone(), ty.clone()));
                }
            }
        }
        if classes_with_imports.is_empty() {
            return;
        }
        let mut by_class: FxHashMap<Name, FxHashMap<Name, Type>> = FxHashMap::default();
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
                let taken = std::mem::replace(ty, Type::mixed());
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
                let ty = std::mem::replace(&mut prop.ty, Type::mixed());
                prop.ty = substitute_type_aliases(ty, &aliases);
                if let Some(doc) = &mut prop.doc_ty {
                    substitute(doc);
                }
                if let Some(set_ty) = prop.set_ty.take() {
                    prop.set_ty = Some(substitute_type_aliases(set_ty, &aliases));
                }
            }
            for c in info.consts.values_mut() {
                let ty = std::mem::replace(&mut c.ty, Type::mixed());
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
        let mut method_additions: Vec<(Name, Name, FunctionReflection)> = Vec::new();
        let mut property_additions: Vec<(Name, Name, PropertyReflection)> = Vec::new();
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
    /// `FunctionReflection::from_decl`/`ClassReflection::from_decl` pipeline (which
    /// `resolve_deferred_value_of` covers) needs this too - e.g.
    /// `Checker::resolve_type_hint`, reached through an inline `@var`
    /// annotation, would otherwise leak the raw, unresolved sentinel
    /// straight into a diagnostic message.
    pub(crate) fn resolve_deferred_value_of_now(&self, ty: Type) -> Type {
        if !ty.contains_deferred() {
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
        resolve_deferred_value_of_with_native_fallback(ty, native, &self.classes)
    }

    /// Whether `ty` mentions a template scoped anywhere
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
    /// vs. `class_info.rs`'s `class_scope`, the class's own name,
    /// for a class-level one - an FQN vs. a bare method name,
    /// structurally distinguishable). A method-scoped template is safe
    /// to copy through verbatim: `Relation::infer_template_bindings`/
    /// `Type::substitute_templates`'s call-site binding is purely name-keyed
    /// and blind to which class's `FunctionReflection` a signature came from.
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
        ty.nested_templates().iter().any(|tp| {
            !matches!(
                tp.scope.as_deref(),
                Some(s) if s == method_name || s == types::CALLABLE_TEMPLATE_SCOPE
            )
        })
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
    pub(crate) fn template_param_scopes_all_safe_to_backfill(
        ty: &Type,
        method_name: &str,
        ancestor_bindings: &FxHashMap<Name, FxHashMap<String, Type>>,
    ) -> bool {
        ty.nested_templates().iter().all(|tp| {
            tp.scope.as_deref() == Some(method_name)
                || tp.scope.as_deref() == Some(types::CALLABLE_TEMPLATE_SCOPE)
                || tp
                    .scope
                    .as_deref()
                    .is_some_and(|s| ancestor_bindings.get(s).is_some_and(|b| !b.is_empty()))
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
        bindings: &FxHashMap<Name, OnceLock<AncestorBindings>>,
    ) -> bool {
        type Backfill = (Name, Name, Vec<(usize, Type, bool)>, Option<(Type, bool)>);

        let class_names: Vec<Name> = self.classes.keys().cloned().collect();
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
                    // at call time later. See `template_param_scopes_all_
                    // safe_to_backfill`'s doc comment for why this must be
                    // the real resolved bindings, not merely "is this
                    // ancestor name reachable at all".
                    let ancestor_bindings = || {
                        bindings
                            .get(class_name)
                            .expect("every class has a bindings slot")
                            .get_or_init(|| registry.ancestor_bindings(class_name, info))
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
                                        ancestor_bindings(),
                                    )
                                    && Registry::safe_to_backfill_object_type(
                                        &ancestor_param.ty,
                                        &p.ty_native,
                                    )
                                {
                                    param_fills.push((
                                        i,
                                        Registry::substitute_class_scoped_ancestor_templates(
                                            &ancestor_param.ty.bind_self(&owner),
                                            ancestor_bindings(),
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
                                        ancestor_bindings(),
                                    )
                                    && Registry::safe_to_backfill_object_type(
                                        &ancestor_param.ty,
                                        &p.ty_native,
                                    )
                                {
                                    param_fills.push((
                                        i,
                                        Registry::substitute_class_scoped_ancestor_templates(
                                            &ancestor_param.ty.bind_self(&owner),
                                            ancestor_bindings(),
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
                    let inherits_tentative_type = registry.built_in_stub_classes.contains(&owner)
                        && sig.return_type_native.is_mixed()
                        && !ancestor_sig.return_type_native.is_mixed();
                    let return_fill = (needs_return
                        && (ancestor_sig.return_has_own_doc || inherits_tentative_type)
                        && Registry::safe_to_backfill_mixed(
                            &ancestor_sig.return_type,
                            &sig.return_type_native,
                        )
                        && Registry::template_param_scopes_all_safe_to_backfill(
                            &ancestor_sig.return_type,
                            method_name,
                            ancestor_bindings(),
                        )
                        && Registry::safe_to_backfill_object_type(
                            &ancestor_sig.return_type,
                            &sig.return_type_native,
                        ))
                    .then(|| {
                        (
                            Registry::substitute_class_scoped_ancestor_templates(
                                &ancestor_sig.return_type.bind_self(&owner),
                                ancestor_bindings(),
                            )
                            .reconcile_with_native(sig.return_type_native.clone()),
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

/// The actual substitution: every `ValueOf`/`ClassConstant` in `ty`, at
/// any depth. See `Registry::resolve_deferred_value_of`'s doc comment
/// for why this exists, and `types::parse_type_keyword_or_class`'s
/// `"value-of"` arm for where a `ValueOf` is produced.
fn resolve_deferred_value_of_in_type(ty: Type, classes: &NameTable<ClassReflection>) -> Type {
    ty.resolve_class_references(&DeclaredConstants(classes))
}

/// The constants and enum cases of the classes indexed so far.
struct DeclaredConstants<'a>(&'a NameTable<ClassReflection>);

impl types::ClassConstants for DeclaredConstants<'_> {
    fn enum_values(&self, class: &str) -> Type {
        resolve_value_of_target(class, self.0)
    }

    fn class_constants(&self, class: &Type, pattern: &str) -> Type {
        resolve_class_const_type(class, pattern, self.0)
    }
}

/// A `ClassConstant`'s value: the union of every matching
/// constant's own type (an enum case's enum case, a literal
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
) -> Type {
    let Some((fqn, _)) = class.as_object() else {
        return Type::mixed();
    };
    let fqn = fqn.trim_start_matches('\\');
    if !classes.contains_key(fqn) {
        return Type::mixed();
    }
    matching_class_const_type(&visible_class_consts(fqn, classes), pattern)
}

/// Whether any type `resolve_deferred_value_of` rewrites on `info`
/// contains a deferred sentinel - lets that pass skip a class outright.
fn class_has_deferred_type(info: &ClassReflection) -> bool {
    info.methods.values().any(|sig| {
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
    }) || info
        .extends_args
        .iter()
        .chain(info.implements_args.values().flatten())
        .any(Type::contains_deferred)
        || info.properties.values().any(|p| {
            p.ty.contains_deferred()
                || p.doc_ty.as_ref().is_some_and(Type::contains_deferred)
                || p.set_ty.as_ref().is_some_and(Type::contains_deferred)
        })
        || info.consts.values().any(|c| c.ty.contains_deferred())
        || info.template_bounds.values().any(Type::contains_deferred)
}

/// Every constant visible on `fqn` - its own, then each parent's and
/// interface's not already shadowed - the set real PHPStan's
/// `ClassReflection::getConstants()` resolves a `Foo::*` against. Walked
/// breadth-first, a class's own declaration winning over an inherited one
/// of the same name.
fn visible_class_consts(
    fqn: &str,
    classes: &NameTable<ClassReflection>,
) -> IndexMap<Name, ConstantReflection> {
    let mut consts: IndexMap<Name, ConstantReflection> = IndexMap::new();
    let mut seen: FxHashSet<Name> = FxHashSet::default();
    let mut queue: std::collections::VecDeque<Name> = std::collections::VecDeque::new();
    queue.push_back(Name::from(fqn));
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
fn matching_class_const_type(consts: &IndexMap<Name, ConstantReflection>, pattern: &str) -> Type {
    let names: Vec<&Name> = consts
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
fn bind_class_const_self(
    ty: Type,
    class: &str,
    own_consts: &IndexMap<Name, ConstantReflection>,
) -> Type {
    bind_class_const_self_at(ty, class, own_consts, 0)
}

/// `bind_class_const_self`'s body. A substituted constant type can itself
/// hold a further own-class reference (`A = self::B`), so each
/// substitution is walked again, `depth` bounding a reference cycle
/// (`A = self::B`, `B = self::A`), which degrades to `Mixed`.
fn bind_class_const_self_at(
    ty: Type,
    class: &str,
    own_consts: &IndexMap<Name, ConstantReflection>,
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
fn resolve_value_of_target(target_fqn: &str, classes: &NameTable<ClassReflection>) -> Type {
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
) -> Type {
    let resolved = resolve_deferred_value_of_in_type(ty, classes);
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
        // (class, method or function, parameter index or `None` for the
        // return, corrected type)
        type Fix = (Option<Name>, Name, Option<usize>, Type);
        let mut fixes: Vec<Fix> = Vec::new();
        let mut visit = |class: Option<&Name>, name: &Name, sig: &FunctionReflection| {
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
    fn excluding_native(&self, ty: &Type, native: &Type, class: Option<&Name>) -> Option<Type> {
        if ty == native || native.is_mixed() {
            return None;
        }
        let bind = |t: &Type| match class {
            Some(c) => t.bind_self(c).bind_static(c),
            None => t.clone(),
        };
        let unbound_native = native;
        let native = bind(native);
        let members = ty.union_members();
        let native_members = unbound_native.union_members();
        let mut kept: Vec<Type> = Vec::new();
        let mut changed = false;
        for m in members {
            let bound = bind(m);
            let keep = match self.is_super_type_of(&native, &bound) {
                types::TrinaryLogic::Yes => true,
                types::TrinaryLogic::No => false,
                // Two fully known class names only partly overlapping: the
                // hint is what's guaranteed.
                types::TrinaryLogic::Maybe => {
                    !(self.is_fully_known_class_type(&bound)
                        && self.is_fully_known_class_type(&native))
                }
            };
            if keep {
                kept.push(m.clone());
                continue;
            }
            changed = true;
            // A wider doc member (`Node` on a `?Class_` hint) still covers
            // the native alternatives it contains.
            for n in native_members {
                if self.is_super_type_of(&bound, &bind(n)) == types::TrinaryLogic::Yes
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
            kept.iter()
                .any(|k| self.is_super_type_of(&bind(n), &bind(k)) != types::TrinaryLogic::No)
        };
        for n in native_members {
            if !covered(&kept, n) {
                kept.push(n.clone());
            }
        }
        Some(match kept.len() {
            0 => unbound_native.clone(),
            1 => kept.into_iter().next().unwrap(),
            _ => Type::union_of(kept),
        })
    }
}

impl Registry {
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
    let mut changed = false;
    let out = resolve_constant_names_rec(ty, registry, &mut changed);
    changed.then_some(out)
}

fn resolve_constant_names_rec(ty: &Type, registry: &Registry, changed: &mut bool) -> Type {
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
    pub(crate) fn finalize_standalone_sig(
        &self,
        mut sig: FunctionReflection,
    ) -> FunctionReflection {
        let resolve = |ty: &mut Type, native: &Type| {
            if ty.contains_deferred() {
                let taken = std::mem::replace(ty, Type::mixed());
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
