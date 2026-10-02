//! A checked scope's variables and the facts recorded about them.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// A checked scope's variable-name-to-inferred-type bindings, plus an
/// opt-in, parallel declaration-span side-channel the LSP's local-variable-
/// navigation feature reads through `Checker::probe_span`/
/// `probe_var_provenance` - reaching-definitions info computed by riding
/// along the checker's own flow-sensitive traversal (branch merging via
/// `merge_scopes`, loop convergence via `converge_loop_body`, termination-
/// pruning via `always_terminates`) rather than a second, independent
/// approximation. `types`/`provenance` are separate fields, not one field
/// carrying `(Type, ProvenanceSet)` pairs, since `Type` is pattern-matched
/// at hundreds of call sites throughout this module and a wrapper tuple
/// would mean unwrapping it back out everywhere for a concern (LSP
/// navigation) unrelated to type inference. Kept in lockstep with `types`
/// via `Locals`'s own API: `insert` leaves `provenance` untouched (correct
/// for narrowing, which refines an existing binding without creating a new
/// one); `declare` is the one real new-declaration primitive;
/// `insert_with_provenance` copies an already-known provenance set across
/// when a value moves between two `Locals` unchanged (e.g. `merge_scopes`'s
/// union).
///
/// `provenance`, like `types`, is backed by `imbl::HashMap` so cloning a
/// `Locals` stays two O(1) pointer bumps rather than reintroducing a deep
/// clone.
#[derive(Clone, Default)]
pub(crate) struct Locals {
    types: LocalsMap,
    // The keys of `types` that are not a real variable's name - a property
    // chain or an index (`narrowing_key`). Only such a key can depend on
    // another (`a->b` on `a`, `m[$k]` on `k`), so what a rebound key
    // invalidates is among these few, not among every local. Kept by every
    // method that adds or removes a key.
    synthetic: KeySet,
    // Each variable's native type - what PHP itself guarantees, ignoring
    // PHPDoc - where known: a parameter's native hint, or the native type
    // of the expression last assigned to it. Absent means unknown
    // (`mixed`). Cleared by every rebind (`declare`); narrowing leaves it,
    // which only keeps it wider than PHPStan's own native scope.
    native: LocalsMap,
    provenance: ProvenanceMap,
    guarded_functions: GuardedFunctionSet,
    // Global constant names a `defined('NAME')` test proved defined.
    guarded_constants: GuardedFunctionSet,
    // Lowercased class names a `class_exists('X')` test (or its interface,
    // trait and enum siblings) proved declared.
    guarded_classes: GuardedFunctionSet,
    guarded_methods: GuardedMemberSet,
    guarded_properties: GuardedMemberSet,
    clone_reinitialized: ReinitializedPropertySet,
    condition_aliases: ConditionAliasMap,
    branch_implied_facts: BranchImpliedFactsMap,
    query_builder_aliases: QueryBuilderAliasMap,
    container_field_facts: ContainerFieldFactsMap,
    discriminant_array_facts: DiscriminantArrayFactsMap,
    // Monotonic: set `true` the first time any of `insert`/`declare`/
    // `insert_with_provenance` (the only three places `types` is ever
    // written - see each one's own check below) inserts one of
    // `narrowing_key`'s own synthetic property-chain/array-index keys,
    // never reset back to `false` except by starting a fresh scope. Lets
    // `check_expr`/`infer_expr`'s own "is this exact property/index/
    // static-property chain narrowed?" probes (`PropertyAccess`/`Index`/
    // `ClassConstFetch`/`StaticPropertyFetch`) skip building the synthetic lookup key entirely -
    // two `String` allocations (`narrowing_key` itself, then the call
    // site's own `format!`) - in the overwhelmingly common case where
    // `locals` holds no such entry at all, without needing every one of
    // `narrowing_key`'s ~30 call sites across `checker/narrowing*.rs`/
    // `checker/assignment.rs` to individually remember to set a flag (a
    // single missed site there would silently reintroduce a correctness
    // bug - a narrowed value never read back - which is worse than the
    // wasted allocation this exists to avoid). A stale `true` after the
    // one narrowed entry is later purged just costs one skippable
    // allocation, not a false negative - `merge_scopes`'s own `types`-
    // merging loop (`checker/control_flow.rs`) already routes every
    // surviving key through `insert_with_provenance`, so this needs no
    // separate per-field merge line to stay correct across branch joins.
    may_have_member_narrowing: bool,
    // Set once an `include`/`require`/`extract()`/`eval()` has run on
    // this path - any variable name may now be defined, with any value
    // (real PHPStan's `afterExtractCall`). Flow-sensitive: a branch
    // merge keeps it if either side has it.
    any_variable_may_exist: bool,
    // Whether `declare`/`insert_with_provenance` do anything - `false` by
    // default for every ordinary check-project run, `true` only for the
    // LSP's own per-request variable-navigation probe. Carried on `Locals`
    // itself, not read from `Checker`, because `Locals` values are
    // constructed in more places than `Checker` methods reach - notably a
    // closure/arrow-fn literal's fresh scope, which needs the enclosing
    // scope's `track` value threaded through explicitly (`fresh_scope`
    // below) rather than defaulting to `false` and losing provenance inside
    // a closure body.
    track: bool,
    // Which content this scope has: a fresh number from `NEXT_VERSION` on
    // every write, kept by a clone, `0` for an untouched default. Two
    // scopes with the same version hold the same facts, so an expression's
    // inferred type can be remembered per version.
    version: u64,
}

static NEXT_VERSION: AtomicU64 = AtomicU64::new(1);

impl Locals {
    pub(crate) fn get(&self, key: &str) -> Option<&Type> {
        self.types.get(key)
    }

    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.types.contains_key(key)
    }

    /// See `may_have_member_narrowing`'s own doc comment.
    pub(crate) fn may_have_member_narrowing(&self) -> bool {
        self.may_have_member_narrowing
    }

    /// The keys that are not a real variable's name.
    pub(crate) fn synthetic_keys(&self) -> impl Iterator<Item = &String> {
        self.synthetic.iter()
    }

    /// Whether rebinding `key` would change anything: a key of the scope
    /// depends on it (`narrowing::key_depends_on`, which `key` itself
    /// satisfies), or a per-field, branch-implied or per-discriminant fact
    /// is held. When it would not, `purge_stale_narrowing_for` does nothing
    /// for any key.
    pub(crate) fn has_state_to_forget(&self) -> bool {
        !self.synthetic.is_empty()
            || !self.container_field_facts.is_empty()
            || !self.branch_implied_facts.is_empty()
            || !self.discriminant_array_facts.is_empty()
    }

    /// Whether the scope has `key`, or a key that depends on it:
    /// `narrowing::key_depends_on` for each key of the scope, found among
    /// the synthetic ones, the only ones that can.
    pub(crate) fn has_key_depending_on(&self, key: &str) -> bool {
        self.types.contains_key(key)
            || self
                .synthetic
                .iter()
                .any(|k| narrowing::key_depends_on(k, key))
    }

    /// Provenance-preserving update, for narrowing an existing binding
    /// (`instanceof`/null/array-shape checks) or a by-ref call's type
    /// widening (`sort($arr)`, `preg_match(..., $matches)`) - the value's
    /// declaration site doesn't move just because its known type changed.
    pub(crate) fn insert(&mut self, key: String, ty: Type) -> Option<Type> {
        self.touch();
        if !narrowing::is_real_variable_key(&key) {
            self.may_have_member_narrowing = true;
            self.synthetic.insert(key.clone());
        }
        self.types.insert(key, ty)
    }

    /// The one real new-declaration primitive - used only at a genuine
    /// syntactic binding site (assignment LHS, parameter, `foreach`/`catch`
    /// binding, `static $x`, closure `use (...)`, `$x++`/`$x--`). Replaces
    /// `key`'s provenance with a fresh singleton `{span}`, since a rebind
    /// makes anything tracked before it a dead candidate.
    pub(crate) fn declare(&mut self, key: String, ty: Type, span: Span) -> Option<Type> {
        self.touch();
        self.native.remove(&key);
        if self.track {
            let mut set = ProvenanceSet::default();
            set.insert(span);
            self.provenance.insert(key.clone(), set);
        }
        self.purge_condition_aliases_targeting(&key);
        self.purge_branch_implied_facts_depending_on(&key);
        self.query_builder_aliases.remove(&key);
        // A container rebound wholesale invalidates every per-field fact
        // recorded about its old value - see `ContainerFieldFactsMap`'s
        // own doc comment. This covers "reassigned wholesale" for every
        // real rebind site (assignment LHS, `foreach` binding, ...); an
        // already-defined container passed by-ref to a function goes
        // through `insert`, not `declare` (see `predefine_byref_or_
        // unknown_args`), so that call site purges this map itself too.
        self.container_field_facts.remove(&key);
        if !narrowing::is_real_variable_key(&key) {
            self.may_have_member_narrowing = true;
            self.synthetic.insert(key.clone());
        }
        self.types.insert(key, ty)
    }

    /// The store half of the condition-alias mechanism - see
    /// `ConditionAliasMap`'s doc comment. `target_keys` are `condition`'s
    /// own narrowing targets (e.g. `["x"]` for `$x instanceof Foo`, or
    /// `["isUserFound", "email"]` for a compound condition naming both),
    /// computed by the caller (`Checker::record_condition_alias`) via
    /// `condition_alias_targets`.
    pub(crate) fn set_condition_alias(
        &mut self,
        name: String,
        target_keys: Vec<String>,
        condition: std::rc::Rc<Expr>,
    ) {
        self.touch();
        self.condition_aliases
            .insert(name, (target_keys, condition));
    }

    /// The store half of `QueryBuilderAliasMap`'s own mechanism - see its
    /// doc comment.
    pub(crate) fn set_query_builder_alias(&mut self, name: String, rhs: std::rc::Rc<Expr>) {
        self.touch();
        self.query_builder_aliases.insert(name, rhs);
    }

    /// Drops `key`'s own alias (rebinding the alias variable makes it
    /// stale) and every other alias whose own `target_keys` contains `key`
    /// (rebinding any one of an alias's targets makes the whole alias
    /// stale too, even though its own name is untouched). `Checker::
    /// record_condition_alias` re-inserts a fresh alias for `key` right
    /// after, when the new RHS qualifies, so a genuine re-alias isn't lost
    /// - only ever a stale one.
    fn purge_condition_aliases_targeting(&mut self, key: &str) {
        self.touch();
        self.condition_aliases.remove(key);
        self.condition_aliases
            .retain(|_, (targets, _)| !targets.iter().any(|t| t == key));
    }

    /// Drops every branch-implied fact that no longer holds once `key`
    /// (a variable or `narrowing_key` chain) gets a new value - see
    /// `BranchImpliedFactsMap`.
    pub(crate) fn purge_branch_implied_facts_depending_on(&mut self, key: &str) {
        self.touch();
        if self.branch_implied_facts.is_empty() {
            return;
        }
        let depends = |k: &str| narrowing::key_depends_on(k, key);
        let mut changed: Vec<(Discriminant, Option<BranchImpliedFacts>)> = Vec::new();
        // A truthiness or `isset` subject wraps the key it tests.
        let tested = |subject: &str| -> String {
            ["truthy(", "isset("]
                .iter()
                .find_map(|prefix| subject.strip_prefix(prefix)?.strip_suffix(')'))
                .unwrap_or(subject)
                .to_string()
        };
        for (fact_key, implied) in self.branch_implied_facts.iter() {
            if depends(&tested(&fact_key.subject)) {
                changed.push((fact_key.clone(), None));
                continue;
            }
            if implied.iter().any(|(var, _)| depends(var)) {
                let kept: Vec<(String, Type)> = implied
                    .iter()
                    .filter(|(var, _)| !depends(var))
                    .cloned()
                    .collect();
                changed.push((
                    fact_key.clone(),
                    (!kept.is_empty()).then(|| std::rc::Rc::new(kept)),
                ));
            }
        }
        for (fact_key, replacement) in changed {
            match replacement {
                Some(kept) => {
                    self.branch_implied_facts.insert(fact_key, kept);
                }
                None => {
                    self.branch_implied_facts.remove(&fact_key);
                }
            }
        }
    }

    /// The provenance-*copying* primitive - used where a value moves
    /// between two different `Locals` values essentially unchanged (a
    /// branch merge, a closure's `use (...)` capture copied in from the
    /// enclosing scope, a `&&`/`||` short-circuit's own new-definition
    /// propagation): `provenance` carries over whatever was already
    /// tracked for it, rather than either fabricating a fresh span (not a
    /// new declaration) or silently discarding it (it has a real,
    /// already-known origin).
    pub(crate) fn insert_with_provenance(
        &mut self,
        key: String,
        ty: Type,
        provenance: ProvenanceSet,
    ) -> Option<Type> {
        self.touch();
        if self.track && !provenance.is_empty() {
            self.provenance.insert(key.clone(), provenance);
        }
        if !narrowing::is_real_variable_key(&key) {
            self.may_have_member_narrowing = true;
            self.synthetic.insert(key.clone());
        }
        self.types.insert(key, ty)
    }

    pub(crate) fn get_provenance(&self, key: &str) -> Option<&ProvenanceSet> {
        self.provenance.get(key)
    }

    pub(crate) fn remove(&mut self, key: &str) -> Option<Type> {
        self.touch();
        self.provenance.remove(key);
        self.native.remove(key);
        if self.may_have_member_narrowing {
            self.synthetic.remove(key);
        }
        self.types.remove(key)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Type)> {
        self.types.iter()
    }

    /// Records `key`'s native type, right after the `declare` binding it.
    /// `mixed` records nothing.
    pub(crate) fn set_native(&mut self, key: &str, ty: Type) {
        self.touch();
        if !ty.is_mixed() {
            self.native.insert(key.to_string(), ty);
        }
    }

    pub(crate) fn clear_native(&mut self, key: &str) {
        self.touch();
        self.native.remove(key);
    }

    pub(crate) fn native(&self, key: &str) -> Option<&Type> {
        self.native.get(key)
    }

    pub(crate) fn natives(&self) -> impl Iterator<Item = (&String, &Type)> {
        self.native.iter()
    }

    /// A fresh, empty scope that still carries `self`'s own `track` flag
    /// forward - see `track`'s own doc comment for why a closure/arrow-fn
    /// literal's own scope needs this instead of a plain `Locals::
    /// default()`.
    pub(crate) fn fresh_scope(&self) -> Locals {
        Locals {
            types: LocalsMap::default(),
            synthetic: KeySet::default(),
            native: LocalsMap::default(),
            provenance: ProvenanceMap::default(),
            guarded_functions: GuardedFunctionSet::default(),
            guarded_constants: GuardedFunctionSet::default(),
            guarded_classes: GuardedFunctionSet::default(),
            guarded_methods: GuardedMemberSet::default(),
            guarded_properties: GuardedMemberSet::default(),
            clone_reinitialized: ReinitializedPropertySet::default(),
            condition_aliases: ConditionAliasMap::default(),
            branch_implied_facts: BranchImpliedFactsMap::default(),
            query_builder_aliases: QueryBuilderAliasMap::default(),
            container_field_facts: ContainerFieldFactsMap::default(),
            discriminant_array_facts: DiscriminantArrayFactsMap::default(),
            may_have_member_narrowing: false,
            any_variable_may_exist: false,
            track: self.track,
            version: NEXT_VERSION.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// A closure body's starting scope: `fresh_scope` plus the functions,
    /// constants and classes proved to exist, which still exist when it
    /// runs.
    pub(crate) fn closure_scope(&self) -> Locals {
        let mut scope = self.fresh_scope();
        scope.guarded_functions = self.guarded_functions.clone();
        scope.guarded_constants = self.guarded_constants.clone();
        scope.guarded_classes = self.guarded_classes.clone();
        scope.touch();
        scope
    }

    /// Turns declaration-span tracking on for this scope and everything
    /// cloned from it - called once, at `check_function`'s entry point,
    /// gated on `Checker::probe_span` being active.
    pub(crate) fn set_track(&mut self, track: bool) {
        self.touch();
        self.track = track;
    }
}

impl PartialEq for Locals {
    /// Compares `types` only - `Checker::converge_loop_body`'s fixed-point
    /// check must converge on type stability alone. `provenance`,
    /// `guarded_functions`/`guarded_methods`/`guarded_properties`, and
    /// `condition_aliases` are bookkeeping side-channels riding along
    /// `types`'s convergence, never inputs to it; on the rare pass where
    /// they haven't independently stabilized, whatever the converging pass
    /// left is used as a bounded approximation, still capped at
    /// `MAX_PASSES`.
    fn eq(&self, other: &Self) -> bool {
        self.types == other.types
    }
}

impl Locals {
    /// Marks this scope's content as changed.
    fn touch(&mut self) {
        self.version = NEXT_VERSION.fetch_add(1, Ordering::Relaxed);
    }

    /// This scope's content version - see the `version` field.
    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    pub(crate) fn guarded_functions(&self) -> &GuardedFunctionSet {
        &self.guarded_functions
    }

    pub(crate) fn guarded_functions_mut(&mut self) -> &mut GuardedFunctionSet {
        self.touch();
        &mut self.guarded_functions
    }

    pub(crate) fn guarded_constants(&self) -> &GuardedFunctionSet {
        &self.guarded_constants
    }

    pub(crate) fn guarded_constants_mut(&mut self) -> &mut GuardedFunctionSet {
        self.touch();
        &mut self.guarded_constants
    }

    pub(crate) fn guarded_classes(&self) -> &GuardedFunctionSet {
        &self.guarded_classes
    }

    pub(crate) fn guarded_classes_mut(&mut self) -> &mut GuardedFunctionSet {
        self.touch();
        &mut self.guarded_classes
    }

    pub(crate) fn guarded_methods(&self) -> &GuardedMemberSet {
        &self.guarded_methods
    }

    pub(crate) fn guarded_methods_mut(&mut self) -> &mut GuardedMemberSet {
        self.touch();
        &mut self.guarded_methods
    }

    pub(crate) fn guarded_properties(&self) -> &GuardedMemberSet {
        &self.guarded_properties
    }

    pub(crate) fn guarded_properties_mut(&mut self) -> &mut GuardedMemberSet {
        self.touch();
        &mut self.guarded_properties
    }

    pub(crate) fn clone_reinitialized(&self) -> &ReinitializedPropertySet {
        &self.clone_reinitialized
    }

    pub(crate) fn clone_reinitialized_mut(&mut self) -> &mut ReinitializedPropertySet {
        self.touch();
        &mut self.clone_reinitialized
    }

    pub(crate) fn condition_aliases(&self) -> &ConditionAliasMap {
        &self.condition_aliases
    }

    pub(crate) fn condition_aliases_mut(&mut self) -> &mut ConditionAliasMap {
        self.touch();
        &mut self.condition_aliases
    }

    pub(crate) fn branch_implied_facts(&self) -> &BranchImpliedFactsMap {
        &self.branch_implied_facts
    }

    pub(crate) fn branch_implied_facts_mut(&mut self) -> &mut BranchImpliedFactsMap {
        self.touch();
        &mut self.branch_implied_facts
    }

    pub(crate) fn query_builder_aliases(&self) -> &QueryBuilderAliasMap {
        &self.query_builder_aliases
    }

    pub(crate) fn query_builder_aliases_mut(&mut self) -> &mut QueryBuilderAliasMap {
        self.touch();
        &mut self.query_builder_aliases
    }

    pub(crate) fn container_field_facts(&self) -> &ContainerFieldFactsMap {
        &self.container_field_facts
    }

    pub(crate) fn container_field_facts_mut(&mut self) -> &mut ContainerFieldFactsMap {
        self.touch();
        &mut self.container_field_facts
    }

    pub(crate) fn discriminant_array_facts(&self) -> &DiscriminantArrayFactsMap {
        &self.discriminant_array_facts
    }

    pub(crate) fn discriminant_array_facts_mut(&mut self) -> &mut DiscriminantArrayFactsMap {
        self.touch();
        &mut self.discriminant_array_facts
    }

    pub(crate) fn any_variable_may_exist(&self) -> bool {
        self.any_variable_may_exist
    }

    pub(crate) fn set_any_variable_may_exist(&mut self, value: bool) {
        self.touch();
        self.any_variable_may_exist = value;
    }
}
