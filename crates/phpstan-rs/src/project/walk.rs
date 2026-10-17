//! The walk that lists a run's PHP files. A file's identity is its resolved
//! path (see `resolve`), so it is listed once however many roots and links
//! reach it, under that path. A link is followed to its target, and a
//! directory is entered once per ancestor chain: a directory that is its own
//! ancestor ends the loop there.
//!
//! `Walk::finish` walks the roots, then the targets of the links they met.
//! Each directory is walked by a task of its own on the pool, and the results
//! are merged in listing order, so the listing does not depend on the number
//! of threads. A link met on the way is recorded, and the targets no root
//! holds are walked strongest role first. A target under a root is left to
//! that root's walk. A file's role is the strongest of the roots and links
//! that list it. A context directory under a stronger root that excludes no
//! names is not walked: that root lists its files.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use composer::dir_listings::DirListings;
use hashbrown::hash_map::EntryRef;
use rayon::prelude::*;
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};
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

/// Where each listed file is in a walk's listing, by its resolved path.
pub(crate) type FileIndex = hashbrown::HashMap<PathBuf, usize, FxBuildHasher>;

/// What a walk listed, where each file is, and the roots it was given.
pub(crate) struct Walked<'a> {
    pub(crate) files: Vec<Listed>,
    pub(crate) index: FileIndex,
    pub(crate) roots: Vec<Root<'a>>,
}

/// One run's walk over its roots. Each root is walked with the directory
/// names it excludes, and a link's target with the names of the walk that
/// met the link.
pub(crate) struct Walk<'a> {
    roots: Vec<Root<'a>>,
    /// The walks of the roots, in the order the roots were added.
    jobs: Vec<Job<'a>>,
    /// The resolved paths of the stronger roots that exclude no names. A
    /// context walk leaves out the directories under them.
    covering: FxHashSet<PathBuf>,
    /// The targets of the links met so far, by the role of the walk that met
    /// each link, with the names that walk excludes.
    targets: [VecDeque<(PathBuf, &'a [&'a str])>; 3],
    listed: Vec<Listed>,
    index: FileIndex,
    /// The resolved path of each directory an explicit file was listed in, by
    /// the directory as it was given.
    directories: hashbrown::HashMap<PathBuf, PathBuf, FxBuildHasher>,
    /// The listings of the directories the walk reads. A load shares them with
    /// its vendor discovery, so each directory is read once.
    listings: Arc<DirListings>,
}

/// The walk of one root: a directory, or a file that is listed whatever its
/// extension.
enum Job<'a> {
    Dir { path: PathBuf, scope: Scope<'a> },
    File { path: PathBuf, role: Role },
}

/// What a walk applies to the entries it meets.
#[derive(Clone, Copy)]
struct Scope<'a> {
    /// The role of the files the walk lists.
    role: Role,
    /// The directory names the walk leaves out below its directory.
    excluded: &'a [&'a str],
    /// Whether the walk is of a link's target. Such a walk leaves the files
    /// under any root to that root's walk.
    outside: bool,
}

/// What a walk lists, in listing order.
enum Found {
    /// A PHP file, under its path.
    File(PathBuf),
    /// The resolved target of a link, which the walk that met the link queues.
    Link(PathBuf),
}

/// An entry of a directory, in listing order: what the directory's walk lists
/// itself, or a subdirectory, whose walk lists its contents.
enum Step {
    Found(Found),
    Dir,
}

/// The directories a walk is inside, from its own directory up, by identity.
/// A directory that is one of them ends the walk there.
struct Ancestors<'p> {
    handle: &'p Handle,
    parent: Option<&'p Ancestors<'p>>,
}

impl Ancestors<'_> {
    fn holds(&self, handle: &Handle) -> bool {
        let mut at = Some(self);
        while let Some(ancestor) = at {
            if ancestor.handle == handle {
                return true;
            }
            at = ancestor.parent;
        }
        false
    }
}

impl<'a> Walk<'a> {
    /// A walk that reads its directories through `listings`.
    pub(crate) fn new(listings: Arc<DirListings>) -> Self {
        Walk {
            roots: Vec::new(),
            jobs: Vec::new(),
            covering: FxHashSet::default(),
            targets: Default::default(),
            listed: Vec::new(),
            index: FileIndex::default(),
            directories: Default::default(),
            listings,
        }
    }

    /// Adds `path` as a root, walked by `finish`. A directory is walked; a file
    /// is listed when it is PHP. The names in `excluded` apply below the root:
    /// the walk does not enter a directory of such a name, nor follow a link of
    /// it, and the root itself is walked whatever its name.
    pub(crate) fn add(&mut self, path: &Path, role: Role, excluded: &'a [&'a str]) {
        let identity = resolve(path);
        self.roots.push(Root {
            path: identity.clone(),
            role,
            stub: role == Role::Stub,
            excluded,
        });
        if identity.is_dir() {
            let scope = Scope {
                role,
                excluded,
                outside: false,
            };
            self.jobs.push(Job::Dir {
                path: identity,
                scope,
            });
        } else if is_php(&identity) {
            self.jobs.push(Job::File {
                path: identity,
                role,
            });
        }
    }

    /// Adds `path` as a root and lists it whatever its extension, as an
    /// explicitly configured file is listed.
    pub(crate) fn add_file(&mut self, path: &Path, role: Role) {
        let identity = self.resolve_file(path);
        self.roots.push(Root {
            path: identity.clone(),
            role,
            stub: role == Role::Stub,
            excluded: &[],
        });
        self.jobs.push(Job::File {
            path: identity,
            role,
        });
    }

    /// Walks the roots, then the targets no root holds, strongest role first,
    /// on `pool`, and returns the listed files with the roots they were given.
    /// A target is walked once for each set of excluded names that meets it.
    pub(crate) fn finish(mut self, pool: &rayon::ThreadPool) -> Walked<'a> {
        self.covering = self
            .roots
            .iter()
            .filter(|root| root.role > Role::Context && root.excluded.is_empty())
            .map(|root| root.path.clone())
            .collect();
        let jobs = std::mem::take(&mut self.jobs);
        pool.install(|| {
            let found: Vec<Vec<Found>> = jobs.par_iter().map(|job| self.run(job)).collect();
            for (job, found) in jobs.iter().zip(found) {
                match job {
                    Job::Dir { scope, .. } => self.merge(found, scope.role, scope.excluded),
                    Job::File { role, .. } => self.merge(found, *role, &[]),
                }
            }
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
                let scope = Scope {
                    role,
                    excluded,
                    outside: true,
                };
                let found = self.walk_target(&target, scope);
                self.merge(found, role, excluded);
            }
        });
        Walked {
            files: self.listed,
            index: self.index,
            roots: self.roots,
        }
    }

    /// What the walk of one root lists.
    fn run(&self, job: &Job<'a>) -> Vec<Found> {
        match job {
            Job::Dir { path, scope } => self.walk_root(path, *scope),
            Job::File { path, .. } => vec![Found::File(path.clone())],
        }
    }

    /// What the walk of a link's target lists: the files under a directory, or
    /// the target itself when it is a PHP file.
    fn walk_target(&self, target: &Path, scope: Scope<'a>) -> Vec<Found> {
        if target.is_dir() {
            self.walk_root(target, scope)
        } else if is_php(target) {
            vec![Found::File(target.to_path_buf())]
        } else {
            Vec::new()
        }
    }

    /// Walks the directory `path`, a resolved path, from the top.
    fn walk_root(&self, path: &Path, scope: Scope<'a>) -> Vec<Found> {
        if self.prunes(scope, path) {
            return Vec::new();
        }
        let Ok(handle) = Handle::from_path(path) else {
            return Vec::new();
        };
        self.walk_dir(
            path,
            scope,
            &Ancestors {
                handle: &handle,
                parent: None,
            },
        )
    }

    /// Lists what `dir`, a resolved directory, holds under `scope`, in listing
    /// order. A subdirectory is walked by a task of its own, and its results
    /// are placed back among the entries in order. `ancestors` holds `dir` and
    /// the directories above it in this walk.
    fn walk_dir(&self, dir: &Path, scope: Scope<'a>, ancestors: &Ancestors<'_>) -> Vec<Found> {
        let Some(listing) = self.listings.listing(dir) else {
            return Vec::new();
        };
        let mut steps = Vec::with_capacity(listing.entries.len());
        let mut dirs = Vec::new();
        for entry in &listing.entries {
            // An entry whose type cannot be read is not listed.
            let Some(file_type) = entry.kind else {
                continue;
            };
            let name = &entry.name;
            let child = dir.join(name);
            let under_root = scope.outside && self.under_root(&child);
            if file_type.is_symlink() {
                if excludes(scope.excluded, name) {
                    continue;
                }
                match fs::canonicalize(&child) {
                    Ok(target) => steps.push(Step::Found(Found::Link(target))),
                    // A dangling `.php` link is listed under its own path, so
                    // that reading it reports that path.
                    Err(_) if is_php(&child) && !under_root => {
                        steps.push(Step::Found(Found::File(child)));
                    }
                    Err(_) => {}
                }
            } else if file_type.is_dir() {
                if excludes(scope.excluded, name) || under_root || self.prunes(scope, &child) {
                    continue;
                }
                steps.push(Step::Dir);
                dirs.push(child);
            } else if is_php(&child) && !under_root {
                steps.push(Step::Found(Found::File(child)));
            }
        }
        let subtrees: Vec<Vec<Found>> = dirs
            .par_iter()
            .map(|child| self.walk_subdirectory(child, scope, ancestors))
            .collect();
        let mut subtrees = subtrees.into_iter();
        let mut found = Vec::new();
        for step in steps {
            match step {
                Step::Found(item) => found.push(item),
                Step::Dir => found.extend(subtrees.next().unwrap_or_default()),
            }
        }
        found
    }

    /// Walks the subdirectory `child`, unless it is one of `ancestors`, which
    /// ends the loop. The subdirectory's handle is opened here, so only the
    /// directories being walked hold one.
    fn walk_subdirectory(
        &self,
        child: &Path,
        scope: Scope<'a>,
        ancestors: &Ancestors<'_>,
    ) -> Vec<Found> {
        let Ok(handle) = Handle::from_path(child) else {
            return Vec::new();
        };
        // A directory that is its own ancestor ends the loop: its files are
        // listed by the walk above it.
        if ancestors.holds(&handle) {
            return Vec::new();
        }
        self.walk_dir(
            child,
            scope,
            &Ancestors {
                handle: &handle,
                parent: Some(ancestors),
            },
        )
    }

    /// Whether a context walk leaves `path`, a resolved path, out, because a
    /// stronger root that excludes no names holds it. The walk of a link's
    /// target is not left out here.
    fn prunes(&self, scope: Scope<'a>, path: &Path) -> bool {
        !self.covering.is_empty()
            && scope.role == Role::Context
            && !scope.outside
            && path
                .ancestors()
                .any(|ancestor| self.covering.contains(ancestor))
    }

    /// Records what a walk listed, in the order it listed it: each file under
    /// the walk's role, and each link's target queued with the walk's role and
    /// names.
    fn merge(&mut self, found: Vec<Found>, role: Role, excluded: &'a [&'a str]) {
        for item in found {
            match item {
                Found::File(path) => self.list(path, role),
                Found::Link(target) => self.targets[role.index()].push_back((target, excluded)),
            }
        }
    }

    /// Records a listed file at its resolved path. A file listed again keeps
    /// the strongest role of its listings.
    fn list(&mut self, path: PathBuf, role: Role) {
        let stub = role == Role::Stub;
        match self.index.entry_ref(&path) {
            EntryRef::Occupied(entry) => {
                let listed = &mut self.listed[*entry.get()];
                listed.stub |= stub;
                listed.role = listed.role.max(role);
            }
            EntryRef::Vacant(entry) => {
                entry.insert(self.listed.len());
                self.listed.push(Listed { path, role, stub });
            }
        }
    }

    /// The resolved path of the explicitly configured file `path`, as `resolve`
    /// gives it. A regular file's path is its directory's resolved path, which
    /// is resolved once per walk, with the file's name joined to it. A link, a
    /// path ending in `.` or `..`, and a path that does not exist are left to
    /// `resolve`.
    fn resolve_file(&mut self, path: &Path) -> PathBuf {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return resolve(path);
        };
        if !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            return resolve(path);
        }
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        match self.directories.entry_ref(parent) {
            EntryRef::Occupied(entry) => entry.get().join(name),
            EntryRef::Vacant(entry) => {
                let directory = resolve(parent);
                let identity = directory.join(name);
                entry.insert(directory);
                identity
            }
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
    if let Ok(resolved) = fs::canonicalize(path) {
        return resolved;
    }
    for ancestor in path.ancestors().skip(1) {
        if let Ok(resolved) = fs::canonicalize(ancestor) {
            let rest = path.strip_prefix(ancestor).unwrap_or(Path::new(""));
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
