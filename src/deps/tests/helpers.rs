use super::super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn refs(src: &str) -> HashSet<String> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let ast = Parser::new(tokens).parse_program().unwrap();
    referenced_names(&ast)
}
