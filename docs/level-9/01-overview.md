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
| 1 | D1: what consumers hold | [`08-part-b.md`](08-part-b.md) |
| 2 | D2: how to migrate | [`08-part-b.md`](08-part-b.md) |
| 3 | D3: position | [`08-part-b.md`](08-part-b.md) |
| 4 | D4: payload bytes | [`08-part-b.md`](08-part-b.md) |
| 5 | D5: names in nodes | [`08-part-b.md`](08-part-b.md) |
| 6 | D6: the mutating passes | [`08-part-b.md`](08-part-b.md) |
| 7 | D7: fabricated nodes | [`08-part-b.md`](08-part-b.md) |
| 8 | D8: rows | [`08-part-b.md`](08-part-b.md) |
| 9 | D9: error nodes | [`08-part-b.md`](08-part-b.md) |
| 10 | SYN-3.2's SIMD | [`15-part-c.md`](15-part-c.md) |
| 11 | the parse-recovery batch | [`15-part-c.md`](15-part-c.md) |
| 12 | order of items 13, 16, 15, 14 | [`15-part-c.md`](15-part-c.md) |
| 13 | oracle tools | [`07-part-f.md`](07-part-f.md) |
| 14 | DOC-2.1 | [`15-part-c.md`](15-part-c.md) |
| 15 | CACHE-13.1 for context files | [`06-part-d.md`](06-part-d.md) |
| 16 | item 18 | [`20-part-j.md`](20-part-j.md) |
| 17 | CHECK-9 and TYPE-5 | [`15-part-c.md`](15-part-c.md) |
| 18 | when to reevaluate | [`16-part-h1.md`](16-part-h1.md) |
| 19 | order of the wall | [`06-part-d.md`](06-part-d.md) |
| 20 | rows for C6 to C9 and D to G | [`21-part-n.md`](21-part-n.md) |
| 21 | Stages D to G | [`06-part-d.md`](06-part-d.md) |
| 22 | level 3 and the Q-11 follow-ups | [`05-part-k.md`](05-part-k.md) |
| 23 | Part A's validation | [`02-part-a-intro.md`](02-part-a-intro.md) |
| 24 | item 9's validation | [`08-part-b.md`](08-part-b.md) |
| 25 | LOAD-2.8's parts 1 and 2 | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| 26 | the LSP completion probe | [`11-part-g4-g6.md`](11-part-g4-g6.md) |
| 27 | the step 1 codemod | [`08-part-b.md`](08-part-b.md) |
| 28 | LOAD-5.2 batch-only | [`06-part-d.md`](06-part-d.md) |
| 29 | NAME-2.1's thin name | [`15-part-c.md`](15-part-c.md) |
| 30 | where Part B lives | [`21-part-n.md`](21-part-n.md) |
| 31 | an analysed file with parse errors | [`10-part-g3.md`](10-part-g3.md) |
| 32 | `PhpVersion` and its types move out of `types` | [`15-part-c.md`](15-part-c.md) |
| 33 | the engine crate and `unsafe_code` | [`17-part-h3.md`](17-part-h3.md) |
| 34 | positionless declarations | [`15-part-c.md`](15-part-c.md) |
| 35 | PROD-5.7 and `deps::Collector` | [`15-part-c.md`](15-part-c.md) |
| 36 | the server's snapshot model | [`18-part-h2.md`](18-part-h2.md) |
| 37 | the persisted database's key | [`19-part-h4.md`](19-part-h4.md) |
| 38 | REG-16.2's "never reused" | [`16-part-h1.md`](16-part-h1.md) |
| 39 | LOAD-5.1's passes | [`06-part-d.md`](06-part-d.md) |
| 40 | the composer crate's B8 follow-ups | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| 41 | rows whose code a later task replaces | [`17-part-h3.md`](17-part-h3.md) |
| 42 | deltas the ledger marks READY or OPEN with half a ruling | [`06-part-d.md`](06-part-d.md) |
