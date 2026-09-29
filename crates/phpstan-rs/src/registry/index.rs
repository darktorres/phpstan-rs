use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::registry::Registry;
use crate::registry::class_decl_name;
use crate::registry::class_info::MemberSpans;
use crate::registry::provenance::{Origin, Provenance};
use crate::registry::{
    ClassReflection, FunctionReflection, folded_key, function_key, infer_const_type,
};
use syntax::ast::{Expr, ExprKind, Literal, Stmt, StmtKind, Visibility};
use syntax::intern::intern;
use syntax::name_resolution::NameContext;
use syntax::visit::{
    Visitor, walk_class_decl, walk_expr, walk_function_decl, walk_stmt, walk_stmts,
};
use types::Type;

/// One declaration's already-built, registry-independent info/sig, plus
/// the file-position facts (`name`, `conditional`) `Registry::
/// insert_built_decl` needs to decide how it interacts with whatever's
/// already in the registry - the output of `build_declarations`, meant
/// to be consumed one at a time, in order, by `insert_built_decl`. See
/// `build_declarations`'s own doc comment for why this exists as a
/// wholly separate step from the insertion logic below.
pub(crate) enum BuiltDecl {
    Class {
        name: Arc<str>,
        conditional: bool,
        info: ClassReflection,
    },
    Enum {
        name: Arc<str>,
        conditional: bool,
        info: ClassReflection,
    },
    Function {
        name: Arc<str>,
        conditional: bool,
        sig: FunctionReflection,
        // The declaration's name position, `(line, col)`.
        site: (u32, u32),
    },
    GlobalConst {
        name: Arc<str>,
        ty: Type,
    },
}

impl Registry {
    /// `pub(crate)` for the same reason as `flatten_traits` - lets
    /// `Project` index one file's statements into an already-built
    /// registry (initial load, or after a re-parse) without re-running
    /// `build`'s from-scratch pass over every other file.
    ///
    /// A thin sequential wrapper: `build_declarations` (see its own doc
    /// comment) does the actual tree walk and produces every declaration
    /// this file contributes as an already-built, registry-independent
    /// `BuiltDecl`, in depth-first order; this just feeds each one to
    /// `insert_built_decl` in that same order. Kept as one function
    /// (rather than inlining this loop at every call site) so `Project::
    /// load_file`'s and `update_file`'s existing single-file callers need
    /// no changes at all - `check_project.rs`'s parallel batch path calls
    /// `build_declarations` directly instead (once per file, concurrently,
    /// no `&mut self`/registry access needed for that half at all), then
    /// runs this same `insert_built_decl` sequentially, in whatever file
    /// order its own collision-resolution policy needs. See
    /// `insert_built_decl`'s own doc comment for why splitting it out
    /// this way - rather than writing a second, independent tree-walker
    /// for the parallel path - matters here specifically.
    #[hotpath::measure]
    pub(crate) fn index(&mut self, stmts: &[Stmt], origin: &Origin, analyzed: bool) {
        for decl in build_declarations(stmts, origin) {
            self.insert_built_decl(decl, analyzed);
        }
    }

    /// Applies one already-built declaration (see `BuiltDecl`/
    /// `build_declarations`) to the registry - factored out so both the
    /// sequential single-file path above and the parallel-parse batch
    /// path in `check_project.rs`'s `insert_parsed_file` call site share
    /// one implementation of every FQN-collision decision below, rather
    /// than risking two independently maintained copies of this dense
    /// logic (documented per-branch below) silently drifting apart from
    /// each other over time.
    ///
    /// `conditional` is `true` once inside any `if`/`else` branch
    /// (tracked by `build_declarations`) - a declaration found there only
    /// fills a name in if nothing unconditional has already claimed it
    /// (`or_insert_with`), while a top-level, unconditional declaration
    /// always wins outright (`insert`), regardless of which is indexed
    /// first. Without this distinction, two files declaring the same
    /// fully-qualified name - one for real, one as a defensive `if (!
    /// class_exists('X')) { class X {...} }`/`if (!function_exists('foo'))
    /// { function foo() {} }` fallback guarding against a dependency that
    /// might not be loaded - would resolve to whichever happened to be
    /// indexed last (a plain `HashMap::insert` is order-dependent), not
    /// whichever is actually real. `PHPUnit\Framework\TestCase` is a
    /// real, observed instance: a vendored guarded fallback with no
    /// `extends Assert`, alphabetically after the real
    /// `phpunit/.../TestCase.php`, silently won the registry slot and
    /// dropped `assertEquals` from every real subclass.
    ///
    /// `analyzed` is `true` only for a first-party file being checked
    /// (never for a `--scan-dir`/composer-vendor context file or a
    /// built-in stub) - see the `BuiltDecl::Class` arm's
    /// `unconditionally_declared_classes` branch for why this needs to be
    /// threaded all the way down here rather than decided once by the
    /// caller: an analyzed file's declaration must always win a same-name
    /// collision outright, never merge with a pre-existing context/stub
    /// entry the way two context-only declarations legitimately do.
    pub(crate) fn insert_built_decl(&mut self, decl: BuiltDecl, analyzed: bool) {
        self.record_assert_tagged_names(&decl);
        match decl {
            BuiltDecl::Class {
                name,
                conditional,
                info: new_info,
            } => {
                if conditional && self.built_in_stub_classes.contains(&name) {
                    // A plain `or_insert_with` no-op (the shape every
                    // other conditional case below uses) fully discards
                    // a conditional declaration's own members the moment
                    // its name already exists - correct when the
                    // existing entry is a real, already-known vendor/
                    // first-party class (a `class_exists`-guarded
                    // defensive fallback's members must never leak onto
                    // it), but too conservative when the existing entry
                    // is only `built_in_stub_classes`' type-checking
                    // fiction (jetbrains/phpstorm-stubs, never really
                    // instantiated) and the conditional declaration is
                    // instead a real, version-gated polyfill
                    // implementation whose method bodies still get
                    // checked against whatever won the name: a
                    // conditionally-declared polyfill class colliding
                    // with a native-class stub for the real, newer PHP
                    // version would have its own real method bodies
                    // checked against the stub's much narrower shape,
                    // not the polyfill's real one. Merging (existing
                    // wins any real conflict, same "first/more-
                    // authoritative wins" contract `merge`'s doc comment
                    // establishes) fills in exactly the members the
                    // winning declaration doesn't have, without touching
                    // anything it already provides.
                    //
                    // `extends`/`implements`/`uses`/`templates`/
                    // `extends_args`/`trait_aliases`/`implements_args`
                    // are the one part of `merge` deliberately excluded
                    // here, restored to the winning declaration's own
                    // values right after - unlike a member map
                    // (additive, never wrong to have more of), a
                    // fictitious inheritance edge is actively dangerous:
                    // a conditional polyfill's own guarded `extends`
                    // merging onto a real stub would let a wholly
                    // unrelated third class inherit that never-taken
                    // relationship transitively, wrongly flagging an
                    // unrelated method as overriding a `final` one it
                    // has no real relation to.
                    if let Some(existing) = self.classes.remove(&name) {
                        let (
                            extends,
                            implements,
                            uses,
                            templates,
                            extends_args,
                            trait_aliases,
                            implements_args,
                        ) = (
                            existing.extends.clone(),
                            existing.implements.clone(),
                            existing.uses.clone(),
                            existing.templates.clone(),
                            existing.extends_args.clone(),
                            existing.trait_aliases.clone(),
                            existing.implements_args.clone(),
                        );
                        let mut merged = new_info.merge(existing);
                        merged.extends = extends;
                        merged.implements = implements;
                        merged.uses = uses;
                        merged.templates = templates;
                        merged.extends_args = extends_args;
                        merged.trait_aliases = trait_aliases;
                        merged.implements_args = implements_args;
                        self.classes.insert(name, merged);
                    } else {
                        self.classes.insert(name, new_info);
                    }
                } else if conditional {
                    self.classes.entry(name).or_insert_with(|| new_info);
                } else if self.unconditionally_declared_classes.contains(&name) {
                    // A second unconditional declaration of the same
                    // name - real, not always a mistake. Mirrors
                    // PHPStan's `stubFiles` mechanism, which exists
                    // specifically to let a project redeclare one method
                    // of a real, already-known class as a targeted
                    // override, relying on the merge to fill in
                    // everything else the real declaration provides. A
                    // plain overwrite here would discard the entire real
                    // vendored class - every other method/constant - the
                    // moment a targeted override's own bare `class Foo
                    // { public function bar() {} }` is indexed.
                    if analyzed && !self.built_in_stub_classes.contains(&name) {
                        // An analyzed (first-party) file's class
                        // declaration is ground truth for whatever is
                        // actually being checked - it must always
                        // outright replace a pre-existing context/
                        // vendor/stub entry of the same name, not merge
                        // with it the way two context-only declarations
                        // legitimately do above.
                        self.classes.insert(name, new_info);
                    } else if self.built_in_stub_classes.contains(&name) {
                        // The one exception to the rule just above: a
                        // *built-in* stub name (`ArrayIterator`, ...)
                        // this project already ships its own vendored,
                        // authoritative `@template`-tagged declaration
                        // for. An "unscoped" (`--no-composer`) run has no
                        // analyzed/context split at all, so a real-world
                        // third-party dev-tooling package that happens to
                        // redeclare the same global PHP core class with
                        // *less* information (real shape: a project's own
                        // vendored `jetbrains/phpstorm-stubs` copy, itself
                        // missing `@template` tags this project's own
                        // vendored copy of the identical package has) is
                        // `analyzed: true` there too - the same collision
                        // an ordinary scoped run never even reaches, since
                        // that dev-tooling package is never composer-
                        // autoload-reachable as application code in the
                        // first place. Merging (matching the `conditional`
                        // built-in-stub-collision case above, and for the
                        // identical reason) keeps this project's own more
                        // authoritative `templates`/`extends`/
                        // `implements`/`uses` rather than letting whatever
                        // an accidentally-analyzed vendor package
                        // declares silently discard them - members still
                        // merge additively either way, so a real, legitimate
                        // first-party override of one specific method still
                        // works exactly as before.
                        let existing = self
                                .classes
                                .remove(&name)
                                .expect("name is in unconditionally_declared_classes, so it must already be indexed");
                        let (
                            extends,
                            implements,
                            uses,
                            templates,
                            extends_args,
                            trait_aliases,
                            implements_args,
                        ) = (
                            existing.extends.clone(),
                            existing.implements.clone(),
                            existing.uses.clone(),
                            existing.templates.clone(),
                            existing.extends_args.clone(),
                            existing.trait_aliases.clone(),
                            existing.implements_args.clone(),
                        );
                        let mut merged = existing.merge(new_info);
                        merged.extends = extends;
                        merged.implements = implements;
                        merged.uses = uses;
                        merged.templates = templates;
                        merged.extends_args = extends_args;
                        merged.trait_aliases = trait_aliases;
                        merged.implements_args = implements_args;
                        self.classes.insert(name, merged);
                    } else {
                        let existing = self
                                .classes
                                .remove(&name)
                                .expect("name is in unconditionally_declared_classes, so it must already be indexed");
                        self.classes.insert(name, existing.merge(new_info));
                    }
                } else {
                    // The *first* unconditional declaration of this
                    // name always wins outright over any earlier
                    // conditional stub (`PHPUnit\Framework\TestCase`'s
                    // own real-world shape) - merging here would wrongly
                    // keep a guarded fallback's own members alongside
                    // the real class's.
                    self.unconditionally_declared_classes.insert(name.clone());
                    self.classes.insert(name, new_info);
                }
            }
            BuiltDecl::Enum {
                name,
                conditional,
                info,
            } => {
                if conditional {
                    self.classes.entry(name).or_insert_with(|| info);
                } else {
                    self.classes.insert(name, info);
                }
            }
            BuiltDecl::Function {
                name,
                conditional,
                sig: new_sig,
                site,
            } => {
                if conditional {
                    // Gated on arity and not losing any type precision
                    // the existing slot already had (see
                    // `conditional_declaration_may_replace_stub`'s doc
                    // comment) - keeps the door open for whichever real
                    // conditional declaration actually matches best,
                    // rather than committing to the first one seen
                    // regardless of quality. Runs the identical
                    // comparison whether the existing slot came from a
                    // built-in stub or an earlier ordinary conditional
                    // declaration of the same global function - two
                    // real, non-stub conditional declarations of the
                    // same name must still be tiebroken, not just fall
                    // to a plain first-wins `or_insert_with`.
                    // `.remove(&name)` on a name that was never stub-
                    // provided in the first place is a harmless no-op
                    // (`HashSet::remove` returns `false`, nothing else),
                    // so unconditionally clearing the marker whenever
                    // the new declaration wins is correct either way.
                    // Uses `better_conditional_function_declaration`
                    // (which also lets a real, non-zero-parameter
                    // declaration replace an entirely parameter-less
                    // `existing` one, on top of the exact-arity-match
                    // rule `conditional_declaration_may_replace_stub`
                    // alone still enforces the rest of the time) - safe
                    // to use uniformly for both origins: a stub is
                    // realistically never itself zero-parameter, so this
                    // never weakens the existing stub-protection tests.
                    match self.functions.get(&name) {
                        Some(existing)
                            if !better_conditional_function_declaration(&new_sig, existing) =>
                        {
                            // `existing` already wins this comparison -
                            // leave it in place, matching
                            // `or_insert_with`'s "first one seen (that's
                            // good enough) wins" behavior.
                        }
                        _ => {
                            self.stub_provided_functions
                                .remove(function_key(&name).as_ref());
                            self.function_decl_sites
                                .insert(folded_key(&name), Some(site));
                            self.functions.insert(name, new_sig);
                        }
                    }
                } else if self
                    .unconditionally_declared_functions
                    .contains(function_key(&name).as_ref())
                {
                    // A second unconditional declaration of the same
                    // name - the real-overload shape described on
                    // `unconditionally_declared_functions`'s doc
                    // comment - merges onto the first rather than
                    // replacing it.
                    //
                    // Deliberately does not also clear
                    // `stub_provided_functions` here, unlike the "first
                    // real declaration, replaces outright" branch below:
                    // a vendored stub package's own unconditional,
                    // guard-free declaration of an otherwise built-in
                    // function gets indexed as if it were ordinary
                    // project source, merging harmlessly onto the
                    // identical signature `merge_stubs` already put
                    // there - clearing the marker here would wrongly
                    // tell a later, better-typed conditional polyfill
                    // declaration that the slot is no longer stub-
                    // equivalent and off limits.
                    let existing = self.functions.remove(&name).expect(
                            "name is in unconditionally_declared_functions, so it must already be indexed",
                        );
                    self.function_decl_sites.insert(folded_key(&name), None);
                    self.functions.insert(name, existing.merge(new_sig));
                } else {
                    self.stub_provided_functions
                        .remove(function_key(&name).as_ref());
                    self.unconditionally_declared_functions
                        .insert(folded_key(&name));
                    self.function_decl_sites
                        .insert(folded_key(&name), Some(site));
                    self.functions.insert(name, new_sig);
                }
            }
            BuiltDecl::GlobalConst { name, ty } => {
                register_global_constant(&mut self.global_constants, name, ty);
            }
        }
    }

    /// Populates `names_with_assert_tags` from `decl`'s signature(s) -
    /// see that field's doc comment. Reads `decl` only, never the
    /// collision-resolution decision `insert_built_decl` goes on to make
    /// right after this call - a name is recorded whenever any indexed
    /// declaration under that name ever had a non-empty `assert` list,
    /// whether or not it ends up winning the registry slot (a losing
    /// declaration's `assert` info being recorded anyway is a harmless,
    /// rare over-approximation, never a missed real one).
    fn record_assert_tagged_names(&mut self, decl: &BuiltDecl) {
        match decl {
            BuiltDecl::Function { name, sig, .. } => {
                if !sig.assert.is_empty() {
                    // `name` is the fully-qualified name
                    // (`Registry::functions`' key shape); store just
                    // the bare, unqualified tail so a bare-statement
                    // `Call`'s as-written callee text - not yet FQN-
                    // resolved at that point, see
                    // `apply_bare_statement_assert_narrowing`'s Call
                    // arm - can check membership without resolving it
                    // first, the same bare-name shape method names
                    // already are.
                    let bare = name.rsplit('\\').next().unwrap_or(name);
                    self.names_with_assert_tags.insert(intern(bare));
                }
            }
            BuiltDecl::Class { info, .. } | BuiltDecl::Enum { info, .. } => {
                for (method_name, sig) in &info.methods {
                    if !sig.assert.is_empty() {
                        self.names_with_assert_tags.insert(method_name.clone());
                    }
                }
            }
            BuiltDecl::GlobalConst { .. } => {}
        }
    }

    /// Surgically removes one file's prior contributions before
    /// re-indexing its new parse - `Project::update_file`'s whole reason
    /// for existing. `functions`/`classes` must already be the
    /// fully-qualified names `Registry::index` actually keyed its entries
    /// under (see `NameContext`) - `project.rs`'s
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
            self.functions.remove(name.as_str());
            // Stay in lockstep with `functions`, for the identical
            // reason the class side does below - otherwise a later
            // re-index of the same name would find a stale "already
            // unconditionally declared" marker with nothing left in
            // `functions` for it to point at.
            self.unconditionally_declared_functions
                .remove(function_key(name).as_ref());
        }
        for name in classes {
            self.classes.remove(name.as_str());
            // Must stay in lockstep with `classes` - otherwise a
            // later re-index of the same name (incremental re-checking's
            // whole reason for calling this) would find a stale "already
            // unconditionally declared" marker with nothing left in
            // `classes` for it to point at, wrongly taking the merge path
            // (`insert_built_decl`'s `BuiltDecl::Class` arm) against a
            // now-nonexistent entry instead of a clean first insertion.
            self.unconditionally_declared_classes.remove(name.as_str());
            // Must stay in lockstep with `classes` too, for the identical
            // reason - otherwise a later re-index would find a stale
            // declaring-file entry pointing at a name that may no longer
            // be `PHPStan\*`-namespaced at all (or may not exist).
            self.phpstan_class_files.remove(name);
        }
    }

    /// Records `name`'s declaring file path, for `is_phpstan_code`'s
    /// vendor-directory check - only ever populated for a name already
    /// passing `is_phpstan_namespace`, so this is a zero-cost no-op for
    /// every ordinary corpus.
    pub(crate) fn record_phpstan_class_file(&mut self, name: &str, path: &std::path::Path) {
        if !crate::registry::is_phpstan_namespace(name) {
            return;
        }
        self.phpstan_class_files
            .insert(name.to_string(), path.to_path_buf());
    }

    /// User-defined symbols always win: a stub only fills in a name that
    /// isn't already declared in the indexed source. Unlike
    /// `merge_stub_classes` below, this is not a merge, on purpose: a
    /// user-declared function sharing a builtin's name is a distinct
    /// definition the user's own code should win outright, not a second
    /// "overload" of the same conceptual function the way phpstorm-stubs'
    /// `strtr`-style split declarations are.
    ///
    /// Every merged-in name is also recorded in
    /// `unconditionally_declared_functions` regardless - not for this call
    /// itself (which never merges), but so a later real project file
    /// legitimately declaring a function with the same name as a builtin
    /// correctly takes `insert_built_decl`'s merge path (the `strtr`
    /// case) instead of blindly overwriting.
    pub fn merge_stubs<S: Into<Arc<str>>>(&mut self, stubs: Vec<(S, FunctionReflection)>) {
        for (name, sig) in stubs {
            let name: Arc<str> = name.into();
            self.unconditionally_declared_functions
                .insert(folded_key(&name));
            self.stub_provided_functions.insert(folded_key(&name));
            self.functions.insert_if_absent(name, sig);
        }
    }

    /// Same "user-defined wins" rule as `merge_stubs`, for classes -
    /// without this, built-in classes (`Exception`, `ArrayObject`, ...)
    /// would never be known to the registry at all.
    ///
    /// Every merged-in name is also recorded in
    /// `unconditionally_declared_classes` - this runs once, in
    /// `Project::new`, before any real project file is indexed, so a
    /// built-in class is already present by the time `insert_built_decl`
    /// later processes a project's own `#[stubFiles]`-style override (a
    /// bare `class Imagick { public function convolveImage(...) {} }`
    /// meaning to patch one method). Without this, `insert_built_decl`'s
    /// merge logic would see "no unconditional declaration recorded for
    /// this name yet" and wholesale-replace the entire built-in class
    /// with the override's bare one-method version. The merge itself
    /// keeps this function's "user-defined always wins" contract robust
    /// even if user code was somehow indexed before this runs - existing
    /// user info is `other` in `ClassReflection::merge`, so it still wins any
    /// member-name conflict.
    pub fn merge_stub_classes<S: Into<Arc<str>>>(&mut self, stubs: Vec<(S, ClassReflection)>) {
        for (name, info) in stubs {
            let name: Arc<str> = name.into();
            self.unconditionally_declared_classes.insert(name.clone());
            self.built_in_stub_classes.insert(name.clone());
            match self.classes.remove(&name) {
                Some(existing) => {
                    self.classes.insert(name, info.merge(existing));
                }
                None => {
                    self.classes.insert(name, info);
                }
            }
        }
    }

    /// The constant-side sibling of `merge_stub_classes`/`merge_stubs` -
    /// unlike those two (first-occurrence-wins across the whole corpus,
    /// since a stub function/class conflict is resolved by this
    /// project's conditional-declaration tiebreaking), a global constant
    /// has no such tiebreaking mechanism, so this reuses
    /// `register_global_constant`'s "poison to Mixed on any real
    /// disagreement" conflict policy - the same policy already used
    /// within one file's `global_constants` map, applied here across
    /// every vendored stub file at once.
    pub fn merge_stub_constants(&mut self, stubs: Vec<(Arc<str>, Type)>) {
        for (name, ty) in stubs {
            register_global_constant(&mut self.global_constants, name, ty);
        }
    }
}

/// Pure: walks `stmts` with the Namespace/Use/If/Block/GlobalConst/
/// Expression(`define()`) recursion, calling only
/// `ClassReflection::from_decl`/`from_enum_decl`/`FunctionReflection::from_decl`/
/// `infer_const_type`/`define_call_constant`, collecting each result
/// into `BuiltDecl`s instead of mutating a registry. No `&self`/
/// `&Registry` access anywhere in this function or anything it calls, so
/// it's safe to call concurrently for many files at once -
/// `check_project.rs`'s parallel parse stage does exactly that, once per
/// file, inside the same rayon pool `project::parse_file` runs in.
/// Returns every declaration `stmts` contributes, in depth-first order -
/// `Registry::insert_built_decl` must see them fed back in this order
/// (single-file callers get that for free from `Registry::index`'s own
/// loop; the parallel batch path must preserve per-file order too,
/// though it may interleave whole files in any order relative to each
/// other - see `insert_parsed_file`'s doc comment).
///
/// Tracks the current `NameContext` positionally as it walks - a
/// `namespace` statement changes it for everything from that point on,
/// exactly like real PHP, rather than computing one static context up
/// front for the whole file. This matters concretely: the parser has no
/// real model of brace-delimited `namespace X { ... }` (see
/// `StmtKind::Namespace`'s parsing comment), so it falls out as a
/// `StmtKind::Namespace(name)` immediately followed by a sibling
/// `StmtKind::Block` holding what was inside the braces - and real
/// vendored stub files can have two such blocks in one file
/// (`namespace { ... }` for a global class, then a later
/// `namespace Pdo { ... }` for a version-gated subclass) - a single
/// whole-file context would apply whichever one it saw last to both
/// blocks, silently mis-qualifying the first one's classes.
#[hotpath::measure]
pub(crate) fn build_declarations(stmts: &[Stmt], origin: &Origin) -> Vec<BuiltDecl> {
    let mut declarations = Declarations {
        origin,
        ctx: NameContext::default(),
        conditional: false,
        out: Vec::new(),
    };
    walk_stmts(&mut declarations, stmts);
    declarations.out
}

/// Every declaration in a file, wherever it sits: PHP declares a class,
/// function or constant when execution reaches it, so one inside a
/// function body, a method, a loop, a `try` or a closure exists once that
/// runs. A declaration reached only that way, or in an `if` branch (the
/// `if (!function_exists('foo')) { function foo() {} }` polyfill idiom),
/// is `conditional`; nothing downstream knows which conditions are
/// guards. The name context is the file's, as the walk reaches each
/// point: namespaces and imports aren't scoped to blocks or functions.
struct Declarations<'a> {
    origin: &'a Origin,
    ctx: NameContext,
    conditional: bool,
    out: Vec<BuiltDecl>,
}

impl Declarations<'_> {
    /// Walks with every declaration found conditional.
    fn conditionally(&mut self, walk: impl FnOnce(&mut Self)) {
        let was = std::mem::replace(&mut self.conditional, true);
        walk(self);
        self.conditional = was;
    }
}

impl<'ast> Visitor<'ast> for Declarations<'_> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        syntax::stack::guard(|| match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => self.ctx.add_uses(imports),
            StmtKind::ClassDecl(c) => {
                let name = intern(&self.ctx.declared_name(class_decl_name(c)));
                let conditional = self.conditional;
                if c.kind == syntax::ast::ClassKind::Enum {
                    let mut info = ClassReflection::from_enum_decl(c, &self.ctx);
                    info.record_provenance(
                        self.origin,
                        &name,
                        c.name_span,
                        &MemberSpans::of_enum(c),
                    );
                    self.out.push(BuiltDecl::Enum {
                        name,
                        conditional,
                        info,
                    });
                } else {
                    let mut info = ClassReflection::from_decl(c, &self.ctx);
                    info.record_provenance(
                        self.origin,
                        &name,
                        c.name_span,
                        &MemberSpans::of_class(c),
                    );
                    self.out.push(BuiltDecl::Class {
                        name,
                        conditional,
                        info,
                    });
                }
                self.conditionally(|v| walk_class_decl(v, c));
            }
            StmtKind::FunctionDecl(f) => {
                let name = intern(&self.ctx.declared_name(&f.name));
                let mut sig = FunctionReflection::from_decl(
                    f,
                    &self.ctx,
                    &rustc_hash::FxHashMap::default(),
                    &rustc_hash::FxHashMap::default(),
                    Visibility::Public,
                );
                sig.provenance = Provenance::new(self.origin.clone(), f.name_span);
                self.out.push(BuiltDecl::Function {
                    name,
                    conditional: self.conditional,
                    sig,
                    site: (f.name_span.start.line, f.name_span.start.col),
                });
                self.conditionally(|v| walk_function_decl(v, f));
            }
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    let name = intern(&self.ctx.declared_name(&c.name));
                    let ty = infer_const_type(&c.value);
                    self.out.push(BuiltDecl::GlobalConst { name, ty });
                }
            }
            // A braced namespace body runs as the file does.
            StmtKind::Block(_) | StmtKind::Declare { .. } => walk_stmt(self, stmt),
            _ => self.conditionally(|v| walk_stmt(v, stmt)),
        })
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        if let Some((name, ty)) = define_call_constant(e) {
            self.out.push(BuiltDecl::GlobalConst { name, ty });
        }
        self.conditionally(|v| walk_expr(v, e));
    }
}

/// Recognizes a top-level `define('NAME', value)` (or `\define(...)`)
/// call statement, returning the constant's literal name plus its
/// inferred value type (`infer_const_type`, the same literal-only scope
/// a `const` declaration's value already gets). `None` for anything else
/// a bare `StmtKind::Expression` could be, a call to some other function,
/// a spread/named argument (real `define()` never takes either), a
/// non-literal first argument (a dynamic constant name this project has
/// no way to know without evaluating it), or a name that's really the
/// case-insensitive language keyword `true`/`false`/`null` rather than
/// an ordinary constant. On a real PHP runtime these three names are
/// parser-level keywords resolved before `define()` could ever apply to
/// them - the call itself is syntactically valid but semantically
/// inert, never actually redefining what the bare token
/// `true`/`false`/`null` means anywhere. Registering them anyway would
/// let a global lookup miss silently pre-empt `bare_name_literal_type`'s
/// already-correct handling of a fully-qualified `\false`/`\true`/
/// `\null` reference for every file in the corpus - every `!== \false`-
/// style narrowing idiom would silently stop narrowing away `false` at
/// all once such a stub was indexed.
fn define_call_constant(e: &Expr) -> Option<(Arc<str>, Type)> {
    let ExprKind::Call(callee, args) = &e.kind else {
        return None;
    };
    let ExprKind::Name(name) = &callee.kind else {
        return None;
    };
    if !name
        .rsplit('\\')
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("define")
    {
        return None;
    }
    if args.len() < 2 || args.iter().any(|a| a.spread || a.name.is_some()) {
        return None;
    }
    let ExprKind::Literal(Literal::String(const_name)) = &args[0].value.kind else {
        return None;
    };
    let const_name = const_name.as_str()?;
    if ["true", "false", "null"]
        .iter()
        .any(|kw| const_name.eq_ignore_ascii_case(kw))
    {
        return None;
    }
    // Vendored stubs commonly `define('__DIR__', '', true);
    // define('__LINE__', 0, true); ...` (and identically for
    // `__FILE__`/`__CLASS__`/`__FUNCTION__`/`__METHOD__`/`__TRAIT__`/
    // `__NAMESPACE__`/`__PROPERTY__`) - the same class of problem the
    // `true`/`false`/`null` exclusion above closes, for a different
    // reason: these nine names are real PHP magic constants, resolved by
    // the parser itself from the surrounding source location, never
    // actually backed by a real `define()`-registered value - the
    // stub's placeholder value (an empty string, or `0` for `__LINE__`)
    // exists purely so the declaration type-checks, and was never meant
    // to be read literally. This project has no dedicated magic-constant
    // `Expr` kind (a bare `__DIR__`/`__LINE__`/... reference parses as
    // an ordinary `ExprKind::Name`, already `mixed` via
    // `bare_name_literal_type`'s default arm) - excluding them here
    // protects that existing, already-correct "don't know" behavior
    // from being overwritten by the stub's placeholder.
    if [
        "__DIR__",
        "__FILE__",
        "__LINE__",
        "__CLASS__",
        "__FUNCTION__",
        "__METHOD__",
        "__TRAIT__",
        "__NAMESPACE__",
        "__PROPERTY__",
    ]
    .contains(&const_name)
    {
        return None;
    }
    // A vendored stub's `define('PHP_INT_MIN', -9223372036854775808);`
    // needs special handling: `9223372036854775808` (the positive
    // magnitude before the unary minus applies) is exactly one past
    // `i64::MAX`, so the lexer correctly represents it as a
    // `Literal::Float`, the same way real PHP's own literal-overflow
    // rule does for this exact source text (`var_dump(-9223372036854775808)`
    // truly prints a float, not `PHP_INT_MIN`'s real int value).
    // `infer_const_type`'s `Neg` arm would then type the whole
    // expression `float`, silently breaking every
    // `random_int(PHP_INT_MIN, PHP_INT_MAX)`-shaped call. `PHP_INT_MAX`
    // itself needs no equivalent exclusion - `9223372036854775807` fits
    // exactly, no `Neg`/overflow involved.
    //
    // Returning `None` here would mean "don't know this constant's
    // type, fall back to `Mixed` at every read site" - a safe default
    // for ordinary constant-value inference, but indistinguishable from
    // "this constant doesn't exist at all" for the `use const`
    // existence check (`check_use_const_imports`), which only sees
    // whether a name made it into `global_constants`. `PHP_INT_MIN` is
    // a real, always-defined PHP constant, so a real
    // `use const PHP_INT_MIN;` should never be flagged
    // `constant.notFound`. `int` (not a literal, and deliberately
    // not `infer_const_type`'s own float-overflow-affected inference)
    // is the safe, correct level of precision.
    if const_name == "PHP_INT_MIN" {
        return Some((Arc::from(const_name), Type::int()));
    }
    Some((Arc::from(const_name), infer_const_type(&args[1].value)))
}

/// Records one `const`/`define()` declaration site for `name`,
/// reconciling it against anything already recorded rather than letting
/// whichever site happened to index first win unconditionally. Two
/// files can legitimately define the same global constant name to
/// different values for unrelated reasons (a genuinely dynamic,
/// config-driven production value vs. a fixed value for test isolation),
/// so a plain first-wins insert would let file-indexing order alone
/// decide which one this registry believes, silently marking an
/// otherwise-live `if` branch unreachable wherever the constant is
/// compared against a literal. Any two declarations that don't perfectly
/// agree, including a first declaration that's itself unknowable
/// (`Mixed`), poison the entry to `Mixed` instead, the same "don't guess
/// wrong" leniency every other real ambiguity in this registry resolves
/// in favor of no information over wrong information. Once poisoned, a
/// later, more literal-looking declaration can never un-poison it back
/// to a specific type, since the true value is still just as
/// undetermined regardless of which site happens to run at this
/// constant's real, load-order-dependent definition time.
pub(crate) fn register_global_constant(
    map: &mut FxHashMap<Arc<str>, Type>,
    name: Arc<str>,
    ty: Type,
) {
    match map.get(&name) {
        None => {
            map.insert(name, ty);
        }
        Some(existing) if *existing == ty && !ty.is_mixed() => {}
        Some(_) => {
            map.insert(name, Type::mixed());
        }
    }
}

/// Whether a conditional declaration (`new_sig`) is trustworthy enough
/// to replace `existing` (a stub-provided signature still sitting in its
/// slot, per `stub_provided_functions`) - this needs two separate gates,
/// not just one.
///
/// Arity: a `func_get_args()`-forwarding shim (zero declared params) can
/// compete with the real, correctly-typed polyfill declaration for the
/// same name - letting the shim win would leave the real declaration's
/// own body-check zipping its one real parameter against the shim's
/// zero-param signature, silently losing it (read as undefined inside
/// its own body).
///
/// Type precision: even at matching arity, a polyfill package can bundle
/// several PHP-version-gated variants of the same function behind
/// separate `function_exists()` guards, of visibly different quality
/// (an untyped bootstrap variant alongside a fully-typed PHP 8.0+ one
/// for the same name). Letting the untyped one win would silently
/// discard real, already-known type information the stub itself
/// carried, and since every other real file's call sites resolve
/// through this one shared registry entry, the damage isn't confined to
/// whichever file lost the race. Never letting a replacement downgrade
/// a parameter or return position that the stub already had typed down
/// to `Mixed` closes this without needing to fully rank two arbitrary
/// types against each other.
fn conditional_declaration_may_replace_stub(
    new_sig: &FunctionReflection,
    existing: &FunctionReflection,
) -> bool {
    if new_sig.params.len() != existing.params.len() {
        return false;
    }
    if !existing.return_type.is_mixed() && new_sig.return_type.is_mixed() {
        return false;
    }
    existing
        .params
        .iter()
        .zip(&new_sig.params)
        .all(|(old, new)| !(!old.ty.is_mixed() && new.ty.is_mixed()))
}

/// The identical question as `conditional_declaration_may_replace_stub`
/// above, but for two ordinary conditional declarations of the same
/// global function name - neither one an already-known-good stub worth
/// being this conservative about protecting. Reuses that same exact-
/// arity-match comparison when arities agree, but additionally lets a
/// real, non-zero-parameter declaration replace an `existing` one that's
/// entirely parameter-less - a `func_get_args()`-forwarding shim
/// competing with the real, correctly-typed declaration for the same
/// name: the strict arity-match rule alone would refuse the swap in
/// either direction, permanently freezing in whichever one happened to
/// be indexed first. The reverse (a real declaration already won the
/// slot, a later 0-param shim trying to displace it) stays correctly
/// refused: a 0-param `new_sig` against a non-zero-param `existing`
/// still fails the ordinary arity-match check, and
/// `existing.params.is_empty()` is false, so neither branch fires for it.
fn better_conditional_function_declaration(
    new_sig: &FunctionReflection,
    existing: &FunctionReflection,
) -> bool {
    if existing.params.is_empty() && !new_sig.params.is_empty() {
        return true;
    }
    conditional_declaration_may_replace_stub(new_sig, existing)
}
