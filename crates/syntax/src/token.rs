use std::sync::Arc;

use crate::ast::StringPayload;
use crate::lexer::{FileFacts, IgnoreMark, LexWarning};
use crate::name::{Name, NameId, name_id};
use crate::span::{LineIndex, Span};

/// A fieldless, byte-sized token tag. Payloads and locations live in parallel
/// tables on [`TokenStream`].
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    InlineHtml, DocComment, Variable, Integer, Float, StringLit, InterpString,
    ShellExec, Identifier,
    KwFunction, KwClass, KwInterface, KwTrait, KwEnum, KwExtends, KwImplements,
    KwPublic, KwPrivate, KwProtected, KwStatic, KwConst, KwFinal, KwAbstract,
    KwReadonly, KwIf, KwElseIf, KwElse, KwWhile, KwDo, KwFor, KwForeach, KwAs,
    KwSwitch, KwCase, KwDefault, KwBreak, KwContinue, KwReturn, KwEcho, KwPrint,
    KwNew, KwClone, KwInstanceof, KwTrue, KwFalse, KwNull, KwNamespace, KwUse,
    KwGlobal, KwTry, KwCatch, KwFinally, KwThrow, KwArray, KwList, KwFn, KwMatch,
    KwAnd, KwOr, KwXor, KwInclude, KwIncludeOnce, KwRequire, KwRequireOnce,
    KwYield, KwGoto, KwEndIf, KwEndWhile, KwEndFor, KwEndForeach, KwEndSwitch,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Semicolon, Comma, Colon,
    DoubleColon, Arrow, NullsafeArrow, FatArrow, Question, QuestionQuestion,
    QuestionQuestionEquals, Dot, DotEquals, Plus, Minus, Star, StarStar, Slash,
    Percent, PlusEquals, MinusEquals, StarEquals, StarStarEquals, SlashEquals,
    PercentEquals, AmpEquals, PipeEquals, CaretEquals, LtLtEquals, GtGtEquals,
    Equals, EqualsEquals, EqualsEqualsEquals, BangEquals, BangEqualsEquals, Lt,
    Gt, LtEquals, GtEquals, Spaceship, AmpAmp, PipePipe, Bang, Amp, Pipe, Caret,
    Tilde, LtLt, GtGt, Increment, Decrement, At, Backslash, Ellipsis, PipeGt,
    DollarBrace, AttributeStart, Dollar, Eof,
}

impl TokenKind {
    /// The keyword represented by this kind, if it is a keyword.
    #[inline]
    pub fn keyword(self) -> Option<Keyword> {
        let kind = self as u8;
        let first = TokenKind::KwFunction as u8;
        let last = TokenKind::KwEndSwitch as u8;
        (first..=last)
            .contains(&kind)
            .then(|| KEYWORDS[usize::from(kind - first)].1.clone())
    }
}

/// A payload attached to one token position in [`TokenStream::payloads`].
#[derive(Debug, Clone, PartialEq)]
pub enum TokenPayload {
    /// Payload has been moved into a parsed node.
    Empty,
    Name(NameId),
    Integer(i64),
    Float(f64),
    String(StringPayload),
    Interpolation(Vec<StringPart>),
    Shell(Vec<StringPart>),
    DocComment(Arc<str>),
}

/// A borrowed view of one flat-stream token. Payload-bearing variants borrow
/// the sparse payload entry so parser peeks never rebuild an owned `Token`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TokenRef<'a> {
    InlineHtml(Span), DocComment(&'a str), Variable(&'a Name), Integer(i64),
    Float(f64), StringLit(&'a StringPayload), InterpString(&'a [StringPart]),
    ShellExec(&'a [StringPart]), Identifier(&'a Name), Keyword(Keyword),
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Semicolon, Comma, Colon,
    DoubleColon, Arrow, NullsafeArrow, FatArrow, Question, QuestionQuestion,
    QuestionQuestionEquals, Dot, DotEquals, Plus, Minus, Star, StarStar, Slash,
    Percent, PlusEquals, MinusEquals, StarEquals, StarStarEquals, SlashEquals,
    PercentEquals, AmpEquals, PipeEquals, CaretEquals, LtLtEquals, GtGtEquals,
    Equals, EqualsEquals, EqualsEqualsEquals, BangEquals, BangEqualsEquals, Lt,
    Gt, LtEquals, GtEquals, Spaceship, AmpAmp, PipePipe, Bang, Amp, Pipe, Caret,
    Tilde, LtLt, GtGt, Increment, Decrement, At, Backslash, Ellipsis, PipeGt,
    DollarBrace, AttributeStart, Dollar, Eof,
}

impl TokenRef<'_> {
    pub fn kind(self) -> TokenKind {
        use TokenRef::*;
        match self {
            InlineHtml(_) => TokenKind::InlineHtml, DocComment(_) => TokenKind::DocComment,
            Variable(_) => TokenKind::Variable, Integer(_) => TokenKind::Integer,
            Float(_) => TokenKind::Float, StringLit(_) => TokenKind::StringLit,
            InterpString(_) => TokenKind::InterpString, ShellExec(_) => TokenKind::ShellExec,
            Identifier(_) => TokenKind::Identifier, Keyword(keyword) => keyword.kind(),
            LParen => TokenKind::LParen, RParen => TokenKind::RParen,
            LBrace => TokenKind::LBrace, RBrace => TokenKind::RBrace,
            LBracket => TokenKind::LBracket, RBracket => TokenKind::RBracket,
            Semicolon => TokenKind::Semicolon, Comma => TokenKind::Comma, Colon => TokenKind::Colon,
            DoubleColon => TokenKind::DoubleColon, Arrow => TokenKind::Arrow,
            NullsafeArrow => TokenKind::NullsafeArrow, FatArrow => TokenKind::FatArrow,
            Question => TokenKind::Question, QuestionQuestion => TokenKind::QuestionQuestion,
            QuestionQuestionEquals => TokenKind::QuestionQuestionEquals,
            Dot => TokenKind::Dot, DotEquals => TokenKind::DotEquals, Plus => TokenKind::Plus,
            Minus => TokenKind::Minus, Star => TokenKind::Star, StarStar => TokenKind::StarStar,
            Slash => TokenKind::Slash, Percent => TokenKind::Percent,
            PlusEquals => TokenKind::PlusEquals, MinusEquals => TokenKind::MinusEquals,
            StarEquals => TokenKind::StarEquals, StarStarEquals => TokenKind::StarStarEquals,
            SlashEquals => TokenKind::SlashEquals, PercentEquals => TokenKind::PercentEquals,
            AmpEquals => TokenKind::AmpEquals, PipeEquals => TokenKind::PipeEquals,
            CaretEquals => TokenKind::CaretEquals, LtLtEquals => TokenKind::LtLtEquals,
            GtGtEquals => TokenKind::GtGtEquals, Equals => TokenKind::Equals,
            EqualsEquals => TokenKind::EqualsEquals, EqualsEqualsEquals => TokenKind::EqualsEqualsEquals,
            BangEquals => TokenKind::BangEquals, BangEqualsEquals => TokenKind::BangEqualsEquals,
            Lt => TokenKind::Lt, Gt => TokenKind::Gt, LtEquals => TokenKind::LtEquals,
            GtEquals => TokenKind::GtEquals, Spaceship => TokenKind::Spaceship,
            AmpAmp => TokenKind::AmpAmp, PipePipe => TokenKind::PipePipe, Bang => TokenKind::Bang,
            Amp => TokenKind::Amp, Pipe => TokenKind::Pipe, Caret => TokenKind::Caret,
            Tilde => TokenKind::Tilde, LtLt => TokenKind::LtLt, GtGt => TokenKind::GtGt,
            Increment => TokenKind::Increment, Decrement => TokenKind::Decrement, At => TokenKind::At,
            Backslash => TokenKind::Backslash, Ellipsis => TokenKind::Ellipsis, PipeGt => TokenKind::PipeGt,
            DollarBrace => TokenKind::DollarBrace, AttributeStart => TokenKind::AttributeStart,
            Dollar => TokenKind::Dollar, Eof => TokenKind::Eof,
        }
    }
}

/// A sparse token payload, addressed by its index in `TokenStream::kinds`.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenPayloadEntry {
    pub token: u32,
    pub payload: TokenPayload,
}

/// A doc comment immediately preceding the token at `before`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocEntry {
    pub before: u32,
    pub span: Span,
    pub text: Arc<str>,
}

/// Flat lexer output. `kinds[i]` and `spans[i]` describe the same token;
/// payloads and metadata that would otherwise be stored inline are sparse.
#[derive(Clone)]
pub struct TokenStream {
    pub kinds: Vec<TokenKind>,
    pub spans: Vec<Span>,
    pub payloads: Vec<TokenPayloadEntry>,
    pub docs: Vec<DocEntry>,
    pub keyword_raw: Vec<(u32, String)>,
    pub line_index: Arc<LineIndex>,
    pub names: Vec<Name>,
    pub decoded_bytes: Vec<u8>,
    pub comments: Vec<(Span, String)>,
    pub ignore_marks: Vec<IgnoreMark>,
    pub warnings: Vec<LexWarning>,
    pub facts: FileFacts,
}

impl Default for TokenStream {
    fn default() -> Self {
        Self {
            kinds: Vec::new(), spans: Vec::new(), payloads: Vec::new(), docs: Vec::new(),
            keyword_raw: Vec::new(), line_index: Arc::new(LineIndex::default()), names: Vec::new(),
            decoded_bytes: Vec::new(), comments: Vec::new(), ignore_marks: Vec::new(),
            warnings: Vec::new(), facts: FileFacts::default(),
        }
    }
}

impl TokenStream {
    /// Clears per-file contents while retaining vector allocations.
    pub fn clear_for_reuse(&mut self) {
        self.kinds.clear();
        self.spans.clear();
        self.payloads.clear();
        self.docs.clear();
        self.keyword_raw.clear();
        self.names.clear();
        self.decoded_bytes.clear();
        self.comments.clear();
        self.ignore_marks.clear();
        self.warnings.clear();
        self.line_index = Arc::new(LineIndex::default());
        self.facts = FileFacts::default();
    }

    pub(crate) fn push_token(
        &mut self,
        kind: TokenKind,
        span: Span,
        payload: Option<TokenPayload>,
        raw_keyword: Option<String>,
    ) {
        let index = self.kinds.len();
        self.kinds.push(kind);
        self.spans.push(span);
        if let Some(payload) = payload {
            self.payloads.push(TokenPayloadEntry {
                token: u32::try_from(index).expect("token stream exceeds u32 indexes"),
                payload,
            });
        }
        if let Some(raw) = raw_keyword {
            self.keyword_raw.push((
                u32::try_from(index).expect("token stream exceeds u32 indexes"),
                raw,
            ));
        }
    }

    pub(crate) fn push_doc(&mut self, span: Span, text: Arc<str>) {
        self.docs.push(DocEntry {
            before: u32::try_from(self.kinds.len()).expect("token stream exceeds u32 indexes"),
            span,
            text,
        });
    }

    /// The kind at `index`, or the EOF sentinel past the stream.
    #[inline]
    pub fn kind_at(&self, index: usize) -> TokenKind {
        self.kinds.get(index).copied().unwrap_or(TokenKind::Eof)
    }

    /// The source span at `index`, or the final span past the stream.
    #[inline]
    pub fn span_at(&self, index: usize) -> Span {
        self.spans
            .get(index)
            .copied()
            .or_else(|| self.spans.last().copied())
            .unwrap_or_default()
    }

    /// A payload is sparse and keyed by token index. The binary search also
    /// supports parser lookahead without advancing the payload cursor.
    #[inline]
    pub fn payload_at(&self, token: usize) -> Option<&TokenPayload> {
        let token = u32::try_from(token).ok()?;
        self.payloads
            .binary_search_by_key(&token, |entry| entry.token)
            .ok()
            .map(|index| &self.payloads[index].payload)
    }

    /// Moves a consumed payload out while leaving its sparse slot in place.
    /// The parser advances its payload cursor when it advances this token.
    #[inline]
    pub fn take_payload_at(&mut self, token: usize) -> Option<TokenPayload> {
        let token = u32::try_from(token).ok()?;
        let index = self
            .payloads
            .binary_search_by_key(&token, |entry| entry.token)
            .ok()?;
        Some(std::mem::replace(
            &mut self.payloads[index].payload,
            TokenPayload::Empty,
        ))
    }

    /// Returns a borrowed token view at `index`, with an EOF view past the
    /// stream. This is the parser's non-allocating lookahead surface.
    pub fn token_at(&self, index: usize) -> TokenRef<'_> {
        use TokenKind as K;
        use TokenPayload as P;
        use TokenRef as R;
        let kind = self.kind_at(index);
        match kind {
            K::InlineHtml => R::InlineHtml(self.span_at(index)),
            K::DocComment => match self.payload_at(index) { Some(P::DocComment(text)) => R::DocComment(text), _ => R::DocComment("") },
            K::Variable => match self.payload_at(index) { Some(P::Name(id)) => self.name(*id).map_or(R::Eof, R::Variable), _ => R::Eof },
            K::Integer => match self.payload_at(index) { Some(P::Integer(value)) => R::Integer(*value), _ => R::Eof },
            K::Float => match self.payload_at(index) { Some(P::Float(value)) => R::Float(*value), _ => R::Eof },
            K::StringLit => match self.payload_at(index) { Some(P::String(value)) => R::StringLit(value), _ => R::Eof },
            K::InterpString => match self.payload_at(index) { Some(P::Interpolation(parts)) => R::InterpString(parts), _ => R::Eof },
            K::ShellExec => match self.payload_at(index) { Some(P::Shell(parts)) => R::ShellExec(parts), _ => R::Eof },
            K::Identifier => match self.payload_at(index) { Some(P::Name(id)) => self.name(*id).map_or(R::Eof, R::Identifier), _ => R::Eof },
            K::LParen => R::LParen, K::RParen => R::RParen, K::LBrace => R::LBrace, K::RBrace => R::RBrace,
            K::LBracket => R::LBracket, K::RBracket => R::RBracket, K::Semicolon => R::Semicolon,
            K::Comma => R::Comma, K::Colon => R::Colon, K::DoubleColon => R::DoubleColon,
            K::Arrow => R::Arrow, K::NullsafeArrow => R::NullsafeArrow, K::FatArrow => R::FatArrow,
            K::Question => R::Question, K::QuestionQuestion => R::QuestionQuestion,
            K::QuestionQuestionEquals => R::QuestionQuestionEquals, K::Dot => R::Dot, K::DotEquals => R::DotEquals,
            K::Plus => R::Plus, K::Minus => R::Minus, K::Star => R::Star, K::StarStar => R::StarStar,
            K::Slash => R::Slash, K::Percent => R::Percent, K::PlusEquals => R::PlusEquals,
            K::MinusEquals => R::MinusEquals, K::StarEquals => R::StarEquals, K::StarStarEquals => R::StarStarEquals,
            K::SlashEquals => R::SlashEquals, K::PercentEquals => R::PercentEquals, K::AmpEquals => R::AmpEquals,
            K::PipeEquals => R::PipeEquals, K::CaretEquals => R::CaretEquals, K::LtLtEquals => R::LtLtEquals,
            K::GtGtEquals => R::GtGtEquals, K::Equals => R::Equals, K::EqualsEquals => R::EqualsEquals,
            K::EqualsEqualsEquals => R::EqualsEqualsEquals, K::BangEquals => R::BangEquals,
            K::BangEqualsEquals => R::BangEqualsEquals, K::Lt => R::Lt, K::Gt => R::Gt,
            K::LtEquals => R::LtEquals, K::GtEquals => R::GtEquals, K::Spaceship => R::Spaceship,
            K::AmpAmp => R::AmpAmp, K::PipePipe => R::PipePipe, K::Bang => R::Bang, K::Amp => R::Amp,
            K::Pipe => R::Pipe, K::Caret => R::Caret, K::Tilde => R::Tilde, K::LtLt => R::LtLt,
            K::GtGt => R::GtGt, K::Increment => R::Increment, K::Decrement => R::Decrement, K::At => R::At,
            K::Backslash => R::Backslash, K::Ellipsis => R::Ellipsis, K::PipeGt => R::PipeGt,
            K::DollarBrace => R::DollarBrace, K::AttributeStart => R::AttributeStart, K::Dollar => R::Dollar,
            K::Eof => R::Eof,
            keyword_or_unhandled => keyword_or_unhandled.keyword().map_or(R::Eof, R::Keyword),
        }
    }

    /// The original keyword spelling, when the lexer recorded one.
    #[inline]
    pub fn raw_keyword_at(&self, token: usize) -> Option<&str> {
        let token = u32::try_from(token).ok()?;
        self.keyword_raw
            .binary_search_by_key(&token, |(index, _)| *index)
            .ok()
            .map(|index| self.keyword_raw[index].1.as_str())
    }

    /// Resolves a file-local name ID through the stream's name table.
    #[inline]
    pub fn name(&self, id: NameId) -> Option<&Name> {
        self.names.get(u32::from(id.into_raw()) as usize)
    }

    /// Builds a stream from flat tables emitted during lexing.
    pub fn from_parts(
        kinds: Vec<TokenKind>,
        spans: Vec<Span>,
        payloads: Vec<TokenPayloadEntry>,
        docs: Vec<DocEntry>,
        keyword_raw: Vec<(u32, String)>,
        line_index: Arc<LineIndex>,
        names: Vec<Name>,
        decoded_bytes: Vec<u8>,
        comments: Vec<(Span, String)>,
        ignore_marks: Vec<IgnoreMark>,
        warnings: Vec<LexWarning>,
        facts: FileFacts,
    ) -> Self {
        assert_eq!(kinds.len(), spans.len(), "token kinds and spans are aligned");
        Self {
            kinds,
            spans,
            payloads,
            docs,
            keyword_raw,
            line_index,
            names,
            decoded_bytes,
            comments,
            ignore_marks,
            warnings,
            facts,
        }
    }

    pub fn from_lexed_parts(
        tokens: Vec<Token>,
        spans: Vec<Span>,
        keyword_raw: Vec<(usize, String)>,
        docs: Vec<DocEntry>,
        line_index: Arc<LineIndex>,
        names: Box<[Name]>,
        decoded_bytes: Vec<u8>,
        comments: Vec<(Span, String)>,
        ignore_marks: Vec<IgnoreMark>,
        warnings: Vec<LexWarning>,
        facts: FileFacts,
    ) -> Self {
        assert_eq!(tokens.len(), spans.len(), "token kinds and spans are aligned");
        let names = if names.is_empty() {
            let mut collected = Vec::new();
            let mut seen = rustc_hash::FxHashSet::default();
            let raw_by_token: std::collections::HashMap<usize, &str> = keyword_raw
                .iter()
                .map(|(index, spelling)| (*index, spelling.as_str()))
                .collect();
            for (index, token) in tokens.iter().enumerate() {
                let name = match token {
                    Token::Variable(name) | Token::Identifier(name) => Some(name.clone()),
                    Token::Keyword(keyword) => Some(
                        crate::name::Interner::global().name(
                            raw_by_token
                                .get(&index)
                                .copied()
                                .unwrap_or_else(|| keyword.as_str()),
                        ),
                    ),
                    _ => None,
                };
                if let Some(name) = name && seen.insert(name.clone()) {
                    collected.push(name);
                }
            }
            collected
        } else {
            names.into_vec()
        };
        let by_name: std::collections::HashMap<Name, NameId> = names
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, name)| (name, name_id(index)))
            .collect();
        let mut kinds = Vec::with_capacity(tokens.len());
        let mut payloads = Vec::new();
        for (index, token) in tokens.into_iter().enumerate() {
            let (kind, payload) = token.into_kind_payload(|name| {
                *by_name.get(name).expect("token name is in the file name table")
            });
            kinds.push(kind);
            if let Some(payload) = payload {
                payloads.push(TokenPayloadEntry {
                    token: u32::try_from(index).expect("token stream exceeds u32 indexes"),
                    payload,
                });
            }
        }
        Self::from_parts(
            kinds,
            spans,
            payloads,
            docs,
            keyword_raw.into_iter().map(|(i, raw)|
                (u32::try_from(i).expect("token stream exceeds u32 indexes"), raw)
            ).collect(),
            line_index,
            names,
            decoded_bytes,
            comments,
            ignore_marks,
            warnings,
            facts,
        )
    }
}

impl Token {
    pub(crate) fn into_kind_payload(
        self,
        mut name_id: impl FnMut(&Name) -> NameId,
    ) -> (TokenKind, Option<TokenPayload>) {
        use Token::*;
        let plain = |kind| (kind, None);
        let mut named = |kind, name: Name| {
            let id = name_id(&name);
            (kind, Some(TokenPayload::Name(id)))
        };
        match self {
            InlineHtml(_) => plain(TokenKind::InlineHtml),
            DocComment(text) => (TokenKind::DocComment, Some(TokenPayload::DocComment(text))),
            Variable(name) => named(TokenKind::Variable, name),
            Integer(n) => (TokenKind::Integer, Some(TokenPayload::Integer(n))),
            Float(n) => (TokenKind::Float, Some(TokenPayload::Float(n))),
            StringLit(s) => (TokenKind::StringLit, Some(TokenPayload::String(s))),
            InterpString(parts) => (TokenKind::InterpString, Some(TokenPayload::Interpolation(parts))),
            ShellExec(parts) => (TokenKind::ShellExec, Some(TokenPayload::Shell(parts))),
            Identifier(name) => named(TokenKind::Identifier, name),
            Keyword(keyword) => (keyword.kind(), None),
            LParen => plain(TokenKind::LParen), RParen => plain(TokenKind::RParen),
            LBrace => plain(TokenKind::LBrace), RBrace => plain(TokenKind::RBrace),
            LBracket => plain(TokenKind::LBracket), RBracket => plain(TokenKind::RBracket),
            Semicolon => plain(TokenKind::Semicolon), Comma => plain(TokenKind::Comma),
            Colon => plain(TokenKind::Colon), DoubleColon => plain(TokenKind::DoubleColon),
            Arrow => plain(TokenKind::Arrow), NullsafeArrow => plain(TokenKind::NullsafeArrow),
            FatArrow => plain(TokenKind::FatArrow), Question => plain(TokenKind::Question),
            QuestionQuestion => plain(TokenKind::QuestionQuestion),
            QuestionQuestionEquals => plain(TokenKind::QuestionQuestionEquals),
            Dot => plain(TokenKind::Dot), DotEquals => plain(TokenKind::DotEquals),
            Plus => plain(TokenKind::Plus), Minus => plain(TokenKind::Minus),
            Star => plain(TokenKind::Star), StarStar => plain(TokenKind::StarStar),
            Slash => plain(TokenKind::Slash), Percent => plain(TokenKind::Percent),
            PlusEquals => plain(TokenKind::PlusEquals), MinusEquals => plain(TokenKind::MinusEquals),
            StarEquals => plain(TokenKind::StarEquals), StarStarEquals => plain(TokenKind::StarStarEquals),
            SlashEquals => plain(TokenKind::SlashEquals), PercentEquals => plain(TokenKind::PercentEquals),
            AmpEquals => plain(TokenKind::AmpEquals), PipeEquals => plain(TokenKind::PipeEquals),
            CaretEquals => plain(TokenKind::CaretEquals), LtLtEquals => plain(TokenKind::LtLtEquals),
            GtGtEquals => plain(TokenKind::GtGtEquals), Equals => plain(TokenKind::Equals),
            EqualsEquals => plain(TokenKind::EqualsEquals), EqualsEqualsEquals => plain(TokenKind::EqualsEqualsEquals),
            BangEquals => plain(TokenKind::BangEquals), BangEqualsEquals => plain(TokenKind::BangEqualsEquals),
            Lt => plain(TokenKind::Lt), Gt => plain(TokenKind::Gt), LtEquals => plain(TokenKind::LtEquals),
            GtEquals => plain(TokenKind::GtEquals), Spaceship => plain(TokenKind::Spaceship),
            AmpAmp => plain(TokenKind::AmpAmp), PipePipe => plain(TokenKind::PipePipe),
            Bang => plain(TokenKind::Bang), Amp => plain(TokenKind::Amp), Pipe => plain(TokenKind::Pipe),
            Caret => plain(TokenKind::Caret), Tilde => plain(TokenKind::Tilde),
            LtLt => plain(TokenKind::LtLt), GtGt => plain(TokenKind::GtGt),
            Increment => plain(TokenKind::Increment), Decrement => plain(TokenKind::Decrement),
            At => plain(TokenKind::At), Backslash => plain(TokenKind::Backslash),
            Ellipsis => plain(TokenKind::Ellipsis), PipeGt => plain(TokenKind::PipeGt),
            DollarBrace => plain(TokenKind::DollarBrace), AttributeStart => plain(TokenKind::AttributeStart),
            Dollar => plain(TokenKind::Dollar), Eof => plain(TokenKind::Eof),
        }
    }
}

impl Keyword {
    fn kind(&self) -> TokenKind {
        use Keyword::*;
        match self {
            Function => TokenKind::KwFunction, Class => TokenKind::KwClass,
            Interface => TokenKind::KwInterface, Trait => TokenKind::KwTrait, Enum => TokenKind::KwEnum,
            Extends => TokenKind::KwExtends, Implements => TokenKind::KwImplements,
            Public => TokenKind::KwPublic, Private => TokenKind::KwPrivate, Protected => TokenKind::KwProtected,
            Static => TokenKind::KwStatic, Const => TokenKind::KwConst, Final => TokenKind::KwFinal,
            Abstract => TokenKind::KwAbstract, Readonly => TokenKind::KwReadonly, If => TokenKind::KwIf,
            ElseIf => TokenKind::KwElseIf, Else => TokenKind::KwElse, While => TokenKind::KwWhile,
            Do => TokenKind::KwDo, For => TokenKind::KwFor, Foreach => TokenKind::KwForeach,
            As => TokenKind::KwAs, Switch => TokenKind::KwSwitch, Case => TokenKind::KwCase,
            Default => TokenKind::KwDefault, Break => TokenKind::KwBreak, Continue => TokenKind::KwContinue,
            Return => TokenKind::KwReturn, Echo => TokenKind::KwEcho, Print => TokenKind::KwPrint,
            New => TokenKind::KwNew, Clone => TokenKind::KwClone, Instanceof => TokenKind::KwInstanceof,
            True => TokenKind::KwTrue, False => TokenKind::KwFalse, Null => TokenKind::KwNull,
            Namespace => TokenKind::KwNamespace, Use => TokenKind::KwUse, Global => TokenKind::KwGlobal,
            Try => TokenKind::KwTry, Catch => TokenKind::KwCatch, Finally => TokenKind::KwFinally,
            Throw => TokenKind::KwThrow, Array => TokenKind::KwArray, List => TokenKind::KwList,
            Fn => TokenKind::KwFn, Match => TokenKind::KwMatch, And => TokenKind::KwAnd,
            Or => TokenKind::KwOr, Xor => TokenKind::KwXor, Include => TokenKind::KwInclude,
            IncludeOnce => TokenKind::KwIncludeOnce, Require => TokenKind::KwRequire,
            RequireOnce => TokenKind::KwRequireOnce, Yield => TokenKind::KwYield, Goto => TokenKind::KwGoto,
            EndIf => TokenKind::KwEndIf, EndWhile => TokenKind::KwEndWhile, EndFor => TokenKind::KwEndFor,
            EndForeach => TokenKind::KwEndForeach, EndSwitch => TokenKind::KwEndSwitch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub enum Keyword {
    Function,
    Class,
    Interface,
    Trait,
    Enum,
    Extends,
    Implements,
    Public,
    Private,
    Protected,
    Static,
    Const,
    Final,
    Abstract,
    Readonly,
    If,
    ElseIf,
    Else,
    While,
    Do,
    For,
    Foreach,
    As,
    Switch,
    Case,
    Default,
    Break,
    Continue,
    Return,
    Echo,
    Print,
    New,
    Clone,
    Instanceof,
    True,
    False,
    Null,
    Namespace,
    Use,
    Global,
    Try,
    Catch,
    Finally,
    Throw,
    Array,
    List,
    Fn,
    Match,
    And,
    Or,
    Xor,
    Include,
    IncludeOnce,
    Require,
    RequireOnce,
    Yield,
    Goto,
    EndIf,
    EndWhile,
    EndFor,
    EndForeach,
    EndSwitch,
}

/// Every keyword with its spelling.
const KEYWORDS: [(&str, Keyword); 62] = {
    use Keyword::*;
    [
        ("function", Function),
        ("class", Class),
        ("interface", Interface),
        ("trait", Trait),
        ("enum", Enum),
        ("extends", Extends),
        ("implements", Implements),
        ("public", Public),
        ("private", Private),
        ("protected", Protected),
        ("static", Static),
        ("const", Const),
        ("final", Final),
        ("abstract", Abstract),
        ("readonly", Readonly),
        ("if", If),
        ("elseif", ElseIf),
        ("else", Else),
        ("while", While),
        ("do", Do),
        ("for", For),
        ("foreach", Foreach),
        ("as", As),
        ("switch", Switch),
        ("case", Case),
        ("default", Default),
        ("break", Break),
        ("continue", Continue),
        ("return", Return),
        ("echo", Echo),
        ("print", Print),
        ("new", New),
        ("clone", Clone),
        ("instanceof", Instanceof),
        ("true", True),
        ("false", False),
        ("null", Null),
        ("namespace", Namespace),
        ("use", Use),
        ("global", Global),
        ("try", Try),
        ("catch", Catch),
        ("finally", Finally),
        ("throw", Throw),
        ("array", Array),
        ("list", List),
        ("fn", Fn),
        ("match", Match),
        ("and", And),
        ("or", Or),
        ("xor", Xor),
        ("include", Include),
        ("include_once", IncludeOnce),
        ("require", Require),
        ("require_once", RequireOnce),
        ("yield", Yield),
        ("goto", Goto),
        ("endif", EndIf),
        ("endwhile", EndWhile),
        ("endfor", EndFor),
        ("endforeach", EndForeach),
        ("endswitch", EndSwitch),
    ]
};

/// Where a keyword of this length, first byte and last byte is in
/// `KEYWORD_SLOTS`: the bytes are folded to lower case by `| 0x20`, which
/// leaves a lower-case letter as it is, so a keyword in any case has its
/// lower-case spelling's slot. The constants were chosen so that no two of
/// the keywords share a slot, which `KEYWORD_SLOTS` is made to assert.
const fn keyword_slot(len: usize, first: u8, last: u8) -> usize {
    (len + (first | 0x20) as usize * 35 + (last | 0x20) as usize * 29) & 255
}

/// For each slot, the index in `KEYWORDS` plus one of the keyword that has
/// it, or 0.
static KEYWORD_SLOTS: [u8; 256] = {
    let mut slots = [0u8; 256];
    let mut i = 0;
    while i < KEYWORDS.len() {
        let bytes = KEYWORDS[i].0.as_bytes();
        let slot = keyword_slot(bytes.len(), bytes[0], bytes[bytes.len() - 1]);
        assert!(slots[slot] == 0, "two keywords share a slot");
        slots[slot] = i as u8 + 1;
        i += 1;
    }
    slots
};

impl Keyword {
    /// No keyword is longer than this (the longest, "require_once" and
    /// "include_once", is 12 bytes): a longer identifier is none.
    const MAX_KEYWORD_LEN: usize = 16;

    pub fn try_from_str(s: &str) -> Option<Keyword> {
        Self::try_from_bytes(s.as_bytes())
    }

    /// The keyword `bytes` spells in any mix of cases, if one: a slot of
    /// `KEYWORD_SLOTS` found from the length and the first and last bytes,
    /// and one comparison with that slot's spelling, without a copy.
    /// `bytes` is a raw identifier-shaped byte span (`is_ident_continue`
    /// accepts any byte `>= 0x80`), so it may hold what is not UTF-8: a
    /// keyword is pure ASCII, so such a span matches none, which is its
    /// answer, with nothing to validate.
    #[inline]
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Keyword> {
        let (&first, &last) = (bytes.first()?, bytes.last()?);
        if bytes.len() > Self::MAX_KEYWORD_LEN {
            return None;
        }
        let slot = KEYWORD_SLOTS[keyword_slot(bytes.len(), first, last)];
        let (spelling, keyword) = KEYWORDS.get(usize::from(slot).checked_sub(1)?)?;
        spelling
            .as_bytes()
            .eq_ignore_ascii_case(bytes)
            .then(|| keyword.clone())
    }

    /// The inverse of `try_from_str` - needed because most of these keywords
    /// are only reserved in *declaration* position in real PHP ("semi-
    /// reserved words"): `enum`, `match`, `new`, `list`, `static`, etc are
    /// all valid namespace segments, method names, and property names.
    /// `use Foo\Enum\Bar;` or `$x->match()` would otherwise misparse just
    /// because this lexer doesn't distinguish position the way PHP's own
    /// grammar does.
    pub fn as_str(&self) -> &'static str {
        use Keyword::*;
        match self {
            Function => "function",
            Class => "class",
            Interface => "interface",
            Trait => "trait",
            Enum => "enum",
            Extends => "extends",
            Implements => "implements",
            Public => "public",
            Private => "private",
            Protected => "protected",
            Static => "static",
            Const => "const",
            Final => "final",
            Abstract => "abstract",
            Readonly => "readonly",
            If => "if",
            ElseIf => "elseif",
            Else => "else",
            While => "while",
            Do => "do",
            For => "for",
            Foreach => "foreach",
            As => "as",
            Switch => "switch",
            Case => "case",
            Default => "default",
            Break => "break",
            Continue => "continue",
            Return => "return",
            Echo => "echo",
            Print => "print",
            New => "new",
            Clone => "clone",
            Instanceof => "instanceof",
            True => "true",
            False => "false",
            Null => "null",
            Namespace => "namespace",
            Use => "use",
            Global => "global",
            Try => "try",
            Catch => "catch",
            Finally => "finally",
            Throw => "throw",
            Array => "array",
            And => "and",
            Or => "or",
            Xor => "xor",
            Include => "include",
            IncludeOnce => "include_once",
            Require => "require",
            RequireOnce => "require_once",
            Yield => "yield",
            List => "list",
            Fn => "fn",
            Match => "match",
            EndIf => "endif",
            EndWhile => "endwhile",
            EndFor => "endfor",
            EndForeach => "endforeach",
            EndSwitch => "endswitch",
            Goto => "goto",
        }
    }
}

// A piece of a double-quoted (interpolated) string. `ComplexExpr` holds raw
// PHP source text (the inside of `{$...}`, or a `$var[key]` access whose own
// key doesn't fit `Index`'s `SimpleIndexKey`) that gets re-lexed and
// re-parsed as a full expression - this avoids writing a second, cut-down
// expression grammar just for strings. `PropertyAccess`/`Index` cover the
// two "simple syntax" shapes (`$var->prop`, `$var[key]`) directly instead -
// the lexer already has the variable/property/key text as separate pieces
// at the point it would otherwise throw them away into a synthesized
// `format!("${name}->{prop}")`-style source string, so building the `ExprNode`
// straight from those pieces (`parser::build_interpolated`) skips a full
// second `Lexer`/`Parser` round-trip per occurrence. `$var->prop`'s own
// property name is always a well-formed identifier by construction (the
// lexer only enters that branch after confirming it starts with an
// identifier byte), so it has no fallback; `[key]` does, since anything can
// sit between the brackets - see `SimpleIndexKey`'s own doc comment.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub enum StringPart {
    // A literal chunk and the span of its source text.
    Literal(StringPayload, Span),
    // `$var`, spanning the `$` and the name.
    SimpleVar(crate::name::Name, Span),
    // The fragment's own source text, plus the real file position it
    // starts at - `parser::build_interpolated` re-lexes the fragment on
    // its own, starting at this position, so an error inside
    // `"{$obj->prop}"` still points at its real position. Deliberately kept as plain `String`, not interned: this
    // text is re-lexed by a throwaway sub-`Lexer`/`Parser` and then
    // immediately discarded (`Parser::build_interpolated`'s own
    // `ComplexExpr` arm is its only consumer) - interning it would pay a
    // real lookup/lock cost for text that's read exactly once and never
    // compared/deduplicated again, with no downstream `Arc<str>` field to
    // carry the saved allocation into anyway.
    ComplexExpr(String, u32),
    // `$var->prop` (or `$var?->prop`) simple syntax. `var_start` is the
    // `$`'s own position, `var_end` the position right after the variable
    // name (where the arrow starts), `end` the position right after `prop`.
    PropertyAccess {
        var: crate::name::Name,
        var_start: u32,
        var_end: u32,
        prop: crate::name::Name,
        nullsafe: bool,
        end: u32,
    },
    // `$var[key]` simple syntax, `key` restricted to `SimpleIndexKey`'s
    // three shapes - anything else falls back to `ComplexExpr` instead
    // (see `SimpleIndexKey`'s own doc comment). Position fields mirror
    // `PropertyAccess`'s own: `var_start`/`var_end` bracket the variable
    // name, `end` is the position right after the closing `]`.
    Index {
        var: crate::name::Name,
        var_start: u32,
        var_end: u32,
        key: SimpleIndexKey,
        end: u32,
    },
}

// The three key shapes PHP's own `$var[key]` string-interpolation simple
// syntax accepts natively (a bare identifier reads as a string key, not a
// constant reference, unlike ordinary PHP code - real PHP's own lexer
// special-cases this exact position). Anything else (a nested expression,
// a quoted string, whitespace, an operator) isn't valid simple syntax at
// all and falls back to `StringPart::ComplexExpr`'s re-lex path, which is
// a defensive fallback (a well-formed key not fitting one of these three
// shapes shouldn't occur in real, already-lexable PHP source) rather than
// a real, expected case.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub enum SimpleIndexKey {
    Var(crate::name::Name),
    Int(i64),
    Str(Arc<str>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "fuzzing", derive(arbitrary::Arbitrary))]
pub enum Token {
    /// Inline text as a source range; its bytes live in the parsed file.
    InlineHtml(crate::span::Span),
    // Raw `/** ... */` text, including the delimiters. Regular `/* */` and
    // `//` comments are still discarded as whitespace - only doc comments
    // are worth carrying through, so the parser can pair one with the
    // declaration it precedes.
    DocComment(Arc<str>),

    Variable(crate::name::Name),
    Integer(i64),
    Float(f64),
    // Deliberately plain `String`, not interned - unlike an identifier or
    // variable name, a string literal's own contents rarely repeat
    // (real, distinct message/SQL/template text almost every time), so
    // interning it would pay a real lookup/lock cost for little to no
    // dedup benefit.
    StringLit(StringPayload),
    InterpString(Vec<StringPart>),
    // `` `shell command $with $interpolation` `` - same interpolation
    // rules as a double-quoted string, different delimiter and runtime
    // meaning (shell_exec).
    ShellExec(Vec<StringPart>),
    Identifier(crate::name::Name),
    Keyword(Keyword),

    // punctuation / operators
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semicolon,
    Comma,
    Colon,
    DoubleColon,
    Arrow,
    NullsafeArrow,
    FatArrow,
    Question,
    QuestionQuestion,
    QuestionQuestionEquals,
    Dot,
    DotEquals,
    Plus,
    Minus,
    Star,
    StarStar,
    Slash,
    Percent,
    PlusEquals,
    MinusEquals,
    StarEquals,
    StarStarEquals,
    SlashEquals,
    PercentEquals,
    AmpEquals,
    PipeEquals,
    CaretEquals,
    LtLtEquals,
    GtGtEquals,
    Equals,
    EqualsEquals,
    EqualsEqualsEquals,
    BangEquals,
    BangEqualsEquals,
    Lt,
    Gt,
    LtEquals,
    GtEquals,
    Spaceship,
    AmpAmp,
    PipePipe,
    Bang,
    Amp,
    Pipe,
    Caret,
    Tilde,
    LtLt,
    GtGt,
    Increment,
    Decrement,
    At,
    Backslash,
    Ellipsis,
    // PHP 8.5's pipe operator (`|>`).
    PipeGt,
    // `${` - starts the older "variable variable" syntax `${expr}`.
    DollarBrace,
    // `#[` - opens an attribute group (`#[Attribute(...), Other]`). Real
    // tokens, not skipped: everything between this and the matching `]`
    // (the attribute's name, `(`, its arguments, `,` between multiple
    // attributes in one group) is ordinary tokenization from here on, so
    // the parser can build a real `Attribute` node instead of the content
    // being thrown away entirely. `#[PhpStormStubsElementAvailable(...)]`
    // and `#[LanguageLevelTypeAware(...)]` are the two narrow exceptions
    // (still consumed as raw text by `skip_whitespace_and_comments`, never
    // reaching the parser as tokens at all) - see that function's own
    // comment.
    AttributeStart,
    // A lone `$` not immediately followed by an identifier or `{` - the
    // brace-less variable-variable form (`$$name`, meaning "the variable
    // named by the value of $name", same as `${$name}`).
    Dollar,

    Eof,
}

#[cfg(test)]
mod keyword_lookup_tests {
    use super::*;

    #[test]
    fn a_keyword_is_found_in_any_case_and_a_near_miss_is_not() {
        for word in [
            "function",
            "class",
            "interface",
            "trait",
            "enum",
            "extends",
            "implements",
            "public",
            "private",
            "protected",
            "static",
            "const",
            "final",
            "abstract",
            "readonly",
            "if",
            "elseif",
            "else",
            "while",
            "return",
            "match",
            "namespace",
            "use",
            "new",
            "instanceof",
        ] {
            let keyword = Keyword::try_from_bytes(word.as_bytes())
                .unwrap_or_else(|| panic!("{word} is a keyword"));
            assert_eq!(keyword.as_str(), word);
            assert_eq!(
                Keyword::try_from_bytes(word.to_ascii_uppercase().as_bytes()),
                Some(keyword.clone()),
                "{word} in capitals"
            );
            let mut mixed = word.to_string();
            mixed[..1].make_ascii_uppercase();
            assert_eq!(
                Keyword::try_from_bytes(mixed.as_bytes()),
                Some(keyword.clone()),
                "{mixed}"
            );
            // One byte short, one byte long, one byte changed.
            assert_eq!(Keyword::try_from_bytes(&word.as_bytes()[1..]), None);
            assert_eq!(Keyword::try_from_bytes(format!("{word}x").as_bytes()), None);
            assert_eq!(Keyword::try_from_bytes(format!("x{word}").as_bytes()), None);
        }
        for not in ["", "Foo", "functio", "classs", "é", "fun\u{e7}tion"] {
            assert_eq!(Keyword::try_from_bytes(not.as_bytes()), None, "{not:?}");
        }
        // A span that is not UTF-8 matches no keyword.
        assert_eq!(Keyword::try_from_bytes(b"cl\xffss"), None);
        assert_eq!(Keyword::try_from_bytes(&[0xe9; 3]), None);
        assert_eq!(Keyword::try_from_bytes(&[b'a'; 200]), None);
    }
}

#[cfg(test)]
mod keyword_tests {
    use super::*;

    /// What a keyword is, by comparing the bytes with every spelling.
    fn by_comparison(bytes: &[u8]) -> Option<Keyword> {
        KEYWORDS
            .iter()
            .find(|(spelling, _)| spelling.as_bytes().eq_ignore_ascii_case(bytes))
            .map(|(_, keyword)| keyword.clone())
    }

    #[test]
    fn every_keyword_is_found_in_any_mix_of_cases_and_by_its_own_spelling() {
        for (spelling, keyword) in &KEYWORDS {
            assert_eq!(keyword.as_str(), *spelling);
            let bytes = spelling.as_bytes();
            for mask in 0u32..(1 << bytes.len().min(12)) {
                let mixed: Vec<u8> = bytes
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        if mask >> i & 1 == 1 {
                            b.to_ascii_uppercase()
                        } else {
                            *b
                        }
                    })
                    .collect();
                assert_eq!(
                    Keyword::try_from_bytes(&mixed).as_ref(),
                    Some(keyword),
                    "{mixed:?}"
                );
            }
        }
    }

    #[test]
    fn the_keywords_are_each_in_a_slot_of_their_own() {
        assert_eq!(
            KEYWORD_SLOTS.iter().filter(|&&slot| slot != 0).count(),
            KEYWORDS.len()
        );
    }

    #[test]
    fn a_span_that_is_not_a_keyword_is_none_as_the_comparison_says() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789\x80\xff";
        for _ in 0..200_000 {
            let len = next() as usize % 15;
            let mut bytes: Vec<u8> = (0..len)
                .map(|_| alphabet[next() as usize % alphabet.len()])
                .collect();
            // Near misses of a keyword too: one byte changed.
            if next() % 2 == 0 {
                let (spelling, _) = &KEYWORDS[next() as usize % KEYWORDS.len()];
                bytes = spelling.as_bytes().to_vec();
                let at = next() as usize % bytes.len();
                bytes[at] = alphabet[next() as usize % alphabet.len()];
            }
            assert_eq!(
                Keyword::try_from_bytes(&bytes),
                by_comparison(&bytes),
                "{bytes:?}"
            );
        }
        for bytes in [
            &b""[..],
            b"functions",
            b"requireonce",
            b"a_very_long_identifier_name",
        ] {
            assert_eq!(Keyword::try_from_bytes(bytes), by_comparison(bytes));
        }
    }
}
