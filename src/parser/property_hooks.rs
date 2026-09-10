use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Span, Token};

impl Parser {
    /// PHP 8.4 property hooks body: `{ get => expr; set(Type $value) { .. }
    /// }`, or the abstract/interface form (`{ get; set; }`) with no bodies
    /// at all. `get`/`set` aren't keywords (they're semi-reserved words
    /// used only in this position), so they arrive as plain identifiers.
    pub(crate) fn parse_property_hooks(&mut self) -> Result<PropertyHooks, ParseError> {
        self.expect(&Token::LBrace)?;
        let mut get = None;
        let mut set = None;
        let mut get_attrs = Vec::new();
        let mut set_attrs = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            // Phase 37: `#[Attr]` group(s) directly preceding this hook's
            // own `get`/`set` keyword - must be parsed before anything
            // else here (the `final`/visibility-modifier loop and the
            // by-ref `&` just below), matching real PHP's own convention
            // of attributes preceding all modifiers. `parse_attributes`
            // is already safe to call unconditionally - it returns an
            // empty `Vec` without consuming anything when the next token
            // isn't an attribute start.
            let hook_attrs = self.parse_attributes()?;
            // A hook can itself carry `final`/visibility modifiers ahead
            // of `get`/`set` - not modeled, just consumed.
            while matches!(
                self.peek(),
                Token::Keyword(Keyword::Final)
                    | Token::Keyword(Keyword::Public)
                    | Token::Keyword(Keyword::Private)
                    | Token::Keyword(Keyword::Protected)
            ) {
                self.advance();
            }
            // `&get => ...` - the hook returns its value by reference.
            // Aliasing isn't modeled anywhere in this checker, so it's
            // just consumed.
            self.eat(&Token::Amp);
            let is_set = matches!(self.peek(), Token::Identifier(s) if s == "set");
            self.advance(); // `get` or `set`

            let param = if is_set && self.eat(&Token::LParen) {
                let param_start = self.previous_end();
                let attrs = self.parse_attributes()?;
                let type_hint_start = self.current_pos();
                let type_hint = self.parse_type_hint()?;
                let type_hint_span = type_hint.as_ref().map(|_| Span {
                    start: type_hint_start,
                    end: self.previous_end(),
                });
                let by_ref = self.eat(&Token::Amp);
                let name_start = self.current_pos();
                let name = self.expect_variable()?;
                let name_span = Span {
                    start: name_start,
                    end: self.previous_end(),
                };
                self.expect(&Token::RParen)?;
                Some(Param {
                    name,
                    name_span,
                    type_hint,
                    type_hint_span,
                    default: None,
                    by_ref,
                    variadic: false,
                    promoted_visibility: None,
                    promoted_readonly: false,
                    hooks: None,
                    attrs,
                    doc_comment: None,
                    span: Span {
                        start: param_start,
                        end: self.previous_end(),
                    },
                })
            } else {
                None
            };

            let body = if self.eat(&Token::FatArrow) {
                let e = self.parse_expression()?;
                self.expect_semicolon()?;
                Some(PropertyHookBody::Arrow(e))
            } else if self.check(&Token::LBrace) {
                Some(PropertyHookBody::Block(self.parse_stmt_list_block()?))
            } else {
                // Abstract form: `get;` / `set;`, no body at all.
                self.expect_semicolon()?;
                None
            };

            if is_set {
                set = body.map(|b| (param, b));
                set_attrs = hook_attrs;
            } else {
                get = body;
                get_attrs = hook_attrs;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(PropertyHooks {
            get,
            set,
            get_attrs,
            set_attrs,
        })
    }
}
