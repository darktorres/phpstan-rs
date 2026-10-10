//! The checked-exception/throw-type model.
//!
//! - `throws.notThrowable` (a `@throws` tag naming something that isn't
//!   actually a `Throwable` subtype) and `throws.void` (an explicit
//!   `@throws void` directly contradicted by a real `throw` visible in
//!   the body) - self-contained, needing no per-callee throw-type model.
//! - Transitive throw-point collection: `Checker::current_function_
//!   throw_points`/`current_function_has_unannotated_call`, recorded at
//!   every real `throw` and every already-resolved-signature call choke
//!   point (`checker/expr.rs`'s `ExprKind::Call`/`New`/`MethodCall`/
//!   `StaticCall` arms, `checker/stmt/try_catch.rs`'s `check_throw`).
//! - Per-callee dynamic throw-type extensions: `dynamic` submodule,
//!   tried before each hook's plain `FunctionReflection.throws` fallback.
//! - Too-wide throw-type checks: `too_wide` submodule.
//! - Opt-in "missing checked exception" family, gated on
//!   `ExtrasSettings::check_missing_checked_exceptions`: `missing` submodule.
//!
//! Declined: a method-throw-type covariance check, since no call-graph-
//! wide implicit-throw model exists to build it on.

use crate::checker::Checker;
use crate::registry::{FunctionReflection, Registry};
use syntax::span::Span;
use types::Type;

mod dynamic;
mod missing;
mod too_wide;

impl<'a> Checker<'a> {
    /// Records one call's contribution to `current_function_throw_points`/
    /// `current_function_has_unannotated_call` - shared by every
    /// call-shape choke point. `dynamic` is a per-callee override,
    /// already computed by the caller (tried first); `sig` is the
    /// plainly-resolved callee signature, whose `throws` field is the
    /// fallback once `dynamic` is `None`.
    ///
    /// `sig` being `None` (a dynamic or genuinely unresolvable call) is
    /// the one case that marks the enclosing declaration `current_
    /// function_has_unannotated_call`: real PHP lets an arbitrary callee
    /// throw anything, so there's no bound to offer. A *resolved*
    /// signature with no `@throws` tag (`sig.throws == None`) is
    /// deliberately treated as "known to throw nothing," not "unknown" -
    /// the common case of an ordinary function nobody documented a
    /// `@throws` tag on because it genuinely doesn't throw. Treating it
    /// as "unknown" instead would make `current_function_has_
    /// unannotated_call` true for nearly every real function body,
    /// suppressing the too-wide-throw-type comparison almost everywhere.
    pub(crate) fn record_call_throw_point(
        &mut self,
        dynamic: Option<Type>,
        sig: Option<&FunctionReflection>,
        span: Span,
    ) {
        if !self.try_throw_states.is_empty() {
            // Two calls one expression makes (an implicit one beside its
            // own) throw either's.
            let thrown = call_throw_type(dynamic.as_ref(), sig);
            let joined = match (self.call_throw_types.remove(&span), thrown) {
                (Some(Some(a)), Some(b)) => Some(self.relation().union(a, b)),
                (Some(a), b) => a.or(b),
                (None, b) => b,
            };
            self.call_throw_types.insert(span, joined);
        }
        if let Some(t) = dynamic {
            if !t.is_void() {
                self.current_function_throw_points.push(t);
            }
            return;
        }
        let Some(sig) = sig else {
            self.current_function_has_unannotated_call = true;
            return;
        };
        if let Some(t) = sig.throws()
            && !t.is_void()
        {
            self.current_function_throw_points.push(t.clone());
        }
    }

    /// `throws` is already the union of every `@throws` tag on this
    /// declaration - checked once, at the declaration site, from
    /// `check_function`'s own "one choke point every function-shaped
    /// declaration passes through" comment.
    pub(crate) fn check_throws_tag_validity(&mut self, throws: &Type, span: Span) {
        if throws_is_valid(throws, self.registry) {
            return;
        }
        report!(self, 
            format!("PHPDoc tag @throws with type {throws} is not subtype of Throwable"),
            span,
            2,
            "throws.notThrowable",
        );
    }
}

/// What a call can throw, for the catches around it: a user function's
/// `@throws` type (a per-callee override first), and anything for an
/// unresolved callee, an undocumented user one, or a builtin, which can run
/// user code (a callback, a reflected method, an autoloader, `__toString`)
/// whatever it documents. `None` for `@throws void`.
fn call_throw_type(dynamic: Option<&Type>, sig: Option<&FunctionReflection>) -> Option<Type> {
    let anything = Type::object("Throwable".into(), Vec::new());
    let Some(sig) = sig else {
        return Some(anything);
    };
    match dynamic.or(sig.throws().as_ref()) {
        Some(t) if t.is_void() => None,
        Some(t) if !sig.is_stub() => Some(t.clone()),
        _ => Some(anything),
    }
}

/// Every member of a union must itself be a `Throwable` subtype for the
/// whole tag to be valid. Anything besides a plain class reference
/// (`Mixed` from an unparseable type, `Void` as the explicit "never
/// throws" escape hatch, a template param, ...) is left alone - "don't
/// know, don't guess." Same reasoning for a class name the registry has
/// no entry for at all (e.g. a native-extension-only exception class
/// never vendored into the stub corpus): `is_subclass_of` on an unknown
/// class can't confidently say anything either way, and treating
/// "unknown" the same as "confirmed not Throwable" would misreport a
/// real typo'd/missing class.
fn throws_is_valid(ty: &Type, registry: &Registry) -> bool {
    ty.union_members().iter().all(|m| {
        m.as_object().is_none_or(|(class, _)| {
            !registry.has_class(class) || registry.is_subclass_of(class, "Throwable")
        })
    })
}
