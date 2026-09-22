use std::sync::Arc;

use crate::ast::{Stmt, StmtKind, UseImport, UseKind};
use crate::intern::intern;
use crate::types::{CallableParam, CallableSig, Type};

/// `NameContext::use_map`'s own backing map - `imbl::HashMap` (a
/// persistent, structurally-shared HAMT), not `std::collections::HashMap`:
/// `NameContext` is deep-cloned once per branch of every top-level
/// conditional-declaration idiom (`registry::index::build_declarations_
/// with_context`'s own `StmtKind::If` arm) to keep each branch's own
/// `use`/namespace changes from leaking, and a plain `HashMap` clone
/// rehashes every key and deep-clones every `String` pair. `imbl::
/// HashMap::clone` is O(1) (a pointer bump plus a refcount increment) -
/// see `Checker`'s own `LocalsMap` (`src/checker.rs`) for the identical
/// rationale, applied there to the checker's per-branch variable-type
/// map instead of this registry-side use-import map.
type UseMap = imbl::GenericHashMap<
    String,
    (String, UseKind),
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

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

/// If `name` starts with a case-insensitive `namespace\` segment (the
/// real PHP `namespace\foo`/`namespace\Foo\Bar` construct - "relative to
/// the current namespace, explicitly," distinct from an ordinary bare
/// reference which would also try a `use`-import first), returns
/// whatever comes after that prefix. `Keyword::try_from_str` already
/// lowercases before matching, so `NAMESPACE\foo`/`Namespace\foo` tokenize
/// identically to the lowercase spelling - matched the same way here for
/// consistency, not because real PHP is actually case-sensitive here.
fn strip_namespace_relative_prefix(name: &str) -> Option<&str> {
    if !name.is_char_boundary(9) {
        return None;
    }
    let (head, rest) = name.split_at(9);
    if !head.eq_ignore_ascii_case("namespace") {
        return None;
    }
    rest.strip_prefix('\\')
}

/// Resolves a bare class-name reference (`use`-imports and same-namespace
/// relative references both apply) to a fully-qualified one, so two
/// unrelated classes with the same short name in different namespaces get
/// different registry keys instead of colliding - see the extensive
/// comment on this in `checker.rs`'s module doc for why that matters.
/// Built once per file (from its own `namespace`/`use`
/// statements) and shared by `Registry::index` (for extends/implements/
/// type hints) and `Checker` (for expression-level references like
/// `new Foo()`) - both need to agree on exactly the same resolution or
/// the two halves of this registry would disagree with each other.
#[derive(Debug, Clone, Default)]
pub(crate) struct NameContext {
    // `Arc<str>`, not `String`, for the same O(1)-clone reason `use_map`
    // is `imbl`-backed rather than `std::collections::HashMap` - see
    // `UseMap`'s own doc comment.
    namespace: Arc<str>,
    // Short name (or `as Alias`) -> (the full path it was imported from,
    // which of PHP's three `use`-namespaces it belongs to). The kind is
    // load-bearing, not decorative - see `add_uses`'s own doc comment for
    // why conflating them is unsafe.
    use_map: UseMap,
}

impl NameContext {
    /// Scans a file's top-level statements for `namespace`/`use`
    /// declarations, applying whichever `namespace` statement comes
    /// *last* in the file to every declaration, regardless of where each
    /// one actually appears - a real, if rare, inaccuracy: real PHP
    /// genuinely allows more than one semicolon-style `namespace Foo;` in
    /// a single file, each one switching the active namespace for
    /// everything that follows until the next one, not a "one per file"
    /// rule. `registry::build_declarations`'s sibling, positional
    /// `ctx.set_namespace` walk gets this right, and so does
    /// `Checker::check_program`'s own main per-statement loop - this
    /// whole-file `build` function itself is kept only for a few
    /// genuinely position-independent, whole-file questions that
    /// `check_program` still uses it for (see its own call sites' doc
    /// comments), where "last namespace wins" is not a problem.
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
        self.namespace = strip_leading_backslash(name).into();
        self.use_map.clear();
    }

    /// The current file's own namespace, empty for the global
    /// namespace - consumed by `checker::internal_usage`'s own root-
    /// namespace comparison. Every other reader of namespace state goes
    /// through `declared_name`/`resolve` instead; this is the one call
    /// site that genuinely needs the raw current namespace itself, not a
    /// name resolved against it.
    pub(crate) fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Whether `first_segment` (a reference's own first `\`-
    /// separated segment, as written in source) matched a real *class*-
    /// kind `use`-import in this file - lets a caller skip the `class.
    /// nameCase` family of checks for exactly this case, matching real
    /// PHPStan's own default `checkImportedClassNameCase: false` config.
    /// Class-kind only, same reasoning as `resolve`'s own kind filter
    /// just below - a class-shaped reference can only ever have been
    /// legitimately imported via a plain `use Foo;`, never `use function`/
    /// `use const`.
    pub(crate) fn is_use_imported(&self, first_segment: &str) -> bool {
        matches!(
            self.use_map.get(&first_segment.to_ascii_lowercase()),
            Some((_, UseKind::Class))
        )
    }

    /// `use_map`'s own keys are stored lowercased - real PHP resolves a
    /// `use`-imported class/interface/trait/enum name (and a
    /// `use function` one) case-insensitively, the same rationale
    /// `Registry::lookup_function`'s own doc comment already gives.
    /// Unlike `lookup_function`'s exact-then-fallback-scan shape, a
    /// straight lowercase-key store is correct and simpler here: PHP
    /// forbids two *different* `use`-imports in one file colliding
    /// case-insensitively, so there's no shadowing-order concern to
    /// preserve. Disclosed, narrow residual: `use_map` is shared across
    /// all three `use` namespaces (class/function/const - `UseImport::
    /// kind` isn't consulted here), and real PHP constants *are* case-
    /// sensitive - a file importing two differently-cased constant
    /// aliases that collide only after lowercasing (e.g. `use const
    /// Foo\BAR, Baz\bar;`) would now mis-resolve one of them. This is
    /// exceptionally rare in practice (nobody names two distinct global
    /// constants differing only by case and imports both in one file),
    /// and constant existence is already, separately, never checked
    /// against this map at all (see `UseImport::kind`'s own doc comment).
    ///
    /// `import.kind` is stored alongside the path (not discarded) and
    /// consulted by every reader below: a file can declare `use function
    /// assert;` (the global `assert()` function) and separately
    /// `extends Assert` (an unrelated class in the same namespace,
    /// needing no `use` of its own) - without tracking the kind, a
    /// case-insensitive lookup would let the *function* import for
    /// `assert` answer a *class* reference to `Assert`, silently
    /// severing that inheritance link.
    pub(crate) fn add_uses(&mut self, imports: &[UseImport]) {
        for import in imports {
            let path = strip_leading_backslash(&import.path);
            let bare = path.rsplit('\\').next().unwrap_or(&path).to_string();
            let short_name = import.alias.clone().unwrap_or_else(|| bare.clone().into());
            self.use_map
                .insert(short_name.to_ascii_lowercase(), (path, import.kind));
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
        self.resolve_as(name, UseKind::Class)
    }

    /// `resolve`'s own real implementation, parameterized by which of
    /// PHP's three `use`-namespaces a match must belong to - see
    /// `add_uses`'s own doc comment for why this parameter exists: a
    /// class-shaped reference must never resolve through a `use
    /// function`/`use const` import sharing the same bare name, and vice
    /// versa.
    fn resolve_as(&self, name: &str, kind: UseKind) -> String {
        if let Some(fq) = name.strip_prefix('\\') {
            return fq.to_string();
        }
        // `namespace\foo` (the keyword `namespace`, case-insensitive per
        // `Keyword::try_from_str` lowercasing before it's ever compared,
        // then a literal `\`) is a real, distinct PHP construct - not an
        // ordinary qualified reference to a symbol literally named
        // "namespace" - meaning "resolve `foo` against the current
        // namespace only, never via a `use`-import and never falling
        // back to the global namespace the way an unqualified function
        // call otherwise would." Without this, falling through to
        // `declared_name(name)` below with the literal word "namespace"
        // still in the string re-qualifies the whole thing against the
        // current namespace a second time, e.g. `Psl\Str\namespace\
        // chunk()` instead of the real `Psl\Str\chunk`.
        if let Some(rest) = strip_namespace_relative_prefix(name) {
            return self.declared_name(rest);
        }
        let mut segments = name.splitn(2, '\\');
        let first = segments.next().unwrap_or(name);
        let rest = segments.next();
        if let Some((imported_path, matched_kind)) = self.use_map.get(&first.to_ascii_lowercase())
            && *matched_kind == kind
        {
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
        let use_matched = is_unqualified
            && matches!(
                self.use_map.get(&name.to_ascii_lowercase()),
                Some((_, UseKind::Function))
            );
        if !is_unqualified || use_matched || self.namespace.is_empty() {
            (self.resolve_as(name, UseKind::Function), None)
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
        // Same reasoning as `object` above for the *name* itself, but
        // unlike `object`/`resource` (always argument-less in practice -
        // real PHP forbids generics on either native keyword), a real,
        // programmer-written `@return Traversable<Edge>`/`Iterator<K, V>`
        // tag is common (confirmed via a real corpus false positive:
        // `bacon/bacon-qr-code`'s own `@return Traversable<Edge>`, `Edge`
        // a real same-namespace class that never got resolved past its
        // bare spelling) - `Traversable` isn't only this project's own
        // internal `iterable`-desugaring synthetic, it's also real,
        // directly-written PHPDoc syntax, so its own type arguments still
        // need ordinary name resolution here, the same as `$$valueof`'s
        // wrapped argument just below - only the outer name itself must
        // stay untouched, for the reason already given.
        Type::Object(name, args) if name == "Traversable" => Type::Object(
            name,
            args.into_iter()
                .map(|a| resolve_names_in_type(a, ctx))
                .collect(),
        ),
        // Same reasoning as `object`/`Traversable` above: `resource` is
        // a sentinel for a real PHP value kind with no actual class
        // behind it (`types::parse_type_keyword_or_class`'s own
        // `"resource" => Type::Object("resource", ...)` arm), not a
        // class reference to resolve - without this, the same pseudo-
        // type used in two different namespaces was treated as two
        // unrelated "classes" (`App\resource` vs. plain `resource`).
        Type::Object(name, args) if name == "resource" => Type::Object(name, args),
        // Same reasoning again: `"$$interface"`/`"$$trait"`
        // (`types::parse_type_keyword_or_class`'s own bare `interface-
        // string`/`trait-string` sentinels, always argument-less) are
        // internal markers `registry::is_assignable`'s `ClassString` arm
        // recognizes directly, not class references to resolve.
        Type::Object(name, args) if name == "$$interface" || name == "$$trait" => {
            Type::Object(name, args)
        }
        // `"$$valueof"` (`types::parse_type_keyword_or_class`'s own
        // `value-of<T>`-on-a-class/enum sentinel, `Type::Object("$$valueof",
        // [Type::Object(name, [])])`) is a marker, not a class reference
        // itself - resolved later by `Registry::resolve_deferred_value_of`
        // once real class/enum data exists - but unlike `object`/
        // `Traversable`/`resource` above, its own *argument* (the real
        // class/enum name it wraps) still needs ordinary name resolution
        // here, the same as any other `Object`'s type arguments - without
        // this special case, the generic arm just below would instead try
        // to resolve `"$$valueof"` itself as if it were a genuine (and
        // nonsensical) class reference.
        //
        // `"$$covariant"`/`"$$contravariant"`/`"$$bivariant"`
        // (`types::parse_generic_arg`'s own call-site variance sentinels)
        // need the exact same treatment, for the exact same reason: a
        // fixture inside any `namespace` block would otherwise mangle
        // e.g. `"$$covariant"` into `App\$$covariant` before
        // `Registry::effective_slot_variance` ever gets a chance to
        // recognize it.
        Type::Object(name, args)
            if matches!(
                name.as_str(),
                "$$valueof" | "$$covariant" | "$$contravariant" | "$$bivariant"
            ) =>
        {
            Type::Object(
                name,
                args.into_iter()
                    .map(|a| resolve_names_in_type(a, ctx))
                    .collect(),
            )
        }
        Type::Object(name, args) => Type::Object(
            ctx.resolve(&name),
            args.into_iter()
                .map(|a| resolve_names_in_type(a, ctx))
                .collect(),
        ),
        Type::Array(key, inner) => Type::Array(
            Box::new(resolve_names_in_type(*key, ctx)),
            Box::new(resolve_names_in_type(*inner, ctx)),
        ),
        Type::ClassString(inner) => Type::ClassString(Box::new(resolve_names_in_type(*inner, ctx))),
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
        // `object{...}`'s own field types need resolving too - a genuine
        // class reference nested inside one
        // (`object{repo: SomeRepository}`) must not stay unqualified
        // inside a namespaced file.
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, resolve_names_in_type(t, ctx), opt))
                .collect(),
        ),
        // Mirrors `mark_template_params`/`substitute_type_aliases`'s own
        // equivalent arms - a class reference inside a `Closure(...)`
        // shape's own param/return types needs the same namespace
        // resolution as anywhere else.
        Type::Callable(Some(sig)) => Type::Callable(Some(std::sync::Arc::new(CallableSig {
            params: sig
                .params
                .iter()
                .map(|p| CallableParam {
                    ty: resolve_names_in_type(p.ty.clone(), ctx),
                    variadic: p.variadic,
                    optional: p.optional,
                    by_ref: p.by_ref,
                })
                .collect(),
            return_type: Box::new(resolve_names_in_type((*sig.return_type).clone(), ctx)),
        }))),
        // Mirrors `mark_template_params`/`substitute_type_aliases`'s own
        // identical arms - a real class reference inside one of a
        // conditional return type's own branches (e.g. `Connection` in
        // `@phpstan-return (... ? T : Connection)`) needs the same
        // namespace resolution as anywhere else.
        Type::Conditional {
            subject,
            target,
            if_true,
            if_false,
            negated,
        } => Type::Conditional {
            subject,
            target: Box::new(resolve_names_in_type(*target, ctx)),
            if_true: Box::new(resolve_names_in_type(*if_true, ctx)),
            if_false: Box::new(resolve_names_in_type(*if_false, ctx)),
            negated,
        },
        other => other,
    }
}

/// The read-only, `(raw, resolved)`-collecting sibling of
/// `resolve_names_in_type` just above - same
/// recursive shape, same sentinel skip-list, but pushes into `out`
/// instead of rewriting `ty` in place. `class.nameCase` for a PHPDoc-typed
/// position needs the pre-resolution *spelling* (`ctx.is_use_imported`'s
/// own input) alongside the resolved FQN, which `resolve_names_in_type`
/// itself destroys the moment it overwrites `Object`'s own name field -
/// this is a parallel traversal run one step earlier (right after
/// `mark_template_params`, right before `resolve_names_in_type`) rather
/// than a new field threaded through `Type` itself (`Type` derives
/// `PartialEq`, used for union dedup/narrowing/subtype tests throughout;
/// a raw-text field would need a hand-written exclusion, and even then
/// wouldn't carry the pre-resolution string this specifically needs).
///
/// Must be kept in sync with `resolve_names_in_type`'s own arms by hand -
/// a case missed in one silently misses a nested reference the other
/// still resolves (or, conversely, collects a case-ref for a sentinel
/// `resolve_names_in_type` itself never treats as a real class
/// reference).
pub(crate) fn collect_class_name_case_refs(
    ty: &Type,
    ctx: &NameContext,
    out: &mut Vec<(Arc<str>, Arc<str>)>,
) {
    match ty {
        // Mirrors `resolve_names_in_type`'s own sentinel arms exactly:
        // `object`/`resource`/`$$interface`/`$$trait` are never a real
        // class reference to check the case of, and (unlike the
        // `$$valueof`/variance sentinels just below) their own argument
        // list isn't recursed into there either, so this doesn't either -
        // both are always argument-less in practice regardless.
        Type::Object(name, _)
            if matches!(
                name.as_str(),
                "object" | "resource" | "$$interface" | "$$trait"
            ) => {}
        Type::Object(name, args)
            if matches!(
                name.as_str(),
                "$$valueof" | "$$covariant" | "$$contravariant" | "$$bivariant" | "Traversable"
            ) =>
        {
            for a in args {
                collect_class_name_case_refs(a, ctx, out);
            }
        }
        Type::Object(name, args) => {
            out.push((intern(name), intern(&ctx.resolve(name))));
            for a in args {
                collect_class_name_case_refs(a, ctx, out);
            }
        }
        Type::Array(key, inner) => {
            collect_class_name_case_refs(key, ctx, out);
            collect_class_name_case_refs(inner, ctx, out);
        }
        Type::ClassString(inner) => collect_class_name_case_refs(inner, ctx, out),
        Type::Union(members) | Type::Intersection(members) => {
            for m in members {
                collect_class_name_case_refs(m, ctx, out);
            }
        }
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => {
            for (_, t, _) in fields {
                collect_class_name_case_refs(t, ctx, out);
            }
        }
        Type::Callable(Some(sig)) => {
            for p in &sig.params {
                collect_class_name_case_refs(&p.ty, ctx, out);
            }
            collect_class_name_case_refs(&sig.return_type, ctx, out);
        }
        Type::Conditional {
            target,
            if_true,
            if_false,
            ..
        } => {
            collect_class_name_case_refs(target, ctx, out);
            collect_class_name_case_refs(if_true, ctx, out);
            collect_class_name_case_refs(if_false, ctx, out);
        }
        _ => {}
    }
}
