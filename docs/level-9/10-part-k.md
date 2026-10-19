# Part K. The task-by-task review of 2026-10-18

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `b9de6dcf1` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| LOAD-2.2 | DONE | Walk DONE in `85a4ecad`. PART: the name floor's replacement is built by Part A's A1 (`097c164b`, `40d889bb`, … | 02 03 04 11 12 |
| LOAD-3.1 | OPEN | OPEN | 02 04 05 06 12 19 21 |
| LOAD-3.3 | PART | PART. **The load: DONE `a5a4d85f`.** In a project built once `keeps_tree` is a function of the role, which LOAD-2.2's … | 05 08 19 |
| LOAD-3.5 | OPEN | OPEN | 05 12 13 19 |
| LOAD-4.7 | OPEN | OPEN, blocked. Its file-references part is one change with PROD-5.7 (OPEN), which needs PROD-5.8 and PROD-5.2, both … | 02 05 06 07 12 21 22 31 |
| LOAD-5 | OPEN | OPEN | 13 |
| LOAD-5.1 | OPEN | OPEN | 04 05 06 07 12 19 21 23 31 |
| LOAD-5.2 | OPEN | OPEN | 05 06 07 12 19 21 22 |
| LOAD-5.4 | DONE | DONE `a5ab84c4` (Part A's A5a): the passes read through `ast_of`, and `FileState::ast` is gone | 01 02 04 05 07 11 12 |
| LOAD-7.1 | READY | READY | 06 07 |
| LOAD-7.3 | READY | READY | 06 |
| LOAD-7.4 | READY | READY | 06 |
| LOAD-7.5 | READY | READY | 06 11 12 |
| LOAD-7.6 | READY | READY; a delta in the config's keys | 04 06 07 08 12 31 |
| LOAD-7.7 | READY | READY | 06 12 |
| LOAD-9 | DONE | DONE `3dcb1192` (Part A's A2): discovery is spawned first, and `read_installed` reads each package's directory from the … | 01 02 04 12 21 |
| SYN-1.1 | DONE | DONE `2f9b1281`: the loop, with a test at a depth the recursion needed a guard for; A6 (2026-10-19) found no standalone … | 02 04 07 |
| SYN-1.2 | READY | READY | 02 04 05 06 07 12 13 31 |
| SYN-1.3 | OPEN | OPEN | 07 11 12 13 |
| SYN-1.4 | OPEN | OPEN | 07 11 12 13 31 |
| SYN-1.5 | OPEN | OPEN | 07 12 13 |
| SYN-1.6 | SUPERSEDED | SUPERSEDED by SYN-1.2 (decision 4): a tree of vectors is `Send + Sync` with no `unsafe`, and the workspace denies … | 08 11 12 13 16 |
| SYN-1.7 | OPEN | OPEN | 07 12 13 |
| SYN-1.8 | OPEN | OPEN | 07 12 13 |
| SYN-1.9 | OPEN | OPEN | 05 07 08 11 12 13 15 16 21 22 |
| SYN-2 | OPEN | OPEN; the target grew | 05 07 09 11 12 13 31 |
| SYN-3.1 | READY | READY | 05 08 09 11 19 |
| SYN-3.2 | READY | READY, with SYN-3.1 | 05 07 08 09 12 |
| SYN-3.3 | READY | READY (its need, LOAD-1.4, is in: `3334ebc9`) | 05 |
| SYN-3.4 | OPEN | OPEN | 05 11 12 13 17 18 |
| SYN-4.1 | OPEN | OPEN | 05 07 09 11 12 |
| SYN-4.2 | READY | READY | 05 11 31 |
| SYN-4.3 | OPEN | OPEN | 05 07 09 11 |
| SYN-4.4 | READY | READY | 05 11 31 |
| SYN-4.5 | READY | READY | 05 07 11 12 18 31 |
| SYN-4.6 | READY | READY | 05 11 31 |
| SYN-4.7 | OPEN | OPEN | 05 11 31 |
| SYN-4.8 | READY | READY | 05 07 09 11 12 17 |
| SYN-4.9 | READY | READY | 05 07 09 11 12 |
| SYN-4.10 | READY | READY | 05 11 31 |
| SYN-6 | OPEN | OPEN | 05 07 08 11 12 15 16 |
| PROD-5.1 | OPEN | OPEN | 05 07 11 12 13 18 20 22 |
| PROD-5.2 | READY | READY | 02 05 09 11 23 |
| PROD-5.3 | READY | READY | 05 11 |
| PROD-5.4 | OPEN | OPEN | 05 06 07 12 21 22 31 |
| PROD-5.5 | READY | READY | 05 12 |
| PROD-5.6 | READY | READY | 05 |
| PROD-5.7 | OPEN | OPEN; one change with LOAD-4.7 | 02 05 07 12 21 22 31 |
| PROD-5.8 | READY | READY | 02 05 07 11 12 19 21 |
| DOC-2.1 | READY | READY, deferred 2026-10-16 (see WORK_STACK level 7) | 05 06 07 08 09 11 12 |
| DOC-2.2 | OPEN | OPEN | 05 07 08 09 12 23 |
| DOC-2.3 | READY | READY | 07 11 |
| DOC-2.4 | READY | READY | 05 07 11 |
| DOC-2.5 | OPEN | OPEN. DOC-1 rewrote the tag parser to pull tokens; the borrowed token text is not built | 05 07 09 12 |
| DOC-2.6 | OPEN | OPEN; overlaps REG-4.10 | 05 07 |
| DOC-2.7 | READY | READY | 07 09 12 |
| DOC-2.8 | READY | READY | 05 07 09 |
| DOC-2.9 | OPEN | OPEN; overlaps DOC-2.6 | 05 06 07 11 12 13 17 19 |
| NAME-1.1 | READY | READY | 05 11 18 |
| NAME-1.2 | READY | READY; the container-field keys are a delta, approved (VAL-2.7) | 05 06 07 11 12 18 |
| NAME-1.3 | OPEN | OPEN | 05 07 18 |
| NAME-1.4 | OPEN | OPEN | 05 18 |
| NAME-1.5 | READY | READY | 05 11 18 22 |
| NAME-1.6 | READY | READY | 05 11 18 |
| NAME-2.1 | READY | READY | 05 07 08 11 12 18 |
| NAME-2.2 | READY | READY | 05 12 18 |
| NAME-2.3 | READY | READY | 05 18 |
| NAME-2.4 | OPEN | OPEN | 05 06 |
| TYPE-4 | OPEN | OPEN | 05 06 18 19 21 |
| TYPE-4.1 | OPEN | OPEN | 06 |
| TYPE-4.2 | OPEN | OPEN | 06 |
| TYPE-4.3 | OPEN | OPEN | 06 |
| TYPE-4.4 | OPEN | OPEN | 06 |
| TYPE-5 | READY | READY | 05 07 09 11 12 31 |
| REG-2 | READY | READY. `xtask modgraph --inventory` lists the crossing edges; the `impl` and name inventories are not made | 06 |
| REG-4.2 | READY | READY; a delta, approved in the plan, fixtured | 05 06 18 20 |
| REG-4.5 | READY | READY. The plan names only the property case. Item 4 also made `redeclared_methods` a flag; since decision 3 it goes … | 06 |
| REG-4.7 | READY | READY | 06 |
| REG-4.9 | READY | READY | 06 11 12 |
| REG-4.10 | READY | READY; overlaps DOC-2.6 | 05 06 07 11 12 |
| REG-4.11 | OPEN | OPEN; the comments can be corrected now | 06 12 18 20 22 |
| REG-4.12 | READY | READY | 06 08 11 12 18 19 20 |
| REG-4.13 | READY | READY | 06 |
| REG-4.14 | READY | READY; a delta, approved (decision 14), fixtured | 06 19 |
| REG-4.15 | OPEN | OPEN | 05 06 07 11 12 18 20 22 |
| REG-5 | PART | PART: the purity part was done by REG-1; what is left is a delta, approved (decision 14): a call spelled in another … | 06 |
| REG-6 | READY | READY; a test-expectation delta, approved (decision 14) | 06 |
| REG-7.1 | OPEN | OPEN | 06 20 23 |
| REG-7.2 | OPEN | OPEN | 04 05 06 11 20 |
| REG-7.3 | OPEN | OPEN | 06 20 23 |
| REG-7.4 | OPEN | OPEN | 05 06 20 23 |
| REG-7.5 | OPEN | OPEN | 01 02 04 05 06 07 12 20 22 |
| REG-7.6 | OPEN | OPEN; a delta, approved (VAL-2.5) | 06 20 |
| REG-7.7 | OPEN | OPEN | 06 20 |
| REG-7.8 | OPEN | OPEN | 06 20 |
| REG-7.9 | OPEN | OPEN | 06 08 19 20 |
| REG-7.10 | OPEN | OPEN | 06 19 20 |
| REG-7.11 | OPEN | OPEN; it lands with the snapshots | 06 20 |
| REG-7.12 | OPEN | OPEN | 06 19 20 23 |
| REG-8.1 | OPEN | OPEN; a delta: no declaration's members merge into another's, and the server and the batch run bind a name alike … | 06 20 |
| REG-8.2 | SUPERSEDED | SUPERSEDED by REG-16.3 (decision 3): the read sets are the dependencies the engine records, absent names included | 06 20 |
| REG-8.3 | OPEN | OPEN | 02 05 06 20 |
| REG-8.4 | OPEN | OPEN | 06 20 |
| REG-8.5 | OPEN | OPEN; a delta, approved and corrected (decision 14; nsrt `property-hooks.php`) | 06 20 |
| REG-8.6 | OPEN | OPEN; overlaps NAME-1 and NAME-2 | 05 06 07 08 12 18 20 22 |
| REG-8.7 | OPEN | OPEN | 06 20 |
| REG-11 | OPEN | OPEN; each a delta, approved in the plan | 06 20 |
| REG-11.1 | OPEN | OPEN | — |
| REG-11.2 | OPEN | OPEN | — |
| REG-11.3 | OPEN | OPEN | — |
| REG-11.4 | OPEN | OPEN | — |
| REG-11.5 | OPEN | OPEN | — |
| REG-11.6 | OPEN | OPEN | — |
| REG-11.7 | OPEN | OPEN | — |
| REG-11.8 | OPEN | OPEN | — |
| REG-11.9 | OPEN | OPEN | — |
| REG-11.10 | OPEN | OPEN | — |
| REG-11.11 | OPEN | OPEN | — |
| REG-11.12 | OPEN | OPEN | — |
| REG-11.13 | OPEN | OPEN | 11 18 |
| REG-11.14 | SUPERSEDED | SUPERSEDED by decision 11: no declaration merges into another, and what is left, a built-in's readings and forms, is … | — |
| REG-11.15 | OPEN | OPEN | — |
| REG-11.16 | OPEN | OPEN | — |
| REG-12 | OPEN | OPEN | 06 19 20 22 |
| REG-14.3 | OPEN | OPEN | 06 12 |
| REG-14.4 | DONE | DONE `8739437e`; A6 (2026-10-19), off callgrind over the Piwigo run, the sum of `from_decl`'s inclusive Ir over all … | 01 02 04 12 |
| REG-14.5 | FOLDED | FOLDED into REG-7.5 (Part E 41): REG-7.5's combination is written without the `Vec`, and the function it would replace … | 01 02 04 07 12 22 31 |
| REG-16.2 | READY | READY | 05 06 07 08 11 12 22 31 |
| REG-16.3 | OPEN | OPEN | 05 06 11 |
| REG-16.4 | OPEN | OPEN | 02 04 05 06 07 08 11 12 13 20 21 22 23 31 |
| REG-16.5 | OPEN | OPEN | 06 12 20 21 22 |
| REG-17 | OPEN | OPEN | 05 06 08 12 19 20 21 |
| REG-17.1 | OPEN | OPEN | 20 23 |
| REG-17.2 | OPEN | OPEN | 11 20 24 |
| REG-17.3 | OPEN | OPEN | 20 |
| REG-17.4 | OPEN | OPEN | 20 |
| REG-18 | OPEN | OPEN | 06 |
| REG-18.1 | READY | READY | 06 |
| REG-18.2 | READY | READY | 06 20 |
| REG-18.3 | READY | READY | 06 20 23 |
| REG-18.4 | READY | READY; a delta | 05 06 07 08 12 18 22 31 |
| REG-18.5 | READY | READY | 06 |
| REG-18.6 | READY | READY | 03 06 11 12 20 |
| STUB-7 | OPEN | OPEN | 06 |
| STUB-7.1 | OPEN | OPEN; a delta, approved (decision 14) | 05 06 07 08 11 12 13 23 |
| STUB-7.2 | OPEN | OPEN; a delta, approved and corrected (decision 14) | 08 |
| STUB-7.3 | OPEN | OPEN; a delta, approved (decision 14) | 05 06 08 13 |
| STUB-7.4 | OPEN | OPEN | 08 23 |
| STUB-7.5 | OPEN | OPEN; a delta, approved (decision 14) | 02 04 06 07 08 12 21 22 31 |
| STUB-7.6 | OPEN | OPEN | 06 08 12 23 |
| STUB-7.7 | OPEN | OPEN | 08 |
| STUB-7.8 | OPEN | OPEN | 06 08 |
| STUB-7.9 | OPEN | OPEN; a delta, approved (decision 14) | 08 |
| STUB-7.10 | OPEN | OPEN; a delta, approved (decision 14) | 08 |
| STUB-7.11 | OPEN | OPEN | 08 23 |
| STUB-7.12 | OPEN | OPEN | 08 |
| STUB-8 | OPEN | OPEN; a delta, approved, both parts (VAL-2.4) | 05 06 23 |
| STUB-10 | PART | PART: decided (STUB-10.2); STUB-10.1's measurements are left | 06 |
| STUB-10.1 | OPEN | OPEN | 06 |
| STUB-11 | OPEN | OPEN | 05 06 07 12 21 23 24 |
| STUB-12.1 | OPEN | OPEN | 06 23 |
| STUB-12.2 | OPEN | OPEN | 06 08 12 23 |
| STUB-12.3 | OPEN | OPEN | 06 23 |
| STUB-12.4 | OPEN | OPEN | 06 12 23 |
| STUB-12.5 | OPEN | OPEN | 06 23 |
| STUB-13 | OPEN | OPEN | 06 07 23 |
| STUB-13.1 | OPEN | OPEN | 06 23 |
| STUB-13.2 | OPEN | OPEN | 06 |
| CACHE-8 | READY | READY | 05 07 12 22 31 |
| CACHE-9 | BLOCKED | BLOCKED: its form follows the stub layer and, since decisions 3 and 7, the persistence's format | 05 06 12 21 22 |
| CACHE-10 | BLOCKED | BLOCKED: as CACHE-9 | 06 12 21 22 |
| CACHE-11 | OPEN | OPEN | 06 12 22 |
| CACHE-13.1 | OPEN | OPEN | 05 06 07 11 12 19 21 22 |
| CACHE-13.2 | OPEN | OPEN | 05 06 07 08 12 21 22 23 |
| CHECK-4.2 | READY | READY | 05 07 12 22 31 |
| CHECK-6.8 | READY | READY | 05 06 11 12 23 |
| CHECK-6.18 | READY | READY | 05 06 12 14 16 23 |
| CHECK-7 | OPEN | OPEN; a delta, approved (VAL-2.5) | 05 06 07 08 12 21 |
| CHECK-8 | OPEN | OPEN | 06 08 21 |
| CHECK-9 | READY | READY | 05 06 07 11 12 23 24 31 |
| CHECK-10 | OPEN | OPEN | 05 06 07 08 12 23 |
| CHECK-11 | READY | READY; a delta | 06 07 08 12 31 |
| VAL-1.1 | OPEN | OPEN | 07 08 11 12 |
| VAL-1.2 | OPEN | OPEN | 08 11 12 |
| VAL-1.3 | OPEN | OPEN | 08 11 12 |
| VAL-1.4 | OPEN | OPEN | 05 08 09 11 12 |
| VAL-1.5 | OPEN | OPEN; `xtask front-end-dump` exists, and hashes only | 07 08 11 12 13 |
| VAL-1.6 | OPEN | OPEN | 05 08 11 12 |
| VAL-1.7 | OPEN | OPEN; `--dump-types` exists | 05 08 09 11 12 |
| VAL-1.8 | OPEN | OPEN; `cargo xtask corpus --full` exists | 07 08 11 12 |
| VAL-1.9 | OPEN | OPEN | 08 11 12 |
| VAL-1.10 | OPEN | OPEN | 08 11 12 |
| VAL-1.11 | OPEN | OPEN; MEAS-2 has the whole-stack profile at this code | 08 11 12 |
| VAL-1.12 | OPEN | OPEN | 06 08 11 12 |
| VAL-1.13 | OPEN | OPEN; the last step of the 20 G plan | 06 11 13 |
| VAL-1.14 | OPEN | OPEN | 11 |
| VAL-1.15 | OPEN | OPEN | 11 |
| Q-10 | OPEN | OPEN, partly probed (2026-10-16, `~/.cache/prs-ledger/q10/`). The recovery is in place: a context file with a syntax … | 05 07 08 11 12 13 15 16 19 |
| COMP-7 | DONE | DONE: the discovery admission `acf48fb8`, the walk's cut `16418a2c`, the tests `c14aa865`. The batch delta for a … | 01 02 03 04 07 12 31 |

<!-- status:end -->

Asked as: is every design the most proper, or does any still avoid churn. The first answer searched the text for
deferring phrases (the audit in Part E), which the user did not want as a method. This part went through the tasks in
the order of "The sequence this gives" and, for each, read its ledger row (or its finding), the plan's design for it
and the code it touches, and checked three things: that the design meets the row, that its facts hold in the code at
`52844b07`, and that it holds on the platforms the project ships (`CROSS_PLATFORM_PLAN.md`) and against the tasks it
meets. A verdict is "sound" (the row, the design and the code agree) or what changed. Nothing was built or run.

## K1. The tasks, in order

| # | task | verdict |
|---|---|---|
| 0 | the start condition | **changed.** The cross-platform plan now comes before this batch, not the items 1 to 3 fixes, which finished at `791a57c6` (R0 to R12 and R18 are built at `2c7bf898`, R13 to R17 open; this row's first version said R0 to R5, and the workflow review found it stale twice). Its rows edit the files Part A edits. The intro says so |
| 1 | A1, LOAD-2.2's last part | **redesigned twice.** First, a list of known vendor directories missed a nested project's `vendor/` (the name test skipped it) in a monorepo opened at its root, so the rule became local to the walk, `vendor_dir(dir)` where a listing holds `composer.json`. Then, in the audit of 2026-10-18, that rule was found to walk a nested project's out-of-subtree vendor directory, to need carried targets and a fact from the parent in the reach tables, and to miss R8's `trees` and `unbuilt`: a directory Composer wrote into is now recognised from its own listing, and the project's directory is `vendor_location`, over candidates (A1; the mechanism was ruled on 2026-10-18, M5d), and the install directories of packages Composer put elsewhere are Composer's too (COMP-7). It follows the cross-platform plan's R7 (the case rule on the names) |
| 2 | A2, LOAD-9 | **amended.** The listing shortcut for the install path is exact only if `vendor/composer` is a real directory (a linked one is tested); discovery, spawned first, takes the one `vendor_location` answer; `DirListings` is safe to ask from both threads (read) |
| 3 | A3, REG-14.4 | **changed.** A predicate `Fn(&str) -> bool`, not `&[Param]`: item 9's step 1 turns the list into a view and the assert code then needs no second change |
| 4 | A4, REG-14.5 | **folded into REG-7.5** (Part E 41). Decision 8's final-level rule replaces `reconcile_with_native` and both its callers; the function is written there as A4 now describes |
| 5 | A5, SYN-1.1 | sound (the loop; the guard in `Drop` stays until step 2) |
| 6 | A6, bookkeeping | sound; one line removed with A4 |
| 7 | VAL-1.1 | sound (`1892b477` exists; the six listed differences are item 0.1's) |
| 8 | F10 | **already fixed in form.** `can_begin_expression` (`parser/expr.rs:211`) lists the tokens that cannot begin an expression, `and`, `or`, `xor`, `instanceof`, `&` and `\|>` among them, since `3b7e4d95` (2026-10-03, after the review's design); `php -l` accepts every shape the finding lists. Unbuilt: the exhaustive allow-list (a new token must be classified), its table test, the seven parser tests and the fixture. Tests and fixture in batch 0; the predicate on SYN-2's `TokenKind` in batch 1 |
| 9 | F20 | sound; after the cross-platform plan's R10 (which edits `syntax::stack`, and is built) and before item 9, so its lint checks the recursion item 9 writes; its `stack` crate restores `LIMIT` from a drop guard so that it runs on unwind too (H2.8) |
| 10 | F57 | **already built** (checked in `parse_arrow_fn` and `parse_ternary`); its box in the review is stale |
| 11 | item 0's tools, VAL-1.2 to 1.12 | **reordered.** VAL-1.2's run includes the edit replay, so the oracle run follows the session and the replay; F21 precedes the printers (it changes `Repr::Mixed`) |
| 12 | level 3 | **recounted and placed** (K2) |
| 13 | the error-node model, G3 | **one addition.** A file's validity range (its marks) is its declarations' availability (decision 15, Q-10), which `declarations_named` now returns with each candidate |
| 14 | item 9, step 0, SYN-1.11 | sound (the fabrication sites B6 puts in step 0 exist: `static_call` and `constant_fetch`, `var_tag_types.rs:537`, and `phpdoc_type_compat.rs:268-300`; `calls.rs:777` follows in step 1 and the probe in step 2) |
| 15 | step 1, SYN-1.3 and 1.12 | sound |
| 16 | step 2, SYN-1.2, 1.4, 1.5, 1.7, 1.8 | sound against the five rows; **I1.5 gains closure and arrow-function doc owners** (the Stage B review's C2 needs them) |
| 17 | step 3, LOAD-3.5 | sound; the engine design is corrected to agree with it (K3) |
| 18 | batch 1, SYN-2 | **amended.** `LexEnd`'s second pass is the lex-the-rest after a failed parse, which the whole-file stream has none of; but a whole-file lex must stop at `__halt_compiler` (the parser calls `skip_to_eof` today, and the data after it is binary), a NUL byte is an `Error` token and not `Eof`, and templates, heredocs, casts and inline HTML need the rows I2 now has |
| 19 | batch 2, SYN-3.1 to 3.4, SYN-4.1 to 4.10 | sound; SYN-3.3 is not an interim (SYN-3.1 gives a context file no columns); SYN-4.1's lookup half goes (the lexer interns nothing); SYN-4.5 moves to batch 4, since it is `ConstName` |
| 20 | batch 3, SYN-1.9, Q-10, SYN-6, F77 | sound |
| 21 | batch 4, NAME-2.1 to 2.3, NAME-1.1 to 1.6 | **three notes.** NAME-2.2's weak pairs are for the type tables, and names are held strongly and swept (H1.3); NAME-1.2 converts the `String` keys of the machinery W2 and W3 delete, because NAME-1.3 cannot delete `FoldKey for str` otherwise; NAME-1.5's hash must not depend on chunking (S-1) |
| 22 | batch 5, PROD-5.1 to 5.3, 5.5, 5.6, 5.8 | **PROD-5.5 keyed by ordinal, not span** (H1.5a); PROD-5.4 is deleted by REG-16.4; PROD-5.7 not built (Part E 35) |
| 23 | batch 6, DOC-2.1 to 2.9, TYPE-5 | **amended.** TYPE-5 is its own commit after I4.5 step 5 (it is the ledger's rule; F14's design text is superseded), never inside the behaviour-exact `phpdoc_keyword` extraction; the extraction also completes the keyword table |
| 24 | batch 7, the check's temporaries | sound; CHECK-6.8 and 6.18 reuse only what holds no tree handle (G1); CHECK-4.2 goes with REG-18.4 (Part E 41) |
| 25 | batch 8, LOAD-5.4, CACHE-13.1, CACHE-8 | CACHE-13.1's stamp follows decision 13 (the stat tuple on known file systems, the content hash on Windows and elsewhere); **CACHE-8 waits for S-4** (Part E 41). The second pass moved LOAD-5.4 to Part A and CACHE-13.1 to W3 (Part E 15), which leaves batch 8 empty |
| 26 | spikes S-1 to S-11 | S-1 gains the hash contract; S-3 the member grain; S-6 the mixed-strategy cycles; S-7 is rewritten (K3); S-11 is new |
| 27 | W1, the fillers | LOAD-7.3 follows the cross-platform plan's R6 and R7 (one matcher; `report_path` and the case rule are its inputs) |
| 28 | W2, REG-4.2, 4.12, 4.15, 16.2, 16.3, 7.1 to 7.12, 8.1 to 8.7, 9 to 12 | **H3.6 gains REG-4.15, REG-4.11 and REG-11**; REG-17 gets its layering (H3.8); salsa's cycle rule is stated (H3.3) |
| 29 | W3, REG-16.4, CACHE-13.2, LOAD-3.1, LOAD-5.x, TYPE-4, CHECK-7, CHECK-8, REG-17, F1 and F2 | **trees corrected** (K3); STUB-7.5 moves here; the persisted file is built for every platform and one platform's file is never read by another's (H4.4); TYPE-4.2's "provisional answers" cannot come from the hierarchy, which has no salsa cycle, and the relation reads resolved types, not constants' values |
| 30 | W4, STUB-7.x, 8, 12, CHECK-10, STUB-13.1 | **`effective_range` and the phar locator** added (J1.2) |
| 31 | W5, OVL, RES, EXT, SUM | rows written from the crate split's C6 text; the workflow review found omissions and stale figures (W-10 to W-12), which J2 now corrects (RES-7, OVL-3, OVL-1, EXT-1) |
| 32 | W6, STUB-10, 11, 13 | sound |
| 33 | W7, Stages D to G | reconciled in J3 against D1, D3 and D4 (read first), and by the workflow review against E1, E7 and G1; **not yet read:** D5, D6, E2 to E6, F1 to F5 and G2 to G5, whose crate-split text stays the authority for each phase |
| 34 | W8, VAL-1.13 to 1.15 | sound |

### K1b. The wall's rows, one by one

Each row below was read in the ledger and set against H1 to H4 and J1 to J3. "H3.1" and the like say where it is
designed.

| row | verdict |
|---|---|
| REG-2 | sound (an `xtask` inventory) |
| REG-4.5, 4.7, 4.13, 4.14 | sound; flags and options carry no position (H1.5a); 4.14 is H2.7's determinism rule |
| REG-4.9 | built with REG-4.12: its `Option<&T>` and slices are the `Provider`'s `&'db T` |
| REG-4.10 | folded into DOC-2.9 (batch 6): the builder parses each doc comment once by reading the file's `doc_blocks` (H2.3) |
| REG-5 | sound; its case delta is F50's |
| REG-6, REG-18.1, 18.2, 18.3, 18.5, 18.6, CHECK-11 | sound |
| REG-18.4 | sound; takes CHECK-4.2 (Part E 41) and `ConstName` (H1.2) |
| LOAD-7.1, 7.4 to 7.7 | sound; 7.3 follows the cross-platform plan's R6 and R7; 7.2's PHPUnit half still needs the user's ruling; 7.8 is each setting's phase |
| REG-4.2 | sound (H1.2: `FoldedName` keys; two names differing in case are one name, so two declarations become `Many` and a duplicate report) |
| REG-4.12 | sound (H2.4: about 280 sites become `Provider` calls; the grain of a read is the query that answers it) |
| REG-4.15 | its span field withdrawn (H1.5a, Part E 34) |
| REG-4.11 | sound; after REG-8.5 and 10 (H3.6 step 5) |
| REG-16.2 | "never reused" reworded (Part E 38); the ids are named in the engine module only (H1.6) |
| REG-16.3 | sound; purity and global variables are queries (H2.3, H3.7), further than the row's "run again per revision" |
| REG-7.1 to 7.5, 7.12 | H3.1; 7.5 is decision 8's rule at the final level and replaces `reconcile_with_native` (A4) |
| REG-7.6 | H3.3 (`cycle_result`; the fallback per constant for the PHPDoc case is S-6's) |
| REG-7.7 | H3.5 (`ClassDecl` keeps the scratch the row names) |
| REG-7.8 | `finish_loading` stays until W3, then `Session::load` (H3.5, H4.1) |
| REG-7.9 | H2.8 and H3.3 (a cycle is homogeneous) |
| REG-7.10, 7.11 | H3.5 |
| REG-8.1, 8.3, 8.5 | H3.5, with `file_decls` an input in W2 (H3.6) |
| REG-8.2 | superseded (the recorded reads) |
| REG-8.4 | H3.5 (`Provider`; no `ReflectionBuilder` is split off) |
| REG-8.6 | re-specified (H1.3) |
| REG-8.7, 9, 10 | H3.5, H3.6 step 4 |
| REG-11.1 to 11.16 | H3.6 step 6; 11.14 is superseded; 11.4 and 11.5 are F75 and F35, F55 |
| REG-12 | H3.6 step 7; its "resolves no class the file does not reach" is a count from the event callback |
| REG-16.4 | H4.1, H4.7; trees corrected (K3); needs SYN-1.9 |
| CACHE-13.2 | H4.6; keyed per config key (Part E 37); the stamp follows decision 13 |
| CACHE-9 to 11 | after S-4; one format bump |
| LOAD-3.1, 3.3's `update_file` half | H2.9: a tree is kept by (role, open): `parse_analysed` for an analysed file, under `lru` in the server; a context file's is never kept |
| LOAD-4.7's index half, PROD-5.4, PROD-5.7 | deleted by REG-16.4 (H4.8, Part E 35) |
| LOAD-5.1 | E1's collectors (Part E 39); the use-site index is `file_products.use_sites` |
| LOAD-5.2 | memory only; S-7 decides (K3) |
| TYPE-4.1 to 4.4, NAME-2.4 | H2.6: the memo is cleared by the first access in a later revision |
| REG-14.3 | after REG-8.3 (`FileDecls`' parameter vector) |
| REG-16.5 | H3.5 |
| REG-17.1 to 17.4 | H3.8 (new in this review) |
| CHECK-7, CHECK-8 | after REG-7.6 and REG-7.5; constants return value and throws (H3.3) |
| STUB-7.1 to 7.4, 7.6 to 7.12 | J1; `effective_range` and the phar locator added; 7.5 moves to W3 (H4.1) |
| STUB-8, STUB-12.1 to 12.5, CHECK-10 | J1.3 to J1.5 |
| STUB-10.1, 11, 13.1, 13.2 | J1.3; the builtin input holds a runtime corpus until STUB-11 (J1.2) |
| OVL-1 to 8, RES-1 to 7, EXT-1 to 4, SUM-1 to 3 | J2, read against the crate split's C6 text: its oracle-miss lists become each row's fixtures (RES-7 is the extension point and the three behaviours the first version dropped) |

## K2. Level 3: what the 136 are

The count overstates the work. `STAGE_B_FIX_REVIEW.md` has 88 open findings, a validation and 16 pointer lines (the
earlier reviews' items that a finding re-opens, listed again under "Full review"); `STAGE_B_REVIEW.md` has 15 and a
validation; `STAGE_C_REVIEW.md` has 14 and a validation, of which O1 and F5 are fixed (`e06f470c`, `1cdfe80d`; the
fix review says so) and two are the same bug as a fix-review finding (Stage C's F4 is F55, its F8 is F30). That is
about 115 findings. Whether each still reproduces is settled by running its probe on the gate binary in batch 0, not by
reading. **The rule for when each is fixed:** a finding in code that a later task rewrites gets its fixture now and
its fix inside that task; one that changes what the baseline records is fixed before the baseline; the rest go in the
review's own order between heavy runs.

| finding | meets | placed |
|---|---|---|
| F10 | the item 0 baseline (a skipped file has no tree dump) | its symptom is fixed; tests and fixture before the baseline, the exhaustive predicate in batch 1 |
| F21 | the canonical printers (`Repr::Mixed` changes) | before them |
| F20 | the cross-platform plan's R10; item 9 | after R10 and before item 9, so the lint checks the recursion item 9 writes |
| F77 | the item 0 baseline (a refused file must show in O2) and SYN-1.9 | batch 0, for analysed files; its context-file half is a run-level notice that leaves the exit status (ruled 2026-10-18, M5); batch 3 feeds it from `errors()` |
| the seven fixes the crate split's F1 puts first (debounce stack, `latest_text`, `didClose`, `--generate-baseline`, `--fix`, the closed pipe, rename) | none: they are on today's code and independent | batch 0, each with its test (all open at `ffa7cccb`) |
| F22 (closure `use` list), F8 and F7 (the use-site walk) | item 9 step 1 (`Visitor` over handles) | in step 1 |
| F14, F41, F19 (the PHPDoc bullets) | DOC-2.1, TYPE-5 | batch 6 |
| F69, T2 | CHECK-9, REG-17.2 | F69's fixtures in batch 0; F69 and CHECK-9 in D2 (they sit on the recorders D2 replaces), T2 with REG-17.2. T2 asks "decide whether callee throw points count" and is a decision row: they do, as PHPStan's `MissingCheckedExceptionInMethodThrowsRule` reads the throw points of calls, and F69's throw points for implicit calls and REG-17.2's inferred throws are what make it complete, so T2 is built with them and the rule reports both a direct call and `$o[…]` on an `ArrayAccess` object |
| F5 (definedness certainty), F6 (closure-literal alias), F45 (reference groups), F61 (unset properties) | D1 and D5 (holder storage): each design adds per-variable state to `Locals`, and the fix review's own first step is one `ExpressionHolder` per narrowing key | fixture now, fix inside D1 and D5 (J3); F6's design stores an `Rc<Expr>`, which G1 and G2 replace by a rooted alias that D1 deletes |
| F18 | the `&&`/`\|\|` and nested-`match` causes are Stage D's one inference; the trait chain is REG-7.2's; the `.` rope and the `elseif` suffix are local | fixture now (the scale scripts), local ones now, the rest in D3 and W2 |
| F35, F50, F55 (Stage C F4), F75, F78, Stage B D1 | REG-4.2, REG-5, REG-11.4 and 11.5, REG-8.3, REG-7.5 | fixture now, fix in W1 and W2 |
| F76 | REG-7.10 (namespace cycles) | W2 |
| F58 | G1.9 and G4a (no per-function clone; the `Resolved` is built only when the anchor matches, in both walkers) | resolved by the design |
| F33 (anonymous class names, infer cache assumption) | Part E 34, G1.5, H1.4 | the name is the shown path and the line, as its design states (`File.shown`, H2.2); the infer cache's context epoch and the env-gated recompute-on-hit assertion are G1.5's |
| F30 (Stage C F8) | REG-4.15's builder | fixture now |
| Stage C F1 | decision 11, REG-8.1 | W2 |
| Stage C F5, F6, F7 | REG-11.10 (F5, fixed), REG-18 and J1 (F6, F7) | W1, W4 |
| Stage C F2, F3 | LOAD-7.3 and 7.4, STUB-7.3 | W1, W4 |
| Stage C F9, F10, F12 | LOAD-7.6, LOAD-7.7 | W1 |
| Stage C F11 | VAL-1.3, REG-8.5 | batch 0, W2 |
| F74 | the composer follow-ups of Part E 40 | with them |
| Stage B "Typed purity", D2 | REG-17.4 and SUM-3, REG-11.6 and EXT-2 | W2, W5 |
| the comments (F24, F34, F37, F43, F51, F59, F80, F88, Stage C F13) and the fixtures (F23) | none | any gap; F23 before the baseline |
| everything else (F2 to F6, F9, F11 to F13, F15 to F17, F25 to F32, F36, F38 to F40, F42 to F49, F52 to F56, F60 to F73, F79, F81 to F87, F89; Stage B W1, C2, M1, CMP-1, DF-1 to DF-7) | none | in the review's own order, F4 first |

## K3. The correction the review found in the engine

H2.9 had the batch run keep trees as a tracked `parse` with `lru`, evicted by `trigger_lru_eviction` between chunks.
That frees every evicted tree on the one thread that evicts, which is the cost LOAD-3.3 measured and removed (−138.6 M
Ir and −214 MiB peak when a context tree's drop moved to the worker that built it), and most trees are context trees.
The design now is: a context file's tree is built in the worker's scratch builder inside `file_products` and cleared
(LOAD-3.5), never a memo; an analysed file's tree is the memo `parse_analysed`, kept for the run in the batch and
under `lru` in the server, so the batch and the server hold trees alike. LOAD-5.2 is memory only, item 9 shrinks what it
frees from 151 MB (MEAS-2), and S-7 decides whether bounding the batch run is worth its frees (W3 builds LOAD-5.2 as
the ledger has it until S-7 says otherwise: H2.9). REG-16.4's phrase "a tree is a
parse result the engine evicts" holds for the analysed trees and not for context trees; that is a deviation from the
ledger's wording, for the user to see.

---
