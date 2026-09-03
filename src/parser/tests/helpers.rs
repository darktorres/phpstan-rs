use super::super::*;
use crate::lexer::Lexer;

pub(super) fn parse(src: &str) -> Vec<Stmt> {
    Parser::new(Lexer::new(src).tokenize_with_spans())
        .parse_program()
        .unwrap()
}
