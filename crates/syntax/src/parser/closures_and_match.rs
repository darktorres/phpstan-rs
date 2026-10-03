use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser<'_> {
    pub(crate) fn parse_closure_like(&mut self, is_static: bool) -> Result<Expr, ParseError> {
        if self.check(&Token::Keyword(Keyword::Fn)) {
            self.parse_arrow_fn(is_static)
        } else {
            self.parse_closure(is_static)
        }
    }

    pub(crate) fn parse_closure(&mut self, is_static: bool) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Function))?;
        let by_ref = self.eat(&Token::Amp);
        let params = self.parse_params()?;

        let mut uses = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Use)) {
            self.expect(&Token::LParen)?;
            while !self.check(&Token::RParen) {
                let use_by_ref = self.eat(&Token::Amp);
                let name_start = self.current_pos();
                let name = self.expect_variable()?;
                uses.push(ClosureUse {
                    name: Ident {
                        name,
                        span: Span {
                            start: name_start,
                            end: self.previous_end(),
                        },
                    },
                    by_ref: use_by_ref,
                });
                if !self.eat(&Token::Comma) {
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
        let body = self.parse_stmt_list_block();
        self.nested_depth.set(self.nested_depth.get() - 1);
        let end = self.leave_goto_scope(depth);
        let body = body?;

        Ok(self.expr(
            ExprKind::Closure(Box::new(ClosureExpr {
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
                attrs: Vec::new(),
            })),
            start,
        ))
    }

    pub(crate) fn parse_arrow_fn(&mut self, is_static: bool) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Fn))?;
        let by_ref = self.eat(&Token::Amp);
        let params = self.parse_params()?;
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
        Ok(self.expr(
            ExprKind::ArrowFn(Box::new(ArrowFnExpr {
                params,
                body: Box::new(body),
                is_generator: end.is_generator,
                is_static,
                by_ref,
                return_type,
                // See `Closure`'s identical note in `parse_closure`.
                attrs: Vec::new(),
            })),
            start,
        ))
    }

    pub(crate) fn parse_match_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Match))?;
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::LBrace)?;

        let mut arms = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            let arm_start = self.current_pos();
            let conditions = if self.eat(&Token::Keyword(Keyword::Default)) {
                if arms.iter().any(|arm: &MatchArm| arm.conditions.is_none()) {
                    return Err(ParseError {
                        message: "Match expressions may only contain one default arm".to_string(),
                        span: Span {
                            start: arm_start,
                            end: self.previous_end(),
                        },
                    });
                }
                // `default` also tolerates a trailing comma before `=>`,
                // same as an ordinary arm's condition list.
                self.eat(&Token::Comma);
                None
            } else {
                let mut conds = vec![self.parse_expression()?];
                while self.eat(&Token::Comma) {
                    if self.check(&Token::FatArrow) {
                        break;
                    }
                    conds.push(self.parse_expression()?);
                }
                Some(conds)
            };
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expression()?;
            arms.push(MatchArm {
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

        Ok(self.expr(
            ExprKind::Match {
                subject: Box::new(subject),
                arms,
            },
            start,
        ))
    }
}
