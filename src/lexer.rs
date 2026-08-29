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
                    if self.starts_with("#[") {
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

        while self.peek().is_ascii_digit() || self.peek() == b'_' {
            self.pos += 1;
        }
        if self.peek() == b'.' && self.peek_at(1).is_ascii_digit() {
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
        self.pos += 1; // opening "
        let mut parts: Vec<StringPart> = Vec::new();
        let mut literal = String::new();

        macro_rules! flush {
            () => {
                if !literal.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                }
            };
        }

        while self.pos < self.src.len() && self.peek() != b'"' {
            if self.peek() == b'\\' {
                let esc = self.peek_at(1);
                match esc {
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
                self.pos += 2;
                continue;
            }

            if self.peek() == b'{' && self.peek_at(1) == b'$' {
                flush!();
                self.pos += 1; // consume '{'
                let start = self.pos;
                let mut depth = 1;
                while self.pos < self.src.len() && depth > 0 {
                    match self.peek() {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        self.pos += 1;
                    }
                }
                let inner = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
                self.pos += 1; // consume '}'
                parts.push(StringPart::ComplexExpr(inner));
                continue;
            }

            if self.peek() == b'$' && is_ident_start(self.peek_at(1)) {
                flush!();
                self.pos += 1;
                let name = self.lex_ident_raw();

                if self.peek() == b'-' && self.peek_at(1) == b'>' && is_ident_start(self.peek_at(2))
                {
                    self.pos += 2;
                    let prop = self.lex_ident_raw();
                    parts.push(StringPart::ComplexExpr(format!("${name}->{prop}")));
                } else if self.peek() == b'[' {
                    self.pos += 1;
                    let key_start = self.pos;
                    while self.pos < self.src.len() && self.peek() != b']' {
                        self.pos += 1;
                    }
                    let key = String::from_utf8_lossy(&self.src[key_start..self.pos]).into_owned();
                    self.pos = (self.pos + 1).min(self.src.len()); // consume ']'
                    // PHP's "simple syntax" treats a bare word key as a
                    // string literal and a numeric key as an int; only a
                    // leading `$` makes it a variable lookup.
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

            literal.push(self.bump() as char);
        }
        flush!();
        self.pos += 1; // closing "

        if parts.is_empty() {
            parts.push(StringPart::Literal(String::new()));
        }
        Token::InterpString(parts)
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
