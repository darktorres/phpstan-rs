//! A checked scope's variables and the facts recorded about them.

use super::keys::KeyTableRef;
use super::*;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Every value a function body writes to each of its variables, shared by
/// the scopes of that body (a clone of a `Locals` shares it), so a by-ref
/// capture (`use (&$x)`) can see each value `$x` may hold when the closure
/// runs, not only the value at its creation.
#[derive(Default, Clone)]
pub(crate) struct WriteLog {
    names: rustc_hash::FxHashMap<syntax::name::Name, Vec<Type>>,
    // Whether a write whose name is not known happened (`havoc_variables`):
    // then any variable may have been written.
    any_name: bool,
}

impl WriteLog {
    fn record(&mut self, name: syntax::name::Name, ty: &Type) {
        let types = self.names.entry(name).or_default();
        if !types.contains(ty) {
            types.push(ty.clone());
        }
    }

    fn record_any_name(&mut self) {
        self.any_name = true;
    }

    /// Adds every write `other` holds to this log.
    pub(crate) fn absorb(&mut self, other: &WriteLog) {
        for (name, types) in &other.names {
            for ty in types {
                self.record(name.clone(), ty);
            }
        }
        self.any_name |= other.any_name;
    }

    /// The distinct types written to `name`.
    pub(crate) fn types_of(&self, name: &str) -> &[Type] {
        self.names
            .get(&syntax::name::Name::from(name))
            .map_or(&[], |types| types.as_slice())
    }

    /// Whether a write whose name is not known happened.
    pub(crate) fn any_name_written(&self) -> bool {
        self.any_name
    }

    /// Whether `self` and `other` hold the same writes: the same names with
    /// the same types, and both or neither with an unknown-name write.
    pub(crate) fn same_writes(&self, other: &WriteLog) -> bool {
        self.any_name == other.any_name
            && self.names.len() == other.names.len()
            && self.names.iter().all(|(name, types)| {
                other.names.get(name).is_some_and(|theirs| {
                    types.len() == theirs.len() && types.iter().all(|ty| theirs.contains(ty))
                })
            })
    }
}

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
    // The variables that some path to here leaves undefined and another
    // defines. Such a variable keeps the type its defining paths give, and a
    // read of it reports `variable.possiblyUndefined`. Cleared by every write
    // and by `unset`, which leaves the variable certainly undefined.
    maybe_undefined: KeySet,
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
    // The writes to this function body's variables, when a by-ref capture
    // of one needs them (`enable_write_log`). Shared by every clone, so a
    // write in any branch, merge or loop pass of the body lands in it.
    write_log: Option<Rc<RefCell<WriteLog>>>,
    // The pairs of variables that share one value by reference (`$g = &$x`),
    // each ordered and closed under transitivity: a write to either name of a
    // pair is a write to both (`insert`, `declare`), and a name a rebinding
    // leaves the pairs it was in (`detach_reference`).
    references: BTreeSet<(LocalKey, LocalKey)>,
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
            maybe_undefined: KeySet::default(),
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
            write_log: None,
            references: BTreeSet::new(),
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
    pub(crate) fn key_of(&self, e: &Expr) -> Option<LocalKey> {
        self.keys.borrow_mut().key_of(e, self.class.as_ref())
    }

    /// `key_of`, when the key has been made: a read never makes one.
    pub(crate) fn find_key(&self, e: &Expr) -> Option<LocalKey> {
        self.keys.borrow().find_key(e, self.class.as_ref())
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

    /// Whether the variable `name` is defined on every path to here.
    pub(crate) fn is_certainly_defined_var(&self, name: &str) -> bool {
        self.contains_var(name) && !self.is_maybe_undefined_var(name)
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

    /// Whether the scope has `key`, or a key that depends on it: found
    /// among the synthetic ones, the only ones that can.
    pub(crate) fn has_key_depending_on(&self, key: LocalKey) -> bool {
        self.types.contains_key(&key) || {
            let keys = self.keys.borrow();
            self.synthetic.iter().any(|&k| keys.depends_on(k, key))
        }
    }

    /// Starts logging the writes to this scope's variables, into a log
    /// every scope cloned from it shares, and returns that log. An existing
    /// log is kept, not replaced.
    pub(crate) fn enable_write_log(&mut self) -> Rc<RefCell<WriteLog>> {
        self.write_log.get_or_insert_with(Rc::default).clone()
    }

    /// Makes this scope log its writes to the log `other` logs to, so a
    /// scope made from `other` (a merge, see `merge_scopes`) keeps logging
    /// into the body's own log.
    pub(crate) fn share_write_log(&mut self, other: &Locals) {
        self.write_log = other.write_log.clone();
    }

    /// Gives this scope a version of its own, so an expression type
    /// remembered for the previous content is not reused after a change
    /// that is not visible in `types`, such as a wider write log.
    pub(crate) fn renew_version(&mut self) {
        self.touch();
    }

    /// Logs that the variable `name` may hold any value, for a write whose
    /// value is not tracked (a reference binding, see `ExprKind::AssignRef`).
    pub(crate) fn log_mixed_write_var(&self, name: &str) {
        let key = self.var_key(name);
        self.log_write(key, &Type::mixed());
    }

    /// Logs a write this scope cannot attribute to a variable, such as a
    /// global that a magic `__get()` writes: a by-reference capture of any
    /// variable may then hold any value.
    pub(crate) fn log_untracked_write(&self) {
        if let Some(log) = &self.write_log {
            log.borrow_mut().record_any_name();
        }
    }

    /// The variables that share a value with `key` by reference, other than
    /// `key` itself.
    fn reference_partners(&self, key: LocalKey) -> Vec<LocalKey> {
        self.references
            .iter()
            .filter_map(|&(a, b)| match (a == key, b == key) {
                (true, _) => Some(b),
                (_, true) => Some(a),
                _ => None,
            })
            .collect()
    }

    /// Gives every variable sharing a value with `key` the value `ty` just
    /// written to `key`.
    fn share_write(&mut self, key: LocalKey, ty: &Type) {
        for partner in self.reference_partners(key) {
            self.types.insert(partner, ty.clone());
            self.maybe_undefined.remove(&partner);
            self.native.remove(&partner);
        }
    }

    /// Drops every reference pair naming `key`. The other names it shared a
    /// value with still share it: the transitive closure keeps their pairs.
    fn detach_reference(&mut self, key: LocalKey) {
        if self.references.iter().any(|&(a, b)| a == key || b == key) {
            self.touch();
            self.references.retain(|&(a, b)| a != key && b != key);
        }
    }

    /// Unbinds the variable `name` from every variable it shares a value
    /// with, for a rebinding that is not a write through the reference
    /// (`global`, `static`, a by-reference `foreach`).
    pub(crate) fn detach_reference_var(&mut self, name: &str) {
        if let Some(key) = self.find_var(name) {
            self.detach_reference(key);
        }
    }

    /// Makes `name` share a value with `target` (`$name = &$target;`): `name`
    /// takes `target`'s value, and each later write to either reaches the
    /// other. The caller has already bound `name` to a value of its own, which
    /// this replaces.
    pub(crate) fn bind_reference_var(&mut self, name: &str, target: &str) {
        let key = self.var_key(name);
        let target_key = self.var_key(target);
        if key == target_key {
            return;
        }
        self.detach_reference(key);
        let mut group = self.reference_partners(target_key);
        group.push(target_key);
        self.touch();
        for member in group {
            self.references.insert(ordered_pair(key, member));
        }
        if let Some(ty) = self.types.get(&target_key).cloned() {
            self.insert(key, ty);
        }
    }

    /// Replaces this scope's reference pairs with those both `then` and
    /// `else_` hold, as a branch join does: a pair only one path makes leaves
    /// its names' value unknown after the join. Returns those names, which the
    /// caller must widen.
    pub(crate) fn join_references(&mut self, then: &Locals, else_: &Locals) -> Vec<LocalKey> {
        let shared: BTreeSet<(LocalKey, LocalKey)> = then
            .references
            .intersection(&else_.references)
            .copied()
            .collect();
        let mut unmatched: Vec<LocalKey> = then
            .references
            .symmetric_difference(&else_.references)
            .flat_map(|&(a, b)| [a, b])
            .collect();
        unmatched.sort_unstable();
        unmatched.dedup();
        self.touch();
        self.references = shared;
        unmatched
    }

    /// Logs the write of `ty` to `key` when it is a variable and the log
    /// is enabled.
    fn log_write(&self, key: LocalKey, ty: &Type) {
        let Some(log) = &self.write_log else {
            return;
        };
        // A write to an element or a property changes its variable too, to
        // a value this log does not hold, so the variable is logged as
        // mixed: the element's own type is not the variable's.
        let mixed = Type::mixed();
        let mut root = key;
        loop {
            match self.data(root) {
                KeyData::Var(name) => {
                    let written = if root == key { ty } else { &mixed };
                    log.borrow_mut().record(name, written);
                    for partner in self.reference_partners(root) {
                        if let KeyData::Var(partner_name) = self.data(partner) {
                            log.borrow_mut().record(partner_name, written);
                        }
                    }
                    return;
                }
                KeyData::Prop { base, .. }
                | KeyData::Index { base, .. }
                | KeyData::VarIndex { base, .. } => root = base,
                _ => return,
            }
        }
    }

    /// Provenance-preserving update, for narrowing an existing binding
    /// (`instanceof`/null/array-shape checks) or a by-ref call's type
    /// widening (`sort($arr)`, `preg_match(..., $matches)`) - the value's
    /// declaration site doesn't move just because its known type changed.
    pub(crate) fn insert(&mut self, key: LocalKey, ty: Type) -> Option<Type> {
        self.touch();
        self.log_write(key, &ty);
        self.maybe_undefined.remove(&key);
        self.note_synthetic(key);
        self.share_write(key, &ty);
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
        self.log_write(key, &ty);
        self.maybe_undefined.remove(&key);
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
        self.share_write(key, &ty);
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
        condition: std::rc::Rc<Expr>,
    ) {
        self.touch();
        self.condition_aliases
            .insert(name, (target_keys, condition));
    }

    /// The store half of `QueryBuilderAliasMap`'s own mechanism - see its
    /// doc comment.
    pub(crate) fn set_query_builder_alias(&mut self, name: LocalKey, rhs: std::rc::Rc<Expr>) {
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
        self.log_write(key, &ty);
        if self.track && !provenance.is_empty() {
            self.provenance.insert(key, provenance);
        }
        self.note_synthetic(key);
        self.share_write(key, &ty);
        self.types.insert(key, ty)
    }

    pub(crate) fn get_provenance(&self, key: LocalKey) -> Option<&ProvenanceSet> {
        self.provenance.get(&key)
    }

    /// Whether the variable `key` is defined on some path to here and not on
    /// every one.
    pub(crate) fn is_maybe_undefined(&self, key: LocalKey) -> bool {
        self.maybe_undefined.contains(&key)
    }

    /// The variables that are defined on some path to here and not on every
    /// one.
    pub(crate) fn maybe_undefined_keys(&self) -> impl Iterator<Item = &LocalKey> {
        self.maybe_undefined.iter()
    }

    /// `is_maybe_undefined` for the variable `name`.
    pub(crate) fn is_maybe_undefined_var(&self, name: &str) -> bool {
        self.find_var(name)
            .is_some_and(|key| self.maybe_undefined.contains(&key))
    }

    /// Marks the variable `key` as defined on some paths only, when it holds a
    /// type here.
    pub(crate) fn mark_maybe_undefined(&mut self, key: LocalKey) {
        if self.types.contains_key(&key) {
            self.touch();
            self.maybe_undefined.insert(key);
        }
    }

    /// Marks the variable `key` defined on every path to here, when the code
    /// that assigns it is known to have run.
    pub(crate) fn clear_maybe_undefined(&mut self, key: LocalKey) {
        if self.maybe_undefined.contains(&key) {
            self.touch();
            self.maybe_undefined.remove(&key);
        }
    }

    /// Narrows `key` to `ty` on a branch. A variable undefined on the way in
    /// reads as `null` to a narrowing, so only a `ty` that rules `null` out
    /// defines it: otherwise it stays as it was, absent or maybe-defined. A
    /// non-variable is inserted as is.
    pub(crate) fn narrow_to(&mut self, key: LocalKey, ty: Type) {
        if !self.is_real_variable(key) {
            self.insert(key, ty);
            return;
        }
        let excludes_null = ty.union_members().iter().all(|m| m.is_null().no());
        if !self.types.contains_key(&key) && !excludes_null {
            return;
        }
        let stays_maybe = !excludes_null && self.maybe_undefined.contains(&key);
        self.insert(key, ty);
        if stays_maybe {
            self.mark_maybe_undefined(key);
        }
    }

    pub(crate) fn remove(&mut self, key: LocalKey) -> Option<Type> {
        self.touch();
        self.log_write(key, &Type::null());
        self.detach_reference(key);
        self.maybe_undefined.remove(&key);
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
        self.log_write(key, &Type::null());
        self.detach_reference(key);
        self.maybe_undefined.remove(&key);
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
            maybe_undefined: KeySet::default(),
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
            write_log: None,
            references: BTreeSet::new(),
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

/// `a` and `b` as a pair with its smaller key first.
fn ordered_pair(a: LocalKey, b: LocalKey) -> (LocalKey, LocalKey) {
    if a <= b { (a, b) } else { (b, a) }
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
    /// Whether a fixed point is reached from `previous`: the same types,
    /// and no data-flow node, which type equality leaves out, that
    /// `previous` lacked. A loop whose types settle while a source still
    /// spreads through it runs again.
    pub(crate) fn settled(&self, previous: &Locals) -> bool {
        self == previous
            && (self.types.ptr_eq(&previous.types)
                || self.types.iter().all(|(key, ty)| {
                    !ty.has_flow()
                        || previous
                            .types
                            .get(key)
                            .is_none_or(|p| p.deep_flow().contains_all(&ty.deep_flow()))
                }))
    }

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

    /// Records a write to a variable whose name is not known statically
    /// (`$$name = ...`, an `extract`, an `include`, an `eval`): any variable
    /// may now hold any value, and any name may now exist.
    pub(crate) fn havoc_variables(&mut self) {
        if let Some(log) = &self.write_log {
            log.borrow_mut().record_any_name();
        }
        let keys: Vec<LocalKey> = self
            .iter()
            .filter(|&(key, _)| self.is_real_variable(key))
            .map(|(key, _)| key)
            .collect();
        for key in keys {
            self.insert(key, Type::mixed());
        }
        self.set_any_variable_may_exist(true);
    }
}
