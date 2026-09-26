# Crate-split audit: Stage A and B1–B7

An item-by-item check of every completed part of `CRATE_SPLIT_PLAN.md`
against the code at `867d9cb3` (2026-09-26): §2 (layers and workspace
conventions), §3.1–§3.3 and §3.7 (designs of the extracted crates), §4
(gates), §6 (risks that apply from Stage A/B), Appendix A rows due by B7,
Appendix F decisions, Stage A (A0–A2) and B1–B7.

Status: **OK** done as planned · **GAP** planned, not done, no step
schedules it · **PARTIAL** done in part · **DEVIATION** done differently
from the plan's wording · **SCHEDULED** deferred and recorded in a later
step.

## Findings (to act on)

| # | Area | Status | Finding | Evidence | Proposed home |
|---|---|---|---|---|---|
| 1 | §2 lints | GAP | No `[workspace.lints]`; none of the five extracted crates sets `[lints] workspace = true`, though the plan activates the lints in each extraction commit. Measured impact: `unreachable_pub` 5 hits (all in `types`), `cargo doc` 0 warnings, no print macros in library crates. | root `Cargo.toml`, `crates/*/Cargo.toml` | now |
| 2 | §2 manifest | GAP | `[workspace.package]` has no `license`. | root `Cargo.toml` | now |
| 3 | §2 dependencies | GAP | Shared third-party dependencies are declared per crate (`rustc-hash` ×5, `hotpath` ×3, `serde` ×2, `sqlparser` ×2, `imbl` ×2) instead of in `[workspace.dependencies]`. Appendix E does not list `imbl` for `syntax`, which uses it (`name_resolution.rs` use map). | `crates/*/Cargo.toml` | now (and Appendix E row) |
| 4 | §2 features | GAP | No `compile_error!` makes `dhat-heap` and `hotpath-alloc` mutually exclusive. | no `compile_error` in `crates/phpstan-rs/src` | now |
| 5 | §2 LSP version | DEVIATION | The LSP reads `env!("CARGO_PKG_VERSION")`; the bin does not pass its version into the LSP entry point (`lsp::run()` takes none). Equal today (one package), diverges once `lsp` is its own crate. | `lsp.rs:2588`, `main.rs:72` | now, or F4 |
| 6 | §4 gates run | GAP | `cargo xtask gates` (either tier) was never run in Stage B; checkpoints ran only fmt, clippy, lib tests, fixtures, the scoped/all-on corpus, type snapshots and the test manifest. The level 0–10 and unscoped corpus runs last ran at Stage A (newest level snapshots are from `1277ae05`/`d132d200`-era commits, before B1). Never run locally since B1: `cargo doc`, `bench --no-run`, fuzz compile, `--no-default-features`, `cargo hack`, `machete`, `deny`. | `.corpus-runs/gate/*-L*/` hashes | now: a `--full` run as the Stage B checkpoint |
| 7 | §4.8 performance | GAP | The performance gate has never recorded a result (`.corpus-runs/gate/perf` does not exist), so there is no timing or memory baseline; §6 says the timing gate applies from B1 on. | no `perf/` under the gate dir | now |
| 8 | §4.9 build time | GAP | Never recorded (no `buildtime/`), and its scenario list is stale: it touches `crates/phpstan-rs/src/lexer.rs`, which moved to `crates/syntax` in B3 (`touch` would create an empty file). The end-of-Stage-B target assertions don't exist. | `xtask/src/main.rs` `BUILD_SCENARIOS` | now |
| 9 | §4.10 architecture | GAP | No architecture check is implemented. Rules whose phases have landed: no print macros in library crates (holds today), no `Diagnostic {` literal outside the builder (23 literals: `phpstan_api.rs` 11, `lsp/protocol.rs` 2, `project.rs`, `duplicate_classes.rs`, `duplicate_functions.rs`, and 7 in tests), no `"$$` type-string sentinels (5 dead checks: `types/src/relation/super_type.rs:285`, `generics/ancestors.rs` 374/561/588/641 — nothing produces `$$` names). | grep | now |
| 10 | §4.5 type snapshot | PARTIAL | Only Piwigo is snapshotted; the fixture corpus is not. The dump carries type and native type but no certainty (maybe-defined). | `xtask/src/main.rs` (`--dump-types` on corpus configs only); `checker.rs:1462` | now (fixtures); certainty at D1 if the holder model is what gives it |
| 11 | §4.6 baseline gate | DEVIATION | The plan says the Stage-A baseline keeps producing zero diagnostics and zero stale entries; the gate holds each commit to its parent plus approved deltas and reports non-zero counts (scoped 1/1, all-on 63/70) as information. | `xtask/src/main.rs:503` | plan text: describe what the gate does |
| 12 | §4.2 wording | DEVIATION | §4.2 says "From Stage B on, zero exceptions"; B2 and the gate keep 218 listed exceptions, each dated. The plan contradicts itself. | `gates/module-graph.txt` | plan text |
| 13 | §4.11 renames | DEVIATION | `fuzz/fuzz_targets/parse_type_string.rs → phpdoc_type.rs` shows as delete + create in `8b2fd202` (moved and rewritten in one commit). No `.git-blame-ignore-revs` exists (every other move is an `R`). | `git show -M --summary 8b2fd202` | record only |
| 14 | §6 changelog | GAP | No changelog. User-visible changes so far: total output order (A1); output formats without the location, baseline v2 and `convert-baseline` (B7); restricted-symbol path zones in the LSP (B7). | no `CHANGELOG*` | now |
| 15 | A2 conversion | PARTIAL | A2 converts every test that asserts on diagnostics; 221 Rust tests were kept (`2b331b4d`: "touch internals or need hand-written fixture config"). Many are plain table-driven diagnostic tests A2 allows as N fixtures: `checker/tests/receiver_type_checks.rs` (52 tests), `checker/phpstan_api/tests.rs` (45), `strict_rules.rs` (10), `key_value_of.rs` (5), `narrowing.rs` (5), and 1–4 each in about 20 more files. | `grep diagnostics_for` over `src/**/tests` | now: convert the diagnostic-asserting ones, keep the internals tests |
| 16 | A2 known bugs | GAP | None of the ~30 A2 known bugs (plus the B3 purity bug) is fixed, and no step schedules them. | commit log since `3bd26284` | a scheduled step (decision pending) |
| 17 | §3.1 interner | PARTIAL | The interner is sharded and project-owned, but about 115 call sites in `registry/` and `checker/` intern project class and member names through `syntax::intern::intern` (the process-wide interner), so they are never reclaimed, which §3.1's Session scoping is for. The parser has its own interner yet calls the global one at 4 sites (`parser/tokens.rs:265`, `parser/classes.rs:128,495,497`). | grep `intern(` | parser now; registry/checker with C3/C4 (snapshots own their names) |
| 18 | §3.1 annotations | GAP | "Extensions can register extra visitors" has no support and no step. | `syntax/src/annotations.rs` | E4 |
| 19 | §3.3 one grammar | PARTIAL | Doc text is still read around the grammar: `checker/unused.rs:363` finds `@phpstan-always-read`/`-written` by substring (matches the words anywhere, e.g. in a description); `project/mock_hygiene.rs:156` takes the text after the first `@var` and splits on whitespace (wrong for shapes with spaces, ignores `@phpstan-var`); `phpdoc::docblock::unknown_phpstan_tags` scans raw text instead of the parsed tags. | the three sites | now (B5 completion) |
| 20 | §3.2 hierarchy | GAP | `ClassHierarchy` has no enum-cases query (§3.2 lists it); nothing needs it yet. | `types/src/hierarchy.rs` | with the first relation that needs it (record in C8) |
| 21 | §3.2 dead code | GAP | `is_unresolved_reference`'s `name.contains("::")` test is dead too: the type parser turns `Foo::BAR` into `ClassConstant`, and nothing builds an `Object` named `A::B`. | `types/src/parse.rs:763`; no producers | now (with #9) |
| 22 | §3.7 tip helpers | GAP | "PHPStan's standard tip helpers" (discovering symbols, `treatPhpDocTypesAsCertain`, …) don't exist; no step lists them. | grep | E3 |
| 23 | §3.7 names | DEVIATION | The model is still `Diagnostic` / `DiagnosticBuilder`, not `RuleError` / `RuleErrorBuilder`. | `crates/diagnostics/src/lib.rs` | decide: rename or amend the plan |
| 24 | §3.7 no clap | GAP | `output.rs` (assigned to the `app` library crate) derives `clap::ValueEnum`. | `output.rs:42` | F3/F4 (record) |
| 25 | Appendix E / B6 | DEVIATION | Static-schema SQL resolution (SELECT/CTE/derived-table column resolution against the catalog) and parameter validation parse SQL with `sqlparser` in the package; Appendix E's target is an `sql` API. | `checker/sql_reflection/resolve.rs`, `parameter_validation.rs` | E4 (ext-sql), record |
| 26 | B5 interface | DEVIATION | TypeNode → Type resolution goes through a free-function adapter (`registry::type_node::parse_type_node`), not an interface. C7 replaces it either way. | `registry/type_node.rs` | record only |
| 27 | B7 approved delta | DEVIATION | The location removal was verified by hand against 6c23897a's snapshots with the column stripped (and 7a42b60b's snapshots derived from them); no `gates/approved/` entry records the baseline stale-report wording change. | `.corpus-runs/gate/*/7a42b60b*` | record only |
| 28 | Docs | GAP | `.corpus-runs/README.md` still lists `location` in the snapshot line format. | `.corpus-runs/README.md:24` | now |
| 29 | Tests placement | GAP | The `diagnostics` crate's own unit tests (builder, `Identifier`, `tip_text`) live in the package (`checker/tests/diagnostic_identifiers.rs`). | that file | now |
| 30 | Leftovers | GAP | Empty directories on disk: `crates/phpstan-rs/src/ast_walk/tests`, `crates/phpstan-rs/proptest-regressions`. | `find crates -type d -empty` | now |
| 31 | CI | pending | The only recorded Rust CI run (`867d9cb3`) was in progress at the time of writing (fmt and clippy passed). | `gh run list` | check |

## Checked and in order

### §2 Crates, layers and workspace conventions
| Item | Status | Evidence |
|---|---|---|
| Virtual workspace, crates under `crates/`, `phpstan-rs-<name>` packages aliased in `[workspace.dependencies]` | OK | root `Cargo.toml` |
| `resolver = "3"`, edition 2024, version, `publish = false` inherited | OK | root `Cargo.toml` |
| `default-members`, `exclude = ["fuzz", "third_party/hotpath"]`, root profiles, `[patch.crates-io] hotpath` | OK | root `Cargo.toml` |
| Release profile: thin LTO, one codegen unit, `dhat` inherits | OK | `[profile.release]`, `[profile.dhat]` |
| `fuzzing` feature in `syntax`; `hotpath` features only on the bin | OK | `crates/syntax/Cargo.toml`, `crates/phpstan-rs/Cargo.toml` |
| Layer dependencies of the extracted crates (syntax; types → syntax; phpdoc → syntax; sql → none; diagnostics → syntax) | OK | manifests; module-graph gate passes |

### §3.1 syntax
| Item | Status | Evidence |
|---|---|---|
| Dense `NodeId` on every `Expr`/`Stmt` | OK | `ast.rs:10,47,1031`; `2bc27bab` |
| Spanned `TypeHint` incl. DNF | OK | `TypeHintKind` Name/Nullable/Union/Intersection; `9efed953` |
| Names resolved at parse time, written and resolved kept | OK | `QualifiedName { written, resolved }`; `2ec57e43` |
| `StaticAccess` split | OK | `ClassConstFetch`, `ClassNameFetch`, `StaticPropertyFetch`, dynamic variants |
| `Cast` enum | OK | `CastKind`, `CastSpelling` |
| Sub-name spans, `text_split` deleted | OK | `52ef3c2a`; no `text_split` module |
| Visitor / VisitorMut, `ast_walk` replaced, walkers ported | OK | `visit.rs`; `62520192`. The remaining self-recursive helpers either use `child_stmts`/`child_exprs`/`stmt_exprs` (built on the Visitor) or are fixed-shape queries (concat chains, flag or-chains, assignment targets, declaration indexing) |
| Visitor reaches static-access receivers | OK | `walk_static_target`, `child_exprs` via `any_child_expr` |
| Parse-time annotations (condition, last condition, catch types, goto targets, first statement, parents, `PHP_VERSION_ID` checks) | OK | `annotations.rs`; `b360e47f` (checker adopts in D7) |
| Lexer returns `Vec<LexWarning>` | OK | `lexer.rs:11,240`; `eaf22716` |
| Stub-attribute rewriting as an AST pass | OK | `0bec7068`; `stub_attributes` module (reflection, C) |
| Sharded interner | OK | `name.rs` 16-shard `Interner` |
| Helpers moved to syntax (`is_primitive_type_name`, `unwrap_grouping`, `literal_string_value`, `is_superglobal`, `body_contains_yield`) | OK | `syntax/src/ast.rs`, `visit.rs` |
| Extraction; fuzz targets repointed; lex/parse microbench | OK | `dcc5974b`; `crates/syntax/benches/lex_parse.rs` |

### §3.2 types
| Item | Status | Evidence |
|---|---|---|
| Variance args, late-resolvable (value-of, key-of, offset access, class constant, conditional) | OK | `VarianceArg`, `ValueOf`/`KeyOf`/`OffsetAccess`/`ClassConstant`, `Conditional` |
| `Static`/`This` unbound with args; bound form at D4 | SCHEDULED | §3.2 text, D4 |
| `ObjectWithoutClass`, `Resource`, `BenevolentUnion` | OK | variants |
| Typed shape keys, explicit unsealed tail | OK | `ShapeKey`, `Shape.unsealed` |
| Class names as interned `Name` | OK | `Object(Name, _)` |
| Consistent `PartialEq`/`Hash` | OK | `6f53291c` |
| Accessory `HasMethod`/`HasProperty`/`HasOffset`/`HasOffsetValue` | OK | variants; guards onto them at D2 (SCHEDULED) |
| `TrinaryLogic` in types | OK | `93e22de5` |
| `ClassHierarchy` trait, adapter over Registry | OK | `hierarchy.rs`, `registry/relation.rs` |
| One tri-state `is_super_type_of`, one `accepts` returning reasons | PARTIAL | reasons always empty — SCHEDULED in E3 |
| `is_subtype`/`is_assignable` deleted | OK | no such functions; `Relation::subtype` / `accepts` |
| No duplicate relation logic outside types | OK | checker helpers (`var_tag_types.rs`) build rule policy on `is_super_type_of` |
| Callers still on `Relation::structural()` | SCHEDULED | C8 |
| Union algebra incl. `MAX_UNION_MEMBERS`; coercions; `Type::list_of`; float formatting | OK | `union.rs:13`, `coercion.rs`, `lib.rs:820`, `dc448ded` |
| `PhpVersion` + `PhpVersions` | OK | `php_version.rs` |
| `PhpVersions` default from composer | SCHEDULED | B8 |
| `ExtensionTypeData` with eq/hash/describe/subtyping hooks, no mandatory merge | OK | `extension.rs`; `cb37d5cd` |
| Proptest seeds moved to `crates/types/proptest-regressions/`, default layout | OK | `proptest-regressions/tests/subtyping_properties.txt` mirrors `src/tests/subtyping_properties.rs` |

### §3.3 phpdoc
| Item | Status | Evidence |
|---|---|---|
| `DocBlock` stores `TypeNode`s | OK | `a9a17985` |
| Docblock string tokenizer deleted | OK | `b5a6d9ec` (was missed in B5, done in this pass) |
| `types::parse_type_string` deleted | SCHEDULED | C7 (with the adapter and `mentions_mixed`) |
| Doctrine-annotation argument parsing stays in the grammar | OK | `grammar::doctrine_argument_errors` |
| Extraction; fuzz target repointed | OK | `8b2fd202` |

### B6 sql
| Item | Status | Evidence |
|---|---|---|
| Own column model | OK | `sql::column` `Column`/`ColumnType`; `ff7655e9` |
| `[sql_reflection]` parsing in `sql` | OK | `sql::config` (`Deserialize`) |
| `SqlContext` service; state off `Registry` | OK | `sql::context`; `c2376d66`; corpus byte-identical |
| SQL → PHP type mapping in the ext-sql module (DQL raw fields keep their own copy: no ext-sql ↔ ext-doctrine edge) | OK | `checker/sql_reflection.rs`, `checker/doctrine/result_shape.rs` |
| Extraction with the drivers | OK | `6c23897a` |

### §3.7 / B7 diagnostics
| Item | Status | Evidence |
|---|---|---|
| Message, span, validated `Identifier`, list of tips, text fixes, `non_ignorable` | OK | `crates/diagnostics/src/lib.rs` |
| `location` and `metadata` removed | OK | `bad4a754`, `00a4b609` |
| File/trait file, identifier families, AST-node fixes, `accepts` reasons | SCHEDULED | E3 |
| Versioned baseline, v1 loading, `convert-baseline` | OK | `baseline.rs`; tests in `baseline_filtering.rs` |
| Extraction; ignore/baseline pipeline stays in the package | OK | `673bd53b` |

### Stage A
| Item | Status | Evidence |
|---|---|---|
| A0: all baselines recorded (corpus all configs and levels, type snapshots, Stage-A baseline, test manifest) | OK | `.corpus-runs/gate/*` from 2026-09-23 |
| A0: performance and build-time baselines | GAP | see #7, #8 |
| A0: CI runs the build-and-test half; submodule fetch limited to `vendor/phpstan-src` | OK | `.github/workflows/*.yml` |
| A0: snapshot format and commands in `.corpus-runs/README.md` | PARTIAL | see #28 |
| A0: `xtask gates` runner | OK | `xtask/src/main.rs:945` |
| A1: total order in CLI and LSP; approved delta | OK | `session.rs:780`, `lsp/protocol.rs:362`; `gates/approved/a1-total-order` |
| A2: capture, dual run, deletion of fully covered tests | OK | `2b331b4d` (3,076 tests replayed, then deleted) |
| A2: every diagnostic-asserting test converted | PARTIAL | see #15 |
| A2: family filter removed from the harness | OK | `tests/fixtures.rs` renders every diagnostic |
| A2: `examples/stress*.php` | OK | `2f96cba5`; no `examples/` |
| A2 known bugs | GAP | see #16 |

### B1, B2
| Item | Status | Evidence |
|---|---|---|
| Package under `crates/phpstan-rs`; fuzz repointed | OK | `3c1cde91`; `fuzz/Cargo.toml` |
| Bench and build script anchored on `CARGO_MANIFEST_DIR` | OK | `benches/vendored_corpus.rs:35`, `build.rs:43` |
| README / VSCode README / profile comments use `-p phpstan-rs` | OK | `README.md:54`, `editors/vscode/README.md:67`, `Cargo.toml:36` |
| CI path filters | OK | Rust CI runs on every push; extension CI filters on `crates/**`, `Cargo.lock`, `editors/vscode/**` |
| Module-graph gate; helpers moved down; `Diagnostic*` into its own module; `whole_program`/`duplicate_*` under `project` | OK | `4883cd2c`, `fbd86d25`, `f5a1527d`; no `checker → project` references |
| No listed exception past its phase | OK | every `allow` dated C1 or later |

### Appendix A rows due by B7
| Row | Status |
|---|---|
| token, lexer, parser, ast, printer, intern, ast_walk → syntax | OK |
| text_split deleted | OK |
| types/*, php_version, php_float_format, type_union → types | OK |
| registry/assignability → types | OK (module gone) |
| coercion → types (methods) + rules (reports, E3b) | OK (rule half in `checker/coercion.rs`, listed) |
| control_flow/generators → syntax (`body_contains_yield`) | OK |
| phpdoc_grammar, docblock → phpdoc | OK |
| sql_reflection_* → sql | OK |
| fix → diagnostics (model) | OK (application is pure text editing and moved too; the file writing stays in the bin) |
| tests/cli.rs, version.rs, lsp_exit_code.rs → bin package | OK |
| fuzz targets | OK |
| examples/stress*.php | OK |
| proptest-regressions → bin, then types | OK |

### Appendix F decisions touching finished work
| Decision | Status | Evidence |
|---|---|---|
| Subtyping home: oracle trait in types, one tri-state relation | OK | see §3.2 |
| ext-sql / ext-doctrine: no crate edge | OK | separate mappings, B6 |
| Fixture timing: harness backend | DEVIATION (early) | the harness already runs through `Session::for_fixture`, which F1 was to introduce |
| `Type` as a closed enum, every concept its own variant | PARTIAL | dead `$$`/`::` string checks remain (#9, #21) |
