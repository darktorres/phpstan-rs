use std::borrow::Cow;

use crate::name::Interner;
use crate::token::{Keyword, Position, Span, Token};
use std::sync::Arc;

mod strings;

/// Something the lexer skipped over, or (`is_error`) something PHP
/// refuses to parse, such as `08` ("Invalid numeric literal").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexWarning {
    pub span: Span,
    pub message: String,
    pub is_error: bool,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    in_php: bool,
    // Byte offsets where each line begins (`line_starts[0]` is always 0) -
    // computed once up front so `line_col_at` is a binary search, not an
    // O(n) rescan per token. The standard rustc-`SourceMap`-style
    // technique - means none of the dozens of `self.pos += ...` sites
    // throughout this file need to track line/col incrementally.
    line_starts: Vec<usize>,
    // The byte offset `next_token` most recently settled on as "this is
    // where the token about to be lexed actually starts" (i.e. right
    // after any leading whitespace/comments were skipped) - `tokenize_
    // with_spans` reads this right after each `next_token()` call to
    // know where that token's own span should start, since `next_token`
    // itself skips whitespace *and* lexes in one call.
    last_token_start: usize,
    // `line_col_at`'s positions already resolved on the line it was last
    // asked about: (line index, (byte offset, position) pairs sorted by
    // offset, starting with the line's own start). A column counts the
    // characters from the nearest resolved offset at or before it, so a
    // generated file's million-character line (a Huffman lookup table) is
    // counted about once, even as a string's own positions are resolved
    // before the token starting before them.
    line_marks: (usize, Vec<(usize, Position)>),
    // Whether the source is all ASCII, where a column is a byte count.
    ascii: bool,
    // The latest position resolved, as (byte offset, position, where its
    // line ends): tokens come in source order, so the next one is
    // usually on the same line, later, and costs the characters between.
    cursor: (usize, Position, usize),
    // Where the source starts in its file: line 1, column 1 for a whole
    // file; a string interpolation fragment's own position when one is
    // lexed on its own, so its tokens get their real positions.
    base: Position,
    // The just-lexed keyword's own original source-text spelling, stashed
    // out of band - `Token::Keyword` itself carries only the
    // keyword's canonical (always-lowercase) spelling, never the raw
    // bytes, so a keyword-shaped identifier (`class Enum { ... }`, a real
    // vendored class literally named the same as the `enum` keyword)
    // would otherwise have its true declared-name case lost the moment
    // it's lexed. `tokenize_with_spans` reads this right after each
    // `next_token()` call, exactly like `last_token_start` above.
    pending_keyword_raw: Option<String>,
    // Byte ranges of every `//`, `#` and `/* */` comment skipped so far
    // (doc comments are tokens of their own).
    comments: Vec<(usize, usize)>,
    warnings: Vec<LexWarning>,
    // What identifiers, variables and other token text are interned into.
    interner: Arc<Interner>,
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

fn compute_line_starts(src: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, &b) in src.iter().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// `Lexer::cursor` at the start of the source.
fn first_line_cursor(line_starts: &[usize], len: usize) -> (usize, Position, usize) {
    (
        0,
        Position { line: 1, col: 1 },
        line_starts.get(1).copied().unwrap_or(len + 1),
    )
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
}

/// A doc comment, and the token it comes before.
pub struct LexedDoc {
    /// The index in `Lexed::tokens` of the next token.
    pub before: usize,
    pub text: Arc<str>,
    pub span: Span,
}

impl Lexed {
    /// The tokens as the spanned list the lexer made before the parser read
    /// the vectors of `Lexed`: a doc comment a token of its own, before the
    /// token it precedes, and every keyword with its spelling or none.
    pub fn into_spanned(self) -> Vec<SpannedToken> {
        let Lexed {
            tokens,
            spans,
            keyword_raw,
            docs,
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
    /// Every non-doc comment: its start and its text.
    pub comments: Vec<(Position, String)>,
    pub warnings: Vec<LexWarning>,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer::from_bytes(src.as_bytes())
    }

    /// A file's source as its bytes, which need not be UTF-8: a string
    /// literal keeps them as written.
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        let line_starts = compute_line_starts(bytes);
        Lexer {
            src: bytes,
            pos: 0,
            in_php: false,
            cursor: first_line_cursor(&line_starts, bytes.len()),
            line_starts,
            last_token_start: 0,
            line_marks: (0, vec![(0, Position { line: 1, col: 1 })]),
            ascii: bytes.is_ascii(),
            base: Position { line: 1, col: 1 },
            pending_keyword_raw: None,
            comments: Vec::new(),
            warnings: Vec::new(),
            interner: Arc::clone(Interner::global()),
        }
    }

    /// For re-lexing a fragment of PHP that is already known to be code,
    /// e.g. the inside of a `{$...}` string interpolation. Skips inline-HTML
    /// mode entirely.
    pub fn new_in_php(src: &'a str) -> Self {
        let bytes = src.as_bytes();
        let line_starts = compute_line_starts(bytes);
        Lexer {
            src: bytes,
            pos: 0,
            in_php: true,
            cursor: first_line_cursor(&line_starts, bytes.len()),
            line_starts,
            last_token_start: 0,
            line_marks: (0, vec![(0, Position { line: 1, col: 1 })]),
            ascii: bytes.is_ascii(),
            base: Position { line: 1, col: 1 },
            pending_keyword_raw: None,
            comments: Vec::new(),
            warnings: Vec::new(),
            interner: Arc::clone(Interner::global()),
        }
    }

    /// Interns token text into `interner` rather than the global one.
    pub fn with_interner(mut self, interner: &Arc<Interner>) -> Self {
        self.interner = Arc::clone(interner);
        self
    }

    /// `new_in_php` for a fragment that starts at `base` in its file:
    /// every position is reported where it is in the file.
    pub fn new_in_php_at(src: &'a str, base: Position) -> Self {
        Lexer {
            base,
            ..Lexer::new_in_php(src)
        }
    }

    /// The position of a byte offset in the file (see `base`).
    fn line_col_at(&mut self, byte_pos: usize) -> Position {
        let pos = self.relative_line_col_at(byte_pos);
        if pos.line == 1 {
            Position {
                line: self.base.line,
                col: self.base.col.saturating_add(pos.col - 1),
            }
        } else {
            Position {
                line: self.base.line.saturating_add(pos.line - 1),
                col: pos.col,
            }
        }
    }

    /// Converts an absolute byte offset into a 1-indexed line/column -
    /// `col` counts UTF-8 *characters* from the line's own start, not raw
    /// bytes: real PHP source has multi-byte UTF-8 inside actual code,
    /// not just comments, so a byte-offset column would silently
    /// overshoot the true character position on every such line. The
    /// extra decoding cost is bounded to one line's own prefix, not the
    /// whole file.
    fn line_idx_at(&self, byte_pos: usize) -> usize {
        match self.line_starts.binary_search(&byte_pos) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    fn relative_line_col_at(&mut self, byte_pos: usize) -> Position {
        let byte_pos = byte_pos.min(self.src.len());
        let (offset, from, line_end) = self.cursor;
        if byte_pos >= offset && byte_pos < line_end {
            if byte_pos == offset {
                return from;
            }
            let position = Position {
                line: from.line,
                col: from.col + self.columns(offset, byte_pos) as u32,
            };
            self.cursor = (byte_pos, position, line_end);
            return position;
        }
        if byte_pos >= offset {
            // A later line, as tokens arrive in order: counted from its own
            // start, which the cursor then moves to.
            let line_idx = self.line_idx_at(byte_pos);
            let position = Position {
                line: (line_idx + 1) as u32,
                col: 1 + self.columns(self.line_starts[line_idx], byte_pos) as u32,
            };
            let line_end = self
                .line_starts
                .get(line_idx + 1)
                .copied()
                .unwrap_or(self.src.len() + 1);
            self.cursor = (byte_pos, position, line_end);
            return position;
        }
        self.line_col_from_marks(byte_pos)
    }

    /// How many columns the bytes from `from` to `to` take: a character
    /// each, a byte each when the source is ASCII.
    fn columns(&self, from: usize, to: usize) -> usize {
        if self.ascii {
            to - from
        } else {
            char_count(&self.src[from..to])
        }
    }

    fn line_col_from_marks(&mut self, byte_pos: usize) -> Position {
        let line_idx = self.line_idx_at(byte_pos);
        if self.line_marks.0 != line_idx {
            let line_start = self.line_starts[line_idx];
            let start = Position {
                line: (line_idx + 1) as u32,
                col: 1,
            };
            self.line_marks = (line_idx, vec![(line_start, start)]);
        }
        let at = self
            .line_marks
            .1
            .partition_point(|&(offset, _)| offset <= byte_pos);
        let (offset, from) = self.line_marks.1[at - 1];
        if offset == byte_pos {
            return from;
        }
        let position = Position {
            line: from.line,
            col: from.col + self.columns(offset, byte_pos) as u32,
        };
        self.line_marks.1.insert(at, (byte_pos, position));
        position
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
        self,
    ) -> (Vec<SpannedToken>, Vec<(Position, String)>, Vec<LexWarning>) {
        let LexOutput {
            lexed,
            comments,
            warnings,
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
                let text = String::from_utf8_lossy(&self.src[start..end]).into_owned();
                (self.line_col_at(start), text)
            })
            .collect();
        LexOutput {
            lexed,
            comments,
            warnings: self.warnings,
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
            let token = self.next_token();
            let raw = self.pending_keyword_raw.take();
            let span = Span {
                start: self.line_col_at(self.last_token_start),
                end: self.line_col_at(self.pos),
            };
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

    fn starts_with(&self, s: &str) -> bool {
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

        if self.starts_with("?>") {
            self.pos += 2;
            // A single newline right after `?>` belongs to the tag.
            if self.starts_with("\r\n") {
                self.pos += 2;
            } else if self.starts_with("\n") {
                self.pos += 1;
            }
            // ?> implies a statement terminator in real PHP; the parser
            // treats end-of-inline-html the same as a semicolon boundary.
            self.in_php = false;
            return Token::Semicolon;
        }

        // A general attribute group - `skip_whitespace_and_comments`
        // deliberately leaves this un-consumed (see its own comment) so
        // it reaches here as a real token instead of being skipped. The
        // two special-cased stub attributes never reach this point at
        // all - `skip_whitespace_and_comments` fully consumes them
        // itself before this check ever runs.
        if self.starts_with("#[") {
            self.pos += 2;
            return Token::AttributeStart;
        }

        let c = self.peek();

        if c == b'$' && (is_ident_start(self.peek_at(1))) {
            self.pos += 1;
            let s = self.lex_ident_raw();
            return Token::Variable(self.interner.name(&s));
        }

        // `${expr}` - the older "variable variable" syntax, where the
        // variable's own name is computed at runtime by evaluating expr
        // (most commonly just `${$name}`, but any expression is legal).
        if c == b'$' && self.peek_at(1) == b'{' {
            self.pos += 2;
            return Token::DollarBrace;
        }

        // `$$name` - the brace-less variable-variable form, equivalent to
        // `${$name}`. Falls through here (not identifier-start, not `{`
        // right after the first `$`) since the inner `$name` becomes its
        // own Variable token next.
        if c == b'$' && self.peek_at(1) == b'$' {
            self.pos += 1;
            return Token::Dollar;
        }

        // `b'...'` / `b"..."` / `b<<<...` - the legacy byte-string prefix
        // (a no-op today; PHP 6's binary-vs-unicode string split never
        // shipped, but the lexical grammar still accepts the prefix).
        // Checked ahead of the generic identifier branch below, since a
        // bare `b` would otherwise be lexed as its own single-letter
        // identifier/constant reference, leaving the string dangling.
        if (c == b'b' || c == b'B')
            && (matches!(self.peek_at(1), b'\'' | b'"')
                || self.src[self.pos + 1..].starts_with(b"<<<"))
        {
            self.pos += 1;
            return self.next_token();
        }

        if is_ident_start(c) {
            let span = self.lex_ident_span();
            if let Some(kw) = Keyword::try_from_bytes(&self.src[span.clone()]) {
                // Stash the raw source spelling out of band, but only
                // when it actually differs from `kw`'s own canonical
                // (always-lowercase) spelling - `take_name_segment`
                // already falls back to `kw.as_str().to_string()` for a
                // `None` here, so the overwhelmingly common
                // canonically-spelled case (`class`, `function`,
                // `public`, ...) needs no allocation at all.
                self.pending_keyword_raw = if &self.src[span.clone()] == kw.as_str().as_bytes() {
                    None
                } else {
                    Some(String::from_utf8_lossy(&self.src[span]).into_owned())
                };
                return Token::Keyword(kw);
            }
            let s = String::from_utf8_lossy(&self.src[span]);
            return Token::Identifier(self.interner.name(&s));
        }

        if c.is_ascii_digit() || (c == b'.' && self.peek_at(1).is_ascii_digit()) {
            return self.lex_number();
        }

        if c == b'\'' {
            return self.lex_single_quoted();
        }
        if c == b'"' {
            return self.lex_double_quoted();
        }
        if c == b'`' {
            return self.lex_backtick();
        }
        if self.starts_with("<<<") {
            return self.lex_heredoc();
        }
        if self.starts_with("/**") {
            return self.lex_doc_comment();
        }

        self.lex_operator()
    }

    fn lex_inline_html(&mut self) -> Token {
        // A leading `#!` line is skipped, as the CLI does, not inline HTML.
        if self.pos == 0 && self.starts_with("#!") {
            while self.pos < self.src.len() && self.peek() != b'\n' {
                self.pos += 1;
            }
            self.pos = (self.pos + 1).min(self.src.len());
        }
        let start = self.pos;
        while self.pos < self.src.len()
            && !self.starts_with_ignore_case("<?php")
            && !self.starts_with("<?=")
        {
            self.pos += 1;
        }
        let html = String::from_utf8_lossy(&self.src[start..self.pos]);

        if self.starts_with_ignore_case("<?php") {
            self.pos += 5;
            self.in_php = true;
        } else if self.starts_with("<?=") {
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

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.pos += 1;
                }
                b'#' => {
                    if self.starts_with("#[") {
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
                        while self.pos < self.src.len()
                            && self.peek() != b'\n'
                            && !self.starts_with("?>")
                        {
                            self.pos += 1;
                        }
                        self.comments.push((start, self.pos));
                    }
                }
                b'/' if self.peek_at(1) == b'/' => {
                    let start = self.pos;
                    while self.pos < self.src.len()
                        && self.peek() != b'\n'
                        && !self.starts_with("?>")
                    {
                        self.pos += 1;
                    }
                    self.comments.push((start, self.pos));
                }
                // `/**/` (empty) is conventionally just a regular comment,
                // not a doc comment - PHP's own tokenizer draws the line the
                // same way. Check that case first since it shares its
                // middle `*` between the `/**`-looking open and the `*/`
                // close.
                b'/' if self.starts_with("/**") && self.peek_at(3) != b'/' => break,
                b'/' if self.peek_at(1) == b'*' => {
                    let start = self.pos;
                    self.pos += 2;
                    while self.pos < self.src.len() && !self.starts_with("*/") {
                        self.pos += 1;
                    }
                    self.pos = (self.pos + 2).min(self.src.len());
                    self.comments.push((start, self.pos));
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
        let span = Span {
            start: self.line_col_at(start),
            end: self.line_col_at(self.pos),
        };
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
        while is_ident_continue(self.peek()) {
            self.pos += 1;
        }
        start..self.pos
    }

    fn lex_ident_raw(&mut self) -> std::borrow::Cow<'a, str> {
        let span = self.lex_ident_span();
        let src: &'a [u8] = self.src;
        String::from_utf8_lossy(&src[span])
    }

    fn lex_number(&mut self) -> Token {
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
                let span = Span {
                    start: self.line_col_at(start),
                    end: self.line_col_at(self.pos),
                };
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
        while self.pos < self.src.len() && !self.starts_with("*/") {
            self.pos += 1;
        }
        self.pos = (self.pos + 2).min(self.src.len());
        Token::DocComment(
            self.interner
                .str(&String::from_utf8_lossy(&self.src[start..self.pos])),
        )
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
                if self.starts_with("<=>") {
                    op!(3, Token::Spaceship);
                }
                if self.starts_with("<<=") {
                    op!(3, Token::LtLtEquals);
                }
                if self.starts_with("<=") {
                    op!(2, Token::LtEquals);
                }
                if self.starts_with("<<") {
                    op!(2, Token::LtLt);
                }
                if self.starts_with("<>") {
                    op!(2, Token::BangEquals);
                }
            }
            b'=' => {
                if self.starts_with("===") {
                    op!(3, Token::EqualsEqualsEquals);
                }
                if self.starts_with("==") {
                    op!(2, Token::EqualsEquals);
                }
                if self.starts_with("=>") {
                    op!(2, Token::FatArrow);
                }
            }
            b'!' => {
                if self.starts_with("!==") {
                    op!(3, Token::BangEqualsEquals);
                }
                if self.starts_with("!=") {
                    op!(2, Token::BangEquals);
                }
            }
            b'?' => {
                if self.starts_with("?->") {
                    op!(3, Token::NullsafeArrow);
                }
                if self.starts_with("??=") {
                    op!(3, Token::QuestionQuestionEquals);
                }
                if self.starts_with("??") {
                    op!(2, Token::QuestionQuestion);
                }
            }
            b'*' => {
                if self.starts_with("**=") {
                    op!(3, Token::StarStarEquals);
                }
                if self.starts_with("**") {
                    op!(2, Token::StarStar);
                }
                if self.starts_with("*=") {
                    op!(2, Token::StarEquals);
                }
            }
            b'.' => {
                if self.starts_with("...") {
                    op!(3, Token::Ellipsis);
                }
                if self.starts_with(".=") {
                    op!(2, Token::DotEquals);
                }
            }
            b'>' => {
                if self.starts_with(">>=") {
                    op!(3, Token::GtGtEquals);
                }
                if self.starts_with(">=") {
                    op!(2, Token::GtEquals);
                }
                if self.starts_with(">>") {
                    op!(2, Token::GtGt);
                }
            }
            b'-' => {
                if self.starts_with("->") {
                    op!(2, Token::Arrow);
                }
                if self.starts_with("-=") {
                    op!(2, Token::MinusEquals);
                }
                if self.starts_with("--") {
                    op!(2, Token::Decrement);
                }
            }
            b':' => {
                if self.starts_with("::") {
                    op!(2, Token::DoubleColon);
                }
            }
            b'+' => {
                if self.starts_with("+=") {
                    op!(2, Token::PlusEquals);
                }
                if self.starts_with("++") {
                    op!(2, Token::Increment);
                }
            }
            b'/' => {
                if self.starts_with("/=") {
                    op!(2, Token::SlashEquals);
                }
            }
            b'%' => {
                if self.starts_with("%=") {
                    op!(2, Token::PercentEquals);
                }
            }
            b'&' => {
                if self.starts_with("&=") {
                    op!(2, Token::AmpEquals);
                }
                if self.starts_with("&&") {
                    op!(2, Token::AmpAmp);
                }
            }
            b'|' => {
                if self.starts_with("|=") {
                    op!(2, Token::PipeEquals);
                }
                if self.starts_with("||") {
                    op!(2, Token::PipePipe);
                }
                if self.starts_with("|>") {
                    op!(2, Token::PipeGt);
                }
            }
            b'^' if self.starts_with("^=") => {
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
                let span = Span {
                    start: self.line_col_at(self.pos - 1),
                    end: self.line_col_at(self.pos),
                };
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
    c == b'_' || c.is_ascii_alphabetic() || c >= 0x80
}

fn is_ident_continue(c: u8) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
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

/// How many characters `bytes` hold: each invalid sequence one, as a
/// lossy decoding reads it.
fn char_count(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.chars().count(),
        Err(_) => String::from_utf8_lossy(bytes).chars().count(),
    }
}
