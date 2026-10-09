//! Dense, `Copy` handles for the declarations and transfers the effects pass
//! passes around, so its sets and worklists hash and compare integers.
//!
//! A handle is a position in an append-only list, given out in the order
//! threads first ask for a value. Nothing may depend on the number a handle
//! holds, only on which value it stands for.

use std::sync::{Mutex, OnceLock, RwLock};

use rustc_hash::{FxBuildHasher, FxHashMap};
use std::hash::BuildHasher;
use syntax::name::Name;

use super::effects::{DeclKey, EffectId, Transfer};

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

/// Which parameters of a declaration take their argument by reference.
pub(crate) struct ByRefParams {
    by_ref: Box<[bool]>,
    names: Box<[Name]>,
    /// Whether the last parameter is variadic, and then whether it is taken
    /// by reference: it stands for every argument past the list.
    variadic: Option<bool>,
}

impl ByRefParams {
    /// From the `(name, by reference, variadic)` of each parameter.
    pub(crate) fn new<'a>(params: impl Iterator<Item = (&'a Name, bool, bool)>) -> Self {
        let (mut by_ref, mut names, mut variadic) = (Vec::new(), Vec::new(), None);
        for (name, by_reference, is_variadic) in params {
            by_ref.push(by_reference);
            names.push(name.clone());
            variadic = is_variadic.then_some(by_reference);
        }
        Self {
            by_ref: by_ref.into(),
            names: names.into(),
            variadic,
        }
    }

    /// Whether the argument at position `at` is taken by reference.
    pub(crate) fn at_position(&self, at: usize) -> bool {
        match self.by_ref.get(at) {
            Some(by_ref) => *by_ref,
            None => self.variadic.unwrap_or(false),
        }
    }

    /// Whether the argument named `name` is taken by reference.
    pub(crate) fn named(&self, name: &str) -> bool {
        match self.names.iter().position(|param| param.as_str() == name) {
            Some(at) => self.by_ref[at],
            None => self.variadic.unwrap_or(false),
        }
    }

    /// Whether any parameter is taken by reference.
    pub(crate) fn any(&self) -> bool {
        self.by_ref.iter().any(|by_ref| *by_ref)
    }
}

/// What the table keeps for a handle: its id, the declaration it was first
/// asked for, and, found once, whether that declaration is a builtin's and
/// which of its parameters are taken by reference.
struct Entry {
    id: EffectId,
    key: DeclKey,
    builtin: OnceLock<bool>,
    by_ref: OnceLock<Option<ByRefParams>>,
}

/// The effect ids met so far, each once, with the handle it stands for.
pub(crate) struct EffectTable {
    index: [Mutex<FxHashMap<EffectId, EffectIx>>; SHARDS],
    entries: boxcar::Vec<Entry>,
}

impl Default for EffectTable {
    fn default() -> Self {
        Self {
            index: std::array::from_fn(|_| Mutex::new(FxHashMap::default())),
            entries: boxcar::Vec::new(),
        }
    }
}

impl EffectTable {
    /// The handle of `id`, which `key` stands for, given out the first time
    /// it is asked for.
    pub(crate) fn intern(&self, id: &EffectId, key: &DeclKey) -> EffectIx {
        let shard = FxBuildHasher.hash_one(id) as usize % SHARDS;
        let mut map = self.index[shard]
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(found) = map.get(id) {
            return *found;
        }
        let at = self.entries.push(Entry {
            id: id.clone(),
            key: key.clone(),
            builtin: OnceLock::new(),
            by_ref: OnceLock::new(),
        }) as u32;
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
        &self.entries[ix.index()].id
    }

    /// The declaration `ix` was first asked for.
    pub(crate) fn key(&self, ix: EffectIx) -> &DeclKey {
        &self.entries[ix.index()].key
    }

    /// Whether the declaration `ix` stands for is a builtin's, from
    /// `compute` the first time it is asked.
    pub(crate) fn is_builtin(&self, ix: EffectIx, compute: impl FnOnce() -> bool) -> bool {
        *self.entries[ix.index()].builtin.get_or_init(compute)
    }

    /// The parameters of the declaration `ix` stands for taken by reference,
    /// from `compute` the first time they are asked; none when the
    /// declaration is not known.
    pub(crate) fn by_ref_params(
        &self,
        ix: EffectIx,
        compute: impl FnOnce() -> Option<ByRefParams>,
    ) -> Option<&ByRefParams> {
        self.entries[ix.index()]
            .by_ref
            .get_or_init(compute)
            .as_ref()
    }

    /// How many handles have been given out.
    pub(crate) fn len(&self) -> usize {
        self.entries.count()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn params(list: &[(&str, bool, bool)]) -> ByRefParams {
        let names: Vec<Name> = list.iter().map(|(name, ..)| Name::from(*name)).collect();
        ByRefParams::new(
            names
                .iter()
                .zip(list)
                .map(|(name, (_, by_ref, variadic))| (name, *by_ref, *variadic)),
        )
    }

    #[test]
    fn a_position_past_the_list_follows_the_variadic_parameter() {
        let sort = params(&[("array", true, false), ("flags", false, false)]);
        assert!(sort.at_position(0));
        assert!(!sort.at_position(1));
        assert!(!sort.at_position(5));
        let sscanf = params(&[("string", false, false), ("vars", true, true)]);
        assert!(!sscanf.at_position(0));
        assert!(sscanf.at_position(1));
        assert!(sscanf.at_position(7));
    }

    #[test]
    fn a_name_not_in_the_list_follows_the_variadic_parameter() {
        let sscanf = params(&[("string", false, false), ("vars", true, true)]);
        assert!(sscanf.named("vars"));
        assert!(!sscanf.named("string"));
        assert!(sscanf.named("other"));
        let sort = params(&[("array", true, false)]);
        assert!(!sort.named("other"));
    }

    #[test]
    fn a_spread_is_by_reference_when_any_parameter_is() {
        assert!(params(&[("a", false, false), ("b", true, false)]).any());
        assert!(!params(&[("a", false, false)]).any());
        assert!(!params(&[]).any());
    }
}
