# Project build cost investigation

Measurement-only investigation into why the test suites are slow, which
turned into where every `Project`/`Registry` build and every LSP edit
spends its time. The catalog was then implemented (section 12). The review
fixed three bugs it found (5.1 at `11933e60`, 5.2a at `a754b490`, 5.2 at
`880f1c75`); every measurement probe was temporary and has been removed.
Measured at `708ff201`..`5d86aac1` (2026-09-30); re-measurements made
during review are marked with the commit they ran at. Section 11 logs the
review passes.

- **Resume point:** the catalog is implemented and validated (section
  12, 4a9e90ea..e5124a6e): lib 947/947 (33 s), every workspace test,
  fixtures 3,807/3,807 (222 s under the new `test` profile). None of
  section 4's numbers were re-measured after the changes.
- **Short answer to the original question:** the tests themselves are
  slow, not the harness. Nearly all of a test's time is building (and
  dropping) a `Project` whose registry holds ~3.5k stub classes and ~5.3k
  stub functions, then running 16 whole-registry resolution passes over
  them. The checks themselves are ~1-1.5% of a fixture's time (4.6);
  with parse and config, at most ~7%.

## 1. Environment and method

- VM, 8 vCPUs (1 thread/core), 23 GB RAM; `rustc 1.100.0-nightly
  (2026-09-21)`, mold + `-Z threads=8` from `.cargo/config.toml`.
- Another Claude instance was working in the main checkout the whole
  time, so load was not always quiet. The first run's later measurements
  gate each run on 1-min loadavg < 1.5 and log it; earlier single runs
  vary more. The review's re-measurements ran at load 3-7 and log it.
- Profiles: `debug`/`test` (what `cargo test` uses), `gate` (release
  opts, no LTO, 16 CGUs), plus three throwaway custom profiles made with
  `--config` (see 4.10).
- Probes were `#[test] #[ignore]` modules placed inside the crate
  (`project::…`, `lsp::tests::…`) so they could reach private fields, run
  one at a time with `--exact`, writing results to a file. perf with DWARF
  unwinding produced no user-space stacks on this binary and the VM has
  no PMU (`perf stat -e cycles` reports it unsupported), so timing is
  `Instant`-based plus hotpath once.
- Medians over 7-9 reps unless stated. Within-run spread on this VM is
  ±20-40% (the review's runs spread ±9-30%); two runs of identical code at different times differed by up
  to 65% under load, so only interleaved comparisons (4.11) are trusted
  at small margins.

## 2. Test suite shape

| Suite | Size | What each test builds |
|---|---|---|
| lib unit tests | 918 run by `cargo test --lib` at `5d86aac1` (of the crate's 922 `#[test]`s, three are the bin target's and one compiles only without the default `pcre-validate` feature): 220 in `lsp/tests`, 73 in `project/tests`, 21 in `checker/tests` | By what each test body calls, at `880f1c75` (924 tests): 182 open a whole LSP session (`lsp/tests/helpers.rs`'s `run`/`run_with_capabilities`/`run_across_debounce`/`run_multi`, or `Server::new`); 56 build a `Project`; 12 build `Registry::build` + stubs; the rest neither, except through module-local helpers this count doesn't see |
| fixtures (`tests/fixtures.rs`, `test = false`) | 3,805 fixtures | a full check-project `Session` per fixture (`gate::load_fixture` → `Session::for_fixture` → `load`) |

Debug fixture suite ≈ 3,805 × ~3.4 CPU-s / 8 cores ≈ 27 min, matching the
"~26 minutes" note in `crates/phpstan-rs/Cargo.toml`. That is why the suite
already runs in the `gate` profile (`Cargo.toml`, CI `rust-ci.yml:68`):
≈ 7.2 min by 4.6's measured throughput (5.0 s per 44 fixtures at 8
threads); perfect scaling of the serial 392 ms per fixture would give 3.1
min, so contention costs over half.

## 3. How a Project is built today (three paths)

*Written before the work in section 12: the pass counts and line numbers
below describe the code at `880f1c75`. Today `Project::new` clones a shared
base, a finish runs scoped passes over the edit's closure, and the checker
tests take path 1.*

1. **Production / fixtures / LSP**: `Project::new` (`project.rs:346`)
   clones `stubs::generated_stubs()`, `generated_stub_classes()`,
   `generated_stub_constants()` (each clones out of the process-wide
   `LazyLock` in `registry/stubs.rs:1003`) and merges them
   (`merge_stubs`/`merge_stub_classes`/`merge_stub_constants`,
   `registry/index.rs:478-535`). Those cached stub entries are not raw:
   each stub file went through `Registry::index_and_flatten`
   (`registry.rs:278`) when the cache was built, so every pass but
   purity already ran on them per file (trait flattening, `@method`
   materialization, PHPDoc inheritance, constant-named types,
   drop-doc-members, member owners, ...); what they lack is resolution
   across stub files. Files are
   indexed; then
   `finish_loading` (`project.rs:745`) runs 16 whole-registry passes.
2. **Every LSP edit**: `update_file_cancellable` (`project.rs:1018`;
   the passes at 1098-1138) reruns the same 16 passes over the whole
   registry, then rechecks dependents. A `didChange` is debounced (200
   ms, coalescing per path) and runs it on the debounce worker thread
   (`lsp.rs:763`); `didOpen` runs `update_file` synchronously on the
   dispatch thread, always - even when the text equals what the workspace
   scan already loaded (`handle_open_or_change`, `lsp.rs:721`).
3. **Checker unit-test helpers** (`checker/tests/helpers.rs`,
   `all_diagnostics_for`/`type_dump_for`): `Registry::build(&program)`
   resolves the user program alone, then merges the cached stubs
   (resolved per file only) and reruns only `precompute_ancestor_names`
   + `precompute_member_owners`. Stub classes there never get the
   cross-file passes (a parent, interface or trait in another stub file),
   and user classes are resolved before their stub parents exist - a
   registry production never builds (fidelity gap, see section 5). 12
   tests take this path at `880f1c75` (section 2).

The one-time stub parse (`LazyLock`): 2,515 files (558 phpstorm-stubs +
1,957 `vendor/phpstan-src/src`), each `index_stub_file` →
`Registry::index_and_flatten` (per-file passes), in parallel; plus PHPStan's
41 `stubs/*.stub` overrides, read by `patch_phpstan_stub_classes`. (The
comments in `registry/stubs.rs` no longer count the files.)

## 4. Measurements

### 4.1 Per-Project cost (1 small file, warm process)

| Stage | debug wall | gate wall | gate CPU |
|---|---|---|---|
| stub parse + index, once per process | 2.6-3.6 s (17-19 s CPU) | 0.83-0.89 s | ~5 s |
| `Project::new` (clone + merge) | 116-238 ms | 61-73 ms | ~60 ms |
| `finish_loading` (16 passes) | 730-1,250 ms (~3.0 s CPU) | 165-219 ms | ~550 ms |
| `update_file`, 1 edit | 730-1,190 ms (~2.5 s CPU) | 136-214 ms | 400-540 ms |
| drop Project | 30-86 ms | 15-26 ms | |
| user file load + check | ~1-2 ms | <1 ms | |

Per pass, gate, all stubs (median ms, 7 reps): purity 40,
constant-named types 38, member owners #2 24 / #1 17, phpdoc inheritance
24, drop-doc-members 23, flatten_traits 21, value_of 16, ancestor names
7, canonicalize 5, everything else < 2. Debug: constant-named 290,
drop-doc 203, purity 199, phpdoc 131, member owners 111 + 89, value_of
76, flatten_traits 49. No single hotspot.

Re-measured in debug at `880f1c75` (5 reps, load 5-6, so absolute values
are inflated): constant-named 194, drop-doc 119, member owners #2 101 /
#1 92, purity 93, value_of 66, phpdoc inheritance 27, ancestor names 23,
flatten_traits 16, canonicalize 12, the rest < 3 (assertion table and
global variables < 0.1). The ranking holds, except phpdoc inheritance:
27 ms here against 131 ms above, so that figure is unstable. Purity costs
~93 ms although no stub body is ever classified - it is the iteration,
key cloning and fixed-point sweep over 5.3k functions and 25k methods.

Table re-checks at `880f1c75`: debug `Project::new` 139-164 ms,
`finish_loading` 687-827 ms, `update_file` 633-1,137 ms, drop 41-77 ms
(5 reps, load ~4), within or just under the ranges above. Gate
`finish_loading` 119-122 ms (stubs-only rerun, 9 reps, load ~4) and 179
ms (inside 4.6's fixture re-run, load 5-6), and its CPU ~340 ms per run
(4.9) against ~550 ms above; gate `Project::new` 108 ms in that fixture
re-run, against 61-73 ms above. The one-time stub parse,
the gate per-pass split and the user-file row were not re-measured.

(The first hotpath run reported `finish_loading` = 2.49 s and an LSP test
≈ 3.5 s; both were inflated - by instrumentation and by a loose
two-run subtraction. The figures above supersede them.)

### 4.2 LSP test session

A test session (`initialize` → `initialized` → `didOpen` → `codeAction`
→ `exit`): `Server::new` (`Project::new`, settings, and a fresh 8-thread
worker pool) ~150 ms debug, workspace scan ~1 ms (run synchronously on
`initialized`, `lsp.rs:2662`), `finish_loading` ~0.7-1.0 s,
`publish_workspace_diagnostics` ~1 ms, `didOpen` → `update_file`
~0.6-0.8 s, drop ~50 ms. Total 1.5-2.6 s wall / 3.3-3.7 s CPU debug;
0.43-0.63 s / 1.1-1.4 s CPU gate. Two full pass rounds per session. The
run loop's `RUN_LOOP_TICK` (50 ms) and the 200 ms debounce add nothing
here: the test helper queues every message before the loop starts, and
`exit` wakes the debounce worker at once (`lsp.rs:2090-2091`).

**Contention** (sessions run concurrently in one process, as libtest does):

| | sequential | 2 at once | 4 | 8 | 16 |
|---|---|---|---|---|---|
| debug, per session | 2.6 s | 3.0 s | 4.0 s | 5.8 s | 8.5 s (p90 11.4 s) |
| gate, per session | 0.43 s | 0.68 s | 0.89 s | 1.64 s | 2.4 s |

A full `cargo test` run was observed at 10-14 s per LSP test; libtest
runs 8 at a time, where the table gives 5.8 s, so the rest presumably
comes from the other tests sharing the CPU (not measured).

Re-measured in gate at `880f1c75` (`didOpen` + `codeAction` sessions in
`std::thread::scope`, 3 rounds, load 5-6.7): 615 / 626 / 893 / 1,465 ms
per session at 1 / 2 / 4 / 8 concurrent. The sequential figure is higher
than above under that load; the curve holds.

### 4.3 Stub corpus closure

Checked every class-edge target and every class named inside stub
signature types against the stub class set:

- **phpstorm-stubs are closed**: zero missing extends/implements/uses/
  mixin/trait-alias/type-import/require targets. Only 6 bogus type names
  (`Job`, `long`, `number`, `Couchbase\bstring`, `mysql_xdevapi\?string`,
  `mysql_xdevapi\FieldMetadata`): `long`, `number` and `bstring` aren't
  types, `Job` (zend) and `FieldMetadata` are classes the stubs never
  declare, and `?string` comes from the invalid PHPDoc `?string|?string[]`.
- **PHPStan-src stubs are open**: 16 missing `extends` targets (131
  classes) and 6 missing `implements` targets (19 classes) - PhpParser
  (`NodeAbstract`, `Node\Expr`, `Node\Stmt`, `NodeVisitorAbstract`,
  `PrettyPrinter\Standard`, `Parser`), Nette DI, Symfony Console,
  PHPUnit (`TestCase`, `Extension`), BetterReflection, phpdoc-parser -
  plus 245 missing type-referenced classes over 4,614 signature sites
  (`PhpParser\Node` alone 1,379). In a project that has those packages,
  these stub classes resolve differently.

Re-checked at `880f1c75`: the edge counts reproduce exactly (16 targets /
131 classes, 6 / 19). phpstorm's closure holds for every edge kind: it
declares no traits (all 30 stub traits are PHPStan-src's), so no trait
use or alias can dangle; its three `@mixin` targets (`ArrayAccess`,
`Countable`, `Traversable`) are stub classes; and it has no
`@phpstan-import-type` or `require-extends`/`require-implements` tags. The type-reference count depends on what counts as
a site, which the first run didn't record: over the cached stub entries'
method and function parameter and return types and property types,
phpstorm has exactly the 6 bogus names (12 sites) and PHPStan-src 191
missing names over 3,661 sites (`PhpParser\Node` 1,135,
`PhpParser\Node\Expr` 751).

### 4.4 phpstorm vs PHPStan-src split

| | classes | methods | properties | consts | functions |
|---|---|---|---|---|---|
| phpstorm | 1,542 | 12,031 | 1,600 | 4,813 | 5,273 |
| PHPStan-src | 1,953 | 13,157 | 5,056 | 371 | 9 |

Classification by name prefix `PHPStan\` and by method provenance
(`Origin::Phpstan`) agreed on every class. The counts reproduce exactly at
`880f1c75`; the pass timings below are the first run's only.

`finish_loading` passes on a registry holding only one group (median):

| | all stubs | phpstorm only | PHPStan-src only |
|---|---|---|---|
| gate | 219 ms | 78 ms | 109 ms |
| debug | 1,169 ms | 484 ms | 969 ms |

**PHPStan-src stubs are ~60% of the stub pass cost** (all − phpstorm-only:
141 of 219 ms gate, 685 of 1,169 ms debug). The PHPStan-src-only column
is not their share: that registry lacks the phpstorm parents
(`Exception`, `Countable`, ...), which changes what the passes do - debug
is superadditive (484 + 969 > 1,169). A design that caches only phpstorm
and resolves PHPStan-src per project forfeits most of the gain.

### 4.5 What each pass reads (from code reading)

| Pass | a stub class's result depends on |
|---|---|
| `resolve_method_tags` | only the class itself |
| `flatten_traits`, `inherit_property_hooks`, `resolve_deferred_mixins`, `resolve_deferred_type_imports`, `resolve_deferred_value_of` (+ class-const refs), `flatten_phpdoc_inheritance`, `precompute_member_owners`, `precompute_ancestor_names` | the classes it names (parents, interfaces, traits, mixins, import sources, const owners) |
| `canonicalize_class_name_case` | the global class-name table |
| `resolve_constant_named_types` | for an all-caps PHPDoc name: whether it is a class anywhere / a global constant anywhere (`constant_named_type`, `registry/resolve.rs:1783`) |
| `drop_doc_members_excluded_by_native` | subtyping + `is_fully_known_class_type` (every ancestor indexed?) |
| `apply_assertion_table` | the built-in and the user's configured assertions - it writes assertion tags into stub entries, so a stub's result depends on project configuration |
| `resolve_transitive_purity` | stubs have no bodies (never classified); user bodies, resolved against the registry |
| `infer_global_variables` | `self.files`' top-level statements and bodies, resolved against the registry's functions and classes |

User code also reaches *into* stub entries: `insert_built_decl`
(`index.rs:103-372`) merges conditional polyfills and unscoped
redeclarations of built-in names into the stub `ClassReflection`. A
function redeclaration merges likewise when unconditional and replaces
the stub when it is a better conditional declaration; a global constant
never merges - a disagreeing value poisons it to `mixed`
(`register_global_constant`).

**Hazard for any "resolve stubs once" design**: merging onto a stub
resolved across files is not equivalent to today's merge-then-resolve for
the passes that read other classes (`flatten_traits`, PHPDoc inheritance,
mixins, type imports, `value_of`, member owners, ancestor names): a merge
can add parents, traits or members those passes would have used. The
per-file passes are already resolve-then-merge today - a stub's `@method`
is materialized in the cache before any polyfill merges into it. Merges
are not keep-existing either: `a.merge(b)` combines colliding methods
through `FunctionReflection::merge` (types unioned, `b`'s parameter
names) and lets `b` win properties and constants - and the call sites
pass different sides: a conditional polyfill merging into a stub is
`new_info.merge(existing)`, so the stub's properties and constants win;
an unscoped redeclaration of a built-in is `existing.merge(new_info)`,
so the redeclaration's win. Any class a user merges into must restart from
the stub's cached form.

`update_file`/`finish_loading` already rerun every pass over
already-resolved classes, but a rerun is not a recompute: several passes
only ever add, so what they derived from a declaration outlives an edit
to it (bug 5.10). `resolve_transitive_purity` never resets
`is_transitively_pure`; `flatten_phpdoc_inheritance` marks a backfilled
tag as the method's own (`has_own_doc`/`return_has_own_doc = true`,
`registry/resolve.rs:1169`/`1176`), so it is never refreshed;
`flatten_traits` and `resolve_deferred_mixins` insert members
vacant-only and never remove one (the limitation `remove_files`'s doc
comment records). Running them on a subset is a further step on top of
that.

### 4.6 Fixtures (per fixture, `checker/date_interval`, 44 fixtures × 3 reps)

| | load | ↳ `Project::new` | ↳ passes | checks | drop | total |
|---|---|---|---|---|---|---|
| debug | 1,296 ms | 243 ms | 985 ms | 19 ms | 76 ms | 1,395 ms |
| gate | 344 ms | 104 ms | 222 ms | 5 ms | 43 ms | 392 ms |

(`↳ passes` measured by rerunning `finish_loading` on the loaded project;
the remainder of `load` is config + discovery + parse, ~20-70 ms.)
Setup + teardown ≈ 93-98% of every fixture. Gate subset wall: 16.3 s
serial / 5.0 s at 8 threads (25 s user, 4-10 s sys).

Re-measured in gate at `880f1c75` (same 44 × 3, load 5-6): load 293 ms,
`Project::new` 108 ms, passes 179 ms, check + finalize 3.9 ms, drop 33 ms,
total 328 ms.

### 4.7 Real workspaces (gate, LSP path: `configure_from_workspace` + `load_workspace_files` + `finish_loading`)

Corpora are the vendored ones under `vendor/` (with their own composer
vendor dirs where present). The corpora are git submodules, and only the
main checkout has run `composer install` in them: guzzle, monolog,
laravel, drupal and symfony have a `vendor/` there (109 / 7,839 / 15,674
/ 7,313 / 7,581 PHP files) and none in a worktree, so the same rows
measured in a worktree describe a much smaller workspace.

| workspace | files | classes | load (parse) | passes (rerun) | `update_file` median (p90) | files rechecked median (p90) | max RSS |
|---|---|---|---|---|---|---|---|
| guzzle | 237 | 3,758 | 1.3 s | 170-365 ms | 2.0 s (2.4 s) | 100 (105) | 222 MB |
| wordpress | 1,899 | 4,341 | 8.9 s | 450-900 ms | 0.73 s (48.6 s) | 1 (1,524) | 673 MB |
| monolog | 7,878 | 9,158 | 35 s | 1.1-1.9 s | 1.2 s (1.3 s) | 2 (2) | 1.9 GB |
| phpstan-src | 8,409 | 13,932 | 23 s | 1.3 s | panicked (5.1) | | 851 MB |
| laravel | 15,486 | 17,635 | 50 s | 3.0-3.6 s | 3.3 s (108 s) | 3 (9,749) | 3.0 GB |
| drupal | 17,256 | 20,637 | 48 s | 2.9-4.3 s | 4.7 s (210 s) | 2 (14,216) | 2.3 GB |
| symfony | 19,287 | 22,373 | 55 s | 3.5-4.5 s | 5.1 s (6.1 s) | 3 (17) | 2.7 GB |

- **Re-measured at `880f1c75`** (gate, against the main checkout's
  corpora with their composer `vendor/`; 8 files evenly spaced over the
  sorted file list, tests included, 60 s cap; phpstan-src 12 files, 120 s
  cap; load 3-6):

  | workspace | load | passes | `update_file` median (p90) | max RSS |
  |---|---|---|---|---|
  | guzzle | 0.72 s | 223-312 ms | 1.5 s (1.6 s), 99-103 rechecked on `src/` edits | 243 MB |
  | wordpress | 5.6 s | 0.50-0.62 s | 0.77 s (47.5 s; 3 edits rechecked ~1,525) | 776 MB |
  | monolog | 18.8 s | 1.5-1.8 s | 1.3 s (14.3 s; one edit 4,359) | 1.86 GB |
  | phpstan-src | 18.3 s | 0.95-1.16 s | 0.9-1.4 s on 6 edits (1-2 rechecked); 6 capped at 120 s (2,592-4,668 rechecked by then) | 831 MB |
  | laravel | 30.7 s | 2.1-2.8 s | 3.4 s (60 s capped; ~9,750) | 3.0 GB |
  | drupal | 22.6 s | 2.2-2.9 s | 3.1 s (3.7 s) | 2.4 GB |
  | symfony | 38.4 s | 2.9-3.1 s | 3.0 s (60 s capped; 9,100) | 2.8 GB |

  RSS and recheck counts match the first run; loads are 20-55% shorter.
  The per-edit columns depend on which files are edited, and the first
  run's sample wasn't recorded. In phpstan-src every capped edit was a
  test-data file (`nsrt/array-column-php82.php`, `data/bug-8818.php`,
  ...).
- **Every edit pays the whole-registry passes** (0.17-4.5 s gate),
  growing with workspace size, so `update_file` medians run 0.73-5.1 s;
  in big workspaces user classes, not stubs, are most of the registry.
- **Dirty set per edit** (first run only; transitive reverse closure over
  extends/implements/uses of the classes the edited file declares; a
  lower bound - no mixins, constants or global-lookup passes): median 1,
  p90 3-5 in every workspace; max 552-4,197 (PHPUnit's `SelfDescribing`/
  `Reorderable`/`Test`, symfony polyfill `Stringable`,
  `PHPStanTestCase`).
- **Recheck blow-ups**: single edits recheck 1.5k-14k files (p90s of
  48-210 s in the table above; the first run's 330 s maximum isn't in
  it) -
  an over-approximation in dependents tracking, separate from the passes.
  `transitive_affected_files_from` closes over any reference (not just
  extends/implements/uses), and every edit - whitespace included - seeds
  it with the bare name of everything the file declares. `dependents` is
  keyed by each reference as written (`deps::referenced_names`: `User`,
  `\App\User`, `Models\User`), so a bare seed matches every file that
  spells some class `User` unqualified, in any namespace (the
  over-approximation) - and misses every qualified spelling (an
  under-approximation, bug 5.9). The phpstan-src test-data files show
  the over-approximation at its worst: `namespace Bug8818; class
  HelloWorld` seeds `HelloWorld`, which every other test-data file
  spelling `HelloWorld` unqualified matches, whatever its namespace -
  637 phpstan-src files declare a `class HelloWorld` of their own.
- **LSP load is sequential**: `load_workspace_files` (`lsp.rs:480-541`)
  calls `load_one_file` per file in a loop, while check-project's
  `Session::load` parses in parallel (`pool.install` + `par_iter`).
- Whole-workspace check (`check_all_analyzed_paths`): guzzle 0.45-0.9 s,
  wordpress 2.2-2.6 s (the noisier single runs showed up to 4.3 s). The
  seventh pass's runs add monolog 0.18 s (228 files), laravel 2.3 s
  (3,082), symfony 8.4 s (11,804) and drupal 58.7 s (10,306) - drupal
  costs ~8× symfony per file (5.7 against 0.71 ms), an outlier no one
  has looked into.

### 4.8 Memory

- Stub `LazyLock`: 103-116 MB per process.
- Each finished Project held: +30-40 MB (median ~36 MB) - 8 parallel
  tests ≈ 290 MB of duplicated stub registries.
- Re-measured at `880f1c75` (debug, RSS from `/proc/self/statm`): the
  first `Project::new` including the stub cache took RSS from 9 to 166 MB;
  holding 1→4 finished projects read 169 / 202 / 237 / 272 MB, ~35 MB
  each, consistent with the above.
- (A "raw clone" measurement came out +0.1 MB because glibc reused freed
  memory - not meaningful.)

### 4.9 Sys time: rayon idle spinning

`strace -c -f` on the variance probe (gate): `futex` 12,449 calls + 52.5%
of syscall time, `sched_yield` 34,324 calls + 45.1%, `mprotect` 4,902 +
2.1%. perf stat: 55.8k minor faults, 25.7k context switches, 20.1k CPU
migrations, 16.2 s user / 5.3 s sys. Minor faults are ~0 per stage once
warm (only `Project::new` faults, ~3k).

rayon-core 1.13 (`src/sleep/mod.rs:56-105`): an idle worker does 32
rounds of steal attempts with `thread::yield_now()` between them before
parking on a futex; hard-coded, no `ThreadPoolBuilder` knob. The passes
open many short parallel regions (12 `par_iter` sites in `registry/`:
11 in the passes - ancestor names, member owners ×4, purity ×2,
canonicalize, `flatten_traits`, method tags, PHPDoc inheritance - and the
stub parse; the `flatten_traits` and PHPDoc-inheritance ones run once per
fixed-point iteration). Each pushed job wakes at most one sleeping
worker, and only when the awake-but-idle ones can't take it
(`sleep/mod.rs:242-286`), so a large iteration wakes workers one by one
as it splits, up to all 8; each woken worker that runs out of work spins
its 32 rounds before sleeping again.
Constant-named types (the costliest pass in debug, second in gate),
drop-doc-members and `value_of` (38 + 23 + 16 of 219 ms gate) have no
parallel region and run sequentially.

Two pools are involved. The passes run on rayon's global pool (they're
called outside any `install`); checking runs on `project::worker_pool()`,
8 threads with 64 MB stacks, which every LSP `Server::new` builds afresh
(`lsp.rs:299`) - one per LSP test - and which the fixture harness builds
once beside the global pool. The per-session pool cost was not
measured.

`RAYON_NUM_THREADS=1`: same CPU per `finish_loading` (~560 ms) but 3×
wall (562 vs 183 ms) and sys 0.8 s vs 3.4 s - parallelism is a net wall
win for a single session; the spin is ~25% extra CPU that matters only
when the CPU is shared.

Not reproduced at `880f1c75` (gate, `finish_loading` rerun on a stubs-only
project, 9 reps, two interleaved rounds, load ~4): 8 threads 119-122 ms
wall at ~340 ms CPU per run (user 236-242, sys 100-105); 1 thread
182-190 ms wall at ~190 ms CPU (sys 6-7). Parallelism buys 1.55× wall,
not 3×, for ~1.8× the CPU, not the same CPU - so under libtest's
concurrency (4.2) the passes' parallelism may cost more than it saves,
which weighs on #3, #16 and #34. (Measured on a stubs-only registry;
the ratio on a real workspace's passes wasn't measured.) `strace -c -f` on that run: `futex`
9,877 calls / 56% of syscall time, `sched_yield` 18,254 / 41%,
`mprotect` 4,734 / 2% - with only 9 threads created, so the `mprotect`s
aren't thread stacks (glibc growing per-thread malloc arenas is the
likely source).

### 4.10 Build profiles (`cargo test -p phpstan-rs --lib`)

| profile | from clean | rebuild after a fn-body edit | load+finish | LSP session | fixture |
|---|---|---|---|---|---|
| debug | 2m49s | 13-15 s | 1,252 ms | 2,645 ms | 1,395 ms |
| opt1 (`opt-level=1`, incremental) | 5m28s | 11 s | 204 ms | 543 ms | 307 ms |
| deps2 (deps `opt-level=2`, workspace 0) | 6m33s | 11-13 s | 820 ms | 1,531 ms | 814 ms |
| gate | 6m09s (7m28s at `880f1c75`, load 6-9) | 228-294 s (87-128 s at `880f1c75`, two rebuilds after test-module edits - non-incremental, so any edit rebuilds the whole crate) | 183 ms | 431 ms | 392 ms |
| gateinc (gate + `incremental=true`) | 5m19s | 9 s | 165 ms | 507 ms | 291 ms |

Custom profiles were defined without editing `Cargo.toml`:
`--config 'profile.opt1.inherits="dev"' --config 'profile.opt1.opt-level=1' --profile opt1`
(likewise `deps2` with `profile.deps2.package."*".opt-level=2`,
`gateinc` inheriting `gate` with `incremental=true`). Builds ran under
`nice -n 19`; load after builds was 5-8, so from-clean times are
pessimistic.

Why `gate` has no incremental: Cargo defaults `incremental = false` for
`release`, and `gate` inherits it; nothing in the repo sets it. Release
defaults it off because many small incremental codegen units cost
optimization quality and the query cache costs disk/bookkeeping - but
`gate` exists to rebuild on every commit, so the default is arguably
wrong for it. (Nothing in `Cargo.toml`, `.cargo/config.toml` or CI sets
`incremental` or `CARGO_INCREMENTAL`.) Only the gate row and its two
re-timed builds were re-checked in review; the other profiles' figures
are the first run's.

### 4.11 Registry table implementation (`registry/name_table.rs`)

Three variants swapped into `NameTable` (built in gateinc, binaries saved,
then 3 interleaved rounds, each run started at load < 1.5):

- **overlay**: two populated `FxHashMap` layers (entries split by folded
  name-length parity), lookups probe overlay then base, iteration chains
  both - models a shared base plus a per-project overlay (#11).
- **imbl**: `imbl::GenericHashMap<Folded, V, FxBuildHasher>` - models a
  persistent map whose O(1) clone would make cloning the stub registry
  free (the alternative to #11 that was not pursued).
- **counting**: baseline + an atomic lookup counter.

Registry lookups during one whole-workspace check: wordpress 3.34 M,
guzzle 0.73 M (whole process incl. load: 9.1 M / 2.8 M).

| wordpress | check-all ms (r1/r2/r3) | passes ms (r1/r2/r3) |
|---|---|---|
| baseline | 2,466 / 2,501 / 2,306 | 430 / 398 / 409 |
| overlay | 2,476 / 2,314 / 2,238 | 431 / 412 / 442 |
| imbl | 2,259 / 2,413 / 2,527 | 508 / 443 / 479 |

guzzle check-all baseline spread 453-728 ms, too noisy to separate.
**Conclusion**: lookup speed is not decisive for checking; imbl costs
11-18% in the passes (per round against baseline: +18%, +11%, +17%); the
two-layer overlay's +0%, +4%, +8% is within the baseline's own spread
(398-430 ms, 8%). Not re-measured in review.
(Synthetic micro-benchmark at 3,495 keys, gate: FxHashMap 25 ns/lookup,
imbl 45-80 ns; imbl clone O(1).)

## 5. Bugs and correctness gaps found

1. **Panic (fixed, `11933e60`)**: `checker/function_decl.rs:120` before
   the fix, `self.diagnostics.split_off(start)` - "`at` split index (is 31) should
   be <= len (is 24)", first seen during `update_file` edits in the
   phpstan-src workspace. Not specific to edits: `check_trait_methods`
   deduplicated the whole diagnostics list, not just the trait's own
   range, so identical findings recorded before `start` shrank it below
   `start` (a panic) or shifted the range the constant-condition filter
   saw. Reproduced by a trait declared inside a method of a trait used by
   two hosts; now a fixture. After the fix, 12 edits in the phpstan-src
   workspace ran without a panic (4.7's re-measure).
2. **Declarations lost on edit or removal (fixed, `880f1c75`)**:
   `unindex_file` → `remove_declarations` removed the whole entry for
   each name the file declared. Confirmed: a polyfill merged into a
   built-in class lost the built-in's members on the first edit (all of
   conditional/unconditional × context/analyzed); a conditional polyfill
   a built-in function had beaten replaced it (`mb_str_pad`, 5 → 2
   params); of two context files declaring one class, editing one erased
   the other's members; removing a polyfill left no built-in. Each name a
   changed file declared (before or after) is now rebuilt from the stub
   and every remaining declaration, in load order. `didOpen` reaches
   this path on every open.
2a. **Edited files lost their `analyzed` flag (fixed, `a754b490`)**:
   `update_file` read the flag after `unindex_file` had removed the file,
   so every edit re-derived it from the path - an edited context file
   outside the default excluded directories became analyzed.
3. **Checker unit tests use a registry production never builds**
   (section 3, path 3).
4. **Recheck over-approximation**: single edits recheck up to 14k files
   (4.7).
5. **Global constants on edit (fixed, `e81eacc0`)**: unindexing never touches
   `global_constants`, and every edit re-registers the file's constants
   through `register_global_constant`, which poisons a disagreeing value
   to `mixed`. Confirmed: editing `const VERSION = 1;` to `= 2` makes
   `VERSION` `mixed`; a constant deleted from the file stays defined (a
   use of it reports nothing, where a never-defined one reports
   `constant.notFound`). A constant change also never reaches the files
   using it: `update_file` seeds the recheck only with declared functions
   and classes. Any incremental design (#12, #19) needs constants
   unindexed, rebuilt and tracked like functions and classes.
6. **A losing conditional polyfill is checked against the stub's
   signature (fixed, `4481ece6`; at load, not only on edit)**: when a conditional
   declaration loses to the built-in stub, no `function_decl_sites`
   entry exists for the name (`merge_stubs` records none), so
   `own_declaration_sig` (`checker/function_decl.rs`) treats the stub's
   slot as the declaration's own and checks the body against the stub's
   parameters. Confirmed: `if (!function_exists('mb_str_pad')) {
   function mb_str_pad($s, $l) { return $s . $l; } }` reports `undefined
   variable $s` and `$l`.
7. **Doctrine entity-table index never invalidated (fixed, `0a63e4b8`)**:
   `Registry::doctrine_entity_table_index` is a `OnceLock` filled on first
   use and never reset, so after an edit adds an entity or changes its
   `#[ORM\Table]` name the LSP keeps resolving SQL tables against the
   first-built map. (From reading; no repro.)
8. **Stale `function_decl_sites` (fixed, `4481ece6`)**: `remove_declarations`
   leaves a removed function's site entry. Rebuilding (5.2) overwrites
   it whenever a file declaration wins again, but a conditional
   declaration that won and, after an edit, loses to the stub at the same
   line and column keeps its old site, which `own_declaration_sig` then
   matches - 5.6's symptom, by a path a fix keyed on the missing entry
   would not cover. Today 5.6 masks it. (From reading; no repro.)
9. **Missed rechecks for qualified and PHPDoc-only references (fixed,
   `01911d9e`, `e5124a6e`)**: the under-approximation side of 4.7. A class edit reaches a
   dependent only when the dependent spells the class bare. Confirmed:
   with `class Base { function m() }` in one file, removing `m()`
   rechecks `use Base; ... (new Base())->m()` but not `(new \Base())->m()`
   (top level or in a namespace) nor `/** @param Base $b */ ... $b->m()`
   (PHPDoc references aren't collected at all); those keep their stale
   diagnostics until edited, though a fresh check reports
   `method.notFound`. Functions are affected the same way: changing
   `g(int)` to `g(string)` doesn't recheck a file calling `\g(1)`. A
   class named only in `@mixin` is a PHPDoc reference, so its users are
   never rechecked either.
10. **Edits leave derived facts stale (fixed, `bc861405`, `9567845b`)**: reruns are additive
   (4.5). Confirmed, each against a fresh project loading the edited
   text, with the dependent file rechecked in the first three cases:
   - purity: `f()` returning `g()` stays pure after `g()` gains an
     `echo`, so `function.resultUnused` on a bare `f();` persists;
   - PHPDoc inheritance: a child method keeps the `@return int` it
     inherited after the parent's tag becomes `@return string`, so two
     stale `return.type` errors persist;
   - traits: a method removed from a trait is still found on the class
     using it, so the fresh check's `method.notFound` is missing;
   - `value-of<E>`: after a case is added to `E`, a parameter typed
     `value-of<E>` still rejects the new value (`argument.type`), even
     when the dependent file is checked directly (it isn't rechecked on
     its own - a PHPDoc-only reference, 5.9).
   Not stale: an inherited property hook's virtual status follows the
   parent's edit (`propertySetHook.noAssign` appears as in a fresh
   project), and a class constant's type follows its edit. A
   `@phpstan-import-type` case was inconclusive. The fix is #10
   (recompute from raw forms); the differential test in section 8 would
   catch all of these.

## 6. Optimization catalog

*Implemented; section 12 maps each item to its commit. The text below is the
catalog as written beforehand.*

Estimates: "measured" = from section 4; "est." = extrapolated.

### Build and test infrastructure

| # | Optimization | Savings | Churn |
|---|---|---|---|
| 1 | `opt-level=1` test profile | lib unit tests 4.5-6× (measured: load+finish 6.1×, LSP session 4.9×, fixture 4.5×); rebuild 11 s vs 13-15 s; +2.5 min from clean. Not the fixture suite, which already runs in `gate` | trivial |
| 2 | `incremental = true` on `gate` | gate rebuild 228-294 s (87-128 s at `880f1c75`) → 9 s (measured) | trivial |
| 3 | One process-wide checking pool instead of one per LSP `Server` (and `install` the passes on it, so one pool serves both) | per-session thread creation (unmeasured) plus part of the passes' parallel overhead (~1.8× CPU for 1.55× wall, 4.9); est. 10-20% when the suite is CPU-bound | low |
| 4 | *Rejected*: nextest (process per test) | worse: +0.85 s gate / 3.3 s debug stub parse per test | - |

### Stub data

| # | Optimization | Savings | Churn |
|---|---|---|---|
| 5 | Load PHPStan-src stubs only when the project references `PHPStan\`/depends on phpstan | −56% stub classes; passes 219 → 78 ms gate (measured phpstorm-only); clone+merge −25%; ~half of stub memory (est.) | low-med (trigger must never be wrong). A trigger exists - `PackageContext::requires_phpstan()`, which already gates the PHPStan API check - but it sees only a direct `phpstan/phpstan` or `phpstan/extension-installer` requirement, missing a project that gets PHPStan through an extension package or runs without composer; a reference to `PHPStan\` in a loaded file is the safe trigger. And `Project::new` merges every stub before configuration is known (in `Session::load` and `Server::new` alike), so the PHPStan-src merge has to move after it |
| 6 | Build the resolved stub base at compile time; zero-copy load (rkyv/mmap) | −0.85 s wall / 5 s CPU gate, −3.3 s / 18 s CPU debug per process (measured); −103 MB heap with zero-copy | high (serde on reflection types, re-interning) |
| 7 | Lazy stub materialization (only referenced classes) | est. 80-95% of remaining stub cost | very high (every "all classes" iteration needs an index) |
| 8 | Load only enabled phpstorm extensions | up to 76% of phpstorm classes: by the stub index's extension column, 1,178 of 1,545 come from extensions php-src doesn't bundle (couchbase 209, cassandra 100, mongodb 81, yaf 76, swoole 60, ...), 367 from bundled ones | medium; "enabled" needs a source (composer `ext-*` requirements, config), and code that uses an undeclared extension would lose its classes |
| 9 | Faster stub indexing (41k `docblock::parse`, 31k `from_decl`) | est. 20-40% of the one-time cost; moot after 6 | medium |

### Registry construction

| # | Optimization | Savings | Churn |
|---|---|---|---|
| 10 | Raw vs resolved reflection separation | none alone; prerequisite for 11/12; fixes bug 5.10 and the baked-in-members limitation (5.2 is fixed separately) | high |
| 11 | Shared immutable resolved base + per-project overlay | `Project::new` 61-73 ms → est. ~1 ms, drop 15-26 ms → est. ~0 gate (up to 238 + 86 ms debug); with the base resolved once per process, the stub passes (200 ms gate / ~1 s debug per Project) go too; 36 MB → few MB per Project; no lookup cost (measured, 4.11) | high (316 direct `.classes`/`.functions` accesses: 155 outside `registry/`, 161 inside; plus the registry-wide derived tables - five member-owner maps, `ancestor_names` - and `global_constants`, `global_variables`, the declared-name sets) |
| 12 | Scoped incremental resolution + reverse-dependency index | real-workspace `update_file` 0.7-5 s → dirty set median 1 / p90 3-5 (measured) → est. ms. (The per-Project stub-pass saving needs #11's shared base; #12 alone still resolves every stub once per Project.) | very high (16 passes) |
| 13 | Merge the two member-owner rebuilds, fuse whole-registry sweeps | est. 15-25% of passes (member owners ×2 = 41 of 219 ms) | medium (the second rebuild exists because `resolve_deferred_mixins` adds members after PHPDoc inheritance consumed the first; merging needs mixin members folded into the tables incrementally) |
| 14 | Lazy per-class member-owner / ancestor tables (`OnceLock`) | up to ~48 of 219 ms eager precompute (22%) - only the share for classes nothing looks up | medium (the passes change classes between rounds, so each round must reset the cells) |
| 15 | Per-pass candidate flags set at index time (purity skips bodiless stubs, constant-named types, doc members vs native, deferred types) | est. 40-60% of the stub-only passes even without 12; real-workspace share unknown (no per-pass breakdown there, section 9) | medium |
| 16 | Coarser rayon regions / sequential below a threshold | removes the parallel overhead (first run: ~25% extra CPU at 3× wall; re-measured: ~1.8× CPU at 1.55× wall, 4.9) where it doesn't pay | low |
| 34 | Parallelize the sequential passes (constant-named types, drop-doc-members, `value_of`; 4.9) | 77 of 219 ms (35%) of the gate stub pass time is sequential today; the wall gain comes at extra CPU (4.9), so it pays for a single session, less under test concurrency | low for the first two (already a read phase then an apply phase); medium for `value_of` (removes and re-inserts each class while reading the rest) |

### LSP

| # | Optimization | Savings | Churn |
|---|---|---|---|
| 17 | Parallel workspace load (reuse `Session::load`'s parallel parse, sequential insert) | laravel 50 s, symfony 55 s, drupal 48 s, monolog 35 s (measured; 30.7 / 38.4 / 22.6 / 18.8 s re-measured) → est. ~8-10 s. The LSP retains source, so `insert_parsed_file` runs use-site indexing (`index_class_fn_refs`) per file in the sequential insert; reaching the estimate likely means moving it into the parallel parse (#21) | low |
| 18 | Skip `update_file` on `didOpen` when text == loaded source (the LSP retains source, so the comparison is free) | one full recompute per opened file: 0.15 s small → 1-5 s large (measured). The opened file's diagnostics still have to be published - from `last_diagnostics` or one `check_file` | trivial |
| 19 | Skip the declaration-driven passes when an edit leaves the file's built declarations (signatures, docblocks, not just names) unchanged | most keystrokes: 0.7-5 s → parse + check + the body-reading passes. `resolve_transitive_purity`, `resolve_property_writes` and `infer_global_variables` read bodies and top-level statements, so they still need a rerun scoped to the file and what depends on it; the end cost depends on their real-workspace share (unmeasured) | medium (cheaper first step toward 12) |
| 20 | Precise dependents for rechecks: resolved FQN keys (with `use` imports and namespace applied, PHPDoc and constant references collected), seeds only for declarations that changed, closure over structural edges rather than every reference (4.7) | p90 edits 48 / 108 / 210 s → est. seconds; also fixes 5.9 | medium |
| 21 | Cost of `retain_source` + use-site indexing in the LSP | unmeasured here; part of the 19-55 s loads. The code records that running this indexing for every loaded file "roughly doubled `check-project`'s own wall time" on a ~14k-file corpus (`project.rs:696-705`) - and the LSP runs it for every file | low to measure |

### Test suite structure

| # | Optimization | Savings | Churn |
|---|---|---|---|
| 22 | Checker unit tests on the production construction path | correctness; cost-neutral after 11 | low |
| 23 | Move Project teardown off the critical path in harnesses: hand it to a background drop thread | 43 ms gate / 76 ms debug of wall per fixture (~11% gate), not CPU; moot after 11. Not "drop at exit": at ~36 MB per finished Project (4.8) that would hold ~134 GB over the 3,805 fixtures and ~8 GB over the 220 LSP tests | low |
| 24 | Arena allocation per Project | drop → free; est. 10-20% faster allocation | very high |
| 25 | *Rejected*: share a Project across fixtures | fixtures reuse class names | - |

### Correctness items

26 = bug 5.1 (panic) - fixed; 27 = bug 5.2 - fixed by rebuilding the
changed names from their remaining declarations, independently of 10.
28 = bug 5.5 (constants; prerequisite for any incremental design);
29 = bug 5.6 (losing polyfill); 30 = bug 5.7 (Doctrine index; folds into
11/12, where every derived table needs invalidation); 31 = bug 5.8;
32 = bug 5.9 (missed rechecks; the correctness half of #20); 33 = bug
5.10 (stale derived facts; needs #10).

### Combined projections (extrapolated)

| workload | today | + #1 | + #5, #11, #12 (+ #6) |
|---|---|---|---|
| fixture suite, 8 cores | ~7 min (gate, as run); 27 min if run in debug | ~5.6 min (opt1 307 vs gate 392 ms per fixture) - the gate win is #2's rebuild, 228-294 s → 9 s | est. ~0.5-1 min (per fixture: parse + check + resolving its own classes ≈ 30-60 ms; 14-29 s at perfect scaling, 33-66 s with 4.6's measured contention) |
| LSP test session | 2.6 s (debug) | 0.5 s | est. ~50 ms (with #18) |
| real LSP edit, symfony | 5 s | - | #19: est. tens of ms for body edits; #12 for signature edits; #20 for outliers |
| LSP startup, laravel | ~50 s (30.7 s re-measured) | - | #17: est. ~10 s |
| CLI startup | - | - | #6: −0.85 s |

### Suggested order

1. Quick wins: 1, 2, 18, 17, 16, 34, 3. #16 and #34 pull opposite ways
   (4.9): parallelism pays for a single LSP session and costs under
   test concurrency, so they need one policy - e.g. run the passes on
   the calling thread inside the test harnesses.
2. Correctness: 32, 28, 29, 31, 30; 33 with 10.
3. 10.
4. 5.
5. 19.
6. 11.
7. 12, with 13/14/15 folded into the scoped passes.
8. 20.
9. 6.
10. 7 last (pays off only after 11/12; largest blast radius).

Not placed: #8 (needs an "enabled extensions" source first), #9 (moot
after #6), #21 (a measurement; do it with #17), #22 (with #11), #23
(moot after #11), #24 (very high churn; only if allocation shows up
after #11/#12).

## 7. Rayon idle spinning: options

1. Fewer, larger parallel regions: fuse per-class passes into one
   parallel sweep; run fixed-point loops inside one region instead of one
   region per iteration (same restructuring as #13/#15). Each iteration
   of `flatten_traits` and PHPDoc inheritance is a parallel read phase
   then a sequential apply, so one region per loop still needs a barrier
   per iteration (e.g. a `rayon::scope` per round, or a double-buffered
   apply).
2. Sequential below a size threshold (`with_min_len`, chunking, explicit
   cutoffs). After #12 most per-edit work never enters rayon;
   parallelism remains for bulk load in a few large regions.
3. `std::thread::scope` with one chunk per core where work-stealing
   isn't needed: no spinning (threads exit), µs spawn cost; load
   imbalance with skewed classes.
4. Patch rayon-core's `ROUNDS_UNTIL_SLEEPY` via `[patch.crates-io]` as a
   one-off experiment to measure the recoverable share; unattractive as a
   permanent fork.
5. Alternative schedulers (e.g. heartbeat-based fork-join such as
   `chili`) - not evaluated.

Recommendation: 1 + 2, which fall out of the pass restructuring anyway;
4 only to measure the ceiling. The seventh pass's numbers (4.9: 1.55×
wall for ~1.8× CPU) strengthen 2: when tests already run eight at a
time, running the passes on the calling thread likely beats parallel
regions outright.

## 8. Design notes for the core fix (10 + 11 + 12 + 5)

- **Raw vs resolved**: keep each class/function's raw form (as indexing
  produces it) apart from its resolved form; resolved = f(raw, resolved
  forms it depends on). Dirty classes always recompute from raw, which
  removes the merge-order hazard (4.5). For stubs, "raw" is the cached
  per-file-resolved form (section 3). Bug 5.2 is already handled by
  rebuilding changed names from the stub plus their remaining
  declarations; separating raw from resolved additionally fixes the
  members other classes baked in from a changed class (the limitation
  `remove_files` documents).
- **Reverse-dependency index**: structural edges (extends, implements,
  uses, mixins, type-import sources, const owners) plus, for the
  global-lookup passes, name → signatures mentioning it. A change marks
  classes dirty, closes over the index, resets to raw, reruns passes on
  that set. Fixed-point passes (`flatten_traits`,
  `flatten_phpdoc_inheritance`, `inherit_property_hooks`, class-const
  refs, purity, property writes - every `loop`/`max_iterations` in the
  passes) need their termination argued within a scope; derived
  tables (`ancestor_names`, the five member-owner tables) need per-class
  updates.
- **Shared base**: resolved phpstorm base (closed, 4.3) as an immutable
  `Arc` layer; per-project overlay shadows it (overlay lookups
  measured free, 4.11). Invalidation triggers for the base: user merges
  into stub names, user classes colliding with the 6 bogus type names,
  user global constants that a stub's PHPDoc names as an all-caps type
  (constant-named types; none of the 6 bogus names is all-caps), and
  configured assertions that target a stub function or
  method (`apply_assertion_table` writes them into the stub entry).
  Passes read no PHP-version setting, so versions are not a trigger. Lazy
  registry caches (`doctrine_entity_table_index`, 5.7) need the same
  per-overlay treatment as the derived tables.
- **PHPStan-src stubs**: either cache them too with invalidation when a
  project supplies their external parents (PhpParser, Nette, Symfony
  Console, PHPUnit, BetterReflection, phpdoc-parser - 4.3), or load them only for projects
  that reference `PHPStan\` (#5). Leaving them per-project loses ~60%
  of the stub pass saving (4.4).
- **Proof obligation**: a differential test running full recompute vs
  scoped recompute and asserting identical registries, over every fixture
  and over random edit sequences (proptest is already a dev-dependency).
  Today's `update_file` already fails it - an edit followed by a fresh
  load of the same text gives different registries (5.10) - so the test
  is worth adding before any of this, against the current full rerun.

## 9. Open questions / not measured

- How much of the stub set a typical project actually references
  (decides #7's payoff).
- Which extensions to treat as enabled for #8 (its share is now
  counted: 76% of phpstorm classes are from non-bundled extensions).
- #21 (source retention + use-site index cost in LSP loads).
- Whether the global-lookup passes' reverse index (name → signatures)
  stays small enough to keep dirty sets tiny.
- Dirty-set sizes including mixins and constants (4.7 is a lower bound).
- `chili`/other schedulers (7.5).
- Per-pass timings in real workspaces: section 4.1's split is stub-only
  registries, where the body-reading passes (purity classification,
  property writes, global-variable inference) cost almost nothing; they
  decide what #15 and #19 can save on user code.
- The cost of building a fresh 8-thread worker pool per LSP session
  (4.9).
- Why drupal's whole-workspace check costs ~8× symfony's per file (4.7).
- Whether the passes' parallelism pays on a real workspace, given
  1.55× wall for ~1.8× CPU on stubs only (4.9).

## 10. Reproducing the measurements

The probes were deleted; recreate them as `#[test] #[ignore]` modules
registered from `project.rs` (`#[cfg(test)] mod …;`) and
`lsp/tests.rs`, run with
`<test-binary> --ignored --exact <path> -q`, results written to a file
(`PROBE_DIR`) - never piped straight to grep. The lib test binary is
`target/<profile>/build/phpstan-rs/<hash>/out/phpstan_rs-<hash>` (the
path `cargo test --no-run` prints).

- **Per-pass timing**: copy `finish_loading`'s body, wrapping each call
  in `Instant` (child module of `project` can reach
  `p.registry`/`p.files`/`p.rebuild_pest_config()`).
- **Stub split**: filter `generated_stub_classes()`/`generated_stubs()` by
  `PHPStan\` prefix (cross-check `provenance().origin()` for
  `Origin::Phpstan`), build `Registry::default()` + `merge_*`, swap into
  `Project::new().registry`, time the passes.
- **Fixture stages**: `gate::fixture_config` → `gate::load_fixture` →
  `session.load()` → `session.analyse(loaded, false)` →
  `analysis.finalize(&session.finalize_options())`; passes measured by
  rerunning `loaded.project.finish_loading()`, `Project::new()` timed
  separately.
- **Workspace scaling**: in `lsp::tests`, `Server::new` →
  `configure_from_workspace(root)` → `load_workspace_files(root, &ctx)` →
  `project.finish_loading()` (first + reruns) →
  `check_all_analyzed_paths()`; dirty sets via
  `registry.all_ancestor_names` inverted + `project::declared_names(ast)`;
  edits via `project.update_file(path, text + "\n")` on files outside the
  workspace's own `vendor/` (filter relative to the root - the corpora
  themselves live under `phpstan-rs/vendor/`). Point it at the main
  checkout's corpora: only there have they had `composer install` (4.7).
- **Contention**: N `helpers::run` sessions in `std::thread::scope`.
- **CPU / faults / RSS**: `/proc/self/stat` fields utime+stime (×10 ms)
  and minflt; `/proc/self/statm` resident pages.
- **Syscalls**: `strace -c -f -o out <binary> …`; `perf stat -e
  task-clock,page-faults,context-switches,cpu-migrations`.
- **NameTable variants**: replace `registry/name_table.rs` wholesale
  (keep its public API; `unreachable_pub = "deny"` rejects a `pub static`
  counter - use a private static + `pub(crate) fn`), build in a
  fast-rebuild profile (gateinc), copy each test binary aside, then run
  the variants interleaved round-robin with a load gate.
- Heavy jobs: one at a time, builds under `nice -n 19`, log
  `/proc/loadavg`, gate timed runs on 1-min load < 1.5.

## 11. Review log

A first review pass checked the document against the code the same day
and fixed bugs 5.1 (`11933e60`), 5.2a (`a754b490`, found while fixing
5.2) and 5.2 (`880f1c75`). A second review pass
(same day, at `880f1c75`) re-checked the line references, reproduced the
stub counts (4.3, 4.4) and the debug per-pass ranking (4.1), and found
bugs 5.5-5.8 (since fixed, section 12). A third pass re-checked the second's own claims,
corrected three of them (section 3's "every pass", 4.7's keying, 5.8's
effect) and found bug 5.9 (since fixed). A fourth pass tested section 4.5's
"the passes are re-runnable" against edits, found it false (bug 5.10,
since fixed), widened 5.9 to functions and `@mixin`, and re-measured 4.8. Fifth
and sixth passes extended 5.10 to `value-of`, settled the property-hook
case, re-counted 4.3's type references, mapped the `par_iter` sites, and
re-ran in gate the 4.2 contention, 4.6 stage, 4.7 guzzle and phpstan-src
measurements (phpstan-src's edits, blocked by 5.1 before, now run). A
seventh pass found the corpora's composer `vendor/` dirs exist only in
the main checkout (which changes 4.7's comparisons), re-ran the 4.7 rows
with them, and contradicted 4.9's single-thread result. An eighth pass
(reading only) checked the catalog's feasibility: #23 as written can't
work, #5 has a trigger but needs the stub merge moved after
configuration, and two projection figures were corrected.
A ninth pass re-checked the document claim by claim, in order, mostly
by reading: it moved this log out of the header, fixed the header's
commit mapping, the fixture-time arithmetic (section 2), 3.2's
`didChange` path, 4.2's tick/debounce claim, 4.5's merge direction and
constant handling, 4.7's mixed-run phpstan-src row and ×8 drupal ratio,
4.11's percentages, 5.10's "every case", and catalog rows #1, #3, #11,
#12, #13, #16, #17, #18, #21; confirmed phpstorm's closure for every
edge kind; and replaced the undefined design labels D1/D2/C. Asked
whether it had really covered every claim, it hadn't; it then re-ran in
order with a per-claim ledger (each claim confirmed, corrected, or left
unverified with the reason), which added: the resume point's dangling
reference, 4.1's gate CPU conflict, 4.3's "typos", 4.7's 330 s, 4.9's
wake-up model, #8's share (76%, answered from the stub index), #14's
real saving, 7.1's per-round barrier, and section 8's missing
fixed-point pass, vacuous constants trigger and ~60%. Asked again, the
ledger still had lumped rows; splitting them added: the fixture
projection (33-66 s with contention, not "< 1 min"), #21's in-code
evidence, the suggested order's unplaced items, and a per-row
comparison of 4.7's first-run table (laravel's file count is 21% off,
unexplained).

## 12. Implementation

Each item one commit (check, clippy and the item's own tests written,
not run - validation was left to the end of the plan):

| item | commit | notes |
|---|---|---|
| #1, #2 | `4a9e90ea` | `[profile.test] opt-level = 1`; `gate` incremental |
| #18 | `29b136d9` | publishes `last_diagnostics`, or checks the file once |
| #17, #21 | `83a5d790` | `Project::load_files`: read + parse + use-site indexing in parallel (also in `Session::load`); #21's indexing cost moved into the parallel stage rather than measured |
| #3 | `662130f5` | `worker_pool()` is one `LazyLock` pool; `finish_loading` installs on it; `update_file` inserts the file and calls `finish_loading` |
| #34 | `22466123` | constant-named types and drop-doc-members read in parallel; `value_of` left sequential (its remove/reinsert reads earlier classes' results, so a parallel version could change them) |
| 5.9 (#32) | `01911d9e` | `deps::referenced_names` keys are `dependency_key`s (resolved, lowercased FQNs); PHPDoc tag names collected |
| 5.5 (#28) | `e81eacc0` | constants tracked per file, rebuilt like functions/classes, seeding rechecks |
| 5.6, 5.8 (#29, #31) | `4481ece6` | no site entry = not the declaration's own slot; sites removed with declarations; the slot's file must match |
| 5.7 (#30) | `0a63e4b8` | `reset_lazy_tables` at every finish |
| #10, 5.10 (#33) | `bc861405`, `9567845b` | raw declarations kept apart from resolved ones; every finish starts from raw |
| #5 | `5f4c4311` | PHPStan's source merged while any loaded file mentions `PHPStan\`; its names rebuilt stubs-first when it comes or goes |
| #12 (+ #19) | `bec3073d`, `7e3324ac` | `registry::scope::Scope`; changes recorded by the registry's mutations; scope = changed names + transitive referencers (loaded files and stub files); scope entries reset from raw, passes run over the scope. #19 folded in: spans shift with any edit above a declaration, so "unchanged declarations" needs the same scoped recompute |
| #11 | `19b92570` | `NameTable` over a shared `Arc` base (copy on write); `Project::new` clones a per-process resolved base; configured assertions record their targets as changed |
| #13 | `fcc258fe` | second member-owner rebuild only for scope classes with a mixin class in their hierarchy |
| #14, #15 | - | folded: with the shared base the eager stub work is per process, and scoping restricts it to the edit's entries |
| #16 | `d7335938` | `with_min_len(PASS_CHUNK = 32)` on every pass region |
| #20 | `ae84f0e0` | seeds = entries whose resolved form changed (`Registry::changed_since`, plus property-write verdicts and constants); closure over declaration-level references (`deps::declaration_referenced_names`) |
| #6 | `d1fe66ca` | stub data and resolved base stored as CBOR under `~/.cache/phpstan-rs`, keyed by a build-script hash of the sources and stubs (`PHPSTAN_RS_NO_STUB_CACHE`, `PHPSTAN_RS_CACHE_DIR`) |
| #7 | `8650bb37`, `2a5548d1` | reverse indexes (subtypes, trait users) replace the checker's whole-registry scans; stored entries decode on first read |
| #22 | `bf205f12` | checker and rule unit-test helpers build through `Project` |
| #8 | - | moot after #11: an unreferenced extension's classes cost nothing per project, and dropping them would report `class.notFound` for undeclared extensions |
| #9, #23 | - | moot after #6 / #11 |
| #24 | - | not done: only if allocation shows up after #11/#12 (unmeasured) |

Gap work after the first pass (see `BUILD_COST_LEDGER.md`):

| item | commit | notes |
|---|---|---|
| G11 | `3d77ee38` | `global $x;` is a tracked reference (`deps::global_variable_key`); a changed inferred global type seeds a recheck |
| G12, G13 | `c4eacee4`, `9ad44af6` | cache checksum; a cancelled recheck drops the unchecked files' diagnostics |
| G1 (#34) | `862d168a` | `value-of` resolves per class in parallel against the pre-pass classes |
| G2, G14 (#19) | `feec6a57` | two scopes: declaration references for resolution, all references for purity, property writes and global variables; property-write classifications and per-file global-variable sites kept between finishes |
| G21 | `stub_cache::prune` | the cache keeps the last 3 builds |
| G16 | `e35088b3`, `76b016b6` | differential test: sampled, random (proptest) and every-fixture (`#[ignore]`) variants |
| G10 (#5/#11) | `c395b928` | a second shared base with PHPStan's source; a project naming `PHPStan\` before its first finish switches to it and replays its declarations |
| G8 (#24) | `433e07f2` | mimalloc as the global allocator in place of per-project arenas (the std containers can't take an arena on stable) |
| G6 (#21) | `54ec427b` | an editor workspace indexes class/function use sites at the first references/rename request |
| G9 | `e2523b66` | `PHPSTAN_RS_THREADS` sizes the worker pool |
| G4 (#8) | - | not done on purpose: loading only enabled extensions would make code using an undeclared extension report `class.notFound`; unreferenced extension classes cost nothing per project since #6/#7/#11 |

Measured at the end (merge-base `5d86aac1` against HEAD, `gate` binaries; the
machine carried other jobs, load 1-4, both sides alike):

| workload | merge-base | HEAD |
|---|---|---|
| one-file project, `check-project` | 743 ms, 3.1 s CPU | 65 ms, 47 ms CPU |
| one test process (lib tests, `test` profile) | 600 ms, 3.2 s CPU | 180 ms, 0.14 s CPU |
| lib suite (952 tests) | 33-42 s | 7 s |
| fixture suite (3,807) | 203 s | 6.6 s |
| every-fixture edit differential | 226 s | 23 s |
| guzzle (70 files): full load / no-change edit | 672 ms / 185 ms (6 files rechecked) | 150 ms / 62 ms (1 file) |
| symfony Console (378 files): load / edit | 950 ms / 620 ms (193 files) | 350 ms / 130 ms (1 file) |
| symfony Routing (267 files): load / edit | 870 ms / 400 ms (120 files) | 190 ms / 70 ms (1 file) |
| Piwigo scoped corpus, CLI | 9.2-9.6 s, 25-27 s CPU | 8.2-8.5 s, 22 s CPU |
| mimalloc against the system allocator (Piwigo scoped, HEAD) | 14.1 s | 10.5 s |

What the measurements found, beyond the planned items:

- The stub cache (#6) never worked until `fb9b3137`'s predecessor: ciborium's
  byte reader fails on entries over 4 KB, so every run rebuilt and rewrote
  90 MB. A stored file that passes its checksum but doesn't decode now says so.
- The stubs' index was read with all of the stub data (90 MB) for the one
  lookup it serves; it is a file of its own, 1.2 MB.
- The stored registry wrote its raw declarations beside the resolved ones,
  94% of classes and every function identical: 41 MB became 23 MB (85 MB
  with PHPStan's source became 57 MB).
- The dependency collector took every word of a PHPDoc tag line, descriptions
  and template parameters included, as a class name: a project function
  called `a` made 5,757 stub declarations dependents. It now reads the types of
  a tag only.
- The closed-class check decoded all stub classes to answer for the
  project's own, two thirds of a one-file run.
- Switching to the registry with PHPStan's source after the declarations
  were inserted rebuilt them one after the other (two seconds on 14k files);
  the choice is now made before the first insertion.
- The CLI no longer frees the project it is about to leave.

The resolution scope (#12) now closes over declaration-level references only;
purity, property writes and global variables rerun over the wider scope of
every reference, bodies included, as before.

Validation (end of plan) found and fixed:

- `906d486d`: #20's before-snapshot read the tables while the raw
  declarations were swapped in, so no edit seeded a recheck; four tests
  asserted superseded behaviour (the trait-member limitation #10
  removed, a PHPStan class replacing its stub outright - it merges, as
  before #5 - PHPStan's classes without a file naming them, a cascade
  from an edit changing no signature).
- `b45b0d06`: two watched-file tests expected a republish after an event
  that re-read content the scan had already loaded.
- `e5124a6e`: `use` import paths weren't references, so a file naming
  `PHPStan\` only through `use function` never brought in PHPStan's
  source (2 fixtures), and importers weren't rechecked.
- The 5.6 fixture's `.expect` needed `no diagnostics`.

The stub cache (#6) writes `~/.cache/phpstan-rs/stub-{data,base}-<hash>.cbor`.
