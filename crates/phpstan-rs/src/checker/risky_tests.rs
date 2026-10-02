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
use syntax::visit::{Visitor, walk_expr, walk_stmts};

impl<'a> Checker<'a> {
    /// `fqn`'s own test methods, each checked independently - only fires
    /// for a real `PHPUnit\Framework\TestCase` subclass at all (a plain
    /// class with a `test*`-named method that happens not to extend it
    /// is just an ordinary, unrelated method, not a test).
    pub(crate) fn check_risky_test_methods(&mut self, fqn: &str, c: &ClassDef) {
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
            if body_has_assertion(f.body_stmts()) {
                continue;
            }
            self.report(format!(
                    "Test {fqn}::{}() has no assertions and no expectException/markTestSkipped/markTestIncomplete/addToAssertionCount call - it will be reported risky by PHPUnit at runtime.",
                    f.name
                ),
                f.name.span,
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
    pub(crate) fn check_pest_risky_test(&mut self, name: &str, closure: &Expr, span: Span) {
        if name != "it" && name != "test" {
            return;
        }
        let has_assertion = match &closure.kind {
            ExprKind::Closure(closure) => body_has_assertion(&closure.body),
            ExprKind::ArrowFn(arrow) => expr_has_assertion(&arrow.body),
            _ => return,
        };
        if has_assertion {
            return;
        }
        self.report("Test has no assertions and no expectException/markTestSkipped/markTestIncomplete/addToAssertionCount call - it will be reported risky by PHPUnit at runtime.".to_string(),
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

/// Whether `e` is itself an assertion call.
fn is_assertion(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::MethodCall(_, method, _, _) => is_assertion_method_name(method),
        ExprKind::StaticCall(_, method, _) => is_assertion_method_name(method),
        // Pest's own `expect($x)->toBe($y)` - the mere presence of a
        // call to the global `expect()` function is already a strong
        // enough signal on its own (real Pest usage never calls it
        // without immediately chaining an expectation off the result),
        // without needing to also verify the chained `->to*()` call.
        ExprKind::Call(callee, _) => matches!(&callee.kind, ExprKind::Name(n) if {
            let (primary, fallback) = n.candidates();
            fallback.unwrap_or(primary) == "expect"
        }),
        _ => false,
    }
}

/// Finds an assertion anywhere in a test body, a nested callback included
/// (`array_map(fn ($x) => $this->assertTrue($x), $items)`): it runs as
/// part of the test. A nested named function or class declaration is not
/// the test's own code.
#[derive(Default)]
struct Assertions {
    found: bool,
}

impl<'ast> Visitor<'ast> for Assertions {
    fn visit_function_decl(&mut self, _: &'ast FunctionDecl) {}

    fn visit_class_def(&mut self, _: &'ast ClassDef) {}

    fn visit_expr(&mut self, e: &'ast Expr) {
        if self.found || is_assertion(e) {
            self.found = true;
            return;
        }
        walk_expr(self, e);
    }
}

fn expr_has_assertion(e: &Expr) -> bool {
    let mut search = Assertions::default();
    search.visit_expr(e);
    search.found
}

fn body_has_assertion(stmts: &[Stmt]) -> bool {
    let mut search = Assertions::default();
    walk_stmts(&mut search, stmts);
    search.found
}
