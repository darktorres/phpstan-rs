//! Flags a declared return-type union member that no actual return point
//! in the body ever produces. Mirrors `checker::throws::too_wide`'s
//! "union every recorded point, flag a declared member nothing
//! satisfies" algorithm, applied to `Checker::current_function_return_
//! points` instead of `current_function_throw_points`.
//!
//! Gates on a count of *explicit* return statements
//! (`Checker::current_function_saw_explicit_return`), not "is the
//! accumulated vector empty" - the falls-off-the-end path alone can make
//! the vector non-empty with zero explicit returns. Also exempts a
//! non-first-declaration, non-private method whose actual-return union is
//! exactly `null`/`true`/`false` ("don't require `@return null` in
//! descendant classes").

use crate::checker::Checker;
use syntax::ast::Visibility;
use syntax::token::Span;
use types::Type;
use types::union_types;

fn flatten_union(ty: &Type) -> Vec<&Type> {
    match ty {
        Type::Union(members) => members.iter().collect(),
        other => vec![other],
    }
}

impl<'a> Checker<'a> {
    /// Mirrors `should_check_too_wide_method_throw_type`'s gate exactly
    /// (`checker/throws/too_wide.rs`): private/final/first-declaration.
    pub(crate) fn should_check_too_wide_method_return_type(
        &self,
        class_name: &str,
        method_name: &str,
        visibility: Visibility,
    ) -> bool {
        if visibility == Visibility::Private {
            return true;
        }
        let Some(info) = self.registry.classes.get(class_name) else {
            return true;
        };
        if info.is_final || info.final_methods.contains(method_name) {
            return true;
        }
        if !self
            .registry
            .check_too_wide_return_type_in_protected_and_public_methods
        {
            return false;
        }
        self.registry
            .find_method_signature_prototype(class_name, method_name)
            .is_some()
    }

    /// `declared` is this declaration's already-resolved return type,
    /// called once after the whole body is checked (so
    /// `current_function_return_points`/`current_function_saw_explicit_
    /// return` are fully populated) and before either resets for the
    /// next declaration. `is_first_declaration`/`is_private` feed only
    /// the descendant-null exemption.
    pub(crate) fn check_too_wide_return_type(
        &mut self,
        declared: &Type,
        is_first_declaration: bool,
        is_private: bool,
        subject: &str,
        span: Span,
    ) {
        // Only acts when the declared type is literally a `Union`; any
        // other single declared type (a bare class, a `list<int>`-shaped
        // `Intersection`, ...) is returned unchanged with no comparison -
        // treating it as "one member to check" via `flatten_union` would
        // produce a redundant second diagnostic for the same fact
        // `return.type` already reports.
        let Type::Union(declared_members) = declared else {
            return;
        };
        if declared_members.len() < 2 || self.current_function_is_generator {
            return;
        }
        // A count of *explicit* return statements, not "is the vector
        // empty" - the falls-off-the-end path alone can make it non-empty
        // with zero explicit returns (already separately caught by
        // `return.missing`).
        if !self.current_function_saw_explicit_return {
            return;
        }
        let actual = self
            .current_function_return_points
            .iter()
            .cloned()
            .reduce(union_types);
        let Some(actual) = actual else {
            return;
        };
        // "Don't require `@return null` in descendant classes": a
        // non-first-declaration, non-private method whose actual-return
        // union is exactly one of `null`/`true`/`false` is exempt.
        if !is_first_declaration
            && !is_private
            && matches!(actual, Type::Null | Type::True | Type::False)
        {
            return;
        }
        // Each return point's own members - not their union, whose shape
        // merging (`array{0: 'a', 1: X}|array{0: 'b', 1: Y, 2: Z}` into one
        // `array{0: 'a'|'b', ...}`) would stop matching either member.
        let actual_members: Vec<Type> = self
            .current_function_return_points
            .iter()
            .flat_map(flatten_union)
            .cloned()
            .collect();
        for member in declared_members {
            // "Unused" is decided via trinary logic: only a definite No
            // flags a member, a Maybe keeps it - a returned type that fits
            // the member, or merely overlaps it, uses it.
            let is_used = actual_members.iter().any(|a| {
                *a == Type::Mixed(false)
                    || self.registry.accepts(member, a).yes()
                    || !types::types_definitely_disjoint(a, member)
            });
            if is_used {
                continue;
            }
            self.report(
                format!(
                    "{subject} never returns {member} so it can be removed from the return type."
                ),
                span,
                4,
                "return.unusedType",
            );
        }
    }
}
