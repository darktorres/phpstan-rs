use crate::lexer::{Lexer, is_ident_continue, is_ident_start, skip_string_literal_bytes};
use crate::name::Interner;
use crate::php_string::PhpString;
use crate::token::{SimpleIndexKey, Span, StringPart, Token};

/// Whether `s` is a full, well-formed PHP identifier (not just that it
/// *starts* like one) - `$var[key]` interpolation's own bareword-string-key
/// and variable-key shapes both need this, since `scan_interpolated`
/// extracts everything up to the closing `]` first and only classifies it
/// afterward, unlike the `->prop` shape (whose own entry condition already
/// guarantees a well-formed identifier by construction).
fn is_full_identifier(s: &str) -> bool {
    let bytes = s.as_bytes();
    match bytes.split_first() {
        Some((&first, rest)) => is_ident_start(first) && rest.iter().all(|&b| is_ident_continue(b)),
        None => false,
    }
}

/// A `{$...}` complex-interpolation block's own inner text, classified as
/// one of the two simple-syntax shapes bare `$var->prop`/`$var[key]`
/// already special-case - byte offsets are relative to the start of the
/// classified slice (which itself starts at the `$`), not the whole file.
/// `None` from `try_parse_simple_interpolation` means the block doesn't
/// consist of *only* one of these shapes (a bare `{$var}`, a method call,
/// an operator, a nested `{$...}`, ...) - real complex syntax (or the
/// bare-var case, which gains nothing from fast-pathing - see its own
/// note below), still routed through `StringPart::ComplexExpr`'s re-lex
/// path unchanged.
enum SimpleInterpolationShape {
    PropertyAccess {
        name_end: usize,
        prop_start: usize,
    },
    Index {
        name_end: usize,
        key_start: usize,
        key_end: usize,
    },
}

/// Tries to classify `bytes` (a `{$...}` block's own inner text, e.g. the
/// `$var->prop` in `{$var->prop}`) as one of `SimpleInterpolationShape`'s
/// two shapes - unlike the bare-`$` scan below (which stops as soon as
/// the name/prop/key ends, since whatever follows is separate literal
/// text), this requires the shape to consume the *entire* slice, since a
/// braced block's own text could just as easily continue into a method
/// call, an operator, or anything else complex syntax allows.
///
/// Deliberately does NOT fast-path a bare `{$var}` (unlike the bare-`$`
/// scan, which does via `StringPart::SimpleVar`): `SimpleVar` carries no
/// position of its own (`build_interpolated` gives every `SimpleVar`/
/// `Literal` part the whole *string's* own start position, not a
/// per-occurrence one - harmless there since a string's bare `$var`
/// parts don't need finer-grained positions for any existing diagnostic),
/// but `{$var}` today gets a real per-occurrence position via the
/// `ComplexExpr` path (lexed at its real position) - fast-pathing it into a
/// position-less `SimpleVar` would collapse every bare-braced-var
/// diagnostic in a string with more than one onto the same (wrong)
/// column, confirmed as a real regression via corpus diff before this
/// fix. A bare-var re-lex is already the cheapest possible `ComplexExpr`
/// case anyway, so there's no real perf reason to fast-path it.
fn try_parse_simple_interpolation(bytes: &[u8]) -> Option<SimpleInterpolationShape> {
    if bytes.first() != Some(&b'$') || !bytes.get(1).is_some_and(|&c| is_ident_start(c)) {
        return None;
    }
    let mut j = 2;
    while j < bytes.len() && is_ident_continue(bytes[j]) {
        j += 1;
    }
    let name_end = j;
    if j == bytes.len() {
        return None;
    }
    if bytes[j] == b'-'
        && bytes.get(j + 1) == Some(&b'>')
        && bytes.get(j + 2).is_some_and(|&c| is_ident_start(c))
    {
        let mut k = j + 2;
        while k < bytes.len() && is_ident_continue(bytes[k]) {
            k += 1;
        }
        return (k == bytes.len()).then_some(SimpleInterpolationShape::PropertyAccess {
            name_end,
            prop_start: j + 2,
        });
    }
    if bytes[j] == b'[' {
        let key_start = j + 1;
        let mut k = key_start;
        while k < bytes.len() && bytes[k] != b']' {
            k += 1;
        }
        return (k < bytes.len() && k + 1 == bytes.len()).then_some(
            SimpleInterpolationShape::Index {
                name_end,
                key_start,
                key_end: k,
            },
        );
    }
    None
}

/// Classifies a `$var[key]` interpolation's own raw `key` text into one of
/// `SimpleIndexKey`'s three shapes, or `None` when it doesn't fit any of
/// them (the caller then falls back to `StringPart::ComplexExpr`'s re-lex
/// path) - see `SimpleIndexKey`'s own doc comment for what real PHP accepts
/// here.
fn classify_index_key(key: &str, interner: &Interner) -> Option<SimpleIndexKey> {
    if let Some(var_name) = key.strip_prefix('$') {
        return is_full_identifier(var_name).then(|| SimpleIndexKey::Var(interner.name(var_name)));
    }
    // A number is an int key only when written canonically and within
    // range (`0|[1-9][0-9]*`, optionally negative, not `-0`); `01`, `-0`
    // and `9223372036854775808` are string keys, as PHP reads them.
    let digits = key.strip_prefix('-').unwrap_or(key);
    if digits.starts_with(|c: char| c.is_ascii_digit()) {
        if !is_number_token(digits) {
            return None;
        }
        let canonical = (digits == "0" && key == "0")
            || (!digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit()));
        return Some(match key.parse::<i64>() {
            Ok(n) if canonical => SimpleIndexKey::Int(n),
            _ => SimpleIndexKey::Str(interner.str(key)),
        });
    }
    is_full_identifier(key).then(|| SimpleIndexKey::Str(interner.str(key)))
}

/// Whether `text` is an integer as PHP's lexer reads one in a simple
/// interpolation's index: decimal, `0x` hex, `0b` binary or `0o` octal,
/// digits optionally separated by single `_`.
fn is_number_token(text: &str) -> bool {
    let (body, is_digit): (&str, fn(&u8) -> bool) =
        match text.get(..2).map(str::to_ascii_lowercase).as_deref() {
            Some("0x") => (&text[2..], u8::is_ascii_hexdigit),
            Some("0b") => (&text[2..], |b| matches!(b, b'0' | b'1')),
            Some("0o") => (&text[2..], |b| matches!(b, b'0'..=b'7')),
            _ => (text, u8::is_ascii_digit),
        };
    !body.is_empty()
        && body
            .split('_')
            .all(|run| !run.is_empty() && run.as_bytes().iter().all(is_digit))
}

impl<'a> Lexer<'a> {
    pub(crate) fn lex_single_quoted(&mut self) -> Token {
        self.pos += 1; // opening '
        // Batches contiguous non-escape byte runs via `push_str` +
        // `from_utf8_lossy` instead of pushing one byte at a time (which
        // would also silently corrupt any multi-byte UTF-8 character in
        // the literal - `b as char` reinterprets each raw byte as its own
        // Latin-1 codepoint). `from_utf8_lossy`, not `.unwrap()`: real
        // PHP source can contain non-UTF-8 bytes, matching every other
        // byte-range conversion already in this file.
        //
        // No `with_capacity` hint here (unlike `scan_interpolated`'s
        // `literal` buffer, which sizes to its own already-extracted
        // run's length): the only cheap upper bound available at this
        // point is `self.src.len() - self.pos`, the size of the entire
        // rest of the file rather than of this one string, which can
        // blow up peak memory on a large file with many short early
        // literals. Finding the string's real length upfront would need
        // its own pre-scan, not worth it for what's normally a short
        // literal - let `String::new()` grow via ordinary amortized
        // doubling.
        let mut s = Vec::new();
        let mut run_start = self.pos;
        while self.pos < self.src.len() && self.peek() != b'\'' {
            if self.peek() == b'\\' && (self.peek_at(1) == b'\'' || self.peek_at(1) == b'\\') {
                s.extend_from_slice(&self.src[run_start..self.pos]);
                s.push(self.peek_at(1));
                self.pos += 2;
                run_start = self.pos;
            } else {
                self.pos += 1;
            }
        }
        s.extend_from_slice(&self.src[run_start..self.pos]);
        self.pos += 1; // closing '
        Token::StringLit(PhpString::from(s))
    }

    pub(crate) fn lex_double_quoted(&mut self) -> Token {
        Token::InterpString(self.lex_interpolated_run(b'"'))
    }

    pub(crate) fn lex_backtick(&mut self) -> Token {
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
            } else if (self.peek() == b'{' && self.peek_at(1) == b'$')
                || (self.peek() == b'$' && self.peek_at(1) == b'{')
            {
                if self.peek() == b'$' {
                    self.pos += 1;
                }
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
        let parts = self.scan_interpolated(raw, start, Some(delim), None);
        self.pos = (self.pos + 1).min(self.src.len()); // closing delimiter
        parts
    }

    pub(crate) fn lex_heredoc(&mut self) -> Token {
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
            if self.starts_with_text(&label) {
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
        let (content, offsets) = strip_heredoc_indent(raw_body, closing_indent);

        if nowdoc {
            Token::StringLit(PhpString::from(content))
        } else {
            // `content` has its per-line indentation stripped (PHP 7.3+
            // flexible heredoc syntax); `offsets` maps each of its bytes
            // back to the source.
            Token::InterpString(self.scan_interpolated(&content, body_start, None, Some(&offsets)))
        }
    }
}

impl<'a> Lexer<'a> {
    /// Shared body of double-quoted-string, shell-exec and heredoc
    /// interpolation (`delimiter` the closing quote, `None` for a heredoc):
    /// escapes, `$var` / `$var->prop` / `$var[key]` simple syntax, and
    /// `{$expr}` complex syntax. Operates on an already-delimited byte
    /// slice (quotes/markers already stripped by the caller) - `abs_base`
    /// is that slice's own byte offset in the *original* source, and
    /// `offsets` maps a heredoc's indent-stripped bytes back to it. A
    /// `{$expr}`'s text is taken from the source itself, so a re-lexed
    /// expression spanning lines keeps its columns; each `ComplexExpr` gets
    /// a real file position via
    /// `line_col_at`, not one relative to a throwaway fragment
    /// `parser::build_interpolated` re-lexes in isolation.
    fn scan_interpolated(
        &mut self,
        bytes: &[u8],
        abs_base: usize,
        delimiter: Option<u8>,
        offsets: Option<&[usize]>,
    ) -> Vec<StringPart> {
        let mut parts = Vec::new();
        // Upper-bound capacity hint - avoids repeated reallocation while
        // pushing long plain-text runs (SQL, translation strings, HTML in
        // a heredoc, ...).
        let mut literal: Vec<u8> = Vec::with_capacity(bytes.len());
        // Where `literal`'s chunk starts in `bytes`.
        let mut literal_start = 0;
        // A byte of `bytes` as an offset in the file: a heredoc's content
        // has its indentation stripped, so `offsets` maps it back.
        let off = |i: usize| abs_base + offsets.map_or(i, |m| m[i.min(m.len() - 1)]);
        let mut i = 0;

        macro_rules! flush {
            () => {
                if !literal.is_empty() {
                    let span = Span {
                        start: self.line_col_at(off(literal_start)),
                        end: self.line_col_at(off(i)),
                    };
                    parts.push(StringPart::Literal(
                        PhpString::from(std::mem::take(&mut literal)),
                        span,
                    ));
                }
            };
        }

        while i < bytes.len() {
            let b = bytes[i];

            if b == b'\\' && i + 1 < bytes.len() {
                if literal.is_empty() {
                    literal_start = i;
                }
                match bytes[i + 1] {
                    b'n' => {
                        literal.push(b'\n');
                        i += 2;
                    }
                    b't' => {
                        literal.push(b'\t');
                        i += 2;
                    }
                    b'r' => {
                        literal.push(b'\r');
                        i += 2;
                    }
                    b'v' => {
                        literal.push(0x0B);
                        i += 2;
                    }
                    b'e' => {
                        literal.push(0x1B);
                        i += 2;
                    }
                    b'f' => {
                        literal.push(0x0C);
                        i += 2;
                    }
                    // `\"` escapes only in `"..."` and `` \` `` only in
                    // `` `...` ``; elsewhere (a heredoc included) both stay
                    // two bytes.
                    quote @ (b'"' | b'`') if delimiter == Some(quote) => {
                        literal.push(quote);
                        i += 2;
                    }
                    quote @ (b'"' | b'`') => {
                        literal.extend_from_slice(&[b'\\', quote]);
                        i += 2;
                    }
                    b'\\' => {
                        literal.push(b'\\');
                        i += 2;
                    }
                    b'$' => {
                        literal.push(b'$');
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
                        literal.push((value % 256) as u8);
                        i = j;
                    }
                    b'x' if bytes.get(i + 2).is_some_and(|c| c.is_ascii_hexdigit()) => {
                        let mut j = i + 2;
                        let mut value: u32 = 0;
                        while j < bytes.len() && j < i + 4 && bytes[j].is_ascii_hexdigit() {
                            value = value * 16 + (bytes[j] as char).to_digit(16).unwrap();
                            j += 1;
                        }
                        literal.push(value as u8);
                        i = j;
                    }
                    // `\u{...}` decodes a full Unicode codepoint (not
                    // limited to one byte, unlike octal/hex above) -
                    // real PHP's one double-quoted escape that can
                    // produce a genuinely multi-byte UTF-8 sequence. An
                    // invalid codepoint (surrogate half, out of range)
                    // leaves the sequence undecoded rather than
                    // panicking.
                    // `\u{hex}`: a codepoint up to U+10FFFF, surrogates
                    // included, as UTF-8; anything else is a compile error.
                    b'u' if bytes.get(i + 2) == Some(&b'{') => {
                        let mut j = i + 3;
                        while j < bytes.len() && bytes[j] != b'}' {
                            j += 1;
                        }
                        let hex = std::str::from_utf8(&bytes[i + 3..j]).unwrap_or("");
                        let codepoint = (j < bytes.len()
                            && !hex.is_empty()
                            && hex.bytes().all(|b| b.is_ascii_hexdigit()))
                        .then(|| u32::from_str_radix(hex, 16).ok())
                        .flatten();
                        match codepoint {
                            Some(c) if c <= 0x10FFFF => {
                                literal.extend_from_slice(&encode_codepoint(c));
                                i = j + 1;
                            }
                            _ => {
                                let span = Span {
                                    start: self.line_col_at(off(i)),
                                    end: self.line_col_at(off((j + 1).min(bytes.len()))),
                                };
                                let too_large = codepoint.is_some() || hex.len() > 8;
                                self.warnings.push(crate::lexer::LexWarning {
                                    span,
                                    message: if too_large {
                                        "Invalid UTF-8 codepoint escape sequence: Codepoint too large"
                                            .to_string()
                                    } else {
                                        "Invalid UTF-8 codepoint escape sequence".to_string()
                                    },
                                    is_error: true,
                                });
                                literal.extend_from_slice(b"\\u");
                                i += 2;
                            }
                        }
                    }
                    other => {
                        literal.push(b'\\');
                        literal.push(other);
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
                let inner_bytes = &bytes[start..i];
                let base = self.line_col_at(off(start));
                let close_brace_pos = i;
                i = (i + 1).min(bytes.len());
                match try_parse_simple_interpolation(inner_bytes) {
                    Some(SimpleInterpolationShape::PropertyAccess {
                        name_end,
                        prop_start,
                    }) => {
                        let name = self
                            .interner
                            .name(&String::from_utf8_lossy(&inner_bytes[1..name_end]));
                        let prop = self
                            .interner
                            .name(&String::from_utf8_lossy(&inner_bytes[prop_start..]));
                        let var_end = self.line_col_at(off(start + name_end));
                        let end = self.line_col_at(off(close_brace_pos));
                        parts.push(StringPart::PropertyAccess {
                            var: name,
                            var_start: base,
                            var_end,
                            prop,
                            nullsafe: false,
                            end,
                        });
                    }
                    Some(SimpleInterpolationShape::Index {
                        name_end,
                        key_start,
                        key_end,
                    }) => {
                        let key_text = String::from_utf8_lossy(&inner_bytes[key_start..key_end]);
                        match classify_index_key(&key_text, &self.interner) {
                            Some(key) => {
                                let name = self
                                    .interner
                                    .name(&String::from_utf8_lossy(&inner_bytes[1..name_end]));
                                let var_end = self.line_col_at(off(start + name_end));
                                let end = self.line_col_at(off(close_brace_pos));
                                parts.push(StringPart::Index {
                                    var: name,
                                    var_start: base,
                                    var_end,
                                    key,
                                    end,
                                });
                            }
                            None => {
                                let inner = String::from_utf8_lossy(
                                    &self.src[off(start)..off(close_brace_pos)],
                                )
                                .into_owned();
                                parts.push(StringPart::ComplexExpr(inner, base));
                            }
                        }
                    }
                    None => {
                        let inner =
                            String::from_utf8_lossy(&self.src[off(start)..off(close_brace_pos)])
                                .into_owned();
                        parts.push(StringPart::ComplexExpr(inner, base));
                    }
                }
                continue;
            }

            // `${name}` is `$name`, `${name[expr]}` is `$name[expr]`, and
            // any other `${expr}` the variable `expr` names.
            if b == b'$' && bytes.get(i + 1) == Some(&b'{') {
                flush!();
                let base = self.line_col_at(off(i));
                i += 2;
                let start = i;
                let mut depth = 1;
                while i < bytes.len() && depth > 0 {
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
                let inner = String::from_utf8_lossy(&self.src[off(start)..off(i)]).into_owned();
                i = (i + 1).min(bytes.len());
                let name_len = inner.bytes().take_while(|&c| is_ident_continue(c)).count();
                let starts_with_name = inner.bytes().next().is_some_and(is_ident_start);
                if starts_with_name && name_len == inner.len() {
                    let span = Span {
                        start: base,
                        end: self.line_col_at(off(i)),
                    };
                    parts.push(StringPart::SimpleVar(self.interner.name(&inner), span));
                } else if starts_with_name
                    && inner[name_len..].starts_with('[')
                    && inner.ends_with(']')
                {
                    parts.push(StringPart::ComplexExpr(format!("${inner}"), base));
                } else {
                    parts.push(StringPart::ComplexExpr(format!("${{{inner}}}"), base));
                }
                continue;
            }

            if b == b'$' && bytes.get(i + 1).is_some_and(|&c| is_ident_start(c)) {
                flush!();
                i += 1;
                let name_start = i;
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                let name = self
                    .interner
                    .name(&String::from_utf8_lossy(&bytes[name_start..i]));
                // Points at the `$` itself (one byte before `name_start`),
                // matching where the reconstructed `${name}...` string below
                // begins - precision within the reconstruction (distinguishing
                // `$name` from `->prop`/`[key]`) isn't attempted, only anchoring
                // the whole thing at its real starting position.
                let base = self.line_col_at(off(name_start - 1));

                // `->` or PHP 8's nullsafe `?->`.
                let arrow_len = match (bytes.get(i), bytes.get(i + 1), bytes.get(i + 2)) {
                    (Some(b'-'), Some(b'>'), _) => Some(2),
                    (Some(b'?'), Some(b'-'), Some(b'>')) => Some(3),
                    _ => None,
                };
                if let Some(arrow_len) = arrow_len
                    && bytes.get(i + arrow_len).is_some_and(|&c| is_ident_start(c))
                {
                    let var_end = self.line_col_at(off(i));
                    let nullsafe = arrow_len == 3;
                    i += arrow_len;
                    let prop_start = i;
                    while i < bytes.len() && is_ident_continue(bytes[i]) {
                        i += 1;
                    }
                    let prop = self
                        .interner
                        .name(&String::from_utf8_lossy(&bytes[prop_start..i]));
                    let end = self.line_col_at(off(i));
                    parts.push(StringPart::PropertyAccess {
                        var: name,
                        var_start: base,
                        var_end,
                        prop,
                        nullsafe,
                        end,
                    });
                } else if bytes.get(i) == Some(&b'[') {
                    let var_end = self.line_col_at(off(i));
                    i += 1;
                    let key_start = i;
                    while i < bytes.len() && bytes[i] != b']' {
                        i += 1;
                    }
                    let key = String::from_utf8_lossy(&bytes[key_start..i]).into_owned();
                    i = (i + 1).min(bytes.len());
                    match classify_index_key(&key, &self.interner) {
                        Some(key) => {
                            let end = self.line_col_at(off(i));
                            parts.push(StringPart::Index {
                                var: name,
                                var_start: base,
                                var_end,
                                key,
                                end,
                            });
                        }
                        None => {
                            let key_expr = if key.starts_with('$') || key.parse::<i64>().is_ok() {
                                key
                            } else {
                                format!("'{key}'")
                            };
                            parts.push(StringPart::ComplexExpr(
                                format!("${name}[{key_expr}]"),
                                base,
                            ));
                        }
                    }
                } else {
                    let end = self.line_col_at(off(i));
                    parts.push(StringPart::SimpleVar(name, Span { start: base, end }));
                }
                continue;
            }

            // Batches the contiguous run of plain (non-escape, non-`{$`,
            // non-`$ident`) bytes via `push_str` + `from_utf8_lossy`
            // instead of pushing one byte at a time (which also silently
            // corrupted any multi-byte UTF-8 character in the run -
            // `b as char` reinterprets each raw byte as its own Latin-1
            // codepoint - a real, independently-confirmed bug, not just a
            // perf issue). `from_utf8_lossy`, not `.unwrap()`: real PHP
            // source can contain non-UTF-8 bytes, matching every other
            // byte-range conversion already in this function (`inner`/
            // `name`/`prop`/`key` above, `label` in `lex_heredoc`).
            let run_start = i;
            while i < bytes.len()
                && !(bytes[i] == b'\\' && i + 1 < bytes.len())
                && !(bytes[i] == b'{' && bytes.get(i + 1) == Some(&b'$'))
                && !(bytes[i] == b'$'
                    && bytes
                        .get(i + 1)
                        .is_some_and(|&c| c == b'{' || is_ident_start(c)))
            {
                i += 1;
            }
            if literal.is_empty() {
                literal_start = run_start;
            }
            literal.extend_from_slice(&bytes[run_start..i]);
        }
        flush!();

        if parts.is_empty() {
            let at = self.line_col_at(off(0));
            parts.push(StringPart::Literal(
                PhpString::default(),
                Span { start: at, end: at },
            ));
        }
        parts
    }
}

/// Flexible heredoc (PHP 7.3+): every content line has up to `indent`
/// leading space/tab characters stripped, matching the closing marker's
/// own indentation.
fn strip_heredoc_indent(text: &[u8], indent: usize) -> (Vec<u8>, Vec<usize>) {
    let mut content = Vec::with_capacity(text.len());
    // For each byte of `content`, and one past the end, its offset in
    // `text`.
    let mut offsets = Vec::with_capacity(text.len() + 1);
    let mut line_start = 0;
    for line in text.split(|&b| b == b'\n') {
        let cut = line
            .iter()
            .take(indent)
            .take_while(|&&b| b == b' ' || b == b'\t')
            .count();
        if line_start > 0 {
            content.push(b'\n');
            offsets.push(line_start - 1);
        }
        content.extend_from_slice(&line[cut..]);
        offsets.extend((line_start + cut)..(line_start + line.len()));
        line_start += line.len() + 1;
    }
    offsets.push(text.len());
    (content, offsets)
}

/// A codepoint as PHP's `\u{...}` writes it: UTF-8, a surrogate included
/// (as its three bytes).
fn encode_codepoint(c: u32) -> Vec<u8> {
    match c {
        0..=0x7F => vec![c as u8],
        0x80..=0x7FF => vec![0xC0 | (c >> 6) as u8, 0x80 | (c & 0x3F) as u8],
        0x800..=0xFFFF => vec![
            0xE0 | (c >> 12) as u8,
            0x80 | ((c >> 6) & 0x3F) as u8,
            0x80 | (c & 0x3F) as u8,
        ],
        _ => vec![
            0xF0 | (c >> 18) as u8,
            0x80 | ((c >> 12) & 0x3F) as u8,
            0x80 | ((c >> 6) & 0x3F) as u8,
            0x80 | (c & 0x3F) as u8,
        ],
    }
}
