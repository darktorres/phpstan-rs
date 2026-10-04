//! The trees a dump's entities make, with the ids git gives them.
//!
//! A directory's entries are its entities and its directories, and, in a
//! dump cut from a text, `.order`: the names of the others in the text's
//! order, a line each. Git sorts a tree's entries by name, so the text
//! comes back from the tree through these.

use std::cmp::Ordering;
use std::io;

use rustc_hash::FxHashSet;

use crate::id::Id;
use crate::invalid;
use crate::manifest::{Kind, Manifest, order_text};

/// The name of a directory's entry listing its other entries in the text's
/// order.
pub const ORDER: &str = ".order";

/// What a tree's entry is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Blob,
    Tree,
}

/// An entry of a tree.
#[derive(Clone, Debug)]
pub struct TreeEntry {
    pub name: String,
    pub mode: Mode,
    pub id: Id,
}

/// A directory of a dump's tree.
#[derive(Debug)]
pub struct Tree {
    /// Its path in the dump, `""` for the dump's root.
    pub path: String,
    pub id: Id,
    /// Its entries in git's order, `.order` included.
    pub entries: Vec<TreeEntry>,
    /// The text of its `.order`, in a dump cut from a text.
    pub order: Option<Vec<u8>>,
}

/// The directories of `manifest`'s tree, each after the directories in
/// it, the root last; an error unless the root's id is the manifest's.
pub fn trees(manifest: &Manifest) -> io::Result<Vec<Tree>> {
    let mut builder = TreeBuilder::new(manifest.kind != Kind::Files);
    builder.keep = Some(Vec::new());
    for entry in &manifest.entries {
        builder.add(&entry.path, entry.id)?;
    }
    let (root, trees) = builder.finish()?;
    if root != manifest.root {
        return Err(invalid(format!(
            "the manifest's entities make the tree {root}, not its root {}",
            manifest.root
        )));
    }
    Ok(trees)
}

/// Builds a dump's tree from its entities' paths, each directory's
/// entities given together: a directory is finished at the first path
/// outside it.
pub(crate) struct TreeBuilder {
    /// Whether each directory has an `.order`.
    order: bool,
    /// The root, and the directories open below it.
    open: Vec<OpenTree>,
    /// The finished directories, when they are kept.
    keep: Option<Vec<Tree>>,
}

struct OpenTree {
    name: String,
    path: String,
    /// In the order they were added.
    entries: Vec<TreeEntry>,
    names: FxHashSet<String>,
}

impl OpenTree {
    fn new(name: String, path: String) -> OpenTree {
        OpenTree {
            name,
            path,
            entries: Vec::new(),
            names: FxHashSet::default(),
        }
    }
}

impl TreeBuilder {
    pub(crate) fn new(order: bool) -> TreeBuilder {
        TreeBuilder {
            order,
            open: vec![OpenTree::new(String::new(), String::new())],
            keep: None,
        }
    }

    /// Adds the blob `id` at `path`, its components joined by `/`.
    pub(crate) fn add(&mut self, path: &str, id: Id) -> io::Result<()> {
        let mut components: Vec<&str> = path.split('/').collect();
        let leaf = components.pop().unwrap_or_default();
        let common = components
            .iter()
            .zip(&self.open[1..])
            .take_while(|(name, open)| **name == open.name)
            .count();
        while self.open.len() > common + 1 {
            self.close()?;
        }
        for name in &components[common..] {
            let parent = self.open.last_mut().expect("the root stays open");
            if !parent.names.insert((*name).to_string()) {
                return Err(invalid(format!(
                    "{path}: {name} is named again in {:?} after it ends",
                    parent.path
                )));
            }
            let path = join(&parent.path, name);
            self.open.push(OpenTree::new((*name).to_string(), path));
        }
        let dir = self.open.last_mut().expect("the root stays open");
        if !dir.names.insert(leaf.to_string()) {
            return Err(invalid(format!("{path} is named twice")));
        }
        dir.entries.push(TreeEntry {
            name: leaf.to_string(),
            mode: Mode::Blob,
            id,
        });
        Ok(())
    }

    /// The root's id, and the directories kept.
    pub(crate) fn finish(mut self) -> io::Result<(Id, Vec<Tree>)> {
        while self.open.len() > 1 {
            self.close()?;
        }
        let root = self.open.pop().expect("the root stays open");
        let tree = self.tree(root);
        let id = tree.id;
        let mut trees = self.keep.take().unwrap_or_default();
        trees.push(tree);
        Ok((id, trees))
    }

    fn close(&mut self) -> io::Result<()> {
        let open = self.open.pop().expect("a directory below the root");
        let name = open.name.clone();
        let tree = self.tree(open);
        let parent = self.open.last_mut().expect("the root stays open");
        parent.entries.push(TreeEntry {
            name,
            mode: Mode::Tree,
            id: tree.id,
        });
        if let Some(keep) = &mut self.keep {
            keep.push(tree);
        }
        Ok(())
    }

    fn tree(&self, open: OpenTree) -> Tree {
        let OpenTree {
            name: _,
            path,
            mut entries,
            names: _,
        } = open;
        let order = self
            .order
            .then(|| order_text(entries.iter().map(|entry| entry.name.as_str())));
        if let Some(order) = &order {
            entries.push(TreeEntry {
                name: ORDER.to_string(),
                mode: Mode::Blob,
                id: Id::blob(order),
            });
        }
        let id = tree_id(&mut entries);
        Tree {
            path,
            id,
            entries,
            order,
        }
    }
}

/// The id of the tree holding `entries`, which it sorts in git's order.
pub fn tree_id(entries: &mut [TreeEntry]) -> Id {
    entries.sort_unstable_by(git_order);
    let mut content = Vec::with_capacity(entries.len() * 64);
    for entry in entries.iter() {
        content.extend_from_slice(match entry.mode {
            Mode::Blob => b"100644 ".as_slice(),
            Mode::Tree => b"40000 ".as_slice(),
        });
        content.extend_from_slice(entry.name.as_bytes());
        content.push(0);
        content.extend_from_slice(&entry.id.0);
    }
    Id::tree(&content)
}

/// Git's order of a tree's entries: by name, a tree's name compared as if
/// `/` followed it.
fn git_order(a: &TreeEntry, b: &TreeEntry) -> Ordering {
    fn key(entry: &TreeEntry) -> impl Iterator<Item = u8> + '_ {
        let slash = (entry.mode == Mode::Tree).then_some(b'/');
        entry.name.bytes().chain(slash)
    }
    key(a).cmp(key(b))
}

/// `name` in the directory at `path`.
pub fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}/{name}")
    }
}
