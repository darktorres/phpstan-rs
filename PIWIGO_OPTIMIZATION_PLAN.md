# Optimizations for the scoped Piwigo run

Basis: `PROFILE_DRILL_PIWIGO.md` (callgrind of `target/gate/phpstan-rs check-project --config phpstan-rs.toml --format json`, 45.13 G Ir, 9 threads). Every figure below is an upper bound taken from that report (self or inclusive Ir, summed over threads); none has been tried. "Checked" means I read the source line the report names and it does what the report says. "From report" means I did not re-read it.

Shape of the run: workers 94.2 %. Parse + per-file declaration indexing 47.8 %, checking 37.4 %, finishing passes 6.9 % (one thread), main thread 5.8 % (serial).

Savings are not additive: several items remove the same instructions.

## A. Quadratic or whole-table scans (largest, cheapest)

**A1. `Shape::set` is O(N^2) while building a constant array literal.** 2.66 G self (5.9 %), plus 0.43 G `Type::eq` and 0.14 G memcmp from the same loop.
- Checked: `types/src/shape.rs:272` does `fields.iter_mut().find(|f| f.key == field.key)`; `registry/function_sig.rs:613` calls it once per literal item; `values.contains(&value_ty)` at `function_sig.rs:585` is a second linear scan.
- Change: build with a side `FxHashMap<ShapeKey, usize>` (last one wins, as `set` does) and an `FxHashSet<Type>` for the value union (types are interned, so hashing is by node). Or collect all fields and dedupe once at the end.
- Risk: field order and last-wins must stay identical; a test comparing old and new on random literals with duplicate keys.
- Sits on thread 04 (2.52 G of its 5.79 G), which is why that worker finishes late.

**A2. `repository_entity` scans every class on each call.** 0.68 G (695 calls x about 14.7 K classes).
- Checked: `registry/hierarchy.rs:1221` iterates `self.classes` and filters on `doctrine_entity.repository_class`.
- Change: a lazily built map repository FQN -> entity names (the existing `doctrine_entity_table_index` OnceLock in `reset_lazy_tables` is the place). Keep the "exactly one entity or none" rule.
- Risk: invalidation; it is already reset with the other lazy tables.

**A3. `register_references` hashes a `PathBuf` 404,459 times.** About 0.98 G inclusive, `Path::hash` 810 M self.
- Checked: `project.rs:2808` does `dependents.entry(name.clone()).or_default().insert(path.to_path_buf())` for every reference of every file, serially on main.
- Change (pick one or stack them):
  1. Give every loaded file a small integer id and store `FxHashSet<FileId>` (or a sorted `Vec<FileId>` per key, appended in load order). The set, `affected`, `stale` and `closure_scope` then hash integers.
  2. Hash paths by their raw bytes (`as_os_str().as_encoded_bytes()`) with a wrapper key; `Path::hash` goes component by component.
  3. Move the reference registration out of the serial insert loop: build per-chunk maps on the workers, merge on main.
- Risk: option 1 touches every `HashSet<PathBuf>` in `Project` (dependents, signature_dependents, affected, stale, closure_scope). Large but mechanical; iteration order of results must stay deterministic (the gates check emission order).

**A4. `closure_scope` and finishing passes hash paths too.** `closure_scope` 158 M `Path::hash` from `self.files.get(file)` (197,539 calls), 138.8 M for `Cloned<Flatten<HashSet<PathBuf>>>`. From report. Fixed by A3 option 1.

## B. Parse phase (47.8 %, 21.6 G)

**B1. Merge the per-file AST walks.** `referenced_names` 1.38 G, `declaration_referenced_names` 1.02 G, `DuplicateClassSites::index` 0.26 G, `declared_names` 0.25 G, `declared_constants` 0.25 G, `DuplicateFunctionSites::index` 0.25 G, `FileBodies::of` 0.14 G, `annotate` 0.46 G, `name_anonymous_classes` 0.14 G, `mentions_stub_attributes` 0.13 G, `scan_parsed` (ignore comments) 0.37 G: about 4.6 G.
- Change: one `Visitor` that computes all of them in a single traversal (the top-level declarations walks are cheap; the expression walks are the cost). The collectors that are independent of each other can share `visit_expr`. Keep `build_declarations` separate (it needs name contexts).
- Risk: the walks have different notions of scope (declaration-only vs everything); the merged visitor needs a "in declaration position" flag. Fixtures cover reference sets heavily (`incremental_updates` tests compare projects).

**B2. `Collector::doc_names` / `doc_tags` allocate a `StrSearcher` per doc line.** 1.50 G inclusive, `StrSearcher::new` 194 M (1,079,621 calls), `Lines::next` 170 M.
- Checked: `deps.rs:99-112` calls `trim_start_matches("/**")` (a string pattern) on every line.
- Change: `strip_prefix("/**")` once on the first line, `trim_start_matches('*')` is already a char pattern; skip lines without `@` by a byte scan before any trimming (`memchr(b'@')`); better, reuse the docblock memo (`phpdoc::docblock::parse_shared`, `grammar::parse_shared`) so the tags are read once per docblock text, not re-split by `deps`.
- `NameContext::resolve` is 342 M (345,941 calls): the resolved name is rebuilt with `format!` for every doc name (see E3).

**B3. Lexer: positions.** `line_col_at` 775 M self (13.78 M calls, two per token); `compute_line_starts` 417 M.
- Checked: `lexer.rs:310-312` computes `start` and `end` line/col for every token; `lexer.rs:95` scans bytes one by one for `\n`.
- Change: (a) `memchr::memchr_iter(b'\n', src)` for line starts (SIMD); (b) a token's end is almost always on the start's line: compute `end` from `start` plus the token length when the token has no newline (identifiers, operators, punctuation), and reuse the previous `end` as the next `start` (whitespace-only gap aside). (c) store spans as byte offsets in the token stream and resolve line/col lazily from `line_starts` only for the spans the parser keeps (statements, expressions that report diagnostics).
- Risk: (c) is large (every `Span` consumer); (a) and (b) are local. The lexer tests compare token spans, including non-ASCII sources.

**B4. Lexer: `next_token` and `starts_with` probes.** `next_token` 1.28 G self (6.89 M tokens), `Lexer::starts_with` 290 M (7.59 M calls, a `memcmp` each), `Keyword::try_from_bytes` 251 M, `Interner::name` 342 M.
- Checked: `lexer.rs:351` is `src[pos..].starts_with(bytes)`.
- Change: dispatch on the first byte (`match b`), then compare the remaining bytes only for that byte's candidates; one table-driven keyword lookup (perfect hash on length + first bytes) instead of `try_from_bytes` per identifier; skip `Keyword::try_from_bytes` when the identifier starts with an uppercase letter and is longer than the longest keyword.
- Risk: PHP lexing has many special cases (casts, heredocs, `?->`); the existing lexer tests plus corpus snapshots catch regressions.

**B5. Lexer: `from_utf8_lossy` everywhere.** `Utf8Chunks::next` 698 M self (1.72 M calls); 394 M for the file contents in `read_and_parse_file`, 208 M per identifier, 82 M per doc comment.
- Checked: `lexer.rs:297,452,456,498,655,756` and `session.rs:575`.
- Change: `std::str::from_utf8(bytes)` (a fast ASCII path) and borrow on success, falling back to `from_utf8_lossy` only on error; for identifiers the lexer already scanned ASCII bytes, so build the `&str` unchecked from a validated slice (the scan loop knows every byte was ASCII). For the file read, validate once with `String::from_utf8(bytes)` and keep the `Vec` (no copy) instead of `from_utf8_lossy(..).into_owned()`.
- Risk: invalid UTF-8 handling must stay lossy-equivalent (replacement characters, same columns).

**B6. `Parser::new` 718 M self (14,252 calls, 50 K Ir per file).** Checked structure: it moves every `(Token, Span, Option<String>)` into three parallel `Vec`s and fills four `FxHashMap<usize, _>` for doc comments.
- Change: filter in place with `retain`-style compaction of one `Vec<(Token, Span)>`; keep `keyword_raw` and doc-comment data in sorted `Vec<(index, ...)>` instead of hash maps (they are written in increasing index order and read by binary search); make `Token::DocComment` a side-table entry created by the lexer so the parser never sees it.
- Risk: `doc_comments_all`, `doc_comment_lines`, `doc_comment_starts` have separate readers; check each.

**B7. Parser moves.** `memcpy` 1.66 G total, at least 853 M from the parser (24.6 M calls, about 35 Ir each); `<&Token as PartialEq>::eq` 160 M (21.3 M calls); `RawVec<Stmt>::grow_one` 110 M. From report.
- Change: measure `size_of::<Expr>()`, `Stmt`, `Result<Expr, ParseError>` (the report could not). Box the cold large variants, return `Expr` by out-pointer in the precedence climber, replace `peek() == &Token::X` with a discriminant check (`matches!`), pre-size the statement vectors from the token count.
- Risk: pure refactors, but wide; the parser tests are extensive.

**B8. `build_declarations_where` / `ClassReflection::from_def` 6.0 G (13.3 %).** Beyond A1: `FunctionReflection::from_decl` 1.08 G (60,854 calls), `docblock::parse_shared` 0.62 G (40,483 calls, of which `apply_tag_recording_errors` 0.64 G inclusive for 39,718 real parses), property map 0.17 G.
- Change: in `from_decl`, `Vec<ParameterReflection>` collect is 533 M (63,296 calls): parse the type hint once per parameter (the `declared_type` calls are 393 M for 123,465 calls, about two per parameter: native and doc); `parse_written_type` 148 M for 31,737 calls: the types memo (below, F2) would remove repeated `@param int $x` strings across files.
- Docblock memo hit rate: 94,228 gets, 39,718 parses (58 % hits). The remaining parses are distinct texts; `apply_tag_recording_errors` still builds a scratch `DocBlock` for every `@param`/`@return`/`@var`. The split of `apply_tag` into parse and apply (the old D10) would save most of 640 M here; it was skipped earlier as 40 arms of churn. Worth doing now: the real numbers are 1.5 % of the whole run.

**B9. `insert_parsed_file` is serial.** 1.50 G on main (5.8 % of the run). Besides A3: `PackageIndex::owner` 315 M (14,251 calls, 22 K Ir each) and `absolute_path` 53 M.
- Checked: `composer/src/lib.rs:144` builds `lexically_normalized(file)` and walks `ancestors()` of both the written and the normalized path, hashing each ancestor `Path`.
- Change: cache the owner per directory (`FxHashMap<&Path, Option<PackageOwner>>`); files in one directory share the answer, and a project has far fewer directories (305 `collect_php_files` calls) than files (14,251). Compute `owner` on the workers during parse (it only reads the immutable index), not on main.
- Risk: none for correctness if the cache key is the exact parent directory.

**B10. Sorting and size lookup on main.** `driftsort<PathBuf>` 157 M + 41.5 M with `compare_components` 181 M, plus `Path::hash` under the size-sort closure.
- Change: sort by a precomputed key (`sort_by_cached_key`, or sort `(Reverse(size), index)` pairs), compare by bytes (`as_encoded_bytes`) where the order only has to be deterministic, not component-wise.
- Risk: output order of files; the gates check emission order. Keep a tiebreak equal to the current one.

## C. Main thread (2.63 G, serial)

**C1. `prepare` 508 M.** `discover_vendor_context` 462 M: `parse_generated_file` 237 M (one PHP file parsed on main), `Resolver::path` 112 M (12,399 calls), `dedup` 99 M.
- Change: run vendor discovery concurrently with the file collection and parse (it only feeds package context), or parse the generated autoload file lazily; `dedup` on a sorted `Vec<PathBuf>` by byte compare.

**C2. `check_phpstan_api_usage` 122 M and `check_class_shapes` 92 M on main**, a full AST walk per analysed file.
- Change: fold `check_phpstan_api_usage` into the per-file check on the workers (it is per file), or into the merged walk of B1.

**C3. `Session::analyse` after `load`:** the serial prefix (load 1.80 G + prepare 0.51 G) before any worker is busy again; overlap `prepare` with the parse by starting the parse pool first.

## D. Finishing passes (6.9 %, one thread)

All 3.13 G run on thread 02 (`run_finishing_passes`); the other eight workers are idle for that stretch in wall time.

**D1. `resolve_transitive_purity` 722 M**: `Type::mentions_any` 230 M (230,680 calls). Test the type flags (`HAS_*`) first, or cache `PropertyWrites` per stub signature (the old C9 note): the check "does a parameter type mention one of eleven code-running kinds" is a pure function of an interned type; memoize it per node identity (`Type::identity`, held), or add a flag.

**D2. `closure_scope` 673 M**: `OnceLock<Box<[u64]>>::initialize` 305 M (2,053 stub-file closures built, 149 K Ir each). Build all closures once by one SCC pass (condense the file graph, propagate bitsets in reverse topological order) instead of a DFS per file; or store them in the stub cache (format bump).

**D3. `infer_global_variables_of` 399 M** and `warm` 192 M: parallelise `infer_global_variables_of` over files; `warm` is already parallel.

**D4. Overlap the finishing passes with checking of files that cannot be affected**: only the first run has no earlier results; for a one-shot CLI run all files are checked after the passes, but the stub base and project-independent checks (syntax-level rules) could start earlier. Design item, not measured.

## E. Check phase (37.4 %, 16.9 G)

**E1. `Type::concat` 1.24 G direct (46,524 calls), `is_super_type_of` 1.28 G from it (456,975 calls).** From report; code read at `concat.rs:20` shows the literal shortcuts, then five `is_refined_string(kind).yes()` per operand.
- Change: compute the refinements of an operand once (a `RefinementSet` bitmask read directly from the `Repr`: `String` plus accessories), not by an `is_within` per kind (each builds an `is_super_type_of` query); the probes are already static after the earlier work, but the relation call still runs. For a bare `string` (no accessories) all five answers are `No`; for a literal the answer is a function of the bytes.
- Risk: the semantics of `is_refined_string` on unions and intersections; keep the old path as a test oracle (the same technique used for the `is_super_type_of` shortcuts).

**E2. `is_super_type_of` collects `Vec<TrinaryLogic>` per union member map.** 1.10 G inclusive, 2.09 M calls, 1.04 M `__rust_alloc`. From report.
- Change: fold with `extreme_identity` / `max_min` straight over the iterator (they already take iterators and short-circuit); add a bounded `(sup, sub) -> TrinaryLogic` memo keyed on node identity for the structural `NoClasses` relation (`Type::is_within`), where the answer is context-free.

**E3. `format!` on hot paths.** `format_inner` 1.20 G inclusive (2.66 %):
- `record_branch_implied_facts` 314 M (586,229): `let truthy = format!("truthy({name})")` at `narrowing.rs:455` is built for every local on every call, used only after three cheaper tests. Checked. Build it lazily inside the `if`, or compare `k.strip_prefix("truthy(").and_then(|r| r.strip_suffix(')')) == Some(name)` with no allocation.
- `NameContext::resolve_as` 245 M (317,425): `format!("{imported_path}\\{r}")` for a qualified name whose first segment is imported. Build into a `String::with_capacity`; or return a `Cow`/interned `Name` so repeats (same alias, same name) share storage.
- `narrowing_key` closure 58 M, `TypeNode::to_source_string` 51 M, `fmt::format` 189 M (235,160 calls): reuse a scratch `String`, or write keys with `write!` into a reused buffer.
- `String::write_str` growth is 655 M of `finish_grow`: pre-size strings (`String::with_capacity`) in these builders.

**E4. `Type::array_value_type` 535 M (9,476 calls, 55 K Ir each) and `array_offset_read` 277 M.** A union over all fields of large shapes, rebuilt per call.
- Change: memoize on node identity (a `OnceLock<Type>` in the interned `Shape`, or a side cache keyed by the `Arc<Shape>` pointer held alive); `Shape::key_type` has the same shape (the earlier skipped item): an `Arc<Shape>`-keyed cache is sound because the cache keeps the `Arc`.

**E5. `Relation::infer_template_bindings` 482 M (362,923 calls) and `substitute_templates_resolving_to_bounds` 194 M (321,770).** Skip when neither the parameter type nor the argument type has `HAS_TEMPLATE_REF`, but note the earlier warning: `settle` rewrites `KeyOf`/`OffsetAccess`/`ValueOf` even with no template, so the guard must also test `HAS_LATE | HAS_DEFERRED`.

**E6. `purge_stale_narrowing_for` 466 M, 1.35 M `is_contained_in` (139.6 M).** The earlier change made the needle once per purge; a substring search per local per assignment remains. Index locals by root variable (a second map `root -> keys`), so the purge visits only keys of that root, plus the `[$key]` subscript keys via a reverse index maintained on insert.

**E7. `merge_scopes` 370 M, imbl `hamt::Iter::next` 250 M (1.07 M items), `is_real_variable_key` 248 M.** `is_real_variable_key` is a char-by-char check on every key at insert: cache it as a flag in the key (a `LocalKey` newtype computed once) or test the first byte only (a real variable name cannot contain `->`, `[`, `(`; the first non-name byte decides). `merge_scopes`: iterate only keys that differ (imbl `diff`) instead of all keys of both sides.

**E8. `always_terminates` closure 2.88 G (upper bound, 165,043 calls into `cached<infer_expr>` 2.59 G).** The report could not tell first-time inference from repeats. Measure first: count `InferKey` cache hits vs misses on this run (an `eprintln` under a feature, then remove). If most calls miss, they are first inferences `check_expr` would pay anyway and the cost only moves; if hits dominate, the key construction is the cost.

**E9. `Registry::lookup_method_with_owner` 183 M (293,248 calls), name tables 1.19 G self (get / get_with_name / with_key / contains_key).** The per-class memo (`class_info`) already covers the per-class passes. Remaining: key as `Name` everywhere in the checker (`TableKey for Name` uses the cached lowercase), and per-call `lookup_method` results cached for `(class identity, method name)` within a file's check.

**E10. `sql_reflection::resolve_sql_row_type` 85-211 M (1,224 calls)**, `var_tag_nodes` 117 M, `check_one_method_override` 105 M, `scan_program_for_rebind_targets` 96 M, `Promotions::visit_stmt` 89 M: each is a full walk; fold the walks that need no per-call state into B1's visitor or run them once per file instead of per function.

## F. Cross-cutting

**F1. Allocation: 22.35 M `__rust_alloc` calls, mimalloc about 2.14 G self.** Top callers: `finish_grow` 406 M (5.0 M calls), `String::clone` 164 M (3.66 M calls: reference sets, `name.clone()` into `dependents`), `intern::insert` 72 M, `NameContext::add_uses` 58 M (1.25 M).
- Change: `Arc<str>`/interned names for the reference sets and `dependents` keys (clone is a refcount bump, hash by pointer or precomputed hash); `add_uses` builds a map per file: reuse one map across files with `clear()`; use `SmallVec`/`ArrayVec` for tiny vectors (`Vec<TrinaryLogic>`, `kept`, `args`).

**F2. Memoize `parse_written_type`** by exact string (like the docblock memo): `@param int $x` style strings repeat. 148 M in `from_decl` (31,737 calls), more in other callers.

**F3. `intern::insert` 709 M self (3.39 M calls, 209 Ir per call, only 605,078 allocate).** The hit path takes the shard lock, hashes, scans the bucket, upgrades a `Weak`. Options: thread-local front cache keyed by exact hash (verified by `exact_eq`) before the shard lock; build common composite types (`array<mixed,mixed>`, `list<mixed>`, `X|null` over the same X) through a keyed cache; shrink the lock to a read lock for lookups.

**F4. `WeakTable::get_or_insert` 260 M + `Interner::name` 224 M for 1.76 M identifier tokens.** The lexer hashes the identifier text; the table hashes it again. Pass the hash from the lexer (compute once, FxHash over the bytes) into `get_or_insert`.

**F5. Worker imbalance.** Threads 02..09 totals: 8.49, 6.05, 5.79, 3.78, 4.62, 3.42, 4.76, 5.60 G. Part of it is A1's 2.5 G on one file's thread and the finishing passes on thread 02. After A1 and D-items, check the rayon chunking (`with_min_len`, `par_chunks`) of parse and check: file sizes vary a lot; sort files by size descending (the loader already does at `session.rs:688`) and use work-stealing granularity of one file.

## G. Not worth doing, or needs data first

- `Lexer::run` Vec capacity (`src.len()/4`): already generous.
- `Output::render_json` 24 M: negligible.
- Stub cache decode 308 M (3,399 classes): already lazy; could shrink with shared decode for the Piwigo-referenced stubs only; low value.
- E8 above needs a measurement before any change.
- The 937 M of worker time outside the three phase roots was not identified; find it first (callgrind `--tree=caller` on `wait_until_cold`) before assuming it is finishing-pass work.

## Suggested order

1. A1, A2, B9 (owner cache), B3a/B5/B2 (small, local, exact), E3 (formats): about 5.5 G upper bound for little risk.
2. B1 (one walk), B6/B4/B3b (parser and lexer rewrites), E1/E2 (relation work): about 7-8 G, moderate.
3. A3 (file ids), D1/D2, E4/E5/E6/E7, F1/F3/F4: structural, each needs its own differential test.
4. Measure E8, the unexplained 937 M, and the worker imbalance (F5) before touching them.
