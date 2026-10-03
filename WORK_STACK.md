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

### 7. The 25 G Piwigo profile — `PROFILE_DRILL_PIWIGO_25G.md` (implemented without validation)

All of Part II's commits that could be written were, gate-only (`scratchpad/gate.sh`:
format and clippy, no tests); none of the end-of-work checks has run: the full
`cargo test --workspace`, the fixtures (`--test fixtures`), the Piwigo O2
checksum (`6e5fa444…`), the O1 front-end dumps (a new baseline: `ParsedFile`
changed), callgrind and `hyperfine`. Run those first, from the commit before
`e1e54c29`, and bisect any difference.

- Not built, with the reason:
  - F3 steps 1-2 (member-level and map-level `Arc` in `ClassReflection`): a
    finishing pass writes through every member, so each shared member is
    copied anyway; it needs the passes to write only on change first. Entry
    level (`NameTable` values behind `Arc`, pointer-first `changed_since`) is done.
  - P7 direct lowering (`phpdoc::lower`) and the retirement of the text
    grammar, P11 byte lexer for tags, P12 steps 2-4 (references and the
    declaration list from the parser, the fused walk), §12.10 (observers,
    record and replay), C1 steps 5-6 (exit and throw tails, effects, shared
    rules), §14 (summaries, persistent cache), the C3 tail (identity `Name`
    equality, `Borrow<str>` removal), C4's structured `LocalKey`, `FileId`
    pre-assignment on the workers, and guards on `class_exists`-style tests
    (the stubs carry no per-version availability).
- Behaviour changes to read in the O2 diff: the Pratt parser's rejection of
  nested unparenthesized ternaries and chained comparisons, `__halt_compiler`
  ending the lex, kind-exact dependency keys, a hyphenated non-keyword word in
  a docblock being no class name, declarations ruled out by the configured PHP
  versions not being indexed, and UTF-16/UTF-8 columns in the language server.
- Two tests of `syntax::span` were wrong and are fixed; expect a few more.

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
