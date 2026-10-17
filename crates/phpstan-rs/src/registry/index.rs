use rustc_hash::FxHashMap;
use std::sync::Arc;
use syntax::name::Name;
use syntax::name::NameMap;

use crate::file_id::FileId;
use crate::registry::Registry;
use crate::registry::class_info::MemberSpans;
use crate::registry::guard::{Guard, GuardExpr};
use crate::registry::provenance::{BindFile, Origin, Provenance};
use crate::registry::{
    ClassReflection, FunctionReflection, folded_key, function_key, infer_const_type,
};
use syntax::ast::{Expr, Stmt, Visibility};
use syntax::declarations::{Declaration, Declared, GuardTerm, for_each_declaration};
use syntax::name_resolution::NameContext;
use syntax::php_string::PhpString;
use types::Type;

/// One declaration's already-built, registry-independent info/sig, plus
/// the file-position facts (`name`, `conditional`) `Registry::
/// insert_built_decl` needs to decide how it interacts with whatever's
/// already in the registry - the output of `build_declarations`, meant
/// to be consumed one at a time, in order, by `insert_built_decl`. See
/// `build_declarations`'s own doc comment for why this exists as a
/// wholly separate step from the insertion logic below.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum BuiltDecl {
    Class {
        name: Name,
        conditional: bool,
        guard: Option<GuardExpr>,
        info: Arc<ClassReflection>,
    },
    Function {
        name: Name,
        conditional: bool,
        guard: Option<GuardExpr>,
        sig: Arc<FunctionReflection>,
        // The declaration's name position, as a byte offset.
        site: u32,
    },
    GlobalConst {
        name: Name,
        guard: Option<GuardExpr>,
        ty: Type,
    },
}

impl BuiltDecl {
    /// Gives the declaration its file's number, once that is known. The
    /// declarations of a file are built pending their file's number.
    pub(crate) fn bind_file(&mut self, file: FileId) {
        match self {
            BuiltDecl::Class { info, .. } if info.provenance().is_pending() => {
                Arc::make_mut(info).bind_file(file);
            }
            BuiltDecl::Function { sig, .. } if sig.provenance().is_pending() => {
                Arc::make_mut(sig).bind_file(file);
            }
            _ => {}
        }
    }

    /// Whether the declaration has its file's number, which `bind_file` gives.
    pub(crate) fn is_bound(&self) -> bool {
        match self {
            BuiltDecl::Class { info, .. } => !info.provenance().is_pending(),
            BuiltDecl::Function { sig, .. } => !sig.provenance().is_pending(),
            BuiltDecl::GlobalConst { .. } => true,
        }
    }

    /// The name the declaration declares.
    pub(crate) fn name(&self) -> &Name {
        match self {
            BuiltDecl::Class { name, .. }
            | BuiltDecl::Function { name, .. }
            | BuiltDecl::GlobalConst { name, .. } => name,
        }
    }

    /// What the declaration declares: its kind and name.
    pub(crate) fn kind_and_name(&self) -> (DeclKind, &str) {
        match self {
            BuiltDecl::Class { name, .. } => (DeclKind::Class, name),
            BuiltDecl::Function { name, .. } => (DeclKind::Function, name),
            BuiltDecl::GlobalConst { name, .. } => (DeclKind::Constant, name),
        }
    }

    /// Whether the declaration's guard holds on `versions`: `Maybe` for
    /// one with no guard to read.
    fn guard_on(&self, versions: Option<types::PhpVersions>) -> Guard {
        let guard = match self {
            BuiltDecl::Class { guard, .. }
            | BuiltDecl::Function { guard, .. }
            | BuiltDecl::GlobalConst { guard, .. } => guard,
        };
        guard.as_ref().map_or(Guard::Maybe, |g| g.on(versions))
    }

    /// The declaration as it takes effect on `versions`: `None` for one whose
    /// guard the versions rule out, which never runs and changes nothing, and
    /// otherwise the declaration itself, made unconditional when its guard
    /// always holds.
    pub(crate) fn in_effect(self, versions: Option<types::PhpVersions>) -> Option<BuiltDecl> {
        match self.guard_on(versions) {
            Guard::Never => None,
            Guard::Always => Some(self.made_unconditional()),
            Guard::Maybe => Some(self),
        }
    }

    /// The declaration as one that always runs.
    fn made_unconditional(mut self) -> BuiltDecl {
        match &mut self {
            BuiltDecl::Class { conditional, .. } | BuiltDecl::Function { conditional, .. } => {
                *conditional = false;
            }
            BuiltDecl::GlobalConst { .. } => {}
        }
        self
    }
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
    /// (rather than inlining this loop at every call site) so the
    /// single-file callers need no changes at all. A load's batch path
    /// builds each file's declarations as the files parse, and merges them
    /// with `Registry::merge_declarations`, which applies the same
    /// `DeclTables::insert` to each name in the same order. See
    /// `insert_built_decl`'s own doc comment for why splitting it out
    /// this way - rather than writing a second, independent tree-walker
    /// for the parallel path - matters here specifically.
    #[hotpath::measure]
    pub(crate) fn index(&mut self, stmts: &[Stmt], origin: &Origin, analyzed: bool) {
        for decl in build_declarations(stmts, origin) {
            self.insert_built_decl(decl, analyzed);
        }
    }

    /// Applies one already-built declaration to the registry's tables, the
    /// way `DeclTables::insert` does over the tables it is given.
    pub(crate) fn insert_built_decl(&mut self, decl: BuiltDecl, analyzed: bool) {
        let Some(decl) = decl.in_effect(self.php_versions) else {
            return;
        };
        for_each_assert_tagged_name(&decl, |name| {
            self.names_with_assert_tags.insert(name);
        });
        let recording = self.raw.is_some();
        DeclTables {
            classes: &mut self.classes,
            functions: &mut self.functions,
            global_constants: &mut self.global_constants,
            built_in_stub_classes: &self.built_in_stub_classes,
            unconditionally_declared_classes: &mut self.unconditionally_declared_classes,
            unconditionally_declared_functions: &mut self.unconditionally_declared_functions,
            stub_provided_functions: &mut self.stub_provided_functions,
            function_decl_sites: &mut self.function_decl_sites,
            changed: recording.then_some(&mut self.changed),
        }
        .insert(decl, analyzed);
    }
}

/// The registry tables one declaration's insert reads and writes, borrowed
/// from wherever they are kept. `insert` holds the one implementation of
/// every collision decision. `changed` is where changed names are recorded,
/// and `None` where they are not.
pub(crate) struct DeclTables<'a> {
    pub(crate) classes: &'a mut super::NameTable<ClassReflection>,
    pub(crate) functions: &'a mut super::FunctionTable,
    pub(crate) global_constants: &'a mut super::NameMap<Type>,
    pub(crate) built_in_stub_classes: &'a super::NameTable<()>,
    pub(crate) unconditionally_declared_classes: &'a mut super::NameSet,
    pub(crate) unconditionally_declared_functions: &'a mut super::NameSet,
    pub(crate) stub_provided_functions: &'a mut super::NameSet,
    pub(crate) function_decl_sites: &'a mut super::NameMap<Option<u32>>,
    pub(crate) changed: Option<&'a mut super::ChangedNames>,
}

impl DeclTables<'_> {
    /// Applies one already-built declaration (see `BuiltDecl`/
    /// `build_declarations`) that takes effect (`BuiltDecl::in_effect`) to
    /// the registry. The sequential path (`insert_built_decl`) and the batch
    /// merge (`declaration_merge`) both apply declarations through this one
    /// implementation of every FQN-collision decision below, which is
    /// documented per-branch.
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
    pub(crate) fn insert(&mut self, decl: BuiltDecl, analyzed: bool) {
        // A name is changed when its entry is: a conditional declaration the
        // existing entry outranks leaves the table as it was.
        let leaves_entry = match &decl {
            BuiltDecl::Class {
                name,
                conditional: true,
                ..
            } => !self.built_in_stub_classes.contains_key(name) && self.classes.contains_key(name),
            BuiltDecl::Function {
                name,
                conditional: true,
                sig,
                ..
            } => self
                .functions
                .get(name)
                .is_some_and(|existing| !better_conditional_function_declaration(sig, existing)),
            _ => false,
        };
        if !leaves_entry {
            match &decl {
                BuiltDecl::Class { name, .. } => self.record_changed_class(name),
                BuiltDecl::Function { name, .. } => self.record_changed_function(name),
                BuiltDecl::GlobalConst { name, .. } => self.record_changed_constant(name),
            }
        }
        match decl {
            BuiltDecl::Class {
                name,
                conditional,
                info: new_info,
                ..
            } => {
                if conditional && self.built_in_stub_classes.contains_key(&name) {
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
                    if let Some(existing) = self.classes.take(&name) {
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
                        let mut merged = Arc::unwrap_or_clone(new_info).merge(existing);
                        merged.extends = extends;
                        merged.implements = implements;
                        merged.uses = uses;
                        merged.templates = templates;
                        merged.extends_args = extends_args;
                        merged.trait_aliases = trait_aliases;
                        merged.implements_args = implements_args;
                        self.classes.insert(name, merged);
                    } else {
                        self.classes.insert_shared(name, new_info);
                    }
                } else if conditional {
                    if !self.classes.contains_key(&name) {
                        self.classes.insert_shared(name, new_info);
                    }
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
                    if analyzed && !self.built_in_stub_classes.contains_key(&name) {
                        // An analyzed (first-party) file's class
                        // declaration is ground truth for whatever is
                        // actually being checked - it must always
                        // outright replace a pre-existing context/
                        // vendor/stub entry of the same name, not merge
                        // with it the way two context-only declarations
                        // legitimately do above.
                        self.classes.insert_shared(name, new_info);
                    } else if self.built_in_stub_classes.contains_key(&name) {
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
                                .take(&name)
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
                        let mut merged = existing.merge(Arc::unwrap_or_clone(new_info));
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
                                .take(&name)
                                .expect("name is in unconditionally_declared_classes, so it must already be indexed");
                        self.classes
                            .insert(name, existing.merge(Arc::unwrap_or_clone(new_info)));
                    }
                } else {
                    // The *first* unconditional declaration of this
                    // name always wins outright over any earlier
                    // conditional stub (`PHPUnit\Framework\TestCase`'s
                    // own real-world shape) - merging here would wrongly
                    // keep a guarded fallback's own members alongside
                    // the real class's.
                    self.unconditionally_declared_classes.insert(name.clone());
                    self.classes.insert_shared(name, new_info);
                }
            }
            BuiltDecl::Function {
                name,
                conditional,
                sig: new_sig,
                site,
                ..
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
                            self.functions.insert_shared(name, new_sig);
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
                    let existing = self.functions.take(&name).expect(
                            "name is in unconditionally_declared_functions, so it must already be indexed",
                        );
                    self.function_decl_sites.insert(folded_key(&name), None);
                    self.functions
                        .insert(name, existing.merge(Arc::unwrap_or_clone(new_sig)));
                } else {
                    self.stub_provided_functions
                        .remove(function_key(&name).as_ref());
                    self.unconditionally_declared_functions
                        .insert(folded_key(&name));
                    self.function_decl_sites
                        .insert(folded_key(&name), Some(site));
                    self.functions.insert_shared(name, new_sig);
                }
            }
            BuiltDecl::GlobalConst { name, ty, .. } => {
                register_global_constant(self.global_constants, name, ty);
            }
        }
    }

    fn record_changed_class(&mut self, name: &str) {
        if let Some(changed) = &mut self.changed {
            changed.record_class(name);
        }
    }

    fn record_changed_function(&mut self, name: &str) {
        if let Some(changed) = &mut self.changed {
            changed.record_function(name);
        }
    }

    fn record_changed_constant(&mut self, name: &str) {
        if let Some(changed) = &mut self.changed {
            changed.record_constant(name, || self.global_constants.get(name).cloned());
        }
    }
}

/// Calls `record` with each name `decl` puts in `names_with_assert_tags`.
/// Reads `decl` only, never the collision decision `DeclTables::insert` goes
/// on to make: a name is recorded whenever any indexed declaration under it
/// had a non-empty `assert` list, whether or not that declaration wins the
/// registry slot (a losing declaration's `assert` info being recorded anyway
/// is a harmless, rare over-approximation, never a missed real one).
pub(crate) fn for_each_assert_tagged_name(decl: &BuiltDecl, mut record: impl FnMut(Name)) {
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
                record(Name::from(bare));
            }
        }
        BuiltDecl::Class { info, .. } => {
            for (method_name, sig) in info.methods.iter() {
                if !sig.assert.is_empty() {
                    record(method_name.clone());
                }
            }
        }
        BuiltDecl::GlobalConst { .. } => {}
    }
}

impl Registry {
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
    pub(crate) fn remove_declarations(&mut self, functions: &[Name], classes: &[Name]) {
        for name in functions {
            self.record_changed_function(name);
            self.functions.remove(name.as_str());
            // Stay in lockstep with `functions`, for the identical
            // reason the class side does below - otherwise a later
            // re-index of the same name would find a stale "already
            // unconditionally declared" marker with nothing left in
            // `functions` for it to point at.
            self.unconditionally_declared_functions
                .remove(function_key(name).as_ref());
            self.function_decl_sites.remove(function_key(name).as_ref());
        }
        for name in classes {
            self.record_changed_class(name);
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
    /// passing `is_phpstan_namespace`, so `path` is called for those names
    /// alone.
    pub(crate) fn record_phpstan_class_file(
        &mut self,
        name: &str,
        path: impl FnOnce() -> std::path::PathBuf,
    ) {
        if !crate::registry::is_phpstan_namespace(name) {
            return;
        }
        self.phpstan_class_files.insert(name.to_string(), path());
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
    pub fn merge_stubs<S: Into<Name>>(&mut self, stubs: Vec<(S, FunctionReflection)>) {
        for (name, sig) in stubs {
            let name: Name = name.into();
            self.record_changed_function(&name);
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
    pub fn merge_stub_classes<S: Into<Name>>(&mut self, stubs: Vec<(S, ClassReflection)>) {
        for (name, info) in stubs {
            let name: Name = name.into();
            self.record_changed_class(&name);
            self.unconditionally_declared_classes.insert(name.clone());
            self.built_in_stub_classes.insert(name.clone(), ());
            match self.classes.take(&name) {
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
    pub fn merge_stub_constants(&mut self, stubs: Vec<(Name, Type)>) {
        for (name, ty) in stubs {
            self.record_changed_constant(&name);
            register_global_constant(&mut self.global_constants, name, ty);
        }
    }

    /// Removes the global constant `name`.
    pub(crate) fn remove_global_constant(&mut self, name: &str) {
        self.record_changed_constant(name);
        self.global_constants.remove(name);
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
/// Returns every declaration `stmts` contributes, in depth-first order,
/// then its anonymous classes (`anonymous_class_decls`) -
/// `Registry::insert_built_decl` must see them fed back in this order
/// (`Registry::index`'s own loop gives that, and `Registry::merge_declarations`
/// gives it over a batch of files, file by file in the order
/// `Project::insert_declarations` is given them).
///
/// Each declaration's PHPDoc resolves in the name context in effect where
/// it sits (`for_each_declaration`).
#[hotpath::measure]
pub(crate) fn build_declarations(stmts: &[Stmt], origin: &Origin) -> Vec<BuiltDecl> {
    build_declarations_where(stmts, origin, None)
}

/// What `build_declarations_where` asks `keep` about.
#[derive(Clone, Copy)]
pub(crate) enum DeclKind {
    Class,
    Function,
    Constant,
}

/// What decides which declarations `build_declarations_where` builds.
pub(crate) type Keep<'a> = &'a dyn Fn(DeclKind, &str) -> bool;

/// `build_declarations`, building only the declarations `keep` accepts,
/// asked by kind and name before the declaration is built. Without a
/// `keep` every declaration is built. With one, the anonymous classes are
/// not: their names can never be a declared name's, which `keep` is
/// about.
pub(crate) fn build_declarations_where(
    stmts: &[Stmt],
    origin: &Origin,
    keep: Option<Keep<'_>>,
) -> Vec<BuiltDecl> {
    let mut out = Vec::new();
    for_each_declaration(stmts, |declaration, ctx, guards| {
        built_declaration(declaration, ctx, guards, origin, keep, &mut out);
    });
    if keep.is_none() {
        out.extend(crate::registry::anonymous_class_decls(stmts, origin));
    }
    out
}

/// The declaration `declaration` builds into, pushed on `out`: nothing
/// when `keep` refuses its kind and name. `ctx` is the name context in
/// effect where it sits and `guards` the terms that guard it.
pub(crate) fn built_declaration(
    declaration: Declaration<'_>,
    ctx: &NameContext,
    guards: &[GuardTerm<'_>],
    origin: &Origin,
    keep: Option<Keep<'_>>,
    out: &mut Vec<BuiltDecl>,
) {
    let kept = |kind: DeclKind, name: &str| keep.is_none_or(|keep| keep(kind, name));
    let conditional = declaration.conditional;
    let guard = if conditional {
        GuardExpr::of_terms(guards)
    } else {
        None
    };
    match declaration.declared {
        Declared::Class(c) => {
            let name = declaration.name();
            if !kept(DeclKind::Class, &name) {
                return;
            }
            let name = Name::from(&name);
            let mut info = ClassReflection::from_decl(c, ctx);
            info.record_provenance(origin, &name, c.name.span, &MemberSpans::of_class(&c.def));
            out.push(BuiltDecl::Class {
                name,
                conditional,
                guard,
                info: Arc::new(info),
            });
        }
        Declared::Function(f) => {
            if keep.is_some() && !kept(DeclKind::Function, &declaration.name()) {
                return;
            }
            let mut sig = FunctionReflection::from_decl(
                f,
                ctx,
                &FxHashMap::default(),
                &NameMap::default(),
                Visibility::Public,
            );
            sig.provenance = Provenance::new(origin.clone(), f.name.span);
            out.push(BuiltDecl::Function {
                name: Name::from(&declaration.name()),
                conditional,
                guard,
                sig: Arc::new(sig),
                site: f.name.span.start,
            });
        }
        Declared::Constant(c) => {
            let name = declaration.name();
            if kept(DeclKind::Constant, &name) {
                out.push(BuiltDecl::GlobalConst {
                    name: Name::from(&name),
                    guard,
                    ty: infer_const_type(&c.value),
                });
            }
        }
        Declared::Define { name, value } => {
            if let Some((name, ty)) = define_constant(name, value)
                && kept(DeclKind::Constant, name.as_str())
            {
                out.push(BuiltDecl::GlobalConst { name, guard, ty });
            }
        }
    }
}

/// The constant a `define('NAME', value)` declares, with its value's
/// inferred type (`infer_const_type`, the same literal-only scope a
/// `const` declaration's value already gets). `None` for a name that
/// isn't text, or one that's really the
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
fn define_constant(const_name: &PhpString, value: &Expr) -> Option<(Name, Type)> {
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
        return Some((Name::from(const_name), Type::int()));
    }
    Some((Name::from(const_name), infer_const_type(value)))
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
pub(crate) fn register_global_constant(map: &mut impl ConstantTable, name: Name, ty: Type) {
    match map.constant(&name) {
        None => map.set_constant(name, ty),
        Some(existing) if *existing == ty && !ty.is_mixed() => {}
        Some(_) => map.set_constant(name, Type::mixed()),
    }
}

/// A table of global constants' types, which `register_global_constant`
/// adds to.
pub(crate) trait ConstantTable {
    fn constant(&self, name: &str) -> Option<&Type>;
    fn set_constant(&mut self, name: Name, ty: Type);
}

impl ConstantTable for NameMap<Type> {
    fn constant(&self, name: &str) -> Option<&Type> {
        self.get(name)
    }

    fn set_constant(&mut self, name: Name, ty: Type) {
        self.insert(name, ty);
    }
}

impl ConstantTable for super::NameMap<Type> {
    fn constant(&self, name: &str) -> Option<&Type> {
        self.get(name)
    }

    fn set_constant(&mut self, name: Name, ty: Type) {
        self.insert(name, ty);
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
