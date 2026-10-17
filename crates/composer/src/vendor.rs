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

use hashbrown::{HashTable, hash_table};
use rustc_hash::FxHashMap;
#[cfg(test)]
use rustc_hash::FxHashSet;
use std::cell::OnceCell;
use std::ffi::OsStr;
use std::fs::FileType;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syntax::ast::{BinOp, ClassDecl, Expr, ExprKind, Literal, Stmt, StmtKind};

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

/// Discovers `vendor/` context for the Composer project rooted at
/// `project_dir`, or an empty `VendorContext` if `project_dir` isn't a
/// Composer project (no `composer.json`) or hasn't been installed yet (no
/// `vendor/composer/`). The directories it reads are read through `listings`,
/// which the walk of the same load shares.
pub fn discover_vendor_context(project_dir: &Path, listings: &DirListings) -> VendorContext {
    if !project_dir.join("composer.json").is_file() {
        return VendorContext::default();
    }
    let composer_dir = project_dir.join("vendor").join("composer");
    if !composer_dir.is_dir() {
        return VendorContext::default();
    }
    let Ok(vendor_dir) = std::fs::canonicalize(composer_dir.join("..")) else {
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
    let entries = discover_inner(&composer_dir, &vendor_dir, &base_dir, listings);
    std::panic::set_hook(default_hook);
    entries.into_context(listings)
}

fn discover_inner(
    composer_dir: &Path,
    vendor_dir: &Path,
    base_dir: &Path,
    listings: &DirListings,
) -> Entries {
    let mut resolver = Resolver::new(vendor_dir, base_dir, composer_dir, listings);
    let static_path = composer_dir.join("autoload_static.php");
    let mut entries = if static_path.is_file()
        && let Some(entries) = parse_static_file(&static_path, &mut resolver)
    {
        entries
    } else {
        parse_separate_files(composer_dir, &mut resolver)
    };
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

fn parse_static_file(path: &Path, resolver: &mut Resolver<'_>) -> Option<Entries> {
    let stmts = parse_generated_file(path)?;
    let class = find_class_decl(&stmts)?;

    let mut entries = Entries::default();
    for prop in &class.def.properties {
        if !prop.is_static {
            continue;
        }
        let Some(default) = &prop.default else {
            continue;
        };
        match prop.name.as_ref() {
            "classMap" | "files" => resolver.collect_files(default, &mut entries.files),
            "prefixDirsPsr4" => resolver.collect_dirs(default, &mut entries.dirs),
            _ => {}
        }
    }
    Some(entries)
}

fn parse_separate_files(composer_dir: &Path, resolver: &mut Resolver<'_>) -> Entries {
    let mut entries = Entries::default();
    for name in ["autoload_classmap.php", "autoload_files.php"] {
        let path = composer_dir.join(name);
        if let Some(arr) = parse_top_level_array(&path) {
            resolver.collect_files(&arr, &mut entries.files);
        }
    }
    for name in ["autoload_psr4.php", "autoload_namespaces.php"] {
        let path = composer_dir.join(name);
        if let Some(arr) = parse_top_level_array(&path) {
            resolver.collect_dirs(&arr, &mut entries.dirs);
        }
    }
    entries
}

fn parse_top_level_array(path: &Path) -> Option<Expr> {
    let stmts = parse_generated_file(path)?;
    stmts.into_iter().find_map(|s| match s.into_kind() {
        StmtKind::Return(Some(e)) if matches!(e.kind, ExprKind::ArrayLiteral(_)) => Some(e),
        _ => None,
    })
}

fn find_class_decl(stmts: &[Stmt]) -> Option<&ClassDecl> {
    stmts.iter().find_map(|s| match &s.kind {
        StmtKind::ClassDecl(c) => Some(&**c),
        _ => None,
    })
}

/// Same pattern as `main.rs::check_chunk`: PHP source isn't guaranteed
/// UTF-8, so a lossy read rather than a strict one; a malformed/future-
/// syntax generated file must degrade to "no context from this file",
/// never crash `check-project` - `parse_program`'s own `Result` already
/// gives that for free, no `catch_unwind` needed.
fn parse_generated_file(path: &Path) -> Option<Vec<Stmt>> {
    let bytes = std::fs::read(path).ok()?;
    syntax::parse(&bytes).ok().map(|parsed| parsed.stmts)
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
        if dir.starts_with(base) {
            Place::In
        } else if base.parent() == Some(dir) {
            Place::Above
        } else {
            Place::Out
        }
    }

    /// Whether the entry `name` of a directory placed so lies in `base`.
    pub(crate) fn holds(self, name: &OsStr, base: &Path) -> bool {
        match self {
            Place::In => true,
            Place::Above => base.file_name() == Some(name),
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
}

impl<'p> Resolver<'p> {
    fn new(
        vendor_dir: &'p Path,
        base_dir: &'p Path,
        generated_dir: &'p Path,
        table: &'p DirListings,
    ) -> Self {
        Resolver {
            vendor_dir,
            base_dir,
            generated_dir,
            vendor_as_written: lexically_normalized(&generated_dir.join("..")),
            table,
            listings: FxHashMap::default(),
            written: FxHashMap::default(),
        }
    }

    fn collect_files(&mut self, array: &Expr, out: &mut Vec<Located>) {
        let ExprKind::ArrayLiteral(items) = &array.kind else {
            return;
        };
        for item in items {
            if let Some(path) = self.path(&item.value) {
                out.push(path);
            }
        }
    }

    fn collect_dirs(&mut self, array: &Expr, out: &mut Vec<Located>) {
        let ExprKind::ArrayLiteral(items) = &array.kind else {
            return;
        };
        for item in items {
            let ExprKind::ArrayLiteral(dirs) = &item.value.kind else {
                continue;
            };
            for dir_item in dirs {
                if let Some(path) = self.path(&dir_item.value) {
                    out.push(path);
                }
            }
        }
    }

    /// Resolves one array value to an absolute path under `vendor_dir`, or
    /// `None` if it can't be evaluated, doesn't exist, or (the common,
    /// load-bearing case) resolves outside `vendor_dir` entirely - a
    /// `$baseDir`/`__DIR__`-with-extra-`/..`-rooted, first-party location,
    /// which must never be silently treated as vendor context. A path that
    /// resolves outside it but is written inside it is kept as written: a
    /// path-repository package is a `vendor/<name>` symlink to a directory
    /// outside `vendor/`, and still belongs to vendor context.
    fn path(&mut self, expr: &Expr) -> Option<Located> {
        let raw = eval_concat(expr, self.vendor_dir, self.base_dir, self.generated_dir)?;
        self.locate(path_of(&raw))
    }

    /// `raw` resolved as `path` resolves an array value's path.
    fn locate(&mut self, raw: &Path) -> Option<Located> {
        let unknown = |path| Located { path, kind: None };
        let (Some(dir), Some(name)) = (raw.parent(), raw.file_name()) else {
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
        let entry = listed.listing.entry(name)?;
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
                let resolved = std::fs::canonicalize(raw).ok()?;
                if resolved.starts_with(vendor_dir) {
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
        let resolved = std::fs::canonicalize(raw).ok()?;
        if resolved.starts_with(self.vendor_dir) {
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
        let Some(parent) = dir.parent() else {
            // The root.
            return canonicalized(dir);
        };
        if parent.as_os_str().is_empty() {
            return canonicalized(dir);
        }
        let Some(name) = dir.file_name() else {
            // Ends in `..`: the parent of what precedes it, as the system
            // reads it.
            return canonicalized(dir);
        };
        let Some(parent_listed) = self.listed(parent) else {
            return canonicalized(dir);
        };
        entry_canonical(parent_listed, name, dir)
    }
}

/// `path` resolved by the system.
fn canonicalized(path: &Path) -> Option<HashedPath> {
    std::fs::canonicalize(path).ok().map(HashedPath::new)
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

/// The path `bytes` spell.
#[cfg(unix)]
fn path_of(bytes: &[u8]) -> &Path {
    use std::os::unix::ffi::OsStrExt;
    Path::new(std::ffi::OsStr::from_bytes(bytes))
}

/// The path `bytes` spell, read lossily where they are not UTF-8.
#[cfg(not(unix))]
fn path_of(bytes: &[u8]) -> &Path {
    Path::new(std::str::from_utf8(bytes).unwrap_or_default())
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

/// Builds the bytes of the literal string a generated autoloader file's path
/// expression evaluates to. Required because real generated code chains
/// multiple concatenations (`__DIR__ . '/..' . '/pkg/path'`), not a flat
/// two-operand shape.
fn eval_concat(
    expr: &Expr,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    append_concat(expr, vendor_dir, base_dir, generated_dir, &mut out)?;
    Some(out)
}

/// Appends the bytes `expr` evaluates to onto `out`, which one `eval_concat`
/// call builds for the whole chain. `None` when `expr` is not such a literal;
/// the bytes appended before that are then not used.
fn append_concat(
    expr: &Expr,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
    out: &mut Vec<u8>,
) -> Option<()> {
    syntax::stack::guard(|| {
        let expr = expr.peeled();
        match &expr.kind {
            ExprKind::Literal(Literal::String(s)) => out.extend_from_slice(s.as_bytes()),
            // The directory containing the generated file itself, i.e.
            // `vendor/composer/` - `autoload_static.php` concatenates `/..`
            // or `/../..` onto this to reach `vendor/` or the project root.
            ExprKind::Name(n) if n == "__DIR__" => {
                out.extend_from_slice(generated_dir.as_os_str().as_encoded_bytes());
            }
            // Only appears in the separate, non-static autoload_*.php files.
            ExprKind::Variable(n) if **n == *"vendorDir" => {
                out.extend_from_slice(vendor_dir.as_os_str().as_encoded_bytes());
            }
            ExprKind::Variable(n) if **n == *"baseDir" => {
                out.extend_from_slice(base_dir.as_os_str().as_encoded_bytes());
            }
            ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
                append_concat(lhs, vendor_dir, base_dir, generated_dir, out)?;
                append_concat(rhs, vendor_dir, base_dir, generated_dir, out)?;
            }
            _ => return None,
        }
        Some(())
    })
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
        let (Some((dir, hash)), Some(name)) = (file.parent(), file.as_path().file_name()) else {
            return false;
        };
        is_php(Path::new(name))
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
        let mut below = Vec::new();
        let mut ancestors = hashed_ancestors(dir);
        let (root, checked) = loop {
            let (ancestor, hash) = ancestors.next()?;
            if let Some(root) = self
                .roots
                .find(hash, |(root, _)| root.as_path() == ancestor)
            {
                break root;
            }
            below.push(ancestor);
        };
        let mut listing = checked
            .get_or_init(|| {
                let is_dir = std::fs::symlink_metadata(root.as_path())
                    .is_ok_and(|metadata| metadata.is_dir());
                is_dir.then(|| self.table.listing(root)).flatten()
            })
            .clone()?;
        let mut path = HashedPath::clone(root);
        for ancestor in below.into_iter().rev() {
            let name = ancestor.file_name()?;
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
