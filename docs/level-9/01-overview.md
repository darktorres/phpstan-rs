# Overview: the conventions and the decisions

The plan is written from the code at `0fb1a4de` (Parts A to F), `30aa2002` (G) and `8a1ca1e0` (H to J); the review passes re-read the code at `52844b07` and `ffa7cccb`. Line numbers
are hints from those commits and the tree moves, so a citation is found by the symbol it names; "the fixes plan's R7"
is `ITEMS_1_TO_3_FIXES_PLAN.md` (ledger LOAD-8.7) and "the cross-platform plan's R7" is `CROSS_PLATFORM_PLAN.md`.

Part A is built and validated (its status is in Part A's intro, [`02-part-a-intro.md`](02-part-a-intro.md)). Nothing from Part B onward is built by this plan. The batch-0 fixes already in the tree are listed in the README's batch 0 rows. The order of the work is the sequence in the [README](README.md).

Both start only after the cross-platform fixes have finished (`CROSS_PLATFORM_PLAN.md`, rows R0 to R18, finished at `a83b1675`). Its rows edit what this plan's first rows edit: `session.rs`, `project.rs`, `walk.rs` and
`lsp.rs` (R5 to R7), `includes.rs` (R8), `composer` (R3, R9, R16), `main.rs` (R10) and the caches (R11). R18 moves
code this plan reads: `file_id` and `shard` into reflection, the completion rules (`Completion`, `Reach`,
`BlockCompletion`) and `is_truthy_literal` into `syntax`, the rebind and include-guard mention bits into
`checker::mentions` (the project hands the checker the file's whole `mentions`), `set_current_file` taking the shown
path, `path_bytes` into a crate of its own (`phpstan-rs-path-bytes`, which R16 works in) and the allocator into one
module of the binary. Heavy jobs run one at a time, and the stack's rule is that level 8 resumes when the cross-platform
fixes finish. Start condition for Part B: `git log` shows R17 and R18, `WORK_STACK.md` no longer lists the cross-platform fixes, `git status` is clean, `ps`
shows no other build or profile. Every design below is read against that level's code (the case rule of R7, the
canonical form of R3, the `/`-spelled report paths of R6, the module homes of R18) and says where it meets one.

Conventions for every row: `cargo fmt` first, then `cargo check`, `cargo clippy -D warnings`,
the targeted tests, a commit with a pathspec. Comments name no row or phase. Output of long
runs goes to a file under `~/.cache`, then is searched. No saving is claimed without a
measurement.

**Validation is at the boundary the user names:** the end of each top-level item (the 19 items of the 20 G plan and
the stages of the crate split), never at a step or a row inside one, and no checkpoint is added that the user did not
ask for. Where a design below says a comparison "at each step" or "at each commit", the dump or oracle run is produced
at that point (a cheap run that stores output) and compared with the stored baseline at the item's end, and the suites,
fixtures and corpus passes run once there; the per-commit checks are `cargo fmt`, `cargo check`, `cargo clippy` and the
touched module's tests.

An old-versus-new comparison is deleted in the commit after its stage: I2's lexer `(kind, span)` fuzz when the old lexer
is deleted; I4.5 step 3's per-`TypeNode` comparison, only if VAL-1.7's PHPDoc record cannot see what it finds; H3.6
step 2's `Provider`-against-engine comparison and step 3's old passes, one level per stage. A2 keeps none.

## Edits at approval

Approving this plan makes these edits. None is made before the go.

- `WORK_STACK.md`, section 9: level 8 is reordered so that item 0 (batch 0) comes before item 9, and Part A's rows come
  before item 0. Item 9 reads "SYN-1.11, then 1.12 (+1.3), then 1.2/1.4/1.5/1.7/1.8, then step 3 (LOAD-3.5); the design
  is Parts B, G1 to G4 and I1". The batches of C1 follow in order: tokens, the lexer's second pass, parse recovery with
  the `php-version` crate, names, the constant lookup, products, PHPDoc, the check's temporaries. Item 2's bullet goes,
  LOAD-9 leaves "Outside the items", item 8 keeps REG-14.1 and 14.3, item 19 loses LOAD-5.4, and the state line is
  rewritten. The "Documents with no place in the stack" list gains this plan's documents.
- `CRATE_SPLIT_PLAN.md`: the docs commit series of #20 (below), which also writes the ledger rows of J2 (OVL, RES, EXT, SUM) and the D and E rows of J3, in [`05-part-d.md`](05-part-d.md).
- The ledger rows that decision 20 writes: OVL, RES, EXT and SUM (J2), with the
needs above; a D-row and an E-row per phase of Stage D and E that the plan lists, each citing its plan section and
the oracle counts it carries; CHECK-6.1 to 6.3, 6.7, 6.13 to 6.15 re-pointed to "built by D1 to D4"; LOAD-5.1
re-pointed to W3 (facts) and E1 (collection); and the ledger edits the engine cluster implies (`01-overview.md`, Edits at approval).
- The ledger's `needs` changes of D8, in [`07-part-b.md`](07-part-b.md) (B11).
- Other documents. `CROSS_PLATFORM_PLAN.md`: R11's database-file follow-up and R16's `path_bytes` encoding pair (H4.6),
  and R13's CRLF probe files marked `-text` (G3.7.1). `CRASH_REPORTING_PLAN.md`: the stack guard's `LIMIT` restored on
  unwind and the engine's two isolation modes (H2.8); the vendor-discovery panic-hook swap is deleted, not converted
  (#40). `UNSAFE_PLAN.md`: its end state changes only if S-2 finds the engine exception necessary (#33).
  `STAGE_B_FIX_REVIEW.md`: F14 superseded by TYPE-5's rule; F33's epoch lands in G1.5; F58's lazy construction in G1.9.
- The ledger rows the engine cluster re-specifies: REG-8.6 (H1.3); REG-4.15 loses its span field and REG-4.11 becomes a
  record over (`Origin`, site) (H1.5a); REG-16.2's and decision 3's "never reused" stand for `DeclKey` (#38), and decision 3's
  member sentence is refined to PHP's namespaces (H1.4); REG-16.4 needs SYN-1.9; REG-16.5 is the event callback plus the
  provider's trace of first readers (H3.5); PROD-5.4, PROD-5.7 and LOAD-4.7's file-references half are marked "deleted by
  REG-16.4" (H4.8); CACHE-13.2 is H4.6, keyed without the version range (#37), and CACHE-9 to CACHE-11's form follows S-4;
  CACHE-13.1 and decision 13 become `File.stamp` and the stat-time rule of #15; STUB-7.5 moves to W3 and needs the Composer
  maps to keep their keys and gain PSR-0 (H4.1, A7); PROD-5.1 and SYN-1.10 keep anonymous classes out of the declaration
  names and the global index (H1.4).
- `crash_report::cache_dir()` moves into `project` (F2).

Two gates hold every row that moves code, and they are cheap to different degrees. `cargo xtask modgraph` parses the
graph and runs `cargo metadata`, so it runs per commit: a new crate gets its `crate` line and a new top-level module of
the package its `module` line in `gates/module-graph.txt` in the commit that makes it, as does a nested module whose
crate differs from its parent's, such as `registry::purity analyser`; a module that only the library root holds is
rejected. `cargo xtask tests` compiles every test target (the test profile for the workspace, the gate profile for the
benches and the fixtures target) and so runs at the validation boundary; a test that is renamed, merged or deleted is
listed as `old -> new` or `old -> REMOVED: reason` in a `gates/test-renames/*.txt` file in the commit that does it
(the gate reads every commit of the range, so "the same commit" is a convention), or the manifest fails; item 9's test
rewrite and the retired oracles are the large cases. A new crate also joins `LIBRARY_CRATES` in `xtask/src/arch.rs`
(else its no-print rule skips it) and the build-time scenario lists in `xtask/src/main.rs` (an unlisted crate is
unconstrained); the new crates of this plan (`php-version`, `engine`, `stack`) do both in the commit that makes them.
A row that adds an `xtask arch` rule adds it with the code that satisfies it. **CI does not run `cargo xtask gates`:**
the `gates` job of `rust-ci.yml` (Linux only, `ubuntu-24.04`) lists its steps one by one and calls `xtask modgraph`,
`arch` and `tests`, and the other systems are the `platforms` job. So a new `xtask arch` rule reaches CI by itself,
and any other cheap gate (F20's recursion lint) is added in two places, `gate_steps` and a step of the `gates` job. A
heavy tool (the dumps, the baseline, the `oracle` profile run) stays local: it needs the frozen Piwigo checkout and a
quiet machine, and a dump spells paths and diagnostics as its platform does.

## What no reading settles: spikes S-1 to S-12 (H5)

None in the ledger. The spikes S-1 to S-12 are this part's own work, defined in the table below, and they are not ledger rows (the ledger has no state for them). Each runs at the gate point the intro gives, and its result is recorded where the design it settles says so. The ledger rows the spikes gate are owned by the parts that build them.

Each is a short build with a measurement and a stated accept and kill. None changes this plan's decisions;
each can send one design to its stated alternative. S-1 runs before batch 4, since it decides the name's layout.
S-2 and S-3 run early too, since the choice of engine is cited by everything from batch 4's rules (H1.5) to the
wall, and a skeleton is cheap against finding out at W2 that salsa is unfit. **Each spike has a gate point:** S-1,
S-2 and S-3 before batch 4; S-12 before item 9's step 2; S-4 right after S-3 and before W2's result types (H4.6 states them); S-5 and
S-9 on the S-3 skeleton, before W2 step 2; S-6 before W2 step 3; S-8 with batch 4; S-7 before W3 step 1; S-10
before W3 step 2; S-11 before W3 step 4. **The measurement rules:** a wall-time accept is G5's wall-time rule
(`abab` pairs, stated pair count and p-value); "the phase start" is REG-12's gate baseline (`xtask gatetime
--since 58cfc5d2`, medians, peak RSS at or below it), and for a skeleton that replaces only part of the
registry it is the same binary built at the skeleton's parent commit; every wall or Ir comparison states its
commit and mode (off or warm), and every skeleton also measures a cold run (below). **One exchange rate between memory
and time serves every spike that trades them (S-7, S-12):** 50 MiB of peak RSS for 1% of wall time, or of the check phase's
Ir where Ir decides (proposed, stated once, so that no spike sets a rate of its own).

| # | question | method | accept | otherwise |
|---|---|---|---|---|
| S-1 | is `triomphe::ThinArc<Header, u8>` fit for `Name`? | a scratch crate: `from_header_and_slice`, pointer identity (`as_ptr()`, there is no `ptr_eq`), the count and uniqueness (`strong_count(..) == 1`), `Send + Sync`, no `Weak`; **how a `&str` is read back without `unsafe` of ours** (validation on every `as_str`, `lower` and `Deref`, or a crate's thin header-plus-`str` type such as `slice-dst`'s `StrWithHeader` behind `erasable::Thin`, judged on merit) and the corpus total of those reads; intern and lookup against `Interner::name` (260.8 M Ir over 1.01 M calls) and `name_hashed` (354.8 M over 1.44 M); allocation count; **retained bytes of count-one names at the peak, the sweep's time per pass, and the share of `FoldedName` comparisons that take the byte-compare path** on Piwigo, the vendor projects and the fixtures; the hash function: one defined over the text's bytes alone, so NAME-1.5 can hash a name's parts as one text and compare a candidate part by part (a hash whose result depends on how the bytes are chunked cannot); where none is fast enough, NAME-1.5 joins the parts in a reused scratch buffer, which allocates as little | the API gives identity and a count; Ir per intern not above today's; **the corpus total of `as_str`, `lower` and `Deref` Ir not above today's loads**; allocations per name 1 | H1.3's fallback: one `Arc<NameData>` with a single `Box<str>` holding the text and the lowercase form (two allocations, the weak table and its sweep kept) |
| S-2 | do salsa's macros build under `unsafe_code = "deny"`, and does the engine build and run on every leg? | **first, build the skeleton under the workspace's `deny` with no `allow`** (a probe on the pinned toolchain suggests an external crate's macro expansions are not reported; only a failure grants the engine module `#![allow(unsafe_code)]`, #33); one of each salsa item; clippy with the workspace lints; an `xtask arch` check that the module's own text has no `unsafe` token; the skeleton's tests run on the macOS, Windows and aarch64 Linux legs of `rust-ci.yml` **and as a `cargo test --release` of the skeleton (an integration test binary built with the release profile) on each leg that constructs a database and runs one query**, because salsa's default `inventory` feature registers its ingredients at link time (with `thin` LTO and `codegen-units = 1` in the release profile, which no step of `rust-ci.yml` links), which is where the platforms differ; `cargo deny check` | it builds under the unchanged lint, or with the one exception and the `xtask` check, and its tests pass on every leg including the release one | the pinned feature set (H2.1) without `inventory`, where one leg fails: every ingredient is then registered by hand in `Storage::builder()` as salsa's `manual_registration` test does (a forgotten one fails at run time); else an engine of our own with H2.1's contract |
| S-3 | cost of the shape | a skeleton on Piwigo plus two vendor projects (the load with and without LOAD-2.4's overlap of walk and products): `file_products`, `decl_index`, `declarations_named`, the level queries over the old declarations, a `Provider` with the member grain of H2.4; it counts the distinct (file, class, member) reads, which REG-16.1 did not, and the interned-key edges; **the memos an immortal `DeclKey` retains** (the bytes of a dead declaration's levels after an edit-replay series of renames and deletions, and the key table's growth); **the distinct ranges and the extra level memos of range-keyed levels** on `vendor/symfony`, `vendor/laravel` and `vendor/rector-src`, whose manifests differ (J1.2); **it measures a cold run** (load plus check, no persistence: cold Ir and wall, the median of repeated runs on Piwigo and the vendor projects), since a batch run pays recording, interning and memo creation for every edge with no incremental benefit and W3 cannot undo a slower cold run; clean and incremental build time and the release binary size | a name read ≤ 80 Ir per lookup (today's 73 to 80); warm verification of all checks ≤ 20 ms (REG-16.1: 5 to 10 ms for 107,068 edges, so 47 to 93 ns per edge, which bounds the member grain at about 220 thousand edges); the member first reads' Ir ≤ 3% of the check phase's (at REG-16.1's ≈1,000 Ir per query that is the same bound; 1% would fail on names alone); the edges' bytes (count × 12 B, salsa's `QueryEdge`, plus memo overhead measured) ≤ 1% of the phase-start peak RSS; `decl_index` and its readers ≤ 50 ms per edit that adds, removes or renames a declaration; a signature edit to one method reruns only the files that read it (a count from the event callback); peak RSS ≤ the phase start; `heap_size` per class level recorded; the retained bytes of dead declarations after the replay series are at most 1% of the phase-start peak RSS (proposed), or H4.6's compaction brings them back under it; **cold-run Ir and wall time not above today's by a stated margin (proposed: 3% wall at p < 0.05 over 20 `abab` pairs)**; the walk/products overlap is kept only if it saves at least 20 ms of the warm load at p < 0.05 over 20 pairs | per-name slots for the index; `class_final` for members (H2.4's fallback); a collectable `DeclKey` with decision 3's sentence amended (#38); an engine of our own |
| S-4 | persistence | on the S-3 database, with the format named (`postcard`, one whole buffer, behind the envelope of H4.6): serialize and restore with `Lazy` values and with the cache's container, both with the chain below every persisted memo (H4.6) and with the graph in our own file; growth over an edit-replay series; the baseline re-measured at S-4's commit | restore **wall time** (and Ir) against the per-file decode's wall share, which runs in parallel on the pool where salsa's `deserialize` is one sequential walk under `&mut` (Ir parity alone could still be a serial wall regression at the head of every warm run); verify-after-restore ms for no change, for one body edit and for one added file; serialize ms; bytes per memo and per edge; the `check_file` executions after a restore plus an edit equal the live session's; the replay-growth series bounded (the stated ratio of H4.6's compaction) | the container for values, salsa for the graph only; or the graph in our own file |
| S-5 | cancellation | a write while a long check runs; a panic inside a shared level with N readers; `$/cancelRequest` during a long inference; a 50 MB array-literal statement and a stalled read; the stack-guard case of H2.8 repeated with a `Cancelled` unwind; a 100,000-deep hierarchy and constant chain on a 256 KiB thread | unwinding within 50 ms of the write at the polls of H2.5; one internal error per distinct cause per reader, none lost, no hang; a cancelled request completes and its answer is dropped by the server; the deep chains do not overflow | more polls; the retry policy changed |
| S-6 | cycles | `hierarchy_cycles` on Piwigo and vendor; a shuffled-order property test; `cycle_result` constants and purity, with a fallback that calls the value-only query (H3.3); the capped fixpoint of H3.3 with the nested-wrapper fixture (`return $n ? [f($n - 1)] : 1;`), a mutual-recursion fixture entered from each member and a `$n + 1` fixture whose iteration count it records; the cyclic fixture of inference and purity (H3.8) and the global-variable shape of H3.7, each entered from every one of its queries, on one thread and eight | the pass ≤ 30 ms; the same edges in any declaration order; the three rules' results equal the old passes'; the same result from every entry point; no run reaches the safety net just under salsa's 200-iteration panic, and the `$n + 1` count is at most about 66 | the explicit per-constant DFS; a lower per-kind literal threshold in the cycle operator (H3.3) |
| S-7 | tree retention and frees | the batch run's load and check with analysed trees kept for the run, against the same trees bounded by `lru` and evicted by one thread between phases (`trigger_lru_eviction`), against freed by the checking worker; context trees in the scratch builder | peak RSS ≤ the phase start; Ir of the frees (mimalloc's cross-thread paths included) and wall time not above today's; a bounded variant is kept only if it lowers peak RSS by at least 50 MiB for each 1% of added wall time (the exchange rate stated in this section's measurement rules) | LOAD-5.2 is built as the ledger has it (W3) and S-7 chooses between freeing on the checking worker and a bounded eviction; or LOAD-5.2 is dropped explicitly in Part D (the trees kept for the run, as memory only); a check scheduled on the worker that parsed its file where the queue allows |
| S-8 | exactness | `Exact`, a result type per class of value, the arch rule; the count of `==` and derived-`PartialEq` sites on `Type` that rely on its semantic equality (H1.5b's stronger form) | a field change makes every result type unequal, once for a tracked field and once for a function result across a case-only edit; a semantic-equal union in another order is unequal | a manual `PartialEq` per result |
| S-9 | threads | the S-3 skeleton's check phase at 1, 2, 4 and 8 threads against today's | the speedup curve not below today's; memo contention under 5% of CPU; cold-run wall not above today's (S-3) | `lru`/sharding of hot queries; a read-through table per worker |
| S-10 | probes | `path_is_file` over `ProbeEpoch` on a file with 200 includes, warm, and the distinct probes counted on the corpora | warm `check_file` of an unchanged file verifies with zero stat calls (`strace -f -c`) without an epoch bump, and at most 5 ms for the 200 per bump | the probe's keys batched per directory |
| S-11 | lazy context loading | the S-3 skeleton with `locate` (H4.1) over `vendor/symfony`, `vendor/monolog` and a package with PSR-0 and one with a classmap, and a link below a PSR-4 directory: list, then read on demand; the files read against today's (all of them); `locate` against Composer's `ClassLoader::findFile` on `php` for every class the old loader found | the files read are those the analysed files reach through `locate`, transitively; the listing costs no more than today's discovery; `locate` agrees with `findFile`; each class the old loader found and no mapping reaches (a link included) is listed as the delta | that case is a proposed change to decision 14, and the listed files of that package are read eagerly only after that decision, or the listing is made lazy by directory |
| S-12 | spans or token indices (decision 4's last sentence; #3) | a scratch build over the corpus (Piwigo, two vendor projects, the stubs): the nodes of I1 once with `Span` fields and once with token indices, the token arrays a kept tree must then keep (kind plus start offset, about 5 B a token), the exact bytes of a kept tree under each, using the node and token counts of step 0's census, and the readers' hot paths under each (`span()`, an identifier's name, a string's bytes, the infer cache's key) as callgrind Ir over the checker's reads of a tree; the layout of I1 is otherwise held fixed | token indices are chosen only if the bytes they save in the kept trees (net of the token arrays they must keep) reach the exchange rate of this section against the readers' added Ir in the check phase (50 MiB for each 1%, proposed): on trees of the plan's estimated 35 to 60 MB that needs a reader cost near zero, which is why byte spans are the expectation; otherwise byte spans, which B3 to B5 and I1 assume | spans, the measurement recorded as the answer to decision 4's last sentence; tokens, a change confined to B3's `span` fields, the retained token arrays of I2 and `Rooted` |

If S-2 or S-3 fails, the plan's contract (H2.1) is implemented by an engine of our own: memo tables with revision
stamps and dependency lists, early cutoff, a cancellation flag and cycle handling. Nothing above the engine
module changes, which is why only that module names salsa.

## Decisions

Churn and effort are not weighed: each answer is what is correct and complete. Each answer is in the part that owns the
design it changes, and no two parts answer the same question differently. The rulings (the nine decisions, the deltas of #42
and the audit's answers) are carried by their answers. The other answers are the plan's, on the same footing: the most
proper course for each.

Decisions in force: the fixes plan's decisions 1 to 7 and the ledger's decisions 1 to 18, which the plan cites by number. Gates, not decisions: the go, and the start condition in the overview. Batch
numbers are the order of the README's "The sequence". An answer that departs from a decision's wording says so.

Each decision is a `### #n` section of the part that owns the design it changes; the index is below.

| # | decision | part |
|---|---|---|
| 1 | D1: what consumers hold | [`07-part-b.md`](07-part-b.md) |
| 2 | D2: how to migrate | [`07-part-b.md`](07-part-b.md) |
| 3 | D3: position | [`07-part-b.md`](07-part-b.md) |
| 4 | D4: payload bytes | [`07-part-b.md`](07-part-b.md) |
| 5 | D5: names in nodes | [`07-part-b.md`](07-part-b.md) |
| 6 | D6: the mutating passes | [`07-part-b.md`](07-part-b.md) |
| 7 | D7: fabricated nodes | [`07-part-b.md`](07-part-b.md) |
| 8 | D8: rows | [`07-part-b.md`](07-part-b.md) |
| 9 | D9: error nodes | [`07-part-b.md`](07-part-b.md) |
| 10 | SYN-3.2's SIMD | [`14-part-c.md`](14-part-c.md) |
| 11 | the parse-recovery batch | [`14-part-c.md`](14-part-c.md) |
| 12 | order of items 13, 16, 15, 14 | [`14-part-c.md`](14-part-c.md) |
| 13 | oracle tools | [`06-part-f.md`](06-part-f.md) |
| 14 | DOC-2.1 | [`14-part-c.md`](14-part-c.md) |
| 15 | CACHE-13.1 for context files | [`05-part-d.md`](05-part-d.md) |
| 16 | item 18 | [`05-part-d.md`](05-part-d.md) |
| 17 | CHECK-9 and TYPE-5 | [`14-part-c.md`](14-part-c.md) |
| 18 | when to reevaluate | [`14-part-c.md`](14-part-c.md) |
| 19 | order of the wall | [`05-part-d.md`](05-part-d.md) |
| 20 | rows for C6 to C9 and D to G | this file, below |
| 21 | Stages D to G | [`05-part-d.md`](05-part-d.md) |
| 22 | level 3 and the Q-11 follow-ups | [`06-part-f.md`](06-part-f.md) |
| 23 | Part A's validation | [`02-part-a-intro.md`](02-part-a-intro.md) |
| 24 | item 9's validation | [`07-part-b.md`](07-part-b.md) |
| 25 | LOAD-2.8's parts 1 and 2 | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| 26 | the LSP completion probe | [`07-part-b.md`](07-part-b.md) |
| 27 | the step 1 codemod | [`07-part-b.md`](07-part-b.md) |
| 28 | LOAD-5.2 batch-only | [`05-part-d.md`](05-part-d.md) |
| 29 | NAME-2.1's thin name | [`14-part-c.md`](14-part-c.md) |
| 30 | where Part B lives | [`07-part-b.md`](07-part-b.md) |
| 31 | an analysed file with parse errors | [`14-part-c.md`](14-part-c.md) |
| 32 | `PhpVersion` and its types move out of `types` | [`14-part-c.md`](14-part-c.md) |
| 33 | the engine crate and `unsafe_code` | [`05-part-d.md`](05-part-d.md) |
| 34 | positionless declarations | [`14-part-c.md`](14-part-c.md) |
| 35 | PROD-5.7 and `deps::Collector` | [`14-part-c.md`](14-part-c.md) |
| 36 | the server's snapshot model | [`05-part-d.md`](05-part-d.md) |
| 37 | the persisted database's key | [`05-part-d.md`](05-part-d.md) |
| 38 | REG-16.2's "never reused" | [`14-part-c.md`](14-part-c.md) |
| 39 | LOAD-5.1's passes | [`05-part-d.md`](05-part-d.md) |
| 40 | the composer crate's B8 follow-ups | [`04-part-a2-to-a8.md`](04-part-a2-to-a8.md) |
| 41 | rows whose code a later task replaces | [`05-part-d.md`](05-part-d.md) |
| 42 | deltas the ledger marks READY or OPEN with half a ruling | [`05-part-d.md`](05-part-d.md) |

### #20 rows for C6 to C9 and D to G

**A docs pass at approval, in one commit series, edits `CRATE_SPLIT_PLAN.md` itself** so there is one source of truth.
The series writes the rows of J2 (OVL, RES, EXT, SUM) and the D and E rows of J3, and marks each superseded paragraph
in place, pointing at the section that replaces it:

- Appendix F's "each level is a per-class `OnceLock`" and "constant reference cycles type as `mixed`" (H3.1, #1);
- C4's "a change to any reflection input resets every memo" (the recorded reads);
- F1's published snapshots (H4.3's cancellation) and its text on line starts (the PHP terminator rule is applied in the
  lexer too: I3);
- F2's resolved dependencies, fingerprints and entry sets (H4.4);
- §3.9's text about the deleted `lsp/workspace_config.rs`;
- the line "a file with parse errors is still not analysed" (around line 6883), superseded by #31;
- G's "26 new packages" count and Appendix A, which gain the engine and `php-version` (J3);
- B8's composer follow-ups (`config.vendor-dir`, Composer's constraint grammar, `psr-0`, the root package's autoload,
  a package's bundled `vendor/`) point at A1, A7, A8 and #40 once they are built; the clamp of an open upper bound to
  `MAX_PHP_VERSION` is marked superseded;
- C6 to C9 and D to G gain the rows of J2 and J3.

Effort is no reason to leave stale text that misleads until its phase is reached.

*Needed before:* approval

