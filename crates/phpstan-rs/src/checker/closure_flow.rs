//! Closure literals whose every call the checker can see.
//!
//! A closure literal's body is checked at the state its by-reference
//! captures could have at any point - the union of every write those
//! captures can see - because the checker cannot tell where the closure is
//! called. That union is sound, but it reports states the closure never
//! runs in when the call site is known. A literal whose value stays in
//! plain local variables, and in local arrays that are only ever indexed
//! to call, is called only where the checker can see it: each call is then
//! checked at the state it actually runs in.
//!
//! The decision is syntactic, per scope (the top level, a function, a
//! method, a closure or an arrow function). A variable is followed only
//! when it has one write, which assigns the literal, and each of its reads
//! is a call `$v(...)`, or an element of an array literal that is itself
//! followed the same way and read only as `$holder[...](...)`. Any other
//! read - passing it, returning it, capturing it, referencing it, a
//! `global` naming it - escapes it, and a scope that calls `compact`,
//! `extract`, `eval`, `include`, or uses a variable variable is not
//! followed at all.

use std::rc::Rc;

use diagnostics::Diagnostic;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{
    ArrowFnExpr, CatchClause, ClosureExpr, ClosureUse, Expr, ExprKind, FunctionDecl, GlobalVar,
    Literal, MethodDecl, Param, Stmt, StmtKind,
};
use syntax::span::Span;
use syntax::visit::{self, Visitor};
use types::{ClosureLiteralId, Type};

use crate::checker::{Checker, Locals};
use crate::file_id::FileId;

/// The identity of the closure literal starting at `span` in `file`.
pub(crate) fn closure_literal_id_in(file: Option<FileId>, span: Span) -> ClosureLiteralId {
    let file = file.map_or(0, |file| file.number());
    ClosureLiteralId((u64::from(file) << 32) | u64::from(span.start))
}

/// How a closure literal's body is checked.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClosureMode {
    /// Not followed: checked as if it could be called anywhere, its
    /// by-reference captures' effects applied where it is written.
    Plain,
    /// Followed: checked where it is written, for its type. Its
    /// diagnostics are held until a call of it is checked, or the file ends
    /// with none having been, and it has no effect on the scope it is
    /// written in.
    Creation,
    /// Followed: checked at one of its calls, against the state that call
    /// sees, with its effects applied there.
    Call,
}

/// What a followed literal's by-value capture held where the literal was
/// written: whether the variable was defined and what it held.
#[derive(Clone)]
pub(crate) struct CapturedVariable {
    pub(crate) ty: Option<Type>,
    pub(crate) maybe_undefined: bool,
}

/// A closure literal the checker follows to its calls: where it starts, and
/// a copy of it to check at each call site.
pub(crate) struct TrackedClosure {
    pub(crate) span: Span,
    pub(crate) closure: ClosureExpr,
}

/// The followed literals of a program, by identity.
pub(crate) type TrackedClosures = FxHashMap<ClosureLiteralId, Rc<TrackedClosure>>;

/// Every followed closure literal in `program`, each keyed by `id_of`'s
/// identity for its span.
pub(crate) fn tracked_closures(
    program: &[Stmt],
    id_of: &dyn Fn(Span) -> ClosureLiteralId,
) -> TrackedClosures {
    let globals = global_scope_facts(program);
    let mut found = FxHashSet::default();
    let mut top = Scope::default();
    top.dynamic |= globals.uses_globals_array;
    for name in &globals.imported {
        top.read(name, Read::Escape);
    }
    let mut flow = ScopeFlow::new(&mut top);
    for stmt in program {
        flow.visit_stmt(stmt);
    }
    found.extend(top.followed_literals());
    let mut discover = Discover { found: &mut found };
    for stmt in program {
        discover.visit_stmt(stmt);
    }
    let mut collect = Collect {
        found: &found,
        id_of,
        out: TrackedClosures::default(),
    };
    for stmt in program {
        collect.visit_stmt(stmt);
    }
    collect.out
}

/// How one occurrence of a variable is used.
#[derive(Clone, PartialEq, Eq)]
enum Read {
    /// `$v(...)`.
    Callee,
    /// `$v[<literal>](...)`, `$v` holding an array.
    CalleeElement,
    /// `$v` as an element of the array literal assigned to `holder`.
    Element(String),
    /// Any other use: the value is passed on or can be reached elsewhere.
    Escape,
}

/// How a variable is written.
#[derive(Clone)]
enum Write {
    /// `$v = function ... {}`: the literal, by its span.
    Closure(Span),
    /// `$v = [ ... ]`, with the elements that can be followed.
    Array(Vec<Element>),
    /// Any other write: a parameter, a capture, `$v = $w`, `foreach`, ...
    Other,
}

/// One element of an array literal assigned to a variable.
#[derive(Clone)]
enum Element {
    /// A closure literal written in place.
    Closure(Span),
    /// A variable, read as an element: its own reads decide whether it is
    /// followed.
    Variable,
    /// Anything else.
    Other,
}

/// What one scope does with its variables.
#[derive(Default)]
struct Scope {
    writes: FxHashMap<String, Vec<Write>>,
    reads: FxHashMap<String, Vec<Read>>,
    /// The scope calls something that makes variables by name, or reads
    /// them through a name it computes: nothing in it is followed.
    dynamic: bool,
}

impl Scope {
    fn write(&mut self, name: &str, write: Write) {
        self.writes.entry(name.to_string()).or_default().push(write);
    }

    fn read(&mut self, name: &str, read: Read) {
        self.reads.entry(name.to_string()).or_default().push(read);
    }

    /// The literals this scope follows, once its variables are all seen.
    fn followed_literals(&self) -> Vec<Span> {
        if self.dynamic {
            return Vec::new();
        }
        let mut found = Vec::new();
        for (name, writes) in &self.writes {
            let [write] = writes.as_slice() else {
                continue;
            };
            match write {
                Write::Closure(span) if self.variable_followed(name) => found.push(*span),
                Write::Array(elements) if self.holder_followed(name) => {
                    for element in elements {
                        if let Element::Closure(span) = element {
                            found.push(*span);
                        }
                    }
                }
                _ => {}
            }
        }
        found
    }

    /// Whether every read of the variable `name` is a call, or an element
    /// of an array that is itself followed.
    fn variable_followed(&self, name: &str) -> bool {
        self.reads.get(name).is_none_or(|reads| {
            reads.iter().all(|read| match read {
                Read::Callee => true,
                Read::Element(holder) => self.holder_followed(holder),
                Read::CalleeElement | Read::Escape => false,
            })
        })
    }

    /// Whether `name` is one array literal, read only as `name[<literal>]
    /// (...)`.
    fn holder_followed(&self, name: &str) -> bool {
        let written_as_array = matches!(
            self.writes.get(name).map(Vec::as_slice),
            Some([Write::Array(_)])
        );
        written_as_array
            && self
                .reads
                .get(name)
                .is_none_or(|reads| reads.iter().all(|read| *read == Read::CalleeElement))
    }
}

/// The facts about the top level that a `global` statement anywhere in the
/// program can change.
#[derive(Default)]
struct GlobalFacts {
    imported: FxHashSet<String>,
    uses_globals_array: bool,
}

fn global_scope_facts(program: &[Stmt]) -> GlobalFacts {
    let mut facts = GlobalFacts::default();
    let mut finder = GlobalFinder { facts: &mut facts };
    for stmt in program {
        finder.visit_stmt(stmt);
    }
    facts
}

struct GlobalFinder<'f> {
    facts: &'f mut GlobalFacts,
}

impl<'ast> Visitor<'ast> for GlobalFinder<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let StmtKind::Global(vars) = &stmt.kind {
            for var in vars {
                if let GlobalVar::Name(ident) = var {
                    self.facts.imported.insert(ident.name.as_str().to_string());
                }
            }
        }
        visit::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if matches!(&expr.kind, ExprKind::Variable(name) if name.as_str() == "GLOBALS") {
            self.facts.uses_globals_array = true;
        }
        visit::walk_expr(self, expr);
    }
}

/// One scope's occurrences, walked without entering the scopes nested in
/// it: a closure's or arrow function's body is its own scope, and a named
/// function or method is too.
struct ScopeFlow<'s> {
    scope: &'s mut Scope,
    /// Inside an arrow function's body, which captures every variable it
    /// names by value: nothing there is followed.
    inside_arrow: u32,
}

impl<'s> ScopeFlow<'s> {
    fn new(scope: &'s mut Scope) -> ScopeFlow<'s> {
        ScopeFlow {
            scope,
            inside_arrow: 0,
        }
    }

    /// The closure's `use (...)` names escape the scope: the closure body
    /// reads them at its own calls.
    fn escape_uses(&mut self, uses: &[ClosureUse]) {
        for closure_use in uses {
            self.scope
                .read(closure_use.name.name.as_str(), Read::Escape);
        }
    }
}

impl<'ast> Visitor<'ast> for ScopeFlow<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Global(vars) => {
                for var in vars {
                    match var {
                        GlobalVar::Name(ident) => {
                            self.scope.read(ident.name.as_str(), Read::Escape);
                        }
                        GlobalVar::Dynamic(name) => {
                            self.scope.dynamic = true;
                            self.visit_expr(name);
                        }
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for (ident, init) in vars {
                    self.scope.read(ident.name.as_str(), Read::Escape);
                    if let Some(init) = init {
                        self.visit_expr(init);
                    }
                }
            }
            _ => visit::walk_stmt(self, stmt),
        }
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        if let Some(var) = &clause.var {
            self.scope.write(var.name.as_str(), Write::Other);
        }
        visit::walk_catch_clause(self, clause);
    }

    fn visit_function_decl(&mut self, _function: &'ast FunctionDecl) {}

    fn visit_class_decl(&mut self, _class: &'ast syntax::ast::ClassDecl) {}

    fn visit_method_decl(&mut self, _method: &'ast MethodDecl) {}

    fn visit_param(&mut self, _param: &'ast Param) {}

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Variable(name) => {
                self.scope.read(name.as_str(), Read::Escape);
            }
            ExprKind::DynamicVariable(name) => {
                self.scope.dynamic = true;
                self.visit_expr(name);
            }
            ExprKind::Eval(_) | ExprKind::Include(..) => {
                self.scope.dynamic = true;
                visit::walk_expr(self, expr);
            }
            ExprKind::Call(callee, args) => {
                if let ExprKind::Name(name) = &callee.kind
                    && is_dynamic_function(name.written.as_str())
                {
                    self.scope.dynamic = true;
                }
                let plain = self.inside_arrow == 0;
                match &callee.kind {
                    ExprKind::Variable(name) if plain => {
                        self.scope.read(name.as_str(), Read::Callee);
                    }
                    ExprKind::Index(base, Some(index)) if plain && is_literal(index) => {
                        match &base.kind {
                            ExprKind::Variable(name) => {
                                self.scope.read(name.as_str(), Read::CalleeElement);
                            }
                            _ => self.visit_expr(callee),
                        }
                    }
                    _ => self.visit_expr(callee),
                }
                visit::walk_args(self, args);
            }
            ExprKind::Assign(lhs, rhs) => match &lhs.kind {
                ExprKind::Variable(name) => {
                    let name = name.as_str().to_string();
                    let write = self.assigned_value(&name, rhs);
                    self.scope.write(&name, write);
                }
                _ => {
                    self.visit_expr(lhs);
                    self.visit_expr(rhs);
                }
            },
            ExprKind::Closure(closure) => {
                self.escape_uses(&closure.uses);
            }
            ExprKind::ArrowFn(arrow) => {
                self.visit_arrow(arrow);
            }
            ExprKind::NewAnonymousClass(anonymous) => {
                visit::walk_args(self, &anonymous.args);
            }
            _ => visit::walk_expr(self, expr),
        }
    }
}

impl ScopeFlow<'_> {
    /// The write an assignment of `rhs` to `name` makes. An array literal's
    /// elements that are variables or literals are followed only inside
    /// the array; anything else in them is walked as ordinary reads.
    fn assigned_value(&mut self, name: &str, rhs: &Expr) -> Write {
        match &rhs.kind {
            ExprKind::Closure(closure) => {
                self.escape_uses(&closure.uses);
                Write::Closure(rhs.span)
            }
            ExprKind::ArrayLiteral(items) if self.inside_arrow == 0 => {
                let mut elements = Vec::new();
                for item in items {
                    if let Some(key) = &item.key {
                        self.visit_expr(key);
                    }
                    let followed = !item.spread && !item.by_ref;
                    match &item.value.kind {
                        ExprKind::Closure(closure) if followed => {
                            self.escape_uses(&closure.uses);
                            elements.push(Element::Closure(item.value.span));
                        }
                        ExprKind::Variable(variable) if followed => {
                            self.scope
                                .read(variable.as_str(), Read::Element(name.to_string()));
                            elements.push(Element::Variable);
                        }
                        _ => {
                            self.visit_expr(&item.value);
                            elements.push(Element::Other);
                        }
                    }
                }
                Write::Array(elements)
            }
            _ => {
                self.visit_expr(rhs);
                Write::Other
            }
        }
    }

    fn visit_arrow(&mut self, arrow: &ArrowFnExpr) {
        for param in &arrow.params {
            self.scope.write(param.name.name.as_str(), Write::Other);
        }
        self.inside_arrow += 1;
        self.visit_expr(&arrow.body);
        self.inside_arrow -= 1;
    }
}

fn is_literal(e: &Expr) -> bool {
    matches!(
        e.peeled().kind,
        ExprKind::Literal(Literal::Int(_) | Literal::String(_))
    )
}

/// Functions that make or read variables by a name they compute.
fn is_dynamic_function(name: &str) -> bool {
    let name = name.trim_start_matches('\\');
    ["compact", "extract", "get_defined_vars"]
        .iter()
        .any(|dynamic| name.eq_ignore_ascii_case(dynamic))
}

/// Finds every scope in the program and collects what it follows.
struct Discover<'f> {
    found: &'f mut FxHashSet<Span>,
}

impl<'ast> Visitor<'ast> for Discover<'_> {
    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        self.analyze_params_and_body(&function.params, &[], function.body_stmts());
        visit::walk_function_decl(self, function);
    }

    fn visit_method_decl(&mut self, method: &'ast MethodDecl) {
        let function = &method.function;
        self.analyze_params_and_body(&function.params, &[], function.body_stmts());
        visit::walk_method_decl(self, method);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::Closure(closure) => {
                self.analyze_params_and_body(&closure.params, &closure.uses, &closure.body);
                visit::walk_stmts(self, &closure.body);
            }
            ExprKind::ArrowFn(arrow) => {
                let mut scope = Scope::default();
                for param in &arrow.params {
                    scope.write(param.name.name.as_str(), Write::Other);
                }
                let mut flow = ScopeFlow::new(&mut scope);
                flow.visit_expr(&arrow.body);
                self.found.extend(scope.followed_literals());
                visit::walk_expr(self, &arrow.body);
            }
            _ => visit::walk_expr(self, expr),
        }
    }
}

impl Discover<'_> {
    fn analyze_params_and_body(&mut self, params: &[Param], uses: &[ClosureUse], body: &[Stmt]) {
        let mut scope = Scope::default();
        for param in params {
            scope.write(param.name.name.as_str(), Write::Other);
        }
        for closure_use in uses {
            scope.write(closure_use.name.name.as_str(), Write::Other);
        }
        let mut flow = ScopeFlow::new(&mut scope);
        for stmt in body {
            flow.visit_stmt(stmt);
        }
        self.found.extend(scope.followed_literals());
    }
}

/// Copies each followed literal into the table, keyed by its identity.
struct Collect<'c> {
    found: &'c FxHashSet<Span>,
    id_of: &'c dyn Fn(Span) -> ClosureLiteralId,
    out: TrackedClosures,
}

impl<'ast> Visitor<'ast> for Collect<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let ExprKind::Closure(closure) = &expr.kind
            && self.found.contains(&expr.span)
        {
            let id = (self.id_of)(expr.span);
            self.out.insert(
                id,
                Rc::new(TrackedClosure {
                    span: expr.span,
                    closure: (**closure).clone(),
                }),
            );
        }
        visit::walk_expr(self, expr);
    }
}

impl Checker<'_> {
    /// Checks each followed literal `callee` may be, at this call.
    pub(crate) fn check_tracked_closure_calls(
        &mut self,
        callee: &Type,
        locals: &mut Locals,
        label: &str,
    ) {
        for id in closure_literal_identities(callee) {
            let Some(tracked) = self.tracked_closures.get(&id).cloned() else {
                continue;
            };
            self.called_closures.insert(id);
            let before = self.diagnostics.len();
            self.check_closure_literal(
                tracked.span,
                &tracked.closure,
                locals,
                label,
                ClosureMode::Call,
            );
            self.drop_repeated_diagnostics(before);
        }
    }

    /// What the creation of the followed literal `id` recorded for its
    /// by-value capture `name`.
    pub(crate) fn captured_at_creation(
        &self,
        id: ClosureLiteralId,
        name: &types::Name,
    ) -> Option<CapturedVariable> {
        self.closure_capture_snapshots
            .get(&id)?
            .iter()
            .find(|(captured, _)| captured == name)
            .map(|(_, captured)| captured.clone())
    }

    /// Holds back the diagnostics a followed literal's creation check made
    /// since `mark`.
    pub(crate) fn hold_closure_diagnostics(&mut self, id: ClosureLiteralId, mark: usize) {
        let held = self.diagnostics.split_off(mark);
        self.pending_closure_diagnostics.insert(id, held);
    }

    /// Releases the held diagnostics of each followed literal no call was
    /// checked at, then forgets this file's followed literals.
    pub(crate) fn flush_pending_closure_diagnostics(&mut self) {
        let mut pending: Vec<(ClosureLiteralId, Vec<Diagnostic>)> =
            self.pending_closure_diagnostics.drain().collect();
        pending.sort_by_key(|(id, _)| id.0);
        for (id, held) in pending {
            if !self.called_closures.contains(&id) {
                self.diagnostics.extend(held);
            }
        }
        self.called_closures.clear();
        self.closure_capture_snapshots.clear();
        self.tracked_closures.clear();
    }

    /// Drops what the diagnostics from `from` on repeat an earlier one: a
    /// literal called at two sites reports its body's problems at each.
    fn drop_repeated_diagnostics(&mut self, from: usize) {
        let mut index = from;
        while index < self.diagnostics.len() {
            let repeated = self.diagnostics[..from].contains(&self.diagnostics[index])
                || self.diagnostics[from..index].contains(&self.diagnostics[index]);
            if repeated {
                self.diagnostics.remove(index);
            } else {
                index += 1;
            }
        }
    }
}

/// The followed literals a value of type `ty` can be: the closure literal
/// identity of each member of a union, or of `ty` itself.
fn closure_literal_identities(ty: &Type) -> Vec<ClosureLiteralId> {
    ty.union_members()
        .iter()
        .filter_map(|member| member.callable_sig()?.literal)
        .collect()
}
