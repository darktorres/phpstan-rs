//! `RequireFileExistsRule`: an `include`/`require` (and `_once`) whose
//! path resolves to known strings, none of which name an existing file.
//! Paths are resolved from string literals, constant string types,
//! `__DIR__`/`__FILE__`, `dirname()`, and concatenations of these, then
//! looked up relative to the working directory and the including file's
//! own directory, and each `include_path` entry - PHP's default plus any
//! `set_include_path()`/`ini_set('include_path', ...)` in the file. A
//! constant declared in the same file resolves through its initializer,
//! so `__DIR__` in it means this file's directory. A path the file also tests with `file_exists()`/
//! `is_file()`/`is_readable()` is left alone.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

use crate::checker::{Checker, Locals};
use crate::registry::class_decl_name;
use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::{Visitor, any_child_expr, walk_stmt, walk_stmts};

const GUARD_FUNCTIONS: [&str; 6] = [
    "file_exists",
    "is_file",
    "is_readable",
    "is_writable",
    "is_writeable",
    "is_executable",
];

/// PHP's compiled-in default `include_path`.
const DEFAULT_INCLUDE_PATH: &str = ".:/usr/share/php";

const MAX_CONSTANT_DEPTH: usize = 8;

fn short_name(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

fn class_constant_key(class: &str, constant: &str) -> String {
    format!("{}::{constant}", short_name(class).to_ascii_lowercase())
}

/// Initializers of the constants `program` declares - `const`, `define()`
/// with a literal name, and class/enum constants - keyed by short name
/// (`Class::NAME` lowercased on the class). A name declared twice is
/// dropped.
pub(crate) fn file_constant_initializers(program: &[Stmt]) -> FxHashMap<String, Expr> {
    let mut initializers = Initializers(FxHashMap::default());
    walk_stmts(&mut initializers, program);
    initializers
        .0
        .into_iter()
        .filter_map(|(k, v)| Some((k, v?)))
        .collect()
}

/// Each constant's initializer, `None` once a name is declared twice.
/// Function bodies are not entered, and a `define()` counts only as a
/// statement's own expression.
struct Initializers(FxHashMap<String, Option<Expr>>);

impl Initializers {
    fn add(&mut self, key: String, value: &Expr) {
        self.0
            .entry(key)
            .and_modify(|v| *v = None)
            .or_insert_with(|| Some(value.clone()));
    }
}

impl<'ast> Visitor<'ast> for Initializers {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.add(short_name(&c.name).to_string(), &c.value);
                }
            }
            StmtKind::ClassDecl(c) => {
                for k in &c.consts {
                    self.add(class_constant_key(class_decl_name(c), &k.name), &k.value);
                }
            }
            StmtKind::FunctionDecl(_) => {}
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        if let ExprKind::Call(callee, args) = &e.kind
            && callee_name(callee).is_some_and(|n| n.eq_ignore_ascii_case("define"))
            && let [name, value, ..] = args.as_slice()
            && let ExprKind::Literal(Literal::String(name)) = &name.value.kind
        {
            self.add(short_name(&name.to_str_lossy()).to_string(), &value.value);
        }
    }
}

/// `(construct as written, identifier prefix)` for an include kind.
pub(crate) fn include_construct(kind: IncludeKind) -> (&'static str, &'static str) {
    let prefix = match kind {
        IncludeKind::Require => "require",
        IncludeKind::RequireOnce => "requireOnce",
        IncludeKind::Include => "include",
        IncludeKind::IncludeOnce => "includeOnce",
    };
    (kind.as_str(), prefix)
}

fn callee_name(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Name(n) => Some(n.trim_start_matches('\\')),
        _ => None,
    }
}

/// A stream-wrapper URL (`phar://...`, `data://...`) - not a path on
/// this filesystem.
fn is_stream_wrapper_path(path: &str) -> bool {
    path.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

impl<'a> Checker<'a> {
    /// The checked file's own absolute path - what PHP's `__FILE__`
    /// holds, however the file was named on the command line or in the
    /// config's `paths` (a relative path's `dirname()` chain would
    /// otherwise walk off its top to `""`). The project supplies it; a
    /// checker run without one falls back to the path as named.
    fn current_file_absolute(&self) -> Option<PathBuf> {
        if let Some(file) = &self.current_file_absolute {
            return Some(file.clone());
        }
        let file = self.current_file.as_ref()?;
        Some(match &self.working_dir {
            Some(dir) => dir.join(file),
            None => file.to_path_buf(),
        })
    }

    fn current_dir_of_file(&self) -> Option<PathBuf> {
        self.current_file_absolute()
            .and_then(|f| f.parent().map(Path::to_path_buf))
    }

    /// Every string `e` can evaluate to, when all of them are known.
    fn resolve_include_paths(&self, e: &Expr, locals: &Locals) -> Option<Vec<String>> {
        self.resolve_paths_at(e, locals, 0)
    }

    fn file_constant_initializer(&self, e: &Expr) -> Option<&Expr> {
        let key = match &e.kind {
            ExprKind::Name(n) if !(n.starts_with("__") && n.ends_with("__")) => {
                short_name(n).to_string()
            }
            ExprKind::ClassConstFetch(target, constant) => {
                let class = target.as_name()?;
                let class = if ["self", "static"]
                    .iter()
                    .any(|k| class.eq_ignore_ascii_case(k))
                {
                    self.current_class.as_deref()?
                } else {
                    class
                };
                class_constant_key(class, constant)
            }
            _ => return None,
        };
        self.include_constant_initializers.get(&key)
    }

    fn resolve_paths_at(&self, e: &Expr, locals: &Locals, depth: usize) -> Option<Vec<String>> {
        syntax::stack::guard(|| {
            if let Some(init) = self.file_constant_initializer(e) {
                return (depth < MAX_CONSTANT_DEPTH)
                    .then(|| self.resolve_paths_at(init, &Locals::default(), depth + 1))
                    .flatten();
            }
            match &e.kind {
                ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => {
                    self.resolve_paths_at(inner, locals, depth)
                }
                // The platform's own separators - the paths are checked
                // against this machine's filesystem.
                ExprKind::Name(n) if n.trim_start_matches('\\') == "PATH_SEPARATOR" => {
                    Some(vec![":".to_string()])
                }
                ExprKind::Name(n) if n.trim_start_matches('\\') == "DIRECTORY_SEPARATOR" => {
                    Some(vec![std::path::MAIN_SEPARATOR.to_string()])
                }
                ExprKind::Call(callee, args)
                    if args.is_empty()
                        && callee_name(callee)
                            .is_some_and(|n| n.eq_ignore_ascii_case("get_include_path")) =>
                {
                    Some(vec![DEFAULT_INCLUDE_PATH.to_string()])
                }
                ExprKind::Name(n) if n.eq_ignore_ascii_case("__DIR__") => Some(vec![
                    self.current_dir_of_file()?.to_string_lossy().into_owned(),
                ]),
                ExprKind::Name(n) if n.eq_ignore_ascii_case("__FILE__") => Some(vec![
                    self.current_file_absolute()?.to_string_lossy().into_owned(),
                ]),
                ExprKind::Binary(BinOp::Concat, l, r) => {
                    let lefts = self.resolve_paths_at(l, locals, depth)?;
                    let rights = self.resolve_paths_at(r, locals, depth)?;
                    let mut out = Vec::new();
                    for left in &lefts {
                        for right in &rights {
                            out.push(format!("{left}{right}"));
                        }
                    }
                    Some(out)
                }
                ExprKind::Call(callee, args)
                    if callee_name(callee).is_some_and(|n| n.eq_ignore_ascii_case("dirname")) =>
                {
                    let [path_arg, rest @ ..] = args.as_slice() else {
                        return None;
                    };
                    let levels = match rest {
                        [] => 1,
                        [level] => match &level.value.kind {
                            ExprKind::Literal(Literal::Int(n)) if *n >= 1 => *n as usize,
                            _ => return None,
                        },
                        _ => return None,
                    };
                    let paths = self.resolve_paths_at(&path_arg.value, locals, depth)?;
                    Some(
                        paths
                            .into_iter()
                            .map(|p| {
                                let mut path = PathBuf::from(p);
                                for _ in 0..levels {
                                    if !path.pop() {
                                        break;
                                    }
                                }
                                path.to_string_lossy().into_owned()
                            })
                            .collect(),
                    )
                }
                _ => {
                    let strings: Vec<String> = self
                        .infer_expr(e, locals)
                        .constant_strings_text()
                        .into_iter()
                        .map(|s| s.into_owned())
                        .collect();
                    (!strings.is_empty()).then_some(strings)
                }
            }
        })
    }

    fn include_target_exists(&self, path: &str) -> bool {
        let path = Path::new(path);
        if path.is_absolute() {
            return path.is_file();
        }
        // Without a working directory, the relative path itself: the
        // filesystem resolves it against the process's.
        let from_cwd = Some(match &self.working_dir {
            Some(dir) => dir.join(path),
            None => path.to_path_buf(),
        });
        let from_file = self.current_dir_of_file().map(|d| d.join(path));
        from_cwd.is_some_and(|p| p.is_file())
            || from_file.is_some_and(|p| p.is_file())
            || self.include_path_dirs.iter().any(|dir| {
                let dir = Path::new(dir);
                let dir = if dir.is_absolute() {
                    dir.to_path_buf()
                } else {
                    match self.current_dir_of_file() {
                        Some(base) => base.join(dir),
                        None => dir.to_path_buf(),
                    }
                };
                dir.join(path).is_file()
            })
    }

    pub(crate) fn check_include_path(
        &mut self,
        (construct, identifier_prefix): (&'static str, &'static str),
        path_expr: &Expr,
        locals: &Locals,
        span: Span,
    ) {
        if self.current_file.is_none() {
            return;
        }
        let Some(paths) = self.resolve_include_paths(path_expr, locals) else {
            return;
        };
        for path in paths {
            if is_stream_wrapper_path(&path)
                || self.include_guarded_paths.contains(&path)
                || self.include_target_exists(&path)
            {
                continue;
            }
            self.report(
                format!("Path in {construct}() \"{path}\" is not a file or it does not exist."),
                span,
                0,
                format!("{identifier_prefix}.fileNotFound"),
            );
        }
    }

    /// Every path `program` tests with a `GUARD_FUNCTIONS` call, and the
    /// `include_path` directories: PHP's default plus every value the
    /// file sets it to.
    pub(crate) fn collect_include_guards(
        &self,
        program: &[Stmt],
    ) -> (FxHashSet<String>, Vec<String>) {
        let locals = Locals::default();
        let mut guarded = FxHashSet::default();
        let mut include_dirs: Vec<String> = DEFAULT_INCLUDE_PATH
            .split(':')
            .map(str::to_string)
            .collect();
        let mut on_call = |call: IncludeCall| match call {
            IncludeCall::Guard(arg) => {
                if let Some(paths) = self.resolve_include_paths(arg, &locals) {
                    guarded.extend(paths);
                }
            }
            IncludeCall::SetIncludePath(arg) => {
                for value in self
                    .resolve_include_paths(arg, &locals)
                    .into_iter()
                    .flatten()
                {
                    for dir in value.split(':').filter(|d| !d.is_empty()) {
                        if !include_dirs.iter().any(|d| d == dir) {
                            include_dirs.push(dir.to_string());
                        }
                    }
                }
            }
        };
        for stmt in program {
            visit_guard_args_in_stmt(stmt, &mut on_call);
        }
        (guarded, include_dirs)
    }
}

enum IncludeCall<'e> {
    Guard(&'e Expr),
    SetIncludePath(&'e Expr),
}

fn visit_guard_args_in_stmt(stmt: &Stmt, out: &mut dyn FnMut(IncludeCall)) {
    GuardArgs(out).visit_stmt(stmt);
}

/// Hands every statement expression, in function and method bodies too,
/// to `visit_guard_args_in_expr`.
struct GuardArgs<'o>(&'o mut dyn FnMut(IncludeCall));

impl<'ast> Visitor<'ast> for GuardArgs<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => walk_stmts(self, &f.body),
            StmtKind::ClassDecl(c) => {
                for m in &c.methods {
                    walk_stmts(self, &m.function.body);
                }
            }
            _ => walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        visit_guard_args_in_expr(e, self.0);
    }
}

fn visit_guard_args_in_expr(e: &Expr, out: &mut dyn FnMut(IncludeCall)) {
    syntax::stack::guard(|| {
        if let ExprKind::Call(callee, args) = &e.kind
            && let Some(name) = callee_name(callee)
        {
            let name = name.to_ascii_lowercase();
            match (name.as_str(), args.as_slice()) {
                (n, [arg, ..]) if GUARD_FUNCTIONS.contains(&n) => {
                    out(IncludeCall::Guard(&arg.value))
                }
                ("set_include_path", [arg]) => out(IncludeCall::SetIncludePath(&arg.value)),
                ("ini_set", [key, value]) if matches!(&key.value.kind, ExprKind::Literal(Literal::String(k)) if *k == "include_path") => {
                    out(IncludeCall::SetIncludePath(&value.value))
                }
                _ => {}
            }
        }
        if let ExprKind::Closure { body, .. } = &e.kind {
            body.iter().for_each(|s| visit_guard_args_in_stmt(s, out));
        }
        any_child_expr(e, &mut |child| {
            visit_guard_args_in_expr(child, out);
            false
        });
    })
}
