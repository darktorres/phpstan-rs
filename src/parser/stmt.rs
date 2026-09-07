use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Position, Span, Token};

impl Parser {
    // ---- statements ----

    pub(crate) fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        // Captured before any leading `#[Attr]` group(s) are consumed, so
        // a declaration's own span includes them - matches how `start`
        // already includes a class's own `abstract`/`final` modifiers.
        let start = self.current_pos();
        // Phase 37: only a function/class/interface/trait/enum
        // declaration below ever actually attaches these (each such arm
        // assigns `attrs` onto the node it builds) - every other
        // statement shape just drops them on the floor, the same lenient
        // "consume and ignore" treatment this parser already gives other
        // unmodeled constructs found somewhere invalid PHP wouldn't allow
        // one at all.
        let attrs = self.parse_leading_attributes()?;
        // A non-destructive peek, not `take_doc_comment()` - a statement
        // that's also a declaration (e.g. a function) consuming its own
        // doc comment for its own `@param`/`@return` tags further down is
        // unaffected, since the peek doesn't remove the map entry.
        let leading_doc_comment = self.doc_comments.get(&self.pos).cloned();
        let mut stmt = match self.peek().clone() {
            Token::InlineHtml(s) => {
                self.advance();
                self.stmt(StmtKind::InlineHtml(s), start)
            }
            Token::Semicolon => {
                self.advance();
                self.stmt(StmtKind::Block(Vec::new()), start)
            }
            Token::LBrace => self.parse_block()?,
            Token::Keyword(Keyword::Echo) => {
                self.advance();
                let mut exprs = vec![self.parse_expression()?];
                while self.eat(&Token::Comma) {
                    exprs.push(self.parse_expression()?);
                }
                self.expect_semicolon()?;
                self.stmt(StmtKind::Echo(exprs), start)
            }
            Token::Keyword(Keyword::Return) => {
                self.advance();
                if self.check(&Token::Semicolon) {
                    self.advance();
                    self.stmt(StmtKind::Return(None), start)
                } else {
                    let e = self.parse_expression()?;
                    self.expect_semicolon()?;
                    self.stmt(StmtKind::Return(Some(e)), start)
                }
            }
            Token::Keyword(Keyword::If) => self.parse_if()?,
            Token::Keyword(Keyword::While) => self.parse_while()?,
            Token::Keyword(Keyword::Do) => self.parse_do_while()?,
            Token::Keyword(Keyword::For) => self.parse_for()?,
            Token::Keyword(Keyword::Foreach) => self.parse_foreach()?,
            Token::Keyword(Keyword::Break) => {
                self.advance();
                let level = self.parse_optional_level();
                self.expect_semicolon()?;
                self.stmt(StmtKind::Break(level), start)
            }
            Token::Keyword(Keyword::Continue) => {
                self.advance();
                let level = self.parse_optional_level();
                self.expect_semicolon()?;
                self.stmt(StmtKind::Continue(level), start)
            }
            Token::Keyword(Keyword::Global) => {
                self.advance();
                let mut names = vec![self.parse_global_target()?];
                while self.eat(&Token::Comma) {
                    names.push(self.parse_global_target()?);
                }
                self.expect_semicolon()?;
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
                    let name = self.expect_variable()?;
                    let default = if self.eat(&Token::Equals) {
                        Some(self.parse_expression()?)
                    } else {
                        None
                    };
                    vars.push((name, default));
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect_semicolon()?;
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
                let mut f = self.parse_function_decl()?;
                f.doc_comment = doc;
                f.attrs = attrs;
                self.stmt(StmtKind::FunctionDecl(f), start)
            }
            Token::Keyword(Keyword::Class)
            | Token::Keyword(Keyword::Interface)
            | Token::Keyword(Keyword::Trait) => {
                let mut c = self.parse_class_decl()?;
                c.attrs = attrs;
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
                let mut c = self.parse_class_decl()?;
                c.attrs = attrs;
                self.stmt(StmtKind::ClassDecl(c), start)
            }
            // Like abstract/final/readonly above, `enum` is only reserved
            // as a declaration keyword - `function enum() {}` and a bare
            // `enum();` call are both legal PHP. A real enum declaration
            // is always followed by its name.
            Token::Keyword(Keyword::Enum)
                if matches!(self.peek_at(1), Token::Identifier(_) | Token::Keyword(_)) =>
            {
                let mut e = self.parse_enum_decl()?;
                e.attrs = attrs;
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
                self.expect_semicolon()?;
                self.stmt(StmtKind::Namespace(name), start)
            }
            Token::Keyword(Keyword::Use) => {
                self.advance();
                // `use function foo;` / `use const BAR;` - Phase 48 needs
                // to tell these apart from a plain class-shaped `use Foo;`
                // (only a function/class import is ever existence-
                // checkable against this project's own registry - see
                // `UseImport::kind`'s own doc comment), so the marker is
                // now captured instead of merely skipped past.
                let default_kind = if self.eat(&Token::Keyword(Keyword::Function)) {
                    UseKind::Function
                } else if self.eat(&Token::Keyword(Keyword::Const)) {
                    UseKind::Const
                } else {
                    UseKind::Class
                };
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
                        // Real PHP only allows a per-member `function`/
                        // `const` marker when the outer `use` itself was
                        // the bare (class-shaped) form - `use function
                        // Foo\{...}` can't mix in its own per-member
                        // markers - but a member here still falls back to
                        // whatever `default_kind` the outer marker set
                        // when it has no marker of its own.
                        let kind = if self.eat(&Token::Keyword(Keyword::Function)) {
                            UseKind::Function
                        } else if self.eat(&Token::Keyword(Keyword::Const)) {
                            UseKind::Const
                        } else {
                            default_kind
                        };
                        let member = self.parse_qualified_name();
                        let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                            Some(self.expect_ident()?)
                        } else {
                            None
                        };
                        imports.push(UseImport {
                            path: format!("{group_prefix}\\{member}"),
                            alias,
                            kind,
                        });
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RBrace)?;
                    self.expect_semicolon()?;
                    return Ok(self.stmt(StmtKind::Use(imports), start));
                }
                let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                let mut imports = vec![UseImport {
                    path: name,
                    alias,
                    kind: default_kind,
                }];
                // `use foo\bar\baz, foo\bar\qux;` - a plain
                // comma-separated list, distinct from the `use Foo\{...}`
                // group form above (each member here is a full path of its
                // own, not relative to a shared prefix).
                while self.eat(&Token::Comma) {
                    let path = self.parse_qualified_name();
                    let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                        Some(self.expect_ident()?)
                    } else {
                        None
                    };
                    imports.push(UseImport {
                        path,
                        alias,
                        kind: default_kind,
                    });
                }
                self.expect_semicolon()?;
                self.stmt(StmtKind::Use(imports), start)
            }
            Token::Keyword(Keyword::Const) => {
                self.advance();
                let mut consts = Vec::new();
                loop {
                    let const_start = self.current_pos();
                    let name_start = self.current_pos();
                    let name = self.expect_ident()?;
                    let name_span = Span {
                        start: name_start,
                        end: self.previous_end(),
                    };
                    self.expect(&Token::Equals)?;
                    let value = self.parse_expression()?;
                    consts.push(ClassConst {
                        name,
                        name_span,
                        value,
                        doc_comment: None,
                        // A top-level `const` has no `public`/`private`
                        // modifier of its own to parse - see `ClassConst::
                        // visibility`'s own doc comment for why `Public`
                        // is a safe, never-actually-consulted default here.
                        visibility: Visibility::Public,
                        attrs: attrs.clone(),
                        // A top-level `const` has no PHP 8.3 typed-
                        // constant syntax either - that's class-const-only.
                        type_hint: None,
                        type_hint_span: None,
                        // Nor any `final` syntax - also class-const-only.
                        is_final: false,
                        span: Span {
                            start: const_start,
                            end: self.previous_end(),
                        },
                    });
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect_semicolon()?;
                self.stmt(StmtKind::GlobalConst(consts), start)
            }
            Token::Keyword(Keyword::Try) => self.parse_try()?,
            Token::Keyword(Keyword::Throw) => {
                self.advance();
                let e = self.parse_expression()?;
                self.expect_semicolon()?;
                self.stmt(StmtKind::Throw(e), start)
            }
            Token::Keyword(Keyword::Switch) => self.parse_switch()?,
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
                self.expect(&Token::LParen)?;
                while !self.check(&Token::RParen) {
                    self.parse_expression()?;
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen)?;
                if self.check(&Token::LBrace) {
                    self.parse_block()?
                } else {
                    self.expect_semicolon()?;
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
                let name = self.expect_ident()?;
                self.expect_semicolon()?;
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
                let e = self.parse_expression()?;
                self.expect_semicolon()?;
                self.stmt(StmtKind::Expression(e), start)
            }
        };
        stmt.doc_comment = leading_doc_comment;
        Ok(stmt)
    }

    pub(crate) fn parse_optional_level(&mut self) -> Option<i64> {
        if let Token::Integer(n) = self.peek() {
            let n = *n;
            self.advance();
            Some(n)
        } else {
            None
        }
    }

    /// A `global` statement's target - almost always a plain `$name`, but
    /// PHP also allows `${expr}` there (`global ${sf("a")};`) for a
    /// runtime-computed variable name. Not modeled beyond parsing past it
    /// (the checker just gets a placeholder "always defined" entry, which
    /// is harmless since nothing else can reference this exact name).
    pub(crate) fn parse_global_target(&mut self) -> Result<String, ParseError> {
        if self.eat(&Token::DollarBrace) {
            self.parse_expression()?;
            self.expect(&Token::RBrace)?;
            return Ok("{dynamic}".to_string());
        }
        if self.eat(&Token::Dollar) {
            self.parse_primary()?;
            return Ok("{dynamic}".to_string());
        }
        self.expect_variable()
    }

    pub(crate) fn parse_block(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_eof() {
            stmts.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(self.stmt(StmtKind::Block(stmts), start))
    }

    pub(crate) fn parse_braced_or_single(&mut self) -> Result<Stmt, ParseError> {
        if self.check(&Token::LBrace) {
            self.parse_block()
        } else {
            self.parse_statement()
        }
    }

    /// A loop/if body, accepting either `{ ... }` / a bare statement, or
    /// PHP's alternative colon syntax (`: ... endwhile;` etc, common in
    /// templates that interleave HTML and PHP).
    pub(crate) fn parse_body(&mut self, alt_end: Keyword) -> Result<Box<Stmt>, ParseError> {
        let start = self.current_pos();
        if self.check(&Token::Colon) {
            let stmts = self.parse_alt_block(alt_end)?;
            Ok(Box::new(self.stmt(StmtKind::Block(stmts), start)))
        } else {
            Ok(Box::new(self.parse_braced_or_single()?))
        }
    }

    pub(crate) fn parse_alt_block(&mut self, end_kw: Keyword) -> Result<Vec<Stmt>, ParseError> {
        self.expect(&Token::Colon)?;
        let mut stmts = Vec::new();
        while !self.check(&Token::Keyword(end_kw.clone())) && !self.at_eof() {
            stmts.push(self.parse_statement()?);
        }
        self.expect(&Token::Keyword(end_kw))?;
        self.expect_semicolon()?;
        Ok(stmts)
    }

    pub(crate) fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance(); // if
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;

        if self.check(&Token::Colon) {
            return self.parse_if_alt(cond, start);
        }

        let then_branch = Box::new(self.parse_braced_or_single()?);

        let else_branch = if self.eat(&Token::Keyword(Keyword::ElseIf)) {
            self.pos -= 1; // put back; re-dispatch as a nested `if`
            self.tokens[self.pos] = Token::Keyword(Keyword::If);
            Some(Box::new(self.parse_if()?))
        } else if self.eat(&Token::Keyword(Keyword::Else)) {
            Some(Box::new(self.parse_braced_or_single()?))
        } else {
            None
        };

        Ok(self.stmt(
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            },
            start,
        ))
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
    pub(crate) fn parse_if_alt(
        &mut self,
        first_cond: Expr,
        if_start: Position,
    ) -> Result<Stmt, ParseError> {
        let mut branches: Vec<(Position, Expr, Vec<Stmt>)> = Vec::new();
        let mut cond = first_cond;
        let mut branch_start = if_start;
        self.expect(&Token::Colon)?;
        loop {
            let mut stmts = Vec::new();
            while !matches!(
                self.peek(),
                Token::Keyword(Keyword::ElseIf)
                    | Token::Keyword(Keyword::Else)
                    | Token::Keyword(Keyword::EndIf)
            ) && !self.at_eof()
            {
                stmts.push(self.parse_statement()?);
            }
            branches.push((branch_start, cond.clone(), stmts));
            if self.eat(&Token::Keyword(Keyword::ElseIf)) {
                branch_start = self.previous_end(); // approx: right at `elseif`
                self.expect(&Token::LParen)?;
                cond = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                self.expect(&Token::Colon)?;
                continue;
            }
            break;
        }

        let else_start = self.current_pos();
        let else_stmts = if self.eat(&Token::Keyword(Keyword::Else)) {
            self.expect(&Token::Colon)?;
            let mut stmts = Vec::new();
            while !self.check(&Token::Keyword(Keyword::EndIf)) && !self.at_eof() {
                stmts.push(self.parse_statement()?);
            }
            Some(stmts)
        } else {
            None
        };

        self.expect(&Token::Keyword(Keyword::EndIf))?;
        self.expect_semicolon()?;
        let end = self.previous_end();

        let mut result = else_stmts.map(|stmts| Stmt {
            kind: StmtKind::Block(stmts),
            span: Span {
                start: else_start,
                end,
            },
            doc_comment: None,
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
                        doc_comment: None,
                    }),
                    else_branch: result.map(Box::new),
                },
                span: Span {
                    start: branch_pos,
                    end,
                },
                doc_comment: None,
            });
        }
        Ok(result.unwrap())
    }

    pub(crate) fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance();
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_body(Keyword::EndWhile)?;
        Ok(self.stmt(StmtKind::While { cond, body }, start))
    }

    pub(crate) fn parse_do_while(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance();
        let body = Box::new(self.parse_braced_or_single()?);
        self.expect(&Token::Keyword(Keyword::While))?;
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        self.expect_semicolon()?;
        Ok(self.stmt(StmtKind::DoWhile { body, cond }, start))
    }

    pub(crate) fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance();
        self.expect(&Token::LParen)?;
        let init = self.parse_expr_list_until(&Token::Semicolon)?;
        self.expect(&Token::Semicolon)?;
        let cond = self.parse_expr_list_until(&Token::Semicolon)?;
        self.expect(&Token::Semicolon)?;
        let step = self.parse_expr_list_until(&Token::RParen)?;
        self.expect(&Token::RParen)?;
        let body = self.parse_body(Keyword::EndFor)?;
        Ok(self.stmt(
            StmtKind::For {
                init,
                cond,
                step,
                body,
            },
            start,
        ))
    }

    pub(crate) fn parse_expr_list_until(&mut self, stop: &Token) -> Result<Vec<Expr>, ParseError> {
        let mut exprs = Vec::new();
        if self.check(stop) {
            return Ok(exprs);
        }
        exprs.push(self.parse_expression()?);
        while self.eat(&Token::Comma) {
            exprs.push(self.parse_expression()?);
        }
        Ok(exprs)
    }

    pub(crate) fn parse_foreach(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance();
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::Keyword(Keyword::As))?;
        let by_ref_first = self.eat(&Token::Amp);
        let first = self.parse_expression()?;

        let (key, by_ref, value) = if self.eat(&Token::FatArrow) {
            let by_ref_val = self.eat(&Token::Amp);
            let value = self.parse_expression()?;
            (Some(first), by_ref_val, value)
        } else {
            (None, by_ref_first, first)
        };

        self.expect(&Token::RParen)?;
        let body = self.parse_body(Keyword::EndForeach)?;
        Ok(self.stmt(
            StmtKind::Foreach {
                subject,
                key,
                value,
                by_ref,
                body,
            },
            start,
        ))
    }

    pub(crate) fn parse_try(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance(); // try
        let body = self.parse_stmt_list_block()?;

        let mut catches = Vec::new();
        while self.eat(&Token::Keyword(Keyword::Catch)) {
            let catch_start = self.previous_end();
            self.expect(&Token::LParen)?;
            let (first_type, first_span) = self.parse_qualified_name_spanned();
            let mut types = vec![first_type];
            let mut types_spans = vec![first_span];
            while self.eat(&Token::Pipe) {
                let (name, span) = self.parse_qualified_name_spanned();
                types.push(name);
                types_spans.push(span);
            }
            let mut var_span = None;
            let var = if matches!(self.peek(), Token::Variable(_)) {
                let var_start = self.current_pos();
                let name = self.expect_variable()?;
                var_span = Some(Span {
                    start: var_start,
                    end: self.previous_end(),
                });
                Some(name)
            } else {
                None
            };
            self.expect(&Token::RParen)?;
            let body = self.parse_stmt_list_block()?;
            catches.push(CatchClause {
                types,
                types_spans,
                var,
                var_span,
                body,
                span: Span {
                    start: catch_start,
                    end: self.previous_end(),
                },
            });
        }

        let finally = if self.eat(&Token::Keyword(Keyword::Finally)) {
            Some(self.parse_stmt_list_block()?)
        } else {
            None
        };

        Ok(self.stmt(
            StmtKind::Try {
                body,
                catches,
                finally,
            },
            start,
        ))
    }

    pub(crate) fn parse_stmt_list_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        if let StmtKind::Block(stmts) = self.parse_block()?.kind {
            Ok(stmts)
        } else {
            unreachable!()
        }
    }

    pub(crate) fn parse_switch(&mut self) -> Result<Stmt, ParseError> {
        let start = self.current_pos();
        self.advance(); // switch
        self.expect(&Token::LParen)?;
        let subject = self.parse_expression()?;
        self.expect(&Token::RParen)?;

        let alt = self.check(&Token::Colon);
        if alt {
            self.advance();
        } else {
            self.expect(&Token::LBrace)?;
        }

        let mut cases: Vec<SwitchCase> = Vec::new();
        let closer = Token::Keyword(Keyword::EndSwitch);
        while !self.at_eof() && !self.check(if alt { &closer } else { &Token::RBrace }) {
            if self.eat(&Token::Keyword(Keyword::Case)) {
                let case_start = self.previous_end();
                let test = self.parse_expression()?;
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
                let stmt = self.parse_statement()?;
                // A statement before any `case`/`default` label is malformed
                // PHP; dropping it here just keeps the parser in sync.
                if let Some(case) = cases.last_mut() {
                    case.span.end = self.previous_end();
                    case.body.push(stmt);
                }
            }
        }

        if alt {
            self.expect(&closer)?;
            self.expect_semicolon()?;
        } else {
            self.expect(&Token::RBrace)?;
        }

        Ok(self.stmt(StmtKind::Switch { subject, cases }, start))
    }
}
