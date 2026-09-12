//! Parsing of string literals and interpolation, including complex interpolation expressions.

use super::super::*;
use super::helpers::*;
use crate::token::StringPart;

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

// Found by the fuzz/ workspace's own token_stream_parse target (feeds a
// synthetic, Arbitrary-derived token stream straight into the parser,
// bypassing the real lexer entirely - real PHP source can never produce
// a ComplexExpr fragment_base this extreme, since no real file gets
// remotely close to u32::MAX lines/columns, but the parser must still
// not crash on one). `bias_position` (interpolation.rs) computed
// `frag_base.col + pos.col - 1` with plain u32 arithmetic - a
// `frag_base` this close to `u32::MAX` overflowed and panicked
// ("attempt to add with overflow") instead of producing any position at
// all. Exercised here through the real Parser::new/parse_program path
// (not a private-helper unit test), matching exactly how the fuzz
// target itself reaches it.
#[test]
fn a_complex_interpolation_fragment_with_an_extreme_base_position_does_not_panic() {
    let extreme = Position {
        line: u32::MAX,
        col: u32::MAX,
    };
    let token = Token::InterpString(vec![StringPart::ComplexExpr("2".to_string(), extreme)]);
    let span = Span {
        start: extreme,
        end: extreme,
    };
    let result = Parser::new(vec![(token, span, None)]).parse_program();
    // Either outcome is fine - the only thing this test guards against is
    // the arithmetic-overflow panic itself.
    let _ = result;
}
