//! Which classes and functions a run of the finishing passes resolves:
//! all of them, or only those a change can have affected, every other
//! entry being read as already resolved.

use std::sync::{Arc, Mutex};

use rustc_hash::FxHashSet;
use syntax::name::Name;

use crate::registry::Registry;
use crate::registry::name_table::FoldedKey;

/// The classes and functions the finishing passes resolve, by lowercased
/// name; `None` for every one.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scope {
    classes: Option<Arc<FxHashSet<String>>>,
    functions: Option<Arc<FxHashSet<String>>>,
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
            classes: Some(Arc::new(
                classes.into_iter().map(str::to_ascii_lowercase).collect(),
            )),
            functions: Some(Arc::new(
                functions.into_iter().map(str::to_ascii_lowercase).collect(),
            )),
            id: next_id(),
        }
    }

    /// Only `classes` and `functions`, already lowercased as `of` does.
    pub(crate) fn from_keys(classes: FxHashSet<String>, functions: FxHashSet<String>) -> Scope {
        debug_assert!(
            classes
                .iter()
                .chain(&functions)
                .all(|key| !key.bytes().any(|b| b.is_ascii_uppercase())),
            "a scope's keys are lowercased"
        );
        Scope {
            classes: Some(Arc::new(classes)),
            functions: Some(Arc::new(functions)),
            id: next_id(),
        }
    }

    pub(crate) fn is_all(&self) -> bool {
        self.classes.is_none() && self.functions.is_none()
    }

    pub(crate) fn has_class(&self, name: &str) -> bool {
        self.classes.as_ref().is_none_or(|classes| {
            crate::registry::with_function_key(name, |key| classes.contains(key))
        })
    }

    pub(crate) fn has_function(&self, name: &str) -> bool {
        self.functions.as_ref().is_none_or(|functions| {
            crate::registry::with_function_key(name, |key| functions.contains(key))
        })
    }

    /// What `self` has that `other` hasn't; both are named sets.
    pub(crate) fn minus(&self, other: &Scope) -> Scope {
        type Keys = Option<Arc<FxHashSet<String>>>;
        let diff = |ours: &Keys, theirs: &Keys| -> Keys {
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
            classes: diff(&self.classes, &other.classes),
            functions: diff(&self.functions, &other.functions),
            id: next_id(),
        }
    }

    /// The lowercased class names, when not every class.
    pub(crate) fn class_keys(&self) -> Option<&FxHashSet<String>> {
        self.classes.as_deref()
    }

    /// `class_keys`, shared: the same set, in the same order.
    pub(crate) fn shared_class_keys(&self) -> Option<Arc<FxHashSet<String>>> {
        self.classes.clone()
    }

    /// The lowercased function names, when not every function.
    pub(crate) fn function_keys(&self) -> Option<&FxHashSet<String>> {
        self.functions.as_deref()
    }

    /// `function_keys`, shared: the same set, in the same order.
    pub(crate) fn shared_function_keys(&self) -> Option<Arc<FxHashSet<String>>> {
        self.functions.clone()
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
    /// The declared names of the classes in the passes' scope. Made once
    /// for as long as the scope and the class names stay as they are.
    pub(crate) fn scoped_class_names(&self) -> Arc<[Name]> {
        self.scoped_names
            .classes(&self.scope, self.class_ids.version(), || {
                match self.scope.class_keys() {
                    None => self.class_ids.keys().cloned().collect(),
                    Some(keys) => keys
                        .iter()
                        .filter_map(|key| {
                            self.class_ids
                                .get_with_name(&FoldedKey(key))
                                .map(|(name, _)| name.clone())
                        })
                        .collect(),
                }
            })
    }

    /// The declared names of the functions in the passes' scope, made
    /// once like the classes'.
    pub(crate) fn scoped_function_names(&self) -> Arc<[Name]> {
        self.scoped_names
            .functions(&self.scope, self.function_ids.version(), || {
                match self.scope.function_keys() {
                    None => self.function_ids.keys().cloned().collect(),
                    Some(keys) => keys
                        .iter()
                        .filter_map(|key| {
                            self.function_ids
                                .get_with_name(&FoldedKey(key))
                                .map(|(name, _)| name.clone())
                        })
                        .collect(),
                }
            })
    }

    /// The classes in the passes' scope, with their declared names.
    pub(crate) fn scoped_classes(&self) -> Vec<(&Name, &crate::registry::ClassReflection)> {
        match self.scope.class_keys() {
            None => self.iter_classes().collect(),
            Some(keys) => keys
                .iter()
                .filter_map(|key| {
                    self.class_ids
                        .get_with_name(&FoldedKey(key))
                        .and_then(|(name, id)| self.class_by_id(*id).map(|class| (name, class)))
                })
                .collect(),
        }
    }

    /// The functions in the passes' scope, with their declared names.
    pub(crate) fn scoped_functions(&self) -> Vec<(&Name, &crate::registry::FunctionReflection)> {
        match self.scope.function_keys() {
            None => self.iter_functions().collect(),
            Some(keys) => keys
                .iter()
                .filter_map(|key| {
                    self.function_ids
                        .get_with_name(&FoldedKey(key))
                        .and_then(|(name, id)| {
                            self.function_declaration_by_id(*id)
                                .map(|function| (name, function))
                        })
                })
                .collect(),
        }
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
        let classes: Vec<&String> = scope.class_keys().into_iter().flatten().collect();
        classes
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|key| {
                self.class_ids.get_with_name(&FoldedKey(key));
            });
        let functions: Vec<&String> = scope.function_keys().into_iter().flatten().collect();
        functions
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|key| {
                self.function_ids.get_with_name(&FoldedKey(key));
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

/// The names in scope; not the scope's number, which tells one apart
/// within a run.
impl syntax::canonical::Canonical for Scope {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let Scope {
            classes,
            functions,
            id: _,
        } = self;
        p.node("Scope", &[("classes", classes), ("functions", functions)]);
    }
}
