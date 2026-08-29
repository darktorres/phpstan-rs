use crate::ast::*;
use crate::lexer::Lexer;
use crate::token::{Keyword, StringPart, Token};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    pub fn parse_program(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while !self.at_eof() {
            stmts.push(self.parse_statement());
        }
        stmts
    }

    // ---- token stream helpers ----

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek_at(&self, offset: usize) -> &Token {
        self.tokens.get(self.pos + offset).unwrap_or(&Token::Eof)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn check(&self, t: &Token) -> bool {
        self.peek() == t
    }

    fn eat(&mut self, t: &Token) -> bool {
        if self.check(t) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Token) {
        if !self.eat(t) {
            panic!(
                "parse error: expected {:?}, found {:?} at token {}",
                t,
                self.peek(),
                self.pos
            );
        }
    }

    fn expect_semicolon(&mut self) {
        // A `?>` close tag lexes as a Semicolon already; plain EOF also
        // terminates the last statement without one.
        if self.at_eof() {
            return;
        }
        self.expect(&Token::Semicolon);
    }

    fn expect_ident(&mut self) -> String {
        match self.advance() {
            Token::Identifier(s) => s,
            other => panic!("parse error: expected identifier, found {:?}", other),
        }
    }

    // ---- statements ----

    fn parse_statement(&mut self) -> Stmt {
        match self.peek().clone() {
            Token::InlineHtml(s) => {
                self.advance();
                Stmt::InlineHtml(s)
            }
            Token::Semicolon => {
                self.advance();
                Stmt::Block(Vec::new())
            }
            Token::LBrace => self.parse_block(),
            Token::Keyword(Keyword::Echo) => {
                self.advance();
                let mut exprs = vec![self.parse_expression()];
                while self.eat(&Token::Comma) {
                    exprs.push(self.parse_expression());
                }
                self.expect_semicolon();
                Stmt::Echo(exprs)
            }
            Token::Keyword(Keyword::Return) => {
                self.advance();
                if self.check(&Token::Semicolon) {
                    self.advance();
                    Stmt::Return(None)
                } else {
                    let e = self.parse_expression();
                    self.expect_semicolon();
                    Stmt::Return(Some(e))
                }
            }
            Token::Keyword(Keyword::If) => self.parse_if(),
            Token::Keyword(Keyword::While) => self.parse_while(),
            Token::Keyword(Keyword::Do) => self.parse_do_while(),
            Token::Keyword(Keyword::For) => self.parse_for(),
            Token::Keyword(Keyword::Foreach) => self.parse_foreach(),
            Token::Keyword(Keyword::Break) => {
                self.advance();
                let level = self.parse_optional_level();
                self.expect_semicolon();
                Stmt::Break(level)
            }
            Token::Keyword(Keyword::Continue) => {
                self.advance();
                let level = self.parse_optional_level();
                self.expect_semicolon();
                Stmt::Continue(level)
            }
            Token::Keyword(Keyword::Global) => {
                self.advance();
                let mut names = vec![self.expect_variable()];
                while self.eat(&Token::Comma) {
                    names.push(self.expect_variable());
                }
                self.expect_semicolon();
                Stmt::Global(names)
            }
            Token::Keyword(Keyword::Function) => {
                let f = self.parse_function_decl();
                Stmt::FunctionDecl(f)
            }
            Token::Keyword(Keyword::Class)
            | Token::Keyword(Keyword::Abstract)
            | Token::Keyword(Keyword::Final)
            | Token::Keyword(Keyword::Interface)
            | Token::Keyword(Keyword::Trait) => Stmt::ClassDecl(self.parse_class_decl()),
            Token::Keyword(Keyword::Namespace) => {
                self.advance();
                let name = self.parse_qualified_name();
                self.expect_semicolon();
                Stmt::Namespace(name)
            }
            Token::Keyword(Keyword::Use) => {
                self.advance();
                let name = self.parse_qualified_name();
                self.expect_semicolon();
                Stmt::Use(name)
            }
            Token::Keyword(Keyword::Try) => self.parse_try(),
            Token::Keyword(Keyword::Throw) => {
                self.advance();
                let e = self.parse_expression();
                self.expect_semicolon();
                Stmt::Throw(e)
            }
            Token::Keyword(Keyword::Switch) => self.parse_switch(),
            _ => {
                let e = self.parse_expression();
                self.expect_semicolon();
                Stmt::Expression(e)
            }
        }
    }

    fn parse_optional_level(&mut self) -> Option<i64> {
        if let Token::Integer(n) = self.peek() {
            let n = *n;
            self.advance();
            Some(n)
        } else {
            None
        }
    }

    fn expect_variable(&mut self) -> String {
        match self.advance() {
            Token::Variable(s) => s,
            other => panic!("parse error: expected variable, found {:?}", other),
        }
    }

    fn parse_block(&mut self) -> Stmt {
        self.expect(&Token::LBrace);
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            stmts.push(self.parse_statement());
        }
        self.expect(&Token::RBrace);
        Stmt::Block(stmts)
    }

    fn parse_braced_or_single(&mut self) -> Stmt {
        if self.check(&Token::LBrace) {
            self.parse_block()
        } else {
            self.parse_statement()
        }
    }

    fn parse_if(&mut self) -> Stmt {
        self.advance(); // if
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);
        let then_branch = Box::new(self.parse_braced_or_single());

        let else_branch = if self.eat(&Token::Keyword(Keyword::ElseIf)) {
            self.pos -= 1; // put back; re-dispatch as a nested `if`
            self.tokens[self.pos] = Token::Keyword(Keyword::If);
            Some(Box::new(self.parse_if()))
        } else if self.eat(&Token::Keyword(Keyword::Else)) {
            Some(Box::new(self.parse_braced_or_single()))
        } else {
            None
        };

        Stmt::If {
            cond,
            then_branch,
            else_branch,
        }
    }

    fn parse_while(&mut self) -> Stmt {
        self.advance();
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);
        let body = Box::new(self.parse_braced_or_single());
        Stmt::While { cond, body }
    }

    fn parse_do_while(&mut self) -> Stmt {
        self.advance();
        let body = Box::new(self.parse_braced_or_single());
        self.expect(&Token::Keyword(Keyword::While));
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);
        self.expect_semicolon();
        Stmt::DoWhile { body, cond }
    }

    fn parse_for(&mut self) -> Stmt {
        self.advance();
        self.expect(&Token::LParen);
        let init = self.parse_expr_list_until(&Token::Semicolon);
        self.expect(&Token::Semicolon);
        let cond = self.parse_expr_list_until(&Token::Semicolon);
        self.expect(&Token::Semicolon);
        let step = self.parse_expr_list_until(&Token::RParen);
        self.expect(&Token::RParen);
        let body = Box::new(self.parse_braced_or_single());
        Stmt::For {
            init,
            cond,
            step,
            body,
        }
    }

    fn parse_expr_list_until(&mut self, stop: &Token) -> Vec<Expr> {
        let mut exprs = Vec::new();
        if self.check(stop) {
            return exprs;
        }
        exprs.push(self.parse_expression());
        while self.eat(&Token::Comma) {
            exprs.push(self.parse_expression());
        }
        exprs
    }

    fn parse_foreach(&mut self) -> Stmt {
        self.advance();
        self.expect(&Token::LParen);
        let subject = self.parse_expression();
        self.expect(&Token::Keyword(Keyword::As));
        let by_ref_first = self.eat(&Token::Amp);
        let first = self.parse_expression();

        let (key, by_ref, value) = if self.eat(&Token::FatArrow) {
            let by_ref_val = self.eat(&Token::Amp);
            let value = self.parse_expression();
            (Some(first), by_ref_val, value)
        } else {
            (None, by_ref_first, first)
        };

        self.expect(&Token::RParen);
        let body = Box::new(self.parse_braced_or_single());
        Stmt::Foreach {
            subject,
            key,
            value,
            by_ref,
            body,
        }
    }

    fn parse_try(&mut self) -> Stmt {
        self.advance(); // try
        let body = self.parse_stmt_list_block();

        let mut catches = Vec::new();
        while self.eat(&Token::Keyword(Keyword::Catch)) {
            self.expect(&Token::LParen);
            let mut types = vec![self.parse_qualified_name()];
            while self.eat(&Token::Pipe) {
                types.push(self.parse_qualified_name());
            }
            let var = if matches!(self.peek(), Token::Variable(_)) {
                Some(self.expect_variable())
            } else {
                None
            };
            self.expect(&Token::RParen);
            let body = self.parse_stmt_list_block();
            catches.push(CatchClause { types, var, body });
        }

        let finally = if self.eat(&Token::Keyword(Keyword::Finally)) {
            Some(self.parse_stmt_list_block())
        } else {
            None
        };

        Stmt::Try {
            body,
            catches,
            finally,
        }
    }

    fn parse_stmt_list_block(&mut self) -> Vec<Stmt> {
        if let Stmt::Block(stmts) = self.parse_block() {
            stmts
        } else {
            unreachable!()
        }
    }

    fn parse_switch(&mut self) -> Stmt {
        self.advance(); // switch
        self.expect(&Token::LParen);
        let subject = self.parse_expression();
        self.expect(&Token::RParen);
        self.expect(&Token::LBrace);

        let mut cases: Vec<SwitchCase> = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            if self.eat(&Token::Keyword(Keyword::Case)) {
                let test = self.parse_expression();
                if !self.eat(&Token::Colon) {
                    self.eat(&Token::Semicolon);
                }
                cases.push(SwitchCase {
                    test: Some(test),
                    body: Vec::new(),
                });
            } else if self.eat(&Token::Keyword(Keyword::Default)) {
                if !self.eat(&Token::Colon) {
                    self.eat(&Token::Semicolon);
                }
                cases.push(SwitchCase {
                    test: None,
                    body: Vec::new(),
                });
            } else {
                let stmt = self.parse_statement();
                // A statement before any `case`/`default` label is malformed
                // PHP; dropping it here just keeps the parser in sync.
                if let Some(case) = cases.last_mut() {
                    case.body.push(stmt);
                }
            }
        }
        self.expect(&Token::RBrace);

        Stmt::Switch { subject, cases }
    }

    fn parse_qualified_name(&mut self) -> String {
        let mut s = String::new();
        loop {
            match self.peek().clone() {
                Token::Identifier(part) => {
                    s.push_str(&part);
                    self.advance();
                }
                Token::Backslash => {
                    s.push('\\');
                    self.advance();
                }
                _ => break,
            }
        }
        s
    }

    // ---- type hints ----

    fn is_type_start(&self) -> bool {
        self.is_type_start_at(0)
    }

    fn is_type_start_at(&self, offset: usize) -> bool {
        matches!(
            self.peek_at(offset),
            Token::Question
                | Token::Identifier(_)
                | Token::Backslash
                | Token::Keyword(Keyword::Array)
                | Token::Keyword(Keyword::Static)
                | Token::Keyword(Keyword::Null)
        )
    }

    // Consumes one qualified type name, e.g. `int`, `array`, `\App\Foo`.
    fn parse_type_atom(&mut self) -> String {
        let mut s = String::new();
        loop {
            match self.peek().clone() {
                Token::Identifier(part) => {
                    s.push_str(&part);
                    self.advance();
                }
                Token::Backslash => {
                    s.push('\\');
                    self.advance();
                }
                Token::Keyword(Keyword::Array) => {
                    s.push_str("array");
                    self.advance();
                }
                Token::Keyword(Keyword::Static) => {
                    s.push_str("static");
                    self.advance();
                }
                Token::Keyword(Keyword::Null) => {
                    s.push_str("null");
                    self.advance();
                }
                _ => break,
            }
        }
        s
    }

    fn parse_type_hint(&mut self) -> Option<String> {
        if !self.is_type_start() {
            return None;
        }
        let mut s = String::new();
        if self.eat(&Token::Question) {
            s.push('?');
        }
        s.push_str(&self.parse_type_atom());
        loop {
            if self.check(&Token::Pipe) {
                self.advance();
                s.push('|');
                s.push_str(&self.parse_type_atom());
                continue;
            }
            // `&` is ambiguous between an intersection type (`A&B`) and a
            // by-ref marker before a param's `$name` - only treat it as part
            // of the type when another type atom actually follows.
            if self.check(&Token::Amp) && self.is_type_start_at(1) {
                self.advance();
                s.push('&');
                s.push_str(&self.parse_type_atom());
                continue;
            }
            break;
        }
        Some(s)
    }

    // ---- functions / params ----

    fn parse_params(&mut self) -> Vec<Param> {
        self.expect(&Token::LParen);
        let mut params = Vec::new();
        while !self.check(&Token::RParen) {
            // constructor-promotion / visibility modifiers: recorded on the
            // class via the property list would be more accurate, but for
            // this prototype we just consume and drop them.
            while matches!(
                self.peek(),
                Token::Keyword(Keyword::Public)
                    | Token::Keyword(Keyword::Private)
                    | Token::Keyword(Keyword::Protected)
                    | Token::Keyword(Keyword::Readonly)
            ) {
                self.advance();
            }

            let type_hint = self.parse_type_hint();
            let by_ref = self.eat(&Token::Amp);
            let variadic = self.eat(&Token::Ellipsis);
            let name = self.expect_variable();
            let default = if self.eat(&Token::Equals) {
                Some(self.parse_expression())
            } else {
                None
            };
            params.push(Param {
                name,
                type_hint,
                default,
                by_ref,
                variadic,
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen);
        params
    }

    fn parse_function_decl(&mut self) -> FunctionDecl {
        self.expect(&Token::Keyword(Keyword::Function));
        self.eat(&Token::Amp); // by-ref return marker
        let name = self.expect_ident();
        let params = self.parse_params();
        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()
        } else {
            None
        };
        let body = if self.eat(&Token::Semicolon) {
            Vec::new()
        } else if let Stmt::Block(stmts) = self.parse_block() {
            stmts
        } else {
            unreachable!()
        };
        FunctionDecl {
            name,
            params,
            return_type,
            body,
        }
    }

    // ---- classes ----

    fn parse_class_decl(&mut self) -> ClassDecl {
        let mut is_abstract = false;
        let mut is_final = false;
        loop {
            match self.peek() {
                Token::Keyword(Keyword::Abstract) => {
                    is_abstract = true;
                    self.advance();
                }
                Token::Keyword(Keyword::Final) => {
                    is_final = true;
                    self.advance();
                }
                _ => break,
            }
        }
        match self.advance() {
            Token::Keyword(Keyword::Class)
            | Token::Keyword(Keyword::Interface)
            | Token::Keyword(Keyword::Trait) => {}
            other => panic!(
                "parse error: expected class/interface/trait, found {:?}",
                other
            ),
        }
        let name = self.expect_ident();

        let extends = if self.eat(&Token::Keyword(Keyword::Extends)) {
            Some(self.parse_qualified_name())
        } else {
            None
        };

        let mut implements = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            implements.push(self.parse_qualified_name());
            while self.eat(&Token::Comma) {
                implements.push(self.parse_qualified_name());
            }
        }

        self.expect(&Token::LBrace);
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut consts = Vec::new();

        while !self.check(&Token::RBrace) && !self.at_eof() {
            let mut visibility = Visibility::Public;
            let mut is_static = false;
            let mut is_abstract_member = false;
            loop {
                match self.peek() {
                    Token::Keyword(Keyword::Public) => {
                        visibility = Visibility::Public;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Private) => {
                        visibility = Visibility::Private;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Protected) => {
                        visibility = Visibility::Protected;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Static) => {
                        is_static = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Abstract) => {
                        is_abstract_member = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Final) | Token::Keyword(Keyword::Readonly) => {
                        self.advance();
                    }
                    _ => break,
                }
            }

            if self.eat(&Token::Keyword(Keyword::Const)) {
                loop {
                    let cname = self.expect_ident();
                    self.expect(&Token::Equals);
                    let value = self.parse_expression();
                    consts.push(ClassConst { name: cname, value });
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect_semicolon();
                continue;
            }

            if self.check(&Token::Keyword(Keyword::Function)) {
                let function = self.parse_function_decl();
                methods.push(MethodDecl {
                    visibility,
                    is_static,
                    is_abstract: is_abstract_member,
                    function,
                });
                continue;
            }

            // property, possibly with a type hint
            let type_hint = self.parse_type_hint();
            loop {
                let name = self.expect_variable();
                let default = if self.eat(&Token::Equals) {
                    Some(self.parse_expression())
                } else {
                    None
                };
                properties.push(PropertyDecl {
                    name,
                    visibility: visibility.clone(),
                    is_static,
                    type_hint: type_hint.clone(),
                    default,
                });
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect_semicolon();
        }
        self.expect(&Token::RBrace);

        ClassDecl {
            name,
            extends,
            implements,
            is_abstract,
            is_final,
            properties,
            methods,
            consts,
        }
    }

    // ---- expressions (precedence climbing) ----

    pub fn parse_expression(&mut self) -> Expr {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Expr {
        let lhs = self.parse_ternary();
        let op = match self.peek() {
            Token::Equals => None,
            Token::PlusEquals => Some(BinOp::Add),
            Token::MinusEquals => Some(BinOp::Sub),
            Token::StarEquals => Some(BinOp::Mul),
            Token::SlashEquals => Some(BinOp::Div),
            Token::PercentEquals => Some(BinOp::Mod),
            Token::DotEquals => Some(BinOp::Concat),
            Token::QuestionQuestionEquals => Some(BinOp::Coalesce),
            _ => return lhs,
        };
        self.advance();
        let rhs = self.parse_assignment();
        match op {
            None => Expr::Assign(Box::new(lhs), Box::new(rhs)),
            Some(binop) => Expr::CompoundAssign(binop, Box::new(lhs), Box::new(rhs)),
        }
    }

    fn parse_ternary(&mut self) -> Expr {
        let cond = self.parse_coalesce();
        if self.eat(&Token::Question) {
            if self.eat(&Token::Colon) {
                let else_branch = self.parse_assignment();
                return Expr::Ternary(Box::new(cond), None, Box::new(else_branch));
            }
            let then_branch = self.parse_assignment();
            self.expect(&Token::Colon);
            let else_branch = self.parse_assignment();
            return Expr::Ternary(
                Box::new(cond),
                Some(Box::new(then_branch)),
                Box::new(else_branch),
            );
        }
        cond
    }

    fn parse_coalesce(&mut self) -> Expr {
        let lhs = self.parse_logical_or();
        if self.eat(&Token::QuestionQuestion) {
            let rhs = self.parse_coalesce();
            return Expr::Binary(BinOp::Coalesce, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_logical_or(&mut self) -> Expr {
        let mut lhs = self.parse_logical_and();
        while self.eat(&Token::PipePipe) {
            let rhs = self.parse_logical_and();
            lhs = Expr::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_logical_and(&mut self) -> Expr {
        let mut lhs = self.parse_bit_or();
        while self.eat(&Token::AmpAmp) {
            let rhs = self.parse_bit_or();
            lhs = Expr::Binary(BinOp::And, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_bit_or(&mut self) -> Expr {
        let mut lhs = self.parse_bit_xor();
        while self.eat(&Token::Pipe) {
            let rhs = self.parse_bit_xor();
            lhs = Expr::Binary(BinOp::BitOr, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_bit_xor(&mut self) -> Expr {
        let mut lhs = self.parse_bit_and();
        while self.eat(&Token::Caret) {
            let rhs = self.parse_bit_and();
            lhs = Expr::Binary(BinOp::BitXor, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_bit_and(&mut self) -> Expr {
        let mut lhs = self.parse_equality();
        while self.eat(&Token::Amp) {
            let rhs = self.parse_equality();
            lhs = Expr::Binary(BinOp::BitAnd, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_equality(&mut self) -> Expr {
        let mut lhs = self.parse_relational();
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
            let rhs = self.parse_relational();
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_relational(&mut self) -> Expr {
        let mut lhs = self.parse_shift();
        loop {
            let op = match self.peek() {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEquals => BinOp::LtEq,
                Token::GtEquals => BinOp::GtEq,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_shift();
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_shift(&mut self) -> Expr {
        let mut lhs = self.parse_additive();
        loop {
            let op = match self.peek() {
                Token::LtLt => BinOp::Shl,
                Token::GtGt => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_additive();
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_additive(&mut self) -> Expr {
        let mut lhs = self.parse_multiplicative();
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                Token::Dot => BinOp::Concat,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_multiplicative();
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_multiplicative(&mut self) -> Expr {
        let mut lhs = self.parse_instanceof();
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_instanceof();
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_instanceof(&mut self) -> Expr {
        let lhs = self.parse_unary();
        if self.eat(&Token::Keyword(Keyword::Instanceof)) {
            let class_name = self.parse_qualified_name();
            return Expr::Instanceof(Box::new(lhs), class_name);
        }
        lhs
    }

    fn parse_unary(&mut self) -> Expr {
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
            let operand = self.parse_unary();
            return Expr::Unary(op, Box::new(operand));
        }
        if self.eat(&Token::At) {
            // error-suppression operator: not modeled, transparent wrapper
            return self.parse_unary();
        }
        self.parse_pow()
    }

    fn parse_pow(&mut self) -> Expr {
        let lhs = self.parse_postfix_from_primary();
        if self.eat(&Token::StarStar) {
            let rhs = self.parse_unary(); // right-assoc, binds through unary
            return Expr::Binary(BinOp::Pow, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_postfix_from_primary(&mut self) -> Expr {
        let primary = self.parse_primary();
        self.parse_postfix(primary)
    }

    fn parse_postfix(&mut self, mut expr: Expr) -> Expr {
        loop {
            match self.peek() {
                Token::Arrow | Token::NullsafeArrow => {
                    self.advance();
                    let name = match self.advance() {
                        Token::Identifier(s) => s,
                        Token::Variable(s) => s, // $obj->$prop dynamic access, best-effort
                        other => panic!(
                            "parse error: expected property/method name, found {:?}",
                            other
                        ),
                    };
                    if self.check(&Token::LParen) {
                        let args = self.parse_args();
                        expr = Expr::MethodCall(Box::new(expr), name, args);
                    } else {
                        expr = Expr::PropertyAccess(Box::new(expr), name);
                    }
                }
                Token::DoubleColon => {
                    self.advance();
                    let class_name = match &expr {
                        Expr::Name(n) => n.clone(),
                        _ => "?".to_string(),
                    };
                    let member = match self.advance() {
                        Token::Identifier(s) => s,
                        Token::Variable(s) => format!("${s}"),
                        Token::Keyword(Keyword::Class) => "class".to_string(),
                        other => panic!("parse error: expected static member, found {:?}", other),
                    };
                    if self.check(&Token::LParen) {
                        let args = self.parse_args();
                        expr = Expr::StaticCall(class_name, member, args);
                    } else {
                        expr = Expr::StaticAccess(class_name, member);
                    }
                }
                Token::LBracket => {
                    self.advance();
                    let index = if self.check(&Token::RBracket) {
                        None
                    } else {
                        Some(Box::new(self.parse_expression()))
                    };
                    self.expect(&Token::RBracket);
                    expr = Expr::Index(Box::new(expr), index);
                }
                Token::LParen => {
                    let args = self.parse_args();
                    expr = Expr::Call(Box::new(expr), args);
                }
                Token::Increment => {
                    self.advance();
                    expr = Expr::Unary(UnaryOp::PostInc, Box::new(expr));
                }
                Token::Decrement => {
                    self.advance();
                    expr = Expr::Unary(UnaryOp::PostDec, Box::new(expr));
                }
                _ => break,
            }
        }
        expr
    }

    fn parse_args(&mut self) -> Vec<Expr> {
        self.expect(&Token::LParen);
        let mut args = Vec::new();
        while !self.check(&Token::RParen) {
            self.eat(&Token::Ellipsis); // spread: argument still parsed normally
            // named arguments (`name: expr`) - accept and discard the name
            if let Token::Identifier(_) = self.peek()
                && self.peek_at(1) == &Token::Colon
            {
                self.advance();
                self.advance();
            }
            args.push(self.parse_expression());
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen);
        args
    }

    fn parse_array_items(&mut self, close: &Token) -> Vec<(Option<Expr>, Expr)> {
        let mut items = Vec::new();
        while !self.check(close) {
            self.eat(&Token::Ellipsis);
            self.eat(&Token::Amp); // by-ref element, not modeled
            let first = self.parse_expression();
            if self.eat(&Token::FatArrow) {
                self.eat(&Token::Amp);
                let value = self.parse_expression();
                items.push((Some(first), value));
            } else {
                items.push((None, first));
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(close);
        items
    }

    fn parse_primary(&mut self) -> Expr {
        match self.peek().clone() {
            Token::Integer(n) => {
                self.advance();
                Expr::Literal(Literal::Int(n))
            }
            Token::Float(f) => {
                self.advance();
                Expr::Literal(Literal::Float(f))
            }
            Token::StringLit(s) => {
                self.advance();
                Expr::Literal(Literal::String(s))
            }
            Token::Keyword(Keyword::True) => {
                self.advance();
                Expr::Literal(Literal::Bool(true))
            }
            Token::Keyword(Keyword::False) => {
                self.advance();
                Expr::Literal(Literal::Bool(false))
            }
            Token::Keyword(Keyword::Null) => {
                self.advance();
                Expr::Literal(Literal::Null)
            }
            Token::Variable(name) => {
                self.advance();
                Expr::Variable(name)
            }
            Token::LParen => {
                self.advance();
                let inner = self.parse_expression();
                self.expect(&Token::RParen);
                Expr::Grouping(Box::new(inner))
            }
            Token::LBracket => {
                self.advance();
                Expr::ArrayLiteral(self.parse_array_items(&Token::RBracket))
            }
            Token::Keyword(Keyword::Array) => {
                self.advance();
                self.expect(&Token::LParen);
                Expr::ArrayLiteral(self.parse_array_items(&Token::RParen))
            }
            Token::Keyword(Keyword::List) => {
                self.advance();
                self.expect(&Token::LParen);
                Expr::ArrayLiteral(self.parse_array_items(&Token::RParen))
            }
            Token::Keyword(Keyword::New) => {
                self.advance();
                let class_expr = match self.peek().clone() {
                    Token::Variable(name) => {
                        self.advance();
                        Expr::Variable(name)
                    }
                    _ => Expr::Name(self.parse_qualified_name()),
                };
                let args = if self.check(&Token::LParen) {
                    self.parse_args()
                } else {
                    Vec::new()
                };
                Expr::New(Box::new(class_expr), args)
            }
            Token::Keyword(Keyword::Clone) => {
                self.advance();
                let e = self.parse_unary();
                Expr::Unary(UnaryOp::Not, Box::new(e)) // placeholder: clone-as-unary, refine later
            }
            Token::Keyword(Keyword::Print) => {
                self.advance();
                let e = self.parse_assignment();
                Expr::Call(Box::new(Expr::Name("print".to_string())), vec![e])
            }
            Token::Keyword(Keyword::Static)
                if matches!(
                    self.peek_at(1),
                    Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Fn)
                ) =>
            {
                self.advance();
                self.parse_closure_like(true)
            }
            Token::Keyword(Keyword::Static) => {
                self.advance();
                Expr::Name("static".to_string())
            }
            Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Fn) => {
                self.parse_closure_like(false)
            }
            Token::Keyword(Keyword::Match) => self.parse_match_expr(),
            Token::InterpString(parts) => {
                self.advance();
                self.build_interpolated(parts)
            }
            Token::Identifier(_) | Token::Backslash => {
                let name = self.parse_qualified_name();
                Expr::Name(name)
            }
            other => {
                panic!(
                    "parse error: unexpected token in expression: {:?} at {}",
                    other, self.pos
                );
            }
        }
    }

    fn parse_closure_like(&mut self, is_static: bool) -> Expr {
        if self.check(&Token::Keyword(Keyword::Fn)) {
            self.parse_arrow_fn(is_static)
        } else {
            self.parse_closure(is_static)
        }
    }

    fn parse_closure(&mut self, is_static: bool) -> Expr {
        self.expect(&Token::Keyword(Keyword::Function));
        let by_ref = self.eat(&Token::Amp);
        let params = self.parse_params();

        let mut uses = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Use)) {
            self.expect(&Token::LParen);
            while !self.check(&Token::RParen) {
                let use_by_ref = self.eat(&Token::Amp);
                let name = self.expect_variable();
                uses.push(ClosureUse {
                    name,
                    by_ref: use_by_ref,
                });
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen);
        }

        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()
        } else {
            None
        };

        let body = if let Stmt::Block(stmts) = self.parse_block() {
            stmts
        } else {
            unreachable!()
        };

        Expr::Closure {
            params,
            uses,
            body,
            is_static,
            by_ref,
            return_type,
        }
    }

    fn parse_arrow_fn(&mut self, is_static: bool) -> Expr {
        self.expect(&Token::Keyword(Keyword::Fn));
        self.eat(&Token::Amp);
        let params = self.parse_params();
        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()
        } else {
            None
        };
        self.expect(&Token::FatArrow);
        let body = self.parse_assignment();
        Expr::ArrowFn {
            params,
            body: Box::new(body),
            is_static,
            return_type,
        }
    }

    fn parse_match_expr(&mut self) -> Expr {
        self.expect(&Token::Keyword(Keyword::Match));
        self.expect(&Token::LParen);
        let subject = self.parse_expression();
        self.expect(&Token::RParen);
        self.expect(&Token::LBrace);

        let mut arms = Vec::new();
        let mut default = None;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            if self.eat(&Token::Keyword(Keyword::Default)) {
                self.expect(&Token::FatArrow);
                default = Some(Box::new(self.parse_expression()));
            } else {
                let mut conds = vec![self.parse_expression()];
                while self.eat(&Token::Comma) {
                    if self.check(&Token::FatArrow) {
                        break;
                    }
                    conds.push(self.parse_expression());
                }
                self.expect(&Token::FatArrow);
                let result = self.parse_expression();
                arms.push((conds, result));
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace);

        Expr::Match {
            subject: Box::new(subject),
            arms,
            default,
        }
    }

    fn build_interpolated(&mut self, parts: Vec<StringPart>) -> Expr {
        let mut exprs = Vec::new();
        for part in parts {
            match part {
                StringPart::Literal(s) => exprs.push(Expr::Literal(Literal::String(s))),
                StringPart::SimpleVar(name) => exprs.push(Expr::Variable(name)),
                StringPart::ComplexExpr(src) => {
                    let tokens = Lexer::new_in_php(&src).tokenize();
                    let mut sub = Parser::new(tokens);
                    exprs.push(sub.parse_expression());
                }
            }
        }
        if exprs.len() == 1
            && let Some(Expr::Literal(Literal::String(_))) = exprs.first()
        {
            return exprs.into_iter().next().unwrap();
        }
        Expr::Interpolated(exprs)
    }
}
