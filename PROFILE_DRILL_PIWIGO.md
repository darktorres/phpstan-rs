# Callgrind drill-down: scoped Piwigo corpus run (gate binary)

Command profiled (from the callgrind header): `target/gate/phpstan-rs check-project --config phpstan-rs.toml --format json`. Total 45,129,557,009 Ir (45.13 G).

## 0. Method and caveats

- Files: `/home/torres/.cache/prs-profile/cl-piwigo/cg.out-01 .. cg.out-09`. All nine threads were read (main = 01, workers = 02..09), not just the largest, because the per-thread numbers had to be summed.
- Commands (run once per thread, output in `<scratch>/pw-drill/`):
  - `nice -n 19 callgrind_annotate --inclusive=yes --tree=none cg.out-NN` -> `inc-NN.txt`
  - `... --inclusive=no --tree=none` -> `self-NN.txt`
  - `... --inclusive=yes --tree=calling` -> `calling-NN.txt`, and `--tree=caller` -> `caller-NN.txt`
- Small helper scripts I wrote to sum across threads: `<scratch>/pw-drill/agg.py` (sum of self or inclusive per function), `ac.py` (callees of a function summed over threads), `acl.py` (callers), `cyc.py` (callees of a whole recursive family), `t.py` (one thread's tree block).
- Percentages are of 45,129,557,009. Per-thread totals: th01 2.630 G, th02 8.494 G, th03 6.050 G, th04 5.792 G, th05 3.775 G, th06 4.615 G, th07 3.416 G, th08 4.755 G, th09 5.602 G.
- Self-cost sums are exact for the functions callgrind_annotate lists (it cuts the tail at 99%: the listed self costs sum to 44.68 G of 45.13 G).
- Inclusive numbers: exact for non-recursive functions. Names ending in `'2` and the checker/parser families are recursive, so their inclusive numbers are upper bounds and are marked "(UB)". The `--tree=calling/caller` files also only list functions above the threshold in each thread, so a cross-thread sum of a small function's callers/callees is a lower bound; where I rely on such a sum I say so.
- "Ir per call" figures are my own division of two tool numbers and are marked "(derived)".
- No source was edited, nothing was rebuilt or rerun. Source claims below are from reading the file at the quoted line.

## 1. Entry points and phases

Threads: th01 is `main` (`phpstan_rs::main` -> `check_project::run_check_project_command`, 2,627 M = 99.9% of that thread). Threads th02..th09 are rayon workers (`thread_start` -> `rayon_core::registry::WorkerThread::wait_until_cold`); together 42,499,949,211 Ir = 94.17% of the total.

| Phase | Where | Ir | % of 45.13 G | Basis |
|---|---|---|---|---|
| Main thread, everything | th01 | 2,629.6 M | 5.83% | PROGRAM TOTALS of cg.out-01 |
| Parse + per-file declaration indexing (workers) | `Session::read_and_parse_file` (session.rs:570), sum th02..09 | 21,558.5 M | 47.77% | exact (non-recursive, 14,251 calls) |
| Checking files (workers) | `Project::run_checker`, sum th02..09 | 16,870.0 M | 37.38% | exact; 2,792 `check_program` calls |
| Finishing passes (th02 root) | `Project::run_finishing_passes` | 3,133.9 M | 6.94% | exact, th02 only |
| Not under those three roots (workers) | 42,499.9 - (21,558.5 + 16,870.0 + 3,133.9) | about 937 M | about 2.1% | by subtraction; spread over th03..th09 (65 M, 277 M, 59 M, 190 M, 133 M, 148 M, 60 M). I did not identify these jobs individually; the `wait_until_cold` callee list in th02 contains other rayon `call_b` jobs (e.g. a 535 M one), consistent with parallel finishing-pass jobs, but that is not verified. |
| Stub cache | no separate phase | decode of 3,399 stub classes: 308.4 M | 0.68% | `stub_cache::decode<ClassReflection>`, called from `name_table::load_base::{closure#1}` (3,399x). It is a lazy decode inside checking, not a loading phase. |
| Output | `output::render_json` (main) | 23.9 M | 0.05% | |

Per-thread split of the three worker roots (Ir):

| thread | total | parse | check | finishing |
|---|---|---|---|---|
| th02 | 8,494 M | 2,946 M | 2,414 M | 3,134 M |
| th03 | 6,050 M | 1,556 M | 4,429 M | - |
| th04 | 5,792 M | 4,363 M | 1,151 M | - |
| th05 | 3,775 M | 1,642 M | 2,074 M | - |
| th06 | 4,615 M | 1,734 M | 2,689 M | - |
| th07 | 3,416 M | 2,459 M | 823 M | - |
| th08 | 4,755 M | 3,668 M | 938 M | - |
| th09 | 5,602 M | 3,191 M | 2,351 M | - |

th04's parse is 4.36 G; 2.52 G of that is one leaf family (section 3.2, Shape::set), which is why it parses much more than th05/th06.

## 2. Main thread (th01, 2,629.6 M)

`run_check_project_command` 2,627.1 M -> `Session::run` 2,089.5 M (79.5% of main) + `app::check_project::prepare` 508.3 M + `render_json` 23.9 M.

### 2.1 `Session::load` 1,802.9 M (68.6% of main, 4.0% of total)

| callee | Ir | calls |
|---|---|---|
| `Project::insert_parsed_file` (project.rs:843) | 1,499.2 M | 14,251 |
| `driftsort_main<PathBuf>` (the `sort()` calls at session.rs:600 and :606) | 157.4 M | 2 |
| `Session::collect_php_files` | 69.9 M | 305 |
| `driftsort_main<PathBuf, sort_by_key<Reverse<usize>>>` (session.rs:688) | 41.5 M | 2 |
| `Once::call` | 15.2 M | 2 |
| `Vec<PathBuf>::dedup` | 8.8 M | 1 |

`insert_parsed_file` (serial, called once per file after the parallel parse) 1,499.2 M:

| callee | Ir | calls |
|---|---|---|
| `project::register_references` (project.rs:2808) | 976.8 M | 28,502 |
| `composer::PackageIndex::owner` (composer/src/lib.rs:144) | 314.8 M | 14,251 |
| `Registry::insert_built_decl` | 61.6 M | 15,266 |
| `Project::absolute_path` | 53.0 M | 14,251 |
| `HashMap<PathBuf, FileState>::insert` | 31.9 M | 14,251 |
| `HashMap<PathBuf, ()>::insert` | 28.9 M | 14,251 |
| `DuplicateClassSites::merge` | 11.5 M | 14,251 |

Leaf 2.1a: `register_references` (read at project.rs:2808-2819): for each name in the file's reference set it does `dependents.entry(name.clone()).or_default().insert(path.to_path_buf())`. Its callees: `HashMap<PathBuf,()>::insert` 810.0 M (404,459 calls), `HashMap<String, HashSet<PathBuf>>::rustc_entry` 63.3 M, `Path::to_path_buf` 48.0 M, `String::clone` 38.4 M. The 810 M is almost entirely `<Path as Hash>::hash::<FxHasher>`: self 976,606,090 Ir on th01 (37.1% of main). Derived: 810.0 M / 404,459 = about 2,000 Ir per insert. `Path::hash` hashes component by component (it showed `std::path::Components::next` 180.8 M and `compare_components` 181.1 M as the next self costs on main).

Leaf 2.1b: `PackageIndex::owner` 678.8 M if summed over `owner` closures, but the direct figure under `insert_parsed_file` is 314.8 M for 14,251 calls (derived 22.1 K Ir per file). Callees (th01): `Ancestors`-chain `Map<Flatten<Chain<FilterMap<Ancestors...>>>>` 182.7 M, `Path::hash` 138.7 M (297,090 calls, lower bound), `Path::parent` 96.0 M (297,090 calls), `lexically_normalized` 42.7 M. Code read: it normalizes the file path, then walks `file.ancestors()` and `normalized.ancestors()` and does a hash lookup in `by_dir`/`by_normalized` for every ancestor (lib.rs:144-160).

Leaf 2.1c: sorting `Vec<PathBuf>`: `quicksort<PathBuf, lt>` 186.9 M + its `'2` instances (422.6 M inclusive recursive, UB); self cost is `std::path::compare_components` 181.1 M on th01. Sort by size (session.rs:688) re-compares via `sort_by_key` and re-hashes paths for the size lookup (14.2 M `Path::hash` under the closure).

### 2.2 `prepare` 508.3 M (19.3% of main)

`Session::resolve` 508.27 M -> `PackageContext::load` 489.5 M -> `composer::vendor::discover_vendor_context` 462.1 M:
- `parse_generated_file` 236.8 M, which is one `syntax::parse::parse` of a generated PHP file (236.8 M, 1 call).
- `Resolver::path` 112.1 M (12,399 calls; derived 9.0 K Ir per call).
- `composer::vendor::dedup` 98.7 M (1 call).

### 2.3 `Session::analyse` 286.6 M
`Project::check_files` 169.2 M (of which `check_phpstan_api_usage` 122.1 M for 2,792 files, a full AST walk per analysed file using `Walker`) and `check_class_shapes` 91.8 M. Both are serial on main.

## 3. Workers: parse phase (21,558 M, 47.8% of total)

`read_and_parse_file` (session.rs:570-581) -> `parse_file` (project.rs:2610) 21,096.6 M (46.75%) + `String::from_utf8_lossy(&bytes).into_owned()` 394.3 M (14,251 calls, 0.87%).

`parse_file` callees (sum th02..09, 14,251 calls each):

| callee | Ir | % total |
|---|---|---|
| `parse_with_ignores` (project.rs:2538) | 11,544.7 M | 25.58% |
| `registry::index::build_declarations_where` | 6,001.3 M | 13.30% |
| `deps::referenced_names` | 1,380.8 M | 3.06% |
| `deps::declaration_referenced_names` | 1,020.0 M | 2.26% |
| `DuplicateClassSites::index` | 255.3 M | 0.57% |
| `project::declared_names` | 253.6 M | 0.56% |
| `project::declared_constants` | 249.6 M | 0.55% |
| `DuplicateFunctionSites::index` | 247.0 M | 0.55% |
| `FileBodies::of` walk | 137.7 M | 0.31% |

### 3.1 `parse_with_ignores` 11,544.7 M -> `syntax::parse::parse` 10,883.9 M (the remaining: `ignore_comments::scan_parsed` 370.4 M, `name_anonymous_classes` 139.9 M, `mentions_stub_attributes::Find` walk 126.9 M)

`syntax::parse::parse` (crates/syntax/src/parse.rs:37), sum of 14,252 calls = 11,120.7 M:

| callee | Ir | % total |
|---|---|---|
| `Lexer::tokenize_with_comments` -> `Lexer::run` | 4,694.9 M | 10.40% |
| `Parser::parse_program` | 4,408.1 M | 9.77% |
| `Parser::new` | 791.2 M | 1.75% |
| `syntax::annotations::annotate` | 463.5 M | 1.03% |
| `Lexer::from_bytes` (`compute_line_starts`) | 452.8 M | 1.00% |
| `drop_glue<Parser>` | 114.4 M | 0.25% |

**Lexer** `Lexer::run` (lexer.rs:304) 4,624.0 M: `next_token` 3,228.4 M (6,891,547 calls) + `line_col_at` 1,058.3 M (13,783,094 calls = two per token, see lexer.rs:310-312) + own 327 M.
- `next_token` (lexer.rs:362) self 1,281.6 M. Its callees (th sum): `Interner::name` 342.5 M (1,757,875), `Lexer::starts_with` 290.2 M (7,590,457; each is `src[pos..].starts_with(bytes)`, lexer.rs:351, a `memcmp` call: 7.95 M memcmp calls from it per the caller tree, 177 M), `Keyword::try_from_bytes` 250.7 M (1,727,119), `lex_doc_comment` 231.8 M (86,911), `String::from_utf8_lossy` 209.0 M (1,773,218; lexer.rs:456 for every identifier), `lex_single_quoted` 195.4 M (335,704), `lex_operator` 180.1 M, `lex_interpolated_run` 142.8 M.
- `line_col_at` (lexer.rs:182) self 775.4 M: derived 56 Ir per call. It runs twice per token: once for `last_token_start` and once for `pos`.
- `compute_line_starts` (lexer.rs:95) self 416.7 M: byte-by-byte loop `for (i,&b) in src.iter().enumerate() { if b == b'\n' { starts.push(i+1) } }`.
- `from_utf8_lossy` -> `Utf8Chunks::next` self 698.1 M (all threads); 542.6 M of it via `from_utf8_lossy` (1,715,685 calls). Callers of `from_utf8_lossy` (inclusive, caller tree): `read_and_parse_file` 394.3 M, `next_token` 207.7 M, `lex_doc_comment` 82.0 M.

**Parser**: `parse_program` 4,408.1 M. Not-recursive callee costs (summed over every Parser function, th sum): `memmove/memcpy` 727.7 M (21,549,367 calls; derived 34 Ir per call), `name_resolution::Resolver::visit_stmt` 475.9 M (110,055 calls; the post-parse name resolution walk), `<&Token as PartialEq>::eq` 159.5 M (21,298,025 calls), `RawVec<Stmt>::grow_one` 109.9 M, `__rust_alloc` 96.4 M, `RawVec<ArrayItem>::grow_one` 93.6 M, `RawVec<Arg>::grow_one` 49.1 M. The rest is Parser self cost spread across the 17-level precedence chain: each level's self cost is 0.08-0.18 G (e.g. `parse_unary_inner'2` 180.7 M, `parse_not'2` 133.7 M, `parse_postfix_from_primary'2` 125.9 M, `parse_binary'2` 121.5 M, `parse_ternary'2` 104.4 M, `parse_low_precedence_and'2` 93.5 M).
- `Parser::new` (parser.rs:95) self 717.8 M (derived 50 K Ir per file): it moves every `(Token, Span, Option<String>)` into three parallel `Vec`s and fills four `FxHashMap`s for doc comments.

### 3.2 `build_declarations_where` 6,001.3 M (13.3% of total)

Its worker is `ClassReflection::from_def` 5,346.6 M (12,407 calls), reached through `Declarations<..>::found`.
`from_def` callees (all threads): class constants `IndexMap<Name, ConstantReflection>::from_iter` 3,194.7 M, `FunctionReflection::from_decl` 1,081.1 M (60,854 calls), `docblock::parse_shared` 617.9 M (40,483 calls), property map 171.5 M, `declared_type` 32.2 M.

**Leaf: `types::shape::Shape::set`, self 2,663,990,435 Ir (5.90% of total, the largest single self cost).**
- Caller: `phpstan_rs::registry::function_sig::infer_const_type_with::{closure#0}` 2,783.6 M inclusive (63,104 calls), reached from `ClassReflection::from_def::{closure#32}` over `ClassConst` (3,165.0 M inclusive, 5,012 calls). Concentration: that closure costs 2,519.2 M inclusive on th04 (43.5% of th04), 382.3 M on th09, 134.8 M on th02, 131.0 M on th07, and is below the tree threshold on the others.
- Code read: function_sig.rs:578-616, the `ExprKind::ArrayLiteral` arm builds a `Shape` item by item with `shape.set(ShapeField::new(key, value_ty, false))`. `Shape::set` (types/src/shape.rs:272) does `self.fields.iter_mut().find(|f| f.key == field.key)`, a linear scan of all fields so far, so a constant array of N literal keys costs O(N^2) key comparisons. The scan's key compares appear as `memcmp` called 6,423,717 times from `Shape::set`. The same function also has `values.contains(&value_ty)` (function_sig.rs:585), a second linear scan; `Type::eq` called from this closure costs 200.3 M (10,545,858 calls).
- Derived: 2,664 M / 63,104 = about 42 K Ir per `set` call on average (the cost is dominated by a few very large literal arrays).
- Idea: build the shape with a side `FxHashMap<ShapeKey, usize>` index (or sort-and-dedupe once at the end, keeping last-wins), and replace `values.contains` by an `FxHashSet<Type>` (types are interned, so hashing by pointer should be cheap). This turns O(N^2) to O(N). Upper bound on gain if both quadratic parts disappeared: about 2.66 G + 0.20 G (self costs) of 45.13 G.

`FunctionReflection::from_decl` 1,081.1 M (all-thread inclusive 2,022.5 M includes other callers): `Vec<ParameterReflection>` collect 532.6 M (63,296 calls), `declared_type` 393.1 M (123,465), `docblock::parse_shared` 158.5 M (32,711), `types::parse::parse_written_type` 148.5 M (31,737), `Type::replace_objects` 76.6 M.

`docblock::parse_shared` -> `Memo<DocBlock>::get<parse>` 1,047.2 M inclusive, 94,228 gets, 39,718 real `docblock::parse` calls (927.9 M) of which `apply_tag_recording_errors` 640.2 M (60,488 calls).

### 3.3 Per-file extra AST passes inside `parse_file`

Seven or more separate full walks of the same AST run per file: `referenced_names` 1,380.8 M, `declaration_referenced_names` 1,020.0 M (both `deps::Collector`, 2,386.2 M inclusive for `Collector::visit_stmt`), `DuplicateClassSites::index`, `declared_names`, `declared_constants`, `DuplicateFunctionSites::index` (each about 0.25 G; each shows `walk_expr'2` self of 82-85 M), `FileBodies::of`, `annotate` 463.5 M, `mentions_stub_attributes` 126.9 M, `Resolver` 475.9 M. Each walk that tracks name context rebuilds the `use` map: `NameContext::add_uses` (270.2 M inclusive in the tree sums, a lower bound; 1,247,604 `__rust_alloc` calls from it).

`deps::Collector::doc_names` 1,504.3 M (3.33%): `doc_tags` closure 595.2 M (1,241,107 calls), `StrSearcher::new` 194.3 M (1,079,621), `Lines::next` 170.5 M, `NameContext::resolve` 341.9 M (345,941), `Vec<&str>` flatmap 235.6 M. Code read at deps.rs:99-112: per line it runs `.trim_start().trim_start_matches("/**").trim_start_matches('*').trim_start()`; the `"/**"` str pattern builds a `StrSearcher` per line (`StrSearcher::new` self 409.5 M across all callers). Of 235.3 M inclusive attributed to `StrSearcher::new` in the caller tree, 107.5 M comes from `doc_tags::{closure#0}` (597,229 calls), 59.9 M from `ignore_comments::scan_parsed`, 40.6 M from `mark_each_marker`.

## 4. Workers: finishing passes (th02, 3,133.9 M, 6.94%)

Callees of `run_finishing_passes` (th02): `Registry::resolve_transitive_purity` 721.7 M, `Project::closure_scope` 672.9 M (2 calls), `Registry::infer_global_variables_of` 399.2 M, `Registry::warm` 192.2 M, `end_declaration_changes` 172.9 M, `flatten_phpdoc_inheritance` 140.8 M, `Cloned<Flatten<..HashSet<PathBuf>>>` 138.8 M (16,599x), `precompute_member_owners` 98.3 M, `precompute_ancestor_names` 94.6 M, `changed_since` 68.4 M, `drop_doc_members_excluded_by_native` 67.8 M, plus smaller ones.

- `closure_scope` (project.rs:1110): the fixpoint walk over the dependents graph. Its callees: `OnceLock<Box<[u64]>>::initialize` 305.3 M (the `StubIndex` file-closure bitset built once per stub file: 2,053 inits, derived 149 K Ir each), `Path::hash` 158.5 M (197,539 calls, from `self.files.get(file)` inside the loop), `closure_scope::{closure#1}::{closure#0}` 51.3 M, `PathBuf::eq` 21.1 M.
- `resolve_transitive_purity` 721.7 M: `Type::mentions_any` 230.1 M (230,680 calls), two rayon `ParallelExtend` branches 134.6 M and 70.8 M, `hash_one` of `(Option<String>, String)` keys 39.6 M (584,230).
- `Path::hash` under `run_finishing_passes` directly: 67.6 M (83,471 calls).

## 5. Workers: check phase (16,870 M, 37.4%)

`run_checker` -> `Checker::check_program` 16,843.9 M (2,792 calls): `check_function` 10,358.0 M (10,312 calls) + `check_stmt` 5,694.8 M (10,026), then `check_one_method_override` 105.4 M, `scan_program_for_rebind_targets` 95.7 M, `Promotions::visit_stmt` 89.4 M.

The checker is a mutually recursive family (`check_stmt_impl`, `check_block`, `check_expr_impl`, `infer_expr_unguarded`, `cached<..>`), so inclusive numbers below are all (UB) and overlap: `check_stmt_impl` 14,386 M, `check_block` 13,534 M, `check_expr` 9,867 M, `cached<infer_expr>` 8,934 M (593,552 + 586,438 `infer_expr_unguarded` runs, 701,761 cache inserts), `check_closure_literal_with_this_and_scope` 4,172 M, `converge_loop_body<check_expr_impl::{closure#76}>` 3,621 M, `converge_loop_body<check_foreach::{closure#4}>` 2,614 M, `check_if::{closure#0}` 3,676 M, `chain_link_type_uncached` 2,835 M, `method_call_return_type` 2,554 M, `declared_method_return_type` 2,342 M, `infer_call_return_type` 1,930 M, `call_template_bindings` 1,595 M, `infer_binary` 1,566 M.

Because of recursion I walked down by listing everything the checker family calls that is outside the family (`cyc.py`, sums of direct callee edges), then by self cost. The largest direct edges out of the checker family:

| callee | Ir | calls | % total |
|---|---|---|---|
| `types::Type::concat` (concat.rs:20) | 1,236.6 M | 46,524 | 2.74% |
| `Registry::repository_entity` (via `doctrine_query_builder_source`) | 682.8 M | 695 | 1.51% |
| `Type::array_value_type` | 535.3 M | 9,476 | 1.19% |
| `Relation::infer_template_bindings` | 482.1 M | 362,923 | 1.07% |
| `alloc::fmt::format::format_inner` | 476.0 M | 802,259 | 1.05% |
| `Type::array_offset_read` | 277.0 M | 28,116 | 0.61% |
| `Type::is_array` | 194.9 M | 33,285 | 0.43% |
| `substitute_templates_resolving_to_bounds` | 193.8 M | 321,770 | 0.43% |
| `Registry::lookup_method_with_owner` | 183.2 M | 293,248 | 0.41% |
| `docblock::parse_shared` | 160.7 M | 7,939 | 0.36% |

(The sum of these is far below 16.9 G: most check-phase cost is spread in self cost of the checker and in `types`/`registry` calls reached through closures and inlined helpers; the leaves are listed in section 7.)

### 5.1 `Type::concat` -> `is_super_type_of` (1,427.6 M total inclusive, 3.16%)
`concat` callees: `Relation::is_super_type_of` 1,275.3 M (456,975 calls), `to_string_type` 35.6 M, `AccessoryKind` fold 26.5 M, `Type::new` 21.3 M. Code read (concat.rs:20-110): after the literal shortcuts it calls `is_refined_string(kind).yes()` for NonEmpty, NonFalsy, Literal, Lowercase, Uppercase on both operands; `is_refined_string` (types/src/queries.rs:735) is `self.is_within(refined_string_probe(kind))`. Derived: 456,975 / 46,524 = about 9.8 `is_super_type_of` calls per `concat`, 2.8 K Ir each. Idea: answer `is_refined_string` for the plain `Repr::String` / `StringLiteral` / `Intersection` shapes by looking at the `Repr` directly (as `with_string_accessory` already matches on `Repr`) and fall back to `is_super_type_of` only for other shapes; or memoize on the two interned nodes.

### 5.2 `is_super_type_of` (all-thread inclusive 8,774 M as callee sum, UB because of recursion; direct self: `is_super_type_of'2` 1,258.2 M + `is_super_type_of` 540.4 M)
Entry (types/src/relation/super_type.rs:38-57) has scalar shortcuts, then `is_super_type_of_by_form` (line 59, wrapped in `syntax::stack::guard`) with a long `match`. I found no cache at entry (read lines 38-140). Callers by cost: `concat` 1,275 M, the union/intersection member map `Vec<TrinaryLogic>::from_iter` 1,097.6 M (2,091,173 calls, 1,041,249 `__rust_alloc` calls from it: it collects member results into a `Vec` before folding), `absorb_contained_members` 366.5 M (213,196), `Type::is_array` 357.5 M (120,062), `member_offset_read` 300.0 M, `infer_template_bindings::{closure#0}` 282.7 M, `shape_super_type_of` 236.0 M.
Idea: fold with `extreme_identity` straight over the iterator (no `Vec<TrinaryLogic>`), and a small (sup,sub) -> result cache keyed on interned node pointers, since the types are hash-consed.

### 5.3 `repository_entity` 682.8 M (1.51%, 695 calls)
Caller `Checker::doctrine_query_builder_source` (checker/expr/member_resolution.rs:1115). Callee: `name_table::Iter<ClassReflection>::next` 601.9 M (10,243,401 steps; derived 14.7 K classes scanned per call). Code read (registry/hierarchy.rs:1221-1228): `self.classes.iter().filter_map(.. info.doctrine_entity ..)`, a linear scan of the whole class table; its own comment says "a real project has at most a few dozen entities". Idea: build a `repository_fqn -> [entity]` map once (after finishing passes), or a `OnceLock` list of the classes that have `doctrine_entity`.

### 5.4 `record_branch_implied_facts` 729.3 M (UB-free, 1.62%)
narrowing.rs:426. Callees: `format_inner` 305.9 M (569,167 calls), `Type::is_null` 106.3 M, `is_real_variable_key` 73.2 M, imbl HAMT `Iter::next` 46.1 M, `any_expr_in_scope` search 45.4 M. Code read at narrowing.rs:455: `let truthy = format!("truthy({name})");` runs for every local in `after.iter()` on every call, though it is used only inside the following `if` after three cheaper tests. Idea: compare lazily (`k.strip_prefix("truthy(")` against `name`), or compute `truthy` only when `is_flag_proxy || ...` is true.

### 5.5 `always_terminates::{closure#0}` 2,875.8 M (UB; 6.4%)
control_flow.rs:93: for a `StmtKind::Expression` it runs `self.infer_expr(e, locals).is_never()`. Its calls into `cached<infer_expr>` cost 2,586.4 M inclusive (165,043 calls). I cannot tell from the profile how much of this is first-time inference that `check_expr` would otherwise pay (the cache is an `InferKey` map, 701,761 inserts) versus repeated work; not determined.

### 5.6 Other check-phase leaves seen
- `purge_stale_narrowing_for` 465.5 M inclusive (UB-free): its closure calls `<&str as Pattern>::is_contained_in` 139.6 M (1,354,133 calls, a substring search for every local per assignment).
- `Type::array_value_type` 520.9 M inclusive (9,476 calls), its closure 526.5 M, `Chain<Map<ShapeField..>>` 525.9 M: derived 55 K Ir per call, a union over all fields of large array shapes.
- `types::intern::insert` see section 7.
- `merge_scopes` 370.2 M (control_flow), `sql_reflection::resolve_sql_row_type` 84.8 M inclusive (211 M if the cyc sum is used, 1,224 calls).
- `var_tag_nodes` (phpdoc_unresolvable) 116.5 M inclusive.

## 6. Cross-cutting: allocator, formatting, hashing

- Allocation: `mi_malloc_aligned` inclusive 2,008.2 M, reached from `__rust_alloc` 1,386.3 M (22,351,790 calls) and `RawVecInner::finish_grow` 133.0 M (2,465,183). Top `__rust_alloc` callers: `finish_grow` 405.8 M (5,006,456), `String::clone` 164.0 M (3,657,408), `intern::insert` 72.1 M (605,130), `NameContext::add_uses` 57.8 M (1,247,604), `Vec<TrinaryLogic>` collect 43.7 M (1,041,249), `RawTable<(String, Type)>::reserve_rehash` 41.8 M, `format_inner` 40.8 M (932,106).
- `RawVecInner::finish_grow` inclusive 1,654.8 M; `do_reserve_and_handle` 6,431,212 calls: 834.2 M, of which `String::write_str` 655.0 M (4,311,870 calls): the `format!`/`Display` path growing `String`s without a capacity.
- Formatting: `format_inner` 1,199.2 M inclusive (2.66%); callers `record_branch_implied_facts` 314.1 M (586,229), `NameContext::resolve_as` 244.7 M (317,425), `fmt::format` 188.8 M (235,160), `narrowing_key::{closure#0}` 57.6 M, `TypeNode::to_source_string` 51.0 M. `resolve_as` (syntax/src/name_resolution.rs:232) builds `format!("{imported_path}\\{r}")` at line 263 for a qualified name whose first segment is imported; `NameContext::resolve` calls it 340,901 times (313.4 M).
- Path hashing (self `Path::hash` 1,531,950,221 = 3.39%): th01 976.6 M, workers 555.3 M (th02 490.0 M of it: `closure_scope` 158.5 M, `HashMap<PathBuf,()>::insert` 115.0 M (141,474), `run_finishing_passes` 67.6 M, `BodySource::body` 37.9 M (44,774), `reserve_rehash` 36.1 M, `resolve_global_scope_files` 24.9 M).

## 7. Ranked table: 20 largest leaf costs (by self Ir, all threads summed)

Self Ir comes from `agg.py self` over the nine `self-NN.txt` files (exact for listed functions). "Where/why" is what I read in the call trees and source.

| # | Leaf | Self Ir | % | Who calls it and why | Source | Idea or why inherent |
|---|---|---|---|---|---|---|
| 1 | `types::shape::Shape::set` | 2,663,990,435 | 5.90% | 63,104 calls from `infer_const_type_with::{closure#0}` building class-constant array literals (th04 holds 2.52 G of the closure's inclusive cost); linear `find` over all fields so far, O(N^2). Also drives `memcmp` (6.4 M calls) | types/src/shape.rs:272; registry/function_sig.rs:613 | Index keys in an `FxHashMap` while building, or sort-and-dedupe once. Not inherent. |
| 2 | `memcpy` (`__memcpy_avx_unaligned_erms`) | 1,662,816,179 | 3.68% | At least 852.9 M from Parser functions (24.6 M calls, about 35 Ir each, listed callers only), also `String::write_str` 109.7 M (7.48 M calls), `String::clone` 50.4 M (3.66 M) | syntax/src/parser/*.rs (parse chain), alloc | Parser memcpys are consistent with by-value moves of `Result<Expr, ParseError>` through the precedence chain; I did not measure `size_of::<Expr>`. Idea: box/shrink hot AST nodes or flatten the chain. Partly inherent. |
| 3 | `<Path as Hash>::hash<FxHasher>` | 1,531,950,221 | 3.39% | Main: 810 M from 404,459 `HashSet<PathBuf>::insert` in `register_references` (about 2,000 Ir each); `PackageIndex::owner` 138.7 M; workers: `closure_scope` 158.5 M, set inserts 115.0 M | project.rs:2808; composer/src/lib.rs:144; project.rs:1110 | Hash the path's raw bytes (`as_os_str().as_encoded_bytes()`) instead of component-wise `Path::hash`, or intern paths to ids/`Arc<Path>` with a precomputed hash; the dependents sets could hold file ids. Component-wise hashing is not inherent. |
| 4 | `Lexer::next_token` | 1,281,571,578 | 2.84% | 6,891,547 calls, one per token; long if-chain with `starts_with("?>")`, `starts_with("#[")` etc. before reaching the identifier path | syntax/src/lexer.rs:362 | Dispatch on the first byte with a `match` before the `starts_with` probes. Partly inherent (one pass over every byte). |
| 5 | `Relation::is_super_type_of` + `'2` | 540,421,334 + 1,258,188,362 = 1,798,609,696 (two symbols) | 3.99% | Recursive subtype test; biggest callers `Type::concat` (457 K calls), union member map (2.09 M calls), `absorb_contained_members`, `is_array` | types/src/relation/super_type.rs:38,59 | Direct `Repr` answer for `is_refined_string`; fold without `Vec<TrinaryLogic>`; (sup,sub) memo on interned pointers. |
| 6 | `memcmp` (`__memcmp_avx2_movbe`) | 1,056,656,749 | 2.34% | `Lexer::starts_with` 177.3 M (7.95 M calls), `Shape::set` 141.8 M (6.4 M), `is_contained_in` 113.6 M (5.14 M), interner `get_or_insert` 58.2 M (3.0 M), `NameTable::with_key` 38.8 M | lexer.rs:351; shape.rs:272 | Fixed by items 1 and 4; the interner and table compares are inherent to string-keyed lookups. |
| 7 | `Lexer::line_col_at` | 775,401,373 | 1.72% | 13,783,094 calls, two per token (start and end span) from `Lexer::run`; 56 Ir per call (derived) | lexer.rs:182, 310-312 | The end position of one token is often the start of the next; compute the span end only when needed, or incrementally from `pos`. |
| 8 | `Parser::new` | 717,839,745 | 1.59% | 14,252 files; moves each `(Token, Span, Option<String>)` into three `Vec`s and fills four hash maps for doc comments | syntax/src/parser.rs:95 | Filter in place (`retain`-style) or keep one `Vec` of a struct; avoid `FxHashMap<usize, _>` for doc comments (use a sorted vec). |
| 9 | `types::intern::insert` | 708,739,563 | 1.57% | 3,391,809 calls from `Type::new` (one per type constructed); only 605,078 allocate a new node, the rest hit an existing one (lock, hash, bucket scan, `Weak::upgrade`); derived 209 Ir per call | types/src/intern.rs:158 | Inherent to hash-consing, but the hit path could be sharded lock-free or skipped by building common types as constants; also 122.5 M of `drop_glue<Repr>` hangs under it (2.79 M calls: the candidate `Repr` is built then dropped on a hit). |
| 10 | `core::str::lossy::Utf8Chunks::next` | 698,101,666 | 1.55% | Via `String::from_utf8_lossy` (1,715,685 calls): file contents once per file (394.3 M), then every identifier (207.7 M) and doc comment (82.0 M) in the lexer | session.rs:575; lexer.rs:456,756 | Use `std::str::from_utf8` (fast ASCII path) and borrow on success; in the lexer identifiers are ASCII slices, so convert with `from_utf8` / `from_utf8_unchecked` after a check. |
| 11 | mimalloc core (`mi_theap_malloc_aligned` 697.8 M, `mi_free` 638.5 M, `_mi_page_malloc_zero` 385.5 M, `_mi_theap_realloc_zero` 221.3 M, `mi_malloc_aligned` 193.8 M) | 2,136,866,284 (sum of the five listed self values; mi_theap_malloc_aligned and mi_free are given with the pre-rounded values 697,824,244 and 638,487,042) | 4.7% | 22.35 M `__rust_alloc` calls; top sources `finish_grow`, `String::clone` (3.66 M), `intern::insert`, `add_uses` (1.25 M), `Vec<TrinaryLogic>` collect (1.04 M), `format_inner` (0.93 M) | many | Fewer short-lived allocations: size `String`/`Vec` up front, avoid `Vec<TrinaryLogic>`, avoid `format!` in loops (items 5, 18, 20). Allocator cost itself is inherent. |
| 12 | `name_table::Iter<ClassReflection>::next` | 453,941,735 | 1.01% | 10,243,401 steps, all from `Registry::repository_entity` (695 calls, 14.7 K classes per call, derived) | registry/hierarchy.rs:1221 | Precompute the repository -> entity map once. Not inherent. |
| 13 | `<Type as PartialEq>::eq` | 425,231,400 | 0.94% | 10.5 M calls from `infer_const_type_with::{closure#0}` (200.3 M) via `values.contains`; 4.0 M + 1.2 M from `is_super_type_of` | function_sig.rs:585 | Same as item 1 (`FxHashSet<Type>` for the value union). |
| 14 | `syntax::lexer::compute_line_starts` | 416,669,814 | 0.92% | once per file via `Lexer::from_bytes` (14,252 parse calls, from_bytes inclusive 452.8 M) | lexer.rs:95 | Byte-wise loop; use `memchr::memchr_iter(b'\n', src)`. |
| 15 | `RawVecInner::finish_grow` (+ `do_reserve_and_handle` 216.6 M) | 410,535,778 (+216,605,382) | 0.91% (+0.48%) | 6.43 M `do_reserve_and_handle` calls, mostly `String::write_str` (4.31 M calls, 655 M inclusive) in `format!`/Display, then `grow_one` for `Vec<Stmt>` (193,615), `ArrayItem` (105,104), `Type` (614,338) | alloc | Pre-size output strings and AST vectors; remove `format!` in hot loops. |
| 16 | `core::str::pattern::StrSearcher::new` | 409,480,874 | 0.91% | 1.08 M calls from `deps::doc_tags` (`trim_start_matches("/**")` per doc line), `ignore_comments::scan_parsed`, `mark_each_marker` | deps.rs:99-112 | Use `strip_prefix` with a byte/char pattern, or one `memchr`-style scan over the doc comment. |
| 17 | Name tables: `NameTable<ClassReflection>::get<str>` 322.9 M, `NameTable<ClassOwners>::get<str>` 320.5 M, `get_with_name` 180.0 M, `Name::with_key` 204.4 M, `HashMap<Folded,()>::contains_key<str>` 158.5 M | 1,186,262,105 (sum of the five) | 2.6% | Method/property/class lookups from the checker (`lookup_method_with_owner` 293,248 calls, `substitute_class_templates` 129,667, `class_template_bindings_rec`...) with per-lookup case folding and string hashing | registry/name_table.rs | Resolve to an interned `Name` id once and index by id; partly inherent. |
| 18 | `WeakTable::get_or_insert` 259.7 M + `Interner::name` 224.2 M | 483,841,376 (259,665,269 + 224,176,107) | 1.07% | 1,757,875 identifier tokens interned from `next_token` (inclusive 342.5 M), plus `Name::new` | syntax/src/name.rs:361,269 | Inherent to interning; could cache the last-seen identifier hash from the lexer to avoid hashing twice. |
| 19 | imbl `hamt::Iter::next` | 249,616,172 | 0.55% | 1.07 M items from `Vec<String>` collects of `Locals` keys, `record_branch_implied_facts`, `merge_scopes`, `forget_members_a_call_may_change` | checker/locals.rs, narrowing.rs | Iterating a persistent map is inherently slower than a flat map; avoid collecting all keys per call. |
| 20 | `is_real_variable_key` 248.4 M / `String::write_str` 238.0 M / `format_inner` 205.6 M (three separate leaves of nearly equal size) | 248,368,154 / 238,018,503 / 205,623,074 | 0.55% / 0.53% / 0.46% | `is_real_variable_key` (narrowing/predicates.rs:165) is a char-by-char check called from `insert_with_provenance`, `record_branch_implied_facts`; the two formatting leaves come from `format!("truthy({name})")` (narrowing.rs:455) and `resolve_as` (name_resolution.rs:263) | see left | `is_real_variable_key` over bytes; formatting: see section 5.4 and section 6. |

## 8. What is serial vs parallel

Main's 2.63 G (5.8%) runs as a serial stretch in Ir terms: `insert_parsed_file` 1.50 G, `prepare` 0.51 G, `analyse` 0.29 G. The finishing-pass root on th02 (3.13 G) is also one thread's work. These are the Ir numbers only; wall time was not measured here. Worker totals differ by up to 2.5x (th02 8.49 G, th07 3.42 G); th04's parse alone contains 2.52 G of the `Shape::set` quadratic.

## 9. Things I could not determine

- Which PHP file produces the huge constant array behind item 1; callgrind has no source-file names here (`???`), only the thread (th04, with th09/th02/th07 smaller).
- How much of `always_terminates`' 2.59 G of `infer_expr` is first-time inference versus repeated.
- The exact split of the 937 M of worker time outside the three roots.
- `size_of::<Expr>` and thus whether the 24.6 M parser memcpys are large-node moves.
- Small-function cross-thread sums from `--tree` outputs are lower bounds (threshold), flagged where used.
