# Callgrind drill-down of the off run: no stub cache, no per-file cache

Profile: `~/.cache/prs-profile/callgrind-20g-modes/off/` (`cg.out-01..17`, 17 threads), **25,625,190,065 Ir**.
- **Command.** `check-project --config phpstan-rs.toml --format json`, run in `/home/torres/piwigo17-frozen` with `PHPSTAN_RS_NO_FILE_CACHE=1 PHPSTAN_RS_NO_STUB_CACHE=1`.
- **Binary.** `callgrind-20g-modes/fp/dhat/phpstan-rs`: the `dhat` profile with frame pointers, built at `d520c9fd` on branch `callgrind-20g`. Its code is that of `5fb858c9`, because the commits between them change only documentation.
- **Machine.** 8 cores.

**Sources.**
- **The profile.** The drill itself built and ran nothing. Its figures come from the callgrind outputs of three runs of that binary (off, cold and warm, §1) and from the code at `d520c9fd`. The token counts of §5 were taken with PHP's own tokenizer (`token_get_all`) over Piwigo's files, and the cache sizes of §10.6 with `zstd` and `gzip` over the cold run's cache files.
- **The investigations.** What the drill could not settle went to a ledger of items (`PROFILE_DRILL_PIWIGO_OFF_LEDGER.md`, retired; its last version is `git show 72cac936:PROFILE_DRILL_PIWIGO_OFF_LEDGER.md`), and every item there was worked out. Its investigations made probe builds (B1-B3, B7, B10), interleaved timings (B4), a fourth callgrind run with both caches warm (B5), strace and perf runs (B6) and builds of the crate graph (B9), and read the code, php-src and the crate split's plan (C1-C8, D6). This report carries their results, and §13 gives each item's method and data path. The ledger's full reasoning is in git history.
- **Checked.** Every item was checked again on 2026-10-14 against the code, the profiles, the data on disk and fresh runs of the scripts (`~/.cache/prs-probe/validate/notes.md`). The figures here are the corrected ones.

**Conventions.**
- "M" is 10^6 Ir and "G" is 10^9 Ir.
- Percentages are of the off run's 25.63 G unless marked.
- "Item n" is an item of the 20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`, Part II), and "proposal n" is one of this report's (§10).
- Dn, Bn, Cn, En and Nn are the ids of the retired ledger's items (decisions, measurements that needed a build, code and data investigations, cuts and defects found, costs no item reached); §13 lists them with their methods.

## Summary

- **The stub build is the whole difference.** The off run costs 5.05 G more than the warm run. The stub build is 5.10 G of it, and the rest of the run is 0.05 G cheaper (§4):
  - 3.50 G parses and indexes the 2,515 stub files on a second rayon pool;
  - 1.60 G runs serially on the main thread while every other core is idle: the stub corpus and the plain base (≈0.98 G, inclusive) before the parse, and PHPStan's base (≈0.65 G, inclusive) at `expect_files`, after it.

  In wall time it is 0.37 s of the off run's 2.68 s (B4, §3.3). The cold run is the off run plus the cache write: 58 M, ≈10 ms of disk (B6). The stub cache is keyed by the build's source hash, so the first run of every build is cold.
- **The rest matches the warm run.** The other 20.57 G is the warm run, which the 20 G report drilled. The off and warm runs agree phase by phase, apart from the stub build, within 0.12 G, and their diagnostics are byte-identical.
- **With both caches warm the run is 14.34 G and 1.60 s** (B5, §3.4). The finishing passes are then ≈40% of the wall time against 18% of the instructions: they run at 1-3 cores in every mode.
- **The 20 G plan reaches nearly every block of the shared work.** It covers each block at least in part, except §5's `read` (81.9 M, which only proposal 10 reaches), `anonymous class names` (5.1 M, which no item reaches), the unattributed `rest` rows, and the off run's name-table lookups (which only proposal 3 reaches). Its items estimate ≈8 G together. What no item removes is smaller than the drill first said (§6, §9.6):
  - the check rules' own logic is ≈0.46 G over 1,181 functions, none above 8.4 M; the items reach ≈0.75 G of the ≈1.2 G first counted as the rules' (N1);
  - the type relation and the interner are lean per call; their cost is in the counts (N2);
  - the analysed files' declarations cost ≈0.5-0.6 G on a run with the per-file cache warm, which E4 removes (N3).
- **Library bodies are 48% of the parsing.** That share of what the parse job lexes and parses is function bodies in context files, and their trees are dropped once their summaries are taken (§5). A run reads ≈0.5-0.6% of those bodies (B1: 0.63% on Piwigo, 0.50% on Laravel).
- **Eleven proposals**, each checked against the code and the data, and three ideas rejected (§10). The "runs" column says which runs gain:

| # | proposal | saving | runs | needs |
|---|---|---|---|---|
| 1 | Resolve only the stub base the files need | −355.1 M serial, and the plain base's finishing run, ≈35-52 M, estimated (C5) | off and cold, when a loaded file mentions PHPStan | E1's fix |
| 2 | Serialize only what a store keeps | −1,005.3 M serial; ≈ −0.71 to −0.74 G after 1 (C3) | off | D5's write check |
| 3 | A hash index for stub bases built in memory | ≈ −0.29 G | off and cold | — |
| 4 | Look each stub file's dependents up once in `build_closures` | ≥ −0.2 G serial | off and cold | — |
| 5 | Build the stub corpus beside the parse | wall time: the corpus's share of the ≈0.25 s serial stretch before the parse (≈0.62 of 0.98 G, about 0.14 s at 7/8 of it: a model, not a measurement); with proposal 1, which removes the plain base's share, the whole stretch (B4) | off and cold | E1's fix |
| 6 | Build the stub data at compile time, into every binary | −5.05 G today; ≈0.2-1.5 G per cold run once the crate split's C4 and C5 land (C8) | off and cold | the crate split's reflection crate; not built in today's form (§10.6) |
| 7 | Tokenize a PHPDoc tag's description only when it is read | ≈ −0.37 G off, ≈ −0.08 G warm, from the measured byte shares (B2); ≈ −0.04 G after proposal 6 and item 14 | all | — |
| 8 | Skim the function bodies of context files, and parse one when something reads it | after the plan, ≈ −2.0 to −2.3 G once a file's parse verdict is cached; ≈ −0.55 to −0.85 G on a run that must check the bodies (B1: d ≈ 0.02 by tokens, of which 0.005-0.006 of the bodies are read) | per-file cache cold or off | proposal 9; a parse-verdict cache |
| 9 | Purity and property writes on demand | ≈ −0.3 to −0.4 G beyond item 4, on the finishing thread; each search visits 0.3-4.3% of the declarations (B1) | all | item 4's ids |
| 10 | Check UTF-8 with SIMD | ≈ −22 M (estimated) | all | — |
| 11 | Make text only when it is read: a reported message when it is displayed, a constant key only when a dynamic constant is configured | −29.1 M | all | — |

- **Eleven more cuts** came out of the investigations (§10.13). The largest apply to a run with the per-file cache warm: E12 (≈ −0.35-0.4 G on the serial insert), E4 (≈ −0.25-0.3 G), E11 (≈ −116 M). For the shared work: E7 (≈ −55 M), E2 (≈ −50 M), E3 and E10 (≈ −35 M each), E5 (≈ −30 M), E6 (≈ −10 M), and E8 on runs that read the stub cache (≈ −65 M).
- **Three defects in today's code** (§10.14), none fixed:
  - **E1.** A cold stub build can deadlock when a rayon worker forces it: a waiting worker steals a job that needs the lock it holds. The fix makes the stubs a value that the entry point owns and builds on a thread of its own, and it carries proposals 1 and 5.
  - **E14.** The parser refuses `$x instanceof static::$name`, which PHP accepts. Six files in the installed dependencies of three trees lose every declaration as context.
  - **E15.** The build's source hash leaves out `vendor/phpstan-src/stubs`, which the stub build reads, so a change there alone runs on a stale stub cache.
- **Rejected** (§10.12):
  - one rayon pool, as a speed-up by itself (it is part of proposal 5);
  - a narrower stub-cache key, while the stub build reads most of the main crate;
  - a dedicated reader for Composer's autoload file.
- **Where it ends** (§11). With the plan, proposals 7-11 and the cuts, the warm run comes to ≈9.6-10.0 G when the files' parse verdicts are cached, and ≈11.1-11.5 G when they are not. The off run meets it only with proposal 6. Without proposal 6, proposals 1-4 take ≈1.6 G off the stub build.
- **Decided** (2026-10-13):
  - a skimmed body is still checked for syntax errors, by a pass that builds only what its checks read, and each file's verdict is cached (D1, §10.8);
  - a skimmed body's text is kept from the bytes the load reads (D2, §10.8);
  - a cache checks once, at its first store, whether it can write, apart from reading (D5, §10.2);
  - the stub data is built at compile time (D3). The crate split then reopened this: it leaves no bases to embed, and D3 is decided again once its C5 sets the version key (C8, §10.6).
- **Recommended, not decided:**
  - a context file that fails to parse keeps what it declares, with what parses recovered and validity following the configured PHP version (D6, §10.8);
  - proposals 1-5 are built now, in four steps from E1's fix, since the crate split removes the eager build they speed (D7, §10).
- **Still open:**
  - D3's form, after C5's version key;
  - a run of the parser over all 111,836 files under `vendor/` against PHP's own lint, which would find any gap like E14;
  - E1's reproduction, designed and not run.

  Nothing is implemented.

## 1. The runs

| mode | stub cache | per-file cache | Ir | wall, s (B4) | exit |
|---|---|---|---:|---:|---|
| off | disabled (`PHPSTAN_RS_NO_STUB_CACHE=1`) | disabled | **25,625,190,065** | 2.681 | 0 |
| cold | enabled, its directory (`PHPSTAN_RS_CACHE_DIR`) empty | disabled | 25,683,423,164 | 2.693 | 0 |
| warm | enabled, filled by the cold run | disabled | **20,572,181,579** | 2.308 | 0 |
| both warm (B5) | warm | warm, 11,459 entries | 14,338,300,289 | 1.595 | 0 |

- **The warm run is the 20 G report's run**, to within 0.01%.
- **The diagnostics are identical.** `off/out.json`, `cold/out.json` and `warm/out.json` are byte-identical (`cmp`), and so is B5's.
- **What the off and cold runs do.** The off run builds the stub bases in memory and drops their encodings (§4.3). The cold run does the same work, keeps the encodings and writes them: 58 M more.
- **A new source hash starts cold.** The hash (`PHPSTAN_RS_SOURCE_HASH`, `crates/phpstan-rs/build.rs`) is computed from the sources, the manifests, the lockfile and `TARGET`. Every source change gives a new hash, and the first run of it is cold. A rebuild with no source change keeps the hash and finds the cache warm: the gate binary of `4db8f996` has the profile's hash. A gate or a benchmark on a build of changed source pays the stub build at least once. The one exception is a defect: an edit to `vendor/phpstan-src/stubs` alone keeps the hash (E15, §10.14).
- **The per-file cache was off in the three profiled runs**, since `stubcache/` has no `files/`. A run with it warm parses no context file (`project/file_cache.rs:73-95`). B5 profiled that run afterwards (§3.4).
- **The wall times** are medians of 8 interleaved rounds on the otherwise idle machine, with the gate binary of `4db8f996`, whose source hash is the profile's (B4, §3.3).

**Threads.** Callgrind numbers threads by slot, and a thread that starts after another has exited takes that one's slot. So each thread's role is read from the functions it runs, not from its number.

| threads | off | warm | role |
|---|---:|---:|---|
| 01 | 2,663.9 M | 1,062.5 M | main thread: session load, both stub bases, the serial insert |
| 02–09 | 19,143.9 M | 19,188.3 M | `worker_pool()`: parse, check, finishing passes |
| 10 | 948.7 M | 321.4 M | Composer's vendor discovery (321.2 M); in the off run, then a worker of the global pool |
| 11–17 | 2,868.8 M | — | the rest of the global rayon pool, which the stub build starts |

- **The second pool.** The stub build calls `par_iter()` at `registry/stubs.rs:1482` outside any `install`, so rayon starts its global pool: 8 more threads. The off run has 16 `ThreadBuilder::run` calls against the warm run's 8. Two places in the code say there should be one pool:
  - `worker_pool()` (`project.rs:362`) says the process has one pool;
  - the test at `project/tests/incremental_updates.rs:1161` says a registry build runs on it.
- **The two pools never run at once**, given the order below, so no more than 8 threads are busy.
- **Order on the main thread** (`project/session.rs:708-826`):
  1. It lists the files and waits for vendor discovery.
  2. `Project::new()` (`:733`) forces the plain stub base. That builds the stub corpus on the global pool, then resolves the base.
  3. The parse runs on the worker pool (`:748`).
  4. `expect_files` (`:791`) switches the project to the base with PHPStan's source, which Piwigo's files need.
  5. The serial insert runs.

  So the stub corpus and the plain base come before the parse, and PHPStan's base comes after it, at `expect_files`. Both serial parts run while every other core is idle.

## 2. Method

- **Phases.** `analysis/attrib.py` gives each function's self cost, thread by thread, to the phases whose roots reach it (`analysis/prop.py`).
  - The call graph's strongly connected components are condensed first, so recursion does not inflate a share.
  - The edges into a root are cut, and the other phases' roots are barriers. So a phase nested in another is carved out of it.
  - A rayon job that another thread steals starts at a rayon frame, not under its phase's root. Such a frame is a root of the phase its closure names: a `Registry` method for the finishing passes, `Session::load` for the parse, `check_files` for the check.
  - **The global pool runs only the stub-source closure** and the jobs it splits off, so all of its work goes to the stub sources. One correction follows from this. The per-file passes that `index_and_flatten` runs, such as `precompute_member_owners`, split off jobs whose frames name the finishing passes' methods: 60.4 M. Without the correction, the attribution would give those jobs to the finishing passes.
  - The shares are normalized, so the phases add up to each run's total exactly: 25,625.2 M and 20,572.2 M.
- **Blocks.** `analysis/blocks.py` splits one phase's attributed cost into subtrees, each named by a regular expression for its roots, under the same barrier rule. "Rest" is what no block reaches.
  - The block files are `bl/parse.txt`, `bl/check.txt`, `bl/finishing.txt` and `bl/stub.txt`.
  - `bl/find/` carves the name tables' lookups and the decoding of stub entries out of the check and the finishing passes, so that the two runs can be compared block by block.
- **No calling context.** Callgrind's edges carry none, so a function that two roots call is split by edge shares, not by the calls each root made. This matters in three places:
  - **The stub bases.** `resolved_stubs` serves both bases. Attribution gives 603.7 M to the plain base and 396.6 M to PHPStan's. The inclusive costs under each base's `LazyLock` are 355.1 M (the plain base's 983.4 M, less the 628.3 M corpus it builds) and 646.1 M. They add to 1,001.2 M, and the attribution to 1,000.3 M (603.7 + 396.6). The 0.9 M difference is the edge-share split, so sums use the attribution and the split uses the inclusive costs.
  - **The stub bases' finishing runs.** They ran on the worker pool beside the project's. C4 told them apart by per-thread call counts, which the raw callgrind files keep (§4.4).
  - **The warm run's stub decoding.** It is 477.8 M by attribution (275.5 M in the finishing passes and 202.3 M in the check) and 439.0 M inclusive. The 275.5 M is the plain finishing carve (`bl/finishing-warm.txt`); the carve that takes the name-table lookups out first (`bl/find/ff-warm.txt`, used in §4.4 and §7) gives 275.1 M, the 0.4 M being the lookups' share.
- **Inclusive costs** are quoted only for a single callee, never added up, because a call's cost counts under every caller on its path.
- **Verdicts.** For each block, the code its functions run was read, and the design of the plan item was checked against those lines:
  - **covered**: the design reaches the block's cost;
  - **partly**: it reaches some of it, and the rest is named;
  - **new**: no item reaches it, and a proposal or a cut does.
- **Savings.** For plan items, the savings quoted are the plan's own estimates. For the proposals and the cuts, they are computed from the profile, the code and the token counts, or measured by the investigations' probes, and each says which.
- **The probe builds.** Probe builds from a worktree at `2a7befd3`, not for commit. One (`~/.cache/prs-profile/off-ledger-b/probe.patch`) answers every purity and property-write question a second time by a search on demand, records every body read, counts what the tag lexer lexes against what a pulling parser would, and records what the checker's truncations drop (B1-B3). Another counts `is_super_type_of`'s operands and logs `powf`'s calls (B7, B10). They ran in the off mode on Piwigo, whose output stayed byte-identical to the profile's, and B1-B3 also on Laravel (`vendor/laravel`, `check-project src`, with its own `vendor/` as context).

## 3. Where the Ir goes

### 3.1 By phase

| phase | off, M | warm, M | runs on | removed by |
|---|---:|---:|---|---|
| parse job | 9,994.0 | 9,983.0 | worker pool | items 2, 4, 8-14; proposals 7, 8, 10 (§5); with the per-file cache warm, E4 |
| check | 6,832.2 | 6,774.7 | worker pool (45 M on the main thread) | items 2, 5, 7, 9, 14-18; proposals 3 (off), 11; E2, E3, E5-E7, E10 (§6) |
| stub sources | 3,495.7 | 0 | global pool | proposal 6; without it, items 4, 8-14, 19 and proposal 7 lower it (§4.1) |
| finishing passes | 2,361.1 | 2,475.0 | worker pool, mostly one thread | items 4-6, 17; proposals 3, 9; with the per-file cache warm, E11 (§7) |
| stub bases, plain and PHPStan's | 1,000.3 | 15.2 | main thread | proposal 6; without it, proposals 1, 2, 3 (§4.2) |
| serial insert | 739.8 | 751.2 | main thread | items 3, 5; proposal 8; with the per-file cache warm, E12 (§8) |
| stub corpus merge | 431.9 | 0 | main thread | proposal 6; without it, proposals 4, 5 |
| Composer vendor discovery | 321.2 | 321.3 | its own thread, off the critical path | item 2 (§8) |
| stub corpus stores | 190.7 | 0 | main thread | proposal 6; without it, proposals 2, 5 |
| session load, less the above | 169.3 | 164.1 | main thread | item 2 (§8) |
| `main` | 52.2 | 52.2 | main thread | — |
| session run | 32.9 | 32.9 | main thread | item 2 |
| reached by no root | 3.7 | 2.6 | | |
| **total** | **25,625.2** | **20,572.2** | | |

The off run runs the finishing passes three times (for the two stub bases and for the project), and the warm run once. The warm run decodes stub entries during its run instead (§4.4).

The 20 G report's own figures for the warm run are: parse 9,968 M, check 6,716 M and finishing 2,481 M. They agree with this table's warm column to within 0.25% (9,983.0 M; 6,729.3 M, with the check counted on the worker pool only; 2,475.0 M). The runs differ slightly (20,570,586,331 Ir against 20,572,181,579) and so does the attribution method.

### 3.2 By subsystem and function

Self costs, summed by subsystem (`analysis/groups.py`) over the whole run:

| subsystem | off, M | warm, M | off − warm, M |
|---|---:|---:|---:|
| lexer (`syntax::lexer`) | 3,172.7 | 2,616.9 | +555.8 |
| parser (`syntax::parser`) | 2,987.6 | 2,461.0 | +526.6 |
| other (unclassified, mostly std and `syntax` helpers) | 2,572.8 | 2,108.6 | +464.3 |
| allocator (mimalloc) | 2,221.5 | 1,753.9 | +467.6 |
| checker (`checker::`) | 2,195.2 | 2,195.2 | **0.0** |
| name interning and folding | 2,193.8 | 1,851.3 | +342.5 |
| registry, other | 2,111.4 | 1,714.9 | +396.4 |
| types | 2,027.9 | 1,964.3 | +63.6 |
| memory and UTF-8 (`mem*`, `memchr`, `from_utf8`, `Utf8Chunks`) | 1,754.6 | 1,260.8 | +493.8 |
| phpdoc | 1,121.9 | 648.4 | +473.5 |
| hashbrown and std collections | 930.1 | 676.5 | +253.7 |
| name tables and packed reads | 726.7 | 222.1 | +504.6 |
| `syntax`, other | 645.8 | 543.8 | +102.1 |
| drop glue | 478.6 | 389.3 | +89.2 |
| serialization and container | 475.6 | 157.1 | +318.5 |
| rayon and sync | 9.0 | 8.1 | +0.9 |
| **total** | **25,625.2** | **20,572.2** | **+5,053.0** |

The checker's self cost is the same in both runs to the 0.1 M. Everything the off run adds is the stub build: its parse, its indexing, its encoding and its in-memory tables.

The largest self costs of the off run (all but `Base::find` are also among the warm run's largest, at a lower cost for most, because the stub build's lexing, parsing and copying is gone: `memcpy` is 515.8 M and `next_token` 687.3 M in the warm run, and `Base::find` is 33.7 M):

| self, M | % | function |
|---:|---:|---|
| 836.3 | 3.3 | `syntax::lexer::Lexer::next_token` |
| 825.3 | 3.2 | `__memcpy_avx_unaligned_erms` |
| 591.9 | 2.3 | `syntax::parser::Parser::parse_expr_bp_inner` |
| 581.2 | 2.3 | `syntax::lexer::Lexer::next_tok` |
| 562.6 | 2.2 | `syntax::parser::Parser::fill` |
| 489.6 | 1.9 | `syntax::lexer::Lexer::next_raw` |
| 451.5 | 1.8 | `mi_free` |
| 410.3 | 1.6 | `<str as syntax::name::FoldKey>::is_folded_eq` |
| 397.9 | 1.6 | `_mi_theap_malloc_zero` |
| 378.9 | 1.5 | `syntax::lexer::Lexer::name_and_bits` |
| 315.1 | 1.2 | `syntax::parser::Parser::skip` |
| 297.4 | 1.2 | `<str as syntax::name::FoldKey>::fold_hash` |
| 293.5 | 1.1 | `registry::name_table::Base<Option<u32>>::find` (warm: 33.7) |
| 291.9 | 1.1 | `alloc::raw_vec::RawVecInner::finish_grow` |
| 288.4 | 1.1 | `__memcmp_avx2_movbe` |
| 259.3 | 1.0 | `Registry::resolve_transitive_purity` |

### 3.3 Wall time (B4)

The gate binary of `4db8f996` (source hash `558a2b69…`, the profile's) ran on Piwigo in six cache modes, round-robin for 8 rounds, each round starting at the next mode (`~/.cache/prs-profile/off-ledger-b/b4/scripts/interleave.py`). The machine was otherwise idle; the load average, 0.9-6, is the runs' own. Two earlier hyperfine passes at a load of 16-22 disagreed by a factor of two and are not used.

| mode | median wall, s | min-max | user CPU, s | system CPU, s |
|---|---:|---:|---:|---:|
| off | 2.681 | 2.48-3.52 | 9.54 | 1.92 |
| stub cache cold (the profile's cold run) | 2.693 | 2.52-3.54 | 9.41 | 1.97 |
| stub cache warm (the profile's warm run) | 2.308 | 2.11-2.50 | 8.28 | 1.94 |
| stub cache warm, per-file cache cold | 2.405 | 2.29-2.93 | 8.46 | 2.45 |
| both warm | 1.595 | 1.53-1.95 | 5.67 | 1.03 |
| both cold | 2.859 | 2.74-3.18 | 10.02 | 2.45 |

- **The stub build costs 0.37 s** (off against stub-warm): 14% of the off run's wall time, against 20% of its Ir.
- **The per-file cache saves 0.71 s** of the stub-warm run's 2.31 s (31%), against 30% of the Ir.
- **Where the phases sit.** The profile's binary was recorded with perf, frame pointers, at 1,999 Hz (`b4/perf/`). Under perf a single run took 2-16% longer than the 8-round median (off +10%, stub-warm +16%, both warm +2%; single recordings against medians, so loose), and a thread that runs all the time samples as 0.5-0.9 cores, so the core counts read about a quarter low.

  | stretch of the off run (2.95 s under perf) | s | cores busy |
  |---|---|---:|
  | start-up and vendor discovery | 0.00-0.15 | ≈1 |
  | stub sources, on the global pool | 0.15-0.35 | ≈4 |
  | stub corpus and plain base | 0.35-0.60 | 1 |
  | parse | 0.60-1.40 | ≈4 |
  | PHPStan's base, at `expect_files` | 1.43-1.60 | 1 |
  | insert | 1.60-1.80 | 1 |
  | finishing passes | 1.80-2.50 | ≈1-3 |
  | check | 2.50-2.90 | ≈5 |

  - Stub-warm (2.69 s under perf): parse 0.1-1.0 s, insert 1.0-1.2 s, finishing 1.2-2.1 s, check 2.1-2.6 s.
  - Both warm (1.63 s): parse 0.1-0.45 s, insert 0.45-0.55 s (nearly all E12's decoding), finishing 0.55-1.2 s, check 1.2-1.6 s.
- **What the wall time shows that Ir does not:**
  - **The stub stretches** take ≈0.6 s under perf, ≈0.4 s of it on one core: the corpus and the plain base (≈0.25 s), and PHPStan's base at `expect_files` (≈0.17 s).
  - **The finishing passes are the longest stretch after the parse.** They run 0.65-0.9 s at 1-3 cores in every mode. With both caches warm they are ≈40% of the wall time, against 18% of the Ir (2.62 of 14.34 G). That is the 20 G report's bottleneck thread (§7), and what items 4-6 and proposal 9 shorten.
  - **The insert is serial.** It takes ≈0.2 s with the per-file cache off, mostly the tree drops, and ≈0.1 s with it warm, mostly E12.

### 3.4 Both caches warm (B5)

The profile's binary ran under callgrind on Piwigo with a cache directory that two earlier runs had filled (`~/.cache/prs-profile/off-ledger-b/b5/`): the stub cache and the per-file cache, 11,459 entries, one per context file, 48.3 MB. The output is byte-identical to the profile's, and the run wrote nothing: every cache file's mtime and size are unchanged.

| phase | stub cache warm, per-file cache off (the warm run) | both warm | change |
|---|---:|---:|---:|
| parse | 9,983.0 M | 3,833.7 M | −6,149.3 M |
| check | 6,774.7 M | 6,704.6 M | −70.1 M |
| finishing | 2,475.0 M | 2,617.3 M | +142.3 M |
| insert (main thread, serial) | 751.2 M | 582.7 M | −168.5 M |
| vendor discovery | 321.3 M | 321.2 M | 0 |
| session and main | 267.0 M | 278.8 M | +11.8 M |
| **total** | **20,572.2 M** | **14,338.3 M** | **−6,233.9 M** |

- **The floor of today's code is 14.34 G**, with nothing cold.
- **What the parse phase keeps, 3.83 G:**
  - **the 2,792 analysed files, ≈2.51 G.** Their parse is 1,961.2 M (`parse_with_ignores`), and their products 548.0 M: `file_declarations::collect` 262.4 M (`from_def` 200.4 M of it), `FileBodies::collect`'s walk 141.3 M, `recorded_references` 79.5 M, `GlobalFacts::of` 64.8 M;
  - **the 11,459 cache hits, ≈1.12 G**, ≈98 K per entry, ≈23 per stored byte: decoding 985.6 M (`BuiltDecl` 452.7 M, `FileBodies` 338.4 M, the references 86.3 M, `LineIndex` 51.0 M, `DeclaredClass` 29.8 M, the rest 27.4 M), making the references' sets 30.0 M, the checksum 23.7 M, the key's content hash 19.7 M, the reads 29.1 M, and the entry's path 30.8 M (`format!` and `Path::join`, twice per hit). About 4 M of the reads and joins are the 2,792 analysed files', which are read and joined too.
  - The two bullets make up 3.63 G of the 3.83 G. The other ≈0.2 G is parse-phase work that they do not itemise, such as the load closure's per-file path work.
- **`from_def`** falls from 1,519.7 M over 12,546 calls to 202.1 M over 2,005: the analysed files' 1,932 classes and 73 anonymous ones.
- **The analysed files' declarations (N3).** The per-file cache stores context files only (`project/file_cache.rs:7-8`; `cacheable` is false for analysed files, `project/session.rs:762`), because checking needs the tree. Their products do not, and they cost 548 M here, matching the ≈0.5-0.6 G that N3 estimated from the tokens (`analysis/nn/decls.php`, `docs.php`):

  | | files | MB | class-likes | methods | functions | closures | docs on declarations | other docs | body tokens |
  |---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
  | analysed | 2,792 | 19.07 | 1,956 | 9,357 | 1,144 | 6,561 | 6,668 (2.61 MB) | 35,377 (1.82 MB) | 1,384,375 |
  | context (classmap) | 10,610 | 34.42 | 10,475 | 50,553 | 9,333 | | 41,253 (6.07 MB) | 2,573 | 2,599,131 (≈2.81 M over 11,459 files) |

  The analysed files hold 13.9% of the declarations' docs by count, 30% by bytes, and 33% of the body tokens. With the cache off, `built_declaration` is 1,668.5 M (classes 1,517.1 M over 12,407, functions 75.4 M over 2,420), so their share is ≈0.24-0.32 G; their bodies' products (780.5 M in all) at 33% are ≈0.26 G. Two things remove it: **E4**, their entries in the per-file cache, which nets ≈0.25-0.3 G after decoding (§10.13), or the F2 result cache (`CRATE_SPLIT_PLAN.md:7005-7110`, not built), which skips unchanged files whole. A changed file's cost is inherent.
- **Two costs only the warm per-file cache pays:**
  - **E11, +116 M in the finishing passes.** The purity and property-write classifiers read 103,921 summaries from cached files, and each read hashes the file's path (`HashMap<PathBuf, FileId>::get`, ≈1.1 K Ir each). The same 121,031 reads cost 11.1 M in all with the cache off.
  - **E12, ≈0.4 G in the serial insert.** `record_assert_tagged_names` decodes 51,750 lazily stored methods of cached classes, 419.2 M, to read whether each one's assert list is empty. Without that, the insert would be ≈0.18 G.

## 4. What the off run adds: 5,053 M

| part | threads | off, M | warm, M | difference |
|---|---|---:|---:|---:|
| stub sources | global pool (10–17) | 3,495.7 | 0 | **+3,495.7** |
| stub corpus and stub bases | main | 1,622.9 | 15.2 | **+1,607.7** |
| the rest of the main thread | main | 1,041.0 | 1,047.3 | −6.3 |
| worker pool | 02–09 | 19,143.9 | 19,188.3 | −44.5 |
| vendor discovery, and what no root reaches | 10 | 321.7 | 321.4 | +0.3 |
| **total** | | **25,625.2** | **20,572.2** | **+5,053.0** |

### 4.1 The stub sources: 3,495.7 M on the global pool

The closure `build_generated_stub_data::{closure#2}` runs once per stub file: 2,515 calls, 3,495.5 M inclusive. Its callees, inclusive:

| callee | Ir | calls |
|---|---:|---:|
| `syntax::parse::parse` | 1,570.9 M | 2,515 |
| `Registry::index_and_flatten` | 1,526.8 M | 2,515 |
| `deps::referenced_names` | 223.2 M | 2,515 |
| drop of the statements | 76.3 M | 2,515 |
| `stub_attributes::apply_stub_attributes` | 67.5 M | 2,515 |

`index_and_flatten` (2,556 calls, 1,548 M) is mostly the building of declarations:
- `index::build_declarations`, 1,364.7 M;
- `GlobalFacts::of`, 38.8 M;
- the per-file passes: `precompute_ancestor_names` 20.7 M, `precompute_member_owners` 18.7 M, `drop_doc_members_excluded_by_native` 17.6 M, `resolve_deferred_value_of` 12.8 M and `resolve_constant_named_types` 11.7 M.

`built_declaration` runs 32,450 times in the off run (2,937 M inclusive) against 15,127 times in the warm run (1,668 M). The 17,323 extra calls are the stub declarations, and they cost 1,268 M:
- `ClassReflection::from_def` costs 2,313 M over 16,164 calls in the off run, against 1,520 M over 12,546 calls in the warm run;
- `FunctionReflection::from_decl` costs 507 M over 7,854 calls in the off run, against 75 M over 2,420 in the warm run, so the 5,434 stub functions cost about 432 M.

By block (attributed, `bl/stub-off.txt`):

| block | M | what removes it, without proposal 6 |
|---|---:|---|
| docblock | 930.5 | item 14; proposal 7 (B2: 78.9% of the bytes the stubs' tag lexer lexes are not needed) |
| parser | 822.1 | items 9, 10 |
| lexer | 662.1 | items 10, 12 |
| `from_decl` | 229.1 | items 8, 14 |
| `deps::referenced_names` | 168.1 | item 13.4 |
| `index_and_flatten`'s own work and its per-file passes | 141.3 (80.9, and 60.4 run as stolen jobs) | item 4 |
| `build_declarations`' walks | 135.2 | item 13 |
| `from_def` | 101.3 | items 4, 8, 14 |
| line index | 93.4 | item 11 |
| tree drop | 79.0 | item 9 (item 3 is the context-tree case and does not apply to stub files) |
| global facts, stub attributes, attribute mentions, the closure's own frames | 133.4 | only proposal 6 |

Nothing of this is reused between builds, because the cache's key is the whole build's source hash (`registry/stub_cache.rs:17`, `:46-48`). The average is ≈0.21 G per MB of stub source (3.50 G for 16.34 MB), which C8 uses to price what is left after the crate split (§10.6).

### 4.2 The stub corpus and the stub bases: 1,622.9 M serial on the main thread

`resolved_stubs` (`project.rs:566`) builds two bases under `LazyLock`, plain and with PHPStan's source, once each. Both read the stub corpus (`built_stub_corpus`), which whichever base comes first builds.

| step | Ir | note |
|---|---:|---|
| the corpus's merge, `build_generated_stub_data` (`stubs.rs:1453`) | 437.5 M inclusive, 153 M self | at least 234.5 M of it is `build_closures` (`stubs.rs:889-912`, inlined): 893,154 `Packed::get` calls cost 170.6 M, and the lines around them 63.9 M |
| the corpus's stores: stub index and stub data (`stubs.rs:1420-1429`) | 190.4 M | |
| the plain base, less the corpus | 355.1 M | its `LazyLock` costs 983.4 M, of which the corpus is 628.3 M |
| the base with PHPStan's source | 646.1 M | built from all the stub data (`stubs.rs:384`), not from the plain base |

Of the two bases' 1,001.2 M:
- the table stores and the container writes cost 814.9 M (`NameTable<ClassReflection>::store` 489.6 M, `NameTable<FunctionReflection>::store` 137.8 M, the other tables, name sets and maps 185.2 M, `store_container` 2.3 M);
- `Registry::freeze` costs 108.8 M, sorting each table;
- the merges (`merge_stubs`, `merge_stub_classes`, `merge_stub_constants`) cost 52.5 M;
- the rest costs 25.0 M.

The stores are four fifths of the bases' cost. The plain base's share of them is ≈0.26-0.30 G (C3, §10.2).

The bases' finishing passes run on the worker pool (`finish_loading`, `project.rs:599`), at ≈0.15-0.17 G for the two (C5, §4.4).

By subsystem, the main thread's extra 1,601.4 M (self cost, off against warm):

| subsystem | M |
|---|---:|
| serialization and container | +459.6 |
| memory and UTF-8 | +238.1 |
| name tables and packed reads | +208.7 |
| registry, other | +197.1 |
| name interning and folding | +182.3 |
| allocator | +132.2 |
| hashbrown | +110.0 |
| other | +45.9 |
| types, phpdoc, rayon, lexer, parser, `syntax` | +27.4 |

### 4.3 Serialization that no store keeps: 1,005.3 M, off run only

Every store in the stub build serializes first and writes second:
- `stub_cache::store` (`stub_cache.rs:117-124`) encodes, then calls `store_container`, which returns at once when `cache_dir()` is `None` (`stub_cache.rs:73-76`);
- `Registry::store_cached` (`registry.rs:733`) and `StubIndex::store_cached` (`stubs.rs:1251`) work the same way.

So with the cache off, the bytes are built and then dropped:
- **The work as a whole.** The whole store work costs 1,005.3 M inclusive under the two places that store: 814.9 M under `resolved_stubs` and 190.4 M under the corpus's `LazyLock`. All of it runs on the main thread, with the pool idle.
- **The serialization itself.** Its self cost (the `postcard` serializer, the `serde` `Serialize` impls, `container::` (the `Writer` and its span map) and `stub_cache::encode`) is 493.2 M on the main thread, and nothing on any other thread. The warm run pays 2.5 M of it.
- **The encodes.** Inclusive: `stub_cache::encode::<ClassReflection>` 598.1 M over 13,569 calls, `stub_cache::encode::<FunctionReflection>` 526.3 M over 130,517 calls. (The `Serialize::serialize` impls under them cost 597.6 M and 521.7 M over 130,617 calls; the extra 100 calls are nested records.)
- **The cold run** pays the same encoding and keeps it, which is why it costs only 58 M more than the off run, not 0.5 G more.

### 4.4 The worker pool: −44.5 M, by phase

| phase | off, M | warm, M | difference | what differs |
|---|---:|---:|---:|---|
| parse | 9,994.0 | 9,983.0 | +11.0 | the same files |
| check (pool) | 6,787.1 | 6,729.3 | +57.9 | **off:** 127.0 M bisecting the name tables of bases built in memory (`bl/find`: 142.9 M against 15.9 M). **Warm:** 113.8 M decoding the stub entries the check reads first, outside the lookups (202.3 M with the decodes under other blocks). Every other block agrees within 32 M (§6) |
| finishing | 2,361.1 | 2,475.0 | −113.9 | **off:** +193.6 M of bisection (238.5 M against 44.9 M), and the two stub bases' runs, spread over the passes. **Warm:** decoding 275.1 M, the deferred-value pass's extra 92.3 M, `Members::eq` 53.5 M, and ≈100 M of stolen jobs that land outside the named blocks (§7's `rest` row: 99.5 M, or 90 M net of the off run's 9.5 M) |
| other | 1.6 | 1.1 | +0.5 | |
| **total** | **19,143.9** | **19,188.3** | **−44.5** | |

So:
- the warm run pays ≈0.48 G to decode stub entries lazily;
- the off run pays ≈0.32 G more for lookups in bases built in memory (proposal 3);
- the off run also pays ≈0.15-0.17 G for the two stub bases' finishing runs.

**The stub bases' finishing runs (C4, C5).** The main thread called `finish_declarations` three times: twice from `resolved_stubs` and once from `Session::load`. Worker 02 executed two of the injected jobs (2,069.1 M inclusive on its thread), and worker 09 one (117.2 M). The per-thread call counts in the raw callgrind files say which was which:
- `flatten_traits_one_pass`'s per-class closure ran 58,905 times on the pool: 1 × 1,542 + 3 × 3,495 + 3 × 15,626. The plain base has no trait users, so it stops after one pass over its 1,542 classes; PHPStan's base takes three passes over 3,495, and the project three over 15,626.
- On worker 09 that closure ran 10,485 = 3 × 3,495 times, `resolve_deferred_value_of` 3,495 times, and `classify_purity` 3,495 (classes) and 5,282 (functions) times: PHPStan's base. On the other workers, `classify_purity`'s class closure ran 5,448 times on worker 02 and 11,720 times on worker 04: 17,168 = 1,542 + 15,626, the plain base and the project. (`drop_doc_members_excluded_by_native` runs one closure per signature, methods and functions alike, `resolve.rs:1742`: 34,969 calls on worker 09, so it does not count functions alone.)

So **PHPStan's base's run is 117.2 M, measured**: the main thread was blocked in `install`, the pool had no other work, and every one of that run's closures ran on worker 09. **The plain base's run is ≈35-52 M, estimated**: it shared worker 02 with the project's run, and scaling 117.2 M by class count (1,542 against 3,495) or by class-table bytes (4.40 against 14.69 MB) gives that range. Together ≈150-170 M. The drill's first figure, ≈211 M, summed the finishing blocks that cost more in the off run once lookups and decoding were carved out (member owners +48.4 M, PHPDoc inheritance +44.5 M, purity +39.0 M, dropped doc members +27.5 M, constant types +16.8 M, ancestors +14.4 M, traits +10.2 M, classification +7.8 M, the rest +2.8 M). Its ≈40-60 M beyond the measured runs is cost that stolen jobs moved between blocks.

### 4.5 The cache writes (B6)

- **The stub cache.** A cold run writes 4 files, 31.4 MB, each through a temporary and a rename (`stub_cache.rs:73-90`). The writes take ≈10 ms under strace, and all of the stub cache's system calls ≈19 ms. With the 58 M of encoding (≈15 ms), the cold run costs what the off run does: 2.693 s against 2.681 s, within a spread of ±0.3 s.
- **The per-file cache's first write.** The profiled runs had it off. The first run with it on writes 11,459 entries, 48.3 MB.
  - Per entry, `store` (`file_cache.rs:223-261`) calls `create_dir_all` (a `mkdir`; 11,473 in all), then `getpid` for the temporary's name, then `openat` with `O_CREAT`, `write`, `close` and `rename`.
  - When the shard exists, the `mkdir` fails with `EEXIST` and `create_dir_all` stats the shard: 11,208 `statx` on the workers. Then `trim` lists the directory and stats every entry once: 11,459 `statx`, on the main thread (`session.rs:786`).
  - Its CPU, ≈0.8 s by perf (the rows below add to 0.82 s, 0.63 s of it in the kernel), against the stub-warm recording (`b6/classify.py` over `b6/filescold.data` and `b4/perf/stubwarm.data`): `openat` +315 ms, `rename` +111 ms, `mkdir` +80 ms, `write` +76 ms, `statx`, `getdents64` and `close` +47 ms, postcard encoding +191 ms. Single perf runs are noisy: the encoding alone is more than the interleaved runs' user CPU.
  - The interleaved runs give +0.51 s of system CPU, +0.18 s of user CPU (≈0.7 s in all) and +0.10 s of wall (2.405 s against 2.308 s). The parse workers share the cost.
  - **E13** makes the shards once and removes the `mkdir`s and the shards' `statx` (§10.13). The `openat`, `write` and `rename` per entry, ≈0.5 s of CPU, stay unless entries are packed into fewer files, which changes the cache's format and its trim.
- Strace's per-call times are not usable here: under `-f` it serializes the threads, so the 11,459 writes show as 3.1 s. Its call counts are exact.

## 5. The parse job: 9,994 M

**Whose files they are.** The job reads 14,251 files: 2,792 analysed and 11,459 context files, whose trees the insert drops (§8).
- **2 context files** come from `scan_paths` (`phpstan-stubs/`).
- **The other 11,457** come from Composer's autoload data:
  - Discovery reads `autoload_static.php`: its `classMap` and `files` entries are files, and its `prefixDirsPsr4` directories are walked for more (`composer/src/vendor.rs:94-116`).
  - Composer's `ClassLoader.php` is added, since no map lists it (`:77-80`).
  - The classmap and `files` entries are 12,391 distinct files: 12,197 from the classmap and 196 from `files` (2 of them in both), as `php` reads `autoload_classmap.php` and `autoload_files.php`, which list what the static file holds. Less the analysed files, 10,610 remain, all under `vendor/` (`analysis/n5/vendor.txt`).
  - The other 847 context files come from the PSR-4 walks and `ClassLoader.php`.

**Most of the code is context files' function bodies.** PHP's tokenizer, over those 10,610 files and the 2,792 analysed ones (`analysis/n5/bodies.php`):

| set | files | tokens, less whitespace and comments | of them inside function and method bodies | bytes inside bodies |
|---|---:|---:|---:|---:|
| context (classmap and `files` entries) | 10,610 | 3,854,257 | 2,599,131 (67.4%) | 17.4 MB of 34.4 MB (50.7%) |
| analysed | 2,792 | 1,732,841 | 1,384,375 (79.9%) | 12.8 MB of 19.1 MB (67.2%) |

- **Their share of the job.** Scaled to all 11,459 context files at the same density, their function bodies are ≈2.8 M of the ≈5.9 M tokens: **≈48% of everything the job lexes and parses (47.6%).** B1's probe counted them directly: 49,533 bodies, 17.7 MB of the context files' 37.1 MB.
- **What is kept of a body.** A context file's tree is dropped at the insert, and of a body only its summary and its flow are kept (`registry/bodies.rs:47-55`).
- **How few are read.** A run reads 313 of Piwigo's 49,533 context bodies (0.63% by count, 102 KB), and 192 of Laravel's 38,146 (B1, §10.8).
- **Who pays for it.** A warm per-file cache keeps those products, and with it a run parses no context file (`ast: None` in `into_parsed`; B5, §3.4). So the bodies' share is paid by every run whose per-file cache is cold or off:
  - the first run of each build, because the cache is keyed by the build (`file_cache.rs:69`; the `BUILD` constant is at `:43`);
  - every run of a CI job that does not keep the cache directory.

| block | off, M | warm, M | what it is | removed by | verdict |
|---|---:|---:|---|---|---|
| parser | 3,647.4 | 3,637.9 | `parse_expr_bp_inner` 501, `fill` 455, `skip` 255, `take_token` 171, `parse_primary` 160, memcpy 108, `Token` drops 102 | items 9 (0.8-1.2 G) and 10; proposal 8 | covered; proposal 8 adds to it |
| lexer | 2,935.5 | 2,937.4 | `next_token` 664, `next_tok` 470, `next_raw` 395, `name_and_bits` 281, `lex_ident_span` 173, `lex_operator` 166, `Keyword::try_from_bytes` 127 | items 10 (≈1.7 G with the parser's side) and 12 (≈0.35 G); proposal 8 | covered; proposal 8 adds to it |
| docblock | 808.1 | 826.4 | `docblock::parse_shared`: the tag lexer, trims, memo probes | item 14 (≈0.35 G); proposal 7 | partly: descriptions are tokenized for one reader |
| `from_decl` | 451.4 | 447.0 | building function signatures | items 8 (≈0.1 G), 14 (its docblock callee), 4 (allocations) | partly; the rest is the declarations' own work |
| `from_def` | 419.7 | 414.7 | building classes | items 4, 8, 14 | partly (below) |
| body summary | 343.9 | 343.2 | `BodySummary::of` | item 13.3 for the walk; proposal 8 for context bodies | partly: one walk saves only the walk frames, ≈0.20 G across this block, global facts and init flow (68.3 + 97.2 + 33.5 M), ≈0.23 G with bodies walk (28.9 M) |
| line index | 244.9 | 244.8 | `LineIndex::new` 105, `Utf8Chunks` 71, memchr 63 | item 11 (≈0.2 G): line starts from newline bitmasks in one pass instead of a memchr per line, columns only for analysed files | covered |
| global facts | 218.5 | 218.5 | `GlobalFacts::of` | item 13.3; proposal 8 | partly |
| init flow | 218.1 | 217.5 | `InitFlow::of` | item 13.3; proposal 8 | partly |
| `file_decls` | 204.2 | 203.8 | the declaration walk | item 13.1 | covered |
| recorded references | 167.2 | 149.6 | `deps::recorded_references`, doc names | items 13.7, 14.6 | covered |
| read | 81.9 | 88.3 | `from_utf8` 29.8 M over 14,251 files (66.5 M with its other callers in the parse phase), `fs::read` | proposal 10 | new, small |
| package owner | 67.2 | 67.3 | `PackageIndex::owner`, path components | item 2 | covered |
| `read_and_parse_file`, rest | 65.2 | 66.3 | `Path` hashing 24.9 M | item 2 | covered |
| absolute path | 59.6 | 59.2 | path normalization | item 2 | covered |
| bodies walk | 55.2 | 55.1 | `FileBodies::collect` | item 13.3 | covered |
| anonymous class names | 5.1 | 5.1 | `name_anonymous_classes` | — | |

**`from_def` and the plan.** In the warm run, `ClassReflection::from_def` costs 1,519.7 M inclusive for 12,546 classes, all under `built_declaration` and `file_declarations::collect`. Its callees:
- `docblock::parse_shared`, 580.7 M (38%): item 14;
- `FunctionReflection::from_decl`, 513.2 M (34%): item 8 covers its case references and `dedup_versioned_params` (≈0.1 G), and item 4 its allocations;
- `infer_const_type_with`, 106.9 M;
- `declared_type`, 52.2 M.

With the per-file cache warm, declarations are built only for the analysed files (`CachedFile.built_decls`, `project/file_cache.rs:85`), and `from_def` costs 202.1 M (B5, §3.4).

## 6. The check: 6,832 M

| block | off, M | warm, M | what it is | removed by | verdict |
|---|---:|---:|---|---|---|
| statements | 2,879.3 | 2,850.1 | `check_stmt_impl` and below, less the blocks named here (split below) | items 4, 9, 11, 12.6, 14, 15, 16, 18.1, 18.2, 18.3, 18.5-18.7, 18.9, 18.10, M10; proposal 3; E2, E3, E10 | partly: ≈0.46 G is the rules' own logic (N1) |
| inference | 536.0 | 545.3 | `infer_expr` | item 18.1 (one inference per node) | covered |
| type relation | 522.2 | 504.7 | `Relation::*` | item 7 (≤0.3 G); E5, E6 (§9.6) | partly: the relation's own work stays |
| interning | 382.8 | 369.9 | `types::intern` | items 16, 18.6; E7 (§9.6) | partly: ≈110 M of hashing and equality stays (`exact_hash` 62.7 M, `exact_eq` 49.5 M in the off run; 59.3 and 49.0 M warm, §9.6) |
| method return types | 324.1 | 328.8 | `method_call_return_type` | items 15, 18.9 | partly |
| `check_files` | 308.5 | 314.5 | per-file setup, `Path` hashing 24.2 M, path compares, `check_program` 20.2 M | items 2 and 5 (path data from the walk, files by id); item 18.10 (one checker per worker) | covered |
| prewalks | 301.8 | 301.5 | `Promotions`, `Members`, `global_constants`, the include and `declare` walkers | item 18.1 (observer families) | covered |
| class lookup | 256.0 | 244.8 | `lookup_class`, `ClassOwners` | item 15 | covered |
| function body setup | 242.3 | 245.1 | `check_function_impl` | items 18.8, 18 F1 | covered |
| docblock | 224.3 | 240.4 | `docblock::parse_shared` called from the checker | item 14; proposal 7 | partly |
| format | 161.4 | 161.7 | `format_inner` | items 14, 18 F1; proposal 11; E10 (§9.2) | covered, except ≈13 M that F4 and F6f only reduce |
| check arguments | 146.0 | 146.6 | `check_args` | items 18.1, 18.7 | covered |
| narrowing | 138.8 | 139.8 | `apply_narrowing_unguarded` | items 18.3, 9 | covered |
| method lookup | 136.6 | 137.1 | `lookup_method*` | item 15 | covered |
| function lookup | 106.9 | 57.8 | `resolve_function`; the off run bisects a built base | item 15; proposal 3 (off) | covered |
| PHPStan-API walker | 87.1 | 87.5 | | item 18.1 | covered |
| declaration checks | 63.0 | 63.7 | generic bounds, overrides, property declarations | item 18 F1 | covered |
| rest | 15.2 | 35.5 | | | |

**The two columns.** They differ by bisection and by decoding:
- the off run spends 127.0 M bisecting the name tables of bases built in memory;
- the warm run spends 113.8 M decoding the stub entries the check reads first (202.3 M with the decodes under other blocks).

With both carved out (`bl/find/cfd-*.txt`), every other block agrees within 32 M. So the check does the same work in both runs.

**The statements block, split** by function family (`analysis/stmtsplit.py`). The groups are tried in this order, and a function falls in the first that matches:

| part | off, M | warm, M | removed by |
|---|---:|---:|---|
| `imbl` maps of the scope: `get` 57.4, `Cursor::next` 34.6, `DiffIter` 29.3, `make_mut` 23.2, leaf inserts 21.7, removes 12.3 | 288.1 | 287.8 | item 18.3: radix arrays |
| scope keys and copies: `KeyTable::find_var` 53.4, `Locals::clone` 31.6, `merge_scopes` 17.2, `KeyTable::intern` and `resolve` | 119.4 | 119.6 | item 18.3: slots, persistent scopes; F6c |
| names: `is_folded_eq` 91.6, `fold_hash` 62.1, `memrchr` 40.9, `Name::as_str` 35.0, `rsplit`'s `next_back` 22.7, the interner 20.3, `to_ascii_lowercase` 16.6, `text_hash` 16.2 | 354.2 | 355.2 | items 15, 16; see the note below |
| name tables: `Base::find` 53.6, `contains_key` 35.9 across three tables (37.1 with a fourth, the `HashMap<String, Type>`), `Keys::bytes` 13.6 | 115.0 | 60.9 | proposal 3 for the off run's bisection; item 15 |
| allocator, `mem*`, drops, growth, clones of vectors and strings | 392.3 | 403.4 | the items M10 assigns |
| the dispatch's own frames: `check_expr_impl` 230.2, `check_stmt_impl` 84.2, `check_expr_cached` 38.4, `check_stmt` 15.0 | 367.8 | 367.9 | item 18.1 removes the re-inference; the dispatch itself stays |
| `resolve_call_args` | 30.5 | 30.5 | item 18.7 |
| the rest: the rules' own code, type operations, registry reads | 1,211.9 | 1,224.9 | about three fifths by items (below) |

- **The names row.** Most of its `memrchr` and `rsplit` is the function-call arm dispatching by name (`checker/expr.rs:817-966`). That arm makes seven `rsplit('\\')` calls and three lowercase copies per call, and item 15 names it.
- **The rest, classed by owner (N1, `analysis/nn/n1split.py`).** Self cost unless marked, warm run:

  | owner | M | largest |
  |---|---:|---|
  | item 15, names as names | 164.5 | `lookup_method_with_owner` 40.9, `substitute_class_templates` 13.6, `declared_method_name` 11.1, `record_member_usage_token` 10.3, `lookup_property*` 15.8, and the function-call arm's 11 `eq_ignore_ascii_case` per call over 47,957 calls |
  | items 18.5 and 18.6, type operations in borrowed form | 149.1 | |
  | item 14, PHPDoc | 76.1 | `var_tag_nodes` 16.2 (112 inclusive); 540 `parse_shared` misses at ≈160 K each |
  | item 16, interning | 73.2 | `remembered` 20.9, `Type::new` 16.7, `Type::eq` 9.0, `canonical_int_key` and `ShapeKey` 9.2 |
  | item 18.3, slots and radix scopes | 60.1 | |
  | item 18.10, F1, F4, F6f | 51.8 | `write_str` 16.1 (14.4 in one function), the regex crates 14.8 (7.1 in `BoundedBacktracker::search_imp`), the SQL parser 13.7 |
  | item 18.1, one inference per node | 37.7 | `open_infer_scope` and `close_infer_scope` 22.4, over 1.43 M opens |
  | items 18.7 and 18.9, argument binding and signatures | 34.5 | |
  | item 9, `peeled` as a loop | 25.7 | 3.08 M calls; 74.1 M over the whole run |
  | item 11, line index | 21.3 | |
  | item 18.2, effects in the statement result | 20.4 | |
  | items 12.6 and 16, UTF-8 checks of names and keys | 18.5 | |
  | item 4, declaration ids | 13.1 | |
  | E2, E3 (new) | 22.7 self, ≈103.7 inclusive | below |

  The items reach ≈746 M of it. What no item reaches is 456.3 M over 1,181 functions, none above 8.4 M; 220.4 M of it is in 59 functions of 2 M or more (`bind_assignment_target` 8.4, `check_deprecated_method_call` 8.3, `resolve_pseudo_at` 8.0, `as_object` 7.7, `type_check_call_outcome` 7.1, `forwards_static_binding`, `check_castable_array_arguments` and `check_static_call_context` 6.5 each). That is the rules' own logic: spread thin, with no single cost to remove. The drill's first reading, "≈1.2 G of the rules' own logic", counted the items' share as well.
- **Two wastes the split found:**
  - **E2.** `check_stmt_impl` walks every statement's assigned names (43,445 calls, 12.0 M) and its defined names twice (290,162 calls over 145,081 statements, 36.3 M). Only the `@var` targets read them, and `var_tag_nodes` ran for only 1,645 statements. ≈ −50 M, exact.
  - **E3.** `apply_phpunit_assertion_narrowing` probes every expression statement's call (`phpunit_method`: 86,463 calls, 55.4 M; off, 57.2 M) for a PHPUnit assertion, inferring the receiver or resolving the class first. Every name it accepts starts with `assert`. ≈ −35 M, exact.
- **A third, from B3: reports that a truncation drops.** The probe recorded what each truncation drops:

  | site | Piwigo | Laravel |
  |---|---:|---:|
  | `rewind_to` from `control_flow.rs:242` | 214 (133 KB) | 285 (27 KB) |
  | `rewind_to` from `stmt/loops.rs:105` | 8 | 12 |
  | `rewind_to` from `stmt/loops.rs:382` | 48 | 5 |
  | `rewind_to` from `stmt.rs:271` | 0 | 2 |
  | `check_function`'s dynamic-variable truncation (`function_decl.rs:1208`) | 31,660 (1.81 MB) | 2 |
  | `check_program`'s (`checker.rs:1806`) | 0 | 0 |
  | reports the check keeps | 651 | 9,800 |

  A body that calls `extract()` or `eval()` drops every report made inside it at its end (`function_decl.rs:1208`, `checker.rs:1806`). Piwigo's compiled Latte templates (`_analysis/phpstan-latte/`, 34,570 `@var` tags) call `extract($ʟ_args)` and then name each variable with a `@var` tag; `check_statement_var_tag_names` (`checker/stmt.rs:433`, `:529`) does not count `extract()` as defining a name, so it formats 31,481 `varTag.variableNotFound` reports that are then dropped: ≈11 M of messages, at the mean cost of `format_inner`'s 66,295 calls from `check_stmt_impl`, and 23.4 M of `report::<&str>` over exactly 31,481 calls (over all 32,395 calls of `report`, `DiagnosticBuilder::build` is 16.1 M, 14.8 M of it `LineIndex::range`, and the vector's growth 4.6 M), ≈35 M in all. **E10** reports nothing while an enclosing body has called either (§10.13). The loops' convergence passes, by contrast, discard only 270 reports on Piwigo and 304 on Laravel.

## 7. The finishing passes: 2,361 M

The off run runs the passes three times (for the two stub bases and the project), and the warm run once. In this table the name tables' lookups and the decoding of stub entries are carved out of the passes (`bl/find/ff-*.txt`), so each row compares the same work.

| block | off, M | warm, M | what it is | removed by | verdict |
|---|---:|---:|---|---|---|
| purity | 660.4 | 621.4 | `resolve_transitive_purity`. Its 259.3 M of self cost is case-folded hashing of `DeclKey`s (the ASCII case fold at `core/src/num/mod.rs:863` 91.1 M, FxHash's multiply and rotate ≈57 M). The `DeclKey` maps cost 67.3 M and 38.8 M, and `DeclQuery::eq` 34.0 M | item 4 (ids: ≈0.5 G of string keys across this row, property writes and classification); proposal 9 | covered by item 4; proposal 9 removes the pass |
| name-table lookups | 238.5 | 44.9 | `Base<Option<u32>>::find` 183.9 M: the off run bisects bases built in memory | proposal 3; item 15 | new for the off run |
| member owners | 190.2 | 141.8 | `precompute_member_owners` | items 4, 5 | covered |
| property writes | 188.2 | 189.2 | `classify_summary` over every body | item 4; proposal 9 | as purity |
| PHPDoc inheritance | 184.5 | 140.0 | `flatten_phpdoc_inheritance` | items 4, 14 | covered |
| ancestors | 112.4 | 98.0 | `precompute_ancestor_names` | item 4 (by levels, inserts by id) | covered |
| dropped doc members | 92.9 | 65.4 | `drop_doc_members_excluded_by_native` | item 4 | covered |
| scopes | 83.5 | 88.0 | `finishing_scopes`, frontier sets of `String` | items 4 (a bit set over ids), 15 | covered |
| classification | 77.8 | 70.0 | `classify_purity` | item 4; proposal 9 | as purity |
| affected | 70.3 | 68.4 | `walk_affected` | items 4, 5 | covered |
| deferred values | 58.0 | 150.3 | `resolve_deferred_value_of` | item 6, step 1 | covered |
| traits | 53.2 | 43.0 | `flatten_traits` | item 4 (`MethodRef`, no copies) | covered |
| constant types | 50.4 | 33.6 | `resolve_constant_named_types` | item 4 (on demand, a memo per id) | covered |
| globals | 32.7 | 31.8 | `infer_global_variables_of` | item 4 | covered |
| reset scoped | 28.8 | 30.2 | `reset_scoped` | item 4 (the stores are the as-built state) | covered |
| decoding | 0 | 275.1 | stub entries decoded on first read | items 6 and 4 (§9.5) | partly: class decodes stay |
| member compares | 5.1 | 58.6 | `Members::eq` in `changed_since` | item 4 | covered |
| the passes' own frames | 224.7 | 225.6 | the property-write table's probes 46.1 M, `run_finishing_passes` 27.9 M, folded names 25 M, `Path` hashing 12 M | items 4, 2 | covered |
| rest | 9.5 | 99.5 | stolen jobs that no block's root reaches | | |
| **total** | **2,361.1** | **2,475.0** | | | |

- **The bottleneck thread.** These passes run on the thread that the 20 G report names as the run's wall-time bottleneck. Their serial part is 0.41 s of the sampled run, and purity and property writes are 0.19 s of that (`PROFILE_DRILL_PIWIGO_20G.md`, item 4). B4 puts the whole stretch at 0.65-0.9 s at 1-3 cores in every mode, ≈40% of the wall time with both caches warm (§3.3).
- **The eager pass.** The purity, property-write and classification rows are the eager pass over every declaration. Together they cost 926.4 M in the off run and 880.6 M in the warm run (§10.9).
- **With the per-file cache warm** the passes cost 142.3 M more: about 116 M of it from the summary reads of cached declarations hashing the file's path (E11, §3.4), and the rest (≈26 M) from decoding the cached entries (interning, UTF-8 checks, postcard).

## 8. The serial insert, vendor discovery and the session

**Serial insert, 739.8 M**, on the main thread, ≈0.2 s of wall (B4).
- **Dropping the trees.** Dropping the 11,459 context files' trees costs 567.8 M (`drop_glue::<[Stmt]>` called from `insert_numbered_file`).
  - It includes mimalloc's cross-thread frees: `mi_free_try_collect_mt` 75.1 M, `mi_abandoned_page_try_reclaim` 68.9 M and `_mi_page_free_collect_partly` 58.3 M. The trees were allocated on the workers and are freed on the main thread.
  - Item 3 drops each tree on its worker, which takes the drop off the serial path and makes the frees local. Proposal 8 takes away about two thirds of the drop's cost, the share the 20 G report puts on function bodies (387.3 of 575.5 M). Built as PROD-1 to PROD-3, it cut the drop by 53% (575.5 to 267.8 M) and the context trees at the off run's peak by ≈42% (MEAS-2).
- **`register_references`**, with the `FileId` set inserts, costs 92.7 M. Item 5 replaces them with posting lists.
- **`insert_built_decl`** costs 44.8 M. Items 4 and 5 cover it.
- **With the per-file cache warm** the insert has no trees to drop, and costs 582.7 M, ≈0.1 s: 419.2 M of it decodes every method of every cached class to read its assert tags (E12, §3.4).

**Composer vendor discovery, 321.2 M**, on a thread of its own.
- **One file is read.** When `autoload_static.php` exists, it is the only generated file read (`composer/src/vendor.rs:84-92`). Piwigo's 1.6 MB `autoload_classmap.php` is not parsed, so no file is read twice.
- **Parsing it, 162.8 M** (self cost of everything reached under `parse_generated_file`). The lexer 46.3 M, the parser 45.1 M and the line index 11.1 M over the 1.8 MB static file are 102.5 M of it; the rest is allocation, copies, UTF-8 checks and the recorder. It is the one PHP parser, which P10 decided to keep.
- **Path work and the rest, 158.4 M** (inclusive): `Resolver::path` 91.8 M, `sort_paths` 45.5 M and dedup 7.0 M. The self-cost leaders are `Components::next` 34.6 M, `parse_next_component_back` 11.0 M, `sort_paths` 8.4 M, `Resolver::listed` 7.0 M and `compare_components` 6.6 M. Item 2 removes the path work.
- **Off the critical path.** The 20 G report shows the thread done by 0.077 s, while the file walk runs to 0.146 s. So a cheaper reader would lower the instruction count, not the wall time (§10.12).

**Session load, 169.3 M, and session run, 32.9 M.**
- Both are path work: `Components::next` 42.8 M and `compare_components` 36.4 M.
- Most of it is the sort of the file list (`drift::sort::<PathBuf>`, 40.7 M of compares), plus `strip_prefix` and `starts_with`.
- Item 2 gives each file its path data once, from the walk, and sorts by it.

## 9. Costs that cross phases

### 9.1 Folded names (item 15)

- **Size.** In the warm run, `is_folded_eq` costs 402.7 M over 4,249,204 calls and `fold_hash` 298.0 M over 5,046,296 calls. Together that is 700.7 M, item 15's 701 M.
- **Callers.** Each of the largest callers is named in item 15's design (the figures are `is_folded_eq` only; the same callers' `fold_hash` adds 40-60% of them, for instance 49.8 M to the `ClassOwners` lookup, 25.1 M to `lookup_class` and 13.9 M to `resolve_function`):
  - the `ClassOwners` lookup, 80.3 M;
  - `lookup_class`, 40.1 M;
  - `ClassReflection` `contains_key`, 22.9 M;
  - `lookup_method_with_owner`, 19.0 M;
  - `resolve_function`, 15.2 M;
  - `substitute_class_templates`, 15.0 M;
  - `declared_method_name`, 14.0 M;
  - `record_member_usage_token` and `class_template_bindings_rec`.
- **Purity's share.** `resolve_transitive_purity`'s 9.6 M of folding belongs to item 4, as purity keys become ids.

### 9.2 Formatting

`format_inner` costs 254.4 M in the warm run, over 319,132 calls. By caller:
- **Item 14, ≈105 M.** These are memo keys and source strings:
  - the P11 tag memo's key in `tag_block`, 57.0 M;
  - `TypeNode::to_source_string` (P7), 30.5 M;
  - the grammar memo keys in `parse_tag_rest_shared`, 17.5 M.
- **Item 18's F1, ≈67 M.** These are messages formatted before anything is reported:
  - `check_stmt_impl`, 24.0 M, of which ≈11 M are the `@var` reports that E10 stops (§6);
  - `check_function_impl`, 20.9 M;
  - `check_class_generic_bounds`, 11.1 M;
  - `check_property_declarations`, 6.9 M;
  - `check_phpdoc_types_against_native` and `printf_placeholders`, ≈4 M.
- **Item 18's display work, 26.0 M.** `write_shape` 20.8 M and `quote_php_string` 5.2 M.
- **Reported, then dropped: 22.5 M (proposal 11).**
  - `report_missing_offset` formats 55 messages at ≈409 K Ir each. All of them are `offsetAccess.mightNotExist`: the only format with a cost is the one at `checker/assignment.rs:809`.
  - That message is level 7, and the run reports at the default level 6 (`DEFAULT_REPORT_LEVEL`, `project/session.rs:39`), so `finalize` drops 22 of the 55 (`session.rs:206`; B3's `level.txt`). The other 33 are discarded earlier by the report truncations (B3: 33 `offsetAccess.mightNotExist` among the discarded reports), so all 55 are formatted for nothing. Q-6 traced the 33 since: 30 are in a convergence pass's `rewind_to` and 3 in bodies that call `extract()`.
  - F1 does not reach them, because they are reported.
- **A key for an empty map: 6.6 M (proposal 11; 5.1 M of it in `format_inner`).** `ConstantResolver::class_constant_type` formats a lowercased `class::NAME` key on every class-constant read: 6,854 calls (`checker/constant_resolver.rs:13-18`, `:94`). It does this to probe `dynamic`, which is empty when no `dynamicConstantNames` are configured, as here.
- **Only in part by F4 and F6f, ≈13 M:**
  - `sqlparser`'s own formatting, 9.3 M, which F4 makes once per distinct query text;
  - the regex scanner, 3.7 M (N4). `scan_branch` calls `concat_literals` once per atom, one `format!` per product (`regex_groups.rs:390-401`): 6,988 calls inside 714 `parse_capture_groups` calls (11.7 M inclusive), 467 from `preg_matches_type` and 247 from `narrow_preg_outcome`. The analysed files hold 170 distinct patterns at 203 sites, so each site is scanned ≈3.5 times and each distinct pattern ≈4.2 times (714 scans). F6f's memo (one analysis per distinct text) removes ≈76% of the 11.7 M, and F6f's rewrite of the scanner ("scanning bytes and keeping slices") replaces these `String`s; a change of its own (E9) is not worth it.

### 9.3 Copies

- **Size.** In the warm run, `__memcpy_avx_unaligned_erms` is 515.8 M self over 15.57 M calls. §3.2's row "memory and UTF-8" (1.26 G) also holds:
  - `memcmp`, 206.5 M;
  - the `memchr` family, 277.1 M;
  - `memset`, 20.9 M;
  - `from_utf8` and `Utf8Chunks`, 240.5 M.
- **An unnamed caller.** The caller at `0x40de400` (110.5 M, 1,419,399 calls) is a PLT stub that only mimalloc's `_mi_theap_realloc_zero` calls. So that memcpy is a reallocation copying a growing buffer.
- **Growth as a whole.** `RawVecInner::finish_grow` costs 740.1 M inclusive over 5.34 M calls. Its largest callers:
  - `reserve` from `String`'s `fmt::Write`, 160.0 M over 1.22 M calls: formatting (items 14, 15, 18);
  - the parser's `ArrayItem`, `Stmt` and `Arg` vectors, 137 M (item 9);
  - `parse_qualified_name`, 48.1 M (items 12, 15);
  - `Path::join` and `push`, 42.1 M (item 2);
  - `docblock::parse`, 31.9 M (item 14).
- **The other memcpy callers** each fall in an item:
  - `from_def` (items 4, 8, 14);
  - `String::clone` (item 15);
  - the parser (item 9);
  - `imbl`'s `make_mut` (item 18);
  - `Arc<Lazy<FunctionReflection>>::make_mut` (item 4's purity copies).

  No single site is large enough to stand alone.

### 9.4 The allocator

- **Coverage.** M10 of the 20 G report gives every one of the dhat run's 19.4 M allocations to an item, and expects about 1.35 M to remain. It counts each reallocation as a block, charged to the site that first allocated it. The heap drill of these three runs (`PROFILE_DRILL_PIWIGO_OFF_HEAP.md`) maps every allocation of the off and cold runs too.
- **What that means here.** The allocator's 1.75 G in the warm run is the cost of those allocations, so the items lower it as they remove them. Its memory-bound share weighs more in wall time than in instructions.

### 9.5 Decoding stub entries (warm run; off and cold runs only with proposal 6)

- **Size.** In the warm run, `decode_entry` costs 439.0 M inclusive: 301.3 M for 39,914 functions and 137.7 M for 3,949 classes. By attribution the decoding block is 477.8 M: 275.5 M in the finishing passes (275.1 M once the name-table lookups are carved out first, as in §4.4 and §7) and 202.3 M in the check.
- **Who triggers the function decodes.** 264.1 M of the function decodes (269.9 M through the `OnceLock::initialize` wrapper) are made on a `Lazy`'s first read. By caller of the wrapper (178.0 + 88.6 + 3.1 M, plus 0.2 M for `flatten_phpdoc_inheritance_one_pass` and 0.1 M for `into_inner`):
  - **`resolve_deferred_value_of`**: 178.0 M for 22,746 methods. Item 6, step 1 removes it.
  - **`Members::eq` in `changed_since`** (`registry.rs:641-679`): 88.6 M for 12,106 methods. Through `Lazy`'s `PartialEq` (`name_table.rs:1185-1189`), it decodes both sides to compare members whose pointers differ. Item 4 removes it (F3 step 4).
  - **`flatten_traits_one_pass`**: 3.1 M.
- **What stays:**
  - the 137.7 M of class decodes, from lookups the run makes (`load_base`);
  - 37.2 M of function decodes through `load_base`.

  Item 6, steps 3 and 5 make each decode cheaper, not fewer: ≈70 M of name re-interning and 35 M of UTF-8 checks across all decodes.
- **Interning the decoded types (E8).** `Type`'s `Deserialize` interns every node of every type it decodes: 416,975 `Type::new` calls, 93.5 M, ≈224 each. The whole warm run makes only 126,748 new nodes, so at least 69.6% (about 70%) of these find a node that exists. E8 stores each cache file's distinct types once, in a table the records index, under item 6 step 5's decode context: ≈ −65 M today, ≈ −25 M once item 6 step 1 stops decoding the 22,746 methods.
- **With proposal 6**, the off and cold runs decode as the warm run does.

### 9.6 The type relation and the interner (N2)

Both are already lean per call; their cost is in how often they are called. Warm run:
- **The relation.** `is_super_type_of`: 1,341,821 calls, 470.3 M inclusive, 141.3 M self (≈105 per call).
  - The shortcuts at its top (`super_type.rs:70-84`) answer 450,410 calls (34%). Their tests (`is_scalar_value`, `scalar_family`, the `mixed` and `never` matches) cost ≈46 M, ≈34 per call, because each one loads the node's `Repr` again and matches it.
  - Structural calls (no registry): 420,653. The memo answers 276,787 (66%); 143,866 are computed.
  - Registry-backed calls: 470,758, all computed, with no memo. That is item 7's subject.
  - `is_super_type_of_by_form`: 614,624 calls, 316.6 M inclusive, 77% of them registry-backed.
- **The interner.** 1,676,428 inserts (owned 1,335,586, borrowed 340,842), 521.2 M inclusive; 126,748 new nodes (7.6%). Per insert ≈311: hashing ≈55 (`exact_hash` 92.9 M), comparing ≈77 (the front table's 1,477,351 compares 123.1 M, ≈83 per compare, plus the shards' 72,501 compares 6.3 M), the insert's own ≈105 (176.8 M), dropping the owned values that hit ≈41 (68.7 M) and ≈33 of other callees (grow, `semantic_hash`, allocation). The hash is already shallow: each child's hash is read, not walked. Dropping the owned values that hit (1,255,518) costs 68.7 M. The largest parts of the check's intern block (369.9 M, 13 lines) are the insert 113.7 M, `exact_hash` 59.3 M, `exact_eq` 49.0 M, dropping `Repr` 34.8 M, `intern` 32.1 M and `is_form_of` 25.6 M. `Type::new`'s largest callers are the stub cache's decoding (416,975 calls, 93.5 M; E8, §9.5), `Type::object` (170,696, 57.6 M), `int_literal` (71,550, 23.8 M) and `bind_self` (64,671).
- **Three cuts** (§10.13):
  - **E5.** A kind code in the node's 5 free flag bits (11 of 16 are used, `types/src/lib.rs:154-173`), so the shortcuts read one word per type: ≈34 to ≈10 per call, ≈ −30 M.
  - **E6.** Answer one node against itself with Yes before the structural memo and the registry-backed forms. B7's probe counted the operands: of 1,479,785 calls on Piwigo, 203,478 (13.8%) are one node against itself, and 164,144 of those pass the shortcuts (67,631 structural, 96,513 registry-backed). Equal nodes that are not one are 0.6%, so identity is the right test. ≈ −12-13 M, less ≈2 M for the test: ≈ −10 M net.

    | `is_super_type_of`, Piwigo, off mode (B7) | calls | of all |
    |---|---:|---:|
    | one node against itself | 203,478 | 13.8% |
    | … answered by the shortcuts | 39,334 | 2.7% |
    | … past them, structural (the memo) | 67,631 | 4.6% |
    | … past them, registry-backed | 96,513 | 6.5% |
    | equal nodes that are not one | 8,713 | 0.6% |
    | … past the shortcuts | 820 | 0.06% |

    The probe's 1,479,785 calls are 1.9% more than the off profile's 1,451,853 with the same code: it counts every entry, inlined or not, while callgrind counts only calls into the function's own code, in a build of another profile. The check asks 1,156,025 of them. Testing `==` past the shortcuts would catch 820 more pairs, and on equal hashes it compares the forms, which costs more than it saves. The test itself runs on each call that passes the shortcuts, ≈0.9-1.0 M (891,411 in the profile), at ≈2 each.
  - **E7.** Of `Type::object`'s calls, 125,364 (73%) build a class's plain object type (B8): `resolve_name_expr` 95,861, `record_call_throw_point` 16,133 (always `Throwable`), `instance_of_any` 13,370 (a fixed list). The type kept with the class's declaration and two statics save ≈35 M. 92% of the analysed files' integer literals are 0-255, so a table of −1..=255 saves ≈20 M of `int_literal`. ≈ −55 M.

## 10. Proposals

Proposals 1-6 concern the stub build, which only an off or a cold run pays. A cold run is the first run of each build, and every run without a persistent cache. Proposals 7, 9, 10 and 11 reach every run, and proposal 8 reaches the runs whose per-file cache is cold or off. None of them is in the 20 G plan, and none is implemented.

**When to build 1-5 (D7, recommended).** Now, in four steps:
1. **E1's fix** (§10.14). It fixes a deadlock that is in today's code, so it is needed whatever proposal 6 does. It carries proposals 1 and 5: its S4 chooses the base at `expect_files`, and its S2 builds each layer on a thread of its own.
2. **D5's write check.** Proposal 2 is that check applied to the stub cache.
3. **Proposal 3.**
4. **Proposal 4.**

Each step is measured with paired cold runs; B4 gives them their wall-time weight, a ceiling of the 0.37 s the warm stub cache saves today. The reason to build them now: proposal 6 waits on the crate split's reflection crate (stage C10), and the split's C4 and C5 replace the eager stub build with lazy, per-file parsing before that, so the code of proposals 1-5 goes with the eager build. Their whole value is the cold runs between now and C5. Until C5, C4 keeps an eager pass over each stub file. The split has finished A0-A2, B1-B9 and C1-C3 (`gates/module-graph.txt:228` on master), so C10 is far off. Until then every cold run pays the build: the development loop once per edit, since the stub cache is keyed by the build's source hash; CI's cold jobs; and the first language-server start, which `tests/lsp_startup.rs` on master holds to 10 s from an empty stub cache as well as a warm one. D5's write check outlives proposals 1-5, since it guards the per-file cache and the verdict cache, which stay. Proposal 6's form does not survive C4 and C5 either: what is left to carry is each stub file's declarations, worth ≈0.2-1.5 G per cold run instead of 5.05 G (§10.6, C8).

### 10.1 Resolve only the stub base the files need

- **Now.**
  - `Session::load` calls `Project::new()` (`session.rs:733`), which clones `Project::plain_base()` (`project.rs:532-541`).
  - After the parse, `expect_files` (`session.rs:791`, `project.rs:1212-1222`) calls `switch_base(Project::phpstan_base().clone())` when any parsed file mentions PHPStan.
  - From then on the project never reads the plain base. The base with PHPStan's source is built from all the stub data (`stubs.rs:384`), not from the plain base.
- **Design.** E1's S4 (§10.14).
  - The project starts with no base and takes the one its files need at `expect_files`.
  - Nothing before that point reads the registry. The parse's `index_use_sites` reads the settings, `retain_source` and `member_index_built` (`project.rs:1106-1129`), and `note_owner` reads the packages (`:1134`).
  - `switch_base` already carries over the PHP versions and the configured assertions (`registry.rs:774-781`).
- **Saving.**
  - 355.1 M on the main thread: the plain base's merge, freeze and stores. The corpus build moves into the other base and is not saved.
  - The plain base's finishing run on the pool, ≈35-52 M, estimated. C4 showed that the 117.2 M run the drill first named is PHPStan's base's, which stays (§4.4).
  - In wall time, the plain base's part of the ≈0.25 s serial stretch before the parse (B4).
  - The warm run loads the plain base in 4.9 M, so it gains nothing to speak of.
- **Who gains.** Any project whose loaded files mention PHPStan's namespace. Composer's vendor context counts, so a project with a PHPStan extension under `vendor/` gains: eight of the ten projects C8 read need phpstan-src, and Piwigo's own files mention it.
- **Adversarial checks.**
  - A file that mentions PHPStan only through a skimmed body (proposal 8) must still choose PHPStan's base. Proposal 8's skimmer flags the name.
  - A project with no file that mentions PHPStan resolves the plain base as it does now, at `expect_files` instead of at `Project::new()`.

### 10.2 Serialize only what a store keeps

- **Design.** Gate `stub_cache::store`, `StubIndex::store_cached`, `Registry::store_cached` and the stub-data store on whether the cache can be written, before encoding. The per-file cache already skips encoding with no directory: `file_cache::key` returns `None` (`project/file_cache.rs:164-165`, `directory()` at `:62-72`).
- **Saving.**
  - 1,005.3 M serial on the main thread (§4.3), on off runs only.
  - **After proposal 1: ≈0.71-0.74 G (C3).** Proposals 1 and 2 overlap by the plain base's stores. The drill bounded them at 0.26-0.36 G of its 355.1 M, from the bases' other work (186.3 M, of which the plain base, a subset of PHPStan's, has at most half). The file sizes (5.27 MB against 15.33 MB) would have put them at about a quarter of 814.9 M, below that bound. C3 parsed both files' tables and found why:
    - **Not deduplication.** `Writer::entry` writes equal entries once (`container.rs:92-109`), but the plain base's share of what was encoded (6.62 of 25.44 MB, 26.0%) is its share of the file bytes too.
    - **What each base holds:**

      | tables | plain: records, bytes encoded | PHPStan's: records, bytes encoded | store cost in the profile |
      |---|---|---|---:|
      | `functions`, `raw.functions` | 10,546, 1.30 MB | 10,564, 1.30 MB | `NameTable<FunctionReflection>::store` 137.8 M |
      | `classes`, `raw.classes` | 3,084, 4.40 MB | 6,990, 14.69 MB | `NameTable<ClassReflection>::store` 489.6 M |
      | the other 13 tables and sets | 26,753, 0.92 MB | 36,720, 2.83 MB | 185.2 M |

      The plain base encodes 6.62 MB of entries (3.81 MB distinct) and PHPStan's 18.82 MB (13.45 MB distinct), read from the containers' section directories and 16-byte records (`container.rs:120-136`, `name_table.rs:1028-1066`).
    - **The function tables.** They are the same in both bases (5,273 against 5,282 functions, 1.30 MB each), and they cost ≈53 Ir per byte against ≈26 for the class tables. Half of `NameTable<FunctionReflection>::store`'s 137.8 M belongs to the plain base, which holds only a quarter of all the bytes. The 8,259 global constants are in both bases too, and the name-only tables cost per record.
    - So the plain base's stores are 68.9 M (functions) + 112.9-149.9 M (classes) + 45.4-78.0 M (the others) + 0.6 M ≈ 0.23-0.30 G, and with the bound ≈0.26-0.30 G.
- **What is lost.**
  - `NameTable::store` reads the entries, and for a built entry `Lazy`'s `Serialize` encodes the value without caching it (`name_table.rs:1192-1203`).
  - `Writer` keeps nothing beyond the dropped container.
  - One thing does go: `store_entries` checks that a table holds no name twice (`name_table.rs:1043`). With the gate, an off run no longer prints "can't be stored" for such a table. A cold run still checks it.
- **How narrow `cache_dir()` is.**
  - It is `None` only when `PHPSTAN_RS_NO_STUB_CACHE` is set, or when `HOME` and `XDG_CACHE_HOME` are both unset (`stub_cache.rs:33-44`).
  - Nothing in the repository sets the variable: not `xtask`, not `.cargo/config.toml`, not the tests. `tests/file_cache.rs:31` removes it.
  - A directory that exists but cannot be written still pays the encoding. Hence the decision below.
- **Decided: each cache checks once whether it can write, apart from reading (D5).**
  - **The check.** At its first store, a cache does what a store does: it creates the directory, then creates and removes a temporary file there. It keeps the result in a `OnceLock` of its own. A run that stores nothing, such as a fully warm one, never checks.
  - **Reading stays on.** A read-only cache, such as one a CI job restores, is still read. Turning the cache off would lose that.
  - **What the gate skips.** Every step a store takes before the write. For the per-file cache that starts with building the entry (`CachedFile::of`, `session.rs:765-771`), not only its encoding.
  - **The shard directories.** The check also makes the per-file cache's 256 shards, once (E13), so a run that stores nothing touches nothing on disk.
  - **What it reports.** One line when the cache cannot be written, and one when a later write fails. A full disk, or one of the 256 shard directories (`file_cache.rs:177-179`) that cannot be written, gives the latter. Both fail silently today.
  - **Reads write too.** A load refreshes the mtime of an entry older than 12 hours (`file_cache.rs:190-196`), which also fails silently in a read-only directory. That stays harmless.
  - **Which caches.** The per-file cache, the stub cache until proposal 6 or the crate split's C5 removes it, and proposal 8's parse-verdict cache. The check outlives proposals 1-5; only its part for the stub cache, proposal 2, goes with the eager build.
  - **Stating intent.** A cache that is read-only on purpose prints that line on every run. A setting that declares it read-only would silence the line, and would make the intent explicit.

### 10.3 A hash index for stub bases built in memory

- **Now.**
  - `Base::find` (`name_table.rs:395-410`) uses the stored hash index only for `Keys::Stored`.
  - A base built in memory and then frozen (`Keys::Built`) is searched by bisection, each step a case-folding compare.
  - `find_by` on a built base lowercases the key first (`:434`).
- **Cost.** In the off run, `Base<Option<u32>>::find` costs 386.8 M inclusive over 392,487 calls, 985 Ir each. In the warm run it costs 63.0 M over 261,651 calls, 241 Ir each. Its callers:
  - `NameTable<()>::contains_key::<str>`, 84.9 M;
  - `NameTable<FunctionReflection>::base_get_by::<str>`, 64.7 M;
  - `Base<ClassReflection>::find_by`, 31.0 M;
  - the `forget` calls of `reset_scoped`.
- **Design.** `freeze` builds the index that `store_entries` already builds (`name_table.rs:1057-1066`).
- **Saving.**
  - At the warm run's cost per call, the off run's calls would cost 94.6 M: a saving of ≈0.29 G.
  - 238.5 M of the lookups' subtrees are in the finishing passes, mostly on their thread, so the gain in wall time is larger than the gain in instructions.
  - The bisection was carved out of the finishing blocks before the stub bases' runs were summed, so C5's correction does not touch this figure.
  - With proposal 6, or once the crate split builds no bases, this gain goes, except for builds that carry no cache.

### 10.4 Look each stub file's dependents up once in `build_closures`

- **Now.** `build_closures` (`stubs.rs:889-912`, inlined into `build_generated_stub_data`) runs a depth-first search from each of the 2,500 stub files that declare something.
  - Each file the search reaches has each of its keys looked up again in the `dependents` table (`:898`).
  - That is 893,154 `Packed::get` calls costing 170.6 M, and the lines around them (`:897`, `:900-902`) add 63.9 M.
- **Design.**
  - Turn each file's keys into a list of dependent files once: about 2,500 lists. The searches then make no lookups.
  - Compute the closures in the order of the dependency graph's strongly connected components, so that no search is repeated.
- **Saving.** At least 0.2 G of the 234.5 M, serial on the main thread. The result is stored in the stub index, so a warm run does not pay this.

### 10.5 Build the stub corpus beside the parse

- **Now.** The stub build runs before the parse (§1). While the main thread does its serial stub work, every other core is idle.
- **Design.** E1's S2 and S3 (§10.14).
  - The corpus, which both bases need, starts before the files are listed, on a thread of its own, as `Project::warm_stubs()` (`project.rs:549-561`) already does for the fixtures (`tests/fixtures.rs:36`). `check-project` never calls it.
  - Its `par_iter` moves into `worker_pool().install`, so the corpus and the parse share the 8 cores instead of running 16 threads on them.
  - The base is resolved after the parse, once its files say which one (proposal 1).
- **Saving.**
  - Running a serial stretch S beside a parallel phase P that keeps every core busy takes about P + S/8 of wall time, against P + S one after the other. So it saves ≈7/8 of S, however busy the parse is.
  - Here S is the corpus's merge and stores, 0.62 G today, less what proposals 2 and 4 remove. B4's ≈0.25 s stretch before the parse is the corpus plus the plain base. The corpus is about 64% of its Ir (622.6 of 977.7 M), so proposal 5 alone hides about 7/8 × 0.16 s ≈ 0.14 s of it. The plain base's part (355.1 M, ≈0.09 s) is removed by proposal 1, so proposals 1 and 5 together take the whole stretch.
  - The corpus's parallel 3.5 G gains nothing beside the parse, since both are parallel.
  - The base's serial work stays on the critical path: 646.1 M for PHPStan's base (≈0.17 s, B4), or 355.1 M for the plain one.
  - Instruction counts do not change.
- **Adversarial checks.**
  - **Deadlock through `install` (C2).** A thread that waits inside rayon runs other jobs while it waits: a worker in a join steals from its pool's deques (`rayon-core-1.13.0`, `registry.rs:780-842`), and an `install` into another pool from a worker (`in_worker_cross`, `:543-563`) waits the same way. Only a thread that belongs to no pool blocks without running anything. So a worker that forces the corpus can steal a job that needs the corpus's own `LazyLock`, and wait on itself. That is already possible in today's code, without proposal 5 (E1, §10.14). Proposal 5's `install` adds a third path of the same kind and removes none, so proposal 5 is safe once E1's fix is in.
  - **Thread limits.** The global pool ignores `PHPSTAN_RS_THREADS` and the 64 MB stack (`project.rs:355`). Moving onto the worker pool fixes both.

### 10.6 Build the stub data at compile time, into every binary

- **Status.** Decided on 2026-10-13 (D3): built at compile time by a build script, and embedded. Reopened by C8: the crate split leaves no bases to embed, so the decision is taken again once its C5 sets the version key, on the numbers below. **Proposal 6 is not built in today's form.**
- **What it removes today.** The whole difference between the off and warm runs: 5,053.0 M (§4), 0.37 s of wall (B4). That includes the stub sources, the serial stub work on the main thread, the stub bases' finishing runs, and the bisection in built bases.
- **Why it is sound.** The cache's key is the build's source hash alone (`registry/stub_cache.rs:17`, `:46-48`). No configuration and no PHP version goes into it, and `switch_base` applies the PHP versions and the configured assertions after the base is loaded (`registry.rs:774-781`). The stub build reads no environment variable except the cache's location (`stub_cache.rs:33-44`) and the pools' sizes (`PHPSTAN_RS_THREADS`, `project.rs:364`, and rayon's own), which do not change the data (B10). So what a cold run writes is a function of the build, and the build can carry it. One input is missing from the hash today: the `.stub` files of `vendor/phpstan-src/stubs`, which the stub build reads (E15, §10.14).
- **What would be carried.** Four files, 31.35 MB at this commit:

  | file | raw | `zstd -19` | `gzip -9` |
  |---|---:|---:|---:|
  | `stub-base` | 5.27 MB | 0.75 MB | 1.17 MB |
  | `stub-base-phpstan` | 15.33 MB | 1.45 MB | 2.24 MB |
  | `stub-data` | 8.71 MB | 0.99 MB | 1.46 MB |
  | `stub-index` | 2.04 MB | 0.25 MB | 0.38 MB |
  | **total** | **31.35 MB** | **3.44 MB** | **5.24 MB** |

  Their entries are read and decoded lazily (`container::Reader`), so a run decodes only what it reads, as a warm run does now.
- **Three ways to carry it**, each with the same effect at run time:
  1. **Built by a build script and embedded.** The stub build runs at compile time, and the binary holds its output as a static.
  2. **Appended to the binary by the release step.** The release step runs the built binary once to write the cache, then appends the files to the executable behind a trailer that the binary finds through `current_exe()`. On macOS the append must come before signing.
  3. **Packaged beside the binary.** The release step writes the files next to the executable, and `cache_dir()` falls back to that directory.
- **Why option 1.** It is the only way every binary carries the data:
  - release binaries;
  - binaries built from source, which is how the README says to build one (the crate is not published: `publish = false`), so D4 needs nothing of its own;
  - test binaries;
  - every way the VS Code extension finds a binary: `serverPath`, `PATH`, or one it downloads (`editors/vscode/README.md:37-41`).

  Options 2 and 3 reach release archives only. With option 3, each of the extension's ways to a binary would also have to carry the files. Since the data is a function of the source, the build that compiles the source makes it. Then no run is cold, and the disk stub cache, its key and its pruning go.
- **What it takes.**
  - **The crate split's reflection layer.** A build script cannot call the crate it builds, and it must run the very code the runtime runs. A stub base is built through `Project::finish_loading` → `run_finishing_passes` (`project.rs:1387-1399`). The registry uses the checker (`registry/init_flow.rs:13`), the finishing passes live in `project.rs`, and two of them, purity and global variables, are modules the module graph assigns to `analyser` (`gates/module-graph.txt:78-79`). So the data can be built only once the registry, its finishing passes and what they read live in a crate below the checker (`CRATE_SPLIT_PLAN.md`'s C10), and today's bases not even then (below).
  - **A small crate that embeds the data.** Its build script depends on the reflection crate, and its `include_bytes!` holds the 31 MB. In a crate of its own, the data is embedded again only when it changes, not on every incremental build of the main crate. It cannot be a dependency of the reflection crate, whose code its build script runs: cargo rejects the cycle (C8). So a crate above reflection hands the data in, and reflection's own tests build the stubs at run time.
  - **An optimized build-dependency.** Build dependencies compile at opt-level 0 unless `[profile.*.build-override]` raises it.
  - **A borrowed `Reader`.** `Reader::new` takes an owned `Vec<u8>`, and the embedded data is a `&'static [u8]`.
  - **A guard that the data equals a runtime build on every release target (B10).** Two release legs cross-compile (`release.yml`): `aarch64-unknown-linux-gnu` on an x86_64 runner, and `x86_64-apple-darwin` on an arm64 one, so the build script runs on another architecture than the binary. Constant folding of `**` calls the platform's `pow` (`types/src/arithmetic.rs:715-717`), whose precision Rust documents as varying by platform.
    - **Equal today.** The stub build calls `powf` 0 times, and so does Piwigo's whole off run: the stub sources' four `**` sit in `IntegerRangeType`'s exponentiation (`phpstan-src/src/Type/IntegerRangeType.php:684-704`), on method results that have no literal value to fold.
    - **Nothing else reads the platform.** Folding's other float operations are exact under IEEE 754, `%` folds on integers, `rem_euclid` (`coercion.rs:329`) is exact, floats are parsed and formatted by Rust's code, `folded_hash` (`syntax/src/name.rs:997`) is a fixed 64-bit function over little-endian words, and all five release targets are 64-bit and little-endian. The one target-dependent value, `MAIN_SEPARATOR` (`checker/includes.rs:214`), matters only on the Windows leg, which is native.
    - **Nor scheduling, nor optimization.** The four files are byte-identical across 1, 2, 3 and 8 threads, one rayon thread, one CPU, and opt-levels 0, 1 and 3. The merge runs in file order (`registry/stubs.rs:1479-1480`).
    - **The guard.** A host-side test that builds the stubs and fails if a fold used a `powf` result keeps it so; it fails the day a stub brings a float power, such as `2 ** 0.5`. Or `**` folds with a pow written in Rust from basic operations, such as the `libm` crate's (to be checked for arch-specific paths), which makes the equality hold by construction, at run time too. A backstop runs each leg's binary under qemu-user (aarch64) or Rosetta 2 (`x86_64-apple-darwin`) and compares a runtime build with the embedded data. Nothing ran on aarch64 or macOS for this report.
- **Costs.**
  - **Size.** The data is a static: `.rodata` grows from 18.2 MB, most of it the embedded stub sources, to ≈50 MB. That applies to the release binary and to each test binary that links it. A run loads only the pages it touches, and a release archive (gzip) compresses the data to ≈5.2-5.3 MB (≈3.4 MB only with zstd -19).
  - **Compile time (B9).** Measured with today's `phpstan-rs` library standing in for the reflection crate, an upper bound:
    - **A clean build.** The opt-level-1 host build of the whole library (release with `opt-level = 1`, `codegen-units = 256`, no LTO, incremental) takes 2 min 44 s of wall and 994 s of user CPU, alone on 8 cores; its units sum to 721 s. The reflection crate's closure ends its last unit at 80 s, so its critical path is ≈80 s plus the crate itself. None of `sqlparser`, the SQL drivers, aws-lc, lsp-types or clap is in it; those are most of the rest.
    - **Cargo's unit sharing**, tested on a three-crate workspace (`b9/share/`) whose middle crate is both a normal and a build dependency, counted from `cargo build -v`:

      | profile | override | compiles of the shared crate |
      |---|---|---:|
      | test | none | 2 |
      | test | `opt-level = 1`, `debug = true` (equal to the profile) | 1 |
      | test | the same, plus `codegen-units = 256` | 2 |
      | release (thin LTO) | none | 2 |
      | release (thin LTO) | `opt-level = 3`, `codegen-units = 1` | 1 |
      | gate | `opt-level = 3`, `codegen-units = 16` | 1 |
      | test, with `--target` | equal | 2 |
    - **A second compile only on the release legs.** Cargo builds a crate that is both a normal and a build dependency once, when no `--target` is passed and the override's settings equal the profile's, an unset field included. So the override should equal the profile in test, gate and release: then `cargo test` (the development loop and `rust-ci.yml`), the gate profile and a native `cargo build --release` compile it once, with the build script at that profile's opt-level. Every leg of `release.yml` passes `--target` (`release.yml:101`) and compiles the reflection crate's closure again for the host: 101 units and 162 s (its closure of 86 units and 139 s, plus `composer` and `config`, which the module graph lets reflection use), plus at most the main crate's 54 s, ≈160-215 s of units (≈2.7-3.6 min, 22-30% of the units), beside the target build (9 min 36 s for the gate profile on this machine, load not logged). These units were measured at opt-level 1, codegen-units 256 and no LTO, so they are a lower bound for a release override equal to the release profile (opt-level 3, codegen-units 1).
    - **The stub build in the build script,** all four containers, timed as an off run less a stub-warm run on a two-file project naming a `PHPStan\` class: 3.67 s of wall and 12.2 s of CPU at opt-level 0, 0.90 s and 2.5 s at 1, 0.59 s and 1.8 s at 3, with byte-identical output. Opt-level 1 is 4.1× faster than 0, within the test profile's comment of 4-6×, and 1.5× slower than 3. Dev's opt-level is 0, so it chooses between 3.7 s per rerun and a second compile at opt-level 1.
    - **The embedding:** 0.3-1.0 s to compile the data crate in release (0.97 s the first time, +0.3-0.4 s on a rebuild), and a thin-LTO relink of 2.79 s against 2.73 s. These timings were read off the console and not kept.
    - **An edit to the reflection crate** (26 s of host rebuild after a one-line edit, 87% of it codegen) adds a serial stretch: the build script needs reflection's rlib, not just its metadata, so the data crate waits for reflection's codegen (22 s here), then the build script's link, the stub build and the data crate. That is up to ≈20 s of lost pipelining, unless only the binary crate, its tests and the crates that test the data take the data crate, so the libraries above reflection compile as they do today. With the unit not shared (`--target`), the host rebuild runs beside the target one. At the opt-level 1 the 26 s was measured at, it would end first; with a release override equal to the profile, both sides are at opt-level 3 and that is not so. A comment added in `types` takes 30 s to rebuild. An edit to any other crate reruns nothing, whereas today every edit makes the next run cold.
    - **Under C8's form** the build script parses every stub file and builds no base: by §4.1's split, ≈3.50 G of today's 5.05 G. Not measured.
- **What stays.**
  - `PHPSTAN_RS_NO_STUB_CACHE` comes to mean "build the stubs at run time", so the off mode still tests the build path.
  - Proposals 1-5 then speed only the off mode and the reflection crate's own tests, and proposal 4 also the build script.
  - E1's hazard narrows to those runs and to the build script, which runs on a thread of no pool. It does not go away.
- **Adversarial checks.**
  - **A stale embedding.** Cargo reruns a build script when one of its build-dependencies is rebuilt, and the stub sources reach it through `stub-sources`. The equality test catches whatever that misses.
  - **A feature that changes the data.** Build-dependencies resolve their features apart from normal ones. The registry, `types` and `phpdoc` have no feature gates. `syntax`'s `fuzzing` only derives `Arbitrary`, and `pcre-validate` switches only `pcre_validate.rs`. The equality test covers the rest.
  - **Inputs a build script cannot know.** There are none: no setting, and no environment variable besides the cache's location and the pools' sizes (above).
- **Against the crate split: the form does not survive (C8).** Read from master's `CRATE_SPLIT_PLAN.md` and its checked-in stub index, with the names ten projects' files reference (PHP's tokenizer: calls, class names in code and PHPDoc, constants, resolved per file):
  - **No bases are left to embed.** C4 resolves each class lazily through five memo levels ("no stub class resolves unless something reaches it"), and keeps an eager pass over each stub file until C5. C5's stub layer is process-wide, keyed by PHP version, and parses a stub file when something reaches it; the checked-in index (the split's B9, done on master: 1.66 MB of text, 17,051 symbols, written by `cargo xtask stub-index`) answers which file declares a name. Today's `stub-base` and `stub-base-phpstan`, 20.6 of the 31.35 MB, are registries after the finishing passes over every stub class, and nothing builds them once C4 lands. C4 also restricts purity classification to analysed and context declarations, and C9 takes purity and global-variable inference out of reflection ("Stubs get no global-variable summaries. Builtin purity comes from `functionMetadata`."), so a build script on C10's reflection crate could not build today's bases either. What is left to embed is each stub file's declarations before resolution, decoded when the file is first reached: what the per-file cache stores for a context file today (its key already has a stub mode, `is_stub`). Master does not read the checked-in index at run time yet: its run-time index is still built from the sources or loaded from the stub cache (`registry/stubs.rs:1482-1500`). The plan never mentions the stub cache, which came on 2026-10-01 (`d1fe66ca`), and C5's version key is "decided first" with no entry in its Appendix F yet.
  - **The saving falls to the parse of the files a run reaches.** All stub sources are 16.34 MB: phpstorm-stubs 9.41 MB and phpstan-src's `src` 6.93 MB. A reached symbol brings in its declaring files; the lower bound closes over the index's ancestors, the upper also over every class name the reached stub files mention:

    | project | analysed files | context files | phpstan-src | analysed files alone, lower | with context, lower | with context, upper |
    |---|---:|---:|---|---:|---:|---:|
    | amphp | 26 | 26 | no | 0.51 MB (3.1%) | 0.98 MB (6.0%) | 1.58 MB (9.6%) |
    | guzzle | 70 | 99 | no | 1.46 MB (8.9%) | 1.47 MB (9.0%) | 1.66 MB (10.1%) |
    | monolog | 125 | 7,651 | yes | 1.84 MB (11.3%) | 3.93 MB (24.1%) | 7.28 MB (44.5%) |
    | league-flysystem | 173 | 8,975 | yes | 1.13 MB (6.9%) | 4.23 MB (25.9%) | 6.60 MB (40.4%) |
    | doctrine-orm | 471 | 3,336 | yes | 1.37 MB (8.4%) | 4.13 MB (25.3%) | 6.96 MB (42.6%) |
    | composer | 308 | 658 | yes | 2.24 MB (13.7%) | 3.51 MB (21.5%) | 6.22 MB (38.1%) |
    | laravel | 1,700 | 12,405 | yes | 2.61 MB (16.0%) | 5.15 MB (31.5%) | 8.05 MB (49.3%) |
    | symfony | 11,748 | 7,386 | yes | 4.57 MB (27.9%) | 5.47 MB (33.4%) | 5.67 MB (34.7%) |
    | psalm | 1,196 | 1,361 | yes | 1.90 MB (11.6%) | 3.29 MB (20.2%) | 5.65 MB (34.6%) |
    | Piwigo | 2,792 | 11,456 | yes | 3.11 MB (19.0%) | 7.07 MB (43.2%) | 9.40 MB (57.5%) |

    C5's composer locator loads context classes on demand, so a run reaches between the "alone" and the "with context" columns. Bytes run ahead of classes (2-18% of the classes at the lower bounds, 3-43% of the bytes), because a phpstorm-stubs file holds a whole extension: one `strlen()` parses `Core/Core.php`, and the standard extension's 552 functions sit in twelve `standard_*.php` files (0.45 MB), eleven of which even amphp reaches. At ≈0.21 G per MB (§4.1; phpstan-src's files are mostly bodies and phpstorm-stubs' mostly doc comments, so the rate varies), that is ≈0.2-1.5 G per cold run at the lower bounds with context and ≈0.3-2.0 G at the upper bounds, against 5,053 M today; for the analysed files alone, 0.1-1.0 G. On Piwigo, B4's stretch of the stub sources (3.50 G in ≈0.2 s on ≈4 cores, under perf) puts 1.5 G at ≈0.1 s of wall if it is parsed as widely, against the 0.37 s the stub build costs today; a file parsed on demand is parsed by the thread that reaches it, so it may be less parallel. Resolution is paid in memory either way, because the embedded form holds no resolved class, and the merge and the stores (0.62 G) go with the bases and the index, with or without proposal 6.

    The method (`~/.cache/prs-probe/c8/`): `scripts/names.php` lists each file's references; `scripts/reach.py` closes them over `crates/stub-sources/index.tsv`; `scripts/context.php` lists the context files as `composer/src/vendor.rs` discovers them, with the package's autoload directories as the analysed files (Piwigo's are those `phpstan-rs.toml` names). `context.php` also walks PSR-0 directories, which `vendor.rs` skips when `autoload_static.php` exists, as in all ten; monolog's row is recomputed without the 32 `doctrine/couchdb` files that adds. Each list is within three files of what the analyser loads.
  - **The version key decides whether the data is still a function of the source.** 90 phpstorm-stubs files carry version gates, 3.45 MB: 712 `PhpStormStubsElementAvailable` and 2,492 `LanguageLevelTypeAware` attributes, counting the aliases `stub_attributes` resolves (88 files without them, as the plan counts). Over the 15 versions the analyser knows (5.3-8.5) they have 388 distinct readings, 2 to 10 per file. Parsing per version, as C5 is written, embeds 19.27 MB of source instead of 3.45 MB, ≈2.7× the phpstorm-stubs part. Keeping the gates as data, lexed as conditions and resolved by the configured range, keeps one parse per file, answers a range with every reading in it, and keeps the data a function of the source, so the per-file cache needs no version in its key. The index's format already gives each declaration a gate (`@from-to`). That is the more proper form, and C5's decision should be made with it in view.
  - **How it would be carried.** Checked in, as the index is, it is unfit: the declarations are binary and change with every edit to the declaration model or its encoding. A build script still reaches every binary. Without embedding, the per-file cache does most of the job: its key holds the build's source hash today, and after the split a key over the crates the declarations depend on (`reflection`, `syntax`, `types`, `phpdoc`, `stub-sources`, `composer`, `config`, `diagnostics`, `semantics`, with the manifest, `Cargo.lock` and the target) keeps the entries across other edits. §10.12 rejected a narrower key because today's stub build reads most of the main crate, so a narrower key is easy to get wrong, and a missed input reads a stale base with no error (E15: today's key already misses one). A binary built from source then pays one cold run per change to those crates, not per build (D4).
  - **So** the costs (B9's compile time, B10's guard, `.rodata`) stay while the saving shrinks to ≈0.2-1.5 G.

### 10.7 Tokenize a PHPDoc tag's description only when it is read

- **Now.**
  - `Lexer::tokenize` (`phpdoc/src/grammar/lexer.rs:177`) lexes a tag's whole text up front, and `TagParser` only receives the token vector. Its comment says a docblock is too small for a lazy lexer.
  - Each identifier becomes a `String` (`lex_identifier`), and each `advance` clones a token.
  - `remaining_description` turns the tokens after the type and the variable back into text, joined by spaces (`grammar/parser.rs`).
- **Who reads a description.**
  - One place reads it: the `param` arm of `apply_tag`, which tests whether the description starts with `[optional]` (`docblock.rs:915`).
  - Nothing else does. `param-out`, `return`, `var`, `property` and `throws` ignore it (`docblock.rs:920-956`, `:990`, `:1246`), and so does `checker/phpdoc_unresolvable.rs:37-40`, `:94`.
  - `PhpDocTagValue::Generic(String)`, the text of every tag the grammar does not model, is built and never matched outside the `phpdoc` crate.
- **Design.**
  - The parser pulls tokens from the lexer instead of taking a token vector.
  - When it reaches the description, it records the byte offset and stops. The value keeps that offset into the tag's text.
  - The description's text is made on request, by the same lexer and the same join, from that offset. `[optional]` becomes a prefix test: it lexes only the description's first three tokens (`[`, `optional`, `]`, as `docblock.rs:909-915` does now) and never builds the text.
  - A `Generic` value is the tag's text, sliced and joined on request.
- **Why it holds (C1).** The tag lexer's state is its text and one byte offset (`Lexer { src, pos }`, `lexer.rs:46-50`): no mode, no nesting depth, no look-behind. Every token starts after `skip_whitespace` at `pos` (`:177-187`). So a fresh lexer started at a token's start gives the same tokens from there on, with spans shifted by that offset. What the pulling parser must keep:
  - **Rewinds.** The parser resets `pos` to a checkpoint in many places (`parser.rs:229`, `:452-456`, `:626`, `:897-910`, `:961-976`, `:1047`, `:1147`). The lexed tokens stay in a buffer, and the description's offset is the start of the token at the parser's final position, not the furthest token lexed.
  - **Lookahead to the end.** `looks_like_html` (`parser.rs:868-890`) scans every remaining token for a closing `</name>` once a type is followed by `<name>`. In that case the parser lexes to the end of the tag, as now.
  - **Spans.** `preceded_by_whitespace` and `is_adjacent_to_previous` (`:636-643`, `:850-852`) compare neighbouring tokens' spans, and work unchanged on a buffer.
  - **The memo.** `parse_tag_rest_shared` shares a value between equal tag texts, so a value that keeps an offset keeps its tag text (or an `Arc<str>` of it) too.
- **Adversarial checks.**
  - **The same text when read: re-lex, never slice.** `skip_whitespace` drops a `//` comment to the end of the line, so `http://x` in a description loses `//x`. `token_text` (`parser.rs:1235-1262`) drops a string literal's quotes and `\` escapes, writes a float in Rust's `{:?}` form (`1.50` becomes `1.5`) and an integer that overflows `i64` as `0`, and `remaining_description` joins with single spaces (`:305-312`). A raw slice would differ in each case. A debug build compares the lazy text with the eager text for every tag.
  - **Errors.** The tag lexer reports none. The parser's errors come from the type and the variable, which are still parsed. `check_description_start_after_type` reads one token past the type, which the pulling parser still lexes.
  - **Memo keys.** `parse_tag_rest_shared` and `tag_block` key their memos on the whole tag text, so their hit rates do not change. Item 14's fingerprint keys remove that formatting.
- **Saving, from measured shares (B2).** The probe counted, for each run of the tag lexer, what it lexed and what a pulling parser would have, up to the last token it looked at before a description. Runs are counted, not tags, so the memos are in the count; Piwigo's 61,615 runs equal `tokenize`'s call count in the profile.

  | phase | lexer runs | bytes lexed | not needed: bytes, tokens | description tokens |
  |---|---:|---:|---:|---:|
  | stub build (the same on both trees) | 21,840 | 2,197,804 | 78.9%, 81.2% | 414,584 |
  | parse, Piwigo | 23,135 | 1,222,438 | 36.4%, 36.3% | 98,541 |
  | check, Piwigo | 16,640 | 571,023 | 7.4%, 8.1% | 10,638 |
  | parse, Laravel | 55,377 | 3,698,324 | 21.1%, 18.6% | 64,391 |
  | check, Laravel | 27,756 | 580,709 | 1.4%, 2.0% | 1,815 |

  - The drill estimated 84.4% for the stubs and 56.5% for Piwigo's parse, from every tag's description share (`analysis/n4/desc.py`). Laravel's parse is lower still: its `@method` tags are 36,859 of the runs, and 83% of their bytes are needed.
  - In Ir on Piwigo, the tag lexer's inclusive cost (308.5 M off, 132.7 M warm) is split by phase in proportion to the lexer's self cost per phase: off, stub build 175.9 M, parse 90.6 M, check 42.0 M; warm, parse 84.8 M, check 47.9 M. Assuming Ir is proportional to the bytes lexed, and at the measured shares, not lexing those bytes saves 138.8 + 33.0 + 3.1 = 174.9 M off and 30.9 + 3.5 = 34.4 M warm, and `remaining_description` goes, 197.9 M off and 47.5 M warm, except for the three-token prefix test on each `@param` description. B2 counted that test as one token; it needs three, and `@param` descriptions are 266,494 of the 414,584 description tokens in the stub build. So the ≈0.37 G and ≈0.08 G below are ceilings.
  - **≈0.37 G off and ≈0.08 G warm**, against the drill's 0.34 and 0.1 G. Four fifths of the off figure is the stub build, which proposal 6 removes. Item 14's borrowed tokens halve the rest: ≈0.04 G once both are in. A tree whose doc comments are mostly signatures, such as Laravel's, saves less.

### 10.8 Skim the function bodies of context files, and parse a body when something reads it

- **What it removes.** For the ≈48% of the parse job's tokens that are context files' function bodies (§5), on today's code:

  | cost | G |
  |---|---:|
  | the parser | ≈1.73 |
  | the lexer's token layer (keywords, names, interning), leaving the raw scan (`next_raw`, `lex_operator`, `lex_ident_raw`, `scan_interpolated`: 23% of the lexer) or a cheaper byte scanner | ≈1.07 |
  | those bodies' summaries, flows and walks (67% of the 617 M body blocks) | ≈0.41 |
  | about two thirds of the 567.8 M of tree drops at the insert | ≈0.38 |
  | the global-facts walk over those bodies, ≈44 Ir a body token by its fall when PROD-2 left bodies with nothing to add unwalked (the reference walk reads only doc comments, which a skim leaves) | ≈0.12 |
  | **total, F, before the bodies something reads are taken out** | **≈3.7** |

  Call this F. Items 9, 10 and 12 lower the cost per token, items 3 and 19 lower the drops, and item 13 merges the walks; after them, F is ≈2.0-2.3 G. A body that something reads costs its share of F as before, so with d the share of body tokens parsed in full, the most proposal 8 can save is F × (1 − d). Two values of d appear below: the bodies something reads are d_read ≈ 0.005-0.006 (by count and by bytes, B1), and with the bodies a skimmer trigger flags added, d ≈ 0.01-0.02 by tokens (B1, below).
- **Who reads a context file's bodies today.** Each reader was traced in the code:
  1. **`BodySource::summary` and `init_flow`** (`registry/bodies.rs:16-27`). Their readers are:
     - the purity pass (`purity.rs:350`);
     - the property-write pass (`property_writes.rs:423`);
     - the uninitialized-property checks (`checker/uninitialized.rs:125`, `uninitialized_dataflow.rs:535`);
     - the parent-call rules (`project/class_shape.rs:343`), whose built-in PHPUnit and Codeception hook rules are on by default.

     The two passes read every body; with proposal 9 they read on demand, which is why proposal 8 depends on it. The checker's reads are already per call.
  2. **The declarations a body holds**: named functions and classes declared in a body, anonymous classes, and `define()` (`project/file_declarations.rs:59-120`, `for_each_declaration_in`). The parser and the lexer already flag the files that can have these (`FileFacts`: `has_nested_declaration`, `has_anonymous_class`, `mentions & MENTIONS_DEFINE`).
  3. **`GlobalFacts`** (`registry/global_vars.rs:471-569`): an assignment to a variable that a `global` statement anywhere in the body imports (the walk collects the names first, so the order of the `global` line and the assignment does not matter), and an `include` or `require` anywhere. A body with neither adds nothing.
  4. **The references a body records.** They feed:
     - `dependents` (`project.rs:1056`), which `walk_affected` (`:2033`) and `closure_scope` (`:1295`) read;
     - `mentions_phpstan` (`project.rs:3179-3187`), which chooses the stub base.
  5. **A property hook's body**, which `virtual_property.rs:41` reads to decide whether the property is virtual. Hooks are not function bodies, and they stay parsed.
  6. **A context file parsed again from disk.** `Project::ast_of` (`project.rs:890-917`) re-reads the file with `std::fs::read`, parses it in full and caches 32 trees, with no hash check. `checker_for` (`:2275`) calls it, and `judge_property_types` (`:2185`) runs the checker on every non-analysed file that declares a class involved in a private-property write, or a trait such a class uses. Skimming leaves it unchanged, because that parse is a full one.
  6. **`is_generator` and the `goto` labels.**
     - Only the checker reads `is_generator`, for analysed functions (`checker/function_decl.rs`, `return_type_too_wide.rs`, `calls/closure_context.rs`), and so does the language server's completion.
     - The labels feed `InitFlow`, which a parse on demand computes.
- **d, measured (B1): ≈0.005-0.006.** The probe recorded every body read through `BodySource::summary` and `init_flow` during the check, with proposal 9's searches answering the purity and property-write questions:

  | | Piwigo | Laravel |
  |---|---:|---:|
  | context bodies | 49,533 (17.7 MB) | 38,146 (13.3 MB) |
  | context bodies read, all readers | 313 (102 KB) | 192 (71 KB) |
  | **d, by count and by bytes** | **0.0063, 0.0058** | **0.0050, 0.0054** |

  - **The checker read no context body through `BodySource`.** The uninitialized-property rules are off by default (`rules.check_uninitialized_properties`), and the parent-call rule reads only for the methods it is configured with. Its built-in PHPUnit and Codeception hook rules (`setUp`, `tearDown`, `setUpBeforeClass`, `_before` ...) are on by default, so on a tree with test classes it reads those few ancestor hook bodies; B1's two trees have none. With the other rules on, the ancestors' bodies they read add to d; that is not measured. The checker does read context files another way: reader 6.
  - **The skimmer's own full parses.** A body that holds a trigger (below) is parsed in full. Counted with `token_get_all` over each run's context files (its body counts, 49,496 and 38,236, come from a scan of outermost bodies and differ from the probe's 49,533 and 38,146 by −37 and +90; the shares agree to the digits shown): Piwigo flags 210 of 49,496 bodies (0.42%), 1.34% of the body tokens (include 151, `new class` 34, a nested `function` 33, a `PHPStan` name 16, `class` 3, `define` 2); Laravel 81 of 38,236 (0.21%), 0.62%. With the bodies read, **≈1-2% of the context body tokens are parsed in full.**
- **Design.**
  - **The skim.** The parser takes a skim flag for context files. At the `{` of a function or method body, it runs the raw lexer to the matching `}`, and notes for each body whether it saw:
    - `function` or `fn` followed by a name;
    - `class`, `interface`, `trait` or `enum` as a declaration;
    - `new class`;
    - `define`;
    - `global`;
    - `include` or `require`, or their `_once` forms;
    - a name with a `PHPStan` segment, in code or in a doc comment inside the body.

    Any of these makes it rewind and parse the body in full. Otherwise the body is stored as its byte range and the name context in effect.
  - **Parsing on demand.** `Body` holds its summary and its flow in `OnceLock`s, filled on first read by parsing the range with the stored name context. Spans stay absolute within the file, because the lexer runs over the file's text from the body's offset.
  - **The source text. Decided (D2): kept, copied at load from the bytes the load reads.**
    - Every load reads the file's bytes, if only for the per-file cache's key (`session.rs:631-637`). A skimmed body's bytes are copied out of them then. A body parsed later then reads the bytes the load checked, even if the file has changed on disk since.
    - A body parsed on demand is lexed with `Lexer::fragment(src, base)` (`syntax/src/lexer.rs:461-473`), whose spans are where they are in the file. `fragment` takes text, so a file that is not UTF-8 needs a bytes variant, as whole files have (`Lexer::from_bytes`).
    - The kept bytes come to ≈18 MB on Piwigo (17.7 MB in 49,533 bodies, B1). The run's peak is 866 MB, of which ≈420 MB are the context files' trees (the 20 G report's M1), and the skimmed bodies' share of those trees is never built.
    - The language server already keeps every file's text (`lsp.rs:294`), so there the bodies cost nothing more.
    - **Reading the file again on demand is rejected.** (`ast_of`, reader 6, already re-reads a file on demand today, without a hash check.) It may have changed since the load, and a hash check would only detect that. Mapping the file has the same problem and needs `unsafe`, which the workspace denies (`unsafe_code = "deny"`) outside the allocator and PCRE2's C API.
  - **References.** A body parsed on demand registers its references then, so an edit to a name it reads still reaches it.
  - **The per-file cache** stores a skimmed body as its range and name context. A load that hits the cache has read the file's bytes for the key, so the body's text comes from them, as on a parse.
  - **Syntax errors. Decided (D1): every skimmed body is still checked, by a pass that builds only what its checks read, and each file's verdict is cached.**
    - **Why the bodies must be checked.** Today a context file whose parse fails is skipped with a warning, and its declarations are lost (`project/session.rs:669-679`, `:828-837`). PHP refuses the whole file too, for one bad method body (php 8.5.4; `~/.cache/prs-probe/syntax-in-body/`, `validate/d1/`):
      - a parse error throws a catchable `ParseError`, and nothing the file declares exists;
      - a compile-time error, such as "Cannot mix keyed and unkeyed array entries" or a `goto` to a missing label, is fatal: the process ends at the `include`, and the declarations compiled before the error are seen only by a shutdown function;
      - the modifier checks (`zend_compile.c:946-1044`, "Multiple final modifiers are not allowed") throw `CompileError` instead, `ParseError`'s parent, which a `catch` of `CompileError` or `Throwable` catches and one of `ParseError` does not.

      Either way no code after the `include` runs. Leaving a skimmed body unchecked would make whether a file loads depend on whether an optimization skipped one of its bodies. On Piwigo no context file failed to parse: the off run's stderr (`off/valgrind.err`) holds no warning.
    - **Every error is the parser's or the lexer's.** `syntax::parse_watching` is the only step of a load that fails (`project.rs:2892`), and a lexer error anywhere in the file outranks the parser's first error (`syntax/src/parse.rs:116-133`). So the check needs the grammar and the lexer's token layer, and no later pass over the tree.
    - **The checking pass is a reduced builder, not an empty one.** The parser checks destructuring after it builds the array (`syntax/src/parser/expr.rs:402-497`), with sets keyed by node id (`long_arrays`, `pattern_only`). So the pass keeps array shapes and node ids, and builds no other node, no reference and no interned name.
    - **The verdict cache (C6).** Whether a file parses is a function of its bytes and the `syntax` crate alone:
      - what `parse_file_with_names` does after the parse, the declarations and the references, returns no error (`project.rs:3003-3071`);
      - the lexer's watch list, the one input from outside `syntax`, sets only `facts.mentions` (`lexer.rs:416-419`), never an error;
      - `syntax` has no stub mode: `is_stub` is read only after the parse, by `file_declarations::collect`;
      - `syntax`'s only feature, `fuzzing`, derives `Arbitrary` (`php_string.rs:13`, `name.rs:263`).

      So a cache keyed by the file's content hash and by a hash of `crates/syntax` (its sources and manifest), the workspace manifest, `Cargo.lock` and the target, computed as `build.rs` computes the build's source hash, is exact. It holds whether the file parses, its first error, and the lexer's warnings in its skimmed bodies, which a load reports (`session.rs:803-812`). A body whose file has a cached verdict is skimmed and not checked again. The cache survives every build that does not touch the parser, whereas the per-file cache is keyed by the whole build. A debug build checks every body anyway and compares the verdicts.
- **What it saves.** Without a cached verdict, every skimmed body is checked, and a body that something reads is then parsed in full. Let S be what the checking pass saves against a full parse, over all the skimmed bodies. The token pipeline and the grammar stay in the pass: `fill`, `skip`, `take_token` and the tokens' drops are ≈1.05 G of the parse job's 3.65 G parser block.

  | cost the pass does not pay | today, G | after the plan, G |
  |---|---:|---:|
  | building the tree: the bodies' share of at most ≈0.53 G in the parser block (allocation 308 M, vector growth ≈94 M, copies 108 M, node drops ≈24 M), less the arrays the pass keeps | ≈0.15-0.25 | ≈0.05-0.1 (item 9) |
  | recording references and resolving names | ≈0.09 | ≈0.05-0.09 |
  | interning names | ≈0.17 | ≈0.08-0.12 |
  | the bodies' summaries, flows and walks | ≈0.41 | ≈0.27 |
  | the global-facts walk | ≈0.12 | ≈0.06-0.1 |
  | the tree drops at the insert | ≈0.38 | ≈0.05-0.15 |
  | **S** | **≈1.3-1.4** | **≈0.55-0.85** |

  At d ≈ 0.02 by tokens (B1):
  - **With the file's verdict cached:** F × (1 − d), ≈3.6 G today and **≈2.0-2.3 G after the plan**.
  - **Without it** (a file not seen before, or the first run after a change to `syntax`): S − F × d, ≈1.25-1.35 G today and **≈0.5-0.8 G after the plan**. That is nothing only from d ≈ 0.24-0.42, so proposal 8 pays on every run.
  - **The verdict cache** is worth F − S ≈ 1.2-1.7 G after the plan on a run whose files have verdicts. It no longer decides whether proposal 8 pays.
- **Adversarial checks.**
  - **The verdict cache cannot go stale the way a narrower stub-cache key would (§10.12).** The verdict's inputs are one crate and its locked dependencies, and the key covers them whole. The stub cache's inputs are most of the main crate.
  - **A panic is not a verdict.** A parse that panics makes the file unread today (`session.rs:661-666`), and nothing caches it.
  - **The stub base must not change.** `mentions_phpstan` reads the references, and a skimmed body's references are not recorded. The references include the names in doc comments, also those inside function bodies (`recorded_references`, `deps.rs:46-105`). So the skimmer flags a `PHPStan` segment in code and in the body's doc comments, and collects the body's doc-comment names for `dependents` (reader 4) too. A debug check compares the base chosen, and the references, against an eager parse.
  - **False flags are safe.** A trigger word inside a string or a comment only causes a full parse.
  - **The facts stay identical.** `FileFacts` stays file-wide and is computed as now, because the skimmer's raw lexer still sees every identifier for the `mentions` bits. A debug build also parses every skimmed body eagerly and asserts equal summaries, flows, declarations, global facts and references.
  - **Summaries do not depend on the registry** (`BodySummary::of(&body.stmts)`, `bodies.rs:112`), so a summary made late equals one made early.
- **Two questions the check raises, settled by the investigations (D1, D6).** Both hold whether or not proposal 8 is built.
  - **The parse-failure policy (D6, recommended).** A context file that fails to parse loses every declaration. That is right when the error is real and wrong when it is a gap in our parser, and the analyser cannot tell which.
    - **What PHP does with a refused file** (php 8.5.4, autoloading a file one of whose bodies does not parse): `new`, a static call and `class_exists()` autoload it and throw `ParseError`, again on every later attempt; `instanceof`, `::class`, `class_exists($n, false)`, a parameter type and a `catch` clause behave as if the class were not declared; a function it declares is undefined. A compile-time error ends the process at the autoload instead.
    - **Neither world gives "not found" at a use the analysis continues past.** Where the error is real, a use that needs the declaration throws or ends the process, so no code after it runs. Where it is a gap of ours, the declarations exist. Keeping them is the union of the two worlds.
    - **What fails in real code.** `php -l` over every PHP file under `vendor/`: 111,836 files in 18 trees, 67,252 of them installed dependencies. 272 fail, 13 by parse errors and 259 by compile errors, most of them of C7's 238 kinds (257 of the 272 are fatal; the other 15 throw, 2 of them as `CompileError`). 72 are phpstorm-stubs, which the stub loader reads: 25 built-in functions redeclared, 35 declarations repeated for version gates, `function exit` (`Core.php:1193`) and 11 other compile errors. 198 are test fixtures, reaching the analyser only through `paths` or a scan directory, 1 is rector's `stubs/Symfony/Component/Form/FormBuilderInterface.php` (below) and 1 is an example. Of the 67,252 installed files, 3 fail (a test script of `php-console`, a fixture of `phpcs-gitlab`, an example of `tracy`), all outside their package's `autoload` directories. A root's own `autoload-dev` is never context (`composer/src/vendor.rs:17-23`): 184 of phpstan-src's 191 failing files are in its `autoload-dev` classmap (171 under `tests/*/data/`, 13 under `tests/PHPStan/Analyser/nsrt/`), which PHPStan's own `paths` exclude (`build/phpstan.neon:30`, `:33`), and 7 are in neither. The one that reaches the analyser as context is `rector-src/stubs/Symfony/Component/Form/FormBuilderInterface.php`, written for analysers, which PHP refuses ("Non-abstract method … must contain body", so the `interface_exists` guard at its top never runs) and rector's `phpstan.neon` scans on purpose, so its analysis sees the class when `symfony/form` is absent. Our parser accepts it (`class A { function f(); }` in C7's run), so the file loads today.
    - **Gaps of ours are in real code.** The parser refuses `$x instanceof static::$name`, which PHP accepts; six files in installed dependencies lose every declaration for it (E14, §10.14). The parser refuses `$s{0}`, valid in PHP 7 (rector's `e2e/parse-php7-code/src/Foo.php`), because its postfix loop has no arm for `{` (`syntax/src/parser/postfix.rs:16-241`), and accepts `(real)` and `(unset)` (`syntax/src/ast.rs:1303`, `:1311`), which PHP 8 refuses.
    - **Recommended:** keep what the file declares. Recover what parses: a failed body keeps its function or method with an unknown summary (nothing new once the skimmer finds a body's end without parsing it), and a failed member or class header leaves the class declared with an unknown shape, a new kind of registry entry on which no rule reports. Without proposal 8, the parser needs recovery points: the end of a member, at class depth, and the end of a top-level statement. Report the file once, with today's warning; a use site gets no report, since it would be false for every gap of ours. Once C7's gap is closed and validity follows the version, a use-site report ("this autoload throws `ParseError`") becomes exact. Validity follows the configured PHP version: the parser accepts every version's syntax and records what a file used, as master's `SyntaxMarks` does for 7.1-8.1 syntax (`syntax/src/lexer.rs:250-283`), extended to removed syntax, and the version is applied after the parse, so the verdict cache's key stays exact. One rule for the batch run, which on master drops a failed context file's declarations (`context_failures`, `project/session.rs:324`, `:2143-2174`), and the language server, which keeps the last version that parsed (`lsp.rs:752`, `parse_failed`). With recovery both use what the current text declares, and a header that fails leaves its class with an unknown shape, not gone, so the server no longer needs the last good version for it. The verdict cache stores the recovered regions with the verdict. The cost: recovery in the parser (none for bodies once proposal 8 is in), a registry entry with an unknown shape, and marks for removed syntax. Psalm recovers what parses (`StatementsProvider.php:367-424`, Psalm 6.20.0, the latest stable); PHPStan drops the file (`FileNodesFetcher.php:32-36`, 2.3.1).
    - **Why not today's policy.** It is wrong for every gap of the parser, and both kinds of gap are in real code. It also ties the parser's strictness to what context files contribute: matching PHP exactly would drop the file written for analysers that rector scans on purpose.
  - **Compile-time errors the parser does not report (C7).** Every site in `zend_compile.c`, `zend_enum.c`, `zend_API.c`, `zend_opcode.c`, `zend_attributes.c` and the scanner of php-src 8.5.4 that raises a compile error got at least one trigger file, and 13 of `zend_inheritance.c`'s 54 did (299 files in all), each run with `php -l` and with `echo "RAN\n";` first, and then through `phpstan-rs check`. 268 sites refuse the file at compile time with their exact message. **The parser rejects 30**: 20 by checks of its own with PHP's message (destructuring, the `foreach` key, empty array elements, `match` with two defaults, nested ternaries, `final` or `abstract` on an anonymous class, numeric literals and `\u{}` escapes), and 10 by its grammar with another message. **It accepts 238**, which PHP refuses while we load their declarations: class members 53, variables 26, constant expressions 24, writes 21, types 18, control flow 18 (a `goto` to a missing label among them), calls and arguments 17, names and namespaces 16, modifiers 11, attributes 10, magic methods 8, `declare` 5, inheritance within one file 5, the lexer 4 (two sites fall in no group). Examples per group:

    | group | sites | examples |
    |---|---:|---|
    | class members | 53 | an abstract method in a class that is not abstract, a property hook with no body, `readonly` without a type, a promoted property outside the constructor, an enum case with no value, `case` outside an enum |
    | variables | 26 | `$this` as a parameter or a `global`, a parameter declared twice, `use ($a, $a)`, `foreach ($a as $this)` |
    | constant expressions | 24 | `const X = $a;`, `static::` in a class constant, `new` in a class constant, `[...1]` |
    | writes | 21 | `f() = 1;`, `$a?->b = 1;`, `$x = $a[];`, `$GLOBALS = [];`, `[$a + 1] = $b;` |
    | types | 18 | `int\|int`, `?mixed`, `void\|int`, `int&A`, `\int`, `function f(void $a)`, a bad default for a typed parameter |
    | control flow | 18 | `break 2;` with one loop, `break;` outside any loop, `goto` to a missing label or into a loop, `return 1;` in a `void` function, `yield` outside a function |
    | calls and arguments | 17 | a positional argument after a named one, `new A(...)`, `self::` in a function, `parent::` with no parent |
    | names, imports, namespaces | 16 | two functions with one name, `use X\B; use Y\B;`, code after a braced namespace, `namespace` after a statement, `namespace namespace;` |
    | modifiers | 11 | `public private`, `static static`, `final abstract class` |
    | attributes | 10 | `#[A(...$a)]`, `#[Attribute]` on a trait, `#[\NoDiscard]` on a `void` function |
    | magic methods | 8 | `__get()` with no parameter, `static function __get`, a return type on `__construct` |
    | declare | 5 | `strict_types` not first, `declare(strict_types=1) { }` |
    | inheritance in one file | 5 | a final method overridden by a class declared later in the same file |
    | lexer | 4 | `(real)`, an unterminated `/*`, a heredoc whose indentation mixes tabs with spaces or is too shallow |

    The parser's 10 grammar rejections are `list()` outside a destructuring, `__halt_compiler()` inside a block, a braced namespace inside another, an unmatched `)`, four hook forms (`get()`, `set` with two parameters, a variadic one, one with a default), and `static` or `abstract` on a trait alias. A reading of `crates/syntax` came first, and the run corrected it on three cases (`f as static g` and `f as abstract g` are rejected, `namespace namespace;` accepted). 19 triggers are not compile-time refusals: they fail only when the declaration runs, after the statements before it (a class declared twice, the enum checks, a trait that does not exist, an interface implemented twice). `zend_inheritance.c`'s checks refuse at compile time only when the parent and interfaces are declared earlier in the same file (early binding); 13 of its 54 sites were triggered, 11 at compile time, and the other 41 are linking checks of the same kind, not triggered one by one. A few sites are not reached from source: PHP raises an earlier error first (`:1153`, `:8253`, `:9810`), the value is folded to a string first (`:11120`), no input reaches them (`:11523`, `:11542`), or the environment raises them (the stack limit, the scanner's encoding errors). Almost all 238 are local to the file, so a pass over the tree could decide them; the checker reports some as diagnostics (`case` outside an enum, `goto` to a missing label, `checker/keywords.rs:180`), in analysed files only, so a context file with one of these errors keeps its declarations. Which of the 238 the checker reports was not counted here; Q-3 counted it since, over the 247 trigger files the parser accepted when C7 ran: 120 reported as PHP does, 1 more with an opt-in rule, 115 by nothing. On these triggers, when the parser fails PHP refuses too, on PHP 8.5. That does not hold on PHP 7 (`$s{0}`), nor in real code on PHP 8.5 (E14). Proposal 8 does not change this split, since its pass runs the same parser.

### 10.9 Purity and property writes on demand

- **Now.** The finishing passes classify every scoped function and method from its body summary, and then run the fixed points over all of them. The purity, property-write and classification blocks cost 926.4 M in the off run and 880.6 M in the warm run (§7). They run on the finishing thread, the run's wall-time bottleneck, and are 0.19 s of its 0.41 s serial window.
- **Who reads the results.**
  - Only the no-op rules read `FunctionReflection::is_transitively_pure` (`checker/noop.rs:249`, `:280`, `:335`, and the `new` arm at `:351`). They ask about the callee of a call whose result a statement discards.
  - A fifth reader asks about the declaration itself: `check_declared_purity_tag` (`checker/noop.rs:395`, called from `checker/function_decl.rs:295`) asks for every function and method the checker visits, before it looks at the doc comment. Proposal 9 must test the `@phpstan-impure` / `@impure` tag first, as the B1 probe did. With that order only tagged declarations ask, and there are none on Piwigo or Laravel, so B1's question counts are the counts after the reorder.
  - Property writes are read per call by `checker/expr.rs:186-297` and `checker/array_access.rs:70-103`, and by `changed_since` (`registry.rs:670`), which compares them across an edit.
- **What the fixed point is.** `run_purity_fixed_point` (`purity.rs:190-274`) computes a least fixed point. A function becomes pure when every one of its dependencies is pure, so a cycle never becomes pure, and the value at `f` depends only on what `f` reaches.
  - **Search.** A depth-first search from `f` gives the same answer exactly: `f` is pure if and only if every node it reaches passes classification and the search reaches no cycle. It stops at the first impure node.
  - **Memo.** Answers are memoized per declaration id. This is item 4's `registry.purity(id)`, which item 4 already has the checker call.
  - **Concurrency.** Two threads asking at once compute the same answer from the same immutable registry. The first to publish wins, and the other's equal result is dropped.
  - **Property writes** have the same shape: a union over what is reached.
- **The incremental path.** `changed_since` needs the old and new property writes of the names an edit can reach, and it computes them for the closure scope it already computes, as now. The memo is cleared on each edit. That is always correct, because the memo caches a function of the registry.
- **Measured (B1).** The probe answered every question of the check a second time by a memoized search from the declaration asked about:

  | | Piwigo | Laravel |
  |---|---:|---:|
  | purity questions (function, method, static, `new`) | 39,880, 5,120 of them on stubs | 5,184, 404 on stubs |
  | distinct non-stub callees asked about | 1,572 | 1,930 |
  | of them pure | 0 | 0 |
  | nodes the purity search visits | 1,590 | 1,939 |
  | property-write questions | 26,189, 7,850 on stubs | 2,130, 1,375 on stubs |
  | distinct non-stub callees asked about (nothing, own object, anything) | 2,489 (1,025, 89, 1,375) | 258 (82, 38, 138) |
  | nodes the property-write search visits | 3,007 | 321 |
  | declarations the eager property-write pass classifies | 69,479 | 113,812 |
  | cycles met (purity, property writes) | 0, 14 | 0, 0 |
  | answers that differ from the eager pass | 0 | 0 |

  - **The search agreed with the eager pass** on every answer asked about, on both trees. But every purity answer was impure, no purity cycle was met on either tree, and only the property-write search met cycles (14, on Piwigo). The pure answer and the purity cycle rule rest on the argument below and the debug-build comparison, not on these runs.
  - **It reaches little.** Each search visits 0.3-4.3% of the declarations (Piwigo 2.3% and 4.3%, Laravel 1.7% and 0.3%). No non-stub callee the no-op rules asked about is pure, so each search stops at the first impure node, often at the root's gate before any body is read. On Piwigo the purity search read 123 context bodies and 258 analysed ones, for roots whose own bodies are 227 and 1,288.
- **Adversarial checks.**
  - **Exactness.** A debug build runs the eager pass too and compares every answer given on demand.
  - **Order.** The results do not depend on which thread asks first, because each answer is a function of the graph.
  - **The fixed point's kind.** With a greatest fixed point, cycles would be pure and the search would need strongly connected components. The code computes a least one: the worklist starts from the nodes with no impure dependency.
- **Saving.**
  - Up to the three blocks, 0.93 G in the off run and 0.88 G in the warm run, less what is asked for, which B1 puts at a few percent.
  - Item 4 removes ≈0.5 G of their string keys first, so proposal 9 adds ≈0.3-0.4 G beyond item 4, plus most of the passes' 0.19 s on the finishing thread.
  - It also removes the reason every context body's summary is needed, which proposal 8 requires.

### 10.10 Check UTF-8 with SIMD

- **Now.** `String::from_utf8` in the file read costs 29.8 M over the 14,251 files, about 56 MB (19.07 MB analysed plus the context files' 37.1 MB): ≈0.5 Ir per byte on std's word-at-a-time path for ASCII. The 66.5 M in the parse block is every caller of `core::str::converts::from_utf8` in that phase: it also holds the `UseMap` name folding, `ShapeKey::from_bytes`, the lexer's numeric literals and others.
- **Design.** `simdutf8`'s AVX2 path checks 32 bytes per step, roughly a quarter of the cost (an estimate): a saving of ≈22 M. LOAD-6 measured a third: `validate_utf8_basic` took 10.2 M over the 14,248 files against std's 29.8 M, so ≈20 M came off the check, and the copy into a `String`, which the workspace's no-`unsafe` rule forces, took it back (−0.47 M; not kept).
- **Adversarial checks.**
  - The check cannot be dropped: whether the text is UTF-8 decides the length a file is weighed by and the source that is kept (`session.rs:649-660`).
  - The `Utf8Chunks` in `LineIndex::new` (70.6 M) is item 11's.

### 10.11 Make text only when it is read

- **Reported messages, formatted when displayed: 22.5 M (11a; 55 messages, of which `finalize` drops 22, §9.2).**
  - **Now.** `Diagnostic.message` is a `String` (`crates/diagnostics/src/lib.rs:99`), formatted when the checker reports. `finalize` then drops what is above the report level (`session.rs:206`), before inline ignores, configured ignores and the baseline.
  - **Design.** The message becomes a value that displays itself: a template and its operands. `Type`s are interned and immutable, and `diagnostics` holds the value as a `dyn Display + Send + Sync`. Text is made for output (JSON, text, the language server), and for whatever compares message text (`ignore_errors` patterns, the baseline). Those comparisons see only the reports that survive the level filter.
  - **Adversarial checks.**
    - A message's text must not depend on state that changes after the report. Type display depends on the type and on settings fixed for the run.
    - The per-file cache stores no diagnostics (`project/file_cache.rs`), so nothing forces the text early.
  - **The truncations (B3).** Reports that a loop's convergence passes discard (`rewind_to`) stop paying for their text too: 270 on Piwigo, 304 on Laravel. Most cost little, but 30 of Piwigo's are §9.2's `offsetAccess.mightNotExist` messages (Q-6), at ≈409 K each ≈12.3 M. The other 3 of those 33 are the `extract()` truncation's, inside the figures below. The `extract()` truncation discards 31,660 on Piwigo; 11a would save only their formatting, ≈11 M, and E10 saves all ≈35 M (§10.13).
  - **Relation to F1.** F1 defers the text of what a rule formats before it knows whether to report. This proposal continues F1 past the report.
- **Class-constant keys: 6.6 M (11b).**
  - **Design.** `class_constant_type` returns the declared type at once when `dynamic` is empty (`has_dynamic_constants`). When dynamic constants are configured, they are keyed by the class's folded name and the constant's name, with no `String` built per read.
  - **Check.** The configured keys are built once in `ConstantResolver::new`, so the lookup must fold the class name the same way: lowercase, without a leading `\`.

### 10.12 Rejected

- **One rayon pool, as a speed-up by itself.**
  - The global pool runs only during `Project::new()`, before the worker pool has any work (§1). Moving the corpus's `par_iter` into `worker_pool().install` gives the same 8 threads the same work.
  - It matters only together with proposal 5, where the two would otherwise run at once.
- **A narrower stub-cache key**, kept across builds whose stub-related code did not change.
  - What the cache holds is the output of the registry's builders (`from_def`, `from_decl`), the PHPDoc parser, the type interner and the serialization layout: most of the crate.
  - A narrower key that misses one input reads a stale base and gives wrong results with no error. The checksum catches damage, not staleness. Today's key already misses one input, and that is what happens (E15).
  - After the crate split, the declarations depend on a known set of crates, and a key over them is the per-file cache's natural form (§10.6).
- **A dedicated reader for Composer's static autoload file.**
  - The PHP parser spends 162.8 M on the 1.8 MB `autoload_static.php` (§8). A reader for its fixed shape would save most of that.
  - But it would save it on a thread that is off the critical path, and P10 decided to keep one parser.

### 10.13 More cuts the investigations found

None is in the 20 G plan. Each is traced in the code below the table.

| # | change | saving | runs | exact | needs |
|---|---|---|---|---|---|
| E2 | a statement's assigned and defined names walked only when it has `@var` tags | ≈ −50 M | all | yes | — |
| E3 | a method or static call probed for a PHPUnit assertion only when its written name starts with `assert` | ≈ −35 M; ≈ −12 M after item 18.1 | all | yes | — |
| E4 | the analysed files stored in the per-file cache too | ≈ −0.25-0.3 G net (548 M gross, B5) | per-file cache warm | debug check | — |
| E5 | a kind code in the type node's free flag bits | ≈ −30 M | all | yes | — |
| E6 | one node against itself answered first | ≈ −10 M net (B7) | all | yes | — |
| E7 | plain object types kept with their class, two statics, a table of small integers | ≈ −55 M | all | yes | item 4's slot; item 16 overlaps |
| E8 | a cache file's types stored once, in a table its records index | ≈ −65 M; ≈ −25 M after item 6 | stub cache warm | yes | item 6, steps 3 and 5 |
| E10 | nothing reported while an enclosing body has called `extract()` or `eval()` | ≈ −35 M on Piwigo | all | yes | — |
| E11 | a cached file's origins decoded as the one `Arc<Path>` its file binds | ≈ −116 M | per-file cache warm | yes | E8's decode context |
| E12 | each class's assert-tagged method names carried in its `BuiltDecl` | ≈ −0.35-0.4 G, serial | per-file cache warm | yes | — |
| E13 | the per-file cache's shard directories made once, and the process id read once | ≈ −80 ms of kernel CPU, ≈ −10 ms of wall | runs that write the cache | yes | D5 |

E9, `format!` per product in the regex scanner (`scan_branch` starts its accumulator at `[""]` and calls `concat_literals` per atom, `regex_groups.rs:778`), is folded into F6f's rewrite of the scanner (`PROFILE_DRILL_PIWIGO_20G.md:1705`; §9.2): extending the left string in place when the right side is one literal belongs in that rewrite.

- **E2.** `check_stmt_impl` calls `inline_var_annotations` (`checker/stmt.rs:624`), which calls `statement_defined_variable_names` (`:1562`), then `statement_assigned_variable_names` (`:628`) and `statement_defined_variable_names` again (`:1260`), for all 145,081 statements. Each name becomes a `String` through `Name`'s `Display` (18.6 M of `collect`'s 35.9 M) and is pushed into a fresh `Vec` (11.6 M of growth), and the frees add about 4 M. Only the two loops over the statement's `@var` assertions read them (`:629-636`, `:1261-1266`), and `statement_var_tags_of` (`:1542-1555`) returns nothing unless the doc comment holds `var`. The inlay-hint test and `check_foreach` (`:999`) read `inline_var` itself, which is empty either way when the statement has no tag. `inline_var_annotations` returns at once when `tags` is empty, and the two walks run only when `inline_var` is not.
- **E3.** `phpunit_method` (`checker/narrowing/phpunit.rs:86-116`) infers a method call's receiver (20,186 calls, 23.6 M) and finds its owner (6.1 M), or resolves a static call's class (8,324 calls, 11.1 M, ≈1,330 each) and looks up the method (1.9 M); a function call goes through `resolve_function` (12,674 calls, 3.6 M); before comparing the owner with `PHPUnit\Framework\Assert`. The name `assertion_check` reads is the written one, and all 52 names of its arms and its `assertis`, `assertisnot` and `assertnot` prefixes (55 literals in all) start with `assert` (`:127-258`). The gate goes at the entry: `apply_phpunit_assertion_narrowing` tests the written name of the statement's method or static call and returns early when it does not start with `assert` (`eq_ignore_ascii_case` of the first six bytes), which cannot change the result. `phpunit_method`'s arms stay ungated, because `constraint()` also calls it, with the `assertThat` factory names (`istrue`, `greaterthan`, `logicalnot`, ...), which never start with `assert`. The function-call arm stays, since `use function … as` can rename an assertion. A dynamic name is `DynamicMethodCall` or `DynamicStaticCall`, never probed today. Piwigo's tests make 4,553 `self::assert*` calls and 890 method calls named `assert*`, out of 33,777 static and 49,421 method calls in the analysed files (`analysis/ee/asserts.php`), so most static calls that statements make are real assertions and keep their cost. Once item 18.1 infers each node once, the receiver's inference becomes a lookup and E3 saves ≈12 M.
- **E4.** `read_and_parse_file` takes `cacheable = index < context_files.len()` (`session.rs:762`), because a hit gives `ast: None` and no ignore comments (`file_cache.rs:125-153`). A hit for an analysed file would read the entry, then parse with `parse_with_ignores` (`project.rs:2887-2914`) for the tree and the ignores, and skip the rest of `parse_file_with_names` (`:3003-3071`): the declarations, references, bodies' products and global facts, 548 M on Piwigo (B5). Over all 14,251 files those steps cost: `file_declarations::collect` 1,799.8 M, `file_references` (`deps::recorded_references`) 247.3 M, `FileBodies::of_file` (`BodySummary` 343.9 M, `InitFlow` 218.4 M, and their walk), `GlobalFacts::of` 218.5 M and `PestFacts::of` 6.2 M. The drill estimated the analysed share from the tokens before B5 measured it: the declarations ≈0.3 G (14-16% by count, 30% of the doc bytes), the bodies' products ≈0.2 G (33% of the body tokens), global facts and references ≈0.14 G, ≈0.6 G gross; and decoding at the stub cache's rates (≈35 K per class, ≈7.5 K per function) at ≈80 M for the classes and functions alone, before the bodies' products. It does not save the parse, the stub attributes or the anonymous classes' names, which change the tree, nor the line index (≈80 M), which the parse builds with the tokens; on a miss it adds the copy into `CachedFile::of`, the encoding and the write, as for a context file. Decoding 2,792 entries at the context entries' rate costs ≈0.25-0.3 G, so E4 nets ≈0.25-0.3 G, more once E12 leaves methods lazy. The key needs nothing new: it covers the bytes, the path, `is_stub` and `index_use_sites` (`file_cache.rs:164-178`). `class_fn_refs` also depends on `retain_source`, which the key does not hold, but only the language server sets it, and the server loads through `Project::load_files` (`project.rs:1142`), which never touches the per-file cache. A debug build asserts that a hit's products, built from the tree, encode to the entry's bytes. The F2 result cache would skip an unchanged file's check as well, and with it this cost; E4 is the part that needs no reasoning about dependencies.
- **E5.** The shortcuts at the top of `is_super_type_of` cost 45.6 M, ≈34 per call (`scalar_family` at `super_type.rs:646` 22.4 M, `is_scalar_value` at `:667` 12.5 M, the `mixed` and `never` test at `:77` 7.3 M, and the two shortcuts' call lines, `:81` 1.8 M and `:73` 1.6 M). The node's state word has 5 free flag bits: 4 for the family or the form (11 values) and 1 for "one scalar value" (`NAN` excluded), set in `Node::new` once per new node. The shortcuts then test bits, ≈10 per call. `is_form_of` is not helped. Other readers: `union.rs:436-437`, `super_type.rs:48`, `:170`, `:178`, `:583`, `:738`, `:759`.
- **E6.** `Type`'s `==` is `Arc::ptr_eq`, then `repr_eq` on equal hashes (`types/src/lib.rs:587-593`), and the relation tests `sup == sub` only at `super_type.rs:114`, after the memo or the registry-backed dispatch. Testing identity first saves ≈35-40 per structural pair and ≈100-110 per registry-backed one (§9.6). Yes is always right for one node against itself: the arms before `:114` already answer Yes for `Error` and two `NAN`s, and a subtracted `mixed~S` against itself, which may give Maybe today, becomes exact.
- **E7.** §9.6. Each plain-object call costs ≈340, all in `Type::new`: hashing, the front table's compare, dropping the owned form. `resolve_name_expr` (95,861 calls, 33.1 M, `checker/resolve.rs:13-28`) builds the type of `new Foo`, `instanceof Foo` and `Foo::` written in code; `record_call_throw_point` (`checker/throws.rs:49`, 5.5 M; its `Throwable` is built in the inlined helper `call_throw_type`, `:106`) and `instance_of_any` (`checker/operators.rs:701-705`, 4.5 M) build fixed types. `int_literal`'s calls come from code literals (`infer_expr_unguarded` 27,993, `check_expr_impl` 22,767) and constants' values (`infer_const_type_with` 17,329); the analysed files hold 23,463 integer literals, 357 distinct values, 21,578 of them 0-255 (`analysis/ee/ints.php`). `resolve_name_expr`'s type is kept as a `OnceLock<Type>` beside the entry item 4's ids index, built on first read; `record_call_throw_point`'s `Throwable` and `instance_of_any`'s fixed list become statics; `int_literal` answers −1..=255 from a static table made on first use.
- **E8.** §9.5. The decoders: `FunctionReflection`'s 213,295 calls (49.2 M), `Vec<Type>`'s 111,634 (18.4 M), and the nested types' ≈92 K; `Type`'s `Deserialize` is `Repr::deserialize(..).map(Type::new)` (`types/src/lib.rs:348-351`). A cache file holds a table of its distinct type nodes, children before parents, and a record holds an index where it holds a type today. Decoding interns each type once per file, and every other occurrence is an `Arc` clone. The per-file cache can share the format, which cuts E4's decoding too.
- **E10.** A body that calls `extract()` or `eval()` sets `saw_dynamic_var_construct`, and its end truncates every report made after its start (`function_decl.rs:1208`, `checker.rs:1806`). A nested function saves, clears and restores the flag (`function_decl.rs:885-886`, `:1211`), but its reports land after the enclosing body's start, so they are dropped too. The checker keeps a count of the enclosing bodies with the flag set; `report` and `report_builder` return at once while it is not zero, and `check_statement_var_tag_names` returns before formatting. `report` only pushes (`checker.rs:1487-1495`), so the output cannot change; a debug build reports as now and asserts that every report the count skipped lies inside a range the truncation drops (the truncation also drops reports made earlier in the body, before the flag was set, which the count never skipped). It applies wherever compiled templates call `extract()`: Latte, Blade, plain PHP views. The rest of such a body's check still runs, for the facts that outlive it.
- **E11.** `FileTable::get_by_origin` (`project/file_table.rs:108-114`) finds a file by the address of the `Arc<Path>` its declarations name it by, which `insert_parsed_file` binds once (`bind_origin`, `project.rs:1055`); a parsed file's declarations all share it (`project.rs:3017-3018`). A cached file's decoded origins are each a new `Arc<Path>` (`registry/provenance.rs:63`, `:129`), and `into_parsed` binds yet another (`file_cache.rs:146`), so every summary read falls back to hashing the path: 103,921 reads, 116.2 M. The entry stores a marker for "this file", and E8's decode context hands back the one `Arc` that `into_parsed` binds; that covers the lazily stored methods if their decode gets the same context. Otherwise a `FileTable` keyed by the path's bytes brings the fallback from ≈1.1 K to ≈0.1 K per lookup. A debug build asserts that each decoded origin spells the entry's path.
- **E12.** `insert_built_decl` calls `record_assert_tagged_names` (`registry/index.rs:153`, `:461-487`), which walks a class's `info.methods` and reads `sig.assert`, forcing the decode of each lazily stored method of a cached class: 51,750 methods, 419.2 M, ≈8.1 K each, on the main thread. Which of them a later step would read is hidden, because the insert decodes them first: the later decodes number 35,397, against 35,398 with the cache off, the stub bases' own. The check asks about 1,572 purity and 2,489 property-write callees (B1), so the methods it reads cost tens of M at most. `BuiltDecl::Class` carries the names of its methods with assert tags, computed when the declaration is built and stored by the cache. A debug build compares them with the walk's. It is also a wall-time cut: it is most of the both-warm run's ≈0.1 s insert (B4).
- **E13.** `store` calls `fs::create_dir_all` on the entry's shard for every entry (`file_cache.rs:240`): 11,473 `mkdir`s and 11,208 `statx` on a cold Piwigo run (§4.5), and one `getpid` per entry (`:245`). D5's check makes the build's directory and its 256 shards at the first store; making them when `directory()` first finds the cache on would cost every run 257 `mkdir`s, a fully warm one included. If a shard disappears during the run, the temporary's open fails and `store` skips that entry, as for any failed write.

### 10.14 Three defects in today's code

None is fixed. Each is found by the investigations, and each has its design below.

**E1. A cold stub build can deadlock on a rayon worker (C2).**
- **The locks.** Five process-wide values hold a lock while their first build runs: `plain_base` and `phpstan_base` (`LazyLock`, `project.rs:537-541`, `:1203-1206`), `built_stub_corpus` (`LazyLock`, `stubs.rs:1420-1429`), and `stub_index` and `generated_stub_data` (`OnceLock`, `stubs.rs:1431-1450`). Two kinds of parallel work run under them: the corpus's `par_iter` (`stubs.rs:1482`), and the bases' finishing passes, through `finish_loading` → `worker_pool().install` (`project.rs:1398`).
- **Two ways it fails.**
  - **Re-entry.** A worker that waits inside the build, in a join or in an `install` into another pool, runs other jobs of its own pool (§10.5). One that needs the same lock waits on its own thread. `OnceLock`'s documentation leaves re-entrant initialization unspecified, and the current implementation deadlocks.
  - **Starvation.** Waiters that are workers of a pool block in the OS. If every worker waits, and the build needs that pool, nothing serves the build. The comment at `incremental_updates.rs:1158-1162` describes this case.
- **Where it can happen.** Only when a base or the corpus is built, not loaded: with the stub cache cold or off. `resolved_stubs` returns before any parallel work when it loads the base from the cache (`project.rs:572-577`). Whether a run deadlocks also depends on timing: the worker has to steal a sibling job while its own work is still pending.
  - `edits_across_every_fixture_match_a_fresh_load` (`incremental_updates.rs:1155-1175`) runs its groups on the test's own pool, and each makes projects. When the first `cargo test --lib` after a source change runs cold and one of its workers is the first to force a base, its corpus `par_iter` and its `install` both steal sibling groups that need that base.
  - In `check-project`, `finishing_scopes` → `rayon::join` → `closure_scope` → `stub_index()` (`project.rs:1290-1296`), and the checker's `builtin_function_extension` (`checker/expr/builtin_calls.rs:36`), force the index on a worker when the bases load from the cache but `stub-index` does not (a damaged, unreadable or deleted file, or a failed write). Rare, since another worker usually steals the join's other half first.
  - The main thread, the language server's threads, `incremental-bench` and the fixtures' threads belong to no pool, so they cannot fail.
- **A deterministic reproduction**, designed and not run: a fresh test binary with `PHPSTAN_RS_NO_STUB_CACHE=1`, a one-thread pool, and `pool.install(|| (0..2).into_par_iter().for_each(|_| { Project::new(); }))`. The first item builds the plain base; its `install` from that worker takes the second item from its own deque (`wait_until_cold`, rayon `registry.rs:791`), which forces `plain_base` again on the same stack. It needs a watchdog (the body on a thread, failing when a channel's `recv_timeout` expires), since a cold debug build takes minutes. It was not run because another instance was building on the machine at the time.
- **The cause.** The stub state is five hidden statics that any code can force, at any depth, including pool jobs, and their first use builds while holding a lock.
- **The fix, in four parts:**
  - **S1. The stubs are a value, owned by the entry point and handed down; production code has no static.** Each entry point (`check-project`, the language server's `Server`, `incremental-bench`, the fixtures binary) makes one `Stubs` from a `StubSource` (`Cached { dir }` until proposal 6, `Embedded` after it, `Build` for the off mode and the reflection crate's tests), chosen there from the environment instead of in `cache_dir()` on every call. It has two layers: `Corpus` (the index and data) and `Base` (built from a corpus, plain or with PHPStan's source), so a base's own build project is given the corpus it is built from and never asks for the value being built. The free functions of `stubs.rs` become `Corpus` methods, and a `Registry` keeps an `Arc<Corpus>`. No project or registry holds the `Stubs` value: only `expect_files` and `finish_loading`'s late switch take `&Stubs`, from the entry points and the session. Those callers are `session.rs:791`, `:871`, `lsp.rs:428` (through `Project::load_files`, whose `expect_files` is at `project.rs:1157`), `lsp.rs:569` and `incremental_bench.rs:47`; the late switch is `sync_phpstan_stubs` (`project.rs:1180-1188`), for a project that gains a file mentioning `PHPStan\` after `expect_files`. Pool jobs get a `&Project` or a `&Registry`: the parse (`session.rs:748-781`, `project.rs:1148-1155`), the checker (`builtin_calls.rs:36`, through `Checker::registry`, `checker.rs:277`), `closure_scope`, `composer_hygiene` and `architecture`. So the type system keeps them from starting or waiting for a build. The partial-cache case goes away, and the stub cache becomes testable in one process.
  - **S2. Each layer is built on a thread of its own, and waiting blocks in the OS.** Making the value starts the corpus's build thread; a base's starts when a session first asks for it. `wait()` blocks without taking jobs, so re-entry is impossible anywhere, and no lock is held while anything parallel runs. A panic is carried to every waiter. The entry point makes the value before it lists the files, so the corpus builds beside the listing and the parse: proposal 5. That overlap holds only because the project gets its stubs at `expect_files`; the parse needs only the settings and the composer packages.
  - **S3. The builds' parallel work runs on the pool the entry point hands in**, so `PHPSTAN_RS_THREADS` and the stack size apply. Starvation would need every worker of that pool to wait for a build, and only the entry points and the session hold a `Stubs`. `wait()` `debug_assert!`s that its caller is not a worker of the build's pool, for the test value.
  - **S4. The base is chosen where the files say which one.** A project being loaded is its own type, with the settings, the packages and the file numbering and no registry. `expect_files(parsed, &Stubs)` turns it into a `Project` with the base its files call for: proposal 1. Checking a project with no base cannot compile.
  - **Tests** share one value behind a `test-support` feature that only the crate's dev-dependency on itself enables. The regression test makes its own `Stubs` with the source `Build`, in-process.
- **Rejected:** forcing the stubs where the project is made (a convention, not a type), a hop onto a plain thread only from rayon workers, waiting through `install` (it steals) or `yield_now` (the jobs it runs sit above the build's frame), building outside the lock (two threads could both build the ≈4.1 G corpus), and a once-cell that panics on re-entry (a crash instead of a hang).
- **With proposal 6.** A run then reads the stub data with serial code only: `Registry::load` and `StubIndex::load` decode in place (`registry.rs:741-751`, `stubs.rs:1237-1249`), and serial code cannot steal a job, so a lazy value over embedded data cannot deadlock. Builds remain in the off mode, the reflection crate's tests and the build script, so the fix holds before proposal 6 and after it.
- **After the crate split.** C4's memo levels are per-class `OnceLock`s whose re-entry panics, and C5's per-file parse memo is forced from pool jobs. E1's rule applies to both: nothing parallel runs under a lock a pool job can wait on.

**E14. The parser refuses `instanceof` with a static property as the class.**
- **What fails.** B1's Laravel run warned twice: "parse error: expected RParen, found DoubleColon" at `aws/aws-sdk-php/src/AbstractConfigurationProvider.php:41:41` (`$found instanceof static::$interfaceClass`) and `phpseclib/phpseclib/phpseclib/Math/BigInteger.php:177:31` (`$x instanceof self::$mainEngine`). `php -l` accepts both.
- **Why.** After `instanceof`, the parser takes an expression only when a variable or `(` follows, and otherwise a name (`syntax/src/parser/expr.rs:106-117`); the `::$name` is left for the enclosing rule. PHP's grammar takes a `class_name_reference` there, as after `new`: a name, or a `new_variable`, which includes `Name::$prop`, `$a->b`, `$a['k']` and `$a::$b`. `parse_new_class_reference` already reads that (`primary.rs:83-188`).
- **Reach.** A tokenizer scan of every file under `vendor/` finds six: the same two files in the installed dependencies of laravel, league-flysystem and monolog. Each loses every declaration as context today, so `phpseclib\Math\BigInteger` and `Aws\AbstractConfigurationProvider` are unknown classes in those projects.
- **The change.** Read `instanceof`'s right side with `parse_new_class_reference`: a plain name stays `InstanceofTarget::Name`, and anything else becomes `InstanceofTarget::Expr`, as a variable target does now. A parser test per form, and the two files as fixtures.
- **Open.** The parser has never been run over the whole corpus. One `phpstan-rs check` over the 111,836 files under `vendor/`, against D6's `php -l` verdicts, would list every file PHP accepts and the parser refuses.

**E15. The source hash misses PHPStan's `.stub` files.**
- **What is hashed.** `crates/phpstan-rs/build.rs` hashes `vendor/phpstorm-stubs`, `vendor/phpstan-src/src`, `Cargo.lock`, `Cargo.toml`, every crate's `src`, `Cargo.toml`, `build.rs` and `index.tsv`, and the target. The stub cache and the per-file cache are keyed by it.
- **What is read.** `crates/stub-sources/build.rs` also embeds `vendor/phpstan-src/stubs/*.stub` (41 files, 74 KB) as `PHPSTAN_STUB_SOURCES`, and reruns when they change. `patch_phpstan_stub_classes` (`registry/stubs.rs:735`) merges their templates, variances, bounds and property types into the stub classes the stub cache stores.
- **So** an edit to a `.stub` file alone, or a `phpstan-src` bump that changes only `stubs/`, rebuilds the binary with the same `PHPSTAN_RS_SOURCE_HASH`, and its runs read the old stub cache with no error. Master's `build.rs` has the same list (`:205-206`).
- **The change.** Add `vendor/phpstan-src/stubs` to the hashed inputs, and a test that the hash's inputs cover every path `stub-sources`' build script reads.

## 11. What remains

Applied in order, each row on what the rows above it leave. The columns assume proposal 6, so the stub build is gone before the plan's items apply, and the per-file cache cold or off, as in the profile. Measured figures replace the drill's estimates where the investigations measured them.

| | warm, G | off, G |
|---|---:|---:|
| today | 20.57 | 25.63 |
| proposal 6: the stub data built at compile time | 0 | −5.05 |
| the 20 G plan's items, at their own estimates | ≈ −8 | ≈ −8 |
| proposal 7, after item 14 (B2) | ≈ −0.04 | ≈ −0.04 |
| proposal 9, beyond item 4 | ≈ −0.3 to −0.4 | ≈ −0.3 to −0.4 |
| proposals 10 and 11 | ≈ −0.05 | ≈ −0.05 |
| E2, E3, E5-E8, E10, after the items they overlap | ≈ −0.2 | ≈ −0.2 |
| proposal 8 after the plan, at d ≈ 0.02 (B1), the files' verdicts cached | ≈ −2.0 to −2.3 | ≈ −2.0 to −2.3 |
| **left** | **≈9.6-10.0** | **≈9.6-10.0** |

- **The off and warm runs meet** once proposal 6 builds the stub data into the binary. After the crate split, proposal 6 saves ≈0.2-1.5 G instead of 5.05 G, so a cold run ends that much above the warm one, or nothing above it if the per-file cache holds the stub files' declarations (§10.6).
- **On a run whose files have no cached verdict,** proposal 8 saves ≈0.5-0.8 G instead, which leaves ≈11.1-11.5 G.
- **The off run without proposal 6.** Proposals 1-4 take ≈1.6 G instead of 5.05 G: 1 ≈0.39-0.41 G (C5), 2 after 1 ≈0.71-0.74 G (C3), 3 ≈0.29 G, 4 ≥0.2 G, less what 1 and 3 share in the plain base's finishing run. That leaves ≈24.0 G before the plan. After the plan, proposal 7 (≈0.19 G with the stub build, after item 14), proposals 8-11 and the cuts, ≈12.9-13.3 G, less what items 9-14 save on the stub parse, and plus up to ≈0.3 G of the plan's savings that an off run without proposal 6 never paid for, because it decodes no stub entries (§7: decoding 0 against 275.1 M warm): item 6 step 1's 178 M, item 4's `Members::eq` decodes (88.6 M) and the cheaper decodes of item 6's steps 3 and 5 (§9.5). A cold run gains proposals 1, 3 and 4 but not 2, since it keeps and writes the encodings (+58 M), so it ends ≈0.8 G above that, at ≈13.7-14.1 G.
- **With the per-file cache warm**, today's floor is 14.34 G (B5). E12, E4 and E11 take ≈0.7-0.8 G of it, and the plan's items apply as they do to the shared phases; that mode was not estimated item by item. Proposal 8 does not change it, since such a run parses no context file.
- **Wall time.** The stub build is 0.37 s of the off run's 2.68 s, and proposal 6 takes all of it; proposals 1-5 at most that. The finishing passes are the longest stretch after the parse in every mode, at 1-3 cores, so items 4-6 and proposal 9 weigh more in wall time than in instructions (§3.3).
- **The largest costs no item removes:**
  - the check's rule logic, ≈0.46 G over 1,181 functions (N1);
  - the type relation's and the interner's own work, which is lean per call (N2);
  - building the analysed files' declarations, which E4 removes on a run with the per-file cache warm (N3).

## 12. What this drill does not establish

The investigations settled most of what the drill left open: wall time (B4), a run with both caches warm (B5), the cache writes on disk (B6), d and proposal 9's reach (B1), proposal 7's saving (B2), the discarded reports (B3), the relation's operands (B7), proposal 6's compile-time cost (B9) and its equality across targets on the host (B10), and the split of the stub bases' finishing runs (C4, C5). The ledger (`PROFILE_DRILL_PIWIGO_LEDGER.md`) records what the later work settled: E1's fix, proposals 1-5 and 7-9, D5, E2, E3, E5-E7, E10 and E13 were built and measured in the off mode (its run register: 25.63 G to 20.93 G), E4, E12 and proposals 10 and 11 were measured and not kept, E14-E16 were fixed, and its questions Q-1, Q-3, Q-5, Q-6 and Q-7 answer the corpus run and the count of C7's errors, E10's dropped bodies, the 33 truncated reports and the readers left out of d below; Q-8's timings are STUB-10.1's to measure, and E1's reproduction (Q-4) and an on-target run (STUB-11) remain. What is still not established, as the drill left it:

- **Instructions are not time.** The allocator's and the copies' costs are memory-bound, so their weight in wall time is higher than in Ir. B4's wall times are of one machine.
- **No calling context** (§2). Shared functions are split by edge shares. The split of the stub bases' Ir and the plain base's finishing run (≈35-52 M, scaled from the measured 117.2 M) are estimates for that reason.
- **The savings of the cuts.** E2-E13 are computed from the profile and the code, not built. Proposals 3, 4, 10 and 11 likewise.
- **Nothing ran on aarch64 or macOS.** B10's equality rests on the host's evidence that no fold reads the platform; the on-target backstop is designed, not run.
- **The crate split's numbers are read, not run.** C8's reach comes from the names the files reference against the checked-in index, between bounds; what C5's locator loads in a real run lies between them. The build script's cost under C8's form (every stub file parsed, no base) is not measured.
- **Readers left out of d.** With the uninitialized-property rules on, or on a tree with test classes whose built-in parent-call hook rules fire, the ancestors' bodies they read add to d (B1).
- **Which truncation holds the 33 `offsetAccess.mightNotExist` reports** that `finalize` does not drop (B3 counts them among the discarded reports but not by site).
- **The parser against the whole corpus.** E14 came from two warnings; a run of the parser over all of `vendor/` against `php -l` is not made. Which of C7's 238 errors the checker already reports was not counted.
- **E1's reproduction** is designed and not run.
- **E10's dropped bodies.** Which facts that outlive an `extract()` body a dropped body still needs is not traced.
- **B9's embed and edit timings** were read off the console and not kept.

## 13. Reproduction

All paths are under `~/.cache/prs-profile/callgrind-20g-modes/` unless marked. Scripts in `~/.cache/prs-profile/drill-scripts/` are named with that prefix.

- **Pickles.** `drill-scripts/parse.py <dir> <out.pkl>` (13 s per run) gives `off/off.pkl` and `warm/warm.pkl`. `drill-scripts/q.py <pkl> self|incl|find|callers|callees|lines|tree|thread <pattern>` queries them, and `drill-scripts/selflines.py <pkl> <pattern> <N> exact` gives self cost by line; it needs the function's full name.
- **Phases.** `python3 analysis/attrib.py off/off.pkl analysis/off-attrib.pkl`, and the same for `warm`. The output also lists the global pool's threads, whose finishing-pass frames go to the stub sources (§2).
- **Blocks.** `python3 analysis/blocks.py off/off.pkl analysis/off-attrib.pkl <phase> analysis/bl/<phase>.txt 12 <tag>` prints the blocks (saved as `bl/<phase>-off.txt` and `-warm.txt` by redirecting stdout; `bl/<phase>.txt` holds the block definitions) and writes the per-function splits to `bl/res-<tag>.pkl`.
- **Lookups and decoding carved out** (§4.4, §6, §7): the block files `bl/find/find.txt`, `bl/find/decode.txt`, `bl/find/finishing-find.txt` and `bl/find/check-fd.txt`. Their outputs are `bl/find/ff-*.txt` and `bl/find/cfd-*.txt`.
- **The statements block's split** (§6): `python3 analysis/stmtsplit.py analysis/bl/res-check.pkl v`, and the same for `res-check-warm.pkl`. Its rest, classed by owner: `analysis/nn/n1split.py` (`n1split-warm.txt`, `n1split-off.txt`).
- **Subsystems** (§3.2, §4.2): `analysis/groups.py off/off.pkl warm/warm.pkl`. The main thread's split applies the same rules to the `selft` entries of thread `01`.
- **Tokens** (§5, §10.8): `php analysis/n5/bodies.php <list>` over `analysis/n5/vendor.txt` (the classmap files under `vendor/`) and `analysis/n5/analysed.txt` (the `.php` files under the configured paths). The outputs are `vendor.out` and `analysed.out`. N3's, N4's, E3's and E7's counts come from `analysis/nn/decls.php`, `docs.php`, `preg.php` and `analysis/ee/asserts.php`, `ints.php`.
- **Descriptions** (§10.7): `python3 -I analysis/n4/desc.py <roots>`.
- **Cache sizes** (§10.6): `analysis/n1/sizes.txt`, from `zstd -19` and `gzip -9` over `stubcache/*.cbor`.
- **Output identity**: `cmp off/out.json warm/out.json` and `cmp off/out.json cold/out.json`.
- **Messages dropped by level** (§9.2): `report_missing_offset`'s calls by line, from the warm pickle's `fnline` and `callline`.
- **The ledger's items.** The ledger (`PROFILE_DRILL_PIWIGO_OFF_LEDGER.md`, 1,098 lines) is retired: its last version is in git at `72cac936` (`git show 72cac936:PROFILE_DRILL_PIWIGO_OFF_LEDGER.md`), and the ids D, B, C, E and N in this report are its item ids. Each result is carried in the sections that cite the id; the table gives each item's method and where its data is. Paths are under `~/.cache/prs-profile/off-ledger-b/` (`b/`), `~/.cache/prs-probe/` (`p/`) and `callgrind-20g-modes/analysis/` (`a/`). `p/validate/notes.md` holds the item-by-item check of 2026-10-14. Probe builds are of a worktree at `2a7befd3` (`b/probe.patch`), not for commit, and ran in the off mode on Piwigo, with output byte-identical to the profile's.

  | item | method | data |
  |---|---|---|
  | D1 | php 8.5.4 run on files with a parse error, a compile-time error and a modifier error, loaded by `include`; the parser's only failing step found by reading `project.rs` and `syntax/src/parse.rs`; the saving of a checking-only pass from the profile's parser block | `p/syntax-in-body/`, `p/validate/d1/` |
  | D2 | read the load path for the bytes it already holds; the share of body text from B1's probe | `b/b1/` |
  | D3, D4 | read the crate split's plan and the build scripts; D3's reopening by C8 | `p/c8/`, `p/validate/CRATE_SPLIT_PLAN.master.md` |
  | D5 | read `file_cache.rs`, `stub_cache.rs`, `session.rs` for the failure paths of a store | none |
  | D6 | php 8.5.4 autoloading a file with a broken body, per kind of use; `php -l` over every PHP file under `vendor/` (111,836 files, 272 fail), classed by hand; Psalm's `StatementsProvider` read | `p/d6/` (`kinds.php`, `main.php`, `php-l-fatal.tsv`, `php-l-class.tsv`, `upstream/`) |
  | D7 | read the crate split's progress (`gates/module-graph.txt` on master) and the cold-run paths | none |
  | B1, B2, B3 | the probe patch answers every purity and property-write question a second time by a search on demand, records every body read, counts what the tag lexer lexes against what a pulling parser would, and records what the truncations drop; on Piwigo and on Laravel (`vendor/laravel`, `check-project src`); `triggers.php` counts the skimmer's full parses with `token_get_all` | `b/probe.patch`, `b/b1/{piwigo,laravel}/` (`level.txt`, `b1-b3.txt`, `stderr.txt`, `context-files.txt`), `b/b1/triggers.php` |
  | B4 | gate binary of `4db8f996` (source hash `558a2b69…`) in six cache modes, round-robin, 8 rounds, each starting at the next mode; medians; perf recordings | `b/b4/scripts/interleave.py`, `b/b4/perf/` |
  | B5 | the profile's `fp/dhat` binary under callgrind on Piwigo, with a cache filled by two earlier runs; output compared with the profile's; cache mtimes checked | `b/b5/` |
  | B6 | strace and perf on a cold per-file-cache run against the stub-warm recording, classed by `classify.py` | `b/b6/`, `b/b4/perf/stubwarm.data`, `p/validate/b6-classify.txt` |
  | B7 | probe build counting, at the top of `is_super_type_of`, its operands, shortcut hits and identical pairs; one run | `b/b7/piwigo/` |
  | B8 | counted with PHP's tokenizer; settled in E7 | E7's data |
  | B9 | a worktree at `3324bacd`, builds under `nice -n 19` with `/proc/loadavg` logged, at opt-levels 0, 1 and 3 and with the build override; a three-crate toy workspace for cargo's unit sharing; `cargo build --timings`. The gate build's 9 min 36 s is from a log that did not record the load | `b/b9/` (`share/`), `~/.cache/prs-b/gate-build.log` |
  | B10 | B7's probe logging every `powf` of constant folding on a two-file project and on Piwigo; the stub sources searched for `**` with the tokenizer; the crates searched for platform-dependent code; four-file build at `PHPSTAN_RS_THREADS` 1, 2, 3, 8, `RAYON_NUM_THREADS=1`, `taskset -c 0` | `b/b10/` |
  | C1 | read `syntax/src/lexer.rs` and `parser.rs` for state across a token boundary | none |
  | C2 | read rayon-core 1.13.0's `registry.rs` and the stub-build lock chain; the scenario traced by hand, not run | none |
  | C3 | a script over the stub cache container's section directory and each table's 16-byte records (`container.rs`, `name_table.rs`), per table, for both bases | `p/validate/c3/` (`tables.py`, `calls.py`, `calls_by_name.py`), over `callgrind-20g-modes/stubcache/*.cbor` |
  | C4, C5 | per-thread call counts read from the raw callgrind files; closure counts matched to the two bases' class and function counts | `off/cg.out-NN` under `callgrind-20g-modes/`, `q.py` |
  | C6 | read `project.rs`, `session.rs`, `lexer.rs`, `syntax`'s manifest | none |
  | C7 | every compile-error site in php-src 8.5.4 listed, one trigger file per site (299 files), each run with `php -l` and with `echo "RAN\n";` first, against the parser; 13 of `zend_inheritance.c`'s 54 sites triggered, the rest read | `~/.cache/php-src/src-8.5.4`, `p/c7/` (`cases/`, `scripts/run.py`, `verdicts.tsv`, `verdicts_run.tsv`), `p/syntax-in-body/goto.php`, `p/validate/c7-accepted.tsv` |
  | C8 | no build: the names each project's files reference (PHP's tokenizer), closed over the checked-in index to a lower and an upper bound; the context files as `composer/src/vendor.rs` discovers them; the crate split's plan read; cargo's cyclic-dependency rejection on a two-crate workspace | `p/c8/` (`scripts/names.php`, `reach.py`, `context.php`, `cyc/`), `p/validate/c8/` (`gates.php`, `reach_lib.py`), `crates/stub-sources/index.tsv` on master |
  | E1 | read every lock that holds a build and every caller that forces it; a deterministic reproduction is designed, not run | none |
  | E2, E4, E5, E6, E8, E11, E12 | callers, call counts and Ir read from the pickles (`q.py incl\|callers\|callees\|lines`); the code at the cited lines read; E6's counts from B7 | `callgrind-20g-modes/{off,warm}/*.pkl`, `b/b5/`, `b/b7/` |
  | E3, E7 | as above, with the analysed files' calls and integer literals counted with PHP's tokenizer | `a/ee/asserts.php`, `a/ee/ints.php`, `a/n5/analysed.txt` |
  | E9 | the regex scanner's calls and `format!` costs from the pickle; the 170 patterns from N4's count | `a/nn/preg.php` |
  | E10 | the reports the truncations drop, from B3's probe; the flag's scope read in `function_decl.rs` and `checker.rs` | `b/b1/` |
  | E13 | syscalls counted under strace on a cold per-file-cache run | `b/b6/` |
  | E14 | B1's Laravel warnings; the parser's `instanceof` rule read against PHP's grammar; a tokenizer scan of every file under `vendor/` | `b/b1/laravel/stderr.txt`, `p/validate/d6/instanceof_sp.php`, `instanceof_sp.tsv` |
  | E15 | `build.rs` of `phpstan-rs` and of `stub-sources` compared; the same lists read on master | none |
  | N1 | the statements block's rest classed by owner function from the pickles; per-rule costs | `a/nn/n1split.py` (`n1split-warm.txt`, `n1split-off.txt`), `a/stmtsplit.py` |
  | N2 | calls, self and inclusive Ir of the relation and the interner from the warm and off pickles | `callgrind-20g-modes/{off,warm}/*.pkl` |
  | N3, N4 | declarations, docs and body tokens of the analysed files counted with PHP's tokenizer; the `preg_match` sites and their distinct patterns counted | `a/nn/decls.php`, `docs.php`, `preg.php` over Piwigo's file lists |
