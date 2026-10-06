//! Interned names and the interner that makes them.
//!
//! A [`Name`] is a class, function, constant, method or property name as
//! written, with its lowercase form computed once: PHP compares class,
//! function and method names case-insensitively and constants and
//! properties exactly, so a consumer picks [`Name::eq_ignore_case`] or
//! `==`. Identical text interned by one [`Interner`] shares one
//! allocation, and a `Name` is one pointer wide.
//!
//! The interner holds what it interned weakly: text nothing uses any more
//! is freed, so a long-running session doesn't accumulate the names of
//! code it no longer holds. There is one, [`Interner::global`]: names made
//! by two interners could be equal text and different allocations, which
//! an identity comparison cannot allow.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};

use rustc_hash::FxHasher;

/// An index into one parsed file's distinct-name table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NameId(u32);

impl NameId {
    pub(crate) fn new(index: usize) -> NameId {
        NameId(u32::try_from(index).expect("a parsed file has too many names"))
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

struct NameData {
    text: Box<str>,
    /// `None` when `text` is already lowercase.
    lower: Option<Box<str>>,
    /// `folded_hash` of the text, made once: what a table keyed by the
    /// name ignoring case files it under.
    folded_hash: u64,
    /// `text_hash` of the text, made once: what a map keyed by the name,
    /// case-sensitively, files it under.
    hash: u64,
}

/// An interned name; see the module documentation.
#[derive(Clone)]
pub struct Name(Arc<NameData>);

/// A cache writer's per-file table of interned names.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct CacheNameEncoder(Rc<RefCell<CacheNameEncoderData>>);

#[derive(Default)]
struct CacheNameEncoderData {
    indexes: HashMap<String, u32>,
    names: Vec<String>,
}

impl CacheNameEncoder {
    /// The per-file index of `text`, adding it on first use.
    pub fn index(&self, text: &str) -> Result<u32, String> {
        let mut data = self.0.borrow_mut();
        if let Some(index) = data.indexes.get(text) {
            return Ok(*index);
        }
        let index = u32::try_from(data.names.len())
            .map_err(|_| "a cache file has too many distinct names".to_string())?;
        data.names.push(text.to_owned());
        data.indexes.insert(text.to_owned(), index);
        Ok(index)
    }

    /// The names in index order, for the cache's eager name section.
    pub fn names(&self) -> Vec<String> {
        self.0.borrow().names.clone()
    }
}

/// A cache reader's names, each interned into the global `Name` table once.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct CacheNameTable {
    texts: Arc<Vec<String>>,
    names: Arc<Vec<OnceLock<Name>>>,
    // Optional opaque identity carried alongside nested lazy records. The
    // cache reader owns its meaning; syntax only preserves it in clones.
    context: Option<u64>,
}

impl CacheNameTable {
    pub fn new(texts: Vec<String>) -> CacheNameTable {
        let names = (0..texts.len()).map(|_| OnceLock::new()).collect();
        CacheNameTable {
            texts: Arc::new(texts),
            names: Arc::new(names),
            context: None,
        }
    }

    pub fn text(&self, index: u32) -> Option<&str> {
        self.texts.get(index as usize).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.texts.len()
    }

    pub fn name(&self, index: u32) -> Option<Name> {
        self.interned(index).cloned()
    }

    pub fn interned(&self, index: u32) -> Option<&Name> {
        let index = index as usize;
        let text = self.texts.get(index)?;
        Some(self.names[index].get_or_init(|| Name::from(text.as_str())))
    }

    pub fn texts(&self) -> &Arc<Vec<String>> {
        &self.texts
    }

    pub fn set_context(&mut self, context: u64) {
        self.context = Some(context);
    }

    pub fn context(&self) -> Option<u64> {
        self.context
    }
}

#[derive(Clone)]
enum CacheNameContext {
    Encode(CacheNameEncoder),
    Decode(CacheNameTable),
}

thread_local! {
    static CACHE_NAME_CONTEXT: RefCell<Option<CacheNameContext>> = const { RefCell::new(None) };
}

/// Runs serialization with the cache writer's current name table.
#[doc(hidden)]
pub fn with_cache_name_encoder<T>(encoder: &CacheNameEncoder, f: impl FnOnce() -> T) -> T {
    with_cache_name_context(CacheNameContext::Encode(encoder.clone()), f)
}

/// Runs deserialization with a cache file's name table.
#[doc(hidden)]
pub fn with_cache_name_table<T>(table: &CacheNameTable, f: impl FnOnce() -> T) -> T {
    with_cache_name_context(CacheNameContext::Decode(table.clone()), f)
}

/// The name table active while a lazy cached record is decoded.
#[doc(hidden)]
pub fn current_cache_name_table() -> Option<CacheNameTable> {
    CACHE_NAME_CONTEXT.with(|context| match context.borrow().as_ref() {
        Some(CacheNameContext::Decode(table)) => Some(table.clone()),
        _ => None,
    })
}

/// The per-file name encoder active while a nested cache value is encoded.
#[doc(hidden)]
pub fn current_cache_name_encoder() -> Option<CacheNameEncoder> {
    CACHE_NAME_CONTEXT.with(|context| match context.borrow().as_ref() {
        Some(CacheNameContext::Encode(encoder)) => Some(encoder.clone()),
        _ => None,
    })
}

fn with_cache_name_context<T>(context: CacheNameContext, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<CacheNameContext>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CACHE_NAME_CONTEXT.with(|context| *context.borrow_mut() = previous);
        }
    }

    let previous = CACHE_NAME_CONTEXT.with(|slot| slot.replace(Some(context)));
    let _restore = Restore(previous);
    f()
}

impl Name {
    /// A name of `text`, whose `text_hash` the caller has made.
    fn new_hashed(text: &str, hash: u64) -> Name {
        let lower = text.to_ascii_lowercase();
        let lower = (lower != text).then(|| lower.into_boxed_str());
        Name(Arc::new(NameData {
            text: text.into(),
            lower,
            folded_hash: folded_hash(text.as_bytes()),
            hash,
        }))
    }

    /// `text_hash` of the name's text, made when the name was.
    pub fn text_hash(&self) -> u64 {
        self.0.hash
    }

    /// `folded_hash` of the name's text, made when the name was.
    pub fn folded_hash(&self) -> u64 {
        self.0.folded_hash
    }

    pub fn as_str(&self) -> &str {
        &self.0.text
    }

    /// Whether the two are one allocation: names of the same text that an
    /// interner made while one was alive are.
    pub fn same_allocation(&self, other: &Name) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// The name in ASCII lowercase, PHP's key for class, function and
    /// method names.
    pub fn lower(&self) -> &str {
        self.0.lower.as_deref().unwrap_or(&self.0.text)
    }

    /// Equal ignoring ASCII case, as PHP compares class, function and
    /// method names.
    pub fn eq_ignore_case(&self, other: &Name) -> bool {
        self == other || self.lower() == other.lower()
    }
}

impl std::ops::Deref for Name {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0.text
    }
}

impl PartialEq for Name {
    /// Identity: the interner makes one allocation of a text for as long as
    /// any name of it lives, and nothing else makes names.
    fn eq(&self, other: &Name) -> bool {
        let same = Arc::ptr_eq(&self.0, &other.0);
        debug_assert_eq!(
            same,
            self.0.text == other.0.text,
            "two allocations of the name {:?}",
            self.0.text
        );
        same
    }
}

impl Eq for Name {}

impl PartialOrd for Name {
    fn partial_cmp(&self, other: &Name) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Name {
    /// By text, as the text's `Ord` compares.
    fn cmp(&self, other: &Name) -> std::cmp::Ordering {
        self.0.text.cmp(&other.0.text)
    }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool {
        *self.0.text == *other
    }
}

impl PartialEq<&str> for Name {
    fn eq(&self, other: &&str) -> bool {
        *self.0.text == **other
    }
}

impl PartialEq<String> for Name {
    fn eq(&self, other: &String) -> bool {
        *self.0.text == **other
    }
}

impl PartialEq<Name> for str {
    fn eq(&self, other: &Name) -> bool {
        *self == *other.0.text
    }
}

impl PartialEq<Name> for &str {
    fn eq(&self, other: &Name) -> bool {
        **self == *other.0.text
    }
}

impl PartialEq<Name> for String {
    fn eq(&self, other: &Name) -> bool {
        **self == *other.0.text
    }
}

impl AsRef<str> for Name {
    fn as_ref(&self) -> &str {
        &self.0.text
    }
}

impl Hash for Name {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.hash);
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0.text, f)
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.text)
    }
}

/// Outside the cache codec a name serializes as text. Within a cache codec
/// it is a per-file `u32` index into the cache's intern table.
impl serde::Serialize for Name {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;
        let context = CACHE_NAME_CONTEXT.with(|context| context.borrow().clone());
        if let Some(CacheNameContext::Encode(encoder)) = context {
            let index = encoder.index(&self.0.text).map_err(S::Error::custom)?;
            serializer.serialize_u32(index)
        } else {
            serializer.serialize_str(&self.0.text)
        }
    }
}

impl<'de> serde::Deserialize<'de> for Name {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Name, D::Error> {
        if let Some(CacheNameContext::Decode(table)) =
            CACHE_NAME_CONTEXT.with(|context| context.borrow().clone())
        {
            struct Index(CacheNameTable);
            impl serde::de::Visitor<'_> for Index {
                type Value = Name;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("an index into the cache's name table")
                }

                fn visit_u32<E: serde::de::Error>(self, index: u32) -> Result<Name, E> {
                    self.0.name(index).ok_or_else(|| {
                        E::custom(format!("cache name index {index} is out of bounds"))
                    })
                }
            }
            return deserializer.deserialize_u32(Index(table));
        }

        struct Text;
        impl serde::de::Visitor<'_> for Text {
            type Value = Name;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a name")
            }

            // Interned from the text where it lies, borrowed or not.
            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Name, E> {
                Ok(Name::from(text))
            }
        }
        deserializer.deserialize_str(Text)
    }
}

impl From<&str> for Name {
    /// Interns into the global interner.
    fn from(text: &str) -> Name {
        Interner::global().name(text)
    }
}

impl From<String> for Name {
    /// Interns into the global interner.
    fn from(text: String) -> Name {
        Interner::global().name(&text)
    }
}

impl From<&String> for Name {
    /// Interns into the global interner.
    fn from(text: &String) -> Name {
        Interner::global().name(text)
    }
}

impl From<&Name> for Name {
    fn from(name: &Name) -> Name {
        name.clone()
    }
}

impl From<std::borrow::Cow<'_, str>> for Name {
    /// Interns into the global interner.
    fn from(text: std::borrow::Cow<'_, str>) -> Name {
        Interner::global().name(&text)
    }
}

impl From<Arc<str>> for Name {
    /// Interns into the global interner.
    fn from(text: Arc<str>) -> Name {
        Interner::global().name(&text)
    }
}

impl From<&Arc<str>> for Name {
    /// Interns into the global interner.
    fn from(text: &Arc<str>) -> Name {
        Interner::global().name(text)
    }
}

impl From<Name> for String {
    fn from(name: Name) -> String {
        name.as_str().to_string()
    }
}

#[cfg(feature = "fuzzing")]
impl<'a> arbitrary::Arbitrary<'a> for Name {
    /// Interns into the global interner.
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Name> {
        Ok(Interner::global().name(<&str>::arbitrary(u)?))
    }
}

const SHARDS: usize = 16;

/// Weak references to interned values, by the hash of their text. A value
/// every holder dropped stays until the table next sweeps, which it does
/// once it has grown to twice what the last sweep left.
struct WeakTable<T: ?Sized> {
    entries: hashbrown::HashTable<(u64, Weak<T>)>,
    sweep_at: usize,
}

impl<T: ?Sized> Default for WeakTable<T> {
    fn default() -> Self {
        WeakTable {
            entries: hashbrown::HashTable::new(),
            sweep_at: MIN_SWEEP,
        }
    }
}

const MIN_SWEEP: usize = 1024;

impl<T: ?Sized> WeakTable<T> {
    /// The live value with text hash `hash` that `is_it` accepts, else
    /// the one `make` builds, now interned. The table is keyed by the
    /// hash it is given, which is not hashed again.
    fn get_or_insert(
        &mut self,
        hash: u64,
        is_it: impl Fn(&T) -> bool,
        make: impl FnOnce() -> Arc<T>,
    ) -> Arc<T> {
        let mut found = None;
        self.entries.find(hash, |(h, weak)| {
            *h == hash
                && match weak.upgrade() {
                    Some(value) if is_it(&value) => {
                        found = Some(value);
                        true
                    }
                    _ => false,
                }
        });
        if let Some(value) = found {
            return value;
        }
        let value = make();
        let weak = Arc::downgrade(&value);
        // A value of this text that was dropped leaves its entry, which the
        // new one takes, so a name interned and dropped over and over keeps
        // one entry and not a longer chain of dead ones to probe.
        if let Some(entry) = self
            .entries
            .find_mut(hash, |(h, old)| *h == hash && old.strong_count() == 0)
        {
            entry.1 = weak;
            return value;
        }
        self.entries.insert_unique(hash, (hash, weak), |(h, _)| *h);
        if self.entries.len() >= self.sweep_at {
            self.sweep();
        }
        value
    }

    /// Drops every entry whose value is gone.
    fn sweep(&mut self) {
        self.entries.retain(|(_, weak)| weak.strong_count() > 0);
        self.sweep_at = (self.entries.len() * 2).max(MIN_SWEEP);
    }
}

#[derive(Default)]
struct Shard {
    strs: WeakTable<str>,
    names: WeakTable<NameData>,
}

/// Deduplicates text and names: every call for the same text while an
/// earlier result is alive returns a clone of it. Sharded by hash so
/// parallel lexing rarely contends on one lock.
pub struct Interner {
    shards: [Mutex<Shard>; SHARDS],
}

/// What a [`NameMap`] is looked up by: a name, or text.
pub trait NameKey {
    /// `text_hash` of the key's text.
    fn key_hash(&self) -> u64;
    /// Whether the key's text is that of `stored`.
    fn is_key_of(&self, stored: &Name) -> bool;
}

impl NameKey for Name {
    fn key_hash(&self) -> u64 {
        self.0.hash
    }
    fn is_key_of(&self, stored: &Name) -> bool {
        self == stored
    }
}

impl NameKey for str {
    fn key_hash(&self) -> u64 {
        text_hash(self)
    }
    fn is_key_of(&self, stored: &Name) -> bool {
        *self == *stored.0.text
    }
}

impl NameKey for String {
    fn key_hash(&self) -> u64 {
        text_hash(self)
    }
    fn is_key_of(&self, stored: &Name) -> bool {
        **self == *stored.0.text
    }
}

impl<T: NameKey + ?Sized> NameKey for &T {
    fn key_hash(&self) -> u64 {
        (**self).key_hash()
    }
    fn is_key_of(&self, stored: &Name) -> bool {
        (**self).is_key_of(stored)
    }
}

/// Values by a name, as written: a lookup by a `Name` reads the hash it
/// holds, and one by text hashes the text.
#[derive(Clone)]
pub struct NameMap<V> {
    table: hashbrown::HashTable<(Name, V)>,
}

impl<V> Default for NameMap<V> {
    fn default() -> Self {
        NameMap {
            table: hashbrown::HashTable::new(),
        }
    }
}

impl<V: fmt::Debug> fmt::Debug for NameMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<V> NameMap<V> {
    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub fn clear(&mut self) {
        self.table.clear();
    }

    pub fn get_key_value<K: NameKey + ?Sized>(&self, key: &K) -> Option<(&Name, &V)> {
        self.table
            .find(key.key_hash(), |(name, _)| key.is_key_of(name))
            .map(|(name, value)| (name, value))
    }

    pub fn get<K: NameKey + ?Sized>(&self, key: &K) -> Option<&V> {
        self.get_key_value(key).map(|(_, value)| value)
    }

    pub fn get_mut<K: NameKey + ?Sized>(&mut self, key: &K) -> Option<&mut V> {
        self.table
            .find_mut(key.key_hash(), |(name, _)| key.is_key_of(name))
            .map(|(_, value)| value)
    }

    pub fn contains_key<K: NameKey + ?Sized>(&self, key: &K) -> bool {
        self.get_key_value(key).is_some()
    }

    /// Holds `value` by `name`, returning what the name held.
    pub fn insert(&mut self, name: Name, value: V) -> Option<V> {
        let hash = name.0.hash;
        if let Some(entry) = self.table.find_mut(hash, |(held, _)| name.is_key_of(held)) {
            return Some(std::mem::replace(&mut entry.1, value));
        }
        self.table
            .insert_unique(hash, (name, value), |(held, _)| held.0.hash);
        None
    }

    /// The value `name` finds, first holding `make()` by it when there is
    /// none.
    pub fn get_or_insert_with(&mut self, name: Name, make: impl FnOnce() -> V) -> &mut V {
        let hash = name.0.hash;
        if self
            .table
            .find(hash, |(held, _)| name.is_key_of(held))
            .is_none()
        {
            self.table
                .insert_unique(hash, (name.clone(), make()), |(held, _)| held.0.hash);
        }
        self.table
            .find_mut(hash, |(held, _)| name.is_key_of(held))
            .map(|(_, value)| value)
            .expect("just found or inserted")
    }

    pub fn remove_entry<K: NameKey + ?Sized>(&mut self, key: &K) -> Option<(Name, V)> {
        match self
            .table
            .find_entry(key.key_hash(), |(name, _)| key.is_key_of(name))
        {
            Ok(entry) => Some(entry.remove().0),
            Err(_) => None,
        }
    }

    pub fn remove<K: NameKey + ?Sized>(&mut self, key: &K) -> Option<V> {
        self.remove_entry(key).map(|(_, value)| value)
    }

    pub fn iter(&self) -> NameMapIter<'_, V> {
        NameMapIter(self.table.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Name, &mut V)> {
        self.table.iter_mut().map(|(name, value)| (&*name, value))
    }

    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        self.table.iter().map(|(name, _)| name)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.table.iter().map(|(_, value)| value)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.table.iter_mut().map(|(_, value)| value)
    }

    /// Keeps the entries `keep` answers yes for.
    pub fn retain(&mut self, mut keep: impl FnMut(&Name, &mut V) -> bool) {
        self.table.retain(|(name, value)| keep(name, value));
    }
}

/// Values by a name, as written, in the order their names were first
/// inserted: a name inserted again keeps its place.
#[derive(Clone)]
pub struct OrderedNameMap<V> {
    entries: Vec<(Name, V)>,
    /// Positions in `entries`, filed by each name's own hash.
    index: hashbrown::HashTable<u32>,
}

impl<V> Default for OrderedNameMap<V> {
    fn default() -> Self {
        OrderedNameMap {
            entries: Vec::new(),
            index: hashbrown::HashTable::new(),
        }
    }
}

impl<V: fmt::Debug> fmt::Debug for OrderedNameMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(name, value)| (name, value)))
            .finish()
    }
}

impl<V> OrderedNameMap<V> {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn position<K: NameKey + ?Sized>(&self, key: &K) -> Option<usize> {
        self.index
            .find(key.key_hash(), |&at| {
                key.is_key_of(&self.entries[at as usize].0)
            })
            .map(|&at| at as usize)
    }

    pub fn get<K: NameKey + ?Sized>(&self, key: &K) -> Option<&V> {
        self.position(key).map(|at| &self.entries[at].1)
    }

    /// The stable position of `key` in insertion order.
    pub fn get_index<K: NameKey + ?Sized>(&self, key: &K) -> Option<usize> {
        self.position(key)
    }

    pub fn get_mut<K: NameKey + ?Sized>(&mut self, key: &K) -> Option<&mut V> {
        self.position(key).map(|at| &mut self.entries[at].1)
    }

    /// The value at an ordered position.
    pub fn get_index_value(&self, index: usize) -> Option<&V> {
        self.entries.get(index).map(|(_, value)| value)
    }

    /// The name at an ordered position.
    pub fn get_index_name(&self, index: usize) -> Option<&Name> {
        self.entries.get(index).map(|(name, _)| name)
    }

    /// The mutable value at an ordered position. Positions remain stable
    /// when an existing entry is updated; new entries append to the end.
    pub fn get_index_mut(&mut self, index: usize) -> Option<&mut V> {
        self.entries.get_mut(index).map(|(_, value)| value)
    }

    pub fn contains_key<K: NameKey + ?Sized>(&self, key: &K) -> bool {
        self.position(key).is_some()
    }

    /// Holds `value` by `name`, returning what the name held; a name held
    /// already keeps its place.
    pub fn insert(&mut self, name: Name, value: V) -> Option<V> {
        if let Some(at) = self.position(&name) {
            return Some(std::mem::replace(&mut self.entries[at].1, value));
        }
        self.push(name, value);
        None
    }

    fn push(&mut self, name: Name, value: V) {
        let at = u32::try_from(self.entries.len()).expect("fewer than 2^32 entries");
        let entries = &self.entries;
        self.index
            .insert_unique(name.0.hash, at, |&held| entries[held as usize].0.0.hash);
        self.entries.push((name, value));
    }

    /// The value `name` finds, first holding `make()` by it when there is
    /// none.
    pub fn get_or_insert_with(&mut self, name: Name, make: impl FnOnce() -> V) -> &mut V {
        let at = match self.position(&name) {
            Some(at) => at,
            None => {
                self.push(name, make());
                self.entries.len() - 1
            }
        };
        &mut self.entries[at].1
    }

    /// Every entry, in order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&Name, &V)> {
        self.entries.iter().map(|(name, value)| (name, value))
    }

    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = (&Name, &mut V)> {
        self.entries.iter_mut().map(|(name, value)| (&*name, value))
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &Name> {
        self.entries.iter().map(|(name, _)| name)
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &V> {
        self.entries.iter().map(|(_, value)| value)
    }

    pub fn values_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut V> {
        self.entries.iter_mut().map(|(_, value)| value)
    }

    /// Puts the entries in the order of the keys `key` makes of them.
    pub fn sort_by_cached_key<K: Ord>(&mut self, mut key: impl FnMut(&Name, &V) -> K) {
        self.entries
            .sort_by_cached_key(|(name, value)| key(name, value));
        self.reindex();
    }

    fn reindex(&mut self) {
        self.index.clear();
        let entries = &self.entries;
        for (at, (name, _)) in entries.iter().enumerate() {
            self.index.insert_unique(name.0.hash, at as u32, |&held| {
                entries[held as usize].0.0.hash
            });
        }
    }
}

impl<V> std::ops::Index<&str> for OrderedNameMap<V> {
    type Output = V;

    fn index(&self, name: &str) -> &V {
        self.get(name).unwrap_or_else(|| panic!("no {name}"))
    }
}

impl<'a, V> IntoIterator for &'a OrderedNameMap<V> {
    type Item = (&'a Name, &'a V);
    type IntoIter = Box<dyn DoubleEndedIterator<Item = (&'a Name, &'a V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<V> IntoIterator for OrderedNameMap<V> {
    type Item = (Name, V);
    type IntoIter = std::vec::IntoIter<(Name, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<V> FromIterator<(Name, V)> for OrderedNameMap<V> {
    fn from_iter<I: IntoIterator<Item = (Name, V)>>(iter: I) -> Self {
        let mut map = OrderedNameMap::default();
        map.extend(iter);
        map
    }
}

impl<V> Extend<(Name, V)> for OrderedNameMap<V> {
    fn extend<I: IntoIterator<Item = (Name, V)>>(&mut self, iter: I) {
        for (name, value) in iter {
            self.insert(name, value);
        }
    }
}

impl<V: PartialEq> PartialEq for OrderedNameMap<V> {
    fn eq(&self, other: &OrderedNameMap<V>) -> bool {
        self.entries == other.entries
    }
}

impl<V: serde::Serialize> serde::Serialize for OrderedNameMap<V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.iter())
    }
}

impl<'de, V: serde::Deserialize<'de>> serde::Deserialize<'de> for OrderedNameMap<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<V>(std::marker::PhantomData<V>);
        impl<'de, V: serde::Deserialize<'de>> serde::de::Visitor<'de> for Entries<V> {
            type Value = OrderedNameMap<V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map by name")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = OrderedNameMap::default();
                while let Some((name, value)) = map.next_entry::<Name, V>()? {
                    out.insert(name, value);
                }
                Ok(out)
            }
        }
        deserializer.deserialize_map(Entries(std::marker::PhantomData))
    }
}

impl<V> std::ops::Index<&str> for NameMap<V> {
    type Output = V;

    fn index(&self, name: &str) -> &V {
        self.get(name).unwrap_or_else(|| panic!("no {name}"))
    }
}

/// The place in a [`NameMap`] a name finds or would be given.
pub struct NameEntry<'a, V> {
    map: &'a mut NameMap<V>,
    name: Name,
}

impl<'a, V> NameEntry<'a, V> {
    /// The value, first holding `make()` by the name when there is none.
    pub fn or_insert_with(self, make: impl FnOnce() -> V) -> &'a mut V {
        self.map.get_or_insert_with(self.name, make)
    }

    pub fn or_insert(self, value: V) -> &'a mut V {
        self.or_insert_with(|| value)
    }

    pub fn or_default(self) -> &'a mut V
    where
        V: Default,
    {
        self.or_insert_with(V::default)
    }
}

impl<V> NameMap<V> {
    /// The place `name` finds or would be given.
    pub fn entry(&mut self, name: Name) -> NameEntry<'_, V> {
        NameEntry { map: self, name }
    }
}

/// A [`NameMap`]'s entries, by reference.
pub struct NameMapIter<'a, V>(hashbrown::hash_table::Iter<'a, (Name, V)>);

impl<'a, V> Iterator for NameMapIter<'a, V> {
    type Item = (&'a Name, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(name, value)| (name, value))
    }
}

impl<'a, V> IntoIterator for &'a NameMap<V> {
    type Item = (&'a Name, &'a V);
    type IntoIter = NameMapIter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<V> IntoIterator for NameMap<V> {
    type Item = (Name, V);
    type IntoIter = hashbrown::hash_table::IntoIter<(Name, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.table.into_iter()
    }
}

impl<V> FromIterator<(Name, V)> for NameMap<V> {
    fn from_iter<I: IntoIterator<Item = (Name, V)>>(iter: I) -> Self {
        let mut map = NameMap::default();
        for (name, value) in iter {
            map.insert(name, value);
        }
        map
    }
}

impl<V> Extend<(Name, V)> for NameMap<V> {
    fn extend<I: IntoIterator<Item = (Name, V)>>(&mut self, iter: I) {
        for (name, value) in iter {
            self.insert(name, value);
        }
    }
}

impl<V: PartialEq> PartialEq for NameMap<V> {
    fn eq(&self, other: &NameMap<V>) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .all(|(name, value)| other.get(name).is_some_and(|v| v == value))
    }
}

impl<V: serde::Serialize> serde::Serialize for NameMap<V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (name, value) in self.iter() {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de, V: serde::Deserialize<'de>> serde::Deserialize<'de> for NameMap<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<V>(std::marker::PhantomData<V>);
        impl<'de, V: serde::Deserialize<'de>> serde::de::Visitor<'de> for Entries<V> {
            type Value = NameMap<V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map by name")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = NameMap::default();
                while let Some((name, value)) = map.next_entry::<Name, V>()? {
                    out.insert(name, value);
                }
                Ok(out)
            }
        }
        deserializer.deserialize_map(Entries(std::marker::PhantomData))
    }
}

/// Names, as written, as a set: a lookup by a `Name` reads the hash it
/// holds, and one by text hashes the text.
#[derive(Clone, Default)]
pub struct NameSet(NameMap<()>);

impl NameSet {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn contains<K: NameKey + ?Sized>(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    pub fn get<K: NameKey + ?Sized>(&self, key: &K) -> Option<&Name> {
        self.0.get_key_value(key).map(|(name, _)| name)
    }

    /// Whether `name` was not in the set.
    pub fn insert(&mut self, name: Name) -> bool {
        self.0.insert(name, ()).is_none()
    }

    pub fn remove<K: NameKey + ?Sized>(&mut self, key: &K) -> bool {
        self.0.remove(key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Name> {
        self.0.keys()
    }

    /// Keeps the names `keep` answers yes for.
    pub fn retain(&mut self, mut keep: impl FnMut(&Name) -> bool) {
        self.0.retain(|name, _| keep(name));
    }
}

impl fmt::Debug for NameSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl PartialEq for NameSet {
    fn eq(&self, other: &NameSet) -> bool {
        self.0 == other.0
    }
}

impl<'a> IntoIterator for &'a NameSet {
    type Item = &'a Name;
    type IntoIter = Box<dyn Iterator<Item = &'a Name> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl IntoIterator for NameSet {
    type Item = Name;
    type IntoIter = Box<dyn Iterator<Item = Name>>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.0.into_iter().map(|(name, ())| name))
    }
}

impl FromIterator<Name> for NameSet {
    fn from_iter<I: IntoIterator<Item = Name>>(iter: I) -> Self {
        let mut set = NameSet::default();
        set.extend(iter);
        set
    }
}

impl Extend<Name> for NameSet {
    fn extend<I: IntoIterator<Item = Name>>(&mut self, iter: I) {
        for name in iter {
            self.insert(name);
        }
    }
}

impl serde::Serialize for NameSet {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> serde::Deserialize<'de> for NameSet {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Vec::<Name>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}

/// The high bit of each byte of `word` that is an ASCII capital: in each
/// byte's low seven bits, the high bit of `+ 0x3f` says `>= 'A'` and that of
/// `+ 0x25` says `> 'Z'`; a byte with its own high bit set is not ASCII.
pub fn ascii_capitals(word: u64) -> u64 {
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let low = word & !HIGH;
    let at_least_a = low.wrapping_add(0x3f3f_3f3f_3f3f_3f3f);
    let past_z = low.wrapping_add(0x2525_2525_2525_2525);
    at_least_a & !past_z & !word & HIGH
}

/// `word` with each ASCII capital in lower case.
fn lowered(word: u64) -> u64 {
    // A capital's lowercase form has the bit `0x20` set.
    word | (ascii_capitals(word) >> 2)
}

/// `word` with the bit `0x20` set in every byte: a capital and its
/// lowercase form become one byte, as do a few other pairs that no name has
/// two of (`\` and `|`, `@` and a backquote). For hashing, where two names
/// that are one ignoring case must hash alike and the rest need only
/// mostly differ.
const fn with_case_bit(word: u64) -> u64 {
    word | 0x2020_2020_2020_2020
}

/// The last eight bytes of `text` (at least eight long) as a word.
fn last_word(text: &[u8]) -> u64 {
    let (_, tail) = text.split_at(text.len() - 8);
    u64::from_le_bytes(tail.try_into().expect("eight bytes"))
}

/// The hash of `text` ASCII-lowercased, eight bytes at a time and without
/// building the lowercase text: the hash a name files itself under in a
/// table that ignores case, so a name in any case finds the same place.
pub fn folded_hash(text: &[u8]) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mix = |hash: u64, word: u64| (hash.rotate_left(5) ^ word).wrapping_mul(K);
    let mut hash = mix(0, text.len() as u64);
    if text.len() >= 8 {
        let (words, _) = text.as_chunks::<8>();
        for word in words {
            hash = mix(hash, with_case_bit(u64::from_le_bytes(*word)));
        }
        // The bytes past the last whole word, with some of it again.
        hash = mix(hash, with_case_bit(last_word(text)));
    } else if !text.is_empty() {
        let mut word = [0u8; 8];
        word[..text.len()].copy_from_slice(text);
        hash = mix(hash, with_case_bit(u64::from_le_bytes(word)));
    }
    hash ^ (hash >> 32)
}

/// Whether `text`, lowercased, is `key`, which is already lowercase, eight
/// bytes at a time.
pub fn eq_lowercased(text: &[u8], key: &[u8]) -> bool {
    if text.len() != key.len() {
        return false;
    }
    if text.len() >= 8 {
        let (text_words, _) = text.as_chunks::<8>();
        let (key_words, _) = key.as_chunks::<8>();
        for (text, key) in text_words.iter().zip(key_words) {
            if lowered(u64::from_le_bytes(*text)) != u64::from_le_bytes(*key) {
                return false;
            }
        }
        // The bytes past the last whole word, with some of it again.
        return lowered(last_word(text)) == last_word(key);
    }
    text.iter()
        .zip(key)
        .all(|(text, key)| text.to_ascii_lowercase() == *key)
}

/// What a [`FoldedMap`] is looked up by: a name, or text.
pub trait FoldKey {
    /// `folded_hash` of the key's text.
    fn fold_hash(&self) -> u64;
    /// Whether the key and `stored` are one name ignoring ASCII case.
    fn is_folded_eq(&self, stored: &Name) -> bool;
    /// Whether the key's text is `stored` ignoring ASCII case.
    fn eq_text_ignoring_case(&self, stored: &[u8]) -> bool;
}

impl FoldKey for Name {
    fn fold_hash(&self) -> u64 {
        self.0.folded_hash
    }
    fn is_folded_eq(&self, stored: &Name) -> bool {
        self.eq_ignore_case(stored)
    }
    fn eq_text_ignoring_case(&self, stored: &[u8]) -> bool {
        self.0.text.as_bytes().eq_ignore_ascii_case(stored)
    }
}

impl FoldKey for str {
    fn fold_hash(&self) -> u64 {
        folded_hash(self.as_bytes())
    }
    fn is_folded_eq(&self, stored: &Name) -> bool {
        eq_lowercased(self.as_bytes(), stored.lower().as_bytes())
    }
    fn eq_text_ignoring_case(&self, stored: &[u8]) -> bool {
        self.as_bytes().eq_ignore_ascii_case(stored)
    }
}

impl FoldKey for String {
    fn fold_hash(&self) -> u64 {
        self.as_str().fold_hash()
    }
    fn is_folded_eq(&self, stored: &Name) -> bool {
        self.as_str().is_folded_eq(stored)
    }
    fn eq_text_ignoring_case(&self, stored: &[u8]) -> bool {
        self.as_str().eq_text_ignoring_case(stored)
    }
}

impl<T: FoldKey + ?Sized> FoldKey for &T {
    fn fold_hash(&self) -> u64 {
        (**self).fold_hash()
    }
    fn is_folded_eq(&self, stored: &Name) -> bool {
        (**self).is_folded_eq(stored)
    }
    fn eq_text_ignoring_case(&self, stored: &[u8]) -> bool {
        (**self).eq_text_ignoring_case(stored)
    }
}

/// Values by a name that ignores ASCII case: an entry keeps the spelling it
/// was inserted with, and a lookup by a `Name` reads the hash the name
/// holds, while one by text hashes it as it is, in any case.
#[derive(Clone)]
pub struct FoldedMap<V> {
    table: hashbrown::HashTable<(Name, V)>,
}

impl<V> Default for FoldedMap<V> {
    fn default() -> Self {
        FoldedMap {
            table: hashbrown::HashTable::new(),
        }
    }
}

impl<V: fmt::Debug> fmt::Debug for FoldedMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<V> FoldedMap<V> {
    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    pub fn clear(&mut self) {
        self.table.clear();
    }

    fn hash_of(name: &Name) -> u64 {
        name.0.folded_hash
    }

    /// The entry `key` finds, with the name it was inserted by.
    pub fn get_key_value<K: FoldKey + ?Sized>(&self, key: &K) -> Option<(&Name, &V)> {
        self.table
            .find(key.fold_hash(), |(name, _)| key.is_folded_eq(name))
            .map(|(name, value)| (name, value))
    }

    pub fn get<K: FoldKey + ?Sized>(&self, key: &K) -> Option<&V> {
        self.get_key_value(key).map(|(_, value)| value)
    }

    pub fn get_mut<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<&mut V> {
        self.table
            .find_mut(key.fold_hash(), |(name, _)| key.is_folded_eq(name))
            .map(|(_, value)| value)
    }

    pub fn contains_key<K: FoldKey + ?Sized>(&self, key: &K) -> bool {
        self.get_key_value(key).is_some()
    }

    /// Holds `value` by `name`, in place of the entry its spelling in any
    /// case had, whose value it returns.
    pub fn insert(&mut self, name: Name, value: V) -> Option<V> {
        let hash = Self::hash_of(&name);
        if let Some(entry) = self
            .table
            .find_mut(hash, |(held, _)| name.eq_ignore_case(held))
        {
            return Some(std::mem::replace(entry, (name, value)).1);
        }
        self.table
            .insert_unique(hash, (name, value), |(held, _)| Self::hash_of(held));
        None
    }

    /// Holds `value` by `name` unless a spelling of it in any case is held:
    /// whether it was held.
    pub fn insert_if_absent(&mut self, name: Name, value: V) -> bool {
        let hash = Self::hash_of(&name);
        if self
            .table
            .find(hash, |(held, _)| name.eq_ignore_case(held))
            .is_some()
        {
            return false;
        }
        self.table
            .insert_unique(hash, (name, value), |(held, _)| Self::hash_of(held));
        true
    }

    /// Removes the entry `key` finds and returns it.
    pub fn remove_entry<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<(Name, V)> {
        match self
            .table
            .find_entry(key.fold_hash(), |(name, _)| key.is_folded_eq(name))
        {
            Ok(entry) => Some(entry.remove().0),
            Err(_) => None,
        }
    }

    pub fn remove<K: FoldKey + ?Sized>(&mut self, key: &K) -> Option<V> {
        self.remove_entry(key).map(|(_, value)| value)
    }

    pub fn iter(&self) -> FoldedIter<'_, V> {
        FoldedIter(self.table.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Name, &mut V)> {
        self.table.iter_mut().map(|(name, value)| (&*name, value))
    }

    pub fn keys(&self) -> impl Iterator<Item = &Name> {
        self.table.iter().map(|(name, _)| name)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.table.iter_mut().map(|(_, value)| value)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.table.iter().map(|(_, value)| value)
    }

    /// The value `key` finds, first holding `make()` by `name` when there
    /// is none.
    pub fn get_or_insert_with(&mut self, name: Name, make: impl FnOnce() -> V) -> &mut V {
        let hash = Self::hash_of(&name);
        let found = self
            .table
            .find(hash, |(held, _)| name.eq_ignore_case(held))
            .is_some();
        if !found {
            self.table
                .insert_unique(hash, (name.clone(), make()), |(held, _)| {
                    Self::hash_of(held)
                });
        }
        self.table
            .find_mut(hash, |(held, _)| name.eq_ignore_case(held))
            .map(|(_, value)| value)
            .expect("just found or inserted")
    }

    /// Keeps the entries `keep` answers yes for.
    pub fn retain(&mut self, mut keep: impl FnMut(&Name, &mut V) -> bool) {
        self.table.retain(|(name, value)| keep(name, value));
    }
}

impl<V: PartialEq> PartialEq for FoldedMap<V> {
    /// The same names, spelled the same, holding equal values.
    fn eq(&self, other: &FoldedMap<V>) -> bool {
        self.len() == other.len()
            && self.iter().all(|(name, value)| {
                other
                    .get_key_value(name)
                    .is_some_and(|(held, v)| held.as_str() == name.as_str() && v == value)
            })
    }
}

impl<V> std::ops::Index<&str> for FoldedMap<V> {
    type Output = V;

    fn index(&self, name: &str) -> &V {
        self.get(name)
            .unwrap_or_else(|| panic!("no entry for {name}"))
    }
}

impl<'a, V> IntoIterator for &'a FoldedMap<V> {
    type Item = (&'a Name, &'a V);
    type IntoIter = FoldedIter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<V> Extend<(Name, V)> for FoldedMap<V> {
    fn extend<I: IntoIterator<Item = (Name, V)>>(&mut self, iter: I) {
        for (name, value) in iter {
            self.insert(name, value);
        }
    }
}

impl<V: serde::Serialize> serde::Serialize for FoldedMap<V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (name, value) in self.iter() {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de, V: serde::Deserialize<'de>> serde::Deserialize<'de> for FoldedMap<V> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<V>(std::marker::PhantomData<V>);
        impl<'de, V: serde::Deserialize<'de>> serde::de::Visitor<'de> for Entries<V> {
            type Value = FoldedMap<V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map by name")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut out = FoldedMap::default();
                while let Some((name, value)) = map.next_entry::<Name, V>()? {
                    out.insert(name, value);
                }
                Ok(out)
            }
        }
        deserializer.deserialize_map(Entries(std::marker::PhantomData))
    }
}

/// A [`FoldedMap`]'s entries, by reference.
pub struct FoldedIter<'a, V>(hashbrown::hash_table::Iter<'a, (Name, V)>);

impl<'a, V> Iterator for FoldedIter<'a, V> {
    type Item = (&'a Name, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(name, value)| (name, value))
    }
}

impl<V> IntoIterator for FoldedMap<V> {
    type Item = (Name, V);
    type IntoIter = hashbrown::hash_table::IntoIter<(Name, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.table.into_iter()
    }
}

impl<V> FromIterator<(Name, V)> for FoldedMap<V> {
    fn from_iter<I: IntoIterator<Item = (Name, V)>>(iter: I) -> Self {
        let mut map = FoldedMap::default();
        for (name, value) in iter {
            map.insert(name, value);
        }
        map
    }
}

/// Names that ignore ASCII case, as a set: each kept with the spelling it
/// was inserted with.
#[derive(Clone, Default, Debug)]
pub struct FoldedSet(FoldedMap<()>);

impl FoldedSet {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn contains<K: FoldKey + ?Sized>(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    pub fn get<K: FoldKey + ?Sized>(&self, key: &K) -> Option<&Name> {
        self.0.get_key_value(key).map(|(name, _)| name)
    }

    pub fn insert(&mut self, name: Name) {
        self.0.insert(name, ());
    }

    pub fn remove<K: FoldKey + ?Sized>(&mut self, key: &K) -> bool {
        self.0.remove(key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Name> {
        self.0.keys()
    }
}

impl FromIterator<Name> for FoldedSet {
    fn from_iter<I: IntoIterator<Item = Name>>(iter: I) -> Self {
        let mut set = FoldedSet::default();
        for name in iter {
            set.insert(name);
        }
        set
    }
}

/// `names` joined by `separator`.
pub fn join(names: &[Name], separator: &str) -> String {
    let mut text = String::new();
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            text.push_str(separator);
        }
        text.push_str(name);
    }
    text
}

/// The hash an [`Interner`] files `text` under.
pub fn text_hash(text: &str) -> u64 {
    let mut hasher = FxHasher::default();
    text.hash(&mut hasher);
    hasher.finish()
}

impl Interner {
    fn new() -> Interner {
        Interner {
            shards: std::array::from_fn(|_| Mutex::new(Shard::default())),
        }
    }

    /// The process-wide interner, the only one there is.
    pub fn global() -> &'static Interner {
        static GLOBAL: LazyLock<Interner> = LazyLock::new(Interner::new);
        &GLOBAL
    }

    /// The shard for text hashing to `hash`. A poisoned shard only means
    /// another thread panicked while holding it; the table itself is
    /// still consistent.
    fn shard(&self, hash: u64) -> std::sync::MutexGuard<'_, Shard> {
        // Bits the shard's table does not index by: it takes the low ones.
        self.shards[(hash >> 24) as usize % SHARDS]
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The shared copy of `text`.
    pub fn str(&self, text: &str) -> Arc<str> {
        let hash = text_hash(text);
        self.shard(hash)
            .strs
            .get_or_insert(hash, |s| s == text, || Arc::from(text))
    }

    /// The shared [`Name`] for `text`.
    pub fn name(&self, text: &str) -> Name {
        self.name_hashed(text, text_hash(text))
    }

    /// `name`, for a caller that has made `text_hash(text)` already.
    pub fn name_hashed(&self, text: &str, hash: u64) -> Name {
        Name(self.shard(hash).names.get_or_insert(
            hash,
            |n| &*n.text == text,
            || Name::new_hashed(text, hash).0,
        ))
    }
}

/// Names a parallel load has met, held by the job that parses one file
/// after another: an identifier recurs across the files of a project (a
/// class, a method, a property, a variable), and a name found here is not
/// asked of the interner, whose shards are locked. It holds its names, so
/// they are not freed while it lives: it is made for a load and dropped
/// with it, and for one interner, the one its names come from.
#[derive(Default)]
pub struct NameCache {
    table: hashbrown::HashTable<(u64, Name)>,
}

impl NameCache {
    /// The name for `text` if this cache holds it; `hash` is
    /// `text_hash(text)`.
    pub fn find(&self, text: &str, hash: u64) -> Option<&Name> {
        self.table
            .find(hash, |(h, name)| *h == hash && name.as_str() == text)
            .map(|(_, name)| name)
    }

    /// Holds `name`, which this cache does not hold yet.
    pub fn insert(&mut self, name: Name, hash: u64) {
        self.table.insert_unique(hash, (hash, name), |(h, _)| *h);
    }

    /// How many names it holds.
    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Texts that differ in case, and in what is not ASCII, from a small
    /// alphabet so that many collide as one name ignoring case.
    fn random_texts(count: usize, seed: u64) -> Vec<String> {
        let mut state = seed;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet: Vec<char> = "aAbBzZ_\\0\u{e9}\u{c9}\u{4e2d}".chars().collect();
        (0..count)
            .map(|_| {
                let len = (next() % 20) as usize;
                (0..len)
                    .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_folded_map_is_a_map_by_the_lowercased_text() {
        let texts = random_texts(4000, 0x9e37_79b9);
        let mut map: FoldedMap<usize> = FoldedMap::default();
        let mut model: std::collections::BTreeMap<String, (String, usize)> = Default::default();
        for (i, text) in texts.iter().enumerate() {
            let name = Interner::global().name(text);
            let before = map.insert(name, i);
            let previous = model.insert(text.to_ascii_lowercase(), (text.clone(), i));
            assert_eq!(before, previous.map(|(_, v)| v), "{text}");
            if i % 7 == 0 {
                let gone = &texts[i / 2];
                assert_eq!(
                    map.remove(gone.as_str()),
                    model.remove(&gone.to_ascii_lowercase()).map(|(_, v)| v),
                    "{gone}"
                );
            }
        }
        assert_eq!(map.len(), model.len());
        for text in &texts {
            let by_text = map.get_key_value(text.as_str());
            let by_name = map.get_key_value(&Interner::global().name(text));
            let want = model.get(&text.to_ascii_lowercase());
            assert_eq!(
                by_text.map(|(n, v)| (n.to_string(), *v)),
                want.cloned(),
                "{text}"
            );
            assert_eq!(
                by_name.map(|(n, v)| (n.to_string(), *v)),
                want.cloned(),
                "{text}"
            );
            assert_eq!(map.contains_key(text), want.is_some());
        }
        let mut held: Vec<(String, usize)> = map.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        held.sort();
        let mut wanted: Vec<(String, usize)> = model.values().cloned().collect();
        wanted.sort();
        assert_eq!(held, wanted);
    }

    #[test]
    fn a_name_map_is_a_map_by_the_exact_text() {
        let texts = random_texts(4000, 0x1357_9bdf);
        let mut map: NameMap<usize> = NameMap::default();
        let mut model: std::collections::BTreeMap<String, usize> = Default::default();
        for (i, text) in texts.iter().enumerate() {
            let before = map.insert(Interner::global().name(text), i);
            assert_eq!(before, model.insert(text.clone(), i), "{text}");
            if i % 5 == 0 {
                let gone = &texts[i / 3];
                assert_eq!(map.remove(gone.as_str()), model.remove(gone), "{gone}");
            }
        }
        assert_eq!(map.len(), model.len());
        for text in &texts {
            assert_eq!(map.get(text.as_str()), model.get(text), "{text}");
            assert_eq!(
                map.get(&Interner::global().name(text)),
                model.get(text),
                "{text}"
            );
            // Another spelling is another key.
            let other = text.to_ascii_uppercase();
            assert_eq!(
                map.contains_key(other.as_str()),
                model.contains_key(&other),
                "{other}"
            );
        }
        *map.get_or_insert_with(Interner::global().name("fresh"), || 1) += 1;
        assert_eq!(map.get("fresh"), Some(&2));
        let json = serde_json::to_string(&map).expect("serializes");
        let back: NameMap<usize> = serde_json::from_str(&json).expect("reads back");
        assert!(back == map);
    }

    #[test]
    fn ordered_name_map_indices_survive_replacing_and_append_new_values() {
        let mut map = OrderedNameMap::default();
        map.insert(Interner::global().name("first"), 1);
        map.insert(Interner::global().name("second"), 2);

        assert_eq!(map.get_index("first"), Some(0));
        assert_eq!(map.get_index("second"), Some(1));
        assert_eq!(map.get_index_value(0), Some(&1));
        assert_eq!(map.get_index_value(2), None);

        *map.get_index_mut(0).unwrap() = 10;
        map.insert(Interner::global().name("first"), 11);
        assert_eq!(
            map.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
            [11, 2]
        );

        map.insert(Interner::global().name("third"), 3);
        assert_eq!(map.get_index("third"), Some(2));
        assert_eq!(map.get_index("first"), Some(0));
        assert_eq!(map.get_index_mut(2).map(|value| *value), Some(3));
        assert!(map.get_index_mut(3).is_none());
    }

    #[test]
    fn the_folded_hash_is_that_of_the_lowercased_text() {
        for text in random_texts(4000, 0x2545_f491) {
            assert_eq!(
                folded_hash(text.as_bytes()),
                folded_hash(text.to_ascii_lowercase().as_bytes()),
                "{text}"
            );
            assert!(eq_lowercased(
                text.as_bytes(),
                text.to_ascii_lowercase().as_bytes()
            ));
            assert_eq!(
                Interner::global().name(&text).folded_hash(),
                folded_hash(text.as_bytes())
            );
        }
        assert!(!eq_lowercased(b"Ab", b"ac"));
        assert!(!eq_lowercased(b"abc", b"ab"));
    }

    #[test]
    fn concurrent_interning_of_one_text_is_one_allocation() {
        let held: Vec<Name> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        (0..200)
                            .map(|i| Interner::global().name(&format!("Concurrent\\Name{}", i % 7)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().expect("interning thread"))
                .collect()
        });
        for name in &held {
            let again = Interner::global().name(name.as_str());
            assert!(name.same_allocation(&again), "{name}");
        }
    }

    #[test]
    fn identical_text_shares_one_allocation() {
        let interner = Interner::new();
        assert!(Arc::ptr_eq(&interner.str("hello"), &interner.str("hello")));
        assert!(Arc::ptr_eq(
            &interner.name("Foo").0,
            &interner.name("Foo").0
        ));
    }

    #[test]
    fn text_nothing_holds_is_freed() {
        let interner = Interner::new();
        let weak = Arc::downgrade(&interner.str("gone"));
        assert!(weak.upgrade().is_none());
        let name = Arc::downgrade(&interner.name("Gone").0);
        assert!(name.upgrade().is_none());
    }

    #[test]
    fn dropped_entries_are_swept_as_the_table_grows() {
        let interner = Interner::new();
        for i in 0..(SHARDS * MIN_SWEEP * 4) {
            interner.str(&format!("transient-{i}"));
        }
        let entries: usize = interner
            .shards
            .iter()
            .map(|s| s.lock().unwrap().strs.entries.len())
            .sum();
        assert!(entries < SHARDS * MIN_SWEEP, "{entries} entries kept");
    }

    #[test]
    fn a_name_keeps_its_spelling_and_its_lowercase_key() {
        let interner = Interner::new();
        let upper = interner.name("DateTime");
        let lower = interner.name("datetime");
        assert_eq!(upper.as_str(), "DateTime");
        assert_eq!(upper.lower(), "datetime");
        assert!(upper.eq_ignore_case(&lower));
        assert_ne!(upper, lower);
    }

    #[test]
    fn concurrent_interning_of_new_text_converges_on_one_allocation() {
        let interner = Arc::new(Interner::new());
        let results: Vec<Name> = (0..8)
            .map(|_| {
                let interner = Arc::clone(&interner);
                std::thread::spawn(move || interner.name("concurrent-name"))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        for r in &results[1..] {
            assert!(Arc::ptr_eq(&results[0].0, &r.0));
        }
    }
}
