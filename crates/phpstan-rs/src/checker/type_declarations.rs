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
use syntax::token::Span;
use types::php_version::PhpFeature;

const ONLY_STANDALONE: [&str; 3] = ["mixed", "never", "void"];

const BUILTIN: [&str; 17] = [
    "int", "float", "string", "bool", "false", "true", "null", "void", "never", "mixed", "array",
    "iterable", "object", "callable", "static", "self", "parent",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypePosition<'s> {
    Parameter,
    Return,
    Property(&'s str),
    ClassConstant(&'s str),
}

enum Shape<'h> {
    Single(&'h str),
    Nullable(&'h str),
    Union(Vec<Member<'h>>),
    Intersection(Vec<&'h str>),
}

enum Member<'h> {
    Name(&'h str),
    Group(Vec<&'h str>),
}

fn parse_shape(hint: &str) -> Shape<'_> {
    let hint = hint.trim();
    if let Some(inner) = hint.strip_prefix('?') {
        return Shape::Nullable(inner.trim());
    }
    if hint.contains('|') {
        return Shape::Union(
            hint.split('|')
                .map(str::trim)
                .map(
                    |m| match m.strip_prefix('(').and_then(|m| m.strip_suffix(')')) {
                        Some(group) => Member::Group(group.split('&').map(str::trim).collect()),
                        None => Member::Name(m),
                    },
                )
                .collect(),
        );
    }
    if hint.contains('&') {
        return Shape::Intersection(hint.split('&').map(str::trim).collect());
    }
    Shape::Single(hint)
}

fn lower(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn is_builtin(name: &str) -> bool {
    BUILTIN.contains(&lower(name).as_str())
}

impl<'a> Checker<'a> {
    fn canonical_type_name(&self, name: &str) -> String {
        if is_builtin(name) {
            return lower(name);
        }
        lower(&self.resolve_class_name_for_hint(name))
    }

    fn resolve_class_name_for_hint(&self, name: &str) -> String {
        match self.resolve_type_hint(Some(name)) {
            types::Type::Object(resolved, _) => resolved.to_string(),
            _ => name.trim_start_matches('\\').to_string(),
        }
    }

    fn report_type_declaration(&mut self, message: String, span: Span, id: String) {
        self.report_builder(DiagnosticBuilder::new(message, span, 0, id).non_ignorable());
    }

    pub(crate) fn check_type_declaration(
        &mut self,
        hint: Option<&str>,
        position: TypePosition,
        span: Span,
    ) {
        let Some(hint) = hint else {
            return;
        };
        self.check_type_version_support(hint, span);
        match parse_shape(hint) {
            Shape::Single(name) => self.check_type_position(name, position, span),
            Shape::Nullable(inner) => {
                let inner_lower = lower(inner);
                if ONLY_STANDALONE.contains(&inner_lower.as_str()) {
                    self.report_type_declaration(
                        format!("Type {inner} cannot be part of a nullable type declaration."),
                        span,
                        format!("nullableType.{inner_lower}"),
                    );
                } else if inner_lower == "null" {
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
                    Member::Name(n) if ONLY_STANDALONE.contains(&lower(n).as_str()) => Some(*n),
                    _ => None,
                });
                if let Some(name) = standalone {
                    self.report_type_declaration(
                        format!("Type {name} cannot be part of a union type declaration."),
                        span,
                        format!("unionType.{}", lower(name)),
                    );
                    return;
                }
                self.check_union_redundancy(hint, &members, span);
                for member in &members {
                    match member {
                        Member::Name(n) => self.check_type_position(n, position, span),
                        Member::Group(names) => self.check_intersection(names, span),
                    }
                }
            }
            Shape::Intersection(names) => self.check_intersection(&names, span),
        }
    }

    fn check_type_version_support(&mut self, hint: &str, span: Span) {
        if self.registry.php_versions.is_none() {
            return;
        }
        let unavailable =
            |feature: PhpFeature| self.registry.php_supports_everywhere(feature) == Some(false);
        let shape = parse_shape(hint);
        let names: Vec<&str> = match &shape {
            Shape::Single(n) | Shape::Nullable(n) => vec![*n],
            Shape::Intersection(ns) => ns.clone(),
            Shape::Union(members) => members
                .iter()
                .flat_map(|m| match m {
                    Member::Name(n) => vec![*n],
                    Member::Group(ns) => ns.clone(),
                })
                .collect(),
        };
        if unavailable(PhpFeature::DisjunctiveNormalFormTypes)
            && let Shape::Union(members) = &shape
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
        if names.iter().any(|n| n.eq_ignore_ascii_case("true")) {
            self.report_type_declaration(
                "Type true is supported only on PHP 8.2 and later.".to_string(),
                span,
                "trueType.notSupported".to_string(),
            );
        }
        let only_null_or_false = match &shape {
            Shape::Single(_) | Shape::Nullable(_) => true,
            Shape::Union(members) => members.iter().all(|m| matches!(m, Member::Name(_))),
            Shape::Intersection(_) => false,
        } && names
            .iter()
            .all(|n| n.eq_ignore_ascii_case("null") || n.eq_ignore_ascii_case("false"));
        if only_null_or_false {
            let which = if names.iter().any(|n| n.eq_ignore_ascii_case("false")) {
                "false"
            } else {
                "null"
            };
            self.report_type_declaration(
                format!("Type {hint} uses {which} as a standalone type, which is supported only on PHP 8.2 and later."),
                span,
                format!("{which}Type.standaloneNotSupported"),
            );
        }
    }

    fn check_union_redundancy(&mut self, hint: &str, members: &[Member], span: Span) {
        let mut seen: Vec<String> = Vec::new();
        let mut has_class = false;
        for member in members {
            let Member::Name(name) = member else {
                continue;
            };
            let canonical = self.canonical_type_name(name);
            if seen.contains(&canonical) {
                self.report_type_declaration(
                    format!("Duplicate type {name} is redundant."),
                    span,
                    "unionType.duplicate".to_string(),
                );
                return;
            }
            has_class |=
                !is_builtin(name) || ["self", "parent", "static"].contains(&canonical.as_str());
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
                format!("Type {hint} contains both object and a class type, which is redundant."),
                "unionType.objectAndClass",
            ))
        } else if has("iterable") && has("array") {
            Some((
                format!("Type {hint} contains both iterable and array, which is redundant."),
                "unionType.iterableAndArray",
            ))
        } else if has("iterable") && has("traversable") {
            Some((
                format!("Type {hint} contains both iterable and Traversable, which is redundant."),
                "unionType.iterableAndTraversable",
            ))
        } else {
            None
        };
        if let Some((message, id)) = redundancy {
            self.report_type_declaration(message, span, id.to_string());
        }
    }

    fn check_intersection(&mut self, names: &[&str], span: Span) {
        let mut seen: Vec<String> = Vec::new();
        for name in names {
            let name_lower = lower(name);
            if is_builtin(name) && !["self", "parent"].contains(&name_lower.as_str()) {
                self.report_type_declaration(
                    format!("Type {name} cannot be part of an intersection type."),
                    span,
                    format!("intersectionType.{name_lower}"),
                );
                return;
            }
            let canonical = self.canonical_type_name(name);
            if seen.contains(&canonical) {
                self.report_type_declaration(
                    format!("Duplicate type {name} is redundant."),
                    span,
                    "intersectionType.duplicate".to_string(),
                );
                return;
            }
            seen.push(canonical);
        }
    }

    /// `void`/`never`/`static` outside the positions that allow them.
    fn check_type_position(&mut self, name: &str, position: TypePosition, span: Span) {
        let name_lower = lower(name);
        let (message, id) = match (position, name_lower.as_str()) {
            (TypePosition::Parameter, t @ ("void" | "never" | "static")) => (
                format!("{t} cannot be used as a parameter type."),
                format!("parameter.{t}"),
            ),
            (TypePosition::Property(prop), t @ ("void" | "never" | "static")) => (
                format!("Property {prop} cannot have type {t}."),
                format!("property.{t}Type"),
            ),
            (
                TypePosition::ClassConstant(constant),
                t @ ("void" | "never" | "callable" | "static"),
            ) => (
                format!("Class constant {constant} cannot have type {t}."),
                format!("classConstant.{t}Type"),
            ),
            _ => return,
        };
        self.report_type_declaration(message, span, id);
    }
}
