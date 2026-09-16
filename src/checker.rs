use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::pest_config;
use crate::registry::{NameContext, Registry};
use crate::token::Span;
use crate::types::Type;

/// A checked scope's variable-name-to-inferred-type bindings.
///
/// Backed by `imbl::HashMap` (a persistent, structurally-shared HAMT), not
/// `std::collections::HashMap`: this is cloned once per branch of every
/// `if`/`while`/ternary/`match`/closure across the checker, and a plain
/// `HashMap` clone rehashes every key and deep-clones every `Type`'s nested
/// `String`s/`Vec`s. `imbl::HashMap::clone` is O(1) (a pointer bump plus a
/// refcount increment); a value is only deep-copied lazily, on the next
/// mutation of whichever clone diverges. `rustc_hash::FxBuildHasher` as the
/// hasher, since `imbl::HashMap` is generic over it the same way
/// `std::collections::HashMap` is.
type LocalsMap = imbl::GenericHashMap<
    String,
    Type,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// `imbl::HashSet` mirrors `imbl::HashMap`'s O(1)-clone design (see
// `LocalsMap`'s doc comment) - not load-bearing here (a per-variable span
// set is small and never iterated at scale), but kept consistent with the
// persistent-map-throughout convention the rest of this module follows.
type ProvenanceSet =
    imbl::GenericHashSet<Span, rustc_hash::FxBuildHasher, imbl::shared_ptr::DefaultSharedPtr>;
type ProvenanceMap = imbl::GenericHashMap<
    String,
    ProvenanceSet,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// Lowercased, bare `function_exists('foo')`-guarded function names proven
// guarded on this exact control-flow path. `Locals`-scoped, not a `Checker`
// field, so it merges/narrows per-branch via `apply_narrowing`/
// `merge_scopes` like `types`/`provenance` do, rather than persisting for a
// whole function body unconditionally.
type GuardedFunctionSet =
    imbl::GenericHashSet<String, rustc_hash::FxBuildHasher, imbl::shared_ptr::DefaultSharedPtr>;

// `(receiver_var_name, member_name)` pairs proven guarded by a
// `method_exists($recv, 'name')`/`property_exists($recv, 'name')` true
// branch on this exact control-flow path - same Locals-scoped, O(1)-clone
// design as `GuardedFunctionSet`.
type GuardedMemberSet = imbl::GenericHashSet<
    (String, String),
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// `name -> (target_key, Rc<Expr>)` for a variable whose last assignment's
// RHS was itself a recognized narrowable condition shape (see
// `condition_alias_shape`/`condition_alias_target` in `checker/
// narrowing.rs`), replayed through `apply_narrowing` when `name` is later
// checked bare (`if ($name)`) - correlates a *second* variable's narrowing
// to `name`'s own truthiness without any new type math: the stored `Expr`
// is the original AST node, re-run through the exact dispatch a direct
// `if (<rhsExpr>)` would use. `target_key` (computed once at store time,
// mirroring `narrowing_key`'s own string form) is what `Locals::declare`
// scans for to invalidate a stale alias whose *target* was rebound, not
// just the alias variable itself. `Rc`, not a deep clone, so every branch
// fork of `Locals` stays O(1); a given alias's `Expr` never changes after
// it's first stored, only wholesale-replaced or purged, so `Rc::ptr_eq` is
// a correct, cheap "did both branches inherit this unchanged" merge test
// (see `merge_scopes`).
type ConditionAliasMap = imbl::GenericHashMap<
    String,
    (String, std::rc::Rc<Expr>),
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// `discriminant_canonical_key(subject_key, value) -> [(implied_var, implied_type), ...]`
// - a same-branch discriminant-variable correlation: whenever an
// `if`'s own condition matches the *same* subject/value pair a prior
// branch already recorded a fact under, every `(var, type)` pair here
// replays into `locals` alongside that condition's own ordinary
// narrowing (see `checker/narrowing.rs`'s `record_branch_implied_facts`/
// `apply_equality_narrowing`'s own replay hook). `Rc`-wrapped `Vec`,
// mirroring `ConditionAliasMap`'s own `Rc<Expr>` value, so cloning
// `Locals` (extremely frequent - one clone per branch fork) stays O(1)
// even though a fact list itself isn't `Copy`. Unlike `condition_
// aliases` (purged the moment its own target is rebound), a recorded
// fact here is a durable structural fact about the code at this exact
// discriminant, not per-branch state - `merge_scopes` unions both
// sides' entries rather than intersecting.
type BranchImpliedFactsMap = imbl::GenericHashMap<
    String,
    std::rc::Rc<Vec<(String, Type)>>,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

/// A checked scope's variable-name-to-inferred-type bindings, plus an
/// opt-in, parallel declaration-span side-channel the LSP's local-variable-
/// navigation feature reads through `Checker::probe_span`/
/// `probe_var_provenance` - reaching-definitions info computed by riding
/// along the checker's own flow-sensitive traversal (branch merging via
/// `merge_scopes`, loop convergence via `converge_loop_body`, termination-
/// pruning via `always_terminates`) rather than a second, independent
/// approximation. `types`/`provenance` are separate fields, not one field
/// carrying `(Type, ProvenanceSet)` pairs, since `Type` is pattern-matched
/// at hundreds of call sites throughout this module and a wrapper tuple
/// would mean unwrapping it back out everywhere for a concern (LSP
/// navigation) unrelated to type inference. Kept in lockstep with `types`
/// via `Locals`'s own API: `insert` leaves `provenance` untouched (correct
/// for narrowing, which refines an existing binding without creating a new
/// one); `declare` is the one real new-declaration primitive;
/// `insert_with_provenance` copies an already-known provenance set across
/// when a value moves between two `Locals` unchanged (e.g. `merge_scopes`'s
/// union).
///
/// `provenance`, like `types`, is backed by `imbl::HashMap` so cloning a
/// `Locals` stays two O(1) pointer bumps rather than reintroducing a deep
/// clone.
#[derive(Clone, Default)]
pub(crate) struct Locals {
    types: LocalsMap,
    provenance: ProvenanceMap,
    pub(crate) guarded_functions: GuardedFunctionSet,
    pub(crate) guarded_methods: GuardedMemberSet,
    pub(crate) guarded_properties: GuardedMemberSet,
    pub(crate) condition_aliases: ConditionAliasMap,
    pub(crate) branch_implied_facts: BranchImpliedFactsMap,
    // Whether `declare`/`insert_with_provenance` do anything - `false` by
    // default for every ordinary check-project run, `true` only for the
    // LSP's own per-request variable-navigation probe. Carried on `Locals`
    // itself, not read from `Checker`, because `Locals` values are
    // constructed in more places than `Checker` methods reach - notably a
    // closure/arrow-fn literal's fresh scope, which needs the enclosing
    // scope's `track` value threaded through explicitly (`fresh_scope`
    // below) rather than defaulting to `false` and losing provenance inside
    // a closure body.
    track: bool,
}

impl Locals {
    pub(crate) fn get(&self, key: &str) -> Option<&Type> {
        self.types.get(key)
    }

    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.types.contains_key(key)
    }

    /// Provenance-preserving update, for narrowing an existing binding
    /// (`instanceof`/null/array-shape checks) or a by-ref call's type
    /// widening (`sort($arr)`, `preg_match(..., $matches)`) - the value's
    /// declaration site doesn't move just because its known type changed.
    pub(crate) fn insert(&mut self, key: String, ty: Type) -> Option<Type> {
        self.types.insert(key, ty)
    }

    /// The one real new-declaration primitive - used only at a genuine
    /// syntactic binding site (assignment LHS, parameter, `foreach`/`catch`
    /// binding, `static $x`, closure `use (...)`, `$x++`/`$x--`). Replaces
    /// `key`'s provenance with a fresh singleton `{span}`, since a rebind
    /// makes anything tracked before it a dead candidate.
    pub(crate) fn declare(&mut self, key: String, ty: Type, span: Span) -> Option<Type> {
        if self.track {
            let mut set = ProvenanceSet::default();
            set.insert(span);
            self.provenance.insert(key.clone(), set);
        }
        self.purge_condition_aliases_targeting(&key);
        self.types.insert(key, ty)
    }

    /// The store half of the condition-alias mechanism - see
    /// `ConditionAliasMap`'s doc comment. `target_key` is `condition`'s
    /// narrowing target (e.g. `$x` for `$x instanceof Foo`), computed by
    /// the caller (`Checker::record_condition_alias`) via
    /// `condition_alias_target`.
    pub(crate) fn set_condition_alias(
        &mut self,
        name: String,
        target_key: String,
        condition: std::rc::Rc<Expr>,
    ) {
        self.condition_aliases.insert(name, (target_key, condition));
    }

    /// Drops `key`'s own alias (rebinding the alias variable makes it
    /// stale) and every other alias whose `target_key` is `key` (rebinding
    /// an alias's *target* makes the alias stale too, even though its own
    /// name is untouched). `Checker::record_condition_alias` re-inserts a
    /// fresh alias for `key` right after, when the new RHS qualifies, so a
    /// genuine re-alias isn't lost - only ever a stale one.
    fn purge_condition_aliases_targeting(&mut self, key: &str) {
        self.condition_aliases.remove(key);
        self.condition_aliases
            .retain(|_, (target, _)| target != key);
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
            guarded_methods: GuardedMemberSet::default(),
            guarded_properties: GuardedMemberSet::default(),
            condition_aliases: ConditionAliasMap::default(),
            branch_implied_facts: BranchImpliedFactsMap::default(),
            track: self.track,
        }
    }

    /// Turns declaration-span tracking on for this scope and everything
    /// cloned from it - called once, at `check_function`'s entry point,
    /// gated on `Checker::probe_span` being active.
    pub(crate) fn set_track(&mut self, track: bool) {
        self.track = track;
    }
}

impl PartialEq for Locals {
    /// Compares `types` only - `Checker::converge_loop_body`'s fixed-point
    /// check must converge on type stability alone. `provenance`,
    /// `guarded_functions`/`guarded_methods`/`guarded_properties`, and
    /// `condition_aliases` are bookkeeping side-channels riding along
    /// `types`'s convergence, never inputs to it; on the rare pass where
    /// they haven't independently stabilized, whatever the converging pass
    /// left is used as a bounded approximation, still capped at
    /// `MAX_PASSES`.
    fn eq(&self, other: &Self) -> bool {
        self.types == other.types
    }
}

/// A generic `@template` name -> concrete `Type` substitution map (class-
/// hierarchy template binding, function-call argument-to-template
/// binding) - built once per call/hierarchy-walk and consumed immediately
/// by `types::substitute_template`/`bind_template_param`, never cloned
/// per-branch the way `Locals` is. A plain `std::collections::HashMap`,
/// since this use case has none of the "cloned once per branch" cost
/// profile that motivates `Locals`'s persistent-map backing.
pub(crate) type TemplateBindings = std::collections::HashMap<String, Type>;

/// Loosely mirrors PHPStan's own rule-gating levels (0 lowest/laxest,
/// per `conf/config.levelN.neon`): 0 for the
/// "definitely broken" undefined-symbol/arity family (matches real
/// PHPStan's own level-0 bucket - "unknown classes, unknown functions,
/// unknown methods..., wrong number of arguments..., always undefined
/// variables" - though this project doesn't implement PHPStan's own
/// $this-only-vs-any-expression distinction for method/property checks,
/// a documented simplification, not an oversight), 1 for unused-
/// constructor-parameter/unused-closure-use checks (matches real
/// PHPStan's `UnusedConstructorParametersRule`/`UnusedClosureUsesRule`
/// level exactly), 2 for undefined-property/array-shape-key (PHPDoc-
/// derived structural checks, real PHPStan's `checkThisOnly: false`/
/// `checkPhpDocMissingReturn` territory), 3 for return-type/property-type
/// mismatches (matches real PHPStan's `checkPhpDocMethodSignatures` level),
/// 4 for unused-private-const/method/property checks (again matching real
/// PHPStan's own level), 5 for argument-type mismatches (matches real
/// PHPStan's `checkFunctionArgumentTypes` level), and 6 for this project's
/// own generic bound/variance checks (no direct real-PHPStan equivalent -
/// generics are baked into its core type engine, not level-gated - placed
/// here as this project's own "advanced, opt-in" tier, alongside real
/// PHPStan's missing-typehints level).
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
    /// involved. Not guaranteed to match phpstan/phpstan's own strings
    /// one-for-one, though it reuses real PHPStan's spelling where
    /// confident.
    pub identifier: String,
    /// An optional one-line, human-facing hint appended below the main
    /// message - real PHPStan's own "💡 Tip: ..." line.
    pub tip: Option<String>,
    /// Mirrors real PHPStan's `RuleErrorBuilder::nonIgnorable()`: when
    /// `true`, this diagnostic must never be suppressible via
    /// `@phpstan-ignore <identifier>`.
    pub non_ignorable: bool,
    /// Free-form structured data a rule wants attached without inventing a
    /// new `Diagnostic` field - a `HashMap`, not `Vec<(String, Value)>`,
    /// since a consumer looks up one known key rather than iterating all.
    pub metadata: HashMap<String, serde_json::Value>,
    /// A suggested textual auto-fix. This project has no code-generation-
    /// from-AST facility, so a fix is a literal source-text replacement
    /// over a `Span` rather than a replacement AST node.
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

/// Fluent assembly for a [`Diagnostic`], mirroring real PHPStan's own
/// `RuleErrorBuilder`. A plain diagnostic with no tip/metadata/fix still
/// goes through this via `Checker::report` (a thin wrapper), so there's one
/// real construction path for every `Diagnostic`.
///
/// Consumed by value (`build(self)`, each setter takes/returns `Self`): a
/// `DiagnosticBuilder` is always a short-lived expression built and
/// immediately consumed at one `report_builder` call site, never stored or
/// branched on mid-construction.
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
    /// `identifier` is required up front, unlike real PHPStan's builder
    /// (which allows a later `.identifier(...)` call) - every call site
    /// here already knows its identifier, so there's no "forgot to set it"
    /// state worth representing.
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
/// Undefined-class/function/method reference checks resolve every name to
/// a fully-qualified one first via `registry::NameContext` (tracking each
/// file's `namespace`/`use` statements) before any registry key is read or
/// written - two classes with the same short name in different namespaces
/// get distinct keys, the same way real PHP tells them apart.
pub struct Checker<'a> {
    pub(crate) registry: &'a Registry,
    pub(crate) diagnostics: Vec<Diagnostic>,
    // Namespace + `use`-import map, built once per file. Needed because
    // almost all real code refers to an imported class/function by its
    // short name, not a fully-qualified path - without resolving it here
    // first, undefined-class/function detection would flag the
    // overwhelming majority of legitimately-imported symbols. Also must
    // resolve to exactly the same fully-qualified names `Registry::index`
    // keys its entries under, or the two would silently disagree.
    pub(crate) name_ctx: NameContext,
    // The class whose method body is currently being checked, if any -
    // needed to resolve self/parent/static to a concrete class name.
    pub(crate) current_class: Option<String>,
    // The name of the method whose body is currently being checked, if
    // any - needed to tell whether a `readonly` property write is
    // happening inside its own declaring class's constructor (any write
    // outside that exact spot is flagged).
    pub(crate) current_method: Option<String>,
    // The trait whose method bodies are currently being checked, if any -
    // `check_trait_methods` re-checks a trait's methods once per using
    // host, with `current_class` set to the *host*, so a trait's own
    // `@deprecated` tag is otherwise unreachable through `current_class`.
    // Set/restored around `check_trait_methods`'s host loop, independent
    // of `current_class`.
    pub(crate) current_trait: Option<String>,
    // Whether the body currently being checked is itself inside a
    // deprecated scope (its enclosing function/method, class, or trait
    // carries `@deprecated`) - mirrors real PHPStan's
    // `DeprecatedScopeHelper`, which suppresses deprecated-usage
    // diagnostics from within such a scope. Computed once per
    // function-like body in `check_function_with_ancestor_hint`, the
    // single choke point where `current_class`/`current_method` are also
    // set.
    pub(crate) current_scope_deprecated: bool,
    // Set for the duration of one function/method (or the top-level script)
    // body when it calls extract()/eval() - both can define variables this
    // checker has no way to see, so undefined-variable detection would be
    // pure noise for the rest of that body. Rather than track precisely
    // which diagnostics came from that check, the whole body's diagnostics
    // are discarded if this ends up true (see check_function).
    pub(crate) saw_dynamic_var_construct: bool,
    // Set for the duration of one function/method/closure body that
    // contains a yield/yield from anywhere - a real generator, whose
    // implicit `return null` at the end (or an explicit bare `return;`)
    // never mismatches its declared Generator/Iterator/Traversable return
    // type, unlike an ordinary function. Consulted by both
    // check_function's falls-off-the-end check and check_stmt's
    // StmtKind::Return(None) arm, neither of which has direct access to
    // the enclosing function's body to answer this itself.
    pub(crate) current_function_is_generator: bool,
    // The enclosing function/method's docblock-preferring declared return
    // type (not `native_return` - only the docblock spelling can carry
    // `Generator<TKey, TValue, TSend, TReturn>`'s generic args),
    // `resolve_pseudo`'d against the current class already - set/restored
    // alongside `current_function_is_generator` at the same choke point.
    // Consulted by `ExprKind::Yield`/`YieldFrom` to resolve `yield`'s
    // TSend and `yield from`'s TReturn via `generator_template_param`.
    pub(crate) current_declared_return_type: Option<Type>,
    // Every real throw point recorded so far inside the enclosing
    // function/method/closure body - an explicit `throw` expression's
    // thrown-value type, or a call's resolved `@throws` type (a matching
    // `checker/throws/dynamic.rs` per-callee override tried first) -
    // recorded at each of `ExprKind::Call`/`New`/`MethodCall`/`StaticCall`'s
    // resolved-signature choke points. `Type::Void` contributes nothing,
    // matching real PHPStan. Consumed by `checker/throws/too_wide.rs`'s
    // `TooWideFunctionThrowTypeRule`/`TooWideMethodThrowTypeRule` port.
    pub(crate) current_function_throw_points: Vec<Type>,
    // Set `true` the moment any call inside the enclosing body resolves to
    // no signature at all (a dynamic call, or a genuinely unresolvable
    // one) - such a callee could throw anything, so its real throw-ness
    // can't be bounded from here. Does NOT fire for a resolved signature
    // that simply has no `@throws` tag - see `Checker::
    // record_call_throw_point`'s doc comment for why that distinction
    // matters. Consumed by the same too-wide-throw rules, which suppress
    // their comparison entirely when this is `true`, favoring a missed
    // diagnostic over a wrongly-confident one.
    pub(crate) current_function_has_unannotated_call: bool,
    // The explicit-`throw`-only subset of `current_function_throw_points`,
    // each paired with its own `throw` expression's span - never populated
    // by a call's `@throws`-derived contribution, since that belongs to
    // the callee, not this declaration's own code. Consumed by
    // `checker/throws/missing.rs`'s "missing checked exception" family,
    // opt-in via `Registry::check_missing_checked_exceptions`.
    pub(crate) current_function_explicit_throw_points: Vec<(Type, Span)>,
    // Every real return point recorded so far inside the enclosing body -
    // `StmtKind::Return(Some(e))`'s value type, `StmtKind::Return(None)`'s
    // implicit `Type::Void` (real PHPStan pushes `VoidType`, not `Null`,
    // for a bare `return;`), and the falls-off-the-end path's implicit
    // `Type::Null` (only when the native return type allows it). Never
    // populated for a generator - its `return` sets `Generator::
    // getReturn()`, not the function's own return value. Consumed by
    // `checker::return_type::too_wide`'s
    // `TooWideFunctionReturnTypehintRule`/`TooWideMethodReturnTypehintRule`.
    pub(crate) current_function_return_points: Vec<Type>,
    // Real PHPStan's `TooWideTypeCheck` bails via a count of *explicit*
    // return statements, not "is the accumulated return-point vector
    // empty" - the falls-off-the-end contribution alone can make
    // `current_function_return_points` non-empty with zero explicit
    // returns, which would otherwise spuriously flag a function whose
    // `return.missing` bug (a forgotten return) is already separately
    // caught. Set the moment either `Return` arm runs, consulted instead
    // of `.is_empty()` by `check_too_wide_return_type`.
    pub(crate) current_function_saw_explicit_return: bool,
    // By-ref parameter direct-assignment/execution-end tracking -
    // `name -> declared type` for every by-ref parameter of the enclosing
    // function/method/closure (native type, or `@param-out` when the tag
    // supplies one - see `ParamSig::out_ty`'s doc comment), set/restored
    // at the same choke point as `current_declared_return_type`.
    pub(crate) current_function_by_ref_params: std::collections::HashMap<String, Type>,
    // Every value ever assigned to a by-ref parameter anywhere in the
    // enclosing body (direct/compound assignment, destructuring,
    // `foreach`-rebind) - unioned and compared against each by-ref
    // parameter's declared type by `check_too_wide_by_ref_param_type`, the
    // by-ref-parameter analogue of `current_function_return_points`.
    pub(crate) current_function_by_ref_assigned_types: std::collections::HashMap<String, Vec<Type>>,
    // Set for the duration of one function/method/closure body to every
    // label name `goto`-targeted anywhere in it (see
    // `collect_goto_targets`'s doc comment for the real corpus false
    // positive this exists for) - consulted by `check_block`'s
    // `deadCode.unreachable` check, which has no direct access to the
    // enclosing function's body to answer "could a `goto` reach this
    // labeled statement" itself.
    pub(crate) current_function_goto_targets: std::collections::HashSet<String>,
    // Set to `true` immediately before `check_expr` is called for a
    // `StmtKind::Expression`'s top-level expression (a bare `foo();`, its
    // return value going nowhere by construction), cleared the instant
    // `check_expr` reads it (`std::mem::take`, at the top of its
    // dispatch) - so it only ever reads `true` for that exact top-level
    // call, never for anything nested inside it (`foo(bar())`'s inner
    // `bar()` call sees `false`, correctly, since its return value is
    // consumed as an argument). `check_useless_return_value_call` is this
    // flag's one reader, mirroring real PHPStan's
    // `UselessFunctionReturnValueRule`.
    pub(crate) in_bare_statement_position: bool,
    // Mirrors `in_bare_statement_position`'s `std::mem::take`-at-top-of-
    // `check_expr` idiom - set to `true` immediately before `check_expr`
    // is called for the direct `cond` of an `if`/`elseif`/`while`/
    // `do-while`/ternary (never `for` - real PHPStan has no dedicated
    // for-condition rule), cleared the instant `check_expr` reads it.
    // Real PHPStan's `Scope::isInFirstLevelStatement` equivalent:
    // `BooleanAndConstantConditionRule`/`BooleanOrConstantConditionRule`'s
    // right-side/whole-expression diagnostics are skipped exactly when the
    // `&&`/`||` itself *is* one of those direct conditions, but never for
    // one reached any other way.
    pub(crate) in_condition_position: bool,
    // Same `std::mem::take` idiom, set (alongside `in_condition_position`)
    // only when the direct condition about to be checked is a genuine
    // `elseif` with no trailing `else` and nothing after it in the chain -
    // real PHPStan's "acts as an implicit else" exemption
    // (`reportAlwaysTrueInLastCondition: false`'s default). `check_if`
    // already applies this to its own `elseif.alwaysTrue`, but
    // `BooleanNotConstantConditionRule` has the same exemption for a
    // standalone `!$expr` condition - `should_skip_constant_condition`
    // deliberately excludes `!` from `check_if`'s own top-level check
    // (deferring to this dedicated one), so without this flag that
    // deferral would silently lose the exemption.
    pub(crate) in_exempt_last_condition: bool,
    // The `$this` type inside a Pest `it`/`test`/`describe`/`beforeEach`/
    // `afterEach`/`beforeAll`/`afterAll` closure - set via
    // `set_pest_this_type` (by `Project::check_file`, from
    // `PestConfig::resolve_this_type`) before `check_program` runs, then
    // possibly overridden by a file-local `uses(...)` call `check_program`
    // itself finds (real Pest's resolution order: file-local wins over
    // directory-scoped). Defaults to `Type::Mixed`, so any call site never
    // exercising an `is_this_binding_call`-recognized closure argument is
    // unaffected.
    pub(crate) pest_this_type: Type,
    // Dynamic property names a Pest `beforeEach(...)` closure assigns onto
    // the shared `$this` (see `pest_config`'s
    // `file_local_hook_properties`/`PestConfig::resolve_hook_properties`) -
    // set via `set_pest_hook_properties` before `check_program` runs, then
    // unioned (not overridden - a property-name set has no "most specific
    // wins" precedence) with whatever file-local `beforeEach(...)` calls
    // `check_program` itself finds. Defaults empty.
    pub(crate) pest_hook_properties: HashSet<String>,
    // Every private property write's own assigned type, discovered
    // anywhere in this file's own `check_program` run - keyed by
    // (declaring class fqn, property name), never reset per-class:
    // `check_trait_methods` re-checks a trait's own methods once per
    // host with `current_class` set to that host, so a trait-donated
    // property write's own contribution must accumulate under the
    // host's fqn regardless of which `ClassDecl` node `check_program`
    // happens to be visiting when it runs - a per-class reset would
    // wipe out an earlier host's own entries the moment a *different*
    // class starts being checked. Consumed by
    // `check_too_wide_property_types`, this project's own
    // `TooWideFunctionReturnTypehintRule`-equivalent for properties.
    pub(crate) property_assign_facts: HashMap<String, HashMap<String, Vec<Type>>>,
    // The project-wide merge of every file's own `property_assign_facts`
    // - set via `set_cross_file_property_assign_facts` (by `Project::
    // check_file`, from a first, fact-collecting pass over every loaded
    // file) before `check_program` runs. `None` for a standalone
    // `Checker` never wired through that two-pass orchestration (most
    // unit tests): `check_too_wide_property_types` then falls back to
    // this run's own same-file `property_assign_facts`, correct for the
    // common case where a trait and the host using it are declared (and
    // visited, in that order) in the one file being checked.
    pub(crate) cross_file_property_assign_facts:
        Option<HashMap<String, HashMap<String, Vec<Type>>>>,
    // One frame per currently-open loop or switch, collecting
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
    // The `continue` counterpart of `loop_break_states` - one frame per
    // currently-open loop, collecting the `locals` snapshot at every
    // `continue` reached inside it. Unlike `break`'s states (folded once,
    // after all of a loop's convergence passes finish, into what flows
    // past the loop), `continue`'s states are consumed *between* passes,
    // inside `converge_loop_body` itself: a `continue` jumps back to the
    // top of the loop, so whatever it reached is as much a real "entry to
    // the next iteration" as the body's own fall-through exit is. Pushed/
    // popped once per pass, not once per loop - each pass's continue-
    // reached states inform only that pass's contribution to the next
    // entry. Without this, a variable reassigned only on a path that ends
    // in `continue` never fed its new value into the next convergence
    // pass, so convergence would silently stop after one pass against a
    // stale pre-loop entry.
    pub(crate) loop_continue_states: Vec<Vec<Locals>>,
    // How many loop bodies (`While`/`DoWhile`/`For`/`Foreach`, nesting
    // included - a `Switch` doesn't count, it runs its body once)
    // currently enclose whatever's being checked. `check_pointless_
    // null_probe` and similar "is this ever true across the whole
    // function" checks need a simple "any loop at all, at any depth,
    // still open right now" signal, distinct from `loop_break_states`'s
    // own stack (which also counts an enclosing `Switch` frame).
    pub(crate) loop_depth: u32,
    // Every variable name seen bound via `static $x = ...;` anywhere in
    // the file being checked. A `static` local can carry a value left
    // over from an earlier call - the same "this single forward pass
    // can't see a wider type a different execution of the same code
    // would" unsoundness `loop_depth` guards against, just across
    // invocations instead of iterations (real PHPStan stays silent on
    // this shape too). Accumulated across the whole file rather than
    // reset per function/closure - a stale entry only costs a missed
    // diagnostic on an unrelated later local sharing the same name.
    pub(crate) static_var_names: HashSet<String>,
    // Every class named as the resolved `$scope` of a `Closure::
    // bind(...)`/`->bindTo(...)` call anywhere in the file being checked
    // (`unused::scan_program_for_rebind_targets`, built once per
    // `check_program`) - a real, idiomatic way to reach a class's private
    // members from a closure declared outside that class's own body,
    // invisible to `check_unused_private_members`'s per-class-body-only
    // usage scan otherwise. `rebind_escape_all` is the coarser fallback
    // for a bind/bindTo call whose scope isn't a literal `Foo::class` this
    // project can resolve without full type inference - safer to skip
    // every class's private-member check for the whole file than risk a
    // false "unused" report on a member a rebound closure actually
    // reaches.
    pub(crate) rebind_target_classes: HashSet<String>,
    pub(crate) rebind_escape_all: bool,
    // The LSP's one-off "what type is this instance receiver" probe.
    // `probe_span` is the exact `Span` of the receiver `Expr` the caller
    // wants typed; `check_expr`'s `MethodCall`/`PropertyAccess` arms
    // already compute the receiver's real `Type` as part of ordinary
    // checking (`recv_ty`) - this just also stashes it in `probe_hit` when
    // the receiver's span matches. Matching by exact span (not "nearest
    // enclosing") sidesteps inner-vs-outer receiver ambiguity for a
    // chained call (`$x->foo()->bar()`) - `lsp::navigation` already
    // resolved which receiver it wants before constructing this `Checker`.
    // `None` (every ordinary call site) is a complete no-op.
    //
    // `Cell`/`RefCell`, not plain fields: the declaration-site hover probe
    // (`Checker::probe_hit_at`) needs to stash a hit from deep inside
    // `bind_assignment_target`/`bind_simple_target`/`apply_narrowing`,
    // several of which only ever hold `&self` (e.g. `infer_expr`/
    // `infer_binary`, whose narrowing side effects go through `&mut
    // Locals`, not `&mut self`). Threading `&mut self` through that whole
    // call graph for a one-off LSP probe would be a large, high-risk
    // signature change; interior mutability keeps every signature
    // untouched.
    pub(crate) probe_span: Cell<Option<Span>>,
    pub(crate) probe_hit: RefCell<Option<Type>>,
    // The LSP's one-off "where could this local variable read have come
    // from" probe - the flow-sensitive counterpart of `lsp::navigation`'s
    // hand-rolled arm-merging heuristic. Checked in `check_expr`'s
    // `ExprKind::Variable` arm the same way `probe_hit` is in `MethodCall`/
    // `PropertyAccess`: when `probe_span` matches this read's span,
    // `locals`'s tracked provenance for that name is stashed here. `None`
    // (every ordinary call site) is a complete no-op - see `Locals::track`'s
    // doc comment for why declaration tracking itself is also gated off
    // whenever no probe is active.
    pub(crate) probe_var_provenance: RefCell<Option<HashSet<Span>>>,
    // A `Call(callee, ...)` node's resolved `(primary, fallback)` name
    // pair, keyed by `callee`'s span - `check_expr`'s `Call` arm and
    // `infer_expr`'s separate `Call` arm both independently re-derive this
    // identical pair for the same callee whenever `infer_expr` is invoked
    // on a call `check_expr` already resolved (e.g. a call nested as
    // another call's argument, re-derived via `check_args`'s contextual-
    // type inference). `RefCell`, not a plain field, for the same reason
    // `probe_hit`/`probe_var_provenance` are: populated/read through a
    // shared `&self` from both `check_expr` (`&mut self`) and `infer_expr`
    // (`&self`) without threading a new parameter through either's call
    // graph. Scoped to one `Checker` (one file), so it never leaks across
    // files.
    call_candidate_cache: RefCell<HashMap<Span, (String, Option<String>)>>,
}

mod assignment;
mod by_ref_params;
mod calls;
mod coercion;
mod control_flow;
mod date_interval;
mod deprecated_usage;
pub(crate) mod duplicate_classes;
pub(crate) mod duplicate_functions;
mod duplicates;
mod expr;
mod function_decl;
mod generics;
mod imports;
pub(crate) use imports::INTERNAL_CLASS_PREFIXES;
mod internal_rules;
mod internal_usage;
mod missing_typehint;
mod narrowing;
mod noop;
mod oop;
mod operators;
pub(crate) mod phpstan_api;
mod preg_pattern_validity;
mod property_assignment;
mod property_type_too_wide;
mod resolve;
mod restricted_symbol_usage;
mod return_type_too_wide;
mod stmt;
mod throws;
mod type_union;
mod uninitialized;
mod uninitialized_dataflow;
mod unused;
mod visibility;
mod xml_name;
mod xml_wellformedness;

pub(crate) use assignment::{
    is_superglobal, literal_string_key, literal_string_value, superglobal_default_type,
};
pub(crate) use calls::{CheckArgsCall, is_language_construct, resolve_call_args};
pub(crate) use coercion::{to_absolute_number, to_bitwise_not};
pub(crate) use control_flow::{
    LoopZeroIterations, body_contains_yield, collect_exit_point_statements, collect_goto_targets,
    exit_point_description, expr_contains_yield, generator_missing_return_type,
    generator_template_param, is_switch_break_equivalent, is_truthy_literal, merge_scopes,
    stmt_contains_break, stmt_contains_escaping_exit, type_allows_generator_return,
};
pub(crate) use duplicates::class_kind_identifier;
pub(crate) use expr::{
    build_matched_shape, int_range_bounds, normalize_int_range, unwrap_grouping,
};
pub(crate) use internal_usage::class_type_word;
pub(crate) use missing_typehint::{
    ITERABLE_GENERIC_CLASS_NAMES, class_kind_word, has_explicit_mixed_nested_somewhere,
};
pub(crate) use narrowing::{
    array_value_type_for_key, is_real_variable_key, is_unsealed_shape,
    narrow_array_shape_by_key_presence, narrow_type_excluding, narrow_type_excluding_falsy,
    narrow_type_to, narrowing_key, purge_stale_narrowing_for, resolve_concat_type,
    string_type_has_accessory,
};
pub(crate) use oop::{
    ATTRIBUTE_TARGET_CLASS, ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_FUNCTION,
    ATTRIBUTE_TARGET_METHOD, ATTRIBUTE_TARGET_PARAMETER, ATTRIBUTE_TARGET_PROPERTY,
};
pub(crate) use throws::{body_contains_explicit_throw, expr_contains_explicit_throw};
#[cfg(test)]
pub(crate) use type_union::generalize_by_key_set;
pub(crate) use type_union::union_types;
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
            current_function_return_points: Vec::new(),
            current_function_saw_explicit_return: false,
            current_function_by_ref_params: std::collections::HashMap::new(),
            current_function_by_ref_assigned_types: std::collections::HashMap::new(),
            current_function_goto_targets: std::collections::HashSet::new(),
            in_bare_statement_position: false,
            in_condition_position: false,
            in_exempt_last_condition: false,
            pest_this_type: Type::Mixed,
            pest_hook_properties: HashSet::new(),
            property_assign_facts: HashMap::new(),
            cross_file_property_assign_facts: None,
            loop_break_states: Vec::new(),
            loop_continue_states: Vec::new(),
            loop_depth: 0,
            static_var_names: HashSet::new(),
            rebind_target_classes: HashSet::new(),
            rebind_escape_all: false,
            probe_span: Cell::new(None),
            probe_hit: RefCell::new(None),
            probe_var_provenance: RefCell::new(None),
            call_candidate_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Memoized `NameContext::resolve_function_candidates` - see `call_
    /// candidate_cache`'s own doc comment for why this exists and why a
    /// per-callee-span key is safe (one `Checker` per file, so a `Span`
    /// never collides across two different call sites' own callees).
    fn resolve_function_candidates_cached(
        &self,
        callee_span: Span,
        name: &str,
    ) -> (String, Option<String>) {
        if let Some(cached) = self.call_candidate_cache.borrow().get(&callee_span) {
            return cached.clone();
        }
        let resolved = self.name_ctx.resolve_function_candidates(name);
        self.call_candidate_cache
            .borrow_mut()
            .insert(callee_span, resolved.clone());
        resolved
    }

    /// Sets the `$this` type a Pest `it`/`test`/`describe`/`beforeEach`/
    /// `afterEach`/`beforeAll`/`afterAll` closure argument is seeded with -
    /// a setter rather than a `Checker::new` parameter, so call sites that
    /// don't care about Pest don't need to change.
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

    /// Seeds the whole-project merge of every file's own `property_
    /// assign_facts`, computed by `Project`'s own collect pass before the
    /// real, diagnostic-producing pass runs - see `cross_file_property_
    /// assign_facts`'s own doc comment.
    pub fn set_cross_file_property_assign_facts(
        &mut self,
        facts: HashMap<String, HashMap<String, Vec<Type>>>,
    ) {
        self.cross_file_property_assign_facts = Some(facts);
    }

    /// This run's own `property_assign_facts` - the collect half of the
    /// two-pass architecture reads this back out of a finished `Checker`
    /// (one per file) to build the project-wide merge every other file's
    /// own real pass then consumes via `set_cross_file_property_assign_
    /// facts`.
    pub fn take_property_assign_facts(&mut self) -> HashMap<String, HashMap<String, Vec<Type>>> {
        std::mem::take(&mut self.property_assign_facts)
    }

    /// The declaration-site counterpart of the read-site probe hook inline
    /// in `ExprKind::Variable` - stashes `ty` in `probe_hit` when `span` is
    /// exactly the LSP's hover-type probe target (`probe_span`). Called at
    /// every real local-variable binding site (`Locals::declare` call
    /// sites: parameter-declare loops, `foreach`/`catch`/`static`/`global`
    /// binds, destructuring-leaf binds), so hovering a variable at its own
    /// declaration - not just a later read - shows an inferred type too.
    pub(crate) fn probe_hit_at(&self, span: Span, ty: &Type) {
        if self.probe_span.get() == Some(span) {
            *self.probe_hit.borrow_mut() = Some(ty.clone());
        }
    }

    /// The plain-message path every call site that needs nothing beyond
    /// `identifier` goes through - a thin wrapper over `DiagnosticBuilder`
    /// so there's still one place a `Diagnostic` is assembled. `identifier`
    /// is `&str` here, not `impl Into<String>` like `DiagnosticBuilder::new`
    /// takes it, so call sites can pass a `&'static str` literal without an
    /// extra `.to_string()`.
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

    /// An object used somewhere real PHP implicitly converts it to a
    /// string - an explicit `(string)` cast, `echo`, and `print` all do
    /// this identically at runtime, invoking `__toString()` - triggers the
    /// same `@internal`/`@deprecated` restricted-usage checks an explicit
    /// `->__toString()` call would, mirroring real PHPStan's
    /// `RestrictedUsageOfDeprecatedStringCastRule` and its internal-tag
    /// equivalent (both run through `PrintHandler`/`EchoRule`'s
    /// `ImplicitToStringCallHelper`). Uses the same "first Object member"
    /// representative-class extraction `ExprKind::MethodCall`'s own
    /// `recv_class` uses for a Union/Intersection operand.
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
        // `NameContext::build` itself does) purely for the file-wide,
        // position-independent questions below - each already scans the
        // entire program regardless of where something appears, so
        // there's no one "current position" they need. Deliberately NOT
        // reused for `self.name_ctx` below, which has to be positional
        // instead (see that loop's own comment).
        let whole_file_ctx = NameContext::build(program);
        self.check_use_function_imports(program);
        self.check_use_const_imports(program);
        self.check_use_class_imports(program);
        self.check_used_names(program);
        let (rebind_targets, rebind_escape_all) =
            unused::scan_program_for_rebind_targets(program, &whole_file_ctx);
        self.rebind_target_classes = rebind_targets;
        self.rebind_escape_all = rebind_escape_all;
        // A file-local `uses(...)` call (no `->in(...)`) wins over
        // whatever `Project::check_file` already set via
        // `set_pest_this_type` from directory-scoped Pest.php config -
        // matches real Pest's resolution order (file-local checked
        // first).
        if let Some(local_ty) = pest_config::file_local_this_type(program, &whole_file_ctx) {
            self.pest_this_type = local_ty;
        }
        // Unlike `pest_this_type` above, hook properties are unioned, not
        // overridden - a property-name set has no "file-local wins"
        // precedence to apply.
        self.pest_hook_properties
            .extend(pest_config::file_local_hook_properties(program));

        let mut top_locals: Locals = Locals::default();
        top_locals.set_track(self.probe_span.get().is_some());
        let diagnostics_before = self.diagnostics.len();
        // `self.name_ctx` is tracked *positionally* here: real PHP only
        // allows a `namespace`/`use` statement at the top level of a file,
        // so a single left-to-right pass, mutating `self.name_ctx` in
        // place when one is seen, means that by the time this loop
        // reaches a `ClassDecl`/`FunctionDecl`/`EnumDecl`, `self.name_ctx`
        // reflects whichever `namespace` was active at that declaration's
        // own position - not merely the last one anywhere in the file,
        // which would wrongly resolve a declaration under an earlier
        // namespace against a later one.
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
                    self.check_require_tag_types(&fqn, c);
                    self.check_sealed_hierarchy(&fqn, c.span);
                    self.check_sealed_tag_types(&fqn, c.doc_comment.as_deref(), c.span, false);
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
                    // The class's own attributes, plus every property's/
                    // const's - method/param attributes are checked
                    // per-declaration inside `check_function` below
                    // instead, the one place that runs for every method
                    // regardless of how it's reached (a direct method
                    // here, or via `check_trait_methods`'s per-host loop
                    // for a trait).
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
                    // A property/const's missing-typehint completeness is
                    // about where it's textually declared, not which
                    // class(es) end up using it - so it's checked once
                    // here, regardless of trait-ness, unlike
                    // `check_function` below, which runs once per host for
                    // a trait's methods (a method's `$this`-dependent body
                    // needs a real host; a property/const's declared type
                    // never does).
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
                        // A class's private consts/methods/properties,
                        // checked once per class, not per method like
                        // check_function's per-body checks - "was this
                        // ever referenced" needs every one of the class's
                        // bodies scanned together, not one at a time.
                        self.check_unused_private_members(
                            &fqn,
                            c.kind,
                            &c.methods,
                            &c.properties,
                            &c.consts,
                        );
                        self.check_too_wide_property_types(&fqn, c);
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
                    self.check_sealed_tag_types(&fqn, e.doc_comment.as_deref(), e.span, true);
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
                    // An enum can't declare instance properties at all
                    // (see `ast::EnumDecl`'s doc comment), but its consts
                    // are as real a missing-typehint candidate as a
                    // class's.
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
