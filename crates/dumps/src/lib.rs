//! Dumps as the store holds them. A dump's text is cut into entities as it
//! is written: a section, a field, a table's entry, a file's lines. Each
//! entity has a path, and the id git gives its text as a blob. The paths
//! make a tree, and the manifest lists the entities in the text's order,
//! which each of the tree's directories records too.
//!
//! The store is a git repository in the SHA-256 object format, so the ids
//! a run computes as it writes are the store's: a manifest is the tree the
//! store files the dump as, and importing it writes only the blobs the
//! store lacks. Given the manifest of another run of the same dump, a run
//! writes only the entities whose ids differ from it, which is all a
//! comparison of the two reads.

use std::io;

mod id;
mod manifest;
mod name;
mod split;
mod tree;
mod writer;

pub use id::Id;
pub use manifest::{Difference, Dump, Entry, Kind, Manifest, first_difference, order_text};
pub use name::escape;
pub use tree::{Mode, ORDER, Tree, TreeEntry, join, tree_id, trees};
pub use writer::{DumpWriter, FileEntity, cut};

/// The manifest beside the dump whose text is at `text`: its path with
/// `.manifest` added.
pub fn manifest_path(text: &std::path::Path) -> std::path::PathBuf {
    let mut path = text.as_os_str().to_owned();
    path.push(".manifest");
    path.into()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests;
