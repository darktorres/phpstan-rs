use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Position, Token};

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
        self.parse_assignment_tail(lhs, start)
    }

    /// Real PHP lets `variable '=' expr` (and its compound forms) appear
    /// as an operand of `==`/`===`/`!=`/`!==`/`<`/`>`/`<=`/`>=` with no
    /// enclosing parentheses - the classic guard-clause idiom
    /// `if (false === $x = fn()) {...}`/`while ($line = fgets($fp))`
    /// nested inside a further comparison. Precedence-climbing alone
    /// gets this wrong: `parse_equality`'s own RHS parsing (via
    /// `parse_relational`, which never looks for `=` at all) returns
    /// `$x` as soon as it sees `=`, and by the time control unwinds
    /// back to this function's own top-level `lhs`/`op` check, `lhs` is
    /// already the *whole* `false === $x` comparison - producing the
    /// nonsensical `(false === $x) = fn()` instead of real PHP's own
    /// `false === ($x = fn())`.
    ///
    /// This is the reusable "did an assignment operator follow this
    /// operand" half of that logic, factored out of `parse_assignment_
    /// inner`'s own top-level case so `parse_equality`/`parse_
    /// relational` (Phase 85) and `parse_logical_and`/`parse_logical_or`
    /// (found later, via the exact same corpus-false-positive pattern -
    /// symfony/console's own `Application.php`, `$e instanceof
    /// CommandNotFoundException && $namespace = $this->findNamespace(
    /// $name)`) can apply it to their own RHS the moment it's parsed,
    /// before wrapping it in the outer `Binary` node - exactly mirroring
    /// how real PHP's grammar treats assignment as available at that
    /// operand position, not only at the very top of the whole
    /// expression. An earlier version of this comment claimed real PHP's
    /// own precedence table "doesn't uniformly allow this everywhere,"
    /// singling out `&&` by name as an example of an operator this
    /// wasn't extended to - confirmed wrong the same way this project's
    /// own `NameContext::build` doc comment was about multi-namespace
    /// files: `php -r '$e=true; if ($e && $x = "hi") { echo $x; }'`
    /// prints `hi`, and `$x` stays genuinely unset when `$e` is falsy
    /// (real short-circuiting, not eager evaluation) - so `&&`/`||` get
    /// the identical treatment `==`/`<` already had. `parse_additive`
    /// (`+`/`-`/`.`) and the cast arm of `parse_unary_inner` (`(int)`/
    /// etc.) later got the same fix too, via a real corpus
    /// `variable.undefined` false positive - confirmed the identical
    /// idiom generalizes to these as well (`php -r '$v=0; $r="x".$v="y";
    /// var_dump($r,$v);'` prints `string(3) "xy"` and `$v` becomes
    /// `"y"`, i.e. `"x" . ($v = "y")`). The bitwise `|`/`^`/`&` operators
    /// show the identical quirk when spot-tested directly via `php -r`
    /// too (real PHP's grammar allows a bare assignment as an operand
    /// essentially everywhere, not just at these specific precedence
    /// levels) - deliberately still left unextended here, since there's
    /// no corpus evidence calling for it the way every operator above
    /// had, and this project's own convention is to extend on
    /// evidence, not on a grammar property alone.
    pub(crate) fn parse_assignment_tail(
        &mut self,
        lhs: Expr,
        start: Position,
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
            let rhs_start = self.current_pos();
            let rhs = self.parse_logical_and()?;
            // Phase 85 follow-up: `$e instanceof Foo || $x = fn()` - a
            // real, confirmed PHP idiom (`php -r` proves `$x` really is
            // assigned whenever the right side actually runs, exactly
            // like `false === $x = fn()` already gets via `parse_
            // assignment_tail`'s own doc comment) that this project's
            // own naive precedence-table reading previously assumed
            // real PHP didn't extend to `&&`/`||` - it does. Without
            // this, `$e instanceof CommandNotFoundException && $namespace
            // = $this->findNamespace($name)` (symfony/console's own
            // `Application.php`) parsed as the nonsensical `($e
            // instanceof CommandNotFoundException && $namespace) =
            // $this->findNamespace($name)`, leaving `$namespace` looking
            // completely unassigned to every later use inside the `if`
            // body (a real, corpus-found `variable.undefined` false
            // positive).
            let rhs = self.parse_assignment_tail(rhs, rhs_start)?;
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
            let rhs_start = self.current_pos();
            let rhs = self.parse_bit_or()?;
            // See `parse_logical_or`'s own identical fix just above -
            // same real idiom, same confirmed-via-`php -r` fix.
            let rhs = self.parse_assignment_tail(rhs, rhs_start)?;
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
            let rhs_start = self.current_pos();
            let rhs = self.parse_relational()?;
            // Phase 85: `false === $x = fn()` - see `parse_assignment_
            // tail`'s own doc comment.
            let rhs = self.parse_assignment_tail(rhs, rhs_start)?;
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
            let rhs_start = self.current_pos();
            let rhs = self.parse_pipe()?;
            // Phase 85: `0 > $x = fn()` - see `parse_assignment_tail`'s
            // own doc comment.
            let rhs = self.parse_assignment_tail(rhs, rhs_start)?;
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
            let rhs_start = self.current_pos();
            let rhs = self.parse_multiplicative()?;
            // See `parse_logical_or`'s own identical fix (and `parse_
            // assignment_tail`'s own doc comment) - the same real-PHP
            // "assignment as a bare operand" idiom extends here too,
            // confirmed directly (`php -r '$v=0; $r="x".$v="y";
            // var_dump($r,$v);'` prints `string(3) "xy"` and `$v`
            // becomes `"y"`, i.e. `"x" . ($v = "y")`). Real corpus find:
            // a top-10-bucket `variable.undefined` scoping round.
            let rhs = self.parse_assignment_tail(rhs, rhs_start)?;
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
            let operand_start = self.current_pos();
            let operand = self.parse_not()?;
            // The same real-PHP "assignment as a bare operand" grammar
            // quirk `parse_assignment_tail`'s own doc comment describes
            // for `==`/`&&`/`||`, confirmed via `php -r '$x=0; $r=!$x=5;
            // var_dump($r,$x);'` printing `bool(false) int(5)` - `!$x =
            // expr` parses as `!($x = expr)`, not `(!$x) = expr`. Must run
            // here, at the point `!` actually binds, rather than relying
            // on an enclosing `&&`/`||`/comparison's own fix further up
            // the chain: by the time control gets there, `!` has already
            // wrapped the bare variable, so their fix would see `Unary(
            // Not, $var)` - not a valid assignment target - as its own
            // "lhs" and misassign into it. A corpus-verified idiom
            // (`plupload`'s `upload.php`: `if (!is_dir($targetDir) ||
            // !$dir = opendir($targetDir))`) that left `$dir` looking
            // undefined to every later `readdir($dir)` without this.
            let operand = self.parse_assignment_tail(operand, operand_start)?;
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
            let operand_start = self.current_pos();
            let operand = self.parse_unary()?;
            // Same real-PHP "assignment as a bare operand" idiom as
            // `parse_additive`'s own identical fix just above - confirmed
            // directly (`php -r '$v=0; $r=(int)$v=5; var_dump($r,$v);'`
            // prints `int(5) int(5)`, i.e. `(int)($v = 5)`, not
            // `((int)$v) = 5` (which isn't even a valid assignment
            // target).
            let operand = self.parse_assignment_tail(operand, operand_start)?;
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
