//! Where the file an `include` names can be.
//!
//! The analyser knows the project's files. The include path, the working
//! directory and the platform belong to the deployment, so a value is judged
//! against every one a run can have: it is missing only when no combination
//! finds a file in the project, and a combination that leaves the project, or
//! has a part this analysis cannot know, counts as finding it. A path leaves
//! the project when it lies outside the trees it reads, or below a directory
//! that a deployment builds and this checkout lacks, such as a Composer
//! project's `vendor` before `composer install`.
//!
//! The text of an include is read in two worlds, Unix's and Windows', and a
//! value found in either is found. A world fixes what separates names, what
//! makes a path absolute and which names are relative to the working
//! directory. PHP looks a name up in this order:
//!
//! - an absolute name is that file;
//! - a name that starts with `./` or `../` is looked up in the working
//!   directory only;
//! - any other name is looked up in each include path entry, then in the
//!   including file's directory, then in the working directory.
//!
//! A relative include path entry is relative to the working directory, and an
//! empty entry before another is the root of the file system. The include
//! path and the working directory are the deployment's start, which the
//! configuration declares, with what the code does to them:
//! `set_include_path()`, `ini_set()` and `chdir()`. A part that is not known
//! opens it: a name that depends on it is not judged.
//!
//! Nothing here reads a file system: whether a candidate is a file is asked
//! through a predicate.

use std::path::{Component, Path, PathBuf};

/// What `get_include_path()` is in the text of an expression: the include
/// path is not known until the whole project is read, and `IncludePath`
/// expands this into it.
pub(crate) const GET_INCLUDE_PATH: &str = "\u{1}get_include_path\u{1}";

/// The platform whose rules read the text of an include.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum World {
    Unix,
    Windows,
}

impl World {
    pub(crate) const ALL: [World; 2] = [World::Unix, World::Windows];

    /// The world of the machine that analyses.
    pub(crate) const fn machine() -> World {
        if cfg!(windows) {
            World::Windows
        } else {
            World::Unix
        }
    }

    /// The position of this world in `ALL`.
    pub(crate) const fn index(self) -> usize {
        match self {
            World::Unix => 0,
            World::Windows => 1,
        }
    }

    /// `PATH_SEPARATOR`.
    pub(crate) const fn path_separator(self) -> char {
        match self {
            World::Unix => ':',
            World::Windows => ';',
        }
    }

    /// `DIRECTORY_SEPARATOR`.
    pub(crate) const fn directory_separator(self) -> char {
        match self {
            World::Unix => '/',
            World::Windows => '\\',
        }
    }

    /// Whether `c` separates names in a path of this world.
    fn is_separator(self, c: char) -> bool {
        match self {
            World::Unix => c == '/',
            World::Windows => c == '/' || c == '\\',
        }
    }

    /// PHP's `dirname($text, $levels)` in this world.
    pub(crate) fn dirname(self, text: &str, levels: usize) -> String {
        let mut current = text.to_string();
        for _ in 0..levels {
            let next = self.dirname_once(&current);
            let shrank = next.len() < current.len();
            current = next;
            if !shrank {
                break;
            }
        }
        current
    }

    /// `zend_dirname`: the text without its last name and the separators
    /// around it, `.` when it has no separator, and the root when only the
    /// root is left. A drive is kept as it is.
    fn dirname_once(self, text: &str) -> String {
        let bytes = text.as_bytes();
        let drive_len = if self == World::Windows
            && bytes.len() >= 2
            && bytes[1] == b':'
            && bytes[0].is_ascii_alphabetic()
        {
            2
        } else {
            0
        };
        let (drive, rest) = text.split_at(drive_len);
        if rest.is_empty() {
            return drive.to_string();
        }
        let bytes = rest.as_bytes();
        let is_separator = |at: usize| self.is_separator(char::from(bytes[at]));
        let root = || format!("{drive}{}", self.directory_separator());
        let mut end = bytes.len();
        while end > 0 && is_separator(end - 1) {
            end -= 1;
        }
        if end == 0 {
            return root();
        }
        while end > 0 && !is_separator(end - 1) {
            end -= 1;
        }
        if end == 0 {
            return format!("{drive}.");
        }
        while end > 0 && is_separator(end - 1) {
            end -= 1;
        }
        if end == 0 {
            return root();
        }
        format!("{drive}{}", &rest[..end])
    }
}

/// The names of a path with `.` and empty names dropped. A `..` stays where
/// it is written: the file system takes it after the names before it, so
/// `link/../x` is not `x`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Names(Vec<String>);

impl Names {
    fn parse(world: World, text: &str) -> Names {
        Names(
            text.split(|c| world.is_separator(c))
                .filter(|part| !matches!(*part, "" | "."))
                .map(str::to_string)
                .collect(),
        )
    }
}

/// Where an absolute path starts.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Anchor {
    /// The root of the file system, or of the current drive.
    Root,
    /// A drive: `C:\`.
    Drive(char),
    /// A share: `\\server\share\`, as `server\share`.
    Share(String),
}

/// What the text of an include names, in a world.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    /// Not a path in a file system this analysis can see: a stream wrapper's
    /// URL, or a path relative to a drive's own working directory.
    Elsewhere,
    Absolute(Anchor, Names),
    /// `./x` or `../x`: relative to the working directory.
    Relative(Names),
    /// Any other name.
    Plain(Names),
}

/// A stream wrapper's URL (`phar://...`, `data://...`).
fn is_stream_wrapper(text: &str) -> bool {
    text.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// What `text` names, in `world`.
fn classify(world: World, text: &str) -> Target {
    if is_stream_wrapper(text) {
        return Target::Elsewhere;
    }
    let mut chars = text.chars();
    let (first, second, third) = (chars.next(), chars.next(), chars.next());
    match (world, first) {
        (World::Unix, Some('/')) => Target::Absolute(Anchor::Root, Names::parse(world, text)),
        (World::Windows, Some(c)) if world.is_separator(c) => {
            if second.is_some_and(|c| world.is_separator(c)) {
                share(text)
            } else {
                Target::Absolute(Anchor::Root, Names::parse(world, text))
            }
        }
        (World::Windows, Some(letter)) if letter.is_ascii_alphabetic() && second == Some(':') => {
            if third.is_some_and(|c| world.is_separator(c)) {
                Target::Absolute(
                    Anchor::Drive(letter.to_ascii_uppercase()),
                    Names::parse(world, &text[2..]),
                )
            } else {
                Target::Elsewhere
            }
        }
        _ => {
            let first_name = text.split(|c| world.is_separator(c)).next().unwrap_or("");
            let relative = matches!(first_name, "." | "..") && text.len() > first_name.len();
            let names = Names::parse(world, text);
            if relative {
                Target::Relative(names)
            } else {
                Target::Plain(names)
            }
        }
    }
}

/// `text`, which starts with two separators, as a share and what lies in it.
fn share(text: &str) -> Target {
    let mut names = Names::parse(World::Windows, text);
    let inside = names.0.split_off(names.0.len().min(2));
    Target::Absolute(Anchor::Share(names.0.join("\\")), Names(inside))
}

/// The directory an include path entry names: the root for an empty entry.
fn entry_target(world: World, entry: &str) -> Target {
    if entry.is_empty() {
        Target::Absolute(Anchor::Root, Names::default())
    } else {
        classify(world, entry)
    }
}

/// The path on the analysing machine that `anchor` and `names` of `world`
/// name, if the machine has one. `root` is the project's, which gives the
/// current drive on Windows.
fn native_absolute(
    world: World,
    machine: World,
    root: Option<&Path>,
    anchor: &Anchor,
    names: &Names,
) -> Option<PathBuf> {
    let mut path = match (machine, world, anchor) {
        (World::Unix, _, Anchor::Root) => PathBuf::from("/"),
        (World::Windows, World::Windows, Anchor::Root) => match root?.components().next() {
            Some(Component::Prefix(prefix)) => {
                PathBuf::from(format!("{}\\", prefix.as_os_str().to_string_lossy()))
            }
            _ => return None,
        },
        (World::Windows, World::Windows, Anchor::Drive(letter)) => {
            PathBuf::from(format!("{letter}:\\"))
        }
        (World::Windows, World::Windows, Anchor::Share(share)) => {
            PathBuf::from(format!("\\\\{share}\\"))
        }
        _ => return None,
    };
    path.extend(&names.0);
    Some(path)
}

/// `names` below `base`, as the file system takes them.
fn join_names(base: &Path, names: &Names) -> PathBuf {
    let mut path = base.to_path_buf();
    path.extend(&names.0);
    path
}

/// `path` with its `.` and `..` taken out by their text alone: the place the
/// text says, which is what decides whether it lies in a tree.
fn folded(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The include path a run can have.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IncludePath {
    /// The deployment's `include_path`, in its syntax. `None` when it is not
    /// declared, and the start of the path is then anything.
    pub(crate) declared: Option<String>,
    /// The directories that Composer's autoloader puts in front of it,
    /// absolute.
    pub(crate) composer: Vec<PathBuf>,
    /// The values the code sets it to.
    pub(crate) set: Vec<WorldValues>,
}

/// The strings an expression can take in each world, `None` where they cannot
/// be computed: a value the code sets the include path to, or a directory it
/// moves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorldValues {
    pub(crate) by_world: [Option<Vec<String>>; 2],
}

impl IncludePath {
    /// The entries of every value of the path in `world`, as the directories
    /// they name. `None` when the path is open: its start is not declared, or
    /// a value set cannot be computed.
    fn entries(&self, world: World) -> Option<Vec<Target>> {
        // PHP does not take an empty path: it keeps its default.
        let declared = self
            .declared
            .as_deref()
            .filter(|declared| !declared.is_empty())?;
        let mut texts = vec![declared.to_string()];
        for set in &self.set {
            for text in set.by_world[world.index()].as_ref()? {
                texts.push(expand_get_include_path(text, declared, world)?);
            }
        }
        let mut entries: Vec<Target> = Vec::new();
        for text in &texts {
            let mut parts: Vec<&str> = text.split(world.path_separator()).collect();
            // PHP ends its walk of the path at its end: an empty entry there
            // names nothing, while one before another entry is the root of the
            // file system. A value that is empty has no entries.
            if parts.last() == Some(&"") {
                parts.pop();
            }
            for entry in parts {
                let target = entry_target(world, entry);
                if !entries.contains(&target) {
                    entries.push(target);
                }
            }
        }
        Some(entries)
    }
}

/// `text` with each `get_include_path()` in it replaced by `declared`, the
/// value it has before any is set. `None` when one is part of an entry and not
/// a whole entry, which no list of entries can hold.
fn expand_get_include_path(text: &str, declared: &str, world: World) -> Option<String> {
    let separator = world.path_separator();
    for (at, _) in text.match_indices(GET_INCLUDE_PATH) {
        let before = text[..at].chars().next_back();
        let after = text[at + GET_INCLUDE_PATH.len()..].chars().next();
        if before.is_some_and(|c| c != separator) || after.is_some_and(|c| c != separator) {
            return None;
        }
    }
    Some(text.replace(GET_INCLUDE_PATH, declared))
}

/// What the configuration and the manifests say of where includes are found.
/// The rest comes from the code (`IncludeEnv`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IncludeSettings {
    /// The deployment's `include_path`, in its syntax.
    pub include_path: Option<String>,
    /// The directories a run starts in, when the configuration declares them.
    pub working_dirs: Option<Vec<PathBuf>>,
    /// The directories that Composer's autoloader adds to the include path.
    pub composer: Vec<PathBuf>,
    /// The trees the project reads: an include that leaves them is not judged.
    pub trees: Vec<PathBuf>,
    /// The directories a deployment builds and this checkout lacks, such as a
    /// Composer project's `vendor` before `composer install`. A file below
    /// one is not judged either.
    pub unbuilt: Vec<PathBuf>,
}

/// What the code does to where includes are found.
#[derive(Debug, Default)]
pub(crate) struct LookupChanges {
    /// The values it sets the include path to.
    pub(crate) include_path: Vec<WorldValues>,
    /// The directories it moves the working directory to.
    pub(crate) chdir: Vec<WorldValues>,
}

/// Everything the include rule knows of the project and its deployment: the
/// settings and what the code adds to them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IncludeEnv {
    /// The project's root. Without one nothing is judged.
    pub(crate) root: Option<PathBuf>,
    pub(crate) trees: Vec<PathBuf>,
    pub(crate) unbuilt: Vec<PathBuf>,
    /// The directories a run can be in.
    pub(crate) working_dirs: Vec<PathBuf>,
    /// Whether a `chdir()` moves a run somewhere this analysis cannot name.
    pub(crate) working_dirs_open: bool,
    pub(crate) include_path: IncludePath,
}

impl IncludeEnv {
    /// The environment of a project whose configuration and manifests say
    /// `settings`, whose root is `root`, and whose code does `changes`.
    /// `script_dirs` is asked for the directories scripts run from when the
    /// configuration declares no working directories.
    pub(crate) fn build(
        settings: &IncludeSettings,
        root: Option<&Path>,
        script_dirs: impl FnOnce() -> Vec<PathBuf>,
        changes: LookupChanges,
    ) -> IncludeEnv {
        let mut trees = settings.trees.clone();
        trees.extend(root.map(Path::to_path_buf));
        let mut working_dirs = settings.working_dirs.clone().unwrap_or_else(script_dirs);
        let mut working_dirs_open = false;
        // A `chdir()` to a directory this machine has adds it to those a run
        // can be in. A relative one, or one that is not computed, moves a run
        // to a directory this analysis cannot name. So does one the machine's
        // own world cannot map; the other world's reading of a path computed
        // from the project is the same place as the machine's, so what it
        // cannot map names no other.
        for value in &changes.chdir {
            for world in World::ALL {
                let Some(texts) = &value.by_world[world.index()] else {
                    working_dirs_open = true;
                    continue;
                };
                for text in texts {
                    let dir = match classify(world, text) {
                        Target::Absolute(anchor, names) => {
                            native_absolute(world, World::machine(), root, &anchor, &names)
                        }
                        _ => None,
                    };
                    match dir {
                        Some(dir) if !working_dirs.contains(&dir) => working_dirs.push(dir),
                        Some(_) => {}
                        None => working_dirs_open |= world == World::machine(),
                    }
                }
            }
        }
        IncludeEnv {
            root: root.map(Path::to_path_buf),
            trees,
            unbuilt: settings.unbuilt.clone(),
            working_dirs,
            working_dirs_open,
            include_path: IncludePath {
                declared: settings.include_path.clone(),
                composer: settings.composer.clone(),
                set: changes.include_path,
            },
        }
    }

    /// The place an include of the file in `includer_dir` is looked up from.
    pub(crate) fn place<'a>(&'a self, includer_dir: Option<&'a Path>) -> Place<'a> {
        Place {
            machine: World::machine(),
            root: self.root.as_deref(),
            includer_dir,
            trees: &self.trees,
            unbuilt: &self.unbuilt,
            working_dirs: &self.working_dirs,
            working_dirs_open: self.working_dirs_open,
            include_path: &self.include_path,
        }
    }
}

/// What a run starts from, in the project.
pub(crate) struct Place<'a> {
    /// The world of the analysing machine.
    pub(crate) machine: World,
    /// The project's root, which gives the current drive on Windows.
    pub(crate) root: Option<&'a Path>,
    /// The including file's directory.
    pub(crate) includer_dir: Option<&'a Path>,
    /// The trees the project reads. A path outside them is not judged.
    pub(crate) trees: &'a [PathBuf],
    /// The directories of the trees that this checkout lacks. A path below
    /// one is not judged either.
    pub(crate) unbuilt: &'a [PathBuf],
    /// The directories a run can be in.
    pub(crate) working_dirs: &'a [PathBuf],
    /// Whether a run can also be in a directory this analysis cannot name,
    /// where any file relative to the working directory can be.
    pub(crate) working_dirs_open: bool,
    pub(crate) include_path: &'a IncludePath,
}

impl Place<'_> {
    /// Whether `path` lies, by its text, in a tree the project reads, and
    /// not in a directory it lacks.
    fn holds(&self, path: &Path) -> bool {
        let path = folded(path);
        let below = |dirs: &[PathBuf]| dirs.iter().any(|dir| path_bytes::starts_with(&path, dir));
        below(self.trees) && !below(self.unbuilt)
    }

    /// Whether `candidate` settles the include: it is a file in the project,
    /// or it is somewhere this analysis cannot look.
    fn settles(&self, candidate: Option<PathBuf>, is_file: &dyn Fn(&Path) -> bool) -> bool {
        match candidate {
            Some(path) if self.holds(&path) => is_file(&path),
            _ => true,
        }
    }

    /// The directories a run can start in, the including file's own first:
    /// it is where a file is most likely to be.
    fn working_dirs_nearest_first(&self) -> impl Iterator<Item = &PathBuf> {
        let includer = self.includer_dir;
        let is_includer = move |dir: &&PathBuf| Some(dir.as_path()) == includer;
        self.working_dirs.iter().filter(is_includer).chain(
            self.working_dirs
                .iter()
                .filter(move |dir| !is_includer(dir)),
        )
    }

    /// Whether `names` below a working directory settle the include.
    fn settles_in_working_dirs(&self, names: &Names, is_file: &dyn Fn(&Path) -> bool) -> bool {
        self.working_dirs_open
            || self
                .working_dirs_nearest_first()
                .any(|dir| self.settles(Some(join_names(dir, names)), is_file))
    }

    /// Whether `names` below the directory that `entry` of the include path
    /// names cannot be looked into without a file system: the entry is not a
    /// path this machine has, or the place is outside the project. A place
    /// that depends on the working directory is not known to be.
    fn entry_leaves(&self, world: World, entry: &Target, names: &Names) -> bool {
        match entry {
            Target::Elsewhere => true,
            Target::Absolute(anchor, entry_names) => {
                native_absolute(world, self.machine, self.root, anchor, entry_names)
                    .is_none_or(|dir| !self.holds(&join_names(&dir, names)))
            }
            Target::Relative(_) | Target::Plain(_) => false,
        }
    }

    /// Whether `names` below the directory that `entry` of the include path
    /// names settle the include.
    fn settles_in_entry(
        &self,
        world: World,
        entry: &Target,
        names: &Names,
        is_file: &dyn Fn(&Path) -> bool,
    ) -> bool {
        match entry {
            Target::Elsewhere => true,
            Target::Absolute(anchor, entry_names) => {
                let dir = native_absolute(world, self.machine, self.root, anchor, entry_names);
                self.settles(dir.map(|dir| join_names(&dir, names)), is_file)
            }
            // Relative to the working directory.
            Target::Relative(entry_names) | Target::Plain(entry_names) => {
                self.working_dirs_open
                    || self.working_dirs_nearest_first().any(|dir| {
                        let dir = join_names(dir, entry_names);
                        self.settles(Some(join_names(&dir, names)), is_file)
                    })
            }
        }
    }

    /// Whether `text`, an include read in `world`, is found, or cannot be
    /// judged. `is_file` says whether a path in the project is a file.
    ///
    /// A candidate that leaves the project settles the include whatever the
    /// others are, so the candidates that need no file system are asked
    /// first, and the rest in the order a file is most likely found.
    pub(crate) fn is_found(
        &self,
        world: World,
        text: &str,
        is_file: &dyn Fn(&Path) -> bool,
    ) -> bool {
        match classify(world, text) {
            Target::Elsewhere => true,
            Target::Absolute(anchor, names) => {
                let path = native_absolute(world, self.machine, self.root, &anchor, &names);
                self.settles(path, is_file)
            }
            Target::Relative(names) => self.settles_in_working_dirs(&names, is_file),
            Target::Plain(names) => {
                let Some(entries) = self.include_path.entries(world) else {
                    return true;
                };
                if entries
                    .iter()
                    .any(|entry| self.entry_leaves(world, entry, &names))
                {
                    return true;
                }
                self.include_path
                    .composer
                    .iter()
                    .any(|dir| self.settles(Some(join_names(dir, &names)), is_file))
                    || self.settles(
                        self.includer_dir.map(|dir| join_names(dir, &names)),
                        is_file,
                    )
                    || self.settles_in_working_dirs(&names, is_file)
                    || entries
                        .iter()
                        .any(|entry| self.settles_in_entry(world, entry, &names, is_file))
            }
        }
    }
}

#[cfg(test)]
mod tests;
