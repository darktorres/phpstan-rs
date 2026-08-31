use crate::ast::*;
use crate::lexer::Lexer;
use crate::token::{Keyword, Position, Span, StringPart, Token};

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
    // Parallel to `tokens` (same length, same indices) - `spans[i]` is
    // `tokens[i]`'s own source span. Kept separate rather than zipped
    // into one `Vec<(Token, Span)>` so every existing `self.tokens[...]`
    // access throughout this file (there are many) keeps working
    // unchanged.
    spans: Vec<Span>,
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
    pub fn new(tokens: Vec<(Token, Span)>) -> Self {
        let mut filtered = Vec::with_capacity(tokens.len());
        let mut filtered_spans = Vec::with_capacity(tokens.len());
        let mut doc_comments = std::collections::HashMap::new();
        let mut pending: Option<String> = None;
        for (tok, span) in tokens {
            match tok {
                Token::DocComment(s) => pending = Some(s),
                other => {
                    if let Some(doc) = pending.take() {
                        doc_comments.insert(filtered.len(), doc);
                    }
                    filtered.push(other);
                    filtered_spans.push(span);
                }
            }
        }
        Parser {
            tokens: filtered,
            spans: filtered_spans,
            pos: 0,
            doc_comments,
            expr_depth: 0,
        }
    }

    /// The position a `Stmt`/`Expr` about to be parsed starts at - call
    /// before consuming its first token. Falls back to the last known
    /// span rather than indexing directly: `self.pos` can run one past
    /// the last real token (e.g. `__halt_compiler()`'s handling
    /// deliberately jumps there), the same real edge case `peek()`/
    /// `advance()` already guard against.
    fn current_pos(&self) -> Position {
        self.spans
            .get(self.pos)
            .or_else(|| self.spans.last())
            .map(|s| s.start)
            .unwrap_or_default()
    }

    /// The end of the last token actually consumed - call right before
    /// returning a `Stmt`/`Expr` to close out its span. Same defensive
    /// fallback as `current_pos` for the same reason.
    fn previous_end(&self) -> Position {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.spans.get(i))
            .or_else(|| self.spans.last())
            .map(|s| s.end)
            .unwrap_or_default()
    }

    /// Builds a `Stmt` spanning from `start` (the position captured before
    /// this statement's first token was consumed) to whatever token was
    /// last consumed.
    fn stmt(&self, kind: StmtKind, start: Position) -> Stmt {
        Stmt {
            kind,
            span: Span {
                start,
                end: self.previous_end(),
            },
        }
    }

    /// Same as `stmt`, for expressions.
    fn expr(&self, kind: ExprKind, start: Position) -> Expr {
        Expr {
            kind,
            span: Span {
                start,
                end: self.previous_end(),
            },
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
            // See parse_statement's own Namespace arm for why the
            // backslash check matters: `namespace\foo();` at the top
            // level is a relative-name call, not a declaration.
            if matches!(self.peek(), Token::Keyword(Keyword::Namespace))
                && !matches!(self.peek_at(1), Token::Backslash)
            {
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
        let start = self.current_pos();
        self.advance(); // `namespace`
        let name = self.parse_qualified_name();
        if self.check(&Token::LBrace) {
            let mut stmts = vec![self.stmt(StmtKind::Namespace(name), start)];
            stmts.extend(self.parse_stmt_list_block());
            stmts
        } else {
            self.expect_semicolon();
            vec![self.stmt(StmtKind::Namespace(name), start)]
        }
    }

    // ---- token stream helpers ----

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek_at(&self, offset: usize) -> &Token {
        self.tokens.get(self.pos + offset).unwrap_or(&Token::Eof)
    }

    /// Scans forward (without consuming) through any of `abstract`/
    /// `final`/`readonly` to see if a `class` keyword eventually follows -
    /// the only thing that actually confirms a class declaration is
    /// starting, since all three are also legal as ordinary
    /// function/method/constant names.
    fn looks_like_class_decl(&self) -> bool {
        let mut offset = 0;
        while matches!(
            self.peek_at(offset),
            Token::Keyword(Keyword::Abstract | Keyword::Final | Keyword::Readonly)
        ) {
            offset += 1;
        }
        matches!(self.peek_at(offset), Token::Keyword(Keyword::Class))
    }

    /// Called with the current token on `function` - does a real
    /// declaration name follow (skipping an optional by-ref-return `&`),
    /// as opposed to `(` directly (an anonymous function expression, only
    /// valid as a statement when written bare and unassigned)? `&` itself
    /// is never a valid function name, so no further lookahead is needed
    /// once it's skipped.
    fn looks_like_function_decl(&self) -> bool {
        let offset = if matches!(self.peek_at(1), Token::Amp) {
            2
        } else {
            1
        };
        !matches!(self.peek_at(offset), Token::LParen)
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
        // A named argument (`clone(object: $x)`) or a leading spread
        // (`clone(...$args)`, as opposed to the bare `clone(...)`
        // first-class-callable shape already ruled out by the caller)
        // also commits to the call-args form even with no comma anywhere -
        // both checked first since they're fixed-width lookaheads right
        // after the `(`.
        if matches!(self.peek_at(1), Token::Identifier(_) | Token::Keyword(_))
            && matches!(self.peek_at(2), Token::Colon)
        {
            return true;
        }
        if matches!(self.peek_at(1), Token::Ellipsis) {
            return true;
        }
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
        let start = self.current_pos();
        match self.peek().clone() {
            Token::InlineHtml(s) => {
                self.advance();
                self.stmt(StmtKind::InlineHtml(s), start)
            }
            Token::Semicolon => {
                self.advance();
                self.stmt(StmtKind::Block(Vec::new()), start)
            }
            Token::LBrace => self.parse_block(),
            Token::Keyword(Keyword::Echo) => {
                self.advance();
                let mut exprs = vec![self.parse_expression()];
                while self.eat(&Token::Comma) {
                    exprs.push(self.parse_expression());
                }
                self.expect_semicolon();
                self.stmt(StmtKind::Echo(exprs), start)
            }
            Token::Keyword(Keyword::Return) => {
                self.advance();
                if self.check(&Token::Semicolon) {
                    self.advance();
                    self.stmt(StmtKind::Return(None), start)
                } else {
                    let e = self.parse_expression();
                    self.expect_semicolon();
                    self.stmt(StmtKind::Return(Some(e)), start)
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
                self.stmt(StmtKind::Break(level), start)
            }
            Token::Keyword(Keyword::Continue) => {
                self.advance();
                let level = self.parse_optional_level();
                self.expect_semicolon();
                self.stmt(StmtKind::Continue(level), start)
            }
            Token::Keyword(Keyword::Global) => {
                self.advance();
                let mut names = vec![self.parse_global_target()];
                while self.eat(&Token::Comma) {
                    names.push(self.parse_global_target());
                }
                self.expect_semicolon();
                self.stmt(StmtKind::Global(names), start)
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
                self.stmt(StmtKind::StaticVar(vars), start)
            }
            // A named declaration (`function foo(...)`) is only one of
            // two things `function` can start at statement position - the
            // other is a bare anonymous function *expression* used as a
            // statement (`function () { ... };`, with no assignment at
            // all) - a real, if unusual in production code, idiom that's
            // extremely common in PHPStan's own test fixtures (an
            // isolated global-scope body with no named function needed).
            // The two are told apart by what follows `function` (skipping
            // an optional by-ref-return `&`, which both forms allow): a
            // name for a declaration, `(` directly for an anonymous one.
            Token::Keyword(Keyword::Function) if self.looks_like_function_decl() => {
                let doc = self.take_doc_comment();
                let mut f = self.parse_function_decl();
                f.doc_comment = doc;
                self.stmt(StmtKind::FunctionDecl(f), start)
            }
            Token::Keyword(Keyword::Class)
            | Token::Keyword(Keyword::Interface)
            | Token::Keyword(Keyword::Trait) => {
                let c = self.parse_class_decl();
                self.stmt(StmtKind::ClassDecl(c), start)
            }
            // `abstract`/`final`/`readonly` are only reserved as class
            // *modifiers* - each is also legal as an ordinary
            // function/method/constant name elsewhere (`function
            // readonly() {}`, then a plain `readonly();` call at
            // statement position) since they were reserved well after
            // `class`/`interface`/`trait` themselves. A lookahead confirms
            // a `class` keyword genuinely follows before committing to a
            // class declaration.
            Token::Keyword(Keyword::Abstract | Keyword::Final | Keyword::Readonly)
                if self.looks_like_class_decl() =>
            {
                let c = self.parse_class_decl();
                self.stmt(StmtKind::ClassDecl(c), start)
            }
            // Like abstract/final/readonly above, `enum` is only reserved
            // as a declaration keyword - `function enum() {}` and a bare
            // `enum();` call are both legal PHP. A real enum declaration
            // is always followed by its name.
            Token::Keyword(Keyword::Enum)
                if matches!(self.peek_at(1), Token::Identifier(_) | Token::Keyword(_)) =>
            {
                let e = self.parse_enum_decl();
                self.stmt(StmtKind::EnumDecl(e), start)
            }
            // `namespace\foo();` - a relative-name expression statement,
            // not a declaration (a real namespace declaration never
            // starts with a backslash right after the keyword - it's
            // always `namespace Name;`, `namespace Name { ... }`, or bare
            // `namespace;`/`namespace { ... }`). Falls through to the
            // generic expression-statement path below, which already
            // parses `namespace\foo` as a name expression.
            Token::Keyword(Keyword::Namespace) if !matches!(self.peek_at(1), Token::Backslash) => {
                self.advance();
                let name = self.parse_qualified_name();
                self.expect_semicolon();
                self.stmt(StmtKind::Namespace(name), start)
            }
            Token::Keyword(Keyword::Use) => {
                self.advance();
                // `use function foo;` / `use const BAR;` - the marker
                // doesn't change what's tracked (undefined-class
                // resolution only cares about class-shaped imports), just
                // skip past it.
                if matches!(
                    self.peek(),
                    Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Const)
                ) {
                    self.advance();
                }
                let name = self.parse_qualified_name();
                // Group use: `use Foo\{const BAR, function baz, Qux as
                // Quux};`, with an optional trailing comma before the
                // closing brace. Every member expands to its own
                // `UseImport`, with the group path (`name`) prefixed onto
                // it - `use Foo\{Bar};` imports `Foo\Bar` under the short
                // name `Bar`, same as `use Foo\Bar;` would.
                if self.eat(&Token::LBrace) {
                    // `parse_qualified_name` greedily consumes the `\`
                    // right before this `{` too (it doesn't know a group
                    // follows), leaving `name` with a trailing separator
                    // already - trimming it avoids doubling up below.
                    let group_prefix = name.trim_end_matches('\\');
                    let mut imports = Vec::new();
                    while !self.check(&Token::RBrace) {
                        if matches!(
                            self.peek(),
                            Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Const)
                        ) {
                            self.advance();
                        }
                        let member = self.parse_qualified_name();
                        let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                            Some(self.expect_ident())
                        } else {
                            None
                        };
                        imports.push(UseImport {
                            path: format!("{group_prefix}\\{member}"),
                            alias,
                        });
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RBrace);
                    self.expect_semicolon();
                    return self.stmt(StmtKind::Use(imports), start);
                }
                let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                    Some(self.expect_ident())
                } else {
                    None
                };
                let mut imports = vec![UseImport { path: name, alias }];
                // `use foo\bar\baz, foo\bar\qux;` - a plain
                // comma-separated list, distinct from the `use Foo\{...}`
                // group form above (each member here is a full path of its
                // own, not relative to a shared prefix).
                while self.eat(&Token::Comma) {
                    let path = self.parse_qualified_name();
                    let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                        Some(self.expect_ident())
                    } else {
                        None
                    };
                    imports.push(UseImport { path, alias });
                }
                self.expect_semicolon();
                self.stmt(StmtKind::Use(imports), start)
            }
            Token::Keyword(Keyword::Const) => {
                self.advance();
                let mut consts = Vec::new();
                loop {
                    let const_start = self.current_pos();
                    let name = self.expect_ident();
                    self.expect(&Token::Equals);
                    let value = self.parse_expression();
                    consts.push(ClassConst {
                        name,
                        value,
                        span: Span {
                            start: const_start,
                            end: self.previous_end(),
                        },
                    });
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect_semicolon();
                self.stmt(StmtKind::GlobalConst(consts), start)
            }
            Token::Keyword(Keyword::Try) => self.parse_try(),
            Token::Keyword(Keyword::Throw) => {
                self.advance();
                let e = self.parse_expression();
                self.expect_semicolon();
                self.stmt(StmtKind::Throw(e), start)
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
                    self.stmt(StmtKind::Block(Vec::new()), start)
                }
            }
            // `__halt_compiler();` - tells the *real* compiler to stop
            // parsing right here; everything after it (often binary PHAR
            // data) isn't valid PHP syntax at all, so instead of trying to
            // parse whatever comes next, just jump straight to EOF.
            // Case-insensitive and not a reserved word, like every other
            // magic constant/function.
            Token::Identifier(s)
                if s.eq_ignore_ascii_case("__halt_compiler")
                    && matches!(self.peek_at(1), Token::LParen) =>
            {
                self.pos = self.tokens.len();
                self.stmt(StmtKind::Block(Vec::new()), start)
            }
            Token::Keyword(Keyword::Goto) => {
                self.advance();
                let name = self.expect_ident();
                self.expect_semicolon();
                self.stmt(StmtKind::Goto(name), start)
            }
            // `label:` - a goto target. Unambiguous at statement position:
            // nothing else in PHP's grammar puts a bare `identifier:` here
            // (ternary's `:` always follows a `?`, and array/match arms
            // use `=>`, not `:`).
            Token::Identifier(name) if matches!(self.peek_at(1), Token::Colon) => {
                self.advance();
                self.advance();
                self.stmt(StmtKind::Label(name), start)
            }
            _ => {
                let e = self.parse_expression();
                self.expect_semicolon();
                self.stmt(StmtKind::Expression(e), start)
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
        if self.eat(&Token::Dollar) {
            self.parse_primary();
            return "{dynamic}".to_string();
        }
        self.expect_variable()
    }

    fn parse_block(&mut self) -> Stmt {
        let start = self.current_pos();
        self.expect(&Token::LBrace);
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            stmts.push(self.parse_statement());
        }
        self.expect(&Token::RBrace);
        self.stmt(StmtKind::Block(stmts), start)
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
        let start = self.current_pos();
        if self.check(&Token::Colon) {
            let stmts = self.parse_alt_block(alt_end);
            Box::new(self.stmt(StmtKind::Block(stmts), start))
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
        let start = self.current_pos();
        self.advance(); // if
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);

        if self.check(&Token::Colon) {
            return self.parse_if_alt(cond, start);
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

        self.stmt(
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            },
            start,
        )
    }

    // `if (c1): ... elseif (c2): ... else: ... endif;` - only one `endif;`
    // terminates the whole chain, so branches are collected flat and then
    // folded (innermost-out) into the same nested StmtKind::If shape the
    // brace syntax produces. `if_start` is where the very first `if`
    // keyword began (captured by the caller, `parse_if`, before it
    // consumed anything) - each nested synthesized `If`/`Block` below
    // still gets its own more specific start where possible (the
    // branch's own condition/keyword), rather than every level collapsing
    // onto the same outer position.
    fn parse_if_alt(&mut self, first_cond: Expr, if_start: Position) -> Stmt {
        let mut branches: Vec<(Position, Expr, Vec<Stmt>)> = Vec::new();
        let mut cond = first_cond;
        let mut branch_start = if_start;
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
            branches.push((branch_start, cond.clone(), stmts));
            if self.eat(&Token::Keyword(Keyword::ElseIf)) {
                branch_start = self.previous_end(); // approx: right at `elseif`
                self.expect(&Token::LParen);
                cond = self.parse_expression();
                self.expect(&Token::RParen);
                self.expect(&Token::Colon);
                continue;
            }
            break;
        }

        let else_start = self.current_pos();
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
        let end = self.previous_end();

        let mut result = else_stmts.map(|stmts| Stmt {
            kind: StmtKind::Block(stmts),
            span: Span {
                start: else_start,
                end,
            },
        });
        for (branch_pos, c, stmts) in branches.into_iter().rev() {
            result = Some(Stmt {
                kind: StmtKind::If {
                    cond: c,
                    then_branch: Box::new(Stmt {
                        kind: StmtKind::Block(stmts),
                        span: Span {
                            start: branch_pos,
                            end,
                        },
                    }),
                    else_branch: result.map(Box::new),
                },
                span: Span {
                    start: branch_pos,
                    end,
                },
            });
        }
        result.unwrap()
    }

    fn parse_while(&mut self) -> Stmt {
        let start = self.current_pos();
        self.advance();
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);
        let body = self.parse_body(Keyword::EndWhile);
        self.stmt(StmtKind::While { cond, body }, start)
    }

    fn parse_do_while(&mut self) -> Stmt {
        let start = self.current_pos();
        self.advance();
        let body = Box::new(self.parse_braced_or_single());
        self.expect(&Token::Keyword(Keyword::While));
        self.expect(&Token::LParen);
        let cond = self.parse_expression();
        self.expect(&Token::RParen);
        self.expect_semicolon();
        self.stmt(StmtKind::DoWhile { body, cond }, start)
    }

    fn parse_for(&mut self) -> Stmt {
        let start = self.current_pos();
        self.advance();
        self.expect(&Token::LParen);
        let init = self.parse_expr_list_until(&Token::Semicolon);
        self.expect(&Token::Semicolon);
        let cond = self.parse_expr_list_until(&Token::Semicolon);
        self.expect(&Token::Semicolon);
        let step = self.parse_expr_list_until(&Token::RParen);
        self.expect(&Token::RParen);
        let body = self.parse_body(Keyword::EndFor);
        self.stmt(
            StmtKind::For {
                init,
                cond,
                step,
                body,
            },
            start,
        )
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
        let start = self.current_pos();
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
        self.stmt(
            StmtKind::Foreach {
                subject,
                key,
                value,
                by_ref,
                body,
            },
            start,
        )
    }

    fn parse_try(&mut self) -> Stmt {
        let start = self.current_pos();
        self.advance(); // try
        let body = self.parse_stmt_list_block();

        let mut catches = Vec::new();
        while self.eat(&Token::Keyword(Keyword::Catch)) {
            let catch_start = self.previous_end();
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
            catches.push(CatchClause {
                types,
                var,
                body,
                span: Span {
                    start: catch_start,
                    end: self.previous_end(),
                },
            });
        }

        let finally = if self.eat(&Token::Keyword(Keyword::Finally)) {
            Some(self.parse_stmt_list_block())
        } else {
            None
        };

        self.stmt(
            StmtKind::Try {
                body,
                catches,
                finally,
            },
            start,
        )
    }

    fn parse_stmt_list_block(&mut self) -> Vec<Stmt> {
        if let StmtKind::Block(stmts) = self.parse_block().kind {
            stmts
        } else {
            unreachable!()
        }
    }

    fn parse_switch(&mut self) -> Stmt {
        let start = self.current_pos();
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
                let case_start = self.previous_end();
                let test = self.parse_expression();
                if !self.eat(&Token::Colon) {
                    self.eat(&Token::Semicolon);
                }
                cases.push(SwitchCase {
                    test: Some(test),
                    body: Vec::new(),
                    span: Span {
                        start: case_start,
                        end: self.previous_end(),
                    },
                });
            } else if self.eat(&Token::Keyword(Keyword::Default)) {
                let case_start = self.previous_end();
                if !self.eat(&Token::Colon) {
                    self.eat(&Token::Semicolon);
                }
                cases.push(SwitchCase {
                    test: None,
                    body: Vec::new(),
                    span: Span {
                        start: case_start,
                        end: self.previous_end(),
                    },
                });
            } else {
                let stmt = self.parse_statement();
                // A statement before any `case`/`default` label is malformed
                // PHP; dropping it here just keeps the parser in sync.
                if let Some(case) = cases.last_mut() {
                    case.span.end = self.previous_end();
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

        self.stmt(StmtKind::Switch { subject, cases }, start)
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
            let param_start = self.current_pos();
            // A visibility modifier here means constructor property
            // promotion (`public function __construct(private int $id) {}`)
            // - `readonly` can appear alongside it but doesn't itself imply
            // promotion, so it's still just consumed and dropped.
            let mut promoted_visibility = None;
            let mut promoted_readonly = false;
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
                    Token::Keyword(Keyword::Readonly) => {
                        promoted_readonly = true;
                    }
                    Token::Keyword(Keyword::Final) => {}
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
            // A promoted property can carry its own hooks directly in the
            // constructor's parameter list (`public $prop { get { ... }
            // set { ... } }`), same shape as a hooked property declared
            // the ordinary way.
            let hooks = if self.check(&Token::LBrace) {
                Some(Box::new(self.parse_property_hooks()))
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
                promoted_readonly,
                hooks,
                span: Span {
                    start: param_start,
                    end: self.previous_end(),
                },
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen);
        params
    }

    fn parse_function_decl(&mut self) -> FunctionDecl {
        let start = self.current_pos();
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
        } else if let StmtKind::Block(stmts) = self.parse_block().kind {
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
            span: Span {
                start,
                end: self.previous_end(),
            },
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
    fn parse_anonymous_class_new(&mut self, start: Position) -> Expr {
        let decl_start = self.current_pos();
        let mut class_is_readonly = false;
        loop {
            match self.peek() {
                Token::Keyword(Keyword::Readonly) => {
                    class_is_readonly = true;
                    self.advance();
                }
                Token::Keyword(Keyword::Final) | Token::Keyword(Keyword::Abstract) => {
                    self.advance();
                }
                _ => break,
            }
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

        let (properties, methods, consts, uses, trait_aliases, resolved_trait_conflicts) =
            self.parse_class_body(class_is_readonly);

        let decl = ClassDecl {
            // No name multiple anonymous classes in the same file could
            // share safely - never indexed into the registry by this name,
            // only used structurally within this one expression.
            name: "class@anonymous".to_string(),
            kind: ClassKind::Class,
            extends,
            implements,
            is_abstract: false,
            is_final: false,
            properties,
            methods,
            consts,
            uses,
            trait_aliases,
            resolved_trait_conflicts,
            doc_comment: None,
            span: Span {
                start: decl_start,
                end: self.previous_end(),
            },
        };

        self.expr(ExprKind::NewAnonymousClass(Box::new(decl), args), start)
    }

    /// The `{ ... }` body shared by class/interface/trait declarations and
    /// anonymous classes (`new class { ... }`) - extracted so the two
    /// don't duplicate this (fairly large) member-parsing loop.
    #[allow(clippy::type_complexity)]
    fn parse_class_body(
        &mut self,
        class_is_readonly: bool,
    ) -> (
        Vec<PropertyDecl>,
        Vec<MethodDecl>,
        Vec<ClassConst>,
        Vec<String>,
        Vec<TraitAlias>,
        Vec<String>,
    ) {
        self.expect(&Token::LBrace);
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut consts = Vec::new();
        let mut uses = Vec::new();
        let mut trait_aliases = Vec::new();
        let mut resolved_trait_conflicts = Vec::new();

        while !self.check(&Token::RBrace) && !self.at_eof() {
            let member_start = self.current_pos();
            let doc = self.take_doc_comment();

            if self.eat(&Token::Keyword(Keyword::Use)) {
                uses.push(self.parse_qualified_name());
                while self.eat(&Token::Comma) {
                    uses.push(self.parse_qualified_name());
                }
                if self.check(&Token::LBrace) {
                    let (aliases, resolved) = self.parse_trait_adaptations();
                    trait_aliases.extend(aliases);
                    resolved_trait_conflicts.extend(resolved);
                } else {
                    self.expect_semicolon();
                }
                continue;
            }

            let mut visibility = Visibility::Public;
            let mut is_static = false;
            let mut is_abstract_member = false;
            let mut is_final_member = false;
            let mut is_readonly_member = false;
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
                    Token::Keyword(Keyword::Final) => {
                        is_final_member = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Readonly) => {
                        is_readonly_member = true;
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
                    let const_start = self.current_pos();
                    let cname = self.expect_ident();
                    self.expect(&Token::Equals);
                    let value = self.parse_expression();
                    consts.push(ClassConst {
                        name: cname,
                        value,
                        span: Span {
                            start: const_start,
                            end: self.previous_end(),
                        },
                    });
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
                    is_final: is_final_member,
                    function,
                    span: Span {
                        start: member_start,
                        end: self.previous_end(),
                    },
                });
                continue;
            }

            // property, possibly with a type hint
            let type_hint = self.parse_type_hint();
            let is_readonly = class_is_readonly || is_readonly_member;
            loop {
                let prop_start = self.current_pos();
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
                        is_readonly,
                        type_hint: type_hint.clone(),
                        default,
                        doc_comment: doc.clone(),
                        hooks: Some(hooks),
                        span: Span {
                            start: prop_start,
                            end: self.previous_end(),
                        },
                    });
                    break;
                }
                properties.push(PropertyDecl {
                    name,
                    visibility: visibility.clone(),
                    is_static,
                    is_readonly,
                    type_hint: type_hint.clone(),
                    default,
                    doc_comment: doc.clone(),
                    hooks: None,
                    span: Span {
                        start: prop_start,
                        end: self.previous_end(),
                    },
                });
                if !self.eat(&Token::Comma) {
                    self.expect_semicolon();
                    break;
                }
            }
        }
        self.expect(&Token::RBrace);

        (
            properties,
            methods,
            consts,
            uses,
            trait_aliases,
            resolved_trait_conflicts,
        )
    }

    /// The `{ ... }` conflict-resolution block after a `use A, B;` clause
    /// (`use A, B { A::hello insteadof B; B::hello as helloB; }`) - each
    /// adaptation is `(TraitName::)?method` followed by either
    /// `insteadof TraitName, ...` (parsed and discarded - not modeled,
    /// see `ClassDecl.trait_aliases`'s own doc comment for why) or
    /// `as [visibility] [newName]` (only a real `newName` is captured;
    /// `as public;`, changing visibility with no rename, has nothing to
    /// register). Real PHP allows a bare `method as newName;` with no
    /// `TraitName::` prefix when the name is unambiguous among the
    /// traits actually used - `trait_name: None` here defers resolving
    /// that ambiguity to `flatten_traits`, which already has the full,
    /// registry-wide picture this parser doesn't.
    fn parse_trait_adaptations(&mut self) -> (Vec<TraitAlias>, Vec<String>) {
        let mut aliases = Vec::new();
        let mut resolved = Vec::new();
        self.expect(&Token::LBrace);
        while !self.check(&Token::RBrace) && !self.at_eof() {
            let first = self.expect_ident();
            let (trait_name, method) = if self.eat(&Token::DoubleColon) {
                (Some(first), self.expect_ident())
            } else {
                (None, first)
            };
            if matches!(self.peek(), Token::Identifier(s) if s == "insteadof") {
                self.advance();
                self.parse_qualified_name();
                while self.eat(&Token::Comma) {
                    self.parse_qualified_name();
                }
                resolved.push(method);
            } else if self.eat(&Token::Keyword(Keyword::As)) {
                if matches!(
                    self.peek(),
                    Token::Keyword(Keyword::Public | Keyword::Private | Keyword::Protected)
                ) {
                    self.advance();
                }
                if !self.check(&Token::Semicolon) {
                    let alias = self.expect_ident();
                    aliases.push(TraitAlias {
                        trait_name,
                        method: method.clone(),
                        alias,
                    });
                }
                resolved.push(method);
            }
            self.expect_semicolon();
        }
        self.expect(&Token::RBrace);
        (aliases, resolved)
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
                let param_start = self.previous_end();
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
                    promoted_readonly: false,
                    hooks: None,
                    span: Span {
                        start: param_start,
                        end: self.previous_end(),
                    },
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
        // Captured here, at the very first token of the whole declaration
        // (whichever of `abstract`/`final`/`readonly`/`class`/`interface`/
        // `trait` a caller's dispatch matched on) rather than by a caller
        // - both of this function's call sites in parse_statement would
        // otherwise need their own, easy-to-forget take_doc_comment call,
        // and it needs the enclosing class's own `@template` (Phase 10 -
        // class-level generics), which was never captured at all before.
        let start = self.current_pos();
        let doc_comment = self.take_doc_comment();
        let mut is_abstract = false;
        let mut is_final = false;
        let mut class_is_readonly = false;
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
                // PHP 8.2 `readonly class Foo {}` - shorthand for every
                // property being readonly, applied to each property parsed
                // in parse_class_body below.
                Token::Keyword(Keyword::Readonly) => {
                    class_is_readonly = true;
                    self.advance();
                }
                _ => break,
            }
        }
        let kind = match self.advance() {
            Token::Keyword(Keyword::Class) => ClassKind::Class,
            Token::Keyword(Keyword::Interface) => ClassKind::Interface,
            Token::Keyword(Keyword::Trait) => ClassKind::Trait,
            other => panic!(
                "parse error: expected class/interface/trait, found {:?}",
                other
            ),
        };
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

        let (properties, methods, consts, uses, trait_aliases, resolved_trait_conflicts) =
            self.parse_class_body(class_is_readonly);

        ClassDecl {
            name,
            kind,
            extends,
            implements,
            is_abstract,
            is_final,
            properties,
            methods,
            consts,
            uses,
            trait_aliases,
            resolved_trait_conflicts,
            doc_comment,
            span: Span {
                start,
                end: self.previous_end(),
            },
        }
    }

    // `enum Name [: BackingType] [implements Iface, ...] { ...members... }`.
    // Structurally close to parse_class_decl's member loop (modifiers,
    // `use`, `const`, methods are identical), except enums have `case
    // Name [= value];` instead of properties - they can't declare instance
    // properties or extend anything.
    fn parse_enum_decl(&mut self) -> EnumDecl {
        let start = self.current_pos();
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
        let mut trait_aliases = Vec::new();
        let mut resolved_trait_conflicts = Vec::new();

        while !self.check(&Token::RBrace) && !self.at_eof() {
            let member_start = self.current_pos();
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
                    span: Span {
                        start: member_start,
                        end: self.previous_end(),
                    },
                });
                continue;
            }

            if self.eat(&Token::Keyword(Keyword::Use)) {
                uses.push(self.parse_qualified_name());
                while self.eat(&Token::Comma) {
                    uses.push(self.parse_qualified_name());
                }
                if self.check(&Token::LBrace) {
                    let (aliases, resolved) = self.parse_trait_adaptations();
                    trait_aliases.extend(aliases);
                    resolved_trait_conflicts.extend(resolved);
                } else {
                    self.expect_semicolon();
                }
                continue;
            }

            let mut visibility = Visibility::Public;
            let mut is_static = false;
            let mut is_abstract_member = false;
            let mut is_final_member = false;
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
                    Token::Keyword(Keyword::Final) => {
                        is_final_member = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Readonly) => {
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
                    let const_start = self.current_pos();
                    let cname = self.expect_ident();
                    self.expect(&Token::Equals);
                    let value = self.parse_expression();
                    consts.push(ClassConst {
                        name: cname,
                        value,
                        span: Span {
                            start: const_start,
                            end: self.previous_end(),
                        },
                    });
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
                    is_final: is_final_member,
                    function,
                    span: Span {
                        start: member_start,
                        end: self.previous_end(),
                    },
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
            trait_aliases,
            resolved_trait_conflicts,
            span: Span {
                start,
                end: self.previous_end(),
            },
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
        let start = self.current_pos();
        let mut lhs = self.parse_low_precedence_xor();
        while self.eat(&Token::Keyword(Keyword::Or)) {
            let rhs = self.parse_low_precedence_xor();
            lhs = self.expr(
                ExprKind::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_low_precedence_xor(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_low_precedence_and();
        while self.eat(&Token::Keyword(Keyword::Xor)) {
            let rhs = self.parse_low_precedence_and();
            lhs = self.expr(
                ExprKind::Binary(BinOp::LogicalXor, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_low_precedence_and(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_assignment();
        while self.eat(&Token::Keyword(Keyword::And)) {
            let rhs = self.parse_assignment();
            lhs = self.expr(
                ExprKind::Binary(BinOp::And, Box::new(lhs), Box::new(rhs)),
                start,
            );
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
        let start = self.current_pos();
        let lhs = self.parse_ternary();
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
            None => self.expr(ExprKind::Assign(Box::new(lhs), Box::new(rhs)), start),
            Some(binop) => self.expr(
                ExprKind::CompoundAssign(binop, Box::new(lhs), Box::new(rhs)),
                start,
            ),
        }
    }

    fn parse_ternary(&mut self) -> Expr {
        let start = self.current_pos();
        let cond = self.parse_coalesce();
        if self.eat(&Token::Question) {
            if self.eat(&Token::Colon) {
                let else_branch = self.parse_assignment();
                return self.expr(
                    ExprKind::Ternary(Box::new(cond), None, Box::new(else_branch)),
                    start,
                );
            }
            let then_branch = self.parse_assignment();
            self.expect(&Token::Colon);
            let else_branch = self.parse_assignment();
            return self.expr(
                ExprKind::Ternary(
                    Box::new(cond),
                    Some(Box::new(then_branch)),
                    Box::new(else_branch),
                ),
                start,
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
        let start = self.current_pos();
        let lhs = self.parse_logical_or();
        if self.eat(&Token::QuestionQuestion) {
            let rhs = self.parse_coalesce();
            return self.expr(
                ExprKind::Binary(BinOp::Coalesce, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_logical_or(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_logical_and();
        while self.eat(&Token::PipePipe) {
            let rhs = self.parse_logical_and();
            lhs = self.expr(
                ExprKind::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_logical_and(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_bit_or();
        while self.eat(&Token::AmpAmp) {
            let rhs = self.parse_bit_or();
            lhs = self.expr(
                ExprKind::Binary(BinOp::And, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_bit_or(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_bit_xor();
        while self.eat(&Token::Pipe) {
            let rhs = self.parse_bit_xor();
            lhs = self.expr(
                ExprKind::Binary(BinOp::BitOr, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_bit_xor(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_bit_and();
        while self.eat(&Token::Caret) {
            let rhs = self.parse_bit_and();
            lhs = self.expr(
                ExprKind::Binary(BinOp::BitXor, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_bit_and(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_equality();
        while self.eat(&Token::Amp) {
            let rhs = self.parse_equality();
            lhs = self.expr(
                ExprKind::Binary(BinOp::BitAnd, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_equality(&mut self) -> Expr {
        let start = self.current_pos();
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
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        lhs
    }

    fn parse_relational(&mut self) -> Expr {
        let start = self.current_pos();
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
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
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
        let start = self.current_pos();
        let mut lhs = self.parse_shift();
        while self.eat(&Token::PipeGt) {
            let rhs = self.parse_shift();
            lhs = self.expr(
                ExprKind::Binary(BinOp::Pipe, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_shift(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_additive();
        loop {
            let op = match self.peek() {
                Token::LtLt => BinOp::Shl,
                Token::GtGt => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_additive();
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        lhs
    }

    fn parse_additive(&mut self) -> Expr {
        let start = self.current_pos();
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
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        lhs
    }

    fn parse_multiplicative(&mut self) -> Expr {
        let start = self.current_pos();
        let mut lhs = self.parse_not();
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_not();
            lhs = self.expr(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), start);
        }
        lhs
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
    fn parse_not(&mut self) -> Expr {
        let start = self.current_pos();
        if self.eat(&Token::Bang) {
            let operand = self.parse_not();
            return self.expr(ExprKind::Unary(UnaryOp::Not, Box::new(operand)), start);
        }
        self.parse_instanceof()
    }

    fn parse_instanceof(&mut self) -> Expr {
        let start = self.current_pos();
        let lhs = self.parse_unary();
        if self.eat(&Token::Keyword(Keyword::Instanceof)) {
            // The right-hand side is usually a static class name, but PHP
            // also allows an arbitrary expression there (`$x instanceof
            // $className`, `$x instanceof ('std' . $class)`) for a
            // runtime-determined class.
            let target = if matches!(self.peek(), Token::Variable(_) | Token::LParen) {
                InstanceofTarget::Expr(Box::new(self.parse_unary()))
            } else {
                InstanceofTarget::Name(self.parse_qualified_name())
            };
            return self.expr(ExprKind::Instanceof(Box::new(lhs), target), start);
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
        let start = self.current_pos();
        if let Some(cast_ty) = self.try_parse_cast() {
            let operand = self.parse_unary();
            return self.expr(ExprKind::Cast(cast_ty, Box::new(operand)), start);
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
            return self.expr(ExprKind::Unary(op, Box::new(operand)), start);
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

    fn parse_pow(&mut self) -> Expr {
        let start = self.current_pos();
        let lhs = self.parse_postfix_from_primary();
        if self.eat(&Token::StarStar) {
            let rhs = self.parse_unary(); // right-assoc, binds through unary
            return self.expr(
                ExprKind::Binary(BinOp::Pow, Box::new(lhs), Box::new(rhs)),
                start,
            );
        }
        lhs
    }

    fn parse_postfix_from_primary(&mut self) -> Expr {
        let primary = self.parse_primary();
        self.parse_postfix(primary)
    }

    fn parse_postfix(&mut self, mut expr: Expr) -> Expr {
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
                            let e = self.parse_expression();
                            self.expect(&Token::RBrace);
                            Some(e)
                        } else if self.check(&Token::Dollar) {
                            self.advance();
                            Some(self.parse_primary())
                        } else {
                            None
                        };
                    if let Some(name_expr) = dynamic_name {
                        expr = if self.check(&Token::LParen) {
                            let (args, is_fcc) = self.parse_args();
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
                        other => panic!(
                            "parse error: expected property/method name, found {:?}",
                            other
                        ),
                    };
                    if self.check(&Token::LParen) {
                        let (args, is_fcc) = self.parse_args();
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
                            self.parse_expression();
                            self.expect(&Token::RBrace);
                            "{dynamic}".to_string()
                        }
                        Token::Dollar => {
                            self.advance();
                            self.parse_primary();
                            "{dynamic}".to_string()
                        }
                        other => panic!("parse error: expected static member, found {:?}", other),
                    };
                    if self.check(&Token::LParen) {
                        let (args, is_fcc) = self.parse_args();
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
                        Some(Box::new(self.parse_expression()))
                    };
                    self.expect(&Token::RBracket);
                    expr = self.expr(ExprKind::Index(Box::new(expr), index), start);
                }
                Token::LParen => {
                    let (args, is_fcc) = self.parse_args();
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
        expr
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
    fn parse_args(&mut self) -> (Vec<Arg>, bool) {
        self.expect(&Token::LParen);
        if matches!(self.peek(), Token::Ellipsis) && matches!(self.peek_at(1), Token::RParen) {
            self.advance();
            self.advance();
            return (Vec::new(), true);
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
            let value = self.parse_expression();
            args.push(Arg {
                name,
                spread,
                value,
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen);
        (args, false)
    }

    fn parse_array_items(&mut self, close: &Token) -> Vec<ArrayItem> {
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
            let first = self.parse_expression();
            if self.eat(&Token::FatArrow) {
                self.eat(&Token::Amp);
                let value = self.parse_expression();
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
        self.expect(close);
        items
    }

    fn parse_primary(&mut self) -> Expr {
        let start = self.current_pos();
        match self.peek().clone() {
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
                let name_expr = self.parse_expression();
                self.expect(&Token::RBrace);
                self.expr(ExprKind::DynamicVariable(Box::new(name_expr)), start)
            }
            // `$$name` (equivalent to `${$name}`) - recurses to allow
            // `$$$name` chains, though that's vanishingly rare in
            // practice.
            Token::Dollar => {
                self.advance();
                let name_expr = self.parse_primary();
                self.expr(ExprKind::DynamicVariable(Box::new(name_expr)), start)
            }
            Token::LParen => {
                self.advance();
                let inner = self.parse_expression();
                self.expect(&Token::RParen);
                self.expr(ExprKind::Grouping(Box::new(inner)), start)
            }
            Token::LBracket => {
                self.advance();
                let items = self.parse_array_items(&Token::RBracket);
                self.expr(ExprKind::ArrayLiteral(items), start)
            }
            Token::Keyword(Keyword::Array) => {
                self.advance();
                self.expect(&Token::LParen);
                let items = self.parse_array_items(&Token::RParen);
                self.expr(ExprKind::ArrayLiteral(items), start)
            }
            Token::Keyword(Keyword::List) => {
                self.advance();
                self.expect(&Token::LParen);
                let items = self.parse_array_items(&Token::RParen);
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
                        let e = self.parse_expression();
                        self.expect(&Token::RParen);
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
                    self.parse_args().0
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
                    return self.expr(ExprKind::FirstClassCallable(Box::new(name)), start);
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
                    let (mut args, _) = self.parse_args();
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
                    return self.expr(ExprKind::Clone(Box::new(obj), overrides), start);
                }
                let e = self.parse_unary();
                self.expr(ExprKind::Clone(Box::new(e), None), start)
            }
            Token::Keyword(Keyword::Print) => {
                self.advance();
                let e = self.parse_assignment();
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
                let inner = self.parse_assignment();
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
                    let inner = self.parse_assignment();
                    return self.expr(ExprKind::YieldFrom(Box::new(inner)), start);
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
                    return self.expr(ExprKind::Yield(None, None), start);
                }
                let first = self.parse_ternary();
                if self.eat(&Token::FatArrow) {
                    let value = self.parse_ternary();
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
                let e = self.parse_assignment();
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
                self.parse_closure_like(true)
            }
            Token::Keyword(Keyword::Static) => {
                self.advance();
                self.expr(ExprKind::Name("static".to_string()), start)
            }
            Token::Keyword(Keyword::Function) => self.parse_closure_like(false),
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
                self.parse_closure_like(false)
            }
            Token::Keyword(Keyword::Fn) => {
                let name = self.parse_qualified_name();
                self.expr(ExprKind::Name(name), start)
            }
            Token::Keyword(Keyword::Match) => self.parse_match_expr(),
            Token::InterpString(parts) => {
                self.advance();
                self.build_interpolated(parts, start)
            }
            // `` `cmd $interpolated` `` - shell-exec shorthand. Modeled as
            // a call to shell_exec() on the same interpolated-string value
            // a double-quoted string with identical content would build,
            // rather than adding a dedicated AST node for it.
            Token::ShellExec(parts) => {
                self.advance();
                let cmd = self.build_interpolated(parts, start);
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
        let start = self.current_pos();
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

        let body = if let StmtKind::Block(stmts) = self.parse_block().kind {
            stmts
        } else {
            unreachable!()
        };

        self.expr(
            ExprKind::Closure {
                params,
                uses,
                body,
                is_static,
                by_ref,
                return_type,
            },
            start,
        )
    }

    fn parse_arrow_fn(&mut self, is_static: bool) -> Expr {
        let start = self.current_pos();
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
        self.expr(
            ExprKind::ArrowFn {
                params,
                body: Box::new(body),
                is_static,
                return_type,
            },
            start,
        )
    }

    fn parse_match_expr(&mut self) -> Expr {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Match));
        self.expect(&Token::LParen);
        let subject = self.parse_expression();
        self.expect(&Token::RParen);
        self.expect(&Token::LBrace);

        let mut arms = Vec::new();
        let mut default = None;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            if self.eat(&Token::Keyword(Keyword::Default)) {
                // `default` also tolerates a trailing comma before `=>`,
                // same as an ordinary arm's condition list.
                self.eat(&Token::Comma);
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

        self.expr(
            ExprKind::Match {
                subject: Box::new(subject),
                arms,
                default,
            },
            start,
        )
    }

    /// `base` is the whole interpolated string literal's own starting
    /// position - used as a reasonable anchor for the synthesized
    /// `Literal`/`Variable` parts (which have no tokens of their own to
    /// derive a more specific position from). A `ComplexExpr` fragment
    /// gets a more precise treatment: it's re-lexed and re-parsed in
    /// isolation (its own sub-`Parser`, starting fresh at line 1 col 1),
    /// then `bias_expr_spans` walks the resulting tree once, adding
    /// `StringPart::ComplexExpr`'s own second field (the fragment's real
    /// starting position, computed correctly back in the lexer) to every
    /// span in it - so an error inside `"{$obj->prop}"` points at its
    /// real position instead of somewhere relative to the throwaway
    /// re-lex.
    fn build_interpolated(&mut self, parts: Vec<StringPart>, base: Position) -> Expr {
        let mut exprs = Vec::new();
        for part in parts {
            match part {
                StringPart::Literal(s) => {
                    exprs.push(self.expr(ExprKind::Literal(Literal::String(s)), base))
                }
                StringPart::SimpleVar(name) => {
                    exprs.push(self.expr(ExprKind::Variable(name), base))
                }
                StringPart::ComplexExpr(src, frag_base) => {
                    let tokens = Lexer::new_in_php(&src).tokenize_with_spans();
                    let mut sub = Parser::new(tokens);
                    let mut parsed = sub.parse_expression();
                    bias_expr_spans(&mut parsed, frag_base);
                    exprs.push(parsed);
                }
            }
        }
        if exprs.len() == 1
            && let Some(ExprKind::Literal(Literal::String(_))) = exprs.first().map(|e| &e.kind)
        {
            return exprs.into_iter().next().unwrap();
        }
        self.expr(ExprKind::Interpolated(exprs), base)
    }
}

/// A sub-parser re-lexing a `{$...}` fragment in isolation starts
/// counting positions fresh at line 1, col 1 - biases one such
/// fragment-relative `Position` into real, absolute file coordinates
/// using `frag_base` (the fragment's own real starting position) as the
/// anchor. Only the fragment's first line needs its column offset by
/// `frag_base`'s own column - a `Position` past the first line already
/// starts counting from column 1 on its own, real line.
fn bias_position(pos: Position, frag_base: Position) -> Position {
    if pos.line == 1 {
        Position {
            line: frag_base.line,
            col: frag_base.col + pos.col - 1,
        }
    } else {
        Position {
            line: frag_base.line + pos.line - 1,
            col: pos.col,
        }
    }
}

fn bias_span(span: Span, frag_base: Position) -> Span {
    Span {
        start: bias_position(span.start, frag_base),
        end: bias_position(span.end, frag_base),
    }
}

/// Recursively rewrites every `Span` in `expr`'s own tree from a
/// `{$...}` fragment's re-lexed-in-isolation coordinates into real,
/// absolute file coordinates (see `bias_position`). Mirrors `deps.rs`'s
/// own `collect_expr` walk (same variants, same recursion shape, no
/// wildcard arm - both need to visit every real sub-expression a parsed
/// `Expr` tree can contain) - rewriting a span here instead of
/// collecting a name. A `Closure`'s own multi-statement body isn't
/// walked (a closure literal defined inline inside `{$...}`
/// interpolation is vanishingly rare, and its body would need a
/// matching `Stmt`-level walker to bias fully) - a narrow, honest gap,
/// not silently wrong: the closure expression's own span is still
/// correctly biased, just not its inner statements'.
fn bias_expr_spans(expr: &mut Expr, frag_base: Position) {
    expr.span = bias_span(expr.span, frag_base);
    match &mut expr.kind {
        ExprKind::Literal(_) | ExprKind::Variable(_) | ExprKind::Name(_) => {}
        ExprKind::StaticAccess(_, _) => {}
        ExprKind::Closure { .. } => {}
        ExprKind::DynamicVariable(e)
        | ExprKind::Unary(_, e)
        | ExprKind::Grouping(e)
        | ExprKind::Cast(_, e)
        | ExprKind::Throw(e)
        | ExprKind::YieldFrom(e)
        | ExprKind::FirstClassCallable(e)
        | ExprKind::PropertyAccess(e, _, _) => bias_expr_spans(e, frag_base),
        ExprKind::ArrowFn { body, .. } => bias_expr_spans(body, frag_base),
        ExprKind::ArrayLiteral(items) => {
            for item in items {
                if let Some(k) = &mut item.key {
                    bias_expr_spans(k, frag_base);
                }
                bias_expr_spans(&mut item.value, frag_base);
            }
        }
        ExprKind::Binary(_, l, r) | ExprKind::Assign(l, r) | ExprKind::CompoundAssign(_, l, r) => {
            bias_expr_spans(l, frag_base);
            bias_expr_spans(r, frag_base);
        }
        ExprKind::Ternary(c, t, e) => {
            bias_expr_spans(c, frag_base);
            if let Some(t) = t {
                bias_expr_spans(t, frag_base);
            }
            bias_expr_spans(e, frag_base);
        }
        ExprKind::Call(callee, args) => {
            bias_expr_spans(callee, frag_base);
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::New(target, args) => {
            bias_expr_spans(target, frag_base);
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::MethodCall(recv, _, args, _) => {
            bias_expr_spans(recv, frag_base);
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::DynamicPropertyAccess(base, name_expr) => {
            bias_expr_spans(base, frag_base);
            bias_expr_spans(name_expr, frag_base);
        }
        ExprKind::DynamicMethodCall(recv, name_expr, args) => {
            bias_expr_spans(recv, frag_base);
            bias_expr_spans(name_expr, frag_base);
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::StaticCall(_, _, args) => {
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::Index(base, idx) => {
            bias_expr_spans(base, frag_base);
            if let Some(i) = idx {
                bias_expr_spans(i, frag_base);
            }
        }
        ExprKind::Instanceof(inner, target) => {
            bias_expr_spans(inner, frag_base);
            if let InstanceofTarget::Expr(e) = target {
                bias_expr_spans(e, frag_base);
            }
        }
        ExprKind::Clone(inner, overrides) => {
            bias_expr_spans(inner, frag_base);
            if let Some(o) = overrides {
                bias_expr_spans(o, frag_base);
            }
        }
        ExprKind::NewAnonymousClass(_, args) => {
            for a in args {
                bias_expr_spans(&mut a.value, frag_base);
            }
        }
        ExprKind::Yield(k, v) => {
            if let Some(k) = k {
                bias_expr_spans(k, frag_base);
            }
            if let Some(v) = v {
                bias_expr_spans(v, frag_base);
            }
        }
        ExprKind::Interpolated(parts) => {
            for p in parts {
                bias_expr_spans(p, frag_base);
            }
        }
        ExprKind::Match {
            subject,
            arms,
            default,
        } => {
            bias_expr_spans(subject, frag_base);
            for (conds, result) in arms {
                for c in conds {
                    bias_expr_spans(c, frag_base);
                }
                bias_expr_spans(result, frag_base);
            }
            if let Some(d) = default {
                bias_expr_spans(d, frag_base);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Vec<Stmt> {
        Parser::new(Lexer::new(src).tokenize_with_spans()).parse_program()
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
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
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
        let StmtKind::ClassDecl(c) = &program[0].kind else {
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
        let StmtKind::ClassDecl(c) = &program[0].kind else {
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
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
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
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert!(f.doc_comment.is_none());
    }

    #[test]
    fn top_level_const_declaration_parses() {
        let program = parse("<?php const FOO = 1, BAR = 2;");
        let StmtKind::GlobalConst(consts) = &program[0].kind else {
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
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
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
        let StmtKind::EnumDecl(e) = &program[0].kind else {
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
        let StmtKind::EnumDecl(e) = &program[0].kind else {
            panic!("expected EnumDecl");
        };
        assert_eq!(e.backing_type.as_deref(), Some("string"));
        assert_eq!(e.implements, vec!["\\JsonSerializable".to_string()]);
        assert!(matches!(
            &e.cases[0].value,
            Some(Expr {
                kind: ExprKind::Literal(_),
                ..
            })
        ));
        assert_eq!(e.methods.len(), 1);
        assert_eq!(e.methods[0].function.name, "label");
    }

    #[test]
    fn word_operators_bind_looser_than_assignment() {
        // The classic PHP gotcha: `$a = $b or $c;` is `($a = $b) or $c;`,
        // not `$a = ($b or $c);` - unlike &&/||.
        let program = parse("<?php $x = 5 or die();");
        let StmtKind::Expression(Expr {
            kind: ExprKind::Binary(BinOp::Or, lhs, _),
            ..
        }) = &program[0].kind
        else {
            panic!("expected a top-level Or binary expression");
        };
        assert!(matches!(&lhs.kind, ExprKind::Assign(..)));
    }

    #[test]
    fn readonly_class_parses() {
        let program = parse("<?php readonly class Point { public int $x; }");
        assert!(matches!(&program[0].kind, StmtKind::ClassDecl(_)));
    }

    #[test]
    fn class_interface_and_trait_declarations_are_tagged_with_their_own_kind() {
        let program = parse("<?php class C {} interface I {} trait T {}");
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.kind, ClassKind::Class);
        let StmtKind::ClassDecl(i) = &program[1].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(i.kind, ClassKind::Interface);
        let StmtKind::ClassDecl(t) = &program[2].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(t.kind, ClassKind::Trait);
    }

    #[test]
    fn a_final_method_is_recorded_as_final_and_a_non_final_one_is_not() {
        let program = parse(
            "<?php class C { final public function locked(): void {} public function open(): void {} }",
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let locked = c
            .methods
            .iter()
            .find(|m| m.function.name == "locked")
            .unwrap();
        assert!(locked.is_final);
        let open = c
            .methods
            .iter()
            .find(|m| m.function.name == "open")
            .unwrap();
        assert!(!open.is_final);
    }

    #[test]
    fn a_readonly_property_modifier_is_recorded_per_property() {
        let program = parse("<?php class C { public readonly int $x; public int $y; }");
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let x = c.properties.iter().find(|p| p.name == "x").unwrap();
        assert!(x.is_readonly);
        let y = c.properties.iter().find(|p| p.name == "y").unwrap();
        assert!(!y.is_readonly);
    }

    #[test]
    fn readonly_class_shorthand_marks_every_property_readonly() {
        let program = parse("<?php readonly class C { public int $x; public int $y; }");
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert!(c.properties.iter().all(|p| p.is_readonly));
    }

    #[test]
    fn a_promoted_constructor_parameter_captures_its_own_readonly_modifier() {
        let program =
            parse("<?php class C { public function __construct(public readonly int $id) {} }");
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let ctor = c
            .methods
            .iter()
            .find(|m| m.function.name == "__construct")
            .unwrap();
        assert!(ctor.function.params[0].promoted_readonly);
    }

    #[test]
    fn a_trait_as_alias_clause_is_captured_in_the_ast() {
        let program = parse(
            r#"<?php
            class C {
                use A, B {
                    A::hello insteadof B;
                    B::hello as helloB;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.trait_aliases.len(), 1);
        assert_eq!(c.trait_aliases[0].trait_name.as_deref(), Some("B"));
        assert_eq!(c.trait_aliases[0].method, "hello");
        assert_eq!(c.trait_aliases[0].alias, "helloB");
    }

    #[test]
    fn a_bare_as_alias_clause_with_no_trait_qualifier_is_captured() {
        let program = parse(
            r#"<?php
            class C {
                use X {
                    greet as greetAlias;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.trait_aliases.len(), 1);
        assert_eq!(c.trait_aliases[0].trait_name, None);
        assert_eq!(c.trait_aliases[0].method, "greet");
        assert_eq!(c.trait_aliases[0].alias, "greetAlias");
    }

    #[test]
    fn an_as_clause_that_only_changes_visibility_captures_no_alias() {
        let program = parse(
            r#"<?php
            class C {
                use X {
                    greet as protected;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert!(c.trait_aliases.is_empty());
        assert_eq!(c.resolved_trait_conflicts, vec!["greet".to_string()]);
    }

    #[test]
    fn an_insteadof_clause_records_its_method_as_a_resolved_conflict() {
        let program = parse(
            r#"<?php
            class C {
                use A, B {
                    A::hello insteadof B;
                }
            }
            "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.resolved_trait_conflicts, vec!["hello".to_string()]);
    }

    #[test]
    fn a_bare_anonymous_function_statement_parses_as_an_expression_not_a_declaration() {
        // `function () { ... };` with no assignment at all - a real, if
        // unusual in production code, idiom that's extremely common in
        // real PHPStan's own test fixtures (an isolated global-scope body
        // with no named function needed). Found via bulk-running this
        // project's checker against the vendored phpstan-src corpus.
        let program = parse("<?php function () { echo 1; };");
        assert_eq!(program.len(), 1);
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!(
                "expected an Expression statement, got {:?}",
                program[0].kind
            );
        };
        assert!(matches!(e.kind, ExprKind::Closure { .. }));
    }

    #[test]
    fn a_bare_anonymous_by_ref_function_statement_still_parses_as_an_expression() {
        let program = parse("<?php function &() { static $x = 1; return $x; };");
        assert_eq!(program.len(), 1);
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!(
                "expected an Expression statement, got {:?}",
                program[0].kind
            );
        };
        let ExprKind::Closure { by_ref, .. } = &e.kind else {
            panic!("expected a Closure, got {:?}", e.kind);
        };
        assert!(by_ref);
    }

    #[test]
    fn a_named_function_declaration_is_unaffected_by_the_anonymous_statement_lookahead() {
        let program = parse("<?php function foo(int $x): int { return $x; }");
        assert_eq!(program.len(), 1);
        assert!(matches!(program[0].kind, StmtKind::FunctionDecl(_)));
    }

    #[test]
    fn a_named_by_ref_function_declaration_is_unaffected_by_the_anonymous_statement_lookahead() {
        let program = parse("<?php function &bar(): int { static $x = 1; return $x; }");
        assert_eq!(program.len(), 1);
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected a FunctionDecl, got {:?}", program[0].kind);
        };
        assert_eq!(f.name, "bar");
    }

    #[test]
    fn a_spread_argument_sets_spread_on_its_own_arg() {
        let program = parse("<?php foo(...$args);");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::Call(_, args) = &e.kind else {
            panic!("expected Call, got {:?}", e.kind);
        };
        assert_eq!(args.len(), 1);
        assert!(args[0].spread);
    }

    #[test]
    fn a_plain_call_with_no_spread_leaves_spread_false() {
        let program = parse("<?php foo($a, $b);");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::Call(_, args) = &e.kind else {
            panic!("expected Call, got {:?}", e.kind);
        };
        assert_eq!(args.len(), 2);
        assert!(args.iter().all(|a| !a.spread));
    }

    #[test]
    fn a_spread_argument_sets_spread_on_a_new_call() {
        let program = parse("<?php new Foo(...$args);");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::New(_, args) = &e.kind else {
            panic!("expected New, got {:?}", e.kind);
        };
        assert!(args[0].spread);
    }

    #[test]
    fn exponent_assign_parses_as_pow_not_mul() {
        let program = parse("<?php $x **= 3;");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::CompoundAssign(op, ..) = &e.kind else {
            panic!("expected CompoundAssign, got {:?}", e.kind);
        };
        assert_eq!(*op, BinOp::Pow);
    }

    #[test]
    fn a_spread_element_inside_an_array_literal_sets_its_own_spread_flag() {
        let program = parse("<?php $x = [1, ...$other, 3];");
        let StmtKind::Expression(e) = &program[0].kind else {
            panic!("expected Expression");
        };
        let ExprKind::Assign(_, rhs) = &e.kind else {
            panic!("expected Assign, got {:?}", e.kind);
        };
        let ExprKind::ArrayLiteral(items) = &rhs.kind else {
            panic!("expected ArrayLiteral, got {:?}", rhs.kind);
        };
        assert_eq!(items.len(), 3);
        assert!(!items[0].spread);
        assert!(items[1].spread);
        assert!(!items[2].spread);
    }

    #[test]
    fn qualified_name_with_keyword_segment_parses() {
        // `Enum` here is a namespace segment, not the `enum` declaration
        // keyword - PHP's keywords are mostly "semi-reserved". The main
        // assertion is just that this doesn't panic (it used to). Known,
        // low-impact precision loss: Keyword::as_str() only has the
        // canonical lowercase spelling, not this source's original
        // capitalization, so the reconstructed segment comes back as
        // "enum" - inert for a `use` target specifically, since a bare
        // class-name reference to it (the only thing checker.rs's use-map
        // resolution consumes) is looked up by its final segment's short
        // name, which PHP identifiers treat case-insensitively for this
        // purpose anyway.
        let program = parse("<?php use Piwigo\\Common\\Enum\\AlbumSortOrder;");
        let StmtKind::Use(imports) = &program[0].kind else {
            panic!("expected Use");
        };
        assert_eq!(imports[0].path, "Piwigo\\Common\\enum\\AlbumSortOrder");
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
        assert!(matches!(&program[0].kind, StmtKind::Expression(_)));
    }

    #[test]
    fn first_class_callable_parses_distinctly_from_a_real_call() {
        let program = parse("<?php $f = strlen(...);");
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected an assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::FirstClassCallable(_)));
    }

    #[test]
    fn include_and_require_parse_without_parens() {
        let program = parse("<?php include $file; require_once $other;");
        assert_eq!(program.len(), 2);
    }

    #[test]
    fn static_local_variable_declaration_parses() {
        let program = parse("<?php function f() { static $count = 0; return $count; } ");
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert!(matches!(&f.body[0].kind, StmtKind::StaticVar(_)));
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
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(f.body.len(), 5);
        for stmt in &f.body {
            assert!(matches!(
                &stmt.kind,
                StmtKind::Expression(Expr {
                    kind: ExprKind::CompoundAssign(..),
                    ..
                })
            ));
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
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected an assignment");
        };
        let ExprKind::NewAnonymousClass(decl, args) = &rhs.kind else {
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
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert_eq!(f.body.len(), 4);

        let StmtKind::Expression(Expr {
            kind: ExprKind::Yield(None, Some(_)),
            ..
        }) = &f.body[0].kind
        else {
            panic!("expected bare-value yield");
        };
        let StmtKind::Expression(Expr {
            kind: ExprKind::Yield(Some(_), Some(_)),
            ..
        }) = &f.body[1].kind
        else {
            panic!("expected key => value yield");
        };
        assert!(matches!(
            &f.body[2].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::YieldFrom(_),
                ..
            })
        ));
        assert!(matches!(
            &f.body[3].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Yield(None, None),
                ..
            })
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
        let StmtKind::ClassDecl(c) = &program[0].kind else {
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
        let StmtKind::ClassDecl(c) = &program[0].kind else {
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
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Literal(Literal::Int(5))));
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Literal(Literal::Int(509))));
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[2].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Literal(Literal::Int(31))));
    }

    #[test]
    fn instanceof_with_dynamic_class_name_parses() {
        let program = parse(
            r#"<?php
            assert($repository instanceof $repositoryClass);
            $ok = $x instanceof Foo;
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Call(_, args),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assert() call");
        };
        let ExprKind::Instanceof(_, target) = &args[0].value.kind else {
            panic!("expected Instanceof");
        };
        assert!(matches!(target, InstanceofTarget::Expr(_)));

        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Instanceof(_, target) = &rhs.kind else {
            panic!("expected Instanceof");
        };
        assert!(matches!(target, InstanceofTarget::Name(n) if n == "Foo"));
    }

    #[test]
    fn not_binds_looser_than_instanceof() {
        // `! $x instanceof Y` means `!($x instanceof Y)` in real PHP -
        // `instanceof` binds tighter than `!`. A real guard clause
        // (`if (! $x instanceof Y) { ... }`) depended on this for
        // narrowing to work at all; getting it backwards silently
        // produced `(!$x) instanceof Y` instead, which never matches the
        // `Unary(Not, Instanceof(..))` shape narrowing looks for.
        let program = parse("<?php $ok = ! $x instanceof Foo;");
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Unary(UnaryOp::Not, inner) = &rhs.kind else {
            panic!("expected the Not to be the outermost node, got: {rhs:?}");
        };
        assert!(matches!(&inner.kind, ExprKind::Instanceof(_, _)));
    }

    #[test]
    fn reference_assignment_parses() {
        let program = parse(
            r#"<?php
            $a = &$b;
            $c =& $d;
        "#,
        );
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Assign(_, _),
                ..
            })
        ));
        assert!(matches!(
            &program[1].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Assign(_, _),
                ..
            })
        ));
    }

    #[test]
    fn destructuring_with_skipped_slot_parses() {
        let program = parse(
            r#"<?php
            [, $extension] = explode('-', $s);
            [$a, , $c] = $t;
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(lhs, _),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::ArrayLiteral(items) = &lhs.kind else {
            panic!("expected ArrayLiteral target");
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(
            &items[0].value.kind,
            ExprKind::Literal(Literal::Null)
        ));
        assert!(matches!(&items[1].value.kind, ExprKind::Variable(_)));

        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(lhs, _),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::ArrayLiteral(items) = &lhs.kind else {
            panic!("expected ArrayLiteral target");
        };
        assert_eq!(items.len(), 3);
        assert!(matches!(
            &items[1].value.kind,
            ExprKind::Literal(Literal::Null)
        ));
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
            .filter_map(|s| match &s.kind {
                StmtKind::ClassDecl(c) => Some(c.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(class_names, vec!["A", "B"]);
        assert!(
            program
                .iter()
                .any(|s| matches!(&s.kind, StmtKind::FunctionDecl(f) if f.name == "f"))
        );
        assert!(matches!(&program[0].kind, StmtKind::Namespace(n) if n == "Foo\\Bar"));
    }

    #[test]
    fn pipe_operator_parses_left_associative_and_at_the_right_precedence() {
        // Precedence checked empirically against a real PHP 8.5 build:
        // tighter than relational, looser than additive/concat.
        let program = parse(r#"<?php $r = 1 |> 'inc' |> 'dbl' < 3;"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        // `<` is outermost: (1 |> 'inc' |> 'dbl') < 3
        let ExprKind::Binary(BinOp::Lt, lhs, _) = &rhs.kind else {
            panic!("expected relational at the top");
        };
        // pipe is left-associative: (1 |> 'inc') |> 'dbl'
        let ExprKind::Binary(BinOp::Pipe, inner, _) = &lhs.kind else {
            panic!("expected Pipe");
        };
        assert!(matches!(&inner.kind, ExprKind::Binary(BinOp::Pipe, _, _)));
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
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::DynamicPropertyAccess(_, _)));

        assert!(matches!(
            &program[1].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::DynamicMethodCall(_, _, _),
                ..
            })
        ));

        let StmtKind::Expression(Expr {
            kind: ExprKind::StaticCall(class, member, _),
            ..
        }) = &program[2].kind
        else {
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
        let StmtKind::ClassDecl(c) = &program[0].kind else {
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
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        assert!(matches!(
            &f.params[0].default,
            Some(Expr { kind: ExprKind::Name(n), .. }) if n == "namespace\\unknown"
        ));

        let StmtKind::FunctionDecl(g) = &program[1].kind else {
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
            &program[0].kind,
            StmtKind::Expression(Expr { kind: ExprKind::Assign(lhs, _), .. }) if matches!(&lhs.kind, ExprKind::DynamicVariable(_))
        ));
        assert!(matches!(
            &program[1].kind,
            StmtKind::Expression(Expr { kind: ExprKind::Assign(lhs, _), .. }) if matches!(&lhs.kind, ExprKind::DynamicPropertyAccess(_, _))
        ));
        assert!(matches!(
            &program[2].kind,
            StmtKind::Expression(Expr { kind: ExprKind::Assign(lhs, _), .. }) if matches!(&lhs.kind, ExprKind::StaticAccess(c, m) if c == "C" && m == "{dynamic}")
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
        let StmtKind::ClassDecl(c) = &program[0].kind else {
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
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Clone(_, Some(_))));

        // `clone $a->b` clones the *result* of `$a->b`, not `$a` itself -
        // postfix binds inside clone's operand (verified against a real
        // PHP 8.5 build).
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Clone(inner, None) = &rhs.kind else {
            panic!("expected Clone with no overrides");
        };
        assert!(matches!(&inner.kind, ExprKind::PropertyAccess(..)));

        // Same rule applies even with explicit parens and no comma:
        // `clone($a)->b` clones `$a->b`, not `$a`.
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[2].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Clone(inner, None) = &rhs.kind else {
            panic!("expected Clone with no overrides");
        };
        assert!(matches!(&inner.kind, ExprKind::PropertyAccess(..)));
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
        assert!(matches!(&program[0].kind, StmtKind::Goto(n) if n == "label"));
        assert!(matches!(&program[2].kind, StmtKind::Label(n) if n == "label"));
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
        assert!(matches!(&program[0].kind, StmtKind::Block(stmts) if stmts.is_empty()));
        assert!(matches!(&program[1].kind, StmtKind::Block(stmts) if !stmts.is_empty()));
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
        let StmtKind::ClassDecl(c) = &program[0].kind else {
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
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        assert_eq!(c.properties[0].name, "a");
        assert_eq!(c.properties[0].visibility, Visibility::Public);
        assert_eq!(c.properties[0].type_hint.as_deref(), Some("A|(B&C)"));
    }

    #[test]
    fn global_with_dynamic_target_parses() {
        let program = parse(r#"<?php global ${sf("a")};"#);
        assert!(matches!(&program[0].kind, StmtKind::Global(names) if names == &["{dynamic}"]));
    }

    #[test]
    fn use_with_comma_separated_list_parses() {
        let program = parse(r#"<?php use function foo\bar\baz, foo\bar\qux;"#);
        let StmtKind::Use(imports) = &program[0].kind else {
            panic!("expected Use");
        };
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].path, "foo\\bar\\baz");
        assert_eq!(imports[1].path, "foo\\bar\\qux");
    }

    #[test]
    fn use_group_expands_each_member_with_the_shared_prefix() {
        let program = parse(r#"<?php use Foo\Bar\{Baz, Qux as Quux};"#);
        let StmtKind::Use(imports) = &program[0].kind else {
            panic!("expected Use");
        };
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].path, "Foo\\Bar\\Baz");
        assert_eq!(imports[0].alias, None);
        assert_eq!(imports[1].path, "Foo\\Bar\\Qux");
        assert_eq!(imports[1].alias, Some("Quux".to_string()));
    }

    #[test]
    fn hooks_on_promoted_constructor_property_parse() {
        let program = parse(
            r#"<?php
            class Test {
                public function __construct(
                    public $prop {
                        get { echo "get\n"; }
                        set { echo "set($value)\n"; }
                    },
                ) {}
            }
        "#,
        );
        let StmtKind::ClassDecl(c) = &program[0].kind else {
            panic!("expected ClassDecl");
        };
        let ctor = &c.methods[0].function;
        let hooks = ctor.params[0].hooks.as_ref().expect("expected hooks");
        assert!(matches!(hooks.get, Some(PropertyHookBody::Block(_))));
        assert!(hooks.set.is_some());
    }

    #[test]
    fn halt_compiler_stops_parsing_cleanly() {
        let program = parse(
            r#"<?php
            print "yo!\n";
            __HALT_COMPILER();
            none of this should be parsed at all! {{{ ???
        "#,
        );
        assert_eq!(program.len(), 2);
    }

    #[test]
    fn void_and_binary_casts_parse() {
        let program = parse(r#"<?php (void) foo(); $x = (binary) 42;"#);
        assert!(
            matches!(&program[0].kind, StmtKind::Expression(Expr { kind: ExprKind::Cast(ty, _), .. }) if ty == "void")
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[1].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Cast(ty, _) if ty == "string"));
    }

    #[test]
    fn match_default_arm_tolerates_trailing_comma() {
        parse(
            r#"<?php
            $x = match (1) {
                default,
                    => "x",
            };
        "#,
        );
    }

    #[test]
    fn readonly_abstract_final_usable_as_plain_names() {
        let program = parse(
            r#"<?php
            function readonly() {}
            readonly();
            abstract();
            final();
        "#,
        );
        assert!(matches!(&program[0].kind, StmtKind::FunctionDecl(f) if f.name == "readonly"));
        for stmt in &program[1..] {
            assert!(matches!(
                &stmt.kind,
                StmtKind::Expression(Expr {
                    kind: ExprKind::Call(_, _),
                    ..
                })
            ));
        }
    }

    #[test]
    fn readonly_class_still_parses_as_a_class() {
        let program = parse(r#"<?php readonly class Foo {}"#);
        assert!(matches!(&program[0].kind, StmtKind::ClassDecl(c) if c.name == "Foo"));
    }

    #[test]
    fn yield_followed_by_pure_binary_operator_is_bare() {
        let program = parse(r#"<?php function gen() { var_dump(yield * -1); }"#);
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected FunctionDecl");
        };
        let StmtKind::Expression(Expr {
            kind: ExprKind::Call(_, args),
            ..
        }) = &f.body[0].kind
        else {
            panic!("expected var_dump() call");
        };
        // (yield) * (-1), not yield applied to some malformed operand.
        let ExprKind::Binary(BinOp::Mul, lhs, _) = &args[0].value.kind else {
            panic!("expected a multiplication at the top");
        };
        assert!(matches!(&lhs.kind, ExprKind::Yield(None, None)));
    }

    #[test]
    fn clone_call_form_variants_parse() {
        parse(
            r#"<?php
            $a = clone($x, );
            $b = clone($x, $y, $z, $w, );
            $c = clone(object: $x, withProperties: []);
            $d = clone($x, withProperties: []);
            $e = clone(...["object" => $x]);
        "#,
        );
    }

    #[test]
    fn clone_as_first_class_callable_parses() {
        let program = parse(r#"<?php $f = clone(...);"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::FirstClassCallable(_)));
    }

    #[test]
    fn enum_usable_as_plain_function_and_constant_name() {
        let program = parse(
            r#"<?php
            function enum() { return 1; }
            var_dump(enum());
            var_dump(enum);
            enum Suit { case Hearts; }
        "#,
        );
        assert!(matches!(&program[0].kind, StmtKind::FunctionDecl(f) if f.name == "enum"));
        assert!(matches!(&program[3].kind, StmtKind::EnumDecl(e) if e.name == "Suit"));
    }

    #[test]
    fn fn_usable_as_namespace_segment_when_not_a_signature() {
        let program = parse(r#"<?php fn\test();"#);
        assert!(
            matches!(&program[0].kind, StmtKind::Expression(Expr { kind: ExprKind::Call(callee, _), .. })
            if matches!(&callee.kind, ExprKind::Name(n) if n == "fn\\test"))
        );
    }

    #[test]
    fn by_reference_arrow_function_still_parses_as_arrow_fn() {
        let program = parse(r#"<?php $id = fn&(&$x) => $x;"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::ArrowFn { .. }));
    }

    #[test]
    fn trailing_dot_float_literal_parses() {
        let program = parse(r#"<?php $x = [5., 6.];"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::ArrayLiteral(items) = &rhs.kind else {
            panic!("expected array literal");
        };
        assert!(matches!(&items[0].value.kind, ExprKind::Literal(Literal::Float(f)) if *f == 5.0));
    }

    #[test]
    fn dynamic_class_expr_after_new_and_instanceof_parses() {
        let program = parse(
            r#"<?php
            $class = 'class';
            var_dump(new ('std'.$class));
            var_dump(new ('std'.$class)());
            var_dump($obj instanceof ('std'.$class));
        "#,
        );
        let StmtKind::Expression(Expr {
            kind: ExprKind::Call(_, args),
            ..
        }) = &program[1].kind
        else {
            panic!("expected var_dump() call");
        };
        let ExprKind::New(class_expr, _) = &args[0].value.kind else {
            panic!("expected New");
        };
        assert!(matches!(&class_expr.kind, ExprKind::Grouping(_)));

        let StmtKind::Expression(Expr {
            kind: ExprKind::Call(_, args),
            ..
        }) = &program[2].kind
        else {
            panic!("expected var_dump() call");
        };
        let ExprKind::New(_, ctor_args) = &args[0].value.kind else {
            panic!("expected New");
        };
        assert_eq!(ctor_args.len(), 0);
    }

    #[test]
    fn brace_less_double_dollar_variable_variable_parses() {
        let program = parse(r#"<?php global $a, $$b;"#);
        assert!(
            matches!(&program[0].kind, StmtKind::Global(names) if names == &["a", "{dynamic}"])
        );
    }

    #[test]
    fn shell_exec_backtick_string_parses() {
        let program = parse(r#"<?php $x = `echo -n ""`;"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        let ExprKind::Call(callee, _) = &rhs.kind else {
            panic!("expected a call");
        };
        assert!(matches!(&callee.kind, ExprKind::Name(n) if n == "shell_exec"));
    }

    // Finding 11 from the architecture audit - every diagnostic inside a
    // {$...} string-interpolation expression reported the wrong source
    // position (the throwaway sub-parse's own fresh line-1-col-1
    // coordinates, never biased into the real file's coordinates).

    #[test]
    fn complex_interpolation_expression_spans_are_biased_to_real_file_positions() {
        let program = parse("<?php\nfunction f() {\n    return \"value: {$undefinedThing}\";\n}\n");
        let StmtKind::FunctionDecl(f) = &program[0].kind else {
            panic!("expected a function declaration, got {:?}", program[0].kind);
        };
        let StmtKind::Return(Some(ret_expr)) = &f.body[0].kind else {
            panic!("expected a return statement, got {:?}", f.body[0].kind);
        };
        let ExprKind::Interpolated(parts) = &ret_expr.kind else {
            panic!("expected an interpolated string, got {:?}", ret_expr.kind);
        };
        let var_part = parts
            .iter()
            .find(|p| matches!(&p.kind, ExprKind::Variable(_)))
            .expect("expected a Variable part for $undefinedThing");
        // Line 3, where the real interpolation is - not line 1 col 1,
        // the throwaway re-lex's own fresh coordinates.
        assert_eq!(var_part.span.start.line, 3, "got: {:?}", var_part.span);
        assert!(var_part.span.start.col > 1, "got: {:?}", var_part.span);
    }

    #[test]
    fn quote_inside_complex_interpolation_does_not_end_the_string() {
        let program = parse(r#"<?php $x = "before {$data["key"]} after";"#);
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::Assign(_, _),
                ..
            })
        ));
    }

    #[test]
    fn namespace_relative_call_statement_is_not_a_declaration() {
        let program = parse(r#"<?php namespace\foo();"#);
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr { kind: ExprKind::Call(callee, _), .. })
                if matches!(&callee.kind, ExprKind::Name(n) if n == "namespace\\foo")
        ));
    }

    #[test]
    fn double_dollar_variable_variable_in_member_position_parses() {
        let program = parse(
            r#"<?php
            $obj->$$name;
            Foo::$$name;
            $obj->$$name();
        "#,
        );
        assert!(matches!(
            &program[0].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::DynamicPropertyAccess(_, _),
                ..
            })
        ));
        assert!(matches!(
            &program[1].kind,
            StmtKind::Expression(Expr { kind: ExprKind::StaticAccess(c, m), .. }) if c == "Foo" && m == "{dynamic}"
        ));
        assert!(matches!(
            &program[2].kind,
            StmtKind::Expression(Expr {
                kind: ExprKind::DynamicMethodCall(_, _, _),
                ..
            })
        ));
    }

    #[test]
    fn array_cast_parses() {
        let program = parse(r#"<?php $tmp = (array)$var;"#);
        let StmtKind::Expression(Expr {
            kind: ExprKind::Assign(_, rhs),
            ..
        }) = &program[0].kind
        else {
            panic!("expected assignment");
        };
        assert!(matches!(&rhs.kind, ExprKind::Cast(ty, _) if ty == "array"));
    }

    // Coverage measurement (cargo llvm-cov) found parse_switch,
    // parse_do_while, parse_if_alt, parse_alt_block, and
    // parse_optional_level all had zero test executions - real,
    // reachable parser functions for common PHP syntax (switch, do-while,
    // the `if (...): ... endif;` alternative syntax used throughout
    // template-heavy code, and `break N;`/`continue N;`) that happened to
    // never come up in any existing test's PHP source. Manually verified
    // via the full pipeline that all five currently produce correct
    // results; these lock that in at the AST level.

    #[test]
    fn switch_with_brace_syntax_parses_cases_default_and_fallthrough() {
        let program = parse(
            r#"<?php
            switch ($x) {
                case 1:
                case 2:
                    doA();
                    break;
                default:
                    doB();
            }
        "#,
        );
        let StmtKind::Switch { cases, .. } = &program[0].kind else {
            panic!("expected Switch");
        };
        assert_eq!(cases.len(), 3);
        // `case 1:` falls through with no statements of its own - they
        // all attach to the following `case 2:` label, matching real
        // PHP's fallthrough semantics.
        assert!(cases[0].body.is_empty());
        assert_eq!(cases[1].body.len(), 2);
        assert!(cases[2].test.is_none(), "expected the default case");
        assert_eq!(cases[2].body.len(), 1);
    }

    #[test]
    fn switch_alt_syntax_parses_the_same_shape_as_brace_syntax() {
        let program = parse(
            r#"<?php
            switch ($x):
                case 1:
                    doA();
            endswitch;
        "#,
        );
        let StmtKind::Switch { cases, .. } = &program[0].kind else {
            panic!("expected Switch");
        };
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].body.len(), 1);
    }

    #[test]
    fn do_while_parses_its_body_and_trailing_condition() {
        let program = parse(
            r#"<?php
            do {
                doSomething();
            } while ($i < 10);
        "#,
        );
        let StmtKind::DoWhile { body, cond } = &program[0].kind else {
            panic!("expected DoWhile");
        };
        assert!(matches!(&body.kind, StmtKind::Block(stmts) if stmts.len() == 1));
        assert!(matches!(&cond.kind, ExprKind::Binary(BinOp::Lt, ..)));
    }

    #[test]
    fn if_alt_syntax_with_elseif_and_else_folds_into_nested_if() {
        let program = parse(
            r#"<?php
            if ($a):
                doA();
            elseif ($b):
                doB();
            else:
                doC();
            endif;
        "#,
        );
        let StmtKind::If {
            then_branch,
            else_branch,
            ..
        } = &program[0].kind
        else {
            panic!("expected If");
        };
        assert!(matches!(&then_branch.kind, StmtKind::Block(s) if s.len() == 1));
        let Some(else_branch) = else_branch else {
            panic!("expected an elseif chain, got no else branch at all");
        };
        // The `elseif` re-dispatches as a nested `if`, exactly like the
        // brace syntax's own elseif handling - one `endif;` still closes
        // the whole chain, not just the innermost `if`.
        let StmtKind::If {
            then_branch: elseif_then,
            else_branch: elseif_else,
            ..
        } = &else_branch.kind
        else {
            panic!("expected a nested If for the elseif branch");
        };
        assert!(matches!(&elseif_then.kind, StmtKind::Block(s) if s.len() == 1));
        assert!(
            matches!(elseif_else.as_deref(), Some(Stmt { kind: StmtKind::Block(s), .. }) if s.len() == 1)
        );
    }

    #[test]
    fn break_and_continue_parse_their_optional_numeric_level() {
        let program = parse(
            r#"<?php
            break;
            break 2;
            continue;
            continue 3;
        "#,
        );
        assert!(matches!(&program[0].kind, StmtKind::Break(None)));
        assert!(matches!(&program[1].kind, StmtKind::Break(Some(2))));
        assert!(matches!(&program[2].kind, StmtKind::Continue(None)));
        assert!(matches!(&program[3].kind, StmtKind::Continue(Some(3))));
    }

    #[test]
    fn foreach_alt_syntax_parses_its_body_via_parse_alt_block() {
        // parse_if_alt and parse_switch each parse their own alt-syntax
        // body inline - parse_alt_block is the shared path only
        // while/for/foreach's alt syntax (`endwhile;`/`endfor;`/
        // `endforeach;`) goes through, and none of the tests above
        // happen to exercise it.
        let program = parse(
            r#"<?php
            foreach ($items as $item):
                doA();
                doB();
            endforeach;
        "#,
        );
        let StmtKind::Foreach { body, .. } = &program[0].kind else {
            panic!("expected Foreach");
        };
        assert!(matches!(&body.kind, StmtKind::Block(s) if s.len() == 2));
    }
}
