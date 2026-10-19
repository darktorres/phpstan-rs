//! Reads a project's real, installed Composer autoloader
//! (`vendor/composer/autoload_static.php`, or the older separate
//! `autoload_classmap.php`/`autoload_psr4.php`/`autoload_namespaces.php`/
//! `autoload_files.php`) to discover `vendor/` files and directories worth
//! loading for type-resolution context - without reporting diagnostics on
//! them. This mirrors real PHPStan's `paths:` vs. Composer-autoloader
//! split: load vendor code for context, but only report on the project's
//! own files.
//!
//! Deliberately reads the generated autoloader files Composer's own
//! `ClassLoader` reads at runtime, rather than re-implementing PSR-4
//! resolution against `composer.json` - those files are just plain PHP
//! returning array literals (or, for `autoload_static.php`, a class with
//! static array properties), and this project already has a full PHP
//! lexer/parser sitting right there to read them with.
//!
//! Every discovered path is required to resolve under the project's own
//! `vendor/` directory. PSR-4/classmap/files entries are not vendor-only -
//! real `composer.json` setups commonly autoload first-party code the same
//! way (e.g. Laravel's `'App\\' => [$baseDir . '/app']`) - so without this
//! filter, a project's own source could get silently folded into "loaded
//! for context, never reported" and its bugs would stop being reported
//! with no warning.
//!
//! What a generated file lists depends on its bytes alone, so it is kept by
//! them (`GeneratedCache`), and a file read before is not parsed again. The
//! paths it lists are resolved on every run, as resolving reads the file
//! system.

use hashbrown::{HashTable, hash_table};
use rustc_hash::FxHashMap;
#[cfg(test)]
use rustc_hash::FxHashSet;
use std::cell::OnceCell;
use std::ffi::OsStr;
use std::fs::FileType;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use syntax::ast::{BinOp, ClassDecl, Expr, ExprKind, Literal, Stmt, StmtKind};

use crate::canonical;
use crate::dir_listings::{DirListings, Listing};
use crate::hashed_path::{HashedPath, PathHash, Resolved, hashed_ancestors};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct VendorContext {
    /// Exact files to load (classmap/files entries), except the PHP files that
    /// the walk of a directory of `dirs` lists.
    pub files: Vec<VendorPath>,
    /// Directories to recursively load (PSR-4 base dirs).
    pub dirs: Vec<VendorPath>,
}

impl VendorContext {
    /// Adds what `other` lists, after what this lists.
    pub fn extend_with(&mut self, other: &VendorContext) {
        self.files.extend(other.files.iter().cloned());
        self.dirs.extend(other.dirs.iter().cloned());
    }
}

/// A path the vendor context lists.
#[derive(Debug, Clone, PartialEq)]
pub enum VendorPath {
    /// A resolved path of the kind its entry names, by its directory's
    /// listing: a directory for a directory to load, a regular file for a
    /// file. The walk takes it as it is.
    Resolved(Resolved),
    /// A path the walk resolves: one kept as written, or one resolved through
    /// a link, whose own kind the listing does not give.
    Unresolved(PathBuf),
}

impl VendorPath {
    pub fn as_path(&self) -> &Path {
        match self {
            VendorPath::Resolved(path) => path.as_path(),
            VendorPath::Unresolved(path) => path,
        }
    }
}

/// How a generated autoload file is read: which of its arrays list paths,
/// and whether those are files or directories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Reading {
    /// `autoload_static.php`: a class whose static `classMap` and `files`
    /// list files, whose `prefixDirsPsr4` lists directories and whose
    /// `prefixesPsr0` lists them by first letter.
    StaticClass,
    /// `autoload_classmap.php`: an array of class names to files.
    Classmap,
    /// `autoload_files.php`: an array of files.
    Files,
    /// `autoload_psr4.php`: an array of namespace prefixes to arrays of
    /// directories.
    Psr4,
    /// `autoload_namespaces.php`: an array of PSR-0 prefixes to arrays of
    /// directories.
    Psr0,
}

/// Keeps what reading a generated autoload file gives, by the file's bytes
/// and its reading, which are all it depends on.
pub trait GeneratedCache: Send + Sync {
    /// The entries kept for a file of `bytes` read as `reading` says.
    fn load(&self, reading: Reading, bytes: &[u8]) -> Option<GeneratedEntries>;

    /// Keeps `entries`, which a file of `bytes` read as `reading` says gave.
    fn store(&self, reading: Reading, bytes: &[u8], entries: &GeneratedEntries);
}

/// Discovers the vendor context of the Composer project whose installed vendor
/// directory is `vendor_dir`, or an empty `VendorContext` if that directory
/// has no `composer/` (Composer has not installed there). The caller names the
/// directory (the project's `vendor_location`), so the project's own
/// `vendor-dir` setting is what is read. The directories it reads are read
/// through `listings`, which the walk of the same load shares, and what the
/// generated files list is kept in `cache`, when there is one.
pub fn discover_vendor_context(
    vendor_dir: &Path,
    listings: &DirListings,
    cache: Option<&dyn GeneratedCache>,
) -> VendorContext {
    discover_vendor_context_with(
        vendor_dir,
        listings,
        cache,
        &OutsideInstalls::filled(Vec::new()),
    )
}

/// `discover_vendor_context`, which also takes the entries that a directory
/// of `outside` holds: the install directories of the packages Composer
/// installed outside the vendor directory, which are context as the vendor
/// directory's own packages are.
pub fn discover_vendor_context_with(
    vendor_dir: &Path,
    listings: &DirListings,
    cache: Option<&dyn GeneratedCache>,
    outside: &OutsideInstalls,
) -> VendorContext {
    let composer_dir = vendor_dir.join("composer");
    if !composer_dir.is_dir() {
        return VendorContext::default();
    }
    let Ok(vendor_dir) = canonical::canonicalize(&composer_dir.join("..")) else {
        return VendorContext::default();
    };
    let Some(base_dir) = vendor_dir.parent().map(Path::to_path_buf) else {
        return VendorContext::default();
    };

    // Every generated file below is machine-written, stable, and small
    // (a handful of files at most) - lex/parse panics are still possible
    // (a future Composer/PHP syntax this parser doesn't handle yet), so
    // the batch is wrapped in one silenced-panic-hook window, matching
    // `main.rs::run_check`'s reasoning for doing the same over a much
    // larger batch: avoid both a crash and unwind-noise on stderr for
    // what should be a quiet "skip this source of context" degrade.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let entries = discover_inner(
        &composer_dir,
        &vendor_dir,
        &base_dir,
        listings,
        cache,
        outside,
    );
    std::panic::set_hook(default_hook);
    entries.into_context(listings)
}

/// The install directories of the packages Composer installed outside the
/// vendor directory, which vendor discovery takes as context. A load sets
/// them once its manifest is read, and discovery, which runs beside the load,
/// waits for them where a path first lies outside the vendor directory.
#[derive(Debug)]
pub struct OutsideInstalls {
    dirs: Mutex<Option<Vec<PathBuf>>>,
    ready: Condvar,
}

impl OutsideInstalls {
    /// The directories, already known.
    pub fn filled(dirs: Vec<PathBuf>) -> OutsideInstalls {
        OutsideInstalls {
            dirs: Mutex::new(Some(dirs)),
            ready: Condvar::new(),
        }
    }

    /// Directories not yet known: `OutsideSetter::new` gives them.
    pub fn pending() -> Arc<OutsideInstalls> {
        Arc::new(OutsideInstalls {
            dirs: Mutex::new(None),
            ready: Condvar::new(),
        })
    }

    /// Keeps `dirs` if none were kept yet, and wakes the waiters.
    fn fill(&self, dirs: Vec<PathBuf>) {
        let mut slot = self.dirs.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(dirs);
            self.ready.notify_all();
        }
    }

    /// The directories, once they are known.
    fn wait(&self) -> Vec<PathBuf> {
        let mut slot = self.dirs.lock().unwrap_or_else(PoisonError::into_inner);
        while slot.is_none() {
            slot = self
                .ready
                .wait(slot)
                .unwrap_or_else(PoisonError::into_inner);
        }
        slot.clone().unwrap_or_default()
    }
}

/// Gives a pending `OutsideInstalls` its directories. Dropped without giving
/// them (a load that ended early), it gives none, so discovery never waits
/// for a value that is not coming.
pub struct OutsideSetter(Arc<OutsideInstalls>);

impl OutsideSetter {
    pub fn new(outside: &Arc<OutsideInstalls>) -> OutsideSetter {
        OutsideSetter(Arc::clone(outside))
    }

    pub fn set(self, dirs: Vec<PathBuf>) {
        self.0.fill(dirs);
    }
}

impl Drop for OutsideSetter {
    fn drop(&mut self) {
        self.0.fill(Vec::new());
    }
}

/// The older separate generated files, each with its reading, in the order
/// their entries are taken.
const SEPARATE_FILES: [(&str, Reading); 4] = [
    ("autoload_classmap.php", Reading::Classmap),
    ("autoload_files.php", Reading::Files),
    ("autoload_psr4.php", Reading::Psr4),
    ("autoload_namespaces.php", Reading::Psr0),
];

fn discover_inner(
    composer_dir: &Path,
    vendor_dir: &Path,
    base_dir: &Path,
    listings: &DirListings,
    cache: Option<&dyn GeneratedCache>,
    outside: &OutsideInstalls,
) -> Entries {
    let static_path = composer_dir.join("autoload_static.php");
    let generated = match static_path
        .is_file()
        .then(|| read_generated(&static_path, Reading::StaticClass, cache))
        .flatten()
    {
        Some(entries) => vec![entries],
        None => SEPARATE_FILES
            .into_iter()
            .filter_map(|(name, reading)| read_generated(&composer_dir.join(name), reading, cache))
            .collect(),
    };
    let mut resolver = Resolver::new(vendor_dir, base_dir, composer_dir, listings, outside);
    let mut entries = Entries::default();
    for generated in &generated {
        resolver.collect(&generated.files, &mut entries.files);
        resolver.collect(&generated.dirs, &mut entries.dirs);
    }
    // Composer's own bootstrap loader class - unlike every other file
    // this function discovers, it's never listed in any of the generated
    // autoload maps at all (it's what *runs* them: `vendor/autoload.php`
    // requires it directly, before the classmap/PSR-4 mechanisms it
    // reads even exist). Present unconditionally whenever
    // `vendor/composer/` exists at all, so always worth loading for
    // context - real code occasionally constructs it directly for its
    // own plugin/dynamic-autoloading needs.
    entries.class_loader = resolver
        .locate(&vendor_dir.join("composer").join("ClassLoader.php"))
        .filter(|found| match found.kind {
            Some(kind) => kind.is_file(),
            None => found.path.as_path().is_file(),
        });
    entries
}

/// What the generated file at `path` lists, read as `reading` says: from
/// `cache` when it keeps the entries of a file of these bytes, and otherwise
/// from the file's tree, then kept there. `None` when the file can't be
/// read, or is not of the shape `reading` expects.
///
/// Same pattern as `main.rs::check_chunk`: PHP source isn't guaranteed
/// UTF-8, so a lossy read rather than a strict one; a malformed/future-
/// syntax generated file must degrade to "no context from this file",
/// never crash `check-project` - `parse_program`'s own `Result` already
/// gives that for free, no `catch_unwind` needed.
fn read_generated(
    path: &Path,
    reading: Reading,
    cache: Option<&dyn GeneratedCache>,
) -> Option<GeneratedEntries> {
    let bytes = std::fs::read(path).ok()?;
    if let Some(entries) = cache.and_then(|cache| cache.load(reading, &bytes)) {
        return Some(entries);
    }
    let stmts = syntax::parse(&bytes).ok()?.stmts;
    let entries = GeneratedEntries::read(&stmts, reading)?;
    if let Some(cache) = cache {
        cache.store(reading, &bytes, &entries);
    }
    Some(entries)
}

fn find_class_decl(stmts: &[Stmt]) -> Option<&ClassDecl> {
    stmts.iter().find_map(|s| match &s.kind {
        StmtKind::ClassDecl(c) => Some(&**c),
        _ => None,
    })
}

/// The array a generated file returns.
fn top_level_array(stmts: &[Stmt]) -> Option<&Expr> {
    stmts.iter().find_map(|s| match &s.kind {
        StmtKind::Return(Some(e)) if matches!(e.kind, ExprKind::ArrayLiteral(_)) => Some(e),
        _ => None,
    })
}

/// What a generated autoload file lists, as its expressions spell the paths,
/// before the directories they name are put in: what reading the file
/// gives, which depends on its bytes alone.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GeneratedEntries {
    /// The files to load: classmap and files entries.
    files: Spelled,
    /// The directories to load: PSR-4 and PSR-0 entries.
    dirs: Spelled,
    /// The classmap's entries, each class name with the file it maps to.
    classes: Keyed,
    /// The PSR-4 entries, each namespace prefix with its directories.
    psr4: Keyed,
    /// The PSR-0 entries, each class name prefix with its directories.
    psr0: Keyed,
}

impl GeneratedEntries {
    /// What `stmts`, a generated file's, list when read as `reading` says;
    /// `None` when they are not of the shape it expects.
    fn read(stmts: &[Stmt], reading: Reading) -> Option<GeneratedEntries> {
        let mut entries = GeneratedEntries::default();
        match reading {
            Reading::StaticClass => {
                let class = find_class_decl(stmts)?;
                for prop in &class.def.properties {
                    if !prop.is_static {
                        continue;
                    }
                    let Some(default) = &prop.default else {
                        continue;
                    };
                    match prop.name.as_ref() {
                        "classMap" => {
                            entries.files.push_items(default);
                            entries.classes.push(default, false);
                        }
                        "files" => entries.files.push_items(default),
                        "prefixDirsPsr4" => {
                            entries.dirs.push_nested_items(default);
                            entries.psr4.push(default, true);
                        }
                        "prefixesPsr0" => {
                            let ExprKind::ArrayLiteral(letters) = &default.kind else {
                                continue;
                            };
                            for letter in letters {
                                entries.psr0.push(&letter.value, true);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Reading::Classmap => {
                let array = top_level_array(stmts)?;
                entries.files.push_items(array);
                entries.classes.push(array, false);
            }
            Reading::Files => entries.files.push_items(top_level_array(stmts)?),
            Reading::Psr4 => {
                let array = top_level_array(stmts)?;
                entries.dirs.push_nested_items(array);
                entries.psr4.push(array, true);
            }
            Reading::Psr0 => {
                let array = top_level_array(stmts)?;
                entries.dirs.push_nested_items(array);
                entries.psr0.push(array, true);
            }
        }
        Some(entries)
    }

    /// Each class of the classmap with the file it maps to, spelled from the
    /// given directories.
    pub fn classes(
        &self,
        generated_dir: &Path,
        vendor_dir: &Path,
        base_dir: &Path,
    ) -> Vec<(String, Vec<PathBuf>)> {
        self.classes.paths(generated_dir, vendor_dir, base_dir)
    }

    /// Each PSR-4 namespace prefix with its directories, spelled from the
    /// given directories.
    pub fn psr4(
        &self,
        generated_dir: &Path,
        vendor_dir: &Path,
        base_dir: &Path,
    ) -> Vec<(String, Vec<PathBuf>)> {
        self.psr4.paths(generated_dir, vendor_dir, base_dir)
    }

    /// Each PSR-0 class name prefix with its directories, spelled from the
    /// given directories.
    pub fn psr0(
        &self,
        generated_dir: &Path,
        vendor_dir: &Path,
        base_dir: &Path,
    ) -> Vec<(String, Vec<PathBuf>)> {
        self.psr0.paths(generated_dir, vendor_dir, base_dir)
    }
}

/// Paths under the keys a generated file gives them: a class name, or a
/// namespace prefix. A key is kept whatever its paths spell to, since the
/// key alone says what the file maps.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Keyed(Vec<(String, Spelled)>);

impl Keyed {
    /// Appends each entry of `array` that has a string key: the path its
    /// value spells, or with `dirs`, the paths of the array its value is.
    fn push(&mut self, array: &Expr, dirs: bool) {
        let ExprKind::ArrayLiteral(items) = &array.kind else {
            return;
        };
        for item in items {
            let Some(key) = item.key.as_ref().and_then(key_of) else {
                continue;
            };
            let mut paths = Spelled::default();
            if dirs {
                paths.push_items(&item.value);
            } else {
                paths.push(&item.value);
            }
            self.0.push((key, paths));
        }
    }

    /// Each key with the paths it names, spelled from the directories given.
    fn paths(
        &self,
        generated_dir: &Path,
        vendor_dir: &Path,
        base_dir: &Path,
    ) -> Vec<(String, Vec<PathBuf>)> {
        self.0
            .iter()
            .map(|(key, spelled)| {
                let mut paths = Vec::new();
                spelled.each(generated_dir, vendor_dir, base_dir, |raw| {
                    paths.extend(path_of(raw).map(Path::to_path_buf));
                });
                (key.clone(), paths)
            })
            .collect()
    }
}

/// The text of a key a generated file gives as a string; `None` for any
/// other key, such as the index an array of a list gives.
fn key_of(expr: &Expr) -> Option<String> {
    match &expr.peeled().kind {
        ExprKind::Literal(Literal::String(text)) => {
            Some(String::from_utf8_lossy(text.as_bytes()).into_owned())
        }
        _ => None,
    }
}

/// Paths spelled in pieces, one path after another, each ended by
/// `Piece::End`, and the text of their literals, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Spelled {
    text: Bytes,
    pieces: Vec<Piece>,
}

/// A piece of a spelled path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Piece {
    /// The next this many bytes of the text.
    Text(u32),
    /// `__DIR__`: the directory containing the generated file itself, i.e.
    /// `vendor/composer/` - `autoload_static.php` concatenates `/..` or
    /// `/../..` onto this to reach `vendor/` or the project root.
    GeneratedDir,
    /// `$vendorDir`, which only the separate, non-static files use.
    VendorDir,
    /// `$baseDir`, which only the separate, non-static files use.
    BaseDir,
    /// The end of a path.
    End,
}

impl Spelled {
    /// Appends the path of each item of `array`.
    fn push_items(&mut self, array: &Expr) {
        let ExprKind::ArrayLiteral(items) = &array.kind else {
            return;
        };
        for item in items {
            self.push(&item.value);
        }
    }

    /// Appends the paths of each array that is an item of `array`.
    fn push_nested_items(&mut self, array: &Expr) {
        let ExprKind::ArrayLiteral(items) = &array.kind else {
            return;
        };
        for item in items {
            self.push_items(&item.value);
        }
    }

    /// Appends the path `expr` spells, or nothing when it is not a path a
    /// generated file writes.
    fn push(&mut self, expr: &Expr) {
        let (text, pieces) = (self.text.0.len(), self.pieces.len());
        if self.append(expr).is_some() {
            self.pieces.push(Piece::End);
        } else {
            self.text.0.truncate(text);
            self.pieces.truncate(pieces);
        }
    }

    /// Appends the pieces `expr` spells. Required because real generated code
    /// chains multiple concatenations (`__DIR__ . '/..' . '/pkg/path'`), not
    /// a flat two-operand shape. `None` when `expr` is not such a path; the
    /// pieces appended before that are then not used.
    fn append(&mut self, expr: &Expr) -> Option<()> {
        syntax::stack::guard(|| {
            let expr = expr.peeled();
            match &expr.kind {
                ExprKind::Literal(Literal::String(s)) => self.append_text(s.as_bytes())?,
                ExprKind::Name(n) if n == "__DIR__" => self.pieces.push(Piece::GeneratedDir),
                ExprKind::Variable(n) if **n == *"vendorDir" => self.pieces.push(Piece::VendorDir),
                ExprKind::Variable(n) if **n == *"baseDir" => self.pieces.push(Piece::BaseDir),
                ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
                    self.append(lhs)?;
                    self.append(rhs)?;
                }
                _ => return None,
            }
            Some(())
        })
    }

    /// Appends `bytes` to the text, in the piece of text right before them
    /// when there is one.
    fn append_text(&mut self, bytes: &[u8]) -> Option<()> {
        let len = u32::try_from(bytes.len()).ok()?;
        match self.pieces.last_mut() {
            Some(Piece::Text(before)) => *before = before.checked_add(len)?,
            _ => self.pieces.push(Piece::Text(len)),
        }
        self.text.0.extend_from_slice(bytes);
        Some(())
    }

    /// Each path, its pieces put together, with `generated_dir`, `vendor_dir`
    /// and `base_dir` for the directories they name.
    fn each(
        &self,
        generated_dir: &Path,
        vendor_dir: &Path,
        base_dir: &Path,
        mut each: impl FnMut(&[u8]),
    ) {
        let mut path = Vec::new();
        let mut text = 0;
        for piece in &self.pieces {
            let dir = match *piece {
                Piece::Text(len) => {
                    let end = text + len as usize;
                    path.extend_from_slice(&self.text.0[text..end]);
                    text = end;
                    continue;
                }
                Piece::GeneratedDir => generated_dir,
                Piece::VendorDir => vendor_dir,
                Piece::BaseDir => base_dir,
                Piece::End => {
                    each(&path);
                    path.clear();
                    continue;
                }
            };
            path.extend_from_slice(dir.as_os_str().as_encoded_bytes());
        }
    }
}

/// Bytes serialized as one byte string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Bytes(Vec<u8>);

impl serde::Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Bytes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = Bytes;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a byte string")
            }

            fn visit_bytes<E: serde::de::Error>(self, bytes: &[u8]) -> Result<Bytes, E> {
                Ok(Bytes(bytes.to_vec()))
            }

            fn visit_byte_buf<E: serde::de::Error>(self, bytes: Vec<u8>) -> Result<Bytes, E> {
                Ok(Bytes(bytes))
            }
        }
        deserializer.deserialize_byte_buf(Visitor)
    }
}

/// What the generated files list, resolved: the files and the directories,
/// and Composer's own loader class.
#[derive(Default)]
struct Entries {
    files: Vec<Located>,
    dirs: Vec<Located>,
    class_loader: Option<Located>,
}

impl Entries {
    /// The context the entries give: each list without its repeats, and the
    /// files without those that the walk of a directory lists, then the
    /// loader class. The lists keep their order: nothing here needs one, as
    /// the walk's output does not depend on the order of its roots.
    fn into_context(mut self, table: &DirListings) -> VendorContext {
        keep_first(&mut self.dirs);
        drop_files_under_dirs(&mut self.files, &self.dirs, table);
        keep_first(&mut self.files);
        let mut files: Vec<VendorPath> = self
            .files
            .into_iter()
            .map(|file| file.into_vendor_path(FileType::is_file))
            .collect();
        files.extend(
            self.class_loader
                .map(|file| file.into_vendor_path(FileType::is_file)),
        );
        VendorContext {
            files,
            dirs: self
                .dirs
                .into_iter()
                .map(|dir| dir.into_vendor_path(FileType::is_dir))
                .collect(),
        }
    }
}

/// A path the resolver found, with its kind when its directory's listing
/// gives the entry as no link, which makes the path resolved.
struct Located {
    path: HashedPath,
    kind: Option<FileType>,
}

impl Located {
    /// The path as the walk takes it: as it is when it is resolved and of the
    /// kind that `is_kind` asks for, and resolved by the walk otherwise.
    fn into_vendor_path(self, is_kind: fn(&FileType) -> bool) -> VendorPath {
        match self.kind {
            Some(kind) if is_kind(&kind) => VendorPath::Resolved(Resolved::known(self.path)),
            _ => VendorPath::Unresolved(self.path.into_path_buf()),
        }
    }
}

/// A directory the resolver found: its canonical form, its listing, and where
/// it lies against the vendor directory.
struct Listed {
    canonical: HashedPath,
    listing: Arc<Listing>,
    vendor: Place,
}

/// A directory as the generated files write it, with `.` and `..` folded
/// away, and where that lies against the vendor directory, resolved and as
/// written.
struct AsWritten {
    dir: HashedPath,
    vendor: Place,
    vendor_as_written: Place,
}

/// Where a directory lies against a base directory, which tells which of its
/// entries lie in the base: an entry is the directory joined with one name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// In the base, or the base itself: every entry lies in it.
    In,
    /// The base's parent: only the entry that is the base lies in it.
    Above,
    /// Elsewhere: no entry lies in it.
    Out,
}

impl Place {
    pub(crate) fn of(dir: &Path, base: &Path) -> Place {
        if path_bytes::starts_with(dir, base) {
            Place::In
        } else if path_bytes::parent(base) == Some(dir) {
            Place::Above
        } else {
            Place::Out
        }
    }

    /// Whether the entry `name` of a directory placed so lies in `base`.
    pub(crate) fn holds(self, name: &OsStr, base: &Path) -> bool {
        match self {
            Place::In => true,
            Place::Above => path_bytes::file_name(base) == Some(name),
            Place::Out => false,
        }
    }
}

/// Resolves a generated autoloader's path expressions against its vendor,
/// base and generated directories.
struct Resolver<'p> {
    vendor_dir: &'p Path,
    base_dir: &'p Path,
    generated_dir: &'p Path,
    /// `vendor_dir` as the generated files write it: the generated
    /// directory's parent, with `.` and `..` folded away.
    vendor_as_written: PathBuf,
    /// The directory listings of the load, by canonical path.
    table: &'p DirListings,
    // Each directory's listing, made once: autoload entries share a few
    // thousand directories among tens of thousands of files, and a listing
    // tells whether an entry exists and whether it is a link, which a
    // `stat` of each file would. Keyed by the directory's text as written,
    // which hashes as bytes - a `Path` hashes component by component. None
    // for a directory that has no canonical form or cannot be listed.
    listings: FxHashMap<Vec<u8>, Option<Listed>>,
    /// The directories whose entries were taken as written, by their text as
    /// written.
    written: FxHashMap<Vec<u8>, AsWritten>,
    /// The install directories of the packages Composer installed outside the
    /// vendor directory, which the load sets (`OutsideInstalls`).
    outside: &'p OutsideInstalls,
    /// `outside`, taken once, when a path first asks for it.
    installs: OnceCell<Vec<PathBuf>>,
}

impl<'p> Resolver<'p> {
    fn new(
        vendor_dir: &'p Path,
        base_dir: &'p Path,
        generated_dir: &'p Path,
        table: &'p DirListings,
        outside: &'p OutsideInstalls,
    ) -> Self {
        Resolver {
            vendor_dir,
            base_dir,
            generated_dir,
            vendor_as_written: lexically_normalized(&generated_dir.join("..")),
            table,
            listings: FxHashMap::default(),
            written: FxHashMap::default(),
            outside,
            installs: OnceCell::new(),
        }
    }

    /// Each of `paths` that `locate` finds, or that lies in a package Composer
    /// installed outside the vendor directory, onto `out`.
    fn collect(&mut self, paths: &Spelled, out: &mut Vec<Located>) {
        let (generated_dir, vendor_dir, base_dir) =
            (self.generated_dir, self.vendor_dir, self.base_dir);
        paths.each(generated_dir, vendor_dir, base_dir, |raw| {
            if let Some(found) = path_of(raw)
                .and_then(|path| self.locate(path).or_else(|| self.installed_elsewhere(path)))
            {
                out.push(found);
            }
        });
    }

    /// `raw` when it lies in an install directory of a package Composer
    /// installed outside the vendor directory, as the walk takes it: as
    /// written when that places it there, else as the system resolves it.
    /// The directories are taken from the load on first use, and the system
    /// is asked only when the written path lies elsewhere.
    fn installed_elsewhere(&mut self, raw: &Path) -> Option<Located> {
        let outside = self.outside;
        let dirs = self.installs.get_or_init(|| outside.wait());
        if dirs.is_empty() {
            return None;
        }
        let lexical = lexically_normalized(raw);
        if dirs
            .iter()
            .any(|dir| path_bytes::starts_with(&lexical, dir))
        {
            return Some(Located {
                path: HashedPath::new(lexical),
                kind: None,
            });
        }
        let resolved = canonical::canonicalize(raw).ok()?;
        dirs.iter()
            .any(|dir| path_bytes::starts_with(&resolved, dir))
            .then(|| Located {
                path: HashedPath::new(resolved),
                kind: None,
            })
    }

    /// Resolves one path a generated file lists to an absolute path under
    /// `vendor_dir`, or `None` if it doesn't exist, or (the common,
    /// load-bearing case) resolves outside `vendor_dir` entirely - a
    /// `$baseDir`/`__DIR__`-with-extra-`/..`-rooted, first-party location,
    /// which must never be silently treated as vendor context. A path that
    /// resolves outside it but is written inside it is kept as written: a
    /// path-repository package is a `vendor/<name>` symlink to a directory
    /// outside `vendor/`, and still belongs to vendor context. A name its
    /// directory's listing does not hold is resolved by the system, which
    /// finds the file under another spelling where the directory folds case.
    fn locate(&mut self, raw: &Path) -> Option<Located> {
        let unknown = |path| Located { path, kind: None };
        let Some((dir, name)) = path_bytes::split(raw) else {
            return self.path_alone(raw).map(unknown);
        };
        if dir.as_os_str().is_empty() {
            return self.path_alone(raw).map(unknown);
        }
        let vendor_dir = self.vendor_dir;
        let Some(listed) = self.listed(dir) else {
            // A directory that can't be listed (no permission to read it
            // but to pass through it) is still resolved as the system does.
            return self.path_alone(raw).map(unknown);
        };
        let Some(entry) = listed.listing.entry(name) else {
            return self.path_alone(raw).map(unknown);
        };
        match entry.kind {
            // An entry that is not a link lies where its directory's
            // canonical form does.
            Some(kind) if !kind.is_symlink() => {
                if listed.vendor.holds(name, vendor_dir) {
                    return Some(Located {
                        path: listed.canonical.child(name),
                        kind: Some(kind),
                    });
                }
            }
            // A link may lead anywhere, and an entry of no known type may be
            // one, so the system resolves it and it is placed itself.
            _ => {
                let resolved = canonical::canonicalize(raw).ok()?;
                if path_bytes::starts_with(&resolved, vendor_dir) {
                    return Some(unknown(HashedPath::new(resolved)));
                }
            }
        }
        self.written_entry(dir, name).map(unknown)
    }

    /// `raw`, which has no directory to look it up in or one that cannot be
    /// listed, resolved by the system; or, when that lies outside the vendor
    /// directory, as written.
    fn path_alone(&self, raw: &Path) -> Option<HashedPath> {
        let resolved = canonical::canonicalize(raw).ok()?;
        if path_bytes::starts_with(&resolved, self.vendor_dir) {
            return Some(HashedPath::new(resolved));
        }
        let lexical = lexically_normalized(raw);
        (lexical.starts_with(self.vendor_dir) || lexical.starts_with(&self.vendor_as_written))
            .then(|| HashedPath::new(lexical))
    }

    /// The entry `name` of `dir` as written, with `.` and `..` folded away,
    /// when that lies in the vendor directory, resolved or as written. Where
    /// `dir` lies is found once per directory.
    fn written_entry(&mut self, dir: &Path, name: &OsStr) -> Option<HashedPath> {
        let key = dir.as_os_str().as_encoded_bytes();
        if !self.written.contains_key(key) {
            let lexical = lexically_normalized(dir);
            let written = AsWritten {
                vendor: Place::of(&lexical, self.vendor_dir),
                vendor_as_written: Place::of(&lexical, &self.vendor_as_written),
                dir: HashedPath::new(lexical),
            };
            self.written.insert(key.to_vec(), written);
        }
        let written = &self.written[key];
        (written.vendor.holds(name, self.vendor_dir)
            || written
                .vendor_as_written
                .holds(name, &self.vendor_as_written))
        .then(|| written.dir.child(name))
    }

    /// The listing of `dir`, found once. A directory is listed under the
    /// text it is written as, which is found by its bytes: one spelled
    /// twice (`a//b`, `a/b`) is listed under either, with a canonical form
    /// that is the same.
    fn listed(&mut self, dir: &Path) -> Option<&Listed> {
        let key = dir.as_os_str().as_encoded_bytes();
        if !self.listings.contains_key(key) {
            let listed = self.list(dir);
            self.listings.insert(key.to_vec(), listed);
        }
        self.listings.get(key)?.as_ref()
    }

    /// The table's listing of `dir`, under its canonical form. A listing
    /// with an entry that could not be read is no listing: the entries it
    /// lacks are not known.
    fn list(&mut self, dir: &Path) -> Option<Listed> {
        let canonical = self.canonical_dir(dir)?;
        let listing = self.table.listing(&canonical)?;
        listing.complete.then(|| Listed {
            vendor: Place::of(canonical.as_path(), self.vendor_dir),
            canonical,
            listing,
        })
    }

    /// The canonical form of `dir`: its parent's, joined with its name,
    /// unless the parent lists it as a link, which is the one case that
    /// needs the system to follow it. The vendor directory and its parent,
    /// spelled as discovery spells them, are canonical already, so the
    /// directories above them are not read.
    fn canonical_dir(&mut self, dir: &Path) -> Option<HashedPath> {
        for known in [self.vendor_dir, self.base_dir] {
            if dir.as_os_str() == known.as_os_str() {
                return Some(HashedPath::new(known.to_path_buf()));
            }
        }
        // The root, or a path that ends in `..`, which names the parent of
        // what precedes it: the system resolves it.
        let Some((parent, name)) = path_bytes::split(dir) else {
            return canonicalized(dir);
        };
        if parent.as_os_str().is_empty() {
            return canonicalized(dir);
        }
        let Some(parent_listed) = self.listed(parent) else {
            return canonicalized(dir);
        };
        entry_canonical(parent_listed, name, dir)
    }
}

/// `path` resolved by the system.
fn canonicalized(path: &Path) -> Option<HashedPath> {
    canonical::canonicalize(path).ok().map(HashedPath::new)
}

/// The canonical form of the entry `name` of a listed directory, or `None`
/// when the listing does not have it. A link, or an entry whose type the
/// system does not give, is resolved by the system at `path`; any other is
/// its directory's canonical form joined with its name.
fn entry_canonical(listed: &Listed, name: &OsStr, path: &Path) -> Option<HashedPath> {
    let entry = listed.listing.entry(name)?;
    match entry.kind {
        Some(kind) if !kind.is_symlink() => Some(listed.canonical.child(name)),
        _ => canonicalized(path),
    }
}

/// The path `bytes` spell. A Unix name is its bytes.
#[cfg(unix)]
fn path_of(bytes: &[u8]) -> Option<&Path> {
    use std::os::unix::ffi::OsStrExt;
    Some(Path::new(std::ffi::OsStr::from_bytes(bytes)))
}

/// The path `bytes` spell.
#[cfg(not(unix))]
fn path_of(bytes: &[u8]) -> Option<&Path> {
    path_of_text(bytes)
}

/// The path `bytes` spell where a path is text: bytes that are not UTF-8 name
/// no file, since PHP opens a path there as UTF-8 and refuses these.
#[cfg(any(not(unix), test))]
pub(crate) fn path_of_text(bytes: &[u8]) -> Option<&Path> {
    std::str::from_utf8(bytes).ok().map(Path::new)
}

/// `path` with `.` and `..` components folded away, without resolving
/// symlinks.
fn lexically_normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Keeps the first of each path in `paths`, in their order.
fn keep_first(paths: &mut Vec<Located>) {
    let first: Vec<bool> = {
        let mut seen = HashTable::with_capacity(paths.len());
        paths
            .iter()
            .map(|Located { path, .. }| {
                match seen.entry(
                    path.hash(),
                    |seen: &&HashedPath| *seen == path,
                    |seen| seen.hash(),
                ) {
                    hash_table::Entry::Occupied(_) => false,
                    hash_table::Entry::Vacant(entry) => {
                        entry.insert(path);
                        true
                    }
                }
            })
            .collect()
    };
    let mut first = first.into_iter();
    paths.retain(|_| first.next() == Some(true));
}

/// Drops the PHP files that the walk of a directory of `dirs` is sure to
/// list: the walk lists them already, and listing them again adds nothing.
/// Other files stay, as an explicit file is listed whatever its extension.
fn drop_files_under_dirs(files: &mut Vec<Located>, dirs: &[Located], table: &DirListings) {
    let mut walks = Walks::of(dirs, table);
    #[cfg(test)]
    let (by_path, mut listable) = (
        dirs.iter()
            .map(|dir| dir.path.as_path())
            .collect::<FxHashSet<&Path>>(),
        FxHashMap::default(),
    );
    files.retain(|Located { path: file, .. }| {
        let listed = walks.list(file);
        #[cfg(test)]
        assert_eq!(
            listed,
            is_php(file.as_path())
                && walked_by_a_directory(file.as_path(), &by_path, &mut listable, table),
            "whether a walk lists {}",
            file.as_path().display()
        );
        !listed
    });
}

#[cfg(test)]
fn is_php(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "php")
}

/// What the walks of some directories are sure to list, read from the
/// listings that the walks read.
struct Walks<'d> {
    table: &'d DirListings,
    /// The walks' directories, by the hashes their paths carry, each with its
    /// listing once asked for: none when it is not a directory, is a link, or
    /// cannot be listed.
    roots: HashTable<(&'d HashedPath, OnceCell<Option<Arc<Listing>>>)>,
    /// The directories of the files asked about, each with its listing when
    /// a walk is sure to list what it holds.
    known: HashTable<(PathBuf, PathHash, Option<Arc<Listing>>)>,
}

impl<'d> Walks<'d> {
    fn of(dirs: &'d [Located], table: &'d DirListings) -> Self {
        let mut roots = HashTable::with_capacity(dirs.len());
        for Located { path: dir, .. } in dirs {
            roots.insert_unique(dir.hash(), (dir, OnceCell::new()), |(dir, _)| dir.hash());
        }
        Walks {
            table,
            roots,
            known: HashTable::new(),
        }
    }

    /// Whether a walk is sure to list `file`: a PHP file that its
    /// directory's listing gives as a regular file, in a directory that a
    /// walk is sure to list. A file that this does not hold for stays listed
    /// explicitly, so a file the walk could miss (a missing one, or one
    /// behind a directory that cannot be read) is still listed.
    fn list(&mut self, file: &HashedPath) -> bool {
        let Some((dir, hash, name)) = file.split() else {
            return false;
        };
        path_bytes::is_php_name(name)
            && self.listing(dir, hash).is_some_and(|listing| {
                listing
                    .entry(name)
                    .and_then(|entry| entry.kind)
                    .is_some_and(|kind| kind.is_file())
            })
    }

    /// The listing of `dir`, whose hash is `hash`, when a walk is sure to
    /// list what it holds. It is found once per directory.
    fn listing(&mut self, dir: &Path, hash: PathHash) -> Option<Arc<Listing>> {
        if let Some((.., known)) = self.known.find(hash, |(known, ..)| known == dir) {
            return known.clone();
        }
        let listing = self.find(dir);
        self.known.insert_unique(
            hash,
            (dir.to_path_buf(), hash, listing.clone()),
            |&(_, hash, _)| hash,
        );
        listing
    }

    /// The listing of `dir` when the walk of the nearest of the walks'
    /// directories at or above it is sure to list what `dir` holds: that
    /// directory is a directory and not a link, each directory below it down
    /// to `dir` is one by its parent's listing, and each can be listed.
    fn find(&self, dir: &Path) -> Option<Arc<Listing>> {
        // The names of the directories below that one, down to `dir`.
        let mut below = Vec::new();
        let mut ancestors = hashed_ancestors(dir);
        let (root, checked) = loop {
            let (ancestor, hash, name) = ancestors.next()?;
            if let Some(root) = self
                .roots
                .find(hash, |(root, _)| root.as_path() == ancestor)
            {
                break root;
            }
            below.push(name);
        };
        let mut listing = checked
            .get_or_init(|| {
                let is_dir = std::fs::symlink_metadata(root.as_path())
                    .is_ok_and(|metadata| metadata.is_dir());
                is_dir.then(|| self.table.listing(root)).flatten()
            })
            .clone()?;
        let mut path = HashedPath::clone(root);
        for name in below.into_iter().rev() {
            let name = name?;
            let is_dir = listing
                .entry(name)
                .and_then(|entry| entry.kind)
                .is_some_and(|kind| kind.is_dir());
            if !is_dir {
                return None;
            }
            path = path.child(name);
            listing = self.table.listing(&path)?;
        }
        Some(listing)
    }
}

/// Whether the walk of a directory of `dirs` is sure to list `file`, read
/// from the paths with a `stat` of the file and of each directory: the
/// directory nearest above the file is one of `dirs`, the file is a regular
/// file, and that directory and each directory down to the file's own can be
/// listed. `Walks` must agree with it.
#[cfg(test)]
fn walked_by_a_directory(
    file: &Path,
    dirs: &FxHashSet<&Path>,
    listable: &mut FxHashMap<PathBuf, bool>,
    table: &DirListings,
) -> bool {
    let Some(dir) = file
        .ancestors()
        .skip(1)
        .find(|ancestor| dirs.contains(ancestor))
    else {
        return false;
    };
    if !std::fs::symlink_metadata(file).is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    let Some(directories) = file.strip_prefix(dir).ok().and_then(Path::parent) else {
        return false;
    };
    let mut current = dir.to_path_buf();
    if !listable_dir(&current, listable, table) {
        return false;
    }
    for part in directories.components() {
        current.push(part);
        if !listable_dir(&current, listable, table) {
            return false;
        }
    }
    true
}

/// Whether a walk can list `dir`: it is a directory and not a link, and the
/// table has a listing of it. Remembered per directory.
#[cfg(test)]
fn listable_dir(dir: &Path, listable: &mut FxHashMap<PathBuf, bool>, table: &DirListings) -> bool {
    if let Some(&known) = listable.get(dir) {
        return known;
    }
    let dir = HashedPath::new(dir.to_path_buf());
    let known = std::fs::symlink_metadata(dir.as_path()).is_ok_and(|metadata| metadata.is_dir())
        && table.listing(&dir).is_some();
    listable.insert(dir.into_path_buf(), known);
    known
}
