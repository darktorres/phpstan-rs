use crate::ast::{NativeType, TypeHint, TypeHintKind};
use crate::parser::{NameKind, ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, TokenRef as Token};

impl Parser<'_> {
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
    pub(crate) fn parse_type_atom(&mut self) -> Result<TypeHint, ParseError> {
        crate::stack::guard(|| {
            let start = self.current_pos();
            if self.eat(&Token::LParen) {
                let members_start = self.begin_type_hint_list();
                let atom = self.parse_type_atom()?;
                self.push_type_hint(atom);
                while self.eat(&Token::Amp) {
                    let atom = self.parse_type_atom()?;
                    self.push_type_hint(atom);
                }
                self.expect(&Token::RParen)?;
                let len = self.type_hint_scratch.len() - members_start;
                if len == 1 {
                    return Ok(self.type_hint_scratch.pop().expect("one type member"));
                }
                let members = self.finish_type_hint_list(members_start);
                return Ok(self.type_hint(TypeHintKind::Intersection(members), start));
            }
            // These are single keyword tokens - namespaced forms like
            // `array\Foo` don't exist, so each just consumes itself and stops,
            // rather than looping on to swallow whatever bare identifier
            // happens to follow (e.g. `array $items`'s `$items`, or
            // `const array NAME`'s own `NAME`).
            let keyword = match self.peek() {
                Token::Keyword(Keyword::Array) => Some(NativeType::Array),
                Token::Keyword(Keyword::Static) => Some(NativeType::Static),
                Token::Keyword(Keyword::Null) => Some(NativeType::Null),
                // PHP 8.2+ stand-alone `true`/`false` types, e.g.
                // `function f(): string|false {}`.
                Token::Keyword(Keyword::True) => Some(NativeType::True),
                Token::Keyword(Keyword::False) => Some(NativeType::False),
                _ => None,
            };
            if let Some(kind) = keyword {
                self.skip();
                return Ok(self.type_hint(TypeHintKind::Keyword(kind, kind.as_str().into()), start));
            }
            // Otherwise it's a (possibly namespaced) identifier: `int`,
            // `Foo`, `\App\Foo`, `App\Foo\Bar` - never two unrelated
            // identifiers glued together with no backslash between them.
            let name = self.parse_qualified_name();
            // A bare keyword is the native type; anything else, `integer`
            // and `\int` included, names a class.
            let kind = match NativeType::from_name(&name) {
                Some(keyword) => TypeHintKind::Keyword(keyword, name.as_str().into()),
                None => {
                    let qualified =
                        self.qualified_name(name, self.span_from(start), NameKind::Class);
                    TypeHintKind::Name(
                        self.node_arenas
                            .borrow_mut()
                            .alloc_qualified_name(qualified),
                    )
                }
            };
            Ok(self.type_hint(kind, start))
        })
    }

    pub fn parse_type_hint(&mut self) -> Result<Option<TypeHint>, ParseError> {
        if !self.is_type_start() {
            return Ok(None);
        }
        let start = self.current_pos();
        let nullable = self.eat(&Token::Question);
        let inner_start = self.current_pos();
        // `|` binds looser than `&`: the terms of a union, each a run of
        // `&`-joined atoms.
        let terms_start = self.begin_type_hint_list();
        loop {
            let members_start = self.begin_type_hint_list();
            let first = self.parse_type_atom()?;
            let member_start = first.span.start;
            self.push_type_hint(first);
            // `&` is ambiguous between an intersection type (`A&B`) and a
            // by-ref marker before a param's `$name` - only treat it as part
            // of the type when another type atom actually follows.
            if self.check(&Token::Amp) && self.is_type_start_at(1) {
                while self.check(&Token::Amp) && self.is_type_start_at(1) {
                    self.skip();
                    let atom = self.parse_type_atom()?;
                    self.push_type_hint(atom);
                }
            }
            let member_count = self.type_hint_scratch.len() - members_start;
            let member_end = self
                .type_hint_scratch
                .last()
                .expect("a type member was just parsed")
                .span
                .end;
            let term = if member_count == 1 {
                self.type_hint_scratch.pop().expect("one type member")
            } else {
                let members = self.finish_type_hint_list(members_start);
                TypeHint {
                    kind: TypeHintKind::Intersection(members),
                    span: Span {
                        start: member_start,
                        end: member_end,
                    },
                }
            };
            self.push_type_hint(term);
            if !self.eat(&Token::Pipe) {
                break;
            }
        }
        let term_count = self.type_hint_scratch.len() - terms_start;
        let inner = if term_count == 1 {
            self.type_hint_scratch.pop().expect("one union term")
        } else {
            let terms = self.finish_type_hint_list(terms_start);
            self.type_hint(TypeHintKind::Union(terms), inner_start)
        };
        Ok(Some(if nullable {
            self.type_hint(TypeHintKind::Nullable(Box::new(inner)), start)
        } else {
            inner
        }))
    }

    fn type_hint(&self, kind: TypeHintKind, start: u32) -> TypeHint {
        TypeHint {
            kind,
            span: Span {
                start,
                end: self.previous_end(),
            },
        }
    }
}
