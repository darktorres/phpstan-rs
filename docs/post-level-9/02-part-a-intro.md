# Part A. The batch after level 9

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
