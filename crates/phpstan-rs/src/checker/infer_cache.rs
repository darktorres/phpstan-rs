//! What `Checker::infer_expr` and `chain_link_type` found while one
//! outermost expression check runs, by node, so a lookup is an index and a
//! short walk, not a hash of a 32-byte key.
//!
//! An answer is for one question about one expression (its address, since
//! trees of other files share node ids) in one scope version. The nodes of
//! a file have dense ids, so each id has a slot holding the head of a chain
//! through `entries`; a slot of an earlier generation is empty, so ending a
//! check is clearing `entries`, not the slots.

use types::Type;

use super::InferQuestion;
use syntax::ast::ExprId;

const NO_ENTRY: u32 = u32::MAX;

/// The most entries an emptied cache keeps room for: one very large
/// expression does not leave its table to every later one.
const SPARE_ENTRIES: usize = 4096;

#[derive(Clone, Copy)]
struct Slot {
    generation: u32,
    head: u32,
}

const EMPTY: Slot = Slot {
    generation: 0,
    head: NO_ENTRY,
};

struct Entry {
    addr: usize,
    version: u64,
    question: InferQuestion,
    next: u32,
    ty: Type,
}

pub(crate) struct InferCache {
    generation: u32,
    slots: Vec<Slot>,
    entries: Vec<Entry>,
    // The map this replaces, kept beside it in a test build, which asserts
    // that each lookup answers as the map does.
    #[cfg(test)]
    shadow: rustc_hash::FxHashMap<(usize, ExprId, u64, InferQuestion), Type>,
}

impl Default for InferCache {
    fn default() -> Self {
        InferCache {
            generation: 1,
            slots: Vec::new(),
            entries: Vec::new(),
            #[cfg(test)]
            shadow: Default::default(),
        }
    }
}

impl InferCache {
    /// The answer to `question` about the expression at `addr`, node `id`,
    /// in scope `version`, if it was found since the last `clear`.
    pub(crate) fn get(
        &self,
        addr: usize,
        id: ExprId,
        version: u64,
        question: InferQuestion,
    ) -> Option<&Type> {
        let found = self.find(addr, id, version, question);
        #[cfg(test)]
        assert_eq!(
            found,
            self.shadow.get(&(addr, id, version, question)),
            "the infer cache answers as the map does"
        );
        found
    }

    fn find(
        &self,
        addr: usize,
        id: ExprId,
        version: u64,
        question: InferQuestion,
    ) -> Option<&Type> {
        let slot = self.slots.get(u32::from(id.into_raw()) as usize)?;
        if slot.generation != self.generation {
            return None;
        }
        let mut at = slot.head;
        while at != NO_ENTRY {
            let entry = &self.entries[at as usize];
            if entry.addr == addr && entry.version == version && entry.question == question {
                return Some(&entry.ty);
            }
            at = entry.next;
        }
        None
    }

    /// Remembers `ty` as the answer; none is held for these yet.
    pub(crate) fn insert(
        &mut self,
        addr: usize,
        id: ExprId,
        version: u64,
        question: InferQuestion,
        ty: Type,
    ) {
        let index = u32::from(id.into_raw()) as usize;
        if index >= self.slots.len() {
            self.slots.resize(index + 1 + index / 4, EMPTY);
        }
        let slot = &mut self.slots[index];
        if slot.generation != self.generation {
            *slot = Slot {
                generation: self.generation,
                head: NO_ENTRY,
            };
        }
        #[cfg(test)]
        self.shadow
            .insert((addr, id, version, question), ty.clone());
        let at = self.entries.len() as u32;
        self.entries.push(Entry {
            addr,
            version,
            question,
            next: slot.head,
            ty,
        });
        slot.head = at;
    }

    /// Forgets every answer: an outermost check ended.
    pub(crate) fn clear(&mut self) {
        if self.entries.capacity() > SPARE_ENTRIES {
            self.entries = Vec::new();
        } else {
            self.entries.clear();
        }
        #[cfg(test)]
        self.shadow.clear();
        self.generation = match self.generation.checked_add(1) {
            Some(next) => next,
            None => {
                self.slots.fill(EMPTY);
                1
            }
        };
    }
}
