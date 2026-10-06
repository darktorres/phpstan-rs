use crate::ast::{Attribute, Ident, QualifiedName};
use crate::lexer::{DocGroup, Tok};
use crate::name::Name;
use crate::parser::{NameKind, ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser<'_> {
    pub(crate) fn name_id(&self, name: &Name) -> Option<crate::name::NameId> {
        match &*self.supply.borrow() {
            crate::parser::Supply::Lexer(lexer) => lexer.name_id(name),
            crate::parser::Supply::List(_) => None,
        }
    }

    // ---- token stream helpers ----

    #[inline]
    pub(crate) fn peek(&self) -> &Token {
        self.ahead.front().map_or(&Token::Eof, |tok| &tok.token)
    }

    /// The token `offset` places past the current one, for an `offset` of
    /// at most `LOOKAHEAD`: `Token::Eof` past the end of the file.
    pub(crate) fn peek_at(&self, offset: usize) -> &Token {
        self.ahead.get(offset).map_or(&Token::Eof, |tok| &tok.token)
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
            && matches!(self.peek_at(2), Token::Identifier(s) if s.eq_ignore_ascii_case("set"))
            && matches!(self.peek_at(3), Token::RParen)
    }

    /// Scans forward from the current `(` (without consuming anything) for
    /// a comma at that paren's own nesting level before its matching `)` -
    /// used to tell PHP 8.5's two-argument `clone(obj, overrides)` form
    /// apart from a plain `clone(expr)` (the classic unary form, spelled
    /// with parens around its operand).
    pub(crate) fn clone_call_has_top_level_comma(&mut self) -> bool {
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
            self.fill(offset);
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

    pub fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    /// Advances past the current token without materializing it; a
    /// caller that needs the token moves it out with `take_token()`.
    pub(crate) fn skip(&mut self) {
        self.pop();
    }

    /// Moves the current token out, falling back to EOF past the end.
    pub(crate) fn take_token(&mut self) -> Token {
        self.pop().map_or(Token::Eof, |tok| tok.token)
    }

    /// Whether the next token is `t`. Most checks find another kind of
    /// token, which the kinds alone settle.
    #[inline]
    pub(crate) fn check(&self, t: &Token) -> bool {
        let next = self.peek();
        std::mem::discriminant(next) == std::mem::discriminant(t) && next == t
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
                span: self.current_span(),
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
    pub(crate) fn expect_ident(&mut self) -> Result<Name, ParseError> {
        match self.peek() {
            Token::Identifier(_) | Token::Keyword(_) => Ok(self.take_name_segment().unwrap()),
            other => {
                let message = format!("parse error: expected identifier, found {:?}", other);
                Err(ParseError {
                    message,
                    span: self.current_span(),
                })
            }
        }
    }

    /// `expect_ident`, with the name's span.
    pub(crate) fn expect_ident_spanned(&mut self) -> Result<Ident, ParseError> {
        let start = self.current_pos();
        let name = self.expect_ident()?;
        Ok(self.ident_from(&name, start))
    }

    /// `expect_variable`, with the variable's span (its `$` included).
    pub(crate) fn expect_variable_spanned(&mut self) -> Result<Ident, ParseError> {
        let start = self.current_pos();
        let name = self.expect_variable()?;
        Ok(self.ident_from(&name, start))
    }

    fn ident_from(&self, name: &str, start: u32) -> Ident {
        self.ident(
            self.intern(name),
            Span {
                start,
                end: self.previous_end(),
            },
        )
    }

    pub(crate) fn ident(&self, name: Name, span: Span) -> Ident {
        let name_id = self.name_id(&name);
        Ident {
            name,
            name_id,
            span,
        }
    }

    pub(crate) fn expect_variable(&mut self) -> Result<Name, ParseError> {
        let span = self.current_span();
        match self.take_token() {
            Token::Variable(s) => Ok(s),
            other => Err(ParseError {
                message: format!("parse error: expected variable, found {:?}", other),
                span,
            }),
        }
    }

    /// `take_name_segment`, with the name's span.
    pub(crate) fn take_ident(&mut self) -> Option<Ident> {
        let start = self.current_pos();
        let name = self.take_name_segment()?;
        Some(self.ident(
            name,
            Span {
                start,
                end: self.previous_end(),
            },
        ))
    }

    /// Consumes one identifier *or* keyword token as a name segment - most
    /// keywords are only reserved in declaration position in real PHP
    /// ("semi-reserved words"), so `Enum`/`Match`/`New`/... are all valid
    /// namespace segments (`Piwigo\Common\Enum\Foo`).
    /// An `Identifier` token's text is moved out already interned; a
    /// keyword's source spelling is interned here.
    pub(crate) fn take_name_segment(&mut self) -> Option<Name> {
        match self.peek() {
            Token::Identifier(_) => {
                let Token::Identifier(part) = self.take_token() else {
                    unreachable!()
                };
                Some(part)
            }
            Token::Keyword(_) => {
                let Some(Tok {
                    token: Token::Keyword(kw),
                    raw_keyword,
                    ..
                }) = self.pop()
                else {
                    unreachable!()
                };
                // A real vendored class can be literally named the same
                // as a PHP keyword - use the original source spelling
                // when we have it, falling back to the keyword's own
                // canonical (always-lowercase) form only if it's somehow
                // missing (shouldn't happen for a real lexer-produced
                // token stream).
                Some(self.intern(raw_keyword.as_deref().unwrap_or_else(|| kw.as_str())))
            }
            _ => None,
        }
    }

    /// A qualified name: one identifier, optionally namespaced by
    /// backslash-separated segments (`Foo`, `\App\Foo`, `App\Foo\Bar`).
    /// Two bare identifiers with no backslash between them are two names,
    /// not one: `array BASE_SECTIONS` isn't "arrayBASE_SECTIONS".
    pub(crate) fn parse_qualified_name(&mut self) -> Name {
        let leading_backslash = self.eat(&Token::Backslash);
        let Some(first) = self.take_name_segment() else {
            return self
                .interner
                .name(if leading_backslash { "\\" } else { "" });
        };
        // A single-segment name is its token's own name.
        if !leading_backslash && !matches!(self.peek(), Token::Backslash) {
            return first;
        }
        let mut s = String::new();
        if leading_backslash {
            s.push('\\');
        }
        s.push_str(&first);
        while matches!(self.peek(), Token::Backslash) {
            self.skip();
            s.push('\\');
            match self.take_name_segment() {
                Some(part) => s.push_str(&part),
                None => break,
            }
        }
        self.interner.name(&s)
    }

    /// A class, function or constant reference: the name and its span.
    pub(crate) fn parse_name_reference(&mut self) -> QualifiedName {
        let start = self.current_pos();
        let name = self.parse_qualified_name();
        self.qualified_name(name, self.span_from(start), NameKind::Class)
    }

    /// `parse_name_reference`, for a name that is only known to be a
    /// reference by what follows it: `record_name` records it then.
    pub(crate) fn parse_unrecorded_name_reference(&mut self) -> QualifiedName {
        let start = self.current_pos();
        let name = self.parse_qualified_name();
        self.unrecorded_name(name, self.span_from(start), NameKind::Class)
    }

    /// `parse_qualified_name`, plus the span it occupied, for a name that
    /// isn't a reference (a `use` import's path).
    pub(crate) fn parse_qualified_name_spanned(&mut self) -> (Name, Span) {
        let start = self.current_pos();
        let name = self.parse_qualified_name();
        let span = Span {
            start,
            end: self.previous_end(),
        };
        (name, span)
    }

    /// Parses zero or more consecutive `#[Name(args), ...]` attribute
    /// groups sitting at the current position - real PHP allows
    /// several bracket groups back-to-back on one declaration
    /// (`#[A] #[B(1)]`, equivalent to the single group `#[A, B(1)]`), so
    /// this loops rather than parsing just one. Returns an empty `Vec`
    /// without consuming anything when the current token isn't
    /// `AttributeStart` at all - every call site can call this
    /// unconditionally rather than checking first.
    pub(crate) fn parse_attributes(&mut self) -> Result<Vec<Attribute>, ParseError> {
        let mut attrs = Vec::new();
        while self.check(&Token::AttributeStart) {
            let group_start = self.current_pos();
            self.skip();
            let first_in_group = attrs.len();
            loop {
                let attr_start = self.current_pos();
                let name = self.parse_name_reference();
                let args = if self.check(&Token::LParen) {
                    self.parse_args()?.0
                } else {
                    Vec::new()
                };
                let span = Span {
                    start: attr_start,
                    end: self.previous_end(),
                };
                attrs.push(Attribute {
                    name,
                    args,
                    span,
                    removal_span: span,
                });
                // A trailing comma before the closing `]` is valid PHP
                // (`#[A, B,]`), same as a call's own trailing comma.
                if !self.eat(&Token::Comma) || self.check(&Token::RBracket) {
                    break;
                }
            }
            self.expect(&Token::RBracket)?;
            let after_group = self.current_pos();
            let group = &mut attrs[first_in_group..];
            if let [only] = group {
                only.removal_span = Span {
                    start: group_start,
                    end: after_group,
                };
            } else {
                let starts: Vec<u32> = group.iter().map(|a| a.span.start).collect();
                let last = group.len() - 1;
                let previous_end = group[last - 1].span.end;
                for (i, attr) in group.iter_mut().enumerate() {
                    attr.removal_span = if i < last {
                        Span {
                            start: attr.span.start,
                            end: starts[i + 1],
                        }
                    } else {
                        Span {
                            start: previous_end,
                            end: attr.span.end,
                        }
                    };
                }
            }
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
        let doc = self.take_doc_comment();
        let attrs = self.parse_attributes()?;
        if let Some(doc) = doc
            && let Some(front) = self.ahead.front_mut()
        {
            front
                .docs
                .get_or_insert_with(|| {
                    Box::new(DocGroup {
                        last: None,
                        folded: None,
                        first_start: None,
                    })
                })
                .last = Some(doc);
        }
        Ok(attrs)
    }
}
