# Master reimplementation ledger

Tracks recreating, by hand on `master2`, the 87 commits `master` has that
this branch lacks (`git log 5d86aac1..master`, merge base `5d86aac1`).
Neither a rebase nor a cherry-pick: each commit's change is written again
against `master2`'s code, one commit per row, with `master`'s subject line.
A cherry-pick trial of the first 30 applied 28 commits as text and stopped
at row 30 with conflicts; it compiled nothing, so "applied as text" is not
"works". It was discarded.

Status: OPEN, DONE (hash here), SKIP (reason).

## What master2 changed that master's commits meet

- **[S] Serde and `PartialEq`.** `Registry`, `ClassReflection`, `FunctionReflection` and
  `Type` are stored in the stub cache and compared by `changed_since` (`registry/stub_cache.rs`,
  `packed.rs`, `StoredRegistry`). A field or type master adds to anything
  stored needs `PartialEq` and `serde` derives, or `#[serde(skip)]` on purpose;
  `registry/tests/stub_cache_round_trip.rs` compares every entry.
- **[R] Scoped, incremental finishing.** The passes after indexing
  (`resolve_*`, `precompute_*`, `infer_global_variables`, purity, property
  writes) run over a `Scope`; declarations are kept as built (`raw`) beside
  the resolved ones; `changed_since` compares resolved forms to seed
  rechecks. A new registry pass or field must run in scope, reset in
  `begin/end_declaration_changes`, survive `freeze`, and be compared in
  `changed_since` if a verdict depends on it.
- **[P] `project.rs`.** `update_file` no longer runs the passes inline;
  `finish_loading`, `sync_phpstan_stubs`, `switch_base`, the lazy use-site
  index and the thread-pool settings changed. 970 lines of diff.
- **[D] Dependencies.** `deps` moved to the `reflection` module;
  `dependents` is split from `signature_dependents`
  (`declaration_references` vs all references).
- **[C] Checker hunks.** `checker.rs` uses `inheritors` instead of scanning
  every class; `function_decl.rs` compares a slot's provenance file and
  sorts only a trait's own diagnostics.
- **[T] Types.** `Type` serializes through `Repr` and is re-interned on
  read; many types and `native.rs` gained serde derives.
- **[G] Gates.** `gates/module-graph.txt` lists every module; a new module
  needs a line. `deps` moved there. Two `allow ... project` edges were
  added.
- **[F] Snapshots.** `gates/approved/build-cost/types-fixtures.diff` is the
  type output of the fixtures; new or changed fixtures change it. Fixtures
  added by master are not run by `cargo test --workspace`; use
  `--test fixtures`.
- **[B] Build.** `build.rs` hashes every crate source, the lockfile and the
  stubs for the cache key: no edit needed, but a generated table is part of
  it.

Master's `Cargo.toml` changes are only entries in the workspace `exclude`
list (`tools/php-sprintf-pieces`, `tools/function-metadata`,
`tools/builtin-throws`); `master2` rewrote the profile section of the same
file, so edit by hand.

Files `master2` changed that master's commits also touch, by number of
commits: `checker.rs` 21, `registry.rs` 11, `project.rs` 8,
`types/src/lib.rs` 7, `checker/function_decl.rs` 7, `registry/class_info.rs`
4, `Cargo.toml` 4, `registry/purity.rs` 3, `registry/function_sig.rs` 3,
`registry/hierarchy.rs` 2, `lsp.rs` 2, and one each of `trinary.rs`,
`native.rs`, `syntax/src/ast.rs`, `property_writes.rs`, `registry/index.rs`,
`checker/tests/helpers.rs`.

## Order and checkpoints

Recreate in master's order; several rows depend on earlier ones (30 -> 38
-> 39 -> 42 -> 51; 34 -> 76; 80 -> 85). Per row: build, clippy, the touched
module's tests, then commit. Whole lib suite, fixtures, runtime oracle,
the stored-registry round trip and the snapshots once the last behavior
row (84) is in; regenerate snapshots before comparing them.

## Commits
| # | master | subject | src files | fixture files | overlap with master2 | adaptation | status |
|---|---|---|---|---|---|---|---|
| 1 | `62d57492` | build: release, gate and dhat builds are incremental | 1 | 0 | Cargo.toml | Already true here: `release`, `gate` and `dhat` are incremental (master2 `ab46b16e`, `d6372ce1`). Nothing to recreate. The trial pick conflicted only because of that. | SKIP (already true) |
| 2 | `ecbdd94b` | docs: a ledger tracking the A/B plan's tasks, step by step, with their commits | 1 | 0 | - | Creates `AB_LEDGER.md`; copy its text, then re-point every hash to the recreated commits as they land. | OPEN |
| 3 | `755bc257` | test: the runtime oracle records a fatal error, counts one on an expected line as executed, checks runtime_errors identifiers against what PHP raised, and runs unpinned fixtures on every present php image with PHPSTAN_RS_ORACLE_ALL_VERSIONS=1 | 3 | 0 | - | Test-only (`runtime_oracle.rs`, `shim.php`, `gate.rs`); no overlap with master2. `gate.rs` is the one file to check against master2's gate changes, if any. | OPEN |
| 4 | `6192e0da` | docs: the A/B ledger ticks the oracle step | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 5 | `be2d1a7b` | feat: a string is numeric per PHP version | 19 | 0 | checker.rs, project.rs, types/src/lib.rs | [C][T] `checker.rs`, `project.rs` and `types/src/lib.rs` hunks sit beside master2's edits (`inheritors`, serde on `Type`). `NumericOn` and any new type reachable from a stored `Type` must derive serde ([S]). | OPEN |
| 6 | `35e547a5` | feat: one version source for numeric strings and comparisons | 25 | 0 | types/src/trinary.rs | [T] Touches `trinary.rs`, where master2 added serde derives; reapply the memoization on top of them. | OPEN |
| 7 | `6ad02470` | feat: a PHP_VERSION_ID comparison narrows the versions its branch runs on, and arithmetic on a string literal reports what each configured PHP version emits | 10 | 8 | checker.rs, types/src/lib.rs | [C][T] `checker.rs` and `types/lib.rs` hunks; a PHP-version narrowing fact stored on a type needs serde ([S]). | OPEN |
| 8 | `9f1cd22f` | docs: the A/B ledger ticks the version-aware numeric strings step | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 9 | `732178e6` | feat: a string failing is_numeric is a non-numeric-string | 11 | 7 | types/src/lib.rs | [T][S][F] New `non-numeric-string` refinement in `types/lib.rs`: serde on the refinement, and a snapshot regeneration for its new fixtures. | OPEN |
| 10 | `b25606d4` | docs: the A/B ledger ticks the failed is_numeric step | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 11 | `224ce75b` | feat: ucwords folds and keeps refinements as the other case functions do (php_charmask ranges, literal delimiters one operation each, named arguments), and before PHP 8.2 the byte case functions follow the C locale | 4 | 11 | - | No overlap. [F] | OPEN |
| 12 | `f04bcc40` | feat: sprintf and vsprintf typed as PHP writes | 14 | 5 | Cargo.toml, checker.rs, types/src/lib.rs | [C][T] Adds `tools/php-sprintf-pieces` to the workspace `exclude` in `Cargo.toml` (hand-edit; master2 changed other parts of the file), and `tests/data/sprintf-pieces-*.json`. `checker.rs` and `types/lib.rs` hunks. | OPEN |
| 13 | `c0f2a6ae` | docs: the A/B ledger ticks the ucwords and sprintf steps | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 14 | `8198aa99` | feat: preg groups typed per group | 4 | 5 | checker.rs | [C] `checker.rs` hunk only. | OPEN |
| 15 | `ab876d3a` | docs: the A/B ledger ticks the preg step | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 16 | `fc5e0c00` | feat: each file knows whether it runs with strict_types=1 | 6 | 5 | checker.rs, lsp.rs | [C] `checker.rs` and `lsp.rs`: `strict_types` per file. master2 changed how the LSP workspace loads and indexes files (lazy use-site index), so check where a file's flag is set on each load path (`Project::update_file`, `finish_loading`, the LSP's own load). | OPEN |
| 17 | `af54bab9` | feat: a string parameter keeps what coerces to a string | 9 | 9 | - | No overlap; 18 files, mostly fixtures [F]. Reads registry data, so compile it against master2's resolution scope. | OPEN |
| 18 | `61a77115` | docs: the review ticks DF-1, DF-2, DF-3, DF-4, DF-6 and DF-7 with their commits, and the A/B ledger ticks the DF-7 step | 2 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 19 | `2df43bf8` | feat: the comparison model knows enums and classes with their own comparison | 10 | 0 | - | Types crate only (`comparison.rs`, data JSON, `tools/php-comparison-matrix`); no overlap. The 44k-line diff is the matrix data. | OPEN |
| 20 | `06b86d82` | fix: an object that may read as false casts to bool | 7 | 2 | - | No overlap. [F] | OPEN |
| 21 | `a46abd09` | feat: loose comparisons report what PHP converts | 8 | 23 | checker.rs | [C] `checker.rs` hunk; 31 files, mostly fixtures. [F] | OPEN |
| 22 | `61af4e82` | docs: the review ticks CMP-1 with its commits, and the A/B ledger ticks the CMP-1 step | 2 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 23 | `65ded9cb` | feat: overrides are checked as PHP links classes | 8 | 66 | registry.rs, registry/hierarchy.rs, types/src/lib.rs, types/src/native.rs | [R][T][C] `registry.rs`, `registry/hierarchy.rs`, `types/lib.rs`, `types/native.rs`. Prototype lookup walks the hierarchy; master2 replaced scans of every class with `inheritors`, `subtype_children`, `trait_users` and scoped member-owner tables, so the walk must read those. `native.rs` gained serde derives in master2: the native variance relation's types need the same if they are stored. 74 files, most fixtures [F]. (Trial pick applied cleanly as text.) | OPEN |
| 24 | `04ef1ac9` | docs: the review ticks M1 with its commit, and the A/B ledger ticks the M1 step | 2 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 25 | `b8a19081` | feat: a write or unset at a chain of offsets goes per key and per member (Relation::write_offset_path) | 5 | 0 | types/src/lib.rs | [T] `types/lib.rs` hunk (`Relation::write_offsets`); no stored type. | OPEN |
| 26 | `7e0177cb` | feat: assignments and unsets at offsets go through the path write | 5 | 11 | registry/class_info.rs | [R][S] `registry/class_info.rs`: master2 added `PartialEq` and serde to the reflection structs there (`changed_since` compares resolved forms). Whatever this hunk adds to `ClassReflection` needs both, and the stored registry's raw-vs-resolved diff must still round-trip. | OPEN |
| 27 | `6506dac1` | docs: the review ticks W1 with its commits, and the A/B ledger ticks the W1 step | 2 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 28 | `4f9c76b4` | feat: an assignment's value narrows as of before it | 4 | 3 | checker.rs | [C] `checker.rs` hunk only. | OPEN |
| 29 | `7e0904d0` | docs: the review ticks DF-5 with its commit, and the A/B ledger ticks the DF-5 step | 2 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 30 | `af4a50f3` | feat: the infrastructure for checking one body alone | 7 | 0 | checker.rs, project.rs, registry.rs, registry/hierarchy.rs | [R][P][C] CONFLICTED in the trial pick. `project.rs`: master wraps a file in `PendingFileBodies` so `resolve_transitive_purity`, `precompute_ancestor_names`, `infer_global_variables` see a file not yet in the table; master2 removed those passes from `update_file` and runs scoped, incremental finishing (`finish_loading`, `Scope`, `changed_since`), so the pending-file view belongs wherever master2 now resolves purity and walks bodies. `registry/hierarchy.rs`: master adds `descendants_by_class` built by `collect_descendants`; master2 already keeps `subtype_children` and `inheritors`, so `Registry::descendants` should read those instead of a second table, and must follow `precompute_member_owners_after_mixins`'s scoped rebuild. Also `BodySource::declaration`/`DeclSite`, `FileContext`, `summaries.rs`. | OPEN |
| 31 | `ba5ede69` | docs: the A/B ledger ticks the summary infrastructure step | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 32 | `23587a8f` | feat: a closure or arrow function carries its doc comment | 5 | 0 | syntax/src/ast.rs | `syntax/src/ast.rs` and the parser; master2's `ast.rs` change is serde derives on `Span` and one position type only, so the closure node's new doc-comment field has no serde obligation unless that node is stored. master2's lexer position counting (`lexer.rs`) feeds the same spans: compile and run the parser tests. | OPEN |
| 33 | `0da96f54` | feat: a closure's docblock types it | 13 | 2 | checker.rs, checker/function_decl.rs, registry.rs, registry/class_info.rs, registry/function_sig.rs | [R][S][C] `registry.rs`, `class_info.rs`, `function_sig.rs`, `checker/function_decl.rs`, `checker.rs`. Closure docblock signatures add fields to the reflection structs (they need `PartialEq` and serde: master2 derives both). `function_decl.rs`: master2 changed the slot-site check (now compares the slot's provenance file) and diagnostic sort/dedup of a trait's own output. | OPEN |
| 34 | `0309b278` | feat: callable values know what they run | 21 | 6 | checker.rs, project.rs, registry/function_sig.rs, types/src/lib.rs | [R][S][T][P][C] `CallableSig` carries origins: it lives in `Type`, which master2 now serializes through `Repr`; `origins` must serialize (or be skipped on purpose), and the stored registry's interned types must come back equal. Touches `types/lib.rs`, `function_sig.rs`, `project.rs`, `checker.rs`. 27 files. | OPEN |
| 35 | `8448fc07` | docs: the review ticks C2 with its commits, and the A/B ledger ticks the C2 step | 2 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 36 | `b36cb61e` | feat: one throw-point model | 14 | 9 | checker.rs, checker/function_decl.rs | [C] `checker.rs`, `checker/function_decl.rs`. Engine-throw model; check against master2's `function_decl.rs` hunks above. | OPEN |
| 37 | `06e23d2d` | docs: the A/B ledger ticks the throw-point step | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 38 | `f816c917` | feat: a checked body yields its summary | 17 | 0 | Cargo.toml, checker.rs, checker/function_decl.rs, registry.rs | [R][B][G][C] `registry/function_metadata.rs` and its generated `table.rs` are new modules: add them to `gates/module-graph.txt`, and to the workspace `exclude` for `tools/function-metadata` in `Cargo.toml` (hand-edit). `build.rs` hashes every crate source, so no change there. `summaries.rs`, `registry.rs`, `checker.rs`, `function_decl.rs`. | OPEN |
| 39 | `8df89884` | feat: verdicts settled in rounds | 13 | 0 | checker.rs, checker/tests/helpers.rs, lsp.rs, project.rs, registry.rs, registry/property_writes.rs, registry/purity.rs | [R][P][D] Biggest conflict risk. Replaces `resolve_property_writes` and `resolve_transitive_purity` (verdicts settled in rounds, `consulted_verdicts`, `SummaryMemo`) in `registry/property_writes.rs`, `registry/purity.rs`, `project.rs`, `registry.rs`, `lsp.rs`, `checker/tests/helpers.rs`. master2 made both passes incremental: `property_write_classes` and `global_sites` caches, `PropertyWrites` compared in `changed_since` to seed rechecks, `declaration_references` vs all references in two scopes. The rounds must run inside that scoping, and what a verdict consults must reach `deps` / `signature_dependents`, or an edit won't recheck dependents. | OPEN |
| 40 | `a86e2394` | docs: the A/B ledger ticks the summary verdicts and lists what's left of them | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 41 | `8c6e8627` | feat: a function or method declared in several places runs whichever is loaded | 3 | 0 | registry.rs, registry/index.rs | [R][S] `registry.rs`, `registry/index.rs`: a `declarations: FxHashMap<(Option<Name>, Name), Vec<Provenance>>` field. Decide stored (serde) vs `#[serde(skip)]`; it must be reset and re-filled by `begin/end_declaration_changes` and survive `freeze`. `index.rs` is where master2 changed `BuiltDecl` insertion. | OPEN |
| 42 | `5b0b101f` | feat: closures have summaries | 8 | 0 | checker.rs, checker/function_decl.rs, registry.rs, registry/purity.rs | [R][C] `summaries.rs`, `registry.rs`, `registry/purity.rs`, `checker.rs`, `function_decl.rs`; builds on 39. | OPEN |
| 43 | `75e24584` | feat: property hooks are calls | 5 | 0 | checker.rs | [C] `checker.rs` hunk. | OPEN |
| 44 | `0ec7ebbc` | feat: a generator runs when it's iterated | 8 | 0 | checker.rs | [C] `checker.rs` hunk. | OPEN |
| 45 | `6fb28da8` | docs: the A/B ledger ticks the rest of the summaries and lists what's left | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 46 | `29345cf4` | feat: a closure bound to another class has a summary of its own | 1 | 0 | - | Single file (`summaries.rs`); no overlap, but depends on 30/38/39. | OPEN |
| 47 | `fe8e3a38` | docs: the A/B ledger ticks rebound closures | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 48 | `e2a83c79` | feat: a call forgets what it may write | 12 | 0 | checker.rs, checker/function_decl.rs | [C] `checker.rs`, `function_decl.rs`; 12 files. | OPEN |
| 49 | `85e0fe43` | feat: what a call can't reach is kept | 11 | 14 | checker.rs, registry/class_info.rs | [R][S][C] `class_info.rs` (a per-class fact about what a constructor keeps: `PartialEq` and serde, as master2's derives on that file) and `checker.rs`. 25 files, mostly fixtures [F]. | OPEN |
| 50 | `8f4bb7f2` | docs: the A/B ledger ticks forgetting | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 51 | `7c6f6aab` | feat: typed purity | 24 | 41 | checker.rs, checker/function_decl.rs, project.rs, registry.rs, registry/class_info.rs, registry/function_sig.rs, registry/purity.rs | [R][S][P][C][D] Second-biggest. Typed purity: removes `resolve_transitive_purity` for good and adds `declared_purity` to the reflection structs (`PartialEq`, serde, tag synthesis). master2 added `has_purity` and `clear_purity` to `FunctionReflection` for its scoped re-resolution, over `purity_dependencies` and `is_transitively_pure`; if those fields go, so do those two methods and their callers. Changes `project.rs`, `registry.rs`, `registry/purity.rs`, `function_sig.rs`, `class_info.rs`, `rules_settings.rs`, `xtask/src/main.rs`, `tests/cli.rs`. 65 files. Reapply on top of the recreated 39. | OPEN |
| 52 | `a8166450` | docs: the A/B ledger ticks typed purity | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 53 | `12302460` | feat: a class the project mocks may run anything | 5 | 1 | project.rs, registry.rs | [R][S][P] `registry/mocks.rs` is a new module (module-graph entry [G]); `mocked_classes: FxHashSet<Name>` on `Registry` (stored or skipped, and reset on a re-finish), and the mock scan in `project.rs` must run on master2's load/update paths. | OPEN |
| 54 | `66f29e12` | docs: the A/B ledger ticks mock openness | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 55 | `c4592dbf` | feat: throws reporting over what bodies throw | 15 | 54 | - | [F][G] No overlap in source, but 69 files: 15 source files in `checker/throws*`, `ext/extras.rs`, `rules_settings.rs`, `xtask`; the rest fixtures. Appends to `gates/test-renames/stage-b-fixes.txt`. Verify the renames still apply to master2's fixture set (it added two). | OPEN |
| 56 | `93f895dc` | docs: the roadmap folds computed throws and closed-world dispatch into Done, adds unreachable-catch and rethrow entries, and points its cross-references at the shipped descendants index and call summaries | 1 | 0 | - | docs: `ROADMAP.md` rewrite; master2 didn't change that file, so it applies as text, but check it against what master2's own commits changed in the code it describes. | OPEN |
| 57 | `3ed82e20` | docs: the A/B ledger ticks throws reporting | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 58 | `8e75e006` | feat: a builtin that can't throw throws nothing | 9 | 3 | Cargo.toml, registry.rs | [R][B][G] `registry/builtin_throws.rs` and a generated `table.rs` (module-graph entry), `tools/builtin-throws` (Python generator, README), `Cargo.toml` `exclude` (hand-edit). Do not commit the `__pycache__/generate.cpython-314.pyc` master committed. `registry.rs` hunk. | OPEN |
| 59 | `8789d8e6` | docs: the A/B ledger ticks the builtin closed-throw table | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
| 60 | `45b4d1b9` | test: summary mode records what a full check records | 4 | 0 | project.rs | [P] `project.rs`, `project/tests/summaries.rs`. The test builds a project and compares summary mode with a full check; master2 moved the test-project helpers (`checker/tests/helpers.rs`, `project/tests/*`), and a store knob for checking fully must go through the new finish path. master2's every-fixture differential test runs by default now. | OPEN |
| 61 | `42b41685` | fix: a write at a key a shape doesn't have opens the shape to that key alone | 4 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 62 | `c3886590` | fix: sprintf writes what PHP writes | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 63 | `a196f315` | fix: a file is rechecked when a call it makes dispatches differently | 4 | 0 | checker.rs, project.rs | [P][D][C] `project.rs`, `checker.rs`, `summaries.rs`: records the dispatches a check made (`FileConsults`), rechecking a file when a call dispatches differently. Must join master2's dependency seeding (`signature_dependents`, `changed_since`) rather than a parallel mechanism. | OPEN |
| 64 | `dc7149f6` | fix: a nested write at a variable key starts from what that element is known to hold | 3 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 65 | `e93771cf` | fix: a format with a specifier PHP 7 doesn't know is an invalid placeholder there | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 66 | `0bb86016` | fix: a bare call's arguments block resultUnused only by assigning a local | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 67 | `ed6f2a3c` | fix: a call to something unknown is a maybe impure point where it runs | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 68 | `407de65f` | fix: a builtin's string parameter isn't a callback | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 69 | `fe48cee1` | fix: assertType() runs nothing | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 70 | `44961b4a` | fix: a return PHP can't take as the native return type throws a TypeError | 7 | 0 | checker.rs, checker/function_decl.rs | [C] `checker.rs`, `function_decl.rs`. | OPEN |
| 71 | `d09b1ddc` | fix: a builtin that only throws throws what its arguments decide | 6 | 0 | registry.rs | [R] `registry.rs`, `builtin_throws` table regenerated; `generate.py` again. | OPEN |
| 72 | `0f8a39fd` | fix: an override's messages name what it lacks | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 73 | `ee7beb91` | fix: a GMP or BcMath\Number reads as false by its value only from PHP 8.4 | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 74 | `919f1945` | fix: two shapes with the same keys merge in the earlier one's key order | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 75 | `bc827521` | fix: a PHPDoc non-numeric-string is read as one | 3 | 0 | - | No overlap; 3 files. A PHPDoc keyword read: check master2's `phpdoc` crate changes (`grammar/ast.rs`). | OPEN |
| 76 | `e48163e1` | fix: a callable type is interned with its origins | 1 | 0 | - | Types crate; interning of `CallableSig` with origins: depends on the recreated 34 and on serde of `Type` ([S]). | OPEN |
| 77 | `a32923a4` | fix: a call forgets the facts kept beside the scope too | 2 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 78 | `3f24c2f1` | fix: a closure's by-reference parameter leaves what a run of its body writes | 3 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 79 | `51818ccd` | fix: a value piped into a first-class callable is typed as the call itself | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 80 | `3a001b05` | test: fixture expectations after the A/B work, every change read | 1 | 189 | - | [F] 190 files: fixture expectations. Do not copy: regenerate after all behavior commits, then read each diff against this list. Also touches `gates/test-renames/stage-b-fixes.txt`. master2's snapshot `gates/approved/build-cost/types-fixtures.diff` needs a regeneration afterwards. | OPEN |
| 81 | `72526aa0` | fix: sprintf with no configured version writes nothing for %h on the PHP 7 side | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 82 | `e8caa325` | fix: a string check keeps true and drops false on their own | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 83 | `0c738cd6` | fix: a Closure return satisfies an overridden callable return only from PHP 8.5 | 2 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 84 | `367177e5` | fix: override checks follow each ancestor position a variadic child covers and the prototypes PHP has | 1 | 0 | - | No overlap with master2's files; apply by hand and compile. | OPEN |
| 85 | `1c9d972e` | test: fixtures follow the runtime oracle on every PHP version | 0 | 23 | - | [F] 23 fixture files pinned to PHP versions; no source. Reapply after 80. | OPEN |
| 86 | `0ad9589b` | chore: the rename log records the 22 tests Stage B renamed without an entry | 1 | 0 | - | Rename log only (`gates/test-renames/stage-b-fixes.txt`); copy the 22 entries, with the recreated commits' hashes. | OPEN |
| 87 | `15bb27d4` | docs: the A/B ledger ticks the test pass with its fix commits | 1 | 0 | - | docs only; written after the commits it ticks, with this branch's hashes. | OPEN |
