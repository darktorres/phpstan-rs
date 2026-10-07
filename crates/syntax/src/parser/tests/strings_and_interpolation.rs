//! Parsing of string literals and interpolation, including complex interpolation expressions.

use super::super::*;
use super::helpers::*;
use crate::span::LineCol;
use crate::token::StringPart;

#[test]
fn unescaped_single_quoted_literal_is_a_source_range() {
    let parsed = crate::parse::parse("<?php $value = 'plain bytes';").unwrap();
    let payload = parsed
        .arenas
        .exprs
        .iter()
        .find_map(|(_, expr)| match &expr.kind {
            ExprKind::Literal(Literal::String(payload)) => Some(payload),
            _ => None,
        })
        .unwrap();
    assert!(matches!(payload, StringPayload::Source(_)));
    assert_eq!(parsed.string_bytes(payload), Some(&b"plain bytes"[..]));
}

fn assignment_rhs(program: &TestParsedFile, index: usize) -> ExprId {
    let StmtKind::Expression(expr) = &program[index].kind else {
        panic!(
            "expected an expression statement, got {:?}",
            program[index].kind
        );
    };
    let ExprKind::Assign(_, rhs) = &program.expr(*expr).kind else {
        panic!("expected an assignment, got {:?}", program.expr(*expr).kind);
    };
    *rhs
}

#[test]
fn shell_exec_backtick_string_parses() {
    let program = parse(r#"<?php $x = `echo -n ""`;"#);
    let rhs = assignment_rhs(&program, 0);
    let ExprKind::ShellExec(cmd) = &program.expr(rhs).kind else {
        panic!("expected a ShellExec, got {:?}", program.expr(rhs).kind);
    };
    let ExprKind::Literal(Literal::String(payload)) = &program.expr(*cmd).kind else {
        panic!("expected a literal shell body");
    };
    assert_eq!(
        program.string_text(payload).as_deref(),
        Some("echo -n \"\"")
    );
}

// A diagnostic inside a {$...} string-interpolation expression must
// report the real file position, not the throwaway sub-parse's own
// fresh line-1-col-1 coordinates.

#[test]
fn complex_interpolation_expression_spans_are_biased_to_real_file_positions() {
    let src = "<?php\nfunction f() {\n    return \"value: {$undefinedThing}\";\n}\n";
    let program = parse(src);
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected a function declaration, got {:?}", program[0].kind);
    };
    let f = program.function(*f);
    let body_stmt = f.body_stmts(&program.arenas)[0];
    let StmtKind::Return(Some(ret_expr)) = &program.stmt(body_stmt).kind else {
        panic!(
            "expected a return statement, got {:?}",
            program.stmt(body_stmt).kind
        );
    };
    let ExprKind::Interpolated(parts) = &program.expr(*ret_expr).kind else {
        panic!(
            "expected an interpolated string, got {:?}",
            program.expr(*ret_expr).kind
        );
    };
    let var_part = program
        .arenas
        .expr_ids(parts)
        .iter()
        .find(|p| matches!(&program.expr(**p).kind, ExprKind::Variable(_)))
        .expect("expected a Variable part for $undefinedThing");
    // Line 3, where the real interpolation is - not line 1 col 1,
    // the throwaway re-lex's own fresh coordinates.
    let span = program.expr_span(*var_part);
    let at = position(src, span.start);
    assert_eq!(at.line, 3, "got: {span:?}");
    assert!(at.col > 1, "got: {span:?}");
}

/// `"$arr[key]"` - a bareword between the brackets is a *string* key in
/// PHP's own interpolation simple syntax, not a constant reference.
#[test]
fn bareword_index_interpolation_parses_as_an_index_with_a_string_literal_key() {
    let program = parse(r#"<?php $x = "$arr[key]";"#);
    let rhs = assignment_rhs(&program, 0);
    let ExprKind::Interpolated(parts) = &program.expr(rhs).kind else {
        panic!(
            "expected an interpolated string, got {:?}",
            program.expr(rhs).kind
        );
    };
    let ExprKind::Index(base, Some(key)) = &program.expr(program.arenas.expr_ids(parts)[0]).kind
    else {
        panic!(
            "expected an Index part, got {:?}",
            program.expr(program.arenas.expr_ids(parts)[0]).kind
        );
    };
    assert!(matches!(&program.expr(*base).kind, ExprKind::Variable(v) if **v == *"arr"));
    let ExprKind::Literal(Literal::String(payload)) = &program.expr(*key).kind else {
        panic!("expected a string index key");
    };
    assert_eq!(program.string_text(payload).as_deref(), Some("key"));
}

/// `"$arr[$idx]"` - a `$`-prefixed key is a variable reference.
#[test]
fn variable_index_interpolation_parses_as_an_index_with_a_variable_key() {
    let program = parse(r#"<?php $x = "$arr[$idx]";"#);
    let rhs = assignment_rhs(&program, 0);
    let ExprKind::Interpolated(parts) = &program.expr(rhs).kind else {
        panic!(
            "expected an interpolated string, got {:?}",
            program.expr(rhs).kind
        );
    };
    let ExprKind::Index(base, Some(key)) = &program.expr(program.arenas.expr_ids(parts)[0]).kind
    else {
        panic!(
            "expected an Index part, got {:?}",
            program.expr(program.arenas.expr_ids(parts)[0]).kind
        );
    };
    assert!(matches!(&program.expr(*base).kind, ExprKind::Variable(v) if **v == *"arr"));
    assert!(matches!(&program.expr(*key).kind, ExprKind::Variable(v) if **v == *"idx"));
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
        let rhs = assignment_rhs(&program, 0);
        let ExprKind::Interpolated(parts) = &program.expr(rhs).kind else {
            panic!(
                "expected an interpolated string, got {:?}",
                program.expr(rhs).kind
            );
        };
        let ExprKind::Index(_, Some(key)) = &program.expr(program.arenas.expr_ids(parts)[0]).kind
        else {
            panic!(
                "expected an Index part, got {:?}",
                program.expr(program.arenas.expr_ids(parts)[0]).kind
            );
        };
        assert!(
            matches!(&program.expr(*key).kind, ExprKind::Literal(Literal::Int(n)) if *n == expected),
            "src {src:?}: got {:?}",
            program.expr(*key).kind
        );
    }
}

/// A number that isn't a canonical, in-range int (`01`, `-0`, a hex form,
/// `1_000`, past `PHP_INT_MAX`) is the string key PHP reads it as.
#[test]
fn a_non_canonical_number_index_in_interpolation_is_a_string_key() {
    for key in ["01", "-0", "0x1A", "1_000", "9223372036854775808"] {
        let src = format!(r#"<?php $x = "$arr[{key}]";"#);
        let program = parse(&src);
        let rhs = assignment_rhs(&program, 0);
        let ExprKind::Interpolated(parts) = &program.expr(rhs).kind else {
            panic!(
                "expected an interpolated string, got {:?}",
                program.expr(rhs).kind
            );
        };
        let ExprKind::Index(_, Some(index)) = &program.expr(program.arenas.expr_ids(parts)[0]).kind
        else {
            panic!(
                "expected an Index part for {key}, got {:?}",
                program.expr(program.arenas.expr_ids(parts)[0]).kind
            );
        };
        let ExprKind::Literal(Literal::String(payload)) = &program.expr(*index).kind else {
            panic!(
                "{key}: expected a string index key, got {:?}",
                program.expr(*index).kind
            );
        };
        assert_eq!(program.string_text(payload).as_deref(), Some(key));
    }
}

/// Each `{$var}` in a string keeps its own position, so diagnostics on
/// several of them in one string land on distinct columns.
#[test]
fn multiple_bare_braced_vars_in_one_string_each_keep_their_own_real_position() {
    let src = "<?php\nfunction f() {\n    return \"{$foo} and {$bar}\";\n}\n";
    let program = parse(src);
    let StmtKind::FunctionDecl(f) = &program[0].kind else {
        panic!("expected a function declaration, got {:?}", program[0].kind);
    };
    let f = program.function(*f);
    let body_stmt = f.body_stmts(&program.arenas)[0];
    let StmtKind::Return(Some(ret_expr)) = &program.stmt(body_stmt).kind else {
        panic!(
            "expected a return statement, got {:?}",
            program.stmt(body_stmt).kind
        );
    };
    let ExprKind::Interpolated(parts) = &program.expr(*ret_expr).kind else {
        panic!(
            "expected an interpolated string, got {:?}",
            program.expr(*ret_expr).kind
        );
    };
    let var_spans: Vec<(String, Span)> = program
        .arenas
        .expr_ids(parts)
        .iter()
        .filter_map(|p| match &program.expr(*p).kind {
            ExprKind::Variable(name) => Some((name.to_string(), program.expr_span(*p))),
            _ => None,
        })
        .collect();
    assert_eq!(
        var_spans
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["foo", "bar"]
    );
    assert_ne!(
        var_spans[0].1, var_spans[1].1,
        "both {{$foo}} and {{$bar}} got the same span - a real per-occurrence position was lost"
    );
    // `    return "{$foo} and {$bar}";` - `{$foo}`'s own `$` sits at
    // column 14, `{$bar}`'s at column 25.
    assert_eq!(
        position(src, var_spans[0].1.start),
        LineCol { line: 3, col: 14 }
    );
    assert_eq!(
        position(src, var_spans[1].1.start),
        LineCol { line: 3, col: 25 }
    );
}

/// `"{$var->prop}"`/`"{$var[key]}"` - the explicitly-braced spelling of
/// the same two simple shapes Stage 1 covers for the bare-`$` form -
/// also skip re-lexing, via `try_parse_simple_interpolation`.
#[test]
fn braced_property_access_interpolation_parses_as_a_property_access() {
    let program = parse(r#"<?php $x = "{$obj->prop}";"#);
    let rhs = assignment_rhs(&program, 0);
    let ExprKind::Interpolated(parts) = &program.expr(rhs).kind else {
        panic!(
            "expected an interpolated string, got {:?}",
            program.expr(rhs).kind
        );
    };
    let ExprKind::PropertyAccess(base, prop, nullsafe) =
        &program.expr(program.arenas.expr_ids(parts)[0]).kind
    else {
        panic!(
            "expected a PropertyAccess part, got {:?}",
            program.expr(program.arenas.expr_ids(parts)[0]).kind
        );
    };
    assert!(matches!(&program.expr(*base).kind, ExprKind::Variable(v) if **v == *"obj"));
    assert_eq!(&*prop.name, "prop");
    assert!(!nullsafe.is_nullsafe());
}

#[test]
fn braced_index_interpolation_parses_as_an_index() {
    let program = parse(r#"<?php $x = "{$arr[key]}";"#);
    let rhs = assignment_rhs(&program, 0);
    let ExprKind::Interpolated(parts) = &program.expr(rhs).kind else {
        panic!(
            "expected an interpolated string, got {:?}",
            program.expr(rhs).kind
        );
    };
    let ExprKind::Index(base, Some(key)) = &program.expr(program.arenas.expr_ids(parts)[0]).kind
    else {
        panic!(
            "expected an Index part, got {:?}",
            program.expr(program.arenas.expr_ids(parts)[0]).kind
        );
    };
    assert!(matches!(&program.expr(*base).kind, ExprKind::Variable(v) if **v == *"arr"));
    let ExprKind::Literal(Literal::String(payload)) = &program.expr(*key).kind else {
        panic!("expected a string index key");
    };
    assert_eq!(program.string_text(payload).as_deref(), Some("key"));
}

/// A braced block that's genuinely complex (here: a method call, not just
/// a property/index access) must still go through the original re-lex
/// path - `try_parse_simple_interpolation` only matches when the simple
/// shape consumes the block's *entire* inner text.
#[test]
fn a_braced_method_call_is_not_mistaken_for_a_simple_property_access() {
    let program = parse(r#"<?php $x = "{$obj->method()}";"#);
    let rhs = assignment_rhs(&program, 0);
    let ExprKind::Interpolated(parts) = &program.expr(rhs).kind else {
        panic!(
            "expected an interpolated string, got {:?}",
            program.expr(rhs).kind
        );
    };
    assert!(
        matches!(&program.expr(program.arenas.expr_ids(parts)[0]).kind, ExprKind::MethodCall(_, name, _, _) if **name == *"method"),
        "got {:?}",
        program.expr(program.arenas.expr_ids(parts)[0]).kind
    );
}

/// A key that fits none of `SimpleIndexKey`'s three shapes (here: starts
/// with a digit but isn't a plain integer) still falls back to the
/// original `ComplexExpr` re-lex path, unchanged.
#[test]
fn a_malformed_index_key_falls_back_to_complex_expr_re_lexing() {
    let program = parse(r#"<?php $x = "$arr[1a]";"#);
    let rhs = assignment_rhs(&program, 0);
    let ExprKind::Interpolated(parts) = &program.expr(rhs).kind else {
        panic!(
            "expected an interpolated string, got {:?}",
            program.expr(rhs).kind
        );
    };
    // Falls back to the re-lexed ComplexExpr path, which still produces a
    // real Index expression (`'1a'` re-lexes as a plain string literal) -
    // same observable shape as the fast path, just built via the slower
    // route, confirming the fallback stays behavior-preserving.
    let ExprKind::Index(base, Some(key)) = &program.expr(program.arenas.expr_ids(parts)[0]).kind
    else {
        panic!(
            "expected an Index part, got {:?}",
            program.expr(program.arenas.expr_ids(parts)[0]).kind
        );
    };
    assert!(matches!(&program.expr(*base).kind, ExprKind::Variable(v) if **v == *"arr"));
    assert!(matches!(
        &program.expr(*key).kind,
        ExprKind::Literal(Literal::String(_))
    ));
    let ExprKind::Index(_, Some(key)) = &program.expr(program.arenas.expr_ids(parts)[0]).kind
    else {
        unreachable!()
    };
    let ExprKind::Literal(Literal::String(payload)) = &program.expr(*key).kind else {
        unreachable!()
    };
    assert_eq!(program.string_text(payload).as_deref(), Some("1a"));
}

#[test]
fn quote_inside_complex_interpolation_does_not_end_the_string() {
    let program = parse(r#"<?php $x = "before {$data["key"]} after";"#);
    assert!(matches!(
        &program[0].kind,
        StmtKind::Expression(expr) if matches!(program.expr(*expr).kind, ExprKind::Assign(_, _))
    ));
}

// A fragment's spans are its base plus its own offsets in u32 arithmetic -
// a `ComplexExpr` base this close to `u32::MAX` must not overflow/panic,
// even though no real PHP file gets remotely close to it. Exercised through the real
// Parser::new/parse_program path, not a private-helper unit test.
#[test]
fn a_complex_interpolation_fragment_with_an_extreme_base_position_does_not_panic() {
    let extreme = u32::MAX;
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

/// `\"` escapes only in a double-quoted string and `` \` `` only in a
/// backtick one; elsewhere, a heredoc included, each stays two bytes.
#[test]
fn quote_escapes_decode_only_for_their_own_delimiter() {
    fn literal_bytes(program: &TestParsedFile, id: ExprId) -> Vec<u8> {
        match &program.expr(id).kind {
            ExprKind::Literal(Literal::String(payload)) => program
                .string_bytes(payload)
                .expect("literal belongs to parsed program")
                .to_vec(),
            ExprKind::ShellExec(inner) => literal_bytes(program, *inner),
            other => panic!("expected a string, got {other:?}"),
        }
    }
    for (src, expected) in [
        ("<?php $x = \"a\\\"b\\`c\";", &b"a\"b\\`c"[..]),
        ("<?php $x = `a\\`b\\\"c`;", &b"a`b\\\"c"[..]),
        ("<?php $x = <<<EOT\na\\\"b\\`c\nEOT;\n", &b"a\\\"b\\`c"[..]),
    ] {
        let program = parse(src);
        let rhs = assignment_rhs(&program, 0);
        assert_eq!(literal_bytes(&program, rhs), expected, "{src}");
    }
}

#[test]
fn a_braced_interpolation_holds_a_variable_chain_not_any_expression() {
    let parses = |src: &str| {
        Parser::from_token_stream(crate::lexer::Lexer::new(src).lex_token_stream())
            .parse_program()
            .is_ok()
    };
    for valid in [
        r#"<?php echo "{$a->b[0]->c()}";"#,
        r#"<?php echo "{$f()}";"#,
        r#"<?php echo "{$c::$p}";"#,
        r#"<?php echo "{$c::m()}";"#,
        r#"<?php echo "{$a?->b}";"#,
        r#"<?php echo "{$a->{$b}}";"#,
    ] {
        assert!(parses(valid), "{valid} should parse");
    }
    for invalid in [
        r#"<?php echo "{$x = 1}";"#,
        r#"<?php echo "{$x + 1}";"#,
        r#"<?php echo "{$c::K}";"#,
    ] {
        assert!(!parses(invalid), "{invalid} should not parse");
    }
}
