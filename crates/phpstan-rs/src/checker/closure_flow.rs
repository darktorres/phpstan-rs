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
//! `extract`, `eval`, `include`, or uses a variable variable it cannot name
//! is not followed at all. A variable variable `$$n(...)` whose `$n` is
//! written only as one string literal is a call of that variable, as
//! `$GLOBALS['f'](...)` is a call of the global `f`.

use std::rc::Rc;

use diagnostics::Diagnostic;
use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::{
    Arg, ArrowFnExpr, CatchClause, ClosureExpr, ClosureUse, Expr, ExprKind, FunctionDecl,
    GlobalVar, Ident, Literal, MethodDecl, ObjectOperator, Param, Stmt, StmtKind,
};
use syntax::span::{Range, Span};
use syntax::visit::{self, Visitor};
use types::{ClosureLiteralId, Type};

use super::global_writes::key;
use crate::checker::invocation::Invocations;
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

/// What a call of a followed literal passes into its body: the types of its
/// arguments when they are all positional, and the by-reference names its
/// nested calls read through the literal's captures.
#[derive(Default)]
pub(crate) struct CallInput<'a> {
    pub(crate) arguments: Option<&'a [Type]>,
    pub(crate) captures: &'a [String],
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
    /// The by-reference names a call reads through the literal's captures: its
    /// own, and those of each literal a captured holder may hold, since a call
    /// of that literal can run inside this one.
    pub(crate) captures: Vec<String>,
}

/// The followed literals of a program, by identity.
pub(crate) type TrackedClosures = FxHashMap<ClosureLiteralId, Rc<TrackedClosure>>;

/// Every followed closure literal in `program`, each keyed by `id_of`'s
/// identity for its span.
pub(crate) fn tracked_closures(
    program: &[Stmt],
    invocations: &Rc<Invocations>,
    id_of: &dyn Fn(Span) -> ClosureLiteralId,
) -> TrackedClosures {
    let globals = global_scope_facts(program);
    let captured = captured_bodies(program, invocations);
    let mut found: FxHashMap<Span, Vec<String>> = FxHashMap::default();
    let mut top = Scope::default();
    top.dynamic |= globals.uses_globals_array;
    for name in &globals.imported {
        if !invocations.must_invoke.covers_global(name) {
            top.read(name, Read::Escape);
        }
    }
    for name in &globals.called {
        top.read(name, Read::Callee);
    }
    let mut flow = ScopeFlow::new(&mut top, Rc::clone(invocations));
    for stmt in program {
        flow.visit_stmt(stmt);
    }
    found.extend(top.followed_literals(invocations, &captured));
    let mut discover = Discover {
        found: &mut found,
        invocations: Rc::clone(invocations),
        captured: &captured,
    };
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
    /// `($v->p)(...)`, `$v` holding a plain object (`Write::Object`).
    PropertyCallee,
    /// `$v` as an element of the array literal assigned to `holder`.
    Element(String),
    /// `$v` as a positional argument of a same-file function whose parameter
    /// its live code never invokes: no call reaches the value through it.
    Passed,
    /// `$v` as a positional argument of a same-file function whose parameter
    /// every path of its body invokes, or a global such a function invokes on
    /// every call: the value runs during that call, which checks it there
    /// (see `check_must_invoked_calls`).
    Invoked,
    /// `$v` copied into another variable (`$w = $v`, or a branch of a ternary
    /// assigned to one): the value reaches that variable, whose own reads
    /// decide whether it is followed.
    Copied,
    /// `$v` captured by reference by the closure literal with this span: its
    /// body reads `$v` at each call of the literal, so the variable is followed
    /// only while the literal is, and its body only calls `$v` (see
    /// `CapturedBodies`).
    Captured(Span),
    /// Any other use: the value is passed on or can be reached elsewhere.
    Escape,
}

/// Whether each by-reference capture of a closure literal is only called in
/// the literal's body: keyed by the literal's span and the captured name, the
/// capture is settled when the body writes it nowhere, reads it only as a
/// callee, and has no dynamic name.
type CapturedBodies = FxHashMap<(Span, String), bool>;

/// How a variable is written.
#[derive(Clone)]
enum Write {
    /// `$v = function ... {}`: the literal, by its span.
    Closure(Span),
    /// `$v = [ ... ]`, with the elements that can be followed.
    Array(Vec<Element>),
    /// `$v = new C()`, with `C` (its `global_writes::key`) a class whose
    /// instances are reached only through the variables they are built into.
    Object(String),
    /// `$v = 'name'`: a string literal, a name a variable variable can call.
    Name(String),
    /// `$v = $w`, or `$v = a ? b : c` with each branch a source.
    Sources(Vec<Source>),
    /// Any other write: a parameter, a capture, `foreach`, ...
    Other,
}

/// Where one branch of a value assigned to a variable comes from.
#[derive(Clone)]
enum Source {
    /// A closure literal, by its span.
    Closure(Span),
    /// Another variable, copied.
    Variable(String),
    /// Anything else.
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
    /// Each closure literal written to a property: the variable holding the
    /// object, the property, and the literal's span.
    property_closures: Vec<(String, String, Span)>,
    /// The variables `$$n(...)` calls name through: each is resolved by
    /// `resolve_named_callees`.
    named_callees: Vec<String>,
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

    /// Resolves each variable variable call `$$n(...)`: when `$n` is written
    /// only as one string literal `'f'` and no read of it escapes, the call
    /// is a read of `$f`; any other one makes the scope dynamic.
    fn resolve_named_callees(&mut self) {
        for name in std::mem::take(&mut self.named_callees) {
            let escapes = self
                .reads
                .get(&name)
                .is_some_and(|reads| reads.contains(&Read::Escape));
            let target = match self.writes.get(&name).map(Vec::as_slice) {
                Some([Write::Name(first), rest @ ..]) => rest
                    .iter()
                    .all(|write| matches!(write, Write::Name(other) if other == first))
                    .then(|| first.clone()),
                _ => None,
            };
            match target {
                Some(target) if !escapes => self.read(&target, Read::Callee),
                _ => self.dynamic = true,
            }
        }
    }

    /// The literals this scope follows, once its variables are all seen: a
    /// literal is followed when every variable that may hold it is.
    fn followed_literals(
        &mut self,
        invocations: &Invocations,
        captured: &CapturedBodies,
    ) -> FxHashMap<Span, Vec<String>> {
        self.resolve_named_callees();
        if self.dynamic {
            return FxHashMap::default();
        }
        let mut found = Vec::new();
        for (name, writes) in &self.writes {
            if let [Write::Array(elements)] = writes.as_slice()
                && self.holder_followed(name)
            {
                for element in elements {
                    if let Element::Closure(span) = element {
                        found.push(*span);
                    }
                }
            }
        }
        for (holder, property, span) in &self.property_closures {
            if let Some([Write::Object(class)]) = self.writes.get(holder).map(Vec::as_slice)
                && self.object_holder_settles(holder)
                && invocations.plain_object_property(class, property)
            {
                found.push(*span);
            }
        }
        // The literals each variable may hold, and the copies between variables.
        let mut holds: FxHashMap<&str, FxHashSet<Span>> = FxHashMap::default();
        let mut copies: Vec<(&str, &str)> = Vec::new();
        for (name, writes) in &self.writes {
            for write in writes {
                match write {
                    Write::Closure(span) => {
                        holds.entry(name).or_default().insert(*span);
                    }
                    Write::Sources(sources) => {
                        for source in sources {
                            match source {
                                Source::Closure(span) => {
                                    holds.entry(name).or_default().insert(*span);
                                }
                                Source::Variable(from) => copies.push((name, from)),
                                Source::Other => {}
                            }
                        }
                    }
                    Write::Array(_) | Write::Object(_) | Write::Name(_) | Write::Other => {}
                }
            }
        }
        loop {
            let mut changed = false;
            for (target, from) in &copies {
                let reaching: Vec<Span> = holds
                    .get(from)
                    .map(|spans| spans.iter().copied().collect())
                    .unwrap_or_default();
                for span in reaching {
                    changed |= holds.entry(target).or_default().insert(span);
                }
            }
            if !changed {
                break;
            }
        }
        // A literal is followed when every holder of it settles. A holder
        // read by a capture settles only while the capturing literal is
        // followed, so the set shrinks until no literal is dropped.
        let mut followed: FxHashSet<Span> = holds.values().flatten().copied().collect();
        loop {
            let unsettled: Vec<Span> = followed
                .iter()
                .copied()
                .filter(|span| {
                    !holds
                        .iter()
                        .filter(|(_, spans)| spans.contains(span))
                        .all(|(name, _)| self.holder_settles(name, &followed, captured))
                })
                .collect();
            if unsettled.is_empty() {
                break;
            }
            for span in unsettled {
                followed.remove(&span);
            }
        }
        found.extend(followed);
        found
            .into_iter()
            .map(|span| {
                let mut names = Vec::new();
                transitive_captures(
                    span,
                    captured,
                    &holds,
                    &mut names,
                    &mut FxHashSet::default(),
                );
                names.sort();
                (span, names)
            })
            .collect()
    }

    /// Whether `name` may hold a followed literal without any of its reads
    /// escaping: its reads are all calls or copies, and it is never written
    /// as an array.
    fn holder_settles(
        &self,
        name: &str,
        followed: &FxHashSet<Span>,
        captured: &CapturedBodies,
    ) -> bool {
        self.variable_followed(name, followed, captured)
            && !self
                .writes
                .get(name)
                .is_some_and(|writes| writes.iter().any(|write| matches!(write, Write::Array(_))))
    }

    /// Whether every read of the variable `name` is a call, a copy, or an
    /// element of an array that is itself followed, or a by-reference capture
    /// whose literal is followed and only calls the variable.
    fn variable_followed(
        &self,
        name: &str,
        followed: &FxHashSet<Span>,
        captured: &CapturedBodies,
    ) -> bool {
        self.reads.get(name).is_none_or(|reads| {
            reads.iter().all(|read| match read {
                Read::Callee | Read::Passed | Read::Invoked | Read::Copied => true,
                Read::Element(holder) => self.holder_followed(holder),
                Read::Captured(literal) => {
                    followed.contains(literal)
                        && captured
                            .get(&(*literal, name.to_string()))
                            .copied()
                            .unwrap_or(false)
                }
                Read::CalleeElement | Read::PropertyCallee | Read::Escape => false,
            })
        })
    }

    /// Whether `name` is one plain object, read only as `($name->p)(...)`.
    fn object_holder_settles(&self, name: &str) -> bool {
        self.reads
            .get(name)
            .is_none_or(|reads| reads.iter().all(|read| *read == Read::PropertyCallee))
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
    /// The keys of the `$GLOBALS['f'](...)` calls, which call the global `f`.
    called: FxHashSet<String>,
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
        if let ExprKind::Call(callee, args) = &expr.kind
            && let Some(key) = globals_array_key(callee)
        {
            self.facts.called.insert(key);
            visit::walk_args(self, args);
            return;
        }
        if matches!(&expr.kind, ExprKind::Variable(name) if name.as_str() == "GLOBALS") {
            self.facts.uses_globals_array = true;
        }
        visit::walk_expr(self, expr);
    }
}

/// The literal key of a `$GLOBALS['key']` element, or `None` for any other
/// expression.
fn globals_array_key(expr: &Expr) -> Option<String> {
    let ExprKind::Index(base, Some(index)) = &expr.peeled().kind else {
        return None;
    };
    let ExprKind::Variable(name) = &base.kind else {
        return None;
    };
    let ExprKind::Literal(Literal::String(key)) = &index.kind else {
        return None;
    };
    (name.as_str() == "GLOBALS")
        .then(|| key.as_str().map(str::to_string))
        .flatten()
}

/// One scope's occurrences, walked without entering the scopes nested in
/// it: a closure's or arrow function's body is its own scope, and a named
/// function or method is too.
struct ScopeFlow<'s> {
    scope: &'s mut Scope,
    /// Inside an arrow function's body, which captures every variable it
    /// names by value: nothing there is followed.
    inside_arrow: u32,
    invocations: Rc<Invocations>,
}

impl<'s> ScopeFlow<'s> {
    fn new(scope: &'s mut Scope, invocations: Rc<Invocations>) -> ScopeFlow<'s> {
        ScopeFlow {
            scope,
            inside_arrow: 0,
            invocations,
        }
    }

    /// The closure literal's `use (...)` names: a by-value name escapes the
    /// scope, since the body reads its own copy at its calls. A by-reference
    /// name is read by the body at each call of the literal `literal`, so it
    /// is followed only while that literal is (see `Read::Captured`).
    fn escape_uses(&mut self, uses: &[ClosureUse], literal: Span) {
        for closure_use in uses {
            let read = if closure_use.by_ref {
                Read::Captured(literal)
            } else {
                Read::Escape
            };
            self.scope.read(closure_use.name.name.as_str(), read);
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
                match &callee.peeled().kind {
                    ExprKind::Variable(name) if plain => {
                        self.scope.read(name.as_str(), Read::Callee);
                    }
                    ExprKind::DynamicVariable(name)
                        if plain && matches!(name.peeled().kind, ExprKind::Variable(_)) =>
                    {
                        if let ExprKind::Variable(variable) = &name.peeled().kind {
                            self.scope.named_callees.push(variable.as_str().to_string());
                        }
                    }
                    ExprKind::PropertyAccess(base, _, ObjectOperator::Arrow)
                        if plain && matches!(base.kind, ExprKind::Variable(_)) =>
                    {
                        if let ExprKind::Variable(holder) = &base.kind {
                            self.scope.read(holder.as_str(), Read::PropertyCallee);
                        }
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
                let uninvoked = self.uninvoked_parameters(callee, args).filter(|_| plain);
                let invoked = self.invoked_parameters(callee, args).filter(|_| plain);
                if plain && let ExprKind::Name(name) = &callee.kind {
                    for global in self.invocations.must_invoke.globals(name.written.as_str()) {
                        self.scope.read(global, Read::Invoked);
                    }
                }
                for (index, arg) in args.iter().enumerate() {
                    match &arg.value.kind {
                        ExprKind::Variable(name)
                            if !arg.spread
                                && arg.name.is_none()
                                && uninvoked
                                    .as_ref()
                                    .is_some_and(|u| u.get(index) == Some(&true)) =>
                        {
                            self.scope.read(name.as_str(), Read::Passed);
                        }
                        ExprKind::Variable(name)
                            if !arg.spread
                                && arg.name.is_none()
                                && invoked
                                    .as_ref()
                                    .is_some_and(|i| i.get(index) == Some(&true)) =>
                        {
                            self.scope.read(name.as_str(), Read::Invoked);
                        }
                        _ => self.visit_arg(arg),
                    }
                }
            }
            ExprKind::Assign(lhs, rhs) => match &lhs.kind {
                ExprKind::Variable(name) => {
                    let name = name.as_str().to_string();
                    let write = self.assigned_value(&name, rhs);
                    self.scope.write(&name, write);
                }
                ExprKind::PropertyAccess(base, property, ObjectOperator::Arrow)
                    if self.inside_arrow == 0 && matches!(base.kind, ExprKind::Variable(_)) =>
                {
                    self.property_write(base, property, rhs);
                }
                _ => {
                    self.visit_expr(lhs);
                    self.visit_expr(rhs);
                }
            },
            ExprKind::Closure(closure) => {
                self.escape_uses(&closure.uses, expr.span);
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
    /// The class `rhs` builds an instance of with no arguments, when that
    /// class's instances are reached only through the variable they are built
    /// into.
    fn plain_new(&self, rhs: &Expr) -> Option<String> {
        let ExprKind::New(class, args) = &rhs.kind else {
            return None;
        };
        let ExprKind::Name(name) = &class.kind else {
            return None;
        };
        let declared = name.resolved_class()?;
        (args.is_empty() && self.inside_arrow == 0 && self.invocations.plain_object_class(declared))
            .then(|| key(declared))
    }

    /// A write of `rhs` to a property of the variable `holder`. A closure
    /// literal written there is held by the property; the variable is not
    /// read by the write.
    fn property_write(&mut self, holder: &Expr, property: &Ident, rhs: &Expr) {
        let ExprKind::Variable(holder) = &holder.kind else {
            return;
        };
        match &rhs.kind {
            ExprKind::Closure(closure) => {
                self.escape_uses(&closure.uses, rhs.span);
                self.scope.property_closures.push((
                    holder.as_str().to_string(),
                    property.name.as_str().to_string(),
                    rhs.span,
                ));
            }
            _ => self.visit_expr(rhs),
        }
    }

    /// The write an assignment of `rhs` to `name` makes. An array literal's
    /// elements that are variables or literals are followed only inside
    /// the array; anything else in them is walked as ordinary reads.
    fn assigned_value(&mut self, name: &str, rhs: &Expr) -> Write {
        if let Some(class) = self.plain_new(rhs) {
            return Write::Object(class);
        }
        match &rhs.kind {
            ExprKind::Closure(closure) => {
                self.escape_uses(&closure.uses, rhs.span);
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
                            self.escape_uses(&closure.uses, item.value.span);
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
            ExprKind::Variable(_) | ExprKind::Ternary(..) if self.inside_arrow == 0 => {
                let branches: Vec<&Expr> = match &rhs.kind {
                    ExprKind::Ternary(cond, then, otherwise) => {
                        self.visit_expr(cond);
                        then.as_deref()
                            .into_iter()
                            .chain(std::iter::once(&**otherwise))
                            .collect()
                    }
                    _ => vec![rhs],
                };
                Write::Sources(
                    branches
                        .into_iter()
                        .map(|branch| self.source_of(branch))
                        .collect(),
                )
            }
            ExprKind::Literal(Literal::String(value)) => match value.as_str() {
                Some(name) => Write::Name(name.to_string()),
                None => Write::Other,
            },
            _ => {
                self.visit_expr(rhs);
                Write::Other
            }
        }
    }

    /// Where a branch of a value assigned to a variable comes from.
    fn source_of(&mut self, branch: &Expr) -> Source {
        match &branch.kind {
            ExprKind::Closure(closure) => {
                self.escape_uses(&closure.uses, branch.span);
                Source::Closure(branch.span)
            }
            ExprKind::Variable(variable) => {
                self.scope.read(variable.as_str(), Read::Copied);
                Source::Variable(variable.as_str().to_string())
            }
            _ => {
                self.visit_expr(branch);
                Source::Other
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
pub(crate) fn is_dynamic_function(name: &str) -> bool {
    let name = name.trim_start_matches('\\');
    ["compact", "extract", "get_defined_vars"]
        .iter()
        .any(|dynamic| name.eq_ignore_ascii_case(dynamic))
}

/// Finds every scope in the program and collects what it follows.
struct Discover<'f> {
    found: &'f mut FxHashMap<Span, Vec<String>>,
    invocations: Rc<Invocations>,
    captured: &'f CapturedBodies,
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
                let mut flow = ScopeFlow::new(&mut scope, Rc::clone(&self.invocations));
                flow.visit_expr(&arrow.body);
                self.found
                    .extend(scope.followed_literals(&self.invocations, self.captured));
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
        let mut flow = ScopeFlow::new(&mut scope, Rc::clone(&self.invocations));
        for stmt in body {
            flow.visit_stmt(stmt);
        }
        self.found
            .extend(scope.followed_literals(&self.invocations, self.captured));
    }
}

/// The by-reference captures of every closure literal in `program`, each
/// settled when its literal's body only calls the captured variable.
fn captured_bodies(program: &[Stmt], invocations: &Rc<Invocations>) -> CapturedBodies {
    let mut out = CapturedBodies::default();
    let mut scan = CaptureScan {
        out: &mut out,
        invocations: Rc::clone(invocations),
    };
    for stmt in program {
        scan.visit_stmt(stmt);
    }
    out
}

/// Appends the by-reference names a call of the literal `literal` reads
/// through its captures: its own, and those of each literal a captured holder
/// may hold, from `holds`.
fn transitive_captures(
    literal: Span,
    captured: &CapturedBodies,
    holds: &FxHashMap<&str, FxHashSet<Span>>,
    names: &mut Vec<String>,
    visited: &mut FxHashSet<Span>,
) {
    if !visited.insert(literal) {
        return;
    }
    for (span, name) in captured.keys() {
        if *span != literal {
            continue;
        }
        if !names.contains(name) {
            names.push(name.clone());
        }
        for held in holds.get(name.as_str()).into_iter().flatten() {
            transitive_captures(*held, captured, holds, names, visited);
        }
    }
}

/// Records each by-reference capture's settlement, scanning the body of
/// each closure literal on its own.
struct CaptureScan<'c> {
    out: &'c mut CapturedBodies,
    invocations: Rc<Invocations>,
}

impl<'ast> Visitor<'ast> for CaptureScan<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let ExprKind::Closure(closure) = &expr.kind {
            let mut scope = Scope::default();
            let mut flow = ScopeFlow::new(&mut scope, Rc::clone(&self.invocations));
            for stmt in &closure.body {
                flow.visit_stmt(stmt);
            }
            for closure_use in closure.uses.iter().filter(|closure_use| closure_use.by_ref) {
                let name = closure_use.name.name.as_str();
                let settled = !scope.dynamic
                    && !scope.writes.contains_key(name)
                    && scope
                        .reads
                        .get(name)
                        .is_none_or(|reads| reads.iter().all(|read| *read == Read::Callee));
                self.out.insert((expr.span, name.to_string()), settled);
            }
        }
        visit::walk_expr(self, expr);
    }
}

/// Whether `inner` lies within `outer`.
fn range_contains(outer: Range, inner: Range) -> bool {
    (outer.start.line, outer.start.col) <= (inner.start.line, inner.start.col)
        && (inner.end.line, inner.end.col) <= (outer.end.line, outer.end.col)
}

/// Copies each followed literal into the table, keyed by its identity.
struct Collect<'c> {
    found: &'c FxHashMap<Span, Vec<String>>,
    id_of: &'c dyn Fn(Span) -> ClosureLiteralId,
    out: TrackedClosures,
}

impl<'ast> Visitor<'ast> for Collect<'_> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let ExprKind::Closure(closure) = &expr.kind
            && let Some(captures) = self.found.get(&expr.span)
        {
            let id = (self.id_of)(expr.span);
            self.out.insert(
                id,
                Rc::new(TrackedClosure {
                    span: expr.span,
                    closure: (**closure).clone(),
                    captures: captures.clone(),
                }),
            );
        }
        visit::walk_expr(self, expr);
    }
}

impl Checker<'_> {
    /// Checks each followed literal `callee` may be, at this call. The
    /// branches of its body that `args`' literal values leave dead report
    /// nothing.
    pub(crate) fn check_tracked_closure_calls(
        &mut self,
        callee: &Type,
        args: &[Arg],
        locals: &mut Locals,
        label: &str,
    ) {
        let values = self.invocations.argument_values(args);
        let positional = args.iter().all(|arg| !arg.spread && arg.name.is_none());
        let arguments: Option<Vec<Type>> = positional.then(|| {
            args.iter()
                .map(|arg| self.infer_expr(&arg.value, locals))
                .collect()
        });
        for id in closure_literal_identities(callee) {
            let Some(tracked) = self.tracked_closures.get(&id).cloned() else {
                continue;
            };
            // The innermost open call's own variables shadow some names. A call
            // reading one of them through a capture would read the shadowing
            // variable, not the captured one, so it is not checked here.
            let shadowed_by_caller = self.closure_call_stack.last().is_some_and(|(_, shadows)| {
                tracked.captures.iter().any(|name| shadows.contains(name))
            });
            if shadowed_by_caller {
                continue;
            }
            self.called_closures.insert(id);
            // A literal reached again from its own body, through a by-reference
            // capture, is checked at its call there until this many are open.
            let open = self
                .closure_call_stack
                .iter()
                .filter(|(open, _)| *open == id)
                .count();
            if open >= MAX_OPEN_CLOSURE_CALLS {
                continue;
            }
            // The names this body shadows: a parameter, or a variable it mentions
            // that it does not capture. The caller's variables of those names
            // may be written by a nested call this check does not follow, so
            // they are unknown after the call.
            let mentioned = crate::checker::invocation::body_mentions(&tracked.closure.body);
            let shadows: Vec<String> = tracked
                .captures
                .iter()
                .filter(|name| {
                    let captured = tracked
                        .closure
                        .uses
                        .iter()
                        .any(|closure_use| closure_use.name.name.as_str() == name.as_str());
                    let own = tracked
                        .closure
                        .params
                        .iter()
                        .any(|param| param.name.name.as_str() == name.as_str())
                        || mentioned.contains(*name);
                    !captured && own && locals.contains_var(name.as_str())
                })
                .cloned()
                .collect();
            let nested: Vec<String> = tracked
                .captures
                .iter()
                .filter(|name| !shadows.contains(name))
                .cloned()
                .collect();
            let before = self.diagnostics.len();
            self.closure_call_stack
                .push((id, shadows.iter().cloned().collect()));
            self.check_closure_literal(
                tracked.span,
                &tracked.closure,
                locals,
                label,
                ClosureMode::Call,
                CallInput {
                    arguments: arguments.as_deref(),
                    captures: &nested,
                },
            );
            self.closure_call_stack.pop();
            for name in &shadows {
                locals.insert_var(name, types::Type::mixed());
                locals.clear_native_var(name);
            }
            let dead = self.invocations.dead_branches(&tracked.closure, &values);
            self.drop_in_dead_branches(before, &dead);
            self.drop_call_specific_constancy(id, before);
            self.drop_repeated_diagnostics(before);
        }
    }

    /// Checks the closure literals a same-file function `name` invokes on every
    /// path, at this call: each positional argument at a parameter the function
    /// invokes, and each global the function invokes when it is called. The
    /// call runs them in the state it sees, so each is checked with that state
    /// and no argument values of its own.
    pub(crate) fn check_must_invoked_calls(
        &mut self,
        name: &str,
        args: &[Arg],
        locals: &mut Locals,
        label: &str,
    ) {
        if let Some(invoked) = self
            .invocations
            .must_invoke
            .parameters(name)
            .map(<[bool]>::to_vec)
            && args.iter().all(|arg| !arg.spread && arg.name.is_none())
        {
            for (index, arg) in args.iter().enumerate() {
                if invoked.get(index) == Some(&true) {
                    let callee = self.infer_expr(&arg.value, locals);
                    self.check_tracked_closure_calls(&callee, &[], locals, label);
                }
            }
        }
        for global in self.invocations.must_invoke.globals(name).to_vec() {
            if locals.contains_var(&global) && !locals.any_variable_may_exist() {
                let callee = self.variable_type(&global, locals);
                self.check_tracked_closure_calls(&callee, &[], locals, label);
            }
        }
    }

    /// Drops what was reported from `from` on that a condition or narrowing
    /// is constant, unless the literal's creation check reports it too. Such a
    /// finding describes the body, which runs at every call, so the argument
    /// values of one call must not decide it. A literal with no creation
    /// record keeps its findings.
    fn drop_call_specific_constancy(&mut self, id: ClosureLiteralId, from: usize) {
        let Some(closure_wide) = self.pending_closure_diagnostics.get(&id) else {
            return;
        };
        let mut index = from;
        while index < self.diagnostics.len() {
            let finding = &self.diagnostics[index];
            let call_specific =
                is_constancy(finding.identifier.as_str()) && !closure_wide.contains(finding);
            if call_specific {
                self.diagnostics.remove(index);
            } else {
                index += 1;
            }
        }
    }

    /// Drops what was reported from `from` on inside one of the `dead` spans.
    pub(crate) fn drop_in_dead_branches(&mut self, from: usize, dead: &[Span]) {
        if dead.is_empty() {
            return;
        }
        let ranges: Vec<Range> = dead
            .iter()
            .map(|span| self.line_index.range(*span))
            .collect();
        let mut index = from;
        while index < self.diagnostics.len() {
            let span = self.diagnostics[index].span;
            if ranges.iter().any(|range| range_contains(*range, span)) {
                self.diagnostics.remove(index);
            } else {
                index += 1;
            }
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

/// How many calls of one followed literal may be open at once, the calls
/// nested through by-reference captures included.
const MAX_OPEN_CLOSURE_CALLS: usize = 4;

/// Whether a diagnostic identifier says that a condition or narrowing is
/// constant: the finding holds for the body whatever its values are.
pub(crate) fn is_constancy(identifier: &str) -> bool {
    [
        "alwaysTrue",
        "alwaysFalse",
        "AlwaysTrue",
        "alwaysEmpty",
        "alreadyNarrowedType",
    ]
    .iter()
    .any(|suffix| identifier.ends_with(suffix))
}

/// The followed literals a value of type `ty` can be: the closure literal
/// identity of each member of a union, or of `ty` itself.
fn closure_literal_identities(ty: &Type) -> Vec<ClosureLiteralId> {
    ty.union_members()
        .iter()
        .filter_map(|member| member.callable_sig()?.literal)
        .collect()
}

impl ScopeFlow<'_> {
    /// For a call of a same-file function `callee` names, with arguments
    /// `args`: which of its positional parameters the call never invokes.
    fn uninvoked_parameters(&self, callee: &Expr, args: &[Arg]) -> Option<Vec<bool>> {
        let ExprKind::Name(name) = &callee.kind else {
            return None;
        };
        if args.iter().any(|arg| arg.spread || arg.name.is_some()) {
            return None;
        }
        let values = self.invocations.argument_values(args);
        let invoked = self
            .invocations
            .invoked_by_call(name.written.as_str(), &values)?;
        Some(invoked.iter().map(|invoked| !invoked).collect())
    }

    /// For a call of a same-file function `callee` names, with arguments
    /// `args`: which of its positional parameters every path of its body
    /// invokes.
    fn invoked_parameters(&self, callee: &Expr, args: &[Arg]) -> Option<Vec<bool>> {
        let ExprKind::Name(name) = &callee.kind else {
            return None;
        };
        if args.iter().any(|arg| arg.spread || arg.name.is_some()) {
            return None;
        }
        self.invocations
            .must_invoke
            .parameters(name.written.as_str())
            .map(<[bool]>::to_vec)
    }
}
