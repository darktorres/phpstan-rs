//! A typed property with no default value that nothing anywhere in the
//! class's own body ever assigns. Real PHP throws `Error: Typed property
//! $x must not be accessed before initialization` the first time such a
//! property is read.
//!
//! Deliberately narrower than a real dataflow that proves a property is
//! initialized *before its first real read* (tracking exactly which
//! methods a constructor calls, and in what order). This settles for a
//! cheaper, more permissive question instead - "is this property
//! assigned *anywhere at all* in the class's own body, by any method" -
//! trading away catching harder cases (a constructor that forgets to
//! call its own initializer, a property only initialized on an
//! unreachable path) for avoiding a false-positive class: a property
//! meant to be set by a separate "handler"/"reset" method called
//! sometime *after* construction, guarded by `assert(!isset($this->x))`
//! (real PHP: `isset()` on an uninitialized typed property returns
//! `false` rather than throwing, so this is genuinely safe code).
//!
//! Opt-in (`RulesSettings::check_uninitialized_properties`), since this
//! heuristic is considerably simpler than a real dataflow analysis and
//! isn't safe to run unconditionally.

use rustc_hash::FxHashSet;

use crate::checker::Checker;
use syntax::ast::*;
use syntax::visit::arena::{self, Visitor};

impl<'a> Checker<'a> {
    /// Flags `property.uninitialized` for a concrete class's own typed,
    /// non-static, non-readonly, no-default, no-hook property that no
    /// method anywhere in the class body ever assigns.
    ///
    /// A readonly property is excluded: its "assign exactly once, only
    /// from inside the constructor" contract is a distinct check
    /// (`property.readOnlyAssignNotInConstructor`) with its own failure
    /// mode. Abstract classes are skipped - a common, legitimate pattern
    /// relies on a concrete subclass's own constructor to finish
    /// initialization the abstract class's own can't (or doesn't) do,
    /// which this project has no way to verify happens on every real
    /// subclass.
    pub(crate) fn check_uninitialized_properties(&mut self, fqn: &str, c: &ClassDecl) {
        if !self.settings.rules.check_uninitialized_properties {
            return;
        }
        let Some(syntax_owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = syntax_owner.arenas();
        let Some(def) = arenas.class(c.def) else {
            return;
        };
        if def.kind != ClassKind::Class || def.is_abstract {
            return;
        }
        for p in def.properties(arenas) {
            if p.is_static || p.is_readonly || p.default.is_some() || p.hooks.is_some() {
                continue;
            }
            // A nullable native hint (`?int`) still needs an explicit
            // assignment in real PHP - unlike a plain untyped property,
            // PHP never implicitly defaults a *typed* property to `null`
            // just because its type happens to allow one, so nullability
            // is deliberately not an exemption here.
            if p.type_hint.is_none() {
                continue;
            }
            if property_assigned_anywhere_in_hierarchy(self, c, fqn, &p.name) {
                continue;
            }
            report!(
                self,
                format!(
                    "{fqn} has an uninitialized property ${}. Give it a default value or assign it in the constructor.",
                    p.name
                ),
                p.span,
                2,
                "property.uninitialized",
            );
        }
    }
}

pub(super) fn class_ever_assigns_property(arenas: &NodeArenas, c: &ClassDecl, prop: &str) -> bool {
    let Some(def) = arenas.class(c.def) else {
        return false;
    };
    arenas.methods(def.methods.clone()).iter().any(|m| {
        // A static "named constructor" (`public static function
        // create(...): static { $node = new static; ...; $node->prop =
        // ...; return $node; }`) populates a *local* variable holding a
        // fresh instance, not `$this`. Collected once per method (not
        // globally) since the same variable name in a different method
        // could hold anything.
        let body = m.function.body_stmts(arenas);
        let self_vars = collect_self_instance_vars(arenas, body, &c.name);
        body_ever_assigns_property(arenas, body, prop, &self_vars)
    })
}

/// Whether `prop` is assigned *anywhere* reachable from `fqn`'s own
/// hierarchy - `class_ever_assigns_property` alone only looks at `c`'s
/// own directly-declared methods, which misses a property re-declared
/// across an inheritance chain (common when a subclass narrows a
/// parent's property type), initialized only by an ancestor's own
/// constructor logic (`parent::__construct()`). Can only make the
/// existing "never initialized" verdict less eager to flag. Ancestors
/// are walked via the registry (`FunctionReflection::body`), not AST - the
/// current file may not even contain them. Doesn't walk downward into a
/// subclass's own overrides - an unbounded, open-hierarchy question a
/// single-class check can't answer.
pub(super) fn property_assigned_anywhere_in_hierarchy(
    checker: &Checker,
    c: &ClassDecl,
    fqn: &str,
    prop: &str,
) -> bool {
    let Some(arenas) = checker.syntax_arenas() else {
        return false;
    };
    if class_ever_assigns_property(arenas, c, prop) {
        return true;
    }
    let mut current = checker
        .registry
        .class_references_by_name(fqn)
        .and_then(|references| references.extends.first().cloned());
    let mut seen = FxHashSet::default();
    while let Some(ancestor) = current {
        if !seen.insert(ancestor.clone()) {
            break; // cycle guard for a malformed/circular hierarchy
        }
        if checker.registry.class_by_name(&ancestor).is_none() {
            break;
        }
        let found = checker.registry.method_names(&ancestor).any(|method| {
            let Some(sig) = checker.registry.lookup_own_method(&ancestor, method) else {
                return false;
            };
            checker
                .bodies
                .summary(sig.provenance())
                .is_some_and(|summary| {
                    summary
                        .assigned
                        .as_ref()
                        .is_some_and(|assigned| assigned.assigns(prop, &ancestor))
                })
        });
        if found {
            return true;
        }
        current = checker
            .registry
            .class_references_by_name(&ancestor)
            .and_then(|references| references.extends.first().cloned());
    }
    false
}

fn body_ever_assigns_property(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    prop: &str,
    self_vars: &FxHashSet<String>,
) -> bool {
    struct Search<'a> {
        prop: &'a str,
        self_vars: &'a FxHashSet<String>,
        found: bool,
    }
    impl Visitor for Search<'_> {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            if self.found {
                return;
            }
            let Some(stmt) = arenas.stmt(id) else {
                return;
            };
            if let StmtKind::Expression(expr) = &stmt.kind
                && initializes_property(arenas, *expr, self.prop, self.self_vars)
            {
                self.found = true;
                return;
            }
            arena::walk_stmt(self, arenas, id);
        }

        fn visit_expr(&mut self, _: &NodeArenas, _: ExprId) {}
    }
    let mut search = Search {
        prop,
        self_vars,
        found: false,
    };
    arena::walk_stmts(&mut search, arenas, stmts);
    search.found
}

/// Recurses through every nested statement looking for a
/// `$this->$prop = ...;` (or, through `self_vars`, `$localVar->$prop =
/// ...;` for a variable known to hold a fresh instance of the same
/// class) anywhere at all, however deeply nested and however
/// conditional - deliberately permissive, unlike a real dataflow
/// guarantee. Doesn't descend into a closure/arrow-fn's own body; a
/// property assigned only inside one is an accepted gap.
fn initializes_property(
    arenas: &NodeArenas,
    id: ExprId,
    prop: &str,
    self_vars: &FxHashSet<String>,
) -> bool {
    let Some(expr) = arenas.expr(id) else {
        return false;
    };
    match &expr.kind {
        ExprKind::Assign(lhs, _) | ExprKind::AssignRef(lhs, _) => {
            is_tracked_property(arenas, *lhs, prop, self_vars)
        }
        // `$this->x ??= $default;` is real PHP's canonical lazy-init
        // idiom for a typed property specifically because `??=`'s
        // left-hand check is `isset()`-based, not a raw read, and
        // `isset()` is exempted from the "must not be accessed before
        // initialization" error a raw read throws.
        ExprKind::CompoundAssign(BinOp::Coalesce, lhs, _) => {
            is_tracked_property(arenas, *lhs, prop, self_vars)
        }
        _ => false,
    }
}

fn is_tracked_property(
    arenas: &NodeArenas,
    id: ExprId,
    prop: &str,
    self_vars: &FxHashSet<String>,
) -> bool {
    let Some(expr) = arenas.expr(id) else {
        return false;
    };
    let ExprKind::PropertyAccess(base, name, _) = &expr.kind else {
        return false;
    };
    if name.as_ref() != prop {
        return false;
    }
    match arenas.expr(*base).map(|base| &base.kind) {
        Some(ExprKind::Variable(v)) => v.as_ref() == "this" || self_vars.contains(v.as_ref()),
        _ => false,
    }
}

/// Every local variable name directly assigned `new self(...)`/`new
/// static(...)`/a bare `new <ThisClass>(...)` anywhere in `stmts` -
/// scoped to the single, common `$var = new ...;` expression-statement
/// shape, not e.g. a ternary-wrapped `new`.
fn collect_self_instance_vars(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    class_name: &str,
) -> FxHashSet<String> {
    let mut vars = FxHashSet::default();
    collect_self_instance_vars_into(arenas, stmts, class_name, &mut vars);
    vars
}

fn collect_self_instance_vars_into(
    arenas: &NodeArenas,
    stmts: &[StmtId],
    class_name: &str,
    vars: &mut FxHashSet<String>,
) {
    struct Find<'a> {
        class_name: &'a str,
        vars: &'a mut FxHashSet<String>,
    }
    impl Visitor for Find<'_> {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            let Some(stmt) = arenas.stmt(id) else {
                return;
            };
            if let StmtKind::Expression(expr) = &stmt.kind
                && let Some(ExprKind::Assign(lhs, rhs)) = arenas.expr(*expr).map(|expr| &expr.kind)
                && let Some(ExprKind::Variable(var)) = arenas.expr(*lhs).map(|lhs| &lhs.kind)
                && is_new_self_instance(arenas, *rhs, self.class_name)
            {
                self.vars.insert(var.to_string());
            }
            arena::walk_stmt(self, arenas, id);
        }

        fn visit_expr(&mut self, _: &NodeArenas, _: ExprId) {}
    }
    let mut find = Find { class_name, vars };
    arena::walk_stmts(&mut find, arenas, stmts);
}

fn is_new_self_instance(arenas: &NodeArenas, id: ExprId, class_name: &str) -> bool {
    let mut id = id;
    loop {
        let Some(expr) = arenas.expr(id) else {
            return false;
        };
        match &expr.kind {
            ExprKind::New(callee, _) => {
                let Some(ExprKind::Name(name)) = arenas.expr(*callee).map(|callee| &callee.kind)
                else {
                    return false;
                };
                let Some(name) = arenas.qualified_name(name) else {
                    return false;
                };
                let name: &str = name.written.as_ref();
                let name = name.trim_start_matches('\\');
                let class_suffix = format!("\\{class_name}");
                return name.eq_ignore_ascii_case("self")
                    || name.eq_ignore_ascii_case("static")
                    || name == class_name
                    || name.ends_with(class_suffix.as_str());
            }
            // A chained assignment (`$node = $tag->node = new static;`)
            // whose right-hand side is itself another `Assign`, not a bare
            // `New` - unwrapped recursively until the actual `new` expression
            // at the tail is reached.
            ExprKind::Assign(_, rhs) => id = *rhs,
            _ => return false,
        }
    }
}
