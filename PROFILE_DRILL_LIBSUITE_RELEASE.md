# Drill of the phpstan-rs lib suite, release build (opt-level 3, thin LTO)

Profile: `/home/torres/.cache/prs-profile/cl-rel/cg.out`, 20,628,507,592 Ir, 1,000 tests in one process (log: "1000 passed ... finished in 167.02s").
Comparison profile: `/home/torres/.cache/prs-profile/cllib7/cg.out` (opt-level 1, 31,422,503,223 Ir). `PROFILE_DRILL_LIBSUITE.md` did not exist when this was written, so nothing was read from it.

## 0. Method and limits

Commands (all on the existing files, nothing rebuilt or rerun):

```
callgrind_annotate --threshold=100 --inclusive=yes --tree=none    cl-rel/cg.out > /tmp/rel-drill/incl.txt
callgrind_annotate --threshold=100 --inclusive=no  --tree=none    cl-rel/cg.out > /tmp/rel-drill/self.txt   (includes per-line annotated sources)
callgrind_annotate --threshold=100 --inclusive=yes --tree=calling cl-rel/cg.out > /tmp/rel-drill/calling.txt
callgrind_annotate --threshold=100 --inclusive=yes --tree=caller  cl-rel/cg.out > /tmp/rel-drill/caller.txt
(same --tree=none pair on cllib7/cg.out -> /tmp/rel-drill/o1_incl.txt, o1_self.txt)
```

`--threshold=100` was needed: at the default 99 the callee lists were truncated. Small python helpers in `/tmp/rel-drill` (`g.py`, `qc.py`, `tree.py`, `ann.py`, `groups.py`, `cmp*.py`) only parse those outputs: they merge the per-file entries of one function, and list callees/callers. The "self" table summed over merged function names equals the 20,628,507,592 total exactly, so the merge loses nothing.

Things to keep in mind when reading numbers:

- Recursion/rayon: names ending in `'2` are recursive instances. A rayon worker waiting in `join` steals other jobs, so the inclusive cost of the test closure of `edits_across_every_fixture...` shows 79 G to 173 G (385% to 842%). Those figures are upper bounds and I do not use them. Wherever a number matters I use either the top-level job under the worker root (`wait_until_cold`, non-recursive) or a call edge from the caller/callee lists.
- Call-edge cost (`calling`/`caller` lists) is inclusive for that edge. When one function appears under several files (inlined code), the entry under its own file is the function total.
- The finishing passes run on the calling pool worker because every pass hands out jobs of at least `PASS_CHUNK` = 32 classes (`registry.rs:101`) and a scope holds ~165 classes per run (188,037 class owners built / 1,141 runs, from the call counts below); the stolen-job entries are tiny. So pass costs below are mostly serial, per project.
- Per-test attribution is only possible for work on a test thread (the test name is in the stack) and for the one test that builds its own pool. Work done on the shared `worker_pool()` has no test name in its stack; for it I can give call counts and per-run averages, not per-test Ir. I say so where it applies.
- At opt 3 many small functions are inlined. Where a cost only shows inside its caller I say so.

## 1. The 20.6 G split by thread and test group

`std::sys::thread::unix::Thread::new::thread_start` holds 20,570,424,520 Ir (99.72%). Its callees (`--tree=calling`, function `boxed.rs:...::thread_start`):

| callee | Ir | % | calls |
|---|---|---|---|
| rayon worker main loop (`DefaultSpawn::spawn` closure) | 17,588,645,108 | 85.27 | 24 threads |
| test thread closures (`test::run_test`) | 2,977,740,421 | 14.43 | 1,000 |
| lsp debounce workers | 2,202,142 | 0.01 | 205 |
| other test helper threads | 1,831,937 (1,726,088 + 105,849) | 0.01 | 2 |
| main thread, harness, startup (total minus thread_start) | 58,083,072 | 0.28 | - |

Rayon worker top-level jobs (children of the non-recursive `WorkerThread::wait_until_cold`, 17,571,100,985):

| job | Ir | % of total | who |
|---|---|---|---|
| `pool.install(finish_declarations::{closure})` as `in_worker_cross` (caller is a worker of another pool) | 10,048,249,170 | 48.71 | `edits_across_every_fixture_match_a_fresh_load` only (see below) |
| same, `in_worker_cold` (caller is a plain thread) | 2,894,597,274 | 14.03 | every other test that loads a project |
| `join call_b` of `par_chunks(50)` bridge (test pool, stolen halves) | 2,852,801,089 | 13.83 | same fixture test |
| `install<edits_across_every_fixture...::{closure#0}>` (test pool root job) | 1,042,731,434 | 5.05 | same fixture test |
| `Registry::warm::{closure#0}` par_iter over scope keys | 612,398,749 (614,921,270 in the job-type total) | 2.97 | first-touch stub decode, not attributable per test |
| small jobs and idle/steal loop | 137,867,392 | 0.67 | - |

Why `in_worker_cross` is the fixture test: it is the only place that builds a second pool (`grep ThreadPoolBuilder`: `project.rs:270` for `worker_pool()` and `project/tests/incremental_updates.rs:1170`). The test (`incremental_updates.rs:1162-1182`) runs `par_chunks(50)` on its own pool; each group calls `assert_edits_match_fresh_loads` (`:1051-1124`), which loads a fresh project (finishing passes on `worker_pool()`, so a cross-pool install) and calls `check_file` for every analyzed file.

Non-overlapping partition of the whole profile (sum 20,624,468,601, 4.04 M or 0.02% unattributed):

| bucket | Ir | % |
|---|---|---|
| `edits_across_every_fixture`: finishing passes (cross-pool) | 10,048,249,170 | 48.71 |
| `edits_across_every_fixture`: work on its own pool (check, drop, parse, update_file) | 3,895,532,523 | 18.88 |
| `edits_across_every_fixture`: test thread | 82,781,735 | 0.40 |
| **that test, total** | **14,026,563,428** | **68.00** |
| finishing passes of all other tests (472 runs) | 2,894,597,274 | 14.03 |
| test-thread work of all other tests | 2,894,958,686 | 14.03 |
| `warm` job (first-touch stub decode) | 612,398,749 | 2.97 |
| small rayon jobs | 137,867,392 | 0.67 |
| main/harness | 58,083,072 | 0.28 |

(The cross-pool run total as an inclusive call edge is 10,465,395,223 = 10,048,249,170 top-level + 417,194,963 nested under a `wait_until_cold'2`; the nested part is already inside a top-level job, so I use the top-level number. `run_finishing_passes` inclusive, 13,359,995,879 (64.76%), is therefore an upper bound by at most that 0.42 G.)

Counts: `run_finishing_passes` runs 1,141 times: 660 + 10 cross jobs and 470 + 2 cold jobs, i.e. about 670 cross and 472 cold. Average per run: cross 10,465,395,223 / 670 = 15.6 M, cold 2,894,600,656 / 472 = 6.1 M (my arithmetic on the two edge totals).

### 1.1 Test-thread work by test group (2,974,858,036 Ir over 995 test closures matched by name; the other 5 hold about 1,000 Ir)

Script `groups.py`, from the callees of `test::__rust_begin_short_backtrace` (`call_once` of each test closure).

| group | Ir | % | tests |
|---|---|---|---|
| `project::tests::incremental_updates` (test-thread part only) | 910,778,112 | 4.42 | 43 |
| `lsp::tests::completion` | 598,006,277 | 2.90 | 7 |
| `registry::tests::stub_cache_round_trip` | 546,253,006 | 2.65 | 6 |
| `registry::stubs::tests` | 279,006,362 | 1.35 | 15 |
| `lsp::workspace_symbol::tests` | 221,921,779 | 1.08 | 15 |
| `registry::name_table::tests` | 124,026,746 | 0.60 | 13 |
| `checker::tests` | 108,000,255 | 0.52 | 21 |
| all other (about 875 tests) | 186,865,499 | 0.91 | - |

Largest single tests on a test thread (Ir): `random_edits_match_a_fresh_load` 402,590,563; `bare_completion_offers_a_class_with_an_auto_import_edit` 356,554,338; `a_read_back_registry_iterates_its_names_in_lowercased_order` 299,746,900; `bare_completion_needs_no_import_when_already_in_the_same_namespace` 236,003,515; `every_generated_stub_entry_decodes` 195,769,614; `phpstans_own_classes_are_loaded_only_while_a_file_names_them` 187,344,628; `the_resolved_stub_registry_survives_a_round_trip` 169,164,759; `edits_across_many_fixtures_match_a_fresh_load` 149,281,481; `edits_across_every_fixture_match_a_fresh_load` 82,781,735 (only its thread).

Caveat on first-touch costs: the stub tables decode an entry the first time it is read (`name_table.rs:453-465`, `Base::value`), through process-wide `OnceLock`s. Whichever test touches an entry first pays for it, so the stub-related groups above (completion, stub_cache_round_trip, stubs tests) carry decode costs that other tests then get for free. Total of that lazy work: `Once::call` 2,006,999,545 (9.73%), split in section 4.11.

Call counts that give the cold-run share by caller: `finish_loading` callers (from `--tree=caller`): `lsp::Server::add_workspace_folders` 192 calls, the fixture closure 65, `a_batch_load_equals_a_load_file_by_file` 2, about 110 other single-call tests; `update_file` calls `finish_declarations` 107 times. The Ir per cold run is not separable per test.

### 1.2 What the fixture test's own pool does (3,895,532,523)

Edges from the test closures (call counts in brackets):

| work | Ir | % | calls |
|---|---|---|---|
| `Project::check_file` (all files of every edit state, diagnostics closure) | 2,334,270,794 | 11.32 | 17,438 |
| `drop_glue<Project>` (a fresh project is dropped per load) | 949,213,717 | 4.60 | 375 |
| `Project::update_file` | 359,587,594 | 1.74 | 300 |
| `parse_file` | 264,837,447 + 55,780,886 = 320,618,333 | 1.55 | 2,021 + 436 |
| `insert_parsed_file` (+ `Option<ParsedFile>::clone` 86,776,540) | 119,815,068 + 31,395,799 = 151,210,867 | 0.73 | 10,844 |
| diagnostic `format!` | 43,636,379 | 0.21 | 26,778 |

These add to about 4.16 G against the 3,895,532,523 job total; I could not reconcile the 0.26 G (6.8%) gap (recursion levels and inlined attribution). I report both.

## 2. Phases inside the finishing passes (`Project::run_finishing_passes`, `project.rs:1200-1275`)

Callee edges, 1,141 calls each unless noted (13,359,995,879 Ir total, 64.76%, self 180,501,171):

| callee | Ir | % | note |
|---|---|---|---|
| `reset_scoped::<ClassReflection>::{closure#0}` (inlined `end_declaration_changes`) | 1,848,081,324 | 8.96 | 262,858 calls |
| `precompute_member_owners` | 1,590,888,619 | 7.71 | |
| `flatten_phpdoc_inheritance` | 1,506,682,303 | 7.30 | 2,077 one-pass calls |
| `Option<&ClassReflection> == ` (inlined `changed_since`) | 1,242,541,932 | 6.02 | 263,484 calls |
| `flatten_traits` | 1,100,990,116 | 5.34 | 1,629 helper runs |
| `drop_doc_members_excluded_by_native` | 840,780,195 | 4.08 | |
| `closure_scope` | 808,633,801 | 3.92 | 2,282 calls |
| `precompute_ancestor_names` | 784,842,514 | 3.80 | |
| `resolve_transitive_purity` | 438,770,988 | 2.13 | |
| `resolve_constant_named_types` | 425,612,109 | 2.06 | |
| `Vec<Option<Detached<ClassReflection>>>::grow_one` | 330,633,372 | 1.60 | 4,454 calls |
| `Registry::warm` par_iter helper | 308,876,937 + 295,036,073 | 1.50 + 1.43 | 1,131 + 10 (overlaps the `warm` job above) |
| `canonicalize_class_name_case` | 233,583,236 | 1.13 | |
| `resolve_deferred_value_of` | 214,584,493 | 1.04 | |
| `drop_glue<Option<ResolvedSnapshot>>` | 213,335,833 | 1.03 | |
| `reset_scoped::<FunctionReflection>::{closure#0}` | 169,499,009 | 0.82 | 72,851 calls |
| `resolve_method_tags` | 103,561,061 | 0.50 | |
| `inherit_property_hooks`, `infer_global_variables_of`, `precompute_member_owners_after_mixins`, `resolve_deferred_type_imports`, `resolve_deferred_mixins` | 76.9 M, 72.3 M, 62.4 M, 57.5 M, 57.4 M | 0.28-0.37 each | |

The passes are not independent of one another in cost: most of them start from a `scoped_classes()`/`scoped_class_names()` list (section 4.9) and `Base::find` lookups (4.14).

## 3. Branch walk (every branch of at least ~1.5% of 20.6 G)

Each section ends with the leaf, its self Ir, the caller and why, and the source.

### 3.1 reset_scoped: copy of every scoped class (1,848,081,324 = 8.96%)

Source: `registry.rs:116-148` (`reset_scoped`), the closure at `:122-128`, called at `:134` (called from `end_declaration_changes`, `:517-565`, inlined into `run_finishing_passes`). For each key of the scope it detaches the old resolved class and re-inserts a copy of the raw declaration (`resolved.insert(name.clone(), value.clone())`, `:125`).

Callees of the closure (class instance): `ClassReflection::clone` 1,366,068,063 (6.62%, 187,713 calls, 7,278 Ir per clone), `NameTable::insert` 203,402,859 (187,713), `base_get` 82,592,621 (253,897), `Base::find` 59,174,298 (235,946), memcpy 43,065,085 (267,485), `HashMap<Folded,()>::insert` 36,632,222, `contains_key` 17,635,756.

Leaf under the clone: `ClassReflection` is `#[derive(Clone)]` (`class_info.rs:345-347`). Its callees: `FunctionReflection::clone` 799,081,545 (1,414,948 calls), `PropertyReflection::clone` 97,492,498 (400,034), `HashMap<Name,()>::clone` 52,170,030, `HashMap<Name,Vec<Type>>::clone` 27,374,665, `HashMap<Name,Type>::clone` 22,338,261, `ReflectionAttribute` `to_vec` 21,351,095, memcpy 109,815,887 (2,217,420 calls), `mi_malloc_aligned` 112,118,769 (511,965). `FunctionReflection::clone` is hand-written (`function_sig.rs:129-`, `clone_vec` at `:85`): self 458,719,438 (2.22%), `ParameterReflection::clone` self 246,747,326, `clone_vec<AssertInfo>` self 123,503,697, and 2,159,189 mallocs (194,627,781 in `mi_malloc_aligned`).

Why: with a scope that is not "all", the registry copies the raw (as-built) entry of every class in the scope over the resolved one, so the passes can run again. 1,141 runs reset 262,858 classes (about 230 per run).

Related leaf, `Vec::new()` + `push` in the snapshot loop (`registry.rs:129-138`): `RawVec<Option<Detached<ClassReflection>>>::grow_one` 336,683,301 (1.63%, 4,507 calls, 74.7 K Ir per call), all through `RawVecInner::finish_grow` -> `mi_realloc_aligned` (copying). The number of entries is known from `snapshot_keys`, so `with_capacity` removes this; the growth shows as 547,202 realloc-memcpy calls in the memcpy breakdown (3.14).

### 3.2 changed_since: compare every class with its snapshot (1,242,541,932 = 6.02%)

Source: `registry.rs:606-640`, class loop `:614-618`, function loop `:621-625` (inlined into `run_finishing_passes`; the `cmp.rs` entry of that function). 263,484 calls of `Option<&ClassReflection> == Option<&ClassReflection>`; 178,512 reach the deep compare.

Callees (`Option<&ClassReflection>::eq`): `HashMap<Name,FunctionReflection> ==` 996,853,533 (4.83%, 178,512 calls), `HashMap<Name,PropertyReflection> ==` 100,837,401, `HashSet<Name> ==` 32,234,392 (1,605,814), `IndexMap<Name,ConstantReflection> ==` 30,290,173, `HashMap<Name,Type> ==` 15,193,708. Under the first: `FunctionReflection::eq` 873,815,677 (4.24%, 1,726,396 calls from the map compare), self 249,916,375; `Vec<ParameterReflection> ==` self 201,078,862 (inclusive 201,576,728, its only caller is `FunctionReflection::eq`, 1,806,517 calls); `Vec<AssertInfo> ==` self 113,990,922; `Provenance ==` inclusive 131,115,695 (119,967,985 from `FunctionReflection::eq`, 11,147,710 directly from the class compare); `Origin ==` self 69,366,112 (callers not traced).

Why: after the passes, every class in the scope is compared field by field with the copy taken by `reset_scoped`, to find which dependency keys changed. Together 3.1 + 3.2 is 3,090,623,256 Ir (14.98%) for the snapshot-and-compare bookkeeping.

### 3.3 precompute_member_owners (1,590,888,619 = 7.71%)

Source: `hierarchy.rs:411-440`, with `own_owner_names` at `:355-396`. Callees: the par_iter helper 1,317,352,648 (6.39%, 1,373 calls) and `NameTable<ClassOwners>::forget` 168,966,364 (267,809), `scoped_removed_class_keys` 60,224,875, `reserve_rehash` of the owner table 25,983,984.

Leaf: the per-class closure (`call_mut`, 1,335,545,239 inclusive, self 419,849,418). Its callees: `HashMap<Name,Name>::insert` 714,283,593 (3,064,239 calls; total function 719,558,704, self 334,095,405), of which `RawTable<(Name,Name)>::reserve_rehash` 385,178,671 (598,581 calls, self 280,609,681); `HashSet<&Name>::insert` 95,963,383 (475,001 calls, the `seen` set at `hierarchy.rs:366-372`); class lookup `HashMap<Folded,ClassReflection>::get` 47,828,417 (416,548); `VecDeque::grow` 24,550,602.

Why: for each class, a BFS over extends/implements builds three `HashMap<Name,Name>` (method/property/const -> owner) from empty, with `contains_key` then `insert` (`:377-390`), no capacity reservation. 3,064,239 inserts cause 598,581 rehash/grow calls, 53% of the insert cost. `mi_malloc_aligned` from these rehashes: 91,313,127 (598,583 calls). Drop of the same maps: `drop_glue<[(Name,Name)]>` self 113,963,893, `drop_glue<RawTable<(Name,Name)>>` self 71,176,708.

### 3.4 flatten_phpdoc_inheritance (1,506,682,303 = 7.30%)

Source: `resolve.rs:355`; one pass `:1097-1355` (2,077 calls = 1.82 passes per run). Callees: the par_iter helper 1,121,002,605 (5.43%), plus per-call map work: `HashSet<&str>::contains` 54,269,865 (567,066), `HashMap<&str,usize>::contains_key` 52,114,388 (552,602), `HashMap<&str,usize>::insert` 45,092,277 (188,015), `HashMap<&str,()>::insert` 42,758,871 (376,034), `HashMap<&str,usize>::get` 39,897,448 (349,989), class `get` 23,940,331 (188,017).

Leaf inside the helper: self of the helper closure 366,885,448 (merged over its recursion levels; 60.7 M of it in `resolve.rs` lines, the rest in hashbrown probing, `rustc-hash`, slice iterators: `find_map` over the parent owner maps and the owner-class lookup at `:1129-1139`, and the loop over every method of every class, `:1121-`). Its callees: `template_param_scopes_all_safe_to_backfill` 342,261,342 (1.66%; 256,627 + 27,489 calls), `Type::replace_templates` 148,443,637, `Type::reconcile_with_native` 122,609,187.

Inside `template_param_scopes_all_safe_to_backfill` (`resolve.rs:1014-1027`): the lazily built `ancestor_bindings` (`:1167-1169`, `Registry::ancestor_bindings` at `:983`) through `OnceLock::get_or_init`: 262,005,670 (1.27%, 49,355 initializations, 5.3 K Ir each), whose own callees are `Registry::class_template_bindings` 157,818,201 (50,793) and `all_ancestor_names` 70,076,394 (49,468). The `OnceLock` array is created once per `flatten_phpdoc_inheritance` call (`resolve.rs:362-363`) and shared by its passes, so this is at most one initialization per class per run (49,355 over 1,141 runs, about 43 per run).

### 3.5 flatten_traits (1,100,990,116 = 5.34%)

Source: `resolve.rs:105-113` (loop to a fixed point) and `:127-` (one pass). Callees: the par_iter helper 765,657,688 (3.71%, 1,629 runs), `HashMap<Name,FunctionReflection>::entry` 142,533,136 (454,630 calls; of which `reserve_rehash` 106,616,600 in 17,114 calls, with memcpy 47,325,710), two memcpy streams 60,298,855 (941,539) and 59,915,710 (936,394), `scoped_class_names` 54,842,570 (2,982 calls).

Leaves in the helper: `FunctionReflection::clone` 238,104,543 from the helper (`methods.push((name.clone(), sig.clone()))`, `resolve.rs:176`), helper self 158,043,279, `Vec<(Name,FunctionReflection)>::grow_one` 170,436,408 (44,189 calls, from the same `Vec::new()` in the helper). Most classes have no `uses` and return early; the cost is from clones of every trait method signature into each using class (the function's own doc comment says so).

### 3.6 drop_doc_members_excluded_by_native (840,780,195 = 4.08%)

Source: `resolve.rs:1696-1780`, `excluding_native` `:1782-1845`. Callees: helper 704,358,029 (3.41%), `scoped_classes` 56,220,796, property helper 63,045,402, function helper 28,563,012.

Leaf: `Registry::excluding_native` 539,200,183 (2.61%), 3,792,932 calls from the closure (484,178,999, about 128 Ir per call, self 211,198,385) plus 49,759,100 from the property helper. It takes `ty == native` first (`:1789`) and most calls return `None`. When it does not, `Relation::is_super_type_of` 148,478,823 (138,551 calls) runs, and `union_of`.

Why: every parameter and return type of every method in scope is checked once per run against its native type (3,792,932 checks, about 3,300 per run).

### 3.7 closure_scope (808,633,801 = 3.92%)

Source: `project.rs:1110-1179`, called twice per run (2,282 calls). Callees: the `reached`/`reach` closure (`:1133-1154`) 303,561,606 (667,546 calls; self 69,582,183 under merged name `closure_scope::{closure#1}::{closure#0}`): `HashMap<String,()>::insert` 144,471,220 (643,053 calls; of which `reserve_rehash` 101,299,593 in 122,491 calls), `HashSet<String>::contains` 66,248,061 (768,989), `mi_malloc_aligned` 28,834,543 (644,298, the `key.to_string()` at `:1143-1145`); and `StubFileSet::add_closure` -> `StubIndex::file_closure` (`stubs.rs:872-893`) 275,659,208 (1,983 initializations, 139 K Ir each): per stub file, a bitset closure over `dependents` with `Packed::get` 157,322,287 (722,896 calls). The `file_closure` is a per-process `OnceLock` per file, so it is paid once per stub file per process (inherent to the first touch, lazy by design).

### 3.8 precompute_ancestor_names (784,842,514 = 3.80%)

Source: `hierarchy.rs:122-`. Callees: helper 285,787,056 (1.39%; `class_extends_implements_closure`), `NameTable<HashSet<Name>>::insert` 180,880,834 (188,017 calls; its `forget` 126,348,411 with `Base::find` 59,426,956 and `HashMap<Folded,()>::insert` 36,616,696), `base_get` 69,217,530 (187,381), `scoped_removed_class_keys` 59,949,057, `scoped_class_names` 55,704,469, `unrecord_parents` 36,926,033 (92,979), class `get` 23,940,331.

### 3.9 scoped_classes / scoped_class_names (338,222,193 + 169,561,600 = 2.46% together)

Source: `scope.rs:226-234` (`scoped_classes`) and `:189-204` (`scoped_class_names`). `scoped_classes` is called by six passes, each about 56.2 M: `resolve_deferred_type_imports` 56,703,687, `inherit_property_hooks` 56,618,005, `resolve_constant_named_types` 56,235,668, `resolve_deferred_value_of` 56,227,351, `drop_doc_members_excluded_by_native` 56,220,796, `resolve_deferred_mixins` 56,216,686 (each 1,353 calls, about 41.5 K Ir per call). It builds a `Vec<(&Name,&ClassReflection)>` with one `get_with_name(&FoldedKey(key))` per scope key, which is the `Base::find` leaf (3.14): 446,808 finds, 73,424,964. `scoped_class_names` has a cache keyed by the scope and the class-table version (`scope.rs:189-191`); it still costs 55.7 M, 55.6 M and 54.8 M for `precompute_ancestor_names`, `resolve_transitive_purity` (1,353 calls each, about 41 K Ir per call) and `flatten_traits` (2,982 calls, about 18 K per call). Those per-call costs are far above a cache hit's, but I did not trace why the calls miss.

### 3.10 resolve_transitive_purity (438,770,988 = 2.13%) and resolve_constant_named_types (425,612,109 = 2.06%)

Purity (`purity.rs:81`): helper 165,242,952, a `FilterMap` over property writes (`resolve_property_writes`) 92,081,127 (189,486 calls), `scoped_class_names` 55,598,895. Constant-named types (`resolve.rs:1910-`): helper 319,856,458, which calls `fixed_sig` (`resolve.rs:1974-2014`) 302,253,177 inclusive: 1,860,999 memcpy calls 120,964,935 and 4,542,423 calls of the lookup closure 54,222,296; plus `scoped_classes` 56,235,668. `fixed_sig` calls the lookup on every parameter type, doc type, out type, closure-this type and the return/self-out types, and clones the whole signature (`sig.clone()`) the first time one of them changes. I did not determine which of the 1.86 M copies are those clones.

### 3.11 Lazy stub decode (`Once::call`, 2,006,999,545 = 9.73%, all contexts)

Components (callees of `futex::Once::call`):

| lazy init | Ir | % | calls | note |
|---|---|---|---|---|
| `OnceLock<Arc<Option<(u32,u32)>>>` = `Base::value` slot decode (`name_table.rs:453-465`) | 674,396,908 (685,564,560 with its own entry) | 3.27 | 58,041 | classes 546,940,116 (11,536), functions 115,664,070 (28,192) |
| `LazyLock<Registry>` | 313,592,279 | 1.52 | 3 | `stub_cache_round_trip::read_back` 298,621,274 (it calls `Registry::store`, inclusive 262,552,450), `phpstan_base` 10,158,305, `plain_base` 4,812,082 |
| `StubIndex::file_closure` | 276,823,231 | 1.34 | 1,998 | 3.7 |
| `ancestor_bindings` cell | 259,011,647 | 1.26 | 49,468 | 3.4 |
| `OnceLock<ClassReflection>` (generated stub classes) | 249,677,657 | 1.21 | 3,495 | |
| `LazyLock<Vec<String>>` (the fixture sources, read once) | 82,729,009 | 0.40 | 3 | |
| `GeneratedStubData` | 44,932,873 | 0.22 | 1 | |
| `OnceLock<FunctionReflection>` | 39,705,577 | 0.19 | 5,282 | |

(The loading itself is cheap: `Registry::load` inclusive 7,967,873 for 3 loads. The cost is in the decode on first read.)

`decode_entry::<ClassReflection>` 789,771,574 (3.83%; postcard, `name_table.rs`): `FunctionReflection::deserialize` 554,416,488 (84,552 signatures), under it `Repr::visit_enum` 233,510,242 (634,022), `Type::new` 119,659,419, `Vec<Type>` visitor 103,423,599. `Type::new` -> `types::intern::intern` 329,826,470 (1,096,493 calls) -> `intern::insert` 289,357,462 (self 216,885,733), `crates/types/src/intern.rs:158-197`: 1,010,469 of the 1,096,533 inserts end in `drop_glue<Repr>` (46,375,216), meaning the freshly decoded `Repr` found an existing node, i.e. a 92% hit rate, with a `Mutex` shard and a `Vec<Weak<Node>>` upgrade per call. `Interner::name` 234,032,008 inclusive (self 186,715,533, 67.6% of its callers are decode paths).

Where it runs: `Registry::warm` (`scope.rs:263-286`; 612 M above), the completion tests (`Server::completion` 152,907,593 for 6 calls: `name_table::Iter::next` 115,275,105 decoding 2,379 classes), `generated_stub_classes_named` 216,236,020 (10,333) under `rebuild_declarations`, `every_generated_stub_entry_decodes`.

### 3.12 check_program (2,358,943,529 = 11.44%, nearly all in the fixture test)

`Project::check_file` callers: fixture diagnostics closure 2,334,270,794 (17,438 calls), `update_file` 110,626,580 (743), the rest under 1 M. `check_program` callees: `check_function_impl` 1,614,622,572 (35,780 calls, 45 K Ir each; its own self 88,490,049), `own_declaration_sig` 146,691,984 (16,207), `check_stmt` closure 92,963,551, `Promotions` visitor 48,981,398 (43,429), `check_one_method_override` 33,881,506, the rest below 25 M each. Under `check_function_impl`: `check_block` 983,715,880 (36,130 calls, recursive), `always_terminates` 89,035,799, `format!` 46,486,235 (84,556), `check_phpdoc_types_against_native` 45,491,000. Under `check_stmt_impl` (987,769,167): `check_expr` 520,771,849 (30,279), `check_if` closure 200,631,053, nested statements 108,508,115. Under `check_expr_impl` (648,070,818): nested `check_expr` 267,999,129, `bind_assignment_target` 50,245,181, `check_args` 47,394,807. Under `infer_expr_unguarded` (293,163,230): `infer_builtin_call_return_type` 108,033,738, `chain_link_type` 54,903,710.

There is no single leaf in the checker that is 1.5% of the profile. Summed self over every function with `checker::` in its name (outside `registry::`) is 693,743,017 (3.36%), the largest being `check_function_impl` 88.5 M and `check_program` 54.7 M. The type engine is the larger part: `Relation::is_super_type_of` inclusive 460,499,562 (2.23%; callers: `excluding_native` 148,478,823, `union::absorb_contained_members` 52,068,003, `Type::concat` 31,407,416, `member_possible_given_native` 25,927,629), `Type::new` 437,128,754 (53.1% from decode paths, 46.9% from the rest), `Type::union` 160,914,771, `Type::replace_templates` 148,443,637.

### 3.13 Dropping projects (`drop_glue<Project>` 1,097,147,202 = 5.32%; `drop_glue<Registry>` 1,070,057,076)

Callers of `drop_glue<Project>`: the fixture closure 949,213,717 (375 drops), proptest `random_edits` 110,605,189, `editing_a_file_merged_into_a_built_in_class...` 15,862,941. `Registry` drop is 770 calls; `NameTable<ClassReflection>` 918,763,025 (2,311 drops), `ClassOwners` table 104,591,172, `FunctionReflection` table 23,634,223. `drop_glue<ClassReflection>` 1,131,629,136 in total: from the class tables 913,157,711 (167,347 drops, 5,457 Ir each), from the snapshot `Vec<Option<Detached<ClassReflection>>>` 188,114,138 (32,165). `drop_glue<FunctionReflection>` 811,839,397: 777,034,777 via `RawTable<(Name,FunctionReflection)>` (1,878,884 calls), with `mi_free` 237,381,742 (2,228,041) and `ParameterReflection` drop `mi_free` 143,873,704 (1,327,685).

### 3.14 Sinks: memcpy, malloc/free, Base::find, hashing

`memcpy` self 1,588,078,286 (7.70%). Callers (edge costs): the realloc path through an unsymbolized frame `???:0x54db1f8` (it only calls memcpy) reached from `_mi_theap_realloc_zero`, 568,595,605 (547,202 calls), `fixed_sig` 120,964,935 (1,860,999), `ClassReflection::clone` 109,815,887 (2,217,420), `flatten_traits` 60,298,855 + 59,915,710, `run_finishing_passes` 48,405,487 (431,779), `RawTable<(Name,FunctionReflection)>::reserve_rehash` 47,325,710, `reset_scoped` 43,065,085 (267,485), `NameTable<ClassReflection>::insert` 35,948,729, `HashMap<Folded,ClassReflection>::insert` 34,248,949. `finish_grow` callers: `Vec<Option<Detached<ClassReflection>>>::grow_one` 336,575,133 (4,507), `reserve::do_reserve_and_handle` 202,281,629 (1,446,770), `Vec<(Name,FunctionReflection)>::grow_one` 169,375,872 (44,189), `Vec<Type>::grow_one` 94,260,332 (1,084,405).

mimalloc self over all `mi_*`/`_mi_*` frames: 2,110,127,252 (10.23%). `mi_malloc_aligned` inclusive 1,200,495,283 (callers: `FunctionReflection::clone` 194,627,781 / 2,159,189; `finish_grow` 157,497,027 / 2,994,300; `ClassReflection::clone` 112,118,769 / 511,965; `reserve_rehash<(Name,Name)>` 91,313,127 / 598,583; `ParameterReflection::clone` 57,967,212 / 1,284,002). `mi_free` inclusive 801,785,548 (callers: `drop_glue<FunctionReflection>` 237,381,742 / 2,228,041; `ParameterReflection` 143,873,704 / 1,327,685; `RawTable<(Name,Name)>` 37,732,563). `mi_realloc_aligned` 672,840,181, 670,897,272 of it from `finish_grow` (548,660 calls).

`Base::find` (`name_table.rs:405-440`): inclusive 874,080,723 (4.24%), self 750,740,831 (3.64%): 362,308,272 in `name_table.rs` lines, 153,660,972 `uint_macros.rs`, 56,163,732 `rustc-hash`, plus `Keys::bytes` 125,051,127 (4,219,868 calls from `find_indexed` line 435). Callers: `NameTable<ClassReflection>::base_get` 336,078,083 (1,513,638), `scoped_classes` 73,424,964 (446,808), `NameTable<(Vec,Vec)>::base_get` 70,273,128, `NameTable<FunctionReflection>::base_get` 63,802,748, `NameTable<HashSet>::forget` 59,426,956, `reset_scoped` closure 59,174,298, `ClassOwners::forget` 59,174,298, `scoped_class_names` 36,712,482. About 250 Ir per find, for a hash of the key (`hash_key`, `name_table.rs:233`) and an 8-byte-at-a-time folded compare (`eq_folded`, `:243`). `HashMap<Folded,ClassReflection>::get::<str>` (resident table): 381,351,526 inclusive, self 327,828,243, 3,149,827 calls (`NameTable::get` 1,079,844 calls 133,434,975), memcmp 53,523,283 (2,908,838).

## 4. Comparison with the opt-1 profile (31.42 G -> 20.63 G, ratio 0.656)

### 4.1 Everything scales by about 0.65

Inclusive costs of the same functions (non-recursive entries, both profiles):

| function | opt 1 | opt 3 | opt3/opt1 |
|---|---|---|---|
| `run_finishing_passes` | 20,541,669,857 | 13,359,995,879 | 0.65 |
| `check_file` | 3,694,039,602 | 2,447,643,303 | 0.66 |
| `precompute_member_owners` | 2,471,481,187 | 1,633,391,435 | 0.66 |
| `flatten_phpdoc_inheritance` | 2,290,992,822 | 1,550,878,106 | 0.68 |
| `flatten_traits` | 1,826,995,851 | 1,134,507,164 | 0.62 |
| `drop_doc_members_excluded_by_native` | 1,326,390,326 | 863,541,102 | 0.65 |
| `closure_scope` | 1,348,489,911 | 817,804,980 | 0.61 |
| `precompute_ancestor_names` | 1,290,733,329 | 803,937,073 | 0.62 |
| `resolve_transitive_purity` | 763,643,528 | 447,825,169 | 0.59 |
| `resolve_constant_named_types` | 671,400,253 | 435,903,012 | 0.65 |
| `reset_scoped::<ClassReflection>` closure | 2,900,406,300 | 1,897,002,040 | 0.65 |
| `drop_glue<Project>` | 1,620,936,966 | 1,097,147,202 | 0.68 |
| `FunctionReflection::clone` | 1,616,556,783 | 1,098,406,554 | 0.68 |
| `template_param_scopes_all_safe_to_backfill` | 511,234,229 | 342,261,342 | 0.67 |
| `Type::new` | 544,530,169 | 437,128,754 | 0.80 |
| `Relation::is_super_type_of` | 553,061,430 | 460,499,562 | 0.83 |
| `intern::insert` | 389,697,439 | 289,357,462 | 0.74 |
| `lsp::tests::helpers::run_with_capabilities` | 1,187,044,552 | 648,619,565 | 0.55 |
| `Base::find` (sum of the typed variants in opt 1) | 1,083,889,651 | 874,080,723 | 0.81 |

The shape of the profile is the same: the passes and checker keep the same shares, so these shares are real and not an opt-1 artifact. Names that appear as 0 in one profile and large in the other (`Option<&ClassReflection> ==`: 29.6 M vs 1,299.8 M; `ClassReflection::clone`: 173.9 M vs 1,473.5 M; `decode_entry`: 389.4 M vs 789.8 M) are inlining/attribution changes (the work sits under a different frame), so I do not read them as changes in cost.

### 4.2 What shrank most (self cost grouped by source file of the executed lines)

| source | opt 1 | opt 3 | ratio |
|---|---|---|---|
| Rust core/alloc/std (iterators, ptr, slice, option, ...) | 14,974,711,617 (47.66%) | 7,885,610,855 (38.23%) | 0.53 |
| serde/postcard | 683,303,959 | 404,151,478 | 0.59 |
| hash tables (hashbrown/rustc-hash/indexmap/imbl) | 4,419,744,730 | 3,082,289,990 | 0.70 |
| `crates/types` | 1,251,873,683 | 996,001,703 | 0.80 |
| mimalloc | 2,746,612,346 | 2,227,716,789 | 0.81 |
| `crates/syntax` | 627,217,632 | 566,193,409 | 0.90 |
| `crates/phpstan-rs` | 3,999,423,509 | 3,413,030,831 | 0.85 |
| libc (memcpy/memcmp/...) | 1,976,287,014 | 1,951,721,409 | 0.99 |
| uncategorized | 646,450,582 | 43,650,047 | 0.07 |

(Category by the path of the file in each annotated entry; the "uncategorized" bucket is entries whose path did not match any of my patterns.) Frames that disappeared into callers at opt 3: `FxBuildHasher::hash_one` + `Hash::hash<FxHasher>` self 2,347,277,316 in opt 1 vs 47,191,589 in opt 3 (hashing is now in the self cost of the hash-table functions; I did not measure the hash instructions alone); `RawVecInner::try_allocate_in` 448,754,022 -> 0; `mi_theap_malloc_aligned_at` 439,448,379 -> 0; `RawVecInner::deallocate` 363,939,855 -> 0; `eq_folded` 383,063,498 -> 0 (inside `Base::find`); `types::Type ==` self 244,755,503 -> 31,793,457.

### 4.3 What barely changed (the real, non-artifact costs)

Self Ir of functions that opt 3 cannot make cheaper (libc, allocator work, per-element work already tight, memory-bound copies):

| function | opt 1 self | opt 3 self | ratio |
|---|---|---|---|
| `__memcpy_avx_unaligned_erms` | 1,604,575,346 | 1,588,078,286 | 0.99 |
| `__memcmp_avx2_movbe` | 311,973,998 | 303,719,801 | 0.97 |
| `core::str::converts::from_utf8` | 137,620,773 | 137,662,084 | 1.00 |
| `clone_vec<AssertInfo>` | 123,565,913 | 123,503,697 | 1.00 |
| `FunctionReflection::eq` | 246,268,704 | 249,916,375 | 1.01 |
| `HashMap<Name,Name>::insert` | 325,707,710 | 334,095,405 | 1.03 |
| `HashMap<Name,PropertyReflection> ==` | 90,445,144 | 88,348,893 | 0.98 |
| `Keys::bytes` | 130,548,870 | 125,051,127 | 0.96 |
| `drop_glue<[(Name,Name)]>` | 106,573,420 | 113,963,893 | 1.07 |
| `RawTable<(Name,Name)>::reserve_rehash` | 249,628,363 | 280,609,681 | 1.12 |
| `HashMap<Folded,ClassReflection>::get` | 295,749,792 | 327,828,243 | 1.11 |
| `Vec<Option<Detached<ClassReflection>>>::grow_one` (inclusive) | 327,866,730 | 336,683,301 | 1.03 |
| `HashMap<Name,FunctionReflection> ==` (inclusive) | 1,134,439,555 | 996,853,533 | 0.88 |
| `FunctionReflection::eq` (inclusive) | 899,793,113 | 873,815,677 | 0.97 |
| mimalloc (all `mi_*` frames) | 2,702,118,215 (8.60%) | 2,110,127,252 (10.23%) | 0.78 |

memcpy grew from 5.11% to 7.70% of the total and mimalloc from 8.60% to 10.23%, because the rest got cheaper and these did not. The costs that opt 3 does not move are copying (clones, reallocs, rehash moves), comparing, and allocation; none of them is an instruction-count problem that a compiler can remove. Only changing the amount of work (fewer clones, fewer inserts, caching) will lower them.

## 5. Ranked table of the 20 largest leaf costs

Ir and shares are from the sections above. Rows overlap in places (a leaf is also inside a larger branch; `Base::find` is inside the `scoped_classes` row, for example), so the column does not add to 100%. "Inherent" means that I see no cheaper form without changing what is computed.

| # | leaf | Ir | % | caller and why | idea or reason it is inherent |
|---|---|---|---|---|---|
| 1 | mimalloc `mi_*` frames (self) | 2,110,127,252 | 10.23 | 2.16 M mallocs from `FunctionReflection::clone`, 0.51 M from `ClassReflection::clone`, 3.0 M from `finish_grow`, frees from drop glue | fewer clones and drops (rows 3, 7, 8) remove the calls that feed it; I did not measure how much of the 10.23% they account for |
| 2 | `memcpy` (self) | 1,588,078,286 | 7.70 | realloc path 568.6 M (547,202), `fixed_sig` 121.0 M, `ClassReflection::clone` 109.8 M, `flatten_traits` 120.2 M | `with_capacity` for the vectors in rows 3, 7, 19; copy less in `fixed_sig` (3.10) |
| 3 | `ClassReflection::clone` in `reset_scoped` (`registry.rs:122-134`, `class_info.rs:347`) | 1,366,068,063 | 6.62 | 187,713 copies of the raw class over the resolved one before each scoped re-run | hold classes behind `Arc` and copy on write, or skip the reset for classes the passes do not change; keep `raw` and resolved sharing structure |
| 4 | owner maps (`own_owner_names`, `hierarchy.rs:355-396`) | 1,335,545,239 | 6.47 | 3,064,239 `HashMap<Name,Name>::insert` (719.6 M; rehash 385.2 M, 598,581 calls) from empty maps, plus a `contains_key` before every insert | reserve capacity from the parent maps' sizes; build the child map by cloning the parent map and adding own members; use `entry` once |
| 5 | `changed_since` compare (`registry.rs:606-625`) | 1,242,541,932 | 6.02 | 263,484 `ClassReflection ==` with 1,726,396 `FunctionReflection ==` (873.8 M) | compare a per-class change stamp, or compare only classes a pass wrote; pointer-equal `Arc`s short-circuit |
| 6 | `flatten_phpdoc_inheritance_one_pass` (`resolve.rs:1097-1355`) | 1,181,989,967 | 5.73 | helper self 366.9 M plus `template_param_scopes_all_safe_to_backfill` 342.3 M and `ancestor_bindings` 262.0 M (49,355 inits), 1.82 passes per run | the bindings are already built once per class per run; cache them across runs for classes whose ancestors did not change; skip methods whose params already all have docs before the owner lookup |
| 7 | `flatten_traits_one_pass` (`resolve.rs:127-`, `:176`) | 1,134,507,164 | 5.50 | `FunctionReflection::clone` 238.1 M, `Vec<(Name,FunctionReflection)>::grow_one` 170.4 M (44,189), `entry` 142.5 M | `with_capacity`; share `Arc` signatures instead of cloning trait methods |
| 8 | drop of the project (`drop_glue<Registry>` -> class table) | 1,070,057,076 | 5.19 | 770 drops, 167,347 `ClassReflection` drops at 5,457 Ir; 949.2 M in the fixture test | `Arc`-shared entries make the drop a counter decrement; in tests, leak the project (`mem::forget`) where the test does not need the memory back |
| 9 | `Base::find` (`name_table.rs:405-440`) | 874,080,723 | 4.24 | 3.5 M finds, about 250 Ir each: `hash_key` plus folded compare | cache the `scoped_classes` vector (row 18) so passes stop re-finding; inherent per lookup |
| 10 | `excluding_native` (`resolve.rs:1782-1845`) | 539,200,183 | 2.61 | 3,792,932 calls, about 128 Ir each; the function's first test (`ty == native`) is the early exit | hoist the test or use the params' `has_own_doc` flags so unchanged signatures are skipped; `is_super_type_of` (148.5 M) runs only on the rest |
| 11 | `closure_scope` (`project.rs:1110-1179`) incl. `file_closure` (`stubs.rs:872-893`) | 808,633,801 | 3.92 | 667,546 `reach` calls, 643,053 `HashSet<String>::insert` (144.5 M, rehash 101.3 M), 1,983 first-touch closures (275.7 M) | `with_capacity` and `Cow`/borrowed keys; the stub closure is a once-per-process cost |
| 12 | `precompute_ancestor_names` (`hierarchy.rs:122-`) | 784,842,514 | 3.80 | `NameTable<HashSet>::insert` 180.9 M (188,017), `forget` 126.3 M, helper 285.8 M | skip classes whose parents are recorded already before building the set; avoid the `forget` + `insert` double lookup |
| 13 | stub decode (`decode_entry<ClassReflection>`, `name_table.rs`, postcard) | 789,771,574 | 3.83 | 84,552 `FunctionReflection::deserialize` (554.4 M) with 1.1 M `Type::new`/`intern` | decode param/return types lazily; intern on a thread-local pre-hash; inherent per first touch |
| 14 | `Relation::is_super_type_of` | 460,499,562 | 2.23 | `excluding_native` 148.5 M, `absorb_contained_members` 52.1 M, `concat` 31.4 M | memoize pairs per pass; mostly checker/type work, inherent |
| 15 | `resolve_transitive_purity` (`purity.rs:81`) | 447,825,169 | 2.17 | helper 165.2 M, property-write filter 92.1 M (189,486), `scoped_class_names` 55.6 M | none found beyond row 18 |
| 16 | `resolve_constant_named_types` -> `fixed_sig` (`resolve.rs:1910-2014`) | 435,903,012 | 2.11 | 1,860,999 memcpy (121.0 M), 4,542,423 lookups | find which of the 1.86 M copies are not the single `sig.clone()` per changed signature and avoid them |
| 17 | `template_param_scopes_all_safe_to_backfill` (`resolve.rs:1014-1027`) | 342,261,342 | 1.66 | 256,627 + 27,489 calls, `ancestor_bindings` init 262.0 M | same as row 6 |
| 18 | `scoped_classes` x 6 passes (`scope.rs:226-234`) + `scoped_class_names` | 338,222,193 + 169,561,600 | 1.64 + 0.82 | 8,118 calls at about 41.5 K Ir, and 5,688 calls from the three costly `scoped_class_names` callers; one `Base::find` per scope key per `scoped_classes` call | cache the vector across passes like the name list, keyed by scope and table version |
| 19 | `Vec<Option<Detached<ClassReflection>>>::grow_one` (`registry.rs:129-138`) | 336,683,301 | 1.63 | 4,507 growths, 74.7 K Ir each, via `mi_realloc_aligned` + memcpy | `Vec::with_capacity(snapshot_keys.len())` |
| 20 | `types::intern::insert` (`intern.rs:158-197`) | 289,357,462 | 1.40 | 1,096,533 calls, 92% hits (`drop_glue<Repr>` 46.4 M); shard `Mutex` + `Weak` upgrade | hash before building the `Repr`; lock-free read path; inherent otherwise |

Other leaf-sized items below the cut: lsp `run_with_capabilities` 648,619,565 (3.14%; `decode_all` 299,992,964 of it, `Message::_read` 213,641,938 over 1,033 messages, `run_loop` 327,665,643, `Server::completion` 152,907,593 for 6 calls); `Registry::store` inclusive 262,552,450 (1.27%, one call in `stub_cache_round_trip::read_back`; self 20,397); `update_file` 622,422,121 (3.02%; the fixture test 359,587,594, `phpstans_own_classes_are_loaded_only_while_a_file_names_them` 187,123,479); `rebuild_declarations` 464,416,399 (2.25%; `generated_stub_classes_named` 216,236,020 in 10,333 calls); `parse_file` 343,776,669 (1.67%).

## 6. What I could not establish

- The cost of one finishing-pass run per test (only totals, run counts and per-run averages for the shared pool; nothing for the 472 cold runs per test).
- The 0.26 G (6.8%) gap between the sum of call edges and the job total in 1.2.
- Why `scoped_class_names` costs about 41 K Ir per call despite its cache (3.9); I did not look at the cache's key use.
- The hash cost alone at opt 3 (inlined into the hash-table functions).
- Which size `Option<Detached<ClassReflection>>` has (so the per-growth 74.7 K Ir is a measured average, not an explained size).
- Per-line costs inside the rayon helper closures: their annotated lines are spread over inlined library code (for example the flatten_phpdoc helper's 366.9 M self splits over 24 file entries, the largest 60.7 M in `resolve.rs`).
