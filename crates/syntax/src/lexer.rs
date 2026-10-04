use std::borrow::Cow;

use crate::name::{Interner, Name, NameCache};
use crate::span::{LineIndex, Span};
use crate::token::{Keyword, Token};
use std::sync::Arc;

mod strings;

/// Something the lexer skipped over, or (`is_error`) something PHP
/// refuses to parse, such as `08` ("Invalid numeric literal").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LexWarning {
    pub span: Span,
    pub message: String,
    pub is_error: bool,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    // The source as text when it is valid UTF-8, which was settled once,
    // where the lexer was made: a name's or a comment's text is then a
    // slice of it, which needs no validation of its own.
    text: Option<&'a str>,
    // The names the files of a load share, asked after `names` and before
    // the interner.
    shared_names: Option<&'a mut NameCache>,
    pos: usize,
    in_php: bool,
    // The file's lines, made where the lexer was made for a whole file,
    // and handed out with the tokens for a position to be made from an
    // offset when one is asked for.
    line_index: Arc<LineIndex>,
    // The byte offset `next_token` most recently settled on as "this is
    // where the token about to be lexed actually starts" (i.e. right
    // after any leading whitespace/comments were skipped) - `tokenize_
    // with_spans` reads this right after each `next_token()` call to
    // know where that token's own span should start, since `next_token`
    // itself skips whitespace *and* lexes in one call.
    last_token_start: usize,
    // Where the source starts in its file, as a byte offset: 0 for a
    // whole file; a string interpolation fragment's own offset when one is
    // lexed on its own, so its tokens get their real spans.
    base: u32,
    // The just-lexed keyword's own original source-text spelling, stashed
    // out of band - `Token::Keyword` itself carries only the
    // keyword's canonical (always-lowercase) spelling, never the raw
    // bytes, so a keyword-shaped identifier (`class Enum { ... }`, a real
    // vendored class literally named the same as the `enum` keyword)
    // would otherwise have its true declared-name case lost the moment
    // it's lexed. `tokenize_with_spans` reads this right after each
    // `next_token()` call, exactly like `last_token_start` above.
    pending_keyword_raw: Option<String>,
    // Whether the byte ranges of every `//`, `#` and `/* */` comment are
    // kept in `comments`, for the readers that paint or list them.
    keep_comments: bool,
    // Byte ranges of every `//`, `#` and `/* */` comment skipped so far
    // (doc comments are tokens of their own), when `keep_comments`.
    comments: Vec<(usize, usize)>,
    // The comments holding `@phpstan-ignore`, doc comments included, in
    // source order.
    ignore_marks: Vec<IgnoreMark>,
    // The start of the latest token that is code, a comment's neighbour.
    last_code_start: Option<usize>,
    // The first of `ignore_marks` whose next code token is not yet known.
    unsettled_marks: usize,
    // What the tokens lexed so far say of the file.
    facts: FileFacts,
    warnings: Vec<LexWarning>,
    // What identifiers, variables and other token text are interned into.
    interner: &'static Interner,
    // The names this lexer has interned, by their text's hash: a file
    // repeats its identifiers and variables, and a repeat is found here
    // without the interner's lock. Held for as long as the lexer is, so
    // the names it returns are those the interner holds for the text.
    names: hashbrown::HashTable<(u64, Name, u32)>,
    // The spellings whose mention the file's facts hold.
    watch: Option<&'static WatchList>,
}

/// A comment that holds `@phpstan-ignore`, which every inline suppression
/// begins with, and what lies next to it in the code: all the scan of
/// suppressions reads of the file's comments and tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreMark {
    /// The comment as written.
    pub text: Box<str>,
    /// Where it starts, as a byte offset.
    pub start: usize,
    /// The line it starts on.
    pub line: u32,
    /// A `/** ... */` doc comment.
    pub is_doc: bool,
    /// A token starts before it on its first line.
    pub code_before_on_line: bool,
    /// The line and byte offset of the first token after it, which is the
    /// first token of its line when no token starts before it on its own.
    pub next_code: Option<(u32, usize)>,
}

/// Whether `text` holds `@phpstan-ignore`, which every marker begins with.
fn has_ignore_marker(text: &[u8]) -> bool {
    static FINDER: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"@phpstan-ignore"));
    FINDER.find(text).is_some()
}

/// Where the `/* */` comment starting at `at` ends: past its `*/`, which
/// is looked for after the `/*`, or at the end of the source.
fn block_comment_end(src: &[u8], at: usize) -> usize {
    let from = (at + 2).min(src.len());
    match memchr::memmem::find(&src[from..], b"*/") {
        Some(found) => from + found + 2,
        None => src.len(),
    }
}

/// Advances past a single- or double-quoted string literal starting at
/// `i` (which must be exactly on the opening quote byte), honoring just
/// enough escaping (`\'`/`\\`) to find the real closing quote - shared
/// by `Lexer::skip_string_literal` (operating on `self.pos`/`self.src`)
/// and `scan_interpolated`'s own separate scan (operating on a local
/// index into an already-extracted byte slice), so both of this
/// lexer's raw bracket/brace-depth counters that need to skip past a
/// string's own content use the identical logic rather than risking a
/// third, independently-drifting copy of it.
fn skip_string_literal_bytes(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let mut i = i + 1;
    while i < bytes.len() && bytes[i] != quote {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
        } else {
            i += 1;
        }
    }
    if i < bytes.len() {
        i += 1;
    }
    i
}

/// `String::from_utf8_lossy`, found by validating the bytes the way
/// `str::from_utf8` does - several bytes at a time for the ASCII most of
/// source is - and decoding lossily only for the rest.
pub(crate) fn lossy(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => std::borrow::Cow::Borrowed(text),
        Err(_) => String::from_utf8_lossy(bytes),
    }
}

/// A token, its span, and a keyword's original spelling.
pub type SpannedToken = (Token, Span, Option<String>);

/// What the lexer found, as the parser reads it: the tokens and their spans
/// in step, and apart from them the spellings of the keywords that are not
/// spelled as the language does and the doc comments, which are not tokens
/// to the parser.
pub struct Lexed {
    /// Every token but a doc comment, `Token::Eof` last.
    pub tokens: Vec<Token>,
    /// `spans[i]` is `tokens[i]`'s.
    pub spans: Vec<Span>,
    /// The original spelling of the keyword `tokens[index]` is, for each
    /// keyword spelled other than in lower case; by index, ascending.
    pub keyword_raw: Vec<(usize, String)>,
    /// The doc comments, in order.
    pub docs: Vec<LexedDoc>,
    /// The lines of the file the tokens are of.
    pub line_index: Arc<LineIndex>,
}

/// A token with what the parser reads along with it.
#[derive(Debug, Clone)]
pub struct Tok {
    pub token: Token,
    pub span: Span,
    /// The doc comments right before it.
    pub docs: Option<Box<DocGroup>>,
    /// A keyword's spelling when it is not in lower case.
    pub raw_keyword: Option<Box<str>>,
}

/// The doc comments right before a token: the last, which is the token's
/// doc comment, and the several folded into one when there are several.
#[derive(Debug, Clone)]
pub struct DocGroup {
    /// The last doc comment, until the declaration it belongs to takes it.
    pub last: Option<Arc<str>>,
    /// All of them as one, when there are more than one.
    pub folded: Option<Arc<str>>,
    /// Where the first starts; none for a group made of a doc comment
    /// moved from before attributes.
    pub first_start: Option<u32>,
}

/// Several consecutive `/** ... */` comments as one: each one's body, in
/// order, inside a single pair of delimiters.
fn fold_doc_comments<'t>(docs: impl Iterator<Item = &'t Arc<str>>) -> Arc<str> {
    let bodies: Vec<&str> = docs
        .map(|d| d.trim().trim_start_matches("/**").trim_end_matches("*/"))
        .collect();
    Arc::from(format!("/**{}\n*/", bodies.join("\n")).as_str())
}

/// What the lexer knows of a file once it has lexed all of it.
pub struct LexEnd {
    pub ignore_marks: Vec<IgnoreMark>,
    pub warnings: Vec<LexWarning>,
    pub facts: FileFacts,
    pub line_index: Arc<LineIndex>,
}

/// What is true of a file as a whole, found as it is lexed and parsed, so
/// that a walk over its tree that can only find something in a file where
/// the fact holds can be left out of the others.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileFacts {
    /// The file has an anonymous class (`new class`).
    pub has_anonymous_class: bool,
    /// The file declares a class, function or constant in the body of a
    /// closure or an anonymous class, which a walk of its statements alone
    /// does not reach.
    pub has_nested_declaration: bool,
    /// The bits of the `WatchList` the lexer was given for the spellings
    /// the file names: an identifier, a whole name or one segment of a
    /// qualified one, in any case.
    pub mentions: u32,
    /// Where the file first spells each syntax only some PHP versions
    /// accept and its tree does not keep.
    pub syntax: SyntaxMarks,
}

/// A spelling only some PHP versions accept, which the tree does not keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxMark {
    /// `public`, `protected` or `private` on a class constant (PHP 7.1).
    ClassConstVisibility,
    /// A trailing comma in a grouped `use` (7.2).
    TrailingCommaInGroupedUse,
    /// A trailing comma in the arguments of a call, `isset` or `unset`
    /// (7.3).
    TrailingCommaInArguments,
    /// A heredoc or nowdoc whose closing label is indented, or followed on
    /// its line by more than a `;` (7.3).
    FlexibleHeredoc,
    /// `_` between the digits of a number (7.4).
    NumericLiteralSeparator,
    /// A trailing comma in a parameter list (8.0).
    TrailingCommaInParameters,
    /// A trailing comma in a closure's `use` list (8.0).
    TrailingCommaInClosureUse,
    /// `0o` before an octal number (8.1).
    ExplicitOctal,
}

impl SyntaxMark {
    pub const ALL: [SyntaxMark; 8] = [
        SyntaxMark::ClassConstVisibility,
        SyntaxMark::TrailingCommaInGroupedUse,
        SyntaxMark::TrailingCommaInArguments,
        SyntaxMark::FlexibleHeredoc,
        SyntaxMark::NumericLiteralSeparator,
        SyntaxMark::TrailingCommaInParameters,
        SyntaxMark::TrailingCommaInClosureUse,
        SyntaxMark::ExplicitOctal,
    ];
}

/// The first place in a file of each `SyntaxMark`: PHP rejects a file at
/// the first syntax its version lacks, so the first is all a report needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SyntaxMarks([Option<Span>; SyntaxMark::ALL.len()]);

impl SyntaxMarks {
    /// Notes `mark` at `span`, unless an earlier place has it.
    pub fn note(&mut self, mark: SyntaxMark, span: Span) {
        let slot = &mut self.0[mark as usize];
        if slot.is_none_or(|first| span.start < first.start) {
            *slot = Some(span);
        }
    }

    /// Where the file first spells `mark`.
    pub fn first(&self, mark: SyntaxMark) -> Option<Span> {
        self.0[mark as usize]
    }

    /// Takes in what `other` noted.
    pub fn merge(&mut self, other: &SyntaxMarks) {
        for mark in SyntaxMark::ALL {
            if let Some(span) = other.first(mark) {
                self.note(mark, span);
            }
        }
    }
}

/// The identifier spellings a caller wants to know a file names, each
/// with the bit it sets in `FileFacts::mentions`: the lists of the walks
/// that can find something only in a file that names one of them, each
/// list with its own bits. The spellings are in lower case.
pub struct WatchList(pub &'static [&'static [(&'static str, u32)]]);

impl WatchList {
    /// The bits of the spellings equal to `text` in any case.
    fn bits(&self, text: &str) -> u32 {
        let mut bits = 0;
        for list in self.0 {
            for &(spelling, bit) in *list {
                if spelling.len() == text.len() && spelling.eq_ignore_ascii_case(text) {
                    bits |= bit;
                }
            }
        }
        bits
    }
}

/// A doc comment, and the token it comes before.
pub struct LexedDoc {
    /// The index in `Lexed::tokens` of the next token.
    pub before: usize,
    pub text: Arc<str>,
    pub span: Span,
}

impl Lexed {
    /// The tokens the way the lexer hands them out one at a time: each
    /// with the doc comments before it and its keyword's spelling.
    pub fn into_toks(self) -> (Vec<Tok>, Arc<LineIndex>) {
        let Lexed {
            tokens,
            spans,
            keyword_raw,
            docs,
            line_index,
        } = self;
        let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
        let mut docs = docs.into_iter().peekable();
        let mut raws = keyword_raw.into_iter().peekable();
        for (index, (token, span)) in tokens.into_iter().zip(spans).enumerate() {
            let mut group: Vec<(Arc<str>, u32)> = Vec::new();
            while let Some(doc) = docs.next_if(|doc| doc.before == index) {
                group.push((doc.text, doc.span.start));
            }
            let docs = match group.as_slice() {
                [] => None,
                [(text, start)] => Some(Box::new(DocGroup {
                    last: Some(Arc::clone(text)),
                    folded: None,
                    first_start: Some(*start),
                })),
                several => Some(Box::new(DocGroup {
                    last: Some(Arc::clone(&several[several.len() - 1].0)),
                    folded: Some(fold_doc_comments(several.iter().map(|(text, _)| text))),
                    first_start: Some(several[0].1),
                })),
            };
            let raw_keyword = raws
                .next_if(|(i, _)| *i == index)
                .map(|(_, raw)| raw.into_boxed_str());
            out.push(Tok {
                token,
                span,
                docs,
                raw_keyword,
            });
        }
        (out, line_index)
    }

    /// The tokens as the spanned list the lexer made before the parser read
    /// the vectors of `Lexed`: a doc comment a token of its own, before the
    /// token it precedes, and every keyword with its spelling or none.
    pub fn into_spanned(self) -> Vec<SpannedToken> {
        let Lexed {
            tokens,
            spans,
            keyword_raw,
            docs,
            ..
        } = self;
        let mut out: Vec<SpannedToken> = Vec::with_capacity(tokens.len() + docs.len());
        let mut docs = docs.into_iter().peekable();
        let mut raws = keyword_raw.into_iter().peekable();
        for (index, (token, span)) in tokens.into_iter().zip(spans).enumerate() {
            while let Some(doc) = docs.next_if(|doc| doc.before == index) {
                out.push((Token::DocComment(doc.text), doc.span, None));
            }
            let raw = raws.next_if(|(i, _)| *i == index).map(|(_, raw)| raw);
            out.push((token, span, raw));
        }
        out
    }
}

/// `Lexer::lex`'s result.
pub struct LexOutput {
    pub lexed: Lexed,
    /// Every non-doc comment: its start and its text, for a lexer that
    /// keeps them (`tokenize_with_comments`).
    pub comments: Vec<(Span, String)>,
    /// The comments that hold `@phpstan-ignore`.
    pub ignore_marks: Vec<IgnoreMark>,
    pub warnings: Vec<LexWarning>,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer::build(src.as_bytes(), Some(src), false)
    }

    /// A file's source as its bytes, which need not be UTF-8: a string
    /// literal keeps them as written. Whether they are valid UTF-8 is
    /// settled here, once.
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        Lexer::build(bytes, std::str::from_utf8(bytes).ok(), false)
    }

    /// For re-lexing a fragment of PHP that is already known to be code,
    /// e.g. the inside of a `{$...}` string interpolation. Skips inline-HTML
    /// mode entirely.
    pub fn new_in_php(src: &'a str) -> Self {
        Lexer::build(src.as_bytes(), Some(src), true)
    }

    fn build(bytes: &'a [u8], text: Option<&'a str>, in_php: bool) -> Self {
        Lexer {
            src: bytes,
            text,
            shared_names: None,
            pos: 0,
            in_php,
            line_index: Arc::new(if in_php {
                LineIndex::default()
            } else {
                LineIndex::new(bytes)
            }),
            last_token_start: 0,
            base: 0,
            pending_keyword_raw: None,
            keep_comments: false,
            comments: Vec::new(),
            ignore_marks: Vec::new(),
            last_code_start: None,
            unsettled_marks: 0,
            facts: FileFacts::default(),
            warnings: Vec::new(),
            interner: Interner::global(),
            names: hashbrown::HashTable::new(),
            watch: None,
        }
    }

    /// The source in `range` as text: a slice of the text when the source
    /// is UTF-8, which a range of identifier bytes always is, and the
    /// lossy reading of the bytes when it is not.
    fn slice(&self, range: std::ops::Range<usize>) -> std::borrow::Cow<'a, str> {
        if let Some(text) = self.text
            && let Some(slice) = text.get(range.clone())
        {
            return std::borrow::Cow::Borrowed(slice);
        }
        let src: &'a [u8] = self.src;
        lossy(&src[range])
    }

    /// The name for `text` as the parser asks for one, by the same tables
    /// as the lexer's own.
    pub(crate) fn intern(&mut self, text: &str) -> Name {
        self.name(text)
    }

    /// The name for `text`: this lexer's own, or the interner's.
    fn name(&mut self, text: &str) -> Name {
        self.name_and_bits(text).0
    }

    /// The name of an identifier: its watched bits go in the facts. They
    /// are found when the spelling is first met in the file, which is far
    /// less often than a token.
    fn identifier_name(&mut self, text: &str) -> Name {
        let (name, bits) = self.name_and_bits(text);
        self.facts.mentions |= bits;
        name
    }

    fn name_and_bits(&mut self, text: &str) -> (Name, u32) {
        let hash = crate::name::text_hash(text);
        if let Some((_, name, bits)) = self
            .names
            .find(hash, |(h, name, _)| *h == hash && name.as_str() == text)
        {
            return (name.clone(), *bits);
        }
        let name = match self.shared_names.as_deref_mut() {
            Some(shared) => match shared.find(text, hash) {
                Some(name) => name.clone(),
                None => {
                    let name = self.interner.name_hashed(text, hash);
                    shared.insert(name.clone(), hash);
                    name
                }
            },
            None => self.interner.name_hashed(text, hash),
        };
        let bits = self.watch.map_or(0, |watch| watch.bits(text));
        self.names
            .insert_unique(hash, (hash, name.clone(), bits), |(h, _, _)| *h);
        (name, bits)
    }

    /// Notes the spellings of `watch` in the facts of the file.
    pub fn watching(mut self, watch: &'static WatchList) -> Self {
        self.watch = Some(watch);
        self
    }

    /// Looks a name up in `cache` before it asks the interner, and adds the
    /// ones it takes from the interner. The cache's names must be from the
    /// interner the lexer uses.
    pub fn with_name_cache(mut self, cache: &'a mut NameCache) -> Self {
        self.shared_names = Some(cache);
        self
    }

    /// `new_in_php` for a fragment that starts at the byte offset `base`
    /// in its file: every span is reported where it is in the file.
    pub fn fragment(src: &'a str, base: u32) -> Self {
        Lexer {
            base,
            ..Lexer::new_in_php(src)
        }
    }

    /// The file's offset of the byte `local` in the source being lexed.
    #[inline]
    fn offset(&self, local: usize) -> u32 {
        self.base.saturating_add(local as u32)
    }

    /// The span of the bytes from `from` to `to` of the source being lexed.
    #[inline]
    fn span(&self, from: usize, to: usize) -> Span {
        Span {
            start: self.offset(from),
            end: self.offset(to),
        }
    }

    /// The real, span-carrying tokenization entry point - `tokenize`
    /// below is a thin adapter over this for the handful of tests that
    /// want to compare bare tokens without spans getting in the way.
    #[hotpath::measure]
    pub fn tokenize_with_spans(mut self) -> Vec<(Token, Span, Option<String>)> {
        self.run()
    }

    /// `tokenize_with_spans`, plus every non-doc comment as its start
    /// position and source text, and the warnings for what was skipped.
    pub fn tokenize_with_comments(
        mut self,
    ) -> (Vec<SpannedToken>, Vec<(Span, String)>, Vec<LexWarning>) {
        self.keep_comments = true;
        let LexOutput {
            lexed,
            comments,
            warnings,
            ..
        } = self.lex();
        (lexed.into_spanned(), comments, warnings)
    }

    /// The tokens as the parser reads them, with every non-doc comment as
    /// its start position and source text, and the warnings for what was
    /// skipped.
    pub fn lex(mut self) -> LexOutput {
        let lexed = self.run_lexed();
        let comments = std::mem::take(&mut self.comments)
            .into_iter()
            .map(|(start, end)| {
                let text = self.slice(start..end).into_owned();
                (self.span(start, end), text)
            })
            .collect();
        LexOutput {
            lexed,
            comments,
            ignore_marks: std::mem::take(&mut self.ignore_marks),
            warnings: self.warnings,
        }
    }

    /// Notes the comment at `start` holding the text `text`.
    fn mark_ignore(&mut self, start: usize, text: &str, is_doc: bool) {
        let line = self.line_index.line(self.offset(start));
        let code_before_on_line = self
            .last_code_start
            .is_some_and(|code| self.line_index.line(self.offset(code)) == line);
        self.ignore_marks.push(IgnoreMark {
            text: text.into(),
            start,
            line,
            is_doc,
            code_before_on_line,
            next_code: None,
        });
    }

    /// A comment skipped between two tokens: kept when asked, and noted
    /// when it holds `@phpstan-ignore`.
    fn skipped_comment(&mut self, start: usize, end: usize) {
        if self.keep_comments {
            self.comments.push((start, end));
        }
        if has_ignore_marker(&self.src[start..end]) {
            let text = self.slice(start..end);
            self.mark_ignore(start, &text, false);
        }
    }

    /// The code token at `start`, on `line`, is the next one after each
    /// comment noted since the one before it, and the latest code.
    fn settle_marks(&mut self, start: usize) {
        if self.unsettled_marks < self.ignore_marks.len() {
            let line = self.line_index.line(self.offset(start));
            for mark in &mut self.ignore_marks[self.unsettled_marks..] {
                mark.next_code = Some((line, start));
            }
            self.unsettled_marks = self.ignore_marks.len();
        }
        self.last_code_start = Some(start);
    }

    /// The next token, a doc comment among them, with its span and, for a
    /// keyword spelled other than in lower case, its spelling. A comment
    /// that holds `@phpstan-ignore` is noted, and so are the names the
    /// file's facts hold.
    fn next_raw(&mut self) -> (Token, Span, Option<String>) {
        let token = self.next_token();
        let raw = self.pending_keyword_raw.take();
        let span = self.span(self.last_token_start, self.pos);
        match &token {
            Token::DocComment(text) => {
                if has_ignore_marker(text.as_bytes()) {
                    self.mark_ignore(self.last_token_start, text, true);
                }
            }
            Token::Eof => {}
            _ => self.settle_marks(self.last_token_start),
        }
        (token, span, raw)
    }

    /// The next token that is not a doc comment, with the doc comments
    /// right before it, which a declaration or a statement reads. Past
    /// the end it is `Token::Eof` again.
    pub fn next_tok(&mut self) -> Tok {
        let mut docs: Vec<(Arc<str>, u32)> = Vec::new();
        loop {
            let (token, span, raw_keyword) = self.next_raw();
            if let Token::DocComment(text) = token {
                docs.push((text, span.start));
                continue;
            }
            let docs = match docs.as_slice() {
                [] => None,
                [(text, start)] => Some(Box::new(DocGroup {
                    last: Some(Arc::clone(text)),
                    folded: None,
                    first_start: Some(*start),
                })),
                several => Some(Box::new(DocGroup {
                    last: Some(Arc::clone(&several[several.len() - 1].0)),
                    folded: Some(fold_doc_comments(several.iter().map(|(text, _)| text))),
                    first_start: Some(several[0].1),
                })),
            };
            return Tok {
                token,
                span,
                docs,
                raw_keyword: raw_keyword.map(String::into_boxed_str),
            };
        }
    }

    /// The lines of the file being lexed.
    pub fn line_index(&self) -> Arc<LineIndex> {
        Arc::clone(&self.line_index)
    }

    /// Moves to the end of the source without lexing the rest, and
    /// returns the span the end of the file has.
    pub fn skip_to_end(&mut self) -> Span {
        self.pos = self.src.len();
        self.last_token_start = self.pos;
        self.span(self.pos, self.pos)
    }

    /// Lexes what is left, which the lexer's own record of the file holds.
    pub fn finish(mut self) -> LexEnd {
        while !matches!(self.next_tok().token, Token::Eof) {}
        LexEnd {
            ignore_marks: std::mem::take(&mut self.ignore_marks),
            warnings: std::mem::take(&mut self.warnings),
            facts: self.facts,
            line_index: Arc::clone(&self.line_index),
        }
    }

    fn run(&mut self) -> Vec<(Token, Span, Option<String>)> {
        self.run_lexed().into_spanned()
    }

    fn run_lexed(&mut self) -> Lexed {
        // A real PHP file averages well under one token per 4 source
        // bytes (identifiers/keywords/string contents are usually several
        // bytes each, and whitespace/comments between tokens contribute
        // to `src.len()` without adding a token at all) - this is a
        // deliberately generous estimate, not a tight one, so the common
        // case needs zero of `Vec::push`'s own amortized-growth
        // reallocations instead of the ~log2(n) a bare `Vec::new()` would
        // pay for a file with thousands of tokens.
        let estimate = self.src.len() / 4;
        let mut tokens: Vec<Token> = Vec::with_capacity(estimate);
        let mut spans: Vec<Span> = Vec::with_capacity(estimate);
        let mut keyword_raw: Vec<(usize, String)> = Vec::new();
        let mut docs: Vec<LexedDoc> = Vec::new();
        loop {
            let (token, span, raw) = self.next_raw();
            if let Token::DocComment(text) = token {
                docs.push(LexedDoc {
                    before: tokens.len(),
                    text,
                    span,
                });
                continue;
            }
            let done = matches!(token, Token::Eof);
            if let Some(raw) = raw {
                keyword_raw.push((tokens.len(), raw));
            }
            tokens.push(token);
            spans.push(span);
            if done {
                break;
            }
        }
        Lexed {
            tokens,
            spans,
            keyword_raw,
            docs,
            line_index: Arc::clone(&self.line_index),
        }
    }

    pub fn tokenize(self) -> Vec<Token> {
        self.tokenize_with_spans()
            .into_iter()
            .map(|(t, _, _)| t)
            .collect()
    }

    fn peek(&self) -> u8 {
        *self.src.get(self.pos).unwrap_or(&0)
    }

    fn peek_at(&self, offset: usize) -> u8 {
        *self.src.get(self.pos + offset).unwrap_or(&0)
    }

    fn bump(&mut self) -> u8 {
        let c = self.peek();
        self.pos += 1;
        c
    }

    /// Whether the source continues with `s` here: a comparison of `N`
    /// bytes the compiler lays out in place, not a call to compare a slice.
    fn starts_with<const N: usize>(&self, s: &[u8; N]) -> bool {
        self.src
            .get(self.pos..self.pos + N)
            .is_some_and(|head| head == s)
    }

    /// `starts_with` for text known only at run time, a heredoc's label.
    fn starts_with_text(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s.as_bytes())
    }

    /// `starts_with` ignoring ASCII case, as PHP reads `<?PHP` and keywords.
    fn starts_with_ignore_case(&self, s: &str) -> bool {
        self.src[self.pos..]
            .get(..s.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(s.as_bytes()))
    }

    fn next_token(&mut self) -> Token {
        if !self.in_php {
            self.last_token_start = self.pos;
            return self.lex_inline_html();
        }

        self.skip_whitespace_and_comments();
        self.last_token_start = self.pos;

        if self.pos >= self.src.len() {
            return Token::Eof;
        }

        // What a token is is settled by its first byte: one jump on its
        // class, and the tests that read more of the source are asked only
        // where the class fits. The rare forms are in functions of their
        // own, off the hot path.
        match BYTE_CLASS[usize::from(self.peek())] {
            ByteClass::IdentStart => self.lex_word(),
            ByteClass::Digit => self.lex_number(),
            ByteClass::Dot => {
                if self.peek_at(1).is_ascii_digit() {
                    self.lex_number()
                } else {
                    self.lex_operator()
                }
            }
            ByteClass::Dollar => self.lex_dollar(),
            ByteClass::Quote1 => self.lex_single_quoted(),
            ByteClass::Quote2 => self.lex_double_quoted(),
            ByteClass::Backtick => self.lex_backtick(),
            ByteClass::Lt => {
                if self.starts_with(b"<<<") {
                    self.lex_heredoc()
                } else {
                    self.lex_operator()
                }
            }
            ByteClass::Slash => {
                if self.starts_with(b"/**") {
                    self.lex_doc_comment()
                } else {
                    self.lex_operator()
                }
            }
            ByteClass::Question => {
                if self.starts_with(b"?>") {
                    self.lex_close_tag()
                } else {
                    self.lex_operator()
                }
            }
            ByteClass::Hash => {
                // A general attribute group - `skip_whitespace_and_comments`
                // deliberately leaves this un-consumed (see its own
                // comment) so it reaches here as a real token instead of
                // being skipped.
                if self.starts_with(b"#[") {
                    self.pos += 2;
                    Token::AttributeStart
                } else {
                    self.lex_operator()
                }
            }
            ByteClass::Operator => self.lex_operator(),
        }
    }

    /// `?>`: PHP mode ends, and the tag stands for a statement terminator.
    #[cold]
    fn lex_close_tag(&mut self) -> Token {
        self.pos += 2;
        // A single newline right after `?>` belongs to the tag.
        if self.starts_with(b"\r\n") {
            self.pos += 2;
        } else if self.starts_with(b"\n") {
            self.pos += 1;
        }
        // ?> implies a statement terminator in real PHP; the parser
        // treats end-of-inline-html the same as a semicolon boundary.
        self.in_php = false;
        Token::Semicolon
    }

    /// A token that starts with `$`: a variable, `${expr}` (the older
    /// "variable variable" syntax, where the variable's own name is
    /// computed at runtime by evaluating expr, most commonly just
    /// `${$name}`) or `$$name` (the brace-less form, equivalent to
    /// `${$name}`; the inner `$name` becomes its own Variable token next).
    fn lex_dollar(&mut self) -> Token {
        let next = self.peek_at(1);
        if is_ident_start(next) {
            self.pos += 1;
            let s = self.lex_ident_raw();
            return Token::Variable(self.name(&s));
        }
        match next {
            b'{' => {
                self.pos += 2;
                Token::DollarBrace
            }
            b'$' => {
                self.pos += 1;
                Token::Dollar
            }
            _ => self.lex_operator(),
        }
    }

    /// A token that starts with an identifier byte: a keyword or a name,
    /// or the legacy byte-string prefix (`b'...'` / `b"..."` / `b<<<...`,
    /// a no-op today: PHP 6's binary-vs-unicode string split never
    /// shipped, but the lexical grammar still accepts the prefix), which
    /// is checked ahead of the identifier since a bare `b` would
    /// otherwise be lexed as its own single-letter name, leaving the
    /// string dangling.
    fn lex_word(&mut self) -> Token {
        let c = self.peek();
        if (c == b'b' || c == b'B')
            && (matches!(self.peek_at(1), b'\'' | b'"')
                || self.src[self.pos + 1..].starts_with(b"<<<"))
        {
            self.pos += 1;
            return self.next_token();
        }
        let span = self.lex_ident_span();
        if let Some(kw) = Keyword::try_from_bytes(&self.src[span.clone()]) {
            // Stash the raw source spelling out of band, but only when
            // it actually differs from `kw`'s own canonical (always-
            // lowercase) spelling - `take_name_segment` already falls
            // back to `kw.as_str().to_string()` for a `None` here, so the
            // overwhelmingly common canonically-spelled case (`class`,
            // `function`, `public`, ...) needs no allocation at all.
            self.pending_keyword_raw = if &self.src[span.clone()] == kw.as_str().as_bytes() {
                None
            } else {
                Some(self.slice(span).into_owned())
            };
            return Token::Keyword(kw);
        }
        let s = self.slice(span);
        Token::Identifier(self.identifier_name(&s))
    }

    fn lex_inline_html(&mut self) -> Token {
        // A leading `#!` line is skipped, as the CLI does, not inline HTML.
        if self.pos == 0 && self.starts_with(b"#!") {
            while self.pos < self.src.len() && self.peek() != b'\n' {
                self.pos += 1;
            }
            self.pos = (self.pos + 1).min(self.src.len());
        }
        let start = self.pos;
        while self.pos < self.src.len()
            && !self.starts_with_ignore_case("<?php")
            && !self.starts_with(b"<?=")
        {
            self.pos += 1;
        }
        let html = lossy(&self.src[start..self.pos]);

        if self.starts_with_ignore_case("<?php") {
            self.pos += 5;
            self.in_php = true;
        } else if self.starts_with(b"<?=") {
            // The HTML before it is its own token; `<?=` is left for the
            // next call, which surfaces it as `echo`.
            if !html.is_empty() {
                return Token::InlineHtml(self.interner.str(&html));
            }
            self.pos += 3;
            self.in_php = true;
            // `<?=` is shorthand for `echo `; surface it as the keyword so
            // the parser can treat it identically to an echo statement.
            return Token::Keyword(Keyword::Echo);
        }

        if html.is_empty() {
            if self.pos >= self.src.len() {
                return Token::Eof;
            }
            return self.next_token();
        }
        Token::InlineHtml(self.interner.str(&html))
    }

    /// Advances past a single- or double-quoted string literal starting
    /// at the current position (which must be exactly on the opening
    /// quote byte), honoring escaping just enough to find the real
    /// closing quote - a raw byte-level bracket/brace-depth counter that
    /// doesn't call this before resuming its own count can be fooled by
    /// an unbalanced `[`/`]`/`{`/`}` inside the string's own content
    /// (e.g. a regex character class like `'/[^\]]+/'`). Shared by every
    /// one of this lexer's own raw depth-counting scanners that needs it
    /// (the two special-cased stub attributes' own argument/whole-
    /// declaration skips, and `{$...}` complex-interpolation scanning)
    /// rather than duplicating the same fix independently in each. A
    /// general `#[Attribute(...)]` group needs no such handling: it's
    /// real tokens now, so a string literal inside its arguments is
    /// already just one ordinary token, immune to this problem by
    /// construction.
    fn skip_string_literal(&mut self) {
        self.pos = skip_string_literal_bytes(self.src, self.pos);
    }

    /// Where the line comment at the position ends: at the newline, or at
    /// the `?>` that closes PHP mode, or at the end of the source.
    fn line_comment_end(&self) -> usize {
        let mut at = self.pos;
        while let Some(found) = memchr::memchr2(b'\n', b'?', &self.src[at..]) {
            let end = at + found;
            if self.src[end] == b'\n' || self.src.get(end + 1) == Some(&b'>') {
                return end;
            }
            at = end + 1;
        }
        self.src.len()
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.pos += class_run(&self.src[self.pos..], &BLANK);
                }
                b'#' => {
                    if self.starts_with(b"#[") {
                        // An `#[Attribute(...)]` group is real code, not
                        // a comment. Breaking out of this whitespace/
                        // comment loop (without consuming `#[` itself)
                        // hands control back to `next_token`, which emits
                        // a real `AttributeStart` token for it and lets
                        // ordinary tokenization (identifiers, `(`,
                        // literals, `,`, `]`, ...) take over from there -
                        // real code, so no separate string-aware
                        // bracket-depth counter is needed: a string
                        // literal's own content becomes one real token
                        // regardless of what bracket-like bytes it
                        // contains.
                        break;
                    } else {
                        let start = self.pos;
                        self.pos = self.line_comment_end();
                        self.skipped_comment(start, self.pos);
                    }
                }
                b'/' if self.peek_at(1) == b'/' => {
                    let start = self.pos;
                    self.pos = self.line_comment_end();
                    self.skipped_comment(start, self.pos);
                }
                // `/**/` (empty) is conventionally just a regular comment,
                // not a doc comment - PHP's own tokenizer draws the line the
                // same way. Check that case first since it shares its
                // middle `*` between the `/**`-looking open and the `*/`
                // close.
                b'/' if self.starts_with(b"/**") && self.peek_at(3) != b'/' => break,
                b'/' if self.peek_at(1) == b'*' => {
                    let start = self.pos;
                    self.pos = block_comment_end(self.src, self.pos);
                    self.skipped_comment(start, self.pos);
                }
                _ => break,
            }
        }
    }

    /// Reports the number just lexed from `start` when PHP wouldn't read
    /// all of it as one (`1e`, `0x`, `1__0`, `1_`): the rest is an
    /// unexpected identifier to PHP.
    fn check_numeric_token(&mut self, start: usize) {
        if is_numeric_token(&self.src[start..self.pos]) {
            return;
        }
        let span = self.span(start, self.pos);
        self.warnings.push(LexWarning {
            span,
            message: "syntax error, unexpected identifier".to_string(),
            is_error: true,
        });
    }

    /// A numeric literal's digit run with PHP's `_` digit separators
    /// (`1_000_000`) stripped, borrowed when it has none. `raw` is ASCII,
    /// the only bytes `lex_number`'s scan advances over.
    fn strip_digit_separators(raw: &[u8]) -> Cow<'_, str> {
        let s = std::str::from_utf8(raw).unwrap_or_default();
        if raw.contains(&b'_') {
            Cow::Owned(s.replace('_', ""))
        } else {
            Cow::Borrowed(s)
        }
    }

    /// Scans an identifier-shaped run without allocating - returns its
    /// raw byte span. Split out from the owned-`String`-returning `lex_
    /// ident_raw` below so `next_token`'s identifier branch can check
    /// `Keyword::try_from_bytes` against the raw bytes first, only
    /// allocating a `String` once the token is confirmed to be a plain
    /// `Token::Identifier`, not a keyword - the identifier text is
    /// otherwise thrown away immediately whenever it does match a
    /// keyword, wasting that allocation entirely.
    fn lex_ident_span(&mut self) -> std::ops::Range<usize> {
        let start = self.pos;
        let rest = &self.src[start.min(self.src.len())..];
        self.pos = start + class_run(rest, &IDENT_CONTINUE);
        start..self.pos
    }

    fn lex_ident_raw(&mut self) -> std::borrow::Cow<'a, str> {
        let span = self.lex_ident_span();
        self.slice(span)
    }

    /// A number, its separators and explicit octal prefix noted in the
    /// facts.
    fn lex_number(&mut self) -> Token {
        let start = self.pos;
        let token = self.lex_number_token();
        let text = &self.src[start..self.pos];
        let span = self.span(start, self.pos);
        if text.contains(&b'_') {
            self.facts
                .syntax
                .note(SyntaxMark::NumericLiteralSeparator, span);
        }
        if text.len() > 1 && text[0] == b'0' && matches!(text[1], b'o' | b'O') {
            self.facts.syntax.note(SyntaxMark::ExplicitOctal, span);
        }
        token
    }

    fn lex_number_token(&mut self) -> Token {
        let start = self.pos;
        let mut is_float = false;

        if self.peek() == b'0' && (self.peek_at(1) == b'x' || self.peek_at(1) == b'X') {
            self.pos += 2;
            while self.peek().is_ascii_hexdigit() || self.peek() == b'_' {
                self.pos += 1;
            }
            self.check_numeric_token(start);
            let text = Self::strip_digit_separators(&self.src[start + 2..self.pos]);
            return radix_literal(&text, 16);
        }

        if self.peek() == b'0' && (self.peek_at(1) == b'b' || self.peek_at(1) == b'B') {
            self.pos += 2;
            while self.peek() == b'0' || self.peek() == b'1' || self.peek() == b'_' {
                self.pos += 1;
            }
            self.check_numeric_token(start);
            let text = Self::strip_digit_separators(&self.src[start + 2..self.pos]);
            return radix_literal(&text, 2);
        }

        // PHP 8.1's explicit octal notation (`0o775`); the legacy
        // leading-zero form (`0775`) is read after the digit loop below.
        if self.peek() == b'0' && (self.peek_at(1) == b'o' || self.peek_at(1) == b'O') {
            self.pos += 2;
            while (b'0'..=b'7').contains(&self.peek()) || self.peek() == b'_' {
                self.pos += 1;
            }
            self.check_numeric_token(start);
            let text = Self::strip_digit_separators(&self.src[start + 2..self.pos]);
            return radix_literal(&text, 8);
        }

        while self.peek().is_ascii_digit() || self.peek() == b'_' {
            self.pos += 1;
        }
        // A `.` right after integer digits always starts a float, even
        // with no fractional digits following (`123.` is a valid float
        // literal, equal to `123.0`) - `1.$b` is a syntax error in PHP
        // too (the `.` is greedily consumed into the number, not treated
        // as concat), so this isn't conditional on what follows the dot.
        if self.peek() == b'.' {
            is_float = true;
            self.pos += 1;
            while self.peek().is_ascii_digit() || self.peek() == b'_' {
                self.pos += 1;
            }
        }
        if self.peek() == b'e' || self.peek() == b'E' {
            is_float = true;
            self.pos += 1;
            if self.peek() == b'+' || self.peek() == b'-' {
                self.pos += 1;
            }
            while self.peek().is_ascii_digit() || self.peek() == b'_' {
                self.pos += 1;
            }
        }

        self.check_numeric_token(start);
        let text = Self::strip_digit_separators(&self.src[start..self.pos]);
        if is_float {
            Token::Float(text.parse().unwrap_or(0.0))
        } else if text.len() > 1 && text.starts_with('0') {
            // A legacy octal literal (`0775`); an `8` or `9` in one is a
            // parse error in PHP.
            if !text.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
                let span = self.span(start, self.pos);
                self.warnings.push(LexWarning {
                    span,
                    message: "Invalid numeric literal".to_string(),
                    is_error: true,
                });
            }
            radix_literal(&text[1..], 8)
        } else {
            match text.parse::<i64>() {
                Ok(v) => Token::Integer(v),
                Err(_) => Token::Float(text.parse().unwrap_or(0.0)),
            }
        }
    }

    fn lex_doc_comment(&mut self) -> Token {
        let start = self.pos;
        self.pos += 3; // consume '/**'
        let rest = &self.src[self.pos.min(self.src.len())..];
        self.pos = match memchr::memmem::find(rest, b"*/") {
            Some(found) => self.pos + found + 2,
            None => self.src.len(),
        };
        Token::DocComment(self.interner.str(&self.slice(start..self.pos)))
    }

    fn lex_operator(&mut self) -> Token {
        macro_rules! op {
            ($len:expr, $tok:expr) => {{
                self.pos += $len;
                return $tok;
            }};
        }

        // Dispatched by leading byte first, rather than a flat linear
        // chain of `starts_with` prefix checks: every plain single-byte
        // token (by far the most common class: `;`, `,`, `(`, `)`, `.`,
        // ...) would otherwise pay the full chain of failed checks before
        // ever reaching the single-byte match below. Each byte arm keeps
        // its own 3-char-before-2-char ordering intact wherever one
        // prefix's checks could otherwise shadow a longer one sharing the
        // same leading byte (e.g. `<=` is a real prefix of `<=>`, so
        // `<=>` must still be checked first under `b'<'`).
        match self.peek() {
            b'<' => {
                if self.starts_with(b"<=>") {
                    op!(3, Token::Spaceship);
                }
                if self.starts_with(b"<<=") {
                    op!(3, Token::LtLtEquals);
                }
                if self.starts_with(b"<=") {
                    op!(2, Token::LtEquals);
                }
                if self.starts_with(b"<<") {
                    op!(2, Token::LtLt);
                }
                if self.starts_with(b"<>") {
                    op!(2, Token::BangEquals);
                }
            }
            b'=' => {
                if self.starts_with(b"===") {
                    op!(3, Token::EqualsEqualsEquals);
                }
                if self.starts_with(b"==") {
                    op!(2, Token::EqualsEquals);
                }
                if self.starts_with(b"=>") {
                    op!(2, Token::FatArrow);
                }
            }
            b'!' => {
                if self.starts_with(b"!==") {
                    op!(3, Token::BangEqualsEquals);
                }
                if self.starts_with(b"!=") {
                    op!(2, Token::BangEquals);
                }
            }
            b'?' => {
                if self.starts_with(b"?->") {
                    op!(3, Token::NullsafeArrow);
                }
                if self.starts_with(b"??=") {
                    op!(3, Token::QuestionQuestionEquals);
                }
                if self.starts_with(b"??") {
                    op!(2, Token::QuestionQuestion);
                }
            }
            b'*' => {
                if self.starts_with(b"**=") {
                    op!(3, Token::StarStarEquals);
                }
                if self.starts_with(b"**") {
                    op!(2, Token::StarStar);
                }
                if self.starts_with(b"*=") {
                    op!(2, Token::StarEquals);
                }
            }
            b'.' => {
                if self.starts_with(b"...") {
                    op!(3, Token::Ellipsis);
                }
                if self.starts_with(b".=") {
                    op!(2, Token::DotEquals);
                }
            }
            b'>' => {
                if self.starts_with(b">>=") {
                    op!(3, Token::GtGtEquals);
                }
                if self.starts_with(b">=") {
                    op!(2, Token::GtEquals);
                }
                if self.starts_with(b">>") {
                    op!(2, Token::GtGt);
                }
            }
            b'-' => {
                if self.starts_with(b"->") {
                    op!(2, Token::Arrow);
                }
                if self.starts_with(b"-=") {
                    op!(2, Token::MinusEquals);
                }
                if self.starts_with(b"--") {
                    op!(2, Token::Decrement);
                }
            }
            b':' => {
                if self.starts_with(b"::") {
                    op!(2, Token::DoubleColon);
                }
            }
            b'+' => {
                if self.starts_with(b"+=") {
                    op!(2, Token::PlusEquals);
                }
                if self.starts_with(b"++") {
                    op!(2, Token::Increment);
                }
            }
            b'/' => {
                if self.starts_with(b"/=") {
                    op!(2, Token::SlashEquals);
                }
            }
            b'%' => {
                if self.starts_with(b"%=") {
                    op!(2, Token::PercentEquals);
                }
            }
            b'&' => {
                if self.starts_with(b"&=") {
                    op!(2, Token::AmpEquals);
                }
                if self.starts_with(b"&&") {
                    op!(2, Token::AmpAmp);
                }
            }
            b'|' => {
                if self.starts_with(b"|=") {
                    op!(2, Token::PipeEquals);
                }
                if self.starts_with(b"||") {
                    op!(2, Token::PipePipe);
                }
                if self.starts_with(b"|>") {
                    op!(2, Token::PipeGt);
                }
            }
            b'^' if self.starts_with(b"^=") => {
                op!(2, Token::CaretEquals);
            }
            _ => {}
        }

        let c = self.bump();
        match c {
            b'(' => Token::LParen,
            b')' => Token::RParen,
            b'{' => Token::LBrace,
            b'}' => Token::RBrace,
            b'[' => Token::LBracket,
            b']' => Token::RBracket,
            b';' => Token::Semicolon,
            b',' => Token::Comma,
            b':' => Token::Colon,
            b'?' => Token::Question,
            b'.' => Token::Dot,
            b'+' => Token::Plus,
            b'-' => Token::Minus,
            b'*' => Token::Star,
            b'/' => Token::Slash,
            b'%' => Token::Percent,
            b'=' => Token::Equals,
            b'<' => Token::Lt,
            b'>' => Token::Gt,
            b'!' => Token::Bang,
            b'&' => Token::Amp,
            b'|' => Token::Pipe,
            b'^' => Token::Caret,
            b'~' => Token::Tilde,
            b'@' => Token::At,
            b'\\' => Token::Backslash,
            0 => Token::Eof,
            other => {
                let span = self.span(self.pos - 1, self.pos);
                self.warnings.push(LexWarning {
                    span,
                    message: format!("unexpected byte 0x{other:02x} in PHP source, skipping"),
                    is_error: false,
                });
                self.next_token()
            }
        }
    }
}

/// A hex, binary or octal integer literal's value: an `int` when it fits,
/// a `float` past `PHP_INT_MAX`, as PHP reads it. PHP's binary and octal
/// conversions (`zend_bin_strtod`, `zend_oct_strtod`) compute
/// `value * radix + c - '0'` in doubles, adding the digit's character
/// code and then subtracting `'0'`, which rounds twice once the value is
/// past 2^53 (`0b1` and 63 zeros is 2^63 - 1024); the hex one adds the
/// digit's value.
fn radix_literal(digits: &str, radix: u32) -> Token {
    match i64::from_str_radix(digits, radix) {
        Ok(v) => Token::Integer(v),
        Err(_) if digits.is_empty() => Token::Integer(0),
        Err(_) if radix == 16 => Token::Float(digits.chars().fold(0.0, |acc, c| {
            acc * 16.0 + f64::from(c.to_digit(16).unwrap_or(0))
        })),
        Err(_) => Token::Float(digits.bytes().fold(0.0, |acc, c| {
            (acc * f64::from(radix) + f64::from(c)) - f64::from(b'0')
        })),
    }
}

fn is_ident_start(c: u8) -> bool {
    IDENT_START[usize::from(c)]
}

fn is_ident_continue(c: u8) -> bool {
    IDENT_CONTINUE[usize::from(c)]
}

/// What a token's first byte says it is.
#[derive(Clone, Copy)]
enum ByteClass {
    IdentStart,
    Digit,
    Dot,
    Dollar,
    Quote1,
    Quote2,
    Backtick,
    Lt,
    Slash,
    Question,
    Hash,
    Operator,
}

const BYTE_CLASS: [ByteClass; 256] = byte_classes();

const fn byte_classes() -> [ByteClass; 256] {
    let mut table = [ByteClass::Operator; 256];
    let mut b = 0;
    while b < 256 {
        let c = b as u8;
        table[b] = if c == b'_' || c.is_ascii_alphabetic() || c >= 0x80 {
            ByteClass::IdentStart
        } else if c.is_ascii_digit() {
            ByteClass::Digit
        } else {
            match c {
                b'.' => ByteClass::Dot,
                b'$' => ByteClass::Dollar,
                b'\'' => ByteClass::Quote1,
                b'"' => ByteClass::Quote2,
                b'`' => ByteClass::Backtick,
                b'<' => ByteClass::Lt,
                b'/' => ByteClass::Slash,
                b'?' => ByteClass::Question,
                b'#' => ByteClass::Hash,
                _ => ByteClass::Operator,
            }
        };
        b += 1;
    }
    table
}

/// The whitespace bytes.
const BLANK: [bool; 256] = {
    let mut table = [false; 256];
    table[b' ' as usize] = true;
    table[b'\t' as usize] = true;
    table[b'\r' as usize] = true;
    table[b'\n' as usize] = true;
    table
};

/// How many bytes at the start of `bytes` are in `class`: eight at a
/// time while all eight are, then one at a time.
fn class_run(bytes: &[u8], class: &[bool; 256]) -> usize {
    let mut at = 0;
    while let Some(chunk) = bytes.get(at..at + 8) {
        let all = chunk
            .iter()
            .fold(true, |all, &b| all & class[usize::from(b)]);
        if !all {
            break;
        }
        at += 8;
    }
    at + bytes[at..]
        .iter()
        .take_while(|&&b| class[usize::from(b)])
        .count()
}

/// The bytes an identifier can begin with, and those it can go on with.
const IDENT_START: [bool; 256] = ident_table(false);
const IDENT_CONTINUE: [bool; 256] = ident_table(true);

const fn ident_table(digits: bool) -> [bool; 256] {
    let mut table = [false; 256];
    let mut b = 0;
    while b < 256 {
        let c = b as u8;
        table[b] =
            c == b'_' || c.is_ascii_alphabetic() || c >= 0x80 || (digits && c.is_ascii_digit());
        b += 1;
    }
    table
}

#[cfg(test)]
mod tests;

/// Whether `raw` is one whole PHP number: digits joined by single `_`s,
/// after `0x`/`0b`/`0o` in their base, or decimal with an optional
/// fraction and an exponent that has digits.
fn is_numeric_token(raw: &[u8]) -> bool {
    fn run(bytes: &[u8], digit: fn(&u8) -> bool) -> Option<usize> {
        let mut i = 0;
        loop {
            let start = i;
            while bytes.get(i).is_some_and(digit) {
                i += 1;
            }
            if i == start {
                return None;
            }
            if bytes.get(i) == Some(&b'_') && bytes.get(i + 1).is_some_and(digit) {
                i += 1;
                continue;
            }
            return Some(i);
        }
    }
    let prefixed = |digit: fn(&u8) -> bool| run(&raw[2..], digit) == Some(raw.len() - 2);
    match raw.get(..2) {
        Some(b"0x" | b"0X") => prefixed(u8::is_ascii_hexdigit),
        Some(b"0b" | b"0B") => prefixed(|b| matches!(b, b'0' | b'1')),
        Some(b"0o" | b"0O") => prefixed(|b| matches!(b, b'0'..=b'7')),
        _ => {
            let mut i = run(raw, u8::is_ascii_digit).unwrap_or(0);
            if raw.get(i) == Some(&b'.') {
                i += 1;
                if raw.get(i).is_some_and(u8::is_ascii_digit) {
                    i += run(&raw[i..], u8::is_ascii_digit).unwrap_or(0);
                }
            }
            if i == 0 {
                return false;
            }
            if matches!(raw.get(i), Some(b'e' | b'E')) {
                i += 1;
                if matches!(raw.get(i), Some(b'+' | b'-')) {
                    i += 1;
                }
                match run(&raw[i..], u8::is_ascii_digit) {
                    Some(n) => i += n,
                    None => return false,
                }
            }
            i == raw.len()
        }
    }
}

#[cfg(test)]
mod lossy_tests {
    use super::lossy;

    #[test]
    fn lossy_is_from_utf8_lossy() {
        let samples: [&[u8]; 9] = [
            b"",
            b"plain ascii",
            "caf\u{e9} \u{10348} \u{4e2d}".as_bytes(),
            b"cut \xe2\x82",
            b"\xff\xfe",
            b"a\xc0\xafb",
            b"\xed\xa0\x80",
            b"ok \xf0\x9f\x98\x80 then \xf0\x9f",
            &[0x80; 5],
        ];
        for bytes in samples {
            assert_eq!(
                &*lossy(bytes),
                &*String::from_utf8_lossy(bytes),
                "{bytes:?}"
            );
        }
        // Every byte, alone and after an ASCII letter and a lead byte.
        for b in 0..=255u8 {
            for bytes in [vec![b], vec![b'a', b], vec![0xe2, b], vec![0xf0, 0x9f, b]] {
                assert_eq!(
                    &*lossy(&bytes),
                    &*String::from_utf8_lossy(&bytes),
                    "{bytes:?}"
                );
            }
        }
    }
}
