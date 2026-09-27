# Making the gates faster

`cargo xtask gates` takes 37.5 minutes on the quick tier (§2) and about
45 more on `--full`. This plan cuts that without weakening any gate. It
sits beside `CRATE_SPLIT_PLAN.md` and runs before Stage C.

Where the proper fix for a cost is already designed in the crate-split
plan, the step pulls that item forward in its final form rather than
landing a stopgap Stage C–F would replace. The crate-split plan is not
rewritten. §5 lists the targeted edits that record each item landing
early; each edit goes in with the step that causes it.

## 1. Rules

- Structural steps are behaviour-neutral: corpus, type, baseline and
  fixture snapshots stay byte-identical.
- A step whose proper form changes output lands that change first, as
  its own behaviour commit with an approved delta. Each changed line is
  reviewed for runtime correctness. The structural commit then follows
  with no delta.
- No gate loses coverage. A check moves out of the quick tier only if it
  is redundant there. `--full` and CI keep running it.
- One commit per step, checked cheaply before the next starts: fmt,
  clippy, the unit tests of the touched modules, and the fixture
  directories each step names, byte-identical
  (`cargo test --profile gate --test fixtures -- <dir>`).
- No per-step timing and no per-step corpus run. The corpus runs only at
  S3a, whose type-snapshot delta has to be recorded and approved, and
  once at the end (§6), together with the whole fixture suite, the full
  `gates` run and the timing against §2.
- If the final run shows a change no approved delta covers, it is traced
  to the step that caused it. That change then lands as its own
  behaviour commit with an approved delta, per the rule above, before
  the plan is done.

## 2. Measured state

S0's first quick-tier run, at `8682ba07` (`gatetime/8682ba07….gates.tsv`),
took 2,252 s (37.5 min) in total:

| Step | Wall | Cause |
|---|---|---|
| `cargo bench --no-run` | 570 s | a release build with thin LTO and one codegen unit; `test --all-targets` already compiles every bench |
| fixture suite (gate profile) | 483 s | ~250 ms per fixture, see below |
| cli tests (debug) | 173 s for 44 tests | ~60 spawned debug binaries, each parsing every stub |
| lib tests (debug) | 124 s | per test: stub clone and registry precompute |
| `vendored_corpus` bench in test mode | 80 s | one debug pass over 2,351 vendored files |
| corpus step | 667 s | the gate build (114 s) and six analysis runs (below) |
| `zstd -19` of the two corpus type dumps (~115 MB each) | 24 s + 21 s | `-3` takes 0.3 s and writes 1.5 MB instead of 1.0 MB |
| `--no-default-features` build | 75 s | a debug build of the package with different features |
| fuzz check | 21 s | |
| clippy, fmt, doc, arch, module graph, test manifest | 10, 2, 6, 6, 6, 4 s | |

The quick tier's six corpus runs:
- the recorded scoped run (57 s) and all-on run (121 s). Both include
  `--dump-types`' second checker pass (S3). Measured on their own,
  without it, they took 29 s and 71 s;
- the same two with the reference baseline applied (40 s and 89 s);
- two more all-on runs with `--emission-order` for the determinism check
  (90 s and 91 s).

`--full` adds the unscoped run, two more unscoped runs for determinism,
and 22 per-level runs (~17 min).

**Corpus runs are mostly serial.** A scoped run uses 1.7 cores of 8 and
an all-on run 1.4. In the frame-pointer profile of all-on, the main
thread does 57% of all CPU work:
- `collect_named_argument_facts` (~24% of total CPU): a facts-only
  checker pass over every loaded file, serial, only when
  `check_renamed_named_arguments` is on. Neither of its inputs needs it:
  - the override facts come from class declarations plus registry
    lookups (`record_named_argument_renames` walks a `ClassDecl`, and
    `record_one_named_argument_rename` reads only `registry.classes` and
    `find_method_signature_prototype`), with no inference;
  - the call sites only matter in analysed files, because
    `whole_program_diagnostic` drops every other path.
- `collect_property_assign_facts` (~14%): a facts-only pass, serial, on
  every configuration. Its results are merged in `files.keys()` order,
  which is hash-map iteration order and depends on insertion history,
  so the LSP's incrementally edited map can merge differently from the
  CLI's.
- `check_composer_hygiene` (~7.5%): `ComposerManifest::owning_package`
  runs `canonicalize` for each file, then `path_starts_with` for every
  installed package, which calls `getcwd` whenever a package doesn't
  match. Nothing is cached, and `belongs_to_root` bypasses `OwnerCache`.
  This is engine-side I/O, which §3.8 of the crate-split plan rules out.
- `Registry::lookup_function`'s miss fallback (~11%): a linear
  `eq_ignore_ascii_case` scan over about 5,000 functions. Every
  unqualified call in a namespace misses first on `Ns\fn`. The
  crate-split plan's C5 already replaces this with a case-folded key
  map.

**Per-fixture cost:**
- `Project::new`: ~65 ms to clone and merge the stubs;
- `finish_loading`: ~145 ms to re-run every registry pass over the stub
  classes;
- `analyse`: ~38 ms on a one-line file, cause not yet profiled (S8).

Each fixture also builds its own worker pool sized to all cores, so 8
parallel fixtures run 64 threads. About a third of the CPU goes to
`sched_yield` and futex spinning, and 8 test threads are only 1.6×
faster than one.

## 3. Steps

The order is the order of the sections below.

### S0. Gate step timing (xtask)
- `gates` and `corpus` print each step's wall time and write
  `.corpus-runs/gate/gatetime/<hash>.<command>.tsv` (step, seconds), with
  `gates`, `gates-full` or `corpus` as the command. The file is written
  even when a step fails. The baseline in §2 and the final run in §6
  come from it.
- `cargo test` runs its test binaries one after another, so each
  target's own time (lib, cli, each crate's tests, fixtures) is recorded
  separately from libtest's `finished in` summary.

### S1. Type-dump compression (xtask)
- `zstd -3 -T0` instead of `-19` for the `types-*` snapshots.
- `compare` already decompresses, so older `-19` snapshots stay readable.

### S2. Named-argument facts without a second pass (engine)
This is the collector shape of crate-split §3.8.
- The rename overrides come from a walk over each loaded file's
  top-level statements, with registry lookups and no checker. Today's
  recording runs only in `check_program`'s top-level loop, so the walk
  covers exactly that:
  - top-level `ClassDecl` statements, with names resolved against the
    `namespace` and `use` statements seen so far in the file. Nested,
    conditional and anonymous classes aren't reached today, and enums
    are a separate statement kind;
  - each class's own methods in source order, then its used-trait
    methods sorted, as `record_named_argument_renames` did.

  It lives in the rule's own module, `whole_program/renamed_named_arguments.rs`,
  with the cross-referencing.
- The named-argument call sites are collected by the main check pass
  of the analysed files, as per-file data merged on the main thread in
  sorted path order.
- Both main-pass entry points return the collected call sites: the
  plain check and the tier-2 check that also collects member references.
  `record_named_argument_call` already runs in both modes.
- `collect_named_argument_facts` and the checker's override recording
  are deleted, and `check_whole_program` reads the collected data.
- Loop bodies are checked several times, and call sites recorded on
  earlier passes are kept. That's identical today in both the facts pass
  and the main pass.
- One real difference: the main pass has the cross-file property-assign
  facts and the facts pass didn't, so a receiver typed through a
  property can resolve differently. If the final run shows a changed
  `argument.parameterRenamedInSubtype`, it comes from here. Keeping only final-pass
  data is the Stage D dispatcher's job ("drops non-final-pass output").
- Verify: `project/renamed_named_arguments` identical.

### S3. Type dump from the main pass (engine and gate)
- `--dump-types` and `PHPSTAN_RS_DUMP_TYPES` go through
  `Project::dump_types`, which runs a second full checker pass per
  analysed file through `checker_for`. That checker never gets the
  cross-file property-assign facts `check_file_impl` passes to the main
  pass. So the `types-*` snapshots record types the diagnostics never
  see, and every recorded corpus run and every fixture in `gates` pays
  for a second pass.
- **S3a (behaviour, gate snapshots only):** the dump walk gets the same
  cross-file property-assign facts as the main pass. `types-scoped`,
  `types-all-on` and `types-fixtures` change wherever those facts change
  an inferred type. The commit carries the approved delta, and each
  changed line is checked to be the type the diagnostics already used.
- **S3b (structural):** the dump is taken from the main-pass checker
  (`set_dump_types` on the checker in `check_file_impl`), and
  `Project::dump_types`' separate walk is deleted. Diagnostics and type
  snapshots are identical to S3a's, since the dump only records.
- The dump comes back as data in the analysis result, and whoever
  requested it writes the file: the binary, the fixture harness, and
  from S9c the gate runner. Today `Session::analyse` writes it and prints
  `failed to write type dump` itself, which is library I/O and a print
  in a library.
- E6 later re-expresses the dump as an observer on the main pass; the
  single-pass property this step establishes carries over.
- Verify:
  - at S3a, a corpus run, with its type-snapshot delta reviewed;
  - at S3b, the type dumps of `project/property_assign_facts` and
    `checker/narrowing` identical to S3a's.

### S4. One injected worker pool (engine and harness)
- `Session` stops building a pool per session. It takes a shared
  `Arc<rayon::ThreadPool>`, which is the 64 MiB-stack pool crate-split
  §3.10 assigns to the Session.
- The binary builds one pool per process. The fixture harness builds
  one for the whole suite: every fixture runs its work on it, and
  work-stealing replaces the 64-thread oversubscription.
- The LSP doesn't use `Session` today. It calls `Project` directly
  (`check_paths_isolating_panics`) and checks serially. It builds the
  same process-wide pool and passes it to `Project`, which S5b's
  parallel facts pass needs. The re-checked paths run on that pool too.
  F1 later moves the LSP onto `Session`, which then carries the pool.
- Verify: `checker/cast_and_coercion` and `project/property_assign_facts`
  identical; LSP tests pass.

### S5. Property-assign facts in parallel, in a defined order (engine)
This is the per-file-then-merge summary shape of crate-split §3.8.
- **S5a (behaviour, only if output changes):** merge the per-file facts
  in sorted path order instead of `files.keys()` order. The CLI and LSP
  then agree by construction. If the corpus or fixtures change, the
  commit carries the approved delta.
- **S5b (structural):** compute each file's facts on the injected pool
  from S4 and merge them sequentially in that same order. The LSP's
  `check_paths_isolating_panics` and `check_paths` use the same
  function on the same pool. Nothing touches rayon's global pool: its
  default worker stack is too small for the checker's deep recursion.
- **S5c (behaviour, only under a panic):** panics are isolated per file.
  Today a single panic anywhere empties the facts for every file, both
  in `session.rs` and in the LSP's `check_paths_isolating_panics` (a
  `catch_unwind` around the whole pass, then `unwrap_or_default`).
  `check_paths` stays panic-transparent, as its tests expect.
  After this, a file that panics contributes no facts, and the other
  files keep theirs. This matches the main pass and F1's panic
  isolation. The corpus has no panics, so snapshots don't change. A
  fixture that injects a panic covers the new behaviour.
- Verify: `project/property_assign_facts` and `checker/property_assignment`
  identical; LSP tests pass.

### S6. Case-folded function keys, pulled forward from C5 (engine)
- The C5 item "a case-folded function key map replacing the linear
  scan" lands now, in its designed form.
- `Registry::functions` is keyed by the case-folded name, and
  `FunctionSig` carries the declared spelling. There's one map and no
  side index, so no mutation path (stub merge, project indexing,
  `remove_declarations`, LSP updates) can leave the two out of step.
- The 59 access sites go through the folded key. Sites that print a
  function name use the declared spelling from the signature.
- Class keys are left alone. C3's reflections, keyed by `syntax::Name`
  with its precomputed lowercase key, replace both the class map and
  `classes_lowercase`, so converting their 181 access sites now would
  be thrown away.
- In PHP, two declarations whose names differ only in case are one
  function. So a case-only collision goes through the same policy as an
  exact-name collision (`registry/index.rs`):
  - a project declaration wins over a stub (`merge_stubs`'
    `or_insert`);
  - conditional declarations are merged (`existing.merge`);
  - otherwise the later insert replaces the earlier one.
- Before changing the map, list the case-colliding keys in the stubs
  and the corpus. If any exist and output changes, that lands as S6a
  with its approved delta.
- Verify: `checker/symbol_existence` (which holds 28 of the 39
  `nameCase` fixtures) and `checker/codemods` identical; registry and
  LSP tests pass.

### S7. Composer hygiene without engine I/O (engine)
- The session resolves its root to an absolute path once, at intake,
  and hands the project that absolute root for resolution. It keeps the
  root's own spelling (`.` for the CLI) for display, so reported paths
  don't change. Today `resolved()` joins onto the spelled root `.` and
  calls `std::path::absolute`, which is where the per-call `getcwd`
  comes from.
- `path_starts_with` becomes a pure comparison of absolute paths, with
  no `getcwd`.
- `includes.rs` resolves against the absolute root instead of falling
  back to `current_dir()`.
- Package ownership gets one implementation, computed once per file
  when the session loads it and stored with the file. Today there are
  two, and they differ:
  - `ComposerManifest::owning_package` (composer hygiene) canonicalizes
    the file before matching install directories, never returns the
    root package, and returns the whole `InstalledPackage`;
  - B8's `owning_package(dirs, file)` (the PHPStan-namespace rule)
    matches the path as written against both vendor and install
    directories, includes the root package, and returns only a name.

  The single version returns an owner, either the root package or an
  installed package's index, and resolves symlinks. The canonical path
  comes from the session's one canonicalization per file at intake,
  which is session I/O, not engine I/O. Composer hygiene,
  `belongs_to_root` and the PHPStan-namespace rule all read it, and
  `OwnerCache` is deleted.
- **S7a (behaviour, only if output changes):** if the two rules ever
  disagreed on a file, reconciling them changes one rule's output. That
  lands first, with its approved delta.
- The arch gate gains the part of §4.10's "no engine I/O" this step
  makes true: no `std::env::current_dir` or `fs::canonicalize` outside
  `project::session`, the binary and the LSP.
- File-existence checks (`includes.rs`'s `is_file()`) remain engine I/O.
  F1's VFS replaces them, because each lookup there also records a
  dependency for F2's result cache; building that ahead of F2 would be
  thrown away.
- Verify: `project/composer_hygiene`, `checker/includes`,
  `project/phpstan_api_gating` and `checker/phpstan_api` identical;
  composer tests pass; `cargo xtask arch` passes.

### S8. Stub-proportional work in per-project passes (engine)
- Profile a one-line fixture's `analyse` (~38 ms).
- Whatever the profile shows as proportional to the stubs rather than
  to the project is made project-proportional, where the result can't
  change. Project-level passes (`check_whole_program`,
  `check_class_shapes`, `check_banned`, `check_architecture`) are
  candidates. At least one returns early under a fixture's default
  settings: `class_shape.rs:105`'s walk over every class only runs when
  closed-class or parent-call checks are on. So the profile decides,
  not guesses.
- Verify: the profiled fixtures identical. The profile before and after
  the change is the step's own evidence, not a separate timing run.

### S9. Analysis result, finalizer and CLI entry point in the library, plus a gate runner
- **S9a:** F1's `AnalysisResult` and finalizer land now. `analyse`
  returns unfinalized per-file results. One finalizer then applies, in
  today's order:
  - the level filter;
  - inline ignores, including unmatched-ignore reports;
  - `ignore_errors`;
  - the baseline, moved out of `main/check_project.rs`;
  - sorting.

  The level stage is temporary. Crate-split §3.10's finalizer has none,
  because after E5 rules above the level never run, and E5 removes the
  stage then.

  `check-project` and the fixture harness call it, and their output
  stays identical. The LSP doesn't yet: its level cutoff differs from
  the CLI's (crate-split §1), so moving it onto the finalizer is a
  behaviour change. That move happens in F1's LSP-parity commits, with
  their approved deltas.
- **S9b:** everything check-project does after parsing its arguments
  moves from `main/check_project.rs` into the library's app layer
  (crate-split §3.10), behind a plain `CheckRequest` struct. That covers
  config loading, the flag overlay, the baseline settings and
  `Session::resolve`.
  - `CheckProjectArgs` and its clap derive stay in the binary, per
    "No `clap` in any library crate" (§3.7) and the L8 binary. Its only
    job is mapping arguments onto `CheckRequest`.
  - C1's `--set` redesign changes that mapping and the library's
    request handling, and nothing else.
- **Diagnostics beyond the report go through the same path.** The
  gate's `.err` snapshots hold what the binary prints to stderr today:
  - startup warnings (SQL drift, lexer warnings; `check_project.rs:673`
    and `:689`);
  - stale baseline entries (`:744`);
  - check panics.

  §3.10 already puts these in `AnalysisResult` as data. S9a makes that
  true now: the library returns them and prints nothing, and one shared
  library function renders them as text. The binary prints that
  rendering, and the runner writes it to the `.err` snapshot. So the
  runner's `.err` output can't drift from the binary's, and no print
  moves into a library.
- **S9c:** `tools/gate-runner` is a workspace member with
  `publish = false`, not shipped, built with `--profile gate`.
  - It builds the `CheckRequest` for each gate configuration (config
    file, level, `--no-composer`) and resolves it through S9b's entry
    point, the same one the binary uses.
  - The only thing it can do differently from the binary is map a
    gate configuration onto that request. The cross-check below
    compares its output with the shipped binary's on every gate run,
    which catches that.
  - It runs one analysis per configuration, then applies the finalizer
    several times to write the recorded report, the baseline views and
    the emission-order view. It also writes the type dump that S3 returns
    as data.
  - The shipped binary gets no gate-only flags.
  - Once roadmap Phase 181 ships `report --export` (one analysis, with
    every finding tagged by level and by what hides it), the corpus gate
    derives its views from that export and the runner is deleted. The
    gate then runs the shipped code path, with no second mechanism
    doing the same job.
- `corpus` uses the runner. The determinism check compares the runner's
  emission-order view with one fresh `check-project --emission-order`
  run from the shipped binary, in a separate process. It compares stdout
  and stderr, as the determinism check does today. That catches
  process-level nondeterminism, and any difference between the runner
  and the binary; the failure message names both possibilities.
- The cross-check covers each configuration shape the runner maps:
  - the quick tier checks all-on, whose `--config <file>` form is the
    same as scoped's;
  - `--full` also checks unscoped, the `--no-composer <dir>` form.
- Quick tier: 3 analyses instead of 6. `--full`: 2 fewer unscoped runs.
- Per-level runs stay real runs. Under crate-split §3.9 the level decides
  which rules run, and only a level-specific run shows that.
- Verify: cli tests pass. The corpus pipeline it rewires runs for real
  in the final run (§6), where `compare` must find every snapshot
  identical.

### S10. Quick-tier redundancy (xtask)
- `cargo bench --no-run` leaves the quick tier: `test --all-targets`
  already compiles every bench, and it only adds a release-profile
  build. `--full` and CI keep it.
- The `--no-default-features` step becomes `cargo check`; `--full` keeps
  the `--no-default-features` test run.
- `cargo doc` (6 s) and the fuzz check (21 s) stay in the quick tier;
  S0 showed neither is material.

### S11. Measure, then decide (build config)
- **Dev-profile optimization:**
  - third-party dependencies: `[profile.dev.package."*"] opt-level = 1`
    or `2`;
  - leaf workspace crates `phpstan-rs-syntax` and `-types` at
    `opt-level = 3`. `-stub-sources` is only embedded data, so its
    optimization level doesn't matter.

  Time the cli and lib suites and the `buildtime` scenarios. Adopt each
  setting only if a suite drops by at least a third and no `buildtime`
  scenario misses its target.
- **Lib tests in an optimized profile:** the lib suite spends ~200 s
  running in debug. Measure its compile time plus run time under a test
  profile that inherits `gate`, against debug's, for a one-file engine
  edit (the `buildtime` "engine" scenario). Switch `gates` to it only if
  the total is lower. Debug stays the default for `cargo test`.
- **The criterion bench under `cargo test --all-targets`:** S0 measured
  `vendored_corpus` at 80 s in test mode, which is material. The bench
  target gets `test = false`, and `--full`'s `cargo bench --no-run` keeps
  compiling it. Before that lands, check what its test-mode run catches
  that the fixtures and the corpus don't.
- The commit records every measurement, whichever way each decision
  goes.

### S12. Fixed sleeps in LSP tests (harness)
`run_across_debounce` sleeps a fixed 500 ms. Replace the sleep with a
wait on an explicit signal from the server's run loop, such as a
debounce-drained notification the test can observe, so the test is
neither slow nor timing-dependent.

## 4. Deferred to Stages C–F

These are the largest remaining costs. The C4, C5 and D designs remove
them, so an interim fix would be thrown away:
- per-fixture and per-lib-test stub setup: the stub clone and
  `finish_loading` re-running the passes over every stub class
  (~210 ms per fixture). C4 resolves each class lazily and memoizes it;
  C5's process-wide, version-keyed stub layer replaces the clone;
- the cli tests' per-process stub parse, replaced by C5's lazy
  resolution from the `stub-sources` index;
- `check_paths` recomputing property-assign facts over every file on
  every LSP check. The Summaries API (crate-split §3.8), stored per
  reflection snapshot with invalidation keys, replaces it;
- `--full`'s 22 per-level runs each redo parsing and `finish_loading`.
  The level reaches the registry through `set_configured_level` before
  load (`session.rs:509`), so sharing one load across levels needs F1's
  `reconfigure`, which reuses unchanged layers;
- the facts pass still runs the whole checker, with `facts_only` checks
  scattered through it to skip report-only work. Stage D separates rules
  from inference ("analyser emits nothing"), and the facts pass becomes
  inference with no rules registered.

## 5. Edits to CRATE_SPLIT_PLAN.md and ROADMAP.md

Each is a targeted edit in the step's own commit:
- S4, §3.10: the Session takes its pool injected.
- S6, §3.5 enumeration APIs and C5: the case-folded function key map is
  marked as landed early. Class keys stay with C3.
- S7, §4.10: the no-current-dir and no-canonicalize rule is listed as
  already enforced.
- S9a/S9b, §3.10 and F1/F4: `AnalysisResult`, the finalizer and the
  check-project entry point in the library are marked as landed early.
  F1 keeps moving the LSP onto the finalizer as part of its parity work,
  and F1 and F4 keep the rest of their lists.
- S9c, roadmap Phase 181: the corpus gate switches to
  `report --export` and `tools/gate-runner` is deleted.
- S10, §4 tier list: quick drops `cargo bench --no-run` and checks
  rather than builds `--no-default-features`; full keeps both.
- C4 and C5 gates: "the fixture suite and lib test wall times
  (`gatetime`) are recorded and lower than at the phase's start".

## 6. Done when

- S0–S12 are committed. Each structural commit leaves the snapshots
  identical, and each behaviour commit carries its approved delta.
- One `cargo xtask gates` run at the end passes, and its `gatetime` is
  compared with the §2 table (37.5 min at `8682ba07`).
- CHANGELOG notes the `check-project` speedup from S2–S8.
