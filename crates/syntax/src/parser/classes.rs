use crate::ast::*;
use crate::name::Name;
use crate::parser::{ParseError, Parser};
use crate::span::Span;
use crate::token::{Keyword, Token};

/// A class-like body's members, as `Parser::parse_class_body` reads them.
pub(crate) struct ClassBody {
    pub properties: Vec<PropertyDecl>,
    pub methods: Vec<MethodDecl>,
    pub consts: Vec<ClassConst>,
    pub trait_uses: Vec<TraitUse>,
    pub enum_cases: Vec<EnumCase>,
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
    pub(crate) fn parse_anonymous_class_new(&mut self, start: u32) -> Result<Expr, ParseError> {
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
                    return Err(ParseError {
                        message: format!("Cannot use the {word} modifier on an anonymous class"),
                        span: self.current_span(),
                    });
                }
                _ => break,
            }
        }
        self.expect(&Token::Keyword(Keyword::Class))?;

        let args = if self.check(&Token::LParen) {
            self.parse_args()?.0
        } else {
            Vec::new()
        };

        let mut extends = Vec::new();
        // A class extends one class.
        if self.eat(&Token::Keyword(Keyword::Extends)) {
            extends.push(self.parse_name_reference());
        }
        let mut implements = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            implements.push(self.parse_name_reference());
            while self.eat(&Token::Comma) {
                implements.push(self.parse_name_reference());
            }
        }

        let ClassBody {
            properties,
            methods,
            consts,
            trait_uses,
            enum_cases,
        } = self.parse_class_body(class_is_readonly)?;

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
            attrs,
            span: self.span_from(decl_start),
        };

        self.saw_anonymous_class = true;
        Ok(self.expr(
            ExprKind::NewAnonymousClass(Box::new(AnonymousClass {
                def,
                args,
                name: None,
            })),
            start,
        ))
    }

    /// The `{ ... }` body shared by class/interface/trait declarations and
    /// anonymous classes (`new class { ... }`) - extracted so the two
    /// don't duplicate this (fairly large) member-parsing loop.
    pub(crate) fn parse_class_body(
        &mut self,
        class_is_readonly: bool,
    ) -> Result<ClassBody, ParseError> {
        self.expect(&Token::LBrace)?;
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut consts = Vec::new();
        let mut trait_uses = Vec::new();
        let mut enum_cases = Vec::new();

        let mut pending_member: Option<crate::parser::MemberStart> = None;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            if let Some(member) = pending_member.take() {
                self.finish_member(member, &mut properties, &mut methods, &mut consts);
            }
            pending_member = Some(self.member_start(properties.len(), methods.len(), consts.len()));
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
                enum_cases.push(self.parse_enum_case_rest(attrs, member_start)?);
                continue;
            }

            if self.eat(&Token::Keyword(Keyword::Use)) {
                let mut traits = vec![self.parse_name_reference()];
                while self.eat(&Token::Comma) {
                    traits.push(self.parse_name_reference());
                }
                let adaptations = if self.check(&Token::LBrace) {
                    self.parse_trait_adaptations()?
                } else {
                    self.expect_semicolon()?;
                    TraitAdaptations::default()
                };
                trait_uses.push(TraitUse {
                    traits,
                    doc_comment: doc,
                    adaptations,
                    span: self.span_from(member_start),
                });
                continue;
            }

            let mut visibility = Visibility::Public;
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
                    consts.push(ClassConst {
                        namespaced_name: None,
                        removal_span: None,
                        name: Ident {
                            name: cname,
                            span: cname_span,
                        },
                        value,
                        doc_comment: doc.clone(),
                        visibility,
                        attrs: attrs.clone(),
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
                let mut function = self.parse_function_decl()?;
                function.doc_comment = doc;
                function.attrs = attrs;
                // PHP 8.2 `readonly class Foo { function __construct(public
                // int $x) {} }` - shorthand for every property being
                // readonly, promoted constructor properties included
                // (`parse_params`, shared with closures/arrow-fns/plain
                // functions with no such concept, has no way to know this
                // on its own - only this call site does).
                if class_is_readonly {
                    for param in &mut function.params {
                        if let Some(promotion) = &mut param.promotion {
                            promotion.readonly = true;
                        }
                    }
                }
                methods.push(MethodDecl {
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
                    properties.push(PropertyDecl {
                        removal_span: None,
                        name: Ident {
                            name: self.interner.name(&name),
                            span: name_span,
                        },
                        visibility,
                        write_visibility,
                        is_static,
                        is_readonly,
                        is_abstract: is_abstract_member,
                        is_final: is_final_member,
                        type_hint: type_hint.clone(),
                        default,
                        doc_comment: doc.clone(),
                        hooks: Some(hooks),
                        attrs: attrs.clone(),
                        span: Span {
                            start: prop_start,
                            end: self.previous_end(),
                        },
                    });
                    break;
                }
                properties.push(PropertyDecl {
                    removal_span: None,
                    name: Ident {
                        name: self.interner.name(&name),
                        span: name_span,
                    },
                    visibility,
                    write_visibility,
                    is_static,
                    is_readonly,
                    is_abstract: is_abstract_member,
                    is_final: is_final_member,
                    type_hint: type_hint.clone(),
                    default,
                    doc_comment: doc.clone(),
                    hooks: None,
                    attrs: attrs.clone(),
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
            self.finish_member(member, &mut properties, &mut methods, &mut consts);
        }
        self.expect(&Token::RBrace)?;

        Ok(ClassBody {
            properties,
            methods,
            consts,
            trait_uses,
            enum_cases,
        })
    }

    /// The `{ ... }` block after `use A, B`: `[Trait::]method insteadof
    /// Other, ...;` and `[Trait::]method as [visibility] [alias];`. A bare
    /// `method as ...` names no trait; flattening finds the one that has it.
    pub(crate) fn parse_trait_adaptations(&mut self) -> Result<TraitAdaptations, ParseError> {
        let mut out = TraitAdaptations::default();
        self.expect(&Token::LBrace)?;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            let first = self.parse_name_reference();
            let (trait_name, method): (Option<QualifiedName>, Name) =
                if self.eat(&Token::DoubleColon) {
                    (Some(first), self.expect_ident()?)
                } else {
                    (None, first.written)
                };
            if matches!(self.peek(), Token::Identifier(s) if s.eq_ignore_ascii_case("insteadof")) {
                self.skip();
                let mut instead_of = vec![self.parse_name_reference()];
                while self.eat(&Token::Comma) {
                    instead_of.push(self.parse_name_reference());
                }
                if let Some(trait_name) = trait_name {
                    out.precedences.push(TraitPrecedence {
                        trait_name,
                        method,
                        instead_of,
                    });
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
                out.aliases.push(TraitAlias {
                    trait_name,
                    method,
                    visibility,
                    alias,
                });
            }
            self.expect_semicolon()?;
        }
        self.expect(&Token::RBrace)?;
        Ok(out)
    }

    pub(crate) fn parse_class_decl(&mut self) -> Result<ClassDecl, ParseError> {
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
                return Err(ParseError {
                    message: format!(
                        "parse error: expected class/interface/trait, found {:?}",
                        other
                    ),
                    span: self.last_span,
                });
            }
        };
        let name = self.expect_ident_spanned()?;

        // A class extends at most one class, but an interface can extend
        // several other interfaces (`interface C extends A, B {}`) - both
        // are accepted here since the parser doesn't distinguish the two
        // declaration kinds structurally.
        let mut extends = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Extends)) {
            extends.push(self.parse_name_reference());
            while self.eat(&Token::Comma) {
                extends.push(self.parse_name_reference());
            }
        }

        let mut implements = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Implements)) {
            implements.push(self.parse_name_reference());
            while self.eat(&Token::Comma) {
                implements.push(self.parse_name_reference());
            }
        }

        let ClassBody {
            properties,
            methods,
            consts,
            trait_uses,
            enum_cases,
        } = self.parse_class_body(class_is_readonly)?;

        Ok(ClassDecl {
            namespaced_name: name.name.as_str().into(),
            name,
            def: ClassDef {
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
                // Overwritten by `parse_statement`, the only caller that
                // actually saw any leading `#[Attr]` groups (both of this
                // function's own call sites are reached only after
                // `parse_statement` already consumed and stashed them) - see
                // its own `Token::Keyword(Keyword::Class | ...)` handling.
                attrs: Vec::new(),
                span: self.span_from(start),
            },
        })
    }
}
