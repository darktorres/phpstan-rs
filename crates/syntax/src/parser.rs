use std::sync::Arc;

use crate::ast::*;
use crate::name::{Interner, Name};
use crate::token::{Keyword, Position, Span, Token};

/// A parse failure: malformed syntax.
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
    pub(crate) doc_comments: rustc_hash::FxHashMap<usize, Arc<str>>,
    // Each `doc_comments` entry's own start line, same keys.
    pub(crate) doc_comment_lines: rustc_hash::FxHashMap<usize, u32>,
    // Where several doc comments directly precede one token, all of them
    // folded into one comment, same keys. A statement reads every one
    // (real PHPStan's `getComments()`); a declaration only its last
    // (`getDocComment()`, `doc_comments` above).
    pub(crate) doc_comments_all: rustc_hash::FxHashMap<usize, Arc<str>>,
    // Where the (first) doc comment in `doc_comments` starts, same keys.
    pub(crate) doc_comment_starts: rustc_hash::FxHashMap<usize, crate::token::Position>,
    // The id the next node built gets - see `NodeId`.
    pub(crate) next_node_id: std::cell::Cell<u32>,
    // What names in the tree are interned into.
    pub(crate) interner: Arc<Interner>,
    // Errors that stand unless the node they're keyed by turns out to be
    // part of a destructuring target: a skipped array slot (`[, $b]`) and
    // a `list(...)`. Which it is is only known once the outermost `[...]`
    // is followed by `=`, so they wait until then.
    pub(crate) pattern_only: Vec<(NodeId, ParseError)>,
    // The `array(...)` literals, which can't be destructuring targets.
    pub(crate) long_arrays: rustc_hash::FxHashSet<NodeId>,
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

/// Where a class member's parse began: its first token and how many
/// members of each kind existed before it.
#[derive(Clone, Copy)]
pub(crate) struct MemberStart {
    pub(crate) first_token: usize,
    pub(crate) properties: usize,
    pub(crate) methods: usize,
    pub(crate) consts: usize,
}

impl Parser {
    pub fn new(tokens: Vec<(Token, Span, Option<String>)>) -> Self {
        let mut filtered = Vec::with_capacity(tokens.len());
        let mut filtered_spans = Vec::with_capacity(tokens.len());
        let mut filtered_keyword_raw = Vec::with_capacity(tokens.len());
        let mut doc_comments = rustc_hash::FxHashMap::default();
        let mut doc_comment_lines = rustc_hash::FxHashMap::default();
        let mut doc_comments_all = rustc_hash::FxHashMap::default();
        let mut doc_comment_starts = rustc_hash::FxHashMap::default();
        let mut pending: Vec<(Arc<str>, crate::token::Position)> = Vec::new();
        for (tok, span, keyword_raw) in tokens {
            match tok {
                Token::DocComment(s) => pending.push((s, span.start)),
                other => {
                    if let Some((first_start, last)) = pending
                        .first()
                        .map(|(_, start)| *start)
                        .zip(pending.last().cloned())
                    {
                        doc_comments.insert(filtered.len(), last.0);
                        doc_comment_lines.insert(filtered.len(), first_start.line);
                        doc_comment_starts.insert(filtered.len(), first_start);
                        if pending.len() > 1 {
                            let lines: Vec<(Arc<str>, u32)> = pending
                                .iter()
                                .map(|(doc, start)| (doc.clone(), start.line))
                                .collect();
                            doc_comments_all.insert(filtered.len(), fold_doc_comments(&lines));
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
            doc_comment_starts,
            next_node_id: std::cell::Cell::new(0),
            interner: Arc::clone(Interner::global()),
            pattern_only: Vec::new(),
            long_arrays: rustc_hash::FxHashSet::default(),
        }
    }

    /// The span of the token about to be read.
    pub(crate) fn current_span(&self) -> Span {
        self.spans
            .get(self.pos)
            .or_else(|| self.spans.last())
            .copied()
            .unwrap_or_default()
    }

    /// The position a `Stmt`/`Expr` about to be parsed starts at - call
    /// before consuming its first token. Falls back to the last known
    /// span: `self.pos` can run one past the last real token
    /// (`__halt_compiler()` jumps there), as `peek()` and `skip()` also
    /// allow.
    pub(crate) fn current_pos(&self) -> Position {
        self.spans
            .get(self.pos)
            .or_else(|| self.spans.last())
            .map(|s| s.start)
            .unwrap_or_default()
    }

    /// What deleting a class member removes, for a member whose first
    /// token (after any doc comment) was `first_token` and whose last is
    /// the token just consumed: whole lines when nothing else shares
    /// them, else from its start up to the next token.
    pub(crate) fn member_removal_span(&self, first_token: usize) -> Span {
        let start = self
            .doc_comment_starts
            .get(&first_token)
            .copied()
            .or_else(|| self.spans.get(first_token).map(|s| s.start))
            .unwrap_or_default();
        let previous_line = first_token
            .checked_sub(1)
            .and_then(|i| self.spans.get(i))
            .map_or(0, |s| s.end.line);
        let end = self.previous_end();
        let next = self.current_pos();
        if previous_line < start.line && next.line > end.line {
            Span {
                start: Position {
                    line: start.line,
                    col: 1,
                },
                end: Position {
                    line: end.line + 1,
                    col: 1,
                },
            }
        } else {
            Span { start, end: next }
        }
    }

    /// Gives the member begun at `start` its `removal_span`, when it was
    /// the only member its declaration produced.
    pub(crate) fn finish_member(
        &self,
        start: MemberStart,
        properties: &mut [PropertyDecl],
        methods: &mut [MethodDecl],
        consts: &mut [ClassConst],
    ) {
        let added = (
            properties.len() - start.properties,
            methods.len() - start.methods,
            consts.len() - start.consts,
        );
        let span = Some(self.member_removal_span(start.first_token));
        match added {
            (1, 0, 0) => properties[start.properties].removal_span = span,
            (0, 1, 0) => methods[start.methods].removal_span = span,
            (0, 0, 1) => consts[start.consts].removal_span = span,
            _ => {}
        }
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

    /// Interns names into `interner` rather than the global one.
    pub fn with_interner(mut self, interner: &Arc<Interner>) -> Self {
        self.interner = Arc::clone(interner);
        self
    }

    /// A reference by name, not yet resolved.
    pub(crate) fn qualified_name(&self, written: Name) -> QualifiedName {
        QualifiedName::new(written)
    }

    /// A fresh id for the node being built.
    pub(crate) fn node_id(&self) -> NodeId {
        let id = self.next_node_id.get();
        self.next_node_id.set(id + 1);
        NodeId::new(id)
    }

    /// Builds a `Stmt` spanning from `start` (the position captured before
    /// this statement's first token was consumed) to whatever token was
    /// last consumed.
    pub(crate) fn stmt(&self, kind: StmtKind, start: Position) -> Stmt {
        Stmt {
            id: self.node_id(),
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
            id: self.node_id(),
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
        self.check_pattern_only()?;
        crate::name_resolution::resolve_names(&mut stmts);
        Ok(stmts)
    }

    /// The older, braced namespace form - `namespace Foo\Bar { ... }`, or
    /// bare `namespace { ... }` for the global namespace - as opposed to
    /// the common `namespace Foo\Bar;` statement form (still handled
    /// generically in parse_statement). Only legal at the top level of a
    /// file, so this is special-cased in parse_program rather than making
    /// every parse_statement caller handle getting more than one Stmt
    /// back. The block's body is spliced into the surrounding statement
    /// list after a `Stmt::Namespace` marker, the same shape the statement
    /// form gives.
    pub(crate) fn parse_namespace_stmt(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let start = self.current_pos();
        self.skip(); // `namespace`
        let name = self.parse_qualified_name();
        if self.check(&Token::LBrace) {
            let mut stmts = vec![self.stmt(StmtKind::Namespace(name.to_string()), start)];
            stmts.extend(self.parse_stmt_list_block()?);
            Ok(stmts)
        } else {
            self.expect_semicolon()?;
            Ok(vec![
                self.stmt(StmtKind::Namespace(name.to_string()), start),
            ])
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
