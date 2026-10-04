# Callgrind drill-down of the 20.57 G Piwigo run, after the 25 G profile's work

Profile: `~/.cache/prs-profile/cl-20g/cg.out-01..10` (ten threads), copied from the scratchpad of the session that built `PROFILE_DRILL_PIWIGO_25G.md`'s designs. Binary `cl-20g/prs-dhat`: the `dhat` profile with frame pointers, code of `5fb858c9` (HEAD's code). Command `check-project --config phpstan-rs.toml --format json` in `/home/torres/piwigo17-frozen`, `PHPSTAN_RS_NO_FILE_CACHE=1`, warm stub cache. Total **20,570,586,331 Ir**. The same directory holds that session's sampling profile (`perf.data`, 3,322 samples) and build log. Percentages are of 20.57 G unless marked.

Nothing was built or run for this report. Everything below is read from these two profiles, from the other session's dhat heap profile of the same code (`target/dhat-heap.json`, §8), from the 25.14 G profile (`cl-final`) and its line-tables twin (`lt`), and from the code at HEAD. Part I compares what was built with the 25 G report's designs, step by step (§2), and drills down the heap (§8). Part II is the plan: twenty items in the order they are built, each validated at its end; §13 outlines the audits that shaped it.

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
| P7 | built otherwise: `2b6aeb84` keeps the `Type` beside its node, made lazily by the old round trip (`parse_written_type(to_source_string())`, `phpdoc/src/doc_type.rs:57`, `:90`) and kept with the memoized docblock, and also keeps a statement's resolved `@var` tags per class and method; `b081be75` and `1892b477` memoize name resolution by node and import-state version (sound: that function reads only namespace and imports) | `phpdoc::lower`, eager lowering, moving the other text-grammar callers, deleting the text grammar | `to_source_string` 238.1 → 83.3 M; `parse_written_type` 118.3 M remains; `resolve_type_node` 118.3 → 94.1 M over the same 34.8 K statement tags | impl.: ≈0.2 G left. The status's "saving gone" rests on 0.18% of 3,322 samples |
| P9 | steps 2-3 (`abd326a2`, `e1e54c29`) | step 1's consumer audit not seen in any commit | with P12: `deps` 841.6 → `recorded_references` 246.7 + the parser's recording | — |
| P10 | generators (`e03e3af6`), lexer facts and `GuardArgs` gate (`c88e1b13`), autoload thread with resolver fixes (`7be53b3c`) | the fact table is a linear scan per new name (`lexer.rs:240`) | `body_contains_yield` 52 → 0; `GuardArgs` 166.6 → 10.3; `prepare` 365.0 → 44.2 on the main thread | — |
| P11 | built otherwise: `c49b68d2` memoizes the scratch `DocBlock` of each tag, keyed by `format!("{tag}\u{1}{rest}")`, and merges it; `3a98ed5e` drops the tag lexer's `Vec<char>` copy: it reads its `&str` by byte offset, still as `char`s with Unicode's classes (item 14 gives it PHP's byte classes) | step 1 (`TagEffect` and `apply`, no scratch block, no merge), step 3 (`Arc` fields), pulling the tag lexer | `apply_tag_recording_errors` 518.2 → 444.8 M; the key's `format!` 54.7 M, `merge_from` 56.2 M, token clones 36.0 M | impl.: step 2 without step 1 |
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
- **Names.** A `Name` is `Arc<NameData>`, and `NameData` holds its text and its lowercase form as two more boxes (`name.rs:22-49`). That is three allocations per name, the lowercase copy made and dropped when it equals the text: at exit, 135,777 `NameData`, as many texts, and ≈84,410 lowercase copies. `Interner::str` keeps 36,952 more `Arc<str>` (9.1 MB).

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
| `Name::to_string` | 677,480 | 15 |
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
| 18, the check phase | 3.61 M | 18.6% |
| 15, names as names | 2.76 M | 14.2% |
| 14, PHPDoc | 2.59 M | 13.3% |
| 4, the registry by ids | 2.02 M | 10.4% |
| 13, a file's products | 1.31 M | 6.7% |
| 16, interned values | 0.78 M | 4.0% |
| 2, the walk and path data | 0.56 M | 2.9% |
| 8, declarations | 0.52 M | 2.7% |
| 12, lexer details | 0.45 M | 2.3% |
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
| F1. A message, subject or identifier is formatted for every element checked, not only when a diagnostic is reported (`check_property_phpdoc_types`, `phpdoc_type_compat.rs:377`; `check_property_declarations`, which also turns each native hint into a `String` to test for `callable`, `property_declarations.rs:133`, `:142`; identifiers built by `format!`, `probe_checks.rs:159`) | 37 | 101,164 | Subjects become values that display themselves, formatted by the report. A built-in rule's identifier is a `&'static str` from its rule's table; `Identifier` stays `Cow<'static, str>` for external rules (item 18). | 18 |
| F2. A constant type is built on every call (`union_of(vec![Type::int(), Type::float()])`, `strict_rules.rs:230`, `operators.rs:57`, `:672`, `:685`; `vec![ty, Type::null()]` for a nullable) | 41 | 33,938 | Named constant types are built once, as `types` already builds its `Constant` nodes (`intern.rs:43`). A two-member union or intersection takes its two operands, with no `Vec`. | 18 |
| F3. An expression is cloned into an `Rc` (the Doctrine and DBAL query-builder chains, `query_builder.rs:161-163`, `sql_reflection/query_builder.rs:336-338`; scope aliases, `narrowing.rs:1313`, `:1399`) | 8 | 58,287 | an `ExprId` into the file's tree | 9 |
| F4. Third-party parsers: `sqlparser` (86.6 K, its tokenizer building a `String` per word and char run) and `regex` (7.7 K, a `Captures` per match) | 112 | 94,265 | A query's text is parsed once per distinct text in the run. Matches are read through one reused `Captures`. | 18 |
| F5. Text where a `Name` exists (`check_internal_instantiation`, `instanceof_outcome`, `static_member_of_class`, the current class as a `String` in `Promotions` and `check_program`, `"Doctrine\ORM\Query".to_string()`, the DQL tokenizer's `String` tokens) | 38 | 102,938 | Item 15's rule; fixed names are interned once. | 15 |
| F6a. A rule collects in order to iterate (`check_type_version_support`'s one-element `Vec`, `type_declarations.rs:182`; `offset_read_type`, `member_resolution.rs:298`; `instance_of_any`, `operators.rs:706`). It rebuilds a constant list per call (`declaration_level_rules`, `internal_rules.rs:56`; `default_include_dirs`, `includes.rs:380`), or keeps a `seen` vector per call (`check_array_literal_duplicate_keys`, `array_ops.rs:70`; `check_function_like_declaration`). | 36 | 143,007 | Borrow (`slice::from_ref`) or iterate. A constant list is a `static` slice, and a per-file one is made once per file. A per-call vector lives in the checker's scratch. | 18 |
| F6b. A registry query walks the hierarchy with a stack and a `seen` set per call (`find_final_ancestor_method`, `hierarchy.rs:917`; `required_abstract_methods`, `:1026`; `has_concrete_implementation`, `:1070`; `class_is_immutable`, `:309`; `has_unresolvable_ancestor`, `:864`). `visible_class_consts` clones every visible constant into a map per query (`resolve.rs:1517`, 4.35 MB). The PHPDoc-inheritance backfill builds per-class vectors of parent maps (`resolve.rs:1147`, 88,501). Attributes are collected per query (`reflection_attribute.rs:63`). | 47 | 149,360 | Item 4's ancestor and owner tables, iterated; levels over ids; slices. | 4 |
| F6c. Throw, exit and loop states: a throw frame is a growing `Vec<(Type, Locals)>` whose element is the whole inline `Locals` struct (`record_throw_state`, `expr.rs:348`: 3,920 growths, 16.1 MB); `check_try` collects such vectors (`try_catch.rs:99`, `:124`, 17.2 MB); every loop pushes break and continue vectors (`control_flow.rs:140-141`) | 23 | 37,839 | `Locals` holds its maps behind one `Rc`, so a frame's element is two words; the frames and per-construct vectors are the checker's scratch stacks (item 18, F6c). | 18 |
| F6d. Narrowing inserts and `Rc<Vec<..>>` implied facts (`narrowing.rs:569`, `control_flow.rs:276`, `:314`, `:336`) | 26 | 29,406 | The radix arrays (item 18, step 3) | 18 |
| F6e. The vectors of separate walks (`bind_targets`, `strict_rules.rs:951`, 20,655; the keyword-scope and `declare` visitors; `section_ends`; the member and variable usage walkers) | 27 | 40,084 | They become families of the one traversal (item 18, step 1) and use its stacks. | 18 |
| F6f. Analyses that are a function of a text, redone at every call: regex groups (`regex_groups.rs`: the pattern as a `Vec<char>` at `:505`, a `String` per literal char at `:936`), `preg` shapes, DQL, SQL query-builder rendering (`query_builder.rs:192-205`), format strings (`sprintf_pieces`, `builtin_calls.rs:3244-3250`; `printf_placeholders`) | 61 | 88,382 | Computed once per distinct text, scanning bytes and keeping slices | 18 |
| F7. The front end and what is kept: sources (59,799: item 3's buffer); one `Arc` per built declaration (28,516: kept); `Checker::new` allocating a default `Arc<LineIndex>` and a key table per file, which are then replaced (`checker.rs:1155`, `:1175`, with the path copies of `set_current_file` and `checker_for`); `if_chain`'s `vec!` per `if` (`declarations.rs:172`, 15,149); heredoc stripping's two vectors (`strings.rs:785`, `:788`); ignore comments and marks as `String`s; `strip_digit_separators`' `replace` (`lexer.rs:1022`); reports | 124 | 125,981 | Items 3, 9, 11, 12 and 13. One checker per worker, reset per file (item 18). The reports and the declarations stay. | several |
| Unattributable: stacks that end inside the allocator | 4 | 10,987 | — | — |

# Part II. The plan

Twenty items, numbered in the order they are built. Each states what is there now, the design, what its validation compares, and what the profiles say it saves, in instructions and in memory. "Exact" means the output cannot change; "semantic" means it can, and §11 lists where. Finding codes (R1-R8, R12 for instructions, M1-M9 for memory) and section numbers refer to Part I.

## 9. How the plan is built and checked

- **Validation at the end of each item (§11).** An item's rows are committed one by one, with `cargo fmt`, `cargo check` and clippy, and nothing else runs between them. When its last row lands, the item is validated: the tests, the fixtures, item 0's dumps and O2, compared with the previous item's, and the equalities. The next item starts after that. O3 measures once, after item 19.
- **Oracles are dumps.** Item 0 takes dumps of everything the items change, from the starting commit, in a form that does not depend on how the code represents it. §11 takes the same dumps at the end of each item and compares them with the previous item's, so each difference belongs to the item that made it. So no item keeps an old implementation alive to compare against, and each replaces what it replaces outright.
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
  - the property-write scan (`property_writes.rs:426-433`, `:619-620`), deleted with its module by row 0.1b, whose effects replace the verdict it checked;
  - the assigned-properties builder (`summaries.rs:115-121`, `:188-201`);
  - the uninitialized-property check's body path (`uninitialized.rs:133-141`);
  - the function bodies `FileBodies` stores for them (`bodies.rs:110-111`, `project.rs:216-221`);
  - the gated body walk against the ungated one (`bodies.rs:75-81`);
  - the reference collector's comparison (`project.rs:2841-2862`), with `deps::declaration_referenced_names` (`deps.rs:105`) and `symmetric_difference` (`project.rs:2865`), which only it uses;
  - the declared classes against their own walk (`project.rs:3029-3036`);
  - the declaration rebuild checks (`project.rs:1248-1262`, `:1615-1636`), with `Keep` and the `keep` parameters of `build_declarations_where` and `built_declaration` (`index.rs:661-687`), which only the second uses;
  - the gated walk's declaration count (`file_declarations.rs:122-131`);
  - and, as a test, `the_one_walk_for_both_sets_is_the_two_walks` (`deps/tests/dependency_collection.rs:228`), with `deps::declaration_referenced_names`, which only the oracle and it use. `project/tests/fused_declarations.rs` (the five walks `parse_file` replaced) is no oracle: the edit path (`update_file`) runs those walks in release, so the test checks two live implementations against each other, and it goes with the walks in item 1.
  
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
1. **The starting state is read first.** At HEAD, each report is compared with the last state it was validated at, and each difference is read: the vendor-tree report with `1892b477`'s, and the all-on and unscoped reports with `7a3f3f99`'s, the 25 G work's baseline, since level 7 compared only the scoped run and the vendor tree. The six changes listed above were all built before `1892b477`, so they show only against `7a3f3f99`. Anything wrong is fixed in its own commit before anything else.
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
     - doc sites, each with its `DocBlock`, every field printed canonically, and the references sorted;
     - every type node of every docblock, with the type today's round trip gives it (`parse_written_type(to_source_string())`), printed canonically and keyed by file, doc span, tag ordinal and node path, for item 14's lowering oracle;
     - every union the run builds, with its operands and its result, printed canonically, for item 18's union pass: one set for the run, not per file, since a memo (a docblock's, a type's) builds a union in whichever file needs it first, and recorded where a union is asked for, above the union memo, so the set does not depend on which thread built it;
     - each built declaration in order, with its guard and full reflection;
     - duplicate sites, declared classes, Pest facts, anonymous-class names and use sites;
     - each body's summary and `InitFlow`, and the global facts.
     
     Its file set includes every extension PHP is run from (`.inc`, `.module`, `.install`, `.theme`, `.engine`, `.profile`, `.php.inc`) and any file that starts with `<?php` or `<?=`. Each file's text is stored gzip-compressed (`flate2`) under its BLAKE3 digest, with an index of digests by role and path.
   - **`registry-dump`**, taken after the finishing passes and before checking. It only reads: lazy records are decoded, but deferred values are printed as stored. It sorts by kind, folded name and load order, and prints:
     - every project declaration, at the configured PHP range and at its two ends. The registry prints as its difference from the stub base it stands on, which it names: for each table, map and set, the entries the project adds or changes and the names it drops. The difference depends only on what the two hold, not on how layers are stored, so item 4's ids leave it as it is; an entry read through to the base is the base's own, so most of the comparison is a pointer test;
     - the stub bases, each whole, in a dump of their own (`stub-bases`), taken in every cache state of one configuration and compared across them, so a base built cold and one read back from the stub cache must print the same;
     - for each, its reflection, its guard and the guard's value, its purity dependencies and verdict, its effect (what it may write, by storage, which replaced the property-write classification in row 0.1b), its ancestors and owners, and the ancestry fact;
     - both scopes, the global constants and the global-variable facts;
     - both reverse indexes, the use-site index and the duplicate-site tables;
     - the file table: path, role, load order, absolute path, Composer owner and `mentions_phpstan`.
   - **The checker's records:**
     - every PHPDoc resolution, keyed by file, host class and function, the reader that resolves it (an inline `@var`, a hook's `@throws`, a `@template` bound, ...) and the written type: the checker reads a docblock's types as text, without the docblock's span or the tag's ordinal (`Stmt::doc_comment` and the declarations' doc comments are strings), so a resolution is known by what is resolved, where and by which reader;
     - every expression's inferred and native type in the final pass, keyed by file, span and host class (`--dump-expr-types`, beside today's `--dump-types`).
   - **O2: the gate's full artifact set** (`cargo xtask corpus --full`: scoped, all-on, unscoped, levels 0-10, the type dumps, the baselines and the emission-order check). It is one pass with the dumps above, not a run of its own beside them:
     - **one analysis per configuration and state writes every artifact:** the runner (`tools/gate-runner`) loads once, writes the front end's dump, the registry dump and the stub bases, checks once with the records on, and writes the report and its views. So each artifact is compared across every state the configuration is run in, not the report alone;
     - **the runs go on a worker per core** from one queue, the corpus's first and at most three of them at once for memory, a replay's two runs one after the other in its copy, and every difference is listed at the end;
     - **the fixture suite runs once,** in this pass, with its type and diagnostic records; the tests step runs the rest;
     - **the level runs stay:** a level changes the analysis, not only which reports print, so no level's report is a filter of another's;
     - **the `oracle` build runs the same pass,** in every state, the per-file cache written and read included, recording nothing. The statements that once kept it off the cache went with the debug oracles (row 0.3), and the one assertion only a debug build makes is the cache's round trip (`file_cache.rs:228-236`), which runs only where the cache is written.
     - It adds a view of every field of every diagnostic: the end position, the UTF-16 columns and the whole fix.
     - It runs over the Piwigo configurations, the vendor tree as one project, each vendor project with every rule set, and the fixtures. A vendor project is each `vendor/<vendor>/<package>` of the corpus with a `composer.json` (205), analysed alone with `--no-composer`.
     - It adds a configuration with architecture rules and banned types, which the all-on configuration does not set (`xtask/src/main.rs:394-461`), so that item 19's passes have a corpus oracle from the baseline on.
     - It runs the scoped, all-on and unscoped configurations with the stub cache cold and warm, the per-file cache off, written and read, and at one thread, each from a cache directory of its own, and the others at the default thread count and at one. Every state must give the same artifacts, all of them.
     - It is compared in output order with `xtask compare`.
   - **The server:** an in-process session over Piwigo and two vendor projects. It records:
     - the loaded files with their roles, and `registry-dump`;
     - diagnostics with UTF-16 ranges, and code actions;
     - definition, hover and references over a seeded sample, context files included.
   - **Edit replay**, for Piwigo and each vendor project, as PHPStan's result-cache end-to-end tests do (`e2e-tests.yml:38-46`):
     - a fixed script runs over a seeded sample of files: save one unchanged, delete a method, rename a class, change a parameter type, add a file declaring a class that is named but declared nowhere, delete a file;
     - each step goes through `update_file` or `remove_file`, and the project dump, `registry-dump` and every file's diagnostics are taken after it;
     - at the end, a fresh load of the edited tree must give the same.
   - **Memory:** the dhat profile of §8, by holder, with the heap recorded at the start of the check and at the check's own maximum, beside the global peak. It is taken once more with untrimmed backtraces (`dhat::Profiler::builder().trim_backtraces(None)`), so that the 10,987 allocations whose stacks end inside the allocator (M10) are placed too.
4. **The baseline** comes from item 0's last commit, once its validation (0.11) passes. The printers and the deletions leave release behaviour as it was, every deletion being under `cfg(debug_assertions)` or `cfg(test)`, so the baseline's reports differ from `5fb858c9`'s by item 0's semantic changes alone (§11), each difference read.
   - Each dump is taken with the `gate` build in every state of its configuration (above), at least at `PHPSTAN_RS_THREADS=1` and at the default thread count, which must give byte-identical output. The pass is taken again with the `oracle` build, which must finish without an assertion.
   - The ordered file list, with roles, is recorded for each configuration. A tree with a link cycle is recorded as it is today: listed until `ELOOP`.
   - Everything is stored with the starting binaries: the `gate` build, `cl-20g`'s and the `dhat` build. None stays in `target/`, which `cargo clean` empties, or in `/tmp`, a tmpfs: binaries go to `.corpus-runs/gate/bin/<commit>/<profile>/`, runs and dumps to `.corpus-runs/gate/<artifact>/`, profiles to `~/.cache/prs-profile/<run>/` with their binary and a `README` (`.corpus-runs/README.md`).

**Found in step 1.** Read at `5fb858c9`'s code, with each report saved under `.corpus-runs/gate/{vendor-tree,all-on,unscoped}/` for `1892b477`, `7a3f3f99` and `5fb858c9`.
- **Against `1892b477`:** the all-on report is identical; the vendor tree and the unscoped report differ in one line each, the same one: `OrderedTraitsFixer.php:122` is no longer reported unreachable. Correct: PHP runs it. An `assert` in a loop failed on the first pass, while the variable was still `null`, and `1892b477` kept that span as failing; `b99855fd` deleted the set of failing spans.
- **Against `7a3f3f99`,** the vendor tree and the unscoped report differ by the same 212 removed and 31 added lines, and the all-on report by 1 and 2:
  - 187 `missingType.parameter` gone and 24 `missingType.iterableValue` added, all in Symfony's `RedisClusterProxy`: its methods, spelled in lower case, override the stub's (`zcount` over `zCount`), and PHP's method names ignore case, so they inherit the stub's parameter types now (`712cd30c`). Correct.
  - 15 `return.missing` gone: each function ends in a loop with no `break` (`do { … } while (true)`), which never ends normally. Correct.
  - three gone at `LogicalXor.php:64`: PHP reads `fn (…) => $a xor $b` with the `xor` inside the arrow function's body, as the Pratt parser now does. Correct; the one of the six changes that shows in these reports.
  - `goto` (`8386baff`): `Http1Parser.php:207`'s `nullsafe.neverNull` and `DeepClone.php:794`, `:827`'s `variable.undefined` gone, since a label is reached with the scope of its `goto`s. Correct.
  - `doctrine/orm` `Parser.php:840`'s `return.type` gone: the `switch`'s `default` calls a `never` method. Correct.
  - three messages whose unions list the same members in another order (`8c309067`, a union's members in order of first occurrence): `AppReference.php:180`, `NoPhp4ConstructorFixer.php:269`, and in the all-on report `triggers_list.php:1094` (22 members each).
  - `deadCode.unreachable` added at `LockRegistry.php:175`: correct, the outer `while (true)` has no `break`.
  - four false reports: `Coverage.php:195` in `nunomaduro/collision` and in `pestphp/pest` (F3 below), `DriverSuspension.php:150` (F2), `LogicalXor.php:68`'s `function.alreadyNarrowedType` (F4), and in the all-on report `ThemesStandardPagesPageRendererTest.php:339`'s `return.neverCandidate` (F1), whose cause also gives the `deadCode.unreachable` at `:381` both states report.
- **The six changes listed above:** only the Pratt parser's changes a diagnostic in these reports. `__halt_compiler`, kind-exact keys, hyphenated words and declarations ruled out by version change none, and the server's columns are not in a CLI report; item 0's dumps (O1, the server's) show them.

**The four bugs,** each a fact kept across code that changes it, and each a false report:
- **F1. Shared storage.** A narrowing of storage other code can write survives a call that writes it: a static property (`ThemesStandardPagesLogoStreamWrapper::$opens` narrowed to `0`, then `render()` increments it through the stream wrapper `fopen` runs), a variable bound by `global`, a function's `static` variable (written by a recursive call), and a variable bound by reference to any of these.
- **F2. Calls that may write anything.** A call whose writes are not known forgets only its receiver's and its object arguments' property narrowings: `($this->run)()` leaves `$this->pending` at `true`. A call known to write its own object forgets the receiver's, though another variable may hold the same object.
- **F3. Captures by reference.** Inside a closure, a variable captured by reference starts with its type at the capture, though the closure's earlier calls and the outer function may have written it since: `$shouldBeNewLine` is `true` at the capture, the closure sets it `false`, and its next call reads `false`.
- **F4. `array_reduce`.** Its type is the callback's return type even when the array may be empty, where PHP returns the initial value; and `$carry` is bound to the initial value's type only, though from the second element on it holds the callback's return.

**The fix: what code may write, by storage, sound, bounded by the class hierarchy** (the user's decision, over keeping the receiver-and-arguments rule, then made field- and type-sensitive):
1. **The premise:** the program is what the run loads. This is the closed world the whole-program checks already assume (dead code, unused members, sealed hierarchies). Everything below is sound for that program and keeps every possibility it leaves open.
2. **An effect is a set of storages written, or anything.** The storages:
   - an object property: the property, on objects that may be of the class declaring it, with a write to the callee's own `$this` kept apart, so that a call site can place it on its receiver;
   - a static property: its storage, the class that declares it as reached from the class named. `static::$p` reaches every storage of `$p` over the class and its descendants. A property a trait gives is stored per using class.
   - a global variable, by name;
   - a declaration's `static` variable. A method's is shared with the classes that inherit it without overriding from PHP 8.1, and is per class before, so it follows the configured versions;
   - a variable captured by reference, as a storage of the frame that captured it.
3. **The summary's one walk records the writes,** together with their base expressions:
   - every write form: assignment, compound, `++`/`--`, `unset`, a write into an element or a nested property, `=&`, `foreach (… as &$v)`;
   - a storage passed where a parameter is by reference;
   - a variable bound by `global`, by `static` or by reference to a storage;
   - a dynamic form (`$o->$p`, `$c::$p`, `X::$$n`, `$GLOBALS[$k]`, `$$n`, `extract`, a one-argument `parse_str`) writes every storage of its kind its base may reach;
   - a closure's or arrow function's body counts for the declaration that holds it.
4. **The fixed point settles every declaration's effect over the call graph.**
   - A call to an overridable method, or through an interface, reaches every loaded implementation: the receiver type's own and every descendant's, by `subtype_children`.
   - A callable whose target is not known writes anything; a closure, an arrow function, a first-class callable or a function named by a literal writes what its target writes.
   - A builtin writes exactly what its documented behaviour writes:
     - nothing, by default;
     - the storage passed at a by-reference parameter;
     - what the code passed as a callback writes;
     - what the methods it calls on an argument write, over the argument's type by the hierarchy: `count` and `Countable::count`, `json_encode` and `jsonSerialize`, iteration, string conversion, the serialization hooks;
     - for a stream function, what the wrapper methods it calls write (`fopen` calls `stream_open`, `file_exists` calls `url_stat`, …), on every class the program registers with `stream_wrapper_register`, or on every loaded class with that method when a registration's class is not known. Which methods each function calls was found by running every PHP function against a wrapper that records its calls, in `php` 8.5; the PHP catalog, found in item 0's validation, derives it from php-src per version instead, the recording becoming its oracle.
5. **The checker forgets what an effect may have changed,** with every key that depends on it:
   - object properties: the facts about property `p` (its narrowing and every key below it) on each value whose type may be an object the write reaches. That is the receiver and the values that may alias it for a write to the callee's own `$this`, and every value whose type may be of the declaring class otherwise. An object the function created and never let escape keeps its facts: not passed, stored, captured, returned, yielded or bound by reference since its `new`.
   - static properties by storage; variables bound to a written global, every variable at a file's top level counting as global; the function's static variables when the call may enter the function again; variables captured by reference that the call may write; and variables bound by reference to any written storage.
6. **F3:** a variable captured by reference is entered with the union of its type at the capture and of every type a write gives it while the reference is shared. The closure's body is checked until that union stops growing, as a loop body is, each pass starting from what it held where the body returned as well as where it ended. The enclosing scope is checked again when it holds a type in the variable, after any statement, that the closure was not entered with. At a file's top level, where checking again would check every declaration again, a capture the scope assigns after the closure is made is entered as anything. The closures checked with a bound `$this` keep their own by-reference state, as the others do.
7. **F4:** `array_reduce` follows its stub's generics: `$carry` is the initial value's type joined with the callback's return type, until that stops growing. The result is the callback's return type for an array that is never empty, the initial value's for one that is always empty, and their union otherwise.
8. **Code PHP runs without a call written** (magic methods and property hooks on member access, lazy objects' initializers, destructors and the cycle collector, autoloaders, error handlers, output-buffer callbacks, tick and signal handlers, included files) is built in item 0 too, after the PHP catalog, with the same effects ("Code PHP runs without a call written", below).

**Oracle for the fix:** fixtures for each storage and binding kind (an inherited, a redeclared and a trait's static property; `static::`; a method's static variable on PHP 7.4 and 8.1), each call kind (a virtual call with two implementations, one writing; an interface call; a callback; a builtin calling an argument's method; a stream wrapper, registered and not), field sensitivity (a call writing another property keeps this one), an escaped and an unescaped new object, a capture written later by the outer function, and `array_reduce` over an empty, a non-empty and a possibly empty array, each checked against `php` (7.4 through `docker` for the static variables). In item 0's validation (0.11) the reports are compared with `5fb858c9`'s: the five false reports above go, and every other difference is read.

**Found in step 2.** The oracle build's run (`.corpus-runs/gate/oracle-runs/`) reached every oracle in its 208 check runs but the two declaration rebuild checks, which only the edit path reaches, and failed twice:
- **A union of more than 64 class-strings was `mixed`.** No fold past the union cap took `class-string<T>` members, so the 65 keys of php-parser's precedence map (`PrettyPrinterAbstract`) survived every fold, and the union's debug assertion that only extension types outlast them failed (`union.rs:319`). Distinct callable signatures would survive them too. Past the cap, class-strings now fold as instances do, to `class-string` of the classes' common supertype, or of `object` without the hierarchy (`string` when one names no class), and callables to `callable` (`21597ad7`).
- **The edit replay changed a project built once.** It loaded through the batch session, which keeps no declarations as built and drops context trees, so editing the project failed `begin_declaration_changes`' assertion. It now loads the project as the server keeps it: declarations as built, every tree, no per-file cache (`cf03d4e7`).

**Found in item 0's validation.** The fixture suite failed 19 fixtures, the new call-effect ones and three older ones, for four bugs in the fixes of step 1, each fixed in its own commit:
- **A written place kept its own fact** (`4ca80506`). The checker purged the facts resting on a written place but not the place's own, so a read after the call still gave the value written before it.
- **Every unprobed stub wrote anything** (`0342898d`). A stub no run of PHP has seen, such as PHPStan's `assertType`, was taken to write anything, so each `assertType` call forgot every fact. Such a stub is now taken at its word: one its doc declares pure (`@phpstan-pure`, `@psalm-pure`, `@pure`, `@phan-side-effect-free`) and that takes no code to run writes nothing. The signature keeps its declared purity.
- **A function's scope had its short name** (`6181696f`), where its static variables' storage has the declared one, with the namespace, so a recursive call's writes were not found.
- **The effects read a stale class index** (`a7363772`). The finishing passes indexed the class hierarchy after resolving purity and effects, so the effects of a call through `static::`, and every bound by descendants, read the index of the load before, or an empty one.

The edit replay found four ways an edit left a file's diagnostics stale, each a file it did not recheck, each fixed in its own commit with a test that holds diagnostics as the server does (`4df4f67c`):
- **Traits** (`cbe3da2e`). A trait is checked as each class using it, so a changed, added or removed class leaves its traits' files stale. An edit now rechecks the files of the traits such a class uses, before the edit and after it.
- **Duplicate declarations** (`1bf9c6a0`). A duplicate's report lists every declaring file. An edit now rechecks the other files declaring a function or a class the edited or removed file declared or declares.
- **`@see` targets** (`29ee128b`). The class an `@see` tag names was no reference of its file, so declaring, renaming or removing it left the tag's report stale. The tag's grammar is now one function (`docblock::see_reference`), read by the check and by the references alike; the class is a reference, but not a declaration reference, since it shapes no declaration.
- **Included paths** (`2db7524c`). The include check probes the filesystem itself, so a file appearing or vanishing where an `include` looked changed no recheck. A check now keeps the places its includes looked, and a file added or removed there rechecks it. The server also watches every other file and directory for creation and deletion, and rechecks the files that looked there.

It also found the project's state after edits apart from a fresh load's, though no diagnostic showed it:
- **Use sites** (`1225f83a`). A recheck dropped a file's use sites and indexed them again only when the source was kept, so a project checking dead code without it read its edited files as referencing nothing.
- **Assert-tagged names** (`dd80910b`). Each declaration added its names as it was inserted, and none left; stubs merged in added none. The names are now indexed from the resolved declarations after the finishing passes, the base's kept as it was frozen.
- **Orders** (`f76fceb3`). The use-site lists, references, a rename's edits and the classes declaring a method name came in the order files were indexed or declared in.
- **A loaded file's length** (`2648ab5e`). A file the session loads had the length of the text it kept, none unless the source was kept, so a recheck weighed every file as empty.
- **Load order** (`1ff368c4`). An edited file keeps its place in the load order and an added one comes last, where a fresh load numbers files as it finds them. The dump now prints files by path with their load order, and the replay calls a section that differs in nothing else `differs only in load order`. Item 1 gives an added file the place a fresh load would.

The replay over Piwigo and all 205 vendor projects then left every file's diagnostics as a fresh load's, and found eight registries apart from it:
- **Lazy tables** (`48812e7b`). The Doctrine entity index and the builtin methods running code were filled when a check first asked, so a dump held them or not by which checks ran, and of two entities on one table the first inserted won. Both are now made in the finishing passes, the entity first by name winning.
- **Duplicate sites** (`69e85680`) printed in load order; they print as sets, as both checks sort them.
- **Effects read doc types PHP never enforces** (`aaad64ab`). Purity and the effects ran before the pass that drops a doc type the native type excludes, so a load resolved `$this->cache->get()` through `@param Cache` on a property declared `int`, and a later finish, with the class out of scope, through `int`. The declared types now settle first.
- **A scoped class copied a finished trait** (`cdbf02a5`). A finish resets only the classes in scope, so a class in scope copied an unscoped trait's finished method, its `@return $this` already dropped to the native interface in the trait, and took the other trait's method of a name two traits declare. The scope now takes in every class a scoped class copies from: what it extends, implements and uses, its mixins and the classes it imports types from.
- **Switching PHPStan's stubs off** (`03d99f6d`) kept the PHPStan base's built-in stub classes, stub-provided functions and names; the project now takes those of the base it stands on.
- **An added file under `vendor/`** is context on the edit path, where the load analyses it: a new file's role comes from directory names, which item 1's `Session::classify` replaces.

The runtime oracle, run with every PHP version's image, found the one PHP 7.0 fixture unrun: its assertion was in an uncalled function, and the shim threw on 7.0 (`void` returns, `JSON_INVALID_UTF8_SUBSTITUTE`). The shim now runs from 7.0 (`34e9fc48`), and running the fixture showed a negative string offset typed `string` before 7.1, where PHP 7.0 reads `''` with a notice and writes nothing with a warning. Before 7.1 such a read is now `''` and reported, such a write leaves the string and is reported illegal, and on a range across 7.1 either holds (`2fbacd93`).

Two gaps turned up with it, each closed in item 0:
- **Syntax a configured version lacks went unreported.** It is now reported:
  - the type, declaration and expression forms from PHP 7.1 to 8.5;
  - the forms the tree does not keep, which the lexer and parser record in the file's facts.
  
  Commits: `492ccf0e`, `50fa1e43`, `d8090e5e`. A fixture's `php_version` also takes a range now, and the runtime oracle runs a ranged fixture at both ends (`dfdfeeb4`).
- **The runtime oracle ran 81 fixtures.** An assertion in a function nothing calls never ran. The oracle now calls each function and method of an asserting fixture with values of its parameters' declared types: literals the relation admits, and objects, enum cases, class-strings, closures, object shapes and arrays built from the type (`gate::samples`, `3d8085f3`). That makes 353 fixtures run. A fixture whose assertions rest on what PHP never checks (inline `@var`, docs-only declarations, assertion functions with empty bodies) opts out with its reason.

The calls found three places where a type assumes what PHP does not give: an int operation with an open end taken not to overflow, a general array's key taken to be there, and an iterator's `current()` taken to give a value. They are built in item 0 ("What PHP gives where a type assumed", below); until then, their seven fixtures opt out of the calls. The 114 asserting fixtures the calls still do not reach are queued in `WORK_STACK.md`.

The dumps at one thread and at the default found two states that depended on the caches, though no diagnostic did, since the first run filled the caches the second read:
- **The stub cache lost which classes have PHPStan ancestry** (`f80df7a4`). The table was not among those it stores, so every warm run's base had none, and the rules gated on it saw no PHPStan class among the stubs'. The round-trip test now compares the whole print of a freshly built base, plain and with PHPStan's source, with what reads back.
- **A context file kept its ignore comments only when parsed** (`00ac7137`), not when read from the per-file cache, whose entries hold none. Only an analysed file's are read, so a context file of a batch run now keeps none.

Running the validation showed its own overlap:
- **The registry dumps printed the stub base whole, every time.** Each table printed joined with its base, and the batch dump printed both bases again after it. A small vendor project's dump was 2.78 M lines, 18 K of them its own, taken eight times per replay and twice per project: the vendor replays were 65% of the dumps' time.
- **The same analysis ran in many processes.** Each configuration was analysed once per dump kind, then again for the report and for each cache state of the equalities, which compared the report alone. So a state that changed a dump and not the report went unseen, as the two above did until the dumps' second run read the caches the first had filled.
- **The fixture suite ran twice,** in the tests and in O2.

Step 3's one pass replaces them. The dumps' runs also share the machine now: a worker per core from one queue (`df6a29ed`), and `cargo xtask progress` shows a pass as it runs (`cbd2a397`). Each checkout keeps its own `.corpus-runs/gate`, where a linked worktree wrote into the main checkout's and shared its scratch (`e58fadf4`).

**Fast validation.** Most of the pass's runs are the 206 edit replays, two each, and the 205 vendor projects. Callgrind of one of each, `vendor/amphp/amp` (26 files), as the pass runs it, warm (`~/.cache/prs-profile/cl-pass-runs-dc4da9a2`, with its binaries, script and README):
- **A vendor project's run: 393 M instructions, 0.53 s, 336 MB.** 283 M of the main thread's 300 M are the whole-program passes walking the stub base:
  - `find_dead_functions` takes every function's name (`dead_code.rs:50-62`) and asks each unreferenced one for its declaration site, which decodes 5,256 stub functions to find none (`function_declaration`, 47 M; 11,519 function decodes, 121 M);
  - `find_dead_classes`, dead members (`dead_members.rs:200`), dead enum cases (`dead_enum_cases.rs:54`), dead abstract classes (`dead_abstract_classes.rs:37`) and the namespace cycles (`circular_namespace_deps.rs:57`) iterate `registry.classes`, which decodes the 1,520 stub classes (49 M) and asks each for its ancestors (10,576 `all_ancestor_names`, 9,298 `is_subclass_of`);
  - inside those decodes, 121,000 name internings (33 M) and 74,000 type internings (25 M).
  
  The effects take 45 M of a finish (below). The project's own parse and check are 40 M.
- **A replay: 1.98 G instructions, 0.33 s, 129 MB.** The main thread's 1.66 G are mostly the dumps:
  - the registry dumps, 871 M over ten: the difference from the base iterates every base entry of every table, set and map, which decodes the base once per table (3,058 class and 5,256 function decodes) and looks up each name (154,000 `find_by`);
  - gzip at the default level, 628 M: eight 1.1 MB dumps, compressed in each run, the one-thread run's only to be compared and deleted;
  - the canonical printer quotes and writes names a character at a time (`quote_bytes` 39.5 M, `write_name` 17.5 M);
  - the work itself: rechecks 78 M, `update_file` 58 M, and five finishes of about 45 M each on other threads.
- **Every finish resolves the effects through every stub subclass.** For a call of a builtin method, `builtin_runs` looks for overrides on the receiver and every subclass (`effects.rs:1033-1044`), stub classes included, which declare no body: 23,232 `lookup_method_with_owner` per finish (21 M of its 29 M of classification).
- **Every process parses the builtin-runs table** (`builtin-runs-8.5.tsv`, `effects.rs:320-323`): 9.8 M, in each of the pass's ≈ 1,000 processes.

The design. Each step is exact unless it says otherwise:
1. **The whole-program passes take their candidates from the analysed files.** A whole-program report is placed only in an analysed file (`whole_program_diagnostic`, `project.rs:2696-2719`). Today each pass walks every name the registry holds, stub and context ones included, decoding each reflection.
   - **Candidates:** each pass iterates the names the analysed files declare and reads each one's resolved declaration, its members as resolved, a trait's included, as today.
   - **Relations:** these still come from every loaded file: the references from the use-site index, and ancestors and descendants from the hierarchy's indexes, read on demand. Dead abstract classes ask the descendants index whether a concrete class descends from each candidate (`dead_abstract_classes.rs:35-54`), where today they walk every class for witnesses.
   - **Semantic for a name declared twice.** A function or class two analysed files declare, such as a polyfill under two guards, is reported at each declaration that nothing references, where today it is reported once, at the site the registry keeps (§11, item 0).
   - **The namespace cycles: a bug fixed.** The graph keeps every project file's classes, context included, since a cycle can pass through vendor code; a stub cannot depend on a project namespace, so none closes a cycle with one. Today a cycle is reported at the alphabetically first class contributing an edge (`circular_namespace_deps.rs:50-109`), and when that class is a context or stub class the report finds no site and the cycle goes unreported, though analysed classes are on it. It is reported at the first analysed class contributing an edge, and a cycle with none is vendor code's, not reported (§11, item 0).
2. **A builtin method's overrides come from the classes that declare the method with a body.** Today a call of a builtin method looks each of the builtin's methods up in the receiver and every subclass, stub classes included (`effects.rs:1033-1044`), though a stub class declares no body. It reads instead, for each method, the method as the receiver's class resolves it, which may be an ancestor's between it and the builtin, and each class below the receiver that declares it with a body. Those come from `methods_by_name`, already kept, which holds a trait's methods in each class using it. The rule itself is unchanged; what PHP calls on a builtin's own object is the PHP catalog's (below).
3. **The builtin facts are built at compile time.** Every process parses the builtin-runs table today. The PHP catalog (below) replaces the table in this item, so `build.rs` turns the catalog, not the table, into static sorted arrays, read with no parse and no allocation. A malformed entry fails the build, where the table's parse skips a malformed line today (`effects.rs:336-340`, `:362-365`, `:371`). Nothing is built for the table in between.
4. **The registry dump's difference from the base costs the project's own entries.** When a table, map or set reads from the base's very storage, its difference is its own entries that differ from the base's, and the hidden names it does not replace. The full comparison stays for one that does not. A test prints every table of a loaded project both ways and requires the same text.
5. **Dumps are streamed as text, and only the pass compresses.** The canonical printer writes into an `io::Write` sink, where today it builds the whole text first; the scoped registry dump passed 1.2 GB. A run writes its dumps as text with each entity's digest computed while it writes (the store's object id, below), and the replay compares the digests, then the texts only where they differ. No run compresses anything, the front end's texts included, which its dump gzips one by one today. Each run writes a manifest of digests per entity: each file, class, function and table entry of a dump, and each dump whole. The pass compares the manifests of a configuration's states and of a replay's two runs, and reads texts only where digests differ, so a difference is located at its entity before any text is read.
   - **The store is a git repository of its own** (`.corpus-runs/gate/store.git`), fed by `git fast-import`: each entity's text is a blob, a dump is a tree of them, and a validated commit's snapshot is a commit of the store under a ref named by its hash. Git holds each text once, deltas and compresses what is alike, and diffs trees by their entries, so an exact item's validation adds a commit and almost no objects, where today each validation would store gigabytes again. `compare` is `git diff` between two snapshots: the entities that differ, then their texts, which is how §11 reads an item's differences. The front end's text pool moves into it.
   - **One digest: the store's own object id.** The store uses git's SHA-256 object format, and a run computes each entity's blob id itself (SHA-256 over git's blob header and the text) as it writes. A manifest is then a tree's entries, and the import writes only the blobs the store lacks, where a separate digest would hash every text twice.
   - **Only what can differ is written.** The first state of a configuration, and the default run of a replay, write every entity's text. A later state's run, and the one-thread run, are given that manifest and write the text of an entity only where its id differs, which is all a difference needs. The pass's scratch then holds one copy of each dump, where every state would write its own: the scoped registry dump alone passed a gigabyte.
   - **The existing snapshots are imported** into the store, each under its commit's ref, `5fb858c9`'s reports and the earlier items' snapshots included, so every comparison goes through the store; the plain files go.
6. **The canonical printer copies runs of plain bytes** where it quotes text and writes names, instead of a character at a time.
7. **The edit replay checks every step against a fresh load** of the tree as the step leaves it, where today it checks the last step only. A divergence a later step hides is then found at the step that made it.
   - **The whole-program diagnostics are compared too,** at every step: the dead code, the namespace cycles, the Composer and mock hygiene, the duplicate declarations, on the edited project against the fresh one. Today the replay compares each file's diagnostics and the dumps only, so a stale use-site index (`1225f83a`) showed in the registry dump and in no diagnostic the replay compares.
   - **The script exercises every dependency the edit path tracks,** each where the project has one, drawn by the seed: a signature, a body alone, a doc type, a constant's value, a class a trait's user is checked as, a declaration declared twice, an `@see` target, an include's target appearing and vanishing, a parse error made and mended, a file renamed. Today's six edits miss most of these, and each stale-diagnostic bug item 0's validation found was one of them.
   - **Then a seeded sequence of those edits at random,** each step checked the same way, as the unit tests already do over the fixtures (`random_edits_match_a_fresh_load`), so that orders the script does not try are tried too.
8. **The edit path rechecks its dependents in one check.** `recheck` checks the dependents one after another on the calling thread (`project.rs:2324-2341`; 52 M of `update_file`'s 58 M in the replay). Each check goes through `check_files` for its one file, which judges the private property types again for that file alone, checking once more the files declaring its classes for their writes. The dependents go to one `check_files` over the worker pool instead, as the load's check does, which judges the property types once for all of them; their use sites are then reindexed in one pass, which is what needs the project mutably. The server's latency gains as much.
9. **Measured again,** once the catalog below has landed: instructions and peak memory, and the whole pass's wall time. The script becomes a command, `cargo xtask profile <kind> [<project>]`, which builds the frame-pointer `dhat` binaries, warms a cache of its own, runs callgrind the way `CLAUDE.md` prescribes and writes the profile with its binaries and a `README` to `~/.cache/prs-profile/`, so that step 10, O3 and every later measurement take the same runs the same way.
10. **Every other kind of run the pass makes is profiled the same way,** a configuration's state, a level, a session and the fixture suite, and planned from what it shows, so the whole pass is fast, not only its two commonest runs.
11. **An item's validation is one command.** `cargo xtask validate` runs §11 whole on the worker pool, with its progress: the `gate` and `oracle` builds, the workspace tests and the item's, the runtime oracle on every version image, the pass with each build, and the comparison with the previous item's snapshot. It writes the summary the ledger's V row records, where today each of these is started by hand, one after another.
12. **The runtime oracle reaches every asserting fixture.** 114 still never reach an assertion (`WORK_STACK.md`), in four kinds, each closed:
    - **an assertion in a closure nothing invokes:** the generated calls invoke each closure and arrow function the fixture makes, with values of its parameters' types, as they call functions;
    - **code that throws first with the values sampled:** a call that throws is made again with the pool's other values until one reaches the assertion, and the fixture records which did;
    - **a parameter no value can be made for:** the sampler makes one for every type it meets, an interface with no implementation by an anonymous class implementing it, and a resource by opening one of its kind;
    - **a trait's method:** called through an anonymous class using the trait.
    
    A fixture that still reaches no assertion fails the oracle, unless it opts out with its reason, as `runtime_calls = false` does.

Expected: a vendor project's run near 70 M instructions and well under its 336 MB peak, most of which is the decoded stub base (the replay, which decodes little of it, peaks at 129 MB). A replay, with a fresh load after every step, near 0.4 G, from 1.98 G.

**The PHP catalog, from PHP's source.** The builtin-runs table is a recording: what PHP called on the inputs the recorder chose (`tools/builtin-runs/README.md`). A builtin it called without seeing a run is taken to run nothing, so a call PHP makes only on another path is missing, and the table narrows effects without proof. It is also 8.5's alone, serving every configured version. It records nothing of what a builtin calls on its own object, where the effects take every override instead, nor what a builtin writes besides its by-reference parameters. And what the language's own operations run (a property read reaching `__get` or a hook, a release reaching a destructor, a warning reaching the error handler) would be decided by hand, rule by rule.
1. **The catalog: everything a builtin or an operation may do that user code observes.** For each PHP version, a tool reads php-src's implementation of:
   - every builtin function and method (`PHP_FUNCTION`, `ZEND_METHOD` and the handlers they reach);
   - every operation the engine runs for a construct: each opcode's handler, in every specialization, with the operator, conversion, object-handler and class-lookup functions it reaches.
   
   For each, it derives:
   - **the user code it may run,** on each argument or operand and on its own object: the methods it may call by name, the operations that reach user methods (string conversion, iteration, array access, counting, comparison, serialization, property access through the magic methods and the property hooks, a lazy object's initialization), the callables it may invoke, and the stream-wrapper and stream-filter methods a URL or a stream reaches;
   - **the user storages it may write** besides its by-reference parameters: global variables and superglobals (`session_start` fills `$_SESSION`), the caller's variables (`$http_response_header`, `$php_errormsg` under `track_errors` before 8.0, `extract`, one-argument `parse_str`);
   - **where it hands control to code the program registered:** each diagnostic it may raise, by level and with its words, which runs the error handler; each class it may look up by name, which runs the autoloaders; the output it may write, which runs the output-buffer callbacks; each interrupt check, which runs the signal handlers once asynchronous signals are on;
   - **the values it may release:** an element it removes or overwrites, an argument or an operand it drops, a temporary it discards. A release runs a destructor when it drops the last reference, and may run the cycle collector when it leaves an array or an object collectable;
   - **what it may return and throw, on every path:** the kinds of value each path returns (`null`, `false`, its own object, a value of the declared type) and the exception classes it may throw. The assumed types (below) take every builtin's outcomes and throw points from these, where today the stubs' hand-written return types and `@throws` give them;
   - **the code it keeps for later and the kept code it runs,** each by the engine state it keeps it in: `session_set_save_handler` keeps an object that `session_start` and the session functions run, `PDOStatement::setFetchMode` keeps a class that the fetches instantiate, `XSLTProcessor::registerPHPFunctions` lets a transformation call PHP functions, `ReflectionClass::newLazyGhost` keeps an initializer that the object's first access runs; and the registrations among these: the error and exception handlers, the autoloaders, the output buffers, the stream wrappers and filters, the tick, signal and shutdown functions.
   
   **Conditions.** Each fact holds under the condition on which the source reaches it, where the source tests what the checker's types decide:
   - an operand's or an argument's type tag (`Z_TYPE_P` and its kin);
   - the parameter parsing's coercions, which follow the caller's typing mode: in weak mode `strlen` converts an object through `__toString`, in strict mode it throws a `TypeError`; and from 8.1 a `null` passed to a scalar parameter raises a deprecation, which runs the error handler;
   - a class (`instanceof_function` and its kin);
   - a constant's bits in an int argument (`JSON_THROW_ON_ERROR`).
   
   A fact behind a test the tool cannot read holds for every input. Without conditions, a registered error handler would run at every operation that can warn for some operand.
   
   **Constructs to operations.** The checker meets constructs, not opcodes. Each AST node kind, with each kind of operand it takes (a literal, a variable, a temporary, a property, an element, a static property), is compiled on each image before optimization (`opcache.opt_debug_level=0x10000`), which gives the opcodes it runs. Compilation depends only on the syntax and the operands' kinds, so one compile of each form covers every program, where a run of a builtin covers only its inputs.
   
   The implicit calls ("Code PHP runs without a call written", below) read their sites from the catalog: which constructs and builtins run which user code, under which condition on their operands, where they run code the program registered or kept, and every release. The assumed types read the builtins' outcomes, and the level and words of each diagnostic by version, such as an undefined array key's.
   
   It follows the engine's entry points for user code (`zend_call_method`, `zend_call_known_*`, `zend_call_function`, `call_user_function`, the user iterator functions), the object handlers that dispatch to user methods, the symbol-table writes (`zend_set_local_var*`, `EG(symbol_table)`), the diagnostics (`php_error_docref`, `zend_error` and their kin) and the class lookups (`zend_lookup_class*`, `zend_fetch_class*`), transitively through php-src's own functions.
   - **Function pointers:** it models the dispatch points the engine uses: the object handlers, the class entry's callbacks, the stream wrapper and stream filter operations, `zend_fcall_info` callables, and a value's release through `zval_ptr_dtor` to a destructor and to the cycle collector. Control also leaves php-src's own calls two other ways, each modelled:
     - **a library calling back:** each php-src function handed to an external library is reachable from every call into that library. libxml2's input callbacks reach the stream wrappers, so `DOMDocument::load`, `simplexml_load_file`, `XMLReader::open` and an external DTD in `loadXML` run a user wrapper's `url_stat` and `stream_open`. So do libcurl's callbacks, SQLite's user functions and libxslt's PHP functions;
     - **a fiber switch** (`zend_fiber_switch_context`), which runs whatever the other side runs: anything.
     
     A call through a pointer it does not model makes the builtin or the operation run anything.
   - **The toolchain:** clang's AST of php-src, configured inside each version's image from the source it ships (`/usr/src/php.tar.xz`), with every bundled extension enabled and its libraries' headers installed, so that the macros (`PHP_FUNCTION`, `ZEND_METHOD`, the `RETURN_*` and call wrappers) are expanded as the compiler sees them. A parser that does not preprocess would read them wrong. The VM is generated with each handler a function of its own (`zend_vm_gen.php --with-vm-kind=CALL`), so that a handler is a unit to read. The extensions outside php-src that the stubs declare are read from their own source, at the releases that support each version.
   - **What no source covers** runs and writes anything, as an unknown call does: an extension that builds only elsewhere (`com_dotnet`), or one whose source is not read. A function no PHP provides, such as PHPStan's `assertType`, keeps today's rule: declared pure and taking no code, it writes nothing.
2. **The recorder becomes the catalog's oracle.** Every call it observes, on every version image, must be in the catalog; one that is not fails the tool, as a gap in its reading of php-src. It also runs each construct form over operands of every type tag, in both typing modes, with an error handler, magic methods, hooks and destructors that record their calls, so that the operations' facts have the same oracle.
3. **The effects read the catalog,** by the union over the configured versions, built at compile time (fast validation, step 3). A fact applies where the argument's type may meet its condition: at a call the checker sees, by the argument's type; in a declaration's summary, made before the checker's types, unless the argument's declared or literal type excludes it. A builtin method's call reaches the overrides of the methods the catalog says it may call on its own object, found as fast validation's step 2 finds them.

Semantic: a builtin runs and writes what its source may make it run and write on the configured versions. Where the recording missed a path or a write, more is forgotten; where a builtin calls none of its own object's methods, less is; a builtin no source covers writes anything (§11, item 0).

**Oracle.** None beyond §11 for the dumps and the deletions: they leave release behaviour as it is. The fixes found in step 1, fast validation's step 1, the catalog, the implicit calls and the assumed types are semantic, read in 0.11 against `5fb858c9`'s reports.

#### Code PHP runs without a call written

Found reading the starting state (row 0.1), first an item of its own at the end of the plan; built in item 0 after the PHP catalog, whose operations, sites and kept code it reads, so that the baseline holds it (rows 0.12a-0.12f).

**Now.** After item 0's fix, a call written in the code forgets what the code it reaches may write. PHP also runs user code where no call is written, and nothing there is forgotten:
- **magic methods:** `__get`, `__set`, `__isset` and `__unset` on a property the code cannot reach; `__toString` where a value becomes a string; `__clone`; `__invoke` when an object is called; `offsetGet`, `offsetSet`, `offsetExists` and `offsetUnset` on an `ArrayAccess` index; `getIterator`, `rewind`, `valid`, `current`, `key` and `next` in a `foreach`; `__serialize`, `__unserialize`, `__sleep`, `__wakeup` and `__debugInfo` through the builtins that call them. No builtin calls `__set_state`: `var_export` writes a call to it as text, which runs where the program runs that text, as a written call;
- **property hooks** (8.4): a hooked property's `get` on a read and on `isset`, its `set` on a write. A subclass may redeclare its parent's plain property with hooks, so a read through the parent's type runs the subclass's hook;
- **lazy objects** (8.4): the initializer that `ReflectionClass::newLazyGhost` or `newLazyProxy` keeps runs at the object's first access that needs its state;
- **destructors:** `__destruct` where the last reference to an object may go: an assignment over it, `unset`, the end of a scope, a temporary nothing keeps (a call's discarded result, `new C;` as a statement), and every value the released one held, in its elements and properties;
- **the cycle collector:** once its root buffer fills, a release that leaves an array or an object collectable runs the destructors of every object in a garbage cycle, wherever the cycle was made. In a probe, an `unset` of a `stdClass` ran 9,980 destructors of cyclic objects that earlier calls had made;
- **a generator's release:** a suspended generator, released, runs its `finally` blocks;
- **autoloaders:** every function `spl_autoload_register` registered, wherever a class not yet loaded may be named: `new`, a static access, a `class_exists`-family test that autoloads;
- **handlers and callbacks:** the error handler wherever a warning, a notice or a deprecation may be raised; output-buffer callbacks where output is written; tick functions under `declare(ticks)`; signal handlers at `pcntl_signal_dispatch()`, at ticks, and, once `pcntl_async_signals(true)` has run, wherever the VM checks for an interrupt;
- **code a builtin keeps and runs later, from another builtin:** session handlers, the SQLite functions PDO and SQLite3 register, the PHP functions XSLT and XPath call, the callbacks curl and the XML parser hold, the classes PDO fetches into and DOM registers for nodes. Row 0.1c's table runs only what a call runs while it runs; an object made with a callable or an object, such as a `Fiber` or an iterator over a user's iterator, already counts as running anything. The PHP catalog (item 0) gives each keeping builtin and each running one, by the engine state between them; these are examples of it, not the list.
- **included files:** an `include` or a `require` runs the file's top level, which counts as writing anything (`summaries.rs:863`), even where the include check resolves the path;
- **generator bodies:** calling a generator function runs none of its body. The body runs, up to its next `yield`, at the generator's first `current`, `key`, `next`, `send`, `throw`, `valid` or `rewind`, and at each step of a `foreach` over it. Today the call takes the body's effects, though nothing of it runs there. A `Generator`'s methods write anything: no run of PHP recorded them and their stubs are not declared pure (`effects.rs:706-728`).

**Design.**
- **Each such site is a call** to the union of what its possible targets write, applied by the checker as after a written call (item 0, fix steps 3-4):
  - a magic or interface method, a hook, a destructor: by the class hierarchy over the value's type, and over every loaded class that declares the method when the type is `mixed` or `object`;
  - autoloaders, handlers, callbacks and lazy initializers: the ones the program registers or keeps, found by their registration calls through the catalog's kept code; one whose callable is not known writes anything;
  - an included file: what its top level writes where the include check resolves the path, anything where it does not.
- **Where a site is decided.** The catalog's operations give which construct may run which user code, under which condition on its operands, and the checker applies each where the operand's type may meet it. What a condition leaves to the program follows PHP's rules:
  - a property access is magic where no declared property is accessible from the scope, and also where the declared one may have been unset, typed or not, since PHP then calls `__get`: any `unset` of it in code that can reach it (the class, a descendant, a closure bound to it, a dynamic `unset`);
  - a hooked access runs the hooks of every class the value may be that declares the property with one;
  - a lazy object's initializer runs at an access that may initialize it, on a value whose class the program may make lazy: the classes it passes to `newLazyGhost`, `newLazyProxy`, `resetAsLazyGhost` or `resetAsLazyProxy`, every class where it passes one not known;
  - a diagnostic runs the error handler where the catalog says the construct or the builtin may raise it, for the operands' types;
  - an autoloader runs where the catalog says a construct or a builtin may look a class up, and the class may not be loaded yet;
  - a signal handler runs at each interrupt check the catalog's operations give, once the program may have turned asynchronous signals on.
- **Destructors.** A release site runs the destructors of every class whose objects the released value may hold: its type's own, its elements' and its objects' properties', transitively. Reference counts are not tracked, so every such site counts.
- **The cycle collector.** A release that may leave an array or an object collectable may run the destructor of any object in a garbage cycle, so it runs the destructors of every loaded class that declares one: whether a cycle can hold an object of a class is not decided.
- **What a dead object's destructor writes to it is not seen.** Its writes to its own `$this` reach an object no code holds any more; a destructor that keeps its object alive writes the storage it keeps it in, which its effect already holds. So a destructor that writes only its own object's properties forgets nothing at either kind of site.
- **Generators:** a call to a generator function binds its arguments and writes nothing of its body. Each resumption site writes what the body may write between one `yield` and the next, over every generator function the receiver's `Generator` may come from: all of them when its origin is not known. A `yield from` there also runs what it delegates to. A generator's release runs the `finally` blocks around each `yield` it may be suspended at.

**Oracle.** Fixtures for each kind of site, each checked against `php` on every version image that has it: hooks and lazy objects from 8.4, the cycle collector run by an unrelated release, signals where `pcntl` is built. O2 read line by line. Semantic, listed in §11.

#### What PHP gives where a type assumed: int overflow, missing keys and properties, builtins' outcomes

Found by the runtime oracle's generated calls in item 0's validation, first an item of its own at the end of the plan; built in item 0 after the implicit calls above, so that the baseline holds it and every later item preserves it (rows 0.13a-0.13i).

**Now.** The runtime oracle's generated calls (`3d8085f3`, in item 0's validation) found three places where a type says less than what PHP gives. Seven fixtures show them; until these rows, they opt out of the calls, each with its reason. Reading the code beside them found the same assumption for properties, and for every builtin's stub.

- **An int operation is taken not to overflow where an operand's end is open.**
  - **What PHP does.** It gives a float where an int `+`, `-`, `*`, `**`, unary `-`, `++`, `--` or `abs` passes `PHP_INT_MAX` or `PHP_INT_MIN`. At an `int` parameter, return or typed property, that float throws a `TypeError` in both typing modes, since no float outside the int range coerces. The argument, return and property checks already report a float there.
  - **Where the types add the float.** Only when both operands' ends are written: an open end counts as no overflow (`checked_bound_op`, `arithmetic.rs:15-27`; `mul_int_range`, `:84-95`). So `int + int` is `int` and `int<1, max> + 1` is `int<2, max>`, while `int<1, PHP_INT_MAX> + 1` is `int<2, max>|float` (`arithmetic.rs:520-538`). The same split runs through:
    - `++` and `--` (`step`, `transform.rs:165-224`);
    - unary minus (`negate`, `arithmetic.rs:458-479`);
    - `abs` (`to_absolute_number`, `coercion.rs:249-285`).
  - **One set, two forms.** `int<1, max>` and `int<1, PHP_INT_MAX>` are the same set (`Type::int_range`, `constructors.rs:68-75`).
  - **Literal overflow.** Two literals that overflow give `int|float`, not the float PHP gives: `PHP_INT_MAX + 1` is `9.2233720368547758E+18`.
  - **Comparisons** narrow through `f64` and drop a bound past ±9.2e18 (`cut_int`, `comparison.rs:870-920`). So `$i < $n` does not cap `$i` at `PHP_INT_MAX - 1`, and the `$i++` after it could not stay `int` once open ends overflow.
  - **What follows from an overflow:**
    - `intdiv(PHP_INT_MIN, -1)`'s `ArithmeticError` is left out on purpose (`throws/dynamic.rs:63-80`).
    - Appending after the key `PHP_INT_MAX` is read as throwing on every version (`shape.rs:68-73`), but PHP before 8.0 warns and writes nothing.
    - A float key outside the int range is converted modulo 2^64 (`php_float_to_int`, `coercion.rs:320-331`; `PHP_INT_MAX + 1` becomes `PHP_INT_MIN`), but PHP's deprecation (8.1-8.4) and warning (8.5) there are not reported.
- **A general array's key is taken to be there.**
  - A read whose presence the type leaves unknown gives the value type alone (`member_offset_read_type`, `member_resolution.rs:321-337`). It is reported only under `report_possibly_nonexistent_general_array_offset` (`report_missing_offset`, `assignment.rs:748-822`; `OffsetPresence::Unknown`, `offsets.rs:810-811`).
  - PHP reads `null` there and raises "Undefined array key" (a warning from 8.0) or "Undefined offset" / "Undefined index" (a notice before). It does so too for:
    - the read inside `.=`, `+=` and the other compound assignments, and `++` and `--` on an element;
    - a destructuring;
    - a nested read (`$a[$k]['x']`, which then also warns on `null`).
  
    Not for `isset`, `empty`, `??`, `??=`, a nested write (`$a[$k][] = …`, which makes the element from nothing) or a reference (`&$a[$k]`, which makes it `null`).
  - A string offset out of range reads `''` with "Uninitialized string offset" (a warning from 8.0, a notice before), reported only under the same setting.
  - Of a key's presence the analysis proves little: a narrowed `$a[$k]` key (`KeyData::VarIndex`, `keys.rs:37`), and lists indexed below `count()` in a `for` condition, for writes only (`in_range_index_writes`, `checker.rs:690-707`).
- **A property is taken to be there.** A read gives the property's type alone where the type leaves its presence unknown (`declared_property_type`, `member_resolution.rs:405-478`): an object shape's optional property gives its type (`:473-477`), and a union member without the property is left out of the union (`:423-431`). PHP reads `null` there with "Undefined property" (a warning from 8.0, a notice before), or runs `__get` where the class has one.
- **A builtin's stub is taken at its word.** Its return type and its `@throws` are written by hand, in PHPStan's stubs and ours, so a path PHP's source takes that they leave out gives what the stub does not say. The iterators are the case the oracle found.
- **An iterator's `current()` is taken to give a value.**
  - PHPStan's stubs declare `Iterator::current()` as `@return TValue` (`iterable.stub:36-39`), which `ArrayIterator` and most builtin iterators inherit (`:127`).
  - Off a valid position, `php` 8.5 gives:
    - `null` for `ArrayIterator`, `RecursiveArrayIterator`, `ArrayObject`'s iterator, a finished `Generator`, the `IteratorIterator` family, `AppendIterator`, `SplDoublyLinkedList`, `SplQueue`, `SplStack` and the heaps;
    - a `RuntimeException` for `SplObjectStorage`, `MultipleIterator` and `FilesystemIterator`;
    - an `OutOfBoundsException` for `SplFixedArray`'s iterator;
    - a `BadMethodCallException` for `EmptyIterator` (its stub says `never`, `:553`);
    - the iterator itself for `DirectoryIterator`;
    - `false` for `SplFileObject`.
  - Its `key()` gives `null`, `0`, `-1`, a count or a path, by class.

**Design.** Each part is the most precise truth PHP gives, kept wherever it may hold.

*Ints:*
1. **Every end is a value.** An int range's open end is its limit, `PHP_INT_MIN` or `PHP_INT_MAX`, so each set has one form, printed with `min` and `max`.
2. **Exact arithmetic.** Every int operation computes its ends exactly in `i128`. Where an end passes a limit, it adds the floats PHP gives: as literals where they are few, as PHP computes them (each operand converted to a float, then the operation; `PHP_INT_MAX + 1` is `9.2233720368547758E+18`), and as `float` otherwise. This applies to `+`, `-`, `*`, `**`, unary `-`, `++`, `--`, `abs`, `array_sum` and `array_product`. Shifts wrap in PHP (`PHP_INT_MAX << 1` is `-2`) and stay ints.
3. **Exact comparisons.** Int against int narrows in integers: `$i < $n`, with `$n` an int, caps `$i` at `PHP_INT_MAX - 1`, so `$i++` after it stays an int. Int against float compares as PHP does, converting the int.
4. **Sources the engine bounds.** `count()` of an array, and a list's keys, are bounded by the engine's maximum array size (`HT_MAX_SIZE` on 64-bit builds): 2^31 through PHP 8.0 and 2^30 from 8.1, the largest over the configured versions. So a counter compared with a count stays an int. A counter that nothing bounds gains the float at its step past `PHP_INT_MAX`, as every open int does.
5. **What PHP does with the float.**
   - At an `int` parameter, return or typed property, the checks already report it.
   - `intdiv(PHP_INT_MIN, -1)` throws `ArithmeticError`, and `PHP_INT_MIN % -1` is `0`.
   - Appending after the key `PHP_INT_MAX` throws `Error` from 8.0; before 8.0 it warns and writes nothing. Either is a throw point or a report wherever an array's int keys may include `PHP_INT_MAX`.
   - A float key outside the int range is the key PHP's conversion gives, reported as PHP raises it: a deprecation from 8.1, a warning from 8.5.

*Missing keys:*

6. **The read.** A read at a key not proven present gives the value type joined with `null`. It is reported as `offsetAccess.mightNotExist` where PHP raises it, at every site the "Now" list names, with PHP's own words for the configured versions: "Undefined array key" (a warning, from 8.0), or "Undefined offset" / "Undefined index" (a notice, before). `isset`, `empty`, `??` and `??=` are neither typed `null` nor reported. A nested write at such a key makes the element from nothing, so its type joins the written shape alone. A string offset not proven in range is read and reported the same way, with "Uninitialized string offset". So is a property read where the type leaves the property's presence unknown, with "Undefined property", unless the class has `__get`, which the implicit calls run. The words and levels are the catalog's, by version. `report_possibly_nonexistent_general_array_offset` goes.
7. **Presence is proven, not assumed.**
   - **The fact.** A value carries the fact that it is a key of a container place (`$a`, `$this->rows`, `$a['x']`), kept with that place's facts. Every read of the place at that value is present: `$a[$k]`, a nested base, a destructuring of `$a` at known keys.
   - **What establishes it:**
     - `isset($a[$k])` and `!empty($a[$k])` where true;
     - `array_key_exists($k, $a)` and `key_exists`;
     - a write at `$k`, and `&$a[$k]`;
     - the key of a `foreach` over `$a`;
     - the values of `array_keys($a)`;
     - `array_key_first`, `array_key_last` and `key` where not `null`;
     - `array_search` where not `false`;
     - `array_rand`;
     - `in_array($k, array_keys($a))` where true;
     - for a list, an int proven at least `0` and below the same list's `count()`;
     - for a string, an offset proven within its `strlen()`, negative offsets by the configured versions;
     - for a property, `isset($o->p)` where true, a write to it, and `property_exists` for a dynamic one.
     
     A copy of the container keeps the fact.
   - **What drops it:** a write that may remove a key, such as `unset` of an element or an assignment to the place. Also a by-reference pass, and a call whose effect may write the place (item 0). A write that only adds or replaces keys keeps it.

*Iterators:*

8. **What each builtin gives, from PHP's source.**
   - Every builtin's return type is its stub's joined with each outcome the PHP catalog finds on a path whose condition the arguments' types may meet and the stub's type leaves out, on the configured versions, and its throw points are the catalog's exceptions. A stub's narrowing that the catalog's conditions confirm stays. Each builtin whose type this changes is listed, and the list is read in 0.11.
   - The iterators are the case found: the PHP catalog gives, for every builtin class implementing `Iterator` and every iterator `getIterator` returns (`ArrayObject`'s, `SplFixedArray`'s, `DatePeriod`'s), what `current()`, `key()` and a `Generator`'s `send()` and `getReturn()` may return and throw on each path, on every version.
   - A recording of a few states (empty, at the end, before the first move) proves nothing of the states it did not reach, such as a position whose element was removed, so the recorder runs those states as the catalog's oracle, not as the source.
   - It is compiled in with the catalog.
9. **The types.**
   - **At a position not proven valid,** a call gives the stub's type for a valid position joined with the catalog's outcomes off one, and the catalog's exceptions become throw points.
   - **On a type that names only an interface** (`Iterator`, `SeekableIterator`, `OuterIterator`), the call is the union over every loaded implementation, by the hierarchy, as item 0's calls are.
   - **A user class's own `current()`** keeps its declared return. Its `parent::current()` reads the builtin's.
10. **A valid position, proven.**
    - **The position is a storage of the object,** like a property. It is written by `next`, `rewind`, `seek`, a generator's `send` and `throw`, and a `foreach` over it. For an iterator that wraps another, a move of the inner one writes it too. Item 0's effects carry it through calls.
    - **Where `valid()` returns true,** the position is valid until such a write, and `current()` and `key()` give exactly the stub's types. The same holds for an `ArrayIterator`, or `ArrayObject`'s iterator, made over a non-empty array and not moved since.
    - **Why a fact on the position.** `TValue` may itself hold `null`, so the proof cannot be a narrowing that removes `null`.

**Oracle.**
- **Fixtures** for each part:
  - each operator at each limit;
  - a counter bounded by a count, and one bounded by nothing;
  - each consequence of the float;
  - each kind of read at an unproven key or property, and each proof;
  - the string offsets;
  - each builtin whose type the catalog changes, at the path it adds;
  - every builtin iterator class at a valid and an invalid position.
- **How they run.** The runtime oracle runs them with generated calls on every version image.
- **The seven fixtures** now opted out take the calls again. Their expectations become what PHP gives, and they are renamed where a name says otherwise (`a_dynamic_key_read_after_a_foreach_populated_array_is_not_spuriously_nullable`).
- **The recorder** runs the iterators' states on each image as the catalog's oracle.
- O2 is read line by line.
- Semantic, listed in §11.


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
  - **Its place in the load order is its place in the session's walk:** the role, the configured root and the path, as the load finds it. A file an edit adds takes the place a fresh load would give it, so a rebuild of a name declared in several files follows the same order after edits as after a load.
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
- **The edit replay of item 0, driven through the server's entry points,** with every section equal to a fresh load's, load order included. It adds:
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

**Saving.** In instructions, the cross-thread frees' premium: 343.4 M of frees at 42.5 Ir a call, against 23 on the allocating thread, ≈0.16 G. The 575.5 M of drops leave the serial loop, which is wall time, not instructions. Most of the drops themselves go with item 9, whose row counts them.

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

**Saving.** ≈0.06 G of instructions: the two path sorts (20.7 M each) where they go, and `closure_scope`'s text keys (16.5 M). The insert's 145.1 M (`insert_built_decl` 52.5 M, `register_references` 92.6 M) leaves the serial path, which is wall time. What the posting lists save on inserts is not separated in this profile.

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
3. **Names are stored once per cache file.** A cache file holds a table of its distinct names. Table keys already refer to it (`container.rs:96-104`), and now records refer to names by index too, where today they embed the text. A run interns an entry the first time a record needs it (a `OnceLock<Name>` per entry). The per-file cache does the same, with one table per file. The cached layout changes, and the key with it (step 6).
4. **F2's count, as a diagnostic.** A debug-only report of the stub keys in the passes' scope, by the changed name that pulled each in, to explain whatever remains. It gates nothing.
5. **Records are read, not mapped or copied.** It changes the layout with step 3.
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
6. **The caches' key covers everything a build embeds.**
   - **Now.** No version number is kept for compatibility. Both caches are keyed by `PHPSTAN_RS_SOURCE_HASH`, which `crates/phpstan-rs/build.rs` makes from every workspace crate's `src`, manifest and build script, `Cargo.lock`, the workspace manifest, `vendor/phpstorm-stubs`, `vendor/phpstan-src/src` and the target triple. A stub cache file is named `<name>-<hash>.cbor` (`stub_cache.rs:17`, `:43`), and per-file entries live under `files/<hash>/` (`file_cache.rs:43`, `:69`). So any change to the code, including a change to a serialized type, gives a new key, and no build reads another's entries.
   - **The gap.** `stub-sources` also embeds `vendor/phpstan-src/stubs/*.stub`, PHPStan's own stubs (`stub-sources/build.rs:95-145`), which the hash does not read. A submodule update that changes only those files keeps the key, and the new binary reads a stub cache built from the old stubs. Step 2's version deltas (`vendor/phpstan-src/resources/functionMap_php*delta.php`) would be a second such input.
   - **One list of embedded inputs.** The build script of the crate that embeds data hashes exactly the files it generates `include_str!`s for, from the same list, and exports the result (`stub_sources::SOURCES_HASH`). The caches' key is the code's hash with that one, both BLAKE3 digests (item 17). `phpstan-rs`'s build script hashes the workspace's code and manifests only. So a file that is embedded is in the key by construction, and a new embedded input, such as step 2's deltas, joins the same list.
   - **The hand-kept `VERSION` goes** (`container.rs:14-18`, now 7). The key already changes with the code that writes the layout, and a number bumped by hand can only fall behind it. The magic numbers stay, to tell our files from others, with the CRCs.
   - **A layout depends on nothing else.** Debug and release builds share a key, which is safe because they write the same layout, and a debug build round-trips what it stores (`file_cache.rs:228-236`). A field under `cfg` in a serialized type would break that, so none is allowed. The debug-only statements `FileBodies` kept went with the debug oracles (row 0.3), so the `oracle` pass runs with the per-file cache on, in every state.
   - So no item has to bump anything when it changes a cached layout: the key changes with its code.

**Oracle.**
- `registry-dump`: the scope, the guards, the extensions, and every stub record decoded.
- O2 with the stub cache cold and warm.
- A test that loads the cache, then stores, prunes and reloads it while a reader still holds the old file. It runs on every platform the release builds; today CI runs on Ubuntu only (`rust-ci.yml:18`).
- A test that corrupts one record and expects a rebuild, not a panic.
- A test that hashes the files `stub-sources` embeds, from its own list, and finds `SOURCES_HASH`.

Exact for 1, 3, 5 and 6; semantic for 2, listed in §11.

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

Exact, except constants on a cycle, listed in §11.

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
- **All six fields leave the declarations,** and the cached layout changes with them (the key follows the code, item 6 step 6). Until this item, item 4's `FileDecls` holds them as one more vector of the file, with ranges, so the item removes a vector and allocates nothing per position.
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

**Allocations: ≈0.52 M (M10).** This covers the case references' lists (`declared_type` 50,267, `collect_class_names` 43,874), their copies when a generic call copies its signature (≈38 K, moved here from item 18's row), `dedup_versioned_params`' map and order vector, `reconcile_with_native` (41,085) and `from_decl`'s name list.

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
4. **The per-file cache stores `LineStarts`** for a context entry, where it stores an `Arc<LineIndex>` today (`file_cache.rs:94`); the key follows the layout (item 6, step 6). Item 17's context base stores line starts too.
5. **Nothing in the front end reads lines.**
   - Ignore marks record offsets: the mark's start, the last and the next code token's start. `ignore_comments` resolves lines through the analysed file's index.
   - A class member records its start, the end of the token before it and the start of the token after it, and `removal_span` is computed from those when a fix asks.
   - A statement's leading doc records its offset.
   - Anonymous-class names take their line from `LineStarts` on the worker.

**Oracle.** O1, O2 (every position, fix span, ignore result, anonymous-class name), the line-index tests over multi-byte and astral lines and over `\r`, `\n` and `\r\n` breaks, the LSP tests. Exact, except the semantic change in §11: lines end at a lone `\r`, and so does a line comment.

**Saving.** ≈0.2 G: the line index's 270.4 M, less the block pass (≈35 M, step 2) and the analysed files' column tables, which stay.

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

**Oracle.** O1, and lexer tests over each of PHP's readings above, checked against `token_get_all`. Exact for steps 1-10 and the rest of step 4. Semantic for the casts, the spaced names, and steps 11-14, listed in §11. **Saving.** ≈0.35 G. **Allocations:** ≈0.45 M (M10): `parse_qualified_name` 282,779 (its 321,919 less the 39,140 name allocations item 16 removes; one per new name, 19,570, stays), `NativeType::from_name` 135,859, `strip_leading_backslash` 29,664, and the digit separators 582.

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
- **A guard does not follow when PHP binds a declaration.**
  - Today an earlier top-level `if (c) { return; }` guards only nested declarations: "PHP declares those before running anything" (`syntax/src/declarations.rs:69-71`). Only `return` counts as an exit there.
  - PHP binds some declarations when it compiles the file, and others only when execution reaches them. Tested with `php` 8.5 on a file whose first statement is `return;`, included from another, with opcache on and off:
    - **bound at compile time, so they exist anyway:** functions; traits that use no trait; interfaces with no parent; classes that implement no interface and use no trait, with no parent, or with a parent that already exists when PHP compiles the file: internal (`extends Exception`), earlier in the same file, or declared by a file loaded before (`class D extends P`, with `P` from an earlier file);
    - **bound only when reached, so they do not exist:** a class that uses a trait or implements an interface, or whose parent is declared later; a trait that uses a trait; an interface with a parent; an enum; a top-level `const`; a `define()`.
  - So `if (!defined('ABSPATH')) { exit; }`, WordPress's guard, decides whether every run-time-bound declaration after it exists. Today `exit` is not counted, and neither kind is told apart.

P12 step 3 was set aside because "it cannot hold references into a tree still being built". With item 9's layout it can: a node's id is fixed when it is built.

**Design.**
1. **The parser lists the declarations** as it builds them: class-likes, functions, constants and `define()` calls. Each entry holds:
   - the name and kind;
   - the guard, from the parser's statement stack, following how PHP binds the declaration:
     - a declaration PHP binds at compile time (the first kind above) is guarded only by the conditions it is nested in;
     - any other is guarded by every earlier top-level exit too, conditional or not: `return`, `exit`, `die`, `throw` and `__halt_compiler`. After an unconditional exit it never exists, and is reported as dead code;
     - a class whose parent is declared in another file is bound at compile time only if that file was loaded first, which only the run decides. So it is guarded as possibly declared: both possibilities are kept;
   - the `ImportScope`;
   - its doc, as item 10's rule picks it. The doc's names are resolved in the entry's own `ImportScope`, since a `use` may stand between the doc and the declaration;
   - the node's id.
   
   **The list is in source order,** which decides collisions: a second unconditional class of the same name merges into the first, and the first wins any conflict (`registry/index.rs:256-345`). Item 9 gives a node its id only when its list commits, after its children. So the parser reserves an entry when a declaration starts and fills in the id at the commit. Anonymous classes are entries of their own, named from item 9's side table, and stay after the named ones as today (`file_declarations.rs:31-32`).
   
   **A file that defines constants it cannot name is marked so:** `define($name, ..)`, `define(__NAMESPACE__ . '\X', ..)`, `call_user_func('define', ..)`, `('define')(..)`. Today only a literal name is recorded. Readers then know some constant may be defined there.
   
   Declaration building iterates it, and so do the duplicate sites, the incremental path and the opt-in rules over analysed files. They all run while the tree exists. What outlives the tree is copied out on the worker, as item 3 says. The declarations walk, its facts-gated expression walks and the separate declared-classes collection go.
2. **`ImportScope`s are published once**, at each `namespace` and `use`. Readers start from them.
3. **One walk for the rest**, with every builder at each node:
   - `BodySummary` (purity, property writes, assigned properties);
   - `InitFlow`, built from the same statement entries and exits;
   - `FileSites` (`GlobalFacts`);
   - the per-file families item 19 moves here from the project-wide passes, registered like the rest.
   
   **The walk visits in PHP's order of evaluation,** which `InitFlow` records and `BodySummary` ignores.
   - In an assignment, PHP evaluates the target's operands first, then the value, then stores: `$a[f()] = g()` and `$o->{f()} = g()` print `fg`. A destructuring assignment evaluates the value first: `[$x[f()]] = [g()]` prints `gf`.
   - The short-circuit operators are visited as they run.
   - Today `events_of` records the value's events before the target's (`init_flow.rs:420-422`), so a read in a target's operands comes after reads in the value. That order is corrected, and the change is semantic.
   
   Statement docblocks' names stay where `2dc1aa9b` put them: the parser records each doc site with its context.
4. **The collector goes.** The stub rules, which item 9 applies as a stub is parsed, record the references of the types they add, as they resolve them. The stub build reads the parser's record.
5. **A file's bodies are one slice** of `(summary, flow)`. Today they are a hash map holding two `Arc`s per body (`BodySummary` and `InitFlow`, 57,506 bodies, M8). A function's or method's `FileDecls` entry (item 4) holds its body's index, so a pass reaches a body through the declaration's id. Only the server's position lookups search the slice by span.
   - A body's lists are ranges into vectors of the file: one each for calls, purity calls, reassigned names, assigned properties, events and flows.
   - So a file's summaries are a few allocations, not several per body, as item 4 stores declarations.
6. **Builders gather in scratch and emit exact-size slices.**
   - Today the summary and flow builders grow vectors per body and per node:
     - `events_of` 165,258 and `Summarizer::call` 138,869;
     - `FileBodies::of_file` 135,912 and `push_events` 104,304;
     - `flow_of_stmt` 76,492, `purity_call` 42,080 and `Summarizer::finish` 37,003.
   - They gather in per-walk scratch stacks, reused across bodies and files, and move each body's lists out once, at their exact size.
7. **References are recorded into one vector per file, sorted once** (item 5's sorted slices). Today they go into hash sets grown insert by insert: `Recorder::add` 57,859 and `Recorder::doc` 51,886.
   - A doc site holds the interned `ImportScope` of step 2. Today every doc site clones the whole name context into an `Arc` of its own (`references.rs:279`).
   - `recorded_references` reads the sites in their recorded order. Today it collects and sorts them, and the frames, per file (`deps.rs:55`: 33,273). What else runs under `file_references` is item 14's `DocNames` memo.
   - A declaration's name is its `Name`. Today `Declaration::name` formats it (`declarations.rs:45`: 30,159).

**Oracle.** O1 (declaration lists, guards, contexts, summaries, flows, references), `registry-dump`, O2, and a fixture of the binding cases above, checked against `php`. Exact, except the semantic changes listed in §11: guards by PHP's binding, constants a file defines under names it cannot spell, and the order of evaluation in `InitFlow`.

**Saving.** ≈0.45 G, estimated: the declarations walk (≈140 M around its builders) and its eight callers' walks, and the traversal of two of the three body walks (`InitFlow::of` and `GlobalFacts::of`, 218.5 M each, of which their builders' own work stays). This profile does not separate a walk's traversal from its builders'.

**Allocations: ≈1.31 M (M10).** This includes the declarations walk's lists, which step 1's list replaces: the walk's `if_chain` 52,762, `Declaration::name` 30,159, the duplicate sites 62,346 (class sites' `record` 36,898 and `merge` 12,326, function sites' 11,198 and 1,924) and `built_declaration` 28,516. The old collector allocates nothing in this run: no stub base is built, and no tree is changed after its parse.

### 14. PHPDoc finished: P7 and P11 as designed (§3.2)

1. **P7's lowering replaces the lazy round trip.** `DocType`'s type comes from `phpdoc::lower(&TypeNode)`, not from `parse_written_type(to_source_string())` on first use (`doc_type.rs:57`, `:90`; 118.3 + 83.3 M).
   - **It lowers eagerly, in `parse_tag`, once per distinct tag line.** A tag's `DocType`s are shared by every docblock with that line, which is why there are 21,146 lowerings today against 41,176 docblock parses. `docblock::parse` lowers only the types no tag effect holds.
   - **The statement `@var` memo stays** (`var_tag_memo`, `checker.rs:282`, since `2b6aeb84`). Its 34,821 misses resolve the `DocType` winners of step 6, lowered once, through step 2's memo. Today each goes through `impl WrittenType for TypeNode`, the text round trip (`doc_type.rs:88-96`: 34.1 M), which goes.
   - **The text grammar's callers, by kind:**
     - text from the configuration (`restricted_symbol_usage.rs`, `constant_resolver.rs:40`) goes through `phpdoc::parse_type`;
     - texts written in our Rust source (`stubs.rs`, `predefined_constants.rs`, `mysqli.rs`, `assertion_table.rs`, `probe_checks.rs`, `narrowing/*`, `result_shape.rs`, `native_type_hints.rs:197`) are built once into `static` tables (25 G §11.7). Today they are parsed again on every call once the memo is gone: `parse_type_string` runs 6,718 times;
     - **a class name from code becomes `Type::object(name)`,** with `self`, `static` and `parent` as today. Today `infer_class_const_type` parses it as PHPDoc (`function_sig.rs:529`), whose keywords match `integer`, `double`, `boolean`, `resource`, `numeric` and `scalar` in any case. So `const A = Integer::MAX;` reads as an `int` constant, though `class Integer`, `class Resource` and `class Numeric` are valid PHP (`php` 8.5: `Integer::MAX` gives the class's 9);
     - a test of whether one word is a type keyword (`phpdoc_unresolvable.rs:258`, `:347`) asks `types::type_keyword`.
   - **What stays of the text grammar:** `native_type`; `keyword_type`, built from the keyword table rather than by parsing (it calls `parse_type_string` today, `parse.rs:913`); `is_reserved_type_word`; `canonical_int_key`; `display_shape_key`; and `parse_display.rs`, the different grammar that reads displayed types for the gate and the tests (`gate/type_drift.rs:196-197`, `gate.rs:385`, `tests/runtime_oracle.rs:223`). The rest of `parse.rs` is deleted.
   - **`mentions_mixed` is set by the lowering, from the node.** Today `text_mentions_mixed` finds the word `mixed` between non-identifier bytes (`parse.rs:1443-1458`), so `App\Mixed\Foo`, a valid class, counts as explicit `mixed`.
2. **Resolution memoized as §11.7 designed.**
   - The memo is per file, keyed by `(written Type, context id)`.
     - The written `Type` is the interned node P7's lowering gives. The entry holds it, as `b081be75`'s memo does, so no address is reused while the entry lives. A `TypeNode` cannot be the key: after step 7 it is an index into one owner's arena.
     - The context is interned by content. It holds the `ImportScope` (item 13; for a declaration's doc, the declaration's scope), the template table with each name's whole `TemplateParam` (bound, variance, default, scope), the alias table with each alias's resolved type (the local table at build time, the registry's final one in the checker), and the class `self` names.
     - Template names alone would not do. Marking puts the whole `TemplateParam` into the type (`registry/generics.rs:141-142`), and a method's table is its own templates, scoped by its name, plus the class's (`function_sig.rs:744-754`). So `@template T of int` on method `a` and `@template T of string` on method `b` share names but not types.
   - It holds the whole resolved type, template marking and alias substitution included. Today's memo (`b081be75`) is sound but covers only the namespace-and-imports step, and `mark_template_params` and `substitute_type_aliases` still run on every use (`type_resolution.rs:74`, `class_info.rs:1129`, `:1220`).
   - A type used in many docblocks of a file is resolved once per context.
3. **P11 step 1: `parse_tag(tag, rest) -> Arc<TagEffect>` and `apply(&mut DocBlock, written, &TagEffect)`.** There is no scratch block and no `merge_from` (56.2 M). Step 3's `Arc` fields follow.
   - **The effect holds only what `(tag, rest)` decides:** the fields and the parse errors.
   - **`apply` makes every decision that depends on the spelling as written:** the Doctrine argument check, whether a tag PHPStan parses is dropped for its errors, and the `InvalidTagValue` naming the spelling (`docblock.rs:451-488`). 25 G §11.8 put `invalid` in the effect; then `@param X` and `@phpstan-param X` would share one effect, and its `tag` and drop decision would come from whichever spelling a thread parsed first.
4. **The tag memo is keyed by the fingerprint of the pair:** the tag's length, the tag and then the rest, fed to one `SipHasher13` (step 8). There is no `format!` (`docblock.rs:497`, 54.7 M) and no separator.
5. **Tag tokens borrow, and the doc is read as bytes.**
   - The tag parser reads a token slice by index, and a token's text is its byte range of the doc (303 K string clones today).
   - **The lexer's classes are PHP's, on bytes.** Blanks are space, tab, `\r` and `\n`. An identifier or a variable is `[A-Za-z_\x80-\xff][A-Za-z0-9_\x80-\xff]*`. Today the doc lexer uses Unicode's classes (`c.is_whitespace()`, `is_alphabetic`, `grammar/lexer.rs:89`, `:124`), so `@param Price€ $p` stops at `Price`, though `class Price€` is valid PHP.
   - Every token keeps its bytes: variables, string literal contents (as `PhpString`), shape keys, and template, property, method and constant-fetch names. A parameter `$caf\xE9` is named by those bytes in reflection and in `getDocComment`, so the doc's variable matches it byte for byte.
   - Identifiers become `Name`s as code identifiers do. From item 15, that keeps their bytes, so a doc naming `\xA9` names the class the code declares.
   - **A wrapped tag is not contiguous in the doc.** A tag's text is the doc's byte range from the tag to the next tag line, and the lexer reads a newline followed by blanks and one `*` (not `*/`) as one blank. A raw slice of `@return array{\n *   id: int,\n * }` would otherwise put `*` tokens in the type. The fingerprint is fed the same logical bytes, so equal tags share an entry whatever their indentation.
   - A description is decoded, its decorations removed, only for display.
6. **One parse and one product per doc text, one per tag line (M6).**
   - **Today one doc text feeds eight memos,** each keyed by its own copy of the text:
     - the grammar node (`grammar.rs:29`);
     - the block (`docblock.rs:295`);
     - one `DocBlock` per tag (`docblock.rs:495`: 26,393 × 840 B);
     - tag values (`grammar.rs:58`);
     - type prefixes (`grammar.rs:73`);
     - the `@var` winners (`phpdoc_unresolvable.rs:73`);
     - `DocNames` (`deps.rs:145`);
     - written types (`types/src/parse.rs:29`).
   - **Today a doc is parsed twice, and the two parses split it differently.** `grammar::parser::parse` cuts each line at its first `//` and joins wrapped lines with a space (`grammar/parser.rs:20-30`); `docblock::parse` keeps `//` and joins with a newline, so that `http://` survives (`docblock.rs:322-341`). So `/** @var 'http://a'|'https://b' $u */` on a statement reads as `@var 'http:` through the grammar path, an unterminated literal that raises no error, while the same doc on a property gives the union. The grammar parse costs 144.0 M (`grammar::parse_shared`, 9,309 calls).
   - **After this step, a doc is parsed once.** The lexer splits it into tags; `PhpDocNode`'s tags are the block's tags, and the block is `apply` over each tag's effect. `grammar::parser::parse`'s own splitter is deleted. The readers of grammar nodes (`var_tag_nodes`, `tag_type_nodes`, `ancestor_tag_types` at `ancestors.rs:512`, `unknown_phpstan_tags` at `docblock.rs:1364`) read the block's tags. A `//` is a comment only between tokens and outside literals, by the lexer's rule.
   - **The block records each tag's winner by one rule,** the resolver's order that `select_var_tag_nodes` follows for `@var` (`PhpDocNodeResolver`): params and param-outs by name, the return, and the var tags, named and unnamed. Each winner is a `DocType`.
     - Today `phpdoc_unresolvable::tag_type_nodes` is a second implementation, not memoized, cloning nodes, at six call sites (`function_decl.rs:445`, `missing_typehint.rs:438`, `phpdoc_type_compat.rs:34`, `:352`, `:362`, `:370`; 69.1 M over 8,704 calls). It disagrees with the block: it ignores `@phan-real-return`, which the block maps to `return` (`docblock.rs:801`, as PHPStan does, `PhpDocNodeResolver.php:431`), and takes `@phan-param-out`, which the block's mapping rejects. `DocBlock.var_type`/`var_name` is a third rule.
     - `tag_type_nodes`, `select_var_tag_nodes` and the single `var_type`/`var_name` go, and every checker site reads the block.
   - **After this step there are two memos.**
     - **The docblock memo** holds, per text, the block (its tags, effects and winners), its referenced names as `Name`s, and its `@var` winners. The names are what the 25 G report's P9 step 3 designed: `DocBlock::referenced_names() -> &[Name]`, computed with the block, which `recorded_references` reads. So `DocNames` and its memo go.
     - **The tag memo** holds `TagEffect`s (step 3), which absorbs the tag-value and prefix memos.
     - The written-type memo goes with the text grammar (step 1).
7. **Nodes live in their owner's arena.**
   - A `TypeNode`'s children are indices into an arena, as the per-file arenas of rust-analyzer and Zig's AST index their nodes: `la_arena::Arena<TypeNode>`, as items 4 and 9 use. Today every `Nullable`, `ArrayOf`, `OffsetAccess` and `VarianceArg` boxes its child, and every union, intersection and generic has a vector of its own (`grammar/ast.rs:114-144`).
   - **Each `TagEffect` owns the arena of its nodes,** since an effect is shared by every docblock with its tag line: its nodes cannot be indices into any one docblock's vector, and copying them into each block would undo the sharing. A docblock owns an arena only for what no effect holds.
   - A `DocType` is `(Arc<owner>, Idx<TypeNode>, Type)`. Readers walk nodes through `DocType::node() -> NodeRef<'_>`, which carries its arena, as `phpdoc_unresolvable.rs:120-140` and `narrowing.rs:1481` walk them today.
   - So a distinct docblock or tag line is a handful of allocations, not one per node.
   - **The caches' format.** A `FunctionReflection` is serialized into the per-file cache and the stub cache (`function_sig.rs:262`), with its asserts' `written_ty: DocType` (`asserts.rs:63`). Today a `DocType` serializes its node only, and decoding calls `DocType::new(node)`, which leaves the lowered type empty (`doc_type.rs:133-143`). A serialized `DocType` holds its own subtree, re-based into an arena of its own, and its lowered `Type`, so decoding lowers nothing. Both caches' layout changes, and their key with it (item 6, step 6).
8. **Keys by fingerprint, bounds by weight.** `syntax::memo::Memo<V>` becomes this memo, for all its users (`memo.rs:70-93`), and item 18's F6f uses it. Its shard-clearing code goes.
   - **A memo is keyed by a 128-bit fingerprint of its text,** as rustc keys incremental results by `Fingerprint`, so no text is copied into a key. Today's key copies come to ≈6.1 MB beside `DocNames`' own.
     - The fingerprint is SipHash-1-3 with 128-bit output (`siphasher` 1.0.4, `sip128::SipHasher13::new_with_keys`, already in the lockfile through `phf`), keyed by a key drawn at random for each process.
     - So no input can be crafted to collide with another without the key, and an accidental collision has a 2^-128 chance per pair. A collision would hand one docblock's types to another.
     - rustc's key is fixed, because it stores its fingerprints. These memos live in one process. Their results are pure, so a different eviction order cannot change the output.
   - **It is bounded by a byte budget** that counts its values as well as its keys.
   - **It evicts by `quick_cache`'s weighted cache** (0.7, `sync::Cache::with_weighter`), a Clock-PRO variant that resists scans, with 21 B of overhead per entry. That replaces a hand-written CLOCK. Today a full shard is emptied whole (`memo.rs:88`), and the tag memo's bound is 131 K entries.

**Oracle.**
- **For every type node of the fixtures, the vendor tree and the corpus,** `lower(node)` against `parse_written_type(node.to_source_string())`, taken before the text grammar is deleted (25 G §11.7). Every difference is listed in §11 as semantic:
  - escaped quotes: the literal printer does not escape them (`grammar/ast.rs:435`), so `"don't"|"do"` prints as `'don't'|'do'`, which `split_top_level` reads as one literal (`parse.rs:300-317`); the lowering gives the two literals;
  - `mentions_mixed` read from the node;
  - identifiers by their bytes (`Price€`, non-UTF-8 names);
  - the unified winners (`@phan-real-return`, `@phan-param-out`);
  - a `//` inside a literal type;
  - class constants of classes named like type keywords (`Integer::MAX`).
- `registry-dump` (every docblock's resolved types), the PHPDoc resolution record, O2 with every cache state (cold, warm, written, read).
- **O1's references and doc sites, the edit replay and the server session.** `DocNames` feeds `recorded_references` (`deps.rs:63-75`), which feeds dependents and `walk_affected`; a name lost there shows only as a stale diagnostic after an edit, which no full run sees. Item 0's O1 prints each doc site's `DocBlock`, every field canonically, so the fields no reflection carries (closures' docs, `optional_params`, `invalid_tag_values`, `parse_errors`, flags) are compared too.

Exact otherwise. **Saving.** ≈0.35 G, and the second parse of every doc (144.0 M).

**Allocations: ≈2.59 M (M10).** Steps 1-7 remove them.
- **Three sites need a word more:**
  - **A docblock's tag lines** are copied into `String`s (`docblock.rs:337`, `:339`: ≈220 K). They become byte ranges of the doc (step 5).
  - **The tag lexer and parser copy text.** The lexer makes a `String` per identifier (`grammar/lexer.rs:127`, 209,225). The parser clones tokens as it advances (`grammar/parser.rs:98`, 210,623), and lowercases identifiers and tag values to match them (≈145 K). Tokens become byte ranges (step 5), matched case-insensitively without copies.
  - **`apply_tag` copies every description to test a prefix** (`description.replace(' ', "")`, `docblock.rs:915`: 19,391). The test skips spaces as it reads.
- **The rest:**
  - memo keys and entries (`docblock.rs:497-498`, `grammar.rs:61-66`: 340 K, of which 199 K are `format!`s and 42 K a value cloned on every hit);
  - `to_source_string` (≈245 K);
  - `merge_from` (44,557);
  - the `DocNames` memo (75,793);
  - the checker re-parsing statement docs and resolving their `@var` tags (≈305 K).
- The checker's eager messages for `@var` tags (`stmt.rs:529` 30,060, `var_tag_types.rs:346` 35,058) are item 18's F1, which formats them only when reported.

**Memory.** About 43 MB of the memos' 72.5 MB at exit (M6), plus what the duplicate memos hold:
- the per-tag blocks, 22.2 MB, become effects of a few words each;
- `DocNames`, 14.6 MB, its 8.9 MB of keys included;
- the other memos' key copies, ≈6.1 MB;
- the duplicate memos (the grammar node, tag values, written types, the `@var` winners).


### 15. Names held as names (R6; C3 steps 6-7)

**Now.**
- `fold_hash` and `is_folded_eq` cost 701 M: lookups by `&str` whose callers hold text where a `Name` existed.
- `Interner::name` runs 1.42 M times on text whose `Name` existed a moment before.
- 1.33 M `String`s are cloned: ≈0.6 M of them PHPDoc's (item 14) and 0.19 M `resolve_transitive_purity`'s (item 4). The ≈0.5 M left carry identifiers through the checker and the registry.

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
  - the round trips through the interner (counts in parentheses are interning calls, not allocations):
    - the parser's `ident_from` re-interns a `Name` the lexer gave it (`parser/tokens.rs:194`);
    - qualified names are one token, interned from their source slice (item 12, step 8; `parse_qualified_name` 94 K calls today), and so are `use` paths (`use_path` 63 K). A group `use A\{B, C}`'s member, the one name still joined from parts, is found by the interner hashing the prefix and the member as one text, as `qualify` is below;
    - `resolve_name_expr` builds a `String` and interns it (96 K);
    - `bind_self` re-interns the class (65 K);
    - the key table resolves property names through text (37 K);
  - then `narrowing`, `calls`, member-usage tokens, `project`, `types` and `phpdoc` by module.
- **The end state** deletes `TableKey for str` and `FoldKey for str` from the registry. The compiler then lists every caller still holding text. Text that enters from outside (configuration, a class named in a string literal) is interned once where it enters.
- **A name is its bytes.** PHP's identifiers are bytes: the lexer takes any byte from `0x80` up, and `class \xA9` and `class \xAA` are two classes (item 9).
  - `Name` holds `[u8]` and compares with PHP's ASCII-only case folding, as it does today (`name.rs:41`). `Deref<Target = str>` goes, and the compiler lists every reader of a name as text.
  - **Where a name's text is shown, a byte outside valid UTF-8 is `\x{A9}`.**
    - `\xA9` would be ambiguous, since `\` is PHP's namespace separator: in `php` 8.5, `namespace Foo; class xA9 {} class \xA9 {}` declares two classes, `466f6f5c784139` and `466f6f5ca9`, and both would print as `Foo\xA9`.
    - A `{` occurs in no PHP name, so `\x{A9}` maps back to exactly one name. A synthetic anonymous-class name, the one name that can hold a `{` through its path, spells it `\x{7B}`.
    - One escape serves everything that shows a name as text: messages, the text and JSON formats, baselines (which match by message, `baseline.rs:111-159`), the type printer, item 0's dumps and the server's strings. `parse_display` decodes it, so the gate's type drift reads names back exactly (`gate/type_drift.rs:196-197`).
  - **A fix holds bytes.** Today `DiagnosticFix` carries `replacement` and `within` as `String`s (`diagnostics/src/lib.rs:172-177`), built from canonical names (`name_case_fix`, `checker/resolve.rs:403-424`), and `apply_fixes` writes them into the file (`fix.rs:44`). An escaped name written there would be read by PHP as a namespace separator and other letters. So `replacement` and `within` are bytes, built from the name's bytes, and `apply_fixes` and `resolve_span` work on bytes, with PHP's word bytes `[A-Za-z0-9_\x80-\xff]`. Today `resolve_span` gives up on a region that is not UTF-8 and finds words with Unicode's `is_alphanumeric` (`fix.rs:13-14`). The JSON view shows a fix with the escape.
  - **`Name` serializes its bytes** when the format is not human-readable (`serialize_bytes`, interned from the bytes), and the escape when it is. Today it writes and reads a `str` (`name.rs:186-206`), so a lossy decode would merge `\xA9` and `\xAA` on every warm load. Both caches' layout changes, and their key with it (item 6, step 6).
  - **A run-time string becomes a name as PHP reads one,** through one conversion, `syntax::name::from_runtime(&PhpString, Kind)`. `php` 8.5 shows the rules:
    - exactly one leading `\` is stripped: `class_exists("\\NS\\C")` is true, and with two leading backslashes it is false;
    - nothing is trimmed: a leading or trailing space finds nothing;
    - a class or function name folds ASCII only: `class_exists("ns\\c")` is true, while a class `\xC9t\xE9` is not found as `\xE9t\xE9`;
    - a constant's namespace folds and its name does not: `defined("ns\\C1")` is true, `defined("NS\\c1")` false (C3's `ConstName`);
    - a callable string splits at the first `::`.
    
    Every reader of a string as a name goes through it: `class_exists`, `function_exists`, `defined`, `constant`, `new $s`, `$s::`, callable strings and arrays, and `is_a` and `is_subclass_of` with a string. Today `callable_values.rs:40`, `:89` and `visibility.rs:251` strip every leading backslash (`trim_start_matches('\\')`), and `as_str()?` (`callable_values.rs:34`, `:52`, `:66`, `visibility.rs:251`) drops a string that is not UTF-8.
  - **Names fold ASCII only everywhere.** Unicode's `to_lowercase` folds names at `oop/attributes.rs:182` (repeated attributes), `restricted_symbol_usage.rs:197`, `:446`, `:629`, `:637` and `restricted_symbols.rs:390`, `:393`, `:398`, so `Été` and `éTé` match there, which PHP does not.
  - **An open document keeps its file's bytes.** The editor sends a document's text as JSON strings, already decoded, and nothing in the server knows a file's encoding today. A file stored as Latin-1 with `class caf\xE9`, opened as Latin-1, arrives as `café`, which is `caf\xC3\xA9`, so the server alone would report the class as not found.
    - At `didOpen`, when the file's bytes on disk are not UTF-8, the server tries the encodings `encoding_rs` decodes (0.8.42, WHATWG labels: windows-1252, ISO-8859-x, Shift_JIS, …). The document's encoding is one whose lossless decoding of the disk bytes equals the editor's text (`decode_without_bom_handling_and_without_replacement`).
    - Every later version of the document is encoded back with it (`encode`), so names keep the bytes PHP will read. A fix is offered as an edit only when its bytes are text in that encoding; otherwise the diagnostic stands without the code action.
    - When no encoding matches, the document is analysed as UTF-8, with a diagnostic saying its non-ASCII names may not match the file on disk.
- **A short name is a slice** of the name, at the offset of its last segment (the 50.9 M of `rsplit` in `check_expr_impl`). `NameData` gains `last_segment: u32`, set when the name is made: the byte after its last `\`, which PHP's lexer takes as a separator in any encoding. Item 16 moves it into the header.

The purity and property-write keys went by item 4, as ids.

**Oracle.** O2, `registry-dump`, `--dump-types`, `--dump-expr-types`, every cache state, the server session, and a fixture checked against `php`: `class \xA9` beside `class \xAA`, `Foo\xA9` in ASCII beside `Foo\` followed by the byte `0xA9`, `Été` beside `éTé`, string names with zero, one and two leading backslashes, and a Latin-1 document opened in the server. Exact, except the changes listed in §11: names that differ only in bytes outside UTF-8 are distinct and print with `\x{NN}`; names fold ASCII only where Unicode's lowercasing was used; string values name classes, functions and constants by PHP's rules; and the container-field facts' keys become canonical array keys (C4 step 5: `'1'` and `1` one key). Static-property keys by resolved class are already built (`2d1e6c45`). **Saving.** ≈0.6 G.

**Two sites need a word more:**
- **Name resolution builds text to intern it.** `qualify` and `resolve_as` (`name_resolution.rs:382`, `:423-425`; ≈315 K) format the joined name into a `String`. Instead, the interner hashes the parts as one text and compares a candidate part by part, so an existing name is found without building its text. Only a new name allocates.
- **A call's callee name is lowercased five times per call** (`expr.rs:499`, `:821`, `:871`, `:889`, `:918`: 47,957 each). Its resolved `Name` already holds its folded form (`Name::lower`).

**Allocations: ≈2.76 M (M10).** The rows below are M10's, ≈2.44 M of it; the rest are smaller callers of the same mechanisms. `Name::to_string` is not a row: it is a utility, and its 677,480 allocations are charged to its callers, here and in items 4 and 18.
- **Lowercase copies:**
  - `check_expr_impl` 338,625 (with the five above);
  - `forwards_static_binding` 97,960 and `type_check_call_outcome` 86,141;
  - `identifier_unresolvable` 71,261;
  - `check_castable_array_arguments` and `check_impossible_haystack_values`, 48 K each;
  - and others.
- **Names turned into text:**
  - `statement_assigned_variable_names` 214,698;
  - `resolve_as` 204,105 and `qualify` 146,264, with `parse_namespace_stmt` 26,045;
  - `canonical_class_name` 114,838 and `resolve_static_class_name` 113,087;
  - `declared_method_name` 83,737, `lookup_method_with_owner` 76,924 and `find_method_receiver` 64,351;
  - the canonical method names copied in `static_method_through_instance` and `check_static_call_context` (≈84 K);
  - `literal_string_key` 44,746 (the checker's 41,632 and `global_vars`' 3,114), and the PHPStan-API walker's class names (44,134).
- **Template bindings by `String`:** `class_template_bindings_rec` 328,384, `substitute_class_templates` 80,734 and `class_template_params_for` 34,781.
- **The tail's text copies (M11, F5: 102,938 in 38 functions).** For example:
  - `check_internal_instantiation` 11,683, `instanceof_outcome` 8,472 and `static_member_of_class` 7,022;
  - the DQL tokenizer's `String` tokens (`dql.rs:172`), and the current class as a `String` in `Promotions` and `check_program`;
  - fixed names built at each call (`"Doctrine\ORM\Query".to_string()`, `member_resolution.rs:1166`), which are interned once.

### 16. Interned values: one allocation each, tables without per-entry allocations (M7)

**Now.**
- **A name takes three allocations to make.** `Name` is an `Arc<NameData>`, and `NameData` holds its text and, when the text is not lowercase, its lowercase form, each in a box of its own (`name.rs:22-49`). The lowercase copy is made for every name and dropped when it equals the text (`name.rs:41-42`). The run makes 186,348 names in 559,042 allocations: 186,348 records, 186,347 texts and 186,347 lowercase copies. At exit there are 135,777 `NameData`, as many texts, and ≈84,410 lowercase copies.
- **The type interner's index costs nearly as much as the types.** It is `FxHashMap<u64, Vec<Weak<Node>>>` (`intern.rs:21-25`), with a `Vec` for each distinct hash: 121,484 allocations and 3.9 MB (`intern.rs:313`), plus 8.7 MB of table, for 119,757 nodes of 112 B (13.4 MB). That is ≈105 B of index per node.
- **A shape's string key allocates each time it is made.** `ShapeKey::from_bytes` builds a `PhpString` (an `Arc<[u8]>`) for every key it is given: 182,591 allocations, 41,300 alive at exit. 103,568 of them copy the bytes of a string-literal type that already holds them in an `Arc<[u8]>` (`ShapeKey::from_type`, `shape.rs:43-48`: `offsets::literal_key` 48,485, `array_literal_key` 45,292, `offset_value_type_at` 9,584, `unset_offset` 207).
- **A second table interns text:** `Interner::str`, a `WeakTable<str>` (`name.rs:271-325`, `:1444`). Inline HTML and doc comments use it today and become source ranges in item 9; the interpolation index keys stay (`lexer/strings.rs:127`, `:130`).
- **The type memos hold 2^16 pairs per thread and are emptied whole when full:** `remember` (`intern.rs:684-705`) and `keys_superset_memo` (`union.rs:995-1025`), as item 7 says. Their entries hold their argument nodes, which an interner that frees could then not free: 5.4 MB and 1.5 MB at exit.

**Design.**
- **A name is one allocation.**
  - A header holds the text hash, the folded hash, the length, the offset of the last segment (item 15's short names), and where the lowercase form starts, or that the text is already lowercase.
  - The text follows the header in the same block, then the lowercase form when it differs.
  - It is a `triomphe::ThinArc<Header, u8>` (0.1.16), built by `from_header_and_slice`: one pointer wide, header and bytes in one block, with no `unsafe` of ours. A thin dynamically sized value written by hand would need it. Rowan holds its green nodes the same way.
  - Equality stays identity, through `ThinArc::ptr`.
- **The tables hold strong entries, as rust-analyzer's interner does** (`ra_ap_intern`'s `Interned`).
  - `triomphe` has no weak references, so the tables cannot hold `Weak`s as today's do.
  - A handle whose drop sees a count of two (itself and the table) takes the table's shard lock, checks the count again, and removes the entry. A lookup clones its entry under the same lock, so it cannot revive an entry being removed.
  - **Two handles dropped at once can both see three,** so neither removes the entry, and only the table holds it. rust-analyzer's `Interned::drop` has the same window. So a shard also sweeps the entries whose count is one once it has grown to twice what its last sweep left, as `WeakTable::sweep` does today.
  - **An entry is removed under the lock and dropped after it is released.** A type node's drop drops its children's handles, each of which may take a shard's lock, the same one included.
  - An entry is hashed by the hash in its header, so it is one pointer, with no stored hash. The type interner loses its per-hash `Vec`s.
  - The type interner's nodes become `triomphe::Arc`s on the same rule, which also removes the weak count from every node.
- **`PhpString` is the interned byte string:** one `ThinArc<Header, u8>` per distinct value, with the text hash in its header, under the same strong-table rule as names.
  - The type interner already hash-conses string literals by their bytes (`ReprRef::StringLiteral`, `intern.rs:621-624`), so a separate table of shape keys would hash the same bytes again, and every conversion between a key and a type would cross between the two tables.
  - `Repr::StringLiteral` and `ShapeKey::Str` hold the same handle, so `from_type` and `to_type` copy a pointer.
  - `ReprRef::StringLiteral` borrows `&[u8]`, so a literal built from source bytes is looked up without making a `PhpString`.
  - Equality is identity; `Ord` and the serialized form stay by bytes.
  - The interpolation index keys (`SimpleIndexKey::Str`) are these handles. `Interner::str` and `WeakTable` go.
- **Every holder of a node or a name holds the handle whose drop runs the rule:** `Type`, `Name` and the interned `PhpString`. That includes each thread's `FRONT` cache, which holds 4,096 raw `Arc<Node>`s today and overwrites them as the thread goes (`intern.rs:257-285`), the memos' kept arguments, and the lexer's tables. A raw holder's drop would leave an entry for the sweep alone, and `FRONT`'s slots turn over constantly. The constant nodes (`Constant::node`, `intern.rs:225-245`) are held for the process and never reach a count of two.
- **The type memos keep their answers, bounded as item 7's memo is.** Their answers depend on the interned types alone: `memoized_pair` is "only for an operation that asks nothing but the types (no class hierarchy)" (`intern.rs:715-717`), and the shape memo answers over the structural relation. So no generation enters them. They take two segments of 2^15 pairs per thread, the older dropped when the current one fills, in place of emptying the whole table. The bound is also what limits the dead nodes they keep from the interner.

**Oracle.**
- O2, `--dump-types`, `registry-dump`.
- **The drop, lookup and sweep protocol is model-checked with `loom`** (0.7.2, a test dependency), which tries every interleaving, over a model of a shard and its counts: the count read before the lock, the re-check under it, and the sweep's count-of-one test are windows a few instructions wide that tests on real threads seldom land in. crossbeam and tokio check their protocols this way.
- **A leak test after item 0's edit replay:** the session is dropped, every shard must hold only the constant nodes and what `FRONT` keeps, and a second replay must end with the same shard sizes. O2 shows output, not freeing, so a leak in a long session would pass it.

Exact.

**What stays.** One allocation per distinct live value: that is what interning with freeing costs. rust-analyzer's `intern::Interned` pays the same. A per-session arena that never frees, as rustc's interners use, would grow without end in a server, so the plan keeps freeing.

**Saving.** ≈0.1 G, estimated: the ≈0.5 M allocations it removes, with their frees, and the weak upgrades on every lookup. This profile does not separate the interner's probing from its callers'.

**Memory.** ≈15 MB at exit.

**Allocations: ≈0.78 M (M10).** What goes:
- **two of the three allocations of every name still made** after items 4, 5, 13 and 15: 249,674, of which the lexer's 176,168, `parse_qualified_name`'s 39,140 (moved here from item 12's row, which keeps its 19,570, one per new name) and the cache loads' 29,312 (`Name::deserialize`). The makers those items remove outright keep all three allocations in their rows: `RefKey::class_text` 101,880, `RefKey::function_text` 22,161, `file_declarations::collect` 17,760, `name_table::Base::name` 17,472, `KeyTable` 10,066, `member_usage::method_key` 8,904, `folded_key` 6,288;
- the type interner's per-hash vectors, 121,484;
- the shape-key copies of bytes a literal already holds, 103,568, and the keys from source that already exist.

What stays of the row: one allocation per new name (the lexer's 88,084 among them), and the lexer's table growth (`lexer.rs:435`, 5,911, and the interner's own).


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
   - A per-project index, written at the end of a run, maps each context file's path to its stat fields and content hash.
   - **The fields.** On Unix: size, mtime and ctime in nanoseconds, inode and device. On Windows: size, `last_write_time` and `creation_time` from the directory listing, with no handle opened, as git for Windows compares. `std` keeps a file's volume serial and index unstable there (`windows_by_handle`), and getting them means opening each file, which costs nearly what the read saves.
   - **A recorded hash is reused only when it could not have been racy.** A file's hash is recorded as reusable only if its mtime and ctime are older than the moment the run read it, on the local clock, by more than its filesystem's timestamp granularity; otherwise it is read and hashed next run too. This is git's rule for racily clean entries, with the comparison made where the file was read: git compares with its index file's own mtime, which lives on the same filesystem, and this index lives in the cache directory, which may not. A restored mtime (`tar x`, `cp -p`, `rsync -t`, `touch -r`) still changes the ctime, which is why both are compared, as git does by default (`core.trustctime`).
   - **`stat` is trusted only where timestamps are fine and local.** The filesystem's type comes from `rustix`'s safe `statfs` on Linux (`f_type`) and macOS (`f_fstypename`), and from `sysinfo`'s volume list on Windows (0.39.6, `Disk::file_system`). ext4, xfs, btrfs, zfs, tmpfs, overlayfs, APFS, NTFS and ReFS are trusted. On FAT and exFAT (2 s), HFS+ and ext3 (1 s), NFS and SMB (the server's clock), 9p, virtiofs and FUSE (WSL's `/mnt/c`, Docker Desktop's bind mounts), and any filesystem that cannot be identified, files are read and hashed: an edit within one tick that keeps the size would be invisible there.
   - Analysed files are read anyway.
   - **Every content hash is BLAKE3** (`blake3` 1.8.7), and every key on disk is its full 256-bit digest: this index's, the base's key, item 1's hash in `FileState`, item 11's check, the per-file cache's keys (today XXH3-128 of a file's bytes and path, `file_cache.rs:164-176`), and the build's own hash and item 6's `SOURCES_HASH` (today `DefaultHasher`, 64 bits, whose algorithm `std` leaves unspecified across releases, `build.rs:7`, `:62-77`).
     - A cache must never answer for other content. XXH3 makes no claim against inputs crafted to collide. A cache directory shared between CI runs of untrusted pull requests is where a crafted file could inherit a clean file's results, and there the attacker writes both files, so a 128-bit digest would leave only 2^64 collision resistance.
     - BLAKE3 is cryptographic. It picks its SIMD at run time behind a safe API, NEON included, and builds its assembly with `cc`, which the release builds already run for `pcre2-sys`.
     - An estimated few tens of ms of CPU over the under 70 MB of source a cold run reads, spread over the workers. A warm run hashes only what `stat` cannot clear.
   - **The server writes neither the index nor the base from its overlays.** Under item 15, an open document's overlay can differ from the disk bytes (an unsaved edit, or a document analysed as UTF-8 because no encoding matched), so the next CLI run would find the disk file's stat unchanged and take the overlay's hash and declarations for it. The index and the base record only what was read from disk: the server writes the base at its initial load, before applying overlays, and updates index entries only for files it reads from disk (watched-file events, `didSave`). An overlay's products may go into the per-file cache, which is addressed by content.
2. **The per-file cache's key covers everything a parse reads.**
   - Today it covers the bytes, the path, `is_stub`, `index_use_sites` and the build (`file_cache.rs:164-176`, `:69`).
   - Two items make a parse read more. Item 10 reads a file as the newest configured version reads it, which decides which declarations exist (`#[Attr] function f() {}` on one line is a comment before 8.0; `enum` and `readonly` are names before 8.1), and checks the constructs the configured versions read otherwise. Item 12 reads `<?` by `short_open_tag`.
   - So the key adds the configured version set and `short_open_tag` as resolved (on, off or unknown). An entry parsed for one configuration is never served to another.
3. **A resolved context base**, as the stub base is resolved.
   - **What it holds:**
     - the registry after the finishing passes over the context declarations alone;
     - the reverse indexes restricted to context files;
     - each context file's per-file products (the per-file entry's fields: references, declared names and constants, duplicate sites, line starts, summaries, global facts, declared classes, lexer warnings, Pest facts, anonymous-class names, `FileFacts` and the use-site index, `file_cache.rs:78-98`), less its built declarations, which are in the registry;
     - the resolved snapshot `changed_since` compares against;
     - **its file list,** in load order: each context file's role, path relative to its root, Composer owner and content hash. Absolute roots are joined at load, so one base serves every checkout of the same tree.
   - **Keyed by the build and the resolved inputs the load and the finishing passes read,** not by the context files' contents, which the file list holds. Were the contents in the key, a changed context file would change it, no base would match, and the path that applies changed files could never run. PHPStan's result cache keeps the same split: `getMeta()` decides whether the cache can be used at all, and per-file hashes inside it decide what to analyse again (`ResultCacheManager.php:2133-2148`).
     - **The key is a hash of the resolved `ProjectSettings`, made by destructuring it** (`project.rs:478-520`). A new field fails to compile until it is classed as tracked or untracked, as rustc classes its options `[TRACKED]` and `[UNTRACKED]` for its incremental hash.
     - **Tracked:** the version set and its source (the configuration, or `composer.json`'s `config.platform.php` or `require.php`, `session.rs:232-243`, `:460-464`); `composer.json`'s platform and `require.php`, `installed.json` and the lock file; item 6's run-time evidence (`platform_check.php`'s content hash, the configured extension list, or the configured `php` binary's answers, `php -m`, its version and `short_open_tag`, asked once per run, or recorded with the binary's stat under step 1's rule); the user stub and bootstrap files with their hashes; the active extensions (`ActiveExtensions::resolve`, `session.rs:465-467`); and the rule settings the finishing passes read.
     - **Untracked:** settings the checker alone reads, the level and most rules among them.
   - **Stored as the stub base is:** item 6's container, records read and decoded lazily, names in one table (item 6), ids by record index (item 4). Item 6's step 6 keys it with the build's inputs.
   - **A cold run** inserts the context files, runs the finishing passes, and writes the base. It then inserts the analysed files and finishes through the incremental path, over the changed names' closure.
   - **A warm run** loads the base and compares its file list with the current one: a changed file goes through `update_file`, an added one through the session's reader, and a removed one through `remove_file`; a change of order or role is a change of the list's inputs, applied the same way. It then inserts the analysed files and finishes the closure. What the analysed files change in vendor code (a vendor function calling a project function, the global-variable pass) is re-finished by that closure, as an edit is in a server session.
   - **The language server's initial load is the same load (item 1),** so it uses the base too.
   - **The two caches answer to different keys.**
     - A per-file entry holds a file's parse products, which depend on what step 2 keys.
     - The base holds resolution, which depends on the context files and on the resolved inputs.
     
     When a tracked input changes, the base is rebuilt from the entries whose keys still match, and the rest are parsed.

**Oracle.**
- **Against the baseline:** item 0's dumps (O2 and `registry-dump`), with the base absent, written and read. Comparing a warm run with the new cold run alone cannot catch a closure that misses a dependency, since both would share the error.
- **An edit replay across two runs:** a context file edited, added, deleted and renamed; `composer.json`'s PHP range changed (entries parsed again, step 2); `platform_check.php` or the extension list changed; a rule the checker alone reads changed (the base reused); the context order changed.
- **`stat`'s cases:** a file rewritten within one mtime tick with the same size, and one whose mtime is restored, must both be read again; on Linux, macOS and Windows in CI.
- **The server:** item 0's server session loading from the base, and an overlay that differs from the disk never reaching the index or the base.
- The incremental tests.

Exact.

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
   - a literal's type is computed once per node per pass (`string_literal_ref` 86.0 M);
   - **`process_expr` evaluates in PHP's order,** the order item 13's walk follows, so a rule sees each operand's type in the scope it ran in. In an assignment (plain, compound, `??=`, by reference), first the target's operands (receiver, dims, property and static names), then the value (for `??=`, only on the branch where the target is null), then the store. In a destructuring, the value first, then each target's operands in order. `php` 8.5: `$a[$i++] = $i` gives `[0 => 1]`, and `[$c[$j++]] = [$j]` gives `[0 => 0]`. Today the checker types the value first (`expr.rs:2002-2008`) and the target after it (`:2047`), so it types `$a[$i++] = $i`'s value as `0`;
   - **the member-usage family counts a member as used when any access may reach it:** a receiver whose type may be the class, a dynamic name whose type may be its name, an array or string callable whose type may name it however uncertain, and an access inside a closure the class may rebind. A member is reported only when no access can reach it. PHPStan's `UnusedPrivateMethodRule`, which §12.10 cites for the mechanism, skips an array callable that is not certainly callable (`UnusedPrivateMethodRule.php:154`), so `call_user_func([$this, $name])` with `$name` a `string` lets every private method be reported unused, though any of them may be the one called.
2. **C1 step 5's tail.**
   - Effects are carried in the statement result, so `noop::expr_side_effects`' walk (`noop.rs:51`, 30.7 M) goes.
   - So is `has_yield`.
   - `collect_throws` (`try_catch.rs:398`) gives way to the throw tails C1 keeps.
3. **Variables as slots, scopes as radix arrays: ≈0.15-0.25 G.**
   - The parser numbers each function-like body's variables. A variable node holds its `NameId` and its `VarSlot`, and a closure's `use` list and an arrow function's captures map to the enclosing body's slots.
   - A variable's `LocalKey` is its slot: no name lookup (`KeyTable::find_var` 79.1 M).
   - **Variables PHP can reach by a computed name or a shared scope keep the name lookup.** The parser marks a body that holds any of:
     - `$$x`, `${expr}`, `include`, `require` or `eval`, whose code runs in the function's scope;
     - a static call of `extract`, `compact` or `get_defined_vars`;
     - `call_user_func` or `call_user_func_array` resolved to the global function at compile time (in global code, fully qualified, or imported), with a literal first argument naming one of those three, in any case and with no leading `\`. PHP compiles that form to a direct call: in `php` 8.5, `call_user_func("extract", ["a" => 2])` and `call_user_func_array("extract", [["a" => 4]])` set the caller's `$a`, while a variable callee, a leading `\`, `array_map("extract", ..)`, or an unqualified `call_user_func` inside a namespace throw "Cannot call extract() dynamically";
     - `$GLOBALS` in the global scope;
     - where the configured versions include 7.x: `parse_str` or `mb_parse_str` with one argument, and `assert` with an argument that may be a string, which 7.x evaluates as code (from PHP's manual; all three end in 8.0).
     
     A fixture of each form is checked against `php`.
   - **Every key a scope holds is dense in its body.** Today slots would be numbered per body from 0, while every other key (properties, indexes, `truthy` and `isset` facts) comes from the file-wide `KeyTable` that every scope of the file shares (`KeyTableRef`, `locals.rs:32`): a slot `n` and a table key `n` would collide in one array, and a function late in a large file would hold a few table keys with ids in the thousands. So each function-like body numbers its keys in a table of its own, slots first, then synthetic keys as they are made. An arrow function's table extends its parent's, and a closure's `use` list maps the parent's keys at entry.
   - **The scope's maps index the dense keys.** They become a persistent radix array: 32-way nodes indexed by the key's bits, `Arc`-shared and copied along the path on a write, in place of `imbl::OrdMap` (`get` 72.9 M, `Cursor::next` 34.6 M, `Leaf::insert` 29.8 M).
     - A lookup is two indexings for up to 1,024 keys, with no comparisons.
     - A node that no other scope shares is written in place (`Arc::get_mut`), so only a shared path is copied. The copies that remain are what persistence costs. C4 keeps it because its diff skips every subtree that is the same pointer in both scopes.
     - The diff C4 relies on walks two arrays and skips every subtree that is the same pointer in both.
     - Merging and diffing two scopes walk both tries and skip every subtree that is the same pointer in both, as Okasaki and Gill's mergeable integer maps (`Data.IntMap`) do.
     - Iteration is in key order, which is slot order. Anything that lists locals (`--dump-types` prints every tracked key) sorts by name, so no output depends on numbering.
4. **C2 step 2:** conditional subjects bound by parameter position (`ConditionalSubject::Parameter { name, index }`), which changes the cached layout (the key follows, item 6 step 6).
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
   - **`concat` makes new bytes, not a member list.** It writes the two operands' bytes into a per-thread byte scratch and interns from `&[u8]` (item 16's `ReprRef::StringLiteral`), so a string that exists already allocates nothing.
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
     - `resolve_call_args`: 374,354, of which its callers `apply_param_out_types`, `generic_object_method_return_type` and `infer_builtin_call_return_type` ask for 236,274;
     - `bind_arguments`: 236,963;
     - `check_parameters`: 126,792, 27.3 MB;
     
     738,109 in all, M9's row.
   - The binding becomes part of the call's record in the pass (step 1's one inference per node). Every rule borrows it.
8. **Per-function state is reused.**
   - `check_function_impl` allocates its tables anew for each function (125,654 allocations), and the expression-type cache grows anew too (`cached`: 19,778 allocations, 27.9 MB).
   - These belong to the checker and are cleared, not freed, between functions.
9. **A generic call's signature is substituted type by type.**
   - Today the call copies the whole signature to substitute it (`FunctionReflection::map_types`, `function_sig.rs:372`: 35,574, plus the parameters' copies). The copies of the case references under it (≈38 K: 9,991 directly in `clone_vec` and 27,902 through the parameters' clones) go with item 8, which moves those fields out of declarations.
   - Instead, the call asks for the substituted type of the parameter or return type it reads.
10. **The rules' temporaries (M11).** Every site is in Appendix A.
    - **F1. Messages are formatted only when reported.** Today 37 rules format a subject, a message or an identifier for every element they check. Examples:
      - `check_property_phpdoc_types` formats a subject for every property (`phpdoc_type_compat.rs:377`: 21,919);
      - `check_property_declarations` formats another, and turns each native hint into a `String` to test it for `callable` (`property_declarations.rs:133`, `:142`);
      - identifiers are built by `format!` (`probe_checks.rs:159`, `strict_rules.rs:218`);
      - `check_function_impl` formats each function's labels (`function_decl.rs:282`, `:1117`: 46,005);
      - `check_statement_var_tag_names` and `check_statement_var_tag_classes` format a message or a subject for every `@var` variable (`stmt.rs:529`, `var_tag_types.rs:346`: ≈65 K).
      
      A subject becomes a value that displays itself, formatted by the report, names with item 15's escape. A built-in rule's identifier is a `&'static str` from its rule's table. `Identifier` stays `Cow<'static, str>` (`diagnostics/src/lib.rs:17`), since the public `Rule` trait lets an external rule name its own (`rule.rs:61-93`).
    - **F2. Constant types are built once.** `union_of(vec![Type::int(), Type::float()])` and its kind are rebuilt on every call in 41 functions (`strict_rules.rs:230`, `operators.rs:57`, `:672`, `:685`). Instead they are interned constants, as `types` builds its `Constant` nodes (`intern.rs:43`). A two-member union or intersection takes its two operands with no `Vec`.
    - **F6a. Rules borrow, iterate, or use scratch.**
      - A one-element list is `slice::from_ref` (`type_declarations.rs:182`).
      - A list collected only to be iterated is iterated (`member_resolution.rs:298`, `operators.rs:706`, `offset_assignment.rs:46`, `:50`).
      - A constant list is a `static` slice (`declaration_level_rules`, `internal_rules.rs:56`, rebuilt per call). A list fixed for the file is made once per file (`default_include_dirs`, `includes.rs:380`).
      - A `seen` vector lives in the checker's scratch (`array_ops.rs:70`, `function_like_declarations.rs:266`).
    - **F6c. A scope is one pointer, and throw, exit and loop states are scratch.**
      - Copying a scope already copies only pointers: every map in `Locals` is an `imbl` persistent map (`checker.rs:27-191`), so `locals.clone()` copies about fifteen roots. The cost is that each element of a throw frame is the whole `Locals` struct inline, a few hundred bytes: `record_throw_state`'s 3,920 allocations (16.1 MB, `expr.rs:347-350`) are the growth of a `Vec<(Type, Locals)>`, and `check_try`'s 17.2 MB is the same element type's vectors (`try_catch.rs:99`, `:124`). Step 3 alone leaves these numbers as they are.
      - `Locals` holds its maps behind one `Rc`, so a copy is one count and a throw frame's element is two words.
      - Throw frames, catch collections and the per-loop break and continue states (`control_flow.rs:140-141`, `try_catch.rs:84`) are scratch stacks of the checker, truncated, not freed, when their construct ends.
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

**Oracle.**
- O2, its diff read family by family.
- `--dump-types` and `--dump-expr-types` byte-identical through §12.10's steps 1, 3 and 4, and through step 3 here, with locals listed by name (25 G §12.10).
- The replay assertion: a test build runs an observed pass from each fixpoint entry and asserts that its reports equal the replay's (25 G §12.10).
- §12.11's differential test over random member lists and every union the corpus run builds, comparing the n-ary result with the left fold; its differences are listed before step 5 lands, since a fold and one pass can differ.
- A debug assertion that the radix arrays' merge over the diff equals the full merge (C4's, for the new structure).
- Fixtures for the order of evaluation, the computed-name forms and the member-usage family, checked against `php`.

Semantic, listed in §11: §12.10's families, the order of evaluation, the member-usage family, and §12.11's single pass where it differs from the fold. Exact otherwise.

**Memory and allocations.** The check phase makes 3.61 M allocations (M10), the `@var` messages F1 defers included and the case-reference copies of step 9 given to item 8. What they leave:
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
  - architecture: the use sites within each class's span and the `new` sites; the rest it reads from the registry;
  - **banned constructs** has no whole-program part: it reports per file, and its walker reads the registry (`banned.rs:359`), so it is a family of item 18's traversal, with the ignore lines from the file's ignore marks;
  - **class shapes:** the declared classes and their members' spans, from the declaration list, and for each method with a body a parent-call fact made by item 13's walk: whether the body calls `parent::<own name>()`, whether its first and last statements are that call and whether it is the only one, and the fix's insertion point and indentation. Today `parent_call_rules` reads each method's body for these (`class_shape.rs:311-420`);
  - **Composer hygiene:** per file, item 13's walk records the string literals (as `PhpString`s, for class names written as strings), the probed extensions and symbols, the use sites and the declared classes with their spans. The whole-program part reads them in path order, as today, since it depends on what earlier files used (`used_before`, `composer_hygiene.rs:285`, `:447`);
  - **mock hygiene** is per file and syntactic (`mock_hygiene.rs:366-385`), so it is a family of item 13's walk that reports per file;
  - unmatched ignores: the trait declarations' line ranges, from the declaration list;
  - the use-site index: item 1's per-file product.
- **Retention stays one function,** item 3's extended: `keeps_tree(role, open, mode, checked)`. Context files never keep their tree; analysed files keep it in a batch until their check ends; in the server, open documents keep it always and other files through the tree cache. The load, `update_file` and the check all ask it.
- **A batch run frees each analysed tree when its file's check ends.**
  - The check takes the tree out of the file's state and owns it for the length of the job.
  - **Each worker takes the largest file left.** Today `map_on_pool_largest_first` sorts the files and then runs `par_iter().with_max_len(1)` (`project.rs:460-468`). rayon splits that in halves: the first worker starts at the largest, and a thief takes the oldest pending half, so the second worker starts at the middle of the list. That order is neither largest-first for freeing nor longest-first for wall time. Files are taken from a queue in descending cost instead: one `spawn_fifo` per file, in sorted order, within a `scope_fifo`, or a shared atomic cursor each worker advances.
  - **The private-property judgment keeps every write PHP can make.** A private property is written from outside its class: in `php` 8.5, `Closure::bind(fn..., $o, C::class)()`, `(function () { $this->p = 7; })->call($o)` and `(new ReflectionProperty(C::class, 'p'))->setValue($o, null)` each set `C::$p` from another scope, and each works from any file. Today `judge_property_types` gathers writes only from the files declaring the class or a trait it uses (`project.rs:2185-2220`), on the premise that "only for those can every write be seen" (`property_type_too_wide.rs:8-9`), so a type only such a write assigns is reported as never assigned.
    - The writes recorded during the checks include those made in closures bound or called with an object or scope that may be the class (`Closure::bind`, `bindTo`, `call`, `fromCallable` on a bound closure), and `ReflectionProperty::setValue` on a name that may be the property. A file whose facts mention rebinding or reflection (`MENTIONS_REBIND`, `project.rs:2289`) contributes them.
    - Where such a write's class or property cannot be narrowed, the properties it may reach are not judged.
    - Files outside the batch that declare a writer class or a used trait are parsed again through `ast_of` and run through item 18's traversal with only the property-write family registered, on those classes' declarations only. Today they get a full check, every rule, for their writes alone (`project.rs:2224-2228`).
- **The server keeps the trees of open documents, plus a cache of recently checked files bounded by bytes.**
  - The cache is `quick_cache`'s weighted cache, weighing each tree by its arenas' bytes. It replaces `AstCache`'s 32-entry `VecDeque` (`project.rs:170-192`).
  - rust-analyzer applies the same policy to its parse trees, but bounds them by count (`lru.capacity`, 128 files by default), whatever their size.
  - **The cache is keyed by `(FileId, content hash)`,** so a file an edit changed never meets an older tree. Open documents' trees are held outside it, and their text comes from the overlay encoded with the document's encoding (item 15).
  - Any other tree is parsed again on demand through `ast_of`, from text read on demand and checked against the hash item 1 keeps.
  - **A re-read whose hash differs from the recorded one is an edit.** A closed file can change on disk before its watched-file event arrives, or with none (an editor outside the workspace, `git checkout` under a slow watcher). Its new tree would disagree with its recorded declarations, references and node ids (item 13), and a recheck would mix the two. So the file goes through `update_file` first (declarations, references, dependents), and is then checked from the new tree.
  - An edit that rechecks a dependent file parses that file again if its tree was evicted.
- **No reader gets an empty file.** `FileState::ast` goes, and every reader asks `ast_of`, which parses again when the tree is gone.

**Oracle.**
- O2's whole-program diagnostics, and each moved pass's output compared with today's over the corpus, pass by pass.
- The server session and its edit replay (item 0), once more with the tree cache's budget at zero, so that every recheck parses again and a reader that still depends on a kept tree shows.
- A closed file changed on disk with no event before a dependent's recheck.
- Fixtures of `Closure::bind`, `bindTo`, `call` and `ReflectionProperty::setValue` writes to private properties, checked against `php`.
- The `oracle` build, whose assertions every dump must pass.

Exact, except the private-property judgment, listed in §11.

**Memory.**
- **In a batch run,** the analysed trees are freed as the check proceeds rather than kept to the end: 173 MB at exit today, about half that after item 9.
- **The peak** becomes the start of the check plus the checks in flight: the largest files' checks, which run first, while every tree is still alive. That load has not been measured: at today's maximum, during the parse, the checker holds almost nothing (§8, M1). Item 0's memory oracle records the heap at the start of the check and at the check's maximum, and §12's projection takes both.
- **In the server,** the trees held are the open documents' and the LRU's, not the workspace's.

## 11. Validation at the end of each item

**When.** An item is validated when its last row lands, before the next item's first row. Between rows only `cargo fmt`, `cargo check` and clippy run. A failure is fixed in its own commit, within the item, and only what failed runs again, from the saved binaries where they still apply. Item 0's validation is its baseline (item 0, step 4), with the tests and the fixtures. Two items have steps that wait for item 9: item 3's scratch store for item 9's step 2, and item 6's step 2 for the stub rules' versions (item 9, step 5). Each is built right after the step it waits for and validated with item 9, whose comparison then also admits item 6's step-2 changes.

**Against what.** Each item's dumps are compared with the previous item's validated ones, item 1's with item 0's baseline. So every difference belongs to the item being validated, and it must be one of the semantic changes that item names (below); an item that names none gives identical dumps. Each item's dumps are saved by its last commit's full hash, as `xtask` lays them out (`.corpus-runs/gate/<artifact>/<commit>.<ext>`, `.corpus-runs/README.md`). Contents are diffed after `LC_ALL=C sort`, and the order of emission is checked apart, by `xtask compare`.

**How it runs.** One heavy job at a time, each under `nice -n 19` with `/proc/loadavg` noted, with no `timeout`, in this order; a job may use the whole machine (`cargo xtask progress --watch` shows the pass as it runs):
1. the `gate` and `oracle` builds;
2. the tests: `cargo test --workspace` and the tests the item wrote (the fixtures run in step 4);
3. the fixtures against `php` 8.5, and, for an item with 7.x readings, against `php:7.4-cli` through `docker`;
4. the pass of item 0's step 3 with the `gate` build (`cargo xtask corpus --full`): every configuration in each of its states, every artifact compared across them and with the equalities that exist by then, the server's sessions, the edit replays, the vendor projects and the fixture suite, recorded by commit;
5. the same pass with the `oracle` build, in every state, the per-file cache written and read included; it records nothing and must finish without an assertion;
6. the differences from the previous item's snapshots, read line by line;
7. the item's state in the ledger and its one-line state in `WORK_STACK.md`.

**What an item adds to its own validation:**
- **item 0:** the runtime oracle with generated calls on every version image from `php:7.0-cli` to 8.5, with no fixture opted out for an assumption the assumed types remove; the recorder on every image as the PHP catalog's oracle, the iterators' states and the construct forms included.
- **item 1:** the server's first dumps against the CLI's join the equalities;
- **item 6:** its tests (a file replaced while a reader holds it, a corrupt record, and `SOURCES_HASH` against the embedded files); the platforms job, below, with the file-replacement test;
- **item 7:** the relation memo on against off joins the equalities;
- **item 10:** the versions check's 7.x readings through `docker`;
- **item 12:** the spaced names' 7.x readings through `docker`;
- **item 14:** the lowering oracle, which compares with the text grammar the item deletes, so it reads what item 0 recorded: each type node is lowered again and compared with the type today's round trip gave it;
- **item 15:** a Latin-1 document opened in the server session;
- **item 16:** the `loom` tests of the interner's protocol, and the interner's leak test after the edit replay;
- **item 17:** its `stat` cases, here and on the platforms job; the per-file cache written under one configuration (the versions, `short_open_tag`) and read under another, against that one's cache-off run (step 2); the base absent, written and read; its edits between two runs, as the item lists them; and a warm run, which leaves the cache directory byte for byte as it found it;
- **item 18:**
  - the union oracle, which compares with the fold the item deletes, so it reads what item 0 recorded: each union is built again from its operands and compared with today's result (§12.11's differential test);
  - the `oracle` build carries the replay assertion and the merge-over-diff assertion;
  - one-argument `parse_str` and `mb_parse_str`, and string `assert`, read on 7.x through `docker`;
  - `--dump-types` and `--dump-expr-types` with locals listed by name, since §12.10's step-by-step identity cannot be checked within one item's validation;
- **item 19:** each edit replay also at a tree-cache budget of zero; a closed file changed on disk with no event before a dependent's recheck.

**Platforms.** CI runs on `ubuntu-24.04` only (`rust-ci.yml:18`), while the release builds for `macos-26` and `windows-2025-vs2026` (`release.yml:33-53`). A CI matrix job on those two runs item 6's file-replacement test from item 6's validation on, and item 17's `stat` cases from item 17's, and each result is recorded.

**The semantic changes, by item.** At an item's validation, every difference from the previous item's dumps must be one of its own:
  - **item 0:** a builtin runs and writes what its php-src implementation may make it run and write, on the configured versions (the PHP catalog), where it ran what a recording on 8.5 saw and every override of its own object's methods, and wrote only its by-reference parameters; a builtin no source covers writes anything; a dead function or class declared twice is reported at each declaration, and a namespace cycle is reported at its first analysed class, where it went unreported when its first class was a context or stub one (fast validation, step 1); the fixes found in step 1: what a call may write, by storage, forgotten after it (object properties by field and type, static properties, globals, static variables, captures and references), calls bounded by the class hierarchy and builtins by what they call, captures by reference entered with every type written while shared, and `array_reduce` with its initial value; the one found in step 2: past the union cap, class-strings fold to one class-string and callables to `callable`, where the union was `mixed`; and the one found in item 0's validation: a stub no run of PHP has seen, declared pure and taking no code, writes nothing;
    - the implicit calls: what a magic method, a property hook, a lazy object's initializer, a destructor (at every release, and through the cycle collector at every release that may leave a value collectable), an autoloader, an error handler, an output-buffer callback, a tick or signal handler, or an included file may write is forgotten where PHP may run it; a generator function's call writes nothing of its body, the generator's resumptions write what the body may write, and its release what its `finally` blocks write;
    - the assumed types:
      - int operations give the float PHP gives past `PHP_INT_MAX` or `PHP_INT_MIN`, as literals where its values are few, with what PHP does with the float;
      - an open end of an int range is its limit, so `int<0, max>` and `int<0, PHP_INT_MAX>` are one type;
      - comparisons narrow ints exactly;
      - an array's count is bounded by the engine's maximum array size;
      - an array read at a key not proven present gives `null` too, and is reported where PHP raises a warning (a notice before 8.0); the same holds for string offsets; `report_possibly_nonexistent_general_array_offset` goes;
      - a nested write at such a key makes the element from nothing;
      - a property read where the type leaves the property's presence unknown gives `null` too, and is reported where PHP warns;
      - every builtin returns and throws what the catalog finds on a path its arguments may take, where its stub said less;
      - a builtin iterator's `current()` and `key()` off a valid position give and throw what PHP does, and exactly the stub's types where `valid()` proved the position.
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
  - item 9: the stub rules apply only to stub sources;
  - item 10: `__COMPILER_HALT_OFFSET__` as its file's offset, with the new check of its use elsewhere; `__halt_compiler` followed by anything but `( ) ;` or `( ) ?>` is a syntax error; a broken file's error is its first one; a declaration's doc is the one PHP gives it, with the new check of a doc given away; and the new check of source the configured versions lex differently;
  - item 11: lines end at a lone `\r` as at `\n`, and so does a `//` or `#` comment;
  - item 12: casts as PHP's single tokens; spaced qualified names a syntax error on 8.x; doc comments only with whitespace after `/**`; `?>` and heredoc lines ending at a lone `\r`; heredoc indentation errors; `<?php` open only before whitespace, and `<?` by `short_open_tag`, with a report at each short tag when the setting is unknown;
  - item 13: declarations exist as PHP binds them: run-time-bound ones are guarded by every earlier exit (`exit`, `die` and `throw` included) and are dead after an unconditional one, and a class whose parent is in another file is possibly declared; a file that defines constants it cannot name (`define($name, ..)`, a name built from the namespace, `call_user_func('define', ..)`, `('define')(..)`) may define any constant, so fewer constants are reported as not found; `InitFlow` in PHP's order of evaluation;
  - item 14: the readings the lowering oracle lists (escaped quotes, `mentions_mixed` from the node, identifiers by their bytes, a `//` inside a literal type, `Integer::MAX`), and one winner rule for every tag (`@phan-real-return`, `@phan-param-out`);
  - item 15: names that differ only in bytes outside UTF-8 are distinct (the lexer keeps the bytes, item 9) and print with `\x{NN}`; names fold ASCII only where Unicode's lowercasing was used; string values name classes, functions and constants by PHP's rules (one leading backslash, no trimming); container-field keys canonical as PHP's array keys; in the server, an open document read as the bytes its encoding gives, and one whose encoding is not found analysed as UTF-8, with a diagnostic; a fix writes a name's bytes;
  - item 18: the rule families of §12.10; the rules that read the statement result where it differs from today's side walks (`finally` overrides, `try`/`catch` and throws, no-op statements, loops' constant conditions); types in PHP's order of evaluation (an assignment's target operands before its value, a destructuring's value first); a member counted as used when any access may reach it; the union's single pass where it differs from the fold;
  - item 19: the private-property judgment counting writes from bound closures and reflection, and leaving unjudged the properties an unnarrowed one may reach;
  
  Each difference is listed with its file and read.

**Equalities that need no baseline,** each run at every item's validation from the item that brings it on:
- one thread against the default;
- the stub cache cold against warm;
- the per-file cache off against written and read; from item 17, also written under one configuration and read under another, against that one's cache-off run;
- from item 17: its base absent, written and read; its edits between two runs; and a warm run, which leaves the cache directory byte for byte as it found it;
- from item 7: the relation memo on against off;
- from item 1: the server's first dumps against the CLI's, with no open document that differs from its file;
- each edit replay against a fresh load of the edited tree; from item 19, with the tree cache's budget as configured and at zero;
- from item 19: a closed file changed on disk with no event before a dependent's recheck.

**O3, once, after item 19's validation,** with the commands `CLAUDE.md` gives:
- callgrind with `--separate-threads=yes` on the frame-pointer `dhat` build, over `cl-20g`'s workload;
- `perf record -F 499 -g` on the same build, read with `--no-inline`, of a warm run (per-file cache and base), for the serial windows;
- `hyperfine -N --warmup 2 --runs 10`, the starting commit's `gate` binary against the last commit's, alternating; warm with both caches warm, and cold with `--prepare` emptying both binaries' cache directories;
- **the heap:**
  - the dhat profile in §8's build, compared by holder with §8's at the peak and at exit, and its allocation count;
  - **the check's own maximum,** which one dhat profile cannot isolate: it keeps one global maximum, today's falls in the parse, and `HeapStats`' maximum cannot be reset. So the `dhat` build records `HeapStats::get().curr_bytes` at the start of the check and after each file's check, and writes `5` to `/proc/self/clear_refs` before the check and reads `VmHWM` after it, which give the check's live and resident maxima;
  - **the check's holders:** a second dhat run starts its profiler (`dhat::Profiler::new_heap()`) at the check's start, so its global maximum is the check's own, by holder;
  - the server's heap on item 0's session;
- the results in §12 and in `WORK_STACK.md`.

## 12. Summary

| # | item | saving (this profile) | serial / wall | memory (§8) | kind |
|---|---|---|---|---|---|
| 0 | baseline dumps, debug oracles retired; the PHP catalog, the implicit calls, the assumed types | debug builds and tests | — | — | semantic: the fixes of step 1, the catalog, the implicit calls, the assumed types (§11) |
| 1 | one session: load, configuration, analysis, identity | the server and the tests get every load item | load off the server's message loop | server: the context trees (≈420 MB) and the unopened files' sources | semantic (§11) |
| 2 | parallel walk, path data | ≈0.3 G | ≈0.15 s serial start | the second copy of each path; 0.56 M allocations | exact, except link cycles |
| 3 | context trees dropped on the worker | ≈0.16 G | 0.58 G off the serial path | peak −≤209 MB alone (the end of the parse −≈420 MB); context reads, 66 MB | exact |
| 4 | registry by ids, declarations never changed after build | ≈0.9 G | ≈0.4 s serial | ≈90 MB at exit; ≈1.5 M allocations | exact, except member order |
| 5 | parallel merge, stable ids, posting lists | ≈0.06 G | 0.15 G off the serial path | ≈5 MB; 0.11 M allocations | exact |
| 6 | stub scope, records read on demand with per-record CRCs | ≈0.35 G, then more of `warm` | partly | ≈40 MB of heap; 0.45 M allocations | exact (1, 3, 5), semantic (2, with the stubs' versions) |
| 7 | one relation memo, by content generation | the registry-backed part of ≈0.47 G, not separated in this profile | — | bounded as today; `KEEP_DECISIONS`' 1.3 MB goes | semantic for constants on a cycle (§11) |
| 8 | case references in the checker, declarations built lean | ≈0.1 G | — | both caches' entries; 0.52 M allocations | exact, except one `@property` case |
| 9 | typed vectors, ids, `peeled` | ≈0.8-1.2 G | — | ≥53 MB of the analysed trees; 3.92 M allocations | exact, but semantic for the stub rules |
| 10 | flat token arrays | ≈1.7 G, less the configured versions' check where they read a construct differently | — | 0.23 M allocations | exact, but semantic for the halt offset, the first error, declarations' docs and the versions |
| 11 | line starts, columns, offsets, ignore marks | ≈0.2 G | — | 0.07 M allocations | exact, but semantic for a lone `\r` |
| 12 | lexer details, qualified names and casts as one token | ≈0.35 G | — | 0.45 M allocations | exact, but semantic for PHP's tokens (§11) |
| 13 | declaration list, one walk, builders in scratch | ≈0.45 G | — | bodies as one slice per file; 1.31 M allocations | exact, but semantic for guards, constants defined under names a file cannot spell, and `InitFlow`'s order |
| 14 | PHPDoc finished, one parse, two memos | ≈0.5 G | — | ≈43 MB; 2.59 M allocations | exact, but semantic for the readings §11 lists |
| 15 | names as names, as bytes | ≈0.6 G | partly | 2.76 M allocations | exact, except the changes §11 lists |
| 16 | interned values in one allocation | ≈0.1 G | — | ≈15 MB; 0.78 M allocations | exact |
| 17 | warm runs | warm: decoding and reads ≈0.85 core-s and most of the finishing window; a cold run pays one incremental finishing over the analysed files' closure, and every run one `stat` per context file | yes | only decoded records in the heap | exact |
| 18 | check phase remainder, rule temporaries | ≈1.6-2.0 G | — | 3.61 M allocations | semantic (§12.10's families, the statement result, the order of evaluation, member usage, the union pass) |
| 19 | each tree until its last reader | — | — | batch: the analysed trees freed during the check; server: open documents and a cache weighted by bytes | exact, except the private-property judgment |

**Instructions.** The rows sum to ≈8.2-9.3 G of this run's 20.57 G (items 3 and 5 at their instruction savings, item 7 at none to ≈0.3 G, items 9 and 18 at the ends of their ranges). Some costs are counted by two rows, and this profile does not separate the shares:
- 3 and 9, the tree drops;
- 9 and 10, the parser's allocation;
- 12 and 15, the qualified names (item 12, step 8 is item 15's round trip);
- 14 and 15, the doc identifiers;
- 16 and 18, interning (`intern::insert`, 521.2 M, which both make cheaper).

So the net is below the sum by an amount §11's callgrind measures; roughly 8 G. The serial time that goes with them: the finishing window's 0.41 s, the insert loop's 0.58 G of drops and its 145 M of inserts, and the start's 0.15 s.

**Memory.**
- **At exit,** the 657 MB the project holds loses:
  - the analysed trees (173 MB, item 19);
  - the passes' copies and string tables (≈90 MB, item 4);
  - the memos' ≈43 MB (item 14);
  - the stub records' ≈40 MB (item 6);
  - the interners' ≈15 MB (item 16);
  - the per-file tables' ≈7 MB (items 2 and 5).
  
  That leaves ≈290 MB.
- **The peak** moves from the end of the parse (866 MB), estimated at the start of the check: the ≈265 MB live after the finishing passes (the ≈290 MB at exit less the checker's 25.7 MB of results, §8), beside the analysed trees, at most 120 MB after item 9 (its ≥53 MB of unused capacity gone from 173 MB), ≈85 MB if its padding estimate holds. That is **≈350-385 MB**, plus the checks in flight, the largest files' first (item 19). The finishing window, which holds the same products plus item 4's pass tables while they are computed, is the other candidate. §11's heap oracle measures both: the global maximum and the check's own.
- **Allocations.** M10 gives every one of the run's 19.4 M allocations to the item that removes it. What each row keeps, estimated from what its design still allocates:

  | item | what stays | today | after |
  |---|---|---|---|
  | 18, the check | shared persistent-scope paths copied on write; per-text analyses once per text; reported messages | 3.61 M | ≈0.5 M |
  | 4, the registry | the declarations being built; the owner tables' entries (item 4: about 1.5 M of its 2.02 M goes) | 2.02 M | ≈0.5 M |
  | 14, PHPDoc | a handful per distinct docblock and tag line | 2.59 M | ≈0.1 M |
  | 13, a file's products | a few vectors per file | 1.31 M | ≈0.05 M |
  | 16, 10 and 6 | one per new type node and new name; the stub records a run reads | 1.46 M | ≈0.4 M |
  | 9, 15, 2, 8, 12, 5, 3, 11 and the rest | kept trees' arenas, per-file vectors, one binding map per generic resolution (item 15's template bindings, ≈0.3 M of maps built today) | 8.46 M | ≈0.3 M |
  | **total** | | **19.44 M** | **≈1.85 M** |
  
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
- **The language server** today holds the 657 MB, plus ≈420 MB of context trees and the sources: about 1.1 GB for this workspace. After items 1, 3 and 19 it holds about the batch run's end state: the trees of its open documents and of its cache weighted by bytes instead of every analysed file's, with memos bounded by weight (item 14).
- **What is measured and what is estimated.**
  - **Measured** in this profile: the instruction totals and their parts, §8's holders at the peak and at exit, and M10's rows.
  - **Estimated** from the items' designs: every saving, every memory figure after an item, every residual, the peak after, and the bytes allocated after. The item-by-item review lowered several (items 3, 4, 5, 6, 14) and raised the allocations left.
  - **Not counted,** so the exit figure is conservative there: the memory items 8, 11 and 13 free (case-reference lists out of declarations; no column tables for context files; one body slice per file in place of two `Arc`s per body, 35.8 MB today).
  - dhat counts requested bytes; the resident size adds mimalloc's overhead. §11's dhat runs and callgrind are the measure.

## 13. How the plan was audited

The plan was:
- checked eleven times, item by item, for whether each design was the most proper form or stopped short;
- then rewritten;
- then given the heap profile's findings;
- and was then reviewed adversarially, one item at a time, in order, by a separate reviewer, through §13. Each of the reviewer's findings was checked against the code, `php` and the profiles before it went in.

All the corrections are in the designs above. The eleven passes, in outline:

| pass | angle | what it changed | superseded by |
|---|---|---|---|
| 1 | each design against the code and the 25 G designs | items 4, 9, 11, 13 and 14 extended to their complete forms; the gaps of §2 given designs | — |
| 2 | remaining costs | the arena before the token arrays; the old collector found still in release builds (13); P7's full resolution memo (14); results by id (5); case references (8) | the arena: item 9's typed vectors |
| 3 | soundness of the arena and of the names | names out of the bump arena, the frozen arena, exact-size slices (9); the names kept by the file (9, 10); SSE2 as the baseline (11, 12); the identifiers copied as text (15); the debug oracles (0) | the arenas: item 9's typed vectors and `NameId`s; SSE2 by hand: `wide` (11, 12) |
| 4 | every cost over 60 M placed against an item | the stub cache's name table (6); interner round trips (15); path data from the walk (2); the relation memo (7) | — |
| 5 | whether each can be built as written | the relation memo's generation (7); level-parallel passes that only read (4); sharded tables, ids by prefix sum (5); scratch streams for fragments (10) | fragments: item 10's inline string parts (only the stub rules' attribute strings keep a scratch stream) |
| 6 | lifetimes and shared paths | what outlives the tree (13); the server's index from the loaded bytes (11); one owner implementation (2); the identifier lookup's real costs (12); member order (4) | — |
| 7 | the cached run's parse window | warm runs (17) | — |
| 8 | the cached run's start | the parallel walk feeding the parse (2) | — |
| 9 | the plan against itself | what the context base must hold; id ranges for it (17, 4) | — |
| 10 | the hottest code left after the plan | nodes as handles, boxed errors (9); operators without a call (12); scopes as radix arrays (18) | handles: item 9's ids |
| 11 | late items against each other and the code | one load path (1); the walk's `stat`s and link cycles (2) | — |

The rewrite made two more changes:
- validation happens once, at the end (§9, §11), and oracles that needed an old implementation alive became dumps from the starting commit (later, when building started, validation moved to the end of each item, at the user's request);
- the file's names live in an append-only arena of their own, so the tree can refer to them before item 10 changes how the lexer hands out tokens. (Later: item 9's `names` vector, by `NameId`.)

**The heap profile (§8) added:**
- the peak's cause and its fix's size (3);
- members held inline in their class's list (4);
- posting lists and sorted reference slices (5);
- records read in place from a mapped file (6) (later: read with `read_at`, no mapping, item 6);
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
- context files read into the worker's buffer, and parsed into its scratch arena (3) (later: item 9's scratch store);
- declarations built straight into their final form (4);
- versioned parameters deduplicated only where they occur (8);
- type hints, interpolation and imports built without temporaries (9);
- per-worker lexer tables (10);
- native type names matched without copies, and qualified names lexed as one token (12);
- summary builders in scratch, with references recorded into one vector (13);
- tag lines and tag tokens as slices (14);
- names resolved by hashing their parts (15);
- and, in the check (18): type operations from borrowed parts, per-function state reused, signatures substituted per type, and the tail's mechanisms. Those are messages formatted only when reported, constant types built once, rules that borrow or use scratch, throw and loop states that copy no scope (later: a scope behind one `Rc`, frames as scratch, item 18), per-text analyses made once per text, SQL parsed once per text, and one checker per worker.
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
  
  It also added radix nodes written in place when unshared (18), and a profile with untrimmed backtraces for the unplaced 11 K (0). What still stays is listed in §12 with its reason: ≈1.35 M allocations as the pass estimated it, ≈1.85 M after the item-by-item review.

**The dependency pass.** New dependencies were allowed after item 9's review, so every item was read again for hand-written code that a sound crate would replace. Each crate's API was checked in its source or its documentation. Our own code still has no `unsafe`; a crate may use it behind a safe API.

| crate | items | what it does there |
|---|---|---|
| `la-arena` 0.3.1 | 4, 9, 14 | typed arenas, ids, ranges and side tables; a list as the range of its contiguous elements |
| `wide` 0.7.33, already in the lockfile | 11, 12 | 16-byte blocks: SSE2 on x86-64, NEON on aarch64 |
| `triomphe` 0.1.16 | 16 | a name as one thin allocation (`ThinArc`); type nodes without a weak count; strong tables, freed by rust-analyzer's rule |
| `quick_cache` 0.7 | 14, 18, 19 | weighted, scan-resistant memos; the server's tree cache bounded by bytes |
| `siphasher` 1.0.4, already in the lockfile through `phf` | 14, 18 | memo fingerprints keyed per process, so no collision can be crafted |
| `blake3` 1.8.7 | 0, 1, 6, 11, 17 | every content hash a cache trusts, the build's own included, as full digests; the digests of O1's index |
| `flate2` 1.1.10, already in the lockfile through `mysql` | 0 | O1's texts, gzip-compressed, which `zcat` reads |
| `encoding_rs` 0.8.42 | 15 | an open document's encoding, so the server reads the bytes PHP reads |
| `same-file` 1.0.6, already in the lockfile through `walkdir` | 1, 2 | a directory's identity on Windows (volume serial and file index) |
| `loom` 0.7.2, for tests | 16 | the interner's drop, lookup and sweep protocol checked in every interleaving |
| `rustix` 1.1.5 (already in the lockfile) and `sysinfo` 0.39.6 | 17 | a file's filesystem type, so `stat` is trusted only where timestamps are fine and local |

**Leaving:** `twox-hash`, whose one use, the per-file key (`file_cache.rs:166`), becomes BLAKE3 (item 17).

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

**The item-by-item review:**

| item | what it found | what changed |
|---|---|---|
| 0 | O1 could not reach the project's parse. The dumps lost information (`Display`, `JsonMessage`). The edit path had no oracle. The list of oracles was incomplete, and their run would have skipped many of them (a per-file cache shared across profiles, an opt-in rule, paths the CLI never takes). The starting state was unvalidated in most configurations. The baseline was to be taken before the code that prints it existed. | The starting state is read first. One oracle run in an `oracle` profile, with coverage counted. Canonical printers that cannot drop a field unseen. O1 made through `parse_file`. A full `registry-dump`. PHPDoc and expression-type records. O2 as the gate's whole artifact set, with every diagnostic field and every cache state. The server session and the edit replay. The baseline from item 0's own commit, identical across thread counts. |
| 1 | Identity by `(device, inode)` is not PHP's. The server resolves its configuration and analyses through second implementations. Tests and benches load a third way. The edit path has its own rules. The server would keep context trees the cache cannot give it. All workspace folders share one project. | The resolved path as identity, with roles by strength and `Session::classify`. One configuration, one load, one analysis. `built_once` split. The server keeps no context trees and no unopened sources. One project per folder. The load off the message loop. The server-equals-CLI oracle. |
| 2 | The `strip_prefix` cost was respelling, not exclusions. The sort's input was mostly duplicates from the vendor context, and a k-way merge of ≈10.8 K runs is a sort. A global set of entered directories makes the result depend on scheduling, and it contradicted item 1. Roles were not final before parsing. The parse queue lost the per-job name cache. Path data was lexical. | Each file listed once, roles from roots, stronger roots pruned. Order by root and entry index, with no comparisons. Cycles checked against ancestors; links resolved once. Per-directory parse tasks with a thread's name cache. Resolved path data and one owner lookup per directory. Each path stored once. A hand-written link tree as the oracle. Item 1's identity rules corrected to match. |
| 3 | Item 3 alone cannot lower the peak by 420 MB: the peak moves to the end of the check. The edit path keeps every tree. The server reparses open files from disk. A parsed and a cached context file have different states. | Retention is one function of (role, open), in the load and in `update_file`. One state shape per role. `ast_of` reads the overlay. A scratch arena per worker. The peak claim corrected to ≤209 MB alone. (Item 19 adds mode and checked.) |
| 4 | Keying a trait method's answers by the trait's own id would share one answer across every using class: unsound. The passes that complete declarations still copy them. Not every pass follows the hierarchy. Ids were reused without a generation, and assigned only by a later item. The fields are public. The memory and allocation estimates were high. | One member-list entry per member a class has, traits' included. Every pass result in a table, so declarations never change after they are built. Passes named by their relation. Generational ids. The fields made private first. ≈90 MB and ≈1.5 M allocations. |
| 5 | The two path sorts were not the results'. File ids are not in path order, and a type-merge order depends on the sort. The tables the insert writes were not all covered. Ids were unstable on the edit path. Sorted slices need an order that agrees with `RefKey`'s case-insensitive equality. `closure_scope`'s text keys were not removed. | Merges by path rank. Named-argument facts only when their rule is on. Every insert table sharded by its own key. Stable ids for a file parsed again. A content order for reference slices. The frontier from ids. ≈5 MB. |
| 6 | The deferred set contradicted item 4's on-demand evaluation. The availability rule would have dropped polyfills for extensions that are not loaded. A mapped file needs `unsafe`, which the workspace denies, and is undefined behaviour under truncation or NFS. A lazy record could not reach its file through serde. A whole-file CRC does not cover lazy decodes. | Deferred types on demand. Availability decided on run-time evidence only (built in, Composer's platform check, the configured runtime). Unknown keeps both declarations. A new check of a polyfill against its native signature. Records read with `read_at` under a decode context, as rustc does, with per-record CRCs and recoverable failures. A test of replacing files in use on every release platform. The context base shares the container. Later, from a question on how the caches are versioned: the key misses PHPStan's own embedded stubs, so the key covers one list of embedded inputs, and the hand-kept `VERSION` goes (step 6). |
| 7 | A registry-backed memo already existed (`KEEP_DECISIONS`). The generation must cover every registry content the relation reads, not just the hierarchy. On-demand evaluation could freeze an answer computed under an open cycle guard. Clearing per generation does not bound a batch run. The settings in the key are not read. The oracle's switch was test-only. | A content generation, also for read-only pass levels. One memo absorbing both. Provisional answers under cycle guards, with each cycle resolved per component by what PHP does: a constant cycle throws, so its reads are `never` and are reported. A two-segment bound. A runtime off switch. |
| 8 | Six case-reference fields with seven kinds of readers, not one. One reader checks the merged reflection, not what the tag writes. Overlap with item 4's `FileDecls`. A wrong reason given for duplicate parameters. `reconcile_with_native` still collected. | Every field listed with its readers. Case references computed at every checked position from what is written. Dedup done in the file's parameter vector. One semantic case listed. The saving restated as work. |
| 9 | The arena's `Sync` came from an `unsafe impl`, which the workspace denies, and the name arena was not `Sync` either. The tree is changed after the parse (the stub rules, anonymous-class names, a second name resolution), and other code builds and keeps expressions. A non-UTF-8 source cannot give `&str` payloads, and its identifiers are decoded lossily today, so `class \xA9` and `class \xAA` are one class. Statement docs need only today's doc sites. The counts mixed rules: the expression clones included PHPDoc's, and M9 counted by a rule no item used. Checking the stub rules found that they apply to any file that mentions them, and for the newest PHP version only. | The tree as typed vectors addressed by `u32` ids: no `unsafe`, no lifetime, a few allocations per kept file. Chosen on merit over a safe reference arena (`bumpalo-herd` with `self_cell`): smaller nodes, exact sizes, no lifetime, dense side tables. The stub rules applied as a stub is parsed, to stubs only, recording versions that item 6's readers answer for the configured range. Anonymous-class names as a side table. Every holder of an expression listed, holding ids. Payloads as byte ranges; names as bytes (item 15) and docs lexed as bytes (item 14). Attributes kept in their node. M9 recounted by M10's rule, and item 9's counts stated by it. |
| 10 | Lexing the whole file first would lex the data after `__halt_compiler` as PHP. Interpolated strings were to keep a sub-parser, where PHP's scanner emits their parts inline. A lexer error anywhere would still beat an earlier syntax error, where PHP reports the first. Docs attached by adjacency miss the docs PHP gives declarations. The token readers were incomplete. Allocations were counted twice, and the item's M10 row held the lexer's names and the line index's `Arc`. The lexer ignores the configured versions. | The lexer stops where PHP's does, records the offset, and `__COMPILER_HALT_OFFSET__` gets its value, with a check of its use elsewhere. String parts as inline tokens; the sub-parser, fragments and scratch streams go. Errors recorded with offsets, the first one reported. Declarations' docs by PHP's rule (only `}` clears a pending doc, as `php` showed, not `{`), with a check of a doc given away. A check of source the configured versions lex differently. Comments only on request. The row recounted to 0.23 M, with the names moved to item 16 (whose `Now` is corrected: three allocations per name, always) and the `Arc` to item 11. |
| 11 | PHP and the editor end a line at a lone `\r`, and PHP ends a line comment there too; today neither does. Context files need columns for their lexer warnings and parse failures. The per-file cache stores the index. Open documents need their overlay. The count was 46,422, not 43,630, and fragment lexers build an empty index. | Lines and line comments end where PHP's do. Warnings and failures record their line and column on the worker. `LineStarts` in the cache. The overlay first. One allocation for `LineStarts`, none for fragments. |
| 12 | Six places where our lexer reads source otherwise than PHP's: doc comments without whitespace after `/**`, a lone `\r` after `?>` and in heredocs, heredoc indentation PHP refuses, casts as one token with only spaces and tabs inside, the spaced qualified name that PHP 8 rejects, and open tags (`<?phpx`, and `<?` by `short_open_tag`). A perfect-hash watch list is not needed. Blank runs are mostly one byte. One comparison cannot answer both questions of step 2. | Each reading made PHP's, checked against `token_get_all`, and listed as semantic; the cast checks of `coercion.rs` kept. Watch bits cached with the name. Blank runs tested a byte first. The keyword comparison restated. `parse_qualified_name`'s allocations moved from item 9's row to this one. |
| 13 | Guards did not follow PHP's binding: compile-time-bound declarations exist after an early exit, run-time-bound ones (trait users, implementers, enums, interfaces with parents, `const`, `define()`) do not, and `exit` was not an exit. Ids given at commit would put nested declarations first. `InitFlow` records a value before its target's operands, against PHP's order. Item 19's families had no host. A doc's scope, a body's lookup and dynamic `define()` were open. Several counts were off, and two sites do not occur in this run. | Guards by PHP's binding, tested with `php` (a parent from a file loaded earlier makes a class bound at compile time, so it is kept as possibly declared). Entries reserved in source order. The walk in PHP's order, destructuring included. Families hosted. Docs resolved in the entry's scope; bodies by declaration id; a marker for constants a file defines by a computed name. Counts by M10's data. |
| 14 | Keying resolution by template names would hand one method's bound and scope to another's same-named template, and a node index is no key. The tag memo would freeze the spelling's decisions from whichever thread came first. Steps 4 and 8 described two structures for one memo. Shared effects cannot index one docblock's vector. Cached `DocType`s would lose their nodes and their types. A doc is parsed twice, split two ways (`//`), and three rules pick tag winners. The doc lexer uses Unicode's classes. Wrapped tags are not slices. The oracle could not be exact, and missed references and edits. The text grammar's callers were not sorted, and a class from code is parsed as PHPDoc (`Integer::MAX`). The `@var` memo already exists. Counts and memory were double-counted. | Resolution keyed by the written `Type` and a context holding whole `TemplateParam`s. `apply` decides by the spelling. One fingerprint-keyed `quick_cache` memo type for every `syntax::memo` user. Arenas per tag effect. A serialized `DocType` with its subtree and type. One parse, one winner rule, PHP's byte classes, logical tag text. A lowering oracle with its differences listed, plus O1, the edit replay and every cache state. Callers by kind, with static tables and `Type::object` for code. Counts by M10: 2.59 M, with 65 K moved to item 18; memory ≈43 MB, so the projected exit is ≈290 MB and the peak ≈375 MB. (§12 now: ≈350-385 MB plus the checks in flight.) |
| 15 | `\xA9` as an escape is ambiguous with a namespace separator. Fixes would write escaped names into source files. An open document arrives decoded by the editor. `Name` serializes a `str`. String values become names by rules that are not PHP's (every backslash stripped, non-UTF-8 dropped), and some sites fold with Unicode. The semantic changes were listed under item 9. The short-name header came from a later item. Item 16's freeing rule could leak an entry or deadlock. Counts were by several rules, `Name::to_string` double-counted its callers. | The `\x{NN}` escape everywhere text shows a name. Fixes as bytes, applied on bytes. Each open document's encoding found with `encoding_rs`. Bytes in the caches. One `from_runtime` conversion by PHP's rules (one backslash, no trimming, ASCII folding, constants' case), tested with `php`. ASCII folding at the eight sites. §11 moved to item 15, with a fixture. `last_segment` in `NameData` now. Item 16 sweeps and drops outside the lock. Counts by M10. |
| 16 | The type memos are emptied when full, and their answers depend on no hierarchy, so a generation protects nothing; their cost is the nodes they keep. A table of shape keys would repeat the type interner's, and most keys copy a literal's bytes. `Interner::str` was left out. A raw holder (`FRONT`) would escape the freeing rule. Tests on threads cannot find the protocol's races, and O2 cannot see a leak. The counts mixed removable and kept allocations, and a name's text count was off. | Memos bounded by two segments, no generation. `PhpString` interned as a `ThinArc`, shared by literal types and shape keys; `Interner::str` goes. Every holder holds the handle. `loom` for the protocol and a leak test after the edit replay. Counts restated by what goes and what stays; 39,140 moved from item 12's row. |
| 17 | Content hashes in the base's key would make every changed context file a cold run. A parse now reads the configured versions and `short_open_tag`, which the per-file key lacked. The base's key missed most resolved inputs (Composer data, run-time evidence, stub and bootstrap files). `stat` has no stable inode on Windows, mtimes can be restored, git's racy rule assumes one filesystem, and some filesystems keep coarse or remote times. The server could write overlay bytes into the index. The build's hash is 64-bit `DefaultHasher`. The oracle compared two new paths with each other. | The base keyed by the build and a destructured hash of resolved settings (tracked or untracked), with its file list applied as edits. The per-file key adds the versions and `short_open_tag`. Stat fields per platform, ctime compared, the racy rule applied where the file was read, and `stat` trusted only on identified local filesystems (`rustix`, `sysinfo`). Disk-only index and base. BLAKE3 everywhere, full digests. An oracle against the baseline, an edit replay across runs, the `stat` cases on three platforms, and the server. |
| 18 | The traversal would keep typing a value before its target's operands, against PHP's order. `call_user_func` with a literal `extract`, `compact` or `get_defined_vars` writes the caller's scope, and 7.x adds `parse_str`/`mb_parse_str` with one argument and string `assert`. Slots and file-wide keys would collide in one radix array, sparse over large ids. The throw states' cost is inline `Locals` structs, which persistent maps do not remove. The member-usage family would inherit PHPStan's skip of uncertain callables. The 25 G oracles were dropped. External rules name their own identifiers. `concat` builds bytes. Step 7 and 9's counts. | PHP's order in `process_expr`. The computed-name forms listed exactly, tested with `php`. Per-body key tables, dense; `IntMap`-style merges; locals listed by name. `Locals` behind one `Rc`, frames as scratch. Uses counted when any access may reach a member. The oracles restored. `Identifier` kept as `Cow`. `concat` through a byte scratch. Counts by M10, ≈38 K moved to item 8. |
| 19 | Class shapes read method bodies, not just the declaration list. Private properties are written from outside their class, through bound closures and reflection, which the judgment never sees, and its outside files get a full check. A changed file on disk had no defined path, and the tree cache had no content key. The pool does not take files largest first, and the peak omits the checks in flight, which were never measured. Banned constructs read the registry, mock hygiene is per file, and Composer hygiene reads files in order. Retention gained inputs outside item 3's function. The oracle could not see a reader of a kept tree. | A parent-call fact per method from item 13's walk. Writes from bound closures and reflection recorded, unnarrowed ones blocking the judgment, outside files run with only the property-write family. A hash mismatch is an edit; the cache keyed by content. A largest-first queue, and the check's own maximum measured. Each pass placed by what it reads. `keeps_tree(role, open, mode, checked)`. Oracles with a zero-budget replay, disk changes and the write forms. |
| §11 | Four items' semantic changes were missing (the unknown `short_open_tag` report, constants a file defines by a computed name, the server's encodings and fixes as bytes, the rules reading the statement result). Three oracles compared with code the plan deletes. Most oracles the reviews added ran nowhere, and the all-on configuration sets no architecture rules or banned types. One dhat profile cannot isolate the check's maximum. The commands and order of the runs, the platforms, PHP 7.x and the snapshot layout were unstated. | The list completed and ordered. Item 0 records type nodes and unions, which §11 lowers and builds again. Tests run once, equalities extended, a configuration for item 19's passes. The check's live and resident maxima and a second dhat run from the check's start. An order of runs under `nice`, `CLAUDE.md`'s commands, a CI matrix for macOS and Windows, `php:7.4-cli` through `docker`, snapshots as `xtask` lays them out. |
| §12 | The allocations' "today" column predated the row moves; item 4's residual contradicted its item, and item 15's binding maps stay, so ≈1.85 M remain, not ≈1.35 M. The instruction rows sum to more than the total stated, overlap in more pairs than named, and five items had no saving line (3 and 5 overstated). The peak counted the checker's results and an item 9 figure item 9 does not make. The kind column missed four §11 changes. The estimates' status was stale. | Rows and totals restated; saving lines with their basis for items 3, 5, 11, 13 and 16 (3 at ≈0.16 G, 5 at ≈0.06 G). The overlapping pairs named, the net left to callgrind. The peak as ≈350-385 MB plus the checks in flight, with the finishing window as the other candidate. Kind column matched to §11. What is measured, estimated and not counted. |
| §13 | M11's F1 and F6c fixes predated item 18's review. Superseded passes read as current designs. M7 and §2's P7 and P11 rows were stale. `same-file` was missing from the dependencies, and `twox-hash`'s removal unstated. | M11 matched to item 18. A "superseded by" column and "later" notes. Part I corrected. The dependency table completed. |

Two parts of item 0 were added while building, after the review: the implicit calls ("Code PHP runs without a call written"), from reading the starting state (row 0.1), and the assumed types ("What PHP gives where a type assumed"), from the runtime oracle's generated calls in item 0's validation. Each was first an item of its own at the end of the plan, and moved into item 0 so that the baseline holds the corrected analysis and every later item is checked against it.

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
