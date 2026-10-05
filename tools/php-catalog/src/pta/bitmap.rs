//! Sets of small integers as sparse bitmaps of 128-bit blocks, each
//! distinct set stored once and named by its id, with the results of the
//! operations on them remembered until cleared.

use std::hash::Hasher;

use hashbrown::HashTable;
use rustc_hash::{FxHashMap, FxHasher};

pub(crate) type SetId = u32;

/// The empty set.
pub(crate) const EMPTY: SetId = 0;

/// A visit of a set id held outside the sets.
pub(crate) type Visit<'a> = dyn FnMut(&mut SetId) + 'a;

pub(crate) struct Sets {
    /// Each set's blocks: where they start in the arena, and how many.
    spans: Vec<(u32, u32)>,
    /// Each set's element count.
    counts: Vec<u32>,
    hashes: Vec<u64>,
    /// The arena: each block's index (its elements divided by 128) and
    /// bits.
    indexes: Vec<u32>,
    bits: Vec<u128>,
    table: HashTable<SetId>,
    unions: FxHashMap<(SetId, SetId), SetId>,
    differences: FxHashMap<(SetId, SetId), SetId>,
    intersections: FxHashMap<(SetId, SetId), SetId>,
    scratch_indexes: Vec<u32>,
    scratch_bits: Vec<u128>,
}

impl Default for Sets {
    fn default() -> Sets {
        Sets::new()
    }
}

impl Sets {
    pub(crate) fn new() -> Sets {
        let mut sets = Sets {
            spans: Vec::new(),
            counts: Vec::new(),
            hashes: Vec::new(),
            indexes: Vec::new(),
            bits: Vec::new(),
            table: HashTable::new(),
            unions: FxHashMap::default(),
            differences: FxHashMap::default(),
            intersections: FxHashMap::default(),
            scratch_indexes: Vec::new(),
            scratch_bits: Vec::new(),
        };
        let empty = sets.intern_scratch();
        debug_assert_eq!(empty, EMPTY);
        sets
    }

    fn blocks(&self, set: SetId) -> (&[u32], &[u128]) {
        let (start, len) = self.spans[set as usize];
        let range = start as usize..(start + len) as usize;
        (&self.indexes[range.clone()], &self.bits[range])
    }

    fn hash_of(indexes: &[u32], bits: &[u128]) -> u64 {
        let mut hasher = FxHasher::default();
        hasher.write_usize(indexes.len());
        for (&index, &bits) in indexes.iter().zip(bits) {
            hasher.write_u32(index);
            hasher.write_u128(bits);
        }
        hasher.finish()
    }

    /// The set the scratch blocks hold, stored where it is new.
    fn intern_scratch(&mut self) -> SetId {
        let hash = Self::hash_of(&self.scratch_indexes, &self.scratch_bits);
        let (spans, indexes, bits) = (&self.spans, &self.indexes, &self.bits);
        let (scratch_indexes, scratch_bits) = (&self.scratch_indexes, &self.scratch_bits);
        let found = self.table.find(hash, |&set| {
            let (start, len) = spans[set as usize];
            let range = start as usize..(start + len) as usize;
            indexes[range.clone()] == scratch_indexes[..] && bits[range] == scratch_bits[..]
        });
        if let Some(&set) = found {
            return set;
        }
        let set = self.spans.len() as SetId;
        let start = self.indexes.len() as u32;
        self.indexes.extend_from_slice(&self.scratch_indexes);
        self.bits.extend_from_slice(&self.scratch_bits);
        self.spans.push((start, self.scratch_indexes.len() as u32));
        self.counts
            .push(self.scratch_bits.iter().map(|b| b.count_ones()).sum());
        self.hashes.push(hash);
        let hashes = &self.hashes;
        self.table
            .insert_unique(hash, set, |&set| hashes[set as usize]);
        set
    }

    /// The set of sorted, distinct elements.
    pub(crate) fn intern_sorted(&mut self, elements: &[u32]) -> SetId {
        self.scratch_indexes.clear();
        self.scratch_bits.clear();
        for &element in elements {
            let index = element >> 7;
            let bit = 1u128 << (element & 127);
            match self.scratch_indexes.last() {
                Some(&last) if last == index => {
                    if let Some(bits) = self.scratch_bits.last_mut() {
                        *bits |= bit;
                    }
                }
                _ => {
                    self.scratch_indexes.push(index);
                    self.scratch_bits.push(bit);
                }
            }
        }
        self.intern_scratch()
    }

    /// The set of the elements, in any order and repeated.
    pub(crate) fn intern_unsorted(&mut self, elements: &mut Vec<u32>) -> SetId {
        elements.sort_unstable();
        elements.dedup();
        self.intern_sorted(elements)
    }

    pub(crate) fn singleton(&mut self, element: u32) -> SetId {
        self.intern_sorted(&[element])
    }

    pub(crate) fn insert(&mut self, set: SetId, element: u32) -> SetId {
        if self.contains(set, element) {
            return set;
        }
        let single = self.singleton(element);
        self.union(set, single)
    }

    pub(crate) fn contains(&self, set: SetId, element: u32) -> bool {
        let (indexes, bits) = self.blocks(set);
        match indexes.binary_search(&(element >> 7)) {
            Ok(at) => bits[at] & (1u128 << (element & 127)) != 0,
            Err(_) => false,
        }
    }

    pub(crate) fn len(&self, set: SetId) -> u32 {
        self.counts[set as usize]
    }

    /// The set's elements, in order, appended to `out`.
    pub(crate) fn extend_into(&self, set: SetId, out: &mut Vec<u32>) {
        let (indexes, bits) = self.blocks(set);
        for (&index, &bits) in indexes.iter().zip(bits) {
            let mut bits = bits;
            while bits != 0 {
                let bit = bits.trailing_zeros();
                out.push(index << 7 | bit);
                bits &= bits - 1;
            }
        }
    }

    pub(crate) fn elements(&self, set: SetId) -> Vec<u32> {
        let mut out = Vec::with_capacity(self.len(set) as usize);
        self.extend_into(set, &mut out);
        out
    }

    pub(crate) fn union(&mut self, a: SetId, b: SetId) -> SetId {
        if a == b || b == EMPTY {
            return a;
        }
        if a == EMPTY {
            return b;
        }
        let key = (a.min(b), a.max(b));
        if let Some(&set) = self.unions.get(&key) {
            return set;
        }
        self.scratch_indexes.clear();
        self.scratch_bits.clear();
        let (sa, la) = self.spans[a as usize];
        let (sb, lb) = self.spans[b as usize];
        let (mut i, mut j) = (sa as usize, sb as usize);
        let (ea, eb) = (i + la as usize, j + lb as usize);
        while i < ea && j < eb {
            let (ia, ib) = (self.indexes[i], self.indexes[j]);
            if ia < ib {
                self.scratch_indexes.push(ia);
                self.scratch_bits.push(self.bits[i]);
                i += 1;
            } else if ib < ia {
                self.scratch_indexes.push(ib);
                self.scratch_bits.push(self.bits[j]);
                j += 1;
            } else {
                self.scratch_indexes.push(ia);
                self.scratch_bits.push(self.bits[i] | self.bits[j]);
                i += 1;
                j += 1;
            }
        }
        while i < ea {
            self.scratch_indexes.push(self.indexes[i]);
            self.scratch_bits.push(self.bits[i]);
            i += 1;
        }
        while j < eb {
            self.scratch_indexes.push(self.indexes[j]);
            self.scratch_bits.push(self.bits[j]);
            j += 1;
        }
        let set = self.intern_scratch();
        self.unions.insert(key, set);
        set
    }

    /// The elements of `a` not in `b`.
    pub(crate) fn difference(&mut self, a: SetId, b: SetId) -> SetId {
        if a == b || a == EMPTY {
            return EMPTY;
        }
        if b == EMPTY {
            return a;
        }
        if let Some(&set) = self.differences.get(&(a, b)) {
            return set;
        }
        self.scratch_indexes.clear();
        self.scratch_bits.clear();
        let (sa, la) = self.spans[a as usize];
        let (sb, lb) = self.spans[b as usize];
        let (mut i, mut j) = (sa as usize, sb as usize);
        let (ea, eb) = (i + la as usize, j + lb as usize);
        while i < ea {
            let ia = self.indexes[i];
            while j < eb && self.indexes[j] < ia {
                j += 1;
            }
            let mut bits = self.bits[i];
            if j < eb && self.indexes[j] == ia {
                bits &= !self.bits[j];
            }
            if bits != 0 {
                self.scratch_indexes.push(ia);
                self.scratch_bits.push(bits);
            }
            i += 1;
        }
        let set = self.intern_scratch();
        self.differences.insert((a, b), set);
        set
    }

    pub(crate) fn intersection(&mut self, a: SetId, b: SetId) -> SetId {
        if a == b {
            return a;
        }
        if a == EMPTY || b == EMPTY {
            return EMPTY;
        }
        let key = (a.min(b), a.max(b));
        if let Some(&set) = self.intersections.get(&key) {
            return set;
        }
        self.scratch_indexes.clear();
        self.scratch_bits.clear();
        let (sa, la) = self.spans[a as usize];
        let (sb, lb) = self.spans[b as usize];
        let (mut i, mut j) = (sa as usize, sb as usize);
        let (ea, eb) = (i + la as usize, j + lb as usize);
        while i < ea && j < eb {
            let (ia, ib) = (self.indexes[i], self.indexes[j]);
            if ia < ib {
                i += 1;
            } else if ib < ia {
                j += 1;
            } else {
                let bits = self.bits[i] & self.bits[j];
                if bits != 0 {
                    self.scratch_indexes.push(ia);
                    self.scratch_bits.push(bits);
                }
                i += 1;
                j += 1;
            }
        }
        let set = self.intern_scratch();
        self.intersections.insert(key, set);
        set
    }

    /// How many distinct sets are stored.
    pub(crate) fn count(&self) -> usize {
        self.spans.len()
    }

    /// The bytes the stored sets take.
    pub(crate) fn arena_bytes(&self) -> usize {
        self.indexes.len() * 4
            + self.bits.len() * 16
            + self.spans.len() * (8 + 4 + 8 + 4)
            + self.table.capacity() * 5
    }

    /// How many results of operations are remembered.
    pub(crate) fn memo_len(&self) -> usize {
        self.unions.len() + self.differences.len() + self.intersections.len()
    }

    pub(crate) fn memo_bytes(&self) -> usize {
        (self.unions.capacity() + self.differences.capacity() + self.intersections.capacity()) * 13
    }

    pub(crate) fn clear_memos(&mut self) {
        self.unions = FxHashMap::default();
        self.differences = FxHashMap::default();
        self.intersections = FxHashMap::default();
    }

    /// Keeps only the sets `roots` holds, renumbered: `roots` visits every
    /// set id held, once to mark it and once to renumber it.
    pub(crate) fn compact(&mut self, roots: &mut dyn FnMut(&mut Visit<'_>)) {
        let mut live = vec![false; self.spans.len()];
        live[EMPTY as usize] = true;
        roots(&mut |set: &mut SetId| live[*set as usize] = true);
        let mut renumbered = vec![EMPTY; self.spans.len()];
        let mut fresh = Sets::new();
        for (set, &is_live) in live.iter().enumerate() {
            if !is_live || set == EMPTY as usize {
                continue;
            }
            let (indexes, bits) = self.blocks(set as SetId);
            fresh.scratch_indexes.clear();
            fresh.scratch_bits.clear();
            fresh.scratch_indexes.extend_from_slice(indexes);
            fresh.scratch_bits.extend_from_slice(bits);
            renumbered[set] = fresh.intern_scratch();
        }
        roots(&mut |set: &mut SetId| *set = renumbered[*set as usize]);
        *self = fresh;
    }
}
