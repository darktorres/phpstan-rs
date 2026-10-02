//! A number for each file a project has been given, so that what is kept by
//! file - which files reference a name, which were found affected - holds
//! and compares four bytes, not a path that is hashed component by
//! component whenever it is looked up or a table of them grows.

/// The number a `Project` gave a file the first time it saw its path. A
/// path keeps its number for as long as the project lives, through the file
/// being removed and loaded again, and no other path ever has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct FileId(u32);

impl FileId {
    pub(crate) fn new(index: usize) -> FileId {
        FileId(u32::try_from(index).expect("fewer than 2^32 files"))
    }

    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}
