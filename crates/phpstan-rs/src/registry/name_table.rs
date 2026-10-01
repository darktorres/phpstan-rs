//! The registry's class and function tables, found the way PHP resolves
//! those names: case-insensitively.

use std::borrow::{Borrow, Cow};
use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::{Arc, OnceLock};
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
/// the table's own entries first, the base staying as it is. A base read
/// back from its serialized form (`Deserialize`) decodes each entry the
/// first time it is read.
#[derive(Debug, Clone)]
pub struct NameTable<V> {
    entries: FxHashMap<Folded, V>,
    base: Option<Arc<Base<V>>>,
    // The base's names this table hides: replaced in `entries`, or
    // removed.
    hidden: FxHashSet<Folded>,
}

/// A table's shared base: each entry decoded already, or kept encoded
/// until first read.
#[derive(Debug)]
struct Base<V> {
    entries: FxHashMap<Folded, Lazy<V>>,
}

/// What decodes an encoded `Lazy` value.
type Decoder<V> = fn(&[u8]) -> V;

/// A value kept encoded until first read, once read back from its
/// serialized form; a value built in memory holds it decoded.
#[derive(Debug)]
pub(crate) struct Lazy<V> {
    value: OnceLock<V>,
    // The entry encoded (`NameTable`'s `Serialize`), with what decodes it;
    // `None` once built decoded.
    encoded: Option<(Arc<Vec<u8>>, Decoder<V>)>,
}

impl<V> Lazy<V> {
    pub(crate) fn get(&self) -> &V {
        self.value.get_or_init(|| {
            let (bytes, decode) = self
                .encoded
                .as_ref()
                .expect("a slot built decoded holds its value");
            decode(bytes)
        })
    }
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
        base.entries
            .get_key_value(key)
            .map(|(declared, slot)| (declared, slot.get()))
    }

    fn base_contains(&self, key: &str) -> bool {
        self.base
            .as_ref()
            .is_some_and(|base| base.entries.contains_key(key))
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
        self.entries.contains_key(key.as_ref())
            || (self.base_contains(&key) && !self.hidden.contains(key.as_ref()))
    }

    /// Every value with its declared name.
    pub fn iter(&self) -> Iter<'_, V> {
        self.into_iter()
    }

    /// Every declared name, without reading any value.
    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        let base = self
            .base
            .iter()
            .flat_map(|base| base.entries.keys())
            .filter(|key| !self.hidden.contains(key.0.lower()));
        self.entries.keys().chain(base).map(|key| &key.0)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }

    /// The entries the table holds itself, not those it reads from its
    /// shared base.
    pub(crate) fn own_iter(&self) -> impl Iterator<Item = (&Name, &V)> {
        self.entries.iter().map(|(key, value)| (&key.0, value))
    }

    pub fn len(&self) -> usize {
        self.entries.len() + self.base.as_ref().map_or(0, |base| base.entries.len())
            - self.hidden.len()
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
                .entries
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
        if self.entries.is_empty() && self.hidden.is_empty() {
            if self.base.is_none() {
                self.base = Some(Arc::new(Base {
                    entries: FxHashMap::default(),
                }));
            }
            return;
        }
        let mut all: FxHashMap<Folded, Lazy<V>> = FxHashMap::default();
        if let Some(base) = self.base.take() {
            for (key, slot) in base.entries.iter() {
                if !self.hidden.contains(key.0.lower()) {
                    all.insert(key.clone(), Lazy::decoded(slot.get().clone()));
                }
            }
        }
        for (key, value) in std::mem::take(&mut self.entries) {
            all.insert(key, Lazy::decoded(value));
        }
        self.hidden.clear();
        self.base = Some(Arc::new(Base { entries: all }));
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
    base: Option<std::collections::hash_map::Iter<'a, Folded, Lazy<V>>>,
    hidden: &'a FxHashSet<Folded>,
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a Name, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((key, value)) = self.entries.next() {
            return Some((&key.0, value));
        }
        let base = self.base.as_mut()?;
        for (key, slot) in base {
            if !self.hidden.contains(key.0.lower()) {
                return Some((&key.0, slot.get()));
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
            base: self.base.as_ref().map(|base| base.entries.iter()),
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
            for (key, slot) in base.entries.iter() {
                if !self.hidden.contains(key.0.lower()) {
                    all.push((key.0.clone(), slot.get().clone()));
                }
            }
        }
        all.extend(self.entries.into_iter().map(|(k, v)| (k.0, v)));
        all.into_iter()
    }
}

impl<V> Lazy<V> {
    pub(crate) fn decoded(value: V) -> Lazy<V> {
        Lazy {
            value: OnceLock::from(value),
            encoded: None,
        }
    }
}

/// A table serializes as its entries, each with its declared name and
/// the entry itself encoded on its own, so a table read back decodes only
/// the entries it reads.
impl<V: serde::Serialize> serde::Serialize for NameTable<V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeSeq};
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for (name, value) in self.iter() {
            let mut bytes = Vec::new();
            ciborium::ser::into_writer(value, &mut bytes).map_err(S::Error::custom)?;
            seq.serialize_element(&(name, Bytes(bytes)))?;
        }
        seq.end()
    }
}

/// A table read back is a frozen base whose entries decode on first read.
impl<'de, V: Clone + serde::de::DeserializeOwned> serde::Deserialize<'de> for NameTable<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = <Vec<(Name, Bytes)>>::deserialize(deserializer)?;
        let entries = entries
            .into_iter()
            .map(|(name, bytes)| {
                let slot = Lazy {
                    value: OnceLock::new(),
                    encoded: Some((Arc::new(bytes.0), decode_entry::<V> as Decoder<V>)),
                };
                (Folded(name), slot)
            })
            .collect();
        Ok(NameTable {
            entries: FxHashMap::default(),
            base: Some(Arc::new(Base { entries })),
            hidden: FxHashSet::default(),
        })
    }
}

/// A table's entries stored as what differs from another table's: an entry
/// that table holds identically is not written again, and reads back
/// sharing that entry's encoding.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Diff(Vec<(Name, Option<Bytes>)>);

impl<V: Clone + serde::Serialize + serde::de::DeserializeOwned> NameTable<V> {
    /// This table's entries, without those `other` holds identically.
    pub(crate) fn diff_against(&self, other: &NameTable<V>) -> Result<Diff, String> {
        let encode = |value: &V| -> Result<Vec<u8>, String> {
            let mut bytes = Vec::new();
            ciborium::ser::into_writer(value, &mut bytes).map_err(|e| e.to_string())?;
            Ok(bytes)
        };
        let mut entries = Vec::with_capacity(self.len());
        for (name, value) in self.iter() {
            let bytes = encode(value)?;
            let same = match other.get(name) {
                Some(theirs) => encode(theirs)? == bytes,
                None => false,
            };
            entries.push((name.clone(), (!same).then_some(Bytes(bytes))));
        }
        Ok(Diff(entries))
    }

    /// The table `diff` was made from, reading what it leaves out from
    /// `other`.
    pub(crate) fn from_diff(diff: Diff, other: &NameTable<V>) -> Result<NameTable<V>, String> {
        let mut entries: FxHashMap<Folded, Lazy<V>> = FxHashMap::default();
        for (name, bytes) in diff.0 {
            let encoded: Arc<Vec<u8>> = match bytes {
                Some(bytes) => Arc::new(bytes.0),
                None => {
                    let key = function_key(&name);
                    let slot = other
                        .base
                        .as_ref()
                        .and_then(|base| base.entries.get(key.as_ref()));
                    match slot.and_then(|slot| slot.encoded.as_ref()) {
                        Some((encoded, _)) => Arc::clone(encoded),
                        None => {
                            let value = other
                                .get(&name)
                                .ok_or_else(|| format!("{name} is in neither table"))?;
                            let mut bytes = Vec::new();
                            ciborium::ser::into_writer(value, &mut bytes)
                                .map_err(|e| e.to_string())?;
                            Arc::new(bytes)
                        }
                    }
                }
            };
            entries.insert(
                Folded(name),
                Lazy {
                    value: OnceLock::new(),
                    encoded: Some((encoded, decode_entry::<V> as Decoder<V>)),
                },
            );
        }
        Ok(NameTable {
            entries: FxHashMap::default(),
            base: Some(Arc::new(Base { entries })),
            hidden: FxHashSet::default(),
        })
    }
}

/// A lazy value serializes encoded on its own.
impl<V: serde::Serialize> serde::Serialize for Lazy<V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(self.get(), &mut bytes).map_err(S::Error::custom)?;
        Bytes(bytes).serialize(serializer)
    }
}

impl<'de, V: serde::de::DeserializeOwned> serde::Deserialize<'de> for Lazy<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = Bytes::deserialize(deserializer)?;
        Ok(Lazy {
            value: OnceLock::new(),
            encoded: Some((Arc::new(bytes.0), decode_entry::<V> as Decoder<V>)),
        })
    }
}

/// One encoded entry, read back.
fn decode_entry<V: serde::de::DeserializeOwned>(bytes: &[u8]) -> V {
    ciborium::de::from_reader_with_recursion_limit(bytes, super::stub_cache::RECURSION_LIMIT)
        .unwrap_or_else(|e| panic!("a stored registry entry no longer decodes: {e}"))
}

/// Bytes serialized as one byte string.
#[derive(Debug, Clone, Default)]
pub(super) struct Bytes(pub(super) Vec<u8>);

impl serde::Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Bytes;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a byte string")
            }

            fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(bytes.to_vec()))
            }

            fn visit_byte_buf<E: serde::de::Error>(self, bytes: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(bytes))
            }
        }
        deserializer.deserialize_byte_buf(Visitor)
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
