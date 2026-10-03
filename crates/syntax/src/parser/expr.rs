use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser<'_> {
    // ---- expressions (precedence climbing) ----

    pub fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_low_precedence_or()
    }

    // `and`/`or`/`xor` bind looser than assignment - the classic PHP
    // gotcha where `$a = $b or $c;` parses as `($a = $b) or $c;`, not
    // `$a = ($b or $c);`, unlike `&&`/`||`. Each tier only recurses into
    // the next-tighter one, bottoming out at parse_assignment (the
    // existing top of the &&/||/ternary/etc chain), so the classic form
    // is unaffected and only these three keywords get the looser binding.
    pub(crate) fn parse_low_precedence_or(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_low_precedence_xor()?;
        while self.eat(&Token::Keyword(Keyword::Or)) {
            let rhs = self.parse_low_precedence_xor()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::LogicalOr, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_low_precedence_xor(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_low_precedence_and()?;
        while self.eat(&Token::Keyword(Keyword::Xor)) {
            let rhs = self.parse_low_precedence_and()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::LogicalXor, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_low_precedence_and(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_assignment()?;
        while self.eat(&Token::Keyword(Keyword::And)) {
            let rhs = self.parse_assignment()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::LogicalAnd, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        crate::stack::guard(|| self.parse_ternary())
    }

    /// An assignment whose target is the operand just parsed. PHP's grammar
    /// has `variable '=' expr` wherever an operand may stand, so the
    /// assignment takes everything to its right whatever operator comes
    /// before it: `false === $x = f()` is `false === ($x = f())`, `$a ?? $b =
    /// 1` is `$a ?? ($b = 1)`, `(int) $v = 5` is `(int) ($v = 5)`. Applied
    /// once, to every operand (`parse_pow`).
    pub(crate) fn parse_assignment_tail(
        &mut self,
        lhs: Expr,
        start: u32,
    ) -> Result<Expr, ParseError> {
        let op = match self.peek() {
            Token::Equals => None,
            Token::PlusEquals => Some(BinOp::Add),
            Token::MinusEquals => Some(BinOp::Sub),
            Token::StarEquals => Some(BinOp::Mul),
            Token::StarStarEquals => Some(BinOp::Pow),
            Token::SlashEquals => Some(BinOp::Div),
            Token::PercentEquals => Some(BinOp::Mod),
            Token::DotEquals => Some(BinOp::Concat),
            Token::QuestionQuestionEquals => Some(BinOp::Coalesce),
            Token::AmpEquals => Some(BinOp::BitAnd),
            Token::PipeEquals => Some(BinOp::BitOr),
            Token::CaretEquals => Some(BinOp::BitXor),
            Token::LtLtEquals => Some(BinOp::Shl),
            Token::GtGtEquals => Some(BinOp::Shr),
            _ => return Ok(lhs),
        };
        let op_span = self.current_span();
        let unexpected = |token: &str| ParseError {
            message: format!("syntax error, unexpected token \"{token}\""),
            span: op_span,
        };
        let is_pattern = matches!(
            lhs.kind,
            ExprKind::ArrayLiteral(_) | ExprKind::Destructure(..)
        );
        // `($a) = 1`: a parenthesized expression isn't a variable, nor is
        // `array(...)`.
        if matches!(lhs.kind, ExprKind::Grouping(_))
            || self.long_arrays.contains(&lhs.id)
            || (is_pattern && op.is_some())
        {
            return Err(unexpected(&format!("{:?}", self.peek())));
        }
        self.skip();
        // Reference assignment (`$a = &$b;`) - only valid on a plain `=`,
        // never a compound one. See `ExprKind::AssignRef`'s own doc
        // comment for why this gets its own variant rather than just
        // being consumed and discarded.
        let amp_span = self.current_span();
        let is_ref = op.is_none() && self.eat(&Token::Amp);
        if is_ref && is_pattern {
            return Err(ParseError {
                message: "syntax error, unexpected token \"&\"".to_string(),
                span: amp_span,
            });
        }
        let lhs = if is_pattern {
            self.as_pattern(lhs, true)?
        } else {
            lhs
        };
        let rhs = self.parse_assignment()?;
        Ok(match op {
            None if is_ref => self.expr(ExprKind::AssignRef(Box::new(lhs), Box::new(rhs)), start),
            None => self.expr(ExprKind::Assign(Box::new(lhs), Box::new(rhs)), start),
            Some(binop) => self.expr(
                ExprKind::CompoundAssign(binop, Box::new(lhs), Box::new(rhs)),
                start,
            ),
        })
    }

    /// `target`, an assignment's or a `foreach`'s, with an array in it
    /// taken as the destructuring pattern it is there. `top` is whether
    /// `target` is the whole target rather than an item of an enclosing
    /// pattern.
    pub(crate) fn as_pattern(&mut self, mut target: Expr, top: bool) -> Result<Expr, ParseError> {
        match &mut target.kind {
            ExprKind::ArrayLiteral(items) => {
                let items = std::mem::take(items);
                if self.long_arrays.contains(&target.id) {
                    return Err(ParseError {
                        message: if top {
                            format!("syntax error, unexpected {:?}", self.peek())
                        } else {
                            "Cannot assign to array(), use [] instead".to_string()
                        },
                        span: if top {
                            self.current_span()
                        } else {
                            target.span
                        },
                    });
                }
                let items = self.destructure_items(items, DestructureSyntax::Short, target.span)?;
                Ok(Expr {
                    id: target.id,
                    kind: ExprKind::Destructure(items, DestructureSyntax::Short),
                    span: target.span,
                })
            }
            ExprKind::Destructure(..) => {
                self.take_pattern_only(target.id);
                Ok(target)
            }
            _ => Ok(target),
        }
    }

    /// The items of a `[...]` or `list(...)` spelled `syntax`, as the
    /// items of the pattern it is, with PHP's compile-time checks on them.
    pub(crate) fn destructure_items(
        &mut self,
        items: Vec<ArrayItem>,
        syntax: DestructureSyntax,
        span: Span,
    ) -> Result<Vec<DestructureItem>, ParseError> {
        let error = |message: &str, span: Span| ParseError {
            message: message.to_string(),
            span,
        };
        let keyed = items.iter().any(|item| item.key.is_some());
        let mut pattern = Vec::with_capacity(items.len());
        for item in items {
            let skipped = matches!(item.value.kind, ExprKind::Literal(Literal::Null))
                && self.take_pattern_only(item.value.id);
            if skipped {
                if keyed {
                    return Err(error(
                        "Cannot use empty array entries in keyed array assignment",
                        item.value.span,
                    ));
                }
                pattern.push(DestructureItem {
                    key: None,
                    target: None,
                    by_ref: false,
                });
                continue;
            }
            if item.spread {
                return Err(error(
                    "Spread operator is not supported in assignments",
                    item.value.span,
                ));
            }
            if item.key.is_some() != keyed {
                return Err(error(
                    "Cannot mix keyed and unkeyed array entries in assignments",
                    item.value.span,
                ));
            }
            let mixed = match (&item.value.kind, syntax) {
                (ExprKind::ArrayLiteral(_), DestructureSyntax::List) => {
                    !self.long_arrays.contains(&item.value.id)
                }
                (ExprKind::Destructure(_, inner), _) => *inner != syntax,
                _ => false,
            };
            if mixed {
                return Err(error("Cannot mix [] and list()", item.value.span));
            }
            pattern.push(DestructureItem {
                key: item.key,
                target: Some(self.as_pattern(item.value, false)?),
                by_ref: item.by_ref,
            });
        }
        if pattern.iter().all(|item| item.target.is_none()) {
            return Err(error("Cannot use empty list", span));
        }
        Ok(pattern)
    }

    /// Drops the error waiting on `id` being part of a destructuring
    /// target; whether there was one.
    fn take_pattern_only(&mut self, id: NodeId) -> bool {
        let before = self.pattern_only.len();
        self.pattern_only.retain(|(pending, _)| *pending != id);
        self.pattern_only.len() != before
    }

    /// The first error still waiting on its node being part of a
    /// destructuring target, now that none can become one.
    pub(crate) fn check_pattern_only(&mut self) -> Result<(), ParseError> {
        match self
            .pattern_only
            .drain(..)
            .map(|(_, error)| error)
            .min_by_key(|error| error.span.start)
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) fn parse_ternary(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let cond = self.parse_coalesce()?;
        if self.eat(&Token::Question) {
            if self.eat(&Token::Colon) {
                let else_branch = self.parse_assignment()?;
                return Ok(self.expr(
                    ExprKind::Ternary(Box::new(cond), None, Box::new(else_branch)),
                    start,
                ));
            }
            let then_branch = self.parse_assignment()?;
            self.expect(&Token::Colon)?;
            let else_branch = self.parse_assignment()?;
            return Ok(self.expr(
                ExprKind::Ternary(
                    Box::new(cond),
                    Some(Box::new(then_branch)),
                    Box::new(else_branch),
                ),
                start,
            ));
        }
        Ok(cond)
    }

    pub(crate) fn parse_coalesce(&mut self) -> Result<Expr, ParseError> {
        crate::stack::guard(|| self.parse_coalesce_inner())
    }

    fn parse_coalesce_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let lhs = self.parse_logical_or()?;
        if self.eat(&Token::QuestionQuestion) {
            let rhs = self.parse_coalesce()?;
            return Ok(self.expr(
                ExprKind::Binary(BinOp::Coalesce, Box::new(lhs), Box::new(rhs)),
                start,
            ));
        }
        Ok(lhs)
    }

    /// The binary operators from `||` down to `*`, each left-associative,
    /// by precedence, loosest first. `|>` (PHP 8.5's pipe: `$value |>
    /// callable` is `callable($value)`) was checked against a real PHP 8.5
    /// build: tighter than relational, looser than shift, concat and
    /// additive; `.` is looser than `<<`/`>>` and `+`/`-` since PHP 8, so
    /// `"a" . $x + 1` is `"a" . ($x + 1)`.
    fn binary_operator(&self) -> Option<(u8, BinOp)> {
        Some(match self.peek() {
            Token::PipePipe => (1, BinOp::Or),
            Token::AmpAmp => (2, BinOp::And),
            Token::Pipe => (3, BinOp::BitOr),
            Token::Caret => (4, BinOp::BitXor),
            Token::Amp => (5, BinOp::BitAnd),
            Token::EqualsEquals => (6, BinOp::Eq),
            Token::EqualsEqualsEquals => (6, BinOp::Identical),
            Token::BangEquals => (6, BinOp::NotEq),
            Token::BangEqualsEquals => (6, BinOp::NotIdentical),
            Token::Spaceship => (6, BinOp::Spaceship),
            Token::Lt => (7, BinOp::Lt),
            Token::Gt => (7, BinOp::Gt),
            Token::LtEquals => (7, BinOp::LtEq),
            Token::GtEquals => (7, BinOp::GtEq),
            Token::PipeGt => (8, BinOp::Pipe),
            Token::Dot => (9, BinOp::Concat),
            Token::LtLt => (10, BinOp::Shl),
            Token::GtGt => (10, BinOp::Shr),
            Token::Plus => (11, BinOp::Add),
            Token::Minus => (11, BinOp::Sub),
            Token::Star => (12, BinOp::Mul),
            Token::Slash => (12, BinOp::Div),
            Token::Percent => (12, BinOp::Mod),
            _ => return None,
        })
    }

    pub(crate) fn parse_logical_or(&mut self) -> Result<Expr, ParseError> {
        self.parse_binary(1)
    }

    /// The operands joined by binary operators of precedence `min` or
    /// tighter.
    fn parse_binary(&mut self, min: u8) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_not()?;
        while let Some((precedence, op)) = self.binary_operator() {
            if precedence < min {
                break;
            }
            self.skip();
            let rhs = self.parse_binary(precedence + 1)?;
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        Ok(lhs)
    }

    /// `!` binds *looser* than `instanceof` in real PHP's own precedence
    /// table (`!$x instanceof Y` means `!($x instanceof Y)`, not
    /// `(!$x) instanceof Y`) - kept as its own production, separate from
    /// the tighter-than-instanceof level `~`/casts/`++`/`--`/`@` share in
    /// `parse_unary_inner`. `apply_narrowing`'s `Unary(Not, Instanceof)`
    /// pattern depends on this: `if (! $x instanceof Y) { ... }` only
    /// narrows `$x` when the parse produces `Unary(Not, Instanceof($x,
    /// Y))`, not `Instanceof(Unary(Not, $x), Y)`.
    pub(crate) fn parse_not(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        if self.eat(&Token::Bang) {
            let operand = crate::stack::guard(|| self.parse_not())?;
            return Ok(self.expr(ExprKind::Unary(UnaryOp::Not, Box::new(operand)), start));
        }
        self.parse_instanceof()
    }

    pub(crate) fn parse_instanceof(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let lhs = self.parse_unary()?;
        if self.eat(&Token::Keyword(Keyword::Instanceof)) {
            // The right-hand side is usually a static class name, but PHP
            // also allows an arbitrary expression there (`$x instanceof
            // $className`, `$x instanceof ('std' . $class)`) for a
            // runtime-determined class.
            let target = if matches!(self.peek(), Token::Variable(_) | Token::LParen) {
                InstanceofTarget::Expr(Box::new(self.parse_unary()?))
            } else {
                InstanceofTarget::Name(self.parse_name_reference())
            };
            return Ok(self.expr(ExprKind::Instanceof(Box::new(lhs), target), start));
        }
        Ok(lhs)
    }

    pub(crate) fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        crate::stack::guard(|| self.parse_unary_inner())
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        if let Some(spelling) = self.try_parse_cast() {
            let operand = self.parse_unary()?;
            return Ok(self.expr(ExprKind::Cast(spelling, Box::new(operand)), start));
        }

        let op = match self.peek() {
            Token::Bang => Some(UnaryOp::Not),
            Token::Minus => Some(UnaryOp::Neg),
            Token::Plus => Some(UnaryOp::Plus),
            Token::Tilde => Some(UnaryOp::BitNot),
            Token::Increment => Some(UnaryOp::PreInc),
            Token::Decrement => Some(UnaryOp::PreDec),
            _ => None,
        };
        if let Some(op) = op {
            self.skip();
            let operand = self.parse_unary()?;
            return Ok(self.expr(ExprKind::Unary(op, Box::new(operand)), start));
        }
        if self.eat(&Token::At) {
            let operand = self.parse_unary()?;
            return Ok(self.expr(ExprKind::ErrorSuppress(Box::new(operand)), start));
        }
        self.parse_pow()
    }

    /// `(int)`, `(string)`, etc: a cast is only distinguishable from a
    /// parenthesized expression by 3-token lookahead - `(`, a known cast
    /// keyword, `)` - with nothing else it could be.
    pub(crate) fn try_parse_cast(&mut self) -> Option<CastSpelling> {
        if !matches!(self.peek(), Token::LParen) {
            return None;
        }
        let spelling = match self.peek_at(1) {
            Token::Identifier(s) => CastSpelling::from_name(s)?,
            // `array` is the one cast name that's also a language keyword
            // (`array(...)`, type hints) rather than a plain identifier -
            // every other cast name (int, string, object, unset, ...)
            // isn't reserved at all.
            Token::Keyword(Keyword::Array) => CastSpelling::Array,
            _ => return None,
        };
        if !matches!(self.peek_at(2), Token::RParen) {
            return None;
        }
        self.skip(); // (
        self.skip(); // type identifier
        self.skip(); // )
        Some(spelling)
    }

    pub(crate) fn parse_pow(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let lhs = self.parse_postfix_from_primary()?;
        let lhs = self.parse_assignment_tail(lhs, start)?;
        if self.eat(&Token::StarStar) {
            let rhs = self.parse_unary()?; // right-assoc, binds through unary
            return Ok(self.expr(
                ExprKind::Binary(BinOp::Pow, Box::new(lhs), Box::new(rhs)),
                start,
            ));
        }
        Ok(lhs)
    }
}
