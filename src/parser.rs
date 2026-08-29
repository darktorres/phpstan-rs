use crate::ast::*;
use crate::lexer::Lexer;
use crate::token::{Keyword, StringPart, Token};

// Real PHP's own engine imposes a call-stack-depth guard specifically so
// pathologically nested input (`~~~~~~...~~~~~1`, deeply chained `??`,
// deeply nested parens/arrays) produces a normal fatal error instead of
// crashing the process - php-src's own Zend/tests/stack_limit/*.phpt exist
// to test exactly that. This parser's few genuinely self-recursive (not
// while-loop) productions need the same guard: a stack overflow is a hard
// fault, not a panic, so it can't be caught by anything upstream (a batch
// checker's catch_unwind included) and takes the whole process down.
const MAX_EXPR_DEPTH: usize = 1000;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    // A doc comment's raw text, keyed by the index (in `tokens`, after
    // filtering) of the token it immediately precedes. Built once up front
    // so the rest of the parser never has to know DocComment tokens
    // existed - only the specific call sites that attach one to a
    // declaration look here.
    doc_comments: std::collections::HashMap<usize, String>,
    // Incremented/decremented around the parser's self-recursive
    // expression productions (parse_unary, parse_assignment,
    // parse_coalesce) - see MAX_EXPR_DEPTH.
    expr_depth: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        let mut filtered = Vec::with_capacity(tokens.len());
        let mut doc_comments = std::collections::HashMap::new();
        let mut pending: Option<String> = None;
        for tok in tokens {
            match tok {
                Token::DocComment(s) => pending = Some(s),
                other => {
                    if let Some(doc) = pending.take() {
                        doc_comments.insert(filtered.len(), doc);
                    }
                    filtered.push(other);
                }
            }
        }
        Parser {
            tokens: filtered,
            pos: 0,
            doc_comments,
            expr_depth: 0,
        }
    }

    /// Consumes any doc comment immediately preceding the current position -
    /// call this before advancing past the first token of a declaration.
    fn take_doc_comment(&mut self) -> Option<String> {
        self.doc_comments.remove(&self.pos)
    }

    /// Bumps the expression-recursion depth counter, panicking (a normal,
    /// catchable error) once it passes MAX_EXPR_DEPTH instead of letting
    /// the recursion keep going until it segfaults. Callers must pair this
    /// with a matching decrement once their own recursive call returns -
    /// see the three call sites for the exact pattern.
    fn enter_expr(&mut self) {
        self.expr_depth += 1;
        if self.expr_depth > MAX_EXPR_DEPTH {
            panic!("parse error: expression nested too deeply (over {MAX_EXPR_DEPTH} levels)");
        }
    }

    fn leave_expr(&mut self) {
        self.expr_depth -= 1;
    }

    pub fn parse_program(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while !self.at_eof() {
            if matches!(self.peek(), Token::Keyword(Keyword::Namespace)) {
                stmts.extend(self.parse_namespace_stmt());
            } else {
                stmts.push(self.parse_statement());
            }
        }
        stmts
    }

    /// The older, braced namespace form - `namespace Foo\Bar { ... }`, or
    /// bare `namespace { ... }` for the global namespace - as opposed to
    /// the common `namespace Foo\Bar;` statement form (still handled
    /// generically in parse_statement). Only legal at the top level of a
    /// file, so this is special-cased in parse_program rather than making
    /// every parse_statement caller handle getting more than one Stmt
    /// back. Namespaces aren't modeled beyond the Stmt::Namespace marker,
    /// so the block's body is spliced into the surrounding statement list
    /// instead of nested - that's also what keeps its declarations visible
    /// to Registry::index, which only scans the top-level Vec<Stmt>.
    fn parse_namespace_stmt(&mut self) -> Vec<Stmt> {
        self.advance(); // `namespace`
        let name = self.parse_qualified_name();
        if self.check(&Token::LBrace) {
            let mut stmts = vec![Stmt::Namespace(name)];
            stmts.extend(self.parse_stmt_list_block());
            stmts
        } else {
            self.expect_semicolon();
            vec![Stmt::Namespace(name)]
        }
    }

    // ---- token stream helpers ----

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek_at(&self, offset: usize) -> &Token {
        self.tokens.get(self.pos + offset).unwrap_or(&Token::Eof)
    }

    /// Is the current position a `(set)` asymmetric-visibility marker
    /// (`private(set)`, `public(set)`, `protected(set)`)? Checked at the
    /// visibility keyword itself, i.e. this looks 3 tokens ahead for the
    /// literal `( set )`. Needed to tell it apart from a visibility
    /// keyword immediately followed by an unrelated `(` for some other
    /// reason - most notably `public (A&B)|X $prop;`, a DNF type hint
    /// that also happens to start with `(`.
    fn peek_is_set_visibility_marker(&self) -> bool {
        matches!(self.peek_at(1), Token::LParen)
            && matches!(self.peek_at(2), Token::Identifier(s) if s == "set")
            && matches!(self.peek_at(3), Token::RParen)
    }

    /// Scans forward from the current `(` (without consuming anything) for
    /// a comma at that paren's own nesting level before its matching `)` -
    /// used to tell PHP 8.5's two-argument `clone(obj, overrides)` form
    /// apart from a plain `clone(expr)` (the classic unary form, spelled
    /// with parens around its operand).
    fn clone_call_has_top_level_comma(&self) -> bool {
        let mut depth = 0i32;
        let mut offset = 0usize;
        loop {
            match self.peek_at(offset) {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return false;
                    }
                }
                Token::Comma if depth == 1 => return true,
                Token::Eof => return false,
                _ => {}
            }
            offset += 1;
        }
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

    /// Used for every "name in declaration position" (function/class/
    /// const/case names, ...). Keyword-tolerant for the same reason
    /// take_name_segment is: PHP reserves most keywords only in specific
    /// declaration positions, not universally - `const DEFAULT = 1;` is
    /// valid PHP even though `default` is a keyword.
    fn expect_ident(&mut self) -> String {
        match self.peek().clone() {
            Token::Identifier(_) | Token::Keyword(_) => self.take_name_segment().unwrap(),
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
                let mut names = vec![self.parse_global_target()];
                while self.eat(&Token::Comma) {
                    names.push(self.parse_global_target());
                }
                self.expect_semicolon();
                Stmt::Global(names)
            }
            // `static $x = 1, $y;` - guarded on the next token being a
            // Variable so this doesn't shadow `static::`/`static function`/
            // `static fn`, all of which are expressions, not statements,
            // and stay handled in parse_primary.
            Token::Keyword(Keyword::Static) if matches!(self.peek_at(1), Token::Variable(_)) => {
                self.advance();
                let mut vars = Vec::new();
                loop {
                    let name = self.expect_variable();
                    let default = if self.eat(&Token::Equals) {
                        Some(self.parse_expression())
                    } else {
                        None
                    };
                    vars.push((name, default));
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect_semicolon();
                Stmt::StaticVar(vars)
            }
            Token::Keyword(Keyword::Function) => {
                let doc = self.take_doc_comment();
                let mut f = self.parse_function_decl();
                f.doc_comment = doc;
                Stmt::FunctionDecl(f)
            }
            Token::Keyword(Keyword::Class)
            | Token::Keyword(Keyword::Abstract)
            | Token::Keyword(Keyword::Final)
            | Token::Keyword(Keyword::Readonly)
            | Token::Keyword(Keyword::Interface)
            | Token::Keyword(Keyword::Trait) => Stmt::ClassDecl(self.parse_class_decl()),
            Token::Keyword(Keyword::Enum) => Stmt::EnumDecl(self.parse_enum_decl()),
            Token::Keyword(Keyword::Namespace) => {
                self.advance();
                let name = self.parse_qualified_name();
                self.expect_semicolon();
                Stmt::Namespace(name)
            }
            Token::Keyword(Keyword::Use) => {
                self.advance();
                // `use function foo;` / `use const BAR;` - the marker
                // doesn't change anything tracked here, just skip past it.
                if matches!(
                    self.peek(),
                    Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Const)
                ) {
                    self.advance();
                }
                let name = self.parse_qualified_name();
                // Group use: `use Foo\{const BAR, function baz, Qux as
                // Quux};`, with an optional trailing comma before the
                // closing brace. Nothing downstream reads Stmt::Use at
                // all (confirmed: not referenced in registry.rs or
                // checker.rs), so each member is just consumed rather
                // than threaded into its own Stmt::Use.
                if self.eat(&Token::LBrace) {
                    while !self.check(&Token::RBrace) {
                        if matches!(
                            self.peek(),
                            Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Const)
                        ) {
                            self.advance();
                        }
                        self.parse_qualified_name();
                        if self.eat(&Token::Keyword(Keyword::As)) {
                            self.expect_ident();
                        }
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RBrace);
                    self.expect_semicolon();
                    return Stmt::Use(name, None);
                }
                let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                    Some(self.expect_ident())
                } else {
                    None
                };
                self.expect_semicolon();
                Stmt::Use(name, alias)
            }
            Token::Keyword(Keyword::Const) => {
                self.advance();
                let mut consts = Vec::new();
                loop {
                    let name = self.expect_ident();
                    self.expect(&Token::Equals);
                    let value = self.parse_expression();
                    consts.push(ClassConst { name, value });
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect_semicolon();
                Stmt::GlobalConst(consts)
            }
            Token::Keyword(Keyword::Try) => self.parse_try(),
            Token::Keyword(Keyword::Throw) => {
                self.advance();
                let e = self.parse_expression();
                self.expect_semicolon();
                Stmt::Throw(e)
            }
            Token::Keyword(Keyword::Switch) => self.parse_switch(),
            // `declare(strict_types=1);` / `declare(ticks=1) { ... }` -
            // `declare` isn't a reserved word (a real PHP file can have a
            // function or constant named `declare`), so it's only treated
            // specially when immediately followed by `(`. The directives
            // themselves (`name = value` pairs) aren't modeled - just
            // consumed as generic expressions - and the optional
            // block-scoped form's body is a plain nested block, unlike
            // `namespace { ... }`, since declare directives don't carry
            // any declarations that a registry needs to see spliced out.
            Token::Identifier(s) if s == "declare" && matches!(self.peek_at(1), Token::LParen) => {
                self.advance();
                self.expect(&Token::LParen);
                while !self.check(&Token::RParen) {
                    self.parse_expression();
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen);
                if self.check(&Token::LBrace) {
                    self.parse_block()
                } else {
                    self.expect_semicolon();
                    Stmt::Block(Vec::new())
                }
            }
            Token::Keyword(Keyword::Goto) => {
                self.advance();
                let name = self.expect_ident();
                self.expect_semicolon();
                Stmt::Goto(name)
            }
            // `label:` - a goto target. Unambiguous at statement position:
            // nothing else in PHP's grammar puts a bare `identifier:` here
            // (ternary's `:` always follows a `?`, and array/match arms
            // use `=>`, not `:`).
            Token::Identifier(name) if matches!(self.peek_at(1), Token::Colon) => {
                self.advance();
                self.advance();
                Stmt::Label(name)
            }
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

    /// A `global` statement's target - almost always a plain `$name`, but
    /// PHP also allows `${expr}` there (`global ${sf("a")};`) for a
    /// runtime-computed variable name. Not modeled beyond parsing past it
    /// (the checker just gets a placeholder "always defined" entry, which
    /// is harmless since nothing else can reference this exact name).
    fn parse_global_target(&mut self) -> String {
        if self.eat(&Token::DollarBrace) {
            self.parse_expression();
            self.expect(&Token::RBrace);
            return "{dynamic}".to_string();
        }
        self.expect_variable()
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

    /// A loop/if body, accepting either `{ ... }` / a bare statement, or
    /// PHP's alternative colon syntax (`: ... endwhile;` etc, common in
    /// templates that interleave HTML and PHP).
    fn parse_body(&mut self, alt_end: Keyword) -> Box<Stmt> {
        if self.check(&Token::Colon) {
            Box::new(Stmt::Block(self.parse_alt_block(alt_end)))
        } else {
            Box::new(self.parse_braced_or_single())
        }
    }

    fn parse_alt_block(&mut self, end_kw: Keyword) -> Vec<Stmt> {
        self.expect(&Token::Colon);
        let mut stmts = Vec::new();
        while !self.check(&Token::Keyword(end_kw.clone())) && !self.at_eof() {
            stmts.push(self.parse_statement());
        }
        self.expect(&Token::Keyword(end_kw));
        self.expect_semicolon();
        stmts
    }

    fn parse_if(&mut self) -> Stmt {
        self.advance(); // if
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);

        if self.check(&Token::Colon) {
            return self.parse_if_alt(cond);
        }

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

    // `if (c1): ... elseif (c2): ... else: ... endif;` - only one `endif;`
    // terminates the whole chain, so branches are collected flat and then
    // folded (innermost-out) into the same nested Stmt::If shape the brace
    // syntax produces.
    fn parse_if_alt(&mut self, first_cond: Expr) -> Stmt {
        let mut branches: Vec<(Expr, Vec<Stmt>)> = Vec::new();
        let mut cond = first_cond;
        self.expect(&Token::Colon);
        loop {
            let mut stmts = Vec::new();
            while !matches!(
                self.peek(),
                Token::Keyword(Keyword::ElseIf)
                    | Token::Keyword(Keyword::Else)
                    | Token::Keyword(Keyword::EndIf)
            ) && !self.at_eof()
            {
                stmts.push(self.parse_statement());
            }
            branches.push((cond.clone(), stmts));
            if self.eat(&Token::Keyword(Keyword::ElseIf)) {
                self.expect(&Token::LParen);
                cond = self.parse_expression();
                self.expect(&Token::RParen);
                self.expect(&Token::Colon);
                continue;
            }
            break;
        }

        let else_stmts = if self.eat(&Token::Keyword(Keyword::Else)) {
            self.expect(&Token::Colon);
            let mut stmts = Vec::new();
            while !self.check(&Token::Keyword(Keyword::EndIf)) && !self.at_eof() {
                stmts.push(self.parse_statement());
            }
            Some(stmts)
        } else {
            None
        };

        self.expect(&Token::Keyword(Keyword::EndIf));
        self.expect_semicolon();

        let mut result = else_stmts.map(Stmt::Block);
        for (c, stmts) in branches.into_iter().rev() {
            result = Some(Stmt::If {
                cond: c,
                then_branch: Box::new(Stmt::Block(stmts)),
                else_branch: result.map(Box::new),
            });
        }
        result.unwrap()
    }

    fn parse_while(&mut self) -> Stmt {
        self.advance();
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);
        let body = self.parse_body(Keyword::EndWhile);
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
        let body = self.parse_body(Keyword::EndFor);
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
        let body = self.parse_body(Keyword::EndForeach);
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

        let alt = self.check(&Token::Colon);
        if alt {
            self.advance();
        } else {
            self.expect(&Token::LBrace);
        }

        let mut cases: Vec<SwitchCase> = Vec::new();
        let closer = Token::Keyword(Keyword::EndSwitch);
        while !self.at_eof() && !self.check(if alt { &closer } else { &Token::RBrace }) {
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

        if alt {
            self.expect(&closer);
            self.expect_semicolon();
        } else {
            self.expect(&Token::RBrace);
        }

        Stmt::Switch { subject, cases }
    }

    // A qualified name is one identifier, optionally namespaced by
    // backslash-separated segments (`Foo`, `\App\Foo`, `App\Foo\Bar`).
    // Consecutive bare identifiers with no backslash between them are two
    // separate tokens, not one name - `array BASE_SECTIONS` must not
    // collapse into "arrayBASE_SECTIONS", which silently swallowed a
    // constant's own name until this was fixed.
    /// Consumes one identifier *or* keyword token as a name segment - most
    /// keywords are only reserved in declaration position in real PHP
    /// ("semi-reserved words"), so `Enum`/`Match`/`New`/... are all valid
    /// namespace segments (`Piwigo\Common\Enum\Foo`).
    fn take_name_segment(&mut self) -> Option<String> {
        match self.peek().clone() {
            Token::Identifier(part) => {
                self.advance();
                Some(part)
            }
            Token::Keyword(kw) => {
                self.advance();
                Some(kw.as_str().to_string())
            }
            _ => None,
        }
    }

    fn parse_qualified_name(&mut self) -> String {
        let mut s = String::new();
        if self.eat(&Token::Backslash) {
            s.push('\\');
        }
        match self.take_name_segment() {
            Some(part) => s.push_str(&part),
            None => return s,
        }
        while matches!(self.peek(), Token::Backslash) {
            self.advance();
            s.push('\\');
            match self.take_name_segment() {
                Some(part) => s.push_str(&part),
                None => break,
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
                | Token::LParen
                | Token::Keyword(Keyword::Namespace)
                | Token::Keyword(Keyword::Array)
                | Token::Keyword(Keyword::Static)
                | Token::Keyword(Keyword::Null)
                | Token::Keyword(Keyword::True)
                | Token::Keyword(Keyword::False)
        )
    }

    // Consumes one qualified type name, e.g. `int`, `array`, `\App\Foo`, or
    // a PHP 8.2 DNF (disjunctive normal form) parenthesized intersection
    // group, e.g. `(Countable&Iterator)` as one term of a union type like
    // `(Countable&Iterator)|null`. Real PHP only allows one level of
    // parens here (no `((A&B)|C)&D`), so this doesn't need to recurse into
    // a further union inside the parens - just a run of `&`-joined atoms.
    fn parse_type_atom(&mut self) -> String {
        if self.eat(&Token::LParen) {
            let mut inner = self.parse_type_atom();
            while self.eat(&Token::Amp) {
                inner.push('&');
                inner.push_str(&self.parse_type_atom());
            }
            self.expect(&Token::RParen);
            return format!("({inner})");
        }
        // These are single keyword tokens - namespaced forms like
        // `array\Foo` don't exist, so each just consumes itself and stops,
        // rather than looping on to swallow whatever bare identifier
        // happens to follow (e.g. `array $items`'s `$items`, or
        // `const array NAME`'s own `NAME`).
        match self.peek().clone() {
            Token::Keyword(Keyword::Array) => {
                self.advance();
                return "array".to_string();
            }
            Token::Keyword(Keyword::Static) => {
                self.advance();
                return "static".to_string();
            }
            Token::Keyword(Keyword::Null) => {
                self.advance();
                return "null".to_string();
            }
            // PHP 8.2+ stand-alone `true`/`false` types, e.g.
            // `function f(): string|false {}`.
            Token::Keyword(Keyword::True) => {
                self.advance();
                return "true".to_string();
            }
            Token::Keyword(Keyword::False) => {
                self.advance();
                return "false".to_string();
            }
            _ => {}
        }
        // Otherwise it's a (possibly namespaced) identifier: `int`, `Foo`,
        // `\App\Foo`, `App\Foo\Bar` - never two unrelated identifiers
        // glued together with no backslash between them.
        self.parse_qualified_name()
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
            // A visibility modifier here means constructor property
            // promotion (`public function __construct(private int $id) {}`)
            // - `readonly` can appear alongside it but doesn't itself imply
            // promotion, so it's still just consumed and dropped.
            let mut promoted_visibility = None;
            loop {
                // PHP 8.4 asymmetric visibility on a promoted property
                // (`public public(set) string $a`) - see the identical
                // case in parse_class_body's property-modifier loop.
                if matches!(
                    self.peek(),
                    Token::Keyword(Keyword::Public | Keyword::Private | Keyword::Protected)
                ) && self.peek_is_set_visibility_marker()
                {
                    self.advance();
                    self.advance();
                    self.expect_ident();
                    self.expect(&Token::RParen);
                    continue;
                }
                match self.peek() {
                    Token::Keyword(Keyword::Public) => {
                        promoted_visibility = Some(Visibility::Public)
                    }
                    Token::Keyword(Keyword::Private) => {
                        promoted_visibility = Some(Visibility::Private)
                    }
                    Token::Keyword(Keyword::Protected) => {
                        promoted_visibility = Some(Visibility::Protected)
                    }
                    Token::Keyword(Keyword::Readonly) | Token::Keyword(Keyword::Final) => {}
                    _ => break,
                }
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
                promoted_visibility,
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
            // Callers that saw a preceding doc comment overwrite this -
            // parse_function_decl itself is entered after any modifiers
            // (and thus after the doc comment's position) have already
            // been consumed.
            doc_comment: None,
        }
    }

    // ---- classes ----

    /// True when, starting right after `new`, the upcoming tokens are
    /// `[readonly|final|abstract]* class` - an anonymous class, as opposed
    /// to `new SomeExistingClass(...)`.
    fn at_anonymous_class_start(&self) -> bool {
        let mut offset = 0;
        loop {
            match self.peek_at(offset) {
                Token::Keyword(Keyword::Readonly)
                | Token::Keyword(Keyword::Final)
                | Token::Keyword(Keyword::Abstract) => offset += 1,
                Token::Keyword(Keyword::Class) => return true,
                _ => return false,
            }
        }
    }

    /// `new [readonly] class(args) [extends X] [implements Y, ...] { ... }`.
    /// Reuses parse_class_body for the `{ ... }` part, the same as a named
    /// class/interface/trait declaration.
    fn parse_anonymous_class_new(&mut self) -> Expr {
        while matches!(
            self.peek(),
            Token::Keyword(Keyword::Readonly)
                | Token::Keyword(Keyword::Final)
                | Token::Keyword(Keyword::Abstract)
        ) {
            self.advance();
        }
        self.expect(&Token::Keyword(Keyword::Class));

        let args = if self.check(&Token::LParen) {
            self.parse_args().0
        } else {
            Vec::new()
        };

        let mut extends = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Extends)) {
            extends.push(self.parse_qualified_name());
            while self.eat(&Token::Comma) {
                extends.push(self.parse_qualified_name());
            }
        }
        let mut implements = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            implements.push(self.parse_qualified_name());
            while self.eat(&Token::Comma) {
                implements.push(self.parse_qualified_name());
            }
        }

        let (properties, methods, consts, uses) = self.parse_class_body();

        let decl = ClassDecl {
            // No name multiple anonymous classes in the same file could
            // share safely - never indexed into the registry by this name,
            // only used structurally within this one expression.
            name: "class@anonymous".to_string(),
            extends,
            implements,
            is_abstract: false,
            is_final: false,
            properties,
            methods,
            consts,
            uses,
        };

        Expr::NewAnonymousClass(Box::new(decl), args)
    }

    /// The `{ ... }` body shared by class/interface/trait declarations and
    /// anonymous classes (`new class { ... }`) - extracted so the two
    /// don't duplicate this (fairly large) member-parsing loop.
    #[allow(clippy::type_complexity)]
    fn parse_class_body(
        &mut self,
    ) -> (
        Vec<PropertyDecl>,
        Vec<MethodDecl>,
        Vec<ClassConst>,
        Vec<String>,
    ) {
        self.expect(&Token::LBrace);
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut consts = Vec::new();
        let mut uses = Vec::new();

        while !self.check(&Token::RBrace) && !self.at_eof() {
            let doc = self.take_doc_comment();

            if self.eat(&Token::Keyword(Keyword::Use)) {
                uses.push(self.parse_qualified_name());
                while self.eat(&Token::Comma) {
                    uses.push(self.parse_qualified_name());
                }
                if self.eat(&Token::LBrace) {
                    // Trait adaptations (`insteadof`/`as`) aren't modeled;
                    // skip to the matching closing brace.
                    let mut depth = 1;
                    while depth > 0 && !self.at_eof() {
                        match self.advance() {
                            Token::LBrace => depth += 1,
                            Token::RBrace => depth -= 1,
                            _ => {}
                        }
                    }
                } else {
                    self.expect_semicolon();
                }
                continue;
            }

            let mut visibility = Visibility::Public;
            let mut is_static = false;
            let mut is_abstract_member = false;
            loop {
                match self.peek() {
                    // PHP 8.4 asymmetric visibility: `private(set)` /
                    // `protected(set)` as a *second* visibility modifier
                    // narrowing writes only (e.g. `public private(set)
                    // string $x`, or standalone `private(set) string $x`
                    // for public-read/private-write). Not modeled as its
                    // own concept - just consumed so the read-side
                    // `visibility` above (already set, or defaulted to
                    // Public) is left alone.
                    Token::Keyword(Keyword::Private) if self.peek_is_set_visibility_marker() => {
                        self.advance();
                        self.advance();
                        self.expect_ident();
                        self.expect(&Token::RParen);
                    }
                    Token::Keyword(Keyword::Protected) if self.peek_is_set_visibility_marker() => {
                        self.advance();
                        self.advance();
                        self.expect_ident();
                        self.expect(&Token::RParen);
                    }
                    // `public(set)` - same idea, redundantly naming the
                    // write-visibility as public too (e.g. `public
                    // public(set) string $x`).
                    Token::Keyword(Keyword::Public) if self.peek_is_set_visibility_marker() => {
                        self.advance();
                        self.advance();
                        self.expect_ident();
                        self.expect(&Token::RParen);
                    }
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
                    // The legacy PHP 4 `var $x;` property declaration -
                    // equivalent to `public $x;`. `var` isn't a reserved
                    // word (unlike every other modifier here), so it only
                    // arrives as a plain Identifier.
                    Token::Identifier(s) if s == "var" => {
                        visibility = Visibility::Public;
                        self.advance();
                    }
                    _ => break,
                }
            }

            if self.eat(&Token::Keyword(Keyword::Const)) {
                // PHP 8.3 typed class constants: `const TYPE NAME = value;`.
                // A type is present exactly when the name-like token here
                // is followed by *another* identifier (the real name)
                // rather than `=` - `const FOO = 1;`'s `FOO` is followed
                // directly by `=`, so nothing is consumed as a type there.
                if self.is_type_start() && !matches!(self.peek_at(1), Token::Equals) {
                    self.parse_type_hint();
                }
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
                let mut function = self.parse_function_decl();
                function.doc_comment = doc;
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
                // PHP 8.4 property hooks (`{ get => ...; set(Type $value)
                // { ... } }`) replace the trailing `;` on a *single*
                // property - no comma-list, no default-then-hooks. Ends
                // its own `}`, so no expect_semicolon() afterwards.
                if self.check(&Token::LBrace) {
                    let hooks = self.parse_property_hooks();
                    properties.push(PropertyDecl {
                        name,
                        visibility: visibility.clone(),
                        is_static,
                        type_hint: type_hint.clone(),
                        default,
                        doc_comment: doc.clone(),
                        hooks: Some(hooks),
                    });
                    break;
                }
                properties.push(PropertyDecl {
                    name,
                    visibility: visibility.clone(),
                    is_static,
                    type_hint: type_hint.clone(),
                    default,
                    doc_comment: doc.clone(),
                    hooks: None,
                });
                if !self.eat(&Token::Comma) {
                    self.expect_semicolon();
                    break;
                }
            }
        }
        self.expect(&Token::RBrace);

        (properties, methods, consts, uses)
    }

    /// PHP 8.4 property hooks body: `{ get => expr; set(Type $value) { .. }
    /// }`, or the abstract/interface form (`{ get; set; }`) with no bodies
    /// at all. `get`/`set` aren't keywords (they're semi-reserved words
    /// used only in this position), so they arrive as plain identifiers.
    fn parse_property_hooks(&mut self) -> PropertyHooks {
        self.expect(&Token::LBrace);
        let mut get = None;
        let mut set = None;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            // A hook can itself carry `final`/visibility modifiers ahead
            // of `get`/`set` - not modeled, just consumed.
            while matches!(
                self.peek(),
                Token::Keyword(Keyword::Final)
                    | Token::Keyword(Keyword::Public)
                    | Token::Keyword(Keyword::Private)
                    | Token::Keyword(Keyword::Protected)
            ) {
                self.advance();
            }
            // `&get => ...` - the hook returns its value by reference.
            // Aliasing isn't modeled anywhere in this checker, so it's
            // just consumed.
            self.eat(&Token::Amp);
            let is_set = matches!(self.peek(), Token::Identifier(s) if s == "set");
            self.advance(); // `get` or `set`

            let param = if is_set && self.eat(&Token::LParen) {
                let type_hint = self.parse_type_hint();
                let by_ref = self.eat(&Token::Amp);
                let name = self.expect_variable();
                self.expect(&Token::RParen);
                Some(Param {
                    name,
                    type_hint,
                    default: None,
                    by_ref,
                    variadic: false,
                    promoted_visibility: None,
                })
            } else {
                None
            };

            let body = if self.eat(&Token::FatArrow) {
                let e = self.parse_expression();
                self.expect_semicolon();
                Some(PropertyHookBody::Arrow(e))
            } else if self.check(&Token::LBrace) {
                Some(PropertyHookBody::Block(self.parse_stmt_list_block()))
            } else {
                // Abstract form: `get;` / `set;`, no body at all.
                self.expect_semicolon();
                None
            };

            if is_set {
                set = body.map(|b| (param, b));
            } else {
                get = body;
            }
        }
        self.expect(&Token::RBrace);
        PropertyHooks { get, set }
    }

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
                // PHP 8.2 `readonly class Foo {}` (shorthand for every
                // property being readonly) - accepted in any order with
                // abstract/final, same as real PHP's grammar, but not
                // modeled as its own flag (ClassDecl doesn't track
                // per-property readonly-ness at all yet, the same way the
                // class body's own `readonly` property modifier is already
                // just consumed and dropped).
                Token::Keyword(Keyword::Readonly) => {
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

        // A class extends at most one class, but an interface can extend
        // several other interfaces (`interface C extends A, B {}`) - both
        // are accepted here since the parser doesn't distinguish the two
        // declaration kinds structurally.
        let mut extends = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Extends)) {
            extends.push(self.parse_qualified_name());
            while self.eat(&Token::Comma) {
                extends.push(self.parse_qualified_name());
            }
        }

        let mut implements = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            implements.push(self.parse_qualified_name());
            while self.eat(&Token::Comma) {
                implements.push(self.parse_qualified_name());
            }
        }

        let (properties, methods, consts, uses) = self.parse_class_body();

        ClassDecl {
            name,
            extends,
            implements,
            is_abstract,
            is_final,
            properties,
            methods,
            consts,
            uses,
        }
    }

    // `enum Name [: BackingType] [implements Iface, ...] { ...members... }`.
    // Structurally close to parse_class_decl's member loop (modifiers,
    // `use`, `const`, methods are identical), except enums have `case
    // Name [= value];` instead of properties - they can't declare instance
    // properties or extend anything.
    fn parse_enum_decl(&mut self) -> EnumDecl {
        self.expect(&Token::Keyword(Keyword::Enum));
        let name = self.expect_ident();

        let backing_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()
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
        let mut cases = Vec::new();
        let mut methods = Vec::new();
        let mut consts = Vec::new();
        let mut uses = Vec::new();

        while !self.check(&Token::RBrace) && !self.at_eof() {
            let doc = self.take_doc_comment();

            if self.eat(&Token::Keyword(Keyword::Case)) {
                let case_name = self.expect_ident();
                let value = if self.eat(&Token::Equals) {
                    Some(self.parse_expression())
                } else {
                    None
                };
                self.expect_semicolon();
                cases.push(EnumCase {
                    name: case_name,
                    value,
                });
                continue;
            }

            if self.eat(&Token::Keyword(Keyword::Use)) {
                uses.push(self.parse_qualified_name());
                while self.eat(&Token::Comma) {
                    uses.push(self.parse_qualified_name());
                }
                if self.eat(&Token::LBrace) {
                    let mut depth = 1;
                    while depth > 0 && !self.at_eof() {
                        match self.advance() {
                            Token::LBrace => depth += 1,
                            Token::RBrace => depth -= 1,
                            _ => {}
                        }
                    }
                } else {
                    self.expect_semicolon();
                }
                continue;
            }

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
                // PHP 8.3 typed class constants: `const TYPE NAME = value;`.
                // A type is present exactly when the name-like token here
                // is followed by *another* identifier (the real name)
                // rather than `=` - `const FOO = 1;`'s `FOO` is followed
                // directly by `=`, so nothing is consumed as a type there.
                if self.is_type_start() && !matches!(self.peek_at(1), Token::Equals) {
                    self.parse_type_hint();
                }
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
                let mut function = self.parse_function_decl();
                function.doc_comment = doc;
                methods.push(MethodDecl {
                    visibility,
                    is_static,
                    is_abstract: is_abstract_member,
                    function,
                });
                continue;
            }

            // Not valid PHP (enums can't have properties), but consume one
            // token and keep going rather than loop forever or panic on
            // whatever unexpected thing is here.
            self.advance();
        }
        self.expect(&Token::RBrace);

        EnumDecl {
            name,
            backing_type,
            implements,
            cases,
            methods,
            consts,
            uses,
        }
    }

    // ---- expressions (precedence climbing) ----

    pub fn parse_expression(&mut self) -> Expr {
        self.parse_low_precedence_or()
    }

    // `and`/`or`/`xor` bind looser than assignment - the classic PHP
    // gotcha where `$a = $b or $c;` parses as `($a = $b) or $c;`, not
    // `$a = ($b or $c);`, unlike `&&`/`||`. Each tier only recurses into
    // the next-tighter one, bottoming out at parse_assignment (the
    // existing top of the &&/||/ternary/etc chain), so the classic form
    // is unaffected and only these three keywords get the looser binding.
    fn parse_low_precedence_or(&mut self) -> Expr {
        let mut lhs = self.parse_low_precedence_xor();
        while self.eat(&Token::Keyword(Keyword::Or)) {
            let rhs = self.parse_low_precedence_xor();
            lhs = Expr::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_low_precedence_xor(&mut self) -> Expr {
        let mut lhs = self.parse_low_precedence_and();
        while self.eat(&Token::Keyword(Keyword::Xor)) {
            let rhs = self.parse_low_precedence_and();
            lhs = Expr::Binary(BinOp::LogicalXor, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_low_precedence_and(&mut self) -> Expr {
        let mut lhs = self.parse_assignment();
        while self.eat(&Token::Keyword(Keyword::And)) {
            let rhs = self.parse_assignment();
            lhs = Expr::Binary(BinOp::And, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    fn parse_assignment(&mut self) -> Expr {
        self.enter_expr();
        let result = self.parse_assignment_inner();
        self.leave_expr();
        result
    }

    fn parse_assignment_inner(&mut self) -> Expr {
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
            Token::AmpEquals => Some(BinOp::BitAnd),
            Token::PipeEquals => Some(BinOp::BitOr),
            Token::CaretEquals => Some(BinOp::BitXor),
            Token::LtLtEquals => Some(BinOp::Shl),
            Token::GtGtEquals => Some(BinOp::Shr),
            _ => return lhs,
        };
        self.advance();
        // Reference assignment (`$a = &$b;`) - only valid on a plain `=`,
        // never a compound one. Aliasing itself isn't modeled (nothing
        // here tracks that two variables now share storage), so this is
        // just consumed and the RHS's type still flows through normally.
        if op.is_none() {
            self.eat(&Token::Amp);
        }
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
        self.enter_expr();
        let result = self.parse_coalesce_inner();
        self.leave_expr();
        result
    }

    fn parse_coalesce_inner(&mut self) -> Expr {
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
        let mut lhs = self.parse_pipe();
        loop {
            let op = match self.peek() {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::LtEquals => BinOp::LtEq,
                Token::GtEquals => BinOp::GtEq,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_pipe();
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        lhs
    }

    /// PHP 8.5's `|> ` pipe operator: `$value |> callable` desugars to
    /// `callable($value)`. Left-associative (a while-loop, not
    /// self-recursion, so it's already immune to the stack-depth issue
    /// MAX_EXPR_DEPTH guards against). Precedence empirically checked
    /// against a real PHP 8.5 build: tighter than relational (`<`), looser
    /// than shift/concat/additive.
    fn parse_pipe(&mut self) -> Expr {
        let mut lhs = self.parse_shift();
        while self.eat(&Token::PipeGt) {
            let rhs = self.parse_shift();
            lhs = Expr::Binary(BinOp::Pipe, Box::new(lhs), Box::new(rhs));
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
            // The right-hand side is usually a static class name, but PHP
            // also allows an arbitrary expression there (`$x instanceof
            // $className`) for a runtime-determined class.
            let target = if matches!(self.peek(), Token::Variable(_)) {
                InstanceofTarget::Expr(Box::new(self.parse_unary()))
            } else {
                InstanceofTarget::Name(self.parse_qualified_name())
            };
            return Expr::Instanceof(Box::new(lhs), target);
        }
        lhs
    }

    fn parse_unary(&mut self) -> Expr {
        self.enter_expr();
        let result = self.parse_unary_inner();
        self.leave_expr();
        result
    }

    fn parse_unary_inner(&mut self) -> Expr {
        if let Some(cast_ty) = self.try_parse_cast() {
            let operand = self.parse_unary();
            return Expr::Cast(cast_ty, Box::new(operand));
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
            let operand = self.parse_unary();
            return Expr::Unary(op, Box::new(operand));
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
    fn try_parse_cast(&mut self) -> Option<String> {
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
            "string" => "string",
            "bool" | "boolean" => "bool",
            "array" => "array",
            "object" => "object",
            "unset" => "unset",
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
                    // `$obj->{expr}` - a fully dynamic property/method
                    // name, computed from an arbitrary expression rather
                    // than a fixed identifier or `$var`. `$obj->${expr}`
                    // (the name comes from a variable-variable instead of
                    // a plain `$var`) is treated the same way - the exact
                    // double-indirection isn't modeled, matching how the
                    // plain `$obj->$prop` case above already just uses
                    // the variable's own name rather than its value.
                    if self.check(&Token::LBrace) || self.check(&Token::DollarBrace) {
                        self.advance();
                        let name_expr = self.parse_expression();
                        self.expect(&Token::RBrace);
                        expr = if self.check(&Token::LParen) {
                            let (args, is_fcc) = self.parse_args();
                            let call =
                                Expr::DynamicMethodCall(Box::new(expr), Box::new(name_expr), args);
                            if is_fcc {
                                Expr::FirstClassCallable(Box::new(call))
                            } else {
                                call
                            }
                        } else {
                            Expr::DynamicPropertyAccess(Box::new(expr), Box::new(name_expr))
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
                        other => panic!(
                            "parse error: expected property/method name, found {:?}",
                            other
                        ),
                    };
                    if self.check(&Token::LParen) {
                        let (args, is_fcc) = self.parse_args();
                        let call = Expr::MethodCall(Box::new(expr), name, args);
                        expr = if is_fcc {
                            Expr::FirstClassCallable(Box::new(call))
                        } else {
                            call
                        };
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
                        // `Foo::{$name}` / `Foo::${$name}` - a fully
                        // dynamic static member name. StaticAccess/
                        // StaticCall aren't given any deeper type-checking
                        // today regardless of whether the name is static
                        // or dynamic, so this is just parsed (consuming
                        // the name expression correctly) rather than
                        // threading a real Expr through here.
                        Token::LBrace | Token::DollarBrace => {
                            self.advance();
                            self.parse_expression();
                            self.expect(&Token::RBrace);
                            "{dynamic}".to_string()
                        }
                        other => panic!("parse error: expected static member, found {:?}", other),
                    };
                    if self.check(&Token::LParen) {
                        let (args, is_fcc) = self.parse_args();
                        let call = Expr::StaticCall(class_name, member, args);
                        expr = if is_fcc {
                            Expr::FirstClassCallable(Box::new(call))
                        } else {
                            call
                        };
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
                    let (args, is_fcc) = self.parse_args();
                    let call = Expr::Call(Box::new(expr), args);
                    expr = if is_fcc {
                        Expr::FirstClassCallable(Box::new(call))
                    } else {
                        call
                    };
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

    /// Returns the parsed arguments plus whether this was a first-class
    /// callable reference (`foo(...)` - literally `...` then nothing else,
    /// as opposed to a real spread argument like `foo(...$rest)`, which is
    /// followed by an expression, not directly by `)`).
    fn parse_args(&mut self) -> (Vec<Expr>, bool) {
        self.expect(&Token::LParen);
        if matches!(self.peek(), Token::Ellipsis) && matches!(self.peek_at(1), Token::RParen) {
            self.advance();
            self.advance();
            return (Vec::new(), true);
        }

        let mut args = Vec::new();
        while !self.check(&Token::RParen) {
            self.eat(&Token::Ellipsis); // spread: argument still parsed normally
            // named arguments (`name: expr`) - accept and discard the
            // name. The name is just a label, so any keyword is valid
            // here too (`create(namespace: 'x')`), not just Identifier.
            if matches!(self.peek(), Token::Identifier(_) | Token::Keyword(_))
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
        (args, false)
    }

    fn parse_array_items(&mut self, close: &Token) -> Vec<(Option<Expr>, Expr)> {
        let mut items = Vec::new();
        while !self.check(close) {
            // A skipped destructuring slot (`[, $b] = $arr;`, `[$a, , $c] =
            // ...`) - only legal on a list-assignment target, but this
            // parser doesn't distinguish that context from a real array
            // literal here, so it's accepted generically. Null stands in
            // as "nothing to bind"; the checker's destructuring/element
            // walks below already skip anything that isn't a Variable.
            if self.check(&Token::Comma) {
                items.push((None, Expr::Literal(Literal::Null)));
                self.advance();
                continue;
            }
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
            Token::DollarBrace => {
                self.advance();
                let name_expr = self.parse_expression();
                self.expect(&Token::RBrace);
                Expr::DynamicVariable(Box::new(name_expr))
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
                if self.at_anonymous_class_start() {
                    return self.parse_anonymous_class_new();
                }
                let class_expr = match self.peek().clone() {
                    Token::Variable(name) => {
                        self.advance();
                        Expr::Variable(name)
                    }
                    _ => Expr::Name(self.parse_qualified_name()),
                };
                // `new Foo(...)` first-class-callable syntax doesn't exist
                // in PHP - a constructor can't be referenced that way - so
                // the flag is simply dropped here rather than handled.
                let args = if self.check(&Token::LParen) {
                    self.parse_args().0
                } else {
                    Vec::new()
                };
                Expr::New(Box::new(class_expr), args)
            }
            Token::Keyword(Keyword::Clone) => {
                self.advance();
                // PHP 8.5's `clone($obj, $overrides)` - a distinct grammar
                // production (verified against a real PHP 8.5 build: it
                // does NOT allow a postfix chain right after it, unlike
                // the classic form) from plain `clone <expr>`, only when a
                // comma actually appears at the top level of the parens.
                // `clone($x)` with no comma is just the classic unary form
                // spelled with parens, and still binds through the full
                // postfix chain as its operand - e.g. `clone($a)->b`
                // clones `$a->b`, not `$a`.
                if matches!(self.peek(), Token::LParen) && self.clone_call_has_top_level_comma() {
                    self.advance(); // (
                    let obj = self.parse_expression();
                    self.expect(&Token::Comma);
                    let overrides = self.parse_expression();
                    self.expect(&Token::RParen);
                    return Expr::Clone(Box::new(obj), Some(Box::new(overrides)));
                }
                let e = self.parse_unary();
                Expr::Clone(Box::new(e), None)
            }
            Token::Keyword(Keyword::Print) => {
                self.advance();
                let e = self.parse_assignment();
                Expr::Call(Box::new(Expr::Name("print".to_string())), vec![e])
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
                let inner = self.parse_assignment();
                Expr::Throw(Box::new(inner))
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
                    let inner = self.parse_assignment();
                    return Expr::YieldFrom(Box::new(inner));
                }
                // A bare `yield;` (no value) - whatever follows can't
                // start an expression, so don't try to parse one.
                if matches!(
                    self.peek(),
                    Token::Semicolon
                        | Token::RParen
                        | Token::RBracket
                        | Token::RBrace
                        | Token::Comma
                ) {
                    return Expr::Yield(None, None);
                }
                let first = self.parse_ternary();
                if self.eat(&Token::FatArrow) {
                    let value = self.parse_ternary();
                    Expr::Yield(Some(Box::new(first)), Some(Box::new(value)))
                } else {
                    Expr::Yield(None, Some(Box::new(first)))
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
                let e = self.parse_assignment();
                Expr::Call(Box::new(Expr::Name(kw.as_str().to_string())), vec![e])
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
            // `namespace\Foo` - a name explicitly relative to the current
            // namespace. Only ever appears as a value in expression
            // position (the bare `namespace` statement/block forms are
            // handled separately in parse_statement/parse_program), so
            // it's unambiguous here.
            Token::Identifier(_) | Token::Backslash | Token::Keyword(Keyword::Namespace) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Vec<Stmt> {
        Parser::new(Lexer::new(src).tokenize()).parse_program()
    }

    #[test]
    fn doc_comment_attaches_to_top_level_function() {
        let program = parse(
            r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double(int $x): int { return $x * 2; }
        "#,
        );
        let Stmt::FunctionDecl(f) = &program[0] else {
            panic!("expected FunctionDecl");
        };
        assert!(f.doc_comment.as_ref().unwrap().contains("@return int"));
    }

    #[test]
    fn doc_comment_attaches_to_method_not_the_previous_one() {
        let program = parse(
            r#"<?php
            class Box {
                /** @return int */
                public function a(): int { return 1; }

                public function b(): int { return 2; }
            }
        "#,
        );
        let Stmt::ClassDecl(c) = &program[0] else {
            panic!("expected ClassDecl");
        };
        assert!(c.methods[0].function.doc_comment.is_some());
        assert!(c.methods[1].function.doc_comment.is_none());
    }

    #[test]
    fn doc_comment_attaches_to_property() {
        let program = parse(
            r#"<?php
            class Box {
                /** @var int */
                public $value;
            }
        "#,
        );
        let Stmt::ClassDecl(c) = &program[0] else {
            panic!("expected ClassDecl");
        };
        assert!(
            c.properties[0]
                .doc_comment
                .as_ref()
                .unwrap()
                .contains("@var int")
        );
    }

    #[test]
    fn empty_slash_star_star_slash_is_not_treated_as_a_doc_comment() {
        // `/**/` is the classic ambiguous case: it looks like the start of
        // a doc comment but is conventionally just an empty regular one.
        let program = parse(
            r#"<?php
            /**/
            function f(): int { return 1; }
        "#,
        );
        let Stmt::FunctionDecl(f) = &program[0] else {
            panic!("expected FunctionDecl");
        };
        assert!(f.doc_comment.is_none());
    }

    #[test]
    fn regular_comment_is_not_captured_as_a_doc_comment() {
        let program = parse(
            r#"<?php
            // just a regular comment
            function f(): int { return 1; }
        "#,
        );
        let Stmt::FunctionDecl(f) = &program[0] else {
            panic!("expected FunctionDecl");
        };
        assert!(f.doc_comment.is_none());
    }

    #[test]
    fn top_level_const_declaration_parses() {
        let program = parse("<?php const FOO = 1, BAR = 2;");
        let Stmt::GlobalConst(consts) = &program[0] else {
            panic!("expected GlobalConst");
        };
        assert_eq!(consts.len(), 2);
        assert_eq!(consts[0].name, "FOO");
        assert_eq!(consts[1].name, "BAR");
    }

    #[test]
    fn true_and_false_are_valid_return_types() {
        // PHP 8.2+ stand-alone true/false types, as used throughout
        // php-src's own stub files (e.g. `string|false`).
        let program = parse("<?php function f(): string|false { return false; }");
        let Stmt::FunctionDecl(f) = &program[0] else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(f.return_type.as_deref(), Some("string|false"));
    }

    #[test]
    fn pure_enum_parses_with_unbacked_cases() {
        let program = parse(
            r#"<?php
            enum SortDirection {
                case Ascending;
                case Descending;
            }
        "#,
        );
        let Stmt::EnumDecl(e) = &program[0] else {
            panic!("expected EnumDecl");
        };
        assert_eq!(e.name, "SortDirection");
        assert!(e.backing_type.is_none());
        assert_eq!(e.cases.len(), 2);
        assert_eq!(e.cases[0].name, "Ascending");
        assert!(e.cases[0].value.is_none());
    }

    #[test]
    fn backed_enum_with_interface_and_method_parses() {
        let program = parse(
            r#"<?php
            enum Suit: string implements \JsonSerializable
            {
                case Hearts = 'H';

                public function label(): string
                {
                    return match ($this) {
                        Suit::Hearts => 'Hearts',
                    };
                }
            }
        "#,
        );
        let Stmt::EnumDecl(e) = &program[0] else {
            panic!("expected EnumDecl");
        };
        assert_eq!(e.backing_type.as_deref(), Some("string"));
        assert_eq!(e.implements, vec!["\\JsonSerializable".to_string()]);
        assert!(matches!(e.cases[0].value, Some(Expr::Literal(_))));
        assert_eq!(e.methods.len(), 1);
        assert_eq!(e.methods[0].function.name, "label");
    }

    #[test]
    fn word_operators_bind_looser_than_assignment() {
        // The classic PHP gotcha: `$a = $b or $c;` is `($a = $b) or $c;`,
        // not `$a = ($b or $c);` - unlike &&/||.
        let program = parse("<?php $x = 5 or die();");
        let Stmt::Expression(Expr::Binary(BinOp::Or, lhs, _)) = &program[0] else {
            panic!("expected a top-level Or binary expression");
        };
        assert!(matches!(lhs.as_ref(), Expr::Assign(..)));
    }

    #[test]
    fn readonly_class_parses() {
        let program = parse("<?php readonly class Point { public int $x; }");
        assert!(matches!(program[0], Stmt::ClassDecl(_)));
    }

    #[test]
    fn qualified_name_with_keyword_segment_parses() {
        // `Enum` here is a namespace segment, not the `enum` declaration
        // keyword - PHP's keywords are mostly "semi-reserved". The main
        // assertion is just that this doesn't panic (it used to). Known,
        // low-impact precision loss: Keyword::as_str() only has the
        // canonical lowercase spelling, not this source's original
        // capitalization, so the reconstructed segment comes back as
        // "enum" - inert for a `use` target specifically, since nothing
        // in the registry/checker consumes Stmt::Use at all yet, and for
        // symbols that *are* looked up (extends/implements/new Foo()),
        // both the declaration site and every usage site lowercase the
        // same way, so registry lookups on a keyword-named symbol still
        // match each other consistently.
        let program = parse("<?php use Piwigo\\Common\\Enum\\AlbumSortOrder;");
        let Stmt::Use(name, _) = &program[0] else {
            panic!("expected Use");
        };
        assert_eq!(name, "Piwigo\\Common\\enum\\AlbumSortOrder");
    }

    #[test]
    fn keyword_as_method_and_static_member_name_parses() {
        let program = parse(
            r#"<?php
            $x = $obj->match();
            $y = Foo::new();
            $z = Foo::list();
        "#,
        );
        assert_eq!(program.len(), 3);
    }

    #[test]
    fn keyword_as_named_argument_name_parses() {
        let program = parse("<?php f(namespace: 'x', default: 1);");
        assert!(matches!(program[0], Stmt::Expression(_)));
    }

    #[test]
    fn first_class_callable_parses_distinctly_from_a_real_call() {
        let program = parse("<?php $f = strlen(...);");
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[0] else {
            panic!("expected an assignment");
        };
        assert!(matches!(rhs.as_ref(), Expr::FirstClassCallable(_)));
    }

    #[test]
    fn include_and_require_parse_without_parens() {
        let program = parse("<?php include $file; require_once $other;");
        assert_eq!(program.len(), 2);
    }

    #[test]
    fn static_local_variable_declaration_parses() {
        let program = parse("<?php function f() { static $count = 0; return $count; } ");
        let Stmt::FunctionDecl(f) = &program[0] else {
            panic!("expected FunctionDecl");
        };
        assert!(matches!(f.body[0], Stmt::StaticVar(_)));
    }

    #[test]
    fn bitwise_compound_assignment_operators_parse() {
        let program = parse(
            r#"<?php
            function f(int $v) {
                $v |= 1;
                $v &= 2;
                $v ^= 3;
                $v <<= 1;
                $v >>= 1;
            }
        "#,
        );
        let Stmt::FunctionDecl(f) = &program[0] else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(f.body.len(), 5);
        for stmt in &f.body {
            assert!(matches!(stmt, Stmt::Expression(Expr::CompoundAssign(..))));
        }
    }

    #[test]
    fn anonymous_class_parses_with_interface_and_constructor_args() {
        let program = parse(
            r#"<?php
            $x = new readonly class(1) implements Countable {
                public function __construct(private int $n) {}
                public function count(): int { return $this->n; }
            };
        "#,
        );
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[0] else {
            panic!("expected an assignment");
        };
        let Expr::NewAnonymousClass(decl, args) = rhs.as_ref() else {
            panic!("expected NewAnonymousClass");
        };
        assert_eq!(args.len(), 1);
        assert_eq!(decl.implements, vec!["Countable".to_string()]);
        assert_eq!(decl.methods.len(), 2);
    }

    #[test]
    fn yield_forms_all_parse() {
        let program = parse(
            r#"<?php
            function gen() {
                yield 1;
                yield 'key' => 2;
                yield from [3, 4];
                yield;
            }
        "#,
        );
        let Stmt::FunctionDecl(f) = &program[0] else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(f.body.len(), 4);

        let Stmt::Expression(Expr::Yield(None, Some(_))) = &f.body[0] else {
            panic!("expected bare-value yield");
        };
        let Stmt::Expression(Expr::Yield(Some(_), Some(_))) = &f.body[1] else {
            panic!("expected key => value yield");
        };
        assert!(matches!(f.body[2], Stmt::Expression(Expr::YieldFrom(_))));
        assert!(matches!(
            f.body[3],
            Stmt::Expression(Expr::Yield(None, None))
        ));
    }

    #[test]
    fn asymmetric_visibility_property_parses() {
        let program = parse(
            r#"<?php
            class Foo {
                public private(set) string $name = 'a';
                private(set) int $count = 0;
            }
        "#,
        );
        let Stmt::ClassDecl(c) = &program[0] else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties.len(), 2);
        assert_eq!(c.properties[0].name, "name");
        assert_eq!(c.properties[0].visibility, Visibility::Public);
        assert_eq!(c.properties[1].name, "count");
        assert_eq!(c.properties[1].visibility, Visibility::Public);
    }

    #[test]
    fn property_hooks_parse() {
        let program = parse(
            r#"<?php
            class Foo {
                public string $upper {
                    get => strtoupper($this->raw);
                    set(string $value) {
                        $this->raw = strtolower($value);
                    }
                }
                public int $bare {
                    get;
                    set;
                }
            }
        "#,
        );
        let Stmt::ClassDecl(c) = &program[0] else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties.len(), 2);

        let hooks = c.properties[0].hooks.as_ref().expect("expected hooks");
        assert!(matches!(hooks.get, Some(PropertyHookBody::Arrow(_))));
        let (param, body) = hooks.set.as_ref().expect("expected set hook");
        assert_eq!(param.as_ref().unwrap().name, "value");
        assert!(matches!(body, PropertyHookBody::Block(_)));

        let bare_hooks = c.properties[1].hooks.as_ref().expect("expected hooks");
        assert!(bare_hooks.get.is_none());
        assert!(bare_hooks.set.is_none());
    }

    #[test]
    fn binary_and_explicit_octal_literals_parse() {
        let program = parse(
            r#"<?php
            $a = 0b101;
            $b = 0o775;
            $c = 0x1F;
        "#,
        );
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[0] else {
            panic!("expected assignment");
        };
        assert!(matches!(rhs.as_ref(), Expr::Literal(Literal::Int(5))));
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[1] else {
            panic!("expected assignment");
        };
        assert!(matches!(rhs.as_ref(), Expr::Literal(Literal::Int(509))));
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[2] else {
            panic!("expected assignment");
        };
        assert!(matches!(rhs.as_ref(), Expr::Literal(Literal::Int(31))));
    }

    #[test]
    fn instanceof_with_dynamic_class_name_parses() {
        let program = parse(
            r#"<?php
            assert($repository instanceof $repositoryClass);
            $ok = $x instanceof Foo;
        "#,
        );
        let Stmt::Expression(Expr::Call(_, args)) = &program[0] else {
            panic!("expected assert() call");
        };
        let Expr::Instanceof(_, target) = &args[0] else {
            panic!("expected Instanceof");
        };
        assert!(matches!(target, InstanceofTarget::Expr(_)));

        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[1] else {
            panic!("expected assignment");
        };
        let Expr::Instanceof(_, target) = rhs.as_ref() else {
            panic!("expected Instanceof");
        };
        assert!(matches!(target, InstanceofTarget::Name(n) if n == "Foo"));
    }

    #[test]
    fn reference_assignment_parses() {
        let program = parse(
            r#"<?php
            $a = &$b;
            $c =& $d;
        "#,
        );
        assert!(matches!(&program[0], Stmt::Expression(Expr::Assign(_, _))));
        assert!(matches!(&program[1], Stmt::Expression(Expr::Assign(_, _))));
    }

    #[test]
    fn destructuring_with_skipped_slot_parses() {
        let program = parse(
            r#"<?php
            [, $extension] = explode('-', $s);
            [$a, , $c] = $t;
        "#,
        );
        let Stmt::Expression(Expr::Assign(lhs, _)) = &program[0] else {
            panic!("expected assignment");
        };
        let Expr::ArrayLiteral(items) = lhs.as_ref() else {
            panic!("expected ArrayLiteral target");
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0].1, Expr::Literal(Literal::Null)));
        assert!(matches!(items[1].1, Expr::Variable(_)));

        let Stmt::Expression(Expr::Assign(lhs, _)) = &program[1] else {
            panic!("expected assignment");
        };
        let Expr::ArrayLiteral(items) = lhs.as_ref() else {
            panic!("expected ArrayLiteral target");
        };
        assert_eq!(items.len(), 3);
        assert!(matches!(items[1].1, Expr::Literal(Literal::Null)));
    }

    #[test]
    fn moderately_nested_unary_still_parses() {
        let nested = "~".repeat(500);
        parse(&format!("<?php $x = {nested}1;"));
    }

    #[test]
    #[should_panic(expected = "expression nested too deeply")]
    fn pathologically_nested_unary_fails_cleanly_instead_of_overflowing_the_stack() {
        // php-src's Zend/tests/stack_limit/stack_limit_013.phpt is exactly
        // this shape (~7700 levels) - it's designed to test the real
        // engine's own stack-depth guard, and used to take this parser's
        // process down with an uncatchable stack overflow instead of a
        // normal parse error.
        let nested = "~".repeat(8000);
        parse(&format!("<?php $x = {nested}1;"));
    }

    #[test]
    fn braced_namespace_declarations_splice_their_body_in() {
        let program = parse(
            r#"<?php
            namespace Foo\Bar {
                class A {}
                function f() {}
            }
            namespace {
                class B {}
            }
        "#,
        );
        let class_names: Vec<&str> = program
            .iter()
            .filter_map(|s| match s {
                Stmt::ClassDecl(c) => Some(c.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(class_names, vec!["A", "B"]);
        assert!(
            program
                .iter()
                .any(|s| matches!(s, Stmt::FunctionDecl(f) if f.name == "f"))
        );
        assert!(matches!(&program[0], Stmt::Namespace(n) if n == "Foo\\Bar"));
    }

    #[test]
    fn pipe_operator_parses_left_associative_and_at_the_right_precedence() {
        // Precedence checked empirically against a real PHP 8.5 build:
        // tighter than relational, looser than additive/concat.
        let program = parse(r#"<?php $r = 1 |> 'inc' |> 'dbl' < 3;"#);
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[0] else {
            panic!("expected assignment");
        };
        // `<` is outermost: (1 |> 'inc' |> 'dbl') < 3
        let Expr::Binary(BinOp::Lt, lhs, _) = rhs.as_ref() else {
            panic!("expected relational at the top");
        };
        // pipe is left-associative: (1 |> 'inc') |> 'dbl'
        let Expr::Binary(BinOp::Pipe, inner, _) = lhs.as_ref() else {
            panic!("expected Pipe");
        };
        assert!(matches!(inner.as_ref(), Expr::Binary(BinOp::Pipe, _, _)));
    }

    #[test]
    fn dynamic_property_and_method_access_parse() {
        let program = parse(
            r#"<?php
            $x = $obj->{'RealVar' . '4'};
            $obj->{$name}('a');
            Foo::{$name}();
        "#,
        );
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[0] else {
            panic!("expected assignment");
        };
        assert!(matches!(rhs.as_ref(), Expr::DynamicPropertyAccess(_, _)));

        assert!(matches!(
            &program[1],
            Stmt::Expression(Expr::DynamicMethodCall(_, _, _))
        ));

        let Stmt::Expression(Expr::StaticCall(class, member, _)) = &program[2] else {
            panic!("expected StaticCall");
        };
        assert_eq!(class, "Foo");
        assert_eq!(member, "{dynamic}");
    }

    #[test]
    fn dnf_type_hint_parses() {
        let program = parse(
            r#"<?php
            class Test {
                public (A&B)|X $prop;
            }
        "#,
        );
        let Stmt::ClassDecl(c) = &program[0] else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties[0].type_hint.as_deref(), Some("(A&B)|X"));
    }

    #[test]
    fn namespace_relative_name_parses_as_expression_and_as_type() {
        let program = parse(
            r#"<?php
            function f($a = namespace\unknown) {}
            function g(namespace\integer $x) {}
        "#,
        );
        let Stmt::FunctionDecl(f) = &program[0] else {
            panic!("expected FunctionDecl");
        };
        assert!(matches!(
            &f.params[0].default,
            Some(Expr::Name(n)) if n == "namespace\\unknown"
        ));

        let Stmt::FunctionDecl(g) = &program[1] else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(g.params[0].type_hint.as_deref(), Some("namespace\\integer"));
    }

    #[test]
    fn dollar_brace_variable_variable_forms_parse() {
        let program = parse(
            r#"<?php
            ${f()} = g();
            $oa->${f()} = g();
            C::${f()} = g();
        "#,
        );
        assert!(matches!(
            &program[0],
            Stmt::Expression(Expr::Assign(lhs, _)) if matches!(lhs.as_ref(), Expr::DynamicVariable(_))
        ));
        assert!(matches!(
            &program[1],
            Stmt::Expression(Expr::Assign(lhs, _)) if matches!(lhs.as_ref(), Expr::DynamicPropertyAccess(_, _))
        ));
        assert!(matches!(
            &program[2],
            Stmt::Expression(Expr::Assign(lhs, _)) if matches!(lhs.as_ref(), Expr::StaticAccess(c, m) if c == "C" && m == "{dynamic}")
        ));
    }

    #[test]
    fn by_reference_property_hook_parses() {
        let program = parse(
            r#"<?php
            class Foo {
                public $prop {
                    &get => $this->_prop;
                }
            }
        "#,
        );
        let Stmt::ClassDecl(c) = &program[0] else {
            panic!("expected ClassDecl");
        };
        let hooks = c.properties[0].hooks.as_ref().expect("expected hooks");
        assert!(matches!(hooks.get, Some(PropertyHookBody::Arrow(_))));
    }

    #[test]
    fn clone_with_overrides_and_classic_forms_both_parse() {
        let program = parse(
            r#"<?php
            $x = clone($a, ['x' => 1]);
            $y = clone $a->b;
            $z = clone($a)->b;
        "#,
        );
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[0] else {
            panic!("expected assignment");
        };
        assert!(matches!(rhs.as_ref(), Expr::Clone(_, Some(_))));

        // `clone $a->b` clones the *result* of `$a->b`, not `$a` itself -
        // postfix binds inside clone's operand (verified against a real
        // PHP 8.5 build).
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[1] else {
            panic!("expected assignment");
        };
        let Expr::Clone(inner, None) = rhs.as_ref() else {
            panic!("expected Clone with no overrides");
        };
        assert!(matches!(inner.as_ref(), Expr::PropertyAccess(_, _)));

        // Same rule applies even with explicit parens and no comma:
        // `clone($a)->b` clones `$a->b`, not `$a`.
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[2] else {
            panic!("expected assignment");
        };
        let Expr::Clone(inner, None) = rhs.as_ref() else {
            panic!("expected Clone with no overrides");
        };
        assert!(matches!(inner.as_ref(), Expr::PropertyAccess(_, _)));
    }

    #[test]
    fn goto_and_labels_parse() {
        let program = parse(
            r#"<?php
            goto label;
            echo "skipped";
            label:
            echo "here";
        "#,
        );
        assert!(matches!(&program[0], Stmt::Goto(n) if n == "label"));
        assert!(matches!(&program[2], Stmt::Label(n) if n == "label"));
    }

    #[test]
    fn declare_simple_and_block_forms_parse() {
        let program = parse(
            r#"<?php
            declare(strict_types=1);
            declare(ticks=1) {
                echo "in block";
            }
        "#,
        );
        assert!(matches!(&program[0], Stmt::Block(stmts) if stmts.is_empty()));
        assert!(matches!(&program[1], Stmt::Block(stmts) if !stmts.is_empty()));
    }

    #[test]
    fn group_use_with_trailing_comma_parses() {
        parse(
            r#"<?php
            use Foo\{
                const FOO_CONST,
                function foo_func,
                FooClass as BarClass,
            };
        "#,
        );
    }

    #[test]
    fn public_set_and_final_promoted_params_parse() {
        let program = parse(
            r#"<?php
            class Foo {
                public function __construct(
                    public public(set) string $a,
                    final $b,
                ) {}
            }
        "#,
        );
        let Stmt::ClassDecl(c) = &program[0] else {
            panic!("expected ClassDecl");
        };
        let ctor = &c.methods[0].function;
        assert_eq!(ctor.params[0].promoted_visibility, Some(Visibility::Public));
        assert_eq!(ctor.params[1].name, "b");
    }

    #[test]
    fn legacy_var_property_declaration_parses() {
        let program = parse(
            r#"<?php
            class Dnf {
                var A|(B&C) $a;
            }
        "#,
        );
        let Stmt::ClassDecl(c) = &program[0] else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties[0].name, "a");
        assert_eq!(c.properties[0].visibility, Visibility::Public);
        assert_eq!(c.properties[0].type_hint.as_deref(), Some("A|(B&C)"));
    }

    #[test]
    fn global_with_dynamic_target_parses() {
        let program = parse(r#"<?php global ${sf("a")};"#);
        assert!(matches!(&program[0], Stmt::Global(names) if names == &["{dynamic}"]));
    }

    #[test]
    fn array_cast_parses() {
        let program = parse(r#"<?php $tmp = (array)$var;"#);
        let Stmt::Expression(Expr::Assign(_, rhs)) = &program[0] else {
            panic!("expected assignment");
        };
        assert!(matches!(rhs.as_ref(), Expr::Cast(ty, _) if ty == "array"));
    }
}
