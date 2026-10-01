//! Sorted records in one byte buffer, read in place: a table of what the
//! stubs declare or reference, which a run only looks a few keys up in, is
//! stored as two buffers rather than as tens of thousands of strings and
//! small vectors each allocated when it is read.

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
/// key and the payload, searched by bisection.
#[derive(Debug, Clone, Default)]
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

    pub(super) fn store(&self, writer: &mut Writer, section: &str) -> Result<(), String> {
        self.0.store(writer, section)
    }

    pub(super) fn load(reader: &Reader, section: &str) -> Result<Packed, String> {
        Ok(Packed(Records::load(reader, section)?))
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
