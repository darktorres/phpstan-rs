//! The strings the operand of an include and the argument of
//! `set_include_path()` and `chdir()` can take, in each world they are read
//! in (`include_lookup::World`). Paths are resolved from string literals,
//! constant string types, `__DIR__`/`__FILE__`, `dirname()`, the world's
//! `DIRECTORY_SEPARATOR` and `PATH_SEPARATOR`, `get_include_path()`, and
//! concatenations of these. A constant declared in the same file resolves
//! through its initializer, so `__DIR__` in it means this file's directory.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use crate::checker::include_lookup::{self, GET_INCLUDE_PATH, LookupChanges, World, WorldValues};
use crate::checker::{Checker, Locals};
use syntax::ast::*;
use syntax::visit::{Visitor, walk_expr, walk_stmt, walk_stmts};

/// The functions whose call tests a path, which guards an include of it. A
/// file that names none of them has no guard to find (`mentions::GUARD`).
pub(crate) const GUARD_FUNCTIONS: [&str; 6] = [
    "file_exists",
    "is_file",
    "is_readable",
    "is_writable",
    "is_writeable",
    "is_executable",
];

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
                for k in &c.def.consts {
                    self.add(class_constant_key(&c.name, &k.name), &k.value);
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

/// The function a call's callee names, as an unqualified name in a
/// namespace falls back to the global one.
fn callee_name(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Name(n) => {
            let (primary, fallback) = n.candidates();
            Some(fallback.unwrap_or(primary))
        }
        _ => None,
    }
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
        Some(match &self.root {
            Some(dir) => dir.join(file),
            None => file.to_path_buf(),
        })
    }

    pub(crate) fn current_dir_of_file(&self) -> Option<PathBuf> {
        self.current_file_absolute()
            .and_then(|f| f.parent().map(Path::to_path_buf))
    }

    /// `path`, a place of the project, as `world` spells it.
    fn place_text(&self, path: &Path, world: World) -> Option<String> {
        let root = self.root.as_deref().map(Path::to_string_lossy);
        include_lookup::spell(
            world,
            World::machine(),
            root.as_deref(),
            &path.to_string_lossy(),
        )
    }

    /// Every string `e` can evaluate to in `world`, when all of them are
    /// known.
    pub(crate) fn resolve_include_paths(
        &self,
        e: &Expr,
        locals: &Locals,
        world: World,
    ) -> Option<Vec<String>> {
        self.resolve_paths_at(e, locals, 0, world)
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

    fn resolve_paths_at(
        &self,
        e: &Expr,
        locals: &Locals,
        depth: usize,
        world: World,
    ) -> Option<Vec<String>> {
        stack::guard(|| {
            if let Some(init) = self.file_constant_initializer(e) {
                return (depth < MAX_CONSTANT_DEPTH)
                    .then(|| self.resolve_paths_at(init, &self.new_locals(), depth + 1, world))
                    .flatten();
            }
            let e = e.peeled();
            match &e.kind {
                // The world's own separators.
                ExprKind::Name(n) if n.trim_start_matches('\\') == "PATH_SEPARATOR" => {
                    Some(vec![world.path_separator().to_string()])
                }
                ExprKind::Name(n) if n.trim_start_matches('\\') == "DIRECTORY_SEPARATOR" => {
                    Some(vec![world.directory_separator().to_string()])
                }
                // The include path is known once the whole project is read.
                ExprKind::Call(callee, args)
                    if args.is_empty()
                        && callee_name(callee)
                            .is_some_and(|n| n.eq_ignore_ascii_case("get_include_path")) =>
                {
                    Some(vec![GET_INCLUDE_PATH.to_string()])
                }
                ExprKind::Name(n) if n.eq_ignore_ascii_case("__DIR__") => {
                    Some(vec![self.place_text(&self.current_dir_of_file()?, world)?])
                }
                ExprKind::Name(n) if n.eq_ignore_ascii_case("__FILE__") => Some(vec![
                    self.place_text(&self.current_file_absolute()?, world)?,
                ]),
                ExprKind::Binary(BinOp::Concat, l, r) => {
                    let lefts = self.resolve_paths_at(l, locals, depth, world)?;
                    let rights = self.resolve_paths_at(r, locals, depth, world)?;
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
                    let paths = self.resolve_paths_at(&path_arg.value, locals, depth, world)?;
                    Some(
                        paths
                            .iter()
                            .map(|path| world.dirname(path, levels))
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

    /// Every value the include operand `e` can take, in each world, when
    /// all of them are known.
    pub(crate) fn include_values(&self, e: &Expr, locals: &Locals) -> Option<[Vec<String>; 2]> {
        let unix = self.resolve_include_paths(e, locals, World::Unix)?;
        let windows = self.resolve_include_paths(e, locals, World::Windows)?;
        // The include path is a list of entries, not a path.
        let names_the_include_path = unix
            .iter()
            .chain(&windows)
            .any(|value| value.contains(GET_INCLUDE_PATH));
        (!names_the_include_path).then_some([unix, windows])
    }

    /// What `program` does to where includes are found: the values it sets
    /// the include path to with `set_include_path()` or
    /// `ini_set('include_path', ...)`, and the directories it moves the
    /// working directory to with `chdir()`.
    pub(crate) fn lookup_changes(&mut self, program: &[Stmt]) -> LookupChanges {
        self.include_constant_initializers = file_constant_initializers(program);
        let locals = self.new_locals();
        let mut changes = LookupChanges::default();
        let mut on_call = |call: IncludeCall| {
            let (arg, into) = match call {
                IncludeCall::SetIncludePath(arg) => (arg, &mut changes.include_path),
                IncludeCall::Chdir(arg) => (arg, &mut changes.chdir),
                IncludeCall::Guard(_) => return,
            };
            into.push(WorldValues {
                by_world: World::ALL.map(|world| self.resolve_include_paths(arg, &locals, world)),
            });
        };
        walk_stmts(&mut IncludeCalls(&mut on_call), program);
        changes
    }
}

pub(crate) enum IncludeCall<'e> {
    Guard(&'e Expr),
    SetIncludePath(&'e Expr),
    Chdir(&'e Expr),
}

/// Hands every `GUARD_FUNCTIONS`, `set_include_path()`,
/// `ini_set('include_path', ...)` and `chdir()` argument anywhere in the
/// program to its callback.
pub(crate) struct IncludeCalls<'o>(pub(crate) &'o mut dyn FnMut(IncludeCall));

impl<'ast> Visitor<'ast> for IncludeCalls<'_> {
    fn visit_expr(&mut self, e: &'ast Expr) {
        stack::guard(|| {
            if let ExprKind::Call(callee, args) = &e.kind
                && let Some(name) = callee_name(callee)
            {
                let is = |wanted: &str| name.eq_ignore_ascii_case(wanted);
                match args.as_slice() {
                    [arg, ..] if GUARD_FUNCTIONS.iter().any(|f| is(f)) => {
                        (self.0)(IncludeCall::Guard(&arg.value))
                    }
                    [arg] if is("set_include_path") => {
                        (self.0)(IncludeCall::SetIncludePath(&arg.value))
                    }
                    [arg] if is("chdir") => (self.0)(IncludeCall::Chdir(&arg.value)),
                    [key, value]
                        if (is("ini_set") || is("ini_alter"))
                            && matches!(&key.value.kind, ExprKind::Literal(Literal::String(k)) if *k == "include_path") =>
                    {
                        (self.0)(IncludeCall::SetIncludePath(&value.value))
                    }
                    _ => {}
                }
            }
            walk_expr(self, e);
        })
    }
}
