//! Reads a project's real, installed Composer autoloader
//! (`vendor/composer/autoload_static.php`, or the older separate
//! `autoload_classmap.php`/`autoload_psr4.php`/`autoload_namespaces.php`/
//! `autoload_files.php`) to discover `vendor/` files and directories worth
//! loading for type-resolution context - without reporting diagnostics on
//! them. This is the "load for context, don't report" path split real
//! PHPStan gets from its own `paths:` vs. Composer-autoloader split (see
//! `ROADMAP.md`'s Phase 13 note this replaces).
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

use std::path::{Path, PathBuf};

use crate::ast::{BinOp, ClassDecl, Expr, ExprKind, Literal, Stmt, StmtKind};
use crate::lexer::Lexer;
use crate::parser::Parser;

#[derive(Debug, Default, PartialEq)]
pub struct ComposerContext {
    /// Exact files to load (classmap/files entries).
    pub files: Vec<PathBuf>,
    /// Directories to recursively load (PSR-4 base dirs).
    pub dirs: Vec<PathBuf>,
}

/// Discovers `vendor/` context for the Composer project rooted at
/// `project_dir`, or an empty `ComposerContext` if `project_dir` isn't a
/// Composer project (no `composer.json`) or hasn't been installed yet (no
/// `vendor/composer/`).
pub fn discover_vendor_context(project_dir: &Path) -> ComposerContext {
    if !project_dir.join("composer.json").is_file() {
        return ComposerContext::default();
    }
    let composer_dir = project_dir.join("vendor").join("composer");
    if !composer_dir.is_dir() {
        return ComposerContext::default();
    }
    let Ok(vendor_dir) = std::fs::canonicalize(composer_dir.join("..")) else {
        return ComposerContext::default();
    };
    let Some(base_dir) = vendor_dir.parent().map(Path::to_path_buf) else {
        return ComposerContext::default();
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
    // autoload maps at all (it's what *runs* them, a real chicken-and-
    // egg case: `vendor/autoload.php` requires it directly, before the
    // classmap/PSR-4 mechanisms it reads even exist). Present
    // unconditionally whenever `vendor/composer/` exists at all, so
    // always worth loading for context - real code occasionally
    // constructs it directly for its own plugin/dynamic-autoloading
    // needs (confirmed live: Piwigo's own `PluginRegistry`/
    // `ThemeRegistry`, registering each installed plugin's own PSR-4
    // mapping via a fresh `new ClassLoader()`).
    let class_loader = composer_dir.join("ClassLoader.php");
    if class_loader.is_file() {
        ctx.files.push(class_loader);
    }
    ctx
}

fn discover_inner(composer_dir: &Path, vendor_dir: &Path, base_dir: &Path) -> ComposerContext {
    let static_path = composer_dir.join("autoload_static.php");
    if static_path.is_file()
        && let Some(ctx) = parse_static_file(&static_path, vendor_dir, base_dir)
    {
        return ctx;
    }
    parse_separate_files(composer_dir, vendor_dir, base_dir)
}

fn parse_static_file(path: &Path, vendor_dir: &Path, base_dir: &Path) -> Option<ComposerContext> {
    let generated_dir = path.parent()?;
    let stmts = parse_generated_file(path)?;
    let class = find_class_decl(&stmts)?;

    let mut ctx = ComposerContext::default();
    for prop in &class.properties {
        if !prop.is_static {
            continue;
        }
        let Some(default) = &prop.default else {
            continue;
        };
        match prop.name.as_str() {
            "classMap" | "files" => {
                collect_files(default, vendor_dir, base_dir, generated_dir, &mut ctx.files)
            }
            "prefixDirsPsr4" => {
                collect_dirs(default, vendor_dir, base_dir, generated_dir, &mut ctx.dirs)
            }
            _ => {}
        }
    }
    dedup(&mut ctx);
    Some(ctx)
}

fn parse_separate_files(
    composer_dir: &Path,
    vendor_dir: &Path,
    base_dir: &Path,
) -> ComposerContext {
    let mut ctx = ComposerContext::default();
    for name in ["autoload_classmap.php", "autoload_files.php"] {
        let path = composer_dir.join(name);
        if let Some(arr) = parse_top_level_array(&path) {
            collect_files(&arr, vendor_dir, base_dir, composer_dir, &mut ctx.files);
        }
    }
    for name in ["autoload_psr4.php", "autoload_namespaces.php"] {
        let path = composer_dir.join(name);
        if let Some(arr) = parse_top_level_array(&path) {
            collect_dirs(&arr, vendor_dir, base_dir, composer_dir, &mut ctx.dirs);
        }
    }
    dedup(&mut ctx);
    ctx
}

fn parse_top_level_array(path: &Path) -> Option<Expr> {
    let stmts = parse_generated_file(path)?;
    stmts.into_iter().find_map(|s| match s.kind {
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
    let source = String::from_utf8_lossy(&bytes).into_owned();
    let tokens = Lexer::new(&source).tokenize_with_spans();
    Parser::new(tokens).parse_program().ok()
}

fn collect_files(
    array: &Expr,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
    out: &mut Vec<PathBuf>,
) {
    let ExprKind::ArrayLiteral(items) = &array.kind else {
        return;
    };
    for item in items {
        if let Some(path) = eval_path(&item.value, vendor_dir, base_dir, generated_dir) {
            out.push(path);
        }
    }
}

fn collect_dirs(
    array: &Expr,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
    out: &mut Vec<PathBuf>,
) {
    let ExprKind::ArrayLiteral(items) = &array.kind else {
        return;
    };
    for item in items {
        let ExprKind::ArrayLiteral(dirs) = &item.value.kind else {
            continue;
        };
        for dir_item in dirs {
            if let Some(path) = eval_path(&dir_item.value, vendor_dir, base_dir, generated_dir) {
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
fn eval_path(
    expr: &Expr,
    vendor_dir: &Path,
    base_dir: &Path,
    generated_dir: &Path,
) -> Option<PathBuf> {
    let raw = eval_concat(expr, vendor_dir, base_dir, generated_dir)?;
    let resolved = std::fs::canonicalize(raw).ok()?;
    if resolved.starts_with(vendor_dir) {
        Some(resolved)
    } else {
        None
    }
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
    match &expr.kind {
        ExprKind::Literal(Literal::String(s)) => Some(s.clone()),
        // The directory containing the generated file itself, i.e.
        // `vendor/composer/` - `autoload_static.php` concatenates `/..`
        // or `/../..` onto this to reach `vendor/` or the project root.
        ExprKind::Name(n) if n == "__DIR__" => Some(generated_dir.to_string_lossy().into_owned()),
        // Only appears in the separate, non-static autoload_*.php files.
        ExprKind::Variable(n) if n == "vendorDir" => {
            Some(vendor_dir.to_string_lossy().into_owned())
        }
        ExprKind::Variable(n) if n == "baseDir" => Some(base_dir.to_string_lossy().into_owned()),
        ExprKind::Binary(BinOp::Concat, lhs, rhs) => {
            let mut left = eval_concat(lhs, vendor_dir, base_dir, generated_dir)?;
            let right = eval_concat(rhs, vendor_dir, base_dir, generated_dir)?;
            left.push_str(&right);
            Some(left)
        }
        ExprKind::Grouping(inner) => eval_concat(inner, vendor_dir, base_dir, generated_dir),
        _ => None,
    }
}

fn dedup(ctx: &mut ComposerContext) {
    ctx.files.sort();
    ctx.files.dedup();
    ctx.dirs.sort();
    ctx.dirs.dedup();
}

#[cfg(test)]
mod tests;
