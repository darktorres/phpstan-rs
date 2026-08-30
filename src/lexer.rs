use crate::token::{Keyword, StringPart, Token};

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    in_php: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src: src.as_bytes(),
            pos: 0,
            in_php: false,
        }
    }

    /// For re-lexing a fragment of PHP that is already known to be code,
    /// e.g. the inside of a `{$...}` string interpolation. Skips inline-HTML
    /// mode entirely.
    pub fn new_in_php(src: &'a str) -> Self {
        Lexer {
            src: src.as_bytes(),
            pos: 0,
            in_php: true,
        }
    }

    pub fn tokenize(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token();
            let done = tok == Token::Eof;
            tokens.push(tok);
            if done {
                break;
            }
        }
        tokens
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
            return self.lex_inline_html();
        }

        self.skip_whitespace_and_comments();

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
            if let Some(kw) = Keyword::from_str(&s) {
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

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    self.pos += 1;
                }
                b'#' => {
                    if self.starts_with("#[PhpStormStubsElementAvailable(") {
                        self.skip_stub_availability_attribute();
                    } else if self.starts_with("#[") {
                        // Attribute like #[Foo(...)] - skip to matching ']'.
                        self.pos += 2;
                        let mut depth = 1;
                        while self.pos < self.src.len() && depth > 0 {
                            match self.peek() {
                                b'[' => depth += 1,
                                b']' => depth -= 1,
                                _ => {}
                            }
                            self.pos += 1;
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
    /// ordinary attribute-only skip. Deliberately conservative about
    /// *where* this attribute appears: if a `{`/`;` turns up before a
    /// parameter delimiter, this isn't a single parameter fragment at all
    /// (most commonly the same attribute gating an entire method, e.g.
    /// `DateTime::__construct`'s 5.5-8.2 overload) - left completely
    /// untouched in that case too.
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
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    self.pos += 1;
                }
            } else {
                self.pos += 1;
            }
        }
        let raw = &self.src[start..self.pos];
        let parts = scan_interpolated(raw);
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
            Token::InterpString(scan_interpolated(content.as_bytes()))
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
            // rare, but keep exponent-assign grouped with the operator table
            self.pos += 3;
            return Token::StarEquals;
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

/// Shared body of double-quoted-string and heredoc interpolation: escapes,
/// `$var` / `$var->prop` / `$var[key]` simple syntax, and `{$expr}` complex
/// syntax. Operates on an already-delimited byte slice (quotes/markers
/// already stripped by the caller).
fn scan_interpolated(bytes: &[u8]) -> Vec<StringPart> {
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
                b'n' => literal.push('\n'),
                b't' => literal.push('\t'),
                b'r' => literal.push('\r'),
                b'"' => literal.push('"'),
                b'\\' => literal.push('\\'),
                b'$' => literal.push('$'),
                other => {
                    literal.push('\\');
                    literal.push(other as char);
                }
            }
            i += 2;
            continue;
        }

        if b == b'{' && bytes.get(i + 1) == Some(&b'$') {
            flush!();
            i += 1;
            let start = i;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
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
            i = (i + 1).min(bytes.len());
            parts.push(StringPart::ComplexExpr(inner));
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
                parts.push(StringPart::ComplexExpr(format!("${name}->{prop}")));
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
                parts.push(StringPart::ComplexExpr(format!("${name}[{key_expr}]")));
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
    fn closed_range_stub_attribute_gating_a_whole_method_is_left_untouched() {
        let src = "<?php class C { #[PhpStormStubsElementAvailable(from: '5.5', to: '8.2')]\n\
                   public function __construct($datetime = \"now\") {} }";
        let with_attr = Lexer::new(src).tokenize();
        let without_attr =
            Lexer::new("<?php class C { public function __construct($datetime = \"now\") {} }")
                .tokenize();
        assert_eq!(with_attr, without_attr);
    }
}
