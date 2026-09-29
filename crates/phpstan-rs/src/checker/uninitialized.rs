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
use syntax::visit::any_stmt_in_body;

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
        if c.def.kind != ClassKind::Class || c.def.is_abstract {
            return;
        }
        for p in &c.def.properties {
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
            self.report(format!(
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

pub(super) fn class_ever_assigns_property(c: &ClassDecl, prop: &str) -> bool {
    c.def.methods.iter().any(|m| {
        // A static "named constructor" (`public static function
        // create(...): static { $node = new static; ...; $node->prop =
        // ...; return $node; }`) populates a *local* variable holding a
        // fresh instance, not `$this`. Collected once per method (not
        // globally) since the same variable name in a different method
        // could hold anything.
        let self_vars = collect_self_instance_vars(m.function.body_stmts(), &c.name);
        body_ever_assigns_property(m.function.body_stmts(), prop, &self_vars)
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
    if class_ever_assigns_property(c, prop) {
        return true;
    }
    let mut current = checker
        .registry
        .classes
        .get(fqn)
        .and_then(|info| info.extends().first().cloned());
    let mut seen = FxHashSet::default();
    while let Some(ancestor) = current {
        if !seen.insert(ancestor.clone()) {
            break; // cycle guard for a malformed/circular hierarchy
        }
        let Some(info) = checker.registry.classes.get(&ancestor) else {
            break;
        };
        let found = info.methods().values().any(|sig| {
            checker.bodies.body(sig.provenance()).is_some_and(|body| {
                let self_vars = collect_self_instance_vars(body, &ancestor);
                body_ever_assigns_property(body, prop, &self_vars)
            })
        });
        if found {
            return true;
        }
        current = info.extends().first().cloned();
    }
    false
}

fn body_ever_assigns_property(stmts: &[Stmt], prop: &str, self_vars: &FxHashSet<String>) -> bool {
    stmts
        .iter()
        .any(|s| stmt_ever_assigns_property(s, prop, self_vars))
}

/// Recurses through every nested statement looking for a
/// `$this->$prop = ...;` (or, through `self_vars`, `$localVar->$prop =
/// ...;` for a variable known to hold a fresh instance of the same
/// class) anywhere at all, however deeply nested and however
/// conditional - deliberately permissive, unlike a real dataflow
/// guarantee. Doesn't descend into a closure/arrow-fn's own body; a
/// property assigned only inside one is an accepted gap.
fn stmt_ever_assigns_property(stmt: &Stmt, prop: &str, self_vars: &FxHashSet<String>) -> bool {
    any_stmt_in_body(
        stmt,
        |s| matches!(&s.kind, StmtKind::Expression(e) if initializes_property(e, prop, self_vars)),
    )
}

fn initializes_property(e: &Expr, prop: &str, self_vars: &FxHashSet<String>) -> bool {
    match &e.kind {
        ExprKind::Assign(lhs, _) | ExprKind::AssignRef(lhs, _) => {
            is_tracked_property(lhs, prop, self_vars)
        }
        // `$this->x ??= $default;` is real PHP's canonical lazy-init
        // idiom for a typed property specifically because `??=`'s
        // left-hand check is `isset()`-based, not a raw read, and
        // `isset()` is exempted from the "must not be accessed before
        // initialization" error a raw read throws.
        ExprKind::CompoundAssign(BinOp::Coalesce, lhs, _) => {
            is_tracked_property(lhs, prop, self_vars)
        }
        _ => false,
    }
}

fn is_tracked_property(e: &Expr, prop: &str, self_vars: &FxHashSet<String>) -> bool {
    let ExprKind::PropertyAccess(base, name, _) = &e.kind else {
        return false;
    };
    if **name != *prop {
        return false;
    }
    match &base.kind {
        ExprKind::Variable(v) => **v == *"this" || self_vars.contains(v.as_ref()),
        _ => false,
    }
}

/// Every local variable name directly assigned `new self(...)`/`new
/// static(...)`/a bare `new <ThisClass>(...)` anywhere in `stmts` -
/// scoped to the single, common `$var = new ...;` expression-statement
/// shape, not e.g. a ternary-wrapped `new`.
fn collect_self_instance_vars(stmts: &[Stmt], class_name: &str) -> FxHashSet<String> {
    let mut vars = FxHashSet::default();
    collect_self_instance_vars_into(stmts, class_name, &mut vars);
    vars
}

fn collect_self_instance_vars_into(stmts: &[Stmt], class_name: &str, vars: &mut FxHashSet<String>) {
    for stmt in stmts {
        any_stmt_in_body(stmt, |s| {
            if let StmtKind::Expression(e) = &s.kind
                && let ExprKind::Assign(lhs, rhs) = &e.kind
                && let ExprKind::Variable(v) = &lhs.kind
                && is_new_self_instance(rhs, class_name)
            {
                vars.insert(v.to_string());
            }
            false
        });
    }
}

fn is_new_self_instance(e: &Expr, class_name: &str) -> bool {
    syntax::stack::guard(|| {
        match &e.kind {
            ExprKind::New(callee, _) => {
                let ExprKind::Name(name) = &callee.kind else {
                    return false;
                };
                let name = name.trim_start_matches('\\');
                name.eq_ignore_ascii_case("self")
                    || name.eq_ignore_ascii_case("static")
                    || name == class_name
                    || name.ends_with(&format!("\\{class_name}"))
            }
            // A chained assignment (`$node = $tag->node = new static;`)
            // whose right-hand side is itself another `Assign`, not a bare
            // `New` - unwrapped recursively until the actual `new` expression
            // at the tail is reached.
            ExprKind::Assign(_, rhs) => is_new_self_instance(rhs, class_name),
            _ => false,
        }
    })
}
