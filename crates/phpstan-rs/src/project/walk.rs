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
//! A file is final as soon as the walk that lists it is the one walk of the
//! strongest role that reaches it: a weaker walk does not change its role,
//! and the stub walks that reach it, which mark it, are known. Such a file is
//! handed to the sink by that walk, with the files of its directory, while
//! the walk goes on, so it can be parsed at once. A file that two walks of its
//! strongest role reach, or that only a link reaches, is handed over by no
//! one: its role is known when the walk ends.
//!
//! A walk of a role above context is decided by the roots of such roles
//! alone, so the context roots can be given while those walks run
//! (`Walk::finish_with`); the context walks start once every root is known.
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
#[cfg(test)]
use std::sync::Mutex;
use std::sync::{Arc, OnceLock};

use composer::dir_listings::{DirListings, Listing};
use composer::hashed_path::{HashedPath, PathHash, Resolved, hashed_ancestors, path_hash};
use composer::manifest::resolve;
use diagnostics::CaseRule;
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

/// The directory names a walk leaves out below a root, and the case rule that
/// compares the letters of a listed name with theirs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Excluded<'a> {
    names: &'a [&'a str],
    case: CaseRule,
    /// Whether the walk also leaves out a directory that Composer wrote into.
    composer_vendor: bool,
}

impl<'a> Excluded<'a> {
    pub(crate) fn new(names: &'a [&'a str], case: CaseRule) -> Self {
        Excluded {
            names,
            case,
            composer_vendor: false,
        }
    }

    /// The same exclusions, which also leave out the directories Composer
    /// wrote into.
    pub(crate) fn leaving_out_composer(mut self) -> Self {
        self.composer_vendor = true;
        self
    }

    /// Whether the walk leaves out a directory that Composer wrote into.
    pub(crate) fn leaves_out_composer(&self) -> bool {
        self.composer_vendor
    }

    /// The case rule the names compare under.
    pub(crate) fn case(&self) -> CaseRule {
        self.case
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.names.is_empty() && !self.composer_vendor
    }

    /// Whether `name` is one of the names.
    pub(crate) fn excludes(&self, name: &OsStr) -> bool {
        let Some(name) = name.to_str() else {
            return false;
        };
        match self.case {
            CaseRule::Exact => self.names.contains(&name),
            CaseRule::Folded => {
                let key = self.case.key(name);
                self.names
                    .iter()
                    .any(|excluded| self.case.key(excluded) == key)
            }
        }
    }
}

/// Whether `dir` is one of `seeds`, the directories of the packages Composer
/// installed outside their vendor directory (sorted by path), compared as
/// `case` compares names.
pub(crate) fn is_seed(seeds: &[PathBuf], case: CaseRule, dir: &Path) -> bool {
    if seeds.is_empty() {
        return false;
    }
    match case {
        CaseRule::Exact => seeds
            .binary_search_by(|seed| seed.as_path().cmp(dir))
            .is_ok(),
        CaseRule::Folded => {
            let dir = dir.to_string_lossy();
            let key = case.key(&dir);
            seeds.iter().any(|seed| {
                let seed = seed.to_string_lossy();
                case.key(&seed) == key
            })
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
    pub(crate) excluded: Excluded<'a>,
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
    /// Takes `files`, a directory's batch in listing order. Each file is one
    /// whose role this walk makes final, as the one walk of the file's
    /// strongest role, with the role and stub mark it has in the walk's
    /// listing. A task spawned on `scope` handles the batch, and the walk does
    /// not return until the task is done.
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
    /// The roots given before the walk starts.
    given: Roots<'a>,
    /// Every root the walk was given, once it has started.
    roots: Vec<Root<'a>>,
    /// The walks the roots of the roles above context start, by the path each
    /// starts from, so that a file is known to be listed by one walk of its
    /// strongest role before the walks run. Those walks are decided by these.
    starts: Starts<'a>,
    /// The walks every root starts, for the context walks: known once the
    /// context roots are.
    every_start: OnceLock<Starts<'a>>,
    /// The context walks: those of the context roots given before the walk
    /// started, then those of the ones given while the stronger walks ran.
    context_jobs: OnceLock<Vec<Job<'a>>>,
    /// The targets of the links met so far, by the role of the walk that met
    /// each link, with the names that walk excludes.
    targets: [VecDeque<(PathBuf, Excluded<'a>)>; 3],
    /// The directories that hold the listed files, with the files in them.
    tree: Tree,
    /// The listed files the tree cannot place: a file whose directory, or one
    /// of the directories above it, cannot be listed.
    unplaced: Vec<Listed>,
    /// The listings of the directories the walk reads. A load shares them with
    /// its vendor discovery, so each directory is read once.
    listings: Arc<DirListings>,
    /// The packages whose directories give each listed file its owner.
    packages: &'p composer::PackageIndex,
    /// Whether a directory is Composer's, for the roots that leave those out.
    composer: Option<&'p (dyn Fn(&Path) -> bool + Sync)>,
    /// How the letters of the names a root excludes compare with a listed
    /// name's.
    path_case: CaseRule,
    /// The directories of the packages Composer installed outside their
    /// project's vendor directory, sorted by path: a walk that leaves out
    /// Composer's directories leaves these out as well.
    seeds: Arc<[PathBuf]>,
}

/// Roots given to a walk, with the walk of each, in the order they were
/// given.
#[derive(Default)]
struct Roots<'a> {
    roots: Vec<Root<'a>>,
    jobs: Vec<Job<'a>>,
    /// The resolved path of each directory an explicit file was listed in, by
    /// the directory as it was given.
    directories: hashbrown::HashMap<PathBuf, HashedPath, FxBuildHasher>,
    /// The `Job::Files` among `jobs`, by their index there, found by the hash
    /// of their directory.
    file_jobs: hashbrown::HashTable<usize>,
}

/// The context roots a walk is given while its stronger walks run, which the
/// context walks start from.
pub(crate) struct ContextRoots<'a>(Roots<'a>);

/// The walk of one root, a directory, or of the file roots of one role in one
/// directory, each of which is listed whatever its extension. The files of a
/// directory share what reaches it and the package that owns it.
enum Job<'a> {
    Dir { path: HashedPath, scope: Scope<'a> },
    Files { files: Vec<HashedPath>, role: Role },
}

impl Job<'_> {
    /// The role of the files the walk lists.
    fn role(&self) -> Role {
        match self {
            Job::Dir { scope, .. } => scope.role,
            Job::Files { role, .. } => *role,
        }
    }
}

/// What a walk applies to the entries it meets.
#[derive(Clone, Copy)]
struct Scope<'a> {
    /// The role of the files the walk lists.
    role: Role,
    /// The directory names the walk leaves out below its directory.
    excluded: Excluded<'a>,
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

/// What the walks that list a file decide for it: the strongest of their
/// roles, and whether one walk alone has it; and whether a stub walk is among
/// them, which marks the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Reach {
    strongest: Option<Strongest>,
    stub: bool,
}

/// The strongest role of some walks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Strongest {
    role: Role,
    /// Whether one walk alone has the role.
    alone: bool,
}

impl Reach {
    /// Counts a walk of `role` in.
    fn add(&mut self, role: Role) {
        self.stub |= role == Role::Stub;
        self.strongest = Some(match self.strongest {
            Some(strongest) if strongest.role > role => strongest,
            Some(strongest) if strongest.role == role => Strongest { role, alone: false },
            _ => Strongest { role, alone: true },
        });
    }

    /// The stub mark of the files that a walk of `role` hands over, when it is
    /// the walk that hands them over: the one walk of the strongest role.
    fn hands_over(self, role: Role) -> Option<bool> {
        (self.strongest == Some(Strongest { role, alone: true })).then_some(self.stub)
    }
}

/// A directory walk: its role and the names it excludes.
type DirWalk<'a> = (Role, Excluded<'a>);

/// Whether a walk with the exclusions it has leaves a directory out (`Walk::leaves_out_dir`).
type Cut<'c, 'a> = &'c dyn Fn(Excluded<'a>, &Path) -> bool;

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
    /// What the directory walks that list the files of the directory decide
    /// for them. A context walk lists nothing under a covering root
    /// (`prunes`). A walk that cannot read a directory is counted too, which
    /// only keeps a file from being handed over.
    fn reach(&self) -> Reach {
        let mut reach = Reach::default();
        for &(role, _) in &self.walks {
            if !(self.covered && role == Role::Context) {
                reach.add(role);
            }
        }
        reach
    }
}

impl<'a> Starts<'a> {
    fn of<'j>(
        jobs: impl IntoIterator<Item = &'j Job<'a>>,
        roots: impl IntoIterator<Item = &'j Root<'a>>,
    ) -> Self
    where
        'a: 'j,
    {
        let mut starts = Starts::default();
        for job in jobs {
            match job {
                Job::Dir { path, scope } => {
                    held(&mut starts.dirs, path).push((scope.role, scope.excluded));
                }
                Job::Files { files, role } => {
                    for path in files {
                        held(&mut starts.files, path).push(*role);
                    }
                }
            }
        }
        for root in roots
            .into_iter()
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
    /// passes down, from the top. `cut` says which directories a walk leaves
    /// out for Composer (`Walk::leaves_out_dir`).
    fn reaching(&self, dir: &Path, cut: Cut<'_, 'a>) -> Reaching<'a> {
        hashed_ancestors(dir)
            .rev()
            .fold(Reaching::default(), |above, (ancestor, hash, name)| {
                self.below(&above, ancestor, hash, name, cut)
            })
    }

    /// What reaches `dir`, whose hash is `hash` and whose name is `name`, a
    /// subdirectory of a directory that `above` reaches: the walks that reach
    /// that directory and neither exclude `name` nor leave `dir` out, and the
    /// walks that start at `dir`.
    fn below(
        &self,
        above: &Reaching<'a>,
        dir: &Path,
        hash: PathHash,
        name: Option<&OsStr>,
        cut: Cut<'_, 'a>,
    ) -> Reaching<'a> {
        let mut walks: Vec<DirWalk<'a>> = above
            .walks
            .iter()
            .copied()
            .filter(|&(_, excluded)| {
                !name.is_some_and(|name| excluded.excludes(name)) && !cut(excluded, dir)
            })
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

    /// The roles of the file walks for `file`, a resolved path.
    fn file_roles(&self, file: &HashedPath) -> &[Role] {
        self.files
            .find(file.hash(), |(root, _)| root == file)
            .map_or(&[], |(_, roles)| roles.as_slice())
    }

    /// What the walks that list `file`, a file in a directory that `dir_reach`
    /// is the reach of, decide for it: those directory walks and the file
    /// walks for it.
    fn with_file_walks(&self, mut dir_reach: Reach, file: &HashedPath) -> Reach {
        for &role in self.file_roles(file) {
            dir_reach.add(role);
        }
        dir_reach
    }

    /// What reaches `dir`, as each ancestor of it reaches it and whether a
    /// covering root holds one, without what a walk carries down: the reading
    /// that `reaching` and `below` must agree with.
    #[cfg(test)]
    fn reach_by_ancestors(&self, dir: &Path, cut: Cut<'_, 'a>) -> (Reach, bool) {
        let covered = dir
            .ancestors()
            .any(|ancestor| self.covers(ancestor, path_hash(ancestor)));
        let mut reach = Reach::default();
        // The names and paths of the directories below the ancestor being
        // checked, down to `dir`.
        let mut below: Vec<(&OsStr, &Path)> = Vec::new();
        for ancestor in dir.ancestors() {
            let hash = path_hash(ancestor);
            let starting = self
                .dirs
                .find(hash, |(start, _)| start.as_path() == ancestor)
                .map_or(&[][..], |(_, walks)| walks.as_slice());
            for &(role, excluded) in starting {
                if (covered && role == Role::Context)
                    || below
                        .iter()
                        .any(|&(name, path)| excluded.excludes(name) || cut(excluded, path))
                {
                    continue;
                }
                reach.add(role);
            }
            if let Some(name) = ancestor.file_name() {
                below.push((name, ancestor));
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
        let (dir, dir_hash, name) = path.split()?;
        let node = self.node(dir, dir_hash)?;
        let at = self.nodes[node].listing.position(name)?;
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
        let (first, first_hash, name) = ancestors.next()?;
        debug_assert_eq!((first, first_hash), (dir, hash));
        let mut new = vec![(dir, name)];
        let mut up = None;
        for (ancestor, hash, name) in ancestors {
            if let Some(node) = self.find(ancestor, hash) {
                up = Some(node);
                break;
            }
            new.push((ancestor, name));
        }
        for (dir, name) in new.into_iter().rev() {
            up = Some(self.make(dir, name, up)?);
        }
        up
    }

    /// The node of `dir`, if it has one.
    fn find(&self, dir: &Path, hash: PathHash) -> Option<usize> {
        self.by_path
            .find(hash, |&node| self.nodes[node].path.as_path() == dir)
            .copied()
    }

    /// Makes the node of `dir`, whose name is `name`, in its slot of `up`, its
    /// parent's node, or as a top when it has no parent. `None` when `dir`
    /// cannot be listed, or when its slot holds something else.
    fn make(&mut self, dir: &Path, name: Option<&OsStr>, up: Option<usize>) -> Option<usize> {
        let (path, link) = match up {
            Some(up) => {
                let name = name?;
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
        stack::guard(|| {
            for slot in std::mem::take(&mut self.nodes[node].slots) {
                match slot {
                    Slot::Empty => {}
                    Slot::File(file) => {
                        out.push(self.files[file].take().expect("a file is emitted once"));
                    }
                    Slot::Dir(child) => self.emit_node(child, out),
                }
            }
        })
    }
}

/// Asserts that each file in `handed` was handed over once, as `files`, the
/// listing, holds it: with its final role, stub mark and owner.
#[cfg(test)]
fn assert_handed_as_listed(mut handed: Vec<Listed>, files: &[Listed], index: &FileIndex) {
    handed.sort_by(|a, b| a.path.cmp(&b.path));
    for pair in handed.windows(2) {
        assert!(
            pair[0].path != pair[1].path,
            "{} is handed over twice",
            pair[0].path.as_path().display()
        );
    }
    for file in &handed {
        let listed = index
            .position(files, file.path.as_path())
            .map(|at| &files[at]);
        assert_eq!(listed, Some(file), "a file is handed over as it is listed");
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

impl<'a> Roots<'a> {
    /// Adds `path` as a root. A directory is walked; a file is listed when it
    /// is PHP. The names in `excluded` apply below the root: the walk does not
    /// enter a directory of such a name, nor follow a link of it, and the root
    /// itself is walked whatever its name.
    fn add(&mut self, path: &Path, role: Role, excluded: Excluded<'a>) {
        let identity = Resolved::resolve(path);
        if identity.as_path().is_dir() {
            self.add_resolved_dir(identity, role, excluded);
            return;
        }
        self.roots.push(Root {
            path: identity.as_path().to_path_buf(),
            role,
            stub: role == Role::Stub,
            excluded,
        });
        if is_php(identity.as_path()) {
            self.push_file(identity.into_hashed(), role);
        }
    }

    /// Adds `dir`, a resolved directory, as a root, as `add` adds one, without
    /// resolving it again or asking the system what it is.
    fn add_resolved_dir(&mut self, dir: Resolved, role: Role, excluded: Excluded<'a>) {
        self.roots.push(Root {
            path: dir.as_path().to_path_buf(),
            role,
            stub: role == Role::Stub,
            excluded,
        });
        let scope = Scope {
            role,
            excluded,
            outside: false,
        };
        self.jobs.push(Job::Dir {
            path: dir.into_hashed(),
            scope,
        });
    }

    /// Adds `file`, a resolved regular file, as a root, as `add_file` adds
    /// one, without resolving it again.
    fn add_resolved_file(&mut self, file: Resolved, role: Role) {
        self.roots.push(Root {
            path: file.as_path().to_path_buf(),
            role,
            stub: role == Role::Stub,
            excluded: Excluded::default(),
        });
        self.push_file(file.into_hashed(), role);
    }

    /// Adds `path` as a root and lists it whatever its extension, as an
    /// explicitly configured file is listed.
    fn add_file(&mut self, path: &Path, role: Role) {
        let identity = self.resolve_file(path);
        self.roots.push(Root {
            path: identity.as_path().to_path_buf(),
            role,
            stub: role == Role::Stub,
            excluded: Excluded::default(),
        });
        self.push_file(identity, role);
    }

    /// Lists the explicit file `file` under `role`, in the job of the files of
    /// its directory and role, which it starts when it is the first.
    fn push_file(&mut self, file: HashedPath, role: Role) {
        let Roots {
            jobs, file_jobs, ..
        } = self;
        let Some((dir, hash)) = file.parent() else {
            jobs.push(Job::Files {
                files: vec![file],
                role,
            });
            return;
        };
        let same = |job: &Job<'_>| match job {
            Job::Files { files, role: held } => {
                *held == role && files[0].parent().is_some_and(|(held, _)| held == dir)
            }
            Job::Dir { .. } => false,
        };
        if let Some(&at) = file_jobs.find(hash, |&at| same(&jobs[at]))
            && let Job::Files { files, .. } = &mut jobs[at]
        {
            files.push(file);
            return;
        }
        file_jobs.insert_unique(hash, jobs.len(), |&at| match &jobs[at] {
            Job::Files { files, .. } => files[0].parent().map_or(0, |(_, hash)| hash),
            Job::Dir { .. } => 0,
        });
        jobs.push(Job::Files {
            files: vec![file],
            role,
        });
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
}

impl<'a> ContextRoots<'a> {
    /// Adds the context directory or file `path`, as `Walk::add` adds one.
    pub(crate) fn add(&mut self, path: &Path) {
        self.0.add(path, Role::Context, Excluded::default());
    }

    /// Adds the resolved context directory `dir`, as
    /// `Walk::add_resolved_dir` adds one.
    pub(crate) fn add_resolved_dir(&mut self, dir: Resolved) {
        self.0
            .add_resolved_dir(dir, Role::Context, Excluded::default());
    }

    /// Adds the resolved context file `file`, as `Walk::add_resolved_file`
    /// adds one.
    pub(crate) fn add_resolved_file(&mut self, file: Resolved) {
        self.0.add_resolved_file(file, Role::Context);
    }

    /// Adds the context file `path`, as `Walk::add_file` adds one.
    pub(crate) fn add_file(&mut self, path: &Path) {
        self.0.add_file(path, Role::Context);
    }
}

/// Where a walk hands over the files whose roles are final: its sink, and in
/// tests a record of them, which the listing is checked against.
struct Hand<'s, S> {
    sink: &'s S,
    #[cfg(test)]
    handed: Mutex<Vec<Listed>>,
}

impl<'s, S: FinalFiles> Hand<'s, S> {
    fn take(&'s self, scope: &rayon::Scope<'s>, files: Vec<Listed>) {
        #[cfg(test)]
        self.handed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(files.iter().cloned());
        self.sink.take(scope, files);
    }
}

impl<'a, 'p> Walk<'a, 'p> {
    /// A walk that reads its directories through `listings`, and finds each
    /// directory's owner in `packages`.
    pub(crate) fn new(listings: Arc<DirListings>, packages: &'p composer::PackageIndex) -> Self {
        Walk {
            given: Roots::default(),
            roots: Vec::new(),
            starts: Starts::default(),
            every_start: OnceLock::new(),
            context_jobs: OnceLock::new(),
            targets: Default::default(),
            tree: Tree::new(Arc::clone(&listings)),
            unplaced: Vec::new(),
            listings,
            packages,
            composer: None,
            path_case: CaseRule::Exact,
            seeds: Arc::from(Vec::new()),
        }
    }

    /// The walk, comparing the letters of the names a root excludes with a
    /// listed name's as `case` compares them.
    pub(crate) fn with_path_case(mut self, case: CaseRule) -> Self {
        self.path_case = case;
        self
    }

    /// The walk, asking `composer` whether a directory is Composer's when a
    /// root leaves those out.
    pub(crate) fn with_composer(mut self, composer: &'p (dyn Fn(&Path) -> bool + Sync)) -> Self {
        self.composer = Some(composer);
        self
    }

    /// The walk, leaving out the directories `seeds` lists (sorted by path)
    /// when a root leaves out Composer's directories.
    pub(crate) fn with_seeds(mut self, seeds: Arc<[PathBuf]>) -> Self {
        self.seeds = seeds;
        self
    }

    /// Whether the walk leaves `dir` out for Composer: the walk's exclusions
    /// say so, and Composer wrote into `dir`, or `dir` is the directory of a
    /// package Composer installed outside its vendor directory.
    fn leaves_out_dir(&self, excluded: Excluded<'_>, dir: &Path) -> bool {
        excluded.leaves_out_composer()
            && (self.composer.is_some_and(|test| test(dir))
                || is_seed(&self.seeds, self.path_case, dir))
    }

    /// Whether a link to `target` is not followed: the same test as for a
    /// directory, and also a target below a seed.
    fn leaves_out_link(&self, excluded: Excluded<'_>, target: &Path) -> bool {
        excluded.leaves_out_composer()
            && (self.composer.is_some_and(|test| test(target))
                || target
                    .ancestors()
                    .any(|ancestor| is_seed(&self.seeds, self.path_case, ancestor)))
    }

    /// The directories below `root` that hold a `composer.json` file: the
    /// nested Composer projects of a degrade root, found before the walk so
    /// that their roots are known when it starts. The directories are those the
    /// walk enters: `excluded` leaves out the names it does, and the directories
    /// `leaves_out_dir` leaves out (Composer's, and the seeds) are left out as
    /// the walk leaves them; a link is followed once, to a target below `root`.
    /// Sorted.
    pub(crate) fn manifest_dirs(&self, root: &Path, excluded: Excluded<'_>) -> Vec<PathBuf> {
        let root = HashedPath::new(root.to_path_buf());
        let mut visited = rustc_hash::FxHashSet::<PathBuf>::default();
        visited.insert(root.as_path().to_path_buf());
        let mut pending = vec![root.clone()];
        let mut found = Vec::new();
        while let Some(dir) = pending.pop() {
            let Some(listing) = self.listings.listing(&dir) else {
                continue;
            };
            let has_manifest = listing.entries.iter().any(|entry| {
                entry.name.as_os_str() == "composer.json"
                    && entry.kind.is_some_and(|kind| !kind.is_dir())
            });
            if has_manifest && dir.as_path() != root.as_path() {
                found.push(dir.as_path().to_path_buf());
            }
            for entry in &listing.entries {
                // An entry whose type cannot be read is not entered, as the walk does.
                let Some(file_type) = entry.kind else {
                    continue;
                };
                let name = entry.name.as_os_str();
                if excluded.excludes(name) {
                    continue;
                }
                let child = dir.child(name);
                if file_type.is_symlink() {
                    let Ok(target) = composer::canonical::canonicalize(child.as_path()) else {
                        continue;
                    };
                    if target.starts_with(root.as_path())
                        && !self.leaves_out_link(excluded, &target)
                        && visited.insert(target.clone())
                    {
                        pending.push(HashedPath::new(target));
                    }
                } else if file_type.is_dir()
                    && !self.leaves_out_dir(excluded, child.as_path())
                    && visited.insert(child.as_path().to_path_buf())
                {
                    pending.push(child);
                }
            }
        }
        found.sort();
        found
    }

    /// Adds `path` as a root, walked by `finish`. A directory is walked; a file
    /// is listed when it is PHP. The names in `excluded` apply below the root:
    /// the walk does not enter a directory of such a name, nor follow a link of
    /// it, and the root itself is walked whatever its name.
    pub(crate) fn add(&mut self, path: &Path, role: Role, excluded: &'a [&'a str]) {
        self.given
            .add(path, role, Excluded::new(excluded, self.path_case));
    }

    /// Adds `path` as `add` does, and also leaves out the directories below it
    /// that Composer wrote into.
    pub(crate) fn add_leaving_out_composer(
        &mut self,
        path: &Path,
        role: Role,
        excluded: &'a [&'a str],
    ) {
        self.given.add(
            path,
            role,
            Excluded::new(excluded, self.path_case).leaving_out_composer(),
        );
    }

    /// Adds `dir`, a resolved directory, as a root, as `add` adds one, without
    /// resolving it again or asking the system what it is.
    #[cfg(test)]
    pub(crate) fn add_resolved_dir(&mut self, dir: Resolved, role: Role, excluded: &'a [&'a str]) {
        self.given
            .add_resolved_dir(dir, role, Excluded::new(excluded, self.path_case));
    }

    /// Adds `file`, a resolved regular file, as a root, as `add_file` adds
    /// one, without resolving it again.
    #[cfg(test)]
    pub(crate) fn add_resolved_file(&mut self, file: Resolved, role: Role) {
        self.given.add_resolved_file(file, role);
    }

    /// Adds `path` as a root and lists it whatever its extension, as an
    /// explicitly configured file is listed.
    #[cfg(test)]
    pub(crate) fn add_file(&mut self, path: &Path, role: Role) {
        self.given.add_file(path, role);
    }

    /// Walks the roots, then the targets no root holds, strongest role first,
    /// on `pool`, and returns the listed files with the roots they were given.
    /// A target is walked once for each set of excluded names that meets it.
    /// `sink` takes the files whose roles are final, as the walk lists them.
    pub(crate) fn finish<S: FinalFiles>(self, pool: &rayon::ThreadPool, sink: &S) -> Walked<'a> {
        self.finish_with(pool, sink, |_| {})
    }

    /// Walks the roots as `finish` does, with the context roots that `context`
    /// adds. `context` runs on the calling thread while the walks of the roots
    /// above context run, which no context root decides, and the context walks
    /// start once it returns.
    pub(crate) fn finish_with<S: FinalFiles>(
        mut self,
        pool: &rayon::ThreadPool,
        sink: &S,
        context: impl FnOnce(&mut ContextRoots<'a>),
    ) -> Walked<'a> {
        let Roots {
            mut roots, jobs, ..
        } = std::mem::take(&mut self.given);
        let (stronger, given_context): (Vec<Job<'a>>, Vec<Job<'a>>) =
            jobs.into_iter().partition(|job| job.role() > Role::Context);
        self.starts = Starts::of(&stronger, &roots);
        let hand = Hand {
            sink,
            #[cfg(test)]
            handed: Mutex::default(),
        };
        let mut later = ContextRoots(Roots::default());
        let stronger_found = OnceLock::new();
        let context_found = OnceLock::new();
        let walk = &self;
        pool.in_place_scope(|tasks| {
            tasks.spawn(|tasks| {
                let take = |files: Vec<Listed>| hand.take(tasks, files);
                let found: Vec<Vec<Found>> = stronger
                    .par_iter()
                    .map(|job| walk.run(job, Some(&take)))
                    .collect();
                let _ = stronger_found.set(found);
            });
            context(&mut later);
            let jobs = walk.context_jobs.get_or_init(|| {
                given_context
                    .into_iter()
                    .chain(std::mem::take(&mut later.0.jobs))
                    .collect()
            });
            let _ = walk.every_start.set(Starts::of(
                stronger.iter().chain(jobs),
                roots.iter().chain(&later.0.roots),
            ));
            tasks.spawn(|tasks| {
                let take = |files: Vec<Listed>| hand.take(tasks, files);
                let found: Vec<Vec<Found>> = jobs
                    .par_iter()
                    .map(|job| walk.run(job, Some(&take)))
                    .collect();
                let _ = context_found.set(found);
            });
        });
        roots.extend(later.0.roots);
        self.roots = roots;
        let context_jobs = self.context_jobs.take().unwrap_or_default();
        let found = stronger
            .iter()
            .zip(stronger_found.into_inner().unwrap_or_default())
            .chain(
                context_jobs
                    .iter()
                    .zip(context_found.into_inner().unwrap_or_default()),
            );
        for (job, found) in found {
            match job {
                Job::Dir { scope, .. } => self.merge(found, scope.role, scope.excluded),
                Job::Files { role, .. } => self.merge(found, *role, Excluded::default()),
            }
        }
        pool.install(|| {
            let mut walked: FxHashMap<PathBuf, Vec<Excluded<'a>>> = FxHashMap::default();
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
        #[cfg(test)]
        assert_handed_as_listed(
            hand.handed
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            &files,
            &index,
        );
        Walked {
            files,
            index,
            roots,
        }
    }

    /// The walks that a walk of `role` is decided by: for a role above
    /// context, those of the roots of such roles, which no context root
    /// changes; for a context walk, every root's.
    fn starts_for(&self, role: Role) -> &Starts<'a> {
        if role > Role::Context {
            &self.starts
        } else {
            self.every_start
                .get()
                .expect("a context walk starts once every root is known")
        }
    }

    /// The package owning the resolved file `path`: the package of its directory.
    fn owner_of(&self, path: &Path) -> Option<composer::PackageOwner> {
        self.packages.owner_of_dir(path_bytes::parent(path)?)
    }

    /// What the walk of one root lists. The files whose roles it makes final
    /// are handed to `take`, when it is given.
    fn run(&self, job: &Job<'a>, take: Option<&Take<'_>>) -> Vec<Found> {
        match job {
            Job::Dir { path, scope } => self.walk_root(path, *scope, take),
            Job::Files { files, role } => {
                let dir = files[0].parent().map(|(dir, _)| dir);
                let owner = dir.and_then(|dir| self.packages.owner_of_dir(dir));
                if let Some(take) = take {
                    let starts = self.starts_for(*role);
                    // What the directory walks that reach the directory decide.
                    let cut = |excluded, dir: &Path| self.leaves_out_dir(excluded, dir);
                    let reach =
                        dir.map_or_else(Reach::default, |dir| starts.reaching(dir, &cut).reach());
                    for path in files {
                        if let Some(stub) = starts.with_file_walks(reach, path).hands_over(*role) {
                            take(vec![Listed {
                                path: path.clone(),
                                role: *role,
                                stub,
                                owner: Some(owner),
                            }]);
                        }
                    }
                }
                files
                    .iter()
                    .map(|path| Found::File(path.clone(), owner))
                    .collect()
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
        let cut = |excluded, dir: &Path| self.leaves_out_dir(excluded, dir);
        let reaching =
            (!scope.outside).then(|| self.starts_for(scope.role).reaching(path.as_path(), &cut));
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
            self.packages.owner_at(path.as_path()),
        )
    }

    /// Lists what `dir`, a resolved directory that `reaching` reaches, holds
    /// under `scope`, in listing order. A subdirectory is walked by a task of
    /// its own, and its results are placed back among the entries in order.
    /// `ancestors` holds `dir` and the directories above it in this walk. The
    /// files of `dir` whose roles this walk makes final are handed to `take` as
    /// one batch, once its entries are read. `owner_at` is where `dir` stands
    /// among the packages' directories.
    fn walk_dir(
        &self,
        dir: &HashedPath,
        reaching: Option<&Reaching<'a>>,
        scope: Scope<'a>,
        ancestors: &Ancestors<'_>,
        take: Option<&Take<'_>>,
        owner_at: composer::OwnerAt<'p>,
    ) -> Vec<Found> {
        stack::guard(|| {
            let Some(listing) = self.listings.listing(dir) else {
                return Vec::new();
            };
            let starts = self.starts_for(scope.role);
            let cut = |excluded, dir: &Path| self.leaves_out_dir(excluded, dir);
            #[cfg(test)]
            if let Some(reaching) = reaching {
                assert_eq!(
                    (reaching.reach(), reaching.covered),
                    starts.reach_by_ancestors(dir.as_path(), &cut),
                    "what reaches {}",
                    dir.as_path().display()
                );
            }
            // The files listed here are all in `dir`, so they share its owner.
            let owner = owner_at.owner();
            #[cfg(test)]
            assert_eq!(
                owner,
                self.packages.owner_of_dir(dir.as_path()),
                "the owner of {}",
                dir.as_path().display()
            );
            // What the walks that reach `dir` decide for its files, when this walk
            // is the one walk of their strongest role.
            let reach = match reaching.map(Reaching::reach) {
                Some(reach) if take.is_some() && reach.hands_over(scope.role).is_some() => {
                    Some(reach)
                }
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
                    if scope.excluded.excludes(name) {
                        continue;
                    }
                    let child = dir.child(name);
                    match composer::canonical::canonicalize(child.as_path()) {
                        Ok(target) => {
                            if !self.leaves_out_link(scope.excluded, &target) {
                                steps.push(Step::Found(Found::Link(target)));
                            }
                        }
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
                    if scope.excluded.excludes(name) {
                        continue;
                    }
                    let child = dir.child(name);
                    if scope.outside && self.under_root(child.as_path()) {
                        continue;
                    }
                    if self.leaves_out_dir(scope.excluded, child.as_path()) {
                        continue;
                    }
                    let below = reaching.map(|reaching| {
                        starts.below(reaching, child.as_path(), child.hash(), Some(name), &cut)
                    });
                    if below.as_ref().is_some_and(|below| scope.prunes(below)) {
                        continue;
                    }
                    steps.push(Step::Dir);
                    dirs.push((child, below, owner_at.child(name)));
                } else if path_bytes::is_php_name(name) {
                    let child = dir.child(name);
                    if !(scope.outside && self.under_root(child.as_path())) {
                        if let Some(reach) = reach
                            && let Some(stub) =
                                starts.with_file_walks(reach, &child).hands_over(scope.role)
                        {
                            batch.push(Listed {
                                path: child.clone(),
                                role: scope.role,
                                stub,
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
                .map(|(child, below, owner_at)| {
                    self.walk_subdirectory(child, below.as_ref(), scope, ancestors, take, *owner_at)
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
        })
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
        owner_at: composer::OwnerAt<'p>,
    ) -> Vec<Found> {
        stack::guard(|| {
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
                owner_at,
            )
        })
    }

    /// Records what a walk listed, in the order it listed it: each file under
    /// the walk's role, and each link's target queued with the walk's role and
    /// names.
    fn merge(&mut self, found: Vec<Found>, role: Role, excluded: Excluded<'a>) {
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

    fn next_target(&mut self) -> Option<(Role, PathBuf, Excluded<'a>)> {
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

fn is_php(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "php")
}
