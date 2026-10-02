//! The registry's class and function tables, found the way PHP resolves
//! those names: case-insensitively.

use std::borrow::{Borrow, Cow};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::{Arc, OnceLock};
use syntax::name::Name;

use super::container::{Buffer, Reader, Stored, Writer};

/// A name's lookup key: the name ASCII-lowercased, as PHP compares class
/// and function names.
pub(crate) fn function_key(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

/// Calls `f` with `function_key(name)`, built on the stack when the name is
/// short, so a lookup of a name with capitals allocates nothing.
pub(crate) fn with_function_key<R>(name: &str, f: impl FnOnce(&str) -> R) -> R {
    if !name.bytes().any(|b| b.is_ascii_uppercase()) {
        return f(name);
    }
    if let Ok(mut key) = arrayvec::ArrayString::<128>::from(name) {
        key.make_ascii_lowercase();
        return f(&key);
    }
    f(&name.to_ascii_lowercase())
}

/// `function_key`, as a stored key.
pub(crate) fn folded_key(name: &str) -> Name {
    function_key(name).into()
}

/// How `text` orders against `key`, a lowercased name: by its own
/// lowercase form, without building it.
fn cmp_folded(text: &[u8], key: &[u8]) -> Ordering {
    for (&a, &b) in text.iter().zip(key) {
        let a = a.to_ascii_lowercase();
        if a != b {
            return a.cmp(&b);
        }
    }
    text.len().cmp(&key.len())
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
/// back from a stored file (`load`) is read in place: its names become
/// `Name`s, and its entries are decoded, the first time each is read.
///
/// Iteration yields the table's own entries first, then the base's. The
/// base's come in the order of their lowercased names, whichever way the
/// base was made; the own entries' order is the hash map's.
#[derive(Debug, Clone)]
pub struct NameTable<V> {
    entries: FxHashMap<Folded, V>,
    base: Option<Arc<Base<V>>>,
    // The base's names this table hides: replaced in `entries`, or
    // removed.
    hidden: FxHashSet<Folded>,
}

/// What decodes a stored entry.
type Decoder<V> = fn(&[u8]) -> V;

/// A table's shared base: its entries sorted by lowercased name, each
/// decoded already, or kept stored until first read.
#[derive(Debug)]
pub(super) struct Base<V> {
    keys: Keys,
    slots: Box<[Slot<V>]>,
    // What decodes an entry of a stored base.
    decode: Option<Decoder<V>>,
    // Whether the names are ordered and found by their lowercased form, as
    // class and function names are, or as they are written.
    fold: bool,
}

/// A base's names in order.
#[derive(Debug)]
enum Keys {
    /// The records of a stored table in the file's buffer: each four
    /// little-endian `u32`s, the name's offset and length, then the
    /// entry's. `index` is a hash index over them: little-endian `u32`s, a
    /// power of two of them, each the number of a record plus one, or zero
    /// for a free place.
    Stored {
        buffer: Buffer,
        records: Range<usize>,
        index: Range<usize>,
    },
    Built(Vec<Name>),
}

/// The hash a stored table's index files a name under: of its key, which is
/// the name lowercased when the table folds, eight bytes at a time.
fn hash_key(key: &[u8]) -> usize {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write(key);
    let hash = hasher.finish();
    (hash ^ (hash >> 32)) as usize
}

/// Whether `text` is `key` once lowercased, for a key already lowercased.
fn eq_text(fold: bool, text: &[u8], key: &[u8]) -> bool {
    if fold {
        text.len() == key.len() && cmp_folded(text, key) == Ordering::Equal
    } else {
        text == key
    }
}

/// The places a stored table's index has for `records` entries: at least
/// twice as many, a power of two, so a probe always ends at a free one.
fn index_places(records: usize) -> usize {
    (records * 2).next_power_of_two().max(2)
}

/// What a base keeps of one entry once it is read: the name as a `Name`,
/// for a stored one, and the decoded value.
#[derive(Debug)]
struct Slot<V> {
    name: OnceLock<Name>,
    value: OnceLock<Box<V>>,
}

impl<V> Default for Slot<V> {
    fn default() -> Self {
        Slot {
            name: OnceLock::new(),
            value: OnceLock::new(),
        }
    }
}

/// The bytes of one record of a stored table.
const RECORD_LEN: usize = 16;

impl Keys {
    fn len(&self) -> usize {
        match self {
            Keys::Stored { records, .. } => records.len() / RECORD_LEN,
            Keys::Built(names) => names.len(),
        }
    }

    /// The record `i` of a stored table: the ranges of its name and entry.
    fn record(&self, i: usize) -> (Range<usize>, Range<usize>) {
        let Keys::Stored {
            buffer, records, ..
        } = self
        else {
            unreachable!("only a stored table has records")
        };
        let at = records.start + i * RECORD_LEN;
        let field = |n: usize| {
            u32::from_le_bytes(buffer[at + n * 4..at + n * 4 + 4].try_into().unwrap()) as usize
        };
        (field(0)..field(0) + field(1), field(2)..field(2) + field(3))
    }

    /// The bytes of name `i`.
    fn bytes(&self, i: usize) -> &[u8] {
        match self {
            Keys::Stored { buffer, .. } => {
                let (name, _) = self.record(i);
                &buffer[name]
            }
            Keys::Built(names) => names[i].as_str().as_bytes(),
        }
    }

    /// The name `i` as text.
    fn text(&self, i: usize) -> &str {
        match self {
            Keys::Stored { .. } => {
                std::str::from_utf8(self.bytes(i)).expect("a stored name is text")
            }
            Keys::Built(names) => names[i].as_str(),
        }
    }
}

/// How a name's text orders against a key: the key is already lowercased
/// when `fold`.
fn cmp_text(fold: bool, text: &[u8], key: &[u8]) -> Ordering {
    if fold {
        cmp_folded(text, key)
    } else {
        text.cmp(key)
    }
}

impl<V> Base<V> {
    pub(super) fn empty(fold: bool) -> Base<V> {
        Base {
            keys: Keys::Built(Vec::new()),
            slots: Box::default(),
            decode: None,
            fold,
        }
    }

    /// A base of `entries`, in memory.
    pub(super) fn built(mut entries: Vec<(Name, V)>, fold: bool) -> Base<V> {
        entries.sort_by(|a, b| {
            let key = if fold { b.0.lower() } else { b.0.as_str() };
            cmp_text(fold, a.0.as_str().as_bytes(), key.as_bytes())
        });
        let mut names = Vec::with_capacity(entries.len());
        let mut slots = Vec::with_capacity(entries.len());
        for (name, value) in entries {
            names.push(name);
            let slot = Slot::default();
            slot.value
                .set(Box::new(value))
                .unwrap_or_else(|_| unreachable!("a new slot is empty"));
            slots.push(slot);
        }
        Base {
            keys: Keys::Built(names),
            slots: slots.into_boxed_slice(),
            decode: None,
            fold,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.keys.len()
    }

    /// The position of the entry whose name (lowercased, for a base that
    /// folds) is `key`.
    pub(super) fn find(&self, key: &str) -> Option<usize> {
        let key = key.as_bytes();
        if let Keys::Stored { buffer, index, .. } = &self.keys {
            return self.find_indexed(buffer, index, key);
        }
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let middle = low + (high - low) / 2;
            match cmp_text(self.fold, self.keys.bytes(middle), key) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => return Some(middle),
            }
        }
        None
    }

    /// `find` through a stored table's hash index.
    fn find_indexed(&self, buffer: &Buffer, index: &Range<usize>, key: &[u8]) -> Option<usize> {
        let places = index.len() / 4;
        let mask = places - 1;
        let mut place = hash_key(key) & mask;
        loop {
            let at = index.start + place * 4;
            let entry = u32::from_le_bytes(buffer[at..at + 4].try_into().unwrap()) as usize;
            if entry == 0 {
                return None;
            }
            let i = entry - 1;
            if i < self.len() && eq_text(self.fold, self.keys.bytes(i), key) {
                return Some(i);
            }
            place = (place + 1) & mask;
        }
    }

    /// The declared name of entry `i`.
    pub(super) fn name(&self, i: usize) -> &Name {
        match &self.keys {
            Keys::Built(names) => &names[i],
            Keys::Stored { .. } => self.slots[i]
                .name
                .get_or_init(|| Name::from(self.keys.text(i))),
        }
    }

    /// Entry `i`, decoded the first time it is read.
    pub(super) fn value(&self, i: usize) -> &V {
        self.slots[i].value.get_or_init(|| {
            let decode = self.decode.expect("a slot built decoded holds its value");
            let Keys::Stored { buffer, .. } = &self.keys else {
                unreachable!("a slot built decoded holds its value")
            };
            let (_, entry) = self.keys.record(i);
            Box::new(decode(&buffer[entry]))
        })
    }
}

/// A value kept encoded until first read, once read back from its
/// serialized form; a value built in memory holds it decoded.
#[derive(Debug)]
pub(crate) struct Lazy<V> {
    value: OnceLock<V>,
    // The entry encoded, with what decodes it; `None` once built decoded.
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
    fn base_get(&self, key: &str) -> Option<(&Name, &V)> {
        let base = self.base.as_ref()?;
        if self.hidden.contains(key) {
            return None;
        }
        let i = base.find(key)?;
        Some((base.name(i), base.value(i)))
    }

    fn base_contains(&self, key: &str) -> bool {
        self.base
            .as_ref()
            .is_some_and(|base| base.find(key).is_some())
    }

    pub fn get(&self, name: &str) -> Option<&V> {
        with_function_key(name, |key| {
            self.entries
                .get(key)
                .or_else(|| self.base_get(key).map(|(_, value)| value))
        })
    }

    /// The value `name` finds, with its declared name.
    pub fn get_with_name(&self, name: &str) -> Option<(&Name, &V)> {
        with_function_key(name, |key| {
            self.entries
                .get_key_value(key)
                .map(|(declared, value)| (&declared.0, value))
                .or_else(|| self.base_get(key))
        })
    }

    pub fn contains_key(&self, name: &str) -> bool {
        with_function_key(name, |key| {
            self.entries.contains_key(key)
                || (self.base_contains(key) && !self.hidden.contains(key))
        })
    }

    /// Every value with its declared name.
    pub fn iter(&self) -> Iter<'_, V> {
        self.into_iter()
    }

    /// Every declared name, without reading any value.
    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        let base = self.base.iter().flat_map(|base| {
            (0..base.len())
                .map(move |i| base.name(i))
                .filter(|name| !self.hidden.contains(name.lower()))
        });
        self.entries.keys().map(|key| &key.0).chain(base)
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
        self.entries.len() + self.base.as_ref().map_or(0, |base| base.len()) - self.hidden.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The names a table finds, as a view that stays whole while the table's
/// entries change: a lookup by any case answers with the declared name.
pub(crate) struct DeclaredNames<V> {
    own: FxHashSet<Folded>,
    base: Option<Arc<Base<V>>>,
    hidden: FxHashSet<Folded>,
}

impl<V> DeclaredNames<V> {
    /// The declared name `name` finds, in any case.
    pub(crate) fn get(&self, name: &str) -> Option<&Name> {
        with_function_key(name, |key| {
            if let Some(own) = self.own.get(key) {
                return Some(&own.0);
            }
            let base = self.base.as_ref()?;
            if self.hidden.contains(key) {
                return None;
            }
            base.find(key).map(|i| base.name(i))
        })
    }
}

impl<V> NameTable<V> {
    /// A view of the table's names as they are now: reading it needs no
    /// borrow of the table, and no name of the base is read as a `Name`
    /// until it is asked for.
    pub(crate) fn declared_names(&self) -> DeclaredNames<V> {
        DeclaredNames {
            own: self.entries.keys().cloned().collect(),
            base: self.base.clone(),
            hidden: self.hidden.clone(),
        }
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
            self.hidden.insert(Folded(declared.clone()));
            self.entries.insert(Folded(declared), value);
        }
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut V> {
        with_function_key(name, move |key| {
            self.own(key);
            self.entries.get_mut(key)
        })
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
        with_function_key(name, move |key| {
            if let Some(value) = self.entries.remove(key) {
                return Some(value);
            }
            let (declared, value) = self.base_get(key)?;
            let (declared, value) = (declared.clone(), value.clone());
            self.hidden.insert(Folded(declared));
            Some(value)
        })
    }

    /// Every value with its declared name, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Name, &mut V)> {
        self.iter_mut_scoped(None)
    }

    /// The values of the lowercased names in `keys` (every value when
    /// `None`) with their declared names, mutably. Only those names are
    /// looked up: a base of many entries is not walked for a few.
    pub fn iter_mut_scoped<'a>(
        &'a mut self,
        keys: Option<&'a FxHashSet<String>>,
    ) -> impl Iterator<Item = (&'a Name, &'a mut V)> + 'a {
        match keys {
            Some(keys) => {
                for key in keys {
                    self.own(key);
                }
            }
            None => {
                let from_base: Vec<String> = match &self.base {
                    Some(base) => (0..base.len())
                        .map(|i| base.name(i))
                        .filter(|name| !self.hidden.contains(name.lower()))
                        .map(|name| name.lower().to_string())
                        .collect(),
                    None => Vec::new(),
                };
                for key in from_base {
                    self.own(&key);
                }
            }
        }
        self.entries
            .iter_mut()
            .filter(move |(name, _)| keys.is_none_or(|keys| keys.contains(name.0.lower())))
            .map(|(k, v)| (&k.0, v))
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.iter_mut().map(|(_, value)| value)
    }

    /// Makes every entry part of a shared base, for clones of the table
    /// to share.
    pub fn freeze(&mut self) {
        if self.entries.is_empty() && self.hidden.is_empty() {
            if self.base.is_none() {
                self.base = Some(Arc::new(Base::empty(true)));
            }
            return;
        }
        let mut all: Vec<(Name, V)> = Vec::with_capacity(self.len());
        if let Some(base) = self.base.take() {
            for i in 0..base.len() {
                let name = base.name(i);
                if !self.hidden.contains(name.lower()) {
                    all.push((name.clone(), base.value(i).clone()));
                }
            }
        }
        all.extend(
            std::mem::take(&mut self.entries)
                .into_iter()
                .map(|(k, v)| (k.0, v)),
        );
        self.hidden.clear();
        self.base = Some(Arc::new(Base::built(all, true)));
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
    base: Option<(&'a Base<V>, Range<usize>)>,
    hidden: &'a FxHashSet<Folded>,
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (&'a Name, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((key, value)) = self.entries.next() {
            return Some((&key.0, value));
        }
        let (base, positions) = self.base.as_mut()?;
        let base: &'a Base<V> = base;
        for i in positions {
            let name = base.name(i);
            if !self.hidden.contains(name.lower()) {
                return Some((name, base.value(i)));
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
            base: self.base.as_ref().map(|base| (&**base, 0..base.len())),
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
            for i in 0..base.len() {
                let name = base.name(i);
                if !self.hidden.contains(name.lower()) {
                    all.push((name.clone(), base.value(i).clone()));
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

impl<V: serde::Serialize> NameTable<V> {
    /// Writes the table as section `section`: a record for each entry,
    /// sorted by lowercased name, pointing at the entry's name and its
    /// encoding in the file. Names and entries equal to ones already in
    /// the file are not written again, so a table that differs from
    /// another in a few entries costs the records and those entries.
    pub(super) fn store(&self, writer: &mut Writer, section: &str) -> Result<(), String> {
        store_entries(writer, section, self.iter().collect(), true)
    }
}

impl<V: serde::de::DeserializeOwned> NameTable<V> {
    /// The table `store` wrote as section `section`: a base read in place.
    pub(super) fn load(reader: &Reader, section: &str) -> Result<NameTable<V>, String> {
        Ok(NameTable {
            entries: FxHashMap::default(),
            base: Some(Arc::new(load_base(reader, section, true)?)),
            hidden: FxHashSet::default(),
        })
    }
}

impl<V: serde::de::DeserializeOwned> Stored for NameTable<V> {
    fn load(reader: &Reader, section: &str) -> Result<NameTable<V>, String> {
        NameTable::load(reader, section)
    }
}

/// Writes `rows` as section `section`, ordered as a base that `fold`s or
/// not orders its names.
pub(super) fn store_entries<V: serde::Serialize>(
    writer: &mut Writer,
    section: &str,
    mut rows: Vec<(&Name, &V)>,
    fold: bool,
) -> Result<(), String> {
    let key = |name: &Name| {
        if fold {
            name.lower().to_string()
        } else {
            name.to_string()
        }
    };
    rows.sort_by_cached_key(|(name, _)| key(name));
    if rows.windows(2).any(|pair| key(pair[0].0) == key(pair[1].0)) {
        return Err(format!("{section} holds a name twice"));
    }
    let mut records = Vec::with_capacity(rows.len() * RECORD_LEN);
    let mut names: Vec<&str> = Vec::with_capacity(rows.len());
    for (name, value) in rows {
        names.push(if fold { name.lower() } else { name.as_str() });
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(value, &mut encoded).map_err(|e| e.to_string())?;
        let name = writer.name(name.as_str())?;
        let entry = writer.entry(&encoded)?;
        for field in [name.offset, name.len, entry.offset, entry.len] {
            records.extend_from_slice(&field.to_le_bytes());
        }
    }
    writer.section(section, &records)?;
    let mut index = vec![0u32; index_places(names.len())];
    let mask = index.len() - 1;
    for (i, name) in names.iter().enumerate() {
        let mut place = hash_key(name.as_bytes()) & mask;
        while index[place] != 0 {
            place = (place + 1) & mask;
        }
        index[place] = i as u32 + 1;
    }
    let index: Vec<u8> = index.iter().flat_map(|entry| entry.to_le_bytes()).collect();
    writer.section(&format!("{section}:index"), &index)
}

/// The base `store_entries` wrote as section `section`, read in place.
pub(super) fn load_base<V: serde::de::DeserializeOwned>(
    reader: &Reader,
    section: &str,
    fold: bool,
) -> Result<Base<V>, String> {
    let records = reader.section(section)?;
    let index = reader.section(&format!("{section}:index"))?;
    let buffer = reader.buffer().clone();
    if records.len() % RECORD_LEN != 0 {
        return Err(format!("{section} is cut in the middle of a record"));
    }
    if index.len() != index_places(records.len() / RECORD_LEN) * 4 {
        return Err(format!("{section}'s index has the wrong size"));
    }
    let keys = Keys::Stored {
        buffer,
        records,
        index,
    };
    validate(&keys, section)?;
    let slots = (0..keys.len()).map(|_| Slot::default()).collect();
    Ok(Base {
        keys,
        slots,
        decode: Some(decode_entry::<V>),
        fold,
    })
}

/// Whether the records of a stored table point inside the file: what
/// reading one needs. That the names are text, in strictly increasing order,
/// which a lookup by bisection relies on, is not checked here: the file's
/// checksum holds, so only a bug in `store_entries` could break it, and the
/// tests of the stored registry check every table (`check_order`).
fn validate(keys: &Keys, section: &str) -> Result<(), String> {
    let Keys::Stored { buffer, .. } = keys else {
        return Ok(());
    };
    for i in 0..keys.len() {
        let (name, entry) = keys.record(i);
        if name.end > buffer.len() || entry.end > buffer.len() {
            return Err(format!("{section}'s record {i} points outside the file"));
        }
    }
    Ok(())
}

#[cfg(test)]
impl<V> Base<V> {
    /// Whether the names of a stored base are text in strictly increasing
    /// order.
    pub(super) fn check_order(&self, section: &str) -> Result<(), String> {
        let Keys::Stored { buffer, .. } = &self.keys else {
            return Ok(());
        };
        let mut previous: Option<&str> = None;
        for i in 0..self.keys.len() {
            let (name, _) = self.keys.record(i);
            let text = std::str::from_utf8(&buffer[name])
                .map_err(|_| format!("{section}'s name {i} is not text"))?;
            let key = if self.fold {
                text.to_ascii_lowercase()
            } else {
                text.to_string()
            };
            if self.find(&key) != Some(i) {
                return Err(format!("{section}'s index does not find name {i}"));
            }
            if let Some(before) = previous {
                let order = if self.fold {
                    cmp_folded(before.as_bytes(), text.to_ascii_lowercase().as_bytes())
                } else {
                    before.cmp(text)
                };
                if order != Ordering::Less {
                    return Err(format!("{section}'s names are not in order at {i}"));
                }
            }
            previous = Some(text);
        }
        Ok(())
    }
}

#[cfg(test)]
impl<V> NameTable<V> {
    /// `Base::check_order` of the table's base.
    pub(super) fn check_order(&self, section: &str) -> Result<(), String> {
        self.base
            .as_ref()
            .map_or(Ok(()), |base| base.check_order(section))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(entries: &[(&str, u32)]) -> Reader {
        let mut writer = Writer::new();
        let table: NameTable<u32> = entries
            .iter()
            .map(|(name, value)| (Name::from(*name), *value))
            .collect();
        table.store(&mut writer, "t").unwrap();
        match Reader::new(writer.finish()) {
            Ok(reader) => reader,
            Err(_) => panic!("a written file reads"),
        }
    }

    #[test]
    fn a_stored_table_finds_every_name_in_any_case() {
        let names = [
            "Zeta",
            "alpha",
            "Mid\\Class",
            "ALPHABET",
            "b",
            "\u{e9}t\u{e9}",
        ];
        let entries: Vec<(&str, u32)> = names.iter().copied().zip(0..).collect();
        let reader = stored(&entries);
        let table: NameTable<u32> = NameTable::load(&reader, "t").unwrap();
        assert_eq!(table.len(), names.len());
        for (name, value) in &entries {
            assert_eq!(table.get(name), Some(value), "{name}");
            assert_eq!(table.get(&name.to_ascii_uppercase()), Some(value), "{name}");
            assert_eq!(table.get(&name.to_ascii_lowercase()), Some(value), "{name}");
            let (declared, found) = table.get_with_name(&name.to_ascii_lowercase()).unwrap();
            assert_eq!((declared.as_str(), found), (*name, value));
        }
        for missing in ["alph", "alphabets", "c", "zet", "zz", "\u{c9}t\u{c9}", ""] {
            assert!(!table.contains_key(missing), "{missing:?}");
            assert_eq!(table.get(missing), None);
        }
    }

    #[test]
    fn a_stored_table_iterates_in_lowercased_order_with_declared_spellings() {
        let reader = stored(&[("Zeta", 1), ("alpha", 2), ("Beta", 3)]);
        let table: NameTable<u32> = NameTable::load(&reader, "t").unwrap();
        let seen: Vec<(&str, u32)> = table.iter().map(|(n, v)| (n.as_str(), *v)).collect();
        assert_eq!(seen, [("alpha", 2), ("Beta", 3), ("Zeta", 1)]);
        let keys: Vec<&str> = table.keys().map(|n| n.as_str()).collect();
        assert_eq!(keys, ["alpha", "Beta", "Zeta"]);
    }

    #[test]
    fn changes_over_a_stored_base_hide_and_replace_its_entries() {
        let reader = stored(&[("One", 1), ("Two", 2), ("Three", 3)]);
        let mut table: NameTable<u32> = NameTable::load(&reader, "t").unwrap();
        let before = table.clone();
        *table.get_mut("two").unwrap() = 20;
        assert_eq!(table.remove("THREE"), Some(3));
        assert_eq!(table.insert(Name::from("Four"), 4), None);
        assert_eq!(table.insert(Name::from("ONE"), 10), Some(1));
        assert_eq!(table.len(), 3);
        assert_eq!(table.get("one"), Some(&10));
        assert_eq!(table.get("two"), Some(&20));
        assert_eq!(table.get("three"), None);
        assert_eq!(table.get("four"), Some(&4));
        let mut seen: Vec<(String, u32)> = table.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        seen.sort();
        assert_eq!(
            seen,
            [("Four".into(), 4), ("ONE".into(), 10), ("Two".into(), 20)]
        );
        // The clone taken before shares the base and sees none of it.
        assert_eq!(before.get("two"), Some(&2));
        assert_eq!(before.get("three"), Some(&3));
        assert_eq!(before.len(), 3);
    }

    #[test]
    fn declared_names_answer_in_any_case_with_the_table_as_it_was() {
        let reader = stored(&[("One", 1), ("Two", 2), ("Three", 3)]);
        let mut table: NameTable<u32> = NameTable::load(&reader, "t").unwrap();
        table.remove("TWO");
        table.insert(Name::from("FOUR"), 4);
        table.insert(Name::from("one"), 10);
        let names = table.declared_names();
        let declared = |key: &str| names.get(key).map(|name| name.as_str().to_string());
        assert_eq!(declared("ONE").as_deref(), Some("one"));
        assert_eq!(declared("three").as_deref(), Some("Three"));
        assert_eq!(declared("Four").as_deref(), Some("FOUR"));
        assert_eq!(declared("two"), None);
        assert_eq!(declared("six"), None);
        // A change after the view was taken is not in it.
        table.insert(Name::from("Six"), 6);
        table.remove("three");
        assert_eq!(declared("six"), None);
        assert_eq!(declared("three").as_deref(), Some("Three"));
    }

    #[test]
    fn scoped_mutation_reaches_only_the_names_asked_for() {
        let reader = stored(&[("One", 1), ("Two", 2), ("Three", 3)]);
        let mut table: NameTable<u32> = NameTable::load(&reader, "t").unwrap();
        let before = table.clone();
        let keys: FxHashSet<String> = ["one", "three", "missing"]
            .map(String::from)
            .into_iter()
            .collect();
        let touched: Vec<String> = table
            .iter_mut_scoped(Some(&keys))
            .map(|(name, value)| {
                *value += 100;
                name.to_string()
            })
            .collect();
        assert_eq!(touched.len(), 2);
        assert_eq!(table.get("one"), Some(&101));
        assert_eq!(table.get("two"), Some(&2));
        assert_eq!(table.get("three"), Some(&103));
        assert_eq!(table.own_iter().count(), 2);
        assert_eq!(table.len(), 3);
        assert_eq!(before.get("one"), Some(&1));
        for (_, value) in table.iter_mut_scoped(None) {
            *value += 1;
        }
        assert_eq!(table.get("two"), Some(&3));
        assert_eq!(table.own_iter().count(), 3);
    }

    #[test]
    fn the_stack_key_is_the_function_key_for_every_length_and_spelling() {
        let long = format!("Vendor\\{}", "AbC".repeat(80));
        let names = [
            "",
            "lower",
            "Mixed\\Case",
            "ALLCAPS",
            "\u{c9}t\u{e9} CAPS",
            "x",
            long.as_str(),
        ];
        for name in names {
            let seen = with_function_key(name, |key| key.to_string());
            assert_eq!(seen, function_key(name).as_ref(), "{name:?}");
        }
        // A name of exactly the buffer's size, and one past it.
        for len in [127, 128, 129] {
            let name = "Ab".repeat(len).chars().take(len).collect::<String>();
            let seen = with_function_key(&name, |key| key.to_string());
            assert_eq!(seen, name.to_ascii_lowercase(), "length {len}");
        }
    }

    #[test]
    fn a_stored_table_of_many_names_finds_each_and_only_those() {
        let names: Vec<String> = (0..3000)
            .map(|i| format!("Vendor{}\\Package{}\\Class{i}", i % 17, i % 101))
            .collect();
        let entries: Vec<(&str, u32)> = names.iter().map(String::as_str).zip(0..).collect();
        let reader = stored(&entries);
        let table: NameTable<u32> = NameTable::load(&reader, "t").unwrap();
        table.check_order("t").unwrap();
        for (name, value) in &entries {
            assert_eq!(table.get(&name.to_ascii_lowercase()), Some(value), "{name}");
            assert_eq!(table.get(&name.to_ascii_uppercase()), Some(value), "{name}");
        }
        for i in 3000..3200 {
            assert_eq!(table.get(&format!("Vendor0\\Package0\\Class{i}")), None);
        }
        assert_eq!(table.get(""), None);
    }

    #[test]
    fn freezing_keeps_what_a_table_holds() {
        let reader = stored(&[("One", 1), ("Two", 2)]);
        let mut table: NameTable<u32> = NameTable::load(&reader, "t").unwrap();
        table.remove("one");
        table.insert(Name::from("Three"), 3);
        table.freeze();
        assert_eq!(table.len(), 2);
        assert_eq!(table.own_iter().count(), 0);
        assert_eq!(table.get("two"), Some(&2));
        assert_eq!(table.get("THREE"), Some(&3));
        assert_eq!(table.get("one"), None);
        let names: Vec<&str> = table.keys().map(|n| n.as_str()).collect();
        assert_eq!(names, ["Three", "Two"]);
    }

    #[test]
    fn a_table_with_unordered_names_fails_the_order_check() {
        let mut writer = Writer::new();
        let (a, b) = (writer.name("b").unwrap(), writer.name("a").unwrap());
        let entry = writer.entry(&[0]).unwrap();
        let mut records = Vec::new();
        for name in [a, b] {
            for field in [name.offset, name.len, entry.offset, entry.len] {
                records.extend_from_slice(&field.to_le_bytes());
            }
        }
        writer.section("t", &records).unwrap();
        writer.section("t:index", &[0u8; 16]).unwrap();
        let reader = match Reader::new(writer.finish()) {
            Ok(reader) => reader,
            Err(_) => panic!("a written file reads"),
        };
        let table = NameTable::<u32>::load(&reader, "t").expect("its records are in the file");
        assert!(table.check_order("t").is_err());
    }

    #[test]
    fn equal_entries_of_two_tables_are_written_once() {
        let mut writer = Writer::new();
        let one: NameTable<u32> = [(Name::from("A"), 7), (Name::from("B"), 8)]
            .into_iter()
            .collect();
        let two: NameTable<u32> = [(Name::from("A"), 7), (Name::from("B"), 9)]
            .into_iter()
            .collect();
        one.store(&mut writer, "one").unwrap();
        let written = writer.body_len();
        two.store(&mut writer, "two").unwrap();
        // Records, their hash index and one new entry; the names and the
        // other entry are the ones already written.
        assert_eq!(
            writer.body_len() - written,
            2 * RECORD_LEN + index_places(2) * 4 + 1
        );
    }
}
