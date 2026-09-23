use std::sync::Arc;

use crate::ast::*;
use crate::token::{Keyword, Position, Span, Token};

// Real PHP's own engine imposes a call-stack-depth guard specifically so
// pathologically nested input (`~~~~~~...~~~~~1`, deeply chained `??`,
// deeply nested parens/arrays) produces a normal fatal error instead of
// crashing the process - php-src's own Zend/tests/stack_limit/*.phpt exist
// to test exactly that. This parser's few genuinely self-recursive (not
// while-loop) productions need the same guard: a stack overflow is a hard
// fault, not a panic, so it can't be caught by anything upstream (a batch
// checker's catch_unwind included) and takes the whole process down.
const MAX_EXPR_DEPTH: usize = 1000;

/// A recoverable parse failure - genuinely malformed syntax, as opposed to
/// `enter_expr`'s own `MAX_EXPR_DEPTH` guard (a resource-exhaustion limit,
/// not a syntax error, and kept as a panic - see its own doc comment).
/// `message` mirrors this parser's own former panic message text.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ParseError {}

pub struct Parser {
    pub(crate) tokens: Vec<Token>,
    // Parallel to `tokens` (same length, same indices) - `spans[i]` is
    // `tokens[i]`'s own source span. Kept separate rather than zipped
    // into one `Vec<(Token, Span)>` so every existing `self.tokens[...]`
    // access throughout this file (there are many) keeps working
    // unchanged.
    pub(crate) spans: Vec<Span>,
    // Parallel to `tokens`/`spans` the same way - `keyword_raw[i]` is
    // `Some(original spelling)` when
    // `tokens[i]` is a `Token::Keyword`, `None` otherwise (including for
    // every non-keyword token). Lets `take_name_segment` recover a
    // keyword-shaped identifier's true declared-name case without
    // touching `Token`'s own shape or any of its ~167 existing match/
    // construct sites.
    pub(crate) keyword_raw: Vec<Option<String>>,
    pub(crate) pos: usize,
    // A doc comment's raw text, keyed by the index (in `tokens`, after
    // filtering) of the token it immediately precedes. Built once up front
    // so the rest of the parser never has to know DocComment tokens
    // existed - only the specific call sites that attach one to a
    // declaration look here. `Arc<str>`, not `String`: carries the
    // lexer's own interned `Token::DocComment` value through unchanged -
    // converting to `String` here and back to `Arc<str>` at the AST
    // fields that ultimately store it would be a pure wash.
    pub(crate) doc_comments: std::collections::HashMap<usize, Arc<str>>,
    // Each `doc_comments` entry's own start line, same keys.
    pub(crate) doc_comment_lines: std::collections::HashMap<usize, u32>,
    // Where several doc comments directly precede one token, all of them
    // folded into one comment, same keys. A statement reads every one
    // (real PHPStan's `getComments()`); a declaration only its last
    // (`getDocComment()`, `doc_comments` above).
    pub(crate) doc_comments_all: std::collections::HashMap<usize, Arc<str>>,
    // Incremented/decremented around the parser's self-recursive
    // expression productions (parse_unary, parse_assignment,
    // parse_coalesce) - see MAX_EXPR_DEPTH.
    pub(crate) expr_depth: usize,
}

mod classes;
mod closures_and_match;
mod control_flow;
mod enums;
mod expr;
mod functions;
mod interpolation;
mod postfix;
mod primary;
mod property_hooks;
mod stmt;
mod tokens;
mod type_hints;

impl Parser {
    pub fn new(tokens: Vec<(Token, Span, Option<String>)>) -> Self {
        let mut filtered = Vec::with_capacity(tokens.len());
        let mut filtered_spans = Vec::with_capacity(tokens.len());
        let mut filtered_keyword_raw = Vec::with_capacity(tokens.len());
        let mut doc_comments = std::collections::HashMap::new();
        let mut doc_comment_lines = std::collections::HashMap::new();
        let mut doc_comments_all = std::collections::HashMap::new();
        let mut pending: Vec<(Arc<str>, u32)> = Vec::new();
        for (tok, span, keyword_raw) in tokens {
            match tok {
                Token::DocComment(s) => pending.push((s, span.start.line)),
                other => {
                    if let Some((first_line, last)) = pending
                        .first()
                        .map(|(_, l)| *l)
                        .zip(pending.last().cloned())
                    {
                        doc_comments.insert(filtered.len(), last.0);
                        doc_comment_lines.insert(filtered.len(), first_line);
                        if pending.len() > 1 {
                            doc_comments_all.insert(filtered.len(), fold_doc_comments(&pending));
                        }
                        pending.clear();
                    }
                    filtered.push(other);
                    filtered_spans.push(span);
                    filtered_keyword_raw.push(keyword_raw);
                }
            }
        }
        Parser {
            tokens: filtered,
            spans: filtered_spans,
            keyword_raw: filtered_keyword_raw,
            pos: 0,
            doc_comments,
            doc_comment_lines,
            doc_comments_all,
            expr_depth: 0,
        }
    }

    /// The position a `Stmt`/`Expr` about to be parsed starts at - call
    /// before consuming its first token. Falls back to the last known
    /// span rather than indexing directly: `self.pos` can run one past
    /// the last real token (e.g. `__halt_compiler()`'s handling
    /// deliberately jumps there), the same real edge case `peek()`/
    /// `advance()` already guard against.
    pub(crate) fn current_pos(&self) -> Position {
        self.spans
            .get(self.pos)
            .or_else(|| self.spans.last())
            .map(|s| s.start)
            .unwrap_or_default()
    }

    /// The end of the last token actually consumed - call right before
    /// returning a `Stmt`/`Expr` to close out its span. Same defensive
    /// fallback as `current_pos` for the same reason.
    pub(crate) fn previous_end(&self) -> Position {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.spans.get(i))
            .or_else(|| self.spans.last())
            .map(|s| s.end)
            .unwrap_or_default()
    }

    /// Builds a `Stmt` spanning from `start` (the position captured before
    /// this statement's first token was consumed) to whatever token was
    /// last consumed.
    pub(crate) fn stmt(&self, kind: StmtKind, start: Position) -> Stmt {
        Stmt {
            kind,
            span: Span {
                start,
                end: self.previous_end(),
            },
            doc_comment: None,
            doc_line: None,
        }
    }

    /// Same as `stmt`, for expressions.
    pub(crate) fn expr(&self, kind: ExprKind, start: Position) -> Expr {
        Expr {
            kind,
            span: Span {
                start,
                end: self.previous_end(),
            },
        }
    }

    /// Consumes any doc comment immediately preceding the current position -
    /// call this before advancing past the first token of a declaration.
    pub(crate) fn take_doc_comment(&mut self) -> Option<Arc<str>> {
        self.doc_comments.remove(&self.pos)
    }

    /// Bumps the expression-recursion depth counter, panicking (a normal,
    /// catchable error) once it passes MAX_EXPR_DEPTH instead of letting
    /// the recursion keep going until it segfaults. Callers must pair this
    /// with a matching decrement once their own recursive call returns -
    /// see the three call sites for the exact pattern.
    pub(crate) fn enter_expr(&mut self) {
        self.expr_depth += 1;
        if self.expr_depth > MAX_EXPR_DEPTH {
            panic!("parse error: expression nested too deeply (over {MAX_EXPR_DEPTH} levels)");
        }
    }

    pub(crate) fn leave_expr(&mut self) {
        self.expr_depth -= 1;
    }

    #[hotpath::measure]
    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        while !self.at_eof() {
            // See parse_statement's own Namespace arm for why the
            // backslash check matters: `namespace\foo();` at the top
            // level is a relative-name call, not a declaration.
            if matches!(self.peek(), Token::Keyword(Keyword::Namespace))
                && !matches!(self.peek_at(1), Token::Backslash)
            {
                stmts.extend(self.parse_namespace_stmt()?);
            } else {
                stmts.push(self.parse_statement()?);
            }
        }
        Ok(stmts)
    }

    /// The older, braced namespace form - `namespace Foo\Bar { ... }`, or
    /// bare `namespace { ... }` for the global namespace - as opposed to
    /// the common `namespace Foo\Bar;` statement form (still handled
    /// generically in parse_statement). Only legal at the top level of a
    /// file, so this is special-cased in parse_program rather than making
    /// every parse_statement caller handle getting more than one Stmt
    /// back. Namespaces aren't modeled beyond the Stmt::Namespace marker,
    /// so the block's body is spliced into the surrounding statement list
    /// instead of nested - that's also what keeps its declarations visible
    /// to Registry::index, which only scans the top-level Vec<Stmt>.
    pub(crate) fn parse_namespace_stmt(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let start = self.current_pos();
        self.skip(); // `namespace`
        let name = self.parse_qualified_name();
        if self.check(&Token::LBrace) {
            let mut stmts = vec![self.stmt(StmtKind::Namespace(name), start)];
            stmts.extend(self.parse_stmt_list_block()?);
            Ok(stmts)
        } else {
            self.expect_semicolon()?;
            Ok(vec![self.stmt(StmtKind::Namespace(name), start)])
        }
    }
}

#[cfg(test)]
mod tests;

/// Several consecutive `/** ... */` comments as one: each one's body, in
/// order, inside a single pair of delimiters.
fn fold_doc_comments(docs: &[(Arc<str>, u32)]) -> Arc<str> {
    let bodies: Vec<&str> = docs
        .iter()
        .map(|(d, _)| d.trim().trim_start_matches("/**").trim_end_matches("*/"))
        .collect();
    Arc::from(format!("/**{}\n*/", bodies.join("\n")).as_str())
}
