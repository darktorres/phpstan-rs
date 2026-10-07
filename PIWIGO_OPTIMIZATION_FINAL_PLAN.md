# Final plan: every item in order, in its most proper form

This replaces the item text of `PIWIGO_OPTIMIZATION_PLAN.md`, `..._PLAN_PASS2.md` and their validations wherever they differ. Source of each verdict: `PIWIGO_OPTIMIZATION_PROPERNESS_AUDIT.md` (three passes). For each item: **Was** = how it stood (NARROWED means it had been cut down for effort or risk; PROPER means it was already complete; FACT-REJECT means rejected because it is wrong). **Do** = the complete change. **Gate** = what must hold or be measured before or while doing it. Sizes are instructions of the 45.13 G Piwigo run, upper bounds, overlapping.

Common rule for every item: keep the old code path as a test oracle (differential test over the fixtures, the vendor tree and the Piwigo corpus) until the new one has matched it; per commit run fmt and clippy only; run the suites once at the end.

## A. Scans and hashing

**A1 `Shape::set` quadratic (2.66 G + 0.57 G). Was NARROWED (one call site).** Do: `ShapeBuilder` in `types` (hash index from key to slot, last wins, value union deduped once at the end and only when not exact) and an n-ary `Shape::from_fields`; every bulk constructor (`function_sig.rs`, stub decoding, `array_value_type`) uses them; `Shape::set` stays for single edits. Gate: test against the old `set` loop on random literals with duplicate and auto keys.

**A2 `repository_entity` (0.68 G). Was NARROWED (per-class cache).** Do: registry index repository FQN to list of entity names, built beside `doctrine_entity_table_index`, reset with the lazy tables; ambiguity (more than one entity) preserved. Gate: the Piwigo `ImageRepository` case.

**A3 `register_references` (0.98 G, serial). Was NARROWED (pick one).** Do: `FileId` newtype, never reused; `dependents`, `signature_dependents`, `affected`, `stale`, `closure_scope` and `files` itself (`Vec<FileState>` by id plus one path-to-id map) use it; reference sets hold interned `Name`s; `get_mut` before any key clone. Emission order unchanged. Combined with V1 the CLI does not build the reverse index.

**A4 `closure_scope` path hashing (0.16 G). Was NARROWED (called PARTIAL).** Do: part of A3 (`files` by id).

## B. Parse phase

**B1 walks (about 4 G of walk and callback cost). Was NARROWED twice.** Do: (1) `FileDeclarations`, one declaration list per file built on the parse worker and kept on the file state (kind, name, span, conditional, name context); `declared_names`, `declared_constants`, both duplicate-site indexes, `build_declarations`, `collect_classes` consume it. (2) `FileFacts` filled by the parser (define call, anonymous class, stub attribute, declaration inside an expression, `global`, property write, `yield`, `eval`/`include`); every other visitor, mutating ones included, returns at once when its fact is absent. (3) the two reference collectors become one walk with two sets. (4) `Program` (statements plus facts) replaces the bare `Vec<Stmt>` at about 36 call sites; `update_file` rebuilds both for the edited file. Gate: declared names and constants compared with a full walk over fixtures plus the vendor tree.

**B2 doc names (1.50 G). Was NARROWED (only the duplicate pass).** Do: remove the duplicate statement-level docblock pass; one shared memo of the raw tag split keyed by docblock text, used by `deps`, `templates_of` and the docblock parser; skip reserved type words after counting them; no `StrSearcher`. Reusing the parsed-docblock memo is FACT-REJECT (`doc_names` over-collects from raw text).

**B3 lexer positions (0.78 G + 0.42 G). Was NARROWED.** Do: (a) `memchr` line starts (new direct dependency, already in the lock file); (b) end position from start plus length for tokens with no newline (the "reuse previous end" form is FACT-REJECT); (c) tokens and AST spans carry byte offsets, line and column come from the line table on demand, behind a `Span` accessor API. (c) also removes `columns` and `char_count`, so N5 disappears.

**B4 `next_token` (1.28 G + 0.29 G + 0.25 G). Was NARROWED.** Do: first-byte dispatch for the whole token table, a generated perfect-hash keyword lookup on length and first bytes (matching on bytes, no `from_utf8`), no per-token `starts_with`.

**B5 `from_utf8_lossy` (0.70 G). Was PROPER.** Do: validate the file once and keep the `Vec`, `from_utf8` fast path for identifiers and doc comments, lossy only on error with the same replacement columns.

**B6 `Parser::new` (0.72 G). Was NARROWED (rejected sorted vectors).** Do: the lexer emits the final parallel vectors and a doc-comment side table; `take_doc_comment` uses a sorted vector with a monotone cursor and a taken bit; `Parser::new` becomes a thin wrapper.

**B7 parser moves (1.66 G memcpy). Was NARROWED (`Stmt` only).** Do: measure `Stmt` (472 B), `Expr` (136 B), `TypeHint` (64 B), the `Result` wrappers and `ParseError`; box the cold large variants of all of them; discriminant checks instead of `peek() == &Token::X`; pre-size statement and argument vectors from the token count.

**B8 `from_decl` and `apply_tag_recording_errors` (1.08 G + 0.64 G). Was NARROWED ("40 arms of churn").** Do: split `apply_tag` into parse (once per distinct tag text, memoized) and apply (per use); parse each parameter's type once; F2's shared memo belongs here.

**B9 `PackageIndex::owner` (0.32 G). Was NARROWED (directory cache).** Do: a directory-keyed index or component trie built once, no per-ancestor `Path` hashing, computed on the parse workers.

**B10 sorting (0.2 G, serial). Was a REJECT on effort.** Do: `sort_by_cached_key` with a byte key that maps `/` below every filename byte, which reproduces `Path::cmp` for joined paths. Gate: equality with `Path::cmp` on the full Piwigo list and the fixtures.

## C. Main thread

**C1 `prepare` (0.51 G). Was NARROWED.** Do: parallelize `Resolver::path`; parse the generated autoload file on a worker; dedupe on a sorted vector. (Overlapping discovery with file collection stays rejected: context directories come from discovery.)

**C2 `check_phpstan_api_usage` (0.12 G) and `check_class_shapes` (0.09 G). Was PROPER (C2), NARROWED (class shapes).** Do: the API check moves into the per-file worker closure; class shapes read `FileDeclarations` (no walk).

**C3 start parsing earlier. Was a REJECT on a coarse reading.** Do: a pipeline: discovery streams file lists, a parse pool consumes the analysed files while `prepare` runs, one ordered inserter drains (context first, analysed second); per-file work that needs no shared registry (references, owner, use-sites, duplicate sites) moves into the worker closure so the serial loop only merges (`insert_parsed_file` is 1.5 G on main). Gate: `Project::new`, the interner and `configure` must exist before discovery ends.

## D. Finishing passes (4.07 G including the stolen work, 9 % of the run)

**D intro (idle workers). FACT-REJECT.** The unexplained 937 M is the stolen `par_iter` work of these passes (settled).

**D1 `mentions_any`/`stub_verdict` (0.72 G). Was NARROWED (run-time memo).** Do: compute the verdict per builtin signature when the stub index is built; store it in the stub cache (one format bump with D2). Also cover the property-writes classification (180 M stolen) the same way where it is a function of the stub alone.

**D2 stub-file closures (0.67 G). Was PROPER.** Do: compute at stub-index build time, store in the cache.

**D3 `infer_global_variables_of` (0.40 G), `warm` (0.19 G). Was PROPER.** Do: `par_iter` with an indexed collect for the first; keep `warm` parallel and check its chunking.

**D4 overlap passes with checking. FACT-REJECT** (checking needs the finished registry).

## E. Check phase

**E1 `Type::concat` (1.24 G + 1.28 G). Was NARROWED; the third-pass form was itself wrong (fixed in the fourth pass).** Concat asks `is_refined_string(kind).yes()` for seven kinds (non-empty, non-falsy, literal, numeric, lowercase, uppercase, decimal-integer). The `flags` field cannot hold them: its bits propagate as "anywhere in the type" (an OR), while these are "every value" (an AND over a union, an OR over an intersection, with implications such as non-falsy implying non-empty). Computing them eagerly at interning would run seven relation calls per new node (605 K nodes) to save 457 K probes, and re-deriving the lattice by hand risks a mismatch. Do: a separate `refined: AtomicU16` on `Node` (two valid bits per kind: computed and yes; check `size_of::<Node>()` is unchanged by padding), filled on the first probe of a node by calling the existing `is_within` path, then read as a load. It is exact by construction (the old function is the source of every bit) and each node pays at most once. Keep the old path as the test oracle.

**E2 `is_super_type_of` `Vec<TrinaryLogic>` (1.10 G). Was NARROWED.** Do: fold with early exit in the intersection and union arms (no `Vec`), plus a bounded memo for the context-free `NoClasses` relation only. Largely overlaps E1.

**E3 `format!` (1.20 G inclusive, about 0.7 G self). Was NARROWED.** Do: `truthy` key built only inside the `if` (first, one line); `resolve_as` with `with_capacity` and `push_str`; no formatted narrowing keys at all once E6 lands; a reused buffer for the rest.

**E4 `array_value_type` (0.54 G), `array_offset_read` (0.28 G). Was NARROWED.** Do: n-ary union deduped by node identity, and the value union, key type and offset index cached on the interned shape (computed once, read as fields).

**E5 template inference guards (0.48 G + 0.19 G). Was PROPER.** Do: skip when `flags & (HAS_TEMPLATE_REF | HAS_LATE | HAS_DEFERRED) == 0` on both types, one mask test on the existing field (no new flag; the fourth pass removed the "precomputed flag" wording).

**E6 `purge_stale_narrowing_for` (0.47 G, plus about 0.12 G from the corrected pass-2 numbers). Was NARROWED (the `'['` test and a mask).** Do: a structured `LocalKey` (root variable, path of property and index steps, subscript variable) with `Locals` indexing keys by root and by subscript variable; a purge visits only dependents; a real variable is a key with an empty path. About 164 `locals.*` call sites and about 30 `narrowing_key` producers change in one commit; the old string keys stay in a test oracle. The hashed-prefix mask is rejected as a stopgap.

**E7 `merge_scopes` (0.37 G), `is_real_variable_key` (0.25 G). Was NARROWED (rejected a key newtype; "imbl diff" does not exist).** Do: `is_real_variable_key` disappears with E6. `merge_scopes` uses a dirty-key log (keys written since the fork point, extending `touch()`/version): start from the then-scope, recompute only keys dirty in either branch. Measured: 94.2 % of visited keys are unchanged (22,497 merges, 392,942 keys, 22,987 changed).

**E8 `always_terminates` (2.88 G upper bound). Was UNVERIFIED, now explained.** Measured: infer cache 41 % hits (578 K of 1.40 M). `always_terminates` calls `infer_expr` outside `check_expr`, where `infer_cache` is `None`, so its inferences are uncached and repeated by the check and by nested `if` bodies. Do: a statement-level termination memo keyed by (statement, scope version) within a function check, with the inference shared with the statement's check. The syntactic "can it be never" shortcut is rejected as too narrow (`$x = neverCall()` is itself never).

**E9 name-table lookups (1.27 G self). Was NARROWED (vague).** Do: first the caller breakdown on all nine threads; then `&Name` keys everywhere the checker holds a `Name` (`TableKey for Name` uses the cached lowercase), and a per-file `(class, method)` lookup cache.

**E10 per-function walks (0.5 G). Was NARROWED.** Do: gate each behind `FileFacts`; the ones with per-call state (SQL reflection) are called once per distinct input.

## F. Cross-cutting

**F1 allocation (22 M allocations). Was NARROWED.** Do: the full caller breakdown first (earlier trees were cut), then interned `Name`s in reference sets (with A3), a reused map in `add_uses`, inline small vectors where the counts say so.

**F2 `parse_written_type` memo. Was UNVERIFIED, now measured.** 310,883 calls, 13,198 distinct (4.2 %), 95.8 % hit. Do: one process-wide sharded memo of interned `Type`s, nullable and trim handling inside the memoized function; fits with B8.

**F3 `intern::insert` front cache (0.71 G). Was NOT READY.** Shard lookup is 82 % hits (3.39 M calls, 605 K allocating). Do: write the node-lifetime design first (a front cache keeps nodes alive), then the thread-local front cache verified by exact equality.

**F4 pass the lexer's hash. FACT-REJECT.**

**F5 worker imbalance. Was PROPER.** Do: re-measure after A1 and the D items; one-file work-stealing granularity.

## G. Items previously set aside

**`Lexer::run` capacity. Leave** (measured non-cost).

**`render_json` (24 M). Was NARROWED.** Do: write the diagnostics straight into a buffered writer.

**Stub decode (308 M). Was NARROWED.** Do: decode members on demand (method table by offset). Gate: count of members read against members decoded.

**The 937 M outside the roots. Resolved:** stolen finishing-pass work (see D).

## Pass 2

**S1 lazy context loading. PROPER in intent, design open.** Measured: 435 of 10,504 vendor classes (4.1 %) are looked up during checking. Do after the design: per-class on-demand registry (once-per-file lazy parse, composer PSR-4 and classmap name-to-file map, eager fallback where no map exists); the finishing passes' per-class cost is the open question.

**S2 persistent parsed-declaration cache. PROPER, depends on S1 and on `FileDeclarations` being serializable.**

**N1 skip expression traversal. Was NARROWED.** Do: B1 (1), then (2). Savings about 0.7 G of expression walks and `define_call`; the flag covers `define` anywhere and declarations inside closures and anonymous-class methods.

**N2 flags per visitor. Was NARROWED.** Do: the single `FileFacts` in B1.

**N3 `add_uses` (0.11 G, 360 K allocations). Was PROPER.** Do: `Arc<str>` import paths and a borrowed lowercase key; shrinks again with fewer walks.

**N4 purge mask. Was NARROWED.** Do: E6. The corrected rule (all prefixes, not the first segment) matters only if a mask were used; it is not.

**N5 non-ASCII columns (35 M). Superseded** by B3(c). If B3(c) is not done: validate once and count non-continuation bytes with a boundary guard.

**N6 `Keyword::try_from_bytes` (24 M). Was PROPER.** Do: inside B4's generated lookup.

**N7 `object_class_names`. Was NARROWED.** Do: a visitor-style `for_each_object_class` plus a collecting wrapper; most of the 36 callers stop allocating.

**N8 union memo allocation (60 M). Was PROPER.** Do: two-slot inline form for the binary operations.

**N9 `is_plain_word` (82 M). Was PROPER.** Do: fold into `split_top_level` so the string is scanned once; first-letter test before the `inf`/`nan` words.

**N10 `looks_like_constant_name`. Was PROPER.** Do: last-byte test, exact.

**N11 `check_class_shapes`. Was PROPER after correction.** Do: read the list from `FileDeclarations`.

**N12 `to_path_buf` and key clones. Was PROPER.** Do: A3.

**N13 vector growth. Re-stated.** Do: its real causes: `fmt` string writes (E3), `PathBuf::push` and `join` on main (49 M; find the callers in `collect_php_files` and `Resolver::path`), `lex_single_quoted` (10 M, no reserve); AST vector reserves are minor.

**N14 per-call checker functions. Was NARROWED.** Do: the E9 caller counts first, then decide.

**N15 `InferKey` cache (130 M). Was PROPER (premise corrected: per outermost expression).** Do: keep the map and `clear()` it, shrink after an outlier.

**V1 lazy `dependents`, `signature_dependents` (977 M, serial). Was PROPER.** Do: not stored for the one-shot run; computed on the first incremental update from each file's stored references using `FileId`s (A3). Decide `PackageIndex::owner` once its readers are known.

**V2 `format!` (about 0.7 G). Was PROPER.** Do: E3.

## Order

1. One-line and exact: E3 `truthy`, V1, N15, N10, N6, N8, N9, N7, D3.
2. Index and id changes: A1, A2, A3/A4/N12 with V1, B9, B10, C2, F2 with B8.
3. Parse restructuring: B1/N1/N2/N11 (`FileDeclarations`, `FileFacts`, `Program`), B2, B5, B6, B7, B4, B3 including (c), C1, C3.
4. Type and relation work: E1/E2, E4, E5, F3 after its design.
5. Checker structure: E6/E7 (`LocalKey`, dirty-key merge), E8 (termination memo).
6. Finishing passes and stub cache: D1, D2, one format bump.
7. Measure first, then decide: E9, N14, F1, G's stub decode, S1's design, S2.

---

# Corrections from the item-by-item walk (these override the entries above)

Each item was re-read against the code and the callgrind data, one at a time, in order. Items not listed here were re-read and stand as written.

**A1.** Add a lazily built key index inside large `Shape`s, used by `Shape::field` (`shape.rs:264`, 53 callers); `Clone`/`Eq`/`Hash`/`Serialize` ignore it. Data: `legacy_is_keys_superset` (production code, 40 M self) calls `this.field(..)` for every field of the other shape, which is O(n x m); `shape_parts` adds 9 M. The builder alone fixes construction, not lookup.

**A2.** Build the entity indexes from `own_iter()` (project-loaded classes), not `&self.classes`: iterating the whole table decodes every stub base class. One pass builds both the table index and the repository index (repository FQN to list of entities, ambiguity kept).

**A3/A4.** The path-hashing cost is mostly set growth: `HashSet<PathBuf>::insert` 360 M and `reserve_rehash` 379 M (a rehash re-hashes every member path). `FileId`s remove both. The size of those sets is driven by B2's reserved-word keys.

**B2.** The collector resolves and inserts every word of every doc type as `ns\word` and `word`, including `int`, `string`, `array`, `mixed`. These can never be classes, so the keys are dead and they make the huge sets. Skip them exactly, using one table shared with the type parser (`types::is_reserved_type_word`, case-insensitive) so the lists cannot drift apart.

**B3(c).** Cost to plan for: `Span` is serialized into the stub cache; about 130 sites read `line`/`col`; readers hold bare spans, so they need a `LineIndex`; columns are characters, so the index keeps char offsets for non-ASCII lines; context files need it for go-to-definition and can rebuild it from the file on demand. Side effect: `Span` goes from 16 to 8 bytes, which helps B7.

**B8.** Confirmed as about 40 arms: `apply_tag` becomes a pure `parse_tag(tag, rest)` returning an effect value, memoized by tag text, plus a cheap `apply`. The scratch `DocBlock` and `merge_from` go away for PHPStan-parsed tags.

**B9.** Replace the per-ancestor hashing with a recursive per-directory memo: `owner_of_dir(d)` is the deeper (ties by index) of d's own entries and `owner_of_dir(parent(d))`, run once for the as-written path and once for the normalized one. A maximum over ancestors equals the maximum of per-ancestor maxima, so the existing tie rule is preserved. Shared prefixes are computed once.

**C1.** Beyond parallelizing `Resolver::path`: replace the per-entry `symlink_metadata` stat (12.4 K) with one `read_dir` per distinct directory (existence and file type from the dirents); derive each directory's canonical form from its parent's cached form plus one `readlink` (the run makes 21 K `readlink` calls, all errors, from `canonicalize`); evaluate the shared `__DIR__ . '/..'` prefix once. Measured: 44 K `statx`, 21 K `readlink`, 4.4 K `getdents64` in a 3.98 s run; this is serial on main.

**C2.** `check_phpstan_api_usage` runs inside `finish_diagnostics`, a serial loop on main over every file, after `judge_property_types`. It cannot move into the worker closure (the later sort must see the property diagnostics). Run the whole `finish_diagnostics` pass in parallel with an ordered collect, and gate the walker by the existing `mentions_phpstan` flag (a superset, since references over-collect). Differential test: gated against ungated over the fixtures and the corpus. The walker also accounts for 21.5 M of `lookup_method_with_owner`.

**D1.** Store the stub verdict on the `FunctionReflection` where `is_stub` is set (it is a function of `params` alone), so cache stubs and project stub directories are both covered. The fixpoint in `resolve_transitive_purity` re-evaluates every classified key each round; use a reverse-dependency worklist that recomputes only dependents of a key whose verdict dropped.

**E2.** The intersection arm is pure: `No` if any member is `No`, `Yes` if all are `Yes`, else `Maybe`; a fold with early exit on the first `No` is exact. The structural-relation memo should be a new `Operation` in `intern::memoized`, not a second mechanism, and N8 (no per-miss `Vec`, no extra clones) is a prerequisite.

**E4.** `Type::union_of(Vec<Type>)` is itself `reduce(Type::union)`, a pairwise fold with 124 callers and 59 inline reduces. Each step is a distinct (accumulator, next) pair: it misses the memo, stores a keep-alive entry and hits `MEMO_CAPACITY` (65 K), which clears the memo. Memo-map operations alone are 148 M, plus 47 M in the memoized closure and 23 M in `Type::union`; `Type::union` also clones both arguments for the key and again for `kept`. Proper: a genuinely n-ary union (flatten once, dedupe by node identity, apply the absorption rules in one pass, fall back to the fold only where rules interact), with the old left fold as the test oracle over random member lists. Do this before caching `array_value_type`; the shape-level cached summaries stay.

**E9.** No single dominant caller. `lookup_method_with_owner` 206 M: `generic_object_method_return_type` 38 M, `find_method_signature_prototype` 30 M, `forget_members_a_call_may_change` 24 M, `static_call_method` 23 M, `phpstan_api::Walker` 21.5 M (gone with C2's gate). Class `get_with_name` 85 M: `find_class` 27 M, `is_subclass_of` 20 M, `canonical_class_name` 20 M. Proper form: `&Name` keys plus a per-file `(class, method)` cache, then re-measure.

**E11 (new). `forget_members_a_call_may_change` (150 M inclusive in thread 03).** Runs after every call, resolves the callee to compute `writes`, and only then asks whether any local depends on the receiver or argument keys. An early return on `may_have_member_narrowing` is not exact: `purge_stale_narrowing_for` also clears container-field, branch-implied and discriminant facts. Proper: with E6's root index, compute the candidate keys first (no allocation), ask the index whether anything depends on them, and only then do the callee lookups; later, share the resolution the call check already did.

---

# Corrections from the second half of the walk (E10 to the pass-2 items)

**B5.** The `String` made by `from_utf8_lossy(..).into_owned()` in `read_and_parse_file` is needed only to index use sites (LSP) or when `retain_source` is on; the parser reads `bytes`; the CLI needs only the length for the size sort. Do: drop the string on the CLI path and keep `source_len`; where a string is needed, `String::from_utf8(bytes)` moves the `Vec` and the lossy decode runs only on invalid UTF-8. Removes all 394 M, not part of it.

**B6.** The doc-comment maps are touched once per doc comment, not per token; the 718 M is moving 6.9 M 72-byte tokens into three vectors plus a `Vec<Option<String>>` of `keyword_raw` (24 B per token, almost all `None`). Do: `keyword_raw` becomes a sparse sorted side table read only for keyword-shaped identifiers; the lexer emits the final vectors and the doc-comment side table; `Parser::new` becomes a zero-copy wrapper.

**E10 (re-split).** (a) `scan_program_for_rebind_targets` runs two whole-program walks per file (`ClosureVariables`, `Rebinds`); the result is empty unless the file has a `bind`, `bindTo`, `call` or `Closure::bind` call: a `FileFacts` entry, and fuse the two walks into one that collects candidate calls and closure-variable names and resolves at the end. (b) `var_tag_nodes` is called on the same doc text from four sites (`stmt.rs:155`, `stmt.rs:371`, `phpdoc_type_compat.rs:597`, `var_tag_types.rs:287`): one memo by doc text, in the B2/B8 family. (c) `Promotions` is a per-file walk: gate it by `FileFacts`. (d) `check_one_method_override` is real Liskov work, not a walk: removed from E10. (e) `resolve_sql_row_type` (1,224 parses): memo by (query text, schema version); first count the distinct queries.

**F1 (breakdown done for threads 01, 03, 04, a lower bound).** Allocation callers: `finish_grow` 271 M (2.2 M calls, mostly `fmt` string writes, E3); `String::clone` 56 M (404 K `register_references`, 174 K imbl `make_mut` cloning String keys in the persistent locals map, 118 K `merge_scopes`, then phpdoc `TypeNode` clones); `to_path_buf` 28 M (A3); `intern::insert` 22 M (F3); `add_uses` 16 M (N3); `HashMap<String, Type>` rehash 15 M; `Vec<TrinaryLogic>` 14 M (E2); `format_inner` 13 M; `object_class_names` 7 M (N7). Each has an owner item; the imbl String-key clones and `merge_scopes` clones are removed by E6/E7 with interned keys.

**F2.** The existing `phpdoc::memo::Memo` (sharded, bounded) is the right structure but sits in `phpdoc`, which `types` cannot depend on, and its limits (256 entries x 64 shards) are about the size of the 13,198 distinct strings, so uneven shards would clear and thrash. Do: move `Memo` to `syntax` (both crates depend on it), make the limits constructor parameters, use it for `parse_written_type` and for the doc memo.

**F3.** Do: a direct-mapped thread-local front cache of strong `Arc<Node>` (for example 4096 slots), probed after `exact_hash` and before the shard lock, verified by `exact_eq`; a hit returns the canonical node. It extends the lifetime of a bounded number of nodes only. On a hit the caller's moved-in `Repr` (with the child `Arc` clones) is still dropped: a borrowed-construction path removes that later. The front-cache hit rate must come from a prototype.

**F5.** Not a later re-measurement: the critical path is the finishing passes. Thread 02's serial root is 3.13 G: `resolve_transitive_purity` 722 M, `closure_scope` 673 M (no `par_iter` in its two calls), `infer_global_variables_of` 399 M, `warm` 192 M, `end_declaration_changes` 173 M, `flatten_phpdoc_inheritance` 141 M, and `Cloned<Flatten<HashSet<PathBuf>>>` 139 M (the `dependents` sets cloned into `affected`, i.e. B2's reserved-word keys on the critical path). With parse parallel before and check parallel after, the finishing passes are an estimated 40 % of the wall path although only 9 % of instructions. D1-D3, B2 and A3 move to the front of the order.

**F6 (new). The allocator's aligned path.** Allocator symbols total 3.13 G self (about 7 % of the run). `mimalloc` 0.1.52 always calls `mi_malloc_aligned`, `mi_zalloc_aligned` and `mi_realloc_aligned`, even for alignment <= 16: `mi_theap_malloc_aligned` 698 M, `mi_malloc_aligned` 194 M, `mi_realloc_aligned` 72 M, plus 208 M in the aligned `_generic` and `overalloc` slow paths. Do: a small `GlobalAlloc` shim sending alignment <= 16 to `mi_malloc`, `mi_zalloc` and `mi_realloc`, others to the aligned calls (precedent for allowed unsafe at `lib.rs:35`; a direct `libmimalloc-sys` dependency). Gate: A/B wall-time (hyperfine, warm cache), since instruction counts and wall time can differ.

**G.** `render_json` (24 M) and `Lexer::run` capacity stand as written. Stub decode (308 M): the measurement is members read against members decoded per decoded class; only then is a per-member format worth a cache version bump.

**N14.** Not "not actionable": `call_template_bindings` builds a `HashMap<String, Type>` by repeated `insert` (196 K inserts, 46 M inclusive, with rehash growth and cloned string keys). Do: reserve from the template count and key by a shared name (the template parameter's own name) instead of an owned `String`.

**S1 and S2 become one design: a resolved vendor base.** Vendor classes depend only on vendor code and the stubs, so their fully resolved form (hierarchy, flattened traits, PHPDoc inheritance, purity and property-write verdicts) can be cached as the stub cache caches builtin stubs, and `NameTable` already has the `base`/`hidden` layering for it. Key: `composer.lock`, the installed packages' paths and mtimes, and the build hash. Load: decoded lazily, like the stub base. Per run: only project files are parsed and finished. This removes the ~17 G of context parsing and a share of the finishing passes (the critical path), and it makes S1's lazy loading unnecessary. Open design questions: vendor function bodies that purity reads become stored verdicts; conditional declarations and the analysed-wins collision order; project-defined classes that vendor code references; duplicate-declaration checks across vendor and project; the incremental/LSP `dependents` index.

**Order change.** Because the finishing passes sit on the critical path, move D1, D2, D3, B2 (reserved words), A3/V1 and F6 to the front, then the resolved vendor base design, ahead of the parse-phase restructuring.

---

# Correction found while implementing

**V1 (lazy `dependents` and `signature_dependents`) is invalid and dropped.** The claim that a one-shot run never reads them rested on `run_finishing_passes` taking the `Scope::all()` branch on a first run. It does not: `has_resolved_declarations()` is true because the stub base counts as resolved, so `finishing_scopes` runs `closure_scope` over both reverse indexes on every run (the profile shows `closure_scope (2x)` in the Piwigo run). The indexes are read on every run, so they cannot be built lazily on the first incremental update. A3's `FileId` change (integer sets, a table keyed by id) stands on its own merits, and B2's removal of reserved-word keys shrinks the sets it hashes. What `closure_scope` costs after D2 and B2 has to be re-measured before deciding on a further change to it.

**D1 as implemented.** The builtin verdict is cached once per signature in a cell that no equality or clone carries (computed from the final resolved parameters, so project stub directories are covered too); storing it in the stub cache would have saved about 7 M more. The property-write fixpoint and the purity fixpoint are both worklists now.

---

# Implementation status (end of the implementation pass)

Piwigo scoped run, callgrind, all threads summed: 45.13 G at the start, 25.14 G now. Output is byte-identical to the baseline (`LC_ALL=C sort | md5sum` = 6e5fa444...). Full `cargo test --workspace` and `--test fixtures` (3,807) pass. `hyperfine -N` against the earliest base binary under light load: 4.0 s to 2.6 s.

**Done in this pass (after the earlier commits):** shape relation by keys first with early exit; key-disjoint without making the key type; key lookup hint (`field_near`); structural relation memo (per thread, by pair of nodes); union merge of shapes holds `Arc<Shape>` and memoizes `is_keys_superset` per pair; absorb pair rejects by form (differential test); `Expr` 136 to 96 bytes and `Stmt` 384 to 184 bytes (box closure, arrow fn, static target names, function and class declarations, foreach operands); lexer position fast path in any file and in fragments; comment skipping by search; first-byte dispatch in `next_token` and table identifier scan; columns by continuation bytes; interner table as one keyed table with dead-entry reuse; per-lexer name cache; ignore-comment marker prefilter; parser `check` by discriminant first; `is_real_variable_key` by bytes.

**Measured and reverted or not built:**
- Registry-scoped relation memo: no gain (lookup cost equals the leaf comparison it saves); reverted.
- Incremental union / "absorbed" bit: non-shape union work is about 0.14 G; not justified.
- B8 split of `apply_tag`: the scratch-block merge overhead is about 0.17 G; not worth the restructuring.
- B3(c) byte-offset spans: `line_col_at` is now a few instructions on the common path; the full `Span` change (about 260 sites) is not justified by what is left.
- E9 per-class method lookup cache: the three-table lookup in `lookup_method_with_owner` is about 0.3 G and returns references into the registry; a cache needs version keys and is not done.

**Not done (architecture, not measurable by instruction count or too risky without a design):**
- C3 pipeline (parse while `prepare` runs): changes wall time, not instructions.
- S1 lazy context loading and S2 persistent parsed-declaration cache: `ClassReflection::from_def` is 1.86 G inclusive (778 M in `FunctionReflection::from_decl`, 576 M in `parse_shared`) and 4 % of vendor classes are looked up, so lazy per-class reflection is the largest remaining win, but the finishing passes iterate every class, so it needs its own design.
- C1 remainder (autoload file parse on a worker, per-directory listing), stub member on-demand decode, N14, F1 allocation breakdown, `render_json` buffered writer: not started.

Lesson recorded: the first version of the interner's hashbrown table regressed name lookup 1.0 G to 2.0 G (dead entries piled up in a hash's chain); caught by the checkpoint callgrind, fixed by reusing the dead entry.

---

# Implementation status, second pass (lazy vendor context)

Piwigo scoped run, callgrind, all threads summed: 25.14 G before; 21.35 G with `lazy_vendor_context` on (the default); 20.08 G with `lazy_vendor_context = "declarations"`. Output is byte-identical to the baseline in all three. `cargo test --workspace` (1,858) and `--test fixtures` (3,807) pass.

**Measured first (instrumented run, not committed):**
- 441 of the 7,203 loaded context files have a class touched by name while checking; the finishing passes touch nearly every loaded class.
- The checker reads no vendor body (0 body reads while checking); only the purity and property-write passes read bodies, in the finishing passes.
- Purity follows `$this`, `self`, `parent` and plain functions only, so it never depends on other classes. The property-write pass follows `Foo::bar()` and typed-receiver calls into other classes.
- Vendor-body strings brought in 2 of the 7,203 files; the 1,300 files that strings bring in come from the analysed files' strings, which stay.
- Context file counts: 7,201 (default), 5,755 (`"declarations"`), 11,459 (every file).
- Method lookups: 813,653 calls, 12,912 distinct pairs, 45,011 counting each file apart.
- 55% of tokens (3.41 M of 6.14 M) are inside function and method bodies; doc comments are 28% of the source bytes (101,632 comments, 51,095 distinct texts).
- Loops: 83% converge in one pass, about 1.3 passes on average.
- 31,660 diagnostics are emitted and discarded in the 223 functions that call `extract` or `eval`; the flag is known only after the function is checked, so it is left.

**Done:** lazy vendor loading (class map and PSR-4 lookup, closure in rounds, insertion in path order); the namespaced class strings of a file count as names (analysed files' all, vendor files' outside bodies); `lazy_vendor_context = "declarations"` opt-in; a per-checker method lookup memo; doc comment end by `memmem`.

**Not done:** S2 (persistent parsed-declaration cache), lazy per-method reflection building, lazy bodies, loop pass reuse, skipping the diagnostics of functions that will be discarded.

**Revised after review:** a setting that can silently drop findings is not wanted. The `"declarations"` mode is removed. `lazy_vendor_context` is now off unless set to `true`, since what is not loaded is not seen: a class the autoloader cannot find by a name (one that shares a file with another, a function in a file the autoloader does not list, a vendor class that extends one of the project's) is missing from what the checks read. With it off a run loads every file as before (about 25.1 G on the Piwigo run); with it on, 21.35 G and the same diagnostics.

**Removed after review:** lazy vendor loading is removed altogether (setting, loader, named-autoload discovery, string collection), since what is not loaded is not seen and a run that can lose a finding without saying so is not wanted in any mode. A run loads every vendor file as before; the Piwigo run is about 25.1 G instructions, with the method lookup memo and the doc comment scan kept. The measurements it rested on (441 of 7,203 context files touched while checking, no vendor body read while checking, 45,011 distinct method lookups per file) stand if a design that loses nothing is wanted: it would have to make the finishing passes work per class, and a class would load only when a lookup asks for it.
