# Piwigo profile drill at HEAD (`97b8f3f0`): open tasks, measurements, and protocol

This ledger holds the measurements of the batch in `PROFILE_DRILL_PIWIGO_9071_BATCH_PLAN.md` at HEAD, the tasks still open from them, and what the drill resolved. Work runs one task at a time.

## Measurement protocol

Instruction counts from callgrind move by 6 to 8 G between runs of the same binary. The protocol is what makes a number usable.

- **Stub-cache state controls the total.** The stub cache keeps the three most recent builds per file kind (`stub_cache.rs`, `KEPT_BUILDS = 3`). A build whose stub files were pruned, or never written, pays to rebuild them, which costs about 5 to 8 G in callgrind. Every run records whether its stub files existed when it started. Cold and warm runs are not comparable.
- **The file cache** is off for off-mode runs (`PHPSTAN_RS_NO_FILE_CACHE=1`). Warm and cold-with-store runs use it. The stub cache has its own switch, `PHPSTAN_RS_NO_STUB_CACHE`, which `PHPSTAN_RS_NO_FILE_CACHE` does not set.
- **Comparisons are paired.** Two binaries are compared only when run back to back in one session, with the stub state of each recorded.
- **Instruction counts are not wall time.** Callgrind serialises threads, so it counts work, not waiting.

## Run register

Each callgrind run is one process, `--separate-threads=yes`, on Piwigo with `check-project --config phpstan-rs.toml --format json`.

| id | binary (commit) | mode | stub state at start | Ir | evidence for the stub state |
|---|---|---|---|---|---|
| C1 | `97b8f3f0` (HEAD) | off | warm, not verified | 49.358 G | written after a plain run of the same binary |
| C2 | `97b8f3f0` | off, rerun | cold | 55.325 G | its stub files were rewritten at 22:28–22:31, during the run |
| C3 | `9071f48f` (batch baseline) | off | not recorded | 54.126 G | none |
| C4 | `9071f48f` | off, rerun | not recorded | 61.933 G | none |
| C5 | `5fb858c9` (20G binary) | off, first run | cold, inferred | 25.669 G | C6 is 5 G lower |
| C6 | `5fb858c9` | off, second run | warm | 20.576 G | matches the 20G plan's 20.57 G |
| C7 | `97b8f3f0` | warm (file cache filled by two unprofiled runs) | not recorded | 39.370 G | none |
| C8 | `97b8f3f0` | cold-with-store (file cache empty) | warm, by construction | 53.128 G | none |
| C10 | `9071f48f` (batch baseline) | off, warm stub (Q2) | warm, restored by a plain warm-up at 07:11 and not rewritten during the run | 54.320 G | the stub files kept their 07:11 timestamps through the run; output identical to the warm-up's |
| C9 | `97b8f3f0` | off, third run (Q1) | warm, restored by a plain warm-up at 23:08 and present at the start | 49.337 G | the stub files were present before the run; output identical to HEAD's |

## Measurements

| id | measure | baseline | HEAD | status |
|---|---|---|---|---|
| M1 | callgrind off, both warm | 54.320 G (C10) | 49.337 G (C9) | paired, both stub caches warm: −4.98 G (−9.2%). This replaces the unpaired −4.77 G, and the earlier rerun pair (C4 → C2, −6.6 G), whose stub states were not recorded. |
| M2 | callgrind warm | 42.14 G | 39.37 G (C7) | unpaired |
| M3 | callgrind cold-with-store | 56.24 G | 53.13 G (C8) | unpaired |
| M4 | hyperfine off, 5 runs | 4.22 s ± 0.11 (`97df097c`) | 4.12 s ± 0.39 | unpaired; baseline is pre-fix, not `9071f48f` |
| M5 | hyperfine warm, 5 runs | 3.56 s ± 0.04 (`97df097c`) | 3.77 s ± 0.29 | unpaired; baseline is pre-fix |
| M6 | heap total allocated | 7.43 GB, 46.17 M blocks (`df603944`) | 7.41 GB, 43.00 M | stub state and settings not recorded for either |
| M7 | heap peak retained | 0.88 GB (`df603944`) | 1.48 GB | as M6 |
| M8 | heap retained at exit | 0.66 GB (`df603944`) | 1.13 GB | as M6 |
| M9 | heap, 2026-10-03 | 2.67 GB, 19.44 M (`5fb858c9`) | not comparable | different commit and settings |
| M10 | Piwigo findings | 611 (reference, not on disk) | 613 | `97df097c` and `97b8f3f0` outputs are byte-identical; see F1 |
| M11 | oracle corpus, debug assertions | 35 panics (`97df097c`) | 0 panics, exit 0 | fixed by fix A (`5ddb4436`) and fix T (`97b8f3f0`) |
| M12 | tests (at `97df097c`) | — | lib 1295, fixtures 3999, workspace 2403 | all pass |

The 20G plan's 20.57 G (C6) and the batch's 54.13 G (C3) are both dhat-profile runs with the file cache off. They differ by code, and C3's stub state is not recorded.

## Wall-time scaling (Q3)

Native timings with `hyperfine -N`, one warm-up and three runs each, output discarded, `PHPSTAN_RS_THREADS` set per run, the file cache off, and both stub caches warm. The two binaries ran in one block per thread count. Their stub files were checked before and after. HEAD's `stub-base` files were rebuilt during the `t=1` block's warm-up, before its timed runs. The timed runs have a tight spread (0.08 s), which is consistent with that.

| threads | `9071f48f` | `97b8f3f0` | HEAD faster by |
|---|---|---|---|
| 1 | 14.03 s ± 0.13 | 12.48 s ± 0.08 | 1.55 s (11%) |
| 2 | 8.41 s ± 0.24 | 7.29 s ± 0.06 | 1.12 s (13%) |
| 4 | 5.86 s ± 0.36 | 4.95 s ± 0.12 | 0.91 s (16%) |
| 8 | 5.11 s ± 0.07 | 4.98 s ± 0.48 | 0.13 s (3%) |

Fitting wall = serial + parallel ÷ threads to the 1, 2 and 4 thread points gives:
- HEAD: serial ≈ 2.1 s, parallel ≈ 10.4 s at one thread.
- `9071f48f`: serial ≈ 2.8 s, parallel ≈ 11.2 s at one thread.

Both binaries stop scaling after four threads. The eight-thread points are about 5 s for both, which the fit does not predict. The load average was not logged for these runs, so I can't yet say whether the other sessions on this machine caused the flattening.

## Open tasks

The order is the suggested one. Each row gives the cause with its measured evidence, the status, the change, and what shows it worked. Tasks marked "needs approval" are runs or builds that the ground rules hold until approved.

| id | target and evidence | status | change | done when |
|---|---|---|---|---|
| O1 | `destroy_held` (effects.rs:4152) runs for each Destroy operation and class candidate: 1.52 M calls, 1.28 M from `implicit_targets'2` and 0.24 M from `implicit_targets`. Each call walks every property. For each one it calls the property lookup (`lookup_property_with_owner`, 5.19 M of 5.6 M calls) and `declared_holder` (effects.rs:3166). Those lookups cost 3.99 G inclusive over 6.72 M calls. Inside the lookups, 11.0 M `key_hash` calls hash the property name, which arrives as a `&Name` coerced to `&str` at effects.rs:4160. `declared_holder` calls `class_references_by_name` once per property (4.7 M of 9.7 M calls). Scalar properties yield an empty holder, but their lookups are still paid. An unknown-typed property ends the walk early (effects.rs:4167). | open, ready | Memoise `destroy_held` per class, keyed like the `implicit_methods` memo, invalidated by the same events. Keep EG.22. Pass the `Name` through at effects.rs:4160, with `lowered()`'s hash for case-insensitive tables. | The lookups and `class_references_by_name` calls from `destroy_held` fall; output and EG.22's fixture unchanged. |
| O2 | 699,322 implicit operations are recorded per run (`scope.calls`, summaries.rs:2432). Their retained size is about 98 MB at about 140 bytes per `CallWrites`, against 74 MB through `destroy_overwritten` and 24 MB through `dies` at peak. | open, needs approval for the instrumented run that counts operations never used | Reduce the operations recorded, not their size. Measure `CallWrites` before choosing. | The count of recorded operations falls; output unchanged. |
| O3 | `classify` reserves `calls` to `listed` (effects.rs:5473). Probe: 3.17 M calls pushed against 0.69 M reserved, so the heavy classifications reallocate. Most reserve exactly (median ratio 1.0). | open, ready | Size the reservation from the pushes, or build the vector at its final size with a count pass. | Peak retained for these sites falls; instruction count not worse. |
| O4 | `Origin::Returned` holds an `Arc<Callee>` allocated per origin in `origins_of` (summaries.rs:609, 620, 626), since `e9215833` (batch item 12d). 38 MB retained at peak. | open, ready | Share one `Arc` per distinct callee within a body. | Peak retained falls; output unchanged. |
| O5 | `Memo<DocBlock>::get` (`tag_block`), 57.8 MB at peak. Probe: 211,804 requests, 115,886 misses (distinct texts), 95,918 hits (45%). The memo's bound is 16,384 entries, so it runs at that bound for most of the run. It held 8 MB at `df603944`. | open, needs approval for the parse-cost measurement | Reduce the bound or the entry size, measuring the parse cost against the 45% hit rate. Why it was nearly empty at `df603944` is not known. | Peak retained falls; parse cost measured against hit rate. |
| O6 | `Parser::token_ref` (parser.rs:567) has no `#[inline]`, with 7.76 M calls from `parse_expr_bp_inner` and 0.48 G self cost in the dhat profile. The release profile is thin LTO with `codegen-units = 1`; dhat sets `lto = false`. | open, needs approval for the release measurement | Add `#[inline]`, then measure in the release build, not dhat. | `token_ref` self falls in release; output unchanged. |
| O7 | 6,801 target sets reach the set path, each at exactly 12 entries (`SCANNED`, effects.rs:1505), receiving about 7.5 M pushes (about 1,100 per set). | open, needs approval to count duplicate pushes | Dedupe pushes before they reach the set, or reduce set sizes. | Pushes into the set path fall; output unchanged. |
| O8 | `callees_of` (effects.rs:2692 to 2699) runs twice for each live classification: a count pass (effects.rs:2712) that sizes the caller lists exactly, then a fill pass (2727). The chain is 18.85 M calls, 0.81 G self. | built: the single-pass variant is in the throwaway worktree `~/.cache/prs-e2v/t11`, binary at `~/.cache/prs-e2v/t11-target/dhat/phpstan-rs`. Needs approval for the paired heap runs. | Measure the peak and instruction count of HEAD and the single-pass variant back to back, under the protocol. | Both heap runs recorded as a pair, and the trade chosen from them. |
| O9 | Depends on 20G plan row 19.2 (a batch frees each analysed tree when its check ends), which is OPEN. It covers the expression arena (78.5 MB at peak), `FileTable::with_paths` (27.2 MB), `ReferenceIndex::from_file_refs` (19.3 MB), and the effect table's buckets (15.7 MB). | depends on 20G row 19.2 | None here. | Row 19.2 closed in the 20G ledger. |

Measurement and question tasks:

| id | question | status | change | done when |
|---|---|---|---|---|
| Q1 | Does the third HEAD run reproduce C1 (49.36 G) with its stub files warm? This tests the stub-cache explanation for the drift. | answered: C9 gives 49.337 G with the stub files warm, against C1's 49.358 G (0.04% apart), and C2's cold 55.325 G. The stub state explains the drift between C1 and C2. | None. | — |
| Q2 | Why do the 20G figure and the batch baseline differ? | answered: C6 reproduces 20.57 G warm; C5 is 5 G higher cold. C3 and C4 had no recorded stub state, so they are dropped. The paired batch comparison is C10 against C9, both warm: 54.320 → 49.337 G, −4.98 G (−9.2%). | None. | — |
| Q3 | Why does wall time not improve while instructions fell? | answered in part. The batch cuts wall time by 11% to 16% at one to four threads, which matches its instruction saving. At eight threads the gain is 3%, and neither binary scales past four threads. | Log the load average for the eight-thread runs, then repeat them. Decide whether the flattening is the machine or the code. | The eight-thread flattening has a cause. |
| Q4 | Which commit between `9071f48f` and `97df097c` adds the two Piwigo findings not in the 611 reference, `src/Piwigo/Search/SearchFilterRenderer.php:442` and `tools/openapi/ResponseBodyCallSiteVisitor.php:79`? | open, not started | Bisect the range by output. The builtin-runs table (`bc9ab94f`) is excluded for `array_intersect`, since its rows did not change. | Commit found; each finding judged a true or false positive. |

## Resolved findings

These need no task. They are kept so the reasons are on record.

| finding | resolution |
|---|---|
| The property-name hashing (formerly T1) | Part of O1, the same loop in `destroy_held`. |
| `class_references_by_name` calls (formerly T10) | Part of O1. The id path `class_references(id)` exists (registry.rs:694), so the fix is the memo. |
| Over-reservation in `classify` (formerly T3) | Refuted by probe: the reserve is too small. Replaced by O3. |
| Fix A's retention of replaced `Arc`s (formerly T4) | Refuted by probe: 736 rewrites against 699,322 new keys. |
| The list-to-set switch in `TargetSet` (formerly T9) | Refuted: the set path starts at 12 entries. The cost is the push volume, O7. |
| `payload_at` and `token_at` removed (formerly T12) | Refuted: inlined, still defined and called (token.rs:461, 503). The −2.8 G is attribution. |
| Rayon spinning in `wait_until` (40.9 G at HEAD) | Withdrawn: an aggregation that double-counted. Rayon's self cost is 0.07 G. |
| T2's 2.5 G inclusive figure | Withdrawn: measured figure is 3.99 G. |
| `ffb3b177` as the origin of the `Arc` (formerly T6) | Withdrawn: that commit changed only a serde attribute. The `Arc` came in `e9215833`. |
| `a01036ec` as the cause of the memo growth (formerly T7) | Withdrawn: it keeps the same entry semantics and bounds. |
| "Expression arena retention is by design" (formerly T13) | Withdrawn: it is 20G row 19.2, OPEN. |

## Probe

A probe of `97b8f3f0` with three `eprintln!` lines (O3, O2, O7) and `memo-stats` ran on Piwigo with the file cache off. Its output was byte-identical to HEAD's. The patch is `~/.cache/prs-e2v/probe-T3-T4-T9.patch`; the worktree is removed; the probe lines are not in the repo.

## Appendices

Generated from the original, unpaired runs: `~/.cache/prs-profile/cl-9071f48f-off` (`9071f48f`), `cl-97b8f3f0-off` (`97b8f3f0`), and the heap run `~/.cache/prs-e2v/heap/dhat-heap.json` (`97b8f3f0`). The stub-cache state of the `9071f48f` run is not recorded. These tables are a drill of one run each, not a paired comparison. Regenerate after the protocol below is applied.

### Appendix A: instruction drill (off mode, self cost)

Totals: base `9071f48f` 54.13 G, HEAD `97b8f3f0` 49.36 G. Functions at HEAD with self cost of 0.1 G or more: 96.

| self G (HEAD) | base G | diff G | function |
|---:|---:|---:|---|
| 1.158 | 0.023 | +1.135 | `<phpstan_rs::registry::Registry>::lookup_property_with_owner::<syntax::name::Name>` |
| 1.125 | 2.516 | -1.392 | `__memcpy_avx_unaligned_erms` |
| 0.941 | 0.313 | +0.628 | `<phpstan_rs::registry::effects::Resolve>::run` |
| 0.807 | 0.000 | +0.807 | `<core::iter::adapters::filter_map::FilterMap<core::iter::adapters::chain::Chain<core::iter::adapters::map::Map` |
| 0.773 | 0.752 | +0.021 | `<syntax::lexer::Lexer>::next_token` |
| 0.767 | 0.026 | +0.741 | `<phpstan_rs::registry::Registry>::property_owners::<syntax::name::Name>` |
| 0.681 | 0.573 | +0.108 | `__memcmp_avx2_movbe` |
| 0.664 | 0.687 | -0.023 | `mi_free` |
| 0.642 | 0.000 | +0.642 | `<phpstan_rs::registry::Registry>::property_ref_by_name::<str>` |
| 0.602 | 0.531 | +0.071 | `_mi_theap_malloc_zero` |
| 0.557 | 0.112 | +0.445 | `<syntax::name::Name as syntax::name::FoldKey>::is_folded_eq` |
| 0.528 | 1.381 | -0.853 | `<str as syntax::name::FoldKey>::is_folded_eq` |
| 0.525 | 0.525 | +0.000 | `<syntax::lexer::Lexer>::lex_token_stream_with_worker_scratch` |
| 0.512 | 0.000 | +0.512 | `<phpstan_rs::registry::effects::Reach>::destroy_held` |
| 0.483 | 0.009 | +0.474 | `<hashbrown::map::HashMap<phpstan_rs::registry::ids::ClassId, (), rustc_hash::FxBuildHasher>>::insert` |
| 0.480 | 0.000 | +0.480 | `<syntax::parser::Parser>::token_ref` |
| 0.478 | 0.099 | +0.379 | `<phpstan_rs::registry::Registry>::class_references` |
| 0.476 | 0.721 | -0.245 | `phpstan_rs::registry::effects::settle::<<phpstan_rs::registry::Registry>::resolve_effects::{closure#8}>` |
| 0.459 | 0.082 | +0.377 | `<str as syntax::name::NameKey>::key_hash` |
| 0.433 | 0.319 | +0.115 | `<syntax::lexer::Lexer>::name_and_bits` |
| 0.421 | 0.401 | +0.021 | `<syntax::lexer::Lexer>::next_raw` |
| 0.415 | 0.840 | -0.425 | `<phpstan_rs::registry::Registry>::lookup_own_method::<syntax::name::Name, syntax::name::Name>` |
| 0.394 | 0.902 | -0.508 | `<str as syntax::name::FoldKey>::fold_hash` |
| 0.379 | 0.305 | +0.074 | `<syntax::name::Interner>::name_hashed` |
| 0.366 | 0.303 | +0.063 | `<phpstan_rs::registry::summaries::Summarizer as syntax::visit::arena::Visitor>::visit_expr::{closure#0}'2` |
| 0.363 | 0.333 | +0.030 | `<phpstan_rs::registry::Registry>::deferred_class_result` |
| 0.355 | 0.034 | +0.320 | `<phpstan_rs::registry::effects::Reach>::implicit_method` |
| 0.351 | 0.000 | +0.351 | `<hashbrown::raw::RawTable<((u32, u32), ())>>::reserve_rehash::<hashbrown::map::make_hasher<(u32, u32), (), rus` |
| 0.346 | 0.000 | +0.346 | `phpstan_rs::registry::effects::shared_names` |
| 0.339 | 0.044 | +0.295 | `<phpstan_rs::registry::effects::Reach>::implicit_methods` |
| 0.333 | 0.001 | +0.332 | `phpstan_rs::registry::effects::element_classes::collect` |
| 0.332 | 0.000 | +0.332 | `<phpstan_rs::registry::Registry>::class_references_by_name::<syntax::name::Name>` |
| 0.328 | 0.341 | -0.012 | `core::str::converts::from_utf8` |
| 0.324 | 0.324 | +0.000 | `<syntax::parser::Parser>::skip` |
| 0.312 | 0.167 | +0.145 | `<phpstan_rs::registry::effects::TargetSet>::push` |
| 0.307 | 0.000 | +0.307 | `<phpstan_rs::registry::Registry>::effect_ix` |
| 0.299 | 0.538 | -0.239 | `<alloc::raw_vec::RawVecInner>::finish_grow` |
| 0.287 | 0.000 | +0.287 | `phpstan_rs::registry::effects::settle::<<phpstan_rs::registry::Registry>::resolve_effects::{closure#8}>::{clos` |
| 0.260 | 0.000 | +0.260 | `<phpstan_rs::registry::Registry>::property_result_shared_by_name` |
| 0.259 | 0.000 | +0.259 | `<[phpstan_rs::registry::effects::Holder]>::contains` |
| 0.250 | 0.194 | +0.056 | `types::intern::insert` |
| 0.245 | 0.000 | +0.245 | `<syntax::name::Name as core::ops::deref::Deref>::deref` |
| 0.242 | 0.482 | -0.241 | `_mi_theap_realloc_zero` |
| 0.236 | 0.028 | +0.208 | `<str as syntax::name::NameKey>::is_key_of` |
| 0.231 | 0.215 | +0.015 | `<phpstan_rs::checker::Checker>::check_expr_impl'2` |
| 0.221 | 0.195 | +0.026 | `mi_malloc` |
| 0.221 | 0.024 | +0.197 | `phpstan_rs::registry::effects::declared_holder` |
| 0.216 | 0.000 | +0.216 | `<smallvec::SmallVec<[syntax::name::Name; 4]> as core::iter::traits::collect::Extend<syntax::name::Name>>::exte` |
| 0.213 | 0.000 | +0.213 | `<syntax::lexer::Lexer>::note_string` |
| 0.211 | 0.065 | +0.146 | `<phpstan_rs::registry::summaries::Summarizer>::implicit_on` |
| 0.209 | 0.213 | -0.004 | `<syntax::parser::Parser>::parse_expr_bp_inner'2` |
| 0.204 | 0.000 | +0.204 | `<phpstan_rs::registry::class_table::ClassTable<alloc::vec::Vec<core::option::Option<alloc::sync::Arc<phpstan_r` |
| 0.193 | 0.171 | +0.023 | `<syntax::ast::NodeArenas>::alloc_expr` |
| 0.190 | 0.000 | +0.190 | `<syntax::name::Name as syntax::name::FoldKey>::fold_hash` |
| 0.181 | 0.000 | +0.181 | `<phpstan_rs::registry::effects::Reach>::implicit_holder_targets` |
| 0.174 | 0.174 | +0.000 | `<syntax::lexer::Lexer>::lex_ident_span` |
| 0.171 | 0.000 | +0.171 | `phpstan_rs::registry::effects::elements_holder_of` |
| 0.169 | 0.110 | +0.060 | `<phpstan_rs::registry::summaries::Summarizer>::call` |
| 0.168 | 0.168 | +0.000 | `<syntax::lexer::Lexer>::lex_operator` |
| 0.168 | 0.000 | +0.168 | `core::ptr::drop_glue::<smallvec::SmallVec<[syntax::name::Name; 4]>>` |
| 0.166 | 0.127 | +0.039 | `<phpstan_rs::registry::Registry>::class_name_by_id` |
| 0.161 | 0.099 | +0.062 | `<phpstan_rs::registry::Registry>::class::<syntax::name::Name>` |
| 0.160 | 0.006 | +0.154 | `phpstan_rs::registry::effects::sorted_classes` |
| 0.158 | 0.000 | +0.158 | `<syntax::name::Name as core::cmp::PartialEq>::eq` |
| 0.154 | 0.159 | -0.004 | `<phpstan_rs::registry::effects::Storage as core::cmp::Ord>::cmp` |
| 0.153 | 0.001 | +0.153 | `<alloc::sync::Arc<[syntax::name::Name]>>::drop_slow` |
| 0.149 | 0.003 | +0.146 | `<types::Type>::is_array` |
| 0.147 | 0.118 | +0.028 | `<phpstan_rs::registry::Registry>::method_owners::<syntax::name::Name>` |
| 0.145 | 0.033 | +0.111 | `<phpstan_rs::registry::effects::Reach>::implicit_targets'2` |
| 0.143 | 0.000 | +0.143 | `<phpstan_rs::registry::effects::Level>::operand_origin` |
| 0.143 | 0.294 | -0.151 | `<std::path::Components as core::iter::traits::iterator::Iterator>::next` |
| 0.142 | 0.000 | +0.142 | `<phpstan_rs::registry::body_passes::Fanout<phpstan_rs::registry::retention::Uses, phpstan_rs::registry::fresh:` |
| 0.141 | 0.141 | -0.000 | `<syntax::token::Token>::into_kind_payload::<<syntax::lexer::Lexer>::lex_token_stream_with_worker_scratch::{clo` |
| 0.138 | 0.130 | +0.009 | `_mi_malloc_generic` |
| 0.136 | 0.121 | +0.014 | `__rustc::__rust_no_alloc_shim_is_unstable_v2` |
| 0.135 | 0.156 | -0.021 | `<rustc_hash::FxBuildHasher as core::hash::BuildHasher>::hash_one::<&phpstan_rs::registry::effects::ImplicitKey` |
| 0.132 | 0.000 | +0.132 | `core::ptr::drop_glue::<smallvec::IntoIter<[syntax::name::Name; 4]>>` |
| 0.131 | 0.000 | +0.131 | `<types::Type>::structural` |
| 0.128 | 0.100 | +0.028 | `<phpstan_rs::registry::global_vars::Scope as syntax::visit::arena::Visitor>::visit_expr'2` |
| 0.127 | 0.127 | +0.000 | `<syntax::token::Keyword>::try_from_bytes` |
| 0.126 | 0.098 | +0.028 | `<hashbrown::raw::RawTable<(u64, alloc::sync::Weak<syntax::name::NameData>)>>::find::<<syntax::name::WeakTable<` |
| 0.121 | 0.264 | -0.144 | `<syntax::token::TokenRef>::kind` |
| 0.120 | 0.000 | +0.120 | `<phpstan_rs::registry::retention::Uses as phpstan_rs::registry::body_passes::Pass>::expr` |
| 0.118 | 0.357 | -0.240 | `<phpstan_rs::registry::Registry>::class::<str>` |
| 0.114 | 0.094 | +0.020 | `<phpstan_rs::checker::Checker>::expr_span` |
| 0.112 | 0.117 | -0.004 | `<core::str::lossy::Utf8Chunks as core::iter::traits::iterator::Iterator>::next` |
| 0.112 | 0.000 | +0.112 | `<syntax::span::Data>::new` |
| 0.111 | 0.009 | +0.102 | `<types::Type>::bind_self` |
| 0.109 | 0.098 | +0.011 | `types::intern::exact_hash` |
| 0.105 | 0.264 | -0.159 | `<alloc::raw_vec::RawVecInner<_>>::reserve::do_reserve_and_handle::<alloc::alloc::Global>` |
| 0.105 | 0.089 | +0.016 | `<phpstan_rs::registry::summaries::Summarizer>::record_call` |
| 0.104 | 0.089 | +0.015 | `<syntax::name::Interner>::name` |
| 0.103 | 0.036 | +0.067 | `<syntax::parser::Parser>::ident` |
| 0.102 | 0.008 | +0.094 | `<types::Type>::bind_static` |
| 0.101 | 0.063 | +0.038 | `phpstan_rs::registry::summaries::origins_of::{closure#1}` |
| 0.100 | 0.080 | +0.020 | `<phpstan_rs::checker::Checker>::check_parameters` |

### Appendix B: allocation drill (default mode, HEAD)

Each site is the first three project frames of the allocation stack. Sites above 10 MB at peak. Retained = bytes live at the peak (`gb`) and at exit (`eb`); total = bytes allocated over the run (`tb`).

| peak MB | exit MB | total MB | project frames |
|---:|---:|---:|---|
| 78.5 | 57.7 | 156.8 | `<la_arena::Arena<syntax::ast::ExprNode>>::alloc (la-arena-0.3.1/src/lib.rs:343:19)`<br>`<syntax::ast::NodeArenas>::alloc_expr (syntax/src/ast.rs:1013:35)`<br>`<syntax::parser::Parser>::expr_at (syntax/src/parser.rs:1115:14)` |
| 74.2 | 74.2 | 267.4 | `<phpstan_rs::registry::summaries::Summarizer>::implicit_on (src/registry/summaries.rs:2432:26)`<br>`<phpstan_rs::registry::summaries::Summarizer>::implicit (src/registry/summaries.rs:2239:14)`<br>`<phpstan_rs::registry::summaries::Summarizer>::destroy_overwritten (src/registry/summaries.rs:2582:3` |
| 71.7 | 71.7 | 71.9 | `<<alloc::sync::Arc<[phpstan_rs::registry::summaries::Origin]> as core::convert::From<alloc::vec::Vec`<br>`<core::option::Option<alloc::vec::Vec<phpstan_rs::registry::summaries::Origin>>>::map::<alloc::sync:`<br>`<phpstan_rs::registry::summaries::Summarizer>::implicit_on (src/registry/summaries.rs:2422:44)` |
| 67.1 | 0.0 | 188.7 | `<phpstan_rs::registry::effects::Level>::classify (src/registry/effects.rs:5473:67)`<br>`<phpstan_rs::registry::effects::Resolve>::run (src/registry/effects.rs:4624:56)`<br>`<phpstan_rs::registry::Registry>::classify (src/registry/effects.rs:2525:17)` |
| 57.8 | 57.8 | 60.9 | `<syntax::memo::Memo<phpdoc::docblock::DocBlock>>::get::<phpdoc::docblock::tag_block::{closure#0}> (s`<br>`phpdoc::docblock::tag_block (phpdoc/src/docblock.rs:500:10)`<br>`phpdoc::docblock::apply_tag_recording_errors (phpdoc/src/docblock.rs:474:19)` |
| 28.4 | 0.0 | 29.7 | `phpstan_rs::registry::effects::settle::<<phpstan_rs::registry::Registry>::resolve_effects::{closure#`<br>`<phpstan_rs::registry::Registry>::resolve_effects (src/registry/effects.rs:1782:27)`<br>`<phpstan_rs::registry::Registry>::resolve_transitive_purity (src/registry/purity.rs:115:14)` |
| 27.2 | 27.2 | 27.2 | `<phpstan_rs::project::file_table::FileTable<phpstan_rs::project::FileState>>::with_paths::<core::ite`<br>`<phpstan_rs::project::session::Session>::add_walked (src/project/session.rs:2057:25)`<br>`<phpstan_rs::project::session::Session>::load_walked (src/project/session.rs:1981:18)` |
| 24.4 | 24.4 | 83.3 | `<phpstan_rs::registry::summaries::Summarizer>::implicit_on (src/registry/summaries.rs:2432:26)`<br>`<phpstan_rs::registry::summaries::Summarizer>::implicit (src/registry/summaries.rs:2239:14)`<br>`<phpstan_rs::registry::summaries::Summarizer>::dies (src/registry/summaries.rs:2246:18)` |
| 24.3 | 0.0 | 27.5 | `<phpstan_rs::registry::effects::Level>::passed (src/registry/effects.rs:5637:21)`<br>`<phpstan_rs::registry::effects::Level>::classify::{closure#1}::{closure#0} (src/registry/effects.rs:`<br>`<core::option::Option<u32>>::get_or_insert_with::<<phpstan_rs::registry::effects::Level>::classify::` |
| 21.9 | 21.9 | 31.2 | `<phpstan_rs::registry::members::Members<phpstan_rs::registry::function_sig::FunctionReflection> as c`<br>`<&mut <phpstan_rs::registry::members::Members<phpstan_rs::registry::function_sig::FunctionReflection`<br>`<core::option::Option<(syntax::name::Name, phpstan_rs::registry::function_sig::FunctionReflection)>>` |
| 21.3 | 21.0 | 25.5 | `<syntax::name::Name>::new_hashed (syntax/src/name.rs:260:14)`<br>`<syntax::name::Interner>::name_hashed::{closure#1} (syntax/src/name.rs:1799:16)`<br>`<syntax::name::WeakTable<syntax::name::NameData>>::get_or_insert::<<syntax::name::Interner>::name_ha` |
| 20.9 | 20.9 | 27.3 | `phpstan_rs::registry::summaries::origins_of::{closure#1} (src/registry/summaries.rs:620:39)`<br>`syntax::stack::guard::<core::option::Option<alloc::vec::Vec<phpstan_rs::registry::summaries::Origin>`<br>`phpstan_rs::registry::summaries::origins_of (src/registry/summaries.rs:494:5)` |
| 20.7 | 0.0 | 31.4 | `rayon::iter::from_par_iter::collect_extended::<alloc::vec::Vec<(phpstan_rs::registry::effect_table::`<br>`<rayon::iter::flat_map_iter::FlatMapIter<rayon::iter::len::MinLen<rayon::slice::Iter<(&syntax::name:`<br>`<phpstan_rs::registry::Registry>::resolve_effects (src/registry/effects.rs:1722:14)` |
| 20.0 | 20.0 | 22.0 | `phpstan_rs::registry::summaries::receiver (src/registry/summaries.rs:2976:56)`<br>`phpstan_rs::registry::summaries::origins_of::{closure#1} (src/registry/summaries.rs:621:31)`<br>`syntax::stack::guard::<core::option::Option<alloc::vec::Vec<phpstan_rs::registry::summaries::Origin>` |
| 19.3 | 19.3 | 19.3 | `<std::collections::hash::map::HashMap<syntax::references::RefKey, phpstan_rs::project::FileIdPosting`<br>`<phpstan_rs::project::ReferenceIndex>::from_file_refs::{closure#1} (phpstan-rs/src/project.rs:4614:2`<br>`<&<phpstan_rs::project::ReferenceIndex>::from_file_refs::{closure#1} as core::ops::function::FnMut<(` |
| 19.3 | 19.1 | 56.4 | `syntax::parse::parse_watching_in_arenas::<syntax::parse::Source> (syntax/src/parse.rs:0:0)`<br>`phpstan_rs::project::parse_recording::<syntax::parse::Source> (phpstan-rs/src/project.rs:0:0)`<br>`phpstan_rs::project::parse_file_recording_with_origin::<&str> (phpstan-rs/src/project.rs:4081:18)` |
| 16.3 | 16.3 | 64.5 | `<phpstan_rs::registry::summaries::Summarizer>::call (src/registry/summaries.rs:2535:40)`<br>`<phpstan_rs::registry::summaries::Summarizer>::record_call (src/registry/summaries.rs:0:0)`<br>`<phpstan_rs::registry::summaries::Summarizer as syntax::visit::arena::Visitor>::visit_expr::{closure` |
| 16.1 | 16.1 | 55.7 | `<phpstan_rs::registry::summaries::Summarizer>::call (src/registry/summaries.rs:2532:26)`<br>`<phpstan_rs::registry::summaries::Summarizer>::record_call (src/registry/summaries.rs:0:0)`<br>`<phpstan_rs::registry::summaries::Summarizer as syntax::visit::arena::Visitor>::visit_expr::{closure` |
| 15.7 | 2.0 | 27.5 | `boxcar::buckets::allocate_slice::<boxcar::vec::raw::Entry<phpstan_rs::registry::effect_table::Entry>`<br>`boxcar::buckets::allocate_race::<boxcar::vec::raw::Entry<phpstan_rs::registry::effect_table::Entry>>`<br>`<boxcar::buckets::Buckets<boxcar::vec::raw::Entry<phpstan_rs::registry::effect_table::Entry>, 58>>::` |
| 14.9 | 0.0 | 29.8 | `<syntax::token::TokenStream>::push_token (syntax/src/token.rs:421:27)`<br>`<syntax::lexer::Lexer>::lex_token_stream_with_worker_scratch (syntax/src/lexer.rs:686:20)`<br>`syntax::parse::parse_watching_in_arenas::<syntax::parse::Source> (syntax/src/parse.rs:398:19)` |
| 14.5 | 0.0 | 14.5 | `<std::collections::hash::map::HashMap<phpstan_rs::registry::ids::ClassId, (syntax::name::Name, phpst`<br>`<phpstan_rs::registry::Registry>::end_declaration_changes::{closure#6} (phpstan-rs/src/registry.rs:1`<br>`<phpstan_rs::registry::Registry>::end_declaration_changes::{closure#23} (phpstan-rs/src/registry.rs:` |
| 14.1 | 14.1 | 20.0 | `<phpstan_rs::registry::summaries::Origin>::elements (src/registry/summaries.rs:467:45)`<br>`<phpstan_rs::registry::summaries::Summarizer>::contents_from_origins (src/registry/summaries.rs:1774`<br>`<phpstan_rs::registry::summaries::Summarizer>::contents_of::{closure#1} (src/registry/summaries.rs:1` |
| 14.0 | 14.0 | 14.0 | `phpstan_rs::registry::index::built_declaration (src/registry/index.rs:1240:23)`<br>`phpstan_rs::project::file_declarations::collect::{closure#0} (src/project/file_declarations.rs:115:9`<br>`<syntax::declarations::Declarations<phpstan_rs::project::file_declarations::collect::{closure#0}>>::` |
| 12.9 | 3.8 | 65.6 | `<syntax::memo::Memo<phpdoc::docblock::DocBlock>>::get::<phpdoc::docblock::parse> (syntax/src/memo.rs`<br>`phpdoc::docblock::parse_shared (phpdoc/src/docblock.rs:307:14)`<br>`<phpdoc::docblock::parse_shared as core::ops::function::FnOnce<(&str,)>>::call_once (src/ops/functio` |
| 12.6 | 0.2 | 12.8 | `<phpstan_rs::registry::effects::TargetSet>::into_shared (src/registry/effects.rs:1529:9)`<br>`<phpstan_rs::registry::effects::Reach>::implicit_holder_targets::{closure#2} (src/registry/effects.r`<br>`<std::sync::once_lock::OnceLock<core::option::Option<alloc::sync::Arc<[(phpstan_rs::registry::effect` |
| 12.1 | 12.1 | 12.1 | `<std::collections::hash::map::HashMap<phpstan_rs::registry::ids::ClassId, phpstan_rs::registry::hier`<br>`<std::collections::hash::map::HashMap<phpstan_rs::registry::ids::ClassId, phpstan_rs::registry::hier`<br>`<phpstan_rs::registry::Registry>::precompute_member_owners (src/registry/hierarchy.rs:1030:32)` |
| 11.5 | 11.5 | 11.5 | `<<phpstan_rs::registry::bodies::FileBodies>::collect::Collect as syntax::visit::arena::Visitor>::vis`<br>`syntax::visit::arena::walk_method_decl::<<phpstan_rs::registry::bodies::FileBodies>::collect::Collec`<br>`<<phpstan_rs::registry::bodies::FileBodies>::collect::Collect as syntax::visit::arena::Visitor>::vis` |
| 11.2 | 8.2 | 22.4 | `<syntax::ast::NodeArenas>::alloc_expr (syntax/src/ast.rs:1015:25)`<br>`<syntax::parser::Parser>::expr_at (syntax/src/parser.rs:1115:14)`<br>`<syntax::parser::Parser>::expr (syntax/src/parser.rs:1101:14)` |
| 11.1 | 0.0 | 15.9 | `<(phpstan_rs::registry::effect_table::EffectIx, phpstan_rs::registry::effect_table::TransferIx) as <`<br>`<[(phpstan_rs::registry::effect_table::EffectIx, phpstan_rs::registry::effect_table::TransferIx)]>::`<br>`<[(phpstan_rs::registry::effect_table::EffectIx, phpstan_rs::registry::effect_table::TransferIx)]>::` |
| 10.9 | 10.9 | 16.7 | `<syntax::memo::Memo<phpstan_rs::deps::DocNames>>::get::<phpstan_rs::deps::doc_names_of::{closure#0}>`<br>`phpstan_rs::deps::doc_names_of (phpstan-rs/src/deps.rs:178:10)`<br>`phpstan_rs::deps::doc_site_references (phpstan-rs/src/deps.rs:105:22)` |
| 10.7 | 0.0 | 27.4 | `<phpstan_rs::registry::effects::Level>::shared_runs::{closure#1}::{closure#0} (src/registry/effects.`<br>`<phpstan_rs::registry::effects::Level>::shared_runs::{closure#1} (src/registry/effects.rs:5590:24)`<br>`<std::sync::once_lock::OnceLock<(alloc::sync::Arc<[(phpstan_rs::registry::effect_table::EffectIx, ph` |
| 10.6 | 10.6 | 34.1 | `<phpstan_rs::registry::summaries::Summarizer>::write (src/registry/summaries.rs:1690:27)`<br>`<phpstan_rs::registry::summaries::Summarizer as syntax::visit::arena::Visitor>::visit_expr::{closure`<br>`syntax::stack::guard::<(), <phpstan_rs::registry::summaries::Summarizer as syntax::visit::arena::Vis` |
| 10.1 | 10.1 | 12.4 | `phpstan_rs::registry::summaries::origins_of::{closure#1} (src/registry/summaries.rs:609:55)`<br>`syntax::stack::guard::<core::option::Option<alloc::vec::Vec<phpstan_rs::registry::summaries::Origin>`<br>`phpstan_rs::registry::summaries::origins_of (src/registry/summaries.rs:494:5)` |
| 10.1 | 10.1 | 16.3 | `<la_arena::Arena<phpstan_rs::registry::function_sig::ParameterReflection>>::alloc (la-arena-0.3.1/sr`<br>`<la_arena::Arena<phpstan_rs::registry::function_sig::ParameterReflection> as core::iter::traits::col`<br>`<la_arena::Arena<phpstan_rs::registry::function_sig::ParameterReflection>>::alloc_many::<core::iter:` |
