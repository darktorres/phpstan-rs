//! Native type declarations PHP rejects at compile time
//! (`InvalidTypesInUnionRule`, plus the rest of the engine's own type
//! legality rules): `mixed`/`never`/`void` inside a union or nullable
//! type, `?null`, duplicate and redundant union members (`int|INT`,
//! `bool|false`, `true|false`, `object|Foo`, `iterable|array`),
//! non-class members of an intersection, `void`/`never`/`static` where
//! the position doesn't allow them, and, below PHP 8.2, standalone
//! `null`/`false`, the `true` type, and DNF types.

use crate::checker::Checker;
use diagnostics::DiagnosticBuilder;
use syntax::ast::{NativeType, QualifiedName, TypeHint, TypeHintKind};
use syntax::span::Span;
use types::php_version::PhpFeature;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypePosition<'s> {
    Parameter,
    Return,
    Property(&'s str),
    ClassConstant(&'s str),
}

/// One named type in a declaration: a keyword, or a class.
#[derive(Clone, Copy)]
enum Atom<'h> {
    Keyword(NativeType, &'h str),
    Class(&'h QualifiedName),
}

impl<'h> Atom<'h> {
    fn of(arenas: &'h syntax::ast::NodeArenas, hint: &'h TypeHint) -> Option<Atom<'h>> {
        match &hint.kind {
            TypeHintKind::Keyword(keyword, written) => Some(Atom::Keyword(*keyword, written)),
            TypeHintKind::Name(name) => Some(Atom::Class(arenas.qualified_name(name)?)),
            _ => None,
        }
    }

    /// The name as written.
    fn written(self) -> &'h str {
        match self {
            Atom::Keyword(_, written) => written,
            Atom::Class(name) => &name.written,
        }
    }

    fn is(self, keyword: NativeType) -> bool {
        matches!(self, Atom::Keyword(k, _) if k == keyword)
    }

    /// What two members must share to be the same type: a keyword in
    /// lowercase, a class by its resolved name, lowercased.
    fn canonical(self) -> String {
        match self {
            Atom::Keyword(_, written) => written.to_ascii_lowercase(),
            Atom::Class(name) => name
                .resolved_class()
                .unwrap_or(name.written.trim_start_matches('\\'))
                .to_ascii_lowercase(),
        }
    }
}

/// A union member: a named type, or a parenthesized intersection.
enum Member<'h> {
    Name(Atom<'h>),
    Group(Vec<Atom<'h>>),
}

enum Shape<'h> {
    Single(Atom<'h>),
    Nullable(Atom<'h>),
    Union(Vec<Member<'h>>),
    Intersection(Vec<Atom<'h>>),
}

fn atoms<'h>(arenas: &'h syntax::ast::NodeArenas, members: &'h [TypeHint]) -> Vec<Atom<'h>> {
    members
        .iter()
        .filter_map(|hint| Atom::of(arenas, hint))
        .collect()
}

/// `hint`'s shape; `None` for a nesting PHP's grammar doesn't produce.
fn shape<'h>(hint: &'h TypeHint, arenas: &'h syntax::ast::NodeArenas) -> Option<Shape<'h>> {
    Some(match &hint.kind {
        TypeHintKind::Keyword(..) | TypeHintKind::Name(_) => Shape::Single(Atom::of(arenas, hint)?),
        TypeHintKind::Nullable(inner) => Shape::Nullable(Atom::of(arenas, inner)?),
        TypeHintKind::Union(members) => Shape::Union(
            arenas
                .type_hints(members)
                .iter()
                .map(|m| match &m.kind {
                    TypeHintKind::Intersection(group) => {
                        Some(Member::Group(atoms(arenas, arenas.type_hints(group))))
                    }
                    _ => Atom::of(arenas, m).map(Member::Name),
                })
                .collect::<Option<_>>()?,
        ),
        TypeHintKind::Intersection(members) => {
            Shape::Intersection(atoms(arenas, arenas.type_hints(members)))
        }
    })
}

const ONLY_STANDALONE: [NativeType; 3] = [NativeType::Mixed, NativeType::Never, NativeType::Void];

fn is_only_standalone(atom: Atom) -> bool {
    ONLY_STANDALONE.iter().any(|k| atom.is(*k))
}

impl<'a> Checker<'a> {
    fn report_type_declaration(&mut self, message: String, span: Span, id: String) {
        report_builder!(self, DiagnosticBuilder::new(message, span, 0, id).non_ignorable());
    }

    pub(crate) fn check_type_declaration(
        &mut self,
        hint: Option<&TypeHint>,
        position: TypePosition,
        span: Span,
    ) {
        let Some(hint) = hint else {
            return;
        };
        let Some(owner) = self.syntax_tree_owner() else {
            return;
        };
        let arenas = owner.arenas();
        let Some(shape) = shape(hint, arenas) else {
            return;
        };
        self.check_type_version_support(hint, arenas, &shape, position, span);
        match shape {
            Shape::Single(atom) => self.check_type_position(atom, position, span),
            Shape::Nullable(inner) => {
                let inner_lower = inner.written().to_ascii_lowercase();
                if is_only_standalone(inner) {
                    self.report_type_declaration(
                        format!(
                            "Type {} cannot be part of a nullable type declaration.",
                            inner.written()
                        ),
                        span,
                        format!("nullableType.{inner_lower}"),
                    );
                } else if inner.is(NativeType::Null) {
                    self.report_type_declaration(
                        "Type null cannot be marked as nullable.".to_string(),
                        span,
                        "nullableType.null".to_string(),
                    );
                } else {
                    self.check_type_position(inner, position, span);
                }
            }
            Shape::Union(members) => {
                let standalone = members.iter().find_map(|m| match m {
                    Member::Name(atom) if is_only_standalone(*atom) => Some(*atom),
                    _ => None,
                });
                if let Some(atom) = standalone {
                    self.report_type_declaration(
                        format!(
                            "Type {} cannot be part of a union type declaration.",
                            atom.written()
                        ),
                        span,
                        format!("unionType.{}", atom.written().to_ascii_lowercase()),
                    );
                    return;
                }
                self.check_union_redundancy(hint, arenas, &members, span);
                for member in &members {
                    match member {
                        Member::Name(atom) => self.check_type_position(*atom, position, span),
                        Member::Group(atoms) => self.check_intersection(atoms, span),
                    }
                }
            }
            Shape::Intersection(atoms) => self.check_intersection(&atoms, span),
        }
    }

    fn check_type_version_support(
        &mut self,
        hint: &TypeHint,
        arenas: &syntax::ast::NodeArenas,
        shape: &Shape,
        position: TypePosition,
        span: Span,
    ) {
        if self.registry.php_versions.is_none() {
            return;
        }
        let unavailable =
            |feature: PhpFeature| self.registry.php_supports_everywhere(feature) == Some(false);
        let atoms: Vec<Atom> = match shape {
            Shape::Single(a) | Shape::Nullable(a) => vec![*a],
            Shape::Intersection(atoms) => atoms.clone(),
            Shape::Union(members) => members
                .iter()
                .flat_map(|m| match m {
                    Member::Name(a) => vec![*a],
                    Member::Group(atoms) => atoms.clone(),
                })
                .collect(),
        };
        if unavailable(PhpFeature::NullableTypes) && matches!(shape, Shape::Nullable(_)) {
            self.report_type_declaration(
                "Nullable types are supported only on PHP 7.1 and later.".to_string(),
                span,
                "nullableType.notSupported".to_string(),
            );
        }
        // Before their versions these keywords name a class, which no
        // value is an instance of.
        let keywords = [
            (NativeType::Void, PhpFeature::VoidType),
            (NativeType::Iterable, PhpFeature::IterableType),
            (NativeType::Object, PhpFeature::ObjectType),
            (NativeType::Mixed, PhpFeature::MixedType),
            (NativeType::Static, PhpFeature::StaticReturnType),
            (NativeType::Never, PhpFeature::NeverType),
        ];
        for (keyword, feature) in keywords {
            if unavailable(feature)
                && let Some(atom) = atoms.iter().find(|a| a.is(keyword))
            {
                let written = atom.written().to_ascii_lowercase();
                self.report_type_declaration(
                    format!(
                        "Type {written} is supported only on PHP {} and later.",
                        feature.threshold()
                    ),
                    span,
                    format!("{written}Type.notSupported"),
                );
            }
        }
        if unavailable(PhpFeature::PureIntersectionTypes) && matches!(shape, Shape::Intersection(_))
        {
            self.report_type_declaration(
                "Intersection types are supported only on PHP 8.1 and later.".to_string(),
                span,
                "intersectionType.notSupported".to_string(),
            );
        }
        if let TypePosition::Property(property) = position
            && unavailable(PhpFeature::NativeUnionTypes)
            && matches!(shape, Shape::Union(_))
        {
            self.report_type_declaration(
                format!(
                    "Property {property} uses native union types but they're supported only on PHP 8.0 and later."
                ),
                span,
                "property.unionTypeNotSupported".to_string(),
            );
        }
        if unavailable(PhpFeature::DisjunctiveNormalFormTypes)
            && let Shape::Union(members) = shape
            && members.iter().any(|m| matches!(m, Member::Group(_)))
        {
            self.report_type_declaration(
                "Disjunctive normal form types are supported only on PHP 8.2 and later."
                    .to_string(),
                span,
                "unionType.dnfNotSupported".to_string(),
            );
        }
        if !unavailable(PhpFeature::StandaloneNullFalseTrueTypes) {
            return;
        }
        if atoms.iter().any(|a| a.is(NativeType::True)) {
            self.report_type_declaration(
                "Type true is supported only on PHP 8.2 and later.".to_string(),
                span,
                "trueType.notSupported".to_string(),
            );
        }
        let only_null_or_false = match shape {
            Shape::Single(_) | Shape::Nullable(_) => true,
            Shape::Union(members) => members.iter().all(|m| matches!(m, Member::Name(_))),
            Shape::Intersection(_) => false,
        } && atoms
            .iter()
            .all(|a| a.is(NativeType::Null) || a.is(NativeType::False));
        if only_null_or_false {
            let which = if atoms.iter().any(|a| a.is(NativeType::False)) {
                "false"
            } else {
                "null"
            };
            self.report_type_declaration(
                format!(
                    "Type {} uses {which} as a standalone type, which is supported only on PHP 8.2 and later.",
                    hint.to_string_with(arenas)
                ),
                span,
                format!("{which}Type.standaloneNotSupported"),
            );
        }
    }

    fn check_union_redundancy(
        &mut self,
        hint: &TypeHint,
        arenas: &syntax::ast::NodeArenas,
        members: &[Member],
        span: Span,
    ) {
        let mut seen: Vec<String> = Vec::new();
        let mut has_class = false;
        for member in members {
            let Member::Name(atom) = member else {
                continue;
            };
            let canonical = atom.canonical();
            if seen.contains(&canonical) {
                self.report_type_declaration(
                    format!("Duplicate type {} is redundant.", atom.written()),
                    span,
                    "unionType.duplicate".to_string(),
                );
                return;
            }
            has_class |= matches!(
                atom,
                Atom::Class(_)
                    | Atom::Keyword(
                        NativeType::SelfType | NativeType::Parent | NativeType::Static,
                        _
                    )
            );
            seen.push(canonical);
        }
        let has = |t: &str| seen.iter().any(|s| s == t);
        let redundancy = if has("bool") && (has("false") || has("true")) {
            let which = if has("false") { "false" } else { "true" };
            Some((
                format!("Duplicate type {which} is redundant."),
                "unionType.duplicate",
            ))
        } else if has("true") && has("false") {
            Some((
                "Type contains both true and false, bool must be used instead.".to_string(),
                "unionType.trueAndFalse",
            ))
        } else if has("object") && has_class {
            Some((
                format!(
                    "Type {} contains both object and a class type, which is redundant.",
                    hint.to_string_with(arenas)
                ),
                "unionType.objectAndClass",
            ))
        } else if has("iterable") && has("array") {
            Some((
                format!(
                    "Type {} contains both iterable and array, which is redundant.",
                    hint.to_string_with(arenas)
                ),
                "unionType.iterableAndArray",
            ))
        } else if has("iterable") && has("traversable") {
            Some((
                format!(
                    "Type {} contains both iterable and Traversable, which is redundant.",
                    hint.to_string_with(arenas)
                ),
                "unionType.iterableAndTraversable",
            ))
        } else {
            None
        };
        if let Some((message, id)) = redundancy {
            self.report_type_declaration(message, span, id.to_string());
        }
    }

    fn check_intersection(&mut self, atoms: &[Atom], span: Span) {
        let mut seen: Vec<String> = Vec::new();
        for atom in atoms {
            if let Atom::Keyword(keyword, written) = atom
                && !matches!(keyword, NativeType::SelfType | NativeType::Parent)
            {
                self.report_type_declaration(
                    format!("Type {written} cannot be part of an intersection type."),
                    span,
                    format!("intersectionType.{}", written.to_ascii_lowercase()),
                );
                return;
            }
            let canonical = atom.canonical();
            if seen.contains(&canonical) {
                self.report_type_declaration(
                    format!("Duplicate type {} is redundant.", atom.written()),
                    span,
                    "intersectionType.duplicate".to_string(),
                );
                return;
            }
            seen.push(canonical);
        }
    }

    /// `void`/`never`/`static` outside the positions that allow them.
    fn check_type_position(&mut self, atom: Atom, position: TypePosition, span: Span) {
        let Atom::Keyword(keyword, _) = atom else {
            return;
        };
        let t = match keyword {
            NativeType::Void => "void",
            NativeType::Never => "never",
            NativeType::Static => "static",
            NativeType::Callable => "callable",
            _ => return,
        };
        let (message, id) = match position {
            TypePosition::Parameter if keyword != NativeType::Callable => (
                format!("{t} cannot be used as a parameter type."),
                format!("parameter.{t}"),
            ),
            TypePosition::Property(prop) if keyword != NativeType::Callable => (
                format!("Property {prop} cannot have type {t}."),
                format!("property.{t}Type"),
            ),
            TypePosition::ClassConstant(constant) => (
                format!("Class constant {constant} cannot have type {t}."),
                format!("classConstant.{t}Type"),
            ),
            _ => return,
        };
        self.report_type_declaration(message, span, id);
    }
}
