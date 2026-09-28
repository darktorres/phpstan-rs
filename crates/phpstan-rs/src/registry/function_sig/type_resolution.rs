use rustc_hash::FxHashMap;

use super::generics::mark_template_params;
use crate::registry::type_node::parse_type_node;
use phpdoc::grammar::TypeNode;
use syntax::ast::{Expr, ExprKind, Literal};
use types::{self, TemplateParam, Type};

/// Resolves a param's/property's type: a PHPDoc type (when present) wins
/// over the native hint, since it's usually the more precise of the two
/// (an untyped native param described only in `@param`, or a native `array`
/// refined to `User[]`) - falls back to the native hint, then `Mixed`.
/// A parameter with a plain (non-`?`) native type hint and a bare
/// `= null` default is implicitly nullable in real PHP - `function
/// f(int $x = null)` accepts `null` exactly as if it had been written
/// `function f(?int $x = null)`. This was the only way to write an
/// optional/nullable native parameter before PHP 7.1 introduced `?`, so
/// a large amount of real code (old and new) still uses this style;
/// later PHP versions only added a deprecation notice for it, the
/// behavior itself is unchanged.
pub(crate) fn apply_implicit_nullable_default(ty: Type, default: Option<&Expr>) -> Type {
    let is_bare_null_default = matches!(
        default.map(|e| &e.kind),
        Some(ExprKind::Literal(Literal::Null))
    );
    if !is_bare_null_default {
        return ty;
    }
    let already_nullable = !ty.is_null().no();
    if already_nullable {
        ty
    } else {
        Type::union_of(vec![ty, Type::null()])
    }
}

pub(crate) fn resolve_type(doc_type: Option<&TypeNode>, native_hint: Option<&str>) -> Type {
    let native_ty = native_hint.map(types::parse_native_type_string);
    resolve_type_with_parsed_native(
        doc_type,
        native_hint,
        native_ty.as_ref(),
        &FxHashMap::default(),
    )
}

/// `resolve_type`'s own real body, taking the native hint's already-parsed
/// `Type` instead of re-deriving it from `native_hint` via a second
/// `types::parse_type_string` call - a caller that also needs the
/// native-only type on its own (`FunctionReflection::from_decl`'s `ty`/`ty_native`
/// split) parses `native_hint` once and passes the same `Type` to both,
/// instead of each independently re-parsing the identical string.
/// `native_hint`'s own raw text is still needed alongside `native_ty` for
/// the two checks below that read it as text (nullability prefix, the
/// `object` exclusion) rather than as a parsed `Type`. `template_params`
/// are the templates in scope, marked in the doc type before it is
/// reconciled with the native one.
pub(crate) fn resolve_type_with_parsed_native(
    doc_type: Option<&TypeNode>,
    native_hint: Option<&str>,
    native_ty: Option<&Type>,
    template_params: &FxHashMap<String, TemplateParam>,
) -> Type {
    let Some(doc) = doc_type else {
        return native_ty.cloned().unwrap_or(Type::mixed());
    };
    let ty = mark_template_params(parse_type_node(doc), template_params);
    // A docblock type is trusted as the complete picture and otherwise
    // fully replaces the native hint (deliberate - it's usually more
    // precise) - but real docblocks very commonly aren't fully redundant
    // with the native hint, most often by not bothering to repeat a `?`
    // the native signature already has (`/** @param int $x */` on
    // `function f(?int $x)`, since the signature already fully states
    // the nullability). Silently losing that would make an entirely
    // legitimate `null` argument a false positive - union `Null` in
    // unless the docblock type already says so itself.
    let native_is_nullable = native_hint.is_some_and(|h| h.trim_start().starts_with('?'));
    // A conditional return type states its nullability per branch
    // (`($p is non-empty-array ? T : T|null)`), so a native `?` must not
    // be unioned onto every resolution of it.
    let already_nullable = ty.as_conditional().is_some() || !ty.is_null().no();
    let ty = if native_is_nullable && !already_nullable {
        Type::union_of(vec![ty, Type::null()])
    } else {
        ty
    };
    // phpstorm-stubs' own `@return`/`@param` doc comments commonly
    // predate a later PHP version's native-signature tightening - e.g.
    // `imagecreatefromstring`'s native return hint was updated to
    // `GdImage|false` for PHP 8's GD object migration, but its `@return
    // resource|GdImage|false` doc comment above it was never revised;
    // `imagesx`/`imagesy`'s native `int` similarly dropped `false`
    // without their `@return int|false` doc ever catching up. PHP
    // enforces a native type hint identically at runtime, so when one is
    // present and real, it's authoritative about what's *possible* - any
    // docblock member incompatible with it can never actually happen,
    // whatever a stale comment claims. `resource` and `false` aren't the
    // only members that can go stale this way, so rather than
    // hardcoding a check per member, `reconcile_with_native` tests
    // "incompatible" precisely via `is_super_type_of`.
    //
    // Excludes `object` specifically - real code (this project's own
    // `Logger::$fileHandle`, Symfony `Process::setInput`) deliberately
    // types something `mixed`/`object` *because* PHP has no native
    // `resource` type to declare, leaning on the docblock to carry the
    // only real type information. A `mixed` native doesn't need its own
    // exclusion here: `subtype(_, Type::Mixed(false))` is unconditionally
    // `true`, so `reconcile_with_native` already keeps every doc member
    // when native is `mixed`, for free. `object`'s own subtyping is
    // name-equality-only (see `Relation::is_super_type_of`'s own doc comment), so
    // it doesn't get the same automatic fallout and still needs the
    // explicit guard.
    match (native_hint, native_ty) {
        (Some(native), Some(native_ty)) if native.trim() != "object" => {
            ty.reconcile_with_native(native_ty.clone())
        }
        _ => ty,
    }
}
