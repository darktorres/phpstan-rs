use crate::ast::*;
use crate::name::Name;
use crate::parser::{NameKind, ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

/// A class-like body's members, as `Parser::parse_class_body` reads them.
pub(crate) struct ClassBody {
    pub properties: PropertyRange,
    pub methods: MethodRange,
    pub consts: ClassConstRange,
    pub trait_uses: TraitUseRange,
    pub enum_cases: EnumCaseRange,
}

impl Parser<'_> {
    // ---- classes ----

    /// True when, starting right after `new`, the upcoming tokens are
    /// `[readonly|final|abstract]* class` - an anonymous class, as opposed
    /// to `new SomeExistingClass(...)`. Doesn't itself look past a leading
    /// `#[Attr]` group - the call site in `primary.rs` checks for
    /// `Token::AttributeStart` separately before falling back to this,
    /// since a bracket-aware lookahead here (attribute arguments can
    /// contain their own `[`/`]`, e.g. an array literal) would need real
    /// nesting-depth tracking this function's simple linear scan doesn't
    /// have.
    pub(crate) fn at_anonymous_class_start(&self) -> bool {
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

    /// `new [#[Attr]...] [readonly] class(args) [extends X] [implements Y,
    /// ...] { ... }`. Real PHP does allow an attribute group here (see
    /// the PHP manual's own `new #[ExampleAttribute] class () {}`
    /// example) - attributes precede any `readonly`/`final`/`abstract`
    /// modifier, the same order `parse_statement`'s own leading-attributes
    /// handling already establishes for every other declaration. Reuses
    /// parse_class_body for the `{ ... }` part, the same as a named
    /// class/interface/trait declaration.
    pub(crate) fn parse_anonymous_class_new(&mut self, start: u32) -> Result<ExprId, ParseError> {
        let decl_start = self.current_pos();
        let attrs = self.parse_attributes()?;
        let mut class_is_readonly = false;
        loop {
            match self.peek() {
                Token::Keyword(Keyword::Readonly) => {
                    class_is_readonly = true;
                    self.skip();
                }
                Token::Keyword(modifier @ (Keyword::Final | Keyword::Abstract)) => {
                    let word = if *modifier == Keyword::Final {
                        "final"
                    } else {
                        "abstract"
                    };
                    return Err(ParseError::new(
                        format!("Cannot use the {word} modifier on an anonymous class"),
                        self.current_span(),
                    ));
                }
                _ => break,
            }
        }
        self.expect(&Token::Keyword(Keyword::Class))?;

        let args = if self.check(&Token::LParen) {
            let args = self.parse_args()?.0;
            self.commit_args(args)
        } else {
            self.node_arenas.borrow_mut().alloc_args([])
        };

        let extends_start = self.qualified_name_scratch.len();
        // A class extends one class.
        if self.eat(&Token::Keyword(Keyword::Extends)) {
            self.qualified_name_scratch
                .push(self.parse_name_reference());
        }
        let extends = self
            .node_arenas
            .borrow_mut()
            .alloc_qualified_names(self.qualified_name_scratch.drain(extends_start..));
        let implements_start = self.qualified_name_scratch.len();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            self.qualified_name_scratch
                .push(self.parse_name_reference());
            while self.eat(&Token::Comma) {
                self.qualified_name_scratch
                    .push(self.parse_name_reference());
            }
        }
        let implements = self
            .node_arenas
            .borrow_mut()
            .alloc_qualified_names(self.qualified_name_scratch.drain(implements_start..));

        self.nested_depth.set(self.nested_depth.get() + 1);
        let body = self.parse_class_body(class_is_readonly);
        self.nested_depth.set(self.nested_depth.get() - 1);
        let ClassBody {
            properties,
            methods,
            consts,
            trait_uses,
            enum_cases,
        } = body?;
        let def = ClassDef {
            kind: ClassKind::Class,
            backing_type: None,
            extends,
            implements,
            is_abstract: false,
            is_final: false,
            is_readonly: class_is_readonly,
            properties,
            methods,
            consts,
            trait_uses,
            enum_cases,
            doc_comment: None,
            attrs: self.strip_stub_attributes(attrs),
            span: self.span_from(decl_start),
        };

        let def = self.node_arenas.borrow_mut().alloc_class(def);
        let anonymous_class = self
            .node_arenas
            .borrow_mut()
            .alloc_anonymous_class(AnonymousClass { def, args });

        self.saw_anonymous_class = true;
        Ok(self.expr(ExprKind::NewAnonymousClass(anonymous_class), start))
    }

    /// The `{ ... }` body shared by class/interface/trait declarations and
    /// anonymous classes (`new class { ... }`) - extracted so the two
    /// don't duplicate this (fairly large) member-parsing loop.
    pub(crate) fn parse_class_body(
        &mut self,
        class_is_readonly: bool,
    ) -> Result<ClassBody, ParseError> {
        self.expect(&Token::LBrace)?;
        let properties_start = self.property_scratch.len();
        let methods_start = self.method_scratch.len();
        let consts_start = self.class_const_scratch.len();
        let trait_uses_start = self.trait_use_scratch.len();
        let enum_cases_start = self.enum_case_scratch.len();

        let mut pending_member: Option<crate::parser::MemberStart> = None;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            if let Some(member) = pending_member.take() {
                let span = self.member_removal_span(member);
                Self::finish_member_at(
                    member,
                    span,
                    &mut self.property_scratch,
                    &mut self.method_scratch,
                    &mut self.class_const_scratch,
                );
            }
            pending_member = Some(self.member_start(
                self.property_scratch.len(),
                self.method_scratch.len(),
                self.class_const_scratch.len(),
            ));
            let member_start = self.current_pos();
            // `#[Attr]` groups precede everything else about a member
            // (even its visibility/`static`/... modifiers) - parsed
            // before `take_doc_comment` reads through to whatever's next,
            // via a wrapper that re-keys a preceding doc comment so it's
            // still found there unchanged.
            let attrs = self.parse_leading_attributes()?;
            let doc = self.take_doc_comment();

            // PHP's grammar allows an enum case in any class-like body;
            // outside an enum it is a compile error the checker reports.
            if self.eat(&Token::Keyword(Keyword::Case)) {
                let mut case = self.parse_enum_case_rest(attrs, member_start)?;
                case.attrs = self.strip_stub_attributes(case.attrs);
                self.enum_case_scratch.push(case);
                continue;
            }

            if self.eat(&Token::Keyword(Keyword::Use)) {
                let traits_start = self.qualified_name_scratch.len();
                self.qualified_name_scratch
                    .push(self.parse_name_reference());
                while self.eat(&Token::Comma) {
                    self.qualified_name_scratch
                        .push(self.parse_name_reference());
                }
                let traits = self
                    .node_arenas
                    .borrow_mut()
                    .alloc_qualified_names(self.qualified_name_scratch.drain(traits_start..));
                let adaptations = if self.check(&Token::LBrace) {
                    self.parse_trait_adaptations()?
                } else {
                    self.expect_semicolon()?;
                    let mut arenas = self.node_arenas.borrow_mut();
                    TraitAdaptations {
                        aliases: arenas.alloc_trait_aliases([]),
                        precedences: arenas.alloc_trait_precedences([]),
                    }
                };
                self.note_doc(&doc, member_start);
                self.trait_use_scratch.push(TraitUse {
                    traits,
                    doc_comment: doc,
                    adaptations,
                    span: self.span_from(member_start),
                });
                continue;
            }

            let mut visibility = Visibility::Public;
            // Where a visibility modifier was written, which a constant
            // takes only from PHP 7.1.
            let mut written_visibility: Option<Span> = None;
            let mut write_visibility = None;
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
                    // for public-read/private-write) - captured into
                    // `write_visibility` below, kept separate from the
                    // read-side `visibility` above.
                    Token::Keyword(Keyword::Private) if self.peek_is_set_visibility_marker() => {
                        write_visibility = Some(Visibility::Private);
                        self.skip();
                        self.skip();
                        self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                    }
                    Token::Keyword(Keyword::Protected) if self.peek_is_set_visibility_marker() => {
                        write_visibility = Some(Visibility::Protected);
                        self.skip();
                        self.skip();
                        self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                    }
                    // `public(set)` - same idea, redundantly naming the
                    // write-visibility as public too (e.g. `public
                    // public(set) string $x`).
                    Token::Keyword(Keyword::Public) if self.peek_is_set_visibility_marker() => {
                        write_visibility = Some(Visibility::Public);
                        self.skip();
                        self.skip();
                        self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                    }
                    Token::Keyword(Keyword::Public) => {
                        visibility = Visibility::Public;
                        written_visibility.get_or_insert(self.current_span());
                        self.skip();
                    }
                    Token::Keyword(Keyword::Private) => {
                        visibility = Visibility::Private;
                        written_visibility.get_or_insert(self.current_span());
                        self.skip();
                    }
                    Token::Keyword(Keyword::Protected) => {
                        visibility = Visibility::Protected;
                        written_visibility.get_or_insert(self.current_span());
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
                        is_readonly_member = true;
                        self.skip();
                    }
                    // The legacy PHP 4 `var $x;` property declaration -
                    // equivalent to `public $x;`. `var` isn't a reserved
                    // word (unlike every other modifier here), so it only
                    // arrives as a plain Identifier.
                    Token::Identifier(s) if s.eq_ignore_ascii_case("var") => {
                        visibility = Visibility::Public;
                        self.skip();
                    }
                    _ => break,
                }
            }

            if self.eat(&Token::Keyword(Keyword::Const)) {
                if let Some(span) = written_visibility {
                    self.syntax_marks
                        .note(crate::lexer::SyntaxMark::ClassConstVisibility, span);
                }
                // PHP 8.3 typed class constants: `const TYPE NAME = value;`.
                // A type is present exactly when the name-like token here
                // is followed by *another* identifier (the real name)
                // rather than `=` - `const FOO = 1;`'s `FOO` is followed
                // directly by `=`, so nothing is consumed as a type there.
                let type_hint = if self.is_type_start() && !matches!(self.peek_at(1), Token::Equals)
                {
                    self.parse_type_hint()?
                } else {
                    None
                };
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
                    let mut const_doc = doc.clone();
                    let const_attrs =
                        self.transform_stub_member_attrs(attrs.clone(), &mut const_doc);
                    self.note_doc(&const_doc, member_start);
                    self.class_const_scratch.push(ClassConst {
                        namespaced_name: None,
                        removal_span: None,
                        name: self.ident(cname, cname_span),
                        value,
                        doc_comment: const_doc,
                        visibility,
                        attrs: const_attrs,
                        type_hint: type_hint.clone(),
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
                let dropped = self.stub_drops_function(attrs.clone());
                let function = self.parse_function_decl(attrs, doc, None, !dropped)?;
                if dropped {
                    continue;
                }
                // PHP 8.2 `readonly class Foo { function __construct(public
                // int $x) {} }` - shorthand for every property being
                // readonly, promoted constructor properties included
                // (`parse_params`, shared with closures/arrow-fns/plain
                // functions with no such concept, has no way to know this
                // on its own - only this call site does).
                if class_is_readonly {
                    for param in self.node_arenas.borrow_mut().params_mut(function.params) {
                        if let Some(promotion) = &mut param.promotion {
                            promotion.readonly = true;
                        }
                    }
                }
                self.method_scratch.push(MethodDecl {
                    removal_span: None,
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
            let type_hint = self.parse_type_hint()?;
            let is_readonly = class_is_readonly || is_readonly_member;
            loop {
                let prop_start = self.current_pos();
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
                // PHP 8.4 property hooks (`{ get => ...; set(Type $value)
                // { ... } }`) replace the trailing `;` on a *single*
                // property - no comma-list, no default-then-hooks. Ends
                // its own `}`, so no expect_semicolon() afterwards.
                if self.check(&Token::LBrace) {
                    let hooks = self.parse_property_hooks()?;
                    let mut property_doc = doc.clone();
                    let property_attrs =
                        self.transform_stub_member_attrs(attrs.clone(), &mut property_doc);
                    self.note_doc(&property_doc, member_start);
                    self.property_scratch.push(PropertyDecl {
                        removal_span: None,
                        name: self.ident(self.intern(&name), name_span),
                        visibility,
                        write_visibility,
                        is_static,
                        is_readonly,
                        is_abstract: is_abstract_member,
                        is_final: is_final_member,
                        type_hint: type_hint.clone(),
                        default,
                        doc_comment: property_doc,
                        hooks: Some(hooks),
                        attrs: property_attrs,
                        span: Span {
                            start: prop_start,
                            end: self.previous_end(),
                        },
                    });
                    break;
                }
                let mut property_doc = doc.clone();
                let property_attrs =
                    self.transform_stub_member_attrs(attrs.clone(), &mut property_doc);
                self.note_doc(&property_doc, member_start);
                self.property_scratch.push(PropertyDecl {
                    removal_span: None,
                    name: self.ident(self.intern(&name), name_span),
                    visibility,
                    write_visibility,
                    is_static,
                    is_readonly,
                    is_abstract: is_abstract_member,
                    is_final: is_final_member,
                    type_hint: type_hint.clone(),
                    default,
                    doc_comment: property_doc,
                    hooks: None,
                    attrs: property_attrs,
                    span: Span {
                        start: prop_start,
                        end: self.previous_end(),
                    },
                });
                if !self.eat(&Token::Comma) {
                    self.expect_semicolon()?;
                    break;
                }
            }
        }
        if let Some(member) = pending_member.take() {
            let span = self.member_removal_span(member);
            Self::finish_member_at(
                member,
                span,
                &mut self.property_scratch,
                &mut self.method_scratch,
                &mut self.class_const_scratch,
            );
        }
        self.expect(&Token::RBrace)?;

        let mut arenas = self.node_arenas.borrow_mut();
        Ok(ClassBody {
            properties: arenas.alloc_properties(self.property_scratch.drain(properties_start..)),
            methods: arenas.alloc_methods(self.method_scratch.drain(methods_start..)),
            consts: arenas.alloc_class_consts(self.class_const_scratch.drain(consts_start..)),
            trait_uses: arenas.alloc_trait_uses(self.trait_use_scratch.drain(trait_uses_start..)),
            enum_cases: arenas.alloc_enum_cases(self.enum_case_scratch.drain(enum_cases_start..)),
        })
    }

    /// The `{ ... }` block after `use A, B`: `[Trait::]method insteadof
    /// Other, ...;` and `[Trait::]method as [visibility] [alias];`. A bare
    /// `method as ...` names no trait; flattening finds the one that has it.
    pub(crate) fn parse_trait_adaptations(&mut self) -> Result<TraitAdaptations, ParseError> {
        let aliases_start = self.trait_alias_scratch.len();
        let precedences_start = self.trait_precedence_scratch.len();
        self.expect(&Token::LBrace)?;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            // A bare name here is a method's, and no reference.
            let first = self.parse_unrecorded_name_reference();
            let (trait_name, method): (Option<QualifiedName>, Name) =
                if self.eat(&Token::DoubleColon) {
                    self.record_name(&first, NameKind::Class);
                    (Some(first), self.expect_ident()?)
                } else {
                    (None, first.written)
                };
            if matches!(self.peek(), Token::Identifier(s) if s.eq_ignore_ascii_case("insteadof")) {
                self.skip();
                let instead_of_start = self.qualified_name_scratch.len();
                self.qualified_name_scratch
                    .push(self.parse_unrecorded_name_reference());
                while self.eat(&Token::Comma) {
                    self.qualified_name_scratch
                        .push(self.parse_unrecorded_name_reference());
                }
                // The names stand only with a trait to take the method from.
                if let Some(trait_name) = trait_name {
                    for name in &self.qualified_name_scratch[instead_of_start..] {
                        self.record_name(name, NameKind::Class);
                    }
                    let instead_of = self.node_arenas.borrow_mut().alloc_qualified_names(
                        self.qualified_name_scratch.drain(instead_of_start..),
                    );
                    self.trait_precedence_scratch.push(TraitPrecedence {
                        trait_name,
                        method,
                        instead_of,
                    });
                } else {
                    self.qualified_name_scratch.truncate(instead_of_start);
                }
            } else if self.eat(&Token::Keyword(Keyword::As)) {
                let visibility = match self.peek() {
                    Token::Keyword(Keyword::Public) => Some(Visibility::Public),
                    Token::Keyword(Keyword::Protected) => Some(Visibility::Protected),
                    Token::Keyword(Keyword::Private) => Some(Visibility::Private),
                    _ => None,
                };
                if visibility.is_some() {
                    self.skip();
                }
                let alias = if self.check(&Token::Semicolon) {
                    None
                } else {
                    Some(self.expect_ident()?)
                };
                self.trait_alias_scratch.push(TraitAlias {
                    trait_name,
                    method,
                    visibility,
                    alias,
                });
            }
            self.expect_semicolon()?;
        }
        self.expect(&Token::RBrace)?;
        let mut arenas = self.node_arenas.borrow_mut();
        Ok(TraitAdaptations {
            aliases: arenas.alloc_trait_aliases(self.trait_alias_scratch.drain(aliases_start..)),
            precedences: arenas
                .alloc_trait_precedences(self.trait_precedence_scratch.drain(precedences_start..)),
        })
    }

    pub(crate) fn parse_class_decl(
        &mut self,
        attrs: AttributeRange,
        statement_doc: &mut Option<DocCommentRanges>,
    ) -> Result<ClassDecl, ParseError> {
        // Captured here, at the very first token of the whole declaration
        // (whichever of `abstract`/`final`/`readonly`/`class`/`interface`/
        // `trait` a caller's dispatch matched on) rather than by a caller
        // - both of this function's call sites in parse_statement would
        // otherwise need their own, easy-to-forget take_doc_comment call,
        // and it needs the enclosing class's own `@template` (class-level
        // generics).
        let start = self.current_pos();
        let doc_comment = self.take_doc_comment();
        let mut is_abstract = false;
        let mut is_final = false;
        let mut class_is_readonly = false;
        loop {
            match self.peek() {
                Token::Keyword(Keyword::Abstract) => {
                    is_abstract = true;
                    self.skip();
                }
                Token::Keyword(Keyword::Final) => {
                    is_final = true;
                    self.skip();
                }
                // PHP 8.2 `readonly class Foo {}` - shorthand for every
                // property being readonly, applied to each property parsed
                // in parse_class_body below.
                Token::Keyword(Keyword::Readonly) => {
                    class_is_readonly = true;
                    self.skip();
                }
                _ => break,
            }
        }
        let kind = match self.take_token() {
            Token::Keyword(Keyword::Class) => ClassKind::Class,
            Token::Keyword(Keyword::Interface) => ClassKind::Interface,
            Token::Keyword(Keyword::Trait) => ClassKind::Trait,
            other => {
                return Err(ParseError::new(
                    format!(
                        "parse error: expected class/interface/trait, found {:?}",
                        other
                    ),
                    self.last_span,
                ));
            }
        };
        let name = self.expect_ident_spanned()?;

        // A class extends at most one class, but an interface can extend
        // several other interfaces (`interface C extends A, B {}`) - both
        // are accepted here since the parser doesn't distinguish the two
        // declaration kinds structurally.
        let extends_start = self.qualified_name_scratch.len();
        if self.eat(&Token::Keyword(Keyword::Extends)) {
            self.qualified_name_scratch
                .push(self.parse_name_reference());
            while self.eat(&Token::Comma) {
                self.qualified_name_scratch
                    .push(self.parse_name_reference());
            }
        }
        let extends = self
            .node_arenas
            .borrow_mut()
            .alloc_qualified_names(self.qualified_name_scratch.drain(extends_start..));

        let implements_start = self.qualified_name_scratch.len();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            self.qualified_name_scratch
                .push(self.parse_name_reference());
            while self.eat(&Token::Comma) {
                self.qualified_name_scratch
                    .push(self.parse_name_reference());
            }
        }
        let implements = self
            .node_arenas
            .borrow_mut()
            .alloc_qualified_names(self.qualified_name_scratch.drain(implements_start..));

        let ClassBody {
            properties,
            methods,
            consts,
            trait_uses,
            enum_cases,
        } = self.parse_class_body(class_is_readonly)?;
        let def = ClassDef {
            kind,
            backing_type: None,
            extends,
            implements,
            is_abstract,
            is_final,
            is_readonly: class_is_readonly,
            properties,
            methods,
            consts,
            trait_uses,
            enum_cases,
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
