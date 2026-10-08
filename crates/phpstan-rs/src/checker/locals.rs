//! A checked scope's variables and the facts recorded about them.

use super::keys::KeyTableRef;
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// A checked scope's variable-name-to-inferred-type bindings, plus an
/// opt-in, parallel declaration-span side-channel the LSP's local-variable-
/// navigation feature reads through `Checker::probe_span`/
/// `probe_var_provenance` - reaching-definitions info computed by riding
/// along the checker's own flow-sensitive traversal (branch merging via
/// `merge_scopes`, loop convergence via `converge_loop_body`, termination-
/// pruning via `Completion::terminates`) rather than a second, independent
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
#[derive(Clone)]
pub(crate) struct Locals {
    // The table of the keys this scope and every scope forked from it use.
    keys: super::keys::KeyTableRef,
    // The class this scope's code belongs to, which `self::$p` names.
    class: Option<syntax::name::Name>,
    types: LocalsMap,
    // The keys of `types` that are not a real variable - a property
    // chain or an index (`KeyTable::key_of`). Only such a key can depend on
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
    exposed: ExposedVariableSet,
    condition_aliases: ConditionAliasMap,
    branch_implied_facts: BranchImpliedFactsMap,
    query_builder_aliases: QueryBuilderAliasMap,
    container_field_facts: ContainerFieldFactsMap,
    discriminant_array_facts: DiscriminantArrayFactsMap,
    // Monotonic: set `true` the first time any of `insert`/`declare`/
    // `insert_with_provenance` (the only three places `types` is ever
    // written) inserts a key that is not a variable - a property chain, an
    // index, a static property - and never reset back to `false` except by
    // starting a fresh scope. Lets `check_expr`/`infer_expr`'s own "is this
    // exact property/index/static-property chain narrowed?" probes
    // (`PropertyAccess`/`Index`/`ClassConstFetch`/`StaticPropertyFetch`)
    // skip looking the key up in the common case where `locals` holds no
    // such entry at all. A stale `true` after the one narrowed entry is
    // later purged just costs one skippable lookup, not a false negative:
    // `merge_scopes`'s own `types`-merging loop (`checker/control_flow.rs`)
    // routes every surviving key through `insert_with_provenance`, so this
    // needs no separate per-field merge line to stay correct across branch
    // joins.
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

#[cfg(test)]
impl Default for Locals {
    /// A scope with a table of its own.
    fn default() -> Locals {
        Locals::new(KeyTableRef::default(), None)
    }
}

impl Locals {
    /// An empty scope whose keys `keys` makes: one table serves every scope
    /// of a checker, so a key from one is a key in the others.
    pub(crate) fn new(keys: KeyTableRef, class: Option<&str>) -> Locals {
        Locals {
            keys,
            class: class.map(syntax::name::Name::from),
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
            exposed: ExposedVariableSet::default(),
            condition_aliases: ConditionAliasMap::default(),
            branch_implied_facts: BranchImpliedFactsMap::default(),
            query_builder_aliases: QueryBuilderAliasMap::default(),
            container_field_facts: ContainerFieldFactsMap::default(),
            discriminant_array_facts: DiscriminantArrayFactsMap::default(),
            may_have_member_narrowing: false,
            any_variable_may_exist: false,
            track: false,
            version: 0,
        }
    }

    /// The table the keys of this scope are made by.
    pub(crate) fn keys(&self) -> &super::keys::KeyTableRef {
        &self.keys
    }

    /// The key of the variable `name`, made when it has not been.
    pub(crate) fn var_key(&self, name: &str) -> LocalKey {
        self.keys.borrow_mut().var(name)
    }

    /// The key of the variable `name` when it has been made.
    pub(crate) fn find_var(&self, name: &str) -> Option<LocalKey> {
        self.keys.borrow().find_var(name)
    }

    /// The key of the expression `e` narrows, made when it has not been
    /// (`KeyTable::key_of`).
    pub(crate) fn key_of(&self, arenas: &NodeArenas, e: &ExprNode) -> Option<LocalKey> {
        self.keys
            .borrow_mut()
            .key_of(arenas, e, self.class.as_ref())
    }

    /// `key_of`, when the key has been made: a read never makes one.
    pub(crate) fn find_key(&self, arenas: &NodeArenas, e: &ExprNode) -> Option<LocalKey> {
        self.keys.borrow().find_key(arenas, e, self.class.as_ref())
    }

    /// The static property `prop` of the class `name` refers to, from this
    /// scope: what `KeyTable::key_of` makes of `name::$prop`.
    pub(crate) fn static_prop_data(
        &self,
        name: &syntax::ast::QualifiedName,
        prop: &syntax::name::Name,
    ) -> KeyData {
        KeyData::StaticProp {
            class: super::keys::static_class(name, self.class.as_ref()),
            prop: prop.clone(),
        }
    }

    /// The key of `data`, made when it has not been.
    pub(crate) fn intern(&self, data: KeyData) -> LocalKey {
        self.keys.borrow_mut().intern(data)
    }

    /// The key of `data` when it has been made.
    pub(crate) fn find(&self, data: &KeyData) -> Option<LocalKey> {
        self.keys.borrow().find(data)
    }

    /// What `key` is.
    pub(crate) fn data(&self, key: LocalKey) -> KeyData {
        self.keys.borrow().data(key).clone()
    }

    /// `key` as text: `a`, `a->b`, `a[k]`, `a[$k]`, `Foo::$p`.
    pub(crate) fn render(&self, key: LocalKey) -> String {
        self.keys.borrow().render(key)
    }

    /// Whether `key` is a variable, not a property, an index or a test.
    pub(crate) fn is_real_variable(&self, key: LocalKey) -> bool {
        self.keys.borrow().is_real_variable(key)
    }

    pub(crate) fn get(&self, key: LocalKey) -> Option<&Type> {
        self.types.get(&key)
    }

    /// The type of the variable `name`.
    pub(crate) fn get_var(&self, name: &str) -> Option<&Type> {
        self.types.get(&self.find_var(name)?)
    }

    pub(crate) fn contains_key(&self, key: LocalKey) -> bool {
        self.types.contains_key(&key)
    }

    /// Whether the variable `name` has a type.
    pub(crate) fn contains_var(&self, name: &str) -> bool {
        self.find_var(name)
            .is_some_and(|key| self.types.contains_key(&key))
    }

    /// See `may_have_member_narrowing`'s own doc comment.
    pub(crate) fn may_have_member_narrowing(&self) -> bool {
        self.may_have_member_narrowing
    }

    /// Whether this scope tracks where each variable was declared (the
    /// LSP's probe) - ordinary checking does not.
    pub(crate) fn tracks_provenance(&self) -> bool {
        self.track
    }

    /// Whether `key` is held and is not a variable.
    pub(crate) fn is_synthetic(&self, key: LocalKey) -> bool {
        self.synthetic.contains(&key)
    }

    /// The keys that are not a real variable.
    pub(crate) fn synthetic_keys(&self) -> impl Iterator<Item = &LocalKey> {
        self.synthetic.iter()
    }

    /// Whether rebinding `key` would change anything: a key of the scope
    /// depends on it (`KeyTable::depends_on`, which `key` itself
    /// satisfies), or a per-field, branch-implied or per-discriminant fact
    /// is held. When it would not, `purge_stale_narrowing_for` does nothing
    /// for any key.
    pub(crate) fn has_state_to_forget(&self) -> bool {
        !self.synthetic.is_empty()
            || !self.container_field_facts.is_empty()
            || !self.branch_implied_facts.is_empty()
            || !self.discriminant_array_facts.is_empty()
    }

    /// Provenance-preserving update, for narrowing an existing binding
    /// (`instanceof`/null/array-shape checks) or a by-ref call's type
    /// widening (`sort($arr)`, `preg_match(..., $matches)`) - the value's
    /// declaration site doesn't move just because its known type changed.
    pub(crate) fn insert(&mut self, key: LocalKey, ty: Type) -> Option<Type> {
        self.touch();
        self.note_synthetic(key);
        self.types.insert(key, ty)
    }

    /// `insert` for the variable `name`.
    pub(crate) fn insert_var(&mut self, name: &str, ty: Type) -> Option<Type> {
        let key = self.var_key(name);
        self.insert(key, ty)
    }

    /// Records `key` as synthetic when it is not a variable.
    fn note_synthetic(&mut self, key: LocalKey) {
        if !self.is_real_variable(key) {
            self.may_have_member_narrowing = true;
            self.synthetic.insert(key);
        }
    }

    /// `insert_with_provenance` for the variable `name`.
    pub(crate) fn insert_with_provenance_var(
        &mut self,
        name: &str,
        ty: Type,
        provenance: ProvenanceSet,
    ) -> Option<Type> {
        let key = self.var_key(name);
        self.insert_with_provenance(key, ty, provenance)
    }

    /// `native` of the variable `name`.
    pub(crate) fn native_var(&self, name: &str) -> Option<&Type> {
        self.native.get(&self.find_var(name)?)
    }

    /// `set_native` for the variable `name`.
    pub(crate) fn set_native_var(&mut self, name: &str, ty: Type) {
        let key = self.var_key(name);
        self.set_native(key, ty);
    }

    /// `clear_native` for the variable `name`.
    pub(crate) fn clear_native_var(&mut self, name: &str) {
        if let Some(key) = self.find_var(name) {
            self.clear_native(key);
        }
    }

    /// `get_provenance` of the variable `name`.
    pub(crate) fn get_provenance_var(&self, name: &str) -> Option<&ProvenanceSet> {
        self.provenance.get(&self.find_var(name)?)
    }

    /// The one real new-declaration primitive - used only at a genuine
    /// syntactic binding site (assignment LHS, parameter, `foreach`/`catch`
    /// binding, `static $x`, closure `use (...)`, `$x++`/`$x--`). Replaces
    /// `key`'s provenance with a fresh singleton `{span}`, since a rebind
    /// makes anything tracked before it a dead candidate.
    pub(crate) fn declare(&mut self, key: LocalKey, ty: Type, span: Span) -> Option<Type> {
        self.touch();
        self.native.remove(&key);
        if self.track {
            let mut set = ProvenanceSet::default();
            set.insert(span);
            self.provenance.insert(key, set);
        }
        self.purge_condition_aliases_targeting(key);
        self.purge_branch_implied_facts_depending_on(key);
        self.query_builder_aliases.remove(&key);
        // A container rebound wholesale invalidates every per-field fact
        // recorded about its old value - see `ContainerFieldFactsMap`'s
        // own doc comment. This covers "reassigned wholesale" for every
        // real rebind site (assignment LHS, `foreach` binding, ...); an
        // already-defined container passed by-ref to a function goes
        // through `insert`, not `declare` (see `predefine_byref_or_
        // unknown_args`), so that call site purges this map itself too.
        self.container_field_facts.remove(&key);
        self.purge_member_facts_about(key);
        self.note_synthetic(key);
        self.types.insert(key, ty)
    }

    /// Forgets what is recorded about `key` as a receiver or a
    /// discriminant: the members a `method_exists`/`property_exists` test
    /// proved it has, and the per-branch array facts recorded under it as
    /// the subject (or the container) of a test. Once it holds another
    /// value, none of that is known.
    pub(crate) fn purge_member_facts_about(&mut self, key: LocalKey) {
        self.touch();
        self.guarded_methods
            .retain(|(receiver, _)| *receiver != key);
        self.guarded_properties
            .retain(|(receiver, _)| *receiver != key);
        self.discriminant_array_facts
            .retain(|(container, subject, _), _| *container != key && *subject != key);
    }

    /// `declare` for the variable `name`.
    pub(crate) fn declare_var(&mut self, name: &str, ty: Type, span: Span) -> Option<Type> {
        let key = self.var_key(name);
        self.declare(key, ty, span)
    }

    /// The store half of the condition-alias mechanism - see
    /// `ConditionAliasMap`'s doc comment. `target_keys` are `condition`'s
    /// own narrowing targets (e.g. `[x]` for `$x instanceof Foo`, or
    /// `[isUserFound, email]` for a compound condition naming both),
    /// computed by the caller (`Checker::record_condition_alias`) via
    /// `condition_alias_targets`.
    pub(crate) fn set_condition_alias(
        &mut self,
        name: LocalKey,
        target_keys: Vec<LocalKey>,
        condition: ExprId,
    ) {
        self.touch();
        self.condition_aliases
            .insert(name, (target_keys, condition));
    }

    /// The store half of `QueryBuilderAliasMap`'s own mechanism - see its
    /// doc comment.
    pub(crate) fn set_query_builder_alias(&mut self, name: LocalKey, rhs: ExprId) {
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
    fn purge_condition_aliases_targeting(&mut self, key: LocalKey) {
        self.touch();
        self.condition_aliases.remove(&key);
        self.condition_aliases
            .retain(|_, (targets, _)| !targets.contains(&key));
    }

    /// Drops every branch-implied fact that no longer holds once `key`
    /// (a variable or a key built on one) gets a new value - see
    /// `BranchImpliedFactsMap`.
    pub(crate) fn purge_branch_implied_facts_depending_on(&mut self, key: LocalKey) {
        self.touch();
        if self.branch_implied_facts.is_empty() {
            return;
        }
        let mut changed: Vec<(Discriminant, Option<BranchImpliedFacts>)> = Vec::new();
        {
            let keys = self.keys.borrow();
            let depends = |k: LocalKey| keys.depends_on(k, key);
            for (fact_key, implied) in self.branch_implied_facts.iter() {
                // A truthiness or `isset` subject depends on the key it
                // tests.
                if depends(fact_key.subject) {
                    changed.push((fact_key.clone(), None));
                    continue;
                }
                if implied.iter().any(|(var, _)| depends(*var)) {
                    let kept: Vec<(LocalKey, Type)> = implied
                        .iter()
                        .filter(|(var, _)| !depends(*var))
                        .cloned()
                        .collect();
                    changed.push((
                        fact_key.clone(),
                        (!kept.is_empty()).then(|| std::rc::Rc::new(kept)),
                    ));
                }
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
        key: LocalKey,
        ty: Type,
        provenance: ProvenanceSet,
    ) -> Option<Type> {
        self.touch();
        if self.track && !provenance.is_empty() {
            self.provenance.insert(key, provenance);
        }
        self.note_synthetic(key);
        self.types.insert(key, ty)
    }

    pub(crate) fn get_provenance(&self, key: LocalKey) -> Option<&ProvenanceSet> {
        self.provenance.get(&key)
    }

    pub(crate) fn remove(&mut self, key: LocalKey) -> Option<Type> {
        self.touch();
        self.provenance.remove(&key);
        self.native.remove(&key);
        if self.may_have_member_narrowing {
            self.synthetic.remove(&key);
        }
        self.types.remove(&key)
    }

    /// `remove` for a scope that tracks no provenance, leaving the native
    /// type of the key as it is.
    pub(crate) fn remove_type(&mut self, key: LocalKey) -> Option<Type> {
        debug_assert!(!self.track);
        self.touch();
        if self.may_have_member_narrowing {
            self.synthetic.remove(&key);
        }
        self.types.remove(&key)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (LocalKey, &Type)> {
        self.types.iter().map(|(key, ty)| (*key, ty))
    }

    /// The variables of the scope, by name, with their types.
    pub(crate) fn variables(&self) -> Vec<(syntax::name::Name, &Type)> {
        let keys = self.keys.borrow();
        self.types
            .iter()
            .filter_map(|(key, ty)| match keys.data(*key) {
                KeyData::Var(name) => Some((name.clone(), ty)),
                _ => None,
            })
            .collect()
    }

    /// The bindings that differ between this scope and `other`, in key
    /// order. Subtrees the two share are skipped, so what it visits is
    /// about what changed, not what is held.
    pub(crate) fn types_diff<'a>(
        &'a self,
        other: &'a Locals,
    ) -> imbl::ordmap::DiffIter<'a, 'a, LocalKey, Type, imbl::shared_ptr::DefaultSharedPtr> {
        self.types.diff(&other.types)
    }

    /// Records `key`'s native type, right after the `declare` binding it.
    /// `mixed` records nothing.
    pub(crate) fn set_native(&mut self, key: LocalKey, ty: Type) {
        self.touch();
        if !ty.is_mixed() {
            self.native.insert(key, ty);
        }
    }

    pub(crate) fn clear_native(&mut self, key: LocalKey) {
        self.touch();
        self.native.remove(&key);
    }

    pub(crate) fn native(&self, key: LocalKey) -> Option<&Type> {
        self.native.get(&key)
    }

    pub(crate) fn natives(&self) -> impl Iterator<Item = (LocalKey, &Type)> {
        self.native.iter().map(|(key, ty)| (*key, ty))
    }

    /// A fresh, empty scope that still carries `self`'s own `track` flag
    /// forward - see `track`'s own doc comment for why a closure/arrow-fn
    /// literal's own scope needs this instead of a plain `Locals::
    /// default()`.
    pub(crate) fn fresh_scope(&self) -> Locals {
        Locals {
            keys: std::rc::Rc::clone(&self.keys),
            class: self.class.clone(),
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
            exposed: ExposedVariableSet::default(),
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
        self.types.ptr_eq(&other.types) || self.types_diff(other).next().is_none()
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

    pub(crate) fn exposed(&self) -> &ExposedVariableSet {
        &self.exposed
    }

    pub(crate) fn exposed_mut(&mut self) -> &mut ExposedVariableSet {
        self.touch();
        &mut self.exposed
    }

    /// From here on, a call may write through a reference to `name` or to
    /// one of its elements.
    pub(crate) fn expose(&mut self, name: &syntax::name::Name) {
        if !self.exposed.contains(name) {
            self.touch();
            self.exposed.insert(name.clone());
        }
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
