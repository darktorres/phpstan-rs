//! Results that belong to one slot of one class, held by class, so the ones
//! of a class are found without looking at the others'.

use rustc_hash::FxHashMap;

use super::{ClassId, MethodRef, PropertyRef};

/// A reference to a member by its class and its slot in that class.
pub(crate) trait SlotRef: Copy {
    type Slot: Copy + Eq + std::hash::Hash;

    fn from_parts(class: ClassId, slot: Self::Slot) -> Self;
    fn class(self) -> ClassId;
    fn slot(self) -> Self::Slot;
}

impl SlotRef for MethodRef {
    type Slot = u16;

    fn from_parts(class: ClassId, slot: u16) -> Self {
        MethodRef::new(class, slot)
    }

    fn class(self) -> ClassId {
        MethodRef::class(self)
    }

    fn slot(self) -> u16 {
        MethodRef::slot(self)
    }
}

impl SlotRef for PropertyRef {
    type Slot = u32;

    fn from_parts(class: ClassId, slot: u32) -> Self {
        PropertyRef::new(class, slot)
    }

    fn class(self) -> ClassId {
        PropertyRef::class(self)
    }

    fn slot(self) -> u32 {
        PropertyRef::slot(self)
    }
}

/// A table of values keyed by member reference, indexed by class.
#[derive(Debug, Clone)]
pub(crate) struct PerClass<R: SlotRef, V> {
    classes: FxHashMap<ClassId, FxHashMap<R::Slot, V>>,
}

impl<R: SlotRef, V> Default for PerClass<R, V> {
    fn default() -> Self {
        Self {
            classes: FxHashMap::default(),
        }
    }
}

impl<R: SlotRef, V> PerClass<R, V> {
    pub(crate) fn get(&self, member: &R) -> Option<&V> {
        self.classes.get(&member.class())?.get(&member.slot())
    }

    pub(crate) fn contains_key(&self, member: &R) -> bool {
        self.get(member).is_some()
    }

    pub(crate) fn insert(&mut self, member: R, value: V) -> Option<V> {
        self.classes
            .entry(member.class())
            .or_default()
            .insert(member.slot(), value)
    }

    /// Inserts `value` unless the member has one; whether it was inserted.
    pub(crate) fn insert_if_absent(&mut self, member: R, value: V) -> bool {
        match self
            .classes
            .entry(member.class())
            .or_default()
            .entry(member.slot())
        {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(value);
                true
            }
            std::collections::hash_map::Entry::Occupied(_) => false,
        }
    }

    /// The entries of one class.
    pub(crate) fn of_class(&self, class: ClassId) -> impl Iterator<Item = (R, &V)> {
        self.classes
            .get(&class)
            .into_iter()
            .flatten()
            .map(move |(slot, value)| (R::from_parts(class, *slot), value))
    }

    /// Every entry.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (R, &V)> {
        self.classes.iter().flat_map(|(class, slots)| {
            slots
                .iter()
                .map(move |(slot, value)| (R::from_parts(*class, *slot), value))
        })
    }

    pub(crate) fn clear(&mut self) {
        self.classes.clear();
    }

    pub(crate) fn remove_class(&mut self, class: ClassId) {
        self.classes.remove(&class);
    }

    /// Keeps the entries of the classes `keep` accepts.
    pub(crate) fn retain_classes(&mut self, mut keep: impl FnMut(ClassId) -> bool) {
        self.classes.retain(|class, _| keep(*class));
    }

    /// Moves each class's entries to the id `retag` gives it.
    pub(crate) fn retag_classes(&mut self, mut retag: impl FnMut(ClassId) -> ClassId) {
        self.classes = self
            .classes
            .drain()
            .map(|(class, slots)| (retag(class), slots))
            .collect();
    }
}

impl<R: SlotRef, V> FromIterator<(R, V)> for PerClass<R, V> {
    fn from_iter<I: IntoIterator<Item = (R, V)>>(entries: I) -> Self {
        let mut table = Self::default();
        for (member, value) in entries {
            table.insert(member, value);
        }
        table
    }
}
