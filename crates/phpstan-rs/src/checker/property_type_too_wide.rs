//! Flags a declared property-type union member that no write to the
//! property ever assigns - the property-typed sibling of
//! `return_type_too_wide.rs`'s `TooWideFunctionReturnTypehintRule`/
//! `TooWideMethodReturnTypehintRule` port, following the same algorithm
//! against `Checker::property_assign_facts` instead of
//! `current_function_return_points`.
//!
//! Scoped to properties only their own class may write
//! (`write_visibility == Private`): only for those can every write be
//! seen. The writes happen in the scope of the class itself or of a host
//! of one of its traits, which may be declared in another file, so a
//! class's properties are recorded as a `PropertyTypeSite` while its file
//! is checked and judged once the facts of every file writing them are
//! merged (`Project`), or when `check_program` ends for a checker whose
//! caller doesn't defer them. A promoted constructor property is never
//! assigned via an `ExprKind::Assign` node, so it collects no facts and
//! is skipped.

use crate::checker::{Checker, PropertyAssignFacts};
use diagnostics::{Diagnostic, DiagnosticBuilder};
use syntax::ast::{ClassDef, Expr, ExprKind, Literal, Visibility};
use syntax::token::Span;
use types::{Name, Relation, Type};

/// A class-like's private properties declared with a union type, waiting
/// for every write to them.
#[derive(Debug, Clone)]
pub(crate) struct PropertyTypeSite {
    /// The class-like declaring the properties.
    class: Name,
    /// The classes in whose scope the properties are written, whose
    /// facts hold the writes: the class itself, or a trait's hosts.
    writers: Vec<Name>,
    span: Span,
    properties: Vec<SiteProperty>,
}

impl PropertyTypeSite {
    /// The classes whose facts this site is judged against.
    pub(crate) fn writers(&self) -> &[Name] {
        &self.writers
    }
}

#[derive(Debug, Clone)]
struct SiteProperty {
    name: Name,
    declared: Type,
    span: Span,
    null_default: bool,
    is_static: bool,
}

/// A property's own `= null` default is a real, reachable value (every
/// instance starts there until a real write happens), but only an
/// `ExprKind::Assign` node inside a method body ever records a fact, so a
/// declaration-site default is otherwise invisible to this check.
fn is_null_default(default: &Option<Expr>) -> bool {
    matches!(
        default,
        Some(Expr {
            kind: ExprKind::Literal(Literal::Null),
            ..
        })
    )
}

impl<'a> Checker<'a> {
    /// Records the private union-typed properties of `c`, the class-like
    /// `fqn`, as a site to judge against the writes made in the scope of
    /// `writers`. A class-like checked more than once (an anonymous class
    /// in a loop body) is recorded once.
    pub(crate) fn record_property_type_site(
        &mut self,
        fqn: &str,
        writers: Vec<Name>,
        c: &ClassDef,
    ) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        let properties: Vec<SiteProperty> = c
            .properties
            .iter()
            .filter_map(|p| {
                let sig = info.properties().get(p.name.as_ref())?;
                if sig.write_visibility() != Visibility::Private
                    || sig.ty().union_members().len() < 2
                {
                    return None;
                }
                Some(SiteProperty {
                    name: Name::from(p.name.as_ref()),
                    declared: sig.ty().clone(),
                    span: p.span,
                    null_default: is_null_default(&p.default),
                    is_static: sig.is_static(),
                })
            })
            .collect();
        if properties.is_empty()
            || self
                .property_type_sites
                .iter()
                .any(|site| *site.class == *fqn && site.span == c.span)
        {
            return;
        }
        self.property_type_sites.push(PropertyTypeSite {
            class: Name::from(fqn),
            writers,
            span: c.span,
            properties,
        });
    }

    /// The sites recorded while checking, for a caller that deferred them
    /// (`set_defer_property_type_checks`).
    pub(crate) fn take_property_type_sites(&mut self) -> Vec<PropertyTypeSite> {
        std::mem::take(&mut self.property_type_sites)
    }
}

/// Judges `sites` against `facts`: a declared union member that none of
/// the assigned types (nor a `null` default) may be is reported. A
/// property never written is skipped - it is dead or keeps its default,
/// neither of which is "declares a member nothing uses".
pub(crate) fn check_too_wide_property_types(
    sites: &[PropertyTypeSite],
    facts: &PropertyAssignFacts,
    relation: &Relation,
) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    for site in sites {
        for p in &site.properties {
            let assigned = site
                .writers
                .iter()
                .filter_map(|writer| facts.get(writer.as_ref())?.get(p.name.as_ref()))
                .flatten()
                .cloned()
                .reduce(Type::union);
            let Some(actual) = assigned else {
                continue;
            };
            let actual = if p.null_default {
                Type::union(actual, Type::null())
            } else {
                actual
            };
            let actual_members = actual.union_members();
            let identifier = if p.is_static {
                "staticProperty.unusedType"
            } else {
                "property.unusedType"
            };
            for member in p.declared.union_members() {
                // Same trinary logic as the return-type sibling: only a
                // definite No flags a member; `Mixed` anywhere in the
                // actual union means "could be anything, including this
                // member," not "definitely not this member."
                let is_used = actual_members
                    .iter()
                    .any(|a| a.is_mixed() || relation.accepts(member, a).yes());
                if is_used {
                    continue;
                }
                found.push(
                    DiagnosticBuilder::new(
                        format!(
                            "{}::${} is never assigned {member} so it can be removed from the property type.",
                            site.class, p.name
                        ),
                        p.span,
                        4,
                        identifier,
                    )
                    .build(),
                );
            }
        }
    }
    found
}
