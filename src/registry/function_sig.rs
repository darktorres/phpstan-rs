use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, ExprKind, FunctionDecl, Literal};
use crate::checker::union_types;
use crate::docblock::{self, DocBlock};
use crate::registry::name_context::{NameContext, resolve_names_in_type};
use crate::types::{self, Type};

#[derive(Debug, Clone)]
pub struct ParamSig {
    pub name: String,
    pub ty: Type,
    pub variadic: bool,
    pub has_default: bool,
    pub by_ref: bool,
}

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub params: Vec<ParamSig>,
    pub return_type: Type,
    // The return type the *native* signature alone declares, ignoring any
    // `@return` docblock refinement - real PHP only ever enforces
    // override-compatibility (Liskov) at this native level, and by
    // default so does real PHPStan (`checkPhpDocMethodSignatures` is off
    // unless a project opts in, confirmed neither Piwigo's own
    // `phpstan.neon` nor this project's own corpus config does).
    // `check_method_overrides`'s own return-type check is the only
    // reader - every other consumer of a function's return type wants
    // the docblock-refined `return_type` above instead.
    pub return_type_native: Type,
}

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
    let already_nullable = match &ty {
        Type::Null | Type::Mixed => true,
        Type::Union(members) => members.contains(&Type::Null),
        _ => false,
    };
    if already_nullable {
        ty
    } else {
        Type::Union(vec![ty, Type::Null])
    }
}

pub(crate) fn resolve_type(doc_type: Option<&str>, native_hint: Option<&str>) -> Type {
    let Some(doc) = doc_type else {
        return native_hint
            .map(types::parse_type_string)
            .unwrap_or(Type::Mixed);
    };
    let ty = types::parse_type_string(doc);
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
    let already_nullable = match &ty {
        Type::Null | Type::Mixed => true,
        Type::Union(members) => members.contains(&Type::Null),
        _ => false,
    };
    let ty = if native_is_nullable && !already_nullable {
        Type::Union(vec![ty, Type::Null])
    } else {
        ty
    };
    // Phase 23/Phase 32 Finding I: phpstorm-stubs' own `@return`/`@param`
    // doc comments commonly predate a later PHP version's native-signature
    // tightening - real shape: `imagecreatefromstring`'s native return hint
    // was updated to `GdImage|false` for PHP 8's GD object migration, but
    // its `@return resource|GdImage|false` doc comment above it was never
    // revised (Phase 23's own motivating case); `imagesx`/`imagesy`'s
    // native `int` similarly dropped `false` without their `@return
    // int|false` doc ever catching up. PHP enforces a native type hint
    // identically at runtime, so when one is present and real, it's
    // authoritative about what's *possible* - any docblock member
    // incompatible with it can never actually happen, whatever a stale
    // comment claims. `resource` and `false` aren't the only members that
    // can go stale this way, so rather than hardcoding a check per member
    // (as this project's own first attempt at `false` did, before finding
    // this generalization instead), `reconcile_with_native` tests
    // "incompatible" precisely via `is_subtype`.
    //
    // Excludes `object` specifically - real code (this project's own
    // `Logger::$fileHandle`, Symfony `Process::setInput`) deliberately
    // types something `mixed`/`object` *because* PHP has no native
    // `resource` type to declare, leaning on the docblock to carry the
    // only real type information. A `mixed` native doesn't need its own
    // exclusion here: `is_subtype(_, Type::Mixed)` is unconditionally
    // `true`, so `reconcile_with_native` already keeps every doc member
    // when native is `mixed`, for free. `object`'s own subtyping is
    // name-equality-only (see `types::is_subtype`'s own doc comment), so
    // it doesn't get the same automatic fallout and still needs the
    // explicit guard.
    match native_hint {
        Some(native) if native.trim() != "object" => {
            reconcile_with_native(ty, types::parse_type_string(native))
        }
        _ => ty,
    }
}

/// Filters `doc_ty`'s own members down to whichever are subtypes of
/// `native_ty` - see `resolve_type`'s own doc comment for the reasoning.
/// Falls back to `native_ty` itself when *no* doc member survives (rather
/// than collapsing to `Mixed`), so a doc type that's entirely stale
/// (`finfo_file`'s bare `resource` against a modern `finfo` native) still
/// resolves to the real, useful native type instead of losing all
/// precision.
fn reconcile_with_native(doc_ty: Type, native_ty: Type) -> Type {
    let members: Vec<Type> = match doc_ty {
        Type::Union(members) => members,
        other => vec![other],
    };
    let kept: Vec<Type> = members
        .into_iter()
        .filter(|m| member_possible_given_native(m, &native_ty))
        .collect();
    match kept.len() {
        0 => native_ty,
        1 => kept.into_iter().next().unwrap(),
        _ => Type::Union(kept),
    }
}

/// Whether a docblock-derived member `m` could still be real given
/// `native_ty` - found necessary the hard way, via a full corpus re-check
/// after first shipping this as a bare `is_subtype(m, native_ty)` call:
/// for anything except a named class/interface (`Type::Object`),
/// `is_subtype` gives a reliable, hierarchy-independent answer (scalar/
/// array/... subtyping never depends on an unverified class
/// relationship). For an `Object`, this project has no class hierarchy
/// to consult at the point `resolve_type` runs (`FunctionSig::from_decl`
/// is still building the very registry a real lookup would need,
/// long before one exists) - `is_subtype`'s own Object handling is
/// name-equality only, so treating that as "incompatible" wrongly
/// discarded a real, common shape: a doc type naming an actual subclass
/// of a more generic native return hint (real corpus regression:
/// Doctrine-repository-typed services declaring a native
/// `EntityRepository` return with a doc type naming the real concrete
/// repository subclass, e.g. `ActivityRepository` - is_subtype couldn't
/// see the inheritance, so the doc's own more-precise type was wrongly
/// dropped in favor of the generic base class). Scoped back to exactly
/// the one class-name case this project *can* prove reliably (Phase 23's
/// own original, narrower finding: a literal `resource` doc member
/// against a native hint that no longer mentions it) - any other class
/// name is always kept, since "doc names a real subclass" and "doc is
/// simply stale" can't be told apart without the hierarchy this function
/// doesn't have, and silently guessing wrong here is worse than not
/// generalizing past what's actually provable.
fn member_possible_given_native(m: &Type, native_ty: &Type) -> bool {
    match m {
        Type::Object(name, _) if name == "resource" => types::is_subtype(m, native_ty),
        Type::Object(_, _) => true,
        _ => types::is_subtype(m, native_ty),
    }
}

/// A class constant's value is almost always a literal in real code - this
/// covers the common cases the same way `checker.rs`'s own literal
/// inference does, without pulling in the full expression-inference engine
/// (which lives in `checker.rs` and depends on `Registry` itself, so it
/// can't be called from here). Anything more complex than a bare literal
/// (an array, a concatenation, another constant) falls back to `Mixed` -
/// undefined-constant detection only needs the name to be known, not a
/// precise type.
pub(crate) fn infer_const_type(value: &crate::ast::Expr) -> Type {
    use crate::ast::Literal;
    match &value.kind {
        ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(*n),
        ExprKind::Literal(Literal::Float(_)) => Type::Float,
        ExprKind::Literal(Literal::String(s)) => Type::StringLiteral(s.clone()),
        ExprKind::Literal(Literal::Bool(true)) => Type::True,
        ExprKind::Literal(Literal::Bool(false)) => Type::False,
        ExprKind::Literal(Literal::Null) => Type::Null,
        // Phase 23: `const int OFF = -1;` parses as a literal `1` wrapped
        // in a unary negation, not a single negative-literal token - a
        // negative-valued typed constant (real shape: `Piwigo\Core\
        // Logger::OFF`, the one negative severity level alongside seven
        // non-negative ones) otherwise silently fell to `Mixed` below.
        ExprKind::Unary(crate::ast::UnaryOp::Neg, inner) => match &inner.kind {
            ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(-n),
            ExprKind::Literal(Literal::Float(_)) => Type::Float,
            _ => Type::Mixed,
        },
        // A round of Piwigo-corpus bucketing found this real, common
        // shape (`AppInfo::URL = 'https://' . self::DOMAIN;`) - a
        // const's own value referencing *another* const via `self::`
        // can't be resolved here at all (this function has no class
        // context, and no registry to look the other const up in even
        // if it did), but real PHP string concatenation always
        // produces a `string` regardless of either operand's own type,
        // so this is sound information even without resolving anything
        // - unlike falling back to `Mixed` below, which silently
        // widened every array literal/argument this const then fed
        // into.
        ExprKind::Binary(crate::ast::BinOp::Concat, ..) => Type::String,
        _ => Type::Mixed,
    }
}

/// Rewrites every `Object(name)` in a type tree whose name is one of this
/// function's own declared `@template` parameters into `TemplateParam(name)`.
/// `parse_type_string` has no way to know "T" is a template rather than an
/// ordinary (if oddly-named) class, so that reclassification happens here,
/// where the declaring function's `@template` list is known. A name that
/// doesn't itself match still needs its own type arguments walked (e.g.
/// `Collection<T>`'s `T` needs marking even though `Collection` doesn't
/// match anything).
pub(crate) fn mark_template_params(ty: Type, template_names: &HashSet<String>) -> Type {
    match ty {
        Type::Object(name, _) if template_names.contains(&name) => Type::TemplateParam(name),
        Type::Object(name, args) => Type::Object(
            name,
            args.into_iter()
                .map(|a| mark_template_params(a, template_names))
                .collect(),
        ),
        Type::Array(inner) => Type::Array(Box::new(mark_template_params(*inner, template_names))),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(mark_template_params(*inner, template_names)))
        }
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| mark_template_params(m, template_names))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|m| mark_template_params(m, template_names))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, mark_template_params(t, template_names), opt))
                .collect(),
        ),
        other => other,
    }
}

impl FunctionSig {
    /// `class_templates` are the enclosing class's own `@template` names
    /// (empty for a plain function, or a method on a non-generic class) -
    /// merged in alongside the function/method's own, so a method body
    /// referencing a template its *class* declares (not itself) still
    /// gets it correctly recognized as `TemplateParam` rather than an
    /// ordinary (nonexistent) class name.
    pub(crate) fn from_decl(
        f: &FunctionDecl,
        ctx: &NameContext,
        class_templates: &[String],
    ) -> FunctionSig {
        let doc: Option<DocBlock> = f.doc_comment.as_deref().map(docblock::parse);
        let mut template_names: HashSet<String> = doc
            .as_ref()
            .map(|d| d.templates.iter().cloned().collect())
            .unwrap_or_default();
        template_names.extend(class_templates.iter().cloned());

        let params = f
            .params
            .iter()
            .map(|p| {
                let doc_ty = doc.as_ref().and_then(|d| d.params.get(&p.name));
                let ty = resolve_type(doc_ty.map(String::as_str), p.type_hint.as_deref());
                let ty = mark_template_params(ty, &template_names);
                let ty = resolve_names_in_type(ty, ctx);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                // A real `= value` default always wins; failing that, a
                // `[optional]` marker on this param's own `@param` tag is
                // trusted too (see `DocBlock::optional_params` - phpstorm-
                // stubs models several functions, e.g. `mt_rand`/
                // `array_keys`, only as their highest-arity overload, with
                // trailing params marked optional in the docblock but
                // carrying no native default at all).
                let has_default = p.default.is_some()
                    || doc
                        .as_ref()
                        .is_some_and(|d| d.optional_params.contains(&p.name));
                ParamSig {
                    name: p.name.clone(),
                    ty,
                    variadic: p.variadic,
                    has_default,
                    by_ref: p.by_ref,
                }
            })
            .collect();
        let params = dedup_versioned_params(params);

        let doc_return = doc.as_ref().and_then(|d| d.return_type.as_deref());
        let return_type = resolve_type(doc_return, f.return_type.as_deref());
        let return_type = mark_template_params(return_type, &template_names);
        let return_type = resolve_names_in_type(return_type, ctx);

        let return_type_native = resolve_type(None, f.return_type.as_deref());
        let return_type_native = mark_template_params(return_type_native, &template_names);
        let return_type_native = resolve_names_in_type(return_type_native, ctx);

        FunctionSig {
            params,
            return_type,
            return_type_native,
        }
    }

    /// Phase 32 Finding C: merges two *unconditional* declarations of the
    /// same function name - the real-overload shape phpstorm-stubs uses
    /// for `strtr` (a 3-string-argument form and a 2-argument, `array
    /// $replace_pairs` form, as two separate top-level declarations
    /// sharing one name), mirroring `ClassInfo::merge`'s own "later
    /// declaration wins on conflict" convention.
    pub(crate) fn merge(self, other: FunctionSig) -> FunctionSig {
        let return_type = union_types(self.return_type, other.return_type);
        let return_type_native = union_types(self.return_type_native, other.return_type_native);
        let len = self.params.len().max(other.params.len());
        let mut self_params = self.params.into_iter();
        let mut other_params = other.params.into_iter();
        let mut params = Vec::with_capacity(len);
        for _ in 0..len {
            let a = self_params.next();
            let b = other_params.next();
            let merged = match (a, b) {
                (Some(a), Some(b)) => ParamSig {
                    // Later declaration wins on conflict, matching
                    // `ClassInfo::merge`'s own convention.
                    name: b.name,
                    ty: union_types(a.ty, b.ty),
                    // Permissive: accepting a variadic-style tail call
                    // when *either* real overload allows it costs nothing
                    // a stricter rule would have caught.
                    variadic: a.variadic || b.variadic,
                    // A position required by only one real overload can't
                    // be required in a callable meant to accept either.
                    has_default: a.has_default || b.has_default,
                    // Unlike the above, NOT permissive: `by_ref` is a
                    // restriction placed on the *caller* (an lvalue is
                    // required), not something being accepted - OR-ing it
                    // would wrongly flag a call matching the non-by-ref
                    // overload's own shape just because the *other*
                    // overload needs a reference there. AND keeps the
                    // merged signature only as strict as every real
                    // overload agrees it should be.
                    by_ref: a.by_ref && b.by_ref,
                },
                // A position present in only one real overload can't be
                // required in the merged callable either.
                (Some(mut only), None) | (None, Some(mut only)) => {
                    only.has_default = true;
                    only
                }
                (None, None) => unreachable!("loop bound is the longer side's own length"),
            };
            params.push(merged);
        }
        FunctionSig {
            params,
            return_type,
            return_type_native,
        }
    }
}

/// phpstorm-stubs represents "this parameter's type/default changed
/// across PHP versions" by declaring the *same conceptual parameter
/// twice*, once per era, each gated by its own
/// `#[PhpStormStubsElementAvailable(from: X, to: Y)]` attribute (real
/// shape: `pg_query`'s `$connection`, `fsockopen`'s `$port`,
/// `SQLite3::open`'s `$flags`/`$encryptionKey`) - but real PHP cannot
/// declare two parameters with the same name in one function (a fatal
/// error), so any duplicate name reaching here is unconditionally this
/// versioning artifact, never a legitimate shape, regardless of which
/// attribute produced it or whether it has a default. The lexer's own
/// `skip_stub_availability_attribute` heuristic already drops some of
/// these (a closed-range duplicate with no default), but keeps one that
/// does have a default - correct for a different real shape
/// (`imagefilter`'s uniquely-named `$arg1`..`$arg4` before a
/// differently-named variadic) but wrong when the kept duplicate's name
/// is reused by a later parameter, producing a function with one extra
/// position and every real argument after it binding to the wrong
/// parameter. Collapsing here, after the lexer, needs no knowledge of
/// that versioning idiom at all: keep each name's *last* occurrence
/// (phpstorm-stubs always writes the legacy era first, the modern one
/// last), at the *position* of its *first* occurrence, so later
/// uniquely-named parameters aren't shifted.
fn dedup_versioned_params(params: Vec<ParamSig>) -> Vec<ParamSig> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, ParamSig> = HashMap::new();
    for p in params {
        if !by_name.contains_key(&p.name) {
            order.push(p.name.clone());
        }
        by_name.insert(p.name.clone(), p);
    }
    order
        .into_iter()
        .map(|name| by_name.remove(&name).expect("just inserted for this name"))
        .collect()
}
