//! Which classes and functions a run of the finishing passes resolves:
//! all of them, or only those a change can have affected, every other
//! entry being read as already resolved.

use rustc_hash::FxHashSet;
use syntax::name::Name;

use crate::registry::Registry;

/// The classes and functions the finishing passes resolve, by lowercased
/// name; `None` for every one.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scope {
    classes: Option<FxHashSet<String>>,
    functions: Option<FxHashSet<String>>,
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
            classes: Some(classes.into_iter().map(str::to_ascii_lowercase).collect()),
            functions: Some(functions.into_iter().map(str::to_ascii_lowercase).collect()),
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
            classes: Some(classes),
            functions: Some(functions),
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
        let diff = |ours: &Option<FxHashSet<String>>, theirs: &Option<FxHashSet<String>>| {
            ours.as_ref().map(|ours| {
                ours.iter()
                    .filter(|key| !theirs.as_ref().is_none_or(|theirs| theirs.contains(*key)))
                    .cloned()
                    .collect()
            })
        };
        Scope {
            classes: diff(&self.classes, &other.classes),
            functions: diff(&self.functions, &other.functions),
        }
    }

    /// The lowercased class names, when not every class.
    pub(crate) fn class_keys(&self) -> Option<&FxHashSet<String>> {
        self.classes.as_ref()
    }

    /// The lowercased function names, when not every function.
    pub(crate) fn function_keys(&self) -> Option<&FxHashSet<String>> {
        self.functions.as_ref()
    }
}

impl Registry {
    /// The declared names of the classes in the passes' scope.
    pub(crate) fn scoped_class_names(&self) -> Vec<Name> {
        match self.scope.class_keys() {
            None => self.classes.keys().cloned().collect(),
            Some(keys) => keys
                .iter()
                .filter_map(|key| {
                    self.classes
                        .get_with_name(key)
                        .map(|(name, _)| name.clone())
                })
                .collect(),
        }
    }

    /// The declared names of the functions in the passes' scope.
    pub(crate) fn scoped_function_names(&self) -> Vec<Name> {
        match self.scope.function_keys() {
            None => self.functions.keys().cloned().collect(),
            Some(keys) => keys
                .iter()
                .filter_map(|key| {
                    self.functions
                        .get_with_name(key)
                        .map(|(name, _)| name.clone())
                })
                .collect(),
        }
    }

    /// The classes in the passes' scope, with their declared names.
    pub(crate) fn scoped_classes(&self) -> Vec<(&Name, &crate::registry::ClassReflection)> {
        match self.scope.class_keys() {
            None => self.classes.iter().collect(),
            Some(keys) => keys
                .iter()
                .filter_map(|key| self.classes.get_with_name(key))
                .collect(),
        }
    }

    /// The functions in the passes' scope, with their declared names.
    pub(crate) fn scoped_functions(&self) -> Vec<(&Name, &crate::registry::FunctionReflection)> {
        match self.scope.function_keys() {
            None => self.functions.iter().collect(),
            Some(keys) => keys
                .iter()
                .filter_map(|key| self.functions.get_with_name(key))
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
            .filter(|key| !self.classes.contains_key(key))
            .cloned()
            .collect()
    }

    /// Reads the declarations of the names in `scope`, in parallel. A table
    /// read back from the cache decodes an entry the first time it is read,
    /// and the passes would otherwise read a scope's thousands of entries
    /// one after another.
    pub(crate) fn warm(&self, scope: &Scope) {
        use rayon::prelude::*;
        let raw = self.raw.as_deref();
        let classes: Vec<&String> = scope.class_keys().into_iter().flatten().collect();
        classes
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|key| {
                self.classes.get_with_name(key);
                if let Some(raw) = raw {
                    raw.classes.get_with_name(key);
                }
            });
        let functions: Vec<&String> = scope.function_keys().into_iter().flatten().collect();
        functions
            .par_iter()
            .with_min_len(crate::registry::PASS_CHUNK)
            .for_each(|key| {
                self.functions.get_with_name(key);
                if let Some(raw) = raw {
                    raw.functions.get_with_name(key);
                }
            });
    }

    /// Forgets the purity of the functions and methods in `scope`, for
    /// the purity passes to settle again; their declarations are as
    /// resolved otherwise.
    pub(crate) fn clear_purity(&mut self, scope: &Scope) {
        for key in scope.function_keys().into_iter().flatten() {
            let Some((name, sig)) = self.functions.get_with_name(key) else {
                continue;
            };
            if sig.has_purity() {
                let name = name.clone();
                self.functions.get_mut(&name).unwrap().clear_purity();
            }
        }
        for key in scope.class_keys().into_iter().flatten() {
            let Some((name, info)) = self.classes.get_with_name(key) else {
                continue;
            };
            if info.methods.values().any(|m| m.has_purity()) {
                let name = name.clone();
                for method in self.classes.get_mut(&name).unwrap().methods.values_mut() {
                    method.clear_purity();
                }
            }
        }
    }

    /// Sets which classes and functions the next finishing passes resolve.
    pub(crate) fn set_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }
}
