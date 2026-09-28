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
        let mut get_by_ref = false;
        let mut set = None;
        let mut get_attrs = Vec::new();
        let mut set_attrs = Vec::new();
        let mut get_doc = None;
        let mut set_doc = None;
        let mut get_final = false;
        let mut set_final = false;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            // `#[Attr]` group(s) directly preceding this hook's own
            // `get`/`set` keyword - must be parsed before anything else
            // here (the `final`/visibility-modifier loop and the by-ref
            // `&` just below), matching real PHP's own convention of
            // attributes preceding all modifiers. `parse_leading_
            // attributes` (rather than a bare `parse_attributes`) re-keys
            // a preceding doc comment so `take_doc_comment` just below
            // still finds it (a hook's own `@throws` tag).
            let hook_attrs = self.parse_leading_attributes()?;
            let hook_doc = self.take_doc_comment();
            // A hook can itself carry `final`/visibility modifiers ahead
            // of `get`/`set` - visibility isn't modeled (real PHP itself
            // restricts a hook's own visibility to be no wider than the
            // property's, so nothing this checker would need to separately
            // enforce), but `final` is (`TooWideMethodThrowTypeRule`'s
            // property-hook sibling gates on it, mirroring real PHPStan's
            // own `$hookReflection->isFinal()`).
            let mut hook_final = false;
            while matches!(
                self.peek(),
                Token::Keyword(Keyword::Final)
                    | Token::Keyword(Keyword::Public)
                    | Token::Keyword(Keyword::Private)
                    | Token::Keyword(Keyword::Protected)
            ) {
                if matches!(self.peek(), Token::Keyword(Keyword::Final)) {
                    hook_final = true;
                }
                self.skip();
            }
            // `&get => ...` - the hook returns its value by reference.
            let hook_by_ref = self.eat(&Token::Amp);
            let is_set =
                matches!(self.peek(), Token::Identifier(s) if s.eq_ignore_ascii_case("set"));
            self.skip(); // `get` or `set`

            let param = if is_set && self.eat(&Token::LParen) {
                let param_start = self.previous_end();
                let attrs = self.parse_attributes()?;
                let type_hint = self.parse_type_hint()?;
                let by_ref = self.eat(&Token::Amp);
                let name_start = self.current_pos();
                let name = self.expect_variable()?;
                let name_span = Span {
                    start: name_start,
                    end: self.previous_end(),
                };
                self.expect(&Token::RParen)?;
                Some(Param {
                    id: self.node_id(),
                    name,
                    name_span,
                    type_hint,
                    default: None,
                    by_ref,
                    variadic: false,
                    promoted_visibility: None,
                    promoted_set_visibility: None,
                    promoted_readonly: false,
                    promoted_final: false,
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
                // Abstract form: `get;` / `set;`, no body at all - a
                // hook that's declared but has no body, unambiguously
                // distinct from no hook being declared at all (this whole
                // `if`/`else` chain only runs once `is_set` has already
                // confirmed a real `get`/`set` keyword was consumed).
                self.expect_semicolon()?;
                Some(PropertyHookBody::Abstract)
            };

            if is_set {
                set = body.map(|b| (param, b));
                set_attrs = hook_attrs;
                set_doc = hook_doc;
                set_final = hook_final;
            } else {
                get = body;
                get_by_ref = hook_by_ref;
                get_attrs = hook_attrs;
                get_doc = hook_doc;
                get_final = hook_final;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(PropertyHooks {
            get,
            get_by_ref,
            set,
            get_attrs,
            set_attrs,
            get_doc,
            set_doc,
            get_final,
            set_final,
        })
    }
}
