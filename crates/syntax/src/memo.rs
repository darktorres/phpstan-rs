//! What parsing a text gave, kept by the text: the same doc comments and
//! the same written types are parsed again and again, by every check that
//! reads one.
//!
//! The memo is bounded: a shard that outgrows its limits is emptied, so a
//! session that reads many different texts does not grow it without end.
//! It is sharded by the text's hash, so threads parsing different texts do
//! not wait for one another.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap;

const SHARDS: usize = 64;
/// How many texts a shard holds before it is emptied, and how many bytes
/// of them, unless the memo is made with limits of its own.
const SHARD_ENTRIES: usize = 256;
const SHARD_BYTES: usize = 256 * 1024;

struct Shard<V> {
    entries: FxHashMap<Box<str>, Arc<V>>,
    bytes: usize,
    hits: u64,
    misses: u64,
}

impl<V> Default for Shard<V> {
    fn default() -> Self {
        Shard {
            entries: FxHashMap::default(),
            bytes: 0,
            hits: 0,
            misses: 0,
        }
    }
}

/// A bounded, sharded memo of `parse` over text.
pub struct Memo<V> {
    shards: Box<[Mutex<Shard<V>>]>,
    shard_entries: usize,
    shard_bytes: usize,
}

/// How a memo has been asked so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoStats {
    pub hits: u64,
    pub misses: u64,
}

impl<V> Memo<V> {
    pub fn new() -> Memo<V> {
        Memo::with_limits(SHARD_ENTRIES, SHARD_BYTES)
    }

    /// A memo whose shards each hold `shard_entries` texts, and
    /// `shard_bytes` bytes of them, before one is emptied.
    pub fn with_limits(shard_entries: usize, shard_bytes: usize) -> Memo<V> {
        Memo {
            shards: (0..SHARDS).map(|_| Mutex::new(Shard::default())).collect(),
            shard_entries,
            shard_bytes,
        }
    }

    /// The value `parse` gives for `raw`: the one kept for that text, else
    /// the one made now, which is kept.
    pub fn get(&self, raw: &str, parse: impl FnOnce(&str) -> V) -> Arc<V> {
        let mut hasher = rustc_hash::FxHasher::default();
        raw.hash(&mut hasher);
        let shard = &self.shards[(hasher.finish() >> 20) as usize % SHARDS];
        {
            let mut shard = lock(shard);
            if let Some(found) = shard.entries.get(raw) {
                let found = Arc::clone(found);
                shard.hits += 1;
                return found;
            }
            shard.misses += 1;
        }
        // Parsed outside the lock; a thread that parsed the same text in
        // the meantime has its value replaced, an equal one.
        let value = Arc::new(parse(raw));
        let mut shard = lock(shard);
        if shard.entries.len() >= self.shard_entries || shard.bytes + raw.len() > self.shard_bytes {
            shard.entries.clear();
            shard.bytes = 0;
        }
        shard.bytes += raw.len();
        shard.entries.insert(raw.into(), Arc::clone(&value));
        value
    }

    pub fn stats(&self) -> MemoStats {
        self.shards.iter().fold(MemoStats::default(), |sum, shard| {
            let shard = lock(shard);
            MemoStats {
                hits: sum.hits + shard.hits,
                misses: sum.misses + shard.misses,
            }
        })
    }
}

impl<V> Default for Memo<V> {
    fn default() -> Memo<V> {
        Memo::new()
    }
}

fn lock<V>(shard: &Mutex<Shard<V>>) -> std::sync::MutexGuard<'_, Shard<V>> {
    shard
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_is_parsed_once_and_then_found() {
        let memo: Memo<usize> = Memo::new();
        let mut parses = 0;
        for _ in 0..3 {
            let value = memo.get("/** a */", |raw| {
                parses += 1;
                raw.len()
            });
            assert_eq!(*value, 8);
        }
        assert_eq!(parses, 1);
        assert_eq!(memo.stats(), MemoStats { hits: 2, misses: 1 });
    }

    #[test]
    fn a_memo_past_its_limits_is_emptied_and_still_answers() {
        let memo: Memo<String> = Memo::new();
        for i in 0..SHARD_ENTRIES * SHARDS * 3 {
            let text = format!("/** doc {i} */");
            assert_eq!(*memo.get(&text, |raw| raw.to_string()), text);
        }
        for shard in memo.shards.iter() {
            let shard = lock(shard);
            assert!(shard.entries.len() <= SHARD_ENTRIES);
            assert!(shard.bytes <= SHARD_BYTES);
        }
    }
}
