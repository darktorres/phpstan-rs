//! The walk that lists a run's PHP files. A file's identity is its resolved
//! path (see `resolve`), so it is listed once however many roots and links
//! reach it, under that path. A link is followed to its target, and a
//! directory is entered once per ancestor chain: a directory that is its own
//! ancestor ends the loop there.
//!
//! Each root is walked as it is added. A link met on the way is recorded, and
//! `Walk::finish` walks the targets no root holds, strongest role first. A
//! target under a root is left to that root's walk. A file's role is the
//! strongest of the roots and links that list it.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use same_file::Handle;

/// The role a listed file plays, weakest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Role {
    Context,
    Stub,
    Analysed,
}

impl Role {
    /// The role's slot in `Walk::targets`.
    fn index(self) -> usize {
        self as usize
    }
}

/// A PHP file the walk listed, once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    /// The file's resolved path.
    pub(crate) path: PathBuf,
    /// The strongest role of the roots and links that list the file.
    pub(crate) role: Role,
    /// Whether a stub root lists it.
    pub(crate) stub: bool,
}

/// A root the walk was given, by its resolved path.
#[derive(Clone, Debug)]
pub(crate) struct Root<'a> {
    pub(crate) path: PathBuf,
    pub(crate) role: Role,
    /// Whether this is a stub root.
    pub(crate) stub: bool,
    /// The directory names the walk leaves out below the root.
    pub(crate) excluded: &'a [&'a str],
}

/// What a walk listed, and the roots it was given.
pub(crate) struct Walked<'a> {
    pub(crate) files: Vec<Listed>,
    pub(crate) roots: Vec<Root<'a>>,
}

/// One run's walk over its roots. Each root is walked with the directory
/// names it excludes, and a link's target with the names of the walk that
/// met the link.
pub(crate) struct Walk<'a> {
    roots: Vec<Root<'a>>,
    /// The targets of the links met so far, by the role of the walk that met
    /// each link, with the names that walk excludes.
    targets: [VecDeque<(PathBuf, &'a [&'a str])>; 3],
    listed: Vec<Listed>,
    /// Where each listed file is, by its resolved path.
    index: FxHashMap<PathBuf, usize>,
}

impl<'a> Walk<'a> {
    pub(crate) fn new() -> Self {
        Walk {
            roots: Vec::new(),
            targets: Default::default(),
            listed: Vec::new(),
            index: FxHashMap::default(),
        }
    }

    /// Adds `path` as a root. A directory is walked now; a file is listed
    /// when it is PHP. The names in `excluded` apply below the root: the walk
    /// does not enter a directory of such a name, nor follow a link of it, and
    /// the root itself is walked whatever its name.
    pub(crate) fn add(&mut self, path: &Path, role: Role, excluded: &'a [&'a str]) {
        let identity = resolve(path);
        self.roots.push(Root {
            path: identity.clone(),
            role,
            stub: role == Role::Stub,
            excluded,
        });
        if identity.is_dir() {
            if let Ok(handle) = Handle::from_path(&identity) {
                self.walk_dir(&identity, role, excluded, &mut vec![handle], false);
            }
        } else if is_php(&identity) {
            self.list(identity, role);
        }
    }

    /// Adds `path` as a root and lists it whatever its extension, as an
    /// explicitly configured file is listed.
    pub(crate) fn add_file(&mut self, path: &Path, role: Role) {
        let identity = resolve(path);
        self.roots.push(Root {
            path: identity.clone(),
            role,
            stub: role == Role::Stub,
            excluded: &[],
        });
        self.list(identity, role);
    }

    /// Walks the targets no root holds, strongest role first, and returns the
    /// listed files with the roots they were given. A target is walked once
    /// for each set of excluded names that meets it.
    pub(crate) fn finish(mut self) -> Walked<'a> {
        let mut walked: FxHashMap<PathBuf, Vec<&'a [&'a str]>> = FxHashMap::default();
        while let Some((role, target, excluded)) = self.next_target() {
            if self.under_root(&target) {
                continue;
            }
            let walks = walked.entry(target.clone()).or_default();
            if walks.contains(&excluded) {
                continue;
            }
            walks.push(excluded);
            if target.is_dir() {
                if let Ok(handle) = Handle::from_path(&target) {
                    self.walk_dir(&target, role, excluded, &mut vec![handle], true);
                }
            } else if is_php(&target) {
                self.list(target, role);
            }
        }
        Walked {
            files: self.listed,
            roots: self.roots,
        }
    }

    /// Lists the files under `dir`, a directory's resolved path. `chain` holds
    /// the resolved directories above `dir`. With `outside`, `dir` is the
    /// target of no root: the files and directories under a root are left to
    /// that root.
    fn walk_dir(
        &mut self,
        dir: &Path,
        role: Role,
        excluded: &'a [&'a str],
        chain: &mut Vec<Handle>,
        outside: bool,
    ) {
        let Ok(read) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<fs::DirEntry> = read.flatten().collect();
        entries.sort_by_cached_key(fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name();
            let child = dir.join(&name);
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => match fs::symlink_metadata(&child) {
                    Ok(metadata) => metadata.file_type(),
                    Err(_) => continue,
                },
            };
            let under_root = outside && self.under_root(&child);
            if file_type.is_symlink() {
                if excludes(excluded, &name) {
                    continue;
                }
                match fs::canonicalize(&child) {
                    Ok(target) => self.targets[role.index()].push_back((target, excluded)),
                    // A dangling `.php` link is listed under its own path, so
                    // that reading it reports that path.
                    Err(_) if is_php(&child) && !under_root => self.list(child, role),
                    Err(_) => {}
                }
            } else if file_type.is_dir() {
                if excludes(excluded, &name) || under_root {
                    continue;
                }
                let Ok(handle) = Handle::from_path(&child) else {
                    continue;
                };
                // A directory that is its own ancestor ends the loop: its
                // files are listed by the walk above it.
                if chain.contains(&handle) {
                    continue;
                }
                chain.push(handle);
                self.walk_dir(&child, role, excluded, chain, outside);
                chain.pop();
            } else if is_php(&child) && !under_root {
                self.list(child, role);
            }
        }
    }

    /// Records a listed file at its resolved path. A file listed again keeps
    /// the strongest role of its listings.
    fn list(&mut self, path: PathBuf, role: Role) {
        let stub = role == Role::Stub;
        if let Some(&at) = self.index.get(&path) {
            let listed = &mut self.listed[at];
            listed.stub |= stub;
            listed.role = listed.role.max(role);
        } else {
            self.index.insert(path.clone(), self.listed.len());
            self.listed.push(Listed { path, role, stub });
        }
    }

    fn next_target(&mut self) -> Option<(Role, PathBuf, &'a [&'a str])> {
        [Role::Analysed, Role::Stub, Role::Context]
            .into_iter()
            .find_map(|role| {
                self.targets[role.index()]
                    .pop_front()
                    .map(|(target, excluded)| (role, target, excluded))
            })
    }

    fn under_root(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(&root.path))
    }
}

/// The resolved path of `path`: its links followed and its `.` and `..`
/// taken out. A path that does not exist resolves through its nearest
/// ancestor that does, so a file that is gone keeps the path it had while
/// its directory stands. A path with no existing ancestor is returned as
/// given.
pub(crate) fn resolve(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(resolved) = fs::canonicalize(ancestor) {
            let rest = path.strip_prefix(ancestor).unwrap_or(Path::new(""));
            if rest.as_os_str().is_empty() {
                return resolved;
            }
            return composer::manifest::lexically_normalized(&resolved.join(rest));
        }
    }
    path.to_path_buf()
}

/// Whether `name` is one of `excluded`.
fn excludes(excluded: &[&str], name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| excluded.contains(&name))
}

fn is_php(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "php")
}
