# Part A. The batch after the cross-platform fixes

## Tasks this part owns

A task is listed where it is built; the state is the ledger's, as of `a3931d49d`.

| task | state | built by |
|---|---|---|
| LOAD-2.2 | PART | A1: the name floor's replacement, `097c164b`, `40d889bb`, `df9640fe`, `c14aa865`, `fc1c65eca`, `00081301`, `2e1728bb`; the name list's deletion waits for LOAD-7.8 (`exclude_paths`, OPEN) |
| COMP-1 | DONE | A1: `097c164b`, `40d889bb`, `df9640fe`, `00081301`, `2e1728bb` |
| COMP-7 | DONE | A1: `acf48fb8`, `16418a2c`, `c14aa865` |
| LOAD-9 | DONE | A2: `3dcb1192` |
| REG-14.4 | DONE | A3: `8739437e` |
| SYN-1.1 | DONE | A5: `2f9b1281` |
| LOAD-5.4 | DONE | A5a: `a5ab84c4` |
| COMP-4 | DONE | A7: `2c6f1eec` |
| COMP-5 | PART | A7: `2c6f1eec`; the class is found through the maps by W3's lazy locator (STUB-7.5, OPEN), so no interim fix is built |
| COMP-6 | DONE | A8: `088089ed` |
| COMP-8 | DONE | A8: `5b76d855` |

A6 is the validation, not a task.

| # | row | item | what | where |
|---|---|---|---|---|
| 1 | LOAD-2.2, last part | 2 | the name floor is replaced: a Composer vendor directory is recognised by what Composer wrote in it, and so is third-party code that Composer installed outside it, whose classes become context in every mode (COMP-7); `.git` and `node_modules` stay by name; the project's vendor directory is resolved as Composer does, for discovery, `read_installed` and the include lookup | `project/session.rs`, `project.rs`, `project/walk.rs`, `crates/composer` |
| 2 | LOAD-9 | none (R14's finding) | installed packages resolved once, beside vendor discovery | `crates/composer/src/lib.rs`, `manifest.rs` |
| 3 | REG-14.4 | 8 | `from_decl` passes the parameters on, not a `Vec<&str>` of names | `registry/function_sig.rs`, `function_sig/asserts.rs` |
| 4 | SYN-1.1 | 9, step 1 | `Expr::peeled` becomes a loop | `crates/syntax/src/ast.rs:1417` |
| 5 | LOAD-5.4 | 19 | no reader gets an empty file: `FileState::ast` goes, so item 9's API needs no empty tree (A5a) | `project.rs`, `project/{architecture,banned,class_shape,composer_hygiene,mock_hygiene}.rs` |
| 6 | the composer follow-up of Part E 40 (a COMP row) | none | the autoload maps keep their keys, the root package's own entries included (COMP-5's data), and gain PSR-0, which STUB-7.5's lazy locator needs (A7); no output moves | `crates/composer/src/manifest.rs`, `vendor.rs` |
| 7 | COMP-6 and COMP-8 | none | the vendor context of every other Composer root: a package's bundled `vendor/` (COMP-6) and, in a degrade root, a nested project's (COMP-8), found by A1's test (A8); a delta, with A1's | `crates/composer/src/lib.rs`, `vendor.rs`, `project/session.rs`, `project/walk.rs` |

Why these rows: item 2 has only this part open (LOAD-2.1 and 2.3 to 2.8 are in).
Items 3 and 5 have no READY row (LOAD-3.1 needs REG-16.4; 3.5 needs SYN-1.2; LOAD-4.7 needs
PROD-5.7, which needs PROD-5.8 and PROD-5.2 of item 13). Items 4, 6 and 7 hold only crate-split
rows (level 2, deferred). Item 8: REG-14.1 needs PROD-5.8 (item 13), 14.3 needs REG-8.3 (level 2),
14.2 is done, so 14.4 and 14.5 are all that is READY. Item 9: SYN-1.1 is READY and "comes
first, on its own". LOAD-9 is READY and sits in `WORK_STACK.md`'s "Outside the items" line, in
none of level 8's item lists. Items 11 to 16, 18 and 19 have READY rows but come after 9 in the plan's build order.

A2 needs A1's `vendor_location` (A1's composer commit comes first); A7 edits the same crate and follows A1 and A2; A8
needs A1's test, A2's package list and A7's maps; COMP-7's two commits follow A2, whose shared package list the
admission reads, and the admission comes first, so that no commit leaves a class unknown that the walk still analysed
(a role of analysed wins over the context one); A3, A5 and A5a are independent of them and of each other. Validation,
once, after the last of them (A6). Four answers of the audit are built in:
A1's mechanism for the skip, which changes the wording of the fixes plan's decision 7; A1's commit that makes discovery,
`read_installed` and the include lookup follow `config.vendor-dir`, `COMPOSER_VENDOR_DIR` and `COMPOSER` (Part E 40), a
delta; A8, a delta of the same kind; and the PHP range's open upper end (J1.1), which is not Part A's; Part E 35 leaves
three rows unbuilt. The two extensions added with them are A1's third clause and its discovery filter (COMP-7) and A8's
scan (COMP-8); each has a delta commit of its own. A6 runs with all of them.
