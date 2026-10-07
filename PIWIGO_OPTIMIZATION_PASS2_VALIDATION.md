# Adversarial validation of pass 2 (`PIWIGO_OPTIMIZATION_PLAN_PASS2.md`)

Each item was checked against the source and the callgrind data in `/tmp/p2/`. Verdicts: HOLDS, HOLDS WITH CORRECTION, WEAKER THAN CLAIMED, WRONG PREMISE. Savings are instructions of the 45.13 G run. One throwaway instrumented run (reverted, not committed) settled S1(a).

## S1. Lazy context loading: PARTLY SETTLED, still not startable
- Measured (name-table lookups, `get`/`get_with_name`/`contains_key`, Piwigo scoped run): during the check phase **435 of 10,504 vendor classes (4.1 %)** are looked up, against 2,005/2,005 project classes. The condition the plan set ("well under 20 %") holds.
- Load-phase touches are not evidence either way: every one of the 10,504 vendor classes is touched while loading, because `insert_built_decl` looks each name up on insertion.
- Not settled: the finishing passes run over every class (hierarchy, traits, purity, member owners, ancestor names). Whether they can run per class on demand is the real design question, and the lookup counts say nothing about it. `repository_entity` and other `classes.iter()` scans would see only loaded classes. Registry reads happen on the parallel check threads, so a lazy parse needs a once-per-file cell.
- Verdict: HOLDS as the largest candidate; the next step is a design of the finishing passes' per-class cost, not more counting.

## S2. Persistent cache of parsed context declarations: UNCHANGED (hypothesis)
Helps repeat CLI runs only. Nothing measured changes its status.

## N1. Skip expression traversal in the `Declarations` walks: HOLDS WITH CORRECTIONS
- Data: `walk_expr` is 105.6 M per walk (not 131-136 M; that figure included `walk_stmt` and `visit_stmt`), five walks plus 29 M for `collect_classes`, plus `define_call` 160.8 M. Real saving about **0.7 G**, not 0.8-1 G.
- Hazard the plan understated: nested declarations are reached through closure bodies **and anonymous-class methods**, and `define()` can sit inside any expression (`defined('X') or define('X', 1)`). The flag must be "a `define` call exists anywhere in the file" plus "a class or function declaration statement exists inside an expression (closure or anonymous class)". Both are exact if set where the parser builds the `Call` node and where it parses a declaration statement at expression depth > 0.
- Plumbing cost: the parser returns `Vec<Stmt>`; `ParsedFile` carries `annotations` but the declaration walks take `&[Stmt]`. The facts have to travel with the statements (about 36 call sites) or sit on `ParsedFile` with the walks taking it.
- **Cheaper first step with no flag at all:** `declared_names` and `declared_constants` are called back to back on the same tree (`project.rs:1587-1588`, `2617-2618`), and `DuplicateClassSites::index` and `DuplicateFunctionSites::index` likewise. One fused walk for each pair removes two of five walks (about 0.3 G) with no parser change.

## N2. Fuse or gate the other walks: HOLDS, sizes confirmed
- `Lines` 134 M + `Found` 134 M + `mentions_stub_attributes::Find` 125 M = 393 M. `NewAnonymousClass` is built at one site (`parser/classes.rs:123`), so the anonymous-class flag is exact and cheap.
- `mentions_stub_attributes` already is the gate for `stub_attributes::apply`; the cost is the gate's own walk, so the parser must set the flag where it parses an attribute.
- `annotations::Builder` is 277 M and `deps::Collector` 277 M in the data (the plan listed 119/173), both bigger than stated; `Resolver` 154 M.
- `FileBodies::of`'s collector did not show up under that name; unverified.

## N3. `add_uses` rebuilt per walk: HOLDS (data), fix needs correcting
It allocates a `(UseKind, String)` key and a `String` path per import per walk. Fusion removes some walks but not the per-walk allocations; storing the path as `Arc<str>` in `UseImport` and keying by a borrowed lowercase form removes them in every walk.

## N4. `purge_stale_narrowing_for`: WRONG AS WRITTEN, fixable
- The plan said to track "the text before the first `->` or `[`" of each synthetic key. **That is unsound**: purging `foo->bar` must drop `foo->bar->baz`, whose first-segment root is `foo`. The set must hold every prefix ending before each `->` or `[`, plus the variables used in `[$var]` subscripts.
- All inserts go through three functions in `checker/locals.rs` (`insert`, `declare`, `insert_with_provenance`), so the set can be maintained at the same three places as the existing flag. A 64-bit mask of hashed prefixes (no allocation, supersets only) is enough and exact for "may depend".
- Size: 122 M in thread 03, 11,082 calls. The `'['` prefilter point stands.

## N5. Non-ASCII column counting: WEAKER THAN CLAIMED
`from_utf8` from `columns` is about 20 M (12.1 M main + 1.1 M + 6.6 M) and the `chars().count()` and lossy paths another ~15 M: about **35 M, 0.08 %**. The "non-continuation byte" count is not identical for a slice that starts or ends inside a character (the lossy count differs); it needs a boundary guard. Low priority.

## N6. `Keyword::try_from_bytes`: HOLDS, small
24 M (`from_utf8` on at most 15 bytes per identifier). Matching on byte slices is exact because every keyword is ASCII.

## N7. `object_class_names`: HOLDS, small
Allocates a `Vec` at every union member as well as the result (nested `flat_map(...).collect()`). Pushing into one vector fixes both; 36 references to adapt. About 7-10 M.

## N8. Union memo allocation: HOLDS, 40-60 M
`kept: Vec<Type>` per miss. A two-slot inline form for the binary operations is exact.

## N9. `is_plain_word`: HOLDS, about 40 M available
82 M self over all threads, 290 Ir per call. The `inf`/`infinity`/`nan` test runs on every word; testing the first letter first, and a byte table for the scan, roughly halves it.

## N10. `looks_like_constant_name`: HOLDS, exact, but only ~7 M
Testing the last byte first is exact (the whole segment must be in the set, so a last byte outside it is a `false`).

## N11. Parallelize `check_class_shapes`: WRONG REMEDY, better one exists
- It is opt-in (`closed_classes` or parent-call rules), so the gain is for users of that rule only.
- `collect_classes` keeps only declarations that are **not conditional**, and nothing declared inside an expression is unconditional. A plain walk of the top-level statements (recursing only into braced `namespace` bodies, `declare` blocks and bare blocks, which are unconditional) gives the same list with no visitor and no expression walk. That removes the 91.8 M without a thread pool or an ordered collect. Needs a differential test against `for_each_declaration` over the fixtures.

## N12. File ids for `register_references`: HOLDS, but the real prize is larger (see V1)
Confirmed: `entry(name.clone())` allocates a `String` for every reference and `path.to_path_buf()` allocates for every reference (404 K each).

## N13. Vector growth: WRONG PREMISE
The premise (AST `Vec<Stmt>` and token vectors dominate) is not what the data says. Growth by `do_reserve_and_handle`: 78.8 M on main, of which `PathBuf::_push` 33 M and `Path::_join` 16 M (path building, 161 K pushes, 67 K joins); in threads 03 and 04 **`String` writes through `fmt` are 154 M and 55 M of the 202 M and 86 M**. The AST vectors are a distant second (`Vec<Stmt>::grow_one` 10-12 M per thread). See V2.

## N14. Per-call checker functions: unchanged, not actionable.

## N15. `InferKey` cache: PREMISE CORRECTED, fix HOLDS
The map is not per function. `expr.rs:314-320` makes a new map for every **outermost** `check_expr` and drops it. Keeping the allocation (`clear()`, shrinking only after an outlier) is exact. The 130 M is real.

## New findings from validating (not in pass 2)

**V1. `register_references` is dead work in a one-shot run: 977 M (2.2 % of the run), serial on main.**
- `insert_parsed_file` is 1,499 M of main's 2,630 M (57 %). 977 M of it is `register_references` (path hashing, string clones), another 315 M `PackageIndex::owner`.
- `dependents` and `signature_dependents` are read only by `walk_affected` and `finishing_scopes`. `run_finishing_passes` takes the `Scope::all()` branch when nothing is resolved yet, so the first and only run of `check-project` never reads either map.
- Change: build both lazily from the files' stored `references` sets the first time an incremental update needs them (`update_file`, `walk_affected`). The LSP path pays once instead of the CLI paying always. Exact for the CLI result. Needs: confirm the `FileState` keeps `references` and `declaration_references` (line 1310 reads `old.references`) and that no other reader exists (grep shows the five sites above).
- `PackageIndex::owner` (315 M) likewise: find the readers of the stored owner before deciding it can be lazy.

**V2. `format!` machinery is about 0.7 G self (1.5 %) in the check threads.**
- Self cost summed over threads: `String::write_str` 238 M, `format_inner` 208 M, `fmt::write` 183 M, `Formatter::pad` 50 M.
- Thread 03 callers of `format_inner` (284 M inclusive): `record_branch_implied_facts` **84 M** (157 K calls), `alloc::fmt::format` 55 M (68 K calls), `NameContext::resolve_as` **45 M** (59 K calls), `narrowing_key` 17 M, `purge_stale_narrowing_for` 8 M.
- `record_branch_implied_facts` (`narrowing.rs:455`) runs `format!("truthy({name})")` for every local in its loop, **before** the condition that uses it. Computing it only when the proxy test passes is exact and removes most of the 84 M in that thread, plus its self cost 50 M summed.
- `resolve_as` builds `"{imported_path}\\{r}"` with `format!`; `String::with_capacity` plus two `push_str` is exact and avoids the formatting machinery.

## Order after validation
1. V2 first fix (lazy `truthy` key): one line, exact, largest per-line gain.
2. V1 (lazy `dependents`): largest serial win; needs the reader audit.
3. N11 (statement-only `collect_classes`), the fused declaration walks (N1 step 1), N15, N10, N6, N7, N8, N9.
4. N4 with the corrected prefix rule, N3, N1 steps 2 with the facts struct, N2 flags.
5. S1 design (finishing passes per class); S2 after it.
6. Drop or defer: N5 (0.08 %), N13 as written, N14.
