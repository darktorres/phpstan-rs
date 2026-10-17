//! A class's members by case-insensitive name, each decoded the first time
//! it is read.
//!
//! A stored class holds its methods as separately encoded entries: reading
//! the class reads the names and the bytes, and a method is decoded when
//! something asks for it. A member built in memory is held decoded. The
//! interface is `FoldedMap`'s, with a value read as the value.
//!
//! The map and each member sit behind a pointer, copied when first written:
//! a clone of the members is a pointer copy, and a write to one member
//! copies that member and the map of pointers, not the others.

use std::sync::Arc;

use syntax::name::{FoldKey, FoldedMap, Name};

use crate::file_id::FileId;
use crate::registry::function_sig::FunctionReflection;
use crate::registry::{Lazy, Shared};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
#[serde(bound(
    serialize = "V: serde::Serialize",
    deserialize = "V: serde::de::DeserializeOwned"
))]
pub struct Members<V> {
    map: Shared<FoldedMap<Arc<Lazy<V>>>>,
}

impl<V> Default for Members<V> {
    fn default() -> Members<V> {
        Members {
            map: Shared::default(),
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
        self.map.get(key).map(|member| member.get())
    }

    pub fn get_key_value<K: FoldKey + ?Sized>(&self, key: &K) -> Option<(&Name, &V)> {
        self.map
            .get_key_value(key)
            .map(|(name, member)| (name, member.get()))
    }

    pub fn contains_key<K: FoldKey + ?Sized>(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Every member with its declared name, each decoded as it is reached.
    pub fn iter(&self) -> impl Iterator<Item = (&Name, &V)> {
        self.map.iter().map(|(name, member)| (name, member.get()))
    }

    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        self.map.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.map.values().map(|member| member.get())
    }

    /// Whether both hold the same map of members, not merely equal ones.
    pub fn ptr_eq(&self, other: &Members<V>) -> bool {
        self.map.ptr_eq(&other.map)
    }

    /// How many members are still the bytes they were read from.
    #[cfg(test)]
    pub(crate) fn encoded_count(&self) -> usize {
        self.map
            .values()
            .filter(|member| member.is_encoded())
            .count()
    }
}

impl<V: Clone> Members<V> {
    pub fn get_mut<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<&mut V> {
        self.map
            .get_mut(key)
            .map(|member| Arc::make_mut(member).get_mut())
    }

    /// Stores `value` under `name`, returning what it replaced.
    pub fn insert(&mut self, name: Name, value: V) -> Option<V> {
        self.map
            .insert(name, Arc::new(Lazy::decoded(value)))
            .map(|member| Arc::unwrap_or_clone(member).into_inner())
    }

    /// Stores `value` under `name` unless `name` finds a member: whether it
    /// was stored.
    pub fn insert_if_absent(&mut self, name: Name, value: V) -> bool {
        if self.map.contains_key(&name) {
            return false;
        }
        self.map.insert(name, Arc::new(Lazy::decoded(value)));
        true
    }

    pub fn get_or_insert_with(&mut self, name: Name, make: impl FnOnce() -> V) -> &mut V {
        Arc::make_mut(
            self.map
                .get_or_insert_with(name, || Arc::new(Lazy::decoded(make()))),
        )
        .get_mut()
    }

    pub fn remove<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<V> {
        self.map
            .remove(key)
            .map(|member| Arc::unwrap_or_clone(member).into_inner())
    }

    pub fn remove_entry<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<(Name, V)> {
        self.map
            .remove_entry(key)
            .map(|(name, member)| (name, Arc::unwrap_or_clone(member).into_inner()))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Name, &mut V)> {
        self.map
            .iter_mut()
            .map(|(name, member)| (name, Arc::make_mut(member).get_mut()))
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.map
            .values_mut()
            .map(|member| Arc::make_mut(member).get_mut())
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&Name, &mut V) -> bool) {
        self.map
            .retain(|name, member| keep(name, Arc::make_mut(member).get_mut()));
    }
}

impl Members<FunctionReflection> {
    /// Gives every member its file's number, without decoding a member that
    /// is still encoded.
    pub(crate) fn bind_file(&mut self, file: FileId) {
        for member in self.map.values_mut() {
            Arc::make_mut(member).bind_file(file);
        }
    }
}

impl<V: PartialEq> PartialEq for Members<V> {
    fn eq(&self, other: &Members<V>) -> bool {
        self.ptr_eq(other) || self.map == other.map
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

impl<V: Clone + 'static> IntoIterator for Members<V> {
    type Item = (Name, V);
    type IntoIter = Box<dyn Iterator<Item = (Name, V)>>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(
            self.map
                .into_iter()
                .map(|(name, member)| (name, Arc::unwrap_or_clone(member).into_inner())),
        )
    }
}

impl<V: Clone> Extend<(Name, V)> for Members<V> {
    fn extend<I: IntoIterator<Item = (Name, V)>>(&mut self, members: I) {
        self.map.extend(
            members
                .into_iter()
                .map(|(name, value)| (name, Arc::new(Lazy::decoded(value)))),
        );
    }
}

impl<V> FromIterator<(Name, V)> for Members<V> {
    fn from_iter<I: IntoIterator<Item = (Name, V)>>(members: I) -> Members<V> {
        Members {
            map: members
                .into_iter()
                .map(|(name, value)| (name, Arc::new(Lazy::decoded(value))))
                .collect(),
        }
    }
}
