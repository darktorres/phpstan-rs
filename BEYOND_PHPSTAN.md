# Beyond parity

ROADMAP.md tracks the path to feature *parity* with real PHPStan. This
document is the opposite direction: concrete, source-grounded opportunities
for phpstan-rs to do things real PHPStan doesn't do, or can't do as well,
once parity work uncovered a real architectural reason why not. Every item
below cites the actual PHPStan source it's grounded in
(`vendor/phpstan-src/src`, `vendor/phpstan-strict-rules/src`) and the actual
phpstan-rs source (`src/`) that would implement it - every citation has been
checked directly against the vendored/working trees, not taken on an
agent's word. 100 findings total, grouped below by what they're actually
about rather than by when they were found. Nothing here is scheduled; it's
a grounded idea backlog, same status as ROADMAP's own "Newly identified
gaps" until triaged into a numbered phase.

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

Nine parts:

1. Precision, whole-program analysis, and closed-world inference
2. Native-performance wins real PHPStan needs a separate extension for
3. New rule categories and design-pattern suggestions
4. Ecosystem: `phpstan-strict-rules` ports and `Won't-port` corrections
5. LSP / editor-native capabilities
6. Codemods
7. Config/strictness composability
8. Composer/dependency hygiene
9. Type-system policy, baseline robustness, reverted-feature fixes, API diffing

---

# Part 1: Precision, whole-program analysis, and closed-world inference

Sections A through P below share one root cause: phpstan-rs analyzes one
closed-world project+vendor universe in a single process with one frozen
`Registry`; real PHPStan must stay sound for arbitrary downstream consumers
of a library it's analyzing, and its per-file reflection is fixed before a
function's body is even walked. Every finding here is either PHPStan
approximating for that reason where phpstan-rs doesn't have to, or a
whole-program fact PHPStan's own architecture (14-collector
`CollectedDataNode` machinery, per-run discarded dependency graphs,
parallel worker processes with no shared memory) makes expensive to compute
that phpstan-rs's single live `Registry` gets nearly for free.

## A. More precise than real PHPStan itself

Real PHPStan approximates in these spots for its own architectural
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
cannot. (The same reverse index is reused directly by P1, U1, and AJ
below.)

**Effort: large.**

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
path. (AJ below reuses the same graph for a narrower, class-specific case:
an abstract class that's referenced but never subclassed concretely.)

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

## H. Whole-program value-flow diagnostics

Distinct from section B above: B reuses the `dependents` graph for
*existence*/dead-code questions. These reuse the same whole-project
registry-fixed-point architecture (Phase 41's purity model, cited
throughout section B) for questions about the *values* flowing through the
program instead.

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
hand-annotate it (the same move A5 already makes for sealed hierarchies).
Real PHPStan's only general "flag a discarded return value" mechanism is
the real PHP 8.5 `#[\NoDiscard]` attribute, checked purely from the
callee's own declared attribute (`CallToMethodStatementWithNoDiscardRule.php:
21-70` and siblings) - never from how callers actually behave; its one
other "useless return value" rule
(`UselessFunctionReturnValueRule.php:25-28,42-49`) is hardcoded to exactly
three stdlib functions with a known-constant return value, not a general or
inferred mechanism. phpstan-rs already has every per-site primitive this
needs: `FunctionSig.no_discard` (`registry/function_sig.rs:120-130`) is
exactly the bit an inferred version would set automatically, and
`check_useless_return_value_call` (`checker/expr/array_ops.rs:342-354`)
already answers "is this exact call's result unused" per occurrence.

**Effort: medium.**

### H3. Whole-program property-type narrowing from every assignment site

A project-wide pass walks every `$this->prop = rhs` assignment across
every file that declares or extends a given class (via the registry's
subclass-aware hierarchy, the same walk B1 already proposes extending for
member *usage*), recording the RHS type `checker/expr.rs`'s
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

## U. Closed-world whole-program inference replacing PHPStan's per-hop, doc-tag-only gaps

Six cases with an identical shape: real PHPStan resolves a fact purely
from an author-written PHPDoc tag on the *immediate* declaration, with no
fallback when the tag is missing, because its per-file reflection is
fixed before the body is even walked (no persisted whole-program inferred
summary can feed a caller). phpstan-rs already proves this exact
fixed-point shape works for a different fact (Phase 41's transitive
purity) and could apply it to all six.

### U1. Resolve `method_exists()`/`property_exists()` narrowing to real closed-world signatures, not PHPStan's open-world dummy stub

Real PHPStan's `method_exists`/`property_exists` narrowing
(`Type/Php/MethodExistsTypeSpecifyingExtension.php:45-102`) attaches a
`HasMethodType`/`HasPropertyType` accessory marker carrying only the
member *name* (`Type/Accessory/HasMethodType.php:47-65`) - its
`getMethod()` resolves to a `DummyMethodReflection`
(`Reflection/Dummy/DummyMethodReflection.php:19-30`, even faking
`getDeclaringClass()` as `stdClass`) backed by a
`TrivialParametersAcceptor` (`Reflection/TrivialParametersAcceptor.php:
40-56`): zero declared parameters, variadic, every type `Mixed`. This is
architecturally forced, not a missed optimization - PHPStan stays sound
for arbitrary downstream classes it never sees, so after
`method_exists($x,'foo')` succeeds it genuinely cannot know which
concrete class `$x` is, only that *some* class somewhere has a callable
named `foo`. phpstan-rs's `narrowing_type_for_predicate` table
deliberately excludes `method_exists`/`property_exists` today
(`checker/narrowing/predicates.rs:300-390`, `checker/tests/narrowing.rs:
190-203` documents the exclusion) - but it already keeps one frozen,
whole-program `Registry.classes: FxHashMap<String, ClassInfo>`
(`src/registry.rs:38`) where every `ClassInfo` carries real, typed
`methods`/`properties` (`registry/class_info.rs:151-152`), not reflection
stand-ins. Inverting that into a member-name -> declaring-classes reverse
index (the same additive move A5/B1/P1 already make from structures
built for other reasons) lets a literal-string `method_exists($x, 'foo')`
guard narrow `$x` to the union of every real class anywhere in the closed
project+vendor universe declaring `foo` - a sound, computable, finite
answer no open-world tool can give - so a following `$x->foo(...)`
resolves against real parameter/return types instead of a signature-blind
placeholder.

**Effort: large.**

### U2. Registry-level transitive `@throws` inference, replacing PHPStan's docblock-only per-hop requirement

Real PHPStan's `getThrowType()` is populated purely from the parsed
`@throws` tag, resolved *before* the function's own body is ever walked
(`Analyser/StmtHandler/FunctionHandler.php:58-90`,
`Reflection/Php/PhpFunctionFromParserNodeReflection.php:256-258`).
`MissingCheckedExceptionInThrowsCheck::check`
(`Rules/Exceptions/MissingCheckedExceptionInThrowsCheck.php:32-65`) only
ever looks at the current function's own body plus each callee's already-
fixed, doc-sourced throw type - nothing feeds a callee's *real*
per-body-walk throw points back into its own reflection for a caller to
consult (compounded by PHPStan's parallel-worker analysis model). This
directly explains why phpstan-rs's own `MethodThrowTypeCovarianceRule`
port was declined per ROADMAP.md:540-541 ("an implicit void-when-untagged
default would false-positive heavily given how inconsistently this
corpus tags @throws") - both projects currently have no real inferred
fallback, so "untagged" is indistinguishable from "throws nothing."
Extend the already-built `registry/purity.rs::resolve_transitive_purity`
fixed point with a sibling that computes each function/method's *real*
union of thrown checked-exception types from its own body (explicit
`throw`s plus resolved callees' computed throw sets) independent of
whether an `@throws` tag exists at all, and use that computed value -
not the raw tag - as what `checker/throws/missing.rs`'s
`check_missing_checked_exceptions` consults for a caller's coverage. This
also directly unblocks the declined covariance rule: with a real computed
throw type available even for untagged functions, the false-positive
risk that killed it no longer applies.

**Effort: large.**

### U3. Registry-level by-ref parameter out-type inference, closing the same doc-tag-only gap

Real PHPStan's `ParameterOutExecutionEndTypeRule` - the rule that would
catch a by-ref parameter ending up incompatible with its declared type
after execution - literally short-circuits on
`$outType = $parameter->getOutType(); if ($outType === null) { return
[]; }` with an inline comment that it "only runs when `@param-out` is
present" (`Rules/Variables/ParameterOutExecutionEndTypeRule.php:80-83,
97`); `getOutType()` is sourced purely from the `@param-out` tag
(`Analyser/PhpDocsResolver.php:186`) through the same doc-parsed-before-
body-walk pipeline U2 uses. The sibling `ParameterOutAssignedTypeRule`
only checks direct, syntactic `$x = ...` assignments inside the *current*
function against the plain declared type (`Rules/Variables/
ParameterOutAssignedTypeRule.php:63-69`) - it never follows a value
through a nested by-ref call either. ROADMAP's own Phase 45 already marks
the by-ref half PLANNED, describing it as needing "no new tag concept,
just per-function reassignment-site tracking" (ROADMAP.md:562-566). The
extension beyond a purely local walk: when the tracked variable is itself
passed by-ref into *another* function inside the same closed-world
registry, resolve that callee's own real computed out-type (from the same
fixed point, recursively) instead of requiring a hand-written tag on it -
the same whole-program, single-process property U1/U2 already exploit,
with no per-file/parallel-worker reflection-before-walk ordering forcing
a fallback to "no info" the moment a tag is missing.

**Effort: large.**

### U4. Closed-world inference of `@phpstan-self-out` for the straight-line case

Real PHPStan resolves a fluent method's narrowed return type (`$this->
foo()->bar()` chains) purely from the parsed `@phpstan-self-out` tag on
the immediate declaration (`Analyser/PhpDocsResolver.php:205`, the same
doc-parsed-before-body-walk pipeline U1-U3 cite) - when the tag is
absent, `MethodCallHandler.php:180-200` does nothing at all, no fallback
inference from the callee's own body is ever attempted even though the
body is fully visible to PHPStan at that same moment.
`MissingMethodSelfOutTypeRule.php:35-42` confirms the tooling never even
tries: it short-circuits the instant `$selfOutType` is null instead of
asking whether one could be inferred. Worth noting in both directions:
its own `IncompatibleSelfOutTypeRuleTest` data file shows PHPStan never
cross-checks a self-out tag's *claim* against what the body actually does
either - the tag is trusted unconditionally either way. phpstan-rs can
compute a registry-level self-out fact for the tractable subset PHPStan
never even validates: a void/self/static-returning method body consisting
of direct, unconditional `$this->prop = <expr>;` assignments with no
intervening branch/loop (the same straight-line bar U3 already applies
to by-ref tracking). Reusing `registry/purity.rs`'s fixed-point shape
(already the shared mechanism U1-U3 cite) to walk every such method
project-wide, computing the resulting property type through the same
assignment-target inference `checker/assignment.rs` already runs for any
ordinary `$this->prop = expr`, and exporting it as a per-method inferred
fact that call-site handling can consult to narrow the receiver exactly
the way a real tag would - falling back to an author-written tag when
both exist, since a tag may assert more (e.g. generic widening) than the
body alone proves. (`@phpstan-self-out` itself is currently entirely
unparsed in phpstan-rs, listed only in a typo-detection tag whitelist per
ROADMAP.md:1043 - this finding is about the inference half once tag
support exists, not a claim that support already does.)

**Effort: large.**

### U5. Cross-check `@phpstan-require-implements`/`@phpstan-require-extends` against actual trait usage, and use it to widen `$this` in phpstan-rs's own orphan-trait fallback

Two paired closed-world checks real PHPStan's architecture cannot
perform, not just one. **(1)** For every trait carrying a require tag,
collect the set of member names the trait's own method bodies actually
access via `$this->` that aren't already satisfied by the trait's own
declared members/ancestors, and compare that set against the required
type's real (registry-known) member set. When the required type declares
members the trait body never references anywhere, report the requirement
as broader than the trait's own body demonstrably needs - a stale or
copy-pasted `@phpstan-require-implements`/`-extends`. **(2)** Use the
required type's real member set to widen `$this` in the exact fallback
path phpstan-rs already added *beyond* real PHPStan: checking a trait
with zero using classes in the closed project standalone
(`checker/function_decl.rs::check_trait_methods`, its own doc comment
explicitly framing this as real behavior beyond doing nothing) - so a
legitimate `$this->requiredInterfaceMethod()` call in a library trait
with no in-repo consumer stops false-positiving as undefined, while a
call to a member the tag does *not* cover still correctly flags (a
diagnostic real PHPStan structurally can never produce, since it never
walks that trait body in this scenario at all). `RequireImplementsRule`/
`RequireExtendsRule` only ever check whether a concrete *using* class
satisfies the tag's claim - never whether the trait's own body needs that
much; `RequireExtendsCheck` only validates the tag's own syntax/target
validity, also never usage-based. More fundamentally, real PHPStan's
trait method bodies are only ever walked through a real using class's
`InTraitNode` - a bare `Stmt\Trait_` node is used only for ignore-comment
bookkeeping, never body-walked directly, and `ScopeContext::enterClass`
throws if given a trait at all (confirmed: grepping the whole of
`NodeScopeResolver.php` for `Trait_` returns zero hits). So a trait with
no using class anywhere in the analyzed file set has its body never
walked by real PHPStan at all - it structurally cannot ever compare a
require-tag's claim to actual usage in that case, and in the has-consumer
case it never needs to since `$this` resolves against the real
consumer's real interface directly. phpstan-rs's registry already keeps
`require_extends`/`require_implements` as separately-resolved, real
class-name lists on `ClassInfo`, fully independent of `flatten_traits`'s
method/property merge - `current_trait` is consulted today only for
deprecation-suppression, never member resolution, confirming the tag's
real member set is present in the registry but simply unconsulted in the
standalone fallback. `checker::oop::check_require_tags` already proves
the consumer-compliance half is ported; this is the usage-based half
neither project has, made newly computable because phpstan-rs's
single-process registry can look up the required type's real, typed
member set at any point - unlike PHPStan's per-file reflection-before-
walk pipeline U1-U4 already document as the systemic blocker for this
whole tag family.

**Effort: large.**

### U6. Infer an implicit `@phpstan-assert` fact for the structurally obvious guard-clause-only function body

At registry-build time, when a function/method's entire body is exactly
one `if (!<condition on $param>) { throw ...; }` statement (no other
statements before or after), phpstan-rs can syntactically read the guard
condition the same way `narrowing_type_for_predicate` already interprets
an `if` condition for ordinary flow narrowing, and synthesize an assert
fact into `FunctionSig::assert` for that declaration - the exact field
every existing consumer (`check_assert_call`/`check_assert_tag_types`,
the purity gate) already reads uniformly regardless of whether the tag
was author-written. When an explicit `@phpstan-assert`-family tag is
already present, it always wins and the inferred fact is not
computed/used at all (mirroring U4's own stated fallback order: an
explicit tag may assert more, e.g. generic widening, than the body alone
proves). Confirmed directly: `FunctionHandler::processStmt` resolves the
assertion list purely from `PhpDocsResolver::getPhpDocs()` and passes it
into `MutatingScope::enterFunction()` *before* the function's own body is
ever walked - the reflection object callers see is fixed at that point
with zero fallback when no tag exists. The one rule that touches the
tag for validation, `AssertRuleHelper::check()`, only checks the tag's
own syntactic/type validity against the parameter's *declared* type - it
never inspects the function's own body to confirm or derive the
assertion, confirming (like U4's own finding) the tag is trusted
unconditionally in both directions once present, and simply absent with
no substitute when it is not. Unlike U2/U3's whole-program fixed point,
this is a single-function, non-recursive syntactic pattern match
(recognizing exactly one shape) - it doesn't need iteration to a fixed
point at all, only the condition-to-type extraction the narrowing
machinery already has, run once per candidate declaration. The result
slots into the pre-existing `FunctionSig::assert` field every consumer
already reads uniformly, so no new consumption path is needed - only a
new producer.

**Effort: medium.**

---

## V. Whole-program structural duplication

Extends D3 (duplicate branch bodies within one function) and D7
(structurally-identical operands within one expression, both in Part 3
below) to duplication *across* declarations project-wide - a different
AST scope neither existing item touches.

### V1. Whole-program duplicate function/method body detection

For every top-level function and class/trait method body project-wide,
build a canonical structural fingerprint (a postorder hash over
`stmt_children`/`expr_children`, each local/parameter alpha-renamed to a
positional slot, `Span` already excluded by those walkers) and bucket
declarations by it; within a bucket, confirm an exact alpha-equivalent
match with a `Stmt`-list equality helper (the same recursive
shape-matching technique `is_switch_break_equivalent` already uses,
generalized from one statement to a statement list). For a matching pair
with no `is_subclass_of`/`classes_using_trait` relationship (skip
legitimately related pairs) and a body exceeding a minimum statement
count (skip trivial getters/setters), emit an opt-in
`method.duplicateBody`/`function.duplicateBody` diagnostic - a DRY/
extract-trait candidate the author likely doesn't know exists. PHPStan's
`Rules/` has no body-content comparator anywhere - grepping both vendored
trees for "identical"/"duplicate" turns up only name-collision rules
(`DuplicateFunctionDeclarationRule.php`, `DuplicateDeclarationHelper.php:
26-60`) and array-literal keys, never two differently-named declarations'
bodies - architecturally awkward for PHPStan's own `Rule::processNode`
interface, which visits one node per call with no project-wide sibling
corpus to pair against without separate collector infrastructure.
phpstan-rs is single-process with every file's full parsed body already
resident project-wide (`FileState.ast: Vec<Stmt>`, `project.rs:28`), and
the generic `stmt_children`/`expr_children` walkers already thread
through every node kind - a ready-made traversal for both hashing and
equality with no new AST plumbing; `Registry::is_subclass_of`/
`classes_using_trait` (`registry/hierarchy.rs:12,76`) already answer the
false-positive gate. (AI below reuses this same machinery for the
inverted case: a *legitimate* override whose body happens to be
identical to its ancestor's.)

**Effort: medium.**

### V2. Whole-program divergent-clone detection: near-duplicate bodies differing by exactly one leaf

Extends V1's fingerprint pass with a second, coarser pre-filter hash
keeping only statement-kind/operator-kind shape (ignoring literal values
and identifier names) to bucket "shape-identical" bodies; within a
bucket, walk two alpha-canonicalized ASTs together and count leaf-level
divergences (a differing literal, a differing operator, an identifier
reference breaking the alpha-mapping). Exactly one (or a small,
config-bounded number of) divergence in an otherwise-identical pair of
unrelated declarations is flagged as `function.divergentClone`/
`method.divergentClone` - a correctness signal, not style: a maintainer
fixed a bug or corrected a threshold in one copy and forgot the sibling
exists. Same absence in PHPStan as V1, plus this specifically needs a
bounded near-miss/tree-distance comparison, which nothing in its rule set
attempts for any purpose - `Rules/Comparison/` reasons purely from each
expression's inferred type, never a structural distance between two
trees. Reuses V1's canonicalization and hash-bucket machinery directly;
only the final comparison step (count divergences instead of requiring
zero) is new.

**Effort: large.**

### AI. Whole-program redundant-override detection (V1's mirror image)

Reuses V1's alpha-equivalence body-canonicalization/hashing machinery,
but inverts the relationship gate: instead of excluding pairs where
`is_subclass_of`/`classes_using_trait` holds (V1's false-positive guard
against flagging a legitimate override as an accidental duplicate), fire
*only* when that relationship holds and the specific overriding method
resolved via `lookup_ancestor_method_with_owner` has a body alpha-
equivalent to the ancestor method it overrides. Such an override changes
nothing behaviorally - plain inheritance already produces the identical
effect - so it's either dead code (safe to delete, rely on inheritance)
or a copy-paste mistake where a divergent edit was intended but never
made. Emits an opt-in `method.redundantOverride` diagnostic pointing at
the overriding declaration, with the ancestor location as a tip, skipping
trivial 0-1 statement bodies (bare `parent::foo()` forwarders, a common
deliberate idiom for documentation/covariance/attribute purposes) using
the same minimum-statement-count guard V1 already uses for its own
triviality filter. Real PHPStan's `OverridingMethodRule` (408 lines)
implements the entire override-checking family - parent-final checks,
parameter/visibility comparison helpers, signature rules, `#[Override]`
validation - but never once reads `getBody()`/statement content from
either the overriding or ancestor method; every check operates on
signature reflection data only. Grepping both vendored `Rules/Methods/`
directories for any body-content comparator returns zero hits - the only
"duplicate"-adjacent rules anywhere are name-collision detectors, never a
comparison between two differently-scoped method bodies. phpstan-rs is
single-process with every file's full parsed AST already resident
project-wide, so pairing an overriding method's body against its
resolved ancestor's needs no new traversal infrastructure - V1's own
canonical fingerprint and statement-list equality comparator apply
unchanged to this inverted case. The only new wiring: for each class's
declared methods (already iterated by `check_method_overrides`), resolve
the ancestor via `lookup_ancestor_method_with_owner` (already used by
`method_overrides.rs`'s own compatibility checking) and run V1's equality
helper directly against the two bodies - no relationship-exclusion filter
needed at all, since here the relationship is required rather than
disqualifying.

**Effort: small.**

---

## X. Whole-program parameter-type narrowing from actual call-site arguments

### X1. Narrowing from the set of concrete object types actually passed

Distinct from H1 (literal *value* constancy - never fires on an object
argument, since a value differs on every call by construction) and H3
(property assignment sites, a different AST location entirely). A
project-wide pass extends the exact per-call-site data `check_args`
already computes: for a call's resolved argument (from
`resolve_call_args`, `checker/calls.rs:1040-1071`), the checker already
infers `arg_ty = self.infer_expr(arg, locals)` (`calls.rs:411`) before
comparing it to the parameter's declared type in the ordinary check.
Instead of discarding `arg_ty`, when it resolves to a concrete,
non-generic `Type::Object(class_name, [])`, push `class_name` into a
project-wide `HashMap<(callee_key, param_index), HashSet<String>>` -
after all files are checked, for every declared parameter whose own type
is "wide" by construction (a single interface or non-final abstract
class, `ClassInfo.kind`/`is_abstract`, `registry/class_info.rs:146-148`,
or a `Union` of 2+ named classes) with 2+ real call sites and exactly one
distinct concrete class seen, flag the declaration suggesting the type
could be narrowed to that one class - or, for the interface/abstract
case, that the wider type is never actually exercised project-wide. Real
PHPStan's cross-call-site aggregation surface is its 14 real `Collector`
implementations (enumerated via `find ... -iname '*Collector*'`), none of
which collects per-call argument *types* - the closest,
`NamedArgumentParameterMethodCallsCollector.php`, only aggregates
named-argument *parameter names* (already H1's own citation). Its only
parameter-adjacent "too wide" rule family, `Rules/TooWideTypehints/`
(read `TooWideTypeCheck.php:294-349` in full to confirm), reasons only
about a by-ref/`@param-out` parameter's own value *after* the function
body assigns to it - single-function-local reasoning, never a
caller-supplied argument type aggregated across real call sites. No
PHPStan rule anywhere narrows an ordinary (non-out) parameter's declared
type from what callers actually pass. phpstan-rs needs no new inference
machinery here, only retaining and aggregating a value already computed
and then thrown away, threaded across the same single shared `Registry`
every other closed-world finding in this document already grounds - no
cross-process reconciliation problem PHPStan's Collector architecture
exists to solve.

**Effort: large.**

### X2. Narrowing from array non-emptiness, not object identity

Extends X1's project-wide call-site aggregation pass to a second
dimension: for every declared parameter typed plain `array` (no
`non-empty-array`/shape refinement already present), track at every real
call site (via the same `resolve_call_args` resolution and the same
already-inferred `arg_ty` X1's own citations establish) whether the passed
argument is provably non-empty - reusing the array-non-emptiness test
that already exists for exactly this purpose, `Self::
is_known_non_empty_array` (`checker/expr/array_ops.rs:315-327`), which
already handles a non-empty `ArrayShape` literal, an intersection
carrying the `NonEmptyArray` accessory, and a union where every branch is
non-empty - precisely the "non-empty literal, or an already-narrowed
non-empty-array-typed variable/expression" disjunction this needs, with
zero new logic for the per-call-site test itself. After all files are
checked, for every plain-`array`-typed parameter with 2+ real call sites
where non-emptiness held at every one, flag the declaration suggesting
`array` be narrowed to `non-empty-array`. Real PHPStan's cross-call-site
aggregation surface is its concrete `Collector` implementations (14
found, enumerated directly); none collects per-call argument
emptiness/type data at all - the only parameter-narrowing-adjacent rule,
`TooWideTypehints/TooWideTypeCheck.php`'s `checkParameterOutType`, reads
only a by-ref/`@param-out` parameter's own value *after* the function
body assigns to it, single-function-local, never a caller-supplied
argument's shape aggregated across real call sites project-wide.
phpstan-rs already threads every file's checking through one shared
`Registry` in a single process (the same closed-world precondition X1's
own citations establish), so this variant's project-wide aggregation
needs no cross-process reconciliation - uniquely for this dimension, the
per-call-site test itself isn't new inference at all, only the
project-wide retain-and-aggregate step (mirroring X1's own design) is.

**Effort: medium.**

---

## AD. Whole-program dead-enum-case detection

A new opt-in check flagging a declared enum case (backed or pure) as dead
when, project-wide, it's never referenced via `EnumName::CaseName`, never
appears as a `match`/`switch` arm value, and is never provably produced
by a literal argument to `::from()`/`::tryFrom()`. Real PHPStan's only
two case-scoped rules are non-usage-based (`EnumCaseAttributesRule` just
forwards attribute groups for target validity; `EnumSanityRule` only
flags duplicate backing values/constructors/native-method redeclaration -
neither cross-references a case against the rest of the project). More
fundamentally, its only dead-code family (`UnusedPrivateMethodRule` and
siblings, cited for B1) gates on `isPrivate()` - but a PHP enum case has
no visibility syntax at all (phpstan-rs's own registration comment,
`registry/class_info.rs:1010`, notes this directly: "always public"), so
PHPStan's existing privacy-gated technique structurally has no
`isPrivate()` case to ever match, even by extension. phpstan-rs already
carries every piece as existing, unconnected infrastructure: cases are
pre-registered per-FQN as `Type::EnumCase` in `ClassInfo.consts`
(`registry/class_info.rs:1002-1024`), a reverse literal-value-to-case
lookup already exists for a different purpose
(`Registry::enum_case_for_value`, `registry/hierarchy.rs:604-611`, built
for Phase 79's `::from()`/`::tryFrom()` narrowing), and the whole
project's `Registry` is built once, single-process, before checking
starts. A real gotcha the implementation must handle: `Match` arm
conditions are walked generically by `expr_children`, but a `Switch`
case's own `test` expression is *not* covered by generic `stmt_children`
(`ast_walk.rs:174` only flat-maps each case's body) - `switch(){ case
Suit::Hearts: }` needs the same manual special-case `member_usage.rs`/
`var_usage.rs` already apply for `Switch` elsewhere. A non-literal
`::from()`/`::tryFrom()` argument, a dynamic `Suit::{$expr}` access, or
any call to `::cases()` marks the *whole enum* escaped, matching
`MemberUsage.escaped`'s own conservative all-or-nothing precedent, since
iterating `::cases()` is a legitimate, common way to use every case
without ever spelling one out by name.

**Effort: medium.**

---

## AJ. Whole-program dead abstract-class detection

For every class with `is_abstract == true`, walk the reverse (parent-to-
children) subclass index A5/P1/U1 already propose building and confirm
no path from it reaches any *concrete* class anywhere in the closed
project+vendor universe - a plain reachability walk over the same graph
(a direct subclass can itself be abstract, so this needs to walk to the
first concrete descendant, not just one hop). Zero such reachable
concrete descendant means the class can provably never be instantiated
as an object anywhere this analysis can see, ever - not "currently
unused," a permanent structural fact. PHPStan's own dead-code family
gates on `isPrivate()`; there is no `isAbstract`-plus-descendant-
enumeration rule anywhere (grepped `Rules/DeadCode/` and `Rules/Classes/`
for "abstract" - only `InstantiationRule.php`, which flags a *syntactic*
`new AbstractClass()` attempt, not a whole-program zero-subclass fact).
More fundamentally, PHPStan's `Reflection/` layer has no descendant-
enumeration API of any kind (grepped the whole vendored tree for
`getSubclasses`/`descendantClasses`/`getKnownSubclasses` - zero hits) -
because PHPStan must stay sound for a library's downstream consumers who
can add real subclasses it never sees, the identical open-world
constraint A5 already documents. phpstan-rs's `is_abstract` is already a
plain struct field on every registered class, and it already flags
direct syntactic instantiation of an abstract class - confirming the
registry already fully distinguishes abstract from concrete; only a
project-wide reachability walk over the (not-yet-built) reverse index is
new. Distinct from A5 (whose reverse index only ever drives instanceof/
class-string narrowing, never a dead-code diagnostic) and from B2 (whose
`dependents` graph is built from bare *name* references - an abstract
class referenced only via a parameter/property type hint for polymorphic
dispatch, or via `static::`/`self::` calls to its own static methods,
accumulates non-zero dependents and so is invisible to B2's "referenced
nowhere" test even with zero real subclasses anywhere - verified directly
against `deps.rs`'s `collect_class` implementation, which unconditionally
inserts every `extends`/`implements`/type-hint name it sees).

**Effort: medium.**

---

## P. Closed-world precision the "Pending / partial" section's own techniques can unlock, beyond just finishing them

Two cases where finishing an in-progress phase using phpstan-rs's own
closed-world registry (the same reverse-hierarchy index A5 already
proposes for sealed-hierarchy narrowing) doesn't just reach parity - it
resolves something real PHPStan is structurally, permanently unable to
resolve, for reasons unrelated to cost.

### P1. Resolve provably-closed virtual dispatch inside the transitive-purity fixed point

Extend Phase 41's transitive-purity fixed point (`registry/purity.rs`) so
a `$this->method()` call through a non-final, overridable method isn't
unconditionally disqualified. Add the same reverse (parent-to-children)
class index A5 proposes (extending `hierarchy.rs`'s forward-only
`all_ancestor_names`), enumerate every real override of the target method
anywhere in the closed analyzed set, and treat the call as a dependency
on all of them; if the reverse index proves zero overrides exist anywhere
in the project+vendor universe, resolve the call exactly like a final
method already is. Real PHPStan's `PossiblyPureCallTransitivePurityResolver::
resolveCall` (`Rules/DeadCode/PossiblyPureCallTransitivePurityResolver.php:
168-207`) unconditionally disqualifies the whole caller unless the method
is private, final, or its declaring/called-on class is final - no attempt
to enumerate real overrides - and the identical gate is duplicated
independently in `FunctionPurityCheck::check`
(`Rules/Pure/FunctionPurityCheck.php:126-141,142-184`), confirming this is
deliberate, structural policy everywhere PHPStan reasons about call-target
purity: it has no descendant-enumeration API anywhere in `Reflection/`
because it must stay sound for a library's downstream consumers who can
add subclasses it never sees - an open-world constraint, not a cost cap.
phpstan-rs's `is_non_overridable` (`registry/purity.rs:503-524`) currently
mirrors this restriction verbatim, by the module's own explicit design
comment - but phpstan-rs analyzes one closed-world project+vendor universe
in one process with one frozen `Registry`, the identical property that
already lets A5 treat "zero subclasses found" as sound. Given the
identical source and the identical missing `final` keyword, phpstan-rs
could resolve what real PHPStan is architecturally forced to treat as
unknowable. Distinct consumer from A5 (narrowing) and B1/H2 (unused-member
detection, NoDiscard inference) - this one drives the purity
dependency-resolution fixed point and `@phpstan-pure` validation
specifically.

**Effort: large.**

### P2. Resolve real Composer package boundaries for `@internal` enforcement, instead of the root-namespace-segment heuristic both projects currently share

Phase 67's already-shipped `@internal` engine approximates "is the caller
in the same package as the `@internal` symbol" by comparing only the
first `\`-delimited namespace segment of caller vs. target. Swap that for
a real composer-package-boundary lookup using the
`find_composer_package_name`/`detect_phpstan_dependency` infrastructure
Part 8 (Composer/dependency hygiene) already establishes exists in this
codebase - resolve both the caller's file and the target's declaring file
to their owning `composer.json` package name, and report only when the
packages genuinely differ. Closes both directions of the mismatch: false
negatives (two separately-versioned composer packages sharing one root
namespace segment - any two same-vendor packages like `symfony/console`
and `symfony/http-foundation`, both rooted at `Symfony\`) and false
positives (one composer package legitimately spanning more than one
top-level namespace). Real PHPStan's `RestrictedInternalUsageHelper::
shouldBeReported`/`getRootNamespaceFromName`
(`Rules/InternalTag/RestrictedInternalUsageHelper.php:36-57`) - the shared
engine behind all five `RestrictedInternal*UsageExtension` classes -
decides package membership purely via the bare first namespace segment,
with no composer.json awareness anywhere: architecturally forced, since at
the point a `Rule`/`Scope` processes one file, PHPStan's reflection layer
has no persistent whole-program "which package declares this symbol" index
to query (the same absence Part 8 independently confirms). phpstan-rs's
`internal_usage.rs` mirrors this exact heuristic on purpose today (its own
doc comment names it "the common Composer-package-as-top-level-namespace
convention") - but `composer::find_composer_package_name`
(`src/composer.rs:144-155`) already exists for a different rule family,
computed once per whole-project run rather than discarded between files.
Wiring it into `internal_usage.rs`'s boundary check costs no new
architecture - only an already-resolved real package name in place of a
namespace substring.

**Effort: medium.**

---

# Part 2: Native-performance wins real PHPStan needs a separate extension for

Real PHPStan's own team ships (or is still building) these as opt-in native
C++ ("Turbo") because their stock PHP implementation can't afford them.
phpstan-rs is already native for every user - it doesn't need the extension
architecture, only the data structure.

## C. Native-performance

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
real run's cost. (C4 below is a scheduling refinement for exactly this
parallelization, not an alternative to it.)

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

### C4. Use a work-stealing thread pool for `check_project`'s per-file loop, not a static size-sorted partition

Real PHPStan's `Scheduler::scheduleWork` solves the straggler problem
(one worker stuck with a run of the project's largest files while
siblings finish early and idle) by computing each file's size, sorting
files descending by size, then dealing them round-robin into N job
buckets so every bucket mixes large and small files - a static partition
computed once, upfront, before any worker starts. This is the right
technique for PHPStan's own constraint: its workers are separate OS
*processes* with no shared memory, so real work-stealing (a worker
asking a coordinator "give me the next file" every time it goes idle)
means an IPC round-trip per request - expensive enough that precomputing
a size-based heuristic partition is cheaper than measuring real
imbalance live. C2 proposes native threads sharing one process and one
`Registry`, where that constraint doesn't hold: a work-stealing queue's
per-item cost there is a plain pop (an atomic op or an uncontended
lock), so there's no reason to pay for a *heuristic proxy* for balance
(file size) when real, measured imbalance can be resolved for free as it
happens - a `Vec<PathBuf>` behind a work-stealing deque (`crossbeam-
deque`, or `analyze_files.par_iter()` via `rayon` for the same effect
with far less code) load-balances any actual file-to-file cost variance,
not just the size-correlated portion of it. It also sidesteps a
complication PHPStan's own technique has to solve for: PHPStan re-sorts
each bucket back into original file order after striping specifically
because "analysis results can be sensitive to" per-job file order in its
own model - but C2's own premise is that `Project::check_file` needs
only `&self`, no shared mutable state carried across files within a
thread, so phpstan-rs's per-file checks are already order-independent;
there is nothing to preserve in the first place, and no reordering step
to write. `check_project` currently has no file-partitioning-across-
workers logic at all (`analyze_files.iter().map(...)` is strictly
sequential today), so there's no existing behavior to regress from
either way - but building C2 via the sibling parse-only `check`
command's own `chunks()`-based `std::thread::scope` pattern (its cited
proof-of-pattern) would reintroduce the exact static-partition imbalance
this item exists to avoid; a work-stealing pool is the smaller, not just
different, fix. Neither `rayon` nor `crossbeam-deque` is a dependency
today (`grep -n 'rayon\|crossbeam' Cargo.toml` - nothing), so this adds
one new dependency either way, same as PHPStan's own `Scheduler` needed
no new dependency for its process-based equivalent - the honest tradeoff
of doing this with threads instead of processes.

**Effort: small.**

---

# Part 3: New rule categories and design-pattern suggestions

Not approximation gaps - PHPStan's entire `Rules/` catalog (Api, Arrays,
Cast, Classes, Comparison, Constants, DeadCode, Exceptions, Functions,
Generators, Generics, Methods, Missing, Names, Namespaces, Operators,
PhpDoc, Properties, Pure, Regexp, RestrictedUsage, TooWideTypehints,
Traits, Types, Variables, Whitespace) simply has no rule of these shapes at
all, confirmed by grep.

## D. New rule categories with zero PHPStan precedent

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
copy-paste bug where one branch was meant to differ. (V1/V2 in Part 1
extend this same idea to duplication *across* declarations, a different
AST scope.)

**Effort: medium.**

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

### D8. Computed edit-distance suggestion for `method.notFound`, the one undefined-symbol diagnostic real PHPStan never attaches a tip to

Real PHPStan attaches *some* tip to three of the four "not found"
diagnostic families, but never a *computed* one: `AccessPropertiesCheck.php`
calls `->tip(...)` directly in its `property.notFound` branch (a computed
tip when available, else a fixed help URL); `class.notFound`/
`function.notFound` sites overwhelmingly attach `RuleErrorBuilder::
discoveringSymbolsTip()` (`RuleErrorBuilder.php:202-205`, a fixed "Learn
more" URL) via `RuleLevelHelper.php:245-246` and ~25 other call sites.
`MethodCallCheck.php`'s `method.notFound` builder (line 144), by
contrast, is a bare `->identifier('method.notFound')` with no tip call at
all (`grep -n "tip(" vendor/phpstan-src/src/Rules/Methods/
MethodCallCheck.php` returns nothing) - confirmed clean, matching
phpstan-rs's own `method.notFound` sites (`src/checker/expr.rs:
1030-1036,2837-2842`, plain `self.report(...)`, no `.tip`). The
opportunity: a *computed* edit-distance suggestion ("did you mean
`foo`?") over the already-resident whole-project `Registry`
(`Registry.classes: FxHashMap<String, ClassInfo>`, `src/registry.rs:38`;
`ClassInfo.methods`, `registry/class_info.rs:151`), using the already-built
but mostly-unused `DiagnosticBuilder::tip()` (`src/checker.rs:407`) -
specifically for `method.notFound`, since class/function/property already
carry *some* tip (static, not computed) and method genuinely has none in
either codebase.

**Effort: small.**

---

## AA. Whole-program design-pattern suggestion rules

Opt-in suggestions in the shape B1/B2/H-section items already establish: a
whole-program fact PHPStan's architecture can't cheaply compute (would
need its 14-collector `CollectedDataNode` machinery to reconcile across
parallel workers) that phpstan-rs's single-process, closed-world registry
gets almost for free. Several of these also expose the same directional
asymmetry: real PHPStan's machinery only ever *validates* an existing
stricter annotation, never *suggests* adding one.

### AA1. Whole-program "readonly candidate" suggestion for properties assigned exactly once

For every declared, non-readonly, non-static, non-promoted property, a
registry-build-time pass walks every `$this->prop = rhs`/`self::$prop =
rhs` assignment site across the property's whole subclass-aware
hierarchy and counts syntactic assignment *sites* (not RHS types - a
different question from H3, which asks whether every RHS *type* across
sites is narrower than the declared type; a property assigned once with
a wide, non-narrowable RHS like a fresh `new DateTime()` triggers this
but never H3, and a property assigned five times with five different
literal-subtype values triggers H3 but never this). If the total count
project-wide is exactly 1, and that single site is a plain `Assign`
(never compound, never by-ref) lexically inside the declaring class's own
`__construct()`, the property is a mechanically safe "add readonly"
candidate - real PHP readonly semantics require initialization from
exactly the declaring class's own scope, exactly once, so a single-site
constructor-only assignment already satisfies both constraints before the
diagnostic is even emitted. Real PHPStan's readonly machinery only ever
validates a property *already* declared readonly against illegal
re-assignment - `MissingReadOnlyPropertyAssignRule.php:33` and its
PHPDoc sibling both explicitly skip any property that isn't already
flagged readonly; it never reasons in the opposite direction. No
PHPStan collector aggregates per-property assignment *counts* across
files at all. phpstan-rs already has every primitive this needs,
unconnected: the exact "is this write inside the declaring class's own
constructor" signal already exists in `check_property_assignment`
(`checker/property_assignment.rs:57-70`, the readonly-violation check's
own gate, reusable in the opposite direction); `checker/unused/
member_usage.rs`'s existing per-class `MemberUsage.writes: HashSet<
String>` needs only to become a project-wide *count*; `PropertySig.
is_readonly` (`registry/class_info.rs:26`) is exactly the flag a
suggested codemod would flip.

**Effort: medium.**

### AA2. Suggest narrowing a public constructor to private/protected when it's only ever called from one factory method

After the registry is built, walk every `new ClassName(...)` call site
project-wide and, per class with a `Public` constructor, record the set
of distinct enclosing-method identities (`current_class` +
`current_method`, already tracked live on `Checker`,
`src/checker.rs:510`) from which `new` targets it. If that set has
exactly one member across the whole closed-world project, and that
member is a static method of the class itself (a `create()`/
`fromArray()`-style named constructor) rather than another instance
method or the class's own constructor, suggest narrowing the constructor
to enforce the factory as the sole entry point - excluding classes with
zero call sites, abstract classes, classes extended elsewhere (a
subclass needs a visible-enough constructor), and classes whose name
escapes into any dynamic-instantiation-adjacent construct (`new $var(...)`,
`ReflectionClass`, `unserialize`), mirroring the conservative
all-or-nothing escape-hatch `MemberUsage::escaped` already uses for the
analogous private-member-usage problem. Real PHPStan has no factory/
named-constructor concept anywhere in its rule set (grepped both
vendored trees for "factory"/"named constructor" - nothing but an
unrelated `StaticTypeFactory` import); more fundamentally, its only
whole-application member-usage rule gates on `isPrivate()` specifically
because determining every call site of anything non-private needs its
own cross-process collector reconciliation (cited already for B1) - the
same cost applies identically to aggregating every `new ClassName(...)`
call site's enclosing method across the whole project. phpstan-rs
already does the two things this needs simultaneously during ordinary
checking: `ExprKind::New`'s resolution of every constructor call's target
class (`checker/expr.rs:1697`) and `Checker`'s live tracking of the
enclosing method (`checker.rs:510`) - a project-wide pass only needs to
record `(target_class, current_class, current_method)` triples during the
existing walk.

**Effort: medium.**

### AA3. Suggest constructor property promotion for a trivially-forwarding constructor

For a class's `__construct()`, when every statement is exactly
`$this->prop = $param;` where `prop_name == param_name`, each such param
is non-variadic/non-by-ref and matches a sibling `PropertyDecl` with an
identical declared type and no conflicting default, the property isn't
itself already promoted/static/hooked, and every constructor param is
consumed by exactly one such assignment (no leftover statement, no unmatched
param) - suggest rewriting the whole constructor to use PHP 8.0
constructor property promotion, using each property's visibility/
`is_readonly` as the promoted modifiers. Real PHPStan's promoted-property
machinery is entirely about *validating* declarations already promoted
(`isPromoted()`/`isPromotedFromTrait()` reflection flags,
`Node/ClassPropertyNode.php:29-65`) - it never reasons in the reverse
direction; grepping both vendored trees for "promot" turns up ~30 hits in
phpstan-src, all reflection/scope-flag machinery for already-promoted
code, and zero in phpstan-strict-rules - the closest ecosystem tool for
this codemod, Rector, is a separate non-PHPStan project. phpstan-rs's
`Param` already fully represents promoted-parameter syntax
(`promoted_visibility`/`promoted_readonly`/`promoted_final`, since it
already supports parsing/checking promoted constructors elsewhere) - this
only proposes a new detector recognizing the unpromoted mirror-image
shape and reusing the same `Param`/`PropertyDecl` fields to build the
suggested rewrite, no new AST or parser plumbing.

**Effort: small.**

### AA4. Class-level `readonly class` shorthand when every property independently qualifies under AA1

Once AA1's per-property pass has run, add one class-level aggregation
step: if a class declares at least one non-static property, has zero
promoted constructor parameters and zero static properties, and *every*
declared property independently passed AA1's single-constructor-site
test, emit one class-level suggestion pointing at PHP 8.2's whole-class
`readonly class Foo { ... }` shorthand instead of N separate per-property
suggestions (suppressing AA1's own diagnostic for that class once the
aggregate fires, to avoid redundant noise). Real PHPStan's readonly
machinery only ever validates a property already declared readonly
against illegal reassignment, or validates *consistency* of an
already-declared `readonly class` modifier against a subclass - neither
direction ever proposes *adding* the modifier, matching AA1's own
confirmed absence of any "suggest readonly" rule concept; no PHPStan
collector aggregates per-property assignment counts across an entire
class's property set either. A genuine, useful implementation detail
surfaced during grounding: phpstan-rs's parser already fully discards the
fact that a class body was introduced with the `readonly` keyword -
`parse_class_decl` tracks `class_is_readonly` as a local variable used
only to desugar into each member's own `is_readonly` flag, never stored
on the returned `ClassDecl` itself - so AA1's own per-property scan
already naturally self-excludes an already-`readonly class`-declared
class (every property already has `is_readonly=true` and is filtered out
before this aggregation would even run); the only genuinely new piece is
the positive aggregation - iterating a class's plain (non-promoted)
declared properties and checking all of them qualified.

**Effort: small.**

### AA5. Suggest declaring `never` when a body provably never returns normally

Real PHPStan has this exact suggest-direction rule already *built* -
`NeverRuleHelper::shouldReturnNever()` walks a function's execution ends
and classifies each always-terminating one as a throw vs. "terminates
script execution" (an infinite loop, or a call to an already-`never`
function/method) - but the rules that consume it,
`FunctionNeverRule`/`MethodNeverRule`, are registered only in
`vendor/phpstan-src/issue-bot/playground.neon`, phpstan.org's own private
issue-reproduction bot config (confirmed: grepping every `conf/*.neon`
for "Playground" returns zero rule-registration hits, no installable
package/level config/strict-rules extension ever turns them on). Every
*other* `never`-related identifier in real PHPStan - `return.never` in
`FunctionReturnTypeCheck.php` and `MissingReturnRule.php` - is
exclusively the *validate* direction (a body that already declares
`never` but returns/falls through), confirming the same directional
asymmetry AA1/AA3/AA4 already established for readonly/promotion:
PHPStan never proposes adding `never` to a project shipping to real
users, only checks it once present. phpstan-rs already computes the
exact "this call never returns" fact generically, unconnected to any
suggestion: `always_terminates` treats any call whose callee registry
signature declares `never` as a terminating statement - already reused
today for guard-clause narrowing, `deadCode.unreachable`, and Phase 61's
own `return.never`/`return.missing` validation. Add a new opt-in
suggestion pass right next to that existing validation block: for a
declared function/method whose return type isn't already `never` (or
generator), contains zero `return` statements anywhere in its body (a
distinct, stricter gate than `always_terminates`'s own broader notion,
which also treats a plain `return;`/`return $x;` as terminating - that
must NOT count here), where every real execution end is either a `throw`
or a statement inferred as `Type::Never`, suggest the stricter type -
distinguishing "always throws" from "always terminates script execution"
the same way real PHPStan's own message text does. Since phpstan-rs
ships one binary/config surface to every user rather than gating a
working rule behind a private internal-only demo tier, this is additive
wiring onto an already-built, already-tested primitive, not new analysis
machinery.

**Effort: medium.**

### AA6. Suggest `@template-covariant`/`@template-contravariant` when actual usage already proves it safe

`TemplateTypeVariance::validPosition()` treats an invariant-declared (or
undeclared - the default) template type as valid in *any* position - so a
plain `@template T` never produces a diagnostic no matter how it's
actually used in the class body. `VarianceCheck::check()` and its two
callers (`PropertyVarianceRule`, `MethodSignatureVarianceRule`) only ever
compare an *already-declared* variance against the position found - no
code path anywhere in those files, or in `TemplateTypeFactory.php`/
`TemplateType.php`, aggregates every usage site of an undeclared/
invariant template parameter to compute what variance would actually be
sound (grepping phpstan-src for "infer.*variance"/"guessVariance"/
"computeVariance" returns nothing; phpstan-strict-rules has no
variance-related rule at all). phpstan-rs's own mirror of this exact
mechanism, `check_variance`/`check_property_variances`, early-returns
before doing any walk at all when a class has only default-invariant
templates - identically to real PHPStan's silent no-op. But the
traversal this needs already exists in the same file: `walk_variance`
already composes a `Position` (In/Out/Inv) through every nested generic
slot for a class's own template occurrences, driven by the same per-class,
declaration-time call sites already wired for properties and methods -
no new data, no cross-file/registry-wide pass, just a second,
always-run, undeclared-template variant of the same walk that *records*
positions instead of comparing them against a pre-existing declared
variance. If every recorded position across the whole class was `Out`
(with at least one occurrence), suggest `@template-covariant T`; if every
position was `In`, suggest `@template-contravariant T`; any `Inv`
occurrence, any mix, or zero occurrences means no suggestion at all -
matching real PHP variance soundness exactly, and mirroring AA1's
"exactly one qualifying condition or nothing" conservatism.

**Effort: medium.**

---

## AB. Two PHP 8.4 rough edges - one PHPStan hasn't caught up to itself, one it structurally can't

Real PHPStan's newest feature support (asymmetric visibility, property
hooks) is exactly where self-inconsistencies would be expected, unlike
its decade-old core mechanisms.

### AB1. Purity/impure-point enforcement for property hooks

Real PHPStan's own `PropertyHooksProcessor::processPropertyHooks`
(`Analyser/PropertyHooksProcessor.php:113-158`) explicitly gathers
`ImpurePoint`s from a hook body via `pushNodeGatherer` (property
assignments recorded at lines 123-131) and stores them on
`PropertyHookReturnStatementsNode` (which exposes `getImpurePoints()`) -
but `Rules/Pure/` contains only `PureFunctionRule.php`/`PureMethodRule.php`
(consuming the ordinary function/method return-statements nodes), and
`Rules/DeadCode/*WithoutImpurePointsCollector.php` has only Function/
Method/Constructor variants - no `PurePropertyHookRule` and no
property-hook-consuming collector exists anywhere. The impure-point data
property hooks compute is gathered and then never read by any Rule or
Collector - hooks get zero purity enforcement despite PHPStan's own
plumbing being built for exactly that purpose. phpstan-rs already has
both pieces this needs, just not connected to hooks: the registry-build-
time transitive purity classifier (`registry/purity.rs`) and the
one-directional `@phpstan-impure` tag-validation check
(`check_declared_purity_tag`, `checker/noop.rs:267-304`), currently
wired into exactly one call site - ordinary function/method declarations
(`checker/function_decl.rs:198`). `checker/oop/property_hooks.rs`'s
`check_get_hook`/`check_set_hook` (lines 141, 276) never call it. Since
every phpstan-rs check is a hardcoded function call with no DI/Rule-
registry indirection, adding one more call site to an already-built,
already-tested classifier is additive wiring that closes a gap real
PHPStan's own source shows it gathers the data for but never consumes.

**Effort: medium.**

### AB2. Dead-write detection for `public private(set)` properties

`UnusedPrivatePropertyRule::processNode`
(`Rules/DeadCode/UnusedPrivatePropertyRule.php:68`) gates entry into the
entire rule on `if (!$property->isPrivate()) { continue; }`, checking
only the base `Modifiers::PRIVATE` bit - entirely independent of
`Modifiers::PRIVATE_SET`/`PROTECTED_SET`, which back separate methods on
the very same class. So a PHP 8.4 property declared
`public private(set) string $x` is skipped from the very first line, even
though its write access is exactly as restricted to the declaring class
as a fully-private property's - the "never written, only read"
diagnostic this same rule already computes for plain-private properties
is structurally unreachable for its asymmetric-visibility sibling.
phpstan-rs's own mirrored scan currently has the identical gate
(`if p.visibility != Visibility::Private { continue; }`,
`checker/unused.rs:216`) - but its `PropertySig` already carries
`write_visibility` as a field entirely separate from `visibility`
(`registry/class_info.rs:41`), already consulted independently for real
property-assignment access-checking and generic variance elsewhere in the
same codebase. Extending `unused.rs`'s entry gate to also admit
`p.write_visibility == Visibility::Private` needs no new data at all -
unlike B1 (which needs a whole new cross-file usage-graph aggregation,
effort: large), this is same-file, needing only a field phpstan-rs
already computes and already uses elsewhere, just never consulted by
this one check.

**Effort: small.**

---

## AE. Assertion-less test method detection ("risky test," as a static check)

A new checker module flags any PHPUnit-style test method (a public method
on a class where `is_subclass_of(class, "PHPUnit\Framework\TestCase")`,
named `test*` or carrying a `#[Test]` attribute) or Pest test closure (an
`it(...)`/`test(...)` call, detected the same way `pest_config.rs::
is_this_binding_call` already recognizes Pest's hook functions) whose
body contains zero assertion-shaped calls anywhere, recursively - no
`assert*` MethodCall/StaticCall, no explicit `Assert::assert*`, and for
Pest no `expect(...)` call. A disclosed narrowing to ship first: exclude
a body that calls `expectException*`/`markTestSkipped`/
`markTestIncomplete`/`addToAssertionCount`, matching real PHPUnit's own
runtime notion that these also count toward "not risky." Grepping every
rule file under both vendored trees case-insensitively for "risky",
"EmptyTest", or "assertion" returns zero hits - the concept doesn't exist
anywhere in either ruleset (PHPStan's own `PhpDoc/{Method,Function}
AssertRule.php` is about unrelated `@phpstan-assert` PHPDoc narrowing
tags, not test-runner assertion-call auditing). This is normally caught
only by PHPUnit's own `--fail-on-risky`/`beStrictAboutTestsThatDoNotTest
Anything` *runtime* flag - PHPStan itself has no static rule for it.
phpstan-rs already has every piece this needs, built for unrelated
features: `is_subclass_of` already answers the TestCase question (the
same call `pest_config.rs`'s default-`$this`-type fallback already
relies on, backed by a real vendored `PHPUnit\Framework\TestCase` stub);
`checker/narrowing/assertions.rs::apply_phpunit_assertion_narrowing`
already maintains and matches against the full real PHPUnit `assert*`
method-name set, directly reusable as the "is this call assertion-shaped"
predicate; the recursive whole-body scan pattern already exists twice
over (`checker/uninitialized_dataflow.rs`'s `walk_stmts`/`walk_expr`,
`checker/noop.rs`'s conservative subtree scan). A composition of four
already-proven, already-vendored-and-grounded mechanisms, no new
architecture needed.

**Effort: medium.**

---

## AF. Opt-in algorithmic-complexity heuristic: linear lookup into a pre-built array from inside an unrelated loop

Explicitly heuristic, like D6/D7 before it. Inside a loop body (`self.
loop_depth > 0`, already-live infrastructure - `checker.rs:728`, already
consulted directly by other heuristic checks at
`checker/expr/probe_checks.rs:154,209,379,454`), when the checker sees a
call to `in_array()`/`array_search()`/`array_key_exists()` whose haystack
is a plain variable (not an inline literal) with an unbounded `Type::
Array(k, v)` shape rather than a small fixed `ArrayShape`, that variable
is never appended to anywhere in the enclosing loop body (so it isn't
being progressively built - a legitimate incremental-lookup pattern), and
it isn't the loop's own iteration subject (so it isn't a self-scan/dedup
pattern), emit a low-confidence `loop.quadraticLookup` diagnostic:
"linear lookup into an already-built array repeated once per outer-loop
iteration - O(n*m); consider `array_flip()`/`isset()` or building an
index before the loop." Ships strictly opt-in with an explicit
false-positive caveat that a small-but-unbounded-at-runtime haystack
makes this a non-issue the check can't resolve - the same acknowledged
low-confidence framing D6/D7 already establish as acceptable for this
category. PHPStan's entire `Rules/` catalog reasons about types,
existence, and soundness, never runtime execution cost - grepping both
vendored trees for "complexity"/"quadratic"/"performance" as a rule
concept returns nothing; there is no rule keyed on a lookup's placement
relative to a loop, or on an array's role as a repeatedly-scanned lookup
table. Distinct from D1 (which reasons about a function's own
control-flow shape, never a variable's role as a collection) and D6/D7
(single-call-site/single-expression checks with no loop-context or
collection-role reasoning at all) - keyed on a loop's own iteration
relationship to a separate collection's construction site.

**Effort: medium.**

---

# Part 4: Ecosystem - `phpstan-strict-rules` ports and `Won't-port` corrections

Grounded directly against the vendored companion package
`vendor/phpstan-strict-rules/src` (a git submodule, matching the existing
`vendor/phpstan-src` precedent in `.gitmodules`) - every citation below
resolves to real, on-disk source, not an assumed API.

## E. Ecosystem checks ported from `phpstan-strict-rules`

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
own abstract/private/deprecated-skip logic. (I4 in Part 6 is the codemod
that attaches to this once it ships.)

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
names already in hand at that call site is additive, not a new pass. (I1
in Part 6 is a shared codemod covering this and every other `.nameCase`
diagnostic.)

**Effort: small.**

---

## W. More `phpstan-strict-rules` "stricter defaults" config flags

A distinct axis from section E's custom Rule classes: three gaps where a
`phpstan-strict-rules` config flag's stricter behavior has no phpstan-rs
equivalent at all, opt-in or otherwise - each confirmed against the real
gating mechanism, not assumed.

### W1. Opt-in strict `checkDynamicProperties` behavior inside `isset()`/`empty()`/`??`

Real PHPStan's `AccessPropertiesCheck::processSingleProperty` only
reaches its undefined-property leniency branch when `hasInstanceProperty`
answers "maybe" (the common case for any non-final class without the
property declared, `Type/ObjectType.php:212-231`); when the access is an
`isset()`/`empty()`/`??`/`??=` root, the check returns `[]` outright
unless `checkDynamicProperties` is true - default `false`
(`Rules/Properties/AccessPropertiesCheck.php:135-151`,
`phpstan-strict-rules/rules.neon:6`). phpstan-rs's
`check_expr_quiet_root` (`checker/expr/probe_checks.rs:79-115`) and the
isset/empty existence-probe handling (`checker/expr.rs:319-348`) already
unconditionally suppress this check for every such root, matching
PHPStan's *default* leniency - but there is no config flag or code path
that ever runs the check in that context, so phpstan-rs cannot currently
reproduce strict-rules' stricter behavior at all, even opt-in. A project
toggle routing an isset-root `PropertyAccess` through the normal
`member_confirmed_missing` check (`checker/expr.rs:2295-2308`) instead of
the blanket skip is small and additive, reusing machinery that already
exists.

**Effort: small.**

### W2. Opt-in strict printf/sprintf placeholder-type checking

phpstan-rs's `format_specifier_accepts` (`checker/calls/format_string.rs:
364-376`) is explicitly documented as "deliberately lenient," flagging
only an array or object argument - matching real PHPStan's own
non-strict default (`checkStrictPrintfPlaceholderTypes = false`;
`StringType::toInteger()`/`toFloat()` never return `ErrorType`,
`Type/StringType.php:161-169`). The gap is the *strict* alternative
(default `false` in core, `true` in strict-rules,
`phpstan-strict-rules/rules.neon:14`) - rejecting `null`/`bool` for
`%s` (only string/Stringable/float allowed), requiring `%d` to be int or
a decimal-int-string and `%f` to be float or numeric-string
(`Rules/Functions/PrintfPlaceholder.php:31-70`) - has no phpstan-rs
equivalent at all. The same call site already has full access to the
resolved argument `Type` (including whether a string is a decimal-int-
string/numeric-string literal, since `Type::IntLiteral`/`StringLiteral`
are already modeled) needed to implement the strict variant additively
alongside the existing lenient one.

**Effort: medium.**

### W3. Opt-in call-site function/method name-case checking

phpstan-rs's own comment (`checker/imports.rs:66-70`) already traces
`checkFunctionNameCase`'s upstream default (`false`) to
`conf/*.neon`, so building nothing by default already matches - but
strict-rules flips it on (`rules.neon:8`) across five rule files
(`CallToNonExistentFunctionRule.php:63-79`, `MethodCallCheck.php:168`,
`StaticMethodCallCheck.php:336`, `FunctionCallableRule.php:61`,
`ExistingNamesInUseRule.php:110`/`ExistingNamesInGroupUseRule.php:109`),
flagging a *call* whose written name-casing differs from the resolved
declaration - distinct from E4's `WrongCaseOfInheritedMethodRule` (which
flags a *declaration* whose casing mismatches its ancestor's declaration;
E4's own text confirms "no inherited-method-name-casing check exists
anywhere," leaving the call-site family entirely untouched). phpstan-rs's
function/class lookups are already deliberately case-insensitive with a
documented rationale (`Registry::lookup_function`,
`registry/hierarchy.rs:623-630`) - exactly the precondition a
case-mismatch diagnostic needs (resolve first, compare resolved canonical
name against as-written name), the identical pattern the existing
`class.nameCase` check already implements end-to-end
(`checker/resolve.rs:323-352`, `registry/name_context.rs:117-124`).
Extending that same mechanism to function calls, method calls, static
calls, and `use function` imports is additive.

**Effort: medium.**

---

## Z. `Won't-port`-mis-bucketed rules: registered only for phpstan.org's own issue-reproduction bot

### Z1. Standalone numeric-string/float/bool array-key cast warnings

Real PHPStan's `LiteralArrayKeyCastRule` (array-literal keys) and
`ArrayDimCastRule` (`$arr['5']` read/write dim-fetch on any array-typed
expression) are fully working, general-purpose, and need no third-party
config to activate - but both live under `Rules/Playground/` and are
registered *only* in `vendor/phpstan-src/issue-bot/playground.neon`,
phpstan.org's own private issue-reproduction bot config. Grepping every
`conf/*.neon` file (`config.neon`, `parametersSchema.neon`) for
"Playground" returns zero rule-registration hits - no installable
package, level config, or strict-rules extension ever turns these on for
a real project. A pure product-distribution constraint, not a technical
one: whenever a constant scalar array key's PHP-coerced form differs from
what's written (`'5'` -> int `5`, `'08'` stays string since not
canonical, `1.5` -> int `1` truncated, `true`/`false` -> `1`/`0`), these
rules flag the silent cast - standalone, not only when it happens to
collide with another key in the same literal. phpstan-rs's existing
`array.duplicateKey` check (`checker/tests/array_key_validity.rs:
404-419`) only fires on an actual collision; it never fires standalone
and has no dim-fetch counterpart. phpstan-rs ships one binary/config
surface to every user - there's no separate "internal demo" tier
withholding a check from the shipped CLI - and already has the exact
primitive needed: `is_canonical_int_key` (`checker/expr.rs:4092-4096`)
plus constant-key tracking already threaded through array-literal
building (`checker/expr/array_ops.rs:276,754`). (AA5 above is the same
Playground-only mis-bucketing, for `never`-return-type suggestions
instead of array-key casts.)

**Effort: small.**

---

# Part 5: LSP / editor-native capabilities

Real PHPStan ships no first-party LSP or interactive query API at all -
`FileAnalyser::analyseFile` (`Analyser/FileAnalyser.php:81`) is its sole
analysis entry point, always a whole file, always from a fresh CLI
invocation, with no resident process and no persisted per-file/per-project
index of any kind. phpstan-rs's LSP server already keeps a resident,
incrementally-updated `Project`/`Registry` (`src/project.rs:69-114`) - the
items below are gaps in already-built plumbing, not new architecture. This
is the cluster most directly relevant to a future VSCode extension for
realtime incremental analysis (see also `VSCODE_EXTENSION_PLAN.md`, a
separate, synthesized execution plan for the LSP server's own
trustworthiness/concurrency/protocol gaps rather than net-new capabilities
like these).

## F. Single-cursor request/response capabilities

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
site. (AM2 below depends on this same gap for one of its two cases.)

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
Part 6 below, which lists exactly which existing diagnostics are safe
to populate `.fix(...)` for first.

**Effort: small** (the wiring); populating individual checks is Part 6.

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
(AH3 and AO1 below both depend on this same index.)

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
later", `project.rs:67-68`). Distinct from section Q below (whole-program
API BC-diffing): Q compares two independently-loaded, disjoint registry
snapshots to report public-surface breaks as the actual product; this
compares shapes within one live incremental session on the *same* tree to
drive re-check scheduling.

**Effort: large.**

---

## Y. Passive LSP rendering: `semanticTokens` and `inlayHint`

Distinct from section F (single-cursor request/response) - these are
request/response-free, editor-painted continuously rather than triggered
by user action.

### Y1. `textDocument/semanticTokens` push-highlighting for deprecated/internal symbol usage

Advertise `semanticTokensProvider` and, at every call/property/const-
access/instantiation site, emit a token modifier (LSP's standard
"deprecated" modifier, plus a custom "internal" one) whenever the
resolved target's `FunctionSig`/`PropertySig`/`ConstSig.deprecated` is
`Some(_)` or `is_internal` is true - computed *unconditionally*, not
gated behind `Registry::check_deprecated_usage`/`check_internal_usage`
(both default `false`, `src/registry.rs:102-111`): the checker already
looks up the callee's sig at every such site for ordinary type-checking
regardless of those flags, so the deprecated/internal *fact* is a free
byproduct of resolution already happening; only the diagnostic-emission
call is gated. This gives continuous strike-through/coloring in the
editor even for a project that (like most real ones, per `registry.rs`'s
own comment) never turns the corresponding diagnostic on. Real PHPStan
has no LSP and no push-rendering channel at all - deprecation warnings
surface only via `phpstan/phpstan-deprecation-rules`, "a separate,
opt-in Composer package, not even part of core phpstan/phpstan"
(`registry.rs:102-106`), and even then only as a batch CLI diagnostic on
the next full run, never a live per-keystroke visual cue.

**Effort: medium.**

### Y2. `textDocument/inlayHint`: parameter-name hints (near-free) vs. inferred-type hints on unannotated locals (materially new work)

Two sub-features, honestly different costs. **(a) Parameter-name hints**
before each call argument: `resolve_call_args` already resolves every
argument (positional/named/spread/variadic) to its exact declaring
`ParamSig` - including `ParamSig.name` (`registry/function_sig.rs:24-25`)
- as part of ordinary call-argument checking; exposing that already-
computed name/span pairing as an `inlayHint` response is close to free,
no new inference. **(b) Inferred-type hints** after an unannotated
`$x = ...`: this is *not* a small extension of F1's `probe_hit`
mechanism - `probe_hit`/`probe_var_provenance` are single-target
(`Checker::probe_span: Option<Span>`, an exact equality check at a
handful of call sites, `checker.rs:775-776`), built for one throwaway
re-check per user click. `inlayHint` needs every relevant span's type
across the whole open file, continuously (re-rendered on every
`didChange`, not once per click) - a genuinely new collection mode (an
unconditional `Vec<(Span, Type)>` populated at every assignment during
one ordinary pass), paying a full-file cost on every edit rather than the
probe's near-zero per-site cost. Real PHPStan's only way to inspect an
inferred type at all is editing the source to insert `PHPStan\dumpType()`
and re-running the whole CLI - no channel, on-demand or continuous, for
rendering a type or resolved parameter name without modifying source,
since it has no LSP and no editor-facing rendering concept at all.

**Effort: medium.**

---

## AH. Whole-file/whole-project symbol lists: `documentSymbol`, `workspace/symbol`, `codeLens`

Distinct from section F (single-cursor-position request/response) and
section Y (per-token passive painting) - these operate over whole-file/
whole-project symbol *lists*, keyed by no cursor position at all.

### AH1. `textDocument/documentSymbol`: per-file hierarchical outline

Advertise `document_symbol_provider` and add a `"textDocument/
documentSymbol"` arm to `handle_request`. Resolve the file's `&[Stmt]`
via `Project::ast(path)` - already used by `lsp::navigation`, and
notably *not* gated behind `retain_source` unlike `decl_locations`/
`source` - then walk its top-level `ClassDecl`/`EnumDecl`/`FunctionDecl`
variants, emitting one `DocumentSymbol` per declaration using fields
every declaration node already carries: `name_span` (for
`selectionRange`) and the whole-declaration `span` (for `range`), both
present on `FunctionDecl`, `ClassDecl`, `PropertyDecl`, `EnumDecl`/
`EnumCase`. A class's own `methods`/`properties`/`consts`/`cases` fields
become nested `children`. This is exactly the declaration set
`DeclLocations::index` already extracts per-file into two flat global
maps - the same walk, restructured into one file's hierarchical response
instead of a project-wide flat index. Real PHPStan ships no LSP, outline,
or resident symbol-list capability of any kind - `FileAnalyser::
analyseFile` is its sole entry point, always a fresh whole-file CLI
invocation with no persisted per-file symbol list to query interactively.
`lsp-types = "0.97.0"` (already a dependency) already exposes
`DocumentSymbol`/`document_symbol_provider`.

**Effort: small.**

### AH2. `workspace/symbol`: project-wide fuzzy symbol search

Advertise `workspace_symbol_provider` and add a `"workspace/symbol"` arm,
reading the query string and matching it (case-insensitive substring, or
a light fuzzy/subsequence match) against every entry in `Project::
decl_locations()` - its `classes`/`functions`/`members` maps already
cover every declared class/function/method/property/const project-wide,
each already carrying a real `(PathBuf, Span)`. Two genuinely new pieces:
(a) the query/filter mechanism itself - no substring/fuzzy-match helper
exists anywhere in the codebase today; (b) each match's `SymbolKind`,
since `DeclLocations` stores only `(path, span)`, never a kind - resolved
via `registry.classes.get(fqn).kind` for a class entry, or the same
three-way `lookup_method_with_owner`/`lookup_property_with_owner`/
`lookup_const_with_owner` probe `format_member_hover` already performs
for a member entry. Unlike `documentSymbol`'s `FileState.ast`,
`DeclLocations` *is* gated behind `retain_source` - but `lsp::Server::
new()` unconditionally turns that gate on before any request can arrive,
so by the time a `workspace/symbol` request is possible the index is
already in the right mode, with no separate prerequisite gap to close
(worth stating explicitly: this relies on a gate that happens to already
be unconditionally on for this server, not on ungated-by-construction
data the way `documentSymbol` is). Distinct from F3 (find-references/
rename, which needs a brand-new use-site span index `DeclLocations`
doesn't have and never will for that purpose) - `workspace/symbol` only
ever needs declaration-site spans, which already exist, plus a
name-matching query layer, which doesn't.

**Effort: medium.**

### AH3. `textDocument/codeLens` reference-count annotations above declarations

Advertise `code_lens_provider` and add a `"textDocument/codeLens"` arm to
`handle_request`. For each declaration in the requested file (walked the
same way AH1's `documentSymbol` walks `Project::ast(path)`), emit one
`CodeLens` whose title is a use-site count (e.g. "3 references") sourced
by aggregating F3's *proposed* use-site span index - grouping its
per-symbol spans by declaration and taking the count. This genuinely
depends on F3 (not yet built). Two pieces are new work beyond
what F3 itself builds: per-declaration count aggregation (F3's own
description only frames its index as enumeration for `references`/
`rename`, never as a rollup count), and the `CodeLens` response
serialization/wiring itself, which touches none of F3's existing code
paths. Real PHPStan ships no LSP or resident analysis server of any kind
- `FileAnalyser::analyseFile` is its sole entry point, always a fresh
whole-file CLI invocation with no persisted cross-file use-site index and
no channel to push editor-rendered annotations without modifying the
analyzed source. Distinct from F1-F3 (single-cursor request/response,
none of which render anything above a declaration without a cursor
action), AH1/AH2 (whole-file/whole-project symbol *lists* returned as a
flat/hierarchical response, never an inline per-declaration annotation),
and Y1/Y2 (per-token/per-argument decorations painted continuously during
editing, not a per-declaration summary line above it) - `codeLens` is a
fourth, structurally separate LSP request type none of them address.

**Effort: medium.**

---

## AM. `signatureHelp` and `typeDefinition`

### AM1. `textDocument/signatureHelp`: active-parameter tooltip while typing inside a call's parentheses

Distinct from Y2 (inlayHint's parameter-name hints, which render
statically before an already-*complete* argument once `resolve_call_args`
has already resolved it) - this is the first item in this document keyed
on cursor position relative to argument-list boundaries during *active
typing*, including the not-yet-typed-argument case none of the
already-complete-expression mechanisms (F1, Y2, X1) ever need to handle.
At the request position, find the innermost enclosing call node whose
own span *contains* the cursor - a genuinely different query shape than
`navigation::resolve_at`'s existing per-child-span match, which only
fires when the cursor lands exactly on an already-parsed sub-expression's
own span, since the cursor here is typically in the gap between two
argument spans, or a trailing empty slot after a comma with no argument
expression there at all yet. Resolve the callee's `FunctionSig`/
`ParamSig` list the same way an ordinary call-argument check already
does, then respond with one `SignatureInformation` per parameter plus an
`active_parameter` index computed by counting paren-depth-aware commas
in the argument-list source text preceding the cursor. Two honest scope
notes: this needs a genuinely new query mode (span-containment/gap
search, not the exact-span-equality matching `resolve_at`/`probe_span`
already do); and the parser is single-pass with no error recovery
(`ParseError` aborts the *whole file's* parse, confirmed no partial/
error-node AST exists anywhere) and `Project::update_file` skips applying
an edit entirely when the new text fails to parse (its own comment:
"momentarily-invalid syntax is the normal state while a file is being
typed") - so a robust implementation must serve signature help from the
last-successfully-parsed AST still held, not assume the just-typed
keystroke's own text parsed at all, which is the common case right when a
user is mid-way through inserting a new argument after a fresh comma.

**Effort: medium.**

### AM2. `textDocument/typeDefinition`: jump from an expression's inferred type to that type's own declaration

Distinct from plain `textDocument/definition` (which jumps to the thing
being *referenced* - a variable's assignment site, a called method's own
declaration) and from F1 (which never navigates anywhere, only renders
text) - typeDefinition jumps from an already-resolved `Type` to that
type's *own* declaring symbol. Two genuinely different cost tiers
depending on what's under the cursor. **(a)** Cursor on a member-access
receiver (`$x->foo()`/`$x->prop`): `probe_hit` is already populated for
exactly this case, so `probe_receiver_type` already returns the real
`Type` with zero checker-side changes - only new LSP-side work is
feeding that `Type` through the already-defined `type_to_classes` helper
and looking each resulting class name up in `decl_locations().classes`
for its own declaration span. **(b)** Cursor on a bare local variable not
used as a receiver: this needs the exact same fix F1 already names and
describes as effort:small - `ExprKind::Variable`'s arm currently only
stashes `probe_var_provenance` (assignment-site spans) and still does not
set `probe_hit`, confirmed by reading that exact arm in the current
working tree - so typeDefinition on a bare variable is blocked on
completing F1's already-identified gap first, not a second independent
gap (disclosed explicitly, not presented as independent).

**Effort: medium.**

---

## AO. `willRenameFiles` and `documentHighlight`

### AO1. `workspace/willRenameFiles`: PSR-4-consistent rename-file refactoring across the whole project

Distinct from F3 (a cursor-triggered `textDocument/rename` for a symbol,
which - as F3 itself discloses - doesn't exist yet either): this is
triggered by a file-explorer drag/move with no cursor position at all,
and its core computation (deriving a new expected FQN from a new file
path) is the *reverse* direction of what F3 does - F3 only ever resolves
symbols already named in source text, never infers a name from a path.
Also distinct from R1 in Part 8 (which flags a path/FQN mismatch as a
diagnostic on files that already sit where they are - it never fires on
a rename event, never computes a *new* FQN, and never rewrites anything):
this performs an active edit at the moment of the rename gesture, using
R1's underlying prefix-map data but the mirror-image arithmetic
(path-to-FQN, not FQN-to-path). On a file-explorer move of a `.php` file,
compute the FQN the file *used* to declare via a `DeclLocations.classes`
reverse lookup and the FQN the *new* path implies (a new `path_to_fqn`
computation: longest-prefix match the new path against each
`autoload.psr-4`/`autoload-dev.psr-4` entry, strip the matched
directory, turn remaining segments into namespace segments, use the
filename stem as the short class name). If the two FQNs differ, respond
with one `WorkspaceEdit` rewriting the moved file's own `namespace`/
`class` declaration plus every project-wide use-site span of the old FQN.
Real PHPStan ships no LSP or IDE-integration module of any kind - the
same structural absence this whole part documents. Honestly disclosed as
depending on two other proposed, not-yet-built items - R1's PSR-4
prefix-map access and F3's use-site span index.

**Effort: large.**

### AO2. `textDocument/documentHighlight`: same-file occurrence highlighting, needing none of F3's dependency

Highlights every occurrence of the symbol under the cursor *within the
current file only* - deliberately distinct from F3's whole-project
find-references, which needs a brand-new use-site span index that isn't
built yet. This is a small, self-contained generalization of code that
already exists and works today: `navigation::resolve_at` already walks
one file's already-parsed AST with a `Walker` that, for every name
reference/declaration, resolves it to a concrete symbol identity via the
same `NameContext`/`Registry` machinery hover and definition already
share, and keeps only the single match whose span contains the cursor
(`Walker::record` replaces `best` unconditionally on every contained
span). `documentHighlight` needs the identical traversal and resolution,
just run twice: once at the cursor position to get the target identity
(exactly today's `resolve_at`), then a second full walk of the same file
collecting every span whose resolved identity equals that target (same
class FQN, same owner+member via the existing `resolve_member_owners`
probe, or - for a local variable - every read/write reachable from the
same enclosing scope `ProbeScope` already delimits). No new resolution
logic, no new index, no cross-file plumbing at all: it's the existing
`Walker::record` single-best-match pattern turned into a `Vec`
accumulator, run against the one file already resident in `Project`.
Unlike F3 (and unlike several other LSP findings above), this genuinely
needs none of F3's use-site index - `documentHighlight`'s LSP contract is
explicitly single-file by spec, so the current file's AST, walked once,
already carries every occurrence hover/definition would each individually
resolve to the same identity. Real PHPStan ships no LSP or interactive
query API of any kind, the same structural absence established
throughout this part.

**Effort: medium.**

---

## T. Diagnostic explainability: derivation traces, not just a final type

Real PHPStan's only type-introspection tool, `PHPStan\dumpType()`,
computes and prints one final type per call site via a plain `sprintf`
(`Rules/Debug/DumpTypeRule.php:32-50) with no accumulated reasoning kept -
each `Scope::getType()` call that fed it is discarded the instant the
message string is built, and it requires editing the analyzed source and
re-running the whole CLI, since PHPStan ships no LSP or resident process
to query interactively at all. Distinct from F1 (a single inferred `Type`
wired into hover for one span, computed fresh each probe): both items
below trace the *chain* of operations that produced a type, an entirely
different data shape.

### T1. Interactive branch-narrowing derivation trace for a diagnostic

Extend `Checker::apply_narrowing` (`src/checker/narrowing.rs:38`, called
from ~30 sites across the checker) so that, under an opt-in trace mode,
each narrowing step pushes a (condition span, branch taken, mechanism,
before-type, after-type) record into a new per-`Locals` side-channel
modeled directly on the existing `provenance: ProvenanceMap` field and its
`track` gate (`checker.rs:113-132,224`). At a branch join, `merge_scopes`
(`control_flow.rs:403-439`) already shows exactly how such a side-channel
composes across paths - it unions types via `union_types` and does
`provenance.extend(...)` - so a trace channel merges into a DAG (both
branches' contributing steps kept) with zero new merge logic to invent.
Surface it by reusing the exact one-shot instrumented-recheck pattern
already proven for hover: `probe_span`/`probe_hit`
(`checker.rs:775-776`) plus `run_probe_scope`/`ProbeScope`
(`lsp.rs:321-378`) - a new LSP request passes the diagnostic's own span
and narrowing key instead of a hover position, re-runs `check_function`
once with tracing on, and returns the recorded step DAG. For a
non-interactive CI fallback, render a summary into the diagnostic's own
`metadata: HashMap<String, serde_json::Value>` field (`checker.rs:327-333`,
which every `Diagnostic` already carries and no current call site
populates).

**Effort: large.**

### T2. Per-pass loop-convergence widening trace, for a diagnostic whose type came from fixed-point iteration rather than branch narrowing

`Checker::converge_loop_body` (`control_flow.rs:331-384`) runs up to
`MAX_PASSES = 3` passes, each producing a full `Locals` clone unioned via
`merge_scopes` into the next entry state - then, for every pass except the
one finally kept, explicitly discards both that pass's diagnostics
(`self.diagnostics.truncate(...)`) and its `Locals` state. For a
diagnostic on a loop variable whose converged type built up over several
passes (pass 1 sees `int`, pass 2's branch adds `string`, final
convergence adds `null`), under the same trace mode as T1, a lightweight
per-pass diff can be appended to a trace record instead of being silently
overwritten - "pass 2 added `string` via this branch; the final pass
added `null` via that one." Structurally different from T1: iteration-
indexed widening has no controlling condition at all, only a pass index,
so it needs its own step kind rather than reusing the narrowing-step
shape. Real PHPStan's identical fixed point
(`NodeScopeResolver::LOOP_SCOPE_ITERATIONS = 3`,
`NodeScopeResolver.php:120`) discards each pre-convergence pass's scope
the same way, but additionally widens literal/constant types to their base
type after only the first candidate pass
(`GENERALIZE_AFTER_ITERATION = 1`, line 121) - destroying the literal type
identity a trace would even need to show, on top of having no interactive
query surface at all. phpstan-rs's convergence loop does plain type-union
merging with no early generalization (A1 in Part 1 already documents this
precision gap in the *other* direction - raising the pass cap for more
precise final types; this is about retaining and exposing the discarded
intermediate passes for explainability instead) - each pass's precise
`Locals` state is already fully materialized in memory immediately before
being thrown away purely to keep iteration bounded, so capturing a
before/after diff there is retaining data that already exists for one more
statement's worth of scope, not computing anything new.

**Effort: medium.**

---

# Part 6: Codemods

Four concrete fixes, each attaching to an existing (or, for one, an
E-section-proposed) diagnostic via the `DiagnosticFix` plumbing F2
wires up. Real PHPStan's entire autofix surface (`RuleErrorBuilder::
fixNode`) is used by only 4 rule files project-wide (`grep -rl fixNode
vendor/phpstan-src/src/Rules/` -> two `Overriding*Rule.php`, `Operators/
BacktickRule.php`, `Playground/PromoteParameterRule.php`) - none of the
diagnostic families below have any real-PHPStan autofix at all.

## I. Codemods attached to diagnostics phpstan-rs already ships

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

# Part 7: Config/strictness composability beyond PHPStan's single global level

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

## J. Config/strictness composability

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

# Part 8: Composer/dependency hygiene

Eight distinct axes, each keyed off a different `composer.json` section
and a different failure mode. PHPStan's `Rules/` catalog has no
package-declaration-aware check of any kind (grepping both vendored trees
for "composer" as a rule concept turns up nothing beyond unrelated API/
bootstrap code) - this is real and structural: PHPStan analyzes one
process per run with no persistent whole-program "which package provides
this symbol" index kept live to query against composer.json's
declarations, and building one would duplicate work its own architecture
discards after each run. phpstan-rs already has most of the underlying
pieces independently built for other reasons: `composer::
find_composer_package_name` (`src/composer.rs:144-155`) already walks a
file's directory upward to its owning composer.json and returns its
package name; `composer::detect_phpstan_dependency` (`composer.rs:
112-127`) already parses `require`/`require-dev` via `serde_json`;
`deps::referenced_names` (`src/deps.rs:26-32`) already collects every name
a file references; `Project`'s `DeclLocations`
(`project/decl_locations.rs:18-22`) already maps a declared FQN to its
owning file - currently gated behind `retain_source` for LSP-only use
(`project.rs:90-99`) rather than populated for batch `check-project` runs,
a shared prerequisite several items below note explicitly.

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
- breaks under a production `composer install --no-dev`).

**Effort: large.**

## R. PSR-4 filesystem consistency

Real PHPStan's only PSR-4 awareness is consuming `Psr4Mapping` purely to
*locate* classes for its BetterReflection source locator
(`Reflection/BetterReflection/SourceLocator/
ComposerJsonAndInstalledJsonSourceLocatorMaker.php:90-192`) - it never
validates that a class's actual location matches where PSR-4 says it
should be. `Rules/Namespaces/` contains exactly 2 files, both about
`use`-statement resolution; a case-insensitive grep for "psr-4"/"psr4"/
"autoload" across every `Rules/` file in both vendored trees returns zero
hits.

### R1. PSR-4 declared-namespace vs. file-path consistency (`psr4.pathMismatch`)

For every declared class/interface/trait/enum FQN, resolve which
`composer.json` `autoload.psr-4`/`autoload-dev.psr-4` prefix it should
fall under (longest-prefix match), compute the file path PSR-4 requires,
and compare against the file it was actually declared in (already
resolved via `DeclLocations.classes: FxHashMap<FQN, (PathBuf, Span)>`).
Flag a mismatch - the file loads fine today only because Composer's
non-optimized/dev autoloader falls back to directory scanning; a
production `composer dump-autoload --optimize` generates a static
classmap purely from PSR-4 resolution and silently drops the class,
producing a runtime "Class not found" with zero PHPStan-shaped warning
today. phpstan-rs already computes almost every piece as a side effect of
other work, just not wired together: `DeclLocations.classes` already maps
every declared FQN to its real file path project-wide, currently gated
behind `retain_source` for LSP-only use rather than populated for batch
`check-project` runs (`project.rs:100,107,238`); `composer::
discover_vendor_context`'s `collect_dirs`/`eval_path`
(`composer.rs:260-300`) already parses the same generated
`prefixDirsPsr4` array Composer compiles from `composer.json`'s
`autoload.psr-4` (root package included) - it just explicitly discards
every entry that doesn't resolve under `vendor/`
(`eval_path`'s `starts_with(vendor_dir)` filter, by design per the
module's own doc comment), throwing away exactly the first-party
`App\ => app/`-style rows this check needs. Building it: stop discarding
those rows (or parse `composer.json`'s own `autoload`/`autoload-dev`
sections directly via `serde_json`, the same pattern
`detect_phpstan_dependency` already uses), and cross-reference against
`DeclLocations` once enabled for batch runs - no new filesystem-walking
or parsing machinery. (AO1 above reuses this same prefix-map data for
the reverse direction: deriving an expected FQN from a *new* file path on
a rename.)

**Effort: medium.**

### R2. PSR-4 exact-case path mismatch, as its own diagnostic (`psr4.caseMismatch`)

Once R1 exists, add a narrower diagnostic for when the expected and
actual paths match case-*insensitively* but differ in letter-casing on
some segment (class `App\Http\Controller\UserController` declared in
`app/Http/controller/UserController.php` - lowercase `controller`). Must
use a byte-exact comparison against real, on-disk directory-entry casing
(`std::fs::read_dir`, never `std::fs::canonicalize`, which silently
normalizes case on a case-insensitive volume and would make the bug
invisible on the very platforms where it happens to still work). Real
PHPStan's `Psr4Mapping`-consuming lookup only needs to *find* the file at
all - on the case-insensitive filesystems (macOS default, Windows) most
PHPStan contributors and CI runners are equally likely to develop on, the
lookup silently succeeds either way, so there is no PHPStan mechanism
that would ever observe the mismatch. The real payoff: the "works on the
author's laptop, breaks in the Linux/Docker production container" bug
class, one real PHPStan structurally cannot report on because its own
contributors and test infrastructure run on the same case-insensitive-by-
default platforms that make the bug invisible to observe. Distinct from
the existing `class.nameCase`/E4 checks, which compare an in-source
*reference*'s casing against its own declaration's casing within PHP text
- this compares a declaration's casing against the real filesystem, a
cross-domain (source-vs-disk) comparison neither touches.

**Effort: small.**

## AC. `misplaced.autoloadDevSymbol`: production code reaching a class only reachable under `autoload-dev`

A third, distinct composer-hygiene axis from K (whole-*package*
require/require-dev declaration hygiene) and R (a symbol's *own* declared
location vs. where PSR-4 says it should live) - this asks whether a
symbol is even loadable under a real production (`--no-dev`) install at
all, given how the project's *own* `composer.json` splits
`autoload.psr-4` vs `autoload-dev.psr-4`. For every resolved symbol
reference, determine which PSR-4 root (if any) the *call-site file*
falls under (production `autoload.psr-4` vs. dev-only
`autoload-dev.psr-4`, longest-prefix match) and the same for the
*declaration site*; flag when the caller resolves under production
`autoload.psr-4` but the callee's only declared location resolves under
`autoload-dev.psr-4` and not also under production - `composer
dump-autoload --no-dev` drops the dev PSR-4 root entirely, so the class
is simply absent from the generated classmap and the reference fatals at
runtime with zero warning today, even though every normal dev-mode
install (what CI and every local phpstan-rs/PHPStan invocation actually
uses) resolves it fine. Real PHPStan's only autoload-dev-aware code reads
a single `dev` boolean from `vendor/composer/installed.json` for the
*entire current process* - it has no notion of "would this symbol still
resolve under a *different*, hypothetical `--no-dev` install than the one
actually running." A grep for "autoload-dev" across every `Rules/` file
in both vendored trees returns zero hits. phpstan-rs already has every
piece except the forward PSR-4-prefix-to-file lookup: `composer.rs`
already parses a project's hand-authored `composer.json` via `serde_json`
for `require`/`require-dev` and `name` - extending that same parse to
also read `autoload.psr-4`/`autoload-dev.psr-4` is a direct extension of
proven code. `DeclLocations.classes` (`project/decl_locations.rs:18-22`)
already maps every declared symbol to its real file path project-wide
(gated behind `retain_source`, same gap R1 already documents needing to
close for batch runs). Notably sidesteps the "is this a test file"
classification gap phpstan-rs has today (no general test-file heuristic
exists, only `Pest.php`-runner-config detection) - the project's own
composer.json already makes the production/dev split explicit and
machine-readable, so a file covered by neither PSR-4 section just
degrades to "no diagnostic," never a guess.

**Effort: medium.**

## AG. `provide.unimplemented`: verify a composer.json `provide` claim against a real registry-wide interface implementation

Distinct from K, R, and AC. For every entry in the project's own root
`composer.json` `"provide"` map, resolve the small set of formally-
specified, stable 1:1 PHP-FIG virtual packages
(`psr/log-implementation` -> `Psr\Log\LoggerInterface`,
`psr/container-implementation` -> `Psr\Container\ContainerInterface`, and
five siblings - each a single, PHP-FIG-published package-to-interface
mapping, not a heuristic) to its target interface FQN via a small
hardcoded table, then check via `Registry::is_subclass_of` (already
transitive over `extends`+`implements`) whether *any* project-declared
class implements it. Flag `provide.unimplemented` when a project claims
e.g. `"provide": {"psr/log-implementation": "1.0"}` but zero class in the
closed-world registry implements `Psr\Log\LoggerInterface` - a false
claim any other package `require`-ing that virtual package would
silently trust, a real library-authoring bug (an implementation
refactored/renamed away without updating or removing a stale `provide`
entry copied from a template). Deliberately excludes
`psr/http-message-implementation`/`psr/http-factory-implementation`
(each maps to several interfaces at once - a class-per-interface
ambiguity needing a stricter all-N-satisfied rule to avoid false
positives, kept out of v1 rather than shipped speculative). A
case-insensitive grep for "provide"/"replace" as a composer-schema
concept across both vendored trees returns zero hits. phpstan-rs already
has every piece except the hardcoded PSR-virtual-package table:
`composer::detect_phpstan_dependency`'s existing `serde_json`-parsing
pattern extends directly to also read `provide`; `Registry::
is_subclass_of` needs no new type-system machinery; `decl_locations`
(gated behind `retain_source`, the same shared prerequisite R1/AC already
document needing open for batch runs) already maps every declared FQN to
its real location.

**Effort: medium.**

## AK. `ext.notRequired`: PHP-extension requirement verification

Distinct from K, R, AC, and AG - a completely different kind of
dependency (a compiled PHP extension the runtime must have loaded),
checked via a completely different data source (the phpstorm-stubs
per-extension directory layout, never touched by any of the other four).
For every call to a built-in function (or reference to a built-in class/
constant) that phpstan-rs's stub pipeline sourced from a specific
`vendor/phpstorm-stubs/<extension>/` directory (`curl_*`, `mb_*`, `gd_*`,
Redis, Imagick, etc.), resolve the project's own root `composer.json`
`require` section and flag when it declares neither `ext-<extension>` nor
a covering polyfill - the classic "works on the author's machine because
the extension happens to be installed" bug, dying with a raw PHP fatal
and zero warning from any tool today. Real PHPStan's `Rules/` catalog has
zero rule keyed on composer.json `require` vs. extension-function usage -
grepping both vendored trees for "ext-" as a rule concept returns
nothing; the one `ext-` string that does appear anywhere in PHPStan's own
conf is its *own* bundled stub-naming convention (`ext-ds.stub`), not a
rule checking an analyzed project's composer.json. The real ecosystem
tool for this, `composer-require-checker`, is (like K's own precedent) a
separate non-PHPStan tool entirely. phpstan-rs already parses
composer.json generically via `serde_json` (`detect_phpstan_dependency`)
- extending that same parse to also collect `ext-*`-prefixed keys is
direct, proven extension of existing code. The honest gap: the
per-function "which extension does this belong to" mapping does *not*
already exist anywhere in phpstan-rs today (no `extension` field on
`FunctionSig`/`ClassInfo`) - but the raw material sits one step upstream
of where it's currently thrown away: `build.rs`'s `collect_php_files`
already recursively walks `vendor/phpstorm-stubs/<extension>/*.php` and
holds each file's real, extension-named parent directory in scope at
exactly the point it decides what to write into the generated stub-source
array, then discards that path entirely, writing only raw file contents.
Threading the extension name through alongside each source string (or a
standalone generated table keyed by file) is additive to a pipeline whose
per-file path is already known and already discarded on purpose today. A
handful of always-compiled-in extensions (`Core`, `SPL`, `Reflection`,
etc.) must be excluded, since no `ext-core`-style Composer platform
package exists to require - the mapping only becomes actionable for
genuinely optional extensions.

**Effort: large.**

## AL. `composerScript.notFound`/`notPublicStatic`: composer.json `scripts` `Class::method` callable-string validation

Distinct from K, R, AC, AG, and AK (each keys off a different
composer.json section and a different failure mode - none mentions
"scripts" at all). Parse the project's root composer.json `"scripts"`
map (a direct extension of the same `serde_json::Value`-reading pattern
`detect_phpstan_dependency` already uses, just a different top-level
key) and, for every string value matching the documented `Class::method`
callable-string convention (a bare FQN, `::`, a bare identifier - no
`$`, `(`, spaces, or shell metacharacters, disambiguating it from a
plain shell-command string; the check is deliberately scoped to exactly
this unambiguous form, never the shell-command or array-of-commands
forms that carry no static contract), split and resolve against the
closed-world registry: report `composerScript.notFound` when the class
or method doesn't exist (mirroring the exact undefined-static-method-call
precedent already in the checker, `__callStatic` suppression included,
since Composer's own invocation is a plain `call_user_func` that magic
method would legitimately intercept), and `composerScript.notPublicStatic`
when the method resolves but isn't public (reusing `FunctionSig::
visibility`, already threaded through since Phase 57) or isn't static -
Composer instantiates nothing and calls the callable via reflection-free
`call_user_func`, so a private/protected or non-static target throws or
silently no-ops when Composer actually runs the script, with zero static
warning today. PHPStan's own `Rules/` catalog has no composer.json-
schema-reading code for the "scripts" section at all - a case-insensitive
grep across both vendored trees returns exactly one hit, an unrelated
code comment about CLI-invocation scripts in `Turbo/
TurboExtensionEnabler.php`, confirming zero rule ever reads this key.
phpstan-rs already has three of the four pieces this needs, each proven
at a real call site: generic composer.json section-reading via
`serde_json`; closed-world class-and-method existence plus visibility
resolution via `Registry::lookup_method`; and a direct precedent for
splitting a `Class::method`-shaped string and resolving it against the
registry, already built for `call_user_func('Class::method')`-equivalent
callables. The one honest gap: `FunctionSig` carries no `is_static` field
at all today (every `sig.is_static` reference found elsewhere in the
checker actually reads `PropertySig::is_static`, a different struct) -
but the raw `MethodDecl::is_static` bool needed to fill it is already
sitting unused in the exact iterator that discards it today (the same
loop that reads `m.visibility` into `FunctionSig::from_decl` also reads
`m.is_abstract` off the same `m` three lines later, just never
`m.is_static`) - threading it through mirrors the already-completed
Phase 57 visibility rollout exactly, not new machinery.

**Effort: medium.**

## AN. `composer.platformPhpInconsistentWithRequire`

composer.json's `config.platform.php` (a pinned PHP version string that
overrides Composer's own dependency-resolution assumption about the
runtime PHP version, distinct from the ordinary `require.php` semver
*constraint*) checked against that same project's own `require.php` for
internal consistency. When the pinned platform version doesn't satisfy
the require constraint (platform says 8.1.0 while require demands
`^8.3`, or vice versa), Composer resolves every dependency's version *as
if* running the platform-pinned PHP - potentially installing older,
less-capable package releases with different type signatures - while the
project's own `require.php` declares a different minimum the analyzed
code is written against: a real, silent self-contradiction in one config
file, invisible to any tool today. Real PHPStan reads `config.platform.php`
purely as a fallback *source* for its own analysis PHP version (used
only when no explicit `phpVersion` config key is set, in a priority
chain of explicit-config > `SOURCE_COMPOSER_PLATFORM_PHP` >
runtime-`PHP_VERSION_ID`) - it never cross-checks that value against the
project's own `require.php` constraint in the same file; grepping both
vendored trees for "platform"/"config.platform" as a `Rules/` concept
returns nothing beyond that one factory - confirming real PHPStan models
the field only as an input to itself, never as something to validate for
internal consistency against the rest of composer.json. Honest scope
note: phpstan-rs does *not* currently derive its own `php_version` from
composer.json at all in either direction (it's populated only from the
`--php-version` CLI flag or config.rs's TOML key, mirroring real
PHPStan's explicit-config path only, never its composer-platform
fallback) - so this needs two genuinely new pieces: parsing the
`config.platform.php` string (trivial, same `serde_json` pattern as
existing composer.json reads) and evaluating it against the `require.php`
semver-style constraint string, for which phpstan-rs has no existing
parser at all - a small hand-rolled evaluator covering Composer's common
constraint forms (`^X.Y`, `~X.Y`, `>=X.Y`, simple `||` OR-groups) is
needed, a bounded, well-known finite grammar for a single X.Y.Z PHP
version, not a general semver-range library.

**Effort: medium.**

## S. Composer path-repository / monorepo workspace support

### S1. Fix the symlink-escaping vendor-context filter, then auto-promote path-repository siblings to first-party analyzed files

Two parts. **Bug**: `eval_path` (`src/composer.rs:282-300`) resolves a
candidate vendor path via `std::fs::canonicalize(raw)` then tests
`resolved.starts_with(vendor_dir)` to decide whether it's genuinely
vendor-scoped. For a Composer path-repository dependency, `vendor/<pkg>`
is a real filesystem symlink to the sibling package's own source tree
outside `vendor/`; canonicalize fully resolves that symlink, so the
containment check always fails and the entire path-repo package is
silently dropped from `ComposerContext` (confirmed with a live repro: a
`vendor/acme/package-b` symlink to a sibling directory resolves to a path
that never starts with the canonicalized `vendor_dir`). The fix needs to
distinguish "legitimately in vendor/, but the directory happens to be a
symlink" from "the raw string genuinely traverses outside vendor/" -
checking containment on the un-resolved joined path's components, the way
real PHPStan's own `PathRoutingParser` walks each path component with
`is_link()` rather than canonicalize-then-prefix-test. **Feature**: once
symlinks are followed correctly, read `composer.json`'s `repositories`
array (today only `require`/`require-dev` are parsed,
`detect_phpstan_dependency`, `composer.rs:119-126`) to recognize which
`vendor/<pkg>` entries are local `type: path` siblings, and promote just
those files from `scan_dirs` (context-only, never reported) into
`analyze_files` (diagnostic-bearing) - one-pass, cross-package
diagnostics with zero manual config. Real PHPStan's vendor-derived class
info comes from a reflection-only `SourceLocator`
(`ComposerJsonAndInstalledJsonSourceLocatorMaker::create`,
`...SourceLocatorMaker.php:44-171`) never wired to the set of files a
`Rule` actually runs against (that's `paths:`, a separate, manually
authored config key) - a path-repo sibling's own bugs are invisible in
package A's run unless someone manually adds the vendor-symlinked
directory to `paths:` in every downstream consumer's own config, repeated
per consumer, with no cross-process dedup since there's no monorepo
concept anywhere in PHPStan (grepped, zero hits). phpstan-rs already has
the exact analyze/context split this needs, working in one process with
one shared `Registry` (`scan_dirs`/`analyze_files`,
`check_project.rs:101-145`) - only the symlink bug and the
`repositories`-array read are missing, both local, additive changes.

**Effort: large.**

### S2. `check-project`'s vendor-context discovery is hardcoded to a single root, losing every sibling package's own dependencies in a real workspace

`run_check_project_command` calls `composer::discover_vendor_context(&cwd)`
and `composer::detect_phpstan_dependency(&cwd)` exactly once, against one
directory (`check_project.rs:329,332`) - even though the underlying
`run_check_project` function already accepts an arbitrary list of `dirs`
to load into one shared `Project`/`Registry` and check together
(`check_project.rs:63-64,82-84`), a multi-package capability with no
real-PHPStan analogue at all. For a workspace of several independently-
`composer install`-ed packages, invoking `check-project package-a/src
package-b/src` today only discovers whichever ONE package's vendor/
happens to be the process's `cwd`; the other package's own third-party
dependencies (types it needs for correct inference) are silently missing.
Real PHPStan resolves exactly one composer.json/vendor root per process
(`ComposerHelper::getComposerConfig(string $root)`,
`Internal/ComposerHelper.php:37-53`, a single `$root` string threaded from
one CLI invocation's cwd) - a genuine multi-root workspace structurally
requires one full separate PHPStan process (own DI container, own
result-cache, own baseline) per package, zero shared type-resolution or
diagnostic reasoning between them. phpstan-rs's multi-root capability is
already ~90% built at the CLI-argument and Registry level - only the
vendor-context-discovery call is hardcoded to a single `cwd`. Fix: for
each entry in `dirs`, walk upward (reusing `find_composer_package_name`'s
existing upward walk as the pattern) to find that entry's own nearest
composer.json, discover its context once per root, and merge+dedup every
resulting `ComposerContext` before loading - one extra loop, not a
rebuilt container per root the way a faithful multi-invocation PHPStan
setup would need.

**Effort: medium.**

---

# Part 9: Type-system policy, baseline robustness, reverted-feature fixes, and API diffing

## M. Type-system representation lessons from real PHPStan's own accreted BC debt

Not performance tradeoffs (Part 1/2) or missing features - real PHPStan's
own source code admits, in its own comments, four places where an early
design decision now has to be permanently carried forward for users it
already shipped to. phpstan-rs has no installed userbase to protect and
can simply not repeat the mistake; each item below is a forward-looking
policy recommendation, not a bug to fix.

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

### M4. Never split builtin function signatures into a legacy-loose stub and an opt-in stricter delta

Real PHPStan ships two parallel builtin-function stub files for the same
functions: a loose/legacy one and a strictly more precise one (e.g.
`php_uname`'s `mode` parameter narrowed from bare `string` to a literal
union of its six valid values; `array_rand`'s `input` narrowed from
`array` to `non-empty-array` and `num_req` from `int` to `positive-int`;
six `DOMDocument` methods' `filename`/`source` parameters narrowed from
`string` to `non-empty-string`).
`FunctionSignatureMapProvider::getSignatureMap()` only merges in the
stricter file when `$this->stricterFunctionMap` is true, and that flag
(`featureToggles.stricterFunctionMap`) defaults to `false` in
`conf/config.neon:38`, flipped to `true` only by `conf/bleedingEdge.neon:7`
- functionally the same opt-in-for-BC gating as `BleedingEdgeToggle::
isBleedingEdge()` (M1's own mechanism), just expressed as a plain DI
parameter instead of the static call. It exists because narrowing a stub
an existing user's code already analyzes cleanly against could surface
brand-new errors for them - the stricter signatures are demonstrably
correct (verified in PHPStan's own bleeding-edge files), but PHPStan
can't ship them as the *only* signature for exactly that BC reason.
phpstan-rs models builtin signatures directly in Rust (e.g. `array_rand`
in `checker/expr/builtin_calls.rs:745`) with no stub-file/toggle
machinery at all - the forward policy is to keep it that way: always
encode the most precise known signature for a builtin (matching what
real PHPStan's own bleeding-edge file already proves correct), never add
a parallel looser variant gated behind a flag, since there is no
installed userbase whose already-passing analysis a narrower builtin
stub could suddenly break.

**Effort: small** (a standing policy, same shape as M1-M3).

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

## O. Reattempting reverted features with real PHPStan's actual technique

ROADMAP's "Reverted" section (grep for that heading) lists features
phpstan-rs once built, corpus-verified, then undid after real false
positives. Both items below found that the *revert* was correct given
what phpstan-rs had available at the time, but that real PHPStan solves
the identical case with a specific technique phpstan-rs's attempt never
used - not a coincidence PHPStan avoided the bug, a real mechanism to
port.

### O1. Foreach's zero-iteration merge: real PHPStan's asymmetric, certainty-downgrading join instead of a symmetric type-union

Real PHPStan does NOT give `foreach` the same zero-iteration merge
treatment as `while`/`for`. `ForeachHandler.php` computes
`$exprType->isIterableAtLeastOnce()` (`Type.php:174`, implemented per-type,
e.g. `ArrayType.php:233`) and branches three ways
(`ForeachHandler.php:374-390`): definitely->=1 iteration keeps the pure
post-body scope; definitely 0 iterations reverts to the pure pre-loop
scope; the "maybe empty" case calls
`MutatingScope::processAlwaysIterableForeachScopeWithoutPollute()`
(`MutatingScope.php:4122-4169`) - which is NOT a symmetric type-union
merge. For each variable, it keeps the post-loop *type* outright and only
downgrades the certainty (yes->maybe), never unioning the pre-loop type
back in for a variable that already existed before the loop - exactly the
case a real idiom needs: a foreach body that mutates its own iteratee
array through the loop variable
(`foreach ($items as $day => $data) { $items[$day]['x'] = ...; }`).
phpstan-rs's only branch-join primitive, `merge_scopes`
(`src/checker/control_flow.rs:403-440`), unconditionally computes
`union_types(then_ty, else_ty)` for every shared variable - purely
symmetric, no certainty concept. ROADMAP's Reverted entry (~lines
1044-1051) and `checker/stmt/loops.rs:584-615`'s own inline comment
confirm phpstan-rs's original and re-attempted foreach zero-iteration
merges both reused this same symmetric primitive and both regressed on
exactly that self-mutating-iteratee idiom (named by the real corpus
function that surfaced it, `CalendarMonthly::buildMonthCalendar`) -
unioning the pre-loop array shape back in re-introduces a stale element
type every real iteration actually overwrites. Confirming this is
foreach-specific in PHPStan, not a general technique: `while`/`for`'s own
zero-iteration handlers use plain symmetric `mergeWith()`, the same tool
phpstan-rs's existing (non-reverted) while/for merge already uses
successfully - foreach just borrowed the wrong one. phpstan-rs already
has the prerequisite Phase 55 fixed-point loop convergence built
(`converge_loop_body`); the missing piece is narrowly a second, asymmetric
join primitive mirroring `processAlwaysIterableForeachScopeWithoutPollute`,
gated on the same "runs >=1 time" provability. Going beyond real PHPStan:
it carries a legacy `polluteScopeWithAlwaysIterableForeach` config flag
(`NodeScopeResolver.php:184,807`) forcing it to maintain BOTH the
symmetric and asymmetric code paths for third-party BC; phpstan-rs has no
such burden and could adopt the more-precise asymmetric merge
unconditionally, everywhere a loop join happens - one code path instead
of PHPStan's two.

**Effort: medium.**

### O2. Register each anonymous class under a real, stable synthetic name, instead of the discarded ancestor-aliasing shortcut

Real PHPStan never infers an anonymous class's type as an alias of its
extended/implemented type. `AnonymousClassNameHelper::
getAnonymousClassName()` (`Broker/AnonymousClassNameHelper.php:30-55`)
gives every anonymous class a real, deterministic synthetic name -
`AnonymousClass<md5(file:start-line[:line-index])>`, where `line-index`
(`AnonymousClassVisitor::ATTRIBUTE_LINE_INDEX`,
`Parser/AnonymousClassVisitor.php:16-56`) disambiguates multiple
anonymous classes starting on the same line - then registers a full,
independent `ClassReflection` under it: own methods, own extra members,
its own `extends`/`implements` as real supertypes. `instanceof`, member
lookups, and inferred-type checks against it work exactly like a named
class, with no risk of conflating its own extra members with whatever it
happens to implement. phpstan-rs currently gives every anonymous class
the identical literal placeholder name `"class@anonymous"`
(`src/parser/classes.rs:93-96`, the parser's own comment noting multiple
anonymous classes in one file "could not share this name safely - never
indexed into the registry by this name"), and `checker/expr.rs:1816-1861`'s
`NewAnonymousClass` handling confirms the consequence in its own Phase 90
comment: an anonymous class's body is only checked ad hoc against one
directly-named ancestor method signature as a narrow fallback, with no
real registry entry and no full member-set checking. ROADMAP's own
Reverted entry (~lines 1058-1066) for the prior ancestor-aliasing attempt
independently arrives at the identical conclusion real PHPStan already
implements: "a real fix needs registering the anonymous class's own full
declaration under some stable synthetic name, not a type-inference
tweak." Because phpstan-rs's registry is a single whole-program pass over
every parsed file (no separate BetterReflection SourceLocator/dynamic-name
layer to thread a synthetic name through), assigning a
`file:line[:index]`-keyed name mirroring `AnonymousClassNameHelper`'s own
scheme and indexing the anonymous `ClassDecl` the way a named class is
indexed is structurally smaller here than in real PHPStan - and unlocks
full type-checking against every implemented interface/extended parent's
declared members at once, a strictly larger win than PHPStan's own
per-name lookup.

**Effort: medium.**

---

## Q. Whole-program public-API backward-compatibility diffing

A new capability with no PHPStan-core or `phpstan-strict-rules`
equivalent at all - grepping both vendored trees for
BackwardCompat/BCBreak/ApiDiff turns up nothing except PHPStan's own
inward-facing `Rules/Api/*` family (flags analyzed code referencing
PHPStan's *own* internal namespace, an unrelated concept). `analyseFile`
is PHPStan's sole entry point: always one fresh CLI invocation over one
filesystem snapshot, no notion of loading and comparing two points in
time. `Project::new()`/`load_file` take no arguments blocking two
independent, simultaneously-live `Project`/`Registry` snapshots in one
process - the diff is a plain field-by-field walk over two frozen
`HashMap<String, ClassInfo>`s.

### Q1. `phpstan-rs api-diff <old-ref> <new-ref>`: structural public-API diff between two Registry snapshots

Loads two independent `Project`/`Registry` instances (one from a git-ref
checkout or old vendored copy, one from the current tree) and structurally
diffs every public (non-`@internal`) symbol's `ClassInfo`/`FunctionSig`/
`PropertySig`/`ConstSig`: a removed public method/property/constant, a
parameter type narrowed (via `is_subtype`) so a previously-legal caller is
now rejected, a return type widened so a previously-guaranteed narrower
type can't be assumed, a visibility narrowed or a symbol newly marked
`@internal`, a method/class newly `final`, a removed interface/parent, or
a non-final class gaining a new abstract method breaking existing
subclasses. phpstan-rs already builds every signature-shape field this
needs as ordinary struct fields, not reconstructed via reflection each
run: `ParamSig`/`FunctionSig` (`registry/function_sig.rs:95-176`,
including visibility, `is_internal`, deprecated, native vs docblock
types) and `ClassInfo`/`PropertySig`/`ConstSig`
(`registry/class_info.rs:23-205`, including `is_abstract`/`is_final`/
`extends`/`implements`/`final_methods`/`final_properties`). `Registry` is
frozen once per `finish_loading()` and is a plain in-memory `HashMap`, so
two independently-loaded, independently-frozen registries can coexist in
one process with no shared-mutable-state hazard - real PHPStan's per-run,
discard-after-use DI container has no analogous ability to keep two full
snapshots alive and comparable at once. Distinct from G2 in Part 7
(signature fingerprinting): G2 compares shapes within one live incremental
session on the *same* tree to drive re-check scheduling; this compares
two independently-loaded, disjoint snapshots to report public-surface
breaks to a human/CI as the actual product.

**Effort: large.**

### Q2. PHPDoc-level generic/template-bound narrowing as its own break category, invisible to native-signature-only diffing

Extends Q1 with a break category specific to phpstan-rs's PHPDoc-aware
generics: a class's `@template T of Bound` tightened between two
snapshots (`@template T of object` narrowed to `@template T of
Countable`), or an `@extends`/`@implements` type-argument binding
changed, silently rejects type arguments a caller's existing code still
passes - even though the native PHP declaration (and therefore the
byte-identical Reflection-visible signature) never changed. This break
category has no PHP-native-signature representation whatsoever: a
`@template`/`@extends`/`@implements` bound lives purely in a docblock
string, invisible to any BC tool working from the generic PHP Reflection
surface alone (the same surface real PHPStan's own runtime-reflection
rules are limited to, per ROADMAP's Won't-port note on
`RuntimeReflectionFunctionRule`/`RuntimeReflectionInstantiationRule`) - two
classes whose native declarations are byte-for-byte identical would
compare as "no change" to such a tool. phpstan-rs's registry already
carries generic bounds as first-class, already-resolved struct fields, not
a string to re-parse: `ClassInfo::templates`, `ClassInfo::
template_bounds: HashMap<String, Type>` (Phase 10.5's already-enforced
`@template T of Bound`), and `ClassInfo::extends_args`/`implements_args:
HashMap<String, Vec<Type>>` (`registry/class_info.rs:178-199`) - all
already on the exact same `ClassInfo` Q1 already walks, so comparing them
across snapshots (checking narrowing direction via `is_subtype`, not just
inequality) is additive to Q1's own walk, not new architecture.

**Effort: medium.**
