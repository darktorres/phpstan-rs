use std::collections::{HashSet, VecDeque};

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::ClassKind;
use crate::registry::Registry;
use crate::registry::{ClassInfo, ConstSig, FunctionSig, PropertySig};
use crate::types::Type;

impl Registry {
    /// Does `class` derive from or implement `of`, directly or transitively?
    /// A single `HashSet::contains` against `self.ancestor_names` (see
    /// `precompute_ancestor_names`'s own doc comment) - this used to be a
    /// fresh `extends`+`implements` DFS, with its own per-call `HashSet`/
    /// `Vec` allocations, on every one of this function's real call sites
    /// (dozens, all in `checker/*`/`assignability.rs`, all read-only and
    /// post-load - confirmed by grepping every call site before making
    /// this change). Found and fixed via a perf-optimization investigation
    /// workflow.
    pub fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        class == of
            || self
                .ancestor_names
                .get(class)
                .is_some_and(|ancestors| ancestors.contains(of))
    }

    /// The exact `extends`+`implements`-only transitive closure
    /// `is_subclass_of` itself used to walk live on every call - NOT
    /// `all_ancestor_names`'s superset (which also chains `uses`, a trait
    /// relationship `instanceof`/`is_subclass_of` never recognizes, see
    /// that function's own doc comment). `class` itself is never included
    /// (matching `is_subclass_of`'s own separate `class == of` check).
    fn class_extends_implements_closure(&self, class: &str) -> FxHashSet<String> {
        let mut seen = FxHashSet::default();
        let mut stack = vec![class.to_string()];
        let mut result = FxHashSet::default();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            for parent in info.extends.iter().chain(info.implements.iter()) {
                result.insert(parent.clone());
                stack.push(parent.clone());
            }
        }
        result
    }

    /// Rebuilds `self.ancestor_names` (see `is_subclass_of`'s own doc
    /// comment) from scratch over every currently-registered class - call
    /// this any time the class graph could have changed, exactly the same
    /// "just recompute the whole thing" precedent every other `finish_
    /// loading`-style pass in this file/`resolve.rs` already follows
    /// (`canonicalize_class_name_case`, `flatten_traits`, ...). Called
    /// from `Registry::index_and_flatten` (covers `Registry::build` and
    /// `php_stubs.rs`'s own stub-extraction registries), `Project::
    /// finish_loading`, and `Project::update_file` - every place that
    /// finishes building or mutating a registry's class graph, so `is_
    /// subclass_of` can always trust this map is current. Read-only over
    /// `self.classes`, so it parallelizes the same way `flatten_traits_
    /// one_pass` does: reborrow as `&Registry` for the DFS itself, then a
    /// plain sequential map assignment - unlike `flatten_traits_one_pass`,
    /// there's no per-class "apply" step, since no class's own result can
    /// invalidate another's.
    #[hotpath::measure]
    pub fn precompute_ancestor_names(&mut self) {
        let class_names: Vec<String> = self.classes.keys().cloned().collect();
        let registry: &Registry = self;
        self.ancestor_names = class_names
            .par_iter()
            .map(|name| {
                (
                    name.clone(),
                    registry.class_extends_implements_closure(name),
                )
            })
            .collect();
    }

    /// Is `class` (or any ancestor it `extends`) tagged `@phpstan-immutable`
    /// (or `@phpstan-readonly` on the class itself - both fold into
    /// `ClassInfo::is_immutable` already, see its own doc comment)?
    /// Query-time, `extends`-only (mirrors real PHPStan's own
    /// `ClassReflection::isImmutable()`, which walks `getParentClass()`
    /// only - never `implements`/`uses`, matching `is_immutable`'s own
    /// deliberate non-inheriting-at-index-time design in `class_info.rs`).
    pub(crate) fn class_is_immutable(&self, class: &str) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![class.to_string()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            if info.is_immutable {
                return true;
            }
            stack.extend(info.extends.iter().cloned());
        }
        false
    }

    /// Every interface/trait `class` transitively inherits from at all -
    /// `extends`/`implements` (the same walk `is_subclass_of` already
    /// does, just collecting every name visited instead of stopping at
    /// the first match) *and* `uses`, which `is_subclass_of` doesn't chain
    /// at all (a used trait isn't itself a supertype PHP's own `instanceof`
    /// recognizes). `class` itself is never included. `@phpstan-require-
    /// extends`/`-implements`'s own consumer check (`checker::oop::
    /// check_require_tags`) needs this: real PHPStan's `getInterfaces()`/
    /// `getTraits(true)` are both transitive the identical way.
    pub(crate) fn all_ancestor_names(&self, class: &str) -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut stack = vec![class.to_string()];
        let mut result = HashSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            for parent in info
                .extends
                .iter()
                .chain(info.implements.iter())
                .chain(info.uses.iter())
            {
                result.insert(parent.clone());
                stack.push(parent.clone());
            }
        }
        result
    }

    /// Every real class/enum that ends up composing `trait_fqn`, walking
    /// past any intermediate *trait* user rather than stopping at direct
    /// `uses` alone (Phase 13 Finding E) - `flatten_traits` merges a used
    /// trait's own methods into the *user's* method map, never the other
    /// way around, so a trait used only by another trait (`trait B { ... }
    /// trait A { use B; } class X { use A; }`) still needs `B` checked
    /// against `X`, the real, final host - checking it against `A` alone
    /// would see exactly the same host-unaware member set `A` itself has.
    pub(crate) fn classes_using_trait(&self, trait_fqn: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut stack = vec![trait_fqn.to_string()];
        let mut seen = HashSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            for (name, info) in &self.classes {
                if !info.uses.iter().any(|u| u == &current) {
                    continue;
                }
                if info.kind == ClassKind::Trait {
                    stack.push(name.clone());
                } else {
                    result.push(name.clone());
                }
            }
        }
        result
    }

    /// The exhaustive BFS build `precompute_member_owners` runs once per
    /// class, replacing what `lookup_method`/`lookup_property`/`lookup_
    /// const`/their `_with_owner` siblings used to walk live on every
    /// call. Walks `class`'s own `extends` (*and* `implements`, so an
    /// interface-typed receiver's own multi-parent `extends` chain and a
    /// class's declared-but-not-overridden interface methods both resolve)
    /// ancestor graph, in the same order `is_subclass_of` does, recording
    /// every method/property/const name found under the *closest*
    /// (first-visited-by-BFS) declaring ancestor - `class` itself
    /// included, checked before any parent. A single-parent-only walk
    /// (`extends.first()`) would be wrong for interfaces specifically,
    /// since unlike a class, PHP lets an interface `extends` more than one
    /// parent at once (real example that surfaced this: league/flysystem's
    /// `FilesystemOperator extends FilesystemReader, FilesystemWriter`).
    ///
    /// Fused: this used to be a single generic `own_owner_names(class,
    /// names_of: impl Fn(&ClassInfo) -> Vec<&str>)` called three separate
    /// times per class (once each for methods/properties/consts), each
    /// independently re-walking the identical ancestor graph with its own
    /// queue/seen-set/lookup - the BFS topology never depends on which
    /// member kind is being extracted, only on `class`'s own `extends`/
    /// `implements`, so walking it once and extracting all three kinds at
    /// each visited node reproduces the exact same per-map first-visited-
    /// wins result while removing two of the three seen-set/queue/lookup
    /// passes. `contains_key` guards (rather than `entry(name.to_string())
    /// .or_insert_with(...)`) avoid allocating a String key on a name a
    /// closer ancestor already claimed. Found and fixed via a perf-
    /// optimization investigation workflow; the old three-call version
    /// measured at 1.91s/124K calls (Symfony), 3.62s/123K calls (Drupal).
    fn own_owner_names(
        &self,
        class: &str,
    ) -> (
        FxHashMap<String, String>,
        FxHashMap<String, String>,
        FxHashMap<String, String>,
    ) {
        let mut method_owner = FxHashMap::default();
        let mut property_owner = FxHashMap::default();
        let mut const_owner = FxHashMap::default();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(class.to_string());
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            for name in info.methods.keys() {
                if !method_owner.contains_key(name) {
                    method_owner.insert(name.clone(), current.clone());
                }
            }
            for name in info.properties.keys() {
                if !property_owner.contains_key(name) {
                    property_owner.insert(name.clone(), current.clone());
                }
            }
            for name in info.consts.keys() {
                if !const_owner.contains_key(name) {
                    const_owner.insert(name.clone(), current.clone());
                }
            }
            for parent in info.extends.iter().chain(info.implements.iter()) {
                queue.push_back(parent.clone());
            }
        }
        (method_owner, property_owner, const_owner)
    }

    /// Builds an `ancestor_*_owner_by_class` entry for `class` from
    /// already-computed `own` maps - correct, not just fast, because of
    /// how `lookup_ancestor_method_with_owner`/`lookup_ancestor_property_
    /// with_owner` are specified: for each of `class`'s own direct
    /// parents *in order*, try that parent's *entire* ancestor chain
    /// before ever trying the next parent - not a single BFS merged
    /// across all parents at once. A parent's own `own_owner_names` entry
    /// already correctly captures "first found within that parent's own
    /// BFS" (`class` itself included in fully bottom of its BFS start),
    /// so merging each direct parent's own map in order, first-insert-
    /// wins, reproduces that exact "try parent 0 fully, then parent 1
    /// fully" priority without re-walking anything.
    fn merge_ancestor_owner_names(
        parents: impl Iterator<Item = String>,
        own_owner_by_class: &FxHashMap<String, FxHashMap<String, String>>,
    ) -> FxHashMap<String, String> {
        let mut merged = FxHashMap::default();
        for parent in parents {
            if let Some(parent_map) = own_owner_by_class.get(&parent) {
                for (name, owner) in parent_map {
                    if !merged.contains_key(name) {
                        merged.insert(name.clone(), owner.clone());
                    }
                }
            }
        }
        merged
    }

    /// Rebuilds every `*_owner_by_class` map (see their own doc comments
    /// on `Registry`) from scratch over every currently-registered class -
    /// call this any time the class graph could have changed, exactly the
    /// same "just recompute the whole thing" precedent `precompute_
    /// ancestor_names` already follows (called from the identical settle
    /// points: `Registry::index_and_flatten`/`build`, `Project::finish_
    /// loading`/`update_file`).
    ///
    /// Replaces a fresh BFS (`own_owner_names`'s own per-call `HashSet`/
    /// `VecDeque`, or worse, a full sequential-per-parent walk for the
    /// `lookup_ancestor_*` siblings) on every `lookup_method`/`lookup_
    /// property`/`lookup_const`/their `_with_owner`/`lookup_ancestor_*`
    /// call - measured, via a perf-optimization investigation workflow,
    /// at roughly half of total wall time combined across two real
    /// corpora before this change (Symfony: ~49%, `lookup_method_with_
    /// owner` alone 1.46M calls/21.81%; Drupal: ~93%, `lookup_method_
    /// with_owner` alone 1.87M calls/51.93%). `find_method_signature_
    /// prototype`'s own hand-ordered interface/trait/extends priority
    /// walk is untouched - it only calls `lookup_method_with_owner` as a
    /// sub-step, so it benefits automatically without needing its own
    /// cache or any change to its own ordering logic.
    ///
    /// Two phases, `own` then `ancestor`, since every `ancestor_*_owner_
    /// by_class` entry is built by merging its class's own already-
    /// computed direct parents' `own` maps (`merge_ancestor_owner_names`'s
    /// own doc comment) - every class's `own` map must finish before any
    /// class's `ancestor` map starts.
    #[hotpath::measure]
    pub fn precompute_member_owners(&mut self) {
        let class_names: Vec<String> = self.classes.keys().cloned().collect();
        let registry: &Registry = self;

        // Every `self.X = ...`-style assignment below is deferred to the
        // very end of this function, once `registry` (a borrow of the
        // *whole* `self`, including every field being written here) is no
        // longer needed - each intermediate map is built into a plain
        // local first, exactly as the parallel computation itself already
        // requires (`registry` must outlive every `par_iter()` call
        // below, including the two `ancestor_*` ones that read `method_
        // owner_by_class`/`property_owner_by_class` back as locals, not
        // as `self` fields, for the same reason).
        // A single fused BFS per class builds all three own-maps at once
        // (see `own_owner_names`'s own doc comment for why this is safe -
        // the ancestor-walk topology never depends on which member kind
        // is being extracted) rather than three independent walks.
        let own_by_class: Vec<_> = class_names
            .par_iter()
            .map(|name| {
                let (methods, properties, consts) = registry.own_owner_names(name);
                (name.clone(), methods, properties, consts)
            })
            .collect();
        let mut method_owner_by_class = FxHashMap::default();
        let mut property_owner_by_class = FxHashMap::default();
        let mut const_owner_by_class = FxHashMap::default();
        for (name, methods, properties, consts) in own_by_class {
            method_owner_by_class.insert(name.clone(), methods);
            property_owner_by_class.insert(name.clone(), properties);
            const_owner_by_class.insert(name, consts);
        }

        let ancestor_method_owner_by_class: FxHashMap<String, FxHashMap<String, String>> =
            class_names
                .par_iter()
                .map(|name| {
                    let parents = registry
                        .classes
                        .get(name)
                        .map(|info| {
                            info.extends
                                .iter()
                                .chain(info.implements.iter())
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    (
                        name.clone(),
                        Self::merge_ancestor_owner_names(
                            parents.into_iter(),
                            &method_owner_by_class,
                        ),
                    )
                })
                .collect();

        // Real PHP interfaces can't declare properties at all, so
        // `lookup_ancestor_property_with_owner`'s own walk (unlike the
        // method version) only ever considers `extends`, never
        // `implements` - mirrored here.
        let ancestor_property_owner_by_class: FxHashMap<String, FxHashMap<String, String>> =
            class_names
                .par_iter()
                .map(|name| {
                    let parents = registry
                        .classes
                        .get(name)
                        .map(|info| info.extends.clone())
                        .unwrap_or_default();
                    (
                        name.clone(),
                        Self::merge_ancestor_owner_names(
                            parents.into_iter(),
                            &property_owner_by_class,
                        ),
                    )
                })
                .collect();

        self.method_owner_by_class = method_owner_by_class;
        self.property_owner_by_class = property_owner_by_class;
        self.const_owner_by_class = const_owner_by_class;
        self.ancestor_method_owner_by_class = ancestor_method_owner_by_class;
        self.ancestor_property_owner_by_class = ancestor_property_owner_by_class;
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
    pub fn lookup_method(&self, class: &str, method: &str) -> Option<&FunctionSig> {
        let owner = self.method_owner_by_class.get(class)?.get(method)?;
        self.classes.get(owner)?.methods.get(method)
    }

    /// Same as `lookup_method`, but also returns the declaring ancestor's
    /// name - needed wherever a caller goes on to do class-level generic
    /// substitution (Phase 10.5's `checker::class_template_bindings`),
    /// since a `@template` name is scoped to the class that declares it,
    /// not globally unique - substituting an inherited method/property's
    /// `TemplateParam`s correctly requires knowing which ancestor's own
    /// template scope they belong to, not just the value found.
    pub fn lookup_method_with_owner(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(String, &FunctionSig)> {
        let owner = self.method_owner_by_class.get(class)?.get(method)?;
        let sig = self.classes.get(owner)?.methods.get(method)?;
        Some((owner.clone(), sig))
    }

    /// Same as `lookup_method_with_owner`, but starts the walk at
    /// `class`'s own parents rather than `class` itself - used for
    /// override-compatibility checking (Finding 50), which needs the
    /// signature a method is overriding, not the overriding one itself.
    pub fn lookup_ancestor_method_with_owner(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(String, &FunctionSig)> {
        let owner = self
            .ancestor_method_owner_by_class
            .get(class)?
            .get(method)?;
        let sig = self.classes.get(owner)?.methods.get(method)?;
        Some((owner.clone(), sig))
    }

    /// Phase 64 Item 4: faithful port of real PHPStan's own
    /// `MethodPrototypeFinder::findPrototype` priority order (confirmed by
    /// reading the full method) - used **only** by `check_one_method_
    /// override`'s own Liskov-compatibility ancestor lookup, not a
    /// replacement for `lookup_ancestor_method_with_owner`'s other call
    /// sites (runtime-dispatch/purity questions, a different question
    /// than "what governs override compatibility"). Real PHP bug this
    /// project's own plain `extends.iter().chain(implements.iter())` walk
    /// above had, confirmed live: an *immediately*-implemented interface's
    /// own native declaration always wins over an `extends` parent's
    /// (checked first here, opposite of the old order), and a used
    /// trait's own **abstract** declaration wins over the `extends`
    /// parent too when no immediate interface declares the method (the
    /// old walk never consulted a trait's own abstract declaration for
    /// this purpose at all - only as a completely separate boolean for
    /// the `#[Override]`-fatal check, `trait_declares_abstract_method`
    /// just above).
    /// Returns `(owner, sig, check_visibility)` - `check_visibility` is
    /// `false` **only** for a trait-abstract-sourced prototype (step 2
    /// below), mirroring real `MethodPrototypeFinder::findPrototype`'s
    /// own three-element-tuple return exactly (confirmed by reading the
    /// full method, not assumed): its interface branch and its `extends`-
    /// parent branch both return `true` for this flag, but its trait
    /// branch explicitly returns `false`. This matters because real PHP
    /// itself does **not** enforce visibility-widening compatibility
    /// between a trait's own `abstract` method declaration and the
    /// concrete method that fulfills it (confirmed live: `trait T {
    /// abstract protected function f(); } class C { use T; private
    /// function f() {} }` compiles with no error) - a real, unconditional
    /// fatal error for the interface/extends cases, but not this one.
    /// Confirmed real corpus false positive without this distinction:
    /// Symfony's own `AmpResponse::schedule()` (`private`) fulfilling
    /// `TransportResponseTrait`'s own `abstract protected static function
    /// schedule()`.
    pub fn find_method_signature_prototype(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(String, &FunctionSig, bool)> {
        let info = self.classes.get(class)?;
        // 1. Immediately-implemented interfaces - walks each interface's
        //    *own* ancestor chain too (`lookup_method_with_owner`, not a
        //    bare `.methods.get`), not just its own direct declaration:
        //    confirmed real corpus false positive otherwise (Piwigo's own
        //    `ExtensionInterface extends SubscriberInterface`, where
        //    `subscribedEvents()` is declared only on the *grandparent*
        //    interface - a class `implements ExtensionInterface` alone
        //    still genuinely overrides it, and real PHPStan doesn't fail
        //    to see that either).
        for iface in &info.implements {
            if let Some((owner, sig)) = self.lookup_method_with_owner(iface, method) {
                return Some((owner, sig, true));
            }
        }
        // 2. A used trait's own abstract declaration (this project doesn't
        //    gate override-compatibility checks on a configured PHP
        //    version elsewhere in this file either, so unconditional here
        //    too - real PHPStan's own equivalent gate,
        //    `supportsAbstractTraitMethods`, is PHP 8.0+, a bar every
        //    realistic target version already clears).
        for used in &info.uses {
            if self
                .classes
                .get(used)
                .is_some_and(|trait_info| trait_info.abstract_methods.contains(method))
                && let Some(sig) = self.classes.get(used).and_then(|i| i.methods.get(method))
            {
                return Some((used.clone(), sig, false));
            }
        }
        // 3. Ordinary `extends` chain (existing, already-correct
        //    recursive walk).
        for parent in &info.extends {
            if let Some((owner, sig)) = self.lookup_method_with_owner(parent, method) {
                return Some((owner, sig, true));
            }
        }
        None
    }

    /// `@phpstan-consistent-constructor`'s own ancestor walk - real
    /// PHPStan's `ConsistentConstructorHelper::findConsistentConstructor`,
    /// transcribed (confirmed directly, `vendor/phpstan-src`): starting at
    /// `class`'s own *parent* (not `class` itself - matches the real
    /// rule's own call site, which already has the child's own method in
    /// hand and is looking for what it's being held *to*), walks up
    /// `extends` until an ancestor tagged `@phpstan-consistent-constructor`
    /// is found, returning that ancestor's own `__construct` - or `None`
    /// if the chain ends (or cycles - bounded by `self.classes.len()`,
    /// same guard `Registry::flatten_traits`'s own cycle protection uses)
    /// with no tagged ancestor at all, meaning no check applies, real
    /// PHP's own default.
    ///
    /// Deliberately doesn't synthesize a zero-parameter dummy constructor
    /// for a tagged ancestor with no `__construct` of its own the way real
    /// PHPStan's `DummyConstructorReflection` does - every `FunctionSig`
    /// this project returns is borrowed from `self.classes`, and a class
    /// that bothers tagging itself `@phpstan-consistent-constructor` at
    /// all almost always also declares the constructor it's pinning down
    /// (the entire point of the tag), so this narrower scope isn't
    /// expected to leave real value on the table.
    pub(crate) fn find_consistent_constructor_ancestor(
        &self,
        class: &str,
    ) -> Option<(String, &FunctionSig)> {
        let mut current = self.classes.get(class)?.extends.first()?.clone();
        for _ in 0..self.classes.len().max(1) {
            let info = self.classes.get(&current)?;
            if info.is_consistent_constructor {
                return info
                    .methods
                    .get("__construct")
                    .map(|sig| (current.clone(), sig));
            }
            current = info.extends.first()?.clone();
        }
        None
    }

    /// Phase 64 follow-up: whether `class` directly `use`s a trait that
    /// declares a method named `method` *abstract* - used only by
    /// `#[\Override]`-attribute checking, which needs to treat
    /// implementing a *trait's own* abstract method as a real override
    /// too (confirmed directly via `php -r`: a class implementing a
    /// trait's own `abstract protected function twist()` with
    /// `#[\Override]` is real, legal PHP - a genuine corpus false
    /// positive: `Psl\RandomSequence\MersenneTwisterSequence::twist()`
    /// overrides `Internal\MersenneTwisterTrait`'s own abstract
    /// declaration this exact way). Deliberately checks the trait's own
    /// `abstract_methods` set, not merely "does the trait have a method
    /// of this name at all" - a trait supplying a *concrete* method the
    /// using class never separately redeclares (own_sig IS the trait's
    /// own contribution, unmodified) is a different case entirely, with
    /// its own already-correct handling: `check_method_overrides`'s own
    /// trait-derived-method sweep re-runs this same function keyed on the
    /// using class, so a genuinely ancestor-less concrete trait method
    /// still fatals there exactly as real PHP does (confirmed directly:
    /// `trait T { #[\Override] function foo() {} } class Standalone {
    /// use T; }` still fatals on `Standalone::foo()`) - this check must
    /// not accidentally swallow that real case too. Deliberately narrower
    /// than folding `uses` into `lookup_ancestor_method_with_owner`'s own
    /// general walk (the "Still open: interface-first/trait-abstract-
    /// method ancestor-lookup priority" item this project's own roadmap
    /// already tracks separately) - that would also change every *other*
    /// check in `check_one_method_override` (Liskov parameter/return-type
    /// comparisons) in ways not verified here; this only answers the one
    /// yes/no question the override-attribute check itself needs.
    pub fn trait_declares_abstract_method(&self, class: &str, method: &str) -> bool {
        let Some(info) = self.classes.get(class) else {
            return false;
        };
        info.uses.iter().any(|used_trait| {
            self.classes
                .get(used_trait)
                .is_some_and(|trait_info| trait_info.abstract_methods.contains(method))
        })
    }

    /// Phase 64 follow-up: whether `class`'s own `extends`/`implements`
    /// ancestor chain includes a name this project's registry has no
    /// declaration for at all, at any depth - used only by
    /// `#[\Override]`-attribute checking, which must never confidently
    /// claim "no matching parent method exists" when the real reason
    /// nothing was found is a missing dependency rather than a genuine
    /// absence (a genuine corpus false positive: `CycloneDX\Composer\
    /// Plugin implements ... Composer\Plugin\PluginInterface` and
    /// `CycloneDX\Composer\_internal\MakeBom\Command extends Composer\
    /// Command\BaseCommand` - neither vendored anywhere in this project's
    /// own verification corpus at all, so this registry has no way to
    /// know whether the real `activate()`/`configure()`/... methods being
    /// overridden are declared there, the same "don't guess wrong" rule
    /// this project applies elsewhere). Every other check in
    /// `check_one_method_override` stays as lenient-by-omission as it
    /// already was (an unresolved ancestor simply yields no comparison,
    /// same as a genuinely absent one) - only this specific check's
    /// polarity (firing exactly when *nothing* is found) makes an
    /// unresolved-vs-absent distinction actually matter.
    pub fn has_unresolvable_ancestor(&self, class: &str) -> bool {
        let Some(info) = self.classes.get(class) else {
            return false;
        };
        let mut seen = HashSet::new();
        let mut queue: VecDeque<String> = info
            .extends
            .iter()
            .chain(info.implements.iter())
            .cloned()
            .collect();
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(current_info) = self.classes.get(&current) else {
                return true;
            };
            queue.extend(
                current_info
                    .extends
                    .iter()
                    .chain(current_info.implements.iter())
                    .cloned(),
            );
        }
        false
    }

    /// Phase 64's own still-open property-override half: same idea as
    /// `lookup_ancestor_method_with_owner` just above, but walking only
    /// `extends` - real PHP interfaces can't declare properties at all,
    /// so there's no `implements` side to check (`precompute_member_
    /// owners`'s own `ancestor_property_owner_by_class` build mirrors
    /// this same `extends`-only restriction).
    pub fn lookup_ancestor_property_with_owner(
        &self,
        class: &str,
        prop: &str,
    ) -> Option<(String, &PropertySig)> {
        let owner = self
            .ancestor_property_owner_by_class
            .get(class)?
            .get(prop)?;
        let sig = self.classes.get(owner)?.properties.get(prop)?;
        Some((owner.clone(), sig))
    }

    /// Finding 46: real PHP forbids overriding a method declared `final`
    /// anywhere in the ancestor chain, no matter how many intervening
    /// subclasses sit between the `final` declaration and this one (a
    /// method that's final stays final for every descendant, not just
    /// its immediate subclass). Walks only `extends` - real PHP's own
    /// `final` keyword applies to ordinary class inheritance; an
    /// interface's own methods have no bodies to protect, and this
    /// project doesn't yet propagate a trait's own `final_methods` the
    /// way it does `abstract_methods` (Finding 46 as reported is scoped
    /// to class inheritance, matching its own repro). Returns the name of
    /// the ancestor that declared it final, plus whether that was via
    /// `@final` (docblock, PHPStan-only convention, real PHP doesn't
    /// enforce it) rather than the real keyword - checked together, one
    /// ancestor at a time nearest-first, real keyword winning over
    /// `@final` when a single ancestor somehow has both (mirrors real
    /// PHPStan's own `OverridingMethodRule` if/elseif order exactly,
    /// `method.parentMethodFinal` vs. `method.parentMethodFinalByPhpDoc`).
    pub fn find_final_ancestor_method(&self, class: &str, method: &str) -> Option<(String, bool)> {
        let info = self.classes.get(class)?;
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = info.extends.clone();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(current_info) = self.classes.get(&current) else {
                continue;
            };
            if current_info.final_methods.contains(method) {
                return Some((current, false));
            }
            if current_info.final_methods_by_phpdoc.contains(method) {
                return Some((current, true));
            }
            stack.extend(current_info.extends.iter().cloned());
        }
        None
    }

    /// Phase 64 Item 3: `find_final_ancestor_method`'s property-side
    /// mirror - PHP 8.4's own `final` property modifier. Walks only
    /// `extends`, same reasoning as the method version (properties have
    /// no `implements`/interface side at all, and this project doesn't
    /// propagate a trait's own `final_properties` the way it does
    /// `abstract_methods` either). See that function's own doc comment
    /// for the `@final`/keyword priority the returned `bool` encodes.
    pub fn find_final_ancestor_property(
        &self,
        class: &str,
        property: &str,
    ) -> Option<(String, bool)> {
        let info = self.classes.get(class)?;
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = info.extends.clone();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(current_info) = self.classes.get(&current) else {
                continue;
            };
            if current_info.final_properties.contains(property) {
                return Some((current, false));
            }
            if current_info.final_properties_by_phpdoc.contains(property) {
                return Some((current, true));
            }
            stack.extend(current_info.extends.iter().cloned());
        }
        None
    }

    /// Phase 42 (`MethodThrowTypeCovarianceRule`): every interface
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
    ) -> Vec<(String, &FunctionSig)> {
        let mut seen_classes = HashSet::new();
        let mut class_queue: VecDeque<String> = VecDeque::new();
        class_queue.push_back(class.to_string());
        let mut seen_ifaces = HashSet::new();
        let mut result = Vec::new();
        while let Some(c) = class_queue.pop_front() {
            if !seen_classes.insert(c.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&c) else {
                continue;
            };
            class_queue.extend(info.extends.iter().cloned());
            let mut iface_queue: VecDeque<String> = info.implements.iter().cloned().collect();
            while let Some(iface) = iface_queue.pop_front() {
                if !seen_ifaces.insert(iface.clone()) {
                    continue;
                }
                let Some(iface_info) = self.classes.get(&iface) else {
                    continue;
                };
                if let Some(sig) = iface_info.methods.get(method) {
                    result.push((iface.clone(), sig));
                }
                iface_queue.extend(iface_info.extends.iter().cloned());
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
    /// `flatten_traits` already folds a trait's own `abstract_methods`
    /// into each using class's own set (unless the using class supplies
    /// its own concrete override), so a plain walk of `abstract_methods`
    /// here already covers it, with no separate trait-specific case
    /// needed.
    pub fn required_abstract_methods(&self, class: &str) -> Vec<String> {
        let mut required = HashSet::new();
        let mut seen = HashSet::new();
        let mut stack = vec![class.to_string()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            if info.kind == ClassKind::Interface {
                // A purely magic `@method` tag (`FunctionSig::is_magic`'s
                // own doc comment - real corpus false positive this
                // excludes: Tracy's own `IBarPanel`, `@method ?string
                // getAgentInfo()`) documents a capability, not a
                // requirement - real PHP itself has no way to even
                // express "this interface method is optional," so an
                // interface's own *real*, textually-declared methods are
                // still all unconditionally required exactly as before.
                required.extend(
                    info.methods
                        .iter()
                        .filter(|(_, sig)| !sig.is_magic)
                        .map(|(name, _)| name.clone()),
                );
            } else {
                required.extend(info.abstract_methods.iter().cloned());
            }
            for parent in info.extends.iter().chain(info.implements.iter()) {
                stack.push(parent.clone());
            }
        }
        let mut required: Vec<String> = required.into_iter().collect();
        required.sort();
        required
    }

    /// A required method (see `required_abstract_methods`) counts as
    /// implemented only if *some* reachable ancestor provides a concrete
    /// one - not necessarily the *closest* ancestor with any entry at
    /// all. Deliberately doesn't reuse `lookup_method_with_owner`'s
    /// closest-match BFS: a class can `implements Throwable` directly
    /// (graph-adjacent) while its real concrete `getCode()` etc. only
    /// live two hops further up an `extends` chain (e.g. `BaseException`
    /// -> real `Exception`) - the closest *node* with any entry for the
    /// name is the interface's own bodyless declaration, even though a
    /// real implementation is reachable elsewhere. So this walks the
    /// whole ancestor graph looking for any concrete match, rather than
    /// stopping at the first node that merely mentions the name at all.
    pub fn has_concrete_implementation(&self, class: &str, method: &str) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![class.to_string()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            if info.kind != ClassKind::Interface
                && info.methods.contains_key(method)
                && !info.abstract_methods.contains(method)
            {
                return true;
            }
            for parent in info.extends.iter().chain(info.implements.iter()) {
                stack.push(parent.clone());
            }
        }
        false
    }

    /// Looks up a property, walking the ancestor graph if not declared
    /// directly on `class` (covers inherited properties, including ones
    /// introduced via constructor promotion). A single two-level
    /// `HashMap` lookup against `self.property_owner_by_class` - see
    /// `precompute_member_owners`'s own doc comment.
    pub fn lookup_property(&self, class: &str, prop: &str) -> Option<&PropertySig> {
        let owner = self.property_owner_by_class.get(class)?.get(prop)?;
        self.classes.get(owner)?.properties.get(prop)
    }

    /// Same as `lookup_property`, but also returns the declaring
    /// ancestor's name - class-level generic substitution needs it the
    /// same way `lookup_method_with_owner`'s own doc comment explains.
    pub fn lookup_property_with_owner(
        &self,
        class: &str,
        prop: &str,
    ) -> Option<(String, &PropertySig)> {
        let owner = self.property_owner_by_class.get(class)?.get(prop)?;
        let sig = self.classes.get(owner)?.properties.get(prop)?;
        Some((owner.clone(), sig))
    }

    /// Looks up a class constant, walking the ancestor graph if not
    /// declared directly on `class` - mirrors `lookup_property`.
    pub fn lookup_const(&self, class: &str, const_name: &str) -> Option<&ConstSig> {
        let owner = self.const_owner_by_class.get(class)?.get(const_name)?;
        self.classes.get(owner)?.consts.get(const_name)
    }

    /// Same as `lookup_const`, but also returns the declaring ancestor's
    /// name - Phase 67 needs it to know *which* class's own `@internal`
    /// tag (if any) actually governs a `Class::CONST` access, the same
    /// way `lookup_method_with_owner`/`lookup_property_with_owner`
    /// already do for their own members; Phase 57's own still-open item
    /// (constant visibility enforcement) needs it the identical way too.
    pub fn lookup_const_with_owner(
        &self,
        class: &str,
        const_name: &str,
    ) -> Option<(String, &ConstSig)> {
        let owner = self.const_owner_by_class.get(class)?.get(const_name)?;
        let sig = self.classes.get(owner)?.consts.get(const_name)?;
        Some((owner.clone(), sig))
    }

    /// Phase 79: a backed enum case's own literal backing value, if this
    /// checker was able to prove one (see `ClassInfo::enum_case_values`'s
    /// own doc comment) - not hierarchy-walked, since an enum can't
    /// extend anything for a case to inherit from in the first place.
    pub fn enum_case_value(&self, class: &str, case: &str) -> Option<&Type> {
        self.classes.get(class)?.enum_case_values.get(case)
    }

    /// Phase 79: the matching case (if any, and if provable) for a
    /// literal argument passed to a backed enum's own `::from()`/
    /// `::tryFrom()` - a reverse lookup over `enum_case_value` above,
    /// since PHP itself only stores value -> case at runtime, never the
    /// other way around.
    pub fn enum_case_for_value(&self, class: &str, value: &Type) -> Option<&str> {
        let info = self.classes.get(class)?;
        info.enum_case_values
            .iter()
            .find(|(_, v)| *v == value)
            .map(|(name, _)| name.as_str())
    }

    /// Phase 71 corpus find: real PHP function calls are case-
    /// insensitive (`STRTOLOWER()`/`strtolower()`/`StrToLower()` all
    /// resolve to the exact same builtin, and the same is true of a
    /// user-declared function under whatever case its own call site
    /// happens to use) - `self.functions` itself stays keyed by
    /// whatever case each function was actually *declared* under (every
    /// existing insertion call site is untouched), so an exact-case miss
    /// falls back to a case-insensitive scan before giving up. Checked
    /// second, not first: the overwhelming majority of real calls
    /// already match a declaration's own case exactly, so the common
    /// path stays a plain, allocation-free `HashMap` lookup.
    pub fn lookup_function(&self, name: &str) -> Option<&FunctionSig> {
        self.functions.get(name).or_else(|| {
            self.functions
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v)
        })
    }

    /// `lookup_function`'s own canonical-name-returning sibling, mirroring
    /// `lookup_class`'s identical `(&str, &Info)` shape - `FunctionSig`
    /// carries no `name` field of its own (unlike `ClassInfo`, whose own
    /// declared name a caller can't otherwise recover once a case-
    /// insensitive fallback match has happened), so `check_function_name_
    /// case`'s own `function.nameCase` diagnostic needs this to report
    /// the real declared casing rather than just presence.
    pub(crate) fn lookup_function_with_name(&self, name: &str) -> Option<(&str, &FunctionSig)> {
        if let Some((k, v)) = self.functions.get_key_value(name) {
            return Some((k.as_str(), v));
        }
        self.functions
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(k, v)| (k.as_str(), v))
    }

    /// Phase 48.1: `lookup_function`'s own class-side mirror - real PHP
    /// resolves class/interface/trait/enum names case-insensitively too,
    /// the same rationale `lookup_function`'s own doc comment already
    /// gives for functions. Returns the *canonical* declared key
    /// alongside the info so a caller can rebind its own local variable
    /// to it (a fresh-per-check-time lookup, not a structural registry
    /// field - see `canonicalize_class_name_case`'s own doc comment for
    /// why those two need different treatment).
    ///
    /// The case-mismatch fallback used to be a full linear scan of every
    /// registered class with a case-insensitive comparison on each - real
    /// cost on a case-mismatched *or* genuinely unresolved name (this
    /// project's own leniency elsewhere already documents the latter as
    /// common in real corpora), since `check_type_class_names_exist` calls
    /// this once per parameter and once per return type on every function
    /// checked. `self.classes_lowercase` (built once alongside
    /// `canonicalize_class_name_case`'s own identical map, see that
    /// field's own doc comment) turns it into one `HashMap` lookup plus
    /// one more exact `get` for the resolved canonical key. Found and
    /// fixed via a perf-optimization investigation workflow.
    pub fn lookup_class(&self, name: &str) -> Option<(&str, &ClassInfo)> {
        if let Some((k, v)) = self.classes.get_key_value(name) {
            return Some((k.as_str(), v));
        }
        let canonical = self.classes_lowercase.get(&name.to_ascii_lowercase())?;
        self.classes
            .get_key_value(canonical)
            .map(|(k, v)| (k.as_str(), v))
    }
}
