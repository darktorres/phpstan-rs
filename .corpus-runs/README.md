# Piwigo corpus-run snapshots

Gitignored (large, environment-specific, not source) - lives here rather
than a session scratchpad so it survives across sessions and never needs
rebuilding an old commit just to re-diff against it.

## Layout

    .corpus-runs/scoped/<commit-hash>.txt    - phpstan-rs.toml-scoped run
    .corpus-runs/unscoped/<commit-hash>.txt  - --no-composer unscoped run

`<commit-hash>` is the short hash of the phpstan-rs commit the binary was
built from when the run was taken (`git rev-parse --short HEAD`). Piwigo's
own checkout (`/home/torres/piwigo17-frozen`) is treated as fixed/frozen,
so it isn't part of the key.

## Producing a run

    cargo build --release --bin phpstan-rs
    HASH=$(git rev-parse --short HEAD)
    cd /home/torres/piwigo17-frozen
    /home/torres/projects/phpstan-rs/target/release/phpstan-rs check-project \
        --config phpstan-rs.toml 2>&1 | LC_ALL=C sort \
        > /home/torres/projects/phpstan-rs/.corpus-runs/scoped/$HASH.txt
    /home/torres/projects/phpstan-rs/target/release/phpstan-rs check-project \
        --no-composer . 2>&1 | LC_ALL=C sort \
        > /home/torres/projects/phpstan-rs/.corpus-runs/unscoped/$HASH.txt

Only save a run for a real commit (working-tree/uncommitted state has no
stable key to file it under - re-run once committed instead).

## Diffing

Before rebuilding an old commit to get a comparison snapshot, check here
first - `ls .corpus-runs/scoped/` - the commit you want may already have
one. Only fall back to a worktree rebuild when it's genuinely missing.

    diff .corpus-runs/scoped/<old-hash>.txt .corpus-runs/scoped/<new-hash>.txt

## Known snapshots (as of 2026-09-11)

- `264d910` - last snapshot before the 0247b04/d1d1d61 phase-design batch
  (355 scoped diagnostics) - no unscoped run exists for this one.
- `d1d1d61` - post-batch, **before** the use_map/throws regressions were
  fixed (5012 scoped diagnostics) - kept deliberately as the historical
  record of that regression, not a state to build against.
- `c522416` - post-fix (359 scoped / 40503 unscoped).
- `1a3f3e4` - master2 branch tip before its rebase onto master (358
  scoped, 0 skipped) - the bucketed-findings baseline the 3 false-
  positive fixes below were diffed against; produced/stored in the
  `phpstan-rs-master2` worktree, not the canonical one.
- `b33d307` - master2 rebased onto master, **before** the 3 false-
  positive fixes (353 scoped) - the clean "pre-fix" comparison point
  isolating just those 3 commits' effect from the unrelated commits the
  rebase itself pulled in; produced in a throwaway worktree at this
  commit, stored under `phpstan-rs-master2`.
- `299e5c5` - post-fix (341 scoped). Removed exactly the 12 confirmed
  false positives (2 `staticMethod.resultUnused` on a bare
  `parent::__construct()`, 8 `method.unused`/`property.onlyWritten`/
  `classConstant.unused` from private-member usage inside a closure/
  arrow-fn, 2 more of the same from usage through a non-`$this`
  same-class-instance local variable) and nothing else in the diff
  moved - produced/stored in the `phpstan-rs-master2` worktree.
- `452eb2b` - post Phase-1 of the corpus-findings-triage plan (339
  scoped). Removed exactly 2 `argument.type` findings
  (`PaginationService::createNavigationBar`'s own bare `min()`,
  `HistoryService::logVisit`'s own `array_find()`) and nothing else
  moved, despite `min`/`max`/`array_find`-family/non-variable-`++`/`--`
  each touching 20-36 more call sites in `src/` alone - produced/stored
  in the `phpstan-rs-master2` worktree.
- `069cff9` - post `#[ArrayShape(...)]` attribute support (348 scoped).
  Removed `ImageDerivativeControllerTest.php`'s own `idcPixel()`/
  `imagecolorsforindex()` `return.type` finding; correctly *added* one
  new, genuine `nullCoalesce.offset` finding at `ImageBackend::
  getGraphicsLibrary` (`gd_info()['GD Version'] ?? null` -
  `gd_info()`'s own `#[ArrayShape]` declares that key unconditionally
  present, matching what real PHPStan would also now report with the
  same precision) - produced/stored in the `phpstan-rs-master2`
  worktree.
- `e9bde62` - post the CategoryService `mixed`-leak fix (three layers:
  empty-shape dynamic-key reads, `check_expr`'s own `Index`-read arm
  unified with `index_result_type`, `advance_incdec_type`'s own `Union`
  recursion) (360 scoped) - current baseline for future diffs. Net +12
  from `069cff9`, all individually investigated (see ROADMAP.md's
  "Known but not yet scheduled"/"Permanent design boundaries" - a
  by-reference-`foreach`-mutation gap and the cross-variable-
  correlation boundary, both newly cross-referenced from this run) -
  1 genuine new true positive
  (`UserService::searchCaseUsername`), 1 confirmed false positive from
  that separate by-ref-foreach gap, 10 sound-but-imprecise cross-
  variable-correlation findings. Produced/stored in the
  `phpstan-rs-master2` worktree.
- `dbfc5f0` - post the PHPDoc colon-in-description parsing fix
  (`1471d7a`) and the `throws.unusedType` union-membership/`Type::Void`
  sentinel fix (`dbfc5f0` itself) (348 scoped). Net -3 from `e9bde62`:
  2 `missingType.iterableValue` findings on
  `NotificationByMailSubController::doTimeoutTreatment` (the colon fix
  let its own `@param array<string, mixed> $post: ...`-style docblock
  tags finally register), 1 genuine `throws.unusedType` false positive
  on `ConfigLoader::validateRequired` (the union-membership fix). No
  separate snapshot exists for `1471d7a` alone - produced/stored in the
  `phpstan-rs-master2` worktree.
- `ce5b411` - post the `LoopZeroIterations` fix (`while`/`for`/`foreach`
  all now merge their exit state with the pre-loop entry, matching real
  PHPStan's own `ForeachHandler::processStmt` "maybe zero iterations"
  merge - confirmed directly against `vendor/phpstan-src`) plus the
  `write_index_into` `Type::Null` auto-vivification arm it exposed a
  need for (353 scoped). Net +5 from `dbfc5f0`: the target
  `UploadService::addUploadedFile` `isset.variable` false positive is
  gone; a `CalendarMonthly::buildMonthCalendar` regression from an
  earlier iteration of this same fix (2 `shapeAccess.notFound` false
  positives) was found via this corpus diff and fixed before landing
  (the `write_index_into` arm above); the remaining 8 new/changed
  `argument.type`/`return.type` findings (`RatingPageRenderer`,
  `CategoryService` x2, `CatPermPageRenderer`, `DerivativeImage`,
  `UserService` x2, plus reordered union members on several more) are
  genuine, sound consequences of the same fix surfacing pre-existing
  `mixed`-leak imprecisions in Piwigo's own code, individually traced
  rather than assumed. Produced/stored in the `phpstan-rs-master2`
  worktree.
- `d9b36bf` - post the `isset()`/`empty()`-as-a-value fix (both now
  infer `bool` when read as a plain value, not just narrowed as a
  condition - `call_fallback_return_type` had no arm for either) (352
  scoped). Net -1 from `ce5b411`: removed
  `SiteUpdateSubController::handle`'s own `sync_meta`/`meta_all`/
  `meta_empty_overrides` `mixed`-leak (all 3 fields collapsed into one
  `argument.type` diagnostic). Produced/stored in the
  `phpstan-rs-master2` worktree.
- `0df31ed` - post the "provably non-empty foreach subject" fix
  (`check_foreach` now passes `LoopZeroIterations::Impossible` instead
  of `Possible` when `Self::is_known_non_empty_array` proves the
  subject can't be empty - matching real PHPStan's own
  `isIterableAtLeastOnce()->yes()` case) (352 scoped, same count -
  three diagnostics lost their spurious `array{}` union member without
  the underlying finding disappearing entirely). Fixed
  `UserSortField::parseOrderClause`'s own `foreach (explode(',',
  $order) as ...)` cleanly (the `array{}` member is fully gone);
  `BatchManagerGlobalPageRenderer.php:195/443` and
  `NotificationByMailSender.php:707` still show `array{}` (their own
  subjects aren't provably non-empty). Produced/stored in the
  `phpstan-rs-master2` worktree.
- `2c09d79` - post the `array_fill()` list/non-empty accessory fix
  (matches real PHPStan's own `ArrayFillFunctionReturnTypeExtension` -
  `list`-marked when the start index is provably `0`, `non-empty`-marked
  when the count is provably `>= 1`) (350 scoped). Net -2 from `0df31ed`:
  both `BatchManagerGlobalPageRenderer::render` findings (`.php:195` and
  `.php:443`) are now fully resolved - `array_fill(0, $n, 0)`'s own
  bare `array<0>` becomes `list<0>`, satisfying the `list<int>`-typed
  sinks directly. Produced/stored in the `phpstan-rs-master2` worktree.
- `ee71e82` - post the `count($x) > 0`-narrows-a-`Union` fix
  (`upgrade_array_to_non_empty` gained a `Type::Union` arm, collapsing a
  genuinely-empty `ArrayShape` member to `Never` and filtering it out -
  it previously had no arm at all for a `Union`, silently leaving an
  empty-shape alternative unstripped) (350 scoped, same count - three
  diagnostics lost their spurious `array{}` union member without the
  underlying finding disappearing, each for its own separate, unrelated
  reason). `CategoryService.php:1586`, `SiteUpdateSubController.php:505`
  and `:717` all lost the `array{}` alternative. Produced/stored in the
  `phpstan-rs-master2` worktree.
- `3b8ca74` - post a 3-way parallel corpus-triage round (4 agents, each
  working an isolated bucket of the remaining `argument.type`/
  `return.type` findings in its own worktree, merged back cleanly with
  no manual conflict resolution needed): (343 scoped). Net -7 full
  removals plus 3 more diagnostics narrowed to a cleaner "given" type
  without disappearing (their own root cause is separate/unrelated -
  `ImageRepository.php:3573`, `ImageStdParamsTest.php:197`,
  `UploadServiceTest.php:1359`). 11 real phpstan-rs bugs fixed across
  the round: `in_array_target_type` bailing on a bare-`[]` haystack
  element; `CompoundAssign`'s rebind step only handling a bare
  `Variable` left side (`??=`/`+=` etc. on a property/index target
  never updated the tracked type); `ReflectionClass::newInstanceArgs()`
  returning nullable native `?object` instead of `T` (no phpstorm-stubs
  tag, unlike its siblings); `array_intersect`/`array_diff` family not
  handling a single spread argument (`array_merge` already did);
  `Enum::cases()` returning a bare `array` instead of `list<static>`;
  `array_key_exists()` narrowing being string-literal-key-only (now
  also accepts an int-literal key); `assertIsResource()` narrowing
  never actually stripping `false` (an assignability leniency bug); no
  `str_repeat()` handling at all (now constant-folds small literals,
  marks non-falsy when provable). The rest of the round's findings were
  genuine (untyped sources, PHP's own numeric-string-key semantics, a
  Symfony interface even real PHPStan can't narrow further) or newly-
  documented instances of existing limitations (cross-variable-
  correlation, dynamic-key narrowing) plus 3 new ones (dynamic-key
  writes dropping list markers even when provably in-bounds; compound
  `||`/`&&` condition-alias decomposition; `array_splice()`'s untracked
  by-ref mutation) - current baseline for future diffs. Produced/stored
  in the `phpstan-rs-master2` worktree.
- `967a13c` - PHASE_BACKLOG.md/TOP100_RESEARCH_NOTES.md doc-only commits
  (top-100 Packagist `phpstan-extension` survey), zero `src/` change -
  scoped 342/unscoped 40459, byte-identical diagnostic counts to
  whatever the last real `src/`-changing commit before it was (only
  line-ordering differed post-sort, a known non-determinism in
  identical-file:line-diagnostic relative ordering, not a real change).
- `4ee7617` - fixed two real bugs in `staticMethod.resultUnused`/
  `method.resultUnused`, found by diffing this exact scoped run against
  a real `vendor/bin/phpstan analyse` run on the same Piwigo checkout
  (real PHPStan: zero relevant findings). (1) `is_transitively_pure` had
  no exclusion for `void`-returning declarations - real PHPStan's own
  `hasSideEffects()` unconditionally treats any non-constructor `void`
  method/function as side-effecting regardless of body, confirmed
  against the real installed `phpstan/phpstan` 2.2.8 phar directly, not
  just the vendored reference source. (2) `PurityDependency::Method`/
  `Function` lowercased the class/function name before storing it, but
  `Registry::classes`/`functions` are keyed by whatever case the
  declaration actually used - silently broke the transitive-purity
  fixed point for any multi-hop dependency chain through an uppercase-
  containing class name (nearly all of them). Scoped 342 -> 80 (exactly
  the 262 false positives removed, nothing else moved); unscoped's only
  two changed identifiers across 40k+ diagnostics: `method.resultUnused`
  231 -> 91, `staticMethod.resultUnused` 319 -> 6.
- `eeee436` - fixed two more real bugs, same corpus-diff methodology.
  (1) `union_types` never dropped a redundant, genuinely-*empty*
  `ArrayShape` (`array{}`) once a general `Array(key, val)` member was
  already present in the same union (`array{}` is a subtype of *any*
  `Array(key, val)`, vacuously) - real PHPStan's own `dumpType()`
  confirms its post-loop type for a dynamic-key-built accumulator array
  is the plain, unqualified general map, no `array{}` alternative
  surviving at all; `index_result_type`'s own separately-correct empty-
  shape-plus-dynamic-key `Null` rule was turning the leftover `array{}`
  branch into a bogus `|null`. Fixed in `union_types` itself, not
  `converge_loop_body`'s own merge algorithm (materially safer -
  doesn't touch the symmetric `LoopZeroIterations::Possible` merge or
  its `subject_exempt` carve-out at all). (2) `ReflectionFunctionAbstract
  ::getParameters()`/`Exception`+`Error::getTrace()`/`debug_backtrace()`
  all return a bare `array` in phpstorm-stubs with no refining PHPDoc,
  while real PHPStan hardcodes a precise `list<T>` for each via its own
  dynamic-return-type extensions - confirmed via `dumpType()` and
  patched in `php_stubs.rs`. Scoped 80 -> 64 (16 false positives
  removed cleanly); 4 new findings surfaced are independently confirmed
  pre-existing, already-documented gaps (by-ref foreach array-mutation
  tracking; an untriaged `int|string` key-imprecision cluster) reached
  for the first time once an unrelated masking effect was removed, not
  regressions - each isolated and verified individually before
  concluding that. Unscoped 40006 -> 40017, entirely inside third-party
  vendor code this project's own real `phpstan.neon` doesn't even scan
  (Symfony YAML, PHP_CodeSniffer via symplify) - not chased down, no
  real-PHPStan ground truth exists for that scope anyway - current
  baseline for future diffs.
- `d437438` - fixed `BinOp::Add`'s array-union arm, which used to
  collapse straight to `Array(unknown_array_key(), Mixed)`
  unconditionally regardless of both operands' own key/value types.
  Traced directly to `$merged = $filters + $functions;` on two real
  `array<string, callable>`-typed values (Latte's own `Engine::
  getFilters()`/`getFunctions()`), making every `foreach ($merged as
  $name => ...)`'s `$name` infer as `int|string` and cascading into real
  `strtolower($name)`/`strtoupper($name)`/`renderMethod($name)` false
  positives in `tools/phpstan/Latte/ShimClassGenerator.php`; the same
  cause separately cost `FeedControllerTest.php`'s `[$headers, $body] =
  explode(...) + ['', ''];` its own `$body` precision. Fixed to mirror
  real PHPStan's own `InitializerExprTypeResolver::getPlusType`'s
  general (non-constant-array) case: union both operands' key types and
  both operands' value types via the existing `array_key_type`/
  `array_element_type` helpers, preserving `ArrayList`/`NonEmptyArray`
  accessories when the real algorithm would. Scoped 64 -> 59 (5 findings
  removed cleanly, zero new false positives). Also investigated (not
  built): the remaining 4-item `int|string` cluster left over from the
  `eeee436` round (`BlockManager::apply`, `CategoryAvailableList
  Controller::__invoke`, `SpecStructureTest`, `openapi-generate-input-
  dto.php`'s `narrowingExpr()`) is now fully root-caused - real PHPStan's
  `Type[]` PHPDoc-shorthand resolves to a `BenevolentUnionType`, not a
  plain `Union`, and `RuleLevelHelper::accepts()` treats it specially
  (confirmed via `dumpType()`: shown parenthesized, `(int|string)`, and
  un-flagged against a `string`-typed parameter, vs. a plain `int|string`
  union of the identical shape which *is* flagged). phpstan-rs has no
  such marker anywhere, so every `Type[]`-shorthand-derived key type is
  wrongly treated as a strict `Union` - a real, disclosed gap needing a
  genuine `Union`-adjacent "benevolent" marker threaded through parsing,
  the `+` operator, and every `is_assignable`/`accepts_at_configured_
  level` call site that should respect it, correctly scoped to the same
  future phase already reserved for `checkBenevolentUnionTypes`
  (ROADMAP.md's Phase 84 entry), not attempted here. Unscoped: 40017 ->
  40033 (all new findings verified inside third-party vendor code
  outside Piwigo's own real `phpstan.neon` scan scope - genuine, newly-
  surfaced precision improvements in vendor code the array `+` fix now
  reveals rather than masks with `Mixed`, e.g. `league/uri`'s own
  `Uri::fromComponents()` chain; not chased down, no real-PHPStan ground
  truth exists for that scope anyway) - current baseline for future
  diffs.
- `cad1fc1` - two more small stdlib precision gaps, same corpus-diff
  round. `array_keys()`'s own key-type-propagation arm gated on `args.
  len() == 1` only, so the real, common 2-/3-arg `$search_value`/
  `$strict` form fell through to the bare stub's own useless `int[]|
  string[]` answer (`UserService::searchCaseUsername`'s own `array_keys
  ($byLower, $usernameLower, true)[0]` against a declared `string`
  return type) - `$search_value`/`$strict` only ever narrow *which* keys
  survive, never the key type itself (confirmed via `dumpType()`: real
  PHPStan returns the identical `list<string>` for the 3-arg form that
  it does for the bare 1-arg form), fixed by widening the guard to
  `(1..=3).contains(&args.len())`. `array_filter()`'s own recognized-
  callback-shape set was missing a real, common third case: a native
  `is_string`/`is_int`/`is_float`/`is_bool`/`is_null` type-check
  predicate (`TagService::buildTagList`'s own `array_filter
  ($altNamesEvent->value, is_string(...))`, the first-class-callable
  form, lost its own precision down to `mixed`) - confirmed via
  `dumpType()` that real PHPStan narrows the retained element type to
  exactly that type; new `is_type_check_filter_callback` recognizes the
  first-class-callable form alongside the same arrow-fn/closure wrapping
  shapes the existing `instanceof` recognizer already handles. Scoped
  59 -> 57 (both target findings removed cleanly, zero new false
  positives). Unscoped 40033 -> 40028 (5 more removed in third-party
  vendor code, zero new) - current baseline for future diffs.
- `ee3500d` - corrected `function.duplicate` (Phase 63) from always-on to
  opt-in, matching its real-PHPStan rule's own `#[ValidatesStubFiles]`-
  only scope (real PHPStan never runs `DuplicateFunctionDeclarationRule`
  against ordinary scanned project files at all, only a project's own
  configured `stubFiles:` list) - the identical reasoning that already
  made the class-side sibling check (`class.duplicate`, Phase 83) opt-in.
  Real corpus false positive found: `C13yInternalTest`'s own `Unit`/
  `Integration` suites each unconditionally declare an identically-named
  top-level function (`c13yInternalTestCheckIntegrity`), never actually
  loaded together in the same PHP process. New `Registry::check_
  duplicate_function_declarations` (opt-in, default `false`), full
  config/CLI/`Project`-setter wiring mirroring `check_duplicate_class_
  declarations` exactly. Scoped 57 -> 55 (both target findings removed
  cleanly, zero new). Unscoped 40028 -> 38168 (zero new; the false-
  positive class turns out to be broad, not just the one test file -
  `thecodingmachine/safe`'s own PHP-version-split generated stub
  directories, e.g. `vendor/thecodingmachine/safe/generated/8.1/*.php`
  vs. its `8.2`/`8.3`/... siblings, declare the identical function names
  in every version directory, never co-loaded) - current baseline for
  future diffs.
- `6de5bc8` - patched `usleep()`'s own phpstorm-stubs-refined `int<0,
  max>` parameter back to plain `int` (`php_stubs::patch_usleep`),
  matching real PHPStan's own `functionMap.php` entry (plain `int`) -
  confirmed via `dumpType()`/a real `vendor/bin/phpstan analyse` run that
  neither an unconstrained `int` nor a literal `-5` is ever flagged, since
  `NativeFunctionReflectionProvider` never consults phpstorm-stubs' own
  PHPDoc for a `functionMap.php`-covered function. Real corpus false
  positive fixed: `RegisterControllerTest::registerMaturedKey`'s own
  `usleep((int) ($remainingSeconds * 1_000_000))` inside an `if
  ($remainingSeconds > 0)` guard - this project has no float-range-
  narrowing to prove the cast's own result is non-negative, moot once
  `usleep`'s own parameter matches reality. Scoped 55 -> 54 (target
  finding removed cleanly, zero new). Unscoped confirmed the same
  pattern is real and broader than Piwigo alone: 38168 -> 38154, 15
  identical `int<0,max>`-vs-plain-`int` false positives removed across
  third-party vendor code (Symfony, Rector's bundled deps, phpbench,
  revolt/event-loop) with zero new; 2 more sites' own wording simply
  updated (`int<0,max>` -> `int` in the message text) since they pass a
  genuinely nullable `int|null` argument, still correctly flagged either
  way - not a regression.
- `4de673e` - typed `proc_open()`'s own by-ref `$pipes` out-parameter
  (previously falling to generic `Mixed` leniency) as `array<int,
  resource>` - real PHPStan's own separate internal `vendor/phpstan-src/
  stubs/core.stub` (a distinct, smaller bundle from phpstorm-stubs,
  deliberately excluded from this project's own stub pipeline per
  `build.rs`'s own exclusion comment) types it via `@template P of int`/
  `@param-out array<P, resource> $pipes`, correlating the out-param's key
  type to `$descriptor_spec`'s own literal keys (confirmed via
  `dumpType()`: `array<1|2, resource>` for a `[1 => ..., 2 => ...]`
  spec) - this fix doesn't chase that template-correlation precision (no
  template-binding-through-`@param-out` consumer exists in this
  codebase), just the generic-`Mixed` false positive, which already
  satisfies the real corpus shape. This traces `usleep`'s own earlier
  "broader, unconfirmed pattern" note to a real, concrete, disclosed
  future item (documented in ROADMAP.md, not chased generally this
  round). Scoped 54 -> 52 (both target findings removed cleanly, zero
  new). Unscoped 38154 -> 38152 (the same 2 findings, zero new) -
  current baseline for future diffs.