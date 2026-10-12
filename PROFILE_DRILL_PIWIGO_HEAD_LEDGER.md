# Piwigo profile drill at HEAD (`97b8f3f0`): measurements and one-at-a-time tasks

This ledger holds the measurements taken after the batch in `PROFILE_DRILL_PIWIGO_9071_BATCH_PLAN.md`, the tasks that follow from them, and a generated drill of the instruction and allocation profiles. It replaces optimising the whole drill at once.

## Ground rules

- **One task at a time.** Each task is one hypothesis and one change, in one commit with a pathspec. Nothing else is in the working tree.
- **Acceptance.** A change is kept only if it lowers the instruction count of the functions it targets, and wall time does not get worse under alternating timing. Otherwise it is reverted and the row says why.
- **Output.** Piwigo's output must stay byte-identical to the previous commit's, unless the task says it changes findings. Findings are tracked by task F1 (the 613 against 611 gap).
- **Correctness fixes stay.** A cost added by a correctness commit (EG.22, `d6d55292`) is traded off by changing its implementation, never by reverting it.
- **Drill existing data.** New profile runs, builds, or timing series need a decision first.
- **Labels.** Allocation sites are named by their project frames, not by the allocator's frame. Identical code is folded by the compiler, so one symbol can stand for several types.

## Measurements

Baseline `9071f48f`, the commit the batch profiled. HEAD `97b8f3f0`, which holds the batch and the two oracle fixes (`5ddb4436`, `97b8f3f0`). Pre-fix `97df097c`.

| id | measure | baseline | HEAD | change | source |
|---|---|---|---|---|---|
| M1 | callgrind off | 54.13 G | 49.36 G | −4.77 G (−8.8%) | `cl-9071f48f-off`, `cl-97b8f3f0-off` |
| M2 | callgrind warm (two unprofiled fills) | 42.14 G | 39.37 G | −2.77 G (−6.6%) | `cl-9071f48f-warm`, `cl-97b8f3f0-warm` |
| M3 | callgrind cold-with-store (stub cache warm, file cache empty) | 56.24 G | 53.13 G | −3.11 G (−5.5%) | `cl-9071f48f-cold`, `cl-97b8f3f0-cold2` |
| M4 | hyperfine off, 5 runs | 4.22 s ± 0.11 (`97df097c`) | 4.12 s ± 0.39 | within noise | `~/.cache/prs-e2v/31-hyperfine-off.json` |
| M5 | hyperfine warm, 5 runs | 3.56 s ± 0.04 (`97df097c`) | 3.77 s ± 0.29 | within noise, wider spread | `~/.cache/prs-e2v/32-hyperfine-warm.json` |
| M6 | heap total allocated | 7.43 GB, 46.17 M blocks (`df603944`) | 7.41 GB, 43.00 M | flat bytes, −7% blocks | `heap-head`, `heap/dhat-heap.json` |
| M7 | heap peak retained | 0.88 GB (`df603944`) | 1.48 GB | +68% | same |
| M8 | heap retained at exit | 0.66 GB (`df603944`) | 1.13 GB | +71% | same |
| M9 | heap, 2026-10-03 profile | 2.67 GB, 19.44 M (`5fb858c9`) | not a comparison: different commit and setting | | `dhat-20g` |
| M10 | Piwigo findings | 611 (reference, not on disk) | 613 | +2, see F1 | `~/.cache/prs-e2v/18-piwigo-97df097c.json`, `19-piwigo-97b8f3f0.json` |
| M11 | oracle corpus, debug assertions | 35 panics (`97df097c`) | 0 panics, exit 0 | fixed by fix A (`5ddb4436`) and fix T (`97b8f3f0`) | before: `~/.cache/prs-e2v/8-oracle-corpus.log`; after: `16-oracle-corpus.log` |
| M12 | tests (at `97df097c`) | — | lib 1295, fixtures 3999, workspace 2403 | all pass | `~/.cache/prs-e2v/2-lib.log`, `3-fixtures.log`, `4-workspace.log` |

Notes on the rows:
- The callgrind totals equal each run's summary totals.
- The M4 and M5 baseline is `97df097c`, the pre-fix binary. No `9071f48f` binary was built, so these two rows are not against the batch's starting commit.
- The settings of the `df603944` and `5fb858c9` heap runs are not recorded here. M6 to M8 are therefore not confirmed as like-for-like with HEAD's run, and M9 compares only the totals.
- The cold baseline (M3) had its stub cache present, checked by listing `cl-9071f48f-cold-cache`.
- The 20G plan's callgrind baseline (20.57 G at `5fb858c9`) and the batch's `9071f48f` baseline (54.13 G) are not on the same basis. Reconciling them is task R1.

## Tasks

Every task is OPEN except T12, which the batch did. The order is the suggested one, from the largest measured cause. Each task names the measure that will show it worked. The validation of each row, and what it changed, is in "Adversarial validation" below.

| id | cause (measured, validated) | hypothesis (validated status) | change | done when |
|---|---|---|---|---|
| T1 | `NameMap::get` hashes the property name, which `lookup_property_with_owner` takes as `&str`. 5.19 M of its 5.6 M calls come from `destroy_held` (effects.rs:4160), where the property is a `&Name` coerced to `&str`, so the hashing is part of T2's loop. The class key already uses the stored folded hash. | Merged into T2, since it is the same loop. Partly confirmed in its own right: a `Name` stores the hash of its original text, which equals the lowercase table key only for lowercase names. | Take T2 first. Then pass the `Name` through at effects.rs:4160 so the stored hash is used, with `lowered()`'s hash for case-insensitive tables. | `key_hash` calls from `lookup_property_with_owner` fall; output unchanged. |
| T2 | `destroy_held` runs for each Destroy operation and each class candidate: 1.52 M calls, 1.28 M from `implicit_targets'2` and 0.24 M from `implicit_targets`. Each call walks every property, and for each one calls the property lookup and `declared_holder`, which does two type bindings and a `class_references_by_name`. Its calls into the property lookups cost 3.99 G inclusive over 6.72 M calls. It has no memo of its own. | Unverified: a scalar property adds an empty holder, which is confirmed, but the cost is paid for each one. An unknown-typed property ends the walk early. A per-class memo is the simplest test. | Memoise `destroy_held` per class, keyed like the `implicit_methods` memo, and invalidate it with the same events. Keep EG.22. | The lookups from `destroy_held` fall; output and EG.22's fixture unchanged. |
| T3 | `classify` reserves `calls` to `listed` (effects.rs:5473). Each `Classification` lives until `resolve_effects` ends, because `settle` reads all of them (`&classified`). | Refuted as stated: the lists cannot be dropped after their wave. Unverified: over-reservation, since `listed` counts target lengths, which may exceed the calls pushed. | Measure the ratio of pushed to reserved entries. Only then shrink the reservation or build the vector at its final size. | Peak retained for these sites falls; instruction count not worse. |
| T4 | `implicit_keys` (fix A, `5ddb4436`) holds the `on` Arc of every recorded implicit operation. The rewrite at summaries.rs:2020–2035 replaces a call's `on` with a new Arc, so the old one lives until the scope ends. | Mechanism confirmed by code reading. Magnitude unmeasured: it needs a count of rewrites, or the pre-fix heap run (`97df097c`). | Count the rewritten entries. If they matter, keep the key without the replaced Arc, and keep the text equality the oracle checks. | Peak retained for this site falls; the implicit-call oracle still passes. |
| T5 | `implicit_on` pushes into `scope.calls` (summaries.rs:2432). The retention is 74 MB through `destroy_overwritten` and 24 MB through `dies`. Static estimate of `CallWrites`: about 140 bytes (a `Callee` of about 56 to 64 bytes, plus five boxed slices of 16 bytes), so the 74 MB is about 500,000 entries. | Unverified: the estimate is from field types, not a measured size. The vector also keeps up to twice its length in capacity. | Measure the size, then box rarely used fields or reserve from a per-body estimate. | Peak retained falls; instruction count not worse. |
| T6 | `Origin::Returned` held the `Callee` by value until `e9215833` (batch item 12d, 2026-10-10), which made each origin allocate an `Arc<Callee>` in `origins_of` (summaries.rs:609, 620, 626). `ffb3b177`, which an earlier version of this ledger named, changed only a serde attribute. | Attributed to `e9215833`, confirmed by the `Returned` lines at `df603944` and `9071f48f`. Whether one shared `Arc` per distinct callee helps is unverified. | Share one `Arc` per distinct callee within a body. | Peak retained falls; output unchanged. |
| T7 | `Memo<DocBlock>::get` (`tag_block`), 57.8 MB at peak. `Memo` is a bounded cache of 64 shards × 256 entries, and its byte bound counts text, not the parsed values. The memo predates `df603944` (`0cf20485`, 2026-10-02). At `df603944` it held 8 MB, so it was nearly empty. | Candidate `a01036ec` refuted: its change keeps the same entry semantics and bounds. The working hypothesis is that the memo now fills toward its 16,384-entry bound, which needs more distinct doc texts from its callers. Unverified. | Count the distinct texts at peak, then find which caller now sends more. Needs an instrumented run, which needs approval. | The entry count and the distinct-text count are known; the caller named. |
| T8 | `Parser::token_ref` (parser.rs:567) has no `#[inline]`, and it has 7.76 M calls from `parse_expr_bp_inner`. | Precondition holds. Release is thin LTO and dhat is not, but both use `codegen-units = 1`, and the caller is in the same crate, so LTO is not the reason it is out of line. The cost has to be measured in release. | Add `#[inline]`, then measure in the release build, not the dhat profile. | `token_ref` self falls in release; output unchanged. |
| T9 | `TargetSet::push` scans a list up to `SCANNED` (12) entries, and uses the `seen` set only beyond that. Its `seen` holds `Run = (EffectIx, TransferIx)`. | Refuted as stated: the set does not replace a list. The profile's `HashMap<ClassId, ()>` label is probably a folded instance of identical code, which the ledger's labelling caveat covers. The open question: why target sets exceed 12 entries. | Count target sets above 12 entries. Needs an instrumented run, which needs approval. | The count is known; the change is chosen from it. |
| T10 | `Registry::class_references(id)` exists (registry.rs:694). `class_references_by_name` (registry.rs:707) is called from `declared_holder`, once per property of each destroy in T2's loop: 4.7 M of its 9.7 M calls. | Refuted as stated: the id path exists. The fix needs the `ClassId` at the call site. Shared cause with T2: removing T2's per-property work removes most of these calls. | Subsumed by T2's memo. If T2 is not taken, thread the `ClassId` to `declared_holder`'s callers. | The calls to `class_references_by_name` fall; output unchanged. |
| T11 | `FilterMap` over `Chain<Map>`: 18.85 M calls, all from `callees_of` in `settle` (effects.rs:2692 to 2699). `callees_of` runs twice for each live classification: once to count callers (effects.rs:2712) and once to fill them (effects.rs:2727). The count pass sizes the caller lists exactly, so the twin pass is a memory trade, not a bug. `Resolve::run` self is 0.63 G, with 65k calls from `classify`. | Named: the two passes. Not yet measured: how much of the 18.85 M is the count pass, since each pass visits the same elements. | Choose between: one pass with growing lists (fewer instructions, more memory), or a per-classification cache of `callees_of` results (memory for the cache). Measure the peak with either before choosing. | The chain's cost falls, and the peak is no higher than the current one. |
| T12 | The lexer and parser group falls by 2.6 G in the name comparison. | Refuted as worded: `payload_at` and `token_at` still exist (token.rs:461, 503) with 10 call sites. They no longer show as separate functions, because they are inlined. The −2.8 G is attribution, not removed work. | None. | — |
| T13 | Peak retention with no task in this ledger: the expression arena (`NodeArenas::alloc_expr`, 78.5 MB at peak), `FileTable::with_paths` (27.2 MB), `ReferenceIndex::from_file_refs` (19.3 MB), and the effect table's buckets (15.7 MB). The arena and the file table hold the trees of analysed files, kept by `keeps_tree` (project.rs:1670). | Tracked elsewhere: the 20G ledger's row 19.2 (a batch frees each tree when its check ends) is OPEN, and it is the planned fix for the arena. | Take row 19.2 from the 20G plan rather than a new task here. Measure entry sizes for the rest. | Each site has an attribution row and a task, or a pointer to row 19.2. |
| R1 | The 20G plan's 20.57 G is a dhat-profile run with a warm stub cache and the file cache off, at `5fb858c9`. The batch's 54.13 G is a dhat-profile run with the cache off, at `9071f48f`. | Not reconciled. The 20G plan itself says that the 25.14 → 20.57 G comparison mixes code and codegen changes of about ±1 G, and that no gate run settled it. No run on one binary exists for both. | Run one commit's binary under both setups. Needs approval, since it is a profile run. | Both figures explained, or the target restated. |
| R2 | Wall time did not improve while instructions fell. | Refuted as an Ir explanation: rayon's own self cost is 0.07 G of 49.36 G, so spinning does not appear in the instruction counts. The earlier claim of 40.9 G spinning in `wait_until` came from a faulty aggregation, and is withdrawn. Unexplained: the gap itself. | Measure per-phase wall time, with alternating runs and more repeats. Needs approval. | A cause for the gap, or a decision that instruction counts cannot guide wall-time work. |
| F1 | Piwigo output: two findings not in the 611 reference. | The builtin-runs table (`bc9ab94f`) is refuted for `array_intersect`: its rows did not change. Neither finding is traced to a commit. | Find the commit between `9071f48f` and `97df097c` that adds them, and check each for being a true positive. | Both findings explained; the gate closed. |

## Adversarial validation

Method: each task's cause was checked against the code at HEAD, the callgrind and dhat data, and the git history. Each hypothesis was tested for something that would refute it. No build, run or profile was made, because the ground rules need a decision first for those. Where a check needs one, the row says so.

| task | verdict | evidence |
|---|---|---|
| T1 | partly confirmed; merged into T2 | 5.19 M of 5.6 M calls to `lookup_property_with_owner::<Name>` come from `destroy_held` (effects.rs:4160). The coercion of a `&Name` to `&str` is at effects.rs:4160. |
| T2 | cause confirmed; hypothesis open | Call counts from `implicit_targets`. No memo for `destroy_held` (effects.rs:4152). `element_classes` returns an empty set for scalars (effects.rs:3001), so scalars add no holder, but they still cost the lookups. An unknown-typed property returns `Holder::Unknown` at once (effects.rs:4167). |
| T3 | hypothesis refuted; over-reservation open | `classified` lives through `settle` (effects.rs:1711 to 1782). `listed` counts target lengths. |
| T4 | mechanism confirmed; magnitude open | The rewrite at summaries.rs:2020–2035 and the `implicit_keys` push at 2416–2430. |
| T5 | open; size estimated | `CallWrites` (summaries.rs:906) has five boxed-slice fields and a `Callee`. The size from the field types is about 140 bytes, which is a static estimate. |
| T6 | corrected | `git show ffb3b177` changes only a serde attribute. The `Returned` lines at `df603944` and `9071f48f` hold `Callee` by value. `e9215833` is after the baseline. |
| T7 | candidate refuted; open | `git show a01036ec` keeps the entry semantics and bounds (memo.rs). The memo's bounds are 256 entries and 256 KB per shard. |
| T8 | precondition confirmed; release needed | The release profile is `lto = "thin"` and `codegen-units = 1` (Cargo.toml:60 to 62). The dhat profile inherits `codegen-units = 1` and sets `lto = false`. |
| T9 | hypothesis refuted | `SCANNED = 12` at effects.rs:1505. `Run` at effects.rs:1485. |
| T10 | refuted as stated; shared cause with T2 | `declared_holder` (effects.rs:3166) calls `class_references_by_name(owner)`, once per property in T2's loop. |
| T11 | callers named; cause named | `callees_of` runs in both passes (effects.rs:2712 and 2727). |
| T12 | refuted as worded | `payload_at` and `token_at` still defined and called (token.rs:461, 503, 10 call sites). |
| T13 | tracked by 20G row 19.2 | `keeps_tree` (project.rs:1670) keeps the trees of analysed files. The 20G ledger's row 19.2 is OPEN and is the planned change. |
| R1 | open | The 20G plan's own note on the ±1 G codegen difference. |
| R2 | Ir explanation refuted | Rayon self cost 0.07 G of 49.36 G. |
| F1 | builtin table refuted for `array_intersect` | `bc9ab94f` leaves the `array_intersect` rows unchanged. |

Claims withdrawn from earlier in the review, and why:
- **Spinning in rayon's `wait_until` (40.9 G at HEAD, 10.8 G at baseline).** It came from an aggregation that double-counted. The parsed self costs contain no such cost; rayon's self total is 0.07 G.
- **`payload_at` and `token_at` removed by the parser change.** They are inlined, not removed.
- **T2's inclusive cost of 2.5 G.** The measured figure is 3.99 G.
- **T6 attributed to `ffb3b177`.** That commit changed only a serde attribute. The `Arc` came in `e9215833`.
- **T7 candidate `a01036ec`.** It keeps the same entry semantics and bounds.
- **T13 "retention by design".** The arena's retention is the planned change in 20G row 19.2, which is still open.

## Appendices

Generated from `~/.cache/prs-profile/cl-9071f48f-off`, `cl-97b8f3f0-off`, and `~/.cache/prs-e2v/heap/dhat-heap.json`. Regenerate rather than edit.

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
