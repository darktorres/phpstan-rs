use syntax::token::{Position, Span};

/// PHPDoc's own token set - deliberately much smaller than the full PHP
/// `Lexer`/`Token` this project already has (`src/lexer.rs`/`src/
/// token.rs`): a docblock's content is type expressions and a handful of
/// tag-structure punctuators, never PHP statements/expressions in their
/// own right.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PhpDocToken {
    At,
    /// A bare word, including real PHPStan's own hyphenated compound
    /// keywords (`class-string`, `non-empty-array`, `positive-int`, ...)
    /// and namespaced class names (`Foo\Bar`) - see `lex_identifier`'s
    /// own doc comment for the exact grammar this accepts.
    Identifier(String),
    /// `$name` (the `$` is not part of the string).
    Variable(String),
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    Pipe,
    Amp,
    Question,
    LAngle,
    RAngle,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    ColonColon,
    Equals,
    Ellipsis,
    Star,
    /// Anything this lexer doesn't recognize at all (a stray `%`, `!`,
    /// ...) - carried through as its own token (rather than silently
    /// dropped) so the parser can turn encountering one into a real,
    /// precisely-located `PhpDocParseError`.
    Unknown(char),
    Eof,
}

pub(crate) struct Lexer<'a> {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
    _src: &'a str,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(src: &'a str) -> Self {
        Lexer {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            _src: src,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn position(&self) -> Position {
        Position {
            line: self.line,
            col: self.col,
        }
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    /// Whitespace, and a `//` comment running to the end of its line -
    /// phpdoc-parser's own `TOKEN_COMMENT`, which its type parser skips
    /// (a wrapped array shape's `// description` after a field). The tag
    /// text itself keeps the comment: only the token stream drops it.
    fn skip_whitespace(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.advance();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    while !matches!(self.peek(), None | Some('\n' | '\r')) {
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    /// A name segment - `[A-Za-z_\x80-\xff][A-Za-z0-9_\x80-\xff]*`,
    /// mirroring the real PHP identifier grammar this project's own main
    /// `Lexer` already uses (any non-ASCII byte counts as a legal
    /// identifier character, matching PHP's own locale-agnostic
    /// treatment) - joined by `\` for a namespaced name, and by `-` for
    /// real PHPStan's own hyphenated compound keywords (`class-string`,
    /// `non-empty-array`, ...). A trailing `-` is never consumed unless
    /// immediately followed by another identifier-start character - this
    /// is what keeps `Foo-` (a genuinely stray trailing hyphen) and a
    /// negative-literal minus (`-1`) both correctly *not* absorbed into
    /// the identifier.
    fn lex_identifier(&mut self) -> String {
        let mut s = String::new();
        loop {
            while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_' || c == '\\') {
                s.push(self.advance().unwrap());
            }
            if self.peek() == Some('-')
                && matches!(self.peek_at(1), Some(c) if c.is_alphabetic() || c == '_')
            {
                s.push(self.advance().unwrap());
                continue;
            }
            break;
        }
        s
    }

    fn lex_number(&mut self) -> PhpDocToken {
        let mut s = String::new();
        if self.peek() == Some('-') {
            s.push(self.advance().unwrap());
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            s.push(self.advance().unwrap());
        }
        let mut is_float = false;
        if self.peek() == Some('.') && matches!(self.peek_at(1), Some(c) if c.is_ascii_digit()) {
            is_float = true;
            s.push(self.advance().unwrap());
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                s.push(self.advance().unwrap());
            }
        }
        if is_float {
            PhpDocToken::FloatLiteral(s.parse().unwrap_or(0.0))
        } else {
            PhpDocToken::IntLiteral(s.parse().unwrap_or(0))
        }
    }

    fn lex_string_literal(&mut self, quote: char) -> PhpDocToken {
        self.advance();
        let mut s = String::new();
        loop {
            match self.peek() {
                None => break,
                Some(c) if c == quote => {
                    self.advance();
                    break;
                }
                Some('\\') if self.peek_at(1) == Some(quote) => {
                    self.advance();
                    s.push(self.advance().unwrap());
                }
                Some(_) => s.push(self.advance().unwrap()),
            }
        }
        PhpDocToken::StringLiteral(s)
    }

    /// The whole input, tokenized up front (a docblock's content is
    /// small - never worth a lazy/streaming lexer the way a real source
    /// file is).
    pub(crate) fn tokenize(mut self) -> Vec<(PhpDocToken, Span)> {
        let mut out = Vec::new();
        loop {
            self.skip_whitespace();
            let start = self.position();
            let Some(c) = self.peek() else {
                out.push((PhpDocToken::Eof, Span { start, end: start }));
                break;
            };
            let token = match c {
                '@' => {
                    self.advance();
                    PhpDocToken::At
                }
                '$' => {
                    self.advance();
                    let name = self.lex_identifier();
                    // A `$` not followed by a name (`$$x`) is no variable.
                    if name.is_empty() {
                        PhpDocToken::Unknown('$')
                    } else {
                        PhpDocToken::Variable(name)
                    }
                }
                '|' => {
                    self.advance();
                    PhpDocToken::Pipe
                }
                '&' => {
                    self.advance();
                    PhpDocToken::Amp
                }
                '?' => {
                    self.advance();
                    PhpDocToken::Question
                }
                '<' => {
                    self.advance();
                    PhpDocToken::LAngle
                }
                '>' => {
                    self.advance();
                    PhpDocToken::RAngle
                }
                '{' => {
                    self.advance();
                    PhpDocToken::LBrace
                }
                '}' => {
                    self.advance();
                    PhpDocToken::RBrace
                }
                '(' => {
                    self.advance();
                    PhpDocToken::LParen
                }
                ')' => {
                    self.advance();
                    PhpDocToken::RParen
                }
                '[' => {
                    self.advance();
                    PhpDocToken::LBracket
                }
                ']' => {
                    self.advance();
                    PhpDocToken::RBracket
                }
                ',' => {
                    self.advance();
                    PhpDocToken::Comma
                }
                '=' => {
                    self.advance();
                    PhpDocToken::Equals
                }
                '*' => {
                    self.advance();
                    PhpDocToken::Star
                }
                ':' => {
                    self.advance();
                    if self.peek() == Some(':') {
                        self.advance();
                        PhpDocToken::ColonColon
                    } else {
                        PhpDocToken::Colon
                    }
                }
                '.' if self.peek_at(1) == Some('.') && self.peek_at(2) == Some('.') => {
                    self.advance();
                    self.advance();
                    self.advance();
                    PhpDocToken::Ellipsis
                }
                '\'' | '"' => self.lex_string_literal(c),
                '-' if matches!(self.peek_at(1), Some(d) if d.is_ascii_digit()) => {
                    self.lex_number()
                }
                c if c.is_ascii_digit() => self.lex_number(),
                c if c.is_alphabetic() || c == '_' || c == '\\' => {
                    PhpDocToken::Identifier(self.lex_identifier())
                }
                other => {
                    self.advance();
                    PhpDocToken::Unknown(other)
                }
            };
            let end = self.position();
            out.push((token, Span { start, end }));
        }
        out
    }
}
