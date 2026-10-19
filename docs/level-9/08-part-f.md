# Part F. What still needs designing

## Tasks this part owns

None. Part F holds designs, not ledger tasks: the designs of G1 to G5, H1 to H4, I1 to I5 and J1 to J3, and the list of what still needs one. Each ledger task those designs serve is owned by the part that builds it (batch 0 by the README's batch 0 rows, the wall by Part D, and so on).


What exists: Parts A and B are designed (B has the open points below). The 20 G plan's items 0 to
19 have designs written at `5fb858c9`. The ledger's rows carry specifications. Level 3's tasks carry
"Design" paragraphs written at `3eaddbd1`. The crate split plan has designs for C6 to G. "Needs a design" below means the design is missing or no longer fits; a design that
exists but needs a read against HEAD is a check, and is listed after the table.

## Needs a design, in the order the work needs it

| # | design | what exists | what is missing | needed before |
|---|---|---|---|---|
| 1 | How the checker holds a tree (#1). **Designed: G1** | B4's rule for ids in checker state; `Checker<'a>` borrows registry and settings, `Locals` has no lifetime, `Checker::new` has 30 call sites, `Rc<Expr>` sits in 11 places | whether `Locals` and the checker take a tree lifetime or root what they store (rooted: Stage D deletes the lifetime again); the rooted form for an expression kept past its walk; skimmed bodies' short-lived trees; the infer cache key | item 9, step 1 (it fixes the API's signatures) |
| 2 | Stored-alias identity. **Designed: G2** | `Rc::ptr_eq` merge tests on the condition and query-builder aliases (`control_flow.rs:446`, `:478`), meaning "both branches inherited this very store"; the `@var` memo keyed by a doc text's address (`var_tag_types.rs:290`) | whether the stored value keeps instance identity (a store stamp, or an `Rc` around the handle) or takes the node's identity, which would be a semantic change; the memo's new key | step 1 |
| 3 | The error-node model. **Designed: G3** | decisions 4 and 15; Q-10, SYN-1.9 and SYN-6 as needs; F77's reporting design | what an error node keeps (salvaged children) and how readers treat it; recovery points on the token stream; marks for versions; how the registry gets "an entry of unknown shape" | step 2 (layout); the rest before batch 3 |
| 4 | The probe as a checker entry point (#26). **Designed: G4** | the current probe (`completion.rs`: synthetic statements, sentinel spans) shared with hover through `probe_receiver_type` | the entry point's inputs and outputs (a statement prefix, an expression handle from another tree), the hover path, static and script scopes | step 2 |
| 5 | Node layout per variant. **Designed: I1** (the census confirms it) | B3's shape and size limits | which of the 47 expression and 26 statement kinds are inline or in side tables, and the nested lists' layouts; it needs step 0's census | step 2, after step 0 |
| 6 | Item 0's tooling plan (#13). **Designed: G5** | 20 G item 0: the printer, `registry-dump`, the expression-types dump, O2, the session, the edit replay | a refresh against HEAD (the registry's shape moved: templates, flags), the baseline procedure at a current commit, what is kept and for how long | batch 0 |
| 7 | Item 10's token payloads under decision 4. **Designed: I2** | 20 G item 10: `TokenStream` with payloads as `&'a Name` and `&'a [u8]` into an arena | the arena was dropped (it is not `Sync`): payloads become ids into the builder's tables, which grow while the lexer runs, so references into them cannot be held | batch 1 |
| 8 | SYN-3.2's kernel (#10). **Designed: I3** | SYN-3.1's two types; the old SSE2 and AVX2 plan | the fused newline and non-ASCII kernel on `wide`: block size, tails, line-terminator rules, the `memchr` oracle | batch 2 |
| 9 | Parse recovery as a whole. **Designed: G3.4, G3.8** | (3) | the recovery algorithm; the load's failure reporting (F77); the editor's rule for a failed parse (`Project::update_from` parses before it unindexes, so a failed parse leaves the file's registry entries as they were) | batch 3 |
| 10 | The engine's identity model (#18). **Designed: H1** | decision 3; REG-16.1's measurement; REG-16.2 and REG-8.6 as rows; NAME-2's weak tables | ids, interners and their lifecycle as one design; the API of `FoldedName`, `ConstName` and `ArrayKey`; NAME-2.1's thin name (a spike) | batch 4 |
| 11 | The single grammar's lowering (#14). **Designed: I4** | F41's design (a `TypeNode` to `Type` lowering, the string parser deleted), DOC-2.1's (`phpdoc::lower`), C7's list | where the lowering lives (F41 moves the grammar to its own crate if `types` cannot depend on `phpdoc`), `parse_phpdoc_type`, how DOC-2.2's memo keys over it | batch 6 |
| 12 | The rule message type. **Designed: I5** | CHECK-6.10 and 4.1; the crate split's D6 `RuleErrorBuilder` | one lazy message type that is also D6's | batch 7 |
| 13 | The engine. **Designed: H2** | decision 3 (principles); REG-16.1 (cost of a query: 374 Ir, 67 per name per file) | the queries and their keys; read recording through REG-4.12's accessors (they are one design); early cutoff; threads and the pool (REG-16.1 did not measure queries on several threads); cancellation; panics (REG-7.9); cycles; revisions in a batch run | W2 |
| 14 | REG-7 and REG-8 as engine queries. **Designed: H3** | rows 7.1 to 7.12, 8.1 to 8.7 | each level's query, key and cutoff, on (13) | W2, after (13) |
| 15 | The pipeline as queries, persistence and the Session. **Designed: H4** | REG-16.4, CACHE-13.2, the crate split's F1 and F2 (F2's design superseded by decision 3) | one design: inputs, the result cache and its key, dependency fingerprints, `AnalysisResult`, `update(FileDelta)`, salsa's `persistence` or the cache's container | W3 |
| 16 | C5's data model. **Designed: J1** | decisions 6, 7, 10, 11, 16, 18; rows STUB-7.1 to 7.12, STUB-12 | version ranges as data, the stub layer on the engine, STUB-12.2's generator and its format, CHECK-10 | W4 |
| 17 | C6 to C9. **Designed: J2** (as rows) | the crate split's text (stale: per-class `OnceLock`, constant cycles typed `mixed`) | ledger rows with needs, and the text re-cut against decisions 1 to 18 | W5 |
| 18 | Stages D to G reconciled. **Designed: J3** | the crate split's D to G; §12.10 of the 25 G report | one inference design from §12.10 and D1 to D6, the rule API, E1 to E7, F3 to F5, G | W7 |

## Designed, but needs a read against HEAD (a check, not a design)

- 20 G items 11 to 19, written at `5fb858c9`. The premise has moved for 13 (PROD-1 to 3), 14
  (DOC-1), 15 and 18; the ledger's targets say where.
- Level 3's "Design" paragraphs, written at `3eaddbd1`. F57 looks built; the 136 count has stale
  items.
- Part B's measurements, re-taken at step 0.

## Specified, or waiting on a ruling

- LOAD-7.2's PHPUnit half and the deltas of LOAD-7.6, REG-18.4 and CHECK-11 are settled (#42):
  the PHPUnit half stays ungated, and the three deltas are approved with their conditions. LOAD-7.8's settings are
  each "its phase".
- REG-17, CHECK-7 and CHECK-8: the semantics are fixed by decisions 9, 17, 1 and 8; their designs
  belong to their phases.
- Level 1: the roadmap's phases, not audited here.

## Order

Every row of the table is designed: 1 to 4 and 6 in Part G, 10 and 13 to 15 in Part H, 5, 7, 8, 11 and 12 in Part
I, 16 to 18 in Part J. What no reading settles is H5's eleven spikes (S-1 and S-2 before batch 4) and step 0's
census, which confirms Part I1's layout. Row 9 is G3.4 and G3.8.

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

**Item 0 first.** Between Part A and item 9, in G5's order: read HEAD against the last validated state (VAL-1.1), build the canonical type printer (1.4, after F21), the full-field O2 (1.8), `registry-dump` (1.6), the expression-types dump and the PHPDoc record (1.7), the server session (1.9) and the edit replay (1.10) and the memory profile (1.11); run the `oracle` profile once over all of them (1.2, which includes the replay, so it comes after it); retire the debug oracles (1.3, which also shrinks item 9's reader set); take the baseline (1.12). VAL-1.5 is item 9's dump. The 20 G plan designed it so (oracles are dumps), Piwigo's 581 diagnostics are a weak oracle for exact refactors, and the rule is not to gate on Piwigo. **The policy on old implementations:** no row keeps one alive as a standing test; a comparison against the implementation being replaced is allowed for one stage where a dump cannot see the difference (every `Provider` answer for every name, H3.6 steps 2 and 3; I4.5 step 3 only if the PHPDoc record cannot see what a per-`TypeNode` comparison finds; I2's comparison of the old and new lexer's (kind, span) pairs), each is listed in Part N with the commit that deletes it, and A2 keeps none

*Needed before:* batch 0

## Facts the decisions rest on

- The 20 G plan's §9 makes the oracles dumps.
  `FileBodies` still keeps statements in debug builds for the old oracles.
