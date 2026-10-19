use crate::ast::*;
use crate::parser::{NameKind, ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser<'_> {
    pub(crate) fn parse_array_items(
        &mut self,
        close: &Token,
    ) -> Result<Vec<ArrayItem>, ParseError> {
        let mut items = Vec::new();
        while !self.check(close) {
            // A skipped slot (`[, $b] = $arr;`), legal only in a
            // destructuring target. A placeholder stands in for it until
            // the array becomes one.
            if self.check(&Token::Comma) {
                let span = self.current_span();
                let placeholder = self.expr(ExprKind::Literal(Literal::Null), span.start);
                self.pattern_only.push((
                    placeholder.id,
                    ParseError {
                        message: "Cannot use empty array elements in arrays".to_string(),
                        span,
                    },
                ));
                items.push(ArrayItem {
                    key: None,
                    value: placeholder,
                    spread: false,
                    by_ref: false,
                });
                self.skip();
                continue;
            }
            let spread = self.eat(&Token::Ellipsis);
            let amp_span = self.current_span();
            let by_ref = self.eat(&Token::Amp);
            if spread && by_ref {
                return Err(ParseError {
                    message: "syntax error, unexpected token \"&\"".to_string(),
                    span: amp_span,
                });
            }
            let first = self.parse_expression()?;
            let arrow_span = self.current_span();
            if self.eat(&Token::FatArrow) {
                // `...$a` has no key.
                if spread || by_ref {
                    return Err(ParseError {
                        message: "syntax error, unexpected token \"=>\"".to_string(),
                        span: arrow_span,
                    });
                }
                let by_ref = self.eat(&Token::Amp);
                let value = self.parse_expression()?;
                items.push(ArrayItem {
                    key: Some(first),
                    value,
                    spread,
                    by_ref,
                });
            } else {
                items.push(ArrayItem {
                    key: None,
                    value: first,
                    spread,
                    by_ref,
                });
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(close)?;
        Ok(items)
    }

    /// What follows `new`: a class name, `(expr)`, or a variable with
    /// property, static-property and offset accesses chained onto it (PHP's
    /// `new_variable`), never a call, so `new $this->model($v)`
    /// instantiates the class `$this->model` names and `($v)` is the
    /// constructor's arguments.
    pub(crate) fn parse_new_class_reference(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut expr = match self.peek() {
            Token::Variable(_) | Token::Dollar | Token::DollarBrace => self.parse_primary()?,
            // `new ('std' . $class)`: the parenthesized expression is the
            // whole reference.
            Token::LParen => {
                self.skip();
                let e = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                return Ok(self.expr(ExprKind::Grouping(Box::new(e)), start));
            }
            _ => {
                let name = self.parse_name_reference();
                self.expr(ExprKind::Name(name), start)
            }
        };
        loop {
            match self.peek() {
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
                Token::Arrow | Token::NullsafeArrow => {
                    let nullsafe = matches!(self.peek(), Token::NullsafeArrow);
                    self.skip();
                    expr = match self.peek() {
                        Token::LBrace => {
                            self.skip();
                            let name = self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            self.expr(
                                ExprKind::DynamicPropertyAccess(
                                    Box::new(expr),
                                    Box::new(name),
                                    ObjectOperator::from_nullsafe(nullsafe),
                                ),
                                start,
                            )
                        }
                        Token::Variable(_) | Token::Dollar | Token::DollarBrace => {
                            let name = self.parse_primary()?;
                            self.expr(
                                ExprKind::DynamicPropertyAccess(
                                    Box::new(expr),
                                    Box::new(name),
                                    ObjectOperator::from_nullsafe(nullsafe),
                                ),
                                start,
                            )
                        }
                        _ => {
                            let name = self.expect_ident_spanned()?;
                            self.expr(
                                ExprKind::PropertyAccess(
                                    Box::new(expr),
                                    name,
                                    ObjectOperator::from_nullsafe(nullsafe),
                                ),
                                start,
                            )
                        }
                    };
                }
                Token::DoubleColon
                    if matches!(
                        self.peek_at(1),
                        Token::Variable(_) | Token::Dollar | Token::DollarBrace
                    ) =>
                {
                    self.skip();
                    let target = match &expr.kind {
                        ExprKind::Name(n) => StaticTarget::Name(Box::new(n.clone())),
                        _ => StaticTarget::Expr(Box::new(expr)),
                    };
                    expr = if matches!(self.peek(), Token::Variable(_)) {
                        let var_start = self.current_pos();
                        let Token::Variable(name) = self.take_token() else {
                            unreachable!()
                        };
                        let ident = Ident {
                            name: self.interner.name(&name),
                            span: Span {
                                start: var_start,
                                end: self.previous_end(),
                            },
                        };
                        self.expr(ExprKind::StaticPropertyFetch(target, ident), start)
                    } else {
                        let name = self.parse_primary()?;
                        self.expr(
                            ExprKind::DynamicStaticPropertyFetch(target, Box::new(name)),
                            start,
                        )
                    };
                }
                _ => return Ok(expr),
            }
        }
    }

    pub(crate) fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        // Matches on `self.peek()` by reference rather than `self.peek()
        // .clone()` - most arms below need no payload at all (delegating
        // or just checking which variant), and the few that do
        // (`StringLit`/`Variable`/`InterpString`/`ShellExec`, all
        // heap-owning) use `take_token()` (a move) inside their own arm
        // instead of a whole-`Token` clone up front.
        Ok(match self.peek() {
            Token::Integer(n) => {
                let n = *n;
                self.skip();
                self.expr(ExprKind::Literal(Literal::Int(n)), start)
            }
            Token::Float(f) => {
                let f = *f;
                self.skip();
                self.expr(ExprKind::Literal(Literal::Float(f)), start)
            }
            Token::StringLit(_) => {
                let Token::StringLit(s) = self.take_token() else {
                    unreachable!()
                };
                self.expr(ExprKind::Literal(Literal::String(s)), start)
            }
            Token::Keyword(Keyword::True) => {
                self.skip();
                self.expr(ExprKind::Literal(Literal::Bool(true)), start)
            }
            Token::Keyword(Keyword::False) => {
                self.skip();
                self.expr(ExprKind::Literal(Literal::Bool(false)), start)
            }
            Token::Keyword(Keyword::Null) => {
                self.skip();
                self.expr(ExprKind::Literal(Literal::Null), start)
            }
            Token::Variable(_) => {
                let Token::Variable(name) = self.take_token() else {
                    unreachable!()
                };
                self.expr(ExprKind::Variable(name), start)
            }
            Token::DollarBrace => {
                self.skip();
                let name_expr = self.parse_expression()?;
                self.expect(&Token::RBrace)?;
                self.expr(ExprKind::DynamicVariable(Box::new(name_expr)), start)
            }
            // `$$name` (equivalent to `${$name}`) - recurses to allow
            // `$$$name` chains, though that's vanishingly rare in
            // practice.
            Token::Dollar => {
                self.skip();
                let name_expr = stack::guard(|| self.parse_primary())?;
                self.expr(ExprKind::DynamicVariable(Box::new(name_expr)), start)
            }
            Token::LParen => {
                self.skip();
                let inner = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                self.expr(ExprKind::Grouping(Box::new(inner)), start)
            }
            Token::LBracket => {
                self.skip();
                let items = self.parse_array_items(&Token::RBracket)?;
                self.expr(ExprKind::ArrayLiteral(items), start)
            }
            Token::Keyword(Keyword::Array) => {
                self.skip();
                self.expect(&Token::LParen)?;
                let items = self.parse_array_items(&Token::RParen)?;
                let array = self.expr(ExprKind::ArrayLiteral(items), start);
                self.long_arrays.insert(array.id);
                array
            }
            // Only ever a destructuring target, so its items are taken as
            // pattern items straight away. Whether it stands where a
            // target can is known once what follows is.
            Token::Keyword(Keyword::List) => {
                self.skip();
                self.expect(&Token::LParen)?;
                let items = self.parse_array_items(&Token::RParen)?;
                let span = Span {
                    start,
                    end: self.previous_end(),
                };
                let items = self.destructure_items(items, DestructureSyntax::List, span)?;
                let list = self.expr(ExprKind::Destructure(items, DestructureSyntax::List), start);
                self.pattern_only.push((
                    list.id,
                    ParseError {
                        message: format!(
                            "syntax error, unexpected {:?}, expecting \"=\"",
                            self.peek()
                        ),
                        span: self.current_span(),
                    },
                ));
                list
            }
            Token::Keyword(Keyword::New) => {
                self.skip();
                // A `#[Attr]` group can only start a `new` expression for
                // an anonymous class (`new #[Attr] class {}`) - no other
                // `new` shape (`new Foo(...)`, `new $var(...)`, `new
                // (expr)(...)`) allows one here, so seeing one is
                // unambiguous even before `at_anonymous_class_start`'s own
                // (attribute-unaware) modifiers/`class` lookahead applies.
                if self.check(&Token::AttributeStart) || self.at_anonymous_class_start() {
                    return self.parse_anonymous_class_new(start);
                }
                let class_expr = self.parse_new_class_reference()?;
                // `new Foo(...)` parses but is a compile error in PHP (a
                // constructor can't be referenced as a first-class
                // callable) - kept as a `FirstClassCallable` so the
                // checker can report it.
                let (args, is_first_class_callable) = if self.check(&Token::LParen) {
                    self.parse_args()?
                } else {
                    (Vec::new(), false)
                };
                let new_expr = self.expr(ExprKind::New(Box::new(class_expr), args), start);
                if is_first_class_callable {
                    self.expr(ExprKind::FirstClassCallable(Box::new(new_expr)), start)
                } else {
                    new_expr
                }
            }
            Token::Keyword(Keyword::Clone) => {
                self.skip();
                // `clone(...)` - the PHP 8.1 first-class-callable syntax
                // applies to `clone` too (`$f = clone(...); $f($obj);`),
                // even though it isn't parsed via the general call/args
                // path every other first-class-callable goes through.
                if matches!(self.peek(), Token::LParen)
                    && matches!(self.peek_at(1), Token::Ellipsis)
                    && matches!(self.peek_at(2), Token::RParen)
                {
                    let clone_span = self.span_from(start);
                    self.skip();
                    self.skip();
                    self.skip();
                    let name = self.expr(
                        ExprKind::Name(self.qualified_name(
                            self.interner.name("clone"),
                            clone_span,
                            NameKind::Constant,
                        )),
                        start,
                    );
                    return Ok(self.expr(ExprKind::FirstClassCallable(Box::new(name)), start));
                }
                // PHP 8.5's `clone($obj, $overrides)` - a distinct grammar
                // production (it does NOT allow a postfix chain right
                // after it, unlike the classic form) from plain `clone
                // <expr>`, chosen
                // whenever what's in the parens can't be a single bare
                // expression (a comma, named argument, or spread appears).
                // `clone($x)` alone is just the classic unary form spelled
                // with parens, and still binds through the full postfix
                // chain as its operand - e.g. `clone($a)->b` clones
                // `$a->b`, not `$a`. Parsed with the same general
                // parse_args() every ordinary call uses (named args,
                // spread, trailing commas and all) - only the first two
                // resulting args are kept, matching how the RFC only ever
                // gives this two real parameters.
                if matches!(self.peek(), Token::LParen) && self.clone_call_has_top_level_comma() {
                    let (mut args, _) = self.parse_args()?;
                    let overrides = if args.len() > 1 {
                        Some(Box::new(args.remove(1).value))
                    } else {
                        None
                    };
                    let obj = args
                        .into_iter()
                        .next()
                        .map(|a| a.value)
                        .unwrap_or_else(|| self.expr(ExprKind::Literal(Literal::Null), start));
                    return Ok(self.expr(ExprKind::Clone(Box::new(obj), overrides), start));
                }
                let e = self.parse_unary()?;
                self.expr(ExprKind::Clone(Box::new(e), None), start)
            }
            Token::Keyword(Keyword::Print) => {
                self.skip();
                let e = self.parse_assignment()?;
                self.expr(ExprKind::Print(Box::new(e)), start)
            }
            // `throw` as an expression (`default => throw new X()`, `$x ??
            // throw new X()`, an arrow function body, ...) - a bare `throw
            // expr;` statement is intercepted earlier, in parse_statement,
            // so this is only reached when throw appears nested inside
            // another expression. Its operand is the whole expression to
            // its right, `or` included, as PHP's grammar has it.
            Token::Keyword(Keyword::Throw) => {
                self.skip();
                let inner = self.parse_expression()?;
                self.expr(ExprKind::Throw(Box::new(inner)), start)
            }
            // `yield`/`yield from` - makes the enclosing function a
            // generator.
            Token::Keyword(Keyword::Yield) => {
                self.skip();
                self.note_yield();
                if matches!(self.peek(), Token::Identifier(name) if name.eq_ignore_ascii_case("from"))
                {
                    self.skip();
                    let inner = self.parse_assignment()?;
                    return Ok(self.expr(ExprKind::YieldFrom(Box::new(inner)), start));
                }
                // A bare `yield` (no value): whatever follows can't start an
                // expression (a closer, `:`, `?`, a binary operator), so
                // don't try to parse one; the loop over the operators
                // picks up a token like `*` as an ordinary binary operator
                // applied to the bare yield, e.g. `yield * -1` parses as
                // `(yield) * (-1)`. `+`/`-` *can* start an expression (a
                // unary-prefixed value), so `yield +1` is `yield` of `+1`.
                if !self.can_begin_expression() {
                    return Ok(self.expr(ExprKind::Yield(None, None), start));
                }
                let first = self.parse_ternary()?;
                if self.eat(&Token::FatArrow) {
                    let value = self.parse_ternary()?;
                    self.expr(
                        ExprKind::Yield(Some(Box::new(first)), Some(Box::new(value))),
                        start,
                    )
                } else {
                    self.expr(ExprKind::Yield(None, Some(Box::new(first))), start)
                }
            }
            Token::Keyword(
                kw @ (Keyword::Include
                | Keyword::IncludeOnce
                | Keyword::Require
                | Keyword::RequireOnce),
            ) => {
                let kind = match kw {
                    Keyword::Include => IncludeKind::Include,
                    Keyword::IncludeOnce => IncludeKind::IncludeOnce,
                    Keyword::Require => IncludeKind::Require,
                    _ => IncludeKind::RequireOnce,
                };
                self.skip();
                self.saw_global_or_include.set(true);
                let e = self.parse_expression()?;
                self.expr(ExprKind::Include(kind, Box::new(e)), start)
            }
            Token::Keyword(Keyword::Static)
                if matches!(
                    self.peek_at(1),
                    Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Fn)
                ) =>
            {
                self.skip();
                // Note: the resulting closure's own span starts at
                // `function`/`fn`, not this `static` - a small, accepted
                // precision gap (same category as other narrow, honestly-
                // documented ones in this parser), not worth threading an
                // extra parameter through 3 more functions for.
                self.parse_closure_like(true)?
            }
            // `#[Attr] function () {}` / `#[Attr] static fn() => ...` -
            // a closure/arrow function preceded by its own
            // attribute group(s), the only expression-position shape real
            // PHP allows one on at all. `parse_closure_like`/
            // `parse_closure`/`parse_arrow_fn` build the node with an
            // empty `attrs` of their own (they have no idea any attributes
            // preceded them), overwritten here once the real node comes
            // back.
            Token::AttributeStart => {
                let attrs = self.parse_attributes()?;
                let is_static = self.eat(&Token::Keyword(Keyword::Static));
                let mut e = self.parse_closure_like(is_static)?;
                match &mut e.kind {
                    ExprKind::Closure(closure) => closure.attrs = attrs,
                    ExprKind::ArrowFn(arrow) => arrow.attrs = attrs,
                    _ => {}
                }
                e
            }
            Token::Keyword(Keyword::Static) => {
                self.skip();
                self.expr(
                    ExprKind::Name(self.qualified_name(
                        self.interner.name("static"),
                        self.span_from(start),
                        self.name_kind_ahead(),
                    )),
                    start,
                )
            }
            Token::Keyword(Keyword::Function) => self.parse_closure_like(false)?,
            // `fn` is only reserved as the arrow-function keyword when
            // it's actually followed by its param list, optionally after
            // a by-reference-return `&` (`fn&(...) => ...`) - `function`
            // is fully reserved (an arrow function's own inner `function`
            // keyword doesn't exist, so no ambiguity there), but `fn`
            // alone is legal as a name/namespace segment (`fn\test()`).
            Token::Keyword(Keyword::Fn)
                if matches!(self.peek_at(1), Token::LParen)
                    || (matches!(self.peek_at(1), Token::Amp)
                        && matches!(self.peek_at(2), Token::LParen)) =>
            {
                self.parse_closure_like(false)?
            }
            Token::Keyword(Keyword::Fn) => {
                let name = self.parse_name_reference();
                self.expr(ExprKind::Name(name), start)
            }
            Token::Keyword(Keyword::Match) => self.parse_match_expr()?,
            Token::InterpString(_) => {
                let Token::InterpString(parts) = self.take_token() else {
                    unreachable!()
                };
                self.build_interpolated(parts, start)?
            }
            // `` `cmd $interpolated` `` - shell-exec shorthand.
            Token::ShellExec(_) => {
                let Token::ShellExec(parts) = self.take_token() else {
                    unreachable!()
                };
                let cmd = self.build_interpolated(parts, start)?;
                self.expr(ExprKind::ShellExec(Box::new(cmd)), start)
            }
            // `namespace\Foo` - a name explicitly relative to the current
            // namespace. Only ever appears as a value in expression
            // position (the bare `namespace` statement/block forms are
            // handled separately in parse_statement/parse_program), so
            // it's unambiguous here. `abstract`/`final`/`readonly`/`enum`
            // reach here as a bare function/constant reference
            // (`readonly()`, `enum()`, `A::readonly`) only when
            // parse_statement's own lookahead already ruled out a class/
            // enum declaration - every other keyword with a real
            // expression meaning (new/match/static/...) has its own
            // dedicated arm earlier in this match, so this is unambiguous
            // too.
            Token::Identifier(_)
            | Token::Backslash
            | Token::Keyword(
                Keyword::Namespace
                | Keyword::Abstract
                | Keyword::Final
                | Keyword::Readonly
                | Keyword::Enum,
            ) => {
                let name = self.parse_qualified_name();
                let name_span = self.span_from(start);
                let exit_kind = if name.eq_ignore_ascii_case("exit") {
                    Some(ExitKind::Exit)
                } else if name.eq_ignore_ascii_case("die") {
                    Some(ExitKind::Die)
                } else {
                    None
                };
                if let Some(kind) = exit_kind {
                    // `exit`, `exit()`, `exit($status)`, and PHP 8.4's
                    // first-class callable `exit(...)`, a reference to the
                    // function.
                    if self.check(&Token::LParen) {
                        let (args, is_first_class_callable) = self.parse_args()?;
                        if is_first_class_callable {
                            let callee = self.expr(
                                ExprKind::Name(self.qualified_name(
                                    name,
                                    name_span,
                                    NameKind::Function,
                                )),
                                start,
                            );
                            let call = self.expr(ExprKind::Call(Box::new(callee), args), start);
                            self.expr(ExprKind::FirstClassCallable(Box::new(call)), start)
                        } else {
                            self.expr(ExprKind::Exit(kind, Some(args)), start)
                        }
                    } else {
                        self.expr(ExprKind::Exit(kind, None), start)
                    }
                } else if name.eq_ignore_ascii_case("isset") && self.check(&Token::LParen) {
                    let paren = self.current_span();
                    let (args, is_first_class_callable) = self.parse_args()?;
                    // `isset` takes one or more variables, spelled out.
                    if args.is_empty() || is_first_class_callable {
                        return Err(ParseError {
                            message: "syntax error, unexpected token \")\"".to_string(),
                            span: Span {
                                start: paren.start,
                                end: self.previous_end(),
                            },
                        });
                    }
                    let operands = args.into_iter().map(|a| a.value).collect();
                    self.expr(ExprKind::Isset(operands), start)
                } else if name.eq_ignore_ascii_case("empty") && self.check(&Token::LParen) {
                    let paren = self.current_span();
                    let (args, _) = self.parse_args()?;
                    let Some(operand) = args.into_iter().next() else {
                        return Err(ParseError {
                            message: "parse error: empty() needs an operand".to_string(),
                            span: paren,
                        });
                    };
                    self.expr(ExprKind::Empty(Box::new(operand.value)), start)
                } else if name.eq_ignore_ascii_case("eval") && self.check(&Token::LParen) {
                    let paren = self.current_span();
                    let (args, _) = self.parse_args()?;
                    let Some(code) = args.into_iter().next() else {
                        return Err(ParseError {
                            message: "parse error: eval() needs the code to evaluate".to_string(),
                            span: paren,
                        });
                    };
                    self.expr(ExprKind::Eval(Box::new(code.value)), start)
                } else {
                    let kind = self.name_kind_ahead();
                    self.expr(
                        ExprKind::Name(self.qualified_name(name, name_span, kind)),
                        start,
                    )
                }
            }
            other => {
                return Err(ParseError {
                    message: format!(
                        "parse error: unexpected token in expression: {:?} at {}",
                        other, self.pos
                    ),
                    span: self.current_span(),
                });
            }
        })
    }
}
