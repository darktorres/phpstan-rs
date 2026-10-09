//! Results that belong to one slot of one class, held by class, so the ones
//! of a class are found without looking at the others'.

use super::class_table::ClassTable;
use super::{ClassId, MethodRef, PropertyRef};

/// A reference to a member by its class and its slot in that class.
pub(crate) trait SlotRef: Copy {
    type Slot: Copy + Eq + std::hash::Hash;

    fn index(slot: Self::Slot) -> usize;
    fn from_index(index: usize) -> Self::Slot;

    fn from_parts(class: ClassId, slot: Self::Slot) -> Self;
    fn class(self) -> ClassId;
    fn slot(self) -> Self::Slot;
}

impl SlotRef for MethodRef {
    type Slot = u16;

    fn index(slot: u16) -> usize {
        slot as usize
    }

    fn from_index(index: usize) -> u16 {
        index as u16
    }

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

    fn index(slot: u32) -> usize {
        slot as usize
    }

    fn from_index(index: usize) -> u32 {
        index as u32
    }

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

/// A table of values keyed by member reference, indexed by class and then
/// by slot, which are dense.
#[derive(Debug, Clone)]
pub(crate) struct PerClass<R: SlotRef, V> {
    classes: ClassTable<Vec<Option<V>>>,
    _member: std::marker::PhantomData<R>,
}

impl<R: SlotRef, V> Default for PerClass<R, V> {
    fn default() -> Self {
        Self {
            classes: ClassTable::default(),
            _member: std::marker::PhantomData,
        }
    }
}

impl<R: SlotRef, V> PerClass<R, V> {
    pub(crate) fn get(&self, member: &R) -> Option<&V> {
        self.classes
            .get(member.class())?
            .get(R::index(member.slot()))?
            .as_ref()
    }

    pub(crate) fn contains_key(&self, member: &R) -> bool {
        self.get(member).is_some()
    }

    fn slot_mut(&mut self, member: R) -> &mut Option<V> {
        let slots = self.classes.get_or_insert_with(member.class(), Vec::new);
        let at = R::index(member.slot());
        if slots.len() <= at {
            slots.resize_with(at + 1, || None);
        }
        &mut slots[at]
    }

    pub(crate) fn insert(&mut self, member: R, value: V) -> Option<V> {
        self.slot_mut(member).replace(value)
    }

    /// Inserts `value` unless the member has one; whether it was inserted.
    pub(crate) fn insert_if_absent(&mut self, member: R, value: V) -> bool {
        let slot = self.slot_mut(member);
        if slot.is_some() {
            return false;
        }
        *slot = Some(value);
        true
    }

    /// The entries of one class.
    pub(crate) fn of_class(&self, class: ClassId) -> impl Iterator<Item = (R, &V)> {
        self.classes
            .get(class)
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(move |(at, value)| {
                Some((R::from_parts(class, R::from_index(at)), value.as_ref()?))
            })
    }

    /// Every entry.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (R, &V)> {
        self.classes.iter().flat_map(|(class, slots)| {
            slots.iter().enumerate().filter_map(move |(at, value)| {
                Some((R::from_parts(class, R::from_index(at)), value.as_ref()?))
            })
        })
    }

    pub(crate) fn clear(&mut self) {
        self.classes.clear();
    }

    pub(crate) fn remove_class(&mut self, class: ClassId) {
        self.classes.remove(class);
    }

    /// Keeps the entries of the classes `keep` accepts.
    pub(crate) fn retain_classes(&mut self, keep: impl FnMut(ClassId) -> bool) {
        self.classes.retain(keep);
    }

    /// Moves each class's entries to the id `retag` gives it.
    pub(crate) fn retag_classes(&mut self, retag: impl FnMut(ClassId) -> ClassId) {
        self.classes.retag(retag);
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
