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
2. Then pop back, in this order: **level 3** (the 140 open review tasks) and
   **level 2** (the crate split from Stage C on). **Level 1** (the lanes) was
   being carried on `master` in the meantime; what is left of it is in
   `AB_LEDGER.md` there.

## The two checkouts

| | branch | directory | holds |
|---|---|---|---|
| main | `master` | `/home/torres/projects/phpstan-rs` | the lanes' work: 87 commits `master2` lacks, `AB_LEDGER.md` |
| second | `master2` | `/home/torres/projects/phpstan-rs-master2` | the crate split, the stage reviews, the gate speed-up, the start-up and build cost work |

Merge base `5d86aac1`. A rebase or a cherry-pick of master's commits onto
`master2` conflicts with the registry rework, so they are written again
(level 6).

## The stack, oldest first

### 1. The roadmap, in two lanes — `ROADMAP.md`

The main plan: phases toward PHPStan-level checking, split between two
instances working in parallel, lane A and lane B (`ROADMAP.md` names which
lane owns which phase). **Paused** to split the codebase into crates.

- Lane work has gone on in the `master` checkout and is tracked in
  `AB_LEDGER.md` (on `master` only), which follows the plan at
  `~/.claude/plans/peppy-drifting-coral.md` (outside the repo).
  At the last look: steps 1-18 and 20 done; step 19 (summary-mode tuning) and
  step 21 (finishing the second version of the plan) open.
  *Confirm that this plan is the lanes' work before relying on it.*
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

- Resume: work the open tasks in each file; tick them there.

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
