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

// A diagnostic inside a {$...} string-interpolation expression must
// report the real file position, not the throwaway sub-parse's own
// fresh line-1-col-1 coordinates.

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

/// `"$arr[key]"` - a bareword between the brackets is a *string* key in
/// PHP's own interpolation simple syntax, not a constant reference.
#[test]
fn bareword_index_interpolation_parses_as_an_index_with_a_string_literal_key() {
    let program = parse(r#"<?php $x = "$arr[key]";"#);
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected assignment, got {:?}", program[0].kind);
    };
    let ExprKind::Interpolated(parts) = &rhs.kind else {
        panic!("expected an interpolated string, got {:?}", rhs.kind);
    };
    let ExprKind::Index(base, Some(key)) = &parts[0].kind else {
        panic!("expected an Index part, got {:?}", parts[0].kind);
    };
    assert!(matches!(&base.kind, ExprKind::Variable(v) if v == "arr"));
    assert!(matches!(
        &key.kind,
        ExprKind::Literal(Literal::String(s)) if s == "key"
    ));
}

/// `"$arr[$idx]"` - a `$`-prefixed key is a variable reference.
#[test]
fn variable_index_interpolation_parses_as_an_index_with_a_variable_key() {
    let program = parse(r#"<?php $x = "$arr[$idx]";"#);
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected assignment, got {:?}", program[0].kind);
    };
    let ExprKind::Interpolated(parts) = &rhs.kind else {
        panic!("expected an interpolated string, got {:?}", rhs.kind);
    };
    let ExprKind::Index(base, Some(key)) = &parts[0].kind else {
        panic!("expected an Index part, got {:?}", parts[0].kind);
    };
    assert!(matches!(&base.kind, ExprKind::Variable(v) if v == "arr"));
    assert!(matches!(&key.kind, ExprKind::Variable(v) if v == "idx"));
}

/// `"$arr[0]"`/`"$arr[-1]"` - a plain (optionally negative) integer key.
/// The negative case locks in that a direct `Literal::Int(-1)` (this
/// fast path) infers identically to the `Unary(Neg, Literal::Int(1))`
/// shape an ordinary parsed `-1` expression would produce, so downstream
/// type inference doesn't need to special-case which shape it sees.
#[test]
fn int_index_interpolation_including_negative_parses_as_an_index_with_an_int_literal_key() {
    for (src, expected) in [
        (r#"<?php $x = "$arr[0]";"#, 0),
        (r#"<?php $x = "$arr[-1]";"#, -1),
    ] {
        let program = parse(src);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment, got {:?}", program[0].kind);
        };
        let ExprKind::Interpolated(parts) = &rhs.kind else {
            panic!("expected an interpolated string, got {:?}", rhs.kind);
        };
        let ExprKind::Index(_, Some(key)) = &parts[0].kind else {
            panic!("expected an Index part, got {:?}", parts[0].kind);
        };
        assert!(
            matches!(&key.kind, ExprKind::Literal(Literal::Int(n)) if *n == expected),
            "src {src:?}: got {:?}",
            key.kind
        );
    }
}

/// A key that fits none of `SimpleIndexKey`'s three shapes (here: starts
/// with a digit but isn't a plain integer) still falls back to the
/// original `ComplexExpr` re-lex path, unchanged.
#[test]
fn a_malformed_index_key_falls_back_to_complex_expr_re_lexing() {
    let program = parse(r#"<?php $x = "$arr[1a]";"#);
    let StmtKind::Expression(Expr {
        kind: ExprKind::Assign(_, rhs),
        ..
    }) = &program[0].kind
    else {
        panic!("expected assignment, got {:?}", program[0].kind);
    };
    let ExprKind::Interpolated(parts) = &rhs.kind else {
        panic!("expected an interpolated string, got {:?}", rhs.kind);
    };
    // Falls back to the re-lexed ComplexExpr path, which still produces a
    // real Index expression (`'1a'` re-lexes as a plain string literal) -
    // same observable shape as the fast path, just built via the slower
    // route, confirming the fallback stays behavior-preserving.
    let ExprKind::Index(base, Some(key)) = &parts[0].kind else {
        panic!("expected an Index part, got {:?}", parts[0].kind);
    };
    assert!(matches!(&base.kind, ExprKind::Variable(v) if v == "arr"));
    assert!(matches!(
        &key.kind,
        ExprKind::Literal(Literal::String(s)) if s == "1a"
    ));
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

// `bias_position` (interpolation.rs) computes `frag_base.col + pos.col
// - 1` with plain u32 arithmetic - a `ComplexExpr` `frag_base` this
// close to `u32::MAX` must not overflow/panic, even though no real PHP
// file gets remotely close to it. Exercised through the real
// Parser::new/parse_program path, not a private-helper unit test.
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
