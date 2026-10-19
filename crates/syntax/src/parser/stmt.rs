use crate::ast::*;
use crate::name::Name;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser<'_> {
    // ---- statements ----

    pub(crate) fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        stack::guard(|| self.parse_statement_unguarded())
    }

    fn parse_statement_unguarded(&mut self) -> Result<Stmt, ParseError> {
        stack::guard(|| {
            // Captured before any leading `#[Attr]` group(s) are consumed, so
            // a declaration's own span includes them - matches how `start`
            // already includes a class's own `abstract`/`final` modifiers.
            let start = self.current_pos();
            // Only a function/class/interface/trait/enum declaration below
            // ever actually attaches these (each such arm
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
            let (leading_doc_comment, leading_doc_line) =
                match self.ahead.front().and_then(|tok| tok.docs.as_deref()) {
                    Some(group) => (
                        group.folded.as_ref().or(group.last.as_ref()).cloned(),
                        group.first_start.map(|start| self.line_index.line(start)),
                    ),
                    None => (None, None),
                };
            let mut stmt = match self.peek() {
                Token::InlineHtml(_) => {
                    let Token::InlineHtml(s) = self.take_token() else {
                        unreachable!()
                    };
                    self.stmt(StmtKind::InlineHtml(s), start)
                }
                Token::Semicolon => {
                    self.skip();
                    self.stmt(StmtKind::Block(Vec::new()), start)
                }
                Token::LBrace => self.parse_block()?,
                Token::Keyword(Keyword::Echo) => {
                    self.skip();
                    let mut exprs = vec![self.parse_expression()?];
                    while self.eat(&Token::Comma) {
                        exprs.push(self.parse_expression()?);
                    }
                    self.expect_semicolon()?;
                    self.stmt(StmtKind::Echo(exprs), start)
                }
                Token::Keyword(Keyword::Return) => {
                    self.skip();
                    if self.check(&Token::Semicolon) {
                        self.skip();
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
                    self.skip();
                    let level = self.parse_optional_level()?;
                    self.expect_semicolon()?;
                    self.stmt(StmtKind::Break(level), start)
                }
                Token::Keyword(Keyword::Continue) => {
                    self.skip();
                    let level = self.parse_optional_level()?;
                    self.expect_semicolon()?;
                    self.stmt(StmtKind::Continue(level), start)
                }
                Token::Keyword(Keyword::Global) => {
                    self.skip();
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
                Token::Keyword(Keyword::Static)
                    if matches!(self.peek_at(1), Token::Variable(_)) =>
                {
                    self.skip();
                    let mut vars = Vec::new();
                    loop {
                        let name = self.expect_variable_spanned()?;
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
                    self.note_function(&f);
                    self.stmt(StmtKind::FunctionDecl(Box::new(f)), start)
                }
                Token::Keyword(Keyword::Class)
                | Token::Keyword(Keyword::Interface)
                | Token::Keyword(Keyword::Trait) => {
                    let mut c = self.parse_class_decl()?;
                    c.def.attrs = attrs;
                    self.stmt(StmtKind::ClassDecl(Box::new(c)), start)
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
                    c.def.attrs = attrs;
                    self.stmt(StmtKind::ClassDecl(Box::new(c)), start)
                }
                // Like abstract/final/readonly above, `enum` is only reserved
                // as a declaration keyword - `function enum() {}` and a bare
                // `enum();` call are both legal PHP. A real enum declaration
                // is always followed by its name.
                Token::Keyword(Keyword::Enum)
                    if matches!(self.peek_at(1), Token::Identifier(_) | Token::Keyword(_)) =>
                {
                    let mut e = self.parse_enum_decl()?;
                    e.def.attrs = attrs;
                    self.stmt(StmtKind::ClassDecl(Box::new(e)), start)
                }
                // `namespace\foo();` - a relative-name expression statement,
                // not a declaration (a real namespace declaration never
                // starts with a backslash right after the keyword - it's
                // always `namespace Name;`, `namespace Name { ... }`, or bare
                // `namespace;`/`namespace { ... }`). Falls through to the
                // generic expression-statement path below, which already
                // parses `namespace\foo` as a name expression.
                Token::Keyword(Keyword::Namespace)
                    if !matches!(self.peek_at(1), Token::Backslash) =>
                {
                    self.skip();
                    let name = self.parse_qualified_name();
                    self.expect_semicolon()?;
                    self.stmt(StmtKind::Namespace(name.to_string()), start)
                }
                Token::Keyword(Keyword::Use) => {
                    self.skip();
                    // `use function foo;` / `use const BAR;` need to be told
                    // apart from a plain class-shaped `use Foo;` (only a
                    // function/class import is ever existence-checkable
                    // against this project's own registry - see
                    // `UseImport::kind`'s own doc comment), so the marker is
                    // captured instead of merely skipped past.
                    let default_kind = if self.eat(&Token::Keyword(Keyword::Function)) {
                        UseKind::Function
                    } else if self.eat(&Token::Keyword(Keyword::Const)) {
                        UseKind::Const
                    } else {
                        UseKind::Class
                    };
                    let (name, name_span) = self.parse_qualified_name_spanned();
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
                            let (member, member_span) = self.parse_qualified_name_spanned();
                            let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                                Some(self.expect_ident_spanned()?)
                            } else {
                                None
                            };
                            imports.push(UseImport {
                                path: self.use_path(&format!("{group_prefix}\\{member}")),
                                written: Ident {
                                    name: member,
                                    span: member_span,
                                },
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
                        Some(self.expect_ident_spanned()?)
                    } else {
                        None
                    };
                    let mut imports = vec![UseImport {
                        path: self.use_path(&name),
                        written: Ident {
                            name,
                            span: name_span,
                        },
                        alias,
                        kind: default_kind,
                    }];
                    // `use foo\bar\baz, foo\bar\qux;` - a plain
                    // comma-separated list, distinct from the `use Foo\{...}`
                    // group form above (each member here is a full path of its
                    // own, not relative to a shared prefix).
                    while self.eat(&Token::Comma) {
                        let (path, path_span) = self.parse_qualified_name_spanned();
                        let alias = if self.eat(&Token::Keyword(Keyword::As)) {
                            Some(self.expect_ident_spanned()?)
                        } else {
                            None
                        };
                        imports.push(UseImport {
                            path: self.use_path(&path),
                            written: Ident {
                                name: path,
                                span: path_span,
                            },
                            alias,
                            kind: default_kind,
                        });
                    }
                    self.expect_semicolon()?;
                    self.stmt(StmtKind::Use(imports), start)
                }
                Token::Keyword(Keyword::Const) => {
                    self.skip();
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
                            namespaced_name: None,
                            removal_span: None,
                            name: Ident {
                                name,
                                span: name_span,
                            },
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
                    self.skip();
                    let e = self.parse_expression()?;
                    self.expect_semicolon()?;
                    self.stmt(StmtKind::Throw(e), start)
                }
                Token::Keyword(Keyword::Switch) => self.parse_switch()?,
                Token::Identifier(s)
                    if s.eq_ignore_ascii_case("unset")
                        && matches!(self.peek_at(1), Token::LParen) =>
                {
                    self.skip();
                    let (args, _) = self.parse_args()?;
                    self.expect_semicolon()?;
                    let targets = args.into_iter().map(|a| a.value).collect();
                    self.stmt(StmtKind::Unset(targets), start)
                }
                // `declare(strict_types=1);` / `declare(ticks=1) { ... }` -
                // `declare` isn't a reserved word (a real PHP file can have a
                // function or constant named `declare`), so it's only treated
                // specially when immediately followed by `(`.
                Token::Identifier(s)
                    if s.eq_ignore_ascii_case("declare")
                        && matches!(self.peek_at(1), Token::LParen) =>
                {
                    self.skip();
                    self.expect(&Token::LParen)?;
                    let mut directives = Vec::new();
                    while !self.check(&Token::RParen) {
                        let name = self.expect_ident_spanned()?;
                        self.expect(&Token::Equals)?;
                        let value = self.parse_expression()?;
                        directives.push((name, value));
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RParen)?;
                    let body = if self.check(&Token::LBrace) {
                        Some(self.parse_stmt_list_block()?)
                    } else if self.eat(&Token::Colon) {
                        let mut stmts = Vec::new();
                        while !matches!(self.peek(), Token::Identifier(s) if s.eq_ignore_ascii_case("enddeclare"))
                            && !self.at_eof()
                        {
                            stmts.push(self.parse_statement()?);
                        }
                        self.skip();
                        self.expect_semicolon()?;
                        Some(stmts)
                    } else {
                        self.expect_semicolon()?;
                        None
                    };
                    self.stmt(StmtKind::Declare { directives, body }, start)
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
                    self.skip_to_eof();
                    self.stmt(StmtKind::Block(Vec::new()), start)
                }
                Token::Keyword(Keyword::Goto) => {
                    self.skip();
                    let name = self.expect_ident_spanned()?;
                    self.expect_semicolon()?;
                    self.note_goto(&name.name);
                    self.stmt(StmtKind::Goto(name), start)
                }
                // `label:` - a goto target. Unambiguous at statement position:
                // nothing else in PHP's grammar puts a bare `identifier:` here
                // (ternary's `:` always follows a `?`, and array/match arms
                // use `=>`, not `:`).
                Token::Identifier(_) if matches!(self.peek_at(1), Token::Colon) => {
                    let name = self.take_ident().unwrap();
                    self.skip();
                    self.note_label(&name.name);
                    self.stmt(StmtKind::Label(name), start)
                }
                _ => {
                    let e = self.parse_expression()?;
                    self.expect_semicolon()?;
                    self.stmt(StmtKind::Expression(e), start)
                }
            };
            // A declaration's own doc comment is recorded with the declaration; a
            // different one before it is the statement's.
            let declaration_doc = match &stmt.kind {
                StmtKind::FunctionDecl(function) => Some(function.doc_comment.as_deref()),
                StmtKind::ClassDecl(class) => Some(class.def.doc_comment.as_deref()),
                _ => None,
            };
            if declaration_doc != Some(leading_doc_comment.as_deref()) {
                self.note_doc(&leading_doc_comment, stmt.span.start);
            }
            stmt.doc_comment = leading_doc_comment;
            stmt.doc_line = leading_doc_line;
            Ok(stmt)
        })
    }

    /// `break`/`continue`'s optional level - real PHP's own grammar is
    /// `T_BREAK optional_expr ';'`/`T_CONTINUE optional_expr ';'`, not
    /// just a bare integer literal: `break (2);`/`continue (2);` are
    /// both legal PHP (a real, non-hypothetical shape - dompdf's own
    /// `Stylesheet::_parse_css` uses `continue (2);` inside a `switch`
    /// nested in a `while`) and were previously misparsed as a bare
    /// `break`/`continue` immediately followed by an unexpected `(`,
    /// aborting the whole file with a parse error. A `;` immediately
    /// after `break`/`continue` still means "no level" (the common
    /// case, defaults to 1 downstream); anything else now parses as a
    /// real expression and gets constant-folded by `const_fold_level`.
    /// Real PHP itself requires this expression to be a compile-time-
    /// constant integer (a variable operand is a fatal error since 5.4),
    /// so every shape `const_fold_level` doesn't handle (anything beyond
    /// a literal/negation/parenthesization) can't occur in valid PHP
    /// source - falling back to `None` there is the same "unknown,
    /// default to implicit level 1" treatment a bare `break;` already
    /// gets, not a silent wrong answer for real code.
    pub(crate) fn parse_optional_level(&mut self) -> Result<Option<i64>, ParseError> {
        if self.check(&Token::Semicolon) {
            return Ok(None);
        }
        let expr = self.parse_expression()?;
        Ok(const_fold_level(&expr))
    }

    /// A `global` statement's target - almost always a plain `$name`, but
    /// PHP also allows `${expr}` / `$$name` there (`global ${sf("a")};`)
    /// for a runtime-computed variable name.
    pub(crate) fn parse_global_target(&mut self) -> Result<GlobalVar, ParseError> {
        if self.eat(&Token::DollarBrace) {
            let name = self.parse_expression()?;
            self.expect(&Token::RBrace)?;
            return Ok(GlobalVar::Dynamic(name));
        }
        if self.eat(&Token::Dollar) {
            return Ok(GlobalVar::Dynamic(self.parse_primary()?));
        }
        self.expect_variable_spanned().map(GlobalVar::Name)
    }

    /// A `use` statement's imported name, without a leading `\`.
    fn use_path(&self, written: &str) -> Name {
        self.interner.name(written.trim_start_matches('\\'))
    }
}

/// `parse_optional_level`'s own constant-folder - see that function's
/// doc comment for why only these three shapes matter.
fn const_fold_level(e: &Expr) -> Option<i64> {
    stack::guard(|| {
        let e = e.peeled();
        stack::guard(|| match &e.kind {
            ExprKind::Literal(Literal::Int(n)) => Some(*n),
            ExprKind::Unary(UnaryOp::Neg, inner) => const_fold_level(inner).map(|n| -n),
            _ => None,
        })
    })
}
