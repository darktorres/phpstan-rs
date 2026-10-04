//! A number for each file a project has been given, so that what is kept by
//! file - which files reference a name, which were found affected - holds
//! and compares four bytes, not a path that is hashed component by
//! component whenever it is looked up or a table of them grows.

/// The number a `Project` gave a file the first time it saw its path. A
/// path keeps its number for as long as the project lives, through the file
/// being removed and loaded again, and no other path ever has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct FileId(u32);

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
}

impl FileId {
    pub(crate) fn new(index: usize) -> FileId {
        FileId(u32::try_from(index).expect("fewer than 2^32 files"))
    }

    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

thread_local! {
    /// The path of each file by number, while a dump prints them.
    static PATHS: std::cell::RefCell<Vec<std::path::PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Runs `print` with each file numbered by `paths` printing as its path,
/// on this thread.
pub(crate) fn printing_paths<R>(paths: Vec<std::path::PathBuf>, print: impl FnOnce() -> R) -> R {
    let previous = PATHS.with(|known| known.replace(paths));
    let result = print();
    PATHS.with(|known| known.replace(previous));
    result
}

/// The file's path, under `printing_paths`; a number is no part of what a
/// file is, and another numbering prints the same.
impl syntax::canonical::Canonical for FileId {
    fn canonical(&self, p: &mut syntax::canonical::Printer) {
        let FileId(index) = *self;
        PATHS.with(|known| match known.borrow().get(index as usize) {
            Some(path) => path.canonical(p),
            None => p.atom(&format!("#{index}")),
        });
    }
}
