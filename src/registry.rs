use std::collections::{HashMap, HashSet};

use crate::ast::{
    ClassDecl, ClassKind, EnumDecl, Expr, ExprKind, FunctionDecl, Literal, Stmt, StmtKind,
    UseImport,
};
use crate::docblock::{self, DocBlock};
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
fn apply_implicit_nullable_default(ty: Type, default: Option<&Expr>) -> Type {
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

fn resolve_type(doc_type: Option<&str>, native_hint: Option<&str>) -> Type {
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
    if native_is_nullable && !already_nullable {
        Type::Union(vec![ty, Type::Null])
    } else {
        ty
    }
}

/// A single leading `\` marks "fully qualified from the global namespace"
/// (`\Exception`) - every declaration is keyed by its own fully-qualified
/// name now (`NameContext`, below), and the global namespace's own prefix
/// is simply empty, so stripping this is all a bare reference needs before
/// `NameContext::resolve` takes over for anything more (a `use`-imported
/// or same-namespace-relative name). Mirrors `types::parse_type_string`'s
/// identical handling of type hints, just for `extends`/`implements`/
/// `uses` and expression-level name references, which go through a
/// separate code path (`parse_qualified_name`, not `parse_type_string`).
pub(crate) fn strip_leading_backslash(name: &str) -> String {
    name.trim_start_matches('\\').to_string()
}

/// A class constant's value is almost always a literal in real code - this
/// covers the common cases the same way `checker.rs`'s own literal
/// inference does, without pulling in the full expression-inference engine
/// (which lives in `checker.rs` and depends on `Registry` itself, so it
/// can't be called from here). Anything more complex than a bare literal
/// (an array, a concatenation, another constant) falls back to `Mixed` -
/// undefined-constant detection only needs the name to be known, not a
/// precise type.
fn infer_const_type(value: &crate::ast::Expr) -> Type {
    use crate::ast::Literal;
    match &value.kind {
        ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(*n),
        ExprKind::Literal(Literal::Float(_)) => Type::Float,
        ExprKind::Literal(Literal::String(s)) => Type::StringLiteral(s.clone()),
        ExprKind::Literal(Literal::Bool(true)) => Type::True,
        ExprKind::Literal(Literal::Bool(false)) => Type::False,
        ExprKind::Literal(Literal::Null) => Type::Null,
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
fn mark_template_params(ty: Type, template_names: &HashSet<String>) -> Type {
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

/// Resolves a bare class-name reference (`use`-imports and same-namespace
/// relative references both apply) to a fully-qualified one, so two
/// unrelated classes with the same short name in different namespaces get
/// different registry keys instead of colliding - see the extensive
/// comment on this in `checker.rs`'s module doc for why that matters (a
/// name collision used to silently corrupt an unrelated class's method
/// table). Built once per file (from its own `namespace`/`use`
/// statements) and shared by `Registry::index` (for extends/implements/
/// type hints) and `Checker` (for expression-level references like
/// `new Foo()`) - both need to agree on exactly the same resolution or
/// the two halves of this registry would disagree with each other.
#[derive(Debug, Clone, Default)]
pub(crate) struct NameContext {
    namespace: String,
    // Short name (or `as Alias`) -> the full path it was imported from.
    use_map: HashMap<String, String>,
}

impl NameContext {
    /// Scans a file's top-level statements for `namespace`/`use`
    /// declarations. PHP only allows one semicolon-style `namespace Foo;`
    /// per file (the brace-delimited multi-namespace-per-file form is
    /// rare enough, and unsupported by the parser's own StmtKind::Namespace
    /// handling already, to not be worth modeling here) - once found, it
    /// applies to the rest of the file, so a single linear scan suffices.
    pub(crate) fn build(stmts: &[Stmt]) -> NameContext {
        let mut ctx = NameContext::default();
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Namespace(name) => ctx.set_namespace(name),
                StmtKind::Use(imports) => ctx.add_uses(imports),
                _ => {}
            }
        }
        ctx
    }

    /// Enters a (possibly new) namespace - real `use` imports are scoped
    /// to the namespace block they appear in, so a fresh one clears
    /// whatever was collected under the previous namespace rather than
    /// letting it leak into declarations that follow (relevant for
    /// `namespace { ... } namespace Pdo { ... }`-shaped files - see
    /// `Registry::index`'s doc comment on why that real shape matters).
    pub(crate) fn set_namespace(&mut self, name: &str) {
        self.namespace = strip_leading_backslash(name);
        self.use_map.clear();
    }

    pub(crate) fn add_uses(&mut self, imports: &[UseImport]) {
        for import in imports {
            let path = strip_leading_backslash(&import.path);
            let bare = path.rsplit('\\').next().unwrap_or(&path).to_string();
            let short_name = import.alias.clone().unwrap_or_else(|| bare.clone());
            self.use_map.insert(short_name, path);
        }
    }

    /// The fully-qualified name a declaration with this bare name gets,
    /// in the current namespace - declarations are never `use`-resolved
    /// (that only applies to *references*), always just namespace-relative.
    pub(crate) fn declared_name(&self, bare: &str) -> String {
        if self.namespace.is_empty() {
            bare.to_string()
        } else {
            format!("{}\\{bare}", self.namespace)
        }
    }

    /// Resolves a *reference* to a class/function name as it appears in
    /// source (`extends Foo`, `new Foo()`, a type hint, ...) to the
    /// fully-qualified name it actually refers to: a leading `\` means
    /// already fully qualified; otherwise the first segment is checked
    /// against this file's `use`-imports, falling back to a
    /// namespace-relative reference (PHP's own resolution order for
    /// unqualified class-shaped names). `self`/`parent`/`static` and
    /// primitive type keywords never reach here - `parse_type_string`
    /// already turns those into a different `Type` variant before this
    /// runs, and `ExprKind::New`'s self/parent/static branch never calls this
    /// either.
    pub(crate) fn resolve(&self, name: &str) -> String {
        if let Some(fq) = name.strip_prefix('\\') {
            return fq.to_string();
        }
        let mut segments = name.splitn(2, '\\');
        let first = segments.next().unwrap_or(name);
        let rest = segments.next();
        if let Some(imported_path) = self.use_map.get(first) {
            return match rest {
                Some(r) => format!("{imported_path}\\{r}"),
                None => imported_path.clone(),
            };
        }
        self.declared_name(name)
    }

    /// Function-call resolution differs from `resolve()` in exactly the
    /// one way real PHP's own does: an unqualified name (no leading/
    /// internal `\`, not matched by a `use function` import) that isn't
    /// declared in the current namespace falls back to the *global*
    /// namespace at runtime - classes have no such fallback, which is why
    /// this is separate from `resolve()` rather than a flag on it.
    /// Returns the primary candidate to look up first, plus a
    /// global-namespace fallback candidate when that rule could apply
    /// (namespaced file, unqualified name, no matching import).
    pub(crate) fn resolve_function_candidates(&self, name: &str) -> (String, Option<String>) {
        let is_unqualified = !name.contains('\\');
        let use_matched = is_unqualified && self.use_map.contains_key(name);
        if !is_unqualified || use_matched || self.namespace.is_empty() {
            (self.resolve(name), None)
        } else {
            (self.declared_name(name), Some(name.to_string()))
        }
    }
}

/// Rewrites every `Object(name)` in a type tree to the fully-qualified
/// name `ctx` resolves it to - mirrors `mark_template_params`'s exact
/// shape (both are a one-time post-processing walk over an already-parsed
/// `Type`, applied right after `parse_type_string` produces it), applied
/// second since a name that turned out to be a `@template` parameter
/// isn't a real class reference to resolve at all.
pub(crate) fn resolve_names_in_type(ty: Type, ctx: &NameContext) -> Type {
    match ty {
        // `object` (the native type-hint keyword, not a real class) is
        // represented as `Type::Object("object")` - `parse_type_string`'s
        // own sentinel for it (see its `"object" => Type::Object(...)`
        // arm) - and is never a genuine reference to resolve: `object` is
        // a PHP-reserved word, so no real class can ever be named that.
        // Without this, a plain `object` type hint got namespace-qualified
        // into nonsense like `App\Some\Namespace\object`.
        Type::Object(name, args) if name == "object" => Type::Object(name, args),
        // Same reasoning as `object` above, for a different reason:
        // `Traversable` here is never something the programmer wrote as
        // a class reference in their own code - it's this project's own
        // internal desugaring of the `iterable` keyword (`array|
        // Traversable`), a fixed, always-global meaning `iterable` itself
        // carries regardless of the current namespace. Without this, a
        // namespaced file's `iterable`-typed parameter got its synthetic
        // `Traversable` half namespace-qualified into nonsense like
        // `App\Traversable`, which no real `Iterator`/`Generator`/
        // `IteratorAggregate` implementation could ever satisfy.
        Type::Object(name, args) if name == "Traversable" => Type::Object(name, args),
        // Same reasoning as `object`/`Traversable` above: `resource` is
        // a sentinel for a real PHP value kind with no actual class
        // behind it (`types::parse_type_keyword_or_class`'s own
        // `"resource" => Type::Object("resource", ...)` arm), not a
        // class reference to resolve - without this, the same pseudo-
        // type used in two different namespaces was treated as two
        // unrelated "classes" (`App\resource` vs. plain `resource`).
        Type::Object(name, args) if name == "resource" => Type::Object(name, args),
        Type::Object(name, args) => Type::Object(
            ctx.resolve(&name),
            args.into_iter()
                .map(|a| resolve_names_in_type(a, ctx))
                .collect(),
        ),
        Type::Array(inner) => Type::Array(Box::new(resolve_names_in_type(*inner, ctx))),
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| resolve_names_in_type(m, ctx))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|m| resolve_names_in_type(m, ctx))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, resolve_names_in_type(t, ctx), opt))
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
    fn from_decl(f: &FunctionDecl, ctx: &NameContext, class_templates: &[String]) -> FunctionSig {
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

#[derive(Debug, Clone)]
pub struct PropertySig {
    pub ty: Type,
    pub is_static: bool,
    pub is_readonly: bool,
}

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub kind: ClassKind,
    pub is_abstract: bool,
    pub is_final: bool,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub methods: HashMap<String, FunctionSig>,
    pub properties: HashMap<String, PropertySig>,
    pub consts: HashMap<String, Type>,
    pub uses: Vec<String>,
    // Method names declared `abstract` in this class's own body (not
    // walked up the hierarchy - a completeness check needs to walk
    // `extends`/`implements` itself and consult each ancestor's own set).
    pub abstract_methods: HashSet<String>,
    // Method names declared `final` in this class's own body.
    pub final_methods: HashSet<String>,
    // This class's own `@template T` names, in declaration order - order
    // matters, since a generic reference (`Collection<User>`) binds its
    // type arguments to these positionally (`checker::
    // class_template_bindings`). Empty for a non-generic class.
    pub templates: Vec<String>,
    // `@extends Parent<Arg, ...>`'s own argument list (Phase 10.5) - fixes
    // `extends.first()`'s own `@template` names for every instance of
    // this class, independent of whether this class is itself generic
    // (e.g. `class IntCollection extends Collection<int> {}`, which isn't
    // generic itself but permanently binds `Collection`'s own `T` to
    // `int`). An argument can itself reference *this* class's own
    // `@template` name (`class Box<T> extends Collection<T> {}`) - such
    // an argument is `Type::TemplateParam(name)`, substituted using
    // whatever this class's own templates are bound to at each use, not
    // a fixed value. Empty when there's no `@extends` tag, or no native
    // `extends` for it to apply to.
    pub extends_args: Vec<Type>,
    // Same idea as `extends_args`, but keyed by which interface each
    // `@implements Iface<Arg, ...>` tag applies to (a class can implement
    // more than one at once) - the key is resolved to the same
    // fully-qualified name `implements` itself uses, so the two agree.
    pub implements_args: HashMap<String, Vec<Type>>,
    // `@template T of Bound` (Phase 10.5's bound-enforcement half) - the
    // resolved bound for whichever of `templates` above declared one.
    // Checked by `checker::check_generic_bounds` wherever this class is
    // referenced with concrete type arguments (`Collection<Foo>`), not
    // just parsed past like it was before this phase.
    pub template_bounds: HashMap<String, Type>,
    // Names from `templates` above additionally declared
    // `@template-covariant`/`@template-contravariant` (Phase 10.5's
    // variance half) - a name absent from both stays invariant.
    pub covariant_templates: HashSet<String>,
    pub contravariant_templates: HashSet<String>,
}

impl ClassInfo {
    fn from_decl(c: &ClassDecl, ctx: &NameContext) -> ClassInfo {
        let class_doc: Option<DocBlock> = c.doc_comment.as_deref().map(docblock::parse);
        let templates: Vec<String> = class_doc
            .as_ref()
            .map(|d| d.templates.clone())
            .unwrap_or_default();
        let template_set: HashSet<String> = templates.iter().cloned().collect();

        let methods: HashMap<String, FunctionSig> = c
            .methods
            .iter()
            .map(|m| {
                (
                    m.function.name.clone(),
                    FunctionSig::from_decl(&m.function, ctx, &templates),
                )
            })
            .collect();
        let abstract_methods: HashSet<String> = c
            .methods
            .iter()
            .filter(|m| m.is_abstract)
            .map(|m| m.function.name.clone())
            .collect();
        let final_methods: HashSet<String> = c
            .methods
            .iter()
            .filter(|m| m.is_final)
            .map(|m| m.function.name.clone())
            .collect();

        let mut properties: HashMap<String, PropertySig> = c
            .properties
            .iter()
            .map(|p| {
                let doc_ty = p.doc_comment.as_deref().map(docblock::parse);
                let ty = resolve_type(
                    doc_ty.as_ref().and_then(|d| d.var_type.as_deref()),
                    p.type_hint.as_deref(),
                );
                let ty = mark_template_params(ty, &template_set);
                let ty = resolve_names_in_type(ty, ctx);
                (
                    p.name.clone(),
                    PropertySig {
                        ty,
                        is_static: p.is_static,
                        is_readonly: p.is_readonly,
                    },
                )
            })
            .collect();

        // Constructor property promotion: `public function __construct(
        // private int $id) {}` declares `$id` as a property, not just a
        // parameter - the parser records this via Param::promoted_visibility.
        // A `@param` on the constructor's own docblock describes the same
        // promoted property, so it gets the same PHPDoc-wins treatment.
        if let Some(ctor) = c.methods.iter().find(|m| m.function.name == "__construct") {
            let ctor_doc = ctor.function.doc_comment.as_deref().map(docblock::parse);
            for p in &ctor.function.params {
                if p.promoted_visibility.is_none() {
                    continue;
                }
                let doc_ty = ctor_doc.as_ref().and_then(|d| d.params.get(&p.name));
                let ty = resolve_type(doc_ty.map(String::as_str), p.type_hint.as_deref());
                let ty = mark_template_params(ty, &template_set);
                let ty = resolve_names_in_type(ty, ctx);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                properties.insert(
                    p.name.clone(),
                    PropertySig {
                        ty,
                        is_static: false,
                        is_readonly: p.promoted_readonly,
                    },
                );
            }
        }

        let consts = c
            .consts
            .iter()
            .map(|cc| (cc.name.clone(), infer_const_type(&cc.value)))
            .collect();

        // `@extends`/`@implements` generic args (Phase 10.5) - an
        // argument can reference *this* class's own `@template` name
        // (`class Box<T> extends Collection<T> {}`), so it goes through
        // the same mark-then-resolve pipeline as every other member type
        // above, not just a bare `types::parse_type_string`.
        let resolve_generic_tag = |raw: &str| -> Type {
            let ty = types::parse_type_string(raw);
            let ty = mark_template_params(ty, &template_set);
            resolve_names_in_type(ty, ctx)
        };
        let extends_args: Vec<Type> = class_doc
            .as_ref()
            .and_then(|d| d.extends.as_deref())
            .map(|raw| match resolve_generic_tag(raw) {
                Type::Object(_, args) => args,
                _ => Vec::new(),
            })
            .unwrap_or_default();
        let implements_args: HashMap<String, Vec<Type>> = class_doc
            .iter()
            .flat_map(|d| &d.implements)
            .filter_map(|raw| match resolve_generic_tag(raw) {
                Type::Object(name, args) => Some((name, args)),
                _ => None,
            })
            .collect();
        let template_bounds: HashMap<String, Type> = class_doc
            .as_ref()
            .map(|d| {
                d.template_bounds
                    .iter()
                    .map(|(name, raw)| (name.clone(), resolve_generic_tag(raw)))
                    .collect()
            })
            .unwrap_or_default();
        let covariant_templates = class_doc
            .as_ref()
            .map(|d| d.covariant_templates.clone())
            .unwrap_or_default();
        let contravariant_templates = class_doc
            .as_ref()
            .map(|d| d.contravariant_templates.clone())
            .unwrap_or_default();

        // Real PHP (8.0+) makes any class declaring a public
        // `__toString()` automatically implement `Stringable`, with no
        // `implements Stringable` needed - added specifically so the
        // pre-existing body of `__toString()`-bearing classes would work
        // seamlessly with the `string|Stringable` idiom without
        // modification. Same shape as the enum UnitEnum/BackedEnum
        // implicit-interface handling below, for a different implicit
        // interface.
        let mut implements: Vec<String> = c.implements.iter().map(|s| ctx.resolve(s)).collect();
        if methods.contains_key("__toString") {
            implements.push("Stringable".to_string());
        }

        ClassInfo {
            kind: c.kind,
            is_abstract: c.is_abstract,
            is_final: c.is_final,
            extends: c.extends.iter().map(|s| ctx.resolve(s)).collect(),
            implements,
            methods,
            properties,
            consts,
            uses: c.uses.iter().map(|s| ctx.resolve(s)).collect(),
            abstract_methods,
            final_methods,
            templates,
            extends_args,
            implements_args,
            template_bounds,
            covariant_templates,
            contravariant_templates,
        }
    }

    /// An enum participates in the class hierarchy the same way a class
    /// does (methods, `implements`, `use`) - it just can't extend anything
    /// or declare instance properties. Cases aren't registered as anything
    /// yet: resolving `Suit::Hearts`'s type would need static-access type
    /// inference, which nothing in the checker does for *any* class today,
    /// not just enums - a separate, pre-existing gap this doesn't attempt
    /// to close.
    fn from_enum_decl(e: &EnumDecl, ctx: &NameContext) -> ClassInfo {
        // Enums can't be generic in real PHP (no `@template` on an enum
        // means anything), so there's no enclosing-class template list to
        // pass down here, unlike ClassInfo::from_decl.
        let fqn = ctx.declared_name(&e.name);
        let mut methods: HashMap<String, FunctionSig> = e
            .methods
            .iter()
            .map(|m| {
                (
                    m.function.name.clone(),
                    FunctionSig::from_decl(&m.function, ctx, &[]),
                )
            })
            .collect();
        // `cases(): array` is a real, built-in static method every enum
        // gets from the language itself (`UnitEnum`), and a backed
        // enum's `from(int|string $value): static` /
        // `tryFrom(...): ?static` (`BackedEnum`) likewise - none
        // declared in source. Found the hard way, same as the
        // ->name/->value properties below: undefined-static-method
        // detection immediately flagged every single real
        // `Enum::cases()`/`::tryFrom()` call in the wild.
        let self_ty = Type::Object(fqn.clone(), Vec::new());
        methods.insert(
            "cases".to_string(),
            FunctionSig {
                params: Vec::new(),
                return_type: Type::Array(Box::new(self_ty.clone())),
            },
        );
        if let Some(backing) = &e.backing_type {
            let backing_ty = types::parse_type_string(backing);
            let value_param = ParamSig {
                name: "value".to_string(),
                ty: backing_ty,
                variadic: false,
                has_default: false,
                by_ref: false,
            };
            methods.insert(
                "from".to_string(),
                FunctionSig {
                    params: vec![value_param.clone()],
                    return_type: self_ty.clone(),
                },
            );
            methods.insert(
                "tryFrom".to_string(),
                FunctionSig {
                    params: vec![value_param],
                    return_type: Type::Union(vec![self_ty.clone(), Type::Null]),
                },
            );
        }
        let mut consts: HashMap<String, Type> = e
            .consts
            .iter()
            .map(|cc| (cc.name.clone(), infer_const_type(&cc.value)))
            .collect();
        // A case is accessed exactly like a constant (`Suit::Hearts`,
        // same `ClassName::member` syntax as `Suit::SOME_CONST`) and has
        // a real, useful static type: the enum itself. Folding it into
        // `consts` (rather than a separate field) means the checker's
        // static-access existence check and its type inference both
        // just work, with no new lookup path needed - previously cases
        // weren't registered as anything at all, so `Suit::Hearts`
        // always read as Mixed and a genuinely undefined case name was
        // never caught.
        for case in &e.cases {
            consts.insert(case.name.clone(), Type::Object(fqn.clone(), Vec::new()));
        }

        // Every enum case is a real object with a built-in `->name`
        // (its case name, as a string) - and a backed enum's case
        // additionally has a real `->value` typed to its backing type -
        // neither declared in source, both provided by the language
        // itself (`UnitEnum`/`BackedEnum`). Found the hard way: adding
        // undefined-property-on-read detection immediately produced a
        // false positive on every single real `$case->value` access in
        // the wild, since nothing registered these as properties at all.
        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            PropertySig {
                ty: Type::String,
                is_static: false,
                is_readonly: false,
            },
        );
        if let Some(backing) = &e.backing_type {
            properties.insert(
                "value".to_string(),
                PropertySig {
                    ty: types::parse_type_string(backing),
                    is_static: false,
                    is_readonly: false,
                },
            );
        }

        // Every enum implements `UnitEnum` (and a backed one also
        // `BackedEnum`) by virtue of being an enum at all - never
        // declared in source, the same as its synthesized `cases()`/
        // `name`/`value` above, but this time needed for hierarchy
        // checks (`is_subclass_of`/`is_assignable`), not member lookup.
        // Found the hard way: real code narrowing/typing an enum against
        // `UnitEnum` (e.g. doctrine/lexer's own `AbstractLexer<T of
        // UnitEnum|string|int>`) was flagged as a bound violation despite
        // being exactly what real PHP guarantees.
        let mut implements: Vec<String> = e.implements.iter().map(|s| ctx.resolve(s)).collect();
        implements.push("UnitEnum".to_string());
        if e.backing_type.is_some() {
            implements.push("BackedEnum".to_string());
        }

        let abstract_methods: HashSet<String> = e
            .methods
            .iter()
            .filter(|m| m.is_abstract)
            .map(|m| m.function.name.clone())
            .collect();
        let final_methods: HashSet<String> = e
            .methods
            .iter()
            .filter(|m| m.is_final)
            .map(|m| m.function.name.clone())
            .collect();

        ClassInfo {
            kind: ClassKind::Enum,
            is_abstract: false,
            is_final: true,
            extends: Vec::new(),
            implements,
            methods,
            properties,
            consts,
            uses: e.uses.iter().map(|s| ctx.resolve(s)).collect(),
            abstract_methods,
            final_methods,
            templates: Vec::new(),
            extends_args: Vec::new(),
            implements_args: HashMap::new(),
            template_bounds: HashMap::new(),
            covariant_templates: HashSet::new(),
            contravariant_templates: HashSet::new(),
        }
    }
}

/// A symbol table built by indexing already-parsed source, standing in for
/// the reflection a real PHP runtime would normally provide (see the
/// discussion this project started from: no live interpreter means no
/// `ReflectionClass`, so class/function signatures come from parsing
/// everything up front instead). `merge_stubs` layers in curated built-in
/// signatures (`stubs.rs`) the same way php-src stubs would sit alongside
/// project code in a real implementation.
#[derive(Debug, Default)]
pub struct Registry {
    pub classes: HashMap<String, ClassInfo>,
    pub functions: HashMap<String, FunctionSig>,
    // Phase 11 Stage 5's strictness-parameter half (real PHPStan's own
    // 7-10-style progressively-stricter handling, here narrowed to the
    // one real leniency `is_assignable` has purely on its own: an
    // unknown array literal's type, `Array(Mixed)`, freely satisfying any
    // specific array/shape type it's checked against). Defaults to
    // `false` (today's existing permissive behavior, unchanged) via
    // `#[derive(Default)]` - a plain `pub` field rather than a
    // constructor parameter, since every one of this struct's 10
    // existing construction sites already goes through `build`/
    // `default()` and none of them need to change to keep compiling.
    pub strict_mixed_arrays: bool,
}

impl Registry {
    pub fn build(program: &[Stmt]) -> Registry {
        let mut registry = Registry::default();
        registry.index(program);
        registry.flatten_traits();
        registry
    }

    /// A `use TraitName;` can reference a trait declared later in the same
    /// file (PHP resolves this at compile time, not top-to-bottom), so
    /// flattening has to be a second pass over the fully-indexed classes -
    /// a using class's own methods/properties win over a trait's.
    ///
    /// `pub(crate)` (not private) so `Project::update_file` can re-run
    /// just this pass after patching one file's contributions in place,
    /// without needing to go through `build`'s full from-scratch `index`.
    pub(crate) fn flatten_traits(&mut self) {
        type TraitAddition = (
            String,
            Vec<(String, FunctionSig)>,
            Vec<(String, PropertySig)>,
            HashSet<String>,
        );

        let class_names: Vec<String> = self.classes.keys().cloned().collect();
        let mut additions: Vec<TraitAddition> = Vec::new();

        for class_name in &class_names {
            let uses = self.classes[class_name].uses.clone();
            if uses.is_empty() {
                continue;
            }
            // A trait can declare its own `abstract` method (the using
            // class is required to supply it) - a using class's own
            // directly-declared method for that same name (checked here,
            // before any trait's contribution is applied) always wins,
            // matching the same `or_insert` "first/own wins" semantics
            // `methods` itself already uses below.
            let own_method_names: HashSet<String> =
                self.classes[class_name].methods.keys().cloned().collect();
            let mut methods = Vec::new();
            let mut properties = Vec::new();
            let mut abstract_from_traits: HashSet<String> = HashSet::new();
            for trait_name in &uses {
                if let Some(trait_info) = self.classes.get(trait_name) {
                    for (name, sig) in &trait_info.methods {
                        methods.push((name.clone(), sig.clone()));
                        if trait_info.abstract_methods.contains(name)
                            && !own_method_names.contains(name)
                        {
                            abstract_from_traits.insert(name.clone());
                        }
                    }
                    properties.extend(
                        trait_info
                            .properties
                            .iter()
                            .map(|(n, s)| (n.clone(), s.clone())),
                    );
                }
            }
            additions.push((
                class_name.clone(),
                methods,
                properties,
                abstract_from_traits,
            ));
        }

        for (class_name, methods, properties, abstract_from_traits) in additions {
            if let Some(info) = self.classes.get_mut(&class_name) {
                for (name, sig) in methods {
                    info.methods.entry(name).or_insert(sig);
                }
                for (name, sig) in properties {
                    info.properties.entry(name).or_insert(sig);
                }
                info.abstract_methods.extend(abstract_from_traits);
            }
        }
    }

    /// `pub(crate)` for the same reason as `flatten_traits` - lets
    /// `Project` index one file's statements into an already-built
    /// registry (initial load, or after a re-parse) without re-running
    /// `build`'s from-scratch pass over every other file.
    /// Indexes a file's declarations, tracking the current
    /// `NameContext` *positionally* as it walks - a `namespace` statement
    /// changes it for everything from that point on, exactly like real
    /// PHP, rather than computing one static context up front for the
    /// whole file. This matters concretely: the parser has no real model
    /// of brace-delimited `namespace X { ... }` (see `StmtKind::Namespace`'s
    /// own parsing comment), so it falls out as a `StmtKind::Namespace(name)`
    /// immediately followed by a sibling `StmtKind::Block` holding what was
    /// inside the braces - and real vendored stub files (phpstorm-stubs'
    /// own `PDO/PDO.php`) really do have *two* such blocks in one file
    /// (`namespace { ... }` for the global `PDO` class, then a later
    /// `namespace Pdo { ... }` for PHP 8.4's `Pdo\*` subclasses) - a
    /// single whole-file context would apply whichever one it saw last to
    /// both blocks, silently mis-qualifying the first one's classes.
    pub(crate) fn index(&mut self, stmts: &[Stmt]) {
        let mut ctx = NameContext::default();
        self.index_with_context(stmts, &mut ctx);
    }

    fn index_with_context(&mut self, stmts: &[Stmt], ctx: &mut NameContext) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Namespace(name) => ctx.set_namespace(name),
                StmtKind::Use(imports) => ctx.add_uses(imports),
                StmtKind::ClassDecl(c) => {
                    self.classes
                        .insert(ctx.declared_name(&c.name), ClassInfo::from_decl(c, ctx));
                }
                StmtKind::EnumDecl(e) => {
                    self.classes.insert(
                        ctx.declared_name(&e.name),
                        ClassInfo::from_enum_decl(e, ctx),
                    );
                }
                StmtKind::FunctionDecl(f) => {
                    self.functions.insert(
                        ctx.declared_name(&f.name),
                        FunctionSig::from_decl(f, ctx, &[]),
                    );
                }
                // The real, common `if (!function_exists('foo')) { function
                // foo() {} }` polyfill idiom (used throughout the PHP
                // ecosystem, e.g. symfony/deprecation-contracts and every
                // symfony/polyfill-* package) declares a genuinely global
                // function/class from inside a conditional - recursing
                // here is what makes those visible at all. This registry
                // has no notion of "conditionally declared" in the first
                // place (a symbol is either indexed or it isn't), so
                // treating a branch's declarations the same as top-level
                // ones is consistent with that existing coarseness, not a
                // new kind of imprecision. Cloning `ctx` for each branch
                // means a `use`/`namespace` change inside one doesn't leak
                // into the other or past the `if` - conservative, and
                // irrelevant in practice (real code doesn't declare
                // namespaces inside an `if`).
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.index_with_context(std::slice::from_ref(then_branch), &mut ctx.clone());
                    if let Some(else_branch) = else_branch {
                        self.index_with_context(
                            std::slice::from_ref(else_branch),
                            &mut ctx.clone(),
                        );
                    }
                }
                // Unlike `If`, deliberately *not* cloned: this is the
                // brace-namespace body itself (see this method's doc
                // comment), so a `use` statement inside it must be
                // visible to the declarations that follow it in the very
                // same block - it's the statement *after* the block ends
                // that must not see it, which cloning at the point the
                // *next* `StmtKind::Namespace` resets `ctx.use_map` already
                // ensures (see `set_namespace`).
                StmtKind::Block(inner) => self.index_with_context(inner, ctx),
                _ => {}
            }
        }
    }

    /// Surgically removes one file's prior contributions before
    /// re-indexing its new parse - `Project::update_file`'s whole reason
    /// for existing. `functions`/`classes` must already be the
    /// fully-qualified names `Registry::index` actually keyed its entries
    /// under (Phase 9.5 - see `NameContext`) - `project.rs`'s
    /// `declared_names` mirrors `index`'s own namespace-tracking exactly
    /// so the two agree. Two files legitimately declaring the exact same
    /// fully-qualified name (verified at real scale: unrelated packages
    /// bundling their own non-scoped copy of a third dependency) still
    /// collide here, the same way real PHP's own class-name uniqueness
    /// requirement would refuse to load both - a real limit on how many
    /// "same file, different builds" scenarios this can represent, not a
    /// namespace-modeling gap.
    pub(crate) fn remove_declarations(&mut self, functions: &[String], classes: &[String]) {
        for name in functions {
            self.functions.remove(name);
        }
        for name in classes {
            self.classes.remove(name);
        }
    }

    /// User-defined symbols always win: a stub only fills in a name that
    /// isn't already declared in the indexed source.
    pub fn merge_stubs<S: Into<String>>(&mut self, stubs: Vec<(S, FunctionSig)>) {
        for (name, sig) in stubs {
            self.functions.entry(name.into()).or_insert(sig);
        }
    }

    /// Same "user-defined wins" rule as `merge_stubs`, for classes -
    /// without this, built-in classes (`Exception`, `ArrayObject`, ...)
    /// would never be known to the registry at all, since stub sources
    /// were previously only mined for function signatures.
    pub fn merge_stub_classes<S: Into<String>>(&mut self, stubs: Vec<(S, ClassInfo)>) {
        for (name, info) in stubs {
            self.classes.entry(name.into()).or_insert(info);
        }
    }

    /// Does `class` derive from or implement `of`, directly or transitively?
    /// Walks `extends` and `implements` together as one graph, which also
    /// covers an interface extending several other interfaces.
    pub fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        if class == of {
            return true;
        }
        let mut seen = HashSet::new();
        let mut stack = vec![class.to_string()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            for parent in info.extends.iter().chain(info.implements.iter()) {
                if parent == of {
                    return true;
                }
                stack.push(parent.clone());
            }
        }
        false
    }

    /// Walks `class`'s own `extends` (*and* `implements`, so an
    /// interface-typed receiver's own multi-parent `extends` chain and a
    /// class's declared-but-not-overridden interface methods both
    /// resolve) ancestor graph, in the same order `is_subclass_of` does,
    /// returning the first ancestor (closest first) whose `ClassInfo`
    /// satisfies `get`. A single-parent-only walk (`extends.first()`) is
    /// wrong for interfaces specifically - unlike a class, PHP lets an
    /// interface `extends` more than one parent at once (real example
    /// that surfaced this: league/flysystem's `FilesystemOperator extends
    /// FilesystemReader, FilesystemWriter`) - so every lookup below needs
    /// this, not just `is_subclass_of`.
    fn lookup_in_hierarchy<'r, T>(
        &'r self,
        class: &str,
        get: impl Fn(&'r ClassInfo) -> Option<T>,
    ) -> Option<T> {
        self.lookup_in_hierarchy_with_owner(class, get)
            .map(|(_, found)| found)
    }

    /// Same walk as `lookup_in_hierarchy`, but also returns *which*
    /// ancestor (closest first, `class` itself included) actually
    /// satisfied `get` - needed wherever a caller goes on to do
    /// class-level generic substitution (Phase 10.5's `checker::
    /// class_template_bindings`), since a `@template` name is scoped to
    /// the class that declares it, not globally unique - substituting an
    /// inherited method/property's `TemplateParam`s correctly requires
    /// knowing which ancestor's own template scope they belong to, not
    /// just the value found.
    fn lookup_in_hierarchy_with_owner<'r, T>(
        &'r self,
        class: &str,
        get: impl Fn(&'r ClassInfo) -> Option<T>,
    ) -> Option<(String, T)> {
        let mut seen = HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(class.to_string());
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            if let Some(found) = get(info) {
                return Some((current, found));
            }
            for parent in info.extends.iter().chain(info.implements.iter()) {
                queue.push_back(parent.clone());
            }
        }
        None
    }

    /// Looks up a method, walking the extends/implements ancestor graph
    /// if not declared directly on `class`. Traits aren't modeled here as
    /// a distinct concept - `flatten_traits` already merges a trait's
    /// members directly into the using class's own `methods` map, so they
    /// show up as if declared there.
    pub fn lookup_method(&self, class: &str, method: &str) -> Option<&FunctionSig> {
        self.lookup_in_hierarchy(class, |info| info.methods.get(method))
    }

    /// Same as `lookup_method`, but also returns the declaring ancestor's
    /// name - see `lookup_in_hierarchy_with_owner`'s doc comment for why.
    pub fn lookup_method_with_owner(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(String, &FunctionSig)> {
        self.lookup_in_hierarchy_with_owner(class, |info| info.methods.get(method))
    }

    /// Same as `lookup_method_with_owner`, but starts the walk at
    /// `class`'s own parents rather than `class` itself - used for
    /// override-compatibility checking (Finding 50), which needs the
    /// signature a method is overriding, not the overriding one itself.
    pub fn lookup_ancestor_method_with_owner(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(String, &FunctionSig)> {
        let info = self.classes.get(class)?;
        for parent in info.extends.iter().chain(info.implements.iter()) {
            if let Some(found) = self.lookup_method_with_owner(parent, method) {
                return Some(found);
            }
        }
        None
    }

    /// Every method name `class` must provide a concrete (non-abstract)
    /// implementation of somewhere in its own hierarchy: every abstract
    /// method declared by an ancestor class, every method declared by an
    /// ancestor interface (interface methods have no body at all, so
    /// every one of them is implicitly abstract - real PHP doesn't
    /// require/allow the `abstract` keyword there), and every abstract
    /// method declared by a trait any of those classes `use`s -
    /// `flatten_traits` already folds a trait's own `abstract_methods`
    /// into each using class's own set (unless the using class supplies
    /// its own concrete override), so a plain walk of `abstract_methods`
    /// here already covers it, with no separate trait-specific case
    /// needed.
    pub fn required_abstract_methods(&self, class: &str) -> Vec<String> {
        let mut required = HashSet::new();
        let mut seen = HashSet::new();
        let mut stack = vec![class.to_string()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            if info.kind == ClassKind::Interface {
                required.extend(info.methods.keys().cloned());
            } else {
                required.extend(info.abstract_methods.iter().cloned());
            }
            for parent in info.extends.iter().chain(info.implements.iter()) {
                stack.push(parent.clone());
            }
        }
        let mut required: Vec<String> = required.into_iter().collect();
        required.sort();
        required
    }

    /// A required method (see `required_abstract_methods`) counts as
    /// implemented only if *some* reachable ancestor provides a concrete
    /// one - not necessarily the *closest* ancestor with any entry at
    /// all. Deliberately doesn't reuse `lookup_method_with_owner`'s
    /// closest-match BFS: a class can `implements Throwable` directly
    /// (graph-adjacent) while its real concrete `getCode()` etc. only
    /// live two hops further up an `extends` chain (e.g. `BaseException`
    /// -> real `Exception`) - the closest *node* with any entry for the
    /// name is the interface's own bodyless declaration, even though a
    /// real implementation is reachable elsewhere. So this walks the
    /// whole ancestor graph looking for any concrete match, rather than
    /// stopping at the first node that merely mentions the name at all.
    pub fn has_concrete_implementation(&self, class: &str, method: &str) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![class.to_string()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(info) = self.classes.get(&current) else {
                continue;
            };
            if info.kind != ClassKind::Interface
                && info.methods.contains_key(method)
                && !info.abstract_methods.contains(method)
            {
                return true;
            }
            for parent in info.extends.iter().chain(info.implements.iter()) {
                stack.push(parent.clone());
            }
        }
        false
    }

    /// Looks up a property, walking the ancestor graph if not declared
    /// directly on `class` (covers inherited properties, including ones
    /// introduced via constructor promotion).
    pub fn lookup_property(&self, class: &str, prop: &str) -> Option<&PropertySig> {
        self.lookup_in_hierarchy(class, |info| info.properties.get(prop))
    }

    /// Same as `lookup_property`, but also returns the declaring
    /// ancestor's name - see `lookup_in_hierarchy_with_owner`'s doc
    /// comment for why.
    pub fn lookup_property_with_owner(
        &self,
        class: &str,
        prop: &str,
    ) -> Option<(String, &PropertySig)> {
        self.lookup_in_hierarchy_with_owner(class, |info| info.properties.get(prop))
    }

    /// Looks up a class constant, walking the ancestor graph if not
    /// declared directly on `class` - mirrors `lookup_property`.
    pub fn lookup_const(&self, class: &str, const_name: &str) -> Option<&Type> {
        self.lookup_in_hierarchy(class, |info| info.consts.get(const_name))
    }

    pub fn lookup_function(&self, name: &str) -> Option<&FunctionSig> {
        self.functions.get(name)
    }

    /// Type-compatibility check that consults the class hierarchy for
    /// `Object` types; every other case defers to `types::is_subtype`,
    /// which has no notion of class context.
    pub fn is_assignable(&self, sub: &Type, sup: &Type) -> bool {
        match (sub, sup) {
            // Type arguments are deliberately ignored here (not checked
            // for equality/variance) - this project doesn't model
            // variance at all (see `Type::Object`'s own doc comment), and
            // guessing wrong in either direction is worse than just not
            // checking: too strict flags real code passing a subtype's
            // narrower generic argument, too loose silently accepts a
            // genuine mismatch either way once real variance rules would
            // matter. Matches this checker's general preference for fewer
            // false positives over catching everything.
            // The native `object` type hint (`Type::Object("object", _)` -
            // `types::parse_type_string`'s own sentinel for it, since
            // `object` is a reserved word no real class can ever be named)
            // is satisfied by *any* class instance - a real, pre-existing
            // gap this had never hit before Phase 10.5's bound-checking
            // started comparing against it for the first time (a real,
            // extremely common pattern: Doctrine's `EntityRepository
            // <T of object>`). Confirmed live: a plain native `function
            // f(): object { return new Foo(); }` was already
            // incorrectly flagged before this fix, unrelated to generics
            // at all.
            (Type::Object(_, _), Type::Object(b, _)) if b == "object" => true,
            // Real PHP recognizes (at least) four distinct shapes as a
            // valid `callable`: an actual `Closure` (already handled by
            // the ordinary class-hierarchy arm just below, since
            // `Closure` is now its own real `Object` type rather than
            // being conflated with `Type::Callable` itself), an object
            // whose class declares `__invoke()`, a plain string naming a
            // real function, and a two-element array naming an instance-
            // or static-method callable. The latter two can't be
            // statically verified further without evaluating the string/
            // array's own literal content - accepting them outright
            // matches this project's existing "don't guess, don't
            // false-positive" leniency elsewhere (e.g. the `Array(Mixed)`
            // leniency just below).
            (Type::Object(class, _), Type::Callable) => {
                self.lookup_method(class, "__invoke").is_some()
            }
            (Type::String | Type::StringLiteral(_), Type::Callable) => true,
            (Type::Array(_), Type::Callable) => true,
            (Type::Object(a, _), Type::Object(b, _)) => a == b || self.is_subclass_of(a, b),
            (Type::Union(members), _) => members.iter().all(|m| self.is_assignable(m, sup)),
            (_, Type::Union(members)) => members.iter().any(|m| self.is_assignable(sub, m)),
            (Type::Intersection(members), _) => members.iter().any(|m| self.is_assignable(m, sup)),
            (_, Type::Intersection(members)) => members.iter().all(|m| self.is_assignable(sub, m)),
            // Deliberately more lenient here than types::is_subtype's pure
            // structural relation (which correctly treats Array(Mixed) as
            // NOT a subtype of Array(Int)): an array literal's element type
            // is never inferred precisely by this checker, so it always
            // shows up as Array(Mixed) meaning "unknown", not "really can
            // hold anything" - without this, e.g. a PHPDoc `int[]` param
            // would false-positive against every `[1, 2, 3]` literal.
            // `strict_mixed_arrays` (Phase 11 Stage 5) opts out of this
            // leniency, matching real PHPStan's own level-9-style "be
            // strict about values with mixed type" philosophy.
            (Type::Array(a), Type::Array(b)) => {
                (!self.strict_mixed_arrays && matches!(**a, Type::Mixed))
                    || self.is_assignable(a, b)
            }
            // Same leniency, same reason, for an array of unknown structure
            // (an array literal, most commonly) checked against a specific
            // expected shape: e.g. `return ["count" => ..., "first" => ...]`
            // against `@return array{count: int, first: string}` shouldn't
            // false-positive just because literal keys/values aren't
            // inferred. A genuinely known, homogeneous Array(V) still
            // correctly fails to satisfy an arbitrary shape.
            (Type::Array(a), Type::ArrayShape(_)) => {
                !self.strict_mixed_arrays && matches!(**a, Type::Mixed)
            }
            _ => types::is_subtype(sub, sup),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn build_registry(src: &str) -> Registry {
        let tokens = Lexer::new(src).tokenize_with_spans();
        let program = Parser::new(tokens).parse_program();
        Registry::build(&program)
    }

    #[test]
    fn function_declared_inside_an_if_guard_is_still_indexed() {
        let r = build_registry(
            r#"<?php
            if (!function_exists('polyfilled')) {
                function polyfilled() {}
            }"#,
        );
        assert!(r.lookup_function("polyfilled").is_some());
    }

    #[test]
    fn class_info_records_its_own_kind_and_final_abstract_readonly_members() {
        let r = build_registry(
            r#"<?php
            interface Iface {}
            abstract class Base {
                abstract public function required(): void;
                final public function locked(): void {}
                public readonly int $id;
            }
            "#,
        );
        let iface = r.classes.get("Iface").unwrap();
        assert_eq!(iface.kind, ClassKind::Interface);

        let base = r.classes.get("Base").unwrap();
        assert_eq!(base.kind, ClassKind::Class);
        assert!(base.is_abstract);
        assert!(!base.is_final);
        assert!(base.abstract_methods.contains("required"));
        assert!(base.final_methods.contains("locked"));
        assert!(base.properties.get("id").unwrap().is_readonly);
    }

    #[test]
    fn class_declared_inside_an_if_else_guard_is_still_indexed() {
        let r = build_registry(
            r#"<?php
            if (PHP_VERSION_ID >= 80100) {
                class Widget {}
            } else {
                class OldWidget {}
            }"#,
        );
        assert!(r.classes.contains_key("Widget"));
        assert!(r.classes.contains_key("OldWidget"));
    }

    #[test]
    fn the_object_type_hint_is_not_namespace_qualified() {
        // `object` is a PHP-reserved word - no real class can ever be
        // named that, so `Type::Object("object")` (parse_type_string's
        // own sentinel for the native `object` type hint) must never be
        // resolved as if it were a genuine class reference.
        let r = build_registry(
            r#"<?php
            namespace App\Some\Deep\Namespace_;
            class Foo {
                public function bar(): object { return new \stdClass(); }
            }"#,
        );
        let sig = r
            .lookup_method("App\\Some\\Deep\\Namespace_\\Foo", "bar")
            .unwrap();
        assert_eq!(
            sig.return_type,
            Type::Object("object".to_string(), Vec::new())
        );
    }

    #[test]
    fn a_leading_backslash_in_a_native_type_hint_is_not_re_namespaced() {
        // Real, previously-live bug found via real code in this
        // project's own verification corpus (a `Doctrine\ORM\QueryBuilder`
        // union member written as `\Doctrine\ORM\QueryBuilder`, i.e.
        // "absolute, from the global namespace" - real PHP syntax): the
        // leading `\` used to be stripped before `resolve_names_in_type`
        // ever saw it (inside `types::parse_type_atom`, purely as part of
        // computing a bare name for keyword matching), losing the "already
        // fully qualified, don't touch" signal - every absolute reference
        // containing a `\` then got silently mis-qualified as if it were
        // namespace-relative (`App\Doctrine\ORM\QueryBuilder` instead of
        // the real `Doctrine\ORM\QueryBuilder`).
        let r = build_registry(
            r#"<?php
            namespace App;
            class Foo {
                public function bar(\Doctrine\ORM\QueryBuilder $qb): void {}
            }"#,
        );
        let sig = r.lookup_method("App\\Foo", "bar").unwrap();
        assert_eq!(
            sig.params[0].ty,
            Type::Object("Doctrine\\ORM\\QueryBuilder".to_string(), Vec::new())
        );
    }

    #[test]
    fn class_consts_are_registered_and_typed_from_their_literal_value() {
        let r = build_registry(
            r#"<?php
            class Config {
                const NAME = "app";
                const MAX = 10;
            }"#,
        );
        assert_eq!(
            r.lookup_const("Config", "NAME"),
            Some(&Type::StringLiteral("app".to_string()))
        );
        assert_eq!(r.lookup_const("Config", "MAX"), Some(&Type::IntLiteral(10)));
        assert_eq!(r.lookup_const("Config", "MISSING"), None);
    }

    #[test]
    fn class_consts_are_inherited_through_extends() {
        let r = build_registry(
            r#"<?php
            class Base { const KIND = "base"; }
            class Derived extends Base {}"#,
        );
        assert_eq!(
            r.lookup_const("Derived", "KIND"),
            Some(&Type::StringLiteral("base".to_string()))
        );
    }

    #[test]
    fn enum_consts_are_registered_too() {
        let r = build_registry(
            r#"<?php
            enum Suit {
                case Hearts;
                const KIND = "hearts";
            }"#,
        );
        assert_eq!(
            r.lookup_const("Suit", "KIND"),
            Some(&Type::StringLiteral("hearts".to_string()))
        );
    }

    #[test]
    fn transitive_class_inheritance() {
        let r = build_registry(
            r#"<?php
            interface Shape {}
            abstract class BaseShape implements Shape {}
            class Circle extends BaseShape {}
        "#,
        );
        assert!(r.is_subclass_of("Circle", "BaseShape"));
        assert!(r.is_subclass_of("Circle", "Shape"));
        assert!(!r.is_subclass_of("BaseShape", "Circle"));
    }

    #[test]
    fn interface_extending_multiple_interfaces() {
        let r = build_registry(
            r#"<?php
            interface A {}
            interface B {}
            interface C extends A, B {}
            class Impl implements C {}
        "#,
        );
        assert!(r.is_subclass_of("Impl", "A"));
        assert!(r.is_subclass_of("Impl", "B"));
    }

    #[test]
    fn method_lookup_walks_up_extends_chain() {
        let r = build_registry(
            r#"<?php
            class Base { public function greet(): string { return "hi"; } }
            class Sub extends Base {}
        "#,
        );
        let sig = r.lookup_method("Sub", "greet").expect("inherited method");
        assert_eq!(sig.return_type.to_string(), "string");
    }

    #[test]
    fn merge_stubs_does_not_override_user_functions() {
        let mut r = build_registry(
            r#"<?php
            function strlen(string $s): int { return 0; }
        "#,
        );
        r.merge_stubs(vec![(
            "strlen",
            FunctionSig {
                params: vec![],
                return_type: Type::Bool,
            },
        )]);
        assert_eq!(r.functions["strlen"].return_type, Type::Int);
    }

    #[test]
    fn is_assignable_uses_class_hierarchy() {
        let r = build_registry(
            r#"<?php
            interface Shape {}
            class Circle implements Shape {}
        "#,
        );
        assert!(r.is_assignable(
            &Type::Object("Circle".to_string(), Vec::new()),
            &Type::Object("Shape".to_string(), Vec::new())
        ));
        assert!(!r.is_assignable(
            &Type::Object("Shape".to_string(), Vec::new()),
            &Type::Object("Circle".to_string(), Vec::new())
        ));
    }

    #[test]
    fn any_class_instance_is_assignable_to_the_native_object_type() {
        let r = build_registry("<?php class Foo {}");
        assert!(r.is_assignable(
            &Type::Object("Foo".to_string(), Vec::new()),
            &Type::Object("object".to_string(), Vec::new())
        ));
        // Not the other way around - a plain `object` isn't known to be
        // any particular class.
        assert!(!r.is_assignable(
            &Type::Object("object".to_string(), Vec::new()),
            &Type::Object("Foo".to_string(), Vec::new())
        ));
    }

    #[test]
    fn is_assignable_treats_unknown_array_element_type_as_compatible() {
        let r = Registry::default();
        let unknown_elements = Type::Array(Box::new(Type::Mixed));
        let ints = Type::Array(Box::new(Type::Int));
        // The lenient direction that types::is_subtype deliberately does
        // NOT take (see its own array-covariance test) - is_assignable is
        // the practical, checker-facing layer where "we don't know" must
        // not read as "definitely incompatible".
        assert!(r.is_assignable(&unknown_elements, &ints));
        // A genuinely known, different element type is still caught.
        let strings = Type::Array(Box::new(Type::String));
        assert!(!r.is_assignable(&strings, &ints));
    }

    #[test]
    fn is_assignable_treats_unknown_array_as_compatible_with_a_shape() {
        let r = Registry::default();
        let unknown = Type::Array(Box::new(Type::Mixed));
        let shape = Type::ArrayShape(vec![("name".to_string(), Type::String, false)]);
        assert!(r.is_assignable(&unknown, &shape));
        // A genuinely known, homogeneous array still doesn't satisfy an
        // arbitrary shape.
        let ints = Type::Array(Box::new(Type::Int));
        assert!(!r.is_assignable(&ints, &shape));
    }

    #[test]
    fn strict_mixed_arrays_opts_out_of_the_unknown_array_element_leniency() {
        let mut r = Registry {
            strict_mixed_arrays: true,
            ..Registry::default()
        };
        let unknown_elements = Type::Array(Box::new(Type::Mixed));
        let ints = Type::Array(Box::new(Type::Int));
        assert!(!r.is_assignable(&unknown_elements, &ints));
        // Still correctly permissive when the element types genuinely
        // match - strictness only removes the "we don't know" free pass,
        // it doesn't break real assignability.
        assert!(r.is_assignable(&Type::Array(Box::new(Type::Int)), &ints));

        r.strict_mixed_arrays = false;
        assert!(r.is_assignable(&unknown_elements, &ints));
    }

    #[test]
    fn strict_mixed_arrays_opts_out_of_the_unknown_array_shape_leniency() {
        let r = Registry {
            strict_mixed_arrays: true,
            ..Registry::default()
        };
        let unknown = Type::Array(Box::new(Type::Mixed));
        let shape = Type::ArrayShape(vec![("name".to_string(), Type::String, false)]);
        assert!(!r.is_assignable(&unknown, &shape));
    }

    #[test]
    fn is_assignable_accepts_a_value_matching_any_union_member_on_the_supertype_side() {
        // Coverage measurement found this branch (sup is a Union - not to
        // be confused with the sub-is-a-Union branch just above, which is
        // well covered by ordinary nullable-param tests) had zero
        // executions. Manually verified correct against real code first.
        let r = Registry::default();
        let int_or_string = Type::Union(vec![Type::Int, Type::String]);
        assert!(r.is_assignable(&Type::Int, &int_or_string));
        assert!(r.is_assignable(&Type::String, &int_or_string));
        assert!(!r.is_assignable(&Type::Bool, &int_or_string));
    }

    #[test]
    fn is_assignable_handles_intersection_types_on_either_side() {
        // Coverage measurement found both Intersection branches had zero
        // executions - PHP 8.1+ intersection types (`Countable&Iterator`)
        // are real, if uncommon, syntax. Manually verified correct
        // against real code (an intersection-typed value satisfies a
        // supertype matching any one of its members; a plain single-
        // interface value does NOT satisfy an intersection requiring
        // more than it has) before locking in here.
        let r = build_registry(
            r#"<?php
            interface Countable2 {}
            interface Iterator2 {}
        "#,
        );
        let both = Type::Intersection(vec![
            Type::Object("Countable2".to_string(), Vec::new()),
            Type::Object("Iterator2".to_string(), Vec::new()),
        ]);
        let countable_only = Type::Object("Countable2".to_string(), Vec::new());

        // sub is the intersection: satisfies a supertype matching any one
        // of its members.
        assert!(r.is_assignable(&both, &countable_only));
        // sup is the intersection: a value only known to be one of the
        // two doesn't satisfy a requirement for both.
        assert!(!r.is_assignable(&countable_only, &both));
    }

    #[test]
    fn declared_property_type_is_tracked() {
        let r = build_registry(
            r#"<?php
            class Box { public int $value = 0; }
        "#,
        );
        assert_eq!(r.lookup_property("Box", "value").unwrap().ty, Type::Int);
    }

    #[test]
    fn constructor_promotion_registers_a_property() {
        let r = build_registry(
            r#"<?php
            class Point {
                public function __construct(private int $x, public string $label) {}
            }
        "#,
        );
        assert_eq!(r.lookup_property("Point", "x").unwrap().ty, Type::Int);
        assert_eq!(
            r.lookup_property("Point", "label").unwrap().ty,
            Type::String
        );
    }

    #[test]
    fn trait_methods_and_properties_are_flattened_in() {
        let r = build_registry(
            r#"<?php
            trait Greets {
                public string $greeting = "hi";
                public function greet(): string { return "hi"; }
            }
            class Person {
                use Greets;
            }
        "#,
        );
        assert_eq!(
            r.lookup_method("Person", "greet").unwrap().return_type,
            Type::String
        );
        assert_eq!(
            r.lookup_property("Person", "greeting").unwrap().ty,
            Type::String
        );
    }

    #[test]
    fn trait_flattening_works_across_namespaced_classes() {
        // Every existing trait-flattening test uses bare (non-namespaced)
        // PHP - exactly the blind spot that let a real Phase 7
        // invalidation regression through undetected (a bare-name lookup
        // silently found nothing once real code's names became FQNs).
        // flatten_traits iterates self.classes (FQN-keyed) and resolves
        // each class's own `uses` list (also FQN, via ClassInfo::from_decl's
        // ctx.resolve) to look trait method/property tables up by - both
        // sides need to actually agree for this to find anything at all.
        let r = build_registry(
            r#"<?php
            namespace App;
            trait Greets {
                public string $greeting = "hi";
                public function greet(): string { return "hi"; }
            }
            class Person {
                use Greets;
            }
        "#,
        );
        assert_eq!(
            r.lookup_method("App\\Person", "greet").unwrap().return_type,
            Type::String
        );
        assert_eq!(
            r.lookup_property("App\\Person", "greeting").unwrap().ty,
            Type::String
        );
    }

    #[test]
    fn trait_flattening_also_works_for_enums_not_just_classes() {
        // Coverage measurement found ClassInfo::from_enum_decl's `uses`
        // closure (resolving an enum's own `use SomeTrait;`) had zero
        // test executions - enums can use traits in real PHP, and
        // flatten_traits/lookup_method don't distinguish a class's
        // ClassInfo from an enum's, but nothing had actually proven an
        // enum's trait methods flatten in correctly until now.
        let r = build_registry(
            r#"<?php
            trait Greets {
                public function greet(): string { return "hi"; }
            }
            enum Status {
                use Greets;
                case Active;
            }
        "#,
        );
        assert_eq!(
            r.lookup_method("Status", "greet").unwrap().return_type,
            Type::String
        );
    }

    #[test]
    fn trait_used_before_its_declaration_still_resolves() {
        // PHP resolves `use` at compile time, not top-to-bottom - a trait
        // declared later in the file must still flatten in correctly.
        let r = build_registry(
            r#"<?php
            class Person {
                use Greets;
            }
            trait Greets {
                public function greet(): string { return "hi"; }
            }
        "#,
        );
        assert!(r.lookup_method("Person", "greet").is_some());
    }

    #[test]
    fn class_method_overrides_trait_method_of_same_name() {
        let r = build_registry(
            r#"<?php
            trait Greets {
                public function greet(): string { return "generic"; }
            }
            class Person {
                use Greets;
                public function greet(): int { return 1; }
            }
        "#,
        );
        assert_eq!(
            r.lookup_method("Person", "greet").unwrap().return_type,
            Type::Int
        );
    }

    #[test]
    fn property_lookup_walks_up_extends_chain() {
        let r = build_registry(
            r#"<?php
            class Base { public string $name = ""; }
            class Sub extends Base {}
        "#,
        );
        assert_eq!(r.lookup_property("Sub", "name").unwrap().ty, Type::String);
    }

    #[test]
    fn phpdoc_types_an_otherwise_untyped_function() {
        let r = build_registry(
            r#"<?php
            /**
             * @param int $x
             * @return int
             */
            function double($x) { return $x * 2; }
        "#,
        );
        let sig = &r.functions["double"];
        assert_eq!(sig.params[0].ty, Type::Int);
        assert_eq!(sig.return_type, Type::Int);
    }

    #[test]
    fn phpdoc_wins_over_a_conflicting_native_hint() {
        let r = build_registry(
            r#"<?php
            /**
             * @param int[] $items
             */
            function f(array $items) {}
        "#,
        );
        assert_eq!(
            r.functions["f"].params[0].ty,
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn phpdoc_types_an_otherwise_untyped_property() {
        let r = build_registry(
            r#"<?php
            class Box {
                /** @var int */
                public $value;
            }
        "#,
        );
        assert_eq!(r.lookup_property("Box", "value").unwrap().ty, Type::Int);
    }

    #[test]
    fn phpdoc_types_a_promoted_constructor_property_via_param_tag() {
        let r = build_registry(
            r#"<?php
            class Point {
                /**
                 * @param int[] $coords
                 */
                public function __construct(public $coords) {}
            }
        "#,
        );
        assert_eq!(
            r.lookup_property("Point", "coords").unwrap().ty,
            Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn missing_doc_comment_falls_back_to_native_hint() {
        let r = build_registry(r#"<?php function f(string $s): string { return $s; }"#);
        assert_eq!(r.functions["f"].params[0].ty, Type::String);
        assert_eq!(r.functions["f"].return_type, Type::String);
    }

    #[test]
    fn enum_is_registered_with_its_methods_and_implements() {
        let r = build_registry(
            r#"<?php
            interface HasLabel {}
            enum Suit: string implements HasLabel {
                case Hearts = 'H';
                public function label(): string { return "Hearts"; }
            }
        "#,
        );
        assert!(r.is_subclass_of("Suit", "HasLabel"));
        assert_eq!(
            r.lookup_method("Suit", "label").unwrap().return_type,
            Type::String
        );
    }

    #[test]
    fn every_enum_implements_unit_enum_and_a_backed_one_also_implements_backed_enum() {
        // Real bug found on the real Piwigo corpus: doctrine/lexer's own
        // `AbstractLexer<T of UnitEnum|string|int>` flagged every real
        // backed enum passed to it as a bound violation, since nothing
        // ever registered an enum as implementing `UnitEnum`/`BackedEnum`
        // - only their synthesized cases()/name/value members existed,
        // never the hierarchy relationship itself.
        let r = build_registry(
            r#"<?php
            enum Suit: string {
                case Hearts = 'H';
            }
            enum Pure {
                case A;
            }
        "#,
        );
        assert!(r.is_subclass_of("Suit", "UnitEnum"));
        assert!(r.is_subclass_of("Suit", "BackedEnum"));
        assert!(r.is_subclass_of("Pure", "UnitEnum"));
        assert!(!r.is_subclass_of("Pure", "BackedEnum"));
    }
}
