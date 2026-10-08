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
use syntax::visit::arena::{self, Visitor};

use crate::registry::summaries::{
    BodySummary, Place, Step, destructures_by_ref, writes_computed_variables,
};

/// See the module's doc comment.
#[derive(Default)]
pub(crate) struct SharedScope {
    /// The scope is a file's top level, where every variable is a global.
    pub(crate) top_level: bool,
    /// The scope is a closure's or an arrow function's, which may be bound
    /// to any class's scope.
    pub(crate) closure: bool,
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
    pub(crate) captured: FxHashMap<Name, FxHashSet<u32>>,
    /// The variables holding only closures, arrow functions or first-class
    /// callables the scope wrote, each one summarized
    /// (`BodySummary::of_closure`).
    pub(crate) closures: FxHashMap<Name, Vec<(ExprId, BodySummary)>>,
    /// The variables holding only objects the scope made, used only as the
    /// base of a property access, an operand of `===`, `!==`, `instanceof`,
    /// `isset` or `empty`, or an assignment's target: no other code can
    /// reach those objects.
    pub(crate) unescaped: FxHashSet<Name>,
    /// The variables a reference to the variable or to one of its elements
    /// may exist for, wherever in the scope it is taken: a by-reference
    /// parameter, an `&` binding, a by-reference array item, `foreach`
    /// subject or destructuring.
    pub(crate) exposed: FxHashSet<Name>,
    /// Any variable may be exposed: the scope computes a variable's name,
    /// extracts into its variables or includes a file.
    pub(crate) all_exposed: bool,
}

impl SharedScope {
    /// The shared scope of the statements `body`, a function's, a closure's
    /// (whose `use` list is `uses`) or a file's top level.
    pub(crate) fn of(
        arenas: &NodeArenas,
        body: &[StmtId],
        uses: &[ClosureUse],
        function: Option<(Option<Name>, Name)>,
        top_level: bool,
    ) -> SharedScope {
        let mut scan = Scan::default();
        for u in uses {
            if u.by_ref {
                scan.captured.entry(u.name.name.clone()).or_default();
                scan.held.insert(u.name.name.clone(), Held::Other);
            }
        }
        arena::walk_stmts(&mut scan, arenas, body);
        scan.finish(function, top_level)
    }

    /// The shared scope of an arrow function's body, which binds nothing.
    pub(crate) fn of_arrow_function(arenas: &NodeArenas, body: ExprId) -> SharedScope {
        let mut scan = Scan::default();
        scan.visit_expr(arenas, body);
        scan.finish(None, false)
    }

    /// The scope's by-reference parameters are exposed.
    pub(crate) fn expose_parameters(&mut self, params: &[Param]) {
        for param in params.iter().filter(|param| param.by_ref) {
            self.exposed.insert(param.name.name.clone());
        }
    }

    /// Whether the array (or string) variable `name` is a value no other
    /// code can write: not shared, and no reference to it or to an element
    /// of it is taken in the scope.
    pub(crate) fn is_private(&self, name: &Name) -> bool {
        !self.top_level
            && !self.any_global
            && !self.all_exposed
            && **name != *"this"
            && !self.globals.contains(name)
            && !self.statics.contains(name)
            && !self.references.contains_key(name)
            && !self.captured.contains_key(name)
            && !self.exposed.contains(name)
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
    /// Only closures the scope wrote, each by its expression and
    /// summarized.
    Closures(Vec<(ExprId, BodySummary)>),
    /// Anything else.
    Other,
}

#[derive(Default)]
struct Scan {
    globals: FxHashSet<Name>,
    any_global: bool,
    statics: FxHashSet<Name>,
    references: FxHashMap<Name, Place>,
    captured: FxHashMap<Name, FxHashSet<u32>>,
    held: FxHashMap<Name, Held>,
    escaped: FxHashSet<Name>,
    exposed: FxHashSet<Name>,
    all_exposed: bool,
}

impl Scan {
    fn finish(self, function: Option<(Option<Name>, Name)>, top_level: bool) -> SharedScope {
        let bound = |name: &Name| {
            self.globals.contains(name)
                || self.statics.contains(name)
                || self.references.contains_key(name)
                || self.captured.contains_key(name)
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
            closure: false,
            globals: self.globals,
            any_global: self.any_global,
            statics: self.statics,
            function,
            references: self.references,
            captured: self.captured,
            closures,
            unescaped,
            exposed: self.exposed,
            all_exposed: self.all_exposed,
        }
    }

    /// A reference to the variable `id` is, or to an element of it, is taken.
    fn expose(&mut self, arenas: &NodeArenas, id: ExprId) {
        match array_root(arenas, id) {
            Some(name) => {
                self.exposed.insert(name);
            }
            None => {
                // A reference to an unnamed place exposes the variables it
                // is made of; one to a property or call result exposes none.
                if root_is_computed(arenas, id) {
                    self.all_exposed = true;
                }
            }
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
    fn kept(&mut self, arenas: &NodeArenas, id: ExprId) {
        if !arenas
            .peeled(id)
            .and_then(|id| arenas.expr(id))
            .is_some_and(|e| matches!(&e.kind, ExprKind::Variable(_)))
        {
            self.visit_expr(arenas, id);
        }
    }

    /// An assignment to `target` of what `held` says.
    fn assign(&mut self, arenas: &NodeArenas, id: ExprId, held: Held) {
        let Some(target) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
            return;
        };
        match &target.kind {
            ExprKind::Variable(name) => self.note(name, held),
            ExprKind::Destructure(items, _) => {
                for item in arenas.destructure_items(items) {
                    if let Some(key) = &item.key {
                        self.visit_expr(arenas, *key);
                    }
                    if let Some(target) = &item.target {
                        self.assign(arenas, *target, Held::Other);
                    }
                }
            }
            ExprKind::PropertyAccess(base, _, _) => self.kept(arenas, *base),
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.kept(arenas, *base);
                self.visit_expr(arenas, *name);
            }
            _ => self.visit_expr(arenas, id),
        }
    }
}

/// The variable `id` is, or the one an element of it is: `$a`, `$a['k']`,
/// `$a[$k][0]`.
pub(crate) fn array_root(arenas: &NodeArenas, id: ExprId) -> Option<Name> {
    let e = arenas.expr(arenas.peeled(id)?)?;
    syntax::stack::guard(|| match &e.kind {
        ExprKind::Variable(name) => Some(name.clone()),
        ExprKind::Index(base, _) => array_root(arenas, *base),
        _ => None,
    })
}

/// Whether the variable `id` is, or the one an element of it is, has a
/// computed name (`$$n`, `${'a' . $b}`).
fn root_is_computed(arenas: &NodeArenas, id: ExprId) -> bool {
    let Some(e) = arenas.peeled(id).and_then(|id| arenas.expr(id)) else {
        return false;
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::DynamicVariable(_) => true,
        ExprKind::Index(base, _) => root_is_computed(arenas, *base),
        _ => false,
    })
}

/// What the scope made or wrote that `e` evaluates to.
fn held_by(arenas: &NodeArenas, id: ExprId) -> Held {
    let Some(id) = arenas.peeled(id) else {
        return Held::Other;
    };
    let Some(e) = arenas.expr(id) else {
        return Held::Other;
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::New(..) | ExprKind::NewAnonymousClass(_) | ExprKind::Clone(..) => Held::Made,
        ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
            Held::Closures(vec![(id, BodySummary::of_closure(arenas, id))])
        }
        ExprKind::Assign(_, rhs) => held_by(arenas, *rhs),
        _ => Held::Other,
    })
}

impl Visitor for Scan {
    // A declaration nested in the scope is a scope of its own.
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}

    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        syntax::stack::guard(|| match &stmt.kind {
            StmtKind::Global(vars) => {
                for var in arenas.global_vars(vars.clone()) {
                    match var {
                        GlobalVar::Name(name) => {
                            self.globals.insert(name.name.clone());
                            self.note(&name.name, Held::Other);
                        }
                        GlobalVar::Dynamic(e) => {
                            self.any_global = true;
                            self.visit_expr(arenas, *e);
                        }
                    }
                }
            }
            StmtKind::StaticVar(vars) => {
                for variable in arenas.static_vars(vars.clone()) {
                    self.statics.insert(variable.name.name.clone());
                    self.note(&variable.name.name, Held::Other);
                    if let Some(default) = variable.default {
                        self.visit_expr(arenas, default);
                    }
                }
            }
            StmtKind::Unset(targets) => {
                for target in arenas.expr_ids(targets) {
                    self.kept(arenas, *target);
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                by_ref,
                body,
            } => {
                self.visit_expr(arenas, *subject);
                if *by_ref
                    || arenas
                        .peeled(*value)
                        .and_then(|id| arenas.expr(id))
                        .is_some_and(|value| match &value.kind {
                            ExprKind::Destructure(items, _) => destructures_by_ref(arenas, items),
                            _ => false,
                        })
                {
                    self.expose(arenas, *subject);
                }
                if let Some(key) = key {
                    self.assign(arenas, *key, Held::Other);
                }
                match arenas.peeled(*value).and_then(|id| arenas.expr(id)) {
                    Some(ExprNode {
                        kind: ExprKind::Variable(name),
                        ..
                    }) if *by_ref => {
                        let place = Place::of(arenas, *subject)
                            .unwrap_or_else(Place::value)
                            .then(Step::Element);
                        self.references.insert(name.clone(), place);
                        self.note(name, Held::Other);
                    }
                    _ => self.assign(arenas, *value, Held::Other),
                }
                self.visit_stmt(arenas, *body);
            }
            _ => arena::walk_stmt(self, arenas, id),
        })
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Variable(name) => {
                self.escaped.insert(name.clone());
            }
            ExprKind::PropertyAccess(base, _, _) => self.kept(arenas, *base),
            ExprKind::DynamicPropertyAccess(base, name, _) => {
                self.kept(arenas, *base);
                self.visit_expr(arenas, *name);
            }
            ExprKind::Assign(lhs, rhs) => {
                if arenas
                    .peeled(*lhs)
                    .and_then(|id| arenas.expr(id))
                    .is_some_and(|target| match &target.kind {
                        ExprKind::Destructure(items, _) => destructures_by_ref(arenas, items),
                        _ => false,
                    })
                {
                    self.expose(arenas, *rhs);
                }
                self.assign(arenas, *lhs, held_by(arenas, *rhs));
                self.visit_expr(arenas, *rhs);
            }
            ExprKind::AssignRef(lhs, rhs) => {
                self.expose(arenas, *lhs);
                self.expose(arenas, *rhs);
                match arenas.peeled(*lhs).and_then(|id| arenas.expr(id)) {
                    Some(ExprNode {
                        kind: ExprKind::Variable(name),
                        ..
                    }) => {
                        let place = Place::of(arenas, *rhs).unwrap_or_else(Place::value);
                        self.references.insert(name.clone(), place);
                        self.note(name, Held::Other);
                    }
                    _ => self.assign(arenas, *lhs, Held::Other),
                }
                self.visit_expr(arenas, *rhs);
            }
            ExprKind::CompoundAssign(_, lhs, rhs) => {
                self.assign(arenas, *lhs, Held::Other);
                self.visit_expr(arenas, *lhs);
                self.visit_expr(arenas, *rhs);
            }
            ExprKind::Unary(
                UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec,
                target,
            ) => {
                self.assign(arenas, *target, Held::Other);
                self.visit_expr(arenas, *target);
            }
            ExprKind::Binary(BinOp::Identical | BinOp::NotIdentical, a, b) => {
                self.kept(arenas, *a);
                self.kept(arenas, *b);
            }
            ExprKind::Instanceof(subject, target) => {
                self.kept(arenas, *subject);
                if let InstanceofTarget::Expr(target) = target {
                    self.visit_expr(arenas, *target);
                }
            }
            ExprKind::Isset(items) => {
                for item in arenas.expr_ids(items) {
                    self.kept(arenas, *item);
                }
            }
            ExprKind::Empty(item) => self.kept(arenas, *item),
            ExprKind::ArrayLiteral(items) => {
                for item in arenas.array_items(items.clone()) {
                    if item.by_ref {
                        self.expose(arenas, item.value);
                    }
                }
                arena::walk_expr(self, arenas, id);
            }
            ExprKind::DynamicVariable(_) | ExprKind::Eval(_) | ExprKind::Include(..) => {
                self.all_exposed = true;
                arena::walk_expr(self, arenas, id);
            }
            ExprKind::Call(..) => {
                if writes_computed_variables(arenas, id) {
                    self.all_exposed = true;
                }
                arena::walk_expr(self, arenas, id);
            }
            // A closure's body is a scope of its own; what it captures leaves
            // this one.
            ExprKind::Closure(closure_id) => {
                if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure_id) {
                    for u in arenas.closure_uses(&closure.uses) {
                        self.escaped.insert(u.name.name.clone());
                        if u.by_ref {
                            self.captured
                                .entry(u.name.name.clone())
                                .or_default()
                                .insert(id.into_raw().into_u32());
                        }
                    }
                }
            }
            _ => arena::walk_expr(self, arenas, id),
        })
    }
}
