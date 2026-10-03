use super::super::*;
use crate::lexer::Lexer;
use crate::span::Span;

pub(super) fn parse(src: &str) -> Vec<Stmt> {
    Parser::new(Lexer::new(src).tokenize_with_spans())
        .parse_program()
        .unwrap()
}

/// The text of `src` a `Span` covers - lets a test assert a span actually
/// covers the identifier text it claims to, not just that it exists.
/// Single-line spans only.
pub(super) fn text_at(src: &str, span: Span) -> String {
    let text = src
        .get(span.start as usize..span.end as usize)
        .unwrap_or_else(|| panic!("{span:?} out of range in {src:?}"));
    assert!(
        !text.contains('\n'),
        "text_at only supports a single-line span, got {span:?}"
    );
    text.to_string()
}

/// The line and character column of the byte offset `offset` of `src`.
pub(super) fn position(src: &str, offset: u32) -> crate::span::LineCol {
    crate::span::LineIndex::new(src.as_bytes()).line_col(offset)
}
