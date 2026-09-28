use super::super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn parse_expr(src: &str) -> Expr {
    let tokens = Lexer::new(&format!("<?php {src};")).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    match program.into_iter().next().unwrap().into_kind() {
        StmtKind::Expression(e) => e,
        other => panic!("expected an Expression statement, got {other:?}"),
    }
}

pub(super) fn parse_stmt(src: &str) -> Stmt {
    let tokens = Lexer::new(&format!("<?php {src}")).tokenize_with_spans();
    Parser::new(tokens)
        .parse_program()
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
}
