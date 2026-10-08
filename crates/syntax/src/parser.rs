use std::sync::Arc;

use crate::ast::*;
use crate::lexer::{LexEnd, LexOutput, Lexed, Lexer};
use crate::name::{Interner, Name};
use crate::name_resolution::Resolution;
use crate::span::{LineIndex, Span};
use crate::token::{Keyword, Token, TokenKind, TokenPayload, TokenRef, TokenStream};

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

/// A standalone parsed program and the file-local arenas its IDs address.
#[derive(Debug)]
pub struct ParsedProgram {
    pub stmts: StmtIdRange,
    pub arenas: NodeArenas,
}

impl ParseError {
    pub(crate) fn new(message: impl Into<String>, span: Span) -> Self {
        Self(Box::new(ParseErrorData {
            message: message.into(),
            span,
        }))
    }
}

impl ParsedProgram {
    /// Resolves the root statement range through its owning arena.
    pub fn statements(&self) -> &[StmtId] {
        self.arenas.stmt_ids(&self.stmts)
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
    // Flat whole-file tokens; parser lookahead indexes the arrays directly.
    token_stream: TokenStream,
    // The namespace and imports in effect where the parser is, which each
    // name it makes is resolved by, in the order PHP applies them.
    pub(crate) ctx: std::cell::RefCell<crate::name_resolution::NameContext>,
    // What the file refers to: the names as they are resolved, and the doc
    // comments with the names in effect where they are.
    pub(crate) recorder: std::cell::RefCell<crate::references::Recorder>,
    // Text fallback for token-list parser constructors that have no source
    // owner. The live lexer-backed path resolves ranges directly from source.
    doc_texts: std::cell::RefCell<rustc_hash::FxHashMap<DocCommentRanges, Arc<str>>>,
    source: Option<&'a [u8]>,
    // Independent cursors through token kinds, sparse payloads, and docs.
    pub(crate) pos: usize,
    payload_pos: usize,
    doc_pos: usize,
    current_docs: Option<Box<crate::lexer::DocGroup>>,
    last_name_id: Option<crate::name::NameId>,
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
    // Reused for complete statement lists before their elements are
    // committed together to `NodeArenas::stmt_ids`.
    stmt_list_scratch: Vec<StmtId>,
    // Reused for expression-ID lists before their elements are committed
    // together to `NodeArenas::expr_ids`.
    expr_id_list_scratch: Vec<ExprId>,
    arg_list_scratch: Vec<Arg>,
    array_item_scratch: Vec<ArrayItem>,
    match_arm_scratch: Vec<MatchArm>,
    elseif_scratch: Vec<ElseIf>,
    switch_case_scratch: Vec<SwitchCase>,
    catch_clause_scratch: Vec<CatchClause>,
    declare_directive_scratch: Vec<DeclareDirective>,
    global_var_scratch: Vec<GlobalVar>,
    static_var_scratch: Vec<StaticVarDecl>,
    property_scratch: Vec<PropertyDecl>,
    method_scratch: Vec<MethodDecl>,
    class_const_scratch: Vec<ClassConst>,
    trait_use_scratch: Vec<TraitUse>,
    enum_case_scratch: Vec<EnumCase>,
    qualified_name_scratch: Vec<QualifiedName>,
    use_import_scratch: Vec<UseImport>,
    trait_alias_scratch: Vec<TraitAlias>,
    trait_precedence_scratch: Vec<TraitPrecedence>,
    closure_use_scratch: Vec<ClosureUse>,
    type_hint_scratch: Vec<TypeHint>,
    param_list_scratch: Vec<Vec<ParamDraft>>,
    param_list_depth: usize,
    destructure_item_scratch: Vec<DestructureItem>,
    // What names in the tree are interned into.
    pub(crate) interner: &'static Interner,
    pub(crate) pattern_only: Vec<(ExprId, ParseError)>,
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
    // Whether the opt-in PHPStan stub-attribute rules changed a declaration
    // while it was being built. A caller then rebuilds reference products
    // from this final tree instead of using records from discarded nodes.
    pub(crate) stub_attributes_changed: std::cell::Cell<bool>,
    // Whether any parsed attribute resolves to one of the historical stub
    // attributes. Once false, parser hooks can skip the stub-rule scans.
    pub(crate) stub_attributes_mentioned: bool,
    pub(crate) goto_scopes: Vec<GotoScope>,
}

/// Reusable temporary buffers for parsing one file. The worker can retain
/// this after extracting it from a parser and pass it to the next parser;
/// `clear` drops elements while keeping each buffer's allocation.
#[derive(Default)]
pub struct ParserScratch {
    // Reused for complete statement lists before their elements are
    // committed together to `NodeArenas::stmt_ids`.
    stmt_list_scratch: Vec<StmtId>,
    // Reused for expression-ID lists before their elements are committed
    // together to `NodeArenas::expr_ids`.
    expr_id_list_scratch: Vec<ExprId>,
    // Reused for argument lists while nested expressions are parsed.
    arg_list_scratch: Vec<Arg>,
    // Reused for array elements while nested expressions are parsed.
    array_item_scratch: Vec<ArrayItem>,
    // Reused for match arms while their conditions and bodies are parsed.
    match_arm_scratch: Vec<MatchArm>,
    // Reused for statement payload lists while nested statements are parsed.
    elseif_scratch: Vec<ElseIf>,
    switch_case_scratch: Vec<SwitchCase>,
    catch_clause_scratch: Vec<CatchClause>,
    declare_directive_scratch: Vec<DeclareDirective>,
    global_var_scratch: Vec<GlobalVar>,
    static_var_scratch: Vec<StaticVarDecl>,
    // Reused for class-body member lists while nested declarations are parsed.
    property_scratch: Vec<PropertyDecl>,
    method_scratch: Vec<MethodDecl>,
    class_const_scratch: Vec<ClassConst>,
    trait_use_scratch: Vec<TraitUse>,
    enum_case_scratch: Vec<EnumCase>,
    qualified_name_scratch: Vec<QualifiedName>,
    use_import_scratch: Vec<UseImport>,
    trait_alias_scratch: Vec<TraitAlias>,
    trait_precedence_scratch: Vec<TraitPrecedence>,
    closure_use_scratch: Vec<ClosureUse>,
    type_hint_scratch: Vec<TypeHint>,
    // One reusable parameter buffer per nesting depth; defaults may contain
    // closures with their own parameter lists.
    param_list_scratch: Vec<Vec<ParamDraft>>,
    param_list_depth: usize,
    // Reused for destructuring patterns before their elements are committed
    // together to `NodeArenas::destructure_items`.
    destructure_item_scratch: Vec<DestructureItem>,
    // Parser-local state that accumulates while parsing and is empty again
    // once its owning expressions/scopes have been committed.
    pattern_only: Vec<(ExprId, ParseError)>,
    long_arrays: rustc_hash::FxHashSet<ExprId>,
    goto_scopes: Vec<GotoScope>,
}

impl ParserScratch {
    fn clear(&mut self) {
        self.stmt_list_scratch.clear();
        self.expr_id_list_scratch.clear();
        self.arg_list_scratch.clear();
        self.array_item_scratch.clear();
        self.match_arm_scratch.clear();
        self.elseif_scratch.clear();
        self.switch_case_scratch.clear();
        self.catch_clause_scratch.clear();
        self.declare_directive_scratch.clear();
        self.global_var_scratch.clear();
        self.static_var_scratch.clear();
        self.property_scratch.clear();
        self.method_scratch.clear();
        self.class_const_scratch.clear();
        self.trait_use_scratch.clear();
        self.enum_case_scratch.clear();
        self.qualified_name_scratch.clear();
        self.use_import_scratch.clear();
        self.trait_alias_scratch.clear();
        self.trait_precedence_scratch.clear();
        self.closure_use_scratch.clear();
        self.type_hint_scratch.clear();
        for params in &mut self.param_list_scratch {
            params.clear();
        }
        self.param_list_depth = 0;
        self.destructure_item_scratch.clear();
        self.pattern_only.clear();
        self.long_arrays.clear();
        self.goto_scopes.clear();
        self.goto_scopes.push(GotoScope::default());
    }
}

/// A parser-only parameter plus its doc range. The retained `Param` stays
/// compact; the doc is copied into the owner's offset-indexed doc sites
/// after parser-time stub rewrites finish.
pub(crate) struct ParamDraft {
    pub(crate) node: Param,
    pub(crate) doc_comment: Option<DocCommentRanges>,
}

/// What a name is read as: PHP resolves classes, functions and constants
/// each by its own imports.
#[derive(Clone, Copy)]
pub(crate) enum NameKind {
    Class,
    Function,
    Constant,
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
mod stub_attributes;
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

fn fold_doc_text<'a>(docs: impl Iterator<Item = &'a str>) -> Arc<str> {
    let bodies = docs
        .map(|doc| doc.trim().trim_start_matches("/**").trim_end_matches("*/"))
        .collect::<Vec<_>>();
    Arc::from(format!("/**{}\n*/", bodies.join("\n")))
}

impl<'a> Parser<'a> {
    /// Compatibility adapter for callers that still build a token vector.
    pub fn new(tokens: Vec<(Token, Span, Option<String>)>) -> Parser<'static> {
        let mut lexed_tokens = Vec::with_capacity(tokens.len());
        let mut spans = Vec::with_capacity(tokens.len());
        let mut keyword_raw = Vec::new();
        let mut docs = Vec::new();
        for (token, span, raw) in tokens {
            match token {
                Token::DocComment(text) => docs.push(crate::lexer::LexedDoc {
                    before: lexed_tokens.len(),
                    span,
                    text,
                }),
                other => {
                    if let Some(raw) = raw {
                        keyword_raw.push((lexed_tokens.len(), raw));
                    }
                    lexed_tokens.push(other);
                    spans.push(span);
                }
            }
        }
        Parser::from_lexed(Lexed {
            tokens: lexed_tokens,
            spans,
            keyword_raw,
            docs,
            line_index: Arc::default(),
        })
    }

    /// A parser of legacy lexer output, retained while readers migrate.
    pub fn from_lexed(lexed: Lexed) -> Parser<'static> {
        Parser::<'static>::from_token_stream(TokenStream::from_lexed_parts(LexOutput {
            lexed,
            comments: Vec::new(),
            ignore_marks: Vec::new(),
            warnings: Vec::new(),
            facts: crate::lexer::FileFacts::default(),
            names: Box::default(),
            decoded_bytes: Vec::new(),
        }))
    }

    /// Parses an already lexed flat stream.
    pub fn from_token_stream(stream: TokenStream) -> Parser<'static> {
        Parser::<'static>::from_token_stream_with_node_arenas(
            stream,
            NodeArenas::default(),
            None,
        )
    }

    /// Creates a parser using reusable transient buffers supplied by a
    /// worker. The caller recovers them with [`Parser::take_scratch`] after
    /// parsing, including when parsing returned an error.
    pub fn from_token_stream_with_scratch(
        stream: TokenStream,
        arenas: NodeArenas,
        source: Option<&'a [u8]>,
        scratch: ParserScratch,
        tracks_name_ids: bool,
    ) -> Parser<'a> {
        Self::from_token_stream_with_node_arenas_and_scratch(
            stream,
            arenas,
            source,
            scratch,
            tracks_name_ids,
        )
    }

    /// A parser that consumes the lexer's complete flat stream.
    pub fn from_lexer(lexer: Lexer<'a>) -> Parser<'a> {
        let stream = lexer.with_source_backed_strings(false).lex_token_stream();
        Self::from_token_stream_with_node_arenas(stream, NodeArenas::default(), None)
    }

    pub(crate) fn from_lexer_with_node_arenas(
        mut lexer: Lexer<'a>,
        mut node_arenas: NodeArenas,
        tracks_name_ids: bool,
    ) -> Parser<'a> {
        let source = lexer.source_bytes();
        node_arenas.clear_preserving_bases();
        lexer.set_decoded_bytes_base(node_arenas.decoded_bytes_base);
        Self::from_token_stream_with_node_arenas_and_scratch(
            lexer.lex_token_stream(),
            node_arenas,
            Some(source),
            ParserScratch::default(),
            tracks_name_ids,
        )
    }

    pub(crate) fn from_token_stream_with_node_arenas(
        token_stream: TokenStream,
        node_arenas: NodeArenas,
        source: Option<&'a [u8]>,
    ) -> Parser<'a> {
        let tracks_name_ids = !token_stream.names.is_empty();
        Self::from_token_stream_with_node_arenas_and_scratch(
            token_stream,
            node_arenas,
            source,
            ParserScratch::default(),
            tracks_name_ids,
        )
    }

    fn from_token_stream_with_node_arenas_and_scratch(
        mut token_stream: TokenStream,
        mut node_arenas: NodeArenas,
        source: Option<&'a [u8]>,
        mut scratch: ParserScratch,
        tracks_name_ids: bool,
    ) -> Parser<'a> {
        scratch.clear();
        node_arenas.clear_preserving_bases();
        node_arenas
            .decoded_bytes
            .append(&mut token_stream.decoded_bytes);
        let expr_base = node_arenas.expr_base;
        let stmt_base = node_arenas.stmt_base;
        let line_index = Arc::clone(&token_stream.line_index);
        let mut parser = Parser {
            token_stream,
            ctx: std::cell::RefCell::default(),
            recorder: std::cell::RefCell::default(),
            doc_texts: std::cell::RefCell::default(),
            source,
            pos: 0,
            payload_pos: 0,
            doc_pos: 0,
            current_docs: None,
            last_name_id: None,
            last_end: 0,
            last_span: Span::default(),
            line_index,
            tracks_name_ids,
            next_expr_id: std::cell::Cell::new(expr_base),
            next_stmt_id: std::cell::Cell::new(stmt_base),
            expr_span_base: expr_base,
            stmt_span_base: stmt_base,
            node_arenas: std::cell::RefCell::new(node_arenas),
            stmt_list_scratch: scratch.stmt_list_scratch,
            expr_id_list_scratch: scratch.expr_id_list_scratch,
            arg_list_scratch: scratch.arg_list_scratch,
            array_item_scratch: scratch.array_item_scratch,
            match_arm_scratch: scratch.match_arm_scratch,
            elseif_scratch: scratch.elseif_scratch,
            switch_case_scratch: scratch.switch_case_scratch,
            catch_clause_scratch: scratch.catch_clause_scratch,
            declare_directive_scratch: scratch.declare_directive_scratch,
            global_var_scratch: scratch.global_var_scratch,
            static_var_scratch: scratch.static_var_scratch,
            property_scratch: scratch.property_scratch,
            method_scratch: scratch.method_scratch,
            class_const_scratch: scratch.class_const_scratch,
            trait_use_scratch: scratch.trait_use_scratch,
            enum_case_scratch: scratch.enum_case_scratch,
            qualified_name_scratch: scratch.qualified_name_scratch,
            use_import_scratch: scratch.use_import_scratch,
            trait_alias_scratch: scratch.trait_alias_scratch,
            trait_precedence_scratch: scratch.trait_precedence_scratch,
            closure_use_scratch: scratch.closure_use_scratch,
            type_hint_scratch: scratch.type_hint_scratch,
            param_list_scratch: scratch.param_list_scratch,
            param_list_depth: scratch.param_list_depth,
            destructure_item_scratch: scratch.destructure_item_scratch,
            interner: Interner::global(),
            pattern_only: scratch.pattern_only,
            long_arrays: scratch.long_arrays,
            saw_anonymous_class: false,
            syntax_marks: crate::lexer::SyntaxMarks::default(),
            nested_depth: std::cell::Cell::new(0),
            saw_nested_declaration: std::cell::Cell::new(false),
            saw_property_unset: false,
            stub_attributes_changed: std::cell::Cell::new(false),
            stub_attributes_mentioned: false,
            goto_scopes: scratch.goto_scopes,
        };
        parser.load_current_docs();
        parser
    }

    fn load_current_docs(&mut self) {
        let first = self.doc_pos;
        while self.doc_pos < self.token_stream.docs.len()
            && self.token_stream.docs[self.doc_pos].before as usize == self.pos
        {
            self.doc_pos += 1;
        }
        let entries = &self.token_stream.docs[first..self.doc_pos];
        self.current_docs = match entries {
            [] => None,
            [doc] => Some(Box::new(crate::lexer::DocGroup {
                last: Some(Arc::clone(&doc.text)),
                last_range: Some(doc.span),
                folded: None,
                folded_ranges: None,
                first_start: Some(doc.span.start),
            })),
            several => {
                let folded_text = fold_doc_text(several.iter().map(|doc| doc.text.as_ref()));
                Some(Box::new(crate::lexer::DocGroup {
                    last: Some(Arc::clone(&several[several.len() - 1].text)),
                    last_range: Some(several[several.len() - 1].span),
                    folded: Some(Arc::clone(&folded_text)),
                    folded_ranges: Some(DocCommentRanges::group(
                        several.iter().map(|doc| doc.span).collect::<Vec<_>>(),
                    )),
                    first_start: Some(several[0].span.start),
                }))
            }
        };
        if self.source.is_none()
            && let Some(group) = self.current_docs.as_deref()
        {
            let mut texts = self.doc_texts.borrow_mut();
            if let (Some(span), Some(text)) = (group.last_range, group.last.as_ref()) {
                texts.insert(DocCommentRanges::One(span), Arc::clone(text));
            }
            if let (Some(ranges), Some(text)) =
                (group.folded_ranges.as_ref(), group.folded.as_ref())
            {
                texts.insert(ranges.clone(), Arc::clone(text));
            }
        }
    }

    /// Advances one token and keeps the payload cursor aligned with the
    /// sparse payload table.
    pub(crate) fn pop(&mut self) {
        let kind = self.token_stream.kind_at(self.pos);
        self.last_span = self.token_stream.span_at(self.pos);
        self.last_end = self.last_span.end;
        if self.payload_pos < self.token_stream.payloads.len()
            && self.token_stream.payloads[self.payload_pos].token as usize == self.pos
        {
            self.payload_pos += 1;
        }
        self.pos += 1;
        self.last_name_id = None;
        self.current_docs = None;
        if kind != TokenKind::Eof {
            self.load_current_docs();
        }
    }

    pub(crate) fn token_ref(&self, offset: usize) -> TokenRef<'_> {
        self.token_stream.token_at(self.pos.saturating_add(offset))
    }

    pub(crate) fn take_payload(&mut self) -> Option<TokenPayload> {
        let entry = self.token_stream.payloads.get_mut(self.payload_pos)?;
        if entry.token as usize != self.pos {
            return None;
        }
        Some(std::mem::replace(&mut entry.payload, TokenPayload::Empty))
    }

    pub(crate) fn remember_name_id(&mut self, id: Option<crate::name::NameId>) {
        self.last_name_id = id;
    }

    /// Returns cleared transient buffers to their owner while preserving
    /// allocations for reuse by the next parser.
    pub fn take_scratch(&mut self) -> ParserScratch {
        let mut scratch = ParserScratch {
            stmt_list_scratch: std::mem::take(&mut self.stmt_list_scratch),
            expr_id_list_scratch: std::mem::take(&mut self.expr_id_list_scratch),
            arg_list_scratch: std::mem::take(&mut self.arg_list_scratch),
            array_item_scratch: std::mem::take(&mut self.array_item_scratch),
            match_arm_scratch: std::mem::take(&mut self.match_arm_scratch),
            elseif_scratch: std::mem::take(&mut self.elseif_scratch),
            switch_case_scratch: std::mem::take(&mut self.switch_case_scratch),
            catch_clause_scratch: std::mem::take(&mut self.catch_clause_scratch),
            declare_directive_scratch: std::mem::take(&mut self.declare_directive_scratch),
            global_var_scratch: std::mem::take(&mut self.global_var_scratch),
            static_var_scratch: std::mem::take(&mut self.static_var_scratch),
            property_scratch: std::mem::take(&mut self.property_scratch),
            method_scratch: std::mem::take(&mut self.method_scratch),
            class_const_scratch: std::mem::take(&mut self.class_const_scratch),
            trait_use_scratch: std::mem::take(&mut self.trait_use_scratch),
            enum_case_scratch: std::mem::take(&mut self.enum_case_scratch),
            qualified_name_scratch: std::mem::take(&mut self.qualified_name_scratch),
            use_import_scratch: std::mem::take(&mut self.use_import_scratch),
            trait_alias_scratch: std::mem::take(&mut self.trait_alias_scratch),
            trait_precedence_scratch: std::mem::take(&mut self.trait_precedence_scratch),
            closure_use_scratch: std::mem::take(&mut self.closure_use_scratch),
            type_hint_scratch: std::mem::take(&mut self.type_hint_scratch),
            param_list_scratch: std::mem::take(&mut self.param_list_scratch),
            param_list_depth: std::mem::take(&mut self.param_list_depth),
            destructure_item_scratch: std::mem::take(&mut self.destructure_item_scratch),
            pattern_only: std::mem::take(&mut self.pattern_only),
            long_arrays: std::mem::take(&mut self.long_arrays),
            goto_scopes: std::mem::take(&mut self.goto_scopes),
        };
        scratch.clear();
        scratch
    }

    /// Moves the consumed token tables out so a worker can reuse their
    /// allocations for its next file.
    pub fn take_token_stream(&mut self) -> TokenStream {
        std::mem::take(&mut self.token_stream)
    }

    /// Extracts lexer metadata before the token stream is retained for reuse.
    pub fn take_lex_end(&mut self, retain_names: bool) -> Option<LexEnd> {
        Some(LexEnd {
            ignore_marks: std::mem::take(&mut self.token_stream.ignore_marks),
            warnings: std::mem::take(&mut self.token_stream.warnings),
            facts: std::mem::take(&mut self.token_stream.facts),
            line_index: Arc::clone(&self.token_stream.line_index),
            names: if retain_names {
                std::mem::take(&mut self.token_stream.names).into_boxed_slice()
            } else {
                Box::default()
            },
            decoded_bytes: Vec::new(),
        })
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
        closer: &TokenRef<'_>,
        mark: crate::lexer::SyntaxMark,
    ) -> bool {
        let comma = self.current_span();
        if !self.eat(&TokenRef::Comma) {
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
        self.token_stream.span_at(self.pos)
    }

    /// The position a `StmtId`/`ExprId` about to be parsed starts at - call
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
        MemberStart {
            start: self
                .current_docs
                .as_deref()
                .and_then(|docs| docs.first_start)
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
        let last = self.token_stream.kinds.len().saturating_sub(1);
        let eof = self.token_stream.span_at(last);
        self.pos = last;
        self.payload_pos = self.token_stream.payloads.len();
        self.doc_pos = self.token_stream.docs.len();
        self.current_docs = None;
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
    pub(crate) fn finish_member_at(
        start: MemberStart,
        span: Span,
        properties: &mut [PropertyDecl],
        methods: &mut [MethodDecl],
        consts: &mut [ClassConst],
    ) {
        let added = (
            properties.len() - start.properties,
            methods.len() - start.methods,
            consts.len() - start.consts,
        );
        let span = Some(span);
        match added {
            (1, 0, 0) => properties[start.properties].removal_span = span,
            (0, 1, 0) => methods[start.methods].removal_span = span,
            (0, 0, 1) => consts[start.consts].removal_span = span,
            _ => {}
        }
    }

    /// The end of the last token actually consumed - call right before
    /// returning a `StmtId`/`ExprId` to close out its span. Same defensive
    /// fallback as `current_pos` for the same reason.
    pub(crate) fn previous_end(&self) -> u32 {
        self.last_end
    }

    /// How a name just parsed is read, by what follows it: a call's callee
    /// is a function, the class of a `::` is a class, and anything else is
    /// a constant.
    pub(crate) fn name_kind_ahead(&self) -> NameKind {
        match self.token_ref(0) {
            TokenRef::DoubleColon => NameKind::Class,
            TokenRef::LParen => NameKind::Function,
            _ => NameKind::Constant,
        }
    }

    /// The name for `text`, through the lexer's tables when it has them.
    pub(crate) fn intern(&self, text: &str) -> Name {
        self.interner.name(text)
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
    pub(crate) fn note_doc(&self, doc: &Option<DocCommentRanges>, pos: u32) {
        self.record_doc(doc, pos, false);
    }

    /// Records a statement or parameter doc both for dependency collection
    /// and for compact lookup by its owning node's source offset.
    pub(crate) fn note_node_doc_site(&self, doc: &Option<DocCommentRanges>, pos: u32) {
        self.record_doc(doc, pos, true);
    }

    fn record_doc(&self, doc: &Option<DocCommentRanges>, pos: u32, node_site: bool) {
        let Some((range, text)) = doc
            .as_ref()
            .and_then(|range| self.doc_text(range).map(|text| (range, text)))
        else {
            return;
        };
        if node_site {
            let line = range.spans().first().map_or_else(
                || self.line_index.line(pos),
                |span| self.line_index.line(span.start),
            );
            self.node_arenas
                .borrow_mut()
                .doc_sites
                .push(crate::ast::NodeDocSite {
                    pos,
                    line,
                    text: Arc::clone(&text),
                });
        }
        self.recorder
            .borrow_mut()
            .doc(&Some(text), pos, &self.ctx.borrow());
    }

    /// Resolves a doc range from the live file source, or uses the token-list
    /// fallback where a parser was built without a source owner.
    fn doc_text(&self, range: &DocCommentRanges) -> Option<Arc<str>> {
        self.source
            .and_then(|source| range.text(source))
            .map(|text| Arc::<str>::from(text.as_ref()))
            .or_else(|| self.doc_texts.borrow().get(range).cloned())
    }

    /// Records the doc comment of a function or method, and what it
    /// encloses.
    pub(crate) fn note_function(&self, function: &FunctionDecl) {
        self.note_doc(&function.doc_comment, function.span.start);
        let doc = function
            .doc_comment
            .as_ref()
            .and_then(|range| self.doc_text(range));
        self.recorder
            .borrow_mut()
            .frame(&doc, function.span.start, function.span.end);
    }

    /// Records the doc comment of a class-like, and what it encloses.
    pub(crate) fn note_class(&self, def: &ClassDef) {
        self.note_doc(&def.doc_comment, def.span.start);
        let doc = def
            .doc_comment
            .as_ref()
            .and_then(|range| self.doc_text(range));
        self.recorder
            .borrow_mut()
            .frame(&doc, def.span.start, def.span.end);
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

    /// Builds a `StmtId` spanning from `start` (the position captured before
    /// this statement's first token was consumed) to whatever token was
    /// last consumed.
    pub(crate) fn stmt(&self, mut kind: StmtKind, start: u32) -> StmtId {
        // What a statement changes in the names that follow it, and the
        // names a declaration gets, are settled as it is made: PHP reads a
        // file in this order.
        match &mut kind {
            StmtKind::Namespace(name) => self.ctx.borrow_mut().set_namespace(name),
            StmtKind::Use(imports) => {
                let arenas = self.node_arenas.borrow();
                let imports = arenas.use_imports(imports);
                self.ctx.borrow_mut().add_uses(imports);
                let mut recorder = self.recorder.borrow_mut();
                for import in imports {
                    recorder.import(import.kind, &import.path);
                }
            }
            StmtKind::Global(vars) => {
                let range = vars.clone();
                let arenas = self.node_arenas.borrow();
                let mut recorder = self.recorder.borrow_mut();
                for name in arenas.global_vars(range).iter().filter_map(GlobalVar::name) {
                    recorder.global_variable(name);
                }
            }
            StmtKind::FunctionDecl(id) => {
                let name = self
                    .node_arenas
                    .borrow()
                    .function(*id)
                    .expect("function declaration was just allocated")
                    .name
                    .clone();
                let namespaced_name = self.ctx.borrow().declared_name(&name).into();
                self.node_arenas
                    .borrow_mut()
                    .function_mut(*id)
                    .expect("function declaration was just allocated")
                    .namespaced_name = Some(namespaced_name);
            }
            StmtKind::ClassDecl(c) => {
                let namespaced_name = {
                    let arenas = self.node_arenas.borrow();
                    let decl = arenas.class_decl(c.clone());
                    self.ctx.borrow().declared_name(&decl.name).into()
                };
                self.node_arenas
                    .borrow_mut()
                    .class_decl_mut(c.clone())
                    .namespaced_name = namespaced_name;
            }
            StmtKind::GlobalConst(consts) => {
                let ctx = self.ctx.borrow();
                for c in self.node_arenas.borrow_mut().class_consts_mut(consts) {
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
        let node = StmtNode { id, kind };
        self.node_arenas.borrow_mut().alloc_stmt(node, span)
    }

    /// Same as `stmt`, for expressions.
    pub(crate) fn expr(&self, kind: ExprKind, start: u32) -> ExprId {
        self.expr_at(
            kind,
            Span {
                start,
                end: self.previous_end(),
            },
        )
    }

    /// Builds an expression whose span came directly from a lexer fragment.
    pub(crate) fn expr_at(&self, kind: ExprKind, span: Span) -> ExprId {
        let id = self.expr_id();
        self.node_arenas
            .borrow_mut()
            .alloc_expr(ExprNode { id, kind }, span)
    }

    pub(crate) fn take_node_arenas(&self) -> NodeArenas {
        let mut arenas = std::mem::take(&mut *self.node_arenas.borrow_mut());
        arenas.doc_sites.sort_by_key(|site| site.pos);
        arenas
    }

    pub(crate) fn begin_stmt_list(&self) -> usize {
        self.stmt_list_scratch.len()
    }

    pub(crate) fn push_stmt_list_item(&mut self, id: StmtId) {
        self.stmt_list_scratch.push(id);
    }

    pub(crate) fn finish_stmt_list(&mut self, start: usize) -> StmtIdRange {
        self.node_arenas
            .borrow_mut()
            .alloc_stmt_ids(self.stmt_list_scratch.drain(start..))
    }

    pub(crate) fn discard_stmt_list(&mut self, start: usize) {
        self.stmt_list_scratch.truncate(start);
    }

    pub(crate) fn begin_expr_id_list(&self) -> usize {
        self.expr_id_list_scratch.len()
    }

    pub(crate) fn push_expr_id_list_item(&mut self, id: ExprId) {
        self.expr_id_list_scratch.push(id);
    }

    pub(crate) fn expr_id_list(&self, start: usize) -> &[ExprId] {
        &self.expr_id_list_scratch[start..]
    }

    pub(crate) fn finish_expr_id_list(&mut self, start: usize) -> ExprIdRange {
        self.node_arenas
            .borrow_mut()
            .alloc_expr_ids(self.expr_id_list_scratch.drain(start..))
    }

    pub(crate) fn discard_expr_id_list(&mut self, start: usize) {
        self.expr_id_list_scratch.truncate(start);
    }

    pub(crate) fn begin_args(&self) -> usize {
        self.arg_list_scratch.len()
    }

    pub(crate) fn push_arg(&mut self, arg: Arg) {
        self.arg_list_scratch.push(arg);
    }

    pub(crate) fn commit_args(&mut self, range: std::ops::Range<usize>) -> ArgRange {
        self.node_arenas
            .borrow_mut()
            .alloc_args(self.arg_list_scratch.drain(range.start..))
    }

    pub(crate) fn discard_args(&mut self, range: std::ops::Range<usize>) {
        self.arg_list_scratch.truncate(range.start);
    }

    pub(crate) fn begin_array_items(&self) -> usize {
        self.array_item_scratch.len()
    }

    pub(crate) fn push_array_item(&mut self, item: ArrayItem) {
        self.array_item_scratch.push(item);
    }

    pub(crate) fn finish_array_items(&mut self, start: usize) -> ArrayItemRange {
        self.node_arenas
            .borrow_mut()
            .alloc_array_items(self.array_item_scratch.drain(start..))
    }

    pub(crate) fn begin_match_arms(&self) -> usize {
        self.match_arm_scratch.len()
    }

    pub(crate) fn push_match_arm(&mut self, arm: MatchArm) {
        self.match_arm_scratch.push(arm);
    }

    pub(crate) fn match_arms(&self, start: usize) -> &[MatchArm] {
        &self.match_arm_scratch[start..]
    }

    pub(crate) fn finish_match_arms(&mut self, start: usize) -> MatchArmRange {
        self.node_arenas
            .borrow_mut()
            .alloc_match_arms(self.match_arm_scratch.drain(start..))
    }

    pub(crate) fn begin_elseifs(&self) -> usize {
        self.elseif_scratch.len()
    }

    pub(crate) fn finish_elseifs(&mut self, start: usize) -> ElseIfRange {
        self.node_arenas
            .borrow_mut()
            .alloc_elseifs(self.elseif_scratch.drain(start..))
    }

    pub(crate) fn begin_switch_cases(&self) -> usize {
        self.switch_case_scratch.len()
    }

    pub(crate) fn finish_switch_cases(&mut self, start: usize) -> SwitchCaseRange {
        self.node_arenas
            .borrow_mut()
            .alloc_switch_cases(self.switch_case_scratch.drain(start..))
    }

    pub(crate) fn begin_catch_clauses(&self) -> usize {
        self.catch_clause_scratch.len()
    }

    pub(crate) fn finish_catch_clauses(&mut self, start: usize) -> CatchClauseRange {
        self.node_arenas
            .borrow_mut()
            .alloc_catch_clauses(self.catch_clause_scratch.drain(start..))
    }

    pub(crate) fn begin_declare_directives(&self) -> usize {
        self.declare_directive_scratch.len()
    }

    pub(crate) fn finish_declare_directives(&mut self, start: usize) -> DeclareDirectiveRange {
        self.node_arenas
            .borrow_mut()
            .alloc_declare_directives(self.declare_directive_scratch.drain(start..))
    }

    pub(crate) fn begin_global_vars(&self) -> usize {
        self.global_var_scratch.len()
    }

    pub(crate) fn finish_global_vars(&mut self, start: usize) -> GlobalVarRange {
        self.node_arenas
            .borrow_mut()
            .alloc_global_vars(self.global_var_scratch.drain(start..))
    }

    pub(crate) fn begin_static_vars(&self) -> usize {
        self.static_var_scratch.len()
    }

    pub(crate) fn finish_static_vars(&mut self, start: usize) -> StaticVarRange {
        self.node_arenas
            .borrow_mut()
            .alloc_static_vars(self.static_var_scratch.drain(start..))
    }

    pub(crate) fn begin_param_list(&mut self) -> usize {
        let depth = self.param_list_depth;
        if depth == self.param_list_scratch.len() {
            self.param_list_scratch.push(Vec::new());
        }
        self.param_list_scratch[depth].clear();
        self.param_list_depth += 1;
        depth
    }

    pub(crate) fn take_param_list(&mut self, depth: usize) -> Vec<ParamDraft> {
        std::mem::take(&mut self.param_list_scratch[depth])
    }

    pub(crate) fn finish_param_list(
        &mut self,
        depth: usize,
        mut params: Vec<ParamDraft>,
    ) -> ParamRange {
        let range = self
            .node_arenas
            .borrow_mut()
            .alloc_params(params.drain(..).map(|draft| draft.node));
        self.param_list_scratch[depth] = params;
        self.param_list_depth = depth;
        range
    }

    pub(crate) fn begin_type_hint_list(&self) -> usize {
        self.type_hint_scratch.len()
    }

    pub(crate) fn push_type_hint(&mut self, hint: TypeHint) {
        self.type_hint_scratch.push(hint);
    }

    pub(crate) fn finish_type_hint_list(&mut self, start: usize) -> TypeHintRange {
        self.node_arenas
            .borrow_mut()
            .alloc_type_hints(self.type_hint_scratch.drain(start..))
    }

    pub(crate) fn finish_qualified_names(&mut self, start: usize) -> QualifiedNameRange {
        let names = self
            .qualified_name_scratch
            .drain(start..)
            .collect::<Vec<_>>();
        self.node_arenas.borrow_mut().alloc_qualified_names(names)
    }

    pub(crate) fn begin_destructure_items(&self) -> usize {
        self.destructure_item_scratch.len()
    }

    pub(crate) fn push_destructure_item(&mut self, item: DestructureItem) {
        self.destructure_item_scratch.push(item);
    }

    pub(crate) fn destructure_items_scratch(&self, start: usize) -> &[DestructureItem] {
        &self.destructure_item_scratch[start..]
    }

    pub(crate) fn finish_destructure_items(&mut self, start: usize) -> DestructureItemRange {
        self.node_arenas
            .borrow_mut()
            .alloc_destructure_items(self.destructure_item_scratch.drain(start..))
    }

    pub(crate) fn expr_node(&self, id: ExprId) -> std::cell::Ref<'_, ExprNode> {
        std::cell::Ref::map(self.node_arenas.borrow(), |arenas| {
            arenas.expr(id).expect("parser expression id exists")
        })
    }

    pub(crate) fn expr_span(&self, id: ExprId) -> Span {
        self.node_arenas
            .borrow()
            .expr_span(id)
            .expect("parser expression id has a parallel span")
    }

    pub(crate) fn stmt_span(&self, id: StmtId) -> Span {
        self.node_arenas
            .borrow()
            .stmt_span(id)
            .expect("parser statement id has a parallel span")
    }

    pub(crate) fn expr_node_mut(&self, id: ExprId) -> std::cell::RefMut<'_, ExprNode> {
        std::cell::RefMut::map(self.node_arenas.borrow_mut(), |arenas| {
            arenas.expr_mut(id).expect("parser expression id exists")
        })
    }

    pub(crate) fn stmt_node(&self, id: StmtId) -> std::cell::Ref<'_, StmtNode> {
        std::cell::Ref::map(self.node_arenas.borrow(), |arenas| {
            arenas.stmt(id).expect("parser statement id exists")
        })
    }

    /// Updates the parallel span when parsing later extends a node's span.
    pub(crate) fn set_expr_span(&self, id: ExprId, span: Span) {
        let index = u32::from(id.into_raw()) - self.expr_span_base;
        let mut arenas = self.node_arenas.borrow_mut();
        arenas.expr_spans[index as usize] = span;
    }

    /// Consumes any doc comment immediately preceding the current position -
    /// call this before advancing past the first token of a declaration.
    pub(crate) fn take_doc_comment(&mut self) -> Option<DocCommentRanges> {
        self.take_doc_comment_with_range().map(|(_, range)| range)
    }

    /// Takes the temporary text projection together with its source range.
    /// The AST migration keeps the range and drops the text after recorder
    /// products that outlive the tree have been made.
    pub(crate) fn take_doc_comment_with_range(&mut self) -> Option<(Arc<str>, DocCommentRanges)> {
        let group = self.current_docs.as_mut()?;
        let range = group
            .folded_ranges
            .take()
            .or_else(|| group.last_range.take().map(DocCommentRanges::One))?;
        let text = group.folded.take().or_else(|| group.last.take())?;
        Some((text, range))
    }

    pub(crate) fn current_doc_ranges(&self) -> Option<DocCommentRanges> {
        self.current_docs.as_deref().and_then(|group| {
            group
                .folded_ranges
                .clone()
                .or_else(|| group.last_range.map(DocCommentRanges::One))
        })
    }

    #[hotpath::measure]
    pub fn parse_program(&mut self) -> Result<ParsedProgram, ParseError> {
        let stmts_start = self.begin_stmt_list();
        while !self.at_eof() {
            // See parse_statement's own Namespace arm for why the
            // backslash check matters: `namespace\foo();` at the top
            // level is a relative-name call, not a declaration.
            if matches!(self.peek(), TokenRef::Keyword(Keyword::Namespace))
                && !matches!(self.peek_at(1), TokenRef::Backslash)
            {
                self.parse_namespace_stmt()?;
            } else if let Some(stmt) = self.parse_statement(true)? {
                self.push_stmt_list_item(stmt);
            }
        }
        self.check_pattern_only()?;
        let stmts = self.finish_stmt_list(stmts_start);
        Ok(ParsedProgram {
            stmts,
            arenas: self.take_node_arenas(),
        })
    }

    /// The older, braced namespace form - `namespace Foo\Bar { ... }`, or
    /// bare `namespace { ... }` for the global namespace - as opposed to
    /// the common `namespace Foo\Bar;` statement form (still handled
    /// generically in parse_statement). Only legal at the top level of a
    /// file, so this is special-cased in parse_program rather than making
    /// every parse_statement caller handle getting more than one StmtId
    /// back. The block's body is spliced into the surrounding statement
    /// list after a `StmtId::Namespace` marker, the same shape the statement
    /// form gives.
    pub(crate) fn parse_namespace_stmt(&mut self) -> Result<(), ParseError> {
        let start = self.current_pos();
        self.skip(); // `namespace`
        let name = self.parse_qualified_name();
        if self.check(&TokenRef::LBrace) {
            let marker = self.stmt(StmtKind::Namespace(name.clone()), start);
            self.push_stmt_list_item(marker);
            self.expect(&TokenRef::LBrace)?;
            while !self.check(&TokenRef::RBrace) && !self.at_eof() {
                if let Some(stmt) = self.parse_statement(true)? {
                    self.push_stmt_list_item(stmt);
                }
            }
            self.expect(&TokenRef::RBrace)?;
        } else {
            self.expect_semicolon()?;
            let marker = self.stmt(StmtKind::Namespace(name), start);
            self.push_stmt_list_item(marker);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
