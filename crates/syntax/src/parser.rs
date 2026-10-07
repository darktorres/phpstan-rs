use std::sync::Arc;

use crate::ast::*;
use crate::lexer::{LexEnd, Lexed, LexedDoc, Lexer, Tok};
use crate::name::{Interner, Name};
use crate::name_resolution::Resolution;
use crate::span::{LineIndex, Span};
use crate::token::{Keyword, Token};

/// Parse failure details.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseErrorData {
    pub message: String,
    pub span: Span,
}

/// A parse failure: malformed syntax.
///
/// Keep this boxed because parser results otherwise carry the message and
/// span beside every large recursive syntax node.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(Box<ParseErrorData>);

impl ParseError {
    pub(crate) fn new(message: impl Into<String>, span: Span) -> Self {
        Self(Box::new(ParseErrorData {
            message: message.into(),
            span,
        }))
    }
}

impl std::ops::Deref for ParseError {
    type Target = ParseErrorData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
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
    supply: std::cell::RefCell<Supply<'a>>,
    // The namespace and imports in effect where the parser is, which each
    // name it makes is resolved by, in the order PHP applies them.
    pub(crate) ctx: std::cell::RefCell<crate::name_resolution::NameContext>,
    // What the file refers to: the names as they are resolved, and the doc
    // comments with the names in effect where they are.
    pub(crate) recorder: std::cell::RefCell<crate::references::Recorder>,
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
    tracks_name_ids: bool,
    // The next indices to allocate until parse nodes are committed directly
    // into their arenas. Their values are checked against the span tables.
    pub(crate) next_expr_id: std::cell::Cell<u32>,
    pub(crate) next_stmt_id: std::cell::Cell<u32>,
    // Spans are parallel to node indices. Interpolation fragments have a
    // non-zero base and append their newly parsed spans to their parent. The
    // base lets their IDs match the parent file when their spans are merged.
    pub(crate) expr_span_base: u32,
    pub(crate) stmt_span_base: u32,
    node_arenas: std::cell::RefCell<NodeArenas>,
    // What names in the tree are interned into.
    pub(crate) interner: &'static Interner,
    // Errors that stand unless the node they're keyed by turns out to be
    // part of a destructuring target: a skipped array slot (`[, $b]`) and
    // a `list(...)`. Which it is is only known once the outermost `[...]`
    // is followed by `=`, so they wait until then.
    pub(crate) pattern_only: Vec<(ExprId, ParseError)>,
    // The `array(...)` literals, which can't be destructuring targets.
    pub(crate) long_arrays: rustc_hash::FxHashSet<ExprId>,
    // Whether the parse has built an anonymous class (`new class`).
    pub(crate) saw_anonymous_class: bool,
    // Where the parse first met each syntax only some PHP versions accept.
    pub(crate) syntax_marks: crate::lexer::SyntaxMarks,
    // How many closure and anonymous class bodies the parse is inside, and
    // whether it has made a class, function or constant declaration in one:
    // a declaration a walk of the statements does not reach.
    pub(crate) nested_depth: std::cell::Cell<u32>,
    pub(crate) saw_nested_declaration: std::cell::Cell<bool>,
    // Whether the parse has built an `unset` of a property.
    pub(crate) saw_property_unset: bool,
    // The labels `goto` statements name, one entry per function-like body
    // being parsed, the file's own first.
    pub(crate) goto_scopes: Vec<GotoScope>,
}

/// What a name is read as: PHP resolves classes, functions and constants
/// each by its own imports.
#[derive(Clone, Copy)]
pub(crate) enum NameKind {
    Class,
    Function,
    Constant,
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
        Self::from_lexer_with_node_arenas(lexer, NodeArenas::default())
    }

    pub(crate) fn from_lexer_with_node_arenas(
        lexer: Lexer<'a>,
        mut node_arenas: NodeArenas,
    ) -> Parser<'a> {
        let line_index = lexer.line_index();
        let tracks_name_ids = lexer.tracks_file_names();
        node_arenas.clear();
        Parser::with_supply_and_node_arenas(
            Supply::Lexer(Box::new(lexer)),
            line_index,
            node_arenas,
            tracks_name_ids,
        )
    }

    fn with_supply(supply: Supply<'a>, line_index: Arc<LineIndex>) -> Parser<'a> {
        Self::with_supply_and_node_arenas(supply, line_index, NodeArenas::default(), true)
    }

    fn with_supply_and_node_arenas(
        supply: Supply<'a>,
        line_index: Arc<LineIndex>,
        node_arenas: NodeArenas,
        tracks_name_ids: bool,
    ) -> Parser<'a> {
        let mut parser = Parser {
            supply: std::cell::RefCell::new(supply),
            ctx: std::cell::RefCell::default(),
            recorder: std::cell::RefCell::default(),
            ahead: std::collections::VecDeque::with_capacity(LOOKAHEAD + 2),
            eof_queued: false,
            pos: 0,
            last_end: 0,
            last_span: Span::default(),
            line_index,
            tracks_name_ids,
            next_expr_id: std::cell::Cell::new(0),
            next_stmt_id: std::cell::Cell::new(0),
            expr_span_base: 0,
            stmt_span_base: 0,
            node_arenas: std::cell::RefCell::new(node_arenas),
            interner: Interner::global(),
            pattern_only: Vec::new(),
            long_arrays: rustc_hash::FxHashSet::default(),
            saw_anonymous_class: false,
            syntax_marks: crate::lexer::SyntaxMarks::default(),
            nested_depth: std::cell::Cell::new(0),
            saw_nested_declaration: std::cell::Cell::new(false),
            saw_property_unset: false,
            goto_scopes: vec![GotoScope::default()],
        };
        parser.fill(LOOKAHEAD);
        parser
    }

    /// Queues tokens until the current one and `count` more are held, or
    /// the supply has no more.
    pub(crate) fn fill(&mut self, count: usize) {
        while !self.eof_queued && self.ahead.len() <= count {
            let Some(tok) = self.supply.get_mut().next() else {
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
        match self.supply.into_inner() {
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

    /// Whether the tree built so far has a class, function or constant
    /// declared in the body of a closure or an anonymous class.
    pub fn saw_nested_declaration(&self) -> bool {
        self.saw_nested_declaration.get()
    }

    /// Whether the tree built so far unsets a property.
    pub fn saw_property_unset(&self) -> bool {
        self.saw_property_unset
    }

    /// Where the parse first met each syntax only some PHP versions accept.
    pub fn syntax_marks(&self) -> crate::lexer::SyntaxMarks {
        self.syntax_marks
    }

    /// Reads the `,` after an item of a list `closer` ends, noting `mark`
    /// at it when it is the list's last: whether there was one.
    pub(crate) fn eat_list_comma(
        &mut self,
        closer: &Token,
        mark: crate::lexer::SyntaxMark,
    ) -> bool {
        let comma = self.current_span();
        if !self.eat(&Token::Comma) {
            return false;
        }
        if self.check(closer) {
            self.syntax_marks.note(mark, comma);
        }
        true
    }

    /// The span of the token about to be read: the last one's when none
    /// is left.
    pub(crate) fn current_span(&self) -> Span {
        self.ahead.front().map_or(self.last_span, |tok| tok.span)
    }

    /// The position a `StmtNode`/`ExprNode` about to be parsed starts at - call
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
        let eof = match self.supply.get_mut() {
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
    /// returning a `StmtNode`/`ExprNode` to close out its span. Same defensive
    /// fallback as `current_pos` for the same reason.
    pub(crate) fn previous_end(&self) -> u32 {
        self.last_end
    }

    /// How a name just parsed is read, by what follows it: a call's callee
    /// is a function, the class of a `::` is a class, and anything else is
    /// a constant.
    pub(crate) fn name_kind_ahead(&self) -> NameKind {
        match self.peek() {
            Token::DoubleColon => NameKind::Class,
            Token::LParen => NameKind::Function,
            _ => NameKind::Constant,
        }
    }

    /// The name for `text`, through the lexer's tables when it has them.
    pub(crate) fn intern(&self, text: &str) -> Name {
        match &mut *self.supply.borrow_mut() {
            Supply::Lexer(lexer) => lexer.intern(text),
            Supply::List(_) => self.interner.name(text),
        }
    }

    /// A reference by name, resolved as `kind` in the namespace and
    /// imports in effect.
    pub(crate) fn qualified_name(
        &self,
        written: Name,
        span: Span,
        kind: NameKind,
    ) -> QualifiedName {
        let name = self.unrecorded_name(written, span, kind);
        self.recorder.borrow_mut().name(&name, kind);
        name
    }

    /// `qualified_name`, for a name that is not yet known to be one the
    /// tree refers to: `record_name` records it when it is.
    pub(crate) fn unrecorded_name(
        &self,
        written: Name,
        span: Span,
        kind: NameKind,
    ) -> QualifiedName {
        let resolution = {
            let ctx = self.ctx.borrow();
            match kind {
                NameKind::Class => ctx.class_reference(&written),
                NameKind::Function => ctx.function_reference(&written),
                NameKind::Constant => ctx.constant_reference(&written),
            }
        };
        let resolved = match resolution {
            Resolution::Unresolved => ResolvedName::Unresolved,
            Resolution::Exact(text) => ResolvedName::Exact(self.resolved_name(&written, text)),
            Resolution::WithGlobalFallback(text) => {
                ResolvedName::WithGlobalFallback(self.resolved_name(&written, text))
            }
        };
        QualifiedName {
            written,
            resolved,
            span,
        }
    }

    /// Records a name made by `unrecorded_name` as a reference of `kind`.
    pub(crate) fn record_name(&self, name: &QualifiedName, kind: NameKind) {
        self.recorder.borrow_mut().name(name, kind);
    }

    /// What the file refers to, once it is parsed.
    pub fn take_references(&mut self) -> crate::references::FileReferences {
        std::mem::take(self.recorder.get_mut()).into_references()
    }

    /// Records `doc`, the doc comment of the node that starts at `pos`.
    pub(crate) fn note_doc(&self, doc: &Option<Arc<str>>, pos: u32) {
        if doc.is_some() {
            self.recorder.borrow_mut().doc(doc, pos, &self.ctx.borrow());
        }
    }

    /// Records the doc comment of a function or method, and what it
    /// encloses.
    pub(crate) fn note_function(&self, function: &FunctionDecl) {
        self.note_doc(&function.doc_comment, function.span.start);
        self.recorder.borrow_mut().frame(
            &function.doc_comment,
            function.span.start,
            function.span.end,
        );
    }

    /// Records the doc comment of a class-like, and what it encloses.
    pub(crate) fn note_class(&self, def: &ClassDef) {
        self.note_doc(&def.doc_comment, def.span.start);
        self.recorder
            .borrow_mut()
            .frame(&def.doc_comment, def.span.start, def.span.end);
    }

    /// The name a resolution gave: the one written when it is that.
    fn resolved_name(&self, written: &Name, text: String) -> Name {
        if text == written.as_str() {
            written.clone()
        } else {
            self.intern(&text)
        }
    }

    /// From `start` to the end of the last consumed token.
    pub(crate) fn span_from(&self, start: u32) -> Span {
        Span {
            start,
            end: self.previous_end(),
        }
    }

    /// Reserves the index the next expression will have in its typed arena.
    pub(crate) fn expr_id(&self) -> ExprId {
        let id = self.next_expr_id.get();
        self.next_expr_id
            .set(id.checked_add(1).expect("expression arena exhausted"));
        ExprId::from_raw(la_arena::RawIdx::from_u32(id))
    }

    /// Reserves the index the next statement will have in its typed arena.
    pub(crate) fn stmt_id(&self) -> StmtId {
        let id = self.next_stmt_id.get();
        self.next_stmt_id
            .set(id.checked_add(1).expect("statement arena exhausted"));
        StmtId::from_raw(la_arena::RawIdx::from_u32(id))
    }

    /// Builds a `StmtNode` spanning from `start` (the position captured before
    /// this statement's first token was consumed) to whatever token was
    /// last consumed.
    pub(crate) fn stmt(&self, mut kind: StmtKind, start: u32) -> StmtNode {
        // What a statement changes in the names that follow it, and the
        // names a declaration gets, are settled as it is made: PHP reads a
        // file in this order.
        match &mut kind {
            StmtKind::Namespace(name) => self.ctx.borrow_mut().set_namespace(name),
            StmtKind::Use(imports) => {
                self.ctx.borrow_mut().add_uses(imports);
                let mut recorder = self.recorder.borrow_mut();
                for import in imports.iter() {
                    recorder.import(import.kind, &import.path);
                }
            }
            StmtKind::Global(vars) => {
                let mut recorder = self.recorder.borrow_mut();
                for name in vars.iter().filter_map(|v| v.name()) {
                    recorder.global_variable(name);
                }
            }
            StmtKind::FunctionDecl(f) => {
                f.namespaced_name = Some(self.ctx.borrow().declared_name(&f.name).into());
            }
            StmtKind::ClassDecl(c) => {
                c.namespaced_name = self.ctx.borrow().declared_name(&c.name).into();
            }
            StmtKind::GlobalConst(consts) => {
                let ctx = self.ctx.borrow();
                for c in consts {
                    c.namespaced_name = Some(ctx.declared_name(&c.name).into());
                }
            }
            _ => {}
        }
        if self.nested_depth.get() > 0
            && matches!(
                kind,
                StmtKind::ClassDecl(_) | StmtKind::FunctionDecl(_) | StmtKind::GlobalConst(_)
            )
        {
            self.saw_nested_declaration.set(true);
        }
        let span = Span {
            start,
            end: self.previous_end(),
        };
        let id = self.stmt_id();
        let mut arenas = self.node_arenas.borrow_mut();
        let span_index = self.stmt_span_base + arenas.stmt_spans.len() as u32;
        debug_assert_eq!(u32::from(id.into_raw()), span_index);
        arenas.stmt_spans.push(span);
        StmtNode {
            id,
            kind,
            span,
            doc_comment: None,
            doc_line: None,
        }
    }

    /// Same as `stmt`, for expressions.
    pub(crate) fn expr(&self, kind: ExprKind, start: u32) -> ExprNode {
        self.expr_at(
            kind,
            Span {
                start,
                end: self.previous_end(),
            },
        )
    }

    /// Builds an expression whose span came directly from a lexer fragment.
    pub(crate) fn expr_at(&self, kind: ExprKind, span: Span) -> ExprNode {
        let id = self.expr_id();
        let mut arenas = self.node_arenas.borrow_mut();
        let span_index = self.expr_span_base + arenas.expr_spans.len() as u32;
        debug_assert_eq!(u32::from(id.into_raw()), span_index);
        arenas.expr_spans.push(span);
        ExprNode { id, kind, span }
    }

    /// Takes the span side tables built by this parser. A fragment parser's
    /// entries begin at its base and are appended by the parent parser.
    pub(crate) fn take_node_spans(&self) -> (Vec<Span>, Vec<Span>) {
        let mut arenas = self.node_arenas.borrow_mut();
        (
            std::mem::take(&mut arenas.expr_spans),
            std::mem::take(&mut arenas.stmt_spans),
        )
    }

    pub(crate) fn take_node_arenas(&self) -> NodeArenas {
        std::mem::take(&mut *self.node_arenas.borrow_mut())
    }

    /// Updates the parallel span when parsing later extends a node's span.
    pub(crate) fn set_expr_span(&self, id: ExprId, span: Span) {
        let index = u32::from(id.into_raw()) - self.expr_span_base;
        self.node_arenas.borrow_mut().expr_spans[index as usize] = span;
    }

    /// Consumes any doc comment immediately preceding the current position -
    /// call this before advancing past the first token of a declaration.
    pub(crate) fn take_doc_comment(&mut self) -> Option<Arc<str>> {
        self.ahead.front_mut()?.docs.as_mut()?.last.take()
    }

    #[hotpath::measure]
    pub fn parse_program(&mut self) -> Result<Vec<StmtNode>, ParseError> {
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
        Ok(stmts)
    }

    /// The older, braced namespace form - `namespace Foo\Bar { ... }`, or
    /// bare `namespace { ... }` for the global namespace - as opposed to
    /// the common `namespace Foo\Bar;` statement form (still handled
    /// generically in parse_statement). Only legal at the top level of a
    /// file, so this is special-cased in parse_program rather than making
    /// every parse_statement caller handle getting more than one StmtNode
    /// back. The block's body is spliced into the surrounding statement
    /// list after a `StmtNode::Namespace` marker, the same shape the statement
    /// form gives.
    pub(crate) fn parse_namespace_stmt(&mut self) -> Result<Vec<StmtNode>, ParseError> {
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
