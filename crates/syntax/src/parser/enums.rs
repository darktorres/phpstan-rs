use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

impl Parser<'_> {
    /// An enum case after its `case` keyword: `Name [= value];`.
    pub(crate) fn parse_enum_case_rest(
        &mut self,
        attrs: AttributeRange,
        member_start: u32,
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
            name: self.ident(name, name_span),
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
    pub(crate) fn parse_enum_decl(
        &mut self,
        attrs: AttributeRange,
        statement_doc: &mut Option<DocCommentRanges>,
    ) -> Result<ClassDecl, ParseError> {
        let start = self.current_pos();
        // Captured here, at the very first token, mirroring
        // `parse_class_decl`'s identical note above - needed for
        // rejecting a stray `@template` on an enum (enums can't be
        // generic; PHP has no `enum Foo<T>` syntax at all).
        let doc_comment = self.take_doc_comment();
        self.expect(&Token::Keyword(Keyword::Enum))?;
        let name = self.expect_ident_spanned()?;

        let backing_type = if self.eat(&Token::Colon) {
            self.parse_type_hint()?
        } else {
            None
        };

        let mut implements = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            implements.push(self.parse_name_reference());
            while self.eat(&Token::Comma) {
                implements.push(self.parse_name_reference());
            }
        }

        let crate::parser::classes::ClassBody {
            properties,
            methods,
            consts,
            trait_uses,
            enum_cases,
        } = self.parse_class_body(false)?;
        let (extends, implements) = {
            let mut arenas = self.node_arenas.borrow_mut();
            (
                arenas.alloc_qualified_names([]),
                arenas.alloc_qualified_names(implements),
            )
        };

        let def = ClassDef {
            kind: ClassKind::Enum,
            backing_type,
            extends,
            implements,
            is_abstract: false,
            // An enum is implicitly final.
            is_final: true,
            is_readonly: false,
            properties,
            enum_cases,
            methods,
            consts,
            trait_uses,
            doc_comment,
            attrs,
            span: self.span_from(start),
        };
        let mut def = def;
        self.transform_stub_class_attrs(&mut def, Some(statement_doc));
        self.note_class(&def);
        let def = self.node_arenas.borrow_mut().alloc_class(def);
        Ok(ClassDecl {
            namespaced_name: name.name.as_str().into(),
            name,
            def,
        })
    }
}
