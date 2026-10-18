//! A number for each file a project has been given, so that what is kept by
//! file - which files reference a name, which were found affected - holds
//! and compares four bytes, not a path that is hashed component by
//! component whenever it is looked up or a table of them grows.

/// The number a `Project` gave a file the first time it saw its path. A
/// path keeps its number for as long as the project lives, through the file
/// being removed and loaded again, and no other path ever has it. A number
/// means something only to the project that gave it, and only the crate
/// makes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

/// A set of files by number, a bit for each.
#[derive(Debug, Default, Clone)]
pub(crate) struct FileIdSet {
    words: Vec<u64>,
}

impl FileIdSet {
    /// Adds `file`; whether it was not in the set.
    pub(crate) fn insert(&mut self, file: FileId) -> bool {
        let (word, bit) = (file.index() / 64, file.index() % 64);
        if self.words.len() <= word {
            self.words.resize(word + 1, 0);
        }
        let new = self.words[word] & (1 << bit) == 0;
        self.words[word] |= 1 << bit;
        new
    }

    /// Whether `file` is in the set.
    pub(crate) fn contains(&self, file: FileId) -> bool {
        let (word, bit) = (file.index() / 64, file.index() % 64);
        self.words
            .get(word)
            .is_some_and(|word| word & (1 << bit) != 0)
    }
}

impl FileId {
    pub(crate) fn new(index: usize) -> FileId {
        FileId(u32::try_from(index).expect("fewer than 2^32 files"))
    }

    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}
