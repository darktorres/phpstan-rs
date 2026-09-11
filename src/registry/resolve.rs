use rayon::prelude::*;
use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet};

use crate::ast::{ClassKind, Visibility};
use crate::registry::Registry;
use crate::registry::function_sig;
use crate::registry::{ClassInfo, FunctionSig, PropertySig, substitute_type_aliases};
use crate::types::Type;

impl Registry {
    /// Phase 48.1: a real, verified correctness bug - PHP itself resolves
    /// class/interface/trait/enum names case-insensitively (the same
    /// rationale `Registry::lookup_function`'s own doc comment already
    /// gives for functions), but every *internal* registry graph-walk
    /// (`flatten_traits`, `flatten_phpdoc_inheritance`, the ancestor/
    /// interface-satisfaction walks in `registry/hierarchy.rs`, ...) does
    /// a plain, exact-case `self.classes.get(name)`. Rather than thread a
    /// case-insensitive fallback through every one of those ~20 call
    /// sites (real, ongoing maintenance risk - a new one added later
    /// could easily forget it), this runs once, right after indexing
    /// finishes and *before* any of those graph-walks - a single
    /// O(classes) rewrite pass that canonicalizes every reference-shaped
    /// field on each `ClassInfo` (`extends`, `implements`, `uses`,
    /// `mixins`, each `TraitAlias::trait_name`, each `ResolvedTypeImport::
    /// source_class_fqn`) to the real declared key's own case, wherever
    /// the differently-cased reference resolves at all. A reference that
    /// still doesn't resolve after lowercasing is left completely
    /// untouched - a genuine typo, which the existing `class.notFound`-
    /// family checks already catch; this function only ever narrows what
    /// "resolves" means, never widens it. Every internal `self.classes.
    /// get(&current)`-style lookup stays a plain, allocation-free `HashMap`
    /// lookup, now simply correct because the stored string is always
    /// canonical - matching `lookup_function`'s own stated philosophy.
    /// Building `canonical_by_lower` stays a single sequential pass (it's
    /// already O(classes), one cheap `HashMap` entry each) - only the
    /// per-class rewrite below is parallelized, since it's the part whose
    /// cost actually scales with how many reference-shaped fields each
    /// class has. Each class's own rewrite touches only its own
    /// `ClassInfo` (`extends`/`implements`/`uses`/`mixins`/
    /// `trait_aliases`/`type_imports`) and reads only the immutable,
    /// already-fully-built `canonical_by_lower` table - no other class is
    /// ever read or written, so `values_mut()` directly (no remove/
    /// reinsert dance) is sound to run concurrently.
    #[hotpath::measure]
    pub(crate) fn canonicalize_class_name_case(&mut self) {
        let mut canonical_by_lower: HashMap<String, String> = HashMap::new();
        for key in self.classes.keys() {
            canonical_by_lower
                .entry(key.to_ascii_lowercase())
                .or_insert_with(|| key.clone());
        }
        // Persisted as `self.classes_lowercase` too - `Registry::
        // lookup_class`'s own doc comment explains why its linear-scan
        // fallback uses this instead of a fresh scan on every call. Built
        // fresh every time this pass runs (the same "just recompute the
        // whole thing" precedent every other struct-level derived field in
        // this file already follows, e.g. `precompute_ancestor_names`),
        // so it can never go stale independently of this pass's own
        // already-established call sites (`Registry::index_and_flatten`/
        // `build`, `Project::finish_loading`/`update_file`).
        self.classes_lowercase = canonical_by_lower
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let canonicalize = |name: &mut String| {
            if let Some(canonical) = canonical_by_lower.get(&name.to_ascii_lowercase())
                && canonical != name
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
    /// Finding 13: a trait that itself declares `use OtherTrait;` needs
    /// `OtherTrait`'s own methods folded into it before a class using the
    /// first trait is processed - a single non-iterative pass only ever
    /// composes one level deep. Runs `flatten_traits_one_pass` to a fixed
    /// point (each pass may itself surface a *new* one-level composition
    /// a later pass can now see, e.g. a trait-of-a-trait-of-a-trait
    /// chain), bounded by the number of classes/traits registered at all,
    /// since a real trait-composition cycle is itself a PHP fatal error
    /// this project doesn't otherwise detect, so degrading to "leave as
    /// partially flattened" once the bound is hit (rather than looping
    /// forever) is the honest answer, not a regression.
    #[hotpath::measure]
    pub(crate) fn flatten_traits(&mut self) {
        let max_iterations = self.classes.len().max(1);
        for _ in 0..max_iterations {
            if !self.flatten_traits_one_pass() {
                break;
            }
        }
    }

    /// One level of trait composition - see `flatten_traits`'s own doc
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
            String,
            Vec<(String, FunctionSig)>,
            Vec<(String, PropertySig)>,
            HashSet<String>,
            HashSet<String>,
        );

        let class_names: Vec<String> = self.classes.keys().cloned().collect();
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
                let own_method_names: HashSet<String> = registry.classes[class_name]
                    .methods
                    .keys()
                    .cloned()
                    .collect();
                let mut methods = Vec::new();
                let mut properties = Vec::new();
                let mut abstract_from_traits: HashSet<String> = HashSet::new();
                // Phase 64: `static_methods` needs the identical trait-
                // flattening treatment `abstract_from_traits` already gets
                // just below - without it, a using class's own `static_
                // methods` set silently never learns about a trait-supplied
                // static method it doesn't also redeclare itself, even though
                // that method's own (correctly flattened-in) `FunctionSig` is
                // right there in `info.methods`. A real corpus false positive
                // this fixes: `Symfony\Component\Cache\Adapter\RedisAdapter`
                // (`use RedisTrait;`) was flagged as overriding
                // `AbstractAdapter::createConnection()` non-statically, since
                // `RedisTrait::createConnection()`'s own real static-ness
                // never made it into `RedisAdapter.static_methods` at all.
                let mut static_from_traits: HashSet<String> = HashSet::new();
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
                    }
                }
                // Finding 30: `use A, B { B::hello as helloB; }` - register
                // the trait method under its new alias too, *alongside* its
                // original name (both stay callable in real PHP). A bare
                // `method as newName;` with no `TraitName::` prefix (real,
                // valid PHP when the name is unambiguous among the traits
                // actually used) resolves against the first used trait that
                // declares it - this project doesn't detect the ambiguous
                // case (two used traits both declaring the same method name
                // with no `insteadof`) as its own separate error either, so
                // this leniency doesn't introduce a new gap next to it.
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
                    abstract_from_traits,
                    static_from_traits,
                ))
            })
            .collect();

        let mut changed = false;
        for (class_name, methods, properties, abstract_from_traits, static_from_traits) in additions
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

    /// `@method`'s own deferred insertion - `ClassInfo::method_tags`'s own
    /// construction site (`from_decl`) has the full "why this can't just
    /// insert eagerly" explanation: a same-named real trait method hasn't
    /// been flattened in yet at that point, and both insertions are
    /// vacant-only, so inserting first would let the tag win by accident.
    /// Runs once, right after `flatten_traits` reaches its own fixed
    /// point (every real method, own or trait-derived, already settled)
    /// and before `flatten_phpdoc_inheritance` (irrelevant either way - a
    /// synthesized signature is always fully resolved already, never a
    /// backfill candidate) - no fixed-point loop of its own needed, since
    /// a class's own `method_tags` list is fixed the moment `from_decl`
    /// built it.
    ///
    /// Fully self-contained per class (`info.method_tags` -> `info.
    /// methods`, on that same `info` only) - no other class is ever read
    /// or written, so this parallelizes with no read phase to separate
    /// out first, unlike `flatten_traits`/`flatten_phpdoc_inheritance`
    /// below (which do need one, since they read *other* classes).
    #[hotpath::measure]
    pub(crate) fn resolve_method_tags(&mut self) {
        self.classes
            .values_mut()
            .collect::<Vec<_>>()
            .into_par_iter()
            .for_each(|info| {
                for (name, sig) in std::mem::take(&mut info.method_tags) {
                    info.methods.entry(name).or_insert(sig);
                }
            });
    }

    /// Phase 87: real PHPStan backfills a method's own missing `@param`/
    /// `@return` from its parent class (first) or first-declared
    /// implemented interface, whenever the child's own docblock lacks a
    /// tag for that position *at all* - verified directly against
    /// `vendor/phpstan-src/src/PhpDoc/ResolvedPhpDocBlock.php`'s own
    /// `mergeParamTags`/`mergeReturnTags` (`array_key_exists($name,
    /// $paramTags)` / `$returnTag !== null`), unconditional and
    /// unrelated to whether the resolved type happens to be `Mixed`.
    /// Reuses `lookup_ancestor_method_with_owner`'s already-correct
    /// parent-then-implements, first-hit-wins order - no new ordering
    /// logic. Runs after `flatten_traits` (so a trait-derived method
    /// already looks like the class's own), to a fixed point exactly
    /// like `flatten_traits` itself, for the identical reason: a
    /// multi-level chain where *each* level textually redeclares the
    /// method without its own docblock needs more than one pass to fully
    /// resolve.
    ///
    /// Scope, deliberately narrow for this first slice (see ROADMAP.md's
    /// Phase 87 entry for the full rationale): methods only, no
    /// properties (real PHP interfaces can't declare them, so the
    /// existing `lookup_ancestor_property_with_owner`'s parent-only walk
    /// already covers the one real path); no trait-abstract-method
    /// fallback; ancestor conflicts aren't reconciled, first-match-wins
    /// (already this project's behavior, and real PHPStan's too); an
    /// ancestor's `private` method is never inherited from (a `private`
    /// method can't be overridden at all, so its own docblock says
    /// nothing about what an unrelated same-named child method means).
    #[hotpath::measure]
    pub(crate) fn flatten_phpdoc_inheritance(&mut self) {
        let max_iterations = self.classes.len().max(1);
        for _ in 0..max_iterations {
            if !self.flatten_phpdoc_inheritance_one_pass() {
                break;
            }
        }
    }

    /// Phase 80 follow-up: resolves `types::parse_type_keyword_or_class`'s
    /// own `value-of<T>`-on-a-class/enum sentinel
    /// (`Type::Object("$$valueof", [Type::Object(target_fqn, [])])`) now
    /// that every class/enum is actually indexed - see that function's own
    /// doc comment for why this can't happen at parse time. A single pass,
    /// not a fixed-point loop like `flatten_phpdoc_inheritance` above: a
    /// `value-of<value-of<X>>`-style chain isn't attempted (no realistic
    /// case, disclosed rather than silently unhandled).
    ///
    /// Every class is taken out of `self.classes` before being mutated
    /// (`HashMap::remove`, reinserted at the end of its own iteration),
    /// not the collect-then-apply shape `flatten_phpdoc_inheritance_one_pass`
    /// needs - that one's own shape exists to let a fixed-point loop read
    /// an *ancestor's* still-unmodified signature while backfilling a
    /// *descendant's*; this pass only ever needs to read some *other*
    /// class/enum's own `properties["value"]` while rewriting the current
    /// one's types, and temporarily removing the current entry sidesteps
    /// borrowing `self.classes` both ways at once with no extra plumbing.
    /// (A class whose own docblock names `value-of<self>` on itself, where
    /// `self` is genuinely an enum, degrades to `Mixed` while it's
    /// temporarily removed - the identical, already-accepted "can't
    /// verify, don't guess" outcome every other unresolvable case here
    /// already gets, not a new failure mode.) Top-level functions
    /// (`self.functions`) need no such dance at all - mutating one
    /// function's own signature while reading `self.classes` for a lookup
    /// borrows two disjoint fields of `self`, which the borrow checker
    /// already allows without help.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_value_of(&mut self) {
        let class_names: Vec<String> = self.classes.keys().cloned().collect();
        for class_name in class_names {
            let Some(mut info) = self.classes.remove(&class_name) else {
                continue;
            };
            for sig in info.methods.values_mut() {
                for param in &mut sig.params {
                    if type_contains_deferred_value_of(&param.ty) {
                        let ty = std::mem::replace(&mut param.ty, Type::Mixed);
                        param.ty = resolve_deferred_value_of_with_native_fallback(
                            ty,
                            &param.ty_native,
                            &self.classes,
                        );
                    }
                }
                if type_contains_deferred_value_of(&sig.return_type) {
                    let ty = std::mem::replace(&mut sig.return_type, Type::Mixed);
                    sig.return_type = resolve_deferred_value_of_with_native_fallback(
                        ty,
                        &sig.return_type_native,
                        &self.classes,
                    );
                }
            }
            for prop in info.properties.values_mut() {
                if type_contains_deferred_value_of(&prop.ty) {
                    let ty = std::mem::replace(&mut prop.ty, Type::Mixed);
                    prop.ty = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
                if let Some(set_ty) = &prop.set_ty
                    && type_contains_deferred_value_of(set_ty)
                {
                    let ty = prop.set_ty.take().unwrap();
                    prop.set_ty = Some(resolve_deferred_value_of_in_type(ty, &self.classes));
                }
            }
            for c in info.consts.values_mut() {
                if type_contains_deferred_value_of(&c.ty) {
                    let ty = std::mem::replace(&mut c.ty, Type::Mixed);
                    c.ty = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            for bound in info.template_bounds.values_mut() {
                if type_contains_deferred_value_of(bound) {
                    let ty = std::mem::replace(bound, Type::Mixed);
                    *bound = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            self.classes.insert(class_name, info);
        }
        for sig in self.functions.values_mut() {
            for param in &mut sig.params {
                if type_contains_deferred_value_of(&param.ty) {
                    let ty = std::mem::replace(&mut param.ty, Type::Mixed);
                    param.ty = resolve_deferred_value_of_with_native_fallback(
                        ty,
                        &param.ty_native,
                        &self.classes,
                    );
                }
            }
            if type_contains_deferred_value_of(&sig.return_type) {
                let ty = std::mem::replace(&mut sig.return_type, Type::Mixed);
                sig.return_type = resolve_deferred_value_of_with_native_fallback(
                    ty,
                    &sig.return_type_native,
                    &self.classes,
                );
            }
        }
    }

    /// `@phpstan-import-type`'s own deferred half - `ClassInfo::from_decl`
    /// already resolved both names (`ResolvedTypeImport`'s own doc
    /// comment), but couldn't look the import up yet: the source class
    /// may not be indexed at all at that point. Mirrors `resolve_deferred_
    /// value_of`'s exact two-step shape (a read-only pass collecting
    /// every resolvable answer, then a second pass applying them) for the
    /// identical reason - looking a *different* class's own `type_aliases`
    /// up while mutating *this* class's own would otherwise borrow
    /// `self.classes` two ways at once.
    ///
    /// A source class's own alias is looked up under `{its own
    /// namespace}\{alias name}` - reconstructed directly from its FQN
    /// (`class_namespace`) rather than re-running that class's own name
    /// resolution, which would need its own `NameContext` (not kept
    /// around once `from_decl` returns). This matches `NameContext::
    /// resolve`'s own actual behavior for a bare, unqualified name with
    /// no matching `use`-import (confirmed directly: falls straight
    /// through to `declared_name`, i.e. plain namespace-prefixing) -
    /// exactly what a `@phpstan-type` tag's own bare alias name always is
    /// in practice, never itself subject to `use`-aliasing.
    ///
    /// Not a fixed-point loop (an import that itself points at another
    /// imported - not locally declared - alias isn't chased further) -
    /// the identical "one level of indirection only" leniency `build_
    /// type_aliases`'s own second pass already accepts for local aliases
    /// referencing each other, extended here rather than reinvented.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_type_imports(&mut self) {
        let mut classes_with_imports: Vec<String> = Vec::new();
        let mut resolved: Vec<(String, String, Type)> = Vec::new();
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
                    .and_then(|src| src.type_aliases.get(&source_key))
                {
                    resolved.push((class_name.clone(), imp.local_key.clone(), ty.clone()));
                }
            }
        }
        if classes_with_imports.is_empty() {
            return;
        }
        let mut by_class: HashMap<String, HashMap<String, Type>> = HashMap::new();
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
            // A class whose own imports all failed to resolve (its source
            // class isn't indexed at all, e.g. an external dependency
            // outside this run's own corpus) still needs `type_imports`
            // cleared - the one-shot pass above already ran, so there's no
            // second chance coming; an empty `aliases` map here makes
            // every `substitute_type_aliases` call below a no-op, correct
            // but slightly wasted work, traded for not duplicating this
            // whole loop body into two near-identical copies.
            let aliases = by_class.remove(&class_name).unwrap_or_default();
            for sig in info.methods.values_mut() {
                for param in &mut sig.params {
                    let ty = std::mem::replace(&mut param.ty, Type::Mixed);
                    param.ty = substitute_type_aliases(ty, &aliases);
                }
                let ty = std::mem::replace(&mut sig.return_type, Type::Mixed);
                sig.return_type = substitute_type_aliases(ty, &aliases);
            }
            for prop in info.properties.values_mut() {
                let ty = std::mem::replace(&mut prop.ty, Type::Mixed);
                prop.ty = substitute_type_aliases(ty, &aliases);
                if let Some(set_ty) = prop.set_ty.take() {
                    prop.set_ty = Some(substitute_type_aliases(set_ty, &aliases));
                }
            }
            for c in info.consts.values_mut() {
                let ty = std::mem::replace(&mut c.ty, Type::Mixed);
                c.ty = substitute_type_aliases(ty, &aliases);
            }
            info.type_aliases.extend(aliases);
            info.type_imports.clear();
            self.classes.insert(class_name, info);
        }
    }

    /// `@mixin`'s own deferred half - mirrors `resolve_deferred_type_
    /// imports`'s exact two-step shape (a read-only collection pass, then
    /// a second pass applying it), for the identical reason: the mixin
    /// target may not be indexed yet at `ClassInfo::from_decl` time.
    ///
    /// Only a *public* method/property is pulled in - confirmed directly
    /// against real PHPStan's own `MixinMethodsClassReflectionExtension`/
    /// `MixinPropertiesClassReflectionExtension`, both of which resolve
    /// the mixin target from `OutOfClassScope` (no special access at
    /// all, the same visibility a caller anywhere outside the class
    /// itself would see) - and only when this class doesn't already
    /// declare that name for real, same "real declaration always wins"
    /// precedent `@property`/`@method` synthesis already have.
    ///
    /// Not chased through a mixin's own further `@mixin`s (one level
    /// only) - the same "don't attempt a fixed-point solve for a shape
    /// not yet confirmed to occur in practice" leniency `build_type_
    /// aliases`'s own alias-of-alias handling already accepts.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_mixins(&mut self) {
        let mut method_additions: Vec<(String, String, FunctionSig)> = Vec::new();
        let mut property_additions: Vec<(String, String, PropertySig)> = Vec::new();
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

    /// The `&self` (read-only, immediate) twin of `resolve_deferred_value_of`
    /// above, for a `Type` parsed directly from a raw docblock string
    /// *during checking* rather than at registry-build time - by then
    /// every class/enum genuinely is already fully indexed (unlike mid-
    /// registry-construction, where `resolve_deferred_value_of` itself
    /// needs its own one-time deferred pass), so this resolves the
    /// `"$$valueof"` sentinel immediately, inline, with no pass needed.
    /// Every checker-side call site that parses a docblock type string
    /// independently of the registry's own `FunctionSig::from_decl`/
    /// `ClassInfo::from_decl` pipeline (which `resolve_deferred_value_of`
    /// already covers) needs this too - real corpus false positive found
    /// via this phase's own corpus diff caught exactly one such site
    /// (`Checker::resolve_type_hint`, reached through an inline `@var`
    /// annotation) that would otherwise leak the raw, unresolved sentinel
    /// straight into a real diagnostic message
    /// (`shipmonk/dead-code-detector`'s own `CollectedUsage::deserialize()`,
    /// `/** @var array{..., t: value-of<MemberType>, ...} $result */`).
    pub(crate) fn resolve_deferred_value_of_now(&self, ty: Type) -> Type {
        if !type_contains_deferred_value_of(&ty) {
            return ty;
        }
        resolve_deferred_value_of_in_type(ty, &self.classes)
    }

    /// `resolve_deferred_value_of_now`'s own native-fallback-aware sibling.
    /// See `resolve_deferred_value_of_with_native_fallback`'s doc comment
    /// (the free function this wraps) for why an unresolvable
    /// `value-of<T>` needs this rather than plain `resolve_deferred_value_of_now`
    /// alone. Used by the anonymous-class param/return docblock fallback
    /// in `checker.rs`, which (like the registry's own `FunctionSig::
    /// from_decl`) reconciles its own raw-parsed doc type against a
    /// native counterpart via `resolve_type` before this ever runs.
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
    /// mark_template_params`'s own recursive-case list (`Object`'s type
    /// args, `Array`'s key/value, `ClassString`, `Union`/`Intersection`,
    /// `ArrayShape`/`ObjectShape` fields). `flatten_phpdoc_inheritance_
    /// one_pass` uses this to refuse backfilling a *foreign*-scoped
    /// template-parameterized ancestor type - i.e. genuinely unsafe to
    /// backfill because it's scoped to the ancestor's own *enclosing
    /// class* (meaningful only via unmodeled `@extends Base<Concrete>`
    /// cross-inheritance binding - this project does not model that).
    /// Real corpus false positive caught verifying this guard's original,
    /// blanket version: Doctrine's `AbstractClassMetadataFactory
    /// implements ClassMetadataFactory` (`@template T of ClassMetadata`,
    /// `@phpstan-return list<T>` on `getAllMetadata()`) backfilling
    /// verbatim onto `Doctrine\ORM\Mapping\ClassMetadataFactory` (which
    /// real PHPStan binds `T` to the *concrete* `Doctrine\ORM\Mapping\
    /// ClassMetadata` via its own `@extends
    /// AbstractClassMetadataFactory<ClassMetadata>`) left `T` resolved
    /// against the *interface* bound instead, silently losing
    /// `Doctrine\ORM\Mapping\ClassMetadata::getTableName()`. This class-
    /// scoped half stays exactly as refused as it always was.
    ///
    /// As opposed to a `@template` declared directly on the ancestor
    /// *method* itself
    /// (`function_sig::from_decl`'s own `build_template_params(d,
    /// &f.name, ctx)` stamps `scope: Some(f.name)` for exactly this
    /// case, vs. `class_info.rs`'s own `class_scope =
    /// ctx.declared_name(&c.name)` for a class-level one - an FQN vs. a
    /// bare method name, structurally distinguishable). A method-scoped
    /// template is safe to copy through verbatim: `types::
    /// bind_template_param`/`substitute_template`'s own call-site
    /// binding is purely name-keyed and blind to which class's
    /// `FunctionSig` a signature came from - confirmed against real,
    /// vendored `Doctrine\ORM\EntityManagerInterface::getReference()`
    /// (`@template T of object`, `@param class-string<T> $entityName`,
    /// `@return T|null`) backfilling cleanly onto `EntityManager::
    /// getReference()`'s own wholly undocumented native signature. Real
    /// PHPStan draws the identical line: `ResolvedPhpDocBlock::
    /// resolveTemplateTypeInTag` substitutes an ancestor tag's type
    /// through `$parentClass->getActiveTemplateTypeMap()`, which only
    /// ever holds *class*-level bindings - a method-scoped `@template`
    /// is never a member of it, so that substitution is a verified
    /// no-op for exactly this shape, and real PHPStan's own merge
    /// leaves it untouched too - it doesn't special-case `getReference()`,
    /// it just never touches method-scoped templates during inheritance
    /// merge.
    pub(crate) fn type_mentions_foreign_scoped_template_param(
        ty: &Type,
        method_name: &str,
    ) -> bool {
        match ty {
            Type::TemplateParam(tp) => tp.scope.as_deref() != Some(method_name),
            Type::Object(_, args) => args
                .iter()
                .any(|a| Registry::type_mentions_foreign_scoped_template_param(a, method_name)),
            Type::Array(key, inner) => {
                Registry::type_mentions_foreign_scoped_template_param(key, method_name)
                    || Registry::type_mentions_foreign_scoped_template_param(inner, method_name)
            }
            Type::ClassString(inner) => {
                Registry::type_mentions_foreign_scoped_template_param(inner, method_name)
            }
            Type::Union(members) | Type::Intersection(members) => members
                .iter()
                .any(|m| Registry::type_mentions_foreign_scoped_template_param(m, method_name)),
            Type::ArrayShape(fields) | Type::ObjectShape(fields) => {
                fields.iter().any(|(_, t, _)| {
                    Registry::type_mentions_foreign_scoped_template_param(t, method_name)
                })
            }
            _ => false,
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
    /// itself). Real corpus false positive caught verifying this pass:
    /// `Doctrine\ORM\EntityManagerInterface::getMetadataFactory()` (no
    /// own doc, native hint already resolves - via its own `use Doctrine\
    /// ORM\Mapping\ClassMetadataFactory` - to the concrete ORM factory)
    /// sits under `Doctrine\Persistence\ObjectManager`'s own `@phpstan-
    /// return ClassMetadataFactory<ClassMetadata<object>>` (that file's
    /// own `use` resolves the *unqualified* interface instead) - a real,
    /// concrete, non-template type on both sides (so neither of the two
    /// guards above catches it), but backfilling it would still silently
    /// widen away the already-correct, more specific native class.
    pub(crate) fn safe_to_backfill_object_type(ancestor_ty: &Type, native_ty: &Type) -> bool {
        match (ancestor_ty, native_ty) {
            (Type::Object(a, _), Type::Object(n, _)) => a == n,
            _ => true,
        }
    }

    /// Whether backfilling an ancestor's own `Mixed` tag is safe: `false`
    /// only when the child's own *native* resolution (`ty_native`/
    /// `return_type_native`, computed with no docblock merge at all) is
    /// itself something more specific than `Mixed` - inheriting a `Mixed`
    /// tag can only ever destroy that kind of real information, never add
    /// any (`Doctrine\DBAL\Types\Type::convertToPHPValue(): mixed`
    /// backfilling onto `AbstractStringVoType::convertToPHPValue():
    /// ?StringVo`, a real corpus false positive caught verifying this
    /// pass). When the child's own native resolution is *already*
    /// `Mixed` too (a genuinely untyped native parameter, PSR-3's own
    /// `LoggerInterface::log(mixed $level, ...)` shape), there is nothing
    /// to lose - inheriting the ancestor's own `Mixed` here is exactly
    /// what correctly propagates `explicit_mixed_hint` (Phase 88) down
    /// from wherever it was genuinely, deliberately written, rather than
    /// leaving the position looking entirely undocumented.
    pub(crate) fn safe_to_backfill_mixed(ancestor_ty: &Type, native_ty: &Type) -> bool {
        *ancestor_ty != Type::Mixed || *native_ty == Type::Mixed
    }

    /// One pass of `flatten_phpdoc_inheritance` - see its own doc comment.
    /// Returns whether anything was actually changed, so the caller knows
    /// when a fixed point has been reached.
    ///
    /// Gates each backfill on the *ancestor's* own `has_own_doc`/
    /// `return_has_own_doc` bit, not merely on the ancestor having *a*
    /// type at all - real PHPStan's own merge only ever pulls from an
    /// ancestor's own parsed `@param`/`@return` *tag*; an ancestor with
    /// no tag of its own has nothing to contribute, even though its
    /// `ty`/`return_type` field is never literally empty (it still holds
    /// whatever the native hint alone resolved to). Confirmed necessary
    /// by a real corpus regression caught while implementing this pass:
    /// `Base14::make(): int` / `Sub14::make(): string`, a genuine
    /// Liskov-violating override with no docblock anywhere - naively
    /// backfilling `int` onto `Sub14` from `Base14`'s bare native
    /// resolution would have both hidden the real override diagnostic
    /// behind a fabricated docblock type and produced a bogus
    /// `return.type` mismatch against the still-native-`string` return
    /// body.
    ///
    /// A successful backfill *does* flip `has_own_doc`/
    /// `return_has_own_doc` to `true` - once resolved, a position counts
    /// as "documented" (own or inherited) for any *further* descendant's
    /// own lookup, which is exactly what makes multi-level transitivity
    /// work one level per pass (an interface only two levels up from a
    /// concrete class, itself redeclaring the method with no doc of its
    /// own, only becomes a usable source on the pass *after* it was
    /// itself backfilled) - and, as a side effect, is what makes a
    /// resolved position permanently ineligible for re-visiting on a
    /// later pass, the same never-revisit-a-filled-slot shape
    /// `flatten_traits_one_pass`'s own `Entry::Vacant` check already has.
    /// The collection loop below only ever *reads* the registry
    /// (`self.classes.get`, `lookup_ancestor_method_with_owner`'s own
    /// read-only ancestor-chain walk, `built_in_stub_classes.contains`) -
    /// no write happens until the sequential apply loop afterward, the
    /// identical shape `flatten_traits_one_pass` already parallelizes -
    /// same reborrow-as-`&Registry` technique, `flat_map` instead of
    /// `filter_map` since one class can contribute more than one
    /// backfill (one per method).
    fn flatten_phpdoc_inheritance_one_pass(&mut self) -> bool {
        type Backfill = (
            String,
            String,
            Vec<(usize, Type, bool)>,
            Option<(Type, bool)>,
        );

        let class_names: Vec<String> = self.classes.keys().cloned().collect();
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
                    // Real PHPStan's own identical carve-out
                    // (`PhpDocInheritanceResolver::resolvePhpDocForMethod`:
                    // `$parentMethod->getName() !== '__construct' ||
                    // !$parentMethod->getDeclaringClass()->isBuiltin()`) - a
                    // subclass's own `__construct` legitimately takes
                    // completely unrelated parameters from a native/engine
                    // ancestor's own constructor (real PHP doesn't require
                    // constructor signature compatibility at all), so
                    // `RuntimeException`/`Exception`'s own `@param string
                    // $message` must never backfill onto e.g. `final class
                    // ResponseReadyException extends RuntimeException {
                    // public function __construct(private readonly
                    // ResponseInterface $response) {...} }` - a genuine
                    // corpus false positive caught verifying this pass.
                    // `built_in_stub_classes` (populated by
                    // `merge_stub_classes`, phpstorm-stubs' own native/
                    // engine-class declarations) is this project's
                    // equivalent of `ClassReflection::isBuiltin()` (real
                    // PHP's `ReflectionClass::isInternal()`) - true for
                    // `Exception`/`ArrayObject`/... but not for a real
                    // vendored userland base class (Symfony's `Command`,
                    // Doctrine's console commands, ...), which must still
                    // participate in this pass normally.
                    if method_name == "__construct"
                        && registry.built_in_stub_classes.contains(&owner)
                    {
                        continue;
                    }
                    let mut param_fills = Vec::new();
                    if needs_param {
                        // Real PHP never enforces constructor-signature
                        // compatibility (arity, order, or names can differ
                        // freely between a class and its parent), so
                        // `remapParameterNames`'s usual by-*position*
                        // correspondence - correct for a real, LSP-bound
                        // override, where PHP itself guarantees positions
                        // line up - cannot be trusted for `__construct`.
                        // Real PHPStan special-cases exactly this
                        // (`resolveMethodPhpDocFromParentClass`'s own
                        // `$lowercaseMethodName !== '__construct'` guard
                        // around building the real positional-name list, vs.
                        // falling back to the child's own names, which makes
                        // its later remap a same-name-only match): match by
                        // parameter *name* instead. Real corpus false
                        // positive caught verifying this pass: `Doctrine
                        // Migrations\Tools\Console\Command\DoctrineCommand`'s
                        // own `__construct(?DependencyFactory
                        // $dependencyFactory, ?string $name)` has no own
                        // doc; positional matching against `Symfony\
                        // Component\Console\Command\Command::__construct(
                        // ?string $name)` (own `@param string|null $name`)
                        // would wrongly stamp `string|null` onto position 0
                        // (`$dependencyFactory`) - name matching correctly
                        // leaves it alone and still lets the genuinely
                        // same-named `$name` inherit.
                        if method_name == "__construct" {
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
                                    && !Registry::type_mentions_foreign_scoped_template_param(
                                        &ancestor_param.ty,
                                        method_name,
                                    )
                                    && Registry::safe_to_backfill_object_type(
                                        &ancestor_param.ty,
                                        &p.ty_native,
                                    )
                                {
                                    param_fills.push((
                                        i,
                                        function_sig::reconcile_with_native(
                                            ancestor_param.ty.clone(),
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
                                    && !Registry::type_mentions_foreign_scoped_template_param(
                                        &ancestor_param.ty,
                                        method_name,
                                    )
                                    && Registry::safe_to_backfill_object_type(
                                        &ancestor_param.ty,
                                        &p.ty_native,
                                    )
                                {
                                    param_fills.push((
                                        i,
                                        function_sig::reconcile_with_native(
                                            ancestor_param.ty.clone(),
                                            p.ty_native.clone(),
                                        ),
                                        ancestor_param.explicit_mixed_hint,
                                    ));
                                }
                            }
                        }
                    }
                    // `safe_to_backfill_mixed` (both here and in the param
                    // loops above) isn't about `__construct` at all - see its
                    // own doc comment for the two real corpus shapes it
                    // reconciles (Doctrine DBAL's `Type::convertToPHPValue():
                    // mixed` must never overwrite a concrete native return
                    // type, but PSR-3's `LoggerInterface::log(mixed $level,
                    // ...)` must still propagate onto an equally-untyped
                    // override, or `explicit_mixed_hint` never reaches it).
                    let return_fill = (needs_return
                        && ancestor_sig.return_has_own_doc
                        && Registry::safe_to_backfill_mixed(
                            &ancestor_sig.return_type,
                            &sig.return_type_native,
                        )
                        && !Registry::type_mentions_foreign_scoped_template_param(
                            &ancestor_sig.return_type,
                            method_name,
                        )
                        && Registry::safe_to_backfill_object_type(
                            &ancestor_sig.return_type,
                            &sig.return_type_native,
                        ))
                    .then(|| {
                        (
                            function_sig::reconcile_with_native(
                                ancestor_sig.return_type.clone(),
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
/// resolve_deferred_type_imports`'s own doc comment explains why this is
/// exactly the prefix a bare `@phpstan-type` alias name declared on that
/// same class resolves against.
fn class_namespace(fqn: &str) -> &str {
    fqn.rsplit_once('\\').map_or("", |(ns, _)| ns)
}

/// `NameContext::declared_name`'s exact logic, replicated for a namespace
/// string reconstructed after the fact (`class_namespace`) rather than a
/// live `NameContext` - no leading `\` for the global namespace, which a
/// naive `format!("{ns}\\{bare}")` would always insert (a real bug this
/// project's own global-namespace corpus fixtures caught immediately: an
/// alias declared with no namespace at all keys its own `type_aliases`
/// entry under the bare name, never `"\\Name"`).
fn declared_name_in_namespace(namespace: &str, bare: &str) -> String {
    if namespace.is_empty() {
        bare.to_string()
    } else {
        format!("{namespace}\\{bare}")
    }
}

/// Cheap, read-only pre-check for `Registry::resolve_deferred_value_of`:
/// does `ty` contain the `"$$valueof"` sentinel anywhere in its tree at
/// all - lets every call site skip the (much more expensive) rebuild-with-
/// clone `resolve_deferred_value_of_in_type` below does for the
/// overwhelming majority of types, which never contain it.
fn type_contains_deferred_value_of(ty: &Type) -> bool {
    match ty {
        Type::Object(name, args) => {
            name == "$$valueof" || args.iter().any(type_contains_deferred_value_of)
        }
        Type::Array(key, inner) => {
            type_contains_deferred_value_of(key) || type_contains_deferred_value_of(inner)
        }
        Type::ClassString(inner) => type_contains_deferred_value_of(inner),
        Type::Union(members) | Type::Intersection(members) => {
            members.iter().any(type_contains_deferred_value_of)
        }
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => fields
            .iter()
            .any(|(_, t, _)| type_contains_deferred_value_of(t)),
        _ => false,
    }
}

/// The actual substitution - mirrors `function_sig::substitute_type_aliases`'s
/// own recursive-rebuild shape exactly, with one extra case (checked
/// first) for the `"$$valueof"` sentinel itself. See `Registry::
/// resolve_deferred_value_of`'s own doc comment for why this exists at
/// all, and `types::parse_type_keyword_or_class`'s `"value-of"` arm for
/// where the sentinel is produced.
fn resolve_deferred_value_of_in_type(ty: Type, classes: &FxHashMap<String, ClassInfo>) -> Type {
    match ty {
        Type::Object(name, mut args) if name == "$$valueof" => {
            let Some(Type::Object(target_fqn, _)) = args.pop() else {
                return Type::Mixed;
            };
            resolve_value_of_target(&target_fqn, classes)
        }
        Type::Object(name, args) => Type::Object(
            name,
            args.into_iter()
                .map(|a| resolve_deferred_value_of_in_type(a, classes))
                .collect(),
        ),
        Type::Array(key, inner) => Type::Array(
            Box::new(resolve_deferred_value_of_in_type(*key, classes)),
            Box::new(resolve_deferred_value_of_in_type(*inner, classes)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(resolve_deferred_value_of_in_type(*inner, classes)))
        }
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| resolve_deferred_value_of_in_type(m, classes))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|m| resolve_deferred_value_of_in_type(m, classes))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, resolve_deferred_value_of_in_type(t, classes), opt))
                .collect(),
        ),
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, resolve_deferred_value_of_in_type(t, classes), opt))
                .collect(),
        ),
        other => other,
    }
}

/// `value-of<target_fqn>`'s own real resolved value, once `target_fqn`'s
/// class/enum data actually exists to consult - mirrors real PHPStan's own
/// `ValueOfType::getResult()` exactly: not an enum at all (or not a
/// registered class/enum) degrades to `Mixed` (can't verify, don't guess -
/// the same leniency real PHPStan's own non-enum `getIterableValueType()`
/// fallback amounts to for an ordinary class); an unbacked enum (no real
/// `->value` property, since real PHP never gives one to a non-backed
/// enum) is `Never`, matching `ValueOfType::getResult()`'s own
/// `count($valueTypes) === 0 -> NeverType`; a backed enum's own already-
/// built `properties["value"]` (`registry/class_info.rs`, built for the
/// enum's real `->value` property) is already the exact union-of-case-
/// literal-values type real PHPStan computes independently - reused
/// directly, not recomputed.
fn resolve_value_of_target(target_fqn: &str, classes: &FxHashMap<String, ClassInfo>) -> Type {
    let Some(info) = classes.get(target_fqn) else {
        return Type::Mixed;
    };
    if info.kind != ClassKind::Enum {
        return Type::Mixed;
    }
    match info.properties.get("value") {
        Some(sig) => sig.ty.clone(),
        None => Type::Never,
    }
}

/// Resolves `ty` (a param/return type that may contain the `"$$valueof"`
/// sentinel) and, if the result is bare `Type::Mixed`, falls back to
/// `native` instead when that's itself something more useful - restores
/// real PHPStan's own (and this project's pre-existing)
/// `function_sig::reconcile_with_native` behavior for a `value-of<T>`
/// that turns out unresolvable (`T` not a real, registered enum), which
/// `member_possible_given_native`'s own deliberate, unconditional trust of
/// the still-unresolved sentinel (see its own doc comment) would otherwise
/// have already locked in as "kept" well before this pass ever runs. Real
/// corpus false positive this fixes: `php-standard-library`'s own
/// `BackedEnumValueType::coerce(): string|int`, `@return value-of<T>` for
/// an unresolvable `@template T` - without this, the native `string|int`
/// this project already trusted here before this whole feature existed
/// was silently replaced by a bare, strictly worse `Mixed`.
///
/// Only `ParamSig::ty`/`FunctionSig::return_type` get this treatment -
/// they're the only two places this registry persists a native-only
/// counterpart type to fall back to at all (`ParamSig::ty_native`/
/// `FunctionSig::return_type_native`, both built for an unrelated,
/// earlier override-compatibility need). `PropertySig`/`ConstSig`/
/// `ClassInfo::template_bounds` have no such counterpart stored (Phase
/// 64's own disclosed, pre-existing gap - see its own ROADMAP entry) -
/// an unresolvable `value-of<T>` there stays plain `Mixed`, a narrower,
/// disclosed simplification with no current corpus evidence it matters.
fn resolve_deferred_value_of_with_native_fallback(
    ty: Type,
    native: &Type,
    classes: &FxHashMap<String, ClassInfo>,
) -> Type {
    let resolved = resolve_deferred_value_of_in_type(ty, classes);
    if resolved == Type::Mixed && *native != Type::Mixed {
        native.clone()
    } else {
        resolved
    }
}
