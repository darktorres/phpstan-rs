use rustc_hash::FxHashMap;
use std::collections::HashSet;

use crate::ast::{ClassKind, Stmt, StmtKind};
use crate::types::{self, Type};

mod class_info;
mod function_sig;
mod name_context;

pub use class_info::{ClassInfo, PropertySig};
pub use function_sig::{FunctionSig, ParamSig};
pub(crate) use name_context::{NameContext, resolve_names_in_type};

/// A symbol table built by indexing already-parsed source, standing in for
/// the reflection a real PHP runtime would normally provide (see the
/// discussion this project started from: no live interpreter means no
/// `ReflectionClass`, so class/function signatures come from parsing
/// everything up front instead). `merge_stubs` layers in curated built-in
/// signatures (`stubs.rs`) the same way php-src stubs would sit alongside
/// project code in a real implementation.
#[derive(Debug, Default)]
pub struct Registry {
    pub classes: FxHashMap<String, ClassInfo>,
    pub functions: FxHashMap<String, FunctionSig>,
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
    // Phase 25: class names that have had at least one *unconditional*
    // declaration indexed so far - lets `index_with_context` tell "this
    // name's existing entry is itself unconditional, so a second
    // unconditional declaration should merge onto it" (Piwigo's own
    // `phpstan-stubs/Imagick.stub.php` shape) apart from "this name's
    // existing entry is only a conditional stub, so the first real
    // unconditional declaration should still replace it outright" (the
    // pre-existing `PHPUnit\Framework\TestCase` shape) - both look
    // identical from `self.classes.contains_key(&name)` alone.
    unconditionally_declared_classes: HashSet<String>,
    // Phase 32 Finding C: the function-side sibling of the tracking set
    // above - phpstorm-stubs represents a real PHP internal function
    // overload (different signatures by argument shape, not version) as
    // two separate top-level `function` declarations sharing one name
    // (`strtr`'s own real stub: a 3-string-argument form and a
    // 2-argument, `array $replace_pairs` form, back to back in the same
    // file) - without this, the second declaration's blind
    // `self.functions.insert` silently discards the first's own params
    // entirely, instead of merging into a callable that accepts either
    // shape.
    unconditionally_declared_functions: HashSet<String>,
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
    /// Finding 13: a trait that itself declares `use OtherTrait;` needs
    /// `OtherTrait`'s own methods folded into it before a class using the
    /// first trait is processed - a single non-iterative pass only ever
    /// composes one level deep. Runs `flatten_traits_one_pass` to a fixed
    /// point (each pass may itself surface a *new* one-level composition
    /// a later pass can now see, e.g. a trait-of-a-trait-of-a-trait
    /// chain), bounded by the number of classes/traits registered at all,
    /// since a real trait-composition cycle is itself a PHP fatal error
    /// this project doesn't otherwise detect, so degrading to "leave as
    /// partially flattened" once the bound is hit (rather than looping
    /// forever) is the honest answer, not a regression.
    #[hotpath::measure]
    pub(crate) fn flatten_traits(&mut self) {
        let max_iterations = self.classes.len().max(1);
        for _ in 0..max_iterations {
            if !self.flatten_traits_one_pass() {
                break;
            }
        }
    }

    /// One level of trait composition - see `flatten_traits`'s own doc
    /// comment for why this needs to run more than once. Returns whether
    /// anything was actually added anywhere, so the caller knows when a
    /// fixed point has been reached.
    fn flatten_traits_one_pass(&mut self) -> bool {
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
            let trait_aliases = self.classes[class_name].trait_aliases.clone();
            if uses.is_empty() && trait_aliases.is_empty() {
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
            // Finding 30: `use A, B { B::hello as helloB; }` - register
            // the trait method under its new alias too, *alongside* its
            // original name (both stay callable in real PHP). A bare
            // `method as newName;` with no `TraitName::` prefix (real,
            // valid PHP when the name is unambiguous among the traits
            // actually used) resolves against the first used trait that
            // declares it - this project doesn't detect the ambiguous
            // case (two used traits both declaring the same method name
            // with no `insteadof`) as its own separate error either, so
            // this leniency doesn't introduce a new gap next to it.
            for alias in &trait_aliases {
                let source_sig = match &alias.trait_name {
                    Some(t) => self
                        .classes
                        .get(t)
                        .and_then(|i| i.methods.get(&alias.method)),
                    None => uses
                        .iter()
                        .find_map(|t| self.classes.get(t)?.methods.get(&alias.method)),
                };
                if let Some(sig) = source_sig {
                    methods.push((alias.alias.clone(), sig.clone()));
                }
            }
            additions.push((
                class_name.clone(),
                methods,
                properties,
                abstract_from_traits,
            ));
        }

        let mut changed = false;
        for (class_name, methods, properties, abstract_from_traits) in additions {
            if let Some(info) = self.classes.get_mut(&class_name) {
                for (name, sig) in methods {
                    if let std::collections::hash_map::Entry::Vacant(slot) =
                        info.methods.entry(name)
                    {
                        slot.insert(sig);
                        changed = true;
                    }
                }
                for (name, sig) in properties {
                    if let std::collections::hash_map::Entry::Vacant(slot) =
                        info.properties.entry(name)
                    {
                        slot.insert(sig);
                        changed = true;
                    }
                }
                let before = info.abstract_methods.len();
                info.abstract_methods.extend(abstract_from_traits);
                if info.abstract_methods.len() != before {
                    changed = true;
                }
            }
        }
        changed
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
    #[hotpath::measure]
    pub(crate) fn index(&mut self, stmts: &[Stmt]) {
        let mut ctx = NameContext::default();
        self.index_with_context(stmts, &mut ctx, false);
    }

    /// `conditional` is `true` once inside any `if`/`else` branch (see the
    /// `StmtKind::If` arm below) - a declaration found there only fills a
    /// name in if nothing unconditional has already claimed it
    /// (`or_insert_with`), while a top-level, unconditional declaration
    /// always wins outright (`insert`), regardless of which is indexed
    /// first. Without this distinction, two files declaring the same
    /// fully-qualified name - one for real, one as a defensive `if (!
    /// class_exists('X')) { class X {...} }`/`if (!function_exists('foo'))
    /// { function foo() {} }` fallback guarding against a dependency that
    /// might not be loaded - would resolve to whichever happened to be
    /// indexed *last* (a plain `HashMap::insert` is order-dependent), not
    /// whichever is actually real. Confirmed real, not hypothetical: `main.rs::run_check_project`'s own
    /// `analyze_files.sort()` already anticipates "a real corpus can
    /// genuinely declare the same fully-qualified class name twice" and
    /// sorts for *determinism* - but a deterministic wrong answer is still
    /// wrong, and sorting alone doesn't decide which of the two should
    /// win. `PHPUnit\Framework\TestCase` is a real, observed instance
    /// (`vendor/rector/rector/stubs-rector/PHPUnit/Framework/TestCase.php`
    /// ships a guarded fallback with no `extends Assert`, alphabetically
    /// after the real `vendor/phpunit/phpunit/.../TestCase.php`, so it
    /// silently won the registry slot and dropped `assertEquals` from
    /// every real subclass).
    fn index_with_context(&mut self, stmts: &[Stmt], ctx: &mut NameContext, conditional: bool) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Namespace(name) => ctx.set_namespace(name),
                StmtKind::Use(imports) => ctx.add_uses(imports),
                StmtKind::ClassDecl(c) => {
                    let name = ctx.declared_name(&c.name);
                    let new_info = ClassInfo::from_decl(c, ctx);
                    if conditional {
                        self.classes.entry(name).or_insert_with(|| new_info);
                    } else if self.unconditionally_declared_classes.contains(&name) {
                        // Phase 25: a *second* unconditional declaration
                        // of the same name - real, not always a mistake -
                        // PHPStan's own `stubFiles` mechanism (Piwigo's own
                        // `phpstan-stubs/Imagick.stub.php`, wired into its
                        // `phpstan.neon`) exists specifically to let a
                        // project redeclare *one* method of a real,
                        // already-known class as a targeted override,
                        // relying on PHPStan to merge it onto everything
                        // else the real declaration already provides. A
                        // plain overwrite here discarded the *entire* real
                        // vendored `imagick.php` class - every other
                        // method/constant - the moment this override's own
                        // bare `class Imagick { public function
                        // convolveImage(...) {} }` was indexed, regardless
                        // of which file happened to load first.
                        let existing = self
                            .classes
                            .remove(&name)
                            .expect("name is in unconditionally_declared_classes, so it must already be indexed");
                        self.classes.insert(name, existing.merge(new_info));
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
                StmtKind::EnumDecl(e) => {
                    let name = ctx.declared_name(&e.name);
                    if conditional {
                        self.classes
                            .entry(name)
                            .or_insert_with(|| ClassInfo::from_enum_decl(e, ctx));
                    } else {
                        self.classes.insert(name, ClassInfo::from_enum_decl(e, ctx));
                    }
                }
                StmtKind::FunctionDecl(f) => {
                    let name = ctx.declared_name(&f.name);
                    let new_sig = FunctionSig::from_decl(f, ctx, &[]);
                    if conditional {
                        self.functions.entry(name).or_insert_with(|| new_sig);
                    } else if self.unconditionally_declared_functions.contains(&name) {
                        // Phase 32 Finding C: a *second* unconditional
                        // declaration of the same name - the real-overload
                        // shape described on this registry's own
                        // `unconditionally_declared_functions` field -
                        // merges onto the first rather than replacing it.
                        let existing = self.functions.remove(&name).expect(
                            "name is in unconditionally_declared_functions, so it must already be indexed",
                        );
                        self.functions.insert(name, existing.merge(new_sig));
                    } else {
                        self.unconditionally_declared_functions.insert(name.clone());
                        self.functions.insert(name, new_sig);
                    }
                }
                // The real, common `if (!function_exists('foo')) { function
                // foo() {} }` polyfill idiom (used throughout the PHP
                // ecosystem, e.g. symfony/deprecation-contracts and every
                // symfony/polyfill-* package) declares a genuinely global
                // function/class from inside a conditional - recursing
                // here is what makes those visible at all. Cloning `ctx`
                // for each branch means a `use`/`namespace` change inside
                // one doesn't leak into the other or past the `if` -
                // conservative, and irrelevant in practice (real code
                // doesn't declare namespaces inside an `if`). Both
                // branches are marked `conditional = true` regardless of
                // the `if`'s own condition text - this registry has no
                // notion of *which* conditions are actually guards, only
                // "was this declaration reached unconditionally or not,"
                // which is exactly the distinction the doc comment above
                // needs.
                StmtKind::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.index_with_context(
                        std::slice::from_ref(then_branch),
                        &mut ctx.clone(),
                        true,
                    );
                    if let Some(else_branch) = else_branch {
                        self.index_with_context(
                            std::slice::from_ref(else_branch),
                            &mut ctx.clone(),
                            true,
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
                // ensures (see `set_namespace`). Inherits whatever
                // `conditional` its enclosing context already has.
                StmtKind::Block(inner) => self.index_with_context(inner, ctx, conditional),
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
            // Phase 32 Finding C: stay in lockstep with `functions`, for
            // the identical reason the class side does below - otherwise
            // a later re-index of the same name would find a stale
            // "already unconditionally declared" marker with nothing left
            // in `functions` for it to point at.
            self.unconditionally_declared_functions.remove(name);
        }
        for name in classes {
            self.classes.remove(name);
            // Phase 25: must stay in lockstep with `classes` - otherwise a
            // later re-index of the same name (incremental re-checking's
            // whole reason for calling this) would find a stale "already
            // unconditionally declared" marker with nothing left in
            // `classes` for it to point at, wrongly taking the merge path
            // (`index_with_context`'s `ClassDecl` arm) against a
            // now-nonexistent entry instead of a clean first insertion.
            self.unconditionally_declared_classes.remove(name);
        }
    }

    /// User-defined symbols always win: a stub only fills in a name that
    /// isn't already declared in the indexed source (verified directly by
    /// `merge_stubs_does_not_override_user_functions` - unlike
    /// `merge_stub_classes` below, this one is *not* a merge, on purpose:
    /// a user-declared function sharing a builtin's name is a distinct
    /// definition the user's own code should win outright, not a second
    /// "overload" of the same conceptual function the way phpstorm-stubs'
    /// own `strtr`-style split declarations are).
    ///
    /// Phase 32 Finding C: every merged-in name is also recorded in
    /// `unconditionally_declared_functions` regardless - not for this call
    /// itself (which never merges), but so a *later* real project file
    /// legitimately declaring a function with the same name as a builtin
    /// correctly takes `index_with_context`'s own merge path (the `strtr`
    /// case) instead of blindly overwriting.
    pub fn merge_stubs<S: Into<String>>(&mut self, stubs: Vec<(S, FunctionSig)>) {
        for (name, sig) in stubs {
            let name: String = name.into();
            self.unconditionally_declared_functions.insert(name.clone());
            self.functions.entry(name).or_insert(sig);
        }
    }

    /// Same "user-defined wins" rule as `merge_stubs`, for classes -
    /// without this, built-in classes (`Exception`, `ArrayObject`, ...)
    /// would never be known to the registry at all, since stub sources
    /// were previously only mined for function signatures.
    ///
    /// Phase 25: every merged-in name is also recorded in
    /// `unconditionally_declared_classes` - this runs once, in
    /// `Project::new`, *before* any real project file is indexed, so a
    /// built-in class like `Imagick` is already present by the time
    /// `index_with_context` later processes a project's own
    /// `#[stubFiles]`-style override (Piwigo's own real shape:
    /// `phpstan-stubs/Imagick.stub.php`, a bare `class Imagick { public
    /// function convolveImage(...) {} }` meaning to patch one method).
    /// Without this, `index_with_context`'s own merge logic (added
    /// alongside this) would only ever see "no unconditional declaration
    /// recorded for this name yet" and wholesale-replace the entire
    /// built-in class with the override's bare one-method version -
    /// exactly the bug this fix exists to close. The merge itself keeps
    /// this function's own "user-defined always wins" contract robust
    /// even in the (today, never-hit in practice, since this always runs
    /// first) case where user code was somehow indexed before this runs -
    /// existing user info is `other` in `ClassInfo::merge`, so it still
    /// wins any member-name conflict.
    pub fn merge_stub_classes<S: Into<String>>(&mut self, stubs: Vec<(S, ClassInfo)>) {
        for (name, info) in stubs {
            let name: String = name.into();
            self.unconditionally_declared_classes.insert(name.clone());
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

    /// Every real class/enum that ends up composing `trait_fqn`, walking
    /// past any intermediate *trait* user rather than stopping at direct
    /// `uses` alone (Phase 13 Finding E) - `flatten_traits` merges a used
    /// trait's own methods into the *user's* method map, never the other
    /// way around, so a trait used only by another trait (`trait B { ... }
    /// trait A { use B; } class X { use A; }`) still needs `B` checked
    /// against `X`, the real, final host - checking it against `A` alone
    /// would see exactly the same host-unaware member set `A` itself has.
    pub(crate) fn classes_using_trait(&self, trait_fqn: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut stack = vec![trait_fqn.to_string()];
        let mut seen = HashSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            for (name, info) in &self.classes {
                if !info.uses.iter().any(|u| u == &current) {
                    continue;
                }
                if info.kind == ClassKind::Trait {
                    stack.push(name.clone());
                } else {
                    result.push(name.clone());
                }
            }
        }
        result
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

    /// Finding 46: real PHP forbids overriding a method declared `final`
    /// anywhere in the ancestor chain, no matter how many intervening
    /// subclasses sit between the `final` declaration and this one (a
    /// method that's final stays final for every descendant, not just
    /// its immediate subclass). Walks only `extends` - real PHP's own
    /// `final` keyword applies to ordinary class inheritance; an
    /// interface's own methods have no bodies to protect, and this
    /// project doesn't yet propagate a trait's own `final_methods` the
    /// way it does `abstract_methods` (Finding 46 as reported is scoped
    /// to class inheritance, matching its own repro). Returns the name
    /// of the ancestor that declared it final, for the diagnostic.
    pub fn find_final_ancestor_method(&self, class: &str, method: &str) -> Option<String> {
        let info = self.classes.get(class)?;
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = info.extends.clone();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(current_info) = self.classes.get(&current) else {
                continue;
            };
            if current_info.final_methods.contains(method) {
                return Some(current);
            }
            stack.extend(current_info.extends.iter().cloned());
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
            (Type::Array(_) | Type::ArrayShape(_), Type::Callable) => true,
            (Type::Object(a, _), Type::Object(b, _)) => a == b || self.is_subclass_of(a, b),
            // Same hierarchy-aware override as the plain Object-vs-Object
            // arm just above, one level up: `types::is_subtype`'s own
            // `ClassString` arm inherits its non-hierarchy-aware Object
            // comparison (name equality only), so without this a
            // `class-string<Derived>` value would incorrectly fail to
            // satisfy a `class-string<Base>`-typed parameter even when
            // `Derived extends Base`.
            (Type::ClassString(a), Type::ClassString(b)) => self.is_assignable(a, b),
            (Type::Union(members), _) => members.iter().all(|m| self.is_assignable(m, sup)),
            (_, Type::Union(members)) => members.iter().any(|m| self.is_assignable(sub, m)),
            // Phase 14: mirrors `types::is_subtype`'s own dedicated
            // Intersection-vs-Intersection rule, and for the same reason
            // - without it, this function's own generic `(Intersection,
            // _) => any(...)` rule below intercepts a same-shaped pair
            // (e.g. `list<string>` checked against a `list<string>`-
            // typed parameter) first and gets it wrong, since it only
            // asks whether one *bare* member of `sub` alone satisfies
            // the whole `sup` intersection, never whether `sub`'s own
            // combination does - a severe, real regression confirmed via
            // a full corpus re-check (211 -> 1861 diagnostics) before
            // this fix landed.
            (Type::Intersection(_), Type::Intersection(sup_members)) => {
                sup_members.iter().all(|m| self.is_assignable(sub, m))
            }
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
            // Finding 48's own completion: now that an array literal
            // carries a real per-position/per-key shape instead of
            // always reading as `Array(Mixed)`, it needs a real rule
            // saying such a shape satisfies an ordinary `array`/
            // `array<T>` hint - without this, `[1, 2, 3]` would stop
            // being assignable to `int[]`/`array<int>` the moment its
            // own type became precise, a real regression `types::
            // is_subtype`'s own (deliberately conservative, pre-Finding-48)
            // rule doesn't allow for.
            (Type::ArrayShape(fields), Type::Array(b)) => {
                fields.iter().all(|(_, ty, _)| self.is_assignable(ty, b))
            }
            // Same completion for shape-to-shape: a literal's own shape
            // satisfies a declared shape when every field the declared
            // shape actually requires is present (an optional field may
            // be missing) with an assignable type - extra fields the
            // literal has beyond what's required are ignored, the same
            // "structural, not exact" spirit `Array(Mixed)`'s own
            // leniency already uses elsewhere in this match.
            (Type::ArrayShape(sub_fields), Type::ArrayShape(sup_fields)) => {
                sup_fields.iter().all(|(name, sup_ty, optional)| {
                    match sub_fields.iter().find(|(n, _, _)| n == name) {
                        Some((_, sub_ty, _)) => self.is_assignable(sub_ty, sup_ty),
                        None => *optional,
                    }
                })
            }
            _ => types::is_subtype(sub, sup),
        }
    }
}

#[cfg(test)]
mod tests;
