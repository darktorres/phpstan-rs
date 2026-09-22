# Beyond parity

ROADMAP.md tracks the path to feature *parity* with real PHPStan. This
document is the opposite direction: concrete, source-grounded opportunities
for phpstan-rs to do things real PHPStan doesn't do, or can't do as well,
once parity work uncovered a real architectural reason why not. Every item
below cites the actual PHPStan source it's grounded in
(`vendor/phpstan-src/src`, `vendor/phpstan-strict-rules/src`) and the actual
phpstan-rs source (`src/`) that would implement it - every citation has been
checked directly against the vendored/working trees, not taken on an
agent's word. 76 findings remain open as of this writing (a 3rd of the
original 116 have since shipped - each closed item now just points at
its own ROADMAP Done entry rather than staying duplicated here),
grouped below by what they're actually about rather than by when they
were found. Nothing here is scheduled; it's a grounded idea backlog,
same status as ROADMAP's own "Newly identified gaps" until triaged into
a numbered phase. Part 10 draws on a second
reference project, Psalm - not `vendor/psalm` (the pinned submodule used
elsewhere in this project purely as a corpus-validation target), but a
separate, ad hoc `git clone https://github.com/vimeo/psalm.git` one
directory up from this repo (`../psalm`), checked out for this research
pass only. Every Part 10 citation was checked directly against that
clone the same way every other citation here was checked against the
vendored trees.

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

Ten parts:

1. Precision, whole-program analysis, and closed-world inference
2. Native-performance wins real PHPStan needs a separate extension for
3. New rule categories and design-pattern suggestions
4. Ecosystem: `phpstan-strict-rules` ports and `Won't-port` corrections
5. LSP / editor-native capabilities
6. Codemods
7. Config/strictness composability
8. Composer/dependency hygiene
9. Type-system policy, baseline robustness, reverted-feature fixes, API diffing
10. Findings grounded in Psalm's production implementation

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
cannot. (The same reverse index is reused directly by P1 and U1 below -
and was exactly the mechanism the now-shipped dead-abstract-class check,
ROADMAP's Phase 93, ended up building.)

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

## H. Whole-program value-flow diagnostics

Whole-program unused-member detection, dead-function/dead-class detection,
namespace-cycle detection, dead-enum-case detection, and dead-abstract-
class detection (all originally proposed here) are now shipped - see
ROADMAP's Done section, Phase 86's B1/B2/B3 and Phases 92/93, and their
own `circular_namespace_deps.rs`/`dead_code.rs`/`dead_members.rs`/
`dead_enum_cases.rs`/`dead_abstract_classes.rs` modules.

Distinct from the now-shipped whole-program dead-code family noted above,
which reuses the `dependents` graph for *existence*/dead-code questions:
these reuse the same whole-project registry-fixed-point architecture
(Phase 41's purity model) for questions about the *values* flowing
through the program instead.

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

Psalm ships the whole-program half of this in production too
(`UnusedReturnValue`/`PossiblyUnusedReturnValue`, `ClassLikes.php:1922-1953`)
and its implementation has two refinements worth folding into this
item when it's built: it exempts "probably fluent" methods (a
`return $this;` builder-chain method) from the check entirely
(`!$method_storage->probably_fluent`, `ClassLikes.php:1928`), and it
splits severity by visibility the same way phpstan-rs's own now-shipped
whole-application unused-member family (ROADMAP's Phase 86 B1/B2) does -
`private` (the whole call-site universe is visible, so "unused" is
definite) versus broader visibility (softer "possibly unused," since an
external/reflection-based caller could exist).

**Effort: medium.**

### H3. Whole-program property-type narrowing from every assignment site

A project-wide pass walks every `$this->prop = rhs` assignment across
every file that declares or extends a given class (via the registry's
subclass-aware hierarchy, the same walk ROADMAP's now-shipped Phase 86 B1
already runs for member *usage*), recording the RHS type `checker/expr.rs`'s
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
index (the same additive move A5/P1 already make, and the now-shipped
Phase 86 B1 already made, from structures
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

Extends D3 (duplicate branch bodies within one function, Part 3 below)
and the now-shipped D7 (structurally-identical operands within one
expression, ROADMAP's Done section - "duplicate-operand detection") to
duplication *across* declarations project-wide - a different AST scope
neither one touches.

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
unknowable. Distinct consumer from A5 (narrowing), the now-shipped Phase
86 B1 (unused-member detection), and H2 (NoDiscard inference) - this one
drives the purity
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

C2 (parallelizing `check_project` across native threads sharing one
`Registry`) and C4 (a work-stealing pool over a static size-sorted
partition for that same loop) are both now shipped - see ROADMAP's Done
section, "shared rayon thread pool for `check-project`," and commit
`020db7b`'s own correction settling on work-stealing over the
size-sorted-partition approach this part originally proposed porting
from real PHPStan's `Scheduler`.

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

D2 (boolean-trap / ambiguous positional-literal-argument detection) is
now shipped - see ROADMAP's Done section, "boolean-trap detection."

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

D5 (`@see` PHPDoc tag reference validation), D6 (swapped-adjacent-
argument-name detection), and D7 (structurally-identical operands on
both sides of a comparison/subtraction/division/self-assignment) are all
now shipped - see ROADMAP's Done section, "@see tag target validation,"
"opt-in swapped-adjacent-argument-name detection," and "duplicate-operand
detection."

D8 (a computed edit-distance "did you mean?" tip for `method.notFound`)
is now shipped - see ROADMAP's Done section, "method.notFound 'did you
mean' tip."

---

## AA. Whole-program design-pattern suggestion rules

Opt-in suggestions in the shape ROADMAP's now-shipped Phase 86 B1/B2 and
this document's own H-section items already establish: a
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
own cross-process collector reconciliation (the same cost ROADMAP's
now-shipped Phase 86 B1 already had to pay around) - the
same cost applies identically to aggregating every `new ClassName(...)`
call site's enclosing method across the whole project. phpstan-rs
already does the two things this needs simultaneously during ordinary
checking: `ExprKind::New`'s resolution of every constructor call's target
class (`checker/expr.rs:1697`) and `Checker`'s live tracking of the
enclosing method (`checker.rs:510`) - a project-wide pass only needs to
record `(target_class, current_class, current_method)` triples during the
existing walk.

**Effort: medium.**

AA3 (constructor-property-promotion suggestion for a trivially-forwarding
constructor) is now shipped - see ROADMAP's Done section, "opt-in
constructor-property-promotion suggestion."

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

AA5 (suggesting `never` when a body provably never returns normally) is
now shipped - see ROADMAP's Done section, "opt-in never-return-type
suggestion."

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

## AB1. Purity/impure-point enforcement for property hooks

(AB2, dead-write detection for `public private(set)` properties, is now
shipped - see ROADMAP's Done section, "a PHP 8.4 asymmetric-visibility
property's own narrower dead-write check.")

Real PHPStan's newest feature support (asymmetric visibility, property
hooks) is exactly where self-inconsistencies would be expected, unlike
its decade-old core mechanisms.

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

---

Section AE (assertion-less/"risky" PHPUnit-and-Pest test method
detection) is now shipped - see ROADMAP's Done section, "opt-in
assertion-less-test (risky test) detection."

---

## AF. Opt-in algorithmic-complexity heuristic: linear lookup into a pre-built array from inside an unrelated loop

Explicitly heuristic, the same acknowledged low-confidence framing D6
and D7 established before both shipped (see ROADMAP's Done section).
Inside a loop body (`self.
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
makes this a non-issue the check can't resolve. PHPStan's entire
`Rules/` catalog reasons about types, existence, and soundness, never
runtime execution cost - grepping both vendored trees for "complexity"/
"quadratic"/"performance" as a rule concept returns nothing; there is no
rule keyed on a lookup's placement relative to a loop, or on an array's
role as a repeatedly-scanned lookup table. Distinct from D1 (which
reasons about a function's own control-flow shape, never a variable's
role as a collection) - keyed on a loop's own iteration relationship to
a separate collection's construction site.

**Effort: medium.**

---

# Part 4: Ecosystem - `phpstan-strict-rules` ports and `Won't-port` corrections

Grounded directly against the vendored companion package
`vendor/phpstan-strict-rules/src` (a git submodule, matching the existing
`vendor/phpstan-src` precedent in `.gitmodules`) - every citation below
resolves to real, on-disk source, not an assumed API.

## E. Ecosystem checks ported from `phpstan-strict-rules`

E1 (non-boolean value used directly as a condition) is now shipped - see
ROADMAP's Done section, "opt-in 'only booleans in condition' family."

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

E3 (a concrete constructor that never calls `parent::__construct()`) and
E4 (an inherited method whose declared name-casing mismatches its
parent/interface) are both now shipped - see ROADMAP's Done section,
"opt-in constructor.missingParentCall" and the override-compatibility
family's "an overriding method's own casing mismatch."

---

## W. More `phpstan-strict-rules` "stricter defaults" config flags

A distinct axis from section E's custom Rule classes: a gap where a
`phpstan-strict-rules` config flag's stricter behavior has no phpstan-rs
equivalent at all, opt-in or otherwise - confirmed against the real
gating mechanism, not assumed. (Two sibling gaps in this same axis, W1
and W3, have since shipped - see ROADMAP's Done section.)

W1 (opt-in strict `checkDynamicProperties` behavior inside
`isset()`/`empty()`/`??`) is now shipped - see ROADMAP's Done section,
"opt-in strict checkDynamicProperties in isset/empty."

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

W3 (opt-in call-site function/method name-case checking) and Z1
(standalone numeric-string/float/bool array-key cast warnings,
`Won't-port`-mis-bucketed since both real-PHPStan rules live under
`Rules/Playground/`, the same mis-bucketing the now-shipped AA5 shared)
are both now shipped - see ROADMAP's Done section, "opt-in call-site
function/method-name-case checking" and "opt-in array-key cast
warnings."

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
realtime incremental analysis (the LSP server's own trustworthiness/
concurrency/protocol gaps, and the VSCode extension client itself, were
a separate, now-fully-done initiative - see ROADMAP's "Performance &
tooling" Done section). Most of what this part originally proposed has
since shipped too: live flow-narrowed hover, `codeAction`/`DiagnosticFix`
wiring, whole-project `references`/`rename`, `semanticTokens`,
`inlayHint`, `documentSymbol`, `workspace/symbol`, `signatureHelp`,
`typeDefinition`, and `documentHighlight` are all in ROADMAP's Done
section now (its "LSP hover/go-to-definition" and "LSP completeness"
entries) - see that section for what each one actually covers. What's
left here is real, narrower residual work: finer-grained incremental
re-check (G), two still-open single-file/whole-project stragglers
(`codeLens`, `willRenameFiles`), and derivation-trace explainability (T).

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

Section Y (`semanticTokens`/`inlayHint` passive rendering), AH1
(`documentSymbol`), and AH2 (`workspace/symbol`) are all now shipped -
see ROADMAP's Done section, "LSP completeness." What remains from the
original AH section:

## AH3. `textDocument/codeLens` reference-count annotations above declarations

Advertise `code_lens_provider` and add a `"textDocument/codeLens"` arm to
`handle_request`. For each declaration in the requested file (walked the
same way the now-shipped `documentSymbol` walks `Project::ast(path)`),
emit one `CodeLens` whose title is a use-site count (e.g. "3 references")
sourced by aggregating the two-tier use-site index the now-shipped
`references`/`rename` support already built (ROADMAP's Done section) -
grouping its per-symbol spans by declaration and taking the count. This
was originally proposed as depending on a not-yet-built index; that index
now exists, so this item is strictly smaller than it was. Two pieces are
still new work: per-declaration count aggregation (the existing index is
built for enumeration, not as a rollup count), and the `CodeLens`
response serialization/wiring itself. Real PHPStan ships no LSP or
resident analysis server of any kind - `FileAnalyser::analyseFile` is its
sole entry point, always a fresh whole-file CLI invocation with no
persisted cross-file use-site index and no channel to push
editor-rendered annotations without modifying the analyzed source.

**Effort: medium.**

---

Section AM (`signatureHelp` and `typeDefinition`) is now shipped in
full - see ROADMAP's Done section, "LSP completeness."

---

## AO1. `workspace/willRenameFiles`: PSR-4-consistent rename-file refactoring across the whole project

(AO2, `textDocument/documentHighlight`, is now shipped - see ROADMAP's
Done section, "LSP completeness.")

Distinct from the now-shipped `textDocument/rename`: this is triggered by
a file-explorer drag/move with no cursor position at all, and its core
computation (deriving a new expected FQN from a new file path) is the
*reverse* direction of what `rename` does - `rename` only ever resolves
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
depending on two things: R1's PSR-4 prefix-map access (not yet built) and
the use-site span index the now-shipped `rename` support already built.

**Effort: large.**

---

## T. Diagnostic explainability: derivation traces, not just a final type

Real PHPStan's only type-introspection tool, `PHPStan\dumpType()`,
computes and prints one final type per call site via a plain `sprintf`
(`Rules/Debug/DumpTypeRule.php:32-50) with no accumulated reasoning kept -
each `Scope::getType()` call that fed it is discarded the instant the
message string is built, and it requires editing the analyzed source and
re-running the whole CLI, since PHPStan ships no LSP or resident process
to query interactively at all. Distinct from the now-shipped live hover
(a single inferred `Type` wired into hover for one span, computed fresh
each probe): both items below trace the *chain* of operations that
produced a type, an entirely different data shape.

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

Four concrete fixes, each attaching to an existing diagnostic via the
already-shipped `DiagnosticFix`/`codeAction` plumbing (ROADMAP's Done
section) - only the per-check `.fix()` population below remains open.
Real PHPStan's entire autofix surface (`RuleErrorBuilder::
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
Scales directly to the now-shipped `method.nameCase` inherited-casing
check with the same mechanism. Real PHPStan's nameCase rule family has zero
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

### I4. Conditional single-line insertion codemod for the now-shipped `constructor.missingParentCall` check

The fix is inserting `parent::__construct();` as the constructor body's
first statement - but only when the resolved parent constructor's
`FunctionSig.params` has zero required entries; otherwise the correct
call needs argument values the checker can't invent, and the codemod
must decline rather than guess. `phpstan-strict-rules` has zero
`fixNode` usage anywhere (`grep -rl fixNode vendor/phpstan-strict-rules/src/`
- empty), confirming this fix was never attempted upstream despite being
as mechanically simple as I2's.

**Effort: small.**

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
the existing `class.nameCase`/`method.nameCase` checks, which compare an
in-source *reference*'s casing against its own declaration's casing
within PHP text
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

O2 (registering each anonymous class under a real, stable synthetic name
mirroring real PHPStan's own `AnonymousClassNameHelper`, instead of the
discarded ancestor-aliasing shortcut) is now shipped - see ROADMAP's Done
section, "Anonymous class stable synthetic naming."

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

---

# Part 10: Findings grounded in Psalm's production implementation

Every part above is grounded in what real PHPStan does or doesn't do.
This part instead cross-checks against Psalm (`vimeo/psalm`), the other
major PHP static analyzer, doing the same kind of comparison Part 4
already does against `phpstan-strict-rules`, but against a whole
separate codebase's own diagnostic catalog: all 325 distinct issue
classes under `src/Psalm/Issue/` in the ad hoc clone described above,
read exhaustively (every file, not a sample chosen by promising-sounding
name) and cross-checked against both `vendor/phpstan-src` (to rule out
that real PHPStan already covers the same ground) and phpstan-rs's own
`src/` (to rule out that this project already covers it a different
way, often under different diagnostic-identifier naming than Psalm
uses). Most of the 325 turned out to be one of three things: ordinary
type-checking phpstan-rs already has (confirmed via phpstan-rs's own
`"identifier.string"` literal census cross-checked against each Issue
class's actual Psalm emission site, not assumed from the name), real
PHPStan parity gaps with no Psalm-specific angle at all (folded into
ROADMAP.md's own "Newly identified gaps" instead - see the new
`MatchExpressionRule.php` entry under its "Comparison" section, this
research pass's one addition there), or Psalm-internal implementation
detail with no general applicability (see part AR below). What survived
all three filters is below.

## L. Concrete new checks with no PHPStan or phpstan-rs precedent

### L1. `ClassMustBeFinal` - suggest `final` on a class that's never subclassed and isn't public API

Psalm ships a whole-program design-pattern suggestion phpstan-rs has no
equivalent of at all: a class gets flagged for `final` when it's
non-abstract, non-enum, non-interface, not already `final`, isn't part
of the codebase's declared public API, and - checked against the whole
analyzed project - is never subclassed anywhere
(`consolidateAnalyzedData`, `Internal/Codebase/ClassLikes.php:864-897`;
`Issue/ClassMustBeFinal.php`). This is a genuinely new rule *category*
for this project (Part 3's "new rule categories" theme), not a variant
of an existing check - nothing in `src/checker/` currently reasons about
whether a class *should* be sealed based on observed whole-program
subclassing, only about enforcing an already-declared `final`/
`@phpstan-sealed`. phpstan-rs's registry already has everything this
needs in the closed-world, single-`Registry` shape Part 1's own thesis
argues for repeatedly: `all_ancestor_names`
(`registry/hierarchy.rs:105`) gives the parent-direction walk, and A5's
proposed reverse (parent-to-children) index - already called for by A5,
P1, and U1 elsewhere in this document, and the exact mechanism the
now-shipped Phase 93 dead-abstract-class check ended up building - is
exactly the "is this
class ever extended" query this item also needs. A `public_api`-
equivalent gate is already close at hand too (`is_internal`/`@api`
tracking this project already carries per `FunctionSig`/`ClassInfo`).

**Effort: medium** (mostly the reverse-index dependency shared with A5).

### L2. `MissingOverrideAttribute` - suggest adding `#[\Override]` where a method silently overrides one

phpstan-rs already validates the *presence* of a `#[\Override]`
attribute is correct - `own_sig.has_override_attr && !has_real_ancestor`
flags an attribute on a method that doesn't actually override anything
(`checker/oop/method_overrides.rs:273-282`). Psalm checks the reverse
direction phpstan-rs has no equivalent of: a method that genuinely
overrides a parent/interface method but is *missing* `#[\Override]`
(`FunctionLikeAnalyzer.php:2029-2056`), exempting `__construct`,
trait-defined methods, and `__toString` unless the class implements
`Stringable`. This is nearly free to add: the exact `has_real_ancestor`/
`ancestor` lookup `method_overrides.rs:273` already computes at this
call site is everything the reverse check needs too - it's an `if
!own_sig.has_override_attr && has_real_ancestor` sitting right next to
the check that's already there, with the same exemption list
`method_overrides.rs`'s own `__construct`-special-casing (line 365-369)
already establishes for the neighboring compatibility check.

**Effort: small.**

### L3. `DirectConstructorCall` - flag `$obj->__construct()` called via ordinary method-call syntax

Legal PHP, but virtually always a bug: `$var->__construct(...)` (as
opposed to the legitimate `parent::__construct()`/`self::__construct()`
chaining syntax) re-runs the constructor on an already-existing object
instead of creating a new one via `new`
(`MethodCallAnalyzer.php:102-106`). Confirmed absent from real PHPStan
(`grep -rli "direct.*constructor" vendor/phpstan-src/src` - nothing) and
from phpstan-rs, which only checks the legitimate chaining direction
(`constructor.missingParentCall`, `checker/oop.rs:989-1034` - a
constructor that *forgets* to call `parent::__construct()`). The two
checks are complementary, not overlapping: this item is about the
*wrong syntax* being used to invoke a constructor at all, not about
whether a legitimate `parent::`/`self::` call happened.

**Effort: small** - a syntactic check on `ExprKind::MethodCall` where
the resolved member name is `__construct` and the receiver isn't
`parent`/`self`/`static`.

### L4. `ConstructorSignatureMismatch` - a subclass constructor requiring *more* parameters than its parent's

Real PHP (and PHPStan, and phpstan-rs - confirmed at
`checker/oop/method_overrides.rs:365-369`) deliberately exempts
`__construct` from the general override signature-compatibility check
entirely, since a subclass is legitimately allowed a totally different
constructor shape from its parent's. Psalm still flags one narrow slice
of that exempted space: a subclass constructor that requires *more*
required parameters than the parent's (`MethodComparator.php:210-225`) -
a latent `ArgumentCountError` the moment that subclass is instantiated
polymorphically (a factory, a DI container, `new $class(...)` via
`class-string<Base>`) by code that only knows the parent's shape.
Neither PHPStan nor phpstan-rs checks this at all today. Cheap to add:
`method_overrides.rs`'s existing `compatibility_ancestor`/`ancestor`
resolution for `__construct` (line 378) already has the parent
constructor's signature in hand right where the general exemption is
applied - this is a narrow carve-out of that exemption, not new
resolution machinery.

**Effort: small.**

### L5. `StringIncrement` - flag `$str++`/`++$str` on a non-numeric string

A real, common PHP gotcha with zero coverage on either side: PHP's `++`
on a non-numeric string performs Perl-style alphabetic increment
(`'a'` becomes `'b'`, `'z'` becomes `'aa'`, `'Az'` becomes `'Ba'`) rather
than any kind of arithmetic, which essentially never matches what a
developer intended when reaching for `++`
(`ArithmeticOpAnalyzer.php:395-416` - `$has_string_increment` set
specifically when the left operand is a `TString` that isn't
`TNumericString`/a numeric `TLiteralString`, and the parent node is a
pre/post-increment). Confirmed absent from real PHPStan
(`grep -rli "string.*increment" vendor/phpstan-src/src` - nothing) and
from phpstan-rs.

**Effort: small** - a type check on the operand of `ExprKind::PreInc`/
`PostInc` (or the by-ref-assignment-inc idiom) whose inferred type is
`Type::String` without the numeric-string accessory.

### L6. `RiskyTruthyFalsyComparison` - flag a boolean-context check whose union type mixes "always truthy/falsy" and "data-dependent" members

Flags `if ($x)`-shaped boolean-context checks where `$x`'s inferred
union type mixes members whose truthiness is a foregone conclusion
(`true`, a non-empty-string literal) with members whose truthiness is
genuinely data-dependent (plain `int`, plain `string`) -
(`Internal/Analyzer/Statements/ExpressionAnalyzer.php:148-172`, message:
"...contains type(s) ..., which can be falsy and truthy... Use strict
comparison instead"). The underlying gotcha is real: PHP's falsy set
(`0`, `'0'`, `''`, `[]`, `null`, `false`) spans multiple otherwise
unrelated types, so an implicit truthy check on a mixed-truthiness union
can silently misbehave for members the author wasn't thinking about.
Confirmed absent from real PHPStan
(`grep -rli "risky.*truthy\|truthy.*falsy" vendor/phpstan-src/src` - only
unrelated infra hits) and from phpstan-rs, whose `narrowing.rs` already
computes exactly which atomic types are always-truthy/always-falsy
internally (for branch narrowing) but never surfaces "this comparison
mixes both kinds" as its own diagnostic.

**Effort: medium** - reuses `narrowing.rs`'s existing truthy/falsy
classification per union member; the new part is applying it as a
standalone diagnostic at condition sites rather than only for narrowing.

### L7. `UnnecessaryVarAnnotation` - flag an inline `@var` that adds no information over plain inference

phpstan-rs's `check_inline_var_annotation_type`
(`checker/stmt.rs:978`) already mirrors real PHPStan's
`VarTagTypeRuleHelper::checkExprType`
(`vendor/phpstan-src/src/Rules/PhpDoc/VarTagTypeRuleHelper.php:100-140`)
- read directly, that helper only ever checks *compatibility* (is the
`@var` type a valid super/subtype of the inferred type -
`varTag.nativeType`/`varTag.type`), never *redundancy*. Psalm adds the
redundancy case on top: when an inline `@var`'s type is identical to
what inference already produces without it (and isn't narrowing away
from `mixed`), it's dead weight worth flagging for removal
(`ForeachAnalyzer.php:190-205`, `AssignmentAnalyzer.php:233-239`).
Cheap: `check_inline_var_annotation_type` already has both the parsed
docblock type and the natively-inferred type sitting side by side at
the exact call site this needs.

**Effort: small.**

### L8. `RedundantCast` / `RedundantFunctionCall` / `RedundantIdentityWithTrue` - semantic no-ops that aren't whole-statement no-ops

Three related, lower-value-but-real checks bundled together: `(string)
$x` where `$x` is already `string` (`CastAnalyzer.php:955`); a call like
`array_values()` whose transformation is a no-op given the argument's
already-known type (`NamedFunctionCallHandler.php:564-574`); and `$x
=== true` where `$x` is already `bool`, where the comparison itself is
redundant (`AssertionFinder.php:2204-2214`). All three are distinct from
phpstan-rs's existing `checker/noop.rs`, which (mirroring real
PHPStan's own `NoopRule`) only flags a bare *statement* with zero
effect - these fire even when the cast/call/comparison's result is
actually used in a larger expression. Cosmetic/code-quality tier rather
than bug-catching, so lower priority than L1-L7 above, but genuinely
absent from both real PHPStan and phpstan-rs.

**Effort: small** each, once the "what does this type's cast/call/
comparison actually change" comparison exists as a shared helper.

### L9. `InvalidTraversableImplementation` / `InvalidInterfaceImplementation` - implementing a marker interface directly instead of through its required base

One real PHP rule, two Psalm issue classes for its two built-in
instances. `class Foo implements Traversable` directly (rather than via
`IteratorAggregate`/`Iterator`) is a genuine PHP compile-time fatal
error, with a short, fixed exception list of SPL/extension classes PHP
itself special-cases (`PDOStatement`, `Ds\Collection`, `DOMNodeList`,
`DatePeriod`) - `ClassAnalyzer.php:2130-2144`, condition: `$fq_interface_
name_lc === 'traversable' && !$storage->abstract && !isset($storage->
class_implements['iteratoraggregate'|'iterator']) && ` (four more
`parent_classes` exceptions). The identical rule applies to `Throwable`
(must extend `Exception`/`Error`, not implement `Throwable` directly) -
same file, `ClassAnalyzer.php:2148-2156`, same condition shape. Confirmed
absent from both real PHPStan (`grep -rli "cannot implement.*Traversable\|
ImplementsTraversable\|toStringReturnType" vendor/phpstan-src/src` -
nothing for either) and phpstan-rs (`grep -rln "Traversable" src` turns
up only `foreach`-subject type-resolution comments, never a
class-*declaration*-time check).

**Effort: small** - a declaration-time check on `implements`, reusing
whatever ancestor-interface-name resolution `oop.rs`'s existing interface
checks already have in hand.

### L10. `InvalidToString` - `__toString()`'s inferred return type must be `string`, even with no declared return type

PHP enforces `__toString()` returning `string` as a runtime `TypeError`
regardless of whether the method has an explicit native return type -
extremely common in code written before PHP made this checkable at all.
Psalm validates the *inferred* return type against `string`
(`ReturnTypeAnalyzer.php:311-328`), which is what makes this distinct
from ordinary declared-vs-actual return-type checking: when there's no
declared return type, phpstan-rs's general return-type check has
nothing to compare against and stays silent. Confirmed absent from real
PHPStan (`find vendor/phpstan-src/src -iname '*ToString*'` turns up
`ImplicitToStringCallHelper` and stub-related type machinery, never a
`__toString`-return-type rule) and from phpstan-rs (no `__toString`
return-type validation anywhere in `src/checker/`).

**Effort: small** - a magic-method-name special case in the same place
`function_decl.rs`/`oop.rs` already validate a method's returns against
its type, requiring `string` regardless of whether one was declared.

### L11. `AbstractMethodCall` - calling an abstract static method directly

`Foo::abstractStaticMethod()` (a real PHP fatal error - there's no
implementation to run) is confirmed absent from both real PHPStan
(`grep -rli "abstract.*method.*cannot.*call\|CallToStaticMethodOnAbstract"
vendor/phpstan-src/src` - nothing) and phpstan-rs (no
`staticMethod.abstract`-shaped identifier in the existing static-call
identifier census: `staticMethod.notFound`/`.private`/`.protected`/
`.deprecated`/`.internalClass`/`.resultDiscarded`/`.resultUnused`, never
one for a directly-called abstract target).

**Effort: small** - the static-call resolution `staticMethod.notFound`
already runs through has the resolved method's `is_abstract` flag
available; this is an extra condition on an already-resolved lookup, not
new resolution machinery.

### L12. `NonVariableReferenceReturn` - a by-ref-returning function returning a non-variable

`function &foo() { return 5; }` (or any non-variable/non-property
expression) is illegal for a `by_ref`-declared function - there's no
real storage location to hand back a reference to
(`ReturnAnalyzer.php:237-249`, condition: `$storage->signature_return_
type->by_ref && !($stmt->expr instanceof Variable|PropertyFetch|
StaticPropertyFetch)`). Confirmed absent from real PHPStan (no
`OnlyVariable`/`returnByRef`-shaped rule in `vendor/phpstan-src/src`) and
from phpstan-rs (no by-ref-return-target validation anywhere in
`src/checker/`).

**Effort: small** - a check on `StmtKind::Return`'s expression kind,
gated on the enclosing function's own `by_ref` flag (already tracked for
by-ref *parameters* via `checker/by_ref_params.rs`'s existing machinery,
just not yet applied to the return side).

### L13. `InvalidStringClass` - `new $plainStringVar()`, stricter than real PHPStan's own `new.nonObject`

Real PHPStan's own `InstantiationRule::checkClassNameExprType`
(`Rules/Classes/InstantiationRule.php:93-116`) accepts *any* `string`-typed
value for `new $x()` unconditionally (`$acceptedType = StringType() |
ObjectWithoutClassType()`), with no further validation that the string
is actually a `class-string` - a real gap even in PHPStan's own model
that Psalm closes by default: `NewAnalyzer.php:1065-1075` rejects a
plain (non-`class-string`-narrowed) `string`, and unconditionally rejects
a `TNumericString` regardless of config, since a numeric string can
never validly name a class. phpstan-rs mirrors PHPStan's own gap here,
not just its behavior - confirmed via `checker/expr.rs:2040-2043`'s own
comment: a dynamic `new $className()` target "has no concrete name to
check" and is skipped entirely, not merely under-validated.

**Effort: small** - a type check at the same `ExprKind::New` site already
cited, requiring the resolved type be `class-string`-narrowed (or an
object) rather than an unnarrowed `string`.

### L14. `ImpurePropertyFetch` - reading (not just writing) a mutable object's property inside a declared-pure context

The one `Impure*`-family member phpstan-rs doesn't already cover: Phase
41's purity-disqualification walk (`registry/purity.rs:367-445`,
`expr_disqualifies`) already treats property *writes*, static access,
static variables, superglobal reads, and by-ref assignment as
disqualifying - matching real PHPStan's own `ImpurePoint` taxonomy
exactly (confirmed via `FunctionPurityCheck.php`, itself already cited
in this project's Phase 41 follow-up notes) - but neither phpstan-rs nor
real PHPStan disqualifies a plain property *read*
(`Internal/Analyzer/Statements/Expression/Fetch/AtomicPropertyFetchAnalyzer.php:507-520`,
condition: `$context->pure && !$class_storage->external_mutation_free`).
Psalm's stricter model: reading a mutable object's property inside a
`@psalm-pure` function isn't side-effecting, but it isn't
referentially-transparent either - the same "pure" call could observe a
different value on a second invocation if something else mutated the
object first, breaking the memoization-style guarantee purity is
usually used for. Narrow and debatable (arguably philosophy, not a bug),
but genuinely absent from both real PHPStan and phpstan-rs, and cheap to
add since the exact "is this a mutable-object receiver" check the
condition needs is already computed one line away for the write-side
check.

**Effort: small.**

## AP. Severity tiering as a systemic diagnostic policy, not a single rule

### AP1. Split "definitely wrong" from "possibly wrong" across type-compatibility diagnostics

Confirmed real PHPStan has no general definite-vs-possibly severity
split anywhere (`grep -rli possibly vendor/phpstan-src/src/Rules` turns
up only unrelated hits - purity/dead-code/named-argument rules, never a
systemic severity tier), and phpstan-rs mirrors that same flat model:
`checker/calls.rs`'s `argument.type` reports one message shape whether
the passed value is *definitely* the wrong type or a nullable/mixed
union that merely *might* be. Psalm splits this at the exact decision
point, repeated across its whole argument/operand/array-access/iterator
diagnostic family: `$input_type->isNull()` gets the hard `NullArgument`
error, `$input_type->isNullable()` (the union merely *contains* null
among other members) gets the softer, individually-suppressible
`PossiblyNullArgument` (`ArgumentAnalyzer.php:1309-1338`); the same
shape repeats for false/invalid-type/undefined-offset. This is a real
signal-to-noise improvement - separating "this call is definitely
broken" from "narrow this before use" - but it's a cross-cutting
severity policy, not a single new rule: adopting it means threading a
"is the offending member the *entire* type or just *part of* the
union" distinction through most of the existing type-compatibility call
sites (`calls.rs`, `assignment.rs`, `return_type_too_wide.rs`'s sibling
checks, `expr.rs`'s operand checks), each picking a harder/softer
message and (optionally) a distinct suppressible identifier for its own
existing diagnostic.

**Effort: large** (small per call site, but the value is in doing it
uniformly across every existing type-compatibility check, not once).

## AQ. The `Mixed*` family - type-erosion tracking with no PHPStan equivalent at all

### AQ1. Flag a `mixed`-typed value flowing into a typed context, with provenance

Confirmed real PHPStan has *zero* rules for this
(`find vendor/phpstan-src/src/Rules -iname '*mixed*'` returns nothing).
Psalm has 18 distinct issue types for exactly this shape -
`MixedAssignment`, `MixedArgument`, `MixedArrayAccess`,
`MixedMethodCall`, `MixedPropertyFetch`, and so on - each firing when a
value phpstan-rs's own inference would resolve to `Type::Mixed` reaches
a context expecting something more specific. The production
implementation (`AssignmentAnalyzer.php:356-376`) does more than flag
the symptom: it walks Psalm's dataflow graph to report *where* the
value became `mixed` in the first place (an untyped `json_decode()`
result, an unstubbed third-party call), not just that it currently is.
This is real type-erosion detection - a `mixed` value silently
defeating the type system somewhere upstream - that no PHPStan-based
tool catches today. It's also the largest single item in this part: a
bare "this is mixed" version is a straightforward walk of expressions
phpstan-rs's checker already resolves to `Type::Mixed` at the point
they're consumed by a typed context, but the *useful* version (pointing
at *where* the value became `mixed`) needs the same kind of
provenance-tracking dataflow graph the taint-analysis item just below
would also need - not separable infrastructure, so realistically a
shared prerequisite for both.

**Effort: large** for the provenance-tracking version; **medium** for a
bare presence-only version with no "where did this come from" tracing.

## AR. Investigated and deliberately not pursued

Recorded here so a future pass over Psalm doesn't re-spend the same
research budget re-discovering the same "not worth it" verdict:

- **Taint analysis** (`Tainted*`, 17 issue types; `Internal/Codebase/
  TaintFlowGraph.php`, ~1,100 lines just for the graph itself, plus
  source/sink annotations threaded through most of the type-checking
  pipeline). Real, valuable, and completely absent from phpstan-rs and
  PHPStan alike - but architecturally a whole separate subsystem
  (a source-to-sink dataflow graph), not a bolt-on rule. Worth a
  dedicated future scoping pass of its own, not folded into this list.
  AQ1's provenance tracking above would share infrastructure with it.
- **The reference-aliasing family** (`ConflictingReferenceConstraint`,
  `ReferenceConstraintViolation`, `ReferenceReusedFromConfusingScope`,
  `UnsupportedReferenceUsage`, `UnsupportedPropertyReferenceUsage`) -
  read the actual implementation (`IfAnalyzer.php:200-215`,
  `ExpressionAnalyzer.php:378-388`, and siblings); these are Psalm's own
  internal "my by-reference type-tracking couldn't cope with this code"
  escape hatches (one message is literally "This reference cannot be
  analyzed by Psalm"), not a distinct user-facing bug class. Low value
  for the size of subsystem (a full reference-aliasing type model) it'd
  require.
- **`ComplexFunction`/`ComplexMethod`** (a complexity-limit warning) -
  real, but its metric is derived from Psalm's taint/dataflow graph's
  own edge statistics (`StatementsAnalyzer.php:886-910`), not a cheap
  standalone cyclomatic-complexity count. Not separable from the
  taint-analysis prerequisite above.
- **`ImplicitToStringCast`** - real PHPStan already has equivalent
  machinery (`Analyser/ExprHandler/ImplicitToStringCallHelper.php`);
  parity territory, not Psalm-exclusive.
- **`OverriddenFinalConstant`/`AmbiguousConstantInheritance`/
  `OverriddenInterfaceConstant`** (PHP 8.1+ final/interface-constant
  override conflicts) - real PHPStan already has this family
  (`Rules/Constants/OverridingConstantRule.php`); already tracked as a
  parity gap in ROADMAP.md's "Newly identified gaps" (Constants
  section), not added here to avoid a duplicate.
- **`UnsafeInstantiation`** (`new static()` without
  `@psalm-consistent-constructor`) - real PHPStan parity territory
  (`Rules/Classes/NewStaticRule.php`), not Psalm-exclusive.
- **`ParamNameMismatch`** (an overriding method renaming a parameter
  that named-argument calls depend on) - phpstan-rs already covers this,
  and more precisely: `checker/named_argument_renames.rs` tracks actual
  call sites project-wide (mirroring real PHPStan's Phase 82), where
  Psalm's version is a purely declaration-time static check.
- **`PrivateFinalMethod`** (redundant `final` on an unoverridable
  private method) - already covered
  (`checker/oop.rs:874-879`, `method.finalPrivate`, matching real
  PHPStan's identical rule).
- **`MissingImmutableAnnotation`/`MutableDependency`** (subclass/trait
  immutability-annotation consistency) - doesn't apply to this project's
  design: `registry::class_is_immutable` (`hierarchy.rs:80-96`) already
  walks `extends` transitively, so immutability is automatically
  inherited rather than needing to be redundantly restated: closer to a
  design improvement over Psalm's requirement than a gap.
- **`PropertyNotSetInConstructor`/`DuplicateEnumCaseValue`** - already
  covered (`checker/uninitialized.rs`, `checker/oop/enums.rs:39-59`).
- **`ReservedWord`** (a user class named `int`/`string`/etc.) - real PHP
  already rejects this at compile time; a static pre-check's practical
  value is marginal.
- **`RedundantFlag`** - narrow, single-function-scoped (`filter_var()`'s
  `flags`/`options` array keys specifically), not a generalizable
  pattern.

The rest of this list was added on a second, genuinely exhaustive pass
after the first pass turned out to have sampled rather than read every
one of the 325 Issue classes individually (the family-level bulk
verification described in this part's own intro above). These are the
additional items that pass turned up and ruled out - not because they
were skipped, but because each was individually read and found to
already be covered, out of scope, or too narrow to be worth the size of
change:

- **`InvalidOverride`** - turned out to be the exact mirror of
  phpstan-rs's own existing check: `#[\Override]` present with no real
  overridden method, or present on `__construct`
  (`FunctionLikeAnalyzer.php:2017-2023`) is precisely `own_sig.has_
  override_attr && !has_real_ancestor` at `method_overrides.rs:273-282`.
  Same check, different issue-class name.
- **`NoInterfaceProperties`** (assigning to a property through an
  interface-typed variable) - interfaces have no properties to find in
  the registry in the first place, so this already falls through to the
  general undefined-property-assignment path every other "property
  doesn't exist" case already goes through.
- **`NonInvariantPropertyType`/`NonInvariantDocblockPropertyType`**
  (property-type invariance on override) - already covered,
  `checker/oop/property_overrides.rs:29-53`'s `check_one_property_
  override`, mirroring real PHPStan's `OverridingPropertyRule`.
- **`ConstantDeclarationInTrait`** (PHP 8.2+ gate on trait constants) -
  already covered, `classConstant.inTrait`
  (`checker/oop.rs:710-731`), matching real PHPStan's
  `ConstantsInTraitsRule` exactly.
- **`TraitMethodSignatureMismatch`** - same visibility-on-override check
  already confirmed for `OverriddenMethodAccess` (`method.visibility`),
  just a trait-specific message variant in Psalm, not a distinct check.
- **`UninitializedProperty`** (reading a property before it's
  definitely initialized) - already covered by
  `checker/uninitialized_dataflow.rs`'s per-constructor "was this
  definitely assigned by this exact point" walk, a deliberately-scoped
  design choice (documented in that module's own header) rather than an
  oversight.
- **`LessSpecificReturnStatement`** - the same general declared-vs-actual
  return-type mismatch phpstan-rs's `return.type` already checks, just
  phrased "more general than" instead of "incompatible with."
- **`InvalidClassConstantType`/`LessSpecificClassConstantType`**
  (typed-class-constant override compatibility, PHP 8.3+) - real but
  narrow: typed class constants are rare, and overriding one with an
  incompatible/wider type rarer still. Read both (`ClassConstAnalyzer.php:
  787-810`), judged too low-incidence to be worth chasing further here.
- **`MoreSpecificReturnType`**, **`IncompatibleTypeParameters`** - both
  tied tightly to Psalm-internal inference mechanics (explicit-`never`
  union tracking; `TypeVariableTracker`'s own bidirectional bound
  resolution) with no clean general pattern to port.
- **`ArgumentTypeCoercion`/`PropertyTypeCoercion`/`MixedArgumentTypeCoercion`/
  `MixedPropertyTypeCoercion`/`MixedArrayTypeCoercion`/`MixedReturnTypeCoercion`**
  - conceptually the same "definite vs. possibly" severity split already
  proposed as its own systemic item (AP1) below, not a separate check.
- **`EmptyArrayAccess`** (indexing a provably-empty array) - an empty
  array shape has zero valid keys, so this is already a degenerate case
  of the general shape-key-existence check (`shapeAccess.notFound`)
  rather than needing its own special case.
- **`RedundantPropertyInitializationCheck`** (an `isset()` check on an
  already-non-nullable property) - same "redundant condition given a
  known type" territory L8 above already covers.
- **`CircularReference`** (a class constant whose own value expression
  references itself, e.g. `const A = self::A;`) - real (PHP treats this
  as a fatal error at runtime) but confirmed low-risk in phpstan-rs's own
  architecture: `infer_const_type` (`registry/function_sig.rs:165-198`)
  never recursively resolves a cross-constant reference at all - it
  falls straight to `Mixed` for anything beyond a literal - so there's no
  infinite-recursion hazard, just an undetected rare-in-practice bug
  class. Low enough incidence not to be worth its own item here.
- **`ParentNotFound`** (`parent::$prop` fetch in a class with no parent)
  - real but low-incidence; the `parent`-as-*type* case is already
  covered (`return.noParent`/`parameter.noParent`), just not this
  narrower `parent::`-as-property-fetch-prefix variant.
- **`CheckType`/`Trace`/`UndefinedTrace`** - Psalm's own `@psalm-check-type`/
  `@psalm-trace` debugging annotations, the same category as real
  PHPStan's own `\PHPStan\dumpType()`/`debugScope()` developer-debug
  helpers (already noted as out of scope under ROADMAP.md's own
  "Keywords+Debug" section) - not a user-facing code check at all.
- **`UnrecognizedExpression`/`UnrecognizedStatement`/`PsalmInternalError`/
  `ParseError`/`UnresolvableConstant`/`UnresolvableInclude`** - Psalm's
  own "I can't make sense of this" internal fallbacks and giving-up
  notices, not a portable diagnostic concept.
- **`IfThisIsMismatch`** - tied to Psalm's own `@psalm-if-this-is`
  conditional-`$this`-typing template mechanism, a Psalm-specific
  generics feature with no clean separation from Psalm's whole template
  system.
- **`InvalidGlobal`/`InvalidStaticInvocation`/`InvalidParent`/
  `InvalidAttribute`/`NoValue`** - each read individually; real but
  either tied to Psalm's own script-execution-scope model (`InvalidGlobal`),
  already covered by an existing check (`InvalidStaticInvocation`,
  same `method.static` family as `NonStaticSelfCall`), or narrow enough
  (a literal `: parent` return type with no parent class; a trait used
  as an attribute class; Psalm's own type-narrowing producing `Never`
  for an argument) that the size of a dedicated check isn't justified
  by how rarely it would fire.
- **`DuplicateParam`** (a function declaring the same parameter name
  twice) - real (confirmed via real PHPStan's own
  `Rules/Functions/RedefinedParametersRule.php`) but already documented
  as a parity gap in ROADMAP.md's "Newly identified gaps" (bundled into
  the `Functions/InvalidParameterNameRule.php...` entry), not added here
  to avoid a duplicate.
- **`InaccessibleProperty`/`InaccessibleMethod`/`InaccessibleClassConstant`**,
  the rest of the **`Undefined*`/`Invalid*`/`Missing*Type`/`Deprecated*`/
  `Duplicate*`/`Impure*`(except L14)/generics/enum/interface/abstract**
  families, and **`UndefinedInterface`** - all individually read this
  pass (not just identifier-census-matched as the first pass did) and
  confirmed already covered by phpstan-rs's existing, comprehensive
  type-checking, visibility, and symbol-resolution machinery.
