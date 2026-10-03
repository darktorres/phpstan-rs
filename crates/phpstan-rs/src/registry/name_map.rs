//! Maps and sets by a name as it is written: what the registry keeps by
//! constant names, and by names it has already folded. Built like
//! `NameTable` - over a shared base read in place from the stored file -
//! but a name is found only by its own spelling.

use std::ops::Range;
use std::sync::Arc;
use syntax::name::Name;

use super::container::{Reader, Stored, Writer};
use super::name_table::{Base, load_base, store_entries};

/// Values by an exact name. A map can sit over a shared, immutable base
/// (`freeze`, `load`): cloning it then shares the base, and a change copies
/// the entry it touches into the map's own entries first. A base read from
/// a stored file decodes each entry the first time it is read.
///
/// Iteration yields the map's own entries first, then the base's in the
/// order of their names.
#[derive(Debug, Clone)]
pub struct NameMap<V> {
    entries: syntax::name::NameMap<V>,
    base: Option<Arc<Base<V>>>,
    // The base's names this map hides: replaced in `entries`, or removed.
    hidden: syntax::name::NameSet,
}

impl<V> Default for NameMap<V> {
    fn default() -> Self {
        NameMap {
            entries: Default::default(),
            base: None,
            hidden: Default::default(),
        }
    }
}

impl<V> NameMap<V> {
    /// The base's entry for `name`, unless hidden.
    fn base_get(&self, name: &str) -> Option<(&Name, &V)> {
        let base = self.base.as_ref()?;
        if self.hidden.contains(name) {
            return None;
        }
        let i = base.find(name)?;
        Some((base.name(i), base.value(i)))
    }

    pub fn get(&self, name: &str) -> Option<&V> {
        self.entries
            .get(name)
            .or_else(|| self.base_get(name).map(|(_, value)| value))
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.entries.contains_key(name)
            || (!self.hidden.contains(name)
                && self
                    .base
                    .as_ref()
                    .is_some_and(|base| base.find(name).is_some()))
    }

    /// Every entry: the map's own, then the base's it doesn't hide.
    pub fn iter(&self) -> Iter<'_, V> {
        self.into_iter()
    }

    /// Every name, without reading any value.
    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        let base = self.base.iter().flat_map(|base| {
            (0..base.len())
                .map(move |i| base.name(i))
                .filter(|name| !self.hidden.contains(name.as_str()))
        });
        self.entries.keys().chain(base)
    }

    pub fn len(&self) -> usize {
        self.entries.len() + self.base.as_ref().map_or(0, |base| base.len()) - self.hidden.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<V: Clone> NameMap<V> {
    /// Copies the base's entry for `name` into the map's own entries, when
    /// it has a visible one.
    fn own(&mut self, name: &str) {
        if self.entries.contains_key(name) {
            return;
        }
        if let Some((declared, value)) = self.base_get(name) {
            let (declared, value) = (declared.clone(), value.clone());
            self.hidden.insert(declared.clone());
            self.entries.insert(declared, value);
        }
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut V> {
        self.own(name);
        self.entries.get_mut(name)
    }

    /// Stores `value` under `name`. What it replaces is not read: `replace`
    /// hands it back.
    pub fn insert(&mut self, name: impl Into<Name>, value: V) {
        let name = name.into();
        self.forget(&name);
        self.entries.insert(name, value);
    }

    /// `insert`, returning what `name` held before.
    pub fn replace(&mut self, name: impl Into<Name>, value: V) -> Option<V> {
        let name = name.into();
        let old = self.take(&name);
        self.entries.insert(name, value);
        old
    }

    /// Drops what `name` holds without reading it: whether there was
    /// anything. A base entry is hidden, not copied.
    fn forget(&mut self, name: &str) -> bool {
        if self.entries.remove(name).is_some() {
            return true;
        }
        if self.hidden.contains(name) {
            return false;
        }
        let Some(base) = &self.base else {
            return false;
        };
        let Some(i) = base.find(name) else {
            return false;
        };
        self.hidden.insert(base.name(i).clone());
        true
    }

    /// Removes what `name` holds, without reading it: whether there was
    /// anything. `take` hands it back.
    pub fn remove(&mut self, name: &str) -> bool {
        self.forget(name)
    }

    /// Removes what `name` holds and returns it.
    pub fn take(&mut self, name: &str) -> Option<V> {
        if let Some(value) = self.entries.remove(name) {
            return Some(value);
        }
        let (declared, value) = self.base_get(name)?;
        let (declared, value) = (declared.clone(), value.clone());
        self.hidden.insert(declared);
        Some(value)
    }

    /// Makes every entry part of a shared base, for clones of the map to
    /// share.
    pub fn freeze(&mut self) {
        if self.entries.is_empty() && self.hidden.is_empty() {
            if self.base.is_none() {
                self.base = Some(Arc::new(Base::empty(false)));
            }
            return;
        }
        let mut all: Vec<(Name, V)> = Vec::with_capacity(self.len());
        if let Some(base) = self.base.take() {
            for i in 0..base.len() {
                let name = base.name(i);
                if !self.hidden.contains(name.as_str()) {
                    all.push((name.clone(), base.value(i).clone()));
                }
            }
        }
        all.extend(std::mem::take(&mut self.entries));
        self.hidden.clear();
        self.base = Some(Arc::new(Base::built(all, false)));
    }
}

impl<V> std::ops::Index<&str> for NameMap<V> {
    type Output = V;

    fn index(&self, name: &str) -> &V {
        self.get(name)
            .unwrap_or_else(|| panic!("no {name} in the registry"))
    }
}

/// `NameMap::iter`'s iterator.
pub struct Iter<'a, V> {
    entries: syntax::name::NameMapIter<'a, V>,
    base: Option<(&'a Base<V>, Range<usize>)>,
    hidden: &'a syntax::name::NameSet,
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a Name, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(entry) = self.entries.next() {
            return Some(entry);
        }
        let (base, positions) = self.base.as_mut()?;
        let base: &'a Base<V> = base;
        for i in positions {
            let name = base.name(i);
            if !self.hidden.contains(name.as_str()) {
                return Some((name, base.value(i)));
            }
        }
        None
    }
}

impl<'a, V> IntoIterator for &'a NameMap<V> {
    type Item = (&'a Name, &'a V);
    type IntoIter = Iter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            entries: self.entries.iter(),
            base: self.base.as_ref().map(|base| (&**base, 0..base.len())),
            hidden: &self.hidden,
        }
    }
}

impl<V: Clone> IntoIterator for NameMap<V> {
    type Item = (Name, V);
    type IntoIter = std::vec::IntoIter<(Name, V)>;

    fn into_iter(self) -> Self::IntoIter {
        let mut all: Vec<(Name, V)> = Vec::with_capacity(self.len());
        if let Some(base) = &self.base {
            for i in 0..base.len() {
                let name = base.name(i);
                if !self.hidden.contains(name.as_str()) {
                    all.push((name.clone(), base.value(i).clone()));
                }
            }
        }
        all.extend(self.entries);
        all.into_iter()
    }
}

impl<V: Clone> Extend<(Name, V)> for NameMap<V> {
    fn extend<I: IntoIterator<Item = (Name, V)>>(&mut self, iter: I) {
        for (name, value) in iter {
            self.insert(name, value);
        }
    }
}

impl<V: Clone> FromIterator<(Name, V)> for NameMap<V> {
    fn from_iter<I: IntoIterator<Item = (Name, V)>>(iter: I) -> Self {
        let mut map = NameMap::default();
        map.extend(iter);
        map
    }
}

impl<V: serde::Serialize> NameMap<V> {
    /// Writes the map as section `section`, as `NameTable::store` writes a
    /// table, its names ordered as written.
    pub(super) fn store(&self, writer: &mut Writer, section: &str) -> Result<(), String> {
        store_entries(writer, section, self.iter().collect(), false)
    }
}

impl<V: serde::de::DeserializeOwned + Send + Sync + 'static> NameMap<V> {
    /// The map `store` wrote as section `section`: a base read in place.
    pub(super) fn load(reader: &Reader, section: &str) -> Result<NameMap<V>, String> {
        Ok(NameMap {
            entries: Default::default(),
            base: Some(Arc::new(load_base(reader, section, false)?)),
            hidden: Default::default(),
        })
    }
}

impl<V: serde::de::DeserializeOwned + Send + Sync + 'static> Stored for NameMap<V> {
    fn load(reader: &Reader, section: &str) -> Result<NameMap<V>, String> {
        NameMap::load(reader, section)
    }
}

impl Stored for NameSet {
    fn load(reader: &Reader, section: &str) -> Result<NameSet, String> {
        NameSet::load(reader, section)
    }
}

#[cfg(test)]
impl<V> NameMap<V> {
    /// `Base::check_order` of the map's base.
    pub(super) fn check_order(&self, section: &str) -> Result<(), String> {
        self.base
            .as_ref()
            .map_or(Ok(()), |base| base.check_order(section))
    }
}

/// A set of names as written, over the same base as `NameMap`.
#[derive(Debug, Clone, Default)]
pub struct NameSet(NameMap<()>);

impl NameSet {
    pub fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    /// Adds `name`; whether it was not there already.
    pub fn insert(&mut self, name: impl Into<Name>) -> bool {
        let name = name.into();
        let new = !self.0.contains_key(&name);
        if new {
            self.0.insert(name, ());
        }
        new
    }

    /// Removes `name`; whether it was there.
    pub fn remove(&mut self, name: &str) -> bool {
        self.0.remove(name)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Name> {
        self.0.keys()
    }

    pub fn freeze(&mut self) {
        self.0.freeze();
    }

    pub(super) fn store(&self, writer: &mut Writer, section: &str) -> Result<(), String> {
        self.0.store(writer, section)
    }

    pub(super) fn load(reader: &Reader, section: &str) -> Result<NameSet, String> {
        Ok(NameSet(NameMap::load(reader, section)?))
    }

    #[cfg(test)]
    pub(super) fn check_order(&self, section: &str) -> Result<(), String> {
        self.0.check_order(section)
    }
}

impl<N: Into<Name>> Extend<N> for NameSet {
    fn extend<I: IntoIterator<Item = N>>(&mut self, iter: I) {
        for name in iter {
            self.insert(name);
        }
    }
}

impl<N: Into<Name>> FromIterator<N> for NameSet {
    fn from_iter<I: IntoIterator<Item = N>>(iter: I) -> Self {
        let mut set = NameSet::default();
        set.extend(iter);
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(map: &NameMap<u32>) -> Reader {
        let mut writer = Writer::new();
        map.store(&mut writer, "m").unwrap();
        match Reader::new(writer.finish()) {
            Ok(reader) => reader,
            Err(_) => panic!("a written file reads"),
        }
    }

    fn map(entries: &[(&str, u32)]) -> NameMap<u32> {
        entries
            .iter()
            .map(|(name, value)| (Name::from(*name), *value))
            .collect()
    }

    #[test]
    fn a_stored_map_is_found_by_the_exact_spelling_only() {
        let reader = written(&map(&[
            ("PHP_EOL", 1),
            ("php_eol", 2),
            ("Foo\\BAR", 3),
            ("a", 4),
        ]));
        let stored: NameMap<u32> = NameMap::load(&reader, "m").unwrap();
        assert_eq!(stored.len(), 4);
        assert_eq!(stored.get("PHP_EOL"), Some(&1));
        assert_eq!(stored.get("php_eol"), Some(&2));
        assert_eq!(stored.get("Foo\\BAR"), Some(&3));
        assert_eq!(stored.get("Foo\\bar"), None);
        assert_eq!(stored.get("A"), None);
        assert!(!stored.contains_key("PHP_EO"));
        assert!(!stored.contains_key("PHP_EOLS"));
    }

    #[test]
    fn changes_over_a_stored_map_hide_and_replace_its_entries() {
        let reader = written(&map(&[("One", 1), ("Two", 2), ("Three", 3)]));
        let mut stored: NameMap<u32> = NameMap::load(&reader, "m").unwrap();
        let before = stored.clone();
        *stored.get_mut("Two").unwrap() = 20;
        assert_eq!(stored.take("Three"), Some(3));
        assert_eq!(stored.replace(Name::from("Four"), 4), None);
        assert_eq!(stored.replace(Name::from("One"), 10), Some(1));
        assert_eq!(stored.len(), 3);
        let mut seen: Vec<(String, u32)> =
            stored.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        seen.sort();
        assert_eq!(
            seen,
            [("Four".into(), 4), ("One".into(), 10), ("Two".into(), 20)]
        );
        assert_eq!(before.get("Two"), Some(&2));
        assert_eq!(before.get("Three"), Some(&3));
        assert_eq!(before.len(), 3);
    }

    #[test]
    fn a_frozen_map_keeps_what_it_held() {
        let reader = written(&map(&[("b", 1), ("a", 2)]));
        let mut stored: NameMap<u32> = NameMap::load(&reader, "m").unwrap();
        stored.remove("b");
        stored.insert(Name::from("c"), 3);
        stored.freeze();
        let names: Vec<&str> = stored.keys().map(|n| n.as_str()).collect();
        assert_eq!(names, ["a", "c"]);
        assert_eq!(stored.get("c"), Some(&3));
        assert_eq!(stored.get("b"), None);
    }

    #[test]
    fn a_set_reads_back_what_it_held() {
        let mut set: NameSet = ["Foo", "bar", "Baz"].into_iter().map(Name::from).collect();
        assert!(!set.insert(Name::from("Foo")));
        let mut writer = Writer::new();
        set.store(&mut writer, "s").unwrap();
        let reader = match Reader::new(writer.finish()) {
            Ok(reader) => reader,
            Err(_) => panic!("a written file reads"),
        };
        let mut stored = NameSet::load(&reader, "s").unwrap();
        assert_eq!(stored.len(), 3);
        assert!(stored.contains("Foo") && stored.contains("bar") && !stored.contains("foo"));
        assert!(stored.remove("bar") && !stored.remove("bar"));
        assert!(stored.insert(Name::from("new")) && stored.contains("new"));
        assert!(!stored.contains("bar"));
        set.freeze();
        assert!(set.contains("Baz"));
    }
}
