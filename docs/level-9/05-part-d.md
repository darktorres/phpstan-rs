# Part D. After batch 8: the wall (levels 1 to 7)

## Tasks this part owns

Each task is listed under the wall step (W1 to W8, the table under "Proposed order") that builds it. The state is the ledger's, as of `a3931d49d`.

| step | task | state |
|---|---|---|
| W1 | LOAD-7.1 | READY |
| W1 | LOAD-7.2 | OPEN (its PHPUnit half is ungated) |
| W1 | LOAD-7.3 | READY |
| W1 | LOAD-7.4 | READY |
| W1 | LOAD-7.5 | READY |
| W1 | LOAD-7.6 | READY (a delta in the config's keys) |
| W1 | LOAD-7.7 | READY |
| W1 | REG-18.1 | READY |
| W1 | REG-18.5 | READY |
| W1 | CHECK-11 | READY (a delta) |
| W1 | REG-2 | READY |
| W1 | REG-4.5 | READY |
| W1 | REG-4.7 | READY |
| W1 | REG-4.13 | READY |
| W1 | REG-4.14 | READY (a delta, approved) |
| W1 | REG-5 | PART (the purity part is REG-1's; a delta is left) |
| W1 | REG-6 | READY (a test-expectation delta, approved) |
| W2 | REG-4.2 | READY (a delta, approved in the plan) |
| W2 | REG-4.12 | READY |
| W2 | REG-4.9 | READY (built with REG-4.12) |
| W2 | REG-4.11 | OPEN |
| W2 | REG-16.2 | READY |
| W2 | REG-16.3 | OPEN |
| W2 | REG-7.1 | OPEN |
| W2 | REG-7.2 | OPEN |
| W2 | REG-7.3 | OPEN |
| W2 | REG-7.4 | OPEN |
| W2 | REG-7.5 | OPEN |
| W2 | REG-7.6 | OPEN (a delta, approved) |
| W2 | REG-7.7 | OPEN |
| W2 | REG-7.8 | OPEN |
| W2 | REG-7.9 | OPEN |
| W2 | REG-7.10 | OPEN |
| W2 | REG-7.11 | OPEN |
| W2 | REG-7.12 | OPEN |
| W2 | REG-8.1 | OPEN (a delta) |
| W2 | REG-8.2 | SUPERSEDED by REG-16.3 |
| W2 | REG-8.3 | OPEN |
| W2 | REG-8.4 | OPEN |
| W2 | REG-8.5 | OPEN (a delta, approved) |
| W2 | REG-8.6 | OPEN (overlaps NAME-1 and NAME-2) |
| W2 | REG-8.7 | OPEN |
| W2 | REG-9 | OPEN (deltas, approved in the plan) |
| W2 | REG-10 | OPEN |
| W2 | REG-11 | OPEN (each a delta, approved in the plan) |
| W2 | REG-12 | OPEN |
| W3 | REG-16.4 | OPEN |
| W3 | CACHE-13.1 | OPEN |
| W3 | CACHE-13.2 | OPEN |
| W3 | STUB-7.5 | OPEN (a delta, approved) |
| W3 | LOAD-3.1 | OPEN |
| W3 | LOAD-3.3 | PART (its `update_file` half is W3's) |
| W3 | LOAD-5.1 | OPEN |
| W3 | LOAD-5.2 | OPEN |
| W3 | LOAD-4.7 | OPEN (its index half; blocked, see its row) |
| W3 | TYPE-4.1 | OPEN |
| W3 | TYPE-4.2 | OPEN |
| W3 | TYPE-4.3 | OPEN |
| W3 | TYPE-4.4 | OPEN |
| W3 | NAME-2.4 | OPEN |
| W3 | REG-16.5 | OPEN |
| W3 | REG-14.3 | OPEN |
| W3 | CHECK-8 | OPEN |
| W3 | REG-17 | OPEN |
| W4 | STUB-7.1 | OPEN (a delta, approved) |
| W4 | STUB-7.2 | OPEN (a delta, approved and corrected) |
| W4 | STUB-7.3 | OPEN (a delta, approved) |
| W4 | STUB-7.4 | OPEN |
| W4 | STUB-7.6 | OPEN |
| W4 | STUB-7.7 | OPEN |
| W4 | STUB-7.8 | OPEN |
| W4 | STUB-7.9 | OPEN (a delta, approved) |
| W4 | STUB-7.10 | OPEN (a delta, approved) |
| W4 | STUB-7.11 | OPEN |
| W4 | STUB-7.12 | OPEN |
| W4 | STUB-8 | OPEN (a delta, approved, both parts) |
| W4 | STUB-12.1 | OPEN |
| W4 | STUB-12.2 | OPEN |
| W4 | STUB-12.3 | OPEN |
| W4 | STUB-12.4 | OPEN |
| W4 | STUB-12.5 | OPEN |
| W4 | CHECK-10 (its version table) | OPEN |
| W4 | STUB-13.1 | OPEN |
| W5 | REG-15.1 | OPEN |
| W5 | REG-15.2 | OPEN |
| W6 | STUB-10.1 | OPEN |
| W6 | STUB-11 | OPEN |
| W6 | STUB-13.2 | OPEN (reworded when W3 lands) |
| W7 | CHECK-9 | READY (its F69 half is FIX-69, in the README's batch 0 rows) |
| W7 | CHECK-7 | OPEN (its checker half; a delta, approved) |
| W7 | CHECK-10 (its operator sites, D3) | OPEN |
| W7 | CHECK-12 | OPEN |
| W8 | VAL-1.13 | OPEN |
| W8 | VAL-1.14 | OPEN |
| W8 | VAL-1.15 | OPEN |

W5 also has the C6 to C10 rows, which are written with the plan's approval and have no ledger IDs yet. The ledger's rows for W5 are REG-15.1 and REG-15.2 above. W7's other rows are written after a re-read (#16, #20 and #21), so the four above are all the ledger rows it owns today.


Sources: `WORK_STACK.md` (levels 1 to 7), the ledger's REG, STUB, CACHE, CHECK, TYPE, NAME, VAL and Q
sections, `PROFILE_DRILL_PIWIGO_20G.md` §12, and `CRATE_SPLIT_PLAN.md`'s stages C to G. Nothing here is
built or measured. Figures are the ledger's and the
reports', and "none" means the ledger gives no instruction figure. Steps are W1 to W8 (W for wall),
so they do not collide with Part B's D1-D9 or the crate split's phases D1-D6.

**TYPE-4.2 (W3).** Its "provisional answers" cannot come from the hierarchy, which has no salsa cycle, and the relation
reads resolved types, not constants' values.

**REG-11.14 is superseded.** The other REG-11 rows stand.

## What is left when batch 8 is done

Level 8's READY list is empty, so the stack's rule ("work the lowest item that has a READY row")
picks nothing. Level 2 still has READY rows (REG-2, REG-4.2, 4.5, 4.7, 4.9, 4.10, 4.12 to 4.14,
REG-5, REG-6, REG-16.2 and the Q-11 follow-ups), but levels 1 to 7 are deferred "to
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

The decisions below change it in five places: W1 is a correctness backlog and partly runs before
item 9 (#22); W2 lands REG-4.15 with level 2 (H3.6); W3 includes Stage F1 and F2 (#19), the six passes' per-file
facts (#39) and STUB-7.5, the lazy Composer locator, which leaves W4 (H4.1); W5 builds C7 on F41's lowering (#14);
W7 includes item 18's reconciled inference core and scopes (#16), F69 with CHECK-9 and CHECK-7's checker half (the crate
split's D2) and CHECK-10's operator sites (the crate split's D3).

| step | what | rows, in order | notes |
|---|---|---|---|
| W1 | the correctness backlog and the follow-ups that need no engine | the Q-11 follow-ups LOAD-7.1, 7.3-7.7, REG-18.1, 18.5, CHECK-11 (REG-18.4 is batch 4b's: it needs `ConstName`); REG-2 (the gates' inventories); the data-model rows the ledger lists READY with no need: REG-4.5, 4.7, 4.13, 4.14, REG-5, REG-6 (REG-4.9 is REG-4.12's change and is built with it, W2 step 1; REG-4.10 is built with DOC-2.9, batch 6); level 3's findings | placed in the sequence, not left "between heavy runs": every finding or follow-up that changes a dump goes before the baseline in batch 0, the rest between heavy runs, and a later behaviour-changing commit runs `dumps-compare`, lists its differences and rolls the baseline forward (#22); none depends on the engine; REG-18.2, 18.3 and 18.6 are W2's needs; LOAD-7.6, REG-18.4 and CHECK-11 are deltas the ledger marks READY with no approval in VAL-2.9 or decision 14, approved with the conditions of #42 (LOAD-7.6 is W1's except that its range per owning manifest is designed with W2), and LOAD-7.2's PHPUnit half is ruled: ungated; LOAD-7.5 and the Q-11 table cite `lsp/workspace_config.rs:66` and `:214`, a file LOAD-1.3 deleted: the LSP's lookup is `root.join("phpstan-rs.toml")` in `lsp.rs` (found by that symbol), to correct in the row when it is worked |
| W2 | C4's spine, the engine's roots first | REG-4.2 and REG-4.12, then REG-16.2, REG-16.3, REG-7.1 to 7.12 (7.6 needs REG-18.2 and 18.3; REG-4.15 lands with level 2: H3.6 step 5), REG-8.1 to 8.7 (8.4 needs REG-18.6), REG-9, REG-10, REG-4.11, REG-11, REG-12 | each commit runs `cargo check`, clippy and the touched modules' tests, and the full gate tier, `registry-dump` and O2 run once at W2's stage checkpoint (the crate split's rule: validation at the stage checkpoints you named, not per level or per commit); most rows are approved deltas (VAL-2.9); REG-4.2 and 4.12 meet NAME-1.2's registry entry points (batch 4), so they are written on the `Name`-keyed API |
| W3 | what the engine opens: the prizes | REG-16.4, then CACHE-13.1 (the stamp, for every file) and CACHE-13.2, STUB-7.5 (the lazy Composer locator, H4.1), LOAD-3.1 (and 3.3's `update_file` half), LOAD-5.1 (the six passes' per-file facts as `file_products` fields, then the use-site index), LOAD-5.2; LOAD-4.7's index half; TYPE-4.1 to 4.4, NAME-2.4; REG-16.5; REG-14.3; CHECK-8 (CHECK-7's checker half is D2's, W7); REG-17 | REG-16.4's listed needs (REG-16.3, SYN-1.2, LOAD-1.4) stop short of REG-7 and REG-8, but LOAD-4.6's text has REG-7 and REG-16.4 together replace `closure_scope` (REG-7 the passes that use it). H3.5 and H4 settle it: REG-8's rows are the inputs and queries it builds on, and REG-9 to REG-12 are W2's |
| W4 | C5, the stub layer | STUB-7.1, then 7.3 (it opens SYN-1.10; PROD-5.4 is gone with REG-16.4) and 7.6 (CACHE-9/10/11 follow, once CACHE-13.2 has fixed the persistence's format); 7.2, 7.4, 7.7 to 7.11 (7.5 is W3's); the gate 7.12; STUB-8; STUB-12.1 to 12.5; CHECK-10's version table (its operator sites are D3's, W7); STUB-13.1 | STUB-7 needs REG-12 as a parent row, and 7.1, 7.2, 7.4, 7.7, 7.8 and 7.11 list no need of their own; deltas are VAL-2.4, 2.9 and 2.10 |
| W5 | C6 to C9, then C10 | the rows are written with the plan's approval (#20, J3); then the overlay layer, `TypeNodeResolver` (built on F41's lowering, batch 6, which has already deleted `types::parse`: #14), the extension points, the summaries; REG-15.1 and 15.2 | the plan lists many approved deltas for each |
| W6 | the embedding | STUB-10.1, then STUB-11; STUB-13.2 (what remains of the stub loader once W3's Session has replaced `Project`; the row is reworded when W3 lands) | the cold-run prize |
| W7 | Stages D to G | CHECK-9, CHECK-7 (checker half), CHECK-10 (operator sites), CHECK-12 above; the rest are written after a re-read (#16, #20 and #21) | the architecture program |
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
   meets what the ledger builds, as J3 below sets out.

   The plan orders Stage D "structure before semantics" because a transition built on
   `check_stmt`/`check_expr` "would be written twice" (Appendix F). The same reasoning applies to item
   18's inference core and its scopes group, which are structure, and to F69 with CHECK-9 and CHECK-7's checker half,
   which sit on the recorders D2 replaces; its temporaries group (CHECK-6.4 to
   6.12, 6.16, 6.17 and the scratch reuse of 6.18) is local and survives either way. (#16 and J3 compare §12.10 with
   D1 to D4 and find one architecture.)
3. **Stage C is the price of the prizes in three tiers.** W2 and W3 (C4's engine slice) give the
   wall-time and warm-run prizes and TYPE-4. W4 (C5) gives item 6. W5 and W6 (C6 to C10) are what
   STUB-11's cold-run prize costs: it needs REG-15, which needs C6 to C9. No tracked row needs Stages
   D to G; REG-7.8 and STUB-7.8 name F1 and E4 only as later clean-ups.
4. **DOC-2.1 and C7 delete the same grammar.** DOC-2.1's "text grammar" is `types::parse_type_string`
   (the crate split's §1 calls it the second grammar: 219 mentions in 30 files, besides the 58 sites
   of `parse_written_type`), and C7 deletes `types::parse` with it. (Superseded by #14 and I4: F41 builds the lowering in batch 6 and deletes `types::parse` there; C7
   then builds the name scope on it.)
5. **The 20 G plan's end validation will find items 4, 6, 7 and 17's second half unbuilt** if it runs
   after item 19 as written, and VAL-1's tooling (the canonical type printer, `registry-dump`, the
   expression-types dump) does not exist (#13 builds it in batch 0). `registry-dump` taken before and after REG-7 would show what
   the change does to the registry itself, beyond what the crate split's gates (corpus, type
   snapshot, baseline, nsrt) see.

## The engine (H2)

The engine's rows are Part D's: REG-16.2 and REG-16.3 in W2, and REG-16.4, CACHE-13.1 and CACHE-13.2 in W3. The rows
that this section names as inputs (REG-16.1, S-3, S-5, S-7) are owned where they are built. The section is a contract
(what the code above the engine may rely on), so that the choice of salsa stays reversible.

### H2.1 The contract, and what it is built on

What the code above the engine may rely on, whichever engine is below:

1. **Inputs change only by a driver write.** A query is a pure function of inputs and other queries: no clock,
   no environment, no filesystem except through the file source, no state that depends on the thread or the
   order.
2. **Reads are recorded:** every input and query a query called, an absent name included.
3. **Early cutoff.** A new result that is equal (H1.5b) to the old one stops the change there. salsa backdates
   only when the new memo has no cycle heads, is not provisional and did not drop in durability, and only when
   the old value is still there (`function/backdate.rs`): so every member of a recursion SCC (purity, property
   writes, inference, constants on a cycle) re-runs its readers whenever anything in the SCC re-executes
   (cheap for constants and purity, relevant for inference), and an evicted `parse_analysed` cannot be
   compared; eviction costs less than it looks, since a re-executed memo's `changed_at` is the maximum of its
   inputs', so an evicted tree re-parsed from unchanged inputs invalidates no reader, and one whose inputs
   changed could not have been backdated anyway.
4. **A write cancels** the queries running on other handles; a cancelled unit is retried or dropped, and no
   partial result is ever visible.
5. **A cycle** is resolved by its rule (H3.3) or is a bug, reported as one.
6. **Results are `'static` values.** A borrowed view lives as long as a database borrow.
7. **Determinism:** the same inputs give the same results at any thread count and in any order (B8's
   equalities, tested through G5's dumps).

Built on salsa 0.28.5 in the engine module (#33; a crate from G1). It uses `#[salsa::db]`, `input`,
`interned`, `tracked` (with `returns(ref)`, `lru`, `cycle_result`, `cycle_fn` and `cycle_initial`, `heap_size`),
`Durability`, `Cancelled`, `cancellation_token` and `persist`, and one tracked struct, `FileProducts` (H2.3),
whose fields are tracked one by one (`tracked_struct.rs`: `update_fields` moves a field's revision only if its
value changed; `tests/expect_reuse_field_x_of_a_tracked_struct_changes_but_fn_depends_on_field_y.rs` is the
pattern). It does not use accumulators: a diagnostic gathered in an accumulator is outside the result, so
outside cutoff and outside the persisted value. **The feature set is pinned:** `salsa` with
`default-features = false` and `macros`, `inventory`, `persistence` and `salsa_unstable` (`heap_size` needs the
last; `rayon` is used by salsa only for an `Either` impl and `accumulator` is unused), and `triomphe` without its
default features; `persistence` adds `serde`, `erased-serde` and `thin-vec/serde`. The crates new to `Cargo.lock`
are `salsa`, `salsa-macros`, `salsa-macro-rules`, `boxcar`, `intrusive-collections`, `inventory`,
`portable-atomic`, `thin-vec`, `tracing`, `tracing-core`, `typeid`, `erased-serde` and `triomphe`. The repository
runs `cargo deny check` (`deny.toml`, `rust-ci.yml`), so S-1's and S-2's commits pass it (advisories, yanks,
licences), and S-3 adds the clean and incremental build times and the release binary size to its measured
list, since the project has a build-time gate.

### H2.2 Inputs

| input | grain | holds | durability |
|---|---|---|---|
| `ReflectionInputs` | singleton | reflection settings, the configured PHP version range (decision 6; when set it overrides every owner's), assertion tables, `stub_paths` | high |
| `CheckInputs` | singleton | the rule settings and the level | high |
| `OutputInputs` | singleton | `ignore_errors`, the baseline, the formatters' settings; read only by `analysis` | high |
| `File` | one per path ever seen | `path` (set once), `shown` (the path as reports spell it, R6's `/` form relative to the report root, set with the path and by the driver when the root changes), `number: FileId` (an opaque handle: nothing is ordered by it, E1-10), `role`, `analysed`, `stamp` (analysed and eager files; a lazily reached context file's is the query `file_stamp`, below), `overlay` | analysed: low; context: medium; configured stubs and the installed PHPStan's files: high |
| `FileSet` | singleton | the sorted `Vec<File>` that exist now, with the path index that `locate` and the include resolution read, and the index by `shown` that an anonymous class's name is resolved through (H1.4) | low |
| `Packages` | singleton | Composer roots, autoload maps with their keys (classmap entries, PSR-4 and PSR-0 prefixes with their directories, files), owners, vendor directories, and per Composer root its declared PHP range (`config.platform.php`, else `require.php`) and its required packages (#42) | medium |
| (`Builtin`) | **not an input** | the stub corpus (`registry::stubs::Corpus`, an `Arc` shared by every database in the process) is a constant of the database: the build identity in a persisted file's key covers any change of it, and a never-changing singleton could not be persisted without a custom codec (salsa asserts that every input a persisted memo reads is `persist`, and a custom `deserialize` gets no database argument) or set again after a restore | – |
| `SqlSchema` | singleton | the schema file's digest, or a live reflector's fingerprint | medium |
| `ProbeEpoch` | singleton | a counter the driver bumps when the filesystem, or a live SQL schema, may have changed (H4.5) | low |
| `Extensions` | singleton | the registered extension implementors and a fingerprint of the set and its configuration (J2) | high |

- Splitting the settings in three is the grain at which a change should reach: a formatter or baseline change
  must not re-resolve a class, and a rule change must not either. Within an input the grain is the field: a salsa
  input field has its own revision and durability, and a read records that field alone (`input.rs:230`). So each
  setting is a field of its input, and the checker reads settings through a view that fetches a field from the
  database the first time a check uses it and keeps it for the rest of that check (one `OnceCell` per field).
  The recorded reads are then the settings that file's check consulted, and toggling a rule reruns the files in
  which that rule's setting was consulted, not every file. Copying every field into `Checker` when it is built
  would make each file depend on all of them, so the checker's `self.settings.x` reads become calls on the view
  (mechanical: 114 of them in 34 files of `checker/` today, reaching the leaf flags of the extensions'
  `…Settings<bool>` structs, which are the fields).
- **The range of a file is its owner's (#42).** The owner of a file is the nearest
  ancestor `composer.json`, a fact of each directory's listing: A8's scan finds a degrade root's nested manifests before
  the walk (COMP-8) and `Packages` holds those directories (A1 builds the vendor fact beside it and not this one); a file
  with no owner uses the project root's manifest. `range_of(File)` is a
  tracked projection of `Packages`: the configured range if one is set, else the owner's declared range (Composer's
  grammar by interval arithmetic, COMP-2), else decision 18's default (open above: #40). It compares equal when the owner's
  range does, so a manifest edit reruns only the files it owns. A declaration's validity is a `VersionSet` that
  does not depend on the range, and the reader intersects it with its own file's range (J1.2); a builtin reading is
  selected with that range (`Readings::over`). A level's memo is keyed by the range only where something it reads
  differs across versions (J1.2), so a class with no version-sensitive ancestor has one memo whatever the owner's
  range, and a project with one range pays nothing. Activation is any owner's: it only turns checks on. A hull of the owners' ranges would judge Symfony's 202
  manifests at `>=8.4.1` against versions they cannot run on, and the root's alone would judge its 8 manifests at
  `>=8.1` as if they needed 8.4.
- `stamp` says whether the file's content may differ, and what it holds follows decision 13 (CACHE-13.1). On a
  file system known to keep a change time that no program sets (ext4, xfs, btrfs, APFS and the like) it is the
  `stat` tuple, trusted under the racily-clean rule, whose reference time is the clock reading taken at the file's
  own stat (#15; decision 13's "the index's own write time" does not fit a database written after the whole
  check); on any other, and on Windows, it is the content hash, so
  the file is read and hashed. Either way the driver bumps it when the content may differ (an overlay changes, or
  the rule distrusts the tuple). The walk takes no `stat` today, so the stamp is new work: for the analysed and
  the eager files, which are all read anyway, the loader takes it from the open descriptor before the read. A
  **lazily reached context file** (H4.1) has no input stamp, because a query may not write an input: its stamp is
  the tracked function `file_stamp(File)` over `ProbeEpoch`, statted by the query that first needs it before it
  reads the bytes (the design of `path_is_file`, H4.5), so a first read is tracked, a later edit is found when
  the epoch moves, and a restore does not turn "none" into "some". The file *source* is not an input; it is
  a handle in the database (the overlay map and the disk), read only by `file_digest(File)` and
  `parse_analysed`. Because the driver bumps `stamp` whenever the content may differ, a read is always
  consistent with it.
- `shown` is a field because the checker takes it (`set_current_file`, the cross-platform plan's R18) for the path
  rules (`in_test_code`, the restricted symbols' path rules) and the finalizer matches `ignoreErrors` paths and
  baselines against it (R6); an anonymous class's name carries it. It is a function of the path and the report root,
  so it is a field of the input the driver sets and not a query; a change of root rewrites it, and only the files
  that read it rerun.
- **Honesty rule:** an input is changed by the driver and by nothing else, and a query reads nothing but inputs
  and queries. The checker has two live reads, and both are made queries over the epoch input to keep it:
  `include_target_exists` (`path_is_file`, H4.5), and a live SQL reflector, whose `PREPARE` per distinct SQL text
  (`resolve_sql_row_type` falls through to `context.reflect(sql)`, cached by text in `SqlContext`'s own map) is
  the tracked function `sql_columns(SqlText)` over `SqlSchema` and `ProbeEpoch`: the driver bumps the epoch at
  each run start and when drift detection fires, so a persisted answer is asked again once per run and an
  unchanged answer cuts off, and `SqlContext`'s own cache goes (the memo is the cache). A closure-gate case
  changes the test database's schema between two runs: the warm run's diagnostics equal a cold run's.

### H2.3 Queries

`returns(ref)` throughout: a reader gets `&'db T` from the memo, no clone. Equality is H1.5b's.

| layer | query | key | result | notes |
|---|---|---|---|---|
| file | `file_digest` | `File` | the content digest, nothing else | **analysed files only.** Depends on the stamp (`File.stamp`, or `file_stamp` for a lazily reached file) and is persisted (digest only). The text is not a result: `parse_analysed` reads it from the `FileSource` handle under the stamp's protection, so a restored `file_digest` memo that is valid by stamp never hands out a missing text (H4.6). A context file keeps no text (LOAD-3.4): its `file_products` reads the stamp and the bytes once, in the worker's reusable buffer, and the products are the cutoff, so a touched but identical file stops there; its skimmed bodies' text is a field of its products, below |
| file | `parse_analysed` | `File` | `Arc<Parsed>`: the tree, errors, marks | a tracked function for **analysed files only**, which need their tree twice (their products, then their check). Kept for the whole run in the batch (no `lru`) and under `lru` in the server (H2.9). A context file has no tree query: its tree is built inside `file_products` (H2.9). Infallible (G3) |
| file | `file_products` | `File` | a tracked struct with one tracked field per product: `names` (PROD-5.8's record), `decls` (`FileDecls`, positionless, H1.5a), `decl_names` (the file's `DeclRef`s alone, anonymous classes left out, which `decl_index` reads), `decl_edges` (each class's `extends`, `implements` and `use` names as written, positionless, which `hierarchy_cycles` reads), `sites` (the positions), `facts` (mentions flags, `has_declaration_in_body` and the rest, and the file's validity `VersionSet` from its marks, G3.5), `globals` (H3.7), `bodies` (a context file's skimmed function bodies, each copied out by ordinal as PROD-4.2 does today, so that `decls` holds no body text and a body edit leaves it equal; an analysed file's bodies are in its tree), `doc_blocks` (DOC-2.9: each doc comment parsed once and found by its offset, read by the checker and by no level), `ignores`, `lines` (`LineStarts` or `LineIndex`, SYN-3.1), `use_sites` | made in one pass over the tree, so no intermediate is a memo: an analysed file reads its `parse_analysed` memo, and a context file's tree is built in the worker's scratch builder and cleared once the products exist (LOAD-3.5); **a reader depends on the field it reads:** a body edit leaves `decls` equal, a member edit changes `decls` and leaves `decl_names` equal, a line inserted above a class changes `sites` and nothing else |
| index | `decl_index` | none | names to `[DeclRef]`, over every eager file's `decl_names` | O(all declarations); see below |
| index | `declarations_named` | `ClassKey` / `FunctionKey` / `ConstKey` (generic over the key) | `Arc<[Candidate]>`, sorted by **the files' path order** (the `FileSet` order), never by `FileId`: a `DeclRef` (or `ConstDeclRef`) and its validity `VersionSet` (the `Availability` over a range is the reader's, H2.2) | **the recorded read of a name.** An absent name is an empty slice. A file's declarations exist only where its syntax holds (decision 15, Q-10): the availability of a project or context declaration is its file's validity `VersionSet` (G3.5's marks, a field of `facts`), which the reader meets with its own file's range (`range_of`, H2.2), and a file valid nowhere in that range gives that reader no candidate, as PHP refuses it there. A file number is first-seen order (`file_table.rs`: a new path takes the next free number), so a database edited into its state and a fresh load number the same paths differently; sorting by path order (which `FileTable::rank_of` already is) is what keeps candidate order, the `global_site_index` lists and every accumulation, and with them a union's member order and "previously declared at", equal between the two (a test builds one file set by two histories and compares `registry-dump`, candidate order included) |
| index | `hierarchy_cycles` | none | the edges dropped | one pass over the declared edges (`decl_edges`) of the project's and context's classes (H3.3); a member or signature edit reruns nothing in it |
| class | `class_ancestors`, `class_own`, `class_inherited`, `class_members`, `class_final` | `DeclKey` | REG-7's five levels | H3 |
| class | `class_member` | `MemberKey` (`DeclKey`, `MemberName`) | `Option<Arc<Member>>`: one member of `class_final`, absent included | a projection, so its readers rerun only when that member changed (H2.4) |
| function | `function_final` | `DeclKey` | REG-7.12 | |
| constant | `global_constant` | `ConstDeclRef`, interned | type and throws | keyed by a declaration like the levels; the name-level combination of candidates is the `Provider`'s (H3.4) |
| constant | `class_constant_value` | (`DeclKey`, name) | value type and throws (decision 1) | `cycle_result` |
| constant | `class_constant_value_only` | (`DeclKey`, name) | the type without the constant's PHPDoc, and whether the PHPDoc was dropped | the fallback of the PHPDoc case in H3.3(2); no PHPDoc edge enters it |
| body | `body_summary` | (`DeclKey`, member ordinal) | `BodySummary`, `InitFlow` | on demand, from the declaration's body text |
| body | `purity`, `property_writes` | `DeclKey` / member | verdicts (REG-1) | `cycle_result` |
| body | `receiver_types` | (`DeclKey`, member ordinal) | the inference in a no-effects mode, for the receivers that stage (H3.8) | may not read `purity` or `property_writes`; read by both |
| body | `infer_body`, `inferred_return`, `inferred_throws` | `DeclKey` / member | REG-17 | fixpoint with widening (H3.3); `infer_body` reads `purity`, `property_writes` and `receiver_types` |
| body | `effective_return`, `effective_property` | `DeclKey` / member | the declared type, else the inferred one (H3.8) | the inference family; the `Provider` reads signatures from `class_member` and these |
| check | `check_file` | `File` | `FileCheck`, a tracked struct with one tracked field per fact (diagnostics before finishing, member refs, escaped enums and classes, named-argument facts, property-assign facts, property-type sites), so each pass depends on the fact it reads | the type dump is a request, not a memo |
| check | `property_type_verdicts` | the class group (the declaring `DeclKey` of the writer scope) | `judge_property_types`'s verdicts for that group | `judge_property_types` merges the facts of every writer in path order, expands them through used traits and re-runs the checker on the other files declaring the class "for their writes alone" (`project.rs`), so the verdicts are keyed by group over an index query of writers built from the check facts; a keyless query would make the first worker to arrive compute every `check_file` while the others block |
| check | `file_diagnostics` | `File` | `check_file`'s diagnostics with the per-file project rules added (duplicates, ignore parse errors) and the verdicts of this file's own property-type sites (`check_too_wide_property_types`), sorted | the batch phases stay explicit: all `check_file`s first, then the verdict-reading diagnostics and the passes |
| project | `global_sites` | `File` | the file's `FileSites`: its global-variable facts resolved through the provider | H3.7 |
| project | `global_scope_files`, `global_site_index` | none | the files whose top level runs in global scope; each name to the files that assign it | H3.7 |
| project | `global_variable` | `GlobalKey` (a `Name`, exact) | one global's accumulated type | H3.7; the provider's `global_variable(&Name)` |
| project | the whole-program passes | none | dead code, class shapes, architecture, banned constructs, Composer and mock hygiene, unmatched ignores | each reads per-file facts, not trees (LOAD-5.1) |
| project | `analysis` | none | `AnalysisResult`: the finalizer over `file_diagnostics`, the passes and `OutputInputs` | |

- **Notation.** `file_decls(file)`, `file_sites(file)`, `file_names(file)` and `line_index(file)` in the text below
  are the `decls`, `sites`, `names` and `lines` fields of `file_products(file)`, each read with its own
  dependency.
- **The declaration index.** One tracked query over every eager file's `decl_names` (the `DeclRef`s alone),
  O(all declarations), run again when a declaration is added, removed or renamed and not when a member or a body
  changes: those change `decls` and leave `decl_names` equal, which is why the two are separate fields. It
  beats the alternative designs on what an edit costs: per-name input slots maintained by the driver
  put derived data back into inputs and need a second revision per edit; a whole-index query
  is plain, and the re-running readers (`declarations_named` of the names asked so far, a hash probe each,
  about 374 Ir by REG-16.1's figure) back out at once when their slice is equal. For Piwigo's size that is of
  the order of 10 to 50 ms per edit that adds, removes or renames a declaration, and nothing for any other
  edit. S-3 measures it; the alternative is the slot design, and the switch is local to these two queries.
- **Why `declarations_named` returns declarations and not one:** decision 11. A name binds to the
  declarations PHP may bind it to; most names have one, a guarded polyfill has two, and each is its own
  entry. `Candidates` is `One(DeclRef)` or `Many`, and the level queries are per declaration; the provider
  combines candidates the way decision 6 combines a builtin's readings (H3.4).
- **`Builtin` declarations** are not in `decl_index`: `declarations_named` adds them from the corpus where they
  are available in the version range (J1.2), and their levels are queries keyed by `DeclKey` too, built from the
  corpus records. The corpus never changes, so queries that read nothing else are never re-verified.

### H2.4 The `Provider`: the accessors, and how a name is read

`Provider<'db>` is the only way the checker, the rules and the LSP read reflection. It holds `&'db dyn Db` and a
per-provider table (`FoldedName` to the answer), is made per check or per request, and is not `Sync` (a handle per
thread, as the database is). Its methods are REG-4.12's accessors (about 280 sites outside `registry/` become
calls on it): `class(&Name)`, `function(&Name)`, `constant(&ConstName)`, `method(&ClassView, &FoldedName)`,
`is_subclass_of`, `ancestors`, `class_constant`, `purity`, and the whole-registry iterations REG-7.10 names.
**The checker, the rules and the LSP do not name `Provider`: the checker-facing reader is a trait in
`reflection`** (these accessors; generic or `&dyn` in `Checker<'a>`), with `engine::Provider` as its implementor.
`analyser` sits below the engine, and a `Checker` that named `engine::Provider` would make `analyser` depend on
its own consumer, which `engine` already is (it calls `check_program`), a cycle the module graph forbids; the
trait keeps `File`, `DeclKey` and the other keys out, as H1.6 intends. H3.1's `LevelReader` is the smaller trait
on the level side.

- A read of a name: probe the provider's table by the name's stored hash (67 Ir, REG-16.1's figure for "one query
  per name per file, then a table of the file's own"); on a miss, `declarations_named(ClassKey::new(db, name))`
  (`FunctionKey` for a function, `ConstKey` for a constant) and the candidates' `class_final`. The provider keeps
  the answer, an absent one too. Inside a tracked function (`check_file`) the reads are recorded; outside one (an
  LSP request, a test) they are plain.
- **Granularity: a read goes to the query that answers it.** Whether a name exists reads `declarations_named`
  and nothing else; a subclass test reads `class_ancestors` (level 1), which an edit to a method or a property leaves equal; a
  member (a method, property, class constant or enum case, present or absent) reads `class_member`, a
  projection of `class_final` that returns that one member and backdates when the member is equal; only a
  whole-class question (abstract-method and interface-completeness checks, member listings) reads `class_final`.
  So editing one method's signature reruns the files that read that method, where today's `dependents` and
  `signature_dependents` stop at the class and rerun every file that reads it at all. `MemberKey` is the engine's
  interned (`DeclKey`, `MemberName`: H1.4); a method is asked by the `FoldedName` it was written as, so one
  method has one memo and one edge whatever spelling is used, and a property, class constant or case by its
  exact `Name`; an absent member is a recorded read (`None`), so a member added later
  reaches the readers that looked for it, and the `__call` and `__get` fallbacks are member reads of their own.
  When `class_final(C)` changes, the projections of C that were read rerun (a lookup and an equality each).
  **Cost, unmeasured:** REG-16.1 counted names (107,068 (file, name) pairs), not members, so the edge count at
  this grain is S-3's: it counts the distinct (file, class, member) reads and what their edges cost (a
  `QueryEdge` is 12 bytes and a packed one 8 in salsa's source, `zalsa_local.rs`, besides the memo's own
  overhead, H2.9), against the criteria H5 gives. The finer grain is the design; reading `class_final` for
  members is the fallback if S-3 kills it, a `Provider` setting that changes no caller.
- **Signature against body.** `class_final` holds no body, summary or position. What a body shows (purity,
  property writes, an inferred return) is its own query, asked when a rule asks, so a file that only calls a
  method depends on the method's signature and not on its body.
- **REG-4.12 first, on the old storage.** The accessors land before the engine, over the existing maps; the
  engine then replaces what is behind them. That is W2's order (H3.6).

### H2.5 The check

`check_file(db, File)` builds a `Provider` and a `Checker` (G1), runs `check_program` over
`parse_analysed(file)`'s root, and returns `FileCheck`. It reads: the file's `parse_analysed`, `file_decls`, `file_sites`, `file_names`, `line_index`; the
`CheckInputs` fields it consults (H2.2), `ReflectionInputs`, `Packages` (the owner); everything its provider reads;
and an included file's `file_digest` (an analysed file) or stamp (a context file's `file_stamp`), whose tree `includes.rs`
takes from `parse_analysed` or, for a context file, parses unmemoized, so editing an included file rechecks its includers, which today is the dependents index's job. **The statement-level poll `db.unwind_if_revision_cancelled()` is new work:** today `should_abort()` is
polled once per affected file, between `check_file` calls (`project.rs`), and no poll exists inside `checker.rs`
or `checker/*.rs`, so a cancelled write waits for the file being checked. The polls inside a file are added per
statement and, where one statement can be huge, per item of an array or call-argument list, per member when a
union is built, per declaration in the products pass and per size step in a parse over a stated size (S-5's accept
includes a 50 MB array-literal statement). Requests that need more than the memo (inlay hints, semantic tokens, hover,
completion probes, find-references' member collection) run the checker again, unmemoized, on a `Provider` outside any
query, as `checker_for` does today; E6's observers (Stage D) later replace the re-run.

### H2.6 Revisions

- **The batch run** writes every input before any query runs, and no write follows. salsa starts a revision
  at each input setter (`ingredient_mut` calls `new_revision`), so N writes are N + 1 revisions; that is
  harmless here (no handle exists to cancel, and the results do not depend on the count), and the cost of a
  `cancel_others` per write is paid only in the server. Nothing in the batch run is evicted by `lru`, since
  trees are not memos (H2.9).
- **The server** makes a revision per write, and **applies a notification's events as one batch** (today
  `handle_watched_files_changed` takes the whole `Vec<FileEvent>` and coalesces it to the last event per path;
  one `FileSet` write per event would cancel and wait once for each): `didOpen`, `didChange` (the overlay and the
  `stamp`), `didClose`, watched-file events, configuration. After the writes the server asks for `file_diagnostics` of each analysed
  file in priority order (the edited file, the open files, the rest), and publishes each as it completes and
  only if it differs from what was published. A file whose reads did not change verifies in a few
  microseconds (REG-16.1: all 2,792 in 5 to 10 ms); a file that read something that changed runs its check.
  This replaces `walk_affected`, `closure_scope`, `dependents` and `signature_dependents` wholesale.
- **TYPE-4's memo** carries the revision it was filled in and is cleared by the first access in a later one. A
  write cancels running queries, so no thread reads the memo at an older revision after a newer one began.

### H2.7 Concurrency and determinism

One database handle per worker, on the existing pool (`worker_pool`, 64 MiB stacks). **How the handles are
made:** the database is `Send` and not `Sync` (its `ZalsaLocal` holds a `RefCell`), so a closure that captures
`&db` cannot be shared; and rayon's `for_each_init` calls `init` once per rayon job, not once per thread, so it
would make a handle per job split. The driver therefore makes N handles (`db.clone()` on the driver thread, or a
`StorageHandle` and `into_storage` on each worker) and puts one in a slot per pool thread (indexed by
`rayon::current_thread_index`), or hands one to each of N long-lived worker tasks that pull files from a queue.
A write waits until every other handle is dropped (`Storage::cancel_others`, `storage.rs`, with no timeout, which
warns that a worker holding two handles deadlocks it), so a thread holds at most one, none survives into a write,
the driver stops dispatching once a write is pending (a worker that starts a unit after the flag is set only
makes a handle to drop it), and the server's request handlers take a handle per request and drop it before they
return. **The server applies writes on a thread other than the one that answers `$/cancelRequest` and
`shutdown`,** since the writer waits for every handle and an unbounded wait must not hold up those.
**Queries do not use `rayon` inside, and no closure inside a query touches a database handle:** a tracked query
body reads everything it needs on its own thread first, then fans plain data out to rayon (a handle cloned on
a worker would record its reads on its own query stack, not the pass's, and a same-thread cycle could be
reported when rayon runs a job inside a wait); the whole-program passes read the per-file facts sequentially
(about 2,800 reads) and parallelise only the pure merge, or are plain driver functions over query results. A
query that is already running on another thread is waited for by the asker (salsa blocks, with deadlock
detection for cross-thread cycles). **A stack guard:** a chain of tracked wrappers recurses on the stack (a
`class_ancestors(C)` calls `class_ancestors(parent)`, a hundred thousand deep for a hundred-thousand-class
chain, and constants that name each other likewise), and salsa has no stack growth, so the engine's wrappers call
`syntax::stack::guard`, and S-5 and S-6 add a 100,000-deep hierarchy and constant chain on a 256 KiB thread (F20's
depth test, extended to the engine). Determinism: no query reads the order of a hash map's
iteration (REG-4.14); a union's member order does not depend on scheduling (the rule `judge_property_types`
already follows: facts merge in path order); and nothing the engine sorts is ordered by `FileId` (H2.3). S-9
measures contention on shared levels, which REG-16.1 did not.

### H2.8 Cancellation and panics

- **Cancellation** is salsa's: a write unwinds other handles' queries with `Cancelled::PendingWrite`, and
  `Database::cancellation_token()` cancels one handle's (`Cancelled::Local`); both unwind by `resume_unwind`,
  without the panic hook. `$/cancelRequest` calls the request's token; F1's `CancellationToken` is that token.
  A cancelled request answers `ContentModified` or `RequestCancelled` (#36 says which methods the client
  retries); a cancelled analysis unit stops and the next revision starts it again. **A local cancel is ignored
  while the thread executes a query with `cycle_result` or `cycle_fn`, and everything nested under it** (salsa
  disables local cancellation there, `function/execute.rs`, because a local cancel would poison the cycle; the
  token is also reset when the outermost attach scope ends), so a hover or completion that reaches a constant,
  purity or inference query cannot be interrupted by `$/cancelRequest` until that query returns, and only the
  pending-write flag still works. The inference passes get statement-level polls of the global flag, and S-5 has
  a case: `$/cancelRequest` during a long inference completes the request, and the server drops the answer
  instead of interrupting it.
- **Isolation, in two modes.** Each per-file unit and each request runs under `crash_context::isolated` (F1, the
  crash reporting plan). *At the driver, outside any query,* it first separates `Cancelled` (not an error:
  `Cancelled::catch`'s downcast) from any other payload (an internal error for that unit) and returns
  `Err(Cancelled)` for the unit. *Inside a tracked query* (the crash plan runs each whole-program pass as
  `isolated(Unit::Pass(name), ..)`, and `carry` catches a worker's panic and resumes with a `CarriedPanic`
  payload) it re-raises a `Cancelled` payload unchanged with `resume_unwind` and records only real panics, and
  `carry` passes `Cancelled` payloads through untouched: a `catch_unwind` that swallowed a `Cancelled` would let
  the query return, be memoized and survive into the next revision with a partial result. A unit that got
  `Cancelled::PropagatedPanic` (another thread's query panicked while this one waited on it) is run once more;
  if the same query panics again it is this unit's internal error, as it is for every file that reads a query
  that cannot complete. A test: a write arriving while a pass runs inside `analysis` leaves no `analysis` memo,
  and a real panic in one pass leaves the others' diagnostics.
- **Internal errors are not a query value.** A panic leaves its query unexecuted, so `analysis` and every
  whole-program query that reads a panicking `check_file` would panic too, and no `AnalysisResult` would exist
  to carry the report. So `check_file` catches the panic at its own boundary into a value, `FileCheck` with an
  internal-error record (the panic's payload text and the unit), which compares and persists like any result;
  `analysis` carries those records into `AnalysisResult` (exit status 2 and "a file with an internal error isn't
  cached" are F2's and follow from it: a `FileCheck` holding a record is not persisted). The "N more file(s)
  loaded for context" count of `check_project.rs` is a count of what this run read, which a lazy and persisted
  load changes between warm and cold, so it is reported from the files the load listed, not the files it read.
- **Cancellation by unwinding depends on the stack guard restoring its limit on unwind.** `syntax::stack::check`
  stores the outer limit, calls `stacker::grow` and restores `LIMIT` only when `grow` returns normally; stacker's
  `_grow` catches a panic inside the new segment, drops its own guard and resumes the unwind on the old stack,
  which skips the restore, so a later guarded call on that thread could compare against a freed segment's address
  (the case exists today: `session.rs` wraps the parse in `catch_unwind`). F20's new `stack` crate
  restores `LIMIT` from a drop guard so that it also runs on unwind, with the test F20 lacks: a panic from inside
  a grown segment, caught, then a 100,000-level recursion on the same thread; S-5 repeats it with a `Cancelled`
  unwind.
- **A panic in a query** leaves it unexecuted; the next ask runs it again (the property REG-7.9 asks of
  `OnceLock`) for an ordinary query. **A query with a cycle strategy (`cycle_result`, `cycle_fn`) is poisoned
  instead:** salsa installs a poison guard for each execution (`execute.rs`), a panic leaves a valueless
  provisional memo, and a retry in the same revision throws `PropagatedPanic` again, so a constant, purity or
  inference query that panicked cannot be re-run until the next revision; the unit's internal error is built
  from the first panic's record, kept in the engine by query key. A panic-injection test over a shared level, a
  constant, a purity and an inference query, and a parallel-resolution test over cyclic fixtures, stay REG-7.9's
  tests.

### H2.9 Memory

- **Which trees are memos.** A tracked `parse` with `lru` for every file, evicted by `trigger_lru_eviction`, would
  free every evicted tree on the one thread that evicts, which is the cost LOAD-3.3 measured and
  removed (−138.6 M Ir and −214 MiB peak when a context tree's drop moved to the worker that built it), and most
  trees are context trees. So a context file's tree is built in the worker's scratch builder inside `file_products`
  and cleared once the products are made (LOAD-3.5): it is freed, if at all, where it was allocated, and it is
  never a memo. A context file's tree wanted later (navigation into a vendor file) is parsed unmemoized. An
  analysed file's tree is a memo, `parse_analysed`, because its products and its check both read it. In the
  batch run it is kept for the run and dropped at exit, as the analysed trees are today; LOAD-5.2 (free each
  tree when its check ends) is memory only, and item 9 shrinks what it would free from 151 MB (MEAS-2; the
  analysed trees at exit) to the flat tree's size, so whether it is still wanted is S-7's to say, with the cost
  of freeing on another thread; **until S-7 says, W3 builds it as the ledger has it** (LOAD-5.2 needs LOAD-5.1,
  whose facts W3 builds, and LOAD-3.1; #39), and S-7 chooses between freeing on the checking worker and a
  bounded eviction, or drops it explicitly in Part D. In the server
  `parse_analysed` has an `lru` (`set_lru_capacity`), so the batch and the server hold trees alike (#28) and
  differ only in the capacity; a tree evicted is parsed again from the overlay or the disk, whose digest
  `file_digest` holds (LOAD-5.3). Eviction never loses cutoff for unchanged inputs (H2.1 3).
- **Memos.** A class's levels share structure (member maps behind `Arc`; a level stores what it changes), so a
  class costs about its final reflection plus the ancestors' set. `heap_size` is set on the heavy queries so the
  memo bytes are measured, not guessed. The dependency graph is one edge per distinct read of each query: about
  107,000 for Piwigo's checks at the grain of names (REG-16.1's count), 12 bytes each (`QueryEdge`; 8 packed),
  plus the member reads of H2.4 and the interned-key edges of H1.4, which S-3 counts. **Retained memos (#38):**
  an immortal `DeclKey` keeps the memos of a dead declaration until the database is rebuilt, so `heap_size` also
  measures them over an edit replay, and H4.6's in-process compaction bounds them.
- **The kill criterion** is REG-12's: peak RSS at or below the phase start.

## REG-7 and REG-8 on the engine (H3)

H3 is the design of the five levels as queries, the finishing passes, the three cycle rules, and what the REG-7 and
REG-8 rows become. Its work is built in W2 and W3, which this part owns: the REG-7 rows (REG-7.1 to 7.12) and REG-4.x in
W2, and REG-17 in W3. The rows that H3 names as inputs or outputs, such as REG-17.1 to 17.4, REG-8.6 and REG-10, are
owned by the parts that build them, and the H3.5 rewording is an edit to the ledger, made on the rows it changes.

### H3.1 The five levels as queries

Each level is a tracked function keyed by `DeclKey`, per declaration (decision 11: no merge). A level reads the
declarations, a lower level of any class, and its own level only along an ancestor edge; that rule is what
keeps the queries acyclic.

| level | query | result | reads |
|---|---|---|---|
| (declaration) | `class_decl` | `Arc<ClassDecl>`, the entry of `file_decls(file)`: parents as written, `use`d traits, `@mixin` and `@method` tags, members as declared, doc types as written names and `TypeNode`s, templates, the file's `ImportScope` | `file_decls(file)` (a change anywhere in the file re-runs it; it is a projection and backdates) |
| 1 | `class_ancestors` | the `extends`/`implements` closure in resolution order, the `use` closure in composition order, `@mixin` targets, unresolved edges (a name no declaration answers to, with the class declaring it), the folded ancestor set that `is_subclass_of` reads, the constants composed with the traits' | `class_decl`; `declarations_named` of each parent; `class_ancestors` of each candidate parent; `cycle_edges_of` |
| 2 | `class_own` | own members composed with the `use` closure's declared members, `@method` tags, every type resolved in the class's context (imported aliases, `value-of`, class-constant types, constant-named types), the interim assertion step | `class_decl`, `class_ancestors`, the traits' `class_decl`, and the exporters' `class_decl` and the target's `class_ancestors` for an imported alias; `class_constant_value` and, for a constant-named type, `global_constant` and `declarations_named` (`constant_named_value` reads a constant and a class's existence) |
| 3 | `class_inherited` | PHPDoc inheritance and property hooks | `class_own`, the ancestors' `class_inherited` |
| 4 | `class_members` | each `@mixin` target's public members, added where the class has none of that name; then the member owners | `class_inherited`, the mixin targets' `class_inherited` |
| 5 | `class_final` | decision 8's combination of native and PHPDoc types, the relation reading other classes' hierarchy and members | `class_members`, other classes' `class_ancestors` and `class_members` through the relation |

A level stores what it changes and shares the rest with the level below (member maps behind `Arc`), so the
five memos cost about the final reflection plus deltas (H2.9 measures). `function_final` is REG-7.12's resolver
(`finalize_standalone_sig` today); `global_constant` is the constant case.

**Where the logic lives.** The crate split's L3 `reflection` holds the resolution logic and L4 `analyser` the
checker, and both sit below the database. So the levels are written as **pure functions over a reader trait**
(`reflection::levels::own(reader: &dyn LevelReader, decl: &ClassDecl) -> OwnLevel`, with
`LevelReader::{declarations_named, class_decl, ancestors, cycle_edges_of, own, inherited, members,
constant_value, global_constant}`; its keys are reflection-level declaration references that the engine
interns, so no engine id crosses the trait), and the engine module wraps each as a tracked function whose
reader calls the other tracked functions, so every call records its read. Tests of `reflection` use a reader
over plain maps, with no database; the checker reads through the checker-facing trait of H2.4, which `Provider`
implements. This is what keeps salsa in one module, `engine`, which sits between `analyser` and `project` in the
crate split's layers (L6, the first half of `project`'s role: the result cache and the dependency graph are what
it is), and what keeps the contract of H2.1 implementable by another engine.

### H3.2 The finishing passes, by level

| today (`run_finishing_passes`, `project.rs:1533`) | becomes |
|---|---|
| `canonicalize_class_name_case` | deleted (REG-4.2: folded keys) |
| `flatten_traits`, `resolve_method_tags`, `apply_assertion_table` (interim until C6) | level 2 |
| `resolve_deferred_type_imports`, `resolve_deferred_value_of`, `resolve_constant_named_types` | level 2, on demand, so `class_has_deferred_type`, `function_has_deferred_type` and the eager pass go (REG-7.2) |
| `flatten_phpdoc_inheritance`, `inherit_property_hooks` | level 3 |
| `resolve_deferred_mixins`, `precompute_member_owners` (and `_after_mixins`) | level 4 |
| `precompute_ancestor_names` | level 1 |
| `drop_doc_members_excluded_by_native` | level 5 |
| `reset_demand`, `changed_since`, `walk_affected`, `closure_scope`, `finishing_scopes`, `Scope` | deleted: recorded reads and cutoff |
| `infer_global_variables_of` | `global_sites(File)`, `global_scope_files`, `global_site_index` and `global_variable(GlobalKey)` (H3.7) |
| `rebuild_pest_config` | an ordinary query over the Pest files |

### H3.3 Cycles: three, each with its own rule

1. **Hierarchy (REG-7.1).** One query, `hierarchy_cycles`, runs Tarjan's algorithm over the declared
   `extends`, `implements` and `use` edges of the project's and context's declarations (candidates expanded; the
   builtin corpus is acyclic and left out), over a canonical node order, and returns the edges between two members
   of one component, a self-edge included. `cycle_edges_of(DeclKey)` slices it per declaration, so a cycle
   appearing or going away re-runs only the classes whose own slice changed; `class_ancestors` drops those edges.
   "The same whichever class resolves first" holds by construction, since no class resolves before the pass. No
   salsa cycle is ever entered, so the default strategy (panic) stays for these queries and a hierarchy cycle that
   reached it would be the bug REG-7.9 says it is. Cost: one pass over the declared edges of the project's and
   context's classes (12,546 classes in the 20 G profile's load), run again when declarations change; S-6
   measures it.
2. **Constants (REG-7.6, decision 1).** `class_constant_value` and `global_constant` use salsa's
   `cycle_result` (`FallbackImmediate`): every query on the cycle gets the fallback, which is the rule, "every read
   is `never`, and the throw is its throw point". Its fallback function receives the query's own id
   (`tests/cycle_fallback_immediate.rs`), so the PHPDoc case, a constant whose PHPDoc reaches a cycle, can fall
   back per constant to *its type without that PHPDoc*, and the PHPDoc is reported unresolvable. **The
   value-only query is `class_constant_value_only`** (H2.3: key (`DeclKey`, name), `cycle_result` = `never`
   plus the definite throw, and a flag in its result for "PHPDoc dropped"; no PHPDoc edge enters it). The
   fallback is: ask it; if it is itself on a value cycle the answer is `never` plus the definite throw, else its
   type and the flag. salsa's tests show a query can be called from a cycle initial function
   (`tests/cycle_initial_call_query.rs`) and that re-entering the cycle panics
   (`cycle_initial_call_back_into_cycle.rs`), and `cycle_initial` and `cycle_result` receive the query's own id
   and its arguments (the macro passes them; only `cycle_fn` gets the `Cycle`), which is what S-6 confirms. "Reported once"
   is produced by a report query over the value-only SCC, at the earliest member in declaration order. A
   constant that merely reads a cycle's member computes from the fallback, as PHP's read of it would throw.
3. **Bodies.** `purity` and `property_writes` use `cycle_result` with the impure / `Anything` answer: REG-1's
   rule is that a declaration in or reaching a cycle never leaves it, which is what an immediate fallback
   on every member gives. `inferred_return` and `inferred_throws` (REG-17) use `Fixpoint`: `cycle_initial` is
   `never`. **Termination is shown, not asserted.** A recursive body that wraps its own result
   (`return $n ? [f($n - 1)] : 1;`, a generic or object argument, a closure type) grows one level of nesting per
   iteration while no literal changes; the `types` crate caps union width (64) and shape fields (256) and has no
   depth cap; and salsa does not stop at 200 iterations (`cycle.rs:58`): it panics, in release too, and re-runs
   every member of the cycle each time. Decision 17's "a literal that an iteration changed" is also a function of
   the iteration history, so a cycle entered at another member could end at another type, against H2.1 7. So
   `cycle_fn` is the capped operator, a function of the value alone: the iterate is the least fixpoint of
   `cap ∘ f` from `never`, which does not depend on the member the iteration is entered from. `cap` is the
   `types` crate's own generalization (a union past `MAX_UNION_MEMBERS` generalizes by the chain in `union.rs`,
   and the result is a fixed point), the shape-field cap, and two caps that do not exist yet, a nesting depth and
   a generic-argument depth. A literal that stays the same across iterations is kept (`'done'` stays `'done'`),
   one that
   keeps changing (`$n + 1`) is bounded by the union cap and generalizes to its kind there. A legitimate chain
   therefore takes up to about 66 iterations (64 members, then the generalization), so the safety net, when
   `cycle.iteration()` reaches K, returns the position's top type and is an internal error in a debug build, sits
   just under salsa's cap of 200: it is never expected to fire, and salsa's own cap is never relied on. Decision
   17's wording is "the iterate is the least fixpoint of the capped operator", with the same intent (an
   inferred return keeps the literals its body returns). S-6 adds the nested-wrapper fixture, a mutual-recursion
   fixture entered from each member, and a `$n + 1` fixture whose iteration count it records: if the cost of
   about 66 iterations per member is too high, a lower per-kind literal threshold inside the cycle operator is the
   fallback, which is a function of the value as well.

Any other cycle is a bug. In a debug build it panics with the query stack; in a release build the unit
reports an internal error, as a panic does.

**A cycle is homogeneous.** salsa lets the query that is entered first decide: the cycle head's strategy
(`cycle.rs`, "the cycle head is responsible for managing iteration") either iterates the whole cycle to a fixed
point or gives every member with a fallback its fallback. Entered from another query, a cycle of mixed strategies
could resolve differently, which H2.1's determinism rule forbids. So every cycle has one strategy: the hierarchy has
none (the pass above), constants and purity are `cycle_result`, inference is `Fixpoint`, and an edge from one family
into another may not close a cycle. REG-17.4 is the row that threatens this (H3.8).

### H3.4 Candidates

`declarations_named` returns every declaration the name may bind to. The `Provider` turns that into a
`ClassView`: `One(&ClassFinal)` for the usual name, `Many` for a guarded polyfill or a duplicate. Every accessor on
`ClassView` has the `Many` semantic defined once, in the combinators decision 6 gives a builtin's readings:
an argument is checked against each candidate, a return or a property's type is their union, and a member that
only some candidates have is available in part (reported unless a guard covers it); the relation reads such a
class as each candidate, an ancestor every candidate has being certain and one that only some have possible.
`class_ancestors` carries that certainty. No level query handles `Many`; the levels are per declaration.

### H3.5 What the rows become

| row | becomes |
|---|---|
| REG-7.1 to 7.5, 7.12 | the queries above |
| REG-7.6 | `class_constant_value`, `global_constant` with `cycle_result` |
| REG-7.7 | nothing is built as scratch: `ClassDecl` carries `method_tags`, `trait_aliases`, `resolved_trait_conflicts` and `type_imports` as declared; the rules that read them read `class_decl` |
| REG-7.8 | `Session::load` demands every file's `file_decls` and `decl_index`; the whole-program passes are queries; `index_and_flatten` is deleted |
| REG-7.9 | H2.8 and H3.3 |
| REG-7.10 | `Provider`'s whole-registry iterations over `decl_index`, provenance and level 1 |
| REG-7.11 | by construction: a trait's members are read from its current declaration every revision |
| REG-8.1 | input writes: `FileSet`, `File.role`, and in W2 `file_decls` as an input (H3.6); collision policy is `declarations_named` plus candidates |
| REG-8.2 | superseded by the recorded reads |
| REG-8.3 | `file_decls`; `DuplicateClassSites` and `DuplicateFunctionSites` become a query over `decl_index` grouping by exact spelling |
| REG-8.4 | `Provider`, which a database with no corpus cannot make; no `ReflectionBuilder` exists to be split off |
| REG-8.5 | the checker reads its file's declaration from `class_decl` and `file_decls`; `redeclared_methods` goes |
| REG-8.6 | H1.3 |
| REG-8.7 | the engine's closure gate: a random edit sequence applied through `apply` equals a database built fresh on the final file set, compared by `registry-dump` and by every `Provider` answer for every name; plus, during W2, the engine against the old passes (H3.6) |
| REG-9 | each staleness case is a test of the closure gate; the fix is the construction |
| REG-11 | behaviour fixes, each its own delta commit in the level or the declaration builder (`class_decl`) whose logic it changes, after step 3 of H3.6: 11.1 on PROD-5.1's vectors, 11.15 on the `Provider`'s folded keys, 11.16 on the entity-index queries (H3.7); 11.14 is superseded |
| REG-17.1 to 17.4 | the queries above the levels, H3.8 |
| REG-10 | a `TestProject` builder (inline files into a database with the corpus) replaces `Registry::build`'s 188 tests' setup; `Registry::build` and `Origin::Unknown` are deleted |
| REG-16.5 | salsa's event callback (`WillExecute`, `DidValidateMemoizedValue`, `WillBlockOn`; `event.rs`) counts executions and validations by query; the *asker* of a query is not in the event, so the provider records the first reader of each name itself (S-3) |

### H3.5a REG-7.5's combination at the final level

`reconcile_with_native` (`decide.rs:13`) is the build-time combination of a PHPDoc type with its native type.
Decision 8's rule replaces it at the final level: REG-7.5's combination reads the class hierarchy, where `decide.rs:211`
trusts a class member unjudged because the registry is still being built. Its callers are the passes that REG-7.2
and 7.5 turn into levels (`function_sig/type_resolution.rs:157`, and `resolve.rs:1274`, `:1305`, `:1340`). Making it
allocation-free now would be work on code W2 replaces, for 41,085 allocations.
The replacement is written as follows: (1) the members are a borrowed slice,
the union's own (`self.repr()`) or `std::slice::from_ref(&self)`, with no `vec![self]` and no `into_repr`, which
copies the member list while the type is shared (`lib.rs:337`); (2) the branches that answer without building are
the doc type inside the native type (the doc type), the native type inside the doc type (the native type) and the
disjoint pair (the native type), each with no `kept`; (3) every other case builds its result once. Decision 8's rule
also *intersects* a PHPDoc member that shares values with a native kind (`callable(int): string` on `Closure` is
`Closure(int): string`, `T` on `Foo` is `T&Foo`) and keeps a native class that no member covers, which today's
function never adds back; those answers are built, not chosen, and they differ from today's, so the existing
`reconcile_with_native` cases in `registry/tests/function_signatures.rs` and `phpdoc_inheritance.rs` that decision 8
keeps carry over as the new function's first tests, one for each branch, and the ones it changes are rewritten as deltas.

### H3.6 The order inside W2, and what `file_decls` is until W3

W2 puts the registry on the engine before the pipeline is on it, so in W2 `file_decls(File)` is an **input**: the
existing loader parses a file and builds its declarations as today, and `apply(FileDelta)` sets them (REG-8.1's
`FileDelta` is exactly that). W3 turns it into a query over `parse`. Everything above it, `decl_index`, the levels,
the provider, does not change between the two. During W2 the old checker driver still decides what to recheck
(`walk_affected` and the dependents indexes stay until REG-16.4), reading the registry through the `Provider`
outside any query. In W3 `check_file` becomes a tracked query and the recording starts.

Within W2, each step has the same shape (a "closure gate" per step), **run with the cadence of your rule: the tests,
the fixtures and the in-binary comparison per commit, and the `registry-dump` and O2 comparisons once, at W2's stage
checkpoint** (the dumps are produced where they are cheap and compared there):
1. **REG-4.2 and REG-4.12** on the old storage: exact-spelling accessors, folded keys on `FoldedName`, the
   `Provider` facade over the old maps. `registry-dump` before and after is equal except REG-4.2's approved delta.
2. **The engine module, shadowing.** Inputs, `decl_index`, `declarations_named`; the `Provider` still answers from
   the old maps and, in a debug build, compares each answer with the engine's (a listed exception to #13's
   policy, deleted with step 3's last level).
3. **One level at a time** in the order 1, 2 (with REG-7.6 and its needs, REG-18.2 and 18.3), 3, 4, 5, then
   REG-7.12 and REG-7.10: the `Provider` flips that level's answers to the engine, the old pass stays as the debug
   oracle for one stage, the in-binary comparison runs over Piwigo, the vendor projects and the fixtures in the
   tests, and the old pass is deleted in the next commit. Deltas (VAL-2.9, 2.10) are read as they land.
4. **REG-8's close:** `apply` for every edit path, the closure gate, REG-9's tests, REG-10's builder.
5. **The data-model rows that wait for the levels.** REG-4.15 (a declaration built straight into its final form,
   with an ordinal where it had a span: H1.5a) is level 2's construction, so it lands with step 3's level 2; REG-4.11
   (provenance as the only location record) lands after REG-8.5 and REG-10 have removed `is_stub`'s and
   `Origin::Unknown`'s readers.
6. **REG-11's behaviour fixes,** one commit each after the structural ones, each a delta with its fixture first.
7. **REG-12's gate:** the gated steps' medians, and the test that checking one file resolves no class the file does
   not reach (a count from the event callback, not a guess).

### H3.7 What is not a level

**Global variables.** `infer_global_variables_of` (`registry/global_vars.rs:323`) is not a fixed point, though it
is run again per stale file. A file's sites are a function of its `GlobalFacts` and the registry: a `StaticValue`
names a call, a static call, a class constant or a method call, never another global, and
`GlobalFacts::resolve` turns it into a type through the registry (run per file, in parallel). The merge then applies
the sites of the files that `resolve_global_scope_files` accepts (an include graph matched by path suffix) to one
accumulator per name. Nothing in a site reads another global, so **there is no cycle before REG-17.1**; once
REG-17.1 makes an undeclared return the body's inference, a site `$x = f();` resolves through
`effective_return(f)`, whose body can read a global (`function get() { global $cache; return $cache; }` with
`$cache = get();`), and `global_sites(F)` → `effective_return(f)` → `inferred_return(f)` →
`global_variable(cache)` → `global_sites(F)` closes. So the global queries and `effective_return` and
`effective_property` belong to the inference family (H3.8): `Fixpoint`, `cycle_initial` = `never`, the same
widening. Before REG-17.1 they keep the default strategy, since there is no cycle to enter. On the engine:

| query | key | result |
|---|---|---|
| `file_products.globals` | `File` | the file's `GlobalFacts` (already serializable), made by the products pass; the include sites are a field of their own, since they depend on no type |
| `global_sites` | `File` | `FileSites`: the facts resolved through the `Provider`, so a changed return type or class constant reruns exactly the files whose sites call it |
| `global_scope_files` | none | the files whose top level runs in global scope, from the include sites and the `File` paths |
| `global_site_index` | none | each name to the files that have a site for it |
| `global_variable` | `GlobalKey` | one name's accumulator `finish`ed: the sites of the files the index lists that the scope set accepts |

The `Provider` exposes `global_variable(&Name)`, so a file that reads one global depends on that global's entry
alone. The `stale` sets, `global_sites` as registry state and the staleness bookkeeping go. The crate split's C9
moves the code out of reflection and J2's SUM-2 changes what counts as a site; neither changes these queries.
The shape above is a fixture of S-6, entered from each of its queries, on one thread and eight.

**Queries with no state of their own.** The doctrine entity index, `names_with_assert_tags`, the PHPStan ancestry
fact and `phpstan_class_files` are queries over `decl_index` and provenance (REG-18.6 makes the last a provenance
query). The Pest configuration and the SQL context are inputs to the check, as `pest_config` and `sql` are
fields of `Project` today.

### H3.8 What REG-17 adds above the levels

REG-17.1 to 17.4 put body inference under the types a caller sees, and a reflection that read inference would make
the levels cyclic (`class_final(C)` reads the inference of `C::m`, which reads `class_final(C)`). So:

- **The levels never read inference.** `class_final` is a function of declarations, as H3.1 has it. Above it sit the
  *effective* queries: `effective_return(DeclKey, member)` (the declared return, else REG-17.1's inferred one, and
  for a context declaration REG-17.3's widening of a refinement its own code refutes) and `effective_property(DeclKey,
  name)`, and `inferred_throws` (REG-17.2). The `Provider`'s method and property views take the signature from
  `class_member` and the return and property type from these. A file that calls `C::m()` therefore depends on
  `m`'s body only when `m`'s return is undeclared or refinable, and a declared one backdates at once.
- **Families.** Inference (`infer_body`, `inferred_return`, `inferred_throws`, `effective_return`,
  `effective_property`, the global queries of H3.7, and the receiver types of REG-17.4) is one family with
  `Fixpoint`; `infer_body`, `effective_return` and `effective_property` are rows of H2.3's table. Purity and
  property writes are another with `cycle_result`. REG-17.4 has a summary read a receiver's type from the body's
  inference, and the inference of a call read the callee's purity (it forgets what the callee may change).
  `purity(f)` → `infer_body(f)` → `purity(f)` would be a mixed cycle, and so would `purity(f)` →
  `infer_body(f)` → `purity(g)` → `infer_body(g)` → `purity(f)` for mutual recursion: salsa records
  dependencies at query grain, so reading `infer_body(f)` "for receiver types alone" depends on its whole
  execution, including its reads of callees' purity. **So the staging is a query, not a hope:**
  `receiver_types(DeclKey, member)` runs the inference in a no-effects mode and may not read `purity` or
  `property_writes`; `purity` and `property_writes` read it; `infer_body` reads `purity`, `property_writes`
  and `receiver_types`: a DAG by construction. Which receivers stage is stated, because soundness needs it: for
  `$this->a->b->m()` the receiver's type depends on what earlier calls may have forgotten
  (`forget_members_a_call_may_change`), so an effect-free typing could follow a call to the wrong body and claim
  purity; only receivers no call can change (locals not taken by reference, parameters, `$this` itself) stage,
  and every other construct keeps the conservative answer it has today (impure, writes anything). S-6 tests it:
  the same results whichever query of a cyclic fixture is entered first, and on one thread and eight.
- **What it costs.** `effective_*` are per member and asked per call site; the effective return of a member with a
  fully declared return is one lookup of `class_member`'s return and no inference.

## The pipeline, persistence and the Session (H4)

REG-16.4, CACHE-13.2 and the crate split's F1 and F2 are one design. Building F1 and then REG-16.4 would build the
Session twice, so what follows is one design. The rows it builds are W3's (REG-16.4, CACHE-13.1, CACHE-13.2, STUB-7.5,
LOAD-3.1, among others); the rows it names beyond W3 are owned where they are built.

### H4.1 The batch run

1. `Session::resolve(config)`: the database (the corpus `Arc` as its constant, the pool), the inputs (cold: all
   created; warm: restored first, H4.6), the settings inputs, `Packages` from the Composer load (vendor discovery
   still runs beside the manifest load, LOAD-9). Until STUB-11 embeds it the corpus is built beside the walk and
   the products (STUB-3), before the first query.
2. The walk (LOAD-2, as built) numbers each path and makes its `File`; `FileSet` is set once. **The stamp is new
   work** (CACHE-13.1's, W3 step 5): the walk takes no `stat` today (`Listed` holds path, role, stub flag and
   owner), so the loader takes the stamp from the open descriptor, before the read, for the files it reads anyway;
   a lazily reached file has none (H2.2's `file_stamp`). Steps 1 and 2 overlap as they do now (LOAD-8.10, 65 ms warm): the analysed and
   stub walks start at once, vendor discovery runs beside them, the context listing starts when discovery ends, and
   `Packages` and `FileSet` are written when all of them are done; no query runs before that. LOAD-2.4 hands each
   listed directory's files to the pool while the walk goes on, and a write per directory would cancel the queries
   running, so that overlap is kept outside the queries:
   the pool runs the pure function `products_of(bytes, role)`, and its results wait in the load's cache keyed by
   (path, stamp): a query that had to know the digest to look an entry up would read and hash the bytes the pool
   already read. `file_products(file)` finds its entry there, or calls the function, so it stays a function of the
   stamp it read. Whether the overlap matters is S-3's to measure (LOAD-2.4's wall effect was never measured), by
   S-3's stated rule; otherwise the plain order, walk and then products, stands. Input creation is confined to the driver by a private constructor and an `xtask arch`
   rule (salsa does not enforce that a query may not create an input).
3. Load: `file_products(file)` for every file of the **eager set** (the analysed files, scan and stub paths, and
   Composer's `files` entries; functions and constants cannot be autoloaded), in parallel on the pool, each file
   read once and parsed once, its products made: an analysed file's tree is its `parse_analysed` memo, kept for the
   check, and a context file's is built in the worker's scratch builder and cleared (H2.9). Then `decl_index`.
   Context *classes* are not in the eager set:
   - Vendor discovery lists the autoload directories' files as it does today and each gets a `File` (a query may
     not create an input, H2.2, and a located path needs one), but none is read or parsed until a name asks for it.
     When no unguarded eager declaration answers a name, `declarations_named` asks `locate(ClassKey)` and reads the
     located file's products. A guarded eager declaration is a candidate beside the located file (decision 11).
   - `locate` is Composer's `ClassLoader::findFile` over `Packages` and `FileSet`: the classmap entry; else the
     longest PSR-4 prefix and the first of its directories that holds the file; else PSR-0 (STUB-7.5; decision 11's
     "an autoloaded class in the file Composer's loader maps it to"). The directory test is a lookup in `FileSet`,
     so no filesystem read. The index is keyed through the root's case rule (the cross-platform plan's R7, probed once per root and
     exact on Linux), so a candidate spelled in another case than the disk's finds its file where the volume folds
     case; where the rule's fold is not the volume's the lookup asks the system as discovery does (R9) through
     the tracked `resolve(PathKey) -> Option<resolved spelling>` of H4.5, whose answer is then looked up in
     `FileSet`'s index (a boolean probe yields no `File`). A candidate spelled through a link below an
     autoload directory, which Composer's `file_exists` finds, is not an index entry (LOAD-1.1 resolves each link
     once and `FileSet` holds resolved paths, where eager loading lists those files): the remaining misses are
     decision 14's delta and S-11 tests them with a link below a PSR-4 directory. **Unicode normalisation:** APFS
     and HFS+ look names up insensitively to normalisation while listing the stored spelling, so R7's probe is
     extended with a normalisation test (an entry with a non-ASCII letter looked up in its other normalisation),
     `FileSet` and `shown` are keyed through the rule, and NFC and NFD names are tested on the macOS leg.
   - This is the crate split's "context classes are read on demand through the locators" and the larger win of the
     engine for a big `vendor/`: a vendor file nothing reaches is never read or parsed. **W3 builds it,**
     so the loader is not written eagerly first and rewritten at W4, and STUB-7.5 is in W3.
   - Its prerequisite is not built: the generated-file reader keeps paths only (`GeneratedEntries` drops the
     classmap's class names and the PSR prefixes) and `Autoload` has no `psr-0`, so the maps first keep their keys
     and gain PSR-0. That addition to `composer` is A7, ahead of W3: it changes no output, and is tested against the
     generated files' own arrays on `php` (the lookup that reads them is `locate`, W3's, tested against
     `ClassLoader::findFile`).
   - The delta (a vendor class that no autoload mapping reaches becomes unknown) is decision 14's, and **if S-11
     fails for a package, that case is a proposed change to decision 14, and the eager fallback
     for that package waits for that decision.** A lazily reached context file is stat-ed by `file_stamp`
     (H2.2). S-11 measures the listing and the read set.
4. Check: the analysed files, largest first, in parallel on the pool: `file_diagnostics(file)` runs `check_file`;
   levels resolve on demand on whichever thread asks first.
5. The whole-program queries, then `analysis(db)`: the finalizer.
6. With persistence on, the database is serialized with no handle alive, and only if something changed (H4.6).

What this removes from `Session::load`: the serial `insert_numbered_file` merge (the declaration index replaces
it), `finish_loading`'s passes (levels on demand), `complete_skipped_references`, and the file table's per-file
bookkeeping that is now `File` fields. The serial stretches the fixes plan measured (the finishing passes,
1.16 to 1.52 s of the off run at 2 to 3 of 8 cores) become the `decl_index` pass plus on-demand work spread over
the pool: REG-7's wall-time prize.

### H4.2 The server

The initial load is the batch load. After it: `didOpen` and `didChange` set the overlay in the `FileSource` and
bump the file's `stamp` (a nonce, which is never a disk stamp); `didClose` removes the overlay and stats the file;
watched-file events stamp, create or drop `File`s and set `FileSet`, **one notification being one batch** (the
stamps, creations and removals are collected, `FileSet` and its indexes are rebuilt once, each input is written
once, and queries restart once; a test creates 30,000 files in one notification). A configuration change writes
the settings inputs. **`reconfigure` runs on the full trigger set** of the crate split's F1, each with the
inputs it writes: a changed `scan_paths`, `exclude_paths`, `stub_paths` or `no_composer` (roles and the eager set
change: `FileSet`, `Packages`); a change to `composer.json`, `composer.lock` or `vendor/composer/installed.json`
and a regenerated `vendor/composer/autoload_*.php` (discovery reads them: `Packages`; with `locate` lazy a stale
`Packages` gives wrong lookups); a change to the baseline or a file it includes (`OutputInputs`); and the
watcher registrations are extended to those files (the watcher registers `**/*.php` only today, so they are
unwatched), so "reload the window" is gone. Then H2.6: `file_diagnostics` in priority order, published as each
completes and only when it differs.

### H4.3 F1, reconciled

| F1 item | on the engine |
|---|---|
| `AnalysisResult` owns its data | `analysis(db)`'s value: file and not-file errors, internal errors (the records `check_file` catches at its own boundary: H2.8; a file holding one is not persisted), stats, warnings, the inline-ignore records (from `file_products.ignores`, so a persisted result finalizes without parsing its file); "N more file(s) loaded for context" counts the files the load listed, a function of the inputs |
| the finalizer | a function of `file_diagnostics`, the whole-program results and `OutputInputs`; stale baseline and unmatched-ignore entries are its output |
| entry points | `Session::run`, `update(FileDelta, &CancellationToken)`, `reconfigure(Config)`, `generate_baseline`, `apply_fixes`, `snapshot()` (a database handle for one request); `Project`'s ~15 entry points collapse into them |
| `Project` folds in | the file table is `File` and `FileSet` plus the VFS; the registry is `Provider` over the engine; `dependents` is gone; `use_sites` and the escape sets are `file_products.use_sites` and a project query; `pest_config` is a query; `sql` is an input |
| the VFS | contents from `FileSource` (overlay first); membership decided once, by `classify` (LOAD-1.2, built), for the walk, a file the editor opens and a watched-file event alike; identity is the canonical path (LOAD-1.1, built; one canonical form on every platform, the cross-platform plan's R3); URIs are the cross-platform plan's R5 (RFC 8089 on each platform, an unopened file named in the spelling its client gave the folder), lines and file events F1's text unchanged |
| panic isolation on the pool | H2.8 |
| cancellation and the event loop | F1 has requests run "against the last published snapshot while an update builds the next". salsa has one current revision: a write cancels the other handles' queries. A request is answered from its handle, and a request cancelled by a write answers `ContentModified` (or is run again by the loop) — rust-analyzer's model. F1's "publish the edited file first, then the open documents, then the rest" is H2.6's order of queries |
| last good AST | **gone with recovery.** The engine needs `parse` to be infallible, so REG-16.4 needs SYN-1.9 (batch 3), which the sequence already has first |
| `reconfigure` | settings, `FileSet` and `Packages` writes (H4.2) |
| one Session per root | one `ProjectDb` per root, sharing the pool, the interner and the corpus `Arc` (each database's corpus constant is the same `Arc`) |
| files outside every folder, untitled buffers | a database rooted at the nearest config or `composer.json`, else for the file alone; an overlay-only file has a `File` with `overlay` and no stat |
| the workspace load runs as the CLI's | `Session::load` (H4.1) |
| LSP parity | the project-level rules are queries, so they run on every update by construction, their per-file data kept as products |

### H4.4 F2, reconciled

| F2 item | on the engine |
|---|---|
| resolved dependencies, absent names included, includes, trait hosts | the recorded reads: an absent name is a `declarations_named` read; an included file is a `file_products` read of it; a trait's host reads the trait's levels. `deps.rs`, `Project::dependents` and `signature_dependents`, `register_references`, `transitive_affected_files*` and `bare_name` are deleted, and with them the whole of PROD-5.4, PROD-5.7 and LOAD-4.7's file-references half (#35) |
| fingerprints per exported name | early cutoff on `class_final`, `function_final` and the constants, and per member on `class_member` (H2.4): a "fingerprint" is the equality of the value a reader read |
| incremental collectors | per-file facts as `file_products` fields and queries, and the whole-program queries over them |
| the persistent cache per analysed file | the database's memos: `FileCheck` (raw errors before the finalizer), the products, the levels, the verdicts, with the dependency graph |
| the key | the file is named by (build identity, project root, config key). The config key hashes `ReflectionInputs` and `CheckInputs`; these are *also* inputs, so a change inside a session invalidates exactly its readers; `OutputInputs` are in neither. Alternating configurations keep their own files (F2's entry sets), the four most recently used kept |
| location, mode `0700`, `CACHEDIR.TAG`, atomic write under a lock, `--no-result-cache`, `clear-result-cache` | unchanged in what they ask, and built for every platform (the cross-platform plan's R11): the directory is `dirs::cache_dir()` joined with `phpstan-rs` (`PHPSTAN_RS_CACHE_DIR` overrides), so `~/Library/Caches` and `%LOCALAPPDATA%` as well as `$XDG_CACHE_HOME`; mode `0700` is Unix's, and a Windows directory under `%LOCALAPPDATA%` is the user's by inheritance, with ownership and not-a-link checks before an existing directory is read or written; the write is a temporary file renamed over the target, as the two caches do today, which replaces an existing file on Windows too unless another process holds it open, so a reader takes no lock and sees the old file or the new; **the lock is a dedicated `.lock` file** (`File::lock`, portable; the data file is never the lock target, since Windows' lock is mandatory against reads by other handles), taken by the writer around the temporary write and the rename, and a lock failure runs the batch uncached with one warning; retention is the four most recently used config keys per project root (F2's entry sets) under a byte cap (`PHPSTAN_RS_RESULT_CACHE_MB`, default 2048 as the file cache's), last use refreshed on a read at most every 12 hours (`REFRESH_AFTER`); the per-file cache (`file_cache.rs`) and its switches keep serving until CACHE-13.2 replaces them and then go, with `--no-result-cache` taking their place; a result file made by one platform's build is never read by another's, since the build is part of its key, and whether `debug_assertions` belongs in the key is decided in the commit that writes it |
| the LSP writes back only results whose overlay matches the disk | a result whose reads include an overlay's stamp is dropped from the write (H4.6), since an overlay's stamp is never a disk stamp; the server serializes at shutdown or after an idle period, not "at any quiet moment" |

### H4.5 Probes outside the file set

`include_target_exists` (`checker/includes.rs:283`) calls `is_file()` on the live filesystem. A query may not,
and a query that reported an untracked read would run again every revision, which for the many files that
`include` something would cancel the point of a warm run (a minority: 242 of Piwigo's 2,798 non-vendor files name
`include` or `require` by a loose count). The probe is a query: `path_is_file(db, IncluderKey, PathKey)`, a
tracked function that stats the path, **keyed by the includer** and reading its stamp, over a singleton input
`ProbeEpoch` that the driver bumps when the filesystem may have changed: once when a persisted database is
restored, on every watched-file event of any extension (the watcher registers `**/*.php` only, so a created
include target of another extension, or outside the watched folders, would move no epoch: watchers are
registered for the directories probed), and on `didSave`. Keying by the includer means an edit to the includer
re-probes its own paths, as every recheck of an includer calls `is_file` afresh today (creating the target and
then editing the includer clears `include.fileNotFound`). Each probe re-stats when the epoch moves, and an
unchanged answer stops there. The probes' paths are interned keys, so a restored database knows which to ask
again. `resolve(PathKey) -> Option<resolved spelling>` is the same design for `locate`'s fallback (H4.1). S-10
counts the distinct probes on the corpora, requires zero stat calls without a bump and a stated ms per bump.

### H4.6 Persistence

- **Format:** salsa's `persistence` feature, with the heavy values `Lazy`, in one whole in-memory buffer (salsa
  reads each memo key as a borrowed `&str`, which needs it; the project's `postcard` is such a format) inside an
  envelope of magic, format version, length and checksum, as `file_cache.rs` has. A tracked function's memo is
  written by its result type's own serde impls (`persist` on a tracked function takes no functions of its own; on
  an input, an interned struct and a tracked struct it may, `salsa-macros/src/options.rs:318-333` with
  `tracked_fn.rs:66` against `input.rs:72`, `interned.rs:72`, `tracked_struct.rs:68`). So the levels' and the
  check's results are types whose `Serialize` writes the bytes `Lazy` already holds and whose `Deserialize` reads
  them back as bytes, decoded on first `get` (CACHE-12's `Lazy`, whose equality by bytes and file number is
  already what cutoff needs). Restore is then a copy plus the graph, not a decode of every value. S-4 decides
  whether this beats the cache's own container (CACHE-9 and CACHE-10): if restore is slower than today's
  per-file decode in wall time, the container carries the values and salsa carries only the graph.
- **Persisted: the whole chain below every persisted memo, down to the base inputs.** salsa does not flatten
  onto the persisted queries: it replaces a non-persisted dependency by the base-input leaves beneath it, and
  its nested walk descends through every edge and inserts only input fields and persisted collectable interned
  structs (H1.0 10). If `check_file` were persisted and `declarations_named`, `decl_index`, the levels,
  `class_member` and `path_is_file` were not, every restored memo would hold edges to every eager file's stamp,
  `FileSet` and `Packages` instead of to the values it read: cutoff would exist only in the session that built the
  memos, one changed stamp or one created file would rerun every `check_file` that read a name after a restore,
  and each memo's edge list would be the whole leaf set. So persisted are: **every input a persisted memo reads**
  (`File`, `FileSet`, the settings, `Packages`, `ProbeEpoch`, `Extensions`, `OutputInputs`, `SqlSchema`: salsa
  asserts at serialization that each such input is `persist` and panics otherwise, so an `xtask arch` rule says
  every salsa input is `persist`, and an engine test serializes a database after one `check_file` and one
  `analysis` with every singleton present); the interned keys; `file_digest`; `file_products`' fields;
  `decl_index`, `declarations_named` and `hierarchy_cycles`; the five levels and `class_member`;
  `function_final`; the constants; `path_is_file`, `file_stamp` and `sql_columns`; `check_file`, the verdicts,
  `file_diagnostics`; and the whole-program results, with `Lazy` values that do not duplicate shared `Arc`
  structure. **Not persisted:** the trees (`parse_analysed`; a context tree is no memo), the accessors' tables,
  and `Builtin`, which is a constant of the database (the process-wide corpus, H2.2) and so a persisted memo
  needs no input for it. The alternative, if the chain is too large (S-4 measures its bytes per memo and per
  edge), is to keep the dependency graph in our own file. A path is persisted as a Unix name's bytes
  (`OsStrExt::as_bytes` and `from_bytes`) or a Windows name's UTF-16 code units (`OsStrExt::encode_wide` and
  `OsStringExt::from_wide`), as one pair of functions added to `path_bytes` (which cuts components and has no
  such API today), not through serde's `PathBuf`, which refuses a name that is not UTF-8.
- **Restore: two paths, never mixed.** salsa deserializes into a database that has no input (a singleton that
  exists already panics, and input slots are re-allocated in the saved order under an assertion; a
  never-changing input cannot be set again), and `deserialize` changes the live database in place and is not
  atomic. *Cold:* `Session::resolve` creates every input. *Warm:* the file is read whole and its envelope
  verified before any byte reaches `deserialize`; `deserialize` runs under `catch_unwind` into a database made
  for the restore; then each singleton is fetched (`try_get`), those the file lacked are created, and the
  others are set only when they differ (an input's setter is a change whatever it is given). **On any error or
  panic that database is dropped, one warning is printed, the run is cold, and the file is replaced at the end;
  a database that saw a failed restore is never reused** (a truncated stream leaves salsa's ingredient `Vec`
  one element short, and an out-of-range index panics in `Vec::remove`; without this a corrupt file, one from a
  skewed build, or one written by another local user into a shared cache directory would fail every run the
  same way until someone cleared the cache). The driver then reconciles: each walked path finds its persisted
  `File` through the persisted path index, **keyed by the exact canonical spelling** (a case-only rename is a
  delete and a create; the index covers every `File`, dead ones included), and its stamp is set if the `stat`
  differs or the racily-clean rule (CACHE-13.1) distrusts it, and on Windows or a file system the rule does not
  know the file is read and hashed instead (decision 13); new paths get new `File`s; paths that are gone leave
  `FileSet`; the settings and `Packages` are compared; `ProbeEpoch` is bumped. A lazily reached context file's
  stamp is `file_stamp` (H2.2), so nothing is "the files a persisted memo names". Queries then verify, and run
  only where a read changed. Nothing the engine sorts is ordered by `FileId` (H2.3), so a restored database's
  numbering (old numbers kept, new paths given the next free one) cannot change a result.
- **Write only when something changed.** The database is serialized at the end of a batch run only if a query
  executed or an input changed since the restore (an event-callback counter), the report is printed before the
  write, and a no-change warm run rewrites nothing (the project's rule for its caches). `as_serialize` cancels
  other workers and waits for their handles, so the server serializes at shutdown or after an idle period,
  never while a request runs, and S-4 bounds serialize time. Memos derived from a dirty buffer are not written:
  an overlay's stamp is never a disk stamp, and a result whose reads include one is dropped from the write, not
  trusted (F2 writes back "the results of files whose overlay matches the disk; an overlay's results are never
  cached").
- **Compaction (#38):** interned keys and `File`s of things that no longer exist stay in
  a persisted database, and an immortal `DeclKey` also keeps the memos of a dead declaration. The rule is a size
  ratio: when the keys, dead `File`s included, outnumber the live declarations several times over, the database is
  rebuilt. *At the start of a run* that means the run starts cold and writes a fresh file. *In a server* it means an
  in-process rebuild, because a server that runs for days has no next start: a new database is loaded from the live
  file set at idle (cold, or restored from the last file written while the ratio was fine) and replaces the old one
  once it has caught up, and the old one is dropped with its last handle. S-4 picks the ratio and measures the
  rebuild from a series of edit-replay runs, and S-3 measures what an immortal key retains before it.
- **Sequence:** CACHE-13.1's stat stamps first (they feed `File.stamp`), then the persistence, with the cache
  closure gate: a sequence of edits applied through cached runs gives, at every step, a cold run's result; a
  warm run after no change equals the cold run and executes no `check_file` (counted from the event callback),
  **even when `ProbeEpoch` is bumped**; after a restore, one body edit and one file creation rerun exactly the
  `check_file` set the live session reruns; a panic-injected file is in the warm-run gate; and the restore is
  tested with a truncated file, a flipped byte behind a recomputed checksum, an out-of-range ingredient index
  and a file from another build key.

### H4.7 W3's order

1. The file source and `File` inputs are built (LOAD-1.4). `file_digest`, `parse_analysed` and `file_products` become
   queries, and `file_decls` flips from W2's input to a field (H3.6).
2. `check_file`, `file_diagnostics`, `property_type_verdicts`, the whole-program queries and `analysis`.
3. The server's update path moves to H2.6; `update_file*`, `walk_affected`, `closure_scope`, `dependents` go.
4. The batch driver (H4.1); `finish_loading` (94 calls, 116 mentions at `ffa7cccb`) and its callers become `Session`.
5. CACHE-13.1, then CACHE-13.2 and the cache closure gate.
6. TYPE-4 on the revision, REG-16.5, LOAD-3.1, LOAD-5.2, LOAD-5.1's use-site index, REG-17 and CHECK-7, CHECK-8.
7. At every step the edit replay (G5's tool 7) equals a fresh load.

### H4.8 Interim work the engine deletes

`deps::Collector`, `recorded_references`, `RefKey` and the dependents indexes exist only to decide which files to
recheck (`deps.rs` says so: "used purely to decide which files might need re-checking"). PROD-5.7 (references as
one vector per file), LOAD-4.7's file-references half and PROD-5.4's rewrite are work on them; REG-16.4 deletes the
machinery whole. PROD-5.8 (the record of written names) stays: its readers are rules, not the dependents index (#35).

## The designs of W4 to W7 (J1 to J3)

W4 to W7 are far from batch 4, and the code they build on will have changed by then, so these sections settle what can be settled now: the data model that must be fixed first (J1), the rows that make C6 to C9 tracked work (J2), and how Stages D to G meet the ledger and the engine (J3).

## J1. C5's data model on the engine (Part F 16)

Fixed already: decision 6 (gates as data, one parse per file, queries take a range), 7 (embed the builtin knowledge,
read PHPStan's classes from the project), 10 (what php-src verifies bounds a type; other sources are claims inside
the bound), 11 (a name binds to the declarations PHP may bind it to), 16 (failure values), 18 (7.0 onward). Missing,
and set here: the representation, where it sits on the engine, the generator's format and the order.

### J1.1 Representation

```rust
pub struct VersionSet(u32);      // bit i is KNOWN_PHP_VERSIONS[i], append-only; a PhpVersions range is its contiguous case (batch 3, the php-version crate: #32)
pub enum Availability { Always, Never, InPart }                  // of a VersionSet within a range (batch 3 as well)
pub struct Readings<T>(SmallVec<[(VersionSet, T); 1]>);          // a value by version set; disjoint; one entry nearly always (STUB-7.1)
```

**A range keeps its exact bounds, and its upper end is open when the constraint's is** (#40, COMP-2; this reads
decision 18's "to the newest version" as "and later" for the default range).
`PhpVersions` carries the range as version ids beside its minor hull: `lo` with its patch (`>=8.4.1` is 80401) and `hi`,
absent when the constraint has no upper bound (`>=8.4.1`, which 282 manifests in the corpora say) or none is declared, and
else the constraint's own (`^8.1` ends at 89999, `>=8.0.2, <8.0.15` is 80002 to 80014), which COMP-2's interval
arithmetic produces directly. The hull, the known minors the interval touches, stays what builtin readings and feature
checks read. Declaration guards read the ids: `GuardExpr::on` already works on `[min.id(), max.id() + 99]` and takes `lo`
and `hi` instead, so `PHP_VERSION_ID >= 80600` is in part for `>=8.4.1` and never only where `hi` excludes it, `< 90000` is
always for `^8.1` and in part for `>=8.1`, and `>= 80401` is always for `>=8.4.1`. A flag alone would lose what today's
hull gives, by accident, for `^8.1` (`< 90000` is always true there now, and for `>=8.1`, where it is wrong). The corpora
already hold such guards: Symfony's `polyfill-php86` (`bootstrap.php`, `SortDirection.php`) and Guzzle's `StreamHandler`
test `>= 80600`, and var-dumper's `SplCaster` and Composer's `PlatformRepository` test `< 90000`. The type of
`PHP_VERSION_ID` can be `int<lo, hi>`, where a range that spans several minors gets no range type for it now (the
consensus of its two ends differs). A builtin added in a later version is still unknown. `VersionSet` itself stays closed.

`Readings::over(range)` yields the entries whose set meets the range. A return type is the union of the readings over
the range; an argument is checked against each (decision 6). **The form is canonical:** all entries with equal
values are merged by union of their sets and the entries are ordered by lowest bit (a bit set has no "adjacent"),
so two `Readings` of equal meaning compare equal, and a `Readings` of types compares exactly (H1.5b). A builtin
declaration is
`{ avail: VersionSet, extension, signature: Readings<Signature>, claims, throws, purity, failure: Readings<Option<Failure>> }`;
a class adds `parents: Readings<..>` and members that carry their own `avail` and readings. Nothing in a record holds
a position (H1.5a); its provenance is `Origin::Builtin { file, extension }` and a member ordinal, which a stub file's
site table resolves to a place for go-to-definition into a stub.

### J1.2 On the engine

- The embedded records are **a constant of the database, not an input** (H2.2): an `Arc<BuiltinKnowledge>` with
  per-file `Lazy` records, probed by folded hash as `NameTable`'s `Base` is today, whose identity is covered by the
  build identity in a persisted file's key. `DeclFile::Builtin(index)` names the file in H1.4's `DeclRef`. Until
  STUB-11 embeds them the same constant is the corpus the run builds from the stub sources (STUB-7.6's layer with
  its per-file parse memo): lazily filled, and a pure function of the sources, so it is a cache and not state.
- **The range a file reads is its owner's** (`range_of`, H2.2; #42): `effective_range` is a
  query per owning manifest, the configured range, else the owner's `config.platform.php` or `require.php`, else
  7.0 to the newest version narrowed to the versions all the analysed files of that owner compile on (STUB-7.1),
  which reads their `facts.validity`. Only in that last case does an analysed file's mark reach the range; a
  configured range is `ReflectionInputs.range` as it stands and overrides every owner's. **Both exceptions of
  decisions 15 and 18 are specified.** A file loaded only through guarded includes (every load of it is an include
  that a guard covers, and no autoloader maps it) does not narrow the range and is not reported; that is not a
  property of the file, since it depends on the include graph and on guards evaluated over the very range being
  computed (`bootstrap.php`'s version check covering `bootstrap80.php`). So `effective_range` reads each analysed
  file's validity, its include sites and the guards covering them, computes the range with every guarded-include-only
  file left out, and then checks the guards against the result; a library whose analysed tree holds a PHP 8-only
  polyfill file therefore keeps its range, and G3.5 reports no construct in that file. When the files left share
  no version, the whole range is read and the conflict is reported once, as the empty-intersection case.
- STUB-13.1's phar locator is a locator in `declarations_named` beside Composer's (STUB-7.4: no locator's place
  makes a declaration win). A phar entry is a `File` whose bytes are a range of the phar's, read by
  `file_products` in its byte-reading path (a phar entry is a context file; `file_digest` is for analysed files),
  so the rest of the pipeline sees a file.
- `declarations_named` applies decision 11's binding: a builtin name is a candidate where its `Availability` over the
  reader's range is not `Never`; an unguarded project redeclaration of it is a candidate only where the builtin is not
  available (`InPart` keeps both); guarded polyfills, locator results and eager files are candidates by their own
  rules; no order picks a winner.
- A builtin declaration's levels are the same tracked functions as a project class's, keyed by its `DeclKey`; they
  read the declaration's `Availability` and the readings that meet the reader's range from the (never-changing)
  record. A level whose inputs differ across versions is also keyed by the range, normalized to the versions at
  which anything it reads changes (an interned `RangeKey`), so a class with no version-sensitive ancestor has one
  memo whatever the owner's range, which is the common case, and a project with one range pays nothing. A manifest
  edit that changes an owner's range reruns only what that owner's files read under the old range.
- The version range is an input (`Packages` per owning manifest, `ReflectionInputs` for the configured one) and
  not a copy of the stub layer per range (decision 6), and STUB-7.6's "one layer serves every range" is the
  database itself. S-3 counts the distinct ranges and the extra level memos on the multi-manifest corpora
  (Symfony, Laravel, rector-src), and the cost of a project with several ranges is stated there.

### J1.3 How the data is built (STUB-11 and STUB-12)

The build script of the data crate runs the code the runtime runs (`reflection`'s record builder) over the sources,
in decision 10's order: php-src's stubs from 8.0 (STUB-12.1: `phpstan/php-8-stubs` is a new vendored corpus, since
nothing in `vendor/` carries it today, and the native signature base from 8.0; `registry/stubs.rs`'s module doc,
which says php-src's stubs "turned out to be the wrong target", is corrected with it); real-binary dumps for 7.0 to
7.4 (STUB-12.2); phpstorm-stubs for the PECL extensions, its attributes read as data (STUB-12.3). Native readings
and the PHPDoc return type at func-info precision form the bound. PHPStan's function map and its deltas, the
`.stub` files and phpstorm's PHPDoc are claims intersected inside it; `@throws` and purity are united; templates
come from the `.stub` file; the function map's two `bleedingEdge` files have their "new" sections applied
unconditionally (never kept looser behind a flag: ROADMAP M4). **What the record holds:** the bound, the
build-time claims, and the combination of them as a cache. A project-dependent overlay (`stub_paths`, a
`StubFilesExtension`, the configured assertion tables, an extension's defaults) is combined at run time by the same
function over the bound and all the claims, with an extension's own inputs (ExtDs's version, which
`ExtDsStubFilesExtension` reads, is known at run time only) an input of that combination; so a run combines
nothing of the built-in sources, and REG-7.12 and STUB-11 agree on where each combination happens: at build time
for the built-in sources, at run time for the project-dependent ones. STUB-12.5's disagreement report is a
build-time check (`xtask stubs-check`), not run-time work. The test that the embedded records equal a runtime
build runs on each target; `**` is folded by a pure-Rust `pow`.

### J1.4 The generator (STUB-12.2)

`xtask stubs-dump` runs a PHP image of each version from 7.0 to the newest (the 8.x runs cross-check the php-src
stubs against a binary) by digest, with a script kept under `tools/php-stub-dump/`. **The image is built per version
with every in-tree extension enabled** (a Dockerfile under `tools/php-stub-dump/`, base image still by digest): a
stock official image builds only its default extensions, and `mysqli`, `intl`, `gd`, `bcmath`, `sockets`, `soap`,
`pcntl`, `gmp`, `zip` and the like need `docker-php-ext-install`; the dump's header lists which extensions it
contains. The script lists the internal
functions, classes, interfaces, traits and constants through Reflection: for each parameter its name, type (named or
union), by-reference, variadic, optional and default (the constant's name when the default is one); the return type;
the flags; for a class its parents, members with visibility, `static`, `final` and `abstract`, constants and
their types; and the extension. **What Reflection reports before 8.0 is unverified here** (no older PHP is installed,
and internal arginfo before 8.0 is believed to carry few parameter or return types and no default values), so a
spike precedes STUB-12.2: run the script on the 7.4 image and count the positions that have a reported type, a
return type and a default. Where Reflection has none, the bound for 7.0 to 7.4 is the probe's accepted and refused
argument kinds, or a claims-only declaration that STUB-12.5 reports as unverified, and `type` and `default` leave
the pre-8.0 format if they are not there. It then probes the failure value (decision 16) by the survey's procedure:
every function whose required parameters are by value is called with an object for each, outside `strict_types`,
and what it returns or throws is recorded (503 `null`, 186 `false`, 4 continuing, 100 throwing, at 7.4). **A
function with a required by-reference parameter is probed with a variable passed for it** (php 8.5.4 has 74 of
them among 1,934 internal functions: `openssl_x509_export`, `openssl_pkcs12_read` and the like, which "by value
only" would skip). The output is JSON text per version, sorted, under `crates/stub-sources/data/` and checked in
(text is source; "checked-in binary data is out", STUB-10.2), with the image digest, `PHP_VERSION` and the extension
list in its header. CI does not run it (it needs docker); `xtask stubs-dump --check` compares a fresh dump with the
committed one when docker is there (the comparison matrix has no such check, only regeneration commands per
image tag, `php:8.0-cli` and the like). The cross-check against the php-src stubs runs on every version from 8.0 to
the newest, not only 8.0.

### J1.5 CHECK-10, the engine read over the range

The checker already follows one precedent: `NextArrayIndex` and `PhpFeature::supports` return a `TrinaryLogic` when
the range straddles a version that changes the behaviour. CHECK-10 generalizes it into a table of version-sensitive
behaviours (a loose comparison of a number and a non-numeric string, a division by zero, arithmetic on a
non-numeric string), each with its thresholds and its semantics. **Each behaviour is a `Readings<Semantics>`,** the
same type as the stub data (J1.1), not a fixed pair: one family has more than two across a range (php 8.5.4:
`"abc" * 1` throws `TypeError` while `"5 apples" * 2` gives 10 with a warning, and 7.x behaves differently
again). `behaviour.at(range)` yields the semantics that meet the range; with more than one, the operation is
computed under each and the types are united, with "possible in part" for what a report says. The table is
data, tested against committed per-version output of a matrix regenerated by hand (`tools/php-comparison-matrix`),
and by the digest-pinned `xtask stubs-dump --check` when docker is there.

### J1.6 Order (W4, with the engine in place)

STUB-7.1 (the data model and one parse per file, with `Readings`; `VersionSet` and `Availability` are batch 3's, #32), 7.3 (the attributes as data: the stub-attribute pass and `resolve_names` go), 7.2 and 7.4 (the locators;
7.5, `locate`, is W3's: H4.1), 7.6 (the stub layer as the database's corpus constant), 7.7 and 7.8 (provenance and
enumeration queries), 7.9 and 7.10 (range queries), 7.12 (the gate), STUB-8 (guards), STUB-12.1 to 12.5 (the sources
and the check, behind the pre-8.0 spike of J1.4), CHECK-10's version table (its operator sites are D3's), STUB-13.1
(13.2 is W6's). STUB-11 (W6) embeds what STUB-12 builds.

## J2. C6 to C9 as tracked work (Part F 17)

REG-15.1 "needs C6 to C9 (not tracked here)". These are the rows to write into the ledger, with their needs, cut
against decisions 1 to 18 and the engine. Nothing here changes the plan's behaviour lists; they become each row's
body.

| row | what (the plan's C-phase) | needs |
|---|---|---|
| OVL-1 (C6) | the `.stub` PHPDoc overlays (30 files of `conf/config.neon`, 11 more by the six gated `StubFilesExtension`s: SocketSelect 2, ReflectionClass 4, ReflectionEnum 2, BcMath, JsonValidate and ExtDs 1 each, so 41 files of `stubs/`) as claims inside decision 10's bound; all the functions (143 by a tokenizer count at `ffa7cccb`) and all method PHPDoc apply, and reach an implementation's untyped parameters by PHPDoc inheritance (level 3) | STUB-12.1, STUB-7.1, REG-7.3 |
| OVL-2 | declarative patch entries replace the Rust patch functions of `registry/stubs.rs`; data files in `stub-sources`, read at run time by the stub layer (STUB-7.6), and by STUB-11's build script later (the same files) | STUB-7.6 |
| OVL-3 | the function map and its seven deltas by version: the `SignatureMapParser` port is STUB-12.4's (one row, with the parser replacing `patch_functionmap_array_shapes`); OVL-3 keeps the fixtures and the settle-by-decision-10 list: its signatures against phpstorm's PHPDoc settle by decision 10 (the plan's list of oracle misses, the curl handles, `printf`'s `Stringable`, `round()`'s `RoundingMode`, and the about 60 narrower parameters the function map and the overlay stubs give (`array_chunk()`'s `positive-int` length, `explode()` with `''`, `chr(256)`, `flock()`'s `int-mask`, the `Reflection*` constructors' `class-string`…), becomes fixtures). The native signature base is `phpstan/php-8-stubs` (8.0 and later) with the dumps below (J1.3), the two `bleedingEdge` files have their "new" sections applied unconditionally (ROADMAP M4), and `registry/stubs.rs`'s module doc is corrected | STUB-12.4 |
| OVL-4 | a function with several variants (a phpstorm overload, a function-map `'1` form) keeps them all; a call takes the variant its arguments fit, named ones included (`selectFromArgs`); a call that fits none is `arguments.count`. A new dimension beside version readings | REG-7.12, OVL-3 |
| OVL-5 | `resources/functionMetadata.php` as the builtin purity source | STUB-12 |
| OVL-6 | user `stub_paths` with PHPStan `stubFiles` semantics: a stub's PHPDoc refines the declaration's and never relocates it (decision 11's overlay); omitted members are kept; a stub declares a symbol only when nothing else does. Combined at run time with the bound and the built-in claims by the same function the build uses (J1.3) | STUB-7.4 |
| OVL-7 | `StubFilesExtension` (its files, and ExtDs's version, are a run-time input of the combination) | OVL-6 |
| OVL-8 | the assertion tables compiled to overlays; `apply_assertion_table` and level 2's interim assertion step deleted; the Mockery and Nette Tester defaults to their extension modules | OVL-6 |
| RES-1 (C7) | the `TypeNodeResolver`: one name scope (namespace and imports, active templates, aliases, `self`) for the registry and the checker, as an implementation of the scope `phpdoc::lower` is generic over (I4.1); an enclosing function's templates inside an anonymous class. ("PHPDoc keyword types resolve before namespace qualification" is done in I4.1's extraction; only `number`, `scalar` and `numeric` yielding to a class or alias of that name is here) | I4, PROD-5.2, DOC-2.2 |
| RES-2 | native `TypeHint` nodes resolved directly, not printed and re-parsed (about 85 sites) | RES-1 |
| RES-3 | PHPDoc array types united with iterable types, a template name read as a template wherever a type is expected (shape keys included), a generic with fewer arguments than templates filled as `TypeNodeResolver` does, defaults taken | RES-1 |
| RES-4 | a type alias defined through itself is `typeAlias.circular`: the engine's `cycle_result` on the alias query, per alias | RES-1 |
| RES-5 | a constant name in a PHPDoc type resolves through `use const` and its initializer | RES-1, REG-18.3 |
| RES-6 | the resolver's tests, one for each F40, F45, F46, F47, F51, F61 and F63 item of `STAGE_B_REVIEW.md` that the crate split's C7 lists; `types::parse` is already gone (I4) | RES-1 to RES-5 |
| RES-7 | `TypeNodeResolverExtension` (the extension point, registered through the `Extensions` input) and the three behaviours the crate split's C7 last bullet lists: assert targets `$param->prop`, `$param->method()` and `$this->method()` are kept, `@throws` and assert types get alias substitution, and a written `Traversable` resolves like any name, with the probes the crate split cites | RES-1 |
| EXT-1 (C8) | `Provider` implements `ClassHierarchy` as the tri-state relation (the trait takes `Name`s, lends memoized views instead of allocating, exposes `TemplateParam`s and gains the enum-cases query); the boolean `is_subclass_of` is deleted; an ancestor through an unresolved edge and a missing class answer `Maybe`; the 13 package sites and 26 in `types` (outside tests, at `ffa7cccb`) that call `Relation::structural()` switch, as a delta; the two project tests that pin a removed ancestor answer `Maybe` and are renamed through `gates/test-renames` | REG-7.1, REG-7.4 |
| EXT-2 | the `@property`, `@method` and `@mixin` implementors, and the built-in ones (universal object crates among them), replace level 2's tags and level 4's mixins; member lookup asks native members (own, traits, ancestors) first and these after, and a tag at the same or a closer hierarchy distance than the native member wins | REG-7.3, REG-7.4 |
| EXT-3 | C8's behaviour fixes, one commit each, fixtured: `@mixin` on a trait, a mixin never shadowing a native member, a mixin's inherited members, its own mixins and a used trait's (guarded per type and member against recursion), `@mixin Box<int>` binding `Box`'s templates and `@mixin T` mixing in the bound argument, a static mixin method under `__callStatic`, `mixin_exclude_classes`, a trait's tags reaching its hosts, a `@method` tag not a prototype, `@property-read`/`-write` as one property, enums reading `@method`/`@mixin`, `@phpstan-require-extends` on an interface | EXT-2 |
| EXT-4 | the class-reflection data extensions (`access_attributes`, `doctrine_entity`) through the trait; attribute arguments evaluate to types | REG-18.7, EXT-2 |
| SUM-1 (C9) | the summaries API: purity and global-variable inference leave reflection; `AnalysisContext` (the `Provider` and the summary accessors) is created here and completed by D6; `BodySource`, `FileBodies` and `NoBodies` move to the analyser | REG-7, H2.3 |
| SUM-2 | the global-variable fixes: sites include `foreach` bindings, destructuring, by-reference out arguments and appends, and expression typing keeps unkeyed array items, resolves call returns (templates, `static`, dynamic return types) and binds `self::`; a global's summary holds `null` and every value it can have, and `mixed` once a write the fold cannot see may reach it | SUM-1 |
| SUM-3 | the purity fixes: shell-exec and `throw` impure, a class-constant read pure, calls and `new` through parse-time names, method dependencies along inheritance, a static call's purity its method's, a user function judged by its body, a `pure-callable` parameter | SUM-1, OVL-5, REG-17.1 |

**On the engine.** Extensions are **an input**: an `Extensions` singleton holds the registered implementors and a
fingerprint (the set, each one's configuration and each one's `ResultCacheMeta`, the crate split's hook for an
extension's declared hidden inputs: a project-level file-set hook such as ext-pest's), which every query that calls
one reads first, so a changed extension or configuration invalidates exactly what called it and a persisted
database made under other extensions is not trusted. **An extension reads the world only through engine-provided
handles that record the read:** the file source, `FileSet`, a declared hidden-input fingerprint (its
`ResultCacheMeta`), and, for a live database, the `SqlSchema` and epoch input of H2.2 (the SQL extension's live
reflector `PREPARE`s per distinct text, which is the world and not a function of its arguments, and becomes the
epoch-keyed query `sql_columns` as `include_target_exists` becomes `path_is_file`, H4.5). It is given no other API:
no `std::fs`, environment or clock in an extension's crates, which `xtask arch` can check, because the harness's
double run (each registered extension run twice over the fixtures, failing on a difference) cannot see a stable
hidden input. With these rows REG-15.1's need reads "OVL, RES, EXT and SUM done".

## J3. Stages D to G, reconciled (Part F 18)

Read against the crate split plan's D1, D3 and D4, and against E1, E7 and G1, and the ledger's rows. The plan's text
stays the authority for what each phase does; this fixes how it meets the ledger, the engine and the designs above.

The rows for D5, D6, E2 to E6, F1 to F5 and G2 to G5 are the crate split's text as written.

| plan | ledger | resolution |
|---|---|---|
| D1 holder storage: every `Locals` map into holder storage, ExprKey interned, persistent maps, byte-identical | CHECK-6.3 (slots, persistent radix arrays), 6.13, 6.14; G1, G2 | D1 is built with CHECK-6.3's design (a variable's `LocalKey` is its slot; 32-way `Arc`-shared radix arrays, with the diff that skips shared subtrees) and the stamps of G2, and deletes the two alias maps of G1 (the only tree references in `Locals`) with their `Rooted` values. D1 states the contract ("interned, persistent, byte-identical") and not the structure; the ledger's measured structure fills it. CHECK-6.13 and 6.14 are D1's follow-ups. **The level-3 findings whose designs add per-variable state to `Locals`** (F5's `maybe_undefined` set, F6's closure-literal alias, F45's reference groups, F61's unset-property keys) are this change's by Appendix F's rule (a bug in code a later phase rewrites gets a fixture now and its fix as that phase's behaviour commit): their fixtures now, their fixes inside D1 and D5 on holder storage (the fix review's own first step is exactly this `ExpressionHolder`) |
| D2 statement results, contexts, StmtHandlers; termination, last conditions, constant conditions | CHECK-6.2 (`has_yield` and `collect_throws` go) | CHECK-6.2 is D2's `StatementResult`; **F69** (implicit calls as throw points) is D2's, and CHECK-9 follows it. D2's `syntax::annotations` (goto labels, last conditions, try/catch types, parent statement types, the closure-argument context, the pipe and magic-constant rewrites; and the kinds the crate split names besides: `DeclarePositionVisitor`, `ImmediatelyInvokedClosureVisitor`, `NewAssignedToPropertyVisitor`, `StandaloneThrowExprVisitor`) are side tables of the flat tree that the builder fills in a pass at `finish`, beside `goto_labels` (B3); B3 and I1 reserve the place and the tables are added to the layout at D2. An immutable tree is not rewritten, so `RemoveUnusedCodeByPhpVersionIdVisitor`, which in PHPStan rewrites a lone `if`/`else` on `PHP_VERSION_ID` to true or false by the configured version, is a mark that records the comparison (operator and id) and is evaluated per range by its reader, as `registry/guard.rs` keeps declaration guards symbolic (decision 6: one parse, no version in any key); and the pipe's `f($x)` form is a view over the pipe node |
| D3 one inference path: ExprHandlers, per-`NodeId` results, `infer_expr` deleted, `TypeSpecifier` | CHECK-6.1 (observers, one inference per node), 6.7 (argument binding once), 6.15 (the walks' vectors) | The "per-`NodeId` results" are the infer cache of G1.5 generalized: a table indexed by `ExprId` under the tree's serial, the only place a bare id is held in the checker |
| D4 `NodeCallback` sink, observers, result storage | CHECK-6.1's observer families | D4's interface is the observer API; the PHPStan-API walker and the statement rules are its first families (CHECK-6.1's text) |
| D5 semantic convergence, one approved delta each | none | as written; the nsrt and rules oracles of gate 12 are the measure |
| D6 engine APIs: Scope, public `TypeSpecifier`, returned-error `Rule`, derived nodes, Collector, Observer, extension registry, concurrency model, `AnalysisContext`, `InternalRule` deletion | CHECK-6.10, 4.1 | the returned-error `Rule` is I5's `Message`, `RuleError` and `RuleErrorBuilder`; `AnalysisContext` is J2's SUM-1; the extension registry is J2's `Extensions` input; the concurrency model is H2.7 |
| E1 collectors and project-level rules (reference sites, dead code, architecture, class shapes, circular namespaces, composer hygiene, mock hygiene, unmatched ignores) | LOAD-5.1 | **The facts are W3's, the collection E1's** (#39): LOAD-5.1's per-file facts are `file_products` fields built in W3, and E1 moves their collection onto D6's Collector API and adds its semantic fixes over those fields. The passes do not all land in E1: architecture, class shapes and Composer hygiene are E1 collectors; banned constructs are per-node rules in the rules walk (the crate split calls them "per-file checks, not project-level"); mock hygiene is ext-mockstan's (E4); unmatched ignores are the finalizer's (F1) |
| E2 builtins; E3 every diagnostic to `rules` and extension modules; E4 extension modules | none | as written, on D6's API |
| E5 levels: no rule above the level runs | — | `CheckInputs` holds the level; `Pending.level` and the finalizer's level stage go (I5, F1) |
| E6 observers for the LSP's presentation | G4 | hover, inlay hints, semantic tokens and completion read D4's per-node results instead of re-running the checker; G4b's node-at-position query is its first user |
| E7 checker decomposition (the checker's fields' exact sets: 88 field lines at `ffa7cccb`, where the crate split's Appendix C counts 74) | CHECK-6.8, 6.18 | the per-file, per-function and per-pass state sets are D1/D2's; "one checker per worker, reset per file" (CHECK-6.18) is buildable under G1 (a `Checker` takes no tree lifetime), and is E7's because its reset clears exactly the sets E7 enumerates |
| F1 Session, F2 result cache | REG-16.4, CACHE-13.2 | H4; F1's seven fixed-first commits are batch 0's (#22) |
| F3 formatters, F4 binary, F5 language features | none | as written; F5 reads D4's results |
| G1 extraction | — | the list gains `engine` (a module of the package until here, with its `crate engine` line between `analyser` and `project`, H3.1, #33); `php-version` is a crate already (batch 3, #32), so it belongs among G's "crates already", and the crate split's "26 new packages" count is corrected; Appendix A's module assignments and Appendix E's third-party rows gain `salsa`, `triomphe` and `wide` |

**Conventions every Stage D and E commit obeys:** tree-derived values are handles in walks and `Rooted` where one
must be stored, and no bare id is held outside `syntax`, the infer cache and the probe module (G1); **no value that
another file's query reads holds a position of a file other than its own** (a file's own diagnostics and per-node
results may), and an engine-compared value's equality is exact (H1.5); a message is a `Message` (I5); reflection is
read through the checker-facing trait that `Provider` implements (H2.4); an extension reads the world only through
engine handles that record the read (J2).

## Decisions

### #15 CACHE-13.1 for context files

**W3's, built once as `File.stamp`.** The stamp (the `stat` tuple, taken before the read, the racily-clean rule, the file-system test, the content hash where a change time cannot be trusted: decision 13) is permanent, and a per-project index file that stores it is not: the persisted database holds `File.stamp` and the memos it validates, and deletes the index. **The racily-clean rule is applied at the stat:** decision 13's reference time, the index's own write time, is git's because git writes its index at once, and here the database is written after the whole check; so each stamp records the clock reading of its stat and is trusted only when its mtime and ctime are older than that reading by a margin of at least the file system's timestamp granularity (decision 13's wording changes), and the file-system test is an explicit list of accepted types with the content hash as the default (the commit that names the system call and crate judges a new dependency on merit, and tests the three platform legs). MEAS-1 puts what the index replaces at about 60-70 M Ir of read wrappers and two hashes (`fs::read` 15.2 M, `File::open` 4.3 M, `crc32` 30.4 M, `xxh3` 17.1 M; the system calls and I/O are not in Ir and have not been timed), with the ≈1,260 M of per-file decode untouched until CACHE-13.2. The ledger gives analysed files to CACHE-13.2 already. Built at W3 step 5 before CACHE-13.2, the stamp has its own tests and the cache closure gate; the no-interim rule (25 G §10.7) of #35 and 41

*Needed before:* W3

### #16 item 18

**Reconcile, build once, as Stage D.** §12.10 ("PHPStan's `NodeScopeResolver`... record and replay") and D3 and D4 (`RecordingNodeCallback`, per-node results) are one architecture; D1 and CHECK-6.3 both specify the scope's storage; CHECK-6.2 is D2's `StatementResult`. The crate split builds the layers bottom-up, so the engine goes on the final reflection API, and the 25 G report's own no-interim rule (its §10.7) says not to build what the traversal replaces. So CHECK-6.1, 6.2, 6.3, 6.7, 6.13 to 6.15 (and the LOAD-5.1 passes that need CHECK-6.1) go to Stage D, reconciled first; batch 7 is the temporaries, with 6.10 and 4.1's message type designed as D6's `RuleErrorBuilder`. The cost: §12.10's ≈1.0-1.3 G and the slots' ≈0.15-0.25 G arrive with Stage D.

The split of item 18's groups stands as the plan gives it, and the inference core is validated by reading its diff family
by family.

*Needed before:* batch 7

**Facts the decision rests on.**

- §12.10 of the 25 G report and the crate split's D3 and D4 describe the same port of PHPStan's
  `NodeScopeResolver`, down to record and replay. The 25 G audits adopted a no-interim rule (its
  §10.7).

### #19 order of the wall

**The spine, then the pipeline as queries (REG-16.4, CACHE-13.2) designed as one thing with Stage F1 and F2** (F2's design is superseded by decision 3, and building both is building the Session twice), then C5, C6 to C9, C10, STUB-11 and 13, Stage D, E, F3 to F5, G

*Needed before:* the reevaluation

### #21 Stages D to G

**Build them, re-baselined:** they are the target architecture and the home of D5's correctness fixes. The re-read is #20's, not an alternative to building

*Needed before:* the reevaluation

### #28 LOAD-5.2 batch-only

**No: keep its needs.** A batch-only version makes the batch run and the server hold trees differently, which LOAD-1 and decision 3 removed

*Needed before:* W3

### #33 the engine crate and `unsafe_code`: salsa's macros emit `unsafe impl` into the crate that uses them, which the workspace's `deny` is thought to reject; allow it in `crates/engine` alone, gated, or build an engine of our own with no macro output

**Conditional on S-2, which builds the skeleton first under the workspace's `deny` with no allow.** The macros do emit `unsafe impl` (salsa's `HasStorage`, the `inventory` constructor), but rustc does not normally report most lints, `unsafe_code` among them, for tokens that come from another crate's macro expansion: a probe on the pinned toolchain (an external `macro_rules!` and an external attribute macro emitting `unsafe impl` and `unsafe` blocks, under `#![deny(unsafe_code)]`) compiled without a diagnostic, while the same code written in the crate was rejected. So the exception is probably unnecessary, and the engine keeps the workspace's two (PCRE, the allocator). Only if rustc reports the expansions of salsa's own macros does the engine's module get `allow(unsafe_code)`, with an `xtask arch` check holding its own text at zero `unsafe` tokens and `UNSAFE_PLAN.md`'s end state amended to "everywhere but PCRE, the allocator and the engine's generated code". **Until G1 the engine is a module of the `phpstan-rs` package, not a crate:** `reflection`, `analyser` and `project` are modules of it until C10 and G1, and a crate built at W2 could name none of the types it wraps; it takes a `crate engine` line (between `analyser` and `project`) and a `module engine engine` line in `gates/module-graph.txt`, and G1 extracts it

*Needed before:* W2

*Left as it is (#33):* probably no exception is needed at all; S-2 decides.

**Facts the decision rests on.**

- The workspace's `deny(unsafe_code)` very likely does not report the expansions of an external crate's macros (a
  probe on the pinned toolchain compiled an external `macro_rules!` and an external attribute macro that emit
  `unsafe` without a diagnostic); salsa's own expansion was not built, which is what S-2 does first.

### #36 the server's snapshot model: salsa cancels a request when a write arrives (the request answers `ContentModified`, or the loop runs it again), where F1 wanted requests to run against the last published snapshot while an update builds the next

**salsa's model**, rust-analyzer's. F1's model needs persistent structures through the whole engine, which is an engine of our own. The visible difference is a request cancelled by a keystroke, and a client retries a `ContentModified` answer only for the methods it lists in `general.staleRequestSupport.retryOnContentModified` (LSP 3.17; semantic tokens and inlay hints in common clients, not hover, completion or definition), so the server reads that capability at `initialize`, runs a request the client will not retry again on the new revision, and answers `ContentModified` only for the retryable ones (H2.8)

*Needed before:* W3

*Left as it is (#36):* what salsa's cancel model costs, which decision 3 already took.

### #37 the persisted database's key: one file per (build, root, config key) with the settings also inputs, or one per (build, root) with the settings only inputs, as CACHE-13.2's row words it

**Per config key, with the settings also inputs, and the config key leaves out the PHP version range.** One file would thrash when two configurations alternate (the editor beside a `--set level=9` run), which F2's entry sets exist to prevent; the inputs keep a change inside a session exact. Decision 6 says no cache key holds a version, so the range stays an input and a changed range reruns only what read it; what changes is CACHE-13.2's wording ("the configuration ... are inputs, not keys"), which the ledger edits of `01-overview.md` (Edits at approval) list. **The build identity** is `PHPSTAN_RS_SOURCE_HASH` plus the rustc version and a persistence-format constant (salsa orders its ingredients by type name, so the bytes follow the compiler's layout); F2's GNU build ID, executable size and modification time cannot work, since the project's builds write no build ID and size and mtime change on every relink

*Needed before:* W3

### #39 LOAD-5.1's passes (architecture, banned constructs, class shapes, Composer and mock hygiene, unmatched ignores) are the crate split's E1 collector rules: build them in batch 8 as per-file products, or as E1's collectors on D6's API

**Split by form: W3 builds the facts, E1 moves their collection.** The six passes' per-file facts are `file_products` fields built in W3 (H2.3 and H4.4 already read them, the persisted whole-program queries need them, and no analysed tree can be freed after its check while a pass still reads it), so LOAD-5.1 moves to W3 and LOAD-5.2 follows it and S-7's verdict. E1 adds the D6 Collector API and the semantic fixes over those fields, so the data is built once; the passes do not all land in E1: architecture, class shapes and Composer hygiene are E1 collectors, banned constructs are per-node rules in the rules walk (the crate split calls them per-file checks, not project-level), mock hygiene is ext-mockstan's (E4) and unmatched ignores are the finalizer's (F1). Built in batch 8 they would have been written twice; with them batch 8 empties (LOAD-5.4 is Part A's, CACHE-13.1 is W3's, CACHE-8 follows S-4)

*Needed before:* W3 (facts); W7 (collection)

### #41 rows whose code a later task replaces: REG-14.5 (`reconcile_with_native`, which REG-7.5's combination replaces), CACHE-8 (the cache `Writer::finish`, which serves the stub caches STUB-11 deletes and a persistence format that S-4 has not chosen), and CHECK-4.2 (the class-constant key, which REG-18.4 re-keys). Build each now for its small interim saving, or fold it into the task that replaces it

**Fold them.** REG-14.5 into REG-7.5 (H3.5a says how the replacement is written), CACHE-8 after S-4 (if the cache's container survives as the persistence's value format its writer is built once, right), CHECK-4.2 into REG-18.4 (batch 4b: it needs `ConstName`, which batch 4 makes, so it is not a filler of W1; its delta is one of #42's). Each saves little (REG-14.5 is 41,085 allocations; CACHE-8 is memory in a cold run only; CHECK-4.2 is 5.1 M Ir), and each would be written twice. REG-14.5 and CACHE-8 wait on the wall as #35 does (REG-7.5 is W2's, and S-4 settles the persistence), so they too come back as READY rows if the wall is dropped. The same rule as #35, and the same line: a type change that an end-state invariant needs is built (NAME-1.2); a new structure or a micro-optimisation of code that goes is not

*Needed before:* Part A; batch 8

### #42 deltas the ledger marks READY, or OPEN with half a ruling, that neither VAL-2.9 nor decision 14 approves: LOAD-7.6 (the config's keys: grouped keys named for the PHPStan parameters they cover, one casing, no key repeating its section's name, PHP versions and extension activation read from every discovered Composer root's manifest), REG-18.4 (a constant's namespace folds case at every lookup: `\APP\A` finds `App\A`, which is `constant.notFound` today), CHECK-11 (`property.unused`: ORM-mapped properties count as used while ext-doctrine is active, and view objects passed to a template renderer are not reported) and LOAD-7.2's PHPUnit half (whether PHPUnit's assertion narrowing and mock types are gated on ext-phpunit's activation, which nothing reads today, or stay a fact of the program, since a PHPUnit assertion throws whenever PHPUnit runs it)

**Each with a condition.** **LOAD-7.6** is approved as a rule: the commit opens with the full old-to-new key table, shown to the user before any key is renamed; the rename is hard, with no aliases (an alias is a second spelling kept forever), and the unknown-key error (LOAD-7.1) names the new key; the one casing is snake_case (only the `sql` section is camelCase today); and the PHP range and the extension activation of a file come from its owning manifest, the nearest ancestor `composer.json`, a fact of each directory's listing that A8's scan finds for a degrade root (COMP-8) and `Packages` holds from W2 (a hull would judge 202 Symfony components, whose manifests say `>=8.4.1`, against versions they cannot run on; J1.2 and H2.2 change; a file with no owner uses the project root; activation is any owner's). **REG-18.4** is approved (php 8.5.4: `\APP\A` and `\app\A` find `App\A`, and `\App\a` does not). **CHECK-11** is approved with a definition its row lacks: the commit says which calls are template renderers (a configured list of methods, with defaults for Latte, Twig and Smarty when installed), the ORM half reads the resolved mapping attributes and annotations, and ROADMAP 182g later replaces the blanket with the properties templates read. **LOAD-7.2's PHPUnit half** stays ungated: the narrowing resolves the callee to `PHPUnit\Framework\Assert` (`checker/narrowing/phpunit.rs`), the mock types come from PHPUnit's own PHPDoc, and the code after an assertion only runs when it held. They are not fillers: W1's other rows do not wait for them. The first three change output on a project that uses them and each carries its fixture and a VAL-2.10 line

*Needed before:* W1; batch 4b (REG-18.4)

---
