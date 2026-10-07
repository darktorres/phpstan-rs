# Drill-down of the lib-suite callgrind profile (31.4 G Ir)

Profile: `/home/torres/.cache/prs-profile/cllib7/cg.out`, 1,000 tests in one process, `[profile.test]` (opt-level 1, debug info), `31,422,503,223` Ir.
Nothing was rebuilt or rerun and no source was edited.

## 0. How the numbers were obtained

- `nice -n 19 callgrind_annotate --inclusive=yes --tree=calling cg.out > /tmp/lib-drill/calling.txt` and `--tree=caller` > `/tmp/lib-drill/caller.txt` were generated, but the 6 MB files are too large to read by eye, and `incl.txt` (tree=none) does not list per-test functions.
- The drill-down uses my own parser of the raw `cg.out` (`/tmp/lib-drill/parse.py`, `lib.py`, `tree.py`, `tree2.py`). It reads every `fn=`, `cfn=`, `calls=` record and keeps, per function, the self cost and, per (caller, callee) edge, the call count and the inclusive cost of the edge.
  Check: the sum of all self costs it parsed is exactly `31,422,503,223`, equal to the PROGRAM TOTALS line.
- "Inclusive" of a function = sum of the edge costs into it. This is exact for non-recursive functions. Names ending in `'2` are callgrind's second recursion level; rayon `join_context`/`wait_until_cold'2` chains are recursive and inflate (the profile shows `bridge_producer_consumer::helper` at 490 % of the total). Whenever I quote a number from a recursive or rayon-generic frame I say so. Numbers for leaf functions and for the pass functions of `Registry` are not recursive and are exact.
- Shared nodes (`mi_malloc_aligned`, `hash_one`, `Once::call`, `OnceLock::initialize`) have one inclusive cost summed over all callers; in the trees I do not expand below them.
- Per-test attribution is limited by the profile: callgrind has no per-thread split here and rayon jobs are anonymous. Section 2 states exactly what is measured and what is derived.

## 1. Where the 31.4 G runs: threads and pools

Command: edges out of `std::sys::thread::unix::Thread::new::thread_start` (`/tmp/lib-drill/lib.py: callees()`).

| thread kind | Ir | share | detail |
|---|---|---|---|
| 24 rayon worker threads | 26,664,960,209 | 84.9 % | `wait_until_cold` (outermost) 26,664,692,495 |
| 1,000 libtest test threads | 4,685,763,719 | 14.9 % | `RunnableTest::run` 4,682,520,202 |
| other (debounce worker threads, main) | ~0.07 G | 0.2 % | |

The 24 workers are two pools of 12. One is `worker_pool()` (`crates/phpstan-rs/src/project.rs:263`); the other is built by a single test, `edits_across_every_fixture_match_a_fresh_load` (`crates/phpstan-rs/src/project/tests/incremental_updates.rs:1170`, the only other `ThreadPoolBuilder` in the crate; I grepped for it).

The outermost jobs of the 24 workers (exact, they are the first-level children of `wait_until_cold`, so not inflated):

| outermost job | Ir | share | calls |
|---|---|---|---|
| `worker_pool().install(run_finishing_passes)` issued from a thread of a different rayon registry (`in_worker_cross`) | 16,630,269,668 | 52.9 % | 674 |
| same, issued from a plain thread (`in_worker_cold`) | 3,911,327,091 | 12.4 % | 465 |
| the edits test's own pool: chunk jobs of its `par_chunks(50)` loop | 3,603,579,932 + 2,243,172,294 = 5,846,752,226 | 18.6 % | 2 + 1 |
| pieces of passes stolen by idle workers (various small `join_context` jobs) | ~0.28 G | 0.9 % | |

In rayon-core 1.13 (`registry.rs:496-512`, which I read) `in_worker` takes `in_worker_cold` when the current thread is not a worker at all, and `in_worker_cross` when it is a worker of another registry. A test that is not on a pool therefore goes through `cold`; the only test that runs on a pool of its own goes through `cross`.

Pass calls seen: `run_finishing_passes` is called 1,143 times (674 + 469 from the two edge types in `caller.txt`), 20,541,669,857 Ir inclusive, 17.97 M Ir per call on average (674 calls from the edits test: 24.7 M each; 469 calls from the rest: 8.3 M each).

## 2. Break-down by test

### 2.1 One test is ~72 % of the suite (derived, with the evidence)

`edits_across_every_fixture_match_a_fresh_load`:

- All 674 `cross` pass calls (16.63 G, 52.9 %) can only come from threads of its own pool, because it is the only test with a pool of its own (source above). The counts agree: callers of `Project::finish_declarations` are the closure of `assert_edits_match_fresh_loads::<edits_across_every_fixture...>` (30 calls on level 1 plus 345 on level `'2` = 375) and `update_file` called from its closure (276 + 24 = 300): 675 calls. (Command: `callers` of `finish_declarations`, `update_file`.)
- Its own pool's outermost chunk jobs: 5.85 G (18.6 %).
- Its test thread: `edits_across_every_fixture...::{closure#0} call_once` = 86,734,025 Ir.
- Sum: 16,630.3 + 5,846.8 + 86.7 = **22,563.8 M Ir = 71.8 %** of the 31.4 G.
- All 18,477 `check_one` / `Project::check_file` calls (3.69 G inclusive, 11.7 %) come from the incremental_updates tests: 2,978,881,783 (14,936 calls) from this test's closures, 378,369,792 (1,956) from `random_edits_match_a_fresh_load`, 164,050,889 (752) from `edits_across_many_fixtures_match_a_fresh_load`, 168,683,128 (758) from `update_file` closures. The 2.98 G sits inside the 5.85 G of chunk jobs above, so it is not additional.

The remaining 31.42 - 22.56 = 8.86 G (28.2 %) is everything else: 4.60 G on test threads, 3.91 G of passes issued from plain threads, ~0.35 G pass pieces and other.

### 2.2 Test threads (4,682,510,202 Ir for all 1,000 tests; exact, from `call_once` of each test closure; the two `LazyLock` statics `BACK` and `SOURCES` nested inside tests are excluded)

| module | Ir | share | tests |
|---|---|---|---|
| `project::tests::*` | 1,387,486,738 | 4.42 % | 103 |
| `lsp::tests::*` | 1,266,386,975 | 4.03 % | 222 |
| `lsp::workspace_symbol::tests::*` | 301,422,129 | 0.96 % | 15 |
| `registry::tests::*` | 889,174,382 | 2.83 % | 189 |
| `registry::stubs::tests::*` | 429,699,117 | 1.37 % | 15 |
| `registry::name_table::tests::*` | 213,440,588 | 0.68 % | 13 |
| `checker::tests::*` | 148,354,916 | 0.47 % | 21 |
| everything else (~80 small modules) | < 0.2 % together | | |

Most expensive single tests on their own thread:

| test | Ir | share |
|---|---|---|
| `lsp::tests::completion::bare_completion_needs_no_import_when_already_in_the_same_namespace` | 609,362,119 | 1.94 % |
| `project::tests::incremental_updates::random_edits_match_a_fresh_load` | 605,116,623 | 1.93 % |
| `registry::tests::stub_cache_round_trip::every_stored_table_reads_back_with_its_names_and_values` | 525,408,406 | 1.67 % (479 M of it is the `read_back::BACK` `LazyLock` init: `Registry::store` 420 M, `ClassReflection` encode 161 M) |
| `lsp::tests::completion::bare_completion_offers_a_class_with_an_auto_import_edit` | 419,616,315 | 1.34 % |
| `registry::stubs::tests::generated_stub_coverage::every_generated_stub_entry_decodes` | 305,767,393 | 0.97 % |
| `project::tests::incremental_updates::phpstans_own_classes_are_loaded_only_while_a_file_names_them` | 294,189,501 | 0.94 % |
| `registry::tests::stub_cache_round_trip::the_declarations_as_built_survive_a_round_trip` | 252,665,909 | 0.80 % |
| `project::tests::incremental_updates::edits_across_many_fixtures_match_a_fresh_load` | 220,051,116 | 0.70 % |

What the completion tests spend (tree from `lsp::tests::completion::run_edit`, 991,924,637 Ir): `lsp::run_loop` 653 M (`handle_request` 631 M: `Server::completion` 241 M of which `enumerate_bare` 240 M, and writing JSON responses 173 M) and `lsp::tests::helpers::decode_all` 502 M (JSON-decoding the responses, `lsp_server::msg::Message::_read` 344 M).

### 2.3 Passes issued from plain threads (3.91 G, 465 calls)

These are the ordinary tests (checker, lsp, project, rule). The profile cannot say which test issued which job. The call counts of `finish_declarations` from named callers: `lsp::workspace_symbol::tests::project` 15, `Project::remove_files` 14, `checker::tests::helpers::all_diagnostics_for_with` 6, `rule::tests::diagnostics_with` 6, `project::tests::property_assign_facts::load` 6, `assert_edits_match_fresh_loads::<random_edits...>` 32, `<edits_across_many_fixtures...>` 9, plus many tests that call it once. `lsp::Server::add_workspace_folders` calls it 192 times but those calls cost 81,922 Ir in total (they do no pass work).

## 3. Break-down by phase (non-recursive inclusive, exact)

| phase | Ir | share of 31.4 G | calls |
|---|---|---|---|
| `Project::run_finishing_passes` (all of 1.-3. below) | 20,541,669,857 | 65.4 % | 1,143 |
| `Checker::check_program` (inside `check_file`, 3,694,039,602 = 11.8 %) | 3,563,056,167 | 11.3 % | 18,477 `check_file` |
| drop of `Project` values (`drop_glue<Project>`) | 1,620,936,966 | 5.2 % | 771 |
| lazy stub decode, postcard (`stub_cache::decode<ClassReflection>` 1,240,514,939 + `<FunctionReflection>` 228,789,611) | 1,469,304,550 | 4.7 % | 10,217 + 14,040 + 5,282 (`load_base`/`decode_entry` entries of the function table; the 10,217 are the class decodes) |
| lsp work on test threads (`run_loop` 660.8 M + `decode_all` 516.3 M + others) | ~1.19 G (`run_with_capabilities` 1,187,044,552) | 3.8 % | |
| `Project::rebuild_declarations` | 737,625,686 | 2.3 % | |
| `project::parse_file` | 508,349,010 | 1.6 % | |

### 3.1 The passes inside `run_finishing_passes` (call counts all 1,143 or 1,149; Ir inclusive of one call site each)

| pass | Ir | % of 31.4 G | % of the passes |
|---|---|---|---|
| `Registry::end_declaration_changes` | 3,597,086,592 | 11.4 | 17.5 |
| `precompute_member_owners` | 2,442,691,064 | 7.8 | 11.9 |
| `flatten_phpdoc_inheritance` | 2,264,783,271 | 7.2 | 11.0 |
| `flatten_traits` | 1,825,073,791 | 5.8 | 8.9 |
| `changed_since` | 1,770,974,488 | 5.6 | 8.6 |
| `Project::closure_scope` (2 calls per pass) | 1,337,518,472 | 4.3 | 6.5 |
| `drop_doc_members_excluded_by_native` | 1,312,306,670 | 4.2 | 6.4 |
| `precompute_ancestor_names` | 1,273,347,598 | 4.1 | 6.2 |
| `warm` | 1,027,827,438 | 3.3 | 5.0 |
| `resolve_transitive_purity` | 753,776,236 | 2.4 | 3.7 |
| `resolve_constant_named_types` | 664,183,348 | 2.1 | 3.2 |
| `resolve_deferred_value_of` | 408,495,612 | 1.3 | 2.0 |
| `canonicalize_class_name_case` | 347,553,976 | 1.1 | 1.7 |
| `drop_glue<ResolvedSnapshot>` | 313,675,497 | 1.0 | 1.5 |
| `walk_affected` | 270,577,945 | 0.9 | 1.3 |
| `inherit_property_hooks` 189 M, `resolve_deferred_type_imports` 159 M, `resolve_deferred_mixins` 158 M, `infer_global_variables_of` 104 M, rest | ~0.7 G | ~2.3 | |

Each pass runs on a scope of the changed classes plus their dependents. Over the 1,143 calls the passes touch `177,289` classes (`own_owner_names` x177,289; `Registry::class_extends_implements_closure` x177,276; `flatten_phpdoc_inheritance_one_pass::{closure#0}` x177,280), i.e. 155 classes per call on average, about 116 K Ir per class per call across all passes (`20.54 G / 177,289`). Pass order is in `crates/phpstan-rs/src/project.rs:1200-1280`.

## 4. Walking each branch to its leaves

Source references are file:line of what I read. Percentages are of 31.4 G.

### 4.1 `end_declaration_changes` / `reset_scoped` (3.60 G, 11.4 %)  `crates/phpstan-rs/src/registry.rs:116-150` (`reset_scoped`), `:517-585`

`reset_scoped<ClassReflection>` 3,330,239,508 (10.6 %) x1,149; of it the per-key closure x256,743 = 2,900 M (9.2 %, rounded from the tree output). For each scope key the closure detaches the old resolved entry and inserts a clone of the raw one (`registry.rs:125`: `resolved.insert(name.clone(), value.clone())`). So every pass starts by deep-cloning ~155 `ClassReflection`s from `raw`:

- `RawTable<(Name, FunctionReflection)>::clone` (the `methods` map of each class) x176,972: 1,331 M (4.2 %) -> `FunctionReflection::clone` x1.34 M: 1,172 M (3.7 %), self 327 M (`function_sig.rs:129`) -> `clone_vec<ParameterReflection>` x1.87 M 876 M -> `ParameterReflection::clone` x1.97 M 503 M (self 182 M, `function_sig.rs:91`) -> `clone_vec<(Name, Name)>` (case_refs) x1.97 M 263 M and x1.87 M 230 M (self 232 M, `function_sig.rs:85`) -> `RawVecInner::try_allocate_in` -> `__rust_alloc` -> mimalloc.
- `NameTable::detach` x256,743: 304 M (1.0 %); `RawTable<(Name, PropertyReflection)>::clone` 239 M; `NameTable::insert` 233 M; `Vec<Name>::clone` 167 M.
- The `before: Vec<Option<Detached<ClassReflection>>>` is built with `Vec::new()` and `push` (`registry.rs:129`, no `with_capacity`): `RawVec<Option<Detached<ClassReflection>>>::grow_one` x4,490 = 327,821,830 (1.04 %), through `finish_grow` -> `__rust_realloc` -> `_mi_theap_realloc_zero` -> memcpy.
- `reset_scoped<FunctionReflection>` 302 M (1.0 %).

The old entries are then dropped by `drop_glue<ResolvedSnapshot>` (313,675,497; `Vec<Option<Detached<ClassReflection>>>` 280 M, 29,929 `ClassReflection` drops).

### 4.2 `precompute_member_owners` (2.44 G, 7.8 %)  `crates/phpstan-rs/src/registry/hierarchy.rs:355-440`

`own_owner_names` x177,289 = 1,977,890,532 (6.3 %), self 187,821,077 (1,059 Ir per call). It does a BFS over each class's ancestors and, for each ancestor, inserts every method, property and const name into three `FxHashMap<Name, Name>` (`hierarchy.rs:376-390`, guarded by `contains_key`):

- `HashMap<Name, Name>::insert` x2,953,326 (x2,901,882 from here): 1,026 M (3.3 %), self 325.7 M; of it `reserve_rehash` x565,519 576 M (self 250 M, with `new_uninitialized` 149 M and 2.07 M allocations)
- `HashMap<Name, Name>::contains_key` x4,524,606: 472 M (1.5 %), self 273 M
- `hash_one<&Name>` -> `<Box<str> as Hash>::hash` (hashes the name text, `crates/syntax/src/name.rs:130-134`): 17.85 M calls, 510 M

Around it: `NameTable<ClassOwners>::insert` x177,289 294 M and `forget` 204 M; `scoped_removed_class_keys` 163 M. The maps are rebuilt from scratch for every ancestor chain of every scoped class even though most chains share ancestors.

### 4.3 `flatten_phpdoc_inheritance` (2.26 G, 7.2 %)  `crates/phpstan-rs/src/registry/resolve.rs:355`, `:1097`

`flatten_phpdoc_inheritance_one_pass` x2,070 (the fix-point loop runs ~1.8 rounds per call) = 1,758 M; per-class closure x177,280 = 1,692 M (5.4 %), self 302 M (`resolve.rs:1097-`):

- `template_param_scopes_all_safe_to_backfill` x269,872: 511 M (1.6 %, `resolve.rs:1014`). Its lazy ancestor-bindings closure (`closure#4`) x97,840 = 404 M runs `OnceLock::initialize` x47,058 -> `Registry::ancestor_bindings` 397 M (`class_template_bindings` 247 M).
- `Type::replace_templates` x268,286: 214 M; `Type::reconcile_with_native` x268,286: 174 M; `HashMap<Name, FunctionReflection>` lookups x1.0 M: 135 M.
- Lazy stub decodes triggered inside (`load_base::{closure#1}` -> `stub_cache::decode<ClassReflection>`) are counted under 4.9.

### 4.4 `flatten_traits` (1.83 G, 5.8 %)  `resolve.rs:105-230`

`flatten_traits_one_pass::{closure#0}` x475,870 = 1,186 M (3.8 %), self 75 M. 475,870 is 2.7 calls per scoped class because `flatten_traits` loops to a fix point (`resolve.rs:106-112`):

- `FunctionReflection::clone` x432,151 = 357 M (`resolve.rs:176`: `methods.push((name.clone(), sig.clone()))` for each trait method, self cost in clone 327 M globally).
- `NameTable<ClassReflection>::get<str>` x475,870 = 311 M (`registry.classes[class_name]`, `resolve.rs:140`), self 406 M globally (388 Ir per call).
- `Vec<(Name, FunctionReflection)>::grow_one` x42,013 = 166 M (`resolve.rs:158` `Vec::new()`), `rustc_entry` x427,652 179 M (+ `reserve_rehash` 131 M), `contains_key` x1.31 M 153 M.
- `scoped_class_names` x2,981 from here: 161 M (see 4.8).

### 4.5 `changed_since` (1.77 G, 5.6 %)  `registry.rs:606-640`

For every class in the old snapshot it compares the whole resolved `ClassReflection` against the new one with `!=`. `HashMap<Name, FunctionReflection> == HashMap<...>` x164,439 = 1,117 M (3.6 %): `FunctionReflection::eq` x1,632,895 = 900 M (self 246 M, 151 Ir per call) -> `ParameterReflection` slice eq 313 M (self 210 M) -> `Type::eq` (x14.2 M calls globally, 245 M self, 17 Ir per call) and `Provenance::eq` 125 M. `PropertyReflection` map eq 145 M. The classes are compared even when `reset_scoped` just cloned them and the passes changed nothing.

### 4.6 `closure_scope` (1.34 G, 4.3 %)  `crates/phpstan-rs/src/project.rs:1110-1170`, `:65-83`

`StubFileSet::add_closure` x202,374 = 1,039 M (3.3 %), self 282 M (1,394 Ir per call): walks the bitset of the stub file closure and calls the visitor for each new stub file, which reaches every stub class/function of that file (`reached(...)`). `HashMap<String, ()>::insert` x618,746 198 M; `stubs::stub_file_closure` is computed lazily the first time per file: `OnceLock<Box<[u64]>>::initialize` x1,975 = 327 M (`Packed::get` x721 k 196 M, `Index::find` x923 k 171 M). `Scope::from_keys` 191 M x2,298. This is what makes the scope ~155 classes per pass: an edit that touches names referenced from stub files pulls in the stub files' declarations.

### 4.7 `drop_doc_members_excluded_by_native` (1.31 G, 4.2 %)  `resolve.rs:1696-1900`

- Closure over every method of every scoped class: `and_then_or_clear<FlatMap<...>>` x184,024 = 994 M (self 221 M); `Registry::excluding_native` x3,383,986 = 682 M (2.2 %, self 177 M, 52 Ir/call); most calls return at `if ty == native || native.is_mixed() { return None }` (`resolve.rs:1789`, I read this), costing a `Type::eq` (74 M from here). The real work in the 132,220 calls that go further: `Relation::is_super_type_of` 178 M.
- `scoped_classes()` Vec x1,149: 158 M (see 4.8).

### 4.8 `scoped_classes()` / `scoped_class_names()` (spread over several passes)  `crates/phpstan-rs/src/registry/scope.rs:189-246`

`scoped_classes()` is materialized as `Vec<(&Name, &ClassReflection)>` by each of six passes per call: `resolve_deferred_type_imports`, `inherit_property_hooks`, `resolve_constant_named_types`, `drop_doc_members_excluded_by_native`, `resolve_deferred_value_of`, `resolve_deferred_mixins`; 157.5 M to 158.2 M each x1,149-1,154 = ~0.95 G inclusive, `Vec::from_iter` self 525,240,151 (1.67 %). Per key it does `classes.get_with_name(&FoldedKey(key))`: `HashMap<Folded, ClassReflection>::get_key_value` x1,540,757 = 257 M (+ `hash_one<&str>` x3.4 M, `contains_key<str>` on the hidden set x2.1 M, `Base::find` x2.1 M 531 M with `eq_folded` 234 M) and, on the first touch, the stub decode (`OnceLock::initialize` x9,114 = 679 M, inside 4.9).
`scoped_class_names()` is cached by `(scope, classes.version())` but is rebuilt 3,483 times (3.05 per pass): closure x789,527 = 456 M (self 261 M, 330 Ir per key), 489 M total, from `flatten_traits` (x2,981), `precompute_ancestor_names` and `resolve_transitive_purity` (x1,355 each, 163 M each).

### 4.9 `warm` and lazy stub decoding  `scope.rs:263-290`, `crates/phpstan-rs/src/registry/name_table.rs:988` (`load_base`), `:1122` (`decode_entry`), `registry/stub_cache.rs:27`

`warm` 1,027,827,438 (3.3 %): per-class closure x257,371 = 897 M (self 168 M), which does `classes.get_with_name` and `raw.classes.get_with_name` in parallel to force the decode of stub entries. `NameTable::base_get` x470,911 = 680 M, which is `OnceLock<Arc<ClassReflection>>::initialize` x9,114 = 679 M (the decode itself) plus `Base::find` x2.14 M 531 M (self 342 M, 130 Ir per call) with `eq_folded` x2.5 M 234 M (`name_table.rs:243`, self 94 Ir/call).

The decode (4.7 % in total): `stub_cache::decode<ClassReflection>` x10,217 = 1,240,514,939 (121 K Ir each) -> `ClassReflection::deserialize` 1,235 M -> `HashMap<Name, FunctionReflection>::deserialize` 939 M -> `FunctionReflection::deserialize` x84,444 = 857 M -> `Vec<ParameterReflection>` 489 M -> `ParameterReflection::deserialize` x129,594 457 M -> `Repr` visit_enum x259,185 125 M, `Type::new` x259,185 60 M (`types::intern::insert` 179 M self globally), `Name::deserialize` x916,495 -> `Name::from(&str)` 259 M (`Interner::name` 1.22 M calls 331 M, `WeakTable::get_or_insert` 99 M + memcmp). Callers: `load_base::{closure#1}` x6,722 = 852 M, `decode_entry` x3,495 = 389 M.

### 4.10 `precompute_ancestor_names` (1.27 G, 4.1 %)  `hierarchy.rs:90-160`

`class_extends_implements_closure` x177,276 = 398 M (self 40 M): BFS into `FxHashSet<&Name>` and `FxHashSet<Name>` (`HashMap<&Name, ()>::insert` x446,594 140 M + `reserve_rehash` 176 M). `NameTable<HashSet<Name>>::insert` x176,972 254 M, `forget` 182 M, `scoped_class_names` 163 M, `scoped_removed_class_keys` 162 M (its closure x513,499 276 M), `Name::lower` hash path 120 M.

### 4.11 `resolve_transitive_purity` (0.75 G), `resolve_constant_named_types` (0.66 G), `resolve_deferred_value_of` (0.41 G), `canonicalize_class_name_case` (0.35 G)

- `resolve_constant_named_types` (`resolve.rs:1910`): `Vec<(Name, Name, FunctionReflection)>::spec_extend` x3,353 = 418 M calling `resolve::fixed_sig` x1,682,355 = 327 M (self 154 M, `resolve.rs:1974`; memcpy x1.77 M 115 M: it returns `Option<FunctionReflection>` by value) plus the `scoped_classes()` Vec 158 M.
- `resolve_transitive_purity` (`purity.rs:81`): `scoped_class_names` 163 M, classes Vec 109 M, `classify_purity` closure `NameTable<ClassReflection>::get<str>` x177,904 113 M; the rest runs in rayon closures whose cost I could not split below 0.3 %.
- `resolve_deferred_value_of`: `resolved_deferred_class` x177,276 188 M (`class_has_deferred_type` 147 M self 113 M, `resolve.rs:1427`), classes Vec 158 M.
- `canonicalize_class_name_case`: closure#2 x177,276 157 M, `DeclaredNames::get` 119 M, `NameTable::own` x513,486 132 M.

### 4.12 `check_program` (3.56 G, 11.3 %)  entirely from the incremental_updates tests (2.2)

Inclusive tree: `check_function` x36,992 2,657 M (8.5 %) -> `check_function_impl` 2,421 M (self 88 M) -> `check_block` x37,366 1,406 M -> `check_stmt_impl` 1,269 M -> `check_expr` 747 M -> `check_expr_impl` 910 M (self 15 M) -> `check_expr` (recursive, 370 M at level `'2`); `check_if::{closure#0}` 292 M; `FunctionReflection::from_decl` x13,046 201 M. About 96 K Ir per checked function. It is a long tail: the largest self costs in the checker/types/syntax crates are `Type::eq` 244.7 M (0.78 %), `types::intern::insert` 179.0 M (0.57 %, x1.08 M, callers `intern` 390 M inclusive), `Name::lower` 168.5 M (0.54 %), `Relation::is_super_type_of` 128.8 M + `'2` 124.2 M (0.41 % each; callers `excluding_native` 178 M x132 k, `absorb_contained_members` 66 M), `Interner::name` 99.3 M. No single leaf in the checker is above 0.8 %, so there is no checker hotspot in this profile.

### 4.13 `drop_glue<Project>` (1.62 G, 5.2 %), 771 drops

`drop_glue<Registry>` x771 1,576 M -> `NameTable<ClassReflection>` x2,315 1,371 M -> `drop_inner_table<(Folded, ClassReflection)>` -> `ClassReflection` drop x159,096 1,365 M (8.6 K Ir each) -> methods table drop x205,383 1,270 M -> `FunctionReflection` drop x1.79 M 1,189 M (self 99 M) -> `Vec<ParameterReflection>` drop x1.89 M 670 M -> `ParameterReflection` drop x1.99 M 435 M -> `RawVecInner::deallocate` x34.9 M (self 526 M globally, 15 Ir per call) -> `mi_free`. The cost is the number of allocations the per-pass clones of 4.1 create.

### 4.14 Others at or above 1.5 %

- `rebuild_declarations` 738 M: `generated_stub_classes_named` x10,327 342 M (`Vec<(Name, ClassReflection)>` 321 M, stub decode 223 M), `build_declarations_where` 172 M.
- `parse_file` 508 M: `parse_with_ignores` 287 M (`syntax::parse::parse` x3,267 306 M: lexer 132 M, parser 111 M), `build_declarations_where` 115 M.
- `lsp::run_loop` 661 M, `decode_all` 516 M: JSON serialisation/deserialisation (`serde_json` `Value`, `lsp_server::msg::Message`), `enumerate_bare` 240 M (iterates every class of the registry, forcing decode of 3,088 entries, 183 M).
- stub-cache round-trip tests (`Registry::store` 420 M, `NameTable::store`, `stub_cache::encode<ClassReflection>` x3,084 161 M).

## 5. Cross-cutting leaf costs (self Ir, aggregated by function name over all call sites)

Allocator and copy layer (inclusive entries): `__rust_alloc` 1,622,544,963 (5.2 %, 16.5 M calls), `__rust_dealloc` 882,927,623 (2.8 %), `__rust_realloc` 663,640,107 (2.1 %), `__memcpy_avx_unaligned_erms` self 1,604,575,346 (5.1 %). Together ~3.2 G (10.1 %) for allocation plus 1.6 G for memcpy.
Who allocates (callers of `__rust_alloc`): `RawVecInner::try_allocate_in` x8.89 M 770.8 M (callers: `clone_vec<(Name, Name)>` x2.29 M 261 M, `clone_vec<ParameterReflection>` x986 k 222 M, `to_ascii_lowercase` Vec<u8> x727 k 84 M, `Vec<Name>::clone` x2.28 M 84 M, `add_closure` x619 k 71 M), `hashbrown ... new_uninitialized` x2.07 M 377.5 M, `finish_grow` x2.97 M 244 M, `String::clone` x933 k 65 M.
Who frees: `RawVecInner::deallocate` x9.73 M 708 M (callers: `drop_glue<ParameterReflection>` x3.98 M 241 M, `drop_glue<FunctionReflection>` x3.77 M 199 M, `drop_glue<Vec<ParameterReflection>>` x1.89 M 186 M, `RawVec<u8>::drop` x1.62 M 80 M).

## 6. Ranked table of the 20 largest leaf costs (by self Ir of the function)

Ir/call = self Ir / total calls (`ecalls` summed over all callers).

| # | leaf function | self Ir | % | calls | Ir/call | who calls it and why | source | idea / why inherent |
|---|---|---|---|---|---|---|---|---|
| 1 | `memcpy` (`__memcpy_avx_unaligned_erms`) | 1,604,575,346 | 5.11 | 24.6 M | 65 | 550.7 M from the mimalloc realloc-zero path (x544,232, grown `Vec`s: `RawVec::grow_one` of `Option<Detached<ClassReflection>>` x4,490 alone 327.8 M); `fixed_sig` 114.9 M x1.77 M; `RawTable<(Name, FunctionReflection)>::clone` 89.4 M; `flatten_traits` 84.5 M; `Vec<u8>::extend` 61.4 M | `registry.rs:129`, `resolve.rs:158`, `resolve.rs:1974` | `Vec::with_capacity(snapshot_keys.len())` for `before`; `with_capacity` for `methods` in `flatten_traits_one_pass`. The by-value return of the large `FunctionReflection` in `fixed_sig` is probably an opt-level-1 move artifact (not verified). Mostly inherent to deep clones until entries are shared (see 7) |
| 2 | `FxBuildHasher::hash_one::<&str>` | 1,031,376,095 | 3.28 | 18.1 M | 57 | `HashMap<Folded, ClassReflection>::get`/`get_key_value` (x3.44 M + x3.54 M), hidden-set `contains_key<str>` x1.85 M, `HashMap<String, ()>::contains_key` x1.64 M (scope key sets) | `name_table.rs:516-540` (`get`, `base_get`), `scope.rs:189-246` | one table lookup hashes the key up to three times (entries map, `hidden` set, `Base::find`'s `hash_key`); compute the hash once. The 57 Ir per call is probably inflated by opt-level 1 (the `FxHasher::write` byte loop is not inlined) |
| 3 | `<Box<str> as Hash>::hash::<FxHasher>` | 604,811,849 | 1.92 | 20.4 M | 30 | `hash_one<&Name>` x17.85 M (510 M): all `HashMap<Name, _>` operations of `own_owner_names`, `FunctionReflection` maps, `class_extends_implements_closure` | `crates/syntax/src/name.rs:130-134` (`Hash for Name` hashes the text) | hash the interned pointer or a cached hash stored in `NameData` (needs `Borrow<str>` lookups changed). Probably partly an opt-1 artifact (non-inlined generic wrapper) |
| 4 | `RawVecInner::try_allocate_in` | 589,406,719 | 1.88 | 15.7 M | 38 | `clone_vec<(Name, Name)>` x2.29 M, `clone_vec<ParameterReflection>` x986 k, `to_ascii_lowercase` x727 k, `Vec<Name>::clone` x2.28 M | `function_sig.rs:85` | fewer clones (see 7); the call layer itself is mostly an opt-1 artifact (inlined in release) |
| 5 | `RawVecInner::deallocate` | 525,758,785 | 1.67 | 34.9 M | 15 | `drop_glue` of `ParameterReflection`/`FunctionReflection`/`Vec<ParameterReflection>`/`Vec<AssertInfo>` (x5.7 M, 69 M) | drop of per-pass clones, `registry.rs:125` | fewer clones; opt-1 artifact of the non-inlined 15-Ir wrapper called 34.9 M times |
| 6 | `Vec<(&Name, &ClassReflection)>::from_iter` | 525,240,151 | 1.67 | 6.9 k (x1,149 per pass) | 76 K | six passes each build `scoped_classes()` (4.8): `resolve_deferred_type_imports` 158.2 M, `inherit_property_hooks` 158.1 M, `resolve_constant_named_types` 157.6 M, `drop_doc_members_excluded_by_native` 157.5 M, `resolve_deferred_value_of` 157.5 M, `resolve_deferred_mixins` | `scope.rs:226-233` | build `scoped_classes()` once per call and share it (the names are already cached by `scoped_class_names`); self includes inlined hash and probe code (opt-1) |
| 7 | `mi_free` | 522,330,375 | 1.66 | 17.4 M | 30 | `__rust_dealloc` x14.9 M 808 M inclusive, hash-table drops | allocator | fewer allocations; inherent otherwise |
| 8 | `mi_theap_malloc_aligned_at` | 439,448,379 | 1.40 | 17.3 M | 25 | `__rust_alloc` -> `mi_malloc_aligned` (190.6 M self) -> `mi_theap_malloc_aligned` (121.3 M self) -> here; with `_mi_page_malloc_zero` 286.5 M (rank 17) the allocation path is ~1.04 G self | `lib.rs:16` global allocator | four call layers per allocation; I did not check mimalloc's compile flags. Fewer allocations is the only sure lever |
| 9 | `NameTable<ClassReflection>::get::<str>` | 405,944,387 | 1.29 | 1.05 M | 388 | `flatten_traits_one_pass::{closure#0}` x475,870 311 M, `classify_purity` x177,904 112.5 M, `check_function_impl` x86,520 | `name_table.rs:516-540` | inlined table probing (entries map then base); opt-1 inlines `with_key`/closures into it. Hoist the `registry.classes[class_name]` lookup out of the fix-point loop |
| 10 | `eq_folded` | 383,063,498 | 1.22 | 4.07 M | 94 | `Base<ClassReflection>::find` x2.55 M 234 M, `Base<FunctionReflection>` x435 k 69 M, `ClassOwners` 47 M | `name_table.rs:243-262` | word-at-a-time compare already; 2.5 M compares for 2.1 M finds means ~1.2 probes per find; inherent to hash-index probing |
| 11 | `Base<ClassReflection>::find` | 341,843,043 | 1.09 | 2.63 M | 130 | `NameTable::base_get` x2.14 M 531 M (from `warm`, `scoped_classes`, ...), `detach` x227 k 76 M | `name_table.rs:405-440` | `warm` already did these lookups in parallel; the passes repeat them (4.8, 4.9): keep resolved references |
| 12 | `FunctionReflection::clone` | 327,335,295 | 1.04 | 1.87 M | 176 | `RawTable<(Name, FunctionReflection)>::clone` x1.34 M 1,172 M (reset_scoped, 4.1), `flatten_traits_one_pass` x432 k 357 M, `reset_scoped<FunctionReflection>` x72 k 74 M | `function_sig.rs:129-` | share `FunctionReflection` behind `Arc` (copy on write) so reset and trait flattening do not copy parameter vectors; inherent as a deep copy |
| 13 | `HashMap<Name, Name>::insert` | 325,707,710 | 1.04 | 2.95 M | 110 | `own_owner_names` x2.90 M (1,026 M inclusive, incl. `reserve_rehash` x565 k 576 M) | `hierarchy.rs:376-390` | build the owner maps with `with_capacity` (`reserve_rehash` 249.6 M self, ~0.6 rehash per class), or share parent maps and only add the class's own members; cache per-class results across passes |
| 14 | `memcmp` (`__memcmp_avx2_movbe`) | 311,973,998 | 0.99 | 17.0 M | 18 | `HashMap<Folded, ClassReflection>` key compares x3.23 M + x1.98 M, `WeakTable::get_or_insert` x1.28 M, `starts_with` x965 k | name tables, interner | inherent to string keys; fewer lookups |
| 15 | `flatten_phpdoc_inheritance_one_pass::{closure#0}` | 302,138,729 | 0.96 | 177,280 | 1,704 | rayon `FilterMapFolder`, once per scoped class per round | `resolve.rs:1097-` | per-class body is inlined here (hash lookups, `Type` ops); skip classes whose ancestors did not change between rounds (the loop runs ~1.8 rounds) |
| 16 | `HashMap<Folded, ClassReflection>::get::<str>` | 295,749,792 | 0.94 | 1.05 M (via `NameTable::get`) | | `NameTable::get` x1.05 M 179 M, `own_owner_names` x393 k 64 M, `class_extends_implements_closure` x393 k 64 M, `changed_since` x257 k 43 M | `name_table.rs:516` | one lookup per ancestor step in the BFS; cache ancestor lists |
| 17 | `_mi_page_malloc_zero` | 286,455,642 | 0.91 | 15.9 M | 18 | `mi_theap_malloc_aligned_at` x14.95 M | allocator | see 8 |
| 18 | `HashMap<Folded, ClassReflection>::get_key_value` | 283,343,042 | 0.90 | 1.54 M | 184 | `scoped_classes()` vec x1.54 M (257 M), `scoped_class_names` closure x789 k 132 M, `warm` x515 k 49 M | `scope.rs:189-246` | see 6 and 20 |
| 19 | `StubFileSet::add_closure::<closure_scope::{closure#2}>` | 282,062,530 | 0.90 | 202,374 | 1,394 | `Project::closure_scope` x202,374 (1,039 M inclusive) | `project.rs:65-83`, `:1110-1170` | per-file closure bitsets are or-ed word by word for every changed key and every reached stub file; cache the per-edit result by changed-key set, or skip stub files already in `stub_files`. The inlined visitor (`reached` -> `HashMap<String, ()>::insert`) is part of the self cost |
| 20 | `HashMap<Name, Name>::contains_key::<Name>` | 272,912,168 | 0.87 | 4.52 M | 60 | `own_owner_names` only | `hierarchy.rs:376-390` | the `contains_key` + `insert` pair hashes the name twice; use `entry().or_insert_with` (the comment at `hierarchy.rs:338-341` says it was avoided to skip cloning the key; `raw_entry` or `try_insert` would avoid both) |

Next five by self Ir, not in the table: `HashMap<Name, Name>::reserve_rehash` 249.6 M, `hash_one<&Name>` 249.9 M, `scoped_class_names::{closure#0}::{closure#0}` 260.6 M (4.8), `FunctionReflection::eq` 246.3 M (4.5), `Type::eq` 244.7 M.

### Clusters (what the leaves add up to)

| cluster | members | Ir | % |
|---|---|---|---|
| allocation, free, realloc (inclusive entry points) | rows 4, 5, 7, 8, 17 and `mi_malloc_aligned`, `finish_grow` | ~3.17 G | 10.1 |
| deep clone of `ClassReflection`/`FunctionReflection` | `reset_scoped` 3.33 G (inclusive, includes part of the allocation above), `flatten_traits` clones 0.36 G | 3.7 G | 11.8 |
| hashing of names | rows 2, 3, 14, `hash_one<&Name>` | ~1.9 G | 6 |
| stored-table lookup (`NameTable` + `Base::find` + `eq_folded`) | rows 9, 10, 11, 16, 18 | ~1.7 G | 5.4 |
| lazy postcard decode of stub entries | section 4.9 | 1.47 G | 4.7 |
| owner/ancestor map rebuilds | rows 13, 20, section 4.2, 4.10 | ~2.9 G inclusive | 9 |

## 7. What is inherent and what looks fixable (ideas only; none were tried)

- The biggest structural cost is that every pass call (1,143) starts from deep clones of every scoped class from `raw` (4.1: 3.33 G, plus 1.6 G of drops, 4.13, and 1.77 G of full-class compares in `changed_since`, 4.5). An `Arc`-shared or copy-on-write `FunctionReflection`/`ClassReflection` would remove most of it and most of rows 1, 4, 5, 7, 8, 12.
- The six `scoped_classes()` materializations, three `scoped_class_names()` rebuilds per call and the repeated `warm` lookups (4.8, 4.9) look up the same ~155 keys many times; one resolved list per call would cut rows 6, 9, 11, 16, 18.
- `own_owner_names` and `class_extends_implements_closure` rebuild per class from scratch (4.2, 4.10: 2.97 G + 1.27 G inclusive).
- Inherent: the checker (4.12) has no hotspot; the postcard decode (4.9) is about 121 K Ir per class and only avoidable by decoding fewer classes (`warm` and `scoped_classes` force the decode of every scope key).
- One test, `edits_across_every_fixture_match_a_fresh_load`, is ~72 % of the suite's instructions (2.1). Making it smaller (fewer fixture groups, fewer edits) would lower the suite more than any code change; whether its coverage is needed is outside what this profile can say.

## 8. Which leaf costs are probably opt-level-1 artifacts

Observed per-call costs that look like missing inlining (the source is a thin wrapper, but I did not compare with a release build): `RawVecInner::try_allocate_in` 38 Ir, `RawVecInner::deallocate` 15 Ir x34.9 M, `Box<str>::hash` 30 Ir x20.4 M, `hash_one<&str>` 57 Ir x18.1 M, `mi_malloc_aligned`/`mi_theap_malloc_aligned` 11 and 7 Ir x17.3 M (the call chain `__rust_alloc` -> `mi_malloc_aligned` -> `mi_theap_malloc_aligned` -> `mi_theap_malloc_aligned_at`; whether mimalloc's C is optimized in this build I did not check), `drop_glue` layers (4.13), the by-value `Option<FunctionReflection>` memcpy in `fixed_sig` (row 1), `Type::eq` 17 Ir x14.2 M. The big self costs of `Vec::from_iter` (row 6), `NameTable::get<str>` (row 9) and `add_closure` (row 19) include inlined closures and iterator adapters; at opt-level 1 these are not folded as in release, so their self cost is overstated relative to the call counts, not their inclusive cost. Counts (calls, rounds, classes per pass) are not affected by optimization level.

## 9. Not obtained / limits

- Exact cost per test for the pool-executed work: callgrind has no job-to-test link. The 71.8 % figure for `edits_across_every_fixture_match_a_fresh_load` is derived from the pool and registry structure (sections 1 and 2.1), not read off a profile line; the 3.91 G of passes from plain threads cannot be split per test.
- Rayon-generic and `'2` frame inclusive costs (up to 490 % in `incl.txt`) are inflated by recursion and were not used except where stated.
- The split of the 0.28 G of stolen pass pieces and the inside of `resolve_transitive_purity` below 0.3 % was not resolved.
- Inclusive numbers of shared nodes (`Once::call` 2.996 G, `OnceLock::initialize`) mix several callers and were only used through their per-caller edges.
