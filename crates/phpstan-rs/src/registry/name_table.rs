//! The registry's class and function tables, found the way PHP resolves
//! those names: case-insensitively.

use std::borrow::{Borrow, Cow};
use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
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
struct Folded(Name);

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
///
/// A table can sit over a shared, immutable base (`freeze`): cloning it
/// then shares the base, and a change copies the entry it touches into
/// the table's own entries first, the base staying as it is.
#[derive(Debug, Clone)]
pub struct NameTable<V> {
    entries: FxHashMap<Folded, V>,
    base: Option<Arc<FxHashMap<Folded, V>>>,
    // The base's names this table hides: replaced in `entries`, or
    // removed.
    hidden: FxHashSet<Folded>,
}

impl<V> Default for NameTable<V> {
    fn default() -> Self {
        NameTable {
            entries: FxHashMap::default(),
            base: None,
            hidden: FxHashSet::default(),
        }
    }
}

impl<V> NameTable<V> {
    /// The base's entry for `key` (a lowercased name), unless hidden.
    fn base_get(&self, key: &str) -> Option<(&Folded, &V)> {
        let base = self.base.as_ref()?;
        if self.hidden.contains(key) {
            return None;
        }
        base.get_key_value(key)
    }

    fn base_contains(&self, key: &str) -> bool {
        self.base
            .as_ref()
            .is_some_and(|base| base.contains_key(key))
    }

    pub fn get(&self, name: &str) -> Option<&V> {
        let key = function_key(name);
        self.entries
            .get(key.as_ref())
            .or_else(|| self.base_get(&key).map(|(_, value)| value))
    }

    /// The value `name` finds, with its declared name.
    pub fn get_with_name(&self, name: &str) -> Option<(&Name, &V)> {
        let key = function_key(name);
        self.entries
            .get_key_value(key.as_ref())
            .or_else(|| self.base_get(&key))
            .map(|(declared, value)| (&declared.0, value))
    }

    pub fn contains_key(&self, name: &str) -> bool {
        let key = function_key(name);
        self.entries.contains_key(key.as_ref()) || self.base_get(&key).is_some()
    }

    /// Every value with its declared name.
    pub fn iter(&self) -> Iter<'_, V> {
        self.into_iter()
    }

    /// Every declared name.
    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        self.iter().map(|(name, _)| name)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }

    pub fn len(&self) -> usize {
        self.entries.len() + self.base.as_ref().map_or(0, |base| base.len()) - self.hidden.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<V: Clone> NameTable<V> {
    /// Copies the base's entry for `key` into the table's own entries,
    /// when it has a visible one.
    fn own(&mut self, key: &str) {
        if self.entries.contains_key(key) {
            return;
        }
        if let Some((declared, value)) = self.base_get(key) {
            let (declared, value) = (declared.clone(), value.clone());
            self.hidden.insert(declared.clone());
            self.entries.insert(declared, value);
        }
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut V> {
        let key = function_key(name);
        self.own(&key);
        self.entries.get_mut(key.as_ref())
    }

    /// Stores `value` under `name`, which becomes the declared spelling,
    /// replacing whatever `name` found before.
    pub fn insert(&mut self, name: impl Into<Name>, value: V) -> Option<V> {
        let name = name.into();
        let old = self.remove(name.lower());
        let key = Folded(name);
        if self.base_contains(key.0.lower()) {
            self.hidden.insert(key.clone());
        }
        self.entries.insert(key, value);
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
        let name = name.into();
        if !self.contains_key(&name) {
            self.insert(name.clone(), make());
        }
        self.get_mut(&name).expect("just found or inserted")
    }

    pub fn remove(&mut self, name: &str) -> Option<V> {
        let key = function_key(name);
        if let Some(value) = self.entries.remove(key.as_ref()) {
            return Some(value);
        }
        let (declared, value) = self.base_get(&key)?;
        let (declared, value) = (declared.clone(), value.clone());
        self.hidden.insert(declared);
        Some(value)
    }

    /// Every value with its declared name, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Name, &mut V)> {
        self.iter_mut_where(|_| true)
    }

    /// The values whose declared name `wanted` accepts, with that name,
    /// mutably.
    pub fn iter_mut_where(
        &mut self,
        wanted: impl Fn(&Name) -> bool,
    ) -> impl Iterator<Item = (&Name, &mut V)> {
        let from_base: Vec<String> = match &self.base {
            Some(base) => base
                .keys()
                .filter(|key| !self.hidden.contains(key.0.lower()) && wanted(&key.0))
                .map(|key| key.0.lower().to_string())
                .collect(),
            None => Vec::new(),
        };
        for key in from_base {
            self.own(&key);
        }
        self.entries
            .iter_mut()
            .map(|(k, v)| (&k.0, v))
            .filter(move |(name, _)| wanted(name))
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.iter_mut().map(|(_, value)| value)
    }

    /// Makes every entry part of a shared base, for clones of the table
    /// to share.
    pub fn freeze(&mut self) {
        let mut all: FxHashMap<Folded, V> = std::mem::take(&mut self.entries);
        if let Some(base) = self.base.take() {
            for (key, value) in base.iter() {
                if !self.hidden.contains(key.0.lower()) {
                    all.insert(key.clone(), value.clone());
                }
            }
        }
        self.hidden.clear();
        self.base = Some(Arc::new(all));
    }
}

impl<V> std::ops::Index<&str> for NameTable<V> {
    type Output = V;

    fn index(&self, name: &str) -> &V {
        self.get(name)
            .unwrap_or_else(|| panic!("no {name} in the registry"))
    }
}

/// `NameTable::iter`'s iterator: the table's own entries, then the base's
/// it doesn't hide.
pub struct Iter<'a, V> {
    entries: std::collections::hash_map::Iter<'a, Folded, V>,
    base: Option<std::collections::hash_map::Iter<'a, Folded, V>>,
    hidden: &'a FxHashSet<Folded>,
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a Name, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((key, value)) = self.entries.next() {
            return Some((&key.0, value));
        }
        let base = self.base.as_mut()?;
        for (key, value) in base {
            if !self.hidden.contains(key.0.lower()) {
                return Some((&key.0, value));
            }
        }
        None
    }
}

impl<'a, V> IntoIterator for &'a NameTable<V> {
    type Item = (&'a Name, &'a V);
    type IntoIter = Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            entries: self.entries.iter(),
            base: self.base.as_ref().map(|base| base.iter()),
            hidden: &self.hidden,
        }
    }
}

impl<V: Clone> IntoIterator for NameTable<V> {
    type Item = (Name, V);
    type IntoIter = std::vec::IntoIter<(Name, V)>;

    fn into_iter(self) -> Self::IntoIter {
        let mut all: Vec<(Name, V)> = Vec::with_capacity(self.len());
        if let Some(base) = &self.base {
            for (key, value) in base.iter() {
                if !self.hidden.contains(key.0.lower()) {
                    all.push((key.0.clone(), value.clone()));
                }
            }
        }
        all.extend(self.entries.into_iter().map(|(k, v)| (k.0, v)));
        all.into_iter()
    }
}

/// A table serializes as its entries, each with its declared name.
impl<V: serde::Serialize> serde::Serialize for NameTable<V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de, V: Clone + serde::Deserialize<'de>> serde::Deserialize<'de> for NameTable<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = <Vec<(Name, V)>>::deserialize(deserializer)?;
        Ok(entries.into_iter().collect())
    }
}

impl<V: Clone> FromIterator<(Name, V)> for NameTable<V> {
    fn from_iter<I: IntoIterator<Item = (Name, V)>>(iter: I) -> Self {
        let mut table = NameTable::default();
        for (name, value) in iter {
            table.insert(name, value);
        }
        table
    }
}
