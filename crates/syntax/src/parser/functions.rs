use crate::ast::*;
use crate::parser::{ParseError, Parser, Supply};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser<'_> {
    // ---- functions / params ----

    pub(crate) fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        while !self.check(&Token::RParen) {
            let param_start = self.current_pos();
            // `#[Attr]` groups on a parameter precede everything else
            // about it, including constructor-promotion visibility
            // modifiers (`function f(#[Attr] private int $id) {}`).
            //
            // `parse_leading_attributes` (not the plain `parse_attributes`)
            // - a promoted property's own inline `/** @var ... */` doc
            // comment very commonly sits *before* an `#[ORM\Column(...)]`
            // -style attribute (`/** @var array<string, mixed>|null */
            // #[ORM\Column(...)] public ?array $details,`); this re-keys
            // past the attributes so `take_doc_comment()` right after
            // correctly finds it either way.
            let attrs = self.parse_leading_attributes()?;
            let doc_comment = self.take_doc_comment();
            // A visibility modifier here means constructor property
            // promotion (`public function __construct(private int $id) {}`)
            // - `readonly`/`final` can appear alongside it, and each also
            // promotes on its own.
            let mut promoted_visibility = None;
            let mut promoted_readonly = false;
            let mut promoted_final = false;
            let mut promoted_set_visibility = None;
            loop {
                // PHP 8.4 asymmetric visibility on a promoted property
                // (`public public(set) string $a`) - see the identical
                // case in parse_class_body's property-modifier loop.
                if matches!(
                    self.peek(),
                    Token::Keyword(Keyword::Public | Keyword::Private | Keyword::Protected)
                ) && self.peek_is_set_visibility_marker()
                {
                    promoted_set_visibility = match self.peek() {
                        Token::Keyword(Keyword::Private) => Some(Visibility::Private),
                        Token::Keyword(Keyword::Protected) => Some(Visibility::Protected),
                        _ => Some(Visibility::Public),
                    };
                    self.skip();
                    self.skip();
                    self.expect_ident()?;
                    self.expect(&Token::RParen)?;
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
                    Token::Keyword(Keyword::Final) => {
                        promoted_final = true;
                    }
                    _ => break,
                }
                self.skip();
            }
            // `readonly`, `final`, or a set-visibility modifier alone
            // (`private(set) string $a`) still promotes, with implicit
            // `public` visibility.
            let promotes = promoted_visibility.is_some()
                || promoted_set_visibility.is_some()
                || promoted_readonly
                || promoted_final;
            let promotion = promotes.then(|| Promotion {
                visibility: promoted_visibility.unwrap_or(Visibility::Public),
                set_visibility: promoted_set_visibility,
                readonly: promoted_readonly,
                is_final: promoted_final,
            });

            let type_hint = self.parse_type_hint()?;
            let by_ref = self.eat(&Token::Amp);
            let variadic = self.eat(&Token::Ellipsis);
            let name_start = self.current_pos();
            let name = self.expect_variable()?;
            let name_span = Span {
                start: name_start,
                end: self.previous_end(),
            };
            let default = if self.eat(&Token::Equals) {
                Some(self.parse_expression()?)
            } else {
                None
            };
            // A promoted property can carry its own hooks directly in the
            // constructor's parameter list (`public $prop { get { ... }
            // set { ... } }`), same shape as a hooked property declared
            // the ordinary way.
            let hooks = if self.check(&Token::LBrace) {
                Some(Box::new(self.parse_property_hooks()?))
            } else {
                None
            };
            self.note_doc(&doc_comment, param_start);
            params.push(Param {
                name: Ident {
                    name,
                    span: name_span,
                },
                type_hint,
                default,
                by_ref,
                variadic,
                promotion,
                hooks,
                attrs,
                doc_comment,
                span: Span {
                    start: param_start,
                    end: self.previous_end(),
                },
            });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RParen)?;
        Ok(params)
    }

    /// A function's `{ ... }` body, at its `{`.
    pub(crate) fn parse_function_body(&mut self) -> Result<FunctionBody, ParseError> {
        if let Some(skimmed) = self.skim_body() {
            return Ok(skimmed);
        }
        let open = self.current_span();
        let depth = self.enter_goto_scope();
        self.recorder.borrow_mut().enter_body();
        self.body_depth.set(self.body_depth.get() + 1);
        let stmts = self.parse_stmt_list_block();
        self.body_depth.set(self.body_depth.get() - 1);
        self.recorder.borrow_mut().leave_body();
        let end = self.leave_goto_scope(depth);
        Ok(FunctionBody {
            open,
            stmts: std::sync::Arc::new(stmts?),
            goto_labels: end.goto_labels,
            is_generator: end.is_generator,
            skimmed: false,
        })
    }

    /// The body at the `{` the parser is at, skipped to its `}` without
    /// parsing it, when nothing in it is read by the load that reads the
    /// file: no declaration (a function, a class, an anonymous class or a
    /// `define`), nothing that adds to the global scope (`global`,
    /// `include`, `require`), no doc comment, no `yield` or `goto`, no
    /// name in `PHPStan\` and no inline HTML or `{$...}` in a string. The
    /// tokens are lexed as they are for a parse, so what the lexer notes is
    /// noted; a body that must be parsed is parsed from the tokens read.
    fn skim_body(&mut self) -> Option<FunctionBody> {
        if !self.skim_bodies
            || self.body_depth.get() > 0
            || self.nested_depth.get() > 0
            || !self.check(&Token::LBrace)
            || self.names_in_effect_could_be_phpstan()
            || !matches!(self.supply.get_mut(), Supply::Lexer(_))
        {
            return None;
        }
        let open = self.current_span();
        let (pos, last_end, last_span) = (self.pos, self.last_end, self.last_span);
        let mut tape = std::mem::take(&mut self.skim_tape);
        let mut depth = 0u32;
        let mut state = SkimState::default();
        let skimmed = loop {
            let Some(tok) = self.pop() else { break false };
            let reads = tok.docs.is_some() || state.reads_at_load(&tok.token);
            if !reads {
                match tok.token {
                    Token::LBrace | Token::DollarBrace => depth += 1,
                    Token::RBrace => depth = depth.saturating_sub(1),
                    _ => {}
                }
                if depth == 0 {
                    break true;
                }
            }
            tape.push(tok);
            if reads {
                break false;
            }
        };
        if skimmed {
            tape.clear();
            self.skim_tape = tape;
            self.skimmed_any = true;
            return Some(FunctionBody {
                open,
                stmts: std::sync::Arc::new(Vec::new()),
                goto_labels: Box::default(),
                is_generator: false,
                skimmed: true,
            });
        }
        // The tokens go back where they were: the `{` first, the one that
        // ended the skim last.
        for tok in tape.drain(..).rev() {
            self.ahead.push_front(tok);
        }
        self.pos = pos;
        self.last_end = last_end;
        self.last_span = last_span;
        self.skim_tape = tape;
        None
    }

    /// Whether the names in effect where a body is can be names in
    /// `PHPStan\`, which the load notes where a file mentions one.
    fn names_in_effect_could_be_phpstan(&self) -> bool {
        let ctx = self.ctx.borrow();
        let (version, answer) = self.skim_phpstan_context.get();
        if version == ctx.version() && version != 0 {
            return answer;
        }
        let answer = ctx.could_name_phpstan();
        self.skim_phpstan_context.set((ctx.version(), answer));
        answer
    }

    pub(crate) fn parse_function_decl(&mut self) -> Result<FunctionDecl, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Function))?;
        let by_ref = self.eat(&Token::Amp);
        let name_start = self.current_pos();
        let name = self.expect_ident()?;
        let name_span = Span {
            start: name_start,
            end: self.previous_end(),
        };
        let params = self.parse_params()?;
        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()?
        } else {
            None
        };
        let body = if self.eat(&Token::Semicolon) {
            None
        } else {
            Some(self.parse_function_body()?)
        };
        Ok(FunctionDecl {
            namespaced_name: None,
            name: Ident {
                name,
                span: name_span,
            },
            params,
            return_type,
            body,
            by_ref,
            // Callers that saw a preceding doc comment overwrite this -
            // parse_function_decl itself is entered after any modifiers
            // (and thus after the doc comment's position) have already
            // been consumed.
            doc_comment: None,
            // Overwritten by whichever caller actually saw the leading
            // `#[Attr]` group(s), if any - same "entered after modifiers"
            // reasoning as `doc_comment` just above: a top-level function
            // is reached from `parse_statement`, a method from
            // `parse_class_body`/`parse_enum_decl`'s own member loops, both
            // of which parse attributes before dispatching here.
            attrs: Vec::new(),
            span: Span {
                start,
                end: self.previous_end(),
            },
        })
    }
}

/// What a skim has seen of the tokens before the one it is at.
#[derive(Default)]
struct SkimState {
    /// The last was `::`, `->` or `?->`, which a keyword then is the name of
    /// a member after.
    after_member_operator: bool,
    /// The last was `function`, which a name or a `(` then follows.
    after_function: bool,
    /// The last was a segment of a name spelled `PHPStan`.
    after_phpstan: bool,
}

impl SkimState {
    /// Whether `token` is something the load of a file reads in a function
    /// body, or something that is not skipped as text: the body is parsed.
    /// Notes the token as the last one seen.
    fn reads_at_load(&mut self, token: &Token) -> bool {
        let after_member_operator = std::mem::replace(&mut self.after_member_operator, false);
        let after_phpstan = std::mem::take(&mut self.after_phpstan);
        if std::mem::take(&mut self.after_function) {
            match token {
                // `function &name` and `function &(`.
                Token::Amp => {
                    self.after_function = true;
                    return false;
                }
                Token::LParen => return false,
                // `function name`: a function declared in the body.
                _ => return true,
            }
        }
        match token {
            Token::DoubleColon | Token::Arrow | Token::NullsafeArrow => {
                self.after_member_operator = true;
                false
            }
            Token::Eof | Token::InlineHtml(_) => true,
            Token::InterpString(parts) | Token::ShellExec(parts) => parts
                .iter()
                .any(|part| matches!(part, crate::token::StringPart::ComplexExpr(..))),
            Token::Keyword(_) if after_member_operator => false,
            Token::Keyword(Keyword::Function) => {
                self.after_function = true;
                false
            }
            Token::Keyword(
                Keyword::Class
                | Keyword::Interface
                | Keyword::Trait
                | Keyword::Enum
                | Keyword::Global
                | Keyword::Include
                | Keyword::IncludeOnce
                | Keyword::Require
                | Keyword::RequireOnce
                | Keyword::Yield
                | Keyword::Goto,
            ) => true,
            // A name in `PHPStan\`, which the load notes, has `PHPStan` for a
            // segment followed by a `\`: as written, after `namespace\` or
            // after an import, which the names in effect settle.
            Token::Backslash => after_phpstan,
            Token::Identifier(_) if after_member_operator => false,
            Token::Identifier(name) => {
                let lower = name.lower();
                self.after_phpstan = lower == "phpstan";
                lower == "define"
            }
            _ => false,
        }
    }
}
