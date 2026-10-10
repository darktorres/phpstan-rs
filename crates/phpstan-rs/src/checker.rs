use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::{Cell, RefCell};
use syntax::name::{NameMap, NameSet};
use types::Name;

use crate::pest_config;
use crate::registry::Registry;
use diagnostics::{Diagnostic, DiagnosticBuilder};
use keys::{KeyData, LocalKey};
use std::sync::Arc;
use syntax::ast::*;
use syntax::name_resolution::NameContext;
use syntax::span::{LineIndex, Span};
use types::Type;

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
type LocalsMap = imbl::GenericOrdMap<LocalKey, Type, imbl::shared_ptr::DefaultSharedPtr>;

// `imbl::HashSet` mirrors `imbl::HashMap`'s O(1)-clone design (see
// `LocalsMap`'s doc comment) - not load-bearing here (a per-variable span
// set is small and never iterated at scale), but kept consistent with the
// persistent-map-throughout convention the rest of this module follows.
type ProvenanceSet =
    imbl::GenericHashSet<Span, rustc_hash::FxBuildHasher, imbl::shared_ptr::DefaultSharedPtr>;
type KeySet = imbl::GenericOrdSet<LocalKey, imbl::shared_ptr::DefaultSharedPtr>;
/// Where a record entry is: a span's start and end, and the class it was
/// checked in.
type RecordedAt = (u32, u32, String);
type ProvenanceMap =
    imbl::GenericOrdMap<LocalKey, ProvenanceSet, imbl::shared_ptr::DefaultSharedPtr>;

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
    (LocalKey, String),
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// Properties a `__clone()` body may already have reinitialized on this
// control-flow path (PHPStan's `CloneReinitializationExpr`): a second
// write to one is reported, so branches merge by union.
type ReinitializedPropertySet =
    imbl::GenericHashSet<String, rustc_hash::FxBuildHasher, imbl::shared_ptr::DefaultSharedPtr>;

// Variables a reference to the variable or to one of its elements may exist
// for on this control-flow path, so a call may write through it: branches
// merge by union.
type ExposedVariableSet = imbl::GenericHashSet<
    syntax::name::Name,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// Variables bound to one another by reference (`$r = &$x`), each key with the
// others of its group: a write to one is a write to all. Branches merge by
// union, since a link on either path may hold.
type ReferenceLinkMap = imbl::GenericHashMap<
    LocalKey,
    std::rc::Rc<Vec<LocalKey>>,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// `name -> (target_key, ExprId)` for a variable whose last assignment's
// RHS was itself a recognized narrowable condition shape (see
// `condition_alias_shape`/`condition_alias_targets` in `checker/
// narrowing.rs`), replayed through `apply_narrowing` when `name` is later
// checked bare (`if ($name)`) - correlates a *second* variable's narrowing
// to `name`'s own truthiness without any new type math: the stored ID points
// to the original AST node, re-run through the exact dispatch a direct
// `if (<rhsExpr>)` would use. `target_keys` (computed once at store time,
// mirroring `key_of`'s own string form - one entry per distinct
// subject a compound `&&`/`||` condition names, e.g. both `$isUserFound`
// and `$email` for `! $isUserFound || $email === null || $email === ''`)
// is what `Locals::declare` scans for to invalidate a stale alias whose
// *any* target was rebound, not just the alias variable itself. IDs are
// copyable, so every branch fork of `Locals` stays O(1); a given alias's ID
// never changes after it's first stored, only wholesale-replaced or purged,
// so ID equality is a correct, cheap "did both branches inherit this
// unchanged" merge test (see `merge_scopes`).
type ConditionAliasMap = imbl::GenericHashMap<
    LocalKey,
    (Vec<LocalKey>, ExprId),
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// `Discriminant::new(subject_key, value) -> [(implied_var, implied_type), ...]`
// - a same-branch discriminant-variable correlation: whenever an
// `if`'s own condition matches the *same* subject/value pair a prior
// branch already recorded a fact under, every `(var, type)` pair here
// replays into `locals` alongside that condition's own ordinary
// narrowing (see `checker/narrowing.rs`'s `record_branch_implied_facts`/
// `apply_equality_narrowing`'s own replay hook). `Rc`-wrapped `Vec`,
// so cloning
// `Locals` (extremely frequent - one clone per branch fork) stays O(1)
// even though a fact list itself isn't `Copy`. A recorded fact is a
// structural fact about the code at this exact discriminant, not
// per-branch state - `merge_scopes` unions both sides' entries rather
// than intersecting - but it only holds while the values it relates are
// the ones it saw: rebinding the discriminant's subject drops the whole
// entry, and rebinding an implied variable drops that pair
// (`Locals::purge_branch_implied_facts_depending_on`). A loop's next
// iteration rebinding its `foreach` variable is the common case.
type BranchImpliedFacts = std::rc::Rc<Vec<(LocalKey, Type)>>;

/// A narrowing subject (its `key_of`) compared against one value:
/// what branch-implied facts are recorded under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Discriminant {
    pub(crate) subject: LocalKey,
    pub(crate) value: Type,
}

impl Discriminant {
    pub(crate) fn new(subject: LocalKey, value: &Type) -> Discriminant {
        Discriminant {
            subject,
            value: value.clone(),
        }
    }
}

type BranchImpliedFactsMap = imbl::GenericHashMap<
    Discriminant,
    BranchImpliedFacts,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// `$qb = $this->createQueryBuilder(...)->...;` - the assignment's own RHS,
// stored verbatim so `resolve_query_builder_chain`'s backward walk
// (`checker/doctrine/query_builder.rs`) can see *through* a variable read
// to the real chain-rooted expression that built it, the same "replay the
// original expression" idea `condition_aliases` already uses for a
// narrowing condition, applied here to a symbolic `QueryBuilder` chain
// instead. Unlike `condition_aliases`, there's no separate "target" to
// invalidate on rebind - the alias variable *is* its own only key, and
// nothing else ever mutates the stored expression text once recorded, so
// `Locals::declare` purging this map's own entry for the rebound name is
// the whole invalidation story. Simpler value type than `ConditionAliasMap`
// for the same reason - just the expression, no accompanying target list.
type QueryBuilderAliasMap = imbl::GenericHashMap<
    LocalKey,
    ExprId,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// Container variable name -> a fact about every element of that
// container's own shape ("field name -> type"), not about one specific
// key - `key_of` (`checker/narrowing/predicates.rs`) has no
// "any/every index" form of its own, which is the one thing this map
// adds. Populated once, at a by-ref `foreach`'s own loop-exit
// (`checker/stmt/loops.rs`), by harvesting the accepted pass's own
// `key_of`-format entries for the loop's value variable -
// `bind_assignment_target` (`checker/assignment.rs`) already keeps those
// entries in sync on every assignment as a side effect of ordinary
// checking, so this is a pure harvest, not a second, independent
// analysis. Consumed by eagerly copying a container's entry onto a
// *freshly bound* variable's own `key_of` synthetic keys the
// moment it's bound from that container via a dynamic-keyed read (a
// `foreach` value, or a plain `$x = $container[$dyn];`) - the two
// pre-existing `key_of`-consulting Index-read fast paths in
// `checker/expr.rs` then pick the copied facts up with no further code.
// `Rc`-wrapped for the same O(1)-clone reason `BranchImpliedFactsMap`'s
// own `Vec` is. `merge_scopes` *intersects* (keeps only when both sides
// carry an identical entry), unlike `branch_implied_facts`'s union: this
// is a fact that can be invalidated on just one branch (a rebind, or a
// by-ref pass to an unrecognized function), so surviving on only one
// side of a merge must not resurrect it - either branch might be the one
// that actually ran.
type ContainerFieldFactsMap = imbl::GenericHashMap<
    LocalKey,
    std::rc::Rc<FxHashMap<String, Type>>,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

// `(container variable name, discriminant subject name, branch value) ->
// the array type this exact (container, branch) pair has accumulated so
// far`, independent of the *other* branch's own accumulation - fixes a
// real bug in `write_index_into`'s generic union handling
// (`checker/assignment.rs`): an append write applied there gets pushed
// onto *every* member of an already-diverged union uniformly, including
// a member that came from a *different* loop iteration's *other* branch
// (confirmed via a minimal repro: `if ($f) { $x[] = 'a'; } else { $x[] =
// new Obj(); }` inside a loop produces a genuinely-impossible
// `list<string|Obj>` member after only two passes, not because
// `merge_scopes` flattens anything - it doesn't, `Type::union` never
// merges two different `Array`/`Intersection` element types together -
// but because the *next* pass's own append, applied uniformly to
// *every* member of the prior pass's already-two-member union, widens
// the "wrong" branch's own member too). Populated *alongside* (never
// instead of) the ordinary, single-flat-type tracking every other
// container already gets - see `Checker::active_discriminant`'s own doc
// comment for the write-side trigger. Consulted only when checking a
// `return` statement whose declared type is a conditional type and the
// ordinary (still-flat, still-corrupted) tracked type would otherwise
// fail: reconstructing `Union([true-slot, false-slot])` from here and
// re-checking against the same declared type's own already-flattened
// fallback union passes through 100% pre-existing `accepts`
// per-member `Union`-vs-`Union` logic, no further mechanism needed.
// `merge_scopes` *unions* both sides (like `branch_implied_facts`, not
// like `container_field_facts`'s intersection): a `(container, subject,
// true)` entry is only ever written inside the `subject`-true branch, so
// it's naturally absent from the `subject`-false branch's own `Locals`
// fork - "present in only one side" here means "the other branch simply
// never had a reason to touch it," not "invalidated," so it must survive
// the merge, not be dropped.
type DiscriminantArrayFactsMap = imbl::GenericHashMap<
    (LocalKey, LocalKey, bool),
    Type,
    rustc_hash::FxBuildHasher,
    imbl::shared_ptr::DefaultSharedPtr,
>;

/// A generic `@template` name -> concrete `Type` substitution map (class-
/// hierarchy template binding, function-call argument-to-template
/// binding) - built once per call/hierarchy-walk and consumed immediately
/// by `Type::substitute_templates`/`Relation::infer_template_bindings`, never cloned
/// per-branch the way `Locals` is. A plain `std::collections::HashMap`,
/// since this use case has none of the "cloned once per branch" cost
/// profile that motivates `Locals`'s persistent-map backing.
pub(crate) type TemplateBindings = rustc_hash::FxHashMap<String, Type>;

/// A private property's every discovered assigned type, keyed by
/// (declaring class fqn, property name) - see `Checker::
/// property_assign_facts`' own doc comment. Writer classes and properties use
/// their stable Registry IDs.
pub(crate) type PropertyAssignFacts =
    FxHashMap<crate::registry::ClassId, FxHashMap<crate::registry::PropertyRef, Vec<Type>>>;

/// How long each record a checking pass appends to was when the pass
/// began (`Checker::pass_mark`), so a pass whose results are discarded -
/// a loop pass before its fixed point, a condition's quiet evaluation -
/// leaves none of them behind (`Checker::rewind_to`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct PassMark {
    diagnostics: usize,
    semantic_tokens: usize,
    throw_points: usize,
    explicit_throw_points: usize,
    return_points: usize,
    exit_points: usize,
    inlay_param_name_hints: usize,
    inlay_local_type_hints: usize,
}

/// Walks function/method bodies, inferring a best-effort type for each
/// expression and flagging argument/return type mismatches against the
/// registry. Deliberately conservative: `mixed` is overloaded here to
/// mean both PHP's real `mixed` type AND "we don't know" for anything this
/// prototype can't infer (property access, closures, dynamic calls, ...) -
/// so every check below treats an inferred `Mixed` as inconclusive and
/// skips reporting, rather than risk false positives.
///
/// `if`/`while` fork `locals` into per-branch copies (narrowed via
/// `apply_narrowing` on the condition) and merge them back at the join
/// point (`merge_scopes`), accounting for a branch that unconditionally
/// terminates (`Completion::terminates`) so a guard clause's narrowing survives
/// past the `if`. Everything else (`for`/`foreach`/`switch`/`try`) still
/// shares one mutable scope with no forking or narrowing - a real but
/// bounded gap, not a soundness issue, since no narrowing ever happens
/// there rather than an incorrect one.
///
/// Undefined-class/function/method reference checks resolve every name to
/// a fully-qualified one first via `name_resolution::NameContext` (tracking each
/// file's `namespace`/`use` statements) before any registry key is read or
/// written - two classes with the same short name in different namespaces
/// get distinct keys, the same way real PHP tells them apart.
/// The methods asked for, by class and then by method, as spelled.
type MethodMemo<'a> = FxHashMap<
    String,
    FxHashMap<String, Option<(syntax::name::Name, &'a crate::registry::FunctionReflection)>>,
>;

pub struct Checker<'a> {
    pub(crate) registry: &'a Registry,
    // The owning file for this check's node IDs and file-local payloads.
    // Standalone checkers may not have a parsed-file owner.
    pub(crate) syntax_tree: Option<&'a crate::project::SyntaxTree>,
    // Context files reparsed through `Project::ast_of` keep their owner in
    // the AST LRU. The checker holds a shared owner while it reads that tree.
    syntax_tree_owned: Option<Arc<crate::project::SyntaxTree>>,
    // Completion's augmented, per-request file owner. Its IDs share one
    // arena with the cloned last-good tree, receiver fragment and probes.
    syntax_file_owned: Option<Arc<syntax::ParsedFile>>,
    // Standalone parses may provide the source when there is no project
    // `SyntaxTree` owner.
    syntax_source: Option<&'a [u8]>,
    // Nodes synthesized while checking this one file live in a bump arena
    // owned by the checker. Their IDs occupy a reserved range disjoint from
    // realistic parsed-file IDs and are valid only until set_current_file
    // resets this scratch store or the checker is dropped.
    synthetic_nodes: synthetic::SyntheticNodeScratch,
    // The class the last `class_info` was asked for.
    class_memo: std::cell::RefCell<(String, Option<&'a crate::registry::ClassReflection>)>,
    // Each statement's resolved `@var` tags, by the address and length of
    // its doc text, for each class and method it was checked in.
    var_tag_memo: std::cell::RefCell<FxHashMap<(usize, usize), Vec<var_tag_types::VarTagMemo>>>,
    // What `lookup_method_with_owner` found for each class and method it
    // was asked about, as they were asked.
    method_memo: std::cell::RefCell<MethodMemo<'a>>,
    // What the checks are configured with - the defaults unless
    // `with_settings` supplies the project's.
    pub(crate) settings: &'a settings::CheckSettings,
    // Function and method bodies, found by a declaration's provenance -
    // none unless `with_bodies` supplies the project's.
    pub(crate) bodies: &'a dyn crate::registry::BodySource,
    // The project's SQL reflection service - an unconfigured one unless
    // `with_sql` supplies the project's.
    pub(crate) sql: &'a sql::context::SqlContext,
    pub(crate) diagnostics: Vec<Diagnostic>,
    // `textDocument/semanticTokens/full`'s own accumulator - every
    // `@deprecated`/`@internal` usage site flagged regardless of whether
    // `check_deprecated_usage`/the internal-usage extension is even
    // enabled, so a client gets push-highlighting for both independent
    // of this project's own opt-in diagnostic gates. See
    // `checker::semantic_tokens`'s own module doc comment for why each
    // `record_*_usage_token` entry point re-derives the identical
    // namespace/scope gating the corresponding diagnostic check applies,
    // rather than firing off the raw `is_internal`/`deprecated.is_some()`
    // booleans alone.
    pub(crate) semantic_tokens: Vec<semantic_tokens::UsageSemanticToken>,
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
    // The current function's own `@template`s, read from its doc - the
    // only source for one the registry doesn't index (an anonymous
    // class's method).
    pub(crate) current_method_templates: rustc_hash::FxHashMap<String, types::TemplateParam>,
    // `checker::restricted_symbol_usage`'s own `allow_in_class_with_
    // attributes`/`allow_except_in_class_with_attributes` matching - the
    // enclosing class's own attribute names, resolved (`ClassReflection::
    // attributes`, looked up by `current_class` at the same choke point
    // that field is set). Empty for a top-level function/no enclosing
    // class.
    pub(crate) current_class_attrs: Vec<Name>,
    // Same as `current_class_attrs`, for `allow_in_methods_with_
    // attributes`/`allow_except_in_methods_with_attributes` - the
    // function/method declaration's own attribute names, resolved,
    // read directly from the AST, no registry lookup needed. Deliberately
    // not reset for a nested closure/arrow-fn body (PHP has no attribute
    // syntax for either) - a restricted call textually inside a closure
    // literal defined inside an attributed method still counts as "in
    // that method" for this check, a disclosed simplification.
    pub(crate) current_method_attrs: Vec<Name>,
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
    // The enclosing function/method's own resolved `@throws` type, if
    // it declares one - set/restored at the same choke point as
    // `current_declared_return_type`, cleared (not inherited) for a
    // nested closure/arrow-fn body the same way that field's own
    // sibling `current_function_*` fields already are, since a real
    // `@throws` tag on the *enclosing* declaration says nothing about
    // what a closure declared inside it is allowed to throw - a
    // genuinely separate callable with its own contract. Consulted by
    // `match.unhandled`'s own real PHPStan `hasUnhandledMatchErrorThrowsTag`
    // exemption - a `match` with no handler for `UnhandledMatchError`
    // is not itself unsound when the enclosing function already
    // declares it may throw one.
    pub(crate) current_declared_throws_type: Option<Type>,
    // Every currently-enclosing `try` block's own resolved catch types,
    // flattened across however many `try`s the current statement is
    // lexically nested inside (pushed in `check_try` before its own
    // `check_block` call, truncated back after) - real PHPStan's own
    // `isUnhandledMatchErrorCaught` exemption for `match.unhandled`,
    // but computed eagerly at `catch` position rather than by walking
    // `TryCatchTypeVisitor`'s own attribute afterward, since this
    // checker's `match.unhandled` check runs inline during the same
    // single-pass body walk a real `try` block's protected region is
    // already checked under.
    pub(crate) current_catch_types: Vec<Type>,
    // One frame per enclosing `try` body in the current function: at each
    // point in it that can throw, what it can throw and the scope there.
    // A catch is entered from the merge of the points it can catch, and
    // is dead when there are none.
    pub(crate) try_throw_states: Vec<Vec<(Type, Locals)>>,
    // What each call checked inside a `try` body can throw, by the call's
    // span, from its resolved signature; `None` when it cannot throw.
    pub(crate) call_throw_types: rustc_hash::FxHashMap<Span, Option<Type>>,
    /// Each `$o[…]` in the function body being checked that calls an
    /// `ArrayAccess` method (`array_access`), for its purity check.
    pub(crate) implicit_calls: Vec<Span>,
    // Every real throw point recorded so far inside the enclosing
    // function/method/closure body - an explicit `throw` expression's
    // thrown-value type, or a call's resolved `@throws` type (a matching
    // `checker/throws/dynamic.rs` per-callee override tried first) -
    // recorded at each of `ExprKind::Call`/`New`/`MethodCall`/`StaticCall`'s
    // resolved-signature choke points. `void` contributes nothing,
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
    // opt-in via `ExtrasSettings::check_missing_checked_exceptions`.
    pub(crate) current_function_explicit_throw_points: Vec<(Type, Span)>,
    /// The `return`, `throw`, `break` and `continue` statements of the
    /// current function body whose path has left what the statement before
    /// them was checked in, and not been taken by a loop or a switch: a
    /// statement's own are the ones pushed while it was checked.
    pub(crate) current_function_exit_points: Vec<ExitPoint>,
    // Every real return point recorded so far inside the enclosing body -
    // `StmtKind::Return(Some(e))`'s value type, `StmtKind::Return(None)`'s
    // implicit `void` (real PHPStan pushes `VoidType`, not `Null`,
    // for a bare `return;`), and the falls-off-the-end path's implicit
    // `null` (only when the native return type allows it). Never
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
    // supplies one - see `ParameterReflection::out_ty`'s doc comment), set/restored
    // at the same choke point as `current_declared_return_type`.
    pub(crate) current_function_by_ref_params: syntax::name::NameMap<Type>,
    // Every by-ref parameter's own `locals`-tracked type, snapshotted at
    // each real execution-end point of the enclosing body (an explicit
    // `return`, with or without a value, and the implicit falls-off-the-
    // end point) - the by-ref-parameter analogue of `current_function_
    // return_points`, but keyed per-parameter and taken from `locals` at
    // each exit rather than from the return expression itself. This is
    // deliberately NOT "every value ever assigned anywhere in the body"
    // (a flat accumulator of that shape doesn't have the flow-sensitivity
    // either of this field's own two real consumers needs - real
    // PHPStan's `ParameterOutExecutionEndTypeRule`/`TooWideParameterOutTypeCheck`
    // both build a merged "final scope" over every real exit point, not a
    // union of every intermediate assignment): a `&$x` reassigned to a
    // different type before every real exit would otherwise still count
    // its now-overwritten earlier value, which no execution path can
    // ever actually observe by the time the function returns. Consumed
    // by `check_by_ref_execution_end`/`check_by_ref_too_wide`
    // (`checker/by_ref_params.rs`).
    pub(crate) current_function_by_ref_exit_snapshots: syntax::name::NameMap<Vec<Type>>,
    // The subset of `current_function_by_ref_params` that actually
    // declares a real `@param-out` tag (its own resolved out-type, not
    // just a native-type fallback) - real `ParameterOutExecutionEndTypeRule`
    // bails entirely (`getOutType() === null`) for a by-ref parameter
    // with no `@param-out` tag, unlike the tag-independent direct-
    // assignment rule (`check_by_ref_value`) which happily falls back to
    // the native type. Consulted by `check_by_ref_execution_end` only.
    pub(crate) current_function_by_ref_params_with_tag: syntax::name::NameMap<Type>,
    // Every name `record_by_ref_exit_snapshot` should snapshot at each
    // real execution-end point - a superset of `current_function_by_ref_
    // params`'s own keys (kept as a separate set, not folded into that
    // map, since that map's mere non-emptiness also gates real
    // `parameterByRef.type` checking, which a closure's own `use (&$x)`
    // capture must NOT trigger - real PHP has no "declared type" for a
    // captured variable the way a real by-ref parameter does). Populated
    // from `current_function_by_ref_params`'s own keys wherever that map
    // is (function_decl.rs), plus every closure `use (&$x)` capture name
    // (checker/expr.rs's `Closure` arm) - the latter is what actually
    // lets `mutated` (that same arm, computing what a `use (&$x)`
    // capture becomes in the declaring scope once the closure returns)
    // properly union every real exit point's own state instead of only
    // ever seeing `closure_locals`'s state at whichever statement the
    // body happened to fall off the end at - which, for the common
    // `if (cond) { $x[] = ...; return ...; } return ...;` guard-clause
    // shape, is never the branch that actually mutated `$x` at all.
    pub(crate) current_function_byref_snapshot_names: syntax::name::NameSet,
    // Set for the duration of one function/method/closure body to every
    // label a `goto` names anywhere in it, as the parser recorded them
    // (not scope-aware: a `goto` is only said to reach a label of its own
    // function, so a labeled statement after a terminating one is not
    // reported unreachable) - consulted by `check_block`'s
    // `deadCode.unreachable` check, which has no direct access to the
    // enclosing function's body to answer "could a `goto` reach this
    // labeled statement" itself.
    pub(crate) current_function_goto_targets: GotoState,
    /// Each assignment's value type (and native type, for a variable
    /// target) as last checked, by the assignment's span: a condition
    /// narrowed after the check binds these, not its right side re-read
    /// once the target already holds the new value.
    pub(crate) assigned_values: rustc_hash::FxHashMap<Span, (Type, Option<Type>)>,
    // How the body of the loop last checked completed, by its last pass.
    pub(crate) loop_body_completion: Completion,
    // The method call that is a whole statement, as its own check resolved
    // it: the statement's rules read it and do not infer the receiver or
    // find the method again.
    pub(crate) statement_method_call: Option<StatementMethodCall>,
    // Every variable assignment site in the current function body, for
    // `variable.implicitArray`; empty unless that check is enabled, and
    // collected when first asked.
    pub(crate) current_function_assigned_variables: strict_rules::LazyAssigned,
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
    // directory-scoped). Defaults to `mixed`, so any call site never
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
    pub(crate) pest_hook_properties: NameSet,
    // The lines of the file being checked, which a diagnostic's span is
    // turned into a line and column by.
    pub(crate) line_index: Arc<LineIndex>,
    // The file being checked, for resolving `include`/`require` paths.
    pub(crate) current_file: Option<std::path::PathBuf>,
    pub(crate) current_file_id: Option<crate::file_id::FileId>,
    // `current_file` as an absolute path (`Project::absolute_path`), what
    // PHP's `__FILE__` holds.
    pub(crate) current_file_absolute: Option<std::path::PathBuf>,
    // Paths the file tests with `file_exists()` and friends.
    pub(crate) include_guarded_paths: FxHashSet<String>,
    // `include_path` directories for the file being checked.
    pub(crate) include_path_dirs: Vec<String>,
    // What relative include paths resolve against first (see
    // `Project::working_dir`); `None` is the process's working directory.
    pub(crate) working_dir: Option<std::path::PathBuf>,
    // `includes::file_constant_initializers` for the file being checked.
    pub(crate) include_constant_initializers: FxHashMap<String, ExprId>,
    // The places an `include` or `require` looked for a file, in the order
    // looked at: what the file's report reads of the filesystem.
    pub(crate) include_probes: Vec<std::path::PathBuf>,
    // `if` statements already reported as the tail of an enum `if`/`elseif`
    // chain (`strict_idioms`).
    pub(crate) enum_chain_tails: FxHashSet<Span>,
    // `global_constants::constant_facts` for the file being checked.
    pub(crate) constant_facts: global_constants::ConstantFacts,
    // `$a[]` argument spans bound to a by-reference (or unknown)
    // parameter, recorded by `predefine_byref_or_unknown_args` before the
    // arguments are checked - the one read-looking position where PHP
    // accepts an empty dim, since it creates the element.
    pub(crate) by_ref_empty_dim_args: std::cell::RefCell<rustc_hash::FxHashSet<syntax::span::Span>>,
    /// The types by-reference arguments had before the call they are passed
    /// to replaced them with its parameters', by where they are.
    pub(crate) byref_before:
        std::cell::RefCell<rustc_hash::FxHashMap<syntax::span::Span, types::Type>>,
    // The type of every legal write to a private property, keyed by the
    // class whose scope performs it and the property's exact-case ID.
    pub(crate) property_assign_facts: PropertyAssignFacts,
    // Named-argument call sites against overridable methods - see
    // `checker::named_argument_renames`'s module doc comment.
    pub(crate) named_argument_call_facts: NamedArgumentCallFacts,
    // The classes whose private property types wait for every write to
    // them to be known (`property_type_too_wide`).
    pub(crate) property_type_sites: Vec<PropertyTypeSite>,
    // Whether `check_program` leaves `property_type_sites` for its caller
    // to judge against facts merged across files, instead of judging them
    // against this run's own facts when it ends.
    pub(crate) defer_property_type_checks: bool,
    // Every externally-registered `rule::Rule`, dispatched from
    // `check_expr`/`check_stmt`'s own thin wrappers - see `set_
    // external_rules`'s own doc comment. Empty (the default for every
    // one of this project's own ~31 `Checker::new` call sites, none of
    // which needs to know this mechanism exists at all) skips dispatch
    // entirely via the `is_empty()` guard at each of those two call
    // sites, so this costs nothing when unused.
    pub(crate) external_rules: &'a [Box<dyn crate::rule::Rule>],
    // One frame per currently-open loop or switch, collecting
    // the `locals` snapshot at every `break` reached inside it - a stack,
    // since loops/switches nest. `StmtKind::Foreach`/`While`/`For`/
    // `DoWhile` each fold every collected state into what flows past the
    // loop once its body is done being checked (a real `break` inside an
    // `if` was otherwise silently discarded from post-loop state
    // entirely - `Completion::terminates` correctly treats it as "doesn't
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
    // `(container variable name, index variable name)` pairs currently
    // "in range" - pushed by `check_for` when a `for` loop's own
    // condition is syntactically `$i < count($container)` (the loop
    // variable bounded by a fresh `count()` of the exact container being
    // indexed inside the body), popped on exit. Consulted by
    // `write_index_into` (`checker/assignment.rs`) so a dynamic-key
    // write `$container[$i] = ...;` inside such a loop can be recognized
    // as rewriting an existing element in place - never growing past the
    // end or skipping an index - so real PHP list-ness genuinely
    // survives it, unlike an arbitrary dynamic-key write (confirmed via
    // `dumpType()` against real PHPStan: `array<int, int>`, a plain,
    // unmarked source, stays `list<int>` through exactly this loop
    // shape, but not through `for ($i = 0; $i < 10; $i++)` or a `count()`
    // captured into a variable *before* the loop instead of re-evaluated
    // in the condition itself - both real PHPStan drops list-ness for
    // too, matching this mechanism's own narrow syntactic trigger). A
    // `Vec`, not a single `Option`, so nested `for` loops over
    // different containers don't clobber each other.
    pub(crate) in_range_index_writes: Vec<(String, String)>,
    // `(subject variable name, branch value)` for every `if`/`else`
    // branch currently being checked whose own condition is exactly a
    // bare boolean `Variable` (`if ($isActionSend) { ... } else { ...
    // }`) - pushed by `check_if` right before checking `then_branch`
    // (`true`) or the whole `else_branch` handling (`false`), popped
    // right after. Consulted by an array-append write
    // (`checker/assignment.rs`'s `ExprKind::Index` arm, append-only, a
    // bare `Variable` container) to additionally record that push under
    // `Locals::discriminant_array_facts`, keyed by which value of the
    // subject this exact push happened under - see that field's own doc
    // comment for why this exists and how it's consumed. A `Vec`, not a
    // single `Option`, so a nested `if` inside a branch (a different
    // subject) doesn't clobber the outer one - though only the
    // innermost, most specific entry is ever actually consulted (`.last()`).
    pub(crate) active_discriminant: Vec<(LocalKey, bool)>,
    // The keys of every scope of this file's check: see `keys::KeyTable`.
    keys: keys::KeyTableRef,
    // The variables `static $x = ...;` binds in the scope being checked. A
    // `static` local can carry a value left over from an earlier call - the
    // same "this single forward pass can't see a wider type a different
    // execution of the same code would" unsoundness `loop_depth` guards
    // against, just across invocations instead of iterations (real
    // PHPStan stays silent on this shape too).
    pub(crate) static_var_names: NameSet,
    // What of the scope being checked other code may write or run
    // (`shared_scope`), and what the closures it wrote may have left in the
    // variables they capture by reference.
    pub(crate) shared_scope: std::rc::Rc<shared_scope::LazyShared<'a>>,
    // Where the statement being checked starts, as the scope's escapes are
    // placed (`SharedScope::unescaped_at`); `None` outside one.
    pub(crate) stmt_at: std::cell::Cell<Option<u32>>,
    // What each closure or arrow function written in the file took of the
    // variables around it, by value, where it was written: the object each
    // held (`note_captures`).
    pub(crate) closure_captures: rustc_hash::FxHashMap<
        syntax::ast::ExprId,
        rustc_hash::FxHashMap<syntax::name::Name, crate::registry::Holder>,
    >,
    pub(crate) captured_types: rustc_hash::FxHashMap<syntax::name::Name, Type>,
    // For each closure of the scope capturing a variable by reference, by
    // the closure's offset: what it was entered with on this pass, and what
    // its next pass enters it with; and what each variable captured by
    // reference held after any statement of the scope
    // (`check_capturing_scope`).
    pub(crate) capture_entries: Vec<(u32, syntax::name::Name, Type)>,
    pub(crate) capture_widening: rustc_hash::FxHashMap<(u32, syntax::name::Name), Type>,
    pub(crate) capture_seen: rustc_hash::FxHashMap<syntax::name::Name, Type>,
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
    pub(crate) rebind_target_classes: NameSet,
    // Whether the file names anything a closure rebind needs (`bind`,
    // `bindTo`, `call`); when it does not, no walk of it finds one.
    pub(crate) closure_rebind_possible: bool,
    // Whether the file names a function that tests a path or sets the
    // include path, which is what its include guards are found by; true
    // until said otherwise.
    pub(crate) include_guards_possible: bool,
    /// The bits of `FileFacts::mentions` of the file being checked; every
    /// bit is set until said otherwise.
    pub(crate) file_mentions: u32,
    // Whether the file builds a function-like or class-like inside a body;
    // when it does not, no walk into a body can find one.
    pub(crate) file_has_nested_function_like: bool,
    #[cfg(debug_assertions)]
    pub(crate) function_like_declarations_checked: u32,
    // Where the file first spells each syntax its tree does not keep
    // (`syntax_versions`).
    pub(crate) syntax_marks: syntax::SyntaxMarks,
    pub(crate) rebind_escape_all: bool,
    // The LSP's one-off "what type is this instance receiver" probe.
    // `probe_span` is the exact `Span` of the receiver `ExprNode` the caller
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
    pub(crate) probe_var_provenance: RefCell<Option<FxHashSet<Span>>>,
    // The LSP's `textDocument/inlayHint` collection mode. `false` (every
    // ordinary check) is a complete no-op; `Checker::checker_for` is the
    // only place that sets it. Populated by `maybe_record_param_name_hints`
    // (call-argument parameter names) and `check_stmt`'s Expression arm
    // (inferred local-variable types), then drained by `lsp::inlay_hints`.
    pub(crate) collect_inlay_hints: bool,
    pub(crate) inlay_param_name_hints: Vec<(Span, Name)>,
    pub(crate) inlay_local_type_hints: Vec<(Span, Type)>,
    // `textDocument/references`/`textDocument/rename`'s Tier-2 (member
    // reference) collection mode - see `project::use_sites`'s own module
    // doc comment for the two-tier split. `false` (every ordinary check,
    // including every diagnostic-only batch run) is a complete no-op.
    // `RefCell<HashSet<...>>`, not a plain `Vec` field: populated from
    // both `check_expr` (`&mut self`) and `infer_expr`/
    // `infer_closure_return_with_param_overrides` (`&self`) without
    // threading a new parameter through either's call graph - the same
    // interior-mutability reasoning `probe_hit` already uses. A `HashSet`, not a `Vec`: the same real site can
    // legitimately be visited more than once (e.g. a condition
    // re-inferred via `infer_expr` for flow-narrowing after `check_expr`'s
    // own primary arm already recorded the identical span) - every
    // insert is a structural no-op on a repeat `(span, owner, member)`
    // triple, regardless of how many call paths independently re-derive
    // the same site.
    pub(crate) collect_member_refs: bool,
    #[allow(clippy::type_complexity)]
    member_ref_sites: RefCell<FxHashSet<(Span, String, String)>>,
    // Dead-enum-case detection's escape hatch: every real `Enum` class this file's
    // check pass saw a `::cases()` call or a dynamic (`{dynamic}`)
    // member access against - either one means "some case of this enum
    // is used in a way this project can't attribute to a specific case
    // name," so `project::whole_program::dead_enum_cases` treats the
    // whole enum as conservatively used rather than risk a false
    // "unused" report. Populated only when `collect_member_refs` is on
    // (the same Tier-2 pass `member_ref_sites` rides along with) -
    // `record_enum_escape` self-gates on it internally, mirroring
    // `record_member_ref`'s own pattern, so every plain call site below
    // stays unconditional.
    escaped_enums: RefCell<FxHashSet<String>>,
    // Dead-member detection's escape hatch: every class (and each of its own
    // ancestors/real subclasses - see `Checker::escape_targets_for_
    // receiver_type`) this file's check pass saw reached through a
    // *genuinely* dynamic member access (`$obj->{$expr}`/`$obj->$name`,
    // the name itself not a literal string - see `check_expr`'s own
    // `DynamicPropertyAccess`/`DynamicMethodCall` arms) whose receiver
    // resolved to a known type. `project::whole_program::dead_members`
    // treats every member of an escaped class as conservatively used,
    // the same "can't attribute to one specific member, don't flag any
    // of them" discipline `escaped_enums` above already uses for enum
    // cases. Populated only when `collect_member_refs` is on -
    // `record_member_class_escape` self-gates on it internally,
    // mirroring `record_enum_escape`'s own pattern.
    escaped_member_classes: RefCell<FxHashSet<String>>,
    // Disjunctive-syllogism narrowing: a stack, not a `Locals` field,
    // since it's scoped to the *traversal position* (currently inside
    // an outer `if ($a || $b)`'s own true branch, at any nesting depth)
    // rather than to any variable's tracked value. `check_if` pushes
    // `(l, r)` before checking a `Binary(Or, l, r)` condition's own
    // then-branch and pops it after, regardless of how deep a nested
    // `if` inside that branch sits; `check_if`'s own false-branch
    // handling consults the top of this stack. The pair contains IDs into
    // the checker's retained file tree, not cloned syntax nodes -
    // see `checker/narrowing.rs`'s `disjunctive_syllogism_atom_key` for
    // exactly which "is this the same atom" comparison is sound enough
    // to act on.
    pub(crate) pending_or_disjunctions: Vec<(ExprId, ExprId)>,
    // `--dump-types`: every tracked variable's type (and native type) after
    // each statement, keyed by the statement's start and the class it was
    // checked in, so a later loop pass or trait host overwrites an earlier
    // one. `None` unless enabled.
    type_dump: Option<std::collections::BTreeMap<(u32, u32, String), String>>,
    // The expression-type record: each expression's inferred and native
    // type, keyed by its span and the class it was checked in, so a later
    // pass overwrites an earlier one and the final pass's stays. `None`
    // unless kept.
    expr_types: Option<std::collections::BTreeMap<RecordedAt, (Type, Type)>>,
    // The PHPDoc resolution record (`record_doc_resolution`), one line for
    // each distinct resolution. `None` unless kept.
    doc_resolutions: RefCell<Option<std::collections::BTreeSet<String>>>,
    // The types `infer_expr` found while one outermost `check_expr` runs,
    // by the file-local expression id and scope version, in a slot for
    // each node (`infer_cache.rs`): checking a
    // nested expression infers each operand again at every level above
    // it, which without this is quadratic in the nesting depth. `None`
    // outside an expression check, where the checker's own state may
    // move on between two inferences.
    infer_cache: RefCell<Option<InferCache>>,
    // The emptied cache of the last outermost expression check, kept for the
    // next one so its table is not built and grown again.
    infer_spare: RefCell<InferCache>,
}

/// A syntax owner handle that can be held while the checker mutates its
/// diagnostics and flow state. Project trees are borrowed for `'a`; trees
/// reparsed from the context cache are kept alive by a cheap `Arc` clone.
pub(crate) enum SyntaxTreeOwner<'a> {
    Borrowed(&'a crate::project::SyntaxTree),
    Shared(Arc<crate::project::SyntaxTree>),
    Parsed(Arc<syntax::ParsedFile>),
}

impl SyntaxTreeOwner<'_> {
    pub(crate) fn arenas(&self) -> &syntax::ast::NodeArenas {
        match self {
            Self::Borrowed(tree) => tree.arenas(),
            Self::Shared(tree) => tree.arenas(),
            Self::Parsed(file) => &file.arenas,
        }
    }

    pub(crate) fn statements(&self) -> &[syntax::ast::StmtId] {
        match self {
            Self::Borrowed(tree) => tree.statements(),
            Self::Shared(tree) => tree.statements(),
            Self::Parsed(file) => file.statements(),
        }
    }
}

use infer_cache::InferCache;

/// A question `infer_cache` answers.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InferQuestion {
    // `Checker::infer_expr`.
    Type,
    // `Checker::chain_link_type`.
    ChainLink,
}

mod access_attributes;
mod synthetic;
/// A method call statement's callee as the check of the call resolved it.
pub(crate) struct StatementMethodCall {
    pub(crate) id: syntax::ast::ExprId,
    /// The class that declares the method.
    pub(crate) owner: syntax::name::Name,
    /// The receiver's type before the call.
    pub(crate) receiver: Type,
}

mod completion;
mod labels;
pub(crate) use completion::{
    BREAK_HERE, BlockCompletion, CONTINUE_HERE, Completion, ExitKind, ExitPoint, GotoState, Reach,
};
pub(crate) use labels::{CalleeLabel, MemberKind, MemberLabel};
mod array_access;
mod infer_cache;
mod keys;
mod locals;
pub(crate) use locals::Locals;
mod assignment;
mod by_ref_params;
mod call_effects;
mod callable_values;
mod calls;
mod captures;
mod castable_arguments;
mod coercion;
mod comparisons;
pub mod constant_resolver;
mod control_flow;
mod current_time;
mod date_interval;
mod date_time;
mod deprecated_usage;
pub(crate) mod doctrine;
mod duplicates;
mod expr;
mod function_decl;
mod function_like_declarations;
mod generics;
pub(crate) mod global_constants;
mod imports;
mod impossible_instanceof;
pub(crate) mod includes;
mod ini_directives;
mod instantiation;
pub mod rule_level_helper;
pub mod settings;
mod strict_idioms;
mod strict_rules;
mod syntax_versions;
pub(crate) use imports::INTERNAL_CLASS_PREFIXES;
mod internal_rules;
mod internal_usage;
mod keywords;
mod magic_numbers;
mod missing_typehint;
mod named_argument_renames;
mod narrowing;
mod native_type_hints;
mod noop;
mod offset_assignment;
mod oop;
mod operators;
mod phpdoc_type_compat;
mod phpdoc_unresolvable;
pub(crate) use phpdoc_unresolvable::var_tag_nodes;
pub(crate) mod phpstan_api;
mod pipe_operator;
mod predefined_constants;
mod preg_pattern_validity;
mod profiling;
mod property_assignment;
mod property_default_types;
mod property_type_too_wide;
mod psr3;
mod resolve;
mod restricted_symbol_usage;
mod return_type_too_wide;
mod risky_tests;
pub(crate) mod semantic_tokens;
mod shared_scope;
pub(crate) mod sql_reflection;
mod static_call_context;
mod stmt;
pub(crate) mod suggest;
mod throws;
mod type_declarations;
mod uninitialized;
mod uninitialized_dataflow;
pub(crate) mod unused;
mod var_tag_types;
mod visibility;
mod whitespace;
mod xml_name;
mod xml_wellformedness;

pub(crate) use assignment::{literal_shape_key, literal_string_key, unassigned_variable_type};
pub(crate) use calls::{CheckArgsCall, resolve_call_args};
pub(crate) use control_flow::{
    LoopZeroIterations, generator_missing_return_type, generator_template_param,
    is_constant_condition_family_identifier, is_truthy_literal, merge_scopes,
    type_allows_generator_return,
};
pub(crate) use duplicates::class_kind_identifier;
pub(crate) use expr::{PregCall, preg_call, preg_result_outcome, preg_result_type};
pub(crate) use internal_usage::class_type_word;
pub(crate) use missing_typehint::{ITERABLE_GENERIC_CLASS_NAMES, class_kind_word};
pub(crate) use named_argument_renames::NamedArgumentCallFacts;
pub(crate) use narrowing::{
    count_call_operand, narrowing_type_for_predicate, propagate_container_field_facts,
    purge_stale_narrowing_for, purge_stale_narrowing_for_var,
};
pub(crate) use oop::{
    ATTRIBUTE_TARGET_CLASS, ATTRIBUTE_TARGET_CLASS_CONSTANT, ATTRIBUTE_TARGET_FUNCTION,
    ATTRIBUTE_TARGET_METHOD, ATTRIBUTE_TARGET_PARAMETER, ATTRIBUTE_TARGET_PROPERTY,
};
pub(crate) use operators::binop_sigil;
pub(crate) use property_type_too_wide::{PropertyTypeSite, check_too_wide_property_types};
pub(crate) use semantic_tokens::UsageTokenKind;
#[cfg(test)]
pub(crate) use types::generalize_by_key_set;

pub(crate) use visibility::MemberVisibilityCheck;

impl<'a> Checker<'a> {
    /// Checks against the project's SQL reflection service.
    pub fn with_sql(mut self, sql: &'a sql::context::SqlContext) -> Self {
        self.sql = sql;
        self
    }

    pub fn with_settings(mut self, settings: &'a settings::CheckSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn with_bodies(mut self, bodies: &'a dyn crate::registry::BodySource) -> Self {
        self.bodies = bodies;
        self
    }

    /// Gives the checker the file that owns the syntax node IDs it reads.
    pub(crate) fn with_syntax_tree(mut self, syntax_tree: &'a crate::project::SyntaxTree) -> Self {
        self.syntax_tree = Some(syntax_tree);
        self
    }

    pub(crate) fn with_owned_syntax_tree(
        mut self,
        syntax_tree: Arc<crate::project::SyntaxTree>,
    ) -> Self {
        self.syntax_tree_owned = Some(syntax_tree);
        self
    }

    /// Gives the checker the parsed file that owns the syntax node IDs it reads.
    pub fn with_owned_parsed_file(mut self, parsed_file: Arc<syntax::ParsedFile>) -> Self {
        self.syntax_file_owned = Some(parsed_file);
        self
    }

    /// The owner of the file-local IDs used by this checker's syntax nodes.
    pub(crate) fn syntax_arenas(&self) -> Option<&syntax::ast::NodeArenas> {
        self.syntax_tree
            .or(self.syntax_tree_owned.as_deref())
            .map(crate::project::SyntaxTree::arenas)
            .or_else(|| self.syntax_file_owned.as_deref().map(|file| &file.arenas))
    }

    /// Reads source-backed syntax payloads through the tree that owns their
    /// file-local spans.
    pub(crate) fn syntax_source_slice(&self, span: syntax::span::Span) -> Option<&[u8]> {
        self.syntax_tree
            .or(self.syntax_tree_owned.as_deref())
            .and_then(|tree| tree.source_slice(span))
            .or_else(|| {
                self.syntax_file_owned
                    .as_deref()
                    .and_then(|file| file.source_slice(span))
            })
            .or_else(|| {
                self.syntax_source
                    .and_then(|source| source.get(span.start as usize..span.end as usize))
            })
    }

    /// Resolves a doc range through the tree that owns its source bytes.
    pub(crate) fn syntax_doc_comment_text<'s>(
        &'s self,
        ranges: &'s syntax::ast::DocCommentRanges,
    ) -> Option<std::borrow::Cow<'s, str>> {
        self.syntax_arenas()
            .and_then(|arenas| arenas.doc_comment_text(ranges))
            .or_else(|| self.syntax_source.and_then(|source| ranges.text(source)))
    }

    /// Resolves a literal payload through its owning parsed file.
    pub(crate) fn syntax_string_bytes<'s>(
        &'s self,
        payload: &'s syntax::ast::StringPayload,
    ) -> Option<&'s [u8]> {
        self.syntax_arenas()?.string_bytes(payload)
    }

    /// Resolves a literal payload as UTF-8 text when its bytes are valid.
    pub(crate) fn syntax_string_text<'s>(
        &'s self,
        payload: &'s syntax::ast::StringPayload,
    ) -> Option<&'s str> {
        std::str::from_utf8(self.syntax_string_bytes(payload)?).ok()
    }

    /// Clones the source owner's shared handle so a caller can keep borrowed
    /// payload slices across mutable checker operations.
    pub(crate) fn syntax_source_owner(&self) -> Option<Arc<[u8]>> {
        self.syntax_tree
            .or(self.syntax_tree_owned.as_deref())
            .map(|tree| Arc::clone(&tree.arenas().source))
            .or_else(|| {
                self.syntax_file_owned
                    .as_ref()
                    .map(|file| Arc::clone(&file.source))
            })
    }

    /// Makes the scope key of an expression through the file that owns its
    /// ID. Keeping this lookup on `Checker` prevents callers from treating
    /// nested expression IDs as if they were recursive node values.
    pub(crate) fn key_of_expr(
        &self,
        locals: &Locals,
        expr: &syntax::ast::ExprNode,
    ) -> Option<LocalKey> {
        locals.key_of(self.syntax_arenas()?, expr)
    }

    /// Finds an existing scope key through the expression's file owner.
    pub(crate) fn find_expr_key(
        &self,
        locals: &Locals,
        expr: &syntax::ast::ExprNode,
    ) -> Option<LocalKey> {
        locals.find_key(self.syntax_arenas()?, expr)
    }

    /// Clones only the owner handle, not its arenas, so callers can retain
    /// arena-backed slices across mutable checker operations.
    pub(crate) fn syntax_tree_owner(&self) -> Option<SyntaxTreeOwner<'a>> {
        self.syntax_tree
            .map(SyntaxTreeOwner::Borrowed)
            .or_else(|| {
                self.syntax_tree_owned
                    .as_ref()
                    .map(Arc::clone)
                    .map(SyntaxTreeOwner::Shared)
            })
            .or_else(|| {
                self.syntax_file_owned
                    .as_ref()
                    .map(Arc::clone)
                    .map(SyntaxTreeOwner::Parsed)
            })
    }

    /// The lines of the file to check, for its diagnostics' lines and
    /// columns.
    pub fn with_line_index(mut self, line_index: &Arc<LineIndex>) -> Self {
        self.line_index = Arc::clone(line_index);
        self
    }

    /// The type relations over the registry's classes, as lenient as the
    /// configured settings say.
    pub(crate) fn relation(&self) -> types::Relation<'a> {
        self.settings.rule_levels.relation(self.registry)
    }

    /// The level a rule reports `ty` at for the parts `fails` holds for
    /// (`RuleLevelHelper::failing_level`).
    pub(crate) fn failing_level(
        &self,
        ty: &Type,
        base: u8,
        fails: impl Fn(&Type) -> bool,
    ) -> Option<u8> {
        self.settings.rule_levels.failing_level(ty, base, fails)
    }

    pub(crate) fn accepts(&self, accepting: &Type, accepted: &Type) -> types::AcceptsResult {
        self.relation().accepts(accepting, accepted)
    }

    /// The lowest configured level at which a rule rejects `accepted`
    /// where `accepting` is declared (`Relation::rejected_from`); `None`
    /// when every level accepts it.
    pub(crate) fn rejected_from(&self, accepting: &Type, accepted: &Type) -> Option<u8> {
        self.relation().rejected_from(accepting, accepted)
    }

    /// The registry, for its Doctrine entities, while the Doctrine
    /// extension is active.
    pub(crate) fn doctrine_entities(&self) -> Option<&'a Registry> {
        self.settings.doctrine.then_some(self.registry)
    }

    /// `Registry::class_by_name`, remembered for the next ask for the
    /// same name: a class is asked for by every check of it, one after
    /// another.
    pub(crate) fn class_info(&self, name: &str) -> Option<&'a crate::registry::ClassReflection> {
        let mut memo = self.class_memo.borrow_mut();
        if memo.0 == name && (memo.1.is_some() || !memo.0.is_empty()) {
            return memo.1;
        }
        let registry: &'a Registry = self.registry;
        let info = registry.class_by_name(name);
        memo.0.clear();
        memo.0.push_str(name);
        memo.1 = info;
        info
    }

    /// `registry.lookup_method_with_owner`, remembered for each class and
    /// method asked about: the registry does not change while a file is
    /// checked, and the checks of it ask about the same few hundred
    /// methods, 800,000 times in the scoped Piwigo run for 13,000 of them.
    pub(crate) fn lookup_method_with_owner(
        &self,
        class: &str,
        method: &str,
    ) -> Option<(syntax::name::Name, &'a crate::registry::FunctionReflection)> {
        if let Some(found) = self
            .method_memo
            .borrow()
            .get(class)
            .and_then(|methods| methods.get(method))
        {
            return found.clone();
        }
        let registry: &'a Registry = self.registry;
        let found = registry.lookup_method_with_owner(class, method);
        self.method_memo
            .borrow_mut()
            .entry(class.to_string())
            .or_default()
            .insert(method.to_string(), found.clone());
        found
    }

    pub fn new(registry: &'a Registry) -> Self {
        static NO_SQL: std::sync::LazyLock<sql::context::SqlContext> =
            std::sync::LazyLock::new(Default::default);
        static DEFAULT_SETTINGS: std::sync::LazyLock<settings::CheckSettings> =
            std::sync::LazyLock::new(Default::default);
        Checker {
            registry,
            syntax_tree: None,
            syntax_tree_owned: None,
            syntax_file_owned: None,
            syntax_source: None,
            synthetic_nodes: synthetic::SyntheticNodeScratch::default(),
            class_memo: std::cell::RefCell::new((String::new(), None)),
            var_tag_memo: std::cell::RefCell::new(FxHashMap::default()),
            method_memo: std::cell::RefCell::new(FxHashMap::default()),
            settings: &DEFAULT_SETTINGS,
            bodies: &crate::registry::NoBodies,
            sql: &NO_SQL,
            diagnostics: Vec::new(),
            name_ctx: NameContext::default(),
            current_class: None,
            current_method: None,
            current_method_templates: rustc_hash::FxHashMap::default(),
            current_class_attrs: Vec::new(),
            current_method_attrs: Vec::new(),
            current_trait: None,
            current_scope_deprecated: false,
            saw_dynamic_var_construct: false,
            current_function_is_generator: false,
            current_declared_return_type: None,
            current_declared_throws_type: None,
            current_catch_types: Vec::new(),
            current_function_throw_points: Vec::new(),
            try_throw_states: Vec::new(),
            call_throw_types: rustc_hash::FxHashMap::default(),
            implicit_calls: Vec::new(),
            current_function_has_unannotated_call: false,
            current_function_explicit_throw_points: Vec::new(),
            current_function_exit_points: Vec::new(),
            current_function_return_points: Vec::new(),
            current_function_saw_explicit_return: false,
            current_function_by_ref_params: NameMap::default(),
            current_function_by_ref_exit_snapshots: NameMap::default(),
            current_function_by_ref_params_with_tag: NameMap::default(),
            current_function_byref_snapshot_names: NameSet::default(),
            current_function_goto_targets: GotoState::default(),
            assigned_values: rustc_hash::FxHashMap::default(),
            loop_body_completion: Completion::FALLS_THROUGH,
            statement_method_call: None,
            current_function_assigned_variables: strict_rules::LazyAssigned::default(),
            in_bare_statement_position: false,
            in_condition_position: false,
            in_exempt_last_condition: false,
            pest_this_type: Type::mixed(),
            pest_hook_properties: NameSet::default(),
            line_index: Arc::default(),
            current_file: None,
            current_file_id: None,
            current_file_absolute: None,
            include_guarded_paths: FxHashSet::default(),
            include_path_dirs: Vec::new(),
            working_dir: None,
            include_constant_initializers: FxHashMap::default(),
            include_probes: Vec::new(),
            enum_chain_tails: FxHashSet::default(),
            constant_facts: Default::default(),
            by_ref_empty_dim_args: Default::default(),
            byref_before: Default::default(),
            property_assign_facts: FxHashMap::default(),
            named_argument_call_facts: FxHashMap::default(),
            property_type_sites: Vec::new(),
            defer_property_type_checks: false,
            external_rules: &[],
            loop_break_states: Vec::new(),
            loop_continue_states: Vec::new(),
            loop_depth: 0,
            in_range_index_writes: Vec::new(),
            active_discriminant: Vec::new(),
            keys: keys::KeyTableRef::default(),
            static_var_names: NameSet::default(),
            shared_scope: Default::default(),
            stmt_at: Default::default(),
            closure_captures: Default::default(),
            captured_types: Default::default(),
            capture_entries: Default::default(),
            capture_widening: Default::default(),
            capture_seen: Default::default(),
            rebind_target_classes: NameSet::default(),
            closure_rebind_possible: true,
            include_guards_possible: true,
            file_mentions: u32::MAX,
            file_has_nested_function_like: true,
            #[cfg(debug_assertions)]
            function_like_declarations_checked: 0,
            syntax_marks: syntax::SyntaxMarks::default(),
            rebind_escape_all: false,
            probe_span: Cell::new(None),
            probe_hit: RefCell::new(None),
            probe_var_provenance: RefCell::new(None),
            collect_inlay_hints: false,
            inlay_param_name_hints: Vec::new(),
            inlay_local_type_hints: Vec::new(),
            collect_member_refs: false,
            member_ref_sites: RefCell::new(FxHashSet::default()),
            escaped_enums: RefCell::new(FxHashSet::default()),
            escaped_member_classes: RefCell::new(FxHashSet::default()),
            semantic_tokens: Vec::new(),
            pending_or_disjunctions: Vec::new(),
            type_dump: None,
            expr_types: None,
            doc_resolutions: RefCell::new(None),
            infer_cache: RefCell::new(None),
            infer_spare: RefCell::new(InferCache::default()),
        }
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
    /// finds itself, rather than one overriding the other. Takes owned
    /// `String`s (`PestConfig::resolve_hook_properties`'s own return
    /// type is unchanged) and interns each one into the shared `Name`
    /// table on the way in, matching the field's own interned
    /// representation.
    pub fn set_pest_hook_properties(&mut self, props: FxHashSet<String>) {
        self.pest_hook_properties = props.into_iter().map(|p| Name::from(&p)).collect();
    }

    /// See `current_file`.
    pub fn set_current_file(&mut self, path: &std::path::Path) {
        self.synthetic_nodes.clear();
        self.current_file = Some(path.to_path_buf());
    }

    /// Allocates a checker-only expression in this checker's current-file
    /// scratch arena. The returned reference and its synthetic ID must not
    /// outlive the current file check.
    pub(crate) fn synthetic_expr_node(
        &self,
        kind: syntax::ast::ExprKind,
        span: syntax::span::Span,
    ) -> &synthetic::SyntheticExpr {
        self.synthetic_nodes.alloc_expr(kind, span)
    }

    pub(crate) fn synthetic_expr_arena(&self) -> std::rc::Rc<bumpalo::Bump> {
        // A local owner handle lets a call borrow its synthetic node while
        // mutably checking it; the arena itself remains owned by this file's
        // checker scratch store.
        self.synthetic_nodes.arena_handle()
    }

    pub(crate) fn synthetic_expr_in<'s>(
        &self,
        arena: &'s bumpalo::Bump,
        kind: syntax::ast::ExprKind,
        span: syntax::span::Span,
    ) -> &'s synthetic::SyntheticExpr {
        self.synthetic_nodes.alloc_expr_in(arena, kind, span)
    }

    pub(crate) fn is_synthetic_expr_id(&self, id: ExprId) -> bool {
        synthetic::SyntheticNodeScratch::is_synthetic_id(id)
    }

    pub(crate) fn expr_span(&self, id: ExprId) -> syntax::span::Span {
        if self.is_synthetic_expr_id(id) {
            return self
                .synthetic_nodes
                .span(id)
                .expect("synthetic expression ID has a scratch span");
        }
        self.syntax_tree_owner()
            .expect("expression span lookup retains its parsed file owner")
            .arenas()
            .expr_span(id)
            .expect("expression ID has a parallel span")
    }

    pub(crate) fn stmt_span(&self, id: StmtId) -> syntax::span::Span {
        self.syntax_tree_owner()
            .expect("statement span lookup retains its parsed file owner")
            .arenas()
            .stmt_span(id)
            .expect("statement ID has a parallel span")
    }

    pub(crate) fn set_current_file_id(&mut self, id: crate::file_id::FileId) {
        self.current_file_id = Some(id);
    }

    pub(crate) fn set_current_file_absolute(&mut self, path: std::path::PathBuf) {
        self.current_file_absolute = Some(path);
    }

    /// Leaves the private property type checks to the caller, who takes
    /// them with `take_property_type_sites` and judges them against the
    /// facts of every file that writes those properties.
    pub(crate) fn set_defer_property_type_checks(&mut self, defer: bool) {
        self.defer_property_type_checks = defer;
    }

    /// Registers every third-party `rule::Rule` this `Checker` should
    /// dispatch to, mid-analysis, from `check_expr`/`check_stmt`'s own
    /// thin wrappers - real PHPStan's own `Rule<TNodeType>` equivalent
    /// (`rule`'s own module doc comment has the full picture). An
    /// embedder calls this once, right after `Checker::new` and before
    /// `check_program`, with a `rules` slice that outlives this
    /// `Checker` - compile-time-registered, same-binary rules, not
    /// dynamically loaded ones (no stable Rust ABI exists for that
    /// without a large separate FFI project).
    pub fn set_external_rules(&mut self, rules: &'a [Box<dyn crate::rule::Rule>]) {
        self.external_rules = rules;
    }

    /// This run's `property_assign_facts`.
    pub(crate) fn take_property_assign_facts(&mut self) -> PropertyAssignFacts {
        std::mem::take(&mut self.property_assign_facts)
    }

    /// Where every record a checking pass appends to stands now.
    pub(crate) fn pass_mark(&self) -> PassMark {
        PassMark {
            diagnostics: self.diagnostics.len(),
            semantic_tokens: self.semantic_tokens.len(),
            throw_points: self.current_function_throw_points.len(),
            explicit_throw_points: self.current_function_explicit_throw_points.len(),
            return_points: self.current_function_return_points.len(),
            exit_points: self.current_function_exit_points.len(),
            inlay_param_name_hints: self.inlay_param_name_hints.len(),
            inlay_local_type_hints: self.inlay_local_type_hints.len(),
        }
    }

    /// Starts checking a scope of its own: what of it other code may write
    /// (`scope`), its static variables, and no capture written yet. What
    /// the scope it is in had comes back with `leave_shared_scope`.
    pub(crate) fn enter_shared_scope(
        &mut self,
        scope: shared_scope::LazyShared<'a>,
    ) -> EnclosingScope<'a> {
        EnclosingScope {
            shared: std::mem::replace(&mut self.shared_scope, std::rc::Rc::new(scope)),
            captured: std::mem::take(&mut self.captured_types),
            statics: std::mem::take(&mut self.static_var_names),
            entries: std::mem::take(&mut self.capture_entries),
            widening: std::mem::take(&mut self.capture_widening),
            seen: std::mem::take(&mut self.capture_seen),
        }
    }

    pub(crate) fn leave_shared_scope(&mut self, enclosing: EnclosingScope<'a>) {
        self.shared_scope = enclosing.shared;
        self.captured_types = enclosing.captured;
        self.static_var_names = enclosing.statics;
        self.capture_entries = enclosing.entries;
        self.capture_widening = enclosing.widening;
        self.capture_seen = enclosing.seen;
    }

    /// Checks a scope's body with `check`, and again, from `locals` as it
    /// came and with what the pass before recorded dropped, while a closure
    /// of the scope captures by reference a variable the scope holds a type
    /// in, after one of its statements, that the closure was not entered
    /// with: the closure's next entry takes that type too. A variable still
    /// growing after a few passes is entered as anything.
    pub(crate) fn check_capturing_scope<R>(
        &mut self,
        locals: &mut Locals,
        mut check: impl FnMut(&mut Self, &mut Locals) -> R,
    ) -> R {
        const MAX_PASSES: usize = 4;
        if self.shared_scope.captures_nothing() {
            return check(self, locals);
        }
        let entry = locals.clone();
        let mark = self.pass_mark();
        let mut pass = 0;
        loop {
            self.capture_entries.clear();
            self.capture_seen.clear();
            let result = check(self, locals);
            pass += 1;
            let relation = self.relation();
            let mut widened = false;
            for (at, name, entered) in std::mem::take(&mut self.capture_entries) {
                let Some(seen) = self.capture_seen.get(&name) else {
                    continue;
                };
                if relation.is_super_type_of(&entered, seen).yes() {
                    continue;
                }
                let wider = if pass >= MAX_PASSES {
                    Type::mixed()
                } else {
                    relation.union_of(vec![entered, seen.clone()])
                };
                self.capture_widening.insert((at, name), wider);
                widened = true;
            }
            if !widened {
                return result;
            }
            self.rewind_to(mark);
            *locals = entry.clone();
        }
    }

    /// Records what each variable the scope's closures capture by reference
    /// holds now (`check_capturing_scope`).
    pub(crate) fn note_captured_types(&mut self, locals: &Locals) {
        if self.shared_scope.captures_nothing() {
            return;
        }
        let scope = std::rc::Rc::clone(&self.shared_scope);
        for name in scope.captured.keys() {
            let Some(held) = locals.get_var(name) else {
                continue;
            };
            let seen = match self.capture_seen.remove(name) {
                Some(earlier) => self.relation().union_of(vec![earlier, held.clone()]),
                None => held.clone(),
            };
            self.capture_seen.insert(name.clone(), seen);
        }
    }

    /// Whether `name` is a `static` variable of the scope being checked:
    /// one a `static` statement has bound, or one the scope binds later.
    pub(crate) fn is_static_var(&self, name: &syntax::name::Name) -> bool {
        self.static_var_names.contains(name.as_ref()) || self.shared_scope.has_static(name)
    }

    /// An empty scope of this checker's, made by its table of keys.
    pub(crate) fn new_locals(&self) -> Locals {
        Locals::new(
            std::rc::Rc::clone(&self.keys),
            self.current_class.as_deref(),
        )
    }

    /// Drops what every record gained since `mark`.
    pub(crate) fn rewind_to(&mut self, mark: PassMark) {
        self.diagnostics.truncate(mark.diagnostics);
        self.semantic_tokens.truncate(mark.semantic_tokens);
        self.current_function_throw_points
            .truncate(mark.throw_points);
        self.current_function_explicit_throw_points
            .truncate(mark.explicit_throw_points);
        self.current_function_return_points
            .truncate(mark.return_points);
        self.current_function_exit_points.truncate(mark.exit_points);
        self.inlay_param_name_hints
            .truncate(mark.inlay_param_name_hints);
        self.inlay_local_type_hints
            .truncate(mark.inlay_local_type_hints);
    }

    /// `textDocument/semanticTokens/full`'s own collection - see
    /// `semantic_tokens` field's own doc comment.
    pub(crate) fn take_semantic_tokens(&mut self) -> Vec<semantic_tokens::UsageSemanticToken> {
        std::mem::take(&mut self.semantic_tokens)
    }

    pub(crate) fn set_working_dir(&mut self, dir: Option<std::path::PathBuf>) {
        self.working_dir = dir;
    }

    /// Records every tracked variable's type after `stmt` - see
    /// `type_dump`.
    pub(crate) fn record_type_dump(&mut self, stmt: &StmtNode, locals: &Locals) {
        let span = self.stmt_span(stmt.id);
        let Some(dump) = self.type_dump.as_mut() else {
            return;
        };
        let mut vars: Vec<(String, LocalKey, &Type)> = locals
            .iter()
            .map(|(key, ty)| (locals.render(key), key, ty))
            .collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        let mut line = String::new();
        for (name, key, ty) in vars {
            if !line.is_empty() {
                line.push_str("; ");
            }
            line.push_str(&format!("{name}: {ty}"));
            if let Some(native) = locals.native(key) {
                line.push_str(&format!(" (native {native})"));
            }
        }
        let host = self.current_class.clone().unwrap_or_default();
        let at = self.line_index.line_col(span.start);
        dump.insert((at.line, at.col, host), line);
    }

    /// `line:col [host] vars` for every statement recorded by
    /// `record_type_dump`, in source order.
    pub(crate) fn take_type_dump(&mut self) -> Vec<String> {
        self.type_dump
            .take()
            .unwrap_or_default()
            .into_iter()
            .map(|((line, col, host), vars)| format!("{line}:{col} [{host}] {vars}"))
            .collect()
    }

    /// Keeps the expression-type and the PHPDoc resolution records, as
    /// `records` asks.
    pub(crate) fn set_records(&mut self, records: crate::project::Records) {
        self.type_dump = records.types.then(std::collections::BTreeMap::new);
        self.expr_types = records.expr_types.then(std::collections::BTreeMap::new);
        *self.doc_resolutions.borrow_mut() = records
            .doc_resolutions
            .then(std::collections::BTreeSet::new);
    }

    /// Whether the expression-type record is kept.
    pub(crate) fn records_expr_types(&self) -> bool {
        self.expr_types.is_some()
    }

    /// Records that the expression at `span` has the type `inferred` and
    /// the native type `native` in this pass.
    pub(crate) fn record_expr_type(&mut self, span: Span, inferred: Type, native: Type) {
        let host = self.current_class.clone().unwrap_or_default();
        if let Some(record) = self.expr_types.as_mut() {
            record.insert((span.start, span.end, host), (inferred, native));
        }
    }

    /// `start..end [host] inferred | native` for every expression the
    /// expression-type record holds, in source order.
    pub(crate) fn take_expr_types(&mut self) -> Vec<String> {
        self.expr_types
            .take()
            .unwrap_or_default()
            .into_iter()
            .map(|((start, end, host), (inferred, native))| {
                format!(
                    "{start}..{end} [{host}] {} | {}",
                    inferred.canonical_text(),
                    native.canonical_text()
                )
            })
            .collect()
    }

    /// Records, when the PHPDoc resolution record is kept, that the type
    /// `written`, read from a doc comment by `site`, resolves to `resolved`
    /// where the checker is: `[class] function site: written => resolved`.
    pub(crate) fn record_doc_resolution(&self, site: &str, written: &Type, resolved: &Type) {
        let mut record = self.doc_resolutions.borrow_mut();
        let Some(record) = record.as_mut() else {
            return;
        };
        record.insert(format!(
            "[{}] {} {site}: {} => {}",
            self.current_class.as_deref().unwrap_or_default(),
            self.current_method.as_deref().unwrap_or_default(),
            written.canonical_text(),
            resolved.canonical_text()
        ));
    }

    /// The PHPDoc resolution record's lines, sorted.
    /// The places the file's includes looked for a file
    /// (`Checker::include_probes`).
    pub(crate) fn take_include_probes(&mut self) -> Vec<std::path::PathBuf> {
        std::mem::take(&mut self.include_probes)
    }

    pub(crate) fn take_doc_resolutions(&mut self) -> Vec<String> {
        self.doc_resolutions
            .take()
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    pub(crate) fn set_collect_member_refs(&mut self, enabled: bool) {
        self.collect_member_refs = enabled;
    }

    /// No-op unless `collect_member_refs` is set. `&self`, not `&mut
    /// self` - see `member_ref_sites`'s own doc comment for why. `member`
    /// is normalized the same way `lsp::navigation`'s own ClassConstFetch
    /// arm is: a leading `$` (a static property's own syntax,
    /// `self::$prop`) stripped, and `"class"`/`"{dynamic}"` (not a real
    /// member at all) excluded by every caller before this is reached.
    pub(crate) fn record_member_ref(&self, span: Span, owner: &str, member: &str) {
        if !self.collect_member_refs {
            return;
        }
        self.member_ref_sites.borrow_mut().insert((
            span,
            owner.to_string(),
            member.trim_start_matches('$').to_string(),
        ));
    }

    /// `record_member_ref` for method `method` of `owner`, named as `owner`
    /// declares it: PHP compares method names without case, and the
    /// index keys a member by its declared name.
    pub(crate) fn record_method_ref(&self, span: Span, owner: &str, method: &str) {
        if !self.collect_member_refs {
            return;
        }
        self.record_member_ref(
            span,
            owner,
            &self.registry.declared_method_name(owner, method),
        );
    }

    /// Drains `member_ref_sites` into an owned `Vec` - the same
    /// `mem::take`-then-collect shape `take_semantic_tokens` uses one
    /// field up.
    pub(crate) fn take_member_ref_sites(&mut self) -> Vec<(Span, String, String)> {
        self.member_ref_sites.take().into_iter().collect()
    }

    /// See `escaped_enums`'s own doc comment. `class` must already be
    /// the registry's own canonically-declared name - every call site
    /// gets this for free (either reusing an already-canonical `owner`
    /// from a `lookup_method_with_owner`/`lookup_class` call it made for
    /// an unrelated reason, or calling `lookup_class` itself), never a
    /// raw, possibly differently-cased receiver name.
    pub(crate) fn record_enum_escape(&self, class: &str) {
        if !self.collect_member_refs {
            return;
        }
        self.escaped_enums.borrow_mut().insert(class.to_string());
    }

    /// `take_member_ref_sites`'s own sibling for `escaped_enums`.
    pub(crate) fn take_escaped_enums(&mut self) -> FxHashSet<String> {
        self.escaped_enums.take()
    }

    /// See `escaped_member_classes`'s own doc comment. Unlike `record_
    /// enum_escape`, `class` here is never assumed pre-canonicalized by
    /// the caller - every call site instead passes each class from
    /// `escape_targets_for_receiver_type`'s own already-canonical
    /// `Vec`, so this stays a plain, unconditional insert.
    pub(crate) fn record_member_class_escape(&self, class: &str) {
        if !self.collect_member_refs {
            return;
        }
        self.escaped_member_classes
            .borrow_mut()
            .insert(class.to_string());
    }

    /// `take_escaped_enums`'s own sibling for `escaped_member_classes`.
    pub(crate) fn take_escaped_member_classes(&mut self) -> FxHashSet<String> {
        self.escaped_member_classes.take()
    }

    /// Every class a *genuinely* dynamic member access on `ty` could
    /// reach a protected/public member of - `ty` resolving to a known,
    /// concrete class includes that class and everything in its own
    /// `Registry::all_ancestor_names` (an inherited member is reachable
    /// through a subclass instance too); `ty` resolving to an interface
    /// or non-final abstract class additionally includes every real
    /// class anywhere in the registry that has it in *their* own
    /// ancestor set (`Registry::inheritors`). A `Union`/`Intersection`
    /// recurses into every member and
    /// escapes the union of all of them - conservative by design: the
    /// real runtime receiver could be any one of them, and under-
    /// escaping (missing a real dynamic-dispatch reference) is the
    /// wrong-direction mistake for an escape hatch, unlike most of this
    /// project's other "favor false negatives" checks. Returns empty for
    /// anything else (`Mixed`, an unresolved type) - a real, disclosed
    /// false-positive risk `dead_members`'s own doc comment names, not
    /// silently assumed safe.
    pub(crate) fn escape_targets_for_receiver_type(&self, ty: &Type) -> Vec<String> {
        ty.object_class_names()
            .into_iter()
            .flat_map(|class| self.escape_targets_for_class(class))
            .collect()
    }

    /// `escape_targets_for_receiver_type` for an instance of `class`.
    fn escape_targets_for_class(&self, class: &str) -> Vec<String> {
        let Some((canonical, info)) = self.registry.lookup_class(class) else {
            return Vec::new();
        };
        let canonical = canonical.to_string();
        let mut targets: Vec<String> = vec![canonical.clone()];
        targets.extend(
            self.registry
                .all_ancestor_names(&canonical)
                .iter()
                .map(|s| s.to_string()),
        );
        if info.is_abstract() || info.kind() != ClassKind::Class {
            for other_fqn in self.registry.inheritors(&canonical) {
                let Some(other_info) = self.registry.class_by_name(&other_fqn) else {
                    continue;
                };
                if other_info.kind() == ClassKind::Class && !other_info.is_abstract() {
                    targets.push(other_fqn.to_string());
                    targets.extend(
                        self.registry
                            .all_ancestor_names(&other_fqn)
                            .iter()
                            .map(|s| s.to_string()),
                    );
                }
            }
        }
        targets
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
        message: String,
        span: Span,
        level: u8,
        identifier: impl Into<diagnostics::Identifier>,
    ) {
        self.diagnostics
            .push(DiagnosticBuilder::new(message, span, level, identifier).build(&self.line_index));
    }

    /// `report`, for a finding made only from configured level
    /// `found_from` on (`rejected_from`).
    pub(crate) fn report_found_from(
        &mut self,
        message: String,
        span: Span,
        level: u8,
        found_from: u8,
        identifier: impl Into<diagnostics::Identifier>,
    ) {
        self.report_builder(
            DiagnosticBuilder::new(message, span, level, identifier).found_from(found_from),
        );
    }

    /// The builder path - for a call site that also wants a `tip`, a
    /// `fix`, or `non_ignorable`, none of which `report`'s own
    /// positional signature has room for without every plain call site
    /// paying for arguments it never uses.
    pub(crate) fn report_builder(&mut self, builder: DiagnosticBuilder) {
        self.diagnostics.push(builder.build(&self.line_index));
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
    pub(crate) fn check_implicit_tostring_usage(&mut self, ty: &Type, span: Span) {
        let Some(class) = ty.first_object_class().cloned() else {
            return;
        };
        let Some((owner, sig)) = self.lookup_method_with_owner(&class, "__toString") else {
            return;
        };
        self.check_internal_method_call(&owner, "__toString", sig, false, span);
        self.check_deprecated_method_call(&owner, "__toString", sig, false, span);
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

    /// Whether the file being checked names what a closure rebind needs
    /// (`unused::WATCH`'s bit in `syntax::FileFacts::mentions`); true until said
    /// otherwise.
    pub fn set_closure_rebind_possible(&mut self, possible: bool) {
        self.closure_rebind_possible = possible;
    }

    /// Whether the file being checked names what its include guards are
    /// found by (`includes::WATCH`); true until said otherwise.
    pub fn set_include_guards_possible(&mut self, possible: bool) {
        self.include_guards_possible = possible;
    }

    /// The spellings the file being checked names, as `FileFacts::mentions`
    /// has them.
    pub fn set_file_mentions(&mut self, mentions: u32) {
        self.file_mentions = mentions;
    }

    /// Whether the file builds a function-like or class-like inside a body,
    /// as `FileFacts::has_nested_function_like` has it.
    pub fn set_file_has_nested_function_like(&mut self, nested: bool) {
        self.file_has_nested_function_like = nested;
    }

    /// Where the file being checked first spells each syntax its tree
    /// does not keep.
    pub fn set_syntax_marks(&mut self, marks: syntax::SyntaxMarks) {
        self.syntax_marks = marks;
    }

    #[hotpath::measure]
    pub fn check_program(&mut self, program: &[StmtId]) {
        let syntax_owner = self
            .syntax_tree_owner()
            .expect("checking arena-backed syntax requires its file owner");
        let arenas = syntax_owner.arenas();
        let stmts = syntax_owner.statements();
        debug_assert_eq!(program, stmts);
        if let Some(tree) = self.syntax_tree.or(self.syntax_tree_owned.as_deref()) {
            debug_assert_eq!(program, tree.statements());
            #[cfg(debug_assertions)]
            assert_node_spans(tree.arenas(), program);
        } else if let Some(file) = self.syntax_file_owned.as_deref() {
            debug_assert_eq!(program, file.statements());
            #[cfg(debug_assertions)]
            assert_node_spans(&file.arenas, program);
        }
        self.current_function_assigned_variables =
            if self.settings.strict_rules.check_implicit_array_creation {
                strict_rules::LazyAssigned::new(arenas, stmts)
            } else {
                strict_rules::LazyAssigned::default()
            };
        self.constant_facts = if self.file_mentions & global_constants::MENTIONS_MAGIC != 0 {
            global_constants::constant_facts(arenas, stmts)
        } else {
            debug_assert!(
                global_constants::constant_facts(arenas, stmts)
                    .magic_in_closure
                    .is_empty(),
                "a file that does not name __FUNCTION__ or __METHOD__ has none in a closure"
            );
            Default::default()
        };
        if self.current_file.is_some() {
            self.include_constant_initializers =
                includes::file_constant_initializers(arenas, stmts);
            (self.include_guarded_paths, self.include_path_dirs) = if self.include_guards_possible {
                self.collect_include_guards(arenas, stmts)
            } else {
                (FxHashSet::default(), includes::default_include_dirs())
            };
        }
        self.check_declare_directives(arenas, stmts);
        self.check_file_whitespace(arenas, program);
        self.check_statement_keywords(stmts);
        let syntax_marks = self.syntax_marks;
        self.check_syntax_versions(arenas, program, &syntax_marks);
        self.check_use_function_imports(arenas, program);
        self.check_use_const_imports(arenas, program);
        self.check_use_class_imports(arenas, program);
        self.check_used_names(arenas, program);
        let (rebind_targets, rebind_escape_all) = if self.closure_rebind_possible {
            unused::scan_program_for_rebind_targets(arenas, stmts)
        } else {
            (NameSet::default(), false)
        };
        self.rebind_target_classes = rebind_targets;
        self.rebind_escape_all = rebind_escape_all;
        // A file-local `uses(...)` call (no `->in(...)`) wins over
        // whatever `Project::check_file` already set via
        // `set_pest_this_type` from directory-scoped Pest.php config -
        // matches real Pest's resolution order (file-local checked
        // first).
        if let Some(local_ty) = pest_config::file_local_this_type(arenas, program) {
            self.pest_this_type = local_ty;
        }
        // Unlike `pest_this_type` above, hook properties are unioned, not
        // overridden - a property-name set has no "file-local wins"
        // precedence to apply. `file_local_hook_properties` returns owned
        // `String`s, interned here on the way in to match
        // `pest_hook_properties`'s own interned representation.
        self.pest_hook_properties.extend(
            pest_config::file_local_hook_properties(arenas, program)
                .into_iter()
                .map(|p| Name::from(&p)),
        );

        let mut top_locals: Locals = self.new_locals();
        top_locals.set_track(self.probe_span.get().is_some());
        let top_level_scope = shared_scope::LazyShared::of_statements(
            self.syntax_tree_owner(),
            self.registry,
            arenas,
            stmts,
            &[],
            None,
            true,
            false,
            &[],
        );
        let enclosing_scope = self.enter_shared_scope(top_level_scope);
        let diagnostics_before = self.diagnostics.len();
        // `self.name_ctx` is tracked *positionally* here: real PHP only
        // allows a `namespace`/`use` statement at the top level of a file,
        // so a single left-to-right pass, mutating `self.name_ctx` in
        // place when one is seen, means that by the time this loop
        // reaches a `ClassDecl`/`FunctionDecl`/`ClassDecl`, `self.name_ctx`
        // reflects whichever `namespace` was active at that declaration's
        // own position - not merely the last one anywhere in the file,
        // which would wrongly resolve a declaration under an earlier
        // namespace against a later one.
        //
        // The top level's own statements share one scope across the whole
        // file, checked in passes while a closure capturing by reference
        // widens (`check_capturing_scope`); the declarations, whose scopes
        // do not read its variables, are checked after, once.
        self.check_capturing_scope(&mut top_locals, |this, locals| {
            this.name_ctx = NameContext::default();
            for &stmt_id in program {
                let Some(stmt) = arenas.stmt(stmt_id) else {
                    continue;
                };
                match &stmt.kind {
                    StmtKind::Namespace(name) => this.name_ctx.set_namespace(name),
                    StmtKind::Use(imports) => this.name_ctx.add_uses(arenas.use_imports(imports)),
                    StmtKind::FunctionDecl(_) | StmtKind::ClassDecl(_) => {}
                    _ => {
                        this.check_stmt(stmt, locals, None, "<main>");
                    }
                }
            }
        });
        self.name_ctx = NameContext::default();
        for &stmt_id in program {
            let Some(stmt) = arenas.stmt(stmt_id) else {
                continue;
            };
            self.check_function_like_declarations_in_stmt(arenas, stmt_id);
            match &stmt.kind {
                StmtKind::Namespace(name) => self.name_ctx.set_namespace(name),
                StmtKind::Use(imports) => self.name_ctx.add_uses(arenas.use_imports(imports)),
                StmtKind::FunctionDecl(id) => {
                    if let Some(function) = arenas.function(*id) {
                        self.check_function(function, None, false, true);
                    }
                }
                StmtKind::ClassDecl(c) => {
                    let c = arenas.class_decl(c.clone());
                    let Some(def) = arenas.class(c.def) else {
                        continue;
                    };
                    // `current_class` must be the fully-qualified name -
                    // it's looked up directly against
                    // `self.registry.classes()` (via `resolve_pseudo` for
                    // self/parent/static, and the undefined-class check
                    // for `new self()`/`new static()`), which is now
                    // FQN-keyed.
                    let fqn = c.fq_name().to_string();
                    let is_enum = def.kind == ClassKind::Enum;
                    let class_doc = def
                        .doc_comment
                        .as_ref()
                        .and_then(|ranges| arenas.doc_comment_text(ranges));
                    if is_enum {
                        self.check_enum_template_tag(&fqn, def);
                        self.check_enum_case_values(&fqn, def);
                        self.check_enum_sanity(&fqn, c);
                    } else {
                        self.check_enum_cases_outside_enum(def.enum_cases(arenas));
                    }
                    self.check_class_generic_bounds(&fqn, c, def);
                    let description = match def.kind {
                        ClassKind::Interface => Some("an interface"),
                        ClassKind::Trait => None,
                        ClassKind::Enum => Some("an enum"),
                        ClassKind::Class => Some("a class"),
                    };
                    if let Some(description) = description {
                        self.check_misplaced_var_tag(
                            class_doc.as_deref(),
                            description,
                            c.name.span,
                        );
                    }
                    self.check_class_completeness(&fqn, def.span);
                    self.check_inheritance_target_kinds(&fqn, false, def.span);
                    self.check_readonly_class_version(def.is_readonly, false, c.name.span);
                    self.check_magic_serialization_methods(&fqn, &fqn, c.name.span);
                    self.check_class_references_exist(&fqn, def.span);
                    self.check_internal_inheritance(&fqn, def.kind, def.span);
                    self.check_restricted_class_inheritance(&fqn, def.span);
                    for name in def
                        .extends(arenas)
                        .iter()
                        .chain(def.implements(arenas))
                        .chain(def.used_traits(arenas))
                    {
                        self.check_class_name_case(name);
                    }
                    self.check_method_tag_types(&fqn, class_doc.as_deref(), def.span);
                    self.check_method_tag_templates(&fqn, class_doc.as_deref(), def.span);
                    self.check_mixin_types(&fqn, class_doc.as_deref(), def.span);
                    self.check_type_alias_types(&fqn, class_doc.as_deref(), def.span);
                    self.check_method_overrides(&fqn, c);
                    self.check_property_overrides(&fqn, def);
                    self.check_require_tags(&fqn, def.span);
                    self.check_require_tag_types(def);
                    self.check_sealed_hierarchy(&fqn, def.span);
                    self.check_sealed_tag_types(def);
                    self.check_unknown_phpstan_tags_in_class(c);
                    if self.settings.sidz.check_magic_numbers {
                        self.check_magic_numbers_in_properties(arenas, def.properties(arenas));
                    }
                    self.check_property_hooks(&fqn, def);
                    self.check_property_variances(&fqn, def);
                    self.check_trait_conflicts(&fqn, def.span);
                    self.check_uninitialized_properties(&fqn, c);
                    self.check_default_property_types(arenas, &fqn, def);
                    self.check_premature_property_access(&fqn, c);
                    self.check_duplicate_class_members(&fqn, def);
                    if self.settings.phpunit.check_risky_tests {
                        self.check_risky_test_methods(arenas, &fqn, def);
                    }
                    self.check_method_declaration_sanity(
                        &fqn,
                        def.kind,
                        def.is_abstract,
                        arenas.methods(def.methods.clone()),
                    );
                    self.check_property_declarations(
                        &fqn,
                        &fqn,
                        def.kind,
                        def.is_abstract,
                        def.properties(arenas),
                        arenas.methods(def.methods.clone()),
                    );
                    self.check_reserved_class_constant_name(def.consts(arenas));
                    self.check_typed_class_constant_values(&fqn, def.consts(arenas));
                    self.check_class_constant_modifiers(&fqn, def.kind, def.consts(arenas));
                    self.check_constant_overrides(&fqn, def.consts(arenas));
                    self.check_trait_constant_conflicts(&fqn, def.consts(arenas));
                    // The class's own attributes, plus every property's/
                    // const's - method/param attributes are checked
                    // per-declaration inside `check_function` below
                    // instead, the one place that runs for every method
                    // regardless of how it's reached (a direct method
                    // here, or via `check_trait_methods`'s per-host loop
                    // for a trait).
                    self.check_attributes(arenas.attributes(def.attrs.clone()), &fqn);
                    self.check_attribute_targets(
                        arenas.attributes(def.attrs.clone()),
                        ATTRIBUTE_TARGET_CLASS,
                        "class",
                    );
                    self.check_self_attribute_class_declaration(
                        arenas.attributes(def.attrs.clone()),
                        &fqn,
                        def.kind,
                        def.is_abstract,
                    );
                    self.check_class_like_marker_attributes(
                        arenas.attributes(def.attrs.clone()),
                        def.kind,
                        def.is_readonly,
                        &fqn,
                    );
                    for p in def.properties(arenas) {
                        self.check_attributes(arenas.attributes(p.attrs.clone()), &fqn);
                        self.check_attribute_targets(
                            arenas.attributes(p.attrs.clone()),
                            ATTRIBUTE_TARGET_PROPERTY,
                            "property",
                        );
                    }
                    for case in def.enum_cases(arenas) {
                        self.check_attributes(arenas.attributes(case.attrs.clone()), &fqn);
                        self.check_attribute_targets(
                            arenas.attributes(case.attrs.clone()),
                            ATTRIBUTE_TARGET_CLASS_CONSTANT,
                            "class constant",
                        );
                    }
                    for cc in def.consts(arenas) {
                        self.check_attributes(arenas.attributes(cc.attrs.clone()), &fqn);
                        self.check_attribute_targets(
                            arenas.attributes(cc.attrs.clone()),
                            ATTRIBUTE_TARGET_CLASS_CONSTANT,
                            "class constant",
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
                    self.check_missing_property_typehints(&fqn, def.properties(arenas));
                    self.check_missing_class_constant_typehints(&fqn, def.consts(arenas));
                    if self.settings.extras.check_native_type_hints {
                        self.check_native_class_constant_types(&fqn, def.consts(arenas));
                    }
                    if def.kind == ClassKind::Trait {
                        self.check_trait_methods(&fqn, c);
                    } else {
                        for m in arenas.methods(def.methods.clone()) {
                            self.check_function(
                                &m.function,
                                Some(&fqn),
                                m.is_static,
                                !m.is_abstract && def.kind != ClassKind::Interface,
                            );
                        }
                        // A class's private consts/methods/properties,
                        // checked once per class, not per method like
                        // check_function's per-body checks - "was this
                        // ever referenced" needs every one of the class's
                        // bodies scanned together, not one at a time.
                        self.check_unused_private_members(
                            &fqn,
                            def.kind,
                            arenas.methods(def.methods.clone()),
                            def.properties(arenas),
                            def.consts(arenas),
                        );
                        self.record_property_type_site(arenas, &fqn, vec![Name::from(&fqn)], def);
                        self.check_parent_constructor_call(&fqn, def);
                        self.check_class_access_attributes(arenas, &fqn, c, def);
                        self.check_promoted_property_candidate(&fqn, def);
                    }
                }
                _ => self.check_nested_declarations(stmt),
            }
        }
        self.leave_shared_scope(enclosing_scope);
        // The top level's statements share one scope, so an `extract()` or
        // `eval()` anywhere in them drops what the whole file found.
        if self.saw_dynamic_var_construct {
            self.diagnostics.truncate(diagnostics_before);
            self.property_type_sites.clear();
            self.saw_dynamic_var_construct = false;
        }
        if !self.defer_property_type_checks {
            let sites = std::mem::take(&mut self.property_type_sites);
            let found = check_too_wide_property_types(
                &sites,
                &self.property_assign_facts,
                &self.relation(),
                &self.line_index,
            );
            self.diagnostics.extend(found);
        }
    }
}

#[cfg(debug_assertions)]
fn assert_node_spans(arenas: &NodeArenas, program: &[StmtId]) {
    struct CheckSpans<'a>(&'a NodeArenas);

    impl syntax::visit::arena::Visitor for CheckSpans<'_> {
        fn visit_stmt(&mut self, arenas: &NodeArenas, id: StmtId) {
            let Some(_stmt) = arenas.stmt(id) else {
                debug_assert!(false, "statement ID does not resolve in the tree owner");
                return;
            };
            debug_assert!(arenas.stmt_span(id).is_some());
            syntax::visit::arena::walk_stmt(self, arenas, id);
        }

        fn visit_expr(&mut self, arenas: &NodeArenas, id: ExprId) {
            let Some(_expr) = arenas.expr(id) else {
                debug_assert!(false, "expression ID does not resolve in the tree owner");
                return;
            };
            debug_assert!(self.0.expr_span(id).is_some());
            syntax::visit::arena::walk_expr(self, arenas, id);
        }
    }

    syntax::visit::arena::walk_stmts(&mut CheckSpans(arenas), arenas, program);
}

/// What `Checker::enter_shared_scope` set aside for the scope a nested one
/// is in.
pub(crate) struct EnclosingScope<'a> {
    shared: std::rc::Rc<shared_scope::LazyShared<'a>>,
    captured: rustc_hash::FxHashMap<syntax::name::Name, Type>,
    statics: NameSet,
    entries: Vec<(u32, syntax::name::Name, Type)>,
    widening: rustc_hash::FxHashMap<(u32, syntax::name::Name), Type>,
    seen: rustc_hash::FxHashMap<syntax::name::Name, Type>,
}

#[cfg(test)]
mod tests;
