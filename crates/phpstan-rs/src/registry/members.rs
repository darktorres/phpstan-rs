//! A class's members by case-insensitive name, each decoded the first time
//! it is read.
//!
//! A stored class holds its methods as separately encoded entries: reading
//! the class reads the names and the bytes, and a method is decoded when
//! something asks for it. A member built in memory is held decoded. The
//! interface is `FoldedMap`'s, with a value read as the value.

use syntax::name::{FoldKey, FoldedMap, Name};

use crate::registry::Lazy;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
#[serde(bound(
    serialize = "V: serde::Serialize",
    deserialize = "V: serde::de::DeserializeOwned"
))]
pub struct Members<V> {
    map: FoldedMap<Lazy<V>>,
}

impl<V> Default for Members<V> {
    fn default() -> Members<V> {
        Members {
            map: FoldedMap::default(),
        }
    }
}

impl<V> Members<V> {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get<K: FoldKey + ?Sized>(&self, key: &K) -> Option<&V> {
        self.map.get(key).map(Lazy::get)
    }

    pub fn get_key_value<K: FoldKey + ?Sized>(&self, key: &K) -> Option<(&Name, &V)> {
        self.map
            .get_key_value(key)
            .map(|(name, value)| (name, value.get()))
    }

    pub fn get_mut<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<&mut V> {
        self.map.get_mut(key).map(Lazy::get_mut)
    }

    pub fn contains_key<K: FoldKey + ?Sized>(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Stores `value` under `name`, returning what it replaced.
    pub fn insert(&mut self, name: Name, value: V) -> Option<V> {
        self.map
            .insert(name, Lazy::decoded(value))
            .map(Lazy::into_inner)
    }

    /// Stores `value` under `name` unless `name` finds a member: whether it
    /// was stored.
    pub fn insert_if_absent(&mut self, name: Name, value: V) -> bool {
        self.map.insert_if_absent(name, Lazy::decoded(value))
    }

    pub fn get_or_insert_with(&mut self, name: Name, make: impl FnOnce() -> V) -> &mut V {
        self.map
            .get_or_insert_with(name, || Lazy::decoded(make()))
            .get_mut()
    }

    pub fn remove<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<V> {
        self.map.remove(key).map(Lazy::into_inner)
    }

    pub fn remove_entry<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<(Name, V)> {
        self.map
            .remove_entry(key)
            .map(|(name, value)| (name, value.into_inner()))
    }

    /// Every member with its declared name, each decoded as it is reached.
    pub fn iter(&self) -> impl Iterator<Item = (&Name, &V)> {
        self.map.iter().map(|(name, value)| (name, value.get()))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Name, &mut V)> {
        self.map
            .iter_mut()
            .map(|(name, value)| (name, value.get_mut()))
    }

    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        self.map.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.map.values().map(Lazy::get)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.map.values_mut().map(Lazy::get_mut)
    }

    /// How many members are still the bytes they were read from.
    #[cfg(test)]
    pub(crate) fn encoded_count(&self) -> usize {
        self.map.values().filter(|value| value.is_encoded()).count()
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&Name, &mut V) -> bool) {
        self.map.retain(|name, value| keep(name, value.get_mut()));
    }
}

impl<V: PartialEq> PartialEq for Members<V> {
    fn eq(&self, other: &Members<V>) -> bool {
        self.map == other.map
    }
}

impl<V> std::ops::Index<&str> for Members<V> {
    type Output = V;

    fn index(&self, name: &str) -> &V {
        self.get(name).unwrap_or_else(|| panic!("no member {name}"))
    }
}

impl<'a, V> IntoIterator for &'a Members<V> {
    type Item = (&'a Name, &'a V);
    type IntoIter = Box<dyn Iterator<Item = (&'a Name, &'a V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<V: 'static> IntoIterator for Members<V> {
    type Item = (Name, V);
    type IntoIter = Box<dyn Iterator<Item = (Name, V)>>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(
            self.map
                .into_iter()
                .map(|(name, value)| (name, value.into_inner())),
        )
    }
}

impl<V> Extend<(Name, V)> for Members<V> {
    fn extend<I: IntoIterator<Item = (Name, V)>>(&mut self, members: I) {
        self.map.extend(
            members
                .into_iter()
                .map(|(name, value)| (name, Lazy::decoded(value))),
        );
    }
}

impl<V> FromIterator<(Name, V)> for Members<V> {
    fn from_iter<I: IntoIterator<Item = (Name, V)>>(members: I) -> Members<V> {
        Members {
            map: members
                .into_iter()
                .map(|(name, value)| (name, Lazy::decoded(value)))
                .collect(),
        }
    }
}
