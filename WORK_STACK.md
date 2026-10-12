# Work stack

The one file that says what is paused, in what order, and where to resume.
Each level's progress lives in its own documents; this file holds the order
and the pointers.

**Rule:** when work is paused for a fix, add the fix to the top of "Where to
resume" (what it is, why it was started, where its plan lives) instead of
starting a new tracking file. When a level is finished, mark it done here and
say which level to resume.

The full version of this file, with the per-item progress that was dropped
here, is `git show 2783a866^:WORK_STACK.md`.

State as of 2026-10-11, `master` at `2783a866`.

## Where to resume

0. **Level 9, the HEAD measurement drill: resume here.** Ledger:
   `PROFILE_DRILL_PIWIGO_HEAD_LEDGER.md`. It holds the measurements taken at
   `97b8f3f0` (callgrind in three modes, hyperfine, heap, the Piwigo output and
   the oracle corpus), the tasks that follow from them, and the generated
   instruction and allocation drill. Work one task at a time, in the ledger's
   order, and keep a change only if it lowers instructions for its own targets
   and wall time does not get worse under alternating timing. The batch's
   output gate (task F1 in the ledger) is still open.
   **CI repair: done** (it blocked the `lint` and `fixtures` gates). Plan:
   `/home/torres/.claude/plans/pure-jumping-wolf.md`. Parts 1 and 2 (items 1-9b)
   are on `origin/master`, and the latest CI on `8ad93491` is green: Rust CI run
   `38174223982` and Extension CI run `38174224048`, both on branch `master3`.
   Still open from the plan's speed-up part: item 10 (move the
   no-default-features fixtures into their own `fixtures-no-default-features`
   job; its precondition, a green default-features run, now holds) and item 12
   (re-measure after CI runs).
1. **Level 8, the builtin-runs recorder: done** (`BUILTIN_RUNS_RECORDER_PLAN.md`,
   steps 1-12 committed; table regenerated at `bc9ab94f`, 10,725 rows). The
   uncommitted E2.V edits to `registry/effects.rs` are not in any checkout; they
   survive only in a throwaway copy, `~/.cache/prs-profile/engine-new/`, which
   is not a git repository. Level 7's item E resumes at its E2.V gate.
2. **Level 7, the 20 G profile: paused behind level 9.** Plan:
   `PROFILE_DRILL_PIWIGO_20G.md`; rows in `PROFILE_DRILL_PIWIGO_20G_LEDGER.md`
   (items 0-19, each closed by its tests-only gate; full validation and O3 once
   after item 19). The ledger's first open row is 10.3: items 9 (including its
   9.V gate) and 10.1-10.2 are done. Item E (effect resolution): E1.V.2 is
   closed, with its 30 remaining extra rows deferred to the PHP catalog
   (EG.15-EG.20); E1.V.3 is done, so E2 has started. Still open: E1.2.4 and
   EG.2 (route (a), built after E2.V) and the E2.V gate itself. The E2 part 2
   batch is in `PROFILE_DRILL_PIWIGO_9071_BATCH_PLAN.md`; its verification runs
   once, after item 14.
   **Deferred, E2.V step 3:** Piwigo gives 613 findings, not the batch's 611.
   The two extra are `src/Piwigo/Search/SearchFilterRenderer.php:442`
   (`array_intersect` given `list<int|string>|null`) and
   `tools/openapi/ResponseBodyCallSiteVisitor.php:79` (`property.notFound` on
   `PhpParser\Node\Expr::$name`). Both came in between `9071f48f` and
   `97df097c`, not from the two fixes of this E2.V (`5ddb4436`, `97b8f3f0`),
   which leave the output byte-identical. To do: find the commit that adds
   them, and check whether each is a true positive. Until then the output gate
   is open.
3. **Level 7's deferred rows:** the PHP catalog, and every row of the 20 G plan
   that changes what the analysis finds beyond what the optimization needs,
   validated together (ledger row D.V). Both wait until after O3.
4. **Then pop back, in this order:** level 3 (140 open review tasks, and steps
   19 and 21 of the stage A/B fix plan, `~/.claude/plans/peppy-drifting-coral.md`),
   then level 2 (the crate split from Stage C on), then level 1 (the lanes).

## The checkouts

| | branch | directory | commit |
|---|---|---|---|
| main | `master` | `/home/torres/projects/phpstan-rs` | `2783a866` |
| second | `master2` | `/home/torres/projects/phpstan-rs-master2` | `8053863c`, one commit behind `master` |
| third | `master3` | `/home/torres/projects/phpstan-rs-master3` | `92c046ec`, a newer commit I did not make, on top of `8053863c` |

## The stack, oldest first

### 1. The roadmap, in two lanes — `ROADMAP.md`

**Paused** on 2026-09-23 to split the codebase into crates. Two Claude
sessions worked in parallel, one lane each. `ROADMAP.md` (the lane-ownership
note, around line 1782) assigns the remaining large phases: Lane A owns Phase
87, Phase 91, Phase 94 and Part 11; Lane B owns Part 10, Phase 89 and Phase 97.
The lane sessions' transcripts are
`~/.claude/projects/-home-torres-projects-phpstan-rs/5b4ed89e-44f2-40ee-b852-a4162eb0bae8.jsonl`
(the pause report, at 2026-09-23T03:15) and
`~/.claude/projects/-home-torres-projects-phpstan-rs-master2/ddf6301a-50ab-484b-9119-41a790c6f310.jsonl`.

Resume when levels 2-5 are done. The `ROADMAP.md` edits that
`GATE_SPEED_PLAN.md` §5 lists are recorded there.

### 2. The crate split — `CRATE_SPLIT_PLAN.md`

Moves the package into a workspace of crates (stages A to G). **Paused** at
Stage C to review what stages A, B and C produced (level 3).

- Done: stages A and B, and Stage C's C1-C3 (the `finished` line of
  `gates/module-graph.txt`). Open: the rest of C, then D, E, F, G.
- Companions: `CRATE_SPLIT_AUDIT.md`, `CRATE_SPLIT_EVIDENCE.md`,
  `CRATE_SPLIT_CAUSES.md`.
- Resume: Stage C, after level 3.

### 3. The reviews of stages A, B and C — `STAGE_*_REVIEW.md`

Every commit of the stages, read against the code, with findings as tasks.
**Paused** because the test suites took too long to run; level 4 (faster
gates) is done.

| file | open | done |
|---|---|---|
| `STAGE_B_REVIEW.md` | 17 | 220 |
| `STAGE_B_FIX_REVIEW.md` | 107 | 241 |
| `STAGE_C_REVIEW.md` | 16 | 14 |

The stage A/B fix plan is `~/.claude/plans/peppy-drifting-coral.md`, outside
the repo. It has no status markers, so steps 19 (summary-mode tuning) and 21
(finishing the second version of the plan) cannot be checked from here.

Resume: work the open tasks in each file and tick them there, then finish
steps 19 and 21 of the fix plan.

### 4. Faster gates — `GATE_SPEED_PLAN.md`  (done)

`cargo xtask gates` went from 37.5 to 13.6 minutes without weakening any gate
(steps S0-S15, result at `39a4d07c`). The start-up cost of one fixture went
into `PROJECT_BUILD_COST_INVESTIGATION.md` §12-12.1.

### 5. The Project build cost — `PROJECT_BUILD_COST_INVESTIGATION.md`  (done)

Why building a `Project` and a test cost so much, and the fixes: shared
resolved stub base, scoped incremental finishing, a stored stub cache, precise
rechecks. The measurements and what was decided against are in §12. The
profiling method is in `CLAUDE.md`.

### 6. The 25 G Piwigo profile — `PROFILE_DRILL_PIWIGO_25G.md`  (done)

The 20 G plan cites `1892b477` as the validated point. The 25G document's
status reports the workspace tests and the fixtures passing, and the O2 report
equal to the baseline's.

Built after that validation, not validated together: the member-level and
map-level sharing, the C3 tail, C4's `LocalKey` index, C1's exit and throw
points, the P11 tag lexer, `FileId` pre-assignment, the §14 body summaries and
the per-file cache, and P12 step 2. The 20 G plan's item 0 reads the
vendor-tree reports against `1892b477` before anything is built on them.

Not built, and placed in the 20 G ledger: P7's direct lowering (item 14), the
§12.10 observers (row 18.1), C3 step 6 (row 15.2), C1's effects and yields and
`collect_throws` (row 18.3), P12 step 3 and the `InitFlow` fusion (row 13.4),
and the `class_exists`-style guards (row EG.18).

### 7. The 20 G Piwigo profile — `PROFILE_DRILL_PIWIGO_20G.md`  (paused; see level 9)

The callgrind drill-down of the 20.57 G run at `5fb858c9` (level 6's end state),
and the dhat heap profile of the same run (2.67 GB, 19.4 M allocations), with
the plan built from both. Targets, estimated in the plan: about 290 MB at exit
(from 657 MB), a peak near 350-385 MB (from 866 MB), about 1.85 M allocations
(from 19.4 M), and about 8 G of the 20.57 G instructions. The plan optimizes;
what it found beyond that is deferred until after O3.

Status is in "Where to resume", item 2. Deferred until after O3, each a row in
the ledger: the PHP catalog, the implicit calls, the assumed types, PHPDoc
claims verified by a fixed point, an open world by configuration, and the
runtime oracle's reach.

### 8. The builtin-runs recorder — `BUILTIN_RUNS_RECORDER_PLAN.md`  (done)

`tools/builtin-runs/record.php` builds `crates/phpstan-rs/src/registry/data/builtin-runs-8.5.tsv`, which the effect
resolver reads. The recorder is sandboxed, resumable and observable, skips the
forks that cannot record, and regenerates the table. Open items are in "Where
to resume", item 1.

### 9. The HEAD measurement drill — `PROFILE_DRILL_PIWIGO_HEAD_LEDGER.md`  ← current

The measurements taken at `97b8f3f0` after the batch: callgrind in off, warm
and cold-with-store modes, hyperfine in off and warm modes, the heap profile,
the Piwigo output, and the oracle corpus. The ledger holds them with their
baselines, the tasks that follow (each with a cause and the measure that shows
it worked), and a generated drill of the instruction and allocation profiles.
The batch's total instructions fell 8.8% in off mode, but its peak memory rose
68% against `df603944`, and wall time showed no clear change. Level 7 is paused
until the ledger's tasks settle what the batch should keep.

## Documents with no place in the stack

Standalone research and plans; none blocks anything:

- `CRASH_REPORTING_PLAN.md`
- `MAGO_ARCHITECTURE_COMPARISON.md`
- `PIWIGO_CORPUS_TRIAGE_ab4c2b5.md`
- `TOP100_PHPSTAN_EXTENSIONS.md`, `TOP100_RESEARCH_NOTES.md`
- `UNSAFE_PLAN.md`

Proposed: move them under `docs/research/`, so the repository root holds only
the stack's files. Not done. `master` carries the same files, so the move is a
rename there on merge.
