use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use syntax::name::Name;

use rayon::prelude::*;

use crate::registry::Registry;
use crate::registry::{
    ClassReflection, ConstantReflection, FunctionReflection, PropertyReflection,
};
use syntax::ast::ClassKind;
use types::Type;

impl Registry {
    /// Does `class` derive from or implement `of`, directly or
    /// transitively? A single `HashSet::contains` against
    /// `self.ancestor_names` (see `precompute_ancestor_names`) - cheaper
    /// than a fresh `extends`+`implements` DFS on every one of this
    /// function's dozens of read-only, post-load call sites.
    /// Whether `class` is `of` or extends or implements it, both compared
    /// as PHP compares class names, case-insensitively.
    pub fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        if class.eq_ignore_ascii_case(of) {
            return true;
        }
        let class = self.lookup_class(class).map_or(class, |(k, _)| k);
        let of = self.lookup_class(of).map_or(of, |(k, _)| k);
        self.ancestor_names
            .get(class)
            .is_some_and(|ancestors| ancestors.contains(of))
    }

    /// The exact `extends`+`implements`-only transitive closure
    /// `is_subclass_of` needs - NOT `all_ancestor_names`'s superset
    /// (which also chains `uses`, a trait relationship
    /// `instanceof`/`is_subclass_of` never recognizes). `class` itself
    /// is never included (matching `is_subclass_of`'s own separate
    /// `class == of` check).
    fn class_extends_implements_closure(&self, class: &str) -> FxHashSet<Name> {
        let mut seen = FxHashSet::default();
        let mut stack = vec![Name::from(class)];
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

    /// Rebuilds `self.ancestor_names` (see `is_subclass_of`) from
    /// scratch over every registered class - call this any time the
    /// class graph could have changed, so `is_subclass_of` can always
    /// trust this map is current. Read-only over `self.classes`, so it
    /// parallelizes like `flatten_traits_one_pass`: reborrow as
    /// `&Registry` for the DFS, then a plain sequential map assignment -
    /// unlike that function, there's no per-class "apply" step, since no
    /// class's result can invalidate another's.
    #[hotpath::measure]
    pub fn precompute_ancestor_names(&mut self) {
        let class_names: Vec<Name> = self.classes.keys().cloned().collect();
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

    /// Every interface/trait `class` transitively inherits from -
    /// `extends`/`implements` *and* `uses`, which `is_subclass_of`
    /// doesn't chain (a used trait isn't itself a supertype
    /// `instanceof` recognizes). `class` itself is never included.
    /// Needed by `checker::oop::check_require_tags`: real PHPStan's
    /// `getInterfaces()`/`getTraits(true)` are both transitive the
    /// identical way.
    pub(crate) fn all_ancestor_names(&self, class: &str) -> FxHashSet<Name> {
        let mut seen = FxHashSet::default();
        let mut stack = vec![Name::from(class)];
        let mut result = FxHashSet::default();
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
    fn own_owner_names(
        &self,
        class: &str,
    ) -> (
        FxHashMap<Name, Name>,
        FxHashMap<Name, Name>,
        FxHashMap<Name, Name>,
    ) {
        let mut method_owner = FxHashMap::default();
        let mut property_owner = FxHashMap::default();
        let mut const_owner = FxHashMap::default();
        let mut seen = FxHashSet::default();
        let mut queue = VecDeque::new();
        queue.push_back(Name::from(class));
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
    /// how `lookup_ancestor_method_with_owner`/
    /// `lookup_ancestor_property_with_owner` are specified: for each of
    /// `class`'s direct parents *in order*, try that parent's *entire*
    /// ancestor chain before ever trying the next parent, not a single
    /// BFS merged across all parents at once. Merging each direct
    /// parent's own map in order, first-insert-wins, reproduces that
    /// exact "try parent 0 fully, then parent 1 fully" priority without
    /// re-walking anything.
    fn merge_ancestor_owner_names(
        parents: impl Iterator<Item = Name>,
        own_owner_by_class: &FxHashMap<Name, FxHashMap<Name, Name>>,
    ) -> FxHashMap<Name, Name> {
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
    ///
    /// Two phases, `own` then `ancestor`, since every
    /// `ancestor_*_owner_by_class` entry is built by merging its class's
    /// already-computed direct parents' `own` maps
    /// (`merge_ancestor_owner_names`) - every class's `own` map must
    /// finish before any class's `ancestor` map starts.
    #[hotpath::measure]
    pub fn precompute_member_owners(&mut self) {
        let class_names: Vec<Name> = self.classes.keys().cloned().collect();
        let registry: &Registry = self;

        // Every `self.X = ...`-style assignment below is deferred to
        // the end of this function, once `registry` (a borrow of the
        // *whole* `self`) is no longer needed - each intermediate map is
        // built into a plain local first, since `registry` must outlive
        // every `par_iter()` call below, including the two `ancestor_*`
        // ones that read `method_owner_by_class`/`property_owner_by_class`
        // back as locals, not as `self` fields.
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

        let ancestor_method_owner_by_class: FxHashMap<Name, FxHashMap<Name, Name>> = class_names
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
                    Self::merge_ancestor_owner_names(parents.into_iter(), &method_owner_by_class),
                )
            })
            .collect();

        // Real PHP interfaces can't declare properties at all, so
        // `lookup_ancestor_property_with_owner`'s own walk (unlike the
        // method version) only ever considers `extends`, never
        // `implements` - mirrored here.
        let ancestor_property_owner_by_class: FxHashMap<Name, FxHashMap<Name, Name>> = class_names
            .par_iter()
            .map(|name| {
                let parents = registry
                    .classes
                    .get(name)
                    .map(|info| info.extends.clone())
                    .unwrap_or_default();
                (
                    name.clone(),
                    Self::merge_ancestor_owner_names(parents.into_iter(), &property_owner_by_class),
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
    /// `method` as `owner` declares it (method names resolve
    /// case-insensitively; messages spell them as declared).
    pub(crate) fn declared_method_name(&self, owner: &str, method: &str) -> String {
        self.classes
            .get(owner)
            .and_then(|info| {
                info.methods
                    .get_key_value(method)
                    .or_else(|| {
                        info.methods
                            .iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case(method))
                    })
                    .map(|(k, _)| k.to_string())
            })
            .unwrap_or_else(|| method.to_string())
    }

    pub fn lookup_method(&self, class: &str, method: &str) -> Option<&FunctionReflection> {
        let owners = self.method_owner_by_class.get(class)?;
        if let Some(owner) = owners.get(method) {
            return self.classes.get(owner)?.methods.get(method);
        }
        // Real PHP resolves method calls case-insensitively, the same
        // as class/function names - `method_owner_by_class` is keyed by
        // exact declared spelling (see `find_case_mismatched_ancestor_
        // method`'s own doc comment), so a mismatched-case call needs
        // this same fallback `lookup_class`/`lookup_function` already
        // have, or `$obj->DoStuff()` calling a real `doStuff()` would
        // wrongly read as undefined.
        let (name, owner) = owners
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(method))?;
        self.classes.get(owner)?.methods.get(name.as_ref())
    }

    /// The real declared spelling `lookup_method`/`lookup_method_with_
    /// owner` would resolve `method` to, for comparing against the
    /// as-written call-site text.
    pub(crate) fn lookup_method_canonical_name(&self, class: &str, method: &str) -> Option<&str> {
        let owners = self.method_owner_by_class.get(class)?;
        if let Some((name, _)) = owners.get_key_value(method) {
            return Some(name.as_ref());
        }
        owners
            .keys()
            .find(|name| name.eq_ignore_ascii_case(method))
            .map(|s| s.as_ref())
    }

    /// Every method name callable on `class` (own + inherited), for
    /// building a "did you mean?" suggestion at a `method.notFound` site -
    /// just the already-precomputed `method_owner_by_class` key set, no
    /// extra ancestor walk needed.
    pub(crate) fn all_method_names(&self, class: &str) -> impl Iterator<Item = &str> {
        self.method_owner_by_class
            .get(class)
            .into_iter()
            .flat_map(|m| m.keys().map(|s| s.as_ref()))
    }

    /// `all_method_names`'s own property counterpart, for
    /// `textDocument/completion`'s member-completion candidate list.
    pub(crate) fn all_property_names(&self, class: &str) -> impl Iterator<Item = &str> {
        self.property_owner_by_class
            .get(class)
            .into_iter()
            .flat_map(|m| m.keys().map(|s| s.as_ref()))
    }

    /// `all_method_names`'s own const counterpart (an enum's cases are
    /// folded into this same map - see `ClassReflection::consts`'s own doc
    /// comment).
    pub(crate) fn all_const_names(&self, class: &str) -> impl Iterator<Item = &str> {
        self.const_owner_by_class
            .get(class)
            .into_iter()
            .flat_map(|m| m.keys().map(|s| s.as_ref()))
    }

    /// `method_owner_by_class[class]` is keyed by exact declared
    /// spelling, so two case-insensitively-equal but distinctly-spelled
    /// names both survive as separate keys - reading that back out here
    /// finds a same-name-different-casing override with no extra walk.
    pub(crate) fn find_case_mismatched_ancestor_method(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(Name, Name)> {
        let owners = self.method_owner_by_class.get(class)?;
        owners.iter().find_map(|(name, owner)| {
            (name.eq_ignore_ascii_case(method) && name.as_ref() != method)
                .then(|| (owner.clone(), name.clone()))
        })
    }

    /// Same as `lookup_method`, but also returns the declaring ancestor's
    /// name - needed wherever a caller goes on to do class-level generic
    /// substitution (`checker::class_template_bindings`),
    /// since a `@template` name is scoped to the class that declares it,
    /// not globally unique - substituting an inherited method/property's
    /// `TemplateParam`s correctly requires knowing which ancestor's own
    /// template scope they belong to, not just the value found.
    pub fn lookup_method_with_owner(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(Name, &FunctionReflection)> {
        let owners = self.method_owner_by_class.get(class)?;
        if let Some(owner) = owners.get(method) {
            let sig = self.classes.get(owner)?.methods.get(method)?;
            return Some((owner.clone(), sig));
        }
        // See `lookup_method`'s own identical case-insensitive fallback
        // doc comment.
        let (name, owner) = owners
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(method))?;
        let sig = self.classes.get(owner)?.methods.get(name.as_ref())?;
        Some((owner.clone(), sig))
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
        let owner = self
            .ancestor_method_owner_by_class
            .get(class)?
            .get(method)?;
        let sig = self.classes.get(owner)?.methods.get(method)?;
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
        let info = self.classes.get(class)?;
        // 1. Immediately-implemented interfaces - walks each interface's
        //    *own* ancestor chain too (`lookup_method_with_owner`, not a
        //    bare `.methods.get`), not just its own direct declaration
        //    (real shape: Piwigo's own
        //    `ExtensionInterface extends SubscriberInterface`, where
        //    `subscribedEvents()` is declared only on the *grandparent*
        //    interface).
        for iface in &info.implements {
            if let Some((owner, sig)) = self.lookup_method_with_owner(iface, method) {
                return Some((owner, sig, true));
            }
        }
        // 2. A used trait's own abstract declaration - unconditional,
        //    matching this file's other override-compatibility checks
        //    (real PHPStan's own equivalent gate,
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

    /// `@phpstan-consistent-constructor`'s own ancestor walk,
    /// transcribed from real PHPStan's
    /// `ConsistentConstructorHelper::findConsistentConstructor`: starting
    /// at `class`'s own *parent* (not `class` itself - the real rule's
    /// call site already has the child's own method in hand and is
    /// looking for what it's being held *to*), walks up `extends` until
    /// an ancestor tagged `@phpstan-consistent-constructor` is found,
    /// returning that ancestor's own `__construct` - or `None` if the
    /// chain ends (or cycles - bounded by `self.classes.len()`, same
    /// guard `Registry::flatten_traits`'s cycle protection uses) with no
    /// tagged ancestor at all, meaning no check applies, real
    /// PHP's own default.
    ///
    /// Deliberately doesn't synthesize a zero-parameter dummy constructor
    /// for a tagged ancestor with no `__construct` of its own the way real
    /// PHPStan's `DummyConstructorReflection` does - every `FunctionReflection`
    /// this project returns is borrowed from `self.classes`, and a class
    /// that bothers tagging itself `@phpstan-consistent-constructor` at
    /// all almost always also declares the constructor it's pinning down
    /// (the entire point of the tag), so this narrower scope isn't
    /// expected to leave real value on the table.
    pub(crate) fn find_consistent_constructor_ancestor(
        &self,
        class: &str,
    ) -> Option<(Name, &FunctionReflection)> {
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
        let Some(info) = self.classes.get(class) else {
            return false;
        };
        info.uses.iter().any(|used_trait| {
            self.classes
                .get(used_trait)
                .is_some_and(|trait_info| trait_info.abstract_methods.contains(method))
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
        let Some(info) = self.classes.get(class) else {
            return false;
        };
        let mut seen = FxHashSet::default();
        let mut queue: VecDeque<Name> = info
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

    /// Same idea as `lookup_ancestor_method_with_owner`, but walking
    /// only `extends` - real PHP interfaces can't declare properties, so
    /// there's no `implements` side to check.
    pub fn lookup_ancestor_property_with_owner(
        &self,
        class: &str,
        prop: &str,
    ) -> Option<(Name, &PropertyReflection)> {
        let owner = self
            .ancestor_property_owner_by_class
            .get(class)?
            .get(prop)?;
        let sig = self.classes.get(owner)?.properties.get(prop)?;
        Some((owner.clone(), sig))
    }

    /// Real PHP forbids overriding a method declared `final` anywhere
    /// in the ancestor chain, no matter how many intervening subclasses
    /// sit between the `final` declaration and this one. Walks only
    /// `extends` - an interface's methods have no bodies to protect, and
    /// this project doesn't yet propagate a trait's `final_methods` the
    /// way it does `abstract_methods`. Returns the name of the ancestor
    /// that declared it final, plus whether that was via `@final`
    /// (docblock, PHPStan-only convention, real PHP doesn't enforce it)
    /// rather than the real keyword - checked together, nearest-ancestor
    /// first, real keyword winning over `@final` when a single ancestor
    /// has both (mirrors real PHPStan's own `OverridingMethodRule`
    /// if/elseif order: `method.parentMethodFinal` vs.
    /// `method.parentMethodFinalByPhpDoc`).
    pub fn find_final_ancestor_method(&self, class: &str, method: &str) -> Option<(Name, bool)> {
        let info = self.classes.get(class)?;
        let mut seen = FxHashSet::default();
        let mut stack: Vec<Name> = info.extends.clone();
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

    /// `find_final_ancestor_method`'s property-side
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
    ) -> Option<(Name, bool)> {
        let info = self.classes.get(class)?;
        let mut seen = FxHashSet::default();
        let mut stack: Vec<Name> = info.extends.clone();
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
            let Some(info) = self.classes.get(&c) else {
                continue;
            };
            class_queue.extend(info.extends.iter().cloned());
            let mut iface_queue: VecDeque<Name> = info.implements.iter().cloned().collect();
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
    pub fn required_abstract_methods(&self, class: &str) -> Vec<Name> {
        let mut required = FxHashSet::default();
        let mut seen = FxHashSet::default();
        let mut stack = vec![Name::from(class)];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            if info.kind == ClassKind::Interface {
                // A purely magic `@method` tag (see `FunctionReflection::is_magic`)
                // documents a capability, not a requirement - real PHP
                // has no way to express "this interface method is
                // optional," so an interface's real, textually-declared
                // methods are still all unconditionally required.
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
    pub fn lookup_property(&self, class: &str, prop: &str) -> Option<&PropertyReflection> {
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
    ) -> Option<(Name, &PropertyReflection)> {
        let owner = self.property_owner_by_class.get(class)?.get(prop)?;
        let sig = self.classes.get(owner)?.properties.get(prop)?;
        Some((owner.clone(), sig))
    }

    /// Looks up a class constant, walking the ancestor graph if not
    /// declared directly on `class` - mirrors `lookup_property`.
    pub fn lookup_const(&self, class: &str, const_name: &str) -> Option<&ConstantReflection> {
        let owner = self.const_owner_by_class.get(class)?.get(const_name)?;
        self.classes.get(owner)?.consts.get(const_name)
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
        let owner = self.const_owner_by_class.get(class)?.get(const_name)?;
        let sig = self.classes.get(owner)?.consts.get(const_name)?;
        Some((owner.clone(), sig))
    }

    /// A backed enum case's own literal backing value, if this
    /// checker was able to prove one (see `ClassReflection::enum_case_values`'s
    /// own doc comment) - not hierarchy-walked, since an enum can't
    /// extend anything for a case to inherit from in the first place.
    pub fn enum_case_value(&self, class: &str, case: &str) -> Option<&Type> {
        self.classes.get(class)?.enum_case_values.get(case)
    }

    /// The matching case (if any, and if provable) for a
    /// literal argument passed to a backed enum's own `::from()`/
    /// `::tryFrom()` - a reverse lookup over `enum_case_value` above,
    /// since PHP itself only stores value -> case at runtime, never the
    /// other way around.
    pub fn enum_case_for_value(&self, class: &str, value: &Type) -> Option<&str> {
        let info = self.classes.get(class)?;
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
    pub(crate) fn is_builtin_class(&self, class: &str) -> bool {
        self.built_in_stub_classes.contains(class)
    }

    /// The function `name` names, compared case-insensitively as PHP does.
    pub fn lookup_function(&self, name: &str) -> Option<&FunctionReflection> {
        self.functions.get(name)
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
        self.functions
            .get_with_name(name)
            .map(|(declared, sig)| (declared.as_str(), sig))
    }

    /// `lookup_function`'s class-side mirror - real PHP resolves
    /// class/interface/trait/enum names case-insensitively too. Returns
    /// the *canonical* declared key alongside the info so a caller can
    /// rebind its own local variable to it (a fresh-per-check-time
    /// lookup, not a structural registry field - see
    /// `canonicalize_class_name_case` for why those two need different
    /// treatment).
    pub fn lookup_class(&self, name: &str) -> Option<(&str, &ClassReflection)> {
        self.classes
            .get_with_name(name)
            .map(|(declared, info)| (declared.as_str(), info))
    }

    /// The entity that names `repository_fqn` as its own
    /// `#[ORM\Entity(repositoryClass: ...)]` argument, if any - the
    /// reverse of `ClassReflection::doctrine_entity`'s own `repository_class`
    /// field, needed to resolve `EntityRepository::createQueryBuilder()`'s
    /// implicit `FROM` when checking a repository class's own method
    /// body (ROADMAP Phase 168 168b). A plain linear scan, not a
    /// precomputed map - a real project has at most a few dozen entities,
    /// nowhere near enough for this to be a real cost next to everything
    /// else a single method-body check already does.
    ///
    /// `None` when more than one entity names the same `repository_fqn`
    /// (real, valid Doctrine: one repository *class* can be instantiated
    /// separately per entity via `EntityManager::getRepository()`/DI, so
    /// nothing in `repository_fqn`'s own method bodies can say which
    /// entity a given call site's instance is actually bound to) - a
    /// confirmed real Piwigo shape (`ImageRepository` serves both
    /// `ImageEntity` and `ImageFormatEntity`), not a hypothetical.
    /// Guessing the first match produced a real false positive:
    /// `ImageRepository::findByIds`'s own `$entity->id` flagged as
    /// undefined because the scan happened to land on `ImageFormatEntity`
    /// first.
    pub(crate) fn repository_entity(&self, repository_fqn: &str) -> Option<&str> {
        let mut matches = self.classes.iter().filter_map(|(name, info)| {
            let entity = info.doctrine_entity.as_ref()?;
            (entity.repository_class.as_deref() == Some(repository_fqn)).then_some(name.as_ref())
        });
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
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
        let Some(a_info) = self.classes.get(a) else {
            return true;
        };
        let Some(b_info) = self.classes.get(b) else {
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
