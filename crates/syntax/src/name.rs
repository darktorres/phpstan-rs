//! Interned names and the interner that makes them.
//!
//! A [`Name`] is a class, function, constant, method or property name as
//! written, with its lowercase form computed once: PHP compares class,
//! function and method names case-insensitively and constants and
//! properties exactly, so a consumer picks [`Name::eq_ignore_case`] or
//! `==`. Identical text interned by one [`Interner`] shares one
//! allocation, and a `Name` is one pointer wide.
//!
//! An interner holds what it interned weakly: text nothing uses any more
//! is freed, so a long-running session doesn't accumulate the names of
//! code it no longer holds. A `Project` owns its interner;
//! [`Interner::global`] serves everything without one (the embedded
//! stubs, tests).

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex, Weak};

use rustc_hash::FxHasher;

struct NameData {
    text: Box<str>,
    /// `None` when `text` is already lowercase.
    lower: Option<Box<str>>,
}

/// An interned name; see the module documentation.
#[derive(Clone)]
pub struct Name(Arc<NameData>);

impl Name {
    fn new(text: &str) -> Name {
        let lower = text.to_ascii_lowercase();
        let lower = (lower != text).then(|| lower.into_boxed_str());
        Name(Arc::new(NameData {
            text: text.into(),
            lower,
        }))
    }

    pub fn as_str(&self) -> &str {
        &self.0.text
    }

    /// The name in ASCII lowercase, PHP's key for class, function and
    /// method names.
    pub fn lower(&self) -> &str {
        self.0.lower.as_deref().unwrap_or(&self.0.text)
    }

    /// Equal ignoring ASCII case, as PHP compares class, function and
    /// method names.
    pub fn eq_ignore_case(&self, other: &Name) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.lower() == other.lower()
    }
}

impl std::ops::Deref for Name {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0.text
    }
}

impl PartialEq for Name {
    fn eq(&self, other: &Name) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.text == other.0.text
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
        self.0.text.hash(state);
    }
}

impl std::borrow::Borrow<str> for Name {
    fn borrow(&self) -> &str {
        &self.0.text
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

/// A name serializes as its text, and deserializes into the global
/// interner.
impl serde::Serialize for Name {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.text)
    }
}

impl<'de> serde::Deserialize<'de> for Name {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Name, D::Error> {
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

impl Default for Interner {
    fn default() -> Self {
        Interner {
            shards: std::array::from_fn(|_| Mutex::new(Shard::default())),
        }
    }
}

fn text_hash(text: &str) -> u64 {
    let mut hasher = FxHasher::default();
    text.hash(&mut hasher);
    hasher.finish()
}

impl Interner {
    pub fn new() -> Interner {
        Interner::default()
    }

    /// The process-wide interner, for callers without one of their own.
    pub fn global() -> &'static Arc<Interner> {
        static GLOBAL: LazyLock<Arc<Interner>> = LazyLock::new(|| Arc::new(Interner::new()));
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
        let hash = text_hash(text);
        Name(
            self.shard(hash)
                .names
                .get_or_insert(hash, |n| &*n.text == text, || Name::new(text).0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
