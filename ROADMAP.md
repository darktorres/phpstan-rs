# phpstan-rs roadmap

A from-scratch Rust PHP parser + type checker, aimed at growing toward
PHPStan feature parity. Phases 8+ are grounded in direct inspection of
`phpstan/phpstan-src` and `jetbrains/phpstorm-stubs` (not recalled
knowledge) and verified at real scale against Piwigo's own codebase
(`src/`, `tools/`) plus its `vendor/` tree, and - from Phase 22 onward -
against a frozen local copy (`/home/torres/piwigo17-frozen`,
`phpstan-rs.toml`-scoped) so before/after diffs aren't disturbed by the
live corpus's own file churn.

Status key:

- **DONE** - shipped and verified.
- **PARTIAL** - some of the phase's own scope shipped, some didn't.
- **PLANNED** - a real, detailed, source-grounded design exists; not
  started.
- **NOT STARTED** - a name and a rough idea only, no detailed plan yet.

This document has two halves. **Current status** is what's true right
now - read this before starting new work. **Completed phase history** is
how it got here, kept for lineage (a limitation or an open item is often
easier to judge with the phase that found it in view). Phase numbers are
cited in both halves so either can be cross-referenced from the other.

---

# Current status

## Headline numbers

- Test suite: 978 unit tests (`src/`) + 26 CLI integration tests + 3
  `main.rs` unit tests, all passing as of Phase 14.5's nineteenth round.
- Piwigo frozen-corpus diagnostic count - the primary false-positive-hunt
  metric (`phpstan-rs.toml`-scoped, `/home/torres/piwigo17-frozen`) -
  has fallen every round: 2312 (Phase 22 start) -> 1280 -> 1021 -> 792 ->
  732 -> 706 -> 580 -> 532 -> 533 -> 512 -> 508 -> 350 -> 295 -> 270 ->
  249 (Phase 35) -> 240 -> 239 (Phase 12) -> 211 (Phase 13) -> 1115 ->
  875 (Phase 14's closing round) -> 453 -> 434 -> 397 -> 296 -> 287 -> 285
  -> 253 -> 221 -> 219 -> 197 -> 188 -> 182 -> 158 -> 147 (Phase 14.5's
  first fourteen rounds) -> 144 (Phase 14.5's fifteenth round) -> 97
  (Phase 14.5's sixteenth round) -> 83 (Phase 14.5's seventeenth round)
  -> 81 (Phase 14.5's eighteenth round) -> **79 (current, Phase 14.5's
  nineteenth round)**. The 211 -> 1115
  jump was a
  real, confirmed *increase*, not a regression: `list<T>`/`non-empty-array<T>` now
  match real PHPStan's own strictness rather than this project's usual
  leniency default, a deliberate, explicitly confirmed choice given
  1575 first-party `list<` occurrences. Phase 14.5's eight rounds then
  closed most of the resulting gap: nearly all of it turned out to be
  this project's own type-inference gaps (spread array literals,
  `array_merge` including its own spread-argument form, PHP 8.4
  property hooks, `preg_match_all`'s per-group arrays, array-append
  widening, a first-class callable used as `array_map`'s own callback,
  two more real `array_filter()` extensions, Doctrine's own
  `Query::getResult()` typing as bare `mixed` - together the single most
  common accumulator/transform/repository idioms in this corpus, the
  `sort()` family's own dynamic type-specifying extensions, `$x !==
  []`/static-property narrowing, a `try`/`catch` scope-merge gap, a
  missing `Union` arm in the core array-type helpers, `glob()`/
  `explode()`'s own dynamic-return-type extensions, a mis-parsed
  conditional-return-type docblock, a real `instanceof` narrowing bug
  unrelated to `list<T>` itself, and - by far the single largest fix of
  any round - a method-override return-type check that compared
  docblock-refined types instead of native ones) rather than genuine
  corpus-side `@return list<T>` overclaims.
- Full Piwigo+`vendor/` corpus (~20,300 files, unscoped): last measured
  1,021 diagnostics as of Phase 24. Not re-measured every round since -
  the scoped frozen corpus above is the round-over-round regression
  signal now; re-run the unscoped check before trusting that number
  today.
- Test coverage: last measured 90.53% line / 97.46% function (`cargo
  llvm-cov`), during the Phase 6/9 audit. Stale - re-run before citing.

## Open phases

Every phase not listed here is DONE (see history below). This section was
rewritten wholesale after an exhaustive, file-by-file audit (no sampling)
of every file phpstan-rs could plausibly need to match under
`vendor/phpstan-src/src/Rules/` (463 files - 600 total minus 129
auto-generated `RuleErrors/RuleError<N>.php` builder-pattern permutation
classes and 8 trivial `RuleError`-family marker interfaces, both
confirmed by direct inspection to carry no unique check logic) and
`vendor/phpstan-src/src/Type/` (392 files, all in scope) - 855 files
total, each individually read and classified as implemented/partial/
missing/not_applicable against this project's own `src/`. The audit ran
as two chained Workflow-tool passes (`.claude/workflows/phpstan-parity.js`
for an initial sampled pass, `.claude/workflows/phpstan-full-parity.js`
for the exhaustive one whose findings this section is built from) with
the full raw per-file results, root-cause clustering, and phase-sequence
draft kept alongside the scripts in
`.claude/workflows/phpstan-full-parity.result.json`.

Of 855 files: 27 already have a real phpstan-rs equivalent
(`implemented`), roughly 130 are partially covered, about 490 are
genuinely missing, and about 208 are `not_applicable` - PHPStan's own
plugin-extension interfaces, DI-container wiring, playground/website-only
rules, BC-promise self-protection for its own `PHPStan\*` namespace, or
PHP-runtime/XML-shell-out techniques with no faithful Rust port. The
"Won't-port" reasoning for those is recorded in the Known limitations
section below (under Permanent design boundaries), so they aren't
rediscovered as false gaps later.

Phases below are numbered to double as a rough dependency/priority
order for anything added past Phase 46 - lower numbers are closer to
foundational, higher numbers are further out on the dependency graph or
lower real-world value. A compact tier summary:

```text
Foundational (do first):    47 -> 48 -> 38 -> 37 -> 49
Type-system structural:     50 -> 51 -> 52 -> 53 -> 54 -> 55
PHPDoc (existing, expanded): 36 -> 56
Exceptions (existing):       42
Purity (existing):           41
Rule-catalog batch:          57 -> 58 -> 59 -> 60 -> 61 -> 62 -> 43
                              -> 63 -> 64 -> 65 -> 66 -> 67 -> 68 -> 15
                              -> 69 -> 70 -> 71
Regex:                       46 (confirmed unchanged) || 72 (independent)
Stdlib batches:               73 (needs 50) -> 74 -> 75 -> 76 -> 77
                              -> 78 (low priority) -> 79 -> 80 -> 81
Opportunistic infra
(fold into a consumer,
 don't schedule alone):      82, 83, 84
Untouched by this audit
(tooling/API-consumer,
 not Rules/Type-shaped):     16, 17, 18, 19, 39, 40, 44, 45
```

**Phase 15 - Unreachable-code and constant-condition rule catalog
(PLANNED, refocused).** Originally a catch-all naming `DeadCode`,
`Comparison`, `Exceptions`, visibility, `continue N`'s level argument,
`Type::Array`'s missing key type, and `goto`/`label` CFG edges in one
paragraph. The exhaustive audit found each of those deserves its own
phase with its own much larger unlock list than a shared paragraph
implied, so this phase now narrowly covers what's left after splitting
the rest out: `DeadCode`/`Comparison`'s core rule catalog. Visibility
moved to Phase 57, `Type::Array`'s key type to Phase 50, and the
exception/throw-type model was already tracked separately at Phase 42
(`continue N`'s level argument and `goto`/`label` CFG edges remain
open, unclaimed by any phase below - see Known limitations).

Most of the underlying primitives this phase needs already exist and
are simply never surfaced as a diagnostic - sequence it as four
gradually-shippable pieces: (1) **unreachable-statement-after-
terminating-code** (`UnreachableStatementRule`) - `always_terminates()`
(`src/checker/control_flow.rs`) already exists and is exercised for
narrowing merges; this is essentially wiring plus a diagnostic call,
S effort, do first. (2) **constant-boolean-condition detection** -
`Type::True`/`Type::False` literal variants already exist; build one
shared "is this expression's type a constant bool -> report" helper and
wire it at the ~13 call sites real PHPStan covers separately
(`BooleanAndConstantConditionRule`, `BooleanNotConstantConditionRule`,
`BooleanOrConstantConditionRule`, `ConstantLooseComparisonRule`,
`DoWhileLoopConstantConditionRule`, `ElseIfConstantConditionRule`,
`IfConstantConditionRule`, `LogicalXorConstantConditionRule`,
`NumberComparisonOperatorsConstantConditionRule`,
`StrictComparisonOfDifferentTypesRule`,
`TernaryOperatorConstantConditionRule`,
`WhileLoopAlwaysFalseConditionRule`/`AlwaysTrueConditionRule`,
`UsageOfVoidMatchExpressionRule`) - `control_flow.rs`'s
`is_truthy_literal()`/`stmt_contains_break()` already detect the exact
`while(true)`-no-break shape PHPStan's own while-loop rules cover.
(3) **match/switch dead-arm and exhaustiveness** (`MatchExpressionRule`,
`SwitchConditionRule`) builds on (2)'s comparison primitive plus new
enum-exhaustiveness reasoning (benefits from, but doesn't strictly need,
Phase 79's enum-case singleton type). (4) **impossible-instanceof**
(`ImpossibleInstanceOfRule`) needs a paired always-true/false detector
layered on the existing `narrow_instanceof_true` narrowing logic (Phase
55 sharpens this further with subtracted-type support). Also folds in
`ImpossibleCheckTypeHelper`/`FunctionCallRule`/`MethodCallRule`/
`StaticMethodCallRule` (redundant `is_string($x)` where `$x` is already
known-`string`, etc.) and `ImpossibleInArrayHaystackFiniteTypesRule` -
these are genuinely part of Phase 55's narrowing-engine generalization
(the predicate-table extension), cross-referenced there rather than
duplicated. Size: S (item 1) -> L (items 2-3) -> L (item 4).

**Phase 16 - CI-friendly output formats (PLANNED).** No JSON/checkstyle/
junit/etc. output. Realistic starting subset: `json` (most universally
consumable) plus `checkstyle` or `junit`. Needs real source positions to
be useful beyond plain text - Phase 11 already shipped those, so this is
otherwise unblocked. Unaffected by the exhaustive audit (not a PHPStan
Rules/Type-shaped feature - PHPStan's own output formatters live outside
`src/Rules`/`src/Type`) beyond one dependency worth noting: Phase 47's
diagnostic-identifier work is a natural prerequisite for a JSON format
a CI tool would actually want to consume (a stable per-check identifier
per finding, not just a message string).

**Phase 17 - LSP hover/go-to-definition (PLANNED).** Needs a
position-keyed lookup index - today's `Registry`/`Project` lookups are
all name-keyed. `src/lsp.rs`'s existing hand-rolled JSON-RPC loop
(`run_loop`) is otherwise ready to extend with `textDocument/hover`/
`textDocument/definition`. Unaffected by the exhaustive audit (tooling,
not a Rules/Type-shaped feature).

**Phase 18 - Extension/plugin API (PLANNED).** Deliberately last and
least concrete - premature before Phases 12-15 give this project enough
built-in rules/type-extensions that a third party would actually want to
add to, rather than just use. Unrelated to Phase 40 below - Phase 40 is
about checking PHP code that consumes *real PHPStan's own* API, not
about phpstan-rs exposing one of its own. Unaffected by the exhaustive
audit - notably, the audit's own "won't-port" list is dominated by
PHPStan's *own* plugin-extension interfaces (`Dynamic*ReturnTypeExtension`,
`*TypeSpecifyingExtension`, etc.), confirming this project's own
match-arm-based architecture has no structural need for an equivalent
internal seam even once it grows an external-facing API.

**Phase 19 - Fuzz testing (NOT STARTED).** Name and rough intent only:
`arbitrary` + `cargo-fuzz` for the lexer/parser (a malformed-input crash
is a real, if narrow, risk the corpus-testing approach can't rule out),
`proptest` for `types.rs`'s subtyping logic (reflexivity, transitivity).
Both dev-only. Deliberately deferred behind Phases 11-15. Unaffected by
the exhaustive audit (dev-tooling, not a Rules/Type-shaped feature).

**Phase 36 - PHPDoc tag ecosystem expansion (PARTIAL).** `docblock.rs`'s
tag matcher was exact literal-string matching (`match tag { "param" =>
..., "return" => ..., ... }`) with no `@phpstan-`-prefix handling at
all, so the `@phpstan-param`/`@phpstan-var`/etc. spelling (a real,
documented convention - see the Doctrine-corpus finding in Known
limitations below) was silently dropped for every tag, including ones
already supported - **fixed in Phase 14.5's sixteenth round**, as a
second, override-priority pass over the same tag list, covering
`@phpstan-param`/`-return`/`-var`/`-template`/`-template-covariant`/
`-template-contravariant`/`-extends`/`-implements`. Every other tag
below remains open.

The exhaustive audit independently arrived at every one of this phase's
already-named sub-items across five separate audit categories,
confirming the scope was already correctly bounded, and adds concrete
unlocks per tag for tracking: `@method`/`@mixin` (synthesized into
`ClassInfo`'s `methods`/`properties` maps the same way enum
`cases()`/`value`/`name` already are, Phase 9) unlock `MethodTagCheck`/
`Rule`/`TraitRule`/`TraitUseRule`, `MixinCheck`/`Rule`/`TraitRule`/
`TraitUseRule`, and (nested-template sub-case) `MethodTagTemplateTypeCheck`/
`Rule`/`TraitRule`. The `@property`/`@property-read`/`@property-write`
*validation* half is a genuine bright spot found by the audit: parsing
and inference already ship (Phase 13 Finding D); only the diagnostic
layer (`PropertyTagCheck`/`Rule`/`TraitRule`/`TraitUseRule`) is missing -
cheapest item in the whole phase, do it first. `@phpstan-type`/
`@phpstan-import-type` (global config-level aliases, circular-alias
detection, duplicate-name checks) unlock `LocalTypeAliasesCheck`/`Rule`/
`TraitAliasesRule`/`TraitUseAliasesRule`, `TypeAlias`, `TypeAliasResolver`,
`UsefulTypeAliasResolver`. `@phpstan-require-extends`/`-implements`/
`@phpstan-sealed` (validated once per declaration, the same shape
`check_generic_bounds` already uses for `@extends`/`@implements` bounds,
Phase 10.5) unlock `RequireExtendsRule`/`Check`, `RequireImplementsRule`,
`RequireExtendsDefinitionClassRule`/`TraitRule`,
`RequireImplementsDefinitionClassRule`/`TraitRule`,
`SealedDefinitionClassRule`/`TraitRule`. `@phpstan-consistent-constructor`
unlocks `ConsistentConstructorHelper`, `ConsistentConstructorDeclarationRule`,
`ConsistentConstructorRule`, plus the related constructor-return-type
check (`ConstructorReturnTypeRule`) and `NewStaticRule`/
`NewStaticInAbstractClassStaticMethodRule` (both need Phase 48's
symbol-existence/self-static-distinction work too). `@phpstan-assert`/
`-if-true`/`-if-false` (a self-contained declaration-consistency check -
real PHPStan's own `AssertRuleHelper` doesn't touch narrowing either,
just validates the tag against the parameter's own declared type)
unlock `AssertRuleHelper`, `FunctionAssertRule`, `MethodAssertRule`.
`@phpstan-self-out` unlocks `IncompatibleSelfOutTypeRule`. `@readonly`
(the documentation-only convention, parallel to the native `is_readonly`
already tracked) unlocks `MissingReadOnlyByPhpDocPropertyAssignRule`,
`ReadOnlyByPhpDocPropertyAssignRule`, `ReadOnlyByPhpDocPropertyRule`.
Inline `@var` validation (Phase 35's `inline_var_annotation`, confirmed
to unconditionally overwrite the tracked type with no validation against
the real expression type) unlocks `VarTagChangedExpressionTypeRule`,
`VarTagTypeRuleHelper`, `WrongVariableNameInVarTagRule` - another bright
spot: the narrowing side-effect already works, only the error-reporting
half is missing. An enumerated `@phpstan-*` tag whitelist for typo
detection remains open too.

**New sub-items the audit surfaces that this phase's text didn't
previously name**: `@throws` tag parsing itself is *also* needed here
for a self-contained "is the tag's type a `Throwable` subtype" check
(`InvalidThrowsPhpDocValueRule`), distinct from the full throw-type
model tracked at Phase 42 - can ship early;
`@param-immediately-invoked-callable`/`-later-invoked-callable`
(`IncompatibleParamImmediatelyInvokedCallableRule`) and
`@param-closure-this` are standalone, S-effort items with no dependency
on anything else in this phase.

**Recommendation.** Split into two waves when executed: Wave A = tag
parsing only (cheap, unlocks synthesized-member inference the way
`@property` already benefits from it); Wave B = the validation/
diagnostic layer on top (per-tag rules above) - this mirrors the pattern
that already worked for `@property` in this codebase.

**Phase 37 - Attribute (`#[...]`) AST support (PLANNED).**
`#[Attribute(...)]` is lexed and thrown away entirely
(`Lexer::skip_whitespace_and_comments`) - real attribute content is
never available anywhere in this project, only two narrowly
special-cased stub-generation attributes get lexer-level text splicing
(`#[PhpStormStubsElementAvailable]`, `#[LanguageLevelTypeAware]`).
Needs a real lexer-level fix (emit tokens for `#[`, the name, `(`,
args, `,`, `]` instead of skipping the span, reusing the existing
string-aware bracket-depth-counting logic those two special cases
already use), a new `Attribute { name, args }` node parsed onto every
declaration site that can carry one (`ClassDecl`/`MethodDecl`/
`PropertyDecl`/`FunctionDecl`/`Param`/`ClassConst`/`EnumCase`, plus
closures/arrow functions), and checker-side attribute-class-existence
and target-validity checking, plus a `#[NoDiscard]` marker.

The exhaustive audit confirms this is the single most cross-cutting gap
in the whole codebase - it independently surfaced as an identical,
already-verified-real (`attr_groups`/`AttrGroup` confirmed absent from
`ast.rs`/`parser/classes.rs`) blocker in **six** separate audit
categories, and its consolidated unlock list spans nearly every
declaration kind: `AttributesCheck`, `ClassAttributesRule`,
`ClassConstantAttributesRule`, `NonClassAttributeClassRule`,
`TraitAttributeClassRule`, `TraitAttributesRule`, `PropertyAttributesRule`,
`PropertyHookAttributesRule`, `EnumCaseAttributesRule`,
`MethodAttributesRule`, `ParamAttributesRule`, `FunctionAttributesRule`,
`ClosureAttributesRule`, `ArrowFunctionAttributesRule`,
`ConstantAttributesRule`, `CallToMethodStatementWithNoDiscardRule`,
`CallToStaticMethodStatementWithNoDiscardRule`,
`CallToFunctionStatementWithNoDiscardRule`. Prerequisite for the
`#[Override]`/`#[ReturnTypeWillChange]` half of Phase 64's
override-compatibility rewrite.

**Phase 38 - Target-PHP-version configuration (PLANNED).** No
`php_version` setting exists anywhere (`config.rs`/CLI) - every "X only
supported on PHP Y.Z and later" portability check is unbuildable
without one. New `php_version: Option<(u8, u8)>`, following the exact
precedent of `strict_mixed_arrays` (Phase 11 Stage 5: `config.rs` field
-> CLI flag -> `Project` setter), plus a small `PhpFeature -> (u8, u8)`
table.

The exhaustive audit found this is already correctly scoped, but is a
**recurring blocker** across four other audit categories - worth
resequencing earlier than its number implies, since it gates the
version-gate half of `FinalConstantRule`, `NativeTypedClassConstantRule`,
`ConstantAttributesRule`, `DeprecatedCastRule`, `VoidCastRule`,
`BacktickRule`, `UnsetCastRule`, `ArrayUnpackingRule`, `ReadOnlyClassRule`,
`ConstantsInTraitsRule`, `InvalidPromotedPropertiesRule`,
`NoncapturingCatchRule`, and `ThrowExpressionRule`.

**Phase 39 - Deprecated-symbol usage warnings (PLANNED).**
`@deprecated` is parsed-past and discarded (confirmed - it only appears
in a `docblock.rs` test fixture, never consumed). New generalized
"restricted symbol usage" checker reusing `Registry::lookup_method`/
`lookup_property`'s already-centralized lookup machinery, switching the
predicate from "not found -> error" to "found, but deprecated -> warn,"
once `FunctionSig`/`PropertySig` gain a `deprecated: bool` field.
Unaffected by the exhaustive audit's own unlock lists (real PHPStan's
own `@deprecated` enforcement isn't organized as a dedicated `Rules/`
category the way the audited categories were - it's threaded through
individual rules elsewhere) - the scope above stands as originally
written.

**Phase 40 - PHPStan-API-usage checks (PLANNED).** Checks PHP code that
consumes `phpstan/phpstan` itself as a Composer library (third-party
PHPStan rule/extension authors) for references into `PHPStan\*` that
aren't marked `@api` - see the Phase 18 note above for why this is a
different feature. A pure namespace-prefix check (`PHPStan\*` minus
`PHPStan\PhpDocParser\`/`PHPStan\BetterReflection\`) plus a
vendor-directory file-path check; `@api`/`@api-do-not-implement`
recognized via Phase 36's own tag infrastructure; gated behind
detecting a `phpstan/extension-installer`/`phpstan/phpstan` dependency
in the analyzed project's own `composer.json` (mirrors the real
Pest-config-detection precedent, Phase 21). The exhaustive audit's own
`Api/*` findings (`ApiClassConstFetchRule`, `ApiMethodCallRule`, etc.,
11 items) were all classified `not_applicable` for phpstan-rs *itself*,
since they gate on referencing PHPStan's own `PHPStan\*` namespace, which
this project has none of. This phase remains the mirror-image, correct
feature: checking *other* PHP code that consumes real PHPStan's API,
not phpstan-rs protecting its own.

**Phase 41 - Purity / side-effect model (PLANNED).** No purity/
impurity tracking exists anywhere in `src/`. Needs an impure-point
detection pass - a body walk over every function/method/closure
classifying around 20 real operation kinds (echo, property writes,
method/function calls, `new`, `include`/`require`, `eval`, superglobal/
static/global access, ...; each a real category from PHPStan's own
`ImpurePoint` type, `certain` vs `maybe`) - plus `@phpstan-pure`/
`@phpstan-impure`/`@phpstan-immutable`/`@phpstan-all-methods-pure`/
`-impure`/`@pure-unless-callable-is-impure` tag parsing, and validating
a tagged-pure function against its own real impure points. The "unused
result of a pure call" rule family additionally needs *transitive*
purity across the call graph - a fixed-point pass over the
already-fully-indexed `Registry` (Phase 7).

The exhaustive audit found the same gap independently in two more
categories and confirms this phase's scope should absorb both rather
than spawning separate phases: a 12-item *function/method/constructor
purity tracking* cluster (`CallToConstructorStatementWithoutImpurePointsRule`,
`CallToFunctionStatementWithoutImpurePointsRule`,
`CallToMethodStatementWithoutImpurePointsRule`,
`CallToStaticMethodStatementWithoutImpurePointsRule`,
`ConstructorWithoutImpurePointsCollector`,
`FunctionWithoutImpurePointsCollector`, `MethodWithoutImpurePointsCollector`,
`PossiblyPureCallTransitivePurityResolver`,
`PossiblyPureFuncCallCollector`/`MethodCallCollector`/`NewCollector`/
`StaticCallCollector`), plus a *possibly-impure tip* decoration
(`PossiblyImpureTipHelper`, needs Phase 47's diagnostic-tip field); and
`FunctionPurityCheck`/`PureFunctionRule`/`PureMethodRule` (the
`@phpstan-pure`-tag-vs-actual-impure-points validation itself) plus a
**simpler, distinct** statement-level "no side effect at all" family
(`CallToConstructorStatementWithoutSideEffectsRule`,
`CallToMethodStatementWithoutSideEffectsRule`,
`CallToStaticMethodStatementWithoutSideEffectsRule`,
`CallToFunctionStatementWithoutSideEffectsRule`) that needs no purity
model at all - just "does this call have zero observable effect." Also
folds in `NoopRule` (bare no-op expression statement - not previously
named anywhere in this roadmap, same "does this statement do anything"
root question). **Recommendation.** Stage as: (1) `NoopRule` + the four
`WithoutSideEffectsRule`s [M effort, ships fast, no purity model
needed], (2) the full impure-point classification + Collector-based
transitive purity resolution [XL, the real architectural work - also
needs Phase 82's Collector/cross-file aggregation architecture for the
transitive half specifically].

**Phase 42 - Checked-exception / throw-type model (PLANNED).**
`FunctionSig` has no `throws` field at all - the same gap Phase 15
above already names as its own missing `Exceptions`-category
prerequisite. Needs `throws: Option<Type>` populated from `@throws`
(parsed-past today, same as `@deprecated`) plus a real body walk
collecting `throw` statements and transitively unioning every called
function's own `throws`. Only the "missing checked exception"/
"throw-type covariance"/"too-wide throw type" rule family needs an
opt-in flag; the "throws void but explicitly throws" family doesn't.

The exhaustive audit found the *same* gap independently in three more
categories and confirms this phase's scope should absorb all of them:
an 11-item `*ThrowsRule`/`*ThrowTypeRule` family
(`CatchWithUnthrownExceptionRule`, `MethodThrowTypeCovarianceRule`,
`MissingCheckedExceptionInFunctionThrowsRule`/`InMethodThrowsRule`,
`ThrowsVoidFunctionWithExplicitThrowPointRule`/`VoidMethodWithExplicitThrowPointRule`,
`TooWideFunctionThrowTypeRule`/`MethodThrowTypeRule`,
`DefaultExceptionTypeResolver`); the per-callee dispatch layer once the
base model exists (`DynamicFunctionThrowTypeExtension`/
`DynamicMethodThrowTypeExtension`/`DynamicStaticMethodThrowTypeExtension`);
and a whole *no throw-type/`@throws` tracking subsystem* cluster in
`src/Type/Php/*` - 16 unlocks (`ArrayCombineFunctionThrowTypeExtension`,
`AssertThrowTypeExtension`, `DateTimeConstructorThrowTypeExtension`,
`DateTimeModifyMethodThrowTypeExtension`, `DateTimeSubMethodThrowTypeExtension`,
`DateTimeZoneConstructorThrowTypeExtension`,
`DomDocumentCreateElementDynamicThrowTypeExtension`,
`DsMapDynamicMethodThrowTypeExtension`, `FilterVarThrowTypeExtension`,
`IntdivThrowTypeExtension`, `JsonThrowTypeExtension`,
`ReflectionClassConstructorThrowTypeExtension`,
`ReflectionFunctionConstructorThrowTypeExtension`,
`ReflectionMethodConstructorThrowTypeExtension`,
`ReflectionPropertyConstructorThrowTypeExtension`,
`VersionCompareFunctionDynamicThrowTypeExtension` - `json_encode`'s
`JSON_THROW_ON_ERROR` bit-detection already exists and is reused for
return-type false-stripping, so the hardest per-callee logic is partly
built already). `@throws` tag parsing itself can ship early and
separately under Phase 36 (`InvalidThrowsPhpDocValueRule`, a
self-contained Throwable-subtype check).

**Also self-contained and shippable independently, ahead of the full
model**: `ExprKind::Throw` (`src/checker/expr.rs:652-655`) never calls
the `Throwable`-subtype check that `StmtKind::Throw` already does - a
pure code-sharing fix (S effort) unlocking `ThrowExprTypeRule` with zero
new infrastructure; do this first, inside this phase, before the rest.
Also folds in `OverwrittenExitPointByFinallyRule` (finally-block
exit-point overwrite detection) as a closely related but separable
M-effort addition reusing the existing try/catch/finally CFG merge in
`StmtKind::Try`. **Recommendation.** Sequence: (1) `ThrowExprTypeRule`
fix [S], (2) `@throws` tag + Throwable-subtype check [S-M], (3)
`ThrowPoint` collection primitive [unlocks the dead-catch half of
`CatchWithUnthrownExceptionRule` immediately - do this before the full
checked-exceptions convention, since dead-catch detection is high-signal
and opt-in-checked-exceptions is comparatively niche], (4) the rest of
the `*ThrowsRule` family, (5) the dynamic per-callee throw-type
extensions.

**Phase 43 - Property-hook checker validation (PLANNED).** PHP 8.4
property hooks are fully parsed (`PropertyDecl.hooks`, and
`Param.hooks` for a promoted constructor parameter's own inline hooks)
but hook bodies are never type-checked at all - confirmed by this
project's own doc comment ("no function-like registry entry to check
them against"). Phase 14.5's `PropertySig.set_ty` fix only makes
property-*write* checking hook-aware when a `set` hook accepts a wider
native type; it doesn't check hook bodies themselves. Needs a
function-like registry entry for each hook body so the checker's
existing per-statement machinery can walk it, plus "virtual" property
detection - real PHP determines this via native
`ReflectionProperty::isVirtual()` at runtime, which this project has no
interpreter to delegate to, so it needs the real RFC semantics
reimplemented directly: virtual iff no hook body ever references the
property's own backing storage.

The exhaustive audit confirms this scope and adds concrete unlocks:
`ExistingClassesInPropertyHookTypehintsRule`,
`GetNonVirtualPropertyHookReadRule`, `SetNonVirtualPropertyHookAssignRule`,
`SetPropertyHookParameterRule` (a bright spot - the hook's declared
parameter type, `PropertySig::set_ty`, is already captured and reused
for write-checking; only validating it against the property's own type
at the declaration site is missing), `ReadingWriteOnlyPropertiesRule`.
**Also requires**, as an explicit dependency the audit surfaces that
this phase's text didn't previously name: property mutability/hook
metadata (`readonly`/`private(set)`) needs to exist on `PropertySig`
(Phase 57) before `PropertyVarianceRule` can run (properties don't
currently participate in variance checking at all - a *readonly*
property typed `T` is actually a legitimate covariant-safe position,
per this document's own "Known limitations" entry) and before the
`UnsetRule` hooked-property sub-diagnostic can be built.

**Phase 44 - `@phpstan-ignore` inline suppression (PLANNED).** Only
whole-file baseline suppression exists today (Phase 11). Real PHPStan's
`@phpstan-ignore-line`/`-next-line` are simple line markers, but bare
`@phpstan-ignore` takes a real comma-separated identifier list (each
optionally followed by a parenthesized comment) matching against a
diagnostic's own stable identifier - which `Diagnostic` doesn't have at
all today (just `location`/`message`/`span`/`level`). The exhaustive
audit's `Ignore/` category findings (`IgnoreParseErrorRule`) fold into
Phase 56's grammar-level PHPDoc parser work instead of here (parse-error
recovery is a parser concern); this phase's own suppression-matching
logic is unaffected. Its stated prerequisite is now concretely tracked
as Phase 47 (diagnostic identifier & metadata infrastructure) rather
than an unscheduled "worth retrofitting regardless" note - build Phase
47 first.

**Phase 45 - Declared-vs-actual-narrowest-type inference
(TooWideTypehints) (PLANNED).** No mechanism compares a declared type
against the narrowest type a function body/property could ever actually
produce. Real PHPStan's own version has three distinct pieces: top-level
union-member narrowing; an independently-toggled bool-narrowing special
case (`bool` narrows to literal `true`/`false` when the body only ever
produces one); and independently-toggled nested/recursive narrowing
(walking declared and actual types in parallel to narrow type arguments
too, not just the top level) - needs a real parallel-tree-walk utility
over `Type`, not a byproduct of `infer_expr`.

The exhaustive audit confirms and splits this into two concrete
sub-clusters: a **return/property variant** - unions every function/
method/closure/arrow-function's actual return statements, or a
property's actual assignments, and compares that union against the
declared type (needs a new narrow-declared-vs-observed-union helper in
`src/types.rs`, consumed from function/method/closure/property-
assignment analysis in `src/checker/*.rs`) - unlocking
`TooWideArrowFunctionReturnTypehintRule`, `TooWideClosureReturnTypehintRule`,
`TooWideFunctionReturnTypehintRule`, `TooWideMethodReturnTypehintRule`,
`TooWidePropertyTypeRule`, `TooWideTypeCheck` (L effort, medium value -
more a type-hygiene nit than a correctness-bug catcher, but a
well-defined, self-contained feature once the comparison helper
exists); and a **`@param-out` variant** - phpstan-rs has no `@param-out`
PHPDoc tag concept at all (confirmed absent from `docblock.rs`/
`registry.rs`), a harder prerequisite than the return-type variant since
there's no declared out-type to even compare against, needing new tag
support plus `src/checker/control_flow.rs` execution-end/return-scope
tracking - unlocking `TooWideFunctionParameterOutTypeRule`,
`TooWideMethodParameterOutTypeRule`, `TooWideParameterOutTypeCheck` (L
effort, low value - `@param-out` is rare in real-world PHP).

**Phase 46 - PCRE pattern-validity checking (PLANNED).** Real
PHPStan's `RegularExpressionPatternRule` flags a genuinely malformed
regex pattern. `regex_groups.rs` (Phase 12) deliberately doesn't do
this - it silently bails on anything outside its own understood subset
rather than validating, the right choice for structure-extraction but
not for catching real syntax errors. This project's first C dependency
(`pcre2` bindings) - Rust's own `regex` crate is a different-enough
flavor to give wrong answers on real PCRE patterns. Needs its own
build-time/vendoring story and a vendored-vs-system-dependency decision.
The exhaustive audit's `arrays-cast-operators-regex` category
independently arrived at the identical cluster with identical
reasoning - **no changes to this phase's scope, just confirmation.**
(`preg_quote()` delimiter cross-checking, a separate small finding from
the same audit category, is folded into Phase 71's small-builtin-checks
batch instead, since it needs no PCRE validation of its own.)

**Phase 47 - Diagnostic identifier & metadata infrastructure (NEW,
PLANNED).** `Diagnostic` (`src/checker.rs:71`) is a flat 4-field struct,
just `{location, message, span, level}`, populated via ad hoc struct
literals/`format!()` calls scattered across `src/checker/*.rs`, with no
identifier field, no tip/canned-tip helper, no metadata bag, no
`nonIgnorable` flag, and no `fixNode`/auto-fix support, and no fluent
builder to assemble any of it consistently (the Rust equivalent of
`RuleErrorBuilder`). This is what real PHPStan's `@phpstan-ignore
<identifier>` matches against, what CI output formats (Phase 16) want
for machine consumption, and what lets a "possibly impure" tip (see
Phase 41) be appended to a constant-condition diagnostic without a
second diagnostic. The core struct change is small; real parity requires
threading an identifier convention through dozens of existing
call-sites and deciding a Rust-idiomatic builder API. Prerequisite for
Phase 44 (`@phpstan-ignore`, which already names this as a blocker),
Phase 16 (CI output formats), and Phase 41's possibly-impure tip
decoration. Size: M.

**Phase 48 - Symbol-existence validation pipeline (classes, functions,
use-imports, typehints) (NEW, PLANNED).** `resolve_name_expr`
(`src/checker/resolve.rs:39`) unconditionally returns
`Type::Object(name, ..)` regardless of whether the class exists, and
`resolve_type_hint` (`src/checker/resolve.rs:53`) silently accepts a
typo'd class name in a param/return/property type unless that name is
*also* used at a `new` or call site elsewhere - confirmed by reading
both functions directly. Build the missing "does this name actually
resolve" primitive once (`ClassKind` is already tracked per class,
`src/ast.rs:213-218`, and the registry can already resolve a name to a
`ClassInfo` - this is checker wiring reusing existing data, not a new
data model), then wire it into every site that currently trusts a name
blindly: `use`/`use function`/`use const`/group-use validation,
catch-clause types, `extends`/`implements`/`instanceof`/trait-use
targets, and - the single highest-value application - parameter/
return/property type hints.

This is the highest real-world-value item in the entire audit -
typo'd/missing class references are one of PHPStan's most common
catches, and it surfaced as an independent blocker in *seven* separate
audit categories. Consolidated unlock list: `ExistingNamesInUseRule`,
`ExistingNamesInGroupUseRule`, `CaughtExceptionExistenceRule`,
`ExistingClassInClassExtendsRule` (partially done already -
`check_final_class_extension`, `src/checker/oop.rs:16-34`, implements
this rule's final-class-extension sub-check using this same registry
data), `ExistingClassesInClassImplementsRule`,
`ExistingClassesInEnumImplementsRule`, `ExistingClassesInInterfaceExtendsRule`,
`ExistingClassInInstanceOfRule`, `ExistingClassInTraitUseRule`,
`ExistingClassesInTypehintsRule`, `ExistingClassesInPropertiesRule`,
`ExistingClassesInArrowFunctionTypehintsRule`,
`ExistingClassesInClosureTypehintsRule`,
`ExistingClassesInPropertyHookTypehintsRule`, `ClassConstantRule`,
`ClassNameCheck`/`ClassCaseSensitivityCheck`/`ClassForbiddenNameCheck`
(case-sensitivity + leaked-prefix checks fold in cheaply once the
lookup exists). A fuller version of this same primitive - a centralized
"resolve a type, split union/intersection members, collect per-class
existence errors while respecting `class_exists()`/`interface_exists()`
guards, tag each with a usage-location-specific message + identifier"
pipeline matching PHPStan's `RuleLevelHelper::findTypeToCheck` +
`ClassNameUsageLocation` taxonomy - is a stretch goal on top of this
phase's MVP; it additionally needs Phase 36's attribute/assert/
template/sealed/mixin/requireExtends/requireImplements tag parsing to
reach several of PHPStan's ~27 usage-location kinds, and unlocks
`ClassNameNodePair`, `ClassNameUsageLocation`, `FoundTypeResult`, and
`RuleLevelHelper`'s `findTypeToCheck()` half. Prerequisite for
`InstantiationRule`'s remaining trait/enum-instantiation cases and
several Phase 15-adjacent items. Size: L for the MVP, XL for the
full `RuleLevelHelper`-equivalent pipeline.

**Phase 49 - Call-argument binding generalization (named args, spread,
by-ref legality) (NEW, PLANNED).** `check_args`
(`src/checker/calls.rs:262`) implements only the MVP: positional arity
plus a per-positional type check. Extend it with named-argument handling
(unknown/duplicate/positional-after-named), unpacked/spread arguments,
and by-ref target legality - the last needs the same "does this access
chain contain a nullsafe operator" walk `NullsafeCheck` provides in real
PHPStan, which this project has never built despite already tracking
nullsafe elsewhere. Unlocks `FunctionCallParametersCheck`,
`NullsafeCheck`. Prerequisite for attribute constructor-argument
validation (Phase 37). Size: L - one of the highest-value gaps found;
named-argument and by-ref misuse are common in modern PHP.

**Phase 50 - `Type::Array` key-type and general offset/shape engine
(NEW, PLANNED).** `Type::Array(Box<Type>)` carries only a value type -
no key type - and `Type::ArrayShape` (`Vec<(String, Type, bool)>`) has
no distinct int-vs-string key typing, no `isList` tri-state field, no
"unsealed"/open-shape tracking, and no per-operation return-type
overrides (slice/splice/chunk/flip/reverse/search/shift/pop/
intersectKey/fillKeys/mergeWith/etc. - roughly 30-40 of them in real
PHPStan) to propagate shape/list-ness/non-emptiness through builtin
array calls; array functions are typed generically instead of
consulting the input's own shape/accessory markers. This is explicitly
named in this document's own former Phase 15 as a recurring gap ("the
same root cause Phase 35 just ran into again, seven phases after it was
first tracked here") - the exhaustive audit shows it's now the blocker
for a much larger rule set than that one-line mention implied, warranting
promotion to its own phase.

Consolidated unlocks: `NonexistentOffsetInArrayDimFetchCheck`/`Rule`,
`ArrayDestructuringRule`, `AllowedArrayKeysTypes`,
`InvalidKeyInArrayDimFetchRule`, `InvalidKeyInArrayItemRule`,
`HasOffsetType`, `HasOffsetValueType`, `OffsetAccessAssignOpRule`,
`OffsetAccessAssignmentRule`, `ConstantArrayType`,
`ConstantArrayTypeBuilder`, `OversizedArrayType`, `OversizedArrayBuilder`,
`AccessoryArrayListType`, `NonEmptyArrayType`, plus (as direct Phase-73
consumers) `ArrayChangeKeyCaseFunctionReturnTypeExtension`,
`ArrayFlipFunctionReturnTypeExtension`, `ArrayFillKeysFunctionReturnTypeExtension`,
`ArrayRandFunctionReturnTypeExtension`, `ArrayCountValuesDynamicReturnTypeExtension`,
`ArraySearchFunctionDynamicReturnTypeExtension`, `ArrayFindKeyFunctionReturnTypeExtension`,
`ArrayIntersectKeyFunctionReturnTypeExtension`,
`ArrayKeyExistsFunctionTypeSpecifyingExtension`,
`UnsafeArrayStringKeyCastingTraverser`. `src/types.rs`'s `Type::Array`
variant and `Type::ArrayShape` definition, `AccessoryKind::ArrayList`,
and `src/checker/expr.rs`'s `infer_array_literal_type`/
`infer_spread_array_literal_type` are the concrete call sites. Size: XL,
a data-model change touching every array-construction and
array-subtyping call site in the checker, but array manipulation
preserving or destroying list-ness/non-emptiness is extremely common in
real PHP code, making this high-value despite the size.

**Phase 51 - `Type` coercion/cast method surface + `IntegerRangeType`
completion (NEW, PLANNED).** `Type` has no `toInteger()`/`toFloat()`/
`toString()`/`toBoolean()`/`toArrayKey()`-style coercion methods at all;
every conversion is done ad hoc per call site (`src/checker/expr.rs`
Cast arm, Echo/print handling, `~`/`-` unary). Pair this with completing
`IntegerRangeType` arithmetic (`shift()`, range-preserving `**`,
`toAbsoluteNumber()`/`toBitwiseNotType()` bound preservation) since
both are the same "give `Type` a real coercion API" root cause.
Unlocks `InvalidCastRule`, `EchoRule`, `PrintRule`,
`InvalidPartOfEncapsedStringRule`, plus the `IntegerRangeType`-consumer
extensions (`AbsFunctionDynamicReturnTypeExtension`,
`StrlenFunctionReturnTypeExtension`, `RandomIntFunctionReturnTypeExtension`,
`CountFunctionReturnTypeExtension`, `HrtimeFunctionReturnTypeExtension`),
`StrvalFamilyFunctionReturnTypeExtension`,
`SetTypeFunctionTypeSpecifyingExtension`. Size: L - the single
most-cited systemic gap in the `core-types` audit category; nearly
every scalar `Type`'s "partial" status traces back to this.

**Phase 52 - Invalid-operand-combination (`ErrorType`) concept for
operators (NEW, PLANNED).** `infer_binary`/unary-operator arms always
compute a best-effort, permissive result and never treat an operand
pairing as an outright type error - confirmed structurally, since
`ExprKind::Cast`/`Binary`/`Unary` and the inc/dec arms only ever compute
a resulting type and never call `self.report(...)`, and `infer_binary`
is an `&self` method that cannot report diagnostics even if someone
wanted to add one without refactoring its signature. Add an
`ErrorType`-equivalent concept (14 raw `Type::Mixed` fallback sites in
`core-types` currently mask this - no distinct "resolution failed"
sentinel exists) plus an operand-validity matrix, consulted from
binary/comparison/unary arms. Unlocks `InvalidBinaryOperationRule`,
`InvalidComparisonOperationRule`, `InvalidUnaryOperationRule`,
`InvalidIncDecOperationRule`. Size: L - high value; array+object,
string-array comparisons, `~` on an array/object are common
real fatal-adjacent bugs.

**Phase 53 - Structural Callable/Closure signature type + contextual
closure-parameter typing (NEW, PLANNED).** `Type::Callable`
(`src/types.rs:48`) is a bare marker with no parameter list, return
type, purity, or template map, and every closure-literal expression
(`src/checker/expr.rs`, ~lines 1342/1379/1470/1509) infers the flat
`Object("Closure", [])` marker, discarding the closure's own PHPDoc/
inferred signature entirely. Once a real structural type exists, wire
it into higher-order builtins (`array_map`/`array_filter`/`usort`/
`array_walk`/`preg_replace_callback`) so a closure's body is actually
*checked* with contextually-injected parameter types - today
`infer_closure_return_with_param_overrides` only uses the override to
compute the outer call's return type, never to type-check the closure
body itself, so a bug inside an `array_map` callback (e.g. calling an
undefined method on `$item`) is never caught today. Unlocks
`CallableType`, `ClosureType`, `CallableTypeHelper`,
`CallableAssertionsHelper`, `FunctionParameterClosureTypeExtension`,
`ArrayFilterParameterClosureTypeExtension`,
`ArrayMapParameterClosureTypeExtension`, `ArrayWalkParameterClosureTypeExtension`,
`PregReplaceCallbackClosureTypeExtension`, `ArrayFilterFunctionReturnTypeExtension`,
`ArrayFilterFunctionReturnTypeHelper`, `CallCallablesRule`,
`FunctionCallableRule`'s remaining dynamic-callee gap,
`DynamicMethodReturnTypeExtension`, `DynamicReturnTypeExtensionRegistry`,
`DynamicStaticMethodReturnTypeExtension`. Size: XL - high value; the
contextual-injection half is a real, currently-reachable source of
missed bugs, not a nicety.

**Phase 54 - Generic template richness (`TemplateParam` bound/scope/
variance rewrite) (NEW, PLANNED).** `Type::TemplateParam(String)`
(`src/types.rs:~60`) is a bare name with no bound, scope, variance, or
default - one flat placeholder standing in for ~19 of PHPStan's
bound-specific `Template*Type` subclasses plus the `TemplateType`
interface/trait. An unbound template can never be used as its own
bound's type (indexed as an array, compared as a bool, treated as its
bound's object methods) prior to substitution; it just becomes an
opaque name until `class_template_bindings`/`substitute_template`
resolve it. Add a `bound: Box<Type>` field (plus scope/variance/
default), rewrite `substitute_template`/`class_template_bindings`/
`check_variance` (`src/checker/generics.rs`) to compose variance
through nested generics instead of the current flat class-level
`covariant_templates`/`contravariant_templates` name-sets, and add
`@template T = Default` PHPDoc parsing (currently silently dropped).

This is the single largest cluster by class count in the whole audit -
PHPStan splits "template bound to X" into ~19 concrete subclasses where
phpstan-rs uses one variant. Unlocks the 19-class `Template*Type`
family (`TemplateArrayType`, `TemplateBooleanType`,
`TemplateConstantArrayType`, `TemplateConstantIntegerType`,
`TemplateConstantStringType`, `TemplateFloatType`,
`TemplateGenericObjectType`, `TemplateIntegerType`,
`TemplateIntersectionType`, `TemplateIterableType`, `TemplateKeyOfType`,
`TemplateMixedType`, `TemplateNullType`, `TemplateObjectShapeType`,
`TemplateObjectType`, `TemplateObjectWithoutClassType`,
`TemplateStringType`, `TemplateUnionType`, plus `TemplateType`/
`TemplateTypeTrait`/`TemplateTypeStrategy`), `TemplateTypeVariance`,
`TemplateTypeVarianceMap`, `TemplateTypeMap`, `TemplateTypeHelper`,
`TemplateTypeFactory`, `TemplateTypeScope`, `VarianceCheck`,
`MethodSignatureVarianceRule`, `FunctionSignatureVarianceRule`,
`PropertyVarianceRule`, `ClassAncestorsRule`, `GenericAncestorsCheck`,
`GenericObjectTypeCheck`, `InterfaceAncestorsRule`,
`CrossCheckInterfacesHelper`, `GenericStaticType` (`static` losing its
own generic type arguments in generic classes). `TemplateKeyOfType`/
`TemplateObjectShapeType` specifically also need Phase 80's `key-of<T>`/
`object{...}` PHPDoc syntax as a prerequisite; everything else in this
phase is independent of it. Size: XL - a genuine algorithmic rewrite,
not incremental wiring, but real-world value is high for
generic-collection-heavy codebases (arg-count mismatches, fluent-builder
`static` returns).

**Phase 55 - Narrowing-engine generalization (NEW, PLANNED).** Four
related gaps in `src/checker/narrowing.rs`, bundled because they're all
"the narrowing engine only handles specific hardcoded shapes" rather
than a general mechanism: (a) no comparison-outcome/range API for
`<`/`>`/`<=`/`>=` at all (the only `Gt`/`GtEq` arm is the narrow
`count($x) > 0` special case, confirmed by reading `infer_binary`) -
`Type::IntRange(Option<i64>, Option<i64>)` already exists in
`src/types.rs`, so this API should be built as range arithmetic on top
of the existing variant rather than a new representation (cross-check
against Phase 51's `IntegerRangeType` completion work before starting,
so the two phases don't each reinvent the same range arithmetic); (b)
no truth table for loose `==`/`!=` between differing scalar kinds; (c)
no subtracted/negated-type (`T&~U`) representation, so `!$x instanceof
Foo` on a non-union type is dropped entirely instead of recorded; (d)
`apply_narrowing` fires only for hardcoded top-level shapes (if/while
conditions, a bare `assert()`, two literal `count()` patterns) rather
than a general recursive walk of any truthy-evaluated sub-expression, so
`assert()` nested in `&&`, or `count($x)` inside a plain `if
(count($x))`, fails to narrow today. Also extend
`narrowing_type_for_predicate`'s single-arg fixed-target-type table to
handle multi-arg/argument-derived targets (`is_a`, `class_exists`,
`str_contains`) - and add the trivial one-line `is_iterable` entry
first, within this phase, as a quick win.

Unlocks `ImpossibleCheckTypeHelper`/`FunctionCallRule`/`MethodCallRule`/
`StaticMethodCallRule`, `ImpossibleInArrayHaystackFiniteTypesRule`,
`ConstantNumericComparisonTypeTrait`, `UndecidedComparisonTypeTrait`,
`ConstantScalarTypeTrait`, `SubstractableTypeTrait`,
`AssertFunctionTypeSpecifyingExtension`, `PregMatchTypeSpecifyingExtension`,
`CountFunctionTypeSpecifyingExtension`, `IsIterableFunctionTypeSpecifyingExtension`,
`ClassExistsFunctionTypeSpecifyingExtension`, `IsAFunctionTypeSpecifyingExtension`,
`IsSubclassOfFunctionTypeSpecifyingExtension`, `StrContainingTypeSpecifyingExtension`,
`StrlenFunctionTypeSpecifyingExtension`. Size: XL - high value;
redundant/impossible checks left over from refactors are a very common,
high-signal real-world finding, and this fix compounds across every
existing narrowing rule too (each currently fails inside `&&`/nested
conditions).

**Phase 56 - Grammar-level PHPDoc parser rewrite (NEW, PLANNED).**
`docblock.rs` is a lenient, best-effort line/regex tokenizer that
silently drops anything it doesn't recognize, unlike real PHPStan's
recoverable-error grammar parser which produces an explicit
`InvalidTagValueNode`. This is architectural, not a bugfix, and every
Phase 36 sub-item would ideally build on it rather than duplicate ad
hoc error handling - but it is not a hard blocker for any of them (they
can each add their own narrow validation first). Unlocks
`InvalidPHPStanDocTagRule`, `InvalidPhpDocTagValueRule`,
`IgnoreParseErrorRule` (the `@phpstan-ignore` parse-error sub-case, once
Phase 44's suppression matching exists to consume it). **Recommendation.**
Sequence optionally, either just before or interleaved with Phase 36's
Wave B - not a hard prerequisite, but doing it first avoids rework.
Size: XL.

**Phase 57 - Property/method/constant visibility enforcement (NEW,
PLANNED).** `PropertySig` (`src/registry/class_info.rs`) carries only
`ty`/`is_static`/`is_readonly`/`set_ty` - no visibility field at all -
so no checker code anywhere can enforce private/protected access,
asymmetric `private(set)`/`protected(set)`, or LSB-unsafe `static::`
access to a private static property (consumed at
`src/checker/expr.rs:1089`/`:1013`, `src/checker/assignment.rs:231`).
Separately, no call site (as opposed to declaration site) ever checks
caller-context-vs-declared-visibility for a *method* call either
(`src/checker/expr.rs`'s `MethodCall`/`StaticCall` arms, ~382-526/
923-1010, do existence and arg-checking but never visibility gating).
**Correction, verified against the current source**: threading "what
class is the calling code currently inside" through to a call site is
*not* new plumbing to build - `self.current_class`/`self.current_method`
(`src/checker.rs:131-137`) already exist, are already set on every
function/method entry and restored on exit, and are already consulted
this exact way by `check_property_assignment`'s readonly-write check
(`src/checker/assignment.rs:~260-263`, comparing `current_class`
against the declaring owner) - a directly analogous sibling check, not
a new capability. The real remaining work is narrower than originally
scoped: (a) a visibility field on `PropertySig` (and an equivalent on
`FunctionSig` for methods), and (b) the comparison logic at the four
read/write/call sites reusing the existing context fields. Both halves
are still the same missing primitive ("is this access legal from
here") applied to two member kinds - merge into one phase.

Unlocks `AccessPrivatePropertyThroughStaticRule`, `AccessPropertiesCheck`/
`Rule`/`InAssignRule`, `AccessStaticPropertiesCheck`/`Rule`/`InAssignRule`,
`MethodCallCheck`, `StaticMethodCallCheck`, `StaticMethodCallableRule`,
`CallPrivateMethodThroughStaticRule`, `AccessPrivateConstantThroughStaticRule`,
`ClassConstantRule` (visibility half). The undefined-property/undefined-
method detection these rules also cover is already implemented - the
gap is purely the visibility layer on top. Size: L - highest-value
cluster found in the `properties-enum-whitespace` audit category;
private/protected access violations are one of the most common
real-world PHP mistakes.

**Phase 58 - `MissingTypehintCheck` completeness battery (level-6
family) (NEW, PLANNED).** No `MissingTypehintCheck`-equivalent exists
anywhere: no detection of implicit-mixed parameters/returns, bare
`iterable`/`array` with no value type, non-generic use of a generic
class, or `callable` with no signature - for any declaration kind
(function, method, closure, arrow-fn, property, class constant).
Unlocks `MissingClassConstantTypehintRule`, `MissingMethodParameterTypehintRule`,
`MissingMethodReturnTypehintRule`, `MissingPropertyTypehintRule`,
`MissingFunctionParameterTypehintRule`, `MissingFunctionReturnTypehintRule`.
Size: L - build the generic `Type`-tree walker once, apply it at five
declaration-kind call sites. High value for teams targeting
PHPStan-level-6-equivalent strictness; not a correctness-bug catcher,
more a completeness nudge.

**Phase 59 - Unused-symbol detection subsystem (NEW, PLANNED).**
phpstan-rs has no dead-code/unused-variable-usage-scanning category at
all. Build one variable/member-usage walker (with the
`func_get_args()`/`compact()`/`get_defined_vars()`/dynamic-variable-name
escape hatches that force "treat everything as used"), then apply it to
four sites: function/method parameters, closure `use()` captures, and
private class members (constants/methods/properties). Unlocks
`UnusedConstructorParametersRule`, `UnusedClosureUsesRule`,
`UnusedPrivateConstantRule`, `UnusedPrivateMethodRule`,
`UnusedPrivatePropertyRule` (the property variant additionally needs 5
distinct read/write messages and `@phpstan-always-read`/`-written` tag
support). Size: L - code-cleanliness signal rather than a
correctness-bug catcher for the parameter/closure-use half, but the
unused-private-member half is a very common, high-signal real finding
after refactors.

**Phase 60 - Isset/empty/nullsafe existence-probe reversal (NEW,
PLANNED).** `is_existence_probe`/`check_expr_quiet_root`
(`src/checker/expr.rs:195-224`) deliberately suppress "offset doesn't
exist" diagnostics inside `isset()`/`empty()`/`??` - a real, tested
design choice, not an oversight. Build the *other* half real PHPStan
has: a diagnostic that fires when the probed expression's statically-
known type proves the check is pointless (`isset($x)` where `$x` is
never null; `?->` on a never-null receiver). Unlocks `EmptyRule`,
`IssetRule`, `NullCoalesceRule`, `NullsafeMethodCallRule`,
`NullsafePropertyFetchRule`. Size: M - strong effort-to-value ratio;
the harder architectural half (quiet-root suppression through nested
chains) is already built, only a type-to-message mapping remains, and
`isset($x)`-on-never-null is a very common real finding.

**Phase 61 - Return-statement / control-flow completeness (NEW,
PLANNED).** Two related gaps in return-path analysis: (a) nothing
compares a function/method's fall-through-without-return status against
its declared/PHPDoc return type (void/never/mixed exemptions, Generator
`TReturn` via yield) even though `always_terminates()` already exists
and is exercised for narrowing merges; (b) `never`-typed functions that
actually reach a `return`, and "value returned from a void function,"
have no dedicated diagnostic path (`stmt.rs`'s Return handling funnels
everything through one generic mismatch path). Unlocks `MissingReturnRule`
(wiring it to the declared return type), `FunctionReturnTypeCheck`'s
never/void completeness half. Size: M - cheap-for-value; reuses an
existing, exercised analysis rather than building new, and forgetting a
`return` is a very common real bug.

**Phase 62 - Uninitialized-property / constructor dataflow (NEW,
PLANNED).** No `ConstructorsHelper`-equivalent exists: nothing tracks
which properties every constructor path (including the parent-
constructor chain) is guaranteed to initialize before a premature read.
Also sharpen `check_property_assignment`'s current one-condition
readonly-write approximation (`current_class==owner &&
current_method=="__construct"`) with real per-constructor dataflow:
uninitialized-at-end-of-constructor detection, `__unserialize`/`__clone`
recognition, double-assignment-in-clone detection. Unlocks
`UninitializedPropertyRule`, `MissingReadOnlyPropertyAssignRule`,
`ReadOnlyPropertyAssignRule`. Size: XL - real cross-method dataflow
(every constructor's CFG plus every read site across the parent chain),
but rated among the highest-value gaps in the audit; uninitialized
typed properties are a very common real PHP error class, especially
under `strict_types`.

**Phase 63 - Duplicate-declaration scans (NEW, PLANNED).** No scan
exists for redeclared enum cases/constants/properties/methods within
one class/trait body (real value here is narrower than it sounds - PHP
itself fatals on a literal same-body duplicate; this mainly catches
duplicate enum cases and trait-composition collisions PHP does *not*
reject at parse time), nor for a same-file case-insensitive collision
between a class's own declarations and its `use`-alias imports, nor for
duplicate top-level function declarations (the registry's
`FxHashMap<String, FunctionSig>` silently last-write-wins). Unlocks
`DuplicateDeclarationHelper`/`Rule`, `DuplicateTraitDeclarationRule`,
`UsedNamesRule`, `DuplicateFunctionDeclarationRule`. Size: M - low-to-
medium value individually; batched because they're all small,
independent name-collision scans with no type-system dependency.
(Whole-*project* duplicate-class-name detection is a separate, lower-
priority item - see Phase 83.)

**Phase 64 - Override-compatibility check rewrite (methods +
properties) (NEW, PLANNED).** `check_method_overrides`
(`src/checker/oop.rs:177-307`) is one simplified function checking only
native parameter contravariance/return covariance; it never compares
visibility, never detects missing/extra/variadic/by-ref parameter
mismatches, and its ancestor lookup has no interface-first priority or
trait-abstract-method special case. Decompose it into PHPStan's
`MethodParameterComparisonHelper`/`MethodVisibilityComparisonHelper`/
`MethodPrototypeFinder` shape, and add the *equivalent* check that
doesn't exist at all today for properties (`OverridingPropertyRule`:
static/readonly/hook/visibility/final/native-type-variance mismatches
on property override). Unlocks `MethodParameterComparisonHelper`,
`MethodVisibilityComparisonHelper`, `MethodPrototypeFinder`,
`MethodSignatureRule`, `OverridingMethodRule`, `ParentMethodHelper`,
`OverridingPropertyRule`. **Prerequisite check, corrected**: the
method half has *no* hard prerequisite and can start immediately -
`check_method_overrides` already exists and just needs decomposing.
The property half's real blockers are narrower than this phase
originally claimed: `PropertyDecl` (`src/ast.rs:~149`) already carries
a `visibility` field from parsing, so *that* part is not a blocker;
what's genuinely missing is (a) `is_final`/`is_abstract` on
`PropertyDecl`/`PropertySig` - not currently in scope for *any* phase,
a real gap this phase should probably just absorb itself rather than
wait on - and (b) Phase 57's registry-side `PropertySig` visibility
field specifically for comparing a property against an *ancestor
class's* declaration (a same-file AST field alone can't do a
cross-class comparison). Phase 37's attribute AST support remains a
real prerequisite, but only for the `#[Override]` sub-check
specifically. Size: L for the method half (extends already-working
logic, ship first as its own milestone), M-L for the property half
(narrower than originally scoped, given the AST already has
visibility).

**Phase 65 - Declaration-site modifier-consistency + constructor-shape
checks (NEW, PLANNED).** A batch of small, independent boolean-flag-
combination checks the parser currently accepts leniently where real
PHP would fatal at compile time (phpstan-rs's own parser is more
permissive than the PHP engine): abstract-in-non-abstract-class,
private-abstract, final-private, interface-method-visibility,
constructor declaring a return type, `$this` reassignment/`global`/
`static`-declaration, reserved class-constant name `class`, class-
constant `final`/visibility/native-type modifiers not currently tracked
on `ClassConst`. Unlocks `AbstractMethodInNonAbstractClassRule`,
`AbstractPrivateMethodRule`, `FinalPrivateMethodRule`,
`MethodVisibilityInInterfaceRule`, `ConstructorReturnTypeRule`,
`ClassAsClassConstantRule`, `FinalConstantRule`, `FinalPrivateConstantRule`,
`NativeTypedClassConstantRule`, `InvalidVariableAssignRule`,
`ThisInGlobalStatementRule`, `ThisInStaticStatementRule`. Size: S-M per
item; batched as one phase since none has any shared blocker beyond
Phase 38's PhpVersion gate for two of the const-modifier sub-checks.

**Phase 66 - Enum sanity, ancestor-check wiring, and template-rejection
completion (NEW, PLANNED).** Extend `check_enum_case_values`
(`src/checker/oop.rs:94-121`, which already correctly detects duplicate
backing values - the hardest sub-check) with the five remaining
structural checks: constructor/destructor/magic-method bans, native
`cases()`/`from()`/`tryFrom()` redeclaration ban, backing-type-must-be-
int-or-string, `Serializable` ban, value-presence/type consistency.
Also wire `check_class_generic_bounds` into the `StmtKind::EnumDecl` arm
(`src/checker.rs:337`, currently gated to `ClassDecl` only at line 324 -
a pure wiring gap) and reject `@template` tags on enums outright (enums
can't be generic). Unlocks the `EnumSanityRule` remainder,
`EnumTemplateTypeRule`, `EnumAncestorsRule`. Size: M - cheap wins; the
hardest logic already exists or is a one-line wiring fix.

**Phase 67 - `@internal` restricted-usage engine (NEW, PLANNED).** No
stage parses or stores `@internal`, and no dispatch mechanism compares
an accessed symbol's declaring namespace against the call site's
namespace at any of the ~8 access-site shapes (method/static-method/
property/static-property/constant/function calls, first-class-
callables, `(string)`-cast-triggers-`__toString`). Unlocks a 16-item
family: `RestrictedInternalClassConstantUsageExtension`,
`RestrictedInternalClassNameUsageExtension`,
`RestrictedInternalFunctionUsageExtension`,
`RestrictedInternalMethodUsageExtension`,
`RestrictedInternalPropertyUsageExtension`, `RestrictedInternalUsageHelper`,
`RestrictedClassConstantUsageRule`, `RestrictedFunctionCallableUsageRule`/
`UsageRule`, `RestrictedMethodCallableUsageRule`/`UsageRule`,
`RestrictedPropertyUsageRule`, `RestrictedStaticMethodCallableUsageRule`/
`UsageRule`, `RestrictedStaticPropertyUsageRule`,
`RestrictedUsageOfDeprecatedStringCastRule`. Size: L - one tag + one
namespace-comparison dispatched at ~8 call-site shapes, no new
type-system concepts needed despite the large unlock count. Value is
medium: matters most for library/framework authors, less for
application codebases like this project's own Piwigo corpus.

**Phase 68 - `array_filter`/`array_values`/duplicate-key no-op
diagnostics (NEW, PLANNED).** Two cheap wins where the hard inference
work is *already done* and only diagnostic emission is missing:
`infer_array_literal_type` already detects duplicate literal-array keys
for overwrite-semantics correctness but discards the fact;
`array_filter`/`array_values`' precise return-type inference (falsy-
narrowing, list-ness via `AccessoryKind::ArrayList`) already computes
everything needed to detect a no-op call (already-list, all-truthy,
empty input). Unlocks `DuplicateKeysInLiteralArraysRule`,
`ArrayFilterRule`, `ArrayValuesRule`. Size: S - best effort-to-value
ratio in the whole array/cast audit category; do this early and
cheaply, independent of Phase 50's `Type::Array` rewrite.

**Phase 69 - printf/sprintf/scanf format-string family completion
(NEW, PLANNED).** `parse_format_specifiers`/`format_specifier_accepts`/
`check_format_string_call` (`src/checker/calls.rs:376,468,529`) already
implement a real, working subset for `sprintf`/`printf` - ordered
specifiers only. Extend with positional (`%N$`) specifiers, width/
precision `*` sub-placeholders, a strict-vs-castable matching mode,
`scanf`-style placeholder counting, and wire the existing engine into
`vprintf`/`vsprintf`/`fprintf`/`sscanf`/`fscanf`. Unlocks
`PrintfArrayParametersRule`, `PrintfHelper`, `PrintfParameterTypeRule`,
`PrintfParametersRule`, `PrintfPlaceholder`. Size: L - high value;
printf-family placeholder/argument mismatches are one of the most
common real bug classes PHPStan catches, and the existing sprintf/
printf coverage proves the approach already works.

**Phase 70 - Generator/yield type modeling (NEW, PLANNED).** No
`Generator<K, V, TSend, TReturn>` type shape exists; `yield`/`yield
from` (`src/checker/expr.rs:656-680`) both return `Type::Mixed`
unconditionally and are never compared against the enclosing function's
declared return type. Add the generic shape and wire it into key/
value/`TSend` checking plus yield-outside-a-function/wrong-return-type
validation. Unlocks `YieldFromTypeRule`, `YieldInGeneratorRule`,
`YieldTypeRule`. Size: L.

**Phase 71 - Small self-contained builtin checks batch (`call_user_func`,
`random_int`, `preg_quote`, and others) (NEW, PLANNED).** Batch of
small, independent builtin-specific checks with no shared architectural
blocker: `call_user_func()`/`call_user_func_array()` argument-
reordering-and-recheck against the target callable; `random_int()`
min>max ordering; `filter_var()`'s `FILTER_NULL_ON_FAILURE`/
`FILTER_THROW_ON_FAILURE` flag-conflict; `define()`'s ignored third
argument; `preg_quote()` delimiter cross-checking (soft-benefits from
Phase 72's pattern-argument-extraction helper); function-name case-
insensitivity + `function_exists()`-guard suppression for the existing
undefined-function check; useless-return-value builtins (`var_export`/
`print_r`/`highlight_string`). Size: M overall (each item S).

**Phase 72 - Regex capture-group inference extensions (distinct from,
not blocked by, Phase 46) (NEW, PLANNED).** Two additive improvements
to `regex_groups.rs`'s Scanner that are about *type precision for
`$matches`*, not pattern *validity* (so independent of Phase 46's PCRE-
validation work): (a) a tri-state fallback for non-enumerable capture
groups (`(\d+)` -> `numeric-string`, `([a-z]+)` -> `non-empty-string`,
instead of degrading to plain `string`) plus `(?|...)` branch-reset and
`x`-modifier support; (b) resolving a statically-foldable `Concat`
pattern argument (e.g. `'/^' . $prefix . '$/'`) instead of only
matching `ExprKind::Literal(String)` in `literal_string_value()`.
Unlocks improvements to `RegexAstWalkResult`, `RegexCapturingGroup`,
`RegexGroupParser`, `RegexGroupWalkResult`, `RegexNonCapturingGroup`,
`RegexExpressionHelper`. Size: M+S - high value; `preg_match()`
`$matches` shape typing is high-frequency, and a non-literal group
(extremely common: `(\d+)`) currently silently degrades.

**Phase 73 - Array/string stdlib function coverage, batch 1
(key-type-dependent) (NEW, PLANNED).** Extensions that specifically
need Phase 50's array key-type work first: `array_change_key_case`,
`array_flip`, `array_fill_keys`, `array_rand`, `array_count_values`,
`array_search` (key precision), `array_find_key`, `array_intersect_key`,
`array_key_exists` narrowing. Prerequisite: Phase 50. Size: L.

**Phase 74 - Array/string stdlib function coverage, batch 2
(independent of key-type work) (NEW, PLANNED).** Everything else in
the array-function long tail with no shared blocker beyond "write the
match arm": generalize the existing `array_unique` "return an
`ArrayType` from arg 0" pattern to its 16 sibling functions in
PHPStan's `ArgumentBasedFunctionReturnTypeExtension` list (`array_diff*`,
`array_udiff*`, `array_intersect*`, `array_uintersect*`);
`array_pop`/`array_shift`'s own expression-result type (the by-ref side
is already solved, only the call's own value is missing);
`array_splice`/`array_replace`/`array_pad` (can reuse `array_merge`'s
existing field-merge logic); the six array-pointer/position accessors
(`current`/`next`/`prev`/`reset`/`end`/`key`/`array_key_first`/
`array_key_last`); PHP 8.4's `array_find`/`array_find_key`/`array_any`/
`array_all` (currently not even stub-registered). Size: M (batch of
many S-effort items with a uniform pattern).

**Phase 75 - String accessory-type extensions (case-tracking) + related
string function coverage (NEW, PLANNED).** Add `Lowercase`/`Uppercase`/
`DecimalIntegerString` `AccessoryKind` variants (currently only
`NonEmptyString`/`NonFalsyString`/`LiteralString`/`NumericString`
exist), then wire `strtolower()`/`strtoupper()`'s dynamic return type to
produce the new case-tracked types, and extend `str_pad`/`str_repeat`/
`substr`/`trim`/`ltrim`/`strrev`/`parse_str` accordingly. Separately:
literal-string inference from string concatenation (currently
`literal-string` only comes from an explicit PHPDoc annotation, never
inferred - this defeats the feature's main real-world use,
SQL-injection-safety checking on functions like `PDO::query`). Unlocks
`StrCaseFunctionsReturnTypeExtension`, `StrrevFunctionReturnTypeExtension`,
`TrimFunctionDynamicReturnTypeExtension`, `LtrimFunctionReturnTypeExtension`,
`StrPadFunctionReturnTypeExtension`, `StrRepeatFunctionReturnTypeExtension`,
`SubstrDynamicReturnTypeExtension`, `ParseStrParameterOutTypeExtension`,
plus `AccessoryLiteralStringType`'s concatenation-inference gap. Size: M.

**Phase 76 - `filter_var`/`filter_input` subsystem (NEW, PLANNED).** A
whole self-contained subsystem with zero coverage today:
`FILTER_VALIDATE_*`/`FILTER_SANITIZE_*` constant-to-type mapping table,
`FILTER_NULL_ON_FAILURE`/`FILTER_REQUIRE_ARRAY`/`FILTER_FORCE_ARRAY`
flag logic, literal-value coercion rules. Unlocks
`FilterFunctionReturnTypeHelper`, `FilterInputDynamicReturnTypeExtension`,
`FilterVarArrayDynamicReturnTypeExtension`, `FilterVarDynamicReturnTypeExtension`.
Size: L - common in real-world form/input-handling code, the kind of
code this project's own Piwigo corpus work has been targeting.

**Phase 77 - Misc built-in-class/runtime-capability modeling batch
(NEW, PLANNED).** Batch several independent, lower-value stdlib-
modeling gaps: `get_class`/`get_called_class`/`get_parent_class`/
`gettype`/`get_debug_type` (late-static-binding + runtime-string
mapping tables); `date()`/`idate()` format-character-to-type table
(static, no runtime simulation needed - unlike the DateTime/DOM/
SimpleXML cluster in Phase 78); hardcoded constant-array-shape tables
for `curl_getinfo` (extend the existing ~20-entry table to the full
~75), `stat`/`dio_stat`, `pathinfo`, `parse_url`, `sscanf`; runtime-
capability-list snapshots for `hash_algos()`, `openssl_get_cipher_methods()`,
`mb_list_encodings()`; Reflection-class modeling in the registry;
`SimpleXMLElement` magic-property resolution. Size: M - low individual
value; batch to amortize planning overhead, skip or deprioritize unless
a corpus surfaces heavy Reflection/XML/hash usage.

**Phase 78 - DateTime/DateInterval/DOM/SimpleXML literal-probing (NEW,
PLANNED, low priority).** Real PHPStan validates these by constructing
the actual PHP object (`DateInterval`, `DOMDocument`, `SimpleXMLElement`)
at analysis time and observing whether it throws - phpstan-rs has no
embedded PHP runtime or XML parser to replicate this. Would need a
hand-written relative-date-string parser and/or XML validator from
scratch. Flagged as low-priority/possibly-permanent-boundary rather
than concretely scheduled - several of the underlying PHPStan classes
(`DateIntervalConstructorThrowTypeExtension`,
`SimpleXMLElementConstructorThrowTypeExtension`) were independently
marked `not_applicable` elsewhere in the audit for the identical "no
PHP runtime to shell out to" reason (see Won't-port list). Size: XL,
low value.

**Phase 79 - Enum-case singleton type + `BackedEnum::from()` narrowing
(NEW, PLANNED).** Enum case fetches resolve through the generic
`lookup_const` path today, producing plain `Type::Object(fqn, [])` with
no per-case singleton identity - `->name` is generic string not a
literal, `->value` is a coarse union of all cases rather than the exact
per-case value. Add a real per-case type, then use it to narrow
`::from()`/`::tryFrom()` to the exact matching case. Unlocks
`EnumCaseObjectType`, `BackedEnumFromMethodDynamicReturnTypeExtension`.
Benefits from (not strictly blocked by) Phase 55's subtracted-type work
for match-exhaustiveness-quality narrowing. Size: L - enums are
idiomatic in PHP 8.1+ codebases.

**Phase 80 - Object-shape/`key-of<T>`/conditional-return-type PHPDoc
syntax (NEW, PLANNED).** `docblock.rs` deliberately skips conditional-
return-type syntax (`($subject is Target ? If : Else)` - a tested,
deliberate design choice, not an oversight, per this document's own
Permanent design boundaries) and never parses `object{...}` shapes or
`key-of<T>`/`value-of<T>` pseudo-types. Add a deferred-resolution
mechanism to `Type` plus parsing for all three syntaxes. Unlocks
`ConditionalType`, `ConditionalTypeForParameter`, `KeyOfType`,
`ValueOfType`, `OffsetAccessType`, `LateResolvableType`,
`ClassConstantAccessType`. Prerequisite for the `TemplateKeyOfType`/
`TemplateObjectShapeType` bound kinds in Phase 54. Size: XL - the
single biggest coherent missing PHPDoc subsystem by item count;
conditional return types are common in real stubs and increasingly in
user `is_*()`-predicate code.

**Phase 81 - Object-operator overloading dispatch (GMP/`BcMath\Number`)
(NEW, PLANNED).** `infer_binary` and unary-op inference assume plain
numeric operands throughout; add a class-name-keyed dispatch hook for
the two built-in classes that overload `+`/`-`/`*`/`/`/`**`/`%`/
comparisons. Unlocks `GmpOperatorTypeSpecifyingExtension`,
`GmpUnaryOperatorTypeSpecifyingExtension`,
`BcMathNumberOperatorTypeSpecifyingExtension`,
`BcMathNumberUnaryOperatorTypeSpecifyingExtension`. Size: M, low value -
GMP/BcMath are rare in typical application corpora.

**Phase 82 - Collector / cross-file aggregation architecture
(opportunistic infrastructure, do not schedule standalone) (NEW,
PLANNED).** phpstan-rs is a single-process, file-by-file, eager-
diagnostic checker with no two-phase Collector-then-aggregate pass.
Blocks a handful of specifically low-value rules:
`ConstantConditionInTraitCollector`/`Helper`/`Rule` (dedup a trait's
constant-condition diagnostic across using classes),
`FunctionCallConstantConditionCollector`/`Helper`/`Rule`,
`TraitDeclarationCollector`/`TraitUseCollector`/`NotAnalysedTraitRule`
(unused-trait detection), `NamedArgumentParameterMethodCallsCollector`/
`OverridingMethodRenamesParameterCollector`/
`MethodCallWithPossiblyRenamedNamedArgumentRule`. Also a **soft**
prerequisite for Phase 41's transitive-purity resolution.
**Recommendation.** Do not schedule this on its own merits - every
cluster that needs it independently notes it's XL cost for a narrow/
low-value payoff. Build it only when Phase 41's transitive-resolution
half is actually being tackled, and let that phase absorb the cost.
Size: XL, low standalone value.

**Phase 83 - Whole-project duplicate class-name detection (NEW,
PLANNED, low priority).** The registry is a single global
name -> `ClassInfo` map with documented last-write-wins semantics
(`checker.rs:~98` explicitly calls this an accepted simplification in
the *opposite* direction - silently pick one rather than flag). Tracking
multiple declarations per name project-wide cuts against the core
registry design. Unlocks `DuplicateClassDeclarationRule`.
**Recommendation.** Low priority; real but rare (a stray duplicate class
in a legacy tree). Size: XL, low value.

**Phase 84 - `RuleLevelHelper`-equivalent configurable type-acceptance +
tri-state-with-reasons architecture (NEW, PLANNED, low priority).**
`is_assignable`/`is_subtype` return plain `bool` today with no
`checkNullables`/`checkUnionTypes`/`checkExplicitMixed`-style
configurability toggles and no tri-state (`Yes`/`No`/`Maybe`)-with-
reasons result - PHPStan's whole `accepts()`/`isSuperTypeOf()` double-
dispatch protocol is built around `AcceptsResult`/`IsSuperTypeOfResult`/
`TypeResult` objects that thread reasons through every comparison, with
no phpstan-rs analog (`src/types.rs`'s `is_subtype`, ~line 779;
`src/registry.rs`'s `is_assignable`). Architectural, touches every call
site. Unlocks `AcceptsResult`, `IsSuperTypeOfResult`, `TypeResult`.
**Recommendation.** Improves diagnostic *explanation* quality more than
it catches new bugs (the "maybe" case already collapses to a concrete
answer somewhere in the match arms today). Low priority - schedule only
if diagnostic-message quality becomes a stated goal. Size: XL,
low-medium value.

## Known limitations

Every deliberate "this checker doesn't model X" boundary, gathered in
one place so none of them are only discoverable by reading in-code
comments.

### Tracked under a future phase

| Limitation | Phase |
| --- | --- |
| PHP 8.5's pipe operator (`BinOp::Pipe`) always infers `Mixed` - needs new infrastructure to resolve an arbitrary callable expression to its own return type, which this project has nowhere yet | 13 |
| `DeadCode`/`Comparison` rule catalog (unreachable-after-terminating-code, constant-condition, match/switch exhaustiveness, impossible-instanceof) - narrowed from a broader catch-all after the exhaustive audit split visibility and `Type::Array`'s key type out to their own phases | 15 |
| `continue N`'s own level argument is parsed and stored but never consulted (`break N`'s own level is, since Phase 29) | 15 |
| No CI-friendly output formats (JSON/checkstyle/junit/...) | 16 |
| No LSP hover/go-to-definition | 17 |
| No extension/plugin API | 18 |
| `#[...]` attributes are lexed and discarded, never becoming real AST nodes (only two narrowly special-cased stub-generation attributes get lexer-level text splicing) - confirmed the single most cross-cutting gap in the codebase, independently blocking six audit categories | 37 |
| No target-PHP-version config, so no "X only supported on PHP Y.Z+" portability check is possible - confirmed as a recurring blocker across four more audit categories | 38 |
| `@deprecated` is parsed-past and discarded; no restricted/deprecated-symbol-usage warnings | 39 |
| No checking of PHP code that consumes real PHPStan's own API for `@api`-boundary violations | 40 |
| No purity/impurity tracking anywhere | 41 |
| `FunctionSig` has no `throws` field; no checked-exception/throw-type model | 42 |
| PHP 8.4 property hook bodies are parsed but never type-checked (Phase 14.5's `set_ty` fix only makes property-write checking hook-aware, not hook bodies themselves) | 43 |
| Only whole-file baseline suppression exists; no `@phpstan-ignore` inline suppression, and `Diagnostic` has no stable per-check identifier at all | 44 |
| No declared-vs-actual-narrowest-type inference (`TooWideTypehints`) - confirmed to split into a return/property variant and a rarer `@param-out` variant | 45 |
| No PCRE pattern-validity checking - `regex_groups.rs` (Phase 12) deliberately doesn't validate, only extracts structure | 46 |
| `Diagnostic` (`checker.rs:71`) has no identifier/tip/metadata fields or fluent builder | 47 |
| `resolve_name_expr`/`resolve_type_hint` never check whether a referenced class/function/use-import actually exists - the single highest real-world-value gap found by the exhaustive audit | 48 |
| `check_args` (`calls.rs:262`) has no named-argument, spread-argument, or by-ref-target-legality handling | 49 |
| `Type::Array` has no key-type parameter, and `Type::ArrayShape` has no int-vs-string key typing or unsealed-shape tracking - a `foreach`'s own `$key`, `array_keys()`'s precise result, or a `preg_match` by-ref `$matches` element, is always `Mixed`/imprecise regardless of how precisely-keyed the real array is. Surfaced repeatedly: Phase 29 (a loop key), Phase 31 (`array_keys()`, partially closed for the literal-shape case only), Phase 32 (the generic fallback), Phase 35 (a repository call finally being checked at all) | 50 |
| `Type` has no coercion/cast method surface (`toInteger`/`toString`/etc.); `IntegerRangeType` arithmetic is incomplete | 51 |
| No invalid-operand-combination (`ErrorType`) concept - `infer_binary`/unary arms always compute a best-effort result and never flag an outright type error | 52 |
| `Type::Callable` is a bare marker with no signature; closures passed to higher-order builtins are never contextually type-checked against their own body | 53 |
| `Type::TemplateParam` carries no bound/scope/variance/default - a bounded `@template T of X` collapses to opaque `Mixed` after substitution instead of `X` | 54 |
| No comparison-outcome API for `<`/`>`/`<=`/`>=`, no loose-`==` truth table, no subtracted/negated-type tracking, and `apply_narrowing` only fires for hardcoded top-level shapes, not nested/nested-boolean sub-expressions | 55 |
| `docblock.rs` is a lenient regex tokenizer, not a recoverable-error grammar parser - malformed tag values pass through silently instead of producing a diagnosable error node | 56 |
| `PropertySig` has no visibility field at all, and no call site checks caller-context-vs-declared-visibility for method calls either | 57 |
| No `MissingTypehintCheck`-equivalent completeness battery (implicit-mixed params/returns, bare `iterable`/`array`, non-generic use of a generic class) | 58 |
| No unused-parameter/closure-use/private-member detection anywhere | 59 |
| No "this `isset()`/`empty()`/`?->` check is statically provably redundant" diagnostic (the suppression half already exists and is deliberate) | 60 |
| Fall-through-without-`return` is never compared against a declared/PHPDoc return type despite `always_terminates()` already existing | 61 |
| No constructor-dataflow tracking for uninitialized typed/readonly properties across the parent-constructor chain | 62 |
| No duplicate-declaration scan within a class/trait body, or for duplicate top-level function declarations | 63 |
| `check_method_overrides` (`oop.rs:177-307`) never compares visibility or detects missing/extra/variadic/by-ref parameter mismatches; no property-override-compatibility check exists at all | 64 |
| Several PHP-fatal-in-practice declaration combinations (abstract-in-non-abstract-class, final-private, interface-method-visibility, `$this` reassignment, reserved constant name `class`, ...) are accepted leniently by the parser with no checker-side rejection | 65 |
| `check_enum_case_values` covers only duplicate backing values, not the five other structural enum-sanity checks; `check_class_generic_bounds` isn't wired to `EnumDecl` at all | 66 |
| No `@internal`-tag parsing or namespace-boundary usage restriction anywhere | 67 |
| `infer_array_literal_type` already detects duplicate literal-array keys and `array_filter`/`array_values`'s own no-op-call conditions but never reports either | 68 |
| `sprintf`/`printf` format-string checking (`calls.rs:376,468,529`) has no positional (`%N$`) specifiers, `*` width/precision, or `scanf`/`vprintf`/`fprintf`/`sscanf`/`fscanf` coverage | 69 |
| No `Generator<K,V,TSend,TReturn>` type shape; `yield`/`yield from` always return `Mixed` and are never checked against the enclosing function's declared type | 70 |
| No `call_user_func`/`call_user_func_array` argument-reordering recheck, `random_int()` ordering check, or several other small self-contained builtin checks | 71 |
| `regex_groups.rs`'s capture-group inference has no tri-state fallback for non-enumerable groups and doesn't resolve a statically-foldable concatenated pattern | 72 |
| `array_change_key_case`/`array_flip`/`array_fill_keys`/`array_rand`/`array_count_values`/key-precise `array_search` are unimplemented, blocked on Phase 50 | 73 |
| The `array_diff`/`array_intersect` family, `array_pop`/`array_shift`'s own return value, `array_splice`/`array_replace`/`array_pad`, array-pointer accessors, and PHP 8.4's `array_find`/`array_any`/`array_all` are unimplemented | 74 |
| No `Lowercase`/`Uppercase`/`DecimalIntegerString` accessory string kinds; literal-string is never inferred from concatenation (undermines SQL-injection-safety-style checking) | 75 |
| `filter_var`/`filter_input` have zero dynamic-return-type coverage | 76 |
| `get_class`/`gettype`/`date()` format tables, `curl_getinfo`'s ~75-entry constant table (only ~20 covered), Reflection/`SimpleXMLElement` modeling are all thin or absent | 77 |
| No PHP-runtime/XML simulation for `DateTime`/`DateInterval`/`DOM`/`SimpleXMLElement` literal-argument validation - likely a permanent boundary, see Won't-port below | 78 |
| Enum cases have no per-case singleton identity type; `::from()`/`::tryFrom()` never narrow to the matching case | 79 |
| Conditional PHPDoc return-type syntax, `object{...}` shapes, and `key-of<T>`/`value-of<T>` are entirely unparsed | 80 |
| `infer_binary`/unary inference has no per-class operator-overload dispatch (GMP, `BcMath\Number`) | 81 |
| No Collector-style two-phase cross-file aggregation pass (fold into Phase 41 when tackled, not standalone) | 82 |
| No whole-project duplicate-class-name detection (registry is single global last-write-wins by design) | 83 |
| `is_assignable`/`is_subtype` return plain `bool`, no tri-state-with-reasons result architecture | 84 |

### Known but not yet scheduled to any phase

- **`callable-string`/`trait-string`/`interface-string`/`enum-string`
  stay aliased to plain `string`** (Phase 14) - each names "a string
  believed to hold a specific kind of identifier," with no literal-
  based or narrowing-based way for this project to ever verify content.
- **`array_merge` only tracks list/non-empty markers and shape-merging
  when its arguments are uniformly known-shaped or uniformly list-like**
  (Phase 14.5) - a *mix* of a known `array{...}` shape and a plain,
  unproven `array<T>` argument still degrades to a generic element-type
  union rather than real PHPStan's own full key-aware merge across all
  arguments together; every other stdlib array function beyond the
  seven Phase 14/14.5 cover (`array_values`/`array_filter`/
  `array_map`/`array_combine`/`array_slice`/`array_reverse`/
  `array_unique`, plus `array_merge` itself) remains untouched.
- **Two narrower narrowing gaps found while building Phase 14's own
  sources, deliberately left unfixed for now**: the reversed comparison
  `0 < count($x)` (only `count($x) > 0`/`count($x) >= 1` narrow); and
  `!empty($x)` doesn't further upgrade an already-`non-empty-string`
  value to the stronger `non-falsy-string` inside a nested check after
  `$x !== ''` - both real, but low-value (a rare reversed idiom; a rare
  redundant double-guard pattern) with no corpus evidence calling for
  them.
- **An overriding method's own parameter doesn't inherit a stricter
  ancestor/interface docblock refinement it doesn't redeclare itself** -
  Doctrine's `EntityManager::getReference()`'s real generic signature
  (`@template T of object`, `@param class-string<T> $entityName`,
  `@return T|null`) lives only on the interface it implements
  (`EntityManagerInterface`); the concrete class's own method has no
  docblock at all, so this project's method-signature resolution never
  sees the generic bound, falling back to `mixed|null`. Confirmed real
  (`ActivityRepository.php:123`, `CommentRepository.php:80`, Phase 13).
  A non-generic instance of the identical gap (Phase 14.5's own fifth
  round): a delegating "spy" wrapper's own `getTagsContentTitle(array
  $tags)` (no local docblock) passing `$tags` straight through to
  `$this->inner->getTagsContentTitle($tags)`, where the *interface*
  separately narrows the same parameter to `list<array>` - confirmed
  directly against real PHPStan that it does NOT flag this (verified
  false the moment the interface relationship is removed), meaning real
  PHPStan really does resolve an overriding method's own parameter type
  against the closest ancestor's docblock when the override doesn't
  provide its own, not just its native hint. Fixing either shape
  properly means real interface-to-implementation PHPDoc inheritance
  for method signatures generally (params and returns, generic or not),
  a sizable feature on its own - not attempted piecemeal per shape.
- ~~`@phpstan-return`/`@phpstan-param`/`@phpstan-var` aren't recognized
  at all~~ **fixed, Phase 14.5's sixteenth round** - see that round's
  own write-up below for what actually closed this without needing the
  full `@phpstan-type`/`@phpstan-import-type` alias-tracking feature a
  ninth-round attempt (below) had concluded was a hard prerequisite:
  rather than parsing alias definitions at all, an unresolvable `Object`
  class name on the *expected* side of a compatibility check now
  degrades to "compatible with anything" (excluding the real `object`
  pseudo-type), the same "don't guess, don't false-positive" philosophy
  `Registry::is_assignable` already applies elsewhere - an alias
  reference and a genuine unresolvable-class typo are indistinguishable
  to this project either way, and treating both leniently costs nothing
  a real corpus re-check didn't already confirm safe. `@phpstan-type`/
  `@phpstan-import-type` themselves (actually resolving what an alias
  *means*, not just tolerating references to one) remain unparsed -
  still real, open work under Phase 36 below if a future gap ever needs
  the alias's own precise type rather than blanket leniency.
- ~~No mechanism for multi-variable compound-boolean narrowing~~
  **fixed, Phase 14.5's seventeenth round** - turned out not to need
  the "sizable narrowing-engine feature" this entry originally expected:
  `and_chain_narrowing_target` (a new De Morgan-aware counterpart of
  the existing `or_chain_narrowing_target`) plus a new `&&`-false-branch
  `apply_narrowing` arm was enough, since the existing `Or`-false-branch
  recursion already decomposes an arbitrarily-long `||` chain into
  independent per-leaf calls with no cross-key reasoning needed at all.
- **No mechanism for narrowing correlated across two *separate*
  variables/statements** - real PHPStan tracks this via its own
  `ConditionalExpressionHolder` machinery; this project has nothing
  equivalent. Two distinct shapes, both confirmed real and repeated
  enough (9 diagnostics total across 5 files, as of the seventeenth
  round) to name together, though probably not fixable by the same
  single mechanism:
  - A boolean flag assigned exactly a condition expression
    (`$flag = ($x === null || $x === 0);`), later checked bare or
    negated (`if (! $flag) { /* $x is knowably non-null/non-zero here
    */ }`) - real shape, `CategoryService::setCatStatusWithinTransaction`/
    `createVirtualCategory` (3 sites). Would need tracking, for a
    boolean-typed local, which condition expression it was assigned
    from - genuinely hard to thread safely through this project's own
    `Locals` (an immutable, structurally-shared map chosen specifically
    for O(1)-clone branch/loop merging, Phase 20 Finding B; a parallel
    mutable side-channel would need its own correct branch-merge and
    reassignment-invalidation semantics duplicated from scratch, not
    free the way `Locals` itself gets it).
  - Two variables assigned together in the same `if`/`else` branch,
    where narrowing one (typically via `instanceof`/`!== null`) should
    imply the other's own nullability too - real shape,
    `UploadService::addUploadedFile`'s own `$dup_detect_lock_conn`/
    `$dup_detect_lock_name` pair (the corpus's own comment there
    explicitly says "PHPStan proves this itself"), and
    `RateService::rate`'s `$userAnonymous`/`$savedAnonymousId` pair
    (further complicated by both being captured into a closure by
    value, so any correlation would need to survive that boundary too).
- **Two structurally-infeasible-without-embedding-PHP-or-a-SQL-parser
  gaps**, investigated during the eighteenth round and confirmed real
  rather than assumed:
  - `strtotime()`'s own real accuracy comes from real PHPStan's
    `StrtotimeFunctionReturnTypeExtension` *literally calling PHP's own
    `strtotime()`* on a constant-string argument at analysis time
    (PHPStan runs inside PHP itself, so it can) to fold the exact
    result, narrowing away the `int|false` union's `false` member only
    when the literal genuinely parses. A pure-Rust reimplementation
    would need either a full clone of PHP's own famously permissive
    relative-date grammar or shelling out to a real PHP binary -
    neither proportionate to the four real corpus call sites this
    causes (`date()`/`gmdate()` fed a `strtotime()` result directly).
  - A raw SQL/DQL query string's own `SELECT` column list determines
    a result row's real shape (which fields exist, and often their
    real type - `COUNT(...)` is an integer, a joined entity column is
    whatever that column's own type is), but neither this project nor,
    as far as a query-builder call site is concerned, real PHPStan
    itself parses SQL/DQL text to recover that - confirmed by tracing
    `TagRepository::findCommonTags`'s own `$row['counter']` (from a
    `COUNT(...) AS counter` DQL select) back to `Doctrine\ORM\
    AbstractQuery::getResult()`'s own declared return type, `mixed`,
    with no shape information at all. Real PHPStan must be accepting
    this the same lenient "an unshaped/`mixed` value satisfies a more
    specific declared type" way this project's own `Array(Mixed)`
    leniency already does - not a gap in narrowing/inference at all,
    just a case this project hasn't traced to ground truth well enough
    yet to be confident replicating the exact same leniency here
    wouldn't cost real precision elsewhere.
- **An anonymous class (`new class implements Foo { ... }`) always
  infers as `Mixed`** - it isn't indexed into the registry under any
  stable name, so nothing else can look its members up even if its own
  type said otherwise. A nineteenth-round attempt tried a narrow
  shortcut around this (alias the instance to whichever single
  `extends`/`implements` target it names, reusing that name's own
  already-registered members instead of inventing anything new) and
  confirmed, via a real corpus re-check, that it's unsound: the
  overwhelmingly common real use of an anonymous class in a *test* file
  is a "spy" - implementing some interface but also declaring its own
  extra instrumentation properties/methods a test later reads directly
  (`$spyTransport->captured`, real shape) - which the shortcut's
  aliased interface-only type doesn't have, turning every one of those
  reads into a new "undefined property" false positive (+11 across 4
  files, worse than the 4 real fixes it found). Reverted. A real fix
  needs what the shortcut tried to avoid: registering the anonymous
  class's own full declaration (members, `extends`/`implements`, the
  works) under *some* stable-enough synthetic name, the same way a
  named class already is - a real, if narrowly-scoped, register-a-class
  feature, not a type-inference tweak.
- **phpstorm-stubs' interleaved-multiple-parameter-list shape isn't
  recognized.** Some stubs (`pg_query_params`, `pg_query`, several
  others) declare *two full parameter lists* under the same parameter
  names across PHP-version eras (e.g. two separate `$connection`
  parameters, one closed-range-with-default, one open-range-with-no-
  default, side by side) rather than the clean "one superseded param +
  one modern variadic" shape `array_map` has, which Phase 8.5's fix
  already handles. Left an estimated ~384 residual "expects at least N
  arguments" diagnostics as of Phase 8.5 (re-confirmed still present,
  not yet fixed, as of Phase 24). No phase has been scheduled for this -
  a candidate for a future round if it turns out to matter in practice.
- **No cross-branch correlation for a repeated identical condition on a
  never-reassigned variable.** Real shape found in Phase 14.5's
  fifteenth round (`UploadService::addUploadedFile`): an outer `if
  (isset($image_id)) {...} else { $dbnow = ...; ... }` merges `$dbnow`
  back to `string|null` after the `else` branch (correctly - the
  `isset($image_id)` branch never touches it), but a *second*,
  textually-later `if (isset($image_id)) {...} else { ...uses $dbnow...
  }` on the exact same never-reassigned `$image_id` is - in real PHP
  semantics - guaranteed to re-enter the same branch, so `$dbnow` is
  still definitely a `string` there. This project has no mechanism for
  correlating two separate conditions on the same variable across a
  merge boundary this way; a close relative of the compound-boolean gap
  above, but present even for a single condition checked twice, and
  present too when the condition is stored in an intermediate boolean
  variable first (`$need_generate = ($x === false or ...); ... if (!
  $need_generate) { /* uses $x narrowed */ }`,
  `ImageDerivativeController::__invoke`, same round). No phase scheduled.
- **A long tail of smaller `core-types`/`type-misc`/`php-dynamic-
  extensions` precision gaps**, surfaced by the exhaustive audit but
  each too narrow to warrant its own phase - grouped here rather than
  itemized past this point (the full per-file reasoning for each is in
  `.claude/workflows/phpstan-full-parity.result.json`, not silently
  dropped): no `NameScope`/`FileTypeMapper`-style per-location caching
  architecture; `IntersectionType` doesn't delegate a general
  `object&object`/`callable&array` combination; `static`/`$this`
  late-binding is a bare marker with no attached class binding;
  `ConstFetch`/user-defined-`define()`-constant type tracking doesn't
  exist; no `BenevolentUnionType` concept; no per-class method-return-
  type override table for extending a built-in class; a default
  parameter value doesn't influence unbound `@template` inference; no
  `HasMethodType`/`HasPropertyType`-style structural capability
  assertion exists for dynamic-member narrowing outside `ArrayShape`;
  `class-string<T>`'s own `instanceof`/`is_a()` and sealed-hierarchy
  narrowing is unmodeled; `TypeCombinator` has no general `intersect()`/
  `remove()` (only a small hand-rolled `union_types` subset) and no
  generic recursive `Type`-tree traverse/map combinator exists anywhere
  for other callers to reuse; `UnionType`'s `describe()` has no
  deterministic member sort order and `VerbosityLevel` has one fixed
  detail level; `NeverType`/`ConstantScalarType`/`ExponentiateHelper`/
  `FiniteTypeSet`/`RecursionGuard` each have a narrow, independent
  precision or ergonomics gap versus their real-PHPStan counterpart.
  None of these block a specific missing rule the way the phased items
  above do - they'd mostly improve inference precision or diagnostic
  quality at the margins. Revisit if a future corpus round traces a
  real false positive/negative back to one of them specifically.

### Permanent design boundaries (not phase-tracked - not planned, by choice)

- **Conditional PHPDoc return types** (`@return ($x is 'a' ? A : B)`) -
  documented in `types.rs`'s own module comment since Phase 2; these
  need real per-call-site type-parameter binding this project's PHPDoc
  parser deliberately doesn't attempt.
- **A dynamic class target** (`$obj::method()`, `(new Foo())::CONST`) is
  never checked, by the same "don't guess" precedent `Expr::New` already
  set for a dynamic class-name expression - there's no static name to
  resolve at all, not a missing feature to build.
- **A generic bound checked against a class this project has no real
  data for** (`checker::is_type_fully_known`, Phase 10.5) silently skips
  that check rather than guessing - a PHAR-bundled dependency with no
  loose source available anywhere this project vendors (Phase 21 closed
  this for real PHPStan itself by reusing `vendor/phpstan-src` as a
  second stub-class source; `vendor/phpstan-src`'s own transitive
  `nikic/php-doc-parser` dependency still has this problem one level
  down, not vendored here), or a local `@phpstan-type` alias this
  project doesn't parse.
- **Variance checking (Phase 10.5) doesn't cover properties, or flip
  variance through a nested generic argument.** Properties: this
  project doesn't track `readonly` at all, and a *readonly* property
  typed `T` is actually a legitimate covariant-safe position - checking
  properties without that information would misflag every real one.
  Nested arguments: a `T` found inside another class's own
  `Collection<T>` type argument is checked the same as a direct
  occurrence, not re-derived from `Collection`'s own variance.
- **Integer overflow/precision, floating-point rounding, and other
  runtime-value-dependent behavior** - this checker only ever reasons
  about static types, never concrete values (Phase 1's own founding
  choice), so anything whose result depends on the actual runtime value
  rather than its type is out of scope by construction.
- **`array_flip`/`compact`/`get_defined_vars`** (Phase 12) - would need
  tracking array *key* types (see the `Type::Array` row above) or
  building a result from a string-named/whole-scope variable lookup, a
  different and much more dynamic mechanism than anything this project
  attempts. Deliberately not attempted, not a "not yet" gap.
- **A `Closure(...)`/`callable(...)` PHPDoc shape's own per-parameter/
  return signature** (Phase 30) - degrades to plain `Type::Callable`,
  matching this project's existing bare-`callable` leniency, rather than
  modeling the shape precisely.
- **A `foreach` header's own inline `@var` annotation** (Phase 35) - the
  loop body is checked inside the same match arm that binds the loop
  variable, so only a tag on a statement *inside* the body (the
  evidenced real-world shape) is supported; a tag directly on the header
  would need to affect the loop body's own checking and was deliberately
  left unsupported rather than shipped half-working.

The exhaustive 855-file audit behind the Open phases rewrite above
additionally classified ~208 PHPStan `Rules`/`Type` files as
`not_applicable` - no sensible Rust-rewrite target, not a missed gap.
The individually-argued reasoning for every one of them lives in
`.claude/workflows/phpstan-full-parity.result.json`; the recurring
patterns, for the record:

- **PHPStan's own BC-promise and reflection-migration machinery**
  (`Api/*`, 11 items - `ApiClassConstFetchRule`, `ApiMethodCallRule`,
  etc., plus `GetTemplateTypeRule` and the PHP-parser-v4-migration
  rules) - every one of these gates on the analyzed code referencing
  classes literally inside PHPStan's own `PHPStan\*` namespace under a
  `vendor/` directory; phpstan-rs is a Rust binary with no such PHP
  namespace of its own for analyzed code to misuse. (Phase 40, above,
  is the correct mirror-image feature - checking code that *consumes*
  real PHPStan's API - and remains scheduled.)
- **`Playground/*`** (11 items - `ArrayDimCastRule`, `FunctionNeverRule`,
  `NoPhpCodeRule`, `PromoteParameterRule`, etc.) - registered only in
  phpstan.org's separate issue-bot service (confirmed via
  `vendor/phpstan-src/conf/*.neon`), never the CLI analyzer real
  PHPStan ships.
- **DI/plugin-architecture wrapper classes with zero logic of their
  own**: `DirectRegistry`/`LazyRegistry`/`Registry`/`Rule`, `RuleErrors/`'s
  129 auto-generated `RuleError<N>` builder-pattern permutation classes
  (confirmed by direct inspection - pure combinatorial codegen, no
  unique check logic; excluded from the audit's 855-file count for the
  same reason) and the 8 trivial `RuleError`-family marker interfaces,
  `AlwaysUsedClassConstantsExtension`/`AlwaysUsedMethodExtension`, the
  ~10 `*TypeSpecifyingExtension`+`Registry` pairs and
  `*ParameterClosureThisExtension`/`ParameterOutTypeExtension`
  interfaces, and the `Traits/*` interface-boilerplate-avoidance stubs
  (`NonArrayTypeTrait`, `NonCallableTypeTrait`, `NonGeneralizableTypeTrait`,
  `NonGenericTypeTrait`, `NonRemoveableTypeTrait`, `TypeWithClassName`,
  `TypeTraverserCallable`, `CompoundType`, `JustNullableTypeTrait`, and
  siblings) - phpstan-rs has no DI container or plugin model and no
  per-type-kind class hierarchy needing default no-op methods; every
  check is a hardcoded Rust function/match arm, so there is no seam
  these interfaces would fill. **Caveat**: `NonGeneralizableTypeTrait`'s
  own notes concede PHPStan's `generalize()` (loop-fixpoint type
  widening for control-flow stabilization) has no counterpart anywhere
  in phpstan-rs - a real, narrow gap hiding behind this not-applicable
  label, and a candidate for folding into Phase 55 if a corpus surfaces
  it.
- **PHP-runtime/XML-shell-out techniques with no faithful Rust port**:
  `DateIntervalConstructorThrowTypeExtension`,
  `DateIntervalCreateFromDateStringThrowTypeExtension`,
  `SimpleXMLElementConstructorThrowTypeExtension` - PHPStan validates
  these by literally constructing the real PHP object at analysis time
  (it runs inside PHP itself); phpstan-rs embeds no PHP runtime. The
  *format-table* half of `DateTime`/`date()` handling (Phase 77) remains
  genuinely portable and stays scheduled; see also Phase 78, scheduled
  but flagged low-priority for the same underlying reason. The two
  already-documented `strtotime()`/raw-SQL-column-shape gaps above are
  the same root cause, reconfirmed by this audit, not newly found.
- **Sealed-hierarchy extension point** (`AllowedSubTypesRule`) - inert
  even in stock PHPStan without third-party configuration; no natural
  home in phpstan-rs's architecture.
- **`ClassNameUsageLocationCreateIdentifierDynamicReturnTypeExtension`**
  - exists solely for PHPStan's own dogfooding self-analysis; no
  general-PHP-analysis meaning.
- **`InnerFunctionRule`** - phpstan-rs *intentionally* does the opposite
  of this rule (it recurses into and checks nested named-function
  declarations, an explicit in-repo design choice citing the common
  `function_exists()`-guarded polyfill idiom). Replicating PHPStan's
  restriction here would be a regression, not a gap.
- **`EnumCaseOutsideEnumRule`** - structurally impossible in
  phpstan-rs's AST shape (`EnumCase` only ever exists as a field of
  `EnumDecl.cases`, never a free-floating node).
- **`ReadWritePropertiesExtension`** - third-party DI extension-point
  interface; no plugin architecture to attach it to.

---

# Completed phase history

## Phases 1-7: foundations

1. **PHPDoc doc comments.** Capture and parse `/** ... */` (`@param`,
   `@return`, `@var`).
2. **PHPDoc types.** Wire tags into the registry; array shapes and
   generic type arguments; `@template` generics with call-site
   substitution.
3. **Flow-sensitive narrowing.** Narrow variable types across `if`/
   `while` branches (`instanceof`, equality, common predicate
   functions).
4. **Properties and class context.** Property types, constructor
   promotion, trait `use`, `self`/`static`/`parent` resolution.
5. **Built-in stub generation.** Generated from real php-src source -
   superseded by Phase 8's broader phpstorm-stubs corpus.
6. **Undefined-variable detection.** By-ref tracking; `foreach`/`catch`/
   destructuring target binding; array/shape index access. **Re-audited
   2026-08-30** via `cargo llvm-cov`: found the largest gap in this
   project's history to that point - `Expr::Closure`/`ArrowFn`/`Match`
   had **no handling anywhere** in the checker, so every diagnostic was
   invisible inside any closure, arrow function, or `match` body
   (2221 -> 2321 first-party diagnostics once fixed, all spot-checked
   genuine). Fixed with real PHP capture semantics (`use (...)`-only for
   closures, implicit whole-scope-by-value for arrow fns, `$this`
   auto-bound for non-static closures). Found 4 more independent bugs
   chasing this: a static method's `$this->prop` wasn't flagged; closure/
   arrow-fn type hints bypassed namespace resolution; multi-line `@param`/
   `@return` tags got truncated mid-parse; an unsealed-shape `...<K, V>`
   marker aborted an entire array-shape parse instead of just that field.
   *(Unplanned) Real-world hardening pass* - a detour fixing dozens of
   parser/lexer gaps (PHP 8.1 enums, DNF types, `declare()`, goto/labels,
   asymmetric visibility, PHP 8.5 clone-with) against Piwigo + php-src's
   own test corpus + Piwigo's `vendor/` (17387 files).
7. **LSP / incremental / tooling.** Incremental multi-file `Project`
   (re-check one changed file), a transitive cross-file dependency graph,
   a real Content-Length-framed JSON-RPC LSP server. **Re-audited
   2026-08-30**, found and fixed a Phase 9.5 regression: `declared_names`
   started returning FQNs, but the invalidation BFS looks names up
   bare-keyed - silently broke cascading invalidation for every
   namespaced class.

## Phase 8 - Ingest `jetbrains/phpstorm-stubs` - DONE

Vendors the real 558 stub files (of 1052 total) via `build.rs`, resolving
1000+ real function signatures instead of two hand-picked extensions.

## Phase 8.5 (unplanned) - Stub arity precision - DONE

Two independent bugs in how a stub's minimum-argument count gets
generated, both real false positives on common functions.

- **Version-gated parameters counted as permanently required.**
  phpstorm-stubs marks "this parameter only existed in versions X-Y" via
  a `#[PhpStormStubsElementAvailable(from, to)]` attribute on individual
  parameters rather than separate per-version signatures - neither the
  parser nor `php_stubs.rs` recognized it, so `array_map($cb, $array)`
  (2 args) was flagged as needing 3. Fixed at the **lexer** level (not
  just stub generation, since real projects vendor phpstorm-stubs
  themselves): a closed-range parameter with no default is dropped from
  the token stream entirely.
- **A stub modeling only its highest-arity overload.** `array_keys(['a'
  => 1])` and `mt_rand()` (both real, valid lower-arity forms) were
  flagged too - fixed via `DocBlock`'s new `optional_params` (a
  `[optional]` `@param` marker treated as an implicit default). Found a
  second bug verifying this: the `@param` parser never matched a
  **by-ref** parameter's name at all (`&$matches`, since it looked for a
  bare `$name`) - this was the actual live cause of false positives on
  `preg_match`/`preg_replace`/`str_replace` against Piwigo's own
  vendored older phpstorm-stubs release.

Verified at real scale (18923 files): 46791 -> 42192 diagnostics, zero
new false positives, 293/293 tests passing. A residual ~384 "expects at
least" diagnostics from a structurally different, harder pattern
(interleaved multi-version parameter lists) were left - see Known
Limitations above.

## Phase 9 - Undefined-symbol rule family - DONE

Undefined-class/function/method/property(-read)/static-access detection,
with real `__call`/`__get`/`__callStatic` exemptions and self/parent/
static-aware resolution. Found and fixed several bugs along the way
(leading-backslash normalization, `use`/`use function` imports parsed
but never read, `Registry::index` never recursing into `if`-guarded
polyfill declarations). Finishing the last two rules surfaced 3 more real
gaps: backed enums' `->value`/`->name` and `cases()`/`from()`/`tryFrom()`
weren't registered at all (~2800 false positives); `&&`/`||`
short-circuit narrowing wasn't modeled; `instanceof` narrowing only ever
matched a bare variable, never a property chain (fixed via a new
`narrowing_key` helper).

## Phase 9.5 (unplanned) - Namespace-aware registry - DONE

Undefined-method detection was reverted after real-scale testing found
it unsafe: zero namespace modeling meant a cross-package class-name
collision (1595 distinct colliding names found across Piwigo+vendor)
could silently overwrite one class's method table with an unrelated
one's. Fixed properly with `registry::NameContext` (namespace + `use`-
import map, tracked positionally per file), re-verified safe at scale
(22,900 false positives -> 3, all genuine). Found 4 more bugs verifying
this (multi-parent interface walking, an `!`-vs-`instanceof` operator-
precedence bug, two pseudo-type name collisions). **Re-audited
2026-08-30** with fresh namespaced tests, found 4 more variations on
"something that used to be a harmless bare name became visibly wrong
once namespace-qualification existed": `lookup_own_signature`'s
top-level-function lookup used raw names not FQNs; `instanceof self`
wasn't resolved against the enclosing class; `@return $this`/
`self::RFC_*`-style pseudo-types weren't recognized; `catch (SomeException
$e)` built its type from the raw AST name with zero resolution.

## Phase 10 - Class-level generics - DONE

`Type::Object` now carries real type arguments (`Collection<User>` ->
`Object("Collection", [Object("User", [])])`). A class's own `@template
T` is recognized across its methods/properties. Deliberately scoped to
call-site substitution only - variance, bound enforcement, and
`@extends`/`@implements` chain substitution moved to Phase 10.5.

## Phase 10.5 (unplanned) - Generic variance and inheritance chains - DONE

- **Inheritance-chain substitution.** `@extends Parent<Arg>`/`@implements
  Iface<Arg>` now parsed and resolved. Found and fixed a real bug: the
  first implementation flat-merged the *entire* ancestor chain's
  template bindings into one map, letting an unrelated ancestor's
  same-named `T` silently overwrite another's (`T` is by far the most
  common single-letter generic name) - fixed by scoping every lookup to
  a specific target ancestor via new `_with_owner` lookup variants.
- **Template bound enforcement.** `@template T of Bound` now actually
  checked (`check_generic_bounds`), not just parsed past. Found 3 real
  bugs verifying this: `@template-covariant`/`@template-contravariant`
  tags weren't recognized as templates at all (15 false positives);
  `is_assignable` never treated native `object` as a universal
  supertype (~25 false positives, real trigger: Doctrine's
  `EntityRepository<T of object>`); an enum was never registered as
  implementing `UnitEnum`/`BackedEnum` in its own hierarchy data. Also
  added a "don't know, don't guess" leniency skipping a bound check
  entirely when either side mentions a class with no real registered
  data (a PHAR-bundled dependency, or a local `@phpstan-type` alias).
  Net corpus effect: 42192 -> 41255 (bug 1 alone fixed real pre-existing
  false positives unrelated to bounds).
- **Variance enforcement.** `check_variance` flags a covariant `T` in
  parameter position or contravariant `T` in return position. Found a
  real bug immediately: every one of an initial 27 diagnostics was on a
  `__construct` (the extremely common `readonly` value-holder pattern) -
  real PHPStan exempts constructors for exactly this reason (never
  invoked polymorphically); fixed by exempting `__construct` entirely,
  restoring the pre-check baseline (a genuinely variance-clean corpus,
  confirmed via 18 new targeted unit tests, not a broken check).

## Phase 11 - Source positions, levels, config - DONE

Six ordered stages, each its own commit:

1. **Lexer emits real spans** (`token::Position`/`Span`, UTF-8-character
   columns, precomputed line-start table for O(log n) offset lookup).
   Found and fixed a severe performance bug: naive byte-to-line/column
   conversion recounted from a line's own start every call - O(line
   length x tokens) blowup, hanging for minutes on a real 1.25M-character
   generated line already in this project's corpus. Fixed with an
   incremental cache exploiting strictly-increasing tokenization offsets.
2. **Spans threaded through the AST** (`Stmt`/`Expr` wrapped as
   `{kind, span}`, ~450 call sites migrated, Rust's exhaustiveness
   checking making every miss a compile error).
3. **Diagnostics carry real spans.**
4. **Real CLI/LSP output positions** - `into_diagnostics()` now sorts by
   `(line, col)` (internal checks don't run in source order, so this is
   load-bearing, not defensive); LSP output converts to 0-indexed ranges.
5. **Discrete rule-gating levels + strictness parameters.** Each of 16
   diagnostic push sites tagged with a level mirroring real PHPStan's own
   groupings (verified against a fresh `phpstan-src` clone's
   `conf/config.levelN.neon`); `--level N` flag. `strict_mixed_arrays`
   opts into treating an unknown array literal as *not* freely
   compatible with a specific shape (real PHPStan's own level-9
   philosophy) - surfaces 1814 additional real findings on Piwigo,
   default output unchanged.
6. **Baseline mechanism, `clap` CLI migration, TOML config file.**
   `--generate-baseline`/`--baseline` (keyed by path+location+message, so
   a baselined finding survives incidental code shifts); derived `Cli`
   enum replacing hand-rolled flag parsing (a deliberate breaking CLI
   syntax change, acceptable pre-1.0); `phpstan-rs.toml` config with CLI
   flags taking precedence.

## Phase 12 - Bespoke per-function/method type extensions - DONE

See **Open phases** above for what's left (`preg_match`'s remaining
literal-enumeration/tagged-union precision). Shipped: `array_map`/
`array_filter`/`array_reduce` (a two-pass mechanism binding the *other*
argument's element type into a closure's own body-inference before
computing its return type - generic template unification alone can't do
this for an untyped closure param); `sprintf`/`printf` format-string
validation against literal arguments (found and fixed a real unguarded
panic in the `array_filter` special case, and a missing
`non-negative-int` pseudo-type along the way: fixing the latter alone
dropped the full corpus 48496 -> 48243); `array_column`/`array_combine`/
`array_fill`/`array_fill_keys`/`array_chunk`; `is_array`/`is_string`/
`is_int`/`is_callable` narrowing predicates (`class_exists`/`in_array`/
`method_exists`-style existence checks deliberately excluded - they
check "does this exist," not "what type is this value now"); and
(unplanned follow-on) `preg_match`/`preg_match_all`'s by-ref `$matches`
shape, via a new structure-only PCRE parser
(`src/regex_groups.rs`) - see **Open phases** above for the full
breakdown and the one item still deferred.

This same follow-on found and fixed a real, pre-existing gap in
`write_index_into` (`checker/assignment.rs`): it had no `Type::Union`
arm at all, silently leaving a `Union`-typed container's write untracked
(traced live to `PiwigoInfosSender::send`). Fixing that - combined with
`preg_match`'s own new `Union([matched, empty])` unconditional shape
(Finding E) making `Type::Union` values "stick" through reads and writes
instead of collapsing to `Mixed` at the first opportunity - surfaced a
second, more serious real hang: `merge_scopes` (`checker/control_flow.rs`,
run once per `if`/`else`) and `write_index_into`'s new `Union` arm both
call `union_types`, which had no cap on how many distinct members it
would accumulate. A real corpus function with many sequential/nested
branches assigning genuinely distinct array shapes to the same variable
grew that variable's own union member count combinatorially, each
merge's `contains`-based dedup scan (and each member's own recursive
`PartialEq`) growing right along with it - confirmed by attaching `gdb`
to the hung process and reading the actual stack (`Type::eq` called from
`union_types` called from `merge_scopes`/`write_index_into`), not
guessed. Fixed with a defensive `MAX_UNION_MEMBERS` cap in `union_types`
(`checker/expr.rs`) collapsing to `Mixed` past 12 members - the same
`MAX_GROUP_DEPTH`/`MAX_EXPR_DEPTH` widening-for-termination precedent
Phase 20 already established, and a stable fixed point (`union_types`
already treats `Mixed` as absorbing, so a variable that trips the cap
once stays collapsed rather than re-paying the cost every subsequent
merge).

**Known follow-up, deliberately not attempted here**: this cap is the
*minimal correct* fix for the termination risk, not the *most precise*
one. Real PHPStan instead **generalizes** same-shaped array types -
several `ArrayShape`s that share the same key set get merged into one
shape with unioned field types, rather than kept as separate union
members - which would preserve real precision for exactly the
many-branches-same-variable pattern that trips this cap today, instead
of falling back to `Mixed`. Building that generalization is a
meaningfully-sized feature in its own right (deciding when two shapes
are "the same shape family," merging their fields correctly) - the same
scope class as the already-deferred Phase 14 (refinement/accessory
types), not a small tweak to land alongside an unrelated hang fix. Left
as a real, named future item: replace/augment the hard `MAX_UNION_MEMBERS`
cap with shape generalization by shared key-set before falling back to
`Mixed`.

The same corpus re-check (frozen-corpus count 249 -> 241) found and fixed
two more real, pre-existing gaps along the way, both surfaced by Finding
E's `Union`-awareness the same way `write_index_into`'s own gap was:

- **`check_assignment_target`'s `Index` arm only exempted the outermost
  key of a write target from `check_shape_key_access`'s "does this key
  exist" check, not any index nested *inside* that target's own base.**
  `$piwigoInfos['updates'][] = [...]` (`PiwigoInfosSender::send`, a real,
  first-time nested append with no prior flat `$piwigoInfos['updates'] =
  [];`) routed its own inner `$piwigoInfos['updates']` sub-expression
  through `check_expr` as a genuine read, wrongly flagging "no key
  'updates'" the moment `$piwigoInfos`'s `Union`-typed shape started being
  inspected for real. Fixed by recursing through
  `check_assignment_target` itself (not `check_expr`) whenever a write
  target's own base is *another* `Index`, generalizing the existing
  single-level exemption over any nesting depth - the same way
  `bind_assignment_target`'s own `Index` arm already does for the write
  side. Also fixed a second, identical-shape false positive this same bug
  caused (`StatsPageRenderer::getMonthStats`, "no key 'month'").
- **`foreach`'s own element-type inference only ever handled a plain
  `Type::Array` subject, falling to `Mixed` for a `Type::ArrayShape`
  one**, discarding real, known per-field types. Real corpus impact
  (`MaintenanceActionDispatcher::dispatch`): `foreach ($versions as $key
  => $value) { $versions[$key] = preg_replace(..., $value); }` over a
  `$versions` shape whose fields are all plainly `string` - with `$value`
  inferred as `Mixed`, `preg_replace`'s own subject-dependent stub
  returned a broader type that leaked an `array<string>` possibility into
  `$versions`'s own dynamic-key-write-widened type, which then stuck
  around (rather than silently collapsing) once `Union` values stopped
  being bailed on immediately. Fixed by joining every field's own type
  (`union_types`-folded) when the `foreach` subject is a `Type::ArrayShape`
  - exact when every field shares one type (the common case both this bug
  and the fix target), still sound but coarser when they don't.

One further diagnostic traced to an **already-tracked** limitation, not a
new bug: `DerivativeImage::build`'s `getByType()` argument mismatch
(`int|string` where `string` is expected) comes from `array_keys()`'s own
lack of a real key-type parameter, the existing `Type::Array` key-type row
in the Known limitations table above (tracked under Phase 15).

**Follow-on investigation, 2026-09-02**: a further corpus re-check (using
`--config phpstan-rs.toml <explicit files>` for bisection) surfaced a
`CategoryUpdateController::__invoke` false positive ("no key 'visible'")
that looked, at first, like a deep, alarming bug: checking that file
alone (or its whole directory) inferred `$update`'s type correctly, but
adding `src/Piwigo/Category/Projection/Category.php` (the `instanceof`
target it narrows `$category` against) to the *same* invocation changed
the result and silently dropped every union member that included
`'visible'`. That framing turned out to be a real but misleading
symptom, not the actual mechanism - direct tracing (via `gdb`-style
targeted `eprintln!` instrumentation on `always_terminates`, then a
from-scratch minimal repro) found two independent, genuinely
general, always-reproducible bugs, neither specific to cross-file
context at all:

- **`apply_equality_narrowing`'s exclusion branch (`checker/
  narrowing.rs`) treated "the other side of a `!==` comparison has this
  *general* static type" the same as "the other side is one specific,
  already-known value"** - sound for `Null`/`True`/`False`/a literal (its
  only real callers before this), unsound for a general type like `Bool`
  (which covers *two* distinct values): `?bool $a` narrowed to `bool` by
  `$a !== null`, then compared `$a !== $b` (`bool $b`, itself not a
  literal) wrongly collapsed `$a` to `Type::Never` the moment
  `$category->visible`'s real type resolved precisely to `Bool` (which is
  why the two-file case differed - `Category.php` not being checked at
  all left `$category->visible` as `Mixed`, which the existing `target_ty
  == Type::Mixed` bail already skips). A `Never`-typed last statement
  then made `always_terminates` (`checker/control_flow.rs`) wrongly treat
  the whole `if`-branch as dead code, discarding its `$update['visible']
  = ...` write entirely from the merged result. Fixed with a new
  `is_known_single_value` gate: only `Null`/`True`/`False`/a literal
  authorize the exclusion; anything else leaves the narrowed type
  unchanged, this project's usual "don't guess" default. Minimized,
  single-file repro: `function test(?bool $a, bool $b) { if ($a !== null
  && $a !== $b) { $update['flag'] = $a; } echo $update['flag']; }`.
  `narrow_type_to` (the `===`-true-branch counterpart) doesn't share this
  bug - for a non-`Union` original it always returns the target
  outright, sound regardless of whether the target is a literal or a
  general type, since `===` between statically-incompatible types is
  dead code either way.
- **`check_expr`'s own `Ternary` arm (`checker/expr.rs`) wrote
  `merge_scopes(then_locals, else_locals)` back into the *real* `locals`
  afterward, copying `StmtKind::If`'s own pattern for a construct that
  isn't actually a control-flow statement.** A ternary's condition
  narrowing a variable within evaluating its own `then`/`else`
  sub-expressions is correct and already achieved by the existing
  per-branch clones; writing that back into the surrounding scope isn't -
  once the ternary as a whole finishes, only its *value* matters to
  whatever contains it, not which hypothetical branch's narrowing might
  have applied. Real, confirmed impact: `$collected === [] ? null :
  preg_replace('/.../', '', $collected[0])` narrowed `$collected`
  (declared `list<string>`) down to the literal empty shape on the
  condition's own true branch, then permanently unioned that into
  `$collected`'s real tracked type - corrupting a *second*, independent
  evaluation of the same sub-expression (`check_property_assignment`'s
  own redundant `infer_expr` call on the same right-hand side) into
  seeing `$collected[0]` as `Mixed` instead of `string`, defeating
  `preg_replace`'s own subject-type-dependent return-type precision.
  `infer_expr`'s own parallel `Ternary` handling never had this bug - it
  only ever receives `&Locals`, not `&mut Locals`, so it structurally
  can't write back at all; the fix removes the write-back from
  `check_expr`'s side to match. Confirmed dead code, not just
  theoretically unnecessary: removing it left all 816 then-existing
  tests passing unchanged.

Both fixed and verified via the disable-and-confirm-failure technique;
frozen-corpus count fell 249 -> 241 (the equality-narrowing fix, plus
incidentally correcting a `never`-typed leak in
`CategoryService::updateUppercats`'s own diagnostic message) -> 240 (the
ternary fix, closing the false positive it had briefly introduced along
the way). No diagnostic regressed at any step.

**Closing round, 2026-09-02**: the two remaining named follow-ups -
`preg_match`'s own literal-enumeration/tagged-union precision, and the
`union_types`/`MAX_UNION_MEMBERS` array-shape-generalization follow-up -
both closed, plus a third gap found while designing the work (the
already-shipped flat shape treated every optional group uniformly as
"maybe entirely absent," when real `$matches` only ever truncates from
its own trailing end). Grounded directly in real PHPStan source
(`RegexGroupParser.php`, `RegexAlternation.php`, `RegexCapturingGroup.php`,
`RegexArrayShapeMatcher.php`) and verified against real PHPStan 2.2.8's
own `\PHPStan\dumpType()` output and live `php`, not recalled from a
single reading - this caught four real design bugs a first read of the
source missed (a sibling alternation branch's earlier group is
force-filled with `''`, not removed; the single-optional-group "without"
variant is a bare `{0: subject}` shape, not "every group but the target";
`in_optional_quantification` resets at every non-quantification AST node
including a non-capturing wrapper, unlike the broader `optional`
propagation; a lookaround's own interior must never leak into its
enclosing literal computation). Full design in
`plans/linked-sniffing-twilight.md`.

Shipped, as four separately committed and verified pieces
(`src/regex_groups.rs`, `checker/expr.rs`, `checker/narrowing.rs`):

- **Literal enumeration** for a constant-only capture group - `(cat|dog)`
  now infers `'cat'|'dog'`, not generic `string` - via a parallel
  `literal_values: Option<Vec<String>>` computed alongside the existing
  structure scan (concatenation as cross-product, alternation as union,
  bounded quantifiers as repetition, all capped at `LITERAL_ENUM_LIMIT`
  (32)), with `^`/`$`/lookarounds correctly contributing a known empty
  string rather than bailing, and a group repeated more than once
  externally correctly losing its own constant typing.
- **Tagged unions** for the two shapes real PHPStan itself special-cases:
  exactly one optional top-level group, and exactly one top-level
  alternation - full array-shape variants (the group/branch present, or
  not) instead of one shape with a bare `optional` field, including the
  `alternation_wrapped_in_optional_quantifier` signal needed for
  `(?:(a)|(b))?`-shaped patterns real PHPStan still tags.
- **Trailing-vs-interior optional-group fix**: an optional group with
  something *required* after it is always present in `$matches` (`''`
  when it didn't itself capture), not "maybe entirely absent" - only a
  *trailing* optional group can really vanish. `''` is also now a real
  possible value for any optional group except the literal last one,
  independent of trailing/interior.
- **`union_types` array-shape generalization**: `generalize_by_key_set`
  merges `ArrayShape`s sharing an identical field-name set (unioning
  their field types) before the `MAX_UNION_MEMBERS` (12) collapse to
  `Mixed` fires, rather than collapsing unconditionally - closing the
  known follow-up left by this phase's own earlier hang fix (see above).
  Confirmed against the live corpus: removed a false positive in
  `SiteUpdateSubController::handle` where `$counts`'s own fields (written
  across many sequential conditional branches) had been collapsing to
  `Mixed` past the cap, wrongly failing `SiteUpdateView`'s own
  constructor-argument check. Frozen-corpus count 240 -> 239, zero
  diagnostics regressed.

**Deliberately still out of scope**: real PHPStan's own literal-value
refinement into `non-empty-string`/`non-falsy-string`/`numeric-string`
accessory intersection types for a regex group. This conflicts with an
existing, deliberate, evidence-backed project-wide policy of aliasing
every string-refinement pseudo-type to plain `Type::String`
everywhere (`types.rs`'s `parse_type_string`) - building it just for
regex groups while every `@param non-empty-string $x` elsewhere keeps
resolving to plain `string` would be an inconsistent special case.
Sequenced as Phase 14's own project-wide effort instead (new refinement
`Type` variant(s), `is_subtype`/`union_types` rules, PHPDoc's own
pseudo-type parsing), with a short follow-on afterward to have this
phase's own literal-vs-plain dispatch use those types instead of falling
back to `string` in the non-literal case.

## Phase 13 - Narrowing precision and remaining false-positive causes - DONE

Three real, independent namespace/pseudo-type resolution bugs found and
fixed via a direct audit, 2026-08-30 (each had been silently live for an
unknown stretch): a native type hint's leading `\` was stripped before
namespace resolution ever saw it (48243 -> 44716); a quoted PHPDoc
literal-string type (`'ASC'|'DESC'`) wasn't recognized, the quotes
becoming part of a mangled "class name" (44716 -> 44525);
`resolve_pseudo` never resolved `self`/`static`/`parent` wrapped in a
`Union` (`?self` factory-method returns) (44525 -> 44353). Real
first-party `src/` diagnostics dropped 2327 -> 2270 across these three.
Also shipped, folded into this phase: **Composer integration for
`vendor/` type context** (`src/composer.rs` - auto-loads a project's
installed autoloader for symbol resolution without reporting on vendor
code itself; `--no-composer` 2269 diagnostics -> auto-detected 1181,
with the 5 residual "not found" cases traced to two understood, accepted
edge cases: first-party code under a non-`src/` PSR-4 root, and
Composer's own bootstrap `ClassLoader` which is structurally invisible
to any of the four autoload mechanisms this feature reads).
**Closing round, 2026-09-02**: every remaining item on this phase's own
list was checked directly against current code and the live frozen
corpus (239 diagnostics) before any fix was designed - most turned out
to already be fixed in earlier phases, with this list simply never
updated: ternary branch narrowing (`check_expr`/`infer_expr`'s own
`Ternary` arms, correct since Phase 8), `always_terminates` recognizing
`switch`/`try`/`match` (fully implemented since Phase 8), `for`/
`foreach`/`switch` sharing one unforked scope (false - all three already
fork/narrow per-branch, Phase 29), and array-literal element types not
tracked (false - `infer_array_literal_type` already builds a full
`ArrayShape`, since Phase 8).

Five real findings shipped:

- **Finding A - `getenv()`'s dynamic return type.** The real stub
  declares `getenv(): array|string|false` unconditionally; real PHP only
  returns `array` for a truly zero-argument call, and only `string|
  false` when `$name` is a real, non-null string (verified against real
  PHPStan's own `dumpType()`, including the easy-to-get-wrong
  `getenv(null)`/`getenv(local_only: true)` cases, both of which stay
  broad). Traced every `getenv(` call site in the corpus by its actual
  data flow: 26 of 239 diagnostics shared this one root cause (21
  named-argument sites, 5 zero-argument `array_merge(getenv(), ...)`
  sites) - by far the largest single fix of this round, and the same
  root cause behind both `strtotime` sites and both `count()`-adjacent
  sites this phase's own "batch" item had named.
- **Finding B - `is_countable()`** missing from the narrowing-predicate
  table, closing a real `count()`-argument false positive
  (`SectionPopulator::computeMetaRobots`, the only real corpus call
  site).
- **Finding C - `BinOp::Div`** hardcoded `Type::Mixed`; real PHPStan
  gives `int|float` unconditionally unless `float` is provably involved
  (verified via `dumpType()`, including the fully-unknown-operand case).
  Not diagnostic-count-neutral by design (today's `Mixed` was silently
  compatible with everything, so no real division site had ever been
  type-checked before) - one site dropped as a bonus
  (`IntroSubController.php:598`'s own `filesize: mixed` field, fed by a
  division result that used to be silently `Mixed`).
- **Finding D - `@property`/`@property-read`/`@property-write`**
  class-level PHPDoc tags, entirely unparsed until now. Structurally
  identical to the already-parsed `@var Type $name`; wired into the same
  property map real fields use, so existing inheritance-aware lookups
  pick it up for free. `vendor/php-openapi/openapi`'s own `OpenApi`
  class declares `@property Paths $paths` - confirmed via a minimal
  repro that this specific corpus diagnostic (`tools/
  openapi-generate-input-dto.php:95`) still doesn't clear even with this
  fix, traced to a separate, pre-existing gap: the receiving variable's
  own type is a `SpecObjectInterface|OpenApi` union (from a library
  author's plain `@return` fallback for tools that don't parse
  `@phpstan-return T` generics, which this project doesn't), and the
  interface half of that union genuinely has no `$paths` of its own - a
  real "every union member needs the member" correctness rule, not a
  bug this fix could have closed. The feature itself is real and
  verified via four new tests.
- **Finding E - trait method bodies checked once per using class**, not
  standalone. `flatten_traits` already merges a trait's own members into
  every using class before checking starts, so no lookup logic needed to
  change - only *which context* a trait's methods are checked under. A
  new `Registry::classes_using_trait` walks past intermediate trait
  users too (a trait used only by another trait still resolves against
  the real, final class). A trait used by 2+ classes now gets checked
  2+ times, so a host-independent bug in its own body is deduped by
  `(span, message)` rather than reported once per host; an orphaned/
  library trait with no real user still gets checked standalone, exactly
  as before. **Lowest confidence of the five**: zero traits exist
  anywhere in this corpus, so verified only via six new tests, never
  against real corpus behavior.

Confirmed real but deliberately left open, each named precisely rather
than left in a vague "batch": `BinOp::Pipe` (zero corpus usage, needs new
callable-resolution infrastructure); `StmtKind::Try`'s own catch-merge
gap (a false negative, not detectable by corpus diffing); a cluster of
`__construct()` mismatches individually traced to three already-tracked/
documented boundaries (`array_keys()`'s key-type imprecision, Phase 15;
a local `@phpstan-type` alias, a permanent design boundary) plus two
genuinely new gaps (Doctrine's `EntityManager::getReference()` generics
living only on its interface; `CommentRepository`'s own multi-variable
compound-boolean narrowing) - see the Known limitations table and
"Known but not yet scheduled" list above for all of these.

Corpus count: 239 -> 213 (Finding A) -> 212 (Finding B) -> 211 (Finding
C) -> 211 (Findings D and E, unchanged as expected). Zero regressions at
any step.

## Phase 14 - Refinement/accessory types - DONE

Real PHPStan's own representation (confirmed by reading `vendor/
phpstan-src/src/Type/Accessory/*.php` directly): a *string* refinement
is `IntersectionType([StringType, AccessoryXType])`; an *array*
refinement (`list`/`non-empty-array`) is `IntersectionType([ArrayType,
AccessoryArrayListType/NonEmptyArrayType])`; an *int* range
(`positive-int`, `int<min, max>`, ...) is its own standalone
`IntegerRangeType`, not an accessory. Built by reusing this project's
existing `Type::Intersection` variant (PHP 8.1 intersection types)
rather than inventing a new mechanism - its `is_subtype`/`is_assignable`
rules already generalized correctly for a marker-plus-base shape.

New `Type` variants: `IntRange(Option<i64>, Option<i64>)` (fully
replaces `Int`'s own range, the same relationship `IntLiteral` has to
`Int`); `FloatLiteral(f64)` (mirrors `IntLiteral`/`StringLiteral` for a
PHPDoc float-literal union); `Accessory(AccessoryKind)` (`NonEmptyString`/
`NonFalsyString`/`LiteralString`/`NumericString`/`ArrayList`/
`NonEmptyArray`, only ever meaningful inside an `Intersection`).
`parse_type_keyword_or_class` now constructs these instead of aliasing
every refinement pseudo-type to its plain base; `callable-string`/
`trait-string`/`interface-string`/`enum-string` stay aliased (no
verifiable content to attach a real type to).

Four narrowing sources added, each verified against real PHPStan's own
`dumpType()`: `$x !== ''` narrows a `string` to `non-empty-string`;
`!empty($x)` narrows a `string` to `non-falsy-string` (not merely
non-empty - `empty()` is also false for `'0'`) and an `array` to
`non-empty-array`; `count($x) > 0`/`count($x) >= 1` also narrow an
`array` to `non-empty-array`; `is_numeric($x)` narrows a `string` to
`numeric-string`, unblocking its own already-shipped narrowing code
(Phase 32 Finding G), which had documented itself as blocked on this
exact type not existing yet. ROADMAP's own prior suggestion that
`trim()` had a "known-safe case" worth narrowing on was checked
directly and found empirically false - trimming can strip an already-
non-empty string down to `""` regardless of its input.

**The largest real risk turned out to be structural, not conceptual**:
grepping every direct `Type::Array`/`Type::ArrayShape` pattern match in
the checker found 25 real call sites that stopped recognizing a
`list<T>`/`non-empty-array<T>` value as an array at all the moment it
became a real `Intersection` instead of plain `Array(T)` - confirmed
live (`foreach` over a `list<T>`-typed variable silently inferred
`Mixed`). Fixed via two shared helpers (`array_element_type`,
`is_array_like`) adopted across `foreach`, the array-union `+`
operator, `??`'s own array-widening, dynamic-key array writes and
destructuring assignment, `sprintf`'s own format-string check (an
active regression in already-shipped bug-catching, not just missed
precision), and by-ref argument widening - most of the remaining 25
sites already recursed into `Intersection` generically via an existing
`Union` sibling arm and needed no change at all, confirmed by reading
rather than assumed. A second, more severe version of the same class of
bug was caught by the corpus re-check itself: `registry::is_assignable`
was missing the same dedicated Intersection-vs-Intersection rule
`types::is_subtype` got, so its own generic dispatch intercepted a
same-shaped pair (even `list<string>` vs `list<string>`) before ever
reaching the correct logic - unfixed, this alone inflated the corpus
count to 1861 before the real, intended ~875 was reached.

Six bespoke array-function special cases, each verified via
`dumpType()`: `array_values` (new - always `list<V>`, any input shape),
`array_unique` (new - never preserves list-ness), `array_filter`/
`array_combine` (existing - now explicitly strip any accessory marker
instead of passing it through unchanged), `array_map`/`array_slice`/
`array_reverse` (the latter two new - preserve the input's own markers
with default arguments only, confirmed to break with `preserve_keys:
true`). `array_merge` and every other stdlib array function beyond
these six are left untracked - a real, open-ended surface with no
bounded stopping point.

**A deliberate, explicitly confirmed decision, not a default**: a plain
`Type::Array(_)` - even with a known element type - never satisfies
`list<T>`/`non-empty-array<T>`, matching real PHPStan's own confirmed
strictness (verified live: even a known-element `int[]` fails a
`list<int>` sink) rather than this project's usual leniency-for-
uncertainty default. Confirmed corpus-scale, not theoretical, before
the decision was made: `list<` appears 1575 times across 463 first-
party files. The corpus re-check after this decision landed showed
exactly the scale of impact predicted - 211 -> 1115 diagnostics, the
large majority real `@return list<T>` declarations whose method body
only provably returns `array<T>` - explicitly reviewed and confirmed as
the accepted, correct outcome of the decision, not a regression,
before continuing to build the rest of the phase on top of it. Task 7's
own array-destructuring fixes then resolved a large share of that
(1115 -> 875), the rest left as real, uncorrected signal about the
corpus's own type-precision gaps rather than individually triaged here.

## Phase 14.5 (unplanned) - list<T> false-positive cleanup - DONE

A round of bucketing the Piwigo corpus by normalized message shape (the
same Phase 22 methodology) against Phase 14's own 875-diagnostic
closing figure, on the premise that "real, uncorrected signal about the
corpus's own type-precision gaps" deserved actual triage rather than
staying an assumption. It mostly wasn't the corpus - five real gaps in
this project's own type inference, each verified against real
PHPStan's own `dumpType()` before being fixed:

- **A spread array literal** (`[...findA(), ...findB()]`) gave up
  entirely, always inferring bare `Array(Mixed)`. Now tracks list-ness/
  shape precision through: two `list<T>`-typed spread sources stay a
  `list<T>`; a plain `array<T>` source degrades list-ness for the whole
  result; a spread source with fully-known fields (an `ArrayShape`)
  merges field-by-field (canonical int keys renumbered, others kept and
  overwritten) rather than degrading - real code building one value
  from another's own `toArray()` shape plus an extra field
  (`UserInfoWithThemeName::toArray()`).
- **`array_merge()`** fell through to its bare `array` native stub,
  discarding all precision. Now merges two known `array{...}` shapes
  field-by-field via the same rule the spread literal above uses
  (shared via a new `merge_shape_field` helper - `Logger::__construct`
  merging a property's own default options shape with a caller-supplied
  one); merges list-like arguments into a `list<T>`; degrades to a
  plain `array<T>` the moment one argument is an unproven plain array.
  General key-aware shape merging when arguments are a *mix* of known
  shapes and plain arrays remains unhandled, along with every other
  stdlib array function beyond the six Phase 14 already covers.
- **A PHP 8.4 property hook's own `set(Type $value)`** was never
  consulted at all - a property write was always checked against the
  property's own declared type, even when an explicit `set` hook
  deliberately accepts a wider native type and coerces internally
  before the real assignment (`CurrentConfig`'s own
  `apiKeyDuration`/`chmodValue`/`defaultFiltersViews`/`filterPages`/
  `randomIndexRedirect`). `PropertySig` gained `set_ty: Option<Type>`,
  populated from the hook's own native parameter type hint (its doc
  comment isn't captured by the parser at all, so no `@param` is
  available - not needed here, since every corpus case already
  declares a wide-enough native hint on its own).
- **`tempnam()`** carries its own real PHPStan dynamic-return-type
  extension beyond its plain `string|false` native stub - a successful
  call always returns a real `non-falsy-string`, confirmed via
  `dumpType()`. The common `$jar = tempnam(...); if ($jar === false) {
  throw ...; }` idiom feeding a `non-empty-string`-typed sink afterward
  was the single largest non-array-related false-positive bucket.
- **`preg_match_all`'s own per-group match array** (`$matches[N]`) was
  built as a plain `Array`, never marked list-like, despite always
  being real, sequentially int-keyed from 0 (one slot per match) -
  confirmed via `dumpType()`.
- **Array-append (`$arr[] = value`) onto an already list-like
  container** (including a freshly-empty `[]` literal) unconditionally
  dropped list-ness, treating a real append exactly like an arbitrary
  dynamic-key write. Real PHP's own append semantics always assign the
  next sequential int key, so the result stays a real, now-provably-
  non-empty list - confirmed via `dumpType()`. This is the single most
  common accumulator idiom in PHP (`$result = []; foreach (...) {
  $result[] = ...; } return $result;`), and by far the largest single
  fix of this round: 771 -> 453 diagnostics on its own.

Every fix was verified via the same location-based diff this project
always uses (a removed diagnostic's location sampled and traced back to
its real cause; a new location individually investigated before being
accepted) - across all five fixes, only two genuinely new locations
ever appeared (both immediately investigated and fixed as part of the
same round: the spread-literal fix's own field-merge gap, and the
array_merge fix's own shape-merge gap), and zero remained unexplained
at the end.

The "array_merge's own key-renumbering isn't tracked" item in **Known
limitations** below is updated to reflect what this round actually
closed versus what's still open.

**A second round** (453 -> 434) found three more gaps, two of them
surfaced only *because* the first round's own increased precision made
them reachable for the first time:

- **The `sort()` family's own real dynamic type-specifying extensions**
  weren't modeled - the generic by-ref-widening rule (Phase 22 Finding
  3) always overwrote a variable's tracked type with the callee's own
  declared parameter type, correct for a real mutation but wrong here,
  since every one of these natively declares a plain `array &$array`
  regardless of what it actually does. Verified via `dumpType()`:
  `sort`/`rsort`/`usort`/`shuffle` always reindex to a real list,
  preserving the input's own element type; `asort`/`arsort`/`ksort`/
  `krsort`/`uasort`/`uksort`/`natsort`/`natcasesort` preserve the exact
  key structure - and so the exact type - unchanged. An extremely
  common idiom (117 first-party files); this fix alone was 453 -> 442.
- **`$x !== []` never excluded the empty shape at all** - the sort()
  fix's own re-verification surfaced it: `is_known_single_value` didn't
  recognize an empty array literal as exhaustively single-valued the
  way it already does a literal string/int. Fixed by extending it, plus
  a `narrow_type_excluding` arm sharing `upgrade_array_to_non_empty`
  with `!empty($x)`/`count($x) > 0`.
- **`array_merge(...$listOfLists)`** - a spread *argument*, not a
  spread array-literal item - was treated as if the whole array-of-
  arrays were one merge argument, giving `list<list<int>>` instead of
  the real `list<int>` (`SearchService::searchAllwords` flattening a
  `word=>list<int>` map's own values). Fixed by unwrapping one level for
  a spread argument specifically.
- **A `catch` clause's own ending state was discarded entirely** after
  the whole `try`/`catch` - `locals` only ever kept the try body's own
  final state (`$body = null; try { ...; } catch (...) { $body = "x";
  }` then reading `$body` afterward, real `RedirectServiceTest` code).
  Fixed the same way every other control-flow join in this checker
  works: merge the try body's own full-completion state with each catch
  clause's, excluding a branch that always terminates - the same
  if/else guard-clause treatment. This ROADMAP had tracked the missing
  merge as a real gap but assumed it only produced a false *negative*,
  undetectable via corpus diffing; found instead to be a real,
  corpus-confirmed false-positive cause too, and removed from **Known
  limitations** below.

Three more new locations surfaced along the way, each individually
traced rather than assumed: two are real, latent true positives the
round's own improved precision newly surfaced (one already covered by
the "no multi-variable compound-boolean narrowing" limitation below;
one confirmed matching real PHPStan's own behavior directly) - both
accepted, neither a phpstan-rs bug; the third (the malformed-looking
`array_merge(...$listOfLists)` output) was the spread-argument bug
above, fixed rather than accepted.

**A third round** (434 -> 397) found one foundational gap plus three
more bespoke stdlib functions, again mostly surfaced by fixing the
foundational one:

- **`array_element_type`/`is_array_like`/`is_list_like`/
  `is_known_non_empty_array` had no `Type::Union` arm at all** - the
  single most common way a real variable's type actually arrives: a
  control-flow merge (`$x = []; if (...) { foreach (...) { $x[] = ...;
  } }` leaves `$x` a `Union` of the "0 iterations" `array{}` branch and
  the "≥1" real list branch). Every one of `sort()`/`array_merge()`/
  `foreach`/etc. silently stopped recognizing such a value as an array
  at all - the same class of regression Phase 14's own Task 7 audit
  exists to prevent, just for `Union` instead of `Intersection`. An
  empty `ArrayShape` branch contributes no real element-type constraint
  and is skipped in the merge rather than folded in as its own
  degenerate `Mixed` (which would otherwise collapse the *whole* merged
  element type down to `Mixed` the moment any branch was ever empty).
  Found via `LatteTemplateFiles::discover`'s own real if+foreach+append+
  `sort()` pattern.
- This fix's own re-verification found the identical pattern it fixes
  elsewhere, twice: **`array_keys()`'s native stub declares `@return
  int[]|string[]`** - real but useless without this project's own
  key-type tracking (`Type::Array` has no key-type parameter at all, a
  known limitation, Phase 15). Previously harmless (`array_element_type`
  gave up entirely on the resulting `Union`), the fix above made it
  start resolving to a real `int|string`, surfacing as a regression
  everywhere a caller read one of the keys (`opcache-preload.php`'s own
  `foreach (array_keys($classMap) as $symbol) { str_starts_with($symbol,
  ...); }`, real corpus code). Falls back to this project's own usual
  `Mixed` key-type default for a non-literal array argument instead of
  the stub's own misleadingly precise answer. **`array_filter()`'s own
  explicit null-check callback** (`fn ($v) => $v !== null`) is a real,
  separately-recognized PHPStan extension that strips `Null` from the
  retained element type, not just the accessory markers every
  `array_filter` call already loses - verified via `dumpType()`. Real
  Piwigo code (`GalleryController`) builds its own page-items list
  exactly this way.
- **`glob()`** carries its own real dynamic-return-type extension beyond
  its plain `array|false` native stub - a successful call always
  returns a real `list<string>`, confirmed via `dumpType()`
  (`StructuralTest`'s own `findCallSitesInRootPhpFiles`/
  `findCallSitesInBinFiles`, narrowing away `false` via `$paths !==
  false ? $paths : []`).
- **`explode()`** never actually returns `false` at all on a modern PHP
  target - the vendored stub's own `@return string[]|false` reflects PHP
  7's behavior for an empty `$separator`; PHP 8+ throws a `ValueError`
  there instead. Real PHPStan's own dynamic-return-type extension drops
  `|false` entirely and always answers `non-empty-list<string>`,
  verified via `dumpType()` (even for a non-literal `$separator` - there
  is no other way it can return fewer than 1 element). Real Piwigo code
  (`GeneratedFileErrorMapper::lines`) relies on exactly this.

**A fourth round** (397 -> 296) found two gaps unrelated to `list<T>`
itself - both pre-existing, surfaced only because `list<T>` becoming a
real, distinguishable type made them consequential for the first time:

- **`check_method_overrides`'s own return-type check compared the
  *docblock-refined* return type on both sides**, wrongly flagging any
  override that faithfully keeps the ancestor's own native type while
  the ancestor's docblock separately narrowed it further - real PHP
  itself only enforces override-compatibility (Liskov) on the native
  signature, and by default so does real PHPStan
  (`checkPhpDocMethodSignatures` is off unless a project opts in,
  confirmed neither Piwigo's own `phpstan.neon` nor this project's own
  corpus config does, and confirmed directly against real PHPStan with
  the exact shape found). `FunctionSig` gained `return_type_native:
  Type`, computed alongside the existing docblock-merged `return_type`
  and threaded through `merge`/`substitute_class_templates`; the check
  now compares this native-only view on both sides instead. By far the
  single largest fix of any round so far: 397 -> 302 diagnostics on its
  own, all of them real - dozens of Piwigo `*View` classes each
  implementing a shared rendering interface's own
  `pageAssets()`/`exposedStrings()`, correctly matching the interface's
  native `: array` while the interface's docblock alone said `list<T>`.
  A companion test now confirms a genuine native-level incompatibility
  is still caught - this check had no positive-case coverage at all
  before.
- **A static property (`self::$prop`) had no narrowing key at all** -
  only an instance property's `->` chain was handled, so `if
  (self::$prop !== null) { return self::$prop; }` never narrowed away
  `null`. Neither `infer_expr`'s nor `check_expr`'s own `StaticAccess`
  handling consulted `locals` for a narrowed override the way
  `PropertyAccess` already does, even once `narrowing_key` recognized
  the shape - both fixed the same way `PropertyAccess` already works;
  the write side needed no separate wiring, since the generic
  purge-and-update in `bind_assignment_target` already keys off
  `narrowing_key` uniformly. Real Piwigo test code
  (`BrowserTestHelpers::adminSessionCookies`) relies on exactly this.

**A fifth round** (296 -> 287) found one real parser gap, plus
confirmed a second real instance of an already-documented, deliberately
out-of-scope limitation rather than fixing it piecemeal:

- **Real PHPStan's own conditional return type syntax** (`@return
  ($param is SomeType ? TrueType : FalseType)`) fell into the DNF-group
  (`(Countable&Iterator)`) unwrap and got mangled into nonsense - the
  whole ternary text, including the literal `$param`/`is` tokens, got
  treated as an ordinary class name and then namespace-qualified into
  garbage. This project has no per-call-site type-expression evaluation
  to actually pick a branch, so `try_parse_conditional_type` now
  recognizes the shape (the literal `is`/`?`/`:` tokens at the top
  level) and takes the union of both branches instead - collapsing to a
  single type when they happen to be identical. Real shape:
  `NotificationByMailSender`'s own `@return ($action is 'send' ?
  list<string> : list<UserMailNotification>)`.
- Confirmed a second, non-generic real instance of the already-tracked
  "no interface-to-implementation PHPDoc inheritance" limitation (see
  **Known limitations** below, updated) rather than attempting a
  piecemeal fix: a delegating "spy" wrapper's own parameter (no local
  docblock) passing straight through to the interface's own
  docblock-narrowed sink - confirmed directly against real PHPStan that
  it does not flag this (and confirmed it does flag the identical shape
  the moment the interface relationship is removed).

**A sixth round** (287 -> 285) found a real, subtle narrowing bug not
tied to `list<T>` at all:

- **`instanceof`'s own true branch unconditionally replaced the checked
  variable's type with just the target class/interface**, discarding
  whatever it was already known to be - correct for the common
  down-cast (a known parent narrowed to a known child) or nullable-
  narrowing (`?Foo` narrowed to `Foo`) shape, but wrong when the target
  is an unrelated *sibling* interface: real Piwigo code
  (`Template\Renderer::render`) takes a single `View $view` parameter
  through three independent, sequential `if ($view instanceof
  SomeOtherInterface)` checks, each discarding the fact that `$view` is
  still a `View` - a later plain-`View` use after all three checks saw
  a nonsensical accumulated union of bare, unrelated interfaces
  instead. `narrow_instanceof_true` now only replaces the type outright
  when the target is already a sub/supertype of the current one (the
  common cases, unchanged); otherwise it builds a real `Intersection`,
  but only when the current type is itself provably object-like
  (recursing through `Union`/`Intersection`, so a *second* independent
  check still combines correctly after the first one's own if/no-else
  merge) - a provably non-object premise (`Null`, a scalar, ...) is a
  sign of a real, separate upstream inference bug elsewhere, and
  building an intersection out of it would only compound that into
  worse nonsense (found live, while verifying this exact fix: a
  mis-inferred bare `Null` intersected into `null&ImageId`).

**A seventh round** (285 -> 253) found the corpus's biggest remaining
idiom gap:

- **A first-class callable (`intval(...)`, `TagId::from(...)`, PHP
  8.1) used as `array_map`'s own callback was completely unrecognized**
  - `infer_closure_return_with_param_overrides` only matched an
    `ArrowFn`/`Closure` literal, falling to its own `_ => None` for
    anything else, discarding both the mapped element type and any
    list-ness/non-emptiness the input already had. An extremely common
    idiom in this corpus: `array_map(intval(...), ...)` alone is 76
    first-party call sites, over 140 total across several native
    functions and static factories. Resolves the wrapped call's own
    signature (a plain function, a static method, or an instance
    method) and binds `param_overrides` against its declared parameter
    types the same way `infer_call_return_type` already does for an
    ordinary call - including `resolve_pseudo` for a static factory
    returning `self`/`static` (`TagId::tryFrom(): ?self`), caught live
    while verifying this fix as a real regression against a related
    corpus location.
  - Re-verifying that fix's own corpus diff surfaced two more real,
    separately-recognized `array_filter()` extensions this project
    didn't have yet: an `instanceof`-shaped filter callback (`fn ($v):
    bool => $v instanceof Target`) narrows the retained element type to
    `Target` specifically, not just strips `Null`; and `array_filter()`
    with no callback at all - real PHP's own default, dropping every
    falsy element - is exactly `narrow_type_excluding_falsy`'s own job
    (already shared with `!empty($x)`'s narrowing), stripping both `0`
    and `null` for an `int|null` element type, not just `null`. Both
    verified via `dumpType()`.

**An eighth round** (253 -> 221) found the corpus's single most
repeated remaining pattern:

- **Doctrine's `AbstractQuery::getResult()` declares a bare `mixed`
  return with no docblock refinement at all.** Real PHPStan's own
  `phpstan-doctrine` extension gives it a much richer, query-builder-
  aware entity type (full DQL SELECT/FROM introspection, tracking
  `indexBy()` too) this project has no way to replicate - a sizable
  feature on its own - but the narrower, still real and bounded fact it
  always proves is worth having by itself: a non-`indexBy()`'d query's
  own result is always a real, sequentially-keyed list, confirmed via
  `dumpType()` (zero `indexBy()` call sites exist anywhere in this
  corpus, the one case that would break this simplification). Nearly
  every repository class in this corpus follows the identical shape
  (`$entities = $qb->getQuery()->getResult(); return
  array_map(self::toProjection(...), $entities);`), and `getResult()`
  previously typing as plain `mixed` meant `array_map`'s own special
  case (needing an array-like input to activate at all) never even
  engaged - by far the single most repeated remaining false-positive
  cause of any round.

**A ninth round** attempted `@phpstan-return`/`@phpstan-param`/
`@phpstan-var` support (reverted after its own corpus re-check showed a
larger regression than its win, a separate, real `@phpstan-type`
alias-tracking gap it exposed rather than caused - closed properly,
via a narrower fix than either round assumed would be needed, in Phase
14.5's sixteenth round; see **Known limitations** below for what that
turned out to be), then landed one real, smaller fix:

- **`vendor/composer/ClassLoader.php` - Composer's own bootstrap
  loader class - is never listed in any of the generated autoload maps
  at all** (it's what *runs* them, a real chicken-and-egg case:
  `vendor/autoload.php` requires it directly, before the classmap/
  PSR-4 mechanisms it reads even exist), so this project's own
  vendor-context discovery (which only reads those generated maps)
  never found it. Real code occasionally constructs it directly for
  its own plugin/dynamic-autoloading needs (real Piwigo code:
  `PluginRegistry`/`ThemeRegistry`, each registering an installed
  plugin's own PSR-4 mapping via a fresh `new ClassLoader()`), and
  every one of those call sites was flagged as an unknown class.
  Present unconditionally whenever `vendor/composer/` exists at all, so
  always worth loading for context the same way every other generated
  file this project already reads is.

**A tenth round** (219 -> 197) found one bug affecting every value
object in the corpus's own `Common\ValueObject` family at once:

- **`check_method_overrides`'s `static`-covariance escape hatch only
  recognized a bare `Type::StaticRef`, missing the far more common
  nullable shape.** Every `ImageId`/`TagId`/`UserId`/... value object
  implements a shared `NumericId` interface declaring `tryFrom(mixed
  $value): ?static`, and overrides it with `?self` on its own `final`
  class - confirmed directly against real PHPStan 2.2.8 to be a
  legitimate override (a `final` class's `self` and `static` always
  name the same concrete type; the same override on a non-`final` class
  is correctly rejected). `resolve_type` builds `?static` as a plain
  `Union([StaticRef, Null])`, never its own dedicated type, so the
  equality check silently never fired for any of the 22 affected
  classes. Fixed by looking for `StaticRef` inside a `Union` too.

**An eleventh round** (197 -> 188) found the exact same fragility
pattern Phase 32 Finding L had already fixed once for `Union`s, this
time for Phase 14's own new `Intersection`-based string accessory
types:

- **`str_replace`/`preg_replace`'s subject-dependent special case
  gated on `subject_type == Type::String` exactly.** Real code very
  commonly narrows a string first (`if ($source_file === '') { return
  false; }`, `Lang::load()`'s own real shape), which Phase 14 makes a
  real `Intersection([String, Accessory(NonEmptyString)])` rather than
  plain `String` - so the equality check silently stopped firing the
  moment Phase 14 landed, falling back to the stub's own broad
  `string|array<string>|null` return and losing precision at every
  downstream `basename()`/`is_readable()`/`Translator::load()` call.
  Fixed by switching both gates to `types::is_subtype(..., &Type::
  String)`, which already generalizes correctly across `Intersection`
  and literal string types without needing a new special case.

**A twelfth round** (188 -> 182) closed a real gap Phase 14 opened
rather than one it broke - `BinOp::Concat` had never inferred anything
beyond plain `Type::String` at all:

- **String concatenation with a non-falsy operand is itself always
  non-falsy.** Verified via `dumpType()`: `$a . 'suffix'` (any `$a`) and
  `'prefix' . $a` both dump as `non-falsy-string`, since a concatenation
  containing a known non-empty, non-`'0'` substring can never itself be
  empty or exactly `'0'`. Real corpus shape: `sys_get_temp_dir() . '/'`,
  `dirname(...) . '/backups/'`, `'.' . $ext`, each feeding a PHPUnit
  `assertStringStartsWith()`/`assertStringEndsWith()` whose stub
  declares its needle as `non-empty-string`.

**A thirteenth round** (182 -> 158) found the corpus's largest single
remaining `list<T>` gap, the same shape `array_values()` already got
fixed for in Phase 14 itself:

- **`array_keys()` returned a bare `Type::Array(key_ty)`, discarding
  list-ness.** Verified via `dumpType()`: `array_keys()` always returns
  a real, sequentially-keyed list (re-numbered from zero) regardless of
  whether the input array's own keys were already sequential - the
  exact same guarantee `array_values()` already carries. Real corpus
  shape: `ShimClassGenerator::renderClass(array_keys($filters), ...)`,
  `VariableMapBuilder::joinSets()`, and several `foreach (array_keys
  (...) as ...)` sites elsewhere, each feeding a `list<string>`-typed
  sink. By far the largest single fix of this round: 24 locations at
  once.

**A fourteenth round** (158 -> 147) found a real, general unsoundness
bug in equality narrowing, unrelated to Phase 14's own accessory types
- the `===` counterpart of the `!==`-exclusion bug Phase 12 already
fixed once for the opposite direction:

- **`narrow_type_to`'s non-`Union` fallback unconditionally returned
  the comparison target's own type, discarding `original` even when it
  was already narrower.** Real corpus shape (`PictureController::
  __invoke`): `$image_id` (a definite `int`) compared `===` against
  `getRefererImageId()` (`int|null`) inside an `if`/`else` that never
  reassigns `$image_id` at all - the pre-existing code still silently
  widened `$image_id` itself to `int|null` on the true branch, purely
  as a side effect of evaluating the comparison, corrupting every
  downstream use for the rest of the method (11 separate call sites all
  fed by the one corrupted variable). Fixed by keeping `original`
  unchanged whenever it's already a subtype of `target`, falling back
  to `target` only when *it's* the narrower side (the literal-narrowing
  case, `$x === 'foo'`, this fallback exists for).

**A fifteenth round** (147 -> 144) found a real lexer-level gap in how
phpstorm-stubs' own PHP-version-gating attribute is handled, unrelated
to narrowing or accessory types:

- **`#[PhpStormStubsElementAvailable]` can gate a whole function/method
  declaration, not just a single parameter - previously left completely
  untouched.** Real shape: `version_compare()`'s own stub carries two
  full declarations side by side, one closed-range (`to: '7.4'`,
  returning `int|bool|null`) and one open-range (`from: '8.0'`,
  returning `int|bool`) - both reached the parser as ordinary,
  unconditional signatures and got unioned together by the registry's
  own "second unconditional declaration merges onto the first"
  real-overload handling (Phase 32 Finding C), wrongly reintroducing the
  pre-8.0 `null` return into a target this project always models as PHP
  8.0+. Fixed with two new lexer helpers, `upcoming_is_function_
  declaration`/`skip_whole_versioned_declaration`, applying the same
  "a closed range is superseded, drop it entirely" principle the
  existing parameter-level handling already had, generalized to a whole
  declaration's own `;`-terminator or balanced `{...}` body.
  Investigated after finding `usleep()`'s own `@param int<0,max>`
  refinement is real but confirmed, via a direct `dumpType()`/argument
  check against real PHPStan, to never actually be enforced for
  argument-type checking even at `--level max` - a separate, narrower
  stdlib-specific PHPStan quirk left alone rather than chased (1
  diagnostic, high uncertainty about a safe general fix).

**A sixteenth round** (144 -> 97) bucketed by normalized message shape
as usual and found eight independent bugs, two of which (docblock tags,
generic template scoping) turned out to be root causes behind a large
fraction of the round's whole `list<T>`/`array<T>` bucket rather than
one-off fixes each:

- **`docblock.rs` never recognized a `@phpstan-`-prefixed sibling of
  `@param`/`@return`/`@var`/`@template*`/`@extends`/`@implements`** - a
  real, common convention (582/437/186 occurrences of the first three
  alone across this project's own vendored corpus, mostly Doctrine
  ORM/DBAL's own stubs) for giving PHPStan a more precise type than a
  native/plain-doc signature carries for other tooling, e.g.
  `EntityRepository::findBy()`'s native `@return array` staying loose
  while `@phpstan-return list<T>` carries the real generic type. Added
  as a second, override-priority pass over the same tag list (Phase
  36's own first item, done ahead of that phase's remaining tags).
  Surfaced two more real gaps once real corpus code exercised it for
  the first time: Doctrine's own `@phpstan-param`/`@phpstan-return`
  routinely name a local `@phpstan-type` alias this project doesn't
  parse at all (`Registry::is_assignable` now treats an `Object` on the
  expected side as compatible with anything when that name isn't a
  real registered class, excluding the native `object` pseudo-type),
  and `DriverManager::getConnection()`'s `@phpstan-return` is a real
  PHPStan conditional return type this module's own doc comment already
  disclaims parsing - now actually skipped rather than parsed into a
  garbage type.
- **A method's own `@template` collapsed to `Mixed` on any non-generic
  class.** `substitute_class_templates` (class-level template
  substitution) always ran before `infer_call_return_type`'s separate
  per-call-argument binding for a method's *own* template - correct
  when both scopes exist, but its `bindings` map was always empty for a
  class with zero templates of its own, and `types::substitute_
  template`'s fallback for an unbound name is `Mixed`, wiping out the
  method's own template before the second, correct pass ever ran. Real
  shape: a plain PSR-14-style `EventDispatcher::dispatch()` with its
  own `@template T of object`. New `class_scope` restricts substitution
  to names actually declared by the owning class, leaving any other
  name untouched for the later pass.
- **`rewrap_with_accessory_markers` (array_map's "preserve the input's
  own list-ness" helper) only recognized an `Intersection`-shaped
  input**, silently dropping both markers when the input arrived as a
  `Union` from a control-flow merge (`$x = []; if (...) { foreach (...)
  { $x[] = ...; } }`) instead - the same shape `array_element_type`
  already handles, just missing here.
- **`array_push`/`array_unshift`/`array_shift`/`array_pop` fell into
  the generic by-ref widening**, which discards list-ness/non-emptiness
  down to a bare `array` - unlike the already-modeled `sort()` family,
  nothing recognized that these four preserve list-ness (the first two
  also gaining provable non-emptiness) when the argument was already
  list-like beforehand.
- **`range()`'s own real stub is a bare `@return array`** - no
  refinement at all, unlike real PHPStan's own dedicated
  `RangeFunctionReturnTypeExtension`. Added as a bespoke per-function
  extension (int and single-character-string bounds only, matching
  real corpus shapes) alongside `array_map`/`array_filter`/etc.
- **`ClassConst` never carried its own doc comment** - a `@var
  list<...>` directly above a `public const array X = [...]` (a real
  way to pin a const array's element type more precisely than either
  its native hint, not even kept past parsing, or the literal value's
  own inferred type) was silently dropped. Threaded through the same
  `take_doc_comment()` the member-parsing loop already captures for
  properties/methods.
- **An unassigned superglobal defaulted to `Type::Mixed`, not `array`**
  - real phpstorm-stubs' own bootstrap file declares `$_SESSION`/etc.
  with a literal `= []`. Because `widen_coalesce_result`'s "keep the
  left side's real array type" rule only recognized an already-known
  `Type::Array`, `$_SESSION ??= [];` collapsed to the literal `array{}`,
  flagging the very next `unset()` as targeting a key that provably
  doesn't exist.
- **`isset()`/`empty()`'s own direct argument went through the
  ordinary `check_expr`**, hitting the same `check_shape_key_access` a
  bare read would - even though probing a possibly-absent key is
  exactly what both constructs are for. Routed through the existing
  `check_expr_quiet_root` (already used for `??`'s own left side)
  instead.
- **A closure's own `use (&$x)` mutation never wrote back to the
  declaring scope.** Real shape: `$seen = []; $client = new
  MockHttpClient(function (...) use (&$seen) { $seen[] = [...]; });
  ...; $seen[0]` (a test-double-recording idiom) - `$seen`'s type at
  the read site stayed frozen at whatever it was *before* the closure
  literal. Every `by_ref` use now writes `union_types(pre-closure type,
  closure body's own final type)` back into the outer scope.
- **A method override's own `self`/`static` was never resolved before
  the Liskov parameter-narrowing check.** Real shape: a `final class
  ImageBackend implements ImageInterface`, whose interface names the
  concrete class directly (`compose(ImageBackend $overlay)`) and whose
  own override re-declares it as `self` - provably the same type, but
  `is_assignable`/`is_subtype` have no rule for a concrete `Object`
  satisfying a still-bare `SelfRef`. Both sides now go through the
  existing `resolve_pseudo` first.

Two real, evidenced gaps deliberately deferred rather than chased (1-2
diagnostics each, needing genuinely new machinery disproportionate to
the count): a `method_exists($x, 'foo')` truthy guard narrowing `$x` to
allow the guarded call (real PHPStan's own `HasMethod` accessory type,
no equivalent here); and a possibly-undefined variable correlated with
a *different* boolean flag variable set in the same branch (real
PHPStan's own conditional-expression-holder tracking - this project
only tracks a single variable's own certain-vs-maybe-defined state, not
cross-variable correlation).

**A seventeenth round** (97 -> 83) found four independent bugs, one of
which closed a Known-limitations item open since Phase 13:

- **Multi-variable compound-boolean narrowing** - real, common idiom
  (`CommentRepository::findAllWithConditions`/`findList`, 7 sites):
  `if ((! is_int($a) && ! is_string($a)) || (! is_int($b) && ! is_string
  ($b)) || ...) { continue; }`, narrowing several *different* variables
  through one combined condition. Turned out not to need the "sizable
  narrowing-engine feature" this had been tracked as needing: the
  existing `Or ... if !branch_true` arm already decomposes an
  arbitrarily-long `||` chain into independent per-leaf calls (De
  Morgan on the *outer* `||`) - each landing on exactly one
  `&&`-of-negated-predicates leaf, one variable at a time. The only
  missing piece was a new `and_chain_narrowing_target`, teaching `&&`'s
  own false branch to finish the same De Morgan step one level down
  (`!(!is_int($v) && !is_string($v))` is `is_int($v) || is_string($v)`)
  by reusing the existing `or_chain_narrowing_target` on each
  un-negated leaf directly.
- **`while (true)`/`for (;true;)` retry loops flagged as possibly
  falling off the function's own end.** `always_terminates` had no case
  for `While`/`For` at all - real shape: `NotificationController::
  findAvailableFeedId`/`AdvisorySessionLock::acquirePostgres`/
  `NotificationByMailService::findAvailableCheckKey` (find-a-free-key
  retry loops), `HttpClientService::guardedRequest` (a redirect-
  following retry loop). New `is_truthy_literal`/`stmt_contains_break`
  helpers, deliberately conservative about `break` (any found anywhere
  in the body, even one targeting a different nested loop/switch,
  counts as "might exit on its own" - only ever misses a
  simplification, never wrongly concludes termination).
- **A class const's own concatenated value fell to `Mixed`.** Real
  shape: `Core\AppInfo::URL = 'https://' . self::DOMAIN;` -
  `infer_const_type` has no class context to resolve what `self::
  DOMAIN` actually is, but real PHP string concatenation always
  produces a `string` regardless of either operand's own type - sound
  information available without resolving anything.
- **`$x ?? null` collapsed a `Mixed` `$x` down to provably-`null`.**
  `widen_coalesce_result` trusted the literal `Type::Null` r_ty as-is,
  but `$x ?? null` is a real no-op (if `$x` is null the result is null
  either way, otherwise it's exactly `$x`) - its own type must be `$x`'s
  own unnarrowed type. Real shape: `BatchManagerGlobalRequest::
  fromArrays`'s `$post['selection'] ?? null` on an unshaped `array`
  (hence `Mixed`), silently collapsed to `null` well before the very
  next `is_array(...)` check on it ever ran. Three existing unit tests
  turned out to have unknowingly built their own "narrows nothing"
  setup on top of this exact bug (an unshaped-array-index coalesce
  happened to produce a `Null` this checker's own return-type check
  could catch, purely as a side effect of the bug) - rewritten to use a
  directly nullable parameter instead, testing the intended narrowing
  behavior on its own terms.

Also found, named, and deliberately deferred to a future round rather
than chased now: a second, distinct correlated-narrowing gap (9
diagnostics across 5 files) - see **Known limitations** above for the
two shapes found (a boolean flag assigned exactly a condition
expression, and two variables always assigned together in the same
branch) and why neither is a quick fix on top of this project's own
`Locals` design.

**An eighteenth round** (83 -> 81) found two more bespoke per-function
return-type extensions missing the same "real stub gives no list-ness"
shape several earlier rounds already fixed for other functions:

- **`preg_split()`'s own real stub is a bare `@return string[]|false`**
  - no list-ness at all, unlike real PHPStan's own dedicated
  `PregSplitDynamicReturnTypeExtension`: splitting a string always
  produces sequential integer keys starting at 0, for any pattern/
  subject, literal or not. Added matching real PHPStan's own fallback
  (non-literal-argument) case exactly: `list<string>|false`, refined
  to `list<non-empty-string>|false` when `PREG_SPLIT_NO_EMPTY` is one
  of the flags.
- **`array_column()`'s own existing special case only recognized a
  *shaped* row type**, falling through to the generic (unmarked) path
  for the far more common real shape: an unshaped row (`array<string,
  mixed>`, a raw DB-fetch row with no further refinement). Extended to
  also read an unshaped row's own uniform element type directly
  (mirrors real PHPStan's own `ArrayColumnHelper::getReturnValueType`)
  - even a bare `Mixed` element type is real, useful information once
  the existing `Array(Mixed)` leniency lets it satisfy a more specific
  declared element type.

Also investigated and confirmed genuinely out of reach without
embedding a PHP interpreter or a SQL/DQL parser - see **Known
limitations** above for both (real `strtotime()` accuracy needing
PHPStan to literally execute PHP's own `strtotime()` at analysis time;
a DQL `SELECT`'s own column list determining a query result's real
shape).

**A nineteenth round** (81 -> 79) found two more bespoke per-function
return-type extensions, plus one real attempted fix reverted after a
corpus re-check showed it unsound - see **Known limitations** above
for the reverted anonymous-class-aliasing attempt and why it doesn't
work:

- **`version_compare()`'s 2-argument form always returns `int`, never
  `bool`.** The real stub declares `int|bool` unconditionally (both
  PHP-version-gated overloads agree), but real PHPStan's own
  `VersionCompareFunctionDynamicReturnTypeExtension` narrows based
  purely on whether the 3rd `$operator` argument is syntactically
  *present* at all, regardless of its own value. Real shape
  (`CalendarBase::buildNextPrev`'s own `static fn (string $a, string
  $b): int => version_compare($a, $b)`).
- **`array_search()` on a *list*-typed haystack can only ever find an
  `int` key.** The real stub declares `string|int|false`
  unconditionally - this project's own `Type::Array` has no key-type
  parameter at all (a known, sizable open gap), so it can't generally
  narrow the key-type half the way real PHPStan's own dedicated
  extension (delegating to the haystack type's own `searchArray()`)
  does, but a list's own keys are always sequential integers by
  definition, a real, narrower special case already trackable without
  any key-type machinery. Real shape (`pageAssetsTestIndexOf`).

## Phase 20 - Performance & robustness - DONE

Started from a real incident: Phase 11's lexer work shipped an O(n²)
blowup that hung for minutes on a real corpus file, caught only by this
project's own real-corpus verification step (no dedicated performance
tooling existed yet). Profiled a real `check-project` run
(`perf record --call-graph dwarf`) to prioritize:

1. **A real stack-overflow crash on deeply-nested expressions.** The
   parser's `MAX_EXPR_DEPTH=1000` guard was undermined by worker threads
   spawning with Rust's default ~2MiB stack - confirmed a hard SIGABRT
   crash at only ~200-300 levels of nesting. Fixed with an explicit 8MiB
   worker stack.
2. **`Locals`'s cloning cost** (~28% of allocator-churn time, cloned at
   ~22 call sites per branch). Swapped to `imbl::GenericHashMap`
   (persistent, structurally-shared, O(1) clone). A real methodology
   lesson: system-wide `perf` sampling first reported this as
   essentially unchanged (~32% -> ~31.5%) - too coarse to see the real
   effect. A new in-process instrumentation feature, `hotpath` (item 6
   below), measured the true, internally-consistent win: `check_stmt`
   -13.9%, `check_function` -10.1%, `check_program` -8.3%, `check_expr`
   -7.5%, total runtime -5.3%.
3. **`FxHashMap` for `Registry`/`Project`'s other hot, build-once
   HashMaps.**
4. **An automated regression-guard benchmark** (`benches/
   vendored_corpus.rs`, criterion, against this project's own in-repo
   vendored corpus so it works in CI without an external Piwigo copy).
5. **Pathological-input regression tests** codifying both findings above.
6. **`hotpath`, an opt-in in-process instrumentation profiler** (zero-
   cost when disabled) - the tool that caught item 2's real effect after
   system-wide `perf` sampling missed it.

## Phases 21-35 - Piwigo false-positive hunts

Fifteen rounds of the same repeatable method, refined as it went: run
`check-project` against a real, PHPStan-clean Piwigo corpus; bucket the
output by normalized message shape (strip argument indices, variable
names, string literals, class-name prefixes - see the categorization
snippet below); root-cause each cluster by reading the *actual*
triggering source (Piwigo's own code, this project's own checker, and
the real vendored `phpstorm-stubs`/`phpstan-src`) rather than guessing
from message text; fix; re-verify with a location-based diff (a changed
line landing at an already-diagnosed location is a safe reshape, a
genuinely new location gets individually traced to ground truth before
being accepted). Phase 22 switched from the live, file-count-drifting
`~/piwigo17-rewrite` to a frozen local copy with a proper scoped config,
the methodology used ever since:

```bash
cd /home/torres/piwigo17-frozen
/path/to/phpstan-rs check-project --config phpstan-rs.toml 2>&1 \
  | grep -oP '(?<=: ).*$' \
  | sed -E "s/\\\$[A-Za-z_][A-Za-z0-9_]*/\$VAR/g; s/[0-9]+/N/g; s/'[^']*'/'STR'/g" \
  | sort | uniq -c | sort -rn | head -50
```

Diagnostic count per round (scoped corpus from Phase 22 on): 43,800
(Phase 21 start, unscoped) -> 32,596 -> **2,312** (Phase 22, first scoped
count) -> 1,280 -> 1,021 -> 792 -> 732 -> 706 -> 580 -> 532 -> 533 (a
disclosed, accepted tiny uptick - see Phase 29) -> 512 -> 508 -> 350 ->
295 -> 270 -> **249**.

**Phase 21 (unscoped corpus).** Pest's `it`/`test`/`describe`/hook
closures binding `$this` at runtime (read directly from the real
vendored `pestphp/pest-plugin-phpstan` extension's resolution algorithm,
1498 -> 13); `assert($x instanceof T)` narrowing; a generator function/
closure never confused for returning null; `phpstan/phpstan`'s own
PHAR-bundled classes resolved by chaining `vendor/phpstan-src` into the
stub pipeline; a conditionally-declared class stub (`if
(!class_exists())`) no longer silently overwriting a real unconditional
class of the same name.

**Phase 22 (2,312 -> 1,280).** Pest's `beforeEach`-assigned dynamic
`$this` properties (571 -> 0); array append/dynamic-key writes now widen
a tracked shape instead of leaving it stale, including recursing through
nested writes; a by-ref argument on an *already-defined* variable now
widens too, not just an undefined one; `?? []` no longer discards a
wider left-side array type; anonymous-class method signatures now
resolve names/pseudo-types correctly instead of falling back to an
unresolved branch; a right-hand `&&`/`||` operand's new variable
definitions are no longer silently dropped. The array-write fix's own
verification surfaced a deeper gap - `narrowing_key` only supported
`Variable`/`PropertyAccess`, never a literal-key array index - fixed by
extending it everywhere, which is what let this phase close out the
"property-chain narrowing" limitation tracked since Phase 13 (extended
to array-index chains too).

**Phase 23 (1,280 -> 1,021).** `isset($x)` never narrowed away `Null`
(the single most common PHP nullability idiom, 1,073 occurrences,
previously a complete no-op); an assignment nested inside a narrowing
predicate's own argument (`is_string($result = expr)`) now narrows;
`str_replace`/`preg_replace`-family return types now depend on
`$subject`'s own type; a ternary's two branches now flatten through the
shared `union_types` helper instead of building an unflattened nested
`Union` narrowing couldn't see into; `exit`/`die` now infer `Type::Never`
so a guard clause built on them is recognized as terminating; a negated
literal class-constant value (`const OFF = -1;`) no longer widens to
`Mixed`; a stale legacy `@return resource|...` doc comment no longer
overrides an already-modernized native hint (with a careful exception
for `mixed`/`object` placeholder hints, where the doc genuinely is the
only source of truth) - this last one also corrected a Phase 22 "not
pursued" note that had misdiagnosed the symptom as an unknown narrowing
bug. **Not pursued**: `#[LanguageLevelTypeAware]` attribute parsing
(2,033 occurrences across 84 stub files) - real, non-trivial new parsing
machinery, not a small addition alongside seven otherwise surgical
fixes; flagged as the likely highest-value remaining investigation.

**Phase 24 (1,021 -> 792) - `#[LanguageLevelTypeAware]`.** Closed the
deferral above: this attribute says "this function/param's type differs
by PHP version" (`substr()`'s real stub has *no* native return hint at
all, so a stale pre-8.0 doc comment was the only source used). Shipped
both halves - return-type gating (splices a fresh `@return` line into
the preceding doc comment, extracting the highest-versioned type,
consistent with this project's existing `#[PhpStormStubsElementAvailable]`
precedent) and parameter-type gating (injects synthetic native type-hint
tokens before the parameter, since a parameter has no guaranteed
docblock tag to overwrite the way a return does) - accounted for roughly
a quarter of everything left at the time.

**Phase 25 (792 -> 732) - class-declaration merging.** Piwigo ships a
project-local `Imagick.stub.php` overriding one stale method (real
PHPStan's own documented `stubFiles` merge mechanism) - this project
treated two unconditional declarations of the same class as plain
last-write-wins, so the one-method override silently discarded the
entire real class (60 diagnostics, ~7.6% of everything left).
`ClassInfo::merge` now unions members across unconditional declarations
(second wins on conflict, `extends`/`implements`/generics only ever
filled in). Given this is a *global* registration-semantics change,
verification swept every file in both vendored stub corpora, not just
the usual before/after diff.

**Phase 26 (732 -> 706) - narrowing through wrapper expressions.**
`narrowing_key` never unwrapped a parenthesized `Grouping` node, so the
overwhelmingly common `while (($file = readdir($dir)) !== false)` shape
(parens are *required* here by operator precedence, not optional style)
never narrowed. Also: `apply_narrowing` had no `Cast` arm at all, so an
explicit `if (! (bool) $fp)` truthiness guard didn't narrow either - a
`(bool)` cast doesn't change which values are truthy, so it now
delegates straight through.

**Phase 27 (706 -> 580) - PHPUnit assertion narrowing.** A systemic idiom
(1,200+ combined uses across the test suite) of
`assertIsString`/`assertNotNull`/`assertInstanceOf`/etc. as a narrowing
proof - if execution continues, the assertion held. New
`apply_phpunit_assertion_narrowing`, matched purely by method name (both
`self::`/`$this->` forms), the largest single-round drop since Phase 24.

**Phase 28 (580 -> 532) - `is_resource`/`is_scalar` narrowing.**
`is_resource` was simply absent from the narrowing-predicate table
despite being the idiomatic guard for `proc_open()`/`fsockopen()`'s real
`resource|false` returns; `is_scalar` added too (exactly
`int|float|string|bool`, unlike `is_numeric`, deliberately still
unnarrowed since this project has no `numeric-string` representation).

**Phase 29 (532 -> 533, a disclosed uptick) - a `break`'s own state
reaching past its loop.** A real control-flow-modeling gap: nothing
captured a `break`-reached branch's state for a loop's own post-loop
state, so a variable only ever assigned on the path that breaks stayed
typed as its pre-loop value forever (masking every real type of the
value it could actually hold). New `loop_break_states` stack, with a
`Break` statement pushing the current scope onto the innermost (or `break
N`'s) open frame, merged back after the loop. A real design mistake was
found and reverted mid-fix: also giving `Foreach` a `While`/`For`-style
"may run zero times" merge with its pre-loop state broke a real,
common idiom (a loop mutating the very array it iterates via its own
loop variable). Correctly modeling `break` surfaced 6 further, previously-
masked diagnostics - all traced to the already-known `Type::Array`-has-
no-key-type gap, a legitimate disclosed limitation, not a new defect.

**Phase 30 (533 -> 512) - `Closure(...)`/`callable(...)` shape syntax.**
A PHPDoc callable-shape type (`@param Closure(self):T $func`, real and
documented, e.g. Doctrine DBAL's own `transactional()`) had no parser
case at all, falling through to the generic class-name branch and
producing a garbled, namespace-mangled "class name" as the diagnostic's
own expected-type text. Now degrades to plain `Type::Callable`, matching
this project's existing bare-`callable` leniency.

**Phase 31 (512 -> 508) - `array_keys()`'s real key type.**
`array_keys()`'s stub declares a whole-array either/or (`int[]|string[]`)
rather than a precise per-key result. Scoped fix: when the argument is a
known `ArrayShape`, compute the real per-key union directly (PHP casts a
canonical non-negative-integer string key to int at build time) instead
of always defaulting to the stub's own wider union. Deliberately scoped
no further - most of this bucket's remaining diagnostics pass a variable
with no shape information at all, the general `Type::Array`-has-no-key-
type gap this fix doesn't (and can't, without that gap closing first)
address.

**Phase 32 (508 -> 350) - a full-corpus bucket sweep.** Twelve
independent bugs in one round instead of chasing a single bucket: (A)
duplicate stub parameters from phpstorm-stubs' own version-era
convention weren't deduplicated, shifting later argument positions; (B)
a `LanguageLevelTypeAware` parameter override lost to a competing legacy
`@param` tag; (I) the same docblock-vs-native reconciliation generalized
past `resource` specifically, then *corrected* after a regression:
`is_subtype`'s `Object` comparison is name-equality only, so a doc type
naming a real subclass of a more generic native hint was wrongly
discarded - scoped back to the one provable case; (C) two unconditional
function declarations of the same name overwrote instead of merging
(the function-side sibling of Phase 25's class-merge fix); (D)
`microtime(true)`'s bool-literal-dependent return type; (E) three more
functions added to the subject-dependent-return-type family; (F)
`narrowing_key` now unwraps `?? null` (a literal fallback specifically -
sound; a non-null fallback isn't); (G) `is_numeric()` narrowing, as its
own dedicated arm with asymmetric per-branch exclusion sets (a shared-
target implementation would have introduced a new false positive); (H)
a docblock union containing `mixed` now collapses to plain `mixed`;
(J) `json_encode(..., JSON_THROW_ON_ERROR)` drops `false` from the
return, including through an arbitrary-depth flag chain; (K) a bare
`$cond || neverReturningCall();` guard now narrows `$cond`; (L) exact
type equality was the wrong tool for union-member exclusion (a
docblock-refined `array<string>` survived an `is_array()` exclusion
untouched) - switched to `is_subtype`-based membership, which also fixed
a previously-silent bug in Phase 28's own `is_scalar` false branch, and
a matching blind spot in `union_types`'s own construction-time
absorption. Left four new Known Limitations rows, all closed the very
next phase.

**Phase 33 (350 -> 295) - closing Phase 32's own disclosed rows.**
`instanceof`'s false branch now drops the checked class from a `Union`
(reusing the existing `narrow_type_excluding`, since the false branch
only needs to *drop* a proven-impossible member, not represent "not
this class" on its own); `in_array($needle, $literalArray, true)` and
`array_key_exists($literalKey, $arr)` are now recognized narrowing
predicates, mirroring real PHPStan's own extensions for both; a real
variable-subscript narrowing key (`is_string($this->mirror[$key])`)
falls back to a `$`-prefixed marker - and, checking whether *any*
invalidation-on-reassignment mechanism existed before trusting this new
key format, found there wasn't one for *any* composite key at all (a
stale `"foo->bar"` narrowed value survived a plain `$foo = ...`
reassignment forever) - fixed with a shared `purge_stale_narrowing_for`,
wired into all seven real call sites that can give a variable a new
value; the disclosed `int<min, max>` gap turned out to be misdiagnosed -
the real bug was `array{...}`'s own unkeyed/positional tuple fields
(`array{int, string}`) failing to parse at all, aborting the whole
shape. A correction found by the mandatory full-corpus re-check:
the new positional-field detection first rejected a real, pre-existing,
hyphenated bare key (`plural-forms`, a gettext convention) - fixed
transparently, with its own regression test.

**Phase 34 (295 -> 270) - a fourth full-corpus sweep.** `curl_getinfo($ch,
CONSTANT)` given a real dynamic return type (a curated ~35-entry subset
of real PHPStan's own extension table, scoped to the 2-arg literal-
constant form); `||`'s true branch can now narrow when every leaf is a
same-key predicate check on the identical expression (fixes `is_int($y)
|| is_string($y) ? $y : 0`); a backed enum's `->value` now types as the
union of its own case literals instead of widening to the generic
backing type; `stdClass` is exempt from "access to undefined property"
(a real, registered, zero-property class - real PHP treats it as the
canonical arbitrary-property bag) - scoped to property reads only, an
undefined *method* call on `stdClass` is still a real fatal error and
stays flagged. Deliberately deferred: `class-string<T>` template
binding and inline `@var` annotations - both needed genuine new type-
system/parser machinery, not a narrow fix; closed properly the very next
phase.

**Phase 35 (270 -> 249) - closing Phase 34's own deferred items,
properly.**
- **`class-string<T>` now participates in `@template` binding.** A new
  `Type::ClassString(Box<Type>)` survives from parsing through
  `Foo::class`/`get_class($obj)` inference, template binding/
  substitution, and every recursive `Type`-tree walker, with
  `is_subtype`/`is_assignable` kept freely interchangeable with plain
  `string` in both directions for backward compatibility (`is_assignable`
  additionally getting its own hierarchy-aware override, the same way it
  already does for plain `Object`-vs-`Object`). Fixed the targeted
  `*TestRepoWithEm`/`*TestConnAndRepo` family (6 diagnostics) - and, it
  turned out, far more: every real `TypedRepository::narrow(...)`-shaped
  repository construction in the corpus had been `Mixed` end to end (so
  every call through it was entirely unchecked), and PHPUnit's own
  `self::createStub(Foo::class)` (a `@template T`/`class-string<T>`
  method in its own stubs) now resolves to `Foo&Stub` instead of `Mixed`.
  One new, isolated false positive surfaced as a direct result of a
  repository finally being checked at all - traced to the already-
  tracked `Type::Array`-has-no-key-type gap (Phase 15), not a new
  regression.
- **Inline `@var` annotations are now recognized on any statement**, not
  just an actual property/param/return declaration. `Stmt` carries its
  own leading doc comment now; `check_stmt` applies a recognized
  assertion before dispatch (so later reads in the same block see it)
  and, only when the statement itself is a plain assignment to the same
  variable, again after dispatch - found via adversarial validation of
  the design *before* writing any code that `bind_assignment_target`
  would otherwise silently clobber the pre-applied assertion in the same
  call. A tag directly on a `foreach` header was deliberately scoped out
  after tracing that the loop body is checked *inside* the same match arm
  that binds the loop variable - an after-dispatch-only reapplication
  would produce a half-working result (correct after the loop, wrong
  inside it), worse than not supporting the shape at all (see Known
  Limitations above). Fixed the evidenced `StructuralTest`/`tools/p58/
  trace.php` family, plus several unrelated real `@var` comments already
  written elsewhere in the corpus that this checker had simply never
  read before.

---

# Verification approach (used for every phase above)

- Unit tests for each new rule/type-system feature.
- Re-run against real corpora after every phase: Piwigo's `src/`, its
  `vendor/`, php-src's own test corpus, phpstorm-stubs' real stub files,
  and (from Phase 22 on) a frozen local Piwigo copy with a
  `phpstan-rs.toml`-scoped config for stable before/after diffing.
- For new diagnostic rules: spot-check a sample of newly-surfaced
  diagnostics by hand to confirm they're genuine findings, not false
  positives.
- Location-based classification for every phase's corpus diff (Phase 25
  on): a changed line landing at an already-diagnosed location (a
  reworded/reordered message) is a safe reshape; every genuinely new
  location is traced individually to ground truth before being accepted
  as expected, not just assumed safe from the raw count alone.
- **Test-coverage measurement (`cargo llvm-cov`)**, added while auditing
  Phase 6/9 - manual review kept finding real bugs the test suite should
  have caught but didn't exercise. Close a file's gaps by writing a test
  for what coverage flags, only after first manually verifying
  correctness.
- **A fresh clone of `phpstan/phpstan-src`**, used to ground every
  numeric/behavioral claim about real PHPStan in Phases 8.5 and 12-18 -
  re-verify against a fresh clone before trusting any of those numbers
  after a long gap, since real PHPStan keeps evolving.

Full detail, adversarial fact-checking history, and a dependency
shortlist for Phases 8-10 live in the planning doc this roadmap was
originally distilled from (kept outside this repo, in the assistant
session that built them).
