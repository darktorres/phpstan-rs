use super::super::*;
use syntax::lexer::Lexer;
use syntax::parser::Parser;

pub(super) fn build_registry(src: &str) -> Registry {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let mut program = Parser::new(tokens).parse_program().unwrap();
    crate::stub_attributes::apply_stub_attributes(&mut program);
    Registry::build(&program)
}

pub(super) fn parse(src: &str) -> Vec<syntax::ast::Stmt> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let mut program = Parser::new(tokens).parse_program().unwrap();
    crate::stub_attributes::apply_stub_attributes(&mut program);
    program
}
