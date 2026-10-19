# Part N. The documents and edits this plan implies

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `b9de6dcf1` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| LOAD-2.2 | DONE | Walk DONE in `85a4ecad`. PART: the name floor's replacement is built by Part A's A1 (`097c164b`, `40d889bb`, … | 02 03 04 10 11 |
| LOAD-3.1 | OPEN | OPEN | 02 04 05 06 10 19 21 |
| LOAD-3.5 | OPEN | OPEN | 05 10 13 19 |
| LOAD-4.7 | OPEN | OPEN, blocked. Its file-references part is one change with PROD-5.7 (OPEN), which needs PROD-5.8 and PROD-5.2, both … | 02 05 06 07 10 21 22 31 |
| LOAD-5.1 | OPEN | OPEN | 04 05 06 07 10 19 21 23 31 |
| LOAD-5.2 | OPEN | OPEN | 05 06 07 10 19 21 22 |
| LOAD-5.4 | DONE | DONE `a5ab84c4` (Part A's A5a): the passes read through `ast_of`, and `FileState::ast` is gone | 01 02 04 05 07 10 11 |
| LOAD-7.2 | OPEN | OPEN; the PHPUnit half needs a ruling | 06 07 08 31 |
| LOAD-7.5 | READY | READY | 06 10 11 |
| LOAD-7.6 | READY | READY; a delta in the config's keys | 04 06 07 08 10 31 |
| LOAD-7.7 | READY | READY | 06 10 |
| LOAD-7.8 | OPEN | OPEN | 03 04 08 31 |
| LOAD-9 | DONE | DONE `3dcb1192` (Part A's A2): discovery is spawned first, and `read_installed` reads each package's directory from the … | 01 02 04 10 21 |
| SYN-1.2 | READY | READY | 02 04 05 06 07 10 13 31 |
| SYN-1.3 | OPEN | OPEN | 07 10 11 13 |
| SYN-1.4 | OPEN | OPEN | 07 10 11 13 31 |
| SYN-1.5 | OPEN | OPEN | 07 10 13 |
| SYN-1.6 | SUPERSEDED | SUPERSEDED by SYN-1.2 (decision 4): a tree of vectors is `Send + Sync` with no `unsafe`, and the workspace denies … | 08 10 11 13 16 |
| SYN-1.7 | OPEN | OPEN | 07 10 13 |
| SYN-1.8 | OPEN | OPEN | 07 10 13 |
| SYN-1.9 | OPEN | OPEN | 05 07 08 10 11 13 15 16 21 22 |
| SYN-1.10 | OPEN | OPEN | 05 06 07 11 13 22 |
| SYN-2 | OPEN | OPEN; the target grew | 05 07 09 10 11 13 31 |
| SYN-3.2 | READY | READY, with SYN-3.1 | 05 07 08 09 10 |
| SYN-3.4 | OPEN | OPEN | 05 10 11 13 17 18 |
| SYN-4.1 | OPEN | OPEN | 05 07 09 10 11 |
| SYN-4.5 | READY | READY | 05 07 10 11 18 31 |
| SYN-4.8 | READY | READY | 05 07 09 10 11 17 |
| SYN-4.9 | READY | READY | 05 07 09 10 11 |
| SYN-6 | OPEN | OPEN | 05 07 08 10 11 15 16 |
| PROD-5.1 | OPEN | OPEN | 05 07 10 11 13 18 20 22 |
| PROD-5.4 | OPEN | OPEN | 05 06 07 10 21 22 31 |
| PROD-5.5 | READY | READY | 05 10 |
| PROD-5.7 | OPEN | OPEN; one change with LOAD-4.7 | 02 05 07 10 21 22 31 |
| PROD-5.8 | READY | READY | 02 05 07 10 11 19 21 |
| DOC-2.1 | READY | READY, deferred 2026-10-16 (see WORK_STACK level 7) | 05 06 07 08 09 10 11 |
| DOC-2.2 | OPEN | OPEN | 05 07 08 09 10 23 |
| DOC-2.5 | OPEN | OPEN. DOC-1 rewrote the tag parser to pull tokens; the borrowed token text is not built | 05 07 09 10 |
| DOC-2.7 | READY | READY | 07 09 10 |
| DOC-2.9 | OPEN | OPEN; overlaps DOC-2.6 | 05 06 07 10 11 13 17 19 |
| NAME-1.2 | READY | READY; the container-field keys are a delta, approved (VAL-2.7) | 05 06 07 10 11 18 |
| NAME-2.1 | READY | READY | 05 07 08 10 11 18 |
| NAME-2.2 | READY | READY | 05 10 18 |
| TYPE-5 | READY | READY | 05 07 09 10 11 31 |
| REG-4.9 | READY | READY | 06 10 11 |
| REG-4.10 | READY | READY; overlaps DOC-2.6 | 05 06 07 10 11 |
| REG-4.11 | OPEN | OPEN; the comments can be corrected now | 06 10 18 20 22 |
| REG-4.12 | READY | READY | 06 08 10 11 18 19 20 |
| REG-4.15 | OPEN | OPEN | 05 06 07 10 11 18 20 22 |
| REG-7 | OPEN | OPEN | 05 06 08 18 19 20 21 23 |
| REG-7.5 | OPEN | OPEN | 01 02 04 05 06 07 10 20 22 |
| REG-8.6 | OPEN | OPEN; overlaps NAME-1 and NAME-2 | 05 06 07 08 10 18 20 22 |
| REG-14.1 | OPEN | OPEN; a delta, approved (VAL-2.6) | 02 05 |
| REG-14.3 | OPEN | OPEN | 06 10 |
| REG-14.4 | DONE | DONE `8739437e`; A6 (2026-10-19), off callgrind over the Piwigo run, the sum of `from_decl`'s inclusive Ir over all … | 01 02 04 10 |
| REG-14.5 | FOLDED | FOLDED into REG-7.5 (Part E 41): REG-7.5's combination is written without the `Vec`, and the function it would replace … | 01 02 04 07 10 22 31 |
| REG-16.2 | READY | READY | 05 06 07 08 10 11 22 31 |
| REG-16.4 | OPEN | OPEN | 02 04 05 06 07 08 10 11 13 20 21 22 23 31 |
| REG-16.5 | OPEN | OPEN | 06 10 20 21 22 |
| REG-17 | OPEN | OPEN | 05 06 08 10 19 20 21 |
| REG-18.4 | READY | READY; a delta | 05 06 07 08 10 18 22 31 |
| REG-18.6 | READY | READY | 03 06 10 11 20 |
| STUB-7.1 | OPEN | OPEN; a delta, approved (decision 14) | 05 06 07 08 10 11 13 23 |
| STUB-7.5 | OPEN | OPEN; a delta, approved (decision 14) | 02 04 06 07 08 10 21 22 31 |
| STUB-7.6 | OPEN | OPEN | 06 08 10 23 |
| STUB-11 | OPEN | OPEN | 05 06 07 10 21 23 24 |
| STUB-12.2 | OPEN | OPEN | 06 08 10 23 |
| STUB-12.4 | OPEN | OPEN | 06 10 23 |
| CACHE-8 | READY | READY | 05 07 10 22 31 |
| CACHE-9 | BLOCKED | BLOCKED: its form follows the stub layer and, since decisions 3 and 7, the persistence's format | 05 06 10 21 22 |
| CACHE-10 | BLOCKED | BLOCKED: as CACHE-9 | 06 10 21 22 |
| CACHE-11 | OPEN | OPEN | 06 10 22 |
| CACHE-13.1 | OPEN | OPEN | 05 06 07 10 11 19 21 22 |
| CACHE-13.2 | OPEN | OPEN | 05 06 07 08 10 21 22 23 |
| CHECK-4.1 | OPEN | OPEN | 05 08 09 23 |
| CHECK-4.2 | READY | READY | 05 07 10 22 31 |
| CHECK-6.1 | READY | READY; approved in principle (VAL-2.8) | 05 06 07 23 |
| CHECK-6.2 | READY | READY | 06 07 23 |
| CHECK-6.3 | OPEN | OPEN | 05 06 07 13 14 23 |
| CHECK-6.7 | OPEN | OPEN | 05 06 07 23 |
| CHECK-6.8 | READY | READY | 05 06 10 11 23 |
| CHECK-6.10 | READY | READY | 05 06 07 08 09 23 |
| CHECK-6.13 | OPEN | OPEN | 05 06 07 23 |
| CHECK-6.14 | OPEN | OPEN | 05 06 07 23 |
| CHECK-6.15 | OPEN | OPEN | 05 06 07 23 |
| CHECK-6.18 | READY | READY | 05 06 10 14 16 23 |
| CHECK-7 | OPEN | OPEN; a delta, approved (VAL-2.5) | 05 06 07 08 10 21 |
| CHECK-9 | READY | READY | 05 06 07 10 11 23 24 31 |
| CHECK-10 | OPEN | OPEN | 05 06 07 08 10 23 |
| CHECK-11 | READY | READY; a delta | 06 07 08 10 31 |
| CHECK-12 | OPEN | OPEN | 07 15 31 |
| CHECK-13 | OPEN | OPEN | 31 |
| VAL-1.1 | OPEN | OPEN | 07 08 10 11 |
| VAL-1.2 | OPEN | OPEN | 08 10 11 |
| VAL-1.3 | OPEN | OPEN | 08 10 11 |
| VAL-1.4 | OPEN | OPEN | 05 08 09 10 11 |
| VAL-1.5 | OPEN | OPEN; `xtask front-end-dump` exists, and hashes only | 07 08 10 11 13 |
| VAL-1.6 | OPEN | OPEN | 05 08 10 11 |
| VAL-1.7 | OPEN | OPEN; `--dump-types` exists | 05 08 09 10 11 |
| VAL-1.8 | OPEN | OPEN; `cargo xtask corpus --full` exists | 07 08 10 11 |
| VAL-1.9 | OPEN | OPEN | 08 10 11 |
| VAL-1.10 | OPEN | OPEN | 08 10 11 |
| VAL-1.11 | OPEN | OPEN; MEAS-2 has the whole-stack profile at this code | 08 10 11 |
| VAL-1.12 | OPEN | OPEN | 06 08 10 11 |
| VAL-2.9 | DONE | DONE: approved (decision 14), STUB-7.2 and REG-8.5 corrected; each is read at VAL-1 as it lands | 06 07 20 |
| VAL-2.10 | OPEN | OPEN: read at VAL-1, as they land | 05 06 07 15 20 |
| Q-10 | OPEN | OPEN, partly probed (2026-10-16, `~/.cache/prs-ledger/q10/`). The recovery is in place: a context file with a syntax … | 05 07 08 10 11 13 15 16 19 |
| Q-11 | DONE | DONE (2026-10-15): of the 20 follow-ups, 1 is done for one version, 5 are partly done and 14 are not; their section … | 06 07 11 31 |
| COMP-1 | DONE | DONE `097c164b`, `40d889bb`, `df9640fe`, `00081301`, `2e1728bb` (the walk's rule and its tests are `df9640fe` and … | 07 31 |
| COMP-2 | OPEN | OPEN. Not part of Part A; before the baseline (batch 0), since it changes a range and so a dump | 07 19 23 31 |
| COMP-3 | OPEN | OPEN, before W2 (H2.8's isolation assumes it) | 07 31 |
| COMP-4 | DONE | DONE `2c6f1eec` (Part A's A7): the maps keep their keys, and PSR-0 is read | 07 31 |
| COMP-5 | PART | PART: A7's maps keep the root's entries (`2c6f1eec`); the flat lists stay filtered, and the class is found through the … | 02 04 07 31 |
| COMP-6 | DONE | DONE `088089ed`: each installed package whose install directory holds a vendor directory by the same test as the root's … | 02 04 07 31 |
| COMP-7 | DONE | DONE: the discovery admission `acf48fb8`, the walk's cut `16418a2c`, the tests `c14aa865`. The batch delta for a … | 01 02 03 04 07 10 31 |
| COMP-8 | DONE | DONE `5b76d855`: the nested roots load before the walk, so owners, roles and contexts are fixed before any task runs. … | 01 02 03 04 07 19 31 |
| SRV-1 | DONE | DONE `4460dee39` | 07 |
| SRV-7 | DONE | DONE `75cc49526` | 07 |

<!-- status:end -->

Nothing here is done in plan mode. This part consolidates what Parts A, B, E, H, J and M scatter ("the ledger would
otherwise keep false"), so that the docs commit series of Part E 20 and the first commits of each batch have one
list. A ledger row is edited in the commit that builds it, except where a row says "at approval"; an edit of a ruled
decision's text is made only after the ruling, and the user ruled on 2026-10-18 (M5): the edits below are the
ones the rulings imply, and the decisions whose text does not change are named.

## N1. Where the plan's parts go (proposed)

| document | parts | why |
|---|---|---|
| `POST_LEVEL_9_PLAN.md` | A, C, D, E, F, G5, I2 to I5, K, L | the batches, their order, the answers and the designs the batches wait for |
| `FLAT_TREE_PLAN.md` | B, G1 to G4, I1 | item 9 and what it forces on the checker and the probe (B11) |
| `ENGINE_PLAN.md` | H, J | the engine, the pipeline, and the wall's data models and rows |
| `PLAN_REVIEW_LEDGER.md` | M | the workflow review's findings |

Each keeps its section numbers, so that a reference such as G1.5 or H4.6 stays valid, and opens with a table of where
the other parts went. The ledger keeps the rows and their states; the plans keep the designs.

## N2. The ledger (`PROFILE_DRILL_PIWIGO_LEDGER.md`)

| row | edit | from |
|---|---|---|
| SYN-1.2, 1.4, 1.5, 1.7, 1.8 | one shared design note pointing at `FLAT_TREE_PLAN.md`; need SYN-1.3; no ruled wording changes (ruled 2026-10-18): SYN-1.2's "owns its source" and SYN-1.4's "payloads are spans" are what B3 builds, and SYN-1.2's "settled first, by a measurement" is spike S-12 | B11, Part E 3 and 4 |
| SYN-1.3 | text "readers read through the handle API (step 1)"; need SYN-1.12, not SYN-1.2 | B2 D8, B11 |
| new SYN-1.11, SYN-1.12 | step 0 (the census; the fabricated nodes removed) and step 1's API (handles, views, the dump); SYN-1.12 is needed by SYN-1.3 | B7 |
| SYN-1.9, SYN-6, Q-10 | the error-node model of G3 (three arms, no member enum; the `incomplete` bit; marks); SYN-6's rows are marks and error kinds; a stray control byte, a NUL byte included, is a validity mark; a context file that fails to parse is a run-level notice that leaves the exit status | G3 |
| SYN-1.10 | keeps anonymous classes out of the declaration names and the global index; their identity is the ordinal | H1.4 |
| SYN-2 | needs SYN-1.2 (the file's names live in its tree); the stream of I2 (kind and span, no payload array, ends at `__halt_compiler`); the cast rule and its two deltas | I2 |
| SYN-3.2, 3.4 | the PHP line-terminator rule in one change (lexer sites, splitters, line index); the accept rule and the `wide` merit comparison; SYN-3.4 keeps `first_start` through the attribute re-key | I3, Part E 10 |
| SYN-4.1, 4.8, 4.9 | batch 1; SYN-4.1's lookup half goes and the watch table is a `const fn` in the caller; SYN-4.5 is `ConstName`, batch 4 | C1, C2 |
| NAME-1.2, 2.1, 2.2 | NAME-1.2 over values and maps, leading with the registry's entry points; NAME-2.1 is `ThinArc` only if S-1 finds a safe read path, else one `Arc` with one `Box<str>`, and the sweep lands with it; NAME-2.2's weak pairs are for the type tables | H1.7, Part E 29 |
| PROD-5.1, 5.4, 5.5, 5.7, 5.8 | PROD-5.1: file-local anonymous classes and the site table; PROD-5.4 and 5.7 "deleted by REG-16.4" (5.7 not built); PROD-5.5 keyed by ordinal; PROD-5.8 keyed by `FoldedName` and `ConstName` | Part E 34, 35, H1.4 |
| DOC-2.1, 2.2, 2.5, 2.7, 2.9; REG-4.10 | DOC-2.1 undeferred as F41, first in batch 6; DOC-2.2 keyed by the interned `Type` and the context id; DOC-2.9 absorbs REG-4.10 | Part E 14, I4 |
| TYPE-5 | its own commit after I4.5 step 5; F14's design text superseded | I4.5 |
| CHECK-6.1 to 6.3, 6.7, 6.13 to 6.15 | "built by D1 to D4" | J3 |
| CHECK-6.8, 6.18 | batch 7 builds the handle-free scratch reuse; "one checker per worker, reset for each file" is E7's | C2 note 7, G1.10 |
| CHECK-6.10, 4.1 | the `Message` of I5 | I5 |
| CHECK-4.2 | folded into REG-18.4 (batch 4b) | Part E 41 |
| CHECK-7, 9, 10 | CHECK-9 and CHECK-7's checker half in D2 (after F69), CHECK-10's operator sites in D3, its version table in W4 | Part E 17 |
| CHECK-11, LOAD-7.6, REG-18.4, LOAD-7.2 | their deltas are approved (2026-10-18) with the conditions of Part E 42: LOAD-7.6's key table is shown before the rename and the PHP range and the activation come from each file's owning manifest, CHECK-11 defines its template renderers, LOAD-7.2's PHPUnit half stays ungated | Part E 42 |
| REG-14.4 | "passes the parameters on" becomes the predicate | A3 |
| REG-14.5 | folded into REG-7.5 | A4 |
| REG-4.9 | built with REG-4.12 | Part D W1 |
| REG-4.11, 4.15 | REG-4.15 loses "a member's span is a field of its entry"; REG-4.11 becomes a record over (`Origin`, site) | H1.5a |
| REG-7.x, 8.x | the levels and queries of H3; there is no REG-8.8 | H3.5 |
| REG-8.6, 16.2 | REG-8.6 re-specified (one interner, counted and swept); REG-16.2 keeps "never reused" for `DeclKey`, which is immortal, and the name probes are collectable (H1.4, H1.6) | H1.3, Part E 38 |
| REG-16.4 | needs SYN-1.9 | H4.3 |
| REG-16.5 | the event callback plus the provider's trace of first readers | H3.5 |
| REG-17.x | the layering of H3.8 (the receiver-types query, the inference family) | H3.8 |
| LOAD-2.2 | stays PART until LOAD-7.8 deletes `path_under_excluded_dir` and the constant; its text, "skip Composer's vendor directory by its resolved path" (the fixes plan's decision 7), becomes "a directory Composer wrote into, or the install directory of a package it put elsewhere" (ruled 2026-10-18, M5d; COMP-7) | A6, A1 |
| REG-18.6 | `phpstan_class_files` is a provenance query, and "a PHPStan class is installed" asks the file's owning package and not a path component called `vendor` (`phpstan_api.rs:268`) | A1 |
| LOAD-5.1, 5.2, 5.4 | LOAD-5.1 moves to W3 (facts) and E1 (collection); LOAD-5.2 is W3's, needs both LOAD-5.1 and LOAD-3.1; LOAD-5.4 is Part A's "every reader asks `ast_of`" | Part E 39, A5a |
| LOAD-4.7 | the file-references half "deleted by REG-16.4" | H4.8 |
| LOAD-7.5, 7.x table | the LSP's lookup is `root.join("phpstan-rs.toml")` in `lsp.rs`, not `lsp/workspace_config.rs` | Part D W1 |
| LOAD-9 | DONE when A2 lands; leaves `WORK_STACK.md`'s "Outside the items" line | A6 |
| CACHE-13.1 | `File.stamp` at W3, with the racily-clean rule applied at the stat and an explicit file-system list; decision 13's "the index's own write time" is replaced | Part E 15 |
| CACHE-13.2 | H4.6; keyed by build, root and config key, the config key without the version range | Part E 37 |
| CACHE-8; CACHE-9 to 11 | after S-4; the form follows S-4 | Part E 41 |
| STUB-7.5 | moves to W3; needs the Composer maps to keep their keys, the root's own entries included, and gain PSR-0 (A7) | H4.1 |
| STUB-7.1, decision 18 | a range keeps exact version-id bounds and is open above when its constraint has no upper bound or none is declared (ruled 2026-10-18): the default is "7.0 to the newest, and later", a gate past `hi` is never, and one inside an open end is in part (`Availability`) | J1.1, Part E 40, M5d |
| STUB-12.2, 12.4 | behind the pre-8.0 spike; STUB-12.4 includes the `SignatureMapParser` port | J1.4, J2 |
| STUB-11 and OVL-2 | OVL-2 needs STUB-7.6, not STUB-11's build | J2 |
| VAL-1.2 to 1.12 | G5's order (the oracle run after the session and the replay); VAL-1.5 is item 9's dump; VAL-1.9's session API is public in `lsp` | Part E 13, G5 |
| VAL-2.9, 2.10 | the deltas the plan adds, approved on 2026-10-18 (the cast rule, `variable.undefined` after an error, `extract` or `include`, `extract` no longer truncating the body's diagnostics, the lone-CR rule, the interpolation and heredoc-indentation errors, the NUL mark, the anonymous-class consumers of H1.4, `Point<1, 2>`, `callable(int)`, the wildcard and `-9223372036854775808` and the other differences of I4.2 and I4.3, the string lexeme, CHECK-11, REG-18.4 and LOAD-7.6's keys and ranges) | I2 to I4, G3.6 |
| new rows | `OVL-1` to `OVL-8`, `RES-1` to `RES-7`, `EXT-1` to `EXT-4`, `SUM-1` to `SUM-3` (J2); a D-row and an E-row per phase of Stage D and E (J3); `COMP-1` to `COMP-8`, `SRV-1` to `SRV-7`, `CHECK-12` and `CHECK-13` (N6) | J2, J3, Part E 40, 22, G3.6, H1.4 |

Ruled decisions whose text changes (ruled on 2026-10-18, M5a): decision 3 (the member sentence only: a method by
folded name, a property and a class constant by exact name, an enum case being a class constant), decision 9 (one
sentence: a context file that fails to parse is reported as a run-level notice that leaves the exit status),
decision 13 (the racily-clean reference time is the clock reading at the file's own stat, not "the index's own write
time") and decision 17 (the widening sentence becomes "the iterate is the least fixpoint of the capped operator",
H3.3). **No text changes** in decision 4 (both sentences stand: the layout is what B3 builds, and the last sentence
is spike S-12), in decision 3's "is never reused" (it stands for `DeclKey`, which is immortal), in decision 14 (a
failed S-11 returns to the user before an eager fallback ships) or in decision 15 ("a context file is not reported"
gains decision 9's sentence by reference). **Two more texts change, ruled on 2026-10-18 (M5d, "agree"):** the fixes plan's
decision 7 ("by its resolved path" becomes "a directory Composer wrote into, or the install directory of a package it put
elsewhere") and the ledger's decision 18 ("to the newest version" becomes "and later").

## N3. `WORK_STACK.md`

Level 8 is reordered: item 0 (batch 0: VAL-1.1; the seven F1 fixes; F10's tests, F77, F20, F21, F69's fixtures; the
dump-changing level-3 findings and Q-11 follow-ups; the tooling and the baseline) before item 9; item 9 reads
"SYN-1.11, then 1.12 (+1.3), then 1.2/1.4/1.5/1.7/1.8, then step 3 (LOAD-3.5); `FLAT_TREE_PLAN.md`"; then the
batches of C1 in order (tokens, the lexer's second pass, parse recovery with the `php-version` crate, names, the
constant lookup, products, PHPDoc, the check's temporaries); Part A's rows come first, before item 0. Item 2's bullet
goes, LOAD-9 leaves "Outside the items", item 8 keeps REG-14.1 and 14.3, item 19 loses LOAD-5.4, the state line is
rewritten. The "Documents with no place in the stack" list gains this plan's four documents.

## N4. `CRATE_SPLIT_PLAN.md` (the docs commit series of Part E 20)

Marked in place, each pointing at the section that replaces it: Appendix F's "each level is a per-class `OnceLock`" and
"constant reference cycles type as `mixed`" (H3.1, decision 1); C4's "a change to any reflection input resets every
memo" (the recorded reads); F1's published snapshots (H4.3's cancellation); F1's text on line starts (the PHP terminator
rule is applied in the lexer too: I3); F2's resolved dependencies, fingerprints and entry sets (H4.4); §3.9's text
about the deleted `lsp/workspace_config.rs`; the line (6883-6891) "a file with parse errors is still not analysed",
superseded (ruled 2026-10-18, Part E 31); G's "26 new packages" count and Appendix A, which gain the engine and `php-version`
(J3); C6 to C9 and D to G gain the rows of J2 and J3. Appendix F's rule ("a bug in code a later phase rewrites gets a
fixture now and its fix as that phase's behaviour commit") is cited by F69, F5, F6, F45 and F61 (K2). B8's composer
follow-ups (`config.vendor-dir`, Composer's grammar, `psr-0`, the root package's autoload, a package's bundled `vendor/`)
point at A1, A7, A8 and Part E 40 once those are built; the clamp of an open upper bound to `MAX_PHP_VERSION` is marked
superseded (ruled 2026-10-18, M5d's third row).

## N5. Other documents

`CROSS_PLATFORM_PLAN.md`: R11's database-file follow-up and R16's `path_bytes` encoding pair (H4.6), R13's CRLF
probe files marked `-text` (G3.7.1). `CRASH_REPORTING_PLAN.md`: the stack guard's `LIMIT` restored on unwind and the
engine's two isolation modes (H2.8); the vendor-discovery panic-hook swap is deleted, not converted (Part E 40).
`UNSAFE_PLAN.md`: its end state changes only if S-2 finds the engine exception necessary (Part E 33).
`STAGE_B_FIX_REVIEW.md`: F14 superseded by TYPE-5's rule; F33's epoch lands in G1.5; F58's lazy construction in G1.9.

## N6. The new rows

`COMP-1` `config.vendor-dir` and `COMPOSER_VENDOR_DIR`, resolved as Composer does over candidates, and the recognition of
a Composer vendor directory by what Composer wrote in it (A1; the delta commit and the mechanism were ruled on 2026-10-18,
M5d); `COMP-2` Composer's constraint grammar by interval arithmetic (whitespace after an operator, a window inside a
minor, a bare version as Composer reads it; batch 0), and, apart, exact version-id bounds with an open upper end (J1.1;
ruled 2026-10-18, M5d); `COMP-3` the process-wide panic hook that `discover_vendor_context` swaps (the crash plan deletes it);
`COMP-4` `psr-0` in `Autoload` and the generated maps' keys (A7); `COMP-5` the root package's own autoload as context
(A7's maps keep its entries and W3's locator finds the class); `COMP-6` a package's bundled `vendor/` as context (A8);
`COMP-7` third-party code that Composer installed outside the vendor directory, left out of a degrade root's analysed walk
and admitted as context by discovery in every mode (A1: the seeds and the filter, two commits); `COMP-8` the vendor
contexts of a degrade root's nested Composer projects, found by a scan before the walk, with their owners and `unbuilt`
(A8). COMP-7 and COMP-8 were added on 2026-10-18 at the user's word ("also (a) and (b)"). All eight were audited against
the code and the corpora on 2026-10-18 (Part E 40), so each row is written as specified and none as OPEN to be audited. `SRV-1` to `SRV-7` are the seven fixes the crate split's F1 puts first: the debounce thread's stack size
and the scan's parses on the pool; `queue_change` writing `latest_text`; `didClose` restoring the disk's content;
`--generate-baseline` keeping the entries of files not analysed; `--fix` skipping a file changed since analysis; a
closed output pipe ending the run quietly; rename refusing qualified names, imports and aliases. Each has its test on
today's code (Part E 22). `CHECK-12` is exact include modelling (the included file's top-level assignments in the
includer's scope, and its return value as the include's type), built in Stage D on the scope model of D1 and D2, with
`any_variable_may_exist` as its fallback (G3.6); `CHECK-13` makes the class-shape parent-call rules include anonymous
classes (H1.4). Both are OPEN rows, each to be read against the code when it is worked.

## N7. The comparisons against an old implementation (Part E 13's policy)

Each is deleted in the commit after its stage: I2's old-versus-new lexer `(kind, span)` fuzz comparison (when the old
lexer is deleted); I4.5 step 3's per-`TypeNode` comparison, only if VAL-1.7's PHPDoc record cannot see what it finds;
H3.6 step 2's `Provider`-against-engine comparison and step 3's old passes, one level per stage. A2 keeps none.

---
