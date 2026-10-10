//! Answers to the hierarchy queries effect resolution asks, each computed
//! once per state of the class graph. A cell is filled by the first thread
//! to ask and read by every other, so the answers do not depend on which
//! thread asks first.

use std::hash::{BuildHasher, Hash};
use std::sync::{Arc, Mutex, OnceLock};

use rustc_hash::{FxBuildHasher, FxHashMap};
use syntax::name::Name;

use super::effect_table::{EffectIx, EffectTable, TransferTable};
use super::effects::{
    BuiltinBase, DeclKey, Holder, HookDeclaration, IdTargets, ImplicitKey, OperationKey, Run,
    Transfer,
};

const SHARDS: usize = 16;

type Shards<K, V> = [Mutex<FxHashMap<K, Arc<OnceLock<V>>>>; SHARDS];

/// A map from a key to a value computed once.
pub(crate) struct OnceMap<K, V> {
    shards: OnceLock<Box<Shards<K, V>>>,
}

impl<K, V> Default for OnceMap<K, V> {
    fn default() -> Self {
        Self {
            shards: OnceLock::new(),
        }
    }
}

impl<K, V> OnceMap<K, V> {
    /// Whether any key has been asked.
    pub(crate) fn is_used(&self) -> bool {
        self.shards.get().is_some()
    }

    /// The shards, made when the map is first used, so a registry that
    /// never asks a memo does not allocate its tables.
    fn shards(&self) -> &Shards<K, V> {
        self.shards
            .get_or_init(|| Box::new(std::array::from_fn(|_| Mutex::new(FxHashMap::default()))))
    }
}

impl<K: Hash + Eq, V: Clone> OnceMap<K, V> {
    /// The value for `key`, from `compute` when no thread has made it. The
    /// shard is not held while `compute` runs, so it may ask for other keys.
    pub(crate) fn get_or_compute(&self, key: K, compute: impl FnOnce() -> V) -> V {
        let shard = FxBuildHasher.hash_one(&key) as usize % SHARDS;
        let cell = {
            let mut map = self.shards()[shard]
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Arc::clone(map.entry(key).or_default())
        };
        cell.get_or_init(compute).clone()
    }

    /// `get_or_compute` for a key asked by reference: it is copied only when
    /// no thread has made its value.
    pub(crate) fn get_or_compute_ref<Q>(&self, key: &Q, compute: impl FnOnce() -> V) -> V
    where
        K: std::borrow::Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K> + ?Sized,
    {
        let shard = FxBuildHasher.hash_one(key) as usize % SHARDS;
        let cell = {
            let mut map = self.shards()[shard]
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match map.get(key) {
                Some(cell) => Arc::clone(cell),
                None => Arc::clone(map.entry(key.to_owned()).or_default()),
            }
        };
        cell.get_or_init(compute).clone()
    }
}

/// What a call's returned holder depends on, when that is no more than the
/// registry and the class the scope belongs to.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum ReturnedKey {
    ThisMethod {
        class: Name,
        method: Name,
    },
    ThisPropertyMethod {
        class: Name,
        property: Name,
        method: Name,
    },
    Static {
        target: StaticTarget,
        method: Name,
        /// The class the scope belongs to, for a target that reads it.
        own: Option<Name>,
    },
    ParentHook {
        property: Name,
        class: Name,
    },
}

/// The class a static call names.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum StaticTarget {
    SelfKeyword,
    Parent,
    StaticKeyword,
    Named(Option<Name>),
}

/// The declarations a call runs, each with its transfer, shared by the
/// calls that ask the same.
pub(crate) type SharedTargets = Arc<[Run]>;

impl ReachMemo {
    /// Replaces the memo with an empty one. A memo that holds the answers
    /// of a pass is freed on a thread of its own, so the next pass does not
    /// wait for its tables to be freed.
    pub(crate) fn reset(&mut self) {
        let old = std::mem::take(self);
        if old.implementations.is_used()
            || old.methods_named.is_used()
            || old.implicit.is_used()
            || old.effect_ixs.is_used()
            || old.builtin_base.is_used()
        {
            std::thread::spawn(move || drop(old));
        }
    }
}

/// The memoised queries, keyed by lowercased names.
#[derive(Default)]
pub(crate) struct ReachMemo {
    /// The classes below a class (with the trait users when the key says),
    /// in the order a walk of the graph finds them.
    pub(crate) descendants: OnceMap<(Name, bool), Arc<[Name]>>,
    pub(crate) implementations: OnceMap<(Name, Name), Arc<[DeclKey]>>,
    pub(crate) every_method: OnceMap<Name, Arc<[DeclKey]>>,
    /// The same three, by handle.
    pub(crate) implementation_ixs: OnceMap<(Name, Name), Arc<[EffectIx]>>,
    pub(crate) every_method_ixs: OnceMap<Name, Arc<[EffectIx]>>,
    pub(crate) methods_named_ixs: OnceMap<Name, Arc<[EffectIx]>>,
    pub(super) builtin_base: OnceMap<(DeclKey, Transfer, Option<Name>), Option<Arc<BuiltinBase>>>,
    pub(crate) wrapper_runs: OnceMap<&'static str, SharedTargets>,
    pub(crate) hook_members: OnceMap<(Option<Name>, bool), Name>,
    pub(crate) overrides_of_builtin: OnceMap<(String, String), Arc<[DeclKey]>>,
    /// What an operation runs, by `ImplicitKey`; none when it may run code
    /// not known.
    /// What an operation runs on an object of any class, but for a
    /// property's.
    pub(crate) unknown_implicit: OnceMap<(OperationKey, Option<Name>, bool), Arc<[EffectIx]>>,
    /// The same with the `$this` each runs with.
    pub(crate) unknown_targets:
        OnceMap<(OperationKey, Option<Name>, bool, Transfer), SharedTargets>,
    pub(crate) implicit: OnceMap<ImplicitKey, Option<SharedTargets>>,
    pub(crate) methods_named: OnceMap<Name, Arc<[DeclKey]>>,
    /// The declaration PHP runs calling a method on an object of exactly a
    /// class, by their lowercase names.
    pub(crate) implicit_method: OnceMap<(Name, Name), Option<DeclKey>>,
    /// The holder a call returns, by what it depends on.
    pub(crate) returned: OnceMap<ReturnedKey, Holder>,
    /// The methods PHP calls for an operation but a property's on an object
    /// of exactly a class.
    pub(crate) implicit_methods: OnceMap<(Name, OperationKey), Arc<[&'static Name]>>,
    /// The table row a builtin declaration is looked up under.
    pub(crate) builtin_entries: OnceMap<DeclKey, Option<Arc<str>>>,
    /// The constructors of every class that has one.
    pub(crate) constructors: OnceLock<Arc<[DeclKey]>>,
    /// The properties of a class with a hook body, by class, property and
    /// hook.
    pub(crate) hook_declarations: OnceMap<(Name, Option<Name>, bool), Arc<[HookDeclaration]>>,
    /// Whether objects of a builtin class run what they were made with.
    pub(crate) stub_holds_code: OnceMap<String, bool>,
    /// Whether a call of a stub, by handle, may run code it was given.
    pub(crate) stub_runs: OnceMap<EffectIx, bool>,
    /// The handle a declaration's key stands for in the effects pass.
    pub(crate) effect_ixs: OnceMap<DeclKey, EffectIx>,
    pub(crate) effects: EffectTable,
    pub(crate) transfers: TransferTable,
    /// Whether a shared set (by address; the cell keeps the set alive) holds
    /// a declaration run with the caller's own `$this`.
    pub(crate) shared_scoped: OnceMap<usize, (SharedTargets, bool)>,
    /// What a shared set runs once its builtins are replaced by what they
    /// run, by address and, for a set that depends on it, the calling
    /// class; none when a builtin may run code not known.
    pub(in crate::registry) shared_runs:
        OnceMap<(usize, Option<String>), (SharedTargets, Option<IdTargets>)>,
}

impl std::fmt::Debug for ReachMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReachMemo")
    }
}

/// A copy of a registry starts with no answers: it may change the graph.
impl Clone for ReachMemo {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_computed_once_and_a_computation_may_ask_for_another_key() {
        let map: OnceMap<u32, u32> = OnceMap::default();
        let calls = std::sync::atomic::AtomicU32::new(0);
        let value = |key: u32| {
            map.get_or_compute(key, || {
                calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if key > 0 {
                    map.get_or_compute(key - 1, || 7) + 1
                } else {
                    0
                }
            })
        };
        assert_eq!(value(3), 8);
        assert_eq!(value(3), 8);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
}
