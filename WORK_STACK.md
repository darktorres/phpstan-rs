# Work stack

The one file that says what is paused, in what order, and where to resume.
Work gets paused to fix something that blocks it, and the fix gets its own
plan; this file keeps the chain so none of it is forgotten.

**Rule:** when work is paused for a fix, add the fix to the top of the stack
below (what it is, why it was started, where its plan lives) instead of
starting a new tracking file. When a level is finished, mark it done here
and say which level to resume. Per-item progress stays in the level's own
documents; this file holds only the order and the pointers.

State as of 2026-10-07, worktree based on `39f0b029`.

## Where to resume

1. **Level 8**: building `PROFILE_DRILL_PIWIGO_20G.md` row by row from
   `PROFILE_DRILL_PIWIGO_20G_LEDGER.md`. Item 9 is complete and its 9.V
   tests-only gate passes; profile item 9 before resuming item 10, at 10.1.
   Use the profiling procedure in `CLAUDE.md` and preserve the baseline/current
   commands, binaries, profiles and logs for comparison. Item 9.7's parser
   scratch lists commit into owner-backed typed
   ranges; node IDs are assigned on insertion, and retained arenas are compacted
   while transient parses reuse worker scratch. Its recorder review is complete
   (no AST element IDs are recorded there). For 9.8, attributes already use
   compact `AttributeRange`s (9.8.e DONE); statement and parameter doc fields
   have been removed and their readers use owner doc sites. Literal `instanceof`
   target names now use the qualified-name arena (9.8.a.a.a DONE, unvalidated).
   Ordinary expression names now store owner-arena ranges in the AST and all
   syntax, checker, registry, dependency, project, Composer and LSP readers
   resolve them through the owner (9.8.a.a.b DONE, unvalidated). The expression
   variant review found no other oversized payload (9.8.a.a.c DONE,
   unvalidated). The `elseif` chain now lives in a typed owner range, with its
   parser, fragment, visitor, canonical, checker, registry and LSP readers
   migrated (9.8.a.b.a.a DONE, unvalidated). Switch case lists now use typed
   owner ranges and all readers resolve through `NodeArenas`
   (9.8.a.b.a.b DONE, unvalidated). `try` catches now use typed owner ranges,
   with parser, fragment, visitor, canonical, checker and registry paths
   migrated (9.8.a.b.a.c DONE, unvalidated). The if/switch/try payload group
   is complete. `declare` directives now use owner-backed typed ranges and
   reusable parser scratch (9.8.a.b.b.a DONE, unvalidated). `global` variable
   lists now use owner-backed ranges and reusable parser scratch, with their
   readers migrated (9.8.a.b.b.b DONE, unvalidated). Static-variable lists
   now use the same owner-backed range pattern, including parser scratch and
   reader migrations (9.8.a.b.b.c DONE, unvalidated). Statement review moved
   namespace names to `Name` and class declarations to `ClassDeclRange`, with
   all readers using `NodeArenas` (9.8.a.b.c DONE, unvalidated). Expression
   spans have been removed from both `ExprNode` and `StmtNode`; syntax,
   checker, registry, project and language-server readers resolve spans from
   the owner side tables, including checker-synthetic expression spans
   (9.8.b.a/b DONE, unvalidated). All item 9 implementation rows are now
   complete; the tests-only 9.V gate has passed. Item 10 is paused at 10.1
   while item 9 is profiled.
   The 10–19 decomposition audit is complete: every referenced child row
   resolves, and each item gate separates workspace tests from item-specific
   tests. Follow-up review added focused test selections for uncovered item
   oracles and explicit final-only rows for PHP, platform, server-session,
   cache and dump checks. Those rows remain OPEN pending level 8's ordered work. Item 9.8's
   statement and parameter doc fields are now removed; parser-owned doc sites
   retain normalized text and comment line by node offset, with checker,
   registry, dependency, promoted-property and canonical readers migrated.
   The 9.V `cargo test --workspace` gate passes with output captured in
   `.cargo-test-9V.log`; all 2,127 tests across 30 test executables pass,
   including phpstan-rs and syntax/parser suites, and the generated stub-index
   check is current.
   Rows 9.V.a-9.V.c are DONE. Item 10.1 is DONE: direct flat-stream emission,
   parser cursors, and per-worker token, lexer name-table, and parser scratch
   reuse are implemented. Item 10.2.a is also DONE because its reusable lexer
   table was required to complete worker scratch reuse.
   The binary builds and the workspace tests pass (2,127 tests across 30
   executables). Resume item 10 at 10.2.
   Full §11
   validation remains deferred until item 19.
   Each item's gate runs workspace and item-specific tests only (including their inherent compile).
   Full §11 validation and O3 run once, after item 19. Only
   the rows that optimize are built now; the rest are DEFERRED.
   **Item E of the same ledger** (effect resolution: the 664-vs-581 false
   positives, then the 130 G Ir regression) is in progress on `master2`:
   E1 (correctness) is built up to its validation row E1.V.2, whose Piwigo run
   leaves 30 rows that need the parametric-summary work (EG.15-EG.20, design
   in the plan's Appendix B.3, decision pending); E2 (perf) starts only after
   E1.V.3. It runs before the final validation, V.0.
   **E2 part 2 (perf)** has its plan in `PROFILE_DRILL_PIWIGO_9071_BATCH_PLAN.md`:
   one batch of items 0-14 (0b, 7a-7h, 12a-12f) for every hotspot of the
   9071f48f callgrind profile. Status: in progress; the Baseline step
   is done (off 54.13 G, warm 42.14 G, cold-with-store 56.24 G; `strace` says
   no `memmap2`), items 0, 0b, 1, 2, 3, 4, 5, 7a, 7b, 7c, 7d, 7e, 7f, 7g, 7h, 9, 10, 11, 12a-12f, 13 and 14 are committed, each with open pieces (item 9: E2.12.29-E2.12.30; checked with `cargo check` and its `walk_gates` tests, the rest waits for E2.V). E1.1.5 with EG.17 is committed (checked with the call_effects and duplicates fixtures; the full fixtures run passed but for the three duplicates fixtures the cache round trip exposed, now fixed), E2.V follows, then E2.V, and E1.2.4 with EG.2. Every open piece is a row E2.12.1-E2.12.21 of `PROFILE_DRILL_PIWIGO_20G_LEDGER.md`; an item counts as done only when its rows are closed. 7g is committed (its open piece is E2.12.19). The correctness bugs found on the way (E1.1.6, E1.2.17-19, E1.3.6-7, EG.21, EG.24-EG.28, E2.12.22) are fixed and committed; EG.29-EG.31 (by-reference builtins, the destroy of a by-reference parameter) are done, EG.32-EG.33 and the EG.31 remainder are open precision rows, and EG.23 (corpus check of the new holder paths, with their cost) runs at E2.V. Resume with the E2.12 remainders (E2.12.6, .15, .16, .18-.20), the reopened rows (E1.1.5 with EG.17, E1.2.4 with EG.2, E2.6, E2.10, E2.12.14), then E2.12.1-E2.12.2 (D and the builtin bit) and item 7h. Verification runs once, after item 14.
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
from both: items 0-19, each ending with a tests-only gate (§11), with full
validation and O3 once after item 19. The plan optimizes: fewer instructions, less wall time and less
memory. What it gathered beyond that, new checks and changes to what the
analysis finds that the optimization does not need, is deferred until after
O3 (2026-10-05; the plan's §9, "Only what optimizes").

- **State: items 4.6-4.8, 5.1-5.9, 6.1/6.6, 7.1-7.5, and 8.1-8.4 implementation complete; tests-only gates 4.V through 8.V passed. Item 9 is active; 9.1 (`3f9ad5b8`) is done.** Item 8 (`451f0fa6`, follow-up `2d383a89`) removed serialized declaration case-reference fields and explicit-`mixed` flags, computes those facts while checking the declaration AST/docblock, preserves inherited explicit-`mixed` semantics in the effective type tree, avoids the unique-parameter dedup map/order allocations, and refines singleton doc types without temporary member vectors. Its workspace gate passed: 2,114 tests, 0 failed. Full §11 validation remains deferred until item 19. Items 0-2 are closed. Item 3's
  implementation is done (3.1-3.2); its workspace tests passed at 3.V. The
  earlier recorded workspace build, oracle corpus runs, and fixture results
  (3,857 passed, 50 existing failures) were additional validation runs, not
  item-gate requirements. The 3.4 scratch-store row remains
  with item 9. Item 9.2 and row 3.4 are implementation-complete; 9.V remains
  pending. Rows 9.3's owner-backed reader migrations are complete by source
  audit, with their tests held for 9.V; implementation is now in 9.4. Syntax parse
  results now own their
  source bytes and the lexer's distinct names in first-use order; retained
  project trees carry both alongside the AST, while cache-only context
  entries and dropped context trees release them. Parser-created `Ident`s
  now carry typed `NameId`s; synthetic/list-parsed identifiers can omit
  them. They still retain their `Name` handle pending reader migration.
  `ParsedFile::ident_name` now resolves parser IDs through the file table
  (`8607a515`). Retained project trees group their AST, source, name table, and
  typed-ID spans under one `SyntaxTree` owner (`21adb973`), with file-scoped
  source/name/span accessors (`c7ceb219`); `parse_recording` now carries that
  owner directly into project state (`8a5c16ce`). The context AST LRU now
  retains an `Arc<SyntaxTree>` instead of only a statement vector, keeping the
  file-local source/name/span owner available for IDs after a context reparse
  (`90acb6ac`, unvalidated). Syntax depends on `la-arena` with typed
  `ExprId`/`StmtId` aliases. `ExprArena`/`StmtArena` aliases are named
  (`6611157d`), and typed aliases now also name the function/method/class/
  closure payload arenas and the argument/array-item/parameter/match-arm/
  attribute list arenas (`6e5a6382`); `NodeArenas` now bundles them and
  provides capacity-preserving clear and shrink-to-fit operations
  (`edb038c1`); an unrun unit test checks expression/statement typed indices
  restart after scratch clearing (`2e0f3e9a`). Parser span recording now writes
  into the bundle, and both `ParsedFile` and the project `SyntaxTree` own it
  (`669bbd3b`); interpolation span vectors merge into the parent's bundle.
  Public `parse_watching_with_scratch` now accepts a reusable arena bundle and
  returns it inside `ParsedFile`; `into_node_arenas` recovers it after use.
  Load workers now pass their `NodeArenas` bundle across files whose trees are
  dropped, reclaiming the arenas from each parsed file. These transient parses
  omit the extra source-byte copy and the file-local name table and `NameId`s;
  retained-tree parses still keep source, names and IDs. Plan §9.2 now states
  this distinction explicitly (`5d5f9d4d`). The capacity and
  span-alignment, parse-error reuse, project tree-to-scratch handoff, and
  transient source/name omission tests are added but unrun. The parser restores scratch arenas on lexer or syntax
  errors, preserving capacity in the worker. A further unrun test verifies
  dense expression/statement IDs and every node span after arena reuse
  (`7c3a4fac`). Rows 9.2 and 3.4 are now implementation-complete: all five
  specified list families and the function/method/class/closure/anonymous-
  class payloads are arena-backed, appended fragments retain file-global IDs,
  and transient parses return warmed capacities after success or failure.
  New unrun tests cover list-range fragment append and retained/transient
  capacity reuse. The 9.V tests-only gate remains pending. The item 9 ledger now breaks multi-part steps into
  separately tracked subtasks, with parent rows summarizing their children.
  The same breakdown is now present for compound implementation tasks in
  items 10-19, derived from the 20G plan before those items begin; deferred
  work and item test gates keep their existing status/order. Follow-up review
  added focused test selections for the token, lexer, line-position,
  declaration, PHPDoc and edit-replay cases, and final-only rows for PHP
  oracles, platform checks, the Latin-1 server session, warm-cache comparison,
  closed-file rechecks and oracle-build assertions. The final §11 validation
  is now tracked separately before O3. The item 18 gate separately tracks its
  fixpoint replay assertion, randomized type differential test and radix
  merge assertion alongside the other test selections.
  The 9.2 parser now commits all five specified list families—arguments,
  array items, parameters, match arms and attributes—to typed arenas, and AST
  fields hold their `IdxRange`s. Interpolation subparsers receive file-global
  bases for all five arenas; `NodeArenas` resolves ranges against local storage
  before append and against the merged owner afterward. The unrun fragment
  test now exercises all five list ranges; the capacity-reuse assertion covers
  the argument arena.
  Function, method, class-definition, closure/arrow and anonymous-class
  payloads now commit to their typed arenas, with file-global IDs and fragment
  append bases. Other child-list fields remain for 9.7. Owner-backed reader
  migration resumes at 9.3 now that 9.2 and 3.4 storage work is complete. The
  first checker consumers now resolve declaration, closure and anonymous-
  class payloads through the file owner in function-like promotion and
  uninitialized-property checks, enum checks, and trait checks. The LSP
  document-symbol reader now carries the tree owner through recursive
  statement traversal. No
  item 9 tests or builds have run. 9.3 now resumes with `syntax::visit`,
  declaration collection, canonical output and the expression printer
  resolving payload IDs and typed ranges through `NodeArenas`; the legacy
  resolver/`VisitorMut`, stub indexing and application readers remain. The IDs target
  `ExprNode`/`StmtNode` (`9763cc81`), and syntax visitor/canonical readers use explicit `ExprNode`
  and `StmtNode` signatures (`a2baf3cb`, `56cedf96`); parser methods now
  return those explicit node values (`36d608ba`), and AST child fields use the
  node names (`e9c7fb2b`). Most large payloads and list fields still use
  `Box`/`Vec` ownership for other child lists; the five specified list
  families and large payload variants are now arena-backed. Existing
  row 9.3 migration remains partial: the shared `NodeId` is
  removed (`fec7cd0a`); the parser reserves distinct typed expression/statement
  indices, and the inference cache is keyed by `ExprId` alone during a
  file's active expression check. Its checker entry point now takes that id
  directly (`e5c79b48`). Project checkers now carry the retained `SyntaxTree`
  owner and assert that their root statement slice belongs to it and that
  statement IDs resolve through its span table (`ab80189f`, `b53aa02e`).
  Context-file checkers now retain an `Arc<SyntaxTree>` owner (`19b30dd3`),
  and debug builds traverse the recursive tree to assert every expression
  and statement ID resolves to that file's recorded span (`dbc8bff4`,
  unvalidated). `AstRef`
  carries the owner explicitly and no longer dereferences to a statement slice
  (`abf88e87`); callers now request statements explicitly (`c568d7b6`). Expression
  and statement readers still use recursive values. `NodeArenas` now
  centralizes typed node and span lookup; `ParsedFile` and project
  `SyntaxTree` expose those file-local methods to parser and checker readers.
  `NodeArenas::alloc_expr/alloc_stmt` commit a node and its span at one dense
  index, enforcing ID/span alignment in release builds too. Until parser
  nodes commit into those arenas, the existing counters reserve IDs and are
  checked against the parallel span tables. Parsed node payload arenas are
  still empty.
  Commit `36ad3c08` removes the address key
  and adds cache coverage for ID, question, and scope; completion's mixed-tree
  probe bypasses it (`a6960c97`). Parser errors are boxed and their canonical
  print is preserved (`2e33f369`); an unrun regression test asserts their
  pointer-sized representation (`88e56ded`). Expression and statement spans
  now travel
  in typed-ID-indexed tables through retained
  project trees (`a42f63ab`); parser tests cover name-ID reuse, parsed-file
  name/source lookup and span alignment (`eee0c503`, `d0a45686`, `a42f63ab`),
  plus `Send + Sync`/`Copy` type invariants (`113b7869`); none have run. The
  workspace call sites now use `ExprNode`/`StmtNode` directly (`1de1a21a`);
  the unused public `Expr`/`Stmt` aliases were removed (`11e69fee`). Current
  uncommitted item 9 work changes AST child links to typed IDs, allocates
  parser-created expression and statement payloads in `NodeArenas`, returns an
  owning `ParsedProgram`, and begins migrating parser readers. The visitor,
  canonical/name-resolution readers, registry entry points, and
  checker consumers still need conversion to resolve IDs through their file
  owner. The declaration collector, anonymous-class registry scan, registry
  guard evaluator, project file-declaration path, Composer hygiene fact and
  reference scans, architecture construction and class-shape scans,
  mock-hygiene scans, completion's declared-name scan, and unset-property
  scan now pass the owner and typed IDs. The in-progress conversion also
  moves `InitFlow`, `FileBodies`, and `GlobalFacts` to owner-backed reads;
  `Registry::build`/`index_and_flatten` now accept the owner and typed
  statement IDs. `BodySummary` now accepts owners and IDs, uses the arena
  visitor, and keys closure/access side tables by `ExprId`. Its place,
  callable, assignment, and shared-scope readers now resolve through the
  owner; closure/function/property-hook callers now require the retained
  owner. The purity-point walk and risky-test assertion scan and their checker
  callers do the same. Magic-constant context facts and strict-rule
  assignment-site scans now traverse owner-backed node IDs. Include
  initializer facts now retain `ExprId`s and the
  include-path prepasses resolve through the same owner. Row 9.6 now stores
  IDs in the disjunction stack, initializer table, condition/query-builder
  alias maps, and both ORM/DBAL chain cursors; it remains unvalidated pending
  the item gate. The constructor/closure variable-usage scan now also walks
  expression and statement IDs through the checker’s retained arena owner.
  The whole-file closure-rebind prepass now does the same. These reader
  changes remain unvalidated until item 9's tests-only gate. Class/function
  use-site indexing now also traverses IDs through the syntax-tree owner,
  including the architecture and Composer-hygiene readers. The class-shape
  scan also resolves statements and expressions through the owner. Mock
  hygiene now gets statement/expression IDs and class bodies from the same
  owner-backed traversal. The declaration dependency collector now also
  traverses statements and expressions through the file owner; its test
  helpers use parsed arena IDs. Statement keyword and `declare` checks now
  resolve their statements and expressions through the checker’s owner too.
  The PHP expression printer now takes a `NodeArenas` owner and `ExprId`,
  and its checker call sites pass those IDs rather than recursive nodes.
  Class reflection construction and virtual-property hook readers now pass
  the arena owner through to their ID-based hook scans.
  The banned-construct/type project pass also walks root `StmtId`s and
  recursive expression IDs through that owner.
  PHP-version syntax gating now also walks arena IDs; its initializer,
  receiver, and nested-`new` checks resolve expressions through the owner.
  By-reference argument checks traverse access chains by `ExprId` as well.
  File whitespace checks now inspect root and section-ending `StmtId`s via
  the owner, removing another top-level recursive-AST reader.
  Use-function/const/class import checks and same-file name-collision checks
  now resolve top-level statement IDs through the owner too.
  Pest's file-local `$this`/hook-property facts and parsed `Pest.php` facts
  now follow statement and closure-expression IDs through the owner.
  `Checker::check_program` now accepts root `StmtId`s, resolves statements
  through the owner, and runs its debug span walk over arena IDs. The
  PHPStan API pass uses an arena-aware namespace/import context walk too.
  Function-like declaration checks now walk IDs for promotions and
  by-reference return legality.
  LSP navigation now follows arena IDs and keeps top-level probe scopes as
  `StmtId`s for the checker. LSP signature help, catch-hygiene's nested
  throw scan, the closure syntactic-return collector, premature-property
  analysis, parent-constructor scan, uninitialized-property scans, loop
  yield detection, private-member usage, gate template-scope/runtime-call
  readers, edit-replay span readers, nested-declaration scans, and goto-region
  discovery now traverse the owning tree's IDs. The member-reference defaults
  collector also reads declaration/default expression IDs and nested bodies
  through that same file owner.
  NoopRule's side-effect search now visits expression IDs through the arena;
  without a retained owner it conservatively skips this report.
  Narrowing's condition-variable and assigned-value dependency probes now
  use owner-backed expression IDs and decline to narrow without an owner.
  The KeyTable structural-versus-rendered test also walks arena IDs and
  resolves child nodes through the owner. Binary-precedence tests now group
  expression trees and assert spans through the parsed file's arena. Parser
  span, name-ID, and native type-hint tests now resolve declarations and
  expression children through parsed-file IDs. The shared parser-test helper
  now returns its arena-owning `ParsedProgram`; namespace/name test readers
  resolve default and call expressions through its node IDs.
  Statement/control-flow tests now resolve nested statement and expression
  edges through the same test owner. Expression/operator tests now inspect
  precedence, assignment, closure, yield, and dynamic-access shapes by ID.
  Declaration tests resolve enum values, anonymous-class expressions, and
  attribute arguments through the owner; string/interpolation tests resolve
  nested expressions, statements, and spans through arena IDs. The name-
  resolution tests now traverse parsed programs and query namespace contexts
  through the owner-backed APIs. The arena visitor now exposes `any_child_*`
  and `for_each_child_*` expression/statement APIs, including function-scope
  searches; visitor tests exercise those walks with owner-backed IDs.
  the obsolete mutable-walk test is removed for row 9.5.
  Item gates are explicitly `cargo test --workspace` plus item-specific test
  selections only; the test command's inherent compilation is allowed, and
  the complete §11 validation remains after item 19. PHP-version readings,
  server-session cases, warm-cache comparisons, and cross-platform runs are
  explicitly final-validation work, not item-gate work.
  Item 9 statement dispatch now resolves `for`/`foreach`/`while`/`do-while`,
  switch cases, try/catch/finally bodies, and nested function payloads through
  the retained owner. Switch case expressions and for-loop conditions now
  follow typed IDs; the matching helpers take statement/expression IDs. No
  item tests or builds have run. Remaining item 9 reader rows are still in
  progress; 9.V remains closed until their implementation is complete.
  Narrowing now resolves `isset` container chains, null-coalesce predicate
  targets, assignment-valued comparisons, and spaceship comparison operands
  by `ExprId` through `NodeArenas`. The ledger tracks these under 9.3.e.d;
  validation remains deferred to 9.V.
  Preg flag chains, PSR-3 interpolation, closure-context inference, postfix
  nullsafe-chain walks, and unused-result wrapper reads now also follow IDs
  through the owner. No item gate has run.
  Duplicate-operand checks, discarded-exception detection, and castable-array
  diagnostics also resolve grouping wrappers through the owning arena.
  Implicit `$this` effect checks now resolve wrapper IDs through the
  checker’s file owner as well.
  Ledger items 10–19 now follow the same parent/child tracking as item 9:
  compound implementation tasks and each tests-only gate have explicit
  independently verifiable rows, including separate selections for the
  item-specific tests. This pass split the remaining compound work for
  heredoc diagnostics, published import scopes, PHPDoc memos, member-use
  cases and deferred private-property writes. The final O3 profiling task is split
  into its build/runtime measurements, heap measurements, and server heap.
  The stub indexing path, remaining registry/test builders, and other
  checker consumers still need migration. The
  transient name-ID check uses the arena visitor too.
  No tests or
  builds have run for this conversion; the transient parse's missing-`NameId`
  assertion now reads the expression arena directly instead of using the
  recursive visitor. Composer's generated autoloader reader now retains its
  `ParsedFile` and reads statement/expression IDs through the file's arenas.
  This continuation also migrated global-constant scope facts, include
  initializer collection, global-variable facts, member-reference defaults,
  and checker import/declare/version-gate scans to resolve declaration and
  closure payload IDs through `NodeArenas`. Additional migrations cover
  `InitFlow`, call/effect summaries, constructor and closure variable-usage
  facts, PHPStan-API class visits, mock-hygiene class scans, generic-bound and
  access-attribute checks, risky-test method checks, and project declaration
  indexing to owner-backed payload and range reads. Class-shape collection now
  resolves class definitions and method/attribute ranges from `NodeArenas`.
  Function and parameter reflection, class/member/parameter attributes,
  promoted-constructor reads, duplicate-property checks, PHPDoc/native
  signature checks, purity checks, and `array_filter` callback inspection also
  resolve through the owning arenas. Body-summary collection and nested
  class/method declaration checks use owner-backed ranges as well. Closure
  context rebinding and parameter-override checks resolve payloads and params
  through the owner. Anonymous-class checking and type inference now resolve
  anonymous-class, closure and arrow-function payload IDs through their file
  owner; callback return inference and spread-argument effect invalidation
  read argument, parameter, expression and statement IDs through that owner.
  The block checker now resolves statement IDs from the owner as it checks
  them, retaining its goto-region convergence and unreachable-code reporting
  without rebuilding statement nodes.
  Closure and arrow-function checking now resolves payloads, parameter and
  attribute ranges, closure body statements, and arrow body expressions
  through the owner. The anonymous-class declaration index also resolves the
  class and definition IDs through its arena visitor. Anonymous-class naming
  now collects IDs in source order with that visitor and writes each name
  through `anonymous_class_mut`; its test reader now uses the parsed owner.
  Ledger row 9.3.d is closed: name resolution and stub transformation moved
  to 9.5.a/e, while stub indexing is tracked with the owner-backed registry
  consumers. Row 9.3.e is split into registry products, declaration
  reflection, recursive checker paths, checker/project facts, and the stub
  indexing work deferred to 9.5.a/e.
  Edit-replay, gate-sample, parser-test, risky-test and Pest
  closure readers were moved to their parsed-file owner too. No tests or builds
  have run; item 9.3.e remains partial. Ledger row 9.3.d is now split into
  canonical printers (done), synthetic stub-name resolution (folded into
  9.5.a/e), anonymous-class naming/declaration readers (done), and the
  immutable visitor removal (folded into 9.5.a/e). Stub indexing is tracked
  under 9.3.e and the parse-time transformation under 9.5.a/e.
  `cargo fmt --all` was run after these edits. Item 9 remains incomplete, so
  item 10 has not started; the 9.V tests-only gate remains pending.
  Owner-backed consumer work continued in 9.3.e/f: LSP navigation and
  signature help now resolve call ranges and anonymous-class payloads through
  the file owner; Composer's autoloader reader and its `__DIR__` parser test
  use `ParsedFile`; Pest's call-chain readers resolve argument ranges through
  `NodeArenas`. The ledger now tracks these 9.3.f deliverables separately.
  `cargo fmt --all` ran. No tests or builds were run; 9.3.e/f remain partial.
  Match checking and inference now read arm ranges, conditions, bodies,
  defaults and subjects through the file's `NodeArenas`; the strict enum
  default and void-match readers do the same. Magic-number checks now resolve
  expression, argument, parameter-default, property-default, match and switch
  IDs from that owner as well. Function, method and constructor call checking
  now resolves callees, receivers and argument IDs through the owner, including
  contextual closure arguments and XML literal checks. `cargo fmt --all` ran;
  no tests or builds ran.
  `check_expr_impl` now also resolves `instanceof`, ternary, index,
  dynamic-variable, property, dynamic-property and interpolation operands
  through the retained file owner. Array-literal checking and inference now
  resolve each item/key ID and its typed range through `NodeArenas`, including
  duplicate-key and spread inference helpers. Assignment and compound-
  assignment operands now resolve through the same owner, and the container-
  field alias probe uses `NodeArenas::peeled`. Formatting ran; tests and builds
  remain deferred until the item 9 gate.
  Destructuring's offset checks and implicit `ArrayAccess` walks now resolve
  nested target and key IDs through the owner, including foreach destructures.
  Nullsafe-assignment and assignability checks, plus the recursive
  assignment-target read walk, now resolve peeled, receiver, index and
  destructuring IDs through that owner as well.
  Assignment binding, nested array-shape writes, unset-index updates,
  by-reference parameter propagation, and statement `@var` destructuring
  checks now carry the owner through recursive target/key reads.
  Inference now resolves wrapped expressions, first-class-callable targets,
  cast and unary operands, binary operands, and assignment values through the
  same owner. The impossible-instanceof and outcome checks now also resolve
  dynamic target IDs through the owner.
  The shared call-argument resolver now turns named and positional argument
  IDs into owner-backed nodes for function, method, constructor, builtin, and
  PHPUnit assertion inference/narrowing paths. Builtin return-type cases now
  and impossible-type predicate checks resolve their direct argument
  expressions through the same owner. Filter/preg/JSON flag scans, compact's
  nested names, `sscanf` format reads, and false-literal peeling now resolve
  argument and array-item IDs through the owner as well. By-reference setup,
  dynamic-callable checks, boolean/swap heuristics, inlay parameter hints,
  `preg_quote` traversal, and `call_user_func(_array)` checks now resolve call
  arguments and recursive operands through that owner. Parameter unpacking,
  binding, type checks and by-reference validation also resolve through the
  owner.
  Strict-comparison mutable-property and loose/numeric-comparison property
  probes now peel wrappers and follow expression and argument IDs through
  `NodeArenas`. Quiet-root and always-exists probe walks now resolve index,
  property and dynamic-property children through that owner too.
  The impossible-haystack and opt-in loose-haystack comparison rules now
  resolve needle, haystack and strictness argument IDs through the owner.
  Useless-return-value and array-filter/array-values call checks, plus enum
  `from`/`tryFrom` narrowing, now resolve their argument IDs through it too.
  `exit`, static and dynamic method calls, and dynamic throw-type helpers now
  resolve argument IDs through the retained owner as well. The same applies
  to current-time and ini-directive checks, date constructor validation,
  class-argument return inference, `define` values, format-string and strict
  call checks, random/key-conversion rules, and attribute constructor args.
  Global and class/trait constant value inference and function/class/member
  existence guard narrowing now resolve IDs through `NodeArenas` too.
  SQL text folding, DBAL query-builder argument chains, and connection
  parameter validation now resolve expression and list IDs through the owner.
  `in_array`'s literal-target narrowing and `count()` comparison and loop
  readers now resolve their array and argument IDs through that owner too.
  Strict null-coercion probes and identity-versus-equals checks now resolve
  their expression IDs through the owner; enum match, switch and if-chain
  readers do the same for subjects, conditions, cases and branches.
  Strict-rule expression and statement readers now resolve recursive
  operands, assignment targets, loop targets and switch case IDs through the
  owner.
  ArrayAccess read/write/probe chains, property writes and property-by-
  reference checks, and current-time expression readers now resolve child IDs
  through the retained tree owner.
  Nested declaration discovery now retains `StmtId`s and resolves each node
  from the owner instead of cloning statement values. Formatting ran; tests
  and builds remain deferred, and item 9.3.e.c is still partial.
  Literal-string folding now takes `(NodeArenas, ExprId)`; builtin inference,
  call resolution, narrowing, PHPStan API, PHPDoc/native-type helpers, Doctrine
  and PHPUnit resolve literal text through the owning file. Formatting ran;
  no tests or builds have run, and 9.3.e.c remains partial.
  PSR-3 and preg-pattern argument checks, preg-call shape extraction,
  condition-alias and predicate-chain narrowing, and include path/guard
  readers now resolve IDs through their retained file owner as well.
  Offset-assignment base/key inference and static-target receiver typing,
  call checks, and diagnostic ranges now resolve expression IDs and spans
  through the retained owner.
  Closure/arrow parameter-default checks and property-default/property-width
  readers now resolve default expression IDs through the file arenas; the
  property-type site recorder receives its arena owner explicitly.
  The shared implicit-nullability helper and closure native-parameter reader
  now resolve default IDs through that owner as well. If-body condition-alias
  detection now resolves its single statement and assignment operands from
  `NodeArenas` rather than treating IDs as nodes.
  Parameter reflection now resolves default IDs through the same owner when
  inferring constant default types.
  The audit caught signature help counting arguments through `Arg.value.span`
  after `value` became an `ExprId`; it now resolves each argument span through
  `NodeArenas::expr_span`. Formatting ran; tests/builds remain deferred until
  item 9's implementation rows are complete.
  A static audit found recursive checker, registry and project traversals on
  the owner-aware arena visitor; item 9.3.e.a-d are now recorded complete,
  unvalidated. The remaining 9.3.f.c work is completion and parser/tooling
  builder ownership; 9.V remains closed.
  Bare-completion import-context and insertion-point readers, including their
  unit-test parser builder, now use `ParsedFile` arenas. The member-completion
  receiver probe needs the synthetic per-file owner support in 9.5.c/d, so
  that work is tracked there; remaining LSP test builders already retain
  `ParsedProgram`. Formatting ran; no tests/builds have run.
  Row 9.3.b's typed child links and owner-bearing parsed-file interfaces are
  complete by source audit; the member-completion probe is the sole reader
  integration assigned to 9.5.c/d. Item 9.3 reader implementation is now
  closed; continue with payload ranges at 9.4. The item gate remains pending.
  `cargo fmt --all` completed; tests/builds remain deferred until item 9's
  implementation rows are complete.
  The five specified list arenas and large-payload arenas are implemented;
  other child-list arenas remain for 9.7, source-backed payloads for 9.4,
  and statement doc-field removal for 9.8. Row 4.1 made
  the four public declaration tables private behind
  `Registry` read accessors (`e06503ad`, preserving owned stub-table
  extraction). Row 4.2 added stub, context-base and project ID ranges
  (`1b8f79aa`); `cargo check -p phpstan-rs --lib --tests` and Clippy passed.
  Row 4.3 is complete: the pinned `la-arena` dependency, typed per-file
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
  Method entries share `Members`' existing `Arc<Lazy<FunctionReflection>>`;
  their serialized layout writes each method once and restores the shared
  handle from the enclosing class. Their parameter ranges are in the file's
  parameter arena too. Each class's methods now also have a case-folded
  file-local slot index, rebuilt when the cached arena is decoded and
  checked against its typed member range. The arena orders methods by their
  recorded declaration positions (with a stable name tie-breaker), not by
  `Members` hash iteration. The focused arena round-trip test checks source
  order and uppercase lookup after decoding. Its test and `cargo clippy -p
  phpstan-rs --all-targets` pass.
  The cache stores class/function declaration metadata beside the arena
  values, then reconstructs `BuiltDecl` with the same `Arc`; mismatched
  metadata is rejected so loading falls back to parsing. The end-to-end cache
  test checks class, function and global-constant reconstruction, pointer
  sharing, mismatch rejection, and method-parameter ranges. The incremental-
  versus-fresh fixture test exposed a missing `PHPStan\Testing\assertType`
  after a finished project switched stub bases. It reproduced at baseline
  `14d2c4b7`: post-freeze stub merges inserted declarations without assigning
  class/function IDs. Those merge paths now assign IDs, covered by a focused
  edit-triggered base-switch test. Project class/function IDs now retain the
  winning file/layout locator when the exact built declaration wins an
  insertion; name-colliding declarations use Arc identity, and merges,
  removals and stub class merges clear the locator. The locator is passed
  through initial loads, base switches, pending edits and loaded-file
  rebuilds. The focused locator and cache tests, base-switch and
  incremental-versus-fresh tests pass. The full library suite passes: 1,137
  tests; Clippy and the library check pass too. During raw as-built access,
  `Registry::class`/`function` now resolve through their IDs and use the file
  locators for unmerged project declarations; resolved and merged entries
  keep the legacy table value. Raw `lookup_method` and
  `lookup_method_with_owner` now use the file's folded member index too,
  with fallback where declarations were merged. Property and constant
  ranges are now typed name slots which share the enclosing class's existing
  reflection Arcs; the cache serializes only names and reconnects them to
  the decoded class. Raw hierarchy lookups for properties and constants use
  these exact-case per-file indexes when available. The focused locator test,
  full library suite (1,137 tests), library check, and Clippy pass.
  Rows 4.3 and 4.4 are complete: the file arena/cache path, locators and raw
  declaration/member/property/constant reads are in place (`a8ff7ef3` closes
  4.3; the previously resolved name tables stay compatibility storage until
  rows 4.6-4.8). Declaration IDs carry a generation, base switches
  increment the registry generation and retag the replacement's IDs, and
  debug reads reject stale IDs (`ac7ff5f9`). The focused stale-ID test and all 1,139
  library tests pass; the library check and Clippy pass. Item validation row
  4.V follows 4.8.
  Item 4.5 is complete (`9ae10e46`, `8c9b69c3`, `9f94a0ec`): each class
  carries a declaration-ordered method list with a folded slot index, and
  `MethodRef` addresses `(ClassId, u16 slot)`. Trait methods, aliases,
  PHPDoc methods and mixin additions enter the list. The PHP 8.5.4 reflection
  oracle confirms own methods precede trait methods and aliases precede their
  source methods. Focused order/lookup tests, all 1,140 library tests, check,
  and Clippy pass. Item 4.6 is active: transitive-purity verdicts now live in
  Registry sets keyed by `FnId` and `MethodRef`, and checker reads use those
  IDs (`b4e4a4fc`). The dependency graph is also stored by `FnId` and
  `MethodRef`, and both purity result fields have left `FunctionReflection`
  (`0949982c`). Changed/removed declarations and base switches clear those
  tables. The focused transitive/clear test and all 1,141 library tests pass;
  library check and Clippy pass. `106045fd` keys the PHPStan-ancestry verdict
  set by `ClassId`, applies scoped changes through IDs, and rebuilds it on
  all-scope passes (`106045fd`). `131bb4ef` preserves the verdicts when
  freezing a fresh base by translating IDs, and rebuilds them from declarations
  when a cache read has no runtime IDs. `fb72f92b` retags the set across base
  generation changes; the base-switch test checks the verdict still resolves.
  The cache round-trip oracle checks class names reached through IDs for both
  stub bases. `17222b57` moves the extends/implements closure into a
  `ClassId`-keyed table; reads in the checker and architecture pass use the
  Registry API, while the disk cache retains its stable name section and
  translates it to IDs when freezing. Closure queries are covered across
  cache reads and base switches. `8e5026ae` moves member-owner tables to
  `ClassId` keys, with the cache retaining its name sections and restoring the
  ID map at freeze. The round-trip oracle checks all owner kinds for every
  class; the generation-switch test checks inherited method ownership still
  resolves. The full 1,141-test library suite, check, and Clippy pass. Other
  pass results and immutable declaration storage remain. `d3c4d0d9` changes
  the ancestor-closure and member-owner builds to collect directly into
  ID-keyed maps and parallel-merges scoped updates. `5b69c001` stores settled
  function and method effects under `FnId` and `MethodRef`, keeping synthetic
  property-hook effects named until they have property slots; canonical output
  and change snapshots translate effects back to names, and base switches
  retag their IDs. Its settling graph still uses `DeclKey`. All 1,199 tests
  from `cargo test -p phpstan-rs`, the library check and Clippy pass. `02ce0ec9`
  also keys the effect pass's `methods_by_name` values by `ClassId`; lookup and
  canonical output resolve IDs through the Registry. The full crate suite,
  check and Clippy pass. `f6ddfc4f` reuses the trait's shared lazy method
  signature for each composed source method and alias, avoiding reflection
  clones while preserving host-local slots; the alias sharing check and all
  1,141 library tests pass. `8efa71a0` translates each classified declaration
  to an `EffectId` once and runs effect settlement, caller edges, queueing and
  dependency reads by ID. `9f560a46` also converts recorded call edges and
  passed-closure targets before settlement. Source discovery still produces
  `DeclKey`, and argument-position lookup converts a callee back at that
  boundary. Closure effects stay in the fixed point before the final table
  drops them. All 1,199 crate tests, check, Clippy and formatting pass. Item 4.6 remains
  open. `da99f22b` keys the parallel trait-composition and inherited-PHPDoc
  result batches by `ClassId`, resolving it only when the existing serial
  apply updates the class. This does not yet separate those results from
  mutable `ClassReflection`. Focused hierarchy and PHPDoc tests, all 1,141
  library tests and all 1,199 crate tests, check, Clippy and formatting pass.
  `7a1558ed` keys inherited property-hook updates and deferred class-type
  resolution outputs by `ClassId` too; their values still apply into mutable
  `ClassReflection`. `15f73523` carries `ClassId` through the deferred
  class-constant fixed-point queue and update records; constant slots and
  writes to the reflection remain name-keyed. ID-keyed inherited-doc values,
  remaining trait outputs, constant-resolution values, other pass results and
  immutable declaration storage remain. `9ccaef59` carries class IDs through
  deferred type-import and mixin result batches; mixin method outputs share the
  source's lazy signature handles. `1f0411bf` keys two more deferred type-fix
  batches by `MethodRef`, `FnId` and `ClassId`; apply resolves the IDs only
  when mutating the current reflection store. The full crate suite (1,199
  tests), check, Clippy and formatting pass. `dec35a80` carries each
  inherited-PHPDoc fill by `MethodRef` inside the class batch. The full crate
  suite, check, Clippy and formatting pass. `3e6a1772` keys deferred class
  constant updates by `ClassId` and constant slot. The full 1,199-test suite,
  check, Clippy and formatting pass. `6cc05daf` applies class constant
  updates by ordered slot through `get_index_mut`, avoiding a linear scan. The
  index-stability test, all 275 syntax tests and all 1,199 crate tests, check,
  Clippy and formatting pass. `6f6f53da` assigns composed methods and
  aliases their host `MethodRef` during collection, then verifies the slot on
  insertion. Reflection-order fixtures and all 1,199 crate tests, check,
  Clippy and formatting pass. `ba4dc50b` keys trait-composed constants by
  `ClassId` and ordered slot; trait property outputs still use names. The full
  suite, check, Clippy and formatting pass. `770b7370` assigns mixin methods
  and aliases host `MethodRef`s, with PHP-folded duplicate handling and slot
  checks; the full suite, check, Clippy and formatting pass. `42c24a58`
  moves abstract, native-final, static and PHPDoc-final facts onto each
  class-local method entry, carries them through trait aliases and mixins, and
  gives synthesized enum built-ins slots. All 1,142 crate tests, 275 syntax
  tests, check, Clippy and formatting pass. `0b82e5f6` puts native-final and
  PHPDoc-final property facts on each `PropertyReflection`, carries them through
  trait composition, and advances the stub-container version to 8; the focused
  inherited-trait-property test and all 1,143 crate tests, 275 syntax tests,
  check, Clippy and formatting pass. `a2ce1d81` adds `OrderedNameMap::get_index`;
  `0ddfa7ed` adds `lookup_const_with_owner_and_slot`, preserving existing APIs
  while returning the declaring class and stable slot. `58930b47` adds immutable
  slot access and uses it after the name-to-slot lookup; `e4bcd895` preserves
  the direct path for old callers. Direct/inherited slot tests, the index test,
  library check, Clippy, formatting and diff checks pass. `c4a849df` adds a
  `(ClassId, slot) -> Type` result table, populates it from deferred class
  constant/value-of resolution, and routes type-sensitive checker, global
  variable and LSP readers through Registry. `a255d3ea` removes the remaining
  deferred constant and imported-type writes to `ConstantReflection.ty`; the
  result table persists by class name and ordered slot, is restored after freeze,
  and remains correct across scoped edits and base switches. Stub-container
  layout version is 9. The imported-alias regression and cache round-trip pass;
  all 1,145 crate tests and 275 syntax tests, check, Clippy, formatting and diff
  checks pass. `f5d8a1a8` changes the deferred visible-constant scratch view
  from cloned reflections to ordered name-to-type pairs, keeping breadth-first
  shadowing and union order; all 1,145 crate tests, 275 syntax tests, check,
  Clippy, formatting and diff checks pass. `dce105fb` moves inherited
  property-hook outputs into a `(ClassId, property name)` result table used by
  Registry property lookup. Cache round trips preserve it, and scoped edits
  clear affected IDs; the container format advances to version 10. The focused
  declaration-immutability and cache tests, all 1,147 crate tests and 275 syntax
  tests, check, Clippy, formatting and diff checks pass. `5318c9cd` extends the
  class property result table to deferred and imported property type fixes,
  constant-named/native property resolution and hook results; property readers
  use the Registry overlay while declarations retain their built values. The
  cache format advances to version 11. Imported-alias, deferred value-of,
  declaration-immutability and cache round-trip checks pass with all 1,148
  crate tests, all 275 syntax tests, Clippy, formatting and diff checks. Item
  `0cdb6a41` moves trait- and mixin-composed properties to the Registry result
  table and exposes their names to owner indexing, hooks/effects, checker,
  architecture, dead-member and LSP readers. Deferred type passes include
  composed properties; host declarations remain as built. The hierarchy and
  cache round-trip fixtures pass with all 1,148 crate tests, 275 syntax tests,
  check, Clippy, formatting and diff checks. `78613439` stores trait-composed
  constants in an ordered class-ID result table with stable slots; constant
  owner/name/slot lookups and deferred type resolution read the table while
  host declarations remain unchanged. Cache round trips, freeze and base
  switches preserve it; the container format advances to version 12. Trait
  self-reference, subclass lookup and cache checks pass with all 1,148 crate
  tests, 275 syntax tests, check, Clippy, formatting and diff checks. Item 4.6
  remains active. Commit `07b88079` moves composed methods/aliases and PHPDoc
  `@method` signatures into a `ClassId`-keyed result table. Deferred constant
  types, imported aliases, inherited PHPDoc and method type fixes write overlays;
  checker, purity/effects, architecture, dead-member, named-argument and LSP
  readers use Registry lookups. The result survives freeze/cache round trips
  (container format 13). The full crate suite (1,149 library, 3 binary and 44
  CLI tests), 275 syntax tests, check, Clippy, formatting and diff checks pass.
  `22518763` adds `FnId`-keyed function-signature result overlays for the late
  native-type and constant-named type fixes, routes declaration checking and
  effects through effective signatures, and moves assertion-table results to
  function/method overlays without mutating their declarations. The cache
  format advances to 14; freeze, base retagging, cache round trips and scoped
  invalidation preserve the results. Incremental snapshots compare old
  function overlays, and the assertion fast-gate indexes effective signatures.
  The focused cache, incremental and fast-gate tests and all 1,153 library
  tests pass; check, formatting and diff checks pass. The 4.6 implementation is complete in commit `d28d22a1` plus the current Registry storage diff. Runtime results now use `FnId`, `MethodRef`, `PropertyRef`, `ClassId`, or `(ClassId, slot)`, including composed constants, effect classifications, and project property-write facts. Active declarations are held in ID-keyed Arc stores; name tables are limited to insertion/merge and cache projection, and production reads use ID-backed lookup/iteration APIs. Project declarations receive their own IDs when shadowing stub IDs; removal reveals the shared base. `freeze()` reconstructs name-keyed views only for the frozen base. The public whole-table `classes()`/`functions()` snapshot accessors were removed; readers use ID-backed lookups and iteration. Stub indexing now uses those ID-backed iterators instead of the empty builder tables. The old raw class reset/swap path is gone. `EffectId::Named(DeclKey)` remains only for targets with no active declaration ID. Item 4.7 maps class-reference resolution by `ClassId`, include resolution by `FileId`, and purity/effect worklists by parallel frontier waves; newly discovered passed-closure edges trigger a follow-up wave. Item 4.8 uses segmented ID bitsets for active scope membership and ID-keyed class, function, and effect snapshots with parallel change comparison. The test portion of 4.V passed: 2,095 workspace tests via `cargo test --workspace --lib --bins --tests -- --quiet`; full §11 builds, runtime oracle and corpus passes remain open. `cargo fmt --all` and `git diff --check` pass. Item 5 reserves per-file class/function ID ranges by prefix sum, retains holes for losing declarations, and replays base switches in load order (`7c1c5fba`). Rows 5.2-5.9 are implemented; the tests-only 5.V gate passed with 2,111 tests. Active work is item 6. Per user direction, item 6 cache rows 6.5 and 6.7-6.9 moved to item 5: the ID-backed registry, stub cache, and per-file cache now share the new container with indexed names, positional record reads, per-record checksums, and a source-complete cache key. There is no old-layout compatibility path. Rows 5.2-5.9 are implemented and passed the tests-only 5.V gate; full §11 validation is deferred until item 19 is complete. Row 5.3 reduces class/function declarations in parallel by folded name while preserving per-name load order and the established conditional/merge rules; reference, use/duplicate-site, global-constant and PHPStan class-file contributions are sharded by their own keys, and per-file/stale state is keyed by FileId. Disjoint reduced declaration results are applied serially after the parallel phase. Row 5.5 now stores project declaration origins as FileIds, removes the FileTable path-address map, and rebinds per-file cache origin markers to each run's current FileId, including nested lazy methods. The plan was committed at
  `d520c9fd`. Item 6 follows its designed rows in `PROFILE_DRILL_PIWIGO_20G.md`: deferred class/function types resolve on first read; stubs are normalized once while building the shared base; cache writes materialize lazy results on a clone; the debug-only closure diagnostic lists scoped stub keys by changed name. Its tests-only 6.V gate passed: 2,109 workspace tests, 0 failed. Item 7 replaced both relation memos with a generation-keyed, two-segment thread-local memo, disabled for provisional evaluations and controlled at runtime by `PHPSTAN_RS_NO_RELATION_MEMO`. Constant type cycles read as `never`; existing alias-cycle resolution yields `mixed`. Both memo-on and memo-off tests-only gates passed with 2,113 tests, 0 failures. Full §11 validation remains deferred until item 19. Item 8 is complete; item 9 is next. A separate reviewer goes through the plan adversarially, one item at
  a time, in order; each finding is checked against the code, `php` 8.5 and
  the heap profile before it goes in, and §13's review table has a row per
  item. Items 0-19 and §11-§13 were reviewed and committed one by one.
  Item 9.4 now has a reusable file-owned decoded-byte vector in `NodeArenas`,
  with typed ranges, fragment append bases, and clear/shrink handling; retained
  `ParsedFile` exposes the range reader, and the checker can now read source
  slices through its owning `SyntaxTree`. Inline HTML now travels as its source
  span through lexer and AST; whitespace checks read the bytes from that owner,
  and an unrun lexer test covers non-UTF-8 HTML bytes. The demo supplies its
  source, while checker test helpers retain the project tree. The lexer keeps
  each doc span through grouping; declaration and statement AST doc fields now
  store source ranges. The lexer-backed parser resolves those ranges for
  reference recording, and doc readers have been migrated. The stub-rule tree
  mutation remains tracked in 9.5.a. Single-quoted unescaped literals now use
  source ranges with owner-backed syntax printing/canonical reads; transient
  parses put unescaped literal bytes in the decoded arena, while detached
  token-list calls materialize values pending item 10. Escaped, interpolated,
  heredoc and nowdoc payloads now use decoded ranges; Unicode output writes
  directly into the lexer byte store, and fragment ranges append after the
  parent range. Checker, registry, project and Composer literal readers now
  resolve through owners; the LSP audit found no literal-payload readers.
  Range, escape, Unicode and fragment-append regression coverage is added but
  unrun. Item 9.7 has now moved every statement-body list to `StmtIdRange`,
  including function bodies; completion appends probe IDs by committing a new
  range in its owned parsed file. Root parsed-program, parsed-file and project
  tree lists now use owner-backed `StmtIdRange`s. Expression child lists and
  destructuring elements use typed ranges; expression IDs, destructuring
  elements, arguments, array items and match arms now build in reusable parser
  scratch before committing. Class-body properties, methods, constants, trait
  uses and enum cases also build in reusable scratch before their owner ranges
  are committed. Parameter lists use reusable buffers by nesting depth before
  stub transformation and arena commit. Top-level constant declarations also
  reuse class-constant scratch and commit to the owner arena. Class inheritance,
  property,
  constant, trait-use/adaptation and enum-case lists now use owner-backed
  ranges, with parser, checker, registry, project, Composer, LSP and parser
  fixture readers migrated (9.7.b.a-c, unvalidated). Catch-type name lists
  now use the same qualified-name arena, and `use` imports commit to their
  owner-backed import arena with parser, checker, project, registry, Composer,
  dependency and parser-test readers migrated (9.7.b.d.a/c, unvalidated). Name
  lists for class inheritance, catch types, trait names and precedence clauses,
  plus grouped imports and trait adaptations, now build in reusable scratch
  before arena commit. Union and intersection type hints now build in scratch,
  commit as `TypeHintRange`s, and resolve through owner arenas across type,
  checker, registry, project, LSP, printer and parser-test readers; this part of
  9.7 is unvalidated. Property hooks need no list migration: `get` and `set`
  are fixed optional fields, attributes already use ranges, and block bodies
  already use statement ranges.
  Closure captures now use an owner-backed range resolved by syntax, checker
  and printer readers (9.7.a.d.c.b/9.7.b.e.a, unvalidated). Node IDs are
  assigned at arena insertion, syntax recorders capture names/spans rather
  than node IDs, retained arenas are compacted, and transient arenas are
  reused. Item 9.7 is implemented but unvalidated; item 9.8 remains. Items
  10-19 have complete child-task and tests-only gate rows. A follow-up
  granularity pass added the PHPDoc cache migration (14.9.d), the deferred
  document-encoding test (15.9.e), separate hash inputs (17.2.a-d), and the
  remaining tree-reader sweep (19.6.b.a-d); item 19's cache-validation
  subtasks now follow execution order.
  `cargo fmt --all` ran; no tests or builds were run yet. Run only workspace and
  item-specific tests at 9.V; full §11 validation stays deferred until after
  item 19.
- **Passes outside the item order,** also in §13: a dependency pass (six
  crates taken: `la-arena`, `wide`, `triomphe`, `quick_cache`, `siphasher`
  and `blake3`; ten considered and not taken; item 15's review added
  `encoding_rs`).
- **The projection,** §12, estimated: ≈290 MB at exit (from 657), a
  ≈350-385 MB peak (from 866) plus the checks in flight, ≈1.85 M allocations
  (from 19.4 M), and roughly 8 G of the 20.57 G instructions.
- **Building it** started on 2026-10-03. Its tasks, in order, with their
  state and commits, are in `PROFILE_DRILL_PIWIGO_20G_LEDGER.md` (items 0-19, each closed by its
  validation row, then O3). Compound implementation tasks and item-gate test
  selections are split into child rows throughout items 9-19; the latest
  audit covers implementation tasks, item gates and final §11 validation.
  Reading the
  starting state (0.1) found four false-report bugs, fixed in item 0
  before the baseline (rows 0.1a-0.1f), and added the implicit calls, now
  part of item 0. Item 3's latest implementation is row 3.2 at `dfa2cebe`;
  item 4.1 is committed at `e06503ad`, 4.2 at `1b8f79aa`, and item 4.3's
  active arena increments are recorded in `PROFILE_DRILL_PIWIGO_20G_LEDGER.md`
  through `e22c002f`.
- **Item 0's baseline attempt (0.11)** stopped at the runtime oracle after
  its build and tests; the edit replay over Piwigo and every vendor project
  found the stale diagnostics and registry state the plan's "Found
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
  rest skipped) and `cargo xtask validate` (0.11u, done). That earlier
  validation attempt ran a build and tests before stopping at the runtime
  oracle; no item-0 snapshot was recorded. Under the current gate policy,
  item 0's validation is not resumed between items: each V row runs tests
  only, and complete §11 validation runs once after item 19. The missing
  baseline means dump comparisons remain unavailable.
  Item 1 (rows 1.2-1.8) is done and its validation (1.V) skipped by the
  user, with no run and no snapshot. Item 2 (rows 2.1-2.7) is built; its
  validation (2.V) is the tests. The plan's validation (§11) compares no
  dumps at item gates: no snapshot, no baseline, and no comparisons between
  runs. Item gates run the workspace and item-specific tests only; runtime
  oracles, fixtures, and other full-validation work wait until the end.
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
