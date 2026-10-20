# Part F. What still needs designing

## Tasks this part owns

None. Part F holds designs, not ledger tasks: the designs of G1 to G5, H1 to H4, I1 to I5 and J1 to J3, and the index of where each one is. Each ledger task those designs serve is owned by the part that builds it (batch 0 by the README's batch 0 rows, the wall by Part D, and so on).


Every design the plan needs is written, in the part named in the table below. The checks that remain are the reads
against HEAD listed after the table.

## Where each design is

| # | design | part | needed before |
|---|---|---|---|
| 1 | How the checker holds a tree (#1) | G1, [`07-part-b.md`](07-part-b.md) | item 9, step 1 |
| 2 | Stored-alias identity | G2, [`07-part-b.md`](07-part-b.md) | item 9, step 1 |
| 3 | The error-node model | G3, [`14-part-c.md`](14-part-c.md) | step 2 (layout); the rest before batch 3 |
| 4 | The probe as a checker entry point (#26) | G4, [`07-part-b.md`](07-part-b.md) | step 2 |
| 5 | Node layout per variant | I1, [`11-part-i1.md`](11-part-i1.md) | step 2, after step 0 |
| 6 | Item 0's tooling plan (#13) | G5, this file | batch 0 |
| 7 | Item 10's token payloads under decision 4 | I2, [`13-part-i.md`](13-part-i.md) | batch 1 |
| 8 | SYN-3.2's kernel (#10) | I3, [`13-part-i.md`](13-part-i.md) | batch 2 |
| 9 | Parse recovery as a whole | G3.4 and G3.8, [`14-part-c.md`](14-part-c.md) | batch 3 |
| 10 | The engine's identity model (#18) | H1, [`14-part-c.md`](14-part-c.md) | batch 4 |
| 11 | The single grammar's lowering (#14) | I4, [`13-part-i.md`](13-part-i.md) | batch 6 |
| 12 | The rule message type | I5, [`13-part-i.md`](13-part-i.md) | batch 7 |
| 13 | The engine | H2, [`05-part-d.md`](05-part-d.md) | W2 |
| 14 | REG-7 and REG-8 as engine queries | H3, [`05-part-d.md`](05-part-d.md) | W2, after 13 |
| 15 | The pipeline as queries, persistence and the Session | H4, [`05-part-d.md`](05-part-d.md) | W3 |
| 16 | C5's data model | J1, [`05-part-d.md`](05-part-d.md) | W4 |
| 17 | C6 to C9 | J2, [`05-part-d.md`](05-part-d.md) | W5 |
| 18 | Stages D to G reconciled | J3, [`05-part-d.md`](05-part-d.md) | W7 |

## Designed, but needs a read against HEAD (a check, not a design)

- 20 G items 11 to 19, written at `5fb858c9`. The premise has moved for 13 (PROD-1 to 3), 14
  (DOC-1), 15 and 18; the ledger's targets say where.
- Level 3's "Design" paragraphs, written at `3eaddbd1`. F57 looks built; the 136 count has stale
  items.
- Part B's measurements, re-taken at step 0.

## Left to their phases

- REG-17, CHECK-7 and CHECK-8: the semantics are fixed by decisions 9, 17, 1 and 8; their designs belong to their phases.
- Level 1: the roadmap's phases, not audited here.

## Order

Every row of the table is designed: 1 to 4 and 6 in Part G, 10 and 13 to 15 in Part H, 5, 7, 8, 11 and 12 in Part
I, 16 to 18 in Part D (J1 to J3). What no reading settles is H5's twelve spikes (S-1 and S-2 before batch 4) and step 0's
census, which confirms Part I1's layout. Row 9 is G3.4 and G3.8.

## Level 3's placement

The level-3 count overstates the work. `STAGE_B_FIX_REVIEW.md` has 88 open findings, a validation and 16 pointer lines
(the earlier reviews' items that a finding re-opens, listed again under "Full review"); `STAGE_B_REVIEW.md` has 15 and a
validation; `STAGE_C_REVIEW.md` has 14 and a validation, of which O1 and F5 are fixed (`e06f470c`, `1cdfe80d`; the fix
review says so) and two are the same bug as a fix-review finding (Stage C's F4 is F55, its F8 is F30). That is about 115
findings. Whether each still reproduces is settled by running its probe on the gate binary in batch 0, not by reading.

**The rule for when each is fixed:** a finding in code that a later task rewrites gets its fixture now and its fix inside
that task; one that changes what the baseline records is fixed before the baseline; the rest go in the review's own order
between heavy runs.

| finding | meets | placed |
|---|---|---|
| F10 | the item 0 baseline (a skipped file has no tree dump) | its symptom is fixed; tests and fixture before the baseline, the exhaustive predicate in batch 1 |
| F21 | the canonical printers (`Repr::Mixed` changes) | before them |
| F20 | the cross-platform plan's R10; item 9 | after R10 and before item 9, so the lint checks the recursion item 9 writes |
| F77 | the item 0 baseline (a refused file must show in O2) and SYN-1.9 | batch 0, for analysed files (built: the analysed-file half and the context-file notice are in `4278ec2a`); the dump and the per-file cache record a context file's failure in batch 0 (needs VAL-1.8); batch 3 feeds it from `errors()` |
| the seven fixes the crate split's F1 puts first (debounce stack, `latest_text`, `didClose`, `--generate-baseline`, `--fix`, the closed pipe, rename) | none: they are on today's code and independent | batch 0, each with its test (built: SRV-1 to SRV-7) |
| F22 (closure `use` list), F8 and F7 (the use-site walk) | item 9 step 1 (`Visitor` over handles) | in step 1 |
| F14, F41, F19 (the PHPDoc bullets) | DOC-2.1, TYPE-5 | batch 6 |
| F69, T2 | CHECK-9, REG-17.2 | F69's fixtures in batch 0; F69 and CHECK-9 in D2, since they sit on the recorders D2 replaces; T2 with REG-17.2. T2 asks "decide whether callee throw points count" and is a decision row: they do, as PHPStan's `MissingCheckedExceptionInMethodThrowsRule` reads the throw points of calls, and F69's throw points for implicit calls and REG-17.2's inferred throws are what make it complete, so T2 is built with them and the rule reports both a direct call and `$o[…]` on an `ArrayAccess` object |
| F5 (definedness certainty), F6 (closure-literal alias), F45 (reference groups), F61 (unset properties) | D1 and D5 (holder storage): each design adds per-variable state to `Locals`, and the fix review's own first step is one `ExpressionHolder` per narrowing key | fixture now, fix inside D1 and D5 (J3); F6's design stores an `Rc<Expr>`, which G1 and G2 replace by a rooted alias that D1 deletes |
| F18 | the `&&`/`\|\|` and nested-`match` causes are Stage D's one inference; the trait chain is REG-7.2's; the `.` rope and the `elseif` suffix are local | fixture now (the scale scripts), local ones now, the rest in D3 and W2 |
| F35, F50, F55 (Stage C F4), F75, F78, Stage B D1 | REG-4.2, REG-5, REG-11.4 and 11.5, REG-8.3, REG-7.5 | fixture now, fix in W1 and W2 |
| F76 | REG-7.10 (namespace cycles) | W2 |
| F58 | G1.9 and G4a (no per-function clone; the `Resolved` is built only when the anchor matches, in both walkers) | resolved by the design |
| F33 (anonymous class names, infer cache assumption) | #34, G1.5, H1.4 | the name is the shown path and the line, as its design states (`File.shown`, H2.2); the infer cache's context epoch and the env-gated recompute-on-hit assertion are G1.5's |
| F30 (Stage C F8) | REG-4.15's builder | fixture now |
| Stage C F1 | decision 11, REG-8.1 | W2 |
| Stage C F5, F6, F7 | REG-11.10 (F5, fixed), REG-18 and J1 (F6, F7) | W1, W4 |
| Stage C F2, F3 | LOAD-7.3 and 7.4, STUB-7.3 | W1, W4 |
| Stage C F9, F10, F12 | LOAD-7.6, LOAD-7.7 | W1 |
| Stage C F11 | VAL-1.3, REG-8.5 | batch 0, W2 |
| F74 | the composer follow-ups of #40 | with them |
| Stage B "Typed purity", D2 | REG-17.4 and SUM-3, REG-11.6 and EXT-2 | W2, W5 |
| the comments (F24, F34, F37, F43, F51, F59, F80, F88, Stage C F13) and the fixtures (F23) | none | any gap; F23 before the baseline |
| everything else (F2 to F6, F9, F11 to F13, F15 to F17, F25 to F32, F36, F38 to F40, F42 to F49, F52 to F56, F60 to F73, F79, F81 to F87, F89; Stage B W1, C2, M1, CMP-1, DF-1 to DF-7) | none | in the review's own order, F4 first |

---


## G5. Item 0's tooling plan (Part F 6)

The 20 G plan's item 0 (written at `5fb858c9`) is the design; this refreshes it against HEAD, builds
on what the gates already have, and says in what order and with what acceptance.

**What exists (verified).** `cargo xtask gates | corpus | compare | type-drift | baseline | gatetime |
modgraph | arch | front-end-dump`, and `tools/gate-runner`, which runs one analysis through the same
library calls `check-project` makes and writes, per configuration, the report lines
(`path, line, col, level, identifier, message, tip`), the emission-order view, the baseline run and
`--dump-types` (every tracked variable's type and native type after each statement, as display text).
Missing against item 0: the diagnostic lines drop the end position, `non_ignorable`, the fix (span,
replacement, description, `within`) and all but a joined tip; `Display` for types drops `mixed`'s
explicit flag and a template's bound, variance, scope and default; `front-end-dump` hashes `Debug`
text; there is no registry dump, no per-expression type dump, no PHPDoc record, no server session, no
edit replay, and no `oracle` profile.

**Tools, in build order.** Each is a commit or two; each ends with its own acceptance. The oracle run comes after the
session and the replay because VAL-1.2 includes the replay, and the oracles go (VAL-1.3) only after that run.

| # | tool | content | acceptance |
|---|---|---|---|
| 1 | VAL-1.1: read the starting state | HEAD against `1892b477` (the last validated state) over the vendor trees, all-on and unscoped reports; the six listed behaviour changes (Pratt rejections, `__halt_compiler`, kind-exact keys, hyphenated words, declarations ruled out by version, the server's UTF-16 columns) read; anything wrong fixed in its own commit | each difference named and either approved or fixed |
| 2 | canonical printers (VAL-1.4), after F21 (the error type as a kind of `mixed`, which changes `Repr::Mixed`'s fields) | `types::canon` over every field of `Repr` (explicit-`mixed` flag and subtraction; template name, scope, bound, variance, default; benevolent unions, call-site variance, accessories; literal bytes and float bits; shape optionality and sealedness; conditional subjects), no interning, addresses or hash order; the same for `Diagnostic` | every printer destructures without `..` and matches without `_`, held by an `xtask arch` rule that the `canon` and `dump` modules contain no `..` in a struct pattern and no `_ =>` arm (a test that must fail to compile cannot be an in-tree test: `Repr` is `pub(crate)`, and a copy of a type with an extra field is not the type the printer destructures), plus a one-time mutation check recorded in the commit message, as tool 4's; two runs and one thread against many give identical bytes |
| 3 | full-field O2 (VAL-1.8) | the gate-runner lines gain end line and column, UTF-16 columns, `non_ignorable`, the fix and every tip; run over Piwigo's three configurations, each vendor project with every rule set, and the fixtures; stub cache cold and warm; per-file cache off, written and read | all states give one output; `xtask compare` reads it |
| 4 | `registry-dump` (VAL-1.6) | taken after the finishing passes, before checking: every project declaration and every record of both stub bases (at the configured range and its ends once ranges exist), with reflection, guard and guard value, purity dependencies and verdict, property-write classification, ancestors, owners, the ancestry fact; the constants, global-variable facts, both reverse indexes, the use-site index, duplicate-site tables, the file table (path, role, load order, absolute path, Composer owner, `mentions_phpstan`). **Defined over an enumeration interface** (all declarations as reflection records, from a registry value), implemented for `Registry` now and for `Provider` at W2 step 1, so that after a level flips to the engine (H3.6 step 3) the dump prints the engine's answers and not the old maps' on both sides; the `Registry` field destructuring below is the pre-W2 completeness check and goes with the struct | sorted by kind, folded name and load order; lazy records decoded, deferred values printed as stored; the dump destructures `Registry` (31 fields at `registry.rs:262`) without `..`, so a field added later is a compile error, and **all 31 are classified**: scratch with its reason (`raw`, `raw_active`, `changed`, `scope`, `scoped_names`, `demand`, and the history-dependent `names_with_assert_tags` ("only ever added, never removed on `remove_declarations`") and `stub_provided_functions` ("not yet overwritten"), which an edited registry and a fresh one hold differently, so printing them would fail the replay's equality by construction) or products that are printed (`doctrine_entity_index`, `trait_users`, `subtype_children`, `recorded_parents`, `as_built`, the two configured assertion tables, and the rest); the printers of `ClassReflection` (about 45 fields), `FunctionReflection` (about 24), the member records and `Provenance` destructure exhaustively too, held by the same `xtask arch` rule as tool 2. The printers never print a version, an id from a process-global counter, an address, an absolute path or a temp-directory name, and a difference between two runs is a finding to triage (engine or dump); mutation test: changing one member's flag changes the dump |
| 5 | `--dump-expr-types` and the PHPDoc record (VAL-1.7) | every expression's inferred and native type in the final pass, keyed by file, span and host class (a trait method is checked once per host); every PHPDoc resolution keyed by file, docblock span, tag ordinal, host class and function | off by default: one `Option` test at the end of inference, nothing when off; the dump is identical across `PHPSTAN_RS_THREADS=1` and the default |
| 6 | the server session (VAL-1.9) | an in-process session over Piwigo and two vendor projects, driven by **a public in-memory session API of the `lsp` crate** (start, send, wait until quiet, run a closure over the `Project`, and a dump request), which `lsp`'s own tests, `tools/gate-runner` and the replay all use. The harness of `lsp/tests/helpers.rs` (`encode`, `decode_all`, `run_rooted`, `run_across_debounce`) is `pub(super)` and test-only today and drives the server's private `run_loop` and `Server`, so it cannot live in `testing` (the module graph gives `testing` no `lsp` edge, `testing` depends on nothing above `project`, and a dev-dependency from `lsp` back on it would be a cycle that links two copies of `lsp`): `testing` keeps the fixture runner only. `run_rooted` pumps all input and exit and returns only the decoded messages, and `run_loop` builds the `Server` inside and drops it, so stepping needs `run_across_debounce`'s thread-and-sentinel pattern (a `didChange` is applied on the debounce worker, so the end of a step is not the end of a message) and a hook into the server's `Project`. The requests: files and roles, `registry-dump`, diagnostics with UTF-16 ranges, code actions, definition, hover and references, and completion (after `->` and `::` at a seeded sample of receivers), signature help, document symbols and semantic tokens, each over a sample chosen by a hash of each path, since item 9's step 1 migrates every `lsp` reader (navigation, completion, signature help, document symbols) and step 2 rewrites the probe. `TempDir` and `TMPDIR` are under `~/.cache` (not the RAM-backed `/tmp`) and removed afterwards | the first dumps equal the CLI's (LOAD-1's equality) |
| 7 | the edit replay (VAL-1.10) | a fixed script over a seeded sample, **on a copy of the corpus** (a hard-link or reflink copy under `~/.cache`, removed afterwards: the Piwigo corpus is frozen and `vendor/*` are submodules whose changes `require_clean_tree` rejects), over Piwigo and two vendor projects: save one unchanged, delete a method, rename a class, change a parameter type, add a file declaring a class named but declared nowhere, delete a file; after each step the project dump, `registry-dump` and every file's diagnostics; at the end a fresh load of the edited tree | the end state equals the fresh load; a failing equality leads to commits of its own, as tool 9's |
| 8 | memory (VAL-1.11) | the dhat profile by holder with untrimmed backtraces, at this commit; the conversion and holder scripts move into `scripts/` first (they live outside the repository today, in `phpstan-rs-kept/prs-profile/dhat-heap-modes/analysis/`, with a hand-curated `OVERRIDES` table) | no allocation left unplaced (the 10,987 of M10), and the holder table at this commit is the new baseline, explained against MEAS-2's (which measured `791d29cb`) by the commits since, LOAD-3.3's -214 MiB among them; no heap noise band exists, so none is claimed |
| 9 | the `oracle` profile and its one run (VAL-1.2) | `gate` plus `debug-assertions` and `overflow-checks`, with `PHPSTAN_RS_NO_FILE_CACHE=1`, over the three Piwigo configurations, each vendor project with every rule set, the server session and the edit replay (the incremental paths hold oracle sites that only they reach); a temporary uncommitted counter shows every oracle site ran | every failure fixed in its own commit |
| 10 | retire the debug oracles (VAL-1.3) | `FileBodies` stops storing statements; `BodySource::body` and the debug-only re-exports go; the test oracles whose second implementation exists only for them go; assertions about one implementation stay (the file cache's round trip) | tests and fixtures pass; item 9's reader set is smaller by these |
| 11 | the baseline (VAL-1.12) | every dump above from the commit that adds the last tool, with the `gate` build at one thread and at the default count (byte-identical) and with the `oracle` build (no assertion); the ordered file lists with roles | stored with the binaries |

VAL-1.5 (the front-end dump) is item 9's: its canonical dump over handles replaces `front-end-dump`
and is baselined at the end of step 1.

**Where things live.** The writers sit with the data: `types::canon`, `registry::dump`,
`checker::dump` (types, PHPDoc), `syntax::dump` (item 9). The gate-runner gains `--dump registry |
expr-types | phpdoc | diagnostics`; `xtask dumps` records the set at HEAD under
`.corpus-runs/dumps/<commit>/`, and `xtask dumps-compare <a> <b>` diffs two sets and classifies every
difference by dump and file. The baseline's binaries are copied to `.corpus-runs/gate/bin/<commit>/<profile>/`
and its profiles to `~/.cache/prs-profile/<run>/` as soon as they exist, and nothing else is kept:
the dumps a comparison reads, and no per-commit hoard.

**What each dump protects.** Item 9: the expression-types dump (the whole checker moves to handles),
the registry dump (unchanged by it), O2, its own dump. Batches 1 to 3: its dump. Batch 4 (names):
registry, expression types, O2 (container-field keys are VAL-2.7's delta). Batch 5: registry, O2.
Batch 6: the PHPDoc record, expression types, registry. Batch 7: expression types, O2 (CHECK-6.4
bumps a format). The wall: `registry-dump` across REG-7, the edit replay across REG-8, REG-16.4 and
CACHE-13.2.

**Wall-time rule.** A wall-time accept in this plan (item 9's, the spikes') is a paired, alternating
comparison by `scripts/abab.py` with the ledger's rule: the pair count stated with the accept, every output
checked against the oracle, a Wilcoxon test at a stated p-value, and an Ir count where Ir decides (the VM has
no performance counters, and plain off runs vary by hundreds of milliseconds: 2,310, 2,296 and 2,878 ms).
`abab.py` has Piwigo's path and digest as constants today and is parametrised by corpus and digest, and item 9's
wall, Ir and heap comparisons measure a second corpus (a vendor project through `check-project` and
`incremental-bench`) beside Piwigo, since the target is general PHP.

**Risks.** The registry dump's field list moves with the registry (REG-4.x already changed it), so
the printers destructure everything: a new field is a compile error in the dump, not a silent gap. A
dump that costs minutes on Piwigo is acceptable; one that differs between runs is a finding to triage
(engine or dump). Nothing here gates on Piwigo alone: each dump runs over the vendor projects and the
fixtures too.

## Decisions

### #13 oracle tools

**Item 0 first.** Between Part A and item 9, in G5's order: read HEAD against the last validated state (VAL-1.1), build the canonical type printer (1.4, after F21), the full-field O2 (1.8), `registry-dump` (1.6), the expression-types dump and the PHPDoc record (1.7), the server session (1.9) and the edit replay (1.10) and the memory profile (1.11); run the `oracle` profile once over all of them (1.2, which includes the replay, so it comes after it); retire the debug oracles (1.3, which also shrinks item 9's reader set); take the baseline (1.12). VAL-1.5 is item 9's dump. The 20 G plan designed it so (oracles are dumps), Piwigo's 581 diagnostics are a weak oracle for exact refactors, and the rule is not to gate on Piwigo. **The policy on old implementations:** no row keeps one alive as a standing test; a comparison against the implementation being replaced is allowed for one stage where a dump cannot see the difference (every `Provider` answer for every name, H3.6 steps 2 and 3; I4.5 step 3 only if the PHPDoc record cannot see what a per-`TypeNode` comparison finds; I2's comparison of the old and new lexer's (kind, span) pairs), each is deleted in the commit after its stage (the overview's convention), and A2 keeps none

*Needed before:* batch 0

### #22 level 3 and the Q-11 follow-ups

**A correctness backlog, not fillers.** Triage the findings first (about 115 are distinct; F57, Stage C's O1 and F5 are
built), by running each probe on the gate binary. F10's symptom is fixed (`can_begin_expression`'s list, `3b7e4d95`), so
its parser tests and fixture go before item 9's baseline, which must not encode a parser bug, and its exhaustive predicate
waits for SYN-2's `TokenKind`. F20 goes before item 9 (after the cross-platform plan's R10), so its lint checks the
recursion that item 9 writes. F21 goes before the canonical printers. F41 goes in batch 6. F77 goes in batch 0, before
item 9's baseline: an analysed file's diagnostic and exit status are the final ones, so a parser rewrite that newly
refuses a file shows in O2, and batch 3 feeds it from `errors()`.

**F77's context-file half** (`phpstanRs.contextParseError`, run-level, failing the run) is user-visible behaviour that
decisions 9 and 15 ("nothing in a context file is reported") do not give. Those two decisions keep their text and gain one
sentence: a context file that fails to parse is reported as a run-level notice (its path, line and column, and the first
error; `phpstanRs.contextParseError`), printed where the "N file(s) skipped: parse error" count is, and it leaves the exit
status as it is; an opt-in switch makes it fail the run. Batch 0 builds the notice and records each failure (the count and
the first error per file) in the dump. F69's fixtures are batch 0's; F69 and CHECK-9 are D2's (#17).

The seven fixes the crate split's F1 puts first (the debounce thread's stack size and the scan's parses on the pool,
`queue_change` writing `latest_text`, `didClose` restoring the disk's content, `--generate-baseline` keeping the entries of
files not analysed, `--fix` skipping a file changed since analysis, a closed output pipe ending the run quietly, rename
refusing qualified names, imports and aliases) end the server, lose data or corrupt files, and are independent of
everything else here. They are batch 0's, each with its test (built: SRV-1 to SRV-7 in the README's batch-0 table), and
rows of the ledger and `WORK_STACK.md`, not part of W3.

Where each finding sits is a place in the sequence: every finding or follow-up that changes a dump goes before the
baseline, and the rest go in the review's own order (F4 first), fixture first where a later phase rewrites the code (the
table under "Level 3's placement" names which). A later commit that changes behaviour runs `dumps-compare`, lists its
differences as its delta, and rolls the baseline forward.

*Needed before:* batch 0

## Facts the decisions rest on

- The 20 G plan's §9 makes the oracles dumps.
  `FileBodies` still keeps statements in debug builds for the old oracles.
