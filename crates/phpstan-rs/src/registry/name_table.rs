//! The registry's class and function tables, found the way PHP resolves
//! those names: case-insensitively.

use std::borrow::{Borrow, Cow};
use std::hash::{Hash, Hasher};

use rustc_hash::FxHashMap;
use syntax::name::Name;

/// A name's lookup key: the name ASCII-lowercased, as PHP compares class
/// and function names.
pub(crate) fn function_key(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

/// `function_key`, as a stored key.
pub(crate) fn folded_key(name: &str) -> Name {
    function_key(name).into()
}

/// A name as a table key: hashed, compared and borrowed as its lowercase
/// form.
#[derive(Debug, Clone)]
pub struct Folded(Name);

impl PartialEq for Folded {
    fn eq(&self, other: &Folded) -> bool {
        self.0.eq_ignore_case(&other.0)
    }
}

impl Eq for Folded {}

impl Hash for Folded {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.lower().hash(state);
    }
}

impl Borrow<str> for Folded {
    fn borrow(&self) -> &str {
        self.0.lower()
    }
}

/// Values by a case-insensitive name. Every lookup folds case, and each
/// entry keeps the spelling it was declared with, which iteration yields.
#[derive(Debug, Clone)]
pub struct NameTable<V> {
    entries: FxHashMap<Folded, V>,
}

impl<V> Default for NameTable<V> {
    fn default() -> Self {
        NameTable {
            entries: FxHashMap::default(),
        }
    }
}

impl<V> NameTable<V> {
    pub fn get(&self, name: &str) -> Option<&V> {
        self.entries.get(function_key(name).as_ref())
    }

    /// The value `name` finds, with its declared name.
    pub fn get_with_name(&self, name: &str) -> Option<(&Name, &V)> {
        self.entries
            .get_key_value(function_key(name).as_ref())
            .map(|(declared, value)| (&declared.0, value))
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut V> {
        self.entries.get_mut(function_key(name).as_ref())
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.entries.contains_key(function_key(name).as_ref())
    }

    /// Stores `value` under `name`, which becomes the declared spelling,
    /// replacing whatever `name` found before.
    pub fn insert(&mut self, name: impl Into<Name>, value: V) -> Option<V> {
        let name = name.into();
        let old = self.entries.remove(name.lower());
        self.entries.insert(Folded(name), value);
        old
    }

    /// Stores `value` under `name` unless `name` already finds a value.
    pub fn insert_if_absent(&mut self, name: impl Into<Name>, value: V) {
        self.get_or_insert_with(name, || value);
    }

    /// The value `name` finds, first storing `make()` under it when there
    /// is none.
    pub fn get_or_insert_with(
        &mut self,
        name: impl Into<Name>,
        make: impl FnOnce() -> V,
    ) -> &mut V {
        self.entries.entry(Folded(name.into())).or_insert_with(make)
    }

    pub fn remove(&mut self, name: &str) -> Option<V> {
        self.entries.remove(function_key(name).as_ref())
    }

    /// Every value with its declared name.
    pub fn iter(&self) -> impl Iterator<Item = (&Name, &V)> {
        self.into_iter()
    }

    /// Every declared name.
    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        self.entries.keys().map(|k| &k.0)
    }

    /// Every value with its declared name, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Name, &mut V)> {
        self.entries.iter_mut().map(|(k, v)| (&k.0, v))
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.values()
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.values_mut()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<V> std::ops::Index<&str> for NameTable<V> {
    type Output = V;

    fn index(&self, name: &str) -> &V {
        self.get(name)
            .unwrap_or_else(|| panic!("no {name} in the registry"))
    }
}

fn split<'a, V>((k, v): (&'a Folded, &'a V)) -> (&'a Name, &'a V) {
    (&k.0, v)
}

impl<'a, V> IntoIterator for &'a NameTable<V> {
    type Item = (&'a Name, &'a V);
    type IntoIter = std::iter::Map<
        std::collections::hash_map::Iter<'a, Folded, V>,
        fn((&'a Folded, &'a V)) -> (&'a Name, &'a V),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(split)
    }
}

impl<V> IntoIterator for NameTable<V> {
    type Item = (Name, V);
    type IntoIter = std::iter::Map<
        std::collections::hash_map::IntoIter<Folded, V>,
        fn((Folded, V)) -> (Name, V),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter().map(|(k, v)| (k.0, v))
    }
}

impl<V> FromIterator<(Name, V)> for NameTable<V> {
    fn from_iter<I: IntoIterator<Item = (Name, V)>>(iter: I) -> Self {
        let mut table = NameTable::default();
        for (name, value) in iter {
            table.insert(name, value);
        }
        table
    }
}
