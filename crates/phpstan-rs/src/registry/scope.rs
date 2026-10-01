//! Which classes and functions a run of the finishing passes resolves:
//! all of them, or only those a change can have affected, every other
//! entry being read as already resolved.

use rustc_hash::FxHashSet;
use syntax::name::Name;

use crate::registry::Registry;
use crate::registry::name_table::function_key;

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

    pub(crate) fn is_all(&self) -> bool {
        self.classes.is_none() && self.functions.is_none()
    }

    pub(crate) fn has_class(&self, name: &str) -> bool {
        self.classes
            .as_ref()
            .is_none_or(|classes| classes.contains(function_key(name).as_ref()))
    }

    pub(crate) fn has_function(&self, name: &str) -> bool {
        self.functions
            .as_ref()
            .is_none_or(|functions| functions.contains(function_key(name).as_ref()))
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

    /// Sets which classes and functions the next finishing passes resolve.
    pub(crate) fn set_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }
}
