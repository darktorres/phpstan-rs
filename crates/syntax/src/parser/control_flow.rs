use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, TokenRef as Token};

impl Parser<'_> {
    pub(crate) fn parse_block(&mut self) -> Result<StmtId, ParseError> {
        let start = self.current_pos();
        let stmts = self.parse_stmt_list_block_range()?;
        Ok(self.stmt(StmtKind::Block(stmts), start))
    }

    pub(crate) fn parse_braced_or_single(&mut self) -> Result<StmtId, ParseError> {
        if self.check(&Token::LBrace) {
            self.parse_block()
        } else {
            Ok(self
                .parse_statement(false)?
                .expect("a single statement is not omitted by the stub rules"))
        }
    }

    /// A loop/if body, accepting either `{ ... }` / a bare statement, or
    /// PHP's alternative colon syntax (`: ... endwhile;` etc, common in
    /// templates that interleave HTML and PHP).
    pub(crate) fn parse_body(&mut self, alt_end: Keyword) -> Result<StmtId, ParseError> {
        let start = self.current_pos();
        if self.check(&Token::Colon) {
            let stmts = self.parse_alt_block(alt_end)?;
            Ok(self.stmt(StmtKind::Block(stmts), start))
        } else {
            self.parse_braced_or_single()
        }
    }

    pub(crate) fn parse_alt_block(&mut self, end_kw: Keyword) -> Result<StmtIdRange, ParseError> {
        self.expect(&Token::Colon)?;
        let list_start = self.begin_stmt_list();
        let parsed = (|| {
            while !self.check(&Token::Keyword(end_kw.clone())) && !self.at_eof() {
                if let Some(stmt) = self.parse_statement(true)? {
                    self.push_stmt_list_item(stmt);
                }
            }
            self.expect(&Token::Keyword(end_kw))?;
            self.expect_semicolon()?;
            Ok::<(), ParseError>(())
        })();
        if let Err(error) = parsed {
            self.discard_stmt_list(list_start);
            return Err(error);
        }
        Ok(self.finish_stmt_list(list_start))
    }

    pub(crate) fn parse_if(&mut self) -> Result<StmtId, ParseError> {
        let start = self.current_pos();
        self.skip(); // if
        let cond = self.parse_paren_condition()?;
        if self.check(&Token::Colon) {
            return self.parse_if_alt(cond, start);
        }
        let then_branch = self.parse_braced_or_single()?;
        let elseifs_start = self.begin_elseifs();
        while self.check(&Token::Keyword(Keyword::ElseIf)) {
            let elseif_start = self.current_pos();
            self.skip();
            let cond = self.parse_paren_condition()?;
            let body = self.parse_braced_or_single()?;
            self.elseif_scratch.push(ElseIf {
                cond,
                body,
                span: Span {
                    start: elseif_start,
                    end: self.previous_end(),
                },
            });
        }
        let else_branch = if self.eat(&Token::Keyword(Keyword::Else)) {
            Some(self.parse_braced_or_single()?)
        } else {
            None
        };
        let elseifs = self.finish_elseifs(elseifs_start);
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
    fn parse_paren_condition(&mut self) -> Result<ExprId, ParseError> {
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        Ok(cond)
    }

    /// `if (c1): … elseif (c2): … else: … endif;`, the `if` keyword and its
    /// condition already read.
    pub(crate) fn parse_if_alt(&mut self, cond: ExprId, start: u32) -> Result<StmtId, ParseError> {
        let then_branch = self.parse_alt_branch()?;
        let elseifs_start = self.begin_elseifs();
        while self.check(&Token::Keyword(Keyword::ElseIf)) {
            let elseif_start = self.current_pos();
            self.skip();
            let cond = self.parse_paren_condition()?;
            let body = self.parse_alt_branch()?;
            self.elseif_scratch.push(ElseIf {
                cond,
                body,
                span: Span {
                    start: elseif_start,
                    end: self.previous_end(),
                },
            });
        }
        let else_branch = if self.eat(&Token::Keyword(Keyword::Else)) {
            Some(self.parse_alt_branch()?)
        } else {
            None
        };
        self.expect(&Token::Keyword(Keyword::EndIf))?;
        self.expect_semicolon()?;
        let elseifs = self.finish_elseifs(elseifs_start);
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
    fn parse_alt_branch(&mut self) -> Result<StmtId, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Colon)?;
        let list_start = self.begin_stmt_list();
        while !matches!(
            self.peek(),
            Token::Keyword(Keyword::ElseIf)
                | Token::Keyword(Keyword::Else)
                | Token::Keyword(Keyword::EndIf)
        ) && !self.at_eof()
        {
            match self.parse_statement(true) {
                Ok(Some(stmt)) => self.push_stmt_list_item(stmt),
                Ok(None) => {}
                Err(error) => {
                    self.discard_stmt_list(list_start);
                    return Err(error);
                }
            }
        }
        let stmts = self.finish_stmt_list(list_start);
        Ok(self.stmt(StmtKind::Block(stmts), start))
    }

    pub(crate) fn parse_while(&mut self) -> Result<StmtId, ParseError> {
        let start = self.current_pos();
        self.skip();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_body(Keyword::EndWhile)?;
        Ok(self.stmt(StmtKind::While { cond, body }, start))
    }

    pub(crate) fn parse_do_while(&mut self) -> Result<StmtId, ParseError> {
        let start = self.current_pos();
        self.skip();
        let body = self.parse_braced_or_single()?;
        self.expect(&Token::Keyword(Keyword::While))?;
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        self.expect_semicolon()?;
        Ok(self.stmt(StmtKind::DoWhile { body, cond }, start))
    }

    pub(crate) fn parse_for(&mut self) -> Result<StmtId, ParseError> {
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

    pub(crate) fn parse_expr_list_until(
        &mut self,
        stop: &Token,
    ) -> Result<ExprIdRange, ParseError> {
        let exprs_start = self.begin_expr_id_list();
        if self.check(stop) {
            return Ok(self.finish_expr_id_list(exprs_start));
        }
        let first = self.parse_expression()?;
        self.push_expr_id_list_item(first);
        while self.eat(&Token::Comma) {
            let expr = self.parse_expression()?;
            self.push_expr_id_list_item(expr);
        }
        Ok(self.finish_expr_id_list(exprs_start))
    }

    pub(crate) fn parse_foreach(&mut self) -> Result<StmtId, ParseError> {
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
                return Err(ParseError::new(
                    "Key element cannot be a reference".to_string(),
                    first_amp,
                ));
            }
            if matches!(
                &self.expr_node(first).kind,
                ExprKind::ArrayLiteral(_) | ExprKind::Destructure(..)
            ) {
                return Err(ParseError::new(
                    "Cannot use list as key element".to_string(),
                    self.expr_span(first),
                ));
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
            &self.expr_node(value).kind,
            ExprKind::ArrayLiteral(_) | ExprKind::Destructure(..)
        );
        if by_ref && is_pattern {
            return Err(ParseError::new(
                "syntax error, unexpected token \"&\"".to_string(),
                amp_span,
            ));
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

    pub(crate) fn parse_try(&mut self) -> Result<StmtId, ParseError> {
        let start = self.current_pos();
        self.skip(); // try
        let body = self.parse_stmt_list_block_range()?;

        let catches_start = self.begin_catch_clauses();
        while self.eat(&Token::Keyword(Keyword::Catch)) {
            let catch_start = self.previous_end();
            self.expect(&Token::LParen)?;
            let types_start = self.qualified_name_scratch.len();
            let name = self.parse_name_reference();
            self.qualified_name_scratch.push(name);
            while self.eat(&Token::Pipe) {
                let name = self.parse_name_reference();
                self.qualified_name_scratch.push(name);
            }
            let var = if matches!(self.peek(), Token::Variable(_)) {
                let var_start = self.current_pos();
                let name = self.expect_variable()?;
                Some(self.ident(
                    name,
                    Span {
                        start: var_start,
                        end: self.previous_end(),
                    },
                ))
            } else {
                None
            };
            self.expect(&Token::RParen)?;
            let body = self.parse_stmt_list_block_range()?;
            let types = self
                .qualified_name_scratch
                .drain(types_start..)
                .collect::<Vec<_>>();
            let types = self.node_arenas.borrow_mut().alloc_qualified_names(types);
            self.catch_clause_scratch.push(CatchClause {
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
            Some(self.parse_stmt_list_block_range()?)
        } else {
            None
        };
        let catches = self.finish_catch_clauses(catches_start);

        Ok(self.stmt(
            StmtKind::Try {
                body,
                catches,
                finally,
            },
            start,
        ))
    }

    /// A `{ ... }` block's statements, built in parser scratch and committed
    /// as one contiguous range in the owning statement arena.
    pub(crate) fn parse_stmt_list_block_range(&mut self) -> Result<StmtIdRange, ParseError> {
        self.expect(&Token::LBrace)?;
        let list_start = self.begin_stmt_list();
        let parsed = (|| {
            while !self.check(&Token::RBrace) && !self.at_eof() {
                if let Some(stmt) = self.parse_statement(true)? {
                    self.push_stmt_list_item(stmt);
                }
            }
            self.expect(&Token::RBrace)?;
            Ok::<(), ParseError>(())
        })();
        if let Err(error) = parsed {
            self.discard_stmt_list(list_start);
            return Err(error);
        }
        Ok(self.finish_stmt_list(list_start))
    }

    pub(crate) fn parse_switch(&mut self) -> Result<StmtId, ParseError> {
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

        let cases_start = self.begin_switch_cases();
        let mut case_list_start = None;
        let closer = Token::Keyword(Keyword::EndSwitch);
        while !self.at_eof() && !self.check(if alt { &closer } else { &Token::RBrace }) {
            if self.eat(&Token::Keyword(Keyword::Case)) {
                if let Some(list_start) = case_list_start.take() {
                    let body = self.finish_stmt_list(list_start);
                    self.switch_case_scratch.last_mut().unwrap().body = body;
                }
                let case_start = self.previous_end();
                let test = self.parse_expression()?;
                if !self.eat(&Token::Colon) {
                    self.eat(&Token::Semicolon);
                }
                case_list_start = Some(self.begin_stmt_list());
                self.switch_case_scratch.push(SwitchCase {
                    test: Some(test),
                    body: self.node_arenas.borrow_mut().alloc_stmt_ids([]),
                    span: Span {
                        start: case_start,
                        end: self.previous_end(),
                    },
                });
            } else if self.eat(&Token::Keyword(Keyword::Default)) {
                if let Some(list_start) = case_list_start.take() {
                    let body = self.finish_stmt_list(list_start);
                    self.switch_case_scratch.last_mut().unwrap().body = body;
                }
                let case_start = self.previous_end();
                if !self.eat(&Token::Colon) {
                    self.eat(&Token::Semicolon);
                }
                case_list_start = Some(self.begin_stmt_list());
                self.switch_case_scratch.push(SwitchCase {
                    test: None,
                    body: self.node_arenas.borrow_mut().alloc_stmt_ids([]),
                    span: Span {
                        start: case_start,
                        end: self.previous_end(),
                    },
                });
            } else {
                let stmt = self.parse_statement(true)?;
                // A statement before any `case`/`default` label is malformed
                // PHP; dropping it here just keeps the parser in sync.
                let end = self.previous_end();
                if let (Some(case), Some(stmt)) = (self.switch_case_scratch.last_mut(), stmt) {
                    case.span.end = end;
                    self.push_stmt_list_item(stmt);
                }
            }
        }

        if let Some(list_start) = case_list_start {
            let body = self.finish_stmt_list(list_start);
            self.switch_case_scratch.last_mut().unwrap().body = body;
        }

        if alt {
            self.expect(&closer)?;
            self.expect_semicolon()?;
        } else {
            self.expect(&Token::RBrace)?;
        }

        let cases = self.finish_switch_cases(cases_start);
        Ok(self.stmt(StmtKind::Switch { subject, cases }, start))
    }
}
