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

#[cfg(test)]
mod tests {
    use super::*;

    fn id(local: u32, generation: u32) -> ClassId {
        ClassId::project_in_generation(local, generation)
    }

    fn method(local: u32, generation: u32, slot: u16) -> MethodRef {
        MethodRef::new(id(local, generation), slot)
    }

    fn entries(table: &PerClass<MethodRef, u32>) -> Vec<(MethodRef, u32)> {
        let mut found: Vec<_> = table
            .iter()
            .map(|(member, value)| (member, *value))
            .collect();
        found.sort_by_key(|(member, _)| (member.class(), member.slot()));
        found
    }

    #[test]
    fn two_generations_of_one_class_number_do_not_share_slots() {
        let mut table = PerClass::default();
        table.insert(method(3, 1, 2), 10);
        table.insert(method(3, 2, 2), 20);
        assert_eq!(table.get(&method(3, 1, 2)), Some(&10));
        assert_eq!(table.get(&method(3, 2, 2)), Some(&20));
        assert_eq!(table.get(&method(3, 3, 2)), None);
        assert_eq!(table.get(&method(3, 1, 1)), None);
    }

    #[test]
    fn a_gap_in_the_slots_holds_nothing_and_insert_if_absent_keeps_the_first() {
        let mut table = PerClass::default();
        assert!(table.insert_if_absent(method(1, 1, 4), 1));
        assert!(!table.insert_if_absent(method(1, 1, 4), 2));
        assert_eq!(table.get(&method(1, 1, 4)), Some(&1));
        assert!(!table.contains_key(&method(1, 1, 0)));
        assert_eq!(table.of_class(id(1, 1)).count(), 1);
        assert_eq!(table.insert(method(1, 1, 4), 3), Some(1));
    }

    #[test]
    fn retain_remove_and_retag_move_whole_classes() {
        let mut table = PerClass::default();
        table.insert(method(1, 1, 0), 1);
        table.insert(method(1, 1, 2), 2);
        table.insert(method(2, 1, 0), 3);
        table.insert(method(3, 1, 1), 4);

        table.retain_classes(|class| class.local() != 2);
        assert!(!table.contains_key(&method(2, 1, 0)));

        table.remove_class(id(3, 1));
        assert!(!table.contains_key(&method(3, 1, 1)));

        table.retag_classes(|old| id(old.local(), old.generation() + 1));
        assert_eq!(
            entries(&table),
            vec![(method(1, 2, 0), 1), (method(1, 2, 2), 2)]
        );
        assert_eq!(table.get(&method(1, 1, 0)), None);
    }
}
