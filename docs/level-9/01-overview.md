# Overview: the conventions and the decisions

The plan is written from the code at `0fb1a4de` (Parts A to F), `30aa2002` (G) and `8a1ca1e0` (H to J); the review passes re-read the code at `52844b07` and `ffa7cccb`. Line numbers
are hints from those commits and the tree moves, so a citation is found by the symbol it names; "the fixes plan's R7"
is `ITEMS_1_TO_3_FIXES_PLAN.md` (ledger LOAD-8.7) and "the cross-platform plan's R7" is `CROSS_PLATFORM_PLAN.md`.

Part A is built and validated (its status is in Part A's intro, [`02-part-a-intro.md`](02-part-a-intro.md)). Nothing from Part B onward is built by this plan. The batch-0 fixes already in the tree are listed in the README's batch 0 rows. The order of the work is the sequence in the [README](README.md).

Both start only after the cross-platform fixes have finished (`CROSS_PLATFORM_PLAN.md`, rows R0 to R18, finished at `a83b1675`). Its rows edit what this plan's first rows edit: `session.rs`, `project.rs`, `walk.rs` and
`lsp.rs` (R5 to R7), `includes.rs` (R8), `composer` (R3, R9, R16), `main.rs` (R10) and the caches (R11). R18 moves
code this plan reads: `file_id` and `shard` into reflection, the completion rules (`Completion`, `Reach`,
`BlockCompletion`) and `is_truthy_literal` into `syntax`, the rebind and include-guard mention bits into
`checker::mentions` (the project hands the checker the file's whole `mentions`), `set_current_file` taking the shown
path, `path_bytes` into a crate of its own (`phpstan-rs-path-bytes`, which R16 works in) and the allocator into one
module of the binary. Heavy jobs run one at a time, and the stack's rule is that level 8 resumes when the cross-platform
fixes finish. Start condition for Part B: `git log` shows R17 and R18, `WORK_STACK.md` no longer lists the cross-platform fixes, `git status` is clean, `ps`
shows no other build or profile. Every design below is read against that level's code (the case rule of R7, the
canonical form of R3, the `/`-spelled report paths of R6, the module homes of R18) and says where it meets one.

Conventions for every row: `cargo fmt` first, then `cargo check`, `cargo clippy -D warnings`,
the targeted tests, a commit with a pathspec. Comments name no row or phase. Output of long
runs goes to a file under `~/.cache`, then is searched. No saving is claimed without a
measurement.

**Validation is at the boundary the user names:** the end of each top-level item (the 19 items of the 20 G plan and
the stages of the crate split), never at a step or a row inside one, and no checkpoint is added that the user did not
ask for. Where a design below says a comparison "at each step" or "at each commit", the dump or oracle run is produced
at that point (a cheap run that stores output) and compared with the stored baseline at the item's end, and the suites,
fixtures and corpus passes run once there; the per-commit checks are `cargo fmt`, `cargo check`, `cargo clippy` and the
touched module's tests.

An old-versus-new comparison is deleted in the commit after its stage: I2's lexer `(kind, span)` fuzz when the old lexer
is deleted; I4.5 step 3's per-`TypeNode` comparison, only if VAL-1.7's PHPDoc record cannot see what it finds; H3.6
step 2's `Provider`-against-engine comparison and step 3's old passes, one level per stage. A2 keeps none.

## Edits at approval

Approving this plan makes these edits. None is made before the go.

- `WORK_STACK.md`, section 9: level 8 is reordered so that item 0 (batch 0) comes before item 9, and Part A's rows come
  before item 0. Item 9 reads "SYN-1.11, then 1.12 (+1.3), then 1.2/1.4/1.5/1.7/1.8, then step 3 (LOAD-3.5); the design
  is Parts B, G1 to G4 and I1". The batches of C1 follow in order: tokens, the lexer's second pass, parse recovery with
  the `php-version` crate, names, the constant lookup, products, PHPDoc, the check's temporaries. Item 2's bullet goes,
  LOAD-9 leaves "Outside the items", item 8 keeps REG-14.1 and 14.3, item 19 loses LOAD-5.4, and the state line is
  rewritten. The "Documents with no place in the stack" list gains this plan's documents.
- `CRATE_SPLIT_PLAN.md`: the docs commit series of #20 (below), which also writes the ledger rows of J2 (OVL, RES, EXT, SUM) and the D and E rows of J3, in [`05-part-d.md`](05-part-d.md).
- The ledger rows that decision 20 writes: OVL, RES, EXT and SUM (J2), with the
needs above; a D-row and an E-row per phase of Stage D and E that the plan lists, each citing its plan section and
the oracle counts it carries; CHECK-6.1 to 6.3, 6.7, 6.13 to 6.15 re-pointed to "built by D1 to D4"; LOAD-5.1
re-pointed to W3 (facts) and E1 (collection); and the ledger edits the engine cluster implies (`01-overview.md`, Edits at approval).
- The ledger's `needs` changes of D8, in [`07-part-b.md`](07-part-b.md) (B11).
- Other documents. `CROSS_PLATFORM_PLAN.md`: R11's database-file follow-up and R16's `path_bytes` encoding pair (H4.6),
  and R13's CRLF probe files marked `-text` (G3.7.1). `CRASH_REPORTING_PLAN.md`: the stack guard's `LIMIT` restored on
  unwind and the engine's two isolation modes (H2.8); the vendor-discovery panic-hook swap is deleted, not converted
  (#40). `UNSAFE_PLAN.md`: its end state changes only if S-2 finds the engine exception necessary (#33).
  `STAGE_B_FIX_REVIEW.md`: F14 superseded by TYPE-5's rule; F33's epoch lands in G1.5; F58's lazy construction in G1.9.
- The ledger rows the engine cluster re-specifies: REG-8.6 (H1.3); REG-4.15 loses its span field and REG-4.11 becomes a
  record over (`Origin`, site) (H1.5a); REG-16.2's and decision 3's "never reused" stand for `DeclKey` (#38), and decision 3's
  member sentence is refined to PHP's namespaces (H1.4); REG-16.4 needs SYN-1.9; REG-16.5 is the event callback plus the
  provider's trace of first readers (H3.5); PROD-5.4, PROD-5.7 and LOAD-4.7's file-references half are marked "deleted by
  REG-16.4" (H4.8); CACHE-13.2 is H4.6, keyed without the version range (#37), and CACHE-9 to CACHE-11's form follows S-4;
  CACHE-13.1 and decision 13 become `File.stamp` and the stat-time rule of #15; STUB-7.5 moves to W3 and needs the Composer
  maps to keep their keys and gain PSR-0 (H4.1, A7); PROD-5.1 and SYN-1.10 keep anonymous classes out of the declaration
  names and the global index (H1.4).
- `crash_report::cache_dir()` moves into `project` (F2).

Two gates hold every row that moves code, and they are cheap to different degrees. `cargo xtask modgraph` parses the
graph and runs `cargo metadata`, so it runs per commit: a new crate gets its `crate` line and a new top-level module of
the package its `module` line in `gates/module-graph.txt` in the commit that makes it, as does a nested module whose
crate differs from its parent's, such as `registry::purity analyser`; a module that only the library root holds is
rejected. `cargo xtask tests` compiles every test target (the test profile for the workspace, the gate profile for the
benches and the fixtures target) and so runs at the validation boundary; a test that is renamed, merged or deleted is
listed as `old -> new` or `old -> REMOVED: reason` in a `gates/test-renames/*.txt` file in the commit that does it
(the gate reads every commit of the range, so "the same commit" is a convention), or the manifest fails; item 9's test
rewrite and the retired oracles are the large cases. A new crate also joins `LIBRARY_CRATES` in `xtask/src/arch.rs`
(else its no-print rule skips it) and the build-time scenario lists in `xtask/src/main.rs` (an unlisted crate is
unconstrained); the new crates of this plan (`php-version`, `engine`, `stack`) do both in the commit that makes them.
A row that adds an `xtask arch` rule adds it with the code that satisfies it. **CI does not run `cargo xtask gates`:**
the `gates` job of `rust-ci.yml` (Linux only, `ubuntu-24.04`) lists its steps one by one and calls `xtask modgraph`,
`arch` and `tests`, and the other systems are the `platforms` job. So a new `xtask arch` rule reaches CI by itself,
and any other cheap gate (F20's recursion lint) is added in two places, `gate_steps` and a step of the `gates` job. A
heavy tool (the dumps, the baseline, the `oracle` profile run) stays local: it needs the frozen Piwigo checkout and a
quiet machine, and a dump spells paths and diagnostics as its platform does.

## Decisions

Churn and effort are not weighed: each answer is what is correct and complete. Each answer is in the part that owns the
design it changes, and no two parts answer the same question differently. The rulings (the nine decisions, the deltas of #42
and the audit's answers) are carried by their answers. The other answers are the plan's, on the same footing: the most
proper course for each.

Decisions in force: the fixes plan's decisions 1 to 7 and the ledger's decisions 1 to 18, which the plan cites by number. Gates, not decisions: the go, and the start condition in the overview. Batch
numbers are the order of the README's "The sequence". An answer that departs from a decision's wording says so.

Each decision is a `### #n` section of the part that owns the design it changes; the index is below.

| # | decision | part |
|---|---|---|
| 1 | D1: what consumers hold | [`07-part-b.md`](07-part-b.md) |
| 2 | D2: how to migrate | [`07-part-b.md`](07-part-b.md) |
| 3 | D3: position | [`07-part-b.md`](07-part-b.md) |
| 4 | D4: payload bytes | [`07-part-b.md`](07-part-b.md) |
| 5 | D5: names in nodes | [`07-part-b.md`](07-part-b.md) |
| 6 | D6: the mutating passes | [`07-part-b.md`](07-part-b.md) |
| 7 | D7: fabricated nodes | [`07-part-b.md`](07-part-b.md) |
| 8 | D8: rows | [`07-part-b.md`](07-part-b.md) |
| 9 | D9: error nodes | [`07-part-b.md`](07-part-b.md) |
| 10 | SYN-3.2's SIMD | [`14-part-c.md`](14-part-c.md) |
| 11 | the parse-recovery batch | [`14-part-c.md`](14-part-c.md) |
| 12 | order of items 13, 16, 15, 14 | [`14-part-c.md`](14-part-c.md) |
| 13 | oracle tools | [`06-part-f.md`](06-part-f.md) |
| 14 | DOC-2.1 | [`14-part-c.md`](14-part-c.md) |
| 15 | CACHE-13.1 for context files | [`05-part-d.md`](05-part-d.md) |
| 16 | item 18 | [`05-part-d.md`](05-part-d.md) |
| 17 | CHECK-9 and TYPE-5 | [`14-part-c.md`](14-part-c.md) |
| 18 | when to reevaluate | [`15-part-h1.md`](15-part-h1.md) |
| 19 | order of the wall | [`05-part-d.md`](05-part-d.md) |
| 20 | rows for C6 to C9 and D to G | this file, below |
| 21 | Stages D to G | [`05-part-d.md`](05-part-d.md) |
| 22 | level 3 and the Q-11 follow-ups | [`06-part-f.md`](06-part-f.md) |
| 23 | Part A's validation | [`02-part-a-intro.md`](02-part-a-intro.md) |
| 24 | item 9's validation | [`07-part-b.md`](07-part-b.md) |
| 25 | LOAD-2.8's parts 1 and 2 | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| 26 | the LSP completion probe | [`10-part-g4-g6.md`](10-part-g4-g6.md) |
| 27 | the step 1 codemod | [`07-part-b.md`](07-part-b.md) |
| 28 | LOAD-5.2 batch-only | [`05-part-d.md`](05-part-d.md) |
| 29 | NAME-2.1's thin name | [`14-part-c.md`](14-part-c.md) |
| 30 | where Part B lives | [`07-part-b.md`](07-part-b.md) |
| 31 | an analysed file with parse errors | [`09-part-g3.md`](09-part-g3.md) |
| 32 | `PhpVersion` and its types move out of `types` | [`14-part-c.md`](14-part-c.md) |
| 33 | the engine crate and `unsafe_code` | [`16-part-h3.md`](16-part-h3.md) |
| 34 | positionless declarations | [`14-part-c.md`](14-part-c.md) |
| 35 | PROD-5.7 and `deps::Collector` | [`14-part-c.md`](14-part-c.md) |
| 36 | the server's snapshot model | [`17-part-h2.md`](17-part-h2.md) |
| 37 | the persisted database's key | [`18-part-h4.md`](18-part-h4.md) |
| 38 | REG-16.2's "never reused" | [`15-part-h1.md`](15-part-h1.md) |
| 39 | LOAD-5.1's passes | [`05-part-d.md`](05-part-d.md) |
| 40 | the composer crate's B8 follow-ups | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| 41 | rows whose code a later task replaces | [`16-part-h3.md`](16-part-h3.md) |
| 42 | deltas the ledger marks READY or OPEN with half a ruling | [`05-part-d.md`](05-part-d.md) |

### #20 rows for C6 to C9 and D to G

**A docs pass at approval, in one commit series, edits `CRATE_SPLIT_PLAN.md` itself** so there is one source of truth.
The series writes the rows of J2 (OVL, RES, EXT, SUM) and the D and E rows of J3, and marks each superseded paragraph
in place, pointing at the section that replaces it:

- Appendix F's "each level is a per-class `OnceLock`" and "constant reference cycles type as `mixed`" (H3.1, #1);
- C4's "a change to any reflection input resets every memo" (the recorded reads);
- F1's published snapshots (H4.3's cancellation) and its text on line starts (the PHP terminator rule is applied in the
  lexer too: I3);
- F2's resolved dependencies, fingerprints and entry sets (H4.4);
- §3.9's text about the deleted `lsp/workspace_config.rs`;
- the line "a file with parse errors is still not analysed" (around line 6883), superseded by #31;
- G's "26 new packages" count and Appendix A, which gain the engine and `php-version` (J3);
- B8's composer follow-ups (`config.vendor-dir`, Composer's constraint grammar, `psr-0`, the root package's autoload,
  a package's bundled `vendor/`) point at A1, A7, A8 and #40 once they are built; the clamp of an open upper bound to
  `MAX_PHP_VERSION` is marked superseded;
- C6 to C9 and D to G gain the rows of J2 and J3.

Effort is no reason to leave stale text that misleads until its phase is reached.

*Needed before:* approval

