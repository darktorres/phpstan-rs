//! PHPStan's `RuleLevelHelper`: the settings that make type acceptance
//! more or less lenient, applied over a class hierarchy's relations.

use types::{ClassHierarchy, Relation, Type};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RuleLevelHelper {
    /// `array<mixed>` and a `mixed` shape field stop meaning "unknown".
    pub strict_mixed_arrays: bool,
    /// PHPStan's `checkBenevolentUnionTypes`.
    pub check_benevolent_union_types: bool,
}

impl RuleLevelHelper {
    /// The relations over `hierarchy`, as lenient as these settings say.
    pub fn relation<'h>(&self, hierarchy: &'h dyn ClassHierarchy) -> Relation<'h> {
        Relation {
            hierarchy,
            strict_mixed_arrays: self.strict_mixed_arrays,
            check_benevolent_union_types: self.check_benevolent_union_types,
        }
    }

    /// PHPStan's `findTypeToCheck`, as the level a rule reports `ty` at
    /// for the members `fails` holds for: the rule's `base` level when it
    /// holds for every member but `null`, level 7 when only for some of
    /// them (`checkUnionTypes`; a benevolent union's members only with
    /// `checkBenevolentUnionTypes`), level 8 when only for `null`
    /// (`checkNullables`). `None` when it holds for none. A template
    /// bounded by a union is judged by its bound's members.
    pub fn failing_level(&self, ty: &Type, base: u8, fails: impl Fn(&Type) -> bool) -> Option<u8> {
        let mut members = Vec::new();
        level_members(ty, &mut members);
        let (nulls, others): (Vec<&Type>, Vec<&Type>) =
            members.into_iter().partition(|m| m.is_null().yes());
        if others.is_empty() {
            return fails(ty).then_some(base);
        }
        if others.iter().all(|m| fails(m)) {
            return Some(base);
        }
        let by_member = !ty.is_benevolent_union() || self.check_benevolent_union_types;
        if by_member && others.iter().any(|m| fails(m)) {
            return Some(base.max(7));
        }
        nulls.iter().any(|m| fails(m)).then_some(base.max(8))
    }
}

/// `ty`'s union members, a template bounded by a union standing for its
/// bound's members.
fn level_members<'t>(ty: &'t Type, out: &mut Vec<&'t Type>) {
    for member in ty.union_members() {
        match member.template_bound() {
            Some(bound) if bound.union_members().len() > 1 => level_members(bound, out),
            _ => out.push(member),
        }
    }
}

#[cfg(test)]
mod tests;
