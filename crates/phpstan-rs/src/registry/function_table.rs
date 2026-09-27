//! The registry's functions, found the way PHP resolves them:
//! case-insensitively.

use std::borrow::Cow;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::FunctionSig;

/// A function name's lookup key: the name ASCII-lowercased, as PHP
/// compares function names.
pub(crate) fn function_key(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

/// `function_key`, as a stored key.
pub(crate) fn folded_key(name: &str) -> Arc<str> {
    function_key(name).into()
}

/// Functions by fully-qualified name. Every lookup folds case, and each
/// entry keeps the spelling it was declared with, which iteration yields.
#[derive(Debug, Default, Clone)]
pub struct FunctionTable {
    entries: FxHashMap<Arc<str>, (Arc<str>, FunctionSig)>,
}

impl FunctionTable {
    pub fn get(&self, name: &str) -> Option<&FunctionSig> {
        self.entries
            .get(function_key(name).as_ref())
            .map(|(_, sig)| sig)
    }

    /// The function `name` finds, with its declared spelling.
    pub fn get_with_name(&self, name: &str) -> Option<(&str, &FunctionSig)> {
        self.entries
            .get(function_key(name).as_ref())
            .map(|(declared, sig)| (declared.as_ref(), sig))
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut FunctionSig> {
        self.entries
            .get_mut(function_key(name).as_ref())
            .map(|(_, sig)| sig)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.entries.contains_key(function_key(name).as_ref())
    }

    /// Stores `sig` under `name`, which becomes the declared spelling,
    /// replacing whatever `name` found before.
    pub fn insert(&mut self, name: Arc<str>, sig: FunctionSig) -> Option<FunctionSig> {
        let key: Arc<str> = match function_key(&name) {
            Cow::Borrowed(_) => Arc::clone(&name),
            Cow::Owned(folded) => folded.into(),
        };
        self.entries.insert(key, (name, sig)).map(|(_, old)| old)
    }

    /// Stores `sig` under `name` unless `name` already finds a function.
    pub fn insert_if_absent(&mut self, name: Arc<str>, sig: FunctionSig) {
        if !self.contains_key(&name) {
            self.insert(name, sig);
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<FunctionSig> {
        self.entries
            .remove(function_key(name).as_ref())
            .map(|(_, sig)| sig)
    }

    /// Every function with its declared name.
    pub fn iter(&self) -> impl Iterator<Item = (&Arc<str>, &FunctionSig)> {
        self.into_iter()
    }

    /// Every declared name.
    pub fn keys(&self) -> impl Iterator<Item = &Arc<str>> {
        self.entries.values().map(|(name, _)| name)
    }

    /// Every function with its declared name, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Arc<str>, &mut FunctionSig)> {
        self.entries.values_mut().map(|(name, sig)| (&*name, sig))
    }

    pub fn values(&self) -> impl Iterator<Item = &FunctionSig> {
        self.entries.values().map(|(_, sig)| sig)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut FunctionSig> {
        self.entries.values_mut().map(|(_, sig)| sig)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::ops::Index<&str> for FunctionTable {
    type Output = FunctionSig;

    fn index(&self, name: &str) -> &FunctionSig {
        self.get(name)
            .unwrap_or_else(|| panic!("no function {name} in the registry"))
    }
}

type Entry = (Arc<str>, FunctionSig);

fn split(entry: &Entry) -> (&Arc<str>, &FunctionSig) {
    (&entry.0, &entry.1)
}

impl<'a> IntoIterator for &'a FunctionTable {
    type Item = (&'a Arc<str>, &'a FunctionSig);
    type IntoIter = std::iter::Map<
        std::collections::hash_map::Values<'a, Arc<str>, Entry>,
        fn(&'a Entry) -> (&'a Arc<str>, &'a FunctionSig),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.values().map(split)
    }
}

impl IntoIterator for FunctionTable {
    type Item = Entry;
    type IntoIter = std::collections::hash_map::IntoValues<Arc<str>, Entry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_values()
    }
}
