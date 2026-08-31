use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Token};

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
                    self.advance();
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
                            self.advance();
                            let e = self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            Some(e)
                        } else if self.check(&Token::Dollar) {
                            self.advance();
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
                    let name = match self.peek().clone() {
                        Token::Identifier(_) | Token::Keyword(_) => {
                            self.take_name_segment().unwrap()
                        }
                        Token::Variable(s) => {
                            self.advance();
                            s // $obj->$prop dynamic access, best-effort
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
                    self.advance();
                    let class_name = match &expr.kind {
                        ExprKind::Name(n) => n.clone(),
                        _ => "?".to_string(),
                    };
                    // `class` (`Foo::class`) already worked as a special
                    // case; any other keyword is just as valid here
                    // (`Foo::new()`, `Foo::default()`, `Foo::list()`).
                    let member = match self.peek().clone() {
                        Token::Identifier(_) | Token::Keyword(_) => {
                            self.take_name_segment().unwrap()
                        }
                        Token::Variable(s) => {
                            self.advance();
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
                            self.advance();
                            self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            "{dynamic}".to_string()
                        }
                        Token::Dollar => {
                            self.advance();
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
                        let call = self.expr(ExprKind::StaticCall(class_name, member, args), start);
                        expr = if is_fcc {
                            self.expr(ExprKind::FirstClassCallable(Box::new(call)), start)
                        } else {
                            call
                        };
                    } else {
                        expr = self.expr(ExprKind::StaticAccess(class_name, member), start);
                    }
                }
                Token::LBracket => {
                    self.advance();
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
                    self.advance();
                    expr = self.expr(ExprKind::Unary(UnaryOp::PostInc, Box::new(expr)), start);
                }
                Token::Decrement => {
                    self.advance();
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
    /// own `name:` label (a named argument, `name: expr`) and `...` spread
    /// marker - previously both were parsed then immediately discarded,
    /// silently making every named/reordered-named/spread call get
    /// checked as if its arguments were positional in written order (see
    /// Item 1's own design notes for the real bug this caused).
    pub(crate) fn parse_args(&mut self) -> Result<(Vec<Arg>, bool), ParseError> {
        self.expect(&Token::LParen)?;
        if matches!(self.peek(), Token::Ellipsis) && matches!(self.peek_at(1), Token::RParen) {
            self.advance();
            self.advance();
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
                self.advance();
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

    pub(crate) fn parse_array_items(
        &mut self,
        close: &Token,
    ) -> Result<Vec<ArrayItem>, ParseError> {
        let mut items = Vec::new();
        while !self.check(close) {
            // A skipped destructuring slot (`[, $b] = $arr;`, `[$a, , $c] =
            // ...`) - only legal on a list-assignment target, but this
            // parser doesn't distinguish that context from a real array
            // literal here, so it's accepted generically. Null stands in
            // as "nothing to bind"; the checker's destructuring/element
            // walks below already skip anything that isn't a Variable.
            if self.check(&Token::Comma) {
                let pos = self.current_pos();
                items.push(ArrayItem {
                    key: None,
                    value: self.expr(ExprKind::Literal(Literal::Null), pos),
                    spread: false,
                });
                self.advance();
                continue;
            }
            let spread = self.eat(&Token::Ellipsis);
            self.eat(&Token::Amp); // by-ref element, not modeled
            let first = self.parse_expression()?;
            if self.eat(&Token::FatArrow) {
                self.eat(&Token::Amp);
                let value = self.parse_expression()?;
                items.push(ArrayItem {
                    key: Some(first),
                    value,
                    spread,
                });
            } else {
                items.push(ArrayItem {
                    key: None,
                    value: first,
                    spread,
                });
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(close)?;
        Ok(items)
    }

    pub(crate) fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        Ok(match self.peek().clone() {
            Token::Integer(n) => {
                self.advance();
                self.expr(ExprKind::Literal(Literal::Int(n)), start)
            }
            Token::Float(f) => {
                self.advance();
                self.expr(ExprKind::Literal(Literal::Float(f)), start)
            }
            Token::StringLit(s) => {
                self.advance();
                self.expr(ExprKind::Literal(Literal::String(s)), start)
            }
            Token::Keyword(Keyword::True) => {
                self.advance();
                self.expr(ExprKind::Literal(Literal::Bool(true)), start)
            }
            Token::Keyword(Keyword::False) => {
                self.advance();
                self.expr(ExprKind::Literal(Literal::Bool(false)), start)
            }
            Token::Keyword(Keyword::Null) => {
                self.advance();
                self.expr(ExprKind::Literal(Literal::Null), start)
            }
            Token::Variable(name) => {
                self.advance();
                self.expr(ExprKind::Variable(name), start)
            }
            Token::DollarBrace => {
                self.advance();
                let name_expr = self.parse_expression()?;
                self.expect(&Token::RBrace)?;
                self.expr(ExprKind::DynamicVariable(Box::new(name_expr)), start)
            }
            // `$$name` (equivalent to `${$name}`) - recurses to allow
            // `$$$name` chains, though that's vanishingly rare in
            // practice.
            Token::Dollar => {
                self.advance();
                let name_expr = self.parse_primary()?;
                self.expr(ExprKind::DynamicVariable(Box::new(name_expr)), start)
            }
            Token::LParen => {
                self.advance();
                let inner = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                self.expr(ExprKind::Grouping(Box::new(inner)), start)
            }
            Token::LBracket => {
                self.advance();
                let items = self.parse_array_items(&Token::RBracket)?;
                self.expr(ExprKind::ArrayLiteral(items), start)
            }
            Token::Keyword(Keyword::Array) => {
                self.advance();
                self.expect(&Token::LParen)?;
                let items = self.parse_array_items(&Token::RParen)?;
                self.expr(ExprKind::ArrayLiteral(items), start)
            }
            Token::Keyword(Keyword::List) => {
                self.advance();
                self.expect(&Token::LParen)?;
                let items = self.parse_array_items(&Token::RParen)?;
                self.expr(ExprKind::ArrayLiteral(items), start)
            }
            Token::Keyword(Keyword::New) => {
                self.advance();
                if self.at_anonymous_class_start() {
                    return self.parse_anonymous_class_new(start);
                }
                let class_expr = match self.peek().clone() {
                    Token::Variable(name) => {
                        let s = self.current_pos();
                        self.advance();
                        self.expr(ExprKind::Variable(name), s)
                    }
                    // `new ('std' . $class)` - a fully dynamic class
                    // reference. Parsed as exactly one grouped expression
                    // (not the general postfix-chaining primary parse),
                    // so a constructor call's own `(...)` right after it
                    // (`new ('std'.$class)()`) is left for the args check
                    // below, not folded into this expression's postfix
                    // chain.
                    Token::LParen => {
                        let s = self.current_pos();
                        self.advance();
                        let e = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        self.expr(ExprKind::Grouping(Box::new(e)), s)
                    }
                    _ => {
                        let s = self.current_pos();
                        let name = self.parse_qualified_name();
                        self.expr(ExprKind::Name(name), s)
                    }
                };
                // `new Foo(...)` first-class-callable syntax doesn't exist
                // in PHP - a constructor can't be referenced that way - so
                // the flag is simply dropped here rather than handled.
                let args = if self.check(&Token::LParen) {
                    self.parse_args()?.0
                } else {
                    Vec::new()
                };
                self.expr(ExprKind::New(Box::new(class_expr), args), start)
            }
            Token::Keyword(Keyword::Clone) => {
                self.advance();
                // `clone(...)` - the PHP 8.1 first-class-callable syntax
                // applies to `clone` too (`$f = clone(...); $f($obj);`),
                // even though it isn't parsed via the general call/args
                // path every other first-class-callable goes through.
                if matches!(self.peek(), Token::LParen)
                    && matches!(self.peek_at(1), Token::Ellipsis)
                    && matches!(self.peek_at(2), Token::RParen)
                {
                    self.advance();
                    self.advance();
                    self.advance();
                    let name = self.expr(ExprKind::Name("clone".to_string()), start);
                    return Ok(self.expr(ExprKind::FirstClassCallable(Box::new(name)), start));
                }
                // PHP 8.5's `clone($obj, $overrides)` - a distinct grammar
                // production (verified against a real PHP 8.5 build: it
                // does NOT allow a postfix chain right after it, unlike
                // the classic form) from plain `clone <expr>`, chosen
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
                self.advance();
                let e = self.parse_assignment()?;
                let name = self.expr(ExprKind::Name("print".to_string()), start);
                self.expr(
                    ExprKind::Call(
                        Box::new(name),
                        vec![Arg {
                            name: None,
                            spread: false,
                            value: e,
                        }],
                    ),
                    start,
                )
            }
            // `throw` as an expression (`default => throw new X()`, `$x ??
            // throw new X()`, an arrow function body, ...) - a bare `throw
            // expr;` statement is intercepted earlier, in parse_statement,
            // so this is only reached when throw appears nested inside
            // another expression. Handled here in parse_primary, the
            // lowest level, rather than parse_assignment, because `??`'s
            // RHS (and others) recurse directly into a tighter tier and
            // never pass back through parse_assignment on the way down.
            Token::Keyword(Keyword::Throw) => {
                self.advance();
                let inner = self.parse_assignment()?;
                self.expr(ExprKind::Throw(Box::new(inner)), start)
            }
            // `yield`/`yield from` - makes the enclosing function a
            // generator, which isn't modeled, but the yielded key/value
            // expression(s) are still worth parsing and checking.
            Token::Keyword(Keyword::Yield) => {
                self.advance();
                if let Token::Identifier(name) = self.peek().clone()
                    && name.eq_ignore_ascii_case("from")
                {
                    self.advance();
                    let inner = self.parse_assignment()?;
                    return Ok(self.expr(ExprKind::YieldFrom(Box::new(inner)), start));
                }
                // A bare `yield` (no value) - whatever follows can't start
                // an expression, so don't try to parse one; the caller
                // (already unwinding back up the precedence chain from
                // this primary position) picks up a token like `*` as an
                // ordinary binary operator applied to the bare yield, e.g.
                // `yield * -1` parses as `(yield) * (-1)`. `+`/`-` are
                // deliberately not listed here - unlike `*`, they *can*
                // start an expression (a unary-prefixed value), so
                // `yield +1` is `yield` of `+1`, not a bare yield followed
                // by unary `+1`.
                if matches!(
                    self.peek(),
                    Token::Semicolon
                        | Token::RParen
                        | Token::RBracket
                        | Token::RBrace
                        | Token::Comma
                        | Token::Star
                ) {
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
            // include/require/*_once are language constructs, not function
            // calls (no parens required: `include $file;`), but modeling
            // them as a call to a synthetic name works fine here - an
            // unregistered function name is already just unchecked, not a
            // parse failure.
            Token::Keyword(
                kw @ (Keyword::Include
                | Keyword::IncludeOnce
                | Keyword::Require
                | Keyword::RequireOnce),
            ) => {
                self.advance();
                let e = self.parse_assignment()?;
                let name = self.expr(ExprKind::Name(kw.as_str().to_string()), start);
                self.expr(
                    ExprKind::Call(
                        Box::new(name),
                        vec![Arg {
                            name: None,
                            spread: false,
                            value: e,
                        }],
                    ),
                    start,
                )
            }
            Token::Keyword(Keyword::Static)
                if matches!(
                    self.peek_at(1),
                    Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Fn)
                ) =>
            {
                self.advance();
                // Note: the resulting closure's own span starts at
                // `function`/`fn`, not this `static` - a small, accepted
                // precision gap (same category as other narrow, honestly-
                // documented ones in this parser), not worth threading an
                // extra parameter through 3 more functions for.
                self.parse_closure_like(true)?
            }
            Token::Keyword(Keyword::Static) => {
                self.advance();
                self.expr(ExprKind::Name("static".to_string()), start)
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
                let name = self.parse_qualified_name();
                self.expr(ExprKind::Name(name), start)
            }
            Token::Keyword(Keyword::Match) => self.parse_match_expr()?,
            Token::InterpString(parts) => {
                self.advance();
                self.build_interpolated(parts, start)?
            }
            // `` `cmd $interpolated` `` - shell-exec shorthand. Modeled as
            // a call to shell_exec() on the same interpolated-string value
            // a double-quoted string with identical content would build,
            // rather than adding a dedicated AST node for it.
            Token::ShellExec(parts) => {
                self.advance();
                let cmd = self.build_interpolated(parts, start)?;
                let name = self.expr(ExprKind::Name("shell_exec".to_string()), start);
                self.expr(
                    ExprKind::Call(
                        Box::new(name),
                        vec![Arg {
                            name: None,
                            spread: false,
                            value: cmd,
                        }],
                    ),
                    start,
                )
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
                self.expr(ExprKind::Name(name), start)
            }
            other => {
                return Err(ParseError {
                    message: format!(
                        "parse error: unexpected token in expression: {:?} at {}",
                        other, self.pos
                    ),
                    span: self.spans.get(self.pos).copied().unwrap_or_default(),
                });
            }
        })
    }

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
                let name = self.expect_variable()?;
                uses.push(ClosureUse {
                    name,
                    by_ref: use_by_ref,
                });
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
        }

        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()?
        } else {
            None
        };

        let body = if let StmtKind::Block(stmts) = self.parse_block()?.kind {
            stmts
        } else {
            unreachable!()
        };

        Ok(self.expr(
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                by_ref,
                return_type,
            },
            start,
        ))
    }

    pub(crate) fn parse_arrow_fn(&mut self, is_static: bool) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Fn))?;
        self.eat(&Token::Amp);
        let params = self.parse_params()?;
        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()?
        } else {
            None
        };
        self.expect(&Token::FatArrow)?;
        let body = self.parse_assignment()?;
        Ok(self.expr(
            ExprKind::ArrowFn {
                params,
                body: Box::new(body),
                is_static,
                return_type,
            },
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
        let mut default = None;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            if self.eat(&Token::Keyword(Keyword::Default)) {
                // `default` also tolerates a trailing comma before `=>`,
                // same as an ordinary arm's condition list.
                self.eat(&Token::Comma);
                self.expect(&Token::FatArrow)?;
                default = Some(Box::new(self.parse_expression()?));
            } else {
                let mut conds = vec![self.parse_expression()?];
                while self.eat(&Token::Comma) {
                    if self.check(&Token::FatArrow) {
                        break;
                    }
                    conds.push(self.parse_expression()?);
                }
                self.expect(&Token::FatArrow)?;
                let result = self.parse_expression()?;
                arms.push((conds, result));
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;

        Ok(self.expr(
            ExprKind::Match {
                subject: Box::new(subject),
                arms,
                default,
            },
            start,
        ))
    }
}
