use std::collections::{HashMap, HashSet};

use crate::ast::{Expr, ExprKind, FunctionDecl, Literal};
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
    // Phase 23: phpstorm-stubs' own `@return`/`@param` doc comments
    // commonly predate a later PHP version's native-signature tightening
    // - real shape: `imagecreatefromstring`'s native return hint was
    // updated to `GdImage|false` for PHP 8's GD object migration, but its
    // `@return resource|GdImage|false` doc comment above it was never
    // revised. PHP enforces a native type hint identically at runtime on
    // both params and returns, so when one is present, real, and its own
    // raw text doesn't mention `resource` at all, it's authoritative over
    // a docblock that still does - the docblock's `resource` member there
    // is simply stale, not a legitimate legacy alternative this project
    // should keep believing is possible.
    //
    // "Real" excludes `mixed`/`object` specifically - real code (this
    // project's own `Logger::$fileHandle`, Symfony `Process::setInput`)
    // deliberately types something `mixed` *because* PHP has no native
    // `resource` type to declare, leaning on the docblock to carry the
    // only real type information rather than the native hint contradicting
    // anything - confirmed the hard way: an earlier, unconditional version
    // of this fix incorrectly stripped `resource` from exactly this
    // legitimate "native hint is a deliberate placeholder" shape.
    match native_hint {
        Some(native)
            if !matches!(native.trim(), "mixed" | "object")
                && !native.to_ascii_lowercase().contains("resource") =>
        {
            strip_resource_member(ty)
        }
        _ => ty,
    }
}

/// Removes any `Type::Object("resource", _)` member from `ty` - see
/// `resolve_type`'s own doc comment for why this only fires when a
/// native type hint is present and itself omits `resource`.
fn strip_resource_member(ty: Type) -> Type {
    fn is_resource(t: &Type) -> bool {
        matches!(t, Type::Object(name, _) if name == "resource")
    }
    match ty {
        Type::Union(members) => {
            let kept: Vec<Type> = members.into_iter().filter(|m| !is_resource(m)).collect();
            match kept.len() {
                0 => Type::Mixed,
                1 => kept.into_iter().next().unwrap(),
                _ => Type::Union(kept),
            }
        }
        Type::Object(name, _) if name == "resource" => Type::Mixed,
        other => other,
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

        FunctionSig {
            params,
            return_type,
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
