//! Parsing a whole file: lexing, parsing, name resolution and the
//! parse-time annotations, with what the lexer found besides the tree.

use std::sync::Arc;

use crate::annotations::{Annotations, annotate};
use crate::ast::Stmt;
use crate::lexer::{LexWarning, Lexer};
use crate::name::Interner;
use crate::parser::{ParseError, Parser};
use crate::token::{Position, Token};

/// A comment as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub start: Position,
    pub text: String,
    /// A `/** ... */` doc comment.
    pub is_doc: bool,
}

/// A parsed file.
#[derive(Debug)]
pub struct ParsedFile {
    pub stmts: Vec<Stmt>,
    /// Every comment, doc comments included, in source order.
    pub comments: Vec<Comment>,
    /// Where each line holding code (anything but a comment) has its
    /// first token, in source order.
    pub code_line_starts: Vec<Position>,
    /// What the lexer skipped over.
    pub warnings: Vec<LexWarning>,
    pub annotations: Annotations,
}

/// Lexes and parses `source`, interning its names in `interner`.
pub fn parse(source: &[u8], interner: &Arc<Interner>) -> Result<ParsedFile, ParseError> {
    let (tokens, line_comments, warnings) = Lexer::from_bytes(source)
        .with_interner(interner)
        .tokenize_with_comments();
    let mut comments: Vec<Comment> = line_comments
        .into_iter()
        .map(|(start, text)| Comment {
            start,
            text,
            is_doc: false,
        })
        .collect();
    let mut code_line_starts: Vec<Position> = Vec::new();
    for (token, span, _) in &tokens {
        match token {
            Token::Eof => {}
            Token::DocComment(text) => comments.push(Comment {
                start: span.start,
                text: text.to_string(),
                is_doc: true,
            }),
            _ => {
                if code_line_starts
                    .last()
                    .is_none_or(|last| last.line != span.start.line)
                {
                    code_line_starts.push(span.start);
                }
            }
        }
    }
    comments.sort_by_key(|c| (c.start.line, c.start.col));
    let stmts = Parser::new(tokens)
        .with_interner(interner)
        .parse_program()?;
    let annotations = annotate(&stmts);
    Ok(ParsedFile {
        stmts,
        comments,
        code_line_starts,
        warnings,
        annotations,
    })
}
