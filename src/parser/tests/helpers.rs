use super::super::*;
use crate::lexer::Lexer;
use crate::token::Span;

pub(super) fn parse(src: &str) -> Vec<Stmt> {
    Parser::new(Lexer::new(src).tokenize_with_spans())
        .parse_program()
        .unwrap()
}

/// Slices `src` by a `Span`'s own 1-indexed line/char-column positions
/// (`Position`'s own doc comment: columns count UTF-8 *characters*, not
/// bytes) - the same convention every real AST span uses, so a test can
/// assert a Phase 17 span actually covers the identifier text it claims
/// to, not just that it exists. Single-line spans only (every case this
/// project's own span-carrying declarations need to test is one).
pub(super) fn text_at(src: &str, span: Span) -> String {
    assert_eq!(
        span.start.line, span.end.line,
        "text_at only supports a single-line span, got {span:?}"
    );
    let line = src
        .lines()
        .nth((span.start.line - 1) as usize)
        .unwrap_or_else(|| panic!("line {} out of range in {src:?}", span.start.line));
    let chars: Vec<char> = line.chars().collect();
    let start = (span.start.col - 1) as usize;
    let end = (span.end.col - 1) as usize;
    chars[start..end].iter().collect()
}
