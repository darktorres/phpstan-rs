//! What of a scope's variables code other than the scope's own may write or
//! run, found before the scope is checked: the variables bound to a global
//! (every one, at a file's top level), to the declaration's static
//! variables, or by reference to a place; the variables a closure the scope
//! writes captures by reference; the variables holding only closures the
//! scope wrote, with what their bodies do; and the variables holding only
//! objects the scope made that never leave it, whose properties no other
//! code can reach.

use std::cell::OnceCell;
use std::ops::Deref;

use rustc_hash::{FxHashMap, FxHashSet};
use syntax::ast::*;
use syntax::name::Name;
use syntax::visit::arena::{self, Visitor};

use super::SyntaxTreeOwner;
use crate::registry::Registry;
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
    pub(crate) captured: FxHashMap<Name, Capture>,
    /// Where each closure the scope wrote may first leave it: by the id of
    /// its expression.
    pub(crate) closure_escapes: FxHashMap<u32, Vec<Escape>>,
    /// The variables holding only closures, arrow functions or first-class
    /// callables the scope wrote, each one summarized
    /// (`BodySummary::of_closure`).
    pub(crate) closures: FxHashMap<Name, Vec<(ExprId, BodySummary)>>,
    /// The variables holding only objects the scope made, each with where
    /// the object may first leave the scope: no other code can reach it
    /// before that, while it is used only as the base of a property access,
    /// an operand of `===`, `!==`, `instanceof`, `isset` or `empty`, or an
    /// assignment's target (`unescaped_at`).
    pub(crate) made: FxHashMap<Name, Vec<Escape>>,
    /// The variables a reference to the variable or to one of its elements
    /// may exist for, wherever in the scope it is taken: a by-reference
    /// parameter, an `&` binding, a by-reference array item, `foreach`
    /// subject or destructuring.
    pub(crate) exposed: FxHashSet<Name>,
    /// Any variable may be exposed: the scope computes a variable's name,
    /// extracts into its variables or includes a file.
    pub(crate) all_exposed: bool,
}

/// What shares a variable with a closure that captures it by reference.
#[derive(Debug, Default)]
pub(crate) struct Capture {
    /// The scope is a closure's, which captures the variable.
    pub(crate) own: bool,
    /// The closures the scope wrote that capture it, by the id of their
    /// expression.
    pub(crate) closures: FxHashSet<u32>,
}

/// A use of a variable that may let the object it holds leave the scope.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Escape {
    /// Where the statement it is in starts; `None` when that is not known,
    /// or a jump may run the code before it again.
    stmt: Option<u32>,
    /// The outermost loop around the statement: the code in it runs again
    /// after it.
    in_loop: Option<(u32, u32)>,
    /// The use leaves the object with code that returns it: only the code
    /// of the statement itself may reach it.
    transient: bool,
}

impl Escape {
    /// Whether any of `escapes` has happened by the statement starting at
    /// `at`: it is not before it, or it is in a loop `at` is in.
    fn reached(escapes: &[Escape], at: Option<u32>) -> bool {
        let Some(at) = at else {
            return !escapes.is_empty();
        };
        escapes.iter().any(|escape| match escape.stmt {
            None => true,
            Some(stmt) if escape.transient => stmt == at,
            Some(stmt) => {
                stmt <= at
                    || escape
                        .in_loop
                        .is_some_and(|(start, end)| start <= at && at < end)
            }
        })
    }
}

impl SharedScope {
    /// Whether no code but the scope's own can reach the object `name`
    /// holds at the statement starting at `at`: it holds only objects the
    /// scope made, and none of its uses before that, or in a loop that
    /// contains it, lets the object leave.
    pub(crate) fn unescaped_at(&self, name: &Name, at: Option<u32>) -> bool {
        self.made
            .get(name)
            .is_some_and(|escapes| !Escape::reached(escapes, at))
    }

    /// Whether code other than the scope's own may hold the closure written
    /// as `closure` at the statement starting at `at`.
    pub(crate) fn closure_left_at(&self, closure: u32, at: Option<u32>) -> bool {
        self.closure_escapes
            .get(&closure)
            .is_none_or(|escapes| Escape::reached(escapes, at))
    }

    /// The shared scope of the statements `body`, a function's, a closure's
    /// (which captures `by_ref_uses` by reference) or a file's top level.
    pub(crate) fn of(
        registry: &Registry,
        arenas: &NodeArenas,
        body: &[StmtId],
        by_ref_uses: &[Name],
        function: Option<(Option<Name>, Name)>,
        top_level: bool,
    ) -> SharedScope {
        let mut scan = Scan::new(registry);
        for name in by_ref_uses {
            scan.captured.entry(name.clone()).or_default().own = true;
            scan.held.insert(name.clone(), Held::Other);
        }
        arena::walk_stmts(&mut scan, arenas, body);
        scan.finish(function, top_level)
    }

    /// The shared scope of an arrow function's body, which binds nothing.
    pub(crate) fn of_arrow_function(
        registry: &Registry,
        arenas: &NodeArenas,
        body: ExprId,
    ) -> SharedScope {
        let mut scan = Scan::new(registry);
        scan.visit_expr(arenas, body);
        scan.finish(None, false)
    }

    /// The scope's by-reference parameters are exposed.
    fn expose(&mut self, by_ref_params: &[Name]) {
        self.exposed.extend(by_ref_params.iter().cloned());
    }

    /// The scope as text, with the keys sorted, for comparing two builds.
    #[cfg(debug_assertions)]
    fn fingerprint(&self) -> String {
        fn sorted<'a, T: std::fmt::Debug + 'a>(items: impl Iterator<Item = T>) -> Vec<String> {
            let mut items: Vec<String> = items.map(|item| format!("{item:?}")).collect();
            items.sort();
            items
        }
        format!(
            "{:?}",
            (
                (
                    self.top_level,
                    self.closure,
                    self.any_global,
                    self.all_exposed
                ),
                sorted(self.globals.iter()),
                sorted(self.statics.iter()),
                &self.function,
                sorted(self.references.keys()),
                sorted(self.captured.iter()),
                sorted(self.closure_escapes.iter()),
                sorted(self.closures.keys()),
                sorted(self.made.iter()),
                sorted(self.exposed.iter()),
            )
        )
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

/// What a lazily built scope is built from.
enum Source {
    Statements(Vec<StmtId>),
    Arrow(ExprId),
}

struct Plan<'a> {
    owner: SyntaxTreeOwner<'a>,
    registry: &'a Registry,
    source: Source,
    by_ref_uses: Vec<Name>,
    function: Option<(Option<Name>, Name)>,
    top_level: bool,
    closure: bool,
    by_ref_params: Vec<Name>,
}

/// A `SharedScope` built when something first reads it: many bodies never
/// do.
pub(crate) struct LazyShared<'a> {
    scope: OnceCell<SharedScope>,
    plan: Option<Plan<'a>>,
    #[cfg(debug_assertions)]
    eager: Option<String>,
}

impl Default for LazyShared<'_> {
    fn default() -> Self {
        LazyShared::ready(SharedScope::default())
    }
}

impl<'a> LazyShared<'a> {
    pub(crate) fn ready(scope: SharedScope) -> Self {
        Self {
            scope: OnceCell::from(scope),
            plan: None,
            #[cfg(debug_assertions)]
            eager: None,
        }
    }

    /// The scope of the statements `body`: a function's, a closure's
    /// (`closure`, whose `use` list is `uses`) or a file's top level.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn of_statements(
        owner: Option<SyntaxTreeOwner<'a>>,
        registry: &'a Registry,
        arenas: &NodeArenas,
        body: &[StmtId],
        uses: &[ClosureUse],
        function: Option<(Option<Name>, Name)>,
        top_level: bool,
        closure: bool,
        params: &[Param],
    ) -> Self {
        let by_ref_uses: Vec<Name> = uses
            .iter()
            .filter(|u| u.by_ref)
            .map(|u| u.name.name.clone())
            .collect();
        let source = Source::Statements(body.to_vec());
        Self::planned(
            owner,
            registry,
            arenas,
            source,
            by_ref_uses,
            function,
            top_level,
            closure,
            params,
        )
    }

    /// The scope of an arrow function's or a hook's expression body.
    pub(crate) fn of_expression(
        owner: Option<SyntaxTreeOwner<'a>>,
        registry: &'a Registry,
        arenas: &NodeArenas,
        body: ExprId,
        closure: bool,
        params: &[Param],
    ) -> Self {
        Self::planned(
            owner,
            registry,
            arenas,
            Source::Arrow(body),
            Vec::new(),
            None,
            false,
            closure,
            params,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn planned(
        owner: Option<SyntaxTreeOwner<'a>>,
        registry: &'a Registry,
        arenas: &NodeArenas,
        source: Source,
        by_ref_uses: Vec<Name>,
        function: Option<(Option<Name>, Name)>,
        top_level: bool,
        closure: bool,
        params: &[Param],
    ) -> Self {
        let by_ref_params: Vec<Name> = params
            .iter()
            .filter(|param| param.by_ref)
            .map(|param| param.name.name.clone())
            .collect();
        let Some(owner) = owner else {
            let plan = PlanParts {
                registry,
                source: &source,
                by_ref_uses: &by_ref_uses,
                function: &function,
                top_level,
                closure,
                by_ref_params: &by_ref_params,
            };
            return Self::ready(plan.build(arenas));
        };
        let plan = Plan {
            owner,
            registry,
            source,
            by_ref_uses,
            function,
            top_level,
            closure,
            by_ref_params,
        };
        #[cfg(debug_assertions)]
        let eager = Some(plan.parts().build(arenas).fingerprint());
        Self {
            scope: OnceCell::new(),
            plan: Some(plan),
            #[cfg(debug_assertions)]
            eager,
        }
    }

    pub(crate) fn get(&self) -> &SharedScope {
        self.scope.get_or_init(|| {
            let plan = self.plan.as_ref().expect("a scope with no plan is ready");
            let scope = plan.parts().build(plan.owner.arenas());
            #[cfg(debug_assertions)]
            if let Some(eager) = &self.eager {
                debug_assert_eq!(*eager, scope.fingerprint());
            }
            scope
        })
    }

    /// The plan of a scope nothing has read yet.
    fn unread(&self) -> Option<&Plan<'a>> {
        match self.scope.get() {
            Some(_) => None,
            None => self.plan.as_ref(),
        }
    }

    /// Whether the scope is a file's top level.
    pub(crate) fn top_level(&self) -> bool {
        match &self.plan {
            Some(plan) => plan.top_level,
            None => self.get().top_level,
        }
    }

    /// `SharedScope::binds_nothing`, without building the scope for a file
    /// with nothing that binds a variable to share.
    pub(crate) fn binds_nothing(&self) -> bool {
        if let Some(plan) = self.unread()
            && !plan.top_level
            && plan.by_ref_uses.is_empty()
            && !plan.owner.arenas().binds_shared_variables()
        {
            debug_assert!(self.get().binds_nothing());
            return true;
        }
        self.get().binds_nothing()
    }

    /// Whether the scope or a closure it writes captures a variable by
    /// reference.
    pub(crate) fn captures_nothing(&self) -> bool {
        if let Some(plan) = self.unread()
            && plan.by_ref_uses.is_empty()
            && !plan.owner.arenas().has_closure_use_by_ref()
        {
            debug_assert!(self.get().captured.is_empty());
            return true;
        }
        self.get().captured.is_empty()
    }

    /// Whether `name` is one of the scope's static variables.
    pub(crate) fn has_static(&self, name: &Name) -> bool {
        if let Some(plan) = self.unread()
            && !plan.owner.arenas().has_static_var_statement()
        {
            debug_assert!(!self.get().statics.contains(name));
            return false;
        }
        self.get().statics.contains(name)
    }
}

impl Deref for LazyShared<'_> {
    type Target = SharedScope;

    fn deref(&self) -> &SharedScope {
        self.get()
    }
}

struct PlanParts<'p> {
    registry: &'p Registry,
    source: &'p Source,
    by_ref_uses: &'p [Name],
    function: &'p Option<(Option<Name>, Name)>,
    top_level: bool,
    closure: bool,
    by_ref_params: &'p [Name],
}

impl PlanParts<'_> {
    fn build(&self, arenas: &NodeArenas) -> SharedScope {
        let mut scope = match self.source {
            Source::Statements(body) => SharedScope::of(
                self.registry,
                arenas,
                body,
                self.by_ref_uses,
                self.function.clone(),
                self.top_level,
            ),
            Source::Arrow(body) => SharedScope::of_arrow_function(self.registry, arenas, *body),
        };
        scope.closure = self.closure;
        scope.expose(self.by_ref_params);
        scope
    }
}

impl Plan<'_> {
    fn parts(&self) -> PlanParts<'_> {
        PlanParts {
            registry: self.registry,
            source: &self.source,
            by_ref_uses: &self.by_ref_uses,
            function: &self.function,
            top_level: self.top_level,
            closure: self.closure,
            by_ref_params: &self.by_ref_params,
        }
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
struct Scan<'r> {
    /// What the functions the scope calls do with their arguments; `None`
    /// when it is not known, and every call keeps them.
    registry: Option<&'r Registry>,
    globals: FxHashSet<Name>,
    any_global: bool,
    statics: FxHashSet<Name>,
    references: FxHashMap<Name, Place>,
    captured: FxHashMap<Name, Capture>,
    /// Where each closure was written.
    closure_at: FxHashMap<u32, Escape>,
    held: FxHashMap<Name, Held>,
    /// Each use of a variable that may let what it holds go, with whether
    /// it leaves the scope (any use but calling the variable or passing it
    /// to a function that keeps nothing of it).
    escapes: FxHashMap<Name, Vec<(Escape, bool)>>,
    /// Where the statement being scanned starts.
    stmt_at: Option<u32>,
    /// The loops being scanned, outermost first.
    loops: Vec<(u32, u32)>,
    /// A jump may run code again.
    jumps: bool,
    exposed: FxHashSet<Name>,
    all_exposed: bool,
}

impl<'r> Scan<'r> {
    fn new(registry: &'r Registry) -> Scan<'r> {
        Scan {
            registry: Some(registry),
            ..Scan::default()
        }
    }

    fn finish(self, function: Option<(Option<Name>, Name)>, top_level: bool) -> SharedScope {
        let bound = |name: &Name| {
            self.globals.contains(name)
                || self.statics.contains(name)
                || self.references.contains_key(name)
                || self.captured.contains_key(name)
        };
        let mut made = FxHashMap::default();
        let mut closures = FxHashMap::default();
        let mut escapes = self.escapes;
        let mut closure_escapes: FxHashMap<u32, Vec<Escape>> = self
            .closure_at
            .iter()
            .map(|(&closure, &escape)| (closure, vec![escape]))
            .collect();
        for (name, held) in self.held {
            if bound(&name) || top_level {
                continue;
            }
            match held {
                Held::Made => {
                    let mut escapes: Vec<Escape> = escapes
                        .remove(&name)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(escape, _)| escape)
                        .collect();
                    if self.jumps {
                        for escape in &mut escapes {
                            escape.stmt = None;
                        }
                    }
                    made.insert(name, escapes);
                }
                Held::Closures(bodies) => {
                    let leaks: Vec<Escape> = escapes
                        .get(&name)
                        .into_iter()
                        .flatten()
                        .filter(|(_, leaks)| *leaks)
                        .map(|(escape, _)| *escape)
                        .collect();
                    if let Some(((last, _), rest)) = bodies.split_last() {
                        for (id, _) in rest {
                            closure_escapes.insert(id.into_raw().into_u32(), leaks.clone());
                        }
                        closure_escapes.insert(last.into_raw().into_u32(), leaks);
                    }
                    closures.insert(name, bodies);
                }
                Held::Other => {}
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
            closure_escapes,
            made,
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
        let Some(slot) = self.held.get_mut(name) else {
            self.held.insert(name.clone(), held);
            return;
        };
        *slot = match (std::mem::replace(slot, Held::Other), held) {
            (Held::Made, Held::Made) => Held::Made,
            (Held::Closures(mut a), Held::Closures(b)) => {
                a.extend(b);
                Held::Closures(a)
            }
            _ => Held::Other,
        };
    }

    /// A use of the variable `name` that may let the object it holds leave
    /// the scope, in the statement being scanned.
    fn escape(&mut self, name: &Name) {
        let escape = self.here();
        self.escapes
            .entry(name.clone())
            .or_default()
            .push((escape, true));
    }

    /// Passing the variable `name` to a function that keeps nothing of it:
    /// code reaches the object it holds during the call alone, and a closure
    /// it holds leaves nothing.
    fn passed(&mut self, name: &Name) {
        let escape = Escape {
            transient: true,
            ..self.here()
        };
        self.escapes
            .entry(name.clone())
            .or_default()
            .push((escape, false));
    }

    /// Calling the variable `name`: what it holds, if an object, gets its
    /// `__invoke` a `$this`, but a closure it holds leaves nothing.
    fn call(&mut self, name: &Name) {
        let escape = self.here();
        self.escapes
            .entry(name.clone())
            .or_default()
            .push((escape, false));
    }

    fn here(&self) -> Escape {
        Escape {
            stmt: self.stmt_at,
            in_loop: self.loops.first().copied(),
            transient: false,
        }
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

/// An object made by `new` of `class` (`None` for a class not known), which
/// no other code can reach unless a constructor leaks it.
fn made_unless_leaked(registry: Option<&Registry>, class: Option<&QualifiedName>) -> Held {
    match registry {
        Some(registry) if !registry.new_may_leak(class) => Held::Made,
        _ => Held::Other,
    }
}

/// What the scope made or wrote that `e` evaluates to: an object made by
/// `new` or `clone`, or given back by a function that returns only objects
/// it made.
fn held_by(registry: Option<&Registry>, arenas: &NodeArenas, id: ExprId) -> Held {
    let Some(id) = arenas.peeled(id) else {
        return Held::Other;
    };
    let Some(e) = arenas.expr(id) else {
        return Held::Other;
    };
    syntax::stack::guard(|| match &e.kind {
        ExprKind::New(callee, _) => {
            let class = match arenas.peeled(*callee).and_then(|id| arenas.expr(id)) {
                Some(ExprNode {
                    kind: ExprKind::Name(name),
                    ..
                }) => arenas.qualified_name(name),
                _ => None,
            };
            made_unless_leaked(registry, class)
        }
        ExprKind::NewAnonymousClass(_) | ExprKind::Clone(..) => made_unless_leaked(registry, None),
        ExprKind::Closure(_) | ExprKind::ArrowFn(_) | ExprKind::FirstClassCallable(_) => {
            Held::Closures(vec![(id, BodySummary::of_closure(arenas, id))])
        }
        ExprKind::Assign(_, rhs) => held_by(registry, arenas, *rhs),
        ExprKind::Call(callee, _) => {
            let fresh = registry.is_some_and(|registry| {
                match arenas.peeled(*callee).and_then(|id| arenas.expr(id)) {
                    Some(ExprNode {
                        kind: ExprKind::Name(name),
                        ..
                    }) => arenas
                        .qualified_name(name)
                        .is_some_and(|name| registry.function_returns_fresh(name)),
                    _ => false,
                }
            });
            if fresh { Held::Made } else { Held::Other }
        }
        ExprKind::StaticCall(target, method, _) => {
            let fresh = registry.is_some_and(|registry| {
                target
                    .as_name()
                    .is_some_and(|class| registry.static_method_returns_fresh(class, &method.name))
            });
            if fresh { Held::Made } else { Held::Other }
        }
        _ => Held::Other,
    })
}

impl Visitor for Scan<'_> {
    // A declaration nested in the scope is a scope of its own.
    fn visit_function_decl(&mut self, _: &NodeArenas, _: &FunctionDecl) {}

    fn visit_class_def(&mut self, _: &NodeArenas, _: &ClassDef) {}

    fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
        let Some(stmt) = arenas.stmt(id) else { return };
        let span = arenas.stmt_span(id);
        let outer = std::mem::replace(&mut self.stmt_at, span.map(|span| span.start));
        let looped = matches!(
            stmt.kind,
            StmtKind::While { .. }
                | StmtKind::DoWhile { .. }
                | StmtKind::For { .. }
                | StmtKind::Foreach { .. }
        );
        if looped {
            match span {
                Some(span) => self.loops.push((span.start, span.end)),
                None => self.jumps = true,
            }
        }
        if matches!(stmt.kind, StmtKind::Goto(_)) {
            self.jumps = true;
        }
        self.scan_stmt(arenas, id, stmt);
        if looped && span.is_some() {
            self.loops.pop();
        }
        self.stmt_at = outer;
    }

    fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        self.scan_expr(arenas, id);
    }
}

impl Scan<'_> {
    fn scan_stmt(&mut self, arenas: &NodeArenas, id: StmtId, stmt: &StmtNode) {
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

    fn scan_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
        let Some(e) = arenas.expr(id) else { return };
        syntax::stack::guard(|| match &e.kind {
            ExprKind::Variable(name) => self.escape(name),
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
                self.assign(arenas, *lhs, held_by(self.registry, arenas, *rhs));
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
            ExprKind::Call(callee, args) => {
                if writes_computed_variables(arenas, id) {
                    self.all_exposed = true;
                }
                match arenas.peeled(*callee).and_then(|id| arenas.expr(id)) {
                    Some(ExprNode {
                        kind: ExprKind::Variable(name),
                        ..
                    }) => {
                        self.call(name);
                        arena::walk_args(self, arenas, args.clone());
                    }
                    Some(ExprNode {
                        kind: ExprKind::Name(callee),
                        ..
                    }) => self.call_function(arenas, id, callee, args),
                    _ => arena::walk_expr(self, arenas, id),
                }
            }
            // A closure's body is a scope of its own; what it captures leaves
            // this one.
            ExprKind::Closure(closure_id) => {
                self.closure_at
                    .insert(id.into_raw().into_u32(), self.here());
                if let Some(ClosurePayload::Closure(closure)) = arenas.closure(*closure_id) {
                    for u in arenas.closure_uses(&closure.uses) {
                        self.escape(&u.name.name);
                        if u.by_ref {
                            self.captured
                                .entry(u.name.name.clone())
                                .or_default()
                                .closures
                                .insert(id.into_raw().into_u32());
                        }
                    }
                }
            }
            _ => arena::walk_expr(self, arenas, id),
        })
    }
}

impl Scan<'_> {
    /// A call of the function written as `callee`: an argument that is a
    /// variable the function keeps nothing of leaves nothing.
    fn call_function(
        &mut self,
        arenas: &NodeArenas,
        id: ExprId,
        callee: &QualifiedNameRange,
        args: &ArgRange,
    ) {
        let Some((registry, callee)) = self.registry.zip(arenas.qualified_name(callee)) else {
            arena::walk_expr(self, arenas, id);
            return;
        };
        let mut positional = true;
        for (at, arg) in arenas.args(args.clone()).iter().enumerate() {
            positional &= !arg.spread;
            let variable = arenas
                .peeled(arg.value)
                .and_then(|value| arenas.expr(value))
                .and_then(|value| match &value.kind {
                    ExprKind::Variable(name) => Some(name),
                    _ => None,
                });
            match variable {
                Some(name)
                    if positional
                        && arg.name.is_none()
                        && registry.releases_argument(callee, at as u32) =>
                {
                    self.passed(name);
                }
                _ => self.visit_expr(arenas, arg.value),
            }
        }
    }
}
