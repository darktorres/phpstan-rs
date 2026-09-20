use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::Token;

impl Parser {
    pub(crate) fn parse_postfix_from_primary(&mut self) -> Result<Expr, ParseError> {
        let primary = self.parse_primary()?;
        self.parse_postfix(primary)
    }

    pub(crate) fn parse_postfix(&mut self, mut expr: Expr) -> Result<Expr, ParseError> {
        let start = expr.span.start;
        loop {
            match self.peek() {
                Token::Arrow | Token::NullsafeArrow => {
                    let nullsafe = matches!(self.peek(), Token::NullsafeArrow);
                    self.skip();
                    // `$obj->{expr}` - a fully dynamic property/method
                    // name, computed from an arbitrary expression rather
                    // than a fixed identifier or `$var`. `$obj->${expr}`
                    // and `$obj->$$expr` (the name comes from a
                    // variable-variable instead of a plain `$var`) are
                    // treated the same way - the exact double-indirection
                    // isn't modeled, matching how the plain `$obj->$prop`
                    // case above already just uses the variable's own name
                    // rather than its value.
                    let dynamic_name =
                        if self.check(&Token::LBrace) || self.check(&Token::DollarBrace) {
                            self.skip();
                            let e = self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            Some(e)
                        } else if self.check(&Token::Dollar) {
                            self.skip();
                            Some(self.parse_primary()?)
                        } else {
                            None
                        };
                    if let Some(name_expr) = dynamic_name {
                        expr = if self.check(&Token::LParen) {
                            let (args, is_fcc) = self.parse_args()?;
                            let call = self.expr(
                                ExprKind::DynamicMethodCall(
                                    Box::new(expr),
                                    Box::new(name_expr),
                                    args,
                                ),
                                start,
                            );
                            if is_fcc {
                                self.expr(ExprKind::FirstClassCallable(Box::new(call)), start)
                            } else {
                                call
                            }
                        } else {
                            self.expr(
                                ExprKind::DynamicPropertyAccess(
                                    Box::new(expr),
                                    Box::new(name_expr),
                                ),
                                start,
                            )
                        };
                        continue;
                    }
                    // A method/property name accepts any keyword too
                    // (`$obj->match()`, `$obj->new`) - PHP only reserves
                    // these in declaration position, not here.
                    let name = match self.peek() {
                        Token::Identifier(_) | Token::Keyword(_) => {
                            self.take_name_segment().unwrap()
                        }
                        Token::Variable(_) => {
                            // $obj->$prop dynamic access, best-effort.
                            let Token::Variable(s) = self.take_token() else {
                                unreachable!()
                            };
                            s
                        }
                        other => {
                            return Err(ParseError {
                                message: format!(
                                    "parse error: expected property/method name, found {:?}",
                                    other
                                ),
                                span: self.spans.get(self.pos).copied().unwrap_or_default(),
                            });
                        }
                    };
                    if self.check(&Token::LParen) {
                        let (args, is_fcc) = self.parse_args()?;
                        let call = self.expr(
                            ExprKind::MethodCall(Box::new(expr), name, args, nullsafe),
                            start,
                        );
                        expr = if is_fcc {
                            self.expr(ExprKind::FirstClassCallable(Box::new(call)), start)
                        } else {
                            call
                        };
                    } else {
                        expr = self.expr(
                            ExprKind::PropertyAccess(Box::new(expr), name, nullsafe),
                            start,
                        );
                    }
                }
                Token::DoubleColon => {
                    self.skip();
                    let receiver_expr = expr;
                    // `class` (`Foo::class`) already worked as a special
                    // case; any other keyword is just as valid here
                    // (`Foo::new()`, `Foo::default()`, `Foo::list()`).
                    let member = match self.peek() {
                        Token::Identifier(_) | Token::Keyword(_) => {
                            self.take_name_segment().unwrap().to_string()
                        }
                        Token::Variable(_) => {
                            let Token::Variable(s) = self.take_token() else {
                                unreachable!()
                            };
                            format!("${s}")
                        }
                        // `Foo::{$name}` / `Foo::${$name}` / `Foo::$$name`
                        // - a fully dynamic static member name.
                        // StaticAccess/StaticCall aren't given any deeper
                        // type-checking today regardless of whether the
                        // name is static or dynamic, so this is just
                        // parsed (consuming the name expression correctly)
                        // rather than threading a real Expr through here.
                        Token::LBrace | Token::DollarBrace => {
                            self.skip();
                            self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            "{dynamic}".to_string()
                        }
                        Token::Dollar => {
                            self.skip();
                            self.parse_primary()?;
                            "{dynamic}".to_string()
                        }
                        other => {
                            return Err(ParseError {
                                message: format!(
                                    "parse error: expected static member, found {:?}",
                                    other
                                ),
                                span: self.spans.get(self.pos).copied().unwrap_or_default(),
                            });
                        }
                    };
                    if self.check(&Token::LParen) {
                        let (args, is_fcc) = self.parse_args()?;
                        let target = match &receiver_expr.kind {
                            ExprKind::Name(n) => StaticTarget::Name(n.clone(), receiver_expr.span),
                            _ => StaticTarget::Expr(Box::new(receiver_expr)),
                        };
                        let call = self.expr(ExprKind::StaticCall(target, member, args), start);
                        expr = if is_fcc {
                            self.expr(ExprKind::FirstClassCallable(Box::new(call)), start)
                        } else {
                            call
                        };
                    } else {
                        let class_name = match &receiver_expr.kind {
                            ExprKind::Name(n) => n.clone(),
                            _ => "?".to_string(),
                        };
                        expr = self.expr(ExprKind::StaticAccess(class_name, member), start);
                    }
                }
                Token::LBracket => {
                    self.skip();
                    let index = if self.check(&Token::RBracket) {
                        None
                    } else {
                        Some(Box::new(self.parse_expression()?))
                    };
                    self.expect(&Token::RBracket)?;
                    expr = self.expr(ExprKind::Index(Box::new(expr), index), start);
                }
                Token::LParen => {
                    let (args, is_fcc) = self.parse_args()?;
                    let call = self.expr(ExprKind::Call(Box::new(expr), args), start);
                    expr = if is_fcc {
                        self.expr(ExprKind::FirstClassCallable(Box::new(call)), start)
                    } else {
                        call
                    };
                }
                Token::Increment => {
                    self.skip();
                    expr = self.expr(ExprKind::Unary(UnaryOp::PostInc, Box::new(expr)), start);
                }
                Token::Decrement => {
                    self.skip();
                    expr = self.expr(ExprKind::Unary(UnaryOp::PostDec, Box::new(expr)), start);
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Returns the parsed arguments plus whether this was a first-class
    /// callable reference (`foo(...)` - literally `...` then nothing else,
    /// as opposed to a real spread argument like `foo(...$rest)`, which is
    /// followed by an expression, not directly by `)`).
    /// Returns `(args, is_first_class_callable)`. Each `Arg` captures its
    /// own `name:` label (a named argument, `name: expr`) and `...`
    /// spread marker - needed so a named/reordered-named/spread call
    /// isn't checked as if its arguments were positional in written
    /// order.
    pub(crate) fn parse_args(&mut self) -> Result<(Vec<Arg>, bool), ParseError> {
        self.expect(&Token::LParen)?;
        if matches!(self.peek(), Token::Ellipsis) && matches!(self.peek_at(1), Token::RParen) {
            self.skip();
            self.skip();
            return Ok((Vec::new(), true));
        }

        let mut args = Vec::new();
        while !self.check(&Token::RParen) {
            let spread = self.eat(&Token::Ellipsis);
            // named arguments (`name: expr`) - the name is just a label,
            // so any keyword is valid here too (`create(namespace: 'x')`),
            // not just Identifier.
            let name = if matches!(self.peek(), Token::Identifier(_) | Token::Keyword(_))
                && self.peek_at(1) == &Token::Colon
            {
                let name = self.take_name_segment().unwrap();
                self.skip();
                Some(name.to_string())
            } else {
                None
            };
            let value = self.parse_expression()?;
            args.push(Arg {
                name,
                spread,
                value,
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen)?;
        Ok((args, false))
    }
}
