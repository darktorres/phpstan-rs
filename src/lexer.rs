use std::borrow::Cow;

use crate::intern::intern;
use crate::token::{Keyword, Position, Span, Token};

mod strings;
mod stub_attrs;
use stub_attrs::{
    append_param_type_override, append_return_type_override, skip_trailing_attribute_groups,
};

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
    // `line_col_at`'s incremental cache: the last byte offset it was
    // asked to convert, and the `Position` that came out. Needed because
    // a naive "recount every character from the line's own start"
    // implementation is O(line length) *per call* - fine for ordinary
    // code, but a real file (a generated Huffman lookup table) can have a
    // single line over a million characters long, and thousands of
    // tokens on that one line turns the naive approach into a real
    // O(line length x tokens on it) blowup. `tokenize_with_spans` calls
    // `line_col_at` with strictly increasing byte offsets (its whole
    // point is a linear scan forward), so caching the last conversion and
    // only counting the delta since then turns the pathological line back
    // into O(line length) total, not squared - and it's never wrong, only
    // sometimes not the fast path: a request for an earlier byte offset,
    // or one on a different line, still gets a fully correct (just not
    // delta-accelerated) recomputation.
    pos_cache: (usize, Position),
    // Set by `skip_language_level_type_aware_attribute` when it finds a
    // return-type-gating `#[LanguageLevelTypeAware(...)]` -
    // `tokenize_with_spans` checks this right after the `next_token()`
    // call that set it (before pushing that call's own token) and, if
    // set, rewrites the immediately-preceding `Token::DocComment` to
    // carry the modern type instead of the stub's stale one.
    pending_return_type_override: Option<String>,
    // The parameter-gating sibling of the field above - set when the
    // attribute instead gates a parameter (`finfo_file`'s
    // `#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')]
    // $finfo`). `tokenize_with_spans` inserts these as real synthetic
    // tokens right before the upcoming `$name` token, since a parameter
    // has no docblock tag guaranteed to exist for it the way a function's
    // one-per-signature `@return` line does.
    pending_param_type_tokens: Option<Vec<Token>>,
    // `pending_param_type_tokens` alone loses to a competing legacy
    // `@param` docblock tag (`resolve_type` treats a matching doc tag as
    // authoritative over the native hint) - real shape: `ini_set`'s
    // `@param string $value` next to a native
    // `#[LanguageLevelTypeAware(['8.1' => 'string|int|float|bool|null'],
    // default: 'string')] $value`. Set alongside
    // `pending_param_type_tokens` to (parameter name, override type);
    // `tokenize_with_spans` searches `tokens` backward for the nearest
    // enclosing `Token::DocComment` and splices a fresh `@param` line onto
    // it - mirroring `pending_return_type_override`'s own `@return`
    // splice, but needing its own field since a parameter override,
    // unlike a return override, isn't found immediately after the doc
    // comment that owns it (other parameters/attributes/commas sit in
    // between), so the backward search can only happen where `tokens`
    // itself lives - `tokenize_with_spans`'s own loop, not this lexer's
    // inner methods.
    pending_param_doc_override: Option<(String, String)>,
    // The just-lexed keyword's own original source-text spelling, stashed
    // out of band the same way the three `pending_*` fields above already
    // do for unrelated data - `Token::Keyword` itself carries only the
    // keyword's canonical (always-lowercase) spelling, never the raw
    // bytes, so a keyword-shaped identifier (`class Enum { ... }`, a real
    // vendored class literally named the same as the `enum` keyword)
    // would otherwise have its true declared-name case lost the moment
    // it's lexed. `tokenize_with_spans` reads this right after each
    // `next_token()` call, exactly like `last_token_start` above.
    pending_keyword_raw: Option<String>,
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

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        let bytes = src.as_bytes();
        Lexer {
            src: bytes,
            pos: 0,
            in_php: false,
            line_starts: compute_line_starts(bytes),
            last_token_start: 0,
            pos_cache: (0, Position { line: 1, col: 1 }),
            pending_return_type_override: None,
            pending_param_type_tokens: None,
            pending_param_doc_override: None,
            pending_keyword_raw: None,
        }
    }

    /// For re-lexing a fragment of PHP that is already known to be code,
    /// e.g. the inside of a `{$...}` string interpolation. Skips inline-HTML
    /// mode entirely.
    pub fn new_in_php(src: &'a str) -> Self {
        let bytes = src.as_bytes();
        Lexer {
            src: bytes,
            pos: 0,
            in_php: true,
            line_starts: compute_line_starts(bytes),
            last_token_start: 0,
            pos_cache: (0, Position { line: 1, col: 1 }),
            pending_return_type_override: None,
            pending_param_type_tokens: None,
            pending_param_doc_override: None,
            pending_keyword_raw: None,
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

    fn line_col_at(&mut self, byte_pos: usize) -> Position {
        let byte_pos = byte_pos.min(self.src.len());
        let (cached_pos, cached) = self.pos_cache;
        if byte_pos == cached_pos {
            return cached;
        }
        let line_idx = self.line_idx_at(byte_pos);
        // The fast path: still on the same line as the last conversion,
        // moving forward - only count the *new* characters since then,
        // not the whole line from its own start again. Falls back to a
        // full (still correct) recompute for a backward jump or a
        // different line - both real but comparatively rare (the
        // interpolation base-position bias below is the only caller that
        // can jump backward at all).
        let position = if byte_pos > cached_pos && line_idx == self.line_idx_at(cached_pos) {
            let delta = String::from_utf8_lossy(&self.src[cached_pos..byte_pos])
                .chars()
                .count();
            Position {
                line: (line_idx + 1) as u32,
                col: cached.col + delta as u32,
            }
        } else {
            let line_start = self.line_starts[line_idx];
            let col = String::from_utf8_lossy(&self.src[line_start..byte_pos])
                .chars()
                .count()
                + 1;
            Position {
                line: (line_idx + 1) as u32,
                col: col as u32,
            }
        };
        self.pos_cache = (byte_pos, position);
        position
    }

    /// The real, span-carrying tokenization entry point - `tokenize`
    /// below is a thin adapter over this for the handful of tests that
    /// want to compare bare tokens without spans getting in the way.
    #[hotpath::measure]
    pub fn tokenize_with_spans(mut self) -> Vec<(Token, Span, Option<String>)> {
        let mut tokens: Vec<(Token, Span, Option<String>)> = Vec::new();
        loop {
            let tok = self.next_token();
            let keyword_raw = self.pending_keyword_raw.take();
            if let Some(override_ty) = self.pending_return_type_override.take() {
                // A plain `tokens.last_mut()` check would assume the doc
                // comment is always the immediately-preceding token - not
                // true once a generic `#[Attr]` group is a real,
                // individually-tokenized token: phpstorm-stubs' own
                // `substr()` stub is `/** @return string|false */ #[Pure]
                // #[LanguageLevelTypeAware(...)] function substr(...) {}`
                // - `#[Pure]` sits directly between the doc comment and
                // the special-cased attribute that actually sets
                // `pending_return_type_override`, hiding the doc comment
                // from a bare `tokens.last_mut()` look and silently
                // dropping the override. Skipping back past any such
                // trailing attribute group(s) first restores the intended
                // adjacency.
                let end = skip_trailing_attribute_groups(&tokens);
                if let Some((Token::DocComment(doc), _, _)) =
                    end.checked_sub(1).and_then(|i| tokens.get_mut(i))
                {
                    *doc = append_return_type_override(doc, &override_ty).into();
                }
            }
            if let Some(type_tokens) = self.pending_param_type_tokens.take() {
                // Synthetic tokens with no real source text of their own -
                // a zero-width span at the upcoming real token's own
                // start (computed the same way just below) is the most
                // sensible position to attribute them to.
                let synthetic_pos = self.line_col_at(self.last_token_start);
                let synthetic_span = Span {
                    start: synthetic_pos,
                    end: synthetic_pos,
                };
                for t in type_tokens {
                    tokens.push((t, synthetic_span, None));
                }
            }
            if let Some((name, override_ty)) = self.pending_param_doc_override.take() {
                // Unlike a return override (found immediately after the
                // doc comment that owns it, via `tokens.last_mut()`
                // above), a parameter override can be many tokens past its
                // own enclosing doc comment (other parameters, attributes,
                // commas) - so this searches backward through everything
                // `tokens` holds so far, stopping (doing nothing) the
                // moment a statement/declaration boundary token is crossed,
                // to avoid misattributing the override to an unrelated
                // *earlier* declaration's docblock when the current
                // function has none of its own.
                for (t, _, _) in tokens.iter_mut().rev() {
                    match t {
                        Token::DocComment(doc) => {
                            *doc = append_param_type_override(doc, &name, &override_ty).into();
                            break;
                        }
                        Token::Semicolon | Token::LBrace | Token::RBrace => break,
                        _ => {}
                    }
                }
            }
            let span = Span {
                start: self.line_col_at(self.last_token_start),
                end: self.line_col_at(self.pos),
            };
            let done = tok == Token::Eof;
            tokens.push((tok, span, keyword_raw));
            if done {
                break;
            }
        }
        tokens
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
            return Token::Variable(intern(&s));
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
            return Token::Identifier(intern(&s));
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
        let start = self.pos;
        while self.pos < self.src.len() && !self.starts_with("<?php") && !self.starts_with("<?=") {
            self.pos += 1;
        }
        let html = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();

        if self.starts_with("<?php") {
            self.pos += 5;
            self.in_php = true;
        } else if self.starts_with("<?=") {
            self.pos += 3;
            self.in_php = true;
            if !html.is_empty() {
                return Token::InlineHtml(intern(&html));
            }
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
        Token::InlineHtml(intern(&html))
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
                    if self.starts_with("#[PhpStormStubsElementAvailable(") {
                        self.skip_stub_availability_attribute();
                    } else if self.starts_with("#[LanguageLevelTypeAware(") {
                        self.skip_language_level_type_aware_attribute("#[LanguageLevelTypeAware(");
                    } else if self.starts_with("#[PhpVersionAware(") {
                        // Real phpstorm-stubs shape (`ldap/ldap.php`'s own
                        // `use JetBrains\PhpStorm\Internal\
                        // LanguageLevelTypeAware as PhpVersionAware;`) -
                        // `PhpVersionAware` is a plain PHP `use`-import
                        // alias for the identical attribute class handled
                        // above, not a different attribute - every
                        // `ldap_*` function stub in that one file uses
                        // this spelling exclusively, so without this arm
                        // every one fell through to the generic `#[`
                        // attribute-token path below, leaving its param/
                        // return type stuck on the stale pre-8.1 `@param
                        // resource $ldap`-style doc comment instead of the
                        // real `\LDAP\Connection`/`\LDAP\Result` override
                        // this attribute exists to apply.
                        self.skip_language_level_type_aware_attribute("#[PhpVersionAware(");
                    } else if self.starts_with("#[") {
                        // A general `#[Attribute(...)]` group isn't
                        // thrown away here - only the two special-cased
                        // stub-generation attributes above get raw-text
                        // treatment. Breaking out of this whitespace/
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
                        while self.pos < self.src.len()
                            && self.peek() != b'\n'
                            && !self.starts_with("?>")
                        {
                            self.pos += 1;
                        }
                    }
                }
                b'/' if self.peek_at(1) == b'/' => {
                    while self.pos < self.src.len()
                        && self.peek() != b'\n'
                        && !self.starts_with("?>")
                    {
                        self.pos += 1;
                    }
                }
                // `/**/` (empty) is conventionally just a regular comment,
                // not a doc comment - PHP's own tokenizer draws the line the
                // same way. Check that case first since it shares its
                // middle `*` between the `/**`-looking open and the `*/`
                // close.
                b'/' if self.starts_with("/**") && self.peek_at(3) != b'/' => break,
                b'/' if self.peek_at(1) == b'*' => {
                    self.pos += 2;
                    while self.pos < self.src.len() && !self.starts_with("*/") {
                        self.pos += 1;
                    }
                    self.pos = (self.pos + 2).min(self.src.len());
                }
                _ => break,
            }
        }
    }

    /// A numeric literal's own digit-run, with PHP's `_` digit separator
    /// (`1_000_000`) stripped - `raw` is always plain ASCII (digits/hex-
    /// digits/`e`/`E`/`+`/`-`/`.`, the only bytes `lex_number`'s own scan
    /// loops ever advance `self.pos` over), so a direct `str::from_utf8`
    /// borrow (no allocation at all) is sound whenever there's nothing to
    /// strip - the overwhelmingly common case, real digit separators being
    /// rare in most real-world source. Only allocates (via `.replace`) for
    /// the literals that actually use one. Found and fixed via a perf-
    /// optimization investigation workflow.
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

    fn lex_ident_raw(&mut self) -> String {
        let span = self.lex_ident_span();
        String::from_utf8_lossy(&self.src[span]).into_owned()
    }

    fn lex_number(&mut self) -> Token {
        let start = self.pos;
        let mut is_float = false;

        if self.peek() == b'0' && (self.peek_at(1) == b'x' || self.peek_at(1) == b'X') {
            self.pos += 2;
            while self.peek().is_ascii_hexdigit() || self.peek() == b'_' {
                self.pos += 1;
            }
            let text = Self::strip_digit_separators(&self.src[start + 2..self.pos]);
            let value = i64::from_str_radix(&text, 16).unwrap_or(0);
            return Token::Integer(value);
        }

        if self.peek() == b'0' && (self.peek_at(1) == b'b' || self.peek_at(1) == b'B') {
            self.pos += 2;
            while self.peek() == b'0' || self.peek() == b'1' || self.peek() == b'_' {
                self.pos += 1;
            }
            let text = Self::strip_digit_separators(&self.src[start + 2..self.pos]);
            let value = i64::from_str_radix(&text, 2).unwrap_or(0);
            return Token::Integer(value);
        }

        // PHP 8.1's explicit octal notation (`0o775`), distinct from the
        // legacy leading-zero form (`0775`) the generic digit loop below
        // already parses (as decimal, which happens to not crash even
        // though the *value* comes out wrong for a real octal literal -
        // not worth a special case since this prototype cares about types,
        // not runtime values).
        if self.peek() == b'0' && (self.peek_at(1) == b'o' || self.peek_at(1) == b'O') {
            self.pos += 2;
            while (b'0'..=b'7').contains(&self.peek()) || self.peek() == b'_' {
                self.pos += 1;
            }
            let text = Self::strip_digit_separators(&self.src[start + 2..self.pos]);
            let value = i64::from_str_radix(&text, 8).unwrap_or(0);
            return Token::Integer(value);
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
            while self.peek().is_ascii_digit() {
                self.pos += 1;
            }
        }

        let text = Self::strip_digit_separators(&self.src[start..self.pos]);
        if is_float {
            Token::Float(text.parse().unwrap_or(0.0))
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
        Token::DocComment(intern(&String::from_utf8_lossy(&self.src[start..self.pos])))
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
                eprintln!(
                    "warning: unexpected byte 0x{:02x} in PHP source, skipping",
                    other
                );
                self.next_token()
            }
        }
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
