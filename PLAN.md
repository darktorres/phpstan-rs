# phpstan-rs plan: a faithful port of PHPStan, then beyond it

## Context

**The problem.** The current repo (`~/projects/phpstan-rs`, read 2026-11-01) lost its thread:

- **Code:** about 240k lines of Rust in 11 crates and about 4.5k tests. It began as a PHPStan port but kept its own AST, type model and checker. Over time it gained:
  - beyond-PHPStan checks;
  - SQL, Doctrine and Laravel support;
  - the PHP catalog (the `php-catalog` branch, still WIP);
  - a crate split that stalled at stage C3 of seven;
  - allocation drills and open-world/overflow research.
- **Docs:** 34 plan, ledger and review files at the root, about 68k lines. `WORK_STACK.md` holds 10 nested levels of paused work. Of the 472 commits since 2026-10-15, 211 touch only docs.
- **Parity:** measured once, by hand. About 40% of nsrt `assertType`s passed against 2.2.16 at `76e738b4`. It never became a standing gate (C-1), so "port PHPStan" never had a finish line.

**What reading every legacy doc established.** Every doc was read in full, on `master` and on `php-catalog`. The findings are in `RESTART_FINDINGS.md` at tag `legacy-final` (`git show legacy-final:RESTART_FINDINGS.md`), cited here by id.

1. **The target architecture already exists on paper.** CRATE_SPLIT_PLAN §3 designed PHPStan's shape. It stalled because it tried to get there in place, byte-identical, under 14 gates (C-2, C-3). The new repo builds that design directly.
2. **A faithful port avoids most of the legacy's bugs.** They came from diverging from PHPStan's architecture: no native-type channel, no certainty, symmetric joins, bespoke idioms instead of conditional holders, and a union cap that collapses to `mixed` (F6, R-1, SBF-1, PT-1).
3. **The finish line half existed.** The nsrt run, the rules oracle and their setup traps are written down (C-1, FR-5, CA-4). The scoreboard makes them permanent.
4. **Predictions ran about 2× too optimistic, and regressions went unseen for weeks** (D25-1, D20-12, C20-8, IT-5, 20L-1, 9B-1, C20-9). The cost meter therefore gates every commit, from the first one.
5. **The process caused the drift** (P-8, L-3, R-3, B-6, X-1, L9-13). It produced:
   - plans thousands of lines long, with audit rounds;
   - ledgers with need-chains;
   - reviews of reviews;
   - plans pulling items from other plans;
   - several branches and sessions at once.

   The new process forbids these shapes.
6. **Extending started before porting finished.** `number` was still unsupported while SMT overflow proofs were being designed (W-2). Everything beyond PHPStan waits for the parity exit.

**Outcome:**
- one public repo, one plan and one status file;
- a scoreboard run against PHPStan's own tests, as every milestone's finish line;
- a cost meter that gates instructions and heap on every commit.

## Decisions (all 2026-11-01)

| question | decision |
|---|---|
| Continue or restart | Ported from scratch on a fresh orphan branch `main` in this same repo, with its layout following PHPStan's `src/` |
| Order | Core parity, then the official extensions (Phase 3b), then work beyond PHPStan (Phase 4) |
| Legacy beyond-PHPStan work | Stays in the legacy repo, tagged, and comes back in Phase 4 |
| Legacy unpushed commits | Pushed |
| Repo (decided 2026-11-01, replacing "new repo") | Reuse `darktorres/phpstan-rs`: the old branches are renamed under `legacy/` and made read-only, the `*-final` tags pin them, and `main` becomes the default branch |
| Visibility | Public on GitHub (it already is), so CI runs on every push |
| PHPStan version | When the submodule is added, it and the oracle phar are set to the latest PHPStan release; both stay frozen for the whole parity phase |
| Measurement | The instruction and allocation cost meter runs from the first commit and gates regressions |
| Config | `phpstan.neon` is read directly; there is no TOML of our own |
| Parity exit | 100% of each suite, minus listed exclusions |
| Locator (PO-1) | PHPStan's locators, exactly; what they can't find is `class.notFound`; no eager vendor scan; anything only the legacy found goes into `known-deviations.md` |

## Ground rules (they become the new repo's `CLAUDE.md`)

**Process:**
- One milestone is active at a time, on one branch (`main`), with one writing session. No lanes, side branches or worktrees, except a throwaway A/B measurement (X-1, HL-6).
- **Only these docs exist:**
  - `README.md`, `CLAUDE.md` and `LESSONS.md`;
  - `PLAN.md`: milestones with numeric exits;
  - `STATUS.md`: under 100 lines, rewritten in place, holding the current milestone, the scoreboard, the cost figures, the next five tasks and a "Later" list;
  - `docs/design/<topic>.md`: at most about 300 lines each, deleted or folded into `PLAN.md` when the work lands.

  There are no `*_PLAN`, `*_LEDGER`, `*_REVIEW`, `*_DRILL` or `*_AUDIT` files, no paused-work stack and no task ids. A failing scoreboard test is the task (L-3, L9-13, FR-11).
- **A defect exists only once a test fails** (L3-1, R-3). It becomes a failing test first. It is fixed inside the milestone if it blocks the scoreboard; otherwise it gets one "Later" line.
- **Done means its tests ran and passed in CI.** There is no "done, unvalidated" state (20L-2).
- **Correctness belongs to the scoreboard and cost to the meter.** No batch mixes the two, and none is measured only at its end (20L-4, 9B-1).
- Only measured deltas are recorded. A predicted saving is a hypothesis (D25-1, WF-1).
- **Design small, build, then measure** (P-8, L9-6). An open design question is a spike stated as four things: the question, the method, an accept bound and a fallback.
- Phase 4 doesn't start before the parity exit (W-2).

**Code:**
- **Port from the PHPStan source file that the failing test exercises.** Keep its structure, identifiers and messages, and read the source rather than recalling it (F12, CA-6, D25-5).
- Legacy code is a quarry. Copy a piece only if it fits the PHPStan-shaped module and passes that module's suite.
- Where PHPStan is wrong at runtime, the port matches PHPStan and the case goes into `parity/known-deviations.md` (CA-3).
- **`unsafe` is allowed only in the `pcre` crate** (UN-1).
- **No hash-map iteration order leaks** into a result, a lookup or an encoding. Serialized maps are written in key order, with a byte round-trip test (WF-2, 20L-5).
- **Every memo has:**
  - a hit counter;
  - an explicit list of its key inputs;
  - a test for each invalidation point.

  A memo under about 20% hits is removed (9B-5, OFF-4, SBF-5, HL-3).
- **Every cache has:**
  - a key built from an enumerated input list, with a test that the list is complete;
  - a test that a warm run reads it and writes nothing (OFF-4, PB-3).
- **Concurrency** (OFF-3, GS-2, MG-11):
  - nothing parallel runs while holding a lock that a pool job can wait on;
  - process-wide state is a value handed down, never a hidden static;
  - one injected worker pool serves the whole program.
- **No second implementation stays resident under `cfg(debug_assertions)`.** A comparison against the old behaviour is a test, or a throwaway run that is deleted with the change (D20-9, 9B-4).
- **Library crates do no I/O.** They return results as data, and printing is the caller's job (GS-4, GS-5).
- Generated files are written to a temporary file and renamed (BR-3).
- Tests clean up any container or process they start, and never change the process environment (BR-3, UN-4).
- A rename re-reads every comment it touches, and a doc comment sits on the item it describes (SB-7).

## Phase 0: Close the legacy repo

1. **Freeze every line of work** (X-1, MO-1, WS-1):
   - Push `master` and `php-catalog`.
   - Tag `legacy-final`, `catalog-final` and `master-old-final` (the effects engine and the builtin-runs recorder), and push the tags.
   - For each of `builtin-runs`, `lazy-context` and `lazy-includes`: push and tag it if it holds unique work, otherwise delete it.
   - Remove every worktree, after checking `git status` in each:
     - the `.claude/worktrees/wf_*` ones, with their branches;
     - the five under `~/.cache`;
     - `~/projects/php-catalog`, once its branch is pushed.
   - **Copy this plan file into the legacy repo as `RESTART_FINDINGS.md` and commit it.** The findings then survive at the tag, and `LESSONS.md` can cite it.
   - Rename the old branches under `legacy/` (locally and on GitHub): `master`, `php-catalog`, `master-old`, `builtin-runs`, `lazy-context`, `lazy-includes`. Delete the `worktree-wf_*` branches (all at a commit already in `master`).
   - Protect `legacy/*` with a GitHub ruleset so nothing can be pushed to them.
   - Old code is read through `git show legacy-final:<path>`, or one read-only worktree of `legacy/master` at `~/projects/phpstan-rs-legacy`, the only worktree allowed.
   - Copy the legacy release binary to `~/.cache/phpstan-rs-legacy/bin/`. It is M8's reference (L-1, LED-11).
   - **Sweep the data that lives outside git.** Move each of these into `~/.cache/phpstan-rs-legacy/<name>/`:
     - `~/projects/phpstan-rs-kept`;
     - `~/.cache/prs-profile` and `prs-probe`;
     - `level3-triage/probes`;
     - `phpstan-rs-scratch/spike`;
     - the C7 compile-error corpus;
     - `.corpus-runs`.

     Add a `LOST.md` recording what is already gone: the E2.V edits in `prs-profile/engine-new`, and `run-631ad001.json`.
2. **Write `LESSONS.md`** (300–400 lines) in the new repo, from `RESTART_FINDINGS.md`. Each lesson takes one or two lines and cites its legacy file and section at the tag. The groups:
   - architecture;
   - measurement traps;
   - portability;
   - PHP runtime facts proved with `php`;
   - reverted attempts and refuted ideas;
   - process.
3. **Seed the design inputs:**
   - `docs/design/` starts from `CRATE_SPLIT_PLAN.md` §3.1–§3.11 and Appendix F, without the `ext-*` crates (C-4, C-5).
   - The scoreboard recipe comes from C-1, FR-5 and CA-4.
   - `docs/design/syntax.md` starts from L9-2 and L9-3 plus the grammar facts (D25-2, SB-1, L9-4, L9-7, 20L-7, C20-1).
   - `parity/known-deviations.md` is seeded from CA-3, EV-4, SB-4 and L9-9, as entries (key, file, php evidence), not prose.
4. **Memory:**
   - Archive the old project memory files into the legacy repo.
   - Keep only the general working rules: no `/tmp`; one niced heavy job at a time; commit per sub-item; fmt before tests; the wine runner; `php` as a runtime oracle; no agents.
   - Rewrite the project-goal memory as "parity first, measured by the scoreboard; beyond parity after the exit".

## Phase 1: The new repo's skeleton and process

- **Repo:** the existing public `darktorres/phpstan-rs`, on an orphan branch `main` (`git checkout --orphan main`, empty tree) made GitHub's default branch. CI checks out `main` with `--single-branch` so the legacy history isn't fetched. Nothing secret goes in, and no local path appears in committed config.
- **Submodule:** `vendor/phpstan-src`.
  - When it is added, check GitHub for the latest PHPStan release (the legacy pinned 2.3.1) and set the submodule to that tag.
  - The oracle phar is the same release.
  - Both stay **frozen for the whole parity phase**.
- **Crates** follow PHPStan's namespaces. They are created empty, each milestone fills its own, and Cargo enforces the layering (CP-11). Each Rust file's header names the PHPStan file it ports.

  | crate | ports / holds |
  |---|---|
  | `paths` | std-only path bytes, file identity, the report spelling of a path (D20-8, CP-1) |
  | `pcre` | PCRE2 built statically from bundled source; the one FFI crate (UN-1); filled in M6 |
  | `syntax` | nikic's AST as a flat, immutable, index-based tree; resolved names; the Visitor; the one interner (LED-3, L9-2, L9-8) |
  | `phpdoc` | phpstan/phpdoc-parser |
  | `types` | `src/Type` and `TrinaryLogic`, behind a `ClassHierarchy` oracle |
  | `stubs` | phpstorm-stubs, `functionMap*.php`, `functionMetadata.php` and `stubs/*.stub`, embedded with a build-time name index and parsed lazily (MG-4, OFF-1, OFF-2) |
  | `reflection` | `src/Reflection`, lazy, with per-class memos and the locator chain |
  | `analyser` | `src/Analyser`; emits nothing |
  | `builtins` | `src/Type/Php` |
  | `rules` | `src/Rules`, with collectors |
  | `diagnostics` | errors, tips, identifiers |
  | `testing` | RuleTestCase and TypeInferenceTestCase, run through the real Session |
  | `project` | the Session, the finalizer, the result cache |
  | `cli` | `src/Command`, config, levels, output formats |
  | `xtask` | `parity`, `cost`, port-map generation |

- **Workspace conventions** (AU-1, CC-3, CR-1):
  - edition 2024 and `resolver = "3"`; dependencies only in `[workspace.dependencies]`;
  - `[workspace.lints]` sets:
    - `unsafe_code = "forbid"`, with only `pcre` overriding it;
    - `unreachable_pub`;
    - print macros denied in library crates;
    - `clippy::debug_assert_with_mut_call` denied;
    - `disallowed-methods` on rayon's raw constructors;
  - `.gitattributes` forces LF.
- **Toolchain:** stable, unless a measured need says otherwise (RM-1).
- **`main`** (CP-4, CR-2):
  - it parses arguments and sets up the allocator;
  - it then runs the command on a named thread with the worker stack size;
  - a panic is resumed, so the exit status stays 101;
  - output goes through a locked writer, and a broken pipe exits 141.
- **One load path** serves the CLI, the tests, the cost workloads and later the LSP (D20-8, GS-5).
- **`parity/port-map.tsv`** is generated from `vendor/phpstan-src/src`: one row per PHPStan file, with its Rust module and a status (`todo`, `ported`, or `wontport` plus a reason).
- **CI from the first push:**
  - **Legs:** Linux, macOS and Windows on x86_64, plus Linux on aarch64.
  - **Checks:** fmt, clippy, tests in debug and once in an optimized profile (CC-3), doc, `cargo deny`, `machete`, the scoreboard subset, and the cost gate on Linux. Every gate the plan relies on runs here on every push (AU-2).
  - **Job settings** (CI-1, CI-2, CI-4):
    - jobs are split by profile and run side by side;
    - `cache-on-failure`;
    - `timeout-minutes` on every job;
    - a concurrency group that cancels only superseded pull-request runs;
    - one composite setup action;
    - `actionlint` on workflow changes;
    - no LTO bench builds.
  - **Release:**
    - the five targets of RM-1, each as `.gz` plus `.sha256`;
    - Linux binaries built for an older glibc or for musl, so they run on current LTS distros.
  - **Local:** Windows is also checked under wine with MinGW (`scripts/winrun.sh`, CP-8).

## Phase 2: The scoreboard and the cost meter (before any analysis code)

### Scoreboard: `cargo xtask parity [--filter <glob>]`

It writes `parity/score.json` and a one-line summary, which every milestone commits.

| suite | oracle | compared |
|---|---|---|
| lexer | `token_get_all` under the pinned php; nikic's lexer emulation where versions differ (20L-7) | token kinds and spans, over every input below and over all three line breaks (CP-6) |
| parser | nikic/php-parser `--json-dump` on these inputs:<br>• every PHP file in PHPStan's tests<br>• nikic's error-recovery fixtures<br>• php-src's `.phpt` sections (F11), which also stand in for the C7 compile-error corpus (OFF-6), lost before Phase 0 (`LOST.md`)<br>• the generated operator-pair file (D25-2)<br>• a large vendor tree (P-9, LED-9) | normalized AST JSON; the same first error as nikic and `php -l`; zero false refusals on the vendor tree |
| phpdoc | phpstan/phpdoc-parser run from PHP on the same files, plus its own test suite | the printed node tree |
| types | `nsrt/*.php`: `assertType`, `assertNativeType`, `assertVariableCertainty`, `assertSuperType` | the expected string against our `describe()` |
| rules | `tests/PHPStan/Rules/**/*RuleTest.php` (418 files). A PHP script parses them with nikic to get each test's rule class, data files, expected `[message, line, tip]` and config; PHPStan is never executed | the error set per test method, with only that rule enabled |
| level oracle | the phar against us over `tests/PHPStan/Rules/**/data` (3,122 files at 2.3.4), at every level, with C-1's recipe (FR-5, EV-5's level table) | per identifier: PHPStan-only, matched, ours-only |
| end-to-end | the phar against us on monolog and symfony/console, at every level, with the same `phpstan.neon` (Piwigo is not public and needs extensions, so it stays the milestone-end regression check) | the JSON output diff |

Rules for the scoreboard:
- **The nsrt recipe** (C-1, EV-3):
  - `tests/PHPStan` is a scan directory, with phpstan-src's composer dependencies installed;
  - each file runs at the highest PHP version its `// lint` header allows;
  - counts are kept by file, by asserted expression kind and by owning milestone.
- **The setup traps are designed in** (CA-4, OWL-2, OM-2):
  - each nsrt file is its own project;
  - `phpVersion` is given as a range;
  - bleeding-edge toggles are set per test;
  - the 14 rule-test files that need PHPStan's classes loaded go into `exclusions.md`;
  - paths are made project-relative before any comparison;
  - an anonymous class's name holds no absolute path.
- **`describe()` produces PHPStan's exact `VerbosityLevel::precise()` strings**, so normalization shrinks to union member order at most (CA-5).
- **Per-assert diffs between commits** classify each change as equal, narrower, wider or incomparable (SB-6).
- **Every pinned input is a released version fetched by digest:** the phar, php-parser, phpdoc-parser, the submodule and the oracle php. CI regenerates every derived artifact and compares (PL-3).
- `parity/exclusions.md` and `parity/known-deviations.md` exist from day one.
- **The legacy binary is run through the suites its CLI can drive**, once, to give a "where the old code stood" line in `STATUS.md` (expected near 40%).

### Cost meter: `cargo xtask cost`

- **What it runs:** callgrind and dhat, in a fixed `cost` profile (release optimizations, debug info, frame pointers, no LTO), at one worker thread, because Ir is deterministic only there (LED-6).
- **What it records per workload,** as one row each in `perf/history.tsv`, committed with the change:
  - Ir;
  - blocks and bytes allocated;
  - peak heap;
  - heap live at exit;
  - the top 20 functions by self cost, and the top 20 allocation sites.
- **Workloads** live in `perf/workloads.toml`. A workload never changes once added; a larger variant gets a new name. The set grows with the port:
  - from M1:
    - parsing PHPStan's `src/` plus the nsrt files;
    - the `deep-*` group (SB-5, SBF-4, EV-3, MG-8, L9-9, PO-2, PD-2): a 100,000-branch `elseif`; 32,000-term `.` and 4,000-term `&&` chains; 10,000 `instanceof` `||` checks; 2,000-deep arrays and calls; 1,000 nested `match`; nested loops; a 3,000-trait chain; a 50,000-member union; a 100,000-deep PHPDoc nest; an 8,000-item constant array;
  - from M2: PHPDoc parsing over the same corpus;
  - from M4:
    - 100 lookups through the lazy stub index;
    - `one-fixture` start-up and `one-file-project`, with the legacy's 50.9 M Ir and 46 ms as reference rows (PB-1);
  - from M5: a fixed set of nsrt files;
  - from M7: `analyse` on symfony/console and on `/home/torres/piwigo17-frozen`, beside the legacy's rows (L-1, LED-11);
  - `suite-lib`: the test suite, measured whole, and never a reason for a product change (LS-1, LS-4, WF-6).
- **Cache mode is part of a workload's definition** (9B-2, HL-1, IT-4):
  - each workload states its mode (off by default) and uses a private cache directory;
  - a warm workload fills the cache first and checks it is unchanged after the run;
  - a run that rewrote its cache is invalid;
  - series in different modes are never compared.
- **Memo counters** are printed with each workload (9B-5).
- **The gate** (WS-2, 9B-1):
  - Ir, peak heap and live heap are all gated against the parent commit;
  - the starting thresholds are 1% for Ir and 2% for each of the others;
  - a commit that exceeds one fails, unless its message carries a `cost:` line;
  - one run is enough, since the counts are deterministic.
- **Reading a regression** (HL-2, OFF-10, IT-4):
  - the meter prints the top-20 self-cost and allocation-site diffs;
  - it notes that function names move with inlining;
  - inclusive costs are never summed;
  - the legacy's `cgfn.py`, `callers.py` and `perfdump.py` are its quarry.
- **CI:** the gate runs on Linux, on every push, with small workloads and ten-frame stacks. Whole-stack Piwigo heap profiles (the patched dhat) run at milestone ends (OH-1).
- **Wall time** is measured locally, at milestone ends:
  - paired alternating A/B runs with a Wilcoxon test and an output-hash check (`abab.py`);
  - `/proc/loadavg` and the THP counters logged;
  - the 1/2/4/8-thread scaling curve is informational (LED-6, IT-3, HL-4).
- **Reference rows** come from `legacy-final`, run through the same workloads (L-1, PB-1). A reference is always reproducible from a tag (9B-3).

**Phase 2 exit:**
- every suite runs and its baseline is recorded (the new code fails them all);
- `xtask cost` has written the first M1 row;
- the CI gate trips on a test commit that adds a `Vec` clone, and that commit is then dropped.

## Phase 3: Port to parity, in PHPStan's dependency order

Each milestone:
- **starts** with one design doc of at most about 300 lines, whose inputs are the findings cited below;
- **ends** with a full scoreboard run (no other suite regresses), `cargo test --workspace` on all OSes in CI, a Piwigo diff used only as a regression check, a committed `score.json`, and `STATUS.md` updated with the milestone's cost rows.

Every commit passes the cost gate or carries a `cost:` line.

### M1: Lexer and parser
- **Port:** nikic's AST shape and names, with the PHP 8.6 grammar (F11).
- **Built in from the start:**
  - **The tree:** flat, immutable and index-based (LED-3, L9-2, L9-3):
    - one typed vector per node kind;
    - `NonZeroU32` ids;
    - `ExprNode` ≤ 32 B and `StmtNode` ≤ 40 B, asserted;
    - a sparse doc table.
  - **The tokens:** a kind-plus-span array filled by one lexing loop, with no payload table and no streaming (D20-1, L9-7, 9B-6).
  - **Recorded as the source is read** (D25-3):
    - by the lexer: ignore comments and doc hashes;
    - by the parser: names resolved in source order, declarations, labels and the generator flag.
  - **Positions:** byte offsets, with a lazy line index over `\n`, `\r\n` and a lone `\r` (MG-5, D20-3, CP-6).
  - **Names:** bytes, with one interner and `Name`, `FoldedName` and `ConstName` (L9-8, C20-1).
  - **Errors:** recovery is kept for the LSP, but parity reports only `phpstan.parse` (OFF-6).
  - **Depth:** every walk iterates or grows its stack. It is tested at 100,000 levels on a 256 KiB thread, and PHP's own nesting limits apply (FR-9, SB-1, LED-9, L9-7).
  - `syntax.md` lists the PHP facts as tests (D25-2, SB-1, L9-4, L9-7, 20L-7, C20-1).
- **Exit:**
  - the lexer and parser suites pass;
  - zero false refusals over the vendor tree;
  - the parse and `deep-*` workloads are recorded.

### M2: PHPDoc
- **Port:** phpdoc-parser, plus `TypeNodeResolver`'s whole keyword list (W-2, OW-1).
- **Built in from the start:**
  - one grammar, lowered straight to types with no print-and-reparse (D25-4);
  - nodes keep their spans (F6);
  - it rejects what phpdoc-parser rejects, and converts numbers the way phpdoc-parser does (EV-1, L9-9);
  - parsing is linear and depth-limited;
  - tags are memoized by text, keyed by the lexer's doc hash (9B-6).
- **Exit:** the phpdoc suite passes.

### M3: Types
- **Port:** `TypeCombinator`; `accepts`/`isSuperTypeOf` with `TrinaryLogic`; constant, accessory and generic types; templates; `ErrorType`; `describe()`.
- **Built in from the start:**
  - `Type` is opaque outside its crate, and every question is a method mirroring PHPStan's `Type` (FR-3);
  - types are hash-consed, and their node flags are assigned in one table (MG-7, D25-4, WF-4, WF-7);
  - an n-ary union builder, using PHPStan's large-union rules and never a cap to `mixed` (F6, C20-2);
  - the relation memo is keyed with a content generation and never `clear()`ed per generation (D20-6, 9B-6);
  - float `pow` is pure Rust (OFF-8);
  - property tests, including "every displayed type parses back" (F4, MG-7).
- **Exit:** the type-only nsrt asserts pass (PHPDoc → type → `describe()`).

### M4: Reflection
- **Port:** the stubs; the function map and its deltas; the locator chain over Composer; class, function and constant reflection; PHPDoc inheritance; `ConstantResolver`; `InitializerExprTypeResolver`.
- **Built in from the start:**
  - **Stubs** (MG-4, OFF-1, OFF-2):
    - a build-time name index;
    - lazy, per-file parsing;
    - version gates kept as data;
    - a serialized snapshot only if the meter asks for one.
  - **Declarations** are immutable and `Arc`-shared. Derived facts go in a five-level per-class `OnceLock` memo, with no whole-registry passes (FR-4, D20-5, LS-3, WF-5).
  - Nothing runs in parallel under a memo lock (OFF-3).
  - **PHPStan's locator chain**, as decided (LED-5, PB-4, PO-1).
  - **Composer is read as PHPStan reads it** (FR-2, L9-5, L9-11).
  - **PHP's binding and identity rules** (D25-2, L9-8, SC-1 to SC-5).
  - **The query-engine question** is decided in the design doc, with plain memos as the default (LED-2, L9-10).
- **Exit:**
  - the reflection-only nsrt files pass;
  - a one-file fixture analyses in milliseconds (GS-1);
  - the M4 workloads are recorded.

### M5a: Analyser
- **Port:**
  - `NodeScopeResolver`;
  - `MutatingScope`: holders with certainty, natives, conditional holders, invalidation;
  - `TypeSpecifier`;
  - `StatementResult` and `ExpressionResult`;
  - the derived nodes;
  - `ArgumentsNormalizer`;
  - the extension registry.
- **Built in from the start:**
  - FR-6 is the component spec;
  - EV-4's holder order sets the sub-milestones;
  - one call pipeline (M-5, MG-9);
  - each expression is inferred once, with per-node results (FR-6, D25-5);
  - scope joins visit only changed keys (MG-8);
  - adding a narrowing table for an assertion library is cheap (TR-2).
- **Exit:**
  - the nsrt asserts outside `Type/Php` calls pass, including native types and certainty;
  - the `deep-*` analysis workloads are linear.

### M5b: `src/Type/Php`
- **Port:** every return-type, type-specifying and throw-type extension, one per commit (CA-1, EV-5).
- **Built in from the start:**
  - PHPStan's value comes first, never a wider runtime value (EV-5);
  - per-text analyses (regex, `sprintf`) are memoized (D20-10);
  - `RegexArrayShapeMatcher`'s grammar is ported as it is (UN-3).
- **Exit:** all nsrt asserts pass.

### M6: Rules
- **Port:** one sub-milestone per `src/Rules/<Family>`, ordered by level from 0 to max, with collectors.
- **Built in from the start:**
  - **The first commit is `RuleLevelHelper::findTypeToCheck` plus `accepts` with reasons**, and every rule goes through them (FR-8, OH-4);
  - identifiers and messages are copied from the rule class, never renamed (CA-6);
  - the rule → level table comes from the neon files (FR-8, EV-5);
  - messages are formatted only when reported (D25-6, OFF-9);
  - collectors replace whole-program passes (GS-3, D20-4);
  - `regexp.pattern` runs through `pcre`, with PHP's front end byte for byte (UN-1, UN-2).
- **Exit:**
  - each family's rule tests pass;
  - the level oracle matches per identifier at every level.

### M7: The command
- **Port:**
  - `phpstan.neon`: NEON, `parametersSchema`, includes, level files, bleedingEdge;
  - the baseline, in PHPStan's format;
  - `ignoreErrors` and inline ignores;
  - PHPStan's output formats and command line;
  - the result cache and parallel analysis;
  - exit codes 0/1/2/130/141.
- **Built in from the start:**
  - **Config:**
    - F10's NEON resolver, with `dump-parameters` as its oracle;
    - FR-1 and EV-2's `ignoreErrors` and `FileExcluder` semantics;
    - `normalizePath` (CP-1).
  - **The finalizer** records what hid each finding (RV-1).
  - **Panics** are isolated per unit and reported in PHPStan's internal-error form (CR-1).
  - **The result cache:**
    - content hashes for change detection, never stat (20L-8, 9B-6);
    - dependencies taken from reflection reads, with invalidation by whole component (PB-2, WF-3);
    - files without `mmap`, keyed by enumerated inputs and hashed with blake3 (D20-7, OFF-4, C20-7).
  - **The incremental-equals-fresh test:** sampled on every push, complete at milestone ends (MG-12, MO-2, LS-1).
  - **Scheduling:** largest-first, on one injected pool (MG-11, C20-7, GS-2).
  - **Unsupported options:** settings that need PHP to run (DI `services:`, executing `bootstrapFiles`) are parsed, reported as unsupported and listed in exclusions.
- **Exit:** the end-to-end diff against the phar is clean at every level.

### M8: Performance
- **Work:**
  - read the cost history and its top-20 tables;
  - measure paired wall time against the legacy binary and the phar;
  - measure afresh the candidates OFF-7, D20-10, D20-13, IT-3 and PB-6.
- **Exit:**
  - faster than the phar on every workload;
  - at or below the legacy's Piwigo reference rows for Ir, peak heap and wall time.

### Exit threshold

**100% of every suite, minus `parity/exclusions.md`.** Each exclusion is added with its reason, in the commit that adds it, and the reason is one of:
- a won't-port, such as PHPStan's own `Api` or `Playground` rules, re-checked against the source;
- a test that needs PHP to run, as PHPStan does in-process;
- a display form the normalization can't cover.

`known-deviations.md` is a separate list. Its tests pass by matching PHPStan. Each milestone's exit is this same rule applied to its slice.

## Phase 3b: Official extensions (right after the core parity exit)

**How each extension is ported:**
- as a crate mirroring its package;
- activated the way PHPStan activates it: `includes:` of its `extension.neon`, or the extension installer's list;
- measured by its own test suite, extracted like core's, with the same exit rule;
- ported rule class by rule class from its current release (TR-1).

**Order, by downloads** (TE-1):
1. phpstan-phpunit;
2. deprecation-rules;
3. phpstan-symfony: the container-XML half (`consoleApplicationLoader` is an exclusion);
4. phpstan-doctrine: type inference first (whatever needs a live `objectManagerLoader` is an exclusion);
5. strict-rules;
6. phpstan-mockery;
7. webmozart-assert;
8. the other official `phpstan/*` packages (beberlei-assert, nette, php-parser, dibi).

## Phase 4: Beyond parity (after the Phase 3b exit)

**Rules for every item:**
- It is a normal milestone with its own tests.
- It is re-justified when picked up, because many legacy designs assume an eager, closed-world architecture (F9, D25-7, TR-4).
- A runtime-truth check ships opt-in until its gained findings have been read and classified on the corpora (OM-1, 20L-6, PT-3).

**The items, in order:**
1. **The out-of-process extension ABI** (K193). It runs PHP-written extensions beside the Rust core, and it is the route to:
   - larastan, phpstan-drupal, disallowed-calls and dba;
   - about 60 framework packages (TE-2, TR-5);
   - custom formatters (TE-3).
2. **The LSP and the VS Code extension**, on the Session (C-4 §3.10, FR-9, CR-3, L9-4). The legacy's LSP is the quarry.
3. **Runtime truth**, built as extensions of `MutatingScope`, impure points and `invalidateExpression`, never as a parallel engine (OW-2, OW-4, C20-10):
   - resolving `known-deviations.md`;
   - the runtime oracle with generated calls (F8, C20-4), with the runtime facts from R-1, SBF-3, BR-1, PL-5 and CC-4;
   - effects and implicit calls, starting from `master-old-final` (MO-1, BR-4);
   - **the PHP catalog, re-scoped to observation:** per-parameter type bounds plus tracing real test suites. Native analysis is not resumed (PE-1);
   - the open-world model and the overflow/relational work (W-1, OM-1, SBF-2).
4. **Extra checks:**
   - the legacy's opt-in families (F2, F3);
   - project passes (EV-5);
   - whole-program dead code, with its own reverse-dependency design (D25-7);
   - ideas from Psalm and Mago (respecting M-9's refutations);
   - the third-party ideas in TR-3.
5. **Ecosystem typing:** SQL/DBAL, Laravel and the rest of the top-100 survey, preferring the extension ABI.
6. **Deeper performance work**, beyond M8.
7. **The rest:**
   - the report viewer (RV-2);
   - crash reporting (CR-4); `crash-repro reduce` may come earlier, in `testing`;
   - distribution: a Composer launcher, a container image, attestations.

## Critical files

- **The legacy code**, read-only at the `legacy-final` tag (branch `legacy/master`):
  - **design inputs:**
    - `RESTART_FINDINGS.md` (this plan);
    - `CRATE_SPLIT_PLAN.md` §3 and Appendix F;
    - `CRATE_SPLIT_CAUSES.md`;
    - `docs/level-9/` Part B;
  - **code quarries:**
    - `crates/syntax`, `crates/phpdoc`, `crates/types`;
    - `gates/phpstan-oracle/oracle.py`;
    - `tools/php-comparison-matrix`;
    - `tests/runtime_oracle.rs`;
    - `scripts/winrun.sh`, `scripts/abab.py`, `scripts/cgfn.py`;
    - `patches/dhat`;
    - `.github/workflows/`;
    - `editors/vscode`.
- **The catalog**, read-only at `catalog-final`. **The effects engine and the recorder**, at `master-old-final`.
- **New in the restarted repo:**
  - `PLAN.md`, `STATUS.md`, `LESSONS.md`, `CLAUDE.md`;
  - `parity/port-map.tsv`, `parity/extract/*.php`, `parity/score.json`, `parity/exclusions.md`, `parity/known-deviations.md`;
  - `xtask/src/parity.rs`, `xtask/src/cost.rs`;
  - `perf/workloads.toml`, `perf/history.tsv`;
  - the crates of Phase 1.

## Verification

- **Phase 0:**
  - `git ls-remote` shows the `*-final` tags and only `main` plus `legacy/*` branches;
  - pushing to a `legacy/*` branch is refused;
  - `git worktree list` shows only the main checkout;
  - every branch is either pushed and tagged, or deleted by decision;
  - `RESTART_FINDINGS.md` is at `legacy-final`;
  - `LESSONS.md` cites only files and sections that exist at the tags;
  - `LOST.md` lists what is already gone.
- **Phase 1:**
  - `main` is the default branch, shares no commit with `legacy/master`, and CI runs green on its first push on every leg;
  - the submodule's tag equals the latest PHPStan release at the time it was added, and the phar is the same version.
- **Phase 2:**
  - every suite runs;
  - for one hand-picked file per suite, the extracted expectations match a real phar run;
  - the legacy line and the first cost row are recorded;
  - the gate trips on the planted regression.
- **Every milestone:**
  - its exit number is in the committed `score.json`;
  - no other suite regresses;
  - CI is green on every OS;
  - every commit passed the cost gate or carries a `cost:` line.
