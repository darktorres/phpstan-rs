use crate::checker::expr::type_definitely_excludes_null;
use syntax::ast::*;
use types::{self, Type};

/// Whether `e` reads a property (instance or static) anywhere in its
/// own expression tree - a guard for `check_constant_
/// loose_comparison`/`check_constant_numeric_comparison`: this project
/// doesn't invalidate a property's own narrowed type across an arbitrary
/// intervening method call (real PHPStan's own `MutatingScope` does,
/// generally assuming any call might mutate any property unless
/// proven otherwise), so e.g. `$this->retrieve_list
/// = [];` followed by `$this->dispatchListCheckIntegrity();` (which, per
/// its own name, populates it) then `if (count($this->retrieve_list) >
/// 0)` would otherwise still believe `$this->retrieve_list` is the
/// empty array literal it was last assigned, making `count(...)`'s own
/// exact-`IntLiteral(0)` special case (`array_shape`'s own zero-field
/// count) look "always false" against the literal `0` it's compared to,
/// even though the intervening call almost certainly changed it. Not
/// an exhaustive AST walk (covers the shapes that actually occur in
/// call arguments/operators: `Call`/`MethodCall` args and receiver,
/// `Binary`/`Unary`/`Grouping`/`Index`) - missing a rarer shape only
/// means this guard occasionally under-protects, never over-flags,
/// matching this project's own "favor false negatives" precedent.
pub(crate) fn expr_reads_a_property(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::PropertyAccess(..)
        | ExprKind::DynamicPropertyAccess(..)
        | ExprKind::ClassConstFetch(..)
        | ExprKind::ClassNameFetch(_)
        | ExprKind::StaticPropertyFetch(..)
        | ExprKind::DynamicStaticPropertyFetch(..) => true,
        ExprKind::Call(callee, args) => {
            expr_reads_a_property(callee) || args.iter().any(|a| expr_reads_a_property(&a.value))
        }
        ExprKind::MethodCall(base, _, args, _) => {
            expr_reads_a_property(base) || args.iter().any(|a| expr_reads_a_property(&a.value))
        }
        ExprKind::Binary(_, l, r) => expr_reads_a_property(l) || expr_reads_a_property(r),
        ExprKind::Unary(_, inner) | ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => {
            expr_reads_a_property(inner)
        }
        ExprKind::Index(base, idx) => {
            expr_reads_a_property(base) || idx.as_deref().is_some_and(expr_reads_a_property)
        }
        _ => false,
    }
}

/// `null_probe_verdict`/`empty_probe_verdict`'s own shared shape - a
/// null-probe or falsiness verdict function, selected once per `isset()`/
/// `empty()` call site (`existence_probe_kind`) and threaded through to
/// both `check_pointless_null_probe` and `check_pointless_offset_or_
/// property_probe`.
pub(crate) type NullProbeVerdictFn = fn(&Type) -> Option<&'static str>;

/// Real PHPStan's own `isset()`/`empty()`/`??`/`??=` "does this
/// null-probe make sense" verdict, condensed to the one shape this
/// project scopes to (a bare, guaranteed-defined variable - see
/// `Checker::check_pointless_null_probe`'s own doc comment for why a
/// deeper chain is out of scope). `None` for a genuinely ambiguous type
/// (`Mixed`, or one that's nullable among other members) - real PHPStan's
/// `IssetCheck` draws the identical two-way split (`isNull()->yes()` vs.
/// `isNull()->no()`, `maybe()` staying silent either way).
pub(crate) fn null_probe_verdict(ty: &Type) -> Option<&'static str> {
    if ty.is_null().yes() {
        return Some("is always null");
    }
    if type_definitely_excludes_null(ty) {
        Some("is not nullable")
    } else {
        None
    }
}

/// `empty()`'s own falsiness-aware verdict - real
/// PHPStan's `EmptyRule`: unlike
/// `null_probe_verdict` just above, `empty()` tests falsiness, not just
/// null-ness, so both `isNull()` and `toBoolean()->isFalse()` have to
/// be unambiguous before it says anything - by the time both are
/// resolved, the message reduces to exactly one of these two outcomes
/// (`isNull`-specific wording is provably unreachable once falsiness is
/// already known).
pub(crate) fn empty_probe_verdict(ty: &Type) -> Option<&'static str> {
    match ty.to_boolean().bool_literal_value() {
        Some(true) => Some("is not falsy"),
        Some(false) => Some("is always falsy"),
        None => None,
    }
}
