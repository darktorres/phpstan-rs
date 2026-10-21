//! The globals a call may write. A function declared in the checked file
//! writes a global through a `global` declaration in its body, and through the
//! functions it calls by name, so a call to it may change a global the caller
//! holds, which the caller's own writes do not show.

use super::Checker;
use super::Locals;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::visit::{self, Visitor};
use types::Type;

/// The name a write to a global whose name is computed stands for: `global
/// ${expr};`, or any access to `$GLOBALS`. Any global may be the one written.
const ANY_GLOBAL: &str = "*";

/// The globals each function of the file may write, keyed by its lowercased
/// name with no leading backslash.
#[derive(Default)]
pub(crate) struct GlobalWrites {
    by_function: FxHashMap<String, FxHashSet<String>>,
}

impl GlobalWrites {
    /// The globals the function `name` may write, or `None` when it is not a
    /// function of this file.
    pub(crate) fn of(&self, name: &str) -> Option<&FxHashSet<String>> {
        self.by_function.get(&key(name))
    }
}

/// Which globals a body's variables are. At the top level every variable is
/// a global; in a named function a variable is one only when the body imports
/// it with `global`; a closure's variables are its own (not modelled here).
#[derive(Default)]
pub(crate) enum Scope {
    #[default]
    Other,
    Top,
    Function(FxHashSet<String>),
}

fn key(name: &str) -> String {
    name.trim_start_matches('\\').to_ascii_lowercase()
}

/// The write set of every function of `program`: its own `global` names and
/// the functions it calls, closed over those calls.
pub(crate) fn global_writes(program: &[Stmt]) -> GlobalWrites {
    let mut collect = Collect::default();
    for stmt in program {
        collect.visit_stmt(stmt);
    }
    let Collect {
        mut writes, calls, ..
    } = collect;
    loop {
        let mut changed = false;
        for (function, callees) in &calls {
            for callee in callees {
                let Some(written) = writes.get(callee).cloned() else {
                    continue;
                };
                let into = writes.entry(function.clone()).or_default();
                for global in written {
                    changed |= into.insert(global);
                }
            }
        }
        if !changed {
            break;
        }
    }
    GlobalWrites {
        by_function: writes,
    }
}

/// The globals `stmts` import with `global`, not counting any nested function,
/// closure or class.
pub(crate) fn imported_globals(stmts: &[Stmt]) -> FxHashSet<String> {
    let mut collect = Collect {
        current: Some(String::new()),
        ..Collect::default()
    };
    for stmt in stmts {
        collect.visit_stmt(stmt);
    }
    collect.writes.remove("").unwrap_or_default()
}

#[derive(Default)]
struct Collect {
    // The function whose body is being walked, lowercased; `None` outside
    // every function.
    current: Option<String>,
    writes: FxHashMap<String, FxHashSet<String>>,
    calls: FxHashMap<String, FxHashSet<String>>,
}

impl Collect {
    fn write(&mut self, global: &str) {
        if let Some(current) = &self.current {
            self.writes
                .entry(current.clone())
                .or_default()
                .insert(global.to_string());
        }
    }
}

impl<'ast> Visitor<'ast> for Collect {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::FunctionDecl(f) => {
                let outer = self.current.take();
                let name = key(f.namespaced_name.as_deref().unwrap_or(f.name.name.as_str()));
                self.writes.entry(name.clone()).or_default();
                self.calls.entry(name.clone()).or_default();
                self.current = Some(name);
                for stmt in f.body_stmts() {
                    self.visit_stmt(stmt);
                }
                self.current = outer;
            }
            // A class's methods are not followed: a call of one is not a
            // call of a function of this file, and no write is attributed.
            StmtKind::ClassDecl(_) => {}
            StmtKind::Global(vars) => {
                for var in vars {
                    match var {
                        GlobalVar::Name(ident) => self.write(ident.name.as_str()),
                        GlobalVar::Dynamic(_) => self.write(ANY_GLOBAL),
                    }
                }
                visit::walk_stmt(self, stmt);
            }
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            // A closure's body runs under its own scope.
            ExprKind::Closure(_) | ExprKind::ArrowFn(_) => {}
            ExprKind::Variable(name) if name.as_str() == "GLOBALS" => {
                self.write(ANY_GLOBAL);
            }
            ExprKind::Call(callee, _) => {
                if let (Some(current), ExprKind::Name(name)) = (&self.current, &callee.kind) {
                    self.calls
                        .entry(current.clone())
                        .or_default()
                        .insert(key(name));
                }
                visit::walk_expr(self, expr);
            }
            _ => visit::walk_expr(self, expr),
        }
    }
}

impl Checker<'_> {
    /// Mixes each variable of the caller that a call of `callee` may write:
    /// at the top level, a variable the callee writes through `global`; in a
    /// named function, one the body imports and the callee writes.
    pub(crate) fn apply_global_writes(&self, callee: &str, locals: &mut Locals) {
        let Some(written) = self.global_writes.of(callee) else {
            return;
        };
        let any = written.contains(ANY_GLOBAL);
        let visible: Vec<String> = match &self.global_scope {
            Scope::Other => return,
            Scope::Top => locals
                .variables()
                .into_iter()
                .map(|(name, _)| name.to_string())
                .filter(|name| any || written.contains(name))
                .collect(),
            Scope::Function(imports) => imports
                .iter()
                .filter(|name| any || written.contains(*name))
                .cloned()
                .collect(),
        };
        for name in visible {
            if locals.contains_var(&name) {
                locals.insert_var(&name, Type::mixed());
            }
        }
    }
}
