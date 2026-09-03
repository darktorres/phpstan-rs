use crate::token::{Keyword, Position, Span, StringPart, Token};

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
    // asked to convert, and the `Position` that came out. Real, not
    // theoretical, motivation: a naive "recount every character from the
    // line's own start" implementation is O(line length) *per call* -
    // fine for ordinary code, but a real file in this project's own
    // Piwigo/vendor test corpus (`amphp/hpack`'s generated Huffman lookup
    // table) has a single line 1.25 million characters long. With
    // thousands of tokens on that one line, the naive approach is a real
    // O(line length x tokens on it) blowup - measured hanging for
    // minutes on a file that used to check in well under a second.
    // `tokenize_with_spans` calls `line_col_at` with strictly increasing
    // byte offsets (its whole point is a linear scan forward), so caching
    // the last conversion and only counting the *delta* since then turns
    // the pathological line back into O(line length) total, not squared -
    // and it's never wrong, only sometimes not the fast path: a request
    // for an earlier byte offset, or one on a different line, still gets
    // a fully correct (just not delta-accelerated) recomputation.
    pos_cache: (usize, Position),
    // Phase 24: set by `skip_language_level_type_aware_attribute` when it
    // finds a return-type-gating `#[LanguageLevelTypeAware(...)]` -
    // `tokenize_with_spans` checks this right after the `next_token()`
    // call that set it (before pushing that call's own token) and, if
    // set, rewrites the immediately-preceding `Token::DocComment` to
    // carry the modern type instead of the stub's stale one.
    pending_return_type_override: Option<String>,
    // Phase 24: the parameter-gating sibling of the field above - set
    // when the attribute instead gates a parameter (`finfo_file`'s
    // `#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')]
    // $finfo`). `tokenize_with_spans` inserts these as real synthetic
    // tokens right before the upcoming `$name` token, since a parameter
    // has no docblock tag guaranteed to exist for it the way a function's
    // one-per-signature `@return` line does.
    pending_param_type_tokens: Option<Vec<Token>>,
    // Phase 32 Finding B: `pending_param_type_tokens` alone loses to a
    // competing legacy `@param` docblock tag (`resolve_type` treats a
    // matching doc tag as authoritative over the native hint) - real
    // shape: `ini_set`'s `@param string $value` next to a native
    // `#[LanguageLevelTypeAware(['8.1' => 'string|int|float|bool|null'],
    // default: 'string')] $value`. Set alongside
    // `pending_param_type_tokens` to (parameter name, override type);
    // `tokenize_with_spans` searches `tokens` backward for the nearest
    // enclosing `Token::DocComment` and splices a fresh `@param` line onto
    // it - mirroring `pending_return_type_override`'s own `@return`
    // splice, but needing its own field since a parameter override, unlike
    // a return override, isn't found immediately after the doc comment
    // that owns it (other parameters/attributes/commas sit in between),
    // so the backward search can only happen where `tokens` itself lives -
    // `tokenize_with_spans`'s own loop, not this lexer's inner methods.
    pending_param_doc_override: Option<(String, String)>,
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

/// Finds the next single- or double-quoted string literal in `s` (quote-
/// agnostic - the real stub corpus mixes `'...'` and `"..."` freely for
/// this attribute), returning its unquoted content and the remainder of
/// `s` right after the closing quote. No escape handling - every type
/// string this attribute carries is a plain identifier/union, never
/// containing a quote of its own.
fn take_quoted_string(s: &str) -> Option<(&str, &str)> {
    let start = s.find(['\'', '"'])?;
    let quote = s.as_bytes()[start] as char;
    let after_open = &s[start + quote.len_utf8()..];
    let end = after_open.find(quote)?;
    Some((&after_open[..end], &after_open[end + quote.len_utf8()..]))
}

/// Parses a `LanguageLevelTypeAware` version key (`"8.1"`) into a
/// `(major, minor)` pair for numeric comparison - a plain string
/// comparison would wrongly rank `"8.10"` below `"8.9"`, and while no
/// version this high exists yet, comparing as numbers costs nothing extra.
fn parse_version(s: &str) -> Option<(u32, u32)> {
    let (major, minor) = s.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Extracts the highest-versioned type string from a captured
/// `LanguageLevelTypeAware(['V.V' => 'TYPE', ...], default: 'TYPE')`
/// attribute's raw text (including its own `#[...` wrapper - callers
/// don't need to trim it first, only `default:`'s presence and the
/// quoted pairs before it matter). This project's own established
/// "assume the modern/open-ended interpretation" precedent (already used
/// for `#[PhpStormStubsElementAvailable]`'s closed-range parameter
/// dropping) - real code being checked is running some real, current PHP
/// version, so the highest-versioned declared type is the one actually
/// in effect. A version key that fails to parse as `major.minor` is
/// skipped, not fatal to the rest of the scan. Falls back to `default:`'s
/// own value only when no version pair parsed at all (shouldn't happen -
/// every real occurrence has at least one - but `default:` is always
/// present regardless, confirmed against the full vendored corpus).
fn extract_modern_type(raw: &str) -> Option<String> {
    let (array_part, default_part) = match raw.find("default:") {
        Some(idx) => (&raw[..idx], Some(&raw[idx + "default:".len()..])),
        None => (raw, None),
    };
    let mut best: Option<((u32, u32), String)> = None;
    let mut rest = array_part;
    while let Some((key, after_key)) = take_quoted_string(rest) {
        let Some(after_arrow) = after_key.trim_start().strip_prefix("=>") else {
            break;
        };
        let Some((ty, tail)) = take_quoted_string(after_arrow) else {
            break;
        };
        if let Some(version) = parse_version(key)
            && best.as_ref().is_none_or(|(v, _)| version > *v)
        {
            best = Some((version, ty.to_string()));
        }
        rest = tail;
    }
    best.map(|(_, ty)| ty).or_else(|| {
        default_part
            .and_then(take_quoted_string)
            .map(|(ty, _)| ty.to_string())
    })
}

/// Splices a fresh `@return <override>` line into `doc` right before its
/// closing `*/` - `docblock::parse` processes `@return` tags in source
/// order with no early exit, so a later one simply overwrites an earlier
/// one's value in the returned `DocBlock`. Appending (rather than
/// surgically replacing the original `@return` line's own type
/// substring) sidesteps needing to reimplement `take_type_token`'s own
/// type-vs-description boundary logic here.
fn append_return_type_override(doc: &str, override_ty: &str) -> String {
    match doc.rfind("*/") {
        Some(idx) => format!(
            "{}\n * @return {override_ty}\n {}",
            &doc[..idx],
            &doc[idx..]
        ),
        None => doc.to_string(),
    }
}

/// Phase 32 Finding B: the parameter-position sibling of
/// `append_return_type_override` - splices a fresh `@param <override>
/// $<name>` line into `doc` right before its closing `*/`, so
/// `resolve_type`'s own "docblock wins" priority (each `@param` tag
/// matched by name, last one wins - see `docblock::parse`'s
/// `HashMap::insert`) makes the modernization override win over a stale
/// legacy `@param` tag for the same parameter, without touching
/// `resolve_type`'s general priority rule at all.
fn append_param_type_override(doc: &str, name: &str, override_ty: &str) -> String {
    match doc.rfind("*/") {
        Some(idx) => format!(
            "{}\n * @param {override_ty} ${name}\n {}",
            &doc[..idx],
            &doc[idx..]
        ),
        None => doc.to_string(),
    }
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
        }
    }

    /// Converts an absolute byte offset into a 1-indexed line/column -
    /// `col` counts UTF-8 *characters* from the line's own start, not raw
    /// bytes (a real, demonstrated need: real PHP source, including this
    /// project's own real-corpus test fixtures, has multi-byte UTF-8
    /// inside actual code, not just comments - a byte-offset column would
    /// silently overshoot the true character position on every such
    /// line). The extra decoding cost is bounded to one line's own
    /// prefix, not the whole file.
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
    pub fn tokenize_with_spans(mut self) -> Vec<(Token, Span)> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            if let Some(override_ty) = self.pending_return_type_override.take()
                && let Some((Token::DocComment(doc), _)) = tokens.last_mut()
            {
                *doc = append_return_type_override(doc, &override_ty);
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
                    tokens.push((t, synthetic_span));
                }
            }
            if let Some((name, override_ty)) = self.pending_param_doc_override.take() {
                // Finding B: unlike a return override (found immediately
                // after the doc comment that owns it, via `tokens.last_mut()`
                // above), a parameter override can be many tokens past its
                // own enclosing doc comment (other parameters, attributes,
                // commas) - so this searches backward through everything
                // `tokens` holds so far, stopping (doing nothing) the
                // moment a statement/declaration boundary token is crossed,
                // to avoid misattributing the override to an unrelated
                // *earlier* declaration's docblock when the current
                // function has none of its own.
                for (t, _) in tokens.iter_mut().rev() {
                    match t {
                        Token::DocComment(doc) => {
                            *doc = append_param_type_override(doc, &name, &override_ty);
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
            tokens.push((tok, span));
            if done {
                break;
            }
        }
        tokens
    }

    pub fn tokenize(self) -> Vec<Token> {
        self.tokenize_with_spans()
            .into_iter()
            .map(|(t, _)| t)
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

        let c = self.peek();

        if c == b'$' && (is_ident_start(self.peek_at(1))) {
            self.pos += 1;
            let s = self.lex_ident_raw();
            return Token::Variable(s);
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
            let s = self.lex_ident_raw();
            if let Some(kw) = Keyword::try_from_str(&s) {
                return Token::Keyword(kw);
            }
            return Token::Identifier(s);
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
                return Token::InlineHtml(html);
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
        Token::InlineHtml(html)
    }

    /// Advances past a single- or double-quoted string literal starting
    /// at the current position (which must be exactly on the opening
    /// quote byte), honoring escaping just enough to find the real
    /// closing quote - a raw byte-level bracket/brace-depth counter that
    /// doesn't call this before resuming its own count can be fooled by
    /// an unbalanced `[`/`]`/`{`/`}` inside the string's own content
    /// (e.g. a regex character class like `'/[^\]]+/'`). Shared by both
    /// of this lexer's own raw depth-counting scanners that need it
    /// (the `#[Attribute(...)]` skip, and `{$...}` complex-interpolation
    /// scanning) rather than duplicating the same fix independently in
    /// each.
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
                        self.skip_language_level_type_aware_attribute();
                    } else if self.starts_with("#[") {
                        // Attribute like #[Foo(...)] - skip to matching
                        // ']'. String-aware: an argument string
                        // containing an unbalanced `[`/`]` (a completely
                        // ordinary thing to write, e.g. a regex
                        // character class like `'/[^\]]+/'`) must not
                        // throw off the depth count.
                        self.pos += 2;
                        let mut depth = 1;
                        while self.pos < self.src.len() && depth > 0 {
                            match self.peek() {
                                b'\'' | b'"' => self.skip_string_literal(),
                                b'[' => {
                                    depth += 1;
                                    self.pos += 1;
                                }
                                b']' => {
                                    depth -= 1;
                                    self.pos += 1;
                                }
                                _ => self.pos += 1,
                            }
                        }
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

    /// phpstorm-stubs represents "this parameter only existed in PHP
    /// versions X-Y" with `#[PhpStormStubsElementAvailable(from: 'X', to:
    /// 'Y')]` on the parameter itself, rather than maintaining separate
    /// per-version signatures - e.g. `array_map`'s real stub carries a
    /// modern, still-open `array $array` parameter *and* a superseded,
    /// closed-range, no-default `$arrays` one, side by side with the
    /// modern variadic `array ...$arrays`, both meant to represent the
    /// same conceptual parameter across two PHP-version eras, never both
    /// present on one real install. A plain attribute skip (this project's
    /// treatment of every other attribute, none of which carry
    /// information any checker feature here consumes) would leave that
    /// superseded parameter in the token stream as an ordinary,
    /// permanently-required one, miscounting the function's real minimum
    /// arity - and not just for this project's own bundled stub corpus:
    /// real projects commonly vendor `jetbrains/phpstorm-stubs` itself as
    /// a transitive phpstan dependency, so this needs to be a lexer-level
    /// fix, not something special-cased only in the stub-generation
    /// pipeline.
    ///
    /// Only a *closed* range (`to:` present) parameter with no `= value`
    /// default of its own gets dropped - along with, if present, its one
    /// trailing separator comma (a trailing comma before `)` is valid PHP
    /// syntax the parser already tolerates, so nothing needs repairing on
    /// the other side). An open-ended range (`from` only, still valid in
    /// the latest PHP) is a real, current parameter and is left alone, as
    /// is one that already has a real default - both fall back to an
    /// ordinary attribute-only skip. Whether this attribute gates a
    /// single parameter or a *whole* function/method declaration (real
    /// shape: `DateTimeImmutable::__construct`'s `to: '8.2'`/`from:
    /// '8.3'` pair) is checked before attempting the parameter-fragment
    /// scan at all, via `upcoming_is_function_declaration` - see
    /// `skip_whole_versioned_declaration`'s own doc comment for that
    /// case (Round 15 of the Piwigo corpus cleanup - previously left
    /// completely untouched, wrongly letting a superseded declaration's
    /// return type merge into the modern one's).
    fn skip_stub_availability_attribute(&mut self) {
        self.pos += "#[PhpStormStubsElementAvailable(".len();
        let attr_args_start = self.pos;
        let mut depth = 1i32;
        while depth > 0 && self.pos < self.src.len() {
            match self.peek() {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            self.pos += 1;
        }
        if depth != 0 {
            // Malformed/unexpected shape (EOF before the attribute
            // closed) - bail out with whatever's been consumed so far.
            return;
        }
        let is_closed_range = std::str::from_utf8(&self.src[attr_args_start..self.pos - 1])
            .is_ok_and(|s| s.contains("to:"));
        if self.peek() == b']' {
            self.pos += 1;
        }
        if !is_closed_range {
            return;
        }
        // Round 15 of the Piwigo corpus cleanup: this same attribute also
        // gates a *whole* function/method declaration (real shape:
        // `version_compare()`'s own two full declarations, one
        // `to: '7.4'` returning `int|bool|null`, one `from: '8.0'`
        // returning `int|bool`; `DateTimeImmutable::__construct`'s
        // `to: '8.2'`/`from: '8.3'` pair) - previously left completely
        // untouched (this function's own doc comment used to document
        // that as deliberate), so both declarations reached the parser
        // as ordinary, unconditional signatures and got unioned together
        // by the registry's own "second unconditional declaration merges
        // onto the first" real-overload handling, wrongly reintroducing
        // the pre-8.0 `null` return into a target this project otherwise
        // always treats as PHP 8.0+. A closed range (`to:` - superseded
        // on the modern PHP version this project models) gating a whole
        // declaration should leave no trace at all, the same principle
        // already applied to a closed-range *parameter* below.
        if self.upcoming_is_function_declaration() {
            self.skip_whole_versioned_declaration();
            return;
        }

        let param_start = self.pos;
        let mut pdepth = 0i32;
        let mut has_default = false;
        loop {
            if self.pos >= self.src.len() {
                // Reached EOF mid-parameter - leave as skipped so far.
                return;
            }
            match self.peek() {
                b'(' | b'[' => pdepth += 1,
                b')' | b']' if pdepth == 0 => break,
                b')' | b']' => pdepth -= 1,
                b',' if pdepth == 0 => break,
                b'{' | b';' if pdepth == 0 => {
                    // Not a simple parameter fragment - restore and leave
                    // the attribute-only skip already applied above.
                    self.pos = param_start;
                    return;
                }
                b'=' if pdepth == 0 => has_default = true,
                _ => {}
            }
            self.pos += 1;
        }
        if has_default {
            // Already counted correctly regardless of its version range.
            self.pos = param_start;
            return;
        }
        // Drop the parameter too, and its one trailing comma if present
        // (a closing delimiter is left for the real tokenizer to see).
        if self.peek() == b',' {
            self.pos += 1;
        }
    }

    /// Whether the attribute just consumed by `skip_stub_availability_attribute`
    /// gates a whole function/method declaration rather than a single
    /// parameter - real PHP only allows `public`/`private`/`protected`/
    /// `static`/`abstract`/`final` modifiers (in any combination) before
    /// the `function` keyword itself at this position; a parameter
    /// fragment can never start with any of them.
    fn upcoming_is_function_declaration(&self) -> bool {
        let mut p = self.pos;
        loop {
            while p < self.src.len() && (self.src[p] as char).is_ascii_whitespace() {
                p += 1;
            }
            let word_start = p;
            while p < self.src.len() && (self.src[p] as char).is_ascii_alphabetic() {
                p += 1;
            }
            if p == word_start {
                return false;
            }
            match std::str::from_utf8(&self.src[word_start..p]) {
                Ok("public" | "private" | "protected" | "static" | "abstract" | "final") => {}
                Ok("function") => return true,
                _ => return false,
            }
        }
    }

    /// Skips an entire closed-range-superseded function/method
    /// declaration (modifiers, `function`, name, parameter list, return
    /// type) up through its own terminator - either a bare `;` (an
    /// abstract/interface method, no body) or a balanced `{...}` body.
    /// `;` can never appear inside the parameter list itself (not even
    /// in a default value - PHP has no expression form that contains a
    /// bare `;`), so a simple paren-depth counter is enough to tell the
    /// declaration's own terminator apart from one inside the parameter
    /// list's parens.
    fn skip_whole_versioned_declaration(&mut self) {
        let mut paren_depth = 0i32;
        loop {
            if self.pos >= self.src.len() {
                return;
            }
            match self.peek() {
                b'\'' | b'"' => self.skip_string_literal(),
                b'(' => {
                    paren_depth += 1;
                    self.pos += 1;
                }
                b')' => {
                    paren_depth -= 1;
                    self.pos += 1;
                }
                b';' if paren_depth <= 0 => {
                    self.pos += 1;
                    return;
                }
                b'{' if paren_depth <= 0 => {
                    self.skip_balanced_braces();
                    return;
                }
                _ => self.pos += 1,
            }
        }
    }

    /// String-aware skip from an opening `{` (the current byte) through
    /// its matching `}`.
    fn skip_balanced_braces(&mut self) {
        self.pos += 1;
        let mut depth = 1i32;
        while depth > 0 && self.pos < self.src.len() {
            match self.peek() {
                b'\'' | b'"' => self.skip_string_literal(),
                b'{' => {
                    depth += 1;
                    self.pos += 1;
                }
                b'}' => {
                    depth -= 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
    }

    /// phpstorm-stubs represents "this function/param's type differs by
    /// PHP version" with `#[LanguageLevelTypeAware(['V.V' => 'MODERN',
    /// ...], default: 'LEGACY')]` - e.g. `substr()`'s real stub has *no*
    /// native return-type hint at all, so its only source of a return
    /// type is its own `@return string|false` doc comment, a pre-8.0
    /// legacy value this attribute exists specifically to override for a
    /// modern PHP install; `finfo_file`'s real stub gates its own first
    /// *parameter* the identical way (`#[LanguageLevelTypeAware(['8.1' =>
    /// 'finfo'], default: 'resource')] $finfo`, no separate native hint
    /// at all either). Captures the attribute's own argument text (via
    /// the same paren-depth, string-aware skip every other call site here
    /// already uses), extracts the highest-versioned type via
    /// `extract_modern_type`, and dispatches on which of the two shapes
    /// this is (`next_parameter_anchor` - an immediately-following `$`/
    /// `&`/`...` means a parameter, anything else means a return type):
    /// a parameter override becomes real, synthetic type-hint tokens
    /// queued in `pending_param_type_tokens` (inserted right before the
    /// upcoming `$name` token - no docblock involved, since a parameter's
    /// own `@param $name` tag may not exist at all for every gated
    /// parameter); a return override is stashed in
    /// `pending_return_type_override` for `tokenize_with_spans` to splice
    /// into the preceding doc comment instead.
    fn skip_language_level_type_aware_attribute(&mut self) {
        let attr_start = self.pos;
        self.pos += "#[LanguageLevelTypeAware(".len();
        let mut depth = 1i32;
        while depth > 0 && self.pos < self.src.len() {
            match self.peek() {
                b'\'' | b'"' => self.skip_string_literal(),
                b'(' => {
                    depth += 1;
                    self.pos += 1;
                }
                b')' => {
                    depth -= 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
        if depth != 0 {
            // Malformed/unexpected shape (EOF before the attribute
            // closed) - bail out with whatever's been consumed so far.
            return;
        }
        let raw = String::from_utf8_lossy(&self.src[attr_start..self.pos]).into_owned();
        if self.peek() == b']' {
            self.pos += 1;
        }
        let Some(override_ty) = extract_modern_type(&raw) else {
            return;
        };
        if self.next_parameter_anchor() {
            // Unlike a return type (routed through the docblock, parsed
            // by `types::parse_type_string` which understands PHPDoc-only
            // syntax like `TYPE[]`), an injected parameter override
            // becomes real *native* type-hint tokens, consumed by the
            // parser's native `parse_type_hint` - which has no notion of
            // `[]` shorthand at all. Real corpus proof this isn't
            // hypothetical: `strip_tags()`'s own `#[LanguageLevelTypeAware(
            // ["7.4" => "string[]|string|null"], ...)]` parameter crashed
            // the parser outright before this guard existed. None of this
            // pass's diagnosed parameter cases (`finfo`, `GdFont|int`,
            // `PgSql\Connection`) use `[]` shorthand, so skipping this
            // shape entirely - falling back to today's existing "leave it
            // alone" behavior - costs nothing evidenced, and is far safer
            // than guessing at a native-hint-compatible rewrite.
            if !override_ty.contains(['[', ']']) {
                let tokens: Vec<Token> = Lexer::new_in_php(&override_ty)
                    .tokenize()
                    .into_iter()
                    .filter(|t| *t != Token::Eof)
                    .collect();
                if !tokens.is_empty() {
                    self.pending_param_type_tokens = Some(tokens.clone());
                }
                // Finding B: also queue a doc-splice fallback, keyed by the
                // parameter's own name - a no-op if there's no enclosing
                // doc comment to find, but the only way to beat a
                // competing legacy `@param` tag for this same name when
                // one does exist (see this field's own doc comment).
                if !tokens.is_empty()
                    && let Some(name) = self.peek_param_name()
                {
                    self.pending_param_doc_override = Some((name, override_ty));
                }
            }
        } else {
            self.pending_return_type_override = Some(override_ty);
        }
    }

    /// Peeks (without consuming) the parameter name that
    /// `next_parameter_anchor` already confirmed follows - skips leading
    /// whitespace, an optional `&` (by-ref) or `...` (variadic) marker,
    /// then the `$` and its identifier. `None` for anything other than
    /// exactly this shape - conservative, matching every other bail-out in
    /// this file rather than guessing.
    fn peek_param_name(&self) -> Option<String> {
        let mut p = self.pos;
        while matches!(self.src.get(p), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            p += 1;
        }
        if self.src[p..].starts_with(b"...") {
            p += 3;
        } else if self.src.get(p) == Some(&b'&') {
            p += 1;
        }
        while matches!(self.src.get(p), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            p += 1;
        }
        if self.src.get(p) != Some(&b'$') {
            return None;
        }
        p += 1;
        let start = p;
        while self.src.get(p).is_some_and(|b| is_ident_continue(*b)) {
            p += 1;
        }
        if p == start {
            return None;
        }
        Some(String::from_utf8_lossy(&self.src[start..p]).into_owned())
    }

    /// Whether the next real token (skipping only plain ASCII whitespace -
    /// deliberately not a full trivia skip, since this only needs to
    /// distinguish "a parameter follows" from everything else) starts a
    /// parameter - a bare `$name`, a by-ref `&$name`, or a variadic
    /// `...$name` - as opposed to a return-type-gating position (a
    /// visibility keyword, `function`, or another attribute).
    fn next_parameter_anchor(&self) -> bool {
        let mut p = self.pos;
        while matches!(self.src.get(p), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            p += 1;
        }
        matches!(self.src.get(p), Some(b'$' | b'&')) || self.src[p..].starts_with(b"...")
    }

    fn lex_ident_raw(&mut self) -> String {
        let start = self.pos;
        while is_ident_continue(self.peek()) {
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    fn lex_number(&mut self) -> Token {
        let start = self.pos;
        let mut is_float = false;

        if self.peek() == b'0' && (self.peek_at(1) == b'x' || self.peek_at(1) == b'X') {
            self.pos += 2;
            while self.peek().is_ascii_hexdigit() || self.peek() == b'_' {
                self.pos += 1;
            }
            let text: String =
                String::from_utf8_lossy(&self.src[start + 2..self.pos]).replace('_', "");
            let value = i64::from_str_radix(&text, 16).unwrap_or(0);
            return Token::Integer(value);
        }

        if self.peek() == b'0' && (self.peek_at(1) == b'b' || self.peek_at(1) == b'B') {
            self.pos += 2;
            while self.peek() == b'0' || self.peek() == b'1' || self.peek() == b'_' {
                self.pos += 1;
            }
            let text: String =
                String::from_utf8_lossy(&self.src[start + 2..self.pos]).replace('_', "");
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
            let text: String =
                String::from_utf8_lossy(&self.src[start + 2..self.pos]).replace('_', "");
            let value = i64::from_str_radix(&text, 8).unwrap_or(0);
            return Token::Integer(value);
        }

        while self.peek().is_ascii_digit() || self.peek() == b'_' {
            self.pos += 1;
        }
        // A `.` right after integer digits always starts a float, even
        // with no fractional digits following (`123.` is a valid float
        // literal, equal to `123.0`) - verified against a real PHP 8.5
        // build: `1.$b` is a real syntax error there too (the `.` is
        // greedily consumed into the number, not treated as concat),
        // confirming this isn't conditional on what follows the dot.
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

        let text: String = String::from_utf8_lossy(&self.src[start..self.pos]).replace('_', "");
        if is_float {
            Token::Float(text.parse().unwrap_or(0.0))
        } else {
            match text.parse::<i64>() {
                Ok(v) => Token::Integer(v),
                Err(_) => Token::Float(text.parse().unwrap_or(0.0)),
            }
        }
    }

    fn lex_single_quoted(&mut self) -> Token {
        self.pos += 1; // opening '
        let mut s = String::new();
        while self.pos < self.src.len() && self.peek() != b'\'' {
            if self.peek() == b'\\' && (self.peek_at(1) == b'\'' || self.peek_at(1) == b'\\') {
                s.push(self.peek_at(1) as char);
                self.pos += 2;
            } else {
                s.push(self.bump() as char);
            }
        }
        self.pos += 1; // closing '
        Token::StringLit(s)
    }

    fn lex_double_quoted(&mut self) -> Token {
        Token::InterpString(self.lex_interpolated_run(b'"'))
    }

    fn lex_backtick(&mut self) -> Token {
        Token::ShellExec(self.lex_interpolated_run(b'`'))
    }

    /// Shared scanning logic for double-quoted strings and backtick
    /// shell-exec strings - identical interpolation rules, just a
    /// different closing delimiter.
    fn lex_interpolated_run(&mut self, delim: u8) -> Vec<StringPart> {
        self.pos += 1; // opening delimiter
        let start = self.pos;
        while self.pos < self.src.len() && self.peek() != delim {
            if self.peek() == b'\\' {
                self.pos = (self.pos + 2).min(self.src.len());
            } else if self.peek() == b'{' && self.peek_at(1) == b'$' {
                // A `{$...}` complex-interpolation block can contain its
                // own delimiter characters (`{$data["key"]}`), which must
                // not be mistaken for this string's own closing delimiter -
                // skip the whole balanced-brace block before resuming the
                // search for the real closing delimiter.
                self.pos += 1;
                let mut depth = 1;
                while self.pos < self.src.len() && depth > 0 {
                    match self.peek() {
                        // String-aware: a nested string literal inside
                        // the expression (`{$f->wrap('}')}`) can contain
                        // a literal, unbalanced `{`/`}` of its own, which
                        // must not be mistaken for this block's own
                        // closing brace.
                        b'\'' | b'"' => self.skip_string_literal(),
                        b'{' => {
                            depth += 1;
                            self.pos += 1;
                        }
                        b'}' => {
                            depth -= 1;
                            self.pos += 1;
                        }
                        _ => self.pos += 1,
                    }
                }
            } else {
                self.pos += 1;
            }
        }
        let raw = &self.src[start..self.pos];
        let parts = self.scan_interpolated(raw, start);
        self.pos = (self.pos + 1).min(self.src.len()); // closing delimiter
        parts
    }

    fn lex_doc_comment(&mut self) -> Token {
        let start = self.pos;
        self.pos += 3; // consume '/**'
        while self.pos < self.src.len() && !self.starts_with("*/") {
            self.pos += 1;
        }
        self.pos = (self.pos + 2).min(self.src.len());
        Token::DocComment(String::from_utf8_lossy(&self.src[start..self.pos]).into_owned())
    }

    fn lex_heredoc(&mut self) -> Token {
        self.pos += 3; // consume '<<<'
        while matches!(self.peek(), b' ' | b'\t') {
            self.pos += 1;
        }

        let nowdoc;
        let label;
        match self.peek() {
            b'\'' => {
                self.pos += 1;
                let start = self.pos;
                while self.pos < self.src.len() && self.peek() != b'\'' {
                    self.pos += 1;
                }
                label = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
                self.pos = (self.pos + 1).min(self.src.len());
                nowdoc = true;
            }
            b'"' => {
                self.pos += 1;
                let start = self.pos;
                while self.pos < self.src.len() && self.peek() != b'"' {
                    self.pos += 1;
                }
                label = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
                self.pos = (self.pos + 1).min(self.src.len());
                nowdoc = false;
            }
            _ => {
                let start = self.pos;
                while is_ident_continue(self.peek()) {
                    self.pos += 1;
                }
                label = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
                nowdoc = false;
            }
        }

        if self.peek() == b'\r' {
            self.pos += 1;
        }
        if self.peek() == b'\n' {
            self.pos += 1;
        }

        let body_start = self.pos;
        let mut closing_indent = 0usize;
        let body_end;
        loop {
            let line_start = self.pos;
            let mut indent = 0usize;
            while matches!(self.peek(), b' ' | b'\t') {
                self.pos += 1;
                indent += 1;
            }
            if self.starts_with(&label) {
                let after = self.pos + label.len();
                let next_byte = *self.src.get(after).unwrap_or(&0);
                if !is_ident_continue(next_byte) {
                    closing_indent = indent;
                    body_end = line_start;
                    self.pos = after;
                    break;
                }
            }
            self.pos = line_start;
            while self.pos < self.src.len() && self.peek() != b'\n' {
                self.pos += 1;
            }
            if self.pos >= self.src.len() {
                // Unterminated heredoc/nowdoc: treat rest of file as body
                // rather than looping forever.
                body_end = self.pos;
                break;
            }
            self.pos += 1; // consume '\n'
        }

        let mut raw_body = &self.src[body_start..body_end];
        if raw_body.last() == Some(&b'\n') {
            raw_body = &raw_body[..raw_body.len() - 1];
        }
        if raw_body.last() == Some(&b'\r') {
            raw_body = &raw_body[..raw_body.len() - 1];
        }
        let content = strip_heredoc_indent(&String::from_utf8_lossy(raw_body), closing_indent);

        if nowdoc {
            Token::StringLit(content)
        } else {
            // `content` has already had its per-line indentation stripped
            // (PHP 7.3+ flexible heredoc syntax), so a byte offset within
            // it no longer maps 1:1 to a real offset in the original
            // source the way the plain double-quoted-string case does -
            // anchoring every `{$...}` in this heredoc at the body's own
            // start is an honest, coarser approximation for this one
            // case (still the right heredoc, not a precise per-occurrence
            // position within it).
            Token::InterpString(self.scan_interpolated(content.as_bytes(), body_start))
        }
    }

    fn lex_operator(&mut self) -> Token {
        macro_rules! op {
            ($len:expr, $tok:expr) => {{
                self.pos += $len;
                return $tok;
            }};
        }

        if self.starts_with("<=>") {
            op!(3, Token::Spaceship);
        }
        if self.starts_with("===") {
            op!(3, Token::EqualsEqualsEquals);
        }
        if self.starts_with("!==") {
            op!(3, Token::BangEqualsEquals);
        }
        if self.starts_with("??=") {
            op!(3, Token::QuestionQuestionEquals);
        }
        if self.starts_with("**=") {
            op!(3, Token::StarStarEquals);
        }
        if self.starts_with("...") {
            op!(3, Token::Ellipsis);
        }
        if self.starts_with("<<=") {
            op!(3, Token::LtLtEquals);
        }
        if self.starts_with(">>=") {
            op!(3, Token::GtGtEquals);
        }

        if self.starts_with("->") {
            op!(2, Token::Arrow);
        }
        if self.starts_with("?->") {
            op!(3, Token::NullsafeArrow);
        }
        if self.starts_with("=>") {
            op!(2, Token::FatArrow);
        }
        if self.starts_with("::") {
            op!(2, Token::DoubleColon);
        }
        if self.starts_with("??") {
            op!(2, Token::QuestionQuestion);
        }
        if self.starts_with(".=") {
            op!(2, Token::DotEquals);
        }
        if self.starts_with("+=") {
            op!(2, Token::PlusEquals);
        }
        if self.starts_with("-=") {
            op!(2, Token::MinusEquals);
        }
        if self.starts_with("*=") {
            op!(2, Token::StarEquals);
        }
        if self.starts_with("/=") {
            op!(2, Token::SlashEquals);
        }
        if self.starts_with("%=") {
            op!(2, Token::PercentEquals);
        }
        if self.starts_with("&=") {
            op!(2, Token::AmpEquals);
        }
        if self.starts_with("|=") {
            op!(2, Token::PipeEquals);
        }
        if self.starts_with("^=") {
            op!(2, Token::CaretEquals);
        }
        if self.starts_with("==") {
            op!(2, Token::EqualsEquals);
        }
        if self.starts_with("!=") {
            op!(2, Token::BangEquals);
        }
        if self.starts_with("<>") {
            op!(2, Token::BangEquals);
        }
        if self.starts_with("<=") {
            op!(2, Token::LtEquals);
        }
        if self.starts_with(">=") {
            op!(2, Token::GtEquals);
        }
        if self.starts_with("&&") {
            op!(2, Token::AmpAmp);
        }
        if self.starts_with("||") {
            op!(2, Token::PipePipe);
        }
        if self.starts_with("|>") {
            op!(2, Token::PipeGt);
        }
        if self.starts_with("++") {
            op!(2, Token::Increment);
        }
        if self.starts_with("--") {
            op!(2, Token::Decrement);
        }
        if self.starts_with("**") {
            op!(2, Token::StarStar);
        }
        if self.starts_with("<<") {
            op!(2, Token::LtLt);
        }
        if self.starts_with(">>") {
            op!(2, Token::GtGt);
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

impl<'a> Lexer<'a> {
    /// Shared body of double-quoted-string and heredoc interpolation:
    /// escapes, `$var` / `$var->prop` / `$var[key]` simple syntax, and
    /// `{$expr}` complex syntax. Operates on an already-delimited byte
    /// slice (quotes/markers already stripped by the caller) - `abs_base`
    /// is that slice's own byte offset in the *original* source (or the
    /// best available anchor - see the heredoc call site's own comment),
    /// used to give each `ComplexExpr` a real file position via
    /// `line_col_at`, not one relative to a throwaway fragment
    /// `parser::build_interpolated` re-lexes in isolation.
    fn scan_interpolated(&mut self, bytes: &[u8], abs_base: usize) -> Vec<StringPart> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut i = 0;

        macro_rules! flush {
            () => {
                if !literal.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                }
            };
        }

        while i < bytes.len() {
            let b = bytes[i];

            if b == b'\\' && i + 1 < bytes.len() {
                match bytes[i + 1] {
                    b'n' => {
                        literal.push('\n');
                        i += 2;
                    }
                    b't' => {
                        literal.push('\t');
                        i += 2;
                    }
                    b'r' => {
                        literal.push('\r');
                        i += 2;
                    }
                    b'v' => {
                        literal.push('\u{0B}');
                        i += 2;
                    }
                    b'e' => {
                        literal.push('\u{1B}');
                        i += 2;
                    }
                    b'f' => {
                        literal.push('\u{0C}');
                        i += 2;
                    }
                    b'"' => {
                        literal.push('"');
                        i += 2;
                    }
                    b'\\' => {
                        literal.push('\\');
                        i += 2;
                    }
                    b'$' => {
                        literal.push('$');
                        i += 2;
                    }
                    // Octal (`\0`-`\777`, 1-3 digits in `0`-`7`) and hex
                    // (`\x` + 1-2 hex digits) both decode to one raw
                    // byte - pushed via `as char`, the same byte-wise
                    // convention this scanner's own fallback arm below
                    // already uses for an unrecognized escape's trailing
                    // character. Real PHP silently truncates an
                    // out-of-range octal value to fit a byte.
                    b'0'..=b'7' => {
                        let mut j = i + 1;
                        let mut value: u32 = 0;
                        while j < bytes.len() && j < i + 4 && (b'0'..=b'7').contains(&bytes[j]) {
                            value = value * 8 + (bytes[j] - b'0') as u32;
                            j += 1;
                        }
                        literal.push((value % 256) as u8 as char);
                        i = j;
                    }
                    b'x' if bytes.get(i + 2).is_some_and(|c| c.is_ascii_hexdigit()) => {
                        let mut j = i + 2;
                        let mut value: u32 = 0;
                        while j < bytes.len() && j < i + 4 && bytes[j].is_ascii_hexdigit() {
                            value = value * 16 + (bytes[j] as char).to_digit(16).unwrap();
                            j += 1;
                        }
                        literal.push(value as u8 as char);
                        i = j;
                    }
                    // `\u{...}` decodes a full Unicode codepoint (not
                    // limited to one byte, unlike octal/hex above) -
                    // real PHP's one double-quoted escape that can
                    // produce a genuinely multi-byte UTF-8 sequence. An
                    // invalid codepoint (surrogate half, out of range)
                    // leaves the sequence undecoded rather than
                    // panicking.
                    b'u' if bytes.get(i + 2) == Some(&b'{') => {
                        let mut j = i + 3;
                        while j < bytes.len() && bytes[j] != b'}' {
                            j += 1;
                        }
                        let hex = std::str::from_utf8(&bytes[i + 3..j]).unwrap_or("");
                        let decoded = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                        match decoded {
                            Some(c) if j < bytes.len() => {
                                literal.push(c);
                                i = j + 1;
                            }
                            _ => {
                                literal.push('\\');
                                literal.push('u');
                                i += 2;
                            }
                        }
                    }
                    other => {
                        literal.push('\\');
                        literal.push(other as char);
                        i += 2;
                    }
                }
                continue;
            }

            if b == b'{' && bytes.get(i + 1) == Some(&b'$') {
                flush!();
                i += 1;
                let start = i;
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
                    // String-aware, the same reason and the same
                    // shared helper as `lex_interpolated_run`'s own
                    // brace-depth counter just above - this scanner
                    // does its own, independent `{`/`}` count over the
                    // same already-extracted substring, so it needs
                    // the identical fix, not just the outer one.
                    if matches!(bytes[i], b'\'' | b'"') {
                        i = skip_string_literal_bytes(bytes, i);
                        continue;
                    }
                    match bytes[i] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        i += 1;
                    }
                }
                let inner = String::from_utf8_lossy(&bytes[start..i]).into_owned();
                let base = self.line_col_at(abs_base + start);
                i = (i + 1).min(bytes.len());
                parts.push(StringPart::ComplexExpr(inner, base));
                continue;
            }

            if b == b'$' && bytes.get(i + 1).is_some_and(|&c| is_ident_start(c)) {
                flush!();
                i += 1;
                let name_start = i;
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                let name = String::from_utf8_lossy(&bytes[name_start..i]).into_owned();
                // Points at the `$` itself (one byte before `name_start`),
                // matching where the reconstructed `${name}...` string below
                // begins - precision within the reconstruction (distinguishing
                // `$name` from `->prop`/`[key]`) isn't attempted, only anchoring
                // the whole thing at its real starting position.
                let base = self.line_col_at(abs_base + name_start - 1);

                if bytes.get(i) == Some(&b'-')
                    && bytes.get(i + 1) == Some(&b'>')
                    && bytes.get(i + 2).is_some_and(|&c| is_ident_start(c))
                {
                    i += 2;
                    let prop_start = i;
                    while i < bytes.len() && is_ident_continue(bytes[i]) {
                        i += 1;
                    }
                    let prop = String::from_utf8_lossy(&bytes[prop_start..i]).into_owned();
                    parts.push(StringPart::ComplexExpr(format!("${name}->{prop}"), base));
                } else if bytes.get(i) == Some(&b'[') {
                    i += 1;
                    let key_start = i;
                    while i < bytes.len() && bytes[i] != b']' {
                        i += 1;
                    }
                    let key = String::from_utf8_lossy(&bytes[key_start..i]).into_owned();
                    i = (i + 1).min(bytes.len());
                    let key_expr = if key.starts_with('$') || key.parse::<i64>().is_ok() {
                        key
                    } else {
                        format!("'{key}'")
                    };
                    parts.push(StringPart::ComplexExpr(
                        format!("${name}[{key_expr}]"),
                        base,
                    ));
                } else {
                    parts.push(StringPart::SimpleVar(name));
                }
                continue;
            }

            literal.push(b as char);
            i += 1;
        }
        flush!();

        if parts.is_empty() {
            parts.push(StringPart::Literal(String::new()));
        }
        parts
    }
}

/// Flexible heredoc (PHP 7.3+): every content line has up to `indent`
/// leading space/tab characters stripped, matching the closing marker's
/// own indentation.
fn strip_heredoc_indent(text: &str, indent: usize) -> String {
    if indent == 0 {
        return text.to_string();
    }
    text.split('\n')
        .map(|line| {
            let mut cut = 0;
            for (i, c) in line.chars().enumerate() {
                if i >= indent || !(c == ' ' || c == '\t') {
                    break;
                }
                cut += c.len_utf8();
            }
            &line[cut..]
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::StringPart;

    // Coverage measurement (cargo llvm-cov) found lex_heredoc and
    // strip_heredoc_indent had zero test executions - lexer.rs had no
    // test module at all before this. Heredoc/nowdoc are real, common
    // PHP syntax (flexible/PHP-7.3+ indentation stripping in
    // particular), verified manually correct before writing these.

    #[test]
    fn nowdoc_strips_the_closing_markers_indentation_with_no_interpolation() {
        let src = "<?php\n$x = <<<'EOT'\n    hello\n    world\n    EOT;\n";
        let toks = Lexer::new(src).tokenize();
        assert_eq!(toks[2], Token::StringLit("hello\nworld".to_string()));
    }

    #[test]
    fn heredoc_strips_indentation_and_still_interpolates_variables() {
        let src = "<?php\n$x = <<<EOT\n    Hello, $name!\n    EOT;\n";
        let toks = Lexer::new(src).tokenize();
        assert_eq!(
            toks[2],
            Token::InterpString(vec![
                StringPart::Literal("Hello, ".to_string()),
                StringPart::SimpleVar("name".to_string()),
                StringPart::Literal("!".to_string()),
            ])
        );
    }

    #[test]
    fn unterminated_heredoc_does_not_hang_or_panic() {
        let src = "<?php\n$x = <<<EOT\nno closing label here\n";
        // Only the absence of a panic/infinite loop matters here - an
        // unterminated heredoc is malformed input, and the documented
        // behavior is to treat the rest of the file as its body rather
        // than error out, matching this lexer's error-recovery style
        // elsewhere.
        let _ = Lexer::new(src).tokenize();
    }

    // `skip_stub_availability_attribute` - see its own doc comment for the
    // real-world motivation (array_map's superseded, closed-range,
    // no-default `$arrays` parameter).

    #[test]
    fn closed_range_stub_attribute_with_no_default_drops_its_whole_parameter() {
        let src = "<?php function f(?callable $cb, #[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $arrays, array ...$arrays) {}";
        let with_attr = Lexer::new(src).tokenize();
        let without_attr =
            Lexer::new("<?php function f(?callable $cb, array ...$arrays) {}").tokenize();
        assert_eq!(with_attr, without_attr);
    }

    #[test]
    fn closed_range_stub_attribute_as_the_only_parameter_leaves_empty_parens() {
        let src =
            "<?php function f(#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $x) {}";
        let with_attr = Lexer::new(src).tokenize();
        let without_attr = Lexer::new("<?php function f() {}").tokenize();
        assert_eq!(with_attr, without_attr);
    }

    #[test]
    fn open_ended_stub_attribute_keeps_its_parameter() {
        let src = "<?php function f(#[PhpStormStubsElementAvailable(from: '8.0')] array $array) {}";
        let with_attr = Lexer::new(src).tokenize();
        let without_attr = Lexer::new("<?php function f(array $array) {}").tokenize();
        assert_eq!(with_attr, without_attr);
    }

    #[test]
    fn closed_range_stub_attribute_on_a_param_with_a_real_default_keeps_it() {
        let src = "<?php function f(#[PhpStormStubsElementAvailable(from: '5.3', to: '5.6')] $flags = 6) {}";
        let with_attr = Lexer::new(src).tokenize();
        let without_attr = Lexer::new("<?php function f($flags = 6) {}").tokenize();
        assert_eq!(with_attr, without_attr);
    }

    #[test]
    fn closed_range_stub_attribute_gating_a_whole_method_is_dropped_entirely() {
        // Round 15 of the Piwigo corpus cleanup: a closed range (`to:`)
        // gating a whole method - real shape:
        // `DateTimeImmutable::__construct`'s `to: '8.2'`/`from: '8.3'`
        // pair - is superseded on the PHP version this project models,
        // same as a closed-range *parameter*, and must leave no trace at
        // all rather than surviving as an ordinary, unconditional
        // declaration (which the registry would then wrongly merge
        // return types with the real, modern declaration).
        let src = "<?php class C { #[PhpStormStubsElementAvailable(from: '5.5', to: '8.2')]\n\
                   public function __construct($datetime = \"now\") {} }";
        let with_attr = Lexer::new(src).tokenize();
        let empty_class = Lexer::new("<?php class C { }").tokenize();
        assert_eq!(with_attr, empty_class);
    }

    #[test]
    fn open_ended_stub_attribute_gating_a_whole_method_is_kept() {
        // The counterpart to the above - an open range (`from` only,
        // still valid on the latest PHP) gating a whole method is the
        // real, current declaration and must survive unchanged.
        let src = "<?php class C { #[PhpStormStubsElementAvailable(from: '8.3')]\n\
                   public function __construct($datetime = \"now\") {} }";
        let with_attr = Lexer::new(src).tokenize();
        let without_attr =
            Lexer::new("<?php class C { public function __construct($datetime = \"now\") {} }")
                .tokenize();
        assert_eq!(with_attr, without_attr);
    }

    #[test]
    fn closed_range_stub_attribute_gating_a_whole_abstract_method_is_dropped_entirely() {
        // The `;`-terminated (no body) shape - an interface/abstract
        // method declaration - must be recognized as a whole-declaration
        // terminator too, not just a `{...}` body.
        let src = "<?php interface I { #[PhpStormStubsElementAvailable(to: '7.4')]\n\
                   public function old(string $x); }";
        let with_attr = Lexer::new(src).tokenize();
        let empty_interface = Lexer::new("<?php interface I { }").tokenize();
        assert_eq!(with_attr, empty_interface);
    }

    #[test]
    fn closed_range_stub_attribute_gating_a_whole_free_function_is_dropped_entirely() {
        // The real version_compare() shape: two whole *free function*
        // declarations (not methods), the closed-range one superseded.
        let src = "<?php #[PhpStormStubsElementAvailable(to: '7.4')]\n\
                   function version_compare(string $a, string $b): ?bool {}\n\
                   #[PhpStormStubsElementAvailable(from: '8.0')]\n\
                   function version_compare(string $a, string $b): bool {}";
        let toks = Lexer::new(src).tokenize();
        let expected =
            Lexer::new("<?php function version_compare(string $a, string $b): bool {}").tokenize();
        assert_eq!(toks, expected);
    }

    // Phase 24: `#[LanguageLevelTypeAware]` - a real shape from
    // `substr()`'s own stub: no native return-type hint at all, an
    // existing but stale `@return` doc comment, and this attribute
    // immediately above `function`.

    #[test]
    fn language_level_type_aware_appends_the_modern_return_type_to_the_doc_comment() {
        let src = "<?php\n/** @return string|false */\n#[LanguageLevelTypeAware([\"8.0\" => \"string\"], default: \"string|false\")]\nfunction f() {}";
        let toks = Lexer::new(src).tokenize();
        let Token::DocComment(doc) = &toks[0] else {
            panic!("expected a doc comment first, got: {toks:?}");
        };
        assert!(
            doc.contains("@return string\n"),
            "modern @return not appended: {doc:?}"
        );
        assert!(
            doc.contains("@return string|false"),
            "original @return line should still be present: {doc:?}"
        );
    }

    #[test]
    fn language_level_type_aware_picks_the_highest_versioned_key_over_default() {
        let src = "<?php\n/** @return resource|false */\n#[LanguageLevelTypeAware(['8.1' => 'PgSql\\Connection|false'], default: 'resource|false')]\nfunction pg_connect() {}";
        let toks = Lexer::new(src).tokenize();
        let Token::DocComment(doc) = &toks[0] else {
            panic!("expected a doc comment first, got: {toks:?}");
        };
        assert!(
            doc.contains("@return PgSql\\Connection|false"),
            "expected the 8.1 override, not the legacy default: {doc:?}"
        );
    }

    #[test]
    fn language_level_type_aware_picks_the_numerically_highest_of_several_keys() {
        let src = "<?php\n/** @return int */\n#[LanguageLevelTypeAware(['8.1' => 'A', '8.10' => 'B', '8.2' => 'C'], default: 'int')]\nfunction f() {}";
        let toks = Lexer::new(src).tokenize();
        let Token::DocComment(doc) = &toks[0] else {
            panic!("expected a doc comment first, got: {toks:?}");
        };
        assert!(
            doc.contains("@return B"),
            "8.10 should numerically outrank 8.2/8.1, not string-sort behind them: {doc:?}"
        );
    }

    #[test]
    fn language_level_type_aware_on_a_parameter_is_not_treated_as_a_return_override() {
        // Real shape: GD's `#[LanguageLevelTypeAware(['8.1' => 'GdFont|int'],
        // default: 'int')] $font` - immediately followed by `$name`, not
        // `function`. Must not append a spurious `@return` override -
        // this shape becomes injected native tokens instead (see the
        // dedicated `_injects_a_real_native_type_hint` test below).
        let src = "<?php\n/** @param int $font */\nfunction f(#[LanguageLevelTypeAware(['8.1' => 'GdFont|int'], default: 'int')] $font) {}";
        let toks = Lexer::new(src).tokenize();
        let Token::DocComment(doc) = &toks[0] else {
            panic!("expected a doc comment first, got: {toks:?}");
        };
        assert!(
            !doc.contains("@return"),
            "a parameter-gating attribute must not append a @return override: {doc:?}"
        );
    }

    #[test]
    fn language_level_type_aware_on_a_parameter_injects_a_real_native_type_hint() {
        // Real shape: `finfo_file`'s own `#[LanguageLevelTypeAware(['8.1'
        // => 'finfo'], default: 'resource')] $finfo` - no separate native
        // hint of its own, so the modern type has to arrive as a real,
        // synthetic type-hint token sequence right before `$finfo`.
        let src = "<?php\nfunction f(#[LanguageLevelTypeAware(['8.1' => 'finfo'], default: 'resource')] $finfo) {}";
        let with_attr = Lexer::new(src).tokenize();
        let without_attr = Lexer::new("<?php\nfunction f(finfo $finfo) {}").tokenize();
        assert_eq!(with_attr, without_attr);
    }

    #[test]
    fn language_level_type_aware_on_a_parameter_with_bracket_shorthand_is_left_alone() {
        // Real shape (found via a crash while verifying this fix against
        // this project's own bundled stub corpus): `strip_tags()`'s own
        // `#[LanguageLevelTypeAware(["7.4" => "string[]|string|null"],
        // default: "string|null")] $allowed_tags = null` - `TYPE[]` is
        // PHPDoc-only shorthand, not valid native type-hint syntax at
        // all, so injecting it as native tokens crashed the parser
        // outright (`expected variable, found LBracket`) before this
        // guard existed. Must fall back to leaving the parameter
        // untouched (today's pre-existing, safe default) rather than
        // injecting anything.
        let src = "<?php\nfunction f(#[LanguageLevelTypeAware([\"7.4\" => \"string[]|string|null\"], default: \"string|null\")] $allowed_tags = null) {}";
        let with_attr = Lexer::new(src).tokenize();
        let without_attr = Lexer::new("<?php\nfunction f($allowed_tags = null) {}").tokenize();
        assert_eq!(with_attr, without_attr);
    }

    #[test]
    fn language_level_type_aware_with_no_preceding_doc_comment_is_a_safe_no_op() {
        let src = "<?php\n#[LanguageLevelTypeAware([\"8.0\" => \"string\"], default: \"string|false\")]\nfunction f() {}";
        // Must not panic, and must tokenize identically to the attribute
        // being absent entirely (nothing to rewrite - no crash, no stray
        // synthetic doc comment materializing from nowhere).
        let with_attr = Lexer::new(src).tokenize();
        let without_attr = Lexer::new("<?php\nfunction f() {}").tokenize();
        assert_eq!(with_attr, without_attr);
    }

    // `tokenize_with_spans` - Phase 11's source-position work.

    #[test]
    fn tokens_on_a_later_line_get_real_line_and_column_positions() {
        let src = "<?php\n$x = 1;\n$y = 2;\n";
        let toks = Lexer::new(src).tokenize_with_spans();
        // toks: Variable(x), Equals, Integer(1), Semicolon, Variable(y), ...
        let (tok, span) = &toks[4];
        assert_eq!(*tok, Token::Variable("y".to_string()));
        assert_eq!(span.start, Position { line: 3, col: 1 });
        assert_eq!(span.end, Position { line: 3, col: 3 });
    }

    #[test]
    fn a_token_spanning_multiple_lines_does_not_throw_off_the_next_token_s_position() {
        let src = "<?php\n$x = <<<EOT\nhello\nworld\nEOT;\n$y = 1;\n";
        let toks = Lexer::new(src).tokenize_with_spans();
        let heredoc_end = toks
            .iter()
            .find(|(t, _)| matches!(t, Token::InterpString(_)))
            .unwrap()
            .1
            .end;
        // The heredoc body itself spans lines 3-4, but the token's own
        // extent runs through its closing marker on line 5, not just the
        // last content line.
        assert_eq!(heredoc_end.line, 5);
        let (next_tok, next_span) = toks
            .iter()
            .find(|(t, _)| *t == Token::Variable("y".to_string()))
            .unwrap();
        assert_eq!(*next_tok, Token::Variable("y".to_string()));
        assert_eq!(next_span.start, Position { line: 6, col: 1 });
    }

    #[test]
    fn a_multi_byte_character_earlier_on_a_line_does_not_shift_later_columns() {
        // "é" is 2 bytes in UTF-8 but 1 character - a byte-offset column
        // would report column 8 for $x below; the real (character) column
        // is 7.
        let src = "<?php\n'é'; $x;\n";
        let toks = Lexer::new(src).tokenize_with_spans();
        let (_, span) = toks
            .iter()
            .find(|(t, _)| *t == Token::Variable("x".to_string()))
            .unwrap();
        assert_eq!(span.start, Position { line: 2, col: 6 });
    }

    #[test]
    fn complex_interpolation_gets_a_real_base_position_on_a_later_line() {
        let src = "<?php\n$s = \"a\\n{$obj->prop}\";\n";
        let toks = Lexer::new(src).tokenize_with_spans();
        let (tok, _) = toks
            .iter()
            .find(|(t, _)| matches!(t, Token::InterpString(_)))
            .unwrap();
        let Token::InterpString(parts) = tok else {
            unreachable!()
        };
        let (_, base) = parts
            .iter()
            .find_map(|p| match p {
                StringPart::ComplexExpr(src, base) => Some((src.clone(), *base)),
                _ => None,
            })
            .expect("expected a ComplexExpr part");
        // The literal "a\n" inside the string doesn't introduce a real
        // newline in the source - {$obj->prop} is still on line 2.
        // `$s = "a\n{$obj->prop}";` - the `$` of `$obj` is the 11th
        // character (right after `{`, the 10th).
        assert_eq!(base, Position { line: 2, col: 11 });
    }

    #[test]
    fn shorthand_property_interpolation_base_points_at_the_dollar_sign() {
        let src = "<?php\n$s = \"x $obj->prop y\";\n";
        let toks = Lexer::new(src).tokenize_with_spans();
        let (tok, _) = toks
            .iter()
            .find(|(t, _)| matches!(t, Token::InterpString(_)))
            .unwrap();
        let Token::InterpString(parts) = tok else {
            unreachable!()
        };
        let (src_text, base) = parts
            .iter()
            .find_map(|p| match p {
                StringPart::ComplexExpr(src, base) => Some((src.clone(), *base)),
                _ => None,
            })
            .expect("expected a ComplexExpr part");
        assert_eq!(src_text, "$obj->prop");
        // `$s = "x $obj->prop y";` - the `$` of `$obj` is the 9th
        // character.
        assert_eq!(base, Position { line: 2, col: 9 });
    }

    #[test]
    fn many_tokens_on_one_extremely_long_line_does_not_blow_up_quadratically() {
        // Real bug, not hypothetical: `line_col_at`'s first implementation
        // recounted every character from the line's own start on *every*
        // call, which is fine for ordinary code but a genuine O(line
        // length x tokens on it) blowup on a real file in this project's
        // own test corpus - `amphp/hpack`'s generated Huffman lookup
        // table has a single line 1.25 million characters long, and hung
        // for minutes (previously well under a second) before the
        // incremental-cache fix. This is a smaller version of the same
        // shape: one 200,000-character line with thousands of tokens on
        // it - a quadratic implementation is easily slow enough here to
        // fail this test's time budget; a linear one finishes instantly.
        let mut src = "<?php $a = [".to_string();
        for i in 0..40_000 {
            if i > 0 {
                src.push(',');
            }
            src.push_str(&i.to_string());
        }
        src.push_str("];\n");
        assert!(src.len() > 200_000);

        let start = std::time::Instant::now();
        let toks = Lexer::new(&src).tokenize_with_spans();
        let elapsed = start.elapsed();

        assert!(toks.len() > 40_000);
        assert!(
            elapsed.as_secs() < 2,
            "tokenizing one long line took {elapsed:?} - looks quadratic again"
        );
    }
    // Finding 20 from the architecture audit - double-quoted strings only
    // decoded 6 of PHP's real escape sequences; anything else kept the
    // literal backslash and following character verbatim.

    #[test]
    fn hex_escape_decodes_to_the_real_byte() {
        let src = r#"<?php "\x41BC";"#;
        let toks = Lexer::new(src).tokenize();
        assert_eq!(
            toks[0],
            Token::InterpString(vec![StringPart::Literal("ABC".to_string())])
        );
    }

    #[test]
    fn octal_escape_decodes_to_the_real_byte() {
        let src = r#"<?php "\101BC";"#;
        let toks = Lexer::new(src).tokenize();
        assert_eq!(
            toks[0],
            Token::InterpString(vec![StringPart::Literal("ABC".to_string())])
        );
    }

    #[test]
    fn unicode_escape_decodes_to_the_real_codepoint() {
        let src = r#"<?php "\u{41}BC";"#;
        let toks = Lexer::new(src).tokenize();
        assert_eq!(
            toks[0],
            Token::InterpString(vec![StringPart::Literal("ABC".to_string())])
        );
    }

    #[test]
    fn vertical_tab_escape_and_form_feed_decode_correctly() {
        let src = r#"<?php "\v\e\f";"#;
        let toks = Lexer::new(src).tokenize();
        assert_eq!(
            toks[0],
            Token::InterpString(vec![StringPart::Literal("\u{0B}\u{1B}\u{0C}".to_string())])
        );
    }

    #[test]
    fn an_unrecognized_escape_still_keeps_the_literal_backslash_and_character() {
        let src = r#"<?php "\d+";"#;
        let toks = Lexer::new(src).tokenize();
        assert_eq!(
            toks[0],
            Token::InterpString(vec![StringPart::Literal("\\d+".to_string())])
        );
    }

    #[test]
    fn the_original_six_escapes_still_decode_correctly() {
        let src = r#"<?php "a\nb\tc\r\"d\\e\$f";"#;
        let toks = Lexer::new(src).tokenize();
        assert_eq!(
            toks[0],
            Token::InterpString(vec![StringPart::Literal("a\nb\tc\r\"d\\e$f".to_string())])
        );
    }
    // Finding 6 from the architecture audit - the #[Attribute(...)] skip
    // logic wasn't string-aware, so an unbalanced `[`/`]` inside an
    // argument string (a completely ordinary regex character class)
    // ended the skip early and corrupted the rest of the parse.

    #[test]
    fn attribute_skip_is_not_fooled_by_an_unbalanced_bracket_in_a_string_argument() {
        let src = "<?php #[Assert\\Regex('/[^\\]]+/')]\nclass C {}\n";
        let toks = Lexer::new(src).tokenize();
        // The attribute is skipped entirely - the next real token is the
        // `class` keyword, not garbage from inside the regex string.
        assert_eq!(toks[0], Token::Keyword(Keyword::Class));
    }
    // Finding 10 from the architecture audit - the same string-
    // unawareness bug as Finding 6, in {$...} complex interpolation's
    // own brace-depth counters (two independent ones: lex_interpolated_run
    // scanning for the block's own extent, and scan_interpolated's
    // separate pass carving it into StringPart pieces).

    #[test]
    fn complex_interpolation_is_not_fooled_by_a_brace_inside_a_nested_string_argument() {
        let src = r#"<?php "value: {$f->wrap('}')} end";"#;
        let toks = Lexer::new(src).tokenize();
        // A successful, non-panicking tokenize that reaches Eof (rather
        // than the old parse error stopping partway through the file)
        // is the real regression test here - the interpolation's own
        // ComplexExpr part additionally proves the block's real extent
        // (through the true closing brace, not the one inside the
        // string) was found correctly.
        assert_eq!(toks.last(), Some(&Token::Eof));
        let Token::InterpString(parts) = &toks[0] else {
            panic!("expected an interpolated string, got {:?}", toks[0]);
        };
        assert!(
            parts.iter().any(
                |p| matches!(p, StringPart::ComplexExpr(inner, _) if inner == "$f->wrap('}')")
            ),
            "got: {parts:?}"
        );
    }
}
