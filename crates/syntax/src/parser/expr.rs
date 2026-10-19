use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

/// Binding powers, loosest first, following `zend_language_parser.y`.
/// Left-associative operators bind their right operand one above their
/// own, right-associative ones at their own.
pub(crate) mod bp {
    pub(crate) const LOWEST: u8 = 0;
    pub(crate) const OR: u8 = 2;
    pub(crate) const XOR: u8 = 4;
    pub(crate) const AND: u8 = 6;
    pub(crate) const TERNARY: u8 = 10;
    pub(crate) const COALESCE: u8 = 12;
    pub(crate) const BOOL_OR: u8 = 14;
    pub(crate) const BOOL_AND: u8 = 16;
    pub(crate) const BIT_OR: u8 = 18;
    pub(crate) const BIT_XOR: u8 = 20;
    pub(crate) const BIT_AND: u8 = 22;
    pub(crate) const EQUALITY: u8 = 24;
    pub(crate) const RELATIONAL: u8 = 26;
    pub(crate) const PIPE: u8 = 28;
    pub(crate) const CONCAT: u8 = 30;
    pub(crate) const SHIFT: u8 = 32;
    pub(crate) const ADD: u8 = 34;
    pub(crate) const MUL: u8 = 36;
    pub(crate) const INSTANCEOF: u8 = 40;
    pub(crate) const UNARY: u8 = 42;
    pub(crate) const POW: u8 = 44;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Assoc {
    Left,
    Right,
    None,
}

/// What an operator between two operands builds.
#[derive(Clone, Copy)]
enum Infix {
    Binary(BinOp),
    Instanceof,
    Ternary,
}

impl Parser<'_> {
    // ---- expressions (binding power, by PHP's precedence table) ----

    pub fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_bp(bp::LOWEST)
    }

    /// An expression without the low-precedence `and`/`or`/`xor`: what an
    /// assignment's right side, a `print` operand and a ternary's branches
    /// are.
    pub(crate) fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        stack::guard(|| self.parse_expr_bp(bp::TERNARY))
    }

    pub(crate) fn parse_ternary(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_bp(bp::TERNARY)
    }

    /// A prefix operator's operand and what binds tighter: `clone`'s
    /// operand, an `instanceof` target.
    pub(crate) fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_bp(bp::UNARY)
    }

    /// The operators of binding power `min` or tighter around one operand,
    /// by `bp`'s levels. One stack guard for each expression nested in
    /// another, where each operand used to pass three.
    pub(crate) fn parse_expr_bp(&mut self, min: u8) -> Result<Expr, ParseError> {
        stack::guard(|| self.parse_expr_bp_inner(min))
    }

    fn parse_expr_bp_inner(&mut self, min: u8) -> Result<Expr, ParseError> {
        stack::guard(|| {
            let start = self.current_pos();
            let mut lhs = self.parse_prefix(start)?;
            // The level of the non-associative operator just applied: another
            // of the same level is a syntax error.
            let mut applied_non_associative: Option<u8> = None;
            while let Some((power, assoc, infix)) = self.infix() {
                if power < min {
                    break;
                }
                if assoc == Assoc::None && applied_non_associative == Some(power) {
                    return Err(ParseError {
                        message: format!("syntax error, unexpected token \"{:?}\"", self.peek()),
                        span: self.current_span(),
                    });
                }
                lhs = match infix {
                    Infix::Binary(op) => {
                        self.skip();
                        let next = if assoc == Assoc::Right {
                            power
                        } else {
                            power + 1
                        };
                        let rhs = self.parse_expr_bp(next)?;
                        self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start)
                    }
                    Infix::Instanceof => {
                        self.skip();
                        // The right-hand side is usually a static class name,
                        // but PHP also allows what `new` does (`$x instanceof
                        // $className`, `$x instanceof static::$class`, `$x
                        // instanceof ('std' . $class)`) for a runtime-determined
                        // class.
                        let reference = self.parse_new_class_reference()?;
                        let target = match &reference.kind {
                            ExprKind::Name(name) => InstanceofTarget::Name(name.clone()),
                            _ => InstanceofTarget::Expr(Box::new(reference)),
                        };
                        self.expr(ExprKind::Instanceof(Box::new(lhs), target), start)
                    }
                    Infix::Ternary => self.parse_ternary_rest(lhs, start)?,
                };
                applied_non_associative = (assoc == Assoc::None).then_some(power);
            }
            Ok(lhs)
        })
    }

    /// The `? ... : ...` or `?: ...` after `cond`, the current token being
    /// the `?`. A ternary is left-associative, and PHP refuses to compile
    /// one that is the condition of another unless both are short
    /// (`$a ?: $b ?: $c`).
    fn parse_ternary_rest(&mut self, cond: Expr, start: u32) -> Result<Expr, ParseError> {
        stack::guard(|| {
            let short = matches!(self.peek_at(1), Token::Colon);
            if let ExprKind::Ternary(_, then, _) = &cond.kind
                && !(then.is_none() && short)
            {
                return Err(ParseError {
                message: "Unparenthesized `a ? b : c ? d : e` is not supported; use either `(a ? b : c) ? d : e` or `a ? b : (c ? d : e)`".to_string(),
                span: self.current_span(),
            });
            }
            self.skip(); // ?
            if short {
                self.skip(); // :
                let else_branch = self.parse_expr_bp(bp::TERNARY + 1)?;
                return Ok(self.expr(
                    ExprKind::Ternary(Box::new(cond), None, Box::new(else_branch)),
                    start,
                ));
            }
            let then_branch = self.parse_expr_bp(bp::LOWEST)?;
            self.expect(&Token::Colon)?;
            let else_branch = self.parse_expr_bp(bp::TERNARY + 1)?;
            Ok(self.expr(
                ExprKind::Ternary(
                    Box::new(cond),
                    Some(Box::new(then_branch)),
                    Box::new(else_branch),
                ),
                start,
            ))
        })
    }

    /// What the next token is as an operator between two operands: its
    /// binding power, associativity and kind; none for a token that is
    /// not one. Binary operators, loosest first: `|>` (PHP 8.5's pipe:
    /// `$value |> callable` is `callable($value)`) was checked against a
    /// real PHP 8.5 build: tighter than relational, looser than shift,
    /// concat and additive; `.` is looser than `<<`/`>>` and `+`/`-` since
    /// PHP 8, so `"a" . $x + 1` is `"a" . ($x + 1)`.
    fn infix(&self) -> Option<(u8, Assoc, Infix)> {
        use Assoc::{Left, None as NonAssoc, Right};
        use Infix::Binary;
        Some(match self.peek() {
            Token::Keyword(Keyword::Or) => (bp::OR, Left, Binary(BinOp::LogicalOr)),
            Token::Keyword(Keyword::Xor) => (bp::XOR, Left, Binary(BinOp::LogicalXor)),
            Token::Keyword(Keyword::And) => (bp::AND, Left, Binary(BinOp::LogicalAnd)),
            Token::Question => (bp::TERNARY, Left, Infix::Ternary),
            Token::QuestionQuestion => (bp::COALESCE, Right, Binary(BinOp::Coalesce)),
            Token::PipePipe => (bp::BOOL_OR, Left, Binary(BinOp::Or)),
            Token::AmpAmp => (bp::BOOL_AND, Left, Binary(BinOp::And)),
            Token::Pipe => (bp::BIT_OR, Left, Binary(BinOp::BitOr)),
            Token::Caret => (bp::BIT_XOR, Left, Binary(BinOp::BitXor)),
            Token::Amp => (bp::BIT_AND, Left, Binary(BinOp::BitAnd)),
            Token::EqualsEquals => (bp::EQUALITY, NonAssoc, Binary(BinOp::Eq)),
            Token::EqualsEqualsEquals => (bp::EQUALITY, NonAssoc, Binary(BinOp::Identical)),
            Token::BangEquals => (bp::EQUALITY, NonAssoc, Binary(BinOp::NotEq)),
            Token::BangEqualsEquals => (bp::EQUALITY, NonAssoc, Binary(BinOp::NotIdentical)),
            Token::Spaceship => (bp::EQUALITY, NonAssoc, Binary(BinOp::Spaceship)),
            Token::Lt => (bp::RELATIONAL, NonAssoc, Binary(BinOp::Lt)),
            Token::Gt => (bp::RELATIONAL, NonAssoc, Binary(BinOp::Gt)),
            Token::LtEquals => (bp::RELATIONAL, NonAssoc, Binary(BinOp::LtEq)),
            Token::GtEquals => (bp::RELATIONAL, NonAssoc, Binary(BinOp::GtEq)),
            Token::PipeGt => (bp::PIPE, Left, Binary(BinOp::Pipe)),
            Token::Dot => (bp::CONCAT, Left, Binary(BinOp::Concat)),
            Token::LtLt => (bp::SHIFT, Left, Binary(BinOp::Shl)),
            Token::GtGt => (bp::SHIFT, Left, Binary(BinOp::Shr)),
            Token::Plus => (bp::ADD, Left, Binary(BinOp::Add)),
            Token::Minus => (bp::ADD, Left, Binary(BinOp::Sub)),
            Token::Star => (bp::MUL, Left, Binary(BinOp::Mul)),
            Token::Slash => (bp::MUL, Left, Binary(BinOp::Div)),
            Token::Percent => (bp::MUL, Left, Binary(BinOp::Mod)),
            Token::Keyword(Keyword::Instanceof) => (bp::INSTANCEOF, Left, Infix::Instanceof),
            Token::StarStar => (bp::POW, Right, Binary(BinOp::Pow)),
            _ => return None,
        })
    }

    /// Whether the current token can be the first of an expression: it is
    /// not a closer, a separator, an operator that takes a left operand
    /// only, or a keyword that continues a statement.
    pub(crate) fn can_begin_expression(&self) -> bool {
        !matches!(
            self.peek(),
            Token::Semicolon
                | Token::RParen
                | Token::RBracket
                | Token::RBrace
                | Token::Comma
                | Token::Colon
                | Token::DoubleColon
                | Token::Arrow
                | Token::NullsafeArrow
                | Token::FatArrow
                | Token::Question
                | Token::QuestionQuestion
                | Token::QuestionQuestionEquals
                | Token::Dot
                | Token::DotEquals
                | Token::Star
                | Token::StarStar
                | Token::Slash
                | Token::Percent
                | Token::Equals
                | Token::PlusEquals
                | Token::MinusEquals
                | Token::StarEquals
                | Token::StarStarEquals
                | Token::SlashEquals
                | Token::PercentEquals
                | Token::AmpEquals
                | Token::PipeEquals
                | Token::CaretEquals
                | Token::LtLtEquals
                | Token::GtGtEquals
                | Token::EqualsEquals
                | Token::EqualsEqualsEquals
                | Token::BangEquals
                | Token::BangEqualsEquals
                | Token::Lt
                | Token::Gt
                | Token::LtEquals
                | Token::GtEquals
                | Token::Spaceship
                | Token::AmpAmp
                | Token::PipePipe
                | Token::Amp
                | Token::Pipe
                | Token::Caret
                | Token::LtLt
                | Token::GtGt
                | Token::PipeGt
                | Token::Ellipsis
                | Token::Eof
                | Token::Keyword(
                    Keyword::And
                        | Keyword::Or
                        | Keyword::Xor
                        | Keyword::Instanceof
                        | Keyword::As
                        | Keyword::Else
                        | Keyword::ElseIf
                        | Keyword::Extends
                        | Keyword::Implements
                        | Keyword::Catch
                        | Keyword::Finally
                        | Keyword::Case
                        | Keyword::EndIf
                        | Keyword::EndWhile
                        | Keyword::EndFor
                        | Keyword::EndForeach
                        | Keyword::EndSwitch
                )
        )
    }

    /// A prefix operator and its operand, or the operand alone. `!` takes
    /// what `instanceof` binds tighter than (`!$x instanceof Y` is
    /// `!($x instanceof Y)`: `apply_narrowing`'s `Unary(Not, Instanceof)`
    /// pattern depends on it), and the other prefix operators take their
    /// operand with `**` (`-$a ** 2` is `-($a ** 2)`).
    fn parse_prefix(&mut self, start: u32) -> Result<Expr, ParseError> {
        stack::guard(|| {
            if let Some(spelling) = self.try_parse_cast() {
                let operand = self.parse_expr_bp(bp::UNARY)?;
                return Ok(self.expr(ExprKind::Cast(spelling, Box::new(operand)), start));
            }
            let op = match self.peek() {
                Token::Bang => {
                    self.skip();
                    let operand = self.parse_expr_bp(bp::INSTANCEOF)?;
                    return Ok(self.expr(ExprKind::Unary(UnaryOp::Not, Box::new(operand)), start));
                }
                Token::Minus => UnaryOp::Neg,
                Token::Plus => UnaryOp::Plus,
                Token::Tilde => UnaryOp::BitNot,
                Token::Increment => UnaryOp::PreInc,
                Token::Decrement => UnaryOp::PreDec,
                Token::At => {
                    self.skip();
                    let operand = self.parse_expr_bp(bp::UNARY)?;
                    return Ok(self.expr(ExprKind::ErrorSuppress(Box::new(operand)), start));
                }
                _ => {
                    // An operand: a primary with its postfix chain, taking an
                    // assignment to its right where PHP's grammar has
                    // `variable '=' expr`, wherever an operand may stand.
                    let operand = self.parse_postfix_from_primary()?;
                    return self.parse_assignment_tail(operand, start);
                }
            };
            self.skip();
            let operand = self.parse_expr_bp(bp::UNARY)?;
            Ok(self.expr(ExprKind::Unary(op, Box::new(operand)), start))
        })
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
        stack::guard(|| {
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
                None if is_ref => {
                    self.expr(ExprKind::AssignRef(Box::new(lhs), Box::new(rhs)), start)
                }
                None => self.expr(ExprKind::Assign(Box::new(lhs), Box::new(rhs)), start),
                Some(binop) => self.expr(
                    ExprKind::CompoundAssign(binop, Box::new(lhs), Box::new(rhs)),
                    start,
                ),
            })
        })
    }

    /// `target`, an assignment's or a `foreach`'s, with an array in it
    /// taken as the destructuring pattern it is there. `top` is whether
    /// `target` is the whole target rather than an item of an enclosing
    /// pattern.
    pub(crate) fn as_pattern(&mut self, mut target: Expr, top: bool) -> Result<Expr, ParseError> {
        stack::guard(|| match &mut target.kind {
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
        })
    }

    /// The items of a `[...]` or `list(...)` spelled `syntax`, as the
    /// items of the pattern it is, with PHP's compile-time checks on them.
    pub(crate) fn destructure_items(
        &mut self,
        items: Vec<ArrayItem>,
        syntax: DestructureSyntax,
        span: Span,
    ) -> Result<Vec<DestructureItem>, ParseError> {
        stack::guard(|| {
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
        })
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
}
