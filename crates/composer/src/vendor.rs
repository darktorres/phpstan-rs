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
use std::path::{Path, PathBuf};
use std::sync::Arc;

use syntax::ParsedFile;
use syntax::ast::{BinOp, ClassDecl, ExprId, ExprKind, Literal, NodeArenas, StmtKind};

use crate::dir_table::{DirTable, EntryKind, Listing};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct VendorContext {
    /// Exact files to load (classmap/files entries).
    pub files: Vec<PathBuf>,
    /// Directories to recursively load (PSR-4 base dirs).
    pub dirs: Vec<PathBuf>,
}

/// Discovers `vendor/` context for the Composer project rooted at
/// `project_dir`, or an empty `VendorContext` if `project_dir` isn't a
/// Composer project (no `composer.json`) or hasn't been installed yet (no
/// `vendor/composer/`).
pub fn discover_vendor_context(project_dir: &Path) -> VendorContext {
    discover_vendor_context_in(project_dir, &DirTable::default())
}

/// `discover_vendor_context`, listing each directory through `table`,
/// which whoever walks the same directories reads too.
pub fn discover_vendor_context_in(project_dir: &Path, table: &DirTable) -> VendorContext {
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
    let mut ctx = discover_inner(&composer_dir, &vendor_dir, &base_dir, table);
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
    table: &DirTable,
) -> VendorContext {
    let static_path = composer_dir.join("autoload_static.php");
    if static_path.is_file()
        && let Some(ctx) = parse_static_file(&static_path, vendor_dir, base_dir, table)
    {
        return ctx;
    }
    parse_separate_files(composer_dir, vendor_dir, base_dir, table)
}

fn parse_static_file(
    path: &Path,
    vendor_dir: &Path,
    base_dir: &Path,
    table: &DirTable,
) -> Option<VendorContext> {
    let generated_dir = path.parent()?;
    let parsed = parse_generated_file(path)?;
    let class = find_class_decl(&parsed)?;
    let def = parsed.arenas.class(class.def)?;

    let mut ctx = VendorContext::default();
    let mut resolver = Resolver::new(vendor_dir, base_dir, generated_dir, table);
    for prop in def.properties(&parsed.arenas) {
        if !prop.is_static {
            continue;
        }
        let Some(default) = &prop.default else {
            continue;
        };
        match prop.name.as_ref() {
            "classMap" | "files" => {
                resolver.collect_files(&parsed.arenas, *default, &mut ctx.files)
            }
            "prefixDirsPsr4" => resolver.collect_dirs(&parsed.arenas, *default, &mut ctx.dirs),
            _ => {}
        }
    }
    resolver.settle(&mut ctx);
    Some(ctx)
}

fn parse_separate_files(
    composer_dir: &Path,
    vendor_dir: &Path,
    base_dir: &Path,
    table: &DirTable,
) -> VendorContext {
    let mut ctx = VendorContext::default();
    let mut resolver = Resolver::new(vendor_dir, base_dir, composer_dir, table);
    for name in ["autoload_classmap.php", "autoload_files.php"] {
        let path = composer_dir.join(name);
        if let Some((parsed, arr)) = parse_top_level_array(&path) {
            resolver.collect_files(&parsed.arenas, arr, &mut ctx.files);
        }
    }
    for name in ["autoload_psr4.php", "autoload_namespaces.php"] {
        let path = composer_dir.join(name);
        if let Some((parsed, arr)) = parse_top_level_array(&path) {
            resolver.collect_dirs(&parsed.arenas, arr, &mut ctx.dirs);
        }
    }
    resolver.settle(&mut ctx);
    ctx
}

fn parse_top_level_array(path: &Path) -> Option<(ParsedFile, ExprId)> {
    let parsed = parse_generated_file(path)?;
    let expr = parsed.statements().iter().find_map(|stmt| {
        let stmt = parsed.stmt(*stmt)?;
        match &stmt.kind {
            StmtKind::Return(Some(expr))
                if matches!(parsed.expr(*expr)?.kind, ExprKind::ArrayLiteral(_)) =>
            {
                Some(*expr)
            }
            _ => None,
        }
    })?;
    Some((parsed, expr))
}

fn find_class_decl(parsed: &ParsedFile) -> Option<&ClassDecl> {
    parsed
        .statements()
        .iter()
        .find_map(|stmt| match &parsed.stmt(*stmt)?.kind {
            StmtKind::ClassDecl(c) => Some(parsed.arenas.class_decl(c.clone())),
            _ => None,
        })
}

/// Same pattern as `main.rs::check_chunk`: PHP source isn't guaranteed
/// UTF-8, so a lossy read rather than a strict one; a malformed/future-
/// syntax generated file must degrade to "no context from this file",
/// never crash `check-project` - `parse_program`'s own `Result` already
/// gives that for free, no `catch_unwind` needed.
fn parse_generated_file(path: &Path) -> Option<ParsedFile> {
    let bytes = std::fs::read(path).ok()?;
    syntax::parse(&bytes).ok()
}

/// A directory that was listed: its canonical form, and its entries.
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
    table: &'p DirTable,
    // Each directory's listing, made once: autoload entries share a few
    // thousand directories among tens of thousands of files, and a listing
    // tells whether an entry exists and whether it is a link, which a
    // `stat` of each file would. Keyed by the directory's text as written,
    // which hashes as bytes - a `Path` hashes component by component. None
    // for a directory that has no canonical form or cannot be listed. The
    // listing itself is the table's.
    listings: FxHashMap<Vec<u8>, Option<Listed>>,
}

impl<'p> Resolver<'p> {
    fn new(
        vendor_dir: &'p Path,
        base_dir: &'p Path,
        generated_dir: &'p Path,
        table: &'p DirTable,
    ) -> Self {
        Resolver {
            vendor_dir,
            base_dir,
            generated_dir,
            table,
            listings: FxHashMap::default(),
        }
    }

    fn collect_files(&mut self, arenas: &NodeArenas, array: ExprId, out: &mut Vec<PathBuf>) {
        let Some(array) = arenas.expr(array) else {
            return;
        };
        let ExprKind::ArrayLiteral(items) = &array.kind else {
            return;
        };
        for item in arenas.array_items(items.clone()) {
            if let Some(path) = self.path(arenas, item.value) {
                out.push(path);
            }
        }
    }

    fn collect_dirs(&mut self, arenas: &NodeArenas, array: ExprId, out: &mut Vec<PathBuf>) {
        let Some(array) = arenas.expr(array) else {
            return;
        };
        let ExprKind::ArrayLiteral(items) = &array.kind else {
            return;
        };
        for item in arenas.array_items(items.clone()) {
            let Some(value) = arenas.expr(item.value) else {
                continue;
            };
            let ExprKind::ArrayLiteral(dirs) = &value.kind else {
                continue;
            };
            for dir_item in arenas.array_items(dirs.clone()) {
                if let Some(path) = self.path(arenas, dir_item.value) {
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
    fn path(&mut self, arenas: &NodeArenas, expr: ExprId) -> Option<PathBuf> {
        let raw = eval_concat(
            arenas,
            expr,
            self.vendor_dir,
            self.base_dir,
            self.generated_dir,
        )?;
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
        match listed.listing.kind_of(name) {
            None => None,
            Some(EntryKind::Link) => std::fs::canonicalize(path).ok(),
            Some(EntryKind::Dir | EntryKind::File) => Some(listed.canonical.join(name)),
        }
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

    fn list(&mut self, dir: &Path) -> Option<Listed> {
        let canonical = self.canonical_dir(dir)?;
        let listing = self.table.list(dir)?;
        Some(Listed { canonical, listing })
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
        match parent_listed.listing.kind_of(name) {
            None => None,
            Some(EntryKind::Link) => std::fs::canonicalize(dir).ok(),
            Some(EntryKind::Dir | EntryKind::File) => Some(parent_listed.canonical.join(name)),
        }
    }
}

impl Resolver<'_> {
    /// Sorts and deduplicates what was found, and leaves out the files a
    /// walk of the directories lists anyway: a `.php` file that is not a
    /// link, under one of `ctx.dirs` through directories that are not
    /// links, is listed by that directory's walk under this very spelling.
    fn settle(&mut self, ctx: &mut VendorContext) {
        crate::path_order::sort_paths(&mut ctx.dirs);
        ctx.dirs.dedup();
        let roots: FxHashSet<&[u8]> = ctx
            .dirs
            .iter()
            .map(|dir| dir.as_os_str().as_encoded_bytes())
            .collect();
        let mut reached = FxHashMap::default();
        let files = std::mem::take(&mut ctx.files);
        ctx.files = files
            .into_iter()
            .filter(|file| !self.walked(file, &roots, &mut reached))
            .collect();
        crate::path_order::sort_paths(&mut ctx.files);
        ctx.files.dedup();
    }

    /// Whether a walk of `roots` lists `file`.
    fn walked(
        &mut self,
        file: &Path,
        roots: &FxHashSet<&[u8]>,
        reached: &mut FxHashMap<Vec<u8>, bool>,
    ) -> bool {
        let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
            return false;
        };
        file.extension().is_some_and(|e| e == "php")
            && self
                .listed(dir)
                .is_some_and(|listed| listed.listing.kind_of(name) == Some(EntryKind::File))
            && self.reached(dir, roots, reached)
    }

    /// Whether a walk of `roots` goes into `dir`: it is one of them, or in
    /// one that is gone into, as a directory its parent lists as one.
    fn reached(
        &mut self,
        dir: &Path,
        roots: &FxHashSet<&[u8]>,
        reached: &mut FxHashMap<Vec<u8>, bool>,
    ) -> bool {
        let key = dir.as_os_str().as_encoded_bytes();
        if let Some(&known) = reached.get(key) {
            return known;
        }
        let gone_into = if roots.contains(key) {
            self.listed(dir).is_some()
        } else {
            match (dir.parent(), dir.file_name()) {
                (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
                    self.reached(parent, roots, reached)
                        && self.listed(parent).is_some_and(|listed| {
                            listed.listing.kind_of(name) == Some(EntryKind::Dir)
                        })
                }
                _ => false,
            }
        };
        reached.insert(key.to_vec(), gone_into);
        gone_into
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

/// The bytes of the literal string a generated autoloader file's path
/// expression evaluates to, built into one buffer. Required because real
/// generated code chains multiple concatenations (`__DIR__ . '/..' .
/// '/pkg/path'`), not a flat two-operand shape.
fn eval_concat(
    arenas: &NodeArenas,
    expr: ExprId,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    append_concat(arenas, expr, vendor_dir, base_dir, generated_dir, &mut out)?;
    Some(out)
}

/// Appends to `out` what `expr` evaluates to; where it does not evaluate
/// to a literal string, `out` holds what came before and is not used.
fn append_concat(
    arenas: &NodeArenas,
    expr: ExprId,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
    out: &mut Vec<u8>,
) -> Option<()> {
    fn bytes_of(dir: &Path) -> &[u8] {
        dir.as_os_str().as_encoded_bytes()
    }
    syntax::stack::guard(|| {
        let expr = arenas.peeled(expr)?;
        let expr = arenas.expr(expr)?;
        match &expr.kind {
            ExprKind::Literal(Literal::String(s)) => {
                out.extend_from_slice(arenas.string_bytes(s)?);
            }
            // The directory containing the generated file itself, i.e.
            // `vendor/composer/` - `autoload_static.php` concatenates `/..`
            // or `/../..` onto this to reach `vendor/` or the project root.
            ExprKind::Name(n) if arenas.qualified_name(n).is_some_and(|n| n == "__DIR__") => {
                out.extend_from_slice(bytes_of(generated_dir))
            }
            // Only appears in the separate, non-static autoload_*.php files.
            ExprKind::Variable(n) if **n == *"vendorDir" => {
                out.extend_from_slice(bytes_of(vendor_dir))
            }
            ExprKind::Variable(n) if **n == *"baseDir" => out.extend_from_slice(bytes_of(base_dir)),
            ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
                append_concat(arenas, *lhs, vendor_dir, base_dir, generated_dir, out)?;
                append_concat(arenas, *rhs, vendor_dir, base_dir, generated_dir, out)?;
            }
            _ => return None,
        }
        Some(())
    })
}
