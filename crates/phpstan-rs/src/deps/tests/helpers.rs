use super::super::*;
use syntax::lexer::Lexer;
use syntax::parser::Parser;

pub(super) fn refs(src: &str) -> FxHashSet<String> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let ast = Parser::new(tokens).parse_program().unwrap();
    referenced_names(&ast)
}
