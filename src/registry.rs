use rustc_hash::FxHashMap;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::{ClassKind, Expr, ExprKind, Literal, Stmt, StmtKind, Visibility};
use crate::types::{self, Type, Variance};

mod class_info;
mod function_sig;
mod name_context;
mod purity;

pub(crate) use class_info::hook_body_references_property;
pub use class_info::{ClassInfo, ConstSig, PropertySig};
pub use function_sig::format_signature;
pub(crate) use function_sig::resolve_type;
pub use function_sig::{AssertInfo, AssertTarget, FunctionSig, ParamSig};
pub(crate) use function_sig::{infer_const_type, mark_template_params, substitute_type_aliases};
pub(crate) use name_context::{NameContext, resolve_names_in_type};
pub use purity::PurityDependency;
pub(crate) use purity::is_non_overridable;

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
    // A global-constant name -> inferred-value-type table, covering both
    // real `const NAME = value;` top-level declarations and `define(
    // 'NAME', value)` calls (jetbrains/phpstorm-stubs' own
    // `define('T_WHITESPACE', 397);`-style constants, real corpus scale:
    // hundreds across that one vendored tree alone). Before this, a bare
    // reference to *any* global constant resolved to plain `Mixed`
    // unconditionally (no table existed at all - see `checker/expr.rs`'s
    // own `bare_name_literal_type` doc comment) - real PHPStan resolves
    // both the same way (`ReflectionConstant`'s own value-based type
    // inference doesn't distinguish how the constant was declared), so
    // this table doesn't either. Only a `const`'s own literal-shaped
    // value (`infer_const_type`'s existing scope: int/float/string/bool/
    // null/negative-int/string-concat literals) or a `define()`'s own
    // literal second argument is captured - anything more dynamic
    // (referencing another constant, a function call, ...) is left
    // unindexed, the identical "don't know, don't guess" leniency
    // `infer_const_type` itself already documents. Keyed the same way as
    // `functions`: a `const` gets its own namespace-qualified name
    // (`NameContext::declared_name`, real PHP's own resolution for `const`
    // declarations); a `define()`'s own name is *never* namespace-relative
    // (real PHP evaluates it as a plain runtime string regardless of the
    // surrounding `namespace` block), so it's inserted verbatim instead.
    // Looked up via `NameContext::resolve_function_candidates` at
    // check-time (`Checker::global_constant_type`) - constants share
    // exactly the same "check the current namespace first, fall back to
    // the global namespace for an unqualified reference" resolution rule
    // real PHP functions do, unlike class names.
    pub global_constants: FxHashMap<String, Type>,
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
    // Phase 62: opts into `Checker::check_uninitialized_properties`.
    // Defaults to `false`, matching real PHPStan's own default *exactly*
    // - `UninitializedPropertyRule` sits behind its own
    // `checkUninitializedProperties` parameter (`false` in `conf/
    // config.neon`, not part of any of the standard level 0-10 rule
    // sets) - real PHPStan itself doesn't trust this rule enough to run
    // it by default, and this project's own version is a substantially
    // simpler heuristic than PHPStan's real `ConstructorsHelper`-backed
    // dataflow (see `checker/uninitialized.rs`'s own doc comment for
    // exactly how much narrower). Same `strict_mixed_arrays` precedent:
    // a plain `pub` field via `#[derive(Default)]`, config -> CLI flag ->
    // `Project` setter -> here.
    pub check_uninitialized_properties: bool,
    // Phase 67: opts into `Checker::check_internal_*` (`internal_usage.rs`).
    // Defaults to `false`, matching real PHPStan's own default *exactly*
    // - the whole `RestrictedInternal*UsageExtension` family sits behind
    // `featureToggles.internalTag` (`false` in `conf/config.neon`, not
    // part of any of the standard level 0-10 rule sets), the same
    // non-default posture `check_uninitialized_properties` above already
    // established a precedent for. Same wiring chain: a plain `pub`
    // field via `#[derive(Default)]`, config -> CLI flag -> `Project`
    // setter -> here.
    pub check_internal_usage: bool,
    // Phase 39: opts into `Checker::check_deprecated_*`
    // (`deprecated_usage.rs`). Defaults to `false` - real PHPStan's own
    // equivalent (`phpstan/phpstan-deprecation-rules`) isn't even part of
    // core `phpstan/phpstan` at all, but a separate, opt-in Composer
    // package most real projects install alongside it; this project has
    // no such "separate package" concept, so an opt-in flag is the
    // closest equivalent, following `check_internal_usage`'s exact same
    // wiring chain and "real value, but real vendor-corpus noise"
    // rationale.
    pub check_deprecated_usage: bool,
    // Phase 38: the project's configured target PHP version, if any -
    // `None` (the default) means "not configured", which every
    // version-gated rule that reads this should treat as "nothing to
    // check yet" rather than assuming any particular version. Set via
    // `Project::set_php_version`, following `strict_mixed_arrays`'s exact
    // precedent (config.rs field -> CLI flag -> merge -> Project setter ->
    // this field). See `crate::php_version::PhpFeature` for the small
    // feature-availability lookup table this unlocks.
    pub php_version: Option<crate::php_version::PhpVersion>,
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
    // Names `merge_stub_classes` provided a built-in native-class stub
    // for (jetbrains/phpstorm-stubs) - a *subset* of `unconditionally_
    // declared_classes` above, distinguishing "this name's existing
    // entry is itself only a type-checking fiction, never really
    // instantiated" from "this name's existing entry is a real,
    // unconditionally-declared vendor/first-party class". Both look
    // identical from `unconditionally_declared_classes.contains(&name)`
    // alone, but `index_with_context`'s own `if conditional` branch
    // needs to tell them apart: merging a version-gated real polyfill
    // implementation's own members onto a built-in stub is safe and
    // desired (the stub is fictional either way, see that branch's own
    // doc comment - `symfony/polyfill-php84`'s own `ReflectionConstant`
    // colliding with jetbrains's native-class stub for it), but doing
    // the identical merge onto a *real*, already-known vendor class
    // (the pre-existing `PHPUnit\Framework\TestCase`-vs-`class_exists`-
    // guarded-fallback shape `unconditionally_declared_classes` itself
    // was added for) would wrongly leak a defensive fallback's own
    // never-taken members onto the real class everything else sees -
    // exactly the regression `unconditional_class_wins_over_a_later_
    // conditional_stub` catches.
    built_in_stub_classes: HashSet<String>,
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
    // Phase 60 corpus find: names `merge_stubs` provided a signature for,
    // not yet overwritten by any real, already-indexed conditional
    // declaration - lets `index_with_context`'s own `StmtKind::
    // FunctionDecl` conditional branch tell "this name's existing entry
    // is only the built-in stub's" apart from "this name's existing
    // entry is a real, already-indexed conditional declaration of the
    // same name from an earlier file" (both look identical from
    // `self.functions.contains_key(&name)` alone, the same ambiguity
    // `unconditionally_declared_functions` already exists to resolve on
    // the *unconditional* side). Without this, the extremely common
    // `if (!function_exists('str_contains')) { function str_contains
    // (?string $haystack, ?string $needle): bool {...} }` polyfill idiom
    // (real corpus shape: symfony/polyfill-php80) always lost to the
    // built-in stub's own native-only signature, silently discarding the
    // polyfill's real, more-precise (here, nullable) parameter types -
    // `entry(...).or_insert_with(...)` is a no-op once the stub already
    // occupies the slot, which `merge_stubs` guarantees happens first.
    stub_provided_functions: HashSet<String>,
}

impl Registry {
    pub fn build(program: &[Stmt]) -> Registry {
        let mut registry = Registry::index_and_flatten(program);
        registry.resolve_transitive_purity();
        registry
    }

    /// Every step `build` runs, *except* the final purity classification -
    /// see `resolve_transitive_purity`'s own call site in `build` for why
    /// that split exists. `php_stubs.rs`'s own `generate_from_source`/
    /// `generate_classes_from_source` call this directly instead of
    /// `build`: they only ever build a registry to extract raw
    /// `FunctionSig`/`ClassInfo` values out of vendored stub source, and a
    /// stub file's own function body is always empty - classifying it
    /// would mean every one of PHP's own built-in functions (`header()`,
    /// `exit()`, ...) looks provably pure, then gets that wrong verdict
    /// baked permanently into the `LazyLock`-cached stub signatures every
    /// real project's own registry later merges in (a real bug, found via
    /// full-corpus verification, not assumed) - `FunctionSig::is_stub`
    /// isn't even set yet at this point (`php_stubs.rs` sets it
    /// afterward, on the values this function returns), so classifying
    /// here couldn't have skipped them via that gate either way.
    pub(crate) fn index_and_flatten(program: &[Stmt]) -> Registry {
        let mut registry = Registry::default();
        registry.index(program);
        registry.flatten_traits();
        registry.flatten_phpdoc_inheritance();
        registry.resolve_deferred_value_of();
        registry.resolve_deferred_type_imports();
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

    /// Phase 87: real PHPStan backfills a method's own missing `@param`/
    /// `@return` from its parent class (first) or first-declared
    /// implemented interface, whenever the child's own docblock lacks a
    /// tag for that position *at all* - verified directly against
    /// `vendor/phpstan-src/src/PhpDoc/ResolvedPhpDocBlock.php`'s own
    /// `mergeParamTags`/`mergeReturnTags` (`array_key_exists($name,
    /// $paramTags)` / `$returnTag !== null`), unconditional and
    /// unrelated to whether the resolved type happens to be `Mixed`.
    /// Reuses `lookup_ancestor_method_with_owner`'s already-correct
    /// parent-then-implements, first-hit-wins order - no new ordering
    /// logic. Runs after `flatten_traits` (so a trait-derived method
    /// already looks like the class's own), to a fixed point exactly
    /// like `flatten_traits` itself, for the identical reason: a
    /// multi-level chain where *each* level textually redeclares the
    /// method without its own docblock needs more than one pass to fully
    /// resolve.
    ///
    /// Scope, deliberately narrow for this first slice (see ROADMAP.md's
    /// Phase 87 entry for the full rationale): methods only, no
    /// properties (real PHP interfaces can't declare them, so the
    /// existing `lookup_ancestor_property_with_owner`'s parent-only walk
    /// already covers the one real path); no trait-abstract-method
    /// fallback; ancestor conflicts aren't reconciled, first-match-wins
    /// (already this project's behavior, and real PHPStan's too); an
    /// ancestor's `private` method is never inherited from (a `private`
    /// method can't be overridden at all, so its own docblock says
    /// nothing about what an unrelated same-named child method means).
    #[hotpath::measure]
    pub(crate) fn flatten_phpdoc_inheritance(&mut self) {
        let max_iterations = self.classes.len().max(1);
        for _ in 0..max_iterations {
            if !self.flatten_phpdoc_inheritance_one_pass() {
                break;
            }
        }
    }

    /// Phase 80 follow-up: resolves `types::parse_type_keyword_or_class`'s
    /// own `value-of<T>`-on-a-class/enum sentinel
    /// (`Type::Object("$$valueof", [Type::Object(target_fqn, [])])`) now
    /// that every class/enum is actually indexed - see that function's own
    /// doc comment for why this can't happen at parse time. A single pass,
    /// not a fixed-point loop like `flatten_phpdoc_inheritance` above: a
    /// `value-of<value-of<X>>`-style chain isn't attempted (no realistic
    /// case, disclosed rather than silently unhandled).
    ///
    /// Every class is taken out of `self.classes` before being mutated
    /// (`HashMap::remove`, reinserted at the end of its own iteration),
    /// not the collect-then-apply shape `flatten_phpdoc_inheritance_one_pass`
    /// needs - that one's own shape exists to let a fixed-point loop read
    /// an *ancestor's* still-unmodified signature while backfilling a
    /// *descendant's*; this pass only ever needs to read some *other*
    /// class/enum's own `properties["value"]` while rewriting the current
    /// one's types, and temporarily removing the current entry sidesteps
    /// borrowing `self.classes` both ways at once with no extra plumbing.
    /// (A class whose own docblock names `value-of<self>` on itself, where
    /// `self` is genuinely an enum, degrades to `Mixed` while it's
    /// temporarily removed - the identical, already-accepted "can't
    /// verify, don't guess" outcome every other unresolvable case here
    /// already gets, not a new failure mode.) Top-level functions
    /// (`self.functions`) need no such dance at all - mutating one
    /// function's own signature while reading `self.classes` for a lookup
    /// borrows two disjoint fields of `self`, which the borrow checker
    /// already allows without help.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_value_of(&mut self) {
        let class_names: Vec<String> = self.classes.keys().cloned().collect();
        for class_name in class_names {
            let Some(mut info) = self.classes.remove(&class_name) else {
                continue;
            };
            for sig in info.methods.values_mut() {
                for param in &mut sig.params {
                    if type_contains_deferred_value_of(&param.ty) {
                        let ty = std::mem::replace(&mut param.ty, Type::Mixed);
                        param.ty = resolve_deferred_value_of_with_native_fallback(
                            ty,
                            &param.ty_native,
                            &self.classes,
                        );
                    }
                }
                if type_contains_deferred_value_of(&sig.return_type) {
                    let ty = std::mem::replace(&mut sig.return_type, Type::Mixed);
                    sig.return_type = resolve_deferred_value_of_with_native_fallback(
                        ty,
                        &sig.return_type_native,
                        &self.classes,
                    );
                }
            }
            for prop in info.properties.values_mut() {
                if type_contains_deferred_value_of(&prop.ty) {
                    let ty = std::mem::replace(&mut prop.ty, Type::Mixed);
                    prop.ty = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
                if let Some(set_ty) = &prop.set_ty
                    && type_contains_deferred_value_of(set_ty)
                {
                    let ty = prop.set_ty.take().unwrap();
                    prop.set_ty = Some(resolve_deferred_value_of_in_type(ty, &self.classes));
                }
            }
            for c in info.consts.values_mut() {
                if type_contains_deferred_value_of(&c.ty) {
                    let ty = std::mem::replace(&mut c.ty, Type::Mixed);
                    c.ty = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            for bound in info.template_bounds.values_mut() {
                if type_contains_deferred_value_of(bound) {
                    let ty = std::mem::replace(bound, Type::Mixed);
                    *bound = resolve_deferred_value_of_in_type(ty, &self.classes);
                }
            }
            self.classes.insert(class_name, info);
        }
        for sig in self.functions.values_mut() {
            for param in &mut sig.params {
                if type_contains_deferred_value_of(&param.ty) {
                    let ty = std::mem::replace(&mut param.ty, Type::Mixed);
                    param.ty = resolve_deferred_value_of_with_native_fallback(
                        ty,
                        &param.ty_native,
                        &self.classes,
                    );
                }
            }
            if type_contains_deferred_value_of(&sig.return_type) {
                let ty = std::mem::replace(&mut sig.return_type, Type::Mixed);
                sig.return_type = resolve_deferred_value_of_with_native_fallback(
                    ty,
                    &sig.return_type_native,
                    &self.classes,
                );
            }
        }
    }

    /// `@phpstan-import-type`'s own deferred half - `ClassInfo::from_decl`
    /// already resolved both names (`ResolvedTypeImport`'s own doc
    /// comment), but couldn't look the import up yet: the source class
    /// may not be indexed at all at that point. Mirrors `resolve_deferred_
    /// value_of`'s exact two-step shape (a read-only pass collecting
    /// every resolvable answer, then a second pass applying them) for the
    /// identical reason - looking a *different* class's own `type_aliases`
    /// up while mutating *this* class's own would otherwise borrow
    /// `self.classes` two ways at once.
    ///
    /// A source class's own alias is looked up under `{its own
    /// namespace}\{alias name}` - reconstructed directly from its FQN
    /// (`class_namespace`) rather than re-running that class's own name
    /// resolution, which would need its own `NameContext` (not kept
    /// around once `from_decl` returns). This matches `NameContext::
    /// resolve`'s own actual behavior for a bare, unqualified name with
    /// no matching `use`-import (confirmed directly: falls straight
    /// through to `declared_name`, i.e. plain namespace-prefixing) -
    /// exactly what a `@phpstan-type` tag's own bare alias name always is
    /// in practice, never itself subject to `use`-aliasing.
    ///
    /// Not a fixed-point loop (an import that itself points at another
    /// imported - not locally declared - alias isn't chased further) -
    /// the identical "one level of indirection only" leniency `build_
    /// type_aliases`'s own second pass already accepts for local aliases
    /// referencing each other, extended here rather than reinvented.
    #[hotpath::measure]
    pub(crate) fn resolve_deferred_type_imports(&mut self) {
        let mut classes_with_imports: Vec<String> = Vec::new();
        let mut resolved: Vec<(String, String, Type)> = Vec::new();
        for (class_name, info) in &self.classes {
            if info.type_imports.is_empty() {
                continue;
            }
            classes_with_imports.push(class_name.clone());
            for imp in &info.type_imports {
                let source_key = declared_name_in_namespace(
                    class_namespace(&imp.source_class_fqn),
                    &imp.source_alias_name,
                );
                if let Some(ty) = self
                    .classes
                    .get(&imp.source_class_fqn)
                    .and_then(|src| src.type_aliases.get(&source_key))
                {
                    resolved.push((class_name.clone(), imp.local_key.clone(), ty.clone()));
                }
            }
        }
        if classes_with_imports.is_empty() {
            return;
        }
        let mut by_class: HashMap<String, HashMap<String, Type>> = HashMap::new();
        for (class_name, local_key, ty) in resolved {
            by_class
                .entry(class_name)
                .or_default()
                .insert(local_key, ty);
        }
        for class_name in classes_with_imports {
            let Some(mut info) = self.classes.remove(&class_name) else {
                continue;
            };
            // A class whose own imports all failed to resolve (its source
            // class isn't indexed at all, e.g. an external dependency
            // outside this run's own corpus) still needs `type_imports`
            // cleared - the one-shot pass above already ran, so there's no
            // second chance coming; an empty `aliases` map here makes
            // every `substitute_type_aliases` call below a no-op, correct
            // but slightly wasted work, traded for not duplicating this
            // whole loop body into two near-identical copies.
            let aliases = by_class.remove(&class_name).unwrap_or_default();
            for sig in info.methods.values_mut() {
                for param in &mut sig.params {
                    let ty = std::mem::replace(&mut param.ty, Type::Mixed);
                    param.ty = substitute_type_aliases(ty, &aliases);
                }
                let ty = std::mem::replace(&mut sig.return_type, Type::Mixed);
                sig.return_type = substitute_type_aliases(ty, &aliases);
            }
            for prop in info.properties.values_mut() {
                let ty = std::mem::replace(&mut prop.ty, Type::Mixed);
                prop.ty = substitute_type_aliases(ty, &aliases);
                if let Some(set_ty) = prop.set_ty.take() {
                    prop.set_ty = Some(substitute_type_aliases(set_ty, &aliases));
                }
            }
            for c in info.consts.values_mut() {
                let ty = std::mem::replace(&mut c.ty, Type::Mixed);
                c.ty = substitute_type_aliases(ty, &aliases);
            }
            info.type_aliases.extend(aliases);
            info.type_imports.clear();
            self.classes.insert(class_name, info);
        }
    }

    /// The `&self` (read-only, immediate) twin of `resolve_deferred_value_of`
    /// above, for a `Type` parsed directly from a raw docblock string
    /// *during checking* rather than at registry-build time - by then
    /// every class/enum genuinely is already fully indexed (unlike mid-
    /// registry-construction, where `resolve_deferred_value_of` itself
    /// needs its own one-time deferred pass), so this resolves the
    /// `"$$valueof"` sentinel immediately, inline, with no pass needed.
    /// Every checker-side call site that parses a docblock type string
    /// independently of the registry's own `FunctionSig::from_decl`/
    /// `ClassInfo::from_decl` pipeline (which `resolve_deferred_value_of`
    /// already covers) needs this too - real corpus false positive found
    /// via this phase's own corpus diff caught exactly one such site
    /// (`Checker::resolve_type_hint`, reached through an inline `@var`
    /// annotation) that would otherwise leak the raw, unresolved sentinel
    /// straight into a real diagnostic message
    /// (`shipmonk/dead-code-detector`'s own `CollectedUsage::deserialize()`,
    /// `/** @var array{..., t: value-of<MemberType>, ...} $result */`).
    pub(crate) fn resolve_deferred_value_of_now(&self, ty: Type) -> Type {
        if !type_contains_deferred_value_of(&ty) {
            return ty;
        }
        resolve_deferred_value_of_in_type(ty, &self.classes)
    }

    /// `resolve_deferred_value_of_now`'s own native-fallback-aware sibling.
    /// See `resolve_deferred_value_of_with_native_fallback`'s doc comment
    /// (the free function this wraps) for why an unresolvable
    /// `value-of<T>` needs this rather than plain `resolve_deferred_value_of_now`
    /// alone. Used by the anonymous-class param/return docblock fallback
    /// in `checker.rs`, which (like the registry's own `FunctionSig::
    /// from_decl`) reconciles its own raw-parsed doc type against a
    /// native counterpart via `resolve_type` before this ever runs.
    pub(crate) fn resolve_deferred_value_of_now_with_native_fallback(
        &self,
        ty: Type,
        native: &Type,
    ) -> Type {
        if !type_contains_deferred_value_of(&ty) {
            return ty;
        }
        resolve_deferred_value_of_with_native_fallback(ty, native, &self.classes)
    }

    /// Whether `ty` mentions a `Type::TemplateParam` anywhere in its tree -
    /// mirrors `function_sig::mark_template_params`'s own recursive-case
    /// list (`Object`'s type args, `Array`'s key/value, `ClassString`,
    /// `Union`/`Intersection`, `ArrayShape`/`ObjectShape` fields).
    /// `flatten_phpdoc_inheritance_one_pass` uses this to refuse
    /// backfilling a template-parameterized ancestor type at all: a
    /// `Type::TemplateParam` is only meaningful within the *ancestor's
    /// own* `@template` scope, and (per `Type::Object`'s own doc comment)
    /// this project does not model cross-inheritance template binding
    /// (`@extends Base<Concrete>` fixing a parent's own type parameter) -
    /// cloning the raw value onto an unrelated descendant would silently
    /// keep it bound to the *ancestor's* own (often wider) bound instead.
    /// Real corpus false positive caught verifying this pass: Doctrine's
    /// `AbstractClassMetadataFactory implements ClassMetadataFactory`
    /// (`@template T of ClassMetadata`, `@phpstan-return list<T>` on
    /// `getAllMetadata()`) backfilling verbatim onto
    /// `Doctrine\ORM\Mapping\ClassMetadataFactory` (which real PHPStan
    /// binds `T` to the *concrete* `Doctrine\ORM\Mapping\ClassMetadata`
    /// via its own `@extends AbstractClassMetadataFactory<ClassMetadata>`)
    /// left `T` resolved against the *interface* bound instead, silently
    /// losing `Doctrine\ORM\Mapping\ClassMetadata::getTableName()`.
    pub(crate) fn type_mentions_template_param(ty: &Type) -> bool {
        match ty {
            Type::TemplateParam(_) => true,
            Type::Object(_, args) => args.iter().any(Registry::type_mentions_template_param),
            Type::Array(key, inner) => {
                Registry::type_mentions_template_param(key)
                    || Registry::type_mentions_template_param(inner)
            }
            Type::ClassString(inner) => Registry::type_mentions_template_param(inner),
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().any(Registry::type_mentions_template_param)
            }
            Type::ArrayShape(fields) | Type::ObjectShape(fields) => fields
                .iter()
                .any(|(_, t, _)| Registry::type_mentions_template_param(t)),
            _ => false,
        }
    }

    /// Whether backfilling `ancestor_ty` onto a position whose own native
    /// hint alone already resolves to `native_ty` is safe: `false` only
    /// when both sides are a concrete `Type::Object` naming *different*
    /// classes - PHP's own native hint is authoritative about what a
    /// position's real static type is (enforced at runtime), so a
    /// same-family-but-different-class ancestor docblock type can only
    /// ever be an ancestor-scoped generic that doesn't correctly apply to
    /// *this* override, never a legitimate narrowing (a real narrowing
    /// covariant override always keeps the *same* class name, or one the
    /// child's own native hint should already name more specifically
    /// itself). Real corpus false positive caught verifying this pass:
    /// `Doctrine\ORM\EntityManagerInterface::getMetadataFactory()` (no
    /// own doc, native hint already resolves - via its own `use Doctrine\
    /// ORM\Mapping\ClassMetadataFactory` - to the concrete ORM factory)
    /// sits under `Doctrine\Persistence\ObjectManager`'s own `@phpstan-
    /// return ClassMetadataFactory<ClassMetadata<object>>` (that file's
    /// own `use` resolves the *unqualified* interface instead) - a real,
    /// concrete, non-template type on both sides (so neither of the two
    /// guards above catches it), but backfilling it would still silently
    /// widen away the already-correct, more specific native class.
    pub(crate) fn safe_to_backfill_object_type(ancestor_ty: &Type, native_ty: &Type) -> bool {
        match (ancestor_ty, native_ty) {
            (Type::Object(a, _), Type::Object(n, _)) => a == n,
            _ => true,
        }
    }

    /// Whether backfilling an ancestor's own `Mixed` tag is safe: `false`
    /// only when the child's own *native* resolution (`ty_native`/
    /// `return_type_native`, computed with no docblock merge at all) is
    /// itself something more specific than `Mixed` - inheriting a `Mixed`
    /// tag can only ever destroy that kind of real information, never add
    /// any (`Doctrine\DBAL\Types\Type::convertToPHPValue(): mixed`
    /// backfilling onto `AbstractStringVoType::convertToPHPValue():
    /// ?StringVo`, a real corpus false positive caught verifying this
    /// pass). When the child's own native resolution is *already*
    /// `Mixed` too (a genuinely untyped native parameter, PSR-3's own
    /// `LoggerInterface::log(mixed $level, ...)` shape), there is nothing
    /// to lose - inheriting the ancestor's own `Mixed` here is exactly
    /// what correctly propagates `explicit_mixed_hint` (Phase 88) down
    /// from wherever it was genuinely, deliberately written, rather than
    /// leaving the position looking entirely undocumented.
    pub(crate) fn safe_to_backfill_mixed(ancestor_ty: &Type, native_ty: &Type) -> bool {
        *ancestor_ty != Type::Mixed || *native_ty == Type::Mixed
    }

    /// One pass of `flatten_phpdoc_inheritance` - see its own doc comment.
    /// Returns whether anything was actually changed, so the caller knows
    /// when a fixed point has been reached.
    ///
    /// Gates each backfill on the *ancestor's* own `has_own_doc`/
    /// `return_has_own_doc` bit, not merely on the ancestor having *a*
    /// type at all - real PHPStan's own merge only ever pulls from an
    /// ancestor's own parsed `@param`/`@return` *tag*; an ancestor with
    /// no tag of its own has nothing to contribute, even though its
    /// `ty`/`return_type` field is never literally empty (it still holds
    /// whatever the native hint alone resolved to). Confirmed necessary
    /// by a real corpus regression caught while implementing this pass:
    /// `Base14::make(): int` / `Sub14::make(): string`, a genuine
    /// Liskov-violating override with no docblock anywhere - naively
    /// backfilling `int` onto `Sub14` from `Base14`'s bare native
    /// resolution would have both hidden the real override diagnostic
    /// behind a fabricated docblock type and produced a bogus
    /// `return.type` mismatch against the still-native-`string` return
    /// body.
    ///
    /// A successful backfill *does* flip `has_own_doc`/
    /// `return_has_own_doc` to `true` - once resolved, a position counts
    /// as "documented" (own or inherited) for any *further* descendant's
    /// own lookup, which is exactly what makes multi-level transitivity
    /// work one level per pass (an interface only two levels up from a
    /// concrete class, itself redeclaring the method with no doc of its
    /// own, only becomes a usable source on the pass *after* it was
    /// itself backfilled) - and, as a side effect, is what makes a
    /// resolved position permanently ineligible for re-visiting on a
    /// later pass, the same never-revisit-a-filled-slot shape
    /// `flatten_traits_one_pass`'s own `Entry::Vacant` check already has.
    fn flatten_phpdoc_inheritance_one_pass(&mut self) -> bool {
        type Backfill = (
            String,
            String,
            Vec<(usize, Type, bool)>,
            Option<(Type, bool)>,
        );

        let class_names: Vec<String> = self.classes.keys().cloned().collect();
        let mut backfills: Vec<Backfill> = Vec::new();

        for class_name in &class_names {
            let Some(info) = self.classes.get(class_name) else {
                continue;
            };
            for (method_name, sig) in &info.methods {
                let needs_param = sig.params.iter().any(|p| !p.has_own_doc);
                let needs_return = !sig.return_has_own_doc;
                if !needs_param && !needs_return {
                    continue;
                }
                let Some((owner, ancestor_sig)) =
                    self.lookup_ancestor_method_with_owner(class_name, method_name)
                else {
                    continue;
                };
                if ancestor_sig.visibility == Visibility::Private {
                    continue;
                }
                // Real PHPStan's own identical carve-out
                // (`PhpDocInheritanceResolver::resolvePhpDocForMethod`:
                // `$parentMethod->getName() !== '__construct' ||
                // !$parentMethod->getDeclaringClass()->isBuiltin()`) - a
                // subclass's own `__construct` legitimately takes
                // completely unrelated parameters from a native/engine
                // ancestor's own constructor (real PHP doesn't require
                // constructor signature compatibility at all), so
                // `RuntimeException`/`Exception`'s own `@param string
                // $message` must never backfill onto e.g. `final class
                // ResponseReadyException extends RuntimeException {
                // public function __construct(private readonly
                // ResponseInterface $response) {...} }` - a genuine
                // corpus false positive caught verifying this pass.
                // `built_in_stub_classes` (populated by
                // `merge_stub_classes`, phpstorm-stubs' own native/
                // engine-class declarations) is this project's
                // equivalent of `ClassReflection::isBuiltin()` (real
                // PHP's `ReflectionClass::isInternal()`) - true for
                // `Exception`/`ArrayObject`/... but not for a real
                // vendored userland base class (Symfony's `Command`,
                // Doctrine's console commands, ...), which must still
                // participate in this pass normally.
                if method_name == "__construct" && self.built_in_stub_classes.contains(&owner) {
                    continue;
                }
                let mut param_fills = Vec::new();
                if needs_param {
                    // Real PHP never enforces constructor-signature
                    // compatibility (arity, order, or names can differ
                    // freely between a class and its parent), so
                    // `remapParameterNames`'s usual by-*position*
                    // correspondence - correct for a real, LSP-bound
                    // override, where PHP itself guarantees positions
                    // line up - cannot be trusted for `__construct`.
                    // Real PHPStan special-cases exactly this
                    // (`resolveMethodPhpDocFromParentClass`'s own
                    // `$lowercaseMethodName !== '__construct'` guard
                    // around building the real positional-name list, vs.
                    // falling back to the child's own names, which makes
                    // its later remap a same-name-only match): match by
                    // parameter *name* instead. Real corpus false
                    // positive caught verifying this pass: `Doctrine
                    // Migrations\Tools\Console\Command\DoctrineCommand`'s
                    // own `__construct(?DependencyFactory
                    // $dependencyFactory, ?string $name)` has no own
                    // doc; positional matching against `Symfony\
                    // Component\Console\Command\Command::__construct(
                    // ?string $name)` (own `@param string|null $name`)
                    // would wrongly stamp `string|null` onto position 0
                    // (`$dependencyFactory`) - name matching correctly
                    // leaves it alone and still lets the genuinely
                    // same-named `$name` inherit.
                    if method_name == "__construct" {
                        for (i, p) in sig.params.iter().enumerate() {
                            if p.has_own_doc {
                                continue;
                            }
                            if let Some(ancestor_param) =
                                ancestor_sig.params.iter().find(|ap| ap.name == p.name)
                                && ancestor_param.has_own_doc
                                && Registry::safe_to_backfill_mixed(
                                    &ancestor_param.ty,
                                    &p.ty_native,
                                )
                                && !Registry::type_mentions_template_param(&ancestor_param.ty)
                                && Registry::safe_to_backfill_object_type(
                                    &ancestor_param.ty,
                                    &p.ty_native,
                                )
                            {
                                param_fills.push((
                                    i,
                                    function_sig::reconcile_with_native(
                                        ancestor_param.ty.clone(),
                                        p.ty_native.clone(),
                                    ),
                                    ancestor_param.explicit_mixed_hint,
                                ));
                            }
                        }
                    } else {
                        for (i, p) in sig.params.iter().enumerate() {
                            if p.has_own_doc {
                                continue;
                            }
                            if let Some(ancestor_param) = ancestor_sig.params.get(i)
                                && ancestor_param.has_own_doc
                                && Registry::safe_to_backfill_mixed(
                                    &ancestor_param.ty,
                                    &p.ty_native,
                                )
                                && !Registry::type_mentions_template_param(&ancestor_param.ty)
                                && Registry::safe_to_backfill_object_type(
                                    &ancestor_param.ty,
                                    &p.ty_native,
                                )
                            {
                                param_fills.push((
                                    i,
                                    function_sig::reconcile_with_native(
                                        ancestor_param.ty.clone(),
                                        p.ty_native.clone(),
                                    ),
                                    ancestor_param.explicit_mixed_hint,
                                ));
                            }
                        }
                    }
                }
                // `safe_to_backfill_mixed` (both here and in the param
                // loops above) isn't about `__construct` at all - see its
                // own doc comment for the two real corpus shapes it
                // reconciles (Doctrine DBAL's `Type::convertToPHPValue():
                // mixed` must never overwrite a concrete native return
                // type, but PSR-3's `LoggerInterface::log(mixed $level,
                // ...)` must still propagate onto an equally-untyped
                // override, or `explicit_mixed_hint` never reaches it).
                let return_fill = (needs_return
                    && ancestor_sig.return_has_own_doc
                    && Registry::safe_to_backfill_mixed(
                        &ancestor_sig.return_type,
                        &sig.return_type_native,
                    )
                    && !Registry::type_mentions_template_param(&ancestor_sig.return_type)
                    && Registry::safe_to_backfill_object_type(
                        &ancestor_sig.return_type,
                        &sig.return_type_native,
                    ))
                .then(|| {
                    (
                        function_sig::reconcile_with_native(
                            ancestor_sig.return_type.clone(),
                            sig.return_type_native.clone(),
                        ),
                        ancestor_sig.return_explicit_mixed_hint,
                    )
                });
                if !param_fills.is_empty() || return_fill.is_some() {
                    backfills.push((
                        class_name.clone(),
                        method_name.clone(),
                        param_fills,
                        return_fill,
                    ));
                }
            }
        }

        let mut changed = false;
        for (class_name, method_name, param_fills, return_fill) in backfills {
            if let Some(sig) = self
                .classes
                .get_mut(&class_name)
                .and_then(|info| info.methods.get_mut(&method_name))
            {
                for (i, ty, explicit_mixed_hint) in param_fills {
                    if let Some(p) = sig.params.get_mut(i) {
                        p.ty = ty;
                        p.has_own_doc = true;
                        p.explicit_mixed_hint = explicit_mixed_hint;
                        changed = true;
                    }
                }
                if let Some((ty, explicit_mixed_hint)) = return_fill {
                    sig.return_type = ty;
                    sig.return_has_own_doc = true;
                    sig.return_explicit_mixed_hint = explicit_mixed_hint;
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
                    if conditional && self.built_in_stub_classes.contains(&name) {
                        // A plain `or_insert_with` no-op (the shape every
                        // other conditional case below still uses) fully
                        // discards a conditional declaration's *own*
                        // members the moment its name already exists -
                        // correct when the existing entry is a *real*,
                        // already-known vendor/first-party class (a
                        // `class_exists`-guarded defensive fallback's own
                        // members genuinely must never leak onto it, see
                        // `unconditional_class_wins_over_a_later_
                        // conditional_stub`), but too conservative when
                        // the existing entry is only `built_in_stub_
                        // classes`' own type-checking fiction (jetbrains/
                        // phpstorm-stubs, never really instantiated) and
                        // the conditional declaration is instead a real,
                        // version-gated polyfill implementation whose own
                        // method bodies still get checked against
                        // whatever won the name: real corpus shape,
                        // `symfony/polyfill-php84`'s own conditionally-
                        // declared `ReflectionConstant` (`if
                        // (PHP_VERSION_ID < 80400) { final class
                        // ReflectionConstant { private $value; ... } }`)
                        // collides with jetbrains/phpstorm-stubs' own
                        // native-class stub for the *real* PHP 8.4+
                        // `ReflectionConstant`. The polyfill's own
                        // `getValue()`/`__toString()`/etc. bodies were
                        // then checked against the stub's much narrower
                        // shape (just `$name`), not the polyfill's real
                        // one - a corpus-verified false positive on every
                        // `$this->value`/`$this->deprecated`/`self::
                        // $persistentConstants` access inside that one
                        // file. Merging (existing wins any real conflict,
                        // same "first/more-authoritative wins" contract
                        // `merge`'s own doc comment establishes) fills in
                        // exactly the members the winning declaration
                        // doesn't have, without touching anything it
                        // already provides.
                        //
                        // `extends`/`implements`/`uses`/`templates`/
                        // `extends_args`/`trait_aliases`/`implements_args`
                        // are the one part of `merge` deliberately
                        // *excluded* here, restored to the winning
                        // declaration's own values right after - unlike a
                        // member map (additive, never wrong to have more
                        // of), a fictitious inheritance edge is actively
                        // dangerous: real corpus regression caught before
                        // shipping, `nikic/php-parser`'s own conditional
                        // `class PhpToken extends
                        // Symfony\Polyfill\Php80\PhpToken {}` (guarding
                        // PHP <8.0, itself colliding with the exact same
                        // stub-vs-conditional pattern this fix targets)
                        // merged that never-taken `extends` onto the real
                        // `\PhpToken` stub, which a wholly unrelated
                        // third file's own `TokenPolyfill extends
                        // \PhpToken {}` then inherited transitively -
                        // wrongly flagging `TokenPolyfill::__construct()`
                        // as overriding a `final` method it doesn't, in
                        // real PHP, have any relation to at all.
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
                    let new_sig = FunctionSig::from_decl(
                        f,
                        ctx,
                        &std::collections::HashMap::new(),
                        &std::collections::HashMap::new(),
                        Visibility::Public,
                    );
                    if conditional {
                        // Gated on arity *and* not losing any type
                        // precision the existing slot already had (see
                        // `conditional_declaration_may_replace_stub`'s own
                        // doc comment for both real corpus shapes this
                        // guards against) - keeps the door open for
                        // whichever real conditional declaration actually
                        // matches best, wherever in the corpus that turns
                        // out to be, rather than committing to the first
                        // one seen regardless of quality. Runs the
                        // identical comparison whether the existing slot
                        // came from a built-in stub *or* an earlier
                        // ordinary conditional declaration of the same
                        // global function - a plain `self.stub_provided_
                        // functions.remove(&name)` gate here used to
                        // decide *when this comparison even runs at all*,
                        // so two real, non-stub conditional declarations
                        // of the same name (two separate `if (!function_
                        // exists(...)) { function foo(...) {...} }`
                        // guards in two unrelated vendor files) fell
                        // straight to a plain first-wins `or_insert_with`
                        // below, never tiebroken at all - real corpus
                        // find: an alphabetically-earlier, `func_get_args(
                        // )`-forwarding 0-param shim (`scoper-autoload.
                        // php`) won registry-wide over the real,
                        // correctly-typed 3-required-plus-variadic
                        // `symfony/deprecation-contracts` declaration,
                        // producing phantom `arguments.count` false
                        // positives at every real call site matching the
                        // real signature. `.remove(&name)` on a name that
                        // was never stub-provided in the first place is a
                        // harmless no-op (`HashSet::remove` returns
                        // `false`, nothing else), so unconditionally
                        // clearing the marker whenever the new
                        // declaration wins is correct either way. Uses
                        // `better_conditional_function_declaration`
                        // (which also lets a real, non-zero-parameter
                        // declaration replace an entirely parameter-less
                        // `existing` one, on top of the exact-arity-match
                        // rule `conditional_declaration_may_replace_stub`
                        // alone still enforces the rest of the time) -
                        // safe to use uniformly for both origins: a stub
                        // is realistically never itself zero-parameter,
                        // so this never weakens the existing stub-
                        // protection tests.
                        match self.functions.get(&name) {
                            Some(existing)
                                if !better_conditional_function_declaration(&new_sig, existing) =>
                            {
                                // `existing` already wins this comparison
                                // - leave it in place, matching `or_insert_
                                // with`'s own "first one seen (that's good
                                // enough) wins" behavior for the case this
                                // fix doesn't change.
                            }
                            _ => {
                                self.stub_provided_functions.remove(&name);
                                self.functions.insert(name, new_sig);
                            }
                        }
                    } else if self.unconditionally_declared_functions.contains(&name) {
                        // Phase 32 Finding C: a *second* unconditional
                        // declaration of the same name - the real-overload
                        // shape described on this registry's own
                        // `unconditionally_declared_functions` field -
                        // merges onto the first rather than replacing it.
                        //
                        // Deliberately does *not* also clear
                        // `stub_provided_functions` here, unlike the
                        // "first real declaration, replaces outright"
                        // branch below - real corpus shape: Piwigo itself
                        // vendors `jetbrains/phpstorm-stubs` as a real
                        // dependency, so its own unconditional (bare,
                        // guard-free) `function str_contains(string
                        // $haystack, string $needle): bool {}` gets
                        // indexed as if it were ordinary project source,
                        // merging harmlessly onto the identical signature
                        // `merge_stubs` already put there - clearing the
                        // marker here would wrongly tell a *later*,
                        // better-typed conditional polyfill declaration
                        // (`?string`-typed, from the real
                        // symfony/polyfill-php80 package) that the slot
                        // is no longer stub-equivalent and off limits,
                        // right back to this phase's own original bug.
                        let existing = self.functions.remove(&name).expect(
                            "name is in unconditionally_declared_functions, so it must already be indexed",
                        );
                        self.functions.insert(name, existing.merge(new_sig));
                    } else {
                        self.stub_provided_functions.remove(&name);
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
                StmtKind::GlobalConst(consts) => {
                    for c in consts {
                        let name = ctx.declared_name(&c.name);
                        let ty = infer_const_type(&c.value);
                        register_global_constant(&mut self.global_constants, name, ty);
                    }
                }
                StmtKind::Expression(e) => {
                    if let Some((name, ty)) = define_call_constant(e) {
                        register_global_constant(&mut self.global_constants, name, ty);
                    }
                }
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
            self.stub_provided_functions.insert(name.clone());
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

    /// `@phpstan-consistent-constructor`'s own ancestor walk - real
    /// PHPStan's `ConsistentConstructorHelper::findConsistentConstructor`,
    /// transcribed (confirmed directly, `vendor/phpstan-src`): starting at
    /// `class`'s own *parent* (not `class` itself - matches the real
    /// rule's own call site, which already has the child's own method in
    /// hand and is looking for what it's being held *to*), walks up
    /// `extends` until an ancestor tagged `@phpstan-consistent-constructor`
    /// is found, returning that ancestor's own `__construct` - or `None`
    /// if the chain ends (or cycles - bounded by `self.classes.len()`,
    /// same guard `Registry::flatten_traits`'s own cycle protection uses)
    /// with no tagged ancestor at all, meaning no check applies, real
    /// PHP's own default.
    ///
    /// Deliberately doesn't synthesize a zero-parameter dummy constructor
    /// for a tagged ancestor with no `__construct` of its own the way real
    /// PHPStan's `DummyConstructorReflection` does - every `FunctionSig`
    /// this project returns is borrowed from `self.classes`, and a class
    /// that bothers tagging itself `@phpstan-consistent-constructor` at
    /// all almost always also declares the constructor it's pinning down
    /// (the entire point of the tag), so this narrower scope isn't
    /// expected to leave real value on the table.
    pub(crate) fn find_consistent_constructor_ancestor(
        &self,
        class: &str,
    ) -> Option<(String, &FunctionSig)> {
        let mut current = self.classes.get(class)?.extends.first()?.clone();
        for _ in 0..self.classes.len().max(1) {
            let info = self.classes.get(&current)?;
            if info.is_consistent_constructor {
                return info
                    .methods
                    .get("__construct")
                    .map(|sig| (current.clone(), sig));
            }
            current = info.extends.first()?.clone();
        }
        None
    }

    /// Phase 64 follow-up: whether `class` directly `use`s a trait that
    /// declares a method named `method` *abstract* - used only by
    /// `#[\Override]`-attribute checking, which needs to treat
    /// implementing a *trait's own* abstract method as a real override
    /// too (confirmed directly via `php -r`: a class implementing a
    /// trait's own `abstract protected function twist()` with
    /// `#[\Override]` is real, legal PHP - a genuine corpus false
    /// positive: `Psl\RandomSequence\MersenneTwisterSequence::twist()`
    /// overrides `Internal\MersenneTwisterTrait`'s own abstract
    /// declaration this exact way). Deliberately checks the trait's own
    /// `abstract_methods` set, not merely "does the trait have a method
    /// of this name at all" - a trait supplying a *concrete* method the
    /// using class never separately redeclares (own_sig IS the trait's
    /// own contribution, unmodified) is a different case entirely, with
    /// its own already-correct handling: `check_method_overrides`'s own
    /// trait-derived-method sweep re-runs this same function keyed on the
    /// using class, so a genuinely ancestor-less concrete trait method
    /// still fatals there exactly as real PHP does (confirmed directly:
    /// `trait T { #[\Override] function foo() {} } class Standalone {
    /// use T; }` still fatals on `Standalone::foo()`) - this check must
    /// not accidentally swallow that real case too. Deliberately narrower
    /// than folding `uses` into `lookup_ancestor_method_with_owner`'s own
    /// general walk (the "Still open: interface-first/trait-abstract-
    /// method ancestor-lookup priority" item this project's own roadmap
    /// already tracks separately) - that would also change every *other*
    /// check in `check_one_method_override` (Liskov parameter/return-type
    /// comparisons) in ways not verified here; this only answers the one
    /// yes/no question the override-attribute check itself needs.
    pub fn trait_declares_abstract_method(&self, class: &str, method: &str) -> bool {
        let Some(info) = self.classes.get(class) else {
            return false;
        };
        info.uses.iter().any(|used_trait| {
            self.classes
                .get(used_trait)
                .is_some_and(|trait_info| trait_info.abstract_methods.contains(method))
        })
    }

    /// Phase 64 follow-up: whether `class`'s own `extends`/`implements`
    /// ancestor chain includes a name this project's registry has no
    /// declaration for at all, at any depth - used only by
    /// `#[\Override]`-attribute checking, which must never confidently
    /// claim "no matching parent method exists" when the real reason
    /// nothing was found is a missing dependency rather than a genuine
    /// absence (a genuine corpus false positive: `CycloneDX\Composer\
    /// Plugin implements ... Composer\Plugin\PluginInterface` and
    /// `CycloneDX\Composer\_internal\MakeBom\Command extends Composer\
    /// Command\BaseCommand` - neither vendored anywhere in this project's
    /// own verification corpus at all, so this registry has no way to
    /// know whether the real `activate()`/`configure()`/... methods being
    /// overridden are declared there, the same "don't guess wrong" rule
    /// this project applies elsewhere). Every other check in
    /// `check_one_method_override` stays as lenient-by-omission as it
    /// already was (an unresolved ancestor simply yields no comparison,
    /// same as a genuinely absent one) - only this specific check's
    /// polarity (firing exactly when *nothing* is found) makes an
    /// unresolved-vs-absent distinction actually matter.
    pub fn has_unresolvable_ancestor(&self, class: &str) -> bool {
        let Some(info) = self.classes.get(class) else {
            return false;
        };
        let mut seen = HashSet::new();
        let mut queue: VecDeque<String> = info
            .extends
            .iter()
            .chain(info.implements.iter())
            .cloned()
            .collect();
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(current_info) = self.classes.get(&current) else {
                return true;
            };
            queue.extend(
                current_info
                    .extends
                    .iter()
                    .chain(current_info.implements.iter())
                    .cloned(),
            );
        }
        false
    }

    /// Phase 64's own still-open property-override half: same idea as
    /// `lookup_ancestor_method_with_owner` just above, but walking only
    /// `extends` - real PHP interfaces can't declare properties at all,
    /// so there's no `implements` side to check.
    pub fn lookup_ancestor_property_with_owner(
        &self,
        class: &str,
        prop: &str,
    ) -> Option<(String, &PropertySig)> {
        let info = self.classes.get(class)?;
        for parent in &info.extends {
            if let Some(found) = self.lookup_property_with_owner(parent, prop) {
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
    pub fn lookup_const(&self, class: &str, const_name: &str) -> Option<&ConstSig> {
        self.lookup_in_hierarchy(class, |info| info.consts.get(const_name))
    }

    /// Same as `lookup_const`, but also returns the declaring ancestor's
    /// name - Phase 67 needs it to know *which* class's own `@internal`
    /// tag (if any) actually governs a `Class::CONST` access, the same
    /// way `lookup_method_with_owner`/`lookup_property_with_owner`
    /// already do for their own members; Phase 57's own still-open item
    /// (constant visibility enforcement) needs it the identical way too.
    pub fn lookup_const_with_owner(
        &self,
        class: &str,
        const_name: &str,
    ) -> Option<(String, &ConstSig)> {
        self.lookup_in_hierarchy_with_owner(class, |info| info.consts.get(const_name))
    }

    /// Phase 79: a backed enum case's own literal backing value, if this
    /// checker was able to prove one (see `ClassInfo::enum_case_values`'s
    /// own doc comment) - not hierarchy-walked, since an enum can't
    /// extend anything for a case to inherit from in the first place.
    pub fn enum_case_value(&self, class: &str, case: &str) -> Option<&Type> {
        self.classes.get(class)?.enum_case_values.get(case)
    }

    /// Phase 79: the matching case (if any, and if provable) for a
    /// literal argument passed to a backed enum's own `::from()`/
    /// `::tryFrom()` - a reverse lookup over `enum_case_value` above,
    /// since PHP itself only stores value -> case at runtime, never the
    /// other way around.
    pub fn enum_case_for_value(&self, class: &str, value: &Type) -> Option<&str> {
        let info = self.classes.get(class)?;
        info.enum_case_values
            .iter()
            .find(|(_, v)| *v == value)
            .map(|(name, _)| name.as_str())
    }

    /// Phase 71 corpus find: real PHP function calls are case-
    /// insensitive (`STRTOLOWER()`/`strtolower()`/`StrToLower()` all
    /// resolve to the exact same builtin, and the same is true of a
    /// user-declared function under whatever case its own call site
    /// happens to use) - `self.functions` itself stays keyed by
    /// whatever case each function was actually *declared* under (every
    /// existing insertion call site is untouched), so an exact-case miss
    /// falls back to a case-insensitive scan before giving up. Checked
    /// second, not first: the overwhelming majority of real calls
    /// already match a declaration's own case exactly, so the common
    /// path stays a plain, allocation-free `HashMap` lookup.
    pub fn lookup_function(&self, name: &str) -> Option<&FunctionSig> {
        self.functions.get(name).or_else(|| {
            self.functions
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v)
        })
    }

    /// This class's own declared variance for the `index`-th `@template`
    /// of `class` (`Covariant`/`Contravariant`/`Invariant` - the same
    /// three-way classification `TemplateParam::variance` itself carries,
    /// just looked up by position rather than read off an already-marked
    /// node) - `None` when `class` isn't a registered class at all, or
    /// declares fewer templates than `index` needs, either of which means
    /// this project has no real basis to say whether subtyping flows
    /// through that slot at all, let alone which direction. Callers treat
    /// `None` as "don't know, don't guess" - matching this project's
    /// general leniency elsewhere - rather than defaulting to `Invariant`
    /// and risking a false positive on a real but unregistered
    /// (uninstalled dependency, PHAR-bundled `phpstan/phpstan` itself, ...)
    /// generic class. Phase 54: hoisted from `checker::generics` (pure
    /// `self.classes` data, no checker-specific state) alongside
    /// `class_template_bindings`/`is_assignable`, its two real consumers.
    fn slot_variance(&self, class: &str, index: usize) -> Option<Variance> {
        let info = self.classes.get(class)?;
        let name = info.templates.get(index)?;
        Some(if info.covariant_templates.contains(name) {
            Variance::Covariant
        } else if info.contravariant_templates.contains(name) {
            Variance::Contravariant
        } else {
            Variance::Invariant
        })
    }

    /// Phase 54: the variance that actually governs one generic argument
    /// slot - a call-site variance marker written directly on `arg`
    /// (`types::strip_call_site_variance`'s own sentinels,
    /// `"$$covariant"`/`"$$contravariant"`/`"$$bivariant"`) always wins
    /// when present; otherwise falls back to `class`'s own declared
    /// variance for that slot (`slot_variance`, `Invariant` by default
    /// when the class declares no variance for it, `None`/"don't know"
    /// when `class` or the slot itself isn't registered at all). Read
    /// directly off real PHPStan's own `GenericObjectType::
    /// isSuperTypeOf`/`getReferencedTemplateTypes`, both of which resolve
    /// a slot's variance with this exact same two-step rule - confirmed
    /// this is genuinely the same concept both assignability
    /// (`is_assignable` below) and declaration-soundness composition
    /// (`checker::generics::walk_variance`) need, not something specific
    /// to one or the other. `None` here (as opposed to `Invariant`) means
    /// "no real basis to say anything about this slot at all," the same
    /// "don't know, don't guess" case `slot_variance` itself already
    /// returns `None` for.
    pub(crate) fn effective_slot_variance(
        &self,
        class: &str,
        index: usize,
        arg: &Type,
    ) -> Option<Variance> {
        match arg {
            Type::Object(name, _) if name == "$$covariant" => Some(Variance::Covariant),
            Type::Object(name, _) if name == "$$contravariant" => Some(Variance::Contravariant),
            // Real PHPStan has no separate "bivariant" `Variance` value
            // reachable through ordinary composition (`Bivariant` only
            // ever comes from a call-site `*`, never a class's own
            // declared variance) - this project doesn't need one either,
            // since every consumer already treats "always compatible,
            // don't recurse further" as its own terminal case rather
            // than a fourth composable variance kind. `is_assignable`
            // checks for the `"$$bivariant"` sentinel directly for this
            // reason; `walk_variance` recursing into a bivariant slot's
            // own (never meaningfully variance-checkable) argument is
            // harmless - there's no real type there to find a
            // `TemplateParam` inside in the first place.
            Type::Object(name, _) if name == "$$bivariant" => None,
            _ => self.slot_variance(class, index),
        }
    }

    /// Binds a generic-typed receiver's own carried type arguments
    /// (`Collection<User>` -> `{"T": Object("User", [])}`) against
    /// `target`'s own declared `@template` parameter names, walking up
    /// `class`'s `extends`/`implements` chain to reach it if `target`
    /// isn't `class` itself (Phase 10.5) - so a concrete subclass that
    /// fixes its parent's template param (`class IntCollection extends
    /// Collection<int> {}`) resolves `Collection`'s own `T` to `int` even
    /// though `IntCollection` isn't generic itself, and a generic
    /// subclass that passes its own template through (`class Box<T>
    /// extends Collection<T> {}`) resolves it to whatever `Box`'s own `T`
    /// is bound to at each use, not a fixed value.
    ///
    /// `target` matters because template names are scoped to the class
    /// that declares them, not globally unique - two unrelated classes in
    /// the same chain commonly both use `T` (confirmed the hard way on a
    /// real corpus: `symfony/type-info`'s `NullableType<T of Type>
    /// extends UnionType<T|BuiltinType<...>>`, where `UnionType` has its
    /// *own*, unrelated `T`). Returning bindings scoped to exactly
    /// `target`'s own level - not a single flat map merged across the
    /// whole chain - is what keeps those two `T`s from colliding into
    /// whichever one happened to be computed last.
    ///
    /// A non-generic class (empty `templates`), a bare unspecified-generic
    /// reference (empty `type_args`), or a chain with no matching
    /// `@extends`/`@implements` tag along the way to `target` all just
    /// leave that level's names unbound - `substitute_template` already
    /// treats an unbound `TemplateParam` as `Mixed`, the right
    /// conservative fallback for an "erased" generic. Phase 54: hoisted
    /// from `checker::generics` alongside `slot_variance`/`is_assignable`.
    pub(crate) fn class_template_bindings(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
    ) -> HashMap<String, Type> {
        let mut seen = HashSet::new();
        self.class_template_bindings_rec(class, type_args, target, &mut seen)
            .unwrap_or_default()
    }

    /// `None` means `target` isn't reachable along this path at all (so
    /// the caller should treat every one of its templates as unbound,
    /// same as `Some` with an empty map) - the distinction only matters
    /// internally, to know when to keep searching sibling branches below.
    /// `seen` guards against a cyclic `extends`/`implements` graph
    /// (invalid PHP, but a malformed/partially-loaded real corpus could
    /// still produce one) - cloned per branch, not threaded through by
    /// mutable reference, so two independent ancestors (e.g. two
    /// interfaces that both eventually reach a common shared parent, a
    /// real "diamond" shape) can each still be searched.
    fn class_template_bindings_rec(
        &self,
        class: &str,
        type_args: &[Type],
        target: &str,
        seen: &mut HashSet<String>,
    ) -> Option<HashMap<String, Type>> {
        if !seen.insert(class.to_string()) {
            return None;
        }
        let info = self.classes.get(class)?;
        let own_bindings: HashMap<String, Type> = info
            .templates
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect();

        if class == target {
            return Some(own_bindings);
        }

        if let Some(parent) = info.extends.first() {
            let parent_args: Vec<Type> = info
                .extends_args
                .iter()
                .map(|t| types::substitute_template(t, &own_bindings))
                .collect();
            if let Some(found) =
                self.class_template_bindings_rec(parent, &parent_args, target, &mut seen.clone())
            {
                return Some(found);
            }
        }
        for iface in &info.implements {
            let iface_args: Vec<Type> = info
                .implements_args
                .get(iface)
                .map(|args| {
                    args.iter()
                        .map(|t| types::substitute_template(t, &own_bindings))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(found) =
                self.class_template_bindings_rec(iface, &iface_args, target, &mut seen.clone())
            {
                return Some(found);
            }
        }
        None
    }

    /// Type-compatibility check that consults the class hierarchy for
    /// `Object` types; every other case defers to `types::is_subtype`,
    /// which has no notion of class context.
    pub fn is_assignable(&self, sub: &Type, sup: &Type) -> bool {
        match (sub, sup) {
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
            (Type::Array(_, _) | Type::ArrayShape(_), Type::Callable) => true,
            // Phase 54: type arguments used to be ignored here entirely
            // (`Collection<Cat>` and `Collection<Dog>` were mutually
            // assignable, always) - this project now genuinely models
            // variance (`Type::TemplateParam::variance`, `@template-
            // covariant`/`-contravariant`, and Phase 54's own call-site
            // `covariant`/`contravariant`/`*` annotations,
            // `types::parse_generic_arg`), so this comparison now
            // actually applies it instead of skipping it. If `sup`'s own
            // class isn't registered at all (an uninstalled dependency,
            // or a PHAR-bundled one like `phpstan/phpstan` itself with no
            // loose source to parse - the same "can't verify, don't
            // guess" case `check_generic_bounds`'s own doc comment
            // names), fall back to the pre-Phase-54 name/hierarchy-only
            // comparison unchanged - the one deliberate leniency left in
            // this design.
            // Phase 54: type arguments used to be ignored here entirely
            // (`Collection<Cat>` and `Collection<Dog>` were mutually
            // assignable, always) - this project now genuinely models
            // variance (`Type::TemplateParam::variance`, `@template-
            // covariant`/`-contravariant`, and Phase 54's own call-site
            // `covariant`/`contravariant`/`*` annotations,
            // `types::parse_generic_arg`), so this comparison now
            // actually applies it instead of skipping it. If `sup`'s own
            // class isn't registered at all (an uninstalled dependency,
            // or a PHAR-bundled one like `phpstan/phpstan` itself with no
            // loose source to parse - the same "can't verify, don't
            // guess" case `check_generic_bounds`'s own doc comment
            // names), fall back to the pre-Phase-54 name/hierarchy-only
            // comparison unchanged - the one deliberate leniency left in
            // this design.
            (Type::Object(a, args_a), Type::Object(b, args_b)) => {
                if a != b && !self.is_subclass_of(a, b) {
                    return false;
                }
                let Some(info_b) = self.classes.get(b) else {
                    return true;
                };
                // A missing trailing slot (an "erased"/under-supplied
                // generic reference - a bare `Collection` with no `<T>`
                // at all is the overwhelmingly common real shape) resolves
                // to plain `Mixed`, not that template's own declared
                // bound. Deliberately verified, not assumed: padding with
                // the *bound* instead would make a bare reference to any
                // class with a real (non-`Mixed`) bound - a very common
                // shape, `@template T of SomeInterface` - fail the
                // Invariant comparison below against *any* concrete
                // argument, since `bound != concrete-subtype` invariantly
                // (`is_assignable(bound, concrete)` fails whenever
                // `concrete` is a strict subtype of `bound`) - a real,
                // dangerous new false-positive class this project doesn't
                // have data to avoid anyway: `ClassInfo` only persists
                // `template_bounds`, never a class-level `@template T =
                // Default` map the way `TemplateParam.default` carries
                // one for a function/method's own templates (`types::
                // substitute_template`'s own real, shipped behavior pads
                // with `.default`, falling back to `Mixed` - never the
                // bound). Plain `Mixed` is both what's actually available
                // and what already skips this slot's comparison entirely
                // via the "either side Mixed" rule below - correctly
                // lenient for the common case without guessing wrong.
                // `args_a` is in `a`'s own frame - map it into `b`'s own
                // template names via the same ancestor-substitution walk
                // `extends_args`/`implements_args` already use elsewhere,
                // unless `a == b` and it's already in the right frame.
                let effective_args_a: Vec<Type> = if a == b {
                    args_a.clone()
                } else {
                    let bindings = self.class_template_bindings(a, args_a, b);
                    info_b
                        .templates
                        .iter()
                        .map(|name| bindings.get(name).cloned().unwrap_or(Type::Mixed))
                        .collect()
                };
                for (i, _name) in info_b.templates.iter().enumerate() {
                    let a_i = effective_args_a.get(i).cloned().unwrap_or(Type::Mixed);
                    let b_i = args_b.get(i).cloned().unwrap_or(Type::Mixed);
                    // A call-site variance marker on `sup`'s own argument
                    // (`Collection<covariant Foo>`) overrides `b`'s own
                    // declared slot variance for this one comparison,
                    // exactly matching `GenericObjectType::isSuperTypeOf`/
                    // `getReferencedTemplateTypes` read directly from
                    // `vendor/phpstan-src` - `None` here means a
                    // `"$$bivariant"` (`*`) marker, real PHPStan's own
                    // "don't check this argument at all."
                    let Some(variance) = self.effective_slot_variance(b, i, &b_i) else {
                        continue;
                    };
                    let a_i = types::strip_call_site_variance(&a_i)
                        .cloned()
                        .unwrap_or(Type::Mixed);
                    let b_i = types::strip_call_site_variance(&b_i)
                        .cloned()
                        .unwrap_or(Type::Mixed);
                    // Either side unresolved/erased - don't know, don't
                    // guess, matches this project's leniency everywhere
                    // else a generic argument can't be verified. A raw,
                    // still-unbound `TemplateParam` (a method/function's
                    // own `@template`, e.g. `E` in `EntityRepository<E>
                    // $repository`) is included here too, not just
                    // `Mixed` - a real, corpus-found false positive this
                    // fixed: `checker::calls`'s own call-argument
                    // template binding only ever binds a template used
                    // as a *bare* param type (`T $value`), never one
                    // nested inside another class's own generic argument
                    // list, so `E` reaches this comparison exactly as
                    // unbound as it was declared - comparing it
                    // structurally against the real, concrete argument
                    // is exactly as wrong as comparing `Mixed` would be.
                    // Real corpus shape: Piwigo's own `TypedRepository::
                    // narrow(EntityRepository<E> $repository)` with
                    // `@template E of object` - before this skip existed,
                    // the pre-existing `(TemplateParam(tp), _) =>
                    // matches!(*tp.bound, Type::Mixed) || is_assignable(
                    // &tp.bound, sup)` arm ran the `object`-bound `E`'s
                    // own upper bound against the real argument's
                    // concrete class, `is_assignable(object, Concrete
                    // Entity)`, and correctly said `false` (an arbitrary
                    // object is genuinely not guaranteed to be that one
                    // specific class - not a bug in that arm; a weak
                    // bound like `object` legitimately can't prove the
                    // narrower assignment sound). Skipping this slot
                    // entirely for an unbound template - rather than
                    // trying to make that comparison artificially permit
                    // it - is what's actually correct: the real answer
                    // isn't "yes" here, it's "not yet known, ask again
                    // once `E` is bound," which this project's own call-
                    // argument binding is simply never invoked for at
                    // this nested position.
                    if a_i == Type::Mixed
                        || b_i == Type::Mixed
                        || matches!(a_i, Type::TemplateParam(_))
                        || matches!(b_i, Type::TemplateParam(_))
                    {
                        continue;
                    }
                    let ok = match variance {
                        Variance::Covariant => self.is_assignable(&a_i, &b_i),
                        Variance::Contravariant => self.is_assignable(&b_i, &a_i),
                        // Mutual assignability approximates real
                        // PHPStan's exact `equals()` here - deliberately
                        // chosen over raw structural `==`, since this
                        // codebase's own `Union`/`Intersection` values
                        // have no canonical member order
                        // (`checker::expr::union_types` dedupes but never
                        // sorts), making `==` order-sensitive and *more*
                        // likely to falsely reject two independently
                        // built but semantically identical unions than
                        // mutual `is_assignable` is to miss a genuine
                        // mismatch.
                        Variance::Invariant => {
                            self.is_assignable(&a_i, &b_i) && self.is_assignable(&b_i, &a_i)
                        }
                    };
                    if !ok {
                        return false;
                    }
                }
                true
            }
            // Phase 79: hierarchy-aware version of `types::is_subtype`'s
            // own bare `EnumCase -> Object` name-equality rule - a case
            // is assignable not just to its own enum's bare type but to
            // `UnitEnum`/`BackedEnum` (both synthesized `implements`
            // entries every enum gets, see `ClassInfo::from_enum_decl`)
            // and any interface the enum itself declares.
            (Type::EnumCase(fqn, _), Type::Object(b, _)) => fqn == b || self.is_subclass_of(fqn, b),
            // Two different cases of the same enum are never
            // interchangeable (`types::is_subtype`'s own `a == b` fast
            // path already covers the identical-case case) - no
            // hierarchy to widen through beyond what's handled above.
            (Type::EnumCase(_, _), Type::EnumCase(_, _)) => false,
            // Same hierarchy-aware override as the plain Object-vs-Object
            // arm just above, one level up: `types::is_subtype`'s own
            // `ClassString` arm inherits its non-hierarchy-aware Object
            // comparison (name equality only), so without this a
            // `class-string<Derived>` value would incorrectly fail to
            // satisfy a `class-string<Base>`-typed parameter even when
            // `Derived extends Base`.
            (Type::ClassString(a), Type::ClassString(b)) => self.is_assignable(a, b),
            // Mirrors `types::is_subtype`'s own bound-aware `TemplateParam`
            // arm (see its doc comment for the full reasoning and why
            // this needs to run *before* the `Union` arms just below),
            // but recurses through `self.is_assignable` rather than the
            // plain, hierarchy-*unaware* `types::is_subtype` - a real bug
            // in an earlier version of this exact fix: delegating
            // straight to `types::is_subtype(sub, sup)` silently dropped
            // the class-hierarchy-walking half of this whole method for
            // any bound check involving a `TemplateParam`, wrongly
            // flagging real corpus code where a template's own bound is a
            // union of *concrete classes* needing a real ancestor walk to
            // match a single-class target (`phpstan/phpstan-strict-rules`'s
            // own `@phpstan-template TNodeType of PreInc|PreDec|PostInc|
            // PostDec` satisfying `Rule<TNodeType>`'s own `TNodeType of
            // Node` bound - each of those four classes extends `Node`
            // only via `Expr`, a real, multi-level ancestor chain plain
            // name-equality can't see).
            (Type::TemplateParam(tp), _) => {
                matches!(*tp.bound, Type::Mixed) || self.is_assignable(&tp.bound, sup)
            }
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
            (Type::Array(_, a), Type::Array(_, b)) => {
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
            (Type::Array(_, a), Type::ArrayShape(_)) => {
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
            // Phase 53: the same `Array(Mixed)` leniency two arms above -
            // an individual shape field with no inferred type of its own
            // (an unresolved global constant, most commonly - real shape:
            // nikic/php-parser's own `$token->is([\T_COMMENT,
            // \T_DOC_COMMENT])`, where neither constant is a name this
            // project's registry models) reads as `Mixed`, not "really
            // can hold anything", so confidently failing it against `b`
            // (e.g. `array<int|string>`) is exactly the false positive
            // this leniency already exists to prevent for a homogeneous
            // `Array(Mixed)` - a shape literal deserves the identical
            // treatment per field, not a stricter one just because its
            // own type happens to be more structurally precise.
            (Type::ArrayShape(fields), Type::Array(_, b)) => fields.iter().all(|(_, ty, _)| {
                (!self.strict_mixed_arrays && matches!(ty, Type::Mixed))
                    || self.is_assignable(ty, b)
            }),
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
            // Phase 80: the `object{...}`-flavored sibling of the two
            // `ArrayShape` rules just above - a real class instance
            // satisfies a declared object shape when it has every
            // property the shape actually requires (an optional field
            // may be missing) with an assignable type, checked via
            // `lookup_property` rather than an array field lookup;
            // extra properties the class has beyond what's required are
            // ignored, the same "structural, not exact" spirit. `object`/
            // `stdClass` (dynamic-property bags this project doesn't
            // track the real fields of) are accepted unconditionally,
            // matching `Array(Mixed)`'s own established leniency for the
            // identical "can't verify, don't guess wrong" situation.
            (Type::Object(class, _), Type::ObjectShape(fields)) => {
                class == "object"
                    || class == "stdClass"
                    || fields.iter().all(|(name, sup_ty, optional)| {
                        match self.lookup_property(class, name) {
                            Some(p) => self.is_assignable(&p.ty, sup_ty),
                            None => *optional,
                        }
                    })
            }
            (Type::ObjectShape(sub_fields), Type::ObjectShape(sup_fields)) => {
                sup_fields.iter().all(|(name, sup_ty, optional)| {
                    match sub_fields.iter().find(|(n, _, _)| n == name) {
                        Some((_, sub_ty, _)) => self.is_assignable(sub_ty, sup_ty),
                        None => *optional,
                    }
                })
            }
            // Same unresolved-`@phpstan-type`-alias leniency as the
            // Object-vs-Object arm above, generalized to every other
            // `sub` shape: Doctrine DBAL's own `DriverManager::
            // getConnection(Params $params)` is called with a real array
            // *literal* (an `ArrayShape`) at nearly every real call site,
            // never an actual `Params`-named object - only the
            // Object-vs-Object arm's leniency would leave those failing
            // just the same way. Excludes the real `object` pseudo-type
            // (PHP's own native `object` hint - already handled, and
            // correctly strict, in the dedicated arm above): unlike a
            // genuinely-unresolvable alias name, `object` means exactly
            // "some real object instance" and a plain array/string/etc.
            // is correctly never one.
            (_, Type::Object(name, _)) if name != "object" && !self.classes.contains_key(name) => {
                true
            }
            _ => types::is_subtype(sub, sup),
        }
    }
}

/// The namespace a fully-qualified class name was declared in - everything
/// before the last `\`, empty for the global namespace. `Registry::
/// resolve_deferred_type_imports`'s own doc comment explains why this is
/// exactly the prefix a bare `@phpstan-type` alias name declared on that
/// same class resolves against.
fn class_namespace(fqn: &str) -> &str {
    fqn.rsplit_once('\\').map_or("", |(ns, _)| ns)
}

/// `NameContext::declared_name`'s exact logic, replicated for a namespace
/// string reconstructed after the fact (`class_namespace`) rather than a
/// live `NameContext` - no leading `\` for the global namespace, which a
/// naive `format!("{ns}\\{bare}")` would always insert (a real bug this
/// project's own global-namespace corpus fixtures caught immediately: an
/// alias declared with no namespace at all keys its own `type_aliases`
/// entry under the bare name, never `"\\Name"`).
fn declared_name_in_namespace(namespace: &str, bare: &str) -> String {
    if namespace.is_empty() {
        bare.to_string()
    } else {
        format!("{namespace}\\{bare}")
    }
}

/// Cheap, read-only pre-check for `Registry::resolve_deferred_value_of`:
/// does `ty` contain the `"$$valueof"` sentinel anywhere in its tree at
/// all - lets every call site skip the (much more expensive) rebuild-with-
/// clone `resolve_deferred_value_of_in_type` below does for the
/// overwhelming majority of types, which never contain it.
fn type_contains_deferred_value_of(ty: &Type) -> bool {
    match ty {
        Type::Object(name, args) => {
            name == "$$valueof" || args.iter().any(type_contains_deferred_value_of)
        }
        Type::Array(key, inner) => {
            type_contains_deferred_value_of(key) || type_contains_deferred_value_of(inner)
        }
        Type::ClassString(inner) => type_contains_deferred_value_of(inner),
        Type::Union(members) | Type::Intersection(members) => {
            members.iter().any(type_contains_deferred_value_of)
        }
        Type::ArrayShape(fields) | Type::ObjectShape(fields) => fields
            .iter()
            .any(|(_, t, _)| type_contains_deferred_value_of(t)),
        _ => false,
    }
}

/// The actual substitution - mirrors `function_sig::substitute_type_aliases`'s
/// own recursive-rebuild shape exactly, with one extra case (checked
/// first) for the `"$$valueof"` sentinel itself. See `Registry::
/// resolve_deferred_value_of`'s own doc comment for why this exists at
/// all, and `types::parse_type_keyword_or_class`'s `"value-of"` arm for
/// where the sentinel is produced.
fn resolve_deferred_value_of_in_type(ty: Type, classes: &FxHashMap<String, ClassInfo>) -> Type {
    match ty {
        Type::Object(name, mut args) if name == "$$valueof" => {
            let Some(Type::Object(target_fqn, _)) = args.pop() else {
                return Type::Mixed;
            };
            resolve_value_of_target(&target_fqn, classes)
        }
        Type::Object(name, args) => Type::Object(
            name,
            args.into_iter()
                .map(|a| resolve_deferred_value_of_in_type(a, classes))
                .collect(),
        ),
        Type::Array(key, inner) => Type::Array(
            Box::new(resolve_deferred_value_of_in_type(*key, classes)),
            Box::new(resolve_deferred_value_of_in_type(*inner, classes)),
        ),
        Type::ClassString(inner) => {
            Type::ClassString(Box::new(resolve_deferred_value_of_in_type(*inner, classes)))
        }
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| resolve_deferred_value_of_in_type(m, classes))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|m| resolve_deferred_value_of_in_type(m, classes))
                .collect(),
        ),
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, resolve_deferred_value_of_in_type(t, classes), opt))
                .collect(),
        ),
        Type::ObjectShape(fields) => Type::ObjectShape(
            fields
                .into_iter()
                .map(|(k, t, opt)| (k, resolve_deferred_value_of_in_type(t, classes), opt))
                .collect(),
        ),
        other => other,
    }
}

/// `value-of<target_fqn>`'s own real resolved value, once `target_fqn`'s
/// class/enum data actually exists to consult - mirrors real PHPStan's own
/// `ValueOfType::getResult()` exactly: not an enum at all (or not a
/// registered class/enum) degrades to `Mixed` (can't verify, don't guess -
/// the same leniency real PHPStan's own non-enum `getIterableValueType()`
/// fallback amounts to for an ordinary class); an unbacked enum (no real
/// `->value` property, since real PHP never gives one to a non-backed
/// enum) is `Never`, matching `ValueOfType::getResult()`'s own
/// `count($valueTypes) === 0 -> NeverType`; a backed enum's own already-
/// built `properties["value"]` (`registry/class_info.rs`, built for the
/// enum's real `->value` property) is already the exact union-of-case-
/// literal-values type real PHPStan computes independently - reused
/// directly, not recomputed.
fn resolve_value_of_target(target_fqn: &str, classes: &FxHashMap<String, ClassInfo>) -> Type {
    let Some(info) = classes.get(target_fqn) else {
        return Type::Mixed;
    };
    if info.kind != ClassKind::Enum {
        return Type::Mixed;
    }
    match info.properties.get("value") {
        Some(sig) => sig.ty.clone(),
        None => Type::Never,
    }
}

/// Resolves `ty` (a param/return type that may contain the `"$$valueof"`
/// sentinel) and, if the result is bare `Type::Mixed`, falls back to
/// `native` instead when that's itself something more useful - restores
/// real PHPStan's own (and this project's pre-existing)
/// `function_sig::reconcile_with_native` behavior for a `value-of<T>`
/// that turns out unresolvable (`T` not a real, registered enum), which
/// `member_possible_given_native`'s own deliberate, unconditional trust of
/// the still-unresolved sentinel (see its own doc comment) would otherwise
/// have already locked in as "kept" well before this pass ever runs. Real
/// corpus false positive this fixes: `php-standard-library`'s own
/// `BackedEnumValueType::coerce(): string|int`, `@return value-of<T>` for
/// an unresolvable `@template T` - without this, the native `string|int`
/// this project already trusted here before this whole feature existed
/// was silently replaced by a bare, strictly worse `Mixed`.
///
/// Only `ParamSig::ty`/`FunctionSig::return_type` get this treatment -
/// they're the only two places this registry persists a native-only
/// counterpart type to fall back to at all (`ParamSig::ty_native`/
/// `FunctionSig::return_type_native`, both built for an unrelated,
/// earlier override-compatibility need). `PropertySig`/`ConstSig`/
/// `ClassInfo::template_bounds` have no such counterpart stored (Phase
/// 64's own disclosed, pre-existing gap - see its own ROADMAP entry) -
/// an unresolvable `value-of<T>` there stays plain `Mixed`, a narrower,
/// disclosed simplification with no current corpus evidence it matters.
fn resolve_deferred_value_of_with_native_fallback(
    ty: Type,
    native: &Type,
    classes: &FxHashMap<String, ClassInfo>,
) -> Type {
    let resolved = resolve_deferred_value_of_in_type(ty, classes);
    if resolved == Type::Mixed && *native != Type::Mixed {
        native.clone()
    } else {
        resolved
    }
}

/// Whether a conditional declaration (`new_sig`) is trustworthy enough to
/// replace `existing` (a stub-provided signature still sitting in its
/// slot, per `stub_provided_functions`) - real corpus find: this needs
/// *two* separate gates, not just one.
///
/// Arity: `scoper-autoload.php`-style `func_get_args()`-forwarding shims
/// (`function grapheme_strrev() { return \Prefix\grapheme_strrev(
/// ...func_get_args()); }`, zero declared params) compete with the real,
/// correctly-typed polyfill declaration for the same name - letting the
/// shim win left the real declaration's own body-check zipping its one
/// real parameter against the shim's zero-param signature, silently
/// losing it (read as undefined inside its own body).
///
/// Type precision: even at matching arity, a polyfill package can bundle
/// *several* PHP-version-gated variants of the same function behind
/// separate `function_exists()` guards, of visibly different quality -
/// real shape: symfony/polyfill-ctype's own `bootstrap.php` (`function
/// ctype_punct($text) {...}`, no native types at all) alongside
/// `bootstrap80.php` (`function ctype_punct(mixed $text): bool {...}`,
/// fully typed) for the exact same name. Letting the untyped one win
/// (whichever file the corpus scan happens to reach first) silently
/// discarded real, already-known type information the stub itself
/// carried - and since every OTHER real file's own `ctype_punct` call
/// sites, including `jetbrains/phpstorm-stubs/ctype/ctype.php`'s own
/// (fully-typed) declaration, all resolve through this one shared
/// registry entry, the damage wasn't confined to whichever file lost the
/// race. Never letting a replacement *downgrade* a parameter or return
/// position that the stub already had typed down to `Mixed` closes this
/// without needing to fully rank two arbitrary types against each other.
/// Recognizes a top-level `define('NAME', value)` (or `\define(...)`)
/// call statement, returning the constant's own literal name plus its
/// inferred value type (`infer_const_type`, the same literal-only scope
/// a `const` declaration's own value already gets). `None` for anything
/// else a bare `StmtKind::Expression` could be, a call to some other
/// function, a spread/named argument (real `define()` never takes
/// either), a non-literal first argument (a dynamic constant name this
/// project has no way to know without evaluating it), or - critically -
/// a name that's really the case-insensitive language keyword `true`/
/// `false`/`null` rather than an ordinary constant: real corpus shape,
/// `jetbrains/phpstorm-stubs/Core/Core_d.php`'s own `define('true',
/// (bool)1, true); define('false', (bool)0, true); define('null', null,
/// true);` - legitimate reflection-level documentation of PHP's own
/// pseudo-constants, present in essentially every real corpus. On a real
/// PHP runtime these three names are parser-level keywords resolved
/// before `define()` could ever apply to them - the call itself is
/// syntactically valid but semantically inert, never actually
/// redefining what the bare token `true`/`false`/`null` means anywhere.
/// Registering them anyway - with whatever `infer_const_type` makes of
/// a `(bool)1` cast expression it doesn't recognize, plain `Mixed` -
/// would let a *global* lookup miss (`Checker::global_constant_type`
/// returning `Some(Mixed)`) silently pre-empt `bare_name_literal_type`'s
/// own already-correct handling of a fully-qualified `\false`/`\true`/
/// `\null` reference (see that function's own doc comment) for every
/// file in the corpus, not just this stub's own - a real, corpus-wide
/// regression caught before shipping: every `!== \false`-style
/// narrowing idiom (extremely common, e.g. `squizlabs/php_codesniffer`'s
/// own `while (($x = $file->findNext(...)) !== \false)`) silently
/// stopped narrowing away `false` at all once this stub was indexed.
fn define_call_constant(e: &Expr) -> Option<(String, Type)> {
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
    if ["true", "false", "null"]
        .iter()
        .any(|kw| const_name.eq_ignore_ascii_case(kw))
    {
        return None;
    }
    // `jetbrains/phpstorm-stubs/standard/basic.php`'s own `define('__DIR__',
    // '', true); define('__LINE__', 0, true); ...` (and identically for
    // `__FILE__`/`__CLASS__`/`__FUNCTION__`/`__METHOD__`/`__TRAIT__`/
    // `__NAMESPACE__`/`__PROPERTY__`) - the exact same class of problem
    // the `true`/`false`/`null` exclusion above closes, for a different
    // reason: these nine names are real PHP *magic constants*, resolved
    // by the parser itself from the surrounding source location, never
    // actually backed by a real `define()`-registered value at all - the
    // stub's own placeholder value (an empty string, or `0` for
    // `__LINE__`) exists purely so the declaration type-checks, and was
    // never meant to be read literally. Registering `__DIR__` as
    // `StringLiteral("")` broke every real call passing it onward (real
    // corpus regression caught before shipping: `ExcludeList::
    // addDirectory(__DIR__)`, `@param non-empty-string` on the receiving
    // end, flagged as `''` given). This project has no dedicated magic-
    // constant `Expr` kind of its own (a bare `__DIR__`/`__LINE__`/...
    // reference parses as an ordinary `ExprKind::Name`, already
    // `Type::Mixed` via `bare_name_literal_type`'s own default arm) -
    // excluding them here just protects that existing, already-correct
    // "don't know" behavior from being overwritten by this stub's own
    // placeholder.
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
    .contains(&const_name.as_str())
    {
        return None;
    }
    // `jetbrains/phpstorm-stubs/Core/Core_d.php`'s own `define('PHP_INT_MIN',
    // -9223372036854775808);` - real PHPStan never evaluates this source
    // text at all (it reads `PHP_INT_MIN`/`PHP_INT_MAX` via reflection
    // against the real, already-running PHP interpreter it needs anyway),
    // but this project has no such interpreter and would otherwise take
    // this literal at face value - and `9223372036854775808` (the
    // positive magnitude *before* the unary minus applies) is exactly
    // one past `i64::MAX`, so the lexer correctly represents it as a
    // `Literal::Float` the same way real PHP's own literal-overflow rule
    // would for this exact source text (confirmed: `var_dump(-9223372036854775808)`
    // truly does print a float, not `PHP_INT_MIN`'s real int value - PHP's
    // own C-level constant is never round-tripped through this decimal
    // text at runtime). `infer_const_type`'s `Neg` arm then correctly
    // types the whole expression `Type::Float`, silently breaking every
    // real `random_int(PHP_INT_MIN, PHP_INT_MAX)`-shaped call (a common,
    // real corpus idiom) the moment this table stopped being uniformly
    // `Mixed`. `PHP_INT_MAX` itself needs no equivalent exclusion -
    // `9223372036854775807` fits exactly, no `Neg`/overflow involved.
    if const_name == "PHP_INT_MIN" {
        return None;
    }
    Some((const_name.clone(), infer_const_type(&args[1].value)))
}

/// Records one `const`/`define()` declaration site for `name`, reconciling
/// it against anything already recorded for the same name rather than
/// letting whichever site happened to index first win unconditionally.
/// Real corpus shape this closes (`squizlabs/php_codesniffer`):
/// `Runner.php` does `define('PHP_CODESNIFFER_VERBOSITY',
/// $this->config->verbosity)` (a genuinely dynamic, config-driven value -
/// `infer_const_type` returns `Mixed`, this project's own "don't know"
/// signal) while a separate `tests/bootstrap.php` does `define(
/// 'PHP_CODESNIFFER_VERBOSITY', 0)` for test isolation - an entirely
/// unrelated, narrower value that has nothing to do with the constant's
/// real production meaning. A plain first-wins insert let file-indexing
/// order alone decide which of the two `Registry` believed - if the
/// literal `0` won, every real `PHP_CODESNIFFER_VERBOSITY > 1` comparison
/// elsewhere in the corpus collapsed to a compile-time-false constant
/// once this feature existed, marking an otherwise-live `if` branch
/// unreachable and corrupting that branch's own local-variable merge for
/// everything after it (found via a real corpus regression: `Ruleset::
/// expandRulesetReference`'s own `$ref` reassignment losing its narrowed
/// type this way, several call sites downstream). Any two declarations
/// that don't perfectly agree - including a *first* declaration that's
/// itself unknowable (`Mixed`) - poison the entry to `Mixed` instead,
/// the same "don't guess wrong" leniency every other real ambiguity in
/// this registry already resolves in favor of no information over wrong
/// information. Once poisoned, a later, more literal-looking declaration
/// can never un-poison it back to a specific type - the true value is
/// still just as undetermined regardless of which site happens to run at
/// this constant's real, load-order-dependent definition time.
fn register_global_constant(map: &mut FxHashMap<String, Type>, name: String, ty: Type) {
    match map.get(&name) {
        None => {
            map.insert(name, ty);
        }
        Some(existing) if *existing == ty && ty != Type::Mixed => {}
        Some(_) => {
            map.insert(name, Type::Mixed);
        }
    }
}

fn conditional_declaration_may_replace_stub(new_sig: &FunctionSig, existing: &FunctionSig) -> bool {
    if new_sig.params.len() != existing.params.len() {
        return false;
    }
    if existing.return_type != Type::Mixed && new_sig.return_type == Type::Mixed {
        return false;
    }
    existing
        .params
        .iter()
        .zip(&new_sig.params)
        .all(|(old, new)| !(old.ty != Type::Mixed && new.ty == Type::Mixed))
}

/// The identical question as `conditional_declaration_may_replace_stub`
/// just above, but for two *ordinary* conditional declarations of the
/// same global function name - neither one an already-known-good stub
/// worth being this conservative about protecting. Reuses that same
/// exact-arity-match comparison when arities agree (the "don't downgrade
/// an already-known type" reasoning applies identically either way), but
/// additionally lets a real, non-zero-parameter declaration replace an
/// `existing` one that's *entirely* parameter-less - a real corpus find:
/// `scoper-autoload.php`-style `func_get_args()`-forwarding shims
/// (`function trigger_deprecation() { return \Prefix\trigger_
/// deprecation(...func_get_args()); }`) are recognizable by exactly this
/// shape, and compete with the real, correctly-typed declaration
/// (`symfony/deprecation-contracts`' own 3-required-plus-variadic
/// `trigger_deprecation()`) for the same name - the strict arity-match
/// rule above would refuse the swap in *either* direction, permanently
/// freezing in whichever one happened to be indexed first. The reverse
/// (a real declaration already won the slot, a later 0-param shim trying
/// to displace it) stays correctly refused: a 0-param `new_sig` against
/// a non-zero-param `existing` still fails the ordinary arity-match
/// check below, and `existing.params.is_empty()` is false, so neither
/// branch here fires for it.
fn better_conditional_function_declaration(new_sig: &FunctionSig, existing: &FunctionSig) -> bool {
    if existing.params.is_empty() && !new_sig.params.is_empty() {
        return true;
    }
    conditional_declaration_may_replace_stub(new_sig, existing)
}

#[cfg(test)]
mod tests;
