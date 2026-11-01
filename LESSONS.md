# Lessons from the legacy phpstan-rs

The legacy code is frozen at tag `legacy-final` (branch `legacy/master`). The catalog line is frozen at `catalog-final`, and the effects engine at `master-old-final`.

Each lesson is one or two lines, followed by the id of its finding in `RESTART_FINDINGS.md` at `legacy-final`. That file cites the legacy document and section behind each id. Read a legacy file with `git show legacy-final:<path>`.

## Architecture

### Front end (M1)
- **The tree is flat, immutable and index-based:** one typed vector per node kind, with `NonZeroU32` ids. It is `Send + Sync` without `unsafe` (LED-3, L9-2).
- **Changing a tree's shape late is expensive.** The legacy had 2,612 match sites on `ExprKind`/`StmtKind`, so the shape is settled in M1 (LED-3, L9-1).
- **The lexer fills a kind-plus-span token array.** Streaming tokens through the parser's lookahead cost +1.77 G to save 0.33 G (D20-1, P-6).
- **No payload table.** A binary search for each token's payload on every `peek()` was 1.0 G of the parser's 1.8 G (9B-6, L9-7).
- **Positions are byte offsets, with a lazy line index.** Building column tables for every file cost +0.27 G (D20-3, MG-5).
- **Line breaks:** a line ends at `\n`, `\r\n` or a lone `\r`, as PHP counts it. The legacy misplaced every line after a lone `\r` (CP-6, FR-9).
- **The lexer and parser record what later walks would recompute:** ignore comments, doc hashes, resolved names, declarations and labels. This removed four post-parse walks (D25-3).
- **Deep input:** every walk iterates or grows its stack. 100,000 levels must run on a 256 KiB thread (SB-1, LED-9).
- **Long `elseif` chains and left-deep `.` chains** crashed the legacy, because it recursed over them (SB-1).
- **Parse failure is `phpstan.parse`** for an analysed file, and the file is dropped. Error recovery is for the LSP only (OFF-6).
- **The parser's bar** is zero false refusals over a large vendor tree, checked against `php -l` (LED-9, P-9).

### Names
- **One interner,** with `Name`, `FoldedName` and `ConstName`. Two interners lose pointer equality (L9-8, MG-6).
- **Names are bytes.** A lossy decode merges two different non-UTF-8 identifiers (C20-1, L9-8).
- **Case rules:** methods fold case; properties and class constants don't; a constant's namespace folds but its last segment doesn't (L9-8).
- **Interning is scoped to the session.** A process-wide interner that is never reclaimed leaks memory in a long-lived LSP (AU-3).

### Types (M3)
- **`Type` is opaque.** Every question is a method mirroring PHPStan's `Type`. The legacy had about 1,055 inline inspections, each answering the same question its own way (FR-3).
- **No union cap.** The cap at 12 members collapsed unions to `mixed` (F6, C20-2).
- **Unions are built n-ary, in one pass.** The legacy's pairwise fold was O(n³) and thrashed its memo (D25-4, PO-2).
- **Node flags let transforms return early.** A flag must cover everything the transform does: "skip when there's no template" missed late types (WF-4, MG-7).
- **The relation memo** is keyed by a content generation. Calling `clear()` per generation cost 621 M in `memset` (D20-6, 9B-6).
- **Float `pow`** is pure Rust, so results don't depend on the platform (OFF-8).
- **Property tests:** `a <: a|b`, narrowing completeness, and every displayed type parses back (MG-7, F4).
- **`describe()` produces PHPStan's exact strings,** so the comparison needs almost no normalization (CA-5).

### Reflection (M4)
- **Lazy, per class.** 80% of parsed files were context files, and a run read 0.5–0.6% of their function bodies (P-2, OFF-1).
- **Whole-registry finishing passes** were 37–40% of wall time, nearly all of it serial (P-3, IT-2).
- **Stubs:** a build-time name index, lazy per-file parsing, and version gates kept as data. A compile-time snapshot pulls the parser into every build (MG-4, OFF-2).
- **Declarations are immutable and `Arc`-shared,** and derived facts live in memos. Deep-clone-and-compare bookkeeping was 18% of the test suite (D20-5, LS-3, WF-5).
- **Never run parallel work under a memo's lock.** A rayon worker that steals jobs re-enters the lock and deadlocks (OFF-3).
- **Locators follow PHPStan's.** The legacy's lazy vendor loading silently lost the classes its autoloader couldn't find by name (PO-1).
- **Composer:** the root package's own autoload entries are context. Missing them gave 1,253 false `class.notFound` (FR-2, L9-5).
- **A trait method is bound to each using class,** so results are keyed by (using class, member) (D20-5).
- **Which is the class:** a project class named like a stub class is the project's (SC-1), and attributes match by the class they resolve to (SC-2).

### Analyser (M5)
- **Each expression is inferred once,** with per-node results. With re-inference, 1,600 nested calls took 236 s, and 20,000 nested arrays took 57 minutes (SB-5, FR-6).
- **Loops** run `LOOP_SCOPE_ITERATIONS = 3` with `generalizeWith`. The legacy's 6 passes without widening caused 208 nsrt misses (FR-6, F2).
- **Conditional holders, certainty and native types are PHPStan's.** The legacy's bespoke idioms each cost a pile of point fixes (F6, SBF-1).
- **Joins and memory:** scope joins visit only changed keys (94% of keys are unchanged), and throw points never clone the scope (MG-8, PO-3, D20-10).
- **`goto` is modelled by PHPStan.** The legacy's audits assumed it wasn't until a fourth round read the source (D25-5).
- **`ArgumentsNormalizer` runs before any builtin extension.** The legacy had about 100 sites reading raw argument indexes (MG-9).

### Rules (M6)
- **`findTypeToCheck` comes first.** About 2,950 of 9,623 oracle misses came from rules deciding about `mixed` themselves (FR-8).
- **Identifiers and messages are copied from PHPStan's rule classes.** Renames and duplicates were 188 of the legacy's own-only findings (CA-6).
- **Messages are formatted only when reported.** 98.5% of built diagnostics were dropped later (OFF-9, 9B-6).
- **Collectors replace facts passes and whole-program passes.** The facts passes merged in hash-map order (GS-3, D20-4).

### Command and result cache (M7)
- **Exit codes:** 0 clean, 1 findings, 2 untrusted, 130 Ctrl-C, 141 closed pipe. The legacy exited 0 on findings (FR-9, M-8).
- **No silent drops.** The legacy dropped 31,660 of 32,555 Piwigo diagnostics for functions that call `extract()` (F13).
- **Change detection** uses content hashes, never stat: `rsync -t` and `tar` keep the size and time (9B-6, 20L-8).
- **Dependencies** come from reflection reads, as resolved, folded names. Matching names as written over- and under-matched (PB-2).
- **Invalidation resets a whole dependent component.** An early cutoff left stale, self-justifying purity (WF-3).
- **The incremental-equals-fresh test is written first.** Five stale-result bugs would each have failed it (MG-12, MO-2).
- **Cache files:** no `mmap`, which needs `unsafe` and raises SIGBUS on truncation. Few files, not one per entry: one per entry cost 0.5 s of kernel time (D20-7, OFF-5).
- **Each cache has a test that a warm run reads it and writes nothing.** The legacy's stub cache never worked, and nothing noticed (PB-3).
- **Panic isolation:** one hook; rayon panics carried through a wrapper; every unit isolated (CR-1).

### Parallelism
- **One injected pool.** Each fixture built its own pool, so eight tests ran 64 threads (GS-2).
- **Largest files first, or fine-grained stealing.** One straggler held 48.8% of the samples while seven threads idled (MG-11, PD-2).
- **Trees are dropped on the worker that built them.** That cut main-thread frees by 72 ms and peak RSS by 214 MiB (IT-1, D20-4).

## Measurement
- **Ir is deterministic only at one thread.** At the default thread count the spread was 15× larger (LED-6).
- **Gate Ir and heap together.** A batch cut Ir 8.8% while peak heap rose 68% (WS-2, 9B-1).
- **Cache mode is part of a workload.** The same binary measured 6–8 G apart depending on the stub cache's state (9B-2, HL-1).
- **Predictions ran about 2× optimistic.** Only measured deltas count (D25-1, D20-12, C20-8).
- **Performance collapsed unnoticed** from 4.59 s to over 60 s across six items, because the gates only ran tests (20L-1).
- **A 130 G regression came from about 150 commits** that each made a primitive dearer. A per-commit meter names the commit (C20-9).
- **Profiles:** one fixed profile. A codegen change moves totals by about ±1 G (P-7).
- **Opt level:** opt 1 vs opt 3 scaled every pass by about 0.65 (LS-2).
- **Instructions are not time.** The allocator runs at 1 G Ir/s and byte loops at 10+ G/s (LED-6, IT-4).
- **Wall time on this VM** varies ±20–40%. Use paired alternating A/B runs with a Wilcoxon test (B-2, IT-4).
- **Shared cores:** the same code takes 44% more CPU at 8 threads on this VM (IT-3).
- **Callgrind:** thread slots are reused, so read a thread's role from its functions. Never sum inclusive costs (OFF-10, HL-2).
- **dhat:** whole stacks need the patched dhat (minutes rather than hours). A realloc is charged to the first allocating site (OH-1, P-7).
- **Memo counters:** a memo under about 20% hits is removed (9B-5).
- **The test suite's profile** was dominated by one differential test (68–72%), so its savings didn't transfer to real runs (LS-1, WF-6).

## Portability
- **Paths in reports:** spell them with `/`, from one function. PHPStan's `normalizePath` turns `\` into `/` on every platform (CP-1).
- **Main thread stack:** the program chooses it. Windows gives 1 MiB, and one run made 36,914 `mmap` calls growing it (CP-4).
- **Caches** live under `dirs::cache_dir()`. On Windows, an entry's last-use time must be set through a write handle (CP-5).
- **Embedded sources:** `/` in embedded paths, and LF copies of sources that contain `\r\n` (CP-7).
- **Tests and tooling:** link helpers per OS; no GNU-only tools in xtask (CP-8, CP-9).
- **CI:** test all three OSes from day one, and check Windows locally under wine (B-3, CP-8).
- **Release binaries:** link an older glibc or musl. The legacy's binaries didn't run on Ubuntu 24.04 (RM-1).

## PHP facts proved with `php` (inputs for tests and `known-deviations.md`)
- **Grammar:**
  - an arrow function, `throw` and `include` take the loosest operand;
  - `1 < 2 < 3` is a parse error;
  - an all-short `?:` chain is valid (D25-2).
- **Casts and strings:**
  - only spaces and tabs are allowed inside cast parentheses;
  - in `"$a[01]"`, the key is the string `'01'`;
  - `"{$x = 1}"` is a parse error (L9-7, SB-1).
- **Nesting:** 9,000 nested `[` evaluate, and 10,000 fail with "memory exhausted" (L9-7).
- **Arithmetic:** overflow gives a float; `$m % -1` is `0`; `<<` wraps (SB-3).
- **Comparison:** `null == '0'` is false on PHP 8; `NAN === NAN` is false; `'abc' + 1` throws from 8.0 (SBF-3).
- **Statements:** `goto` can jump into an `if` but never into a loop; top-level declarations after a `return` are hoisted (D25-2).
- **Regex:** a pattern's delimiter is found by scanning forward with bracket nesting, never by `rfind`; `u` means UTF|UCP (UN-2).
- **Error handlers:** `@` doesn't stop one, and a user function given `null` throws without the handler seeing it (BR-1).
- **Effects:** builtins run user code (property hooks via `get_object_vars`, `array_column` via `__isset`/`__get`). Phase 4 work (PE-1).

## Reverted or refuted
- **Lazy vendor loading** was built (−3.8 G) and reverted: classes went missing silently. The port follows PHPStan's locators instead (PO-1).
- **Lazy context includes** were built, and the branch was never merged. It is kept at `lazy-includes-final` (X-1).
- **The streaming lexer** was built and lost: +1.77 G (D20-1).
- **Mago's twelve refuted ideas** are not re-proposed. Among them: a per-file token table, and flattened concat chains (M-9).
- **Deriving builtin behaviour from php-src's C** doesn't scale: every pointer analysis died at 10 GiB. Observation worked (PE-1).
- **A second semantics that must agree with the checker forever** is rejected (OW-4).

## Process
- **One line of work.** Several branches, worktrees and sessions at once caused the lost track (X-1).
- **No plan pulls items from another plan.** Cross-plan coupling grew the stack to 10 levels (B-6).
- **No ledgers of task ids.** About 200 ids with need-chains became the lost track (L-3, LED-1).
- **No review documents.** About one in five findings written from reading did not reproduce (L3-1, R-3).
- **No audit rounds before building.** Seven rounds produced thousands of lines of design per few G saved (P-8).
- **Agent-made findings needed agents to correct them,** and still came out as a model (WF-1, PL-2).
- **Done means tests ran in CI.** Rows marked "done, unvalidated" hid an unbuildable snapshot (20L-2).
- **Correctness and cost are not mixed in one batch.** A performance plan absorbed correctness and catalog work (20L-4).
- **Extend after porting.** `number` was still unsupported while SMT proofs were being designed (W-2).
- **Gates run in CI,** or they don't run (AU-2).
- **Comments:** a rename re-reads every comment it touches. Reviews found about 40 false doc comments after renames (SB-7).
- **No side-effecting call inside `debug_assert!`.** One dropped `@method` members in release builds only (CC-3).
- **No hash-map iteration order** in results or encodings. It changed which method won a lookup (WF-2, 20L-5).
- **Rigour attaches to a scoreboard number.** One builtin's model took a 171 KB artifact and five review passes (PL-1, CC-5).
