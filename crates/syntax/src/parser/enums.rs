use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Span, Token};

impl Parser {
    /// An enum case after its `case` keyword: `Name [= value];`.
    pub(crate) fn parse_enum_case_rest(
        &mut self,
        attrs: Vec<Attribute>,
        member_start: crate::token::Position,
    ) -> Result<EnumCase, ParseError> {
        let name_start = self.current_pos();
        let name = self.expect_ident()?;
        let name_span = Span {
            start: name_start,
            end: self.previous_end(),
        };
        let value = if self.eat(&Token::Equals) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        self.expect_semicolon()?;
        Ok(EnumCase {
            name,
            name_span,
            value,
            attrs,
            span: Span {
                start: member_start,
                end: self.previous_end(),
            },
        })
    }

    // `enum Name [: BackingType] [implements Iface, ...] { ...members... }`.
    // The body is a class-like body; the checker reports a property in an
    // enum, which PHP rejects at compile time.
    pub(crate) fn parse_enum_decl(&mut self) -> Result<ClassDecl, ParseError> {
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
            implements.push(self.qualified_name(name));
            implements_spans.push(span);
            while self.eat(&Token::Comma) {
                let (name, span) = self.parse_qualified_name_spanned();
                implements.push(self.qualified_name(name));
                implements_spans.push(span);
            }
        }

        let crate::parser::classes::ClassBody {
            properties,
            methods,
            consts,
            uses,
            uses_spans,
            uses_doc_comments,
            trait_aliases,
            trait_precedences,
            resolved_trait_conflicts,
            enum_cases,
        } = self.parse_class_body(false)?;

        Ok(ClassDecl {
            namespaced_name: None,
            name: Some(name),
            name_span,
            kind: ClassKind::Enum,
            backing_type,
            extends: Vec::new(),
            extends_spans: Vec::new(),
            implements,
            implements_spans,
            is_abstract: false,
            // An enum is implicitly final.
            is_final: true,
            is_readonly: false,
            properties,
            enum_cases,
            methods,
            consts,
            uses,
            uses_spans,
            uses_doc_comments,
            trait_aliases,
            trait_precedences,
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
