use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Token};

impl Parser {
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
                self.skip();
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
        // Matches on `self.peek()` by reference rather than `self.peek()
        // .clone()` - most arms below need no payload at all (delegating
        // or just checking which variant), and the few that do
        // (`StringLit`/`Variable`/`InterpString`/`ShellExec`, all
        // heap-owning) use `take_token()` (a move) inside their own arm
        // instead of a whole-`Token` clone up front. Found and fixed via
        // a perf-optimization investigation workflow.
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
                let name_expr = self.parse_primary()?;
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
                self.expr(ExprKind::ArrayLiteral(items), start)
            }
            Token::Keyword(Keyword::List) => {
                self.skip();
                self.expect(&Token::LParen)?;
                let items = self.parse_array_items(&Token::RParen)?;
                self.expr(ExprKind::ArrayLiteral(items), start)
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
                let class_expr = match self.peek() {
                    Token::Variable(_) => {
                        let s = self.current_pos();
                        let Token::Variable(name) = self.take_token() else {
                            unreachable!()
                        };
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
                        self.skip();
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
                self.skip();
                // `clone(...)` - the PHP 8.1 first-class-callable syntax
                // applies to `clone` too (`$f = clone(...); $f($obj);`),
                // even though it isn't parsed via the general call/args
                // path every other first-class-callable goes through.
                if matches!(self.peek(), Token::LParen)
                    && matches!(self.peek_at(1), Token::Ellipsis)
                    && matches!(self.peek_at(2), Token::RParen)
                {
                    self.skip();
                    self.skip();
                    self.skip();
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
                self.skip();
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
                self.skip();
                let inner = self.parse_assignment()?;
                self.expr(ExprKind::Throw(Box::new(inner)), start)
            }
            // `yield`/`yield from` - makes the enclosing function a
            // generator, which isn't modeled, but the yielded key/value
            // expression(s) are still worth parsing and checking.
            Token::Keyword(Keyword::Yield) => {
                self.skip();
                if matches!(self.peek(), Token::Identifier(name) if name.eq_ignore_ascii_case("from"))
                {
                    self.skip();
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
                let kw = kw.clone();
                self.skip();
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
                    ExprKind::Closure { attrs: a, .. } | ExprKind::ArrowFn { attrs: a, .. } => {
                        *a = attrs;
                    }
                    _ => {}
                }
                e
            }
            Token::Keyword(Keyword::Static) => {
                self.skip();
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
            Token::InterpString(_) => {
                let Token::InterpString(parts) = self.take_token() else {
                    unreachable!()
                };
                self.build_interpolated(parts, start)?
            }
            // `` `cmd $interpolated` `` - shell-exec shorthand. Modeled as
            // a call to shell_exec() on the same interpolated-string value
            // a double-quoted string with identical content would build,
            // rather than adding a dedicated AST node for it.
            Token::ShellExec(_) => {
                let Token::ShellExec(parts) = self.take_token() else {
                    unreachable!()
                };
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
}
