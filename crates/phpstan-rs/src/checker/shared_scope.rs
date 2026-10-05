//! What of a scope's variables code other than the scope's own may write or
//! run, found before the scope is checked: the variables bound to a global
//! (every one, at a file's top level), to the declaration's static
//! variables, or by reference to a place; the variables a closure the scope
//! writes captures by reference; the variables holding only closures the
//! scope wrote, with what their bodies do; and the variables holding only
//! objects the scope made that never leave it, whose properties no other
//! code can reach.

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::name::Name;
use syntax::span::Span;
use syntax::visit::{Visitor, walk_expr, walk_stmt};

use crate::registry::summaries::{BodySummary, Place, Step};

/// See the module's doc comment.
#[derive(Default)]
pub(crate) struct SharedScope {
    /// The scope is a file's top level, where every variable is a global.
    pub(crate) top_level: bool,
    /// The variables `global` binds, each to the global of its name.
    pub(crate) globals: FxHashSet<Name>,
    /// A `global` statement binds a variable whose name is computed.
    pub(crate) any_global: bool,
    /// The variables `static` binds.
    pub(crate) statics: FxHashSet<Name>,
    /// The declaration the static variables belong to: its class, when a
    /// method, and its name.
    pub(crate) function: Option<(Option<Name>, Name)>,
    /// The variables bound by reference, each to the place it refers to.
    pub(crate) references: FxHashMap<Name, Place>,
    /// The variables a closure captures by reference: the scope's own
    /// captures, for a closure's scope, and those of the closures it writes.
    pub(crate) captured: FxHashSet<Name>,
    /// The variables holding only closures, arrow functions or first-class
    /// callables the scope wrote, each one summarized
    /// (`BodySummary::of_closure`).
    pub(crate) closures: FxHashMap<Name, Vec<BodySummary>>,
    /// The variables holding only objects the scope made, used only as the
    /// base of a property access, an operand of `===`, `!==`, `instanceof`,
    /// `isset` or `empty`, or an assignment's target: no other code can
    /// reach those objects.
    pub(crate) unescaped: FxHashSet<Name>,
    /// Where each variable is assigned, by offset, and the loops of the
    /// scope, by span.
    pub(crate) writes: FxHashMap<Name, Vec<u32>>,
    pub(crate) loops: Vec<Span>,
}

impl SharedScope {
    /// The shared scope of the statements `body`, a function's, a closure's
    /// (whose `use` list is `uses`) or a file's top level.
    pub(crate) fn of(
        body: &[Stmt],
        uses: &[ClosureUse],
        function: Option<(Option<Name>, Name)>,
        top_level: bool,
    ) -> SharedScope {
        let mut scan = Scan::default();
        for u in uses {
            if u.by_ref {
                scan.captured.insert(u.name.name.clone());
                scan.held.insert(u.name.name.clone(), Held::Other);
            }
        }
        for stmt in body {
            scan.visit_stmt(stmt);
        }
        scan.finish(function, top_level)
    }

    /// The shared scope of an arrow function's body, which binds nothing.
    pub(crate) fn of_arrow_function(body: &Expr) -> SharedScope {
        let mut scan = Scan::default();
        scan.visit_expr(body);
        scan.finish(None, false)
    }

    /// Whether the scope may assign `name` after the code at `at` runs: an
    /// assignment after it, or one in a loop around it.
    pub(crate) fn written_after(&self, name: &Name, at: Span) -> bool {
        self.writes.get(name).is_some_and(|offsets| {
            offsets.iter().any(|&offset| {
                offset >= at.end
                    || self.loops.iter().any(|l| {
                        l.start <= at.start
                            && at.end <= l.end
                            && l.start <= offset
                            && offset < l.end
                    })
            })
        })
    }

    /// Whether no variable of the scope is shared.
    pub(crate) fn binds_nothing(&self) -> bool {
        !self.top_level
            && self.globals.is_empty()
            && !self.any_global
            && self.statics.is_empty()
            && self.references.is_empty()
            && self.captured.is_empty()
    }
}

/// What a variable is written.
enum Held {
    /// Only objects the scope made.
    Made,
    /// Only closures the scope wrote, each one summarized.
    Closures(Vec<BodySummary>),
    /// Anything else.
    Other,
}

#[derive(Default)]
struct Scan {
    writes: FxHashMap<Name, Vec<u32>>,
    loops: Vec<Span>,
    globals: FxHashSet<Name>,
    any_global: bool,
    statics: FxHashSet<Name>,
    references: FxHashMap<Name, Place>,
    captured: FxHashSet<Name>,
    held: FxHashMap<Name, Held>,
    escaped: FxHashSet<Name>,
}

impl Scan {
    fn finish(self, function: Option<(Option<Name>, Name)>, top_level: bool) -> SharedScope {
        let bound = |name: &Name| {
            self.globals.contains(name)
                || self.statics.contains(name)
                || self.references.contains_key(name)
                || self.captured.contains(name)
        };
        let mut unescaped = FxHashSet::default();
        let mut closures = FxHashMap::default();
        for (name, held) in self.held {
            if bound(&name) || top_level {
                continue;
            }
            match held {
                Held::Made if !self.escaped.contains(&name) => {
                    unescaped.insert(name);
                }
                Held::Closures(bodies) => {
                    closures.insert(name, bodies);
                }
                Held::Made | Held::Other => {}
            }
        }
        SharedScope {
            top_level,
            globals: self.globals,
            any_global: self.any_global,
            statics: self.statics,
            function,
            references: self.references,
            captured: self.captured,
            closures,
            unescaped,
            writes: self.writes,
            loops: self.loops,
        }
    }

    /// That `name` is written what `held` says.
    fn note(&mut self, name: &Name, held: Held) {
        let joined = match (self.held.remove(name), held) {
            (None, held) => held,
            (Some(Held::Made), Held::Made) => Held::Made,
            (Some(Held::Closures(mut a)), Held::Closures(b)) => {
                a.extend(b);
                Held::Closures(a)
            }
            _ => Held::Other,
        };
        self.held.insert(name.clone(), joined);
    }

    /// A use of `e` that does not let the object it is leave the scope:
    /// the base of a property access, an identity or type test.
    fn kept(&mut self, e: &Expr) {
        if !matches!(&e.peeled().kind, ExprKind::Variable(_)) {
            self.visit_expr(e);
        }
    }

    /// An assignment to `target` of what `held` says.
    fn assign(&mut self, target: &Expr, held: Held) {
        match &target.peeled().kind {
            ExprKind::Variable(name) => {
                self.writes
                    .entry(name.clone())
                    .or_default()
                    .push(target.span.start);
                self.note(name, held);
            }
            ExprKind::Destructure(items, _) => {
                for item in items {
                    if let Some(key) = &item.key {
                        self.visit_expr(key);
                    }
                    if let Some(target) = &item.target {
                        self.assign(target, Held::Other);
                    }
                }
            }
            ExprKind::PropertyAccess(base, _, _) => self.kept(base),
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.kept(base);
                self.visit_expr(name);
            }
            _ => self.visit_expr(target),
        }
    }
}

/// What the scope made or wrote that `e` evaluates to.
fn held_by(e: &Expr) -> Held {
    let e = e.peeled();
    syntax::stack::guard(|| match &e.kind {
        ExprKind::New(..) | ExprKind::NewAnonymousClass(_) | ExprKind::Clone(..) => Held::Made,
        ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
            Held::Closures(vec![BodySummary::of_closure(e)])
        }
        ExprKind::Assign(_, rhs) => held_by(rhs),
        _ => Held::Other,
    })
}

impl<'ast> Visitor<'ast> for Scan {
    // A declaration nested in the scope is a scope of its own.
    fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}

    fn visit_class_def(&mut self, _: &'ast ClassDef) {}

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if matches!(
            stmt.kind,
            StmtKind::While { .. }
                | StmtKind::DoWhile { .. }
                | StmtKind::For { .. }
                | StmtKind::Foreach { .. }
        ) {
            self.loops.push(stmt.span);
        }
        syntax::stack::guard(|| match &stmt.kind {
            StmtKind::Global(vars) => {
                for var in vars {
                    match var {
                        GlobalVar::Name(name) => {
                            self.globals.insert(name.name.clone());
                            self.note(&name.name, Held::Other);
                        }
                        GlobalVar::Dynamic(e) => {
                            self.any_global = true;
                            self.visit_expr(e);
                        }
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for (name, default) in vars {
                    self.statics.insert(name.name.clone());
                    self.note(&name.name, Held::Other);
                    if let Some(default) = default {
                        self.visit_expr(default);
                    }
                }
            }
            StmtKind::Unset(targets) => {
                for target in targets {
                    self.kept(target);
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                by_ref,
                body,
            } => {
                self.visit_expr(subject);
                if let Some(key) = key {
                    self.assign(key, Held::Other);
                }
                match &value.peeled().kind {
                    ExprKind::Variable(name) if *by_ref => {
                        let place = Place::of(subject)
                            .unwrap_or_else(Place::value)
                            .then(Step::Element);
                        self.references.insert(name.clone(), place);
                        self.writes
                            .entry(name.clone())
                            .or_default()
                            .push(value.span.start);
                        self.note(name, Held::Other);
                    }
                    _ => self.assign(value, Held::Other),
                }
                self.visit_stmt(body);
            }
            _ => walk_stmt(self, stmt),
        })
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Variable(name) => {
                self.escaped.insert(name.clone());
            }
            ExprKind::PropertyAccess(base, _, _) => self.kept(base),
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.kept(base);
                self.visit_expr(name);
            }
            ExprKind::Assign(lhs, rhs) => {
                self.assign(lhs, held_by(rhs));
                self.visit_expr(rhs);
            }
            ExprKind::AssignRef(lhs, rhs) => {
                match &lhs.peeled().kind {
                    ExprKind::Variable(name) => {
                        let place = Place::of(rhs).unwrap_or_else(Place::value);
                        self.references.insert(name.clone(), place);
                        self.writes
                            .entry(name.clone())
                            .or_default()
                            .push(lhs.span.start);
                        self.note(name, Held::Other);
                    }
                    _ => self.assign(lhs, Held::Other),
                }
                self.visit_expr(rhs);
            }
            ExprKind::CompoundAssign(_, lhs, rhs) => {
                self.assign(lhs, Held::Other);
                self.visit_expr(lhs);
                self.visit_expr(rhs);
            }
            ExprKind::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                target,
            ) => {
                self.assign(target, Held::Other);
                self.visit_expr(target);
            }
            ExprKind::Binary(BinOp::Identical | BinOp::NotIdentical, a, b) => {
                self.kept(a);
                self.kept(b);
            }
            ExprKind::Instanceof(subject, target) => {
                self.kept(subject);
                if let InstanceofTarget::Expr(target) = target {
                    self.visit_expr(target);
                }
            }
            ExprKind::Isset(items) => {
                for item in items {
                    self.kept(item);
                }
            }
            ExprKind::Empty(item) => self.kept(item),
            // A closure's body is a scope of its own; what it captures leaves
            // this one.
            ExprKind::Closure(closure) => {
                for u in &closure.uses {
                    self.escaped.insert(u.name.name.clone());
                    if u.by_ref {
                        self.captured.insert(u.name.name.clone());
                    }
                }
            }
            _ => walk_expr(self, e),
        })
    }
}
