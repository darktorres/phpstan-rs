//! Parsing of string literals and interpolation, including complex interpolation expressions.

use super::super::*;
use super::helpers::*;

#[test]
fn shell_exec_backtick_string_parses() {
    let program = parse(r#"<?php $x = `echo -n ""`;"#);
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected assignment");
    };
    let ExprKind::Call(callee, _) = &rhs.kind else {
        panic!("expected a call");
    };
    assert!(matches!(&callee.kind, ExprKind::Name(n) if n == "shell_exec"));
}

// Finding 11 from the architecture audit - every diagnostic inside a
// {$...} string-interpolation expression reported the wrong source
// position (the throwaway sub-parse's own fresh line-1-col-1
// coordinates, never biased into the real file's coordinates).

#[test]
fn complex_interpolation_expression_spans_are_biased_to_real_file_positions() {
    let program = parse("<?php\nfunction f() {\n    return \"value: {$undefinedThing}\";\n}\n");
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected a function declaration, got {:?}", program[0].kind);
    };
    let StmtKind::Return(Some(ret_expr)) = &f.body[0].kind else {
        panic!("expected a return statement, got {:?}", f.body[0].kind);
    };
    let ExprKind::Interpolated(parts) = &ret_expr.kind else {
        panic!("expected an interpolated string, got {:?}", ret_expr.kind);
    };
    let var_part = parts
        .iter()
        .find(|p| matches!(&p.kind, ExprKind::Variable(_)))
        .expect("expected a Variable part for $undefinedThing");
    // Line 3, where the real interpolation is - not line 1 col 1,
    // the throwaway re-lex's own fresh coordinates.
    assert_eq!(var_part.span.start.line, 3, "got: {:?}", var_part.span);
    assert!(var_part.span.start.col > 1, "got: {:?}", var_part.span);
}

#[test]
fn quote_inside_complex_interpolation_does_not_end_the_string() {
    let program = parse(r#"<?php $x = "before {$data["key"]} after";"#);
    assert!(matches!(
        &program[0].kind,
        StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, _),
            ..
        })
    ));
}
