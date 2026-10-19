# Part A. The batch after the cross-platform fixes

<!-- status:begin -->

## Status of this part's tasks

From the ledger at `13ff6c1e1` on 2026-10-19. Regenerate with `scripts/plan_status.py`; change the ledger, not this table. `also in` lists the other parts that name the task.

| task | status | ledger | also in |
|---|---|---|---|
| LOAD-2.1 | DONE | DONE `3d2dc7c3`, `08877e58`, `5cca45c1`; the drop is a guard on each file's directories (`walked_by_a_directory`), not … | — |
| LOAD-2.2 | DONE | Walk DONE in `85a4ecad`. PART: the name floor's replacement is built by Part A's A1 (`097c164b`, `40d889bb`, … | 03 04 10 11 12 |
| LOAD-2.3 | DONE | DONE `07595369`. The configured spelling is not carried: the walk builds every path from its resolved parent, so no … | — |
| LOAD-2.4 | DONE | DONE `df804239`. The walk hands each directory's files to the sink as it reads the listing, when one walk alone reaches … | 03 04 21 22 |
| LOAD-2.5 | DONE | DONE `8590ff55`, with two deviations from the design above. Nested roots are not dropped, and the roots are not sorted: … | — |
| LOAD-2.6 | DONE | DONE `c1db8372`, `c7ff651c`: the index keeps resolved directories only, and `owner_of_dir` is one trie lookup of a … | — |
| LOAD-2.7 | DONE | DONE `2cd13c8e`: the map is a `hashbrown::HashTable<FileId>`, and each slot holds its path and hash. `with_paths` takes … | — |
| LOAD-2.8 | DONE | Parts 1 and 2 DONE: `27c3b093` (the manifests and `installed.json` read member by member; a repeated key keeps its last … | 04 07 |
| LOAD-3.1 | OPEN | OPEN | 04 05 06 10 12 19 21 |
| LOAD-4.7 | OPEN | OPEN, blocked. Its file-references part is one change with PROD-5.7 (OPEN), which needs PROD-5.8 and PROD-5.2, both … | 05 06 07 10 12 21 22 31 |
| LOAD-5.4 | DONE | DONE `a5ab84c4` (Part A's A5a): the passes read through `ast_of`, and `FileState::ast` is gone | 01 04 05 07 10 11 12 |
| LOAD-9 | DONE | DONE `3dcb1192` (Part A's A2): discovery is spawned first, and `read_installed` reads each package's directory from the … | 01 04 10 12 21 |
| SYN-1.1 | DONE | DONE `2f9b1281`: the loop, with a test at a depth the recursion needed a guard for; A6 (2026-10-19) found no standalone … | 04 07 10 |
| SYN-1.2 | READY | READY | 04 05 06 07 10 12 13 31 |
| PROD-5.2 | READY | READY | 05 09 10 11 23 |
| PROD-5.7 | OPEN | OPEN; one change with LOAD-4.7 | 05 07 10 12 21 22 31 |
| PROD-5.8 | READY | READY | 05 07 10 11 12 19 21 |
| REG-7.5 | OPEN | OPEN | 01 04 05 06 07 10 12 20 22 |
| REG-8.3 | OPEN | OPEN | 05 06 10 20 |
| REG-14.1 | OPEN | OPEN; a delta, approved (VAL-2.6) | 05 12 |
| REG-14.4 | DONE | DONE `8739437e`; A6 (2026-10-19), off callgrind over the Piwigo run, the sum of `from_decl`'s inclusive Ir over all … | 01 04 10 12 |
| REG-14.5 | FOLDED | FOLDED into REG-7.5 (Part E 41): REG-7.5's combination is written without the `Vec`, and the function it would replace … | 01 04 07 10 12 22 31 |
| REG-16.4 | OPEN | OPEN | 04 05 06 07 08 10 11 12 13 20 21 22 23 31 |
| STUB-7.5 | OPEN | OPEN; a delta, approved (decision 14) | 04 06 07 08 10 12 21 22 31 |
| COMP-5 | PART | PART: A7's maps keep the root's entries (`2c6f1eec`); the flat lists stay filtered, and the class is found through the … | 04 07 12 31 |
| COMP-6 | DONE | DONE `088089ed`: each installed package whose install directory holds a vendor directory by the same test as the root's … | 04 07 12 31 |
| COMP-7 | DONE | DONE: the discovery admission `acf48fb8`, the walk's cut `16418a2c`, the tests `c14aa865`. The batch delta for a … | 01 03 04 07 10 12 31 |
| COMP-8 | DONE | DONE `5b76d855`: the nested roots load before the walk, so owners, roles and contexts are fixed before any task runs. … | 01 03 04 07 12 19 31 |

<!-- status:end -->

| # | row | item | what | where |
|---|---|---|---|---|
| 1 | LOAD-2.2, last part | 2 | the name floor is replaced: a Composer vendor directory is recognised by what Composer wrote in it (ruled 2026-10-18, M5d), and so is third-party code that Composer installed outside it, whose classes become context in every mode (COMP-7); `.git` and `node_modules` stay by name; the project's vendor directory is resolved as Composer does, for discovery, `read_installed` and the include lookup | `project/session.rs`, `project.rs`, `project/walk.rs`, `crates/composer` |
| 2 | LOAD-9 | none (R14's finding) | installed packages resolved once, beside vendor discovery | `crates/composer/src/lib.rs`, `manifest.rs` |
| 3 | REG-14.4 | 8 | `from_decl` passes the parameters on, not a `Vec<&str>` of names | `registry/function_sig.rs`, `function_sig/asserts.rs` |
| 4 | REG-14.5 | 8 | not built on its own: `reconcile_with_native` is replaced by REG-7.5's combination, which is written without the `Vec` (A4, Part E 41) | `crates/types/src/decide.rs` |
| 5 | SYN-1.1 | 9, step 1 | `Expr::peeled` becomes a loop | `crates/syntax/src/ast.rs:1417` |
| 6 | LOAD-5.4 | 19 | no reader gets an empty file: `FileState::ast` goes, so item 9's API needs no empty tree (A5a) | `project.rs`, `project/{architecture,banned,class_shape,composer_hygiene,mock_hygiene}.rs` |
| 7 | the composer follow-up of Part E 40 (a COMP row) | none | the autoload maps keep their keys, the root package's own entries included (COMP-5's data), and gain PSR-0, which STUB-7.5's lazy locator needs (A7); no output moves | `crates/composer/src/manifest.rs`, `vendor.rs` |
| 8 | COMP-6 and COMP-8 | none | the vendor context of every other Composer root: a package's bundled `vendor/` (COMP-6) and, in a degrade root, a nested project's (COMP-8), found by A1's test (A8); a delta, with A1's | `crates/composer/src/lib.rs`, `vendor.rs`, `project/session.rs`, `project/walk.rs` |

Why these (the READY audit): item 2 has only this part open (LOAD-2.1 and 2.3 to 2.8 are in).
Items 3 and 5 have no READY row (LOAD-3.1 needs REG-16.4; 3.5 needs SYN-1.2; LOAD-4.7 needs
PROD-5.7, which needs PROD-5.8 and PROD-5.2 of item 13). Items 4, 6 and 7 hold only crate-split
rows (level 2, deferred). Item 8: REG-14.1 needs PROD-5.8 (item 13), 14.3 needs REG-8.3 (level 2),
14.2 is done, so 14.4 and 14.5 are all that is READY. Item 9: SYN-1.1 is READY and "comes
first, on its own". LOAD-9 is READY and sits in `WORK_STACK.md`'s "Outside the items" line (added by `791a57c6`), in
none of level 8's item lists. Items 11 to 16, 18 and 19 have READY rows but come after 9 in the plan's build order.

A2 needs A1's `vendor_location` (A1's composer commit comes first); A7 edits the same crate and follows A1 and A2; A8
needs A1's test, A2's package list and A7's maps; COMP-7's two commits follow A2, whose shared package list the
admission reads, and the admission comes first, so that no commit leaves a class unknown that the walk still analysed
(a role of analysed wins over the context one); A3, A5 and A5a are independent of them and of each other. Validation,
once, after the last of them (A6). Four answers of the audit were ruled on 2026-10-18 ("agree", M5d) and are built in:
A1's mechanism for the skip, which changes the wording of the fixes plan's decision 7; A1's commit that makes discovery,
`read_installed` and the include lookup follow `config.vendor-dir`, `COMPOSER_VENDOR_DIR` and `COMPOSER` (Part E 40), a
delta; A8, a delta of the same kind; and the PHP range's open upper end (J1.1), which is not Part A's; Part E 35 leaves
three rows unbuilt. The two extensions added with them are A1's third clause and its discovery filter (COMP-7) and A8's
scan (COMP-8); each has a delta commit of its own. A6 runs with all of them.
