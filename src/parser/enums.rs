use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Span, Token};

impl Parser {
    // `enum Name [: BackingType] [implements Iface, ...] { ...members... }`.
    // Structurally close to parse_class_decl's member loop (modifiers,
    // `use`, `const`, methods are identical), except enums have `case
    // Name [= value];` instead of properties - they can't declare instance
    // properties or extend anything.
    pub(crate) fn parse_enum_decl(&mut self) -> Result<EnumDecl, ParseError> {
        let start = self.current_pos();
        // Captured here, at the very first token, mirroring
        // `parse_class_decl`'s identical note above - needed for
        // rejecting a stray `@template` on an enum (enums can't be
        // generic; PHP has no `enum Foo<T>` syntax at all).
        let doc_comment = self.take_doc_comment();
        self.expect(&Token::Keyword(Keyword::Enum))?;
        let name_start = self.current_pos();
        let name = self.expect_ident()?;
        let name_span = Span {
            start: name_start,
            end: self.previous_end(),
        };

        let backing_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()?
        } else {
            None
        };

        let mut implements = Vec::new();
        let mut implements_spans = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            let (name, span) = self.parse_qualified_name_spanned();
            implements.push(name);
            implements_spans.push(span);
            while self.eat(&Token::Comma) {
                let (name, span) = self.parse_qualified_name_spanned();
                implements.push(name);
                implements_spans.push(span);
            }
        }

        self.expect(&Token::LBrace)?;
        let mut cases = Vec::new();
        let mut methods = Vec::new();
        let mut consts = Vec::new();
        let mut uses = Vec::new();
        let mut uses_spans = Vec::new();
        let mut uses_doc_comments = Vec::new();
        let mut trait_aliases = Vec::new();
        let mut resolved_trait_conflicts = Vec::new();

        while !self.check(&Token::RBrace) && !self.at_eof() {
            let member_start = self.current_pos();
            // See the identical handling in `parse_class_body`.
            let attrs = self.parse_leading_attributes()?;
            let doc = self.take_doc_comment();

            if self.eat(&Token::Keyword(Keyword::Case)) {
                let case_name_start = self.current_pos();
                let case_name = self.expect_ident()?;
                let case_name_span = Span {
                    start: case_name_start,
                    end: self.previous_end(),
                };
                let value = if self.eat(&Token::Equals) {
                    Some(self.parse_expression()?)
                } else {
                    None
                };
                self.expect_semicolon()?;
                cases.push(EnumCase {
                    name: case_name,
                    name_span: case_name_span,
                    value,
                    attrs,
                    span: Span {
                        start: member_start,
                        end: self.previous_end(),
                    },
                });
                continue;
            }

            if self.eat(&Token::Keyword(Keyword::Use)) {
                let (name, span) = self.parse_qualified_name_spanned();
                uses.push(name);
                uses_spans.push(span);
                uses_doc_comments.push(doc.clone());
                while self.eat(&Token::Comma) {
                    let (name, span) = self.parse_qualified_name_spanned();
                    uses.push(name);
                    uses_spans.push(span);
                    uses_doc_comments.push(doc.clone());
                }
                if self.check(&Token::LBrace) {
                    let (aliases, resolved) = self.parse_trait_adaptations()?;
                    trait_aliases.extend(aliases);
                    resolved_trait_conflicts.extend(resolved);
                } else {
                    self.expect_semicolon()?;
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
                        self.skip();
                    }
                    Token::Keyword(Keyword::Private) => {
                        visibility = Visibility::Private;
                        self.skip();
                    }
                    Token::Keyword(Keyword::Protected) => {
                        visibility = Visibility::Protected;
                        self.skip();
                    }
                    Token::Keyword(Keyword::Static) => {
                        is_static = true;
                        self.skip();
                    }
                    Token::Keyword(Keyword::Abstract) => {
                        is_abstract_member = true;
                        self.skip();
                    }
                    Token::Keyword(Keyword::Final) => {
                        is_final_member = true;
                        self.skip();
                    }
                    Token::Keyword(Keyword::Readonly) => {
                        self.skip();
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
                let type_hint_start = self.current_pos();
                let type_hint = if self.is_type_start() && !matches!(self.peek_at(1), Token::Equals)
                {
                    self.parse_type_hint()?
                } else {
                    None
                };
                let type_hint_span = type_hint.as_ref().map(|_| Span {
                    start: type_hint_start,
                    end: self.previous_end(),
                });
                loop {
                    let const_start = self.current_pos();
                    let name_start = self.current_pos();
                    let cname = self.expect_ident()?;
                    let cname_span = Span {
                        start: name_start,
                        end: self.previous_end(),
                    };
                    self.expect(&Token::Equals)?;
                    let value = self.parse_expression()?;
                    consts.push(ClassConst {
                        name: cname,
                        name_span: cname_span,
                        value,
                        doc_comment: doc.clone(),
                        visibility: visibility.clone(),
                        attrs: attrs.clone(),
                        type_hint: type_hint.clone(),
                        type_hint_span,
                        is_final: is_final_member,
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
                continue;
            }

            if self.check(&Token::Keyword(Keyword::Function)) {
                let mut function = self.parse_function_decl()?;
                function.doc_comment = doc;
                function.attrs = attrs;
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
            self.skip();
        }
        self.expect(&Token::RBrace)?;

        Ok(EnumDecl {
            name,
            name_span,
            backing_type,
            implements,
            implements_spans,
            cases,
            methods,
            consts,
            uses,
            uses_spans,
            uses_doc_comments,
            trait_aliases,
            resolved_trait_conflicts,
            doc_comment,
            // Overwritten by `parse_statement`, the only caller that saw
            // any leading `#[Attr]` groups - see `ClassDecl.attrs`'s
            // identical note in `parse_class_decl`.
            attrs: Vec::new(),
            span: Span {
                start,
                end: self.previous_end(),
            },
        })
    }
}
