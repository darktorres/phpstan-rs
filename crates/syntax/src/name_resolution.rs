//! Name resolution: what a class, function or constant name written in
//! a file refers to, given the file's `namespace` and `use` statements.

use crate::ast::*;
use crate::name::Name;
use crate::visit::{
    VisitorMut, walk_attribute_mut, walk_catch_clause_mut, walk_class_def_mut, walk_expr_mut,
    walk_stmt_mut, walk_stmts_mut,
};

/// The imports of one namespace: what each name a `use` brings in was
/// imported from, in the three key spaces of PHP's imports - classes and
/// functions by name ignoring case, constants by name as written. A key is
/// the interned name it is read from with where in it the imported name
/// starts, and a value is the interned path, so adding an import allocates
/// nothing and a lookup of a name allocates nothing.
#[derive(Debug, Clone, Default)]
struct UseMap {
    classes: rustc_hash::FxHashMap<ImportKey<true>, Name>,
    functions: rustc_hash::FxHashMap<ImportKey<true>, Name>,
    constants: rustc_hash::FxHashMap<ImportKey<false>, Name>,
}

/// The name an import is known by: the part of `name` from `from` on,
/// compared ignoring case (`FOLDED`) or as written.
#[derive(Debug, Clone)]
struct ImportKey<const FOLDED: bool> {
    name: Name,
    from: usize,
}

impl<const FOLDED: bool> ImportKey<FOLDED> {
    fn text(&self) -> &str {
        // Lowercasing ASCII keeps every byte offset.
        let text = if FOLDED {
            self.name.lower()
        } else {
            self.name.as_str()
        };
        &text[self.from..]
    }
}

impl<const FOLDED: bool> PartialEq for ImportKey<FOLDED> {
    fn eq(&self, other: &Self) -> bool {
        self.text() == other.text()
    }
}

impl<const FOLDED: bool> Eq for ImportKey<FOLDED> {}

impl<const FOLDED: bool> std::hash::Hash for ImportKey<FOLDED> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.text().hash(state);
    }
}

impl<const FOLDED: bool> std::borrow::Borrow<str> for ImportKey<FOLDED> {
    fn borrow(&self) -> &str {
        self.text()
    }
}

/// Calls `f` with `text` in ASCII lowercase, built on the stack when it is
/// short.
fn with_folded<R>(text: &str, f: impl FnOnce(&str) -> R) -> R {
    if !text.bytes().any(|b| b.is_ascii_uppercase()) {
        return f(text);
    }
    let mut buffer = [0u8; 128];
    if let Some(bytes) = buffer.get_mut(..text.len()) {
        bytes.copy_from_slice(text.as_bytes());
        bytes.make_ascii_lowercase();
        if let Ok(folded) = std::str::from_utf8(bytes) {
            return f(folded);
        }
    }
    f(&text.to_ascii_lowercase())
}

impl UseMap {
    fn clear(&mut self) {
        self.classes.clear();
        self.functions.clear();
        self.constants.clear();
    }

    /// Records that the name `name` has from `from` on is `path`'s.
    fn insert(&mut self, kind: UseKind, name: &Name, from: usize, path: Name) {
        let name = name.clone();
        match kind {
            UseKind::Class => self.classes.insert(ImportKey { name, from }, path),
            UseKind::Function => self.functions.insert(ImportKey { name, from }, path),
            UseKind::Const => self.constants.insert(ImportKey { name, from }, path),
        };
    }

    /// What `name` was imported from in `kind`'s key space.
    fn get(&self, kind: UseKind, name: &str) -> Option<&Name> {
        match kind {
            UseKind::Class => with_folded(name, |folded| self.classes.get(folded)),
            UseKind::Function => with_folded(name, |folded| self.functions.get(folded)),
            UseKind::Const => self.constants.get(name),
        }
    }

    fn contains(&self, kind: UseKind, name: &str) -> bool {
        self.get(kind, name).is_some()
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
pub struct NameContext {
    // Which namespace and imports the context holds: a number no other
    // state of any context has, new at each change, kept by a clone, `0`
    // for none. Two contexts with one version resolve every name alike.
    version: u64,
    namespace: String,
    // (which of PHP's three `use`-namespaces, short name or `as Alias`)
    // -> the full path it was imported from. Each namespace is its own
    // key space - see `add_uses`'s own doc comment.
    use_map: UseMap,
}

/// Calls `f` with each top-level statement of a file, in order, and the
/// context in effect there. A block's or a `declare` block's statements
/// are visited in its place, so a braced namespace body sees its `namespace` and `use`
/// statements; a `namespace` or `use` statement is passed after it takes
/// effect.
pub fn for_each_in_context<'a>(stmts: &'a [Stmt], mut f: impl FnMut(&'a Stmt, &NameContext)) {
    fn walk<'a>(
        stmts: &'a [Stmt],
        ctx: &mut NameContext,
        f: &mut impl FnMut(&'a Stmt, &NameContext),
    ) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Namespace(name) => ctx.set_namespace(name),
                StmtKind::Use(imports) => ctx.add_uses(imports),
                StmtKind::Block(inner)
                | StmtKind::Declare {
                    body: Some(inner), ..
                } => {
                    walk(inner, ctx, f);
                    continue;
                }
                _ => {}
            }
            f(stmt, ctx);
        }
    }
    walk(stmts, &mut NameContext::default(), &mut f);
}

/// The context in effect at the byte offset `pos`: every `namespace` and
/// `use` statement that starts before it has taken effect.
pub fn context_at(stmts: &[Stmt], pos: u32) -> NameContext {
    let mut at = NameContext::default();
    for_each_in_context(stmts, |stmt, ctx| {
        if matches!(stmt.kind, StmtKind::Namespace(_) | StmtKind::Use(_)) && stmt.span.start < pos {
            at = ctx.clone();
        }
    });
    at
}

/// What a `NameContext` is stored as: its namespace and each import as
/// the name it is read from, where in it the imported name starts, and the
/// path imported, in the order of the names.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StoredContext {
    namespace: String,
    classes: Vec<(Name, usize, Name)>,
    functions: Vec<(Name, usize, Name)>,
    constants: Vec<(Name, usize, Name)>,
}

impl UseMap {
    fn stored<const FOLDED: bool>(
        imports: &rustc_hash::FxHashMap<ImportKey<FOLDED>, Name>,
    ) -> Vec<(Name, usize, Name)> {
        let mut entries: Vec<(Name, usize, Name)> = imports
            .iter()
            .map(|(key, path)| (key.name.clone(), key.from, path.clone()))
            .collect();
        entries.sort_unstable_by(|a, b| (a.0.as_str(), a.1).cmp(&(b.0.as_str(), b.1)));
        entries
    }
}

/// A version number not given before.
fn next_version() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl NameContext {
    /// Enters a (possibly new) namespace - real `use` imports are scoped
    /// to the namespace block they appear in, so a fresh one clears
    /// whatever was collected under the previous namespace rather than
    /// letting it leak into declarations that follow (relevant for
    /// `namespace { ... } namespace Pdo { ... }`-shaped files - see
    /// `Registry::index`'s doc comment on why that real shape matters).
    pub fn set_namespace(&mut self, name: &str) {
        self.namespace = strip_leading_backslash(name);
        self.use_map.clear();
        self.version = next_version();
    }

    /// Which namespace and imports this context holds, see the field.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// This context as it is stored.
    pub fn to_stored(&self) -> StoredContext {
        StoredContext {
            namespace: self.namespace.clone(),
            classes: UseMap::stored(&self.use_map.classes),
            functions: UseMap::stored(&self.use_map.functions),
            constants: UseMap::stored(&self.use_map.constants),
        }
    }

    /// The context `stored` was made from, with a version of its own.
    pub fn from_stored(stored: StoredContext) -> NameContext {
        let mut context = NameContext::default();
        if stored.namespace.is_empty()
            && stored.classes.is_empty()
            && stored.functions.is_empty()
            && stored.constants.is_empty()
        {
            return context;
        }
        context.namespace = stored.namespace;
        for (name, from, path) in stored.classes {
            context.use_map.insert(UseKind::Class, &name, from, path);
        }
        for (name, from, path) in stored.functions {
            context.use_map.insert(UseKind::Function, &name, from, path);
        }
        for (name, from, path) in stored.constants {
            context.use_map.insert(UseKind::Const, &name, from, path);
        }
        context.version = next_version();
        context
    }

    /// The current file's own namespace, empty for the global
    /// namespace - consumed by `checker::internal_usage`'s own root-
    /// namespace comparison. Every other reader of namespace state goes
    /// through `declared_name`/`resolve` instead; this is the one call
    /// site that genuinely needs the raw current namespace itself, not a
    /// name resolved against it.
    pub fn namespace(&self) -> &str {
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
    pub fn is_use_imported(&self, first_segment: &str) -> bool {
        self.use_map.contains(UseKind::Class, first_segment)
    }

    /// Whether `name`, a class reference as written, resolves to `fqn`:
    /// `resolve(name) == fqn`, with no name built to compare.
    pub fn resolves_to(&self, name: &str, fqn: &str) -> bool {
        if let Some(fq) = name.strip_prefix('\\') {
            return fq == fqn;
        }
        if let Some(rest) = strip_namespace_relative_prefix(name) {
            return self.declares_as(rest, fqn);
        }
        let mut segments = name.splitn(2, '\\');
        let first = segments.next().unwrap_or(name);
        let rest = segments.next();
        if let Some(imported) = self.use_map.get(UseKind::Class, first) {
            return match rest {
                Some(r) => fqn
                    .strip_prefix(imported.as_str())
                    .and_then(|tail| tail.strip_prefix('\\'))
                    .is_some_and(|tail| tail == r),
                None => imported.as_str() == fqn,
            };
        }
        self.declares_as(name, fqn)
    }

    /// Whether a declaration with this bare name gets the name `fqn` in
    /// the current namespace: `declared_name(bare) == fqn`.
    fn declares_as(&self, bare: &str, fqn: &str) -> bool {
        if self.namespace.is_empty() {
            bare == fqn
        } else {
            fqn.strip_prefix(self.namespace.as_str())
                .and_then(|tail| tail.strip_prefix('\\'))
                .is_some_and(|tail| tail == bare)
        }
    }

    /// Keys are `(kind, name)`: PHP's class, function, and constant
    /// imports live in three separate namespaces, so `use function
    /// assert;` must never answer a class reference to `Assert`, nor
    /// `use const Foo\BAR;` a function call `bar()`. Class and function
    /// names resolve case-insensitively and are stored lowercased;
    /// constant names are case-sensitive and stored as written.
    pub fn add_uses(&mut self, imports: &[UseImport]) {
        if !imports.is_empty() {
            self.version = next_version();
        }
        for import in imports {
            // The name an import is known by: its alias, or the last
            // segment of what it imports.
            match &import.alias {
                Some(alias) => {
                    self.use_map
                        .insert(import.kind, &alias.name, 0, import.path.clone());
                }
                None => {
                    let from = import.path.rfind('\\').map_or(0, |at| at + 1);
                    self.use_map
                        .insert(import.kind, &import.path, from, import.path.clone());
                }
            }
        }
    }

    /// The fully-qualified name a declaration with this bare name gets,
    /// in the current namespace - declarations are never `use`-resolved
    /// (that only applies to *references*), always just namespace-relative.
    pub fn declared_name(&self, bare: &str) -> String {
        qualify(&self.namespace, bare)
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
    pub fn resolve(&self, name: &str) -> String {
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
        if let Some(imported_path) = self.use_map.get(lookup_kind, first) {
            return match rest {
                Some(r) => {
                    let mut resolved = String::with_capacity(imported_path.len() + 1 + r.len());
                    resolved.push_str(imported_path);
                    resolved.push('\\');
                    resolved.push_str(r);
                    resolved
                }
                None => imported_path.to_string(),
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
    fn resolve_function_candidates(&self, name: &str) -> (String, Option<String>) {
        self.resolve_with_global_fallback(name, UseKind::Function)
    }

    /// The constant counterpart of `resolve_function_candidates` - real
    /// PHP applies the identical current-namespace-then-global fallback
    /// to an unqualified constant, but matches it against `use const`
    /// imports, never `use function` ones.
    fn resolve_constant_candidates(&self, name: &str) -> (String, Option<String>) {
        self.resolve_with_global_fallback(name, UseKind::Const)
    }

    fn resolve_with_global_fallback(&self, name: &str, kind: UseKind) -> (String, Option<String>) {
        let is_unqualified = !name.contains('\\');
        let use_matched = is_unqualified && self.use_map.contains(kind, name);
        if !is_unqualified || use_matched || self.namespace.is_empty() {
            (self.resolve_as(name, kind), None)
        } else {
            (self.declared_name(name), Some(name.to_string()))
        }
    }
}

/// `name` in `namespace`; `name` itself in the global namespace.
pub fn qualify(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_string()
    } else {
        let mut qualified = String::with_capacity(namespace.len() + 1 + name.len());
        qualified.push_str(namespace);
        qualified.push('\\');
        qualified.push_str(name);
        qualified
    }
}

/// Fills in [`QualifiedName::resolved`] for every name in a tree, tracking
/// `namespace` and `use` statements in order: a braced namespace body
/// shares the context of its `namespace` statement. Declarations get their
/// namespaced name. The parser resolves each name as it makes it, so this
/// is for a tree changed after: the stub attributes' types.
pub fn resolve_names(stmts: &mut [Stmt]) {
    let mut resolver = Resolver {
        ctx: NameContext::default(),
    };
    walk_stmts_mut(&mut resolver, stmts);
}

struct Resolver {
    ctx: NameContext,
}

impl Resolver {
    /// The trait names a trait-use block's clauses name.
    fn trait_adaptations(&self, adaptations: &mut TraitAdaptations) {
        for name in adaptations
            .aliases
            .iter_mut()
            .filter_map(|a| a.trait_name.as_mut())
        {
            self.class(name);
        }
        for precedence in &mut adaptations.precedences {
            self.class(&mut precedence.trait_name);
            for name in &mut precedence.instead_of {
                self.class(name);
            }
        }
    }

    fn class(&self, name: &mut QualifiedName) {
        resolve_class_name(name, &self.ctx);
    }

    fn function(&self, name: &mut QualifiedName) {
        name.resolved = self.ctx.function_reference(&name.written).into();
    }

    fn constant(&self, name: &mut QualifiedName) {
        name.resolved = self.ctx.constant_reference(&name.written).into();
    }

    #[inline(never)]
    fn resolve_expr(&mut self, expr: &mut Expr) {
        match &mut expr.kind {
            ExprKind::Name(name) => self.constant(name),

            ExprKind::Call(callee, args) if matches!(callee.kind, ExprKind::Name(_)) => {
                if let ExprKind::Name(name) = &mut callee.kind {
                    self.function(name);
                }
                for arg in args {
                    self.visit_arg(arg);
                }
            }
            ExprKind::New(callee, args) if matches!(callee.kind, ExprKind::Name(_)) => {
                if let ExprKind::Name(name) = &mut callee.kind {
                    self.class(name);
                }
                for arg in args {
                    self.visit_arg(arg);
                }
            }
            ExprKind::ClassConstFetch(target, _)
            | ExprKind::ClassNameFetch(target)
            | ExprKind::StaticPropertyFetch(target, _) => self.static_target(target),
            ExprKind::DynamicStaticPropertyFetch(target, name)
            | ExprKind::DynamicClassConstFetch(target, name) => {
                self.static_target(target);
                self.visit_expr(name);
            }
            ExprKind::StaticCall(target, _, args) => {
                self.static_target(target);
                for arg in args {
                    self.visit_arg(arg);
                }
            }
            ExprKind::DynamicStaticCall(target, name, args) => {
                self.static_target(target);
                self.visit_expr(name);
                for arg in args {
                    self.visit_arg(arg);
                }
            }
            ExprKind::Instanceof(inner, target) => {
                self.visit_expr(inner);
                match target {
                    InstanceofTarget::Name(name) => self.class(name),
                    InstanceofTarget::Expr(e) => self.visit_expr(e),
                }
            }
            _ => walk_expr_mut(self, expr),
        }
    }

    fn static_target(&mut self, target: &mut StaticTarget) {
        match target {
            StaticTarget::Name(name) => self.class(name),
            StaticTarget::Expr(e) => self.visit_expr(e),
        }
    }
}

/// What a name written in a file refers to, as text: the result of a
/// resolution before its name is interned.
pub(crate) enum Resolution {
    Unresolved,
    Exact(String),
    WithGlobalFallback(String),
}

impl NameContext {
    /// What a class reference as written refers to.
    pub(crate) fn class_reference(&self, written: &str) -> Resolution {
        if is_class_keyword(written) {
            Resolution::Unresolved
        } else {
            Resolution::Exact(self.resolve(written))
        }
    }

    /// What a function reference as written refers to.
    pub(crate) fn function_reference(&self, written: &str) -> Resolution {
        match self.resolve_function_candidates(written) {
            (primary, Some(_)) => Resolution::WithGlobalFallback(primary),
            (primary, None) => Resolution::Exact(primary),
        }
    }

    /// What a constant reference as written refers to.
    pub(crate) fn constant_reference(&self, written: &str) -> Resolution {
        match self.resolve_constant_candidates(written) {
            (primary, Some(_)) => Resolution::WithGlobalFallback(primary),
            (primary, None) => Resolution::Exact(primary),
        }
    }
}

impl From<Resolution> for ResolvedName {
    fn from(resolution: Resolution) -> ResolvedName {
        match resolution {
            Resolution::Unresolved => ResolvedName::Unresolved,
            Resolution::Exact(name) => ResolvedName::Exact(name.into()),
            Resolution::WithGlobalFallback(name) => ResolvedName::WithGlobalFallback(name.into()),
        }
    }
}

/// Fills in `name`'s resolution as a class name in `ctx`, for a name
/// parsed apart from the file it belongs to.
pub fn resolve_class_name(name: &mut QualifiedName, ctx: &NameContext) {
    name.resolved = ctx.class_reference(&name.written).into();
}

/// `self`, `parent` and `static` name a class relative to where they
/// appear, not by resolution.
fn is_class_keyword(name: &str) -> bool {
    ["self", "parent", "static"]
        .iter()
        .any(|k| name.eq_ignore_ascii_case(k))
}

impl VisitorMut for Resolver {
    fn visit_stmt(&mut self, stmt: &mut Stmt) {
        match &mut stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => self.ctx.add_uses(imports),
            _ => {
                match &mut stmt.kind {
                    StmtKind::FunctionDecl(f) => {
                        f.namespaced_name = Some(self.ctx.declared_name(&f.name).into());
                    }
                    StmtKind::ClassDecl(c) => {
                        c.namespaced_name = self.ctx.declared_name(&c.name).into();
                    }
                    StmtKind::GlobalConst(consts) => {
                        for c in consts {
                            c.namespaced_name = Some(self.ctx.declared_name(&c.name).into());
                        }
                    }
                    _ => {}
                }
                walk_stmt_mut(self, stmt);
            }
        }
    }

    fn visit_expr(&mut self, expr: &mut Expr) {
        // Only name-bearing expressions need more than the plain walk;
        // keeping the rest out of `resolve_expr` keeps the frames of this
        // recursion small.
        if matches!(
            expr.kind,
            ExprKind::Name(_)
                | ExprKind::Call(..)
                | ExprKind::New(..)
                | ExprKind::ClassConstFetch(..)
                | ExprKind::ClassNameFetch(_)
                | ExprKind::StaticPropertyFetch(..)
                | ExprKind::DynamicStaticPropertyFetch(..)
                | ExprKind::DynamicClassConstFetch(..)
                | ExprKind::StaticCall(..)
                | ExprKind::DynamicStaticCall(..)
                | ExprKind::Instanceof(..)
        ) {
            self.resolve_expr(expr);
        } else {
            walk_expr_mut(self, expr);
        }
    }

    fn visit_class_def(&mut self, class: &mut ClassDef) {
        for name in class.extends.iter_mut().chain(class.implements.iter_mut()) {
            self.class(name);
        }
        for trait_use in &mut class.trait_uses {
            for name in &mut trait_use.traits {
                self.class(name);
            }
            self.trait_adaptations(&mut trait_use.adaptations);
        }
        walk_class_def_mut(self, class);
    }

    fn visit_catch_clause(&mut self, clause: &mut CatchClause) {
        for name in &mut clause.types {
            self.class(name);
        }
        walk_catch_clause_mut(self, clause);
    }

    fn visit_attribute(&mut self, attribute: &mut Attribute) {
        self.class(&mut attribute.name);
        walk_attribute_mut(self, attribute);
    }

    fn visit_type_hint(&mut self, hint: &mut TypeHint) {
        crate::stack::guard(|| match &mut hint.kind {
            TypeHintKind::Keyword(..) => {}
            TypeHintKind::Name(name) => self.class(name),
            TypeHintKind::Nullable(inner) => self.visit_type_hint(inner),
            TypeHintKind::Union(members) | TypeHintKind::Intersection(members) => {
                for member in members {
                    self.visit_type_hint(member);
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
