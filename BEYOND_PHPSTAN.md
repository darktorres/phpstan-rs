# Beyond parity

ROADMAP.md tracks the path to feature *parity* with real PHPStan. This document
is the opposite direction: concrete, source-grounded opportunities for
phpstan-rs to do things real PHPStan doesn't do, or can't do as well, once
parity work uncovered a real architectural reason why not. Every item below
was produced by three rounds of independent surveys (six lenses each, each
round instructed to avoid every prior round's territory and to read this
document in full before proposing anything) over `vendor/phpstan-src/src`
and `vendor/phpstan-strict-rules/src` (real PHPStan) and `src/` (phpstan-rs),
then adversarially re-verified against the actual cited files (not the
surveying agent's own claim) before being kept. Round 1: 14 of 18 survived;
the other 4 (section E) were provisionally dropped only because their
citation, `phpstan/phpstan-strict-rules`, wasn't vendored yet - it now is,
and all 4 are included. Round 2: 15 of 15 survived. Round 3: 11 of 12
survived; the twelfth is salvaged in corrected, narrower form at the end
rather than discarded, since its underlying premise held even though its
original citation didn't. Total: 45 confirmed findings across sections
A-N plus one corrected finding. Nothing here is scheduled; it's a grounded
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

---

# Round 2 and round 3

Two further survey rounds, each six lenses deliberately distinct from every
prior round (and from each other - round 3 was instructed to avoid round
2's six areas by name, and both were required to read this document in full
before proposing anything). Round 2: 15 of 15 raw proposals survived
verification. Round 3: 11 of 12 survived; the twelfth is salvaged below in
corrected, narrower form rather than discarded, since the underlying
technical premise held even though the original citation didn't.

### A6. Uniform int-to-float overflow promotion in range arithmetic - real PHPStan only got this right for one of four operators

phpstan-rs's `int_range_bounds`-based arithmetic (`BinOp::Add`/`Sub`/`Mul`,
`src/checker/expr.rs:3661-3763`) computes result bounds with plain native
integer math and never considers that PHP's real result silently promotes
to `float` once a bound exceeds `PHP_INT_MAX` - matching ROADMAP's own
"integer overflow/precision is out of scope by construction" boundary. But
real PHPStan's own analogous path, `InitializerExprTypeResolver::
integerRangeMath` (`InitializerExprTypeResolver.php:2321-2563`), is itself
inconsistent about this: only the Div branch unions in `new FloatType()`
on overflow (line 2533); the Plus (2363-2376), Minus (2377-2400), and Mul
(2427-2441) branches compute bounds with plain arithmetic and fall through
to a shared tail (2556-2563) that explicitly *discards* any float bound as
unbounded int rather than unioning it in. So for the common case of
adding/subtracting/multiplying two wide `int<min,max>` ranges, real
PHPStan's own static type silently claims a plain `int` even when the
runtime value could be a `float` - a genuine, confirmed inconsistency in
PHPStan's own arithmetic modeling (the Div branch proves the technique was
known and cheap), not a deliberate universal design choice. phpstan-rs's
range arithmetic is plain native Rust math with none of PHPStan's
combinatorial-cost pressure elsewhere in that same file (its
`CALCULATE_SCALARS_LIMIT = 128`-gated exact-value cartesian path is the
actual cost tradeoff there) - a single cheap `PHP_INT_MAX` boundary check
per bound, applied uniformly to all three operators, costs nothing extra
and closes a gap real PHPStan has never closed for its own three most-used
arithmetic operators.

**Effort: small.**

---

## F. LSP/editor-native capabilities real PHPStan structurally cannot offer

Real PHPStan ships no first-party LSP or interactive query API at all -
`FileAnalyser::analyseFile` (`Analyser/FileAnalyser.php:81`) is its sole
analysis entry point, always a whole file, always from a fresh CLI
invocation. phpstan-rs's LSP server already keeps a resident,
incrementally-updated `Project`/`Registry` (`src/project.rs:69-114`) - the
items below are gaps in already-built plumbing, not new architecture.

### F1. Live, flow-narrowed hover type for local variables (currently wired to `None`)

`Checker::probe_span`/`probe_hit` (`src/checker.rs:758-776`) already stash
a matched span's inferred type during an ordinary `check_expr` pass for
`MethodCall`/`PropertyAccess` receivers, and the identical span-match
pattern already fires for `ExprKind::Variable` (`src/checker/expr.rs:
1223-1245`) to populate `probe_var_provenance` - but that same call site
computes the variable's live `Type` right there (`locals.get(name)`,
`expr.rs:1241-1244`) and never stashes it into `probe_hit` too. `lsp.rs:
266-269` documents this exact gap in its own code comment. Real PHPStan's
only way to inspect an inferred type at a point is `PHPStan\dumpType($expr)`
(`Rules/Debug/DumpTypeRule.php:1-50`) - editing the analyzed source to
insert the call, then re-running the whole CLI. Wiring the same field for
`ExprKind::Variable` is a ~3-line addition at an already-instrumented call
site.

**Effort: small.**

### F2. Wire the already-typed `DiagnosticFix` plumbing into `textDocument/codeAction`

`Diagnostic`/`DiagnosticBuilder` already carry a fully-typed `fix:
Option<DiagnosticFix>` field shaped exactly like an LSP `TextEdit`
(`DiagnosticFix { span, replacement, description }`, `src/checker.rs:
342-354`, builder at `checker.rs:422-431`) - but no production call site
uses it (only a synthetic test, `src/checker/tests/diagnostic_identifiers.rs:
214`), and `lsp.rs`'s `ServerCapabilities` never advertises
`code_action_provider`, nor does `handle_request` have a
`textDocument/codeAction` arm. Real PHPStan's `RuleErrorBuilder::
fixNode()` (`RuleErrorBuilder.php:296-302`) feeds a wholly separate,
proprietary subsystem instead: `Command/FixerApplication.php`/
`FixerWorkerCommand.php`, labeled "(Internal) Support for PHPStan Pro" -
a paid-tier, signed-phar, out-of-process batch rewriter, never a
per-diagnostic in-editor quick-fix. phpstan-rs's `Diagnostic` already
models the fix in exactly the shape an LSP `WorkspaceEdit` needs, computed
in the same in-process pass that raised the diagnostic - only the emission
(per-check) and the LSP-side exposure are missing. Pairs naturally with
section I below, which lists exactly which existing diagnostics are safe
to populate `.fix(...)` for first.

**Effort: small** (the wiring); populating individual checks is section I.

### F3. Whole-project "Find All References" / rename-safe refactoring, on a new use-site span index

Neither `textDocument/references` nor `textDocument/rename` exists in
`handle_request` (`src/lsp.rs:548-625`), and the data doesn't fully exist
yet either: `Project.dependents` (`project.rs:71`) tracks only *which
files* reference a bare symbol name (file granularity, for re-check
invalidation, built from `deps::referenced_names`'s flat `HashSet<String>`
per file, `src/deps.rs:1-30`); `DeclLocations` (`project/decl_locations.rs:
18-22`) stores only one declaration-site span per symbol, never use-site
spans. Real PHPStan has no reference-finding or rename capability of any
kind (no LSP module anywhere in `vendor/phpstan-src/src`; its dependency
graph exists only to feed cache invalidation, never a queryable "every use
of X" index). A new, additive per-file use-site index, resolved through
the same `NameContext`/registry hierarchy walk `resolve_member_owners`
already uses for hover/definition (`lsp.rs:81-110`), would let
`references` enumerate every real use across the whole project and
`rename` batch those into one `WorkspaceEdit` - safe specifically because
it resolves each use through the live, whole-project `Registry`
(inheritance-aware, same-name-different-class disambiguation) rather than
a textual grep. The resolution logic already exists and is already
exercised by `definition`; only the use-site span recording is missing.

**Effort: large.**

---

## G. Finer-grained incremental re-check than real PHPStan's file-level result cache

### G1. Sub-file (per-function) real-time re-check on `didChange`

Every `didChange` today unconditionally calls `Project::update_file`
(`src/lsp.rs:169-206, 638-650`), which re-parses the *whole file* and
re-runs a fixed sequence of whole-registry passes (`project.rs:356-363`)
on every keystroke, even when the edit stays inside one function body and
touches no declaration at all. Yet phpstan-rs already has, and already
uses for hover, a mechanism that type-checks an isolated function against
the frozen registry with none of that rebuild: `run_probe_scope`'s
`ProbeScope::Function` arm calls `checker.check_function(...)` directly
(`lsp.rs:363-378`) with no re-parse and no registry re-indexing. Real
PHPStan has no notion of checking less than a whole file at all -
`FileAnalyser::analyseFile` is the sole entry point, and even its own
result cache invalidates at file granularity only
(`ResultCacheManager::restore`/`process`, `ResultCacheManager.php:
440-602`), read from disk once per whole CLI invocation, never queried
continuously from a resident process. Detecting that an edit's diff falls
entirely within one function's span and doesn't add/remove/rename a
declaration, then re-running just `check_function` over that function
against the still-valid registry (falling back to full `update_file` on
save or any declaration-shaped edit), gives sub-file, sub-registry-rebuild
latency for the dominant editing case.

**Effort: large.**

### G2. Per-symbol signature-shape fingerprinting to skip re-checking unaffected dependents

Give every declared symbol a cheap fingerprint covering only its
externally-visible signature (param types/names/variadic/by-ref/defaults,
return type, visibility, ancestry), explicitly excluding `FunctionSig::
body` (already correctly excluded from equality reasoning per that
struct's own doc comment, `src/registry/function_sig.rs:214-233`). In
`Project::update_file` (`project.rs:305-388`), instead of unconditionally
seeding `changed_symbols` with every name the file declares regardless of
whether its shape actually differs (lines 321-322, 341-342), seed it only
with names whose fingerprint changed - `transitive_affected_files`'s
existing per-symbol BFS (`project.rs:404-448`, already keyed by bare name
via `dependents: FxHashMap<String, HashSet<PathBuf>>`, `project.rs:71`)
then naturally narrows to only the dependents of the specific symbol(s)
whose shape moved. Real PHPStan already does something similar but only
at whole-*file* granularity, and its own data structures make finer
resolution structurally impossible without a rewrite:
`ResultCacheManager::exportedNodesChanged` (`ResultCacheManager.php:
684-717`) correctly skips a file's dependents on a body-only edit, but the
invalidation edge is stored per *file* (`NodeDependencies::
getFileDependencies` explicitly dedups by `$dependencyFile`,
`NodeDependencies.php:37-64,60`) - so if a file declares two classes and
only one's signature changes, PHPStan reanalyzes *all* dependents,
including ones that only reference the untouched class, because the
stored edge can't say which declaration a dependent came from. This is
architecturally forced by PHPStan's per-run, discarded-and-rebuilt
dependency graph, not a missed optimization; phpstan-rs's own doc comment
already names this exact gap as future work ("straightforward to tighten
later", `project.rs:67-68`).

**Effort: large.**

---

## H. Whole-program value-flow diagnostics (not existence/usage - distinct from section B)

Section B above reuses the `dependents` graph for *existence*/dead-code
questions. These reuse the same whole-project registry-fixed-point
architecture (Phase 41's purity model, cited throughout section B) for
questions about the *values* flowing through the program instead.

### H1. "Dead flexibility" parameter detection (always the same literal argument, project-wide)

A registry-build-time pass resolves every call's arguments to declared
parameters via the same `resolve_call_args` helper `checker/calls.rs`
already calls per-site (`calls.rs:1033`), records each resolved argument's
already-inferred literal `Type` (`Type::IntLiteral`/`StringLiteral`/etc.,
`src/types.rs:32,35`, produced by `infer_const_type`,
`function_sig.rs:278-293`) into a project-wide
`HashMap<(callee, param_index), (site_count, distinct_values)>`. A
parameter with 2+ call sites where every single site resolves to the exact
same literal gets a diagnostic at its declaration - it's never actually
varied, a candidate to hardcode or drop. Real PHPStan's only
cross-call-site aggregation mechanism is its 16-collector machinery (`find
vendor/phpstan-src/src -iname '*Collector*'`), and the one real example
close to this shape, `NamedArgumentParameterMethodCallsCollector.php`
feeding `MethodCallWithPossiblyRenamedNamedArgumentRule.php:29-71`, only
aggregates named-argument *parameter names*, never argument *values* -
confirming no PHPStan rule anywhere reasons about whether an argument's
literal value is constant across every call.

**Effort: large.**

### H2. Whole-program inferred "effectively NoDiscard" detection

Extend the existing per-call-site `Checker::in_bare_statement_position`
signal with a project-wide aggregation: for every call to a
project-defined, non-void-returning function/method, record whether that
call's value was discarded. A callee with 2+ recorded sites where the
value was discarded at every single one gets a diagnostic at its
declaration - the fact a real `#[\NoDiscard]` attribute would assert,
discovered from observed usage instead of requiring the author to
hand-annotate it (the same move BEYOND_PHPSTAN's own A5 already makes for
sealed hierarchies). Real PHPStan's only general "flag a discarded return
value" mechanism is the real PHP 8.5 `#[\NoDiscard]` attribute, checked
purely from the callee's own declared attribute
(`CallToMethodStatementWithNoDiscardRule.php:21-70` and siblings) - never
from how callers actually behave; its one other "useless return value"
rule (`UselessFunctionReturnValueRule.php:25-28,42-49`) is hardcoded to
exactly three stdlib functions with a known-constant return value, not a
general or inferred mechanism. phpstan-rs already has every per-site
primitive this needs: `FunctionSig.no_discard`
(`registry/function_sig.rs:120-130`) is exactly the bit an inferred
version would set automatically, and `check_useless_return_value_call`
(`checker/expr/array_ops.rs:342-354`) already answers "is this exact
call's result unused" per occurrence.

**Effort: medium.**

### H3. Whole-program property-type narrowing from every assignment site

A project-wide pass walks every `$this->prop = rhs` assignment across
every file that declares or extends a given class (via the registry's
subclass-aware hierarchy, the same walk section B1 already proposes
extending for member *usage*), recording the RHS type `checker/expr.rs`'s
`ExprKind::Assign` arm already infers at that site (`expr.rs:1486`)
immediately before `check_property_assignment` (`expr.rs:1509`). If every
recorded site project-wide is a strict subtype of the property's declared
type (e.g. a `string $status` property only ever assigned one of 3 known
literals anywhere in the codebase), flag the declaration suggesting the
type could be narrowed to a literal union or backed enum. Real PHPStan's
property-assignment checking only ever validates one site at a time
against the declared type - no rule or collector intersects the RHS types
seen across every assignment site project-wide (the same 16-collector
inventory confirms nothing is collected for property-assignment values).

**Effort: medium.**

---

## I. Codemods attached to diagnostics phpstan-rs already ships

Four concrete fixes, each attaching to an existing (or, for one, an
E-section-proposed) diagnostic via the `DiagnosticFix` plumbing section F2
wires up. Real PHPStan's entire autofix surface (`RuleErrorBuilder::
fixNode`) is used by only 4 rule files project-wide (`grep -rl fixNode
vendor/phpstan-src/src/Rules/` -> two `Overriding*Rule.php`, `Operators/
BacktickRule.php`, `Playground/PromoteParameterRule.php`) - none of the
diagnostic families below have any real-PHPStan autofix at all.

### I1. Byte-exact same-length splice for every `*.nameCase` mismatch

phpstan-rs already reports `class.nameCase`/etc. (`src/checker/resolve.rs:
341-352`) and already knows the canonical casing via `NameContext`'s
use-import table (`registry/name_context.rs:100-124`). Because identifier
casing changes never change byte count, this is the safest codemod shape
possible: overwrite `[span.start, span.end)` with the canonical-cased
string in place, zero length delta, zero downstream-span invalidation.
Scales directly to E4's not-yet-built inherited-method-casing check with
the same mechanism. Real PHPStan's nameCase rule family has zero
`fixNode` usage anywhere.

**Effort: small.**

### I2. Delete-attribute-group codemod for the already-shipped invalid-`#[Override]` diagnostic

`method.override`/property equivalent already fire when `#[Override]` is
present with no real ancestor (`checker/oop/method_overrides.rs:137-149`,
`property_overrides.rs:100-104`). The fix is a pure span deletion of the
attribute group's own source text - no reprinting of the declaration
needed. Real PHPStan's fix for the identical rule instead rebuilds the
*whole* node via `filterOverrideAttribute` and reprints it through
PhpParser's printer (`OverridingMethodRule.php:119-124`,
`Fixable/Patcher.php:1-80`) - a whole node-reprint-and-merge pipeline built
because PHPStan's AST has no reliable way to splice just the attribute
group's range. phpstan-rs's attribute-group node already carries its own
span independent of the declaration span (`src/ast.rs:178-198`), so the
fix is strictly smaller and safer than PHPStan's own approach to the
identical rule.

**Effort: small.**

### I3. Whole-declaration deletion for the zero-usage subset of unused-private-member diagnostics

`classConstant.unused`/`method.unused`/`property.unused`
(`checker/unused.rs:184-212,293-298`) mean the member is referenced
*nowhere at all* - safe to delete the full declaration span, unlike the
sibling `property.neverRead`/`property.onlyWritten` identifiers
(`unused.rs:299-309`), which must NOT be auto-deleted since the member is
still genuinely used somewhere. Real PHPStan's `UnusedPrivate*Rule` family
has zero `fixNode` usage. The one real nuance: a preceding doc-comment/
attribute-group isn't included in the declaration span
(`src/ast.rs:194-198`), so a correct codemod must walk backward past any
immediately-preceding doc-comment text before deleting.

**Effort: medium.**

### I4. Conditional single-line insertion codemod for E3 (`parent::__construct()` never called)

Once E3 ships, the fix is inserting `parent::__construct();` as the
constructor body's first statement - but only when the resolved parent
constructor's `FunctionSig.params` has zero required entries; otherwise
the correct call needs argument values the checker can't invent, and the
codemod must decline rather than guess. `phpstan-strict-rules` has zero
`fixNode` usage anywhere (`grep -rl fixNode vendor/phpstan-strict-rules/src/`
- empty), confirming this fix was never attempted upstream despite being
as mechanically simple as I2's.

**Effort: small** (once E3 itself ships).

---

## D (continued). More new rule categories with zero PHPStan precedent

### D4. Narrow, single-function-scoped taint check: superglobal input reaching a dangerous sink

Within one function/method body only (no interprocedural or
library-boundary tracking), flag a value syntactically traceable back to
`$_GET`/`$_POST`/`$_REQUEST`/`$_COOKIE` (through same-function plain
reassignment/concatenation/interpolation, no recognized sanitizer)
reaching `eval`, `system`/`exec`/`shell_exec`/`passthru`/`popen`/
`proc_open`, `unserialize`, or a non-literal `include`/`require` path.
Real PHPStan has zero taint infrastructure - grepping both vendored trees
case-insensitively for "taint" turns up only substring false-positives
(e.g. "uncer-TAINTY"); the closest adjacent mechanism, `RestrictedUsage`'s
outright function-name banning, has no notion of data provenance at all,
so it can't distinguish `eval($safe_constant)` from `eval($_GET['x'])`.
phpstan-rs already has every structural piece a narrow, same-function
version needs cheaply: superglobals are already a recognized, named set
(`is_superglobal`, `checker/assignment.rs:377-392`); `eval`/`include`/
`require` are already special-cased at one call site
(`checker/expr.rs:194-198`); the per-function `Locals` map already carries
a precedent second persistent side-channel alongside its type map (the
existing `provenance: ProvenanceMap` field, `checker.rs:107-131`, built
for LSP go-to-definition). Be honest about false-positive risk in the
writeup when this ships - a narrow, single-function-scoped version is much
safer than an interprocedural one, and should stay that way.

**Effort: medium.**

### D5. Validate `@see` PHPDoc tag references actually resolve to a real symbol

When a docblock carries `@see SomeClass::someMethod()` (or `@see
\Fqcn\Class`, `@see functionName()`), resolve the referenced element
through the registry the same way an ordinary code reference is resolved;
emit `phpDoc.seeTagInvalidReference` when it doesn't resolve anywhere in
the closed-world project+vendor universe - the "renamed the method,
forgot the `@see` cross-reference" drift that has zero tooling today.
Real PHPStan's underlying `phpstan/phpdoc-parser` library has no
`SeeTagValueNode` at all, so `@see` falls through to a fully generic,
unstructured tag node - not an approximation PHPStan chose for
architectural reasons, the capability to check it simply doesn't exist
upstream (even its structurally closest tag, `@deprecated`, only captures
a free-text description with no parsed replacement-symbol field,
`PhpDoc/Tag/DeprecatedTag.php:9-19`). phpstan-rs already has a clean,
single dispatch point for a new structured tag: `DocBlock`'s tag-name
match arms in `src/docblock.rs` (`"deprecated"` at line 728, `"throws"` at
731, `"param"`/`"return"`/`"var"` at 451/482/502) - a `"see"` arm is
additive to a pattern already used for every other tag, and symbol
resolution against the registry is infrastructure the checker already
runs for real code references. (Deliberately targets `@see`, not `@uses`
- PHPStan's own `@uses` already means something different, a `Type`
checked above a `use Trait;` statement.)

**Effort: small.**

### D6. Flag adjacent same-typed positional arguments whose variable names suggest a swapped call order

At a resolved call site, when two adjacent parameters share the same type
(e.g. `function resize(int $width, int $height)`) and the corresponding
positional arguments are plain variables/properties whose names are a
case-insensitive/substring match for a *different* parameter than the one
they landed on (`resize($height, $width)`), emit a low-confidence, opt-in
`argument.possiblySwapped` diagnostic - the classic "both correct types,
wrong order" copy-paste bug. `FunctionCallParametersCheck.php` (the shared
engine behind every argument rule) resolves and checks each argument
purely by *type* - grepping it for any variable-name-vs-parameter-name
comparison for ordinary positional arguments returns nothing; the only
name-based matching anywhere in PHPStan's rules is for *named* arguments,
which are correct by construction. PHPStan's type system has no way to
represent "these two `int` parameters are semantically different roles",
so a type-sound call with swapped same-typed arguments is invisible to
its entire architecture, not merely unimplemented. phpstan-rs already
resolves the full `FunctionSig` (`ParamSig { name, ty, .. }`,
`registry/function_sig.rs:24-30`) at every call site and matches
positionally (`calls.rs:369,509,751`); `ExprKind::Variable(String)`
already exposes the argument's own source-level name with no extra
plumbing. Distinct from D2 (boolean-trap): D2 only concerns a bare literal
`true`/`false`; this concerns two *variables* of a shared non-bool type
whose *names* suggest transposition - D2 is inert once both arguments are
variables.

**Effort: medium.**

### D7. Flag structurally-identical operands on both sides of a comparison, subtraction/division, or self-assignment

Detect `$x === $x`, `$a - $a`, `$a / $a`, `$a % $a`, `$a ^ $a`, `$a = $a`
where both operands are the exact same syntactic expression (same
variable, or same property/array-index chain with no intervening side
effects) - almost always a copy-paste bug where one side should reference
a different variable, and for `-`/`/`/`^` a redundant computation that
always yields the same constant regardless of runtime value. PHPStan's
`Rules/Comparison/` family reasons entirely from each operand's inferred
*type*, never from whether the two operand *expressions* are the same
syntax tree - `$x === $x` with `$x: mixed` is type-sound on both sides and
produces no diagnostic; `phpstan-strict-rules`' `Rules/Operators/` family
has the same gap (numeric-strictness only, never identity-driven).
phpstan-rs already threads a recursive, `Span`-ignoring structural-match
helper through exactly this kind of AST-shape check for a related purpose
(`is_switch_break_equivalent`, `checker/control_flow.rs:48-64`) - the same
technique one level down, over `ExprKind::Binary`/`Assign`
(`src/ast.rs:614-616`) via the generic `expr_children` walker
(`ast_walk.rs:39`), gives a reusable `exprs_structurally_equal` with no
runtime-value dependency. Distinct from D3 (duplicate *branch bodies*,
`Stmt`-level): this operates one AST level down, on a single expression's
own two operands.

**Effort: medium.**

---

## J. Config/strictness composability beyond PHPStan's single global level

Real PHPStan's parameters (including every opt-in rule) resolve exactly
once into one frozen DI container for the whole run
(`ContainerFactory::create`, `DependencyInjection/ContainerFactory.php:
102-170`); rule registration is queried only by AST node type, never by
file path (`Rules/Registry.php:9-15`, `Rules/LazyRegistry.php:36-53`) - a
rule is on-or-off for literally every file in a run. The only path-aware
mechanism anywhere in its config schema is `ignoreErrors[].path`/
`excludePaths`, strictly subtractive (suppress an already-computed error,
or skip a file entirely) - it can never turn a rule ON for a subset that's
off elsewhere. phpstan-rs has no DI container to rebuild per invocation
and no parallel-worker-process model to keep consistent - one `Registry`
is built once (`Project::finish_loading`, `project.rs:277`) and a fresh,
cheap `Checker` is constructed per file (`Project::check_file`,
`project.rs:453`) that already carries per-file state alongside the
shared `&Registry`.

### J1. Per-directory additive opt-in checks

phpstan-rs's own `Config` (`src/config.rs`) already has opt-in bool flags
(`check_uninitialized_properties`, `check_internal_usage`, etc.) that flow
into single `Registry` bool fields, read uniformly at every check site.
Resolving a small path-pattern list once at load time into a per-file-path
override table, consulted before falling back to the project-wide bool,
costs nothing architecturally - no new dispatch mechanism, no re-running
rule registration, just one extra lookup at the point `check_file` already
builds a `Checker` for that path. Real ADDITIVE per-directory strictness
(a stricter check ON only in one subtree) that PHPStan's frozen-container
model has no path to support at all.

**Effort: medium.**

### J2. Per-directory strictness tiers in one run, with cross-directory calls still checked consistently

Splitting a codebase into a stricter `src/New/` and looser `src/Legacy/`
in real PHPStan requires either one level for everything, or two entirely
separate invocations with no shared registry and no cross-directory
soundness (a call from legacy into new code isn't checked against the new
code's stricter rules at all, since level-gated rules attach to the file
being *visited*, not the callee's declared strictness) -
`CommandHelper::begin` (`Command/CommandHelper.php:246-310,401`) resolves
`$level` as one scalar for the entire container build, with no code path
reading a per-path level list. phpstan-rs's single, always-shared
`Registry` means a file-path-keyed tier lookup (generalizing J1's
override table from on/off to a small set of named tiers) doesn't require
duplicating or re-resolving any graph - the whole project, including
cross-directory calls, stays in one pass with one shared registry.

**Effort: large.**

---

## K. Composer declared-vs-used dependency hygiene

Three diagnostics, all keyed off resolving "which composer package owns
this symbol": `unused.composerDependency` (a `require` entry contributing
zero symbols actually referenced in first-party code - only reachable
transitively, or a stale leftover), `undeclared.composerDependency`
(first-party code resolves to a `vendor/<pkg>/` file whose package isn't a
`require`/`require-dev` key at all - only present because some *other*
required package happens to pull it in transitively, so an unrelated
`composer update` can silently break the build), and
`misplaced.composerDependency` (a package declared only under
`require-dev` whose symbols are referenced from non-test application code
- breaks under a production `composer install --no-dev`). PHPStan's own
Rules/ catalog has no package-declaration-aware check at all (grepping
both vendored trees for "composer" turns up only unrelated API/bootstrap
code); this is real and structural - PHPStan analyzes one process per run
with no persistent whole-program "which package provides this symbol"
index kept live to query against composer.json's declarations, and
building one would duplicate work its own architecture discards after
each run. phpstan-rs already has every piece independently built for
other reasons: `composer::find_composer_package_name`
(`src/composer.rs:144-155`) already walks a file's directory upward to
its owning composer.json and returns its package name (built for a
different Phase 40.6 rule);
`composer::detect_phpstan_dependency` (`composer.rs:112-127`) already
parses `require`/`require-dev`; `deps::referenced_names`
(`src/deps.rs:26-32`) already collects every name a file references;
`Project`'s `DeclLocations` (`project/decl_locations.rs:18-22`) already
maps a declared FQN to its owning file - currently gated behind
`retain_source` for LSP-only use (`project.rs:90-99`) rather than
populated for batch `check-project` runs, but the same data.

**Effort: large.**

---

## M. Type-system representation lessons from real PHPStan's own accreted BC debt

Not performance tradeoffs (section A/C) or missing features - real
PHPStan's own source code admits, in its own comments, three places where
an early design decision now has to be permanently carried forward for
users it already shipped to. phpstan-rs has no installed userbase to
protect and can simply not repeat the mistake; each item below is a
forward-looking policy recommendation, not a bug to fix.

### M1. One unsealed-array-shape representation, never a null-vs-explicit-marker duality

PHPStan's `ConstantArrayType::$unsealed` is `?array{Type,Type}`, defaulting
to `null` (ambiguous, `isUnsealed()` answers `Maybe`) for non-bleeding-edge
users or an explicit `[NeverType(true), NeverType(true)]` marker for
bleeding-edge users (`ConstantArrayType.php:176-192`) - because flipping
every existing user's array-shape comparison semantics in one release was
judged too disruptive, so the toggle exists to opt in gradually. Its own
`equals()` documents the fallout directly (lines 937-943): a legacy-null
shape and a marker-sealed shape that mean the same thing compare unequal,
forcing a permanent second code path (`legacyIsKeysSupersetOf` at line
2934, `legacyMergeWith` at 3112, both still invoked whenever either side
is legacy-null) that ripples into consumer code too
(`ArrayMergeFunctionDynamicReturnTypeExtension.php:126-136` explicitly
checks `isUnsealed()->maybe()` to decide whether it may even attempt the
newer result shape). phpstan-rs's `ArrayShape` already carries
unsealedness as one in-band sentinel field (a literal `"..."` entry,
`src/types.rs:55`, tested by a 3-line `is_unsealed_shape`,
`checker/narrowing/array_shape.rs:23-25`) - the recommendation is to keep
it that way as deliberate policy when the tracked unsealed-shape
superset/merge reasoning (A3's territory) is actually built out: implement
it once, unconditionally, never add a nullable "ambiguous/legacy" tier
alongside an explicit marker, since there's no prior release's comparison
behavior to preserve bit-for-bit.

**Effort: small** (a standing policy, not a build item on its own).

### M2. Keep the `Type` layer closed - no public trait, so old accessor names can be deleted instead of deprecated-and-kept-forever

PHPStan's `Type` interface, implemented by every concrete type class *and*
by any third-party extension's custom Type class (it's `@api`), still
declares 7 `@deprecated`-tagged methods every implementor must fully
implement, because an interface method can't be made optional and removing
one breaks anyone implementing `Type` directly (`Type.php:129-136,
186-197`; confirmed still fully implemented in `ObjectType.php`,
`UnionType.php`, `ConstantArrayType.php`). Deprecation buys no removal
here - the debt is permanent by construction, not a cleanup backlog.
phpstan-rs's `Type` is a closed Rust `enum` (`src/types.rs:22-23`) matched
exhaustively by free functions, never a public trait outside code
implements, so a rename/removal is a same-repo refactor, never a
permanent API-surface commitment - already the practice in this codebase
(`Registry::lookup_property`/`lookup_property_with_owner` etc.,
`registry/hierarchy.rs:157,163,556,563,573,583`, no deprecated sibling
kept alongside the current one). The concrete forward policy: keep
declining to expose the type/registry layer as a public trait for
external plugins, matching ROADMAP's own Won't-port precedent on
DI/plugin architecture, so this debt category structurally cannot begin.

**Effort: small** (a standing policy).

### M3. One canonical member order for Union/Intersection types, never a describe()-only sorted cache alongside it

PHPStan's `UnionType`/`IntersectionType` must keep both a canonical
`$types` array (whose order is part of the type's own value - `getTypes()`
callers merge array shapes in that order) and a separate
`$sortedTypesCache` used only for human-readable `describe()` output,
because an earlier version sorted `$types` in place inside `describe()`,
silently corrupting later `getTypes()` calls - both classes' own code
comments admit this outright (`UnionType.php:64-70`,
`IntersectionType.php:79-85`: "It used to be sorted in place... making an
immutable value object's observable state depend on what had been called
on it before"). phpstan-rs's `Type::Union`/`Intersection` are plain
`Vec<Type>` (`src/types.rs:121`) whose `Display` impl
(`types/display.rs:106-116`) takes `&self` and never writes back - there
is exactly one order everywhere, enforced by ordinary Rust immutable-borrow
semantics rather than by discipline. Forward policy: if union-display
readability is ever improved, do it by collecting into a locally sorted
`Vec` inside the `Display` call only, never by mutating the stored
`Vec<Type>` itself.

**Effort: small** (a standing policy).

---

## N. Baseline robustness

Real PHPStan's baseline is not actually unbounded, and phpstan-rs's
currently is - both gaps are concrete and fixable inside `src/baseline.rs`'s
existing data flow, without touching `Diagnostic` or any rule.

### N1. Bound baseline matches by recorded occurrence count

Give `baseline::Entry` a `count: usize` field and make `contains`/`filter`
consume matches up to that count per run, letting any occurrence beyond it
fall through as a new diagnostic. `generate()` already builds one Entry
per diagnostic including duplicates (`src/baseline.rs:74-84`) - the count
information already flows through the pipeline and is only thrown away
when `load()` collapses the Vec into a `HashSet<Entry>` (lines 100-105).
Real PHPStan's baseline works at all specifically because it tracks a
running count per ignore entry and un-suppresses anything past it
(`IgnoredErrorHelperResult.php:100-117,292-314`) - without this,
phpstan-rs's baseline can silently absorb unlimited *new* occurrences of
the same message at the same location forever, masking copy-pasted new
instances of an already-baselined bug.

**Effort: medium.**

### N2. Opt-in identifier-anchored baseline match mode, surviving cosmetic message-text changes

An alternate match mode keyed on `(file, location, identifier)` instead of
`(file, location, message)`, so a baselined finding keeps matching after
the rendered message text changes (a symbol rename, a wording tweak from
upgrading phpstan-rs). Real PHPStan's own baseline generator never offers
this: `BaselineNeonErrorFormatter::formatErrors` always attaches the
exact-text message as an anchored regex alongside the identifier
(`ErrorFormatter/BaselineNeonErrorFormatter.php:75-98`), and
`IgnoredError::shouldIgnore` ANDs both conditions - the coupling is fixed
generator policy, not a user toggle. phpstan-rs's `Diagnostic` already
carries a dedicated, stable `identifier` field, documented as deliberately
stable across occurrences unlike `message` (`src/checker.rs:302-311`) -
the exact anchor this mode needs already exists on every diagnostic today
for free; `baseline.rs`'s `Entry` (`baseline.rs:27-32`) just doesn't read
it yet.

**Effort: medium.**

### N3. Detect and report stale (unmatched) baseline entries

Track which baseline entries were actually consumed during `filter()`'s
existing pass, and expose the complement (entries matched by nothing this
run) as an opt-in report - mirroring real PHPStan's
`reportUnmatchedIgnoredErrors` (on by default:
`IgnoredErrorHelperResult::process()` walks every unconsumed ignore entry
and emits `ignore.unmatched`, `IgnoredErrorHelperResult.php:319-370`,
specifically because ignore/baseline entries otherwise accumulate silently
forever after the underlying bug is fixed and nobody notices). No such
code path exists anywhere in `baseline.rs`/`main.rs`/`check_project.rs`
today. `Baseline` already holds every loaded entry in memory and `filter()`
already visits every diagnostic against it in one pass - recording match
hits alongside the existing lookup is a small, local addition to that same
pass.

**Effort: small.**

---

## Corrected from round 3: `method.notFound` specifically has no tip mechanism in real PHPStan (narrower than first proposed)

Round 3 originally proposed whole-program "did you mean" tips for
undefined method/property/class/function/constant errors generally,
citing that neither `AccessPropertiesCheck.php` nor `ClassNameCheck.php`
attaches a tip anywhere. Verification found that claim false for 3 of the
4 categories: `AccessPropertiesCheck.php` *does* call `->tip(...)`
directly in its `property.notFound` branch (a computed tip when available,
else a fixed help-URL), and `class.notFound`/`function.notFound` sites
overwhelmingly attach `RuleErrorBuilder::discoveringSymbolsTip()`
(`RuleErrorBuilder.php:202-205`, a fixed "Learn more" URL) via
`RuleLevelHelper.php:245-246` and ~25 other call sites. Re-checked
directly here, independent of the workflow: `grep -n "tip(" vendor/
phpstan-src/src/Rules/Methods/MethodCallCheck.php` returns nothing, and
its `method.notFound` builder (line 144) is a bare
`->identifier('method.notFound')` with no tip call at all - confirmed
clean, matching phpstan-rs's own `method.notFound` sites
(`src/checker/expr.rs:1030-1036,2837-2842`, plain `self.report(...)`, no
`.tip`). So the real, narrower opportunity: a *computed* edit-distance
suggestion (over the already-resident whole-project `Registry` -
`Registry.classes: FxHashMap<String, ClassInfo>`, `src/registry.rs:38`;
`ClassInfo.methods`, `registry/class_info.rs:151`) specifically for
`method.notFound`, using the already-built but mostly-unused
`DiagnosticBuilder::tip()` (`src/checker.rs:407`). class/function/property
already have *some* tip (static, not computed); method genuinely has
none, in either codebase.

**Effort: small.**
