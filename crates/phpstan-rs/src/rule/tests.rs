use super::*;
use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;

/// Flags every bare-variable expression named `$flag_me`, regardless of
/// its own inferred type - just enough to prove `wants_expr`/
/// `check_expr` and the diagnostic actually reaching `Checker::
/// into_diagnostics` end to end, without depending on any real check's
/// own semantics.
struct FlagsVariableNamed(&'static str);

impl Rule for FlagsVariableNamed {
    fn wants_expr(&self, kind: &ExprKind) -> bool {
        matches!(kind, ExprKind::Variable(name) if name.as_ref() == self.0)
    }

    fn check_expr(&self, _scope: &Scope, expr: &Expr, ty: &Type) -> Vec<Diagnostic> {
        vec![
            DiagnosticBuilder::new(
                format!("flagged ${} (inferred {ty})", self.0),
                expr.span,
                6,
                "test.flagged"
            )
            .build(&syntax::span::LineIndex::default());
            1
        ]
    }
}

/// Flags every `echo` statement.
struct FlagsEchoStatements;

impl Rule for FlagsEchoStatements {
    fn wants_stmt(&self, kind: &StmtKind) -> bool {
        matches!(kind, StmtKind::Echo(_))
    }

    fn check_stmt(&self, _scope: &Scope, stmt: &Stmt) -> Vec<Diagnostic> {
        vec![
            DiagnosticBuilder::new(
                "flagged an echo".to_string(),
                stmt.span,
                6,
                "test.echoFlagged",
            )
            .build(&syntax::span::LineIndex::default()),
        ]
    }
}

fn diagnostics_with(src: &str, rules: &[Box<dyn Rule>]) -> Vec<Diagnostic> {
    let path = std::path::Path::new("test.php");
    let mut project = crate::project::Project::new();
    project.load_analyzed_file(path.to_path_buf(), src).unwrap();
    project.finish_loading();
    let program = project.ast(path).unwrap();
    let mut checker = Checker::new(project.registry());
    checker.set_external_rules(rules);
    checker.check_program(program);
    checker.into_diagnostics()
}

#[test]
fn an_external_rule_fires_only_for_its_own_wanted_variable_name() {
    let rules: Vec<Box<dyn Rule>> = vec![Box::new(FlagsVariableNamed("flag_me"))];
    // An assignment target alone (`$flag_me = 1;`) never reaches an
    // ordinary `check_expr` recursion the way a real *read* does - a
    // separate `$other = $flag_me;` read is what actually exercises the
    // dispatch mechanism this test means to prove.
    let diagnostics = diagnostics_with(
        "<?php function f() { $flag_me = 1; $other = $flag_me; }",
        &rules,
    );
    let flagged: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.identifier == "test.flagged")
        .collect();
    assert_eq!(flagged.len(), 1, "got: {diagnostics:?}");
    assert!(flagged[0].message.contains("flag_me"));
}

#[test]
fn an_external_rule_never_fires_when_the_wanted_variable_never_appears() {
    let rules: Vec<Box<dyn Rule>> = vec![Box::new(FlagsVariableNamed("flag_me"))];
    let diagnostics = diagnostics_with("<?php function f() { $other = 2; }", &rules);
    assert!(!diagnostics.iter().any(|d| d.identifier == "test.flagged"));
}

#[test]
fn a_stmt_level_external_rule_fires_per_echo() {
    let rules: Vec<Box<dyn Rule>> = vec![Box::new(FlagsEchoStatements)];
    let diagnostics = diagnostics_with("<?php echo 1; echo 2; $x = 3;", &rules);
    let flagged: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.identifier == "test.echoFlagged")
        .collect();
    assert_eq!(flagged.len(), 2, "got: {diagnostics:?}");
}

#[test]
fn no_external_rules_registered_means_no_dispatch_overhead_or_diagnostics() {
    // The default, empty-slice `Checker::new` path - every one of this
    // project's own first-party call sites.
    let diagnostics = diagnostics_with("<?php function f() { $x = 1; }", &[]);
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.identifier.starts_with("test."))
    );
}

#[test]
fn scope_exposes_the_current_class_and_method_and_the_variable_s_own_local_type() {
    // A `Rule` has no way to hand a value back except through its own
    // `Vec<Diagnostic>` return - encode what `Scope` reported directly
    // into the message text, then assert on that.
    struct RecordsScopeAsDiagnostic;

    impl Rule for RecordsScopeAsDiagnostic {
        fn wants_expr(&self, kind: &ExprKind) -> bool {
            matches!(kind, ExprKind::Variable(name) if name.as_ref() == "x")
        }

        fn check_expr(&self, scope: &Scope, expr: &Expr, _ty: &Type) -> Vec<Diagnostic> {
            vec![
                DiagnosticBuilder::new(
                    format!(
                        "class={:?} method={:?} local_type={:?}",
                        scope.current_class(),
                        scope.current_method(),
                        scope.local_type("x")
                    ),
                    expr.span,
                    6,
                    "test.scope",
                )
                .build(&syntax::span::LineIndex::default()),
            ]
        }
    }

    let rules: Vec<Box<dyn Rule>> = vec![Box::new(RecordsScopeAsDiagnostic)];
    let diagnostics = diagnostics_with(
        "<?php class Foo { function bar() { $x = 1; return $x; } }",
        &rules,
    );
    let scope_diagnostics: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.identifier == "test.scope")
        .collect();
    assert_eq!(scope_diagnostics.len(), 1, "got: {diagnostics:?}");
    assert!(scope_diagnostics[0].message.contains("class=Some(\"Foo\")"));
    assert!(
        scope_diagnostics[0]
            .message
            .contains("method=Some(\"bar\")")
    );
    assert!(scope_diagnostics[0].message.contains("local_type=Some"));
}

#[test]
fn is_function_guarded_reflects_a_real_function_exists_narrowing() {
    struct ChecksGuard;

    impl Rule for ChecksGuard {
        fn wants_expr(&self, kind: &ExprKind) -> bool {
            matches!(kind, ExprKind::Call(callee, _)
                if matches!(&callee.kind, ExprKind::Name(n) if n == "maybe_missing"))
        }

        fn check_expr(&self, scope: &Scope, expr: &Expr, _ty: &Type) -> Vec<Diagnostic> {
            vec![
                DiagnosticBuilder::new(
                    format!("guarded={}", scope.is_function_guarded("maybe_missing")),
                    expr.span,
                    6,
                    "test.guard",
                )
                .build(&syntax::span::LineIndex::default()),
            ]
        }
    }

    let rules: Vec<Box<dyn Rule>> = vec![Box::new(ChecksGuard)];
    let diagnostics = diagnostics_with(
        "<?php if (function_exists('maybe_missing')) { maybe_missing(); }",
        &rules,
    );
    let guard_diagnostics: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.identifier == "test.guard")
        .collect();
    assert_eq!(guard_diagnostics.len(), 1, "got: {diagnostics:?}");
    assert!(guard_diagnostics[0].message.contains("guarded=true"));
}
