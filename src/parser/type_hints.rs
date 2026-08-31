use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Token};

impl Parser {
    // ---- type hints ----

    pub(crate) fn is_type_start(&self) -> bool {
        self.is_type_start_at(0)
    }

    pub(crate) fn is_type_start_at(&self, offset: usize) -> bool {
        matches!(
            self.peek_at(offset),
            Token::Question
                | Token::Identifier(_)
                | Token::Backslash
                | Token::LParen
                | Token::Keyword(Keyword::Namespace)
                | Token::Keyword(Keyword::Array)
                | Token::Keyword(Keyword::Static)
                | Token::Keyword(Keyword::Null)
                | Token::Keyword(Keyword::True)
                | Token::Keyword(Keyword::False)
                // `enum` is only reserved as a declaration keyword at
                // statement position (see `looks_like_class_decl`'s own
                // sibling check for it there) - as a type hint it's just
                // an ordinary, if unusually-named, class reference (real,
                // valid PHP: a class literally named `Enum`, distinct
                // from an `enum` *declaration*). Falls through to
                // `parse_type_atom`'s final `parse_qualified_name` call,
                // which already accepts any keyword as a name segment via
                // `take_name_segment` - no separate arm needed there.
                | Token::Keyword(Keyword::Enum)
        )
    }

    // Consumes one qualified type name, e.g. `int`, `array`, `\App\Foo`, or
    // a PHP 8.2 DNF (disjunctive normal form) parenthesized intersection
    // group, e.g. `(Countable&Iterator)` as one term of a union type like
    // `(Countable&Iterator)|null`. Real PHP only allows one level of
    // parens here (no `((A&B)|C)&D`), so this doesn't need to recurse into
    // a further union inside the parens - just a run of `&`-joined atoms.
    pub(crate) fn parse_type_atom(&mut self) -> Result<String, ParseError> {
        if self.eat(&Token::LParen) {
            let mut inner = self.parse_type_atom()?;
            while self.eat(&Token::Amp) {
                inner.push('&');
                inner.push_str(&self.parse_type_atom()?);
            }
            self.expect(&Token::RParen)?;
            return Ok(format!("({inner})"));
        }
        // These are single keyword tokens - namespaced forms like
        // `array\Foo` don't exist, so each just consumes itself and stops,
        // rather than looping on to swallow whatever bare identifier
        // happens to follow (e.g. `array $items`'s `$items`, or
        // `const array NAME`'s own `NAME`).
        match self.peek().clone() {
            Token::Keyword(Keyword::Array) => {
                self.advance();
                return Ok("array".to_string());
            }
            Token::Keyword(Keyword::Static) => {
                self.advance();
                return Ok("static".to_string());
            }
            Token::Keyword(Keyword::Null) => {
                self.advance();
                return Ok("null".to_string());
            }
            // PHP 8.2+ stand-alone `true`/`false` types, e.g.
            // `function f(): string|false {}`.
            Token::Keyword(Keyword::True) => {
                self.advance();
                return Ok("true".to_string());
            }
            Token::Keyword(Keyword::False) => {
                self.advance();
                return Ok("false".to_string());
            }
            _ => {}
        }
        // Otherwise it's a (possibly namespaced) identifier: `int`, `Foo`,
        // `\App\Foo`, `App\Foo\Bar` - never two unrelated identifiers
        // glued together with no backslash between them.
        Ok(self.parse_qualified_name())
    }

    pub(crate) fn parse_type_hint(&mut self) -> Result<Option<String>, ParseError> {
        if !self.is_type_start() {
            return Ok(None);
        }
        let mut s = String::new();
        if self.eat(&Token::Question) {
            s.push('?');
        }
        s.push_str(&self.parse_type_atom()?);
        loop {
            if self.check(&Token::Pipe) {
                self.advance();
                s.push('|');
                s.push_str(&self.parse_type_atom()?);
                continue;
            }
            // `&` is ambiguous between an intersection type (`A&B`) and a
            // by-ref marker before a param's `$name` - only treat it as part
            // of the type when another type atom actually follows.
            if self.check(&Token::Amp) && self.is_type_start_at(1) {
                self.advance();
                s.push('&');
                s.push_str(&self.parse_type_atom()?);
                continue;
            }
            break;
        }
        Ok(Some(s))
    }
}
