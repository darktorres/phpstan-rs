use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::checker::{Checker, Locals, union_types};
use crate::registry::Registry;
use crate::types::Type;

impl<'a> Checker<'a> {
    /// The classic guard clause (`if ($x === null) { return; }`, or the
    /// less obvious `if (! $x instanceof Foo) { $this->fatalError(...); }`)
    /// depends on recognizing a branch that never falls through, so its
    /// (narrowed) scope doesn't incorrectly leak past the `if`. A bare
    /// call expression counts too, not just `return`/`throw`/`break`/
    /// `continue`, when its own return type is `never` - a real, common
    /// pattern (`private function fatalError(string $msg): never`) that a
    /// purely syntactic check would miss entirely.
    pub(crate) fn always_terminates(&self, stmt: &Stmt, locals: &Locals) -> bool {
        match &stmt.kind {
            StmtKind::Return(_)
            | StmtKind::Throw(_)
            | StmtKind::Break(_)
            | StmtKind::Continue(_) => true,
            StmtKind::Expression(e) => self.infer_expr(e, locals) == Type::Never,
            StmtKind::Block(stmts) => stmts
                .last()
                .is_some_and(|s| self.always_terminates(s, locals)),
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.always_terminates(then_branch, locals)
                    && else_branch
                        .as_deref()
                        .is_some_and(|e| self.always_terminates(e, locals))
            }
            // A `switch` guarantees ending control flow only when there's
            // a `default` (otherwise the subject might match nothing and
            // fall straight through, doing nothing) and every non-empty
            // case body genuinely terminates - not just `break`s, which
            // only exits the switch itself and falls through to whatever
            // follows it, not out of the enclosing function. An empty
            // body (`case a: case b: ...`, a pure fallthrough label) is
            // skipped rather than required to independently terminate -
            // only the eventual non-empty body it falls into needs to.
            StmtKind::Switch { cases, .. } => {
                cases.iter().any(|c| c.test.is_none())
                    && cases.iter().all(|c| {
                        c.body.is_empty()
                            || c.body.last().is_some_and(|s| {
                                !matches!(s.kind, StmtKind::Break(_))
                                    && self.always_terminates(s, locals)
                            })
                    })
            }
            // A `try` guarantees ending control flow when its own body
            // does (whether by returning normally or by throwing - an
            // uncaught throw propagates out of the function just as
            // surely as a caught one returns from a catch block) *and*
            // every one of its `catch` clauses also does. An empty
            // `catches` list (a bare `try { ... } finally { ... }`)
            // vacuously satisfies the second half - nothing can catch and
            // fail to terminate if nothing catches at all. `finally`
            // itself doesn't need to independently terminate for this
            // question - it always runs before whatever the try/catch
            // already decided.
            StmtKind::Try { body, catches, .. } => {
                body.last()
                    .is_some_and(|s| self.always_terminates(s, locals))
                    && catches.iter().all(|c| {
                        c.body
                            .last()
                            .is_some_and(|s| self.always_terminates(s, locals))
                    })
            }
            // A round of Piwigo-corpus bucketing found `while (true) {
            // ...; if (...) { return $x; } }` (real shape,
            // `NotificationController::findAvailableFeedId`, a
            // find-a-free-key retry loop) flagged as possibly falling
            // off the function's own end - a literal `true`/non-zero
            // condition never lets the loop exit by itself, so the only
            // way past it at all is whatever `return`/`throw` inside
            // its body runs, exactly what `always_terminates` already
            // exists to recognize for every other construct.
            // Deliberately conservative about `break`: any `Break`
            // statement *anywhere* in the body - even one that actually
            // targets a different, nested loop/switch - counts as "this
            // loop might exit on its own," never wrongly concluding
            // termination. `goto`/`label` share the same theoretical
            // gap `always_terminates` already has everywhere else (see
            // Phase 15's own note on this).
            StmtKind::While { cond, body } => is_truthy_literal(cond) && !stmt_contains_break(body),
            // Same idiom, `for`'s own real spelling: `for ($i = 0; true;
            // $i++) { ... }` (real shape, `HttpClientService::
            // guardedRequest`, a redirect-following retry loop) - a
            // `for` with no condition at all (`for (;;)`) is PHP's own
            // "no condition means always true," same real semantics as
            // an explicit truthy literal.
            StmtKind::For { cond, body, .. } => {
                cond.last().is_none_or(is_truthy_literal) && !stmt_contains_break(body)
            }
            _ => false,
        }
    }

    /// Whether `stmt` exits the whole enclosing function or loop
    /// (`return`/`throw`/`continue`, or a nested construct that
    /// guarantees one of those) rather than merely exiting a `switch`
    /// itself (`break`, or not transferring control at all). Distinct
    /// from `always_terminates`'s own notion of "doesn't fall through to
    /// the next case" (which correctly treats `break` as terminating
    /// *that* question) - a switch's own post-switch scope merge needs
    /// this different distinction instead: a case ending in `break`
    /// still reaches the code after the switch, one ending in
    /// `return`/`throw`/`continue` never does.
    pub(crate) fn exits_function_or_loop(&self, stmt: &Stmt, locals: &Locals) -> bool {
        match &stmt.kind {
            StmtKind::Return(_) | StmtKind::Throw(_) | StmtKind::Continue(_) => true,
            StmtKind::Break(_) => false,
            StmtKind::Expression(e) => self.infer_expr(e, locals) == Type::Never,
            StmtKind::Block(stmts) => stmts
                .last()
                .is_some_and(|s| self.exits_function_or_loop(s, locals)),
            StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.exits_function_or_loop(then_branch, locals)
                    && else_branch
                        .as_deref()
                        .is_some_and(|e| self.exits_function_or_loop(e, locals))
            }
            _ => false,
        }
    }
}

/// Joins two branch-local scopes back into one after an `if`/`while`. A
/// variable present in both branches gets the union of its two types; one
/// present in only one branch was introduced there and can't be assumed to
/// exist afterward, so it's dropped rather than carried forward unsoundly.
pub(crate) fn merge_scopes(then_locals: &Locals, else_locals: &Locals) -> Locals {
    let mut merged = Locals::default();
    for (name, then_ty) in then_locals {
        if let Some(else_ty) = else_locals.get(name) {
            merged.insert(name.clone(), union_types(then_ty.clone(), else_ty.clone()));
        }
    }
    merged
}

/// Does this function/method/closure body contain a `yield`/`yield from`
/// anywhere, making it a real PHP generator? A generator always returns a
/// real `Generator` object when called, regardless of whether it has an
/// explicit `return` - `return;`/`return $x;` *inside* it sets
/// `Generator::getReturn()`'s own value, not the enclosing function's
/// return value, so real PHPStan never checks either against the
/// function's own declared `Generator`/`Iterator`/`Traversable` return
/// type (only against a `@return Generator<...,...,...,TReturn>`
/// docblock's 4th argument, if present - not modeled here, no corpus
/// evidence for it).
pub(crate) fn body_contains_yield(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_contains_yield)
}

/// Phase 70: whether `ty` is a real PHP generator-compatible return type,
/// mirroring real PHP's own engine-level check exactly (confirmed
/// directly via `php`/`php -l`), which only needs *one* member of a
/// union to be `Traversable`-compatible, not all of them (`int|Generator`
/// is genuinely accepted by real PHP, however misleading, and `iterable`
/// itself is `array|Traversable` - the `array` half alone would
/// otherwise wrongly sink the whole check). `Mixed` (no useful
/// information either way) is treated as always compatible, matching
/// this project's usual "don't guess against yourself" leniency for an
/// unresolved/undeclared type.
pub(crate) fn type_allows_generator_return(ty: &Type, registry: &Registry) -> bool {
    match ty {
        Type::Mixed => true,
        Type::Object(name, _) => registry.is_subclass_of(name, "Traversable"),
        Type::Union(members) | Type::Intersection(members) => members
            .iter()
            .any(|m| type_allows_generator_return(m, registry)),
        _ => false,
    }
}

/// Phase 70: an arrow function's own body is a single expression, not a
/// `Vec<Stmt>` `body_contains_yield` above can walk - this is that same
/// walk's expression-only counterpart (`expr_children` already stops at
/// a nested `Closure`/`ArrowFn` boundary, the correct semantics here
/// too: a `yield` inside a nested one doesn't make *this* arrow function
/// a generator).
pub(crate) fn expr_contains_yield(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Yield(..) | ExprKind::YieldFrom(_))
        || expr_children(e).into_iter().any(expr_contains_yield)
}

/// Mirrors `deps::collect_stmt`'s own exhaustive per-variant shape: each
/// `StmtKind` variant's own `Expr`-bearing field(s), checked via
/// `expr_contains_yield`, plus recursing into `stmt_children` for nested
/// statements. `stmt_children` (and `expr_children`, transitively)
/// already stop at `Closure`/`ArrowFn`/`NewAnonymousClass`/`FunctionDecl`/
/// `ClassDecl`/`EnumDecl` boundaries by design - exactly the correct
/// semantics here too, since a `yield` inside any of those nested scopes
/// does NOT make the *outer* function a generator.
fn stmt_contains_yield(stmt: &Stmt) -> bool {
    let own = match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => expr_contains_yield(e),
        StmtKind::Return(Some(e)) => expr_contains_yield(e),
        StmtKind::Return(None) => false,
        StmtKind::Echo(exprs) => exprs.iter().any(expr_contains_yield),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => expr_contains_yield(cond),
        StmtKind::For {
            init, cond, step, ..
        } => init.iter().chain(cond).chain(step).any(expr_contains_yield),
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            expr_contains_yield(subject)
                || key.as_ref().is_some_and(expr_contains_yield)
                || expr_contains_yield(value)
        }
        StmtKind::GlobalConst(consts) => consts.iter().any(|c| expr_contains_yield(&c.value)),
        StmtKind::StaticVar(vars) => vars
            .iter()
            .any(|(_, d)| d.as_ref().is_some_and(expr_contains_yield)),
        StmtKind::Switch { subject, cases } => {
            expr_contains_yield(subject)
                || cases
                    .iter()
                    .any(|c| c.test.as_ref().is_some_and(expr_contains_yield))
        }
        _ => false,
    };
    own || stmt_children(stmt).into_iter().any(stmt_contains_yield)
}

/// Real PHP's own two spellings for "this condition is always true":
/// the literal `true`, and any non-zero integer literal (`while (1)`,
/// a real, common pre-`true`-keyword-idiom carryover). Deliberately not
/// attempting anything symbolic beyond a bare literal - a variable or
/// expression that merely *evaluates* to a truthy constant isn't
/// something this checker tracks precisely enough to trust here.
fn is_truthy_literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Literal(Literal::Bool(true)) => true,
        ExprKind::Literal(Literal::Int(n)) => *n != 0,
        _ => false,
    }
}

/// Whether `stmt` contains a `break` anywhere in its own subtree,
/// including inside a nested loop/switch that statement doesn't
/// unqualified-`break` out of at all - deliberately conservative in the
/// same direction `always_terminates`'s own `while (true)` case needs:
/// treating any `break` found anywhere as "this loop might exit on its
/// own" only ever causes a missed simplification, never a wrong
/// termination conclusion, so there's no need to track which specific
/// loop/switch a given `break`'s own level actually targets.
fn stmt_contains_break(stmt: &Stmt) -> bool {
    matches!(stmt.kind, StmtKind::Break(_))
        || stmt_children(stmt).into_iter().any(stmt_contains_break)
}
