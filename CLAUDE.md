# phpstan-rs

A port of PHPStan to Rust, done in milestones toward parity, then extended. The plan is `PLAN.md`; where the work stands is `STATUS.md`; what the legacy code taught is `LESSONS.md`. The legacy code is frozen on the `legacy/*` branches and the `*-final` tags (`git show legacy-final:<path>`).

## Process

- **One line of work.** One milestone is active, on `main`, with one session writing. There are no side branches and no worktrees, except a throwaway A/B measurement.
- **Docs, and only these:**
  - `README.md`, `CLAUDE.md`, `PLAN.md`, `STATUS.md`, `LESSONS.md`;
  - `docs/design/<topic>.md`: at most about 300 lines each, deleted or folded into `PLAN.md` when their work lands;
  - `parity/*.md`.

  There are no `*_PLAN`, `*_LEDGER`, `*_REVIEW`, `*_DRILL` or `*_AUDIT` files, no paused-work stack and no task ids.
- **A defect exists when a test fails.** It becomes a failing test first. It is fixed inside the milestone if it blocks the scoreboard; otherwise it gets one line under "Later" in `STATUS.md`.
- **Done means its tests ran and passed in CI.**
- **Correctness belongs to the scoreboard and cost to the meter.** No batch mixes them, and none is measured only at its end.
- **Only measured deltas are recorded.** A predicted saving is a hypothesis.
- **Design small, build, measure.** An open question is a spike: the question, the method, an accept bound and a fallback.
- **Work beyond PHPStan waits for the parity exit** (Phase 4 in `PLAN.md`).
- **Commit each item** as soon as its own checks pass.

## Porting

- **Port from the PHPStan source file that the failing test exercises,** in `vendor/phpstan-src`. That is 2.3.4, frozen for the parity phase. Keep PHPStan's structure, identifiers and messages, and read the source rather than recalling it.
- **A Rust file's header names the PHPStan file it ports,** and `parity/port-map.tsv` records the mapping (`cargo xtask port-map`).
- **Legacy code is a quarry, not a base.** Copy a piece only if it fits the PHPStan-shaped module and passes its suite.
- **Where PHPStan is wrong at runtime,** the port matches PHPStan and the case goes into `parity/known-deviations.md`.
- **`php` 8.5 is installed** and settles runtime questions (`php -r '…'`).

## Code rules

- **`unsafe`:** forbidden everywhere except the `pcre` crate.
- **Library crates do no I/O and print nothing.** They return results as data.
- **No hash-map iteration order leaks** into a result, a lookup or an encoding. Serialized maps are written in key order, with a byte round-trip test.
- **Memos:** each has a hit counter, an explicit list of its key inputs, and a test per invalidation point. A memo under about 20% hits is removed.
- **Caches:** each key is built from an enumerated input list, with a test that the list is complete. Each cache has a test that a warm run reads it and writes nothing.
- **Concurrency:**
  - nothing parallel runs while holding a lock that a pool job can wait on;
  - process-wide state is handed down as a value, never a hidden static;
  - there is one injected worker pool.
- **No second implementation stays resident under `cfg(debug_assertions)`.** A comparison against old behaviour is a test, or a throwaway run deleted with the change.
- **Generated files** are written to a temporary file and renamed.
- **Tests** clean up any process they start and never change the process environment.
- **Comments** state what the code does and why, never task or plan references. A rename re-reads every comment it touches.
- **Cross-platform:** Linux, macOS and Windows. Paths in reports are spelled with `/`. Windows is checked locally under wine.

## Checks

- **Per item:**
  - `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test` for the crates touched.
- **Per milestone end:**
  - the full scoreboard (`cargo xtask parity`);
  - `cargo test --workspace`;
  - a Piwigo diff, as a regression check only (`/home/torres/piwigo17-frozen`).
- **CI runs every gate on every push.** A gate that only runs by hand isn't a gate.
- **Output handling:** write test and build output to a file under `~/.cache/phpstan-rs/`, then grep that file. Scratch never goes in `/tmp`.

## Measuring

- **The cost meter is `cargo xtask cost`** (Phase 2). It counts callgrind Ir and dhat heap in the `cost` profile, at one worker thread, one row per workload in `perf/history.tsv`.
- **The gate:** Ir, peak heap and live heap against the parent commit. A rise needs a `cost:` line in the commit message.
- **Wall time** is measured only at milestone ends, with paired alternating A/B runs and a Wilcoxon test, under `nice -n 19` with `/proc/loadavg` logged.
- **This machine is a VM** with 8 vCPUs on shared cores, so wall time is noisy and scaling fits mislead.
- **Sampling profiles:** `perf record -F 499 -g` on a frame-pointer build. Always pass `--no-inline` to `perf report` and `perf script`. Fold stacks with `inferno-collapse-perf`.
- **Traps:**
  - `pkill -f` kills its own shell, so kill by pid;
  - don't wrap commands in `timeout`;
  - don't commit measurement prints;
  - run one heavy job at a time.
