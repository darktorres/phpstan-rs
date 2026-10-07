use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, TokenRef as Token};

impl Parser<'_> {
    pub(crate) fn parse_closure_like(&mut self, is_static: bool) -> Result<ExprId, ParseError> {
        if self.check(&Token::Keyword(Keyword::Fn)) {
            self.parse_arrow_fn(is_static)
        } else {
            self.parse_closure(is_static)
        }
    }

    pub(crate) fn parse_closure(&mut self, is_static: bool) -> Result<ExprId, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Function))?;
        let by_ref = self.eat(&Token::Amp);
        let mut no_doc = None;
        let params = self.parse_params(&mut no_doc, true)?;

        let uses_start = self.closure_use_scratch.len();
        if self.eat(&Token::Keyword(Keyword::Use)) {
            self.expect(&Token::LParen)?;
            while !self.check(&Token::RParen) {
                let use_by_ref = self.eat(&Token::Amp);
                let name_start = self.current_pos();
                let name = self.expect_variable()?;
                self.closure_use_scratch.push(ClosureUse {
                    name: self.ident(
                        name,
                        Span {
                            start: name_start,
                            end: self.previous_end(),
                        },
                    ),
                    by_ref: use_by_ref,
                });
                if !self.eat_list_comma(
                    &Token::RParen,
                    crate::lexer::SyntaxMark::TrailingCommaInClosureUse,
                ) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
        }

        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()?.map(Box::new)
        } else {
            None
        };

        let depth = self.enter_goto_scope();
        self.nested_depth.set(self.nested_depth.get() + 1);
        let body = self.parse_stmt_list_block_range();
        self.nested_depth.set(self.nested_depth.get() - 1);
        let end = self.leave_goto_scope(depth);
        let body = body?;

        let attrs = self.node_arenas.borrow_mut().alloc_attributes([]);
        let uses = self
            .node_arenas
            .borrow_mut()
            .alloc_closure_uses(self.closure_use_scratch.drain(uses_start..));
        let closure = self
            .node_arenas
            .borrow_mut()
            .alloc_closure(ClosurePayload::Closure(ClosureExpr {
                params,
                uses,
                body,
                goto_labels: end.goto_labels,
                is_generator: end.is_generator,
                is_static,
                by_ref,
                return_type,
                // Overwritten by `parse_primary`'s own `AttributeStart`
                // arm when this closure was actually preceded by one -
                // `parse_closure`/`parse_closure_like` are entered only
                // after `static` (if present) has already been consumed,
                // past where an attribute group would sit.
                attrs,
            }));
        Ok(self.expr(ExprKind::Closure(closure), start))
    }

    pub(crate) fn parse_arrow_fn(&mut self, is_static: bool) -> Result<ExprId, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Fn))?;
        let by_ref = self.eat(&Token::Amp);
        let mut no_doc = None;
        let params = self.parse_params(&mut no_doc, true)?;
        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()?.map(Box::new)
        } else {
            None
        };
        self.expect(&Token::FatArrow)?;
        let depth = self.enter_goto_scope();
        let body = self.parse_expression();
        let end = self.leave_goto_scope(depth);
        let body = body?;
        let attrs = self.node_arenas.borrow_mut().alloc_attributes([]);
        let closure = self
            .node_arenas
            .borrow_mut()
            .alloc_closure(ClosurePayload::ArrowFn(ArrowFnExpr {
                params,
                body,
                is_generator: end.is_generator,
                is_static,
                by_ref,
                return_type,
                // See `Closure`'s identical note in `parse_closure`.
                attrs,
            }));
        Ok(self.expr(ExprKind::ArrowFn(closure), start))
    }

    pub(crate) fn parse_match_expr(&mut self) -> Result<ExprId, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Match))?;
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace)?;

        let arms_start = self.begin_match_arms();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            let arm_start = self.current_pos();
            let conditions = if self.eat(&Token::Keyword(Keyword::Default)) {
                if self
                    .match_arms(arms_start)
                    .iter()
                    .any(|arm| arm.conditions.is_none())
                {
                    return Err(ParseError::new(
                        "Match expressions may only contain one default arm".to_string(),
                        Span {
                            start: arm_start,
                            end: self.previous_end(),
                        },
                    ));
                }
                // `default` also tolerates a trailing comma before `=>`,
                // same as an ordinary arm's condition list.
                self.eat(&Token::Comma);
                None
            } else {
                let conditions_start = self.begin_expr_id_list();
                let first = self.parse_expression()?;
                self.push_expr_id_list_item(first);
                while self.eat(&Token::Comma) {
                    if self.check(&Token::FatArrow) {
                        break;
                    }
                    let condition = self.parse_expression()?;
                    self.push_expr_id_list_item(condition);
                }
                Some(self.finish_expr_id_list(conditions_start))
            };
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expression()?;
            self.push_match_arm(MatchArm {
                conditions,
                body,
                span: Span {
                    start: arm_start,
                    end: self.previous_end(),
                },
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;

        let arms = self.finish_match_arms(arms_start);
        Ok(self.expr(ExprKind::Match { subject, arms }, start))
    }
}
