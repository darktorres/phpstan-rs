# Part D. After batch 8: the wall (levels 1 to 7)

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `54abfd547` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| LOAD-1 | DONE | DONE (LOAD-1.6 SUPERSEDED by REG-16.4) | 07 08 |
| LOAD-1.3 | DONE | DONE `c9f6a85c` | 11 |
| LOAD-1.4 | DONE | DONE `3334ebc9` | 21 |
| LOAD-3.1 | OPEN | OPEN | 02 04 05 10 12 19 21 |
| LOAD-4.6 | SUPERSEDED | SUPERSEDED by REG-7 and REG-16.4 (decision 3). `closure_scope` computes the finishing passes' scopes … | — |
| LOAD-4.7 | OPEN | OPEN, blocked. Its file-references part is one change with PROD-5.7 (OPEN), which needs PROD-5.8 and PROD-5.2, both … | 02 05 07 10 12 21 22 31 |
| LOAD-5.1 | OPEN | OPEN | 04 05 07 10 12 19 21 23 31 |
| LOAD-5.2 | OPEN | OPEN | 05 07 10 12 19 21 22 |
| LOAD-7 | OPEN | OPEN | — |
| LOAD-7.1 | READY | READY | 07 10 |
| LOAD-7.2 | OPEN | OPEN; the PHPUnit half needs a ruling | 07 08 12 31 |
| LOAD-7.3 | READY | READY | 10 |
| LOAD-7.4 | READY | READY | 10 |
| LOAD-7.5 | READY | READY | 10 11 12 |
| LOAD-7.6 | READY | READY; a delta in the config's keys | 04 07 08 10 12 31 |
| LOAD-7.7 | READY | READY | 10 12 |
| LOAD-8.8 | DONE | DONE `f676049c`. Before `finalize_results` a project's diagnostics are keyed by the file's number: the per-file checks, … | — |
| SYN-1.2 | READY | READY | 02 04 05 07 10 12 13 31 |
| SYN-1.10 | OPEN | OPEN | 05 07 11 12 13 22 |
| PROD-5.4 | OPEN | OPEN | 05 07 10 12 21 22 31 |
| DOC-2.1 | READY | READY, deferred 2026-10-16 (see WORK_STACK level 7) | 05 07 08 09 10 11 12 |
| DOC-2.9 | OPEN | OPEN; overlaps DOC-2.6 | 05 07 10 11 12 13 17 19 |
| NAME-1 | OPEN | OPEN | 05 11 |
| NAME-1.2 | READY | READY; the container-field keys are a delta, approved (VAL-2.7) | 05 07 10 11 12 18 |
| NAME-2.4 | OPEN | OPEN | 05 10 |
| TYPE-4 | OPEN | OPEN | 05 10 18 19 21 |
| TYPE-4.1 | OPEN | OPEN | 10 |
| TYPE-4.2 | OPEN | OPEN | 10 |
| TYPE-4.3 | OPEN | OPEN | 10 |
| TYPE-4.4 | OPEN | OPEN | 10 |
| REG-1 | DONE | DONE `c0dd2788` | 19 20 |
| REG-2 | READY | READY. `xtask modgraph --inventory` lists the crossing edges; the `impl` and name inventories are not made | 10 |
| REG-3 | DONE | DONE `58cfc5d2` | — |
| REG-4 | PART | PART | 08 |
| REG-4.1 | DONE | DONE (already on this branch) | — |
| REG-4.2 | READY | READY; a delta, approved in the plan, fixtured | 05 10 18 20 |
| REG-4.3 | DONE | DONE `5401afc9` | — |
| REG-4.4 | DONE | DONE `8d35c99d` | — |
| REG-4.5 | READY | READY. The plan names only the property case. Item 4 also made `redeclared_methods` a flag; since decision 3 it goes … | 10 |
| REG-4.6 | DONE | DONE `7a61603e` | — |
| REG-4.7 | READY | READY | 10 |
| REG-4.8 | DONE | DONE `e7b2521f`, corrected `0f32829b` | — |
| REG-4.9 | READY | READY | 10 11 12 |
| REG-4.10 | READY | READY; overlaps DOC-2.6 | 05 07 10 11 12 |
| REG-4.11 | OPEN | OPEN; the comments can be corrected now | 10 12 18 20 22 |
| REG-4.12 | READY | READY | 08 10 11 12 18 19 20 |
| REG-4.13 | READY | READY | 10 |
| REG-4.14 | READY | READY; a delta, approved (decision 14), fixtured | 10 19 |
| REG-4.15 | OPEN | OPEN | 05 07 10 11 12 18 20 22 |
| REG-5 | PART | PART: the purity part was done by REG-1; what is left is a delta, approved (decision 14): a call spelled in another … | 10 |
| REG-6 | READY | READY; a test-expectation delta, approved (decision 14) | 10 |
| REG-7 | OPEN | OPEN | 05 08 12 18 19 20 21 23 |
| REG-7.1 | OPEN | OPEN | 10 20 23 |
| REG-7.2 | OPEN | OPEN | 04 05 10 11 20 |
| REG-7.3 | OPEN | OPEN | 10 20 23 |
| REG-7.4 | OPEN | OPEN | 05 10 20 23 |
| REG-7.5 | OPEN | OPEN | 01 02 04 05 07 10 12 20 22 |
| REG-7.6 | OPEN | OPEN; a delta, approved (VAL-2.5) | 10 20 |
| REG-7.7 | OPEN | OPEN | 10 20 |
| REG-7.8 | OPEN | OPEN | 10 20 |
| REG-7.9 | OPEN | OPEN | 08 10 19 20 |
| REG-7.10 | OPEN | OPEN | 10 19 20 |
| REG-7.11 | OPEN | OPEN; it lands with the snapshots | 10 20 |
| REG-7.12 | OPEN | OPEN | 10 19 20 23 |
| REG-8 | OPEN | OPEN | 08 18 20 22 |
| REG-8.1 | OPEN | OPEN; a delta: no declaration's members merge into another's, and the server and the batch run bind a name alike … | 10 20 |
| REG-8.2 | SUPERSEDED | SUPERSEDED by REG-16.3 (decision 3): the read sets are the dependencies the engine records, absent names included | 10 20 |
| REG-8.3 | OPEN | OPEN | 02 05 10 20 |
| REG-8.4 | OPEN | OPEN | 10 20 |
| REG-8.5 | OPEN | OPEN; a delta, approved and corrected (decision 14; nsrt `property-hooks.php`) | 10 20 |
| REG-8.6 | OPEN | OPEN; overlaps NAME-1 and NAME-2 | 05 07 08 10 12 18 20 22 |
| REG-8.7 | OPEN | OPEN | 10 20 |
| REG-9 | OPEN | OPEN; deltas, approved in the plan | 20 |
| REG-10 | OPEN | OPEN | 20 |
| REG-11 | OPEN | OPEN; each a delta, approved in the plan | 10 20 |
| REG-12 | OPEN | OPEN | 10 19 20 22 |
| REG-14.2 | DONE | DONE by REG-4.8 (`e7b2521f`, `0f32829b`); no such field is left | — |
| REG-14.3 | OPEN | OPEN | 10 12 |
| REG-15 | OPEN | OPEN | 24 |
| REG-15.1 | OPEN | OPEN | 23 24 |
| REG-15.2 | OPEN | OPEN | — |
| REG-16 | OPEN | OPEN | — |
| REG-16.1 | DONE | DONE (2026-10-15): salsa, with each name asked once per file (its record) | 08 18 19 22 |
| REG-16.2 | READY | READY | 05 07 08 10 11 12 22 31 |
| REG-16.3 | OPEN | OPEN | 05 10 11 |
| REG-16.4 | OPEN | OPEN | 02 04 05 07 08 10 11 12 13 20 21 22 23 31 |
| REG-16.5 | OPEN | OPEN | 10 12 20 21 22 |
| REG-17 | OPEN | OPEN | 05 08 10 12 19 20 21 |
| REG-18 | OPEN | OPEN | 10 |
| REG-18.1 | READY | READY | 10 |
| REG-18.2 | READY | READY | 10 20 |
| REG-18.3 | READY | READY | 10 20 23 |
| REG-18.4 | READY | READY; a delta | 05 07 08 10 12 18 22 31 |
| REG-18.5 | READY | READY | 10 |
| REG-18.6 | READY | READY | 03 10 11 12 20 |
| STUB-7 | OPEN | OPEN | 10 |
| STUB-7.1 | OPEN | OPEN; a delta, approved (decision 14) | 05 07 08 10 11 12 13 23 |
| STUB-7.3 | OPEN | OPEN; a delta, approved (decision 14) | 05 08 10 13 |
| STUB-7.5 | OPEN | OPEN; a delta, approved (decision 14) | 02 04 07 08 10 12 21 22 31 |
| STUB-7.6 | OPEN | OPEN | 08 10 12 23 |
| STUB-7.8 | OPEN | OPEN | 08 10 |
| STUB-8 | OPEN | OPEN; a delta, approved, both parts (VAL-2.4) | 05 10 23 |
| STUB-10 | PART | PART: decided (STUB-10.2); STUB-10.1's measurements are left | 10 |
| STUB-10.1 | OPEN | OPEN | 10 |
| STUB-11 | OPEN | OPEN | 05 07 10 12 21 23 24 |
| STUB-12 | OPEN | OPEN | 08 23 |
| STUB-12.1 | OPEN | OPEN | 10 23 |
| STUB-12.2 | OPEN | OPEN | 08 10 12 23 |
| STUB-12.3 | OPEN | OPEN | 10 23 |
| STUB-12.4 | OPEN | OPEN | 10 12 23 |
| STUB-12.5 | OPEN | OPEN | 10 23 |
| STUB-13 | OPEN | OPEN | 07 10 23 |
| STUB-13.1 | OPEN | OPEN | 10 23 |
| STUB-13.2 | OPEN | OPEN | 10 |
| CACHE-9 | BLOCKED | BLOCKED: its form follows the stub layer and, since decisions 3 and 7, the persistence's format | 05 10 12 21 22 |
| CACHE-10 | BLOCKED | BLOCKED: as CACHE-9 | 10 12 21 22 |
| CACHE-11 | OPEN | OPEN | 10 12 22 |
| CACHE-13.1 | OPEN | OPEN | 05 07 10 11 12 19 21 22 |
| CACHE-13.2 | OPEN | OPEN | 05 07 08 10 12 21 22 23 |
| CHECK-6.1 | READY | READY; approved in principle (VAL-2.8) | 05 07 12 23 |
| CHECK-6.2 | READY | READY | 07 12 23 |
| CHECK-6.3 | OPEN | OPEN | 05 07 12 13 14 23 |
| CHECK-6.4 | READY | READY | 05 07 08 11 |
| CHECK-6.5 | READY | READY | 05 07 11 |
| CHECK-6.6 | READY | READY | 05 07 |
| CHECK-6.7 | OPEN | OPEN | 05 07 12 23 |
| CHECK-6.8 | READY | READY | 05 10 11 12 23 |
| CHECK-6.9 | READY | READY | 05 07 |
| CHECK-6.10 | READY | READY | 05 07 08 09 12 23 |
| CHECK-6.11 | READY | READY | 05 07 |
| CHECK-6.12 | READY | READY | 05 07 |
| CHECK-6.13 | OPEN | OPEN | 05 07 12 23 |
| CHECK-6.14 | OPEN | OPEN | 05 07 12 23 |
| CHECK-6.15 | OPEN | OPEN | 05 07 12 23 |
| CHECK-6.16 | READY | READY | 05 07 |
| CHECK-6.17 | READY | READY | 05 07 |
| CHECK-6.18 | READY | READY | 05 10 12 14 16 23 |
| CHECK-7 | OPEN | OPEN; a delta, approved (VAL-2.5) | 05 07 08 10 12 21 |
| CHECK-8 | OPEN | OPEN | 08 10 21 |
| CHECK-9 | READY | READY | 05 07 10 11 12 23 24 31 |
| CHECK-10 | OPEN | OPEN | 05 07 08 10 12 23 |
| CHECK-11 | READY | READY; a delta | 07 08 10 12 31 |
| VAL-1 | OPEN | OPEN | 05 07 |
| VAL-1.12 | OPEN | OPEN | 08 10 11 12 |
| VAL-1.13 | OPEN | OPEN; the last step of the 20 G plan | 10 11 13 |
| VAL-2.4 | DONE | DONE: approved, both parts | — |
| VAL-2.9 | DONE | DONE: approved (decision 14), STUB-7.2 and REG-8.5 corrected; each is read at VAL-1 as it lands | 07 12 20 |
| VAL-2.10 | OPEN | OPEN: read at VAL-1, as they land | 05 07 12 15 20 |
| Q-11 | DONE | DONE (2026-10-15): of the 20 follow-ups, 1 is done for one version, 5 are partly done and 14 are not; their section … | 07 11 12 31 |

<!-- status:end -->

Read from `WORK_STACK.md` (levels 1 to 7), the ledger's REG, STUB, CACHE, CHECK, TYPE, NAME, VAL and Q
sections, `PROFILE_DRILL_PIWIGO_20G.md` §12, and `CRATE_SPLIT_PLAN.md`'s stages C to G (the plan was
last edited on 2026-10-18, `c1a7e224`, for the module graph; its stage text dates from 2026-09-28). Nothing here is
built or measured. Figures are the ledger's and the
reports', and "none" means the ledger gives no instruction figure. Steps are W1 to W8 (W for wall),
so they do not collide with Part B's D1-D9 or the crate split's phases D1-D6.

## What is left when batch 8 is done

Level 8's READY list is empty, so the stack's rule ("work the lowest item that has a READY row")
picks nothing. Level 2 still has READY rows (REG-2, REG-4.2, 4.5, 4.7, 4.9, 4.10, 4.12 to 4.14,
REG-5, REG-6, REG-16.2 and the Q-11 follow-ups), but levels 1 to 7 were deferred on 2026-10-16 "to
be reevaluated together". The 20 G plan's unbuilt rows, and what they are worth:

| remaining | needs | worth |
|---|---|---|
| REG-7, REG-8: the finishing passes become queries | REG-4.2, REG-16.3 | **wall time**: the passes run 1.16-1.52 s of the 2.02 s off run and 0.75-1.19 s of the 1.70 s warm run, at 2 to 3 of 8 cores (MEAS-4, the runs under `perf record`; the ledger's medians are 1.796 s off at `a5a4d85f` and 1.322 s warm at `6705eedd`). Ir: none; the passes cost 0.97 G, plus ≈0.7 G that other threads steal |
| CACHE-13.2: the engine's results persisted | REG-16.4 | the warm run's parse of the analysed files (1,967.9 M), their check (6,414.6 M) and the per-file decode (≈1,260 M), of 13.18 G, where a file's recorded reads did not change (MEAS-1): the ceiling, for a re-run that changed nothing |
| STUB-11: the builtin knowledge built at compile time | STUB-10.1, REG-15 | the stub build is 3.18 G of the 20.93 G off run at `791d29cb` (20.37 G at `8a1ca1e0`); the report's estimate after C4 and C5 is ≈0.2-1.5 G per cold run |
| item 6: REG-7.2, STUB-8, CACHE-9/10 | REG-7.1, STUB-7.1, 7.6, the persistence's format | ≈0.35 G and ≈40 MB; the warm run decodes 87,146 stub function records (682.5 M) |
| item 7: TYPE-4, then NAME-2.4 | REG-16.3 | up to ≈0.3 G (`is_super_type_of` 422.5 M) |
| LOAD-3.1, LOAD-5.1's use-site index, LOAD-5.2 | REG-16.4 | memory: the analysed trees (151 MB in MEAS-2; 173 MB at `5fb858c9`) freed as each check ends |
| REG-17, CHECK-7, CHECK-8, CHECK-10 | REG-16.4; REG-7.6; REG-7.5; STUB-7.1 | semantic, not savings; REG-17 adds work |
| tidying: PROD-5.4, SYN-1.10, REG-14.3, LOAD-4.7's index half | STUB-7.3; REG-8.3; REG-16.4 | deletions behind the rows above |

## What the wall contains

| part | rows | state |
|---|---|---|
| C4, the registry | REG-2 to REG-12, REG-16, REG-18 | tracked. Done: REG-1, 3, 4.1, 4.3, 4.4, 4.6, 4.8, 14.2, 16.1 |
| C5, the stub layer | STUB-7, 8, 10 to 13; CACHE-9 to 11 | tracked; nothing built |
| C1 to C3 follow-ups (Q-11) | LOAD-7.x, REG-18.x, CHECK-11 | tracked; READY: LOAD-7.1, 7.3 to 7.7, REG-18.1 to 18.6, CHECK-11 |
| C6 to C9: overlays, `TypeNodeResolver`, extension points, summaries | none | **not tracked** (REG-15.1 says so); about 300 lines of the plan |
| C10 | REG-15 | tracked; needs C6 to C9 |
| Stages D to G: the engine, everything out of it, the Session and the LSP, the extraction wave | none | **not tracked**; about 5,000 lines of the plan (D 1,626, E 2,074, F 913, G 425) |
| level 3 | 136 open review tasks (`STAGE_B_REVIEW.md` 16, `STAGE_B_FIX_REVIEW.md` 105, `STAGE_C_REVIEW.md` 15) | runnable now: its pause reason, level 4's slow suites, is over |
| levels 1, 4, 5, 6 | the roadmap lanes (paused for the split), the gate-speed leftovers, one rename carry-over, the dropped master ledger | parked |

## Proposed order

First pass. Part E and the review change it in five places: W1 is a correctness backlog and partly runs before
item 9 (Part E 22); W2 lands REG-4.15 with level 2 (H3.6); W3 includes Stage F1 and F2 (19), the six passes' per-file
facts (39) and STUB-7.5, the lazy Composer locator, which leaves W4 (H4.1); W5 builds C7 on F41's lowering (14);
W7 includes item 18's reconciled inference core and scopes (16), F69 with CHECK-9 and CHECK-7's checker half (D2) and
CHECK-10's operator sites (D3).

| step | what | rows, in order | notes |
|---|---|---|---|
| W1 | the correctness backlog and the follow-ups that need no engine | the Q-11 follow-ups LOAD-7.1, 7.3-7.7, REG-18.1, 18.5, CHECK-11 (REG-18.4 is batch 4b's: it needs `ConstName`); REG-2 (the gates' inventories); the data-model rows the ledger lists READY with no need: REG-4.5, 4.7, 4.13, 4.14, REG-5, REG-6 (REG-4.9 is REG-4.12's change and is built with it, W2 step 1; REG-4.10 is folded into DOC-2.9, batch 6); level 3's findings | placed in the sequence, not left "between heavy runs": every finding or follow-up that changes a dump goes before the baseline in batch 0, the rest between heavy runs, and a later behaviour-changing commit runs `dumps-compare`, lists its differences and rolls the baseline forward (Part E 22); none depends on the engine; REG-18.2, 18.3 and 18.6 are W2's needs; LOAD-7.6, REG-18.4 and CHECK-11 are deltas the ledger marks READY with no approval in VAL-2.9 or decision 14, approved by the user on 2026-10-18 with the conditions of Part E 42 (LOAD-7.6 is W1's except that its range per owning manifest is designed with W2), and LOAD-7.2's PHPUnit half is ruled: ungated; LOAD-7.5 and the Q-11 table cite `lsp/workspace_config.rs:66` and `:214`, a file LOAD-1.3 deleted: the LSP's lookup is `root.join("phpstan-rs.toml")` in `lsp.rs` (found by that symbol), to correct in the row when it is worked |
| W2 | C4's spine, the engine's roots first | REG-4.2 and REG-4.12, then REG-16.2, REG-16.3, REG-7.1 to 7.12 (7.6 needs REG-18.2 and 18.3; REG-4.15 lands with level 2: H3.6 step 5), REG-8.1 to 8.7 (8.4 needs REG-18.6), REG-9, REG-10, REG-4.11, REG-11, REG-12 | each commit runs `cargo check`, clippy and the touched modules' tests, and the full gate tier, `registry-dump` and O2 run once at W2's stage checkpoint (the crate split's rule: validation at the stage checkpoints you named, not per level or per commit); most rows are approved deltas (VAL-2.9); REG-4.2 and 4.12 meet NAME-1.2's registry entry points (batch 4), so they are written on the `Name`-keyed API |
| W3 | what the engine opens: the prizes | REG-16.4, then CACHE-13.1 (the stamp, for every file) and CACHE-13.2, STUB-7.5 (the lazy Composer locator, H4.1), LOAD-3.1 (and 3.3's `update_file` half), LOAD-5.1 (the six passes' per-file facts as `file_products` fields, then the use-site index), LOAD-5.2; LOAD-4.7's index half; TYPE-4.1 to 4.4, NAME-2.4; REG-16.5; REG-14.3; CHECK-8 (CHECK-7's checker half is D2's, W7); REG-17 | REG-16.4's listed needs (REG-16.3, SYN-1.2, LOAD-1.4) stop short of REG-7 and REG-8, but LOAD-4.6's text has REG-7 and REG-16.4 together replace `closure_scope` (REG-7 the passes that use it). H3.5 and H4 settle it: REG-8's rows are the inputs and queries it builds on, and REG-9 to REG-12 are W2's |
| W4 | C5, the stub layer | STUB-7.1, then 7.3 (it opens SYN-1.10; PROD-5.4 is gone with REG-16.4) and 7.6 (CACHE-9/10/11 follow, once CACHE-13.2 has fixed the persistence's format); 7.2, 7.4, 7.7 to 7.11 (7.5 is W3's); the gate 7.12; STUB-8; STUB-12.1 to 12.5; CHECK-10's version table (its operator sites are D3's, W7); STUB-13.1 | STUB-7 needs REG-12 as a parent row, and 7.1, 7.2, 7.4, 7.7, 7.8 and 7.11 list no need of their own; deltas are VAL-2.4, 2.9 and 2.10 |
| W5 | C6 to C9, then C10 | the rows are written with the plan's approval (Part E 20, J3); then the overlay layer, `TypeNodeResolver` (built on F41's lowering, batch 6, which has already deleted `types::parse`: Part E 14), the extension points, the summaries; REG-15.1 and 15.2 | the plan lists many approved deltas for each |
| W6 | the embedding | STUB-10.1, then STUB-11; STUB-13.2 (what remains of the stub loader once W3's Session has replaced `Project`; the row is reworded when W3 lands) | the cold-run prize |
| W7 | Stages D to G | rows to be written after a re-read (rulings 2 to 4) | the architecture program; no tracked row needs it |
| W8 | VAL-1, then level 1 | the end validation of what was built (VAL-1.13 against VAL-1.12's baseline, VAL-2.9 and 2.10 read as they landed); the roadmap lanes resume | |

## What bears on the order

1. **The prizes sit at the end of one chain.** Items 10 to 19 cut instructions inside the parse and
   the check. They leave the finishing passes' structure alone (NAME-1 shortens their hashing leaves,
   `is_folded_eq` and `fold_hash`), and none stops a warm run from parsing and checking a file that
   did not change, which CACHE-13.2 does. The chain is REG-4.2 and REG-16.2 (both READY), REG-16.3,
   REG-7 and REG-8, REG-16.4, CACHE-13.2.
2. **Stages D to G were written without the 20 G plan.** The crate split plan cites the 25 G report once
   (`CRATE_SPLIT_PLAN.md:336`, §10.1) and no ledger or stack, and it still says "five levels, each a per-class
   `OnceLock`" and "constant reference cycles type as `mixed`" where decisions 3 and 1 say otherwise. For C4 and C5 the ledger's
   rows are the authority. For C6 to G there are no rows, so the stale text is all there is. Where it
   meets what the ledger builds:

   | crate split | ledger | meeting |
   |---|---|---|
   | D1: `Locals` maps into holder storage, `ExprKey` interned, persistent maps | CHECK-6.3, 6.13, 6.14: slots, persistent radix arrays | both replace the scope's storage |
   | D3: one inference path, `ExprHandler`s, per-node before-scopes, `infer_expr` deleted | CHECK-6.1, 6.7: one inference per node | both make each node inferred once |
   | D4: a `NodeCallback` sink; the `RefCell` recording channels become observer events | CHECK-6.1, 6.15: observer families | both introduce observers, shaped differently |
   | D2: `syntax::annotations` (goto labels, last condition, try/catch types, closure-argument context) | item 9's tree | the annotations need a place in the stored tree; no such module exists |
   | E7: the checker's field sets | CHECK-6.8, 6.18 | the same per-function and per-file state; batch 7 builds only the scratch reuse (C2 note 7), and the one-checker-per-worker form is E7's file-state set |
   | F1: the Session | LOAD-1 (done), LOAD-8.8, REG-16.4 | F1 lists the LSP's own settings assembly and walk as outside the Session; LOAD-1.3 and 1.4 deleted both |
   | F2: result cache, resolved dependencies, fingerprints; `deps.rs` and `dependents` deleted | REG-16.4, CACHE-13.2 | decision 3 replaced the design |

   The plan orders Stage D "structure before semantics" because a transition built on
   `check_stmt`/`check_expr` "would be written twice" (Appendix F). The same reasoning applies to item
   18's inference core and its scopes group, which are structure, and to F69 with CHECK-9 and CHECK-7's checker half,
   which sit on the recorders D2 replaces; its temporaries group (CHECK-6.4 to
   6.12, 6.16, 6.17 and the scratch reuse of 6.18) is local and survives either way. (First-pass reading. Part E 16 and J3 compare §12.10 with
   D1 to D4 and find one architecture.)
3. **Stage C is the price of the prizes in three tiers.** W2 and W3 (C4's engine slice) give the
   wall-time and warm-run prizes and TYPE-4. W4 (C5) gives item 6. W5 and W6 (C6 to C10) are what
   STUB-11's cold-run prize costs: it needs REG-15, which needs C6 to C9. No tracked row needs Stages
   D to G; REG-7.8 and STUB-7.8 name F1 and E4 only as later clean-ups.
4. **DOC-2.1 and C7 delete the same grammar.** DOC-2.1's "text grammar" is `types::parse_type_string`
   (the crate split's §1 calls it the second grammar: 219 mentions in 30 files, besides the 58 sites
   of `parse_written_type`), and C7 deletes `types::parse` with it. (First pass: DOC-2.1 deferred, taken inside C7.
   Superseded by Part E 14 and I4: F41 builds the lowering in batch 6 and deletes `types::parse` there; C7
   then builds the name scope on it.)
5. **The 20 G plan's end validation will find items 4, 6, 7 and 17's second half unbuilt** if it runs
   after item 19 as written, and VAL-1's tooling (the canonical type printer, `registry-dump`, the
   expression-types dump) does not exist (Part E 13 builds it in batch 0). `registry-dump` taken before and after REG-7 would show what
   the change does to the registry itself, beyond what the crate split's gates (corpus, type
   snapshot, baseline, nsrt) see.

## Rulings (Part D; the questions, with Part E's answers)

1. **The order.** W2 and W3 (the engine slice and its prizes) before C5 to C10, with Stages D to G as
   a separate decision (recommended); or the crate split's own order, C4 to G, which puts CACHE-13.2
   behind Stage F, where its text is stale.
2. **Item 18's inference core and scopes** (CHECK-6.1, 6.2, 6.3, 6.7, 6.13 to 6.15): build them in
   batch 7 as planned and re-read D1, D3 and D4 against what they leave, or leave them to Stage D and
   build only the temporaries now. The first takes the largest check-phase saving (§12.10,
   ≈1.0-1.3 G) early and makes Stage D's text partly redundant; the second keeps the structure in one
   place and waits for the engine port. (Answered: Part E 16 takes the second.)
3. **Rows for C6 to C9, and for D to G.** A docs task that can run beside the engineering: re-read the
   phases against decisions 1 to 18, write ledger rows with needs, so that REG-15 and STUB-11 depend
   on tracked work. (Answered: Part E 20, at approval.)
4. **Whether Stages D to G are built as designed**, or re-planned after W6 on what the engine and the
   flat tree leave. No tracked row needs them. (Answered: Part E 21.)
5. **DOC-2.1 folds into C7** and stays deferred until then. (Answered otherwise: Part E 14.)
6. **VAL-1's tooling before W2** (it is a precondition for no row): the printers and `registry-dump`
   would make REG-7's change to the registry visible. Say if it goes first. (Answered: Part E 13, batch 0.)
7. **The fillers (W1)**: used only between heavy runs, or left for after the prizes. Their value is
   not speed: level 3 is review findings, and the Q-11 follow-ups finish C1 to C3. (Answered otherwise: Part E 22,
   a correctness backlog placed in the sequence.)
8. **Design next**: the spine's roots (REG-4.2, REG-4.12, REG-16.2, REG-16.3) in a session of their
   own while batches 5 to 7 are built, as Part B was designed; and REG-16.4 once item 9's step 1
   has fixed the tree API it reads. (Done in Part H; Part E 18 asks for H1 before batch 4.)

---
