# Part L. The row-by-row pass

## Tasks this part owns

None. Part L is a review of the ledger's rows: its verdicts (L1, L1b, L2), what it leaves for the user (L3) and the review of its own findings (L4). The rows are owned by the parts that build them.


This pass reads each row's ledger text (its task, state and
needs), then the code it touches or the document it cites, and asks of each: (1) does the row's stated need hold;
(2) is it the end-state design or a structure a later task deletes (the 25 G report's no-interim rule, which Part
E 35 and 41 apply); (3) does it hold on every platform the cross-platform plan makes the project ship on, and against the module
moves the cross-platform plan's R18 makes; (4) what checks it. A change needed in an earlier part is made in that part. Nothing was built or run.

## L1b. The walk, item by item, in the sequence's order (each read at its source)

| item | task | read | verdict |
|---|---|---|---|
| T0 | the start condition | `WORK_STACK.md:265-272`, `CROSS_PLATFORM_PLAN.md:23-25`, `git log` | R18 comes before R6 |
| T1 | A1 | `session.rs:335-394` (`Root::contains`, `unlisted_role`, `classify_unlisted`), `walk.rs:225-462` (`Scope`, `Reach`, `Starts`, `Reaching`, `below`) | the reach tables are part of the design (A1) |
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
| T15 | G5 tool 4, `registry-dump` | `registry.rs:262-453` (31 fields) | the dump destructures without `..`; scratch is listed with reasons |
| T16 | G5 tool 5, expression dump | `project.rs:2359-2377`, `checker.rs:1328` (`set_dump_types`) | holds; a second switch of the same kind |
| T17 | G5 tool 6, server session | `lsp/tests/helpers.rs` | the session API is public in `lsp` (G5 tool 6) |
| T18 | G5 tool 7, edit replay | `main/incremental_bench.rs:9-69` | holds; the script is new, the entry points exist |
| T19-20 | G5 tools 8-9, memory and the `oracle` profile | `Cargo.toml:51-82` | holds |
| T21 | G5 tool 10, retiring the oracles | 54 `cfg(debug_assertions)` sites in 13 files | holds |
| T22-23 | G5 tool 11, the baseline; level 3 | K2 | holds |
| T24 | G3, the error-node model | `ParseError` sites (31), `LexWarning`, the `expect*` helpers (six) | holds |
| T25 | step 0, SYN-1.11 | `phpdoc_type_compat.rs:268-300`, `calls.rs:770-790` | holds |
| T26 | step 1, SYN-1.3 and 1.12 | counts: 134 files and 2,452 lines, 51 `Visitor` impls, 415 `Locals` mentions | holds |
| T27-28 | step 2 layout, step 3 | I1.2's arithmetic (`MethodCall` is 20 bytes) | holds |
| T29 | batch 1, SYN-2 | `lexer.rs`, `parser.rs:96-260` (`Supply`, `Lexed`, `Tok`, `VecDeque`, `fill`, `pop`) | holds; `tokenize_with_spans` and `with_comments` are production readers (`lsp/completion.rs`, `stub_attributes.rs`, `output/codeframe.rs`) |
| T30 | SYN-3.1 | 81 `LineIndex` mentions, 82 `.line_index` uses | holds |
| T31 | SYN-4.1 | `lexer.rs:243-262` (`WatchList::bits`) | holds; `keyword_slot` is the `const fn` precedent |
| T32-33 | SYN-4.2 to 4.10 | `token.rs:74-189`, `references.rs:23`, `name_resolution.rs:121`, `lexer.rs:1042` | holds |
| T34 | F77 | `STAGE_B_FIX_REVIEW.md:3724-3763` | batch 0 |
| T35 | SYN-1.9, Q-10, SYN-6 | Q-2, Q-10 records | holds |
| T36 | NAME-2.1 | `name.rs:22-80` | holds (two allocations and a temporary; three only with capitals) |
| T37-39 | NAME-1.1, 1.2, 1.5, 1.6 | `checker.rs:281-287`, 181 `String`-keyed maps outside tests (187 with), `name_resolution.rs:424`, `:498`, `checker/expr.rs:55-890` | holds |
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
  `session.rs` (R8's `vendor_trees` and `unbuilt`) is a second reader of the name, found by listing every `vendor` in the non-test code. A2's `read_installed` is as described (`manifest.rs:284-322`: two
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
- **The wall's text.** C6 to C9 against J2 and D1, D3, D4 against J3.

---
