use crate::lexer::{Lexer, is_ident_continue, is_ident_start, skip_string_literal_bytes};
use crate::token::{StringPart, Token};

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
        let mut s = String::new();
        let mut run_start = self.pos;
        while self.pos < self.src.len() && self.peek() != b'\'' {
            if self.peek() == b'\\' && (self.peek_at(1) == b'\'' || self.peek_at(1) == b'\\') {
                s.push_str(&String::from_utf8_lossy(&self.src[run_start..self.pos]));
                s.push(self.peek_at(1) as char);
                self.pos += 2;
                run_start = self.pos;
            } else {
                self.pos += 1;
            }
        }
        s.push_str(&String::from_utf8_lossy(&self.src[run_start..self.pos]));
        self.pos += 1; // closing '
        Token::StringLit(s)
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
        // Upper-bound capacity hint - avoids repeated reallocation while
        // pushing long plain-text runs (SQL, translation strings, HTML in
        // a heredoc, ...).
        let mut literal = String::with_capacity(bytes.len());
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
            // Found and fixed via a perf-optimization investigation
            // workflow.
            let run_start = i;
            while i < bytes.len()
                && !(bytes[i] == b'\\' && i + 1 < bytes.len())
                && !(bytes[i] == b'{' && bytes.get(i + 1) == Some(&b'$'))
                && !(bytes[i] == b'$' && bytes.get(i + 1).is_some_and(|&c| is_ident_start(c)))
            {
                i += 1;
            }
            literal.push_str(&String::from_utf8_lossy(&bytes[run_start..i]));
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
