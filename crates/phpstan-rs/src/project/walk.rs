//! The walk that lists a run's PHP files. A file's identity is its resolved
//! path (see `resolve`), so it is listed once however many roots and links
//! reach it, under that path. A link is followed to its target, and a
//! directory is entered once per ancestor chain: a directory that is its own
//! ancestor ends the loop there.
//!
//! `Walk::finish` walks the roots, then the targets of the links they met.
//! Each directory is walked by a task of its own on the pool, and the results
//! are placed the same way whatever the number of threads. A link met on the
//! way is recorded, and the targets no root holds are walked strongest role
//! first. A target under a root is left to that root's walk. A file's role is
//! the strongest of the roots and links that list it. A context directory under
//! a stronger root that excludes no names is not walked: that root lists its
//! files.
//!
//! A file is final as soon as one walk can list it: no other walk reaches it,
//! so its role cannot change. Such a file is handed to the sink with the files
//! of its directory, while the walk goes on, so it can be parsed at once. A
//! file that two walks can reach, or that only a link reaches, is handed over
//! by no one: its role is known when the walk ends.
//!
//! The listed files are placed in a tree of the directories that hold them.
//! Each directory has one slot per entry of its listing, in the listing's
//! order, and a depth-first walk of the tree lists the files in path order.
//! Nothing is sorted: a listing is sorted by name bytes, which is the order
//! `Path` compares components in.

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use composer::dir_listings::{DirListings, Listing};
use composer::hashed_path::{HashedPath, PathHash, hashed_ancestors, path_hash};
use composer::manifest::resolve;
use hashbrown::hash_map::EntryRef;
use hashbrown::hash_table::Entry;
use rayon::prelude::*;
use rustc_hash::{FxBuildHasher, FxHashMap};
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
    /// The file's resolved path, with its hash.
    pub(crate) path: HashedPath,
    /// The strongest role of the roots and links that list the file.
    pub(crate) role: Role,
    /// Whether a stub root lists it.
    pub(crate) stub: bool,
    /// The package that owns the file's directory, as the walk that listed the
    /// file found it: `Some(None)` when no package does. `None` when no walk
    /// listed the file, and the project finds the owner when it inserts it.
    pub(crate) owner: Option<Option<composer::PackageOwner>>,
}

impl Listed {
    /// A file listed at `path`, with no owner found.
    pub(crate) fn new(path: PathBuf, role: Role, stub: bool) -> Self {
        Listed {
            path: HashedPath::new(path),
            role,
            stub,
            owner: None,
        }
    }
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

/// Where each listed file is in a walk's listing, by its path. The index
/// holds positions, so a path is stored once, in the listing.
#[derive(Clone, Debug, Default)]
pub(crate) struct FileIndex(hashbrown::HashTable<usize>);

impl FileIndex {
    /// The index of `files`, where each file is at its position. A path
    /// listed twice has the position of its last listing.
    pub(crate) fn of(files: &[Listed]) -> Self {
        let mut index: hashbrown::HashTable<usize> =
            hashbrown::HashTable::with_capacity(files.len());
        for (at, file) in files.iter().enumerate() {
            match index.entry(
                file.path.hash(),
                |&i| files[i].path == file.path,
                |&i| files[i].path.hash(),
            ) {
                Entry::Occupied(mut entry) => *entry.get_mut() = at,
                Entry::Vacant(entry) => {
                    entry.insert(at);
                }
            }
        }
        FileIndex(index)
    }

    /// The position of `path` in `files`, the listing this index was made for.
    pub(crate) fn position(&self, files: &[Listed], path: &Path) -> Option<usize> {
        self.0
            .find(path_hash(path), |&at| files[at].path.as_path() == path)
            .copied()
    }
}

/// What a walk listed, where each file is, and the roots it was given.
pub(crate) struct Walked<'a> {
    pub(crate) files: Vec<Listed>,
    pub(crate) index: FileIndex,
    pub(crate) roots: Vec<Root<'a>>,
}

/// Where a walk hands over the files whose roles are final. A directory's files
/// are handed over in one batch, as its listing is read, so that they can be
/// handled on the pool while the walk goes on.
pub(crate) trait FinalFiles: Sync {
    /// Takes `files`, a directory's batch in listing order. Each file is one that
    /// only this walk lists, with the role it has in the walk's listing. A task
    /// spawned on `scope` handles the batch, and the walk does not return until
    /// the task is done.
    fn take<'s>(&'s self, scope: &rayon::Scope<'s>, files: Vec<Listed>);
}

/// A sink that takes nothing, for a caller that needs only the listing.
pub(crate) struct Unstreamed;

impl FinalFiles for Unstreamed {
    fn take<'s>(&'s self, _scope: &rayon::Scope<'s>, _files: Vec<Listed>) {}
}

/// How a walk hands a directory's batch to the sink, from inside its scope.
type Take<'t> = dyn Fn(Vec<Listed>) + Sync + 't;

/// One run's walk over its roots. Each root is walked with the directory
/// names it excludes, and a link's target with the names of the walk that
/// met the link. Each file listed carries the package that owns its
/// directory, found in the packages the walk was given.
pub(crate) struct Walk<'a, 'p> {
    roots: Vec<Root<'a>>,
    /// The walks of the roots, in the order the roots were added.
    jobs: Vec<Job<'a>>,
    /// The walks the roots start, by the path each starts from, so that a file
    /// is known to be listed by one walk alone before the walks run.
    starts: Starts<'a>,
    /// The targets of the links met so far, by the role of the walk that met
    /// each link, with the names that walk excludes.
    targets: [VecDeque<(PathBuf, &'a [&'a str])>; 3],
    /// The directories that hold the listed files, with the files in them.
    tree: Tree,
    /// The listed files the tree cannot place: a file whose directory, or one
    /// of the directories above it, cannot be listed.
    unplaced: Vec<Listed>,
    /// The resolved path of each directory an explicit file was listed in, by
    /// the directory as it was given.
    directories: hashbrown::HashMap<PathBuf, HashedPath, FxBuildHasher>,
    /// The listings of the directories the walk reads. A load shares them with
    /// its vendor discovery, so each directory is read once.
    listings: Arc<DirListings>,
    /// The packages whose directories give each listed file its owner.
    packages: &'p composer::PackageIndex,
}

/// The walk of one root: a directory, or a file that is listed whatever its
/// extension.
enum Job<'a> {
    Dir { path: HashedPath, scope: Scope<'a> },
    File { path: HashedPath, role: Role },
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

impl Scope<'_> {
    /// Whether a context walk leaves out a directory that `reaching` reaches,
    /// because a stronger root that excludes no names holds it. The walk of a
    /// link's target is not left out here.
    fn prunes(self, reaching: &Reaching<'_>) -> bool {
        self.role == Role::Context && !self.outside && reaching.covered
    }
}

/// How many directory walks reach a directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    None,
    /// One walk reaches it, with this role.
    Sole(Role),
    Several,
}

/// A directory walk: its role and the names it excludes.
type DirWalk<'a> = (Role, &'a [&'a str]);

/// The walks the roots start, by the path each starts from. Each walk is
/// counted here before any runs, so a file's walks are known when it is listed.
#[derive(Default)]
struct Starts<'a> {
    /// The directory walks.
    dirs: hashbrown::HashTable<(HashedPath, Vec<DirWalk<'a>>)>,
    /// The file walks, each with its role, by the file's resolved path.
    files: hashbrown::HashTable<(HashedPath, Vec<Role>)>,
    /// The resolved paths of the stronger roots that exclude no names. A
    /// context walk leaves out the directories under them.
    covering: hashbrown::HashTable<HashedPath>,
}

/// What reaches a directory: the directory walks that list its files, each
/// with its role and the names it excludes, and whether a covering root holds
/// it. A walk carries it down from its root, a subdirectory's made from its
/// directory's (`Starts::below`).
#[derive(Clone, Default)]
struct Reaching<'a> {
    walks: Vec<DirWalk<'a>>,
    covered: bool,
}

impl Reaching<'_> {
    /// How many directory walks list the files of the directory. A context walk
    /// lists nothing under a covering root (`prunes`). A walk that cannot read
    /// a directory is counted too, which only keeps a file from being handed
    /// over.
    fn reach(&self) -> Reach {
        let mut reach = Reach::None;
        for &(role, _) in &self.walks {
            if self.covered && role == Role::Context {
                continue;
            }
            reach = match reach {
                Reach::None => Reach::Sole(role),
                Reach::Sole(_) | Reach::Several => Reach::Several,
            };
        }
        reach
    }
}

impl<'a> Starts<'a> {
    fn of(jobs: &[Job<'a>], roots: &[Root<'a>]) -> Self {
        let mut starts = Starts::default();
        for job in jobs {
            match job {
                Job::Dir { path, scope } => {
                    held(&mut starts.dirs, path).push((scope.role, scope.excluded));
                }
                Job::File { path, role } => held(&mut starts.files, path).push(*role),
            }
        }
        for root in roots
            .iter()
            .filter(|root| root.role > Role::Context && root.excluded.is_empty())
        {
            let path = HashedPath::new(root.path.clone());
            if !starts.covers(path.as_path(), path.hash()) {
                starts
                    .covering
                    .insert_unique(path.hash(), path, HashedPath::hash);
            }
        }
        starts
    }

    /// What reaches `dir`, a resolved directory: what each directory above it
    /// passes down, from the top.
    fn reaching(&self, dir: &Path) -> Reaching<'a> {
        let ancestors: Vec<(&Path, PathHash)> = hashed_ancestors(dir).collect();
        ancestors
            .into_iter()
            .rev()
            .fold(Reaching::default(), |above, (ancestor, hash)| {
                self.below(&above, ancestor, hash)
            })
    }

    /// What reaches `dir`, whose hash is `hash`, a subdirectory of a directory
    /// that `above` reaches: the walks that reach that directory and do not
    /// exclude the name of `dir`, and the walks that start at `dir`.
    fn below(&self, above: &Reaching<'a>, dir: &Path, hash: PathHash) -> Reaching<'a> {
        let name = dir.file_name();
        let mut walks: Vec<DirWalk<'a>> = above
            .walks
            .iter()
            .copied()
            .filter(|&(_, excluded)| !name.is_some_and(|name| excludes(excluded, name)))
            .collect();
        if let Some((_, starting)) = self.dirs.find(hash, |(start, _)| start.as_path() == dir) {
            walks.extend_from_slice(starting);
        }
        Reaching {
            walks,
            covered: above.covered || self.covers(dir, hash),
        }
    }

    /// Whether a covering root is `dir`, whose hash is `hash`.
    fn covers(&self, dir: &Path, hash: PathHash) -> bool {
        self.covering
            .find(hash, |root| root.as_path() == dir)
            .is_some()
    }

    /// Whether a file walk is for `file`, a resolved path.
    fn is_file_root(&self, file: &HashedPath) -> bool {
        self.file_roles(file).is_some()
    }

    /// The roles of the file walks for `file`, a resolved path.
    fn file_roles(&self, file: &HashedPath) -> Option<&[Role]> {
        self.files
            .find(file.hash(), |(root, _)| root == file)
            .map(|(_, roles)| roles.as_slice())
    }

    /// Whether the explicit file `file` is listed by its own walk alone: no
    /// directory walk reaches its directory, and no other file walk is for it.
    fn file_alone(&self, file: &HashedPath) -> bool {
        file.parent()
            .is_some_and(|(dir, _)| self.reaching(dir).reach() == Reach::None)
            && self.file_roles(file).is_some_and(|roles| roles.len() == 1)
    }

    /// What reaches `dir`, as each ancestor of it reaches it and whether a
    /// covering root holds one, without what a walk carries down: the reading
    /// that `reaching` and `below` must agree with.
    #[cfg(test)]
    fn reach_by_ancestors(&self, dir: &Path) -> (Reach, bool) {
        let covered = dir
            .ancestors()
            .any(|ancestor| self.covers(ancestor, path_hash(ancestor)));
        let mut reach = Reach::None;
        // The names of the directories below the ancestor being checked, down to `dir`.
        let mut below: Vec<&OsStr> = Vec::new();
        for ancestor in dir.ancestors() {
            let hash = path_hash(ancestor);
            let starting = self
                .dirs
                .find(hash, |(start, _)| start.as_path() == ancestor)
                .map_or(&[][..], |(_, walks)| walks.as_slice());
            for &(role, excluded) in starting {
                if (covered && role == Role::Context)
                    || below.iter().any(|&name| excludes(excluded, name))
                {
                    continue;
                }
                reach = match reach {
                    Reach::None => Reach::Sole(role),
                    Reach::Sole(_) | Reach::Several => Reach::Several,
                };
            }
            if let Some(name) = ancestor.file_name() {
                below.push(name);
            }
        }
        (reach, covered)
    }
}

/// What `table` holds for `path`, made empty when it holds nothing.
fn held<'t, T: Default>(
    table: &'t mut hashbrown::HashTable<(HashedPath, T)>,
    path: &HashedPath,
) -> &'t mut T {
    let entry = match table.entry(
        path.hash(),
        |(held, _)| held == path,
        |(held, _)| held.hash(),
    ) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => entry.insert((path.clone(), T::default())).into_mut(),
    };
    &mut entry.1
}

/// What a walk lists, in listing order.
enum Found {
    /// A PHP file, under its path, with the package that owns its directory.
    File(HashedPath, Option<composer::PackageOwner>),
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

/// The directories that hold the listed files: a node for each directory with a
/// listed file, and for each directory above it, up to the top, with one slot
/// per entry of its listing. A depth-first walk of the nodes, in each listing's
/// order, lists the files in path order, with no comparison. A file has one
/// slot, so a file listed twice is merged there.
struct Tree {
    listings: Arc<DirListings>,
    nodes: Vec<Node>,
    /// The nodes, found by their directories' hashes.
    by_path: hashbrown::HashTable<usize>,
    /// The nodes whose directory has no parent, where a walk starts.
    tops: Vec<usize>,
    /// The listed files, each taken out once it is emitted.
    files: Vec<Option<Listed>>,
}

/// A directory with listed files, or one above such a directory.
struct Node {
    path: HashedPath,
    listing: Arc<Listing>,
    /// One slot for each entry of `listing`, in its order.
    slots: Vec<Slot>,
}

/// What an entry of a directory is, in the tree.
#[derive(Clone, Copy)]
enum Slot {
    Empty,
    /// A listed file, by its index in `Tree::files`.
    File(usize),
    /// A subdirectory, by its index in `Tree::nodes`.
    Dir(usize),
}

impl Tree {
    fn new(listings: Arc<DirListings>) -> Self {
        Tree {
            listings,
            nodes: Vec::new(),
            by_path: hashbrown::HashTable::new(),
            tops: Vec::new(),
            files: Vec::new(),
        }
    }

    /// Puts `listed` in the slot of its entry. A file already there keeps its
    /// slot, with the strongest role and the stub mark of its listings. Gives
    /// the file back when the tree cannot place it.
    fn attach(&mut self, listed: Listed) -> Result<(), Listed> {
        let Some((node, at)) = self.slot_of(&listed.path) else {
            return Err(listed);
        };
        match self.nodes[node].slots[at] {
            Slot::Empty => {
                self.nodes[node].slots[at] = Slot::File(self.files.len());
                self.files.push(Some(listed));
            }
            Slot::File(file) => {
                let kept = self.files[file]
                    .as_mut()
                    .expect("a listed file is kept until the walk ends");
                kept.role = kept.role.max(listed.role);
                kept.stub |= listed.stub;
            }
            Slot::Dir(_) => return Err(listed),
        }
        Ok(())
    }

    /// The node and slot of the entry at `path`, or `None` when its directory
    /// cannot be placed.
    fn slot_of(&mut self, path: &HashedPath) -> Option<(usize, usize)> {
        let (dir, dir_hash) = path.parent()?;
        let node = self.node(dir, dir_hash)?;
        let at = self.nodes[node]
            .listing
            .position(path.as_path().file_name()?)?;
        Some((node, at))
    }

    /// The node of the directory `dir`, whose hash is `hash`, made with the
    /// nodes above it when it is new. `None` when `dir` is relative, or when
    /// `dir` or a directory above it cannot be listed.
    fn node(&mut self, dir: &Path, hash: PathHash) -> Option<usize> {
        if !dir.is_absolute() {
            return None;
        }
        if let Some(node) = self.find(dir, hash) {
            return Some(node);
        }
        // The directories above it are looked up from its parent up, by the
        // hashes of one fold of its path, to the nearest one with a node; the
        // nodes below that one are made from the top down.
        let mut ancestors = hashed_ancestors(dir);
        let first = ancestors.next();
        debug_assert_eq!(first, Some((dir, hash)));
        let mut new = vec![dir];
        let mut up = None;
        for (ancestor, hash) in ancestors {
            if let Some(node) = self.find(ancestor, hash) {
                up = Some(node);
                break;
            }
            new.push(ancestor);
        }
        for dir in new.into_iter().rev() {
            up = Some(self.make(dir, up)?);
        }
        up
    }

    /// The node of `dir`, if it has one.
    fn find(&self, dir: &Path, hash: PathHash) -> Option<usize> {
        self.by_path
            .find(hash, |&node| self.nodes[node].path.as_path() == dir)
            .copied()
    }

    /// Makes the node of `dir`, in its slot of `up`, its parent's node, or as a
    /// top when it has no parent. `None` when `dir` cannot be listed, or when
    /// its slot holds something else.
    fn make(&mut self, dir: &Path, up: Option<usize>) -> Option<usize> {
        let (path, link) = match up {
            Some(up) => {
                let name = dir.file_name()?;
                let at = self.nodes[up].listing.position(name)?;
                if !matches!(self.nodes[up].slots[at], Slot::Empty) {
                    return None;
                }
                (self.nodes[up].path.child(name), Some((up, at)))
            }
            None => (HashedPath::new(dir.to_path_buf()), None),
        };
        let listing = self.listings.listing(&path)?;
        let node = self.nodes.len();
        let nodes = &self.nodes;
        self.by_path
            .insert_unique(path.hash(), node, |&node| nodes[node].path.hash());
        self.nodes.push(Node {
            path,
            slots: vec![Slot::Empty; listing.entries.len()],
            listing,
        });
        match link {
            Some((up, at)) => self.nodes[up].slots[at] = Slot::Dir(node),
            None => self.tops.push(node),
        }
        Some(node)
    }

    /// The listed files, in path order.
    fn emit(&mut self) -> Vec<Listed> {
        let mut tops = std::mem::take(&mut self.tops);
        tops.sort_by(|&a, &b| self.nodes[a].path.cmp(&self.nodes[b].path));
        let mut out = Vec::with_capacity(self.files.len());
        for top in tops {
            self.emit_node(top, &mut out);
        }
        out
    }

    /// Emits the files under `node`, in the order of its slots.
    fn emit_node(&mut self, node: usize, out: &mut Vec<Listed>) {
        for slot in std::mem::take(&mut self.nodes[node].slots) {
            match slot {
                Slot::Empty => {}
                Slot::File(file) => {
                    out.push(self.files[file].take().expect("a file is emitted once"));
                }
                Slot::Dir(child) => self.emit_node(child, out),
            }
        }
    }
}

/// Merges the files the tree could not place into `files`, which are in path
/// order, and sorts the result. Only a walk with such a file sorts.
fn merge_unplaced(mut files: Vec<Listed>, unplaced: Vec<Listed>) -> Vec<Listed> {
    files.extend(unplaced);
    composer::path_order::sort_by_path(&mut files, |listed| listed.path.as_path());
    let mut merged: Vec<Listed> = Vec::with_capacity(files.len());
    for listed in files {
        match merged.last_mut() {
            Some(last) if last.path == listed.path => {
                last.role = last.role.max(listed.role);
                last.stub |= listed.stub;
            }
            _ => merged.push(listed),
        }
    }
    merged
}

impl<'a, 'p> Walk<'a, 'p> {
    /// A walk that reads its directories through `listings`, and finds each
    /// directory's owner in `packages`.
    pub(crate) fn new(listings: Arc<DirListings>, packages: &'p composer::PackageIndex) -> Self {
        Walk {
            roots: Vec::new(),
            jobs: Vec::new(),
            starts: Starts::default(),
            targets: Default::default(),
            tree: Tree::new(Arc::clone(&listings)),
            unplaced: Vec::new(),
            directories: Default::default(),
            listings,
            packages,
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
                path: HashedPath::new(identity),
                scope,
            });
        } else if is_php(&identity) {
            self.jobs.push(Job::File {
                path: HashedPath::new(identity),
                role,
            });
        }
    }

    /// Adds `path` as a root and lists it whatever its extension, as an
    /// explicitly configured file is listed.
    pub(crate) fn add_file(&mut self, path: &Path, role: Role) {
        let identity = self.resolve_file(path);
        self.roots.push(Root {
            path: identity.as_path().to_path_buf(),
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
    /// `sink` takes the files whose roles are final, as the walk lists them.
    pub(crate) fn finish<S: FinalFiles>(
        mut self,
        pool: &rayon::ThreadPool,
        sink: &S,
    ) -> Walked<'a> {
        let jobs = std::mem::take(&mut self.jobs);
        self.starts = Starts::of(&jobs, &self.roots);
        pool.install(|| {
            rayon::scope(|tasks| {
                let take = |files: Vec<Listed>| sink.take(tasks, files);
                let take: &Take<'_> = &take;
                let found: Vec<Vec<Found>> = jobs
                    .par_iter()
                    .map(|job| self.run(job, Some(take)))
                    .collect();
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
            })
        });
        let Walk {
            mut tree,
            unplaced,
            roots,
            ..
        } = self;
        let files = tree.emit();
        let files = if unplaced.is_empty() {
            files
        } else {
            merge_unplaced(files, unplaced)
        };
        debug_assert!(
            files.windows(2).all(|pair| pair[0].path < pair[1].path),
            "a walk lists each file once, in path order"
        );
        let index = FileIndex::of(&files);
        Walked {
            files,
            index,
            roots,
        }
    }

    /// The package owning the resolved file `path`: the package of its directory.
    fn owner_of(&self, path: &Path) -> Option<composer::PackageOwner> {
        self.packages.owner_of_dir(path.parent()?)
    }

    /// What the walk of one root lists. The files it lists alone are handed to
    /// `take`, when it is given.
    fn run(&self, job: &Job<'a>, take: Option<&Take<'_>>) -> Vec<Found> {
        match job {
            Job::Dir { path, scope } => self.walk_root(path, *scope, take),
            Job::File { path, role } => {
                let owner = self.owner_of(path.as_path());
                if let Some(take) = take
                    && self.starts.file_alone(path)
                {
                    take(vec![Listed {
                        path: path.clone(),
                        role: *role,
                        stub: *role == Role::Stub,
                        owner: Some(owner),
                    }]);
                }
                vec![Found::File(path.clone(), owner)]
            }
        }
    }

    /// What the walk of a link's target lists: the files under a directory, or
    /// the target itself when it is a PHP file. Nothing is handed over: a file
    /// outside every root can be listed by a link that another walk meets, so
    /// its role is not final until the walk ends.
    fn walk_target(&self, target: &Path, scope: Scope<'a>) -> Vec<Found> {
        if target.is_dir() {
            self.walk_root(&HashedPath::new(target.to_path_buf()), scope, None)
        } else if is_php(target) {
            vec![Found::File(
                HashedPath::new(target.to_path_buf()),
                self.owner_of(target),
            )]
        } else {
            Vec::new()
        }
    }

    /// Walks the directory `path`, a resolved path, from the top. What reaches
    /// it is found for a root's walk, which can hand files over or be left
    /// out, and not for a link target's, which does neither.
    fn walk_root(
        &self,
        path: &HashedPath,
        scope: Scope<'a>,
        take: Option<&Take<'_>>,
    ) -> Vec<Found> {
        let reaching = (!scope.outside).then(|| self.starts.reaching(path.as_path()));
        if reaching
            .as_ref()
            .is_some_and(|reaching| scope.prunes(reaching))
        {
            return Vec::new();
        }
        let Ok(handle) = Handle::from_path(path) else {
            return Vec::new();
        };
        self.walk_dir(
            path,
            reaching.as_ref(),
            scope,
            &Ancestors {
                handle: &handle,
                parent: None,
            },
            take,
        )
    }

    /// Lists what `dir`, a resolved directory that `reaching` reaches, holds
    /// under `scope`, in listing order. A subdirectory is walked by a task of
    /// its own, and its results are placed back among the entries in order.
    /// `ancestors` holds `dir` and the directories above it in this walk. The
    /// files of `dir` that only this walk lists are handed to `take` as one
    /// batch, once its entries are read.
    fn walk_dir(
        &self,
        dir: &HashedPath,
        reaching: Option<&Reaching<'a>>,
        scope: Scope<'a>,
        ancestors: &Ancestors<'_>,
        take: Option<&Take<'_>>,
    ) -> Vec<Found> {
        let Some(listing) = self.listings.listing(dir) else {
            return Vec::new();
        };
        #[cfg(test)]
        if let Some(reaching) = reaching {
            assert_eq!(
                (reaching.reach(), reaching.covered),
                self.starts.reach_by_ancestors(dir.as_path()),
                "what reaches {}",
                dir.as_path().display()
            );
        }
        // The files listed here are all in `dir`, so they share its owner.
        let owner = self.packages.owner_of_dir(dir.as_path());
        // The role of the files of `dir` when one walk reaches it.
        let sole = match reaching.map(Reaching::reach) {
            Some(Reach::Sole(role)) if take.is_some() => Some(role),
            _ => None,
        };
        let mut batch = Vec::new();
        let mut steps = Vec::with_capacity(listing.entries.len());
        let mut dirs = Vec::new();
        for entry in &listing.entries {
            // An entry whose type cannot be read is not listed.
            let Some(file_type) = entry.kind else {
                continue;
            };
            let name = entry.name.as_os_str();
            // A child's path is built only when the walk needs it.
            if file_type.is_symlink() {
                if excludes(scope.excluded, name) {
                    continue;
                }
                let child = dir.child(name);
                match fs::canonicalize(&child) {
                    Ok(target) => steps.push(Step::Found(Found::Link(target))),
                    // A dangling `.php` link is listed under its own path, so
                    // that reading it reports that path.
                    Err(_)
                        if is_php(child.as_path())
                            && !(scope.outside && self.under_root(child.as_path())) =>
                    {
                        steps.push(Step::Found(Found::File(child, owner)));
                    }
                    Err(_) => {}
                }
            } else if file_type.is_dir() {
                if excludes(scope.excluded, name) {
                    continue;
                }
                let child = dir.child(name);
                if scope.outside && self.under_root(child.as_path()) {
                    continue;
                }
                let below = reaching
                    .map(|reaching| self.starts.below(reaching, child.as_path(), child.hash()));
                if below.as_ref().is_some_and(|below| scope.prunes(below)) {
                    continue;
                }
                steps.push(Step::Dir);
                dirs.push((child, below));
            } else if is_php(Path::new(name)) {
                let child = dir.child(name);
                if !(scope.outside && self.under_root(child.as_path())) {
                    if let Some(role) = sole
                        && !self.starts.is_file_root(&child)
                    {
                        batch.push(Listed {
                            path: child.clone(),
                            role,
                            stub: role == Role::Stub,
                            owner: Some(owner),
                        });
                    }
                    steps.push(Step::Found(Found::File(child, owner)));
                }
            }
        }
        if let Some(take) = take
            && !batch.is_empty()
        {
            take(batch);
        }
        let subtrees: Vec<Vec<Found>> = dirs
            .par_iter()
            .map(|(child, below)| {
                self.walk_subdirectory(child, below.as_ref(), scope, ancestors, take)
            })
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

    /// Walks the subdirectory `child`, which `reaching` reaches, unless it is
    /// one of `ancestors`, which ends the loop. The subdirectory's handle is
    /// opened here, so only the directories being walked hold one.
    fn walk_subdirectory(
        &self,
        child: &HashedPath,
        reaching: Option<&Reaching<'a>>,
        scope: Scope<'a>,
        ancestors: &Ancestors<'_>,
        take: Option<&Take<'_>>,
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
            reaching,
            scope,
            &Ancestors {
                handle: &handle,
                parent: Some(ancestors),
            },
            take,
        )
    }

    /// Records what a walk listed, in the order it listed it: each file under
    /// the walk's role, and each link's target queued with the walk's role and
    /// names.
    fn merge(&mut self, found: Vec<Found>, role: Role, excluded: &'a [&'a str]) {
        for item in found {
            match item {
                Found::File(path, owner) => self.place(path, role, owner),
                Found::Link(target) => self.targets[role.index()].push_back((target, excluded)),
            }
        }
    }

    /// Places a listed file at its resolved path, whose owner is `owner`, under
    /// the walk's role. A file listed again keeps the strongest role of its
    /// listings; its owner is the same, as the owner depends on the path. A
    /// file the tree cannot place is kept aside.
    fn place(&mut self, path: HashedPath, role: Role, owner: Option<composer::PackageOwner>) {
        let listed = Listed {
            path,
            role,
            stub: role == Role::Stub,
            owner: Some(owner),
        };
        if let Err(listed) = self.tree.attach(listed) {
            self.unplaced.push(listed);
        }
    }

    /// The resolved path of the explicitly configured file `path`, as `resolve`
    /// gives it. A regular file's path is its directory's resolved path, which
    /// is resolved once per walk, with the file's name joined to it. A link, a
    /// path ending in `.` or `..`, and a path that does not exist are left to
    /// `resolve`.
    fn resolve_file(&mut self, path: &Path) -> HashedPath {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return HashedPath::new(resolve(path));
        };
        if !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            return HashedPath::new(resolve(path));
        }
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        match self.directories.entry_ref(parent) {
            EntryRef::Occupied(entry) => entry.get().child(name),
            EntryRef::Vacant(entry) => entry.insert(HashedPath::new(resolve(parent))).child(name),
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

/// Whether `name` is one of `excluded`.
fn excludes(excluded: &[&str], name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| excluded.contains(&name))
}

fn is_php(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "php")
}
