//! Flags a declared property-type union member that no actual write
//! anywhere in the class's own body ever assigns - the property-typed
//! sibling of `return_type_too_wide.rs`'s `TooWideFunctionReturnTypehintRule`/
//! `TooWideMethodReturnTypehintRule` port, following the identical
//! algorithm against `Checker::property_assign_facts`/`cross_file_
//! property_assign_facts` instead of `current_function_return_points`.
//!
//! Scoped to `write_visibility == Private` only (see `check_property_
//! assignment`'s own accumulation-site comment): that's the only
//! visibility for which this project can claim to have seen every real
//! write. A promoted constructor property is never assigned via a real
//! `ExprKind::Assign` node (`check_property_assignment`'s own doc
//! comment), so it never accumulates any facts and is silently skipped
//! here - a disclosed leniency, not a false negative this check claims
//! to catch.

use crate::checker::Checker;
use syntax::ast::{ClassDecl, Expr, ExprKind, Literal, Visibility};
use types::Type;
use types::union_types;

fn flatten_union(ty: &Type) -> Vec<&Type> {
    match ty {
        Type::Union(members) => members.iter().collect(),
        other => vec![other],
    }
}

/// A property's own `= null` default is a real, reachable value (every
/// instance starts there until a real write happens), but only an
/// `ExprKind::Assign` node inside a method body ever accumulates a
/// `property_assign_facts`/`cross_file_property_assign_facts` entry, so
/// a declaration-site default is otherwise invisible to this check.
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
    /// Called once per class, right after every one of its own methods
    /// (and, for a trait, every host re-check via `check_trait_methods`)
    /// has been checked - the same choke point `check_unused_private_
    /// members` already uses for its own "needs every body scanned
    /// together" requirement.
    pub(crate) fn check_too_wide_property_types(&mut self, fqn: &str, c: &ClassDecl) {
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        for p in &c.properties {
            let Some(prop_sig) = info.properties().get(p.name.as_ref()) else {
                continue;
            };
            if prop_sig.write_visibility() != Visibility::Private {
                continue;
            }
            let Type::Union(declared_members) = prop_sig.ty() else {
                continue;
            };
            if declared_members.len() < 2 {
                continue;
            }
            // Prefer the whole-project merge (sees a trait-donated
            // write's own contribution, wherever it was discovered) -
            // falls back to this run's own same-file accumulation when
            // no two-pass orchestration wired the merge in at all
            // (`cross_file_property_assign_facts`'s own doc comment).
            let assign_types = self
                .cross_file_property_assign_facts
                .as_ref()
                .and_then(|f| f.get(fqn))
                .and_then(|m| m.get(p.name.as_ref()))
                .or_else(|| {
                    self.property_assign_facts
                        .get(fqn)
                        .and_then(|m| m.get(p.name.as_ref()))
                });
            // Never assigned anywhere this project saw at all - not this
            // check's own concern (an always-`null`-default/never-
            // written property is either dead code or reads its initial
            // default forever, neither of which is "declares a union
            // member nothing ever uses").
            let Some(assign_types) = assign_types else {
                continue;
            };
            let Some(actual) = assign_types.iter().cloned().reduce(union_types) else {
                continue;
            };
            let actual = if is_null_default(&p.default) {
                union_types(actual, Type::null())
            } else {
                actual
            };
            let actual_members = flatten_union(&actual);
            let identifier = if prop_sig.is_static() {
                "staticProperty.unusedType"
            } else {
                "property.unusedType"
            };
            let location = format!("{fqn}::${}", p.name);
            for member in declared_members {
                // Same trinary logic as the return-type sibling: only a
                // definite No flags a member; `Mixed` anywhere in the
                // actual union means "could be anything, including this
                // member," not "definitely not this member."
                let is_used = actual_members
                    .iter()
                    .any(|a| a.is_mixed() || self.accepts(member, a).yes());
                if is_used {
                    continue;
                }
                self.report(format!(
                        "{location} is never assigned {member} so it can be removed from the property type."
                    ),
                    p.span,
                    4,
                    identifier,
                );
            }
        }
    }
}
