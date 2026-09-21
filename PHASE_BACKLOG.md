# phpstan-rs phase backlog (85+)

This file merges the two untriaged idea backlogs this project had
accumulated — ROADMAP.md's own "Newly identified gaps" (a parity audit
against real PHPStan's `Rules/`) and BEYOND_PHPSTAN.md's 100-finding
"beyond parity" backlog — into one phase-numbered list, continuing
ROADMAP.md's existing phase sequence (1-84) rather than renumbering it.

**This file does not replace ROADMAP.md or BEYOND_PHPSTAN.md.** Both stay
the source of truth for their own material:
- ROADMAP.md: phases 1-84 (Done/Pending), their own full Done-section
  prose, Reverted, Known limitations, Permanent design boundaries, and
  Won't-port. This file does not duplicate any of that.
- BEYOND_PHPSTAN.md: every phase below sourced from it keeps that
  document's own exhaustive file:line citations (both
  `vendor/phpstan-src`/`vendor/phpstan-strict-rules` and this project's
  own `src/`) — this file condenses each finding to what's needed to
  schedule and scope it; go back to BEYOND_PHPSTAN.md's own numbered
  finding (e.g. "A1", "U3") for the full grounding before implementing.

**Status:** every phase below is **PLANNED** — none of this is built,
with twenty-two exceptions: Phases 166, 169, and 174 (unblocking two
live dependency edges in `PHASE_DEPENDENCIES.md`), Phases 161, 162,
167, 170, 171, and 172 (the small narrowing-engine batch this
unblocking made it natural to pick up next), and Phase 96's D8, D2,
D5, D6, and D7, Phase 97's AA3 and AA5, Phase 98's AB2, Phase 101's
E4, E3, and E1, Phase 102's W1 and W3, and Phase 103's Z1 (thirteen
small, self-contained items picked up next as standalone units - D2,
D6, Z1, E3, AA3, AA5, E1, and W1 all opt-in, per this project's "ship
opt-in" convention: D2 for a real false-positive risk on unambiguous
single-bool-parameter calls, D6 because it's a disclosed heuristic
(two same-typed parameters are structurally invisible to any
type-system-based check as "different roles"), Z1 because real
PHPStan itself only ever ran it in a private issue-bot config leaving
no real-world noise signal, E3, E1, and W1 because real PHPStan ships
each only via a separate opt-in mechanism of its own (E3/E1 via
`phpstan-strict-rules`, W1 via its own opt-in `checkDynamicProperties`
parameter; E3's own legitimate-exception risk: a constructor
genuinely choosing not to call an otherwise-real parent one), AA3 and
AA5 because both are purely stylistic suggestions, not bugs. W3's own
method-call/static-call half additionally required and got a real
correctness fix to `Registry::lookup_method`/`lookup_method_with_
owner` along the way - see ROADMAP's Done section). All twenty-two
are now **DONE** and have moved into ROADMAP.md's own Done section,
per this project's standing "fold DONE phases into Done" convention -
their entries are removed from this backlog file, not left in place
with a status tag flipped.
"Triaged" here means "assigned a phase number and scoped," not
"scheduled" in any priority order; the numbering is sequential by
source-document position, not urgency. Pick items the same way any
other Pending/partial phase gets picked up.

**Numbering:** 85-124 = BEYOND_PHPSTAN.md's 40 lettered sections, one
phase each, in that document's own Part 1-9 order (sub-items keep their
original letter+number, e.g. 85's own "A1"-"A6"). 125-139 = ROADMAP.md's
15 "Newly identified gaps" categories, one phase each. 140 = ROADMAP.md's
"Known but not yet scheduled" residuals, one phase with lettered
sub-items. 141+ = a real, one-by-one source read of the top 100
Packagist `phpstan-extension` packages by download count (see
`TOP100_PHPSTAN_EXTENSIONS.md`), triaged the same way: genuinely new,
portable findings get their own phase or a sub-item on an existing one;
single-framework/single-library glue (Symfony/Doctrine/Laravel/Drupal/
WordPress/TYPO3/CakePHP/Magento/SilverStripe/Nette/CodeIgniter/GLPI/
Maho, and every single-library mock/assert/type-extension package) is
named once per recurring theme below rather than once per package —
every one of those needed a framework's own runtime metadata model
phpstan-rs has no equivalent of, confirmed individually for all ~40
such packages, not assumed from the framework's name.

---

# Part 1 (BEYOND_PHPSTAN): Precision, whole-program analysis, closed-world inference

phpstan-rs analyzes one closed-world project+vendor universe in a single
process with one frozen `Registry`; real PHPStan must stay sound for
arbitrary downstream consumers of a library it's analyzing, and its
per-file reflection is fixed before a function's body is even walked.
Every phase in this Part is either PHPStan approximating for that
architectural reason where phpstan-rs doesn't have to, or a
whole-program fact PHPStan's own 14-collector `CollectedDataNode`
machinery makes expensive that phpstan-rs's single live `Registry` gets
nearly for free.

**Shared prerequisite for every whole-program *diagnostic* phase below
(86-94, and Part 8's composer-hygiene phases 113-120) - now built,
found and closed while implementing Phase 92 (folded into ROADMAP's
Done section, "Whole-program diagnostic-emission pipeline" - see there
for the full design).** This project used to have no whole-project,
non-per-file diagnostic-emission path at all, and `Project::decl_
locations` used to be gated behind `retain_source` (off in batch
`check-project` mode) - both fixed. Any of these phases can now build
directly on `Project::check_whole_program` (gate a new opt-in `Registry`
bool, resolve findings through `decl_locations` exactly like Phase 92's
own `checker::whole_program::dead_enum_cases` does) instead of
re-deriving this infrastructure.

## Phase 85 — More precise than real PHPStan itself (BEYOND_PHPSTAN §A)

Real PHPStan approximates in these spots for its own architectural
reasons; phpstan-rs doesn't share the reason.

- **A1 (small).** Raise `converge_loop_body`'s `MAX_PASSES = 3` cap
  (`checker/control_flow.rs`) — copied from PHPStan's own
  `LOOP_SCOPE_ITERATIONS = 3`, but without copying the *reason* 3 is
  tolerable there: PHPStan widens literal types to their base type after
  just 1 pass (`GENERALIZE_AFTER_ITERATION = 1`) specifically because
  each pass re-walks the whole Scope/Rule-registry/PHPDoc machinery.
  phpstan-rs's own convergence does plain union merging with no early
  widening — already more precise per pass, just capped for no real
  reason. Raise to 5-8, or adaptive on non-convergence.
- **A2 (large, new feature).** Unroll `foreach` over small compile-time
  arrays with a higher ceiling than PHPStan's 16/8
  (`FOREACH_UNROLL_LIMIT`/`_NESTED_LIMIT`) — `check_foreach`
  (`checker/stmt/loops.rs`) has no unrolling at all today, always
  unioning every `ArrayShape` field into one coarse element type.
- **A3 (medium, folds into whichever phase builds `offsetAccess.notFound`
  — see Phase 125 Arrays).** When that check is built, its power-set
  key-presence expansion should use a bailout tuned to phpstan-rs's own
  (cheap `Vec` clone) per-variant cost, not a reflexive import of
  PHPStan's 16384 (`TypeUtils::flattenTypes`, object-allocation-costed
  there).
- **A4 (medium).** Check `BenevolentUnionType` results by default instead
  of leaving them permanently unchecked. Real PHPStan computes a real
  narrowed result for `mixed + mixed`/`mixed & mixed`/etc.
  (`InitializerExprTypeResolver::getBitwiseAndType`/etc.) then
  *systematically declines to check it* (`checkBenevolentUnionTypes`
  hardcoded `false` in every shipped level, protecting an installed base
  phpstan-rs doesn't have). phpstan-rs's own `check_binary_operand_validity`
  bails on `Mixed` operands entirely — could compute and enforce the
  same narrowed union by default.
- **A5 (large).** Auto-infer closed (sealed) hierarchies from
  whole-program subclass enumeration, not just an explicit
  `@phpstan-sealed` tag. Real PHPStan's `ObjectType::isInstanceOf`
  always returns `Maybe` for an untagged class — deliberate, since a
  library's downstream consumers can add subclasses it never sees.
  phpstan-rs's closed-world model makes "zero subclasses found anywhere
  in the analyzed set" sound. Needs a new reverse (parent-to-children)
  index on top of `hierarchy.rs`'s forward-only `all_ancestor_names`
  (reused directly by Phases 88/94/93 below).
- **A6 remainder (small).** `Add`, `Sub`'s constant-shift fast path, and
  `Mul` now union `Float` in on genuine `i64` overflow (see ROADMAP's
  Done section) - `sub_int_range_full`'s own general range-minus-range
  case (right side not a single constant) is a faithful transcription
  of real PHPStan's own multi-branch logic and was left untouched to
  avoid risking a real bug in already-correct, carefully-verified code;
  still silently discards a would-be float bound as unbounded there.

## Phase 86 — Whole-program analysis PHPStan's architecture can't cheaply do (§B)

PHPStan computes real dependency graphs but discards them after each
run, only to decide what to re-check next; phpstan-rs already keeps
`Project.dependents` live, in memory, before/during checking.

- **B1 (large).** Whole-application unused protected/public member
  detection. PHPStan's only dead-member rule gates on `isPrivate()`
  because verifying no *other file* reaches a protected/public member
  needs its cross-process collector machinery. Extend `checker/
  unused/member_usage.rs`'s existing private-member tracker to walk
  every class's protected/public accesses project-wide via the registry.
  **Triple-confirmed by real production tools** (top-100 Packagist
  survey): `shipmonk/dead-code-detector` and `tomasvotruba/unused-public`
  are both comprehensive, independently-built, actively-maintained
  implementations of exactly this shape — worth reading both before
  building. Two concrete refinements they surface: (1) `shipmonk/dead-
  code-detector` ships a real auto-fix (deletes the dead declaration's
  source text) — pair with a Phase 111-style codemod once built, not
  just detection; (2) both tools' own ~20 framework "usage provider"
  plugins (Twig/Symfony/Laravel/Nette/Doctrine/Eloquent/Blade/Behat/
  PHPUnit/Composer/plain Reflection) reveal the real scoping risk any
  sound whole-project dead-code detector must handle — a member
  reachable only via framework magic (a template calling a method by
  name, a DI container definition, PHPUnit attribute discovery, runtime
  Reflection) is invisible to ordinary reference-counting — B1 should
  explicitly generalize the same conservative "escaped means don't
  flag" discipline `member_usage.rs`'s existing private-member
  `escaped` fallback already uses, rather than assume it away when
  widening to protected/public members (B2, below, already does this
  for top-level functions/classes - see ROADMAP's Done section). This
  is also the one sub-item of Phase 86 still actually needing
  `Project.dependents` (or a Tier-1-shaped replacement, per B2's own
  precision finding) - B2 and B3 both ended up not needing it at all,
  once built.
  `tomasvotruba/unused-public` additionally covers properties and class
  constants, not just methods — broaden B1's own scope statement to
  match — and separately flags a public method *never called from
  outside its own declaring class* (over-visible, not dead) as a
  "could be private" suggestion, worth a small B4 sub-item alongside B1
  if picked up.
- **B2 (medium, DONE)** — dead top-level function/class detection,
  folded into ROADMAP's Done section ("Whole-program dead-function/
  dead-class detection"). Built on `Project::use_sites`'s canonical,
  `NameContext`-resolved Tier-1 index, not `Project.dependents` (that
  index's own raw, unresolved, bare-last-segment string keys turned out
  too imprecise for a real "is this ever referenced" existence check -
  see that Done entry for the full reasoning).
- **B3 (medium, DONE)** — namespace-level circular structural-dependency
  detection, folded into ROADMAP's Done section ("Namespace-level
  circular-dependency detection"). Built directly on `Registry.classes`'
  own already-resolved `extends`/`implements`/`uses` fields instead of
  `Project.dependents` either - narrower than the "any reference"
  cycle graph this sub-item's own sketch originally described (a cycle
  formed purely through method-body calls/type hints is out of scope),
  but fully precise and effectively free, needing neither `dependents`
  nor `use_sites` at all - see that Done entry for the full reasoning.

## Phase 87 — Whole-program value-flow diagnostics (§H)

Same whole-project registry-fixed-point architecture as Phase 86, for
questions about *values* flowing through the program instead of
existence.

- **H1 (large).** "Dead flexibility" parameter detection — a parameter
  with 2+ call sites that always resolves to the exact same literal
  value project-wide is a hardcode/drop candidate. No PHPStan collector
  aggregates argument *values* across call sites (only named-argument
  *names*, `NamedArgumentParameterMethodCallsCollector`).
  `resolve_call_args` already resolves the per-site literal `Type`
  (`checker/calls.rs`) — just needs a project-wide accumulator.
- **H2 (medium).** Whole-program inferred "effectively NoDiscard"
  detection — a non-void callee whose return value is discarded at
  *every* recorded call site infers the same fact a real
  `#[\NoDiscard]` attribute would assert. `FunctionSig.no_discard` is
  exactly the bit to set automatically; `check_useless_return_value_call`
  already answers "was this call's result unused" per site.
- **H3 (medium).** Whole-program property-type narrowing from every
  assignment site — if every `$this->prop = rhs` across the whole
  subclass-aware hierarchy is a strict subtype of the declared type,
  suggest narrowing to a literal union/backed enum. No PHPStan rule or
  collector intersects RHS types across assignment sites.

## Phase 88 — Closed-world inference replacing PHPStan's doc-tag-only gaps (§U)

Six cases with an identical shape: PHPStan resolves a fact purely from
an author-written PHPDoc tag on the immediate declaration, with zero
fallback when it's missing, because its per-file reflection is fixed
before the body is walked. phpstan-rs's Phase 41 transitive-purity
fixed point already proves this exact shape works for one fact; these
extend it to five more.

- **U1 (large).** Resolve `method_exists()`/`property_exists()`
  narrowing to real closed-world signatures, not PHPStan's
  `DummyMethodReflection` stub (zero params, everything `Mixed` —
  architecturally forced open-world leniency). Invert `Registry.classes`
  into a member-name → declaring-classes reverse index so a literal
  `method_exists($x,'foo')` narrows `$x` to the union of every real
  class anywhere in the closed universe declaring `foo`.
- **U2 (large).** Registry-level transitive `@throws` inference,
  replacing PHPStan's docblock-only per-hop requirement — extend
  `registry/purity.rs`'s fixed point with a sibling computing each
  function's *real* union of thrown checked-exception types from its
  own body, independent of whether `@throws` exists. This is also what
  directly unblocks the `MethodThrowTypeCovarianceRule` port ROADMAP
  Phase 45 already declined for exactly this "untagged means unknown,
  not nothing" reason. **A concrete future consumer** (top-100
  Packagist survey): once U2 exists, `nette`'s own `RethrowException
  Rule` idea (flag a `try` block containing a call that can throw a
  specific exception subtype when the only `catch` present is a bare
  `\Exception`/`\Throwable` with no more-specific clause and no re-
  `throw`) becomes a strictly more powerful, framework-independent
  check — works for *any* throwing call project-wide instead of
  Nette's own hardcoded per-method exception table. Fold in as a
  concrete U2 consumer, not a separate framework-specific item.
  Separately, `pepakriz/phpstan-exception-rules` confirms an
  `UnreachableCatchRule`/`DeadCatchUnionRule` pair is real, buildable,
  and previously uncatalogued: a `catch (Specific $e)` clause (or one
  member of a `catch (A|B $e)` union) matching nothing the `try` body
  can actually throw, once U2's aggregate throw-type is known — a real
  dead-code family specific to exception handling, worth its own small
  sub-item (U2a) once U2 lands.
- **U3 (large).** Registry-level by-ref parameter out-type inference,
  the by-ref analogue of U2 — real `ParameterOutExecutionEndTypeRule`
  literally short-circuits when no `@param-out` tag exists. Recurse
  through a nested by-ref call's own computed out-type via the same
  fixed point instead of requiring a hand-written tag on it.
- **U4 (large).** Closed-world inference of `@phpstan-self-out` for the
  straight-line case (a void/self/static-returning body of direct,
  unconditional `$this->prop = <expr>;` with no branches) — real
  PHPStan never attempts inference here either direction, only trusts
  an existing tag unconditionally. (Depends on `@phpstan-self-out`
  itself gaining real tag support first — currently only in the
  typo-detection whitelist.)
- **U5 (large).** Cross-check `@phpstan-require-implements`/`-extends`
  against actual trait usage (flag a tag broader than the trait body
  demonstrably needs), and use the required type's real member set to
  widen `$this` in phpstan-rs's own existing orphan-trait fallback
  (`check_trait_methods`) — real PHPStan structurally never walks a
  trait body with zero using classes at all, so it can't do either half.
- **U6 (medium).** Infer an implicit `@phpstan-assert` fact when a
  function/method's entire body is exactly one
  `if (!<condition>) { throw ...; }` guard clause — a single-function,
  non-recursive syntactic pattern match feeding the existing
  `FunctionSig::assert` field every consumer already reads uniformly.
  An explicit tag always wins when present.

## Phase 89 — Whole-program structural duplication (§V)

Extends single-function duplication checks (Phase 96's D3/D7) to
duplication *across* declarations project-wide.

- **V1 (medium).** Whole-program duplicate function/method body
  detection — canonical structural fingerprint (postorder hash,
  alpha-renamed locals) bucketed by hash, confirmed via an
  alpha-equivalence `Stmt`-list comparator (generalizing
  `is_switch_break_equivalent`). Skip related pairs
  (`is_subclass_of`/`classes_using_trait`) and trivial bodies. PHPStan's
  `Rules/` has no body-content comparator at all.
- **V2 (large).** Divergent-clone detection — near-duplicate bodies
  differing by exactly one leaf (a differing literal/operator), via a
  coarser shape-only pre-filter over V1's machinery plus a bounded
  leaf-divergence count. A correctness signal (a fix applied to one
  copy, forgotten in its sibling), not style.

## Phase 90 — Whole-program redundant-override detection (§AI)

V1's (Phase 89) mirror image: reuses the same alpha-equivalence
machinery but fires *only* when the override relationship holds and the
overriding method's body is alpha-equivalent to the ancestor it
overrides — dead code or a forgotten divergent edit. Skip trivial
0-1-statement bodies (bare `parent::foo()` forwarders). Real PHPStan's
408-line `OverridingMethodRule` never once reads body content from
either side. **Effort: small.**

## Phase 91 — Whole-program parameter-type narrowing from call-site arguments (§X)

- **X1 (large).** Narrow a "wide" parameter (an interface/non-final
  abstract class, or a 2+-class Union) to the one concrete class
  actually passed, when 2+ real call sites all agree — retaining and
  aggregating `arg_ty`, a value `check_args` already computes and
  currently discards. No PHPStan collector aggregates per-call argument
  *types*.
- **X2 (medium).** Same aggregation, second dimension: narrow a plain
  `array`-typed parameter to `non-empty-array` when every real call site
  provably passes a non-empty value (`is_known_non_empty_array` already
  exists for this exact test).

**Confirmed already shipping in a real, popular tool** (top-100
Packagist survey): `rector/type-perfect`'s own `PublicClassMethodParam
TypesCollector`/`NarrowPublicClassMethodParamTypeRule` is literally X1's
own proposed mechanism, already live using real PHPStan's Collector
architecture (which core PHPStan itself has no first-party equivalent
of) — cite as concrete "this exact idea already ships" evidence when X1
is picked up. The same package's `NarrowReturnObjectTypeRule` is the
return-type sibling, closer in spirit to the already-built too-wide-
return-type check (Phase 111's I3/Phase 45's 45A) than to X1 itself.

## Phase 93 — Whole-program dead abstract-class detection (§AJ)

For every `is_abstract` class, walk the reverse subclass index (Phase
85's A5) to confirm no concrete descendant exists anywhere in the closed
universe — not "currently unused," a permanent structural fact. No
PHPStan rule exists (its `Reflection/` layer has no descendant-
enumeration API at all, for the same open-world reason as A5). Distinct
from Phase 86's B2 (name-reference-based dead code — a polymorphic-
dispatch-only abstract class has non-zero dependents there even with
zero real subclasses). **Effort: medium.**

## Phase 94 — Closed-world precision unlocking Pending techniques further (§P)

Two cases where finishing/extending an existing mechanism with
phpstan-rs's own closed-world registry resolves something real PHPStan
is *structurally* unable to resolve, not just unbuilt.

- **P1 (large).** Resolve provably-closed virtual dispatch inside the
  transitive-purity fixed point (`registry/purity.rs`) — a `$this->
  method()` call through a non-final overridable method isn't
  unconditionally disqualified anymore if the same reverse index (A5)
  proves zero real overrides exist anywhere in the closed universe.
  Real PHPStan's `PossiblyPureCallTransitivePurityResolver` unconditionally
  disqualifies here, by deliberate open-world policy, not a cost cap.
- **P2 (medium).** Resolve real Composer package boundaries for `@internal`
  enforcement (Phase 67), replacing the root-namespace-segment heuristic
  both projects currently share, using `composer::
  find_composer_package_name` (already built for Part 8's composer
  hygiene work below) — closes both false negatives (two same-vendor
  packages sharing one namespace root) and false positives (one package
  spanning several namespaces). **Reinforced by two independent real
  packages** (top-100 Packagist survey): `dave-liddament/phpstan-php-
  language-extensions`' `Package{Static,New,Method}CallRule` family
  directly overlaps this item — real Composer-package-boundary
  visibility enforcement via attributes, worth reading as a second
  reference implementation. `pb30/phpstan-composer-analysis` (wrapping
  the standalone `shipmonk/composer-dependency-analyser` tool)
  independently needs the identical class-FQN → owning-package index
  P2 needs for its own, differently-shaped Phase 142 checks below — the
  two items should share one `composer::find_composer_package_name`-
  style implementation, not duplicate it.

---

# Part 2 (BEYOND_PHPSTAN): Native-performance wins real PHPStan needs a separate extension for

Real PHPStan's own team ships (or is building) these as opt-in native
C++ ("Turbo") because stock PHP can't afford them. phpstan-rs is already
native for every user — it needs the data structure, not the extension
architecture.

## Phase 95 — Native-performance (§C)

- **C1 (large).** Type interning + memoized union/intersect/remove
  (`TypeCombinator` caching), on by default. Real PHPStan's own
  `TypeCombinatorCache` documents "~91% of calls repeat an argument
  tuple already computed" — shipped only behind Turbo, so most installs
  never get it. phpstan-rs hit the identical problem shape already
  (`checker/type_union.rs`'s own doc comment records a real `gdb`-caught
  hang inside `Type::eq`/`union_types`/`merge_scopes`, worked around with
  an ad hoc `MAX_UNION_MEMBERS = 12` cap rather than the root fix).
- **C3 (medium).** Memoize class-hierarchy member lookups
  (`Registry::lookup_in_hierarchy_with_owner`, re-walks the full
  ancestor BFS from scratch on every call) — real PHPStan's own
  July-2026 profiling names the identical lookup cost as a still-open
  target, unsolved even by its native extension. A plain per-(class,
  member) memo table is immediately buildable once the class graph
  freezes at `finish_loading()`.
---

# Part 3 (BEYOND_PHPSTAN): New rule categories and design-pattern suggestions

Not approximation gaps — PHPStan's entire `Rules/` catalog simply has no
rule of these shapes at all, confirmed by grep.

## Phase 96 — New rule categories with zero PHPStan precedent (§D)

- **D1 (medium).** Function/method complexity/maintainability metrics
  (cyclomatic complexity, max nesting, parameter count) — zero PHPStan
  rule is keyed on body *shape* at all. Additive to the existing generic
  `stmt_children`/`expr_children` walk every function already passes
  through. **Algorithm recommendation from a real, popular tool**
  (top-100 Packagist survey): `tomasvotruba/cognitive-complexity`
  implements Campbell/SonarSource's "Cognitive Complexity" metric
  specifically (weighs nested control flow more heavily than a flat
  sequence of branches, unlike plain cyclomatic complexity) — build
  against that named algorithm, not an unspecified "some complexity
  metric." The same package's `ClassDependencyTreeRule` (sum a class's
  own constructor-injected dependencies' complexity *transitively* —
  this class's complexity plus everything `__construct()` pulls in,
  recursively — flag when the total exceeds a threshold) is a distinct,
  novel "god object with a deep, complex dependency graph" smell worth
  a D1b sub-item; needs only recursive constructor-param-type
  resolution through the already-live closed-world registry.
- **D3 (medium).** Identical-branch / structurally-duplicate conditional-
  body detection — two branches with satisfiable-but-different
  conditions and identical bodies; PHPStan's `Rules/Comparison`/
  `DeadCode` families only ever reason about condition *values*, never
  body equality. Reuses `is_switch_break_equivalent`'s technique one
  level up.
- **D4 (medium).** Narrow, single-function-scoped taint check —
  superglobal input reaching `eval`/`exec`/`unserialize`/a non-literal
  `include` within one function body only, no interprocedural tracking.
  PHPStan has zero taint infrastructure. Ship with an explicit
  false-positive-risk caveat; stay single-function-scoped.
- **D5 remainder (small).** `@see`'s own validation is now built (see
  ROADMAP's Done section) — the one piece still open is
  `brainbits/phpstan-rules`' `CoversClassExistsRule` sibling (validates
  a PHPUnit `@covers`/`#[CoversClass]` tag's target class exists),
  originally scoped to fold into the same D5 item rather than a
  separate check.

## Phase 97 — Whole-program design-pattern suggestion rules (§AA)

Real PHPStan's machinery in this space only ever *validates* an
existing stricter annotation, never *suggests* adding one — a
directional asymmetry every sub-item below exploits.

- **AA1 (medium).** "Readonly candidate" suggestion — a non-readonly,
  non-static, non-promoted property assigned exactly once project-wide,
  as a plain (non-compound, non-by-ref) `Assign` inside the declaring
  class's own constructor, is a mechanically safe readonly candidate.
  Distinct from Phase 87's H3 (site *count*, not RHS type diversity).
  **Third independent confirmation** (top-100 Packagist survey):
  `shipmonk/phpstan-rules`' `EnforceReadonlyPublicPropertyRule` ships
  the *enforcement*-mode sibling of this same idea (hard requirement,
  not a suggestion) — real, valued in production, worth citing as
  evidence when AA1 is picked up, and worth offering both a suggestion
  mode (AA1 as scoped) and an opt-in hard-enforcement mode matching
  shipmonk's own stricter default.
- **AA2 (medium).** Suggest narrowing a public constructor to
  private/protected when `new ClassName(...)` is only ever called from
  exactly one static factory method project-wide — excluding classes
  extended elsewhere or escaping into dynamic instantiation.
- **AA4 (small).** Class-level `readonly class` shorthand once every
  property independently qualifies under AA1 — one aggregation step on
  top of AA1, suppressing the per-property suggestions for that class.
- **AA6 (medium).** Suggest `@template-covariant`/`-contravariant` when
  actual usage already proves it safe — `walk_variance`'s existing
  per-class traversal just needs a second, always-run pass that
  *records* positions (In/Out/Inv) across every occurrence of an
  undeclared/invariant template instead of comparing them against a
  pre-existing declared variance. No PHPStan code path infers variance
  from usage anywhere.

## Phase 98 — Two PHP 8.4 rough edges (§AB)

- **AB1 (medium).** Purity/impure-point enforcement for property hooks.
  Real PHPStan's own `PropertyHooksProcessor` gathers `ImpurePoint`s from
  a hook body but no `Rule`/`Collector` anywhere ever reads them —
  gathered and never consumed, confirmed in PHPStan's own source. Wire
  phpstan-rs's existing `check_declared_purity_tag` into
  `check_get_hook`/`check_set_hook` (`checker/oop/property_hooks.rs`),
  currently wired only for ordinary function/method declarations.

## Phase 100 — Opt-in algorithmic-complexity heuristic (§AF)

Flag a linear `in_array()`/`array_search()`/`array_key_exists()` lookup
into a pre-built, unbounded array from inside an unrelated loop
(`loop.quadraticLookup`) — explicitly heuristic and opt-in, with a
disclosed false-positive caveat for small-but-unbounded-at-runtime
haystacks. PHPStan's entire `Rules/` catalog reasons about types/
existence/soundness, never runtime execution cost — no rule concept
like this exists anywhere. **Effort: medium.**

---

# Part 4 (BEYOND_PHPSTAN): Ecosystem — `phpstan-strict-rules` ports and Won't-port corrections

Grounded directly against the vendored `phpstan-strict-rules` submodule.

## Phase 101 — Ecosystem checks ported from `phpstan-strict-rules` (§E)

- **E2 (medium, gate on revisiting two prior false positives first).**
  `array_filter()` without an explicit callback for an ambiguous
  element type — phpstan-rs already declined two siblings
  (`arrayFilter.same`/`.alwaysEmpty`) after corpus false positives
  traced to this project's own flow-sensitive variable-type-tracking
  gaps, not the checks themselves; this one risks the identical
  truthy/falsy union-splitting judgment call.

---

# Part 5 (BEYOND_PHPSTAN): LSP / editor-native capabilities

Real PHPStan ships no first-party LSP or interactive query API at all —
`FileAnalyser::analyseFile` is its sole entry point, always a fresh
whole-file CLI invocation. phpstan-rs's LSP server already keeps a
resident, incrementally-updated `Project`/`Registry` — every phase below
is a gap in already-built plumbing. (See also `VSCODE_EXTENSION_PLAN.md`
for the LSP server's own trustworthiness/concurrency/protocol
hardening, a separate concern from net-new capabilities like these.)

## Phase 105 — Finer-grained incremental re-check (§G)

- **G1 (large).** Sub-file (per-function) real-time re-check on
  `didChange` — every edit today unconditionally re-parses the whole
  file and re-runs the full registry-rebuild sequence, even for an edit
  entirely inside one function body touching no declaration.
  `run_probe_scope`'s existing `ProbeScope::Function` arm already
  proves the isolated-function-check mechanism works for hover; detect
  a declaration-shape-preserving edit and reuse it, falling back to full
  `update_file` on save or a declaration-shaped edit.
- **G2 (large).** Per-symbol signature-shape fingerprinting to skip
  re-checking unaffected dependents — `Project::update_file` seeds
  `changed_symbols` with every name a file declares regardless of
  whether its externally-visible shape actually changed; fingerprint
  each symbol's signature (excluding body) and seed only genuinely-
  changed ones, narrowing `transitive_affected_files`'s existing BFS.
  Real PHPStan's own invalidation edge is stored per-*file*, not
  per-declaration, so it structurally cannot reach this granularity
  without a rewrite — this project's own code comment already names it
  as future work.

## Phase 107 — Whole-file/whole-project symbol lists (§AH)

- **AH3 (medium).** `textDocument/codeLens` reference-count annotations
  above declarations — aggregating the already-built Tier-1/Tier-2
  use-site index (`project::use_sites`, built for `textDocument/
  references`/rename) into per-declaration counts, plus new `CodeLens`
  response wiring.

## Phase 109 — `willRenameFiles` (§AO)

- **AO1 (large, depends on Phase 114's R1).** `workspace/
  willRenameFiles` — PSR-4-consistent rename-file refactoring on a
  file-explorer move: derive the old FQN (reverse `DeclLocations`
  lookup) and the new FQN the new path implies (mirror-image
  arithmetic of R1's path-to-FQN), rewrite the declaration plus every
  project-wide use-site via the already-built `project::use_sites`
  index (no longer a second dependency — it's the same index
  `textDocument/references`/rename already consume).

## Phase 110 — Diagnostic explainability: derivation traces (§T)

Real PHPStan's only introspection tool, `PHPStan\dumpType()`, computes
one final type per call with no accumulated reasoning kept and requires
editing source + a full CLI re-run.

- **T1 (large).** Interactive branch-narrowing derivation trace — an
  opt-in trace mode on `apply_narrowing` pushing (condition, branch,
  mechanism, before/after type) records into a new side-channel modeled
  on the existing `provenance: ProvenanceMap` field; `merge_scopes`
  already shows how such a channel composes across branch joins.
  Surfaced via the same one-shot instrumented-recheck pattern
  `probe_span`/`run_probe_scope` already proves for hover.
- **T2 (medium).** Per-pass loop-convergence widening trace — each
  pre-final pass of `converge_loop_body` is fully materialized in
  memory immediately before being discarded purely to keep iteration
  bounded; capture a before/after diff per pass under the same trace
  mode instead of truncating it silently. Needs its own step kind
  (iteration-indexed, no controlling condition).

---

# Part 6 (BEYOND_PHPSTAN): Codemods

Four fixes attaching to an existing (or Phase 101 E3-proposed)
diagnostic via the already-built `textDocument/codeAction` wiring
(`Diagnostic.fix: Option<DiagnosticFix>`, `Server::code_actions`) — the
wiring itself has zero production call sites populating a real `fix`
yet, which is exactly what each item below does. Real PHPStan's entire
autofix surface is used by only 4 rule files project-wide.

## Phase 111 — Codemods attached to diagnostics phpstan-rs already ships (§I)

- **I1 (small).** Byte-exact same-length splice for every `*.nameCase`
  mismatch (`class.nameCase`, and Phase 101's E4 once built) —
  identifier casing changes never change byte count, the safest
  codemod shape possible.
- **I2 (small).** Delete-attribute-group codemod for the already-
  shipped invalid-`#[Override]` diagnostic — a pure span deletion,
  strictly smaller/safer than real PHPStan's own whole-node-reprint
  approach to the identical rule.
- **I3 (medium).** Whole-declaration deletion for the zero-usage subset
  of unused-private-member diagnostics (`*.unused`, not the
  `neverRead`/`onlyWritten` siblings, which must NOT be auto-deleted).
  Must walk backward past any preceding doc-comment before deleting.
- **I4 (small).** `constructor.missingParentCall` (Phase 101's E3, now
  built) is ready to consume. Conditional single-line
  `parent::__construct();` insertion — only when the resolved parent
  constructor has zero required params; decline rather than guess
  otherwise.

---

# Part 7 (BEYOND_PHPSTAN): Config/strictness composability

Real PHPStan's parameters resolve exactly once into one frozen DI
container for the whole run; a rule is on-or-off for literally every
file. phpstan-rs has no such container and no parallel-worker model to
keep consistent.

## Phase 112 — Config/strictness composability (§J)

- **J1 (medium).** Per-directory additive opt-in checks — resolve a
  small path-pattern list once at load time into a per-file-path
  override table, consulted before falling back to the project-wide
  bool. No new dispatch mechanism needed.
- **J2 (large).** Per-directory strictness tiers in one run, with
  cross-directory calls still checked consistently — real PHPStan can
  only do this via two entirely separate invocations with no shared
  registry and no cross-directory soundness at all. Generalizes J1's
  override table from on/off to named tiers.

---

# Part 8 (BEYOND_PHPSTAN): Composer/dependency hygiene

Eight distinct axes, each keyed off a different `composer.json` section.
PHPStan's `Rules/` catalog has no package-declaration-aware check of any
kind — structural, since it has no persistent whole-program "which
package provides this symbol" index. phpstan-rs already has most
underlying pieces built for other reasons (`composer::
find_composer_package_name`, `detect_phpstan_dependency`,
`deps::referenced_names`, `DeclLocations` — several items below
originally noted `DeclLocations` needing to be opened up for batch
`check-project` runs, not just its LSP-only `retain_source` gate; that,
plus the whole-program diagnostic-emission pipeline every phase below
also needs, are both now built — see Part 1's own shared-prerequisite
note above Phase 85).

## Phase 113 — Composer declared-vs-used dependency hygiene (§K)

Three diagnostics keyed off resolving "which composer package owns this
symbol": `unused.composerDependency` (a `require` entry contributing
zero referenced symbols), `undeclared.composerDependency` (first-party
code resolves into a vendor package that's only present transitively,
not actually required — an unrelated `composer update` can silently
break the build), `misplaced.composerDependency` (a `require-dev`-only
package referenced from non-test application code, breaking
`composer install --no-dev`). **Effort: large.**

## Phase 114 — PSR-4 filesystem consistency (§R)

Real PHPStan's only PSR-4 awareness is consuming the mapping to *locate*
classes for reflection — it never validates location matches namespace.

- **R1 (medium).** `psr4.pathMismatch` — resolve each declared FQN's
  expected PSR-4 path and compare against its real declared file
  (`DeclLocations.classes`, now populated in batch runs too - see
  Part 1's own shared-prerequisite note above Phase 85). A file
  loading fine today only because Composer's dev autoloader falls back
  to directory scanning silently breaks under `composer dump-autoload
  --optimize`. (Phase 109's AO1 reuses this same prefix-map data in
  reverse.)
- **R2 (small, needs R1 first).** `psr4.caseMismatch` — case-
  insensitive-but-not-exact path match, via real on-disk `read_dir`
  casing (never `canonicalize`, which normalizes case and hides the
  bug). The classic "works on the author's case-insensitive laptop,
  breaks in Linux/Docker production" bug class — invisible to PHPStan
  since its own contributors/CI overwhelmingly run case-insensitive
  platforms too.

## Phase 115 — `misplaced.autoloadDevSymbol` (§AC)

Flag production code (resolving under `autoload.psr-4`) reaching a
symbol only declared under `autoload-dev.psr-4` — `composer
dump-autoload --no-dev` drops the dev root entirely, so the reference
fatals at runtime with zero warning today even though every dev-mode
install (what CI and every local run actually uses) resolves it fine.
Needs extending `composer.rs`'s existing parse to also read
`autoload-dev.psr-4` - `DeclLocations`'s own batch-mode prerequisite
(same as Phase 114's R1) is already built. **Effort: medium.**

## Phase 116 — `provide.unimplemented` (§AG)

For each `composer.json` `"provide"` entry matching one of the ~7
formally-specified PSR virtual packages (`psr/log-implementation` →
`Psr\Log\LoggerInterface`, etc.), verify via `Registry::is_subclass_of`
that some project-declared class actually implements the target
interface — a false claim any downstream `require`-ing package would
silently trust. Deliberately excludes the two PSR packages mapping to
several interfaces at once (ambiguous, v2 material). **Effort:
medium.**

## Phase 117 — `ext.notRequired` (§AK)

Flag a call to a stdlib function/class sourced from a specific
`vendor/phpstorm-stubs/<extension>/` directory when the project's own
`composer.json` declares neither `ext-<extension>` nor a covering
polyfill — the "works because the extension happens to be installed
locally" bug. The one real gap: no per-function "which extension" table
exists yet, though `build.rs`'s stub-collection pipeline already knows
and discards each file's extension-named parent directory at exactly
the point that data is needed. Must exclude always-compiled-in
extensions (`Core`, `SPL`, `Reflection`, ...). **Effort: large.**

## Phase 118 — `composerScript.notFound`/`notPublicStatic` (§AL)

Parse `composer.json`'s `"scripts"` map; for every `Class::method`-
shaped callable-string value (deliberately excluding shell-command
strings), resolve against the registry and flag a missing symbol or a
non-public/non-static target — Composer calls it via reflection-free
`call_user_func`, so either throws or silently no-ops at real
invocation time with zero static warning today. One honest gap:
`FunctionSig` has no `is_static` field yet (the raw `MethodDecl::
is_static` bool is already read and discarded one line away from where
`visibility` is threaded through, mirroring the already-completed
Phase 57 visibility rollout). **Effort: medium.**

## Phase 119 — `composer.platformPhpInconsistentWithRequire` (§AN)

Cross-check `config.platform.php` (a pinned PHP version overriding
Composer's own dependency-resolution assumption) against the project's
own `require.php` semver constraint for internal self-contradiction —
real PHPStan reads the platform field only as a fallback *input* to its
own analysis version, never validates it against anything else in the
same file. Needs a new small hand-rolled evaluator for Composer's
common constraint forms (`^X.Y`, `~X.Y`, `>=X.Y`, simple `||` groups) —
phpstan-rs has none today. **Effort: medium.**

## Phase 120 — Composer path-repository / monorepo workspace support (§S)

- **S1 (large).** Fix the symlink-escaping vendor-context filter (a
  real bug: `canonicalize`-then-`starts_with(vendor_dir)` always fails
  for a path-repository's `vendor/<pkg>` symlink, silently dropping the
  whole package from context), then read `composer.json`'s
  `repositories` array to auto-promote local `type: path` siblings from
  context-only into first-party analyzed files.
- **S2 (medium).** `check-project`'s vendor-context discovery is
  hardcoded to a single `cwd` root, even though the underlying
  multi-`dirs` capability already exists — for a real multi-package
  workspace, only whichever package happens to be `cwd` gets its
  vendor context discovered at all. Walk upward per entry in `dirs`
  (reusing `find_composer_package_name`'s pattern) and merge+dedup
  contexts.

---

# Part 9 (BEYOND_PHPSTAN): Type-system policy, baseline robustness, reverted-feature fixes, API diffing

## Phase 121 — Type-system representation policy lessons (§M)

Not missing features — four places real PHPStan's own source admits, in
its own comments, permanent BC debt from an early design decision.
phpstan-rs has no installed userbase to protect; each item is a
forward-looking policy to keep following, not a build task. **Effort:
small each, standing policy.**

- **M1.** Keep `ArrayShape`'s one in-band unsealed-marker representation
  — never add a nullable "ambiguous/legacy" tier alongside an explicit
  marker the way PHPStan's `ConstantArrayType::$unsealed` had to,
  forcing permanent parallel `legacyIsKeysSupersetOf`/`legacyMergeWith`
  code paths. Relevant once Phase 85's A3 territory (unsealed-shape
  superset/merge reasoning) is actually built.
- **M2.** Keep the `Type` layer a closed Rust `enum`, never a public
  trait third parties implement — PHPStan's `Type` interface still
  carries 7 permanently-`@deprecated`-but-unremovable methods because
  any implementor (including third-party extensions) must fully
  implement the interface forever.
- **M3.** Keep one canonical member order for `Union`/`Intersection`,
  never a `describe()`-only sorted cache alongside the canonical order —
  PHPStan's own comments admit an earlier version corrupted `getTypes()`
  by sorting in place inside `describe()`.
- **M4.** Never split builtin function signatures into a legacy-loose
  stub plus an opt-in stricter delta (real PHPStan ships parallel
  loose/strict stub files gated by `stricterFunctionMap`, defaulting
  off, purely for BC) — always encode the most precise known signature
  directly.

## Phase 122 — Baseline robustness (§N)

Real PHPStan's baseline is not actually unbounded; phpstan-rs's
currently is. Both concrete and fixable inside `src/baseline.rs`'s
existing data flow.

- **N1 (medium).** Bound baseline matches by recorded occurrence count
  — give `baseline::Entry` a `count` field so an occurrence beyond the
  originally-baselined count falls through as new. The count
  information already flows through `generate()` and is only discarded
  when `load()` collapses into a `HashSet`.
- **N2 (medium).** Opt-in identifier-anchored baseline match mode,
  surviving cosmetic message-text changes — `Diagnostic.identifier` is
  already stable-by-design; `baseline.rs`'s `Entry` just doesn't
  consult it yet as an alternate match key.
- **N4 (small, new from top-100 Packagist survey).** Per-identifier
  baseline file splitting — `shipmonk/phpstan-baseline-per-identifier`
  splits one monolithic baseline into separate files per diagnostic
  identifier (`baseline/argument.type.neon`, `baseline/return.type.neon`,
  etc.), making git diffs on baseline changes far smaller/more
  reviewable and letting a team pay down baseline debt one error
  category at a time. A pure file-organization/generation feature on
  top of `baseline.rs`'s existing data, no new matching logic needed.

## Phase 123 — Reattempting reverted features with real PHPStan's actual technique (§O)

Both items found the original *revert* (see ROADMAP.md's Reverted
section) was correct given what phpstan-rs had available, but that real
PHPStan solves the identical case with a specific technique phpstan-rs's
attempt never used.

- **O1 (medium).** Foreach's zero-iteration merge needs real PHPStan's
  *asymmetric*, certainty-downgrading join (`processAlwaysIterable
  ForeachScopeWithoutPollute` — keeps the post-loop type, only
  downgrades certainty, never re-unions the pre-loop type), not
  `merge_scopes`'s symmetric `union_types` — the exact self-mutating-
  iteratee idiom (`foreach ($items as $day => $data) { $items[$day][...]
  = ...; }`) both of phpstan-rs's prior attempts regressed on. Real
  PHPStan's `while`/`for` use the symmetric join fine — only `foreach`
  needs the asymmetric one, and phpstan-rs could adopt it
  unconditionally (PHPStan carries a legacy BC flag phpstan-rs doesn't
  need to).

  **UPDATE — the corpus-driven investigation below found the real fix
  is narrower than this item's own original framing, and it has now
  shipped; O1 itself (the general `converge_loop_body` merge algorithm)
  was never touched and stays open on its own remaining merits, if any
  survive after this.** Real-PHPStan A/B testing (a minimal `$counter =
  []; foreach ($keys as $k) { $counter[$k] = 1; } ... $counter[$someKey]
  ...` reproduction, confirmed via `\PHPStan\dumpType()` that real
  PHPStan's own post-loop type is the plain `array<string, 1>`, no
  `array{}` alternative at all) traced the actual defect one level
  *below* the loop-merge algorithm this item names: `union_types`
  itself never simplified a genuinely-*empty* `ArrayShape` (`array{}`)
  away when a general `Array(key, val)` member was already present in
  the same union — a plain, redundant-subtype absorption a sound type
  system should already do unconditionally (an empty array vacuously
  satisfies "every key/value is `key`/`val`-typed" for *any* `key`/
  `val`), matching what real PHPStan's own `TypeCombinator::union()`
  evidently already does. Fixed directly in `union_types`
  (`checker/type_union.rs`) — **`converge_loop_body`'s own merge
  algorithm needed no change at all**; the `array{}` branch is still
  computed and unioned in exactly as before, it just no longer survives
  as a separate, redundant member once a compatible general array is
  also present. This is a materially safer fix than O1's own proposed
  asymmetric-join redesign: it never touches the symmetric
  `LoopZeroIterations::Possible` merge at all, so the documented past
  regression (the self-mutating-iteratee idiom) and its own
  `subject_exempt` carve-out are both completely unaffected — confirmed
  by a dedicated regression test (`a_whole_variable_reassigned_inside_a_
  foreach_still_keeps_its_null_alternative`) pinning that a *non*-array
  redundant case (`Null` vs `String`, not a subtype relationship) still
  keeps its full union, matching real PHPStan's own `dumpType()` output
  for that shape too (`string|null`).

  Real corpus impact, verified via a full before/after scoped+unscoped
  Piwigo diff: **16 of 70** `argument.type`/`return.type` findings in
  the scoped run were fully removed (`ActivityRepository`,
  `HistoryRepository` ×4, `ImageRepository`, `CategoryService` ×2,
  `DerivativeImage`, `SiteUpdateSubController`, `CatPermPageRenderer`,
  `RatingPageRenderer`, `UserService::getBrowserLanguage` ×2, plus the
  two Reflection/backtrace sites the sibling fix below independently
  also fixes), and several more had their own `array{}`/`array{}|`
  alternative stripped without the whole finding disappearing (their
  own root cause is separate and unrelated - `SearchService` ×2,
  `SectionPopulator`). This also corrects ROADMAP.md's own "Known but
  not yet scheduled" entry blaming `isset()`/`array_key_exists()`
  dynamic-key narrowing for several of these same sites — narrowing was
  never the real cause (confirmed: an `array_key_exists()`-guarded read
  against an *explicitly*-PHPDoc'd `array<K,V>` parameter already
  infers correctly today, with zero gap) — the corruption happened one
  level upstream, in the union-simplification gap fixed here.

  **One real, disclosed trade-off found during verification, not a
  regression this fix introduced**: the scoped diff also showed 4 new
  findings (`ConfigurationSubController::processSizes` ×3,
  `openApiOperations`'s own `narrowingExpr()` call ×1) that a byte-for-
  byte isolated reproduction of each confirms were already latent,
  independent of this fix — the *by-ref foreach array-mutation still
  isn't tracked* gap (ROADMAP.md's own "Known but not yet scheduled",
  now updated with this second corpus site) and the `int|string` array-
  key-type-imprecision cluster were both simply reached for the first
  time once this fix removed an unrelated masking effect elsewhere in
  the same merge pipeline - the same "surfaced only once a separate fix
  stopped masking it" pattern this project's own history already has
  several instances of, not new bugs. **The `int|string` cluster is now
  fully root-caused** (a follow-up round, not this one): real PHPStan's
  own `Type[]` PHPDoc-shorthand resolution keys the result with a
  `BenevolentUnionType([Integer, String])`, not a plain `Union` - and
  `RuleLevelHelper::accepts()` treats a Benevolent union specially,
  silently skipping the strict per-member check a plain `Union` gets
  (confirmed directly: `dumpType()` even displays it distinctly,
  parenthesized). This project's `Type::unknown_array_key()` has no such
  marker, so every `Type[]`-shorthand-derived key type - and, separately,
  the array `+` operator's own key-type fallback below - gets treated as
  a strict `Union` instead, a confirmed source of real false positives
  (`BlockManager::apply`, `CategoryAvailableListController::__invoke`,
  `SpecStructureTest`, `openapi-generate-input-dto.php`'s
  `narrowingExpr()` - see ROADMAP.md's own "Known but not yet scheduled"
  entry for the full citation). Not built this round: a real fix needs a
  genuine `Union`-adjacent "benevolent" marker threaded through parsing,
  the `+` operator, and every `is_assignable`/`accepts_at_configured_
  level` call site that should respect it - correctly the same future
  phase this document's own Phase 84 entry already reserves for
  `checkBenevolentUnionTypes`, not a new one.
- **O2 (medium).** Register each anonymous class under a real, stable
  synthetic name (`file:line[:index]`-keyed, mirroring real PHPStan's
  own `AnonymousClassNameHelper`) with a full independent registry
  entry, instead of the discarded ancestor-aliasing shortcut — unlocks
  full member-set checking against every implemented interface/extended
  parent at once, not just one ad hoc directly-named fallback.

## Phase 124 — Whole-program public-API backward-compatibility diffing (§Q)

A new capability with no PHPStan-core or strict-rules equivalent at
all — its `analyseFile` entry point has no notion of comparing two
points in time; `Project`/`Registry` can trivially coexist twice in one
process since both are frozen, in-memory structures.

- **Q1 (large).** `phpstan-rs api-diff <old-ref> <new-ref>` — load two
  independent `Project`/`Registry` snapshots and structurally diff every
  public symbol's already-existing struct fields (removed member,
  narrowed parameter/widened return via `is_subtype`, narrowed
  visibility, newly `@internal`/`final`, removed interface, a
  non-final class gaining an abstract method). No reconstruction-via-
  reflection needed — every field this needs already exists as a plain
  struct field.
- **Q2 (medium, extends Q1).** PHPDoc-level generic/template-bound
  narrowing as its own break category — a tightened `@template T of
  Bound` or changed `@extends`/`@implements` type argument silently
  rejects existing callers even though the native PHP declaration (and
  therefore Reflection-visible signature) never changed at all,
  invisible to any tool working from native signatures alone.
  `ClassInfo::template_bounds`/`extends_args`/`implements_args` are
  already first-class, already-resolved fields on the same `ClassInfo`
  Q1 walks.

---

# ROADMAP.md's "Newly identified gaps" — triaged into phases 125-139

Source: a 20-agent parity audit against every real PHPStan `Rules/`
class (~420), confirmed against source, not recalled. Each phase below
is one of that audit's own category groupings; sub-bullets are its own
individual findings, lightly condensed. Full per-finding evidence
(file:line in both `vendor/phpstan-src` and this project's own `src/`)
lives in that audit's own journal, cited in ROADMAP.md's own header for
this section.

## Phase 125 — Classes (real-PHPStan `Rules/Classes/` gaps)

- `ImpossibleInstanceOfRule` — `instanceof.alwaysTrue`/`.alwaysFalse`/
  `.invalidExprType` when an `instanceof`'s own type resolves to a
  literal boolean or a non-string/object dynamic class-name expr;
  phpstan-rs's unreachable-code family deliberately excludes
  `instanceof` today.
- `NewStaticRule`/`NewStaticInAbstractClassStaticMethodRule` — "Unsafe
  usage of `new static()`" in a non-final class without a
  consistent-constructor guarantee, and the abstract-static-method
  variant.
- `InstantiationCallableRule` — `new Foo(...)` (first-class-callable
  syntax on `new`) is syntactically legal but a real semantic error;
  `callable.notSupported`.
- `InstantiationRule`'s remaining sub-checks — phpstan-rs's `new`
  checking only covers `new.interface`/`new.abstract`, missing several
  siblings.
- `ExistingClassesInClassImplementsRule` — phpstan-rs's `extends`/
  `implements`/trait-use check is existence-only, never confirms the
  target is the *right kind* of symbol (a class implementing an
  interface that's actually a trait, etc.).
- `InvalidPromotedPropertiesRule` — promotion legality (only inside
  `__construct`, never abstract, never variadic, PHP-8.0-gated, `final`
  promotion additionally gated).
- `ReadOnlyClassRule` — the PHP-version-gate data already exists
  (`PhpFeature::ReadOnlyClasses`/`ReadOnlyAnonymousClasses`,
  `php_version.rs`) but is never consumed by any checker call site.

## Phase 126 — Functions (`Rules/Functions/` gaps)

- `CallCallablesRule` (+ `FunctionCallableRule`'s dynamic-target half) —
  a dynamic call `$callable(...)`'s own non-callable/inaccessible-
  method/arity/param-type checking.
- `IncompatibleDefaultParameterTypeRule` family (functions, closures,
  arrow functions) — a parameter's default-value expression checked
  against its own declared type hint; phpstan-rs's AST carries
  `default: Option<Expr>` but nothing validates it anywhere.
- `ReturnNullsafeByRefRule` family — "nullsafe cannot be returned by
  reference" when a by-ref function/arrow-fn's return passes through
  `?->`; the existing `contains_nullsafe_access` helper is unused for
  this.
- `InvalidParameterNameRule`/`RedefinedParametersRule`/
  `VariadicParametersDeclarationRule` — a small family of level-0
  parameter-list sanity checks (superglobal/`$this` param name, a
  duplicate parameter name, a non-last variadic parameter) with zero
  phpstan-rs equivalent.
- `ParameterCastableToStringRule`/`...ToNumberRule` family — array-
  argument values passed to `implode`/`array_combine`/`sort`/etc.
  actually castable to string/number, since these implicitly cast at
  runtime.

## Phase 127 — Methods (`Rules/Methods/` gaps)

- `IncompatibleDefaultParameterTypeRule` (method half of Phase 126's
  same family).
- `MissingMagicSerializationMethodsRule` — a non-abstract `Serializable`
  class lacking `__serialize()`/`__unserialize()`.
- `AbstractMethodInNonAbstractClassRule`'s second half — a non-abstract,
  non-interface method with no body at all (real PHP's parser accepts
  this invalid syntax; PHPStan must catch it).
- `MethodCallableRule` — gates first-class-callable syntax
  (`$obj->method(...)`) behind a PHP-8.1 version check; phpstan-rs's own
  extensive `PhpFeature`/`php_version.rs` framework covers roughly a
  dozen similarly narrow gates but not this one.

## Phase 128 — Properties (`Rules/Properties/` gaps)

- `DefaultValueTypesAssignedToPropertiesRule` (level 3) — a typed
  property's own literal default checked against its declared type.
- `ExistingClassesInPropertiesRule`'s `property.trait`/two further
  identifiers — the `class.notFound` half is built (Phase 48); the
  "wrong kind of symbol" half isn't.
- `PropertiesInInterfaceRule`/`PropertyInClassRule` — PHP 8.4
  property-hook *declaration-site* legality (hooked/public/non-
  readonly/non-static/non-abstract/non-final for interfaces; the
  matching modifier-combination matrix for classes).
- `ReadOnlyPropertyRule`/`ReadOnlyByPhpDocPropertyRule` — `readonly`
  declaration legality (native type required, no default, not static,
  PHP-version-gated) for both the native and `@phpstan-readonly`/
  `@readonly` doc-tag forms.
- `InvalidCallablePropertyTypeRule` — `callable` as a property's native
  type is a real PHP fatal error; unchecked today.
- Five-rule cluster around PHP 8.4 write-only/read-only/asymmetric-
  visibility properties: assign-by-ref (plain/readonly-native/
  readonly-by-phpdoc), reading a write-only property, writing a
  not-writable one.

## Phase 129 — Comparison (`Rules/Comparison/` gaps)

- `StrictComparisonOfDifferentTypesRule` — a `===`/`!==` comparison
  whose outcome is provably constant (disjoint or provably-equal
  operand types).
- `ImpossibleInArrayHaystackFiniteTypesRule` — `in_array()`/
  `array_search()`/`array_keys()` where a needle's finite type can never
  match any constant-array-haystack value (level 4).
- `UsageOfVoidMatchExpressionRule` — using the result of a `match`
  whose every arm is a void-returning call; phpstan-rs's `Match`
  handling has no void-result-usage check at all.

## Phase 130 — PhpDoc (`Rules/PhpDoc/` gaps)

- `IncompatiblePhpDocTypeRule` family (param/property/property-hook/
  class-constant) — a PHPDoc type incompatible with its own native type
  hint; phpstan-rs silently reconciles/coerces instead of diagnosing.
  **Quick-win sub-slice from the top-100 Packagist survey**:
  `shipmonk/phpstan-rules`' `ForbidPhpDocNullabilityMismatchWithNative
  TypehintRule` covers just the *nullability* disagreement between a
  PHPDoc type and its native type hint — sharper-scoped and easier to
  implement than the full family; worth building first as an early
  win before tackling the general case.
- `IncompatibleSelfOutTypeRule` — `@phpstan-self-out` is entirely
  unimplemented (only in the typo-detection whitelist). (Phase 88's U4
  is the *inference* half of this same tag family, once real tag
  support exists.)
- `FunctionAssertRule`/`MethodAssertRule`'s deeper assertion-validity
  half — Phase 48.7 closed the plain class-existence slice
  (`class.notFound`); `assert.impossibleType`/`.alreadyNarrowedType`/
  `.unresolvableType`/`.trait`/`.unknownExpr` and generic/missing-
  typehint checks on the asserted type remain.
- `FunctionConditionalReturnTypeRule`/`MethodConditionalReturnTypeRule`
  — validating a conditional PHPDoc return type's own declaration
  (`conditionalType.subjectNotFound`/`.alwaysTrue`/`.alwaysFalse`).
- `InvalidPhpDocVarTagTypeRule`/`WrongVariableNameInVarTagRule`/
  `VarTagChangedExpressionTypeRule` — phpstan-rs's inline-`@var`
  checking is deliberately scoped to the plain-assignment case only;
  the much larger name-matching surface (foreach/static/global/return/
  misplaced-declaration variants) is unbuilt.

## Phase 131 — Generics (`Rules/Generics/` gaps)

- `MethodTagTemplateTypeCheck` — a `@method` tag's own inline template
  parameters (distinct from a real method's `@template`) aren't
  modeled at all, including the template-name-shadows-class-template
  check.
- `UsedTraitsRule`'s own remaining diagnostics — incompatible/
  unresolvable type on the `@use<...>` tag itself, an `@use` tag naming
  a trait the class doesn't actually use, and its one real behavioral
  difference from `@extends`/`@implements` (a call-site variance
  annotation is *never* valid inside `@use<...>`, unlike the other two)
  — Phase 54 already closed the bound/arity/variance half.

## Phase 132 — Arrays (`Rules/Arrays/` gaps)

- `IterableInForeachRule` — `foreach` over a provably non-iterable
  subject; phpstan-rs's `Foreach` handling never validates the subject
  type at all, just falls back to `Mixed`.
- `DeadForeachRule` — `foreach` over a provably always-empty array,
  level 4.
- `ArrayDestructuringRule` — `list()`/`[]` destructuring's own
  RHS-is-array-like and per-key-existence checks.
- `ArrayUnpackingRule`/`UnpackIterableInArrayRule` — `[...$arr]`'s
  PHP-8.1-string-key-support gating and non-iterable-spread checking;
  spread items (`item.spread`) aren't specially checked at all today.
- `OffsetAccessWithoutDimForReadingRule` — `$x[]` used in read position
  (a real PHP compile-time fatal), non-ignorable.
- `NonexistentOffsetInArrayDimFetchRule` — reading a nonexistent key
  across constant arrays/general arrays (opt-in)/shapes, plus indexing
  a non-offset-accessible type. (Ties into Phase 85's A3 sizing note.)
- `OffsetAccessAssignmentRule`/`OffsetAccessAssignOpRule` — an illegal
  key type for `$arr[$key] = $v`/compound-assign.
- `OffsetAccessValueAssignmentRule` — assigning a wrong-typed value into
  a typed array/object offset; `write_index_into` always unions the new
  value in rather than checking it against a pre-existing declared
  value type.

## Phase 133 — Constants (`Rules/Constants/` gaps)

- `ConstantRule` (level 1) — a bare undefined global-constant reference;
  phpstan-rs's `global_constant_type` falls back to `Mixed` for anything
  not in the table.
- `DynamicClassConstantFetchRule` — PHP 8.3's `Foo::{$expr}` dynamic
  class-constant fetch; `StaticAccess`'s AST has no way to represent a
  dynamic member name at all.
- `MagicConstantContextRule` — `__CLASS__`/`__TRAIT__`/`__FUNCTION__`/
  `__METHOD__`/`__NAMESPACE__` used outside their own context; blocked
  on phpstan-rs having no dedicated magic-constant `Expr` kind at all
  (already documented in `registry.rs`).
- `OverridingConstantRule` — class-constant override compatibility
  against a parent/interface (final override, visibility narrowing,
  native-type narrowing/dropping, PHPDoc-value-type covariance).
- `ValueAssignedToDefineRule`/`ValueAssignedToGlobalConstantRule` — a
  `define()`'d/`const`-declared global constant's value checked against
  a configured `dynamicConstantNames` type.
- `ValueAssignedToClassConstantRule`'s PHPDoc-`@var`-type half — the
  native-typed-constant half is built; an untyped `const` whose inline
  `@var` doesn't accept the literal value (`classConstant.phpDocType`)
  is explicitly skipped.

## Phase 134 — Variables (`Rules/Variables/` gaps)

- `AssignToByRefExprFromForeachRule` — assigning to (or property-
  fetching through) the same expression just bound as a `foreach ($x as
  &$expr)` reference — a well-known gotcha, unchecked.
- `CompactVariablesRule` — `compact()`'s own literal-string-argument
  variable names checked for definedness; phpstan-rs's
  `variable.undefined` only fires for direct `$var` reads today.
- `UnsetRule` — five distinct `unset(...)`-target diagnostics
  (undefined variable, illegal/nonexistent offset, readonly-property
  variants, hooked/possibly-hooked property).
- `VariableCloningRule` — `clone $expr` on a non-cloneable (non-object)
  type; phpstan-rs infers a type for `Clone` but performs zero
  cloneability validation.

## Phase 135 — Operators+Cast (`Rules/Operators/`, `Rules/Cast/` gaps)

- `BacktickRule` — the backtick shell-exec operator's PHP-8.5
  deprecation warning.
- `InvalidAssignVarRule` — nullsafe on an assignment target, nullsafe
  on the RHS of assign-by-ref, and any non-assignable LHS (a call
  result, a literal).
- `PipeOperatorRule` — PHP 8.5's `|>` pipe with a by-ref-first-parameter
  callable on its right side (the pipe always passes by-value).
- `DeprecatedCastRule` — the non-canonical cast spellings `(integer)`/
  `(boolean)`/`(double)`/`(binary)`, PHP 8.5-deprecated.
- `UnsetCastRule` — the `(unset)` cast is a hard PHP >= 8.0 removal,
  non-ignorable; phpstan-rs infers `Type::Null` for it but never checks
  the configured PHP version.
- `VoidCastRule` — PHP 8.5's `(void)` cast, both its own version-gate
  and its "only legal as a first-level statement" constraint.

## Phase 136 — Traits+InternalTag (`Rules/Traits/` gaps)

- `ConflictingTraitConstantsRule` — a class constant overriding one
  supplied by a used trait must keep the same visibility/final-ness/
  native type (and, for immediately-used traits, the same value).
- `ConstantsInTraitsRule` — constants-in-traits is PHP 8.2+; phpstan-rs
  already has the matching `PhpFeature::ConstantsInTraits` gate data
  but no call site consumes it.

## Phase 137 — Keywords+Debug (`Rules/Keywords/`, `Rules/Debug/` gaps)

- `ContinueBreakInLoopRule` — a `break`/`continue`'s numeric level
  resolving outside enough loop/switch nesting, or crossing a closure
  boundary first.
- `DeclareStrictTypesRule` — `declare(strict_types=...)`'s value must
  be literally `0`/`1`, and the declare must be the file's first
  statement.
- `GotoUndefinedLabelRule` — a `goto` targeting a label that doesn't
  exist in the function; `collect_goto_targets` today only feeds
  dead-code reachability, never validates target existence.
- `UnusedLabelRule` — a `label:` statement never targeted by any
  `goto` — the inverse of the above, same underlying data.
- `RequireFileExistsRule` — resolving `require`/`include`(`_once`) path
  expressions (including the `__DIR__ . 'x'` idiom) against cwd/
  include_path/the file's own directory.
- `{Dump,DumpNative,DumpPhpDoc}TypeRule`/`DebugScopeRule` — PHPStan's
  own ad hoc developer-debugging hooks (`\PHPStan\dumpType()` and
  siblings). (Phase 110's T1/T2 and the already-shipped hover-type
  lookup (ROADMAP's Phase 17/108-AM2 Done entries) are phpstan-rs's own,
  materially more capable, interactive answer to this same need — this
  entry stays here only as the parity-audit's own finding, not a
  separate build.)

## Phase 138 — Generators+Pure (`Rules/Generators/` gaps)

- `YieldFromTypeRule` — `generator.nonIterable` when the expression
  after `yield from` is definitely (or, with `reportMaybes`, possibly)
  not iterable. A standalone check needing only `exprType.isIterable()`,
  independent of the broader `Generator<K,V,TSend,TReturn>` generic-
  shape work.

## Phase 139 — EnumCases+Namespaces+Regexp+Ignore+Missing+Names+Types+Whitespace (mixed small gaps)

- `Rules/Whitespace/FileWhitespaceRule` — a leading UTF-8 BOM
  (`whitespace.bom`) and a whitespace-only closing-tag remnant as the
  file's last statement (`whitespace.fileEnd`).
- `Rules/Types/InvalidTypesInUnionRule` — rejecting `mixed`/`never`/
  `void` inside a union or nullable type on a param/return/property.
- `Rules/Ignore/IgnoreParseErrorRule` — a non-ignorable diagnostic for
  malformed `@phpstan-ignore` identifier-list syntax; phpstan-rs's own
  `ignore_comments.rs` deliberately swallows this with no diagnostic at
  all today (confirmed by its own test name).

---

# ROADMAP.md's "Known but not yet scheduled" — triaged into Phase 140

## Phase 140 — Long-tail precision gaps with a disclosed reason to stay deferred

Each sub-item already has a sharper, corpus-checked reason to stay
deferred (not a generic placeholder) — reproduced here only to give the
bucket a phase number, not because the reasoning changed.

- **140a.** `strtotime()`'s real accuracy and a raw SQL/DQL query's own
  result shape — both structurally infeasible without embedding a PHP
  runtime or a SQL parser respectively (confirmed real PHPStan needs
  exactly that too).
- **140b.** A long tail of smaller type-system precision gaps, each
  independently triaged: no per-location type-inference caching (a
  performance mechanism — see Phase 95's C1/C3 for the actual build
  items in this space; revisit only if profiling calls for it, not a
  missing diagnostic); cosmetic-only nested-`Intersection` flattening
  (zero confirmed false positive/negative); `static`/`$this` late
  binding has no attached calling class (a real architectural gap, no
  concrete corpus finding yet); no `BenevolentUnionType` (see Phase
  85's A4 for the actual build item; structurally not needed elsewhere
  today); `class-string<T>`'s own `instanceof`/sealed-hierarchy
  narrowing unmodeled (see Phase 85's A5); no general `TypeCombinator`
  intersect/remove/traverse combinator (a refactor-timing call, revisit
  once several bespoke traverse/map functions accumulate).
- **140c.** A `foreach ($arr as &$v) { $v['key'] = ...; }` by-reference
  mutation never updates `$arr`'s own tracked element shape — a real,
  buildable gap (real PHPStan does track this), surfaced only once a
  separate fix stopped masking it behind `Mixed`. (Phase 123's O1 is
  the closely-related foreach-merge fix; this is a distinct, narrower
  by-ref-mutation-tracking gap, not the same item.)
- **140d.** Dynamic-key `isset($arr[$dynamicKey])`/`array_key_exists(
  $dynamicKey, $arr)` never narrows the specific element's own value
  type — only a *literal* string key does today. A real, buildable
  feature (extending `array_value_type_for_key`'s literal-key-only
  mechanism to a narrowed dynamic key expression), confirmed live at
  four real corpus call sites, not attempted since it's a materially
  larger change (value-keyed, not name-keyed, narrowing) than a
  corpus-cleanup pass should take on — each individual finding it
  produces today is sound, not a false positive to chase down.

---

# Part 10 — Top-100 Packagist `phpstan-extension` survey (new phases)

Every one of the top 100 `phpstan-extension` packages on Packagist by
download count was cloned and read from real source, one at a time (see
`TOP100_RESEARCH_NOTES.md` for the full per-package notes this section
condenses). ~40 packages were single-framework/single-library glue
(Symfony, Doctrine, Laravel, Drupal, WordPress, TYPO3, CakePHP, Magento,
SilverStripe, Nette, CodeIgniter, GLPI, Maho, Laminas, and every single-
library mock/assert/type-extension package) — each needs that
framework's own runtime metadata model phpstan-rs has no equivalent of,
confirmed individually rather than assumed from the framework's name;
none produced a portable finding and none are re-listed phase-by-phase
below. A further ~10 were abandoned/deleted/duplicate/config-only with
nothing to port. The phases below are what remained: genuinely new,
portable ideas, plus several real, independent confirmations of ideas
already scoped above (folded in there instead of repeated here).

## Phase 141 — Declarative architecture/dependency-direction testing (new capability)

A whole new, well-established category with no precedent anywhere in
BEYOND_PHPSTAN.md/ROADMAP.md, confirmed by `carlosas/phpat` (1276
favers — among the highest real-developer-approval counts in the entire
top 100, not just a transitively-pulled-in download count). A fluent
DSL over composable selectors:

```
PHPat::rule()
  ->classes(Selector::inNamespace('App\Domain'))
  ->shouldNotDependOn()
  ->classes(Selector::inNamespace('App\Infrastructure'))
```

- **Selectors** compose: `inNamespace`, `classname`, `extends`,
  `implements`, `isAbstract`/`isFinal`/`isInterface`/`isEnum`/
  `isReadonly`/`isException`/`isTrait`/`isAttribute`/
  `appliesAttribute`/`isStandardClass`, with `->excluding(...)`
  exemptions.
- **Relations**: `shouldDependOn`/`shouldNotDependOn`/`shouldImplement`/
  `shouldExtend`/`beFinal`/`beReadonly`/etc.
- **Effort: large** (a new rule-DSL layer), but the underlying
  mechanism — resolving every class's own `use`-import/type-hint/
  extends/implements references against a small set of configured
  namespace/selector rules, flagging a forbidden dependency direction —
  is a project-wide pass phpstan-rs's own closed-world, single-process
  Registry is arguably *better* suited to than real PHPStan itself (the
  same "whole-program fact PHPStan's architecture can't cheaply compute"
  theme Part 1 already establishes repeatedly). Real corpus relevance is
  plausible for Piwigo's own layered namespace structure — worth
  checking at implementation time.
- Genuinely distinct from `restricted_symbols` (per-symbol banning) and
  from the `@internal` engine (package-boundary enforcement) — namespace
  *layering direction* is a third, different axis.

## Phase 142 — Composer-manifest dependency-declaration hygiene (new capability)

Two independent real packages converge on the same underlying need:
validate that every class a file uses is actually reachable through
that *specific module's own* declared Composer dependencies, not merely
resolvable via Composer's flat, whole-project autoloader.

- **Shared prerequisite (medium).** Parse the project's own
  `composer.json` (`require`/`require-dev`) plus `vendor/composer/
  installed.json` (every installed package's own PSR-4 namespace
  mapping) into a class-FQN → owning-package index. (Phase 94's P2
  above needs the identical index for real Composer package-boundary
  `@internal` enforcement — share one implementation, don't duplicate.)
- **142a (medium).** Shadow-dependency detection: a class is used, but
  its owning package is declared in neither `require` nor
  `require-dev` — only resolvable because some *other* direct
  dependency happens to also require it. Breaks the moment that
  incidental transitive path is removed. (`gosuperscript/phpstan-rules`'
  `RestrictImplicitDependencyUsage`, independently confirmed by
  `pb30/phpstan-composer-analysis`'s wrapped `shipmonk/composer-
  dependency-analyser` tool — real, two-package-validated demand.)
- **142b (small, needs a prod-path/dev-path file classification).**
  Dev-dependency-used-in-production-code: a class from a
  `require-dev`-only package is referenced from a file classified as
  "prod" — would break a `composer install --no-dev` deployment.
- **142c (small).** The inverse of 142b: a `require`-declared package
  is only ever referenced from dev/test paths — should be
  `require-dev` instead.
- **142d (small).** Unused-dependency detection: a declared package
  (either section) has zero real usage anywhere in scanned paths —
  dependency-bloat, safe to remove.
- 142b-142d all come from `pb30/phpstan-composer-analysis` alone;
  142a is the one both packages independently confirm.

## Phase 143 — Generalized "ban this construct/keyword/type" mechanism

A small, self-contained AST-shape-match mechanism — deliberately
simpler than ROADMAP's already-declined `spaze` allow/disallow-in DSL
(Phase 67), because none of these need call-site-scope-path/attribute-
context matching, only "does this bare AST-node-kind/type appear at
all, opt-in per rule." Real, independently popular (`ergebnis/
phpstan-rules`, republished/forked as `localheinz/phpstan-rules` —
one of the richest single packages surveyed):

- **143a.** Ban a bare language construct outright, config/opt-in per
  rule: `eval`, `isset()`, `compact()`, the `@` error-suppression
  operator, `switch`, PHP 8 named arguments, assignment-by-reference
  (`=&`) and by-ref function/method/closure parameters/returns, the
  short ternary `?:`, loose `==`/`!=` comparison, implicit
  array-append-key creation (`$arr[] = x`), `empty()`, backticks,
  `@phpstan-ignore`/`@phpstan-ignore-line` comments (forces fixing real
  issues instead of suppressing — an "eat your own dog food" policy
  check), and direct `$_GET`/`$_POST`/etc. superglobal access (forces a
  request-wrapper abstraction). Sources: `ergebnis/phpstan-rules`,
  `phpstan/phpstan-strict-rules`' `Disallowed*`/`NoSuperglobalsRule`
  family (`thecodingmachine/phpstan-strict-rules`).
- **143b.** Ban a bare *type* outright: `roave/no-floaters`' float/
  double ban (property types, param/return signatures, variable
  assignments) — avoid floating-point in money/precision-critical
  domains. A distinct "ban this native type" sub-family from 143a's
  "ban this construct."
- **143c.** Ban dynamic member-name-expression access entirely (opt-in
  strictness, not a resolution problem): `$$foo` variable-variables,
  `$obj->$memberNameExpr`/`$class::$memberNameExpr` where the member
  name is a runtime expression rather than a literal — these already
  collapse to `Mixed` in phpstan-rs's checker; this rule only needs to
  flag the *syntactic shape*, not resolve anything
  (`phpstan/phpstan-strict-rules`' `VariableVariablesRule`/`Variable*`
  family, duplicated independently in `symplify/phpstan-rules`'
  `NoDynamicNameRule`).
- **143d (curated preset, not new architecture).** A ready-made config
  preset banning 12 byte-oriented string functions (`strlen`/`substr`/
  `strtolower`/`strpos`/`stripos`/`strstr`/etc.) in favor of their
  `mb_*`-prefixed multibyte-safe counterparts — a real, common
  UTF-8-correctness bug class (`korbeil/phpstan-generic-rules`). Ships
  as a documented example config for `restricted_symbols`
  (Phase 67), not a new mechanism.
- **Effort: medium** overall — one AST-node-kind/type-kind matcher plus
  a config table, reused across all four sub-families.

## Phase 144 — "Closed by default" class shape + generalized parent-hook-call requirement

- **144a (small).** Opinionated "closed by default" class-shape bundle:
  every class `final` unless `abstract`; every member `private` unless
  the class is `final` (no subclass exists to protect from); concrete
  methods inside an `abstract` class themselves `final` (only
  overridable via the class's own abstract methods) — `ergebnis/
  phpstan-rules`' `FinalRule`/`PrivateInFinalClassRule`/
  `FinalInAbstractClassRule`. Each a trivial class/member-shape check.
- **144b (medium, generalizes and supersedes Phase 101's E3 scope).**
  A fully generic "must invoke `parent::method()`" mechanism —
  config-driven `{className, methodName, invocation: first|last|any|
  never, hasContent: yes|no|maybe}` entries, walking up to the real
  declaring parent class via reflection (not requiring an exact
  class-name match). Ship with sensible defaults for PHPUnit's own
  `setUp`/`tearDown`/`setUpBeforeClass`/`tearDownAfterClass`/
  `assertPreConditions`/`assertPostConditions` and Codeception's
  equivalent hooks — real, concrete value even with zero project
  config. This is the canonical, best-designed version of the same
  underlying idea independently reinvented three more times in the
  survey: `phpstan/phpstan-phpunit`'s `ShouldCallParentMethodsRule`
  (PHPUnit-only), `cambis/silverstan`'s
  `RequireParentCallInOverridenMethodRule` (SilverStripe-only,
  first/anywhere only, no last/never), and BEYOND_PHPSTAN's own E3
  (`parent::__construct()` only). Build against `ergebnis`'s shape
  directly — it strictly subsumes all three narrower versions,
  including E3 itself (a `{className: null (any), methodName:
  '__construct', invocation: any}`-style entry, or keep E3 as its own
  cheap special case if the generic form's config surface isn't worth
  it yet).

## Phase 145 — Extended access-control primitives (alternative to Phase 67's declined DSL)

`dave-liddament/phpstan-php-language-extensions` ships a materially
*simpler*, attribute-based design for the same underlying need Phase
67's declined `spaze` allow/disallow-in DSL was trying to solve
generically — worth building this instead when that need resurfaces,
since it needs no path/context-matching engine at all:

- **145a.** `#[Friend(AllowedCaller::class)]` on a method/class —
  explicit whitelist of which *other* specific classes may call it (the
  classic C++ `friend` concept via attributes). The whitelist lives on
  the target itself, not in a separate global config file.
- **145b.** Namespace-level sibling of 145a — declare a whole namespace/
  class visible only from specific other namespaces via attribute.
- **145c.** `#[TestOnly]`-style marker on a method, flagging *production*
  code that calls it — the inverse framing of Phase 115's autoload-
  dev-symbol-reachability idea, keyed on an explicit author-written
  marker instead of `composer.json`'s PSR-4 split.
- **145d.** Restrict a trait to only be `use`d by a whitelisted set of
  classes — a novel, narrow access-control primitive with no existing
  phpstan-rs equivalent.
- **145e.** Composer-package-boundary visibility enforcement — directly
  overlaps Phase 94's P2 above; share one implementation.
- **Effort: medium** — an attribute-reading pass plus a caller-identity
  check at each of the relevant call/instantiate/extend sites, reusing
  the same call-site inventory the `@internal` engine (Phase 67) and
  `restricted_symbols` already walk.

## Phase 146 — Pluggable assertion/mock-narrowing tables + PHPUnit native mock-builder hygiene

- **146a (medium).** Generalize phpstan-rs's existing
  `apply_phpunit_assertion_narrowing` mechanism into a **pluggable
  table design** rather than one hardcoded library. Confirmed by real,
  independent demand across at least 6 separate assertion/mock
  libraries surveyed, each needing only a new table entry once the
  mechanism is pluggable: `webmozart/assert` (~64 static methods —
  `string`/`nullOr`-prefixed/`all`-prefixed variants of each basic
  type/shape check; genuinely likely in Piwigo's own transitive
  dependency tree, worth checking first), `beberlei/assert` (same
  fluent shape), `azjezz/psl` (PHP Standard Library), Mockery,
  Prophecy, Nette Tester's `Assert::*`, and `eloquent/phony`'s
  MockBuilder/InstanceHandle fluent API.
- **146b (medium, new family, entirely generic, no new mechanism
  needed).** PHPUnit-native mock-builder-API misuse detection —
  distinct from 146a (this catches mock-*configuration* mistakes, not
  assertion-type narrowing), confirmed by `rector/mockstan`'s rich,
  fully generic rule set (needs only phpstan-rs's existing test-class
  detection + method-name matching):
  - `$this->expects($this->any())` is ambiguous — require an explicit
    count or a stub instead.
  - `->method('x')->with(...)` without a preceding `->expects(...)`
    call is a stub, and PHPUnit's own `with()` on a stub is silently a
    no-op — a real, easy-to-miss test bug (the assertion looks like
    it's checking arguments but never does).
  - Combining `willReturnOnConsecutiveCalls()` and
    `willReturnCallback()` on the same mock is redundant/confusing.
  - A property typed as a union/intersection mixing `MockObject` with a
    real class is an ambiguous smell.
  - A mocked method configured via `->method(...)` needs an explicit
    `->expects(...)` count.
  - `$this->atLeast(0)` is meaningless (matches any call count
    including zero) — require ≥ 1.
  - A test consisting only of mocked properties with no real property
    is suspicious — it isn't testing anything real.
  - Config-driven ban on mocking specific classes — same shape as
    `restricted_symbols`, applied to mock-target class names.

## Phase 147 — Exception-hygiene family (consolidates 3 independent confirmations)

Entirely generic, zero framework dependency, confirmed independently by
`nette/nette` (RethrowExceptionRule), `thecodingmachine/phpstan-strict-
rules`, and `shipmonk/phpstan-rules` — three separate real tools
converging on the same small set of checks is a strong signal this is
real, widely-valued, and cheap:

- **147a (small, easy, high value).** Inside a `catch` block, a newly
  thrown *wrapping* exception must pass the caught variable as its own
  `$previous` constructor argument somewhere — otherwise the original
  stack trace/cause is silently lost. Walk a catch body's own
  `throw new Exception(...)` sites, check the caught variable appears
  in the constructor call's own args.
- **147b (small).** Catching the base `\Exception`/`\RuntimeException`/
  `\Throwable` requires an unconditional rethrow somewhere in the catch
  body, unless deliberately building an exception handler — the
  unconditional sibling of Phase 88 U2's Nette-rethrow consumer (which
  only fires when a *more specific* exception was thrown and
  swallowed).
## Phase 148 — "No mixed member access" + null-coercion-safety + enum-exhaustiveness-enforcement families

Confirmed independently by `rector/type-perfect` and `shipmonk/
phpstan-rules` (same "no mixed" idea reinvented twice) — entirely
generic, zero framework dependency:

- **148a (small, 2x confirmed).** Opt-in ban on calling a method or
  accessing a property on a `mixed`-typed value at all, forcing
  explicit narrowing before any member access.
- **148b (small).** Ban `isset()`/`empty()`/array-index syntax
  targeting a plain object (each behaves oddly/silently on a plain
  object without `ArrayAccess`/magic methods).
- **148c (novel family, small each).** Ban a possibly-null value from
  silently participating in string interpolation (renders as `""`), a
  binary operator, or a compound assignment (`+=` etc.) — PHP's silent
  null-to-empty-string/zero coercion is a real, common bug source.
- **148d (novel, 3x-confirmed-independently-valuable, medium).** A
  `match` over an enum subject with a `default` arm defeats PHPStan's
  own exhaustiveness benefit — when a new case is added later, the
  default arm silently swallows it instead of surfacing a missing-arm
  error. Pair with requiring `match` (not `switch`/if-chains) when
  branching over an enum value at all, so exhaustiveness checking
  actually applies.

## Phase 149 — Native-typehint-preference + arithmetic/comparison-operand strictness families

- **149a (small each, easy).** Require a *native* type hint (not just a
  PHPDoc-only one) wherever PHP's own grammar could already express it,
  for return types and closure params — closes real precision gaps
  PHPDoc-only typing leaves open to drift.
- **149b (medium, real 3rd-independent-confirmation family, from
  `phpstan/phpstan-strict-rules` §5 + `shipmonk/phpstan-rules` +
  `voku/phpstan-rules`).** Ban PHP's automatic numeric-string coercion
  in arithmetic/binary operators entirely when strict — requires
  genuinely numeric operands for `+`/`-`/`*`/`/`/`%`/`**` and unary
  `+`/`-`/`++`/`--` (phpstan-strict-rules' 12-operator
  `OperatorRuleHelper` family); restrict `<`/`>`/`<=`/`>=` to genuinely
  comparable type pairs (`shipmonk`'s
  `AllowComparingOnlyComparableTypesRule`); ban a string operand
  combined with a non-string/non-mixed operand, and an array operand
  combined with a non-array/non-mixed operand, across most binary
  operators (`voku/phpstan-rules`' `ExtendedBinaryOpRule`/
  `ExtendedAssignOpRule`). One shared mechanism (a type-kind check on
  both operands of a `BinOp`/`AssignOp`/unary-op node), three
  independently-converged-on operand-mismatch policies.
- **149c (small, real, version-relevant).** `in_array()`/
  `array_search()` called without an explicit strict `true` third
  argument, where the needle/haystack-element types are a non-numeric-
  string vs. int/float pair — the exact PHP-7-to-PHP-8 loose-comparison
  hazard (`0 == "foo"` flipped from true to false in PHP 8). Fits
  naturally alongside phpstan-rs's existing `PhpFeature`-gated
  version-behavior-change checks (`voku/phpstan-rules`'
  `InArrayLooseComparisonRule`).
- **149d (small).** `ini_get`/`ini_set`/`ini_alter`/`ini_restore`/
  `get_cfg_var` called with a literal string naming a PHP ini directive
  deprecated as of a specific PHP version (hardcoded table:
  `mbstring.func_overload` since 7.2, `assert.active` since 8.3,
  `session.sid_length` since 8.4, etc.), checked against the project's
  configured PHP version — a tiny addition to the existing
  `PhpFeature`/`php_version.rs` framework (`phpstan/phpstan-
  deprecation-rules`).
- **149e (small each).** Sharp-edge gotcha checks: `iterator_to_array()`
  called without `$preserve_keys` explicit (silently loses/collides
  keys iterating a Generator with non-sequential/string keys);
  identical-class `===` comparison; calling `__construct()` directly as
  an ordinary method/static call (`$obj->__construct()` re-runs the
  constructor on an already-built object); a non-static closure
  explicitly capturing `$this` via `use ($this)` when it's already
  implicitly bound (pure redundancy); a `foreach`/`for` loop's own
  loop-variable shadowing a variable already holding a value from
  outside the loop (data-loss footgun); calling a statically-declared
  method via `->` instead of `::`; a `switch` case value whose type
  differs from the subject's (switch uses loose `==` internally).
- **149f (small, easy, generic).** `$a === $b` (`Identical`/
  `NotIdentical`) where `$a`'s static type is a class defining a native
  `equals($other)` method whose parameter type accepts `$b`'s type —
  the class author clearly intended value-equality semantics via
  `->equals()`, so `===` (reference identity) is almost always a bug
  (two logically-equal but distinct instances won't be `===`-equal).
  Fully generic, no framework dependency (`gosuperscript/phpstan-
  rules`).

## Phase 150 — "No magic numbers" + typed-class-constants families

- **150a (medium, novel, comprehensive, zero existing equivalent).** Ban
  a bare numeric literal (int/float, configurable exempt-numbers list
  e.g. `[0, 1, -1]`, optional numeric-string variant) across ~12
  distinct AST positions: assignment, ternary, switch case, return,
  match, logical/comparison/bitwise/arithmetic operators, default
  parameter value, property assignment, function argument. One shared
  base-class detection predicate (handling unary `+`/`-`, string-casts,
  optional numeric-string literals) reused across all position-specific
  checks — one shared helper plus N small call-site checks
  (`sidz/phpstan-rules`).
- **150b (small, easy).** Require every class constant carry an
  explicit PHP 8.3 native type declaration (`const int FOO = 5;`, not
  bare `const FOO = 5;`) — a natural sibling of phpstan-rs's own
  `missingType.*` family if class constants aren't already covered
  (`hiperdk/phpstan-rules`).

## Phase 151 — PSR-3 structured-logging hygiene family

Confirmed and deepened independently by two real packages
(`staabm/phpstan-psr3`, `struggle-for-php/sfp-phpstan-psr-log`) — same
shape as phpstan-rs's own already-built printf/sprintf placeholder-type
checking (`format_string.rs`), just PSR-3's `{key}`-brace syntax against
a literal context array's own keys instead of positional arguments.
Genuinely good real-corpus-value potential given how near-universal
PSR-3 logging is in any DI-framework-based PHP app:

- **151a (medium).** For any `Psr\Log\LoggerInterface` method call
  (`log`/`debug`/`info`/`notice`/`warning`/`error`/`critical`/`alert`/
  `emergency`), cross-check the message string's `{placeholder}` tokens
  against the literal keys of the passed context array — flag a
  placeholder with no matching key, and vice versa — plus checking
  context values used for interpolation are stringable.
- **151b (small).** Require the log message itself be a static/literal
  string, not a dynamically-concatenated one — dynamic values belong in
  the context array's own placeholders for structured-logging/log-
  aggregation friendliness.
- **151c (small).** Validate a log-level argument is a recognized PSR-3
  level constant.
- **151d (medium).** Validate the *structure* of the context array
  itself — e.g. requiring a `context['exception']` key actually be a
  `Throwable`.

## Phase 152 — "No implicit current-time dependence" family

A genuinely novel, valuable family: real, simple AST-shape/literal-
argument checks with zero framework dependency, useful for testable,
deterministic code (a function silently depending on wall-clock "now"
is a classic source of flaky tests and hard-to-reproduce bugs)
(`slam/phpstan-extensions`):

- Bans `time()`.
- Bans `strtotime('now'/'+1 day'/etc.)` — anything relative to the
  current instant rather than a fixed, explicit reference.
- Bans `new DateTime()`/`new DateTimeImmutable()` with no explicit
  fixed date/reference.
- Bans `date($format)` without its optional second `$timestamp`
  argument, which implicitly uses the current time.
- **Effort: small** overall — four independent literal-argument/
  call-shape checks sharing no state.

## Phase 153 — Actionable TODO-comment family

A genuinely novel, well-designed family turning `// TODO` comments into
live, checkable facts instead of text that rots forever
(`staabm/phpstan-todo-by`) — directly complements Phase 96's D5 (`@see`
tag validation) in spirit (checking a comment's own claim against
reality) but is a distinct, separately-valuable idea:

- **153a (small, pure/self-contained).** `// TODO: fix by 2024-01-01`
  flags once that date has passed — compare a parsed date literal
  against wall-clock "now," no network needed.
- **153b (small, pure/self-contained).** `// TODO: remove when php >=
  8.3` / `// TODO: ... when symfony/console >= 6.0` flags once the
  project's own configured PHP version, or a specific installed
  Composer package version (resolvable from `composer.lock`),
  satisfies the condition.
- **153c (medium, network-dependent — a real design departure worth
  flagging honestly).** `// TODO: JIRA-123` flags once that real ticket
  is closed, via a live HTTP call to Jira/GitHub Issues/YouTrack at
  analysis time — no existing phpstan-rs check does live I/O; scope
  and gate this separately from 153a/153b if pursued at all.

## Phase 154 — Output-format additions

Two small, self-contained `output::OutputFormat` additions, neither
touching any diagnostic logic — reads the already-resolved file's own
source text plus the diagnostic's already-known span/message:

- **154a (small).** A JUnit-XML `ErrorFormatter` — each diagnostic
  becomes a `<testcase>` (failure) in a `<testsuite>`, consumable
  natively by Jenkins/GitLab-CI/GitHub Actions test-report UI widgets.
  Add `Junit` as a 4th `OutputFormat` variant alongside the existing
  `Text`/`Json`/`Checkstyle` (`mavimo/phpstan-junit`).
- **154b (small).** A "friendly"/codeframe terminal formatter —
  renders each diagnostic with a syntax-highlighted source-code frame
  around the offending line, the same UX convention modern compilers
  use (rustc, ESLint's codeframe formatter) versus the terse default
  "file:line: message" one-liner (`yamadashy/phpstan-friendly-
  formatter`).

## Phase 155 — Small, cheap opinionated additions (bucket)

Each independent, self-contained, low-effort:

- **155a (small).** Flag a file declaring more than one top-level
  class/interface/trait/enum (a real PSR-1-adjacent convention many
  projects want) — a trivial file-level top-level-declaration count
  (`symplify/phpstan-rules`).
- **155b (small).** Require explicit visibility on class constants (PHP
  7.1+ allows it) (`prestashop/phpstan-prestashop`).

## Phase 156 — Whole-project type-coverage ratchet (§ reinforces Part 1's aggregate-fact theme)

A **project-wide type-coverage ratchet**: real PHPStan Collectors tally,
project-wide, what fraction of return types/param types/property
types/class constant types are actually declared (plus
`declare(strict_types=1)` file coverage), then a Rule fails the run if
the percentage is below a configured required threshold — a CI-friendly
"coverage must not regress" gate, not a per-site diagnostic
(`tomasvotruba/type-coverage`). phpstan-rs already computes every
individual missing-type-hint diagnostic this would aggregate
(`missingType.*`) — this is purely a project-wide count-and-threshold-
compare on top of data phpstan-rs already has per-site, the same
"whole-program aggregate from per-site data already computed" shape
Part 1 already establishes repeatedly. **Effort: medium.**

## Phase 157 — Database-migration hygiene family (low priority, no current corpus consumer)

A real, coherent, novel category (`bellangelo/phpstan-migration-rules`,
duplicated across Laravel-migration and Phinx variants) — but tied to a
specific migration framework's own schema-builder DSL, so still
framework-specific in the same sense as the DI-container/ORM packages
(needs that framework's own schema-builder call shape). Noted for
completeness, low priority given this project's own target corpus
(Piwigo is a plain PHP app, not built on Laravel-migrations/Phinx) and
would need a schema-builder-DSL-aware checker layer from scratch with no
existing precedent — not a natural next step without a concrete first
consumer:

- Require explicit charset/collation on new tables.
- Ban positional column placement (`->after('column')`) — a real
  zero-downtime-deployment/DB-engine-portability hazard.
- Ban SQL `ENUM` columns — well-known "hard to modify later"
  anti-pattern.
- One table per migration file.
- Ban raw SQL strings — force the schema-builder API for portability/
  reviewability.
- Opinionated forward-only-migration policy — discourage writing
  reversible `down()` methods.

---

Confirmed **no new finding, no action needed** from the remaining ~50
top-100 packages (each individually verified from real source, not
assumed from the package name): every Symfony/Doctrine/Laravel/Drupal/
WordPress/TYPO3/CakePHP/Magento/SilverStripe/Nette/CodeIgniter/GLPI/
Maho/Laminas/PHP-DI/PSR-11-container framework-integration package
(needs that framework's own runtime metadata model); every single-
library type-extension package with no independent second confirmation
(Mockery, Prophecy, Stripe SDK, Faker, Dibi, bonami/collections,
Deployer stubs); Blade/Twig/Latte template-compiler packages (needs a
template-DSL-to-PHP compiler plus a source-map — architecturally
interesting but no live successor found for two of the three, and the
third is Laravel-Blade-specific); Pest's own plugin (entirely built
around Pest's global-function DSL, no portable rule surface without
building Pest-DSL-detection infrastructure first); `#[Sealed]`-attribute
subtype validation (already covered by real PHPStan's own native
`@phpstan-sealed` tag, already scoped in Phase 67's design); a live-
network-calling Bitbucket "code insights" error formatter (a CI-vendor-
API-integration concern outside a local diagnostic formatter's natural
scope, unlike Phase 154's pure-text formats); WordPress-core API
version-gating (needs a WordPress-specific version-tagged symbol
database with no pure-PHP trigger to justify vendoring it);
`AutoMapper+`'s own BC-compat `ignoreErrors` config (not a checker
mechanism at all); and `phpstan/phpstan-php-parser`'s self-analysis
stub-precision package (not a rule/check package). Full per-package
detail and citations: `TOP100_RESEARCH_NOTES.md`.

## Phase 165 — Anonymous-class synthetic per-declaration-site identity

Reevaluated out of "Permanent design boundaries" - that entry's own
wording ("a materially bigger feature than a one-line type change, not
attempted here") already describes disclosed, buildable engineering
work, not an architectural impossibility, so it belongs in the backlog
proper rather than the permanent-boundary list.

`parser/classes.rs`'s `parse_anonymous_class_new` gives every anonymous
class in a program the identical literal name `"class@anonymous"`,
never indexed into the registry. A real fix needs each anonymous class
declaration site to register as its own synthetic class (a
deterministic name keyed by file+span, e.g.
`"class@anonymous:<file>:<span>"`, registered into `Registry::classes`
with its own real `extends`/`implements` edges exactly like a named
class) instead of only being parsed inline. Every consumer that
currently keys off a class's own literal name for identity/hierarchy
lookups then works unchanged - the fix is entirely in registration,
not in `is_assignable`/hierarchy-check logic, which already does the
right thing once each anonymous class has a real, unique registry
entry.

**Effort: large** — real registry-architecture work (new identity
scheme, indexing pass over every anonymous-class declaration site,
verifying no consumer assumes class names are only ever parsed from
top-level/nested named declarations) rather than a contained function
addition like the phases above.

## Phase 173 — Same-function parallel dynamically-keyed array correlation

Reevaluated a third time (the cross-variable-correlation cluster's own
"parallel dynamically-keyed arrays" item) after two prior over-
pessimistic passes on neighboring items in the same cluster - the
same-function case (both the populating loop and the later read in one
method, matching every corpus citation: `ActivityRepository::
findDailyActionCountsSince`, `HistoryRepository::findGroupedCountsSince`,
`ImageRepository::findExtensionBreakdown`) is bounded and buildable,
unlike the genuinely cross-function generalization (still declined -
see ROADMAP.md's own updated entry for exactly why).

Design mirrors `condition_aliases` (`checker.rs:104-177`), not
`AccessoryKind`: a correlation between two *named* variables needs
real invalidation-on-rebind, which a payload-carrying `AccessoryKind`
variant baked into a `Type` value has no story for (every existing
variant is a stateless unit variant, `types.rs:191-205`, precisely
because presence/absence is all `ArrayList`/`NonEmptyArray` ever need -
a named cross-reference is a different shape of fact entirely). Add a
new `Locals` side-table, `co_indexed: HashMap<String, HashSet<String>>`,
populated and purged the same way `condition_aliases` already is:

- Producer side: recognize a loop body where two (or more) array
  variables each receive an unconditional write
  (`$arr[$k] = ...;`/`$arr[$k] += ...;`) at the syntactically-identical
  key expression, at the same statement-list nesting depth with no
  intervening conditional/early-exit between them, every iteration -
  after the loop, record each pair's mutual correlation.
- Consumer side: a `foreach ($counterByKey as $k => ...)` whose subject
  carries a `co_indexed` entry - a later `$dateByKey[$k]` read inside
  that loop body, where `$k` is provably the loop's own key variable,
  skips the "possibly undefined index" diagnostic.
- Purge exactly like `condition_aliases`: rebinding either correlated
  variable (a fresh `declare`, not a same-shape `insert`) invalidates
  the pair.

**Effort: medium** — new side-table plus two new detection passes
(producer loop-shape recognition, consumer key-provenance matching);
larger than a single leaf-arm addition but bounded and self-contained,
reusing `condition_aliases`'s own invalidation discipline rather than
inventing a new one.

## Phase 175 — Closure capturing an outer branch's co-assignment fact

`RateService::rate`'s own closure - `$userAnonymous && $anonymousId
!== $savedAnonymousId` implying `$savedAnonymousId` was set to a real,
non-null value by the *outer* scope's own `if ($userAnonymous) {
$savedAnonymousId = ... ?? ...; }` before the closure even runs.

Phase 169's own mechanism (`Locals::branch_implied_facts`,
`checker.rs`/`checker/narrowing.rs`) is now built and its exact shape
settled, so this is independently assessable - and needs real, further
work beyond what 169 shipped, not just closure-capture plumbing on top
of it: `record_branch_implied_facts` only recognizes an equality
discriminant (`$subject === value`/`$subject == value`), matching the
`AuthService::authKeyLogin` shape 169 was built for. `RateService::
rate`'s own discriminant is a bare boolean truthiness test
(`if ($userAnonymous) { $savedAnonymousId = ...; }`), a different
recognized-shape gap on top of the closure-capture question - the
discriminant key would need to cover a bare `narrowing_key`-able
truthiness condition too, not just an equality comparison, before the
closure-capture half (surviving into a closure body reading the
captured variable, seeded from the outer scope's own `Locals` at the
closure literal's declaration point - see `checker/expr.rs`'s
`closure_locals`/`arrow_locals` seeding) is even reachable.

**Effort: medium** — one more discriminant-shape arm in
`record_branch_implied_facts`/its replay hook (bare truthiness, not
just equality), plus verifying the closure-capture seeding path
actually carries `branch_implied_facts` forward (`Locals::fresh_scope`
currently resets it to empty for every closure literal, matching
`condition_aliases`'s own precedent - may need to *not* reset it here
specifically, unlike the boolean-flag-scoped `condition_aliases`,
since a fact here is meant to survive scope boundaries).
