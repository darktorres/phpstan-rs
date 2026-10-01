# Build-cost work: gap ledger

Tracks what `PROJECT_BUILD_COST_INVESTIGATION.md` (section 12) left skipped,
partly done or unvalidated. Rebuilt from the audit after `e5124a6e`
(2026-10-01); the earlier claim-by-claim review ledger was lost with its
scratch directory.

Status: DONE, OPEN, NOTE.

## Skipped or partly done catalog items
| id | item | gap | status |
|---|---|---|---|
| G1 | #34 | `resolve_deferred_value_of` ran sequentially. Now parallel: each class resolves against the classes as they were before the pass (a class's reference to itself still degrades to `Mixed`); results no longer depend on visit order | DONE (`862d168a`); lib suite passed, fixtures pending |
| G2 | #19 | a body-only edit still walked every body for property writes and global variables. Property-write classifications and each file's global-variable sites are kept; only declarations and files the edit reaches are walked again | DONE (`feec6a57`); lib suite passed, fixtures pending |
| G3 | #14, #15 | lazy per-class tables and per-pass candidate flags not implemented; "folded" into the shared base and scoping without measurement | CLOSED by measurement: a one-function project's whole finish is a few milliseconds; the passes no longer show in any profile |
| G4 | #8 | completion and `class_exists` still see classes of never-enabled extensions (behaviour predates this work) | NOT DONE ON PURPOSE: enabling only some extensions would make code using an undeclared one report `class.notFound`; loading all matches PHPStan, and unreferenced extension classes cost nothing per project (#6, #7, #11) |
| G5 | #9 | a cold stub cache still pays full stub indexing (~0.85 s gate, ~3 s debug) | NOT NEEDED: a cold cache costs one build of the stubs per source change, and the cache now loads in 60 ms |
| G6 | #21 | cost of `retain_source` + use-site indexing never measured | DONE (`54ec427b`): the workspace indexes class and function use sites at the first references or rename request |
| G7 | #23 | project drop cost never measured after #11 | CLOSED: the CLI no longer frees its project, and the shared base makes a test project's drop small |
| G8 | #24 | arena allocation not done | DONE as mimalloc (`433e07f2`); true arenas aren't possible with std containers on stable |
| G9 | section 7 | only option 2 (`with_min_len`) done; options 1 (fused passes), 4 (`ROUNDS_UNTIL_SLEEPY` experiment) and 5 (`chili`) not done | PARTLY: `PHPSTAN_RS_THREADS`; fused passes and the rayon experiments not pursued - the profiles show parallel regions are not where time goes |
| G10 | #5/#11 | a project naming `PHPStan\` resolves PHPStan's ~2k classes itself; no second shared base. Swapping the base under a loaded project must reconcile overlay entries that shadow the old base, so measure the cost first | DONE (`c395b928`) |

## Correctness gaps
| id | gap | status |
|---|---|---|
| G11 | a change in inferred global variables seeded no recheck: `global $x;` is a tracked reference (`deps::global_variable_key`) and `changed_global_variables` seeds the recheck | DONE (`3d77ee38`) |
| G12 | corrupt or truncated cache file panicked mid-run: cache files carry a checksum and are rebuilt when it fails | DONE (`c4eacee4`) |
| G13 | a cancelled cascade left stale `last_diagnostics`: `Project::take_unchecked` drops them | DONE (`9ad44af6`) |
| G14 | the resolution scope closed over every reference, bodies included: now two scopes, declaration references for resolution and all references for purity, property writes and global variables | DONE (`feec6a57`); lib suite passed, fixtures pending |

## Validation
| id | gap | status |
|---|---|---|
| G15 | regression gates not run: corpus/type/baseline snapshots (`gates/approved`), performance gate | DONE: scoped, all-on and type snapshots are identical to the merge-base except the two approved fixtures (`gates/approved/build-cost`). Gates failing before this work and still failing: test manifest (renames in master's earlier history), fuzz crate (`parse_file` takes bytes), `cargo deny` (yanked `yoke-derive`), 14 module-graph edges |
| G16 | differential test sampled only: now a shared helper with a sampled run, a proptest random-edit run, and an every-fixture run (`#[ignore]`, 226 s; passes) | DONE (`e35088b3`) |
| G17 | section 4 numbers, section 6 projections and section 9 questions not re-measured | DONE: measurements in section 12 of the doc |
| G24 | workspace tests and the 3,807 fixtures not rerun since G1, G2, G11-G13, G21 | DONE: lib 952, integration 44, fixtures 3,807, runtime oracle 3,807, other crates |

## Doc and comment staleness
| id | gap | status |
|---|---|---|
| G18 | doc sections 3, 5, 6 and 11 described pre-implementation behaviour | DONE (doc edited; the doc is not yet committed) |
| G19 | "2351 files" in `registry/stubs.rs` comments | DONE (`7106cb2b`) |

## Process notes
| id | note | status |
|---|---|---|
| G20 | workspace-wide clippy clean at the audit | DONE |
| G21 | stub cache removed other builds' files on every store, and never cleaned up otherwise: keeps the last 3 builds | DONE (`stub_cache::prune`) |
| G22 | fixtures run under the `test` profile (opt-level 1), not `gate` | NOTE |
| G23 | `Project::built_once` now only gates a `debug_assert`; `end_declaration_changes`' `keep_raw` matters only for the base build | NOTE |
| G26 | the stub cache never loaded (ciborium byte reader limit); found by timing the CLI, fixed with a round-trip test | DONE (`fb9b3137` and before) |
| G27 | the PHPDoc dependency collector read descriptions and template names as classes | DONE (deps.rs) |
| G28 | whole-program closed-class check decoded every stub class | DONE (`9c52a784`) |
| G25 | files load in the order given, but a rebuild of a name declared in several files uses path order, so duplicate declarations can resolve differently after an edit than in a fresh load unless files load path-sorted | NOTE (found by the differential test) |
