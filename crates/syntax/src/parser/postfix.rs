use crate::name::Name;

use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Span, Token};

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
                    // A computed member name: `$obj->{expr}` is `expr`,
                    // `$obj->$var` the value of `$var`, `$obj->$$var` and
                    // `$obj->${expr}` the value of the variable they name.
                    let dynamic_name = if self.check(&Token::LBrace) {
                        self.skip();
                        let e = self.parse_expression()?;
                        self.expect(&Token::RBrace)?;
                        Some(e)
                    } else if self.check(&Token::DollarBrace) {
                        let brace_start = self.current_pos();
                        self.skip();
                        let e = self.parse_expression()?;
                        self.expect(&Token::RBrace)?;
                        Some(self.expr(ExprKind::DynamicVariable(Box::new(e)), brace_start))
                    } else if matches!(self.peek(), Token::Dollar | Token::Variable(_)) {
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
                                    ObjectOperator::from_nullsafe(nullsafe),
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
                                    ObjectOperator::from_nullsafe(nullsafe),
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
                        Token::Identifier(_) | Token::Keyword(_) => self.take_ident().unwrap(),
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
                            ExprKind::MethodCall(
                                Box::new(expr),
                                name,
                                args,
                                ObjectOperator::from_nullsafe(nullsafe),
                            ),
                            start,
                        );
                        expr = if is_fcc {
                            self.expr(ExprKind::FirstClassCallable(Box::new(call)), start)
                        } else {
                            call
                        };
                    } else {
                        expr = self.expr(
                            ExprKind::PropertyAccess(
                                Box::new(expr),
                                name,
                                ObjectOperator::from_nullsafe(nullsafe),
                            ),
                            start,
                        );
                    }
                }
                Token::DoubleColon => {
                    self.skip();
                    let receiver_expr = expr;
                    let target = match &receiver_expr.kind {
                        ExprKind::Name(n) => StaticTarget::Name(n.clone(), receiver_expr.span),
                        _ => StaticTarget::Expr(Box::new(receiver_expr)),
                    };
                    // `class` (`Foo::class`) already worked as a special
                    // case; any other keyword is just as valid here
                    // (`Foo::new()`, `Foo::default()`, `Foo::list()`).
                    let member = match self.peek() {
                        Token::Identifier(_) | Token::Keyword(_) => {
                            StaticMember::Name(self.take_ident().unwrap())
                        }
                        Token::Variable(_) => {
                            let var_start = self.current_pos();
                            let Token::Variable(s) = self.take_token() else {
                                unreachable!()
                            };
                            let span = Span {
                                start: var_start,
                                end: self.previous_end(),
                            };
                            StaticMember::Variable(s, span)
                        }
                        // `Foo::{$name}` - a computed constant name, or
                        // with `(...)` a computed method name.
                        Token::LBrace if !matches!(self.peek_at(1), Token::RBrace) => {
                            self.skip();
                            let name = self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            if !self.check(&Token::LParen) {
                                expr = self.expr(
                                    ExprKind::DynamicClassConstFetch(target, Box::new(name)),
                                    start,
                                );
                                continue;
                            }
                            StaticMember::Dynamic(name)
                        }
                        // `Foo::${expr}` / `Foo::$$name` - a computed
                        // static property name.
                        Token::LBrace | Token::DollarBrace => {
                            self.skip();
                            let name = self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            StaticMember::Dynamic(name)
                        }
                        Token::Dollar => {
                            self.skip();
                            StaticMember::Dynamic(self.parse_primary()?)
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
                        let kind = match member {
                            StaticMember::Name(n) => ExprKind::StaticCall(target, n, args),
                            StaticMember::Variable(v, span) => {
                                let name = Expr {
                                    id: self.node_id(),
                                    kind: ExprKind::Variable(v),
                                    span,
                                };
                                ExprKind::DynamicStaticCall(target, Box::new(name), args)
                            }
                            StaticMember::Dynamic(name) => {
                                ExprKind::DynamicStaticCall(target, Box::new(name), args)
                            }
                        };
                        let call = self.expr(kind, start);
                        expr = if is_fcc {
                            self.expr(ExprKind::FirstClassCallable(Box::new(call)), start)
                        } else {
                            call
                        };
                    } else {
                        let kind = match member {
                            StaticMember::Name(n) if n.eq_ignore_ascii_case("class") => {
                                ExprKind::ClassNameFetch(target)
                            }
                            StaticMember::Name(n) => ExprKind::ClassConstFetch(target, n),
                            StaticMember::Variable(name, span) => {
                                ExprKind::StaticPropertyFetch(target, Ident { name, span })
                            }
                            StaticMember::Dynamic(name) => {
                                ExprKind::DynamicStaticPropertyFetch(target, Box::new(name))
                            }
                        };
                        expr = self.expr(kind, start);
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
                let name = self.take_ident().unwrap();
                self.skip();
                Some(name)
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

/// What follows `::` in a static member access or call.
enum StaticMember {
    Name(Ident),
    /// `$name`, and where it is.
    Variable(Name, Span),
    Dynamic(Expr),
}
