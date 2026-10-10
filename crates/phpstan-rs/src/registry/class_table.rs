//! Values held by class id, found by the id's local number instead of by
//! hashing it.

use rustc_hash::FxHashMap;

use super::{ClassId, IdSource};

/// A value for each of some classes. A class is found at its source and
/// local number; the full id, generation included, is compared on every
/// read, and an id whose place another generation holds is kept aside.
/// An id is in one place only: a dense slot, or aside while another
/// generation holds its slot.
#[derive(Debug, Clone)]
pub(crate) struct ClassTable<T> {
    dense: [Vec<Option<(ClassId, T)>>; 3],
    aside: FxHashMap<ClassId, T>,
}

impl<T> Default for ClassTable<T> {
    fn default() -> Self {
        Self {
            dense: [Vec::new(), Vec::new(), Vec::new()],
            aside: FxHashMap::default(),
        }
    }
}

fn place(id: ClassId) -> Option<(usize, usize)> {
    let source = match id.source()? {
        IdSource::Stub => 0,
        IdSource::Context => 1,
        IdSource::Project => 2,
    };
    Some((source, id.local() as usize))
}

impl<T> ClassTable<T> {
    pub(crate) fn get(&self, id: ClassId) -> Option<&T> {
        if let Some((source, local)) = place(id)
            && let Some(Some((held, value))) = self.dense[source].get(local)
            && *held == id
        {
            return Some(value);
        }
        if self.aside.is_empty() {
            return None;
        }
        self.aside.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: ClassId) -> Option<&mut T> {
        if let Some((source, local)) = place(id)
            && matches!(self.dense[source].get(local), Some(Some((held, _))) if *held == id)
        {
            return self.dense[source][local].as_mut().map(|(_, value)| value);
        }
        self.aside.get_mut(&id)
    }

    /// The value of `id`, made by `make` when it has none.
    pub(crate) fn get_or_insert_with(&mut self, id: ClassId, make: impl FnOnce() -> T) -> &mut T {
        if let Some((source, local)) = place(id) {
            let column = &mut self.dense[source];
            if column.len() <= local {
                column.resize_with(local + 1, || None);
            }
            match &column[local] {
                None => {
                    let value = match self.aside.remove(&id) {
                        Some(value) => value,
                        None => make(),
                    };
                    column[local] = Some((id, value));
                }
                Some((held, _)) if *held == id => {}
                Some(_) => return self.aside.entry(id).or_insert_with(make),
            }
            return column[local].as_mut().map(|(_, value)| value).unwrap();
        }
        self.aside.entry(id).or_insert_with(make)
    }

    /// Sets the value of `id`, and returns the one it had.
    pub(crate) fn insert(&mut self, id: ClassId, value: T) -> Option<T> {
        if let Some((source, local)) = place(id) {
            let column = &mut self.dense[source];
            if column.len() <= local {
                column.resize_with(local + 1, || None);
            }
            match &mut column[local] {
                slot @ None => {
                    *slot = Some((id, value));
                    return self.aside.remove(&id);
                }
                Some((held, old)) if *held == id => return Some(std::mem::replace(old, value)),
                Some(_) => {}
            }
        }
        self.aside.insert(id, value)
    }

    pub(crate) fn remove(&mut self, id: ClassId) -> Option<T> {
        if let Some((source, local)) = place(id)
            && matches!(self.dense[source].get(local), Some(Some((held, _))) if *held == id)
        {
            let dense = self.dense[source][local].take().map(|(_, value)| value);
            let aside = self.aside.remove(&id);
            return dense.or(aside);
        }
        self.aside.remove(&id)
    }

    pub(crate) fn clear(&mut self) {
        for column in &mut self.dense {
            column.clear();
        }
        self.aside.clear();
    }

    /// Every class and its value: by source and local number, then the ones
    /// kept aside.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (ClassId, &T)> {
        self.dense
            .iter()
            .flatten()
            .flatten()
            .map(|(id, value)| (*id, value))
            .chain(self.aside.iter().map(|(id, value)| (*id, value)))
    }

    /// Keeps the classes `keep` accepts.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(ClassId) -> bool) {
        for column in &mut self.dense {
            for slot in column.iter_mut() {
                if matches!(slot, Some((id, _)) if !keep(*id)) {
                    *slot = None;
                }
            }
        }
        self.aside.retain(|id, _| keep(*id));
    }

    /// Moves each class's value to the id `retag` gives it.
    pub(crate) fn retag(&mut self, mut retag: impl FnMut(ClassId) -> ClassId) {
        let old = std::mem::take(self);
        for column in old.dense {
            for (id, value) in column.into_iter().flatten() {
                self.insert(retag(id), value);
            }
        }
        for (id, value) in old.aside {
            self.insert(retag(id), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(local: u32, generation: u32) -> ClassId {
        ClassId::project_in_generation(local, generation)
    }

    fn entries(table: &ClassTable<u32>) -> Vec<(ClassId, u32)> {
        let mut found: Vec<_> = table.iter().map(|(id, value)| (id, *value)).collect();
        found.sort();
        found
    }

    #[test]
    fn two_generations_of_one_local_number_are_two_classes() {
        let mut table = ClassTable::default();
        assert_eq!(table.insert(id(3, 1), 10), None);
        assert_eq!(table.insert(id(3, 2), 20), None);
        assert_eq!(table.get(id(3, 1)), Some(&10));
        assert_eq!(table.get(id(3, 2)), Some(&20));
        assert_eq!(table.get(id(3, 3)), None);
        assert_eq!(entries(&table), vec![(id(3, 1), 10), (id(3, 2), 20)]);
    }

    #[test]
    fn an_id_moves_out_of_aside_when_its_slot_frees() {
        let mut table = ClassTable::default();
        table.insert(id(5, 1), 1);
        table.insert(id(5, 2), 2);
        assert_eq!(table.remove(id(5, 1)), Some(1));
        // The slot is free: the aside id takes it, once.
        assert_eq!(table.insert(id(5, 2), 3), Some(2));
        assert_eq!(entries(&table), vec![(id(5, 2), 3)]);
        assert_eq!(table.remove(id(5, 2)), Some(3));
        assert_eq!(table.get(id(5, 2)), None);
        assert!(entries(&table).is_empty());
    }

    #[test]
    fn a_lookup_that_makes_finds_the_value_kept_aside() {
        let mut table = ClassTable::default();
        table.insert(id(7, 1), 1);
        table.insert(id(7, 2), 2);
        table.remove(id(7, 1));
        assert_eq!(*table.get_or_insert_with(id(7, 2), || 99), 2);
        assert_eq!(entries(&table), vec![(id(7, 2), 2)]);
        assert_eq!(table.remove(id(7, 2)), Some(2));
        assert_eq!(table.get(id(7, 2)), None);
    }

    #[test]
    fn insert_remove_insert_remove_leaves_nothing() {
        let mut table = ClassTable::default();
        table.insert(id(2, 9), 1);
        table.remove(id(2, 8));
        table.insert(id(2, 8), 2);
        table.insert(id(2, 9), 3);
        table.remove(id(2, 8));
        table.insert(id(2, 9), 4);
        table.remove(id(2, 9));
        assert_eq!(table.get(id(2, 9)), None);
        assert!(entries(&table).is_empty());
    }

    #[test]
    fn retain_keeps_the_classes_it_accepts_dense_or_aside() {
        let mut table = ClassTable::default();
        for generation in 1..=3 {
            table.insert(id(1, generation), generation);
        }
        table.insert(id(2, 1), 7);
        table.retain(|id| id.generation() != 2);
        assert_eq!(
            entries(&table),
            vec![(id(1, 1), 1), (id(1, 3), 3), (id(2, 1), 7)]
        );
    }

    #[test]
    fn retag_moves_every_value_to_its_new_id() {
        let mut table = ClassTable::default();
        table.insert(id(1, 1), 1);
        table.insert(id(1, 2), 2);
        table.insert(id(4, 1), 4);
        table.retag(|old| id(old.local(), old.generation() + 10));
        assert_eq!(
            entries(&table),
            vec![(id(1, 11), 1), (id(1, 12), 2), (id(4, 11), 4)]
        );
        assert_eq!(table.get(id(1, 1)), None);
    }
}
