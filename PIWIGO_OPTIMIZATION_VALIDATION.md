# Adversarial validation of PIWIGO_OPTIMIZATION_PLAN.md

Method: for each item in order I tried to break the claim: re-read the source it points at, checked whether the proposed change preserves behaviour, checked whether the number could be explained another way, and ran a query against the callgrind files or the corpus where one could settle it. Verdicts:

- CONFIRMED: the claim holds as written.
- CONFIRMED, CHANGED: the cost is real, but my fix or number needed correcting.
- PARTIAL: part holds, part does not.
- REJECTED: the claim or the fix is wrong.
- UNVERIFIED: the evidence in hand cannot settle it; the item says what would.

The plan file is not edited; this file supersedes it where they differ.

## A. Quadratic or whole-table scans

**A1 `Shape::set` quadratic. CONFIRMED, CHANGED.**
- Settled by the corpus, not only the profile: a scan of every `const X = [...]` in the 14.2 K loaded files found `PhpStormStubsMap.php` with arrays of 8,137, 5,101 and 1,479 items, `symfony/mime/MimeTypes.php` 1,874 and 1,848, the league and guzzle mime maps about 1,300 each. Sum of N^2/2 key comparisons is about 52 M; at about 50 Ir each that is about 2.6 G, which matches the 2.66 G self cost. The cause is real and it is vendor code (loaded for context, never reported).
- Correction 1: `values.contains(&value_ty)` (the second quadratic) only matters when the array turns out not exact: in the exact case `values` is never read. Fix: push every value without a membership test, and dedupe once at the end only when `exact` is false.
- Correction 2: a hash index is not needed to beat O(N^2) when every key is auto-indexed or distinct literal; but explicit duplicate keys are legal PHP, so keep an index (or a final stable dedupe, last wins).
- Hazard the plan understated: a shape with thousands of fields is then cloned and compared downstream (`array_value_type`, `Shape::key_type`, equality); see E4.

**A2 `repository_entity` scan. CONFIRMED, CHANGED.**
- The scan at `hierarchy.rs:1221` and the single call site at `query_builder.rs:219` are as described, and a lazily built index with `reset_lazy_tables` already exists for the table-name case (`doctrine_entity_table_index`, `registry.rs:257,744`).
- The index must store ambiguity: the function returns an entity only if exactly one class names this repository (its comment documents a real Piwigo case with two). So the map is repository -> one-or-many, not repository -> name.
- A cheaper alternative worth stating: cache the answer per `current_class` in the `Checker`, since all 695 calls come from `$this->createQueryBuilder()` inside a few repository classes.

**A3 `register_references` path hashing. CONFIRMED, CHANGED.**
- Read at `project.rs:2808`: `entry(name.clone()).or_default().insert(path.to_path_buf())` for each of 404,459 references. The `Path::hash` cost is component-wise over a long absolute path.
- New finding: `entry(name.clone())` allocates a `String` for every reference even when the key exists. `String::clone` from `register_references` is 38.4 M in the caller tree. Fix: `get_mut(name)` first and clone only on first insert. This is a trivial sub-item (A3d) independent of file ids.
- Scope of file ids (option 1): about 41 `dependents`/`signature_dependents` mentions and 16 `HashSet<PathBuf>` sites, about 12 in `project.rs`. Mechanical, but ids must never be reused (an update/remove cycle) and emission order must stay as today.
- Wall-time note: this runs serially on the main thread, so its wall effect is larger than its 2 % of instructions.

**A4 `closure_scope` path hashing. PARTIAL.** File ids in `dependents` do not remove `self.files.get(file)` (the 158 M `Path::hash`), because `files` is keyed by `PathBuf`. It needs `files` keyed by id (a `Vec<FileState>` or id map), which is larger than A3.

## B. Parse phase

**B1 merge the per-file walks. PARTIAL (number overstated).**
- Four of the walks (`declared_names`, `declared_constants`, `DuplicateClassSites::index`, `DuplicateFunctionSites::index`) all call `for_each_declaration`, as does `build_declarations`; they can share one traversal producing a list of declarations. About 1.0 G.
- `referenced_names` and `declaration_referenced_names` are the same `Collector` with `bodies` true and false (`deps.rs:18,32`): the second is a strict subset of the first. One walk with two sets (names found outside bodies go to both, names inside bodies only to the full set) removes the second walk, about 1.0 G.
- `annotate`, `name_anonymous_classes`, `mentions_stub_attributes` and `scan_parsed` are not mergeable: some are mutable visitors that rewrite the tree, some run in other crates. Realistic total about 2 G, not 4.6 G.

**B2 `doc_names`. CONFIRMED, CHANGED (and one suggestion REJECTED).**
- New finding: a function's or class's docblock is processed twice with identical result. `visit_stmt` calls `doc_names(stmt.doc_comment)`, then `visit_function_decl`/`visit_class_def` calls it again on the declaration's copy of the same text; the parser stores the doc on both (`parser/stmt.rs:30`, `parser/functions.rs:25`, `stmt.rs:127-129`), and `templates_of` splits it a third time. The context is identical, so skipping the statement-level call for function/class statements, and splitting each docblock once, changes no result.
- REJECTED: "reuse the docblock memo". `doc_names` deliberately over-collects from raw tag text without parsing it (`deps.rs:10-17`); parsed `DocBlock`s would change which names are collected.
- New: `tag_type_tokens` yields scalar words (`int`, `string`, `array`) which are then resolved and inserted as `ns\int` keys. Words that cannot be class names (the reserved type keywords) can be skipped; it needs a count of how many tokens that removes before claiming a gain.

**B3 lexer positions. PARTIAL.**
- The profile is of HEAD, so `line_col_at` is already the P5/P6 version: 56 Ir per call. The remaining saving from fewer calls is small.
- (a) `memchr` for `compute_line_starts` is sound; `memchr` 2.8.3 is already in `Cargo.lock` but is not a dependency of the syntax crate, so it is a new direct dependency.
- (b) REJECTED as written: "reuse the previous end as the next start" is wrong when whitespace or a comment lies between tokens. The valid variant is computing a token's end from its start plus its byte length when the token cannot contain a newline.
- (c) lazy line/column is a large cross-cutting change; not validated, treat as a separate design.

**B4 `next_token`. CONFIRMED.** Every token calls `starts_with("?>")` and `starts_with("#[")` first (`lexer.rs:371,387`); each is a slice plus a libc `memcmp` call for two bytes. A first-byte test removes both. `lex_ident_raw` returns a `Cow`, so there is no allocation there (a worry I checked and dismissed).

**B5 `from_utf8_lossy`. CONFIRMED, CHANGED.** `read_and_parse_file` builds `source` with `from_utf8_lossy(&bytes).into_owned()` for every file but parses `&bytes` (`session.rs:576-582`); the `String` is only kept when `retain_source` is on and for `index_use_sites`. Use `String::from_utf8(bytes)`-style validation without the copy, or avoid creating it unless needed. The identifier path needs `std::str::from_utf8` (fast ASCII path) because identifiers may contain bytes above 0x7f.

**B6 `Parser::new`. PARTIAL.**
- Measured size: `(Token, Span, Option<String>)` is 72 bytes (Token 32, Span 16); 6.9 M tokens are moved twice (the lexer builds the vector, then `Parser::new` rebuilds three vectors): about 500 MB of copying, which explains the 718 M.
- REJECTED: "doc-comment maps become sorted vectors" because `take_doc_comment` calls `remove` on the map (`parser/tokens.rs:404`).
- Valid: have the lexer emit the filtered parallel vectors directly and keep doc comments as a side table.

**B7 parser moves. CONFIRMED, CHANGED (now measured).** `size_of::<Stmt>()` is 472 bytes, `Expr` 136, `TypeHint` 64. Every `Result<Stmt, _>` return moves 472+ bytes, which fits about 35 Ir per memcpy and 24.6 M calls. Boxing the large `Stmt` variants (class, function, trait/enum bodies) is the concrete fix; `Stmt` is the item to shrink first.

**B8 `from_decl`. CONFIRMED, CHANGED.** My claim "two `declared_type` calls per parameter" is wrong: 123,465 calls = 63,296 parameters plus about 60 K returns (one per function). The docblock numbers (94,228 gets, 39,718 parses, 58 % hits) are from the report and consistent. The `apply_tag_recording_errors` scratch-block cost is real; its split is still 40 arms of churn.

**B9 owner cache. CONFIRMED.** `owner()` depends on the ancestors of the file (and of its normalised form), and a file path is never itself a package directory key, so the answer depends only on the parent directory; caching by parent is exact. It needs the package index, which `prepare` has built before `load`, so it can run on the workers.

**B10 sorting. REJECTED as written.** The sorts at `session.rs:600,606` order files; that order is the load order and decides first-wins collisions. A byte-order sort differs from component-wise order (`a-b` vs `a/b`), so it could change output. Only an order-preserving speedup is acceptable; the gain (about 0.2 G, serial) is small.

## C. Main thread

**C1 `prepare` overlap. PARTIAL (the overlap is wrong).** The vendor context supplies the scan directories used to find the files, so file discovery depends on it; it cannot overlap. What remains valid: `Resolver::path` (12,399 calls) and the `dedup` can be made cheaper or parallel inside `prepare`.

**C2 `check_phpstan_api_usage`. CONFIRMED.** Caller tree of thread 01: called from `Project::check_files` 2,792 times, 122 M, serial. It is a pure function of `(&file.ast, &registry, package name)`, so it can run in the per-file worker closure.

**C3 start the parse pool earlier. REJECTED** (same dependency as C1).

## D. Finishing passes

**D intro claim "the other eight workers are idle". REJECTED/UNSUPPORTED.** The passes use `par_iter` internally, so most of their work is stolen by other workers; that is the likely home of the 937 M "outside the three roots". The cost on thread 02 is only the root.

**D1 `mentions_any`. CONFIRMED, CHANGED.** `stub_verdict` is called at each lookup of a builtin's property writes (`property_writes.rs:202,210,224`), not once per signature, so 230,680 calls repeat the same verdict for about 5 K builtins. Memoize the verdict per signature or name; that beats the flag pre-filter I proposed.

**D2 stub-file closures. CONFIRMED.** 2,053 lazy closure builds at about 149 K Ir each. Better than an SCC at runtime: compute them at stub-index build time and store them in the cache (format bump), so the CLI pays nothing.

**D3 `infer_global_variables_of`. CONFIRMED.** The per-file walk is a pure map over files with a read-only registry (`global_vars.rs:212-227`); `par_iter` with an indexed collect preserves order.

**D4 overlap passes with checking. REJECTED:** checking needs the finished registry; nothing in the profile supports an earlier start.

## E. Check phase

**E1 `Type::concat` refinements. CONFIRMED.** Read the code: concat calls `is_refined_string` ten times plus more for numeric (`concat.rs:76-120`); 456,975 / 46,524 is 9.8 `is_super_type_of` calls per concat. A refinement mask read straight from the `Repr` (bare `string`: none; literal: from its bytes; intersection: its accessories; union: AND of members; anything else falls back) removes them. Keep the old path as the differential oracle.

**E2 `Vec<TrinaryLogic>` per intersection. CONFIRMED, CHANGED.** The `Vec` is the `(Repr::Intersection(members), _)` arm of `is_super_type_of`: exactly what concat's `string & accessory` probes hit. So E1 and E2 are largely the same cost; the plan double counts. Folding without the `Vec` is trivial and safe, and returns at the first `No`. A `(sup, sub)` memo is only sound for the context-free `NoClasses` relation.

**E3 `format!` hot paths. CONFIRMED.** `truthy({name})` at `narrowing.rs:455` is built per local; it can be avoided by comparing `strip_prefix`/`strip_suffix` or by building it inside the `if`. `resolve_as`'s `format!` is real; see B2 for the skip-keywords idea.

**E4 `array_value_type`. CONFIRMED, CHANGED.** The cost is `reduce(Type::union)` over every field (`queries.rs:869`): a pairwise reduce is quadratic in the member count (each step flattens and absorbs against a growing union), on top of the union memo. Fix: one n-ary union (dedupe by node identity first), or a cache keyed by the `Arc<Shape>` pointer (sound: the cache keeps the `Arc` alive). This is the same family as A1's huge shapes.

**E5 template inference guards. CONFIRMED, WITH CONDITION.** Skipping is exact for `infer_template_bindings` when neither type has `HAS_TEMPLATE_REF`. For `substitute_templates_resolving_to_bounds` the guard must also test `HAS_LATE | HAS_DEFERRED` because `settle` rewrites those without a template.

**E6 `purge_stale_narrowing_for`. CONFIRMED, CHANGED (my own omission).** My earlier version builds the subscript text once but then calls `k.contains(subscript)` for every local without the cheap `'['` test that `key_depends_on` has. Adding `k.as_bytes().contains(&b'[')` first restores it. The reverse index by root variable is a large change I do not recommend before this one-line fix is measured.

**E7 `is_real_variable_key`. CONFIRMED, CHANGED.** `Locals::insert` calls it on every insert only to set `may_have_member_narrowing`; test the flag first (`if !self.may_have_member_narrowing && !is_real_variable_key(&key)`), then the check is skipped after the first member key. A byte loop is exactly equivalent to the `chars()` loop. The "cache as a flag in the key" idea is REJECTED (a key newtype touches every caller). `merge_scopes` with imbl `diff`: UNVERIFIED (I did not read its body).

**E8 `always_terminates`. UNVERIFIED, agree it needs a measurement first** (hit versus miss counts of the infer cache).

**E9 name-table lookups. UNVERIFIED/VAGUE.** The numbers are from the lib-suite table, not this run; no call-count breakdown by caller exists, so there is nothing actionable yet.

**E10 other walks. WEAK.** The claim that those walks can be folded or run once per file was not checked; they have per-call state (SQL reflection).

## F. Cross-cutting

**F1 allocation / `String::clone`. UNVERIFIED except one source.** The caller trees are cut at the profile threshold, so only `register_references` (38.4 M, see A3d) can be attributed. A full caller breakdown (rerun `callgrind_annotate --threshold=100`) is needed before the `Arc<str>` proposal.

**F2 memo for `parse_written_type`. UNVERIFIED:** needs the count of distinct type strings.

**F3 `intern::insert`. PLAUSIBLE, NOT VALIDATED:** a front cache that holds nodes alive changes node lifetimes; needs a design note and a hit-rate count.

**F4 pass the hash from the lexer. REJECTED.** `Interner::name` hashes the text once and hands the hash to the table; the table keys buckets by that `u64`. There is no double hashing of the text.

**F5 worker imbalance. PARTIAL.** Real (3.4 G to 8.5 G per worker) and largely explained by A1's concentration on thread 04; files are already sorted by size descending (`session.rs:688`), so no further action until A1 is done.

## What changes in the plan

Strike: B10, C3, D4, F4; B3(b); the "reuse the docblock memo" idea in B2; the sorted-vector idea in B6; the "idle workers" claim; the two-per-parameter claim in B8; the key-newtype idea in E7.

Add: A3d (`get_mut` before cloning the key), B2 (skip the duplicate statement-level docblock pass), B7 (shrink `Stmt`: 472 bytes), D1 (memoize the verdict per signature), E6 (the `'['` prefilter), E7 (flag first), A1's second correction (skip `values` dedupe when exact).

Re-rank by confidence: A1, A2, A3d, B2, B4, B5, B9, C2, D1, D2, D3, E2, E3, E6, E7 are verified and small to medium. B1 (about 2 G), B6, B7, E1, E4 are verified but larger changes. Everything marked UNVERIFIED needs its stated measurement first.
