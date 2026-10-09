//! Answers to the hierarchy queries effect resolution asks, each computed
//! once per state of the class graph. A cell is filled by the first thread
//! to ask and read by every other, so the answers do not depend on which
//! thread asks first.

use std::hash::{BuildHasher, Hash};
use std::sync::{Arc, Mutex, OnceLock};

use rustc_hash::{FxBuildHasher, FxHashMap};
use syntax::name::Name;

use super::effects::{DeclKey, ImplicitKey, Transfer};

const SHARDS: usize = 16;

/// A map from a key to a value computed once.
pub(crate) struct OnceMap<K, V> {
    shards: [Mutex<FxHashMap<K, Arc<OnceLock<V>>>>; SHARDS],
}

impl<K, V> Default for OnceMap<K, V> {
    fn default() -> Self {
        Self {
            shards: std::array::from_fn(|_| Mutex::new(FxHashMap::default())),
        }
    }
}

impl<K: Hash + Eq, V: Clone> OnceMap<K, V> {
    /// The value for `key`, from `compute` when no thread has made it. The
    /// shard is not held while `compute` runs, so it may ask for other keys.
    pub(crate) fn get_or_compute(&self, key: K, compute: impl FnOnce() -> V) -> V {
        let shard = FxBuildHasher.hash_one(&key) as usize % SHARDS;
        let cell = {
            let mut map = self.shards[shard]
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Arc::clone(map.entry(key).or_default())
        };
        cell.get_or_init(compute).clone()
    }
}

/// The declarations a call runs, each with its transfer, shared by the
/// calls that ask the same.
pub(crate) type SharedTargets = Arc<[(DeclKey, Transfer)]>;

/// The memoised queries, keyed by lowercased names.
#[derive(Default)]
pub(crate) struct ReachMemo {
    /// The classes below a class (with the trait users when the key says),
    /// in the order a walk of the graph finds them.
    pub(crate) descendants: OnceMap<(String, bool), Arc<[Name]>>,
    pub(crate) implementations: OnceMap<(String, String), Arc<[DeclKey]>>,
    pub(crate) every_method: OnceMap<String, Arc<[DeclKey]>>,
    pub(crate) overrides_of_builtin: OnceMap<(String, String), Arc<[DeclKey]>>,
    /// What an operation runs, by `ImplicitKey`; none when it may run code
    /// not known.
    pub(crate) implicit: OnceMap<ImplicitKey, Option<SharedTargets>>,
    pub(crate) methods_named: OnceMap<String, Arc<[DeclKey]>>,
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
