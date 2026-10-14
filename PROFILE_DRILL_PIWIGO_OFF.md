# Callgrind drill-down of the off run: no stub cache, no per-file cache

Profile: `~/.cache/prs-profile/callgrind-20g-modes/off/` (`cg.out-01..17`, 17 threads), **25,625,190,065 Ir**.
- **Command.** `check-project --config phpstan-rs.toml --format json`, run in `/home/torres/piwigo17-frozen` with `PHPSTAN_RS_NO_FILE_CACHE=1 PHPSTAN_RS_NO_STUB_CACHE=1`.
- **Binary.** `callgrind-20g-modes/fp/dhat/phpstan-rs`: the `dhat` profile with frame pointers, built at `d520c9fd` on branch `callgrind-20g`. Its code is that of `5fb858c9`, because the commits between them change only documentation.
- **Machine.** 8 cores.

**Sources.** Nothing was built or run for this report.
- Every figure comes from the callgrind outputs of three runs of that binary (off, cold and warm, §1) and from the code at `d520c9fd`.
- The token counts of §5 were taken with PHP's own tokenizer (`token_get_all`) over Piwigo's files.
- The cache sizes of §10.6 were taken with `zstd` and `gzip` over the cold run's cache files.

**Conventions.**
- "M" is 10^6 Ir and "G" is 10^9 Ir.
- Percentages are of the off run's 25.63 G unless marked.
- "Item n" is an item of the 20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`, Part II), and "proposal n" is one of this report's (§10).

## Summary

- **The stub build is the whole difference.** The off run costs 5.05 G more than the warm run, and all of it is the stub build:
  - 3.50 G parses and indexes the 2,515 stub files on a second rayon pool;
  - 1.60 G runs serially on the main thread before the parse, while every other core is idle.

  The cold run is the off run plus the cache write (58 M). The stub cache is keyed by the build's source hash, so the first run of every build is cold.
- **The rest matches the warm run.** The other 20.57 G is the warm run, which the 20 G report drilled. The off and warm runs agree phase by phase, apart from the stub build, within 0.12 G, and their diagnostics are byte-identical.
- **The 20 G plan reaches every block of the shared work.** It covers each block at least in part, and its items estimate ≈8 G together. No item removes:
  - the check rules' own work, ≈1.2 G;
  - the type relation's and the interner's own work, ≈0.5 G;
  - building the analysed files' declarations.
- **Library bodies are 47% of the parsing.** That share of what the parse job lexes and parses is function bodies in context files, and their trees are dropped once their summaries are taken (§5).
- **Eleven proposals**, each checked against the code and the data, and three ideas rejected (§10). The "runs" column below says which runs gain:

| # | proposal | saving | runs | needs |
|---|---|---|---|---|
| 1 | Resolve only the stub base the files need | −355.1 M serial, and one of the two stub-base finishing runs (≈0.2 G together) | off and cold, when a loaded file mentions PHPStan | — |
| 2 | Serialize only what a store keeps | −1,005.3 M serial; −0.65 to −0.74 G after 1 | off | — |
| 3 | A hash index for stub bases built in memory | ≈ −0.29 G | off and cold | — |
| 4 | Look each stub file's dependents up once in `build_closures` | ≥ −0.2 G serial | off and cold | — |
| 5 | Build the stub corpus beside the parse | wall time: ≈7/8 of the corpus's 0.62 G of serial work | off and cold | the corpus on the worker pool |
| 6 | Build the stub data at compile time, into every binary | −5.05 G, down to the warm run's cost | off and cold | the crate split's reflection layer (§10.6) |
| 7 | Tokenize a PHPDoc tag's description only when it is read | ≈ −0.1 G warm, ≈ −0.34 G off; about half of that after item 14 | all | — |
| 8 | Skim the function bodies of context files, and parse one when something reads it | after the plan, ≈ −2.0 to −2.3 G × (1 − d) once a file's parse verdict is cached, where d is the share of bodies read; on a run that must check the bodies, ≈ −0.6 to −0.9 G + 2.0 to 2.3 G × d, nothing from d ≈ 0.26-0.45 | per-file cache cold or off | proposal 9; a parse-verdict cache (§10.8) |
| 9 | Purity and property writes on demand | ≈ −0.3 to −0.4 G beyond item 4, less what is asked for; it runs on the finishing thread | all | item 4's ids |
| 10 | Check UTF-8 with SIMD | ≈ −50 M | all | — |
| 11 | Make text only when it is read: a reported message when it is displayed, a constant key only when a dynamic constant is configured | −29.1 M | all | — |

- **Rejected** (§10.12):
  - one rayon pool, as a speed-up by itself (it is part of proposal 5);
  - a narrower stub-cache key;
  - a dedicated reader for Composer's autoload file.
- **Where it ends.** With the plan and proposals 6-11, both modes come to ≈10.3-10.6 G (§11). Without proposal 6, proposals 1-4 take ≈1.5-1.7 G off the stub build instead.
- **Decided** (§10.6, §10.8, §10.2):
  - the stub data is built at compile time;
  - a skimmed body is still checked for syntax errors, by a pass that builds only what its checks read, and each file's verdict is cached;
  - a skimmed body's text is kept from the bytes the load reads;
  - a cache checks once whether it can write, apart from reading.
- **Still open.** `PROFILE_DRILL_PIWIGO_OFF_LEDGER.md` tracks what needs a decision or a look. The largest:
  - the callees that proposal 9's demand reaches, which also sets d, and so whether proposal 8 pays on a run that must check the bodies;
  - proposal 7's measured saving;
  - what building the stub data at compile time costs a build.

## 1. The runs

| mode | stub cache | per-file cache | Ir | exit |
|---|---|---|---:|---|
| off | disabled (`PHPSTAN_RS_NO_STUB_CACHE=1`) | disabled | **25,625,190,065** | 0 |
| cold | enabled, its directory (`PHPSTAN_RS_CACHE_DIR`) empty | disabled | 25,683,423,164 | 0 |
| warm | enabled, filled by the cold run | disabled | **20,572,181,579** | 0 |

- **The warm run is the 20 G report's run**, to within 0.01%.
- **The diagnostics are identical.** `off/out.json`, `cold/out.json` and `warm/out.json` are byte-identical (`cmp`).
- **What the off and cold runs do.** The off run builds the stub bases in memory and drops their encodings (§4.3). The cold run does the same work, keeps the encodings and writes them: 58 M more.
- **Every new build starts cold.** Each build has a new source hash (`PHPSTAN_RS_SOURCE_HASH`), so its first run is cold. A gate or a benchmark on a fresh build pays the stub build at least once.
- **The per-file cache was off in all three runs**, since `stubcache/` has no `files/`. A run with it warm parses no context file (`project/file_cache.rs:73-95`), and this profile cannot show such a run.

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

  So the stub build comes before the parse, and its serial part runs while every other core is idle.

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
- **No calling context.** Callgrind's edges carry none, so a function that two roots call is split by edge shares, not by the calls each root made. This matters in two places:
  - **The stub bases.** `resolved_stubs` serves both bases. Attribution gives 603.7 M to the plain base and 396.6 M to PHPStan's. The inclusive costs under each base's `LazyLock` are 355.1 M (the plain base's 983.4 M, less the 628.3 M corpus it builds) and 646.1 M. Their total, 1,000.3 M, is right either way, so sums use the attribution and the split uses the inclusive costs.
  - **The warm run's stub decoding.** It is 477.8 M by attribution (275.5 M in the finishing passes and 202.3 M in the check) and 439.0 M inclusive.
- **Inclusive costs** are quoted only for a single callee, never added up, because a call's cost counts under every caller on its path.
- **Verdicts.** For each block, the code its functions run was read, and the design of the plan item was checked against those lines:
  - **covered**: the design reaches the block's cost;
  - **partly**: it reaches some of it, and the rest is named;
  - **new**: no item reaches it, and a proposal does.
- **Savings.** For plan items, the savings quoted are the plan's own estimates. For the proposals, they are computed here from the profile, the code and the token counts, and each says what it rests on.

## 3. Where the Ir goes

### 3.1 By phase

| phase | off, M | warm, M | runs on | removed by |
|---|---:|---:|---|---|
| parse job | 9,994.0 | 9,983.0 | worker pool | items 2, 9-14; proposals 7, 8, 10 (§5) |
| check | 6,832.2 | 6,774.7 | worker pool (45 M on the main thread) | items 7, 14-18; proposals 3 (off), 11 (§6) |
| stub sources | 3,495.7 | 0 | global pool | proposal 6; without it, items 9-14 and proposal 7 lower it (§4.1) |
| finishing passes | 2,361.1 | 2,475.0 | worker pool, mostly one thread | items 4-6, 17; proposals 3, 9 (§7) |
| stub bases, plain and PHPStan's | 1,000.3 | 15.2 | main thread | proposal 6; without it, proposals 1, 2, 3 (§4.2) |
| serial insert | 739.8 | 751.2 | main thread | items 3, 5; proposal 8 (§8) |
| stub corpus merge | 431.9 | 0 | main thread | proposal 6; without it, proposals 4, 5 |
| Composer vendor discovery | 321.2 | 321.3 | its own thread, off the critical path | item 2 (§8) |
| stub corpus stores | 190.7 | 0 | main thread | proposal 6; without it, proposals 2, 5 |
| session load, less the above | 169.3 | 164.1 | main thread | item 2 (§8) |
| `main` | 52.2 | 52.2 | main thread | — |
| session run | 32.9 | 32.9 | main thread | item 2 |
| reached by no root | 3.6 | 2.6 | | |
| **total** | **25,625.2** | **20,572.2** | | |

The off run runs the finishing passes three times (for the two stub bases and for the project), and the warm run once. The warm run decodes stub entries during its run instead (§4.4).

The 20 G report's own figures for the warm run are: parse 9.97 G, finishing 2.48 G and check 6.72 G. Those are this table's warm column, with the check counted on the worker pool only (6,729.3 M).

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

The largest self costs of the off run (all but `Base::find` are as large in the warm run):

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

`built_declaration` runs 32,450 times in the off run (2,937 M inclusive) against 15,127 times in the warm run (1,668 M). The 17,323 extra calls are the stub declarations, and they cost 1,269 M:
- `ClassReflection::from_def` costs 2,313 M over 16,164 calls in the off run, against 1,520 M over 12,546 calls in the warm run;
- `FunctionReflection::from_decl` costs 507 M for the 7,854 stub functions.

By block (attributed, `bl/stub-off.txt`):

| block | M | what removes it, without proposal 6 |
|---|---:|---|
| docblock | 930.5 | item 14; proposal 7 (84.4% of the stubs' tag tokens are descriptions) |
| parser | 822.1 | items 9, 10 |
| lexer | 662.1 | items 10, 12 |
| `from_decl` | 229.1 | items 8, 14 |
| `deps::referenced_names` | 168.1 | item 13.7 |
| `index_and_flatten`'s own work and its per-file passes | 141.3 (80.9, and 60.4 run as stolen jobs) | item 4 |
| `build_declarations`' walks | 135.2 | item 13 |
| `from_def` | 101.3 | items 4, 8, 14 |
| line index | 93.4 | item 11 |
| tree drop | 79.0 | item 19 |
| global facts, stub attributes, attribute mentions, the closure's own frames | 133.4 | only proposal 6 |

Nothing of this is reused between builds, because the cache's key is the whole build's source hash (`registry/stub_cache.rs:17`, `:22`).

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

The stores are four fifths of the bases' cost.

The bases' finishing passes run on the worker pool (`finish_loading`, `project.rs:599`), at ≈0.2 G for the two (§4.4).

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
- **The serialization itself.** Its self cost (postcard, serde, `Writer`, `Span`) is 493.2 M on the main thread, and nothing on any other thread. The warm run pays 2.5 M of it.
- **The encodes.** Inclusive: `ClassReflection` 598.1 M over 13,569 calls, `FunctionReflection` 526.3 M over 130,517 calls.
- **The cold run** pays the same encoding and keeps it, which is why it costs only 58 M more than the off run, not 0.5 G more.

### 4.4 The worker pool: −44.5 M, by phase

| phase | off, M | warm, M | difference | what differs |
|---|---:|---:|---:|---|
| parse | 9,994.0 | 9,983.0 | +11.0 | the same files |
| check (pool) | 6,787.1 | 6,729.3 | +57.9 | **off:** 127.0 M bisecting the name tables of bases built in memory (`bl/find`: 142.9 M against 15.9 M). **Warm:** 113.8 M decoding the stub entries the check reads first, outside the lookups (202.3 M with the decodes under other blocks). Every other block agrees within 32 M (§6) |
| finishing | 2,361.1 | 2,475.0 | −113.9 | **off:** +193.6 M of bisection (238.5 M against 44.9 M), and ≈+211 M for the two stub bases' runs, spread over the passes. **Warm:** decoding 275.1 M, the deferred-value pass's extra 92.3 M, `Members::eq` 53.5 M, and ≈98 M of stolen jobs that land outside the named blocks (§7) |
| other | 1.6 | 1.1 | +0.5 | |
| **total** | **19,143.9** | **19,188.3** | **−44.5** | |

So:
- the warm run pays ≈0.48 G to decode stub entries lazily;
- the off run pays ≈0.32 G more for lookups in bases built in memory (proposal 3);
- the off run also pays ≈0.2 G for the two stub bases' finishing runs.

The ≈211 M is the sum of the finishing blocks that cost more in the off run once lookups and decoding are carved out:
- member owners +48.4 M;
- PHPDoc inheritance +44.5 M;
- purity +39.0 M;
- dropped doc members +27.5 M;
- constant types +16.8 M;
- ancestors +14.4 M;
- traits +10.2 M;
- classification +7.8 M;
- the rest +2.8 M.

It is an estimate: stolen jobs move cost between blocks. It agrees with the inclusive figures: 2,186.3 M for three runs in the off run, against 1,995.7 M for one in the warm run, which also decodes.

## 5. The parse job: 9,994 M

**Whose files they are.** The job reads 14,251 files: 2,792 analysed and 11,459 context files, whose trees the insert drops (§8).
- **2 context files** come from `scan_paths` (`phpstan-stubs/`).
- **The other 11,457** come from Composer's autoload data:
  - Discovery reads `autoload_static.php`: its `classMap` and `files` entries are files, and its `prefixDirsPsr4` directories are walked for more (`composer/src/vendor.rs:94-116`).
  - Composer's `ClassLoader.php` is added, since no map lists it (`:77-80`).
  - The classmap and `files` entries are 12,391 distinct files: 12,197 and 196, as `php` reads `autoload_classmap.php` and `autoload_files.php`, which list what the static file holds. Less the analysed files, 10,610 remain, all under `vendor/` (`analysis/n5/vendor.txt`).
  - The other 847 context files come from the PSR-4 walks and `ClassLoader.php`.

**Most of the code is context files' function bodies.** PHP's tokenizer, over those 10,610 files and the 2,792 analysed ones (`analysis/n5/bodies.php`):

| set | files | tokens, less whitespace and comments | of them inside function and method bodies | bytes inside bodies |
|---|---:|---:|---:|---:|
| context (classmap and `files` entries) | 10,610 | 3,854,257 | 2,599,131 (67.4%) | 17.4 MB of 34.4 MB (50.7%) |
| analysed | 2,792 | 1,732,841 | 1,384,375 (79.9%) | 12.8 MB of 19.1 MB (67.2%) |

- **Their share of the job.** Scaled to all 11,459 context files at the same density, their function bodies are ≈2.8 M of the ≈5.9 M tokens: **≈47% of everything the job lexes and parses.**
- **What is kept of a body.** A context file's tree is dropped at the insert, and of a body only its summary and its flow are kept (`registry/bodies.rs:47-55`).
- **Who pays for it.** A warm per-file cache keeps those products, and with it a run parses no context file (`ast: None` in `into_parsed`). So the bodies' share is paid by every run whose per-file cache is cold or off:
  - the first run of each build, because the cache is keyed by the build (`file_cache.rs:68`);
  - every run of a CI job that does not keep the cache directory.

| block | off, M | warm, M | what it is | removed by | verdict |
|---|---:|---:|---|---|---|
| parser | 3,647.4 | 3,637.9 | `parse_expr_bp_inner` 501, `fill` 455, `skip` 255, `take_token` 171, `parse_primary` 160, memcpy 108, `Token` drops 102 | items 9 (0.8-1.2 G) and 10; proposal 8 | covered; proposal 8 adds to it |
| lexer | 2,935.5 | 2,937.4 | `next_token` 664, `next_tok` 470, `next_raw` 395, `name_and_bits` 281, `lex_ident_span` 173, `lex_operator` 166, `Keyword::try_from_bytes` 127 | items 10 (≈1.7 G with the parser's side) and 12 (≈0.35 G); proposal 8 | covered; proposal 8 adds to it |
| docblock | 808.1 | 826.4 | `docblock::parse_shared`: the tag lexer, trims, memo probes | item 14 (≈0.35 G); proposal 7 | partly: descriptions are tokenized for one reader |
| `from_decl` | 451.4 | 447.0 | building function signatures | items 8 (≈0.1 G), 14 (its docblock callee), 4 (allocations) | partly; the rest is the declarations' own work |
| `from_def` | 419.7 | 414.7 | building classes | items 4, 8, 14 | partly (below) |
| body summary | 343.9 | 343.2 | `BodySummary::of` | item 13.3 for the walk; proposal 8 for context bodies | partly: one walk saves only the walk frames, ≈215 M across this block and the next three |
| line index | 244.9 | 244.8 | `LineIndex::new` 105, `Utf8Chunks` 71, memchr 63 | item 11 (≈0.2 G): line starts by memchr, columns where read | covered |
| global facts | 218.5 | 218.5 | `GlobalFacts::of` | item 13.3; proposal 8 | partly |
| init flow | 218.1 | 217.5 | `InitFlow::of` | item 13.3; proposal 8 | partly |
| `file_decls` | 204.2 | 203.8 | the declaration walk | item 13.1 | covered |
| recorded references | 167.2 | 149.6 | `deps::recorded_references`, doc names | items 13.7, 14.6 | covered |
| read | 81.9 | 88.3 | `from_utf8` 66.5 M over 14,251 files, `fs::read` | proposal 10 | new, small |
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

With the per-file cache warm, declarations are built only for the analysed files (`CachedFile.built_decls`, `project/file_cache.rs:86`), and `from_def` costs less by an amount this profile cannot show.

## 6. The check: 6,832 M

| block | off, M | warm, M | what it is | removed by | verdict |
|---|---:|---:|---|---|---|
| statements | 2,879.3 | 2,850.1 | `check_stmt_impl` and below, less the blocks named here (split below) | items 15, 16, 18.1, 18.3, 18.6, 18.7, M10; proposal 3 | partly: ≈1.2 G is the rules' own work |
| inference | 536.0 | 545.3 | `infer_expr` | item 18.1 (one inference per node) | covered |
| type relation | 522.2 | 504.7 | `Relation::*` | item 7 (≤0.3 G) | partly: the relation's own work stays |
| interning | 382.8 | 369.9 | `types::intern` | items 16, 18.6 | partly: ≈150-200 M of hashing and equality stays |
| method return types | 324.1 | 328.8 | `method_call_return_type` | items 15, 18.9 | partly |
| `check_files` | 308.5 | 314.5 | per-file setup, `Path` hashing 24.2 M, path compares, `check_program` 20.2 M | items 2 and 5 (path data from the walk, files by id); item 18.10 (one checker per worker) | covered |
| prewalks | 301.8 | 301.5 | `Promotions`, `Members`, `global_constants`, the include and `declare` walkers | item 18.1 (observer families) | covered |
| class lookup | 256.0 | 244.8 | `lookup_class`, `ClassOwners` | item 15 | covered |
| function body setup | 242.3 | 245.1 | `check_function_impl` | items 18.8, 18 F1 | covered |
| docblock | 224.3 | 240.4 | `docblock::parse_shared` called from the checker | item 14; proposal 7 | partly |
| format | 161.4 | 161.7 | `format_inner` | items 14, 18 F1; proposal 11 (§9.2) | covered, except ≈13 M |
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
| name tables: `Base::find` 53.6, `contains_key` 36.9 across three tables, `Keys::bytes` 13.6 | 115.0 | 60.9 | proposal 3 for the off run's bisection; item 15 |
| allocator, `mem*`, drops, growth, clones of vectors and strings | 392.3 | 403.4 | the items M10 assigns |
| the dispatch's own frames: `check_expr_impl` 230.2, `check_stmt_impl` 84.2, `check_expr_cached` 38.4, `check_stmt` 15.0 | 367.8 | 367.9 | item 18.1 removes the re-inference; the dispatch itself stays |
| `resolve_call_args` | 30.5 | 30.5 | item 18.7 |
| the rest: the rules' own code, type operations, registry reads | 1,211.9 | 1,224.9 | spread; the largest are listed below |

- **The names row.** Most of its `memrchr` and `rsplit` is the function-call arm dispatching by name (`checker/expr.rs:817-966`). That arm makes seven `rsplit('\\')` calls and three lowercase copies per call, and item 15 names it.
- **The largest in the rest:**
  - `lookup_method_with_owner` 40.9 M (item 15);
  - `intern::remembered` 20.9 M and `Type::new` 16.2 M (item 16);
  - `var_tag_nodes` 16.2 M (item 14);
  - `LineIndex::line_col` 13.6 M (item 11);
  - the union and traversal functions, ≈60 M (items 18.5, 18.6);
  - the property-write table's probes, 9.4 M (item 4);
  - `expr_side_effects`, 8.2 M (item 18.2).

  About 1 G is spread over functions under 7 M each: the rules themselves.

## 7. The finishing passes: 2,361 M

The off run runs the passes three times (for the two stub bases and the project), and the warm run once. In this table the name tables' lookups and the decoding of stub entries are carved out of the passes (`bl/find/ff-*.txt`), so each row compares the same work.

| block | off, M | warm, M | what it is | removed by | verdict |
|---|---:|---:|---|---|---|
| purity | 660.4 | 621.4 | `resolve_transitive_purity`. Its 259.3 M of self cost is case-folded hashing of `DeclKey`s (the ASCII case fold at `num/mod.rs:863` 91.1 M, FxHash's multiply and rotate ≈57 M). The `DeclKey` maps cost 67.3 M and 38.8 M, and `DeclQuery::eq` 34.0 M | item 4 (ids: ≈0.5 G of string keys across this row, property writes and classification); proposal 9 | covered by item 4; proposal 9 removes the pass |
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

- **The bottleneck thread.** These passes run on the thread that the 20 G report names as the run's wall-time bottleneck. Their serial part is 0.41 s of the sampled run, and purity and property writes are 0.19 s of that (`PROFILE_DRILL_PIWIGO_20G.md`, item 4).
- **The eager pass.** The purity, property-write and classification rows are the eager pass over every declaration. Together they cost 926.4 M in the off run and 880.6 M in the warm run (§10.9).

## 8. The serial insert, vendor discovery and the session

**Serial insert, 739.8 M**, on the main thread.
- **Dropping the trees.** Dropping the 11,459 context files' trees costs 567.8 M (`drop_glue::<[Stmt]>` called from `insert_numbered_file`).
  - It includes mimalloc's cross-thread frees: `mi_free_try_collect_mt` 75.1 M, `mi_abandoned_page_try_reclaim` 68.9 M and `_mi_page_free_collect_partly` 58.3 M. The trees were allocated on the workers and are freed on the main thread.
  - Item 3 drops each tree on its worker, which takes the drop off the serial path and makes the frees local. Proposal 8 makes the trees about two thirds smaller.
- **`register_references`**, with the `FileId` set inserts, costs 92.7 M. Item 5 replaces them with posting lists.
- **`insert_built_decl`** costs 44.8 M. Items 4 and 5 cover it.

**Composer vendor discovery, 321.2 M**, on a thread of its own.
- **One file is read.** When `autoload_static.php` exists, it is the only generated file read (`composer/src/vendor.rs:84-92`). Piwigo's 1.6 MB `autoload_classmap.php` is not parsed, so no file is read twice.
- **Parsing it, 154.4 M.** The lexer, the parser and the line index over the 1.8 MB static file: the one PHP parser, which P10 decided to keep.
- **Path work, 102.7 M.** `Components::next` 34.6 M, `parse_next_component_back` 11.0 M, `sort_paths` 8.4 M, `Resolver::listed` 7.0 M and `compare_components` 6.6 M. Item 2 removes it.
- **The other 64.1 M** is allocation, copies and compares that serve both.
- **Off the critical path.** The 20 G report shows the thread done by 0.077 s, while the file walk runs to 0.146 s. So a cheaper reader would lower the instruction count, not the wall time (§10.12).

**Session load, 169.3 M, and session run, 32.9 M.**
- Both are path work: `Components::next` 42.8 M and `compare_components` 36.4 M.
- Most of it is the sort of the file list (`drift::sort::<PathBuf>`, 40.7 M of compares), plus `strip_prefix` and `starts_with`.
- Item 2 gives each file its path data once, from the walk, and sorts by it.

## 9. Costs that cross phases

### 9.1 Folded names (item 15)

- **Size.** In the warm run, `is_folded_eq` costs 402.7 M over 4,249,204 calls and `fold_hash` 298.0 M over 5,046,296 calls. Together that is 700.7 M, item 15's 701 M.
- **Callers.** Each of the largest callers is named in item 15's design:
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
  - `check_stmt_impl`, 24.0 M;
  - `check_function_impl`, 20.9 M;
  - `check_class_generic_bounds`, 11.1 M;
  - `check_property_declarations`, 6.9 M;
  - `check_phpdoc_types_against_native` and `printf_placeholders`, ≈4 M.
- **Item 18's display work, 26.0 M.** `write_shape` 20.8 M and `quote_php_string` 5.2 M.
- **Reported, then dropped: 22.5 M (proposal 11).**
  - `report_missing_offset` formats 55 messages at ≈409 K Ir each. All of them are `offsetAccess.mightNotExist`: the only format with a cost is the one at `checker/assignment.rs:809`.
  - That message is level 7, and the run reports at the default level 6 (`DEFAULT_REPORT_LEVEL`, `project/session.rs:39`), so `finalize` drops all 55 (`session.rs:206`).
  - F1 does not reach them, because they are reported.
- **A key for an empty map: 6.6 M (proposal 11).** `ConstantResolver::class_constant_type` formats a lowercased `class::NAME` key on every class-constant read: 6,854 calls (`checker/constant_resolver.rs:13-18`, `:94`). It does this to probe `dynamic`, which is empty when no `dynamicConstantNames` are configured, as here.
- **No item, ≈13 M:**
  - `sqlparser`'s own formatting, 9.3 M, which F4 makes once per distinct query text;
  - the regex scanner, 3.7 M.

### 9.3 Copies

- **Size.** In the warm run, `__memcpy_avx_unaligned_erms` is 515.8 M self over 15.57 M calls. §3.2's row "memory and UTF-8" (1.26 G) also holds:
  - `memcmp`, 206.5 M;
  - the `memchr` family, 276.9 M;
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

- **Coverage.** M10 of the 20 G report gives every one of the dhat run's 19.4 M allocations to an item, and expects about 1.35 M to remain. It counts each reallocation as a block, charged to the site that first allocated it.
- **What that means here.** The allocator's 1.75 G in the warm run is the cost of those allocations, so the items lower it as they remove them. Its memory-bound share weighs more in wall time than in instructions.

### 9.5 Decoding stub entries (warm and cold runs)

- **Size.** In the warm run, `decode_entry` costs 439.0 M inclusive: 301.3 M for 39,914 functions and 137.7 M for 3,949 classes. By attribution, 275.5 M of it is in the finishing passes and 202.3 M in the check.
- **Who triggers the function decodes.** 269.2 M are made on a `Lazy`'s first read, through `OnceLock::initialize`. By caller:
  - **`resolve_deferred_value_of`**: 178.0 M for 22,746 methods. Item 6, step 1 removes it.
  - **`Members::eq` in `changed_since`** (`registry.rs:641-679`): 88.6 M for 12,106 methods. Through `Lazy`'s `PartialEq` (`name_table.rs:1185-1189`), it decodes both sides to compare members whose pointers differ. Item 4 removes it (F3 step 4).
  - **`flatten_traits_one_pass`**: 3.1 M.
- **What stays:**
  - the 137.7 M of class decodes, from lookups the run makes (`load_base`);
  - 37.2 M of function decodes through `load_base`.

  Item 6, steps 3 and 5 make each decode cheaper, not fewer: ≈70 M of name re-interning and 35 M of UTF-8 checks across all decodes.
- **With proposal 6**, the off and cold runs decode as the warm run does.

## 10. Proposals

Proposals 1-6 concern the stub build, which only an off or a cold run pays. A cold run is the first run of each build, and every run without a persistent cache. Proposals 1-5 come first (the ledger's D7): E1's fix carries 1 and 5, D5's gate carries 2, and then 3 and 4. The crate split's C4 and C5 replace the eager stub build with lazy, per-file parsing, and the code of proposals 1-5 goes with it, so they pay only until then. Proposal 6 waits on the split's stage C10, and its form does not survive C4 and C5: what is left to carry is each stub file's declarations, worth ≈0.2-1.5 G per cold run instead of 5.05 G. It is decided again once C5 sets its version key (§10.6, the ledger's C8). Proposals 7-11 reach every run. None of them is in the 20 G plan.

### 10.1 Resolve only the stub base the files need

- **Now.**
  - `Session::load` calls `Project::new()` (`session.rs:733`), which clones `Project::plain_base()` (`project.rs:532-541`).
  - After the parse, `expect_files` (`session.rs:791`, `project.rs:1212-1222`) calls `switch_base(Project::phpstan_base().clone())` when any parsed file mentions PHPStan.
  - From then on the project never reads the plain base. The base with PHPStan's source is built from all the stub data (`stubs.rs:384`), not from the plain base.
- **Design.**
  - The project starts with no base and takes the one its files need at `expect_files`.
  - Nothing before that point reads the registry. The parse's `index_use_sites` reads the settings, `retain_source` and `member_index_built` (`project.rs:1106-1129`), and `note_owner` reads the packages (`:1134`).
  - `switch_base` already carries over the PHP versions and the configured assertions (`registry.rs:774-781`).
- **Saving.**
  - 355.1 M on the main thread: the plain base's merge, freeze and stores. The corpus build moves into the other base and is not saved.
  - One of the two stub-base finishing runs on the pool. The two cost ≈0.2 G together (§4.4). One of them spent 117.2 M on the thread that called it; which base each run served is not in the profile.
  - The warm run loads the plain base in 4.9 M, so it gains nothing to speak of.
- **Who gains.** Any project whose loaded files mention PHPStan's namespace. Composer's vendor context counts, so a project with a PHPStan extension under `vendor/` gains. Piwigo's own files mention it.
- **Adversarial checks.**
  - A file that mentions PHPStan only through a skimmed body (proposal 8) must still choose PHPStan's base. Proposal 8's skimmer flags the name.
  - A project with no file that mentions PHPStan resolves the plain base as it does now, at `expect_files` instead of at `Project::new()`.

### 10.2 Serialize only what a store keeps

- **Design.** Gate `stub_cache::store`, `StubIndex::store_cached`, `Registry::store_cached` and the stub-data store on `cache_dir()` before encoding. The per-file cache already works this way: with no directory, `file_cache::key` returns `None` (`project/file_cache.rs:164-165`, `directory()` at `:62-72`), and no entry is encoded.
- **Saving.**
  - 1,005.3 M serial on the main thread (§4.3), on off runs only.
  - Proposals 1 and 2 overlap by the plain base's stores. Those are 0.26-0.36 G of its 355.1 M. The two bases' work other than the stores is 186.3 M (§4.2). The plain base's entries are a subset of the other's (`builtin_stubs` filters out `entry.phpstan`, `stubs.rs:937-944`), so it has at most half of that work. So after proposal 1, proposal 2 saves 0.65-0.74 G.
  - The cache files' sizes (5.27 MB against 15.33 MB, §10.6) would put the plain base's stores at about a quarter of 814.9 M. That is below the bound above, so the encoding does not cost the same per byte in the two bases. The profile has no calling context to split them further.
- **What is lost.**
  - `NameTable::store` reads the entries, and for a built entry `Lazy`'s `Serialize` encodes the value without caching it (`name_table.rs:1192-1203`).
  - `Writer` keeps nothing beyond the dropped container.
  - One thing does go: `store_entries` checks that a table holds no name twice (`name_table.rs:1043`). With the gate, an off run no longer prints "can't be stored" for such a table. A cold run still checks it.
- **How narrow it is.**
  - `cache_dir()` is `None` only when `PHPSTAN_RS_NO_STUB_CACHE` is set, or when `HOME` and `XDG_CACHE_HOME` are both unset (`stub_cache.rs:33-44`).
  - Nothing in the repository sets the variable: not `xtask`, not `.cargo/config.toml`, not the tests. `tests/file_cache.rs:31` removes it.
  - A directory that exists but cannot be written still pays the encoding, which the gate does not catch.
- **Decided: each cache checks once whether it can write, apart from reading.**
  - **The check.** At its first store, a cache does what a store does: it creates the directory, then creates and removes a temporary file there. It keeps the result in a `OnceLock` of its own. A run that stores nothing, such as a fully warm one, never checks.
  - **Reading stays on.** A read-only cache, such as one a CI job restores, is still read. Turning the cache off would lose that.
  - **What the gate skips.** Every step a store takes before the write. For the per-file cache that starts with building the entry (`CachedFile::of`, `session.rs:765-771`), not only its encoding.
  - **What it reports.** One line when the cache cannot be written, and one when a later write fails. A full disk, or one of the per-file cache's 256 shard directories (`file_cache.rs:177`) that cannot be written, gives the latter. Both fail silently today.
  - **Reads write too.** A load refreshes the mtime of an entry older than 12 hours (`file_cache.rs:190-196`), which also fails silently in a read-only directory. That stays harmless.
  - **Which caches.** It applies to the per-file cache, to the stub cache until proposal 6 removes it, and to proposal 8's parse-verdict cache.
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
  - With proposal 6, the bases are stored and this gain goes, except for builds that carry no cache.

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
- **Design.**
  - The corpus, which both bases need, starts before the files are listed, on a thread of its own, as `Project::warm_stubs()` (`project.rs:549-561`) already does for the fixtures (`tests/fixtures.rs:36`). `check-project` never calls it.
  - Its `par_iter` moves into `worker_pool().install`, so the corpus and the parse share the 8 cores instead of running 16 threads on them.
  - The base is resolved after the parse, once its files say which one (proposal 1).
- **Saving.**
  - Running a serial stretch S beside a parallel phase P that keeps every core busy takes about P + S/8 of wall time, against P + S one after the other. So it saves ≈7/8 of S, however busy the parse is.
  - Here S is the corpus's merge and stores, 0.62 G today, less what proposals 2 and 4 remove.
  - The corpus's parallel 3.5 G gains nothing beside the parse, since both are parallel.
  - The base's serial work stays on the critical path: 646.1 M for PHPStan's base, or 355.1 M for the plain one.
  - Instruction counts do not change.
- **Adversarial checks.**
  - **Starvation through `install`.** From the main thread, `install` blocks a thread that belongs to no pool, which is safe. From a worker of the pool, the `par_iter` already runs on that pool. From a thread of a third pool, such as the one the test at `incremental_updates.rs:1155-1175` makes, `install` blocks while that pool's thread keeps stealing that pool's jobs. A stolen job that needs the same `LazyLock` would then wait on itself. The change needs that test.
  - **Thread limits.** The global pool ignores `PHPSTAN_RS_THREADS` and the 64 MB stack (`project.rs:355`). Moving onto the worker pool fixes both.

### 10.6 Build the stub data at compile time, into every binary

- **What it removes.** The whole difference between the off and warm runs: 5,053.0 M (§4). That includes the stub sources, the serial stub work on the main thread, the stub bases' finishing runs, and the bisection in built bases.
- **Why it is sound.** The cache's key is the build's source hash alone (`registry/stub_cache.rs:17`, `:46-48`). No configuration and no PHP version goes into it, and `switch_base` applies the PHP versions and the configured assertions after the base is loaded (`registry.rs:774-781`). The stub build reads no environment variable except the cache's location (`stub_cache.rs:33-44`). So what a cold run writes is a function of the build, and the build can carry it.
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
- **Decided: option 1, at compile time.** It is the only way every binary carries the data:
  - release binaries;
  - binaries built from source, which is how the README says to build one (the crate is not published: `publish = false`);
  - test binaries;
  - every way the VS Code extension finds a binary: `serverPath`, `PATH`, or one it downloads (`editors/vscode/README.md:37-41`).

  Options 2 and 3 reach release archives only. With option 3, each of the extension's ways to a binary would also have to carry the files. Since the data is a function of the source, the build that compiles the source makes it. Then no run is cold, and the disk stub cache, its key and its pruning go.
- **What it takes.**
  - **The crate split's reflection layer.** A build script cannot call the crate it builds, and it must run the very code the runtime runs. A stub base is built through `Project::finish_loading` → `run_finishing_passes` (`project.rs:1387-1399`). The registry uses the checker (`registry/init_flow.rs:13`, and about ten more paths into `checker`, `project` and `ext`). So the data can be built only once the registry, its finishing passes and what they read live in a crate below the checker (the reflection layer of `CRATE_SPLIT_PLAN.md`).
  - **A small crate that embeds the data.** Its build script depends on the reflection crate, and its `include_bytes!` holds the 31 MB. In a crate of its own, the data is embedded again only when it changes, not on every incremental build of the main crate.
  - **An optimized build-dependency.** Build dependencies compile at opt-level 0 unless `[profile.*.build-override]` raises it. The test profile's comment says the stub registry builds 4-6× faster at opt-level 1 than at 0 (`Cargo.toml`).
  - **A borrowed `Reader`.** `Reader::new` takes an owned `Vec<u8>`, and the embedded data is a `&'static [u8]`.
  - **A test, on every release target, that the embedded data equals a runtime build.**
    - Two release legs cross-compile (`release.yml`): `aarch64-unknown-linux-gnu` on an x86_64 runner, and `x86_64-apple-darwin` on an arm64 one. So the build script runs on a different architecture from the binary.
    - Constant folding of `**` calls the platform's `pow` (`types/src/arithmetic.rs:715-717`).
    - The stored hash index is the same everywhere: `folded_hash` (`syntax/src/name.rs:997`) is a fixed 64-bit function over little-endian words.
- **Costs.**
  - **Size.** The data is a static: `.rodata` grows from 18.2 MB, most of it the embedded stub sources, to ≈50 MB. That applies to the release binary and to each test binary that links it. A run loads only the pages it touches, and a release archive compresses the data to ≈3.4 MB.
  - **Compile time.** The reflection crate also compiles for the host. The build script reruns the stub build whenever that crate, its dependencies or the stubs change. This is not measured (§12). An edit to any other crate leaves the data as it is, whereas today every edit makes the next run cold.
- **What stays.**
  - `PHPSTAN_RS_NO_STUB_CACHE` comes to mean "build the stubs at run time", so the off mode still tests the build path.
  - The reflection crate's own tests cannot use data that a crate above them builds, so they build the stubs at run time.
  - Proposals 1-5 then speed only those, and proposal 4 also speeds the build script. Until the crate split lands, they speed every cold run. Once its C4 and C5 land, no eager build is left for them to speed (below).
- **Adversarial checks.**
  - **A stale embedding.** Cargo reruns a build script when one of its build-dependencies is rebuilt, and the stub sources reach it through `stub-sources`. The equality test catches whatever that misses.
  - **A feature that changes the data.** Build-dependencies resolve their features apart from normal ones. The registry, `types` and `phpdoc` have no feature gates. `syntax`'s `fuzzing` only derives `Arbitrary`, and `pcre-validate` switches only `pcre_validate.rs`. The equality test covers the rest.
  - **Inputs a build script cannot know.** There are none: no setting, and no environment variable besides the cache's location (above).
- **Against the crate split: the form does not survive, and the decision is taken again after C5's version key** (ledger C8).
  - **No bases are left to embed.** C4 resolves each class lazily, and C5's stub layer parses a stub file when something reaches it, keyed by PHP version. The checked-in index answers which file declares a name. What is left to embed is each stub file's declarations before resolution, which is what the per-file cache stores for a context file today.
  - **The saving falls to the parse of the files a run reaches.** Over ten projects a run reaches 6-43% of the stub bytes (10-58% at the upper bounds), because a phpstorm-stubs file holds a whole extension. At ≈0.21 G per MB of stub source, that is ≈0.2-1.5 G per cold run (≈2 G at the upper bounds), against 5,053 M today. Resolution is paid in memory either way, and the merge and the stores go with the bases.
  - **The version key decides whether the data is still a function of the source.** Parsing per version, as C5 is written, gives 81 gated files 331 readings over the 15 known versions, 15.06 MB of source instead of 2.99 MB. Keeping the gates as data, lexed as conditions and resolved by the configured range, keeps one parse per file and answers a range with every reading in it.
  - **How it would be carried.** Checked in, as the index is, it is unfit: the declarations are binary, and they change with every edit to the declaration model. A build script still reaches every binary, and its data must be handed in by a crate above reflection, since cargo rejects the cycle. Without embedding, the per-file cache keyed over the crates the declarations depend on does most of the job.
  - **So** the costs (B9, B10, `.rodata`) stay while the saving shrinks, and proposal 6 is not built in today's form.

### 10.7 Tokenize a PHPDoc tag's description only when it is read

- **Now.**
  - `TagParser::tokenize` lexes a tag's whole text up front (`phpdoc/src/grammar/lexer.rs`). Its comment says a docblock is too small for a lazy lexer.
  - Each identifier becomes a `String` (`lex_identifier`), and each `advance` clones a token.
  - `remaining_description` turns the tokens after the type and the variable back into text, joined by spaces (`grammar/parser.rs`).
- **Who reads a description.**
  - One place reads it: the `param` arm of `apply_tag`, which tests whether the description starts with `[optional]` (`docblock.rs:915`).
  - Nothing else does. `param-out`, `return`, `var`, `property` and `throws` ignore it (`docblock.rs:920-956`, `:990`, `:1246`), and so does `checker/phpdoc_unresolvable.rs:37-40`, `:94`.
  - `PhpDocTagValue::Generic(String)`, the text of every tag the grammar does not model, is built and never matched outside the `phpdoc` crate.
- **How much of the tag text it is.** `analysis/n4/desc.py` splits each tag the way `docblock.rs` joins its lines, and counts as description what follows a bracket-balanced type and an optional variable. Descriptions are 84.4% of the stubs' tag tokens and 56.5% of Piwigo's.
- **Design.**
  - The parser pulls tokens from the lexer instead of taking a token vector.
  - When it reaches the description, it records the byte offset and stops. The value keeps that offset into the tag's text.
  - The description's text is made on request, by the same lexer and the same join, from that offset. `[optional]` becomes a test on that text.
  - A `Generic` value is the tag's text, sliced and joined on request.
- **Adversarial checks.**
  - **The same text when read.** The lexer's `skip_whitespace` also drops a `//` comment to the end of the line, so `http://x` in a description loses `//x`. A raw slice would differ from today's text, so the design lexes again on request rather than slicing.
  - **No lexer state** may cross the point where the parser stops; otherwise a fresh lexer started at the offset gives different tokens. This is to be checked in the code before building. A debug build compares the lazy text with the eager text for every tag.
  - **Errors.** The tag lexer reports none. The parser's errors come from the type and the variable, which are still parsed. `check_description_start_after_type` reads one token past the type, which the pulling parser still lexes.
  - **Memo keys.** `parse_tag_rest_shared` and `tag_block` key their memos on the whole tag text, so their hit rates do not change. Item 14's fingerprint keys remove that formatting.
- **Saving.**
  - The tokenizing part of the docblock blocks is ≈280 M in the stub sources, ≈150 M in the parse job and ≈40 M in the check. At the description shares above, that is ≈0.34 G for the off run and ≈0.1 G for the warm run.
  - Item 14's borrowed tag tokens remove the per-token allocations first, so after item 14 the saving is about half.
  - The figure counts every tag, not every distinct tag, so the memos may lower it. It needs a measurement on a build.

### 10.8 Skim the function bodies of context files, and parse a body when something reads it

- **What it removes.** For the ≈47% of the parse job's tokens that are context files' function bodies (§5), on today's code:

  | cost | G |
  |---|---:|
  | the parser | ≈1.73 |
  | the lexer's token layer (keywords, names, interning), leaving the raw scan (`next_raw`, `lex_operator`, `lex_ident_raw`, `scan_interpolated`: 23% of the lexer) or a cheaper byte scanner | ≈1.07 |
  | those bodies' summaries, flows and walks (67% of the 617 M body blocks) | ≈0.41 |
  | about two thirds of the 567.8 M of tree drops at the insert | ≈0.38 |
  | the global-facts and reference walks over those bodies | ≈0.19 |
  | **total, less the bodies something reads** | **≈3.8** |

  Call this F. Items 9, 10 and 12 lower the cost per token, items 3 and 19 lower the drops, and item 13 merges the walks; after them, F is ≈2.0-2.3 G. A body that something reads costs its share of F as before, so with d the share of bodies read, the most proposal 8 can save is F × (1 − d). What it saves depends on whether the bodies must still be checked for syntax errors (below).
- **Who reads a context file's bodies today.** Each reader was traced in the code:
  1. **`BodySource::summary` and `init_flow`** (`registry/bodies.rs:16-27`). Their readers are:
     - the purity pass (`purity.rs:350`);
     - the property-write pass (`property_writes.rs:423`);
     - the uninitialized-property checks (`checker/uninitialized.rs:125`, `uninitialized_dataflow.rs:535`);
     - the class shape's parent constructor (`project/class_shape.rs:343`).

     The two passes read every body; with proposal 9 they read on demand, which is why proposal 8 depends on it. The checker's reads are already per call.
  2. **The declarations a body holds**: named functions and classes declared in a body, anonymous classes, and `define()` (`project/file_declarations.rs:59-120`, `for_each_declaration_in`). The parser and the lexer already flag the files that can have these (`FileFacts`: `has_nested_declaration`, `has_anonymous_class`, `mentions & MENTIONS_DEFINE`).
  3. **`GlobalFacts`** (`registry/global_vars.rs:471-569`): an assignment after a `global` statement in the body, and an `include` or `require` anywhere. A body with neither adds nothing.
  4. **The references a body records.** They feed:
     - `dependents` (`project.rs:1056`), which `walk_affected` (`:2033`) and `closure_scope` (`:1295`) read;
     - `mentions_phpstan` (`project.rs:3179-3187`), which chooses the stub base.
  5. **A property hook's body**, which `virtual_property.rs:41` reads to decide whether the property is virtual. Hooks are not function bodies, and they stay parsed.
  6. **`is_generator` and the `goto` labels.**
     - Only the checker reads `is_generator`, for analysed functions (`checker/function_decl.rs`, `return_type_too_wide.rs`, `calls/closure_context.rs`), and so does the language server's completion.
     - The labels feed `InitFlow`, which a parse on demand computes.
- **Design.**
  - **The skim.** The parser takes a skim flag for context files. At the `{` of a function or method body, it runs the raw lexer to the matching `}`, and notes for each body whether it saw:
    - `function` or `fn` followed by a name;
    - `class`, `interface`, `trait` or `enum` as a declaration;
    - `new class`;
    - `define`;
    - `global`;
    - `include` or `require`, or their `_once` forms;
    - a name with a `PHPStan` segment.

    Any of these makes it rewind and parse the body in full. Otherwise the body is stored as its byte range and the name context in effect.
  - **Parsing on demand.** `Body` holds its summary and its flow in `OnceLock`s, filled on first read by parsing the range with the stored name context. Spans stay absolute within the file, because the lexer runs over the file's text from the body's offset.
  - **The source text. Decided: kept, copied at load from the bytes the load reads.**
    - Every load reads the file's bytes, if only for the per-file cache's key (`session.rs:631-637`). A skimmed body's bytes are copied out of them then. A body parsed later then reads the bytes the load checked, even if the file has changed on disk since.
    - A body parsed on demand is lexed with `Lexer::fragment(src, base)` (`syntax/src/lexer.rs:461-473`), whose spans are where they are in the file. `fragment` takes text, so a file that is not UTF-8 needs a bytes variant, as whole files have (`Lexer::from_bytes`).
    - The kept bytes come to ≈19 MB on Piwigo. The run's peak is 866 MB, of which ≈420 MB are the context files' trees (the 20 G report's M1), and the skimmed bodies' share of those trees is never built.
    - The language server already keeps every file's text (`lsp.rs:294`), so there the bodies cost nothing more.
    - **Reading the file again on demand is rejected.** It may have changed since the load, and a hash check would only detect that. Mapping the file has the same problem and needs `unsafe`, which the project has none of.
  - **References.** A body parsed on demand registers its references then, so an edit to a name it reads still reaches it.
  - **The per-file cache** stores a skimmed body as its range and name context. A load that hits the cache has read the file's bytes for the key, so the body's text comes from them, as on a parse.
  - **Syntax errors. Decided: every skimmed body is still checked, by a pass that builds only what its checks read, and each file's verdict is cached.**
    - **Why the bodies must be checked.** Today a context file whose parse fails is skipped with a warning, and its declarations are lost (`project/session.rs:669-679`, `:828-837`). PHP does the same: one bad method body leaves none of the file's classes or functions declared. That holds for a parse error and for a compile-time error such as "Cannot mix keyed and unkeyed array entries" (php 8.5; `~/.cache/prs-probe/syntax-in-body/`). Leaving a skimmed body unchecked would make whether a file loads depend on whether an optimization skipped one of its bodies. On Piwigo no context file failed to parse: the off run's stderr (`off/valgrind.err`) holds no warning.
    - **Every error is the parser's or the lexer's.** `syntax::parse_watching` is the only step of a load that fails (`project.rs:2892`), and a lexer error anywhere in the file outranks the parser's first error (`syntax/src/parse.rs:106-113`). So the check needs the grammar and the lexer's token layer, and no later pass over the tree.
    - **The checking pass is a reduced builder, not an empty one.** The parser checks destructuring after it builds the array (`syntax/src/parser/expr.rs:402-497`), with sets keyed by node id (`long_arrays`, `pattern_only`). So the pass keeps array shapes and node ids, and builds no other node, no reference and no interned name.
    - **The verdict cache.** Whether a file parses is a function of its bytes and the `syntax` crate alone:
      - the lexer's watch list sets only `facts.mentions` (`lexer.rs:416-419`), never an error;
      - `syntax` has no stub mode: `is_stub` is read only after the parse (`project.rs:3003-3027`).

      So a cache keyed by the file's content hash and by a hash of `crates/syntax`, the workspace manifest, `Cargo.lock` and the target (computed as `build.rs` computes the build's source hash) is exact. It holds:
      - whether the file parses;
      - its first error;
      - the lexer's warnings in its skimmed bodies, which a load reports (`session.rs:803-812`).

      A body whose file has a cached verdict is skimmed and not checked again. The cache survives every build that does not touch the parser, whereas the per-file cache is keyed by the whole build. A debug build checks every body anyway and compares the verdicts.
- **What it saves.** Without a cached verdict, every skimmed body is checked, and a body that something reads is then parsed in full. Let S be what the checking pass saves against a full parse, over all the skimmed bodies. The token pipeline and the grammar stay in the pass: `fill`, `skip`, `take_token` and the tokens' drops are ≈1.05 G of the parse job's 3.65 G parser block.

  | cost the pass does not pay | today, G | after the plan, G |
  |---|---:|---:|
  | building the tree: the bodies' share of at most ≈0.53 G in the parser block (allocation 308 M, vector growth ≈94 M, copies 108 M, node drops ≈24 M), less the arrays the pass keeps | ≈0.15-0.25 | ≈0.05-0.1 (item 9) |
  | recording references and resolving names | ≈0.09 | ≈0.05-0.09 |
  | interning names | ≈0.17 | ≈0.08-0.12 |
  | the bodies' summaries, flows and walks | ≈0.41 | ≈0.27 |
  | the global-facts and reference walks | ≈0.19 | ≈0.1-0.15 |
  | the tree drops at the insert | ≈0.38 | ≈0.05-0.15 |
  | **S** | **≈1.3-1.5** | **≈0.6-0.9** |

  - **With the file's verdict cached:** F × (1 − d), ≈3.8 G × (1 − d) today and ≈2.0-2.3 G × (1 − d) after the plan.
  - **Without it** (a file not seen before, or the first run after a change to `syntax`): S − F × d. That is nothing at d = S / F, ≈0.34-0.39 today and ≈0.26-0.45 after the plan.

  So the verdict cache is what makes proposal 8 pay on most runs, and d (below) decides whether it pays on the others.
- **Adversarial checks.**
  - **The verdict cache cannot go stale the way a narrower stub-cache key would (§10.12).** The verdict's inputs are one crate and its locked dependencies, and the key covers them whole. The stub cache's inputs are most of the main crate.
  - **A panic is not a verdict.** A parse that panics makes the file unread today (`session.rs:661-666`), and nothing caches it.
  - **The stub base must not change.** `mentions_phpstan` reads the references, and a skimmed body's references are not recorded. So the skimmer flags a `PHPStan` segment, and a debug check compares the base chosen against an eager parse.
  - **False flags are safe.** A trigger word inside a string or a comment only causes a full parse.
  - **The facts stay identical.** `FileFacts` stays file-wide and is computed as now, because the skimmer's raw lexer still sees every identifier for the `mentions` bits. A debug build also parses every skimmed body eagerly and asserts equal summaries, flows, declarations, global facts and references.
  - **Summaries do not depend on the registry** (`BodySummary::of(&body.stmts)`, `bodies.rs:112`), so a summary made late equals one made early.
- **Two questions the check raises, settled in the ledger.** Both hold whether or not proposal 8 is built.
  - **The parse-failure policy (D6).** A context file that fails to parse loses every declaration. That is right when the error is real and wrong when it is a gap in our parser, and the analyser cannot tell which.
    - **Recommended:** keep what the file declares. Recover what parses: a failed body leaves an unknown summary, and a failed header a class whose shape is unknown. Report the file once.
    - **Validity follows the configured PHP version.** The parser refuses `$s{0}`, which PHP 7 accepts, and a real file has it.
    - **One rule for the batch run and the language server.**
    - **Why.** Where the error is real, every use that autoloads the file throws, so the code after it sees the declarations in either world. Of the 272 files under `vendor/` that PHP 8.5 refuses, none is in an installed dependency's autoload. The one that reaches the analyser as context is a stub written for analysers, which PHP refuses and the project scans on purpose.
  - **Compile-time errors the parser does not report (C7).** PHP refuses a file with a `goto` to a label that does not exist (`goto.php` in the probe directory), but the parser reports no such error, so the file loads. Of the 268 compile-time errors PHP 8.5 raises, the parser rejects 30 and accepts 238. When it fails, PHP 8.5 refuses too, though not PHP 7.
- **d, the share of bodies read**, is unknown until proposal 9's probe runs. The calls from analysed code into vendor code, followed through their callees as purity and property writes follow them, mark the bodies a run reads. A search on demand stops at the first impure callee. Without a cached verdict, proposal 8 saves nothing once d reaches ≈0.26-0.45 after the plan.

### 10.9 Purity and property writes on demand

- **Now.** The finishing passes classify every scoped function and method from its body summary, and then run the fixed points over all of them. The purity, property-write and classification blocks cost 926.4 M in the off run and 880.6 M in the warm run (§7). They run on the finishing thread, the run's wall-time bottleneck, and are 0.19 s of its 0.41 s serial window.
- **Who reads the results.**
  - Only the no-op rules read `FunctionReflection::is_transitively_pure` (`checker/noop.rs:249`, `:280`, `:335`, and the `new` arm). They ask only about the callee of a call whose result a statement discards.
  - Property writes are read per call by `checker/expr.rs:187-248` and `checker/array_access.rs:70-103`, and by `changed_since` (`registry.rs:670`), which compares them across an edit.
- **What the fixed point is.** `run_purity_fixed_point` (`purity.rs:190-260`) computes a least fixed point. A function becomes pure when every one of its dependencies is pure, so a cycle never becomes pure, and the value at `f` depends only on what `f` reaches.
  - **Search.** A depth-first search from `f` gives the same answer exactly: `f` is pure if and only if every node it reaches passes classification and the search reaches no cycle. It stops at the first impure node.
  - **Memo.** Answers are memoized per declaration id. This is item 4's `registry.purity(id)`, which item 4 already has the checker call.
  - **Concurrency.** Two threads asking at once compute the same answer from the same immutable registry. The first to publish wins, and the other's equal result is dropped.
  - **Property writes** have the same shape: a union over what is reached.
- **The incremental path.** `changed_since` needs the old and new property writes of the names an edit can reach, and it computes them for the closure scope it already computes, as now. The memo is cleared on each edit. That is always correct, because the memo caches a function of the registry.
- **Adversarial checks.**
  - **Exactness.** A debug build runs the eager pass too and compares every answer given on demand.
  - **Order.** The results do not depend on which thread asks first, because each answer is a function of the graph.
  - **The fixed point's kind.** With a greatest fixed point, cycles would be pure and the search would need strongly connected components. The code computes a least one: the worklist starts from the nodes with no impure dependency.
- **Saving.**
  - Up to the three blocks, 0.93 G in the off run and 0.88 G in the warm run, less what is asked for.
  - Item 4 removes ≈0.5 G of their string keys first, so proposal 9 adds ≈0.3-0.4 G beyond item 4, plus most of the passes' 0.19 s on the finishing thread.
  - It also removes the reason every context body's summary is needed, which proposal 8 requires.
- **The probe that settles it.** It needs a build, and none was run. On Piwigo and on a general tree, count the distinct callees that the no-op rules and the property-write reads ask about, and the nodes the searches visit.

### 10.10 Check UTF-8 with SIMD

- **Now.** `String::from_utf8` costs 66.5 M over the 14,251 files, about 54 MB: ≈1.2 Ir per byte on std's word-at-a-time path for ASCII.
- **Design.** `simdutf8`'s AVX2 path checks 32 bytes per step, roughly a quarter of the cost: a saving of ≈50 M.
- **Adversarial checks.**
  - The check cannot be dropped: whether the text is UTF-8 decides the length a file is weighed by and the source that is kept (`session.rs:649-660`).
  - The `Utf8Chunks` in `LineIndex::new` (70.6 M) is item 11's.

### 10.11 Make text only when it is read

- **Reported messages, formatted when displayed: 22.5 M.**
  - **Now.** `Diagnostic.message` is a `String` (`crates/diagnostics/src/lib.rs:99`), formatted when the checker reports. `finalize` then drops what is above the report level (`session.rs:206`), before inline ignores, configured ignores and the baseline.
  - **Design.** The message becomes a value that displays itself: a template and its operands. `Type`s are interned and immutable, and `diagnostics` holds the value as a `dyn Display + Send + Sync`. Text is made for output (JSON, text, the language server), and for whatever compares message text (`ignore_errors` patterns, the baseline). Those comparisons see only the reports that survive the level filter.
  - **Adversarial checks.**
    - A message's text must not depend on state that changes after the report. Type display depends on the type and on settings fixed for the run.
    - The per-file cache stores no diagnostics (`project/file_cache.rs`), so nothing forces the text early.
    - Reports that a loop's convergence passes discard (`diagnostics.truncate`, `checker.rs:1278`, `:1807`, `function_decl.rs:1209`) stop paying for their text too. This profile cannot count them.
  - **Relation to F1.** F1 defers the text of what a rule formats before it knows whether to report. This proposal continues F1 past the report.
- **Class-constant keys: 6.6 M.**
  - **Design.** `class_constant_type` returns the declared type at once when `dynamic` is empty (`has_dynamic_constants`). When dynamic constants are configured, they are keyed by the class's folded name and the constant's name, with no `String` built per read.
  - **Check.** The configured keys are built once in `ConstantResolver::new`, so the lookup must fold the class name the same way: lowercase, without a leading `\`.

### 10.12 Rejected

- **One rayon pool, as a speed-up by itself.**
  - The global pool runs only during `Project::new()`, before the worker pool has any work (§1). Moving the corpus's `par_iter` into `worker_pool().install` gives the same 8 threads the same work.
  - It matters only together with proposal 5, where the two would otherwise run at once.
- **A narrower stub-cache key**, kept across builds whose stub-related code did not change.
  - What the cache holds is the output of the registry's builders (`from_def`, `from_decl`), the PHPDoc parser, the type interner and the serialization layout: most of the crate.
  - A narrower key that misses one input reads a stale base and gives wrong results with no error. The checksum catches damage, not staleness.
  - Proposal 6 removes the cost this aimed at, for every build.
- **A dedicated reader for Composer's static autoload file.**
  - The PHP parser spends 154.4 M of self cost on the 1.8 MB `autoload_static.php` (§8). A reader for its fixed shape would save most of that.
  - But it would save it on a thread that is off the critical path, and P10 decided to keep one parser.

## 11. What remains

Applied in order, each row on what the rows above it leave. The off column assumes proposal 6, so the stub build is gone before the plan's items apply.

| | warm, G | off, G |
|---|---:|---:|
| today | 20.57 | 25.63 |
| proposal 6: the stub data built at compile time | 0 | −5.05 |
| the 20 G plan's items, at their own estimates | ≈ −8 | ≈ −8 |
| proposal 7, after item 14 | ≈ −0.05 | ≈ −0.05 |
| proposal 9, beyond item 4 | ≈ −0.3 | ≈ −0.3 |
| proposal 8 after the plan, with d assumed ≈ 0.2 and the verdict cache holding the files (per-file cache cold or off) | ≈ −1.6 to −1.85 | ≈ −1.6 to −1.85 |
| proposals 10 and 11 | ≈ −0.08 | ≈ −0.08 |
| **left** | **≈10.3-10.6** | **≈10.3-10.6** |

- **The off and warm runs meet** once proposal 6 builds the stub data into the binary.
- **On a run whose files have no cached verdict,** proposal 8 saves ≈0.15-0.5 G at d ≈ 0.2 instead (§10.8), which leaves ≈11.6-12.0 G.
- **The off run without proposal 6.** Proposals 1-4 take ≈1.5-1.7 G instead of 5.05 G, because they overlap:
  - 1 and 2 share the plain base's stores;
  - 1 and 3 share the plain base's finishing run.

  That leaves ≈23.9-24.1 G before the plan, and ≈13.7-14.1 G after the plan and proposals 7-11, less what items 9-14 and proposal 7 save on the stub parse. The cold run pays all of that except proposal 2's part.
- **With the per-file cache warm**, a run parses no context file. Its floor is lower by the parse job's context share, and proposal 8 does not change it.
- **The largest costs no item removes:**
  - the check's rule logic, ≈1.2 G in the statements block;
  - the type relation's and the interner's own work, ≈0.5 G;
  - building the analysed files' declarations.

## 12. What this drill does not establish

- **Wall time.** Nothing here is timed. The order of the phases is known from the code (§1): the stub build runs before the parse, so the main thread's extra 1.6 G of serial work is on a cold run's critical path, with the other cores idle. How long that takes is not measured.
- **Instructions are not time.** The allocator's and the copies' costs are memory-bound, so their weight in wall time is higher than in Ir.
- **No calling context** (§2). Shared functions are split by edge shares. The split of the stub bases and the ≈211 M of stub-base finishing runs (§4.4) are estimates for that reason.
- **The per-file cache was off.** A run with it warm builds declarations for the analysed files only and parses no context file. This profile cannot show that run.
- **The cache write is not measured on disk.** The cold run's extra 58 M is callgrind's count; disk time is not in it.
- **What needs a build** (none was run):
  - the callees that proposal 9's demand reaches, on Piwigo and on a general tree, which also gives proposal 8's d;
  - proposal 7's measured saving;
  - how many reports the loops' convergence passes discard after formatting them (proposal 11);
  - what building the stub data at compile time adds to a build's time, and to an incremental build's (proposal 6);
  - whether the stub data built for a release target equals the data that target builds at run time, on the two cross-compiled targets (proposal 6).

## 13. Reproduction

All paths are under `~/.cache/prs-profile/callgrind-20g-modes/`. Scripts in `~/.cache/prs-profile/drill-scripts/` are named with that prefix.

- **Pickles.** `drill-scripts/parse.py <dir> <out.pkl>` (13 s per run) gives `off/off.pkl` and `warm/warm.pkl`. `drill-scripts/q.py <pkl> self|incl|find|callers|callees|thread <pattern>` queries them, and `drill-scripts/selflines.py <pkl> <pattern> <N> exact` gives self cost by line.
- **Phases.** `python3 analysis/attrib.py off/off.pkl analysis/off-attrib.pkl`, and the same for `warm`. The output also lists the global pool's threads, whose finishing-pass frames go to the stub sources (§2).
- **Blocks.** `python3 analysis/blocks.py off/off.pkl analysis/off-attrib.pkl <phase> analysis/bl/<phase>.txt 12 <tag>` writes `bl/<phase>-off.txt` (and `-warm`), with per-function splits in `bl/res-<tag>.pkl`.
- **Lookups and decoding carved out** (§4.4, §6, §7): the block files `bl/find/find.txt`, `bl/find/decode.txt`, `bl/find/finishing-find.txt` and `bl/find/check-fd.txt`. Their outputs are `bl/find/ff-*.txt` and `bl/find/cfd-*.txt`.
- **The statements block's split** (§6): `python3 analysis/stmtsplit.py analysis/bl/res-check.pkl v`, and the same for `res-check-warm.pkl`.
- **Subsystems** (§3.2, §4.2): `analysis/groups.py off/off.pkl warm/warm.pkl`. The main thread's split applies the same rules to the `selft` entries of thread `01`.
- **Tokens** (§5, §10.8): `php analysis/n5/bodies.php <list>` over `analysis/n5/vendor.txt` (the classmap files under `vendor/`) and `analysis/n5/analysed.txt` (the `.php` files under the configured paths). The outputs are `vendor.out` and `analysed.out`.
- **Descriptions** (§10.7): `python3 -I analysis/n4/desc.py <roots>`.
- **Cache sizes** (§10.6): `analysis/n1/sizes.txt`, from `zstd -19` and `gzip -9` over `stubcache/*.cbor`.
- **Output identity**: `cmp off/out.json warm/out.json` and `cmp off/out.json cold/out.json`.
- **Messages dropped by level** (§9.2): `report_missing_offset`'s calls by line, from the warm pickle's `fnline` and `callline`.
