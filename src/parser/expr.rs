use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Token};

impl Parser {
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
                ExprKind::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs)),
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
                ExprKind::Binary(BinOp::And, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        self.enter_expr();
        let result = self.parse_assignment_inner();
        self.leave_expr();
        result
    }

    pub(crate) fn parse_assignment_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let lhs = self.parse_ternary()?;
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
        self.advance();
        // Reference assignment (`$a = &$b;`) - only valid on a plain `=`,
        // never a compound one. Aliasing itself isn't modeled (nothing
        // here tracks that two variables now share storage), so this is
        // just consumed and the RHS's type still flows through normally.
        if op.is_none() {
            self.eat(&Token::Amp);
        }
        let rhs = self.parse_assignment()?;
        Ok(match op {
            None => self.expr(ExprKind::Assign(Box::new(lhs), Box::new(rhs)), start),
            Some(binop) => self.expr(
                ExprKind::CompoundAssign(binop, Box::new(lhs), Box::new(rhs)),
                start,
            ),
        })
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
        self.enter_expr();
        let result = self.parse_coalesce_inner();
        self.leave_expr();
        result
    }

    pub(crate) fn parse_coalesce_inner(&mut self) -> Result<Expr, ParseError> {
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

    pub(crate) fn parse_logical_or(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_logical_and()?;
        while self.eat(&Token::PipePipe) {
            let rhs = self.parse_logical_and()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_logical_and(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_bit_or()?;
        while self.eat(&Token::AmpAmp) {
            let rhs = self.parse_bit_or()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::And, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_bit_or(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_bit_xor()?;
        while self.eat(&Token::Pipe) {
            let rhs = self.parse_bit_xor()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::BitOr, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_bit_xor(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_bit_and()?;
        while self.eat(&Token::Caret) {
            let rhs = self.parse_bit_and()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::BitXor, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_bit_and(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_equality()?;
        while self.eat(&Token::Amp) {
            let rhs = self.parse_equality()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::BitAnd, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_relational()?;
        loop {
            let op = match self.peek() {
                Token::EqualsEquals => BinOp::Eq,
                Token::EqualsEqualsEquals => BinOp::Identical,
                Token::BangEquals => BinOp::NotEq,
                Token::BangEqualsEquals => BinOp::NotIdentical,
                Token::Spaceship => BinOp::Spaceship,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_relational()?;
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        Ok(lhs)
    }

    pub(crate) fn parse_relational(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_pipe()?;
        loop {
            let op = match self.peek() {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEquals => BinOp::LtEq,
                Token::GtEquals => BinOp::GtEq,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_pipe()?;
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        Ok(lhs)
    }

    /// PHP 8.5's `|> ` pipe operator: `$value |> callable` desugars to
    /// `callable($value)`. Left-associative (a while-loop, not
    /// self-recursion, so it's already immune to the stack-depth issue
    /// MAX_EXPR_DEPTH guards against). Precedence empirically checked
    /// against a real PHP 8.5 build: tighter than relational (`<`), looser
    /// than shift/concat/additive.
    pub(crate) fn parse_pipe(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_shift()?;
        while self.eat(&Token::PipeGt) {
            let rhs = self.parse_shift()?;
            lhs = self.expr(
                ExprKind::Binary(BinOp::Pipe, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        Ok(lhs)
    }

    pub(crate) fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_additive()?;
        loop {
            let op = match self.peek() {
                Token::LtLt => BinOp::Shl,
                Token::GtGt => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_additive()?;
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        Ok(lhs)
    }

    pub(crate) fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_multiplicative()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                Token::Dot => BinOp::Concat,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_multiplicative()?;
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        Ok(lhs)
    }

    pub(crate) fn parse_multiplicative(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let mut lhs = self.parse_not()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_not()?;
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        Ok(lhs)
    }

    /// `!` binds *looser* than `instanceof` in real PHP's own precedence
    /// table (`!$x instanceof Y` means `!($x instanceof Y)`, not
    /// `(!$x) instanceof Y`) - a real, previously-wrong precedence bug
    /// this project's negation used to get backwards by folding `!` into
    /// the same tighter-than-instanceof level as `~`/casts/`++`/`--`/`@`
    /// (which genuinely do bind tighter than `instanceof` and stay in
    /// `parse_unary_inner` unchanged). Confirmed via a real guard clause
    /// (`if (! $x instanceof Y) { ... }`) that never actually narrowed
    /// `$x` because of this - `apply_narrowing`'s `Unary(Not, Instanceof)`
    /// pattern never matched, since the old parse produced
    /// `Instanceof(Unary(Not, $x), Y)` instead.
    pub(crate) fn parse_not(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        if self.eat(&Token::Bang) {
            let operand = self.parse_not()?;
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
                InstanceofTarget::Name(self.parse_qualified_name())
            };
            return Ok(self.expr(ExprKind::Instanceof(Box::new(lhs), target), start));
        }
        Ok(lhs)
    }

    pub(crate) fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        self.enter_expr();
        let result = self.parse_unary_inner();
        self.leave_expr();
        result
    }

    pub(crate) fn parse_unary_inner(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        if let Some(cast_ty) = self.try_parse_cast() {
            let operand = self.parse_unary()?;
            return Ok(self.expr(ExprKind::Cast(cast_ty, Box::new(operand)), start));
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
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(self.expr(ExprKind::Unary(op, Box::new(operand)), start));
        }
        if self.eat(&Token::At) {
            // error-suppression operator: not modeled, transparent wrapper
            return self.parse_unary();
        }
        self.parse_pow()
    }

    /// `(int)`, `(string)`, etc: a cast is only distinguishable from a
    /// parenthesized expression by 3-token lookahead - `(`, a known cast
    /// keyword, `)` - with nothing else it could be.
    pub(crate) fn try_parse_cast(&mut self) -> Option<String> {
        if !matches!(self.peek(), Token::LParen) {
            return None;
        }
        let name = match self.peek_at(1) {
            Token::Identifier(s) => s.to_ascii_lowercase(),
            // `array` is the one cast name that's also a language keyword
            // (`array(...)`, type hints) rather than a plain identifier -
            // every other cast name (int, string, object, unset, ...)
            // isn't reserved at all.
            Token::Keyword(Keyword::Array) => "array".to_string(),
            _ => return None,
        };
        let normalized = match name.as_str() {
            "int" | "integer" => "int",
            "float" | "double" | "real" => "float",
            "string" | "binary" => "string",
            "bool" | "boolean" => "bool",
            "array" => "array",
            "object" => "object",
            "unset" => "unset",
            // `(void)` - a real, legal cast (discards the value; unlike
            // every other cast here it doesn't even change the type),
            // distinct from `void` the return-type keyword.
            "void" => "void",
            _ => return None,
        };
        if !matches!(self.peek_at(2), Token::RParen) {
            return None;
        }
        self.advance(); // (
        self.advance(); // type identifier
        self.advance(); // )
        Some(normalized.to_string())
    }

    pub(crate) fn parse_pow(&mut self) -> Result<Expr, ParseError> {
        let start = self.current_pos();
        let lhs = self.parse_postfix_from_primary()?;
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
