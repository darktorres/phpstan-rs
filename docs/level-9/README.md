# The batch after the cross-platform fixes: the plan (index)

This directory holds the plan, split into one file per part, and one file per section where a part was long. Section
numbers are kept, so a reference such as "#40" or "I4.5" reads the same in every file. Where the plan names its
own documents (for example "call it `FLAT_TREE_PLAN.md`"), those are the plan's proposals, not the files here.

Each open question is answered in the part that owns it, under its number (`#n`); the index is below the sequence.

## The sequence

The order the batches and parts follow. Numbers in parentheses (#n) are the decisions indexed below.


0. The cross-platform fixes, finished at `a83b1675`; level 8 then resumes
   with this plan.
1. Part A's rows: A1 (its mechanism for the skip, its `config.vendor-dir`, `COMPOSER_VENDOR_DIR` and `COMPOSER` commit and
   its COMP-7 commits), A2, A3, A5 (SYN-1.1), A5a (LOAD-5.4), A7 (the autoload maps keep
   their keys, the root's own entries included, and gain PSR-0) and A8 (the vendor context of every other Composer root: a
   package's bundled `vendor/` and a degrade root's nested projects, with A1's delta commit), then
   A6's validation against the batch's base binaries.
2. Batch 0, in this order: VAL-1.1; the seven fixes the crate split's F1 puts first, each with its test (#22); F10's
   tests and fixture, F77 (analysed files; its context half is a run-level notice that leaves the exit status), F20, F21 and F69's fixtures; every level-3 finding or Q-11 follow-up that changes a dump (W1), with
   the #42 deltas apart (with their conditions, #42), and COMP-2's grammar (#40: it changes a
   range, so a dump; its open-above half is part of the same answer);
   then item 0's tooling in G5's order and the baseline (#13, #22). F10's symptom and F57 are built.
3. Item 9: the error-node model designed (#9), then steps 0 to 3, with spike S-12 between step 0's census and step 2.
4. Batch 1 tokens (I2) with SYN-4.1, 4.8 and 4.9; 2 the lexer's second pass (I3 for SYN-3.2); 3 parse recovery
   (SYN-1.9, Q-10's parser half, SYN-6) with the `php-version` crate (#32); 4 names (S-1 first; item 16, then 15: H1.7)
   with SYN-4.5; 4b the constant lookup (REG-18.4 with CHECK-4.2, #41; a delta, #42); 5 products (item 13,
   without PROD-5.7: #35; PROD-5.1 with file-local anonymous classes and the site table: #34); 6 PHPDoc (F41 first,
   I4, then DOC-2.2 to 2.9 with REG-4.10 in 2.9, then TYPE-5); 7 the check's temporaries (the `Message` of
   I5, CHECK-6.4 to 6.6, 6.9 to 6.12, 6.16, 6.17 and the scratch reuse of 6.8 and 6.18; F69 and CHECK-9 are D2's: #17);
   8 is empty (LOAD-5.4 is Part A's; CACHE-13.1 and the LOAD-5.1 facts are W3's, #15 and #39; CACHE-8 after S-4, #41).
5. The spikes at their gate points (H5): S-12 before item 9's step 2 (it settles whether B3's nodes hold byte spans
   or token indices; step 0's census feeds it); S-1, S-2 and S-3 before batch 4, with H1 (the engine's identity
   model); S-4 right after S-3 and before W2's result types; S-6 before W2 step 3; S-8 with batch 4; S-10 before W3
   step 2; S-11 before W3 step 4.
6. The wall: Part D's W2 to W8, with W2 as H3.6, W3 as H4.7 (REG-16.4 also needs SYN-1.9), W3 including Stage F1
   and F2 (the seven fixed-first commits of F1 are batch 0's) and the six passes' facts, W4 as J1, W5 as J2 with C7 on
   I4's lowering, and W7 as J3 (item 18's reconciled core, F69 with CHECK-9 and CHECK-7's checker half in D2, exact
   include modelling, CHECK-12, on the scope model of D1 and D2, CHECK-10's operator sites in D3).

## Decisions

Each decision is a `### #n` section of the part that owns the design it changes. Its answer is there, with the facts it rests on and what must be settled before it is built (`*Needed before:*`).

| # | decision | part |
|---|---|---|
| 1 | D1: what consumers hold | [`13-part-b.md`](13-part-b.md) |
| 2 | D2: how to migrate | [`13-part-b.md`](13-part-b.md) |
| 3 | D3: position | [`13-part-b.md`](13-part-b.md) |
| 4 | D4: payload bytes | [`13-part-b.md`](13-part-b.md) |
| 5 | D5: names in nodes | [`13-part-b.md`](13-part-b.md) |
| 6 | D6: the mutating passes | [`13-part-b.md`](13-part-b.md) |
| 7 | D7: fabricated nodes | [`13-part-b.md`](13-part-b.md) |
| 8 | D8: rows | [`13-part-b.md`](13-part-b.md) |
| 9 | D9: error nodes | [`13-part-b.md`](13-part-b.md) |
| 10 | SYN-3.2's SIMD | [`05-part-c.md`](05-part-c.md) |
| 11 | the parse-recovery batch | [`05-part-c.md`](05-part-c.md) |
| 12 | order of items 13, 16, 15, 14 | [`05-part-c.md`](05-part-c.md) |
| 13 | oracle tools | [`08-part-f.md`](08-part-f.md) |
| 14 | DOC-2.1 | [`05-part-c.md`](05-part-c.md) |
| 15 | CACHE-13.1 for context files | [`06-part-d.md`](06-part-d.md) |
| 16 | item 18 | [`23-part-j.md`](23-part-j.md) |
| 17 | CHECK-9 and TYPE-5 | [`05-part-c.md`](05-part-c.md) |
| 18 | when to reevaluate | [`18-part-h1.md`](18-part-h1.md) |
| 19 | order of the wall | [`06-part-d.md`](06-part-d.md) |
| 20 | rows for C6 to C9 and D to G | [`12-part-n.md`](12-part-n.md) |
| 21 | Stages D to G | [`06-part-d.md`](06-part-d.md) |
| 22 | level 3 and the Q-11 follow-ups | [`10-part-k.md`](10-part-k.md) |
| 23 | Part A's validation | [`02-part-a-intro.md`](02-part-a-intro.md) |
| 24 | item 9's validation | [`13-part-b.md`](13-part-b.md) |
| 25 | LOAD-2.8's parts 1 and 2 | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| 26 | the LSP completion probe | [`16-part-g4-g6.md`](16-part-g4-g6.md) |
| 27 | the step 1 codemod | [`13-part-b.md`](13-part-b.md) |
| 28 | LOAD-5.2 batch-only | [`06-part-d.md`](06-part-d.md) |
| 29 | NAME-2.1's thin name | [`05-part-c.md`](05-part-c.md) |
| 30 | where Part B lives | [`12-part-n.md`](12-part-n.md) |
| 31 | an analysed file with parse errors | [`15-part-g3.md`](15-part-g3.md) |
| 32 | `PhpVersion` and its types move out of `types` | [`05-part-c.md`](05-part-c.md) |
| 33 | the engine crate and `unsafe_code` | [`20-part-h3.md`](20-part-h3.md) |
| 34 | positionless declarations | [`05-part-c.md`](05-part-c.md) |
| 35 | PROD-5.7 and `deps::Collector` | [`05-part-c.md`](05-part-c.md) |
| 36 | the server's snapshot model | [`19-part-h2.md`](19-part-h2.md) |
| 37 | the persisted database's key | [`21-part-h4.md`](21-part-h4.md) |
| 38 | REG-16.2's "never reused" | [`18-part-h1.md`](18-part-h1.md) |
| 39 | LOAD-5.1's passes | [`06-part-d.md`](06-part-d.md) |
| 40 | the composer crate's B8 follow-ups | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| 41 | rows whose code a later task replaces | [`20-part-h3.md`](20-part-h3.md) |
| 42 | deltas the ledger marks READY or OPEN with half a ruling | [`06-part-d.md`](06-part-d.md) |

## Where each part is

| part | file |
|---|---|
| Overview | [`01-overview.md`](01-overview.md) |
| Part A (intro and table) | [`02-part-a-intro.md`](02-part-a-intro.md) |
| Part A, A1 | [`03-part-a1.md`](03-part-a1.md) |
| Part A, A2 to A8 | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| Part C | [`05-part-c.md`](05-part-c.md) |
| Part D | [`06-part-d.md`](06-part-d.md) |
| Part F (and G5) | [`08-part-f.md`](08-part-f.md) |
| Part I (I2 to I5) | [`09-part-i.md`](09-part-i.md) |
| Part K | [`10-part-k.md`](10-part-k.md) |
| Part L | [`11-part-l.md`](11-part-l.md) |
| Part N | [`12-part-n.md`](12-part-n.md) |
| Part B | [`13-part-b.md`](13-part-b.md) |
| Part G (G1, G2) | [`14-part-g1-g2.md`](14-part-g1-g2.md) |
| Part G, G3 | [`15-part-g3.md`](15-part-g3.md) |
| Part G (G4, G6) | [`16-part-g4-g6.md`](16-part-g4-g6.md) |
| Part I1 | [`17-part-i1.md`](17-part-i1.md) |
| Part H (H1) | [`18-part-h1.md`](18-part-h1.md) |
| Part H, H2 | [`19-part-h2.md`](19-part-h2.md) |
| Part H, H3 | [`20-part-h3.md`](20-part-h3.md) |
| Part H, H4 | [`21-part-h4.md`](21-part-h4.md) |
| Part H (H5, H6) | [`22-part-h5-h6.md`](22-part-h5-h6.md) |
| Part J | [`23-part-j.md`](23-part-j.md) |

The ledger, `PROFILE_DRILL_PIWIGO_LEDGER.md`, is the only record of each task's state. The parts name the tasks they depend on.

## Batch 0's rows

Batch 0 is the second step of the sequence. No part owns its rows: each row is listed here with its state, as of the ledger's `a3931d49d`. The level-3 findings that change a dump are W1's (Part D), and VAL-1.13 to VAL-1.15 are the wall's last step (Part D, W8).

| task | state | what it is |
|---|---|---|
| VAL-1.1 | OPEN | the starting-state read |
| SRV-1 | DONE `4460dee39` | the debounce thread gets the pool's stack size |
| SRV-2 | DONE `cfb7b9cb5` | `queue_change` writes `latest_text` |
| SRV-3 | DONE `9f2fde136` | `didClose` restores the disk's content |
| SRV-4 | DONE `8d68e6a1a` | `--generate-baseline` keeps the entries of unanalysed files |
| SRV-5 | DONE `b8a332b38` | `--fix` writes only while the file's bytes are the analysed bytes |
| SRV-6 | DONE `e93890d8f` | a closed output pipe ends the run quietly |
| SRV-7 | DONE `75cc49526` | rename refuses names a qualified name, an alias or an import would leave stale |
| FIX-10 | DONE `2091070e` | a bare `yield` before a word or symbol operator (its symptom was fixed by `3b7e4d95`; the exhaustive predicate is batch 1's, on SYN-2) |
| FIX-20 | DONE `eb24e5b44`, `ee32ca801`, `a231be60c`, `a8bfadace`, `6c37fa51e`, `b4f06aac` | the recursion guard, with the 100,000-level tests |
| FIX-21 | DONE `98d46d9a`, `8454569a` | the error type is a kind of `mixed` |
| FIX-69 | OPEN | fixtures for a `catch` that its `try` reaches only through magic |
| FIX-77 | PART | a file that does not parse is a diagnostic: the analysed-file half and the context-file notice are built in `4278ec2a`; the dump and the per-file cache do not record a context file's failure yet, and the dump needs VAL-1.8 |
| COMP-2 | OPEN | Composer's constraint grammar (before the baseline, since it changes a range) |
| VAL-1.2 | OPEN | an `oracle` profile |
| VAL-1.3 | OPEN | the debug oracles and the code only they use are deleted |
| VAL-1.4 | OPEN | one canonical type printer in `types` |
| VAL-1.5 | OPEN | O1: `--dump-front-end` |
| VAL-1.6 | OPEN | `registry-dump` |
| VAL-1.7 | OPEN | the checker's records: PHPDoc resolutions and expression types |
| VAL-1.8 | OPEN | O2: the gate's full artifact set |
| VAL-1.9 | OPEN | the server: an in-process session |
| VAL-1.10 | OPEN | the edit replay |
| VAL-1.11 | OPEN | the dhat profile by holder |
| VAL-1.12 | OPEN | the baseline, from the commit that adds the printers and makes the deletions |

- Open parser findings: F10 (the bare `yield` shapes parse since `3b7e4d95`, through a list of tokens that cannot begin an expression; the review's exhaustive `starts_expression` and its tests are not there), F77 (the "file(s) skipped: parse error" line is
  still printed), F20 (no `xtask recursion`, no `stack` crate). F57's arrow body and its ternary
  error are already in the parser.
- The seven fixes the crate split's F1 puts first were all open at `ffa7cccb`, and each now has a commit: `4460dee39` (the debounce stack), `cfb7b9cb5` (`latest_text`), `9f2fde136` (`didClose`), `8d68e6a1a` (`--generate-baseline`), `b8a332b38` (`--fix`), `e93890d8f` (the closed pipe) and `75cc49526` (rename), which are SRV-1 to SRV-7 in that order. (the debounce thread has no stack size,
  `queue_change` does not write `latest_text`, `didClose` is a no-op, `--generate-baseline` writes the run's results
  alone, `--fix` patches at the earlier analysis's offsets, rename returns an edit for every span, and a closed
  output pipe is not handled).
- Batch 0, built by commits outside Part A (from the commit messages): F20 (FIX-20), the recursion guard as its own crate with the 100,000-level tests (`eb24e5b44`, `ee32ca801`, `a231be60c`, `a8bfadace`, `6c37fa51e`, `b4f06aac`); F21 (FIX-21), the error type as a kind of `mixed` and explicit `mixed` as its own kind (`98d46d9a`, `8454569a`); F10's tests (FIX-10) for a bare `yield` (`2091070e`); F77 (FIX-77) for a file that does not parse (`4278ec2a`). The context-file half of F77 is a stderr notice and the `--fail-on-context-error` switch in that commit. The dump and the per-file cache do not record a context file's failure yet; the dump needs VAL-1.8. Not built: VAL-1.1, the oracle tools, the item-0 baseline and F69's fixtures (FIX-69).
