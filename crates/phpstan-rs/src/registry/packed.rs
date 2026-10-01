//! Sorted records in one byte buffer, read in place: a table of what the
//! stubs declare or reference, which a run only looks a few keys up in, is
//! stored as two buffers rather than as tens of thousands of strings and
//! small vectors each allocated when it is read.

use super::name_table::Bytes;

/// Records of bytes, each found by its position.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct Records {
    // Where each record starts in `data`, and a last entry where the last
    // one ends.
    offsets: Vec<u32>,
    data: Bytes,
}

impl Records {
    pub(super) fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    pub(super) fn get(&self, position: usize) -> &[u8] {
        &self.data.0[self.offsets[position] as usize..self.offsets[position + 1] as usize]
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
            offsets: self.offsets,
            data: Bytes(self.data),
        }
    }
}

/// A map from text to bytes: `Records` sorted by key, each a length, the
/// key and the payload, searched by bisection.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct Packed(Records);

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
        Packed(records.finish())
    }

    fn key(&self, position: usize) -> &[u8] {
        let record = self.0.get(position);
        let length = u16::from_le_bytes([record[0], record[1]]) as usize;
        &record[2..2 + length]
    }

    /// What `key` maps to.
    pub(super) fn get(&self, key: &str) -> Option<&[u8]> {
        let (mut low, mut high) = (0, self.0.len());
        while low < high {
            let middle = low + (high - low) / 2;
            if self.key(middle) < key.as_bytes() {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low == self.0.len() || self.key(low) != key.as_bytes() {
            return None;
        }
        let record = self.0.get(low);
        let length = u16::from_le_bytes([record[0], record[1]]) as usize;
        Some(&record[2 + length..])
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
    fn an_empty_map_finds_nothing() {
        assert!(!Packed::default().contains(""));
        assert_eq!(Packed::build(Vec::new()).get("a"), None);
    }

    #[test]
    fn a_map_survives_being_stored() {
        let map = packed(&["one", "two", "three"]);
        let mut stored = Vec::new();
        ciborium::ser::into_writer(&map, &mut stored).unwrap();
        let back: Packed = ciborium::de::from_reader(&stored[..]).unwrap();
        assert_eq!(back.get("two"), map.get("two"));
        assert!(!back.contains("four"));
    }
}
