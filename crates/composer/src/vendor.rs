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

use rustc_hash::{FxHashMap, FxHashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syntax::ast::{BinOp, ClassDecl, Expr, ExprKind, Literal, Stmt, StmtKind};

use crate::dir_listings::{DirListings, Listing};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct VendorContext {
    /// Exact files to load (classmap/files entries), except the PHP files that
    /// the walk of a directory of `dirs` lists.
    pub files: Vec<PathBuf>,
    /// Directories to recursively load (PSR-4 base dirs).
    pub dirs: Vec<PathBuf>,
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
    let mut ctx = discover_inner(&composer_dir, &vendor_dir, &base_dir, listings);
    std::panic::set_hook(default_hook);

    // Composer's own bootstrap loader class - unlike every other file
    // this function discovers, it's never listed in any of the generated
    // autoload maps at all (it's what *runs* them: `vendor/autoload.php`
    // requires it directly, before the classmap/PSR-4 mechanisms it
    // reads even exist). Present unconditionally whenever
    // `vendor/composer/` exists at all, so always worth loading for
    // context - real code occasionally constructs it directly for its
    // own plugin/dynamic-autoloading needs.
    let class_loader = composer_dir.join("ClassLoader.php");
    if class_loader.is_file() {
        ctx.files.push(class_loader);
    }
    ctx
}

fn discover_inner(
    composer_dir: &Path,
    vendor_dir: &Path,
    base_dir: &Path,
    listings: &DirListings,
) -> VendorContext {
    let static_path = composer_dir.join("autoload_static.php");
    if static_path.is_file()
        && let Some(ctx) = parse_static_file(&static_path, vendor_dir, base_dir, listings)
    {
        return ctx;
    }
    parse_separate_files(composer_dir, vendor_dir, base_dir, listings)
}

fn parse_static_file(
    path: &Path,
    vendor_dir: &Path,
    base_dir: &Path,
    listings: &DirListings,
) -> Option<VendorContext> {
    let generated_dir = path.parent()?;
    let stmts = parse_generated_file(path)?;
    let class = find_class_decl(&stmts)?;

    let mut ctx = VendorContext::default();
    let mut resolver = Resolver::new(vendor_dir, base_dir, generated_dir, listings);
    for prop in &class.def.properties {
        if !prop.is_static {
            continue;
        }
        let Some(default) = &prop.default else {
            continue;
        };
        match prop.name.as_ref() {
            "classMap" | "files" => resolver.collect_files(default, &mut ctx.files),
            "prefixDirsPsr4" => resolver.collect_dirs(default, &mut ctx.dirs),
            _ => {}
        }
    }
    dedup(&mut ctx, listings);
    Some(ctx)
}

fn parse_separate_files(
    composer_dir: &Path,
    vendor_dir: &Path,
    base_dir: &Path,
    listings: &DirListings,
) -> VendorContext {
    let mut ctx = VendorContext::default();
    let mut resolver = Resolver::new(vendor_dir, base_dir, composer_dir, listings);
    for name in ["autoload_classmap.php", "autoload_files.php"] {
        let path = composer_dir.join(name);
        if let Some(arr) = parse_top_level_array(&path) {
            resolver.collect_files(&arr, &mut ctx.files);
        }
    }
    for name in ["autoload_psr4.php", "autoload_namespaces.php"] {
        let path = composer_dir.join(name);
        if let Some(arr) = parse_top_level_array(&path) {
            resolver.collect_dirs(&arr, &mut ctx.dirs);
        }
    }
    dedup(&mut ctx, listings);
    ctx
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

/// A directory the resolver found: its canonical form, and its listing.
struct Listed {
    canonical: PathBuf,
    listing: Arc<Listing>,
}

/// Resolves a generated autoloader's path expressions against its vendor,
/// base and generated directories.
struct Resolver<'p> {
    vendor_dir: &'p Path,
    base_dir: &'p Path,
    generated_dir: &'p Path,
    /// The directory listings of the load, by canonical path.
    table: &'p DirListings,
    // Each directory's listing, made once: autoload entries share a few
    // thousand directories among tens of thousands of files, and a listing
    // tells whether an entry exists and whether it is a link, which a
    // `stat` of each file would. Keyed by the directory's text as written,
    // which hashes as bytes - a `Path` hashes component by component. None
    // for a directory that has no canonical form or cannot be listed.
    listings: FxHashMap<Vec<u8>, Option<Listed>>,
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
            table,
            listings: FxHashMap::default(),
        }
    }

    fn collect_files(&mut self, array: &Expr, out: &mut Vec<PathBuf>) {
        let ExprKind::ArrayLiteral(items) = &array.kind else {
            return;
        };
        for item in items {
            if let Some(path) = self.path(&item.value) {
                out.push(path);
            }
        }
    }

    fn collect_dirs(&mut self, array: &Expr, out: &mut Vec<PathBuf>) {
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
    /// which must never be silently treated as vendor context.
    fn path(&mut self, expr: &Expr) -> Option<PathBuf> {
        let raw = eval_concat(expr, self.vendor_dir, self.base_dir, self.generated_dir)?;
        let raw = path_of(&raw);
        // Containment is decided on the path as written, not its canonical
        // form: a path-repository package is a `vendor/<name>` symlink to a
        // directory outside `vendor/`, and still belongs to vendor context.
        let resolved = self.canonical(raw)?;
        if resolved.starts_with(self.vendor_dir) {
            return Some(resolved);
        }
        let lexical = lexically_normalized(raw);
        let vendor_as_written = lexically_normalized(&self.generated_dir.join(".."));
        (lexical.starts_with(self.vendor_dir) || lexical.starts_with(&vendor_as_written))
            .then_some(lexical)
    }

    /// `std::fs::canonicalize(path)`, through its directory's canonical
    /// form when the entry itself isn't a link, and none for an entry its
    /// directory does not list.
    fn canonical(&mut self, path: &Path) -> Option<PathBuf> {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return std::fs::canonicalize(path).ok();
        };
        if dir.as_os_str().is_empty() {
            return std::fs::canonicalize(path).ok();
        }
        let Some(listed) = self.listed(dir) else {
            // A directory that can't be listed (no permission to read it
            // but to pass through it) is still resolved as the system does.
            return std::fs::canonicalize(path).ok();
        };
        entry_canonical(listed, name, path)
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
        listing.complete.then_some(Listed { canonical, listing })
    }

    /// The canonical form of `dir`: its parent's, joined with its name,
    /// unless the parent lists it as a link, which is the one case that
    /// needs the system to follow it.
    fn canonical_dir(&mut self, dir: &Path) -> Option<PathBuf> {
        let Some(parent) = dir.parent() else {
            // The root.
            return std::fs::canonicalize(dir).ok();
        };
        if parent.as_os_str().is_empty() {
            return std::fs::canonicalize(dir).ok();
        }
        let Some(name) = dir.file_name() else {
            // Ends in `..`: the parent of what precedes it, as the system
            // reads it.
            return std::fs::canonicalize(dir).ok();
        };
        let Some(parent_listed) = self.listed(parent) else {
            return std::fs::canonicalize(dir).ok();
        };
        entry_canonical(parent_listed, name, dir)
    }
}

/// The canonical form of the entry `name` of a listed directory, or `None`
/// when the listing does not have it. A link, or an entry whose type the
/// system does not give, is resolved by the system at `path`; any other is
/// its directory's canonical form joined with its name.
fn entry_canonical(listed: &Listed, name: &OsStr, path: &Path) -> Option<PathBuf> {
    let entry = listed.listing.entry(name)?;
    match entry.kind {
        Some(kind) if !kind.is_symlink() => Some(listed.canonical.join(name)),
        _ => std::fs::canonicalize(path).ok(),
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

/// Recursively builds the bytes of the literal string a generated
/// autoloader file's path expression evaluates to. Required because real
/// generated code chains multiple concatenations (`__DIR__ . '/..' .
/// '/pkg/path'`), not a flat two-operand shape.
fn eval_concat(
    expr: &Expr,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
) -> Option<Vec<u8>> {
    let bytes_of = |dir: &Path| dir.as_os_str().as_encoded_bytes().to_vec();
    syntax::stack::guard(|| {
        let expr = expr.peeled();
        match &expr.kind {
            ExprKind::Literal(Literal::String(s)) => Some(s.as_bytes().to_vec()),
            // The directory containing the generated file itself, i.e.
            // `vendor/composer/` - `autoload_static.php` concatenates `/..`
            // or `/../..` onto this to reach `vendor/` or the project root.
            ExprKind::Name(n) if n == "__DIR__" => Some(bytes_of(generated_dir)),
            // Only appears in the separate, non-static autoload_*.php files.
            ExprKind::Variable(n) if **n == *"vendorDir" => Some(bytes_of(vendor_dir)),
            ExprKind::Variable(n) if **n == *"baseDir" => Some(bytes_of(base_dir)),
            ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
                let mut left = eval_concat(lhs, vendor_dir, base_dir, generated_dir)?;
                let right = eval_concat(rhs, vendor_dir, base_dir, generated_dir)?;
                left.extend_from_slice(&right);
                Some(left)
            }
            _ => None,
        }
    })
}

/// Sorts and deduplicates both lists, then leaves out the files that a
/// directory of the lists loads.
fn dedup(ctx: &mut VendorContext, table: &DirListings) {
    crate::path_order::sort_paths(&mut ctx.dirs);
    ctx.dirs.dedup();
    drop_files_under_dirs(ctx, table);
    crate::path_order::sort_paths(&mut ctx.files);
    ctx.files.dedup();
}

/// Drops the PHP files that the walk of a directory of `ctx.dirs` is sure to
/// list: the walk lists them already, and listing them again adds nothing.
/// Other files stay, as an explicit file is listed whatever its extension.
fn drop_files_under_dirs(ctx: &mut VendorContext, table: &DirListings) {
    let dirs: FxHashSet<&Path> = ctx.dirs.iter().map(PathBuf::as_path).collect();
    let mut listable = FxHashMap::default();
    ctx.files
        .retain(|file| !(is_php(file) && walked_by_a_directory(file, &dirs, &mut listable, table)));
}

fn is_php(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "php")
}

/// Whether the walk of a directory of `dirs` is sure to list `file`: the
/// directory nearest above the file is one of `dirs`, the file is a regular
/// file, and that directory and each directory down to the file's own can be
/// listed. A file that this does not hold for stays listed explicitly, so a
/// file the walk could miss (a missing one, or one behind a directory that
/// cannot be read) is still listed.
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
fn listable_dir(dir: &Path, listable: &mut FxHashMap<PathBuf, bool>, table: &DirListings) -> bool {
    if let Some(&known) = listable.get(dir) {
        return known;
    }
    let known = std::fs::symlink_metadata(dir).is_ok_and(|metadata| metadata.is_dir())
        && table.listing(dir).is_some();
    listable.insert(dir.to_path_buf(), known);
    known
}
