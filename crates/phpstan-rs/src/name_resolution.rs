//! Name resolution: what a class, function or constant name written in
//! a file refers to, given the file's `namespace` and `use` statements.

use std::sync::Arc;

use crate::ast::{Stmt, StmtKind, UseImport, UseKind};

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
    (UseKind, String),
    String,
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
    // (which of PHP's three `use`-namespaces, short name or `as Alias`)
    // -> the full path it was imported from. Each namespace is its own
    // key space - see `add_uses`'s own doc comment.
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
        self.use_map
            .contains_key(&(UseKind::Class, first_segment.to_ascii_lowercase()))
    }

    /// Keys are `(kind, name)`: PHP's class, function, and constant
    /// imports live in three separate namespaces, so `use function
    /// assert;` must never answer a class reference to `Assert`, nor
    /// `use const Foo\BAR;` a function call `bar()`. Class and function
    /// names resolve case-insensitively and are stored lowercased;
    /// constant names are case-sensitive and stored as written.
    pub(crate) fn add_uses(&mut self, imports: &[UseImport]) {
        for import in imports {
            let path = strip_leading_backslash(&import.path);
            let bare = path.rsplit('\\').next().unwrap_or(&path).to_string();
            let short_name = import.alias.clone().unwrap_or_else(|| bare.clone().into());
            self.use_map
                .insert((import.kind, use_map_key(import.kind, &short_name)), path);
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
        // A qualified name (`Foo\BAR`) always resolves its first segment
        // through a class-kind (namespace) import, whatever the kind of
        // the symbol it finally names.
        let lookup_kind = if rest.is_some() { UseKind::Class } else { kind };
        if let Some(imported_path) = self
            .use_map
            .get(&(lookup_kind, use_map_key(lookup_kind, first)))
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
        self.resolve_with_global_fallback(name, UseKind::Function)
    }

    /// The constant counterpart of `resolve_function_candidates` - real
    /// PHP applies the identical current-namespace-then-global fallback
    /// to an unqualified constant, but matches it against `use const`
    /// imports, never `use function` ones.
    pub(crate) fn resolve_constant_candidates(&self, name: &str) -> (String, Option<String>) {
        self.resolve_with_global_fallback(name, UseKind::Const)
    }

    fn resolve_with_global_fallback(&self, name: &str, kind: UseKind) -> (String, Option<String>) {
        let is_unqualified = !name.contains('\\');
        let use_matched =
            is_unqualified && self.use_map.contains_key(&(kind, use_map_key(kind, name)));
        if !is_unqualified || use_matched || self.namespace.is_empty() {
            (self.resolve_as(name, kind), None)
        } else {
            (self.declared_name(name), Some(name.to_string()))
        }
    }
}

/// `NameContext::use_map`'s key normalization - see `add_uses`.
fn use_map_key(kind: UseKind, name: &str) -> String {
    match kind {
        UseKind::Const => name.to_string(),
        UseKind::Class | UseKind::Function => name.to_ascii_lowercase(),
    }
}
