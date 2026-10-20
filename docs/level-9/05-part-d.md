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

### #39 LOAD-5.1's passes (architecture, banned constructs, class shapes, Composer and mock hygiene, unmatched ignores) are the crate split's E1 collector rules: build them in batch 8 as per-file products, or as E1's collectors on D6's API

**Split by form: W3 builds the facts, E1 moves their collection.** The six passes' per-file facts are `file_products` fields built in W3 (H2.3 and H4.4 already read them, the persisted whole-program queries need them, and no analysed tree can be freed after its check while a pass still reads it), so LOAD-5.1 moves to W3 and LOAD-5.2 follows it and S-7's verdict. E1 adds the D6 Collector API and the semantic fixes over those fields, so the data is built once; the passes do not all land in E1: architecture, class shapes and Composer hygiene are E1 collectors, banned constructs are per-node rules in the rules walk (the crate split calls them per-file checks, not project-level), mock hygiene is ext-mockstan's (E4) and unmatched ignores are the finalizer's (F1). Built in batch 8 they would have been written twice; with them batch 8 empties (LOAD-5.4 is Part A's, CACHE-13.1 is W3's, CACHE-8 follows S-4)

*Needed before:* W3 (facts); W7 (collection)

### #42 deltas the ledger marks READY, or OPEN with half a ruling, that neither VAL-2.9 nor decision 14 approves: LOAD-7.6 (the config's keys: grouped keys named for the PHPStan parameters they cover, one casing, no key repeating its section's name, PHP versions and extension activation read from every discovered Composer root's manifest), REG-18.4 (a constant's namespace folds case at every lookup: `\APP\A` finds `App\A`, which is `constant.notFound` today), CHECK-11 (`property.unused`: ORM-mapped properties count as used while ext-doctrine is active, and view objects passed to a template renderer are not reported) and LOAD-7.2's PHPUnit half (whether PHPUnit's assertion narrowing and mock types are gated on ext-phpunit's activation, which nothing reads today, or stay a fact of the program, since a PHPUnit assertion throws whenever PHPUnit runs it)

**Each with a condition.** **LOAD-7.6** is approved as a rule: the commit opens with the full old-to-new key table, shown to the user before any key is renamed; the rename is hard, with no aliases (an alias is a second spelling kept forever), and the unknown-key error (LOAD-7.1) names the new key; the one casing is snake_case (only the `sql` section is camelCase today); and the PHP range and the extension activation of a file come from its owning manifest, the nearest ancestor `composer.json`, a fact of each directory's listing that A8's scan finds for a degrade root (COMP-8) and `Packages` holds from W2 (a hull would judge 202 Symfony components, whose manifests say `>=8.4.1`, against versions they cannot run on; J1.2 and H2.2 change; a file with no owner uses the project root; activation is any owner's). **REG-18.4** is approved (php 8.5.4: `\APP\A` and `\app\A` find `App\A`, and `\App\a` does not). **CHECK-11** is approved with a definition its row lacks: the commit says which calls are template renderers (a configured list of methods, with defaults for Latte, Twig and Smarty when installed), the ORM half reads the resolved mapping attributes and annotations, and ROADMAP 182g later replaces the blanket with the properties templates read. **LOAD-7.2's PHPUnit half** stays ungated: the narrowing resolves the callee to `PHPUnit\Framework\Assert` (`checker/narrowing/phpunit.rs`), the mock types come from PHPUnit's own PHPDoc, and the code after an assertion only runs when it held. They are not fillers: W1's other rows do not wait for them. The first three change output on a project that uses them and each carries its fixture and a VAL-2.10 line

*Needed before:* W1; batch 4b (REG-18.4)

---
