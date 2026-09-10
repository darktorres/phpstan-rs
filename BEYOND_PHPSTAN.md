# Beyond parity

ROADMAP.md tracks the path to feature *parity* with real PHPStan. This document
is the opposite direction: concrete, source-grounded opportunities for
phpstan-rs to do things real PHPStan doesn't do, or can't do as well, once
parity work uncovered a real architectural reason why not. Every item below
was produced by six independent surveys of `vendor/phpstan-src/src` (real
PHPStan) and `src/` (phpstan-rs), then adversarially re-verified against the
actual cited files (not the surveying agent's own claim) before being kept -
14 of 18 raw proposals survived that pass; the other 4 (section E) were
provisionally dropped only because their citation, `phpstan/phpstan-strict-
rules`, wasn't vendored in this repo yet - it now is, and all 4 are
re-verified and included below. Nothing here is scheduled; it's a grounded
idea backlog, same status as ROADMAP's own "Newly identified gaps" until
triaged into a numbered phase.

Two structural facts most items below trace back to, repeatedly:

- **phpstan-rs is a native binary for every user, always.** Real PHPStan is a
  PHP process; the speed/precision it's willing to pay for is capped by PHP
  interpreter cost, and its own team is mid-flight on a separate, opt-in
  native C++ extension ("Turbo", `vendor/phpstan-src/turbo-ext/`,
  `src/Turbo/`) specifically to claw some of that back for users who install
  it. Several items below are phpstan-rs getting a Turbo-tier win by
  construction, with no separate extension, no build matrix, no
  `PHPSTAN_TURBO=0` fallback path.
- **phpstan-rs checks one closed-world project (app + vendored deps) in one
  process with one shared, frozen `Registry`.** Real PHPStan has to stay sound
  for arbitrary downstream consumers of a library it's analyzing, and its own
  parallelism model is separate OS worker processes with no shared memory
  (hence its own `ArenaCache`/shared-memory-arena subsystem, built just to
  claw back the cost of *not* sharing). phpstan-rs doesn't have either
  constraint - single process, real shared references, whole-program view by
  default.

---

## A. More precise than real PHPStan itself

Real PHPStan approximates in these four spots for its own architectural
reasons; phpstan-rs doesn't share the reason, so it can be exact where
PHPStan chooses not to be.

### A1. Raise the loop-convergence pass cap (currently copied from PHPStan verbatim, for the wrong reason)

`checker::converge_loop_body` (`src/checker/control_flow.rs:289-384`) mirrors
real PHPStan's bounded loop-body fixed-point at `MAX_PASSES = 3`
(`control_flow.rs:338`), copied from
`NodeScopeResolver::LOOP_SCOPE_ITERATIONS = 3`. But phpstan-rs never copied
the *reason* 3 is tolerable for PHPStan: PHPStan widens literal/constant
types to their base type after just **one** candidate pass
(`GENERALIZE_AFTER_ITERATION = 1`, `NodeScopeResolver.php:120-121`,
applied in `StmtHandler/WhileHandler.php:70-108`,
`MutatingScope::generalizeWith`/`generalizeType`,
`MutatingScope.php:4171-4333`) specifically because every pass re-walks the
loop body through PHPStan's full Scope-object/Rule-registry/PHPDoc machinery
- expensive at PHP-interpreter speed, which is also why
`isReplayableConvergenceBody`/`REPLAYABLE_BODY_ATTRIBUTE` caching exists
(`NodeScopeResolver.php:970-1008`) purely to dodge a second walk.
phpstan-rs's own convergence loop does plain type-union merging on every
pass with no early widening at all - it's already more precise per pass than
PHPStan, it just stops at a pass count borrowed from a mechanism it doesn't
use. Each pass here is a native match-arm walk with no per-node
Rule-registry dispatch, so raising `MAX_PASSES` (5-8, or adaptive on
non-convergence) costs little and lets more real loops (an accumulator
unioning several distinct branch outcomes, a state machine advancing
through several literal states) reach their true fixpoint instead of being
truncated early.

**Effort: small.**

### A2. Unroll `foreach` over small literal arrays, with a higher ceiling than PHPStan's 16/8

Real PHPStan re-walks a `foreach` body once per key/value pair when the
subject is a small compile-time-known `ConstantArrayType`
(`tryProcessUnrolledConstantArrayForeach`,
`StmtHandler/ForeachHandler.php:544-663`), binding each iteration's exact
literal element type - so a diagnostic inside the body can be exact
per-iteration (e.g. a `match` arm unreachable only on the 2nd of 3 known
elements). It caps this at `FOREACH_UNROLL_LIMIT = 16` total keys and
`FOREACH_UNROLL_NESTED_LIMIT = 8` for nested unrolled loops
(`ForeachHandler.php:68-69`) because each extra unrolled iteration re-walks
the entire body through PHPStan's per-statement Scope-copy/Rule-dispatch
machinery - real, measurable cost. phpstan-rs has no such mechanism at all
today: `check_foreach` (`src/checker/stmt/loops.rs:526-583`) always takes
the beyond-the-cap path, unconditionally unioning every `ArrayShape` field
into one coarse element type and checking the body once, even for a trivial
2-element literal. Building this as a new feature - phpstan-rs's
`Type::ArrayShape` is a flat `Vec<(String, Type, bool)>`
(`src/types.rs:52-58`) and `check_stmt` is a plain function call, no
DI/PHPDoc-reparse per statement - could ship with a materially higher
default ceiling (or one scaled by the body's own statement count) and still
finish faster than PHPStan does today at its lower ceiling.

**Effort: large** (new feature, not a tuning knob).

### A3. When `offsetAccess.notFound` gets built (ROADMAP.md:946, already a tracked parity gap), use a higher power-set bailout than PHPStan's 16384

Real PHPStan's `TypeUtils::flattenTypes` (`Type/TypeUtils.php:134-186`)
expands a `ConstantArrayType` with N optional keys into all 2^N
present/absent variants, cartesian-producing across multiple constant arrays
being flattened together, so downstream checks (including
`Rules/Arrays/NonexistentOffsetInArrayDimFetchCheck.php`) reason exactly
about which key-presence combinations are possible. It bails to the
un-expanded type past an estimated 16384 variants
(`TypeUtils.php:150-169`) because each materialized variant is a real
`ConstantArrayTypeBuilder` object allocation. phpstan-rs's own
`ArrayShape` power-set enumeration would be a cheap `Vec` clone-plus-flag-flip
per variant, with none of that object-graph overhead - when this rule's
missing half gets built, its bailout threshold should come from a benchmark
against phpstan-rs's own per-variant cost, not a reflexive import of
PHPStan's 16384.

**Effort: medium**, and only relevant once A3's parent rule (ROADMAP.md:946)
is scheduled - fold this into that phase's design rather than tracking
separately.

### A4. Check `BenevolentUnionType` results by default, instead of leaving them permanently unchecked

phpstan-rs has no `BenevolentUnionType` concept at all (`grep -rn Benevolent
src/` - nothing): `check_binary_operand_validity`
(`src/checker/operators.rs:236-238`) bails immediately once either operand
is `Mixed`, and `infer_binary`'s `BinOp::Add` arm
(`src/checker/expr.rs:3560-3608`) falls through to a bare `Type::Int` for
any combination it can't range-reason about - discarding the fact that PHP's
`+`/`&`/`|` on two `mixed`s has a real, narrow result shape. Real PHPStan
computes that narrower shape explicitly
(`InitializerExprTypeResolver::getBitwiseAndType`/`getBitwiseOrType` return
`new BenevolentUnionType([new IntegerType(), new StringType()])` when both
operands are `MixedType`, `InitializerExprTypeResolver.php:1077-1079,
1137-1139, 1197-1199`) - then **systematically declines to check it**:
`RuleLevelHelper::findTypeToCheckImplementation`/`filterTypeWithMethod`
route any `BenevolentUnionType` through the same skip path unless
`checkBenevolentUnionTypes` is true (`RuleLevelHelper.php:130-142,
260-293`), and that flag ships hardcoded `false` in `conf/config.neon:77`,
never raised by any of `conf/config.level0.neon` through `level10.neon` -
confirmed by grepping every level file - and re-pinned `false` in dozens of
PHPStan's own `RuleTest` classes. The flag exists to protect PHPStan's
installed base from a BC break; phpstan-rs has no installed base or BC
promise to protect (ROADMAP's own Won't-port section: "a Rust binary with
no such namespace"). It could compute the same narrowed union (mirroring
the `InitializerExprTypeResolver` arms) and actually enforce it by default -
catching bugs real PHPStan silently waves through at every strictness
level, including 9 and 10.

**Effort: medium.**

### A5. Auto-infer closed (sealed) hierarchies from whole-program subclass enumeration, not just an explicit `@phpstan-sealed` tag

ROADMAP.md's own "Known but not yet scheduled" list (~line 1125-1126) names
`class-string<T>`/`instanceof` sealed-hierarchy narrowing as an unmodeled
gap - phpstan-rs already tracks `@phpstan-sealed` for declaration
enforcement (Phase 36), and porting that same tag to drive narrowing would
only be parity. The genuinely-beyond move: real PHPStan's sealed narrowing
(`ObjectType::changeSubtractedType`, `ObjectType.php:1766-1818`, correctly
collapsing to `NeverType` once every allowed subtype is exhausted) only ever
fires when fed data from an explicit author-written `@phpstan-sealed`/
`@psalm-inheritors` tag (`SealedAllowedSubTypesClassReflectionExtension.php:16-19`,
gated on `count($classReflection->getSealedTags()) > 0`) or a real PHP enum
- for an ordinary untagged class, `ObjectType::isInstanceOf`
(`ObjectType.php:1726-1750`) always returns `Maybe`, and there is no
descendant-enumeration API anywhere in `Reflection/` (confirmed: no
`getSubclasses`/`descendantClasses`/`getKnownSubclasses`). This is
deliberate on PHPStan's side, not a fixpoint/perf limit: PHPStan is one
binary used to analyze both applications and reusable libraries, and a
library's downstream consumers can add subclasses PHPStan never sees, so
auto-inferring closedness from one repo's visible files would be unsound
for that core use case. phpstan-rs's own verification model is different -
closed-world, whole-program analysis of an application plus its vendored
dependencies as one fixed universe (exactly ROADMAP's own corpus
methodology). In that setting, "zero subclasses found anywhere in the
analyzed set" is sound. phpstan-rs's registry already builds the
project-wide class graph this would read from
(`src/registry/hierarchy.rs:44`, `all_ancestor_names`) but only indexes the
parent direction - adding a reverse (parent-to-children) index and treating
a subclass-free non-final class/interface as implicitly closed would let
phpstan-rs narrow `instanceof`/`class-string<T>` chains that real PHPStan,
run on the identical codebase with the identical missing annotation,
cannot.

**Effort: large.**

---

## B. Whole-program analysis PHPStan's own architecture can't cheaply do

Real PHPStan computes real dependency graphs (`Dependency/DependencyResolver.php`),
but only to decide what to re-check next run - the graph is discarded per
run, never kept as one persistent structure a rule queries. phpstan-rs
already keeps exactly that structure live, for a different purpose
(incremental recheck), and it's already in memory before/during checking.

### B1. Whole-application unused protected/public member detection

`UnusedPrivateMethodRule` (and its Property/Constant siblings) explicitly
gate on `isPrivate()` (`Rules/DeadCode/UnusedPrivateMethodRule.php:59`) -
it's the *only* dead-member rule PHPStan ships (`find vendor/phpstan-src/src
-iname '*Unused*'` turns up no protected/public equivalent anywhere),
because verifying no *other file* (a subclass override caller, a sibling
class using a parent's protected API) reaches a protected/public member
would need PHPStan's 14-collector `CollectedDataNode` machinery, built
specifically to reconcile state across its own parallel worker processes
(ROADMAP.md:812-825, "Phase 82" note) - a cost real PHPStan's process model
forces on it. phpstan-rs is single-process with one whole-project `Registry`
merged before checking starts (`Project::load_file`, `src/project.rs:69-72`)
- no parallel-worker reconciliation problem to begin with. The same
registry-build-time fixed-point shape Phase 41's transitive-purity model
already uses (`src/registry/purity.rs`) could extend
`checker/unused/member_usage.rs`'s existing private-member tracker
(`MemberUsage`) to walk every class's protected/public member accesses
project-wide, resolving each receiver's declared/inherited class through
the registry instead of assuming `self`.

**Effort: large.**

### B2. Dead top-level function / never-instantiated class detection

There is no `UnusedFunctionRule`/`UnusedClassRule` anywhere in PHPStan
(`find vendor/phpstan-src/src -iname '*Unused*'` returns only
parameter/private-member/label/closure-use variants, never a top-level
symbol). PHPStan does build a whole-project dependency graph
(`Dependency/DependencyResolver.php`), but only to feed
`ResultCacheManager`'s `$invertedDependencies`
(`Analyser/ResultCache/ResultCacheManager.php:438`) for cache invalidation -
never turned into a "referenced nowhere" diagnostic. phpstan-rs already
builds and keeps, in one process, both directions of this exact graph:
`Project.dependents: FxHashMap<String, HashSet<PathBuf>>`
(`src/project.rs:70`, populated in `load_file`) is a live reverse-reference
index built from `deps::referenced_names`'s deliberately over-approximating
name collection (`src/deps.rs:27-30`). A symbol with zero entries in
`dependents` (and not an entry point/magic method/attribute target) is
provably dead project-wide - a query against data phpstan-rs already
computes for cache invalidation, not a new cross-process aggregation step.
Ship opt-in, matching this project's own "don't discard correct checks,
ship opt-in" precedent, since libraries/entry points need an exclusion
path.

**Effort: medium.**

### B3. Circular class/namespace dependency-cycle detection

PHPStan has no circular-class/namespace-dependency rule at all - its only
"circular" concept is `CircularTypeAliasErrorType`/
`CircularTypeAliasDefinitionException` (self-referential `@phpstan-type`
alias *definitions*, not class/namespace reference cycles). Its
`DependencyResolver`/`ExportedNode` machinery computes the edges a
cycle-detector would need but discards the graph per-run rather than
keeping it live for a rule to query. phpstan-rs's `Project.dependents`
(`src/project.rs:70`) is already a persistent, whole-project, in-memory
graph kept live across incremental updates - `update_file`'s
`transitive_affected_files` (`src/project.rs:395-435`) already walks it
transitively on every edit. Running SCC-based cycle detection (Tarjan's) over
that graph, aggregated to namespace level via the existing
`registry::NameContext`, is querying an already-live structure rather than
building new cross-file infrastructure.

**Effort: medium.**

---

## C. Native-performance wins real PHPStan needs a separate extension for

Real PHPStan's own team ships (or is still building) these as opt-in native
C++ ("Turbo") because their stock PHP implementation can't afford them.
phpstan-rs is already native for every user - it doesn't need the extension
architecture, only the data structure.

### C1. Type interning + memoized union/intersect/remove (TypeCombinator caching), on by default

Real PHPStan's own `TypeCombinatorCache` states plainly that the stock PHP
implementation "performs no caching at all" for union/intersect/remove
calls, and that the native version exists because "roughly 91% of the calls
in a self-analysis run repeat an argument tuple that was already computed"
(`Type/TypeCombinatorCache.php:7-23`) - real PHPStan's own documented
biggest lever, shipped only behind the separately-built Turbo extension, so
most installs (source checkouts, unsupported platforms, `PHPSTAN_TURBO=0`)
never get it. phpstan-rs needs this architecture just as badly: its own
`checker/type_union.rs` doc comments record a real hang this project hit -
`gdb` caught a `check-project` run spinning inside `Type::eq`, called from
`union_types`, called from `merge_scopes` on every `if`/`else` - worked
around with an ad hoc `MAX_UNION_MEMBERS = 12` cap that collapses to
`Mixed` past a threshold, rather than the root fix (memoize, intern) real
PHPStan's own team applied to the identical problem shape. A
structural-key memo cache plus an interning arena for `union_types` (and
future intersect/remove helpers) gives every phpstan-rs user PHPStan's
Turbo-tier win by default, no opt-in extension, no build matrix.

**Effort: large.**

### C2. Parallelize `check_project`'s per-file loop across native threads sharing one `Registry`

Real PHPStan is a PHP process; PHP gives it no way to share a live object
graph across cores, so any parallelism requires separate OS processes
(`ParallelAnalyser`), each independently paying to materialize PHPStan's
own read-mostly data. Real PHPStan's team built an entire shared-memory
arena subsystem just to claw that duplication back -
`ArenaCache`'s PHP twin "shares nothing... lookups always miss", its native
counterpart a POSIX-shm/Windows-pagefile arena the master process creates
before spawning workers (`src/Cache/ArenaCache.php:8-19`), with fork-guard/
process-restart machinery (`TurboProcessRestarter.php`) layered on top.
phpstan-rs's `check-project` command runs sequentially today
(`run_check_project`'s `analyze_files.iter().map(...)`,
`src/main/check_project.rs:140-143`) even though `Project::check_file`
only needs `&self` (`src/project.rs:444`) and neither `Project` nor
`Registry` has interior mutability blocking cross-thread sharing. Rust's
ownership model gets phpstan-rs exactly the property `ArenaCache` was
purpose-built to bolt on - one shared registry, no per-worker duplication -
for free, via ordinary borrowed references across `std::thread::scope`.
The sibling parse-only `check` command already proves the pattern
(`src/main/check.rs:51-65`); it just hasn't been applied to
`check_project`'s semantic-analysis loop, the one that actually dominates a
real run's cost.

**Effort: small.**

### C3. Memoize class-hierarchy member lookups

Real PHPStan's own July-2026 performance write-up names member-lookup
pricing ("`ObjectType::getMethod` + `getMethodReflection` + dynamic-extension
registry sweeps, ~1.9M calls") as a *still-open* target for the next speed
tier, unsolved even by the native Turbo extension
(`turbo-ext/README.md:371-379`) - blocked behind a larger, unbuilt "native
expression engine" tier. phpstan-rs has the exact analogous hot path,
unmemoized: `Registry::lookup_in_hierarchy_with_owner`
(`src/registry/hierarchy.rs:127-150`) re-walks the full ancestor BFS from
scratch - fresh `HashSet`, fresh `VecDeque`, cloning every visited class
name - on every call, backing `lookup_method`, `lookup_property`,
`lookup_const`, `is_subclass_of` and more (`hierarchy.rs:12,157-169,399,
556-595`) - essentially every method call, property access, and subtype
check the checker performs anywhere. Because `Registry`'s class graph is
frozen right after `Project::finish_loading()` (`src/project.rs:277`), a
plain per-(class, member) memo table is an immediately buildable,
same-tier win real PHPStan's own profiling says it hasn't gotten to yet
even with a compiled extension in hand.

**Effort: medium.**

---

## D. New rule categories with zero PHPStan precedent

Not approximation gaps - PHPStan's entire `Rules/` catalog (Api, Arrays,
Cast, Classes, Comparison, Constants, DeadCode, Exceptions, Functions,
Generators, Generics, Methods, Missing, Names, Namespaces, Operators,
PhpDoc, Properties, Pure, Regexp, RestrictedUsage, TooWideTypehints,
Traits, Types, Variables, Whitespace) simply has no rule of these shapes at
all, confirmed by grep.

### D1. Function/method complexity and maintainability metrics

Zero PHPStan rule is keyed on a function/method body's own *shape* (branch
count, nesting, parameter count) rather than a type/existence/soundness
fact (`grep -rli 'complexity|cyclomatic|npath' vendor/phpstan-src/src/` -
nothing). phpstan-rs already has the traversal infrastructure for free: a
generic `stmt_children`/`expr_children` walk helper (`src/ast_walk.rs:39,
136`) and a single per-body entry point every function/method already
passes through (`src/checker/function_decl.rs`). Tallying decision points
(if/elseif/case/catch/`&&`/`||`/`?:`/loop conditions) for McCabe cyclomatic
complexity, max nesting depth, and parameter count (`FunctionSig.params.len()`,
`src/registry/function_sig.rs:96`) is additive to a walk that already
happens for control-flow analysis - exactly the "hardcoded function/match
arm" shape every existing phpstan-rs check takes, with a configurable
threshold mirroring how other opt-in thresholds already read from
`config.rs`.

**Effort: medium.**

### D2. Boolean-trap / ambiguous positional-literal-argument detection

PHPStan's argument-checking rules (`FunctionCallParametersCheck.php`,
`CallToFunctionParametersRule.php` machinery) only ever validate a passed
argument's *type* against the declared parameter type - a literal `true`
passed to a `bool` parameter is always type-correct, so
`setVisible(true, false)`-style calls (unreadable, easy to transpose) are
structurally invisible to that entire rule family; no rule anywhere
inspects literal-ness combined with parameter name/position ambiguity.
phpstan-rs already resolves the callee's full `FunctionSig` (per-parameter
types) at every call site (`src/checker/calls.rs`) and already has a
working literal-recognition helper reused elsewhere
(`literal_string_value`, used by the format-string checker,
`src/checker/calls/format_string.rs:2`) - flagging a bare literal
`true`/`false` passed positionally (not via named arguments) into a `bool`
parameter is additive to logic that already exists.

**Effort: small.**

### D3. Identical-branch / structurally-duplicate conditional-body detection

Neither `Rules/Comparison/` (`IfConstantConditionRule`,
`MatchExpressionRule`, `SwitchConditionRule`, etc.) nor `Rules/DeadCode/`
(`NoopRule`, `UnreachableStatementRule`, etc.) contains a rule comparing two
branch *bodies* for structural equality - every one evaluates a single
condition's possible truth values, never a pair of branches whose
conditions are both satisfiable but whose bodies happen to be identical
(`grep -rli 'identical|duplicate' vendor/phpstan-src/src/Rules/` - only
`DuplicateKeysInLiteralArraysRule.php`/`DuplicateFunctionDeclarationRule.php`,
both about *declarations*, never bodies). phpstan-rs's `Stmt`/`StmtKind`
AST (`src/ast.rs:24-40`) with its generic `stmt_children` walker
(`src/ast_walk.rs:136`) already threads through both `If` and
`Match`/`Switch` handling - a structural-equality comparison over that AST
(ignoring `Span`) is a local helper comparable to the recursive
shape-matching `is_switch_break_equivalent` already implements for a
different purpose in `src/checker/control_flow.rs`, not new architecture.
Almost always either dead code (the condition can never matter) or a
copy-paste bug where one branch was meant to differ.

**Effort: medium.**

---

## E. Ecosystem checks, now grounded via vendored `phpstan-strict-rules`

The first pass of this analysis dropped four proposals because their only
citation was to `phpstan/phpstan-strict-rules`, a real companion package
that wasn't vendored in this repo yet - every cited path resolved to
nothing on disk. `vendor/phpstan-strict-rules` is now added as a git
submodule (`.gitmodules`, matching the existing `vendor/phpstan-src`
precedent), so all four are re-verified here directly against the real
vendored source.

### E1. Non-boolean value used directly as a condition

`BooleanRuleHelper::passesAsBoolean`
(`vendor/phpstan-strict-rules/src/Rules/BooleansInConditions/BooleanRuleHelper.php:22-40`)
backs eight rules (`BooleanInIfConditionRule`, `BooleanInWhileConditionRule`,
`BooleanInDoWhileConditionRule`, `BooleanInElseIfConditionRule`,
`BooleanInTernaryOperatorRule`, `BooleanInBooleanAndRule`,
`BooleanInBooleanOrRule`, `BooleanInBooleanNotRule`) that flag a condition
whose *inferred type* isn't exactly `bool` (an `int`, `string`, nullable
object, etc. used directly as an `if`/`while`/ternary/`&&`/`||`/`!`
condition without an explicit boolean cast or comparison) - distinct from,
and not offered by, core PHPStan at any level. This is a genuinely
different check from phpstan-rs's existing `boolean_conditions.rs`, which
only ports core PHPStan's *constant*-condition family
(`ConstantConditionRuleHelper`/`BooleanAndConstantConditionRule`/
`BooleanOrConstantConditionRule` - flagging a condition whose value is
*always* true or *always* false, regardless of its declared type). A
condition that's genuinely `int`-typed and could be either truthy or falsy
at runtime is invisible to phpstan-rs's current check family entirely
(confirmed: `inferred_constant_condition` only ever matches
`Type::True`/`Type::False`, never "not `Type::Bool`"). phpstan-rs already
has every piece this needs in hand at the same call sites
`boolean_conditions.rs` already instruments (`Checker::in_condition_position`,
`infer_expr`) - it's a type-kind check (`!= Type::Bool`), not new
inference.

**Effort: small.**

### E2. `array_filter()` without an explicit callback, for an ambiguous element type

`ArrayFilterStrictRule::processNode`
(`vendor/phpstan-strict-rules/src/Rules/Functions/ArrayFilterStrictRule.php:52-147`)
flags a 1-argument `array_filter($x)` call (`arrayFilter.strict`) unless the
array's element type is provably boolean, provably array-valued, or a union
that splits cleanly into all-truthy/all-falsy members - i.e. unless PHP's
implicit loose-truthy cast is guaranteed safe. phpstan-rs already ports
core PHPStan's narrower `arrayFilter.empty` sub-check
(`src/checker/expr/array_ops.rs:395-439`) but explicitly declined to port
two siblings, `arrayFilter.same`/`arrayFilter.alwaysEmpty`, after corpus
verification caught two real false positives - both traced to gaps in this
project's own flow-sensitive variable-type tracking, not to the checks
themselves (`array_ops.rs:395-416`). `ArrayFilterStrictRule`'s truthy/falsy
union-splitting logic (lines 89-106) is the same shape of judgment that
tripped those two false positives, so this is real, buildable, and
genuinely absent from core PHPStan - but should not be picked up without
first addressing (or explicitly accepting the risk of re-triggering) the
same variable-type-tracking gaps that killed its siblings.

**Effort: medium**, and gate it on revisiting `array_ops.rs:395-416`'s two
documented false positives first.

### E3. A concrete constructor that never calls `parent::__construct()`

`RequireParentConstructCallRule::processNode`
(`vendor/phpstan-strict-rules/src/Rules/Classes/RequireParentConstructCallRule.php:29-68`)
flags a non-abstract `__construct()` (skipping traits, interfaces,
anonymous classes) that doesn't call `parent::__construct()`
(`callsParentConstruct`, recursing through statements and unwrapping `@`
error-suppression, lines 70-99) when a real, callable parent constructor
exists - walking the ancestor chain past any abstract/private/deprecated
constructor to find one (`getParentConstructorClass`, lines 105-131).
Core PHPStan has no equivalent (`grep -rli parentconstruct
vendor/phpstan-src/src/Rules/` - nothing). phpstan-rs's registry already
walks the ancestor chain for method resolution
(`Registry::lookup_ancestor_method_with_owner`,
`src/registry/hierarchy.rs:175`) and already reasons about
`parent::__construct()`'s property-initialization effect elsewhere
(`src/checker/uninitialized.rs:114`) - this rule is a direct, additive
combination of both: find the nearest real ancestor constructor via the
existing hierarchy walk, then check the current constructor's own body for
a `parent::__construct()` call, matching `RequireParentConstructCallRule`'s
own abstract/private/deprecated-skip logic.

**Effort: small.**

### E4. Inherited method whose declared name-casing mismatches its parent/interface

`WrongCaseOfInheritedMethodRule::findMethod`
(`vendor/phpstan-strict-rules/src/Rules/Methods/WrongCaseOfInheritedMethodRule.php:61-84`)
walks a class's parent and every implemented interface
(`processNode`, lines 25-59) and flags `method.nameCase` when an
overriding method's declared name differs only in case from the ancestor
method it overrides (PHP itself is case-insensitive for method calls, so
this is purely a readability/consistency signal, never a soundness one).
phpstan-rs already has a `.nameCase` diagnostic family, but only for
import/reference casing (`class.nameCase`,
`src/registry/name_context.rs:117`, `src/checker/resolve.rs:351`) - grep
confirms no inherited-*method*-name-casing check exists anywhere in the
checker. phpstan-rs's existing override-checking pass
(`check_method_overrides`/`check_one_method_override`,
`src/checker/oop/method_overrides.rs`) already visits every
overriding-method/ancestor-method pair via
`lookup_ancestor_method_with_owner` (`src/registry/hierarchy.rs:175`) for
signature-compatibility checking - a string case-comparison on the method
names already in hand at that call site is additive, not a new pass.

**Effort: small.**
