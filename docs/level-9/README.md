# The batch after the cross-platform fixes: the plan (index)

This directory holds the plan, split into one file per part, and one file per section where a part was long. Section
numbers are kept, so a reference such as "Part E 40" or "I4.5" reads the same in every file. Where the plan names its
own documents (for example "call it `FLAT_TREE_PLAN.md`"), those are the plan's proposals, not the files here.

Part E gives the answer for each open question, and the sequence of the batches is below.

## The sequence

The order the batches and parts follow. Numbers in parentheses (#n) are the answers in Part E's table.


0. The cross-platform fixes, finished at `a83b1675`; level 8 then resumes
   with this plan.
1. Part A's rows: A1 (its mechanism for the skip, its `config.vendor-dir`, `COMPOSER_VENDOR_DIR` and `COMPOSER` commit and
   its COMP-7 commits), A2, A3, A5 (SYN-1.1), A5a (LOAD-5.4), A7 (the autoload maps keep
   their keys, the root's own entries included, and gain PSR-0) and A8 (the vendor context of every other Composer root: a
   package's bundled `vendor/` and a degrade root's nested projects, with A1's delta commit), then
   A6's validation against the batch's base binaries.
2. Batch 0, in this order: VAL-1.1; the seven fixes the crate split's F1 puts first, each with its test (#22); F10's
   tests and fixture, F77 (analysed files; its context half is a run-level notice that leaves the exit status), F20, F21 and F69's fixtures; every level-3 finding or Q-11 follow-up that changes a dump (W1), with
   the Part E 42 deltas apart (with their conditions, #42), and COMP-2's grammar (#40: it changes a
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

## Where each part is

| part | file |
|---|---|
| Overview | [`01-overview.md`](01-overview.md) |
| Part A (intro and table) | [`02-part-a-intro.md`](02-part-a-intro.md) |
| Part A, A1 | [`03-part-a1.md`](03-part-a1.md) |
| Part A, A2 to A8 | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| Part C | [`05-part-c.md`](05-part-c.md) |
| Part D | [`06-part-d.md`](06-part-d.md) |
| Part E | [`07-part-e.md`](07-part-e.md) |
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
