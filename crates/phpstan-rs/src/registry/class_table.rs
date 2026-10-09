//! Values held by class id, found by the id's local number instead of by
//! hashing it.

use rustc_hash::FxHashMap;

use super::{ClassId, IdSource};

/// A value for each of some classes. A class is found at its source and
/// local number; the full id, generation included, is compared on every
/// read, and an id whose place another generation holds is kept aside.
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
                None => column[local] = Some((id, make())),
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
                    return None;
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
            return self.dense[source][local].take().map(|(_, value)| value);
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
