//! Interned names and the interner that makes them.
//!
//! A [`Name`] is a class, function, constant, method or property name as
//! written, with its lowercase form computed once: PHP compares class,
//! function and method names case-insensitively and constants and
//! properties exactly, so a consumer picks [`Name::eq_ignore_case`] or
//! `==`. Identical text interned by one [`Interner`] shares one
//! allocation, and a `Name` is one pointer wide.
//!
//! A `Project` owns its interner, so the names of a long-running session
//! are freed with it; [`Interner::global`] serves everything without one
//! (the embedded stubs, tests).

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};

use rustc_hash::{FxHashSet, FxHasher};

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

impl From<&str> for Name {
    /// Interns into the global interner.
    fn from(text: &str) -> Name {
        Interner::global().name(text)
    }
}

const SHARDS: usize = 16;

#[derive(Default)]
struct Shard {
    strs: FxHashSet<Arc<str>>,
    names: FxHashSet<Name>,
}

/// Deduplicates text and names: every call for the same text returns a
/// clone of one allocation. Sharded by hash so parallel lexing rarely
/// contends on one lock.
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

impl Interner {
    pub fn new() -> Interner {
        Interner::default()
    }

    /// The process-wide interner, for callers without one of their own.
    pub fn global() -> &'static Arc<Interner> {
        static GLOBAL: LazyLock<Arc<Interner>> = LazyLock::new(|| Arc::new(Interner::new()));
        &GLOBAL
    }

    fn shard(&self, text: &str) -> &Mutex<Shard> {
        let mut hasher = FxHasher::default();
        text.hash(&mut hasher);
        &self.shards[hasher.finish() as usize % SHARDS]
    }

    /// The shared copy of `text`.
    pub fn str(&self, text: &str) -> Arc<str> {
        // A poisoned shard only means another thread panicked while
        // holding it; the set itself is still consistent.
        let mut shard = self.shard(text).lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = shard.strs.get(text) {
            return Arc::clone(existing);
        }
        let arc: Arc<str> = Arc::from(text);
        shard.strs.insert(Arc::clone(&arc));
        arc
    }

    /// The shared [`Name`] for `text`.
    pub fn name(&self, text: &str) -> Name {
        let mut shard = self.shard(text).lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = shard.names.get(text) {
            return existing.clone();
        }
        let name = Name::new(text);
        shard.names.insert(name.clone());
        name
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
