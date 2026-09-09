//! Phase 62: `UninitializedPropertyRule`'s own core case - a typed
//! property with no default value that nothing anywhere in the class's
//! own body ever assigns. Real PHP throws `Error: Typed property $x
//! must not be accessed before initialization` the first time such a
//! property is read.
//!
//! Deliberately narrower than real PHPStan's own `ConstructorsHelper`-
//! backed dataflow, which proves a property is initialized *before its
//! first real read* (tracking exactly which methods a constructor calls,
//! and in what order). This project settles for a much cheaper, more
//! permissive question instead - "is this property assigned *anywhere at
//! all* in the class's own body, by any method" - deliberately trading
//! away catching the harder cases (a constructor that forgets to call
//! its own initializer, a property only initialized on an unreachable
//! path) for avoiding a real, corpus-confirmed false-positive class: a
//! property meant to be set by a separate "handler"/"reset" method
//! called sometime *after* construction (real shape: amphp's own
//! `Http1Driver::$client`, assigned only in `handleClient()`, guarded by
//! its own `assert(!isset($this->client))` - real PHP semantics confirm
//! `isset()` on an uninitialized typed property returns `false` rather
//! than throwing, so this is genuinely safe, intentional code, not a
//! bug). A first attempt here required the constructor specifically to
//! guarantee it, which flagged that shape and others like it.
//!
//! Real PHPStan itself doesn't trust `UninitializedPropertyRule` enough
//! to run it by default at *any* level - it sits behind its own opt-in
//! `checkUninitializedProperties` parameter (`false` in PHPStan's own
//! `conf/config.neon`). This project's version, being a considerably
//! simpler heuristic than PHPStan's real dataflow, mirrors that default
//! exactly (`Registry::check_uninitialized_properties`, wired through
//! `Project::set_check_uninitialized_properties` the same way
//! `strict_mixed_arrays` is) rather than guessing at a precision this
//! project can't independently verify matches PHPStan's own bar for
//! "safe to run unconditionally."

use std::collections::HashSet;

use crate::ast::*;
use crate::ast_walk::stmt_children;
use crate::checker::Checker;

impl<'a> Checker<'a> {
    /// Flags `property.uninitialized` for a concrete class's own typed,
    /// non-static, non-readonly, no-default, no-hook property that no
    /// method anywhere in the class body ever assigns.
    ///
    /// A readonly property is excluded on purpose, matching real
    /// PHPStan's own `UninitializedPropertyRule` exactly - readonly's
    /// own "assign exactly once, only from inside the constructor"
    /// contract is a distinct check (`property.readOnlyAssignNotInConstructor`,
    /// already implemented) with its own, different failure mode; a
    /// readonly property doing neither isn't this rule's concern.
    /// Abstract classes are skipped entirely - a common, legitimate
    /// pattern relies on a concrete subclass's own constructor to
    /// finish initialization the abstract class's own can't (or
    /// doesn't) do, which this project has no way to verify happens on
    /// every real subclass.
    pub(crate) fn check_uninitialized_properties(&mut self, fqn: &str, c: &ClassDecl) {
        if !self.registry.check_uninitialized_properties {
            return;
        }
        if c.kind != ClassKind::Class || c.is_abstract {
            return;
        }
        for p in &c.properties {
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
            self.report(
                fqn.to_string(),
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

pub(super) fn class_ever_assigns_property(c: &ClassDecl, prop: &str) -> bool {
    c.methods.iter().any(|m| {
        // Phase 62 corpus find: a static "named constructor" (`public
        // static function create(...): static { $node = new static;
        // ...; $node->prop = ...; return $node; }`, real shape: Latte's
        // own `IncludeFileNode::create()`) populates a *local* variable
        // holding a fresh instance, not `$this` - a real, common PHP
        // OOP idiom this project's own corpus is full of. Collected
        // once per method (not globally) since the same variable name
        // in a different method could hold anything.
        let self_vars = collect_self_instance_vars(&m.function.body, &c.name);
        body_ever_assigns_property(&m.function.body, prop, &self_vars)
    })
}

/// Whether `prop` is assigned *anywhere* reachable from `fqn`'s own
/// hierarchy - `class_ever_assigns_property` alone only ever looks at
/// `c`'s own directly-declared methods, which misses a real, disclosed
/// shape: a property re-declared across an inheritance chain (common
/// when a subclass narrows a parent's own property type), initialized
/// only by an *ancestor's* own constructor logic (`parent::__construct()`
/// absorption's own motivating case - see the Phase 62 plan's "Cross-
/// method absorption" section). A genuine leniency fix, not a narrowing:
/// this can only make the existing "never initialized" verdict *less*
/// eager to flag, matching this whole check's own already-established
/// permissive direction. Ancestors are walked via the registry
/// (`FunctionSig::body`), not AST - the current file may not even
/// contain them. Deliberately doesn't walk *downward* into a subclass's
/// own overrides (an unbounded, open-hierarchy question a single-class
/// check can't answer at all - real PHPStan's own abstract-class
/// exemption exists for the same reason, just for the narrower case
/// where the base class can't be instantiated on its own to begin with).
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
        .and_then(|info| info.extends.first().cloned());
    let mut seen = HashSet::new();
    while let Some(ancestor) = current {
        if !seen.insert(ancestor.clone()) {
            break; // cycle guard for a malformed/circular hierarchy
        }
        let Some(info) = checker.registry.classes.get(&ancestor) else {
            break;
        };
        let found = info.methods.values().any(|sig| {
            sig.body.as_ref().is_some_and(|body| {
                let self_vars = collect_self_instance_vars(body, &ancestor);
                body_ever_assigns_property(body, prop, &self_vars)
            })
        });
        if found {
            return true;
        }
        current = info.extends.first().cloned();
    }
    false
}

fn body_ever_assigns_property(stmts: &[Stmt], prop: &str, self_vars: &HashSet<String>) -> bool {
    stmts
        .iter()
        .any(|s| stmt_ever_assigns_property(s, prop, self_vars))
}

/// Recurses through every nested statement (`stmt_children` - every
/// `if`/`while`/`for`/`foreach`/`try`/`switch`/block body, however
/// deeply nested and however conditional) looking for a single
/// unconditional-shaped `$this->$prop = ...;` (or, through `self_vars`,
/// `$localVar->$prop = ...;` for a variable known to hold a fresh
/// instance of the same class) anywhere at all - deliberately
/// permissive, unlike a real dataflow guarantee, per this module's own
/// doc comment. Doesn't descend into a closure/arrow-fn's own body
/// (`stmt_children`'s documented boundary) - a property assigned *only*
/// inside one is a real, rarer gap this project accepts rather than
/// builds around, matching the same boundary this project's other
/// statement-tree walkers (e.g. `body_contains_yield`) already draw.
fn stmt_ever_assigns_property(stmt: &Stmt, prop: &str, self_vars: &HashSet<String>) -> bool {
    let own =
        matches!(&stmt.kind, StmtKind::Expression(e) if initializes_property(e, prop, self_vars));
    own || stmt_children(stmt)
        .into_iter()
        .any(|s| stmt_ever_assigns_property(s, prop, self_vars))
}

fn initializes_property(e: &Expr, prop: &str, self_vars: &HashSet<String>) -> bool {
    match &e.kind {
        ExprKind::Assign(lhs, _) => is_tracked_property(lhs, prop, self_vars),
        // `$this->x ??= $default;` is real PHP's own canonical lazy-init
        // idiom for a typed property specifically *because* `??=`'s own
        // left-hand check is `isset()`-based, not a raw read - and
        // `isset()` is specially exempted from the "must not be accessed
        // before initialization" error a raw read of an uninitialized
        // typed property throws, unlike every *other* compound operator
        // (confirmed directly: `php` on a minimal repro genuinely
        // completes and returns the default, no error). Real corpus
        // shape: Symfony's own `SymfonyStyle::$questionHelper`, never
        // touched anywhere except `$this->questionHelper ??= new
        // SymfonyQuestionHelper(...)`.
        ExprKind::CompoundAssign(BinOp::Coalesce, lhs, _) => {
            is_tracked_property(lhs, prop, self_vars)
        }
        _ => false,
    }
}

fn is_tracked_property(e: &Expr, prop: &str, self_vars: &HashSet<String>) -> bool {
    let ExprKind::PropertyAccess(base, name, _) = &e.kind else {
        return false;
    };
    if name != prop {
        return false;
    }
    match &base.kind {
        ExprKind::Variable(v) => v == "this" || self_vars.contains(v),
        _ => false,
    }
}

/// Every local variable name directly assigned `new self(...)`/`new
/// static(...)`/a bare `new <ThisClass>(...)` anywhere in `stmts` -
/// scoped to the single, common `$var = new ...;` expression-statement
/// shape (not e.g. a ternary-wrapped `new`), matching this whole
/// module's own "don't guess past the common, provable case" scope.
fn collect_self_instance_vars(stmts: &[Stmt], class_name: &str) -> HashSet<String> {
    let mut vars = HashSet::new();
    collect_self_instance_vars_into(stmts, class_name, &mut vars);
    vars
}

fn collect_self_instance_vars_into(stmts: &[Stmt], class_name: &str, vars: &mut HashSet<String>) {
    for s in stmts {
        if let StmtKind::Expression(e) = &s.kind
            && let ExprKind::Assign(lhs, rhs) = &e.kind
            && let ExprKind::Variable(v) = &lhs.kind
            && is_new_self_instance(rhs, class_name)
        {
            vars.insert(v.clone());
        }
        for child in stmt_children(s) {
            collect_self_instance_vars_into(std::slice::from_ref(child), class_name, vars);
        }
    }
}

fn is_new_self_instance(e: &Expr, class_name: &str) -> bool {
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
        // A real corpus shape (Latte's own `IterateWhileNode::create()`):
        // `$node = $tag->node = new static;` - a chained assignment,
        // whose own right-hand side is itself another `Assign`, not a
        // bare `New` - unwrapped recursively until the actual `new`
        // expression at the tail is reached.
        ExprKind::Assign(_, rhs) => is_new_self_instance(rhs, class_name),
        _ => false,
    }
}
