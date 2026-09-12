use crate::ast::Attribute;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Span, Token};

impl Parser {
    // ---- token stream helpers ----

    pub(crate) fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    pub(crate) fn peek_at(&self, offset: usize) -> &Token {
        self.tokens.get(self.pos + offset).unwrap_or(&Token::Eof)
    }

    /// Scans forward (without consuming) through any of `abstract`/
    /// `final`/`readonly` to see if a `class` keyword eventually follows -
    /// the only thing that actually confirms a class declaration is
    /// starting, since all three are also legal as ordinary
    /// function/method/constant names.
    pub(crate) fn looks_like_class_decl(&self) -> bool {
        let mut offset = 0;
        while matches!(
            self.peek_at(offset),
            Token::Keyword(Keyword::Abstract | Keyword::Final | Keyword::Readonly)
        ) {
            offset += 1;
        }
        matches!(self.peek_at(offset), Token::Keyword(Keyword::Class))
    }

    /// Called with the current token on `function` - does a real
    /// declaration name follow (skipping an optional by-ref-return `&`),
    /// as opposed to `(` directly (an anonymous function expression, only
    /// valid as a statement when written bare and unassigned)? `&` itself
    /// is never a valid function name, so no further lookahead is needed
    /// once it's skipped.
    pub(crate) fn looks_like_function_decl(&self) -> bool {
        let offset = if matches!(self.peek_at(1), Token::Amp) {
            2
        } else {
            1
        };
        !matches!(self.peek_at(offset), Token::LParen)
    }

    /// Is the current position a `(set)` asymmetric-visibility marker
    /// (`private(set)`, `public(set)`, `protected(set)`)? Checked at the
    /// visibility keyword itself, i.e. this looks 3 tokens ahead for the
    /// literal `( set )`. Needed to tell it apart from a visibility
    /// keyword immediately followed by an unrelated `(` for some other
    /// reason - most notably `public (A&B)|X $prop;`, a DNF type hint
    /// that also happens to start with `(`.
    pub(crate) fn peek_is_set_visibility_marker(&self) -> bool {
        matches!(self.peek_at(1), Token::LParen)
            && matches!(self.peek_at(2), Token::Identifier(s) if s == "set")
            && matches!(self.peek_at(3), Token::RParen)
    }

    /// Scans forward from the current `(` (without consuming anything) for
    /// a comma at that paren's own nesting level before its matching `)` -
    /// used to tell PHP 8.5's two-argument `clone(obj, overrides)` form
    /// apart from a plain `clone(expr)` (the classic unary form, spelled
    /// with parens around its operand).
    pub(crate) fn clone_call_has_top_level_comma(&self) -> bool {
        // A named argument (`clone(object: $x)`) or a leading spread
        // (`clone(...$args)`, as opposed to the bare `clone(...)`
        // first-class-callable shape already ruled out by the caller)
        // also commits to the call-args form even with no comma anywhere -
        // both checked first since they're fixed-width lookaheads right
        // after the `(`.
        if matches!(self.peek_at(1), Token::Identifier(_) | Token::Keyword(_))
            && matches!(self.peek_at(2), Token::Colon)
        {
            return true;
        }
        if matches!(self.peek_at(1), Token::Ellipsis) {
            return true;
        }
        let mut depth = 0i32;
        let mut offset = 0usize;
        loop {
            match self.peek_at(offset) {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                Token::Comma if depth == 1 => return true,
                Token::Eof => return false,
                _ => {}
            }
            offset += 1;
        }
    }

    pub(crate) fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    /// Advances past the current token without materializing it - the
    /// large majority (126 of 128) of real call sites across the parser
    /// discard `advance()`'s own return value entirely, paying a wasted
    /// `Token::clone()` (a real heap allocation for 7 of `Token`'s
    /// variants: `Identifier`/`Variable`/`StringLit`/`InterpString`/
    /// `ShellExec`/`DocComment`/`InlineHtml`) for nothing. Found and
    /// fixed via a perf-optimization investigation workflow; the 2 real
    /// consumers (`expect_variable` just below, `classes.rs`'s own
    /// `parse_class_like_decl`) use `take_token()` instead.
    pub(crate) fn skip(&mut self) {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
    }

    /// `advance()`'s own real-consumer counterpart - moves the current
    /// token out via `mem::replace` (leaving an inert `Token::Eof`
    /// placeholder behind) instead of cloning it. Bounds-guarded the same
    /// way `advance()`/`skip()` always were, to preserve the identical
    /// graceful-EOF fallback rather than panicking on an out-of-range
    /// index. Sound to leave a placeholder behind: the only place
    /// `self.tokens` is ever indexed by something other than `self.pos`
    /// (`control_flow.rs`'s `elseif`-rewrite backtrack) unconditionally
    /// overwrites that slot before it's ever read again.
    pub(crate) fn take_token(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            let t = std::mem::replace(&mut self.tokens[self.pos], Token::Eof);
            self.pos += 1;
            t
        } else {
            Token::Eof
        }
    }

    pub(crate) fn check(&self, t: &Token) -> bool {
        self.peek() == t
    }

    pub(crate) fn eat(&mut self, t: &Token) -> bool {
        if self.check(t) {
            self.skip();
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, t: &Token) -> Result<(), ParseError> {
        if !self.eat(t) {
            return Err(ParseError {
                message: format!(
                    "parse error: expected {:?}, found {:?} at token {}",
                    t,
                    self.peek(),
                    self.pos
                ),
                span: self.spans.get(self.pos).copied().unwrap_or_default(),
            });
        }
        Ok(())
    }

    pub(crate) fn expect_semicolon(&mut self) -> Result<(), ParseError> {
        // A `?>` close tag lexes as a Semicolon already; plain EOF also
        // terminates the last statement without one.
        if self.at_eof() {
            return Ok(());
        }
        self.expect(&Token::Semicolon)
    }

    /// Used for every "name in declaration position" (function/class/
    /// const/case names, ...). Keyword-tolerant for the same reason
    /// take_name_segment is: PHP reserves most keywords only in specific
    /// declaration positions, not universally - `const DEFAULT = 1;` is
    /// valid PHP even though `default` is a keyword.
    pub(crate) fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Token::Identifier(_) | Token::Keyword(_) => Ok(self.take_name_segment().unwrap()),
            other => {
                let message = format!("parse error: expected identifier, found {:?}", other);
                Err(ParseError {
                    message,
                    span: self.spans.get(self.pos).copied().unwrap_or_default(),
                })
            }
        }
    }

    pub(crate) fn expect_variable(&mut self) -> Result<String, ParseError> {
        let span = self.spans.get(self.pos).copied().unwrap_or_default();
        match self.take_token() {
            Token::Variable(s) => Ok(s),
            other => Err(ParseError {
                message: format!("parse error: expected variable, found {:?}", other),
                span,
            }),
        }
    }

    // A qualified name is one identifier, optionally namespaced by
    // backslash-separated segments (`Foo`, `\App\Foo`, `App\Foo\Bar`).
    // Consecutive bare identifiers with no backslash between them are two
    // separate tokens, not one name - `array BASE_SECTIONS` must not
    // collapse into "arrayBASE_SECTIONS", which silently swallowed a
    // constant's own name until this was fixed.
    /// Consumes one identifier *or* keyword token as a name segment - most
    /// keywords are only reserved in declaration position in real PHP
    /// ("semi-reserved words"), so `Enum`/`Match`/`New`/... are all valid
    /// namespace segments (`Piwigo\Common\Enum\Foo`).
    pub(crate) fn take_name_segment(&mut self) -> Option<String> {
        match self.peek() {
            Token::Identifier(_) => {
                let Token::Identifier(part) = self.take_token() else {
                    unreachable!()
                };
                Some(part)
            }
            Token::Keyword(_) => {
                let idx = self.pos;
                let Token::Keyword(kw) = self.take_token() else {
                    unreachable!()
                };
                // Phase 48 follow-up (item 6): a real vendored class can
                // be literally named the same as a PHP keyword
                // (`marc-mabe/php-enum`'s own `MabeEnum\Enum`) - use the
                // original source spelling when we have it, falling back
                // to the keyword's own canonical (always-lowercase) form
                // only if it's somehow missing (shouldn't happen for a
                // real lexer-produced token stream).
                Some(
                    self.keyword_raw
                        .get(idx)
                        .cloned()
                        .flatten()
                        .unwrap_or_else(|| kw.as_str().to_string()),
                )
            }
            _ => None,
        }
    }

    pub(crate) fn parse_qualified_name(&mut self) -> String {
        let mut s = String::new();
        if self.eat(&Token::Backslash) {
            s.push('\\');
        }
        match self.take_name_segment() {
            Some(part) => s.push_str(&part),
            None => return s,
        }
        while matches!(self.peek(), Token::Backslash) {
            self.skip();
            s.push('\\');
            match self.take_name_segment() {
                Some(part) => s.push_str(&part),
                None => break,
            }
        }
        s
    }

    /// Phase 17: `parse_qualified_name`, plus the span it occupied - kept
    /// as a thin wrapper rather than changing `parse_qualified_name`'s own
    /// signature, which has ~30 existing call sites that have no use for
    /// the span. Used only at the smaller set of call sites (`extends`/
    /// `implements`/`uses`/`instanceof`/`catch`) that feed the LSP's own
    /// declaration-location index or position-walker.
    pub(crate) fn parse_qualified_name_spanned(&mut self) -> (String, Span) {
        let start = self.current_pos();
        let name = self.parse_qualified_name();
        let span = Span {
            start,
            end: self.previous_end(),
        };
        (name, span)
    }

    /// Phase 37: parses zero or more consecutive `#[Name(args), ...]`
    /// attribute groups sitting at the current position - real PHP allows
    /// several bracket groups back-to-back on one declaration
    /// (`#[A] #[B(1)]`, equivalent to the single group `#[A, B(1)]`), so
    /// this loops rather than parsing just one. Returns an empty `Vec`
    /// without consuming anything when the current token isn't
    /// `AttributeStart` at all - every call site can call this
    /// unconditionally rather than checking first.
    pub(crate) fn parse_attributes(&mut self) -> Result<Vec<Attribute>, ParseError> {
        let mut attrs = Vec::new();
        while self.check(&Token::AttributeStart) {
            self.skip();
            loop {
                let attr_start = self.current_pos();
                let name = self.parse_qualified_name();
                let args = if self.check(&Token::LParen) {
                    self.parse_args()?.0
                } else {
                    Vec::new()
                };
                attrs.push(Attribute {
                    name,
                    args,
                    span: Span {
                        start: attr_start,
                        end: self.previous_end(),
                    },
                });
                // A trailing comma before the closing `]` is valid PHP
                // (`#[A, B,]`), same as a call's own trailing comma.
                if !self.eat(&Token::Comma) || self.check(&Token::RBracket) {
                    break;
                }
            }
            self.expect(&Token::RBracket)?;
        }
        Ok(attrs)
    }

    /// Same as `parse_attributes`, but also preserves a doc comment
    /// recorded as immediately preceding the attribute group(s) - a doc
    /// comment normally attaches to whatever real token comes right after
    /// it (`self.doc_comments` is keyed by that token's index), which
    /// before this phase was always the declaration itself. With a real
    /// `#[...]` group now sitting between the two
    /// (`/** doc */ #[Attr] function f() {}`), the doc comment is keyed to
    /// the attribute group's own position instead - re-keying it here, to
    /// wherever parsing the attributes leaves `self.pos`, is what lets the
    /// declaration parser that follows (which looks up its own doc comment
    /// via `take_doc_comment()`, unaware any attributes were even there)
    /// keep finding it unchanged.
    pub(crate) fn parse_leading_attributes(&mut self) -> Result<Vec<Attribute>, ParseError> {
        if !self.check(&Token::AttributeStart) {
            return Ok(Vec::new());
        }
        let doc = self.doc_comments.remove(&self.pos);
        let attrs = self.parse_attributes()?;
        if let Some(doc) = doc {
            self.doc_comments.insert(self.pos, doc);
        }
        Ok(attrs)
    }
}
