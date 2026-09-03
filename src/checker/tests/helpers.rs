use super::super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::Registry;

pub(super) fn diagnostics_for(src: &str) -> Vec<Diagnostic> {
    diagnostics_for_with(src, |_| {})
}

pub(super) fn diagnostics_for_with(
    src: &str,
    configure: impl FnOnce(&mut Registry),
) -> Vec<Diagnostic> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut registry = Registry::build(&program);
    registry.merge_stubs(crate::php_stubs::generated_stubs());
    registry.merge_stub_classes(crate::php_stubs::generated_stub_classes());
    configure(&mut registry);
    let mut checker = Checker::new(&registry);
    checker.check_program(&program);
    checker.into_diagnostics()
}
