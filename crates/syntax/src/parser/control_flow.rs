use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Position, Span, Token};

impl Parser {
    pub(crate) fn parse_block(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        let stmts = self.parse_stmt_list_block()?;
        Ok(self.stmt(StmtKind::Block(stmts), start))
    }

    pub(crate) fn parse_braced_or_single(&mut self) -> Result<Stmt, ParseError> {
        if self.check(&Token::LBrace) {
            self.parse_block()
        } else {
            self.parse_statement()
        }
    }

    /// A loop/if body, accepting either `{ ... }` / a bare statement, or
    /// PHP's alternative colon syntax (`: ... endwhile;` etc, common in
    /// templates that interleave HTML and PHP).
    pub(crate) fn parse_body(&mut self, alt_end: Keyword) -> Result<Box<Stmt>, ParseError> {
        let start = self.current_pos();
        if self.check(&Token::Colon) {
            let stmts = self.parse_alt_block(alt_end)?;
            Ok(Box::new(self.stmt(StmtKind::Block(stmts), start)))
        } else {
            Ok(Box::new(self.parse_braced_or_single()?))
        }
    }

    pub(crate) fn parse_alt_block(&mut self, end_kw: Keyword) -> Result<Vec<Stmt>, ParseError> {
        self.expect(&Token::Colon)?;
        let mut stmts = Vec::new();
        while !self.check(&Token::Keyword(end_kw.clone())) && !self.at_eof() {
            stmts.push(self.parse_statement()?);
        }
        self.expect(&Token::Keyword(end_kw))?;
        self.expect_semicolon()?;
        Ok(stmts)
    }

    pub(crate) fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.skip(); // if
        let cond = self.parse_paren_condition()?;
        if self.check(&Token::Colon) {
            return self.parse_if_alt(cond, start);
        }
        let then_branch = Box::new(self.parse_braced_or_single()?);
        let mut elseifs = Vec::new();
        while self.check(&Token::Keyword(Keyword::ElseIf)) {
            let elseif_start = self.current_pos();
            self.skip();
            let cond = self.parse_paren_condition()?;
            let body = Box::new(self.parse_braced_or_single()?);
            elseifs.push(ElseIf {
                id: self.node_id(),
                cond,
                body,
                span: Span {
                    start: elseif_start,
                    end: self.previous_end(),
                },
            });
        }
        let else_branch = if self.eat(&Token::Keyword(Keyword::Else)) {
            Some(Box::new(self.parse_braced_or_single()?))
        } else {
            None
        };
        Ok(self.stmt(
            StmtKind::If(IfStmt {
                cond,
                then_branch,
                elseifs,
                else_branch,
            }),
            start,
        ))
    }

    /// `( expr )`.
    fn parse_paren_condition(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        Ok(cond)
    }

    /// `if (c1): … elseif (c2): … else: … endif;`, the `if` keyword and its
    /// condition already read.
    pub(crate) fn parse_if_alt(&mut self, cond: Expr, start: Position) -> Result<Stmt, ParseError> {
        let then_branch = Box::new(self.parse_alt_branch()?);
        let mut elseifs = Vec::new();
        while self.check(&Token::Keyword(Keyword::ElseIf)) {
            let elseif_start = self.current_pos();
            self.skip();
            let cond = self.parse_paren_condition()?;
            let body = Box::new(self.parse_alt_branch()?);
            elseifs.push(ElseIf {
                id: self.node_id(),
                cond,
                body,
                span: Span {
                    start: elseif_start,
                    end: self.previous_end(),
                },
            });
        }
        let else_branch = if self.eat(&Token::Keyword(Keyword::Else)) {
            Some(Box::new(self.parse_alt_branch()?))
        } else {
            None
        };
        self.expect(&Token::Keyword(Keyword::EndIf))?;
        self.expect_semicolon()?;
        Ok(self.stmt(
            StmtKind::If(IfStmt {
                cond,
                then_branch,
                elseifs,
                else_branch,
            }),
            start,
        ))
    }

    /// One branch of the alternative `if` syntax: `:` and the statements up
    /// to the next `elseif`, `else` or `endif`, as a block.
    fn parse_alt_branch(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Colon)?;
        let mut stmts = Vec::new();
        while !matches!(
            self.peek(),
            Token::Keyword(Keyword::ElseIf)
                | Token::Keyword(Keyword::Else)
                | Token::Keyword(Keyword::EndIf)
        ) && !self.at_eof()
        {
            stmts.push(self.parse_statement()?);
        }
        Ok(self.stmt(StmtKind::Block(stmts), start))
    }

    pub(crate) fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.skip();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_body(Keyword::EndWhile)?;
        Ok(self.stmt(StmtKind::While { cond, body }, start))
    }

    pub(crate) fn parse_do_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.skip();
        let body = Box::new(self.parse_braced_or_single()?);
        self.expect(&Token::Keyword(Keyword::While))?;
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        self.expect_semicolon()?;
        Ok(self.stmt(StmtKind::DoWhile { body, cond }, start))
    }

    pub(crate) fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.skip();
        self.expect(&Token::LParen)?;
        let init = self.parse_expr_list_until(&Token::Semicolon)?;
        self.expect(&Token::Semicolon)?;
        let cond = self.parse_expr_list_until(&Token::Semicolon)?;
        self.expect(&Token::Semicolon)?;
        let step = self.parse_expr_list_until(&Token::RParen)?;
        self.expect(&Token::RParen)?;
        let body = self.parse_body(Keyword::EndFor)?;
        Ok(self.stmt(
            StmtKind::For {
                init,
                cond,
                step,
                body,
            },
            start,
        ))
    }

    pub(crate) fn parse_expr_list_until(&mut self, stop: &Token) -> Result<Vec<Expr>, ParseError> {
        let mut exprs = Vec::new();
        if self.check(stop) {
            return Ok(exprs);
        }
        exprs.push(self.parse_expression()?);
        while self.eat(&Token::Comma) {
            exprs.push(self.parse_expression()?);
        }
        Ok(exprs)
    }

    pub(crate) fn parse_foreach(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.skip();
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::Keyword(Keyword::As))?;
        let first_amp = self.current_span();
        let by_ref_first = self.eat(&Token::Amp);
        let first = self.parse_expression()?;

        let (key, by_ref, amp_span, value) = if self.check(&Token::FatArrow) {
            if by_ref_first {
                return Err(ParseError {
                    message: "Key element cannot be a reference".to_string(),
                    span: first_amp,
                });
            }
            if matches!(
                first.kind,
                ExprKind::ArrayLiteral(_) | ExprKind::Destructure(..)
            ) {
                return Err(ParseError {
                    message: "Cannot use list as key element".to_string(),
                    span: first.span,
                });
            }
            self.skip();
            let amp_span = self.current_span();
            let by_ref_val = self.eat(&Token::Amp);
            let value = self.parse_expression()?;
            (Some(first), by_ref_val, amp_span, value)
        } else {
            (None, by_ref_first, first_amp, first)
        };
        let is_pattern = matches!(
            value.kind,
            ExprKind::ArrayLiteral(_) | ExprKind::Destructure(..)
        );
        if by_ref && is_pattern {
            return Err(ParseError {
                message: "syntax error, unexpected token \"&\"".to_string(),
                span: amp_span,
            });
        }
        let value = self.as_pattern(value, true)?;

        self.expect(&Token::RParen)?;
        let body = self.parse_body(Keyword::EndForeach)?;
        Ok(self.stmt(
            StmtKind::Foreach {
                subject,
                key,
                value,
                by_ref,
                body,
            },
            start,
        ))
    }

    pub(crate) fn parse_try(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.skip(); // try
        let body = self.parse_stmt_list_block()?;

        let mut catches = Vec::new();
        while self.eat(&Token::Keyword(Keyword::Catch)) {
            let catch_start = self.previous_end();
            self.expect(&Token::LParen)?;
            let mut types = vec![self.parse_name_reference()];
            while self.eat(&Token::Pipe) {
                types.push(self.parse_name_reference());
            }
            let var = if matches!(self.peek(), Token::Variable(_)) {
                let var_start = self.current_pos();
                let name = self.expect_variable()?;
                Some(Ident {
                    name,
                    span: Span {
                        start: var_start,
                        end: self.previous_end(),
                    },
                })
            } else {
                None
            };
            self.expect(&Token::RParen)?;
            let body = self.parse_stmt_list_block()?;
            catches.push(CatchClause {
                types,
                var,
                body,
                span: Span {
                    start: catch_start,
                    end: self.previous_end(),
                },
            });
        }

        let finally = if self.eat(&Token::Keyword(Keyword::Finally)) {
            Some(self.parse_stmt_list_block()?)
        } else {
            None
        };

        Ok(self.stmt(
            StmtKind::Try {
                body,
                catches,
                finally,
            },
            start,
        ))
    }

    /// A `{ ... }` block's statements.
    pub(crate) fn parse_stmt_list_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            stmts.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(stmts)
    }

    pub(crate) fn parse_switch(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.skip(); // switch
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::RParen)?;

        let alt = self.check(&Token::Colon);
        if alt {
            self.skip();
        } else {
            self.expect(&Token::LBrace)?;
        }

        let mut cases: Vec<SwitchCase> = Vec::new();
        let closer = Token::Keyword(Keyword::EndSwitch);
        while !self.at_eof() && !self.check(if alt { &closer } else { &Token::RBrace }) {
            if self.eat(&Token::Keyword(Keyword::Case)) {
                let case_start = self.previous_end();
                let test = self.parse_expression()?;
                if !self.eat(&Token::Colon) {
                    self.eat(&Token::Semicolon);
                }
                cases.push(SwitchCase {
                    test: Some(test),
                    body: Vec::new(),
                    span: Span {
                        start: case_start,
                        end: self.previous_end(),
                    },
                });
            } else if self.eat(&Token::Keyword(Keyword::Default)) {
                let case_start = self.previous_end();
                if !self.eat(&Token::Colon) {
                    self.eat(&Token::Semicolon);
                }
                cases.push(SwitchCase {
                    test: None,
                    body: Vec::new(),
                    span: Span {
                        start: case_start,
                        end: self.previous_end(),
                    },
                });
            } else {
                let stmt = self.parse_statement()?;
                // A statement before any `case`/`default` label is malformed
                // PHP; dropping it here just keeps the parser in sync.
                if let Some(case) = cases.last_mut() {
                    case.span.end = self.previous_end();
                    case.body.push(stmt);
                }
            }
        }

        if alt {
            self.expect(&closer)?;
            self.expect_semicolon()?;
        } else {
            self.expect(&Token::RBrace)?;
        }

        Ok(self.stmt(StmtKind::Switch { subject, cases }, start))
    }
}
