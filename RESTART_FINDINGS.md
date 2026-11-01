# Plan: restart phpstan-rs as a faithful port of PHPStan, then extend it

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

**What reading every legacy doc established.** Every doc was read in full, on `master` and on `php-catalog`. The appendix holds the findings, cited here by id.

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
| Continue or restart | A new repo, ported from scratch, with its layout following PHPStan's `src/` |
| Order | Core parity, then the official extensions (Phase 3b), then work beyond PHPStan (Phase 4) |
| Legacy beyond-PHPStan work | Stays in the legacy repo, tagged, and comes back in Phase 4 |
| Legacy unpushed commits | Pushed |
| Name | The new repo takes `phpstan-rs`; the old one becomes `phpstan-rs-legacy` and is archived |
| Visibility | The new repo is public on GitHub, so CI runs on every push |
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
   - **Copy this plan file into the legacy repo as `RESTART_FINDINGS.md` and commit it.** The appendix then survives at the tag, and `LESSONS.md` can cite it.
   - Rename the GitHub repo to `phpstan-rs-legacy` and archive it. Move the checkout to `~/projects/phpstan-rs-legacy`.
   - Copy the legacy release binary to `~/.cache/phpstan-rs-legacy/bin/`. It is M8's reference (L-1, LED-11).
   - **Sweep the data that lives outside git.** Move each of these into `~/.cache/phpstan-rs-legacy/<name>/`:
     - `~/projects/phpstan-rs-kept`;
     - `~/.cache/prs-profile` and `prs-probe`;
     - `level3-triage/probes`;
     - `phpstan-rs-scratch/spike`;
     - the C7 compile-error corpus;
     - `.corpus-runs`.

     Add a `LOST.md` recording what is already gone: the E2.V edits in `prs-profile/engine-new`, and `run-631ad001.json`.
2. **Write `LESSONS.md`** (300–400 lines) in the new repo, from the appendix. Each lesson takes one or two lines and cites its legacy file and section at the tag. The groups:
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

- **Repo:** a fresh `git init` at `~/projects/phpstan-rs`, pushed to the **public** GitHub repo `phpstan-rs`. Nothing secret goes in, and no local path appears in committed config.
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
| parser | nikic/php-parser `--json-dump` on these inputs:<br>• every PHP file in PHPStan's tests<br>• nikic's error-recovery fixtures<br>• php-src's `.phpt` sections (F11)<br>• the C7 compile-error corpus (OFF-6)<br>• the generated operator-pair file (D25-2)<br>• a large vendor tree (P-9, LED-9) | normalized AST JSON; the same first error as nikic and `php -l`; zero false refusals on the vendor tree |
| phpdoc | phpstan/phpdoc-parser run from PHP on the same files, plus its own test suite | the printed node tree |
| types | `nsrt/*.php`: `assertType`, `assertNativeType`, `assertVariableCertainty`, `assertSuperType` | the expected string against our `describe()` |
| rules | `tests/PHPStan/Rules/**/*RuleTest.php` (418 files). A PHP script parses them with nikic to get each test's rule class, data files, expected `[message, line, tip]` and config; PHPStan is never executed | the error set per test method, with only that rule enabled |
| level oracle | the phar against us over `tests/PHPStan/Rules/**/data` (2,936 files) and the corpus files, at every level, with C-1's recipe (FR-5, EV-5's level table) | per identifier: PHPStan-only, matched, ours-only |
| end-to-end | the phar against us on Piwigo, monolog and symfony/console, at every level, with the same `phpstan.neon` | the JSON output diff |

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

- **The legacy repo**, read-only at `legacy-final` (`~/projects/phpstan-rs-legacy`):
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
  - `git ls-remote` shows the three tags;
  - `git worktree list` shows only the main checkout;
  - every branch is either pushed and tagged, or deleted by decision;
  - `RESTART_FINDINGS.md` is at `legacy-final`;
  - `LESSONS.md` cites only files and sections that exist at the tags;
  - `LOST.md` lists what is already gone.
- **Phase 1:**
  - the repo is public, and CI runs green on its first push on every leg;
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

---

# Appendix: findings from the legacy docs

This appendix is the evidence behind the plan. Every legacy doc was read in full: 34 files on `master`, plus the `php-catalog` branch's own docs. Each section below covers one file, and lists that file's findings by id.

**Ids by file, main repo** (lines in parentheses):

| file | finding ids |
|---|---|
| CRATE_SPLIT_PLAN.md (8,318) | C-1 to C-5, FR-1 to FR-11 |
| CRATE_SPLIT_CAUSES.md (2,798) | CA-1 to CA-7 |
| ROADMAP.md (5,932) | F1 to F13 |
| CRATE_SPLIT_EVIDENCE.md (5,534) | EV-1 to EV-6 |
| CRATE_SPLIT_AUDIT.md (190) | AU-1 to AU-4 |
| MAGO_ARCHITECTURE_COMPARISON.md (17,594) | MG-1 to MG-12, M-1 to M-9 |
| PROFILE_DRILL_PIWIGO_25G.md (1,703) | D25-1 to D25-7, P-1, P-2, P-8 |
| PROFILE_DRILL_PIWIGO_20G.md (2,002) | D20-1 to D20-13, P-3, P-4, P-6, P-7 |
| PROFILE_DRILL_PIWIGO_OFF.md (1,299) | OFF-1 to OFF-10, P-5, P-9 |
| PROFILE_DRILL_PIWIGO_OFF_HEAP.md (499) | OH-1 to OH-4 |
| PROFILE_DRILL_PIWIGO_LEDGER.md (2,244) | LED-1 to LED-11, L-1 to L-3 |
| STAGE_B_REVIEW.md (1,535) | SB-1 to SB-7, R-2 |
| STAGE_B_FIX_REVIEW.md (4,842) | SBF-1 to SBF-6, R-1, R-3 |
| STAGE_C_REVIEW.md (237) | SC-1 to SC-6 |
| LEVEL3_TRIAGE.md (162) | L3-1, L3-2 |
| OPEN_WORLD_PLAN.md (766) | OW-1 to OW-4, W-1 |
| OPEN_WORLD_LEDGER.md (160) | OWL-1 to OWL-3, W-2 |
| BY_REF_WARNING_AND_FIBER_DESIGN.md (372) | BR-1, BR-2 |
| OVERFLOW_MEASUREMENT.md (276) | OM-1 to OM-3 |
| CI_SPLIT_PLAN.md (334) | CI-1 to CI-4 |
| GATE_SPEED_PLAN.md (622) | GS-1 to GS-6 |
| PROJECT_BUILD_COST_INVESTIGATION.md (1,137) | PB-1 to PB-6, B-1, B-2 |
| CROSS_PLATFORM_PLAN.md (2,310) | CP-1 to CP-12, B-3 |
| CRASH_REPORTING_PLAN.md (1,316) | CR-1 to CR-5, B-5 |
| UNSAFE_PLAN.md (284) | UN-1 to UN-4, B-4 |
| ITEMS_1_TO_3_FIXES_PLAN.md (2,008) | IT-1 to IT-5 |
| MASTER_ONLY_LEDGER.md (1,168) | MO-1 to MO-6 |
| PIWIGO_CORPUS_TRIAGE_ab4c2b5.md (252) | PT-1 to PT-4, X-4 |
| TOP100_PHPSTAN_EXTENSIONS.md (114) | TE-1 to TE-3 |
| TOP100_RESEARCH_NOTES.md (1,659) | TR-1 to TR-5 |
| REPORT_VIEWER_PLAN.md (169) | RV-1 to RV-4 |
| docs/level-9/*.md (4,994) | L9-1 to L9-13, X-3 |
| README.md (182), CLAUDE.md (111) | RM-1, RM-2 |

**Ids by file, `php-catalog` branch:**

| file | finding ids |
|---|---|
| PHP_SEMANTICS_CATALOG_PLAN | PC-1 to PC-3, X-2 |
| PHP_SEMANTICS_CATALOG_LEDGER | PL-1 to PL-5 |
| PHP_SEMANTICS_CATALOG_CONTRACT and _CONTRACT_REVIEW | CC-1 to CC-5 |
| PHP_SEMANTICS_CATALOG_EVALUATION, _ALTERNATIVES, _COMPARISON | PE-1 to PE-3 |
| WORK_STACK.md | WS-1, WS-2 |
| BUILTIN_RUNS_RECORDER_PLAN | BR-3 to BR-5 |
| PROFILE_DRILL_PIWIGO_20G.md (only the 1,880 lines not in master's copy) | C20-1 to C20-10 |
| the six PIWIGO_OPTIMIZATION docs | PO-1 to PO-5 |
| PROFILE_DRILL_LIBSUITE and _LIBSUITE_RELEASE | LS-1 to LS-4 |
| PROFILE_DRILL_PIWIGO | PD-1 to PD-3 |
| PROFILE_DRILL_PIWIGO_9071_BATCH_PLAN | 9B-1 to 9B-7 |
| PROFILE_DRILL_PIWIGO_HEAD_LEDGER | HL-1 to HL-6 |
| PROFILE_DRILL_PIWIGO_20G_LEDGER | 20L-1 to 20L-9 |
| WORKFLOW_PROFILE_REPORT | WF-1 to WF-7 |

The branch's PROFILE_DRILL_PIWIGO_25G and PIWIGO_CORPUS_TRIAGE are identical to master's.

**Cross-document findings:** X-1 and B-6, in the last section.

### CRATE_SPLIT_PLAN.md (8,318 lines, read in full)

- **C-1. Parity was measured once, by hand, and the harness was never built.** The run was against PHPStan 2.2.16 at `76e738b4`:
  - **nsrt:** the suite has 1,688 files, with 18,480 `assertType` and 313 `assertNativeType` calls. **phpstan-rs missed 11,106 `assertType`s, so about 40% passed.**
    - The misses by asserted expression: 4,788 variables, 3,399 calls that PHPStan types through extensions, 1,016 operators, 707 method calls, and smaller groups.
    - `assertNativeType` and `assertVariableCertainty` (681 calls) were not measured.
  - **Rules:** an oracle ran the phar and phpstan-rs over PHPStan's own rule-test data (2,936 files, level 10) and matched findings per identifier.
  - **Display:** the normalization the comparison needs is written down: union member order, `true|false` as `bool`, `int<0,1>` as `0|1`, and so on.
  - **Causes:** every mismatch was sorted by cause into `CRATE_SPLIT_CAUSES.md`, Appendices G–J.

  The `cargo xtask nsrt` and `rules-oracle` commands were planned for Stage D and **never built**, so the numbers exist only in prose, from one run. The scoreboard is that design built and made permanent.
- **C-2. The crate split was already "re-architect into PHPStan's shape, in place."** Its §2–§3 target is PHPStan's architecture:
  - **analysis core:**
    - an engine that emits nothing;
    - expression-keyed holders with certainty;
    - `MutatingScope` transitions and invalidation;
    - `ExprHandler`s with `specifyTypes`, and a public `TypeSpecifier`;
    - a NodeCallback sink;
  - **API surface:**
    - a port of the `Scope` API;
    - derived nodes mapped one to one onto `src/Node`;
    - the full set of extension traits;
  - **rules and reflection:**
    - `RuleLevelHelper`, with levels read from the neon files;
    - lazy reflection with a locator chain, and a function-map overlay;
  - **infrastructure:**
    - one PHPDoc grammar;
    - interned names, NodeIds and a Visitor;
    - a Session and a result cache;
  - **extensions:** one `ext-*` crate per mirrored package.

  That is this plan's destination too. It stalled at C3 of seven stages.
- **C-3. Why it stalled.** The approach was "byte-identical refactor first, then the behaviour change as an approved delta".
  - It ran 14 permanent gates, some of them per commit: the corpus at levels 0–10, type snapshots, baseline compatibility, test-name manifests, module-graph edges, performance, build time, and others.
  - Every step had to keep 240k lines and 3.7k fixtures working exactly while the design underneath them changed.

  In the new repo nothing has to stay byte-identical, and the scoreboard is the only correctness gate. Keep the cheap gates that catch real regressions: fmt, clippy, tests, the scoreboard and the cost meter.
- **C-4. §3 is a ready design input for M1–M7, taken item by item rather than as a plan to execute:**

  | section | what to take |
  |---|---|
  | §3.1 syntax | NodeIds, resolved names, the Visitor, parse-time annotations |
  | §3.2 types | dedicated variants, a `ClassHierarchy` oracle, one tri-state relation, accessory types, a `PhpVersions` range defaulting to 50207–80699 |
  | §3.4–3.5 reflection | the five-level lazy memo, the locator chain, the function-map overlay, `ConstantResolver`; the stub storage is replaced by MG-4's lazy index |
  | §3.7 | `Diagnostic` and `RuleError` |
  | §3.8 | the analyser |
  | §3.9 | levels from the neon files |
  | §3.10 | the Session, the finalizer order, the result cache, exit codes 0/1/2/130/141 |
  | §3.11 | the testing crate |

  From Appendix F, these decisions carry over:
  - rules return errors;
  - `Type` is a closed enum plus `ExtensionTypeData`;
  - an unknown ancestor is `Maybe`;
  - the exit codes.

  One is reversed: "a PHPStan baseline is never read". The new repo reads `phpstan.neon` and PHPStan baselines directly.
- **C-5. There are no `ext-*` crates until parity.** The 19 extension crates go to Phase 3b (the official ones) or Phase 4. During parity only `phpstan-src` itself is ported, including its bundled `src/Type/Php`.
- **FR-5. How C-1's rules-oracle numbers were made** (§D intro). The run setup:
  - the phar ran outside `vendor/`, from a directory without one, with only the corpus autoloader;
  - level 10, with `phpVersion` as a range;
  - results were filtered to engine identifiers;
  - files that raised internal errors were excluded, leaving 941.

  At `76e738b4` it gave 1,620 findings only phpstan-rs reported (1,056 of them `offsetAccess.nonOffsetAccessible`) and 2,039 misses. The scoreboard's level oracle takes this recipe as it is.

#### Stages A–G (lines 1091–7928)

- **FR-1. Stages A–C are a catalog of PHPStan behaviour the legacy got wrong, each with the PHPStan test file that shows it.** About 150 items: the nsrt or rules-oracle file and the count of mismatches. It confirms the scoreboard covers this ground: every item is a test in PHPStan's suites. It's also a ready checklist for the milestones (B3 → M1, B4/B5 → M2/M3, C4–C8 → M4, C9 → M5). Examples by milestone:
  - **M1 (parser facts):**
    - `"${expr}"` interpolation;
    - legacy octal literals; hex past i64 is a float;
    - byte strings (`"\xff"` keeps its bytes);
    - keywords fold case (`<?PHP`, `DECLARE`, `PRIVATE(SET)`);
    - a shebang line;
    - PHP 8 precedence of `+`/`-` vs `.`;
    - arrow fn/`throw`/`include` binding looser than `and`/`or`/`xor`;
    - `new $b[0]()` / `new $o->cls()` as `new_variable`;
    - `(void)` over a whole statement; `yield $x = 1`;
    - assignment as an operand everywhere;
    - trait adaptations kept; `(set)` visibility on promoted params;
    - `$obj->$$name`; spans inside strings; language constructs as nodes (isset, empty, exit, eval, include, print, list, `@`).
  - **M2/M3:**
    - `TypeCombinator::union`: `never` absorbed, ranges merging, `processArrayTypes`/`reduceArrays` keeping tagged unions (163 nsrt mismatches), `bool` absorbing `true|false`;
    - `TypeCombinator::intersect` reducing by hierarchy;
    - the `ErrorType` (289 nsrt `*ERROR*` expectations);
    - subtracted types `mixed~T` (54);
    - template inference by `inferTemplateTypes` plus `generalizeInferredTemplateType` (265 nsrt);
    - late types (`new<T>`, `key-of`, `T[K]`, `int-mask`);
    - callability and class-strings in `accepts`; `EQUAL_UNION_CLASSES`; `EXTRA_OFFSET_CLASSES` (80 rules-oracle findings);
    - conditional parameter types (81 nsrt);
    - display escaping (127 nsrt);
    - `iterable<K,V>`; phpdoc-parser's number literals and shape-key identifiers; linear and depth-limited type parsing.
  - **M4:**
    - `TypehintHelper::decideType` whole;
    - PHPDoc inheritance of assert/param-out/self-out/throws/pure/templates/`@var`;
    - trait composition (`insteadof`, `as` visibility);
    - `@extends` binding only the named parent;
    - mixins (templates, recursion, traits);
    - `@property-read`/`-write` pairs;
    - `@phpstan-require-extends` members;
    - a removed function is unknown in the analysed range (`mysql_*` under 8.5);
    - `PS_UNRESERVE_PREFIX_`;
    - the functionMap and its deltas through `SignatureMapParser` and `Php8SignatureMapProvider::mergeSignatures`, replacing phpstorm-stubs' PHPDoc (about 100 rules-oracle findings: `curl_*` benevolent false, `__stringAndStringable`, `RoundingMode`, …);
    - multi-variant signatures (`ParametersAcceptorSelector::selectFromArgs`);
    - the 30 `conf/config.neon` stub files plus 6 gated `StubFilesExtension`s;
    - `functionMetadata.php` for purity;
    - `stubFiles` semantics;
    - TypeNodeResolver keyword precedence (pure-callable, `enum-string`, `__benevolent`);
    - generics with fewer args than templates (`IteratorAggregate<T>` = `<mixed, T>`);
    - `typeAlias.circular`;
    - constant-named PHPDoc types through `use const`;
    - `InitializerExprTypeResolver` for constants and enum values;
    - `PHP_VERSION_ID` as a range; `PHP_INT_MAX` as a 32/64-bit union;
    - an unknown ancestor is `Maybe`.
  - **M7:**
    - `ignoreErrors` with `IgnoredErrorHelper`'s full semantics (message/messages/rawMessage/identifier(s)/path(s)/count/reportUnmatched, merging);
    - `IgnoredRegexValidator`;
    - `FileExcluder` semantics shared by `excludePaths`;
    - `parametersSchema.neon` parameters one by one;
    - `featureToggles` (about 70 rules-oracle findings came from bleeding-edge checks the legacy ran unconditionally);
    - `skipCheckGenericClasses`;
    - strict-rules setting core parameters;
    - baseline paths relative to the baseline file;
    - config discovery of PHPStan's six `phpstan.neon` names;
    - symlink loops walked once.
- **FR-2. Composer and the PHP version, as M4/M7 facts:**
  - the root package's own autoload is context (1,253 false `class.notFound` came from missing this);
  - an installed package's bundled `vendor/` (rector, ECS) is context;
  - `config.vendor-dir`; `psr-0`;
  - Composer's constraint grammar (whitespace after an operator);
  - an open upper bound runs to `MAX_PHP_VERSION` 80699;
  - the default `PhpVersions` range comes from `config.platform.php`, else `require.php`, else PHPStan's whole range;
  - functions, constants and `files` entries load eagerly, classes lazily.
- **FR-3. Design rule from B4 "Type behaviour lives on the type":** the legacy had 373 functions taking a `Type` and about 1,055 inline `match`/`if let` inspections across 108 files, answering the same question differently in different files. **For the new repo, from M3's first commit:** `Type` is opaque outside `types`, and every question is a method mirroring PHPStan's `Type` interface (`isArray()`, `getOffsetValueType()`, `toBoolean()`, `generalize()`, …), trinary where PHPStan's is. Subtraction is a field of the subtractable variants, not a wrapper. `==` is structural, and PHPStan's `equals()` is a method.
- **FR-4. Design rules for reflection (C4/C5):**
  - a five-level per-class memo (ancestors, own, inherited, members, final), each level a `OnceLock`, reading only lower levels or the same level along an ancestor edge;
  - inheritance cycles broken on the ancestor level;
  - a panic leaves a cell retryable;
  - snapshots share declarations by `Arc` and record per-memo read sets for invalidation, which the LSP needs later;
  - collision order is canonical (stubs, context, analysed, by path), never arrival order;
  - a test that a load plus checking one file resolves no class that file doesn't reach.
- **FR-6. Stage D (lines 2990–4516) is the most direct input for M5.** It is a component-by-component port spec of PHPStan 2.2.16's `Analyser`, and each item names the PHPStan class to follow and the nsrt/rules files that test it:
  - **the PHPStan sources it follows:**
    - the parser visitors (`LastConditionVisitor`, `TryCatchTypeVisitor`, the closure-argument visitors for `array_map`/`array_filter`/`curl_setopt`/`Closure::bind`, `PipeTransformerVisitor`, …);
    - `StatementResult`/`ExpressionResult` with exit, throw and impure points;
    - one `StmtHandler`/`ExprHandler` per kind, each with processing, type resolution and `specifyTypes`;
    - `SpecifiedTypes` with conditional-holder recipes;
    - two-phase assignment;
    - one issetability descriptor for isset/empty/`??`;
    - the `EqualityTypeSpecifyingHelper` call comparisons (count/strlen/preg_match/get_class/array_key_first…);
    - `FuncCallScopeEffectsHelper`;
    - `processArgs` with the intrinsic callback overrides;
    - `ClosureTypeResolver`;
    - `NewHandler` (`asFinal`; a class-string gives `T`).
  - **Scope (D5):**
    - natives as full holders;
    - the virtual keys;
    - certainty with Maybe, and `canAnyVariableExist` at a file's top level;
    - `PassedByReference::createsNewVariable`;
    - conditional holders from merges, `&&`/`||` and assignments;
    - `invalidateExpression` (impure calls on the receiver, closures invoked immediately, volatile expressions, `clearstatcache`, declarations);
    - by-reference intertwining;
    - `afterExtractCall`;
    - `rememberPossiblyImpureFunctionValues`.
  - **Control flow:**
    - **`LOOP_SCOPE_ITERATIONS = 3` with `generalizeWith`** (208 nsrt mismatches came from the legacy's 6 passes without widening);
    - `ForHandler`/`ForeachHandler` (unrolling constant arrays up to 16/8 keys, the post-loop rewrite, the never-entered scope);
    - backward-goto fixpoint;
    - short-circuit scopes;
    - `SwitchHandler`; `MatchHandler` (`isScopeConditionallyImpossible`);
    - `TryCatchHandler` (catch entry from throw points, `finally` merging);
    - class member order;
    - constructor first (`rememberConstructorScope`);
    - closure binding and by-reference `use` (`processClosureScope`);
    - `PHP_VERSION_ID` narrowing the scope's version.
  - **The cost lesson behind "each expression inferred once":** with two inference paths and re-inference, nested calls took 7 s at 800 levels and 183 s at 1,600, and a 32,000-term `.` chain overflowed the stack. M5's design is per-`NodeId` results plus a memoized pure `type_of` per scope snapshot, and handlers that iterate over left-deep binary chains.
  - **The legacy's three runtime-over-PHPStan deviations** (D5 intro: eval defining variables, an assignment inside `assert()` being Maybe, a by-reference `use` seeing every enclosing write) go into `known-deviations.md`. **The port matches PHPStan first**, per the parity rule.
- **FR-7. The size of the gap at `76e738b4`, by named cause (Appendix H keys cited in Stage D):**
  - loop-generalize 208; byref-array-fns 210; count-narrow 217;
  - list-write 114; closure-param-infer 77; loose-eq-narrow 78;
  - conditional-holders 57; strlen-narrow 59; string-fn-narrow 52;
  - guards 54; ob-level 47; impure-invalidate 42;
  - method-call-memory 29; readonly-remember 25; byref-closure-use 27;
  - operator-ranges 87; comparison-operators 96.

  These are the largest nsrt buckets M5 has to close. They confirm M5 is the biggest milestone and should be split into sub-milestones by handler family (statements/flow, assignments and arrays, calls and closures, narrowing/TypeSpecifier, scope memory/invalidation).
- **FR-8. Stage E (lines 4517–6590) is the rules and builtins catalog, in the same form as FR-1:** each item is a PHPStan rule behaviour the legacy got wrong, with the rule-test data file and a count. Every item is a test in PHPStan's suite, so the scoreboard's rules suite and level oracle cover them. The new repo needs no copy of the catalog. The structural points that shape M6:
  - **Build `RuleLevelHelper::findTypeToCheck` before any rule.** About 2,950 of 9,623 misses on the corpus oracle came from rules deciding for themselves whether a `mixed` operand (implicit or explicit) is reportable. This repeats in E3(b) for every operand rule. **M6's first commit is `findTypeToCheck` plus `accepts`, and every rule asks it.**
  - **The rule → level table comes from the neon files.** The legacy capped levels at 6 in the CLI and used another cutoff in the LSP. Its level of first report differed from PHPStan's on about 10 rows (union argument 7 vs 5, union return 7 vs 3, `method.notFound` on non-`$this` 2 vs 0, and others).
  - **Builtins (E2) follow the same rule as parity:** each `src/Type/Php` extension is ported as PHPStan has it, and the throw-type extensions are included (without them `catch.neverThrown` can't be judged). Appendix G's runtime counts are 517 calls wrong at runtime: 220 missing a value, 190 typing a call PHP rejects, and 107 giving another PHP version's return. These are evidence for Phase 4's runtime-truth work. They are not M5 scope beyond what PHPStan itself does.
  - **Version-dependent returns:** a return that changed between PHP versions follows the analysed `PhpVersions` range, as PHPStan's deltas do. This confirms that M4 carries the version range into function signatures.
  - **The rules oracle's matching recipe** (E intro):
    - same identifier, same file and line first, then within 3 lines;
    - within 60 lines for `missingType.*`, `return.missing`, `constructor.unusedParameter`, `parameter.phpDocType` and `phpDoc.parseError`;
    - each finding used once;
    - "(in context of class …)" copies collapsed;
    - the 13 files that need php-parser/PHPStan classes loaded excluded.

    This goes into the scoreboard's level-oracle row as is. At `76e738b4` it gave 15,476 PHPStan findings, 8,840 matched, 6,636 PHPStan-only and 2,588 phpstan-rs-only, with 152 identifiers never matched.
  - **Collectors (E1) are PHPStan's `Collector`/`CollectedDataNode`:** unused private members, trait-in-every-host verdicts (`ConstantConditionInTraitRule`), and the "in context of class" merge. The legacy's project passes (duplicates, architecture, banned, composer hygiene, class shapes) are its own checks and belong in Phase 4.
  - **Restricted usage (E3g)** is PHPStan's `Restricted*UsageExtension` set, with one list of usage sites. `@internal` is core; `@deprecated` belongs to deprecation-rules (Phase 3b).
  - **E4's extension list** (doctrine, phpunit, pest, strict-rules, deprecation, disallowed-calls, shipmonk, …) and its notes on how much of each package the legacy covered are input for Phase 3b and Phase 4. phpstan-doctrine's rules (`doctrine.dql`, `findByArgument`, the entity checks) did not exist in the legacy at all.
- **FR-9. Stage F (lines 6591–7503) is the M7 command-line design and the Phase 4 LSP design.** For M7:
  - **The finalizer owns the result**: file, not-file and internal errors, inline-ignore records, collected data, stats and warnings. It runs without re-parsing.
  - **Exit codes:** 0 clean, 1 findings, 2 untrusted (bad config, no files, internal error), 130 Ctrl-C, 141 closed pipe. The legacy exited 0 on findings and parse errors.
  - **A parse error is a non-ignorable `phpstan.parse` diagnostic.**
  - **Lines end at `\n`, `\r\n` and a lone `\r`.** The legacy's lexer missed the lone `\r` and so misplaced every later line and inline ignore. This goes into M1's tests.
  - **Deep nesting uses `stacker::maybe_grow` at each recursion point instead of a depth limit.** PHP parses 2,000 nested arrays; the legacy rejected 400. This goes into M1/M5's "built in from the start".
  - **Baseline paths are relative to the baseline file.** Generating from part of the project keeps the rest. Only an analysed file's entries can be stale.
  - **Formats:** PHPStan's names (`raw`, `table`, `json`, `prettyJson`, `github`, `gitlab`, `teamcity`, `junit`, `checkstyle`, `ci-detected`), with `table` on a terminal and `raw` otherwise. `--format` can repeat.
  - **PHPStan's command line works as is** (`analyse`/`analyze`, `-c`, `-l`, `-b`, `--error-format`, `--autoload-file`, `--memory-limit` and `--xdebug` accepted and ignored).
  - **The result cache** has a per-file entry with content hash, raw errors, ignore records, collected data, dependencies resolved from reflection reads (not syntactic names), and a per-exported-name fingerprint. The key covers binary identity, config, version range, level and composer files. It keeps several entry sets, is written atomically under a lock with `0700` and `CACHEDIR.TAG`, and a config-file `tmpDir` can't point outside the project.

  Everything about the LSP and language features (VFS overlays, per-root Sessions, position encodings, incremental sync, rename, completion, semantic tokens, inlay hints, pull diagnostics, the VS Code extension) is Phase 4 item 2's design input. Its listed LSP bugs are bugs of the legacy's architecture, not tasks.
- **FR-10. Stage G (lines 7504–7928) is the crate-extraction mechanics.** The new repo starts with its crates, so G has nothing to port except four lessons:
  - **build-time targets:** a rule or extension edit must not rebuild the engine; a stub edit must not rebuild `syntax`/`types`. At `1b20b638` a rule edit took 8.5 s, an engine edit 7.3 s;
  - **hermetic fixtures:** a fixture reads nothing outside its directory;
  - **fixtures in plain `cargo test`** in the debug profile, which needs a shared stub layer (B-1);
  - **a cited-path check for docs**, so a move can't leave a doc pointing at a missing file.

  These go into Phase 1's process rules and CI.
- **FR-11. The size and shape of CRATE_SPLIT_PLAN confirms C-3.** About 8,300 lines, of which roughly 5,000 are per-item catalogs of PHPStan behaviour, each item citing PHPStan's own test file. The scoreboard enumerates the same items automatically. **The new PLAN.md therefore lists milestones and exits, never per-behaviour items.** A failing scoreboard test is the item.
### CRATE_SPLIT_CAUSES.md (2,798 lines, read in full)

This file is generated: Appendices G–J sort every nsrt and rules-oracle mismatch at `76e738b4` to a named cause, each with its files.

- **CA-1. Where the nsrt gap sat, by layer** (Appendix H, uncited files):
  - parser B3: 6;
  - types B4: 947;
  - reflection C4/C6/C7/C8: 424;
  - analyser D3/D5: 2,254;
  - builtins E2: 1,498.

  Appendix G adds 3,108 more on calls to the 94 `Type/Php` return-type extensions. Appendix J adds 1,711 in cited files. **Roughly half the type-inference gap was the analyser and half the builtins, with types next and reflection small.** For the new plan:
  - **split M5 into M5a (analyser: scope, flow, TypeSpecifier) and M5b (all of `src/Type/Php`: return-type, type-specifying and throw-type extensions)**, each with its own nsrt exit;
  - M5b covers about 4,600 legacy mismatches over 94+ extensions, about the size of M5a, and is mechanical port work, one extension per commit.
- **CA-2. The largest single causes**, as a sanity list for M3/M5 (each is a PHPStan mechanism, ported whole rather than patched):
  - Appendix G: filter_var 277; min/max 217; array_column 211;
  - D3: count-narrow 217; byref-array-fns 210; loop-generalize 208;
  - preg-shapes 155+174; key-exists-fold 153; settype 156; bcmath 141+;
  - type-print 127; list-write 114; operator-ranges 87+34;
  - loose-eq-narrow 78 and loose-eq-fold 107; closure-param-infer 77+42;
  - error-type 65; static-type 64+42; conditional-holders 57.
- **CA-3. Seed list for `parity/known-deviations.md`.** Every "right at runtime" entry, checked by the legacy against php 8.5:
  - Appendix G's ten: `vsprintf('foo', [])`; `array_flip` duplicates; `get_class` never false; `array_fill` negative start on PHP 8; `ini_get('auto_prepend_file')`; `filter_var(null, BOOLEAN, NULL_ON_FAILURE)`; `mb_substitute_character(null)`; `strval([])`; preg_split's invalid pattern and its null flags;
  - Appendix H's 3 and Appendix J's 12: `array_diff*($a, [])` unchanged; `match` without default narrowing after; `BcMath\Number == null`; typed `IteratorAggregate` foreach; `E_ALL` 30719 from 8.4; a non-falsy string `== true`; nullsafe else; duplicate `@var`; and others;
  - Appendix I's 124 approved findings: `private(set)` never written; a static property statement; `catch (\Throwable)` after Throwable; `require_once` of a missing constant path; `@var` above `return`; and others.

  The port reproduces PHPStan's behaviour on each, and Phase 4 decides it. Copy the entries (key, file, php evidence) into `known-deviations.md` in Phase 0, not the prose.
- **CA-4. Scoreboard setup traps the legacy measured** (Appendix I "oracle setup", Appendix J "suite setup"):
  - give the phar `phpVersion` as a range, never one int (11 findings);
  - analyse each nsrt file in its own project, or scope `define()` per file (`bug-8526.php`'s `define('FOO')` leaked into `constants.php`);
  - assert `PHP_VERSION_ID` against the configured range;
  - build- and machine-dependent constants (`PHP_EXTRA_VERSION`, `DEFAULT_INCLUDE_PATH`, `PEAR_*`, `ZEND_*`) are their general type, not phpstorm-stubs' sample value;
  - PHPStan doesn't analyse a trait no class uses;
  - 14 rule-test files need php-parser/PHPStan classes the phar loads (exclude them, listed);
  - the phar's `phpstan.*` assert identifiers vs ours;
  - bleeding-edge toggles per test (43 findings came from toggles run unconditionally).

  Each goes into `parity/extract/` and the exclusions file from day one.
- **CA-5. Display normalization needs more than top-level union sorting.** Nested unions inside generics, `never` vs `*NEVER*`, a list's explicit keys, `array<(int|string), T>` vs `array<T>`, a template with its bound (`T of X (class …, argument)`), a newline inside a constant string, callable vs `callable(): mixed`, iterable splitting. **Better: make `describe()` produce PHPStan's exact `VerbosityLevel::precise()` strings,** so normalization shrinks to union-member order at most. That's also what PHPStan's own `assertType` compares.
- **CA-6. Of the 2,588 phpstan-rs-only rules findings** (Appendix I's verdicts on the 1,463 traced here):
  - 860 wrong;
  - 188 another identifier or reported twice;
  - 70 assertType mismatches;
  - 124 right at runtime;
  - 43 bleeding-edge toggles;
  - 178 oracle setup or approved renames.

  "Another identifier / reported twice / approved rename" doesn't exist in a port that takes PHPStan's identifiers and one-report-per-site from its rule classes. **M6 rule: identifiers and messages are copied from the PHPStan rule class, never renamed.**
- **CA-7. Process note.** The appendices were hand-traced line by line (5,137 + 1,711 + 1,463 lines read against fixtures) and frozen at one commit. In the new repo the scoreboard regenerates per-file results on every run. Cause lists aren't kept; the failing test file plus the PHPStan source it exercises is the cause.

### ROADMAP.md (5,932 lines, read in full)

- **F1. The legacy ported much of PHPStan's *behaviour*, but not its *structure*.**
  - "Done" lists hundreds of PHPStan identifiers and type behaviours. Many name the PHPStan class they mirror: `RuleLevelHelper::accepts`, `TypehintHelper::decideType`, `MissingReturnRule`, `MethodPrototypeFinder`, `LooseComparisonHelper`, `InitializerExprTypeResolver::resolveConcatType`.
  - Several were matched against the phar on Piwigo: `phpDoc.parseError` 155/155, `parameter.phpDocType` 47/47, `*.unresolvableType` 55/55.
  - The code is shaped around its own `Type` enum, `Registry` and `Checker`, not around PHPStan's classes.

  It ported behaviour piecemeal and measured it on one corpus, never against PHPStan's own tests.
- **F2. Deliberate divergences** that a faithful port undoes:
  - loop convergence capped at 6 passes with no widening, where PHPStan uses 3 with widening (`:388-403`);
  - receiver checks that flag a union as soon as any member is closed, where PHPStan filters the union (`:1217-1226`);
  - `Collector`s replaced by fixed points at registry build time (`:947-955`);
  - many opt-in checks with no PHPStan counterpart (`:957-1002`). These are Phase 4.
- **F3. Extensions mixed into the core,** in the same "Done" list as the parity work (`:1004-1081`):
  - Doctrine ORM and DQL;
  - live SQL reflection for three engines;
  - `restricted_symbols` (disallowed-calls);
  - PCRE validation over FFI;
  - the XML checks.
- **F4. Its own tools worth keeping:**
  - a PHP comparison model checked against a matrix that PHP itself generates (`tools/php-comparison-matrix`);
  - a `sebastian/comparator` model checked against comparator 4–7 (`tools/phpunit-comparator-matrix`);
  - proptest subtyping properties, which found two soundness bugs (`:329-331`);
  - "every displayed type parses back" (`:356`);
  - an audit that found about 20 places where matching on `Type::Object` alone turned unions and intersections into `mixed` (`:838-846`).
- **F5. How parity was judged before:**
  - a 20-agent audit of about 420 Rule classes (`:3633-3647`);
  - an 855-file agent audit (`:5855`);
  - identifier diffs against the phar on Piwigo;
  - the "Phase 183 level oracle" (`gates/phpstan-oracle`).

  Every one was done by agents or measured on one corpus. That is why a measured scoreboard is needed, and why `port-map.tsv` replaces those audits.
- **F6. Structural gaps against PHPStan, as the roadmap records them:**
  - no native-type channel, so `treatPhpDocTypesAsCertain` can't be built (Phase 180);
  - no asymmetric `foreach` join with a certainty downgrade (O1), which caused 47% of one scoped Piwigo run's `argument.type`/`return.type` findings (`:5770-5785`);
  - no `generalize()` widening in loops (`:5871-5874`);
  - no general intersect/remove and no traverse/map combinators (`:5716-5728`);
  - `static`/`$this` late binding without a calling class (`:5722`);
  - doc comments without spans, so PHPDoc errors land on the wrong line (`:1664-1674`);
  - `MAX_UNION_MEMBERS = 12`, collapsing to `mixed` to work around a hang (Phase 95 C1);
  - `ConditionalExpressionHolder` declined and replaced by about ten bespoke idioms (`:405-446`, `:5752`).

  Each is a divergence from PHPStan's architecture that cost a pile of point fixes. This is the strongest argument for a structural port.
- **F7. Design policies to carry over** (Phase 121 M1–M4):
  - one representation of unsealed shapes;
  - a closed `Type` with no public trait;
  - one canonical member order, sorted only at display time;
  - the most precise builtin signature (bleedingEdge's function map).

  For parity, the scoreboard runs with bleedingEdge on or off as each test's config says, because PHPStan's default is the loose map.
- **F8. A runtime oracle exists.** `tests/runtime_oracle.rs` runs fixtures under `php`, with older versions in Docker, and checks that `assertType` holds for the real values (`:1591-1597`). It is a Phase 4 second oracle.
- **F9. Parts 1–16 (Phases 85–188) are a backlog of about 70 beyond-PHPStan ideas**, written as designs against the legacy's code:
  - closed-world inference: A5, U1–U6, P1, X1/X2, H1–H3, AA1–AA6;
  - whole-program duplication: V1/V2, AI;
  - rule categories: D1 complexity, D3 identical branches, D4 taint-lite;
  - Psalm-derived: L1–L14, AP1 severity tiers, AQ1 mixed provenance;
  - J1/J2 per-directory strictness, and Q1/Q2 API diffing;
  - 153 TODO comments, and the 156 type-coverage ratchet;
  - the 181 viewer and 182 Latte;
  - 183 `init`, 184 rename, 185 project page, 186 PHP 8.6;
  - the LSP phases 105/107/109/110/187;
  - 188 "no silent drops".

  They go to Phase 4 as titles with a pointer each. Several rest on "phpstan-rs is closed-world and single-process", which the new architecture doesn't keep by default, so each is re-justified when picked up.
- **F10. Phase 183 made `phpstan.neon` a one-time conversion source to TOML. The new repo reads it live.** Three things carry over to M7:
  - **the NEON resolver spec:**
    - full nette/neon syntax;
    - includes, where later wins, lists append and maps recurse;
    - `key!`, entities and `(?)`;
    - `%currentWorkingDirectory%`, `%rootDir%`, `%env.X%` and `%tmpDir%`;
    - `expandRelativePaths`, which skips `%` and `*` values;
    - a flat `excludePaths` meaning `analyseAndScan`;
    - a base layer of phpstan-src's `conf/` plus each installed package's `extra.phpstan.includes`;
  - **its oracle:** the resolved parameters equal the pinned phar's `phpstan dump-parameters --json`, minus `analysedPaths`, `debugMode`, `productionMode`, `tempDir`, `__validate` and the editor keys. `dump-parameters` runs `bootstrapFiles`, so a fixture with a bootstrap needs its environment;
  - **the parameter table's measured classification:** which `parametersSchema` parameters and feature toggles change PHPStan's output, verified by flipping each. It becomes the M7 checklist.

  The new repo also reads PHPStan's baseline format.
- **F11. Phase 186's conformance corpus is the second half of the parser suite** (M1):
  - **inputs:**
    - php-src's ~23k `.phpt` `--FILE--` sections: raw bytes, `--INI--` as `-d` flags, `--FILE_EXTERNAL--`, skipping `--REDIRECTTEST--`;
    - 136 `*.stub.php` files;
    - nikic's 180 parser fixtures;
    - optionally other parsers' fixtures;
  - **oracle:** `php -l` in a Docker `php:<tag>` image, cached by php-src hash and tag;
  - **result classes:** `REJECTS-VALID` (a bug), `CRASH`, `ACCEPTS-PARSE-ERROR`, `ACCEPTS-COMPILE-ERROR` (informational) and `FIXTURE-DISAGREES`;
  - **allowlist,** with reasons: stack-limit tests, `zend.multibyte`, `short_open_tag`, `__halt_compiler` payloads;
  - **PHP 8.6** (GA 2026-11-19): partial function application (`?`, `name: ?`, a bare `...`, `clone(?)`) and one doc-comment placement change, with no new tokens. M1 targets the 8.6 grammar; nikic v5.9 has PFA.
- **F12. Verification habits to carry over:**
  - check against a real current `phpstan` or `php`, never memory;
  - `LC_ALL=C sort` before diffing;
  - classify corpus diffs by location: a changed message at an already-diagnosed line is a reshape, and a new location is traced;
  - `cargo llvm-cov` finds untested code.
- **F13. "No silent drops" (Phase 188).** The legacy dropped every diagnostic of any function that called `extract()` or `eval()`: 223 functions, 31,660 of 32,555 Piwigo diagnostics, without saying so. PHPStan has no such rule; it handles `extract()` through `afterExtractCall`.
  - The port does what PHPStan does.
  - Under `-v`, the finalizer may print one count line per suppression mechanism (level, inline, `ignoreErrors`, baseline).

### CRATE_SPLIT_EVIDENCE.md (5,534 lines, read in full)

The uncondensed copy of CRATE_SPLIT_PLAN's stage items at `df967fc1`, with every fixture and count. It's a frozen record. Its content overlaps FR-1 and FR-8; only what the condensed plan dropped is recorded here.

- **EV-1. Stage A–B extras** (lines 1–819):
  - Stage A's harness design is a useful pattern for the new `testing` crate. Fixture kinds:
    - Integration (`is_empty()` and cross-rule negatives);
    - rule-scoped (one identifier family);
    - directory (multi-file and composer);
    - Codemod (fixes).

    A fixture's settings live in a toml header; a setting with no key gets one.
  - **B4's type checklist for M3, beyond FR-1:**
    - one subtyping implementation (`accepts` built on `isSuperTypeOf`, not the reverse);
    - `ErrorType` accepts and is accepted by everything, so a reported error doesn't cascade;
    - conditional types distribute over a template's argument;
    - `EXTRA_OFFSET_CLASSES`: DOMNamedNodeMap, DOMNodeList, the Dom\ lists, PDORow, ResourceBundle, FFI\CData, SimpleXMLElement, Threaded;
    - removing an enum case through `getEnumCases()`;
    - truthiness removes whole members;
    - a template narrows by intersection (`A&T`, else `T~A`);
    - late types `new<T>`, `template-type<>`, `KeyOfType`/`ValueOfType`/`OffsetAccessType` stay late over templates;
    - callable parameters carry names and purity;
    - `PhpVersion` carries the patch (`>= 80105`);
    - display: escaped quotes, control characters in double quotes, float literals keep `.0`, parenthesized callable returns.
  - **B5 phpdoc for M2:**
    - one tag splitter;
    - phpdoc-parser's number literals (hex, octal, binary, `_`, exponents);
    - by-reference unnamed callable parameters;
    - an offset access as a conditional subject;
    - else-chained conditionals;
    - `$this` as a callable parameter type;
    - **reject what phpdoc-parser rejects** (`@use T` without `<`, `callable()` without a return, `A & B | C`, `\\Foo`, `what{}`);
    - a tag's diagnostics sit on the tag's own line;
    - shape keys take `-` and `\` after the first character;
    - **type parsing is linear and depth-limited** (the legacy took 9 s at 2,000-deep `array<>`).
  - **B8 Composer for M4/M7:** the same facts as FR-2, plus: no process-global panic hook (it raced parallel analysis).
  - **EV-2. Stage C extras** (lines 820–1803), for M4 and M7:
    - **`ignoreErrors` port checklist:**
      - `message`, `messages`, `rawMessage(s)`, `identifier(s)`, `path(s)`, `count`, `reportUnmatched`, and the string form;
      - entries with the same path, message and identifier merge with their counts summed;
      - `count` applies to single-path entries only;
      - messages are matched by PCRE with their own delimiters;
      - `IgnoredRegexValidator`'s checks (an unescaped `||` or `()`, a top-level alternation starting with a type name, an anchor in the middle) are load errors, which needs a PCRE-syntax parser plus the phpdoc type parser;
      - the path matcher is `FileExcluder`'s (a literal file, a directory prefix, or fnmatch where `*` crosses `/`), shared with `excludePaths`;
      - every config error names its full key path.
    - **Predefined constants over the version range:**
      - `PHP_VERSION_ID` is `int<min.id, max.id+99>`;
      - `PHP_MAJOR_VERSION`/`PHP_MINOR_VERSION` are unions;
      - `PHP_INT_MAX` is the 32/64-bit union;
      - never fall back to the stub's `define()` value.
    - **One `InitializerExprTypeResolver`** for class and global constants, enum values, parameter defaults and attribute arguments (arithmetic, bitwise, ternary, `??`, constants, `new`). A constant reference cycle is `mixed`.
    - **`generalize`** keeps list-ness and the key type.
    - **The memo's five levels** (FR-4) in detail:
      - ancestors: extends/implements in resolution order, use in composition order, mixin targets, unresolved edges;
      - own: own members, `@method` tags, aliases, value-of, constant-named types;
      - inherited: PHPDoc inheritance and property hooks;
      - members: mixins and member owners;
      - final: `TypehintHelper::decideType` whole;
      - constants are typed through the ConstantResolver;
      - cycles are broken on the ancestor level;
      - "a load plus checking one file resolves no class that file doesn't reach";
      - collision order is canonical: embedded stubs, then context, then analysed, each by path.
    - **The stub layer is keyed by `PhpVersions`.** phpstorm-stubs carries 2,234 `LanguageLevelTypeAware` and 598 `PhpStormStubsElementAvailable` attributes in 88 files, and 185 declarations change type at 8.4 or later. Member and parameter gates sit inside the files; the legacy concluded the layer parses per version, which OFF-2 revises to one parse per file with gates kept as conditions. Fixes to carry:
      - a `LanguageLevelTypeAware` type on a property or constant becomes `@var`, not `@return`;
      - `newest_type` reads every entry;
      - deprecation is per overload;
      - `PS_UNRESERVE_PREFIX_` names are the reserved words they spell, and language-construct pseudo-functions and `___PHPSTORM_HELPERS` leave the index.
    - **C6 overlay facts:**
      - the 30 `conf/config.neon` stubs plus 10 behind 6 gated `StubFilesExtension`s (BcMathNumber, JsonValidate, ReflectionClass, ReflectionEnum, SocketSelect by PHP version; ExtDs by the ds major version);
      - all 146 function and method PHPDocs apply, and reach implementations through PHPDoc inheritance;
      - functionMap plus seven deltas (74–85) plus the two bleedingEdge files, merged by `mergeSignatures`. **Decide the native signature base** (`phpstan/php-8-stubs` vs phpstorm-stubs) **by reading what PHPStan 2.x itself uses** (`Php8SignatureMapProvider` reads php-8-stubs);
      - multi-variant selection through `selectFromArgs`, named arguments included;
      - `functionMetadata.php` for purity;
      - user `stubFiles` semantics (a stub's PHPDoc replaces the declaration's, the native signature stays the real one, omitted members are kept, a stub declares a symbol only when nothing else does).
    - **C7 TypeNodeResolver facts:**
      - one name scope;
      - keyword types resolve before namespace qualification;
      - `__benevolent<>`;
      - an enclosing function's templates are active inside an anonymous class;
      - native type hints are resolved directly, not printed and re-parsed;
      - `T[]|iterable` unions;
      - a template name is a template even as a shape key;
      - fewer generic arguments than templates fill as `IteratorAggregate<T>` = `<mixed,T>`;
      - template defaults;
      - `typeAlias.circular`;
      - `use const` aliases in PHPDoc.
    - **C8:**
      - member lookup asks native members first, then the `@property`/`@method`/`@mixin` extensions; a tag at the same or a closer distance wins (`PhpClassReflectionExtension`);
      - mixins: recursion guards, a trait's mixins, `@mixin T`, a mixin method is static under `__callStatic`;
      - `@property-read`/`-write` pairs;
      - `@phpstan-require-extends` makes the base's members visible and makes the interface a subtype;
      - an unknown ancestor or class relates as `Maybe`.
    - **C9 global variables:** a global's summary includes `null` (a function can run before the assignment) and is `mixed` once an unseen write can reach it. That's runtime reasoning beyond PHPStan, which says `mixed`. **Port PHPStan's `mixed`; the summary is Phase 4.**
  - **EV-3. Stage D1–D3 extras** (lines 1804–2797), for M5a/M5b and the scoreboard:
    - **nsrt run detail:** `tests/PHPStan` as a scan directory; `// lint` headers (60 files; 47 run at 7.4). The 11,106 misses split by asserted expression kind, so the scoreboard reports by kind too.
    - **nsrt display normalization list** (until `describe()` is exact, CA-5): union order, benevolent parentheses, shape key order, `array<(int|string),T>`, list shape keys, `true|false`, the template scope suffix, combined accessory string names, `int<0,1>` vs `0|1`, `*NEVER*`, backslashes.
    - **Expression facts the analyser owns** (each an nsrt or rules-oracle file): compound assignments typed as the assigned value; pipes (`PipeHandler`, the left side checked against the first parameter, arity, `#[\NoDiscard]` through a pipe); magic constants as values; offset reads on constant arrays with non-literal keys, string offsets, `ArrayAccess` with template arguments, union containers; `ConstantArrayType`'s next-key rule (8.3 negative keys, `PHP_INT_MAX`); `$o::class`/`get_class` on final classes and templates; key-producing assignments (`array_key_first`, `array_search`, `count - 1`); `$this` carrying the class's own templates; class-string receivers; `static` in returns as the whole receiver; template receivers through their bound; union receivers via `combineAcceptors`; type-check calls as constants (including `@phpstan-assert*`); enum `cases()`/`name`/`value`; first-class callables with signatures; `processArgs` order and intrinsic callback overrides; inherited constructor bindings; `static::C` as declared type; template bound checked at the call; `clone($o, $with)`; dynamic method names; `call_user_func*`; spreads of shape unions; variadics `list<T>` under `@no-named-arguments`; by-ref out types of builtins and the array-modifying family; `ClosureTypeResolver` (generators, `never`, implicit nullable); `void` as `null` in a call's value; `K::{'X'}`; `new $c()` from a class-string; `new C()` as `asFinal`; `new` into a generic property; `array.duplicateKey` with PHP's key casts; key casts on every write/read/unset; static variables (PHPStan: `mixed`); `FuncCallScopeEffectsHelper`.
    - **Narrowing facts:** enum case subtraction; `!empty` on `int`; increments in conditions; `instanceof $classString`; `gettype()` compares; `specifyTypesForEqual`; list offsets implying lower ones and numeric-string keys; `CastHandler::specifyTypes`; `!is_scalar`; `!`/`&&`/`||` composition; nullsafe chains; `get_class` match arms; templates keeping `null`; conditional return types narrowing arguments; conditional `@phpstan-assert` resolved per call; every `EqualityTypeSpecifyingHelper` comparison (count, strlen, `array_key_first`, `preg_match`, `get_class`, string functions, `strtolower`).
    - **Deep expressions:** left-deep binary chains iterate (32,000-term `.` overflowed; 8,000 terms were quadratic). An M1/M5 cost workload: one deep-chain file.
  - **EV-4. Stage D4–D5 extras** (lines 2798–), for M5a:
    - **NodeCallback sink:** observer events instead of side channels; `ExpressionResultStorage` keeps each node's before-scope and answers rule queries; callbacks post-order; non-final passes run with a Noop sink; gatherer frames (`pushNodeGatherer`) for returns, yields, exits, impure points; replay of a converged pass (`RecordingNodeCallback`, `isReplayableConvergenceBody`). No diagnostics truncation anywhere.
    - **Holder model order** (each group builds on the last): natives as full holders → virtual keys (property-initialized, parameter original value, by-ref intertwining, always-remembered, possibly-impure call, clone reinitialization, foreach key/value, by-ref foreach value) → `MutatingScope::equals` for convergence → guards as `HasMethod`/`HasProperty`/`HasOffset` intersections → certainty. M5a's sub-milestones follow this order.
    - **Certainty facts:** maybe-defined is level 1 "might not be defined"; `canAnyVariableExist` at top level (a `@var` names it, `$argc`/`$argv` types); merges keep Maybe only for variables, calls and virtual keys, drop superglobals; `createsNewVariable`; variable variables, `compact()`, destructuring keys, `Closure::bind` `$this`, correlated `unset`; `isset`/`empty` don't define; truthy `??` defines.
    - **Conditional holders** come from three sources, each ported whole: merges (`createConditionalExpressions`, undefined-on-the-other-side holders, `preserveVacuousConditionals`), `&&`/`||` (`processBooleanSureConditionalTypes`/`NotSure`), and assignments (`AssignHandler`: truthiness-guarded sure types, ternary/match assignments narrowing the subject, `in_array`/`preg_match`/ternary results, `isExprSafeToProjectThroughVariable`).
    - **Invalidation facts:** impure method calls drop the receiver's holders (and `$this`'s for `self`/`static`/`parent`); immediately-invoked and later-invoked closures (`InvalidateExprNode`); typed property writes store the coerced value; `shouldKeepList`; offset writes make non-empty, literal-union keys build optional keys, property arrays update like variables, template arrays intersect; writes keep only members that accept them; `specifyExpressionTypeInPlace` narrows containers; `unset` removes keys; volatile expressions; the stat family and `clearstatcache`; declarations drop negative existence checks; output-buffer level; by-ref intertwining (including array-literal refs and `&$a[0]`); `afterExtractCall` (clears conditional holders, unknown variables Maybe); `rememberPossiblyImpureFunctionValues` defaults to `true`.
    - **Control flow facts:** loops 3 passes + `generalizeWith`; `ForHandler` (first-condition true/false, falsey exit narrowing, `inferForLoopExpressions`, `polluteScopeWithLoopInitialAssignments`); `ForeachHandler` (unsealed tails, `!== []` body narrowing, unrolling ≤16/8 keys, key→value conditional holders, `Traversable` ancestor arguments and `key()`/`current()`, `array_keys` iteratee, `getIterator()` throw point, by-ref value expr, never-entered scope, `polluteScopeWithAlwaysIterableForeach`); backward-goto fixpoint; short-circuit Maybe assignments and nullsafe argument evaluation; `SwitchHandler` (fall-through, exclusions, `continue` as break, nested `break`); `MatchHandler` (`isScopeConditionallyImpossible`, arms on earlier falsey scopes, `alwaysTrue` unless last, array subjects, small int ranges, `UnhandledMatchError` caught/`@throws`, template subjects); `TryCatchHandler` (catch from throw points, interface `&Throwable`, finally merge, `ReturnAfterFinallyNode`); expression statements with null context.
    - **Class/function context:** member order (static, constructor, instance, properties/hooks); `rememberConstructorScope` with `CalledMethodProcessor`; closure binding (`call`/`bind`/`bindTo`), immediately-invoked bodies, `processClosureScope`, by-ref closures converging like loops, static closures without `$this`, `enterAnonymousFunction` carrying used holders, closure types carrying throw/impure points with `@param-immediately-invoked-callable`; `PHP_VERSION_ID` narrowing the scope's version.
    - **D6 engine APIs** confirm M5a's public surface: Scope API, public TypeSpecifier with extension hooks, returned-error `Rule` + dispatcher (trait file attached), rule identifier families declared and debug-asserted, derived-node catalogue, Collector, extension registry. An `ignoreErrors` entry naming only identifiers we lack isn't reported unmatched — moot once every identifier exists, so the port doesn't need it.
    - **Deviations to `known-deviations.md`:** assignment inside `assert()` Maybe (zend.assertions); by-ref `use` seeing every enclosing write; eval defining variables.
  - **EV-5. Stage E extras** (lines 3419–5477). Its body is FR-8's catalog with every fixture; the scoreboard covers it. What it adds:
    - **The legacy's E2 stance differs from this plan's.** It treated PHPStan's builtin return value as "the candidate, not the target" and kept wider or runtime-checked values. **The new repo ports PHPStan's value first**; only the ten approved runtime deviations (CA-3) go into `known-deviations.md`. Appendix G's 517 runtime-wrong cases (220 missing values, 190 rejected calls typed as returning, 107 other-version returns) are Phase 4 input, except the version-dependent ones PHPStan's own deltas already cover.
    - **M5b's extension list includes the throw-type extensions** (`*ThrowTypeExtension`: printf, min/max, round, get_class, unserialize, str_split, array_chunk, trigger_error, Ds\Map, DateTime::modify, …); `catch.neverThrown` can't be judged without them. The largest return-type ports by mismatch count: `filter_var*` 290, min/max 217, `array_column` 211, sprintf 135, BcMath 134 (+170 `BcMath\Number` operators), `count` 114, `array_slice` 101, `settype` 156, preg `$matches` shapes 204+155.
    - **The level divergence table** (first level reporting, PHPStan vs legacy): `isset.variable`/`nullCoalesce.variable` 1/0; maybe-undefined 1/0; `method.notFound` on non-`$this` 2/0; `__call`/`__get` members 2/never; union argument 7/5; union return 7/3; maybe-non-offset-accessible 7/3; maybe-missing offset 7/never; PHPDoc-only child parameter conflict 3/never; `@var array` without value type 6/never. **The level-oracle suite tests each row at every level.**
    - **M6 rule infrastructure, beyond `findTypeToCheck`:** `accepts` with reasons as tips; PHPStan's standard tip helpers; messages naming namespaced functions by their full name and integer shape offsets unquoted (576 legacy rows differed on the first); `ExecutionEndNode` placement (`return.missing` and `paramOut` at each falling-off end, not the declaration); `constantToFunctionParameterMap.php` for flag parameters; `ParameterCastableToStringRule`; one restricted-usage site set (`Restricted*UsageExtension`) shared by `@internal`, `@deprecated` (3b) and disallowed-calls (Phase 4); `FileAssertRule` plus the `dumpType`/`dumpNativeType`/`dumpPhpDocType`/`debugScope` rules under PHPStan's own identifiers (`phpstan.*`), so the nsrt suite can also run through the rules path.
    - **Anonymous classes are declarations like any other** (41 rules-oracle misses named `class@anonymous`).
    - **E1's project-level checks** (duplicates, architecture, banned, class shapes, composer hygiene, `argument.parameterRenamedInSubtype` on by default) are the legacy's own: Phase 4. PHPStan's collectors (unused private members, trait-in-every-host via `ConstantConditionInTraitRule`, the "in context of class" merge) are M6.
    - **E4's coverage notes for 3b/Phase 4:** phpstan-doctrine's rules (`doctrine.dql`, `findByArgument`, entity checks) never existed; phpstan-phpunit's rules (`phpunit.covers*`, assertion simplifications, `mockMethod`, `dataProvider*`) never existed; strict-rules lacked `disallowedLooseComparison`, `disallowedBacktick`, `disallowedEmpty`, `disallowedShortTernary`, `noVariableVariables`, `closureUsesThis`. Each 3b extension is ported rule class by rule class against its current release, as the legacy compared them.
  - **EV-6. Stages F–G** (lines 5478–5534) are the condensed plan's F/G (FR-9, FR-10); nothing new.
  - **B6 SQL** is Phase 4 only. Its lessons: identifier case follows the driver; outer-join nullability; a failed connection is a startup warning, not silent.

### CRATE_SPLIT_AUDIT.md (190 lines, read in full)

An item-by-item audit of Stage A–B7 against the code at `867d9cb3`.

- **AU-1. Workspace conventions for Phase 1's skeleton:**
  - `[workspace.lints]` (`unreachable_pub`, `unsafe_code`, broken intra-doc links) with `[lints] workspace = true` in every member, and print macros denied in library crates;
  - `[workspace.package]` with `license`, edition 2024, `resolver = "3"`, `publish = false`;
  - shared dependencies only in `[workspace.dependencies]`;
  - mutually exclusive profiling features enforced with `compile_error!`;
  - the bin passes its version into library entry points rather than reading `CARGO_PKG_VERSION` there;
  - `.git-blame-ignore-revs` for moves.
- **AU-2. Gates that aren't automatic don't run.** The audit found `cargo xtask gates` never run in Stage B, the performance and build-time gates never recorded once, and `doc`/`hack`/`machete`/`deny` never run locally. **For the new repo: every gate the plan relies on runs in CI on every push** (fmt, clippy, tests, doc, the scoreboard subset, the cost gate, `cargo deny`/`machete`). A gate that only runs by hand isn't listed as a gate.
- **AU-3. Interning has to be session-scoped from the start.** About 115 call sites interned project names through a process-wide interner that is never reclaimed, which matters for the LSP. The new `syntax` crate has one interner owned by the session (or a deliberately global one for stub names only).
- **AU-4. Small facts:** a native `resource` hint is a class name, not the resource pseudo-type; `class_exists()` and friends guard `new X()` (PHPStan does this through its type-specifying extensions, covered by M5b).

### MAGO_ARCHITECTURE_COMPARISON.md (17,594 lines, read in full)

There are 314 ideas (K1–K314) in ten categories, each a fix or feature against the legacy's code, cited by legacy file and line. The new repo doesn't take them as a backlog. They split three ways:

- **MG-1. Parity facts the scoreboard already covers.** Most of §3.1 precision and part of §3.3 robustness: per-disjunct merges (K90), finally entry state (K104), by-reference sites creating their target (K105), named-argument-aware builtin handlers (K116), reachability computed during analysis instead of a syntactic `always_terminates` (K102), match-arm narrowing shared between the inference and check paths (K98), string offsets (K120), stale inherited PHPDoc (K237), static/global variables (K109), trait accesses to host private members (K272), case-insensitive extension dispatch (K185), and more. Each is a place where the legacy's own engine diverged from PHPStan. A port of PHPStan's handlers gets them by construction, and the nsrt and rule suites test them.
- **MG-2. Phase 4 titles.** Most of §3.2 new-check, plus parts of §3.6, §3.7 and §3.10:
  - checks: dead stores, unused imports, taint, metrics, persistent CNF clauses (K91), covariant generic merging in unions (K87), dropped native union members (K252), whole-program dead code in the LSP (K276), architecture rules over symbols (K181), restricted-symbol patterns (K183, K166);
  - output and policy: severity separate from level (K160), identifier filters and stats (K202), scoped and counted ignore pragmas (K167);
  - workflow: watch modes (K220, K222), explanations of mismatches (K125), did-you-mean on variables (K113);
  - distribution: musl and old-glibc builds (K301), a Composer launcher (K300), a container image (K304), attestations (K306), PGO (K307), Nix (K305);
  - **the out-of-process extension ABI (K193)**, Mago's way of running extensions written in PHP. It is the only route to the third-party PHPStan extensions that Phase 3b doesn't port by hand, so it belongs in Phase 4's ecosystem item.
- **MG-3. Design inputs for the new repo**, listed below by milestone (MG-4 to MG-12).

- **MG-4. The stub snapshot (revises P-5/M-4).** K50 argues against Mago's `build.rs` route: it makes the parser, reflection and types crates build-dependencies of the snapshot, so every edit to the reflection code reruns a full stub parse at compile time, which hurts most in a debug build. The legacy's own path was step by step:
  1. stop keeping two copies of the stubs;
  2. a build-time **name → stub file index** that needs no parser (`stub-sources/build.rs` already walks the files);
  3. a lazy parse of only the files a run touches;
  4. a serialized runtime cache keyed by binary identity, and only if the meter asks for it.

  That fits a lazy reflection design (PHPStan's own stubs are located by name and parsed on demand). **Revised M4: a build-time name index plus lazy, per-version parsing of stub files. A serialized snapshot is added only if the M4 cost workload shows the parse dominating.** The M4 row and P-5 are changed to match.
- **MG-5. Parser (M1), built in from the start:**
  - **byte offsets in spans, with a lazy line index** (K6); line/column is computed on demand. LSP UTF-16/UTF-8 position encodings convert at the boundary;
  - **`__halt_compiler` stops lexing** (K3): the data after it is never tokenized;
  - error recovery (K1): an error node types as neutral `mixed` and emits nothing. For parity, a file with a parse error reports only `phpstan.parse`, as PHPStan does; recovery serves the LSP and keeps a context file's declarations;
  - parse errors read like PHP's (`unexpected variable "$x", expecting ")"`, K2);
  - AST compactness from the first commit (K7, K10–K13): `memchr` scanning for comments, inline HTML and strings; boxed rare large variants; exact-size `Box<[T]>` lists; `const _: () = assert!(size_of::<Expr>() <= N)` guards; doc comments with a start offset;
  - the parser suite also takes nikic's error-recovery fixtures and a curated invalid set, and caches `php -l` verdicts by (blob hash, `php -v`) with an allowlist for version-gated syntax (K9, K184).
- **MG-6. Names (M1/M4).** Interned, case-folded symbol ids, with the display spelling kept apart (K35). Property and class-constant names stay exact-case, so the types keep `Folded` and exact names distinct. The legacy's split between a global and a per-project interner lost the `ptr_eq` fast path between stub names and project names (K20, K33). AU-3 asks for session scope. **Decision for M1:** a global table for stub names and a per-session table layered over it, so comparing ids is the same operation across both.
- **MG-7. Types (M3), built in:**
  - per-node flags computed when a node is interned (`HAS_TEMPLATE`, `HAS_LATE`, …), so `substitute`/`resolve_late` return template-free subtrees unchanged (K69);
  - probe types cached rather than re-interned per query (K70);
  - an n-ary union builder that normalizes once (K68);
  - the memo keyed by owned `Type`s with no per-entry allocation (K21);
  - property tests (K259): `a <: a|b`, `t.without(e) <: t`, narrowing completeness `t <: t.without(e) | (t & e)`, memo agreement. Each deliberate non-law is a named `prop_assume!` with its reason. These join F4's properties in M3's test suite;
  - a micro-benchmark workload for type algebra seeded from real type shapes (K261).
- **MG-8. Analyser (M5a), cost lessons:**
  - scope joins short-circuit on `ptr_eq` and visit only changed keys (K112);
  - scope keys are interned names, not `String`s (K22);
  - long `&&`/`||` chains narrow in linear time (K253);
  - nested loops: with `LOOP_SCOPE_ITERATIONS = 3`, cost grows as 3^depth. A deep-nesting file becomes an M5 cost workload (K254);
  - per-function state is one `BodyContext` swapped as a unit when entering a closure or arrow function (K277). Without it, the legacy leaked return points from contextual closures into the enclosing function.
- **MG-9. Calls (M5a/M5b).** PHPStan's `ArgumentsNormalizer` reorders named arguments before any extension sees them. Porting it first gives every `src/Type/Php` extension positional arguments for free; K116's ~100 raw-index sites were the legacy's cost of not having it. Function names dispatch case-insensitively at one boundary (K185).
- **MG-10. Command (M7), parity items Mago confirms:**
  - PHPStan's `fileExtensions` parameter (K226);
  - `--tmp-file`/`--instead-of` (K235): Infection's PHPStan integration uses them, so they are PHPStan parity;
  - `dump-parameters` and `diagnose` (K229);
  - the progress bar on stderr only for the table format (K210);
  - `--debug` and `-vvv` file and timing output (K208, K209);
  - `reportUnmatchedIgnoredErrors` and baseline staleness counted only over analysed files (K199, K201);
  - `github`/`gitlab` formats and checkstyle/junit dropping XML-forbidden characters (K196, K197);
  - one project-relative path identity for output, baselines and `ignoreErrors` (K198);
  - symlink loops walked once (K214, already in FR-1);
  - SIGPIPE restored, so a closed pipe exits 141 (K223, FR-9);
  - skip freeing the heap at exit with `mem::forget` once output is flushed (K215).
- **MG-11. Parallelism (M7/M8), built in rather than retrofitted:**
  - one worker pool for every phase, stub work included, with named threads (K206);
  - fine-grained work stealing (`with_max_len(1)`) or largest-first scheduling. The legacy's straggler had one thread at 48.8% of samples and seven idle (K205);
  - a thread count from the flag, `PHPSTAN_RS_THREADS` or config, respecting cgroups (K207);
  - the global allocator (glibc or mimalloc) chosen by the cost meter, not by default (K24).
- **MG-12. Testing:**
  - **differential incremental-vs-fresh tests** for the result cache in M7 and the LSP in Phase 4 (K219, K263): after each edit step, the whole-project result equals a fresh load. The legacy's five stale-result bugs would each have failed one;
  - the `testing` crate's harness: a multiplicity-aware diff, orphan-fixture detection, and a bless that can never accept a failing `assertType` (K264, K265);
  - a checker-level fuzz target seeded from the test files, run at milestone ends (K256);
  - a robustness smoke run over a few vendor projects that fails only on panics, parse skips, timeouts or memory caps, never on findings (K258);
  - an optional CI gate: `cargo check --target wasm32-unknown-unknown` on the pure crates (`syntax`, `phpdoc`, `types`) keeps them free of I/O and C dependencies by construction (K267);
  - name-case fixtures (K38): every symbol kind at every reference site in every spelling, with the negative cases where PHP is case-sensitive. These are M4 tests checked once against `php`.

  §4 sequencing orders legacy fixes and doesn't carry over. §5's twelve refuted ideas are M-9.

- **§2's architecture table** compares Mago and the legacy over 29 aspects. What it says for the port:
  - **M-1. Parser:** the legacy lexed whole files into owned tokens, stopped at the first error and dropped the file. It took 7.55 s against Mago's 0.62 s on a 7.1 MB file, with 35–40% of parse time in malloc. The port has error recovery from day one; nikic recovers too (`ErrorHandler\Collecting`), so its recovery tests join the parser suite. The tree and token shapes are LED-3 and D20-1.
  - **M-2. Names:** the legacy's `Arc` names did about 52 linear case scans. Mago uses a 16-byte `Copy` interned word with a precomputed hash and lowercase identity. The port interns case-folded ids from the start (refined by MG-6 and L9-8).
  - **M-3. Codebase model:** Mago splits a scanner (each file's own facts) from a populator (derived maps, in topological order). It resolves duplicates in a total order and invalidates by signature hash plus reference graph. The legacy re-ran about 15 whole-registry passes on every edit. This maps onto PHPStan's reflection layer, and onto the M7 and LSP invalidation design.
  - **M-4. Stubs:** Mago decodes a bincode snapshot built at compile time (0.07 s, 96 MB). The legacy re-parsed 2,515 stub files per process (0.95 s, 150 MB). The port uses MG-4's lazy name index instead, with a snapshot only if the meter asks.
  - **M-5. Calls:** Mago has one invocation pipeline plus a provider registry keyed by name. The legacy spread call handling over several functions, with a 72-arm builtin match. PHPStan has Mago's shape, through `Dynamic*ReturnTypeExtension` registered in DI, and the port mirrors it.
  - **M-6. Rule catalog:** the legacy had about 650 identifier literals, no registry and about 93 settings booleans. The port has a central rule and identifier registry, taken from PHPStan's `Rule` classes and `conf/config.level*.neon`.
  - **M-7. What the legacy did better than Mago,** worth keeping:
    - O(1) scope forks (imbl);
    - hash-consed types with memoized operations;
    - overflow-correct arithmetic;
    - per-file panic isolation;
    - structure-aware fuzzers;
    - full-diagnostic fixture expectations;
    - the LSP.
  - **M-8. Process items,** each taken up elsewhere in the plan:
    - exit codes (the legacy always exited 0);
    - located parse errors;
    - CI on three OSes;
    - instruction-count benches (K260);
    - incremental-vs-full differential tests (K219);
    - extensions as features or separate crates: the `sql` crate alone pulled in 162 of 214 dependencies.
  - **M-9. §5's twelve refuted ideas are not re-proposed.** Among them: a per-file token table, refined strings stored as flags, and flattened concat chains.

### PROFILE_DRILL_PIWIGO_25G.md (1,703 lines, read in full)

- **P-1. Where the instructions went.** On Piwigo (about 14k files parsed, 2.8k checked) the run started at 25.1 G and was 20.6 G after the 25 G work.
  - By phase: parsing and indexing about 43%, checking 41%, finishing passes 10%, the serial main thread 4%.
  - Cross-cutting: the allocator about 2.4 G (22.6 M allocations), `memcpy` 0.8 G, formatting about 1 G.
- **P-2. Four cross-cutting causes** (§2), each with the port's answer:

  | cause | the port's answer |
  |---|---|
  | names carried as `String`, re-lowercased and re-hashed at every hop | interned ids |
  | work for files that are never checked: 80% of parsed files are context, and a run reads only 0.5–0.6% of their function bodies | lazy reflection |
  | answers computed twice: termination, return types re-inferring arguments, PHPDoc printed and re-parsed | `StatementResult`/`ExpressionResult`, one PHPDoc lowering |
  | per-node overhead in the front end | the flat tree |

- **P-8. Process.** The designs went through seven audit rounds (§10.3–§10.9), each finding 5–15 more defects, and produced several thousand lines of design per few gigainstructions saved. Design small, build, let the meter judge.

Part I is a callgrind drill-down of the legacy's 25.14 G Piwigo run, with 26 proposals. Part II designs each of them against the legacy's code, through seven audit rounds (§10.3–§10.9). Part I's summary had already given P-1 to P-3 and P-6 to P-8. What the full read adds:

- **D25-1. Estimates ran about 2× optimistic.** §8 summed the proposals to ≈9.5 G net (25.1 → about 15.5–16 G). What got built reached 20.57 G (status line), about 4.6 G, and several items were never built: the text-grammar lowering (P7), the single traversal (§12.10), and others. **For the new repo:** a design's predicted saving is a hypothesis. The cost meter's per-commit delta is the number that counts, and `STATUS.md` records measured deltas only.
- **D25-2. PHP grammar facts for M1's tests**, each checked by the legacy with `php -l` and nikic/php-parser 5:
  - **an arrow function's body, `throw`, and `include`/`require` take their operand at the lowest level**: `fn() => $a or $b` is `fn() => ($a or $b)`, and `require $f or die()` requires `"1"`;
  - `$a instanceof B instanceof C` is valid and left-associative;
  - the equality and relational levels are non-associative: `1 < 2 < 3` is a parse error, but mixing the two levels is allowed;
  - an unparenthesized nested `?:` with a full ternary is a compile error, while an all-short `?:` chain is valid and left-associative;
  - a bare `yield` is allowed wherever the next token cannot begin an expression (`yield and $x`);
  - `goto` may jump into an `if`, `else`, `try` or plain block, never into a loop, `switch` or `finally`;
  - a top-level class or function after an early top-level `return` is still declared (hoisting); one nested in an `if` is not;
  - a `use` import applies only to code after it, and a conflicting later import is a compile error.

  §11.6's binding-power table (Zend's own order, loosest first) is M1's precedence reference. The **generated operator-pair test** goes into the parser suite: every ordered pair of operators and prefix forms, nested two deep, with and without parentheses, compared against nikic's tree and `php -l`.
- **D25-3. Front-end shape (M1), beyond MG-5:**
  - validate UTF-8 once at the read, then slice `&str` in the lexer;
  - build single-quoted literals with one allocation;
  - keywords through a perfect hash compared case-insensitively in place;
  - one byte-class dispatch per token, with rare forms in `#[cold]` functions;
  - ~~a lexer that streams tokens with a small lookahead ring~~ **superseded by D20-1**: streaming was built and measured as a +1.77 G loss. The lexer fills flat token arrays;
  - **the lexer records `@phpstan-ignore` comments** with their neighbourhood resolved as it goes (code before on the line, the next code line), so no comment list or line-start table is built;
  - **the parser records** names resolved in source order (PHP's order; each distinct import state published once), references, the declaration list, anonymous-class numbering, `goto` labels with the backward ones marked, and a function's generator flag. nikic resolves names in a separate visitor; the parser suite compares against that visitor's output, so doing it in the parser is a free choice;
  - doing these from the start removes the legacy's four post-parse walks (≈0.55–0.6 G) and its dead `annotate` pass (0.43 G).
- **D25-4. PHPDoc and types (M2/M3):**
  - one PHPDoc grammar, lowered straight to `Type` (no print-and-reparse round trip);
  - tags memoized by `(tag, rest)` text, because the same tag lines recur across thousands of docblocks;
  - **resolution memoized per resolution context, where the context covers namespace, imports, template names, type aliases and `self`.** A first design keyed it by namespace and imports only and would have returned wrong types;
  - `union_of(&[Type])` as the primitive, over a member index, keeping first-occurrence order (the legacy's left fold of a binary union was O(n³));
  - interning from borrowed parts, building the owned `Repr` only on a miss;
  - flags `HAS_CONDITIONAL`, `HAS_OBJECT` and `HAS_PARAM_CONDITIONAL`, so transforms return early. A template-free call return type skips all binding work (≈79% of calls);
  - cached bits for structural predicates;
  - a conditional type's parameter subject bound by position.
- **D25-5. Analyser (M5a), confirmed against PHPStan 2.2.16's source:**
  - **`goto` is modelled.** `GotoHandler` returns an always-terminating result with an exit point; `mergeForwardGotoExitPoints` merges at the label; `resolveBackwardGotoScope` runs a backward jump to a fixpoint. The legacy's audits wrongly assumed PHPStan ignores `goto`, until the fourth round read the source. That is the F12 rule again: read the source, never recall;
  - a statement result carries completion, exit and throw points, impure points and `hasYield`, which removes the legacy's nine syntactic side walks;
  - rules see the scope before their node's effects and get shared references only, so the type system enforces "rules can't move the scope";
  - one inference engine, where a hypothetical question runs the same processing on a cloned scope;
  - node types are stored per node id per pass (`ExpressionResultStorage`);
  - **scope keys are PHP's identities**: a static property under its resolved class, an array index canonical as PHP stores keys (`'1'` ≡ `1`), and every fact about a variable purged when it is rebound (the legacy kept `method_exists` guards across a rebind);
  - joins and convergence use persistent ordered maps, whose `diff` skips the nodes two forks share.
- **D25-6. Rules (M6):**
  - message parts are `Copy` values that implement `Display` and are formatted only when an error is reported. The legacy formatted hundreds of thousands of labels for 32,581 diagnostics;
  - rules are registered per node kind, and an opt-in rule that is off is simply not registered;
  - a rule reads the resolution its call node recorded and never resolves the callee again.
- **D25-7. Laziness and whole-program facts (M4 and Phase 4).** The legacy found that limiting its finishing passes to what the analysed files reach is not exact for its own whole-program facts: property writes unioned over every writer, global-variable types over every writing file, transitive purity. Readers don't name the writers, so a forward closure misses them, which is why lazy-vendor was reverted. **PHPStan computes no such facts, so lazy reflection is exact for parity.** Every whole-program inference in Phase 4 has to come with its own reverse-dependency design and can't assume eager loading.

  Smaller M4/M7 notes:
  - Composer's generated autoload files are read by the one parser, off the main thread;
  - a directory's canonical path is its parent's plus its name, calling `readlink` only where the listing shows a link;
  - directories are walked in sorted order, so file lists need no sort;
  - JSON output is streamed;
  - per-file path and package-owner work happens on the workers.

  One open M4 question for the source read: how does PHPStan treat a vendor polyfill of a built-in class behind a `PHP_VERSION_ID` guard (symfony/polyfill-php80 and the like)? The legacy evaluated the guard against the configured version range (F2). The port follows whatever PHPStan's locator chain and stub precedence do.

### PROFILE_DRILL_PIWIGO_20G.md (2,002 lines, read in full)

Part I profiles the legacy at 20.57 G (callgrind) and on the heap (dhat: 866 MB peak, 657 MB at exit, 19.4 M allocations, every allocation site assigned to a fix). Part II is a 20-item plan against the legacy's code, audited eleven times and then reviewed adversarially item by item; it was never built. Most items fix legacy-specific structure. What carries over:

- **D20-1. The lexer fills flat token arrays; it does not stream (corrects D25-3).** The 25 G report's streaming design was built as written. It cost 257 Ir per token in plumbing against 48 for vectors (+1.77 G), because it put a function boundary and a 64-byte move between lexer and parser on every token, while the parser's lookahead never needed streaming. The design counted what the vectors cost and not what passing tokens through the lookahead cost. **M1:** one lexing loop into a struct of arrays (`kinds: Vec<u8-sized kind>`, `spans`, `payloads` only for tokens that carry one, `docs` as (token index, span)), reused per worker thread and cleared between files. `peek_at(n)` is an index. This is also the textbook case for the cost meter: the M1 workload would have shown it on the commit.
- **D20-2. AST memory shape (M1), measured:**
  - a tree was 9× its source (173 MB for 19 MB). Every node had its type's full size (`Expr` 80 B, `Stmt` 160 B, `Param` 200 B), every statement carried 24 B of doc fields though few have a doc, and lists grown by `push` held 53 MB (31%) of unused capacity;
  - **the design:** a per-file arena; `Expr<'a>`/`Stmt<'a>` as two-word handles to variant-sized nodes (oxc's shape); children gathered in reusable scratch stacks and moved into the arena once as exact-size slices; payloads borrowing the source (doc comments, literals without escapes, inline HTML); docs found by offset in the file's doc sites, not stored on statements; parse errors boxed so `Result` stays small; `Expr::peeled`-style unwrapping by loop, not recursion;
  - **an open M1 design question:** the legacy plan needed one `unsafe impl Sync` for a frozen `bumpalo` arena to read trees from other threads. The new repo forbids `unsafe` outside one FFI crate (B-4), so M1 picks an arena that is `Sync` once frozen without our own `unsafe` (an index-based arena of typed vectors, as rust-analyzer and Zig do, is one), or keeps each tree on its parsing thread. Decided in `docs/design/syntax.md`, measured by the M1 workload.
- **D20-3. Lines (M1/M7):** two types. `LineStarts` for every file, built by one block pass that also says whether the file is ASCII; `LineIndex` (with column tables) only for analysed files. Nothing in the front end reads lines: ignore marks, member removal spans and doc positions record offsets, resolved through the index when reported. Column tables for every file cost +0.27 G in the legacy.
- **D20-4. Where trees die (M5a/M7):**
  - the peak was every file's tree alive at once, because the load collected all parse results before a serial insert loop; dropping context trees in that loop also cost 575 M of cross-thread frees (`mi_free` per call 23 → 42.5 Ir);
  - **rules:** whether a tree is kept is a function of (role, open document) and known before parsing; a tree not kept is dropped on the thread that built it; a batch run frees an analysed file's tree when its check ends, checking largest files first; project-wide passes read per-file facts recorded during the check, never trees. In PHPStan's architecture that last one is the `Collector` → `CollectedDataNode` mechanism, so M6's collectors are the design, not a retrofit;
  - a reader of a dropped tree must re-parse (`ast_of`), never get an empty file.
- **D20-5. Reflection storage (M4), lessons from the legacy's registry:**
  - declarations never change after they are built; anything a later pass computes (ancestors, member owners, inherited docs, purity) is a table or a lazy memo by id, not a write into the shared declaration (the legacy's writes cloned 47 K functions and then compared them field by field);
  - a file's declarations stored together in a few vectors (rust-analyzer's `ItemTree`), member facts as flag bits;
  - ids carry a generation (slotmap-style), so nothing held across an edit can name a different declaration. Needed for the result cache (M7) and the LSP;
  - **a parity fact:** a trait method is bound to each using class (`$this`, `self`, `static`, `__CLASS__`), so per-method results are keyed by (using class, member), never by the trait alone. PHPStan's `PhpMethodReflection` has both `declaringClass` and `declaringTrait`;
  - registry queries iterate precomputed ancestor slices; none walks the hierarchy with a fresh stack and seen set per call.
- **D20-6. The relation memo (M3/M4):** keyed by (supertype, subtype) under a **content generation** of everything the relation reads through the class-hierarchy oracle (classes, members, signatures, variances, template bindings), not just extends/implements. An answer computed while a cycle guard is open is provisional and not stored. Bounded per thread with two segments so recent answers survive a flush. A runtime switch turns it off so "memo on = memo off" is a test.
- **D20-7. Cache files (M4 snapshot if built, M7 result cache):**
  - **no `mmap`:** it needs `unsafe` and is undefined behaviour if the file is truncated or replaced (SIGBUS in a long-running server, NFS);
  - lazy records read with `read_at`/`seek_read` into scratch, decoded into their own allocation, each with its own CRC; a bad record is logged and rebuilt, never a panic;
  - names stored once per file in a table and referenced by index;
  - a test that stores, prunes and reloads a cache while a reader holds the old file, on all three OSes.
- **D20-8. File identity and one load path (Phase 1 testing crate, M7):**
  - a file is its resolved path, as PHP's `include_once` and `__FILE__` see it; each link is resolved once; a link cycle ends where a directory's (device, inode), or volume serial and file index on Windows, equals an ancestor's; hard links and bind mounts stay distinct; a file's role is the strongest configured root containing its resolved path;
  - **the CLI, the tests, the benches and later the LSP load through one code path.** The legacy had three (CLI, server, tests), so its unit tests never exercised the load that its optimizations changed, and its bench timed a load nothing shipped. The `testing` crate's harness drives the real session with an in-memory overlay;
  - the directory walk takes entry kinds from the dirent (no `stat` per entry: 16,773 in the legacy), sorts each directory's entries by name bytes so the file order needs no global sort, lists each directory once, and starts parsing as directories are listed. The serial walk was ≈9% of a warm run's wall time with every worker idle.
- **D20-9. Verification patterns (Phase 2 and every milestone):**
  - canonical dump printers destructure every struct without `..` and match without `_`, so a field can't silently drop out of a comparison; dumps show no addresses, ids or hash order;
  - equalities that need no oracle, run in CI: one thread vs the default thread count (byte-identical), cache cold vs warm vs off, memo on vs off, incremental edit replay vs fresh load (MG-12);
  - **no second implementation kept alive under `cfg(debug_assertions)` to compare against.** The legacy did this for about ten migrations; the old code then ran in every test and kept extra data alive. Compare against a dump from the previous commit instead.
- **D20-10. The analyser and rules (M5a/M6), cost rules that are also clean design:**
  - a call's argument-to-parameter binding is made once and kept in the call's result; every rule borrows it (the legacy rebound the same call up to 4×, ≈590 K allocations). This is PHPStan's `ParametersAcceptorSelector` result per call;
  - rule messages are values formatted only when reported; identifiers are `&'static str`;
  - constant types (`int|float`, `T|null`) are interned once, not rebuilt per call;
  - per-function checker state is cleared, not reallocated; one checker per worker;
  - throw points and loop exits hold persistent-scope pointers, never scope clones (the legacy cloned the whole scope at every throw point);
  - per-text analyses (regex groups, `sprintf` placeholders, preg shapes) are memoized per distinct text;
  - variables as per-body slots, keeping a name lookup only in bodies PHP can reach by computed name or shared scope (`$$x`, `extract`, `compact`, `get_defined_vars`, `parse_str`, `$GLOBALS`, `include`/`eval`); a candidate for M8, measured, not a day-one requirement since PHPStan keys by expression.
- **D20-11. Interning (M1/M3):** a name is one allocation (a header with both hashes, the length, the last segment's offset for short names, and the folded form inline after the text); interner indexes are `HashTable<(hash, Weak)>`, no `Vec` per hash; shape string keys are interned too; type memos clear with the relation memo's generation so a long LSP session doesn't keep dead hierarchies.
- **D20-12. Estimates, again.** The plan's own review had already found three of its first five items overstated before it stopped, and it projected 20.57 → ≈12.6 G and 19.4 M → ≈1.35 M allocations without building anything. With D25-1 that is two drills whose designs predicted twice what they delivered. **Reinforces the measured-deltas-only rule and the cost meter's per-commit gate**, and P-8's "design small, build, measure".
- **D20-13. Out of scope for the port:** stub availability decided by run-time evidence (built-in extensions, Composer's `platform_check.php`, the configured runtime's `php -m`) and a new check that a polyfill's signature matches the native one are beyond PHPStan: Phase 4 titles. The SQL/DQL/query-builder costs are Phase 4. The warm-run "resolved context base" is an M8 candidate if the meter shows warm runs dominated by re-resolving vendor code, which lazy reflection should already avoid.

- **P-3. Serial stretches set the wall time.** The finishing passes (about 15 whole-registry passes) were 37–40% of wall time but only 12–18% of instructions. A lazy per-class memo (FR-4) removes them instead of parallelizing them.
- **P-4. Peak memory:** all 14,251 trees were alive at once, 569 MB of an 867 MB peak. Trees are dropped after their check (D20-4).
- **P-6. Designs that lost when built:**
  - the streaming lexer interface: +1.77 G of plumbing to save about 0.33 G (D20-1);
  - column tables for every file: +0.27 G;
  - "speeds up cold runs too": the parse job paid +0.78 G.

  The front end delivered 1.05 G of the 3.8 G it was designed to save. A per-commit meter flags each of these on its own commit.
- **P-7. Measurement traps** for `LESSONS.md` and the meter:
  - a codegen change (16 codegen units vs 1, frame pointers) moves totals by about ±1 G, so the meter keeps one fixed profile;
  - callgrind serializes threads, so serial windows come from `perf`;
  - dhat charges a realloc as a block to the first allocating site;
  - ten frames are too few for per-phase questions, which need the patched dhat's whole stacks.

### PROFILE_DRILL_PIWIGO_OFF.md (1,299 lines, read in full)

- **P-5. The stub build** costs 5.05 G and 0.37 s per process without the cache, and the cache is keyed by build hash, so every new build's first run is cold. The port answers this with MG-4 and OFF-1's lazy index, not with compile-time stub data.
- **P-9. Bugs the drills found that the new parser and loader must not repeat:**
  - E14: `$x instanceof static::$name` was refused, losing six vendor files;
  - E1: a cold stub build could deadlock (OFF-3);
  - E15: the cache key missed the stubs directory (OFF-4);
  - the parser had never been run against `php -l` over all 111,836 vendor files. The parser suite includes that differential.

A drill of the run with no caches (25.63 G): the stub build is the whole 5.05 G difference from the warm run (0.37 s of 2.68 s wall), with 11 proposals, 13 cuts and three defects, every one investigated with probes. What carries over:

- **OFF-1. Lazy stubs, measured (M4; refines MG-4).** Over ten real projects, closing each project's referenced names over the stub index reaches 3–28% of the 16.3 MB of stub source from the analysed files alone, and 6–43% with context (up to 57% at the upper bound). Bytes run ahead of classes because a phpstorm-stubs file holds a whole extension: one `strlen()` parses `Core/Core.php`. At ≈0.21 G per MB that is ≈0.2–1.5 G per cold run against 5.05 G eager. **This confirms MG-4: a name index plus lazy per-file parsing, with no embedded snapshot unless the meter asks.**
- **OFF-2. Version gates as data, one parse per stub file (M4; revises EV-2's "the layer parses per version").** 90 phpstorm-stubs files carry gates (712 `PhpStormStubsElementAvailable`, 2,492 `LanguageLevelTypeAware`), with 2–10 distinct readings each over PHP 5.3–8.5. Parsing per version would mean 2.7× the source. Keeping each gate as a condition on the declaration, resolved against the configured `PhpVersions` range, keeps one parse per file and answers a range with every reading in it. **M4 checks how PHPStan's own `PhpStormStubsSourceStubber` applies the attributes against a version range and follows that.** The stub index records each declaration's gate.
- **OFF-3. Lazy memos must never run parallel work under their lock (M4/M5).** The legacy's E1: a `LazyLock`/`OnceLock` whose first build ran a `par_iter` or `install` could deadlock, because a rayon worker that waits steals sibling jobs, which can force the same lock on the same stack. The legacy found it by reading rayon's source and then reproduced it. **Rules for the new repo:**
  - nothing parallel runs while a lock that a pool job can wait on is held;
  - process-wide state (stubs, the name table) is a value owned by the entry point and handed down, never a hidden static that any depth can force;
  - FR-4's per-class `OnceLock` memo levels compute serially within one class, and a re-entrant force is a reported cycle, not a hang;
  - every parallel job runs on the one pool (the legacy's stub build spun up rayon's global pool by accident, ignoring `PHPSTAN_RS_THREADS` and the stack size; MG-11).

  A regression test with a one-thread pool that forces the same memo from two jobs goes into M4's suite.
- **OFF-4. Every cache key is an enumerated input list with a test (M4/M7).** The legacy's stub-cache key missed `vendor/phpstan-src/stubs`, so a stub edit ran on a stale cache with no error (E15). A narrower key "over what the cache depends on" was rejected because a missed input fails silently. **Rule:** each cache key is built from an explicit list of inputs, and a test asserts that the list covers every path the build scripts read. A cache derived from one crate (a parse-verdict cache over `syntax`) is the safe narrow case.
- **OFF-5. Cache writing (M7):**
  - a store checks once whether it can write, and skips building and encoding the entry otherwise; a read-only cache (restored in CI) is still read; a failed write is reported once, never silently;
  - one file per entry cost ≈0.5 s of kernel CPU for 11,459 entries (`openat`, `write`, `rename`, plus `mkdir`/`statx` per entry for shard directories). The M7 result cache packs entries into few files, and makes its directories once.
- **OFF-6. The parser against real code (M1), for the parser suite's inputs:**
  - `php -l` over 111,836 files under `vendor/` of 18 trees: 272 fail (13 parse errors, 259 compile errors); 72 of them are phpstorm-stubs files themselves (redeclared built-ins, version-gated duplicates, `function exit`), so the stub loader must tolerate files PHP refuses;
  - the legacy parser refused valid code PHP accepts (`$x instanceof static::$name`, `self::$prop` likewise; six vendor files lost every declaration), refused PHP 7's `$s{0}`, and accepted `(real)`/`(unset)` casts PHP 8 refuses. Each becomes a parser test against nikic with the right version setting;
  - **the C7 corpus:** one trigger file for each of php-src 8.5.4's 268 compile-error sites (`~/.cache/prs-probe/c7/cases/`, 299 files). The legacy parser rejected 30 and accepted 238. For parity the parser follows nikic: a site nikic accepts is `ACCEPTS-COMPILE-ERROR` (informational, F11), and PHPStan's rules report some of them. The corpus is copied into `parity/` as a parser-suite input and as a rule-suite cross-check;
  - **parse failure is PHPStan's behaviour for parity:** PHPStan drops a file whose parse fails (`FileNodesFetcher.php:32-36`) and reports `phpstan.parse` for an analysed one. Psalm-style recovery of what parses (D6) serves the LSP in Phase 4.
- **OFF-7. Skimming context function bodies is an M8 candidate.** 48% of the tokens the legacy lexed and parsed were context files' function bodies, and a run read 0.5–0.6% of them. With lazy reflection (OFF-1, P-2) most context files aren't parsed at all; for those that are, a skimmed body (stored as a byte range, parsed on first read) saves most of the rest. Conditions the legacy worked out, if M8 builds it: a skimmed body is still syntax-checked (PHP refuses the whole file for one bad body), the check's verdict is cached per (content hash, `syntax` crate hash), the body's bytes are kept from the read so a later parse can't see a changed file, and the skimmer flags bodies holding nested declarations, `define`, `global`, `include` or a `PHPStan` name for a full parse.
- **OFF-8. Determinism across targets (M3).** Constant folding of `**` called the platform's `pow`, whose precision Rust documents as platform-dependent, and two release legs cross-compile. **M3 folds float powers with a pure-Rust implementation** (a `libm`-style port), so a type's value can't depend on the build machine. The rest of folding is exact under IEEE 754.
- **OFF-9. Small design facts worth building in (M3/M6):**
  - a kind code in the type node's spare flag bits, so the relation's shortcuts read one word (the shortcuts answered 34% of 1.34 M `isSuperTypeOf` calls);
  - one node against itself answered `Yes` first (13.8% of calls);
  - fixed object types (`Throwable`, a class's plain object type) and small integer literals (−1..=255, 92% of literals) built once;
  - a message is a template plus operands formatted when output, so messages filtered by level or ignored never format (the legacy formatted ≈31 K messages a truncation then dropped);
  - an empty `dynamicConstantNames` makes class-constant reads take the declared type with no key built.
- **OFF-10. Measurement method, for `LESSONS.md` and the cost meter's reader:**
  - callgrind reuses thread slots, so a thread's role is read from its functions, not its number;
  - callgrind has no calling context: a function two roots call is split by edge shares, and inclusive costs are quoted for one callee and never summed;
  - perf at a fixed rate under-counts busy cores by about a quarter; wall time comes from interleaved round-robin runs, medians of 8, with the load noted;
  - strace `-f` serializes threads, so its call counts are exact and its times aren't.

  The proposals for the eager stub base (1–5), the purity search (9) and the per-file-cache cuts (E4, E11, E12) are legacy-structure fixes. The parity port has no eager base and computes no transitive purity (D25-7).

### PROFILE_DRILL_PIWIGO_OFF_HEAP.md (499 lines, read in full)

The dhat counterpart of OFF, with whole stacks (off, cold and warm runs). It confirms the 20 G heap findings (M1–M11) and maps every allocation site, including the stub build's 5.8 M blocks. What it adds:

- **OH-1. The heap meter's tooling (Phase 2).** With the patched dhat (frame pointers walked outside dhat's lock, return addresses resolved once at exit; legacy `patches/dhat`) a whole-stack Piwigo run takes 3½–4 minutes, against ≈9 h projected with upstream dhat-rs. Ten frames were enough to map each allocation to its site, but 63% of blocks then reached no phase root, so per-phase questions need whole stacks. **For the cost meter:** per-commit heap runs use small workloads with ten-frame stacks; the Piwigo whole-stack profile runs at milestone ends only.
- **OH-2. Process-wide memos must be scoped and weighed (M2/M4).** Memos filled while building stub data kept entries to exit (the PHPDoc tag memo held 58 MB in the off run against 25 MB warm), because their bound counted entries and key bytes, not values, and nothing tied them to a phase or session. The new repo's memos belong to a session or a stub layer, and are bounded by the total weight of keys plus values (D20 item 14's design).
- **OH-3. Write a cache file from one buffer (M7).** The legacy's container writer built the body, then copied it with the header into a second `Vec`, so both were live at the peak (+15 MB). The writer reserves the header in the body and patches it at the end.
- **OH-4. Two small M6 rules:** `RuleLevelHelper`'s per-call member split allocated 96 K vectors on Piwigo, so `findTypeToCheck` iterates members without collecting; and a signature mapped for a generic call was rebuilt whole at each use (44 K), so substitution is per parameter or return type read (D20-10).

### PROFILE_DRILL_PIWIGO_LEDGER.md (2,244 lines, read in full)

The merged ledger of the off drill, the 20 G plan, the 25 G designs and the crate split's C4/C5/C10 sub-tasks: about 200 rows over 16 areas (SYN, DOC, TYPE, NAME, LOAD, REG, STUB, CACHE, CHECK, PROD, VAL, MEAS, Q, COMP, SRV/FIX), eighteen architectural decisions (§2.4) and four id-mapping appendices. What carries over:

- **LED-1. The ledger's shape is the process failure in one file.** Rows are renumbered by area with need-chains between them, then re-renumbered when sources merge (Appendix A maps four generations of ids: OFF-E/P, 20G-n, DEC-n, CRATE-1.C4.xx). Several rows were "rewritten" or "superseded" by later decisions on the same day. It confirms L-3 and P-8: the new repo has no task ids beyond `STATUS.md`'s next five, and the failing test is the task.
- **LED-2. Decision 3: one incremental query engine (salsa 0.28.5) over the whole pipeline, with persistence.** REG-16.1 measured it against the legacy's tables: a salsa query per lookup 374 Ir, a plain table read 36, the legacy's tables 73–300; one query per name per file, then a per-file table, came to 67 Ir. **For the new repo:** whether reflection (M4), the result cache (M7) and the LSP (Phase 4) sit on one query engine is decided in M4's design doc, before reflection is written, measured by the M4 workload. If it is salsa, lookups go through per-file tables, never a query per name lookup. Retrofitting an engine later is what the legacy's REG-13 → REG-16 churn was.
- **LED-3. Decision 4: a flat, index-based, immutable tree** (one vector per node kind, `u32` ids, as Zig, Carbon and la-arena do). It is `Send + Sync` with no `unsafe`, which **answers D20-2's open question**: M1's tree is this shape. The legacy measured 2,612 `ExprKind::`/`StmtKind::` match sites over 135 files as the cost of changing a tree shape late; another reason to settle it in M1.
- **LED-4. Decision 7: builtin knowledge is embedded; PHPStan's own classes are read from the project's installed phar** (its `src/` is stored uncompressed: 7,127 entries, 1,959 under `src/`), as a locator, so code that extends PHPStan (custom rules, extensions) analyses with PHPStan's classes known. **M4 parity fact:** PHPStan analyses extension code against its own classes; the locator chain includes the phar's (or `vendor/phpstan/phpstan`'s) sources.
- **LED-5. PHPStan source facts the ledger checked (M4):**
  - builtin layers nest through `decideType`: php-8-stubs' PHPDoc, then the function map merged by `Php8SignatureMapProvider::mergeSignatures`, then the `.stub` PHPDoc (`NativeFunctionReflectionProvider:163/179`);
  - `AggregateSourceLocator` takes the first locator that finds a name, and analysed files come before internals (`BetterReflectionSourceLocatorFactory::create :146-214`);
  - PHPStan 2.3.1's `MIN_PHP_VERSION` is 7.1 (decisions 15–18 probed PHP 5.6–8.0 images and the installed packages' requirements);
  - `withNativeType` for late-resolving members (2.3.1).

  These are re-read at the pinned submodule version when M4 starts; the ledger's line numbers are for 2.3.1.
- **LED-6. Measurement facts for the cost meter (Phase 2):**
  - **MEAS-3: Ir is deterministic only at one thread.** Run-to-run spread was 16.9 M at the default thread count and 1.1 M at `PHPSTAN_RS_THREADS=1`. The cost meter counts Ir with one worker thread; a parallel workload is a separate, informational series;
  - wall time is judged by paired alternating A/B runs and a Wilcoxon test (the legacy's `scripts/abab.py`), not hyperfine rounds: the second command of each round ran slower whichever binary it was;
  - instructions are not time: the allocator runs at 1–5 G Ir per CPU-second, byte loops at 10+ G, so a cut in allocations is worth more wall time than its Ir share;
  - mimalloc: `purge_delay=-1` for batch runs (−5.4% wall); transparent huge pages must stay on; trees dropped on the worker that built them reuse its memory and avoid page zeroing (−0.22 s). These are M8 inputs, measured afresh.
- **LED-7. The runtime-truth decisions (1, 8–18) are Phase 4 titles.** VAL-2.12, 2.16 and 2.17 (certainty, `&&` definedness, by-reference capture) show the legacy hand-designing behaviour PHPStan already models in `MutatingScope`. That is the drift of W-2 again; the port takes PHPStan's model.
- **LED-8. "Byte-identical output plus approved deltas" was the legacy's validation mode** (VAL-1, decision 14's approvals, M5d rulings). It made every change a judgment call recorded in a ledger. The scoreboard replaces it: a change is right when tests pass that failed before.
- **LED-9. Parser and checker facts the runs settled:**
  - Q-1: after E14 the legacy parser had zero false refusals over 111,974 vendor files. That is the bar for M1's `php -l` differential;
  - Q-3: of 247 compile errors the legacy parser accepted, its checker reported 120. In the port, which of them PHPStan's rules report is the rules suite's answer (OFF-6's C7 corpus);
  - FIX-20: one recursion guard at every recursive function, restoring the limit after a panic inside a grown segment; 100,000 levels parse and walk on a 256 KiB thread. That is M1's deep-nesting test (FR-9's `stacker::maybe_grow`);
  - FIX-77: a context file that fails to parse is recorded, not silently skipped. For parity PHPStan drops it (OFF-6); an LSP notice is Phase 4.
- **LED-10. Tests the ledger names that carry over:** `stub_base_reentry` (OFF-3's deadlock regression), the closures proptest, the differential path-bytes tests (D20-8), and the four FIX-69 fixtures (a `catch` reached only through `ArrayAccess`, string conversion, `__get` or `include`), which are PHPStan throw-point behaviour checked by the rules suite.
- **LED-11. L-1's baseline, corrected by the ledger's end:** COMP-6 (bundled `vendor/` as context) added +2.1 G and FIX-20 +0.67 G, both accepted for correctness. M8's comparison with the legacy's Piwigo Ir is therefore against `legacy-final`'s measured row, run again with the meter's single-thread setting, not against the 20.29 G low.

- **L-1. The legacy's last measured numbers,** the baseline for M8's comparison.
  - In off mode on Piwigo it went from 25.63 G to a low of 20.29 G at `3dcb1192`.
  - It then rose to 23.07 G at `fa8e001b7`, mostly from changes accepted for correctness: COMP-6 +2.1 G and FIX-20 +0.67 G (LED-11).
  - The last three rows note "output differs from the oracle".
  - Warm wall time is 1.2–1.3 s.

  M8 compares against `legacy-final`, run again through the meter's single-thread workloads.
- **L-2. A per-row Ir measurement already worked, by hand.** Every row records its Ir and warm wall time, with a p-value. The meter automates this, and the 2.1 G jump in one row shows why every commit needs it.
- **L-3. Process.** About 200 task ids over 16 areas, in the order of four source plans, with need-chains between them. This is the "lost track".

### STAGE_B_REVIEW.md (1,535 lines, read in full)

A review of Stage B (B3 syntax, B4 types), judged "on runtime correctness and design, not parity": validation items (V, P1, O1), the V2 corpus findings (FIX-1 to FIX-4, N1, L1, R1, I1, A1, C1, G1, T1, W1, C2, M1, T2, CMP-1, DF-1 to DF-7), Review 1 and 2, and a full review of 93 commits (F1 to F65) with every module of `types` and `syntax` re-read in its final state. About 15 items are still open. What carries over:

- **SB-1. M1 parser tests, each a PHP fact the legacy got wrong** (all checkable against nikic and `php -l`):
  - deep input without nesting in the source: a flat `if … elseif` chain of 100,000 branches and a 1,000-term `.` chain crashed the legacy, because `elseif` was parsed by recursion and the left-deep tree was walked recursively later (F23). Every consumer of the tree must iterate or grow its stack;
  - `new $obj->prop(...)`, `new $this->map['m'](1)`: the class reference is a full variable (F54);
  - `"$o?->b"` is a nullsafe access in simple interpolation (F56); `"$a[01]"` reads the string key `'01'`; only `0|[1-9][0-9]*` with an optional `-` is an int key there (F57);
  - `"{$x = 1}"` is a parse error: only a variable expression follows `{$` (F64);
  - escapes are decoded by delimiter, not by a heredoc flag: `\"` in `"…"`, `` \` `` in backticks, neither in a heredoc (F9);
  - `08`, `1e`, `0x`, `0b` with no digits, doubled or trailing `_`, and a bad `\u{…}` are parse errors (F10, F28);
  - a second `default` arm in `match`, `list()` used as a value, an empty `isset()`, a spread with a key, and `(real)`/`(unset)` casts are errors; `[&$x]` and `[&$a, $b] = …` keep the reference (F17, F28);
  - `exit(...)` is a first-class callable since 8.4, and `exit` takes `string|int` (F1);
  - an anonymous class's constructor arguments belong to the enclosing scope (`new class(yield 1) {}` makes a generator) (F55);
  - heredoc `{$…}` fragments spanning lines keep exact columns (F21).
- **SB-2. AST shape rules for M1** (S1–S11, F12–F20, F26–F28). The legacy converged on these one refactor at a time; in the new repo they hold from the first commit, and nikic's AST already has most of them:
  - every name is one spanned, interned node, and variables are names too;
  - names are resolved once, by the parser (D25-3); nothing re-resolves a written name except PHPDoc text;
  - one iterator over a file's declarations with their namespaced names and whether they are conditional;
  - trait uses keep their adaptations per `use`; promotion is one optional value; a hook is one value; a body is optional; an anonymous class is its own node with no name; destructuring targets are a pattern node apart from array literals; keyword types are a variant apart from class names;
  - **every walk is a `Visitor`** that overrides only what it special-cases; the legacy's 12 hand-rolled walkers missed property-hook bodies (F24, F27);
  - `NodeId`s are unique; nothing synthesized shares one (F28).
- **SB-3. M3 type facts:**
  - integer overflow in folding gives a float as PHP does (`-PHP_INT_MIN`, `PHP_INT_MAX + 1`, `$i++` at the max); `$m % -1` is `0`; `<<` wraps and loses bits, so a shifted range that may wrap is `int` (F29, F30);
  - bitwise operators on two strings give a string; `++` on a string follows PHP's alphanumeric increment, and `null++` is `1` (F31, F32);
  - `is_numeric`'s whitespace is `" \t\n\r\v\f"`, not Unicode (F48);
  - string keys and literals are bytes: `"\xff"` and `"\xfe"` are two keys and display distinctly (F5–F7);
  - an append to a shape needs the shape's next index (`nextAutoIndexes`), and the rule after a negative key changed at PHP 8.3 (P3, F58);
  - one normalizing constructor per form (`int<10, 1>` is `never`, `int<5, 5>` is `5`), raw construction private (F47);
  - union and intersection equality is order-independent (F36);
  - `types` does not depend on `syntax`'s operator enum (F36).

  The scoreboard covers each of these as far as PHPStan models them; the rest go into `known-deviations.md`.
- **SB-4. Where the legacy chose runtime over PHPStan on purpose**, each a `known-deviations.md` candidate and not port scope: template bounds of any value type, where PHPStan's whitelist rejects `callable` and `class-string` bounds (R1-6); `InvalidComparisonOperationRule`'s number-against-array message (CMP-1); the `DF-*` string refinements (`non-decimal-int-string`, an accessory PHPStan lacks); the comparator-model `assertEquals` narrowing; version-keyed increment rules (I1).
- **SB-5. Re-inference cost, measured (M5a cost workloads):** before memoization, 1,600 nested calls took 236 s, 20,000 nested arrays 57 minutes and 19 GB, a 20,000-term chain 186 s, and an 800-member union built through branches 64 s (P2). FR-6's design (each expression inferred once, per-node results) prevents it; these four inputs become M5's deep-input workloads beside EV-3's deep-chain file.
- **SB-6. Tools worth rebuilding in the scoreboard:** `cargo xtask type-drift` (T1) classified every changed binding by relation (equal, narrower, wider, incomparable). The scoreboard's per-assert diff between two commits reports the same classes, so a milestone can tell a regression from a refinement. "Every displayed type parses back" (P1) is already in M3 (F4).
- **SB-7. Comment rot from mechanical renames** (F21, F53): about 40 false or misplaced doc comments after renames and a type-language rewrite. **Process rule:** a rename commit re-reads every comment it touches; a doc comment must sit on the item it describes (clippy's `doc_lazy_continuation` and a review of the diff, not a later review document).

- **R-2. The reviews were judged "on runtime correctness and design, not parity with PHPStan",** while the code was being reshaped into PHPStan's architecture. The two goals pulled against each other. That is one more reason the restart puts parity first and runtime truth after it.

### STAGE_B_FIX_REVIEW.md (4,842 lines, read in full)

A review of the Stage B review's fixes: 167 commits read diff by diff, every module they reshaped re-read whole, 89 findings (F1–F89), each open one with a full design paragraph against the legacy's code, plus a re-check of every item the earlier review marked done (about 15 did not hold in full). About 70 findings are open; their designs were never built. What carries over:

- **SBF-1. Many designs reinvent a PHPStan mechanism under another name.** The port gets each by construction; each is also a check for the milestone that ports it:
  - an implicit vs explicit `never`, so an impossible narrowing doesn't make code unreachable but a `never` return does (F4): `NeverType::isExplicit()` (M3);
  - variable certainty Yes/Maybe, quiet reads of undefined variables as `null`, and `isset.variable`/`nullCoalesce.variable` on a never-defined name (F5): `MutatingScope`'s certainty (M5a);
  - one holder per expression carrying type, native type, certainty and aliases, instead of nine side maps each with its own merge rule (header): `ExpressionTypeHolder` (M5a);
  - branch scopes of `&&`/`||` computed once per node, bottom-up (F18): `SpecifiedTypes` (M5a);
  - a shape's possible next keys as a set that is part of the type (F13, F25): `ConstantArrayType::$nextAutoIndexes` (M3);
  - named arguments reordered and sealed spreads expanded before any builtin rule reads an argument (F65, F72): `ArgumentsNormalizer` (MG-9);
  - `match` exhaustiveness decided by narrowing every arm false and checking for `never` (F53): `MatchHandler` with the type specifier (M5a);
  - union display order by `UnionTypeHelper::sortTypes` (F22): CA-5's exact `describe()` (M3);
  - an argument checked against its parameter with the call's templates resolved, a rejected one by its bound (F36, F38): `ParametersAcceptorSelector` and `GenericParametersAcceptorResolver` (M5a);
  - the error type as a kind of `mixed` that every "unknown" guard covers (F21): `ErrorType extends MixedType` (M3);
  - `ImpurePoints` per statement, property facts forgotten after an impure call (F62, F82): `ExpressionResult` impure points and `invalidateExpression` (M5a);
  - the asymmetric loop joins and discarded passes leaving nothing behind (F63, F87): non-final passes with a Noop callback (EV-4).

  This is FR-1/F6's conclusion again, from the bug side: the legacy's open findings are mostly PHPStan's architecture, rebuilt one finding at a time.
- **SBF-2. Designs beyond PHPStan, as Phase 4 titles** (each kept with its PHP evidence in the legacy file):
  - integer overflow typed as a marked float, with `*.intOverflow` identifiers, and engine limits bounding `count()` (`HT_MAX_SIZE`) and string lengths (F39);
  - recursive types (`μA. array{x: int, y?: A}`) as the exact fixed point of self-nesting loops (F81);
  - open and closed resources, with `fclose` closing its argument (F27);
  - reference groups that carry a write through `$a = &$b`, a by-reference `foreach` and `global` (F45); PHPStan models only part of this (intertwined references), so the port takes PHPStan's part first;
  - duplicate declarations judged by load sets (Composer `files`, entry points, the include graph, the PHPUnit run) with certainty levels, and ambiguous classmap autoloading (F75);
  - forgetting property facts by may-alias over object types (F82);
  - `$argv`/`$argc` certainty by CLI entry point (F42);
  - `rand()`'s crossed bounds, `mysqli` result kinds by SQL verb, Doctrine field mapping (F44, F64, F79).
- **SBF-3. PHP facts the legacy probed with `php`.** Each goes to `known-deviations.md` if PHPStan differs, or is already a scoreboard test if it agrees:
  - `null == '0'` is false on PHP 8, `null == ''` true; a float against a non-numeric string compares as strings on PHP 8 (F40);
  - `NAN === NAN` is false, and a `NAN` match arm never matches (F11);
  - `'5 apples' + 1` is `6` with a warning; `'abc' + 1` throws only from PHP 8.0 (F32, F34);
  - `/` and `%` throw for divisors that convert to zero (`false`, `null`, `'0'`, `0.5` under `%`) (F16);
  - `rand(10, 1)` runs, while `mt_rand`/`random_int` throw (F44);
  - trait method and property collisions: the class's own method wins, an `as` alias doesn't resolve, an abstract method never collides, one trait reached by two paths doesn't collide (F29);
  - a promoted `public private(set)` property is read-only outside (F30);
  - `__tostring` in any case makes a class `Stringable`; method names fold in overrides, `final` and abstract checks (F35, F55);
  - `preg_match_all` fills unmatched groups with `''` (or `null`, or `['', -1]`) in pattern order, and set order is a list (F71);
  - `array_combine`, `array_change_key_case`, `array_fill_keys`, `explode` with a negative limit, `sscanf`'s `%X`/`%n` (F49);
  - `mb_strtolower`'s final sigma changed in PHP 8.3 (F22);
  - `\int` as a native hint is a compile error (F19): a parser-suite case (M1).
- **SBF-4. Deep-input workloads with measured legacy times** (F18, F58, F78), for M5's cost workloads and their exit "linear up to 20,000 terms":
  - a literal `.` chain (8,000 terms: 51.8 s), an `&&` chain (4,000: 15.8 s), an `||` of 10,000 `instanceof` checks (705 s);
  - 1,000 nested `match` (71 s); a chain of traits each using the previous (3,000: 63 minutes, cubic);
  - a long `elseif` chain (quadratic through a suffix check at every branch);
  - duplicate-declaration checks over files × names; an LSP hover that cloned the enclosing scope once per variable (Phase 4).

  Together with SB-5 and EV-3 these make one `perf/workloads.toml` group, `deep-*`, added when M5 starts.
- **SBF-5. Verification tools the legacy designed:**
  - **a recursion lint plus a small-stack depth test** (F20): every traversal runs over 100,000-deep inputs on a 256 KiB thread. In the new repo the test belongs in M1 (syntax walks) and M3 (type traversals). The lint is needed only if recursion is allowed; an iterative design per D20-2/SB-1 makes the test the only check;
  - **memo keys include every input** (F33): the legacy's infer cache read checker state missing from its key. Same rule as OFF-4, for in-memory memos: each memo key is an explicit input list, and a debug switch can recompute on a hit and compare (opt-in, not a second implementation per D20-9);
  - **coverage-scoped tests** (F86): a fix landed with two fixtures failing because the fixture target ran outside the module's tests. In the new repo the scoreboard's `--filter` and CI's subset run on every push, so this does not recur; a coverage map is not needed.
- **SBF-6. Process.** The file ends with a dependency order among fixes ("F4 first… F35 before F55…") and a review-of-the-review section, and about 10% of its findings are comment rot (F24, F34, F37, F43, F51, F59, F80, F88). It is R-3 at its largest: 4,842 lines of designs written before building any. Confirms "a defect becomes a failing test, then a fix" and SB-7's comment rule.

- **R-1. What the three reviews' findings are:** semantic bugs in the hand-built engine. Examples:
  - a type check that can't pass narrows to the checked type;
  - `NAN === NAN` reported as always true;
  - `null == '0'` taken as always true;
  - a write through a reference that misses its variable;
  - an exhaustive `match(true)` reported as unhandled;
  - a `break` inside an `if` in a `switch` that loses its path;
  - loose `==` narrowing as if it were `===`;
  - an unparsable file skipped without a report.

  There are 136 open tasks across the three files. Most are bug classes that PHPStan's nsrt and rule tests cover, so the port avoids them by construction. The runtime-truth ones become runtime-oracle probes in Phase 4.
- **R-3. Process.** First a review, then a review of the review's fixes (4,842 lines, with a design paragraph per finding), then a review of Stage C. The new repo has no review documents: a defect becomes a failing test, then a fix.

### STAGE_C_REVIEW.md (237 lines, read in full)

A review of the crate split's C1–C3 (config owners, provenance, bodies out of the registry, resolved attributes): 13 findings and one outside the stage, all open. What carries over (M4/M7):

- **SC-1. A project class named like a stub class keeps its own identity** (F1): in the legacy the stub's provenance and attributes won the merge. PHPStan's locator chain puts analysed files before internals (LED-5), so the project's declaration is the class. An M4 test: a global `class Event` with the event extension's stub present.
- **SC-2. Attributes are matched by the class they resolve to** (F3): `#[Override]` in `namespace App;` without an import names `App\Override`, so it is not PHP's `Override` and no override check applies; the same for `NoDiscard`, `Deprecated`, `AllowDynamicProperties`, `ReturnTypeWillChange`. A pattern in configuration written `\Lib\Route` matches `Lib\Route` (F2).
- **SC-3. An unqualified constant in a namespace resolves the namespaced name first**, declared or predefined, then the global one (F7): `namespace Foo; const PHP_EOL = 5;` makes `PHP_EOL` `5` there. M4 test.
- **SC-4. Build-dependent predefined constants are their general type** (F6; the list: `PHP_EXTRA_VERSION`, `ZEND_THREAD_SAFE`, `ZEND_DEBUG_BUILD`, `PCRE_JIT_SUPPORT`, `DEFAULT_INCLUDE_PATH`, `PEAR_*`, `PHP_CONFIG_FILE_SCAN_DIR`, `PHP_BUILD_PROVIDER`, `INTL_ICU_*`, `LIB*_VERSION`, `OPENSSL_VERSION_TEXT`, `GD_*`, `ZLIB_VERSION`, `CURLVERSION_NOW`). Same as CA-4; PHPStan's `ConstantResolver` has its own list, which the port takes.
- **SC-5. A promoted property keeps its `(set)` visibility and its parameter's `@deprecated`/`@internal`** (F8). M4 test.
- **SC-6. Configuration rules for M7:**
  - no compatibility shim for renamed keys: an unknown key is an error (F9). With `phpstan.neon` read directly this is PHPStan's own `parametersSchema` validation;
  - any "every key on" or "every key listed" set (a gate, a reference doc) is generated from the schema, never kept by hand (F12);
  - a parameter's doc says what it changes, so the generated reference is complete (F10).

### LEVEL3_TRIAGE.md (162 lines, read in full)

Every open finding of the three stage reviews, re-run as a probe on the legacy's gate build at `b9de6dcf1`: of 120 rows, 65 reproduced, 11 partly, 23 did not reproduce in the shape tested, 4 had been fixed, and 17 were not probed.

- **L3-1. Findings written from reading were wrong about one time in five.** 23 of 103 probed findings did not reproduce. Several had a design paragraph of their own (F6, F28, F40, F47, F62, F67, F69, F72). **For the new repo:** a defect exists when a test fails, never when a review says so; this is the case for "a defect becomes a failing test first" in measured form.
- **L3-2. 120 probe files exist** at `~/.cache/level3-triage/probes/` (one `index.php` per finding, with opt-in flags). They test the legacy's behaviour, not PHPStan's, so they are not scoreboard inputs. In Phase 0 they move to `~/.cache/phpstan-rs-legacy/probes/` with the legacy archive. The ones whose finding is a PHP runtime fact (SBF-3) are candidates for the runtime oracle in Phase 4.

### OPEN_WORLD_PLAN.md (766 lines, read in full)

The active legacy work when it stopped: an open/closed setting for writes by unseen code (globals, by-reference captures, includes), exact integer-overflow detection with environment provenance (O1–O7, built), and a relational domain plus CHC solving to clear guarded overflow findings (O8, in design: Z3 Spacer, cvc5 certification, rope replay for witnesses, spikes on WordPress's tag processor, `WP_Query` and sodium_compat). All of it is Phase 4 (W-1). What carries over:

- **OW-1. Group A is parity work that was never finished:** `number` and about ten `TypeNodeResolver` keywords (`associative-array`, `callable-array`, `callable-object`, `empty`, `empty-scalar`, `non-empty-literal-string`, `non-empty-mixed`, `non-empty-scalar`, `static-closure`, `static-pure-closure`) were missing (A1); unary operators on `mixed` (A2). M2/M3 port `TypeNodeResolver` whole, so these come with it (W-2).
- **OW-2. Phase 4 needs PHPStan's shape first.** The design hooks into `Locals`, `merge_scopes`, `apply_narrowing` and loop convergence: in the port these are `MutatingScope`, `mergeWith`, the type specifier and `generalizeWith`. Its effect model (B1–B7) is the sibling of the legacy's `PropertyWrites` fact, which in the port is `ExpressionResult`'s impure points plus `invalidateExpression`. When Phase 4 picks this up, it extends those, not a parallel engine (the plan's own "separate IR" attack reached the same verdict).
- **OW-3. Evidence worth keeping, by pointer:**
  - the corpus set and classification protocol (lost, gained, artifact, legitimate uncertainty; zero artifacts before cost is reported);
  - PHP 8.5 facts: `1 << 63` is `PHP_INT_MIN`, `intdiv(PHP_INT_MIN, -1)` throws, `abs(PHP_INT_MIN)` and `-PHP_INT_MIN` are floats, `HT_MAX_SIZE` is 2^30 on 64-bit, string lengths and `time()` have no universal bound, MySQL/PostgreSQL/SQLite column bounds differ;
  - the eleven true overflow positives (Piwigo pagination, WordPress XML-RPC, Laravel `firstItem()`, monolog and Psalm memory shorthand, `WP_Query`'s `abs($cat)`) and the `remove_attribute` duplicate-attribute corruption in WordPress's tag processor, reproduced under PHP 8.5;
  - the spike encodings and solver matrix under `~/.cache/phpstan-rs-scratch/spike/`, archived with the legacy in Phase 0.
- **OW-4. A design rule from its attacks, general enough to keep:** a second semantics that must agree with the checker forever is rejected; an encoding's clauses come from the same transition as the code, never written apart (a query missing a bound gave a false witness). In the port this is "one inference engine" (D25-5).

- **W-1. All of this is beyond-parity research,** and it was the active level when work stopped:
  - the open/closed-world setting for global writes;
  - exact integer-overflow detection in i128;
  - runtime limits used as return ranges: `HT_MAX_SIZE`, address spaces per architecture, OS clock limits;
  - O8's relational domain, with SMT spikes and witness synthesis.

  It extends PHPStan's `Type` and `Scope`, so it needs them first: Phase 4.

### OPEN_WORLD_LEDGER.md (160 lines, read in full)

- **W-2. Row A1 is still open:** `number`, a basic PHPStan PHPDoc keyword, gives `class.notFound`, and about ten other `TypeNodeResolver` keywords are missing too. This was the state while SMT-backed overflow proofs were being designed. It is the cost of extending before porting, in one row. M2 ports the keyword list whole.

The status ledger for OPEN_WORLD_PLAN: decisions Q1/Q3 still open, groups A, B, C and O9/O10 open, O1–O7 done, O8 spikes done with design pending. What it adds:

- **OWL-1. Data flow in types has a cost the meter must see from the first commit of Phase 4.** O7's first build took the Piwigo run from 20.47 G to 35.95 G (an interner defect), and to 20.86 G once fixed. One defect almost doubled the count, and only a hand callgrind run caught it.
- **OWL-2. A precision corpus has to be one project per input** (M2): Mago's analyser snippets declare `class Collection` eight ways and loaded as one project, which made 88 `method.notFound` losses meaningless. This is CA-4's "each nsrt file in its own project", confirmed on another corpus.
- **OWL-3. PHPStan's function-level stub PHPDoc removed 9 Piwigo false positives at no gain** (M9, `2c189db92`): one more measured case where the legacy's own builtin knowledge was worse than PHPStan's.

### BY_REF_WARNING_AND_FIBER_DESIGN.md (372 lines, read in full)

A design for code that runs at control transfers the caller does not see: error handlers at warnings and fibers at suspends, plus the open-world cost experiments X1–X7. It was never built; two fixtures wait on it. All Phase 4 (beyond PHPStan, which models neither). What carries over:

- **BR-1. PHP facts, probed under PHP 8.5**, for Phase 4 and the runtime oracle:
  - `@` does not stop an error handler; the mask filters by level; a later `set_error_handler` replaces the earlier one;
  - a user function given `null` throws `TypeError`, which no handler sees, while a builtin given `null` raises `E_DEPRECATED`, which a handler does see. The legacy used one identifier (`argument.type`) for both. PHPStan's identifiers are the port's; the distinction is Phase 4's;
  - `trigger_error` runs the handler with `E_USER_WARNING`; a handler that throws `ErrorException` ends the path;
  - a fiber's by-reference capture can be written through another closure that shares the creator's frame; `$fiber->throw()` resumes at the suspend point with an exception.
- **BR-2. Its own adversarial pass refuted its first version** (the fiber state source, the shared summary, the cost claim). That is L3-1 again: a design judged by reading is wrong often enough that only a test settles it.

### OVERFLOW_MEASUREMENT.md (276 lines, read in full)

The corpus measurement behind OPEN_WORLD's group O: a blunt "unbounded `int` arithmetic may be a float" experiment, then the bounded, counter, provenance and source-catalogue prototypes, with every gained finding read. Phase 4 evidence. What carries over:

- **OM-1. A check that is true at runtime can still be mostly noise.** The blunt model added 24% to Piwigo's findings and 1–12% to the vendor packages (about 764 new locations). Seven were real request-driven `TypeError`s; the rest were offsets, counts and clocks the model couldn't bound. Provenance plus a source catalogue cut it to 10 gained, all read. **Phase 4 rule:** a runtime-truth check ships opt-in until its gained findings have been read and classified on the corpora (the user's existing "ship opt-in, don't delete" rule), with the classification kept beside the check.
- **OM-2. Comparison artifacts look like findings.** Two of its first numbers were wrong because of the diff, not the checker: vendor keys not normalized to package-relative paths, and the absolute path inside anonymous-class names, which made unchanged findings look changed (78 each in Laravel and Symfony). **For the scoreboard and the end-to-end diff (Phase 2):** paths are project-relative before comparison, and an anonymous class's displayed name never holds an absolute path (PHPStan's own `class@anonymous` naming is what the port uses).
- **OM-3. A version-dependent return range:** `strcmp` and friends return `-1|0|1` from PHP 8.2 while the stubs say `int`. Whether PHPStan's function map has it is checked in M5b; if not, it goes into `known-deviations.md`.

### CI_SPLIT_PLAN.md (334 lines, read in full)

A plan to split the legacy's serial CI (35 minutes, 16 checks in one job, each OS leg serial) into 13 parallel jobs with a shared setup action, read from the logs of one run. What carries over to Phase 1's CI:

- **CI-1. CI time is compilation, one build per profile.** Tests ran 19 s of a 450 s step; each profile (check, test, gate, bench) compiled its own dependency set. So **jobs are split by profile and run side by side** (lint/check, test, scoreboard subset, cost meter, one per OS leg), since a split repeats no build.
- **CI-2. Settings the legacy found by failure:**
  - rust-cache with `cache-on-failure: true`: no failing job had ever saved a cache, so every red run started cold;
  - a cache path for a nested workspace resolves against that workspace (`fuzz -> target`, not `fuzz -> fuzz/target`);
  - `timeout-minutes` on every job (the default is 6 hours, holding one of 20 slots, 5 for macOS);
  - a concurrency group that cancels superseded pull-request runs but neither queues nor cancels pushes;
  - one composite setup action for every workflow (release too, with caches off so a branch-run cache never reaches a shipped binary);
  - `cargo check --release --benches` instead of linking an LTO bench nobody runs (486 s);
  - listing tests or fixtures from a different feature set than the build recompiles the library (features unify differently under `--workspace`).
- **CI-3. The free plan's limits shape the layout:** 20 concurrent jobs, 5 macOS, a 10 GiB cache that LRU-evicts. The new repo's job count stays well under them; the cost meter's job is Linux only.
- **CI-4. Validation before pushing a workflow:** `actionlint`, `act -l`, and a multiset comparison of old and new `run:` lines so no check is dropped or doubled in a refactor.

### GATE_SPEED_PLAN.md (622 lines, read in full)

A plan (S0–S15, done) that cut the legacy's local quick gates from 37.5 to 13.6 minutes without weakening any of them, measured with per-step timing. What carries over:

- **GS-1. Where test time went:** 250 ms per fixture, nearly all of it stub setup (65 ms cloning the stubs, 145 ms re-running registry passes over every stub class) against 0.5% analysis; a CLI test spent 11.8 s per spawn, 59% of it lexing the 2,351 stub files serially. **For the new repo:** B-1's shared immutable stub layer plus lazy reflection is the fix the legacy deferred to C4/C5 and never reached. The M4 exit includes "a one-file fixture analyses in milliseconds", measured.
- **GS-2. One worker pool, injected** (S4): each fixture built a pool sized to all cores, so 8 parallel fixtures ran 64 threads, a third of the CPU went to `sched_yield`, and 8 test threads were 1.6× one. The Session takes its pool from the caller; the test harness shares one (MG-11, OFF-3).
- **GS-3. Facts passes are collectors.** Two serial "facts-only" checker passes (named-argument renames 24% of CPU, property-assign facts 14%) re-ran the checker over every file before the real pass, and merged in hash-map order, so the LSP and CLI could merge differently. PHPStan's `Collector`/`CollectedDataNode` collects during the one pass, merged in a defined order (D20-4).
- **GS-4. Engine code does no I/O** (S7): `canonicalize` per file and `getcwd` per package lookup were 7.5% of CPU. Paths are made absolute once at the root and compared lexically after that; the rule was enforced by an `arch` lint. The new repo's library crates follow it from Phase 1 (paths come from the Session, D20-8).
- **GS-5. Results are data, printing is the caller's** (S3, S9): the type dump ran a second checker pass and the library printed its own errors. One analysis returns results, notices and dumps; the binary, the tests and any gate render them. The legacy also measured that a gate runner beside the binary needs a cross-check against the shipped binary, which is why the new repo's scoreboard drives the real CLI path (D20-8).
- **GS-6. Small settings:** zstd `-3` instead of `-19` (45 s → 0.6 s for a 1.5 MB vs 1.0 MB file); a fixed sleep in a test is a wait on an explicit signal (S12); `cargo check` instead of a build for a feature-set check.

### PROJECT_BUILD_COST_INVESTIGATION.md (1,137 lines, read in full)

The investigation behind B-1, then its catalog built (§12): a shared resolved stub base, scoped incremental resolution, raw/resolved separation, a stub cache on disk, and an instruction-counted start-up drill. Measured results: a one-file `check-project` went from 743 ms to 46 ms, the fixture suite from 203 s to 6.6 s, one fixture's start-up from 231.5 M to 50.9 M Ir, and the lib suite under callgrind from 201 G to 31 G. What carries over:

- **PB-1. The numbers the new repo starts against.** One fixture's start-up at 50.9 M Ir (the analysis itself under 1 M Ir), 26–30 ms warm, and the whole fixture suite in 2.6 s are what the legacy reached after redesigning around a shared base. The new repo's lazy design (P-2, MG-4) should be at or under these from M4. **They become reference rows in `perf/history.tsv` beside L-1:** "one-fixture start-up" and "one-file project" workloads, defined in Phase 2, with the legacy's figures noted.
- **PB-2. Incremental results go stale in specific ways** (bugs 5.2, 5.5, 5.9, 5.10, all reproduced):
  - reruns that only add facts keep what an edit removed (purity, inherited PHPDoc, trait members, `value-of` cases);
  - removing a file's declarations erased other files' merged declarations of the same name;
  - constants were never unindexed, so editing `const VERSION = 1` to `2` made it `mixed`;
  - dependents keyed by a name as written missed qualified spellings and PHPDoc-only references, and over-matched unqualified ones across namespaces (one edit rechecked 14k files);
  - a dependency collector that read every word of a PHPDoc tag line made 5,757 stub declarations dependents of a function named `a`.

  **Rules for M7's result cache and Phase 4's LSP:** keep raw declarations apart from derived ones and recompute derived facts from raw; dependencies are recorded from reflection reads as resolved, case-folded names (FR-9), so spelling never matters; and the differential test (MG-12: incremental equals a fresh load) is written first, because the legacy's `update_file` failed it before any optimization.
- **PB-3. Caches fail silently unless a test checks a warm run reads them.** The legacy's stub cache never worked until found by profiling: its CBOR reader failed on entries over 4 KB, so every run rebuilt and rewrote 90 MB. Other cache lessons: a stored index that one lookup needs is a file of its own; raw and resolved copies that are 94% identical are stored once; `postcard` stored the same data in a quarter of CBOR's size; serde numbers variants after a skipped one differently when reading, so a skipped variant goes last. **For M4/M7:** each cache has a test that a second run reads it and writes nothing (the project's CLAUDE.md already says to check this by hand).
- **PB-4. PHPStan's own source as a stub source is open** (§4.3): 16 `extends` targets and 245 type-referenced classes come from its dependencies (PhpParser, Nette DI, Symfony Console, PHPUnit, BetterReflection, phpdoc-parser). LED-4's locator over the installed phar must include the phar's bundled vendor code, or every PHPStan class extending `PhpParser\NodeVisitorAbstract` has an unknown parent. Loading it only when a project names `PHPStan\` (#5, G10) is a cost rule, not a parity rule; PHPStan itself always has its classes.
- **PB-5. Parallelism has a price under concurrency.** rayon's idle workers spin 32 yield rounds before sleeping; many small parallel regions gave 1.55× wall for 1.8× CPU, which loses when tests already run eight at a time. Regions get a minimum chunk (`with_min_len`), and the test harness's pool is sized by measurement (GS-2). A test that ran its own groups in parallel deadlocked on the project's pool, since one group held a lock its build needed workers for (OFF-3).
- **PB-6. M8 inputs:** mimalloc took the Piwigo scoped run from 14.1 s to 10.5 s; parsing the 755 fixture configs cost 4 M Ir per start; a stack-limit query per guarded recursion and two binary searches per token for line and column were hotspots. Each is measured afresh in M8.

- **B-1. Test cost came from building the world in every test** ("the tests themselves are slow, not the harness").
  - Each test built a `Project` with about 3.5k stub classes and 5.3k stub functions, and ran 16 whole-registry passes. The checks themselves were 1–1.5% of a fixture's time.
  - The quick gates took 37.5 minutes and CI 35 minutes per run. Benches alone took 487–570 s, and Windows tests 1,017 s.

  In the port, lazy reflection makes a test cost what it reads, the harness shares one immutable stub layer, and CI never builds LTO benches.
- **B-2. Wall time on this VM is noise:** ±20–40% within a run, and up to 65% between runs under load (§1). Only interleaved A/B runs can be trusted. The meter therefore judges by Ir and allocation counts, and measures wall time only at milestone ends, interleaved.

### CROSS_PLATFORM_PLAN.md (2,310 lines, read in full)

Eighteen rows (R0–R18) over four CI rounds that made the legacy pass on macOS, Windows and aarch64 Linux, each with Now / Change / Test / Built, mutations per row and wine runs. B-3 had the checklist; the full read adds the rules and the PHPStan facts behind them:

- **CP-1. Path spelling in machine output (M7).** One function spells a path for reports: `/` on Windows, the path's own text on Unix (a `\` is a name character there). It is used for JSON, checkstyle, JUnit, the baseline, anonymous class names and any message that names a file, so one project gives the same report and baseline on every platform. Text and table output keep the platform's spelling. **PHPStan fact:** `FileHelper::normalizePath` turns every `\` into `/` for both the pattern and the file before `FileExcluder` compares them, on every platform, and disallowed-calls' `FilePath::fnMatch` does the same. So a `\` in `ignoreErrors`, `excludePaths` or `paths` is a separator everywhere. The legacy first planned otherwise and was wrong. `FileExcluder` folds case on Windows only. **The port takes `normalizePath` and `FileExcluder` as they are.** The legacy's per-root case probe and `\`-as-both readings are its own (Phase 4 at most).
- **CP-2. Configured paths (M7):**
  - every configured path is relative to the config file, never the working directory;
  - a database or schema file opens read-only and fails when absent, instead of being created (Phase 4's SQL);
  - fnmatch's `?` and `[…]` in `ignoreErrors` were a parity gap on every platform, which the scoreboard's M7 tests cover.
- **CP-3. Include resolution.** The legacy's include rule (two worlds, a declared `include_path` and `working_dirs`, Composer's `include-path`, `chdir`) is its own design, so it goes to Phase 4. The port ports PHPStan's `RequireFileExistsRule` as it is. The PHP facts it probed with php 8.5 are runtime-oracle inputs:
  - a plain name is tried in each include path entry, then the includer's directory, then the working directory;
  - `./x` and `../x` are tried in the working directory only;
  - an empty entry before another is `/`;
  - `set_include_path('')` fails.
- **CP-4. The command runs on a thread whose stack the program chooses (Phase 1 skeleton).** Windows gives the main thread 1 MiB. With a 1 MiB main stack, one monolog run made 36,914 `mmap` calls (one per stack-guard growth) against 65 after the fix. `main` parses arguments, sets the allocator up, then runs the command on a named thread with the worker stack size, and resumes a panic so the exit status stays 101.
- **CP-5. Platform services (Phase 1/M7):**
  - caches go under `dirs::cache_dir()/phpstan-rs`, with an override variable and an off switch, and the choice is a pure function of its inputs;
  - on Windows an entry's last-use time must be set through a write handle, because a read-only one is silently refused;
  - colour goes through `anstream::AutoStream` (Windows VT mode, or console API translation), with `NO_COLOR` set and non-empty turning it off.
- **CP-6. Line breaks (M1, M2), on every platform.** A line ends at `\n`, `\r\n` or a lone `\r`, as PHP counts it (`__LINE__`, `//` comment end, the newline `?>` takes, heredoc lines and labels) and as LSP does. The legacy kept the `\r` in CRLF `//` comments and ignored a lone `\r` everywhere. **M1 tests:**
  - every token's line and column, comments, `?>`, heredoc/nowdoc bodies, labels and indentation, each over the three breaks;
  - a randomized test of 3,000 sources written with each break, lexing to the same tokens at the same positions;
  - the line index scans `\n` alone unless one `memchr` pass finds a `\r`, which costs +12 M Ir on Piwigo against +51 M for a two-byte search.

  **M2:** docblocks split by the same rule. **A PHP fact for the regex work:** PCRE's `x`-flag comment ends at `\n` only, so a pattern in a `\r` file can fail to compile under PHP too.
- **CP-7. Build scripts and embedded data (M4):**
  - embedded paths are joined with `/`;
  - a source that holds `\r\n` is embedded from an LF copy, so a Windows checkout with `core.autocrlf` (which `.gitattributes` doesn't reach inside the phpstan-src submodule) embeds the repository's bytes;
  - the source hash covers each crate's `src`, `Cargo.toml`, `build.rs` and data files, which is OFF-4's enumerated-input rule.
- **CP-8. Testing on every platform (Phase 1):**
  - a test helper makes links: a symlink on Unix, a junction for a directory on Windows (no privilege needed, the `junction` crate), and `symlink_file` with a Developer Mode hint;
  - permission tests stay Unix-only;
  - tests name paths through the same canonical and report spelling the code uses, never `format!("file://{}")`;
  - **Windows is checked locally with MinGW and wine** (`scripts/winrun.sh`, the quarry). Wine found two defects in the Windows path reader before CI did. CI runs Linux, macOS, Windows and aarch64 Linux.
- **CP-9. Tooling has no GNU-only commands (Phase 1 xtask):**
  - `File::set_modified` for `touch`;
  - `similar` for `diff -u`;
  - peak RSS from GNU `time -f %M` or BSD `time -l`, and none on Windows;
  - path rules matched by components, never by splitting on `/`;
  - Linux-only scripts say so in their usage line;
  - `-Z threads=8` sits under `[target.'cfg(all())']` so every host gets it.
- **CP-10. The heap profiler is per platform (Phase 2 cost meter):**
  - the patched dhat's frame-pointer walk reads `rbp`/`x29`, and is bounded by `pthread_getattr_np` on Linux and by `pthread_get_stackaddr_np`/`pthread_get_stacksize_np` on macOS;
  - Windows falls back to the `backtrace` unwinder;
  - `patches/dhat` has its own lockfile and tests (40 nested calls give 40 frames).

  The cost gate runs on Linux only, so this matters only for local runs elsewhere.
- **CP-11. Layering (Phase 1).** The legacy's in-crate module-graph gate missed new modules held only by the root, and six layering edges grew unnoticed (rules reading project internals, an extension calling composer). In the new repo the crates are the layers, so Cargo enforces the edges, and no module-graph gate is needed. The two placements it settled carry over:
  - the completion rules (`Completion`, `Reach`) are pure functions in `syntax`;
  - path byte reading is a std-only crate at the bottom.
- **CP-12. Process.** Several rows' "Built" sections start with "the plan said X; built Y because…": the `report_path` join, the case rule per root collapsed into one, `anstream` instead of the planned function, and the per-row Windows `Path` claims that wine disproved. That is L3-1 and D25-1 again: design by reading, corrected by building.

- **B-3. Portability checklist, from the first commit** (details in CP-1 to CP-10):
  - no `\\?\` verbatim paths;
  - paths compared and hashed in a canonical form (`c:` equals `C:`);
  - reports spell paths with `/`;
  - `PATH_SEPARATOR` and the include path set per platform;
  - the main thread's stack chosen by the program;
  - each platform's cache directory;
  - read-only handles and ANSI escapes handled on Windows;
  - LF in `.gitattributes`, and `/` in embedded paths;
  - no GNU-only tools in xtask.

  The legacy found all of these late, because CI ran only on Linux until 2026-10-18.

### CRASH_REPORTING_PLAN.md (1,316 lines, read in full)

A design for opt-in Sentry reporting of the legacy's own crashes. It was never implemented. It covers a supervisor and worker process pair, minidumps through `minidumper`/`crash-handler`, an on-disk outbox, a bundle of the inputs that reproduces each crash with a replay check, and a `crash-repro` tool with replay, reduce and fetch. The Sentry half is Phase 4 item 7. Step 1, panic isolation, is M7 parity work, and it measured how the legacy got it wrong:

- **CR-1. Panic isolation, as M7 builds it from the start:**
  - **one panic hook, installed once.** The legacy swapped in a silent hook around every batch, so the panics that mattered reached no hook and kept no location;
  - **rayon moves a worker's panic to the caller with `resume_unwind`, which runs no hook.** Every parallel closure runs its body through one `carry` wrapper that records the message, location and backtrace on the worker. A clippy `disallowed-methods` entry on rayon's constructors makes a new call site go through it;
  - **every unit is isolated, not only per-file checks.** The legacy left its whole-program passes and its load steps unisolated, so a panic there ended the run with exit 101 and no message. In the port that means per-file analysis, each collector or `CollectedDataNode` rule pass, and reflection's lazy memos (a panicking memo cell stays retryable, FR-4);
  - **an internal error carries a unit, the file in focus, the message, the source location and a backtrace**, and appears in every output format. PHPStan's own form is the parity target: its `InternalError` list, the "Internal error … while analysing file" message, and the exit code it gives. M7 reads how PHPStan reports and exits, and copies that;
  - a parse panic is an internal error, never a silently "skipped" file (FR-9, F13).
- **CR-2. Process facts for M7:**
  - output goes through a locked `Write` with errors propagated. `println!` panics on a closed pipe (exit 101 into `| head -1`), so a `BrokenPipe` ends the run quietly with 141;
  - a cache behind a `Mutex` recovers from poison (`PoisonError::into_inner`) when its critical sections leave the map valid, so one panic doesn't cascade into every later file;
  - every `process::exit` goes through one exit function, so pending output and records are flushed;
  - every place that spawns the binary (xtask, integration tests) clears the tool's own environment variables, so a developer's shell can't change what a gate measures.
- **CR-3. LSP facts (Phase 4 item 2):**
  - handlers that parse or check run on the worker pool's large stacks; the legacy's 2 MiB debounce thread overflowed at 60 nested arrays;
  - parse and check phases are isolated and answer `InternalError` (-32603), so the server mutex is never poisoned;
  - an update's registry phase can only recover through snapshots (FR-4), so until then a panic there is a crash and the editor restarts the server.
- **CR-4. Phase 4 inputs, by pointer:**
  - the process design (the supervisor hosts the minidump server; the worker never touches the network; reports stay on disk until accepted; `/proc/self/exe` survives an upgrade; PDEATHSIG, kqueue or a job object so the worker never outlives its supervisor; PID 1 reaping; OOM kills read from cgroup v2 `memory.events`);
  - the privacy rules (only `PHPSTAN_RS_SENTRY_DSN` and never `SENTRY_DSN`; no config key can enable it; the `LinuxEnviron` and `LinuxCmdLine` minidump streams blanked in place; secret config keys dropped by a schema registry);
  - the release-side work (a `dist` profile with `debug = "limited"`, build IDs for mold, split debug info per OS, `sentry-cli` pinned by SHA-256);
  - **`crash-repro reduce`**, a span-deleting delta reducer over bundles. It is a useful general tool for turning any crash or scoreboard failure into a minimal fixture, and is a candidate for the `testing` crate whenever M5/M6 need one.
- **CR-5. Process.** The plan "pulls items forward in their final form" from four CRATE_SPLIT stages (§9). That is B-6's coupling again, in a plan whose own step 1 was the only part needed for correctness.

- **B-5. The Sentry half and the report viewer are Phase 4.** What carries over now is panic isolation, with a panic reported as an internal error (CR-1).

### UNSAFE_PLAN.md (284 lines, read in full)

- **B-4. The only real need for `unsafe` was the PCRE2 FFI.** The audit found nine ways the legacy's regex front end differed from `php_pcre.c`:
  - bytes vs chars;
  - the delimiter scan;
  - `u` meaning UTF|UCP;
  - the `r`, `n` and `J` modifiers;
  - inline option scoping;
  - and others.

  `unsafe_code` stays forbidden everywhere outside the one FFI crate.

The plan moves the PCRE2 FFI into one crate behind a safe RAII API with `forbid(unsafe_code)` everywhere else, and makes the PHP regex front end byte-exact against `php_pcre.c` 8.5.4. Each point below was probed with php.

- **UN-1. Parity needs real PCRE2 (M6).** PHPStan's `RegularExpressionPatternRule` validates a pattern by running `preg_match` in-process, so its `regexp.pattern` messages are PCRE2's own text and offsets. A port that matches them needs PCRE2 itself, at the same version PHP bundles. In practice that means one FFI crate, the only place `unsafe` is allowed, with:
  - PCRE2 built from bundled source as a static library (`PCRE2_SYS_STATIC=1`, so the build machine's `libpcre2-8` is never linked), with its version asserted in a test;
  - one FFI call per `unsafe` block, each with a `// SAFETY:` comment enforced by clippy (`undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`);
  - a `compile(bytes, options) -> Result<PatternInfo, CompileError>` that knows nothing about PHP;
  - a fuzz target on the boundary, since Miri can't run through FFI.

  B-4 already says this. This is the concrete design, and Phase 1 creates the crate empty.
- **UN-2. PHP's front end, byte for byte (M5b/M6):**
  - PHP works on bytes. A non-UTF-8 pattern is validated too, and a multibyte "delimiter" is its first byte;
  - the delimiter is found by a forward scan with `\` skipping and bracket nesting from the `"([{< )]}> )]}>"` table, never by `rfind`;
  - modifiers are `i m n s x A D r S X U u J`, with `u` meaning `UTF|UCP` and `r` meaning `EXTRA_CASELESS_RESTRICT`. A space, `\n` or `\r` is skipped, a NUL is its own error, and anything else is `Unknown modifier '%c'`;
  - the messages are PHP's own (`Empty regular expression`, `No ending (matching) delimiter`);
  - patterns from constants and constant arrays are validated, not only literals.

  This is what PHPStan's rule does by calling PHP. The port needs it because it can't call PHP.
- **UN-3. `$matches` shapes (M5b).** PHPStan's `RegexArrayShapeMatcher` parses patterns with its own grammar (`RegexGroupParser` over the Hoa `Regex.pp` grammar), and **parity is that parser, ported as it is.** The legacy's probed PHP facts are oracle tests for it, and `known-deviations.md` gets any where PHPStan differs:
  - `n` (no auto capture);
  - inline option scoping, where `(?n)` applies to the rest of its group and later alternatives;
  - `J` duplicate names (the named key holds the last matched duplicate, at the first duplicate's position);
  - byte units without `u` (`/^(é?)$/` can capture `"\xC3"`);
  - the key layout from `CAPTURECOUNT` and `NAMETABLE` (`make_subpats_table`, `populate_subpat_array`).

  A layout taken from PCRE2 itself, plus the match-oracle fuzz target that checks inferred shapes against real matches, is Phase 4 ("runtime truth").
- **UN-4. Tests never mutate the process environment.** `set_var` in parallel tests is a data race, and it's `unsafe` in edition 2024. Code that reads the environment takes a lookup closure, and tests pass their own.

### ITEMS_1_TO_3_FIXES_PLAN.md (2,008 lines, read in full)

Fifteen load-path rows (R1–R15), all built. Together they took the off-mode Piwigo run from 21.33 G to 20.37 G Ir (−956.6 M, 4.5%), and Step 0 measured wall time thoroughly along the way. The rows fix the legacy's own load structure. What carries over:

- **IT-1. Design facts for M4/M7's load path** (they confirm D20-8 and add to it):
  - **validate UTF-8 once** with a SIMD check (`simdutf8`, a third of std's cost) and decode lossily only when that fails. The legacy's lossy decode of every file cost 394 M. A file that isn't UTF-8 is lexed from its bytes;
  - a path carries its hash, extended per child as the walk descends, and identity inside a scope is a dense `FileId`. Paths are converted to display text once, at the finalizer;
  - **each path is resolved once.** glibc's `realpath` calls `readlink` once per component. The legacy resolved vendor paths twice (7,494 `readlink`s, 7,493 of them failing). A file's kind comes from its directory listing (the dirent type), not a `statx` per file;
  - **the analysed files' walk does not wait for vendor discovery.** A walk hands a directory's files over when it is the only walk of the strongest role reaching it, so analysed and stub roots start before the context roots are known. This removed 120 ms of idle pool time on a warm run;
  - Composer's generated autoload files (1.8 MB `autoload_static.php` on Piwigo) are read once, and the extracted entries are cached by the file's bytes;
  - trees are dropped on the worker that built them (D20-4, measured): −72 ms of main-thread frees and −214 MiB peak RSS, most of it page zeroing avoided.
- **IT-2. Where the wall time went (M8 input; confirms P-3).** At 8 threads the run left 0.75 s (off) and 0.85 s (warm) of core capacity unused. The largest serial stretch was the whole-registry finishing passes, 0.26–0.28 s. Lazy per-class reflection removes that stretch rather than parallelizing it. The next largest were a 110 ms serial decode of cached signatures on the main thread and the 120 ms vendor wait.
- **IT-3. Allocator and kernel facts (M8, measured afresh there):**
  - the largest kernel leaf is page zeroing on first touch (0.82 s of CPU at 8 threads), so only a smaller footprint lowers it;
  - transparent huge pages stay on: 2.7–2.9 s with them against 3.6–4.1 s without;
  - THP `defrag=madvise` makes a huge-page fault compact memory directly, and one recording took 6.35 s for that reason. Every wall claim is a median of repeated runs, with `/proc/vmstat` compaction counters logged;
  - mimalloc purging cost a batch run 5.4% before trees moved to the workers, and about 1% after. A batch run sets `purge_delay = -1` before any thread starts, while the LSP keeps the default. In the new repo this setter is FFI, so it goes into the FFI crate (UN-1) or a dependency;
  - the same code takes 44% more CPU at 8 threads on this VM (shared host cores and L3), so "wall = serial + parallel/threads" fits mislead. The measure that holds is unused capacity per timeline stretch.
- **IT-4. Measurement tooling to carry into Phase 2:**
  - **Ir per CPU second varies 10×:** allocation runs at 1 G/s, `memcpy` and the lexer at 2–5 G/s, byte loops at 10+ G/s. Ir rows and wall rows are claimed separately, and a row is never sold on Ir alone (LED-6);
  - **paired A/B timing:** `scripts/abab.py` runs 20 alternating pairs with a Wilcoxon signed-rank test and checks every output against an oracle hash. In hyperfine's rounds the second command always ran slower;
  - **the warm-cache check:** fill the cache, touch a marker, run, and `find -newer marker | wc -l` must be 0. A `-newer` check against the output file misses files written earlier;
  - **scripts worth porting into `xtask cost` or keeping as scripts:** `cgfn.py` (per-function Ir diff between two runs, with a self-check that `summary:` equals the sum of self costs), `callers.py` (inclusive Ir by caller), `perfdump.py` (`phases`, `unused`, `main`, `incl`, `leaf`, `split`, `kernel`), `cpugrowth.py`, `thp-probe.py` and `scaling.sh` (which now hash-checks its output).
- **IT-5. Process.** Four of fifteen rows' "Built" sections say the plan's Cost or Change did not hold:
  - R9's premise was wrong, because no load takes both hashes;
  - R12's carried fact would have saved nothing;
  - R10's in-scope merge was built, measured, gained nothing, and was reverted;
  - R15's "safe" function had to be `unsafe`.

  The batch delivered 4.5% of the Ir over a 2,008-line plan, which matches D25-1/D20-12. The meter's per-commit delta would have given the same numbers with no plan document.

### MASTER_ONLY_LEDGER.md (1,168 lines, 366 KB, read in full)

The ledger judges, one row each, all 1,134 commits that the old `master` (now `master-old`) has and the current line lacks: 8 WORTH, 258 ADAPT, 836 SKIP, 32 HAVE. What the old line holds, and what carries over:

- **MO-1. What exists only on `master-old`:**
  - **an effects engine** (rows 30–39, 179–187, 529–584, 645–680, 1128–1134). A call writes a set of storages settled over the call graph. It also models closures' own effects, implicit calls (magic methods, `ArrayAccess`, `__toString`, hooks, iterators), destructors where an object dies, includes writing their top level's variables, and globals through every write form. It reached 137 G Ir on Piwigo before memoization brought it to 81 G;
  - **the builtin-runs recorder** (`tools/builtin-runs`, rows 32, 778–791, 1070–1127). It calls every PHP 8.5 function and method in a bwrap-sandboxed child with recording probes, which yields `builtin-runs-8.5.tsv` (what each builtin runs: callables, methods on arguments, stream-wrapper methods), with a coverage-gap sidecar;
  - the start of the PHP catalog (rows 138–176, 792–936, 1019–1069), deferred there after its whole-program pass didn't fit in 10 GiB;
  - an arena and typed-id route (rows 242–522);
  - dump and validation tooling (canonical printers, an edit replay, a git-backed snapshot store; rows 41–161);
  - a per-level finding model (row 160).

  **All of it is Phase 4 (runtime truth, effects) or legacy structure.** PHPStan models none of the effects engine: its impure points and `invalidateExpression` are the parity target. Phase 0 tags `master-old` (e.g. `master-old-final`) and pushes it, so the effects engine and the recorder can be found when Phase 4 starts.
- **MO-2. Incremental staleness cases the legacy found real (M7 result cache and Phase 4 LSP).** Each becomes a case in the incremental-equals-fresh differential test (MG-12, PB-2):
  - an edit to a class rechecks the files of the traits it uses;
  - an edit rechecks the other files declaring the same name;
  - a class named only in `@see` is a dependency;
  - a file appearing or vanishing where an include looked rechecks its includer;
  - a rechecked file keeps its use sites when no source is retained;
  - a stub restored by an edit;
  - PHPDoc `@method` tags drained in a pass are gone on the next pass.
- **MO-3. Facts for the scoreboard to settle, not port scope.** The legacy found these as bugs; in the port they are whatever PHPStan does, and nsrt or rule tests check them:
  - magic constants typed by value (`__LINE__` an int, `__DIR__` a string);
  - an inline-HTML token's span excluding the open tag;
  - a nested declaration reading its own templates, not the enclosing method's;
  - negative string offsets before 7.1 (PHPStan's version handling);
  - an explicit `mixed` inherited stays explicit;
  - weak-mode scalar coercion and the 8.1 null-to-internal-scalar deprecation by version;
  - a string callable naming a builtin checked through the named-callable route.
- **MO-4. Level model.** The legacy made acceptance depend on the configured level through its `RuleLevelHelper`. `master-old` changed that so each finding carries the lowest level that shows it, and one analysis yields every level's report. **The port takes PHPStan's model,** where level parameters (`checkUnionTypes`, `reportMaybes`, …) configure `RuleLevelHelper` per level. The one-analysis-every-level model is a Phase 4 idea for LSP severity grading. The level oracle runs the phar once per level, as PHPStan works.
- **MO-5. Small cost designs, as M3/M8 candidates the meter judges:**
  - hash a text once for both its shard and its entry;
  - `Name`'s `Ord` returns `Equal` on `Arc::ptr_eq`;
  - imports filed under a case-insensitive hash, so a lookup copies nothing;
  - mention bits gating per-file walks (magic constants, PHPStan API use, nested function-likes);
  - a line index built on first question;
  - a file read by its known length (`take(len + 1)`);
  - union shapes grouped by a key index;
  - `intersect`/`union` keeping non-compound members instead of re-interning them;
  - a regex's capture groups parsed once per text;
  - SQL statements parsed once per text (Phase 4);
  - a run whose level cuts a finding never formats its message (OFF-9).
- **MO-6. Process.** A line of 1,134 commits that nobody merged needed a 366 KB ledger to triage, and 212 of its rows concern a track that was itself deferred. That is X-1 at its largest. It confirms the new repo's rule of one line of work, with Phase 4 tracks started only after the parity exit.

### PIWIGO_CORPUS_TRIAGE_ab4c2b5.md (252 lines, read in full)

A triage of the legacy's 235 scoped Piwigo findings. It found 100 false positives from 22 causes and 135 stricter-than-PHPStan true positives. Real PHPStan, at level 10 with doctrine, dba and strict-rules on the same files, reported 0.

- **PT-1. Most of the 22 causes are PHPStan mechanisms the legacy had not ported**, which confirms X-4 and SBF-1:
  - `ConstantResolver` widens `PHP_SAPI`, `PHP_OS`, `PHP_EOL`, `DIRECTORY_SEPARATOR` and `PHP_INT_SIZE`;
  - `functionMap_php80delta` removes `proc_get_status`'s `false`;
  - `afterExtractCall()` runs after any include;
  - switch fall-through enters a case both directly and by falling through;
  - a join with an open shape (`array{…, ...}`) keeps that the tail may hold the key;
  - a property default is evaluated in its class (`self::X`);
  - a fact is invalidated when its subject is rebound;
  - a dynamic-key write purges nested entries;
  - `MissingTypehintCheck` skips templates with defaults;
  - a bare `exit;` terminates;
  - `use const` imports are honoured;
  - `isset` on `array{k?: int}|false` narrows;
  - `in_array` narrows against a class-constant haystack;
  - `array_filter` with `is_int(...)` or `'is_int'` narrows;
  - an assignment's `@var` applies after its right-hand side;
  - `dom.stub` property types (`documentElement` nullable) are part of the stub overlay.

  Each is an nsrt or rule test in PHPStan's suite, so the scoreboard covers it. The minimal repros (§"Minimal repros") are a ready seed for the `testing` crate's smoke fixtures.
- **PT-2. The rest are extension or Phase 4 behaviour:**
  - Pest `beforeEach` by-reference write-back;
  - Doctrine `MIN()`/`MAX()` over custom types and `HYDRATE_SINGLE_SCALAR` (phpstan-doctrine in Phase 3b);
  - SQL static-schema nullability and `mysqli::query` typing (phpstan-dba, Phase 4).
- **PT-3. "Stricter than PHPStan" findings (135: `arrayValues.list`, `function.alreadyNarrowedType` on Doctrine rows, …)** came from the legacy's own Doctrine and DBAL typing. Real PHPStan sees `mixed` there. In the port they arrive only with phpstan-doctrine's own behaviour (Phase 3b) or Phase 4. The note that such findings "would break the project's own level-10 PHPStan gate" is a real adoption cost for Phase 4's extra precision, and it argues for shipping that precision opt-in (OM-1).
- **PT-4. The corpus-context probe recipe** (symlink `vendor`, `src` and the stubs into a scratch directory, analyse only a `probe/` path) is already in memory as `reference_real_phpstan_oracle_on_piwigo`. It carries over for M7's end-to-end debugging.

- **X-4. The triage was never acted on from the active work.** Its causes are PHPStan mechanisms the port picks up from the source (PT-1).

### TOP100_PHPSTAN_EXTENSIONS.md (114 lines, read in full)

A table of the 100 most-downloaded `phpstan-extension` packages on Packagist, fetched from the search API.

- **TE-1. Phase 3b's order needs one correction: phpstan-symfony is third** (82 M downloads), ahead of doctrine (78 M) and strict-rules (78 M). The plan left it out because "it needs the container XML or a console loader". Of those two, the container XML (`symfony.containerXmlPath`) is a file the extension reads, which is data a port can read. Only `consoleApplicationLoader` executes PHP. **Revised 3b order:**
  1. phpstan-phpunit (124 M);
  2. deprecation-rules (99 M);
  3. **phpstan-symfony (82 M), with the container-XML half; the console loader is a listed exclusion;**
  4. phpstan-doctrine (78 M), with `objectManagerLoader` an exclusion;
  5. strict-rules (78 M);
  6. phpstan-mockery (30 M);
  7. phpstan-webmozart-assert (29 M).

  phpstan-beberlei-assert, phpstan-nette, phpstan-php-parser and phpstan-dibi are the other official `phpstan/*` packages in the list (all under 12 M), and they follow, ordered by downloads.
- **TE-2. The third-party extensions that matter** are larastan (70 M + 48 M under its old name), phpstan-drupal (33 M), disallowed-calls (25 M), phpstan-prophecy (21 M), symplify rules (14 M), type-coverage (14 M), phpstan-wordpress (13 M), shipmonk's dead-code-detector and rules, and phpstan-dba (2.7 M, used by Piwigo). Ported by hand, each is a project of its own size. **This is why MG-2's out-of-process extension ABI (Mago's K193: run PHP-written extensions beside the Rust core) is the main Phase 4 ecosystem item**, ahead of porting any third-party package by hand.
- **TE-3. Formatter packages** (friendly-formatter 1.5 M, ticketswap, oneline, junit, bitbucket) show that output formats are an extension point. M7 ports PHPStan's built-in formatters. The `errorFormat` service and custom formatters belong to the extension ABI in Phase 4.

### TOP100_RESEARCH_NOTES.md (1,659 lines, read in full)

The legacy's source read of 99 of the top-100 extension packages, each judged against the legacy's ROADMAP for "new finding" or "no new finding". Most are framework or single-library magic (Laravel, Magento, TYPO3, CakePHP, Drupal, WordPress, Laminas, SilverStripe, GLPI, Faker, Pest, Nextras, PHP-DI), three are abandoned with their repos deleted, and a few are stubs only. What carries over:

- **TR-1. For the Phase 3b ports, the notes are an inventory, not a spec.** phpunit: callParent, dataProvider, covers, and the assertSame family. deprecation-rules: `CallWithDeprecatedIniOptionRule` and the deprecated-ini table. strict-rules: about 45 rule files, of which the legacy covered 4. webmozart: one 896-line file with about 64 narrowing methods. beberlei: the same pattern. symfony and doctrine: framework metadata (doctrine's `ObjectMetadataResolver` instantiates the ORM; DQL needs a grammar parser, which is the `objectManagerLoader` exclusion). Each 3b port still reads the package's current release rule class by rule class (EV-5); these notes only size the work.
- **TR-2. The assertion-narrowing pattern recurs in six libraries** (webmozart, beberlei, psl, nette-tester, mockery, phony). In PHPStan each is its own `TypeSpecifyingExtension` package. The port's extension registry (M5a) must make a narrowing table cheap to add: one table per library, not code per method. That is a design check for M5a's extension API, not new scope.
- **TR-3. Third-party ideas for Phase 4, as titles only** (the legacy file keeps each with its rule class):
  - architecture: phpat's selector/relation DSL; Composer dependency hygiene (shadow, dev-in-prod, prod-only-in-dev and unused dependencies; #74, #84);
  - dead code: shipmonk dead-code-detector (with its auto-fix and its framework usage providers), unused-public, local-only-public;
  - exception hygiene: previous-exception chaining (three packages), an exception built but never thrown, unreachable catch;
  - "no mixed": member access on `mixed` (three packages);
  - enums: no `default` arm on an enum `match`, `match` over `switch`;
  - null coercion in interpolation, binary and compound-assignment operators;
  - operand-type strictness (shipmonk, voku, strict-rules);
  - `in_array` without strict across the PHP 8 loose-comparison change (voku);
  - `===` on a class with an `equals()` method;
  - wall-clock independence (`time()`, relative `strtotime`, `new DateTime()`, `date()` with no timestamp);
  - TODO-by-date and TODO-by-version;
  - PSR-3 placeholders against context keys, static messages, and an `exception` key that holds a `Throwable` (two packages);
  - no magic numbers; no `float`; typed class constants;
  - `InvokeParentHookMethodRule` (ergebnis/localheinz, with PHPUnit's hook defaults);
  - friend, namespace and test-only visibility attributes (dave-liddament);
  - PHPUnit mock hygiene (rector/mockstan: `with()` on a stub is a no-op, `atLeast(0)`, `any()`);
  - cognitive complexity, and dependency-tree complexity;
  - a baseline split per identifier;
  - formatters: a code frame, JUnit, one line per error.
- **TR-4. The notes' own frame is the legacy's.** Each "new finding" is judged against the legacy's ROADMAP and checker paths (`src/output.rs`, `member_usage.rs`), and several call a check "easy" because the legacy had a mechanism for it. In the new repo none of that exists until parity. Phase 4 re-judges each title when it is picked up. Corpus relevance ("Piwigo doesn't use Blade") is not a criterion, per the general-usage rule.
- **TR-5. This confirms TE-2.** About 60 of the 99 packages are framework or library magic that only PHP-side code can model cheaply. That is the out-of-process extension ABI's case again: running the PHP package beats porting it by hand.

### REPORT_VIEWER_PLAN.md (169 lines, read in full)

A plan for ROADMAP Phase 181: scan once, then browse and triage findings in a local web viewer, with ignore, baseline and fix writes. No row was built. It is Phase 4 item 7. What carries over:

- **RV-1. A finalizer that records why each finding was hidden (M7).** The legacy's filters dropped findings without saying which inline comment, `ignoreErrors` entry or baseline entry hid them, and its inline-ignore records were keyed by line without the comment's position. PHPStan's own finalizer keeps this. Its unmatched-ignore reporting needs to know which entries matched, and `--generate-baseline` needs counts. The port's finalizer (FR-9) keeps a per-finding "hidden by" record from the start, off by default in output. The viewer, Phase 188's counts (F13) and the LSP all read the same record.
- **RV-2. The viewer's security design, for Phase 4:**
  - it binds to loopback only;
  - a 128-bit token travels in the URL fragment and in a header on every API call;
  - Host and Origin headers are checked;
  - data never loads as a script in served mode;
  - CSP with `frame-ancestors 'none'`;
  - data is rendered only with `textContent`;
  - each write is guarded by the content hash taken at scan time, and undo works only if the file still holds what the write left.
- **RV-3. Its V1 question is answered by MO-4.** The question was whether one max-level scan filtered by level equals a real run at each level. It doesn't, because the configured level drives type acceptance through `RuleLevelHelper`. The port follows PHPStan's per-level model, so a viewer in Phase 4 runs one analysis per level, or uses the one-analysis model if Phase 4 builds it.
- **RV-4. Process.** The plan was written on top of three moving plans (level 9's H4 session redesign, LOAD-2.2, R6–R8), with "do not start V2 while either is in flight" coordination rules. That is B-6 again.

### docs/level-9 (4,994 lines, read in full)

The level-9 plan was a merged sequence of the 20 G items, the crate split's stages, the review fixes and the Composer work. It has batches, a "wall" (W2–W8), spikes S-1 to S-13 and 42 decisions spread over seven files. Part A was built. Nothing from Part B onward was built.

- **L9-1. The plan is the process failure at its peak.** It has:
  - an "Edits at approval" list that rewrites five other plans;
  - ledger rows re-pointed, "superseded" paragraphs marked in place, and `needs` chains;
  - scaffolding sized at 1,000–1,500 lines and argued against a "no-interim rule";
  - an `xtask arch` check that reads the ledger so that scaffolding can't outlive its row.

  All of this exists because the flat tree had to be retrofitted under 2,452 match sites while the old tree kept working (B1, D2). The new repo builds the flat tree in M1, before any reader exists, so none of it applies (C-3, LED-3).
- **L9-2. Part B's I1 is a ready M1 layout, measured against real counts.** M1's `docs/design/syntax.md` starts from it:
  - an immutable `Ast` that owns its source, with one typed vector per node kind;
  - raw ids as `NonZeroU32` below 2^30, so `Option<Id>` is 4 B and the top two bits hold flags;
  - a debug-only tree tag on the *public* id, not on stored ids, so node sizes are the same in debug and release;
  - **`ExprNode` 32 B and `StmtNode` 40 B, asserted with `const`**, with every record at or under its listed size and no element that `needs_drop`;
  - a per-variant payload table (47 expression and 26 statement kinds), where `MethodCall`/`StaticCall` fill the payload exactly and a new field spills to a side table;
  - records stored by value and reached only through lists (args, items, arms, params), with no id;
  - `IdentAt { NameId, at }`, an exact encoding with `dollar` and `explicit_end` bits instead of an assertion;
  - `Text { start, len }` over the source or two side buffers (decoded bytes, synthesized UTF-8);
  - a sparse doc table sorted by owner, with a `has_doc` bit per owner, which also gives closures and arrow functions their docblocks;
  - declaration nodes that reserve their id on entry, so `functions`/`classes` are in source order;
  - limits as errors: the loader refuses sources of 2^28 bytes or more, and a table past 2^30 is a `ParseError`.
- **L9-3. Part B's read API for M1:**
  - `Copy` handles (`Expr<'a>`, `Stmt<'a>`) with `kind()` returning a view enum;
  - list views with `exact::<N>()`, `prefix::<N>()`, `split_first()` and `split_prefix::<N>()` in place of slice patterns;
  - a `Visitor<'a>` over handles;
  - **`Rooted<T>` (an `Arc<Ast>` plus an id)** for the few values that outlive a walk, so no analyser type carries a tree lifetime and a bare id is only a cache key;
  - handle equality as (tree serial, id).

  The canonical dump (B8) destructures without `..` and matches without `_` (D20-9), and covers tokens, comments, marks, the line summary (UTF-8 and UTF-16 columns) and errors. That dump is the parser suite's own printer, beside the nikic-JSON comparison.
- **L9-4. Parser facts for the M1 suite and the Phase 4 LSP:**
  - an arrow typed before an existing statement takes the next token as the member name: `$x->` newline `$y = 5;` is `$x->$y = 5;`, and `Foo::` newline `bar();` is a static call (php 8.5.4). So completion triggers on the operator token and finds the innermost access whose operator is that anchor;
  - `use Foo \ Bar;` (spaces in a `use` path) is refused by PHP 8.5;
  - `clone(...)` and `exit(...)` have a callee span that differs from the node span;
  - a `finally` block, a `declare` body and a bare `exit` are *absent*, not empty;
  - a `match` arm list that is empty is `default`.
- **L9-5. Composer facts for M4 (Part A, A1/A2/A7/A8 and #40):**
  - **PHPStan's own reading is the parity target:** `ComposerHelper::getComposerJsonPath` honours the `COMPOSER` variable, and `getVendorDirFromComposerConfig` joins `config.vendor-dir` as `$root . '/' . trim($dir, '/')`, which misreads an absolute value and ignores `COMPOSER_VENDOR_DIR`. The port does what PHPStan does; Composer's own `Config` resolution (variable first, `~`, `$VAR`, absolute values) goes to `known-deviations.md` as a Phase 4 candidate;
  - Composer's constraint grammar facts (COMP-2), as tests for wherever PHPStan reads `require.php`: 5 of 1,594 manifests write a space after an operator (`>= 7`); a window inside one minor version (`>=8.0.2, <8.0.15`); a bare `8.1` is `==8.1.0.0`. PHPStan uses `composer/semver`, so the port's semver must match it, checked against `php` with the vendored library;
  - the autoload maps keep classmap keys, PSR-4 and PSR-0 prefixes (with PSR-0's `_` rule), the root package's entries included. The lazy locator (M4) needs them; dropping the root's entries was the 1,253 false `class.notFound` of FR-2;
  - `install-path` in `installed.json` is relative to `vendor/composer` (`../name`, `./name`, `null` for a metapackage, `../../web/core` for a custom installer);
  - a package's bundled `vendor/` (rector, ECS) and custom-installer packages (Drupal's `web/core`, WordPress plugins). PHPStan finds these classes by running the project's autoloader (its `AutoloadSourceLocator` hooks the real one), which a port can't do. **M4's design doc decides how the locator reaches them** (reading the bundled `vendor/composer` maps is the legacy's answer) **and lists what remains in the exclusions file.**
- **L9-6. Spike method worth keeping for M4's engine decision (LED-2):** S-2 and S-3 state the question, the method, an accept bound and the fallback before the build. S-2 asks whether salsa's macros build under `unsafe_code = deny` on every CI leg, including a release-profile link, because `inventory` registers ingredients at link time. S-3 measures a cold run as well as warm verification, since a batch run pays for every recorded edge with no incremental benefit. M4's design doc uses this form, one spike per open question, measured by the cost meter, not 13 spikes ahead of time.
- **L9-7. Lexer and parser facts for M1 (Part C, I2, I3, G3), each probed with php 8.5.4:**
  - **casts:** a cast is `(` + optional spaces/tabs + a case-insensitive cast name + optional spaces/tabs + `)`. A newline or comment inside the parentheses makes it not a cast, and `foo(string);` lexes as a cast and is a parse error;
  - **interpolation:** `"$arr[1a]"`, `"$arr[foo bar]"` and `"$arr[ 0 ]"` are refused, and `${name[expr]}` is an index on `$name`;
  - **heredoc:** a body line indented less than the closing marker, or mixing tabs and spaces there, is "Invalid body indentation level". It is an error, not a silent cut;
  - **lone `\r`:** a `//` or `#` comment ends at a lone `\r`, `?>` absorbs a lone `\r`, and a heredoc's closing label is found after one (CP-6);
  - **control bytes:** a NUL or stray control byte is "unexpected character 0x00" from 8.0 and a skipped warning before. NUL is a token, never end of file;
  - **`__halt_compiler`:** after `__halt_compiler ( ) ;` (or `?>`) nothing is lexed. It is not a halt after `->`, `::`, `function` or `const`;
  - **nesting:** 9,000 nested `[` evaluate and 10,000 fail with "memory exhausted" (likewise 40,000 parentheses). M1's deep-nesting tests use PHP's limits, and a fixed depth bound near PHP's gives the parser a memory bound as well;
  - **recovery** (for the Phase 4 LSP; parity reports only `phpstan.parse`): `foo(1, 2` followed by `$y = 3;` on the next line is one error in PHP, and keywords stay valid member names after `->`/`::`;
  - **the token stream:** a token is a kind plus a span, and text is read at the span when the parser takes it. There is no payload array. One `kinds` byte array, `spans`, and doc marks as (token index, span). A skimmed body interns and decodes nothing;
  - the line-scan kernel (`wide` SIMD, 16-byte blocks, one carry bit for `\r` at a block edge) has a stated accept/kill rule against `memchr`. Its exhaustive test is all strings up to length 6 over `{a, LF, CR, 0x80}` at offsets 0–33.
- **L9-8. Identity facts for M1/M4 (H1), each probed with php 8.5.4:**
  - methods fold case; properties and class constants don't (`$a->DOSTUFF()` runs `doStuff`, `isset($a->P)` is false);
  - an enum case is a class constant, one namespace;
  - static and instance properties share one namespace;
  - classes, interfaces, traits and enums share one namespace, and functions have another;
  - a constant's namespace folds and its last segment doesn't (`\APP\A` finds `App\A`, `\App\a` doesn't);
  - `true`/`false`/`null` fold in every spelling, even where `App\TRUE` is defined, and `define('True', 1)` fails;
  - `define('\\BAR\\X', 2)` succeeds and defines a constant no name reaches;
  - on PHP 7, `define($n, $v, true)` registers a case-insensitive constant;
  - the string key `'-9223372036854775808'` becomes the int `PHP_INT_MIN` (a canonical-key overflow trap);
  - a non-UTF-8 identifier is bytes in PHP; a lossy decode merges two names.

  **Interner design for M1** (MG-6 and AU-3 refined by H1.3):
  - **one** process-wide interner, since two interners break pointer equality;
  - liveness by count, swept at doubling and at an LSP revision boundary;
  - `Name` with an inline lowercase form, `FoldedName` with no spelling accessor, `ConstName` (namespace folded, last segment exact), and `ArrayKey` built only through its canonicalizing constructor.

  `triomphe::ThinArc` can't read a `&str` back without validation or our own `unsafe`. The fallback is `Arc<NameData>` with one `Box<str>`.
- **L9-9. PHPDoc facts for M2 (I4):**
  - phpdoc-parser's number lexemes: an optional sign, `0b`/`0o`/`0x`, `_` separators, case-insensitive, floats with a trailing dot (`1.`), and no leading-`0` octal;
  - **PHPStan converts the lexeme with `(int)`/`(float)` casts** (`(int)"0x1F"` is 0, `(int)"017"` is 17), where PHP source reads 31 and 15. The legacy chose PHP's reading. **The port takes PHPStan's, and the legacy's reading goes into `known-deviations.md`;**
  - `-9223372036854775808` in PHPDoc lexes with its sign;
  - `callable(int)` with no return type is a phpdoc-parser syntax error (2.3.6 consumes the colon unconditionally);
  - string-literal escapes follow phpdoc-parser's lexer (a backslash pairs with any non-newline character);
  - `int<a, b>` is a range only for the exact word `int` (the legacy turned `Point<1, 2>` into a range);
  - `array<*, int>` is `array<mixed, int>`, because keyword generics ignore variance;
  - `?mixed` is `mixed`;
  - `number`, `scalar` and `numeric` yield to a class or `use` alias of that name (`tryResolvePseudoTypeClassType`);
  - a written union of n members must not cost n²: dedupe by hash before normalizing. A 50,000-member union and a 100,000-deep nest are M2/M3 cost inputs.
- **L9-10. If M4's design doc picks salsa (LED-2), Part D's salsa reading is the checklist** (read from salsa 0.28.5's source, never run):
  - **backdating keeps the old value when the new one compares equal.** A result type's `PartialEq` must therefore be exact: types by node, names by pointer, floats by bits. PHPStan's semantic `equals()` is a separate method (FR-3 already says so);
  - **results that other files' queries read hold no position.** Positions live in a per-file site table; otherwise one inserted line invalidates every reader below it;
  - every member of a cycle uses one strategy (fallback or fixpoint), and a panic inside a `cycle_result` query poisons it until the next revision;
  - salsa panics at 200 fixpoint iterations, so a widening that is a function of the value alone (`LOOP_SCOPE_ITERATIONS`-style generalization) must converge well under that;
  - `deserialize` isn't atomic, so a failed restore drops the database;
  - one handle per thread, and `cancel_others` waits with no timeout;
  - no `rayon` inside a query;
  - extensions read the world only through handles that record the read.

  **For parity none of this is needed:** PHPStan's analysis is a batch run with a result cache (FR-9), and lazy `OnceLock` memos (FR-4) are the parity design. The engine question is M7's result cache plus Phase 4's LSP. **So M4's design doc decides the engine, but defaults to plain memos unless the M7 result-cache design needs more.**
- **L9-11. Small M5a/M6/M7 facts:**
  - **messages:** a `message!`-style deferred message, where a dropped report never formats and a rewound loop pass discards pending reports unformatted. PHPStan's `RuleErrorBuilder` takes a string; the port can keep the string API for extensions while its own rules defer (D25-6, OFF-9);
  - **anonymous classes** are named `class@anonymous/{path}:{line}[:{i}]`. The port takes PHPStan's exact naming and its own handling of paths with `:digits`;
  - **`RemoveUnusedCodeByPhpVersionIdVisitor`** rewrites `PHP_VERSION_ID` conditions by the configured version. It is a parser visitor M5a ports (FR-6);
  - **Composer's `ClassLoader::findFile` order** (classmap, longest PSR-4 prefix, first matching directory, then PSR-0) is the lazy locator's spec. M4 tests it against `php` running Composer's own loader;
  - APFS/HFS+ look names up insensitive to Unicode normalization (NFC/NFD), which the macOS leg tests.
- **L9-12. Beyond PHPStan, so Phase 4:**
  - per-version syntax "marks" bounding a file's PHP range;
  - per-owning-manifest PHP ranges;
  - `VersionSet`/`Readings` builtin data built from php-src stubs plus real-binary Reflection dumps for 7.0–7.4 (`xtask stubs-dump`), with failure values probed per function;
  - version-sensitive operator semantics (`"abc" * 1` throws, `"5 apples" * 2` warns);
  - checking an analysed file's parseable parts despite parse errors, with its 95th-percentile noise bar;
  - the Composer `Config`-exact vendor-dir resolution (L9-5);
  - the SQL `sql_columns` query.

  The parity port takes PHPStan's single `phpVersion` (or range) handling.
- **L9-13. Process.** The README says "the ledger is the only record of each task's state". The plan stacks:
  - 13 spikes;
  - 42 decisions, several re-wording earlier decisions ("decision 7's wording changes", "decision 18's amended reading");
  - "Left as it is" notes;
  - "a scaffold cannot outlive its row" checks.

  Of all this, Part A and seven server fixes were built. It is the clearest case for the new repo's rules: a design doc of at most about 300 lines per topic, decisions recorded once in `PLAN.md`, and no ledger.

- **X-3. Level 9 is B-6 again:** a merged plan that sequenced performance items, crate-split stages, review fixes and Composer work into batches.

### README.md (182) and CLAUDE.md (111), read in full

- **RM-1. Distribution facts for Phase 1's release workflow:**
  - five targets: x86_64 and aarch64 Linux gnu, aarch64 and x86_64 macOS, and x86_64 Windows msvc;
  - each asset is `.gz` with a `.sha256`, and the README gives verify-and-unpack recipes per OS (PowerShell's `GzipStream` included);
  - the Linux binaries link the runner image's glibc (2.43 on `ubuntu-26.04`), so they don't run on Ubuntu 24.04 (2.39). **The new repo builds on an older base or for musl** (MG-2's K301 moves up to Phase 1, since a binary that doesn't run on current LTS distros blocks the end-to-end scoreboard on CI machines and users alike);
  - the macOS binaries aren't signed. The VS Code extension's downloader verifies the checksum and strips the quarantine attribute (Phase 4 quarry);
  - a nightly toolchain is pinned (`-Z threads=8`), clang and mold on x86_64 Linux, and `RUSTFLAGS` overrides the config flags. **Phase 1 decides stable vs nightly** from what the workspace really needs: the parallel front end is a build-speed choice, not a requirement, and a stable toolchain removes a class of churn;
  - the default `pcre-validate` feature builds PCRE2 from vendored C source via `cc`, so a C toolchain is needed (UN-1 makes it static and mandatory, since `regexp.pattern` parity needs it).
- **RM-2. The project CLAUDE.md's profiling section carries over almost as is:**
  - frame-pointer `dhat` profile in its own target dir;
  - `perf` at 499 Hz, run once before recording for warm caches, and always `--no-inline`;
  - the serial-window bucketing, the scaling-curve fit, and callgrind for short runs with one file per thread;
  - the traps: `pkill -f` kills its own shell, no `timeout` wrappers, no measurement `eprintln!`s committed, one heavy job at a time.

  Two edits: the timing rule changes from hyperfine A/B rounds to `abab.py`'s paired Wilcoxon runs (IT-4, LED-6), and the workloads table becomes `perf/workloads.toml`'s names. Its first section (`WORK_STACK.md`) is replaced by `STATUS.md`.

### PHP_SEMANTICS_CATALOG_PLAN.md (1,072 lines, read in full)

A design for deriving builtin behaviour from php-src itself. The producer is LLVM IR plus Clang metadata. CIR covers 44 of 523 units, and SVF on the whole link hits the 10 GiB cap. Z3 runs over SMT-LIB in a child process. The schema is a `BehaviorGraph` with Closed/Open coverage per domain and evidence records, where an observation never justifies a negative fact. The checker would be rewritten as `PhpFlowIR` plus a `SemanticHost`, and `PhpRelease` adds patch-level versions. The task list runs F00–F06 and then C00–C62. Its status is DESIGN, and its Phase F gate was never passed.

- **PC-1. This confirms X-2: the whole catalog is Phase 4.** The parity port's builtin knowledge is PHPStan's (`functionMap*`, `functionMetadata.php`, the `.stub` files, `src/Type/Php`).
- **PC-2. Points the port gets from PHPStan's own mechanisms, which serve as cross-checks:**
  - named arguments normalized before any builtin handler runs (MG-9);
  - evaluation order of arguments and implicit calls (`processArgs`);
  - `PhpVersion` carries the patch (EV-1).

  Two of its rules go beyond PHPStan and are Phase 4 design rules: an observation can't prove an absence, and a missing `@throws` is no proof that nothing throws.
- **PC-3. Process.** A 1,072-line design with a 60-task roadmap whose own first gate never closed. It is P-8 and L9-13 again.

- **X-2. For parity, PHPStan's own builtin knowledge is the spec:**
  - `functionMap.php` and its deltas;
  - `functionMetadata.php`;
  - `stubs/*.stub` and phpstorm-stubs;
  - `src/Type/Php`.

  The catalog is a Phase 4 project that refines that knowledge where PHPStan is wrong at runtime (re-scoped by PE-1). The related builtin-runs table had 1,363 coverage gaps.

### PHP_SEMANTICS_CATALOG_LEDGER.md (1,014 lines, read in full)

The status record of Phase F: about 30 F tasks and 50 R rows, each with acceptance lines, "Investigation, 2026-10-23" notes, five direct-validation passes over agent workflows, and the owner-decision follow-ups of 2026-10-24 (the module digest, SVF repinning, CBMC reruns, regenerating the builtin-runs table, Imagick crashes). The only thing it built end to end is one function, `strlen` at PHP 8.5.4, through a 171 KB artifact.

- **PL-1. The cost of rigour without a finish line.** For one builtin, the ledger tracks a CBMC model with 93 properties, an LLVM inventory of 241 instructions, paired TAILCALL/CALL runtime witnesses, a cleanup-replacement matrix and a certificate replay. Its own roll-up still reads F00–F06 "Open". **For the new repo:** rigour attaches to a scoreboard number. Evidence that serves no test the scoreboard counts is Phase 4 research and is time-boxed as such.
- **PL-2. Agent workflows created most of the ledger's churn.** Five "direct validation" passes exist to check agent investigators and validators, and they found investigator counts wrong, validators disagreeing, "unverified" verdicts, and git refused in worktrees. This confirms the global no-agents rule and L3-1. The new repo's process has no workflow-derived status.
- **PL-3. Reproducibility lessons for the cost meter and any pinned artifact (Phase 2):**
  - a pinned digest the locked inputs couldn't reproduce: 24 of 525 bitcode objects differed for an unrecorded input;
  - a committed witness that was stale against the pin;
  - a lock that claimed a digest it didn't contain;
  - an SVF pin on an unreleased commit whose binaries no one held.

  **Rule:** every pinned input (phar, php-parser, phpdoc-parser, the PHPStan submodule) is a released version fetched by digest, and CI regenerates every derived artifact and compares it.
- **PL-4. Operational facts:**
  - an orphaned `openssl s_server` from php-src's curl tests kept `heavy.lock` held for over an hour, so a lock-guarded job checks for leftover processes;
  - the builtin-runs recorder's stale sidecar hash meant the table couldn't be reproduced by the current recorder;
  - the recorder's probes crash ImageMagick in invalid states.

  All Phase 4 (the recorder is MO-1).
- **PL-5. PHP facts it probed, as runtime-oracle inputs for Phase 4:**
  - PHP 8.5.4 emits the null-to-internal-scalar deprecation for `str_repeat` and `strtoupper`, not only `strlen`;
  - a destructor's exception replaces a pending one and links it as `getPrevious()`;
  - fatal bailout skips the caller's `finally`;
  - `foreach` over an `Iterator` calls `rewind`/`valid`;
  - `in_array` with a string haystack is a `TypeError`;
  - an unserialized discarded object runs `__destruct`;
  - `include 'php://filter/...'` runs a user filter;
  - a generator's locals' destructors run when the generator is destroyed.

  Where PHPStan models the behaviour (the deprecation at PHP 8.1+ through its function signatures), the scoreboard covers it.

### PHP_SEMANTICS_CATALOG_CONTRACT.md (349) and _CONTRACT_REVIEW.md (1,463), read in full

The contract defines a host-relative soundness claim (concretizations `gammaV`/`gammaS`, five simulation obligations, a finite-path induction), the five completion kinds and the cleanup-replacement cells, coverage composition, and dependency and assumption rules. It ends with a table of 22 compositions the evaluator must reject. The review is five passes plus author remediations (RV-1 to RV-31, ACR-1 to ACR-6), and ends with an "acceptance policy" (2026-10-12) that drops the independent review it had required until then.

- **CC-1. Design principles worth keeping for Phase 4, and already true of PHPStan's `TrinaryLogic`/`mixed`:**
  - missing knowledge is never a negative fact;
  - an unknown transfer havocs what it may touch;
  - uncertainty propagates through every dependency edge;
  - a join can add behaviour but never remove it.

  In the port these are PHPStan's own rules (`Maybe`, `mixed`, `invalidateExpression`). For parity no second formalism is built.
- **CC-2. Legacy checker bugs the review reproduced, as parity facts the scoreboard settles:**
  - a bare `object`, an object shape, an intersection or an object-bounded template may be `Stringable` (RV-9);
  - a `__toString` from a trait makes the class `Stringable` (RV-17; `zend_inheritance.c`);
  - `@template T of string|null` keeps the null case (the nullable-template gap);
  - a `Stringable` is accepted for every `string` parameter in weak mode (`str_repeat($w, 2)`).

  PHPStan's `accepts` and its `Stringable` handling decide each, and the rules suite checks them.
- **CC-3. RV-14 is a general Rust rule.** A side-effecting call inside `debug_assert!` dropped `@method`/`@mixin` members in release builds, and only a release-profile run found it. **For the new repo:**
  - clippy's `debug_assert_with_mut_call` is denied;
  - CI runs the test suite in an optimized profile as well as debug at least once per push, because the scoreboard and the cost meter use optimized builds.
- **CC-4. PHP facts from the source read** (8.5.4, for the M1/M5 parser and analyser suites):
  - a single plain-argument `strlen` compiles to the `ZEND_STRLEN` opcode and doesn't call `zif_strlen` (no observable difference);
  - a destructor during a pending throw can return, throw, `exit()`, fatal or suspend;
  - a non-string `__toString` return is a thrown `Error`.

  Phase 4 runtime-oracle inputs.
- **CC-5. Process.** The review needed five passes to judge one function's model, and kept correcting itself (RV-24's mechanism retracted, the "byte-for-byte" claim checked on the wrong tree, and RV-23: about 50 uncommitted files with claims in them). It ends by redefining acceptance so the author can accept. This is R-3 and L3-1 at their sharpest. **The new repo has no acceptance step separate from tests; a committed scoreboard number is the acceptance.**

### PHP_SEMANTICS_CATALOG_EVALUATION (783), _ALTERNATIVES (173), _COMPARISON (149), read in full

These are the measured evidence behind the catalog. The minimal PHP CLI links 525 objects into 14,142 functions and 3.7 M LLVM instructions, with 4,642 indirect call sites. `zif_strlen` alone reaches 996 native definitions through direct calls. LLVM plus Clang metadata was chosen as the producer, because Clang's AST CFG drops a C++ exceptional destructor edge and CIR emits only 44 of 523 units. Every pointer analysis that tells builtins apart (SVF, sea-dsa, Yapall, Phasar union-find) dies at the 10 GiB cap. The sound ones that finish (CodeQL with field resolution, Phasar CFL-Steensgaard) conclude that "every builtin may run every magic method".

- **PE-1. The decisive result for Phase 4's runtime-truth work (ALTERNATIVES):** deriving builtin behaviour from php-src's C doesn't scale. **The route that worked cheaply was observation.** A PHP observer extension traced php-src's own `.phpt` suite (21,836 tests, 519 s) and found 217 builtin-runs-user-code cases the table lacked. Among them:
  - property hooks run by `get_object_vars`, `json_encode`, `var_export` and Reflection;
  - `array_column` running `__isset` then `__get`;
  - stream-wrapper `rename`;
  - callbacks inside `filter_var` options and `PDO::FETCH_FUNC`;
  - `__debugInfo` at `var_dump`'s variadic positions.

  **Phase 4's catalog item is re-scoped by this:** the per-parameter type bound plus tracing real test suites, with native analysis dropped. The catalog plan's F/C task list is not resumed.
- **PE-2. PHPStan's position:** PHPStan doesn't model a builtin running user code (beyond `@param-immediately-invoked-callable` and the callable-parameter extensions). So none of this is parity scope. It is Phase 4's "effects" title, together with master-old's effects engine (MO-1).
- **PE-3. Measurement facts worth keeping:**
  - a PHP build's VM kind is chosen by compiler capability (TAILCALL vs CALL), so a pinned PHP must record its generated configuration, not just its flags;
  - three sequential exports were byte-identical, so the toolchains are deterministic and drift means an input changed.

  This supports PL-3's rule.

### php-catalog WORK_STACK.md (254) and BUILTIN_RUNS_RECORDER_PLAN.md (408), read in full

The catalog branch's stack, as of 2026-10-12, has three checkouts (`master`, `master2`, `master3`) and a resume order of level 9 → builtin-runs gaps → level 7 → the catalog → levels 3, 2, 1. The recorder plan built `tools/builtin-runs`, a sandboxed PHP script that calls every builtin in about 800 K forked children (10,725-row table, steps 1–12 done). It records its design under fourteen adversarial passes.

- **WS-1. Phase 0 has more to sweep than the plan listed.**
  - `~/projects/phpstan-rs-kept` exists. It isn't a git repository but a folder of kept scratch data (`prs-profile`, `prs-probe`, `php-alt-exp`, …).
  - The uncommitted E2.V edits, which the stack says "survive only in `~/.cache/prs-profile/engine-new/`", are gone: that directory no longer exists.

  Phase 0's freeze lists every directory under `~/projects` and `~/.cache` that holds legacy work, archives each one that has unique data, and records what was already lost.
- **WS-2. The HEAD drill's summary is the cost meter's case in one line:** "the batch's total instructions fell 8.8% in off mode, but its peak memory rose 68%". An Ir-only gate would have accepted it. The meter gates Ir **and** peak and live heap together (Phase 2 already does).
- **BR-3. Operational rules from the recorder's runs, for xtask and the test harness:**
  - **every generated file is written to a temporary file and renamed** (`score.json`, `history.tsv`, baselines). A full disk truncated a plan file to 0 bytes through an in-place write;
  - **a test that starts a container or a long-lived process must clean it up.** An orphaned `runtime_oracle` Docker container held a core for 1 h 48 min and wrote a 34.5 GB file that filled `/`;
  - `--stats`-style runs record `/proc/loadavg` and free space at start and end.
- **BR-4. PHP facts for the Phase 4 effects work:**
  - `proc_open` stringifies the elements of its array arguments, so an "asserted runs nothing" row was wrong;
  - PHP 8.5's allocator refreshes its heap key in every forked child, so a fork's cost tracks the parent's peak heap (`USE_ZEND_ALLOC=0` halves it);
  - `XMLWriter::openUri` keeps a copy of its URL string and runs the wrapper on a later `flush`.

  All Phase 4 (MO-1).
- **BR-5. Process.** A 408-line plan for a PHP script, with fourteen passes of claims attacked and re-verdicted ("refuted", "changed", "holds"). It is P-8 at small scale. The scripts themselves are fine. The new repo's xtask tools are designed in at most one short design doc and judged by their outputs.

### PIWIGO_OPTIMIZATION_PLAN, _PLAN_PASS2, _VALIDATION, _PASS2_VALIDATION, _PROPERNESS_AUDIT, _FINAL_PLAN (934 lines in all), read in full

This is the drill before the 25 G one. It took Piwigo from 45.13 G to 25.14 G (output byte-identical, 4.0 s → 2.6 s). Its plan was written, validated, written again, validated again, audited three times for "properness" and then walked item by item.

- **PO-1. Lazy vendor loading was built, measured and then removed. That matters for M4.**
  - The measurements: during checking, 435 of 10,504 vendor classes (4.1%) were looked up, and 441 of 7,203 context files were touched. The checker read no vendor body. 25.14 → 21.35 G.
  - It was removed on review because "what is not loaded is not seen": a class the autoloader can't find by name (in a file shared with another class, a function in a file the autoloader doesn't list, or a vendor class extending a project class) went missing silently.
  - **For the port, PHPStan's locator chain is the specification, not the legacy's eager load.** PHPStan scans `paths`/`scanFiles`/`scanDirectories` with `OptimizedDirectorySourceLocator`, a cheap regex pass for declared names with no full parse, and finds vendor classes through Composer's maps or the real autoloader. What PHPStan can't locate is `class.notFound` there too.
  - So M4's lazy design follows PHPStan's locators exactly, and the scoreboard checks it. Any class the legacy's eager load found and PHPStan doesn't is a `known-deviations.md` entry, not a hidden drop.
  - This is a point where an earlier user ruling ("a run that can lose a finding without saying so is not wanted") and the parity rule meet. **Raised with the user before M4 starts** (see the open questions).
- **PO-2. Workload shapes for the cost meter:**
  - vendor `const` arrays of 8,137, 5,101 and 1,874 items (`PhpStormStubsMap.php`, the mime maps) made shape building quadratic (2.66 G). A **large constant-array literal** joins the `deep-*` group. PHPStan's `ConstantArrayTypeBuilder` degrades past a size limit, and the port takes that limit.
  - pairwise `reduce(Type::union)` over many members thrashed a 65 K memo, so M3's n-ary union (D25-4) is confirmed.
- **PO-3. Facts measured on a real run, for M1/M5 design defaults:**
  - 55% of tokens are inside function bodies;
  - doc comments are 28% of source bytes, and half are duplicates (101,632 comments, 51,095 distinct texts);
  - `parse_written_type` had 310,883 calls over 13,198 distinct strings (a 95.8% memo hit rate);
  - method lookups: 813,653 calls over 12,912 distinct pairs;
  - 83% of loops converge in one pass (about 1.3 on average);
  - 94% of keys a branch merge visits are unchanged in both branches.

  These support D25-4 (PHPDoc memo by text), MG-8 (merge only changed keys) and FR-6 (the loop budget).
- **PO-4. Errors the per-commit meter would have caught.**
  - The interner's new hash table doubled name lookup (1.0 → 2.0 G) because dead entries piled up; only a checkpoint callgrind run found it.
  - One claim ("the CLI never reads the reverse dependency index") was found wrong only while implementing.

  This is L3-1 and D25-1 again.
- **PO-5. Process.** A six-file chain of plan → validation → pass 2 → validation → "properness audit" in three passes → final plan, each superseding the last "where they differ". It is the legacy's pattern in miniature. The new repo states the change, builds it and reads the meter.

### PROFILE_DRILL_LIBSUITE (276) and _LIBSUITE_RELEASE (349), read in full

Callgrind runs of the legacy's library test suite (1,000 tests), at opt-level 1 (31.4 G) and opt-level 3 with thin LTO (20.6 G).

- **LS-1. One differential test was 68–72% of the whole suite.** `edits_across_every_fixture_match_a_fresh_load` replays edits over every fixture and compares each against a fresh load, on its own thread pool. **For MG-12's incremental-vs-fresh test in the new repo:**
  - the per-push run takes a fixed sample of fixtures and edits;
  - the full sweep runs at milestone ends;
  - the test suite itself is a cost-meter workload (`suite-lib`), so a test that grows the suite tenfold shows up on its commit.
- **LS-2. Profile shape doesn't depend on the opt level, but absolute numbers do.** Every pass scaled by about 0.65 from opt 1 to opt 3. The costs that didn't move were memcpy, memcmp, allocation and cloning, which only less work removes. This confirms P-7: the meter uses one fixed profile, and its numbers are compared only within that profile.
- **LS-3. The deep-clone design cost is measured, which confirms D20-5 and FR-4.**
  - Every scoped re-run of the finishing passes deep-cloned each class from its raw copy (`ClassReflection::clone` 6.6%).
  - It then compared every class field by field against a snapshot to find what changed (6%) and dropped the clones (5%): 18% of the suite in snapshot bookkeeping.
  - The new repo's declarations are immutable and `Arc`-shared from the start, and derived facts are lazy memos, so there is nothing to reset or compare.
- **LS-4. A first touch pays for everyone.** Process-wide lazy stub decode is charged to whichever test touches an entry first, so per-test costs are not comparable. The cost meter's test-suite workload is measured whole, never per test.

### PROFILE_DRILL_PIWIGO.md (catalog, 250 lines, read in full)

The earliest drill (45.13 G), the source data of the PIWIGO_OPTIMIZATION docs. Most of it is PO-1 to PO-5 again. What it adds:

- **PD-1. Phase shares at the start:** parse plus declaration indexing 47.8%, checking 37.4%, finishing passes 6.9%, the serial main thread 5.8%. Front-end cost dominated before any optimization, which is why the M1 workload exists from the first commit.
- **PD-2. One large vendor constant array put 2.5 G on one worker.** A single huge literal skews a parallel stage. That supports MG-11's largest-first or fine-grained scheduling, and PO-2's large-constant-array workload.
- **PD-3. Repeated whole-tree work:** seven or more AST walks per file, `line_col_at` twice per token, `always_terminates` re-inferred (2.88 G). The port's answers are D25-3 (the parser records what the walks computed), D20-3 (offsets, lines on demand) and D25-5 (reachability from statement results).

### PROFILE_DRILL_PIWIGO_9071_BATCH_PLAN.md (catalog, 468 lines, read in full)

A batch of about 40 items (A–AE plus small rows) against the legacy's effects engine at 42.2 G warm / 54.13 G off, with every function over 20 M given a section, built in one go and measured once at the end (E2.V). Most items fix the effects engine, which is master-old/catalog structure (MO-1) and Phase 4. What carries over:

- **9B-1. The batch is the strongest case for the per-commit meter.** Savings were "deliberately not part of this plan" and measured only at the end. The result: off −8.8% Ir, warm −6.6%, but **peak heap +68% and heap at exit +71%**, wall time inside the noise, and Piwigo output 611 → 613 findings with two new findings nobody could attribute. "The memory growth is not explained yet." With about 40 items in one measurement, no regression could be tied to its commit. The meter gates Ir and heap on every commit (WS-2), and the scoreboard catches output changes on the commit that makes them.
- **9B-2. Cache mode is part of a workload's definition.** The same code measured 54.13 G with the file cache off, 42.14 G warm and 56.24 G cold-with-store, and an earlier baseline (20.57 G) turned out to be a different mode of older code, so two series had been compared that weren't comparable. **`perf/workloads.toml` names each workload's cache mode (off by default) and its warm-up, and the meter never compares across modes.**
- **9B-3. A comparison reference must be regenerable, not a file.** The batch's reference output `run-631ad001.json` was "no longer on disk" when the batch was verified. The scoreboard's and the meter's baselines are committed (`score.json`, `history.tsv`), and any reference output is reproducible from a tagged commit.
- **9B-4. Oracles caught both defects of the batch.** The `oracle` profile ran the old path beside each new one over the corpus and gave 35 debug-assertion panics from two bugs (a fused body-walk changing parameter-use order, and a structural dedupe). **This refines D20-9:** a comparison against the previous behaviour is valuable while a change is being made. It belongs in a test or a throwaway comparison run against the previous commit's dump, and is deleted with the commit that lands the change, never kept as resident code.
- **9B-5. Memo hit-rate counters with a removal rule (Phase 2 and every milestone).** An always-on `stats` facility (thread-local counters per memo site, printed with `PHPSTAN_RS_MEMO_STATS=1`) reported each memo's hit rate. A memo under about 20% hits is removed, and the measured rate is recorded either way. Two memos were withdrawn on measurement (a call-transcript memo at 12%). **The port adds a memo only with a counter, and `xtask cost` prints the counters with each workload.**
- **9B-6. Design facts for the port, each measured here:**
  - **M1:** token access was 1.0 G of the parser's 1.8 G, from a binary search for each token's payload on every `peek()`; L9-7's kind-plus-span stream with no payload table avoids it, and the parser reads a token's kind without building the token. A doc comment's text hash is computed in the lexer's scan, so the PHPDoc memo probes without rehashing. A file's content hash is computed once and handed to every consumer (the legacy hashed each file three times). A file is read by its known length from one `fstat` (`read_to_end` added an `fstat` and an `lseek` per file).
  - **M3:** a relation memo cleared on every generation change spent 621 M in `memset` and element drops, because `HashMap::clear()` zeroes a grown table's whole control array. Put the generation in the key (D20-6), or drop and replace a large table. An `accepts` memo key includes the relation settings it reads (`checkBenevolentUnionTypes`, strict mixed arrays), the F33 rule. The union of many array shapes merges only shapes that share a key, through a key index; whatever PHPStan's `TypeCombinator` does is ported first and the meter judges it.
  - **M5a/M7:** LSP-only data (semantic tokens) is never built during a CLI run. A diagnostic below the level cutoff returns before its range is computed (98.5% of 41,718 built diagnostics were dropped later). A file's role is found by looking its ancestors up in a map of roots, not by trying every root in turn (55 `strip_prefix` per file). The line index is built on first use.
  - **M7:** a stat-or-mtime shortcut for change detection is unsound (`rsync -t`, `tar` and composer installs rewrite with the same size and time), so change detection uses the content hash.
- **9B-7. Effects-engine facts kept for Phase 4 by pointer:** `call_targets` has side effects, so memos may replace only its lookups; a receiver read from partially solved state can't be memoized; settling order is independent only for non-"anything" entries. These are in the catalog tag with the plan.

### PROFILE_DRILL_PIWIGO_HEAD_LEDGER.md (catalog, 283 lines, read in full)

The follow-up to the 9071 batch: a measurement protocol, a run register (C1–C10), paired heap runs, nine open tasks (O1–O9) and four questions. O1 (a per-class memo of `destroy_held`) cut 7.71 G (−16.5%). The rest is effects-engine work (Phase 4 by pointer, as 9B-7).

- **HL-1. Ir moved 6–8 G between runs of the same binary because of on-disk cache state.** The stub cache kept the three most recent builds, so a run whose stub files had been pruned rebuilt them (C1 49.36 G warm against C2 55.33 G cold, same binary). Unpaired runs with no recorded stub state had to be thrown out (C3, C4). **The cost meter controls every on-disk cache:** each workload runs with a private cache directory that it either leaves empty (off) or fills in a warm-up and then checks unchanged after the run (the CLAUDE.md `find -newer` check, IT-4). A run that rewrote its cache is invalid, not a data point. Together with 9B-2 this makes the meter's history comparable across commits without pairing.
- **HL-2. Attribution errors in reading a profile.** Three "findings" were withdrawn on recheck: a 40.9 G rayon figure from an aggregation that double-counted (its self cost was 0.07 G), a −2.8 G "removed function" that was only inlined elsewhere, and two commits wrongly blamed. **For the meter's top-20 diff:** it lists self cost by function and states that names move with inlining. Inclusive cost is quoted only for one callee and never summed (OFF-10).
- **HL-3. Memo invalidation is tested by mutation.** Each reset point of the O1 memo got a test, and removing each reset in turn failed its own test while the others passed. **For the port's memos (M3/M4/M7):** each invalidation point has a test, and the memo-key rule (OFF-4, SBF-5) applies.
- **HL-4. Wall-time scaling stopped at four threads** on this VM for both binaries (HEAD: about 2.1 s serial and 10.4 s parallel at one thread), with the load not logged. The scaling curve (1, 2, 4, 8 threads) is an informational series of the M7/M8 workloads, with the load average recorded (IT-3: shared host cores).
- **HL-5. Where the heap peak sat:** the expression arena (78.5 MB), implicit-call records (about 98 MB, effects), a PHPDoc tag memo running at its 16,384-entry bound with a 45% hit rate (57.8 MB), and per-file token streams. That confirms D20-4 (trees die with their check) and OH-2 (memos bounded by weight and owned by a session). The memo had held 8 MB a few weeks earlier, and nobody knew why it grew, which is what a per-commit heap gate would have caught.
- **HL-6. Process.** Wall-time checks were "waived, because other agents' load makes it unreliable". Several sessions on one machine made wall time unmeasurable. That is one more reason for the one-session rule and for the meter gating on deterministic counts.

### PROFILE_DRILL_PIWIGO_20G_LEDGER.md (catalog, 1,141 lines, read in full)

The catalog branch's ledger for building the 20 G plan: items 0–19 split into several hundred rows (down to five levels, `9.7.a.d.c.a`), then item E (effects: E1 correctness, 664 → 611 findings; E2 a 130 G Ir regression), gaps EG.1–EG.33, lint debt EL.1–EL.7, and a final validation that never ran. Items 0–9 were built, item 10 half, 11–19 never.

- **20L-1. Performance was measured only at the end, and it collapsed in between.** Item gates through 9.V were "tests-only" by decision. An out-of-band release sweep found the warm Piwigo run going 4.59 s (3.V) → 9.78 s (4.V) → over 60 s from 6.V on, unnoticed for six items. The effects work (E) separately reached 326 G Ir (from 20.57 G) before E2 brought it back to 42 G. This is the decisive case for P-6 and 9B-1: **the cost meter runs on every commit, from the first one, and a gate that only runs tests isn't a cost gate.**
- **20L-2. Rows were marked DONE before their tests ran.** Dozens of item 9 rows read "DONE (unvalidated; tests added but unrun)", and the 9.V snapshot didn't build in release. **Rule for the new repo:** work is done when its tests have run and passed in CI. `STATUS.md` has no "done, unvalidated" state.
- **20L-3. No baseline existed when it was needed.** Item 0's validation, which was to record "item 1's reference", was skipped, so "item 1's validation has no baseline to compare with". The scoreboard and the meter record their baselines in Phase 2, before any port code (already in the plan), and they're never skipped.
- **20L-4. A performance plan absorbed correctness and catalog work.** The ledger says "only what optimizes", yet it holds item 0.1's effects rewrite, item E's 664-vs-581 correctness work, and catalog rows covering every PHP version from 7.0 to 8.5 at every patch release, Windows' MSVC build, macOS, glibc from Debian source and PECL. Each new row pulled in more (B-6). **In the port, correctness is the scoreboard's job and cost is the meter's.** Both run per commit, and no plan mixes them.
- **20L-5. Determinism of cache files.** EG.6: a stored cache entry didn't read back as the same bytes, because name maps and sets were encoded in hash order. **M4/M7 rule:** serialized maps and sets encode in key order, and a round-trip test asserts byte equality.
- **20L-6. The effects engine's cost, measured against PHPStan.** E1.V.2 ended at 611 findings where the older code gave 581 and real PHPStan gives none of the extras. Each of the 30 extra rows was a fact dropped by an unknown effect: 9,983 of 10,962 unknown effects came from unprobed stub classes. A beyond-PHPStan analysis that forgets facts on uncertain calls costs precision across the board until its model is complete. That is a Phase 4 design constraint: OM-1's opt-in rule applies to the effects engine too.
- **20L-7. PHP lexer facts for M1's suite** (rows 10–12, deferred and never built): a doc comment is `/**` followed by whitespace (`/**x*/` and `/***/` are plain comments); `<?php` is an open tag only before whitespace; `<?` depends on `short_open_tag`; a pending doc comment is cleared only by `}` (PHP's own rule for which declaration gets it); casts are single tokens; `__halt_compiler` ends lexing and `__COMPILER_HALT_OFFSET__` is that file's offset; heredoc indentation errors. **The lexer's oracle is `token_get_all`** (row V.0.c.a), beside nikic's AST for the parser. For parity, nikic's lexer emulation decides where PHP versions differ.
- **20L-8. The change-detection choice.** Row 17.1 planned a stat index with a "racy file" rule for warm runs, and the 9071 plan later rejected stat shortcuts as unsound (9B-6). M7's result cache uses content hashes, and a stat index only as a hint verified by the hash.
- **20L-9. Process.** Every row was split into child rows "for each independently verifiable deliverable" before work started, with a gate row per test selection: hundreds of rows for 19 items, of which half were never reached. The ledger's last section is a timing sweep where runs were "censored" at a 60-second cap under contention from other jobs. It is L-3 and X-1 at their largest.

### PROFILE_DRILL_PIWIGO_20G.md (catalog version, 3,354 lines; the 1,880 lines not in master's copy read in full via `diff`)

The catalog copy adds item 0's oracle findings, a "fast validation" section, the native-catalog design, the design rationale for the 20 G items, and Appendix B (effects: E1 correctness, E2 the 130 G regression, B.3 open-gap designs). Most of it is Phase 4 or legacy structure. What carries over:

- **C20-1. Parity facts for M1/M4, each probed with php:**
  - a non-UTF-8 identifier is its bytes (`class \xA9` and `class \xAA` are two classes; 3 Piwigo files, 2 vendor files), so no lossy decode of names (L9-8);
  - a lone `\r` ends lines, `//` comments, the newline `?>` takes and heredoc lines, checked with a `__LINE__` probe (CP-6);
  - casts allow only spaces and tabs inside the parentheses; PHP 8 refuses whitespace inside qualified names;
  - in a file with both a lexer and a parser error, the first one in source order is reported;
  - declaration binding after an early top-level `return`/`exit`: classes PHP binds at compile time exist, run-time-bound ones (with an unknown parent, conditional) don't (D25-2);
  - a doc comment goes to a declaration by the `}` rule (20L-7), checked with `getDocComment()`;
  - evaluation order: `$a[$i++] = $i` gives `[0 => 1]`; in a destructuring assignment the value is evaluated first;
  - `call_user_func('extract')` compiles to a direct call, so PHPStan's `afterExtractCall` handling must see it;
  - phpdoc-parser's lexer byte classes (`Price€` is one identifier), `//` inside a PHPDoc string literal, `Integer::MAX` as a class constant and not the `int` keyword, `@phan-real-return` read as a return tag. Each is phpdoc-parser's behaviour, so the M2 suite checks it.
- **C20-2. The union cap again.** Piwigo had more than 64 class-strings in one union, and the legacy's cap turned it into `mixed`. PHPStan's `TypeCombinator` has its own rules for large unions; the port takes them, and a 64+ class-string union is an M3 test (F6).
- **C20-3. Incremental staleness cases** found by item 0's oracle runs (traits, same-name duplicates, `@see`, include targets, kept use sites, load order) are MO-2's list again. They join M7's incremental-equals-fresh test.
- **C20-4. The runtime oracle's reach went from 81 to 353 fixtures** once the calls were generated from signatures (`gate::samples`). For Phase 4's runtime oracle (F8): generate calls, don't hand-write them.
- **C20-5. Cache facts:** the stub cache dropped a table (the PHPStan-ancestry table) and nothing noticed; context files kept their inline ignores only when parsed. Both are PB-3's "a test checks the cache round-trips every table" rule.
- **C20-6. "Fast validation" rules, for the scoreboard and M7:**
  - whole-program passes take candidates only from analysed files;
  - references resolve as PHP resolves them (case-insensitive class and function names, an unqualified function falling back to the global one, attributes counted as references);
  - a namespace cycle is reported at an analysed declaration, never at a context one;
  - registry dumps are written as a diff from a base dump; a git-backed dump store keeps them (tooling the scoreboard's per-assert diff replaces).
- **C20-7. Design rationale worth keeping for M1/M3/M7 design docs** (each item argued against alternatives, never built):
  - typed index arenas (`la-arena`-style ids) over `bumpalo-herd`, which agrees with LED-3;
  - `triomphe` `ThinArc` names were rejected because reading `&str` back needs `unsafe` (L9-8);
  - strong intern tables with a sweep, its concurrency checked with `loom`;
  - weighted memos (`quick_cache`) keyed by a 128-bit SipHash fingerprint, so a key holds no owned data (OH-2);
  - **blake3 for anything shared through a cache**, because XXH3 isn't collision-resistant and a CI cache is shared across branches;
  - a cache key built from the settings struct destructured field by field, each field marked as tracked or untracked (as rustc's `dep_tracking` does), so a new setting can't be forgotten (OFF-4);
  - racy-stat rules: a file modified within the timestamp granularity of its stat is re-hashed (20L-8 makes the hash the authority anyway);
  - **largest-first scheduling** through `scope_fifo` and an atomic cursor over files sorted by size; rayon's `with_max_len(1)` halves the list and is not largest-first (PD-2, MG-11);
  - a table of crates considered and why each was taken or not (the M1/M3 design docs keep that form, one line per crate).
- **C20-8. Estimates once more:** the plan predicted about 8–9 G fewer Ir and a peak of 350–385 MB. Measured at its end: 49.36 G (from a different baseline nobody reconciled) and a 1.48 GB peak, with heap growth from 2.67 GB to 7.43 GB total allocated arriving before the batch and still unexplained. The meter's per-commit, same-mode history (9B-2, HL-1) is what makes such numbers comparable.
- **C20-9. The 130 G regression came from one migration making primitives dearer, not from a new algorithm.** The effects classification was already quadratic when it was cheap (4.59 s); about 150 commits that moved it to declaration ids made every lookup it called dearer (a flat result map scanned per class, `classes_declaring` collected per call, padded slot vectors at O(classes²) bytes). The commit attribution was done by `git log -L`/`-S` afterwards, and the largest item (a linear dedupe, 33.8 G) had no commit history could name. **A per-commit meter names the commit at once.** Design rules it adds:
  - a per-class view of a member-keyed map is an index, never a filter over the whole map;
  - a dedupe of growing lists is a set, not a linear scan through a dynamic equality;
  - a "snapshot for change detection" is built only when something reads it (the CLI built 15,660 overlays nobody compared).
- **C20-10. Phase 4 by pointer:** the effects model (holders, transfers, `Storage::Captured`, escape and retention via a "retains the callable" builtin column, unknown-code write bounds, parametric summaries, holders derived from types, stub classes with a `contents` pseudo-field, autoload and handler registration events, stream wrappers by path), the 83-row cause ledger (P1/P5/A0–A5/B1–B5), and the blame ranking of unknown-code sources. All of it is in the catalog tag. It confirms OW-2: the port's `invalidateExpression` and impure points are what Phase 4 extends.

### WORKFLOW_PROFILE_REPORT.md (catalog, 354 lines, read in full)

An optimization plan for the legacy's library test suite (a 69.19 G debug-build profile), produced by an agent workflow: proposers per area, then "skeptics" correcting each figure. It lists 41 items (S1–S41) with a "firm total" of 21–28 G (30–40%), a rejected list and an "unexplained" list. Nothing in it was built. What carries over:

- **WF-1. Agent-made findings needed a second agent round to correct, and still came out as a model.** Dozens of proposals were dropped as "misread or non-existent profile row", one cost was read as a call count (MO-11), two counts in the data contradict each other (S38: 79 against 271 switches), and the totals rest on unmeasured hit rates. This is the global no-agents rule and D25-1 again: the meter's measured delta is the only number.
- **WF-2. Iteration order leaked into results.** `lookup_method` returned the first case-insensitive match in hash-map iteration order, so map capacity changed which method won and what completion listed (S40, MO-4); equal values encoded to different bytes because map fields serialized in iteration order (A6). **Rule for M1/M4/M7:** no lookup, output or encoding depends on hash-map iteration order. Lookups are by folded key with one canonical winner (FR-4's collision order), and serialization is in key order (20L-5).
- **WF-3. An early cutoff inside a fixed point leaves stale, self-justifying values.** Mutually recursive `f` and `g` stayed "pure" after `f` gained an `echo`, because the reset stopped at "own entry unchanged". **Rule for M7's result cache and any incremental memo:** invalidation resets a whole strongly connected component of dependent entries, and HL-3's mutation tests cover it.
- **WF-4. "Skip when there is no template" was wrong**, because the substitution step also settles late types (`key-of`, `value-of`, offset access) with no template present. **For M3's node flags (MG-7, D25-4):** a flag that lets a transform return early is defined over everything the transform does, and a debug assertion in the test profile compares the shortcut with the full transform on the suites.
- **WF-5. `Arc` plus `make_mut` only moves a deep copy** into the passes that mutate (A4, D7), so it gains nothing while passes write into declarations. That confirms D20-5 and LS-3: declarations are immutable, and derived facts live in memos, so there is no `make_mut` to pay.
- **WF-6. A test's description and its coverage disagreed.** The every-fixture differential test's doc said every fixture takes part; 42% never did. And that one test made the suite's profile (675 of 1,146 finishing runs), so suite savings didn't transfer to a real run. **For LS-1's sampled differential test:** the sample is stated in the test and covers every fixture over a known rotation, and the `suite-lib` cost workload is never used to justify a product change.
- **WF-7. Node flag bits are a shared, small budget** (3 of 8 bits used, four items wanting more). M3's design doc assigns the flag bits in one table.

### Across the documents: branches, worktrees and plan coupling (from `git branch`, `git worktree list`, WORK_STACK.md)

- **X-1. Several lines of work diverged at once. This is the main cause of the lost track.**
  - `master` (formerly `callgrind-20g`) and `master-old`, with 1,134 commits only on `master-old` (MO-1);
  - `php-catalog`, with 1,167 commits and +705k lines not in `master`;
  - `builtin-runs`, `lazy-context`, and `lazy-includes` (a worktree in `~/.cache/prs-lazy`);
  - about 20 `worktree-wf_*` branches with worktrees, and five detached worktrees under `~/.cache`;
  - lanes A and B for two sessions in parallel, and "another session shares the checkout" written into several plans.
- **B-6. Plans coupled to each other.** CI_SPLIT, GATE_SPEED, PROJECT_BUILD_COST, CROSS_PLATFORM, CRASH_REPORTING, UNSAFE and ITEMS_1_TO_3 each:
  - ran 300–2,300 lines, with their own decisions, rows, rounds and verification;
  - were opened beside the crate split;
  - pulled items forward "in their final form" from the crate split's later stages.

  That coupling is how `WORK_STACK.md` reached 10 levels.


