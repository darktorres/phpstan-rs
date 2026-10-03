use std::sync::Arc;

use crate::ast::*;
use crate::lexer::{LexEnd, Lexed, LexedDoc, Lexer, Tok};
use crate::name::{Interner, Name};
use crate::span::{LineIndex, Span};
use crate::token::{Keyword, Token};

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

pub struct Parser<'a> {
    // Where the tokens come from: the lexer, as the parser asks for them,
    // or a list already made.
    supply: Supply<'a>,
    // The tokens not yet consumed, the current one first: the parser looks
    // a few tokens ahead and never back, so these are all of the file it
    // holds. Filled to `LOOKAHEAD` tokens past the current one whenever
    // the supply has them, so `peek_at` is a read and not a pull.
    pub(crate) ahead: std::collections::VecDeque<Tok>,
    // Whether `Token::Eof` is queued: the supply has no more.
    eof_queued: bool,
    // How many tokens have been consumed.
    pub(crate) pos: usize,
    // Where the last consumed token ended, and its span.
    pub(crate) last_end: u32,
    pub(crate) last_span: Span,
    // The file's lines, which the parser asks about the lines tokens and
    // doc comments are on.
    pub(crate) line_index: Arc<LineIndex>,
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
    // Whether the parse has built an anonymous class (`new class`).
    pub(crate) saw_anonymous_class: bool,
    // The labels `goto` statements name, one entry per function-like body
    // being parsed, the file's own first.
    pub(crate) goto_scopes: Vec<GotoScope>,
}

/// How many tokens past the current one the parser holds.
pub(crate) const LOOKAHEAD: usize = 8;

/// A parser's tokens: lexed as it reads, or made before.
enum Supply<'a> {
    Lexer(Box<Lexer<'a>>),
    List(std::vec::IntoIter<Tok>),
}

impl Supply<'_> {
    fn next(&mut self) -> Option<Tok> {
        match self {
            Supply::Lexer(lexer) => Some(lexer.next_tok()),
            Supply::List(tokens) => tokens.next(),
        }
    }
}

/// The labels seen and the `goto` targets met so far in one function-like
/// body.
#[derive(Default)]
pub(crate) struct GotoScope {
    labels: Vec<crate::name::Name>,
    targets: Vec<GotoLabel>,
    // Whether the body has a `yield` of its own.
    is_generator: bool,
}

/// What a function-like body's parse found about the body as a whole.
pub(crate) struct ScopeEnd {
    /// The labels its `goto`s name.
    pub(crate) goto_labels: Box<[GotoLabel]>,
    /// Whether it is a generator: a `yield` or `yield from` in it, outside
    /// the functions and classes nested in it.
    pub(crate) is_generator: bool,
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

/// Where a class member's parse began: where its first token or the doc
/// comment before it starts, where the token before it ended, and how
/// many members of each kind existed before it.
#[derive(Clone, Copy)]
pub(crate) struct MemberStart {
    pub(crate) start: u32,
    pub(crate) previous_end: Option<u32>,
    pub(crate) properties: usize,
    pub(crate) methods: usize,
    pub(crate) consts: usize,
}

impl<'a> Parser<'a> {
    /// A parser of spanned tokens, doc comments among them as tokens of
    /// their own (the list `Lexer::tokenize_with_spans` makes).
    pub fn new(tokens: Vec<(Token, Span, Option<String>)>) -> Parser<'static> {
        let mut lexed = Lexed {
            tokens: Vec::with_capacity(tokens.len()),
            spans: Vec::with_capacity(tokens.len()),
            keyword_raw: Vec::new(),
            docs: Vec::new(),
            line_index: Arc::default(),
        };
        for (token, span, keyword_raw) in tokens {
            match token {
                Token::DocComment(text) => lexed.docs.push(LexedDoc {
                    before: lexed.tokens.len(),
                    text,
                    span,
                }),
                other => {
                    if let Some(raw) = keyword_raw {
                        lexed.keyword_raw.push((lexed.tokens.len(), raw));
                    }
                    lexed.tokens.push(other);
                    lexed.spans.push(span);
                }
            }
        }
        Parser::from_lexed(lexed)
    }

    /// A parser of what the lexer found.
    pub fn from_lexed(lexed: Lexed) -> Parser<'static> {
        let (tokens, line_index) = lexed.into_toks();
        Parser::with_supply(Supply::List(tokens.into_iter()), line_index)
    }

    /// A parser that lexes as it reads: the tokens past the few it looks
    /// at are not lexed until they are needed.
    pub fn from_lexer(lexer: Lexer<'a>) -> Parser<'a> {
        let line_index = lexer.line_index();
        Parser::with_supply(Supply::Lexer(Box::new(lexer)), line_index)
    }

    fn with_supply(supply: Supply<'a>, line_index: Arc<LineIndex>) -> Parser<'a> {
        let mut parser = Parser {
            supply,
            ahead: std::collections::VecDeque::with_capacity(LOOKAHEAD + 2),
            eof_queued: false,
            pos: 0,
            last_end: 0,
            last_span: Span::default(),
            line_index,
            next_node_id: std::cell::Cell::new(0),
            interner: Arc::clone(Interner::global()),
            pattern_only: Vec::new(),
            long_arrays: rustc_hash::FxHashSet::default(),
            saw_anonymous_class: false,
            goto_scopes: vec![GotoScope::default()],
        };
        parser.fill(LOOKAHEAD);
        parser
    }

    /// Queues tokens until the current one and `count` more are held, or
    /// the supply has no more.
    pub(crate) fn fill(&mut self, count: usize) {
        while !self.eof_queued && self.ahead.len() <= count {
            let Some(tok) = self.supply.next() else {
                self.eof_queued = true;
                break;
            };
            self.eof_queued = matches!(tok.token, Token::Eof);
            self.ahead.push_back(tok);
        }
    }

    /// Takes the current token off the front, and queues one more.
    pub(crate) fn pop(&mut self) -> Option<Tok> {
        let tok = self.ahead.pop_front()?;
        self.pos += 1;
        self.last_end = tok.span.end;
        self.last_span = tok.span;
        self.fill(LOOKAHEAD);
        Some(tok)
    }

    /// The lexer's own record of the file once the parse is over: the rest
    /// of the file is lexed, so that what it found in the part the parse
    /// did not reach is there too. None for a parser of a list of tokens.
    pub fn into_lex_end(self) -> Option<LexEnd> {
        match self.supply {
            Supply::Lexer(lexer) => Some(lexer.finish()),
            Supply::List(_) => None,
        }
    }
}

impl Parser<'_> {
    /// Opens the scope of a function-like body; give the result to
    /// `leave_goto_scope` when it ends.
    pub(crate) fn enter_goto_scope(&mut self) -> usize {
        self.goto_scopes.push(GotoScope::default());
        self.goto_scopes.len()
    }

    /// Closes the scope `enter_goto_scope` opened, and returns its labels.
    /// A body that failed to parse leaves its scope behind; this drops it.
    pub(crate) fn leave_goto_scope(&mut self, depth: usize) -> ScopeEnd {
        self.goto_scopes.truncate(depth);
        match self.goto_scopes.pop() {
            Some(scope) => ScopeEnd {
                goto_labels: scope.targets.into_boxed_slice(),
                is_generator: scope.is_generator,
            },
            None => ScopeEnd {
                goto_labels: Box::default(),
                is_generator: false,
            },
        }
    }

    /// A `yield` or `yield from` in the body being parsed.
    pub(crate) fn note_yield(&mut self) {
        if let Some(scope) = self.goto_scopes.last_mut() {
            scope.is_generator = true;
        }
    }

    /// A label statement in the body being parsed.
    pub(crate) fn note_label(&mut self, name: &crate::name::Name) {
        if let Some(scope) = self.goto_scopes.last_mut() {
            scope.labels.push(name.clone());
        }
    }

    /// A `goto` statement in the body being parsed.
    pub(crate) fn note_goto(&mut self, name: &crate::name::Name) {
        let Some(scope) = self.goto_scopes.last_mut() else {
            return;
        };
        let backward = scope.labels.contains(name);
        match scope.targets.iter_mut().find(|t| t.name == *name) {
            Some(target) => target.backward |= backward,
            None => scope.targets.push(GotoLabel {
                name: name.clone(),
                backward,
            }),
        }
    }

    /// The labels the file's own top-level `goto`s name.
    pub fn file_goto_labels(&mut self) -> Box<[GotoLabel]> {
        match self.goto_scopes.first_mut() {
            Some(scope) => std::mem::take(&mut scope.targets).into_boxed_slice(),
            None => Box::default(),
        }
    }

    /// Whether the tree built so far has an anonymous class in it.
    pub fn saw_anonymous_class(&self) -> bool {
        self.saw_anonymous_class
    }

    /// The span of the token about to be read: the last one's when none
    /// is left.
    pub(crate) fn current_span(&self) -> Span {
        self.ahead.front().map_or(self.last_span, |tok| tok.span)
    }

    /// The position a `Stmt`/`Expr` about to be parsed starts at - call
    /// before consuming its first token. Falls back to the last known
    /// span: `self.pos` can run one past the last real token
    /// (`__halt_compiler()` jumps there), as `peek()` and `skip()` also
    /// allow.
    pub(crate) fn current_pos(&self) -> u32 {
        self.current_span().start
    }

    /// Where a class member about to be parsed begins.
    pub(crate) fn member_start(
        &self,
        properties: usize,
        methods: usize,
        consts: usize,
    ) -> MemberStart {
        let first = self.ahead.front();
        MemberStart {
            start: first
                .and_then(|tok| tok.docs.as_ref()?.first_start)
                .unwrap_or_else(|| self.current_pos()),
            previous_end: (self.pos > 0).then_some(self.last_end),
            properties,
            methods,
            consts,
        }
    }

    /// Stops the parse where it is: the rest of the file is not read, as
    /// for `__halt_compiler()`, after which PHP reads no more code.
    pub(crate) fn skip_to_eof(&mut self) {
        self.ahead.clear();
        self.eof_queued = true;
        let eof = match &mut self.supply {
            Supply::Lexer(lexer) => lexer.skip_to_end(),
            Supply::List(tokens) => tokens.last().map_or(self.last_span, |tok| tok.span),
        };
        self.pos += 1;
        self.last_span = eof;
        self.last_end = eof.end;
    }

    /// What deleting a class member removes, for a member that began as
    /// `member` says and whose last token is the one just consumed: whole
    /// lines when nothing else shares them, else from its start up to the
    /// next token.
    pub(crate) fn member_removal_span(&self, member: MemberStart) -> Span {
        let start = member.start;
        let previous_line = member
            .previous_end
            .map_or(0, |end| self.line_index.line(end));
        let end = self.previous_end();
        let next = self.current_pos();
        let (start_line, end_line) = (self.line_index.line(start), self.line_index.line(end));
        if previous_line < start_line && self.line_index.line(next) > end_line {
            Span {
                start: self.line_index.line_start(start_line).unwrap_or(start),
                end: self.line_index.line_start(end_line + 1).unwrap_or(next),
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
        let span = Some(self.member_removal_span(start));
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
    pub(crate) fn previous_end(&self) -> u32 {
        self.last_end
    }

    /// Interns names into `interner` rather than the global one.
    pub fn with_interner(mut self, interner: &Arc<Interner>) -> Self {
        self.interner = Arc::clone(interner);
        self
    }

    /// A reference by name, not yet resolved.
    pub(crate) fn qualified_name(&self, written: Name, span: Span) -> QualifiedName {
        QualifiedName::new(written, span)
    }

    /// From `start` to the end of the last consumed token.
    pub(crate) fn span_from(&self, start: u32) -> Span {
        Span {
            start,
            end: self.previous_end(),
        }
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
    pub(crate) fn stmt(&self, kind: StmtKind, start: u32) -> Stmt {
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
    pub(crate) fn expr(&self, kind: ExprKind, start: u32) -> Expr {
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
        self.ahead.front_mut()?.docs.as_mut()?.last.take()
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
