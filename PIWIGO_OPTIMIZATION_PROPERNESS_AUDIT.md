# Properness audit of the whole plan

Question asked of every item, in order: is the form in `PIWIGO_OPTIMIZATION_PLAN.md`, `..._VALIDATION.md`, `..._PLAN_PASS2.md` and `..._PASS2_VALIDATION.md` the most proper one, or did it shrink the change to avoid churn? Verdicts:

- **PROPER**: the form on the table is the complete one.
- **NARROWED**: the change was cut down for effort or risk; the proper form is stated.
- **RE-OPEN**: a rejection that rested on effort or on a coarse reading, not on a fact.
- **FACTUAL REJECT**: rejected because it is wrong, not because it is big; stays rejected.

Savings and sizes are as in the earlier files; nothing here was re-measured except where stated.

## Pass 1

**A1 `Shape::set` quadratic. NARROWED.** The plan patches the one call site (`function_sig.rs:613`). The quadratic is in the type's API: any code that builds a shape field by field pays it. Proper: a `ShapeBuilder` in the `types` crate (hash index from key to slot, last wins, values deduped once at the end only when not exact) and every bulk constructor of a shape goes through it; `Shape::set` stays for single edits. The second scan (`values.contains`) goes with it. Pairs with E4: a shape with thousands of fields should carry its value union, key type and offset index, computed once on the interned node.

**A2 `repository_entity`. NARROWED by the validation.** The "cache the answer per `current_class` in the `Checker`" alternative is the cheap one. Proper: the registry index (repository FQN to the list of entities, ambiguity kept), built where `doctrine_entity_table_index` is and reset with the other lazy tables. The checker cache then has nothing left to save.

**A3 `register_references`. NARROWED.** "Pick one or stack them" and the separate `get_mut`-first fix are partial. Proper: a `FileId` newtype issued once per path, never reused, used by `dependents`, `signature_dependents`, `affected`, `stale`, `closure_scope` **and** by `files` itself (`Vec<FileState>` by id plus one path-to-id map). Names in the reference sets become the interned `Name` already used by the registry (clone is a refcount, hash is precomputed) instead of `String`. Combined with V1 below, the CLI does not build the reverse index at all.

**A4 `closure_scope` hashing. NARROWED.** Marked PARTIAL because it needs `files` keyed by id. That is the proper form, and it is part of A3 above, not a separate larger task to avoid.

**B1 merge the walks. NARROWED twice.** First cut: "annotate, name_anonymous_classes, scan_parsed are not mergeable" (some mutate the tree). Second cut (pass 2): per-visitor flags. Proper, in one design:
1. One `FileDeclarations` list per file, computed once on the parse worker: every declaration with kind, name, span, conditional flag and name context. `declared_names`, `declared_constants`, both duplicate-site indexes and `build_declarations` consume it. That removes four of five `Declarations` walks with no flag at all, and the sixth in `collect_classes`.
2. A `FileFacts` struct filled by the parser (define call seen, anonymous class seen, stub attribute seen, declaration inside an expression, `global` statement, property write, `yield`, `eval`/`include`). Every remaining visitor, including the mutating ones, starts with a facts test and returns without walking. This is what makes the mutating visitors skippable; they did not need to be merged.
3. The two reference collectors become one walk with two sets.
The facts travel on a `Program` (statements plus facts) instead of a bare `Vec<Stmt>`; about 36 call sites change.

**B2 `doc_names` and the docblock work. NARROWED.** "Reuse the docblock memo" is a FACTUAL REJECT (the collector over-collects from raw tag text). But the replacement stopped at skipping the duplicate statement-level call. Proper: a memo of the raw tag split keyed by docblock text (the same technique as `parse_shared`), so `deps`, `templates_of` and the docblock parser share one split per distinct text; the reserved type words are dropped by a counted check; `StrSearcher` is gone. The duplicate pass over a declaration's docblock is removed as already validated.

**B3 lexer positions. NARROWED.** (a) `memchr` line starts: PROPER. (b) as written: FACTUAL REJECT; the valid form (end from start plus length when the token holds no newline) is PROPER. (c) lazy line/column was filed as "a separate design". Proper: tokens and AST spans carry byte offsets; line and column come from the line table when something reads them (diagnostics, a few node kinds). That also shrinks `Token` and `Span` (16 bytes) and removes `columns` and `char_count` (so N5 disappears). It touches every `Span` consumer. Do it, behind a `Span` accessor API so call sites change once.

**B4 `next_token`. NARROWED.** First-byte tests only remove two probes. Proper: dispatch on the first byte for the whole token table, a generated perfect-hash keyword lookup on length plus first bytes, and no per-token `starts_with`.

**B5 `from_utf8_lossy`. PROPER** with the validation's form (validate once, keep the `Vec`, `from_utf8` fast path for identifiers, lossy only on error).

**B6 `Parser::new`. NARROWED.** The "sorted vectors" idea was rejected because the parser calls `remove` on the map. That is an effort-shaped reason: a sorted vector with a monotone cursor and a taken bit serves `take_doc_comment`. Proper: the lexer emits the final parallel vectors directly (no second move of 6.9 M tokens) and the doc-comment side table; `Parser::new` becomes a wrapper.

**B7 parser moves. NARROWED.** Boxing the large `Stmt` variants alone leaves `Expr` (136 bytes) and every `Result<_, ParseError>`. Proper: measure each of `Stmt`, `Expr`, `TypeHint`, the `Result` wrappers and `ParseError`; box the cold large variants of all of them; replace `peek() == &Token::X` with discriminant checks; pre-size the statement and argument vectors from the token count.

**B8 `from_decl` and `apply_tag_recording_errors`. NARROWED.** The plan itself calls the parse/apply split "40 arms of churn". That is the churn-avoidance: the scratch `DocBlock` per tag costs 640 M. Proper: split `apply_tag` into parse (once per distinct tag text, memoized) and apply (per use). F2 (memoize `parse_written_type`) belongs here and needs one count of distinct type strings first.

**B9 owner. PROPER** as validated (exact by parent directory), plus computing it on the parse workers. With V1 the readers of the stored owner decide whether it can be lazy.

**B10 sorting. RE-OPEN.** Rejected because a byte sort differs from component order (`a-b` against `a/b`). That is a fact about byte order, not about the goal. An exact order-preserving form exists: sort by a cached key that maps the separator to a byte below every filename byte (or by the component list), with a test against `Path::cmp` over the corpus and the fixtures. About 0.2 G serial.

**C1 `prepare`. NARROWED.** The overlap was rejected because vendor context supplies the context scan directories. True for the context scan; but `parse_generated_file` (237 M, one file on main), `Resolver::path` (112 M) and `dedup` (99 M) are not ordered by anything and can be parallel or cached. Proper: parallelize `Resolver::path` over packages, parse the generated autoload file on a worker while the rest of discovery runs, dedupe on a sorted vector.

**C2 `check_phpstan_api_usage`. PROPER**, in the per-file worker closure; `check_class_shapes` goes through `FileDeclarations` (N11).

**C3 start parsing earlier. RE-OPEN.** Rejected with C1's argument. `load` collects the analysed files from `self.dirs`, which do not depend on vendor discovery; only the context files do. The analysed files can be parsed while `prepare` runs, with insertion order kept (context first, then analysed). Needs a read of `prepare` and `load` to confirm the split before starting; the gain is overlap of up to 0.5 G of main-thread time with parse.

**D intro (idle workers). FACTUAL REJECT**, stays.

**D1 `mentions_any`. PROPER** (verdict memoized per builtin signature, since the same ~5 K builtins are asked 230 K times).

**D2 stub-file closures. PROPER** (compute at stub-index build time, store in the cache, format bump).

**D3 `infer_global_variables_of`. PROPER** (`par_iter`, indexed collect).

**D4 overlap passes with checking. FACTUAL REJECT**, stays.

**E1/E2 `concat` and `is_super_type_of`. NARROWED.** E2 was reduced to "fold without the `Vec`". Proper: E1's refinement mask read from the `Repr` (bare string none; literal from its bytes; intersection from its accessories; union the AND of its members), the fold with early exit in the intersection arm, and a bounded memo for the context-free relation. The old path stays as the test oracle.

**E3 `format!`. NARROWED.** The plan fixes each call. Proper: stop building narrowing keys as formatted strings at all (see E6/E7) and, for the rest, write into a reused buffer. `truthy` lazily is still the first step because it is one line and exact.

**E4 `array_value_type`. NARROWED.** Proper: n-ary union with dedupe by node identity, and the value union, key type and offset index cached on the shape node (cached cell in the interned `Shape`), so every consumer benefits, not only `array_value_type`. Shares the shape work of A1.

**E5 template inference guards. PROPER** with the `HAS_LATE | HAS_DEFERRED` condition.

**E6 `purge_stale_narrowing_for`. NARROWED.** The validation recommended only the `'['` test and called the reverse index "large, not recommended". That is the churn-avoiding call. Proper: locals keyed structurally, a `LocalKey` of (root variable, path of `->prop` and `[index]` steps, subscript variable), with `Locals` holding a second index from root to keys and from subscript variable to keys. A purge visits only the keys that depend on the target. There is no string scan and no `format!`. About 164 call sites touch `locals.*` and `narrowing_key` has about 30 producers; do it as one change with the old string keys kept in a test oracle.

**E7 `is_real_variable_key` and `merge_scopes`. NARROWED.** The "key newtype touches every caller" rejection is effort-shaped; with E6's `LocalKey` the question disappears (a real variable is a key with an empty path). `merge_scopes` by `imbl` diff is UNVERIFIED and needs a read of the function before a decision.

**E8 `always_terminates`. PROPER to measure first** (hit and miss counts of the infer cache), because the cost may only move. Do the counting now.

**E9 name-table lookups. NARROWED.** Left vague. Proper: the caller breakdown on all nine threads (one `callgrind_annotate --tree=caller` pass), then `&Name` keys everywhere the checker holds a `Name` and a per-file `(class, method)` cache for method lookups.

**E10 other walks. NARROWED.** Proper: they go behind `FileFacts` (B1) and the ones with per-call state (SQL reflection) are called once per distinct input.

**F1 allocation. NARROWED.** Proper: the full caller breakdown first (the earlier trees were cut), then interned names in the reference sets, a reused map in `add_uses` and inline small vectors where the counts say so.

**F2 `parse_written_type` memo. PROPER after one count** (distinct strings), done with B8.

**F3 `intern::insert` front cache. PLAUSIBLE, NOT VALIDATED.** The hit-rate count comes first; the lifetime design (a front cache keeps nodes alive) has to be written before code.

**F4 hash from the lexer. FACTUAL REJECT**, stays.

**F5 imbalance. PROPER** to revisit after A1.

## Pass 2

**S1 lazy context loading. PROPER in intent, unfinished in design.** Measured: 4.1 % of vendor classes are looked up during checking. Not proper to start until the finishing passes' per-class cost is designed. The proper design is a per-class on-demand registry (a once-per-file lazy parse, a class-name to file map from composer's PSR-4/classmap), with an eager fallback for what has no map.

**S2 persistent parsed-declaration cache. PROPER, depends on S1's and `FileDeclarations`' stored form.**

**N1 skip expression traversal. NARROWED** (see B1): the facts struct alone leaves five walks. `FileDeclarations` first, then facts.

**N2 flags per visitor. NARROWED** into B1's single `FileFacts`.

**N3 `add_uses`. PROPER** with the validation's form (`Arc<str>` import paths, no per-walk allocation), and it shrinks again once the walks are fewer.

**N4 `purge_stale` mask. NARROWED.** The 64-bit hashed-prefix mask is a stopgap that avoids restructuring. Proper is E6's structured key and root index. The corrected prefix rule is only needed if the mask were used.

**N5 non-ASCII columns. NARROWED the other way (a stopgap).** Superseded by B3(c): no column counting at all. If B3(c) is not done, do N5 with the boundary guard.

**N6 `Keyword::try_from_bytes`. PROPER**, and subsumed by B4's generated lookup.

**N7 `object_class_names`. NARROWED.** "Return an iterator or a stack vector" for 36 references. Proper: a visitor-style `for_each_object_class` plus a collecting wrapper for the callers that need a list, so most callers stop allocating.

**N8 union memo. PROPER** (a two-slot inline form for the binary operations).

**N9 `is_plain_word`. PROPER** with the first-letter test; the complete form folds it into `split_top_level` so the string is scanned once.

**N10 `looks_like_constant_name`. PROPER** (last-byte test, exact).

**N11 `check_class_shapes`. PROPER after correction** (no thread pool): the list comes from `FileDeclarations`, which already holds the unconditional classes.

**N12 file ids. PROPER**, same as A3.

**N13 vector growth. RE-STATED, not dropped.** The premise was wrong, but the data names real causes: `String` writes through `fmt` (E3/V2), `PathBuf::push` and `join` on main (33 M and 16 M; find the callers in `collect_php_files`, `Resolver::path`) and `lex_single_quoted` (10 M, no reserve). Each gets its own fix; reserve calls for the AST vectors are minor.

**N14 per-call checker functions. NARROWED.** "Not actionable" came from a missing caller count, not a fact. Proper: take the caller-side counts with the E9 pass, then decide.

**N15 `InferKey` cache. PROPER** (one map kept and cleared per outermost expression, shrunk after an outlier).

**V1 lazy `dependents`, `signature_dependents` (977 M). PROPER** in the plan as written (built on the first incremental update from each file's stored reference sets), combined with A3's `FileId`. Also decide `PackageIndex::owner` (315 M) once its readers are known.

**V2 `format!` (about 0.7 G). PROPER** for `truthy` and `resolve_as`; the rest of its callers follow E3 and E6.

## What this changes

- Make **proper** (were narrowed): A1 (builder), A2 (index), A3/A4 (ids everywhere), B1/N1/N2 (`FileDeclarations` plus `FileFacts`), B2 (shared raw split memo), B3(c) (offset spans, which also removes N5), B4 (full dispatch and generated keywords), B6 (final vectors from the lexer), B7 (all large types), B8 (tag parse/apply split), C1 (parallel pieces), E1/E2 (mask, fold, memo), E4 (cached shape summaries), E6/E7/N4 (structured `LocalKey` with root index), E9/N14/F1 (measure all threads first), N7.
- Re-open rejections that rested on effort: **B10** (exact order-preserving sort), **C3** (parse analysed files during `prepare`, after reading both).
- Stay rejected on facts: D intro, D4, F4, B3(b) as written, "reuse docblock memo" in B2.
- Still unmeasured, so not yet proper to start: S1's finishing-pass design, E7's `merge_scopes`, F3, E8.

---

# Second pass over the audit

The first pass above judged most items from the plan text. This pass re-read the code behind the open verdicts, re-examined every item I had marked PROPER for a hidden narrowing, and covered section G, which the first pass skipped.

## Open verdicts settled by reading code

**C3 (parse analysed files during `prepare`). CONFIRMED FEASIBLE, restructure needed.** `Session::load` (`session.rs:595-700`) collects the analysed files from `self.dirs` (no dependence on vendor discovery), then parses context and analysed files in **one** parallel batch, then inserts serially (context first, analysed second). `prepare`'s 462 M of vendor discovery runs on main before any of it. So the analysed files can be parsed while discovery runs; insertion order is unchanged because it is a separate serial loop. It needs `Project::new`, the interner and `configure` available before discovery ends, which they are not today. The complete form is a pipeline: discovery streams file lists, a parse pool consumes them, one ordered inserter drains. It also answers the larger serial cost: `insert_parsed_file` is 1.5 G on main, so the per-file work that does not need the shared registry (references, owner, use-sites, duplicate sites) moves into the worker closure and the main loop only merges.

**B10 (sort). CONFIRMED EXACT FORM EXISTS.** `analyze_files.sort()` and `context_files.sort()` use `PathBuf::cmp` (component order). Mapping `/` to a byte below every filename byte gives the same order as component comparison for normalized joined paths (`a/b` before `a-b`, as `Path::cmp` has it). Paths here come from `PathBuf::join` of `read_dir` names, so there are no doubled separators or `.` components in the middle. Test: the new key against `Path::cmp` over every pair of a sample and over the full Piwigo list, plus the fixtures. Use `sort_by_cached_key`.

**E7 `merge_scopes`. READ, and the plan's "imbl diff" is wrong.** `merge_scopes` (`control_flow.rs:576`) walks every entry of the then-scope, looks each up in the else-scope, clones provenance, compares, and re-inserts into a fresh scope. `imbl::HashMap` has no structural diff, so that idea does not exist. The proper change is a dirty-key log: each `Locals` records the keys written since its fork point. The merge starts from a clone of the then-scope (sharing structure) and recomputes only keys dirty in either branch; a key present in only one branch is always dirty (a removal is a write). `touch()` and the version counter already mark writes, so the log extends an existing mechanism. This is the full form of E7 and needs its cost measured first (370 M for `merge_scopes`, 250 M `hamt::Iter::next`) against how many keys a typical merge actually changes.

## Items I marked PROPER that were still narrowed

- **D1 `mentions_any`:** memoizing per signature at run time is the small form. The verdict is a function of a stub signature alone, so it belongs in the stub cache next to D2's closures (computed at stub-index build, stored, format bump). The CLI then pays nothing.
- **V1 lazy `dependents`:** correct for the CLI, but the complete form removes the reverse index as a stored structure for the one-shot path **and** makes the incremental path compute affected files from the stored per-file reference sets with `FileId`s (A3), so the LSP no longer pays at load either.
- **B9 owner:** a per-directory cache is the small form. The index itself walks every ancestor of a file and hashes each `Path`; the complete form is a component trie (or a directory-keyed map built once) so a lookup is one walk without hashing paths, computed on the workers.
- **B5, C2, D3, E5, N3, N8, N9, N10, N15, V2:** re-checked; each is already the complete form of its own fix. (E5 should test one precomputed "needs settle" flag rather than two flag bits.)

## Section G, covered now

- **"`Lexer::run` capacity already generous". PROPER to leave**, a fact about a measured non-cost.
- **"`render_json` 24 M, negligible". NARROWED.** Proper: write the diagnostics straight into a buffered writer instead of building the string and printing it; exact output, a few lines.
- **"Stub cache decode 308 M, low value". NARROWED.** The classes decode lazily as whole classes. Proper: decode members on demand (method table by offset) so a class referenced for one method does not decode the rest. Needs a count of members read against members decoded before it is worth the format change.
- **"The 937 M of worker time outside the three roots". RE-OPEN as a measurement.** Run `callgrind_annotate --tree=caller` on the nine threads for `wait_until_cold` and the pool entry points; until it is attributed, the pass-1 claim that it is finishing-pass work is also unsupported.

## Plan text that contradicts this audit

Pass 2 section 3 ("B10, C1/C3, D4, F4 stay rejected") is superseded: B10 and C3 are open (above), C1's three pieces are parallel work, D4 and F4 stay rejected on facts.

---

# Third pass: measurements that decide the open verdicts, and a look at the "proper" forms themselves

Three things were still guessed. Each is now measured on the scoped Piwigo run (throwaway counters, reverted, nothing committed) or read from the callgrind data.

## Measured

**The 937 M "outside the three roots" is the stolen part of the finishing passes (settled).** Per worker, total minus (`check_program` + `read_and_parse_file` + `run_finishing_passes`) is 0, 69, 281, 75, 192, 134, 149, 61 M (sum 963 M). The jobs a worker runs outside those roots are the `par_iter` pieces of the finishing passes stolen from thread 02: the property-writes classification (180 M on thread 04), `Registry::warm` (181 M on thread 06), member-owner maps (62 M), ancestor and flatten pieces. So the finishing passes cost about 3.13 G + 0.94 G = **4.07 G (9.0 % of the run), not 6.9 %**. Every D item is worth about a third more than the plan said, and D1's property-writes classification and `warm` are parallel work that is not on thread 02's critical path but is still instructions.

**E8, the infer cache: 578,015 hits and 819,969 misses (41 % hits).** The cache works. The 2.88 G under `always_terminates` is mostly first inferences, but reading the code shows why they are expensive and repeated: `always_terminates` (`control_flow.rs:93`) calls `self.infer_expr(e, locals)` for each expression statement, and it runs **outside** `check_expr`, where `infer_cache` is `None` (`expr.rs:314`), so `cached` returns the uncached answer. The same statement's expression is then inferred again by `check_expr`, and nested `if` bodies call `always_terminates` on the same statements repeatedly. Proper form: a statement-level termination memo keyed by (statement, scope version), valid within one function check, and the inference it needs shared with the check of that statement instead of repeated. The cheap alternative (a syntactic pre-test for "can this expression type be `never`") is the narrower fix because an assignment of a `never` call is itself `never`; the memo covers it.

**`merge_scopes`: 22,497 merges, 392,942 keys visited (17.5 per merge), 22,987 changed (5.8 %).** 94 % of the iteration is over keys equal in both branches. The dirty-key design (start from the then-scope, recompute only keys written in either branch since the fork) is justified by data, not only by reading. It is also the form that removes the per-key `Type` clone, provenance clone and `insert_with_provenance` for the 94 %.

**`parse_written_type`: 310,883 calls, 13,198 distinct strings (4.2 %).** A memo by exact string hits 95.8 %. F2 moves from UNVERIFIED to proper and large (the calls are in `from_decl`, doc-tag application and checker resolution). Proper form: one process-wide sharded memo (not thread-local; nine threads would each rebuild 13 K entries), holding interned `Type`s, bounded only if memory needs it (13 K entries is small), with the nullable and trim handling inside the memoized function so every caller benefits.

## The "proper" forms, re-read for a remaining compromise

- **E1 refinement mask:** the plan reads the mask from the `Repr` at each call. The complete form stores it on the interned node as flag bits computed at construction (the `u16` flags already exist for `HAS_*`), so a probe is one load and the intersection/union cases combine children's bits at intern time. No call-time walk at all.
- **E4 shape summaries, E5 template flag:** same shape of change: compute once at interning, read as a field. E5's "needs settle" is one flag set at construction.
- **A1 `ShapeBuilder`:** also provide the n-ary constructor `Shape::from_fields` used by stub decoding and `array_value_type`, so the quadratic pattern has no public entry left.
- **D1 stub verdicts and D2 closures:** computed when the stub index is built and stored in the stub cache in one format bump (version 8), not two bumps.
- **B1 `FileDeclarations` and `FileFacts`:** computed on the parse worker and kept on the file state; the incremental path (`update_file`) recomputes them for the edited file only, so the LSP uses the same code.
- **F3 `intern::insert` front cache:** still unmeasured. The measurement is the hit rate of the shard lookup (3.39 M calls, 605 K allocating, so 82 % hit): the case for a front cache is already strong, but the node-lifetime design has to be written first. Not narrowed, just not ready.
- **S1/S2:** the proper form depends on `FileDeclarations` being serializable (S2) and on a per-class finishing design (S1); both stay design items.

## New plan entries from this pass

1. Statement-level termination memo plus shared inference for `always_terminates` (E8's real fix).
2. Dirty-key merge for `merge_scopes` (E7), justified at 94 % unchanged.
3. Shared `parse_written_type` memo (F2), 95.8 % hit rate.
4. Finishing passes are 9 % of the run: D1/D2/D3, the `warm` closure, and the property-writes classification all count.

---

# Fourth pass: feasibility of the proper forms

The earlier passes asked whether each change was complete. This one asked whether each complete form can be built as described. Two third-pass forms failed:

- **E1 (refinement bits in `flags`):** `flags` propagates by OR ("anywhere in the type"); string refinements are "every value" (AND over unions, OR over intersections, with implications). Eager computation at interning costs seven relation calls per new node (605 K) against 457 K probes saved. Replaced by a lazy per-node `AtomicU16` cache filled by the existing relation on first probe: exact by construction, each node pays once.
- **E5 ("precomputed needs-settle flag"):** unnecessary; the three bits already exist and one mask test covers them.

Re-read with no further change: E4 (lazy `OnceLock` on the shape), E6/E7 (dirty log covers `types` and provenance only; the guard sets keep their intersect/union code), A3 (the path-to-id map stays for the LSP's path lookups), B3(c) (the line table stays on the file state), C3 (needs `Project::new`, the interner and `configure` before discovery ends).
