# Piwigo performance ledger

Every optimization task for the Piwigo profile, on both lines of work, with its state and what blocks it. State as of 2026-10-15: this branch (`callgrind-20g`) at `36815464`, `master` at `975b3f05`.

It replaces `PROFILE_DRILL_PIWIGO_OFF_BUILD_LEDGER.md` (folded in, Appendix A) and the status text of `PROFILE_DRILL_PIWIGO_20G.md`. The 20 G report stays as the plan's design text. The copy on this branch is the one written at `d520c9fd` (2,002 lines); the current one is on `master` (3,354 lines). Its status claims were written at `5fb858c9` and are out of date: Appendix B re-evaluates every item against the code and the profiles, and Appendix C says how.

**Status words.** DONE (hash), PART (what is left), OPEN, READY (open, nothing blocks it), BLOCKED (on what), DUPLICATE (the other line has it), SKIP (reason), SUPERSEDED, DEFERRED (not an optimization, or waits on one that is not).

**Ids.** `OFF-x` is a row of the off drill, built on this branch (Appendix A has its full text under the id `x`). `20G-n.m` is a row of `master`'s `PROFILE_DRILL_PIWIGO_20G_LEDGER.md` (1,141 lines, about 1,100 rows; this ledger keeps the parent rows and names the child rows by range). `HEAD-Ox` is a row of `master`'s `PROFILE_DRILL_PIWIGO_HEAD_LEDGER.md`. `INT`, `GATE`, `DEC`, `CRATE` and `MEAS` rows are new here: the tasks that block optimization rows.

**Rules.**

- One commit per row, made as soon as the row's own check passes, with a pathspec (`git commit -m … -- <paths>`).
- A row that changes behaviour or layout keeps the Piwigo output byte-identical to the base's (`LC_ALL=C sort` both before diffing), unless the row says why it cannot.
- A saving is claimed only from a paired measurement: callgrind `Ir` summed over threads for instructions; `hyperfine -N`, alternating A B A B, `nice -n 19`, `/proc/loadavg` noted, for wall time. A "target" in this ledger is the cost of the code a row would replace, read from a profile. It is not a saving.
- `master`'s keep rule applies to rows built there: a change is kept only if it lowers instructions for its own targets and wall time does not get worse.
- Builds, profiling runs and `check-project` probes need the user's go-ahead. Heavy jobs run one at a time.

## 1. Two lines of work

Both lines were cut from `d520c9fd` (2026-10-03, the commit that added the 20 G plan). Neither has any row of the other.

| | `callgrind-20g` (this checkout) | `master` |
|---|---|---|
| head | `36815464` | `975b3f05` (`master2` and `master3` share `e32fd95d` with it and hold the builtin-runs and PHP-catalog work) |
| built | the off drill's rows (`OFF-`): 18 code commits, `3d5434b3` to `791d29cb` | the 20 G plan's items 0-9 and 10.1-10.2 (the 1,141-line ledger), effect resolution (ledger item E) and the batch of 128 `perf` commits that cut its cost (`PROFILE_DRILL_PIWIGO_9071_BATCH_PLAN.md`), then the level-9 drill (HEAD ledger) |
| ledgers | this file | the three files above and `WORK_STACK.md` (state 2026-10-11) |
| measured | off 25,625,190,065 → 20,928,053,254 Ir (−4.70 G, −18.3%) | at `97b8f3f0`: off 49.34 G, warm 39.37 G, cold with store 53.13 G; heap 6.28 GB allocated, 1.41 GB peak, 1.08 GB at exit; wall 4.12 s off, 3.77 s warm |
| what "off" means | stub cache and file cache both off | file cache off, stub cache warm |

- **The totals do not compare.** In `master`'s mode the code measured 20.58 G at `5fb858c9` and 49.34 G at `97b8f3f0` (`HEAD-C6`, `HEAD-C9`). About 26 G of the 49.34 G is effect resolution: `Registry::classify` 16.6 G on the workers (over every non-stub function, method, file top level and property hook), `Registry::resolve_effects` 4.5 G on the finishing thread (called from `resolve_transitive_purity`), `BodySummary::summarize` 5.4 G in the load. This line's code has none of it.
- **The lines conflict.** Merging `callgrind-20g` into `master` conflicts in 49 files (`git merge-tree`, nothing touched), among them `project.rs`, `session.rs`, `parser.rs`, `lexer.rs`, `relation.rs` and `purity.rs`. Applying each of this line's code commits to `master` alone gives the conflicts in §3C.
- **Savings from this line were measured on the `5fb858c9` code.** `master` has changed the code under most of them, so none carries over without a new measurement (INT-1).
- **The 20 G plan's status is `master`'s.** On this branch none of the plan's items is built; on `master` items 0-9 and 10.1-10.2 are.

## 2. Blocking tasks

These are not optimizations. Each gates optimization rows.

| id | task | kind | blocks | state |
|---|---|---|---|---|
| INT-1 | Decide how the two lines meet. Options: (a) apply this line's rows to `master` one at a time, each measured again in `master`'s protocol; (b) merge `callgrind-20g` into `master` and resolve the 49 files once; (c) drop the rows that duplicate or do not apply (INT-2, INT-3) and apply only the stub-build rows (`OFF-E1`, `P1`, `P2`, `P4`, `P5`) and the independent cuts and defect fixes. Recommended: (c), then (a) for what is left, because `OFF-P4` applies cleanly and most of the cuts conflict in one file, while a merge would settle P8 and P9 by accident. | decision, yours | every `OFF-` row's reaching `master`; `OFF-E8`; any comparison between the lines | OPEN |
| INT-2 | Rows that exist on both lines, to drop on one: `OFF-P3` is `master`'s `5ca07bd1` (built name bases get a hash index); `OFF-E2` is already in `master` (`stmt.rs` gates on `inline_var.is_empty()`); `OFF-E8` is `master`'s `3f2309b1` and `ffb3b177` (a cache file holds each distinct type and call once); `OFF-E4` is `master`'s `30e0af82` (an analysed file's derived data in the file cache), which this line measured and rejected (`OFF-E4`: warm −2% to −5%, cold +9% to +14% on two trees). | cleanup | `OFF-P3`, `E2`, `E4`, `E8` | OPEN |
| INT-3 | `OFF-P8a`, `P8b` and `P9` assume a context body is rarely read. On `master` effect resolution reads every body: `Registry::resolve_effects` classifies every non-stub function and method (`effects.rs`, `own_functions_by_id`, `members_with_slots`), and `BodySummary::summarize` costs 5.4 G in the load (4.3 G over 59,926 function bodies). So their saving on `master` is close to nothing unless classification becomes demand-driven (only declarations that analysed code can reach), and no row on either line asks for that. Needs a design before any port. | design, yours | `OFF-P8a`, `P8b`, `P9` | OPEN |
| GATE-1 | The Piwigo output gate is open: 613 findings against the batch's 611 (`HEAD-Q4`: find the commit between `9071f48f` and `97df097c` that adds `SearchFilterRenderer.php:442` and `ResponseBodyCallSiteVisitor.php:79`, judge each). | verification | GATE-2 | OPEN |
| GATE-2 | `20G-E2.V`: workspace tests, fixtures, report equal to the Part 1 baseline, `oracle` pass, callgrind, heap, `hyperfine`. `master`'s batch plan runs its verification once, after the batch's last item. | verification | `20G-E2.6`, `20G-E2.12.27`, `20G-E1.2.4` and `EG.2` (decided to be built after it) | OPEN (needs `20G-E2.11`) |
| GATE-3 | Order on `master`: level 9 first (the HEAD ledger's tasks, one at a time), then level 7 resumes at `20G-10.3`. Items run in order, each closed by its gate `10.V` to `19.V` (workspace tests, item tests, one callgrind run, one heap run), then `V.0`-`V.2` once. This is a process rule, not a technical dependency; the technical `needs` are in §3B. | process | every `20G-1x` row, as a sequence | in force |
| GATE-4 | Runs that wait for your go-ahead: `HEAD-O2` (the instrumented run), `O5` (parse-cost measurement), `O6` (release measurement), `O7` (counting duplicate pushes); on this line every build, profile or `check-project` probe. | approval | those rows | OPEN |
| DEC-1 | `OFF-D3`: how a binary carries the stub data (compile time or not). Decided again once the crate split's C5 sets its version key, on ≈0.2-1.5 G per cold run (C8) against 5.05 G in the old code. | decision | `OFF-P6` | OPEN |
| CRATE-1 | Level 2, the crate split from Stage C on (C4 lazy resolution, C5 stub layer, C10 reflection crate). Paused behind level 3 (140 review tasks) and level 6; `master`'s `WORK_STACK.md` has the order. C4 and C5 remove the eager stub build that `OFF-P1`-`P4` speed, so those rows pay only until then. | other plan | `OFF-P6`; the lifetime of `OFF-P1`-`P4` | paused |
| DEC-2 | `20G-17.1`'s stat index against the batch plan's recorded exclusion: a `stat`/`mtime` shortcut was judged unsound because a same-size rewrite that keeps its mtime (`rsync -t`, `tar`, composer installs) is served stale data. The design adds inode, device and the racy-file rule; whether it also compares `ctime` (which such a rewrite changes) is not written down. | decision | `20G-17.1`, `17.3` | OPEN |
| SEM-1 | Deferred semantic rows under a perf row: `20G-17.4` (cache key adds the configured versions and `short_open_tag`) needs `10.8` and `12.13`, and `12.13` needs `6.3`, which needs `6.2` (the stub index's version ranges). All are marked "after O3, not optimization". | other plan | `20G-17.4` | DEFERRED |
| MEAS-1 | Memory is up where the plan promised it would shrink. At `97b8f3f0` (cache off) the peak is 1.48 GB and 1.13 GB are held at exit, against 0.88 GB and 0.66 GB at `df603944` (the cache setting of that run is not recorded); the paired run with both caches warm gives 1.41 GB and 1.08 GB. The plan's end state was ≈0.37 GB at the start of the check. The batch plan lists the growth as unexplained. Until it is, a memory saving for `20G-19.x`, `HEAD-O9` or item 16 has no baseline. | measurement | memory claims of `19.x`, `16.x`, `HEAD-O9` | OPEN |
| MEAS-2 | Wall time stops improving after four threads (≈5.0 s at 4 and at 8 threads; serial ≈2.1 s, parallel ≈10.4 s at one thread; load average not logged for the 8-thread runs). The cause is not known (`HEAD-Q3`). | measurement | wall claims for any row that adds parallelism | OPEN |

## 3. Open optimization tasks

**How to read the numbers.** "Target" is the inclusive cost, in millions of instructions, of the code a row would replace, read from `master`'s off-mode callgrind profile at `97b8f3f0` (`cl-97b8f3f0-off`, 49.36 G, summed over all threads, calls in brackets). A target is the most a row could remove, not what it saves; a row's saving is claimed only from its own paired run. Rows whose work is memory or allocation count, or sits in a mode the off profile does not show, say so.

### 3A. `master`: level 9 and effect resolution

Effect resolution is about 26 G of the run (§1). The rows here are the ones the HEAD ledger and the effect part of the 20 G ledger still carry.

| id | task | target or evidence | state | blocked by |
|---|---|---|---|---|
| HEAD-O2 | Stop retaining implicit operations: `scope.calls` holds 98.6 MB at peak (74.2 through `destroy_overwritten`, 24.4 through `dies`, about 140 bytes per `CallWrites`) and `Arc<[Origin]>` another 71.7 MB; 699,322 records per run. The plan (`~/.claude/plans/recursive-growing-unicorn.md`, outside the repo) derives call-result destroys from a flag on the call, derives scope-end destroys from the stored origins, interns what stays and stores it once per cache file. | steps 0-3 committed (`6598647a`, `4a3a1f40`, `95b86df6`, `e4af599d`, `e15e1cae`, `14a156b4`, `975b3f05`); steps 4-11 open: box `Origin`'s `Place` payload, the interner, move the remaining operations, the cache table, the seed and builtin-scalar results and program-wide pruning each decided by measurement, validation, measurements, ledger. Last recorded gate: monolog `check-project` +0.51% instructions at `e4af599d`, so the keep rule was not met then. Warm profile: the cache decode of summaries is `Callee` 1,232 M (1.19 M calls) and `Vec<Origin>` 737 M (848,635 calls), which the plan's step 7 targets | PART (steps 0-3 committed on `master`) | GATE-4 (the instrumented run) |
| HEAD-O3 | `classify` reserves `calls` to `listed` (`effects.rs`); probe: 3.17 M pushes against 0.69 M reserved. Peak site 67.1 MB | open | READY | — |
| HEAD-O4 | `Origin::Returned` holds an `Arc<Callee>` allocated per origin (since `e9215833`): 38 MB retained at peak. Overlaps O2, which changes origins | open | READY (sequence after O2's step 6) | — |
| HEAD-O5 | The tag memo (`Memo<DocBlock>` in `tag_block`) holds 57.8 MB at peak; probe: 211,804 requests, 115,886 misses, 95,918 hits (45%); bound 16,384 entries | open; `20G-14.1` and `14.3` (a fingerprint-keyed memo bounded by weight) replace it, so build it there or settle which comes first | OPEN, needs approval | GATE-4; the choice against `14.1` |
| HEAD-O6 | `Parser::token_ref` has no `#[inline]`: 480 M self over 16.1 M calls in this profile (a `dhat` profile, `lto = false`; the shipped release is thin LTO) | open; measure in a release build | OPEN, needs approval | GATE-4 |
| HEAD-O7 | 6,801 target sets reach the set path at exactly 12 entries, receiving about 7.5 M pushes | open; count the duplicate pushes first | OPEN, needs approval | GATE-4 |
| HEAD-O9 | Memory sites that wait for the tree release: the expression arena (78.5 MB at peak, 57.7 MB at exit), `FileTable::with_paths` (27.2 MB), `ReferenceIndex::from_file_refs` (19.3 MB), the effect table's buckets (15.7 MB) | open | BLOCKED | `20G-19.2`; MEAS-1 |
| 20G-E2.3 | Effects on ids end to end. Landed: name-keyed lookups through the held `Name`, `TargetSet`, handle targets. Left: `Holder::Classes(Arc<[ClassId]>)`. A holder names classes that may not be loaded (an annotated type of a missing vendor class), which an id cannot say, so the id form needs a place for unloaded names first | `Holder` still holds `Arc<[Name]>` | PART | a design for unloaded classes |
| 20G-E2.4 | `ReachMemo` and the descendants index. Left: `classes_declaring` as `&[ClassId]`, implicit sets for unknown holders, the descendants index built with the relation passes, the debug comparison with `effects/reference.rs` | memoises `implementations`, `every_method`, `overrides_of_builtin`, `methods_named`, the descendants walk | PART | `20G-E2.3` |
| 20G-E2.6 | Cached classifications on warm runs, under a whole-run key (every file's path and content hash, the settings, the build). The row records that a key made of memo reads is unsound and only the whole-run key is | warm profile (39.37 G): `resolve_effects` 5.0 G on the finishing thread and `classify` 16.5 G on the workers, redone although nothing changed | OPEN | GATE-2 |
| 20G-E2.8 | Deferred class results by slot. Left: `deferred_class_result` (0.33 G, one map probe per call) and the per-call `resolved_deferred_class` scan | warm: `resolved_deferred_class` 303.4 M decoding 59,434 stub methods (the cost the 20 G report named R5, now reached through the deferred-result path); `lookup_own_method` decodes another 34,991 (164.0 M) | PART | — |
| 20G-E2.11 | Check-phase residual after E2.2-E2.9: fix only what the callgrind rerun shows. Left in the row: `&str` entry points that hash and compare the class name per call, the siteless dedupe of the list path, `builtin_runs` with arguments (0.99 G, 79 K calls), `relation_memo` | `<str>::is_folded_eq` 528 M (5.8 M calls), `<str>::fold_hash` 394 M (7.1 M calls); `Name::is_folded_eq` 557 M and `Name::fold_hash` 190 M over 32 M calls | PART | — |
| 20G-E2.12.17 | `call_effect` by handle. Landed: handle reads. Not done, by soundness: `stub_effect_results` and `effects` stay keyed by `EffectId`, because handles reset at `begin_declaration_changes` while both maps survive it | | PART | a design that survives the reset |
| 20G-E2.12.25 | `resolve_pseudo_at` returning `Cow<Type>` | re-evaluated, not built: a `Type` is a hash-consed `Arc`, so nothing is copied | SKIP | — |
| 20G-E2.12.27 | `class_result_overlay` skip: `end_declaration_changes` returns before building overlays unless resolved declarations are set | | OPEN (waits for E2.V) | GATE-2 |
| 20G-E2.12.28 | `Callee` node table in the cache, the vendor autoload cache and byte-prefix tests | built in code (`ffb3b177`, `f6f914d1`, `3c711a42`) | DONE in code, validation at E2.V | GATE-2 |

### 3B. `master`: the 20 G plan's remaining items

Rows keep `master`'s ids. "(changes output)" marks a row `master`'s ledger calls semantic: it is an optimization whose result may differ from today's. "Blocked by" is the open `needs` of the row in `master`'s ledger; a row with none is READY, apart from the process order of GATE-3. Child rows are in `master`'s ledger under `N.m.a`.

**Item 10, tokens in flat arrays** (plan estimate ≈1.7 G; `20G-10.1` and `10.2` are done, and `master`'s end-of-batch run puts the lexer-and-parser group at −2.6 G). Cost left: lexing 3,619 M (`lex_token_stream_with_worker_scratch`, 16,419 files; `Lexer::next_token` 2,454 M of it), the parser's cursor (`Parser::token_ref` 480 M self over 16.1 M calls, `skip` 324 M self over 6.8 M).

| row | task | target now | blocked by |
|---|---|---|---|
| 10.3 | `__halt_compiler` ends the stream where parsing ends today, its offset recorded | none (keeps behaviour) | — |
| 10.5 | String parts lexed inline in the parent stream; `Lexer::fragment`, the sub-parser, the context clone, the recorder fork and the scratch streams go | `build_interpolated` 84.9 M + `scan_interpolated` 71.8 M (148,947 strings) | — |
| 10.9 | Comments recorded only when asked; stub-rule type strings lexed through a scratch stream; completion its own stream; `Parser::new(TokenStream)` for tests | allocations; not in the profile | — |
| 10.V | Gate: workspace tests, token-stream, lexer, parser and Phar-stub tests, callgrind, heap | | 10.9 |

**Item 11, line starts and columns** (plan estimate ≈0.2 G). Since `b188d671` the line index is built at the first question, not for every file; 11,945 builds still cost 252.7 M, asked for by `member_removal_span` (181.6 M, a fix-it span computed for every class member), `record_doc` (52.4 M) and `mark_ignore` (16.9 M).

| row | task | target now | blocked by |
|---|---|---|---|
| 11.1 | `LineStarts` for every file, the column tables only for analysed files and open documents; `enum Lines` in `FileState` | the 252.7 M above | — |
| 11.3 | The block pass on `wide::u8x16` | part of the 252.7 M | 11.1 |
| 11.4 | Lexer warnings and parse failures record line and column on the worker; fragment lexers build no index | small | 11.1 |
| 11.5 | The server indexes context files on demand: the overlay first, the hash check otherwise | server only | — |
| 11.6 | The per-file cache stores `LineStarts`; no `Arc<LineIndex>` | warm only; not measured | 11.1 |
| 11.7 | Nothing in the front end reads lines: ignore marks, member spans and doc lines become offsets; anonymous-class lines come from `LineStarts` | the three callers above, 251 M | 11.1 |

**Item 12, lexer details** (plan estimate ≈0.35 G).

| row | task | target now | blocked by |
|---|---|---|---|
| 12.1 | The identifier lookup inlined; watch bits cached with each name in the cross-file cache | `Lexer::name_and_bits` 547.1 M (1,769,358) | — |
| 12.2 | Keywords compared a word at a time | `Keyword::try_from_bytes` 127.2 M (1,741,165) | — |
| 12.3 | Blank runs: one byte first, the `wide` mask for longer runs | inlined, not separable | 11.3 |
| 12.4 | Operators without a call | `Lexer::lex_operator` 168.3 M (3,707,887) | — |
| 12.5 | `RefKey::Constant` keeps namespace and name apart | `RefKey` equality 60.4 M (1,074,667); the key already stores its hash (`9f5d76d3`) | — |
| 12.6 | `UseMap::get` stops re-validating UTF-8 (nearly moot) | 11.7 M (59,723): the cost fell about tenfold since the 20 G profile (imports filed under a hash that ignores case, `f8e1cfea`) | — |
| 12.7 | Native type names matched without a lowercase copy | `NativeType::from_name` 22.1 M (135,859) | 12.2 |
| 12.8 | A qualified name written without spaces is one token | `parse_qualified_name` 240.1 M (479,995) and its spanned form 96.6 M | — |
| 12.9 | Numbers parsed skipping `_`; `strip_leading_backslash` returns a slice | `strip_digit_separators` 16.8 M (172,023) | — |

**Item 13, a file's products** (plan estimate ≈0.45 G). The per-file products are now the larger half of the load: `derive_file` 8,494 M (14,251 files) against `parse_recording` 7,375 M. Of it `FileBodies::collect` 5,766 M (`BodySummary::summarize` 4,318 M over 59,926 function bodies and 1,049 M over 14,251 file top levels, the shared body-pass walk, `InitFlow::of` 299.5 M over 57,506), declarations 1,793 M (of which `ClassReflection::from_def` 1,488 M over 12,546 classes), `GlobalFacts::of` 568.8 M, `recorded_references` 242.6 M. The plan's ≈0.45 G is the walks around the builders (the declarations walk ≈140 M, summaries, flows, global facts). Most of what those walks now cost is the builders' own work, which fusing the walks does not remove.

| row | task | target now | blocked by |
|---|---|---|---|
| 13.1 | The parser lists declarations, with today's guards, docs and scope; the declarations walk, its facts-gated walks and the declared-classes collection go | the walk is 81.6 M; the 1,668 M under `Declarations::found` is reflection building, which this row does not remove | — |
| 13.3 | `ImportScope`s published once | `NameContext::add_uses` 98.5 M (186,470) | 13.1 |
| 13.4 | One walk for `BodySummary`, `InitFlow`, `FileSites` and the registered families, in today's order. The batch plan excludes fusing the `Summarizer` (its order is part of its meaning), which cuts this row | `FileBodies::collect` 5,766 M, `GlobalFacts::of` 569 M | 13.1 |
| 13.5 | The old reference collector goes; the stub rules record their types' references | not in the warm-stub off profile | — |
| 13.6 | A file's bodies as one sorted slice reached through the declaration id | memory | 13.4 |
| 13.7 | Builders gather in scratch and emit exact-size slices | allocations | 13.4 |
| 13.8 | References in one vector per file, sorted once; doc sites hold the interned scope; `Declaration::name` is a `Name` | `recorded_references` 242.6 M (`doc_site_references` 212.4 M, `DocNames` 170.5 M), `RefKey` set inserts 139.7 M (810,413) | 13.3 |

**Item 14, PHPDoc** (plan estimate ≈0.35 G). `parse_shared` 875.2 M (145,004 lookups), `tag_block` 378.7 M, `apply_tag_recording_errors` 441.4 M.

| row | task | target now | blocked by |
|---|---|---|---|
| 14.1 | `syntax::memo::Memo` keyed by a fingerprint and bounded by weight (`siphasher`, `quick_cache`) | the tag memo's 57.8 MB at peak (`HEAD-O5`) | — |
| 14.2 | `parse_tag` returns `Arc<TagEffect>`; `apply(block, written, effect)` | `apply_tag_recording_errors` 441.4 M (75,726), `merge_from` 58.9 M (76,610) | — |
| 14.3 | The tag memo keyed by the pair's fingerprint | `tag_block` 378.7 M (76,622) | 14.1 |
| 14.4 | Eager lowering in `parse_tag`; `impl WrittenType for TypeNode` goes | `DocType` lowering, lazy at first read, 107.5 M (21,147); `parse_written_type` 122.8 M (111,006) | 14.2 |
| 14.5 | The text grammar's callers by kind (changes output) | | 14.4 |
| 14.6 | The resolution memo keyed by (written `Type`, context id) | `resolve_type_node` 86.6 M (34,825) | 14.4 |
| 14.7 | Tag tokens borrow byte ranges; the doc is lexed as bytes (changes output) | the tag lexer still builds `Vec<(PhpDocToken, Span)>` | — |
| 14.8 | One parse per doc; one winner rule per tag (changes output) | | 14.2, 14.7 |
| 14.9 | Docblock nodes in the owner's `la-arena`; `DocType` as (owner, `Idx`, `Type`) | allocations | 14.8 |

**Item 15, names as names** (plan estimate ≈0.6 G). Folded-name hashing and comparison by text now cost 922 M (`<str>::fold_hash` 394.4 M over 7.06 M calls, `<str>::is_folded_eq` 527.9 M over 5.81 M); the `Name` forms cost another 747 M over 64 M calls and are the target form, not a cost to remove.

| row | task | target now | blocked by |
|---|---|---|---|
| 15.1 | The checker's method memo and `class_memo` deleted | not measured | — |
| 15.2 | Every `String`-keyed map and identifier value takes PHP's identity (`FoldedName`, `Name`, `ConstName`, `ArrayKey`) (changes output: container-field keys become canonical array keys) | | 15.1 |
| 15.3 | `TableKey for str` and `FoldKey for str` deleted; outside text interned where it enters | the 922 M above | 15.2 |
| 15.10 | `NameData.last_segment`: a short name is a slice | not measured on this profile | 15.3 |
| 15.11 | Name resolution hashes the parts without building text; a callee's folded name reused | `Interner::name` 682.1 M (2,771,491), `name_hashed` 627.3 M (2,878,449) | 15.3 |

**Item 16, interned values** (plan estimate ≈0.1 G and ≈15 MB).

| row | task | target now | blocked by |
|---|---|---|---|
| 16.1 | `Name` as one allocation (`triomphe::ThinArc`), `last_segment` in the header | `Name::new_hashed` holds 21 MB at peak | 15.10 |
| 16.2 | Strong tables with removal at a count of two; type nodes as `ThinArc`; the type interner loses its per-hash `Vec<Weak<Node>>` (still there, `types/src/intern.rs`) | `types::intern::insert` 605.6 M (2,082,142), 250.4 M self | 16.1 |
| 16.3 | Literal types and shape keys interned as `ThinArc`; `Interner::str` and `WeakTable` go | `Interner::str` 38.5 M, `ShapeKey::from_bytes` 21.4 M | 16.2 |
| 16.4 | Every holder holds the handle (`FRONT`, memos' kept arguments, the lexer's tables) | memory | 16.2 |
| 16.5 | The type memos bounded by two segments | | — |
| 16.6 | A `loom` model of drop, lookup and sweep; the leak test after the edit replay (infra) | | 16.2 |

**Item 17, warm runs** (plan estimate ≈0.85 core-seconds). Costs read from the warm profile (`cl-97b8f3f0-warm`, 39.37 G).

| row | task | target now | blocked by |
|---|---|---|---|
| 17.1 | The stat index: a context file's content hash taken without reading the file when its stat fields match and it is not racily clean | `Reader::open_modified` 318.4 M (14,254 files); the reads are not separable | DEC-2 |
| 17.2 | Every content hash BLAKE3 with full digests; `twox-hash` removed | `ContentHash::of` ≈30 M (the batch plan puts it at 0.07% of the profile) | — |
| 17.3 | The server writes neither the index nor the base from overlays | server | 17.1 |
| 17.5 | The context base: the registry after the finishing passes over the context declarations, stored in item 6's container, keyed by the build, the settings and the files' hashes; a cold run inserts the context files, finishes, writes, then finishes the analysed files incrementally | `CachedFile::into_parsed` 41.3 M; stub method decodes 509.5 M (104,496) and class decodes 170.9 M (5,037); the summaries' decode belongs to `HEAD-O2` step 7 | 17.2 |
| 17.6 | Platform `stat` tests, each warm edit-replay case, the server loading from the base (infra) | | 17.5 |
| 17.4 | The per-file key adds the configured versions and `short_open_tag` | | SEM-1 |

**Item 18, the check phase's remainder** (plan estimate ≈1.6-2.0 G). The check phase is `check_program` 7,587 M (2,792 files). The API walker is already 50.7 M (it runs only on a file that names PHPStan, `f67cee0f`); SQL parsing is 10 M (88 M in this line's profile; `0f26c0e5`); regex groups ≈3 M (`3e83bff3`).

| row | task | target now | blocked by |
|---|---|---|---|
| 18.1 | §12.10: observers and one inference per node; the PHPStan-API walker and the literal types as families (changes output) | `infer_expr` 1,062.8 M (822,693 calls), `check_args` 531.4 M (145,061), `call_bindings` 133.2 M (50,078), `infer_binary` 246.6 M (53,049) | 13.4 |
| 18.3 | C1 step 5's tail: effects carried in the statement result; `has_yield` and `collect_throws` go (changes output) | `expr_side_effects` walk 39.4 M (194,864) | 18.1 |
| 18.4 | Variables as slots; the computed-name forms marked exactly; per-body key tables | `KeyTable::find_var` 123.9 M (1.27 M calls) | — |
| 18.5 | Scopes as radix arrays; locals listed by name | `OrdMap::get` 77.2 M (1,102,847), `imbl` insert 104.4 M (322,826) and its leaf and branch work ≈61 M, `Locals::clone` 38.9 M, `merge_scopes` 152.1 M | 18.4, 16.2 |
| 18.6 | Conditional subjects by parameter position | `resolve_conditional` 3.2 M | — |
| 18.7 | §12.11's rest: `MemberIndex` and the normalized-operand fast path (changes output) | `union_many` 364.8 M (537,811), `reduce_array_members` 187.9 M (25,959) | — |
| 18.8 | Type operations from borrowed parts; copy on first change; memos probed borrowed; `Display` into the formatter | `types::intern::insert` 605.6 M; `map_all` ≈66 M over three instances | 16.3 |
| 18.9 | One argument binding per call | `resolve_call_args` 70.4 M (449,917), `check_parameters` 519.2 M (145,061) | 18.1 |
| 18.10 | Per-function state reused | allocations | — |
| 18.11 | Generic signatures substituted per type | `substitute_class_templates` 200.2 M (132,012) | — |
| 18.12 | Messages formatted only when reported; built-in identifiers as `&'static str`. Part done: findings the level cuts are left unmade before formatting (`251353aa`, `c785b7a4`) | not measured | — |
| 18.13 | Constant types built once; two-operand unions without a `Vec` | allocations | — |
| 18.14 | Borrow, iterate or use scratch | allocations | — |
| 18.15 | `Locals` behind one `Rc`; throw, catch and loop frames as scratch | `record_throw_state` 8.7 M, `Locals::clone` 38.9 M (155,554) | — |
| 18.16 | Narrowing facts in the radix arrays; the separate walks' vectors go | `apply_narrowing_unguarded` 418.0 M (70,700) | 18.5, 18.1 |
| 18.17 | Per-text analyses once per distinct text; SQL once; one reused regex `Captures` | mostly done: see the item header | 14.1 |
| 18.18 | One checker per worker | `checker_for` 33.2 M, `Checker::new` 6.9 M (2,792) | — |

**Item 19, each tree until its last reader.** The rows are about memory; the instruction cost is small (`check_class_shapes` 8.3 M). `keeps_tree` exists (`20G-3.1`).

| row | task | target now | blocked by |
|---|---|---|---|
| 19.1 | The project-wide passes as families of the check traversal: architecture, banned constructs, class shapes, Composer hygiene, mock hygiene, unmatched ignores, the use-site index | | 13.4, 18.1 |
| 19.2 | `keeps_tree(role, open, mode, checked)`: a batch frees each tree when its check ends | expression arena 78.5 MB at peak, 57.7 MB at exit | — |
| 19.3 | Each worker takes the largest file left | wall time | — |
| 19.4 | Outside files run only the property-write family | | 18.1 |
| 19.5 | The tree cache: `quick_cache`, weighted by bytes, keyed by (`FileId`, content hash) | | 14.1, 17.2 |
| 19.6 | `FileState::ast` goes; every reader asks `ast_of` | | 19.2 |

**Gates.** `10.V`-`19.V` (workspace tests, item tests, one callgrind run, one heap run each), then `V.0` (the whole §11 validation: dumps, PHP-backed fixtures, platform cases, the `oracle` build) and `V.1`-`V.2` (the final profiles and the record). All OPEN, each blocked by the item before it (GATE-3).

**Ready now** (perf and output-changing rows with no open `needs`): 10.3, 10.5, 10.9, 11.1, 11.5, 12.1, 12.2, 12.4, 12.5, 12.6, 12.8, 12.9, 13.1, 13.5, 14.1, 14.2, 14.7, 15.1, 16.5, 17.2, 18.4, 18.6, 18.7, 18.10, 18.11, 18.12, 18.13, 18.14, 18.15, 18.18, 19.2, 19.3; `HEAD-O3` and `O4`.

**Longest chains** (the order a blocked row has to wait for): 15.1 → 15.2 → 15.3 → 15.10 → 16.1 → 16.2 → 16.3 → 18.8, and 16.2 → 18.5 → 18.16; 13.1 → 13.4 → 18.1 → {18.3, 18.9, 18.16, 19.1, 19.4}; 14.1 → {14.3, 18.17, 19.5}; 14.2 → 14.4 → {14.6, 14.8 → 14.9}; 11.1 → {11.3 → 12.3, 11.4, 11.6, 11.7}; 12.2 → 12.7; 17.2 → 17.5 → 17.6.

### 3C. `callgrind-20g`: the off drill's rows against `master`

The rows built here are in Appendix A with their evidence and §4B with their numbers. This table says what each one is on `master`. "Applying the commit" is `git merge-tree --write-tree --merge-base=<commit>^ master <commit>`: the number of files that conflict, and how many `master` commits touched the same files since `d520c9fd`. A clean result says nothing about whether the saving holds.

The stub-build rows (`E1`, `P1`, `P2`, `P4`, `P5`) show only in a run that builds the stubs. `master`'s own off runs start with a warm stub cache, so its profiles do not show them; its cold run costs 55.33 G against 49.34 G warm (`HEAD-C2`, `C9`), about 6 G for the build, more than the 5.05 G the old code paid.

| row | here | on `master` | applying the commit | blocked by |
|---|---|---|---|---|
| OFF-E1 `3d5434b3` | DONE (off −595.5 M, cold −445.1 M, warm −21.1 M) | the defect is present: `PLAIN_BASE` and `PHPSTAN_BASE` are statics, `resolved_stubs` builds them, `expect_files` takes no `Stubs` value | 13 files; 399 commits (`project.rs` alone 105) | INT-1 |
| OFF-P1 (rode on E1) | DONE | absent for the same reason | with E1 | INT-1 |
| OFF-P2, E13, D5 `c2cd1586` | DONE (off −530.8 M; cold with the per-file cache −24.6 M) | the premise is present: `Registry::store_cached` encodes the whole registry first, and `store_container` finds out afterwards whether it can write | 6 files; 183 commits (`registry.rs` 98) | INT-1 |
| OFF-P3 `a13f4ed2` | DONE (−343.8 M) | DUPLICATE: `5ca07bd1` gives a built base the hash index | 1 file, `name_table.rs`; 14 commits | INT-2 |
| OFF-P4 `0ce52479` | DONE (−288.3 M) | the premise is present: `build_closures` still searches from every file | clean; 9 commits on `stubs.rs` | INT-1 |
| OFF-P5 (rode on E1) | DONE (wall time, within noise) | absent | with E1 | INT-1 |
| OFF-E14 `6dd06124` | DONE (defect) | the defect is present: `instanceof` reads its class with `parse_unary`, so `$x instanceof static::$p` is refused | 2 files (the parser now builds arena ids); 14 commits | INT-1 |
| OFF-E15 `efe621ad` | DONE (defect) | the defect is present: `build.rs` does not hash `vendor/phpstan-src/stubs` | 1 file; 8 commits | INT-1 |
| OFF-E16 `480107e3` | DONE (defect) | absent: maps and sets of names write in table order. Not verified that `master` has a re-encode check that fails on it | 1 file, `name.rs`; 20 commits | INT-1 |
| OFF-E2 `60b0cd33` | DONE (−54.7 M) | DUPLICATE: `stmt.rs` already gates both walks on `inline_var.is_empty()` | 1 file | INT-2 |
| OFF-E3 `07a0f111` | DONE (−27.1 M) | absent; premise not measured | 1 file; 2 commits | INT-1 |
| OFF-E5 `5ec70c1e`, E6 `f2f65cbb` | DONE (−59.9 M, −23.2 M) | absent. `is_super_type_of` is 397 M inclusive (928,594 calls) with 36.2 M self, against 422 M and 109 M self over 1.45 M calls on this line, which fits the generation memo answering earlier; the shortcuts' share is probably smaller there | 2 files and 1 file; 17 and 4 commits | INT-1 |
| OFF-E7 `60e85393` | DONE (−29.9 M) | absent; overlaps `5ca07bd1` (bound types intern the class once) | 2 files; 51 commits | INT-1 |
| OFF-E10 `e081dbe9` | DONE (−52.5 M) | absent; premise not measured | 2 files; 103 commits (`checker.rs` 52) | INT-1 |
| OFF-P7 `a4035361` | DONE (−403.6 M) | the premise is present: the tag lexer's `tokenize` builds a `Vec` of every token eagerly. Overlaps `20G-14.2` and `14.7` | 1 file, `grammar.rs`; 27 commits (23 of them on `Cargo.lock`) | INT-1; `20G-14.2`, `14.7` |
| OFF-P8a `7726dacf`, P8b1 `11837a04`, P8b `791d29cb` | DONE (−348.5 M, −238.6 M, −847.6 M) | the saving needs bodies unread; see INT-3 | 9, 8 and 11 files; 283, 227 and 341 commits | INT-3 |
| OFF-P9 `c0dd2788` | DONE (−853.3 M) | the eager purity pass is replaced by effect resolution; see INT-3 | 13 files; 363 commits | INT-3 |
| OFF-E8 | OPEN here | DUPLICATE: `3f2309b1` (a cache file holds each distinct type once, a type is read as an index) and `ffb3b177` (the same for calls). Its blocker here, `20G-5.8` (names by index, `read_at`, a decode context), is DONE there | | INT-1 |
| OFF-E11 | OPEN here | SUPERSEDED: `20G-5.5` binds declaration provenance and duplicate sites to a `FileId`, so there is no `Arc<Path>` per origin to share | | INT-1 |
| OFF-E4 | SKIP here | DUPLICATE: built as `30e0af82`; measured there only inside the batch | | INT-2 |
| OFF-P6 | OPEN | stub data at compile time: not on `master` | | DEC-1, CRATE-1 |
| OFF-H1 (`PROFILE_DRILL_PIWIGO_OFF_HEAP.md` §11; no row in the retired ledger) | OPEN, READY: write the cache file from one buffer instead of building the file in a second `Vec` beside the body. −15.3 MB at the cold run's peak and −31.4 MB allocated per cold run, measured on the old code with the PHPStan base; a memory row, no instruction saving is claimed | the premise is present: `Writer::finish` copies the body into a second `Vec` (`container.rs`, which now also holds the type and call tables) | not tried | — |

## 4. Done

### 4A. `master`

Statuses are `master`'s ledger's, spot-checked in its code (Appendix C). Items 0-9 were closed by tests-only gates (`N.V`); no per-item instruction or memory effect was recorded. What is recorded is below the table.

| item | built | rows |
|---|---|---|
| 0, baselines and retired oracles | canonical type printer (`0fa1723c`), front-end dump (`64ddd7e3`…), full registry dump (`a4acabd2`), expression and PHPDoc records (`ba5cb5e1`), `cargo xtask corpus --full` (`f273b394`), server dumps (`9d30b919`…), heap profile by holder (`149a2256`), the `oracle` profile and the oracle code deleted (`693c5d58`), the dump store and the fast validation `0.11a`-`0.11j`; the effect fixes `0.1a`-`0.1f` | 0.1-0.10, 0.11a-j; `0.11` (the validation run itself) SKIPPED |
| 1, one session | role by strongest root, `Session::resolve` for the server, load through a file source with an overlay, `finalize` by value, `built_once` split, one project per folder, load off the message loop (`1ea4c475`, `e9b30e39`, `17d4c5e6`, `37fb5289`, `8e44f36b`, `b7ae40db`, `4f61b224`, `a0690baf`) | 1.2-1.8; `1.1` (resolved-path identity) DEFERRED |
| 2, the walk | one listing per directory, a task per directory, spellings built, parse per directory, order by construction, `FileTable` as a `HashTable`, typed manifests (`42b713d0`, `d64266dc`, `42140a68`, `61eb2aea`, `8d6a828d`, `553c1ddc`, `745401bb`) | 2.1-2.7 |
| 3, trees dropped on the worker | `keeps_tree`, the drop on the worker that allocated, reads into the worker's buffer (`8e75ba79`, `725e4264`); 3.4 under item 9 | 3.1, 3.2, 3.4; `3.3` DEFERRED |
| 4, the registry by ids | private fields, three id ranges, per-file declaration arenas, generations, one entry per member, results as tables by id, parallel passes, scope as a bitset (`e06503ad`, `1b8f79aa`, `210695c5`, `ac7ff5f9`, `9ae10e46`, `b4e4a4fc`…) | 4.1-4.8 |
| 5, the merge in parallel | id ranges by prefix sum, stable ids on edits, sharded merge, path-rank merges, `FileId` provenance, posting lists, the cache format (names by index, `read_at`, per-record checks, `SOURCES_HASH`) and its tests (`7c1c5fba`…) | 5.1-5.9 |
| 6, the stub scope | deferred types on demand; the debug-only scope count | 6.1, 6.6; `6.5`, `6.7`-`6.9` pulled forward into 5.8; `6.2`-`6.4` DEFERRED |
| 7, one relation memo | content generation, one memo, final answers only, two segments, `PHPSTAN_RS_NO_RELATION_MEMO` | 7.1-7.5; `7.4b` DEFERRED |
| 8, lean declarations | the six case-reference fields and both explicit-`mixed` flags leave the declarations (`451f0fa6`, `2d383a89`) | 8.1-8.4 |
| 9, trees in typed vectors | `peeled` a loop (`3f9ad5b8`), `la-arena` stores, nodes by index, payloads as byte ranges, stub rules applied while parsing, lists in one commit, small nodes; 9.3-9.8 are marked unvalidated | 9.1-9.8, 9.V |
| 10, tokens in flat arrays | `TokenStream` with three cursors, per-worker buffers, the name table to `NameId`s, escaped literals once into the file's bytes | 10.1, 10.2 |
| E1, correctness | Piwigo went from 664 findings to 610-613 against the 581 reference; the residual gaps are deferred to the PHP catalog on `master3` | E1.0-E1.5, E1.V |
| E2, cost of effect resolution | ids and memos for effects, per-class overlay maps, settle once per wave, isolated registries, the 9071 batch's items A-AE | E2.1, E2.2, E2.5, E2.7, E2.9, E2.10, E2.12.* except those in §3A |

Recorded outcomes, as `master`'s ledgers state them (not re-measured here):

- The run fell from 326 G (`631ad001`) to 42.2 G (`9071f48f`) as the cost of effect resolution was cut, output byte-identical.
- The batch took the off run from 54.13 G to 49.36 G (−4.77 G, −8.8%), the warm run from 42.14 G to 39.37 G (−2.77 G), cold with store from 56.24 G to 53.13 G (`9071f48f` to `97b8f3f0`). The lexer and parser group fell 2.6 G (`TokenStream`), `memcpy` 1.4 G, `lowered_text` 0.8 G, the folded-name comparisons 0.9 G.
- `HEAD-O1` (a destroyed object's holders memoised per class), paired, both stub caches warm: `19f78c3e` 46.62 G → 38.91 G (−7.71 G, −16.5%). `HEAD-O8` (`callees_of` once per classification): −0.70 G.
- Wall time (`HEAD-Q3`): `9071f48f` to `97b8f3f0` is 11% to 16% faster at one to four threads (14.03 s → 12.48 s at one) and 3% at eight. The single timings of the off and warm modes between `97df097c` and `97b8f3f0` (4.22 s → 4.12 s, 3.56 s → 3.77 s) are inside the noise.
- Heap rose (MEAS-1).

### 4B. `callgrind-20g`

Each row's output was byte-identical to the base's; the rows' own text (Appendix A) has the checks. Callgrind `Ir` in off mode (stub cache and file cache off), `dhat` profile with frame pointers.

| row | commit | Ir after | change |
|---|---|---|---|
| base | `5fb858c9`'s code | 25,625,190,065 | |
| E1 (with P1, P5) | `3d5434b3` | 25,029,730,463 | −595.5 M |
| P2, D5, E13 | `c2cd1586` | 24,498,907,030 | −530.8 M |
| P3 | `a13f4ed2` | 24,155,076,222 | −343.8 M |
| P4 | `0ce52479` | 23,866,820,730 | −288.3 M |
| E2 | `60b0cd33` | 23,812,168,909 | −54.7 M |
| E3 | `07a0f111` | 23,785,065,626 | −27.1 M |
| E5 | `5ec70c1e` | 23,725,134,207 | −59.9 M |
| E6 | `f2f65cbb` | 23,701,951,690 | −23.2 M |
| E7 | `60e85393` | 23,672,079,898 | −29.9 M |
| E10 | `e081dbe9` | 23,619,596,541 | −52.5 M |
| P7 | `a4035361` | 23,216,039,715 | −403.6 M |
| P9 | `c0dd2788` | 22,362,774,991 | −853.3 M |
| P8a | `7726dacf` | 22,014,267,117 | −348.5 M |
| P8b1 | `11837a04` | 21,775,661,606 | −238.6 M |
| P8b | `791d29cb` | 20,928,053,254 | −847.6 M |

Defects fixed along the way: E14 `6dd06124` (`instanceof` with a static property as the class), E15 `efe621ad` (the stub cache key left out `vendor/phpstan-src/stubs`), E16 `480107e3` (maps and sets of names wrote in table order). D6 is decided (skim unchecked, 2026-10-15), D1, D2 and D5 are decided.

## 5. Skipped, superseded, withdrawn

| row | why |
|---|---|
| OFF-P10 (`simdutf8`) | built, measured, reverted: −0.47 M against ≈ −22 M estimated; the checked bytes can't become a `String` without checking again under the workspace's no-`unsafe` rule |
| OFF-P11 (messages formatted when displayed; class-constant keys when dynamic constants are set) | built, measured, reverted: −0.65 M against 29.1 M estimated |
| OFF-E4 | built, measured, reverted: warm −2% to −5%, cold +9% to +14% on two of ten trees. `master` built the same idea (INT-2) |
| OFF-E9 | folded into the plan's regex-scan row (`20G-18.17`) |
| OFF-E12 | built, measured, reverted: the 420 M of method decoding moved to the workers, total −23 K |
| `20G-6.5`, `6.7`, `6.8`, `6.9` | pulled forward into `5.8` and `5.9` |
| `20G-9.3.d.b`, `9.3.d.d`, `9.3.e.e`, `9.3.f.c.b`, `9.7.b.e.b`, `9.7.c.b` | folded into other rows or not needed (reasons in the ledger) |
| `20G-E2.12.15`, `E2.12.25` | not applicable; not built (a `Type` is a hash-consed `Arc`) |
| the batch plan's exclusions | a `stat`/`mtime` shortcut for `ContentHash::of` (unsound; see DEC-2), fusing the `Summarizer` with other body passes (its order is part of its meaning), a recorded-transcript memo for calls (12% hit rate), per-thread off-thread drops, interned `Storage` ids and node-address keys (unsound) |

## 6. Not optimizations

Kept for the record. None is built from this ledger.

- **Deferred semantic and check rows on `master`** ("after O3, not optimization"): `0.11k2`-`0.11n2`, `0.11r`, `0.12a`-`0.15f` (the PHP catalog work), `1.1`, `3.3`, `6.2`-`6.4`, `7.4b`, `10.4`, `10.6`-`10.8`, `11.2`, `12.10`-`12.13`, `13.2`, `15.4`-`15.9`, `18.2`, `D.V`. `17.4` is a perf row held back by them (SEM-1). `9.4.d.b` and `9.4.d.c` are perf rows deferred into items 14 and 15.
- **Effect-resolution correctness rows on `master`:** `E1.2.4` and `EG.2` (reachability refinement, route (a), after E2.V), `E1.2.20`, `EG.11`, `EG.15`, `EG.16`, `EG.18`-`EG.20`, `EG.23`, `EG.31`-`EG.33`, `EL.6`.
- **Questions on `master`:** `HEAD-Q3` (MEAS-2), `HEAD-Q4` (GATE-1).
- **Questions on this line** (Appendix A, rows U1-U9): the parser against the whole `vendor/` corpus (U1), known parser gaps `$s{0}`, `(real)`, `(unset)` (U2), compile-time errors the parser does not report (U3), E1's deterministic reproduction (U4), what an `extract()` body still needs (U5), which truncation holds the 33 `offsetAccess.mightNotExist` reports (U6), readers left out of `d` (U7), P6's build-script cost (U8), a third check of the report's claims (U9). None gates an open optimization row; U8 matters only if `OFF-P6` is revived.

## 7. Where to resume

- **This line has one row nothing blocks: `OFF-H1`**, a memory row for cold runs. Every other row of the off drill is done, skipped, or blocked: `OFF-P6` on DEC-1 and CRATE-1, `OFF-E8` and `E11` by being duplicate or superseded on `master`. INT-1 comes first. My recommendation is its option (c), starting with the rows that are small or clean there: `OFF-P4` (clean), `E3`, `E6`, `E15`, `E16` (one conflicting file each), `E14` (two), then `E5`, `E7`, `E10`, `P7` (one or two files each), then the stub-build rows `E1`, `P1`, `P2`, `P5` re-derived, and INT-3 decided before P8 or P9 is touched. Each is measured again in `master`'s protocol.
- **`master`** continues in its own order (GATE-3): `HEAD-O2` (in progress), `O3`, `O4`, then the approvals for `O5`-`O7`, then `20G-10.3` onward. The READY rows are listed under §3B. `20G-E2.6` is the largest single target in either ledger (the warm profile's 21.5 G of classification and `resolve_effects`, 55% of 39.37 G) and waits for GATE-2, which waits for GATE-1.
- **Where the 49.36 G sits** (off, `97b8f3f0`): `classify` 16.6 G; the load 16.0 G (parse 7.4 G, per-file products 8.5 G); the check 7.6 G; the finishing pass 6.4 G, of which `resolve_effects` 4.5 G. The plan's open items 10-19 reach the lexer's 3.6 G, the products, the check phase and the names, not the classification.

## Appendix A. The off drill's rows

The rows of the retired `PROFILE_DRILL_PIWIGO_OFF_BUILD_LEDGER.md` (in git before this commit), kept whole because they hold the evidence behind §3C, §4B and §5. Their ids are unchanged; elsewhere in this file they appear as `OFF-<id>`. One status was corrected: D7 (the proposals it recommended are built). The report that holds each row's design is `PROFILE_DRILL_PIWIGO_OFF.md` (section numbers and the ids Dn, Bn, Cn, En, Nn are its); "item n" is an item of the 20 G plan. Rows P8a, P8b1, P8b and P9 were built on this line only and are not in `master`; see INT-3. Row H1 comes from `PROFILE_DRILL_PIWIGO_OFF_HEAP.md` and was never in the retired ledger; it is in §3C.

### A1. Decisions that gate the build

| # | status | decision | gates |
|---|---|---|---|
| D7 | DONE (moot: proposals 1-5 are all built, rows P1-P5; the build order below is complete) | Build proposals 1-5 now, in the order of the build order below, since the crate split removes the eager build they speed and every cold run pays it until then. Alternative: only proposal 4 and E1's fix. | P1-P5 |
| D6 | DONE (decided 2026-10-15, answered in the session: adopt D6, skim unchecked) | A context file that fails to parse keeps what it declares: what parses is recovered, what failed is unknown. A skimmed body is not checked: a syntax error in one is not found at load, and a body that does not parse when read is unknown. There is no check pass and no verdict cache, so D1's check requirement does not apply to P8b. Validity following the configured PHP version and one rule for the batch run and the language server stay as written in §10.8. | P8, parser gaps |
| D3 | OPEN (reopened by C8, §10.6) | How a binary carries the stub data. Decided again once the crate split's C5 sets its version key, on the numbers in §10.6 (≈0.2-1.5 G per cold run once C4 and C5 land, against 5.05 G today). Proposal 6 is not built in today's form. | P6 |
| D1, D2, D5 | DONE (decided 2026-10-13, report) | D1: every skimmed body is checked and each file's verdict cached. D2: a skimmed body's text is copied at load from the bytes the load reads. D5: each cache checks once, at its first store, whether it can write. | P8, P2, E13 |

### A2. Build order (D7)

1. **E1's fix**, which carries proposals 1 and 5.
2. **D5's write check**, which carries proposal 2.
3. **Proposal 3.**
4. **Proposal 4.**

Each step is measured with paired cold runs. B4 puts the ceiling at the 0.37 s
the warm stub cache saves today. The cuts, proposals 7-11 and the defects E14
and E15 are independent of this order unless a row's "needs" says otherwise.

### A3. Defects in today's code (§10.14)

| id | status | task | verify |
|---|---|---|---|
| E1 | DONE (3d5434b3; measured: Piwigo output byte-identical to the base in the off, cold and warm modes; Ir off 25,029,730,463 against 25,625,190,065 (−595.5 M), cold 25,238,288,982 against 25,683,423,164 (−445.1 M), warm 20,551,124,205 against 20,572,181,579 (−21.1 M); the cold run stored `stub-base-phpstan`, `stub-data` and `stub-index` and no plain base, and the warm run rewrote none of the three) | The stub state is a value owned by the entry point (`Stubs`, `registry/stub_set.rs`), built per layer on a thread of its own with the wait blocking in the OS (S2), on the pool the entry point hands in (S3), with the base chosen at `expect_files` (S4: `Project::loading`, then `expect_files(parsed, &Stubs)`; `finish_loading_with(&Stubs)` for a project that got none). Production code has no static for it; a `Registry` keeps an `Arc<Corpus>`. Not done: S4 is a runtime check (a registry with no corpus panics when asked), not a type; the regression test (U4) and `cargo test`/fixtures/clippy were not run, by instruction. | The regression test with `PHPSTAN_RS_NO_STUB_CACHE=1`, a one-thread pool and two nested `Project::new()` calls, with a watchdog (designed, not run); `cargo test --lib` cold; fixtures. Re-entry impossible by construction. |
| E14 | DONE (6dd06124; unit test over `static::$p`, `self::$p`, `$this->p`, a parenthesized expression and a plain name; the syntax lib's 269 tests pass, clippy clean; the two vendor files are not added as fixtures, the test holds their two forms) | Read `instanceof`'s right side with `parse_new_class_reference`: a plain name stays `InstanceofTarget::Name`, anything else becomes `InstanceofTarget::Expr`. | A parser test per form; the two files (`AbstractConfigurationProvider.php:41`, `BigInteger.php:177`) as fixtures. |
| E15 | DONE (efe621ad; the hash covers `vendor/phpstan-src/stubs`; the test reads the `rerun-if-changed` paths of `stub-sources/build.rs` and asserts each is in the key's inputs; master's `build.rs` is not changed, it takes the fix when this branch merges) | Add `vendor/phpstan-src/stubs` to the inputs `crates/phpstan-rs/build.rs` hashes (and master's `build.rs`). | A test that the hash's inputs cover every path `stub-sources`' build script reads. |
| E16 | DONE (480107e3; found by E4's debug oracle: `file_cache::store`'s debug check that a stored file re-encodes to its bytes fails on any class with several properties, because `NameMap`, `NameSet` and `FoldedMap` wrote their entries in table order, and a map rebuilt from them iterates in another; they write in name order now, the test fills one set in two orders and the mutation of removing the sort fails it; the syntax lib's 270 tests pass; the debug build then ran cold and warm over 215 files with no failure) | A map or set of names writes its entries in the order of the names. | The unit test; a debug run with the file cache on over a tree with classes. |

### A4. Proposals (§10.1-§10.11)

| # | status | task | estimated saving | needs | verify |
|---|---|---|---|---|---|
| P1 | DONE (rode on 3d5434b3; verified at 0ce52479: a two-file project that never mentions PHPStan and one that does give output byte-identical to the base binary's; the first stores only `stub-base`, the second only `stub-base-phpstan`, where the base binary stored both; a mention only inside a body is P8's and not built) | Resolve only the stub base the files need: the project starts with no base and takes one at `expect_files` (E1's S4). | −355.1 M serial; the plain base's finishing run ≈35-52 M (estimated) | E1 | Output identical on a project that mentions PHPStan and on one that does not; a file that mentions PHPStan only inside a body (P8) still picks PHPStan's base. |
| P2 | DONE (c2cd1586; measured: off run 24,498,907,030 Ir against 25,029,730,463 at E1's commit, −530.8 M, output byte-identical; less than the estimated −0.71 to −0.74 G; a read-only cache directory run prints one line per cache and gives the same output) | Serialize only what a store keeps: gate `stub_cache::store`, `StubIndex::store_cached`, `Registry::store_cached` and the stub-data store on D5's write check. | −1,005.3 M serial; ≈ −0.71 to −0.74 G after P1 | D5's check | A cold run still checks `store_entries`' duplicate-name test; an unwritable cache directory pays no encoding. |
| P3 | DONE (a13f4ed2; measured: off run 24,155,076,222 Ir against 24,498,907,030 at D5's commit, −343.8 M, output byte-identical; the test of the lookups against the ordering is written and compiled, not run) | A hash index for stub bases built in memory: `freeze` builds the index `store_entries` already builds. | ≈ −0.29 G | — | A built-then-frozen base answers every lookup as the bisection did (to be checked by a test). |
| P4 | DONE (0ce52479; measured: off run 23,866,820,730 Ir against 24,155,076,222 at P3's commit, −288.3 M, output byte-identical; the stub index a cold run stores is byte-identical to the one stored before, so the closures are equal file by file) | Look each stub file's dependents up once in `build_closures`: per-file dependent lists, closures in strongly-connected-component order. | ≥ −0.2 G serial | — | The closures equal today's, file by file (to be checked by a test). |
| P5 | DONE (rode on 3d5434b3; paired wall time, Piwigo, file cache off, stub cache warm, `hyperfine -N` 6 runs after 1 warmup, alternating base/new twice, `nice -n 19`, loadavg 4-7 throughout: base binary mean 3.380 s and 3.286 s, the binary at 0ce52479 3.057 s and 3.206 s, medians 3.423/3.220 against 3.046/3.161. The new binary carries E1 to P4 together, so P5's share alone is not separated, and the difference is within one standard deviation of the base's runs; it is not slower. An earlier pair of runs overlapped another and was discarded.) | Build the stub corpus beside the parse: E1's S2 and S3; the corpus's `par_iter` moves into `worker_pool().install`; `check-project` makes the stubs before listing files. | wall time: ≈0.14 s alone, the whole ≈0.25 s stretch with P1 (a model); no Ir change | E1 | Paired wall-time runs; `PHPSTAN_RS_THREADS` and the 64 MB stack apply to the corpus. |
| P6 | OPEN, blocked | Build the stub data at compile time into every binary. Not built in today's form. | −5.05 G today; ≈0.2-1.5 G per cold run after the crate split's C4 and C5 | D3; the reflection crate (C10) | If built: B10's guard test, an embedded-equals-runtime-build test, the override equal to the profile (B9). |
| P7 | DONE (a4035361; measured: off run 23,216,039,715 Ir against 23,619,596,541 at E10's commit, −403.6 M against the ceiling of ≈ −0.37 G, Piwigo output byte-identical; the tag parser pulls tokens from the lexer (`Lexer::next_token`) into a buffer and fills it only as far as a lookahead or rewind asks, and a tag's description is `Description`, the source from the first token the parser did not take, which `text()` lexes again and joins with single spaces and `starts_with_optional_marker()` answers from the first tokens, on the same rule as the old test (the joined text without spaces starts with `[optional]`, so `[opt ional]` still matches); a debug build compares every description's text with the tokens the parser goes on to read, which fails with the offset moved to the token's end (23 tests); five new tests (text for `//`, string escapes, floats, overflowing integers, `<b>`; the offset after rewinds; the marker against the old rule on 14 inputs; a serde round trip; no allocation without a description); the warm figure was not measured; the workspace tests, clippy of the other crates and the fixtures not run) | Tokenize a PHPDoc tag's description only when it is read: the parser pulls tokens, records the description's offset, and re-lexes on request; `[optional]` becomes a three-token prefix test. | ≈ −0.37 G off, ≈ −0.08 G warm (ceilings); ≈ −0.04 G after P6 and item 14 | — | Re-lex, never slice (`//`, string escapes, floats, overflowing integers, single-space join); every phpdoc test; the memo keys unchanged. |
| P8a | DONE (7726dacf; measured: off run 22,014,267,117 Ir against 22,362,774,991 at P9's commit, −348.5 M, Piwigo output byte-identical in the optimized build, in a debug build with the oracle on (cache off) and in two warm-cache debug runs. A context file with no anonymous class, no nested declaration and no declaration as a statement of a body (`FileFacts::has_declaration_in_body`, new, set by the parser) keeps each function and method body as its bytes (copied at load from the bytes the load reads, D2) with the index of the name context in effect where it is declared; `FileBodies` makes the summary and flow in a `OnceLock` when a pass first reads the body, by `syntax::parse_function_body` over `Lexer::fragment_bytes(text, base)` with that context, so spans stay absolute. The load no longer walks those bodies for their summaries, flows or nested declarations. The per-file cache stores a skimmed body as its range and context (`NameContext::to_stored`/`from_stored`, a fresh version on decode) and `CachedFile::into_parsed` takes the bytes and copies the text again, whether or not the body was read. Only context files skim (`parse_file_with_names`'s `skim_bodies` is the loader's `cacheable`); analysed files and `ast_of` are unchanged. A debug build reads every skimmed body at load and asserts its summary and flow equal the tree's. Tests: `registry::tests::skimmed_bodies` (6: tree equality across namespaces, `use function`, goto labels, names of the declaring context, cache round trip with re-encode equality, stored the same read or not, a file that is not UTF-8, files with a declaration in a body keep every body read); three mutations (context dropped, base offset 0, the `has_declaration_in_body` condition removed) fail them; syntax 270, lib 1069, fixtures 3830 pass. Not run: workspace tests other than those, clippy. Not measured: peak memory (the kept text is ≈18 MB on Piwigo, D2).) | The first slice of P8: bodies of context files made lazy at the summary and flow, the parse itself still done in full at load. | −348.5 M measured (below the whole-P8 estimate because the parse, the lexer's token layer and the drops are still paid) | P9 | Debug build asserts lazy = eager for every skimmed body. |
| P8b1 | DONE (11837a04; measured: off run 21,775,661,606 Ir against 22,014,267,117 at P8a's commit, −238.6 M, Piwigo output byte-identical in the optimized build and in a debug build with the oracles on (cache off). The parser records two more file facts: `has_global_or_include` (a `global` statement or an `include`/`require` expression, also in a `{$...}` fragment, whose flags are now merged into the file's) and uses `has_declaration_in_body` (P8a). `GlobalFacts::of_file` walks no function, method or closure body when the file has no `global` and no include, because such a body adds no site; `file_declarations::collect` enters no body unless the file mentions `define`, has a nested declaration or has a declaration as a statement of a body (`for_each_declaration_with`). A debug build walks the bodies too and asserts the same global facts, and the existing count of declarations of a full walk still guards the declarations. Tests: two fact tests in `parse`, mutation (bodies never walked) fails 2 global-variable tests; syntax 272, lib 1069, fixtures 3830 pass. Not run: workspace tests other than those, clippy.) | The load's walks over context bodies (`GlobalFacts`, declarations) skipped by file facts. | −238.6 M measured | P8a | Debug build walks the bodies too and compares. |
| P8b | DONE (791d29cb; measured: off run 20,928,053,254 Ir against 21,775,661,606 at P8b1's commit, −847.6 M, Piwigo output byte-identical in the optimized build and in a debug build with the oracles on (cache off and stub cache off). The parser skims a context file's function bodies that the load keeps as text (`parse_skimming`): the tokens of a body that opens at `{` are counted to the matching `}` and not parsed. A doc comment, `Eof`, inline HTML, a string with a complex part, a class/interface/trait/enum/global/include/require/yield/goto keyword, a named `function`, `define`, a name that can be `phpstan\...` rewinds the tape to a full parse of that body. Lexer errors and unbalanced braces are still found; a syntax error in a skimmed body is not (D6). Skimmed bodies' references are registered by `complete_skipped_references`, which parses every skimmed body once, so it runs only where a lookup needs it: incremental updates and removals, not a project built once. The first measurement of this commit, with the completion running in the batch, was 23,567,209,391 Ir (+1.79 G): the completion parsed 46,413 bodies for 2.36 G Ir. In a project built once the only `dependents` lookup is the one finishing pass, and every name its files declare is among those that changed, so what a skipped body refers to adds no declared name to its scopes; a debug build completes the references anyway and asserts both scopes equal (it ran once on Piwigo, with and without the stub cache). Tests: 6 parse tests (skipped bodies parse to the whole-parse content, each trigger, member-operator keywords and closures, PHPStan contexts, syntax error not found but lexer and brace errors found, references union), 3 project tests, mutation (`define` never triggers) fails the oracle at `file_cache.rs`; syntax 278, lib 1072, fixtures 3830 pass. Not run: workspace tests other than those, clippy; peak memory not measured.) | The rest of P8: the parser skims context-file bodies to the matching `}` and rewinds to a full parse on a trigger. | −847.6 M measured | P8a; D6 | A debug build parses every skimmed body eagerly and asserts equal summaries, flows, declarations, global facts and references (`file_cache::assert_same_but_bodies`); a debug build of a built-once project asserts the scopes equal with the references completed. |
| P9 | DONE (c0dd2788; measured: off run 22,362,774,991 Ir against 23,216,039,715 at P7's commit, −853.3 M, Piwigo output byte-identical; keyed by names, not by item 4's ids, so it did not wait for them. `Registry::function_is_pure`, `method_is_pure`, `function_property_writes` and `method_property_writes` take the `BodySource` the checker holds and search from the declaration asked about: an iterative depth-first search over the calls of its body, a call back into a declaration the search is inside of answering impure / `Anything`, which is the least fixed point because a declaration in or reaching a cycle never leaves it; every declaration the search settles is remembered in `Registry::demand` (two `RwLock`ed maps, nothing held while a search runs, a copy of a registry starts empty). The eager passes, `purity_dependencies`, `is_transitively_pure`, `clear_purity`, `property_writes` and `property_write_classes` are gone from the build. The incremental path takes the answers found since the last finish (`reset_demand`) and, once the declarations have settled, finds each again and marks the class or function whose answer differs (`changed_since`), so a file is re-checked on the same ground as before, for the answers something read. A debug build settles every declaration at once, as the build did, and each answer given on demand is asserted equal to it wherever the bodies at hand are the build's. Tests: `registry::tests::demand` (7: chain, cycle asked from any member, impure dependency, methods, `NoBodies`, property-write verdicts, order of askers), one new fixture (a cycle of methods with a return type), lib 1056 and fixtures 3829 passing in the test profile; both mutation checks (a stack hit answering pure; a stack hit answering `Nothing`) fail the oracle and the new tests. Not run: workspace tests other than the two, clippy.) | Purity and property writes on demand: a memoized depth-first search from the declaration asked about (least fixed point), the memo cleared on each edit; `check_declared_purity_tag` tests the `@phpstan-impure` / `@impure` tag first. | −853.3 M measured (estimated ≈ −0.3 to −0.4 G beyond item 4) | — | A debug build runs the eager pass and compares every answer given on demand; order of askers does not matter. |
| P10 | SKIP (not kept: built, measured and reverted, nothing committed. `simdutf8::basic::from_utf8` in the file read, the text then made with `to_owned` because the workspace denies `unsafe` and so cannot turn the checked bytes into a `String` without checking them again: off run 23,215,574,656 Ir against 23,216,039,715 at P7's commit, −0.47 M against the estimated ≈ −22 M, Piwigo output byte-identical. The copy costs what the faster check saves. Wall time not measured; Ir shows no gain to look for) | Check UTF-8 with SIMD (`simdutf8`). | ≈ −22 M (estimated) | — | The check stays; the `LineIndex` `Utf8Chunks` is item 11's. |
| P11 | SKIP (not kept: built, measured and reverted, nothing committed. Both parts built: (a) `Diagnostic.message` became a `Message` that dereferences to `str` and can be made by a closure on first read, with `report_lazy` used by the `offsetAccess.mightNotExist` report, and (b) `class_constant_type` returning the declared type while `dynamic` is empty. Off run 23,215,388,146 Ir against 23,216,039,715 at P7's commit, −0.65 M against the estimated 22.5 M + 6.6 M, Piwigo output byte-identical. The 22.5 M was 55 messages at ≈ 409 K Ir; the cause is not traced; the likely one is that E10 now drops, before formatting, the reports made inside bodies that call `extract()`, where B3 found most of the 55 were discarded. The one annotated `report_missing_offset` call line in the P7 profile costs ≈ 12 K Ir. What is left of (a) formats a message only for a rare setting (`reportPossiblyNonexistentGeneralArrayOffset`), at the price of a new message type touching 76 readers, so it is not kept without a measurement that shows it) | (a) A reported message formatted when displayed (a template and its operands in `Diagnostic.message`); (b) class-constant keys built only when a dynamic constant is configured. | 22.5 M (a), 6.6 M (b) | — | A message's text must not depend on state that changes after the report; the baseline and `ignore_errors` compare the displayed text; (b) folds the class name as `ConstantResolver::new` does. |

### A5. Cuts the investigations found (§10.13)

| id | status | task | estimated saving | needs |
|---|---|---|---|---|
| E2 | DONE (60b0cd33; measured: off run 23,812,168,909 Ir against 23,866,820,730 at P4's commit, −54.7 M against the estimated ≈ −50 M, Piwigo output byte-identical to the base's; the same binary also carries E14's parser change, which changes nothing on Piwigo; tests, clippy and fixtures not run) | A statement's assigned and defined names walked only when it has `@var` tags. | ≈ −50 M | — |
| E3 | DONE (07a0f111; measured: off run 23,785,065,626 Ir against 23,812,168,909 at E2's commit, −27.1 M against the estimated ≈ −35 M, Piwigo output byte-identical; tests, clippy and fixtures not run) | A method or static call probed for a PHPUnit assertion only when its written name starts with `assert`. | ≈ −35 M | — |
| E4 | SKIP (not kept: built once and reverted, nothing committed. Judged on ten general trees under `vendor/` with alternating repeats: the warm saving is 2-5% CPU, inside the ±10% spread of one run; a cold run costs +14% on composer and +9% on rector-src for the extra entries written; the cache grows by about 7 KB per file (137 MB on symfony). Not measured: a project whose analysed files change between runs, where hits are rarer and each changed file still pays the write, so it would be no better. The evidence is below; the E4 binary is `~/.cache/prs-profile/p4/e4-phpstan-rs`. Earlier text of this row: Piwigo output byte-identical in cold and warm; a hit for an analysed file reads the entry and parses the tree: warm 14,199,535,864 Ir against 14,223,265,225 without it (−23.7 M, where −250 to −300 M was estimated), cold 25,773,237,521 against 25,360,553,359 (+412.7 M for storing 2,792 more entries, 98 M to 116 M of cache); the decode of an analysed file's entry costs about what the steps it replaces do, so the saving on this one corpus is small. Not decided: how the cold store and the warm hit trade on a project whose analysed files change between runs. A debug oracle that a hit's products encode to the same bytes as a full parse's passed on 215 files.) | The analysed files stored in the per-file cache too. | ≈ −0.25-0.3 G net, per-file cache warm | — (debug check that a hit's products encode to the entry's bytes) Ten other trees under `vendor/` (`check-project --no-composer .`, so every file is analysed, E4's best case; the pre-E4 and the E4 binaries, one cold run and the mean of three warm runs each, `nice -n 19`, loadavg not logged; wall time is too noisy to read, laravel's pre-E4 run took 22 s against 8 s cold with E4): output identical in all ten; warm CPU time (user + system) −4% to −10% on eight of them (guzzle, wordpress, psalm, league-commonmark, rector-src, drupal, laravel, symfony), +4% on composer and +14% on doctrine-orm; cold CPU time +0% to +26% (doctrine-orm) where it rose; the cache grew to 2-137 MB (about 7 KB per file). Single runs; `~/.cache/prs-profile/e4/corpora/`. doctrine-orm repeated: four alternating rounds (start order swapped each round), each a cold run and two warm runs per binary, loadavg 0.5-2.7, output identical every round: warm CPU 8.73 s without E4 against 8.56 s with it (−2%, round values 7.5-10.2 s, so inside the noise), cold CPU 9.17 s against 9.27 s (+1%), wall 1.55 s against 1.58 s; the +14% and +26% of the first run were noise; `~/.cache/prs-profile/e4/repeat/`. The other trees that rose, repeated the same way (four alternating rounds, output identical, loadavg in the range given): composer (0.7-1.9) warm CPU 3.88 s against 3.82 s (−2%), cold 3.83 s against 4.35 s (+14%, higher in 3 of 4 rounds); rector-src (1.8-4.7) warm 10.96 s against 10.78 s (−2%), cold 11.39 s against 12.43 s (+9%, higher in 3 of 4); wordpress (4.7-6.0) warm 7.88 s against 7.46 s (−5%), cold 8.08 s against 7.93 s (−2%); psalm (5.8-6.9) warm 23.33 s against 22.85 s (−2%), cold 23.45 s against 23.98 s (+2%, rounds 20.7-27.0 s). So the warm rises were noise and the warm saving is 2-5% on these trees, within the spread of single runs; the cold cost is real on composer and rector-src (the extra entries are written) and absent on wordpress and psalm; `~/.cache/prs-profile/e4/repeat/`. |
| E5 | DONE (5ec70c1e; measured: off run 23,725,134,207 Ir against 23,785,065,626 at E3's commit, −59.9 M against the estimated ≈ −30 M, Piwigo output byte-identical; the kind (a family code 1-8, plain `mixed`, `never`) and the scalar-value bit are set in `reference_flags` and masked out of what a node passes up to its parent; `scalar_family` and `is_scalar_value` read them, so `union.rs` and the other readers got cheaper too; a unit test checks the kind against the form on the samples, NAN, class-kind strings and nested forms, and the types crate's shortcut tests pass; clippy clean on the types crate, the workspace tests and fixtures not run) | A kind code in the type node's free flag bits. | ≈ −30 M | — |
| E6 | DONE (f2f65cbb; measured: off run 23,701,951,690 Ir against 23,725,134,207 at E5's commit, −23.2 M against the estimated ≈ −10 M net, Piwigo output byte-identical; the test is `Arc::ptr_eq` on the two nodes, first in `is_super_type_of`, and answers Yes; a subtracted `mixed~S` against itself, which the form-only answer gave Maybe, is now Yes, so the two existing tests that compared with the form-only answer expect Yes for equal types, and `a_type_is_a_super_type_of_itself` is new; the types crate's 275 lib tests pass; the workspace tests, clippy and fixtures not run) | One node against itself answered first in `is_super_type_of`. | ≈ −10 M net | — |
| E7 | DONE (60e85393; measured: off run 23,672,079,898 Ir against 23,701,951,690 at E6's commit, −29.9 M against the estimated ≈ −55 M, Piwigo output byte-identical; item 4's index is not built, so the class's type is kept in a `OnceLock` inside `ClassReflection` (left out of its comparison, copy and stored form, and checked against the declared name on each read); a class the registry does not know is built each time as before; `Throwable` is a static and the fixed class lists of `instance_of_any` come from a per-thread table keyed by the literal, which needed the callers' class names to be `'static`; `int_literal` answers −1..=255 from a table; unit tests for the table and for the class's kept type, the types crate's integer-literal test and the three registry tests pass; the workspace tests, clippy and fixtures not run) | Plain object types kept with their class, two statics, a table of small integers. | ≈ −55 M | item 4's slot; item 16 overlaps |
| E8 | OPEN | A cache file's types stored once, in a table its records index. | ≈ −65 M; ≈ −25 M after item 6 | item 6, steps 3 and 5 |
| E9 | SKIP (folded into the plan's F6f, `PROFILE_DRILL_PIWIGO_20G.md:1705`) | The regex scanner's concatenation. | — | — |
| E10 | DONE (e081dbe9; measured: off run 23,619,596,541 Ir against 23,672,079,898 at E7's commit, −52.5 M against the estimated ≈ −35 M, Piwigo output byte-identical; `report` and `report_builder` return at once while the body's own flag or an enclosing body's is set (`enclosing_dynamic_bodies` counts the saved flags that are true), and `check_statement_var_tag_names` returns before formatting; the other pushes to `diagnostics` stay and are truncated as before; a debug build reports as before and records each report made while dropping, prunes the record at every truncation and rewind, and asserts at `into_diagnostics` that none is left, which fails when one prune is removed; seven new tests (extract, eval, a function declared in such a body before and after the call, a body after one, a script, a method before a top-level `extract`) and the six `extract` fixtures pass; the workspace tests and clippy not run) | Nothing reported while an enclosing body has called `extract()` or `eval()`. | ≈ −35 M on Piwigo | — (debug assertion that the truncation drops exactly what the count skipped) |
| E11 | OPEN | A cached file's origins decoded as the one `Arc<Path>` its file binds. | ≈ −116 M, per-file cache warm | E8's decode context |
| E12 | SKIP (not kept: built, measured and reverted, nothing committed; the patch is `~/.cache/prs-profile/e12/e12.patch`. Warm per-file cache, Piwigo, the binary at E10 against the same with E12: total 14,039,357,749 Ir against 14,039,334,519 (−23 K), output byte-identical in cold and warm. The main thread's `insert_built_decl` fell from 471.9 M to 52.1 M, but the 420 M of method decoding moved to the worker threads, because later passes read those methods anyway, so the report's estimate of ≈ −0.35-0.4 G assumed methods that nothing reads later. Wall time, `hyperfine -N`, 8 runs after 2 warmups, three alternating rounds, `nice -n 19`, loadavg 2.6-3.5 throughout: mean 1.806 s (rounds 1.840, 1.745, 1.834) against 1.796 s (1.801, 1.750, 1.838), no difference. Nothing is saved in instructions or in wall time, so by the rule that a saving is claimed only from a measurement the change is not kept) | Each class's assert-tagged method names carried in its `BuiltDecl`. | ≈ −0.35-0.4 G serial, per-file cache warm | — |
| E13 | DONE (c2cd1586; measured: cold run with the per-file cache on, 26,149,980,863 Ir against 26,174,556,141, −24.6 M, same 11,459 entries in 256 shards, output identical; the mkdir and getpid syscalls are not counted by Ir and were not measured) | The per-file cache's shard directories made once, by D5's check; the process id read once. | ≈ −80 ms kernel CPU, ≈ −10 ms wall | D5's check |

### A6. Left unmeasured or untraced (§12, §10.8, §10.11)

| # | status | what | how it would be settled |
|---|---|---|---|
| U1 | OPEN | The parser against the whole corpus: every file PHP accepts and the parser refuses. E14 came from two warnings only. | One `phpstan-rs check` over the 111,836 files under `vendor/`, against D6's `php -l` verdicts (`~/.cache/prs-probe/d6/`). A run of the binary: needs the user's go-ahead. |
| U2 | OPEN | Parser gaps already known (D6): `$s{0}` (valid in PHP 7; the postfix loop has no arm for `{`, `syntax/src/parser/postfix.rs:16-241`); `(real)` and `(unset)` accepted (`syntax/src/ast.rs:1303`, `:1311`) though PHP 8 refuses them. | Fix with the parser, with tests; decide per gap whether the parser follows the configured PHP version (D6). |
| U3 | OPEN | Which of C7's 238 compile-time errors the parser does not report the checker already reports. | Count by running each of the 299 trigger files (`~/.cache/prs-probe/c7/cases/`) through `check`; the verdicts are in `c7/verdicts.tsv`. |
| U4 | OPEN | E1's deterministic reproduction. | Designed in §10.14, not run; becomes E1's regression test. |
| U5 | OPEN | E10: which facts that outlive an `extract()` body (property assignments, class-level sites) a dropped body still needs. | Trace in `function_decl.rs` and `checker.rs` before widening E10 past the report calls. |
| U6 | OPEN | Which truncation holds the 33 `offsetAccess.mightNotExist` reports that `finalize` does not drop (B3 counts them among the discarded reports but not by site). Decides whether P11a's truncation saving is "well under 1 M" or ≈13 M. | Record the truncation site in the B3 probe (`~/.cache/prs-profile/off-ledger-b/probe.patch`); needs a probe build. |
| U7 | OPEN | Readers left out of d: the uninitialized-property rules on, or a tree with test classes whose built-in parent-call hook rules fire, add the ancestors' bodies to d. | Re-run B1's probe on such a tree; needs a probe build. |
| U8 | OPEN | B9's embed and edit timings were read off the console and not kept; the build script's cost under C8's form (every stub file parsed, no base) is not measured; nothing ran on aarch64 or macOS (B10's backstop is designed, not run). | Only if P6 is revived (D3). |
| U9 | OPEN | A third pass on the report's claims the review workflow could not check (102 items). | Hand check against the code and the profiles, as the 104 confirmed and the 17 split findings were. |

## Appendix B. The 20 G report, re-evaluated

The report (`PROFILE_DRILL_PIWIGO_20G.md`) describes `5fb858c9` and plans twenty items. Its status is stale in three ways: `master` built items 0-9 and 10.1-10.2; the batch changed costs the report priced in the items still open; and the report's own §2 list of unbuilt 25 G designs (the earlier list of tasks) is now spread across those items. Here are all three.

### B1. Part I's findings

| finding | the report | now |
|---|---|---|
| R1, context trees dropped on the main thread (575.5 M serial) | item 3 | DONE on `master` (`20G-3.1`, `3.2`). On this line the drop is still serial: 267.8 M for 11,459 trees after P8b (was 575.5 M) |
| R2, five functions per token (1.77 G) | item 10 | DONE on `master` (`20G-10.1`; −2.6 G with the rest of the lexer and parser group). On this line the six plumbing functions cost 2,868 M (`next_token` 838, `next_tok` 582, `fill` 567, `next_raw` 490, `skip` 231, `take_token` 160), about 78% of it in the project's parse; P8b's skim still pays the plumbing for every skimmed token |
| R3, pass results written into shared declarations; string keys (≈0.5 G); `Members::eq` 96.9 M | item 4 | DONE on `master` (`20G-4.6`-`4.8`: results are tables by id). On this line P9 removed the eager purity pass: `Members::eq` 4.8 M |
| R4, a line index for every file (270 M) | item 11 | on `master` lazy since `b188d671`; 252.7 M of builds remain (`20G-11.x`) |
| R5, stub members decoded to learn they have no deferred types (178 M) | items 6 and 4 | the eager pass is gone (`20G-6.1`), but the warm profile still decodes 59,434 stub methods in `resolved_deferred_class` (303.4 M): `20G-E2.8` |
| R6, lookups by `&str` (701 M of folding) | item 15 | 922 M by `str` now: `20G-15.3`, `E2.11` |
| R7, products for every body (`BodySummary` 344 M, `InitFlow` 218.5 M, `GlobalFacts` 218.5 M) | item 13 | larger on `master` (§3B item 13: 8.5 G of per-file products, effect summaries included). On this line P8a/P8b made them lazy: `BodySummary::of` 83.6 M |
| R8, stack guards in hot paths | item 9 | DONE on `master` (`peeled` a loop, `20G-9.1`); on this line `peeled` is 61 M (2.5 M calls) |
| R12, identifier names (`name_and_bits`, keywords) | item 12 | open: 547 M and 127 M on `master` |
| M1-M11, memory | items 3, 4, 9, 14, 16, 19 | the plan's end state was ≈365 MB at the start of the check; the measured peak is 1.41 GB (MEAS-1) |

### B2. Part II's items

| item | the plan | on `master` | on this line |
|---|---|---|---|
| 0, baselines | dumps and canonical printers; debug oracles retired | DONE (`0.11` skipped; the catalog rows deferred). Blocks nothing open | not built |
| 1, one session | one identity, one configuration, one load, one analysis | DONE except `1.1` (resolved-path identity, deferred) | not built |
| 2, the walk | parallel walk, path data from it | DONE (`2.1`-`2.7`) | not built; `collect_php_files` 75.3 M, `sort_paths` 112.3 M, `owner` 69.3 M, `absolute_path` 54.1 M, `id_for` 33.7 M remain |
| 3, trees dropped on the worker | `keeps_tree`, worker drop | DONE | not built; 267.8 M serial |
| 4, the registry by ids | ids, generations, tables by id, parallel passes | DONE (`4.1`-`4.8`); the eager purity pass was replaced by effect resolution | not built |
| 5, the merge in parallel | prefix-sum ids, sharded merge, posting lists, the cache format | DONE (`5.1`-`5.9`) | not built; `insert_numbered_file` 414 M serial |
| 6, the stub scope | deferred types on demand; availability guards; names once per file; `read_at` records | `6.1` and `6.6` DONE; the format work done under `5.8`; guards `6.2`-`6.4` deferred as semantic | not built |
| 7, one relation memo | content generation, one memo | DONE | E5 and E6 are short-cuts in front of the old memo |
| 8, lean declarations | case references move to the checker | DONE (`8.1`-`8.4`) | not built |
| 9, trees in an arena | `bumpalo` arena, handles | DONE as `la-arena` stores and ids in the syntax crate (the copy of the report on this branch says `bumpalo` handles; `bumpalo` is used only for the checker's synthetic expressions); `9.3`-`9.8` unvalidated; memory not measured (the expression arena is 78.5 MB at peak) | not built |
| 10, flat tokens | `TokenStream` | `10.1`, `10.2` DONE; `10.3`, `10.5`, `10.9` open | not built |
| 11, line starts | `LineStarts` and `LineIndex` | open; premise changed (lazy index) | not built |
| 12, lexer details | nine lexer cuts | open; `12.6`'s cost fell tenfold | not built |
| 13, a file's products | declaration list, one walk | open; the cost is 8.5 G, not the ≈0.45 G of walks the plan priced; the batch excludes fusing the `Summarizer` | P8 made context bodies lazy (INT-3) |
| 14, PHPDoc | `TagEffect`, eager lowering, two memos | open; the batch built the SQL and regex per-text analyses and the type-text buffer | P7 (tag descriptions lexed when read) |
| 15, names as names | `String` keys to `Name`, `FoldedName`, `ArrayKey` | open; the batch moved class names, callee names and statement variable names to `Name` | not built |
| 16, interned values | one allocation per name and type | open | E7 keeps a class's object type |
| 17, warm runs | stat index, a resolved context base | open; a warm run is dominated by effect classification (21.5 G of 39.37 G), which `17.5` does not say whether its base would hold; `20G-E2.6` is the other route to the same saving; decide the two together | not built |
| 18, the check phase | observers, slots, radix scopes, rule temporaries | open; the API walker, SQL and regex premises are mostly cut | E2, E3, E10 cut small parts |
| 19, each tree until its last reader | per-check release, tree cache | open; `keeps_tree` exists | not built |
| E, effect resolution | not in the copy of the report on this branch | in progress (§3A) | none |

### B3. The 25 G report's unbuilt designs (the report's §2 lists)

| design | now |
|---|---|
| C3 steps 6-7: names as `Name` keys (≈0.6 G) | `20G-15.1`-`15.3` |
| F4: purity and property-write keys as folded strings (≈0.5 G) | gone: the keys are ids (`20G-4.6`, `4.7`) and the purity pass is replaced by effects |
| F3: results inside shared declarations; a serial `changed_since` | DONE on `master` (`20G-4.6`-`4.8`) |
| §14: trees dropped in the serial insert loop | DONE on `master` (`20G-3.x`) |
| P7 and P11 step 1: PHPDoc lowering and tag effects | `20G-14.x`; this line's P7 is a different cut of the same work |
| P6(d): the streaming lexer's plumbing | DONE on `master` (`20G-10.1`) |
| P4: column tables for every file | `20G-11.x` |
| F1 and F2: the stubs' availability guards and their count | `6.1` DONE; guards `6.2`-`6.4` deferred as semantic |
| C1 step 5, C5, §12.10 | `20G-18.1`, `18.3` |
| C2 steps 2-3, C4 step 5's keys | `20G-18.6`, `15.2` |
| F5: a `FileId` bitset, string frontier sets | DONE on `master` (`20G-4.8`, `5.6`) |
| P12 steps 3-4: declaration list, the collector goes | `20G-13.1`, `13.5` |
| §12.11: `MemberIndex` | `20G-18.7` |
| the sorted traversal's k-way merge | DONE on `master` (`20G-2.5`) |

## Appendix C. How this ledger was made

- **Read:** this branch's two files whole; from `master`, by `git show master:<file>`, the 20 G ledger (all 1,141 rows), the HEAD ledger, the 9071 batch plan's measured sections and exclusions, `WORK_STACK.md`. `master`'s copy of the 20 G report was compared by size and header only.
- **Profiles:** `~/.cache/prs-profile/p8b/cg` (this line at `791d29cb`, off mode, 20,928,053,254 Ir), `cl-97b8f3f0-off` (49,358,114,819 Ir) and `cl-97b8f3f0-warm` (39,370,007,169 Ir), each indexed with `drill-scripts/parse.py` and read with `q.py` and `roots.py`. Figures are inclusive, summed over threads. An inclusive figure for a recursive function (the parser, the visitors) counts nested calls more than once; such figures are quoted only for entry points or marked.
- **Code checks, `git grep` at `master`:** absent, so the row is not built: `LineStarts`, `enum Lines`, `TagEffect`, `quick_cache`, `siphasher`, `triomphe`, `ThinArc`, `blake3`, `VarSlot`, `IntMap`, a stat index, a context base, a declaration list, `ImportScope`. Present, so the premise stands: `class_memo` and `method_memo` in the checker, `TableKey for str`, `Vec<Weak<Node>>` in the type interner, `eq_ignore_ascii_case` in `Keyword::try_from_bytes`, `to_ascii_lowercase` in `NativeType::from_name`, the eager `tokenize` in the tag lexer. Present, so the row is built: `keeps_tree`, `ast_of`, `read_at`, `la-arena` `NodeArenas`, `DirTable`, a `HashTable` in `FileTable`, `FileDecls`, `PHPSTAN_RS_NO_RELATION_MEMO`, a `TokenStream`. Absent, so the row is built or moot: `KEEP_DECISIONS`, `structural_memo`, `collect_php_files_excluding`, and any `bumpalo` in the syntax crate.
- **Merge facts:** `git merge-tree --write-tree --name-only master callgrind-20g` for the whole merge, and `--merge-base=<commit>^ master <commit>` for each row; the per-row count of `master` commits on the same files is `git log d520c9fd..master -- <file>`. Nothing was merged or checked out.
- **Not verified:** the child rows of each `master` row marked DONE; the O2 plan beyond the commits named in `HEAD-O2`; `master2` and `master3` beyond the shared history and the catalog work; whether the premises of `OFF-E3`, `E5`-`E7` and `E10` hold on `master` in instructions (they are absent there, not measured); whether `master` has a check that fails on `OFF-E16`'s table-order writes.
- **Stale on arrival:** `master` moves. The newest commits on `master` (2026-10-13) are `HEAD-O2`'s. Re-run the checks above before building a row.
