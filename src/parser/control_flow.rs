use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Position, Span, Token};

impl Parser {
    pub(crate) fn parse_block(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            stmts.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace)?;
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
        self.advance(); // if
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;

        if self.check(&Token::Colon) {
            return self.parse_if_alt(cond, start);
        }

        let then_branch = Box::new(self.parse_braced_or_single()?);

        let else_branch = if self.eat(&Token::Keyword(Keyword::ElseIf)) {
            self.pos -= 1; // put back; re-dispatch as a nested `if`
            self.tokens[self.pos] = Token::Keyword(Keyword::If);
            Some(Box::new(self.parse_if()?))
        } else if self.eat(&Token::Keyword(Keyword::Else)) {
            Some(Box::new(self.parse_braced_or_single()?))
        } else {
            None
        };

        Ok(self.stmt(
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            },
            start,
        ))
    }

    // `if (c1): ... elseif (c2): ... else: ... endif;` - only one `endif;`
    // terminates the whole chain, so branches are collected flat and then
    // folded (innermost-out) into the same nested StmtKind::If shape the
    // brace syntax produces. `if_start` is where the very first `if`
    // keyword began (captured by the caller, `parse_if`, before it
    // consumed anything) - each nested synthesized `If`/`Block` below
    // still gets its own more specific start where possible (the
    // branch's own condition/keyword), rather than every level collapsing
    // onto the same outer position.
    pub(crate) fn parse_if_alt(
        &mut self,
        first_cond: Expr,
        if_start: Position,
    ) -> Result<Stmt, ParseError> {
        let mut branches: Vec<(Position, Expr, Vec<Stmt>)> = Vec::new();
        let mut cond = first_cond;
        let mut branch_start = if_start;
        self.expect(&Token::Colon)?;
        loop {
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
            branches.push((branch_start, cond.clone(), stmts));
            if self.eat(&Token::Keyword(Keyword::ElseIf)) {
                branch_start = self.previous_end(); // approx: right at `elseif`
                self.expect(&Token::LParen)?;
                cond = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::Colon)?;
                continue;
            }
            break;
        }

        let else_start = self.current_pos();
        let else_stmts = if self.eat(&Token::Keyword(Keyword::Else)) {
            self.expect(&Token::Colon)?;
            let mut stmts = Vec::new();
            while !self.check(&Token::Keyword(Keyword::EndIf)) && !self.at_eof() {
                stmts.push(self.parse_statement()?);
            }
            Some(stmts)
        } else {
            None
        };

        self.expect(&Token::Keyword(Keyword::EndIf))?;
        self.expect_semicolon()?;
        let end = self.previous_end();

        let mut result = else_stmts.map(|stmts| Stmt {
            kind: StmtKind::Block(stmts),
            span: Span {
                start: else_start,
                end,
            },
            doc_comment: None,
        });
        for (branch_pos, c, stmts) in branches.into_iter().rev() {
            result = Some(Stmt {
                kind: StmtKind::If {
                    cond: c,
                    then_branch: Box::new(Stmt {
                        kind: StmtKind::Block(stmts),
                        span: Span {
                            start: branch_pos,
                            end,
                        },
                        doc_comment: None,
                    }),
                    else_branch: result.map(Box::new),
                },
                span: Span {
                    start: branch_pos,
                    end,
                },
                doc_comment: None,
            });
        }
        Ok(result.unwrap())
    }

    pub(crate) fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_body(Keyword::EndWhile)?;
        Ok(self.stmt(StmtKind::While { cond, body }, start))
    }

    pub(crate) fn parse_do_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance();
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
        self.advance();
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
        self.advance();
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::Keyword(Keyword::As))?;
        let by_ref_first = self.eat(&Token::Amp);
        let first = self.parse_expression()?;

        let (key, by_ref, value) = if self.eat(&Token::FatArrow) {
            let by_ref_val = self.eat(&Token::Amp);
            let value = self.parse_expression()?;
            (Some(first), by_ref_val, value)
        } else {
            (None, by_ref_first, first)
        };

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
        self.advance(); // try
        let body = self.parse_stmt_list_block()?;

        let mut catches = Vec::new();
        while self.eat(&Token::Keyword(Keyword::Catch)) {
            let catch_start = self.previous_end();
            self.expect(&Token::LParen)?;
            let (first_type, first_span) = self.parse_qualified_name_spanned();
            let mut types = vec![first_type];
            let mut types_spans = vec![first_span];
            while self.eat(&Token::Pipe) {
                let (name, span) = self.parse_qualified_name_spanned();
                types.push(name);
                types_spans.push(span);
            }
            let mut var_span = None;
            let var = if matches!(self.peek(), Token::Variable(_)) {
                let var_start = self.current_pos();
                let name = self.expect_variable()?;
                var_span = Some(Span {
                    start: var_start,
                    end: self.previous_end(),
                });
                Some(name)
            } else {
                None
            };
            self.expect(&Token::RParen)?;
            let body = self.parse_stmt_list_block()?;
            catches.push(CatchClause {
                types,
                types_spans,
                var,
                var_span,
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

    pub(crate) fn parse_stmt_list_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        if let StmtKind::Block(stmts) = self.parse_block()?.kind {
            Ok(stmts)
        } else {
            unreachable!()
        }
    }

    pub(crate) fn parse_switch(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance(); // switch
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::RParen)?;

        let alt = self.check(&Token::Colon);
        if alt {
            self.advance();
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
