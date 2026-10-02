//! Sorted records in one byte buffer, read in place: a table of what the
//! stubs declare or reference, which a run only looks a few keys up in, is
//! stored as two buffers rather than as tens of thousands of strings and
//! small vectors each allocated when it is read.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use super::container::{Reader, Slice, Writer};

/// Records of bytes, each found by its position.
#[derive(Debug, Clone, Default)]
pub(super) struct Records {
    // Where each record starts in `data`, as little-endian `u32`s, and a
    // last entry where the last one ends.
    offsets: Slice,
    data: Slice,
}

impl Records {
    pub(super) fn len(&self) -> usize {
        (self.offsets.len() / 4).saturating_sub(1)
    }

    fn offset(&self, position: usize) -> usize {
        let at = position * 4;
        u32::from_le_bytes(self.offsets[at..at + 4].try_into().unwrap()) as usize
    }

    pub(super) fn get(&self, position: usize) -> &[u8] {
        &self.data[self.offset(position)..self.offset(position + 1)]
    }

    /// Writes the records as the sections `<section>:offsets` and
    /// `<section>:data`.
    pub(super) fn store(&self, writer: &mut Writer, section: &str) -> Result<(), String> {
        writer.section(&format!("{section}:offsets"), &self.offsets)?;
        writer.section(&format!("{section}:data"), &self.data)
    }

    /// The records `store` wrote, read in place.
    pub(super) fn load(reader: &Reader, section: &str) -> Result<Records, String> {
        let records = Records {
            offsets: reader.slice(&format!("{section}:offsets"))?,
            data: reader.slice(&format!("{section}:data"))?,
        };
        if !records.offsets.len().is_multiple_of(4) {
            return Err(format!("{section}'s offsets are cut in the middle of one"));
        }
        // The last offset must end the data, or a record could point past
        // it. That every offset follows the one before is checked by the
        // tests of what is stored (`check_order`), not on every load.
        let count = records.offsets.len() / 4;
        if count > 0 && records.offset(count - 1) != records.data.len() {
            return Err(format!("{section}'s last record doesn't end its data"));
        }
        Ok(records)
    }

    /// Whether every offset follows the one before it.
    #[cfg(test)]
    pub(super) fn check_order(&self, section: &str) -> Result<(), String> {
        let mut before = 0;
        for position in 0..self.offsets.len() / 4 {
            let offset = self.offset(position);
            if offset < before {
                return Err(format!("{section}'s offset {position} is out of order"));
            }
            before = offset;
        }
        Ok(())
    }
}

/// Builds `Records`.
#[derive(Default)]
pub(super) struct RecordsBuilder {
    offsets: Vec<u32>,
    data: Vec<u8>,
}

impl RecordsBuilder {
    pub(super) fn push(&mut self, record: &[u8]) {
        self.offsets.push(self.data.len() as u32);
        self.data.extend_from_slice(record);
    }

    pub(super) fn finish(mut self) -> Records {
        self.offsets.push(self.data.len() as u32);
        Records {
            offsets: Slice::Owned(self.offsets.iter().flat_map(|o| o.to_le_bytes()).collect()),
            data: Slice::Owned(self.data),
        }
    }
}

/// A map from text to bytes: `Records` sorted by key, each a length, the
/// key and the payload, searched by bisection - or, for a map looked up
/// often enough, through a hash index made in memory.
#[derive(Debug, Clone, Default)]
pub(super) struct Packed(Records, Arc<Lookups>);

/// How a `Packed` map has been looked up so far, shared by its clones, which
/// hold the same records.
#[derive(Debug, Default)]
struct Lookups {
    count: AtomicUsize,
    index: OnceLock<Index>,
}

/// Open addressing over the positions of a map's keys: a slot holds a
/// position plus one, `0` for none. At most half the slots are used, so a
/// search always meets an empty one.
#[derive(Debug)]
struct Index {
    slots: Box<[u32]>,
    shift: u32,
}

/// The lookups after which an index is made: the cost of making it, a
/// hash of every key, is about that of this many bisections.
fn lookups_before_index(keys: usize) -> usize {
    keys / 16 + 64
}

fn hash(key: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write(key);
    hasher.finish()
}

impl Index {
    /// The index of `map`'s keys. A key present twice, which a damaged file
    /// could hold, finds its first position, as bisection does.
    fn of(map: &Packed) -> Index {
        let keys = map.0.len();
        let places = (keys * 2).next_power_of_two().max(2);
        let shift = 64 - places.trailing_zeros();
        let mut slots = vec![0u32; places].into_boxed_slice();
        let mask = places - 1;
        'keys: for position in 0..keys {
            let key = map.key(position);
            let mut at = Self::start(hash(key), shift);
            while slots[at] != 0 {
                if map.key(slots[at] as usize - 1) == key {
                    continue 'keys;
                }
                at = (at + 1) & mask;
            }
            slots[at] = position as u32 + 1;
        }
        Index { slots, shift }
    }

    fn start(hash: u64, shift: u32) -> usize {
        (hash.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> shift) as usize
    }

    fn find(&self, map: &Packed, key: &[u8]) -> Option<usize> {
        let mask = self.slots.len() - 1;
        let mut at = Self::start(hash(key), self.shift);
        loop {
            match self.slots[at] {
                0 => return None,
                slot if map.key(slot as usize - 1) == key => return Some(slot as usize - 1),
                _ => at = (at + 1) & mask,
            }
        }
    }
}

impl Packed {
    /// The map of `entries`, keys unique.
    pub(super) fn build(mut entries: Vec<(String, Vec<u8>)>) -> Packed {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut records = RecordsBuilder::default();
        for (key, payload) in &entries {
            let mut record = Vec::with_capacity(2 + key.len() + payload.len());
            record.extend_from_slice(&(key.len() as u16).to_le_bytes());
            record.extend_from_slice(key.as_bytes());
            record.extend_from_slice(payload);
            records.push(&record);
        }
        Packed(records.finish(), Arc::default())
    }

    pub(super) fn store(&self, writer: &mut Writer, section: &str) -> Result<(), String> {
        self.0.store(writer, section)
    }

    pub(super) fn load(reader: &Reader, section: &str) -> Result<Packed, String> {
        Ok(Packed(Records::load(reader, section)?, Arc::default()))
    }

    /// Whether the offsets hold together and the keys are in strictly
    /// increasing order, which `get` relies on.
    #[cfg(test)]
    pub(super) fn check_order(&self, section: &str) -> Result<(), String> {
        self.0.check_order(section)?;
        for position in 1..self.0.len() {
            if self.key(position - 1) >= self.key(position) {
                return Err(format!("{section}'s keys are not in order at {position}"));
            }
        }
        Ok(())
    }

    fn key(&self, position: usize) -> &[u8] {
        let record = self.0.get(position);
        let length = u16::from_le_bytes([record[0], record[1]]) as usize;
        &record[2..2 + length]
    }

    /// The position of `key` among the keys, by bisection.
    fn bisect(&self, key: &[u8]) -> Option<usize> {
        let (mut low, mut high) = (0, self.0.len());
        while low < high {
            let middle = low + (high - low) / 2;
            if self.key(middle) < key {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        (low < self.0.len() && self.key(low) == key).then_some(low)
    }

    /// What `key` maps to.
    pub(super) fn get(&self, key: &str) -> Option<&[u8]> {
        let key = key.as_bytes();
        let keys = self.0.len();
        let index = match self.1.index.get() {
            Some(index) => Some(index),
            None if keys > 0
                && self.1.count.fetch_add(1, Ordering::Relaxed) >= lookups_before_index(keys) =>
            {
                Some(self.1.index.get_or_init(|| Index::of(self)))
            }
            None => None,
        };
        let position = match index {
            Some(index) => index.find(self, key),
            None => self.bisect(key),
        }?;
        let record = self.0.get(position);
        let length = u16::from_le_bytes([record[0], record[1]]) as usize;
        Some(&record[2 + length..])
    }

    /// How many keys the map holds.
    #[cfg(test)]
    pub(super) fn records_len(&self) -> usize {
        self.0.len()
    }

    /// The key at `position`, as text.
    #[cfg(test)]
    pub(super) fn key_text(&self, position: usize) -> &str {
        std::str::from_utf8(self.key(position)).expect("a key is text")
    }

    pub(super) fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packed(keys: &[&str]) -> Packed {
        Packed::build(
            keys.iter()
                .enumerate()
                .map(|(i, key)| (key.to_string(), vec![i as u8; i % 4]))
                .collect(),
        )
    }

    #[test]
    fn every_key_finds_its_own_payload_and_others_find_none() {
        let keys = ["zeta", "alpha", "", "mid", "alphabet", "b", "\u{e9}t\u{e9}"];
        let map = packed(&keys);
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(map.get(key), Some(&vec![i as u8; i % 4][..]), "{key:?}");
        }
        for missing in ["alph", "alphabets", "c", "zet", "zz", "\u{e9}"] {
            assert!(!map.contains(missing), "{missing:?}");
        }
    }

    #[test]
    fn the_index_finds_what_bisection_finds() {
        let keys: Vec<String> = (0..500)
            .map(|i| format!("key{i}\\{}", "x".repeat(i % 9)))
            .chain(["".to_string(), "\u{e9}t\u{e9}".to_string()])
            .collect();
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let map = packed(&refs);
        let index = Index::of(&map);
        for key in keys
            .iter()
            .map(String::as_str)
            .chain(["key", "key1\\xx", "zzz", "key499", "\u{e9}", "keys"])
        {
            assert_eq!(
                index.find(&map, key.as_bytes()),
                map.bisect(key.as_bytes()),
                "{key:?}"
            );
        }
        // A map past its lookup threshold answers through the index.
        for key in &keys {
            for _ in 0..2 {
                assert!(map.contains(key), "{key:?}");
            }
        }
        for _ in 0..lookups_before_index(keys.len()) + 1 {
            assert!(!map.contains("absent"));
        }
        assert!(map.1.index.get().is_some());
        assert!(map.contains(&keys[7]) && !map.contains("absent"));
    }

    #[test]
    fn an_index_over_a_duplicate_key_finds_its_first_position() {
        let map = Packed(
            {
                let mut records = RecordsBuilder::default();
                for (key, payload) in [("a", 1u8), ("b", 2), ("b", 3), ("c", 4)] {
                    let mut record = (key.len() as u16).to_le_bytes().to_vec();
                    record.extend_from_slice(key.as_bytes());
                    record.push(payload);
                    records.push(&record);
                }
                records.finish()
            },
            Arc::default(),
        );
        assert_eq!(Index::of(&map).find(&map, b"b"), map.bisect(b"b"));
        assert_eq!(map.bisect(b"b"), Some(1));
    }

    #[test]
    fn an_empty_map_finds_nothing() {
        assert!(!Packed::default().contains(""));
        assert_eq!(Packed::build(Vec::new()).get("a"), None);
    }

    fn read_back(map: &Packed) -> Packed {
        let mut writer = Writer::new();
        map.store(&mut writer, "p").unwrap();
        let reader = match Reader::new(writer.finish()) {
            Ok(reader) => reader,
            Err(_) => panic!("a written file reads"),
        };
        Packed::load(&reader, "p").unwrap()
    }

    #[test]
    fn a_map_survives_being_stored() {
        let map = packed(&["one", "two", "three"]);
        let back = read_back(&map);
        assert_eq!(back.get("two"), map.get("two"));
        assert!(!back.contains("four"));
    }

    #[test]
    fn an_empty_map_survives_being_stored() {
        for map in [Packed::default(), Packed::build(Vec::new())] {
            let back = read_back(&map);
            assert!(!back.contains(""));
        }
    }

    #[test]
    fn offsets_that_do_not_hold_together_fail_the_order_check() {
        let mut writer = Writer::new();
        writer
            .section(
                "p:offsets",
                &[0u32, 5, 3]
                    .iter()
                    .flat_map(|o| o.to_le_bytes())
                    .collect::<Vec<u8>>(),
            )
            .unwrap();
        writer.section("p:data", &[0; 3]).unwrap();
        let reader = match Reader::new(writer.finish()) {
            Ok(reader) => reader,
            Err(_) => panic!("a written file reads"),
        };
        let records = Records::load(&reader, "p").expect("the last offset ends the data");
        assert!(records.check_order("p").is_err());
    }
}
