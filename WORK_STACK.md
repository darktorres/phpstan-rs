# Work stack

The one file that says what is paused, in what order, and where to resume.
Work gets paused to fix something that blocks it, and the fix gets its own
plan; this file keeps the chain so none of it is forgotten.

**Rule:** when work is paused for a fix, add the fix to the top of the stack
below (what it is, why it was started, where its plan lives) instead of
starting a new tracking file. When a level is finished, mark it done here
and say which level to resume. Per-item progress stays in the level's own
documents; this file holds only the order and the pointers.

State as of 2026-10-03, `48669123`. Counts below were read from the files on
that date.

## Where to resume

1. **Level 8**: building `PROFILE_DRILL_PIWIGO_20G.md` row by row from
   `PROFILE_DRILL_PIWIGO_20G_LEDGER.md`, item 0 first; each item is
   validated at its end, and O3 closes it. Only the rows that optimize are
   built now; the rest are DEFERRED.
2. Then **level 8's deferred rows**, as a plan of their own: the PHP
   catalog, and every row of the 20 G plan that changes what the analysis
   finds beyond what the optimization needs, validated together (the
   ledger's row D.V).
3. Then **level 6**: recreate master's 87 commits on `master2`, one row
   at a time (`MASTER_REIMPLEMENTATION_LEDGER.md`: 86 open, 1 skipped, none
   done).
4. Then pop back, in this order: **level 3** (the 140 open review tasks, and
   the A/B ledger's open steps 19 and 21 on `master`), then **level 2** (the
   crate split from Stage C on), then **level 1** (the lanes).

## The two checkouts

| | branch | directory | holds |
|---|---|---|---|
| main | `master` | `/home/torres/projects/phpstan-rs` | the fixes to the stage A/B review findings: 87 commits `master2` lacks, and their ledger, `AB_LEDGER.md` |
| second | `master2` | `/home/torres/projects/phpstan-rs-master2` | the crate split, the stage reviews, the gate speed-up, the start-up and build cost work |

Merge base `5d86aac1`. A rebase or a cherry-pick of master's commits onto
`master2` conflicts with the registry rework, so they are written again
(level 6).

## The stack, oldest first

### 1. The roadmap, in two lanes — `ROADMAP.md`

The main plan: phases toward PHPStan-level checking, split between two Claude
sessions working in parallel on separate checkouts, one lane each (ground
rules for both: build the complete, correct version, and treat PHPStan's
behaviour as the minimum, not the target). **Paused** on 2026-09-23 (Lane B
reported "paused with a clean tree" at `47ca5bb`; the branches converged at
`ab4c2b5`) to split the codebase into crates.

- **Lane A** (the `master` checkout): PHPDoc, the type system, narrowing.
- **Lane B** (the `master2` checkout): declaration and OOP rules, and tooling
  (Composer, baseline, codemods, output formats).
- `ROADMAP.md` names which lane owns which phase (around line 1810).
- Where it stopped, from the report written at the pause (2026-09-23, so
  check `ROADMAP.md`'s phase statuses for anything newer):
  - Lane A, remaining: Phase 130 (the PHPDoc-versus-native check for
    properties and hooks was next; `*.unresolvableType`; template checks on
    callable shapes; `@phpstan-self-out`; assert and `@var`-family checks;
    conditional-return declarations), Phase 131 (templates on `@method`, the
    rest of `@use<...>`), Phase 134 (`nullCoalesce`/`isset`/`empty`,
    `compact()`, `unset()`), items 173, 140d, A4, A6, O1, 180, and a
    known limit with narrowing on two correlated conditions.
  - Lane B, remaining: Phase 139 (`InvalidTypesInUnionRule`, malformed
    `@phpstan-ignore`), Composer phases 113-120, baseline Phase 122
    (N1, N2, N4), output formats (Phase 154), codemods (Phase 111, I1-I4).
  - Unassigned, whoever frees up first: Phase 87 (whole-program value
    flow), 89 (structural duplication), 91 (parameter narrowing from call
    sites), 94 (closed-world precision), AA, and Parts 10 and 11 (the
    Packagist extension survey, the Psalm-derived findings).
- The report itself is in the session transcript
  `~/.claude/projects/-home-torres-projects-phpstan-rs/5b4ed89e-44f2-40ee-b852-a4162eb0bae8.jsonl`
  (message at 2026-09-23T03:15); the lane sessions' exchanges are in that file
  and in `-home-torres-projects-phpstan-rs-master2/ddf6301a-50ab-484b-9119-41a790c6f310.jsonl`.
- Resume: when levels 2-6 are done. Edits `GATE_SPEED_PLAN.md` §5 lists for
  `ROADMAP.md` are recorded there.

### 2. The crate split — `CRATE_SPLIT_PLAN.md` and its companions

Moves the package into a workspace of crates (stages A to G). **Paused** at
Stage C to review what stages A, B and C had produced (level 3).

- Done: stages A and B, Stage C's C1-C3 (the module graph's `finished`
  line: A0 A1 A2 B1-B9 C1 C2 C3). Open: the rest of C, then D, E, F, G.
- Companions: `CRATE_SPLIT_AUDIT.md` (item-by-item check of stages A-B7
  against the code), `CRATE_SPLIT_EVIDENCE.md` (the measurements behind each
  stage item, not kept current), `CRATE_SPLIT_CAUSES.md` (cause lists of the
  oracle mismatches, regenerated, not edited).
- Resume: Stage C, after level 3.

### 3. The reviews of stages A, B and C — `STAGE_*_REVIEW.md`

Reading every commit of the stages against the code, with findings as tasks.
**Paused** because the test suites took too long to run (level 4).

| file | covers | open | done |
|---|---|---|---|
| `STAGE_B_REVIEW.md` | Stage B follow-ups | 17 | 220 |
| `STAGE_B_FIX_REVIEW.md` | the fixes to that review | 107 | 241 |
| `STAGE_C_REVIEW.md` | Stage C so far | 16 | 14 |

- `AB_LEDGER.md` (on `master` only) tracks the plan that fixes the stage A/B
  review findings, at `~/.claude/plans/peppy-drifting-coral.md` (outside the
  repo): the 87 commits of level 6 are that work. At the last look: steps 1-18
  and 20 (the test pass) done; step 19 (summary-mode tuning) and step 21
  (finishing the second version of the plan) open, 16 unticked boxes.
- Resume: work the open tasks in each file; tick them there; finish the A/B
  ledger's steps 19 and 21 on `master`.

### 4. Faster gates — `GATE_SPEED_PLAN.md`

Cut `cargo xtask gates` from 37.5 to 13.6 minutes without weakening any gate.
**Done** (steps S0-S15, result at `39a4d07c`). Two parts of it grew beyond
the plan:

- the start-up cost of one fixture, folded into
  `PROJECT_BUILD_COST_INVESTIGATION.md` §12 and §12.1 (the plan file for it,
  `STARTUP_COST_PLAN.md`, is in the history, `441f9366`);
- the Project build cost (below).

### 5. The Project build cost — `PROJECT_BUILD_COST_INVESTIGATION.md`

Why building a `Project` and a test cost so much, and the fixes: shared
resolved stub base, scoped incremental finishing, a stored stub cache,
precise rechecks and more. **Done**; the measurements and what was decided
against are in §12. The profiling method is in `CLAUDE.md` and the README.

- Two things to carry into level 6: the 23 renames mapped in
  `gates/test-renames/build-cost.txt` overlap master's `0ad9589b` (which
  records 22 of them), so drop one set when that commit is recreated; and
  the module-graph edges added here (`gates/module-graph.txt`) may meet
  master's own edits to that file.

### 6. Recreate master's commits here — `MASTER_REIMPLEMENTATION_LEDGER.md`

The 87 commits `master` has that `master2` lacks, each written again against
this branch with master's subject line, one row each. 86 open, 1 skipped.
The ledger also lists what `master2` changed that those commits meet (stored
types need serde and `PartialEq`, passes must run in scope, `project.rs`).

### 7. The 25 G Piwigo profile — `PROFILE_DRILL_PIWIGO_25G.md` (done; validated at `1892b477`, its later work read by level 8's item 0)

Validated: `cargo test --workspace` and the fixtures (3,818) pass; the Piwigo
O2 checksum equals the baseline's (`6e5fa444…`, from `7a3f3f99`); both binaries
parse all 17,393 vendor files with no parse error; `hyperfine` on Piwigo,
alternating, warm, load average 6-7: 2.46 s to 2.20 s wall (user 10.2 s to
8.4 s). Not run: callgrind (about 25 G Ir under valgrind) and the O1
front-end dumps (the dump hashes the whole `ParsedFile`, which changed, so no
baseline compares).

Validation found and fixed: a `new` class counted as a constant in the
dependency walk; a completion cursor past the end of a stale line; `if
(false)` declarations treated as never run; the resolution memo answering a
type written `\stdClass` with the one written `\StdClass`; and two wrong
tests of mine. The vendor-tree report differs from the baseline's in
improvements only (read: loops with no `break` end, so `return.missing` goes and
`deadCode.unreachable` appears; an arrow function's body takes `xor`; a method's
docblock is inherited across case-different names; union member order).

- Built after that validation, each with a debug-build oracle, not yet
  validated together: tests and fixtures pass, O2 equals the baseline, callgrind
  20.57 G Ir (from 25.14 G), warm 1.79 s with the file cache and 2.48 s
  without:
  F3 member-level and map-level sharing; the C3 tail except step 6 (identity
  `Name` equality, one interner, `Borrow<str>` removed); C4's structured
  `LocalKey` with a dependents index; C1's exit and throw points and the
  shared completion rules; the P11 byte lexer for tags; `FileId`
  pre-assignment; §14 (body summaries for purity, property writes, assigned
  properties and the uninitialized-property flow, declarations, Pest facts
  and declared classes kept per file, context trees dropped in a batch run
  and parsed again on demand, and the per-file cache under
  `~/.cache/phpstan-rs/files/<build>/`, keyed by the file's bytes, path,
  parse options and the build, checksummed, size-capped); P12 step 2 (the
  parser records references and doc sites) and facts-gated declaration and
  body walks.
- Not built:
  - P7 direct lowering and retiring the text grammar: a sampling profile of
    Piwigo at this state puts `parse_written_type` at 0.18% inclusive and
    `to_source_string` at 0.09%, so the saving the plan counted is gone (the
    docblock, text and resolution memos).
  - §12.10 (observers, record and replay): a rewrite of the checker's 335
    rule entry points, family by family, each needing its O2 diff read; not
    started.
  - C3 step 6 (`String` keys to `Name` by module), C1's effects and yields
    as traversal products, `collect_throws`, pull-parsing for the tag lexer.
  - P12 step 3 as written (the declaration list from the parser: it cannot
    hold references into a tree still being built) and the fusion of
    `InitFlow` into the summary walk (its builder follows statement
    structure and evaluation order).
  - Guards on `class_exists`-style tests: the stubs carry no per-version
    availability; it needs `@since`/`@removed` read into the stub index.
- Behaviour changes to read in the O2 diff: the Pratt parser's rejection of
  nested unparenthesized ternaries and chained comparisons, `__halt_compiler`
  ending the lex, kind-exact dependency keys, a hyphenated non-keyword word in
  a docblock being no class name, declarations ruled out by the configured PHP
  versions not being indexed, and UTF-16/UTF-8 columns in the language server.
- Two tests of `syntax::span` were wrong and are fixed; expect a few more.
- **Carried into level 8.** The later work above, never validated together,
  is read first by level 8's item 0 (step 1: the vendor-tree, all-on and
  unscoped reports against `1892b477`, each difference read), before
  anything is built on it. Each item not built here is designed or ruled on
  in level 8's plan:
  - P7's lowering and the text grammar's retirement: item 14, now for
    correctness as much as cost (one parse per doc, where today two parses
    split a doc two ways; a lowering oracle with its differences listed),
    with the second parse's 144.0 M and the round trip's 118.3 + 83.3 M;
  - §12.10: item 18, step 1;
  - C3 step 6: item 15;
  - C1's effects and yields, and `collect_throws`: item 18, step 2;
  - P12 step 3 and the `InitFlow` fusion: item 13 (ids make the list
    possible while the tree is built; the one walk visiting in PHP's order
    of evaluation is deferred until after O3);
  - the guards on `class_exists`-style tests: item 6, step 2 (availability
    on run-time evidence only, from the stubs' version ranges), deferred
    until after O3;
  - pull parsing for the tag lexer: item 14, step 5 (a token slice read by
    index instead).

### 8. The 20 G Piwigo profile — `PROFILE_DRILL_PIWIGO_20G.md`  ← current

The callgrind drill-down of the 20.57 G run at `5fb858c9` (level 7's end
state), the dhat heap profile of the same run (`target/dhat-heap.json`; its
2.67 GB and 19.4 M allocations each given to an item, §8), and the plan built
from both: items 0-19, each validated at its end (§11), with O3 once after
item 19. The plan optimizes: fewer instructions, less wall time and less
memory. What it gathered beyond that, new checks and changes to what the
analysis finds that the optimization does not need, is deferred until after
O3 (2026-10-05; the plan's §9, "Only what optimizes").

- **State: building, item 4.3.** Items 0-2 are closed. Item 3's
  implementation is done (3.1-3.2); its validation at 3.V passed the gate
  workspace build, workspace tests, and the scoped and all-on oracle corpus
  runs. The fixture suite reported 3,857 passed and 50 failed, matching the
  existing failure count recorded at 2.V. The 3.4 scratch-store row remains
  with item 9. Row 4.1 made the four public declaration tables private behind
  `Registry` read accessors (`e06503ad`, preserving owned stub-table
  extraction). Row 4.2 added stub, context-base and project ID ranges
  (`1b8f79aa`); `cargo check -p phpstan-rs --lib --tests` and Clippy passed.
  Row 4.3 is under way: the pinned `la-arena` dependency, typed per-file
  arenas, and class/member/property/constant/name and function/parameter
  range layouts and accessors are in place (`b7651f06`, `105f4b2b`); an arena
  serialization codec is in `00321546`, with a typed-range round-trip test
  at `c9efbee6` and deserialization bounds checks at `0e522f0b`. The parsed
  declaration collector now populates those ranges, and `ParsedFile`, the
  retained `FileState`, and the disk cache carry the arena. Validation for
  this increment passed: the focused round-trip test, an end-to-end
  `CachedFile` serialization/reconstruction test, `cargo check -p phpstan-rs
  --lib --tests`, and `cargo clippy -p phpstan-rs --all-targets`.
  Property and constant ranges share their existing `Arc` values rather than
  deep-copying reflections into the arena.
  The cache stores class/function declaration metadata beside the arena
  values, then reconstructs `BuiltDecl` with the same `Arc`; mismatched
  metadata is rejected so loading falls back to parsing. The end-to-end cache
  test checks class, function and global-constant reconstruction, pointer
  sharing, and mismatch rejection.
  Registry name tables and resolution still read legacy declaration values;
  their migration remains open. The plan was committed at
  `d520c9fd`. A separate reviewer goes through it adversarially, one item at
  a time, in order; each finding is checked against the code, `php` 8.5 and
  the heap profile before it goes in, and §13's review table has a row per
  item. Items 0-19 and §11-§13 were reviewed and committed one by one.
- **Passes outside the item order,** also in §13: a dependency pass (six
  crates taken: `la-arena`, `wide`, `triomphe`, `quick_cache`, `siphasher`
  and `blake3`; ten considered and not taken; item 15's review added
  `encoding_rs`),
  and item 6, step 6: the caches' key covers every input the build embeds,
  where today it misses `vendor/phpstan-src/stubs`.
- **The projection,** §12, estimated: ≈290 MB at exit (from 657), a
  ≈350-385 MB peak (from 866) plus the checks in flight, ≈1.85 M allocations
  (from 19.4 M), and roughly 8 G of the 20.57 G instructions.
- **Building it** started on 2026-10-03. Its tasks, in order, with their
  state and commits, are in `PROFILE_DRILL_PIWIGO_20G_LEDGER.md` (items 0-19, each closed by its
  validation row, then O3). Reading the
  starting state (0.1) found four false-report bugs, fixed in item 0
  before the baseline (rows 0.1a-0.1f), and added the implicit calls, now
  part of item 0. The latest completed implementation is row 3.2 at
  `dfa2cebe`; row 4.1 is committed at `e06503ad` and row 4.2 at `1b8f79aa`.
- **Item 0's validation (0.11)** is under way: the tests, fixtures and
  runtime oracle pass, and the edit replay over Piwigo and every vendor
  project found the stale diagnostics and registry state the plan's "Found
  in item 0's validation" block lists, each fixed. The dumps and O2 became
  one pass, `cargo xtask corpus --full` on a worker per core (rows
  0.11a-0.11c, done), with `cargo xtask progress --watch`. Done since:
  fast validation (0.11d-0.11j), from the profile of a replay and a vendor
  project's run, and every level's report from one analysis (0.11v); the
  builtin-runs table built into static arrays at compile time (0.11n1);
  the fix's precision (0.11o-0.11q): closures' writes where they run, calls
  through a parameter resolved at each call, captures at a file's top
  level; `cargo xtask profile` (0.11s), whose first profiles of amphp/amp
  are a vendor run of 286 M instructions, 0.06 s, 80 MB and a replay of
  16.8 G, 2.32 s, 111 MB (`~/.cache/prs-profile/*-91537c79`). Next, before
  the baseline: the other kinds' profiles (0.11t: `config scoped` 202 G, 33.8 s,
  1.75 GB; the corpus session warm 29.4 s, 7 GB; callgrind stopped, the
  rest skipped) and `cargo xtask validate` (0.11u, done). The validation
  itself (0.11) was started and stopped at the runtime oracle (build and
  tests passed): no snapshot of item 0 is recorded, so item 1's validation
  has nothing to compare with until one is.
  Item 1 (rows 1.2-1.8) is done and its validation (1.V) skipped by the
  user, with no run and no snapshot. Item 2 (rows 2.1-2.7) is built; its
  validation (2.V) is the tests. The plan's validation (§11) compares no
  dumps from here on: no snapshot, no baseline, no equalities between runs,
  and the items' Oracle bullets name tests and fixtures only.
- **Deferred until after O3, not optimization** (2026-10-05): in item 0,
  the implicit calls (0.12a-0.12f; magic and interface methods, property
  hooks and a property's magic where it may be unset were built first,
  `e4923d51`, `2786f1f2`, `c89c2e37`), the assumed types (0.13a-0.13k),
  PHPDoc types as claims verified by a fixed point (0.14a-0.14g), an open
  world by configuration (0.15a-0.15f), and the runtime oracle reaching
  every asserting fixture (0.11r); in items 1-19, every new check and every
  change to what the analysis finds that the optimization does not need,
  each row marked in the ledger and each part marked where the plan
  designs it.
- **The PHP catalog is deferred until after O3** (2026-10-05): every
  builtin and engine operation read from php-src per PHP version, rows
  0.11k2-0.11m and 0.11n2, with 0.13f and 0.13g that read it. Its
  whole-program pass (`dbbddd8d`) did not fit in 10,000 MiB on 8.4; the
  plan's deferral note, before "The PHP catalog, from PHP's source", says
  why and what to settle before resuming. Until then the rows that read
  it take today's sources, the builtin-runs table and the stubs.
- **Found along the way, done:** version gates for the syntax PHP 7.1 to 8.5
  added (`492ccf0e`, `50fa1e43`, `d8090e5e`), and `php_version` taking a
  range (`dfdfeeb4`). The runtime oracle calls each asserting function with
  values of its parameters' types (`3d8085f3`): 353 fixtures run, from 81.
- **Decided, in item 0 (rows 0.13a-0.13k), now deferred until after O3:** three assumptions the
  oracle's calls showed against what PHP does are built as PHP's truth,
  after the implicit calls (rows 0.12a-0.12f), both before the baseline.
  Int operations get the float PHP gives past a limit, exact, with its
  consequences. A read at a key not proven present gives `null` and is
  reported as PHP warns, with presence proven, and so does a property read
  where the type leaves the property's presence unknown. Every builtin
  returns and throws what the PHP catalog derives from php-src where its
  stub says less; builtin iterators off a valid position are the case
  found, with `valid()` proving the position. A declaration throws what
  its body may throw, settled with its effects, and `@throws` is checked
  against that instead of trusted: today `@throws void` on a function
  dividing by its argument leaves a `catch (\Throwable)` around it
  unentered, so the state after the `try` misses the catch's. Their seven fixtures opt out of the
  calls until then. The implicit calls also took generator bodies: a
  generator's body runs at its resumptions, not at its call. An audit
  against `php` 8.5 added property hooks, lazy objects' initializers, the
  cycle collector (an `unset` of one `stdClass` ran 9,980 destructors of
  earlier cyclic objects), a generator's `finally` at its release, signal
  handlers, included files, and libxml2 reaching user stream wrappers;
  a second added fibers, `__autoload` before 8.0, `#[\Deprecated]`
  callees, and no destructor at a release proven not the last; a third
  added throwing implicit calls (an error handler turning warnings into
  `ErrorException`), images built with every extension (the official
  ones lack `pcntl`, `intl` and `xsl`), `phpdbg -p*` for the construct
  map (7.0 has no opcache listing), and strings' numericness as a
  condition (`"abc" * 1` throws on 8.x).
- **Moved into item 0 (row 0.11r), now deferred until after O3:** the 114 fixtures that assert and
  never reach an assertion under the runtime oracle: assertions in closures
  nothing invokes, code that throws first with the values sampled,
  parameters no value can be made for, methods of traits.

## Documents with no place in the stack

Standalone research and plans; none blocks anything:

- `CRASH_REPORTING_PLAN.md`
- `MAGO_ARCHITECTURE_COMPARISON.md`
- `PIWIGO_CORPUS_TRIAGE_ab4c2b5.md`
- `TOP100_PHPSTAN_EXTENSIONS.md`, `TOP100_RESEARCH_NOTES.md`
- `UNSAFE_PLAN.md`

Proposed: move them under `docs/research/` so the repository root holds only
the stack's files. Not done yet; `master` carries the same files, so a move
here is a rename there on merge.
