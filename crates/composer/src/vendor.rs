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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use syntax::ast::{BinOp, ClassDecl, Expr, ExprKind, Literal, Stmt, StmtKind};
use syntax::name::Interner;

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
    let mut ctx = discover_inner(&composer_dir, &vendor_dir, &base_dir);
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

fn discover_inner(composer_dir: &Path, vendor_dir: &Path, base_dir: &Path) -> VendorContext {
    let static_path = composer_dir.join("autoload_static.php");
    if static_path.is_file()
        && let Some(ctx) = parse_static_file(&static_path, vendor_dir, base_dir)
    {
        return ctx;
    }
    parse_separate_files(composer_dir, vendor_dir, base_dir)
}

fn parse_static_file(path: &Path, vendor_dir: &Path, base_dir: &Path) -> Option<VendorContext> {
    let generated_dir = path.parent()?;
    let stmts = parse_generated_file(path)?;
    let class = find_class_decl(&stmts)?;

    let mut ctx = VendorContext::default();
    let mut resolver = Resolver::new(vendor_dir, base_dir, generated_dir);
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
    dedup(&mut ctx);
    Some(ctx)
}

fn parse_separate_files(composer_dir: &Path, vendor_dir: &Path, base_dir: &Path) -> VendorContext {
    let mut ctx = VendorContext::default();
    let mut resolver = Resolver::new(vendor_dir, base_dir, composer_dir);
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
    dedup(&mut ctx);
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
        StmtKind::ClassDecl(c) => Some(c),
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
    syntax::parse(&bytes, Interner::global())
        .ok()
        .map(|parsed| parsed.stmts)
}

/// Resolves a generated autoloader's path expressions against its vendor,
/// base and generated directories.
struct Resolver<'p> {
    vendor_dir: &'p Path,
    base_dir: &'p Path,
    generated_dir: &'p Path,
    // Each directory's canonical form, found once: autoload entries share
    // a few thousand directories among tens of thousands of files.
    canonical_dirs: HashMap<PathBuf, Option<PathBuf>>,
}

impl<'p> Resolver<'p> {
    fn new(vendor_dir: &'p Path, base_dir: &'p Path, generated_dir: &'p Path) -> Self {
        Resolver {
            vendor_dir,
            base_dir,
            generated_dir,
            canonical_dirs: HashMap::new(),
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
        // Containment is decided on the path as written, not its canonical
        // form: a path-repository package is a `vendor/<name>` symlink to a
        // directory outside `vendor/`, and still belongs to vendor context.
        let resolved = self.canonical(Path::new(&raw))?;
        if resolved.starts_with(self.vendor_dir) {
            return Some(resolved);
        }
        let lexical = lexically_normalized(Path::new(&raw));
        let vendor_as_written = lexically_normalized(&self.generated_dir.join(".."));
        (lexical.starts_with(self.vendor_dir) || lexical.starts_with(&vendor_as_written))
            .then_some(lexical)
    }

    /// `std::fs::canonicalize(path)`, through its directory's canonical
    /// form when the entry itself isn't a link.
    fn canonical(&mut self, path: &Path) -> Option<PathBuf> {
        let entry = std::fs::symlink_metadata(path).ok()?;
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return std::fs::canonicalize(path).ok();
        };
        if entry.file_type().is_symlink() || dir.as_os_str().is_empty() {
            return std::fs::canonicalize(path).ok();
        }
        let dir = self
            .canonical_dirs
            .entry(dir.to_path_buf())
            .or_insert_with(|| std::fs::canonicalize(dir).ok())
            .as_ref()?;
        Some(dir.join(name))
    }
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

/// Recursively builds the literal string a generated autoloader file's
/// path expression evaluates to. Required because real generated code
/// chains multiple concatenations (`__DIR__ . '/..' . '/pkg/path'`), not
/// a flat two-operand shape.
fn eval_concat(
    expr: &Expr,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
) -> Option<String> {
    syntax::stack::guard(|| {
        let expr = expr.peeled();
        match &expr.kind {
            ExprKind::Literal(Literal::String(s)) => Some(s.to_string()),
            // The directory containing the generated file itself, i.e.
            // `vendor/composer/` - `autoload_static.php` concatenates `/..`
            // or `/../..` onto this to reach `vendor/` or the project root.
            ExprKind::Name(n) if n == "__DIR__" => {
                Some(generated_dir.to_string_lossy().into_owned())
            }
            // Only appears in the separate, non-static autoload_*.php files.
            ExprKind::Variable(n) if **n == *"vendorDir" => {
                Some(vendor_dir.to_string_lossy().into_owned())
            }
            ExprKind::Variable(n) if **n == *"baseDir" => {
                Some(base_dir.to_string_lossy().into_owned())
            }
            ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
                let mut left = eval_concat(lhs, vendor_dir, base_dir, generated_dir)?;
                let right = eval_concat(rhs, vendor_dir, base_dir, generated_dir)?;
                left.push_str(&right);
                Some(left)
            }
            _ => None,
        }
    })
}

fn dedup(ctx: &mut VendorContext) {
    ctx.files.sort();
    ctx.files.dedup();
    ctx.dirs.sort();
    ctx.dirs.dedup();
}
