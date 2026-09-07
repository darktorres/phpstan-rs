use crate::ast::*;
use crate::parser::{ParseError, Parser};
use crate::token::{Keyword, Position, Span, Token};

impl Parser {
    // ---- classes ----

    /// True when, starting right after `new`, the upcoming tokens are
    /// `[readonly|final|abstract]* class` - an anonymous class, as opposed
    /// to `new SomeExistingClass(...)`.
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

    /// `new [readonly] class(args) [extends X] [implements Y, ...] { ... }`.
    /// Reuses parse_class_body for the `{ ... }` part, the same as a named
    /// class/interface/trait declaration.
    pub(crate) fn parse_anonymous_class_new(
        &mut self,
        start: Position,
    ) -> Result<Expr, ParseError> {
        let decl_start = self.current_pos();
        let mut class_is_readonly = false;
        loop {
            match self.peek() {
                Token::Keyword(Keyword::Readonly) => {
                    class_is_readonly = true;
                    self.advance();
                }
                Token::Keyword(Keyword::Final) | Token::Keyword(Keyword::Abstract) => {
                    self.advance();
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
        let mut extends_spans = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Extends)) {
            let (name, span) = self.parse_qualified_name_spanned();
            extends.push(name);
            extends_spans.push(span);
            while self.eat(&Token::Comma) {
                let (name, span) = self.parse_qualified_name_spanned();
                extends.push(name);
                extends_spans.push(span);
            }
        }
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

        let (
            properties,
            methods,
            consts,
            uses,
            uses_spans,
            trait_aliases,
            resolved_trait_conflicts,
        ) = self.parse_class_body(class_is_readonly)?;

        let decl_span = Span {
            start: decl_start,
            end: self.previous_end(),
        };
        let decl = ClassDecl {
            // No name multiple anonymous classes in the same file could
            // share safely - never indexed into the registry by this name,
            // only used structurally within this one expression.
            name: "class@anonymous".to_string(),
            // No real name token to point at - see `ClassDecl::name_span`'s
            // own doc comment.
            name_span: decl_span,
            kind: ClassKind::Class,
            extends,
            extends_spans,
            implements,
            implements_spans,
            is_abstract: false,
            is_final: false,
            properties,
            methods,
            consts,
            uses,
            uses_spans,
            trait_aliases,
            resolved_trait_conflicts,
            doc_comment: None,
            // An anonymous class expression has no declaration position of
            // its own for a `#[Attr]` group to precede - not valid PHP
            // syntax at all (`new #[Attr] class {}` doesn't parse in real
            // PHP either).
            attrs: Vec::new(),
            span: decl_span,
        };

        Ok(self.expr(ExprKind::NewAnonymousClass(Box::new(decl), args), start))
    }

    /// The `{ ... }` body shared by class/interface/trait declarations and
    /// anonymous classes (`new class { ... }`) - extracted so the two
    /// don't duplicate this (fairly large) member-parsing loop.
    #[allow(clippy::type_complexity)]
    pub(crate) fn parse_class_body(
        &mut self,
        class_is_readonly: bool,
    ) -> Result<
        (
            Vec<PropertyDecl>,
            Vec<MethodDecl>,
            Vec<ClassConst>,
            Vec<String>,
            Vec<Span>,
            Vec<TraitAlias>,
            Vec<String>,
        ),
        ParseError,
    > {
        self.expect(&Token::LBrace)?;
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut consts = Vec::new();
        let mut uses = Vec::new();
        let mut uses_spans = Vec::new();
        let mut trait_aliases = Vec::new();
        let mut resolved_trait_conflicts = Vec::new();

        while !self.check(&Token::RBrace) && !self.at_eof() {
            let member_start = self.current_pos();
            // Phase 37: `#[Attr]` groups precede everything else about a
            // member (even its visibility/`static`/... modifiers) - parsed
            // before `take_doc_comment` reads through to whatever's next,
            // via a wrapper that re-keys a preceding doc comment so it's
            // still found there unchanged.
            let attrs = self.parse_leading_attributes()?;
            let doc = self.take_doc_comment();

            if self.eat(&Token::Keyword(Keyword::Use)) {
                let (name, span) = self.parse_qualified_name_spanned();
                uses.push(name);
                uses_spans.push(span);
                while self.eat(&Token::Comma) {
                    let (name, span) = self.parse_qualified_name_spanned();
                    uses.push(name);
                    uses_spans.push(span);
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
                        self.advance();
                        self.advance();
                        self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                    }
                    Token::Keyword(Keyword::Protected) if self.peek_is_set_visibility_marker() => {
                        write_visibility = Some(Visibility::Protected);
                        self.advance();
                        self.advance();
                        self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                    }
                    // `public(set)` - same idea, redundantly naming the
                    // write-visibility as public too (e.g. `public
                    // public(set) string $x`).
                    Token::Keyword(Keyword::Public) if self.peek_is_set_visibility_marker() => {
                        write_visibility = Some(Visibility::Public);
                        self.advance();
                        self.advance();
                        self.expect_ident()?;
                        self.expect(&Token::RParen)?;
                    }
                    Token::Keyword(Keyword::Public) => {
                        visibility = Visibility::Public;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Private) => {
                        visibility = Visibility::Private;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Protected) => {
                        visibility = Visibility::Protected;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Static) => {
                        is_static = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Abstract) => {
                        is_abstract_member = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Final) => {
                        is_final_member = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Readonly) => {
                        is_readonly_member = true;
                        self.advance();
                    }
                    // The legacy PHP 4 `var $x;` property declaration -
                    // equivalent to `public $x;`. `var` isn't a reserved
                    // word (unlike every other modifier here), so it only
                    // arrives as a plain Identifier.
                    Token::Identifier(s) if s == "var" => {
                        visibility = Visibility::Public;
                        self.advance();
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

            // property, possibly with a type hint
            let type_hint_start = self.current_pos();
            let type_hint = self.parse_type_hint()?;
            let type_hint_span = type_hint.as_ref().map(|_| Span {
                start: type_hint_start,
                end: self.previous_end(),
            });
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
                        name,
                        name_span,
                        visibility: visibility.clone(),
                        write_visibility: write_visibility.clone(),
                        is_static,
                        is_readonly,
                        type_hint: type_hint.clone(),
                        type_hint_span,
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
                    name,
                    name_span,
                    visibility: visibility.clone(),
                    write_visibility: write_visibility.clone(),
                    is_static,
                    is_readonly,
                    type_hint: type_hint.clone(),
                    type_hint_span,
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
        self.expect(&Token::RBrace)?;

        Ok((
            properties,
            methods,
            consts,
            uses,
            uses_spans,
            trait_aliases,
            resolved_trait_conflicts,
        ))
    }

    /// The `{ ... }` conflict-resolution block after a `use A, B;` clause
    /// (`use A, B { A::hello insteadof B; B::hello as helloB; }`) - each
    /// adaptation is `(TraitName::)?method` followed by either
    /// `insteadof TraitName, ...` (parsed and discarded - not modeled,
    /// see `ClassDecl.trait_aliases`'s own doc comment for why) or
    /// `as [visibility] [newName]` (only a real `newName` is captured;
    /// `as public;`, changing visibility with no rename, has nothing to
    /// register). Real PHP allows a bare `method as newName;` with no
    /// `TraitName::` prefix when the name is unambiguous among the
    /// traits actually used - `trait_name: None` here defers resolving
    /// that ambiguity to `flatten_traits`, which already has the full,
    /// registry-wide picture this parser doesn't.
    pub(crate) fn parse_trait_adaptations(
        &mut self,
    ) -> Result<(Vec<TraitAlias>, Vec<String>), ParseError> {
        let mut aliases = Vec::new();
        let mut resolved = Vec::new();
        self.expect(&Token::LBrace)?;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            // The trait name before `::` can be fully qualified
            // (`\Some\Trait::method insteadof ...`, a real, valid PHP
            // shape - found via bulk-running this project's checker
            // against the vendored phpstan-src corpus) - `parse_qualified_
            // name` handles that and degrades to a single bare segment
            // for the (far more common) unqualified-trait or bare-method
            // case identically to the plain `expect_ident` this replaced.
            let first = self.parse_qualified_name();
            let (trait_name, method) = if self.eat(&Token::DoubleColon) {
                (Some(first), self.expect_ident()?)
            } else {
                (None, first)
            };
            if matches!(self.peek(), Token::Identifier(s) if s == "insteadof") {
                self.advance();
                self.parse_qualified_name();
                while self.eat(&Token::Comma) {
                    self.parse_qualified_name();
                }
                resolved.push(method);
            } else if self.eat(&Token::Keyword(Keyword::As)) {
                if matches!(
                    self.peek(),
                    Token::Keyword(Keyword::Public | Keyword::Private | Keyword::Protected)
                ) {
                    self.advance();
                }
                if !self.check(&Token::Semicolon) {
                    let alias = self.expect_ident()?;
                    aliases.push(TraitAlias {
                        trait_name,
                        method: method.clone(),
                        alias,
                    });
                }
                resolved.push(method);
            }
            self.expect_semicolon()?;
        }
        self.expect(&Token::RBrace)?;
        Ok((aliases, resolved))
    }

    /// PHP 8.4 property hooks body: `{ get => expr; set(Type $value) { .. }
    /// }`, or the abstract/interface form (`{ get; set; }`) with no bodies
    /// at all. `get`/`set` aren't keywords (they're semi-reserved words
    /// used only in this position), so they arrive as plain identifiers.
    pub(crate) fn parse_property_hooks(&mut self) -> Result<PropertyHooks, ParseError> {
        self.expect(&Token::LBrace)?;
        let mut get = None;
        let mut set = None;
        while !self.check(&Token::RBrace) && !self.at_eof() {
            // A hook can itself carry `final`/visibility modifiers ahead
            // of `get`/`set` - not modeled, just consumed.
            while matches!(
                self.peek(),
                Token::Keyword(Keyword::Final)
                    | Token::Keyword(Keyword::Public)
                    | Token::Keyword(Keyword::Private)
                    | Token::Keyword(Keyword::Protected)
            ) {
                self.advance();
            }
            // `&get => ...` - the hook returns its value by reference.
            // Aliasing isn't modeled anywhere in this checker, so it's
            // just consumed.
            self.eat(&Token::Amp);
            let is_set = matches!(self.peek(), Token::Identifier(s) if s == "set");
            self.advance(); // `get` or `set`

            let param = if is_set && self.eat(&Token::LParen) {
                let param_start = self.previous_end();
                let attrs = self.parse_attributes()?;
                let type_hint_start = self.current_pos();
                let type_hint = self.parse_type_hint()?;
                let type_hint_span = type_hint.as_ref().map(|_| Span {
                    start: type_hint_start,
                    end: self.previous_end(),
                });
                let by_ref = self.eat(&Token::Amp);
                let name_start = self.current_pos();
                let name = self.expect_variable()?;
                let name_span = Span {
                    start: name_start,
                    end: self.previous_end(),
                };
                self.expect(&Token::RParen)?;
                Some(Param {
                    name,
                    name_span,
                    type_hint,
                    type_hint_span,
                    default: None,
                    by_ref,
                    variadic: false,
                    promoted_visibility: None,
                    promoted_readonly: false,
                    hooks: None,
                    attrs,
                    doc_comment: None,
                    span: Span {
                        start: param_start,
                        end: self.previous_end(),
                    },
                })
            } else {
                None
            };

            let body = if self.eat(&Token::FatArrow) {
                let e = self.parse_expression()?;
                self.expect_semicolon()?;
                Some(PropertyHookBody::Arrow(e))
            } else if self.check(&Token::LBrace) {
                Some(PropertyHookBody::Block(self.parse_stmt_list_block()?))
            } else {
                // Abstract form: `get;` / `set;`, no body at all.
                self.expect_semicolon()?;
                None
            };

            if is_set {
                set = body.map(|b| (param, b));
            } else {
                get = body;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(PropertyHooks { get, set })
    }

    pub(crate) fn parse_class_decl(&mut self) -> Result<ClassDecl, ParseError> {
        // Captured here, at the very first token of the whole declaration
        // (whichever of `abstract`/`final`/`readonly`/`class`/`interface`/
        // `trait` a caller's dispatch matched on) rather than by a caller
        // - both of this function's call sites in parse_statement would
        // otherwise need their own, easy-to-forget take_doc_comment call,
        // and it needs the enclosing class's own `@template` (Phase 10 -
        // class-level generics), which was never captured at all before.
        let start = self.current_pos();
        let doc_comment = self.take_doc_comment();
        let mut is_abstract = false;
        let mut is_final = false;
        let mut class_is_readonly = false;
        loop {
            match self.peek() {
                Token::Keyword(Keyword::Abstract) => {
                    is_abstract = true;
                    self.advance();
                }
                Token::Keyword(Keyword::Final) => {
                    is_final = true;
                    self.advance();
                }
                // PHP 8.2 `readonly class Foo {}` - shorthand for every
                // property being readonly, applied to each property parsed
                // in parse_class_body below.
                Token::Keyword(Keyword::Readonly) => {
                    class_is_readonly = true;
                    self.advance();
                }
                _ => break,
            }
        }
        let kind = match self.advance() {
            Token::Keyword(Keyword::Class) => ClassKind::Class,
            Token::Keyword(Keyword::Interface) => ClassKind::Interface,
            Token::Keyword(Keyword::Trait) => ClassKind::Trait,
            other => {
                return Err(ParseError {
                    message: format!(
                        "parse error: expected class/interface/trait, found {:?}",
                        other
                    ),
                    span: self
                        .pos
                        .checked_sub(1)
                        .and_then(|i| self.spans.get(i))
                        .copied()
                        .unwrap_or_default(),
                });
            }
        };
        let name_start = self.current_pos();
        let name = self.expect_ident()?;
        let name_span = Span {
            start: name_start,
            end: self.previous_end(),
        };

        // A class extends at most one class, but an interface can extend
        // several other interfaces (`interface C extends A, B {}`) - both
        // are accepted here since the parser doesn't distinguish the two
        // declaration kinds structurally.
        let mut extends = Vec::new();
        let mut extends_spans = Vec::new();
        if self.eat(&Token::Keyword(Keyword::Extends)) {
            let (name, span) = self.parse_qualified_name_spanned();
            extends.push(name);
            extends_spans.push(span);
            while self.eat(&Token::Comma) {
                let (name, span) = self.parse_qualified_name_spanned();
                extends.push(name);
                extends_spans.push(span);
            }
        }

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

        let (
            properties,
            methods,
            consts,
            uses,
            uses_spans,
            trait_aliases,
            resolved_trait_conflicts,
        ) = self.parse_class_body(class_is_readonly)?;

        Ok(ClassDecl {
            name,
            name_span,
            kind,
            extends,
            extends_spans,
            implements,
            implements_spans,
            is_abstract,
            is_final,
            properties,
            methods,
            consts,
            uses,
            uses_spans,
            trait_aliases,
            resolved_trait_conflicts,
            doc_comment,
            // Overwritten by `parse_statement`, the only caller that
            // actually saw any leading `#[Attr]` groups (both of this
            // function's own call sites are reached only after
            // `parse_statement` already consumed and stashed them) - see
            // its own `Token::Keyword(Keyword::Class | ...)` handling.
            attrs: Vec::new(),
            span: Span {
                start,
                end: self.previous_end(),
            },
        })
    }

    // `enum Name [: BackingType] [implements Iface, ...] { ...members... }`.
    // Structurally close to parse_class_decl's member loop (modifiers,
    // `use`, `const`, methods are identical), except enums have `case
    // Name [= value];` instead of properties - they can't declare instance
    // properties or extend anything.
    pub(crate) fn parse_enum_decl(&mut self) -> Result<EnumDecl, ParseError> {
        let start = self.current_pos();
        // Phase 66: captured here, at the very first token, mirroring
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
        let mut trait_aliases = Vec::new();
        let mut resolved_trait_conflicts = Vec::new();

        while !self.check(&Token::RBrace) && !self.at_eof() {
            let member_start = self.current_pos();
            // Phase 37: see the identical handling in `parse_class_body`.
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
                while self.eat(&Token::Comma) {
                    let (name, span) = self.parse_qualified_name_spanned();
                    uses.push(name);
                    uses_spans.push(span);
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
                        self.advance();
                    }
                    Token::Keyword(Keyword::Private) => {
                        visibility = Visibility::Private;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Protected) => {
                        visibility = Visibility::Protected;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Static) => {
                        is_static = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Abstract) => {
                        is_abstract_member = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Final) => {
                        is_final_member = true;
                        self.advance();
                    }
                    Token::Keyword(Keyword::Readonly) => {
                        self.advance();
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
            self.advance();
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
