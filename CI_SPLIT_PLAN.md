# Split CI into parallel jobs

## Context

`rust-ci.yml` runs its 16 `gates` checks one after another on one runner (2084 s), and each
`platforms` leg runs clippy, tests, fixtures and the dhat build one after another (Windows 1866 s).
A run takes the sum: 35 min for run 39138433432 (`52844b07`, 2026-10-18). The checks share no
build, so nothing is lost by running them side by side.

The same split was done once on `master3` (`a429aad2`). `MASTER_ONLY_LEDGER.md` lists it as row 949
(ADAPT: "rewrite it onto this branch's version"), with row 948 (rust-cache saves when a job fails) as
its companion. This is that port, plus the `platforms` job, which master never had, plus rows 970,
972 and 973, plus rows 968 and 969 (SKIP in the ledger; see "xtask" below), plus Extension CI.

## Decisions (2026-10-18)

- Layout: one workflow, jobs side by side, setup shared in a composite action.
- Platform legs: three jobs per OS (check, test, fixtures).
- Extras, all four: drop `needs: unit`; row 970; row 973; row 972. Row 948 rides with the split.
- Churn is not a concern; the aim is the most proper form of each part.
- Nothing is edited until the user says to implement.

Added by the properness audit (each open to veto): the test-name manifest runs inside `test` through
`cargo xtask tests --profile test`, which reverses the ledger's SKIP of rows 968 and 969; the bench
check is a step of `lint`; `extension-ci.yml` and `release.yml` use the same setup action;
`timeout-minutes` on every job; row 970 keeps pushes out of the queue (master's block would not);
a measured decision on splitting the test compile.

## What the logs of run 39138433432 show

Read with `gh api repos/darktorres/phpstan-rs/actions/jobs/<id>/logs`, cold caches.

| step (x86_64 Linux `gates`) | wall | of which compile | profile it builds |
|---|---:|---:|---|
| Clippy | 76 s | 74 s | dev (check) |
| Tests | 450 s | 431 s | test |
| Fixture suite | 271 s | 268 s | gate; the 3832 fixtures run in 2.5 s |
| Test-name manifest | 315 s | most | `cargo xtask tests`: test, then gate for benches and fixtures |
| Tests, no default features | 232 s | 216 s | test, other features |
| Fixture suite, no default features | 94 s | 91 s | gate, other features |
| Benches compile | 487 s | 486 s | bench (release, thin LTO) |
| Docs, fuzz, feature matrix, modgraph, arch | 10, 65, 31+, 13, 5 s | | check |
| lockfiles, fmt, machete, deny | 3, 2, 0, 1 s | | none |

Windows: clippy 162 s, tests 1017 s (860 s compile), fixtures 502 s (495 s compile), dhat 129 s.
macOS: 109, 644, 363, 140 s. aarch64 Linux (warm dependencies): 21, 226, 90, 15 s.

1. **The time is compilation.** Tests run 19 s of 450 s. Each profile compiles its own dependency
   set, so no step reuses another's build, and separate jobs lose nothing.
2. **No `gates`, macOS or Windows cache was ever saved.** rust-cache's post step is skipped on a
   failed job; the cache list holds no entry for them, and run 39138433432 printed `No cache found`
   in all three. Only aarch64 Linux, which passes, has one (655 MB). The split pays off warm, so it
   needs `cache-on-failure: true`.
3. **The fuzz cache points nowhere.** `fuzz -> fuzz/target` resolves to `fuzz/fuzz/target` (the
   log's "Cache Paths"); rust-cache's README resolves the target against the workspace entry, and
   the crate's target is `fuzz/target`. The right entry is `fuzz -> target`.
4. **The manifest step builds the gate profile a second time.** `cargo xtask tests` took 315 s in
   CI (`GATE_SPEED_PLAN.md` §2 recorded 4 s locally, before S11 added the gate-profile bench
   listing). The Fixture suite step had built the gate profile for the default-members feature set
   (`cargo test --profile gate --test fixtures`); the manifest lists the benches with `--workspace`,
   whose features unify differently, so it compiles the gate lib again. The log fits: the crate's
   warnings print 3m49s into the step, and the fixtures listing, which matches the earlier build,
   replays its cached warnings at 5m13s. Master's `ad33c992` met the same unification and lists the
   fixtures with `--workspace` too. One recompile is still unexplained: `master3`'s log shows its
   test-profile listing rebuilding `ring`, `webpki`, `mysql` and `phpstan-rs` (3m31s) although the lib
   test binary it then lists has the Tests step's hash, so the unit is the same and cargo judged it
   stale. The plan checks for it (below) and does not depend on the answer.
5. **Bench and fixture names do not depend on the profile.** `debug_assertions` appears in no file
   under `tests/`, `benches/`, `tools/` or `xtask/`; the lib unit tests that do depend on it (for
   instance `project/file_table.rs`' `#[cfg(debug_assertions)] #[should_panic]`) are already listed
   from the test profile. Fixture names depend on `requires_feature` (cargo features), not the
   profile. So listing benches and fixtures from the test profile yields the same set.
6. There is no branch protection or ruleset, so renaming `gates` breaks no required check.
   `actionlint` 1.7.12 passes on the three files today, and `act -l` lists their jobs.

## Design

### Jobs of `rust-ci.yml` (13)

| job | runs on | commands, verbatim from today unless noted | compile unit | cold, est. |
|---|---|---|---|---:|
| `lint` | ubuntu-24.04 | lockfiles, fmt, clippy `--keep-going`, modgraph, arch, doc, fuzz check, `cargo hack`, `cargo check --release --workspace --benches` (row 973), machete, deny | check, no object code | 6.5 min |
| `test` | ubuntu-24.04 | `cargo test --workspace --all-targets --no-fail-fast`, then the manifest: cache restore, `cargo xtask tests --profile test`, keep this run's file, cache save; `fetch-depth: 0` | test | 8.5 to 12.5 |
| `test-no-default-features` | ubuntu-24.04 | `cargo test --workspace --no-default-features --no-fail-fast` | test, other features | 6 |
| `fixtures` | ubuntu-24.04 | fixtures, then fixtures `--no-default-features` (gate profile) | gate | 6.5 |
| `platform-check` | macos-26, windows-2025-vs2026, ubuntu-24.04-arm | clippy, then the dhat build (`RUSTFLAGS` frame pointers) | check | 1 to 6 |
| `platform-test` | same three | `cargo test --workspace --all-targets --no-fail-fast` | test | 4 to 18 |
| `platform-fixtures` | same three | `cargo test --profile gate --test fixtures` | gate | 2 to 9 |

Estimates are from the step times above, before row 972. No job has `needs`, so the run lasts as
long as its slowest job: Windows `platform-test`, about 18 min cold (10.7 min of it is `phpstan-rs`
at the test profile). That is half of today's 35 min; warm, about 14 min. The Linux side tops out
near 12.5 min. 13 jobs + Extension CI's 4 = 17, under the free plan's 20 concurrent jobs; 3 + 1
macOS jobs under its 5 (docs.github.com, Actions limits).

Kept as they are: the `if: ${{ !cancelled() }}` chain between the checks of one job (first check
unguarded), `--keep-going`, `--no-fail-fast`, `fail-fast: false`, the `core.autocrlf false` step
before checkout on the platform jobs, the manifest cache protocol, every pin.
Every job gets `timeout-minutes: 60`: the default is 6 hours, and a hung job holds one of 20 slots
(5 for macOS) that long.

### Shared setup: `.github/actions/setup-rust/action.yml` (new)

A composite action holding what every Rust-building CI job repeats, in today's order and with
today's comments and pins: submodule fetch (`shell: bash`), rustup-toolchain cache (key unchanged, so
existing entries still hit), `rustup install --no-self-update`, `Swatinem/rust-cache` with
`cache-on-failure: true` and `workspaces: ${{ inputs.workspaces }}` (default `. -> target`),
`rui314/setup-mold` under `runner.os == 'Linux' && runner.arch == 'X64'`, and
`taiki-e/install-action` when `inputs.tools` is set. Inputs: `workspaces`, `tools`, `cache`
(default `"true"`). `lint` passes `workspaces: ". -> target" + "fuzz -> target"` and
`tools: cargo-hack@0.6.45,cargo-machete@0.9.2,cargo-deny@0.20.2`. Each job is `checkout` +
`uses: ./.github/actions/setup-rust` + its commands; the platform, Extension and release jobs put the
autocrlf step before checkout, which a composite cannot do.

`extension-ci.yml`'s `integration` job uses it too: its six setup steps are exactly the action's
(submodule, rustup cache and install, rust-cache, mold), and its rust-cache key does not change
(`CACHE_ON_FAILURE` is not hashed, `workspaces` stays the default), so its caches keep hitting. Its
`paths` filters gain `.github/actions/**` (push and pull_request), or a change to the action alone
would not run it.

`release.yml` uses it with a `cache` input set to `"false"` (default `"true"`; it skips the toolchain
cache and rust-cache). Its setup is the same three steps (submodule, toolchain install, mold) with
no caches, and one implementation keeps the mold pin and the toolchain command from drifting. The
caches stay off: `release.yml` has never had them (the commit that added caches to the other two
workflows touched only those, and gave no reason), and a cache saved by a branch run must not reach a
shipped binary. Its macOS target, Rosetta and build steps stay inline. A `workflow_dispatch` dry run
on all five targets (asked first) is its check.

`renovate.json`: the mold custom manager's `managerFilePatterns` becomes
`/^\.github/(workflows|actions)/.+\.ya?ml$/`, because `mold-version` moves into the action. The
cargo-tool pins stay in the workflow. Renovate's default patterns already match `action.yml` files,
and the repo's `helpers:pinGitHubActionDigests` pins the `uses:` lines; confirm on its first run.

Nested post steps: the runner executes the pre and post steps of every action a composite calls (its
ADR 1144; a maintainer repeated it for `actions/cache` in January 2026), so rust-cache's and
actions/cache's saves still run. The open bugs (`actions/runner` #2030, #1434, #2009) concern a
composite nested inside another composite, which this is not. The first run confirms it; the
fallback is the setup inline in each job.

### xtask: `cargo xtask tests --profile <name>` (rows 968 and 969)

Today `cargo xtask tests` builds the test profile, then the gate profile for the benches and the
fixtures, which in CI is two builds beside the ones the `test` and `fixtures` jobs already make.
The flag lists every target from one profile's build (names do not depend on the profile, finding 5),
all three with `--workspace`, so each listing has the feature set of the `test` job's own
`--workspace --all-targets` build (finding 4; this is master's `ad33c992`).
`list_tests(profile: Option<&str>)` adds `--profile <name>` to the first command, uses `<name>` in
place of `gate` for the other two, and adds `--workspace` to the fixtures one; no flag keeps today's
behaviour exactly, so `cargo xtask gates` and the local timings do not change. `USAGE` documents it,
`test_manifest` takes it, and `tests.rs` gains tests for the argument parsing (none,
`--profile test`, a stray argument refused). CI's `test` job then lists right after its own tests:
the benches are already built in test mode by `--all-targets`, and only the fixtures harness is new.
The alternative, a job of its own running plain `cargo xtask tests`, would rebuild the test and gate
profiles that the `test` and `fixtures` jobs build (about 12 min, a runner slot and a 500 MB cache)
to read a list of names.

The ledger SKIPped rows 968 and 969 as master's validation tooling and as dependent on 968. This is
a flag on the branch's own gate, not master's tooling, so the rows become HAVE (ported natively).
That reverses a recorded verdict, so it is called out for the user's veto.

Before the flag lands, check for the unexplained recompile (finding 4, last sentence): in a
throwaway `CARGO_TARGET_DIR` under `~/.cache` (the shared `target/` belongs to another session), run
`cargo test --workspace --all-targets --no-run` from the shell, then
`CARGO_LOG=cargo::core::compiler::fingerprint=trace cargo xtask tests --profile test`, and read the
"dirty because" lines. If the nested cargo rebuilds, the cause is fixed where it lies (xtask's
environment, say) in a commit of its own. If nothing rebuilds locally, the first CI run's manifest
step time shows it (seconds when clean, minutes when not), and a trace is added to one CI run then.

### Splitting the test compile (decided by a measurement)

The test compile is the long pole (Windows 10m44s, macOS 7m06s, Linux 5m26s for `phpstan-rs` alone).
The Tests logs show its shape: the leaf crates `syntax`, `stub-sources` and `types` first (Linux
1m37s, Windows 3m23s), then the `phpstan-rs` lib (Linux 4m31s, Windows 9m18s, up to the moment
`gate-runner` starts), then its dependents, the binary, the integration tests and the benches (55 s
and 1m26s). The lib is compiled twice, as the library and as the unit-test harness, and the two
builds compete for the runner's four cores. A partition of `--all-targets` puts them on two runners:
`cargo test --workspace --lib` and `cargo test --workspace --bins --tests --benches --examples`.

Whether it pays depends on how the compile spreads over the cores, which no log shows. So, before
commit 4: in a scratch target directory, `cargo test --workspace --all-targets --no-run --timings`
and read the report. If the lib-test unit and the lib-and-dependents chain each take over about 40%
of the wall time and overlap, the split would shorten the Windows critical path by roughly the
overlap. It costs three jobs (one more per OS: 16, and 20 with Extension CI, the free plan's cap
with no headroom), so it is taken only for a gain of about 3 min or more, and then as the user's
call with the number in hand. Otherwise it is dropped, with the report's numbers recorded in the
commit message of commit 4.

### What is compiled where

Splitting a leg into three jobs does not compile one thing three times: each job builds a different
artifact set, and the serial leg already built all of them, one per step (every platform log has four
`Compiling phpstan-rs` lines, one per step; the old `gates` job has twelve).

| job | builds | mode, profile |
|---|---|---|
| `platform-check` | every crate and dependency type-checked by clippy; then `phpstan-rs` and its dependencies type-checked again with frame pointers (dhat) | check, no object code; dev, then dhat |
| `platform-test` | everything with object code, opt-level 1 with debuginfo | test |
| `platform-fixtures` | the library and the fixture harness, release optimizations without LTO | gate |

Dependencies were already built once per profile in the serial leg, so no dependency is built twice.
What the split repeats, per extra job: checkout, submodule, toolchain and cache restore (20 to 50 s in
the logs), and the proc-macro crates the old chain shared between clippy and tests (`syn` 2 and
`serde_derive`: seconds, cold runs only). CPU rises by about a minute per job; wall time does not.
On Linux, `test` and `test-no-default-features` each build the same third-party dependencies when
cold; nothing else is built twice.

### Costs to know

- The first run after the change is cold in every Rust CI job (new cache keys). Judge timings from
  the second; `gh run rerun <id>` gives a warm run of the same commit. Jobs of one OS that race to
  save the shared `rustup-…` key log "Unable to reserve cache": a warning, not a failure.
- Repository caches stand at 9.8 GiB of the 10 GiB cap (55 entries): `master`, `master2` and
  `master3` hold 6.5 GiB, this branch 3.3. The 13 job caches add about 3.5 GiB, so LRU eviction
  starts, and takes the untouched `master*` entries first. `v0-rust-platforms-Linux-arm64-*`
  (655 MB) is orphaned by the new job ids and ages out.

## The extras

1. **Drop `needs: unit`** (`extension-ci.yml`). `unit` takes 15 s, so the three `integration` builds
   start at once instead of 15 to 25 s later, and they also run when `unit` fails. With the setup
   action, its rust-cache now saves on failure too (row 948's second file): a red smoke test
   otherwise discards the release-build cache, a 3.5 to 7 min rebuild on every retry.

2. **Row 970, superseded pull-request runs** (`rust-ci.yml` and `extension-ci.yml`, not
   `release.yml`). Master's block is `group: workflow-event-ref` with `cancel-in-progress` only for
   pull requests. GitHub runs one run per group at a time and replaces an older pending run with a
   newer one (docs.github.com, "Using concurrency"), so under that block a second push to a branch
   waits for the first run and a third drops the second, against the commit message's "every pushed
   commit keeps its full result". The port gives a push run a group of its own:
   `group: ${{ github.workflow }}-${{ github.event_name == 'pull_request' && github.ref || github.run_id }}`
   with `cancel-in-progress: true`. A pull request still cancels its superseded run; pushes neither
   queue nor cancel.

3. **Row 973, benches type-checked.** `cargo check --release --workspace --benches` replaces
   `cargo bench --no-run`, which links a thin-LTO bench executable (486 s) that no job runs; the
   check becomes a step of `lint`. `test --all-targets` still runs each bench once in criterion's
   test mode. `--workspace` is the one deviation from master: `gates --full` runs
   `cargo bench --workspace --no-run`, and the old CI step covered only `phpstan-rs`' bench, not
   `syntax`'s `lex_parse`. `gates --full` keeps the full link. The header comment of `rust-ci.yml`
   says so. Docs that say CI builds the benches change with it: `CRATE_SPLIT_PLAN.md` §4 (the CI
   paragraph, which also lists the new jobs, and gate 1's last bullet) and `GATE_SPEED_PLAN.md` S10
   ("`--full` and CI keep it").

4. **Row 972, line tables in the test profile.** `CARGO_PROFILE_TEST_DEBUG: line-tables-only` in the
   workflow-level `env`, next to `CARGO_TERM_COLOR`: one line to delete if it does not pay. Only
   test-profile builds read it (`test`, `test-no-default-features`, `platform-test`, and the
   manifest's listing inside `test`); rust-cache folds it into every job's key. Backtraces keep file
   and line; only a debugger's variable inspection is lost. It is unmeasured, so it is kept only if
   `phpstan-rs`' compile at the test profile (the span from `Compiling phpstan-rs` to `Finished`)
   drops against the logged baselines: 5m26s Linux, 7m06s macOS, 10m44s Windows, 3m31s aarch64
   Linux. If it does not, a follow-up commit removes the line.

## Commits

One commit per item, each `git commit -m … -- <paths>` (another session shares the checkout and is
editing `Cargo.toml` files). Local builds use a separate `CARGO_TARGET_DIR` under `~/.cache`. Each
commit leaves CI valid. Nothing is pushed without the user's go-ahead. Comments in the YAML follow
the files' style (the why, in plain words) and name no row, phase or task.

0. Two measurements, no commit, before anything else: the diagnosis of finding 4, and the
   `--timings` report of the test compile (see "Splitting the test compile").
1. xtask: `tests --profile <name>`, with its tests (`cargo fmt`, `clippy -p xtask`, `test -p xtask`).
   If the diagnosis finds a cause in xtask's environment, a commit of its own follows.
2. Row 973 on the serial `gates` job (`cargo check --release --workspace --benches`, run once
   locally first), the header comment, `CRATE_SPLIT_PLAN.md`, `GATE_SPEED_PLAN.md`.
3. `.github/actions/setup-rust/action.yml`, `renovate.json`, `rust-ci.yml`: `gates` becomes `lint`,
   `test` (with the manifest), `test-no-default-features` and `fixtures`; `timeout-minutes`; the
   header comment describes the layout.
4. `rust-ci.yml`: `platforms` becomes `platform-check`, `platform-test`, `platform-fixtures` (or the
   split of the test compile, if the measurement and the user say so; the numbers go in the message).
5. Row 972: the `env` line in `rust-ci.yml`.
6. Row 970: the concurrency blocks in `rust-ci.yml` and `extension-ci.yml`.
7. `extension-ci.yml`: `integration` uses the setup action; the `paths` filters gain
   `.github/actions/**`.
8. `extension-ci.yml`: drop `needs: unit`; `timeout-minutes`.
9. `release.yml`: the three setup steps become the setup action with `cache: "false"`.
10. Docs: `MASTER_ONLY_LEDGER.md` rows 948, 949, 970, 972 and 973 ADAPT to HAVE and rows 968 and 969
   SKIP to HAVE, with their commits and the deviations above (Progress counts: ADAPT 264, SKIP 838,
   HAVE 24 become 259, 836, 31); `CROSS_PLATFORM_PLAN.md` R2 gets one line saying the `platforms`
   job is now three jobs (its round results keep their text). No `WORK_STACK.md` entry if this
   finishes in one session; otherwise it goes on top of the stack per CLAUDE.md.

## Left out, each checked

- Row 937 (ubuntu-26.04): the owner recorded that the glibc floor is decided first, so it is not
  taken here.
- The x86_64 Linux jobs stay apart from the platform matrix: folding them in saves two job
  definitions but needs conditionals for the manifest and the full-history checkout and adds a
  clippy-plus-dhat leg, and the two families keep the roles the cross-platform plan's R2 gave them.
- A Windows `cargo doc` gate: the gap exists in principle (`cfg(windows)` items are never documented
  on Linux, so a broken intra-doc link there goes unseen), but a scan of every Windows-gated item
  finds none with an intra-doc link today, so there is nothing to catch yet. It adds a gate whose
  first red would be unrelated to this change. `--no-default-features` on the platforms: the pure
  Rust paths are the same on every OS, and the default build already covers the C dependency.
  Both are coverage decisions, not splits.
- Reusing unchanged leaf crates between runs. The leaf chain (`syntax`, `stub-sources`, `types`) is
  1m37s of Linux's test compile and 3m23s of Windows', and of the last 138 commits that touch code
  or manifests, 83 touch only `phpstan-rs` (`syntax` 11, `types` 6, `composer` 23). So the chain is
  reusable in most runs. It needs `cache-workspace-crates` and a freshness rule that does not trust
  checkout mtimes (`-Zchecksum-freshness`, unstable and tied to the pinned nightly; restored mtimes
  are unsafe, since a changed file whose commit time precedes the cached build would be served
  stale and CI would go green on old code). That changes how CI decides what is stale, so it is its
  own decision with its own check.
- An aggregate "all green" job and a `permissions:` block: there is no branch protection to feed the
  first, and the second does not touch the split.
- A `save-if` default-branch policy (it would slow the other branches' CI), `sccache` (it hits only
  on identical invocations, which the restored dependency cache already covers, and the workspace
  crates are the leaf chain above), and a change to `-Z threads=8` (set for x86_64 Linux only; no
  log says whether four cores oversubscribe it): none of them is part of splitting.

## Verification

1. `cargo xtask tests --profile test` and `cargo xtask tests` list the same names (copy
   `.corpus-runs/gate/tests/<hash>.txt` between the runs and diff), and `cargo test -p xtask`
   passes. The manifest needs a clean tree, which the shared checkout is not while another session
   edits it, so this runs in a scratch worktree of HEAD (only `vendor/phpstan-src` initialised,
   `--reference` to the main checkout) with its own target directory, removed afterwards.
2. `actionlint .github/workflows/*.yml` exits 0 (it checks `with:` against the new action's inputs,
   and the concurrency and env expressions).
3. `act -l` lists the seven Rust CI job ids (it lists a matrix once) and Extension CI's two, all in
   stage 0: `integration` leaves stage 1 with `needs: unit`.
4. The composite parses (`python3 -I -c 'import yaml; …'`): `runs.using: composite`, every `run:` has
   a `shell:`, every nested `uses:` is pinned to the same SHA as before; `renovate.json` still
   parses (`python3 -m json.tool`).
5. The old and new `run:` lines match as multisets (a script over both files), except the bench
   line and the manifest's `--profile test`: no check is dropped or doubled.
6. `release.yml` by `workflow_dispatch` on the branch (asked first): all five targets build, and each
   packaged binary runs once, as the file's own steps check.
7. Push only after the user's go-ahead. Then: all 13 jobs appear; each job's post step prints
   "Cache saved" (red ones too); `gh api repos/darktorres/phpstan-rs/actions/caches` lists
   `v0-rust-<job>-…` entries and `v0-rust-integration-…` keeps hitting; per-job durations against
   the table above; the manifest step takes seconds, not minutes; `lint` shows the bench check;
   row 972's compile spans against the baselines. A second push made while the first run is going
   shows both proceeding; `gh run rerun` shows the warm restore.
