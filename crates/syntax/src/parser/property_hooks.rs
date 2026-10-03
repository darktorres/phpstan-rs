use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser {
    /// PHP 8.4 property hooks body: `{ get => expr; set(Type $value) { .. }
    /// }`, or the abstract/interface form (`{ get; set; }`) with no bodies
    /// at all. `get`/`set` aren't keywords (they're semi-reserved words
    /// used only in this position), so they arrive as plain identifiers.
    pub(crate) fn parse_property_hooks(&mut self) -> Result<PropertyHooks, ParseError> {
        self.expect(&Token::LBrace)?;
        let mut get = None;
        let mut set = None;
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
                    name: Ident {
                        name,
                        span: name_span,
                    },
                    type_hint,
                    default: None,
                    by_ref,
                    variadic: false,
                    promotion: None,
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

            let mut goto_labels: Box<[GotoLabel]> = Box::default();
            let body = if self.eat(&Token::FatArrow) {
                let e = self.parse_expression()?;
                self.expect_semicolon()?;
                PropertyHookBody::Arrow(e)
            } else if self.check(&Token::LBrace) {
                let depth = self.enter_goto_scope();
                let stmts = self.parse_stmt_list_block();
                goto_labels = self.leave_goto_scope(depth);
                PropertyHookBody::Block(stmts?)
            } else {
                // Abstract form: `get;` / `set;`, no body at all - a
                // hook that's declared but has no body, unambiguously
                // distinct from no hook being declared at all (this whole
                // `if`/`else` chain only runs once `is_set` has already
                // confirmed a real `get`/`set` keyword was consumed).
                self.expect_semicolon()?;
                PropertyHookBody::Abstract
            };

            let hook = PropertyHook {
                body,
                goto_labels,
                by_ref: hook_by_ref,
                param,
                attrs: hook_attrs,
                doc: hook_doc,
                is_final: hook_final,
            };
            if is_set {
                set = Some(hook);
            } else {
                get = Some(hook);
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(PropertyHooks { get, set })
    }
}
