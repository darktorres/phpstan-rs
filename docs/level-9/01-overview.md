## Overview

## Tasks this part owns

None. The overview names no task of its own: each part's own table lists the rows that part builds or decides, and the ledger is the only record of their states.

The plan is written from the code at `0fb1a4de` (Parts A to F), `30aa2002` (G) and `8a1ca1e0` (H to J); the review passes re-read the code at `52844b07` and `ffa7cccb`. Line numbers
are hints from those commits and the tree moves, so a citation is found by the symbol it names; "the fixes plan's R7"
is `ITEMS_1_TO_3_FIXES_PLAN.md` (ledger LOAD-8.7) and "the cross-platform plan's R7" is `CROSS_PLATFORM_PLAN.md`.
Part A is built and validated (its status is in Part A's intro, `02`). Nothing from Part B onward is built by this plan. The batch-0 fixes already in the tree are listed in Part E. Part C (what follows item 9) and Part D (what follows that:
levels 1 to 7) are at the end, and Part E gives the answer for every pending decision,
by number. Where Parts B to D differ from Part E, Part E governs.
Part F lists what needed designing; Part G holds the designs of its rows 1 to 4 and 6, Part H the engine cluster
(rows 10, 13 to 15), Part I the designs the batches wait for (rows 5, 7, 8, 11, 12) and Part J the wall's later
ones (rows 16 to 18). Part K records the review by task group, and Part L the second pass by ledger row;
where either changed an earlier part, the change is made there. Part N lists the documents and the ledger and stack edits the plan implies.

- **Part A** designs the batch that follows the cross-platform fixes: five rows (`WORK_STACK.md`: item 2's last part, item 8's
  REG-14.4, item 9's first step, item 19's LOAD-5.4, and LOAD-9), the composer follow-ups A7 and A8 (Part E 40) and
  REG-14.5, which is folded into REG-7.5.
- **Part B** designs item 9 (SYN-1: the flat tree) as a staged migration. It needs nine
  decisions settled first (B2, D1 to D9); the three that matter most are D1 (handles and
  views, not bare ids), D2 (staged migration over the boxed tree first) and D3 (what a node
  records of position, which the ledger leaves to a measurement: spike S-12 makes it before step 2).
  D4 keeps the ledger's layout: the tree owns its source, a string is a span,
  and a side buffer holds the bytes that no source has.

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

---
