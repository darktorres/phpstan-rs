# Part L. The second pass, at the grain of the ledger's rows (2026-10-18)

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `2f0a3779` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table.

| task | status | ledger |
|---|---|---|
| LOAD-2.2 | SEE LEDGER | Walk DONE in `85a4ecad`. PART: the name floor's replacement is built by Part A's A1 (`097c164b`, `40d889bb`, … |
| LOAD-5.4 | DONE | DONE `a5ab84c4` (Part A's A5a): the passes read through `ast_of`, and `FileState::ast` is gone |
| CACHE-13.1 | OPEN | OPEN |
| REG-4.10 | READY | READY; overlaps DOC-2.6 |
| DOC-2.6 | OPEN | OPEN; overlaps REG-4.10 |
| DOC-2.9 | OPEN | OPEN; overlaps DOC-2.6 |
| SYN-4.1 | OPEN | OPEN |
| LOAD-7.5 | READY | READY |
| VAL-1.1 | OPEN | OPEN |
| SYN-1.3 | OPEN | OPEN |
| SYN-2 | OPEN | OPEN; the target grew |
| SYN-3.1 | READY | READY |
| SYN-4.2 | READY | READY |
| SYN-1.9 | OPEN | OPEN |
| Q-10 | OPEN | OPEN, partly probed (2026-10-16, `~/.cache/prs-ledger/q10/`). The recovery is in place: a context file with a syntax … |
| SYN-6 | OPEN | OPEN |
| NAME-2.1 | READY | READY |
| NAME-1.1 | READY | READY |
| PROD-5.8 | READY | READY |
| PROD-5.3 | READY | READY |
| DOC-2.3 | READY | READY |
| CHECK-6.4 | READY | READY |
| CHECK-6.5 | READY | READY |
| CHECK-9 | READY | READY |
| REG-4.15 | OPEN | OPEN |
| REG-16.2 | READY | READY |
| TYPE-5 | READY | READY |

<!-- status:end -->

Asked again as: "go through all tasks, one by one, in order and review them. maximum churn/properness". Part K
went through the tasks in groups and marked many "sound"; this pass reads each row's ledger text (its task, state and
needs), then the code it touches or the document it cites, and asks of each: (1) does the row's stated need hold;
(2) is it the end-state design or a structure a later task deletes (the 25 G report's no-interim rule, which Part
E 35 and 41 apply); (3) does it hold on every platform the cross-platform plan makes the project ship on, and against the module
moves the cross-platform plan's R18 makes; (4) what checks it. A change made to an earlier part is made in that part and listed
here. Nothing was built or run.

## L1. Findings and what changed, in sequence order

| # | task | what the pass found | changed |
|---|---|---|---|
| 1 | the start condition | the cross-platform plan now has **R18**, the module graph, built before R6 and partly in the tree: `file_id` and `shard` go to reflection, `Completion`/`Reach`/`BlockCompletion` and `is_truthy_literal` to `syntax`, the mention bits to `checker::mentions`, `set_current_file` takes the shown path, `path_bytes` becomes a crate, `allocator` is one binary module | intro: rows R0 to R18, the start condition and what R18 moves; `File.shown` (H2.2) |
| 2 | the conventions | two gates hold every row that moves code, and the plan named neither: `cargo xtask modgraph` (a `crate` or `module` line in `gates/module-graph.txt`; a module held only by the root is rejected) and `cargo xtask tests` (a renamed, merged or deleted test needs `old -> new` or `old -> REMOVED: reason` in `gates/test-renames/*.txt`). Item 9's test rewrite and VAL-1.3's oracle deletions are the large cases | intro: conventions |
| 3 | A1, LOAD-2.2's last part | the walk counts, for every directory, the walks that reach it (`Starts::below`, `reaching`), filtering by excluded **name**; a vendor rule that only the listing tells would leave the reach counting a walk that lists nothing there, and the carried-state oracle (`reach_by_ancestors`) would disagree | A1: "The walk's reach follows from the listing" (a vendor directory's own task drops the walks that have the rule; `reaching` and `reach_by_ancestors` make the same cut; covering roots are `Excluded::is_empty()`; the first version's `vendor_child` went with the carried targets, M5d), the tests |
| 4 | A5a, LOAD-5.4 (new in Part A) | `FileState::ast()` returns `&[]` in release when a file kept no tree; item 9's new API would need an empty-tree constructor for the one case that must not happen. Four readers, all of files that keep trees. It is a type change an end-state invariant needs, not a structure W3 deletes | Part A table and A5a; bookkeeping |
| 5 | B3, I1.1, I1.7: the node ids | stored ids carried a debug-only tree tag, so `size_of::<ExprNode>()` would differ between the debug build the tests run in and the release build, and the size assertions could hold in only one | stored ids are raw 4-byte ids (`RawExpr`…); the tag is on the public `ExprId` and on handles |
| 6 | H1.4, Part E 34, anonymous classes | the plan keyed them by ordinal and said the displayed name stays `class@anonymous`. The ledger (decision 5, PROD-5.1, SYN-1.10, SYN-3.4) and the review's F33 design name them from their position, `class@anonymous/{path}:{line}[:{i}]`, which is also how types display them today (`types/src/parse_display.rs:91`); REG-11.13's `class@anonymous` is a display change of diagnostics, later. The ordinal was the plan's, to keep a line out of the global index | file-local entities with positional names, outside `decl_names` and `decl_index`, found by the provider through the file the name carries (`File.shown`, `FileSet`'s `shown` index) |
| 7 | CACHE-13.1 (batch 8) | MEAS-1 puts what a stat index replaces at ≈60-70 M Ir of reads and their system calls, with the ≈1,260 M of decode untouched; the persisted database holds `File.stamp` and deletes any index file. The stamp is permanent and its storage in batch 8 is not | built once as `File.stamp` at W3 (Part E 15); batch 8 is empty |
| 8 | REG-4.10 and DOC-2.9 (batch 6) | DOC-2.9 "needs REG-4.10 (level 2)", and REG-4.10 is the builder reading each doc comment's block once, which is what DOC-2.9's per-file product is; the need made item 14 close PART | REG-4.10 is folded into DOC-2.9; item 14 closes whole |
| 9 | Part C's table | it still had the first-pass order (products as batch 4, names as batch 5), "DOC-2.1 is deferred", the inference core in batch 7 and the error variants in step 1, against Part E 9, 11, 12, 14, 16 and 17 | C1, C2 notes 3 to 7 and C3 rewritten to the ruled order |
| 10 | W1's fillers | REG-4.9 is REG-4.12's change ("item 4's first step is the same change") and REG-4.10 is DOC-2.9's | W1 row, the tidying row |
| 11 | B3's store | it still listed `anon_classes` and `stmt_docs`, against I1.4 (one `classes` vector, an anonymous class is a `ClassNode` with no name) and I1.5 (one `docs` table keyed by owner kind) | B3 aligned to I1; a note that B5, B6 and Part I's `ExprId` means the raw id |
| 12 | G3.6, Part E 31 | an error node was said to infer `mixed`; F21 (batch 0, before recovery) makes the error type a kind of `mixed` that every unknown-type guard covers and that records a report was made | error nodes infer `Mixed(Error)` |
| 13 | B9 and DOC-2.9 | the plan said the comment's offset "is not carried by the parser, so that row adds it"; SYN-3.4 (batch 2) turns `doc_line` into the offset first | B9 |
| 14 | Part D and Part E | first-pass statements survived against the rulings: "DOC-2.1 is deferred", "batch 5" for names, "R7 lands", "four rows", "Part A's validation", W2's note, W3's open question, the sequence | each corrected or marked superseded |
| 15 | SYN-4.1, `WATCHED` | the plan said the watch list becomes a compile-time perfect hash; the cross-platform plan's R18 composes `WATCHED` from three layers' lists (`stub_attributes`, `checker::mentions`, `file_declarations`), which `syntax` cannot see when it compiles | a `const fn` in the caller builds the table |
| 16 | step 1's module order | R18 moves the statement-completion rules into `syntax` | `completion` joins the syntax internals |
| 17 | J2, OVL/RES/EXT/SUM against C6 to C9 | read against the crate split's own text this time: faithful, with omissions in OVL-3 (the ≈60 narrower-parameter misses and the native-signature-base decision), EXT-1 (the trait's shape: `Name`s, memoized views, `TemplateParam`s, the enum-cases query; two renamed tests), EXT-2 (universal object crates; distance rule), EXT-3 (a used trait's mixins; template binding) and SUM-2 (expression typing) | rows completed |
| 18 | J3, D1 to D4 against the stage text | D1 (holder storage), D3 (per-node results), D4 (`RecordingNodeCallback`, `isReplayableConvergenceBody`, the `RefCell` channels as observer events) are as J3 says; D2's `syntax::annotations` needed a place in an immutable tree | J3's D2 row: side tables filled at `finish`, and `RemoveUnusedCodeByPhpVersionIdVisitor` a mark |
| 19 | Part E 32 | the `php-version` crate would have moved `KNOWN_PHP_VERSIONS` (5.3 to 8.5) and, with decision 18, been rebased to 7.0 in the same change | it moves as is; the rebasing is STUB-7.1's delta |
| 20 | the conventions | a heavy tool in CI would run on three systems that spell paths differently | cheap gates join `cargo xtask gates`; dumps and the oracle run stay local |
| 21 | F10 (the walk, T9) | the plan treated it as an open parser bug that gates the baseline; `3b7e4d95` (2026-10-03) made a bare `yield` end at any token in `can_begin_expression`'s list, and `php -l` accepts every shape the finding lists. The review's exhaustive `starts_expression`, its table test, seven parser tests and the fixture are unbuilt | tests and fixture in batch 0, the exhaustive predicate on SYN-2's `TokenKind` in batch 1 (Part E 22, K1 8, K2) |
| 22 | F77 (T34) | scheduled with parse recovery (batch 3), but its design needs none: a parse failure becomes `phpstanRs.parseError`, non-ignorable, and fails the run. Item 9's step 2 rewrites the parser, and a file it newly refuses must show in O2, not vanish into a count | F77 in batch 0, before the baseline; batch 3 feeds it from `errors()` (Part E 11 and 22, C1, the sequence) |
| 23 | G5 tool 4 (T15) | `Registry` has 31 fields, some scratch and some products the list did not name | the dump destructures it without `..`; each field printed or listed as scratch with its reason |
| 24 | G5 tool 6 (T17) | the in-process harness is `pub(super)` in `lsp/tests/helpers.rs` | it moves to the `testing` crate, which R18 makes the fixture runner's home |
| 25 | S-2 (T47, T48) | the spike tested the build on Linux only; salsa's default `inventory` feature registers ingredients at link time, and the project now ships and tests on four targets | the skeleton's tests run on the macOS, Windows and aarch64 legs; `default-features = false` is the fallback |
| 26 | W1, LOAD-7.5 (T50) | the row and the Q-11 table cite `lsp/workspace_config.rs`, which LOAD-1.3 deleted | the lookup is `lsp.rs:561`; corrected when the row is worked |
| 27 | Stage B T2 (T59) | filed as a finding in K2, it asks the user's tool to "decide whether callee throw points count" | decided by runtime truth and PHPStan's rule (`getThrowPoints()` of the statement result includes calls): they count; K2's row says so |


## L1b. The walk, item by item (the sequence's order; each item's real source read when it was reached)

| item | task | read | verdict |
|---|---|---|---|
| T0 | the start condition | `WORK_STACK.md:265-272`, `CROSS_PLATFORM_PLAN.md:23-25`, `git log` | changed: R18 first |
| T1 | A1 | `session.rs:335-394` (`Root::contains`, `unlisted_role`, `classify_unlisted`), `walk.rs:225-462` (`Scope`, `Reach`, `Starts`, `Reaching`, `below`) | the reach tables were missing: added, then simplified by the evidence rule (M5d) |
| T2 | A2 | `composer/lib.rs:92-141` (`PackageContext::load`), `manifest.rs:255-364`, `dir_listings.rs` | holds; `listings` is already a parameter |
| T3 | A3 | `function_sig.rs:822-842`, `asserts.rs:66-125` | holds |
| T4 | A4 | `types/decide.rs:1-60`, the four callers | holds |
| T5 | A5 | `ast.rs:1414-1428` | holds; the guard goes with the recursion (147 callers) |
| T6 | A5a (LOAD-5.4) | `project.rs:100-234`, `:770-830`, `:962-1000`, `:1812-1876`, `:2606-2626` | holds; all four readers read analysed or retained trees |
| T7 | A6 | `ITEMS_1_TO_3_FIXES_PLAN.md:1253-1254` | digest and counts match |
| T8 | VAL-1.1 | `git cat-file 1892b477`, `PROFILE_DRILL_PIWIGO_20G.md:613` | holds |
| T9 | F10 | `STAGE_B_FIX_REVIEW.md:543-580`, `parser/primary.rs:388-421`, `parser/expr.rs:211-285`, `git merge-base --is-ancestor 3b7e4d95 HEAD`, `php -l` on seven shapes | **already fixed in form** (`3b7e4d95`); tests, fixture and the exhaustive predicate remain |
| T10 | F20 | `STAGE_B_FIX_REVIEW.md:3446-3481`, `ls crates xtask/src`, `syntax/stack.rs` | open as said |
| T11 | F21 | `STAGE_B_FIX_REVIEW.md:3482-3518`, `types/queries.rs`, `lib.rs:375` | open as said |
| T12 | F57 | `closures_and_match.rs:79-107` | built (`parse_expression`) |
| T13 | G5 tool 2, printers | `Repr` (about 40 variants), `types/src` | holds; after F21 |
| T14 | G5 tool 3, O2 | `xtask/src/main.rs:37-88`, `gate-runner/src/main.rs` | holds |
| T15 | G5 tool 4, `registry-dump` | `registry.rs:262-453` (31 fields) | **a field rule added** (destructure without `..`; scratch listed with reasons) |
| T16 | G5 tool 5, expression dump | `project.rs:2359-2377`, `checker.rs:1328` (`set_dump_types`) | holds; a second switch of the same kind |
| T17 | G5 tool 6, server session | `lsp/tests/helpers.rs` | **the harness moves to `testing`** |
| T18 | G5 tool 7, edit replay | `main/incremental_bench.rs:9-69` | holds; the script is new, the entry points exist |
| T19-20 | G5 tools 8-9, memory and the `oracle` profile | `Cargo.toml:51-82` | holds |
| T21 | G5 tool 10, retiring the oracles | 54 `cfg(debug_assertions)` sites in 13 files | holds |
| T22-23 | G5 tool 11, the baseline; level 3 | K2 | holds |
| T24 | G3, the error-node model | `ParseError` sites (31), `LexWarning`, the `expect*` helpers (six) | holds |
| T25 | step 0, SYN-1.11 | `phpdoc_type_compat.rs:268-300`, `calls.rs:770-790` | holds |
| T26 | step 1, SYN-1.3 and 1.12 | counts: 134 files and 2,452 lines, 51 `Visitor` impls, 415 `Locals` mentions | holds |
| T27-28 | step 2 layout, step 3 | I1.2's arithmetic (`MethodCall` is 20 bytes) | holds |
| T29 | batch 1, SYN-2 | `lexer.rs`, `parser.rs:96-260` (`Supply`, `Lexed`, `Tok`, `VecDeque`, `fill`, `pop`) | holds; `Lexed` is the fuzz and test path |
| T30 | SYN-3.1 | 81 `LineIndex` mentions, 82 `.line_index` uses | holds |
| T31 | SYN-4.1 | `lexer.rs:243-262` (`WatchList::bits`) | holds; `keyword_slot` is the `const fn` precedent |
| T32-33 | SYN-4.2 to 4.10 | `token.rs:74-189`, `references.rs:23`, `name_resolution.rs:121`, `lexer.rs:1042` | holds |
| T34 | F77 | `STAGE_B_FIX_REVIEW.md:3724-3763` | **moved to batch 0** |
| T35 | SYN-1.9, Q-10, SYN-6 | Q-2, Q-10 records | holds |
| T36 | NAME-2.1 | `name.rs:22-80` | holds (three allocations a name today) |
| T37-39 | NAME-1.1, 1.2, 1.5, 1.6 | `checker.rs:281-287`, 209 `String`-keyed maps, `name_resolution.rs:424`, `:498`, `checker/expr.rs:55-890` | holds |
| T40 | PROD-5.8 | `references.rs:175-215` | holds; `Recorder::published` is PROD-5.2's seed |
| T41 | PROD-5.3 | `project.rs:3316`, `summaries.rs:380`, `global_vars.rs:551` | holds |
| T42 | DOC-2.3, 2.4 | `docblock.rs:391-497`, `:596` | holds |
| T43 | CHECK-6.4 | `types/lib.rs:539` (16 uses) | holds |
| T44 | CHECK-6.5, 6.8 | `checker.rs:421`, `:676`, `:691` | holds |
| T45 | F69, CHECK-9 | `try_catch.rs:234`, `throws.rs:105` | holds |
| T46-48 | S-1 to S-11 | ledger NAME-1 targets, `salsa-macros` `unsafe impl`, `salsa` features | S-2 extended to the other legs |
| T49-52 | W1 and W2 anchors | seven nested structs, `glob_matches`, `psr3_context_key_pattern`, `DEFAULT_REPORT_LEVEL`, `lsp.rs:561`, `class_info.rs:393-402`, `function_sig.rs:40`, 115 `finish_loading` mentions | holds; one stale citation |
| T53-55 | REG-4.15, REG-16.2 to 16.4 | `class_info.rs:986` (`from_def`), H1 to H4 | holds; the engine rows have no code to read, their anchors are the designs |
| T56 | F22 (the `use` list) | `visit.rs:595-605` | open as said (`Closure { .. } => {}`) |
| T57 | F7, F8 (the use-site walk) | `use_sites.rs:207-218`, `navigation.rs:489` | open as said |
| T58 | F14 / TYPE-5 | `types/parse.rs:1330-1349` | open as said; the arm I4 extracts into `phpdoc_keyword` |
| T59 | Stage B T2 (a callee's `@throws`) | `STAGE_B_REVIEW.md:321`, `vendor/phpstan-src/.../MissingCheckedExceptionInMethodThrowsRule.php:33` | a decision row, settled by PHPStan's rule: callee throw points count; built with F69 and REG-17.2 |

## L2. Checked, and holding

What this pass read and found as the plan has it (so that a "sound" in Part K has its evidence):

- **Part A.** The line references of A1 hold at HEAD (`session.rs:375`, `:386`, `:701`; `project.rs:1949`, `:3414`,
  `:3420`; `lsp.rs:541` for `classify_unlisted` has moved ten lines with R5); `phpstan_api.rs:268` is REG-18.6's, and
  `session.rs` (R8's `vendor_trees` and `unbuilt`) is a second reader of the name that this pass missed and the audit of
  2026-10-18 found by listing every `vendor` in the non-test code. A2's `read_installed` is as described (`manifest.rs:284-322`: two
  `resolve` calls per package, the install path joined to `vendor/composer` with `/`). A3's `param_names` has the three
  `resolve_assert_tags` calls and one `contains` (`function_sig.rs:828-835`, `asserts.rs:76-90`). Four readers call
  `FileState::ast` (A5a).
- **Batch 0.** F10's symptom is fixed and its design, an exhaustive predicate on the token, belongs on SYN-2's `TokenKind`; F20's is a `stack` crate
  and a recursion lint (after R10, which edits the guard); F21's is `Repr::Mixed(MixedKind, …)`, so G5's printers come
  after it; F57's arrow body and ternary error are in the parser. VAL-1.1 to 1.15's needs agree with G5's tool order,
  and VAL-1.3 (the oracles go) shrinks item 9's reader set as Part E 13 says.
- **Item 9.** The fabricated nodes exist where B1 says (`expr.rs:6054`, `:6071`, and the `NodeId::SYNTHETIC` test at
  `:4523`; `var_tag_types.rs:537`; `completion.rs:310-326`). The parser backtracks in one place only, the body skim's
  token pushback (`parser/functions.rs:219`), which the whole-file stream replaces by a cursor probe, so G3's "no node
  rollback" holds. Anonymous classes are named `class@anonymous/{file}:{line}[:{i}]` in `registry/anonymous_class.rs`.
- **Level 3.** The headings of the open findings were read against K2's placement; none that K2 leaves to "the review's
  order" sits in code that a later task deletes before its fixture exists.
- **The engine.** `run_finishing_passes` (`project.rs:1533`) runs the passes in the order H3.2 maps them (the
  assertion table after PHPDoc inheritance is the one difference, and the ledger's REG-7.2 already puts it at the own
  level); every pass H3.2 names exists where it says; salsa 0.28.5, `wide` 0.7.33 and `safe_arch` are in the registry
  cache or `Cargo.lock`, `triomphe` is in neither (S-1 fetches it); the checker's live filesystem reads are the
  `is_file` probes of `includes.rs` (`:272`, `:281`, `:282`, `:293`) and nothing else (H4.5); R11's cache directory is
  H4.4's; R18 step 7 (phpdoc may depend on types) is I4's placement.
- **The wall's text.** C6 to C9 against J2 and D1, D3, D4 against J3 (the last two with the changes of L1 17 and 18).

## L3. What this pass leaves for the user

No new ruling from this pass (the workflow review's rulings and changed answers are M5). Four answers changed or were
sharpened and want the user's eye with the rest of Part E: #15
(CACHE-13.1 is W3's), #32 (the `php-version` crate moves code as is), #34 (anonymous classes are file-local with
positional names, not ordinal; on 2026-10-18 the user ruled the identity ordinal, with the name still positional) and
#39's timing. Two edits to the ledger the plan would otherwise have left false were listed in B11 (decision 4's
layout bullet and SYN-1.4, on the arena, Part E 4); the user ruled on 2026-10-18 that the ledger's layout stands, so
neither edit is made. The pass changed nothing that was built, and ran nothing.

## L4. What the workflow review found wrong in this part (2026-10-18)

This part is the record of the second pass; the statements below were checked by the workflow review and found
wrong, and the parts they concern carry the corrections (the finding's id says where it was found). The rows above are
left as they were written.

| where | said | is | id |
|---|---|---|---|
| L1 #1, L1b T0, L2 start condition | the cross-platform plan is at R18 and R6 to R17 follow | R0 to R12 and R18 are built at `2c7bf898`; R13 to R17 are open | A-17, S-21, C-01 |
| L1 #4, L1b T6, L2 Part A | A5a has four readers | twelve non-test call sites in six files | A-04 |
| L1 #7 | what a stat index replaces is "reads and their system calls" in Ir | the ≈60-70 M Ir is read wrappers and two hashes; system calls and I/O are not in Ir | V-11 |
| L1 #22 | F77 fails the run for any file it refuses | analysed files only; the context half departs from decisions 9 and 15 and waits for a ruling | S-12, C-14 |
| L1 #24, L1b T17 | the in-process harness moves to the `testing` crate | `testing` has no `lsp` edge; the session API is public in `lsp` | V-01 |
| L1 #25 | `default-features = false` is the fallback | the pinned feature set without `inventory`, with every ingredient registered by hand; and a release-profile test on each leg | E2-22, X-06 |
| L1b T24 | `ParseError` is built at 31 sites | 28 constructions (31 lines name the type) | G-19 |
| L1b T29 | `Lexed` is the fuzz and test path | `tokenize_with_spans` and `with_comments` are used in production by `lsp/completion.rs`, `stub_attributes.rs` and `output/codeframe.rs` | T-07 |
| L1b T36 | a name takes three allocations today | two and a temporary; three only with capitals | N-18 |
| L1b T37 to 39 | 209 `String`-keyed maps | 181 outside tests (187 with); the plan carried three counts | N-12 |
| L1b T58 | the iterable arm is extracted into `phpdoc_keyword` | TYPE-5 is its own commit after I4.5 step 5 | P-01 |
| L2 engine | `triomphe` is in neither the registry cache nor `Cargo.lock`; the checker's live filesystem reads are the `is_file` probes and nothing else | `triomphe` 0.1.14 is in the registry cache; the SQL extension's live reflector is a second live read | X-22, E1-01 |
| L2 level 3 | no finding K2 leaves to the review's order sits in code a later task deletes | F5, F6, F45 and F61 add per-variable state to `Locals`, which D1 replaces | S-02 |

---
