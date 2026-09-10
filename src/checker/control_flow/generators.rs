use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::registry::Registry;
use crate::types::Type;

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
