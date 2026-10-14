# Work stack

The one file that says what is paused, in what order, and where to resume.
Work gets paused to fix something that blocks it, and the fix gets its own
plan; this file keeps the chain so none of it is forgotten.

**Rule:** when work is paused for a fix, add the fix to the top of the stack
below (what it is, why it was started, where its plan lives) instead of
starting a new tracking file. When a level is finished, mark it done here
and say which level to resume. Per-item progress stays in the level's own
documents; this file holds only the order and the pointers.

State as of 2026-10-01, `f0babecf`. Counts below were read from the files on
that date.

## Where to resume

1. Finish **level 6**: recreate master's 87 commits on `master2`, one row
   at a time (`MASTER_REIMPLEMENTATION_LEDGER.md`: 86 open, 1 skipped, none
   done).
2. Then pop back, in this order: **level 3** (the 140 open review tasks, and
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

### 6. Recreate master's commits here — `MASTER_REIMPLEMENTATION_LEDGER.md`  ← current

The 87 commits `master` has that `master2` lacks, each written again against
this branch with master's subject line, one row each. 86 open, 1 skipped.
The ledger also lists what `master2` changed that those commits meet (stored
types need serde and `PartialEq`, passes must run in scope, `project.rs`).

### 7. The 25 G Piwigo profile — `PROFILE_DRILL_PIWIGO_25G.md` (implemented, validated at `1892b477`)

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

### 8. The off-run drill — `PROFILE_DRILL_PIWIGO_OFF.md` (drilled and validated, nothing implemented)

On branch `callgrind-20g` (at `d520c9fd`, the code of `5fb858c9`). Three
callgrind runs of one binary on Piwigo:

- off (stub cache and per-file cache off): 25.63 G;
- cold (stub cache empty): 25.68 G;
- warm: 20.57 G, the 20 G plan's run.

The stub build is the whole 5.05 G between off and warm. 3.50 G of it
parses the stub files on a second rayon pool; 1.60 G runs serially on the
main thread before the parse, with the other cores idle. Every other phase
matches the warm run, and the 20 G plan's items reach every block of it.

Proposals (§10), numbered as in the report:

- For the stub build (off and cold runs):
  - 1: resolve only the stub base the files need, −355 M serial;
  - 2: serialize only what a store keeps, −1.0 G serial, off only;
  - 3: a hash index for bases built in memory, ≈ −0.29 G;
  - 4: `build_closures`' dependents looked up once, ≥ −0.2 G serial.

  1-4 overlap, together ≈ −1.6 G.
  - 5: the stub corpus built beside the parse, on the worker pool.
  - 6: the stub data built at compile time into every binary, −5.05 G. It
    needs level 2's reflection layer, since the build script must run the
    code the runtime runs. `.rodata` grows from 18.2 to ≈50 MB per binary.
    Once it lands, 1-5 speed only the off mode and the reflection crate's
    own tests (and 4 the build script). After level 2's C4 and C5 only each
    stub file's declarations are left to carry, ≈0.2-1.5 G per cold run
    (C8).
- For every run:
  - 7: a PHPDoc tag's description tokenized only when read, measured
    ≈0.08 G warm, ≈0.04 G after item 14;
  - 8: skim context files' function bodies (≈48% of the parse job's
    tokens) and parse one on demand. Every skimmed body is still checked,
    by a pass that builds only what its checks read, and each file's
    verdict is cached by its bytes and the `syntax` crate. After the plan
    it saves ≈2.0-2.3 G with the verdict cached and ≈0.55-0.85 G without
    it, at the measured d ≈ 0.005-0.006 (≈0.02 by tokens);
  - 9: purity and property writes on demand, exact (a least fixed point),
    ≈0.3-0.4 G beyond item 4, on the finishing thread;
  - 10: `simdutf8`, ≈50 M;
  - 11: text made only when read: a reported message at display (55
    level-7 messages are formatted and then dropped, 22.5 M) and no
    class-constant key without dynamic constants (6.6 M).
- Rejected: one rayon pool as a speed-up by itself, a narrower cache key,
  a dedicated autoload reader.

With the plan, proposals 6-11 and the off report's cuts (E2-E13), both modes
end at ≈9.6-10.0 G with the files' parse verdicts cached, ≈11.0-11.4 G
without; without 6, the off run ends at ≈12.9-13.3 G. With both caches
warm today's floor is 14.34 G and 1.60 s (B5, B4).

Decided on 2026-10-13 (the off report's D1-D5): the stub data is built at compile
time (reopened by C8, below); skimmed bodies are checked and their
verdicts cached; a skimmed body's text is copied from the bytes the load
reads; each cache checks once, at its first store, whether it can write.

Recommended on 2026-10-13, not yet decided:

- **D6.** A context file that fails to parse keeps what it declares: what
  parses is recovered, and what failed is unknown. Validity follows the
  configured PHP version, and the batch run and the language server use one
  rule. The parser refuses `$s{0}`, which PHP 7 accepts, and a real file
  has it. It also refuses `$x instanceof static::$name`, valid in PHP 8.5
  (the off report's E14).
- **D7.** Proposals 1-5 are built now, since proposal 6 waits on level 2's
  stage C10. Level 2's C4 and C5 remove the eager stub build they speed, so
  they pay only until then.

C7 is settled: of PHP 8.5's 268 compile-time errors the parser rejects 30.
C8 is settled: D3's form does not survive level 2. After C4 and C5 no
stub bases are left to embed, only each stub file's declarations, and a
run reaches 6-43% of the stub bytes, so proposal 6 saves ≈0.2-1.5 G per
cold run instead of 5.05 G. C5's version key should keep the gates as data
(one parse per file), or the data stops being a function of the source.
Checked-in binary data is unfit. D3 is decided again once C5 sets its
version key. These and the investigations are in `PROFILE_DRILL_PIWIGO_OFF.md` (§10,
§13 lists each item's method and data); the ledger that tracked them was
retired, and is in git at `72cac936`.

The order, from D7 (rows and state: `PROFILE_DRILL_PIWIGO_OFF_BUILD_LEDGER.md`):

1. ~~the deadlock fix (the off report's E1)~~ done at `3d5434b3` (callgrind-validated only: tests, clippy and fixtures not run); its proposals 1 and 5 ride on it and are measured in the ledger;
2. ~~D5's write check, which carries proposal 2~~ done at `c2cd1586` (callgrind-validated only);
3. proposal 3;
4. proposal 4.

Each is measured with paired cold runs. Proposal 6 is not built in its
current form; it is decided again with level 2's C5. Proposal 8 waits on
9; B1 measured d ≈ 0.005-0.006. No proposal is implemented.

The ledger's items were checked one by one on 2026-10-14 and the report corrected.
The check found two bugs in today's code, neither fixed:

- **E14.** The parser refuses `instanceof` with a static property as the
  class; six files in installed dependencies lose every declaration as
  context. Running the parser over all of `vendor/` against `php -l` would
  find any other such gap; that run has not been made.
- **E15.** The build's source hash leaves out `vendor/phpstan-src/stubs`,
  which the stub build reads, so an edit to a `.stub` file alone leaves a
  stale stub cache in use.

The heap drill of the same three runs, with whole stacks, is
`PROFILE_DRILL_PIWIGO_OFF_HEAP.md` (done). Every allocation maps to a 20 G
item or to proposal 6, and the peak falls in `expect_files` with every tree
live. It adds H1: write the stub cache file from one buffer, −15.3 MB at
the cold run's peak. H1 is not implemented. The profiles are taken with the
patched dhat-rs in `patches/dhat`.

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
