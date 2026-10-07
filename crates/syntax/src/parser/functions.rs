use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser<'_> {
    // ---- functions / params ----

    pub(crate) fn parse_params(
        &mut self,
        function_doc: &mut Option<DocCommentRanges>,
        apply_stub_rules: bool,
    ) -> Result<ParamRange, ParseError> {
        self.expect(&Token::LParen)?;
        let params_depth = self.begin_param_list();
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
            self.param_list_scratch[params_depth].push(Param {
                name: self.ident(name, name_span),
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
            if !self.eat_list_comma(
                &Token::RParen,
                crate::lexer::SyntaxMark::TrailingCommaInParameters,
            ) {
                break;
            }
        }
        self.expect(&Token::RParen)?;
        let mut params = self.take_param_list(params_depth);
        if apply_stub_rules {
            self.transform_stub_params(&mut params, function_doc);
        }
        Ok(self.finish_param_list(params_depth, params))
    }

    pub(crate) fn parse_function_decl(
        &mut self,
        attrs: AttributeRange,
        doc_comment: Option<DocCommentRanges>,
        mut statement_doc: Option<&mut Option<DocCommentRanges>>,
        record: bool,
    ) -> Result<FunctionDecl, ParseError> {
        let start = self.current_pos();
        self.expect(&Token::Keyword(Keyword::Function))?;
        let by_ref = self.eat(&Token::Amp);
        let name_start = self.current_pos();
        let name = self.expect_ident()?;
        let name_span = Span {
            start: name_start,
            end: self.previous_end(),
        };
        let mut doc_comment = doc_comment;
        let overrides = if record {
            self.return_overrides(attrs.clone())
        } else {
            Vec::new()
        };
        for ty in &overrides {
            self.append_tag(&mut doc_comment, &format!("@return {ty}"));
            if let Some(doc) = statement_doc.as_deref_mut() {
                self.append_tag(doc, &format!("@return {ty}"));
            }
        }
        let attrs = if record {
            self.strip_stub_attributes(attrs)
        } else {
            attrs
        };
        let params = self.parse_params(&mut doc_comment, record)?;
        let return_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()?
        } else {
            None
        };
        let body = if self.eat(&Token::Semicolon) {
            None
        } else {
            let open = self.current_span();
            let depth = self.enter_goto_scope();
            self.recorder.borrow_mut().enter_body();
            let stmts = self.parse_stmt_list_block_range();
            self.recorder.borrow_mut().leave_body();
            let end = self.leave_goto_scope(depth);
            Some(FunctionBody {
                open,
                stmts: stmts?,
                goto_labels: end.goto_labels,
                is_generator: end.is_generator,
            })
        };
        let function = FunctionDecl {
            namespaced_name: None,
            name: self.ident(name, name_span),
            params,
            return_type,
            body,
            by_ref,
            doc_comment,
            attrs,
            span: Span {
                start,
                end: self.previous_end(),
            },
        };
        if record {
            self.note_function(&function);
        }
        Ok(function)
    }
}
