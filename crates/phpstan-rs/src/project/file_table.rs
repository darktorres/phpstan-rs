//! The files a project holds, by path and by `FileId`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use rustc_hash::FxHashMap;

use super::walk::{PathHash, path_hash};
use crate::file_id::FileId;

/// A map from path to what is known of that file, which also numbers the
/// paths it has seen. Looking a file up by `FileId` hashes nothing; looking
/// it up by path hashes the path once.
///
/// A slot keeps its path after its file is removed, and a path that comes
/// back has the slot it had: a number is never given to two paths, and a
/// table keyed by `FileId` can hold the number of a file removed meanwhile.
pub(crate) struct FileTable<V> {
    /// The numbers of the slots, found by their paths' hashes. A path is held
    /// in its slot only.
    ids: hashbrown::HashTable<FileId>,
    slots: Vec<Slot<V>>,
    len: usize,
    // The file by the address of the `Arc<Path>` its declarations name it
    // by (`bind_origin`): an `Arc` kept in the slot, so its address stays
    // that file's.
    by_origin: FxHashMap<usize, FileId>,
}

struct Slot<V> {
    path: PathBuf,
    /// `path_hash(path)`.
    hash: PathHash,
    value: Option<V>,
    origins: Vec<Arc<Path>>,
}

impl<V> Default for FileTable<V> {
    fn default() -> Self {
        FileTable {
            ids: hashbrown::HashTable::new(),
            slots: Vec::new(),
            len: 0,
            by_origin: FxHashMap::default(),
        }
    }
}

impl<V> FileTable<V> {
    /// A table that has numbered `paths`, each with its hash, by its first
    /// place in the list, and the number each place got. Nothing is held yet.
    pub(crate) fn with_paths<'a>(
        paths: impl ExactSizeIterator<Item = (&'a Path, PathHash)>,
    ) -> (FileTable<V>, Vec<FileId>) {
        let mut table = FileTable::default();
        table
            .ids
            .reserve(paths.len(), |id| table.slots[id.index()].hash);
        table.slots.reserve(paths.len());
        let numbers = paths.map(|(path, hash)| table.number(path, hash)).collect();
        (table, numbers)
    }

    /// Whether any path has been numbered.
    pub(crate) fn has_numbers(&self) -> bool {
        !self.slots.is_empty()
    }

    /// The number of files held.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The number of `path`, if it has one.
    pub(crate) fn id_of(&self, path: &Path) -> Option<FileId> {
        self.find(path_hash(path), path)
    }

    /// The number of `path`, given now if it has none.
    pub(crate) fn id_for(&mut self, path: &Path) -> FileId {
        self.number(path, path_hash(path))
    }

    /// The number of `path`, whose hash is `hash`, if it has one.
    fn find(&self, hash: PathHash, path: &Path) -> Option<FileId> {
        let slots = &self.slots;
        self.ids
            .find(hash, |id| slots[id.index()].path.as_path() == path)
            .copied()
    }

    /// The number of `path`, whose hash is `hash`, given now if it has none.
    fn number(&mut self, path: &Path, hash: PathHash) -> FileId {
        debug_assert_eq!(
            hash,
            path_hash(path),
            "`hash` must be the path's `path_hash`"
        );
        if let Some(id) = self.find(hash, path) {
            return id;
        }
        let id = FileId::new(self.slots.len());
        self.slots.push(Slot {
            path: path.to_path_buf(),
            hash,
            value: None,
            origins: Vec::new(),
        });
        let slots = &self.slots;
        self.ids
            .insert_unique(hash, id, |id| slots[id.index()].hash);
        id
    }

    /// The path `id` numbers, whether or not its file is held.
    pub(crate) fn path(&self, id: FileId) -> &Path {
        &self.slots[id.index()].path
    }

    /// Notes that declarations naming their file by `origin` are `id`'s.
    pub(crate) fn bind_origin(&mut self, id: FileId, origin: &Arc<Path>) {
        let address = Arc::as_ptr(origin).cast::<u8>() as usize;
        if self.by_origin.insert(address, id).is_none() {
            self.slots[id.index()].origins.push(Arc::clone(origin));
        }
    }

    /// The file `path` is, when it is the `Arc<Path>` of a declaration
    /// bound by `bind_origin` - found by address, hashing nothing - and
    /// otherwise by what the path spells.
    pub(crate) fn get_by_origin(&self, path: &Path) -> Option<&V> {
        let address = std::ptr::from_ref(path).cast::<u8>() as usize;
        match self.by_origin.get(&address) {
            Some(id) => self.get_by_id(*id),
            None => self.get(path),
        }
    }

    pub(crate) fn get(&self, path: &Path) -> Option<&V> {
        self.get_by_id(self.id_of(path)?)
    }

    pub(crate) fn get_mut(&mut self, path: &Path) -> Option<&mut V> {
        let id = self.id_of(path)?;
        self.slots[id.index()].value.as_mut()
    }

    pub(crate) fn get_by_id(&self, id: FileId) -> Option<&V> {
        self.slots.get(id.index())?.value.as_ref()
    }

    pub(crate) fn get_by_id_mut(&mut self, id: FileId) -> Option<&mut V> {
        self.slots.get_mut(id.index())?.value.as_mut()
    }

    /// Holds `value` for `path`, returning what it held for it.
    pub(crate) fn insert(&mut self, path: PathBuf, value: V) -> Option<V> {
        let id = self.id_for(&path);
        self.insert_at(id, value)
    }

    /// Holds `value` for the file `id` numbers, returning what it held.
    pub(crate) fn insert_at(&mut self, id: FileId, value: V) -> Option<V> {
        let previous = self.slots[id.index()].value.replace(value);
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    pub(crate) fn remove(&mut self, path: &Path) -> Option<V> {
        let id = self.id_of(path)?;
        let removed = self.slots[id.index()].value.take();
        if removed.is_some() {
            self.len -= 1;
        }
        removed
    }

    /// Every file held, in the order its path was first seen.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&PathBuf, &V)> {
        self.slots
            .iter()
            .filter_map(|slot| slot.value.as_ref().map(|value| (&slot.path, value)))
    }

    /// `iter`, with the number of each file.
    pub(crate) fn iter_with_ids(&self) -> impl Iterator<Item = (FileId, &PathBuf, &V)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value
                .as_ref()
                .map(|value| (FileId::new(index), &slot.path, value))
        })
    }

    /// `iter`, side by side.
    pub(crate) fn par_iter(&self) -> impl ParallelIterator<Item = (&PathBuf, &V)>
    where
        V: Sync,
    {
        self.slots
            .par_iter()
            .filter_map(|slot| slot.value.as_ref().map(|value| (&slot.path, value)))
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &PathBuf> {
        self.iter().map(|(path, _)| path)
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }
}

/// `table[&path]`, for a file that is held.
impl<V> std::ops::Index<&PathBuf> for FileTable<V> {
    type Output = V;

    fn index(&self, path: &PathBuf) -> &V {
        self.get(path).expect("the file is held")
    }
}

impl<'a, V> IntoIterator for &'a FileTable<V> {
    type Item = (&'a PathBuf, &'a V);
    type IntoIter = Box<dyn Iterator<Item = (&'a PathBuf, &'a V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_keeps_its_number_through_removal_and_a_second_insert() {
        let mut table: FileTable<u32> = FileTable::default();
        let a = table.id_for(Path::new("/a.php"));
        table.insert(PathBuf::from("/a.php"), 1);
        let b = table.id_for(Path::new("/b.php"));
        table.insert(PathBuf::from("/b.php"), 2);
        assert_ne!(a, b);
        assert_eq!(table.len(), 2);
        assert_eq!(table.remove(Path::new("/a.php")), Some(1));
        assert_eq!(table.len(), 1);
        assert!(table.get(Path::new("/a.php")).is_none());
        assert!(table.get_by_id(a).is_none());
        assert_eq!(table.path(a), Path::new("/a.php"));
        assert_eq!(table.insert(PathBuf::from("/a.php"), 3), None);
        assert_eq!(table.id_of(Path::new("/a.php")), Some(a));
        assert_eq!(table.get_by_id(a), Some(&3));
        assert_eq!(table.insert(PathBuf::from("/a.php"), 4), Some(3));
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn a_bound_origin_finds_its_file_by_address_and_another_by_its_path() {
        let mut table: FileTable<u32> = FileTable::default();
        let id = table.id_for(Path::new("/a.php"));
        table.insert(PathBuf::from("/a.php"), 1);
        let origin: Arc<Path> = Arc::from(Path::new("/a.php"));
        table.bind_origin(id, &origin);
        assert_eq!(table.get_by_origin(&origin), Some(&1));
        // Another allocation spelling the same path is found by its text.
        let spelled: Arc<Path> = Arc::from(Path::new("/a.php"));
        assert_eq!(table.get_by_origin(&spelled), Some(&1));
        let unknown: Arc<Path> = Arc::from(Path::new("/b.php"));
        assert_eq!(table.get_by_origin(&unknown), None);
        // A file removed is not found by its origin.
        table.remove(Path::new("/a.php"));
        assert_eq!(table.get_by_origin(&origin), None);
    }

    #[test]
    fn iteration_is_in_the_order_paths_were_first_seen_and_skips_removed_files() {
        let mut table: FileTable<u32> = FileTable::default();
        for (i, name) in ["/c", "/a", "/b", "/d"].iter().enumerate() {
            table.insert(PathBuf::from(name), i as u32);
        }
        table.remove(Path::new("/a"));
        let keys: Vec<&Path> = table.keys().map(PathBuf::as_path).collect();
        assert_eq!(keys, [Path::new("/c"), Path::new("/b"), Path::new("/d")]);
        assert_eq!(table.values().copied().collect::<Vec<_>>(), [0, 2, 3]);
        assert_eq!((&table).into_iter().count(), 3);
    }

    #[test]
    fn a_path_with_no_number_is_absent_and_gets_one_on_asking() {
        let mut table: FileTable<u32> = FileTable::default();
        assert!(table.id_of(Path::new("/x")).is_none());
        assert!(table.get(Path::new("/x")).is_none());
        let id = table.id_for(Path::new("/x"));
        assert_eq!(table.id_of(Path::new("/x")), Some(id));
        // A number is not a file held.
        assert!(table.is_empty());
        assert!(table.get(Path::new("/x")).is_none());
    }

    #[test]
    fn paths_numbered_ahead_keep_the_number_of_their_first_place() {
        let paths = [
            Path::new("/c"),
            Path::new("/a"),
            Path::new("/c"),
            Path::new("/b"),
        ];
        let (mut table, numbers): (FileTable<u32>, _) =
            FileTable::with_paths(paths.into_iter().map(|path| (path, path_hash(path))));
        assert_eq!(numbers[0], numbers[2]);
        assert_ne!(numbers[0], numbers[1]);
        assert_ne!(numbers[1], numbers[3]);
        assert!(table.is_empty());
        assert!(table.has_numbers());
        for (path, id) in paths.iter().zip(&numbers) {
            assert_eq!(table.path(*id), *path);
        }
        assert_eq!(table.insert_at(numbers[1], 7), None);
        assert_eq!(table.get(Path::new("/a")), Some(&7));
        assert_eq!(table.id_for(Path::new("/b")), numbers[3]);
    }
}
