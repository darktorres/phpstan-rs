# Build-cost work: gap ledger

Tracks what `PROJECT_BUILD_COST_INVESTIGATION.md` (section 12) left skipped,
partly done or unvalidated. Rebuilt from the audit after `e5124a6e`
(2026-10-01); the earlier claim-by-claim review ledger was lost with its
scratch directory.

Status: DONE, OPEN, NOTE.

## Skipped or partly done catalog items
| id | item | gap | status |
|---|---|---|---|
| G1 | #34 | `resolve_deferred_value_of` ran sequentially. Now parallel: each class resolves against the classes as they were before the pass (a class's reference to itself still degrades to `Mixed`); results no longer depend on visit order | DONE (`862d168a`); lib, fixtures and differential tests pass |
| G2 | #19 | a body-only edit still walked every body for property writes and global variables. Property-write classifications and each file's global-variable sites are kept; only declarations and files the edit reaches are walked again | DONE (`feec6a57`); lib, fixtures and differential tests pass |
| G3 | #14, #15 | lazy per-class tables and per-pass candidate flags not implemented; "folded" into the shared base and scoping without measurement | CLOSED by measurement: a one-function project's whole finish is a few milliseconds; the passes no longer show in any profile |
| G4 | #8 | completion and `class_exists` still see classes of never-enabled extensions (behaviour predates this work) | NOT DONE ON PURPOSE: enabling only some extensions would make code using an undeclared one report `class.notFound`; loading all matches PHPStan, and unreferenced extension classes cost nothing per project (#6, #7, #11) |
| G5 | #9 | a cold stub cache still pays full stub indexing (~0.85 s gate, ~3 s debug) | NOT NEEDED: a cold cache costs one build of the stubs per source change, and the cache now loads in 60 ms |
| G6 | #21 | cost of `retain_source` + use-site indexing never measured | DONE (`54ec427b`): the workspace indexes class and function use sites at the first references or rename request |
| G7 | #23 | project drop cost never measured after #11 | CLOSED: the CLI no longer frees its project, and the shared base makes a test project's drop small |
| G8 | #24 | arena allocation not done | DONE as mimalloc (`433e07f2`); true arenas aren't possible with std containers on stable |
| G9 | section 7 | only option 2 (`with_min_len`) done; options 1 (fused passes), 4 (`ROUNDS_UNTIL_SLEEPY` experiment) and 5 (`chili`) not done | DONE by measurement: a pool-size curve showed the cost. One-file run: 44-45 ms on 1-4 threads, 54 ms on 8 (the idle-spinning section 7 described); Console load levels off at 2 threads; Piwigo fit wall = 6.6 s serial + 8.3 s / threads, so most of its time was serial, not spin. The serial stretches were then found and fixed (`6a6c2a9c`): largest files checked first, property writes classified in parallel, a scope's cached entries decoded in parallel; Piwigo 7.5 s to 5.1 s and 4 threads 9.1 s to 6.5 s. Not done: sizing the pool by workload (a one-file run pays ~9 ms for 8 threads; the file count isn't known before the pool exists) and patching rayon's sleep rounds (option 4), whose target - spinning - the data shows is small |
| G10 | #5/#11 | a project naming `PHPStan\` resolves PHPStan's ~2k classes itself; no second shared base. Swapping the base under a loaded project must reconcile overlay entries that shadow the old base, so measure the cost first | DONE (`c395b928`) |

## Correctness gaps
| id | gap | status |
|---|---|---|
| G11 | a change in inferred global variables seeded no recheck: `global $x;` is a tracked reference (`deps::global_variable_key`) and `changed_global_variables` seeds the recheck | DONE (`3d77ee38`) |
| G12 | corrupt or truncated cache file panicked mid-run: cache files carry a checksum and are rebuilt when it fails | DONE (`c4eacee4`) |
| G13 | a cancelled cascade left stale `last_diagnostics`: `Project::take_unchecked` drops them | DONE (`9ad44af6`) |
| G14 | the resolution scope closed over every reference, bodies included: now two scopes, declaration references for resolution and all references for purity, property writes and global variables | DONE (`feec6a57`); lib, fixtures and differential tests pass |

## Validation
| id | gap | status |
|---|---|---|
| G15 | regression gates not run: corpus/type/baseline snapshots (`gates/approved`), performance gate | DONE: scoped, all-on and type snapshots are identical to the merge-base except the two approved fixtures (`gates/approved/build-cost`). Gates failing before this work and still failing: test manifest (renames in master's earlier history), fuzz crate (`parse_file` takes bytes), `cargo deny` (yanked `yoke-derive`), 14 module-graph edges |
| G16 | differential test sampled only: now a shared helper with a sampled run, a proptest random-edit run, and an every-fixture run (23 s; passes, runs with the default tests) | DONE (`e35088b3`) |
| G17 | section 4 numbers, section 6 projections and section 9 questions not re-measured | DONE: measurements in section 12 of the doc |
| G24 | workspace tests and the 3,807 fixtures not rerun since G1, G2, G11-G13, G21 | DONE: lib 956, integration 44, fixtures 3,807, runtime oracle 3,807, other crates |

## Doc and comment staleness
| id | gap | status |
|---|---|---|
| G18 | doc sections 3, 5, 6 and 11 described pre-implementation behaviour | DONE (doc edited and committed) |
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
| G29 | profile hotspots S1 (checksum), S2 (entry copy), S4-S6 (stub index), E1/P8 (parser, lexer), P5 (stack guard) | DONE (`c12fb048`..`479ee2f6`); S3 (name interning), S7 (`Name::lower`), S8-S10 and the purity walks left: each under 3% of a run |
| G25 | files load in the order given, but a rebuild of a name declared in several files uses path order, so duplicate declarations can resolve differently after an edit than in a fresh load unless files load path-sorted | NOTE (found by the differential test) |
