//! Which classes and functions a run of the finishing passes resolves:
//! all of them, or only those a change can have affected, every other
//! entry being read as already resolved.

use std::sync::{Arc, Mutex};

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::name::Name;

use crate::registry::name_table::FoldedKey;
use crate::registry::{ClassId, FnId, IdSource, Registry};

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq)]
struct Segment {
    source: IdSource,
    generation: u32,
}

trait DeclarationId: Copy {
    fn source(self) -> Option<IdSource>;
    fn local(self) -> u32;
    fn generation(self) -> u32;
    fn from_parts(source: IdSource, local: u32, generation: u32) -> Self;
}

macro_rules! declaration_id_impl {
    ($id:ty) => {
        impl DeclarationId for $id {
            fn source(self) -> Option<IdSource> {
                self.source()
            }
            fn local(self) -> u32 {
                self.local()
            }
            fn generation(self) -> u32 {
                self.generation()
            }
            fn from_parts(source: IdSource, local: u32, generation: u32) -> Self {
                <$id>::from_parts(source, local, generation)
            }
        }
    };
}

declaration_id_impl!(ClassId);
declaration_id_impl!(FnId);

/// A compact set of declaration IDs. Local IDs are dense within an ID
/// source and generation, so each segment is a plain vector of machine words.
#[derive(Debug, Clone, Default)]
struct IdBitSet {
    segments: FxHashMap<Segment, Box<[u64]>>,
}

impl IdBitSet {
    fn from_ids<I, T>(ids: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: DeclarationId,
    {
        let mut words: FxHashMap<Segment, Vec<u64>> = FxHashMap::default();
        for id in ids {
            let Some(source) = id.source() else { continue };
            let segment = Segment {
                source,
                generation: id.generation(),
            };
            let local = id.local() as usize;
            let bucket = words.entry(segment).or_default();
            bucket.resize(bucket.len().max(local / 64 + 1), 0);
            bucket[local / 64] |= 1u64 << (local % 64);
        }
        Self {
            segments: words
                .into_iter()
                .map(|(key, words)| (key, words.into_boxed_slice()))
                .collect(),
        }
    }

    fn contains<T: DeclarationId>(&self, id: T) -> bool {
        let Some(source) = id.source() else {
            return false;
        };
        let Some(words) = self.segments.get(&Segment {
            source,
            generation: id.generation(),
        }) else {
            return false;
        };
        let local = id.local() as usize;
        words
            .get(local / 64)
            .is_some_and(|word| word & (1u64 << (local % 64)) != 0)
    }

    fn ids<T: DeclarationId>(&self) -> Vec<T> {
        let mut ids = Vec::new();
        for (segment, words) in &self.segments {
            for (bucket, word) in words.iter().copied().enumerate() {
                let mut bits = word;
                while bits != 0 {
                    let bit = bits.trailing_zeros() as usize;
                    let local = bucket * 64 + bit;
                    if local <= u32::MAX as usize {
                        ids.push(T::from_parts(
                            segment.source,
                            local as u32,
                            segment.generation,
                        ));
                    }
                    bits &= bits - 1;
                }
            }
        }
        ids
    }

    fn minus(&self, other: &Self) -> Self {
        let mut segments = FxHashMap::default();
        for (segment, words) in &self.segments {
            let theirs = other.segments.get(segment);
            let remaining: Vec<u64> = words
                .iter()
                .enumerate()
                .map(|(i, word)| {
                    word & !theirs.and_then(|words| words.get(i)).copied().unwrap_or(0)
                })
                .collect();
            if remaining.iter().any(|word| *word != 0) {
                segments.insert(*segment, remaining.into_boxed_slice());
            }
        }
        Self { segments }
    }
}

/// The declarations the finishing passes resolve, as ID bitsets; `None`
/// means every declaration. Name keys are retained for removed declarations
/// and direct name-selected compatibility scopes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scope {
    classes: Option<Arc<IdBitSet>>,
    functions: Option<Arc<IdBitSet>>,
    // Names here are compatibility selectors for direct name-based callers
    // and, for ID scopes, declarations removed before their ID can be read.
    class_keys: Option<Arc<FxHashSet<String>>>,
    function_keys: Option<Arc<FxHashSet<String>>>,
    // Which scope: `0` for every class and function, otherwise a number
    // no other scope has, kept by a clone.
    id: u64,
}

/// A number no scope has had.
fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Scope {
    /// Every class and function.
    pub(crate) fn all() -> Scope {
        Scope::default()
    }

    /// Only `classes` and `functions`, in any case.
    pub(crate) fn of<'a>(
        classes: impl IntoIterator<Item = &'a str>,
        functions: impl IntoIterator<Item = &'a str>,
    ) -> Scope {
        Scope {
            classes: Some(Arc::new(IdBitSet::default())),
            functions: Some(Arc::new(IdBitSet::default())),
            class_keys: Some(Arc::new(
                classes.into_iter().map(str::to_ascii_lowercase).collect(),
            )),
            function_keys: Some(Arc::new(
                functions.into_iter().map(str::to_ascii_lowercase).collect(),
            )),
            id: next_id(),
        }
    }

    /// Only `classes` and `functions`, already lowercased as `of` does.
    #[allow(dead_code)] // Kept for name-selected compatibility scopes.
    pub(crate) fn from_keys(classes: FxHashSet<String>, functions: FxHashSet<String>) -> Scope {
        debug_assert!(
            classes
                .iter()
                .chain(&functions)
                .all(|key| !key.bytes().any(|b| b.is_ascii_uppercase())),
            "a scope's keys are lowercased"
        );
        Scope {
            classes: Some(Arc::new(IdBitSet::default())),
            functions: Some(Arc::new(IdBitSet::default())),
            class_keys: Some(Arc::new(classes)),
            function_keys: Some(Arc::new(functions)),
            id: next_id(),
        }
    }

    /// A scope whose active declarations are selected by their runtime IDs.
    /// `removed_*` retains names for declarations deleted before this scope
    /// was built, so stale derived results can still be cleared.
    pub(crate) fn from_ids(
        classes: impl IntoIterator<Item = ClassId>,
        functions: impl IntoIterator<Item = FnId>,
        removed_classes: FxHashSet<String>,
        removed_functions: FxHashSet<String>,
    ) -> Scope {
        Scope {
            classes: Some(Arc::new(IdBitSet::from_ids(classes))),
            functions: Some(Arc::new(IdBitSet::from_ids(functions))),
            class_keys: Some(Arc::new(removed_classes)),
            function_keys: Some(Arc::new(removed_functions)),
            id: next_id(),
        }
    }

    pub(crate) fn is_all(&self) -> bool {
        self.classes.is_none()
            && self.functions.is_none()
            && self.class_keys.is_none()
            && self.function_keys.is_none()
    }

    pub(crate) fn has_class(&self, name: &str) -> bool {
        self.class_keys.as_ref().is_some_and(|classes| {
            crate::registry::with_function_key(name, |key| classes.contains(key))
        }) || (self.classes.is_none() && self.class_keys.is_none())
    }

    pub(crate) fn has_function(&self, name: &str) -> bool {
        self.function_keys.as_ref().is_some_and(|functions| {
            crate::registry::with_function_key(name, |key| functions.contains(key))
        }) || (self.functions.is_none() && self.function_keys.is_none())
    }

    pub(crate) fn has_class_id(&self, id: ClassId) -> bool {
        self.classes
            .as_ref()
            .is_some_and(|classes| classes.contains(id))
            || (self.classes.is_none() && self.class_keys.is_none())
    }

    pub(crate) fn has_function_id(&self, id: FnId) -> bool {
        self.functions
            .as_ref()
            .is_some_and(|functions| functions.contains(id))
            || (self.functions.is_none() && self.function_keys.is_none())
    }

    pub(crate) fn class_ids(&self) -> Option<Vec<ClassId>> {
        self.classes.as_ref().map(|ids| ids.ids())
    }

    pub(crate) fn function_ids(&self) -> Option<Vec<FnId>> {
        self.functions.as_ref().map(|ids| ids.ids())
    }

    pub(crate) fn class_keys(&self) -> Option<&FxHashSet<String>> {
        self.class_keys.as_deref()
    }

    pub(crate) fn function_keys(&self) -> Option<&FxHashSet<String>> {
        self.function_keys.as_deref()
    }

    /// What `self` has that `other` hasn't, for the ID and name selectors.
    pub(crate) fn minus(&self, other: &Scope) -> Scope {
        type Names = Option<Arc<FxHashSet<String>>>;
        let diff = |ours: &Names, theirs: &Names| -> Names {
            ours.as_ref().map(|ours| {
                Arc::new(
                    ours.iter()
                        .filter(|key| !theirs.as_ref().is_none_or(|theirs| theirs.contains(*key)))
                        .cloned()
                        .collect(),
                )
            })
        };
        Scope {
            classes: match (&self.classes, &other.classes) {
                (Some(ours), Some(theirs)) => Some(Arc::new(ours.minus(theirs))),
                (Some(_), None) if other.class_keys.is_none() => {
                    Some(Arc::new(IdBitSet::default()))
                }
                (None, Some(_)) => None,
                (None, None) => None,
                (Some(ours), None) => Some(Arc::clone(ours)),
            },
            functions: match (&self.functions, &other.functions) {
                (Some(ours), Some(theirs)) => Some(Arc::new(ours.minus(theirs))),
                (Some(_), None) if other.function_keys.is_none() => {
                    Some(Arc::new(IdBitSet::default()))
                }
                (None, Some(_)) => None,
                (None, None) => None,
                (Some(ours), None) => Some(Arc::clone(ours)),
            },
            class_keys: diff(&self.class_keys, &other.class_keys),
            function_keys: diff(&self.function_keys, &other.function_keys),
            id: next_id(),
        }
    }
}

/// The names `Registry::scoped_class_names` and `scoped_function_names`
/// made last, with the scope and the table's version they are of.
#[derive(Default)]
pub(crate) struct ScopedNames {
    classes: Mutex<Option<Made>>,
    functions: Mutex<Option<Made>>,
}

type Made = (u64, u64, Arc<[Name]>);

impl ScopedNames {
    fn classes(
        &self,
        scope: &Scope,
        version: u64,
        make: impl FnOnce() -> Vec<Name>,
    ) -> Arc<[Name]> {
        Self::of(&self.classes, scope, version, make)
    }

    fn functions(
        &self,
        scope: &Scope,
        version: u64,
        make: impl FnOnce() -> Vec<Name>,
    ) -> Arc<[Name]> {
        Self::of(&self.functions, scope, version, make)
    }

    fn of(
        slot: &Mutex<Option<Made>>,
        scope: &Scope,
        version: u64,
        make: impl FnOnce() -> Vec<Name>,
    ) -> Arc<[Name]> {
        let mut slot = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((made_for, table, names)) = &*slot
            && *made_for == scope.id
            && *table == version
        {
            return Arc::clone(names);
        }
        let names: Arc<[Name]> = make().into();
        *slot = Some((scope.id, version, Arc::clone(&names)));
        names
    }
}

impl Clone for ScopedNames {
    /// A clone makes its own names, from its own tables.
    fn clone(&self) -> Self {
        ScopedNames::default()
    }
}

impl std::fmt::Debug for ScopedNames {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScopedNames")
    }
}

impl Registry {
    fn scoped_class_ids_vec(&self) -> Vec<ClassId> {
        let mut ids = self.scope.class_ids().unwrap_or_else(|| {
            if self.scope.is_all() {
                self.class_ids.iter().map(|(_, id)| *id).collect()
            } else {
                Vec::new()
            }
        });
        if let Some(keys) = self.scope.class_keys() {
            ids.extend(keys.iter().filter_map(|key| {
                self.class_ids
                    .get_with_name(&FoldedKey(key))
                    .map(|(_, id)| *id)
            }));
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn scoped_function_ids_vec(&self) -> Vec<FnId> {
        let mut ids = self.scope.function_ids().unwrap_or_else(|| {
            if self.scope.is_all() {
                self.function_ids.iter().map(|(_, id)| *id).collect()
            } else {
                Vec::new()
            }
        });
        if let Some(keys) = self.scope.function_keys() {
            ids.extend(keys.iter().filter_map(|key| {
                self.function_ids
                    .get_with_name(&FoldedKey(key))
                    .map(|(_, id)| *id)
            }));
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The declared names of the classes in the passes' scope. Made once
    /// for as long as the scope and the class names stay as they are.
    pub(crate) fn scoped_class_names(&self) -> Arc<[Name]> {
        self.scoped_names
            .classes(&self.scope, self.class_ids.version(), || {
                self.scoped_class_ids_vec()
                    .into_iter()
                    .filter_map(|id| self.class_name_by_id(id).cloned())
                    .collect()
            })
    }

    /// The declared names of the functions in the passes' scope, made
    /// once like the classes'.
    pub(crate) fn scoped_function_names(&self) -> Arc<[Name]> {
        self.scoped_names
            .functions(&self.scope, self.function_ids.version(), || {
                self.scoped_function_ids_vec()
                    .into_iter()
                    .filter_map(|id| self.function_name_by_id(id).cloned())
                    .collect()
            })
    }

    /// The classes in the passes' scope, with their declared names.
    pub(crate) fn scoped_classes(&self) -> Vec<(&Name, &crate::registry::ClassReflection)> {
        self.scoped_class_ids_vec()
            .into_iter()
            .filter_map(|id| {
                let name = self.class_name_by_id(id)?;
                self.class_by_id(id).map(|class| (name, class))
            })
            .collect()
    }

    /// The functions in the passes' scope, with their declared names.
    pub(crate) fn scoped_functions(&self) -> Vec<(&Name, &crate::registry::FunctionReflection)> {
        self.scoped_function_ids_vec()
            .into_iter()
            .filter_map(|id| {
                let name = self.function_name_by_id(id)?;
                self.function_declaration_by_id(id)
                    .map(|function| (name, function))
            })
            .collect()
    }

    /// The scope's class names no class is registered under any more,
    /// whose derived entries are to go.
    pub(crate) fn scoped_removed_class_keys(&self) -> Vec<String> {
        self.scope
            .class_keys()
            .into_iter()
            .flatten()
            .filter(|key| !self.class_ids.contains_key(&FoldedKey(key)))
            .cloned()
            .collect()
    }

    /// Reads the declarations of the names in `scope`, in parallel. A table
    /// read back from the cache decodes an entry the first time it is read,
    /// and the passes would otherwise read a scope's thousands of entries
    /// one after another.
    pub(crate) fn warm(&self, scope: &Scope) {
        use rayon::prelude::*;
        let classes = scope.class_keys().into_iter().flatten().collect::<Vec<_>>();
        classes
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|key| {
                self.class_ids.get_with_name(&FoldedKey(key));
            });
        let functions = scope
            .function_keys()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        functions
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|key| {
                self.function_ids.get_with_name(&FoldedKey(key));
            });
        scope
            .class_ids()
            .unwrap_or_default()
            .into_par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|id| {
                self.class_by_id(id);
            });
        scope
            .function_ids()
            .unwrap_or_default()
            .into_par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|id| {
                self.function_declaration_by_id(id);
            });
    }

    /// Forgets the purity of the functions and methods in `scope`, for
    /// the purity passes to settle again; their declarations are as
    /// resolved otherwise.
    pub(crate) fn clear_purity(&mut self, scope: &Scope) {
        self.clear_purity_results(scope);
    }

    /// Sets which classes and functions the next finishing passes resolve.
    pub(crate) fn set_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }
}

/// A stable representation of the selected IDs and compatibility names.
impl syntax::canonical::Canonical for Scope {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let class_ids: Option<Vec<(u8, u32, u32)>> = self.classes.as_ref().map(|ids| {
            let mut ids: Vec<_> = ids
                .ids::<ClassId>()
                .into_iter()
                .map(|id| {
                    (
                        match id.source() {
                            Some(IdSource::Stub) => 0,
                            Some(IdSource::Context) => 1,
                            Some(IdSource::Project) => 2,
                            None => 3,
                        },
                        id.generation(),
                        id.local(),
                    )
                })
                .collect();
            ids.sort_unstable();
            ids
        });
        let function_ids: Option<Vec<(u8, u32, u32)>> = self.functions.as_ref().map(|ids| {
            let mut ids: Vec<_> = ids
                .ids::<FnId>()
                .into_iter()
                .map(|id| {
                    (
                        match id.source() {
                            Some(IdSource::Stub) => 0,
                            Some(IdSource::Context) => 1,
                            Some(IdSource::Project) => 2,
                            None => 3,
                        },
                        id.generation(),
                        id.local(),
                    )
                })
                .collect();
            ids.sort_unstable();
            ids
        });
        p.node(
            "Scope",
            &[
                ("class_ids", &class_ids),
                ("classes", &self.class_keys),
                ("function_ids", &function_ids),
                ("functions", &self.function_keys),
            ],
        );
    }
}
