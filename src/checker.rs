use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::pest_config;
use crate::registry::{NameContext, Registry};
use crate::token::Span;
use crate::types::Type;

/// A checked scope's variable-name-to-inferred-type bindings - repeated
/// across nearly every checker method's own parameter/return type before
/// this alias existed.
///
/// Phase 20 Finding B: backed by `imbl::HashMap`, a persistent,
/// structurally-shared map (a HAMT), not `std::collections::HashMap` -
/// cloned at ~22 call sites across `checker/stmt.rs`/`checker/expr.rs`
/// (once per branch of every `if`/`while`/ternary/`match`/closure, in
/// every function, in every file), profiling identified this as the
/// single biggest allocation source in the whole checker: a plain
/// `HashMap` clone rehashes every key and deep-clones every `Type`
/// value's own nested `String`s/`Vec`s. `imbl::HashMap::clone` is O(1) -
/// "really only a matter of copying a pointer and increasing a reference
/// counter" - a value only gets deep-copied lazily, on the *next*
/// mutation of whichever clone diverges from the other. `insert`/
/// `remove`/`entry`/`get`/`contains_key`/`iter`/`Default`/`IntoIterator`/
/// `FromIterator`/`PartialEq` all mirror `std::collections::HashMap`'s
/// own signatures closely enough that every existing call site needed
/// little to no change - confirmed directly against `imbl`/`im`'s own
/// docs before making this change, not assumed. `rustc_hash::
/// FxBuildHasher` (Finding C) as the hasher, since `imbl::HashMap` is
/// generic over it the same way `std::collections::HashMap` is.
type LocalsMap = imbl::GenericHashMap<
    String,
    Type,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// Phase 17 follow-up: `imbl::HashSet` mirrors `imbl::HashMap`'s own O(1)-
// clone design (Phase 20 Finding B's own doc comment above) - not load-
// bearing here the way it is for the outer `provenance` map itself (a
// per-variable span set is small and only ever mutated by inserting one
// more `Span`, never iterated at scale), but consistent with the rest of
// this module's own "persistent map/set throughout, not a plain
// `std::collections` one partway through" convention.
type ProvenanceSet =
    imbl::GenericHashSet<Span, rustc_hash::FxBuildHasher, imbl::shared_ptr::DefaultSharedPtr>;
type ProvenanceMap = imbl::GenericHashMap<
    String,
    ProvenanceSet,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// Phase 71 Item 4: lowercased, bare `function_exists('foo')`-guarded
// function names currently proven guarded on this exact control-flow
// path - `Locals`-scoped (not a `Checker` field) for the same reason
// every other flow-sensitive fact here is: it needs to merge/narrow
// per-branch exactly like `types`/`provenance` do, via `apply_narrowing`/
// `merge_scopes`, not persist for a whole function body unconditionally.
type GuardedFunctionSet =
    imbl::GenericHashSet<String, rustc_hash::FxBuildHasher, imbl::shared_ptr::DefaultSharedPtr>;

/// A checked scope's variable-name-to-inferred-type bindings, plus (Phase
/// 17 follow-up) an opt-in, parallel declaration-span side-channel the
/// LSP's own local-variable-navigation feature reads through `Checker::
/// probe_span`/`probe_var_provenance` - real reaching-definitions
/// precision (which real assignment(s) could have produced the value a
/// given read sees), computed by riding along the checker's own already-
/// correct, corpus-verified flow-sensitive traversal (branch merging via
/// `merge_scopes`, loop fixed-point convergence via `converge_loop_body`,
/// termination-pruning via `always_terminates`) rather than a second,
/// independent approximation. `types`/`provenance` are two separate
/// fields, not one field carrying `(Type, ProvenanceSet)` pairs, on
/// purpose: `Type` is this checker's own inference domain, read and
/// pattern-matched at hundreds of call sites throughout this module - a
/// wrapper tuple there would mean touching every one of them just to
/// unwrap it back out, for a concern (LSP navigation) that has nothing to
/// do with type inference itself. Kept in exact lockstep with `types`
/// instead via `Locals`'s own small, explicit API (`insert` leaves
/// `provenance` for that key untouched - correct for type-narrowing,
/// which refines an existing binding's type without creating a new one;
/// `declare` is the one exception, used only at a genuine syntactic
/// binding site - an assignment, a parameter, a `foreach`/`catch`
/// binding, ...; `insert_with_provenance` copies an already-known
/// provenance set across when a value moves between two `Locals` values
/// unchanged, e.g. `merge_scopes`'s own union).
///
/// Phase 20 Finding B: backed by `imbl::HashMap`, a persistent,
/// structurally-shared map (a HAMT), not `std::collections::HashMap` -
/// cloned at ~22 call sites across `checker/stmt.rs`/`checker/expr.rs`
/// (once per branch of every `if`/`while`/ternary/`match`/closure, in
/// every function, in every file), profiling identified this as the
/// single biggest allocation source in the whole checker: a plain
/// `HashMap` clone rehashes every key and deep-clones every `Type`
/// value's own nested `String`s/`Vec`s. `imbl::HashMap::clone` is O(1) -
/// "really only a matter of copying a pointer and increasing a reference
/// counter" - a value only gets deep-copied lazily, on the *next*
/// mutation of whichever clone diverges from the other. `insert`/
/// `remove`/`entry`/`get`/`contains_key`/`iter`/`Default`/`IntoIterator`/
/// `FromIterator`/`PartialEq` all mirror `std::collections::HashMap`'s
/// own signatures closely enough that every existing call site needed
/// little to no change - confirmed directly against `imbl`/`im`'s own
/// docs before making this change, not assumed. `rustc_hash::
/// FxBuildHasher` (Finding C) as the hasher, since `imbl::HashMap` is
/// generic over it the same way `std::collections::HashMap` is.
///
/// This wrapper struct preserves that exact clone/allocation profile:
/// `provenance` is itself an `imbl::HashMap` (same O(1)-clone property),
/// so wrapping `types` alongside it doesn't reintroduce the deep-clone
/// cost Finding B eliminated - cloning a `Locals` is still, structurally,
/// two pointer bumps.
#[derive(Clone, Default)]
pub(crate) struct Locals {
    types: LocalsMap,
    provenance: ProvenanceMap,
    pub(crate) guarded_functions: GuardedFunctionSet,
    // Whether `declare`/`insert_with_provenance` actually do anything -
    // `false` (via `Default`) for every ordinary `check-project` run,
    // `true` only for the one throwaway, per-request `Checker` `lsp.rs`'s
    // own variable-navigation probe builds (mirroring `Checker::probe_
    // span`'s own "only ever `Some` during that one request" contract).
    // Carried explicitly on `Locals` itself (not read from `Checker` at
    // each call site) because `Locals` values are constructed in more
    // places than `Checker` methods alone reach - most importantly, a
    // closure/arrow-fn literal's own fresh scope (`Locals::default()` at
    // several `checker/calls.rs`/`checker/expr.rs` call sites) needs the
    // *enclosing* scope's own `track` value threaded through explicitly
    // (`Locals::fresh_scope`, below) rather than silently defaulting back
    // to `false` and losing provenance the instant a probed variable read
    // sits inside a closure body.
    track: bool,
}

impl Locals {
    pub(crate) fn get(&self, key: &str) -> Option<&Type> {
        self.types.get(key)
    }

    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.types.contains_key(key)
    }

    /// Plain, provenance-preserving update - leaves whatever `provenance`
    /// already has for `key` completely untouched. Correct for the large
    /// majority of this module's own `locals.insert(...)` call sites:
    /// type-narrowing (an `instanceof`/null/array-shape/... check refines
    /// an *existing* binding's type without it being a new declaration)
    /// and a call's own by-reference type-widening effect (`sort($arr)`,
    /// `preg_match($p, $s, $matches)`, ...) both change what's *known*
    /// about a variable without changing *where it came from* - `$arr`'s
    /// own real declaration is still wherever it was first assigned, not
    /// this call site.
    pub(crate) fn insert(&mut self, key: String, ty: Type) -> Option<Type> {
        self.types.insert(key, ty)
    }

    /// The one real "this is a new declaration" primitive - used only at
    /// a genuine syntactic binding site (an assignment's left-hand side,
    /// a parameter, a `foreach`/`catch` binding, `static $x`, a closure's
    /// own `use (...)` capture, `$x++`/`$x--`). Replaces `key`'s own
    /// provenance with a fresh singleton `{span}`, discarding whatever
    /// was tracked before - a real rebind, not a refinement, so nothing
    /// from before it remains a live candidate. A no-op on `provenance`
    /// itself when `track` is off, at the cost of one `bool` check.
    pub(crate) fn declare(&mut self, key: String, ty: Type, span: Span) -> Option<Type> {
        if self.track {
            let mut set = ProvenanceSet::default();
            set.insert(span);
            self.provenance.insert(key.clone(), set);
        }
        self.types.insert(key, ty)
    }

    /// The provenance-*copying* primitive - used where a value moves
    /// between two different `Locals` values essentially unchanged (a
    /// branch merge, a closure's `use (...)` capture copied in from the
    /// enclosing scope, a `&&`/`||` short-circuit's own new-definition
    /// propagation): `provenance` carries over whatever was already
    /// tracked for it, rather than either fabricating a fresh span (not a
    /// new declaration) or silently discarding it (it has a real,
    /// already-known origin).
    pub(crate) fn insert_with_provenance(
        &mut self,
        key: String,
        ty: Type,
        provenance: ProvenanceSet,
    ) -> Option<Type> {
        if self.track && !provenance.is_empty() {
            self.provenance.insert(key.clone(), provenance);
        }
        self.types.insert(key, ty)
    }

    pub(crate) fn get_provenance(&self, key: &str) -> Option<&ProvenanceSet> {
        self.provenance.get(key)
    }

    pub(crate) fn remove(&mut self, key: &str) -> Option<Type> {
        self.provenance.remove(key);
        self.types.remove(key)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &Type)> {
        self.types.iter()
    }

    /// A fresh, empty scope that still carries `self`'s own `track` flag
    /// forward - see `track`'s own doc comment for why a closure/arrow-fn
    /// literal's own scope needs this instead of a plain `Locals::
    /// default()`.
    pub(crate) fn fresh_scope(&self) -> Locals {
        Locals {
            types: LocalsMap::default(),
            provenance: ProvenanceMap::default(),
            guarded_functions: GuardedFunctionSet::default(),
            track: self.track,
        }
    }

    /// Turns declaration-span tracking on for this scope and everything
    /// cloned from it - called exactly once, at `check_function`'s own
    /// entry point, gated on `Checker::probe_span` being active (see
    /// `track`'s own doc comment).
    pub(crate) fn set_track(&mut self, track: bool) {
        self.track = track;
    }
}

impl PartialEq for Locals {
    /// Deliberately compares `types` only, never `provenance` - `Checker::
    /// converge_loop_body`'s own fixed-point convergence check (`next_
    /// entry == pass_entry`) must keep converging purely on type
    /// stability, exactly as it already did before this field existed.
    /// `provenance` naturally reaches its own steady state alongside
    /// `types` in the overwhelming majority of real code either way; on
    /// the rare case it doesn't, whatever provenance resulted from the
    /// pass that actually triggered convergence is used - a reasonable,
    /// bounded approximation, never a hang (still capped at `MAX_PASSES`
    /// regardless), and never a behavior change for `track: false`
    /// (`provenance` stays empty on both sides throughout, so this
    /// distinction is unobservable there).
    fn eq(&self, other: &Self) -> bool {
        self.types == other.types
    }
}

/// A generic `@template` name -> concrete `Type` substitution map (class-
/// hierarchy template binding, function-call argument-to-template
/// binding) - built once per call/hierarchy-walk and consumed
/// immediately by `types::substitute_template`/`bind_template_param`,
/// never cloned per-branch the way `Locals` is. Used to reuse `Locals`'s
/// own type alias opportunistically (both were `HashMap<String, Type>`
/// underneath) before Phase 20 Finding B gave `Locals` its own,
/// persistent-map-backed identity - kept as a plain
/// `std::collections::HashMap` here since this use case has none of
/// `Locals`'s own "cloned once per branch" cost profile that motivated
/// the swap.
pub(crate) type TemplateBindings = std::collections::HashMap<String, Type>;

/// Loosely mirrors real PHPStan's own rule-gating levels (0 lowest/laxest,
/// verified against a fresh `phpstan/phpstan-src` clone's `conf/config.
/// levelN.neon` files rather than recalled from memory): 0 for the
/// "definitely broken" undefined-symbol/arity family (matches real
/// PHPStan's own level-0 bucket - "unknown classes, unknown functions,
/// unknown methods..., wrong number of arguments..., always undefined
/// variables" - though this project doesn't implement PHPStan's own
/// $this-only-vs-any-expression distinction for method/property checks,
/// a real, documented simplification, not an oversight), 1 for Phase 59's
/// unused-constructor-parameter/unused-closure-use checks (real PHPStan's
/// own level exactly - `UnusedConstructorParametersRule`/
/// `UnusedClosureUsesRule`'s own `#[RegisteredRule(level: 1)]`), 2 for
/// undefined-property/array-shape-key (PHPDoc-derived structural checks,
/// real PHPStan's `checkThisOnly: false`/`checkPhpDocMissingReturn`
/// territory), 3 for return-type/property-type mismatches (real PHPStan's
/// own `checkPhpDocMethodSignatures` level exactly), 4 for Phase 59's
/// unused-private-const/method/property checks (again real PHPStan's own
/// level exactly), 5 for argument-type mismatches (real PHPStan's own
/// `checkFunctionArgumentTypes` level exactly), and 6 for this project's
/// own generic bound/variance checks
/// (no direct real-PHPStan equivalent - generics are baked into its core
/// type engine, not level-gated - placed here as this project's own
/// "advanced, opt-in" tier, alongside real PHPStan's missing-typehints
/// level).
///
/// Phase 47: four fields grew to eight, mirroring what real PHPStan's own
/// `RuleError` implementations (`Message`/`RuleErrorBuilder`'s product) can
/// carry beyond a bare message - `identifier` is what `@phpstan-ignore
/// <identifier>` (Phase 44, not yet implemented) will need to match
/// against, CI output formats (Phase 16) want it for machine consumption,
/// `tip`/`non_ignorable`/`metadata`/`fix` are the same shape real
/// PHPStan's own JSON error format exposes. Every existing 4-field call
/// site still compiles unchanged in spirit - `Checker::report` keeps its
/// old positional shape, just with one more required argument
/// (`identifier`) - while a call site that also wants a tip/fix/metadata
/// goes through `DiagnosticBuilder` (`Checker::report_builder`) instead.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub location: String,
    pub message: String,
    pub span: Span,
    pub level: u8,
    /// A dot-separated, PHPStan-style machine identifier for this
    /// diagnostic's *kind* (e.g. `"argument.type"`, `"class.notFound"`,
    /// `"return.type"`) - stable across occurrences of the same underlying
    /// problem, unlike `message`, which embeds the specific types/names
    /// involved. Never empty in practice (every real call site names one),
    /// but not a real PHPStan identifier list lookalike guaranteed to
    /// match phpstan/phpstan's own strings one-for-one - chosen for
    /// internal consistency within this project, reusing real PHPStan's
    /// own spelling where this project's own audit was confident of it.
    pub identifier: String,
    /// An optional one-line, human-facing hint appended below the main
    /// message - real PHPStan's own "💡 Tip: ..." line (`RuleErrorBuilder
    /// ::tip()`). Rare: most diagnostics are self-explanatory, so most
    /// call sites leave this `None`.
    pub tip: Option<String>,
    /// Mirrors real PHPStan's own `RuleErrorBuilder::nonIgnorable()`: when
    /// `true`, this diagnostic must never be suppressible via a future
    /// `@phpstan-ignore <identifier>` (Phase 44) - the same reason real
    /// PHPStan reserves it for a handful of its own rules (e.g. flagging a
    /// malformed `@phpstan-ignore` comment itself, which obviously can't
    /// be ignored away by another one). Defaults to `false`; nothing in
    /// this project sets it yet, since Phase 44 doesn't exist to consume
    /// it - the field exists so that phase doesn't also need a struct
    /// change.
    pub non_ignorable: bool,
    /// Free-form structured data a rule wants attached to its own
    /// diagnostic without inventing a new `Diagnostic` field for it -
    /// mirrors real PHPStan's own `RuleErrorBuilder::metadata()` (consumed
    /// there by e.g. its `bleedingEdge` collectors). Empty for every
    /// current call site; a `HashMap` (not `Vec<(String, Value)>`) since a
    /// consumer looks up one known key, never iterates all of them.
    pub metadata: HashMap<String, serde_json::Value>,
    /// A suggested textual auto-fix, when this diagnostic's rule can
    /// propose one - the data-model half of real PHPStan's own
    /// `RuleErrorBuilder::fixNode()` (which hands back a *replacement AST
    /// node*; this project has no code-generation-from-AST facility to
    /// mirror that with, so a fix here is a literal source-text
    /// replacement over a `Span` instead). No current call site
    /// constructs one - a real auto-fix command (reading this field and
    /// rewriting files) is future work, same as `non_ignorable` above.
    pub fix: Option<DiagnosticFix>,
}

/// A single suggested textual replacement: replace whatever source text
/// `span` covers with `replacement`. `description` is a short human label
/// for the fix itself (real PHPStan's own fixer CLI output shows one per
/// applied fix, e.g. "Add missing return statement").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticFix {
    pub span: Span,
    pub replacement: String,
    pub description: String,
}

/// Fluent assembly for a [`Diagnostic`] - the Rust-idiomatic equivalent of
/// real PHPStan's own `RuleErrorBuilder`
/// (`RuleErrorBuilder::message(...)->identifier(...)->tip(...)->build()`).
/// A plain diagnostic with no tip/metadata/fix still goes through this
/// (via `Checker::report`, which is a thin wrapper around it) - one real
/// construction path for every `Diagnostic`, not two independently
/// maintained ones.
///
/// Consumed by value (`build(self)`, each setter takes/returns `Self`),
/// matching real PHPStan's own chain rather than `&mut self` - a
/// `DiagnosticBuilder` is always a short-lived, single-use expression built
/// and immediately consumed at one `self.report_builder(...)` call site,
/// never stored or branched on mid-construction.
pub struct DiagnosticBuilder {
    location: String,
    message: String,
    span: Span,
    level: u8,
    identifier: String,
    tip: Option<String>,
    non_ignorable: bool,
    metadata: HashMap<String, serde_json::Value>,
    fix: Option<DiagnosticFix>,
}

impl DiagnosticBuilder {
    /// `identifier` is required up front, not a later `.identifier(...)`
    /// call like real PHPStan's own builder allows omitting - every call
    /// site in this project was written (or rewritten, Phase 47) knowing
    /// its own identifier, so there's no "forgot to set it" state worth
    /// representing.
    pub fn new(
        location: impl Into<String>,
        message: impl Into<String>,
        span: Span,
        level: u8,
        identifier: impl Into<String>,
    ) -> Self {
        DiagnosticBuilder {
            location: location.into(),
            message: message.into(),
            span,
            level,
            identifier: identifier.into(),
            tip: None,
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        }
    }

    pub fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    pub fn non_ignorable(mut self) -> Self {
        self.non_ignorable = true;
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn fix(
        mut self,
        span: Span,
        replacement: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.fix = Some(DiagnosticFix {
            span,
            replacement: replacement.into(),
            description: description.into(),
        });
        self
    }

    pub fn build(self) -> Diagnostic {
        Diagnostic {
            location: self.location,
            message: self.message,
            span: self.span,
            level: self.level,
            identifier: self.identifier,
            tip: self.tip,
            non_ignorable: self.non_ignorable,
            metadata: self.metadata,
            fix: self.fix,
        }
    }
}

/// Walks function/method bodies, inferring a best-effort type for each
/// expression and flagging argument/return type mismatches against the
/// registry. Deliberately conservative: `Type::Mixed` is overloaded here to
/// mean both PHP's real `mixed` type AND "we don't know" for anything this
/// prototype can't infer (property access, closures, dynamic calls, ...) -
/// so every check below treats an inferred `Mixed` as inconclusive and
/// skips reporting, rather than risk false positives.
///
/// `if`/`while` fork `locals` into per-branch copies (narrowed via
/// `apply_narrowing` on the condition) and merge them back at the join
/// point (`merge_scopes`), accounting for a branch that unconditionally
/// terminates (`always_terminates`) so a guard clause's narrowing survives
/// past the `if`. Everything else (`for`/`foreach`/`switch`/`try`) still
/// shares one mutable scope with no forking or narrowing - a real but
/// bounded gap, not a soundness issue, since no narrowing ever happens
/// there rather than an incorrect one.
///
/// Resolved limitation (Phase 9.5): undefined-method detection was
/// initially built and then reverted after real-scale testing found it
/// trusted *which* `ClassInfo` a bare name resolved to, and this
/// project's registry didn't model namespaces at all - 1595 distinct
/// class names were found to collide across one real project plus its
/// `vendor/` (e.g. 77 unrelated declarations of `InvalidArgumentException`
/// alone), silently overwriting a class's real method table with an
/// unrelated one's. Fixed by making `Registry`/`Checker` resolve every
/// class/function reference to a fully-qualified name first
/// (`registry::NameContext`, tracking each file's `namespace`/`use`
/// statements) before any registry key is read or written - two classes
/// with the same short name in different namespaces now get distinct
/// keys, the same way real PHP tells them apart. Undefined-method
/// detection on `ExprKind::MethodCall` is safe again as of this fix.
///
/// Phase 9 completed since: `ExprKind::PropertyAccess` now flags undefined
/// properties on read (`__get`-exempt; the existing write-side check
/// stays lenient, since a write can legitimately create a real, if
/// deprecated, dynamic property - a read never does), and
/// `ExprKind::StaticAccess`/`StaticCall` now check constants, static
/// properties, and static methods (`__callStatic`-exempt) the same way.
pub struct Checker<'a> {
    pub(crate) registry: &'a Registry,
    pub(crate) diagnostics: Vec<Diagnostic>,
    // Namespace + `use`-import map, built once per file. Needed because
    // almost all real code refers to an imported class/function by its
    // short name, not a fully-qualified path - without resolving it here
    // first, undefined-class/function detection would flag the
    // overwhelming majority of legitimately-imported symbols. Also must
    // resolve to exactly the same fully-qualified names
    // `Registry::index` keys its entries under (Phase 9.5's fix for the
    // namespace-collision bug documented above), or the two would
    // silently disagree with each other.
    pub(crate) name_ctx: NameContext,
    // The class whose method body is currently being checked, if any -
    // needed to resolve self/parent/static to a concrete class name.
    pub(crate) current_class: Option<String>,
    // The name of the method whose body is currently being checked, if
    // any - needed to tell whether a `readonly` property write is
    // happening inside its own declaring class's constructor (Finding
    // 47's pragmatic first cut: any write outside that exact spot is
    // flagged).
    pub(crate) current_method: Option<String>,
    // Phase 39.2: the trait whose method bodies are currently being
    // checked, if any - `check_trait_methods` re-checks a trait's own
    // methods once per using host, with `current_class` set to the
    // *host* (see that function's own doc comment), so a trait's own
    // `@deprecated` tag is otherwise unreachable through `current_class`
    // in the common (used) case. Set/restored around `check_trait_
    // methods`'s own host loop, independent of `current_class`.
    pub(crate) current_trait: Option<String>,
    // Phase 39.2: whether the function/method/trait/class body currently
    // being checked is itself inside a deprecated scope (its own
    // enclosing function/method, class, or trait carries `@deprecated`) -
    // real PHPStan's own `DeprecatedScopeHelper` suppresses every
    // deprecated-usage diagnostic entirely from within such a scope
    // (using one deprecated thing from another is not itself worth
    // warning about). Computed once per function-like body in
    // `check_function_with_ancestor_hint`, the single choke point where
    // `current_class`/`current_method` are also set.
    pub(crate) current_scope_deprecated: bool,
    // Set for the duration of one function/method (or the top-level script)
    // body when it calls extract()/eval() - both can define variables this
    // checker has no way to see, so undefined-variable detection would be
    // pure noise for the rest of that body. Rather than track precisely
    // which diagnostics came from that check, the whole body's diagnostics
    // are discarded if this ends up true (see check_function).
    pub(crate) saw_dynamic_var_construct: bool,
    // Set for the duration of one function/method/closure body that
    // contains a yield/yield from anywhere (Finding 3) - a real
    // generator, whose implicit `return null` at the end (or an explicit
    // bare `return;`) never mismatches its declared Generator/Iterator/
    // Traversable return type, unlike an ordinary function. Consulted by
    // both check_function's own falls-off-the-end check and check_stmt's
    // StmtKind::Return(None) arm, which has no direct access to the
    // enclosing function's body to answer this itself.
    pub(crate) current_function_is_generator: bool,
    // Phase 70.A: the enclosing function/method's own *docblock*-preferring
    // declared return type (not `native_return` - only the docblock
    // spelling can carry `Generator<TKey, TValue, TSend, TReturn>`'s own
    // generic args), `resolve_pseudo`'d against the current class already -
    // set/restored alongside `current_function_is_generator` at the same
    // choke point. Consulted by `ExprKind::Yield`/`YieldFrom` (`checker/
    // expr.rs`) to resolve `yield`'s own TSend and `yield from`'s own
    // TReturn via `generator_template_param`, since neither expression has
    // any other way to see the enclosing declaration from mid-body.
    pub(crate) current_declared_return_type: Option<Type>,
    // Phase 42 Sub-item A: every real throw point recorded so far inside
    // the enclosing function/method/closure body - an explicit `throw`
    // expression's own thrown-value type (`checker/stmt/try_catch.rs`'s
    // `check_throw`, `checker/expr.rs`'s own `ExprKind::Throw` arm), or a
    // call's own resolved `@throws` type (`FunctionSig.throws`, or a
    // matching `checker/throws/dynamic.rs` per-callee override tried
    // first) - recorded at each of `ExprKind::Call`/`New`/`MethodCall`/
    // `StaticCall`'s own already-resolved-signature choke points. `Type::
    // Void` (a callee explicitly declared to throw nothing) contributes
    // nothing at all, matching real PHPStan's own identical treatment.
    // Consumed by `checker/throws/too_wide.rs`'s `TooWideFunctionThrowTypeRule`/
    // `TooWideMethodThrowTypeRule` port - see that module's own doc
    // comment for the full "is a declared `@throws` member actually ever
    // thrown" comparison this feeds.
    pub(crate) current_function_throw_points: Vec<Type>,
    // Phase 42 Sub-item A: set `true` the moment any call inside the
    // enclosing function/method/closure body resolves to *no signature at
    // all* (a dynamic call, or a genuinely unresolvable one) - real PHP
    // allows an arbitrarily-implemented callee to throw literally
    // anything, so such a call's real throw-ness genuinely can't be
    // bounded from here. Deliberately does NOT also fire for a resolved
    // signature that simply has no `@throws` tag of its own - see
    // `Checker::record_call_throw_point`'s own doc comment
    // (`checker/throws.rs`) for why that distinction matters. Consumed by
    // `checker/throws/too_wide.rs`'s `TooWideFunctionThrowTypeRule`/
    // `TooWideMethodThrowTypeRule` port, which suppresses its whole
    // comparison for a declaration this is `true` for, favoring a missed
    // diagnostic over a wrongly-confident one.
    pub(crate) current_function_has_unannotated_call: bool,
    // Phase 42 bonus item: the explicit-`throw`-only subset of
    // `current_function_throw_points`, each paired with its own `throw`
    // expression's span - populated at the same two choke points
    // (`check_throw`/`ExprKind::Throw`), but never by `record_call_
    // throw_point` (a call's own `@throws`-derived contribution belongs
    // to *that callee*, not something *this* declaration's own code
    // wrote). Consumed by `checker/throws/missing.rs`'s "missing checked
    // exception" family, opt-in via `Registry::check_missing_checked_
    // exceptions` - see that module's own doc comment for why only the
    // explicit subset is checked.
    pub(crate) current_function_explicit_throw_points: Vec<(Type, Span)>,
    // Set for the duration of one function/method/closure body to every
    // label name `goto`-targeted anywhere in it (`collect_goto_targets`'s
    // own doc comment has the full real corpus false positive this
    // exists for) - consulted by `check_block`'s own `deadCode.
    // unreachable` check, which has no direct access to the enclosing
    // function's body to answer "could a `goto` reach this labeled
    // statement" itself, the same shape `current_function_is_generator`
    // already has for its own equally function-scoped question.
    pub(crate) current_function_goto_targets: std::collections::HashSet<String>,
    // Phase 71: set to `true` immediately before `check_expr` is called
    // for a `StmtKind::Expression`'s own top-level expression (a bare
    // `foo();`, its return value going nowhere by construction), cleared
    // the instant `check_expr` reads it (`std::mem::take`, at the very
    // top of its own dispatch) - so it only ever reads `true` for that
    // exact top-level call, never for anything nested inside it
    // (`foo(bar());`'s inner `bar()` call sees `false`, correctly, since
    // *its* return value genuinely is consumed as an argument).
    // `check_useless_return_value_call` is this flag's one reader: real
    // PHPStan's own `UselessFunctionReturnValueRule` needs exactly this
    // "is my own result being used at all" signal, the mirror image of
    // `check_no_discard_statement`'s "is my result NOT being used"
    // question - which already has its own dedicated call site (the
    // literal top-level statement) to answer that from directly, unlike
    // this one, which needs to answer it from every *other*, much more
    // varied call site instead.
    pub(crate) in_bare_statement_position: bool,
    // Phase 15 continuation: mirrors `in_bare_statement_position`'s own
    // exact `std::mem::take`-at-top-of-`check_expr` idiom - set to `true`
    // immediately before `check_expr` is called for the direct `cond` of
    // an `if`/`elseif`/`while`/`do-while`/ternary (never `for` - real
    // PHPStan has no dedicated for-condition rule at all), cleared the
    // instant `check_expr` reads it. Real PHPStan's own `Scope::
    // isInFirstLevelStatement` equivalent: `BooleanAndConstantConditionRule`/
    // `BooleanOrConstantConditionRule`'s own right-side/whole-expression-
    // result diagnostics are skipped exactly when the `&&`/`||` itself
    // *is* one of those direct conditions - already covered by the
    // if/while/ternary's own check there - but never for a `&&`/`||`
    // reached any other way (an ordinary sub-expression, a nested one
    // several levels deep, ...), which still gets the full three-way
    // check.
    pub(crate) in_condition_position: bool,
    // Phase 15 continuation: same `std::mem::take` idiom, set (alongside
    // `in_condition_position`) only when the direct condition about to be
    // checked is a genuine `elseif` with no trailing `else` and nothing
    // after it in the chain - real PHPStan's own "acts as an implicit
    // else" exemption (`reportAlwaysTrueInLastCondition: false`'s
    // default, confirmed in `vendor/phpstan-src/conf/config.neon`).
    // `check_if` already applies this directly to its own `elseif.
    // alwaysTrue`, but `BooleanNotConstantConditionRule` has the *same*
    // exemption for a standalone `!$expr` condition (`elseif (!false)
    // {}` with nothing after it) - `should_skip_constant_condition`
    // deliberately excludes `!` from `check_if`'s own top-level check
    // (deferring to this dedicated one), so without this flag that
    // deferral would silently lose the exemption `check_if` alone can't
    // apply to a shape it never evaluates itself.
    pub(crate) in_exempt_last_condition: bool,
    // Finding 1: the `$this` type inside a Pest `it`/`test`/`describe`/
    // `beforeEach`/`afterEach`/`beforeAll`/`afterAll` closure - set via
    // `set_pest_this_type` (by `Project::check_file`, from its own
    // `PestConfig::resolve_this_type`) before `check_program` runs, then
    // possibly overridden by a file-local `uses(...)` call
    // `check_program` itself finds (real Pest's own resolution order:
    // file-local wins over directory-scoped). Defaults to `Type::Mixed`
    // so every existing call site untouched by Finding 1 (the ~300+
    // direct `Checker::new` calls in this project's own tests, the `demo`
    // subcommand, ...) is completely unaffected - this field is only ever
    // consulted inside an `is_this_binding_call`-recognized closure
    // argument, which none of those exercise.
    pub(crate) pest_this_type: Type,
    // Phase 22 Finding 1: dynamic property names a Pest `beforeEach(...)`
    // closure assigns onto the shared `$this` (see `pest_config`'s own
    // `file_local_hook_properties`/`PestConfig::resolve_hook_properties`)
    // - set via `set_pest_hook_properties` (by `Project::check_file`, from
    // `PestConfig::resolve_hook_properties`) before `check_program` runs,
    // then unioned (not overridden - a property-name set has no "most
    // specific wins" precedence the way a single class type does) with
    // whatever file-local `beforeEach(...)` calls `check_program` itself
    // finds. Defaults empty, so every existing call site untouched by
    // this is completely unaffected.
    pub(crate) pest_hook_properties: HashSet<String>,
    // Phase 29: one frame per currently-open loop or switch, collecting
    // the `locals` snapshot at every `break` reached inside it - a stack,
    // since loops/switches nest. `StmtKind::Foreach`/`While`/`For`/
    // `DoWhile` each fold every collected state into what flows past the
    // loop once its body is done being checked (a real `break` inside an
    // `if` was otherwise silently discarded from post-loop state
    // entirely - `always_terminates` correctly treats it as "doesn't
    // fall through to the next statement in this block", but nothing
    // separately captured it for the *enclosing loop's* own exit state,
    // the same two-question split `Switch`'s own pre-existing
    // `exit_states` mechanism already draws for itself). `Switch` also
    // pushes/pops a frame here - not to consume it (its own `exit_states`
    // computation is entirely self-contained) but so a bare `break;`
    // written directly inside a case attaches to *that* frame (discarded
    // on pop) instead of leaking into whichever enclosing loop happens to
    // be open, matching real PHP's "innermost loop-or-switch" rule for an
    // unqualified `break;`.
    pub(crate) loop_break_states: Vec<Vec<Locals>>,
    // Phase 55: the `continue` counterpart of `loop_break_states` above -
    // one frame per currently-open loop, collecting the `locals` snapshot
    // at every `continue` reached inside it. Unlike `break`'s states
    // (folded once, after all of a loop's convergence passes finish, into
    // what flows *past* the loop), `continue`'s states are consumed
    // *between* passes, inside `converge_loop_body` itself: a `continue`
    // jumps back to the top of the loop, so whatever it reached is
    // exactly as much a real "entry to the next iteration" as the body's
    // own ordinary fall-through exit is. Pushed/popped once per pass
    // (not once per loop, unlike `loop_break_states`) - each pass's own
    // continue-reached states only ever inform *that* pass's contribution
    // to the next entry, never a later pass's. Without this, a variable
    // reassigned *only* on a path that ends in `continue` (a real, common
    // shape: `if ($x === null) { ...; $x = compute(); continue; }`) never
    // fed its new value into the next convergence pass at all - the
    // pass-to-pass entry computation only ever saw the body's ordinary
    // fall-through exit, which such a variable never reaches, so
    // convergence silently converged after a single pass and kept that
    // pass's own diagnostics, computed against the *stale* pre-loop
    // entry - reintroducing the exact false-positive class Phase 15's
    // `UnreachableStatementRule` was reverted over in the first place
    // (real corpus find: sebastian/diff's `Parser::parse`).
    pub(crate) loop_continue_states: Vec<Vec<Locals>>,
    // Phase 60: how many loop bodies (`While`/`DoWhile`/`For`/`Foreach`,
    // nesting included - a `Switch` doesn't count, it runs its body
    // exactly once) currently enclose whatever's being checked right
    // now. Originally motivated by this project's own single-pass loop
    // model (superseded by Phase 55's `converge_loop_body` - a variable's
    // tracked type at a given point can now reflect an earlier
    // iteration's own write, within the pass budget) - kept as its own
    // counter regardless, since `check_pointless_null_probe` and similar
    // "is this ever true across the whole function" checks still need a
    // simple "any loop at all, at any depth, still open right now" signal
    // distinct from `loop_break_states`'s own stack (which also counts an
    // enclosing `Switch` frame, not relevant to this one).
    pub(crate) loop_depth: u32,
    // Phase 60 corpus find: every variable name seen bound via
    // `static $x = ...;` anywhere in the file being checked - real
    // shape: Symfony's own `HttpClient::create()`'s `static $curlVersion
    // = null; $curlVersion ??= curl_version();`. A `static` local
    // genuinely can carry a value left over from an *earlier call* -
    // exactly the same "this single forward pass can't see a wider type
    // a different execution of the same code would" unsoundness
    // `loop_depth` already exists to guard against, just across
    // invocations instead of iterations (confirmed against real PHPStan
    // itself, which stays silent on this exact shape too). Accumulated
    // across the whole file rather than reset per function/closure - a
    // stale entry only costs a missed diagnostic on an unrelated later
    // local that happens to share the same name, never a wrong one.
    pub(crate) static_var_names: HashSet<String>,
    // Phase 59: every class named as the resolved `$scope` of a
    // `Closure::bind(...)`/`->bindTo(...)` call anywhere in the file being
    // checked (`unused::scan_program_for_rebind_targets`, built once per
    // `check_program`) - a real, idiomatic way to reach a class's private
    // members from a closure declared entirely outside that class's own
    // body (see `checker/expr.rs`'s own Finding 49 handling), invisible to
    // `check_unused_private_members`'s own per-class-body-only usage scan
    // otherwise. `rebind_escape_all` is the coarser fallback for a
    // bind/bindTo call whose scope isn't a literal `Foo::class` this
    // project can resolve without full type inference (an omitted scope
    // argument, or an arbitrary expression) - safer to skip every class's
    // private-member check for the whole file than risk a false "unused"
    // report on a member a rebound closure actually reaches.
    pub(crate) rebind_target_classes: HashSet<String>,
    pub(crate) rebind_escape_all: bool,
    // Phase 17 Stage B: the LSP's own one-off "what type is this instance
    // receiver" probe. `probe_span` is the exact `Span` of the receiver
    // `Expr` (`lsp::navigation` already knows it precisely - the same
    // `MethodCall`/`PropertyAccess` node its own Stage A walk already
    // identifies) the caller wants typed; `check_expr`'s own `MethodCall`/
    // `PropertyAccess` arms already compute the receiver's real `Type` as
    // part of ordinary checking (`recv_ty`) - this just also stashes it
    // in `probe_hit` when the receiver's own span matches. Matching by
    // exact span (not "nearest enclosing") sidesteps any inner-vs-outer
    // receiver ambiguity for a chained call (`$x->foo()->bar()`) entirely
    // - `lsp::navigation` already resolved *which* receiver it wants
    // before ever constructing this `Checker`, so there's nothing left
    // for this to disambiguate. `None` (every existing call site,
    // `check-project` included) is a complete no-op - `probe_span` never
    // equals a real span, so the extra check at each call site is the
    // only added cost, and it's checked only where `recv_ty` was already
    // being computed anyway.
    //
    // `Cell`/`RefCell` (not plain fields): Phase 17 follow-up's own
    // declaration-site hover probe (`Checker::probe_hit_at`) needs to
    // stash a hit from deep inside `bind_assignment_target`/
    // `bind_simple_target`/`apply_narrowing` - all three are called from
    // dozens of sites across the checker, several of them themselves only
    // ever holding `&self` (e.g. `infer_expr`/`infer_binary`, whose own
    // narrowing side effects are already expressed through `&mut Locals`,
    // not `&mut self`). Threading `&mut self` through that whole call
    // graph just for a one-off LSP probe would be a large, high-risk,
    // purely-mechanical signature change rippling through the most
    // heavily-called functions in the module. Interior mutability keeps
    // every one of those signatures untouched: a probe field is written
    // through a shared `&self` reference, exactly like any other "off by
    // default, harmless no-op" side channel - the same reasoning that
    // already justifies these fields being unconditionally cheap when no
    // probe is active.
    pub(crate) probe_span: Cell<Option<Span>>,
    pub(crate) probe_hit: RefCell<Option<Type>>,
    // Phase 17 follow-up: the LSP's own one-off "where could this local
    // variable read have come from" probe - the real, flow-sensitive
    // counterpart of `lsp::navigation`'s own hand-rolled arm-merging
    // heuristic it replaces. Checked in `check_expr`'s own `ExprKind::
    // Variable` arm the same way `probe_hit` already is in `MethodCall`/
    // `PropertyAccess`: when `probe_span` matches this read's own span,
    // `locals`'s own tracked provenance for that name (see `Locals::
    // declare`'s own doc comment) is stashed here. `None` (every existing
    // call site, `check-project` included) is a complete no-op - see
    // `Locals::track`'s own doc comment for why declaration tracking
    // itself is also gated off whenever no probe is active, not just this
    // one read.
    pub(crate) probe_var_provenance: RefCell<Option<HashSet<Span>>>,
}

mod assignment;
mod calls;
mod coercion;
mod control_flow;
mod date_interval;
mod deprecated_usage;
pub(crate) mod duplicate_functions;
mod duplicates;
mod expr;
mod function_decl;
mod generics;
mod imports;
mod internal_usage;
mod missing_typehint;
mod narrowing;
mod noop;
mod oop;
mod operators;
pub(crate) mod phpstan_api;
mod property_assignment;
mod resolve;
mod stmt;
mod throws;
mod type_union;
mod uninitialized;
mod uninitialized_dataflow;
mod unused;
mod visibility;

pub(crate) use assignment::{
    is_superglobal, literal_string_key, literal_string_value, superglobal_default_type,
};
pub(crate) use calls::{CheckArgsCall, is_language_construct, resolve_call_args};
pub(crate) use coercion::{to_absolute_number, to_bitwise_not};
pub(crate) use control_flow::{
    body_contains_yield, collect_exit_point_statements, collect_goto_targets,
    exit_point_description, expr_contains_yield, generator_missing_return_type,
    generator_template_param, is_switch_break_equivalent, is_truthy_literal, merge_scopes,
    stmt_contains_break, stmt_contains_escaping_exit, type_allows_generator_return,
};
pub(crate) use duplicates::class_kind_identifier;
pub(crate) use expr::{
    build_matched_shape, int_range_bounds, normalize_int_range, unwrap_grouping,
};
pub(crate) use internal_usage::class_type_word;
pub(crate) use missing_typehint::has_explicit_mixed_nested_somewhere;
pub(crate) use oop::{
    ATTRIBUTE_TARGET_CLASS, ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_FUNCTION,
    ATTRIBUTE_TARGET_METHOD, ATTRIBUTE_TARGET_PARAMETER, ATTRIBUTE_TARGET_PROPERTY,
};
pub(crate) use throws::body_contains_explicit_throw;
pub(crate) use type_union::union_types;
// Only ever called by `union_types` itself in real code (`checker/expr.rs`) -
// this crate-level re-export exists solely so this module's own test
// suite can exercise it directly, the same reason `preg_match_matches_shape`
// isn't re-exported either.
pub(crate) use narrowing::{
    array_value_type_for_key, is_real_variable_key, is_unsealed_shape,
    narrow_array_shape_by_key_presence, narrow_type_excluding, narrow_type_excluding_falsy,
    narrow_type_to, narrowing_key, purge_stale_narrowing_for,
};
#[cfg(test)]
pub(crate) use type_union::generalize_by_key_set;
pub(crate) use visibility::MemberVisibilityCheck;

impl<'a> Checker<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Checker {
            registry,
            diagnostics: Vec::new(),
            name_ctx: NameContext::default(),
            current_class: None,
            current_method: None,
            current_trait: None,
            current_scope_deprecated: false,
            saw_dynamic_var_construct: false,
            current_function_is_generator: false,
            current_declared_return_type: None,
            current_function_throw_points: Vec::new(),
            current_function_has_unannotated_call: false,
            current_function_explicit_throw_points: Vec::new(),
            current_function_goto_targets: std::collections::HashSet::new(),
            in_bare_statement_position: false,
            in_condition_position: false,
            in_exempt_last_condition: false,
            pest_this_type: Type::Mixed,
            pest_hook_properties: HashSet::new(),
            loop_break_states: Vec::new(),
            loop_continue_states: Vec::new(),
            loop_depth: 0,
            static_var_names: HashSet::new(),
            rebind_target_classes: HashSet::new(),
            rebind_escape_all: false,
            probe_span: Cell::new(None),
            probe_hit: RefCell::new(None),
            probe_var_provenance: RefCell::new(None),
        }
    }

    /// Sets the `$this` type Finding 1's Pest support seeds an
    /// `it`/`test`/`describe`/`beforeEach`/`afterEach`/`beforeAll`/
    /// `afterAll` closure argument with - a setter rather than a
    /// `Checker::new` parameter, so the ~300+ existing call sites that
    /// don't care about Pest at all don't need to change.
    pub fn set_pest_this_type(&mut self, ty: Type) {
        self.pest_this_type = ty;
    }

    /// Seeds the directory-scoped half of the dynamic-`$this`-property
    /// name set (`PestConfig::resolve_hook_properties`) - a setter for
    /// the same reason `set_pest_this_type` is one. `check_program`
    /// unions this with whatever file-local `beforeEach(...)` calls it
    /// finds itself, rather than one overriding the other.
    pub fn set_pest_hook_properties(&mut self, props: HashSet<String>) {
        self.pest_hook_properties = props;
    }

    /// Phase 17 follow-up: the declaration-site counterpart of the
    /// read-site probe hook already inline in `ExprKind::Variable` - stashes
    /// `ty` in `probe_hit` when `span` is exactly the LSP's own hover-type
    /// probe target (`probe_span`). A one-line call at every real
    /// local-variable binding site (every `Locals::declare` call site: the
    /// parameter-declare loops, `foreach`/`catch`/`static`/`global` binds,
    /// destructuring-leaf binds), so hovering a variable at its own
    /// declaration - not just a later read of it - shows an inferred type
    /// too. `None` (every existing call site, `check-project` included) is
    /// a complete no-op, the same contract every other probe hook in this
    /// module already has.
    pub(crate) fn probe_hit_at(&self, span: Span, ty: &Type) {
        if self.probe_span.get() == Some(span) {
            *self.probe_hit.borrow_mut() = Some(ty.clone());
        }
    }

    /// The plain-message path every call site that needs nothing beyond
    /// `identifier` goes through - a thin wrapper over `DiagnosticBuilder`
    /// (Phase 47) so there's still exactly one place a `Diagnostic` is
    /// actually assembled. `identifier` is `&str` here (not
    /// `impl Into<String>` the way `DiagnosticBuilder::new` takes it) so
    /// every call site can keep passing a `&'static str` literal
    /// (`"argument.type"`, ...) without an extra `.to_string()`.
    pub(crate) fn report(
        &mut self,
        location: String,
        message: String,
        span: Span,
        level: u8,
        identifier: &str,
    ) {
        self.diagnostics
            .push(DiagnosticBuilder::new(location, message, span, level, identifier).build());
    }

    /// The builder path - for a call site that also wants a `tip`, a
    /// `fix`, `metadata`, or `non_ignorable`, none of which `report`'s own
    /// positional signature has room for without every plain call site
    /// paying for arguments it never uses.
    pub(crate) fn report_builder(&mut self, builder: DiagnosticBuilder) {
        self.diagnostics.push(builder.build());
    }

    /// Phase 67/Phase 39 follow-up: an object used somewhere real PHP
    /// implicitly converts it to a string - an explicit `(string)` cast
    /// (`ExprKind::Cast`), `echo`, and `print` all do this identically at
    /// runtime, invoking the object's own `__toString()` - triggers the
    /// exact same `@internal`/`@deprecated` restricted-usage checks an
    /// explicit `->__toString()` call would (real PHPStan's own
    /// `RestrictedUsageOfDeprecatedStringCastRule` and the internal-tag
    /// equivalent, both previously left as "easy to add once needed, just
    /// not wired" in ROADMAP.md's own Phase 67 entry - confirmed real via
    /// `vendor/phpstan-src`'s own `PrintHandler`/`EchoRule`, which run the
    /// identical `ImplicitToStringCallHelper` this project's checks
    /// mirror). Same "first Object member" representative-class
    /// extraction `ExprKind::MethodCall`'s own `recv_class` uses for a
    /// Union/Intersection operand.
    pub(crate) fn check_implicit_tostring_usage(&mut self, ty: &Type, span: Span, label: &str) {
        let class = match ty {
            Type::Object(class, _) => Some(class.clone()),
            Type::EnumCase(fqn, _) => Some(fqn.clone()),
            Type::Union(members) | Type::Intersection(members) => {
                members.iter().find_map(|m| match m {
                    Type::Object(class, _) => Some(class.clone()),
                    Type::EnumCase(fqn, _) => Some(fqn.clone()),
                    _ => None,
                })
            }
            _ => None,
        };
        let Some(class) = class else { return };
        let Some((owner, sig)) = self.registry.lookup_method_with_owner(&class, "__toString")
        else {
            return;
        };
        self.check_internal_method_call(&owner, "__toString", sig, false, span, label);
        self.check_deprecated_method_call(&owner, "__toString", sig, false, span, label);
    }

    pub fn into_diagnostics(mut self) -> Vec<Diagnostic> {
        // Internal checks run in traversal order (properties/bounds
        // before method bodies, methods in AST declaration order, an
        // assignment's right-hand side before its left, ...) - not
        // necessarily line/column order once different diagnostic
        // categories or evaluation orders interleave. Sorted once here,
        // the single real source every caller (CLI output, LSP
        // publishing, this module's own tests) goes through.
        self.diagnostics
            .sort_by_key(|d| (d.span.start.line, d.span.start.col));
        self.diagnostics
    }

    #[hotpath::measure]
    pub fn check_program(&mut self, program: &[Stmt]) {
        // A whole-file scan (last `namespace` seen wins, same as
        // `NameContext::build` itself does) purely for the three
        // file-wide, position-independent questions right below - "does
        // this file import symbol X anywhere," "what classes anywhere in
        // it are rebind targets," "does it have a file-local Pest
        // `uses()`" - each already scans the *entire* program regardless
        // of where in it something appears, so there's no one "current
        // position" for them to need in the first place. Deliberately
        // NOT reused for `self.name_ctx` itself below - see the main
        // per-statement loop's own comment for why that one has to be
        // positional instead.
        let whole_file_ctx = NameContext::build(program);
        // Phase 48: `ExistingNamesInUseRule`'s own function-import half -
        // see `check_use_function_imports`'s own doc comment for why a
        // plain, class-shaped `use Foo;` is deliberately NOT
        // existence-checked here.
        self.check_use_function_imports(program);
        self.check_use_class_imports(program);
        // Phase 63: `UsedNamesRule`'s own case-insensitive
        // declaration-vs-import collision check - a whole-file,
        // position-independent scan for the identical reason as the
        // other checks in this block.
        self.check_used_names(program);
        // Phase 59: see `rebind_target_classes`'s own doc comment - built
        // once per file, ahead of `check_unused_private_members`'s own
        // per-class use below.
        let (rebind_targets, rebind_escape_all) =
            unused::scan_program_for_rebind_targets(program, &whole_file_ctx);
        self.rebind_target_classes = rebind_targets;
        self.rebind_escape_all = rebind_escape_all;
        // Finding 1: a file-local `uses(...)` call (no `->in(...)`) wins
        // over whatever `Project::check_file` already set via
        // `set_pest_this_type` from directory-scoped Pest.php config -
        // matches real Pest's own resolution order (file-local checked
        // first). A file with no such call leaves `pest_this_type`
        // exactly as `Project::check_file` set it.
        if let Some(local_ty) = pest_config::file_local_this_type(program, &whole_file_ctx) {
            self.pest_this_type = local_ty;
        }
        // Unlike `pest_this_type` just above, file-local and
        // directory-scoped hook properties are unioned, not one
        // overriding the other - a property-name set has no "file-local
        // wins" precedence to apply.
        self.pest_hook_properties
            .extend(pest_config::file_local_hook_properties(program));

        let mut top_locals: Locals = Locals::default();
        // Phase 17 follow-up: mirrors `check_function_with_ancestor_
        // hint`'s own identical wiring - a top-level-script-scope probe
        // (`lsp::navigation::ProbeScope::TopLevel`) needs this too.
        top_locals.set_track(self.probe_span.get().is_some());
        let diagnostics_before = self.diagnostics.len();
        // `self.name_ctx` is tracked *positionally* here, mirroring
        // `Registry::index_with_context`'s own identical walk exactly -
        // real PHP only ever allows a `namespace`/`use` statement at the
        // top level of a file (never nested inside a class/function/
        // block), so a single left-to-right pass over `program`'s own
        // top-level statements, mutating `self.name_ctx` in place when
        // one is seen, is already enough: by the time this loop reaches
        // a `ClassDecl`/`FunctionDecl`/`EnumDecl`, `self.name_ctx`
        // already correctly reflects whichever `namespace` was actually
        // active *at that declaration's own position*, not merely "the
        // last one anywhere in the file" the old whole-file `NameContext
        // ::build` pre-pass used to apply even to declarations under an
        // *earlier* one - a real, corpus-found false positive
        // (`jetbrains/phpstorm-stubs`' own `relay/Cluster.php`: `namespace
        // Relay;` for its own `Cluster` class, then a later, empty
        // `namespace Relay\Cluster;` for the rest of the file, wrongly
        // resolving `Cluster`'s own self-referencing return type as
        // `Relay\Cluster\Cluster` instead of `Relay\Cluster`).
        self.name_ctx = NameContext::default();
        for stmt in program {
            match &stmt.kind {
                StmtKind::Namespace(name) => self.name_ctx.set_namespace(name),
                StmtKind::Use(imports) => self.name_ctx.add_uses(imports),
                StmtKind::FunctionDecl(f) => self.check_function(f, None, false, true),
                StmtKind::ClassDecl(c) => {
                    // `current_class` must be the fully-qualified name -
                    // it's looked up directly against
                    // `self.registry.classes` (via `resolve_pseudo` for
                    // self/parent/static, and the undefined-class check
                    // for `new self()`/`new static()`), which is now
                    // FQN-keyed.
                    let fqn = self.name_ctx.declared_name(&c.name);
                    self.check_class_generic_bounds(&fqn, c);
                    self.check_class_completeness(&fqn, c.span);
                    self.check_final_class_extension(&fqn, c.span);
                    self.check_class_references_exist(&fqn, c.span);
                    self.check_internal_inheritance(&fqn, c.kind, c.span);
                    for (i, parent) in c.extends.iter().enumerate() {
                        let span = c.extends_spans.get(i).copied().unwrap_or(c.span);
                        self.check_class_name_case(&fqn, parent, span);
                    }
                    for (i, iface) in c.implements.iter().enumerate() {
                        let span = c.implements_spans.get(i).copied().unwrap_or(c.span);
                        self.check_class_name_case(&fqn, iface, span);
                    }
                    for (i, used) in c.uses.iter().enumerate() {
                        let span = c.uses_spans.get(i).copied().unwrap_or(c.span);
                        self.check_class_name_case(&fqn, used, span);
                    }
                    self.check_method_tag_types(&fqn, c.span);
                    self.check_mixin_types(&fqn, c.span);
                    self.check_type_alias_types(&fqn, c.span);
                    self.check_method_overrides(&fqn, c);
                    self.check_property_overrides(&fqn, c);
                    self.check_require_tags(&fqn, c.span);
                    self.check_require_tag_types(&fqn, c.span);
                    self.check_sealed_hierarchy(&fqn, c.span);
                    self.check_sealed_tag_types(&fqn, c.span);
                    self.check_unknown_phpstan_tags_in_class(&fqn, c);
                    self.check_property_hooks(&fqn, c);
                    self.check_property_variances(&fqn, c);
                    self.check_trait_conflicts(&fqn, c.span);
                    self.check_uninitialized_properties(&fqn, c);
                    self.check_premature_property_access(&fqn, c);
                    self.check_duplicate_class_members(&fqn, c);
                    self.check_method_declaration_sanity(&fqn, c.kind, c.is_abstract, &c.methods);
                    self.check_reserved_class_constant_name(&fqn, &c.consts);
                    self.check_typed_class_constant_values(&fqn, &c.consts);
                    self.check_class_constant_modifiers(&fqn, &c.consts);
                    // Phase 37: the class's own attributes, plus every
                    // property's/const's - method/param attributes are
                    // checked per-declaration inside `check_function`
                    // below instead, the one place that already runs for
                    // every method regardless of how it's reached (a
                    // direct method here, or via `check_trait_methods`'s
                    // own per-host loop for a trait).
                    self.check_attributes(&c.attrs, &fqn);
                    self.check_attribute_targets(&c.attrs, ATTRIBUTE_TARGET_CLASS, "class", &fqn);
                    self.check_self_attribute_class_declaration(
                        &c.attrs,
                        &fqn,
                        c.kind,
                        c.is_abstract,
                        &fqn,
                    );
                    self.check_class_like_marker_attributes(&c.attrs, c.kind, c.is_readonly, &fqn);
                    for p in &c.properties {
                        self.check_attributes(&p.attrs, &fqn);
                        self.check_attribute_targets(
                            &p.attrs,
                            ATTRIBUTE_TARGET_PROPERTY,
                            "property",
                            &fqn,
                        );
                    }
                    for cc in &c.consts {
                        self.check_attributes(&cc.attrs, &fqn);
                        self.check_attribute_targets(
                            &cc.attrs,
                            ATTRIBUTE_TARGET_CLASS_CONSTANT,
                            "class constant",
                            &fqn,
                        );
                    }
                    // Phase 58: a property/const's own missing-typehint
                    // completeness is about where it's textually
                    // declared, not which class(es) end up using it - so
                    // it's checked once here, regardless of trait-ness.
                    // `check_function` below runs once per *host* for a
                    // trait's own methods instead (a method's own
                    // `$this`-dependent body needs a real host to check
                    // against; a property/const's own declared type
                    // never does, so it has no such need).
                    self.check_missing_property_typehints(&fqn, &c.properties);
                    self.check_missing_class_constant_typehints(&fqn, &c.consts);
                    if c.kind == ClassKind::Trait {
                        self.check_trait_methods(&fqn, c);
                    } else {
                        for m in &c.methods {
                            self.check_function(
                                &m.function,
                                Some(&fqn),
                                m.is_static,
                                !m.is_abstract && c.kind != ClassKind::Interface,
                            );
                        }
                        // Phase 59: a class's own private consts/methods/
                        // properties, checked once per class (not per
                        // method the way check_function's own per-body
                        // checks are) since "was this ever referenced"
                        // needs every one of the class's own bodies
                        // scanned together, not just one at a time.
                        self.check_unused_private_members(
                            &fqn,
                            c.kind,
                            &c.methods,
                            &c.properties,
                            &c.consts,
                        );
                    }
                }
                StmtKind::EnumDecl(e) => {
                    let fqn = self.name_ctx.declared_name(&e.name);
                    self.check_class_completeness(&fqn, e.span);
                    self.check_class_references_exist(&fqn, e.span);
                    self.check_internal_inheritance(&fqn, ClassKind::Enum, e.span);
                    for (i, iface) in e.implements.iter().enumerate() {
                        let span = e.implements_spans.get(i).copied().unwrap_or(e.span);
                        self.check_class_name_case(&fqn, iface, span);
                    }
                    for (i, used) in e.uses.iter().enumerate() {
                        let span = e.uses_spans.get(i).copied().unwrap_or(e.span);
                        self.check_class_name_case(&fqn, used, span);
                    }
                    self.check_ancestor_generic_bounds(&fqn, e.span);
                    self.check_enum_template_tag(&fqn, e);
                    self.check_enum_case_values(&fqn, e);
                    self.check_enum_sanity(&fqn, e);
                    self.check_trait_conflicts(&fqn, e.span);
                    self.check_duplicate_enum_members(&fqn, e);
                    self.check_method_declaration_sanity(&fqn, ClassKind::Enum, false, &e.methods);
                    self.check_reserved_class_constant_name(&fqn, &e.consts);
                    self.check_typed_class_constant_values(&fqn, &e.consts);
                    self.check_class_constant_modifiers(&fqn, &e.consts);
                    self.check_attributes(&e.attrs, &fqn);
                    self.check_attribute_targets(&e.attrs, ATTRIBUTE_TARGET_CLASS, "class", &fqn);
                    self.check_self_attribute_class_declaration(
                        &e.attrs,
                        &fqn,
                        ClassKind::Enum,
                        false,
                        &fqn,
                    );
                    self.check_class_like_marker_attributes(&e.attrs, ClassKind::Enum, false, &fqn);
                    for case in &e.cases {
                        self.check_attributes(&case.attrs, &fqn);
                        self.check_attribute_targets(
                            &case.attrs,
                            ATTRIBUTE_TARGET_CLASS_CONSTANT,
                            "class constant",
                            &fqn,
                        );
                    }
                    for cc in &e.consts {
                        self.check_attributes(&cc.attrs, &fqn);
                        self.check_attribute_targets(
                            &cc.attrs,
                            ATTRIBUTE_TARGET_CLASS_CONSTANT,
                            "class constant",
                            &fqn,
                        );
                    }
                    // Phase 58: an enum can't declare instance properties
                    // at all (see `ast::EnumDecl`'s own doc comment), but
                    // its own consts are exactly as real a
                    // missing-typehint candidate as a class's.
                    self.check_missing_class_constant_typehints(&fqn, &e.consts);
                    for m in &e.methods {
                        self.check_function(&m.function, Some(&fqn), m.is_static, !m.is_abstract);
                    }
                    // An enum can't declare instance properties at all
                    // (see `ast::EnumDecl`'s own doc comment) - only its
                    // own consts/methods are real unused-private-member
                    // candidates.
                    self.check_unused_private_members(
                        &fqn,
                        ClassKind::Enum,
                        &e.methods,
                        &[],
                        &e.consts,
                    );
                }
                // Top-level script statements share one scope across the
                // whole file, so the extract()/eval() guard spans the
                // entire loop rather than resetting per statement.
                _ => self.check_stmt(stmt, &mut top_locals, None, "<main>"),
            }
        }
        if self.saw_dynamic_var_construct {
            self.diagnostics.truncate(diagnostics_before);
            self.saw_dynamic_var_construct = false;
        }
    }
}

#[cfg(test)]
mod tests;
