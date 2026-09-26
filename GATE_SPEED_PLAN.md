# Making the gates faster

`cargo xtask gates` takes about 25–30 minutes on the quick tier and about
45 more on `--full`. This plan cuts that without weakening any gate. It
sits beside `CRATE_SPLIT_PLAN.md` and runs before Stage C; the few edits
it makes to that plan's §4 are listed in §5 and land with the step that
causes them.

## 1. Rules

- Every step is behaviour-neutral. Corpus, type, baseline and fixture
  snapshots stay byte-identical, with no approved deltas. A step that
  would need one is split out and reconsidered.
- No gate loses coverage. A check leaves the quick tier only if `--full`
  and CI still run it.
- One commit per step, verified before the next starts. Engine steps
  check the corpus (`cargo xtask corpus` then `compare`). Harness steps
  check the suite they touch. The full `gates` run happens once, at the
  end (§6).
- Each step records its before and after wall times in its commit
  message, measured on an idle machine.

## 2. Measured state (HEAD `acf0827c`)

| Step | Wall | Cause |
|---|---|---|
| fixture suite (gate profile) | 410–490 s | ~250 ms per fixture, see below |
| lib tests (debug) | ~200 s | per test: stub clone and registry precompute |
| cli tests (debug) | ~150 s for 44 tests | ~60 spawned debug binaries, each parsing every stub |
| `cargo bench --no-run` | minutes | a release build with thin LTO and one codegen unit; `test --all-targets` already compiles the benches |
| doc, fuzz check, `--no-default-features` build | ~1–2 min each | three more compiles of the package |
| corpus step | ~7.5 min | six analysis runs (below) plus the gate build (~70 s) |
| `zstd -19` of each type dump (~115 MB) | 11.6 s ×2 | `-3` takes 0.3 s and writes 1.5 MB instead of 1.0 MB |

The quick tier's six corpus runs are:
- the recorded scoped run (29 s) and all-on run (71 s);
- the same two again with the reference baseline applied;
- two more all-on runs with `--emission-order` for the determinism check.

`--full` adds the unscoped run, two more unscoped runs for determinism,
and 22 per-level runs (~17 min).

**Corpus runs are mostly serial.** A scoped run uses 1.7 cores of 8 and
an all-on run 1.4. In the frame-pointer profile of all-on, the main
thread does 57% of all CPU work, spread over:
- `collect_named_argument_facts` (~24% of total CPU): a facts-only
  checker pass over every loaded file, serial, only when
  `check_renamed_named_arguments` is on;
- `collect_property_assign_facts` (~14%): the same kind of pass, serial,
  on every configuration;
- `check_composer_hygiene` (~7.5%): `ComposerManifest::owning_package`
  runs `canonicalize` for each file, then `path_starts_with` for every
  installed package, which calls `getcwd` whenever a package doesn't
  match. Nothing is cached, and `belongs_to_root` bypasses `OwnerCache`;
- `Registry::lookup_function`'s miss fallback (~11%): a linear
  `eq_ignore_ascii_case` scan over about 5,000 functions. Every
  unqualified call in a namespace misses first on `Ns\fn`.

**Per-fixture cost.** `Project::new` takes ~65 ms to clone and merge the
stubs, `finish_loading` ~145 ms to re-run every registry pass over the
stub classes, and `analyse` ~38 ms on a one-line file. Each fixture also
builds its own worker pool sized to all cores, so 8 parallel fixtures run
64 threads. About a third of the CPU goes to `sched_yield` and futex
spinning, and 8 test threads are only 1.6× faster than one.

## 3. Steps

### S0. Gate step timing
`gates` and `corpus` print each step's wall time and write
`.corpus-runs/gate/gatetime/<hash>.tsv` (step, seconds). Later steps
read their before and after times from it.

### S1. Parallel facts pass (engine)
- One facts-only pass collects both fact kinds:
  - property-assign facts from the files `file_may_write_private_property`
    accepts, as now;
  - named-argument facts from every file, when
    `check_renamed_named_arguments` is on.
- The pass runs on the session's rayon pool over a `Vec` of the paths in
  today's `files.keys()` order. The per-file results are merged
  sequentially in that same order, so every merged `Vec` keeps its
  element order.
- `check_whole_program` takes the named-argument facts from that pass
  instead of running its own.
- `check_paths` and the LSP paths keep their current shape but use the
  parallel pass.
- Verify: corpus snapshots identical; scoped and all-on wall time and
  core usage recorded.

### S2. Case-insensitive function index (engine)
- The registry gets a lowercase function-name index, like
  `classes_lowercase`, kept current everywhere `functions` changes: merge
  paths, `remove_declarations`, LSP updates.
- `lookup_function` and `lookup_function_with_name` use it for the miss
  fallback.
- When two keys differ only in case, today's fallback returns whichever
  comes first in hash iteration order. The index must return the same
  key where that is well-defined. Before building it, list the
  case-colliding function keys in the stubs and the corpus. If there are
  any, the index resolves them by declaration order, as a separate
  behaviour commit with its own approved delta; the main commit must not
  change output.
- Verify: corpus identical; LSP and registry tests pass.

### S3. Composer hygiene lookups (engine)
- Resolve the working directory once per check, not once per
  `path_starts_with` call. `path_starts_with` takes the base explicitly.
- `owning_package` canonicalizes each file once. `belongs_to_root` goes
  through `OwnerCache`.
- Where the answer is the same, use the B8 `PackageContext` and
  `owning_package(dirs, file)` rather than keeping a second ownership
  implementation.
- Verify: corpus identical (all-on runs composer hygiene); composer tests
  pass.

### S4. Type-dump compression (xtask)
- `zstd -3 -T0` instead of `-19` for `types-*` snapshots.
- `compare` already decompresses, so older `-19` snapshots stay readable.

### S5. Quick-tier trim (xtask)
- `cargo bench --no-run`, `cargo doc` and the fuzz check move to
  `--full`. CI keeps running all three.
- The `--no-default-features` step becomes `cargo check`, which still
  catches feature-gating errors but skips codegen and linking. The
  `--full` tier keeps the `--no-default-features` test run.
- Amends CRATE_SPLIT_PLAN §4 (§5 below).

### S6. One worker per fixture session (harness)
- `Session` gets a worker-count setting. `Session::for_fixture` uses 1,
  since libtest-mimic already runs fixtures in parallel. `check-project`
  and the LSP keep using all cores.
- Verify: fixture suite passes; `types-fixtures` identical; suite time
  before and after with the default test-thread count.

### S7. One recorded run, several views (binary and xtask)
- Two hidden `check-project` flags, applied only after analysis:
  - `--also-write-baselined <baseline>=<json>`: writes a second report
    with the baseline applied, plus its stale-entry lines;
  - `--also-write-emission-order <json>`: writes the unsorted report.
- `corpus` records the baseline views from the scoped and all-on runs.
  The determinism check compares the recorded all-on run's
  emission-order file against one fresh `--emission-order` run (and does
  the same for unscoped on `--full`).
- Quick tier: 3 runs instead of 6. `--full`: 2 fewer unscoped runs.
- Per-level runs stay separate. Under §3.9 the level decides which rules
  run, and a level-specific run is what shows that.
- Verify: `compare` finds every snapshot identical to the previous
  commit's.

### S8. Measure, then decide
- **Debug-profile optimization of leaf crates.** Time the cli and lib
  tests with `[profile.dev.package.phpstan-rs-syntax] opt-level = 3`
  (and `stub-sources`, `types`). Adopt it if the cli suite drops by at
  least a third and the `buildtime` lexer scenario stays within its
  target.
- **The criterion bench under `cargo test --all-targets`.** Measure what
  `vendored_corpus` costs in test mode (one debug pass over 2,351 files).
  If it's material, run it under `--full` only by giving the bench target
  `test = false` and building it with `cargo bench --no-run`.
- The commit records both measurements, whichever way each decision goes.

## 4. Deferred to Stage C

These are the largest remaining costs. The C4/C5 designs remove them, so
an interim fix here would be thrown away:
- per-fixture and per-lib-test stub setup: the stub clone and
  `finish_loading` re-running the passes over every stub class
  (~210 ms per fixture). C4 resolves each class lazily and memoizes it.
  C5's process-wide, version-keyed stub layer replaces the clone;
- the cli tests' per-process stub parse. C5 resolves stubs lazily from
  the `stub-sources` index.

C4 and C5 each record the fixture suite and lib test times from S0's
table, and their gates include "fixture suite faster than before" (§5).

## 5. Edits to CRATE_SPLIT_PLAN.md

Each edit is a targeted edit in the step's own commit; nothing else in
that file changes:
- S5, §4 tier list: quick becomes "gates 1–3 without `cargo
  hack`/`machete`/`deny`, doc, fuzz compile or `bench --no-run`, and
  with the `--no-default-features` build checked rather than built";
  full adds those three.
- S7, §4 gate 6: "the baseline views are written by the recorded runs".
- §4 of this plan, C4 and C5 gates: add "the fixture suite and lib test
  wall times (`gatetime`) are recorded and lower than at the phase's
  start".

## 6. Done when

- The S0–S8 commits are in, each with identical snapshots.
- One `cargo xtask gates` run at the end passes, and its `gatetime` is
  compared with the §2 table. The expected quick tier is about 10–12 min,
  down from 25–30.
- CHANGELOG notes the check-project speedup from S1–S3.
