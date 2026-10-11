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
use syntax::ast::*;
use syntax::span::Span;
use syntax::visit::arena::{self, Visitor};

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
pub(crate) fn file_constant_initializers(
    arenas: &NodeArenas,
    program: &[StmtId],
) -> FxHashMap<String, ExprId> {
    let mut initializers = Initializers(FxHashMap::default());
    arena::walk_stmts(&mut initializers, arenas, program);
    initializers
        .0
        .into_iter()
        .filter_map(|(k, v)| Some((k, v?)))
        .collect()
}

/// Each constant's initializer, `None` once a name is declared twice.
/// Function bodies are not entered, and a `define()` counts only as a
/// statement's own expression.
struct Initializers(FxHashMap<String, Option<ExprId>>);

impl Initializers {
    fn add(&mut self, key: String, value: ExprId) {
        self.0
            .entry(key)
            .and_modify(|v| *v = None)
            .or_insert(Some(value));
    }
}

impl Visitor for Initializers {
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}

    fn visit_class_decl(&mut self, _: &NodeArenas, _: &ClassDecl) {}

    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        match &stmt.kind {
            StmtKind::GlobalConst(consts) => {
                for c in arenas.class_consts(consts) {
                    self.add(short_name(&c.name).to_string(), c.value);
                }
            }
            StmtKind::ClassDecl(c) => {
                let c = arenas.class_decl(c.clone());
                if let Some(def) = arenas.class(c.def) {
                    for k in def.consts(arenas) {
                        self.add(class_constant_key(&c.name, &k.name), k.value);
                    }
                }
            }
            StmtKind::FunctionDecl(_) => {}
            _ => arena::walk_stmt(self, arenas, id),
        }
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        if matches!(&e.kind, ExprKind::Closure(_) | ExprKind::ArrowFn(_)) {
            return;
        }
        if let ExprKind::Call(callee, args) = &e.kind
            && callee_name(arenas, *callee).is_some_and(|n| n.eq_ignore_ascii_case("define"))
            && let [name_arg, value, ..] = arenas.args(args.clone())
            && let Some(ExprNode {
                kind: ExprKind::Literal(Literal::String(name)),
                ..
            }) = arenas.expr(name_arg.value)
        {
            let Some(name) = arenas.string_bytes(name) else {
                return;
            };
            let name = String::from_utf8_lossy(name);
            self.add(short_name(&name).to_string(), value.value);
        }
        arena::walk_expr(self, arenas, id);
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

/// The function a call's callee names, as an unqualified name in a
/// namespace falls back to the global one.
fn callee_name(arenas: &NodeArenas, id: ExprId) -> Option<&str> {
    match &arenas.expr(id)?.kind {
        ExprKind::Name(n) => {
            let n = arenas.qualified_name(n)?;
            let (primary, fallback) = n.candidates();
            Some(fallback.unwrap_or(primary))
        }
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
    fn resolve_include_paths(&self, id: ExprId, locals: &Locals) -> Option<Vec<String>> {
        let arenas = self
            .syntax_arenas()
            .expect("include path resolution requires the syntax tree owner");
        self.resolve_paths_at(arenas, id, locals, 0)
    }

    fn file_constant_initializer(&self, arenas: &NodeArenas, id: ExprId) -> Option<ExprId> {
        let e = arenas.expr(id)?;
        let key = match &e.kind {
            ExprKind::Name(n) => {
                let n = arenas.qualified_name(n)?;
                if n.starts_with("__") && n.ends_with("__") {
                    return None;
                }
                short_name(n).to_string()
            }
            ExprKind::ClassConstFetch(target, constant) => {
                let class = target.as_name()?.written.as_ref();
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
        self.include_constant_initializers.get(&key).copied()
    }

    fn resolve_paths_at(
        &self,
        arenas: &NodeArenas,
        id: ExprId,
        locals: &Locals,
        depth: usize,
    ) -> Option<Vec<String>> {
        syntax::stack::guard(|| {
            if let Some(init) = self.file_constant_initializer(arenas, id) {
                return (depth < MAX_CONSTANT_DEPTH)
                    .then(|| self.resolve_paths_at(arenas, init, &self.new_locals(), depth + 1))
                    .flatten();
            }
            let id = arenas.peeled(id)?;
            let e = arenas.expr(id)?;
            match &e.kind {
                // The platform's own separators - the paths are checked
                // against this machine's filesystem.
                ExprKind::Name(n)
                    if arenas
                        .qualified_name(n)
                        .is_some_and(|n| n.trim_start_matches('\\') == "PATH_SEPARATOR") =>
                {
                    Some(vec![":".to_string()])
                }
                ExprKind::Name(n)
                    if arenas
                        .qualified_name(n)
                        .is_some_and(|n| n.trim_start_matches('\\') == "DIRECTORY_SEPARATOR") =>
                {
                    Some(vec![std::path::MAIN_SEPARATOR.to_string()])
                }
                ExprKind::Call(callee, args)
                    if args.is_empty()
                        && callee_name(arenas, *callee)
                            .is_some_and(|n| n.eq_ignore_ascii_case("get_include_path")) =>
                {
                    Some(vec![DEFAULT_INCLUDE_PATH.to_string()])
                }
                ExprKind::Name(n)
                    if arenas
                        .qualified_name(n)
                        .is_some_and(|n| n.eq_ignore_ascii_case("__DIR__")) =>
                {
                    Some(vec![
                        self.current_dir_of_file()?.to_string_lossy().into_owned(),
                    ])
                }
                ExprKind::Name(n)
                    if arenas
                        .qualified_name(n)
                        .is_some_and(|n| n.eq_ignore_ascii_case("__FILE__")) =>
                {
                    Some(vec![
                        self.current_file_absolute()?.to_string_lossy().into_owned(),
                    ])
                }
                ExprKind::Binary(BinOp::Concat, l, r) => {
                    let lefts = self.resolve_paths_at(arenas, *l, locals, depth)?;
                    let rights = self.resolve_paths_at(arenas, *r, locals, depth)?;
                    let mut out = Vec::new();
                    for left in &lefts {
                        for right in &rights {
                            out.push(format!("{left}{right}"));
                        }
                    }
                    Some(out)
                }
                ExprKind::Call(callee, args)
                    if callee_name(arenas, *callee)
                        .is_some_and(|n| n.eq_ignore_ascii_case("dirname")) =>
                {
                    let [path_arg, rest @ ..] = arenas.args(args.clone()) else {
                        return None;
                    };
                    let levels = match rest {
                        [] => 1,
                        [level] => match arenas.expr(level.value).map(|node| &node.kind) {
                            Some(ExprKind::Literal(Literal::Int(n))) if *n >= 1 => *n as usize,
                            _ => return None,
                        },
                        _ => return None,
                    };
                    let paths = self.resolve_paths_at(arenas, path_arg.value, locals, depth)?;
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

    /// Where a file `path` names can be, in the order looked at: the path
    /// itself when it is absolute; else from the working directory, from
    /// the including file's directory, and from each `include_path`
    /// directory, a relative one taken from the including file's.
    fn include_candidates(&self, path: &str) -> Vec<PathBuf> {
        let path = Path::new(path);
        if path.is_absolute() {
            return vec![path.to_path_buf()];
        }
        let file_dir = self.current_dir_of_file();
        // Without a working directory, the relative path itself: the
        // filesystem resolves it against the process's.
        let mut candidates = vec![match &self.working_dir {
            Some(dir) => dir.join(path),
            None => path.to_path_buf(),
        }];
        candidates.extend(file_dir.as_ref().map(|dir| dir.join(path)));
        for dir in &self.include_path_dirs {
            let dir = Path::new(dir);
            let dir = match &file_dir {
                Some(base) if !dir.is_absolute() => base.join(dir),
                _ => dir.to_path_buf(),
            };
            candidates.push(dir.join(path));
        }
        candidates
    }

    /// Whether a file is at one of the places `path` names, each place
    /// looked at, up to the first holding one, recorded as probed.
    fn include_target_exists(&mut self, path: &str) -> bool {
        for candidate in self.include_candidates(path) {
            let found = candidate.is_file();
            self.include_probes.push(candidate);
            if found {
                return true;
            }
        }
        false
    }

    pub(crate) fn check_include_path(
        &mut self,
        (construct, identifier_prefix): (&'static str, &'static str),
        path_expr: ExprId,
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
            report!(
                self,
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
        arenas: &NodeArenas,
        program: &[StmtId],
    ) -> (FxHashSet<String>, Vec<String>) {
        let locals = self.new_locals();
        let mut guarded = FxHashSet::default();
        let mut include_dirs = default_include_dirs();
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
        arena::walk_stmts(&mut GuardArgs(&mut on_call), arenas, program);
        (guarded, include_dirs)
    }
}

/// The `include_path` directories before the file sets any.
pub(crate) fn default_include_dirs() -> Vec<String> {
    DEFAULT_INCLUDE_PATH
        .split(':')
        .map(str::to_string)
        .collect()
}

enum IncludeCall {
    Guard(ExprId),
    SetIncludePath(ExprId),
}

/// Hands every `GUARD_FUNCTIONS`, `set_include_path()` and
/// `ini_set('include_path', ...)` argument anywhere in the program to its
/// callback.
struct GuardArgs<'o>(&'o mut dyn FnMut(IncludeCall));

impl Visitor for GuardArgs<'_> {
    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        syntax::stack::guard(|| {
            if let ExprKind::Call(callee, args) = &e.kind
                && let Some(name) = callee_name(arenas, *callee)
            {
                let is = |wanted: &str| name.eq_ignore_ascii_case(wanted);
                match arenas.args(args.clone()) {
                    [arg, ..] if GUARD_FUNCTIONS.iter().any(|f| is(f)) => {
                        (self.0)(IncludeCall::Guard(arg.value))
                    }
                    [arg] if is("set_include_path") => {
                        (self.0)(IncludeCall::SetIncludePath(arg.value))
                    }
                    [key, value]
                        if is("ini_set")
                            && arenas.expr(key.value).is_some_and(|node| {
                                matches!(&node.kind, ExprKind::Literal(Literal::String(k))
                                    if arenas.string_bytes(k).is_some_and(|bytes| bytes == b"include_path"))
                            }) =>
                    {
                        (self.0)(IncludeCall::SetIncludePath(value.value))
                    }
                    _ => {}
                }
            }
            arena::walk_expr(self, arenas, id);
        })
    }
}
