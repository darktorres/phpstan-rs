use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Span, Token};

impl Parser {
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
            if promoted_visibility.is_none()
                && (promoted_set_visibility.is_some() || promoted_readonly || promoted_final)
            {
                promoted_visibility = Some(Visibility::Public);
            }

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
            params.push(Param {
                name,
                name_span,
                type_hint,
                default,
                by_ref,
                variadic,
                promoted_visibility,
                promoted_set_visibility,
                promoted_readonly,
                promoted_final,
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
            let open = self.current_span();
            Some(FunctionBody {
                open,
                stmts: std::sync::Arc::new(self.parse_stmt_list_block()?),
            })
        };
        Ok(FunctionDecl {
            namespaced_name: None,
            name,
            name_span,
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
