# Work stack

The one file that says what is paused, in what order, and where to resume.
Work gets paused to fix something that blocks it, and the fix gets its own
plan; this file keeps the chain so none of it is forgotten.

**Rule:** when work is paused for a fix, add the fix to the top of the stack
below (what it is, why it was started, where its plan lives) instead of
starting a new tracking file. When a task is finished, delete it from this
file; the level's own documents keep the record. Per-item progress stays in
the level's own documents; this file holds only the order, the tasks still
open and the pointers.

State as of 2026-10-17, `df804239` on branch `callgrind-20g`. Counts below were
read from the files on that date. This file covers this branch only.

## Where to resume

Levels 1 to 7 are deferred, decided on 2026-10-16, and will be reevaluated
together later. Their sections below keep the content they had that day. The one
exception is level 7's designs, which are rows of the 20 G plan and are worked in
level 8's order; DOC-2.1 stays deferred.

Level 8 is the one active level. It holds the 20 G plan and the off drill, and
its order is in its section below. Work the lowest item that has a READY row
(item 3 on 2026-10-17).

## The stack

Levels 1 to 7 are deferred; see Where to resume.

### 1. The roadmap, in two lanes — `ROADMAP.md`

Paused on 2026-09-23 to split the codebase into crates. `ROADMAP.md` lines
1783–1785 assign Lane A to Phases 87, 91, 94 and Part 11, and Lane B to Part 10,
Phase 89 and Phase 97 (AA). Phases 186, 187 and 188 are PLANNED, and no lane is
named for them; the 2026-09-23 report predates them.

Remaining from the report's list (not re-checked item by item, except where
noted):

- Lane A, Phase 130: the PHPDoc-versus-native check for properties and hooks;
  `*.unresolvableType` (9 references in code); template checks on callable
  shapes; `@phpstan-self-out`; assert and `@var`-family checks; conditional-return
  declarations. Phase 130 has no heading on this branch.
- Lane A, Phase 131 (templates on `@method`, the rest of `@use<...>`) and Phase
  134 (`nullCoalesce`/`isset`/`empty`, `compact()`, `unset()`).
- Lane A, items 173, 140d (a sub-item of Phase 140, the "Known but not yet
  scheduled" bucket), A4, the A6 remainder ("mostly shipped"), O1 ("narrowed,
  partly shipped"), Phase 180 (`treatPhpDocTypesAsCertain: false`, 0 references
  in code), and the narrowing limit with two correlated conditions.
- U1's beyond-parity half (`ROADMAP.md` line 2110, "the parity half is built").
  The report did not list it.
- Lane B: the two named items of Phase 139 are dropped. `InvalidTypesInUnionRule`
  and `ignore.parseError` are in the code, and the rule catalog describes both as
  built (`ROADMAP.md` lines 665 and 1253). Its receiver half is built too (line 289).
- Lane B: Composer phases 113–120, baseline Phase 122 (N1, N2, N4), output
  formats (Phase 154) and codemods (Phase 111, I1–I4). This branch's roadmap has
  no heading or item for any of them, so their text is not in this checkout.
- Phases 87, 89, 91, 94, 97 (AA), Parts 10 and 11 have headings but no
  `**Status**` marker in their sections. The lane lists above assign them.

### 2. The crate split — `CRATE_SPLIT_PLAN.md` and its companions

Remaining after C1–C3 (`gates/module-graph.txt` line 218, the `finished` line):

- **C4**, REG-2 to REG-12, in the ledger's order (`PROFILE_DRILL_PIWIGO_LEDGER.md`
  §2.1 and §2.2). Part-built: REG-3, 4.1, 4.3, 4.4, 4.6, 4.8 and 16.1 are done,
  and REG-2 is ready.
  - ready: REG-2; REG-4.2, 4.5, 4.7, 4.9, 4.10, 4.12, 4.13 and 4.14; REG-5 (a
    delta left); REG-6; REG-16.2 (interned identity);
  - open: REG-4.11 (needs REG-8.1, 8.5 and REG-10) and REG-4.15 (needs 4.2 and
    4.12); REG-7 (its five levels, 7.1 to 7.12; needs REG-16.3); REG-8 (needs
    REG-7); REG-9 and REG-10 (need REG-8); REG-11 (behaviour fixes; 11.14 is
    superseded); REG-12, C4's gate;
  - the engine, decision 3: 16.3 needs 16.2; 16.4 needs 16.3, SYN-1.2 and LOAD-1.4;
    16.5 (the query trace) needs 16.3.
- **C5**: STUB-7 (needs REG-12). Its sub-steps hold dependents: STUB-7.1 for
  STUB-12 and CHECK-10, STUB-7.3 for PROD-5.4, STUB-7.6 for CACHE-9 and CACHE-10.
- **C10**: REG-15 (15.1 first, then 15.2).
- **C1–C3 follow-ups (Q-11)**: LOAD-7.1, LOAD-7.3 to 7.7, REG-18.1 to 18.6 and
  CHECK-11, all ready.
- **Stages D to G** (`CRATE_SPLIT_PLAN.md` lines 2878, 4504, 6578 and 7491): open.
- The seven `[>]` tasks moved out of the reviews (level 3) belong to this plan.
- Companions: `CRATE_SPLIT_AUDIT.md`; `CRATE_SPLIT_EVIDENCE.md` (not kept
  current); `CRATE_SPLIT_CAUSES.md` (regenerated, not edited).

### 3. The reviews of stages A, B and C — `STAGE_*_REVIEW.md`

Its pause reason, the slow test suites (level 4), is finished, so this level can
run. Open tasks, counted as `- [ ]` lines:

| file | open |
|---|---|
| `STAGE_B_REVIEW.md` | 16 |
| `STAGE_B_FIX_REVIEW.md` | 105 |
| `STAGE_C_REVIEW.md` | 15 |

136 in all. Work the open tasks in each file and tick them there. The seven
`[>]` tasks (4 in `STAGE_B_REVIEW.md`, 3 in `STAGE_C_REVIEW.md`) are not counted
here; they are in the crate-split plan (level 2).

### 4. Faster gates — `GATE_SPEED_PLAN.md`

Still open:

- S11's two experiments, stopped at the user's request in favour of the
  profiling work (S13–S15): the dev-profile `opt-level` setting, and the lib tests
  under an optimized profile. Parked.
- §4's deferred costs, which the crate split removes: the per-fixture stub setup
  (C4 and C5); the cli tests' per-process stub parse (C5); `check_paths`
  recomputing property-assign facts (the Summaries API, crate-split §3.8); the
  `--full` run's 22 per-level runs (F1's `reconfigure`); the facts pass running
  the whole checker (Stage D).
- §5's wording for the C4 and C5 gates (`gatetime` wall times lower than at the
  phase's start). It goes into those stages.
- `tools/gate-runner` is deleted when Phase 181 ships `report --export`
  (`ROADMAP.md` lines 4458–4459). The code has only `--export-json`, so the
  runner stays.

### 5. The project build cost — `PROJECT_BUILD_COST_INVESTIGATION.md`

One carry-over, parked. `gates/test-renames/build-cost.txt` (23 renames)
overlaps the master commit `0ad9589b` (row 86 of
`MASTER_REIMPLEMENTATION_LEDGER.md`). If that commit is ever recreated on this
branch, one set of renames is dropped when its row is worked. The module-graph
edges in `gates/module-graph.txt` may also meet that commit's own edits to the
file.

### 6. Dropped on 2026-10-16 — `MASTER_REIMPLEMENTATION_LEDGER.md`

Not tracked here. The file stays in the repository, listed below. The number is
kept because the reports cite level 7.

### 7. The 25 G Piwigo profile — `PROFILE_DRILL_PIWIGO_25G.md`

Validated up to `1892b477`. What was built after that point is validated only by
VAL-1 (below). Remaining designs, each with its ledger row (the mapping is the
ledger's Appendix B.2). Every row below except DOC-2.1 is also a row of the 20 G
plan, so it is worked in level 8's order (section 8). Code counts are references
in `.rs` files on this branch:

| remaining | code now | row | state |
|---|---|---|---|
| §12.10 observers; `statement_method_call` (5) goes | 5 | CHECK-6.1 | READY, approved in principle |
| C1 step 5's tail: `expr_side_effects` (4) and `collect_throws` (2) go | 6 | CHECK-6.2 | READY |
| P7, lowering: `parse_written_type` (58) goes | 58 | DOC-2.1 | DEFERRED with levels 1 to 7 |
| P11 step 1, tag effects | — | DOC-2.3 | READY |
| tag tokens borrowed (the pull-parsing is built, DOC-1) | — | DOC-2.5 | OPEN, needs NAME-1 |
| P12 step 3, declarations as the tree's own vectors | — | PROD-5.1 | OPEN, needs SYN-1.2 |
| `InitFlow` fused into one walk (16) | 16 | PROD-5.3 | READY |
| P12 step 4, `deps::Collector` goes | — | PROD-5.4 | OPEN, needs STUB-7.3 |
| C3 step 6, `String` keys to `Name`; `Borrow<str>` on `ImportKey` (1) | 1 | NAME-1.2 | READY |
| C3 step 7, the checker's method memo | — | NAME-1.1 | READY |
| §14: trees dropped in the serial insert loop (267.8 M serial) | — | LOAD-3 | OPEN |
| P6(d), the streaming lexer's plumbing | — | SYN-2 | OPEN |
| P4, column tables for every file | — | SYN-3 | OPEN |
| F1, the guards on `class_exists`-style tests | — | STUB-8 | OPEN, needs STUB-7 |
| the end validation; the O1 dumps; reading the diffs | — | VAL-1.5, VAL-1.13, VAL-1 | OPEN |

Notes:

- `has_yield`, which CHECK-6.2's text says goes, has no references in the code
  now. Correct the row before it is worked.
- F2's count (STUB-9) is superseded by REG-16.5, so it is not pending.
- DOC-2.1 is deferred. The 25 G report's opening summary says its saving is gone
  (`parse_written_type` at 0.18% inclusive in a sampling profile), but its P7
  section estimates 0.35–0.45 G. The 20 G figure (about 0.2 G) is for
  `5fb858c9`, not HEAD. Before it starts: measure at HEAD, and settle the
  statement `@var` memo, which overlaps DOC-2.2.

### 8. The 20 G plan and the off-run drill — `PROFILE_DRILL_PIWIGO_20G.md`, `PROFILE_DRILL_PIWIGO_OFF.md`, `_OFF_HEAP.md`

Every row and what blocks it is in `PROFILE_DRILL_PIWIGO_LEDGER.md`: §2.1 is the
ready list, §2.2 the chains, §2.3 the biggest targets. The ledger lists rows by
area, which is not the order of work.

**The order.** The 20 G plan builds its items one at a time, in number order (its
§9). Within an item a row starts when it is READY. A row whose need sits in a
later item waits for that need. The READY rows, item by item:

- Item 0: none. Its open rows (VAL-1) are the final validation, after item 19.
- Item 2: none ready. LOAD-2.3 to 2.8 are in: `07595369`, `8590ff55`,
  `c1db8372`, `c7ff651c`, `2cd13c8e`, `47adfd19` and `df804239` (LOAD-2.4). LOAD-2.2's
  walk is in (`85a4ecad`). Its last part, deleting
  `path_under_excluded_dir` and `DEFAULT_SCAN_EXCLUDED_DIR_NAMES`, waits on a
  decision: what replaces the name floor for the editor's degrade root (no
  `paths`), for a file under no configured root, and for a never-seen path.
  Options: (a) keep `.git` and `node_modules` as names, and skip the Composer
  vendor directory by its resolved path, as MAGO does (recommended; a `vendor/`
  outside Composer is then walked); (b) keep all three names until
  `exclude_paths` exists; (c) delete all three now, so an editor with no
  `paths` analyses `vendor/`. Until the decision is made, the walk keeps its
  name list. The Piwigo gate and the measurements of items 2's rows (wall time,
  instructions) have not run; they need approval.
- Item 3: LOAD-3.2 (3.1 waits on REG-16.4, level 2).
- Item 5: LOAD-4.3, 4.4, 4.5 and 4.7 (4.4's need, LOAD-2.5, is in: `8590ff55`).
- Item 8: REG-14.4 and 14.5 (14.1 waits on PROD-5.8, 14.3 on REG-8.3).
- Item 9: SYN-1.1 and 1.2, then 1.3, 1.4, 1.5, 1.7 and 1.8.
- Item 11: SYN-3.1 and 3.2, then 3.3 and 3.4.
- Item 12: SYN-4.2, 4.4, 4.5, 4.6, 4.8, 4.9 and 4.10.
- Item 13: PROD-5.2, 5.3, 5.5, 5.6, and PROD-5.8 (decision 2).
- Item 14: DOC-2.3, 2.4, 2.7 and 2.8. DOC-2.1 is deferred.
- Item 15: NAME-1.1, 1.2, 1.5 and 1.6.
- Item 16: NAME-2.1, 2.2 and 2.3.
- Item 18: CHECK-6.1, 6.2, 6.4 to 6.6, 6.8 to 6.12, and 6.16 to 6.18. CHECK-4.1 is
  built with CHECK-6.10.
- Item 19: LOAD-5.4.

Items 4, 6, 7, 10 and 17 have no READY row in level 8. Item 4's two are C4 rows
(level 2). Items 6 and 7 wait on level 2 and the C5 rows. Item 10 (SYN-2) waits
on SYN-1 (item 9). Item 17 (CACHE-13) waits on LOAD-1.4 and on REG-16.4 (level 2).

Outside the items, with no item number, CACHE-8 and CHECK-4.2 (off drill) and
TYPE-5 and CHECK-9 (decisions 8 and 9) are READY now.

The off-run rows still open:

- **Stub embedding** (proposal 6): STUB-11, OPEN, needs STUB-10.1 (its
  measurements, OPEN) and REG-15. The stub build still embeds `include_str!`
  paths (`crates/stub-sources/build.rs:37`).
- **Cache**:
  - CACHE-8 (heap H1, READY): `Writer::finish`
    (`crates/phpstan-rs/src/registry/container.rs:120`) still builds `directory`
    and a second `file` buffer.
  - CACHE-9 and CACHE-10 are BLOCKED: they need STUB-7.6 and the persistence
    format of CACHE-13.2.
  - CACHE-11 (E8) and CACHE-12 (E11) follow them.
  - CACHE-13 (warm runs) is OPEN; its persistence part needs REG-16.4.
- **Text made when read** (proposal 11): CHECK-4.1, messages formatted when
  displayed, is OPEN and needs CHECK-6.10 (READY). `report_missing_offset`
  (`crates/phpstan-rs/src/checker/assignment.rs:743`) still formats eagerly.
  CHECK-4.2, class-constant keys, is READY.
- **Parser gaps** (from the probes): SYN-6 is OPEN and needs SYN-1.9 (OPEN, needs
  SYN-1.2) and Q-10 (OPEN, partly probed). Q-2 is probed, and its fix is SYN-6.
  Q-1, the corpus run, is done; its three parse-error files join Q-2.
- Settled, not pending: proposal 10 (`simdutf8`, LOAD-6 is SKIP); E4 (CACHE-6),
  E9 (CHECK-5, folded into CHECK-6.16) and E12 (CACHE-7) are SKIP; D3, D6, C7 and
  C8 are decided.

## Documents with no place in the stack

Standalone research and plans; none blocks anything. Still at the repository root:

- `CRASH_REPORTING_PLAN.md`
- `MAGO_ARCHITECTURE_COMPARISON.md`
- `MASTER_REIMPLEMENTATION_LEDGER.md` (level 6, dropped)
- `PIWIGO_CORPUS_TRIAGE_ab4c2b5.md`
- `TOP100_PHPSTAN_EXTENSIONS.md`, `TOP100_RESEARCH_NOTES.md`
- `UNSAFE_PLAN.md`

Proposed: move them under `docs/research/` so the root holds only the stack's
files. Not done; `docs/` does not exist yet.
