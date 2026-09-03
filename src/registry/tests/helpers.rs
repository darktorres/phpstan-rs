use super::super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn build_registry(src: &str) -> Registry {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    Registry::build(&program)
}
