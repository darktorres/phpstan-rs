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
- `analyse`: ~38 ms on a one-line file (~4 ms after S5 and S6; see S8).

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
  isolation. The corpus has no panics, so snapshots don't change. S5b
  and S5c land as one commit: the per-file `catch_unwind` is the
  parallel loop's body. A lib test injects a panic through a
  `#[cfg(test)]` hook (`project/tests/panic_injection.rs`) and checks
  that only that file's facts are lost. It's a lib test rather than a
  fixture, because fixtures link the library without `cfg(test)`.
- Verify: `project/property_assign_facts` and `checker/property_assignment`
  identical; LSP tests pass.

### S6. Case-folded function keys, pulled forward from C5 (engine)
- The C5 item "a case-folded function key map replacing the linear
  scan" lands now, in its designed form.
- `Registry::functions` is a `FunctionTable` (`registry/function_table.rs`):
  a map-like type keyed by the case-folded name, each entry keeping the
  declared spelling beside its `FunctionSig`. `FunctionSig` is also the
  method signature type, so it doesn't get a name field. Every lookup
  method folds, so no call site can skip it; iteration yields declared
  names. The side structures keyed by function name
  (`unconditionally_declared_functions`, `stub_provided_functions`,
  `function_decl_sites`) fold through the same `function_key`.
- Sites that used exact `get` before, bypassing `lookup_function`, now
  resolve case-insensitively too, as PHP does. That only matters for a
  call whose case differs from its declaration's.
- The case-collision survey found none: none among the 5,090 stub
  functions, and in the corpus only three names inside php-cs-fixer
  heredoc code samples, which aren't declarations. S6a isn't needed.
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
- The session canonicalizes its root once (`Project::set_absolute_root`),
  and so does the LSP. `Project::absolute_path` maps an analysed path
  spelled from the working directory onto it: swap the spelled root for
  the canonical one, then resolve `.` and `..` lexically. Paths keep
  their spelling everywhere they're shown.
- Each `ComposerManifest` keeps its spelled paths for display and
  records its canonical root, and `ComposerManifest::absolute` maps its
  own paths the same way. `path_starts_with` is a pure lexical
  comparison, and autoload coverage and exclusion are manifest methods
  on absolute paths. No `getcwd` remains.
- Package ownership has one implementation: `composer::PackageIndex`,
  built from every composer root's manifest. It maps an absolute path to
  the deepest owning directory: a root's own, or an installed package's
  `vendor/` link or canonical install directory.
  - A file reached through a `vendor/` symlink matches the link, and a
    file inside a path-repository package matches the install directory,
    both lexically. That's what the per-file `canonicalize` did before,
    so no file is canonicalized any more.
  - Each file's owner is computed when it's inserted, or when the index
    changes, and stored in its `FileState`.
  - Composer hygiene runs per root index. It reads the owners, keeping
    only installed packages of its own root as before, and
    `belongs_to_root` becomes "owned by this root".
  - The PHPStan-namespace rule takes the owner's name, and
    `registry.package_dirs`, both old `owning_package`s and `OwnerCache`
    are gone.
- `includes.rs` drops its `current_dir()` fallback: a relative path's
  `is_file()` already resolves against the process. `__FILE__` comes
  from `Project::absolute_path`, the canonical path PHP defines it as,
  instead of `std::path::absolute`.
- **S7a (behaviour, only if output changes):** with nested composer
  roots, a file inside a sub-project used to count for both roots in
  composer hygiene and now counts only for its own. And the owner of a
  file now considers root packages alongside installed ones. If the
  final run shows a change from this, it lands as its own behaviour
  commit.
- The arch gate gains the part of §4.10's "no engine I/O" this step
  makes true: no `std::env::current_dir`, `fs::canonicalize` or
  `std::path::absolute` in the package outside `project::session`, the
  binary, the LSP and tests. xtask tests show the rule firing and its
  exemptions.
- File-existence checks (`includes.rs`'s `is_file()`) remain engine I/O.
  F1's VFS replaces them, because each lookup there also records a
  dependency for F2's result cache; building that ahead of F2 would be
  thrown away.
- Verify: `project/composer_hygiene`, `checker/includes`,
  `project/phpstan_api_gating` and `checker/phpstan_api` identical;
  composer tests pass; `cargo xtask arch` passes.

### S8. Stub-proportional work in per-project passes (engine)
- Profiled at S7 with timers around `analyse`'s parts, over
  `checker/cast_and_coercion`: a one-line fixture's `analyse` now takes
  ~4 ms, not ~38 ms. The facts pass and the main check take under 1 ms
  each. The rest is ~2.5 ms of project-level passes after
  `check_whole_program`, about 1 s across the whole suite on 8 threads.
- The drop came from S6, which removed the linear scan over every
  function on each namespaced-call miss, and from S5.
- No change: the remaining 2.5 ms isn't material next to the ~210 ms of
  stub setup §4 leaves to C4/C5.

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
- S9a and S9b land as one commit, since the binary's rewrite uses both
  the finalizer and the entry point. The `.err` notices come back as
  lines from `CheckRun::notices`, `AnalysisResult::notices` and
  `Report::notices`. The flag overlay stays in the binary, before
  `app::check_project::prepare`; the runner's configurations need none.
- The runner writes `report`, `emission` and `baseline` `.json`/`.err`
  pairs, plus `types.txt`, per configuration. A smoke run on a small
  project matched the binary's `--emission-order` stdout and stderr
  byte for byte. The module graph assigns `app` to the `app` crate.
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
- **The benches under `cargo test --all-targets`:** S0 measured
  `vendored_corpus` at 80 s in test mode, debug. It isn't redundant: it's
  the only per-commit check that loading and checking all 2,351
  phpstorm-stubs and phpstan-src files doesn't panic. So it stays in the
  quick tier, made cheap instead:
  - the debug test step runs `--lib --bins --tests`;
  - a `gate`-profile step runs `--bench '*'` in test mode, reusing the
    library the fixtures already built. Both benches take 15 s there,
    against ~82 s in debug;
  - the test manifest lists each target from the profile it runs in.
- **Still open:** the dev-profile `opt-level` experiment and lib tests in
  an optimized profile. Each needs full dependency rebuilds and suite
  runs to measure. A side result: the gate-profile lib test run took
  47 s against 124 s in debug, but that doesn't count its separate
  compile. Whether to run them is the user's call.
- The commit records every measurement, whichever way each decision
  goes.

### S12. Fixed sleeps in LSP tests (harness)
`run_across_debounce` sleeps a fixed 500 ms. Replace the sleep with a
wait on an explicit signal from the server's run loop, such as a
debounce-drained notification the test can observe, so the test is
neither slow nor timing-dependent.

### S13–S15: from profiling the test suites

After S12, `perf` with frame pointers over three workloads: 257 fixtures
(gate profile), the whole lib-test binary (debug) and one debug
`check-project` spawn as a cli test makes. Profiling builds live in
`target/prof`.

- Fixtures: the analysis itself is 0.5% of samples. `finish_loading`'s
  passes over the stub classes take 21.8% on the calling thread, plus
  their parallel part on the pool. `Project::new`'s stub copy takes 14.3%,
  and dropping that copy 10.6%. About 48% of samples are idle pool
  threads spinning in `sched_yield`.
- Lib tests: 51% of samples are under `flatten_phpdoc_inheritance_one_pass`'s
  parallel closure, 32.5% in `Registry::all_ancestor_names` and about 30%
  in hashing.
- CLI spawn: 11.8 s for a one-line file, 59% of it `generated_stub_data`
  lexing, parsing and indexing the 2,351 stub files one after another.

### S13. Ancestor bindings computed once per class (engine)
- `flatten_phpdoc_inheritance_one_pass` rebuilds a class's ancestor
  template bindings (`all_ancestor_names`, then `class_template_bindings`
  per templated ancestor) for every method that needs a backfill, on
  every fixed-point pass. They depend only on the class's declared
  hierarchy and template arguments, which the pass never changes.
- `flatten_phpdoc_inheritance` keeps one memo per class for the whole
  fixed point, filled lazily from the parallel closure (a `OnceLock` per
  class, prepared before the first pass). A method asks for the bindings
  only once a backfill candidate passes its cheaper checks. Output is
  identical.
- Replacing the whole-registry fixed point with per-class resolution is
  C4's job, so the pass keeps its shape.
- Verify: `checker/generics_and_templates`, `checker/oop_overrides_and_inheritance`
  and `checker/type_declarations` identical; registry tests pass.

### S14. Stub data generated in parallel (engine)
- `generated_stub_data` lexes, parses, rewrites stub attributes and
  indexes each of the 2,351 files one after another. That per-file work is
  independent: it runs on rayon, and the results are merged
  sequentially in today's file order, so which declaration wins, the
  extension maps and the constant merge are identical.
- The `LazyLock` runs on whichever thread first asks. So the parallel
  part runs on the global rayon pool, not the checker's worker pool:
  stub files are flat declarations, far from the parser's recursion
  limits.
- C5 replaces `generated_stub_data` with lazy resolution from the
  `stub-sources` index; its gate compares against this function's result.
- Verify: the `php_stubs` tests and the stub-index regeneration test
  pass; a cli test spawn is timed before and after.

### S15. Pool and test-thread sizing (measure, then decide)
- About 48% of fixture samples are pool threads spinning between each
  fixture's tiny jobs, taking CPU from the 8 test threads. The lib tests'
  registry passes run their `par_iter`s on the global rayon pool, which
  has the same effect.
- Measure the fixture suite with the harness pool at 1, 2 and all cores,
  and the lib suite with `RAYON_NUM_THREADS` at 1, 2 and all cores. Adopt
  the fastest setting: the harness takes its pool size from the
  measurement, and the gate sets the lib tests' global pool size.

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
  compared with the §2 table.
- CHANGELOG notes the `check-project` speedup from S2–S7.

**Result** (6e186945): every S0–S12 commit is in, and one `cargo xtask
gates` run passes. Every snapshot is byte-identical to the S3a
recording (`78047041`), so no approved delta was needed: S3a, S5a, S6
and S7's possible behaviour changes changed nothing on the corpus or
the fixtures. The quick tier took 1,008 s (16.8 min), against 2,252 s
(37.5 min) at `8682ba07`:

| Step | Before | After |
|---|---|---|
| corpus step | 667 s | 172 s (gate build 84 s, scoped 27 s, all-on 32 s, determinism 28 s) |
| `cargo bench --no-run` | 570 s | `--full` only |
| benches in test mode | ~82 s (debug) | 14 s (gate profile) |
| type-dump compression | 45 s | 0.6 s |
| `--no-default-features` | 75 s (build) | 8 s (check) |
| fixture suite | 483 s | 391 s |
| lib + cli tests (debug) | 124 s + 173 s | 172 s + 200 s |

What's left is the debug lib and cli tests (~6 min) and the fixture
suite (~6.5 min), each dominated by stub setup that §4 leaves to C4/C5.
S11's open question, running the debug tests in an optimized profile,
is the user's call.
