//! PHPUnit's own `--fail-on-risky` flags a test method that performed
//! zero assertions - a real, common mistake (a copy-pasted test body
//! whose assertion line got deleted, a test that only exercises a
//! `void` method and forgot to check anything about it) that PHPUnit
//! itself only catches at runtime; neither real PHPStan ruleset has any
//! static equivalent to it at all. `test.risky` is this project's own
//! identifier, not a real-PHPStan one - the phase this shipped under
//! confirmed no such identifier exists anywhere in `vendor/phpstan-src`
//! or `vendor/phpstan-strict-rules`.
//!
//! Scope, disclosed: a test method is recognized only by PHPUnit's own
//! plain `test`-prefix naming convention (`#[Test]`/`@test`-annotated
//! methods with a different name are not recognized) - the
//! overwhelming common real-world shape, and the one this project's
//! own corpus exclusively uses.

use crate::checker::Checker;
use syntax::ast::*;
use syntax::token::Span;
use syntax::visit::{any_child_expr, any_child_stmt};

impl<'a> Checker<'a> {
    /// `fqn`'s own test methods, each checked independently - only fires
    /// for a real `PHPUnit\Framework\TestCase` subclass at all (a plain
    /// class with a `test*`-named method that happens not to extend it
    /// is just an ordinary, unrelated method, not a test).
    pub(crate) fn check_risky_test_methods(&mut self, fqn: &str, c: &ClassDecl) {
        if !self
            .registry
            .is_subclass_of(fqn, "PHPUnit\\Framework\\TestCase")
        {
            return;
        }
        for m in &c.methods {
            let f = &m.function;
            if m.is_abstract || !f.name.starts_with("test") {
                continue;
            }
            if body_has_assertion(&f.body) {
                continue;
            }
            self.report(
                fqn.to_string(),
                format!(
                    "Test {fqn}::{}() has no assertions and no expectException/markTestSkipped/markTestIncomplete/addToAssertionCount call - it will be reported risky by PHPUnit at runtime.",
                    f.name
                ),
                f.name_span,
                4,
                "test.risky",
            );
        }
    }

    /// Same idea, for a Pest `it('...', function () { ... })`/
    /// `test('...', function () { ... })` test body - Pest's own
    /// `expect(...)` expectation API stands in for PHPUnit's `assert*`
    /// family here (a real Pest test can freely mix both, since it
    /// still runs inside a real, bound `TestCase` instance - both are
    /// recognized by `body_has_assertion`).  Called from the same
    /// `ExprKind::Call` site `pest_config::is_this_binding_call`'s
    /// `$this`-rebinding special case already recognizes, scoped further
    /// here to just the two real test-body functions (`describe`/
    /// `beforeEach`/etc. are structural grouping/hooks, never a test
    /// body of their own).
    pub(crate) fn check_pest_risky_test(
        &mut self,
        name: &str,
        closure: &Expr,
        span: Span,
        label: &str,
    ) {
        if name != "it" && name != "test" {
            return;
        }
        let has_assertion = match &closure.kind {
            ExprKind::Closure { body, .. } => body_has_assertion(body),
            ExprKind::ArrowFn { body, .. } => expr_has_assertion(body),
            _ => return,
        };
        if has_assertion {
            return;
        }
        self.report(
            label.to_string(),
            "Test has no assertions and no expectException/markTestSkipped/markTestIncomplete/addToAssertionCount call - it will be reported risky by PHPUnit at runtime.".to_string(),
            span,
            4,
            "test.risky",
        );
    }
}

fn is_assertion_method_name(name: &str) -> bool {
    name.starts_with("assert")
        || name.starts_with("expectException")
        || name == "markTestSkipped"
        || name == "markTestIncomplete"
        || name == "addToAssertionCount"
}

fn expr_has_assertion(e: &Expr) -> bool {
    let own = match &e.kind {
        ExprKind::MethodCall(_, method, _, _) => is_assertion_method_name(method),
        ExprKind::StaticCall(_, method, _) => is_assertion_method_name(method),
        // Pest's own `expect($x)->toBe($y)` - the mere presence of a
        // call to the global `expect()` function is already a strong
        // enough signal on its own (real Pest usage never calls it
        // without immediately chaining an expectation off the result),
        // without needing to also verify the chained `->to*()` call.
        ExprKind::Call(callee, _) => {
            matches!(&callee.kind, ExprKind::Name(n) if n.trim_start_matches('\\') == "expect")
        }
        _ => false,
    };
    own || any_child_expr(e, &mut expr_has_assertion)
}

/// Mirrors `control_flow::generators::stmt_contains_yield`'s own
/// exhaustive per-`StmtKind`-variant shape (that function's own doc
/// comment has the full reasoning) - each variant's own `Expr`-bearing
/// field(s), checked via `expr_has_assertion`, plus `any_child_stmt` for
/// nested statements. Both `any_child_stmt`/`any_child_expr` stop at a
/// nested `Closure`/`ArrowFn` boundary by design - correct here too: an
/// assertion inside a nested callback (`array_map(fn ($x) => $this->
/// assertTrue($x), $items)` is a real, if unusual, exception) still
/// needs to count for *this* test, so nested-closure bodies are walked
/// by `any_child_expr` exactly like any other child expression - the
/// boundary only matters for `body_contains_yield`'s different question
/// ("is the *enclosing* function itself a generator"), not this one.
fn stmt_has_assertion(stmt: &Stmt) -> bool {
    let own = match &stmt.kind {
        StmtKind::Expression(e) | StmtKind::Throw(e) => expr_has_assertion(e),
        StmtKind::Return(Some(e)) => expr_has_assertion(e),
        StmtKind::Return(None) => false,
        StmtKind::Echo(exprs) => exprs.iter().any(expr_has_assertion),
        StmtKind::If { cond, .. }
        | StmtKind::While { cond, .. }
        | StmtKind::DoWhile { cond, .. } => expr_has_assertion(cond),
        StmtKind::For {
            init, cond, step, ..
        } => init.iter().chain(cond).chain(step).any(expr_has_assertion),
        StmtKind::Foreach {
            subject,
            key,
            value,
            ..
        } => {
            expr_has_assertion(subject)
                || key.as_ref().is_some_and(expr_has_assertion)
                || expr_has_assertion(value)
        }
        StmtKind::GlobalConst(consts) => consts.iter().any(|c| expr_has_assertion(&c.value)),
        StmtKind::StaticVar(vars) => vars
            .iter()
            .any(|(_, d)| d.as_ref().is_some_and(expr_has_assertion)),
        StmtKind::Switch { subject, cases } => {
            expr_has_assertion(subject)
                || cases
                    .iter()
                    .any(|c| c.test.as_ref().is_some_and(expr_has_assertion))
        }
        _ => false,
    };
    own || any_child_stmt(stmt, &mut stmt_has_assertion)
}

fn body_has_assertion(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_has_assertion)
}
