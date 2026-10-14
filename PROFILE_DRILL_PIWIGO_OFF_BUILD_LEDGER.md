# Off-run drill: build ledger

Tracks the tasks in `PROFILE_DRILL_PIWIGO_OFF.md`: the proposals, the cuts,
the defects, the decisions that gate them, and what the report leaves
unmeasured. The report holds each task's design, its evidence and its
adversarial checks; this file holds only the state and the order. Nothing
here is implemented yet.

Status: OPEN, DONE (hash here), SKIP (reason). A row's status changes in the
commit that finishes it.

Section numbers (§), proposal numbers, and the ids Dn, Bn, Cn, En and Nn are
the report's (§13 lists the ids' methods and data). "Item n" is an item of the
20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`, Part II).

**Rules for every row.**

- One commit per row, made as soon as the row's own check passes (pathspec
  commit).
- A row that changes behaviour or layout keeps the Piwigo output byte-identical
  to the base commit's (`LC_ALL=C sort` both before diffing), unless the row
  says why it cannot.
- A saving is claimed only from a paired warm-cache measurement (`gate`
  profile, alternating A B A B, `nice -n 19`, `/proc/loadavg` noted). The
  figures below are the report's estimates and not results.
- Builds, profiling runs and `check-project` probes need the user's go-ahead.
- Heavy jobs run one at a time.

## Decisions that gate the build

| # | status | decision | gates |
|---|---|---|---|
| D7 | OPEN (recommended, §10) | Build proposals 1-5 now, in the order of the build order below, since the crate split removes the eager build they speed and every cold run pays it until then. Alternative: only proposal 4 and E1's fix. | P1-P5 |
| D6 | OPEN (recommended, §10.8) | A context file that fails to parse keeps what it declares: what parses is recovered, what failed is unknown; validity follows the configured PHP version; the batch run and the language server use one rule. Alternative: keep today's policy (the file's declarations are absent). | P8, parser gaps |
| D3 | OPEN (reopened by C8, §10.6) | How a binary carries the stub data. Decided again once the crate split's C5 sets its version key, on the numbers in §10.6 (≈0.2-1.5 G per cold run once C4 and C5 land, against 5.05 G today). Proposal 6 is not built in today's form. | P6 |
| D1, D2, D5 | DONE (decided 2026-10-13, report) | D1: every skimmed body is checked and each file's verdict cached. D2: a skimmed body's text is copied at load from the bytes the load reads. D5: each cache checks once, at its first store, whether it can write. | P8, P2, E13 |

## Build order (D7)

1. **E1's fix**, which carries proposals 1 and 5.
2. **D5's write check**, which carries proposal 2.
3. **Proposal 3.**
4. **Proposal 4.**

Each step is measured with paired cold runs. B4 puts the ceiling at the 0.37 s
the warm stub cache saves today. The cuts, proposals 7-11 and the defects E14
and E15 are independent of this order unless a row's "needs" says otherwise.

## Defects in today's code (§10.14)

| id | status | task | verify |
|---|---|---|---|
| E1 | OPEN | The stub state becomes a value owned by the entry point (S1), built per layer on a thread of its own with the wait blocking in the OS (S2), on the pool the entry point hands in (S3), with the base chosen at `expect_files` (S4). Production code has no static for it. | The regression test with `PHPSTAN_RS_NO_STUB_CACHE=1`, a one-thread pool and two nested `Project::new()` calls, with a watchdog (designed, not run); `cargo test --lib` cold; fixtures. Re-entry impossible by construction. |
| E14 | OPEN | Read `instanceof`'s right side with `parse_new_class_reference`: a plain name stays `InstanceofTarget::Name`, anything else becomes `InstanceofTarget::Expr`. | A parser test per form; the two files (`AbstractConfigurationProvider.php:41`, `BigInteger.php:177`) as fixtures. |
| E15 | OPEN | Add `vendor/phpstan-src/stubs` to the inputs `crates/phpstan-rs/build.rs` hashes (and master's `build.rs`). | A test that the hash's inputs cover every path `stub-sources`' build script reads. |

## Proposals (§10.1-§10.11)

| # | status | task | estimated saving | needs | verify |
|---|---|---|---|---|---|
| P1 | OPEN | Resolve only the stub base the files need: the project starts with no base and takes one at `expect_files` (E1's S4). | −355.1 M serial; the plain base's finishing run ≈35-52 M (estimated) | E1 | Output identical on a project that mentions PHPStan and on one that does not; a file that mentions PHPStan only inside a body (P8) still picks PHPStan's base. |
| P2 | OPEN | Serialize only what a store keeps: gate `stub_cache::store`, `StubIndex::store_cached`, `Registry::store_cached` and the stub-data store on D5's write check. | −1,005.3 M serial; ≈ −0.71 to −0.74 G after P1 | D5's check | A cold run still checks `store_entries`' duplicate-name test; an unwritable cache directory pays no encoding. |
| P3 | OPEN | A hash index for stub bases built in memory: `freeze` builds the index `store_entries` already builds. | ≈ −0.29 G | — | A built-then-frozen base answers every lookup as the bisection did (to be checked by a test). |
| P4 | OPEN | Look each stub file's dependents up once in `build_closures`: per-file dependent lists, closures in strongly-connected-component order. | ≥ −0.2 G serial | — | The closures equal today's, file by file (to be checked by a test). |
| P5 | OPEN | Build the stub corpus beside the parse: E1's S2 and S3; the corpus's `par_iter` moves into `worker_pool().install`; `check-project` makes the stubs before listing files. | wall time: ≈0.14 s alone, the whole ≈0.25 s stretch with P1 (a model); no Ir change | E1 | Paired wall-time runs; `PHPSTAN_RS_THREADS` and the 64 MB stack apply to the corpus. |
| P6 | OPEN, blocked | Build the stub data at compile time into every binary. Not built in today's form. | −5.05 G today; ≈0.2-1.5 G per cold run after the crate split's C4 and C5 | D3; the reflection crate (C10) | If built: B10's guard test, an embedded-equals-runtime-build test, the override equal to the profile (B9). |
| P7 | OPEN | Tokenize a PHPDoc tag's description only when it is read: the parser pulls tokens, records the description's offset, and re-lexes on request; `[optional]` becomes a three-token prefix test. | ≈ −0.37 G off, ≈ −0.08 G warm (ceilings); ≈ −0.04 G after P6 and item 14 | — | Re-lex, never slice (`//`, string escapes, floats, overflowing integers, single-space join); every phpdoc test; the memo keys unchanged. |
| P8 | OPEN | Skim the function bodies of context files and parse a body when something reads it: skim flag in the parser, lazy summary and flow in `OnceLock`s, bytes copied at load (D2), every body checked by a reduced pass with a per-file verdict cache keyed by the file's bytes and the `syntax` crate (D1). | ≈ −2.0 to −2.3 G with the verdict cached; ≈ −0.55 to −0.85 G without | P9; D6 for failed parses | A debug build parses every skimmed body eagerly and asserts equal summaries, flows, declarations, global facts and references; verdict cache keyed as `build.rs` keys the source hash; a panic is not a verdict. |
| P9 | OPEN | Purity and property writes on demand: a memoized depth-first search from the declaration asked about (least fixed point), the memo cleared on each edit; `check_declared_purity_tag` tests the `@phpstan-impure` / `@impure` tag first. | ≈ −0.3 to −0.4 G beyond item 4 | item 4's ids | A debug build runs the eager pass and compares every answer given on demand; order of askers does not matter. |
| P10 | OPEN | Check UTF-8 with SIMD (`simdutf8`). | ≈ −22 M (estimated) | — | The check stays; the `LineIndex` `Utf8Chunks` is item 11's. |
| P11 | OPEN | (a) A reported message formatted when displayed (a template and its operands in `Diagnostic.message`); (b) class-constant keys built only when a dynamic constant is configured. | 22.5 M (a), 6.6 M (b) | — | A message's text must not depend on state that changes after the report; the baseline and `ignore_errors` compare the displayed text; (b) folds the class name as `ConstantResolver::new` does. |

## Cuts the investigations found (§10.13)

| id | status | task | estimated saving | needs |
|---|---|---|---|---|
| E2 | OPEN | A statement's assigned and defined names walked only when it has `@var` tags. | ≈ −50 M | — |
| E3 | OPEN | A method or static call probed for a PHPUnit assertion only when its written name starts with `assert`. | ≈ −35 M | — |
| E4 | OPEN | The analysed files stored in the per-file cache too. | ≈ −0.25-0.3 G net, per-file cache warm | — (debug check that a hit's products encode to the entry's bytes) |
| E5 | OPEN | A kind code in the type node's free flag bits. | ≈ −30 M | — |
| E6 | OPEN | One node against itself answered first in `is_super_type_of`. | ≈ −10 M net | — |
| E7 | OPEN | Plain object types kept with their class, two statics, a table of small integers. | ≈ −55 M | item 4's slot; item 16 overlaps |
| E8 | OPEN | A cache file's types stored once, in a table its records index. | ≈ −65 M; ≈ −25 M after item 6 | item 6, steps 3 and 5 |
| E9 | SKIP (folded into the plan's F6f, `PROFILE_DRILL_PIWIGO_20G.md:1705`) | The regex scanner's concatenation. | — | — |
| E10 | OPEN | Nothing reported while an enclosing body has called `extract()` or `eval()`. | ≈ −35 M on Piwigo | — (debug assertion that the truncation drops exactly what the count skipped) |
| E11 | OPEN | A cached file's origins decoded as the one `Arc<Path>` its file binds. | ≈ −116 M, per-file cache warm | E8's decode context |
| E12 | OPEN | Each class's assert-tagged method names carried in its `BuiltDecl`. | ≈ −0.35-0.4 G serial, per-file cache warm | — |
| E13 | OPEN | The per-file cache's shard directories made once, by D5's check; the process id read once. | ≈ −80 ms kernel CPU, ≈ −10 ms wall | D5's check |

## Left unmeasured or untraced (§12, §10.8, §10.11)

| # | status | what | how it would be settled |
|---|---|---|---|
| U1 | OPEN | The parser against the whole corpus: every file PHP accepts and the parser refuses. E14 came from two warnings only. | One `phpstan-rs check` over the 111,836 files under `vendor/`, against D6's `php -l` verdicts (`~/.cache/prs-probe/d6/`). A run of the binary: needs the user's go-ahead. |
| U2 | OPEN | Parser gaps already known (D6): `$s{0}` (valid in PHP 7; the postfix loop has no arm for `{`, `syntax/src/parser/postfix.rs:16-241`); `(real)` and `(unset)` accepted (`syntax/src/ast.rs:1303`, `:1311`) though PHP 8 refuses them. | Fix with the parser, with tests; decide per gap whether the parser follows the configured PHP version (D6). |
| U3 | OPEN | Which of C7's 238 compile-time errors the parser does not report the checker already reports. | Count by running each of the 299 trigger files (`~/.cache/prs-probe/c7/cases/`) through `check`; the verdicts are in `c7/verdicts.tsv`. |
| U4 | OPEN | E1's deterministic reproduction. | Designed in §10.14, not run; becomes E1's regression test. |
| U5 | OPEN | E10: which facts that outlive an `extract()` body (property assignments, class-level sites) a dropped body still needs. | Trace in `function_decl.rs` and `checker.rs` before widening E10 past the report calls. |
| U6 | OPEN | Which truncation holds the 33 `offsetAccess.mightNotExist` reports that `finalize` does not drop (B3 counts them among the discarded reports but not by site). Decides whether P11a's truncation saving is "well under 1 M" or ≈13 M. | Record the truncation site in the B3 probe (`~/.cache/prs-profile/off-ledger-b/probe.patch`); needs a probe build. |
| U7 | OPEN | Readers left out of d: the uninitialized-property rules on, or a tree with test classes whose built-in parent-call hook rules fire, add the ancestors' bodies to d. | Re-run B1's probe on such a tree; needs a probe build. |
| U8 | OPEN | B9's embed and edit timings were read off the console and not kept; the build script's cost under C8's form (every stub file parsed, no base) is not measured; nothing ran on aarch64 or macOS (B10's backstop is designed, not run). | Only if P6 is revived (D3). |
| U9 | OPEN | A third pass on the report's claims the review workflow could not check (102 items). | Hand check against the code and the profiles, as the 104 confirmed and the 17 split findings were. |

## Sequencing notes

- The ledger of the 20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`) is separate; its
  items are inputs here (items 4, 6, 14, 16 appear as "needs"), not rows.
- A row that lands changes the report's figures: record the measured number
  here as `DONE (hash; measured X)` and fold it into the report's text when it
  is next edited, not as a second source of truth.
- `WORK_STACK.md` names the order; update it when a row finishes or the order
  changes.
