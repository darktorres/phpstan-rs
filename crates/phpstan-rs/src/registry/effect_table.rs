//! Dense, `Copy` handles for the declarations and transfers the effects pass
//! passes around, so its sets and worklists hash and compare integers.
//!
//! A handle is a position in an append-only list, given out in the order
//! threads first ask for a value. Nothing may depend on the number a handle
//! holds, only on which value it stands for.

use std::sync::{Mutex, RwLock};

use rustc_hash::{FxBuildHasher, FxHashMap};
use std::hash::BuildHasher;
use syntax::name::Name;

use super::effects::{EffectId, Transfer};

const SHARDS: usize = 16;

/// A declaration's effect id, by position in an `EffectTable`; the top bit
/// says the id is a closure's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EffectIx(u32);

impl EffectIx {
    const CLOSURE: u32 = 1 << 31;

    pub(crate) fn index(self) -> usize {
        (self.0 & !Self::CLOSURE) as usize
    }

    /// Whether the id is a closure's.
    pub(crate) fn is_closure(self) -> bool {
        self.0 & Self::CLOSURE != 0
    }
}

/// A `Transfer`, by position in a `TransferTable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TransferIx(u32);

impl TransferIx {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// The effect ids met so far, each once, with the handle it stands for.
pub(crate) struct EffectTable {
    index: [Mutex<FxHashMap<EffectId, EffectIx>>; SHARDS],
    ids: boxcar::Vec<EffectId>,
}

impl Default for EffectTable {
    fn default() -> Self {
        Self {
            index: std::array::from_fn(|_| Mutex::new(FxHashMap::default())),
            ids: boxcar::Vec::new(),
        }
    }
}

impl EffectTable {
    /// The handle of `id`, given out the first time it is asked for.
    pub(crate) fn intern(&self, id: &EffectId) -> EffectIx {
        let shard = FxBuildHasher.hash_one(id) as usize % SHARDS;
        let mut map = self.index[shard]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(found) = map.get(id) {
            return *found;
        }
        let at = self.ids.push(id.clone()) as u32;
        debug_assert!(at & EffectIx::CLOSURE == 0);
        let ix = EffectIx(if id.is_closure() {
            at | EffectIx::CLOSURE
        } else {
            at
        });
        map.insert(id.clone(), ix);
        ix
    }

    /// The id `ix` stands for.
    pub(crate) fn id(&self, ix: EffectIx) -> &EffectId {
        &self.ids[ix.index()]
    }

    /// How many handles have been given out.
    pub(crate) fn len(&self) -> usize {
        self.ids.count()
    }
}

/// The transfers met so far, each once, with the handle it stands for. The
/// four that name no class have fixed handles.
pub(crate) struct TransferTable {
    others: RwLock<FxHashMap<Name, TransferIx>>,
    list: boxcar::Vec<Transfer>,
}

impl Default for TransferTable {
    fn default() -> Self {
        let list = boxcar::Vec::new();
        for fixed in [
            Transfer::Same,
            Transfer::Static,
            Transfer::Made,
            Transfer::Other(None),
        ] {
            list.push(fixed);
        }
        Self {
            others: RwLock::new(FxHashMap::default()),
            list,
        }
    }
}

impl TransferTable {
    pub(crate) const SAME: TransferIx = TransferIx(0);
    pub(crate) const STATIC: TransferIx = TransferIx(1);
    pub(crate) const MADE: TransferIx = TransferIx(2);
    pub(crate) const ANY: TransferIx = TransferIx(3);

    /// The handle of `transfer`, given out the first time it is asked for.
    pub(crate) fn intern(&self, transfer: &Transfer) -> TransferIx {
        let class = match transfer {
            Transfer::Same => return Self::SAME,
            Transfer::Static => return Self::STATIC,
            Transfer::Made => return Self::MADE,
            Transfer::Other(None) => return Self::ANY,
            Transfer::Other(Some(class)) => class,
        };
        if let Some(found) = self
            .others
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(class)
        {
            return *found;
        }
        let mut others = self
            .others
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(found) = others.get(class) {
            return *found;
        }
        let ix = TransferIx(self.list.push(transfer.clone()) as u32);
        others.insert(class.clone(), ix);
        ix
    }

    /// The transfer `ix` stands for.
    pub(crate) fn get(&self, ix: TransferIx) -> &Transfer {
        &self.list[ix.index()]
    }
}
