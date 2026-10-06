//! What `Checker::infer_expr` and `chain_link_type` found while one
//! outermost expression check runs, by node, so a lookup is an index and a
//! short walk, not a hash of a 32-byte key.
//!
//! An answer is for one question about one expression in one scope version.
//! A checker runs this cache against one file's tree at a time; synthetic
//! expressions bypass it. The nodes of a file have dense ids, so each id
//! has a slot holding the head of a chain through `entries`; a slot of an
//! earlier generation is empty, so ending a check clears `entries`, not the
//! slots.

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
    shadow: rustc_hash::FxHashMap<(ExprId, u64, InferQuestion), Type>,
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
    /// The answer to `question` about node `id` in scope `version`, if it
    /// was found since the last `clear`.
    pub(crate) fn get(&self, id: ExprId, version: u64, question: InferQuestion) -> Option<&Type> {
        let found = self.find(id, version, question);
        #[cfg(test)]
        assert_eq!(
            found,
            self.shadow.get(&(id, version, question)),
            "the infer cache answers as the map does"
        );
        found
    }

    fn find(&self, id: ExprId, version: u64, question: InferQuestion) -> Option<&Type> {
        let slot = self.slots.get(u32::from(id.into_raw()) as usize)?;
        if slot.generation != self.generation {
            return None;
        }
        let mut at = slot.head;
        while at != NO_ENTRY {
            let entry = &self.entries[at as usize];
            if entry.version == version && entry.question == question {
                return Some(&entry.ty);
            }
            at = entry.next;
        }
        None
    }

    /// Remembers `ty` as the answer; none is held for these yet.
    pub(crate) fn insert(&mut self, id: ExprId, version: u64, question: InferQuestion, ty: Type) {
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
        self.shadow.insert((id, version, question), ty.clone());
        let at = self.entries.len() as u32;
        self.entries.push(Entry {
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

#[cfg(test)]
mod tests {
    use super::*;
    use la_arena::RawIdx;

    #[test]
    fn entries_are_keyed_by_expression_id_question_and_scope() {
        let mut cache = InferCache::default();
        let id = ExprId::from_raw(RawIdx::from_u32(4));
        let inferred = Type::int();
        let chain = Type::string();

        cache.insert(id, 7, InferQuestion::Type, inferred.clone());
        cache.insert(id, 7, InferQuestion::ChainLink, chain.clone());

        assert_eq!(cache.get(id, 7, InferQuestion::Type), Some(&inferred));
        assert_eq!(cache.get(id, 7, InferQuestion::ChainLink), Some(&chain));
        assert!(cache.get(id, 8, InferQuestion::Type).is_none());

        cache.clear();
        assert!(cache.get(id, 7, InferQuestion::Type).is_none());
    }
}
