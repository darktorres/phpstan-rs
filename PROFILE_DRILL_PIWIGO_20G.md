# Callgrind drill-down of the 20.57 G Piwigo run, after the 25 G profile's work

Profile: `~/.cache/prs-profile/cl-20g/cg.out-01..10` (ten threads), copied from the scratchpad of the session that built `PROFILE_DRILL_PIWIGO_25G.md`'s designs. Binary `cl-20g/prs-dhat`: the `dhat` profile with frame pointers, code of `5fb858c9` (HEAD's code). Command `check-project --config phpstan-rs.toml --format json` in `/home/torres/piwigo17-frozen`, `PHPSTAN_RS_NO_FILE_CACHE=1`, warm stub cache. Total **20,570,586,331 Ir**. The same directory holds that session's sampling profile (`perf.data`, 3,322 samples) and build log. Percentages are of 20.57 G unless marked.

Nothing was built or run for this report. Everything below is read from these two profiles, from the other session's dhat heap profile of the same code (`target/dhat-heap.json`, §8), from the 25.14 G profile (`cl-final`) and its line-tables twin (`lt`), and from the code at HEAD. Part I compares what was built with the 25 G report's designs, step by step (§2), and drills down the heap (§8). Part II is the plan: twenty items in the order they are built, validated once at the end; §13 outlines the audits that shaped it.

# Part I. Drill-down

## 0. Method, and what can be compared with the 25.14 G run

- Tables: the scripts of `~/.cache/prs-profile/drill-scripts/` (`parse.py`, `q.py`, `sites.py`, `selflines.py`), plus four new ones in this session's scratchpad:
  - `attrib.py`: self cost under a root, propagated by edge shares;
  - `thr.py`: callees on one thread;
  - `byfile.py` and `lines.py`: self cost by source file and line after inlining;
  - `stolen.py`: finishing-pass work stolen by other threads.
  
  Self costs sum to the total exactly.
- **The two runs were built differently, so totals do not compare exactly.**
  - `cl-final` (25.14 G) is the `gate` profile: 16 codegen units, no frame pointers.
  - `cl-20g` is `dhat`, which inherits `release`'s single codegen unit, with `-C force-frame-pointers=yes`.
  - Frame pointers add a prologue and epilogue to each non-inlined call. This run makes 362 M calls into non-libc code, so at 2-3 Ir each that is **+0.7-1.1 G**.
  - One codegen unit inlines more, which pulls the other way by an amount no data here measures.
  - So "25.14 → 20.57 G" (WORK_STACK level 7, the 25 G report's status) mixes code changes with a codegen change of perhaps ±1 G. A `gate` run of HEAD would settle it; none was made.
- What compares soundly:
  - phase totals, as long as differences of a few percent are not read as code changes;
  - call counts of functions that exist in both runs;
  - allocator and `mem*` call counts;
  - **self cost by source file and line after inlining.** Both this binary and the 25 G line-tables twin carry line tables, so a source line's cost does not depend on which function it was inlined into. The twin's total is 25.19 G.
  
  Function-level self costs move between functions with inlining (`converge_loop_body`, for one, no longer exists as a function), so this report compares by inclusive cost of stable entry points, by call counts, and by source line.

## 1. Where the 20.57 G goes

| phase | 25.14 G run | 20.57 G run | change |
|---|---|---|---|
| Parse job, workers (with the per-file path work then on the main thread: owner, absolute path, file id, 164.2 M) | 11,018 M | **9,968 M** (`read_and_parse_file` 9,811.2 + owner 69.1 + absolute path 54.1 + file id 33.5) | −1,050 M |
| Checking, workers (`run_checker` + PHPStan-API walker) | 10,449 M | **6,716 M** (6,606.9 + 109.0) | **−3,733 M** |
| Finishing passes (their thread, plus work other threads steal) | 2,737 M (1,516.7 + 1,220.5) | **2,481 M** (2,003.5 + 477.2) | −256 M |
| Main thread, serial | 1,080.5 M | **1,064.2 M** | −16 M |
| Autoload-file thread (new) | (on the main thread) | **321.4 M** | moved |

Each thread's total:

| thread | Ir |
|---|---|
| 01 (main) | 1,064 M |
| 02 | 4,243 M |
| 03 | 2,204 M |
| 04 | 1,875 M |
| 05 | 2,247 M |
| 06 | 3,126 M |
| 07 | 2,384 M |
| 08 | 1,867 M |
| 09 | 1,240 M |
| 10 (autoload files) | 321 M |

**Wall time follows the serial stretches, not the Ir.** The sampling profile (1.61 s of samples) has this shape:

| window | busy cores | what runs |
|---|---|---|
| 0.1-0.5 s | up to 7.3 | parse |
| 0.5-0.6 s | 1 | serial insert |
| 0.6-1.2 s | 2.1 on average (1.28 core-seconds in 0.6 s) | finishing passes |
| 1.2-1.6 s | 7-7.8 | check |

That run was most likely made with the per-file cache on. Its duration matches the 1.79 s reported for the cached run, and its insert window lasts 0.1 s, too short for the 756 M of serial insert this run's cache-off callgrind has. Either way, the finishing passes are about 37% of the wall time while holding 12% of the Ir. Their thread is busy for ≈0.54 s of the window, and 0.41 s of that is outside any parallel section: serial. Purity and property writes take 0.19 s of it, and the other passes' serial parts 0.22 s (§10 item 4).

The session that built the designs reported 2.20 s cache-off at `1892b477` and 2.48 s cache-off at HEAD. Ir fell between those two points, so the serial regression of §4 (R1) is the likely cause. The two timings were taken under different loads, so this is consistent with R1, not proved.

## 2. What was built, against the 25 G report's designs

Each row takes an item's steps from the 25 G report's Part II (§11-§14), the commits since `e3536b42` that built them, what was left out or built in another form (checked in the code at HEAD, not taken from the status lists), and what this profile measures (§0's comparable units).

The "shortfall" column says where a gap comes from:
- **design**: the 25 G design itself specified the costly form;
- **impl.**: the code departs from the design, or builds part of it;
- **not built**: the step was not started.

### Parse phase

| item | built (commits) | not built, or built otherwise | measured, 25.14 G run → this run | shortfall |
|---|---|---|---|---|
| P1 | both steps (`67bd3969`) | — | `annotate` 431.4 → 0 | — |
| P2 | both steps (`2bf1d99c`) | — | `scan_parsed` 117.9 → 2.4 | — |
| P3 | `0a787410` | — | `str/validations.rs` 319 → 130 M; the rest is the read (29.8), `UseMap::get` (20.5, a site the design did not list), `ShapeKey` (13.5) and stub-cache decoding (≈35) | — |
| P4 | steps 1-6 (`5091f8fc`, `0acbfc93`, `5c44312d`, `230dd95b`, `f0ee312d`) | — | positions −≈700 M; `LineIndex::new` +270.4 M for every file (R4) | **design**: §11.4 has the lexer build the whole index, column tables included, for every file. Context files need line starts (duplicate-declaration messages, anonymous-class names, the LSP), but columns only for analysed files' diagnostics |
| P5 | steps 1-3 (`8eee829f`, `3b7e4d95`, `9170b36a`) | — | ladder 1,286 → `parse_expr_bp_inner` 516.9 M | — |
| P6(a) keywords | `688d021d` | — | 98.4 → 127.2 M | **design**: the probe's `eq_ignore_ascii_case` is byte-wise, and `lex_word` compares again |
| P6(b) literals | `aab273f3` | — | gone from the top | — |
| P6(c) names across files | `42e80071` | — | interner probes 133.5 → 75.6 M, but the name path is 375.7 → 519.4 M inclusive (R12: second hash pass, P10's watch list scanned per new name) | impl. |
| P6(d) streaming lexer | `7d50e92e`, as designed: `Tok` with doc group and keyword spelling, `VecDeque` lookahead | the design's `TokenKind` + `Payload` split (`Token` kept) | **+1,773 M of plumbing in place of ≈0.33 G of vectors** (R2) | **design** |
| P6(e) byte classes | `d88aaa55` | — | `class_run` 100 M, the eight-at-a-time table loop the design specified | design (small) |
| P7 | built otherwise: `2b6aeb84` keeps the `Type` beside its node, made lazily by the old round trip (`parse_written_type(to_source_string())`, `phpdoc/src/doc_type.rs:57`, `:90`) and kept with the memoized docblock; `b081be75` and `1892b477` memoize name resolution by node and import-state version (sound: that function reads only namespace and imports) | `phpdoc::lower`, eager lowering, moving the other text-grammar callers, deleting the text grammar, the per-file statement `@var` memo | `to_source_string` 238.1 → 83.3 M; `parse_written_type` 118.3 M remains; `resolve_type_node` 118.3 → 94.1 M over the same 34.8 K statement tags | impl.: ≈0.2 G left. The status's "saving gone" rests on 0.18% of 3,322 samples |
| P9 | steps 2-3 (`abd326a2`, `e1e54c29`) | step 1's consumer audit not seen in any commit | with P12: `deps` 841.6 → `recorded_references` 246.7 + the parser's recording | — |
| P10 | generators (`e03e3af6`), lexer facts and `GuardArgs` gate (`c88e1b13`), autoload thread with resolver fixes (`7be53b3c`) | the fact table is a linear scan per new name (`lexer.rs:240`) | `body_contains_yield` 52 → 0; `GuardArgs` 166.6 → 10.3; `prepare` 365.0 → 44.2 on the main thread | — |
| P11 | built otherwise: `c49b68d2` memoizes the scratch `DocBlock` of each tag, keyed by `format!("{tag}\u{1}{rest}")`, and merges it; `3a98ed5e` makes the tag lexer read bytes | step 1 (`TagEffect` and `apply`, no scratch block, no merge), step 3 (`Arc` fields), pulling the tag lexer | `apply_tag_recording_errors` 518.2 → 444.8 M; the key's `format!` 54.7 M, `merge_from` 56.2 M, token clones 36.0 M | impl.: step 2 without step 1 |
| P12 | step 1 (`4c60f8a8`); step 2 (`2dc1aa9b`, with the collector walk kept as a debug oracle and as the path for a changed stub file) | step 3 (the declaration list) replaced by facts-gated walks (`2827b8ab`, `674ae836`); step 4 partly (`5fb858c9`; `InitFlow` keeps its own walk); `ImportScope` published once not built (`add_uses` still 160,194 calls) | `resolve_names` 376.3 → 0 | impl. |

### Main thread (§13.5)

| item | built (commits) | not built, or built otherwise | measured | shortfall |
|---|---|---|---|---|
| Path work on the workers | `689f318d`, `377f8266` | — | owner, absolute path, file id 164.2 M serial → 156.7 M on the workers | — |
| Package owner | built otherwise: a prefix trie over the directories (`composer/src/lib.rs:252`), not a per-directory memo | — | 62.5 → 69.1 M, now on the workers | — |
| Sorted traversal | `689f318d`: directories listed in name order | the k-way merge: `sort_paths` still sorts the analysed list when it is not in order, comparing `PathBuf`s by components | 74.2 → 66.9 M, serial | impl. |
| Size order | `689f318d` | — | 40.8 → 0.6 M | — |

### Check phase

| item | built (commits) | not built, or built otherwise | measured | shortfall |
|---|---|---|---|---|
| C1 | steps 1-4 (`9bec1258`, `f6d1286d`, `2d12024a`, `8386baff`, `b99855fd`); step 5's exit and throw tails (`bea1542c`, `57c7827c`); step 6 (`fba979fd`) | step 5's effects and yields in the result; `collect_throws` (`try_catch.rs:398`) and `expr_side_effects` (`noop.rs:51`) remain | `always_terminates` 1,553.3 → 0 | not built (tail) |
| C2 | step 1 (`7ec0eed5`) | step 2: subjects still `ConditionalSubject::Parameter(Name)`, bound by name (`types/src/conditional.rs:47`); step 3: `TemplateBindings` still `FxHashMap<String, Type>` (`checker.rs:224`) | `infer_call_return_type` 1,076.2 → 10.5 M | impl. (small now) |
| P8 | `7ec0eed5`, `5ea6b430` | — | `conditional.rs` 20.1 → 1.1, `transform.rs` 59.4 → 17.9, `traverse.rs` 117.5 → 49.8 M | — |
| C3 | steps 1-5 (`66eb5d12`, `81fb686a`, `712cd30c`, `2e5b37b6`, `91c23460`, `30e0007d`) | step 6 (`String` keys by module); step 7: the method memo is kept, keyed by `String` (`checker.rs:271`) | `name_table.rs` 344 → 145 M; lookups by `&str` 701 M of folding (R6) | impl. ≈0.6 G |
| C4 | steps 1-4 (`808f2d8d`, `3161944d`, `d721b941`); step 5 partly (`2d1e6c45`) | step 5's keys: guarded functions, constants and classes are `String` sets, guarded members `(LocalKey, String)`, container fields `String` (`checker.rs:44-60`, `:172`) where the design has `FoldedName`, `Name`, `ConstName` and `ArrayKey` | `record_branch_implied_facts` 320.7 → 57.8, `merge_scopes` 319.7 → 223.9, `imbl` self 473 → 320 M | impl. |
| C5 | step 1, labels (`b8426679`) | step 2 built otherwise: an interim `statement_method_call` record (`calls/misc_rules.rs:49-58`), which §12.6 ruled out because §12.10's observer API replaces it | formatting self 420 → 164 M; 66,295 formats per statement and 33,209 per function remain | impl. |
| C6 | its three commits (`feda35d7`, `4fb95682`, `e17921f1`) | the walker and the member-usage check move with §12.10's families, by design | `check_node_connecting_attribute` 210.4 → 1.0 M | — |
| C7 | `eb2fc46b` | — | ≈283 → ≈82 M | — |
| C8 | step 1 and part of step 2 (`bb5ade43`: string literals, objects, enum cases, class-strings) | the other constructors | `intern::insert` 605.1 M / 2.15 M → 521.2 M / 1.68 M | impl. (small) |
| §12.10 | — | steps 1-5 | re-inference ≈0.75 G, separate walks ≈0.35 G (§6) | not built |
| §12.11 | step 1 in part (`8c309067`: one normalization of all members in first-occurrence order, memoized by member list, the 59 fold sites converted) | `MemberIndex`, the normalized-operand fast path | `union.rs` 77.7 → 58.7, `reduce_array_members` 51 → 21 M | impl. |

### Finishing passes and §14

| item | built (commits) | not built, or built otherwise | measured | shortfall |
|---|---|---|---|---|
| F2 | steps 2-3 (`dd76f88b`, `f2072ccf`, fixed by `df4c1e66`); guards taken from the declarations walk, P12's list not existing | the stubs' availability (`class_exists`, `function_exists` and the rest), step 4's count of stub keys in scope | stub class entries in scope 6,320 → 6,279 | impl. |
| F1 | `6c059a46`, as designed | — | class decoding ≈436 → 138.8 M of shells, but `resolve_deferred_value_of` decodes 22,746 stub methods anyway (178.3 M), through `class_has_deferred_type` (R5) | **design**: §13.2 left "do the passes iterate every member" to F2's count, which was not built |
| F3 | steps 1-3 (`8b3e5f73`, `e7ae78c1`, `111960d7`, `d8b6a4ce`); step 4's pointer-first equality | step 4's parallel filter: `changed_since` loops serially (`registry.rs:641-676`) | `end_declaration_changes` 166.9 → `reset_scoped` 47.2; `Members::eq` 96.9 M, because purity writes into 59,637 members and copies 47,309 (R3) | **design** (pass results kept inside the shared declarations) and impl. (serial filter) |
| F4 | — | keys stay `(Option<String>, String)`; `7a7de9a0` hashes and compares them folded instead | ≈0.5 G of string keys in purity and property writes (R3) | not built |
| F5 | `f26b96b5` (side by side) | the `FileId` bitset; the frontier's class and function sets are `FxHashSet<String>` (`project.rs:1304`) | 180.1 → 114.9 M | impl. |
| §14 | part 1, steps 1-7 (`a0bc39a5`, `75cbcd95`, `ec8ea05e`, `fba979fd`, `fa7085f1`, `20361eb1`, `a94b13dd`, `1f0ea319`, `5fb858c9`); part 2 (`03583eeb`) | trees dropped in the serial insert loop (`project.rs:1065`); the design names no thread | parse job +782 M of summaries; `walk_top_level` 227.5 → 0; purity and property writes 855 → 1,031 M; +575.5 M of serial frees (R1) | impl. (where trees drop) and **design**: "speeds up every run, cold ones included" was wrong, since the passes' cost is resolution, not walks |

### Where the gap comes from

The check phase delivered ≈3.7 G, about as designed for what was built. The front end delivered ≈1.05 G of ≈3.8 G designed, the finishing passes 0.26 G, the main thread nothing. The gap divides three ways:

1. **My designs:**
   - the streaming token interface (P6(d), ≈+1.4 G against the vectors it replaced);
   - column tables for every file (P4, ≈0.2 G);
   - F1 shipped ahead of the count it was meant to wait for (178 M);
   - pass results kept in the shared declarations (F3/§14: copies and deep compares);
   - §14's claim of cold-run savings: the parse job pays +0.78 G and the passes are no cheaper.
2. **Built in part or otherwise:**
   - the tree drop placed in the serial loop (0.58 G serial);
   - C3 steps 6-7 (≈0.6 G);
   - F4 (≈0.5 G of folded string keys);
   - P7 (≈0.2 G);
   - P11 step 1 (≈0.15 G);
   - C2 steps 2-3, C4 step 5's keys, C5's interim record, F3's parallel filter, F5's strings, P12 steps 3-4, the union index, the sort.
3. **Not started:** §12.10 (≈1.0-1.3 G), and C1 step 5's effects and yields.

## 3. Parse job (9.97 G)

| part | Ir |
|---|---|
| `parse_with_ignores` (lexing, parsing, name resolution, reference and doc-site recording) | 6,854.4 M (was 7,579.9) |
| `file_declarations::collect` | 1,800.8 M |
| body summaries and init flows | 617.7 M |
| `recorded_references` | 246.7 M |
| `GlobalFacts::of` | 218.5 M |
| the read and its UTF-8 check | 45 M |
| Pest facts | 6.2 M |
| owner, absolute path and file id | 156.7 M |

### 3.1 The token interface (R2): 1.77 G of plumbing

Each of the 6.906 M tokens passes through five functions, and each moves the token by value:

| layer | self | calls | Ir per call |
|---|---|---|---|
| `Lexer::next_token` | 687.3 M | 6.906 M | 99.5 |
| `Lexer::next_raw` | 401.3 M | 6.906 M | 58 |
| `Lexer::next_tok` | 476.6 M | 6.819 M | 70 |
| `Parser::fill` | 461.7 M | 6.805 M | 68 |
| `Parser::skip` | 257.8 M | 4.52 M | 57 |
| `Parser::take_token` | 175.3 M | 2.25 M | 78 |

What each layer moves:
- `next_raw` returns `(Token, Span, Option<String>)`, 64 B.
- `next_tok` builds a `Tok { token, span, docs: Option<Box<DocGroup>>, raw_keyword: Option<Box<str>> }`, 64 B, with a `Vec` of doc comments per call.
- `Supply` is a `RefCell<enum { Lexer(Box<Lexer>), List(..) }>`, matched per token.
- The token is pushed into a `VecDeque` and popped as `Option<Tok>`, and `pop` calls `fill(LOOKAHEAD)` again.

By line, the cost is all moves and frames:
- the entries and returns of `next_raw` (`lexer.rs:573`, `:586`, `:587`: 207 M);
- the entries and returns of `next_tok` (`:592`, `:595`, `:613`, `:620`: 253 M);
- the `docs` vector (`vec/mod.rs:465`, 61 M);
- `fill`'s loop and `ptr::write` (`parser.rs:214-223`, 270 M);
- `vec_deque/mod.rs` (364 M over the run).

The plumbing alone is 257 Ir per token. The vectors it replaced cost ≈0.33 G, which is ≈48 Ir per token (25 G report §3, P6). The lexing proper is nearly unchanged: `next_token` inclusive went 2,211.6 → 2,065.3 M, 321 → 299 Ir per token. So the streaming interface cost about 1.2-1.4 G net, which is P4's whole gain and more. It is the interface the 25 G report designed (§11.5(d): `Tok` carrying its doc group and keyword spelling, a `VecDeque` of lookahead), built as written. The fault is the design's, which counted what the vectors cost but not what passing each token through the parser's lookahead would cost.

### 3.2 The rest of the front end

- **`LineIndex::new` (R4), 270.4 M, built for all 19,211 lexers** in `Lexer::build` (`lexer.rs:368`). It is made of:
  - a `memchr_iter` call per line feed (1.68 M calls, 63.5 M);
  - the non-ASCII column tables of 1,876 files through `Utf8Chunks` (83.4 M);
  - 116.3 M of self.
  
  The 25 G report's Part I had it built on first use. Its design (§11.4) instead has the lexer build it for every file and keeps it for context files, which do need line starts: duplicate-declaration messages print other files' lines (`duplicate_classes.rs:61`, `duplicate_functions.rs:78`, through `Project::line_in`), anonymous-class names hold their line, and the language server navigates into context files. What no context file needs is the column tables, which only analysed files' diagnostics and the LSP read. The per-line `memchr` call is the implementation's. The parser also reads the index, in two places:
  - `member_removal_span` (`parser.rs:370`): a fix-it span computed for every class member of every file;
  - a statement's `doc_line` (`parser/stmt.rs:31`, `:436`).
- **Stack guards in hot paths (R8).** `stack.rs` costs 293 M by source line.
  - `Expr::peeled` (`ast.rs:1412`) unwraps `(`/`@` by recursion under `stack::guard`, which reads a thread-local on every call. That is 3.08 M calls at ≈53 Ir: 74.1 M of self plus 81.7 M of thread-local reads, 160 M in total. A loop would take ≈5 Ir.
  - Every `Expr` and `Stmt` is dropped under the guard too (`ast.rs:1434-1446`): 131.7 M at `stack.rs:30`.
- **Identifier names (R12):**
  - `Lexer::name_and_bits` is 324.2 M self over 1.98 M calls, 519.4 M inclusive (the 25 G run's `Lexer::name`: 375.7 M inclusive). The per-lexer table hashes the text in a second pass after `lex_ident_span` has scanned it (174.4 M).
  - Each name new to the file runs `WatchList::bits`, a linear scan over every watched spelling with `eq_ignore_ascii_case` (`lexer.rs:240-250`, 17.9 M).
  - `Keyword::try_from_bytes` is 127.2 M over 1.74 M (73 Ir). The slot lookup is cheap, but the comparison is `eq_ignore_ascii_case` byte by byte (`num/mod.rs:863`, 34.9 M), and `lex_word` compares the spelling a second time (`lexer.rs:867`).
- **Whitespace runs:** `class_run` (`lexer.rs:1428`) tests eight bytes by eight table lookups and a fold: 100 M.
- **Declarations, 1,800.8 M.** `ClassReflection::from_def` costs 1,522.2 M for 12,546 classes:
  - docblocks 584.2 M (46,814 parses through the memo);
  - method signatures 512.4 M (60,854);
  - constant types 107.5 M (10,219 inferences);
  - `declared_type` 51.9 M.
  
  The phpdoc work across the run is still ≈0.88 G: `parse_shared` 882.7 M inclusive over 130,847 lookups, 41,176 distinct docblocks parsed at 18.5 K Ir each. P7's round trip is still there: `parse_written_type` 118.3 M over 74,325, `TypeNode::to_source_string` 83.3 M over 110,077, about 0.2 G. The sampling figure (0.18%) that retired P7 rests on about six samples.
- **§14 products, built for every body of every file (R7):**
  - `BodySummary::of`, 344.0 M over 59,926 bodies;
  - `InitFlow::of`, 218.5 M over 57,506, a second walk of each body;
  - `GlobalFacts::of`, 218.5 M.
- **Name resolution leftovers:**
  - `NameContext::add_uses` still runs 160,194 times (20.6 M of `memrchr` alone);
  - `UseMap::get` re-validates UTF-8 on 262,599 lookups (20.5 M);
  - a `RefKey::Constant` splits its text on every hash and comparison (`references.rs:110-132`: `memrchr` 12.5 M + 7.9 M).

## 4. Main thread (1,064 M serial) and the autoload thread

`Session::load` is 932.2 M of it:

| part | Ir |
|---|---|
| `insert_numbered_file` | 755.9 M |
| `collect_php_files` | 75.5 M |
| `sort_paths` | 66.9 M |
| `Once` | 10.3 M |
| `dedup` | 8.7 M |

Inside `insert_numbered_file`:

| part | Ir |
|---|---|
| **`drop_glue::<[Stmt]>` (R1)** | **575.5 M** |
| `register_references` | 92.6 M |
| `insert_built_decl` | 52.5 M (with a clone of each `BuiltDecl`) |
| `mentions_phpstan` | 6.2 M |

**R1.** The workers parse every file and return its tree. `insert_numbered_file` then drops each of the 11,459 context trees inside the serial insert loop, through `kept_ast: ast.filter(|_| analyzed || !self.built_once)` (`project.rs:1065`). The 387.3 M of that is `Arc<Vec<Stmt>>` function bodies.

The frees are cross-thread, since the blocks were allocated by workers:
- `mi_free` costs 343.4 M on the main thread, more than half of the run's 648.0 M;
- per call, `mi_free` costs 42.5 Ir against 23 in the 25 G run, whose trees lived until exit;
- `mi_free_try_collect_mt` and `mi_abandoned_page_try_reclaim` appear at all only because of this.

R1 applies to every run without the cache: the first run after a build, CI, and any run where files changed.

Thread 10 (321.4 M) parses the Composer autoload files with the full parser, whose recorded references and doc sites `parse_generated_file` drops (it keeps `stmts`, `composer/src/vendor.rs:159`), plus path work: `Components::next` 34.6 M, `sort_paths` 8.4 M, `starts_with`. It runs while the file lists are made, off the critical path unless the listing finishes first.

## 5. Finishing passes (2.48 G, and the wall-time bottleneck)

On their thread (2,003.5 M):

| pass | this run | 25 G run |
|---|---|---|
| `resolve_transitive_purity`, with the property writes inside it | 879.3 M | 316.4 M plus 440.4 M of property writes, stolen |
| `warm`'s decoding loop | 150.5 M (class shells 138.8 M, 6,279 decodes) | |
| `flatten_phpdoc_inheritance` | 116.0 M + 30.7 M stolen | |
| `precompute_ancestor_names` | 115.9 M | |
| `Members<FunctionReflection>::eq` | **96.9 M over 885 calls** | |
| `HashMap<DeclKey, PropertyWrites>::get` | 67.8 M | |
| `closure_scope` | 62.7 M + 52.2 M stolen | |
| `resolve_deferred_value_of` | 55.1 M + **175.3 M stolen** | 24.4 M |
| `precompute_member_owners` | 53.7 M + 74.0 M stolen | |
| `flatten_traits` | 43.5 M | |
| `reset_scoped` | 47.2 M | |
| `infer_global_variables_of` | 30.3 M | 141.6 M + 117.8 M |
| `RefKey::class_text` | 28.4 M | |
| `walk_affected` | 21.9 M | |
| `RefKey` and `PathBuf` set inserts | 32.0 M | |
| the rest | under 30 M each | |

- **R3. Pass results written into shared declarations.**
  - `classify_purity` writes `purity_dependencies` into each method through `classes.get_mut(..).methods.get_mut(..)` (`purity.rs:132`, `:160-167`). That `make_mut`s 59,637 members and clones 47,309 `FunctionReflection`s (24.4 M), which undoes F3's sharing.
  - `changed_since` then finds those members' pointers different and compares them field by field: `Members::eq` costs 109 K Ir per class map.
  - The property-write tables are keyed by `DeclKey`, an `(Option<String>, String)` hashed and compared case-insensitively on every operation (`property_writes.rs:62-160`). `resolve_transitive_purity`'s own self cost is 258.8 M, mostly `to_ascii_lowercase` per byte (`num/mod.rs:863`, 91.1 M) and FxHash over the folded bytes. Add the `DeclKey` map inserts (67.3 M), `is_folded_eq` (57.6 M), `fold_hash` (38.6 M), `DeclQuery::eq` (34.4 M) and `eq_ignore_ascii_case_chunks` (30.6 M). That is ≈0.5 G spent on string keys in a pass whose answers are about a few thousand functions.
  - Most of it is serial: the maps are built and read on one thread.
- **R5. Stub members decoded anyway, and the stub scope unchanged.**
  - `resolved_deferred_class` asks `class_has_deferred_type(info)` of every scoped class (`resolve.rs:587-592`). That iterates the class's methods, which decodes each lazily stored stub method (22,746 decodes, 178.3 M), to learn what the code's own comment says: "the overwhelming majority, stubs included, have none".
  - `warm` initialises 6,279 stub class entries, where the 25 G run initialised 6,320. F2's version guards shrank nothing here.
  - The half of F2 left unbuilt (guards on `function_exists`/`class_exists` evaluated against the stubs' availability) and its step 4 (a count of the stub keys in scope, by the name that pulled each in) are what would show and remove the rest. Without that count, this profile cannot say which names pull the 6.3 K classes in.
- `RefKey::class_text` 82,977 calls (28.4 M) and the `RefKey` and `PathBuf` sets: the affected-file walk turns keys back into text and paths to look them up.

## 6. Check phase (6.72 G, was 10.45 G)

| part | this run | 25 G run |
|---|---|---|
| `check_function_impl` | 3,975.9 M (10,506 calls) | 6,309.9 M |
| top-level statements | 2,114.2 M | 3,402.5 M |
| per-file pre-walks | ≈250 M: `Promotions` 86.3, `Members` 76.0, `global_constants` 56.8, `GuardArgs` 10.3, `Declares` 8.0, `Assigned` 4.3, `Initializers` 3.2, `Assertions` 3.1 | ≈610 M |
| PHPStan-API walker | 103.9 M | |
| declaration checks | class generic bounds 61.9, overrides 31.1, unknown PHPStan tags 24.7, property declarations 19.2 | |

Subsystems, inclusive and overlapping:

| subsystem | this run | 25 G run |
|---|---|---|
| `infer_expr` | 1,118.1 M over 822,504 calls; 534,412 computed, 978.2 M | 1.13 M computed |
| `method_call_return_type` | 454.1 M over 75,259 | 1,382.9 M over 153,953 |
| `check_args` | 540.4 M | 647.2 M |
| `apply_narrowing_unguarded` | 482.8 M | 693.5 M |
| `merge_scopes` | 223.9 M | 319.7 M |

What is left, by kind:
- **Re-inference** (§12.10's subject), about 0.75 G:
  - `check_args` infers 178,218 arguments again after the expression check (263.0 M);
  - `check_expr_impl` re-infers 96,939 sub-expressions (330.4 M);
  - `call_bindings` 55,927 (71.2 M);
  - `infer_binary` 80,799 (94.4 M).
- **Lookups by `&str` (R6).** The C3 tail is not built: about 490 M of the 701 M of `fold_hash`/`is_folded_eq` falls in this phase. The callers hold text where a `Name` exists:

  | caller | lookups |
  |---|---|
  | `NameTable<ClassOwners>::get::<str>` | 782 K |
  | `lookup_class` | 413 K |
  | `lookup_method_with_owner` | 288 K |
  | `resolve_function` | 273 K |
  | `contains_key::<str>` | 230 K |
  | `substitute_class_templates` | 168 K |
  | `declared_method_name` | 167 K |
  | `record_member_usage_token` | 121 K |
  
  `fold_hash` runs at 59 Ir and `is_folded_eq` at 95 Ir per call. With `Name` keys, both become a load and a pointer comparison: `get::<Name>` costs 35.5 M over the whole phase.
- **Locals.**
  - `KeyTable::find_var`: 79.1 M, a variable name to its key, on every access;
  - `OrdMap::get`: 72.9 M;
  - `Locals::clone`: 33.5 M over 148 K.
- **Interning:**
  - `intern::insert` 521.2 M over 1.68 M;
  - `string_literal_ref` 86.0 M over 292,885 literal types, which §12.10's one inference per node halves;
  - `exact_hash` 62.0 M;
  - `remembered` 55.8 M.
- **Short names by splitting text:** `check_expr_impl` splits names on `\` from the right 335,699 times (`str/iter.rs:551`, 50.9 M).
- **Formatting left by C5:** `check_stmt_impl` 66,295 formats (23.9 M), `check_function_impl` 33,209 (20.9 M).

## 7. Cross-cutting

| | 25.14 G run | this run |
|---|---|---|
| `mi_malloc` calls | 21.6 M | 16.6 M |
| `mi_free` calls | 20.4 M | 15.2 M, at a higher cost per call (R1) |
| mimalloc self | 1,708 M | 1,578 M |
| `finish_grow` | 1,064 M | 737 M |
| `memcpy` | 27.1 M calls, 784 M | 15.6 M calls, 514 M |
| `memcmp` | 23.8 M calls, 447 M | 10.9 M calls, 207 M |
| formatting self | 420 M | 164 M |
| `&str` folding (new) | — | 701 M |
| AST drop glue self | 34 M | 252 M (R1, R8) |

## 8. Memory: the dhat heap profile

**The profile.**
- `target/dhat-heap.json`, made by the other session with `dhat-rs` 0.3.3 as the global allocator (the `dhat-heap` feature, `main.rs:19-21`) in a `dhat` build of `5fb858c9`'s code (`target/dhat/phpstan-rs`, built after the last code commit).
- The command was the same as callgrind's, in `/home/torres/piwigo17-frozen`. No per-file cache frame appears anywhere, so the cache was not in play.
- dhat records requested bytes over the system allocator, not mimalloc's resident size. It takes every allocation under one lock, so the run took 518 s and its timeline means nothing.
- Each stack keeps about ten physical frames. Allocations are therefore attributed by the innermost frames that name a holder (a memo, a pass, the parser), and the categories below are approximate at their edges.
- `dhat-rs` counts a `realloc` as a block of its new size, charged to the site that first allocated.
- The analysis scripts are in this session's scratchpad (`mem/lib.py`, `mem/cats.pkl`).

**Totals.**

| | bytes | blocks |
|---|---|---|
| allocated over the run | 2,668.6 MB | 19.44 M |
| live at the peak (t = 425 s of 518 s) | **866.1 MB** | 4.57 M |
| live at exit | **657.1 MB** | 4.11 M |

`check-project` leaks the project on purpose at exit (`check_project.rs:159-160`). So "at exit" is what the project holds after checking, which is close to what a language server holds for this workspace (but see M3).

**By holder.**

| holder | at the peak | at exit | allocated | blocks allocated |
|---|---|---|---|---|
| trees and their payloads | **595.8 MB** | **173.1 MB** | 934.5 MB | 5.24 M |
| declarations | 74.9 | 76.6 | 263.6 | 1.32 M |
| docblock memos | 66.1 | 72.5 | 186.4 | 1.58 M |
| file states and indexes | 40.4 | 47.3 | 99.2 | 0.55 M |
| body summaries | 35.8 | 35.8 | 104.7 | 0.80 M |
| stub and base records | 23.6 | 67.6 | 86.1 | 0.43 M |
| type interner and its memos | 11.3 | 34.5 | 59.4 | 0.54 M |
| purity and property-write tables | 0 | 64.1 | 112.5 | 0.85 M |
| copies the finishing passes make | 0 | 43.8 | 104.9 | 0.59 M |
| checker | 0 | 25.7 | 524.9 | **6.45 M** |
| relation memo | 2.0 | 9.7 | 19.3 | — |
| the rest (load, names outside trees, phpdoc) | 16.2 | 6.4 | 172.9 | 1.08 M |

**M1. The peak is every file's tree at once.**
- At the peak, nothing the checker or the finishing passes allocate is live: the peak falls at the end of the parse.
- `Session::load` parses every file on the pool into one `Vec<Read>` (`session.rs:748-777`, `.collect()`), then inserts them in turn (`:817`, `:845`). A context file's tree is dropped only in that loop (R1).
- So at the peak, all 14,251 trees are alive: 596 MB of the 866.
- The analysed files' trees are what remains at exit, 173 MB, so the context files' trees are ≈420 MB of the peak.
- The sources are not part of it: each is dropped after its parse unless kept (`session.rs:684`). Their reads allocated 57.4 MB in all.

**M2. A tree is 9 times its source.** The 2,793 analysed files hold 19.07 MB of source and 173 MB of tree at exit, in 1.10 M allocations.
- **Every node has its type's full size,** whatever its variant. The block sizes give:

  | type | size |
  |---|---|
  | `Expr` | 80 B |
  | `Stmt` | 160 B |
  | `Arg` | 104 B |
  | `ArrayItem` | 168 B |
  | `Param` | 200 B |
  
  `Stmt` inlines an `IfStmt`'s condition. Every statement carries `doc_comment` and `doc_line` (24 B), though few have a comment (`ast.rs:47-67`).
- **Lists grow by `push` from empty and are never shrunk** (`postfix.rs:261`, `primary.rs:11`, `control_flow.rs:323`), so a list of one element holds four.
- **A count with nikic/php-parser 5.8 over the same files, sized by `Vec`'s growth rule:**

  | lists | count | average length | allocated | of which used |
  |---|---|---|---|---|
  | arguments | 107,569 | 1.56 | 46.2 MB | 17.5 MB |
  | array items | 11,855 | 2.49 | 9.9 MB | 5.0 MB |
  | statements | 38,688 | 3.87 | 40.5 MB | 24.0 MB |
  | parameters | 6,463 | 2.38 | 6.4 MB | 3.1 MB |
  | **total** | | | **102.9 MB** | **49.5 MB** |
  
  The allocated column agrees with dhat's retained bytes for the same sites:
  - arguments 46.7 MB;
  - array items 9.8 MB;
  - parameters 6.4 MB;
  - statements 36.9 MB, where php-parser also counts an unbraced body as a list.
  
  So 53 MB of the 173 MB, 31%, is capacity no element uses.
- **The context files' large array literals** allocated 143 MB of item lists over the run (100.8 MB live at the peak, 9.8 MB at exit).

**M3. A tree outlives its last reader, and the language server keeps more.**
- In a batch run, the analysed trees are kept to the end because project-wide passes walk them after every file is checked:
  - architecture (`architecture.rs:245-247`);
  - banned constructs (`banned.rs:477`);
  - class shapes (`class_shape.rs:103`);
  - Composer hygiene (`composer_hygiene.rs:287`, `:304`, `:519`);
  - mock hygiene (`mock_hygiene.rs:376`);
  - use sites (`project.rs:1681`, `:1718`);
  - the unmatched-ignore pass's trait lines, in `finish_diagnostics` (`project.rs:813`);
  - the PHPStan-API walker (`project.rs:2376`).
- The language server (`built_once: false`, `retain_source: true`, `workspace_config.rs:200-201`) also keeps every context file's tree (`project.rs:1065`) and every file's source (`session.rs:684`).
- For this workspace, that is ≈420 MB of trees and the sources on top of the 657 MB. The batch run already drops the context trees and parses one again on demand (`ast_of`, `AstCache`).

**M4. The finishing passes copy declarations and key their answers by strings** (R3), and both stay to the end.
- **Copies, 43.8 MB, kept beside the as-built snapshot:**
  - `Arc::make_mut` copies of `ClassReflection` (8.4 + 5.5 MB);
  - rebuilt `Members` (12.1 + 3.2 + 2.9 + 1.3 MB);
  - cloned parameter lists (5.0 + 2.9 MB).
- **The purity and property-write tables, 64.1 MB:**
  - the `DeclKey` maps (10.6 + 7.5 MB);
  - `Resolve::call`'s vectors (7.6 MB);
  - 221,618 lowercased method-key `String`s (`method_key`, 5.5 MB);
  - 136,679 `DeclKey` clones (3.6 MB);
  - `own_owner_names`' maps by `Name` (6.7 + 2.5 MB).
- **For scale, the declarations themselves:**
  - a `ClassReflection` is 1,322 B (16.4 MB for 12,407);
  - each method is its own `Arc<Lazy<FunctionReflection>>`, 352 B (21.4 MB for 60,854).

**M5. Stub records are copied out of a file read whole.**
- `load_container` reads the whole cache file into a `Vec` (`stub_cache.rs:53-54`): 20.6 MB, plus 2.0 MB for the stub index's.
- A class decoded from it copies each method's encoded bytes into an `Arc<Vec<u8>>` (`name_table.rs:1208-1213`; 35,857 copies, 7.2 + 1.4 MB). The copy stays after the method is decoded.
- `Lazy<V>` keeps room for the decoded value inline (`name_table.rs:493-497`), so a method costs 352 B whether or not it is ever decoded (12.6 MB for 35,782).
- 22,746 of the methods decoded here are decoded only by `class_has_deferred_type` (R5).

**M6. Eight process-wide text memos, each keyed by its own copy of the text** (`memo.rs`). They are at `deps.rs:145`, `phpdoc_unresolvable.rs:73`, `docblock.rs:295` and `:495`, `grammar.rs:29`, `:58` and `:73`, and `types/src/parse.rs:29`. Together they hold 72.5 MB at exit.
- **The keys are copies:** whole doc comments for `DocNames` (8.9 MB, 36,619 keys), and docblocks, tag lines and tag values for the others (3.3 + 1.5 + 1.3 MB).
- **The tag memo stores a whole `DocBlock` for each tag** (`docblock.rs:495`): 26,393 × 840 B = 22.2 MB.
- **`DocNames` is still there.** It keeps each doc's class names as `String`s (`deps.rs:137-155`), though the 25 G report's P9 step 3 put them on the block and deleted it.
- **The bound is crude.** A shard that fills is emptied whole, its hot entries with the cold (`memo.rs:88`). The tag memo's bound is 64 shards of 2,048 entries and 1 MB of keys: 131 K entries.

**M7. Interned values cost more than one allocation each.**
- **Types.** 119,757 nodes take 13.4 MB (112 B each). Their index, `FxHashMap<u64, Vec<Weak<Node>>>` (`intern.rs:21-25`), adds a `Vec` for every distinct hash (120,644 allocations, 3.9 MB) and 8.7 MB of table: ≈105 B of index per node. The type memos keep 5.4 MB (`remember`) and 1.5 MB (shapes), and the structural relation memo keeps 9.4 MB in eight thread-local tables, each emptied whole when it reaches 2^17 entries.
- **Names.** A `Name` is `Arc<NameData>`, and `NameData` holds its text and its lowercase form as two more boxes (`name.rs:22-49`). That is up to three allocations per name: at exit, 135,777 `NameData`, 118,237 texts and 79,600 lowercase copies. `Interner::str` keeps 36,952 more `Arc<str>` (9.1 MB).

**M8. Per-file tables, 47.3 MB at exit.**
- `FileState` slots, inline: 547 B per file (7.8 MB).
- `LineIndex`es: 6.6 MB, plus 1.0 MB of `Arc`s.
- Each file's references as a `HashSet` (6.4 MB).
- The reverse index: an outer map of 4.8 MB, plus one `HashSet<FileId>` for each of 45,696 keys (3.0 MB). Most keys are referenced by one file.
- Each path stored twice (28,502 buffers, 2.5 MB).
- Duplicate sites (3.1 MB).
- Body summaries add 35.8 MB, two `Arc`s per body (`BodySummary`, `InitFlow`) for 57,506 bodies.

**M9. Churn: 19.4 M allocations, a third of them the checker's**, each living milliseconds. The largest sites (M10 maps them all):

Counted by M10's rule: a row naming a function counts what M10 charges to that function. A row naming a utility (`Name::to_string`, `imbl`'s leaf copies) counts the allocations made inside it, whoever calls it, which M10 charges to the callers.

| sites | allocations | item |
|---|---|---|
| `Name::to_string` | 507,745 | 15 |
| `bind_arguments`, `check_parameters`, `resolve_call_args` (39 MB: each rule binds the same call's arguments again) | 738,109 | 18 |
| `class_template_bindings_rec`, its `HashMap<String, Type>`s (24 MB) | 328,384 | 15 |
| `check_expr_impl`, `forwards_static_binding`, `type_check_call_outcome`, `identifier_unresolvable`, mostly lowercase copies | 593,987 | 15 |
| `ShapeKey::from_bytes` | 182,591 | 16 |
| `imbl` leaf copies (31 MB) | 114,979 | 18 (radix arrays) |
| `statement_assigned_variable_names` | 214,698 | 15 |
| `TypeNode::to_source_string` | 244,893 | 14 |
| `Expr` clones (query-builder chains, `check_if`'s disjunctions, scope aliases) | 52,349 | 9 |
| the lexer's interpolation scans and the parser's interpolated parts (84 MB) | 609,127 | 9, 10 |
| `parse_type_hint`: 468,730 temporaries and 11,843 nullable boxes | 480,573 | 9 |
| `parse_qualified_name`, `qualify`, `NativeType::from_name`, mostly lowercase and joined copies | 604,042 | 12, 15 |

**M10. Every allocation site, by the item that removes it.**
- **Method.** Each allocation was charged to its allocating function: the innermost project frame that is not a container, a conversion or a utility (`to_string`, `to_ascii_lowercase`, `clone`, map inserts). There are 1,591 such functions.
- **All 1,591 were read at their allocating lines** (the two most frequent of each) and given to the item whose design removes them:
  - the 1,007 that a first matching by function placed, which a reading then corrected: `sqlparser`'s parser, the DQL parser and `clap` had been placed in items 9 and 10;
  - the 584 of the tail (M11).
  
  Where the reading found a mechanism no design covered, the design was added to its item.

| item | allocations | share |
|---|---|---|
| 9, trees in typed vectors | 3.92 M | 20.2% |
| 18, the check phase | 3.58 M | 18.4% |
| 15, names as names | 2.76 M | 14.2% |
| 14, PHPDoc | 2.66 M | 13.7% |
| 4, the registry by ids | 2.02 M | 10.4% |
| 13, a file's products | 1.31 M | 6.7% |
| 16, interned values | 0.74 M | 3.8% |
| 2, the walk and path data | 0.56 M | 2.9% |
| 12, lexer details | 0.49 M | 2.5% |
| 8, declarations | 0.48 M | 2.5% |
| 6, stub records | 0.45 M | 2.3% |
| 10, flat token arrays | 0.23 M | 1.2% |
| 5, merge and indexes | 0.11 M | 0.6% |
| 3, context files read into a worker's buffer | 0.06 M | 0.3% |
| 11, line starts and ignore marks | 0.07 M | 0.3% |
| unattributable: stacks that end inside the allocator | 0.01 M | 0.1% |
| made once per process: argument and configuration parsing, the pool | 0.001 M | 0.0% |

The rows sum to the run's 19,440,892. An item removes most of its row, not all of it: a kept tree still allocates its few vectors, interning still allocates each new type, and the results are kept. The final heap profile (§11) counts what remains.

**M11. The tail, site by site.** The 584 functions outside M10's first pass hold 1.02 M allocations (236 MB). Each was read at its allocating lines (Appendix A lists every one), and they fall into thirteen mechanisms:

| mechanism | functions | allocations | the fix | item |
|---|---|---|---|---|
| F1. A message, subject or identifier is formatted for every element checked, not only when a diagnostic is reported (`check_property_phpdoc_types`, `phpdoc_type_compat.rs:377`; `check_property_declarations`, which also turns each native hint into a `String` to test for `callable`, `property_declarations.rs:133`, `:142`; identifiers built by `format!`, `probe_checks.rs:159`) | 37 | 101,164 | Subjects become values that display themselves, formatted by the report. Identifiers are `&'static str`: they are a closed set. | 18 |
| F2. A constant type is built on every call (`union_of(vec![Type::int(), Type::float()])`, `strict_rules.rs:230`, `operators.rs:57`, `:672`, `:685`; `vec![ty, Type::null()]` for a nullable) | 41 | 33,938 | Named constant types are built once, as `types` already builds its `Constant` nodes (`intern.rs:43`). A two-member union or intersection takes its two operands, with no `Vec`. | 18 |
| F3. An expression is cloned into an `Rc` (the Doctrine and DBAL query-builder chains, `query_builder.rs:161-163`, `sql_reflection/query_builder.rs:336-338`; scope aliases, `narrowing.rs:1313`, `:1399`) | 8 | 58,287 | an `ExprId` into the file's tree | 9 |
| F4. Third-party parsers: `sqlparser` (86.6 K, its tokenizer building a `String` per word and char run) and `regex` (7.7 K, a `Captures` per match) | 112 | 94,265 | A query's text is parsed once per distinct text in the run. Matches are read through one reused `Captures`. | 18 |
| F5. Text where a `Name` exists (`check_internal_instantiation`, `instanceof_outcome`, `static_member_of_class`, the current class as a `String` in `Promotions` and `check_program`, `"Doctrine\ORM\Query".to_string()`, the DQL tokenizer's `String` tokens) | 38 | 102,938 | Item 15's rule; fixed names are interned once. | 15 |
| F6a. A rule collects in order to iterate (`check_type_version_support`'s one-element `Vec`, `type_declarations.rs:182`; `offset_read_type`, `member_resolution.rs:298`; `instance_of_any`, `operators.rs:706`). It rebuilds a constant list per call (`declaration_level_rules`, `internal_rules.rs:56`; `default_include_dirs`, `includes.rs:380`), or keeps a `seen` vector per call (`check_array_literal_duplicate_keys`, `array_ops.rs:70`; `check_function_like_declaration`). | 36 | 143,007 | Borrow (`slice::from_ref`) or iterate. A constant list is a `static` slice, and a per-file one is made once per file. A per-call vector lives in the checker's scratch. | 18 |
| F6b. A registry query walks the hierarchy with a stack and a `seen` set per call (`find_final_ancestor_method`, `hierarchy.rs:917`; `required_abstract_methods`, `:1026`; `has_concrete_implementation`, `:1070`; `class_is_immutable`, `:309`; `has_unresolvable_ancestor`, `:864`). `visible_class_consts` clones every visible constant into a map per query (`resolve.rs:1517`, 4.35 MB). The PHPDoc-inheritance backfill builds per-class vectors of parent maps (`resolve.rs:1147`, 88,501). Attributes are collected per query (`reflection_attribute.rs:63`). | 47 | 149,360 | Item 4's ancestor and owner tables, iterated; levels over ids; slices. | 4 |
| F6c. Throw, exit and loop states: every throw point clones the whole scope (`record_throw_state`, `expr.rs:348`: 3,920 clones, 16.1 MB); `check_try` collects scopes (`try_catch.rs:99`, `:124`, 17.2 MB); every loop pushes break and continue vectors (`control_flow.rs:140-141`) | 23 | 37,839 | A scope copy is a pointer copy with item 18's persistent scopes. The per-construct vectors are scratch stacks. | 18 |
| F6d. Narrowing inserts and `Rc<Vec<..>>` implied facts (`narrowing.rs:569`, `control_flow.rs:276`, `:314`, `:336`) | 26 | 29,406 | The radix arrays (item 18, step 3) | 18 |
| F6e. The vectors of separate walks (`bind_targets`, `strict_rules.rs:951`, 20,655; the keyword-scope and `declare` visitors; `section_ends`; the member and variable usage walkers) | 27 | 40,084 | They become families of the one traversal (item 18, step 1) and use its stacks. | 18 |
| F6f. Analyses that are a function of a text, redone at every call: regex groups (`regex_groups.rs`: the pattern as a `Vec<char>` at `:505`, a `String` per literal char at `:936`), `preg` shapes, DQL, SQL query-builder rendering (`query_builder.rs:192-205`), format strings (`sprintf_pieces`, `builtin_calls.rs:3244-3250`; `printf_placeholders`) | 61 | 88,382 | Computed once per distinct text, scanning bytes and keeping slices | 18 |
| F7. The front end and what is kept: sources (59,799: item 3's buffer); one `Arc` per built declaration (28,516: kept); `Checker::new` allocating a default `Arc<LineIndex>` and a key table per file, which are then replaced (`checker.rs:1155`, `:1175`, with the path copies of `set_current_file` and `checker_for`); `if_chain`'s `vec!` per `if` (`declarations.rs:172`, 15,149); heredoc stripping's two vectors (`strings.rs:785`, `:788`); ignore comments and marks as `String`s; `strip_digit_separators`' `replace` (`lexer.rs:1022`); reports | 124 | 125,981 | Items 3, 9, 11, 12 and 13. One checker per worker, reset per file (item 18). The reports and the declarations stay. | several |
| Unattributable: stacks that end inside the allocator | 4 | 10,987 | — | — |

# Part II. The plan

Twenty items, numbered in the order they are built. Each states what is there now, the design, what the final validation compares for it, and what the profiles say it saves, in instructions and in memory. "Exact" means the output cannot change; "semantic" means it can, and §11 lists where. Finding codes (R1-R8, R12 for instructions, M1-M9 for memory) and section numbers refer to Part I.

## 9. How the plan is built and checked

- **One validation, at the end (§11).** Items are committed one by one, with `cargo fmt`, `cargo check` and clippy. Apart from item 0's runs (the starting state, the oracle run, the baseline), nothing else runs until the last item lands.
- **Oracles are dumps.** Item 0 takes dumps of everything the items change, from the starting commit, in a form that does not depend on how the code represents it. §11 takes the same dumps from the last commit and compares. So no item keeps an old implementation alive to compare against, and each replaces what it replaces outright.
- **Measuring.**
  - `cl-20g` is the callgrind baseline: the `dhat` profile with frame pointers, as `CLAUDE.md` prescribes, with `PHPSTAN_RS_NO_FILE_CACHE=1`. The final callgrind run uses the same profile and setting.
  - Wall time is read from a sampling profile's serial windows (§1, §5), not from callgrind's per-thread totals: valgrind runs one thread at a time, so those show nothing about parallelism.
  - Memory is read from the dhat heap profile of §8, by holder, at the peak and at exit. The final run uses the same build (the `dhat` profile with the `dhat-heap` feature) and the same command. The server's memory is measured on the in-process session of item 0.

## 10. Items

### 0. Baseline dumps; the debug oracles retired

**Now.**
- **No dump can compare across the items.**
  - `xtask front-end-dump` (O1) depends only on `syntax` (`xtask/Cargo.toml:8-13`) and parses with `syntax::parse` (`front_end.rs:44`), with no watch list and no name cache. The project parses with `parse_watching` and then rewrites the tree: stub attributes, anonymous-class names (`project.rs:2810-2815`, `:2897-2905`).
  - It reads only `.php` files (`front_end.rs:24`) and stores `DefaultHasher` digests of `Debug` text (`:55-59`). So a difference shows no text, and the shape it hashes is what items 9-13 change.
  - Nothing dumps the registry or a type in full: `Display` drops `mixed`'s explicit flag and a template's bound, variance, scope and default (`display.rs:57`, `:118`).
  - Nothing dumps an expression's type, or a diagnostic's end position and fix (`JsonMessage`, `output.rs:81-89`).
- **The edit path builds each per-file product its own way.** `update_file` (`project.rs:1817-1882`) uses:
  - `parse_with_ignores`, with no name cache;
  - `declared_names`, `declared_constants`, and an ungated `build_declarations`;
  - `anonymous_class_names`;
  - `duplicate_class_sites.index(.., false)`;
  - `FileBodies::of`, `GlobalFacts::of`, `PestFacts::of` and `declared_classes`.
  
  The CLI never takes this path, and no dump does.
- **The other session migrated its items by keeping each old implementation alive** under `cfg(debug_assertions)`, comparing on every debug run:
  - purity's tree walk: the comparison (`purity.rs:356-363`), `classify_body` (`:416-436`) and `Disqualifier` (`:452-574`);
  - the property-write scan (`property_writes.rs:426-433`, `:619-620`);
  - the assigned-properties builder (`summaries.rs:115-121`, `:188-201`);
  - the uninitialized-property check's body path (`uninitialized.rs:133-141`);
  - the function bodies `FileBodies` stores for them (`bodies.rs:110-111`, `project.rs:216-221`);
  - the gated body walk against the ungated one (`bodies.rs:75-81`);
  - the reference collector's comparison (`project.rs:2841-2862`), with `deps::declaration_referenced_names` (`deps.rs:105`) and `symmetric_difference` (`project.rs:2865`), which only it uses;
  - the declared classes against their own walk (`project.rs:3029-3036`);
  - the declaration rebuild checks (`project.rs:1248-1262`, `:1615-1636`), with `Keep` and the `keep` parameters of `build_declarations_where` and `built_declaration` (`index.rs:661-687`), which only the second uses;
  - the gated walk's declaration count (`file_declarations.rs:122-131`);
  - and, as tests, `project/tests/fused_declarations.rs` (the five walks `parse_file` replaced) and `the_one_walk_for_both_sets_is_the_two_walks` (`deps/tests/dependency_collection.rs:228`).
  
  So `cargo test` runs two implementations and keeps every body's statements.
- **These oracles have run over the fixtures only, and a debug run as things stand would skip many of them.**
  - The per-file cache's key does not include the build profile (`build.rs:41-55`), so a debug build reads entries a release build wrote, and those carry no statements (`bodies.rs:163-176`).
  - One oracle sits behind an opt-in rule (`uninitialized.rs:43`).
  - Two sit on paths the CLI never takes (`project.rs:1248`, `:1615`).
- **The starting state was validated only in parts:** tests, fixtures and the scoped O2. `WORK_STACK.md` lists six behaviour changes "to read in the O2 diff" of the other configurations:
  - Pratt rejections;
  - `__halt_compiler`;
  - kind-exact keys;
  - hyphenated words;
  - declarations ruled out by version;
  - the server's UTF-16 columns.

**Design.**
1. **The starting state is read first.** At HEAD, the vendor-tree, all-on and unscoped reports are compared with those of the last validated state (`1892b477`), and each difference is read, the six listed among them. Anything wrong is fixed in its own commit before anything else.
2. **The debug oracles run once, over everything, then go.**
   - **The build:** an `oracle` profile, `gate` with `debug-assertions` and `overflow-checks`, run with `PHPSTAN_RS_NO_FILE_CACHE=1`.
   - **The runs:**
     - the gate's scoped, all-on and unscoped configurations of Piwigo;
     - each vendor project with `--no-composer` and every rule set on;
     - the edit replay of step 3 over each vendor project.
   - **Coverage.** A temporary counter, never committed, shows that every oracle site ran at least once. A failure is fixed in its own commit.
   - **Then the oracle code listed above is deleted:**
     - `FileBodies` stops storing statements;
     - `BodySource::body` and the debug-only re-exports (`registry.rs:73`, `project.rs:21`) go;
     - so do the test oracles whose second implementation exists only for them.
   - **What release code still uses stays** until the item that replaces it: the edit path's walks (item 1) and `deps::referenced_names` for the stub build (item 13).
   - Assertions about one implementation stay, such as the file cache's round trip (`file_cache.rs:228-236`).
3. **Dumps whose text does not depend on representation.**
   - **One canonical type printer** in `types` covers all of `Repr` and depends on no interning, as PHPStan's `VerbosityLevel::cache()` does for its cache keys (`VerbosityLevel.php:94-102`). It prints:
     - `mixed`'s explicit flag and its subtraction;
     - a template's name, scope, bound, variance and default;
     - benevolent unions, call-site variance and accessories;
     - literal bytes and float bits;
     - shape optionality and sealedness;
     - conditional subjects.
   - **Printers cannot lose fields unseen.** Every printer destructures without `..` and matches without a `_` arm, so a field an item drops cannot vanish from the dump. An item's change to a printer only renames paths, and its commit says so. No print shows addresses, node ids, interner indices or hash order.
   - **O1, the front end, per file.** It is a view of `phpstan-rs` (`--dump-front-end`), made through the project's own `parse_file`:
     - for each role (analysed, context, stub), with the load's name cache;
     - through the stub build, for the embedded stubs.
     
     Each file's dump holds:
     - tokens and comments;
     - the tree after the stub-attribute pass and anonymous-class naming;
     - the facts with every watch bit;
     - ignore marks, warnings, the parse error and goto labels;
     - at every token boundary, the line, the UTF-8 and UTF-16 columns, and the `offset_in` round trip;
     - doc sites, and the references sorted;
     - each built declaration in order, with its guard and full reflection;
     - duplicate sites, declared classes, Pest facts, anonymous-class names and use sites;
     - each body's summary and `InitFlow`, and the global facts.
     
     Its file set includes every extension PHP is run from (`.inc`, `.module`, `.install`, `.theme`, `.engine`, `.profile`, `.php.inc`) and any file that starts with `<?php` or `<?=`. Each file's text is stored compressed, with an index of digests.
   - **`registry-dump`**, taken after the finishing passes and before checking. It only reads: lazy records are decoded, but deferred values are printed as stored. It sorts by kind, folded name and load order, and prints:
     - every project declaration, and every record of both stub bases, at the configured PHP range and at its two ends;
     - for each, its reflection, its guard and the guard's value, its purity dependencies and verdict, its property-write classification and verdict, its ancestors and owners, and the ancestry fact;
     - both scopes, the global constants and the global-variable facts;
     - both reverse indexes, the use-site index and the duplicate-site tables;
     - the file table: path, role, load order, absolute path, Composer owner and `mentions_phpstan`.
   - **The checker's records:**
     - every PHPDoc resolution, keyed by file, docblock span, tag ordinal, host class and function;
     - every expression's inferred and native type in the final pass, keyed by file, span and host class (`--dump-expr-types`, beside today's `--dump-types`).
   - **O2: the gate's full artifact set** (`cargo xtask corpus --full`: scoped, all-on, unscoped, levels 0-10, the type dumps, the baselines and the emission-order check).
     - It adds a view of every field of every diagnostic: the end position, the UTF-16 columns and the whole fix.
     - It runs over the Piwigo configurations, each vendor project with every rule set, and the fixtures.
     - It runs with the stub cache cold and warm, and the per-file cache off, written and read. Every state must give the same output.
     - It is compared in output order with `xtask compare`.
   - **The server:** an in-process session over Piwigo and two vendor projects. It records:
     - the loaded files with their roles, and `registry-dump`;
     - diagnostics with UTF-16 ranges, and code actions;
     - definition, hover and references over a seeded sample, context files included.
   - **Edit replay**, for Piwigo and each vendor project, as PHPStan's result-cache end-to-end tests do (`e2e-tests.yml:38-46`):
     - a fixed script runs over a seeded sample of files: save one unchanged, delete a method, rename a class, change a parameter type, add a file declaring a class that is named but declared nowhere, delete a file;
     - each step goes through `update_file` or `remove_file`, and the project dump, `registry-dump` and every file's diagnostics are taken after it;
     - at the end, a fresh load of the edited tree must give the same.
   - **Memory:** the dhat profile of §8, by holder. It is taken once more with untrimmed backtraces (`dhat::Profiler::builder().trim_backtraces(None)`), so that the 10,987 allocations whose stacks end inside the allocator (M10) are placed too.
4. **The baseline** comes from the commit that adds the printers and makes the deletions. Every deletion is under `cfg(debug_assertions)` or `cfg(test)`, so that commit's release behaviour is `5fb858c9`'s.
   - Each dump is taken with the `gate` build at `PHPSTAN_RS_THREADS=1` and at the default thread count, which must give byte-identical output. It is taken again with the `oracle` build, which must finish without an assertion.
   - The ordered file list, with roles, is recorded for each configuration. A tree with a link cycle is recorded as it is today: listed until `ELOOP`.
   - Everything is stored with the starting binaries: the `gate` build, `cl-20g`'s and the `dhat` build.

**Oracle.** None beyond §11: release behaviour does not change. Exact.

### 1. One session for the CLI, the language server and the tests

**Now.** Each part of the work has more than one implementation.
- **Loading.**
  - The CLI loads through `Session::load` (`session.rs:708`).
  - The language server loads through `load_workspace_files` (`lsp.rs:470-528`) and `Project::load_files` (`project.rs:1142`). These:
    - walk for themselves;
    - canonicalize each file in a serial loop (`lsp.rs:486`, `:517`);
    - read each file into a lossy copy;
    - parse with no name cache, no per-file cache and no numbering.
  - The tests, `incremental-bench` and the benches load a third way, through `load_file` plus `finish_loading`: about 120 calls in `project/tests`, the checker's helper (`checker/tests/helpers.rs:11-15`), `main/incremental_bench.rs:41-46` and `benches/vendored_corpus.rs:68`. These parse with `parse_file(.., false)` (`project.rs:944-957`). So the unit tests never run the load that items 2, 3, 5 and 9-13 change, and the bench times a load nothing ships.
- **Configuration.** The server resolves its configuration in `workspace_config.rs:59-245`, not through `Session::resolve`. The two differ:

  | | CLI (`Session::resolve`) | server (`workspace_config::resolve`) |
  |---|---|---|
  | Composer roots | per analysed directory, with path-repository promotion (`session.rs:418-470`) | the root manifest only |
  | stub paths | stub files | plain context directories (`workspace_config.rs:184`) |
  | relative paths | from the configuration file | from the root |
  | configuration errors | fatal | fall back to defaults |
  | no `paths` configured | an error | the whole root, less `vendor`, `.git` and `node_modules` (`lsp.rs:494-506`) |
  | `defer_inline_ignores` | true | false (`workspace_config.rs:199`) |
  | banned constructs, architecture, class shapes, mock hygiene | present | absent |

  A configuration reload (`lsp.rs:1954-1994`) asks for a window reload when the file set changes.
- **Analysis.** The server publishes from `check_all_analyzed_paths` (`lsp.rs:614`) and `apply_post_filters` (`:386`). `Session::analyse` (`session.rs:894-990`) also runs the whole-program passes and reports parse errors, so the server never shows those diagnostics.
- **Identity.**
  - The server deduplicates by `canonicalize`, and it skips an analysed file whose canonical path is already loaded as context, so that file is never analysed.
  - The CLI deduplicates context files only among themselves, by exact path (`session.rs:726-727`). An analysed file that is also context is inserted twice under one id, and overlapping or lexically different configured paths (`./src`) load twice (`path_order.rs:39-49`).
  - PHP identifies a file by its resolved path: `include_once`, `__FILE__` and `get_included_files()` all resolve symbolic links, and hard links are two files (checked with `php -r`). phpstan-rs's `__FILE__` is lexical (`project.rs:863-873`, `checker/includes.rs:150-165`).
- **Edits.** The edit path has rules of its own:
  - a new file's role comes from directory names (`project.rs:1846`);
  - `is_stub` is always false (`:1874-1878`);
  - a document is keyed by its URI's spelling (`lsp.rs:721-756`);
  - watched files are read with `read_to_string`, so a non-UTF-8 file is skipped (`lsp.rs:855`);
  - `update_file` builds every per-file product its own way (item 0).
- **What the server keeps.**
  - Every context file's tree (`built_once: false`, `project.rs:1065`) and every file's source (`retain_source`, `session.rs:684`): ≈420 MB and the sources on top of the 657 MB of §8 (M3).
  - `built_once` decides two things at once: keeping the as-built snapshot for later changes (`project.rs:1416`) and keeping context trees.
  - Through `Session::load`, cached context entries arrive with no tree and no source (`session.rs:640-648`). The server's lazy use-site index reads every file's tree and source (`project.rs:1712-1720`), so a cached file's references would vanish silently, and its two variants (`index_class_fn_refs` with source, `..._without_source`, `project.rs:1106-1119`) give different spans under one cache key.
- **Folders.** `add_workspace_folders` (`lsp.rs:547-571`) loads every root into one project, with the last root's configuration winning, inside the message loop (`lsp.rs:2770`).

**Design.**
- **One identity.** A file is its resolved path, as PHP's `include_once` and `__FILE__` see it.
  - Every listed path is first normalized lexically.
  - **Each link is resolved once, by `realpath`.**
    - A target under a walked root is skipped, since that root's walk lists it.
    - A target outside every root is claimed in a set keyed by its resolved path. Every walk that reaches it lists it under that one spelling, so the result does not depend on which walk gets there first.
  - **A link cycle ends** where a directory's file identity equals one of its ancestors'. This is how `walkdir`'s `check_loop` and `ignore`'s `check_symlink_loop` work; neither keeps a global set of entered directories.
    - The identity is `(device, inode)` from `fstatat`/`statx` against the directory's descriptor, never the dirent's inode.
    - On Windows, which the release builds target, it is the volume serial and file index from `GetFileInformationByHandle`, as the `same-file` crate gives them.
  - **Directories that are not links are never merged by identity.** A bind mount reached by two real paths is two directories, as it is to PHP.
  - Hard links stay two files.
  
  Item 1 brings the rule into today's walk; item 2 keeps it.
- **What the identity decides.**
  - A file's spelling, `__FILE__`, `__DIR__` and Composer owner come from its resolved path.
  - **Its role is a function of its resolved path:** the strongest configured root that contains it, among analysed, stub and context. The roots' resolved paths are known once the session is resolved.
    - A file reachable only through links to a target outside every root takes the strongest role among those links.
  - One function, `Session::classify(path) -> (FileId, role, is_stub)`, serves the load and every edit entry point: `didOpen`, `didChange`, `didSave`, watched-file create, change and delete, and workspace-folder add and remove. An alias table from path to `FileId` keeps other spellings and deleted files resolving.
- **One configuration.** `Session::resolve` is the server's resolution too, with one error policy as a parameter: fail, or degrade and report through `window/showMessage`. `workspace_config::resolve` is deleted. A reload resolves again and applies the difference in file sets: removed files go through `remove_file`, added ones through the session's reader.
- **One load.** `Session::load` reads through a file source with an in-memory overlay. The server's open documents are that overlay, and so are the tests' files.
  - The server, the tests, `incremental-bench` and the benches all load through it.
  - `load_file`, `load_analyzed_file`, `Project::load_files` and the server's walk are deleted.
  - Every read goes through the session's byte reader. `update_file` takes bytes or text, and builds the file's products through the load's own `parse_file`.
  - The read hashes every file's bytes, and `FileState` keeps the hash (items 11, 17). `handle_open_or_change`'s comparison of whole texts (`lsp.rs:735`) becomes a hash comparison.
- **One analysis.**
  - `finalize` takes the results by value. Today it clones every file's results (`session.rs:204`).
  - The server's first publish is `Session::analyse` plus `finalize`, with the server's options.
  - An edit rechecks the affected files through the same per-file check and the same `finalize`.
  - The whole-program passes run again on save, debounced.
  - `defer_inline_ignores` goes, leaving one place that applies inline ignores.
- **What the server keeps.**
  - `built_once` splits. `keeps_as_built` is set by the server, and decides only the snapshot. Whether a tree is kept is decided by the file's role: analysed files' trees are kept (item 19 refines this), and context files' never are.
  - A context file's tree comes from `ast_of`'s re-parse cache (`project.rs:890`). Its text is read on demand, checked against the kept hash.
  - Only open documents' text is kept, which `latest_text` (`lsp.rs:174`) already holds.
  - The class and function use-site index becomes one per-file product, computed with the source when the file is parsed, and stored in the per-file cache.
- **Folders.**
  - Each workspace folder has its own session and project, with its own configuration.
  - A document belongs to the deepest folder that contains it.
  - Adding a folder is a new session load; removing one drops its project.
- **Off the message loop.** The load runs on the pool and reports through `$/progress`. Edits that arrive meanwhile are queued as overlays.

**Oracle.**
- **Without a baseline:** for the same workspace and configuration, the server's first project dump, `registry-dump` and published diagnostics equal the CLI's views of the same session.
- **The edit replay of item 0, driven through the server's entry points.** It adds:
  - opening a document by a link's spelling;
  - a non-UTF-8 file;
  - a watched-file delete;
  - adding and removing a folder;
  - a configuration reload that changes the file set.
- **Semantic, listed in §11:**
  - a file reached through a link is loaded once and reported under its resolved path, with `__FILE__` and `__DIR__` resolved;
  - a file both analysed and context is analysed, once, in both;
  - overlapping configured paths are checked once;
  - non-UTF-8 files are read as bytes in the server;
  - stub paths are stubs in the server;
  - the server resolves Composer roots, promotion, relative paths and the no-`paths` default as the CLI does;
  - the server publishes the whole-program diagnostics and parse errors.

**Memory.** The server stops keeping context trees and the sources of files that are not open: ≈420 MB and the sources for this workspace (M3).

### 2. The parallel walk, and each file's path data from it (§4)

**Now.**
- **The walk is the critical path.** In the cached run's samples, the main thread walks, sorts and lists from 0.000 s to 0.146 s while every worker is idle (their first samples are at 0.149-0.152 s): ≈9% of the run's wall time. Of the main thread's 51 samples in that window, 40 are the walk and the sort and 12 are `statx`. The vendor thread is done by 0.077 s.
- **`collect_php_files_excluding`** (`project.rs:3138-3164`) calls `is_dir()` on every entry: 16,773 `stat`s. It follows symbolic links with no cycle check.
- **Each walked path is respelled.** It is joined to the root, then stripped back to its configured spelling (`session.rs:606-616`): `strip_prefix` 34.3 M over 12,507 files.
- **The context list is mostly duplicates, sorted twice.**
  - The vendor context lists the classmap's files one by one *and* the PSR-4 directories that hold them (`composer/src/vendor.rs:31-36`). The main thread walks those directories.
  - `sort_paths` sorts the analysed list and the context list (66.9 M, serial, comparing `PathBuf`s by component), and only then does `dedup` drop ≈9 K duplicates, leaving 11,459 (`session.rs:713`, `:726-727`).
  - `discover_vendor_context` sorts its lists once more on its thread (45.5 M, `vendor.rs:394-396`).
  - `sort_paths` already merges a few sorted runs (`path_order.rs:39-49`), so a k-way merge over ≈10.8 K one-file runs is a full sort again.
- **The vendor resolver keeps directory listings of its own** (`Resolver::listings`), apart from the walk's.
- **The stub directories are walked twice:** as scan directories, and again for the set of stub paths (`session.rs:739-744`).
- **Each worker recomputes what the walk could have handed it: 157 M in all.**
  - `absolute_path` normalizes the path, 54.1 M.
  - `PackageIndex::owner` walks a directory trie per file, 69.1 M; across the run, `Components::next` costs 139.4 M.
  - `FileTable::id_for` hashes the path, 33.5 M; across the run, `Path` hashing costs 65.5 M.
- **The file table stores each path twice:** as the key of `ids: FxHashMap<PathBuf, FileId>` and in the slot (`file_table.rs:18-31`; 28,502 buffers, M8).

**Design.**
- **One listing per directory per load.**
  - The vendor resolver's listings become the walk's directory table.
  - The vendor context lists each file once. A classmap or `files` entry under one of the context's PSR-4 directories is dropped at discovery, by a trie of the resolved directories.
  - The stub directories are walked once, with the role "stub".
- **One task per directory on the pool.** A task lists its directory and sorts the entries by name bytes, then spawns its subdirectories.
  - An entry's kind comes from the dirent (`DirEntry::file_type`), with no `stat`.
  - Links follow item 1's rules: each is resolved once, and cycles are found against the task's ancestors.
  - A walk prunes the subtrees of stronger roots inside it, and of promoted packages (`is_promoted`). So each file is listed once, by the walk of its own role, and its role is final before it is parsed. Files reached only through links outside every root are the exception: they wait for the walk's end (item 1).
  - `collect_php_files_excluding`, `path_under_excluded_dir` and `DEFAULT_SCAN_EXCLUDED_DIR_NAMES` are deleted. No configured exclusions exist, and after item 1 no walk uses a list of directory names.
- **Spellings are built, not respelled.** A task carries its directory's configured spelling, its resolved path and its path hash. A child's are its parent's plus its name.
- **Parsing starts as directories are listed.** Each listed directory's PHP files go to the pool as one task, parsed in order with the worker thread's name cache. That cache is thread-local, owned by the load, and cleared when the load ends, so a server's long-lived pool keeps nothing between loads.
- **Order without comparisons.**
  - Each result carries its root's index and the index path of its entries from that root. That key compares exactly as path order does.
  - The roots are sorted in component order, and roots nested in others are dropped (item 1). The remaining subtrees are disjoint, so the list is the concatenation of the runs, in root order.
  - An explicit file under no root is placed by one binary search.
  - Results are placed by key after the walk, and ids follow the placement: today's numbering by position, until item 5 assigns ids by prefix sum. A debug assertion checks the order.
- **Path data from the walk, all resolved (item 1).**
  - A root's absolute path comes from one `realpath`, and a link target's from that link's `realpath`. A child's is its parent's plus its name; a file link's is its target's.
  - The package index stores each package directory once, under its resolved path, when the session is resolved. `owner_of_dir(d)` is a single trie lookup of `d`'s resolved path, made once per directory, with the index's tie rule (deepest, then last listed). The language server asks it for a single file's directory, so there is one implementation.
- **A path is stored once.** `FileTable`'s map becomes a `hashbrown::HashTable<FileId>` hashing the slot's path, with the hash the walk computed.

**Oracle.**
- **The file lists, with roles and resolved paths, in order:**
  - at `PHPSTAN_RS_THREADS=1`, at the default thread count, and on repeated runs;
  - over the fixtures and the corpus;
  - over a generated link tree, whose expected list is written by hand. It holds:
    - two links from roots of different roles to one outside directory;
    - a link from an analysed root into a context root, and one back;
    - a self-cycle and a mutual cycle;
    - a file link, a dangling link and a link to a non-PHP file;
    - an unreadable directory;
    - a classmap entry under a PSR-4 directory.
- **O2.**

Exact against item 1's results. Against the baseline, a link cycle's tree is listed once instead of until `ELOOP` (§11).

**Saving.** ≈0.3 G: `sort_paths` 66.9 M, the respelling 34.3 M, the owner 69.1 M, the absolute path 54.1 M, `id_for` 33.5 M and the vendor sort 45.5 M. Also the serial start, ≈0.15 s.

**Allocations: ≈0.56 M (M10).**
- **What goes once the walk builds each path once:**
  - the walk itself: `collect_php_files_excluding` 90,669, `Session::collect_php_files` 28,633;
  - the path work on the workers: `lexically_normalized` 101,187, `PackageIndex::owner` 40,998, `absolute_path` 31,182;
  - vendor discovery: `Resolver::list` 62,316, `eval_concat` 61,591, `Resolver::canonical` 25,316;
  - the sorts' keys (21,734) and the list copies (13,681).
- **Two more go:**
  - `installed.json` and the Composer manifests are deserialized straight into their typed form, not through a `serde_json::Value` tree (`read_installed`, 28,132).
  - `eval_concat` (`vendor.rs:364-375`) builds a generated autoloader's path expression into one buffer. Today it copies each operand into a `Vec` of its own and joins them.
  - `ignore_errors::filter` matches a file by the text the walk already built. Today it formats each file's path again (`ignore_errors.rs:139`).

### 3. Context trees dropped on the worker (R1)

**Now.**
- Workers parse every file and return its tree. The main thread drops each of the 11,459 context trees inside the serial insert loop, through `kept_ast: ast.filter(..)` (`project.rs:1065`).
- That is 575.5 M serial, 343.4 M of it cross-thread frees, and the run's `mi_free` cost per call nearly doubles (23 → 42.5 Ir).
- The load collects every file's result before that loop starts (`session.rs:748-777`). So every context tree is alive at once: ≈420 MB of the 866 MB peak (M1).

**Design.**
- **Whether a tree is kept is one function of (role, open):** an analysed or open file's tree is kept, and any other is dropped on the thread that parsed it.
  - It holds in the load and in `update_file` alike. Today `update_file` keeps every tree whatever the file's role (`project.rs:1890`), so a `composer update` seen by the server leaves each changed vendor file's tree behind.
  - It is known before parsing, because item 2 gives each file its final role before its parse. The insert's assertion that an analysed file has its tree (`project.rs:1036-1039`) stays, and runs in the `oracle` build.
  - Item 19 refines the analysed case.
- **A context file's state has one shape, the cached one.** The worker builds it, so a parsed and a cached context file give the same state, and item 0's check that every cache state gives the same output covers both. It holds:
  - declarations and duplicate sites;
  - declared names, constants and classes, and anonymous-class names;
  - both kinds of references, and use sites when the session indexes them;
  - body summaries and flows;
  - global facts, Pest facts and file facts;
  - line starts and lexer warnings.
  
  It keeps no ignore comments; today a freshly parsed context file keeps them and a cached one does not (`file_cache.rs:137`). `ReadFile` carries no tree for such a file.
- **The drop runs in parallel,** on the thread that allocated the tree.
- **A context file is read into the worker's reusable read buffer.** Its products copy what they keep, so its source need not outlive the worker's job. Only a file whose tree is kept gets a source of its own.
- **After item 9, a file whose tree is not kept is parsed into the worker's scratch store,** whose vectors are cleared, keeping their capacity, once its products are copied out. A kept tree's vectors are moved out at their exact size.
- **`ast_of` reads the overlay before the disk,** so an open file with unsaved edits is parsed from its editor text (`project.rs:900-904` reads the disk).

**Oracle.** O2, item 0's server session and its edit replay, all run with the `oracle` build too, whose assertions fire on any read of a dropped tree. Exact.

**Memory (M1).**
- **The end of the parse falls from 866 MB to ≈446 MB.**
- **But the run's peak moves to the end of the check,** where at least the 657 MB live at exit is still live. So item 3 alone lowers the peak by at most ≈209 MB. The items that shrink what lives to the end (4, 6, 14, 16 and 19) lower it further; §12 has the end state.
- **The reads of context files stop allocating:** 66.5 MB in ≈60 K blocks today, minus the analysed files' 19 MB.

### 4. The registry by declaration ids (R3; F3 step 4 and F4 of the 25 G report)

**Now.** The finishing passes hold their thread for ≈0.54 s of the sampled run, and 0.41 s of that is serial:

| serial part | samples |
|---|---|
| purity and property writes | 97 (0.19 s) |
| `run_finishing_passes` itself | 22 |
| ancestor names | 13 |
| member owners | 12 |
| `Members::eq` | 11 |
| PHPDoc inheritance | 9 |
| deferred values | 6 |
| case canonicalization | 5 |
| the `DeclKey` maps | 5 |

The causes:
- Purity writes its answers into the shared declarations: 59,637 member copies, 47,309 `FunctionReflection` clones. `changed_since` then compares those members field by field (`Members::eq` 96.9 M).
- Property writes are keyed by `(Option<String>, String)`, hashed and compared folded (≈0.5 G).
- Every pass collects in parallel and applies serially into a `NameTable`, one hash table by name.
- The passes' scope is a `HashSet<String>`, turned back into names on every pass (`scoped_classes`, `scoped_class_names`: 32.8 M).
- `changed_since` loops serially (`registry.rs:641-676`).

**Design.**
```rust
pub(crate) struct ClassId { index: u32, generation: u32 }  pub(crate) struct FnId { index: u32, generation: u32 }
pub(crate) struct MethodRef { class: ClassId, slot: u16 }        // a slot in that class's own member list
struct Store { slots: Vec<(u32, Option<DeclAt>)>, free: Vec<u32> }  // a reused slot gets a new generation
struct DeclAt { file: Arc<FileDecls>, local: u32 }                   // a declaration is its file's entry
// Registry:
classes: Store<ClassReflection>,      class_ids: NameTable<ClassId>,
functions: Store<FunctionReflection>, function_ids: NameTable<FnId>,
ancestors: Vec<Box<[ClassId]>>, owners: Vec<OwnerTable>,           // by ClassId
purity: PurityTable, property_writes: WriteTable, phpstan_ancestry: BitVec,
```
- **Each store's ids fall in three ranges, one per source,** and the registry's accessors dispatch by range:
  - a stub base entry's id is its record's index in the stub cache's base, fixed when that cache is built;
  - a context base entry's (item 17) is its record's index in that base;
  - project ids follow. Until item 5 assigns them before the merge, they are assigned in the serial insert. A declaration merged at insert (a conditional polyfill onto a stub class, `index.rs:200-245`; context declarations onto each other) keeps the winning entry's id, with the merged value in the store.
  
  So no id is assigned while passes run in parallel, and base slots are decoded lazily, as `Base` decodes stub slots today.
- **A file's declarations are stored together, as rust-analyzer's `ItemTree` stores a file's items in typed arenas,** with `la-arena`'s arenas and ranges, as item 9's tree is.
  - `FileDecls` holds one vector per kind: classes, functions, members, parameters, properties, constants and names (for `extends`, `implements`, `use`). A class's members and a function's parameters are ranges into them. Until item 8 moves the case references to the checker, they are one more vector of the file.
  - One folded index per file gives a member's slot.
  - Per-member facts are flag bits on the member's entry. Today they are per-class name sets: `abstract_methods`, `final_methods`, `static_methods`, `final_properties` and their `_by_phpdoc` forms, plus the `redeclared_methods` map (`class_info.rs:375-409`).
  - So a file's declarations are a few allocations, not about seventeen per class (`ClassReflection::from_def`: 213,200 allocations for 12,407 classes) plus each function's parameter vector.
  - A file parsed again gets a new `FileDecls`. Its kept ids (item 5) point into it, and an unchanged file's entries are compared by their `Arc`'s address first.
  - A stub base record decodes into a block of its own in the same layout.
- **Ids carry a generation,** as `slotmap` keys do. A slot reused after a removal gets a new generation, so an id held anywhere outside the recomputed scope (the server's caches, item 7's memo, results kept across edits, other classes' tables) can never name a different declaration. The registry's id space has a generation of its own, which a switch to the PHPStan base bumps. Every cache that holds ids is keyed by it. A debug build checks an id's generation whenever it is read.
- **A class's member list has one entry for every member the class has: its own and its traits', aliases included.**
  - Entries are in PHP's declaration order, and a folded name map gives the slot.
  - Each entry points at the declaring `FunctionReflection` without copying it. It carries its own name, visibility and declaring trait.
  - A `MethodRef` is (class, slot in *that* class's list). So a trait method has one `MethodRef` per using class, as PHP binds `$this`, `self`, `static`, `parent` and `__CLASS__` in it to the using class. PHPStan's `PhpMethodReflection` likewise carries both `declaringClass` and `declaringTrait` (`PhpMethodReflection.php:69-71`). Today `flatten_traits_one_pass` copies each trait method into each using class (`resolve.rs:193-205`), and purity classifies it with the using class (`purity.rs:141-153`).
  - Inherited members are not entries of the child's list: lookups reach them through the owner table, as today.
  - Today `Members::Built` is a hash table, so code iterating a class's methods sees hash order. With the list it sees declaration order. Diagnostics are sorted by position, so the report does not move; anything else that takes the first member it finds is listed in §11.
- **Declarations never change after they are built. Every pass result is a table by id, completions included:**
  - purity verdicts and dependencies, and property-write verdicts, keyed by the `MethodRef` of the class that has the member;
  - ancestors and member owners;
  - the PHPStan-ancestry fact;
  - each `MethodRef`'s inherited doc types, computed per method by today's rule (a parameter or return type without a doc of its own takes the nearest ancestor's);
  - each class's trait members, as the entries the trait pass fills;
  - each constant's resolved value.
  
  `PurityDependency(Name, Name)` and `DeclKey` become `FnId`/`MethodRef`, so no string is hashed in a pass. The stores are the as-built state, so `raw`, `reset_scoped` (47.2 M) and the per-member copies go.
- **Passes are parallel maps.** A pass computes `(0..n).into_par_iter().map(|id| ..)` into its table, with no serial apply. Each pass names the relation it depends on:
  - **Over the hierarchy:** ancestors, owners, inherited docs and trait members run level by level over extends, implements and uses. A level only reads, and its results are stored by id before the next begins. `@mixin`s, which change member sets from any class (`hierarchy.rs:496-541`), are further edges where they are acyclic, and a fixed point after the levels where they are not. Computing the levels finds the cycles invalid code can write, and those classes get today's treatment of a cycle after the acyclic levels.
  - **Over references between classes,** such as constant values and `@phpstan-import-type`: these can name any class, not only ancestors (`resolve.rs:537-552`). They are evaluated on demand, with a memo per id and a cycle guard.
  - **The global-variable pass** follows the include graph (`global_vars.rs:368`).
  - **The purity and property-write fixed points** are worklists over id vectors.
- **The scope is a bitset over ids**, which `closure_scope` yields.
- **Snapshots and change detection by id.**
  - The stores are the as-built state, so a snapshot is a copy of their slot vectors.
  - `changed_since` is a parallel filter: pointer equality for the stores, element equality for the scope's table entries.
  - A changed verdict marks its declaration's key.
- **Readers, behind the registry's API.**
  - Today `classes`, `functions`, `global_constants` and `global_variables` are public fields (`registry.rs:226-258`), read directly at 149 places outside the registry module.
  - The item's first step makes them private behind lookups by name that return `(id, &declaration)`. It converts the 149 sites, so the compiler lists every reader the stores change.
  - The checker resolves a call to a signature through that API and asks `registry.purity(id)` and `registry.property_writes(id)`.

**Oracle.**
- `registry-dump`, which prints each class's member list in its order beside the sorted view.
- O2.
- Item 0's server session and edit replay, with one step in which a file starts naming `PHPStan\`, so the base switches mid-session.

Exact, except the member-order effects listed in §11.

**Saving.** About 0.4 s of serial wall time in the sampled run. In Ir:
- the string keys, ≈0.5 G;
- `Members::eq`, 96.9 M;
- the member copies;
- the serial applies' inserts (`precompute_ancestor_names` alone makes 25.2 M of them);
- the scope's lookups, 32.8 M.

**Memory (M4).**
- **What goes:**
  - all the copies the passes keep (43.8 MB): purity's, and those of the passes that complete declarations, which become tables;
  - the string-keyed purity and property-write tables (64.1 MB).
- **What stays, smaller:** the owner tables. They are keyed by id and folded name, with `MethodRef` values, and still hold one entry for every member a class sees (`own_owner_names` keeps 9.1 MB at exit), as PHP's own class tables do.
- **The member lists** save the `Arc` header and one allocation per method: ≈1 MB and ≈48 K allocations.
- **Total:** ≈90 MB of what the project holds after the check.

**Allocations.** Item 4's row of M10 is 2.02 M. About 1.5 M of it goes; what stays is the declarations being built (`from_def`'s own allocations, beside the ≈128 K listed below) and the owner tables' entries.
- **The passes build no text keys and no hash sets of names.** Their tables are indexed by id, and a level writes its results into them.
  - Purity and property writes: `method_key`'s 238,104 lowercased keys, `resolve_property_writes` 209,662, `Resolve::call` 72,107, `function_key` 41,541, `classify_facts` 35,179, `receiver_class` 24,957 and the parallel collect's list nodes 93,353.
  - The other passes:
    - `own_owner_names` 97,317, `RefKey::class_text` from `changed_since` and `run_finishing_passes` 46,791 (its 55,048 from `closure_scope` are item 5's keys), and `class_extends_implements_closure` 55,252;
    - `precompute_ancestor_names`, `class_reaches_phpstan_name` and `precompute_member_owners_after_mixins`, 42-44 K each;
    - `all_ancestor_names` 46,731;
    - `resolve_global_scope_files` 40,568. It builds two path-text `String`s per file on every finishing run (`global_vars.rs:368-378`), and item 2's path data gives it each file's normalized text and file name once per load;
    - `classify_facts` 35,179. It builds a map from parameter name to type for every body (`property_writes.rs:446-451`); the body summary refers to parameters by position instead;
    - `Members::get_mut`'s copies 63,117, and the parameter lists those copies clone, ≈81 K.
- **A declaration is built straight into its final form.**
  - `ClassReflection::from_def` (213,200 allocations, 70.4 MB, of which ≈128 K go) builds three hash tables per class to merge methods (`collect_merged_methods`, `class_info.rs:826-850`: a count of each folded name, the redeclared methods, and a `FoldedMap` of whole `FunctionReflection`s, 58.7 MB), plus a hash table of member spans (`MemberSpans::add_methods`).
  - Instead, the ordered member list is filled once, in declaration order, with one folded index from name to slot. A redeclared method is found by that index, and a member's span is a field of its entry.
- **Queries read the tables; they do not walk the hierarchy (M11, F6b: 149,360 allocations).**
  - Today `find_final_ancestor_method` and `..._property`, `required_abstract_methods`, `has_concrete_implementation`, `class_is_immutable` and `has_unresolvable_ancestor` each walk the hierarchy with a stack and a `seen` set on every call (`hierarchy.rs:309`, `:864`, `:917`, `:950`, `:1026`, `:1070`). They iterate the class's ancestor slice instead.
  - `visible_class_consts` clones every visible constant into a map on each query (`resolve.rs:1517`). It reads the owner table.
  - The PHPDoc-inheritance backfill builds per-class vectors of the parents' method maps (`resolve.rs:1147`: 88,501). It runs level by level over ids.
  - Attributes are returned as a slice, not collected per query (`reflection_attribute.rs:63`).

### 5. The load's merge in parallel; files by id (§4)

**Now.**
- **The insert is serial.** After the parse, one loop on the main thread inserts each file in turn: its declarations (`insert_built_decl`, 52.5 M over 15,266 calls), its references (`register_references`, 92.6 M over 28,502 calls) and its state.
  - It writes more tables than those, and not all of them are keyed by the declaration's name:
    - the assert-tagged names, by the function's short name (`index.rs:461-475`);
    - the unconditionally declared classes and functions, the function declaration sites and the stub-provided functions (`index.rs:256-420`);
    - the changed sets, `global_constants` and the before-values of changed constants;
    - the PHPStan class files (`project.rs:1047-1049`);
    - both duplicate-site tables;
    - the use sites, indexed both by file and by target;
    - the origin map, keyed by an `Arc`'s address (`file_table.rs:96-101`);
    - `globals_stale`.
- **Two sorts compare paths by component (20.7 M each).** Neither sorts the diagnostics.
  - `Session::analyse` sorts the named-argument facts (`session.rs:939`). Those facts are recorded and sorted whether or not their rule is on (`named_argument_renames.rs:51-80`); only `check_renamed_named_arguments` reads them (`project.rs:2733`).
  - `judge_property_types` sorts its sources by path (`project.rs:2240-2251`). It then appends each class's property types in that order, so the order decides the order of union members in its messages.
- **File ids are not in path order.** The load numbers files by position in context files followed by analysed files. A path the server adds later takes the next id (`file_table.rs:72-88`).
- **Text where keys exist.**
  - `closure_scope` builds its keys from text: 16.5 M and 55,048 allocations. 37,452 of these come from its frontier, which starts from the changed names as `String`s (`project.rs:1304-1316`). The other 17,596 come from its walk of stub files, which reads each stub's names as `&str` and makes a key and a `String` of each (`:1352-1363`).
  - `changed_since`'s `RefKey::class_text` (28.4 M) is item 4's.

**Design.**
- **Ids before the merge, deterministic.** Each file's declarations take a contiguous range, given by a prefix sum of the per-file counts in load order.
- **Stable ids on the edit path.**
  - A file parsed again keeps its declarations' ids, matched by kind, folded name and position among same-named declarations in the file. A new name takes a free slot with a new generation (item 4).
  - `changed_since` compares the old and new values of a kept id; pointer equality is only its fast path. Without this, an unchanged declaration parsed again would look changed, and every dependent would be rechecked on each keystroke.
- **The merge is sharded, over every table the insert writes.**
  - Each worker buckets its file's declarations and references by the top bits of their key's own hash (`RefKey::hash`, which treats a constant's namespace and name differently), not a folded hash of their text.
  - Tables keyed by the declaration's name become sub-tables in the same shard, so each shard's merge owns its sub-tables outright. This covers `NameTable` (one `FoldedMap` today), the reverse indexes, the unconditional-declaration sets, the declaration sites and the stub-provided functions.
  - Shards merge in parallel, each over the files in load order, so the precedence rule holds: context files first, an analysed file's declaration winning a collision.
  - Tables with another key are filled in a second parallel step, bucketed by their own key: the assert set by short name, use sites by target, the duplicate sites.
  - Per-file state, the origins and `globals_stale` go in by `FileId`.
- **Whole-program merges go by path rank.**
  - Each file has a rank: its place in path order across all roles. The load computes it once from item 2's placement, and the server computes it again before a whole-program pass.
  - `judge_property_types` and the named-argument merge sort by rank, so they sort integers in the same order as today.
  - Named-argument facts are recorded and merged only when `check_renamed_named_arguments` is on.
- **Keys stay keys, and files stay `FileId`s,** in:
  - `walk_affected` and the scope bookkeeping;
  - the duplicate sites (today a `PathBuf` per declaration site, `duplicate_class_sites.rs:65`);
  - the declarations' origins (today an `Arc<Path>` per file, bound to its id through an address map, `file_table.rs:101`).
- **The frontier starts from ids.** `closure_scope`'s frontier starts from item 4's changed ids, each through its declaration's `Name` (`RefKey::class(&name)`, no allocation). The stub dependency index holds `RefKey`s and stub base ids, not text. The scope it yields is item 4's bitset.
- **The indexes hold posting lists, not sets (M8).**
  - **Today** a reverse index is `FxHashMap<RefKey, FxHashSet<FileId>>` (`project.rs:3229-3245`): a hash set for each of 45,696 keys, most of them naming one file.
  - **A posting list** is a sorted `FileId` list, with one id held inline and no allocation. File ids are never reused (`file_table.rs:14-17`), so an edit inserts and removes by binary search, touching only the keys whose membership changed.
  - **A file's own references** (`FileState::references` and `declaration_references`, a `HashSet` each) become slices, built once when the file is parsed.
    - **Order:** by kind, then the key's hash as `RefKey::hash` computes it, then its folded bytes. That order agrees with `RefKey`'s equality, which ignores case for classes, functions and a constant's namespace (`references.rs:104-134`), and it depends only on content.
    - **Duplicates:** a slice is deduplicated by equality.
    - **Edits:** an edit compares the old and new slices by a merge.
  - **The per-file cache** stores the slices in that order (`file_cache.rs:85`), and a debug build checks the order when it decodes them.

**Oracle.**
- `registry-dump`: the reverse indexes, the use-site index and the duplicate-site tables.
- O2, including the messages of `judge_property_types` and the renamed-named-argument rule, whose type order follows the merge order.
- Item 0's dumps at one thread and at the default count, for the sharded merge.
- Item 0's server session and edit replay, for posting-list edits and stable ids.

Exact.

**Memory.**
- **≈5 MB:** most of the 3.0 MB of inner sets (35,769 of the 45,696 are the smallest table, 36 B), and about a third of the 6.4 MB of reference sets, whose slack goes.
- **Up to 45,696 allocations:** one for each key that only one file references.

### 6. The stub scope (R5; F2's other half)

**Now.**
- `resolved_deferred_class` asks `class_has_deferred_type` of every scoped class (`resolve.rs:560-592`). That iterates the class's methods, which decodes 22,746 lazily stored stub methods (178.3 M, plus 55.1 M in the pass), to learn what its own comment says: stubs have none.
- `warm` initialises 6,279 stub class entries, where the 25 G run initialised 6,320. The version guards built so far shrank nothing here.
- Decoding a stub record re-interns every name in it: ≈310 K of the 766 K `Name::from(&str)` calls, ≈70 M, plus their UTF-8 checks (≈35 M).
- **The cache is read whole into the heap, and its records are copied out of it (M5).**
  - `load_container` reads the file into a `Vec`: 20.6 MB, plus 2.0 MB for the stub index.
  - Every lazily stored method of a decoded class copies its bytes into an `Arc<Vec<u8>>`, and keeps them after it is decoded.
  - Every such method reserves room for its decoded value inline: 352 B, decoded or not.
  - At exit these records hold 67.6 MB.

**Design.**
1. **Deferred types are resolved on demand,** as item 4 resolves every reference between classes.
   - A declaration's resolved types are a table entry, computed the first time they are read. Today an eager pass maps every scoped class through `class_has_deferred_type` and rewrites deferred function signatures in place (`resolve.rs:537-592`).
   - That pass goes, and so do `class_has_deferred_type` and `function_has_deferred_type`.
   - The stub cache already stores the stubs resolved (`stub_cache.rs:1-3`). Building it asserts that no stub record holds a deferred type.
   - Exact.
2. **Guards on the stubs' availability, decided by what is true at run time.**
   - **The stub index records each declaration's PHP version range,** read when the stub cache is built:
     - phpstorm-stubs' `#[PhpStormStubsElementAvailable(from:, to:)]` (598 uses in `vendor/phpstorm-stubs`) and `@since`/`@removed` (249 files);
     - PHPStan's version deltas (`vendor/phpstan-src/resources/functionMap_php*delta.php`, `new` and `old`).
   - **It records the versions of what is inside a declaration too,** as item 9's stub rules hand them over: a parameter's range, and each version's type from `#[LanguageLevelTypeAware]`. Today the stub rules apply the newest PHP version's types and drop what that version removed (`newest_type`, `stub_attributes.rs:74-100`), whatever versions the project is configured for.
   - **A reader answers for the configured versions** (`Registry::php_versions`, `project.rs:491`).
     - Where every configured version gives one signature, it is that signature.
     - Where they differ, a call is checked against each version's signature, and a return or property type is the union of the versions' types. Every possibility PHP may run with stays.
     - The stub cache records every version, so it stays independent of the configuration.
   - **It also records each declaration's extension.** phpstorm-stubs are laid out by extension, one directory each.
   - **An extension counts as loaded only on evidence that holds at run time:**
     - **PHP cannot be built without it:** Core, date, pcre, Reflection, SPL and standard; hash from 7.4, json from 8.0, random from 8.2.
     - **Composer's generated `vendor/composer/platform_check.php` tests it with `extension_loaded`** (with `platform-check: true`), so no code loaded through the autoloader runs without it. Piwigo's checks only the PHP version.
     - **The configuration names the target runtime's extensions,** as a list or as the `php` binary to ask (`php -m`).
   - **What is not evidence:**
     - `require: ext-*` alone is checked only at install time, on whichever machine installs.
     - `config.platform` only tells Composer what to assume while resolving.
   - **A guard is decided only on that evidence.**
     - `!function_exists`, `!class_exists`, `!interface_exists`, `!trait_exists`, `!enum_exists` and `!defined` evaluate to `Never` when the stub's version range covers the configured range and its extension is known to be loaded in every configured version.
     - They evaluate to `Always` when the stub is known absent: a declaration removed before the configured range began, or an extension known not to be loaded.
     - Otherwise they stay unknown, and both declarations stay. For example, `symfony/polyfill-mbstring` declares `mb_convert_encoding` under `!function_exists(..)` (`bootstrap72.php:14`), and Piwigo's evidence leaves mbstring unknown.
   - **New: a polyfill must match what it replaces.** When a guarded user declaration and a stub declaration can both be the one PHP runs, a check reports where the polyfill's signature differs from the native one: parameter count, by-reference and variadic parameters, parameter types, and the return type. Calls are checked against either, so a call valid for one and not the other is reported.
   - Semantic.
3. **Names are stored once per cache file.** A cache file holds a table of its distinct names. Table keys already refer to it (`container.rs:96-104`), and now records refer to names by index too, where today they embed the text. A run interns an entry the first time a record needs it (a `OnceLock<Name>` per entry). The per-file cache does the same, with one table per file. Format bump.
4. **F2's count, as a diagnostic.** A debug-only report of the stub keys in the passes' scope, by the changed name that pulled each in, to explain whatever remains. It gates nothing.
5. **Records are read, not mapped or copied.** This goes in the same format bump as step 3.
   - **What loads eagerly:** the directory, the tables' keys and the name table are read into the heap when the file loads. They are all needed at once, and they are a fraction of today's 20.6 MB.
   - **A lazy record is its offset and length,** beside a shared `Arc<File>`. Decoding reads it with `FileExt::read_at` (`seek_read` on Windows) into a per-thread scratch buffer and allocates only the decoded value, in a box of its own (`OnceLock<Box<V>>`). Nothing keeps a record's bytes, and an undecoded member costs a few words, not 352 B.
   - **The decode context.** Decoding runs under a scoped context that holds the file handle and the name table, as rustc decodes crate metadata through its `DecodeContext` and `LazyValue<T>`.
     - `Lazy`'s `Deserialize` takes the borrowed bytes and records their range within the context's file. Today it needs `V: DeserializeOwned` and copies them (`name_table.rs:1218-1226`).
     - `Name`'s `Deserialize` reads an index into the context's name table.
     - Decoding a lazy record later sets the context again, from the record's own handle.
   - **Integrity is checked per record.**
     - Each record carries its own CRC, checked before it is decoded. The header's CRC covers only the sections read eagerly. Today one whole-file CRC is checked at load (`container.rs:224-232`), and a record decoded later is not covered by it.
     - A mismatch or a decode error is recoverable: it is logged, the cache is rebuilt by the next process, and the declaration reads as missing for now. Today a decode failure panics (`decode_entry`).
   - **No `unsafe`, and no mapping.**
     - The workspace denies `unsafe_code` (`Cargo.toml:42`), and `UNSAFE_PLAN.md` moves towards forbidding it.
     - A mapped file is undefined behaviour if it is modified "in or out of process" (`memmap2` `lib.rs:141-147`). Truncation by another program, or a prune by another machine sharing the cache directory over NFS, would raise SIGBUS in a running server.
     - SQLite keeps memory-mapped I/O off by default for the same reason.
   - **Replacing a file in use.** A file that fails its checks is closed before the rebuild stores its replacement. Because `std` opens files with `FILE_SHARE_DELETE`, Windows lets a store or a prune replace a file that a running process still reads. Pruning already ignores a file it cannot delete (`stub_cache.rs:152`).
   - **Item 17's context base shares this container,** with its own name table, per-record CRCs, lazy records read with `read_at`, and `u64` offsets (today's are `u32`, `container.rs:84-93`). A per-file cache entry is small and wholly needed, so it is still decoded whole.
   - **The stub index's keys are read with the eager sections,** not copied into a `Box<str>` each (`stubs.rs:923`).

**Oracle.**
- `registry-dump`: the scope, the guards, the extensions, and every stub record decoded.
- O2 with the stub cache cold and warm.
- A test that loads the cache, then stores, prunes and reloads it while a reader still holds the old file. It runs on every platform the release builds; today CI runs on Ubuntu only (`rust-ci.yml:18`).
- A test that corrupts one record and expects a rebuild, not a panic.

Exact for 1, 3 and 5; semantic for 2, listed in §11.

**Allocations: ≈0.45 M (M10).** Decoding a stub class (266,562 allocations) or function (136,911) allocates every member, parameter and name it holds, whether it is read or not. After steps 1, 3 and 5, a run decodes only the records it reads, and its names come from the table.

**Memory.** About 40 MB of heap at exit (M5):
- the container's 22.6 MB, less the eager sections, which stay;
- 8.6 MB of byte copies;
- 12.6 MB of inline reservations;
- the 22,746 methods that step 1 no longer decodes.

### 7. One memo for the type relation (§6)

**Now.**
- **The question.** `Relation::is_super_type_of` is asked 1.36 M times (≈0.47 G inclusive across the threads).
- **Two memos exist today:**
  - the structural one (`structural_memo`, `super_type.rs:594-626`);
  - `KEEP_DECISIONS`, which holds registry-backed answers within one finishing pass, keyed by type identities and a pass number (`resolve.rs:1893-1925`, `:2178-2190`).
  
  The checker's registry-backed questions are not memoized.
- **Bounds.** The structural memo and its kin empty a thread's table when it fills: `structural_memo` at 2^17 entries (`super_type.rs:604-618`), `keys_superset_memo` and the intern memo at 2^16. `KEEP_DECISIONS` has no bound within a pass.
- **What an answer reads.** It is a function of the two interned types and of the registry. Through `ClassHierarchy` (`types/src/hierarchy.rs:17-62`) it reads classes, methods, signatures, return and property types, functions, variances and ancestor template bindings, not only extends and implements.
  - The three settings at `relation.rs:15-23` are read only by `accepts` (`accepts.rs:332-630`), not by `is_super_type_of`.
  - Types name classes by `Name`, so ids never enter the question.
- **When the registry is not fixed:**
  - the finishing passes ask while they change the registry (`excluding_native`, 13,470 times);
  - declaration building asks the structural relation on the workers (`declared_type`, 37,181 times).

**Design.**
- **`ClassHierarchy::generation() -> Option<u64>` is the registry's content generation.**
  - It returns `Some` whenever the registry cannot change until its next bump: after a finishing run, and during each read-only level of item 4's passes, which takes a generation of its own.
  - It returns `None` only while declarations are being inserted or a level's results written. Nothing is kept then.
  - Every finishing run bumps it: an edit, a removal, a configuration reload, a stub sync. A switch of base bumps it too, along with item 4's id-space generation.
  - `NoClasses` returns `Some(0)`.
- **One memo replaces `structural_memo` and `KEEP_DECISIONS`.**
  - Its key is `(sup node, sub node)`, under its generation; if `accepts` is ever memoized, its key adds the three settings.
  - Its entries hold the two types, so their addresses stay theirs.
  - A thread that meets a generation other than its memo's empties the memo.
- **Answers are stored only when they are final.**
  - While an on-demand evaluation of items 4 or 6 has a cycle guard open on the thread, the relation's answers are provisional and are not stored.
  - On-demand evaluation resolves a cycle by its strongly connected component, by what PHP does, so a stored answer never depends on which member was asked first:
    - Constants on a cycle throw when read, whichever is read first ("Cannot declare self-referencing constant", checked with `php -r`). Each member's read is `never`, and each gets a diagnostic, since reading it can never succeed.
    - A cycle among PHPDoc-only references, such as type-alias imports, has no run-time meaning. Each member keeps every possibility (`mixed`).
  - Answers are stored once the component is resolved.
- **The memo is bounded within a generation.**
  - A batch check is one generation, so emptying per generation is not a bound.
  - Each thread's memo holds at most 2^17 entries, as today. When the current segment fills, the older one is dropped: two segments, so recent answers survive, where today's tables are emptied whole.
  - Entries hold their types, and the bound limits how many dead types the memo keeps from item 16's reclamation.
- **Before the memo goes in, the relation's code is read for any input beyond these.** That means any thread-local, the current class, or a `static` resolved from context. Any such input joins the key or is resolved before the question is asked. (The relation keeps no depth or visited-set state today.)

**Oracle.**
- **A runtime switch** (`PHPSTAN_RS_NO_RELATION_MEMO`, read once into a static) turns the memo off in any build. Today's `disable_shortcuts` exists only under `#[cfg(test)]` (`types/src/lib.rs:176-200`), so the `gate` build cannot reach it.
- **Compared with the memo on and off:**
  - O2 and `--dump-expr-types`;
  - item 0's server session and edit replay, which cross generations;
  - each at one thread and at the default count.

Exact.

**Memory.** Each thread's memo is bounded as today's (2^17 entries; the structural tables hold 9.4 MB at exit, M7), and keeps the more recent half when full. `KEEP_DECISIONS`' unbounded table goes (1.3 MB at exit).

**Saving.** Up to the registry-backed part of the ≈0.47 G, which this profile does not separate from the already memoized structural part.

### 8. Declarations carry only what every reader needs (§3.2)

**Now.**
- **The declarations carry six case-reference fields,** lists of class names as written beside their canonical spelling:
  - `ParameterReflection.case_refs` and `FunctionReflection.return_case_refs`, built by `declared_type` for every parameter and return type of every declaration of every file (`type_resolution.rs:56-89`: 35.1 M from native hints, 36.3 M walking doc types);
  - `PropertyReflection.case_refs` and `.set_case_refs`, for property hooks (`class_info.rs:156-163`);
  - `ClassReflection.type_alias_case_refs`, a `NameMap` (`:532`), and `.mixin_case_refs` (`:579`).
  
  They are serialized into both caches.
- **Only the checker reads them, while checking the declaration they belong to:**
  - functions (`function_decl.rs:516-517`, `:698-714`);
  - `@method`, `@mixin` and `@phpstan-type` tags (`oop.rs:263-272`, `:384-385`, `:609`);
  - properties and `@property` tags (`generics/bounds.rs:210-211`, `:295-296`);
  - set hooks (`oop/property_hooks.rs:449`).
- **The checker already computes the same lists from the AST** for closures and arrow functions (`resolve_type_hint_with_case_refs`, `expr.rs:3668`, `:4004`), and for functions without a signature (`function_decl.rs:638-666`).
- **One reader checks a merged reflection, not what is written.** A `@property` tag is looked up in the resolved properties, so a tag shadowed by a declared property is checked against the declared property's type (`generics/bounds.rs:255-264`).
- **Other fields read only while their own declaration is checked:** `explicit_mixed_hint` and `return_explicit_mixed_hint` (`missing_typehint.rs:457`, `:539`).

**Design.**
- **Each declaration field is listed with its readers.** A field read only while its own declaration is checked moves to the checker. The case references are the costly case; the rest are flags.
- **The checker computes case references from what it checks.** From the AST and docblocks of the declaration it checks, it computes the written and canonical class names at every position:
  - parameters and returns;
  - properties and their hooks;
  - `@property`, `@method`, `@mixin` and `@phpstan-type` tags.
  
  Each position's check reads the names written there: a docblock tag has no run-time effect of its own, so what it writes is what a case check reads. Closures keep today's computation, which is the same one.
- **All six fields leave the declarations,** with a format bump. Until this item, item 4's `FileDecls` holds them as one more vector of the file, with ranges, so the item removes a vector and allocates nothing per position.
- **Duplicate parameters are resolved as a function's parameters are pushed into its file's parameter vector** (item 4).
  - A scan of the function's range finds any duplicate, and only a range with one is fixed up. Today's rule is kept: the last occurrence, at the first one's place.
  - Duplicates occur in stubs, as versioned parameters (`#[PhpStormStubsElementAvailable]`). They also occur in code PHP refuses to compile ("Redefinition of parameter", checked with `php -r`), which is reported as `parameter.duplicate`.
  - Today `dedup_versioned_params` (`function_sig.rs:1112-1125`) builds a `NameMap` and an order vector for every function: 127,004 allocations, 35.3 MB, the map dropped 63,296 times by `from_decl`. The stub cache stores the result.
- **`from_decl` passes the parameters on,** not a `Vec<&str>` of their names collected for the purpose (`function_sig.rs:872`).
- **`reconcile_with_native` refines a lone member and returns it,** with no `Vec`. Today it builds `vec![self]` (`decide.rs:22`) and collects the result again (`:25-29`). A native union whose other members must be kept takes the general path.

**Oracle.**
- O2 with every rule set on (`class.nameCase`, internal usage).
- Fixtures with a case mismatch at each position listed above.

Exact, except one case listed in §11: a `@property` tag shadowed by a declared property is checked against the tag's own names.

**Saving.** The work, not the storage: case references computed for the analysed files only (2,793), where today they are computed for all 14,251 files, plus the bytes they take in both caches. ≈0.1 G. The kept parameter lists are item 4's per-file vector, not this item's.

**Allocations: ≈0.48 M (M10).** This covers the case references' lists (`declared_type` 50,267, `collect_class_names` 43,874), `dedup_versioned_params`' map and order vector, `reconcile_with_native` (41,085) and `from_decl`'s name list.

### 9. Trees in typed vectors, nodes by index (R8; §3.2)

**Now.**
- Every `Expr` and `Stmt` is its own allocation, dropped under a stack guard (`ast.rs:1434-1446`; 131.7 M at `stack.rs:30`).
- `Expr::peeled` unwraps `(` and `@` by recursion under that guard (`ast.rs:1412-1417`): 3.08 M calls, ≈160 M.
- Source slices are copied into their own allocations: doc comments (`Arc<str>`), unescaped literals (`PhpString`) and inline HTML.
- **Text from a source that is not UTF-8 is decoded lossily** (`lexer.rs:146-151`, `:392-400`). The lexer takes any byte from `0x80` up as an identifier byte (`lexer.rs:1393`), as PHP does, but `slice` decodes the identifier's bytes lossily, on the false premise that "a range of identifier bytes always is" UTF-8.
  - So `class \xA9` (`vendor/symfony/cache/Traits/ValueWrapper.php:19`, which Composer's class map names) and `class \xAA` both become the class `U+FFFD`.
  - PHP keeps them apart. `php` 8.5 runs a file declaring both: `class_exists("\xA9")` and `class_exists("\xAA")` are both true, `get_class` gives back the byte `a9`, `class_exists("\u{FFFD}")` is false, and `getDocComment()` returns a doc's bytes unchanged.
  - 3 of Piwigo's 20,200 PHP files are not UTF-8, and 2 of its `vendor/`'s 44,931.
- **The tree is changed after the parse,** through `syntax::visit::VisitorMut`:
  - `apply_stub_attributes` drops functions and parameters, sets parameter types, appends `@return`/`@param` lines to doc comments, and runs `resolve_names` again (`stub_attributes.rs:176-186`, `:305-323`). It runs on any file that mentions either attribute (`project.rs:2897-2898`), and on the stubs (`stubs.rs:741`, `:1382`).
  - The rewritten docs are not in the parser's doc sites, so a changed file drops its recorded references and goes through the old collector (`project.rs:2906`; item 13).
  - It applies the **newest** PHP version's types and removes what that version removed (`newest_type`, `stub_attributes.rs:74-100`), whatever versions the project is configured for (`Registry::php_versions`, `project.rs:491`).
  - `name_anonymous_classes` writes each anonymous class's name into its node (`registry/anonymous_class.rs:45-73`, `:117-134`).
- **Expressions are built and kept outside the parser:**
  - the checker builds nodes: `static_call` and `constant_fetch` (`checker/expr.rs:6059-6093`), and a `Variable` (`var_tag_types.rs:538-542`);
  - completion parses a standalone expression and builds statements (`lsp/completion.rs:169-172`, `:310-326`);
  - the checker holds copies: `pending_or_disjunctions: Vec<(Expr, Expr)>` (`checker.rs:847`, filled at `stmt.rs:1440-1447`), `Initializers(FxHashMap<String, Option<Expr>>)` (`includes.rs:76`), the alias maps' `Rc<Expr>` (`checker.rs:63-144`, `locals.rs:382-391`), and both query-builder chains' `Owned(Rc<Expr>)` (`doctrine/query_builder.rs:108`, `sql_reflection/query_builder.rs:283`);
  - Composer's autoload reader takes statements by value with `into_kind` (`composer/src/vendor.rs:137-159`).
- Every parse function returns `Result<Expr, ParseError>` by value: the whole node, or an error carrying a `String`. The Pratt loop's `?` costs 55.5 M (`result.rs:2177`) and its moves 28.8 M, with the same pattern in `parse_primary`, `expect`, `parse_args` and `parse_array_items`.

**Design.**
1. **`peeled` becomes a loop**, with no guard. This step comes first, on its own.
2. **A file's tree is a handful of typed arenas, addressed by index.** `ParsedFile` has no lifetime and owns:
   - `source: Box<[u8]>`, the file's bytes as read;
   - `names: Box<[Name]>`, each distinct name of the file once, which a node refers to by `NameId(u32)`;
   - `exprs: Arena<ExprNode>` and `stmts: Arena<StmtNode>`, with their spans in parallel vectors;
   - in arenas of their own, which exist only when the file has one: the large variants that few nodes are (functions and methods, classes, closures), and the elements of lists (arguments, array items, parameters, `match` arms, attributes).
   
   Every child list is an `IdxRange` of its elements in their arena (step 7), so no list needs a vector of its own.
   
   `ParsedFile` is plain vectors, so it is `Send` and `Sync` with no `unsafe` of ours: the workspace denies `unsafe_code` (`Cargo.toml:42`), and item 19 shares trees across threads.
   
   **Why ids, not a bump arena of references.** A reference arena can be built safely too. `bumpalo::Bump` is not `Sync` (its state is in `Cell`s, `bumpalo-3.20.3/src/lib.rs:395-417`), but `bumpalo-herd`'s `Herd` is, its `Member::alloc` returns a `&'h mut T` that lives as long as the herd, and `self_cell` ties the tree to the herd. Ids are chosen on merit:
   - **Size.** A child is 4 bytes as an id, and 16 as a reference that carries its variant's tag (oxc's `Expression<'a>`). Most of a node is its children, so a node of ids is a third to a half the size.
   - **Exact size.** A bump arena grows by chunks of doubling size (`alloc_layout_slow`, `checked_mul(2)`) and keeps the last chunk's unused tail, which can be as large as everything before it. A kept tree's vectors are moved out at their exact size (step 7).
   - **No lifetime.** A tree of references puts `'a` on the checker, on item 18's scopes and on whatever the server keeps across edits, and every holder that crosses files needs a self-referential owner of its own. An id needs only its file.
   - **Side tables.** Ids are dense, so a table keyed by node is a vector indexed by id (`ArenaMap`), not a hash map keyed by address.
   - **What ids cost:** an index and a bounds check per access, where a reference is one load. The check is a branch that is always predicted.
   
   This is the layout of rust-analyzer's `Body` and Zig's AST, and of item 4's `FileDecls`.
   
   **The arenas are `la-arena`'s** (0.3.1), the crate rust-analyzer's `Body` uses. `Arena<T>` is a `Vec<T>`, `Idx<T>` a `u32` that is `Copy` and typed by what it indexes, `IdxRange<T>` a typed `Range<u32>`, and `ArenaMap` a dense side table. The crate has no `unsafe`. The types keep an expression's id from indexing statements, which bare `u32`s would not.
   - **A kept tree** is a few exact-size allocations.
   - **A tree that is not kept** (item 3) is built in the worker's scratch store, a set of the same vectors cleared between files, so a context file allocates nothing for its tree once the worker has warmed up.
   - **The lexer puts each distinct name in `names` once.** This holds whether it still streams (before item 10) or lexes the whole file first (after it).
3. **Nodes by index.**
   - `ExprId` and `StmtId` (`Idx<ExprNode>`, `Idx<StmtNode>`) are `Copy` and carry no lifetime. So no `'a` runs through the checker, through item 18's persistent scopes, or through the language server's caches.
   - An id is meaningful only with its file. A holder that crosses files keeps the `Arc<ParsedFile>` with the ids, as the re-parse cache and the debug build's bodies do (`project.rs:175-217`, `registry/bodies.rs:51`).
   - An id is its node's identity. `NodeId` numbers expressions and statements together today (`ast.rs:7-11`), and its one keyed table, the inference cache (`infer_cache.rs:47-108`), takes `ExprId`.
   - A parse function returns its node once its children are built, and its caller commits it: alone, which gives its id, or with the rest of its list (step 7). `ParseError` is boxed, so a `Result` is the node's size and a word.
   - Matching changes from `match &e.kind { ExprKind::Call(..) }` to `match file.expr(id) { ExprNode::Call { .. } }`, compile-driven. `syntax::visit` takes the file and ids.
4. **Payloads are byte ranges of the source.**
   - A doc comment, an unescaped literal and inline HTML are ranges of `source`. An escaped literal is decoded once, into a byte vector of the file.
   - A name is a `NameId`. A body is an `IdxRange<StmtNode>`.
   - **Nothing is decoded to text for its own sake, and nothing lossily but for display.** An identifier becomes a `Name` with its bytes (item 15), so `class \xA9` and `class \xAA` are two classes, as PHP sees them. A doc is parsed from its bytes (item 14), so a doc naming `\xA9` names the class the code declares.
   - What outlives the file (interned types, memoized docblocks, reflections) copies what it keeps, as now.
5. **Nothing changes a tree after its parse.** `VisitorMut`, `walk_*_mut` and `resolve_names`' second run go.
   - **The stub rules are applied as the parser builds a stub's nodes.**
     - The parser knows a file's role before it starts (item 2), and an attribute's class from the imports above it.
     - The rules apply to stub sources only. Both attributes are user classes, which PHP gives no effect unless reflection reads them, so in code PHP runs, a declaration carrying `#[PhpStormStubsElementAvailable(to: ..)]` still exists.
     - The rules record rather than choose. A declaration or parameter carries its version range, and a type-aware attribute carries each version's type. Item 6's readers then answer for the configured versions.
     - The `@return` and `@param` lines the rules add are built into the file's doc vector and recorded in its doc sites like any other doc, so a stub's references are kept (item 13).
   - **Anonymous-class names are a side table by node id,** filled on the worker from the line starts (item 11).
   - **Nodes built outside the parser:**
     - The checker's synthetic nodes go in a per-file scratch store, cleared with the file.
     - Completion and Composer's autoload reader parse into a `ParsedFile` of their own. `into_kind` goes.
6. **Holders keep ids.** `pending_or_disjunctions`, `Initializers`, the alias maps and both query-builder chains hold `ExprId`s into the file's tree.
7. **No growth inside a kept tree.** A list's elements wait in the parser's reusable scratch stack until the list ends, and are then committed together (`alloc_many`), so they are contiguous and the list is their `IdxRange`. Their ids are assigned at that commit, so whatever records an element's id during the parse records it then (item 13's declaration list). Each arena of a kept tree is moved out of the scratch store at its exact size. Today a list grows by `push` from empty and is never shrunk, so 53 MB of the analysed trees' 173 MB is capacity no element uses (M2).
8. **Nodes are small.**
   - Today every `Expr` is 80 B, every `Stmt` 160 B, an `Arg` 104 B and an `ArrayItem` 168 B, whatever they hold.
   - An `ExprNode` or `StmtNode` holds ids, ranges and flags, so a common node is 16-24 B: a binary operation is its operator and two ids, and a method call is an id, a name and a list range. A variant that would be larger, such as a closure, is an id into its own vector.
   - The span is in the parallel vector, and the id is the index.
   - A statement carries no doc fields. Today `doc_comment` and `doc_line` cost 24 B on every statement, though few statements have a doc. A statement's or a parameter's doc is found in the file's doc sites by its start offset.
     - The parser records those sites today (`Parser::note_doc`, `parser.rs:490-495`; `DocSite`, `references.rs:138-148`).
     - The readers of a statement's or a parameter's `.doc_comment` switch to that lookup. A declaration's own doc (function, class, property, constant) stays a range in its node, which is one of the large variants.
   - Attributes stay in their node, as a usually empty range: they change what PHP does at run time (`#[\SensitiveParameter]`, `#[\Override]`, `#[\Deprecated]`, `#[\NoDiscard]`), and the walks visit them with the rest of the tree.

**Oracle.**
- O1, printing the tree after the stub rules and the anonymous-class names. The printer moves with the tree.
- O2, with the stub cache rebuilt.
- Item 0's server session: open documents, edits, and completion's standalone parse.
- The `oracle` build, and the dumps at one thread and at the default count.

Exact, except the semantic changes listed in §11: non-UTF-8 identifiers keep their bytes, and the stub rules apply to stubs only, by the configured versions (item 6).

**Memory (M2).**
- **At least the 53 MB of unused capacity** in the analysed trees, 31% of their 173 MB.
- **The 1.10 M per-node allocations become a few arenas per kept file,** about a dozen for each of the 2,793 analysed files, so their allocator overhead goes too, along with the padding of every node to its largest variant.
- **The tree keeps its file's source alive** (19.1 MB for the analysed files) in place of the copies it holds today: doc comments and inline HTML through `Interner::str` (9.1 MB) and literals as `PhpString`s (4.4 MB). It costs a few MB more and saves those allocations.
- **The final heap profile measures the total.** The trees on the workers shrink likewise, and with them the peak once item 3 is in.

**Saving.**
- The AST's share of the parse job's allocation, ≈0.3 G.
- The node frees, ≈0.3 G.
- The drop guards, 0.13 G, and `peeled`, 0.16 G.
- The result moves, ≈0.1-0.2 G.
- The copies of source slices, and the second name resolution of a file the stub rules change.

**Allocations: ≈3.92 M of the run's 19.4 M (M10).** Counted as M10 counts: each allocation goes to the innermost project function that is not a container, conversion or utility, and to the item whose design removes it. `parse_qualified_name`'s 321,919, which a first matching by function had put here, are item 12's.
- **What goes with the per-node allocations:**
  - every `Box<Expr>` and `Box<Stmt>`: `parse_postfix` 537,753, `parse_expr_bp_inner` 271,838, `parse_assignment_tail` 244,768, and so on;
  - every list: arguments 258,870, statements 166,525, array items 105,245, parameters 46,703, and so on;
  - the payloads: single-quoted literals 352,352 and doc comments 72,171.
- **Three builders allocate where nothing is kept.** The scratch stacks of step 7 take them over, and item 10's inline tokens the interpolated strings:
  - `parse_type_hint` builds a `Vec<Vec<TypeHint>>` for every type hint, `int` included: 468,730 temporaries (`type_hints.rs:102`, `:105`, `:135`), beside the 11,843 boxes of nullable types (`:142`), which are nodes;
  - interpolated strings: the lexer's fragment scan (`scan_interpolated`, `strings.rs:383-396`, 454,535 with the `PhpString` conversions it asks for) and the parser's parts (`build_interpolated`, 154,592);
  - each `use` import, in a box of its own (`stmt.rs:255`, 62,346).
- **The checker's copies of expressions** become ids (step 6). M10 charges 52,349 allocations to the clones themselves (`Expr` and `ArrowFnExpr`), and 81,615 lie under a clone of the tree. By the function that asks for the copy:
  - the Doctrine and DBAL query-builder chains (`doctrine/query_builder.rs:134`, `:161-163`; `sql_reflection/query_builder.rs:336-338`), 60,481;
  - the disjunction pairs `check_if` keeps (`stmt.rs:1442`, `:1446`), 16,994;
  - the scope aliases (`narrowing.rs:1313`, `:1399`), 3,931;
  - `Initializers` (`includes.rs:83`), 209.
- **Two more lexer payloads allocate where nothing is kept:**
  - heredoc indentation stripping builds two vectors per heredoc (`strings.rs:785`, `:788`). It decodes into the file's byte vector in one pass.
  - a `\u{..}` escape builds a `Vec` of its UTF-8 bytes (`strings.rs:814`). It writes them in place.

### 10. Tokens in flat arrays (R2)

**Now.** §3.1. Five functions per token, and a 64-byte value moved through each: 1,773 M of plumbing, plus `drop_glue::<Token>` 103.3 M and `drop_glue::<Tok>` 73.4 M. This is the 25 G report's P6(d), built as designed. It was right to drop the index-keyed maps and the per-token values, and wrong to stream: the parser's lookahead never needed it, and streaming put a function boundary and a 64-byte move between lexer and parser for every token.

**Design.** The lexer lexes the whole file in one loop into a `TokenStream`:
```rust
pub struct TokenStream {
    pub kinds: Vec<TokenKind>,        // one byte: 73 token kinds and 62 keywords fit
    pub spans: Vec<Span>,
    pub payloads: Vec<Payload>,       // one per payload-bearing token, in order:
                                      // Name(NameId) | Int | Float | Str(a range of the source or of the file's decoded bytes) | Html(a source range)
    pub docs: Vec<(u32, Span)>,       // each doc comment, with the index of the token it precedes
    pub errors: Vec<LexError>,        // each lexer error, with its offset
}
```
- **The parser keeps three cursors:** token, payload and doc.
  - `peek_at(n)` is `kinds[pos + n]`, at any distance.
  - A payload moves out when its token is taken.
  - A keyword's spelling, when it stands as a name, is the source slice at its span.
- **Names.** The lexer's name table is scratch: it maps a spelling's bytes to its `NameId` in the file's `names` (item 9), which owns the names.
- **Escaped literals** are decoded by the lexer once, straight into the file's byte vector (item 9). `Str` is a range of the source or of that vector.
- **Buffers.** The vectors are per worker thread for the length of a load, as the cross-file name cache is. They are cleared, not freed, between files, and emptied when the load ends.
- **`__halt_compiler` ends the stream, as it ends PHP's lexing.**
  - PHP's lexer takes the three tokens after `__halt_compiler` (any case; whitespace and comments between them are skipped), then returns the rest of the file as one inline-HTML token, whatever it holds. Its grammar then requires `( ) ;` or `( ) ?>`. `php` 8.5 confirms: `__halt_compiler(1);` is a syntax error, and `__halt_compiler /* c */ ( ) ?>tail` lexes `tail` as inline HTML.
  - Today the parser stops the lexer at `__halt_compiler` followed by `(` (`parser/stmt.rs:389-400`, `parser.rs:351-362`), so the data after it, often a phar's binary payload, is never lexed. A lexer that ran over the whole file first would lex that data as PHP.
  - So the lexer does as PHP's does: it emits those three tokens, records the offset where the data begins, and stops. The parser reports what PHP's grammar rejects there.
  - **`__COMPILER_HALT_OFFSET__` gets its value.** In a file with `__halt_compiler`, it is that offset (just past the `;` or `?>`: 61 and 59 in the probes). In any other file, PHP throws `Error: Undefined constant "__COMPILER_HALT_OFFSET__"`, which a new check reports. Today it is `int<1, max>` everywhere (`predefined_constants.rs:67`).
- **Interpolated strings are lexed inline, as PHP's scanner emits them.**
  - `token_get_all` gives a string's parts as tokens of the one stream: literal parts, `{$` (`T_CURLY_OPEN`), `${` (`T_DOLLAR_OPEN_CURLY_BRACES`), a simple variable with its `[` or `->` offset, and the closing `}`.
  - Today a `{$…}` part is kept as text, and `build_interpolated` lexes and parses it again with `Lexer::fragment` and a parser of its own. That costs a clone of the `NameContext`, a fork of the reference recorder, and the node counter copied in and out (`parser/interpolation.rs:21-45`).
  - The lexer emits the parts inline, and the parser reads them from the same stream. A literal part is a range of the source or of the decoded bytes.
  - `Lexer::fragment`, the scratch streams, the sub-parser, the context clone, the recorder fork and the `Parts` payload all go. So do `scan_interpolated`'s buffers (item 9).
- **The file's error is the first one in the file, as PHP reports it.**
  - Today a lexer error anywhere is the file's error, ahead of a syntax error found earlier in the file (`parse.rs:115-128`).
  - PHP's parser pulls its tokens from the lexer, so it reports whichever error it meets first. `eval("$a = ;\n$b = 08;")` reports the syntax error on line 1; `eval("$a = 1;\n$b = 08;\n$c = ;")` reports "Invalid numeric literal" on line 2.
  - The stream records each lexer error with its offset, and the parse stops at its first error. The file's error is whichever of the two comes first.
- **A declaration's doc is the one PHP gives it.**
  - PHP keeps the last doc comment it lexed. A `}` clears it, and a declaration takes it: a function, method, class-like, closure, arrow function, anonymous class, property, class constant or enum case. Neither a `{`, a statement nor `?>` clears it.
  - `php` 8.5 confirms, through `getDocComment`:
    - `/** first */ /** second */ function f` gives `f` the second;
    - `/** d */ $y = [1]; function j() {}` gives `j` `/** d */`, and so does `/** a */ { function k2() {} }`;
    - with `{ }` or `if (true) { }` before the function, it gets none;
    - `/** h */ $z = function() {}; function m() {}` gives the closure the doc and `m` none, and an arrow function or an anonymous class likewise takes it;
    - in a class, `/** p */ public $a; public $b;` gives `$b` none.
  - That is what an annotation reader sees when the code runs: Doctrine's, PHPUnit's `@dataProvider` before attributes, a framework's routes.
  - Today a declaration takes the doc right before its first token (`parser.rs:608-611`), so `j` gets none.
  - From the `docs` array, the parser gives each declaration the doc by PHP's rule. A statement keeps the doc right before it, for an inline `@var`.
  - A new check reports a doc comment that PHP gives to a declaration it does not precede, where the declaration has no doc of its own: the declaration gets that doc's tags at run time.
- **Reading by the configured versions.**
  - Some source lexes differently across the versions a project may run on:
    - `#[` starts a comment before 8.0, so `#[Attr] function f() {}` on one line is no function at all there;
    - `fn` is an identifier before 7.4, `match` before 8.0, and `enum` and `readonly` before 8.1;
    - `?->` is two tokens before 8.0;
    - `1_000` lexes differently before 7.4, and `0o17` before 8.1.
  - The lexer reads the configured versions (`Registry::php_versions`). Where they lex a construct differently, a check reports it, with the versions that read it otherwise. The file is read as the newest configured version reads it.
- **Unchanged.** Ignore marks and file facts are recorded in the lexing loop, as now.
- **Readers of tokens.**
  - Comments are recorded only when asked (`keep_comments` today), for the code frame (`output/codeframe.rs:49-63`) and O1.
  - The stub rules, which item 9 moves into the parse, lex an attribute's type string into a scratch stream (`stub_attributes.rs:121-133` today).
  - Completion lexes the editor's text into a stream of its own.
  - `Parser::new(TokenStream)` replaces the test constructors.
- **What goes:** `Token`, `Tok`, `Supply`, `next_raw`/`next_tok`, the `VecDeque`, `fill`/`pop`, `raw_keyword`, and `LexEnd`'s second pass.

**Oracle.** O1, O2, and phar stubs among the fixtures. Exact, except the semantic changes listed in §11: the halt offset and its check, the first error in the file, PHP's doc rule for declarations and its check, and the versions' check.

**Saving.** About two array writes per token in the lexer and an index per read in the parser, ≈20-30 Ir per token, in place of ≈1.95 G: ≈1.7 G.

**Allocations: ≈0.23 M (M10).**
- **Doc comments go into the `docs` array.** Today the token supply pushes each one onto a vector and boxes each group (`next_tok`: `lexer.rs:597` 54,107, `:602` 52,330, `:607` 313).
- **The parser's start-up for each file:** `with_supply` 44,744 (a goto-scope box at `parser.rs:193` and buffers at `:206`) and `from_lexer` 16,419 (`parser.rs:185`).
- **The lexer's name table,** which grows anew for each file: 64,311 rehashes (`lexer.rs:443`).
- **What becomes per-worker scratch, cleared between files:** the name table and the parser's goto scopes, beside the token buffers.
- **Not this item's:**
  - the names the lexer makes (`lexer.rs:434-435`, 270,240), which item 16 makes one allocation each;
  - the line index's `Arc` in `Lexer::build` (`lexer.rs:368`, 16,419), which goes with item 11's line starts.
  
  M10 charges both to the lexer's functions, and gives them to the items whose designs remove them.

### 11. Line starts for every file, columns where read, offsets in the tree (R4)

**Now.** The lexer builds a whole line index for every file:
- a `memchr` call per line, 1.68 M of them;
- column tables for 1,876 non-ASCII files (`Utf8Chunks` 83.4 M);
- 270.4 M in all.

Context files need line starts, for duplicate-declaration messages (`duplicate_classes.rs:61`, `duplicate_functions.rs:78`), anonymous-class names and the server's navigation. Columns are read for analysed files, and for the lexer warnings and parse failures of any file (`session.rs:674`, `:806-827`). The lexer reads the index for ignore marks (`lexer.rs:530`, `:560`), and the parser reads it for member removal spans and statement doc lines.

**Lines are not PHP's.**
- Only `\n` starts a line today (`span.rs:126-129`, `memchr_iter(b'\n', ..)`).
- PHP also ends a line at a lone `\r`, and counts `\r\n` as one line break. In `php` 8.5, a file of `<?php\r$a = __LINE__;\r$b = __LINE__;\r\n$c = __LINE__;\n$d = __LINE__; // x\r$e = __LINE__;` gives 2, 3, 4, 5 and 6. A parse error after two lone `\r`s is reported on line 3.
- The language server protocol counts `\r`, `\n` and `\r\n` as line ends too, so today's lines are wrong for PHP's error lines and for the editor alike.
- **A `//` or `#` comment ends at a lone `\r` too:** in the probe, `$e` runs. The lexer ends one only at `\n` or `?>` (`lexer.rs:941-946`), so in such a file the code after a comment is read as comment.
- None of the 65,131 PHP files in Piwigo and its `vendor/` has a lone `\r`. Classic Mac line endings still occur in older code.

**Design.**
1. **Two types, so the compiler enforces who may ask for a column.**
   - `LineStarts` (`line`, `line_start`) for every file, built by one pass that turns each block's line-break bitmask into offsets. The same pass's high-bit mask says whether the file is ASCII.
     - **Lines end where PHP's do:** at `\n`, and at a `\r` whose next byte (carried across blocks) is not `\n`. `\r\n` is one break.
     - The offsets are gathered in per-worker scratch and copied out at their exact size, so `LineStarts` is one allocation.
   - `LineIndex` (`LineStarts` plus the column tables, `line_col`, `offset(LineCol)`) for analysed files and open documents, built on the worker.
   - **A lexer warning or a parse failure records its line and column on the worker,** while the source is at hand, by scanning its own line. So a context file keeps only `LineStarts`, and the failure path builds no second index (`session.rs:674` builds one today).
   - A lexer for a fragment of PHP (completion, the stub rules' type strings) builds no index. Today each builds `LineIndex::default()` (`lexer.rs:367-371`).
   - **The lexer's line comments end at a lone `\r`,** as PHP's do.
   
   `FileState` holds `enum Lines { Starts(LineStarts), Full(LineIndex) }`.
2. **Blocks, through `wide`.** The release builds target baseline x86-64 (`.cargo/config.toml` sets no `target-cpu`) and aarch64.
   - The pass reads 16-byte blocks as `wide::u8x16` (`u8x16::new`, `cmp_eq`, `move_mask`). These compile to SSE2 on x86-64 and to NEON on aarch64, with a scalar form elsewhere (`wide-0.7.33/src/i8x16_.rs`, `move_mask`), and need no `unsafe` of ours. `wide` 0.7.33 is already in the lockfile, through `imbl`.
   - **Intrinsics written by hand would need `unsafe`** for their loads, which the workspace denies.
   - **No AVX2 path.** One behind run-time detection would also need `unsafe` to call, and `pulp`, the safe dispatching crate, documents no bitmask operation. At about half an instruction per byte over the under 70 MB of source a run reads (`read_and_parse_file` allocates 66.5 MB, reallocations included), the SSE2 pass costs ≈35 M. AVX2 would save about half of that, under 0.1% of the run.
3. **The language server indexes context files on demand.**
   - An open document's index is built from its overlay text, as item 3's `ast_of` reads the overlay first.
   - A file that is not open is read and indexed only when the hash of the bytes the project loaded, which item 1 keeps in `FileState`, still matches the file on disk. When the hash differs, the file is reloaded first.
4. **The per-file cache stores `LineStarts`** for a context entry, where it stores an `Arc<LineIndex>` today (`file_cache.rs:94`), with a format bump. Item 17's context base stores line starts too.
5. **Nothing in the front end reads lines.**
   - Ignore marks record offsets: the mark's start, the last and the next code token's start. `ignore_comments` resolves lines through the analysed file's index.
   - A class member records its start, the end of the token before it and the start of the token after it, and `removal_span` is computed from those when a fix asks.
   - A statement's leading doc records its offset.
   - Anonymous-class names take their line from `LineStarts` on the worker.

**Oracle.** O1, O2 (every position, fix span, ignore result, anonymous-class name), the line-index tests over multi-byte and astral lines and over `\r`, `\n` and `\r\n` breaks, the LSP tests. Exact, except the semantic change in §11: lines end at a lone `\r`, and so does a line comment.

**Allocations.** `LineIndex::new` makes 46,422 allocations (20.7 MB): the line starts' capacity guess, its growths and its shrink (`span.rs:128`), then `surplus` (`:151`) and `astral` (`:147`). `LineStarts` makes one, and context files build no column tables. The lexer also wraps each index in an `Arc` (`lexer.rs:368`, 16,419); the file's state holds its `Lines` itself, and the checker borrows them. Ignore marks keep their text as a `String` today (`lexer.rs:535`), and ignore comments keep their identifiers that way too (`ignore_comments.rs:268`, `:334`). Both become offsets and slices, read through the analysed file's source.

### 12. Lexer details (R12)

1. **The identifier lookup.** `name_and_bits` costs 324.2 M self over 1.98 M calls, ≈164 Ir each:
   - ≈40 Ir of call overhead;
   - ≈25 Ir of hashing, already word at a time through `rustc-hash`;
   - ≈90 Ir of probing and comparing;
   - the watch-list scan on each name new to the file;
   - the table's growth (64,311 rehashes).
   
   The lookup is inlined into the identifier arm, and the table is item 10's scratch. A probe compares the length first.
   
   **The watch bits are kept with the name** in the worker's cross-file name cache, computed once per distinct spelling per process. Today `WatchList::bits` scans every spelling for each name new to the file (`lexer.rs:238-250`). A scan of a dozen spellings once per distinct name costs nothing measurable, so no table is generated: a perfect hash built at compile time from four modules' lists buys nothing more, and `phf`'s SipHash would cost more (§13). The stub-attribute spellings and their `MENTIONS` bit go with item 9, which applies the stub rules while it parses a stub.
2. **Keywords by word.** The slot's spelling and the identifier (at most 16 bytes; the longest keyword, `__halt_compiler`, is 15) are loaded as `u64` pairs. An exact comparison answers "is it the keyword, in lower case", the common case. Only when it fails, the identifier is folded by OR with the slot's case mask, which leaves `_` and digits alone, and compared again: "is it the keyword in another case". `lex_word`'s separate case comparison (`lexer.rs:867`) goes. Today: 127.2 M, 73 Ir per call.
3. **Blank runs.** Most runs are one space, where a 16-byte block costs more than a byte test. So the byte after the first blank is tested on its own, and only a longer run, such as indentation, goes to item 11's `wide::u8x16` mask: four `cmp_eq`s against `[ \t\r\n]`, an OR, `move_mask`, `trailing_ones` (`class_run` 100 M, which tests eight bytes at a time through a table, `lexer.rs:1427-1442`). The blank class is PHP's: `\f` and `\v` are `T_BAD_CHARACTER`.
4. **Operators without a call.** `lex_operator` costs 45 Ir per operator token over 3.7 M tokens, a third of it its own prologue and epilogue (`lexer.rs:1145`, `:1338`).
   - A byte that is a whole operator nothing can extend (`)`, `;`, `,`, `{`, `}`, `[`, `]`, `~`, `@`) takes its kind from the byte-class table: `ByteClass::Single(TokenKind)`.
   - **`(` can begin a cast, which PHP lexes as one token.**
     - `( int )`, `(\tbinary)`, `(real)` and `(void)` (8.5) are `T_*_CAST` tokens: `(`, spaces and tabs only, a cast name in any case, spaces and tabs only, `)`. With a newline or a comment inside, `(\nint)$y` is not a cast, and PHP reports "syntax error, unexpected variable".
     - Today the parser finds a cast by looking ahead over three tokens (`parser/expr.rs:522-535`), so it accepts a newline or a comment inside.
     - The lexer makes cast tokens as PHP does, and `(` is a single-byte token only where no cast follows. The parser's lookahead goes.
     - The checks of removed and non-canonical casts stay (`coercion.rs:49`, `:69`, `:115`). PHP 8.5 refuses `(real)` with a `ParseError` and `(unset)` with a fatal error, and deprecates `(double)`, `(integer)`, `(boolean)` and `(binary)`.
   - A byte that can begin a longer operator goes to a matcher that loads four bytes as one `u32` and compares against that byte's candidates, longest first, from a table generated with the token list.
5. **`RefKey::Constant` keeps namespace and name apart**, so hashing and equality never search the text (`references.rs:110-132`).
6. **`UseMap::get` stops re-validating UTF-8** (20.5 M).
7. **Native type names are matched without a lowercase copy.** Today `NativeType::from_name` lowercases every type name to match it (`ast.rs:1068`, 135,859 allocations). It matches case-insensitively instead, by the word comparison of step 2.
8. **A qualified name is one token, as PHP 8 lexes it** (`T_NAME_QUALIFIED`, `T_NAME_FULLY_QUALIFIED`, `T_NAME_RELATIVE` for `namespace\X`), interned from its source slice.
   - Today `parse_qualified_name` builds the name in a `String`, segment by segment, and then interns it (`tokens.rs:283-294`): 321,919 allocations.
   - **The spaced form is a syntax error in PHP 8:** `new \A \ B` is "syntax error, unexpected token \"\\\"". It is accepted only where the configured versions include 7.x, and reported by item 10's versions check.
   - Item 15's round trip for qualified names is this.
9. **Numbers with `_` separators are parsed skipping them.** Today the lexer makes a copy without the underscores (`lexer.rs:1022`).
10. **`strip_leading_backslash` returns a slice,** not a new `String` (`name_resolution.rs:122`: 29,664).
11. **A doc comment is what PHP calls one:** `/**` followed by a space, tab, `\r` or `\n`. `/**x*/` and `/***/` are plain comments in PHP (`token_get_all`), so `/**@var int $x*/` gives no types and reflection sees no doc. Today any `/**` not followed by `/` is a doc comment (`lexer.rs:990`). Item 10's doc rule depends on this.
12. **Line breaks after `?>` and in heredocs are PHP's,** item 11's rule applied to two more places.
    - `?>` takes one following line break: `\r\n`, `\n` or a lone `\r` (`?>\rX` gives `T_CLOSE_TAG("?>\r")`). Today a lone `\r` is left in the inline HTML (`lexer.rs:803-811`), which PHP does not output.
    - A heredoc's lines end at `\n`, `\r\n` or a lone `\r`, and PHP opens and closes `<<<EOT\rhello\rEOT;` across them. Today the scanner splits body lines only at `\n` (`lexer/strings.rs:310-336`), so it misses the closing label and reads the rest of the file as the heredoc's body.
13. **Heredoc indentation errors are reported, as PHP refuses to compile them.** A body line indented less than the closing marker is "Invalid body indentation level (expecting an indentation level of at least 2)", and tabs and spaces mixed in the indentation are "Invalid indentation - tabs and spaces cannot be mixed". `strip_heredoc_indent` (`strings.rs:784`) strips whatever it finds and reports neither.
14. **Open tags are PHP's.**
    - `<?php` opens PHP only before whitespace (a lone `\r` included) or the end of the file: `<?phpx echo 1;` is inline HTML. Today any case-insensitive `<?php` prefix opens it (`lexer.rs:887-897`).
    - **`<?` is read by `short_open_tag`,** from the run-time configuration item 6 reads. PHP's default is on (`php -n`), and `php.ini-development` turns it off, so the setting decides whether `<? … ?>` is code. Today `<?` is never an open tag. When the setting is unknown, each short tag is reported as code whose meaning depends on it, and is analysed as code.

**Oracle.** O1, and lexer tests over each of PHP's readings above, checked against `token_get_all`. Exact for steps 1-10 and the rest of step 4. Semantic for the casts, the spaced names, and steps 11-14, listed in §11. **Saving.** ≈0.35 G. **Allocations:** ≈0.49 M (M10): `parse_qualified_name` 321,919, `NativeType::from_name` 135,859, `strip_leading_backslash` 29,664, and the digit separators 582.

### 13. A file's products: the declaration list and one walk (R7; P12 steps 2-4)

**Now.**
- **Three more walks per file after the parse:**
  - the declarations walk (≈140 M around its builders), with eight other callers walking again;
  - the summary walk (`BodySummary::of` 344.0 M), with a second walk for `InitFlow::of` (218.5 M);
  - a third for `GlobalFacts::of` (218.5 M).
- **Import tables rebuilt** 160,194 times (`add_uses`).
- **The old reference collector** (`deps::Collector`, `deps.rs:114-381`, with its raw doc reader) still runs in release builds:
  - for any tree a pass changed after the parse (`file_references`, `project.rs:2836`);
  - for the stub base (`stubs.rs:1385`).
- **The incremental path** walks again (`declared_names`, `declared_constants`, `project.rs:3201-3225`).

P12 step 3 was set aside because "it cannot hold references into a tree still being built". With item 9's layout it can: a node's id is fixed when it is built.

**Design.**
1. **The parser lists the declarations** as it builds them: class-likes, functions, constants and `define()` calls. Each entry holds:
   - the name and kind;
   - the guard (the enclosing conditions and the earlier top-level early returns, from the parser's statement stack);
   - the `ImportScope`;
   - the node's id.
   
   Declaration building iterates it, and so do the duplicate sites, the incremental path and the opt-in rules over analysed files. They all run while the tree exists. What outlives the tree is copied out on the worker, as item 3 says. The declarations walk, its facts-gated expression walks and the separate declared-classes collection go.
2. **`ImportScope`s are published once**, at each `namespace` and `use`. Readers start from them.
3. **One walk for the rest**, with every builder at each node:
   - `BodySummary` (purity, property writes, assigned properties);
   - `InitFlow`, built from the same statement entries and exits, since it follows the statement structure and evaluation order the walk visits;
   - `FileSites` (`GlobalFacts`).
   
   Statement docblocks' names stay where `2dc1aa9b` put them: the parser records each doc site with its context.
4. **The collector goes.** The stub-attribute pass records the references of the types it adds, as it resolves them. The stub build reads the parser's record.
5. **A file's bodies are one sorted slice** of `(span, summary, flow)`. Today they are a hash map holding two `Arc`s per body (`BodySummary` and `InitFlow`, 57,506 bodies, M8). A pass finds a body by binary search on its span.
   - A body's lists are ranges into vectors of the file: one each for calls, purity calls, reassigned names, assigned properties, events and flows.
   - So a file's summaries are a few allocations, not several per body, as item 4 stores declarations.
6. **Builders gather in scratch and emit exact-size slices.**
   - Today the summary and flow builders grow vectors per body and per node:
     - `events_of` 165,258 and `Summarizer::call` 173,767;
     - `FileBodies::of_file` 135,912 and `push_events` 104,304;
     - `flow_of_stmt` 76,492, `purity_call` 42,080 and `Summarizer::finish` 37,003.
   - They gather in per-walk scratch stacks, reused across bodies and files, and move each body's lists out once, at their exact size.
7. **References are recorded into one vector per file, sorted once** (item 5's sorted slices). Today they go into hash sets grown insert by insert: `Recorder::add` 89,648, `Recorder::doc` 51,886, `file_references` 122,290.
   - A doc site holds the interned `ImportScope` of step 2. Today every doc site clones the whole name context into an `Arc` of its own (`references.rs:279`).
   - `recorded_references` reads the sites in their recorded order. Today it collects and sorts them, and the frames, per file (`deps.rs:55`).
   - A declaration's name is its `Name`. Today `Declaration::name` formats it (`declarations.rs:45`: 24,814).

**Oracle.** O1 (declaration lists, guards, contexts, summaries, flows, references), `registry-dump`, O2. Exact.

**Allocations: ≈1.31 M (M10).** This includes the declarations walk's lists, which step 1's list replaces: `if_chain` 52,762, the collector 52,520, `Declaration::name` 30,159, duplicate sites 49,224 and `built_declaration` 28,516.

### 14. PHPDoc finished: P7 and P11 as designed (§3.2)

1. **P7's lowering replaces the lazy round trip.** `DocType`'s type comes from `phpdoc::lower(&TypeNode)`, eagerly in `docblock::parse`, not from `parse_written_type(to_source_string())` on first use (`doc_type.rs:57`, `:90`; 118.3 + 83.3 M).
   - The text grammar's other callers move to `phpdoc::parse_type`, and the text grammar is deleted.
   - The statement `@var` memo goes in: `resolve_type_node` still resolves 34,821 tags per run.
2. **Resolution memoized as §11.7 designed.** The memo is per file, keyed by the type node and an interned resolution context: item 13's `ImportScope`, plus the template names, the type aliases and the class `self` names. It holds the whole resolved type, template marking and alias substitution included. Today's memo (`b081be75`) is sound but covers only the namespace-and-imports step, and `mark_template_params` and `substitute_type_aliases` still run on every use (`type_resolution.rs:74`, `class_info.rs:1129`, `:1220`).
3. **P11 step 1:** `parse_tag -> Arc<TagEffect>` and `apply(&mut DocBlock, &TagEffect)`. There is no scratch block and no `merge_from` (56.2 M). Step 3's `Arc` fields follow.
4. **The tag memo keyed in place,** by the borrowed pair `(tag, rest)` in a `hashbrown::HashTable`, not a `format!` (`docblock.rs:497`, 54.7 M).
5. **Tag tokens borrow.** The tag parser reads a token slice by index. A `PhpDocToken`'s text is a `&'t [u8]` into the tag (303 K string clones today), and `TypeNode`'s identifiers are `Name`s (item 15).
   - A doc is lexed from its bytes, as the PHP lexer reads source (item 9). An identifier takes any byte from `0x80` up, as PHP's does, and keeps its bytes as a `Name`, so a doc naming `\xA9` names the class the code declares.
   - A description is text only for display.
6. **One product per doc text, one per tag line (M6).**
   - **Today one doc text feeds eight memos,** each keyed by its own copy of the text:
     - the grammar node (`grammar.rs:29`);
     - the block (`docblock.rs:295`);
     - one `DocBlock` per tag (`docblock.rs:495`: 26,393 × 840 B);
     - tag values (`grammar.rs:58`);
     - type prefixes (`grammar.rs:73`);
     - the `@var` winners (`phpdoc_unresolvable.rs:73`);
     - `DocNames` (`deps.rs:145`);
     - written types (`types/src/parse.rs:29`).
   - **After this step there are two memos.**
     - **The docblock memo** holds, per text, the grammar node, the block, its referenced names as `Name`s and its `@var` winners. The names are what the 25 G report's P9 step 3 designed: `DocBlock::referenced_names() -> &[Name]`, computed with the block, which `recorded_references` reads. So `DocNames` and its memo go.
     - **The tag memo** holds `TagEffect`s (step 3), which absorbs the tag-value and prefix memos.
     - The written-type memo goes with the text grammar (step 1).
7. **A docblock's nodes are one vector.**
   - A `TypeNode`'s children are indices into its docblock's node vector, as the per-file arenas of rust-analyzer and Zig's AST index their nodes. Today every `Nullable`, `ArrayOf`, `OffsetAccess` and `VarianceArg` boxes its child, and every union, intersection and generic has a vector of its own (`grammar/ast.rs:114-144`).
   - The grammar node, the tag effects and the block's per-tag maps live in vectors of the docblock too.
   - So a distinct docblock is a handful of allocations, not one per node.
8. **Keys by fingerprint, bounds by weight.**
   - **A memo is keyed by a 128-bit fingerprint of its text,** as rustc keys incremental results by `Fingerprint`, so no text is copied into a key. Today's key copies come to about 15 MB.
     - The fingerprint is SipHash-1-3 with 128-bit output (`siphasher` 1.0.4, `sip128::SipHasher13::new_with_keys`, already in the lockfile through `phf`), keyed by a key drawn at random for each process.
     - So no input can be crafted to collide with another without the key, and an accidental collision has a 2^-128 chance per pair. A collision would hand one docblock's types to another.
     - rustc's key is fixed, because it stores its fingerprints. These memos live in one process.
   - **It is bounded by a byte budget** that counts its values as well as its keys.
   - **It evicts by `quick_cache`'s weighted cache** (0.7, `sync::Cache::with_weighter`), a Clock-PRO variant that resists scans, with 21 B of overhead per entry. That replaces a hand-written CLOCK. Today a full shard is emptied whole (`memo.rs:88`), and the tag memo's bound is 131 K entries.

**Oracle.** `registry-dump` (every docblock's resolved types), the PHPDoc resolution record, O2. Exact. **Saving.** ≈0.35 G.

**Allocations: ≈2.66 M (M10).** Steps 1-7 remove them.
- **Three sites need a word more:**
  - **A docblock's tag lines** are copied into `String`s (`docblock.rs:337`, `:339`: ≈220 K). They become slices of the doc text.
  - **The tag lexer and parser copy text.** The lexer makes a `String` per identifier (`grammar/lexer.rs:127`, 209,225). The parser clones tokens as it advances (`grammar/parser.rs:98`, 210,623), and lowercases identifiers and tag values to match them (≈145 K). Tokens become slices (step 5), matched case-insensitively without copies.
  - **`apply_tag` copies every description to test a prefix** (`description.replace(' ', "")`, `docblock.rs:915`, 47,902). The test skips spaces as it reads.
- **The rest:**
  - memo keys built by `format!` (`docblock.rs:497`, `grammar.rs:61-66`: ≈390 K);
  - `to_source_string` (≈245 K);
  - `merge_from` (44,557);
  - the `DocNames` memo (75,793);
  - the checker re-parsing statement docs and resolving their `@var` tags (≈370 K).

**Memory.** About 50 MB of the memos' 72.5 MB at exit (M6):
- the per-tag blocks, 22.2 MB, become effects of a few words each;
- `DocNames`, 14.6 MB;
- the key copies, ≈15 MB;
- the duplicate memos.

### 15. Names held as names (R6; C3 steps 6-7)

**Now.**
- `fold_hash` and `is_folded_eq` cost 701 M: lookups by `&str` whose callers hold text where a `Name` existed.
- `Interner::name` runs 1.42 M times on text whose `Name` existed a moment before.
- 1.33 M `String`s are cloned to carry identifiers.

**Design.**
- **C3 step 7.** The checker's method memo (`checker.rs:271`, keyed by `String`) and `class_memo` are deleted.
- **C3 step 6, all of it.** Every `String`-keyed map, and every value holding a PHP identifier, gets the identity PHP gives it: `FoldedName`, `Name`, `ConstName`, or `ArrayKey` for array keys. In this profile's order of cost:
  - the registry's `&str` entry points (`ClassOwners` and `ClassReflection` lookups, `lookup_class`, `lookup_method_with_owner`, `resolve_function`, `declared_method_name`) and their callers;
  - `TemplateBindings` (`FxHashMap<String, Type>`, `checker.rs:224`), and `substitute_class_templates`;
  - the scope's guarded sets and container fields (`checker.rs:44-60`, `:172`);
  - `closure_scope`'s frontier sets (`project.rs:1304`);
  - the identifiers copied as text:
    - `PhpDocToken` and `TypeNode` (with item 14);
    - `statement_assigned_variable_names`, which turns each assigned variable's `Name` into a `String` for every statement (`stmt.rs:1656`);
    - `init_flow`'s property names (`Vec<(bool, String)>`);
    - `global_vars`' keys, which become `ArrayKey`s (`global_vars.rs:52`, `:109`, `:177`, `:201`);
  - the round trips through the interner:
    - the parser's `ident_from` re-interns a `Name` the lexer gave it (`parser/tokens.rs:194`);
    - qualified names and `use` paths go through the global interner where P12 designed the lexer's tables (`parse_qualified_name` 94 K, `use_path` 63 K);
    - `resolve_name_expr` builds a `String` and interns it (96 K);
    - `bind_self` re-interns the class (65 K);
    - the key table resolves property names through text (37 K);
  - then `narrowing`, `calls`, member-usage tokens, `project`, `types` and `phpdoc` by module.
- **The end state** deletes `TableKey for str` and `FoldKey for str` from the registry. The compiler then lists every caller still holding text. Text that enters from outside (configuration, a class named in a string literal) is interned once where it enters.
- **A name is its bytes.** PHP's identifiers are bytes: the lexer takes any byte from `0x80` up, and `class \xA9` and `class \xAA` are two classes (item 9).
  - `Name` holds `[u8]` and compares with PHP's ASCII-only case folding, as it does today (`name.rs:41`).
  - It gives text only for display, where a byte outside valid UTF-8 prints as `\xA9`, in messages and in JSON alike.
  - A string value used as a name (`class_exists`, `new $class`, a callable string) becomes a `Name` from its `PhpString` bytes, so it finds a class whatever its bytes.
  - `Deref<Target = str>` goes, and the compiler lists every reader of a name as text.
- **A short name is a slice** of the name, at the offset of its last segment, which item 16 stores in the name's header (the 50.9 M of `rsplit` in `check_expr_impl`).

The purity and property-write keys went by item 4, as ids.

**Oracle.** O2, `registry-dump`, `--dump-types`, `--dump-expr-types`. Exact, except the container-field facts' keys becoming canonical array keys (C4 step 5: `'1'` and `1` one key), listed in §11. Static-property keys by resolved class are already built (`2d1e6c45`). **Saving.** ≈0.6 G.

**Two sites need a word more:**
- **Name resolution builds text to intern it.** `qualify` and `resolve_as` (`name_resolution.rs:382`, `:423-425`; ≈315 K) format the joined name into a `String`. Instead, the interner hashes the parts as one text and compares a candidate part by part, so an existing name is found without building its text. Only a new name allocates.
- **A call's callee name is lowercased five times per call** (`expr.rs:499`, `:821`, `:871`, `:889`, `:918`: 47,957 each). Its resolved `Name` already holds its folded form (`Name::lower`).

**Allocations: ≈2.76 M (M10).**
- **Lowercase copies:**
  - `check_expr_impl` 338,625 (with the five above);
  - `forwards_static_binding` 97,960 and `type_check_call_outcome` 86,141;
  - `identifier_unresolvable` 71,261;
  - `check_castable_array_arguments` and `check_impossible_haystack_values`, 48 K each;
  - and others.
- **Names turned into text:**
  - `Name::to_string` 507,745;
  - `statement_assigned_variable_names` 214,698;
  - `resolve_as` 178,381 and `qualify` 135,623;
  - `canonical_class_name` 114,838 and `resolve_static_class_name` 113,087;
  - `declared_method_name` 83,737, `lookup_method_with_owner` 76,924 and `find_method_receiver` 64,351;
  - the canonical method names copied in `static_method_through_instance` and `check_static_call_context` (≈84 K);
  - `literal_string_key` 41,632, and the PHPStan-API walker's class names (44,134).
- **Template bindings by `String`:** `class_template_bindings_rec` 301,021, `substitute_class_templates` 80,734 and `class_template_params_for` 34,781.
- **The tail's text copies (M11, F5: 102,938 in 38 functions).** For example:
  - `check_internal_instantiation` 11,683, `instanceof_outcome` 8,472 and `static_member_of_class` 7,022;
  - the DQL tokenizer's `String` tokens (`dql.rs:172`), and the current class as a `String` in `Promotions` and `check_program`;
  - fixed names built at each call (`"Doctrine\ORM\Query".to_string()`, `member_resolution.rs:1166`), which are interned once.

### 16. Interned values: one allocation each, tables without per-entry allocations (M7)

**Now.**
- **A name takes three allocations to make.** `Name` is an `Arc<NameData>`, and `NameData` holds its text and, when the text is not lowercase, its lowercase form, each in a box of its own (`name.rs:22-49`). The lowercase copy is made for every name and dropped when it equals the text (`name.rs:41-42`). The run makes 186,348 names in 559,250 allocations. At exit there are 135,777 `NameData`, 118,237 texts and 79,600 lowercase copies.
- **The type interner's index costs nearly as much as the types.** It is `FxHashMap<u64, Vec<Weak<Node>>>` (`intern.rs:21-25`), with a `Vec` for each distinct hash: 120,644 allocations and 3.9 MB, plus 8.7 MB of table, for 119,757 nodes of 112 B (13.4 MB). That is ≈105 B of index per node.
- **A shape's string key allocates each time it is made.** `ShapeKey::from_bytes` builds a `PhpString` (an `Arc<[u8]>`) for every key the checker forms: 128,140 allocations, and 29,131 held at exit.
- **The type memos are never cleared.** `remember` keeps 5.4 MB and the shape memo 1.5 MB at exit.

**Design.**
- **A name is one allocation.**
  - A header holds the text hash, the folded hash, the length, the offset of the last segment (item 15's short names), and where the lowercase form starts, or that the text is already lowercase.
  - The text follows the header in the same block, then the lowercase form when it differs.
  - It is a `triomphe::ThinArc<Header, u8>` (0.1.16), built by `from_header_and_slice`: one pointer wide, header and bytes in one block, with no `unsafe` of ours. A thin dynamically sized value written by hand would need it. Rowan holds its green nodes the same way.
  - Equality stays identity, through `ThinArc::ptr`.
- **The tables hold strong entries, as rust-analyzer's interner does** (`ra_ap_intern`'s `Interned`).
  - `triomphe` has no weak references, so the tables cannot hold `Weak`s as today's do.
  - A handle whose drop sees a count of two (itself and the table) takes the table's shard lock, checks the count again, and removes the entry. A lookup clones its entry under the same lock, so it cannot revive an entry being removed.
  - An entry is hashed by the hash in its header, so it is one pointer, with no stored hash. The type interner loses its per-hash `Vec`s.
  - The type interner's nodes become `triomphe::Arc`s on the same rule, which also removes the weak count from every node.
- **Shape string keys are interned** in a table of byte strings, as names are. A key found in the table costs no allocation.
- **The type memos are cleared with the relation memo's generation** (item 7), so a long session does not keep the answers of hierarchies it no longer has.

**Oracle.** O2, `--dump-types`, `registry-dump`; the interner's tests over freed entries and drops racing lookups on other threads. Exact.

**What stays.** One allocation per distinct live value: that is what interning with freeing costs. rust-analyzer's `intern::Interned` pays the same. A per-session arena that never frees, as rustc's interners use, would grow without end in a server, so the plan keeps freeing.

**Memory.** ≈15 MB at exit.

**Allocations: ≈0.74 M (M10):**
- the type interner's per-hash vectors (120,644);
- shape keys that already exist (most of 182,591);
- the names the lexer makes (`name_and_bits`, `lexer.rs:434-435`: 270,240).

Two of every name's three allocations go wherever it is made: ≈373 K of the run's 559,250. The rest of those are in the rows of the items whose code asks for the names (`RefKey::class_text` 101,896, `parse_qualified_name` 58,737, and others).


### 17. Warm runs: entries checked by `stat`, a resolved context base

**Now.** In the cached run's samples, the parse job's 1,017 samples are:

| part | samples |
|---|---|
| parsing the analysed files and their products | 542 |
| decoding the context files' entries | 226 |
| reading files | ≈200 |

Each cached file is read whole only to hash it for its entry's key (`session.rs:631-648`), then the entry is read and decoded whole. Then the finishing passes resolve every vendor declaration again: the window item 4 shortens but cannot remove.

**Design.**
1. **Entries checked by `stat`.**
   - A per-project index, written at the end of a run, maps each path to `(size, mtime_ns, inode, device, content hash)`.
   - When a context file's four stat fields match, and its recorded mtime is older than the index's own write time (git's rule for racily clean entries), the recorded hash is taken without reading the file. Otherwise the file is read and hashed.
   - Analysed files are read anyway.
   - **Every content hash is BLAKE3** (`blake3` 1.8.7): this index's, the base's key, item 1's hash in `FileState` and item 11's check, and the per-file cache's keys, which today are XXH3-128 of a file's bytes and path (`file_cache.rs:164-176`).
     - A cache must never answer for other content. XXH3 makes no claim against inputs crafted to collide. A cache directory shared between CI runs of untrusted pull requests is where a crafted file could inherit a clean file's results.
     - BLAKE3 is cryptographic. It picks its SIMD at run time behind a safe API, NEON included, and builds its assembly with `cc`, which the release builds already run for `pcre2-sys`.
     - An estimated few tens of ms of CPU over the under 70 MB of source a cold run reads, spread over the workers. A warm run hashes only what `stat` cannot clear.
2. **A resolved context base**, as the stub base is resolved.
   - **What it holds:**
     - the registry after the finishing passes over the context declarations alone;
     - the reverse indexes restricted to context files;
     - each context file's state less its tree (references, declared names and constants, duplicate sites, line starts, summaries, global facts, declared classes);
     - the resolved snapshot `changed_since` compares against.
   - **Keyed by** the build, the configuration and PHP version range, and the set of `(path, content hash)` of the context files.
   - **Stored as the stub base is:** item 6's container, records read and decoded lazily, names in one table (item 6), ids by record index (item 4).
   - **A cold run** inserts the context files, runs the finishing passes, and writes the base. It then inserts the analysed files and finishes through the incremental path, over the changed names' closure.
   - **A warm run** loads the base, applies any changed context file through `update_file`, inserts the analysed files and finishes the closure. What the analysed files change in vendor code (a vendor function calling a project function, the global-variable pass) is re-finished by that closure, as an edit is in a server session.
   - **The language server's initial load is the same load (item 1),** so it uses the base too.
   - **The two caches answer to different keys.**
     - A per-file entry holds a file's parse products, which depend on its bytes, path, parse options and the build.
     - The base holds resolution, which depends on every context file and on the configuration.
     
     When the configuration changes, the base is rebuilt from the entries without parsing.

**Oracle.** A warm run's report equals a cold run's, byte for byte, over the corpus, the fixtures and the vendor tree, with a context file edited between the runs and with none; the incremental tests. Exact.

**Saving, on warm runs.**
- The decoding (≈0.45 core-s in the sample).
- The context files' reads (≈0.4 core-s).
- Most of the finishing window.

A cold run pays one more incremental finishing, over the analysed files' closure.

**Memory.** The base is read as the stub cache is: lazy records with `read_at` (item 6, step 5). A warm run therefore holds in its heap only the records it decodes: those of the changed names' closure and those the check reads. Today it holds every context declaration, decoded or built.

### 18. The check phase's remainder

1. **§12.10 of the 25 G report, as designed: ≈1.0-1.3 G.**
   - Re-inference costs ≈0.75 G: `check_args` 263.0 M, `check_expr_impl` 330.4 M, `call_bindings` 71.2 M, `infer_binary` 94.4 M.
   - The separate walks cost ≈0.35 G: `Promotions` 86.3 M, `Members` 76.0 M, `global_constants` 56.8 M, the PHPStan-API walker 103.9 M, and the rest.
   - Then the loop passes after the first.
   
   Within it:
   - step 1 deletes C5's interim `statement_method_call` record (`calls/misc_rules.rs:49-58`), because the statement rules become the first observer family;
   - the PHPStan-API walker becomes its family, registered by `mentions_phpstan` or the ancestry fact;
   - a literal's type is computed once per node per pass (`string_literal_ref` 86.0 M).
2. **C1 step 5's tail.**
   - Effects are carried in the statement result, so `noop::expr_side_effects`' walk (`noop.rs:51`, 30.7 M) goes.
   - So is `has_yield`.
   - `collect_throws` (`try_catch.rs:398`) gives way to the throw tails C1 keeps.
3. **Variables as slots, scopes as radix arrays: ≈0.15-0.25 G.**
   - The parser numbers each function-like body's variables. A variable node holds its `NameId` and its `VarSlot`, and a closure's `use` list and an arrow function's captures map to the enclosing body's slots.
   - A variable's `LocalKey` is its slot: no name lookup (`KeyTable::find_var` 79.1 M).
   - **Variables PHP can reach by a computed name or a shared scope keep the name lookup:** `$$x`, `${expr}`, `extract`, `compact`, `get_defined_vars`, `parse_str` with no result, `$GLOBALS` in the global scope, and `include`/`require`/`eval`, whose code runs in the function's scope. The parser marks a body holding any of these.
   - **The scope's maps index the dense keys.** They become a persistent radix array: 32-way nodes indexed by the key's bits, `Arc`-shared and copied along the path on a write, in place of `imbl::OrdMap` (`get` 72.9 M, `Cursor::next` 34.6 M, `Leaf::insert` 29.8 M).
     - A lookup is two or three indexings, with no comparisons.
     - A node that no other scope shares is written in place (`Arc::get_mut`), so only a shared path is copied. The copies that remain are what persistence costs. C4 keeps it because its diff skips every subtree that is the same pointer in both scopes.
     - The diff C4 relies on walks two arrays and skips every subtree that is the same pointer in both.
     - Iteration stays in key order.
4. **C2 step 2:** conditional subjects bound by parameter position (`ConditionalSubject::Parameter { name, index }`), with a format bump.
5. **§12.11's rest:** the `MemberIndex` and the normalized-operand fast path (`union.rs` 58.7 M, `reduce_array_members` 21.3 M).
6. **C8 step 2's rest: type operations from borrowed parts.**
   - Each operation gathers its members in a per-thread scratch stack and interns from the borrowed slice, for every kind (C8's `intern_ref`). So an operation whose result already exists allocates nothing.
   - Today these build owned member lists first:
     - `intersect` ≈205 K;
     - unions ≈170 K;
     - `concat` ≈155 K;
     - `array_shape` ≈80 K;
     - traversals ≈75 K;
     - `into_repr` ≈55 K;
     - `reconcile_with_native`, `finite_types`, `Shape::push` and `absorb_contained_members`.
   - **A traversal copies on the first change.** Today `map_all` allocates its output before it knows whether anything changes (`traverse.rs:241`: ≈50 K across the passes, among them `bind_self`, `replace_objects`, `map_class_names` and template substitution).
   - **A memo is probed with the borrowed member list,** and builds its key only on a miss. Today `union_many` clones its members to key the memo (`union.rs:79`), and `memoized_many` collects the rest of the addresses (`intern.rs:766`).
   - **Shared values are borrowed, not cloned:**
     - `into_repr` works on a `&Repr`, where today it clones a shared one (`lib.rs:326`: ≈34 K);
     - `CallableSig::own_templates_as_bounds` and `instantiated_against` return the unchanged signature borrowed, where they clone it (`templates.rs:282`, `:298`);
     - `same_members::by_hash` compares without sorting a copy of the members (`lib.rs:555`).
   - **`Display for Type` writes into the formatter.**
     - Today it builds a `Vec<String>` of a union's members and joins it (`display.rs:122`, `:126`, `:141`, `:271`, `:330`; `write_shape`, `:347`), and copies each string literal to escape it (`quote_php_string`, `:397`): ≈60 K.
     - Where a type is displayed for a message, the message itself is deferred (step 10, F1).
7. **A call's argument binding is made once (M9).**
   - Today each rule that needs a call's binding of arguments to parameters builds it again:
     - `resolve_call_args`, from `apply_param_out_types`, `generic_object_method_return_type` and `infer_builtin_call_return_type`: 119,628 + 59,308 + 57,338 vectors;
     - `bind_arguments`: 236,963;
     - `check_parameters`: 116,324, 27 MB.
   - The binding becomes part of the call's record in the pass (step 1's one inference per node). Every rule borrows it.
8. **Per-function state is reused.**
   - `check_function_impl` allocates its tables anew for each function (125,654 allocations), and the expression-type cache grows anew too (`cached`: 19,778 allocations, 27.9 MB).
   - These belong to the checker and are cleared, not freed, between functions.
9. **A generic call's signature is substituted type by type.**
   - Today the call copies the whole signature to substitute it (`FunctionReflection::map_types`, `function_sig.rs:372`, with `clone_vec`'s parameter copies: ≈110 K).
   - Instead, the call asks for the substituted type of the parameter or return type it reads.
10. **The rules' temporaries (M11).** Every site is in Appendix A.
    - **F1. Messages are formatted only when reported.** Today 37 rules format a subject, a message or an identifier for every element they check. Examples:
      - `check_property_phpdoc_types` formats a subject for every property (`phpdoc_type_compat.rs:377`: 21,919);
      - `check_property_declarations` formats another, and turns each native hint into a `String` to test it for `callable` (`property_declarations.rs:133`, `:142`);
      - identifiers are built by `format!` (`probe_checks.rs:159`, `strict_rules.rs:218`);
      - `check_function_impl` formats each function's labels (`function_decl.rs:282`, `:1117`: 46,005);
      - `check_statement_var_tag_names` and `check_statement_var_tag_classes` format a message or a subject for every `@var` variable (`stmt.rs:529`, `var_tag_types.rs:346`: ≈65 K).
      
      A subject becomes a value that displays itself, formatted by the report. An identifier is a `&'static str`, since identifiers are a closed set.
    - **F2. Constant types are built once.** `union_of(vec![Type::int(), Type::float()])` and its kind are rebuilt on every call in 41 functions (`strict_rules.rs:230`, `operators.rs:57`, `:672`, `:685`). Instead they are interned constants, as `types` builds its `Constant` nodes (`intern.rs:43`). A two-member union or intersection takes its two operands with no `Vec`.
    - **F6a. Rules borrow, iterate, or use scratch.**
      - A one-element list is `slice::from_ref` (`type_declarations.rs:182`).
      - A list collected only to be iterated is iterated (`member_resolution.rs:298`, `operators.rs:706`, `offset_assignment.rs:46`, `:50`).
      - A constant list is a `static` slice (`declaration_level_rules`, `internal_rules.rs:56`, rebuilt per call). A list fixed for the file is made once per file (`default_include_dirs`, `includes.rs:380`).
      - A `seen` vector lives in the checker's scratch (`array_ops.rs:70`, `function_like_declarations.rs:266`).
    - **F6c. Throw, exit and loop states copy no scope.**
      - Every throw point clones the whole scope (`record_throw_state`, `expr.rs:348`: 3,920 clones, 16.1 MB), and `check_try` collects scopes (`try_catch.rs:99`, `:124`: 17.2 MB). With step 3's persistent scopes, a copy is a pointer copy.
      - The per-try and per-loop vectors (`control_flow.rs:140-141`, `try_catch.rs:84`) are scratch stacks.
    - **F6d.** The narrowing inserts and the `Rc<Vec<..>>` implied facts (`narrowing.rs:569`, `control_flow.rs:276`, `:314`, `:336`) live in step 3's radix arrays.
    - **F6e.** The separate walks' vectors (`bind_targets`, `strict_rules.rs:951`: 20,655; the keyword-scope and `declare` visitors; `section_ends`; the usage walkers) go with the walks, which become families of step 1's traversal.
    - **F6f. Analyses that are a function of a text are made once per distinct text,** in item 14's fingerprint-keyed, weight-bounded memo, scanning bytes and keeping slices:
      - regex groups (`regex_groups.rs`: the pattern as a `Vec<char>`, `:505`; a `String` per literal char, `:936`);
      - `preg` shapes;
      - DQL (`dql.rs:172`);
      - SQL query-builder rendering (`query_builder.rs:192-205`);
      - format strings (`sprintf_pieces`, `builtin_calls.rs:3244-3250`; `printf_placeholders`).
    - **F4. Third-party parsers.**
      - The SQL extension parses a query's text once per distinct text in the run. Today each of the four functions that read it parses it again (`sql_reflection/resolve.rs:136`, `:176`, `:348`, `:386`). `sqlparser`'s tokenizer accounts for 86.6 K allocations.
      - Regex matches are read through one reused `Captures`, not one allocated per match (`regex` 7.7 K).
    - **One checker per worker.** Today `Checker::new` allocates a default `Arc<LineIndex>` and a key table for each file and then replaces them (`checker.rs:1155`, `:1175`), and `set_current_file` and `checker_for` copy paths. A worker's checker is reset for each file instead.

**Oracle.** O2 with its diff read family by family, `--dump-types` and `--dump-expr-types`. Semantic for §12.10, as the 25 G report says; exact otherwise.

**Memory and allocations.** The check phase makes 3.58 M allocations (M10). What they leave:
- the results;
- the radix arrays' path copies, which replace `imbl`'s 114,979 leaf copies (31 MB);
- new types.

The autoload thread (321.4 M, off the critical path) keeps the one parser, as P10 decided. The references it records go unread, at ≈10 M.

### 19. What a run keeps: each tree until its last reader (M3)

**Now.**
- **A batch run keeps every analysed tree to the end:** 173 MB at exit. Project-wide passes walk the trees after every file is checked:
  - architecture (`architecture.rs:245-247`);
  - banned constructs (`banned.rs:477`);
  - class shapes (`class_shape.rs:103`);
  - Composer hygiene (`composer_hygiene.rs:287`, `:304`, `:519`);
  - mock hygiene (`mock_hygiene.rs:376`);
  - the use-site index (`project.rs:1681`, `:1718`);
  - the trait lines that `finish_diagnostics` reads for unmatched ignores (`project.rs:813`).
  
  Item 18 already moves the PHPStan-API walker (`project.rs:2376`) into the check.
- **After item 1, the server keeps the tree of every analysed file in the workspace,** whether or not it is open.
- **A reader of a dropped tree gets an empty file in release builds.** `FileState::ast` asserts only in debug builds, then returns `&[]` (`project.rs:199-205`).

**Design.**
- **The project-wide passes read per-file facts, not trees.** Each pass's per-file part becomes a family of item 18's traversal, or of item 13's walk when it needs no types. It records its facts with the file's check result, and the pass's whole-program part reads those facts:
  - architecture: the class and function references and the `new` sites;
  - banned constructs: the banned constructs a file uses;
  - class shapes: the declared classes, from the declaration list;
  - Composer hygiene: its facts, references and declared classes;
  - mock hygiene: what its walker records;
  - unmatched ignores: the trait declarations' line ranges, from the declaration list;
  - the use-site index: item 1's per-file product.
- **A batch run frees each analysed tree when its file's check ends.**
  - The check takes the tree out of the file's state and owns it for the length of the job.
  - Files are checked largest first, so the largest trees go first.
  - The private-property judgment (`judge_property_types`) reads the writes recorded during the checks, and parses again only the files outside the batch, through `ast_of`.
- **The server keeps the trees of open documents, plus a cache of recently checked files bounded by bytes.**
  - The cache is `quick_cache`'s weighted cache, weighing each tree by its arenas' bytes. It replaces `AstCache`'s `VecDeque` (`project.rs:175-190`).
  - rust-analyzer applies the same policy to its parse trees, but bounds them by count (`lru.capacity`, 128 files by default), whatever their size.
  - Any other tree is parsed again on demand through `ast_of`, from text read on demand and checked against the hash item 1 keeps.
  - An edit that rechecks a dependent file parses that file again if its tree was evicted.
- **No reader gets an empty file.** `FileState::ast` goes, and every reader asks `ast_of`, which parses again when the tree is gone.

**Oracle.**
- O2's whole-program diagnostics.
- The server session and its edit replay (item 0).
- The `oracle` build, whose assertions every dump must pass.

Exact.

**Memory.**
- **In a batch run,** the analysed trees are freed as the check proceeds rather than kept to the end: 173 MB at exit today, about half that after item 9. The run's peak becomes the start of the check.
- **In the server,** the trees held are the open documents' and the LRU's, not the workspace's.

## 11. Validation, once, after item 19

- `cargo test --workspace` and the fixtures (`--test fixtures`).
- **The dumps of item 0, taken again** with the `gate` build and the `oracle` build. The `oracle` build must finish without an assertion. Every difference from the baseline must be one of the semantic changes the items name:
  - **item 1:**
    - a file reached through a link is loaded once and reported under its resolved path, with `__FILE__` and `__DIR__` resolved;
    - a file both analysed and context is analysed, once;
    - overlapping configured paths are checked once;
    - the server reads non-UTF-8 files as bytes;
    - the server treats stub paths as stubs;
    - the server resolves its configuration as the CLI does;
    - the server publishes the whole-program diagnostics and parse errors;
  - item 2: a tree with a link cycle is listed once, not until `ELOOP`;
  - item 4: the order in which code iterating a class's methods meets them, where anything depends on it;
  - item 6: guards decided by the stubs' availability, on run-time evidence; a stub's parameters and types by the configured versions; and the new polyfill-signature check;
  - item 7: constants on a cycle read as `never` and reported;
  - item 8: a `@property` tag shadowed by a declared property checked against the tag's own names;
  - item 9: identifiers and docs read from a source that is not UTF-8 keep their bytes, so names that differ only in such bytes are distinct; the stub rules apply only to stub sources;
  - item 10: `__COMPILER_HALT_OFFSET__` as its file's offset, with the new check of its use elsewhere; `__halt_compiler` followed by anything but `( ) ;` or `( ) ?>` is a syntax error; a broken file's error is its first one; a declaration's doc is the one PHP gives it, with the new check of a doc given away; and the new check of source the configured versions lex differently;
  - item 11: lines end at a lone `\r` as at `\n`, and so does a `//` or `#` comment;
  - item 12: casts as PHP's single tokens; spaced qualified names a syntax error on 8.x; doc comments only with whitespace after `/**`; `?>` and heredoc lines ending at a lone `\r`; heredoc indentation errors; `<?php` open only before whitespace, and `<?` by `short_open_tag`;
  - item 15: container-field keys canonical as PHP's array keys;
  - item 18: the rule families of §12.10.
  
  Each difference is listed with its file and read.
- **O2 against the baseline's**, in output order (`xtask compare`), saved as `.corpus-runs/<scope>/<hash>.txt`, its diff read line by line.
- **Equalities that need no baseline:**
  - one thread against the default;
  - the stub cache cold against warm;
  - the per-file cache off against written and read;
  - a warm run against a cold one (item 17);
  - the relation memo on against off (item 7);
  - the server's first dumps against the CLI's (item 1);
  - each edit replay against a fresh load of the edited tree.
- **O3:**
  - callgrind in `cl-20g`'s profile;
  - a sampling profile of a cached run, for the serial windows;
  - `hyperfine`, cold and warm, alternating with the starting commit's `gate` binary under `nice`, with the load average noted;
  - the dhat heap profile in §8's build, compared by holder with §8's at the peak and at exit, and its allocation count;
  - the server's heap on the in-process session of item 0;
  - the results in `WORK_STACK.md`.

## 12. Summary

| # | item | saving (this profile) | serial / wall | memory (§8) | kind |
|---|---|---|---|---|---|
| 0 | baseline dumps, debug oracles retired | debug builds and tests | — | — | — |
| 1 | one session: load, configuration, analysis, identity | the server and the tests get every load item | load off the server's message loop | server: the context trees (≈420 MB) and the unopened files' sources | semantic (§11) |
| 2 | parallel walk, path data | ≈0.3 G | ≈0.15 s serial start | the second copy of each path; 0.56 M allocations | exact, except link cycles |
| 3 | context trees dropped on the worker | ≈0.3 G | 0.58 G off the serial path | peak −≤209 MB alone (the end of the parse −≈420 MB); context reads, 66 MB | exact |
| 4 | registry by ids, declarations never changed after build | ≈0.9 G | ≈0.4 s serial | ≈90 MB at exit; ≈1.5 M allocations | exact, except member order |
| 5 | parallel merge, stable ids, posting lists | ≈0.25 G | yes | ≈5 MB; 0.11 M allocations | exact |
| 6 | stub scope, records read on demand with per-record CRCs | ≈0.35 G, then more of `warm` | partly | ≈40 MB of heap; 0.45 M allocations | exact (1, 3, 5), semantic (2, with the stubs' versions) |
| 7 | one relation memo, by content generation | up to ≈0.3 G | — | bounded as today; `KEEP_DECISIONS`' 1.3 MB goes | exact |
| 8 | case references in the checker, declarations built lean | ≈0.1 G | — | both caches' entries; 0.48 M allocations | exact, except one `@property` case |
| 9 | typed vectors, ids, `peeled` | ≈0.8-1.2 G | — | ≥53 MB of the analysed trees; 3.92 M allocations | exact, but semantic for non-UTF-8 names and the stub rules |
| 10 | flat token arrays | ≈1.7 G | — | 0.23 M allocations | exact, but semantic for the halt offset, the first error, declarations' docs and the versions |
| 11 | line starts, columns, offsets, ignore marks | ≈0.2 G | — | 0.07 M allocations | exact, but semantic for a lone `\r` |
| 12 | lexer details, qualified names and casts as one token | ≈0.35 G | — | 0.49 M allocations | exact, but semantic for PHP's tokens (§11) |
| 13 | declaration list, one walk, builders in scratch | ≈0.45 G | — | bodies as one slice per file; 1.31 M allocations | exact |
| 14 | PHPDoc finished, two memos | ≈0.35 G | — | ≈50 MB; 2.66 M allocations | exact |
| 15 | names as names | ≈0.6 G | partly | 2.76 M allocations | exact, except key corrections |
| 16 | interned values in one allocation | ≈0.1 G | — | ≈15 MB; 0.74 M allocations | exact |
| 17 | warm runs | decoding and reads ≈0.85 core-s, most of the finishing window | yes | only decoded records in the heap | exact |
| 18 | check phase remainder, rule temporaries | ≈1.6-2.0 G | — | 3.58 M allocations | semantic (§12.10) |
| 19 | each tree until its last reader | — | — | batch: the analysed trees freed during the check; server: open documents and an LRU | exact |

**Instructions.** The rows overlap little: 9 and 10 share the parser's allocation, and 4 and 15 the purity keys. Together they come to ≈8 G of this run's 20.57 G. Most of the serial time goes with them: the finishing window's 0.41 s, the insert's 0.58 G and the start's 0.15 s.

**Memory.**
- **At exit,** the 657 MB the project holds loses:
  - the analysed trees (173 MB, item 19);
  - the passes' copies and string tables (≈90 MB, item 4);
  - the memos' ≈50 MB (item 14);
  - the stub records' ≈40 MB (item 6);
  - the interners' ≈15 MB (item 16);
  - the per-file tables' ≈7 MB (items 2 and 5).
  
  That leaves ≈280 MB.
- **The peak** moves from the end of the parse (866 MB) to the start of the check. There, those ≈280 MB sit beside the analysed trees, about 85 MB after item 9: **≈365 MB**, plus the check's working set.
- **Allocations.** M10 gives every one of the run's 19.4 M allocations to the item that removes it. What each row keeps, estimated from what its design still allocates:

  | item | what stays | today | after |
  |---|---|---|---|
  | 18, the check | shared persistent-scope paths copied on write; per-text analyses once per text; reported messages | 3.58 M | ≈0.5 M |
  | 4, the registry | a few vectors per file; each stub record a run decodes | 2.02 M | ≈0.1 M |
  | 14, PHPDoc | a handful per distinct docblock and tag line | 2.66 M | ≈0.1 M |
  | 13, a file's products | a few vectors per file | 1.31 M | ≈0.05 M |
  | 16, 10 and 6 | one per new type node and new name; the stub records a run reads | 1.44 M | ≈0.4 M |
  | 9, 15, 2, 8, 12, 5, 3, 11 and the rest | kept trees' vectors, kept sources, per-file vectors | 8.43 M | ≈0.2 M |
  | **total** | | **19.4 M** | **≈1.35 M** |
  
  **What stays, and why:**
  - **Inherent to a design the plan chose:**
    - persistence's path copies (C4 diffs scopes by pointer);
    - interning with freeing (item 16);
    - an allocation per distinct docblock, query text and new name.
  - **Kept results:** diagnostics and declarations.
  - **Allocations inside third-party crates:**
    - `sqlparser`'s tokenizer and parser, which the plan calls once per distinct query text;
    - `regex`, compiled once per pattern;
    - argument and configuration parsing, once per process.

  The bytes allocated fall from 2.67 GB to roughly 0.5-0.7 GB. Most of what remains is the kept trees' vectors, the declarations, the docblocks and the check's transient types. Storing declarations, summaries and docblock nodes in per-file and per-docblock vectors also removes each object's allocation header and size-class rounding, which dhat does not count.
- **The language server** today holds the 657 MB, plus ≈420 MB of context trees and the sources: about 1.1 GB for this workspace. After items 1, 3 and 19 it holds about the batch run's end state: the trees of its open documents and its LRU instead of every analysed file's, with memos bounded by weight (item 14).
- **These are estimates, and probably optimistic.** The review has so far found items 3, 4 and 5 overstated, and items 6-19 are not yet reviewed. dhat counts requested bytes; the resident size adds mimalloc's overhead. The final heap profile (§11) is the measure.
- The final heap profile (§11) measures all of this.

## 13. How the plan was audited

The plan was:
- checked eleven times, item by item, for whether each design was the most proper form or stopped short;
- then rewritten;
- then given the heap profile's findings;
- and is now being reviewed adversarially, one item at a time, in order, by a separate reviewer. Each of the reviewer's findings was checked against the code before it went in.

All the corrections are in the designs above. The eleven passes, in outline:

| pass | angle | what it changed |
|---|---|---|
| 1 | each design against the code and the 25 G designs | items 4, 9, 11, 13 and 14 extended to their complete forms; the gaps of §2 given designs |
| 2 | remaining costs | the arena before the token arrays; the old collector found still in release builds (13); P7's full resolution memo (14); results by id (5); case references (8) |
| 3 | soundness of the arena and of the names | names out of the bump arena, the frozen arena, exact-size slices (9); the names kept by the file (9, 10); SSE2 as the baseline (11, 12); the identifiers copied as text (15); the debug oracles (0) |
| 4 | every cost over 60 M placed against an item | the stub cache's name table (6); interner round trips (15); path data from the walk (2); the relation memo (7) |
| 5 | whether each can be built as written | the relation memo's generation (7); level-parallel passes that only read (4); sharded tables, ids by prefix sum (5); scratch streams for fragments (10) |
| 6 | lifetimes and shared paths | what outlives the tree (13); the server's index from the loaded bytes (11); one owner implementation (2); the identifier lookup's real costs (12); member order (4) |
| 7 | the cached run's parse window | warm runs (17) |
| 8 | the cached run's start | the parallel walk feeding the parse (2) |
| 9 | the plan against itself | what the context base must hold; id ranges for it (17, 4) |
| 10 | the hottest code left after the plan | nodes as handles, boxed errors (9); operators without a call (12); scopes as radix arrays (18) |
| 11 | late items against each other and the code | one load path (1); the walk's `stat`s and link cycles (2) |

The rewrite made two more changes:
- validation happens once, at the end (§9, §11), and oracles that needed an old implementation alive became dumps from the starting commit;
- the file's names live in an append-only arena of their own, so the tree can refer to them before item 10 changes how the lexer hands out tokens.

**The heap profile (§8) added:**
- the peak's cause and its fix's size (3);
- members held inline in their class's list (4);
- posting lists and sorted reference slices (5);
- records read in place from a mapped file (6);
- the relation memo bounded within a generation (7);
- nodes the size of their variant, with no doc fields on statements (9);
- a file's bodies as one slice (13);
- one memo per doc text and one per tag line, keyed by fingerprint and bounded by weight (14);
- interned values in one allocation each (16, new);
- one argument binding per call (18);
- each tree kept until its last reader (19, new);
- the server keeping no context trees and no unopened sources (1);
- and a memory column throughout.

**The allocation pass (M10, M11, Appendix A)** gave every one of the run's 19.4 M allocations to an item. Its tail of 584 functions was read site by site. It added:
- the walk's and vendor discovery's copies (2);
- context files read into the worker's buffer, and parsed into its scratch arena (3);
- declarations built straight into their final form (4);
- versioned parameters deduplicated only where they occur (8);
- type hints, interpolation and imports built without temporaries (9);
- per-worker lexer tables (10);
- native type names matched without copies, and qualified names lexed as one token (12);
- summary builders in scratch, with references recorded into one vector (13);
- tag lines and tag tokens as slices (14);
- names resolved by hashing their parts (15);
- and, in the check (18): type operations from borrowed parts, per-function state reused, signatures substituted per type, and the tail's mechanisms. Those are messages formatted only when reported, constant types built once, rules that borrow or use scratch, throw and loop states that copy no scope, per-text analyses made once per text, SQL parsed once per text, and one checker per worker.
- From the tail, also: registry queries that read the tables (4); expressions not cloned into query-builder chains, and heredoc and escape payloads decoded in place (9); ignore marks as offsets (11); digit separators skipped (12); and the tail's text copies (15).
- Reading the other 1,007 functions at their lines moved ≈92 K misfiled allocations (`sqlparser`, DQL, `clap`) to items 18 and "once". It added:
  - `finalize` taking its results by value (1);
  - ignore paths matched by the walk's text (2);
  - duplicate sites and origins by `FileId` (5);
  - stub index keys read in place (6);
  - `reconcile_with_native`'s lone member (8);
  - doc groups in the `docs` array (10);
  - `strip_leading_backslash` as a slice (12);
  - doc sites holding the interned scope (13);
  - and, in 18: traversals copying on the first change, memos probed by borrowed keys, shared values borrowed, `Display` writing into the formatter, and the per-function labels and `@var` messages deferred.
- Challenging what the plan kept (≈2.3 M) added three changes:
  - declarations stored per file, as rust-analyzer's `ItemTree` does, with member facts as flag bits (4);
  - summaries in per-file vectors (13);
  - docblock nodes in one vector per docblock (14).
  
  It also added radix nodes written in place when unshared (18), and a profile with untrimmed backtraces for the unplaced 11 K (0). What still stays is listed in §12 with its reason: ≈1.35 M allocations.

**The dependency pass.** New dependencies were allowed after item 9's review, so every item was read again for hand-written code that a sound crate would replace. Each crate's API was checked in its source or its documentation. Our own code still has no `unsafe`; a crate may use it behind a safe API.

| crate | items | what it does there |
|---|---|---|
| `la-arena` 0.3.1 | 4, 9, 14 | typed arenas, ids, ranges and side tables; a list as the range of its contiguous elements |
| `wide` 0.7.33, already in the lockfile | 11, 12 | 16-byte blocks: SSE2 on x86-64, NEON on aarch64 |
| `triomphe` 0.1.16 | 16 | a name as one thin allocation (`ThinArc`); type nodes without a weak count; strong tables, freed by rust-analyzer's rule |
| `quick_cache` 0.7 | 14, 18, 19 | weighted, scan-resistant memos; the server's tree cache bounded by bytes |
| `siphasher` 1.0.4, already in the lockfile through `phf` | 14, 18 | memo fingerprints keyed per process, so no collision can be crafted |
| `blake3` 1.8.7 | 1, 11, 17 | every content hash a cache trusts |

**Considered and not taken:**
- `bumpalo-herd` with `self_cell` (9): a safe arena of references, but ids give smaller nodes, exact sizes, no lifetime and dense side tables.
- `pulp` (11, 12): safe dispatch at run time, but it documents no bitmask operation, and AVX2 would save under 0.1%.
- `rkyv` (6, 17): records read in place with no decoding, but every reader would need the archived form beside the owned one, and after items 6 and 17 only the records a run reads are decoded.
- `jwalk` and `ignore` (2): their walks cannot carry item 2's role pruning, link rules, index-path order and per-directory parse tasks. `same-file` already gives item 2's identity on Windows.
- `slotmap` (4): ids are assigned by prefix sum across a parallel merge (item 5), which a slot map's serial insert cannot do. The generation is kept as `slotmap` designs it.
- `imbl::Vector` and `rpds::Vector` (18): neither exposes the subtree pointers that C4's diff skips by.
- `phf` (12): its SipHash costs more per identifier than the hash the lexer's perfect tables use.
- `roaring` (5): posting lists are short, with one id held inline.
- `bstr` (15): `std`'s `utf8_chunks` gives a name's display.
- `simdutf8`: after item 9, the lexer does not validate a whole source to borrow text from it.

**The item-by-item review** (in progress):

| item | what it found | what changed |
|---|---|---|
| 0 | O1 could not reach the project's parse. The dumps lost information (`Display`, `JsonMessage`). The edit path had no oracle. The list of oracles was incomplete, and their run would have skipped many of them (a per-file cache shared across profiles, an opt-in rule, paths the CLI never takes). The starting state was unvalidated in most configurations. The baseline was to be taken before the code that prints it existed. | The starting state is read first. One oracle run in an `oracle` profile, with coverage counted. Canonical printers that cannot drop a field unseen. O1 made through `parse_file`. A full `registry-dump`. PHPDoc and expression-type records. O2 as the gate's whole artifact set, with every diagnostic field and every cache state. The server session and the edit replay. The baseline from item 0's own commit, identical across thread counts. |
| 1 | Identity by `(device, inode)` is not PHP's. The server resolves its configuration and analyses through second implementations. Tests and benches load a third way. The edit path has its own rules. The server would keep context trees the cache cannot give it. All workspace folders share one project. | The resolved path as identity, with roles by strength and `Session::classify`. One configuration, one load, one analysis. `built_once` split. The server keeps no context trees and no unopened sources. One project per folder. The load off the message loop. The server-equals-CLI oracle. |
| 2 | The `strip_prefix` cost was respelling, not exclusions. The sort's input was mostly duplicates from the vendor context, and a k-way merge of ≈10.8 K runs is a sort. A global set of entered directories makes the result depend on scheduling, and it contradicted item 1. Roles were not final before parsing. The parse queue lost the per-job name cache. Path data was lexical. | Each file listed once, roles from roots, stronger roots pruned. Order by root and entry index, with no comparisons. Cycles checked against ancestors; links resolved once. Per-directory parse tasks with a thread's name cache. Resolved path data and one owner lookup per directory. Each path stored once. A hand-written link tree as the oracle. Item 1's identity rules corrected to match. |
| 3 | Item 3 alone cannot lower the peak by 420 MB: the peak moves to the end of the check. The edit path keeps every tree. The server reparses open files from disk. A parsed and a cached context file have different states. | Retention is one function of (role, open), in the load and in `update_file`. One state shape per role. `ast_of` reads the overlay. A scratch arena per worker. The peak claim corrected to ≤209 MB alone. |
| 4 | Keying a trait method's answers by the trait's own id would share one answer across every using class: unsound. The passes that complete declarations still copy them. Not every pass follows the hierarchy. Ids were reused without a generation, and assigned only by a later item. The fields are public. The memory and allocation estimates were high. | One member-list entry per member a class has, traits' included. Every pass result in a table, so declarations never change after they are built. Passes named by their relation. Generational ids. The fields made private first. ≈90 MB and ≈1.5 M allocations. |
| 5 | The two path sorts were not the results'. File ids are not in path order, and a type-merge order depends on the sort. The tables the insert writes were not all covered. Ids were unstable on the edit path. Sorted slices need an order that agrees with `RefKey`'s case-insensitive equality. `closure_scope`'s text keys were not removed. | Merges by path rank. Named-argument facts only when their rule is on. Every insert table sharded by its own key. Stable ids for a file parsed again. A content order for reference slices. The frontier from ids. ≈5 MB. |
| 6 | The deferred set contradicted item 4's on-demand evaluation. The availability rule would have dropped polyfills for extensions that are not loaded. A mapped file needs `unsafe`, which the workspace denies, and is undefined behaviour under truncation or NFS. A lazy record could not reach its file through serde. A whole-file CRC does not cover lazy decodes. | Deferred types on demand. Availability decided on run-time evidence only (built in, Composer's platform check, the configured runtime). Unknown keeps both declarations. A new check of a polyfill against its native signature. Records read with `read_at` under a decode context, as rustc does, with per-record CRCs and recoverable failures. A test of replacing files in use on every release platform. The context base shares the container. |
| 7 | A registry-backed memo already existed (`KEEP_DECISIONS`). The generation must cover every registry content the relation reads, not just the hierarchy. On-demand evaluation could freeze an answer computed under an open cycle guard. Clearing per generation does not bound a batch run. The settings in the key are not read. The oracle's switch was test-only. | A content generation, also for read-only pass levels. One memo absorbing both. Provisional answers under cycle guards, with each cycle resolved per component by what PHP does: a constant cycle throws, so its reads are `never` and are reported. A two-segment bound. A runtime off switch. |
| 8 | Six case-reference fields with seven kinds of readers, not one. One reader checks the merged reflection, not what the tag writes. Overlap with item 4's `FileDecls`. A wrong reason given for duplicate parameters. `reconcile_with_native` still collected. | Every field listed with its readers. Case references computed at every checked position from what is written. Dedup done in the file's parameter vector. One semantic case listed. The saving restated as work. |
| 9 | The arena's `Sync` came from an `unsafe impl`, which the workspace denies, and the name arena was not `Sync` either. The tree is changed after the parse (the stub rules, anonymous-class names, a second name resolution), and other code builds and keeps expressions. A non-UTF-8 source cannot give `&str` payloads, and its identifiers are decoded lossily today, so `class \xA9` and `class \xAA` are one class. Statement docs need only today's doc sites. The counts mixed rules: the expression clones included PHPDoc's, and M9 counted by a rule no item used. Checking the stub rules found that they apply to any file that mentions them, and for the newest PHP version only. | The tree as typed vectors addressed by `u32` ids: no `unsafe`, no lifetime, a few allocations per kept file. Chosen on merit over a safe reference arena (`bumpalo-herd` with `self_cell`): smaller nodes, exact sizes, no lifetime, dense side tables. The stub rules applied as a stub is parsed, to stubs only, recording versions that item 6's readers answer for the configured range. Anonymous-class names as a side table. Every holder of an expression listed, holding ids. Payloads as byte ranges; names as bytes (item 15) and docs lexed as bytes (item 14). Attributes kept in their node. M9 recounted by M10's rule, and item 9's counts stated by it. |
| 10 | Lexing the whole file first would lex the data after `__halt_compiler` as PHP. Interpolated strings were to keep a sub-parser, where PHP's scanner emits their parts inline. A lexer error anywhere would still beat an earlier syntax error, where PHP reports the first. Docs attached by adjacency miss the docs PHP gives declarations. The token readers were incomplete. Allocations were counted twice, and the item's M10 row held the lexer's names and the line index's `Arc`. The lexer ignores the configured versions. | The lexer stops where PHP's does, records the offset, and `__COMPILER_HALT_OFFSET__` gets its value, with a check of its use elsewhere. String parts as inline tokens; the sub-parser, fragments and scratch streams go. Errors recorded with offsets, the first one reported. Declarations' docs by PHP's rule (only `}` clears a pending doc, as `php` showed, not `{`), with a check of a doc given away. A check of source the configured versions lex differently. Comments only on request. The row recounted to 0.23 M, with the names moved to item 16 (whose `Now` is corrected: three allocations per name, always) and the `Arc` to item 11. |
| 11 | PHP and the editor end a line at a lone `\r`, and PHP ends a line comment there too; today neither does. Context files need columns for their lexer warnings and parse failures. The per-file cache stores the index. Open documents need their overlay. The count was 46,422, not 43,630, and fragment lexers build an empty index. | Lines and line comments end where PHP's do. Warnings and failures record their line and column on the worker. `LineStarts` in the cache, with a format bump. The overlay first. One allocation for `LineStarts`, none for fragments. |
| 12 | Six places where our lexer reads source otherwise than PHP's: doc comments without whitespace after `/**`, a lone `\r` after `?>` and in heredocs, heredoc indentation PHP refuses, casts as one token with only spaces and tabs inside, the spaced qualified name that PHP 8 rejects, and open tags (`<?phpx`, and `<?` by `short_open_tag`). A perfect-hash watch list is not needed. Blank runs are mostly one byte. One comparison cannot answer both questions of step 2. | Each reading made PHP's, checked against `token_get_all`, and listed as semantic; the cast checks of `coercion.rs` kept. Watch bits cached with the name. Blank runs tested a byte first. The keyword comparison restated. `parse_qualified_name`'s allocations moved from item 9's row to this one. |

# Appendix A. The allocation tail, every site

The 584 allocating functions of M11 are given below, grouped by mechanism, with their allocations and their main allocating line. Third-party parsers are grouped by crate. The fix for each mechanism is in M11 and in the item it names.

**F1 messages formatted before a report** (37 sites, 101,164 allocations):
 `Checker::check_property_phpdoc_types` 27,188 (phpdoc_type_compat.rs:377); `Checker::check_property_declarations` 26,507 (property_declarations.rs:133); `format_string::printf_placeholders` 11,768 (format_string.rs:105); `Checker::check_phpdoc_types_against_native` 11,545 (phpdoc_type_compat.rs:46); `Checker::check_class_generic_bounds` 9,218 (bounds.rs:198); `Checker::report_missing_offset` 8,319 (assignment.rs:764); `Checker::check_closure_literal_with_param_overrides` 1,652 (closure_context.rs:117); `Checker::check_ancestor_tags` 1,310 (ancestors.rs:365); `Checker::check_non_boolean_condition` 477 (operators.rs:269); `Checker::check_impossible_type_check_call` 467 (probe_checks.rs:600); `Checker::check_class_constant_modifiers` 400 (oop.rs:1388); `Checker::check_set_hook` 301 (property_hooks.rs:345); `Checker::resolve_named_callable` 286 (callable_values.rs:73); `Checker::check_pointless_null_probe` 215 (probe_checks.rs:159); `Checker::check_constant_numeric_comparison` 197 (probe_checks.rs:537); `Checker::check_useless_cast` 174 (strict_rules.rs:548); `Checker::check_pest_risky_test` 134 (risky_tests.rs:76); `Checker::check_arithmetic_operands` 133 (strict_rules.rs:218); `Checker::check_method_tag_types` 114 (oop.rs:268); `AncestorMessages::variance_context` 111 (ancestors.rs:180); `Checker::check_hook_phpdoc_types` 84 (phpdoc_type_compat.rs:305); `Checker::report_missing_typehint_findings` 75 (missing_typehint.rs:230); `Checker::check_strict_rules_statement` 72 (strict_rules.rs:644); `Checker::check_array_values_no_op` 67 (array_ops.rs:433); `diagnostics::DiagnosticBuilder::tip::` 50 (lib.rs:222); `Checker::check_get_hook` 48 (property_hooks.rs:165); `Checker::check_pointless_offset_or_property_probe` 45 (probe_checks.rs:295); `Checker::check_impossible_instanceof` 44 (impossible_instanceof.rs:43); `Checker::check_boolean_and_or` 44 (boolean_conditions.rs:134); `Checker::check_offset_accessibility` 29 (assignment.rs:831); `Checker::check_non_boolean_and_or_operand` 27 (operators.rs:296); `parameters::describe_parameter` 18 (parameters.rs:0); `Checker::check_risky_test_methods` 16 (risky_tests.rs:42); `Checker::check_strict_comparison_of_different_types` 11 (probe_checks.rs:468); `Checker::check_ini_directive_call` 8 (ini_directives.rs:200); `Checker::check_stmt_range` 6 (stmt.rs:164); `Checker::check_one_assert_tag` 4 (narrowing.rs:1479).

**F2 constant types rebuilt per call** (41 sites, 33,938 allocations):
 `Checker::infer_builtin_call_return_type` 8,002 (builtin_calls.rs:127); `Checker::valid_operand` 7,926 (strict_rules.rs:230); `Checker::is_strictly_number` 4,124 (operators.rs:672); `Checker::is_number_operand` 3,481 (operators.rs:57); `Checker::doctrine_query_type` 1,790 (member_resolution.rs:1166); `predicates::narrowing_type_for_predicate` 1,464 (predicates.rs:153); `Checker::is_incdec_operand_valid` 1,189 (operators.rs:685); `Checker::object_method_call_return_type` 907 (member_resolution.rs:882); `Checker::infer_spread_array_literal_type` 774 (array_ops.rs:263); `Checker::infer_case_function_call` 729 (builtin_calls.rs:2202); `phpunit::is_kind_type` 539 (phpunit.rs:557); `format_string::format_value_type` 535 (format_string.rs:650); `Checker::predefine_byref_or_unknown_args` 419 (calls.rs:158); `member_resolution::dbal_row_method_return_type` 381 (member_resolution.rs:1029); `preg_shapes::accessory_string` 365 (preg_shapes.rs:59); `builtin_calls::date_format_return_type` 265 (builtin_calls.rs:3043); `array_ops::general_array_literal_type` 265 (array_ops.rs:762); `builtin_calls::curl_getinfo_component_type` 224 (builtin_calls.rs:2776); `type_resolution::reconcile_doc_with_native` 115 (type_resolution.rs:125); `preg_shapes::list_of` 101 (preg_shapes.rs:622); `builtin_calls::parse_url_components_array_shape` 81 (builtin_calls.rs:3168); `builtin_calls::unseal_with` 52 (builtin_calls.rs:2445); `type_resolution::apply_implicit_nullable_default` 36 (type_resolution.rs:38); `Checker::bitwise_not_type` 28 (operators.rs:101); `builtin_calls::get_debug_type_for` 22 (builtin_calls.rs:0); `builtin_calls::pathinfo_array_shape` 20 (builtin_calls.rs:3150); `Checker::bitnot_operand_is_valid` 14 (operators.rs:488); `Checker::assertion_check` 11 (phpunit.rs:224); `Checker::mysqli_query_type` 11 (mysqli.rs:76); `builtin_calls::parse_url_component_type` 10 (builtin_calls.rs:3186); `CaseOp::fold` 10 (builtin_calls.rs:2289); `builtin_calls::hrtime_number_type` 9 (builtin_calls.rs:3471); `builtin_calls::gettype_result` 8 (builtin_calls.rs:2951); `Checker::overloads_arithmetic` 7 (strict_rules.rs:268); `builtin_calls::filter_validate_type_for_constant` 6 (builtin_calls.rs:2688); `mysqli::returns_result_set` 6 (mysqli.rs:20); `Checker::mysqli_fetch_type` 3 (mysqli.rs:151); `Checker::check_format_values_array_type` 3 (format_string.rs:491); `mysqli::widen_for_text_protocol` 3 (mysqli.rs:44); `builtin_calls::date_interval_format_return_type` 2 (builtin_calls.rs:3119); `builtin_calls::date_interval_format_literal` 1 (builtin_calls.rs:3101).

**F3 expressions cloned into Rc** (8 sites, 58,287 allocations):
 `Checker::resolve_query_builder_chain` 32,182 (query_builder.rs:163); `Checker::resolve_dbal_query_builder_chain` 15,514 (query_builder.rs:338); `Checker::record_condition_alias` 4,821 (narrowing.rs:1313); `Checker::foldable_arg_string` 2,641 (query_builder.rs:340); `Checker::record_query_builder_alias` 1,355 (query_builder.rs:134); `Checker::foldable_arg_strings` 788 (query_builder.rs:354); `Checker::foldable_sql_arg_strings` 689 (query_builder.rs:616); `Checker::record_if_body_flag_alias` 297 (narrowing.rs:1399).

**F4 third-party parsers** (112 sites, 94,265 allocations):
 `sqlparser` 86,548; `regex_automata` 6,912; `regex_syntax` 765; `sql` 37; `regex` 3.

**F5 text copies (item 15)** (38 sites, 102,938 allocations):
 `Checker::closure_literal_type` 15,819 (expr.rs:5839); `Checker::check_internal_instantiation` 11,683 (internal_usage.rs:294); `Checker::forget_members_a_call_may_change` 9,455 (expr.rs:280); `Checker::instanceof_outcome` 8,472 (impossible_instanceof.rs:85); `Checker::static_member_of_class` 7,022 (expr.rs:6188); `Checker::check_closure_missing_return` 6,408 (function_decl.rs:1555); `stmt::statement_defined_variable_names` 6,316 (stmt.rs:1647); `Checker::walk_for_interface_conflicts` 6,157 (interfaces.rs:127); `Checker::check_attribute_targets` 4,311 (attributes.rs:182); `Checker::check_program` 3,903 (checker.rs:1629); `Promotions::visit_stmt` 3,864 (function_like_declarations.rs:78); `Checker::class_info` 2,755 (checker.rs:1073); `Checker::check_attributes` 2,231 (attributes.rs:69); `Checker::first_class_callable_type` 2,022 (expr.rs:5789); `Checker::check_require_tags` 1,932 (oop.rs:1824); `Checker::check_class_like_marker_attributes` 1,932 (attributes.rs:352); `Checker::check_magic_constant_context` 1,826 (global_constants.rs:143); `Checker::first_class_callable_target` 1,357 (expr.rs:5745); `Checker::unpacked_constant_array_arguments` 1,023 (parameters.rs:385); `Checker::resolve_class_ref` 996 (ancestors.rs:303); `Checker::check_default_property_types` 785 (property_default_types.rs:50); `Checker::resolve_type_hint_with_case_refs` 782 (resolve.rs:77); `Checker::constant_string_value` 714 (var_tag_types.rs:68); `Initializers::add` 339 (includes.rs:83); `Checker::check_self_attribute_class_declaration` 203 (attributes.rs:277); `resolve::collect_scoped_template_params` 132 (resolve.rs:682); `Checker::check_template_type_not_in_parameter` 109 (resolve.rs:240); `Checker::check_typed_class_constant_values` 81 (oop.rs:883); `Checker::check_call_site_variance` 78 (bounds.rs:467); `Checker::check_enum_case_values` 67 (enums.rs:77); `Checker::current_file_absolute` 47 (includes.rs:158); `Checker::current_dir_of_file` 47 (includes.rs:169); `ConstantResolver::configured_type` 29 (constant_resolver.rs:105); `Initializers::visit_stmt` 29 (includes.rs:92); `Checker::record_named_argument_call` 4 (named_argument_renames.rs:77); `Checker::check_trait_constant_conflicts` 4 (oop.rs:1170); `member_usage::seed_self_typed_param_aliases` 3 (member_usage.rs:294); `ClosureVariables::visit_expr` 1 (rebind.rs:54).

**F6a rule temporaries collected per call** (36 sites, 143,007 allocations):
 `Checker::check_type_version_support` 35,176 (type_declarations.rs:182); `Checker::offset_read_type` 22,482 (member_resolution.rs:298); `Checker::check_array_literal_duplicate_keys` 13,714 (array_ops.rs:70); `Checker::instance_of_any` 13,106 (operators.rs:706); `Checker::check_offset_assignment` 10,587 (offset_assignment.rs:46); `internal_rules::declaration_level_rules` 9,287 (internal_rules.rs:56); `includes::default_include_dirs` 8,376 (includes.rs:380); `Checker::check_function_like_declaration` 7,390 (function_like_declarations.rs:266); `Checker::call_bindings` 5,382 (array_ops.rs:560); `assignment::native_after_offset_change` 4,377 (assignment.rs:959); `Checker::check_duplicate_properties` 2,482 (duplicates.rs:115); `class_shape::collect_classes` 1,875 (class_shape.rs:34); `Checker::check_incdec_version_warnings` 1,189 (operators.rs:645); `assignment::literal_keys_from_type` 1,120 (assignment.rs:975); `Checker::infer_expr_unguarded` 1,007 (expr.rs:4976); `Project::insert_numbered_file` 999 (project.rs:1034); `Checker::check_property_assignment` 719 (property_assignment.rs:126); `type_declarations::shape` 677 (type_declarations.rs:94); `Checker::castable_parameter_name` 674 (castable_arguments.rs:0); `Checker::infer_array_literal_type` 568 (array_ops.rs:117); `Checker::infer_closure_literal_in_context` 512 (expr.rs:5985); `Checker::record_usage_semantic_token` 405 (semantic_tokens.rs:63); `predefined_constants::string_literals` 276 (predefined_constants.rs:104); `Checker::apply_check` 173 (phpunit.rs:0); `Checker::check_duplicate_consts` 168 (duplicates.rs:56); `includes::file_constant_initializers` 141 (includes.rs:70); `Checker::check_comparison_validity` 72 (operators.rs:467); `Callee::of` 30 (parameters.rs:0); `Checker::infer_binary` 23 (expr.rs:5502); `Checker::check_strict_function_call` 4 (strict_rules.rs:0); `Checker::implicit_method_call` 4 (array_access.rs:110); `Checker::check_array_access_offset` 4 (array_access.rs:206); `Checker::filters_unambiguously` 4 (strict_rules.rs:530); `bool::from_elem::` 2 (spec_from_elem.rs:24); `type_declarations::atoms` 1 (type_declarations.rs:79); `stmt::reconstructed_discriminant_return_types` 1 (stmt.rs:29).

**F6b registry queries and passes (item 4)** (47 sites, 149,360 allocations):
 `Registry::flatten_phpdoc_inheritance (resolve.rs:1147 parent_maps, by ` 88,501; `Registry::find_final_ancestor_method` 7,734 (hierarchy.rs:917); `ClassReflection::add_enum_members` 6,506 (class_info.rs:1834); `Registry::required_abstract_methods` 6,457 (hierarchy.rs:1028); `resolve::visible_class_consts` 6,270 (resolve.rs:1517); `Registry::has_concrete_implementation` 5,581 (hierarchy.rs:1070); `Registry::class_is_immutable` 5,060 (hierarchy.rs:311); `ReflectionAttribute::all` 4,853 (reflection_attribute.rs:63); `Registry::ancestor_bindings` 1,744 (resolve.rs:1014); `reflection_attribute::value` 1,337 (reflection_attribute.rs:112); `Registry::find_final_ancestor_property` 1,146 (hierarchy.rs:950); `resolve::matching_class_const_type` 1,096 (resolve.rs:1539); `GlobalFacts::resolve` 1,053 (global_vars.rs:244); `ReflectionAttribute::from_ast` 1,016 (reflection_attribute.rs:54); `resolve::glob_matches` 940 (resolve.rs:1557); `GuardExpr::of_terms` 906 (guard.rs:116); `Checker::check_class_ancestors` 865 (ancestors.rs:202); `asserts::resolve_assert_tags` 804 (asserts.rs:130); `global_vars::resolve_include_target_suffix` 735 (global_vars.rs:455); `Checker::check_inheritance_target_kinds` 694 (oop.rs:126); `GuardExpr::of_condition` 648 (guard.rs:128); `generics::build_type_aliases` 636 (generics.rs:77); `global_vars::resolve_include_target_suffix` 510 (global_vars.rs:455); `Checker::check_constant_overrides` 502 (oop.rs:1026); `Registry::has_unresolvable_ancestor` 474 (hierarchy.rs:864); `class_info::class_and_args` 415 (class_info.rs:2130); `Scope::visit_expr` 377 (global_vars.rs:560); `doctrine_entity::compute_doctrine_entity` 338 (doctrine_entity.rs:238); `ClassReflection::own_template_args` 294 (resolve.rs:1726); `ClassReflection::own_template_args` 243 (resolve.rs:1726); `Checker::check_generic_reference_arity` 230 (bounds.rs:408); `Checker::ancestor_tag_types` 222 (ancestors.rs:542); `global_vars::handle_assignment_target` 196 (global_vars.rs:614); `generics::expand_type_alias` 194 (generics.rs:111); `GuardExpr::on` 180 (guard.rs:183); `ClassReflection::merge` 170 (class_info.rs:2048); `Checker::parent_classes` 138 (strict_rules.rs:805); `tag_synthesis::method_tag_sig` 115 (tag_synthesis.rs:99); `global_vars::resolve_value` 109 (global_vars.rs:784); `global_vars::static_value` 31 (global_vars.rs:731); `FunctionReflection::merge` 13 (function_sig.rs:945); `Checker::members_implementing` 11 (array_access.rs:40); `ContributionFact::resolve` 5 (global_vars.rs:281); `Checker::check_method_overrides` 4 (method_overrides.rs:68); `global_vars::handle_assignment_target` 3 (global_vars.rs:614); `global_vars::static_value` 3 (global_vars.rs:731); `Checker::check_used_traits` 1 (ancestors.rs:328).

**F6c throw, exit and loop states** (23 sites, 37,839 allocations):
 `Checker::check_try` 8,733 (try_catch.rs:99); `Checker::push_exit_point` 6,619 (completion.rs:364); `Checker::record_call_throw_point` 6,335 (throws.rs:79); `Checker::converge_loop_body` 6,298 (control_flow.rs:143); `Checker::record_throw_state` 3,920 (expr.rs:348); `Checker::check_throw` 2,835 (try_catch.rs:390); `Checker::check_too_wide_return_type` 993 (return_type_too_wide.rs:104); `Checker::record_by_ref_exit_snapshot` 501 (by_ref_params.rs:123); `Checker::check_foreach` 451 (loops.rs:892); `Checker::statement_exit_points` 304 (completion.rs:358); `Checker::check_switch` 187 (switch.rs:116); `Checker::closure_syntactic_return_type` 129 (expr.rs:5913); `Checker::closure_syntactic_return_type::Returns as syntax::visit::Visi` 129 (expr.rs:5903); `Checker::record_property_type_site` 107 (property_type_too_wide.rs:101); `Checker::check_too_wide_by_ref_type` 67 (by_ref_params.rs:183); `property_type_too_wide::check_too_wide_property_types` 54 (property_type_too_wide.rs:144); `Checker::check_too_wide_throw_type` 48 (too_wide.rs:84); `Checker::converge_loop_body` 42 (control_flow.rs:140); `Checker::check_for` 32 (loops.rs:386); `Checker::converge_loop_body` 30 (control_flow.rs:141); `Checker::converge_loop_body` 16 (control_flow.rs:140); `Checker::check_by_ref_execution_end` 5 (by_ref_params.rs:154); `Checker::converge_loop_body` 4 (control_flow.rs:140).

**F6d narrowing and implied facts** (26 sites, 29,406 allocations):
 `Checker::check_if` 7,253 (stmt.rs:1374); `narrowing::record_implied_facts` 3,249 (narrowing.rs:569); `narrowing::condition_alias_targets` 3,106 (narrowing.rs:590); `control_flow::add_implied_fact` 2,987 (control_flow.rs:276); `Checker::apply_ordering_narrowing` 2,772 (assertions.rs:325); `Checker::apply_preg_comparison` 1,497 (preg.rs:48); `expr::merge_new_variable_definitions` 1,237 (expr.rs:5560); `Checker::narrow_counted` 1,201 (narrowing.rs:1807); `control_flow::widen_implied_facts` 865 (control_flow.rs:336); `Checker::narrow_strict_membership` 784 (narrowing.rs:1741); `query_builder::combine_variadic_predicates` 687 (query_builder.rs:824); `assertions::single_values_of` 529 (assertions.rs:502); `Checker::in_array_target_type` 529 (assertions.rs:131); `Checker::narrow_preg_outcome` 418 (preg.rs:117); `Checker::refine_string_members` 406 (phpunit.rs:544); `control_flow::merge_implied_facts` 352 (control_flow.rs:314); `Checker::replay_truthiness_discriminant_facts` 349 (narrowing.rs:320); `Checker::apply_loose_equality_narrowing` 325 (assertions.rs:290); `Checker::narrow_isset_containers` 265 (narrowing.rs:241); `Checker::in_array_narrowing` 224 (assertions.rs:96); `Checker::widen_towards_entry` 210 (control_flow.rs:102); `Checker::narrow_key_presence` 113 (narrowing.rs:1713); `Checker::replay_object_discriminant_facts` 39 (narrowing.rs:346); `predicates::propagate_container_field_facts` 6 (predicates.rs:111); `Checker::apply_assert_infos` 2 (assertions.rs:423); `Checker::check_unset_target` 1 (assignment.rs:627).

**F6e separate walks** (27 sites, 40,084 allocations):
 `strict_rules::bind_targets` 20,655 (strict_rules.rs:951); `Scope::visit_stmt` 2,831 (keywords.rs:95); `whitespace::section_ends` 2,792 (whitespace.rs:28); `Declares::visit_stmt` 2,723 (keywords.rs:310); `Members::visit_expr` 2,405 (member_usage.rs:139); `Checker::check_generic_callables` 2,103 (phpdoc_type_compat.rs:189); `Checker::declared_method_return_type` 1,388 (member_resolution.rs:556); `Assigned::visit_stmt` 1,384 (strict_rules.rs:890); `Checker::expand_enum_cases` 937 (expr.rs:6558); `stmt::assigned_variable_names` 788 (stmt.rs:1692); `walk_expr::` 395 (visit.rs:243); `member_usage::mark_write_target` 380 (member_usage.rs:0); `Checker::resolve_paths_at` 287 (includes.rs:200); `Vars::visit_expr` 274 (var_usage.rs:79); `Checker::declared_property_type` 224 (member_resolution.rs:395); `Checker::collect_target_checks` 175 (var_tag_types.rs:568); `ast::literal_string_value` 82 (ast.rs:1616); `ast::literal_string_value` 68 (ast.rs:1616); `strict_rules::defined_targets` 68 (strict_rules.rs:815); `PropertyView::hook_bodies` 58 (property_declarations.rs:38); `Checker::check_unused_constructor_parameters` 25 (unused.rs:50); `walk_expr::` 18 (visit.rs:243); `Checker::iterable_element_type` 12 (loops.rs:487); `Checker::instanceof_verdict` 4 (impossible_instanceof.rs:97); `calls::find_preg_quote_calls` 3 (calls.rs:924); `phpdoc_type_compat::collect_conditionals` 3 (phpdoc_type_compat.rs:648); `GuardArgs::visit_expr` 2 (includes.rs:395).

**F6f per-text analyses (regex, DQL, SQL, format strings)** (61 sites, 88,382 allocations):
 `builtin_calls::sprintf_pieces` 15,654 (builtin_calls.rs:3250); `AtomResult::literal` 9,668 (regex_groups.rs:376); `Scanner::scan_atom` 8,522 (regex_groups.rs:936); `dql::tokenize` 7,017 (dql.rs:172); `TypedColumn::from_column` 5,793 (resolve.rs:453); `DbalQueryBuilderChain::render` 5,675 (query_builder.rs:192); `query_builder::fold_string_expr` 4,763 (query_builder.rs:625); `builtin_calls::sprintf_literal_remainder` 3,849 (builtin_calls.rs:3225); `result_shape::build_alias_map` 2,417 (result_shape.rs:62); `Scanner::scan_branch` 2,393 (regex_groups.rs:707); `Scanner::finish_capturing_group` 2,191 (regex_groups.rs:1364); `Scanner::new` 2,106 (regex_groups.rs:505); `Scanner::scan_alternation` 1,702 (regex_groups.rs:582); `resolve::resolve_select_from_static_schema` 1,591 (resolve.rs:477); `preg_shapes::preg_match_matches_shape` 1,484 (preg_shapes.rs:106); `resolve::cross_reference_entity_fields` 1,369 (resolve.rs:122); `Checker::apply_dbal_chain_call` 1,139 (query_builder.rs:386); `parameter_validation::sql_placeholder_keys` 866 (parameter_validation.rs:73); `dql::parse_select_items` 809 (dql.rs:451); `Scanner::combine_branches` 747 (regex_groups.rs:673); `query_builder::combine_predicate_parts` 686 (query_builder.rs:259); `sql_reflection::column_php_type` 681 (sql_reflection.rs:37); `regex_groups::parse_capture_groups` 616 (regex_groups.rs:210); `resolve::refine_count_columns` 554 (resolve.rs:334); `resolve::single_table_name` 550 (resolve.rs:152); `Checker::format_counts` 545 (format_string.rs:394); `Checker::resolve_chain_root` 522 (query_builder.rs:229); `Scanner::scan_escape_atom` 521 (regex_groups.rs:1162); `Checker::preg_matches_type` 467 (preg_shapes.rs:574); `parameter_validation::connection_params_from_args` 317 (parameter_validation.rs:149); `preg_shapes::build_matched_shape` 297 (preg_shapes.rs:188); `AggregateFn::from_name` 264 (dql.rs:44); `regex_groups::repeat_literals` 260 (regex_groups.rs:415); `result_shape::resolve_join_target` 231 (result_shape.rs:80); `Checker::apply_chain_call` 207 (query_builder.rs:314); `resolve::resolve_pure_ast` 166 (resolve.rs:194); `result_shape::to_array_mode` 166 (result_shape.rs:537); `preg_match_all_pattern_order_shape::` 145 (preg_shapes.rs:366); `Scanner::scan_digits` 131 (regex_groups.rs:869); `resolve::count_column_positions` 125 (resolve.rs:370); `resolve::pure_ast_expr` 125 (resolve.rs:214); `resolve::count_argument_name` 125 (resolve.rs:265); `Scanner::scan_branch` 118 (regex_groups.rs:707); `result_shape::unaliased_scalar_key` 117 (result_shape.rs:113); `Scanner::scan_alternation` 115 (regex_groups.rs:582); `Scanner::combine_branches` 94 (regex_groups.rs:641); `query_builder::fold_sprintf_call` 88 (query_builder.rs:685); `regex_groups::repeat_literals` 55 (regex_groups.rs:420); `AtomResult::literal` 49 (regex_groups.rs:376); `query_builder::order_by_entry` 49 (query_builder.rs:834); `Scanner::scan_name_until` 40 (regex_groups.rs:1433); `preg_shapes::preg_leaf_type` 36 (preg_shapes.rs:14); `Scanner::scan_digits` 34 (regex_groups.rs:869); `preg_shapes::group_leaf_type` 33 (preg_shapes.rs:38); `Scanner::scan_atom` 32 (regex_groups.rs:936); `preg_shapes::parse_preg_flags` 24 (preg_shapes.rs:415); `Scanner::finish_capturing_group` 17 (regex_groups.rs:1345); `Scanner::scan_escape_atom` 17 (regex_groups.rs:1162); `resolve::literal_name` 5 (resolve.rs:0); `Scanner::finish_lookaround` 2 (regex_groups.rs:1412); `format_string::placeholder_pattern` 1 (format_string.rs:44).

**F7 needed or front end** (124 sites, 125,981 allocations):
 `Session::read_and_parse_file` 59,799 (session.rs:631); `index::built_declaration` 28,516 (index.rs:715); `declarations::if_chain (declarations.rs:172, by location)` 15,149; `Checker::new` 5,584 (checker.rs:1155); `Checker::set_current_file` 2,792 (checker.rs:1222); `Project::checker_for` 2,792 (project.rs:2287); `strings::strip_heredoc_indent` 1,722 (strings.rs:788); `ignore_comments::parse_identifiers` 1,508 (ignore_comments.rs:335); `Checker::report` 1,000 (checker.rs:1495); `Lexer::mark_ignore` 988 (lexer.rs:535); `Lexer::lex_word` 884 (lexer.rs:870); `Lexer::strip_digit_separators` 582 (lexer.rs:1022); `Registry::record_phpstan_class_file` 518 (index.rs:543); `ignore_comments::tokenize_ignore` 457 (ignore_comments.rs:268); `Pending::settle` 438 (ignore_comments.rs:53); `anonymous_class::synthetic_name` 372 (anonymous_class.rs:0); `ignore_comments::scan_parsed` 267 (ignore_comments.rs:104); `Registry::record_changed_constant` 261 (registry.rs:634); `write_json::` 176 (output.rs:177); `Project::judge_property_types` 159 (project.rs:2248); `strings::encode_codepoint` 151 (strings.rs:814); `Registry::keeps_member` 86 (resolve.rs:1918); `function::FnOnce` 86 (function.rs:250); `anonymous_class::name_anonymous_classes` 82 (anonymous_class.rs:59); `name_context::resolve_names_in_type` 81 (name_context.rs:41); `config::Schema::validate` 78 (lib.rs:120); `config::ConfigFile::section::` 78 (lib.rs:243); `config::Schema::section` 78 (function.rs:250); `config::Schema::section` 78 (function.rs:250); `config::Schema::section` 78 (function.rs:250); `Lines::visit_expr` 78 (anonymous_class.rs:103); `Found::visit_expr` 76 (anonymous_class.rs:144); `toml::de::from_str::` 74 (mod.rs:76); `Registry::insert_built_decl` 73 (index.rs:353); `Checker::check_anonymous_class` 73 (expr.rs:165); `parse_with_ignores::` 72 (project.rs:2902); `start_thread` 69 (pthread_create.c:454); `Checker::report` 54 (checker.rs:1495); `name::NameEntry` 53 (name.rs:743); `Records::load` 44 (packed.rs:44); `clap_builder::builder::styled_str::StyledStr::push_str` 28 (styled_str.rs:47); `Registry::doctrine_entity_index` 24 (registry.rs:794); `clap_builder::builder::arg::Arg::value_names::` 23 (arg.rs:1352); `toml::value::Value::try_into::` 20 (value.rs:71); `clap_builder::parser::matches::matched_arg::MatchedArg::append_val` 18 (matched_arg.rs:118); `assertion_tables::default_class_argument_returns` 17 (assertion_tables.rs:142); `clap_builder::parser::matches::matched_arg::MatchedArg::new_val_group` 16 (matched_arg.rs:112); `name_table::NameTable` 13 (name_table.rs:745); `name_table::NameTable` 13 (name_table.rs:822); `toml::value::Value::try_into::` 13 (value.rs:71); `Session::analyse` 12 (session.rs:929); `toml::value::Value::try_into::` 12 (value.rs:71); `name_table::NameTable` 11 (name_table.rs:745); `Project::check_class_shapes` 11 (class_shape.rs:99); `Project::finish_diagnostics` 11 (project.rs:2373); `clap_builder::builder::command::Command::_build_subcommand` 11 (command.rs:4557); `clap_builder::mkeymap::MKeyMap::push` 10 (mkeymap.rs:95); `clap_builder::builder::arg::Arg::_build` 10 (arg.rs:4605); `start_thread` 8 (pthread_create.c:454); `Registry::record_assert_tagged_names` 8 (index.rs:481); `config::Schema::check_table` 8 (lib.rs:149); `toml::de::parser::dearray::DeArray::push` 7 (dearray.rs:30); `config::Schema::section` 7 (function.rs:250); `clap_builder::builder::arg_group::ArgGroup::arg::` 7 (arg_group.rs:139); `clap_builder::builder::command::Command::get_matches` 7 (command.rs:664); `config::Schema::moved` 6 (lib.rs:82); `clap_builder::builder::command::Command::group::` 6 (command.rs:469); `clap_builder::builder::arg::Arg::get_possible_values` 6 (arg.rs:4326); `toml::de::from_str::` 6 (mod.rs:76); `PestConfig::resolve_this_type` 6 (pest_config.rs:314); `clap_builder::parser::matches::matched_arg::MatchedArg::push_index` 6 (matched_arg.rs:75); `PestFacts::of` 6 (pest_config.rs:270); `Checker::into_diagnostics` 5 (checker.rs:1536); `name::NameEntry` 5 (name.rs:743); `name_table::NameTable` 5 (name_table.rs:765); `clap_builder::util::any_value::AnyValue::new::` 5 (any_value.rs:12); `clap_builder::builder::os_str::OsStr::to_os_string` 5 (os_str.rs:46); `config::Schema::section::` 4 (lib.rs:66); `name::NameEntry` 4 (name.rs:743); `clap_builder::util::flat_map::Entry` 4 (flat_map.rs:161); `clap_builder::builder::command::Command::unroll_arg_requires::` 4 (command.rs:5092); `map_on_pool_largest_first::` 3 (project.rs:460); `toml::de::parser::document::State::capture_key_value` 3 (document.rs:246); `clap_builder::mkeymap::MKeyMap::_build` 3 (mkeymap.rs:139); `clap_builder::util::graph::ChildGraph` 3 (graph.rs:24); `toml::value::Value::try_into::` 3 (value.rs:71); `toml_datetime::de::VisitMap::next_key_seed::` 3 (de.rs:74); `toml::de::parser::parse_document` 2 (mod.rs:32); `check_project::report` 2 (check_project.rs:308); `clap_builder::builder::command::Command::subcommand_internal` 2 (command.rs:538); `config::ConfigFile::load` 2 (lib.rs:202); `crossbeam_epoch::collector::Collector::new as core::ops::function::FnO` 2 (function.rs:250); `Registry::load` 2 (registry.rs:746); `clap_builder::util::flat_map::FlatMap` 2 (flat_map.rs:32); `pest_config::class_names_from_uses_call` 2 (pest_config.rs:63); `Scope::from_keys` 2 (scope.rs:63); `clap_builder::util::any_value::AnyValue::new::` 2 (any_value.rs:12); `php_date_parser::validate_iso_date_time` 2 (php_date_parser.rs:203); `lexer::lossy` 2 (lexer.rs:149); `name_context::resolve_names_in_type` 1 (name_context.rs:41); `function::FnOnce` 1 (function.rs:250); `Index::of` 1 (packed.rs:140); `toml::de::parser::document::descend_path` 1 (document.rs:403); `toml::de::parser::document::State::finish_table` 1 (document.rs:310); `renamed_named_arguments::merge_named_argument_calls` 1 (renamed_named_arguments.rs:37); `PestFacts::of` 1 (pest_config.rs:273); `toml::de::parser::key::State::close_key` 1 (key.rs:120); `Project::new` 1 (project.rs:533); `Project::expect_files::` 1 (project.rs:1220); `clap_lex::RawArgs as core::convert::From` 1 (lib.rs:269); `clap_builder::parser::arg_matcher::ArgMatcher::subcommand` 1 (arg_matcher.rs:122); `walk_expr::` 1 (visit.rs:243); `clap_builder::util::any_value::AnyValue::new::` 1 (any_value.rs:12); `clap_builder::parser::matches::matched_arg::MatchedArg::push_index` 1 (matched_arg.rs:75); `function::FnOnce` 1 (function.rs:250); `clap_builder::util::any_value::AnyValue::new::` 1 (any_value.rs:12); `walk_expr::` 1 (visit.rs:243); `Project::apply_inline_ignores` 1 (project.rs:784); `clap_builder::builder::value_parser::EnumValueParser` 1 (value_parser.rs:1142); `check_project::run_check_project_command` 1 (check_project.rs:99); `clap_builder::builder::command::Command::try_get_matches_from_mut::` 1 (command.rs:905); `toml_datetime::de::VisitMap::next_key_seed::` 1 (de.rs:74); `file_cache::directory` 1 (file_cache.rs:65); `check_project::prepare` 1 (check_project.rs:91).

**unattributable** (4 sites, 10,987 allocations):
 `-` 7,673; `-` 3,186; `-` 92; `-` 36.
