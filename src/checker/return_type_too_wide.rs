//! Phase 45 (item 11, 45A): `TooWideFunctionReturnTypehintRule`/
//! `TooWideMethodReturnTypehintRule` (both `#[RegisteredRule(level: 4)]`),
//! meaning "this declared return-type union member is never actually
//! produced, narrow it." A file-for-file mirror of `checker::throws::
//! too_wide`'s own `TooWideFunctionThrowTypeRule`/`TooWideMethodThrowTypeRule`
//! port, sharing the same body-differing-subject-text shape and the same
//! "union every recorded point, flag a declared member nothing satisfies"
//! algorithm, applied to `Checker::current_function_return_points`
//! instead of `current_function_throw_points`.
//!
//! Real semantics (`vendor/phpstan-src/src/Rules/Functions/
//! TooWideFunctionReturnTypehintRule.php`, `TooWideTypeCheck::check()`):
//! unlike the throw-type check, this one gates on a real *count of
//! explicit return statements* (`Checker::current_function_saw_explicit_
//! return`), not "is the accumulated vector empty" - the falls-off-the-
//! end path alone can make the vector non-empty with zero explicit
//! returns (a real, load-bearing correction from an earlier pass of this
//! item's own plan - see that field's own doc comment). Also exempts a
//! non-first-declaration, non-private method whose actual-return union is
//! exactly `null`/`true`/`false` (`TooWideTypeCheck.php:226-232`, "don't
//! require `@return null` in descendant classes").

use crate::ast::Visibility;
use crate::checker::Checker;
use crate::checker::type_union::union_types;
use crate::token::Span;
use crate::types::Type;

fn flatten_union(ty: &Type) -> Vec<&Type> {
    match ty {
        Type::Union(members) => members.iter().collect(),
        other => vec![other],
    }
}

impl<'a> Checker<'a> {
    /// Mirrors `should_check_too_wide_method_throw_type`'s own gate
    /// exactly (`checker/throws/too_wide.rs`) - real PHPStan's own
    /// `TooWideMethodReturnTypehintRule` shares the identical private/
    /// final/first-declaration nested gate as its throw-type sibling.
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

    /// `declared` is this declaration's own already-resolved return type
    /// (`declared_return`/`current_declared_return_type`, whichever the
    /// caller already has in hand) - called once, after the whole body
    /// has been checked (so `current_function_return_points`/
    /// `current_function_saw_explicit_return` are fully populated), and
    /// before either is reset for the next declaration. `is_first_
    /// declaration`/`is_private` feed the descendant-null exemption only
    /// (the visibility/finality choke point above already decided
    /// *whether* to call this at all).
    pub(crate) fn check_too_wide_return_type(
        &mut self,
        declared: &Type,
        is_first_declaration: bool,
        is_private: bool,
        subject: &str,
        span: Span,
        label: &str,
    ) {
        // Real, load-bearing correction found via the test suite: real
        // PHPStan's own `TooWideTypeCheck::narrowType` only ever acts
        // when the declared type is literally a `UnionType` (its very
        // first check), or - a deliberately unbuilt case, see this
        // module's own doc comment - a boolean-family type under
        // `reportTooWideBool`. For any other single, non-Union declared
        // type (a bare class, a `list<int>`-shaped `Intersection`, ...)
        // it returns the declared type completely unchanged with no
        // further comparison at all - so this check must bail the same
        // way, not treat an arbitrary non-Union type as "one member to
        // check" the way `flatten_union`'s own generic shape invites.
        // Without this, a `list<int>`-declared function whose one actual
        // return value doesn't satisfy it produced a redundant *second*
        // diagnostic for the identical fact `return.type` already
        // reports.
        let Type::Union(declared_members) = declared else {
            return;
        };
        if declared_members.len() < 2 || self.current_function_is_generator {
            return;
        }
        // The real gate this phase's own review found and fixed: a count
        // of *explicit* return statements, not "is the vector empty" -
        // the falls-off-the-end path alone can make it non-empty with
        // zero explicit returns (already separately caught by `return.
        // missing`).
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
        // "Don't require `@return null` in descendant classes"
        // (`TooWideTypeCheck.php:226-232`): a non-first-declaration,
        // non-private method whose actual-return union is exactly one of
        // `null`/`true`/`false` is exempt entirely.
        if !is_first_declaration
            && !is_private
            && matches!(actual, Type::Null | Type::True | Type::False)
        {
            return;
        }
        let actual_members = flatten_union(&actual);
        for member in declared_members {
            // Real, corpus-confirmed bug: real PHPStan's own `narrowType`
            // decides "unused" via *trinary* logic
            // (`$innerType->isSuperTypeOf($actualReturnType)->no()` -
            // only a *definite* No flags a member; a `Maybe` keeps it).
            // `MixedType::isSubTypeOf` (`vendor/phpstan-src/src/Type/
            // MixedType.php:420-434`) returns `Maybe` for exactly this
            // comparison whenever the actual side is `mixed` - real
            // PHPStan treats "the actual value could be anything,
            // including this declared member" as reason to keep it, not
            // reason to flag it. This project's `is_assignable` is a
            // plain bool with no "unknown" state, so `is_assignable(
            // Mixed, member)` collapsing to a hard `false` (Mixed isn't
            // *assignable to* a narrower type) was wrongly read as "this
            // member is never produced" instead of "can't tell" -
            // flagging every declared member on any real-world return
            // whose actual type widens to `Mixed` (an untyped Doctrine
            // hydration result, `ReflectionMethod::invoke()`, an
            // anonymous-class property with no tracked precise type, ...).
            let is_used = actual_members
                .iter()
                .any(|a| **a == Type::Mixed || self.registry.is_assignable(a, member));
            if is_used {
                continue;
            }
            self.report(
                label.to_string(),
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
