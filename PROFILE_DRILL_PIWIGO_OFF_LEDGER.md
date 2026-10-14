# Off-run drill ledger

Tracks what `PROFILE_DRILL_PIWIGO_OFF.md` leaves open: the decisions it
leaves to the user, and the questions it could not settle. The proposals
themselves are in the report (§10). This file lists only what still needs a
decision or a look before they can be built or judged.

Status: OPEN, DONE (hash or answer here), SKIP (reason).

Section numbers (§) and proposal numbers are the report's. "Item n" is an
item of the 20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`, Part II).

## Decisions

| # | status | decision | options | gates |
|---|---|---|---|---|
| D1 | DONE (D1 below) | Syntax errors inside a skimmed body (§10.8). Today a context file that fails to parse is skipped with a warning and its declarations are lost. | 1. Load the file anyway, and report the error only when something reads the body. 2. Check skimmed bodies with a pass that builds only what its checks read: today's behaviour, with ≈1.3-1.5 G of the 3.8 G saved, and more once each file's verdict is cached. | proposal 8 |
| D2 | DONE (D2 below) | Where a skimmed body's text comes from when it is read later (§10.8). | Keep it in memory (≈19 MB of body text for Piwigo's context files), or read it again from disk with a hash check. | proposal 8 |
| D3 | DONE (D3 below) | How a binary carries the stub data (§10.6). | 1. Built at compile time by a build script, and embedded. 2. Appended to the binary by the release step. 3. Packaged beside the binary. | proposal 6 |
| D4 | DONE (D4 below) | What a binary built from source (`cargo install`, `cargo build`) does, since it carries no cache unless its build makes one (§10.6). | | proposal 6 |
| D5 | DONE (D5 below) | A cache directory that exists but can't be written (§10.2). | Leave the gap, where the encoding is still paid, or detect it. | proposal 2 |
| D6 | OPEN | What a context file that fails to parse contributes (§10.8). Today it loses every declaration. That is right when the error is real and wrong when it is a gap in our parser, and the analyser cannot tell which. | 1. Keep today's policy: its declarations are absent. 2. Keep every possibility: what the file declares is possibly declared, neither known nor missing. Not designed. | none: it holds whether or not proposal 8 is built |
| D7 | OPEN | When to build proposals 1-5 (§10, §10.6). Once proposal 6 builds the stub data at compile time, they speed only the off mode and the reflection crate's own tests, and proposal 4 also the build script. Proposal 6 waits on the crate split's reflection layer. | 1. Build them now: every cold run gains until the crate split lands. 2. Build only proposal 4 and E1's fix, and leave the rest to the off mode. | proposal 6's place in `WORK_STACK.md` |

### D1. Option 2, with each file's verdict cached

- **Decided** 2026-10-13. Every skimmed body is checked, by a pass that builds only what its checks read. A cache keyed by the file's bytes and the `syntax` crate holds each file's verdict, so a file is checked once per parser version (§10.8).
- **Why not option 1.** PHP refuses the whole file for one bad body: none of its classes or functions is declared, for a parse error and for a compile-time error ("Cannot mix keyed and unkeyed array entries"; php 8.5, `~/.cache/prs-probe/syntax-in-body/`). With option 1, whether a file loads would depend on whether an optimization skipped one of its bodies. Option 2 keeps one policy for every file. That policy itself is D6.
- **Every error is the parser's or the lexer's.** `syntax::parse_watching` is the only step of a load that fails (`project.rs:2892`), and a lexer error anywhere in the file outranks the parser's first error (`syntax/src/parse.rs:106-113`). No later pass over the tree reports one.
- **Corrections the adversarial pass made:**
  - **The pass cannot build nothing.** The parser checks destructuring after it builds the array, with sets keyed by node id (`syntax/src/parser/expr.rs:402-497`: `long_arrays`, `pattern_only`). It keeps array shapes and node ids.
  - **It saves less than first estimated:** ≈1.3-1.5 G before the plan and ≈0.6-0.9 G after it, not 1.5-2 G. Tree building is at most ≈0.53 G of the parse job's 3.65 G parser block, and the token pipeline (≈1.05 G) and the grammar stay in the pass. The report's §10.8 has the table.
  - **A body that is read is checked and then parsed.** So without a cached verdict, the saving is S − F × d, not S × (1 − d). After the plan, that is nothing from d ≈ 0.26-0.45. With the verdict cached, it is F × (1 − d): ≈2.0-2.3 G × (1 − d) after the plan.
- **The verdict cache's key** is sound (C6).

### D2. Kept, copied at load from the bytes the load reads

- **Decided** 2026-10-13 (§10.8).
- **No read of its own.** Every load reads the file's bytes, if only for the per-file cache's key (`session.rs:631-637`). The copy is taken from them, so a body parsed later reads what the load checked.
- **Lexing a body alone.** `Lexer::fragment(src, base)` (`syntax/src/lexer.rs:461-473`) lexes a piece with spans absolute in its file. It needs a bytes variant for files that are not UTF-8.
- **Memory.** About half of each context file is body text (§5), so ≈19 MB of Piwigo's ≈37 MB. The peak is 866 MB, of which ≈420 MB are the context files' trees (the 20 G report's M1). The language server already keeps every file's text (`lsp.rs:294`).
- **Rejected.**
  - Reading the file again on demand: it may have changed since the load, and a hash check only detects that.
  - Mapping the file: the same problem, and it needs `unsafe`.

### D3. Option 1: the stub data built at compile time, into every binary

- **Decided** 2026-10-13 (§10.6). It is the only option that reaches every binary:
  - release binaries;
  - binaries built from source (D4);
  - test binaries;
  - every way the VS Code extension finds one (`serverPath`, `PATH`, a download).

  Options 2 and 3 reach release archives only. The data is a function of the source alone: the build reads no setting and no environment variable besides the cache's location (`stub_cache.rs:33-44`), and `switch_base` applies the PHP versions after the load.
- **Conditions found by the adversarial pass:**
  - **The crate split's reflection layer.** A build script cannot call the crate it builds, and must run the very code the runtime runs. A stub base is built through `Project::finish_loading` → `run_finishing_passes` (`project.rs:1387-1399`), and the registry uses the checker (`registry/init_flow.rs:13`). So proposal 6 lands with level 2 of `WORK_STACK.md`.
  - **A test on every release target that the embedded data equals a runtime build.** Two release legs cross-compile (`release.yml`), and constant folding of `**` calls the platform's `pow` (`types/src/arithmetic.rs:715-717`). `folded_hash` (`syntax/src/name.rs:997`) is the same everywhere. B10.
  - **Build settings.** A `[profile.*.build-override]` opt-level for the build-dependency (the test profile's comment: 4-6× faster at opt-level 1). The 31 MB `include_bytes!` goes in a small crate of its own, so the main crate's incremental builds do not embed it again.
  - **A borrowed `Reader`**, since `Reader::new` takes an owned `Vec<u8>`.
- **Correction.** The data is not read in place from a file: it is a static. `.rodata` grows from 18.2 MB to ≈50 MB, in the release binary and in each test binary that links it. A run loads only the pages it touches. The compile-time cost is B9.
- **What it changes elsewhere.**
  - A run builds the stubs only in the off mode (`PHPSTAN_RS_NO_STUB_CACHE`, which comes to mean "build at run time") and in the reflection crate's own tests.
  - E1's hazard narrows to those runs and to the build script, which runs on a thread of no pool. It does not go away.
  - Proposals 1-5 lose most of their reach (D7).

### D4. Settled by D3

- The crate is not published (`publish = false`), and the README's route for a binary other than a release one is building from source.
- Once D3 lands, every binary built that way carries the data.
- Until then, such a binary pays one cold run per build, as today.

### D5. Detect it: one check, at the first store, apart from reading

- **Decided** 2026-10-13 (§10.2).
- **The check.** It runs lazily, at the first store, and is kept in a `OnceLock` of its own. A fully warm run stores nothing and checks nothing.
- **Reading stays on.** A directory that cannot be written still serves what it holds.
- **The gate comes before `CachedFile::of`**, which builds the entry before it is encoded (`session.rs:765-771`), so a file that will not be stored pays neither.
- **Reports.**
  - One warning when the cache cannot be written.
  - One more on a later write failure: entries sit in 256 shard directories (`file_cache.rs:177`), so one write can fail after the check passed.
  - A load refreshes an entry's mtime after 12 h (`file_cache.rs:190-196`), and a failure there passes silently today.
- **Which caches:** the per-file cache, the stub cache until proposal 6 removes it, and D1's verdict cache.
- **Stating intent.** A cache that is read-only on purpose (one restored in CI, say) would get a note on every run. An explicit read-only setting states the intent and silences it.

## Investigations that need a build

None of these was run for the report.

| # | status | question | settles |
|---|---|---|---|
| B1 | OPEN | What proposal 9's on-demand search reaches, on Piwigo and on a general tree: the distinct callees the no-op rules and the property-write reads ask about, and the nodes the searches visit (§10.9). This also gives d, the share of bodies read. §11 assumes d ≈ 0.2. | proposal 9's saving; proposal 8's saving: ≈2.0-2.3 G × (1 − d) after the plan with each file's verdict cached, and without it whether proposal 8 pays at all, since it saves nothing from d ≈ 0.26-0.45 (D1) |
| B2 | OPEN | Proposal 7's real saving. The estimate counts every tag, not every distinct tag, so the memos may lower it (§10.7). | proposal 7 |
| B3 | OPEN | How many reports the loops' convergence passes format and then discard (`diagnostics.truncate`: `checker.rs:1278`, `:1807`, `function_decl.rs:1209`; §10.11). | how far proposal 11a reaches |
| B4 | OPEN | Wall time. Nothing is timed, including the 1.6 G of serial stub work on a cold run's critical path, with the other cores idle (§12). | the wall-time weight of proposals 1-5 |
| B5 | OPEN | A run with the per-file cache warm. All three runs had it off, so its floor and `from_def`'s cost with it on are unknown (§11, §12). | the floor of the per-file-cache-warm mode |
| B6 | OPEN | Disk time of the cold run's cache write. Its 58 M is callgrind's count only (§12). | the cold run's full cost |
| B7 | OPEN | How many of `is_super_type_of`'s 1,341,821 questions ask about one node against itself. The profile counts calls, not their operands (N2). | E6's saving |
| B8 | OPEN | How many of `Type::object`'s 170,696 calls build a class's plain object type, with no arguments, and how many of `int_literal`'s 71,550 build a small integer (N2). | whether E7 is worth building |
| B9 | OPEN | What building the stub data at compile time adds to a build: the reflection crate compiled again for the host, and the stub build in the build script at the `build-override` opt-level, for a clean build and for an incremental one after an edit to the reflection crate (§10.6, §12). | proposal 6's cost to development (D3) |
| B10 | OPEN | Whether the stub data built for a release target equals the data that target builds at run time, on the two cross-compiled legs (`aarch64-unknown-linux-gnu` on x86_64, `x86_64-apple-darwin` on arm64). Constant folding of `**` calls the platform's `pow` (`types/src/arithmetic.rs:715-717`). This becomes the test that D3 requires, run natively or under emulation (§10.6, §12). | whether the cross-compiled legs can embed what their build script makes |

## Investigations in the code or the data, no build

| # | status | question | settles |
|---|---|---|---|
| C1 | DONE (C1 below) | The tag lexer (`phpdoc/src/grammar/lexer.rs`): does any lexer state cross the point where the parser would stop at a description? If it does, a fresh lexer started at the offset gives different tokens (§10.7). | whether proposal 7's design holds |
| C2 | DONE (C2 below) | Deadlock through `install` (§10.5): can a stolen job wait on the stub corpus's own `LazyLock` when `install` is called from a third pool's thread, such as the one the test at `incremental_updates.rs:1155-1175` makes? | whether proposal 5 is safe as designed |
| C3 | DONE (C3 below) | Why encoding costs differ per byte between the two stub bases (§10.2). The file sizes (5.27 MB against 15.33 MB) would put the plain base's stores at about a quarter of 814.9 M, below the 0.26-0.36 G bound the code gives. Unexplained. | the overlap of proposals 1 and 2 |
| C4 | DONE (C4 below) | Which stub base each finishing run served (§10.1). One run cost 117.2 M on its calling thread; the profile can't say which base it was. | proposal 1's saving on the pool |
| C5 | DONE (C5 below) | The ≈211 M for the two stub bases' finishing runs (§4.4) is an estimate, because stolen jobs move cost between blocks and the profile has no calling context (§12). | proposals 1 and 3's share of the finishing passes |
| C6 | DONE (C6 below) | Whether a file's parse verdict depends on anything besides its bytes and the `syntax` crate (§10.8, D1). If it does, a verdict cache keyed by those two can go stale. | whether D1's verdict cache is exact |
| C7 | OPEN | Which of PHP's compile-time errors the parser does not report. A `goto` to a label that does not exist is one: PHP refuses the file (`~/.cache/prs-probe/syntax-in-body/goto.php`), but the parser only records labels (`GotoScope`, `syntax/src/parser.rs:103`), so the file loads. The list of PHP's compile-time errors (`zend_compile.c`) against the parser's errors gives the rest. | how far a failed parse matches PHP's, with or without proposal 8 (D6) |

### C1. The tag lexer carries no state across a token boundary

- **The lexer's state** is its text and one byte offset (`Lexer { src, pos }`, `lexer.rs:46-50`). It has no mode, no nesting depth and no look-behind: `peek_at` looks only forward, and every token starts after `skip_whitespace` at `pos` (`:177-187`). A fresh lexer started at a token's start offset therefore gives the same tokens from there on, with spans shifted by that offset. Proposal 7's design holds.
- **What the pulling parser must still keep:**
  - **Rewinds.** The parser resets `pos` to a checkpoint in many places (`parser.rs:229`, `:452-456`, `:626`, `:897-910`, `:961-976`, `:1047`, `:1147`). The lexed tokens have to stay in a buffer, and the description's offset is the start of the token at the parser's final position, not the furthest token lexed.
  - **Lookahead to the end.** `looks_like_html` (`parser.rs:868-890`) scans every remaining token for a closing `</name>` once a type is followed by `<name>`. In that case the parser lexes to the end of the tag, as it does now.
  - **Spans.** `preceded_by_whitespace` and `is_adjacent_to_previous` (`:636-643`, `:850-852`) compare the spans of neighbouring tokens. They work unchanged on a buffer.
- **Re-lexing on request, not slicing, is needed for more than `//`.** `token_text` (`parser.rs:1235-1262`) drops a string literal's quotes and its `\` escapes, writes a float in Rust's `{:?}` form (`1.50` becomes `1.5`), writes an integer that overflows `i64` as `0`, and `remaining_description` joins the tokens with single spaces (`:305-312`). A raw slice would differ in each case.
- **The memo.** `parse_tag_rest_shared` shares a value between equal tag texts. A value that keeps an offset must keep its tag text (or an `Arc<str>` of it) to make the description on request.

### C2. The deadlock exists today, without proposal 5

A thread that waits inside rayon runs other jobs while it waits. If one of those jobs needs a lock that a frame lower on the same stack holds, the thread blocks itself.

- **How rayon waits.**
  - A `par_iter` on a worker thread runs on that worker's own pool. While it waits in a join, the worker takes jobs from the deques of the other workers of that pool (`rayon-core-1.13.0`, `registry.rs:780-842`: `wait_until_cold`, then `find_work`, then `steal`).
  - An `install` into another pool from a worker thread (`in_worker_cross`, `registry.rs:543-563`) also waits with `wait_until`, so it also runs the calling pool's jobs while it waits.
  - Only a thread that belongs to no pool blocks without running anything.
- **The test's scenario.** At `incremental_updates.rs:1155-1175`, the groups run on the test's own pool T, each through `assert_edits_match_fresh_loads`, which makes projects. Suppose the stub cache is cold or off, and a worker tA of T is the first thread to force a stub value:
  1. `Project::new` → `plain_base` (a `LazyLock`, `project.rs:537-541`) → `resolved_stubs(false)` → `builtin_stubs` → `generated_stub_data` (a `OnceLock`, `stubs.rs:1444-1450`) → `built_stub_corpus` (a `LazyLock`, `:1420-1429`) → `build_generated_stub_data`.
  2. Its `files.par_iter()` (`stubs.rs:1482`) runs on T, because tA is a worker of T. While it waits, tA can take another group's job from T.
  3. That job calls `Project::new` → `plain_base`, whose initialization tA itself is running lower on its stack. tA waits on itself. `OnceLock`'s documentation leaves re-entrant initialization unspecified, and the current implementation deadlocks.

  The other workers of T block on the same locks. They block in the OS, not in rayon, so they cannot help either.
- **A second path, through `install`.** `resolved_stubs` then calls `finish_loading` → `finish_declarations` → `worker_pool().install(...)` (`project.rs:1395-1399`), still under the base's `LazyLock`. From tA this is `in_worker_cross`, so tA again runs T's jobs while it waits. A stolen group that needs the base being built blocks, either directly (`phpstan_base`, when its files mention PHPStan) or after starting the other base, whose own `install` steals again.
- **When it can happen.** Only when a base or the corpus is built, not loaded: with the stub cache cold (the first run after a source change) or off. `resolved_stubs` returns before any parallel work when it loads the base from the cache (`project.rs:572-577`). Whether a run deadlocks also depends on timing: tA has to steal a sibling group while its own work is still pending.
- **Proposal 5 adds a third path of the same kind.** Moving the corpus's `par_iter` into `worker_pool().install` replaces path 2's T-local `par_iter` with an `in_worker_cross` wait, which steals in the same way. It creates no new kind of deadlock and removes none. The hazard needs its own fix (E1 below), and proposal 5 is safe once that fix is in.

### C3. The plain base's stores cost more per byte because the two bases share their function tables

The two files were parsed with a script that walks the container's section directory and each table's 16-byte records (`container.rs:120-136`, `name_table.rs:1028-1066`).

- **Deduplication is not the cause.** `Writer::entry` writes equal entries once (`container.rs:92-109`), so a file is smaller than what was encoded. Counted per record, the plain base encodes 6.62 MB of entries (3.81 MB distinct) and PHPStan's base encodes 18.82 MB (13.45 MB distinct). The plain base's share is 26.0% of what was encoded, the same as its 25.6% share of the file bytes.
- **The cause is what each base holds:**

  | tables | plain: records, bytes encoded | PHPStan's: records, bytes encoded | store cost in the profile |
  |---|---|---|---:|
  | `functions`, `raw.functions` | 10,546, 1.30 MB | 10,564, 1.30 MB | `NameTable<FunctionReflection>::store` 137.8 M |
  | `classes`, `raw.classes` | 3,084, 4.40 MB | 6,990, 14.69 MB | `NameTable<ClassReflection>::store` 489.6 M |
  | the other 13 tables and sets | 26,753, 0.92 MB | 36,720, 2.83 MB | 185.2 M |

  - The function tables are the same in both bases (5,273 against 5,282 functions), and they cost ≈53 Ir per byte against ≈26 for the class tables. Half of their 137.8 M belongs to the plain base, although the plain base holds only a quarter of all the bytes.
  - The 8,259 global constants are also in both bases. The name-only tables cost per record, not per byte.
- **The plain base's stores** come to 68.9 M (functions) + 112.9-149.9 M (classes, split by bytes or by records) + 45.4-78.0 M (the others) + 0.6 M (`store_container`) = ≈0.23-0.30 G. With the §10.2 bound of at least 0.26 G, that is **≈0.26-0.30 G**.
- **Proposal 2 after proposal 1** therefore saves 1,005.3 M less that: **≈0.71-0.74 G**, against the report's 0.65-0.74 G.

### C4. Worker 09's 117.2 M run served PHPStan's base

The per-thread call counts were read from the raw callgrind files (`off/cg.out-NN`).

- **Three finishing runs.** The main thread called `finish_declarations` three times: twice from `resolved_stubs` (the two bases) and once from `Session::load` (the project). Worker 02 executed two of the injected jobs (2,069.1 M inclusive on its own thread), and worker 09 executed one (117.2 M).
- **The counts that identify them:**
  - **Trait flattening.** The closure that `flatten_traits_one_pass` runs per class (`resolve.rs:161-164`) was called 58,905 times on the worker pool: 1 × 1,542 + 3 × 3,495 + 3 × 15,626.
    - The plain base has no class that uses a trait (`trait_users` is empty in its file), so its first pass changes nothing and `flatten_traits` stops after one pass over its 1,542 classes (`resolve.rs:127-134`).
    - PHPStan's base has 30 trait users and takes three passes over its 3,495 classes.
    - The project takes three passes over its 15,626 classes.
  - **On worker 09,** that closure ran 10,485 = 3 × 3,495 times. So did the other per-class and per-function closures of that run: `resolve_deferred_value_of` 3,495, `classify_purity` 3,495 (classes) and 5,282 (functions), and `drop_doc_members_excluded_by_native` 5,282 (functions). These are PHPStan's base's 3,495 classes and 5,282 functions.
  - **On the other workers,** `classify_purity`'s class closure ran 5,448 times on worker 02 and 11,720 times on worker 04: 17,168 = 1,542 + 15,626. That is the plain base and the project.
- **So** worker 09 ran PHPStan's base's finishing run, and the plain base's run shared worker 02 with the project's run. Proposal 1, which removes the plain base, removes the run that is not the 117.2 M one (C5).

### C5. The two stub bases' finishing runs cost ≈0.15-0.17 G, not ≈0.21 G

- **PHPStan's base: 117.2 M, measured.** Nothing of its run ran on another worker: every closure counted in C4 ran on worker 09 exactly once per class or function of that base. The main thread was blocked in `install` and the pool had no other work, so everything under that frame on worker 09 is that run's.
- **The plain base: ≈33-52 M, estimated.** Its run shares worker 02 with the project's run, and the counts cannot separate their costs.
  - Its classes and functions are a subset of PHPStan's base (`builtin_stubs` filters out `entry.phpstan`, `stubs.rs:937-944`), so it costs at most about 117.2 M.
  - The passes' work is mostly per class. Scaled by class count (1,542 against 3,495), it is ≈52 M. Scaled by the class tables' bytes (4.40 against 14.69 MB), it is ≈35 M.
- **Together, ≈150-170 M** against §4.4's ≈211 M. The ≈40-60 M left over in §4.4's block differences is cost that stolen jobs moved between blocks, not stub-base work.
- **Proposal 1's saving on the pool** is the plain base's run: **≈35-50 M**, not the 117.2 M that §10.1 names.
- **Proposal 3** is unaffected. Its bisection (193.6 M in the finishing passes) was carved out of the blocks before the ≈211 M was summed.

### C6. A file's parse verdict depends only on its bytes and the `syntax` crate

- **The only step of a load that fails** is `syntax::parse_watching` (`project.rs:2892`, through `parse_with_ignores`). What `parse_file_with_names` does after it, the declarations and the references, returns no error (`project.rs:3003-3027`).
- **The watch list** is the one input from outside `syntax`, and it sets only bits of `facts.mentions` (`syntax/src/lexer.rs:416-419`), never an error.
- **No stub mode.** `is_stub` is read only after the parse, by `file_declarations::collect`.
- **No feature changes the parse.** `syntax`'s only feature, `fuzzing`, derives `Arbitrary` (`php_string.rs:13`, `name.rs:263`).
- **So the key** is the file's content hash and a hash of `crates/syntax` (its sources and manifest), the workspace manifest (the profiles), `Cargo.lock` (its dependencies) and the target, computed as `build.rs` computes the build's source hash.
- **A panic is not a verdict.** A parse that panics leaves the file unread (`session.rs:661-666`), and nothing caches it.

## Found by the investigations

| # | status | item | gates |
|---|---|---|---|
| E1 | OPEN, investigated (E1 below) | The deadlock of C2, present in today's code. The fix (S1-S4 below) and its test wait on a decision. The reproduction was not run. | proposal 5 |
| E2 | OPEN | Walk a statement's assigned and defined names only when it has `@var` tags. `check_stmt_impl` computes `statement_assigned_variable_names` (`checker/stmt.rs:628`) and, through `inline_var_annotations` (`:1562`) and `:1260`, `statement_defined_variable_names` twice, for every statement. Only the `@var` targets use them. ≈ −48 M warm, exact (N1). | — |
| E3 | OPEN | Probe a method or static call for a PHPUnit assertion only when its written name starts with `assert`, ignoring case (`checker/narrowing/phpunit.rs:86-116`). Every name `assertion_check` reads starts so. Keep the function-call arm: `use function … as` can rename those. ≈ −50 M warm (N1). | — |
| E4 | OPEN | Store the analysed files in the per-file cache too: their declarations, summaries and lines from the entry, their tree from a parse. Same key, `index_use_sites` included. A debug build checks the entry against a fresh build. ≈ −0.5 G on a run with that cache warm (N3). | B5; the F2 result cache, which would remove the same cost and more |
| E5 | OPEN | Kind bits on the interned node (scalar value, scalar family, mixed, never, object), so the relation's shortcuts and `is_form_of` read one byte. ≈ −40-50 M, an estimate (N2). | — |
| E6 | OPEN | Answer `sup == sub` (one pointer compare) before the structural memo and the registry-backed forms, not inside `is_super_type_of_by_form` (`types/src/relation/super_type.rs:114`) (N2). | B7 |
| E7 | OPEN | Keep a class's plain object type with its interned name, and a table of small integer nodes, so `Type::object` and `int_literal` skip the interner on a repeat (N2). | B8 |
| E8 | OPEN | Decode each stub-cache file's types once into a table the file's declarations index, instead of one `Type::new` per occurrence: ≈35-40 M left after items 4 and 6 (N2). | item 6, step 3 (the cache format's bump) |
| E9 | OPEN | The regex scanner's literal concatenation: extend the left string in place when both sides are one literal, `push_str` otherwise (`regex_groups.rs:390-401`), with F6f's one `String` per character (`:936`, `:1162`) (N4). | after F6f's memo, which removes ≈75% of the scans |

### E1. Where a stub build can deadlock, and the fix

**The locks.** Five process-wide values hold a lock while their first build runs:
- `plain_base` and `phpstan_base` (`LazyLock`, `project.rs:537-541`, `:1203-1206`);
- `built_stub_corpus` (`LazyLock`, `stubs.rs:1420-1429`);
- `stub_index` and `generated_stub_data` (`OnceLock`, `stubs.rs:1431-1450`).

Two kinds of parallel work run under them: the corpus's `par_iter` (`stubs.rs:1482`), and the bases' finishing passes, through `finish_loading` → `worker_pool().install` (`project.rs:1398`). No other static `LazyLock` or `OnceLock` in the crate runs parallel work.

**Two ways it fails.**
- **Re-entry.** A rayon worker that waits inside the build (in a join, or in an `install` into another pool) runs other jobs of its own pool. One that needs the same lock waits on its own thread (C2).
- **Starvation.** Waiters that are workers of pool P block in the OS. If every worker of P waits, and the build needs P, nothing serves the build. The comment at `incremental_updates.rs:1158-1162` describes this case: it is why that test runs its groups on a pool of its own.

**Who forces a stub value, and on which thread:**

| caller | thread | can start a build? | risk |
|---|---|---|---|
| `check-project`: `Session::load` → `Project::new`, `expect_files` → `switch_to_phpstan_base` | main | yes (cold or off cache) | none: main belongs to no pool |
| `check-project`: `finishing_scopes` → `rayon::join` of two `closure_scope`s → `stub_index()` (`project.rs:1290-1296`) | a worker of `worker_pool`. The warm profile shows `stub_index` first forced by `closure_scope` on worker 02, with worker 04 blocked on the same `OnceLock` | only when the bases load from the cache but `stub-index` does not (a damaged, unreadable or deleted file, or a failed write). The corpus then builds on worker 02, inside the finishing passes | **re-entry**, if worker 02 takes back the join's other half (the second `closure_scope`) while it waits in the corpus's `par_iter`. Normally another worker steals that half first, so this is rare |
| checker: `builtin_function_extension` → `stub_index()` (`checker/expr/builtin_calls.rs:36`) | workers of `worker_pool` | the same partial-cache case, when the index was not forced before the check | the same re-entry |
| language server: `Server::new` → `Project::new` (`lsp.rs:289`); edits and rebuilds | main, and the debounce thread | yes | none: neither belongs to a pool |
| `incremental-bench` (`main/incremental_bench.rs:43`) | main | yes | none |
| `check` (`main/check.rs`) | parses only | no | none |
| fixtures (`tests/fixtures.rs`): `warm_stubs` on threads of its own; `Session::for_fixture` on libtest-mimic's threads | no pool | yes | none |
| `edits_across_every_fixture_match_a_fresh_load` (`incremental_updates.rs:1155-1175`) | workers of the test's own pool | yes, when the first `cargo test --lib` after a source change runs cold and this test's worker is the first to force a base | **re-entry**, through both paths of C2. Other groups are queued in that pool's deques, so a steal is likely |

**A deterministic reproduction (not run).** A fresh process (an integration-test binary of its own) with `PHPSTAN_RS_NO_STUB_CACHE=1`, a rayon pool of **one** thread, and `pool.install(|| (0..2).into_par_iter().for_each(|_| { Project::new(); }))`:
1. The `for_each` splits once. The second item stays on the worker's own deque.
2. The first item forces `plain_base`. The corpus's `par_iter` runs on that one-thread pool, and each join takes back its own half, so the build itself completes.
3. `finish_loading` then calls `worker_pool().install` from that worker. While it waits, `wait_until_cold` takes the second item from its own deque first (`registry.rs:791`).
4. The second item calls `Project::new` → `plain_base`, whose initialization is still running lower on the same stack. The thread waits on itself.

The test needs a watchdog: run the body on a thread and fail if a channel's `recv_timeout` expires (a cold stub build in a debug build takes minutes, so the bound must allow for that). Not run because another instance is building on this machine (load 18.7 on 8 cores, a probe worktree at `2a7befd3`).

**The cause.** The stub state is five hidden statics. Any code can force them, at any depth, including jobs of a pool, and their first use builds while holding a lock. Both failures follow from that: a pool job can start a build, and a thread can wait for a build while its pool hands it other jobs.

**The fix.**
- **S1. One stub value, made in one place and handed in.**
  - A `Stubs` value (the corpus's index and data, and the resolved bases) is obtained by one named call, at project creation, and handed in. The `Registry` keeps an `Arc` to the parts its readers need: the extension maps, the file closures, the dependents and the names each stub file declares.
  - The free functions of `stubs.rs` that force the statics become methods on it: `stub_index`, `generated_stub_data`, `builtin_function_extension`, `builtin_class_extension`, `stub_file_closure`, `stub_dependent_files`, `stub_file_names`, `phpstan_stub_names` and the `generated_stub*_named` family.
  - The code that runs in pool jobs reads through the registry: the checker (`builtin_calls.rs:36`, through `Checker::registry`, `checker.rs:277`), `closure_scope`, `composer_hygiene` and `architecture`. It has no call that can start a build. The type system enforces this, so no convention or assertion has to.
  - The partial-cache case goes away: whatever the cache lacks is built at the named call.
- **S2. The build always runs on a thread of its own.**
  - The one process-wide memo (shared by the tests' many projects and by the language server) holds a handle, not the value. Making the handle only starts a named build thread, so nothing parallel runs under its lock. `wait()` blocks in the OS until the build thread publishes.
  - So every waiter, main or a worker of any pool, blocks without taking jobs, and re-entry is impossible anywhere. There is no branch on whether the caller is a rayon worker.
  - A panic in the build is carried to every waiter (`resume_unwind`), as a `LazyLock`'s poisoning does now.
  - The handle can be started when the process starts. That is proposal 5's "beside the parse", and it replaces `warm_stubs`' threads (`project.rs:549-561`).
- **S3. The build's parallel work runs on `worker_pool`.**
  - The corpus's `par_iter` moves into `worker_pool().install`: one pool, and `PHPSTAN_RS_THREADS` and the stack size apply to it (§10.5, §10.12).
  - Starvation would need every worker of `worker_pool` to wait for the build. Under S1 no pool job can wait for it, unless a project is made inside a job of `worker_pool`, which no caller does. `wait()` `debug_assert!`s that its caller is not a worker of `worker_pool`.
- **S4. The base is chosen through the same handle where it is needed.** The plain base or PHPStan's is chosen at `expect_files` (proposal 1), on the session's thread.
- **`Project::new()`** can stay as `Project::with_stubs(Stubs::shared())`. It is the named point at project creation, not a read deep inside a pool job, so the hundreds of test call sites keep working.
- **The test at `incremental_updates.rs:1155`:** its groups get the stubs at project creation, and they block in the OS while the build runs. The reproduction above becomes this design's regression test.
- **Rejected:**
  - **The first proposal**, in three parts:
    1. Force the stub values where the project is made. This is a convention that each new entry point must follow, and S1's types enforce it instead.
    2. Move the build onto a plain thread only when the caller is a rayon worker. This is a conditional hop around the hidden statics, and S2's unconditional build thread removes the need for it.
    3. Assert in debug builds that no `worker_pool` thread waits. S3 narrows this to the one rule left.
  - **Waiting with `rayon::ThreadPool::install` from a worker of another pool**: it steals while it waits (`in_worker_cross`).
  - **Waiting with `rayon::yield_now` in a loop**: the jobs it runs sit above the build's frame on the same stack, so the build cannot resume until they return.
  - **Building outside the lock and then publishing**: two threads could both build the 5 G corpus.
  - **A once-cell that panics on re-entry**: it turns the hang into a crash, but it doesn't fix the cause.


## Costs the report says nothing addresses

The report lists these as costs no proposal and no item of the 20 G plan removes (§11). The figures below are the warm run's unless a line says otherwise.

| # | status | cost | where |
|---|---|---|---|
| N1 | DONE (N1 below) | The check rules' own logic, ≈1.2 G in the statements block. | §6, §11 |
| N2 | DONE (N2 below) | The type relation's and the interner's own work, ≈0.5 G. | §11 |
| N3 | DONE (N3 below) | Building the analysed files' declarations. | §11 |
| N4 | DONE (N4 below) | The regex scanner's formatting, 3.7 M. | §9.2 |

### N1. Two thirds of the statements block's rest is items of the 20 G plan; two new wastes, ≈100 M

The statements block's rest (what is left after the blocks of §6) is 1,224.9 M (off: 1,211.9 M). Classed by function, self cost unless marked:

| owner | M | largest |
|---|---|---|
| item 15, names as names | 164.5 | `lookup_method_with_owner` 40.9, `substitute_class_templates` 13.6, `declared_method_name` 11.1, `record_member_usage_token` 10.3, `lookup_property*` 15.8, and the function-call arm's 11 `eq_ignore_ascii_case` per call over 47,957 calls |
| items 18.5 and 18.6, type operations in borrowed form | 149.1 | |
| item 14, PHPDoc | 76.1 | `var_tag_nodes` 16.2 (112 inclusive); 540 `parse_shared` misses at ≈160 K each |
| item 16, interning | 73.2 | `remembered` 20.9, `Type::new` 16.7, `Type::eq` 9.0, `canonical_int_key` and `ShapeKey` 9.2 |
| item 18.3, slots and radix scopes | 60.1 | |
| item 18.10, F1, F4, F6f | 51.8 | `write_str` 14.4, the SQL parser ≈10, regex 7.1 |
| item 18.1, one inference per node | 37.7 | `open_infer_scope` and `close_infer_scope` 22.4, over 1.43 M opens |
| items 18.7 and 18.9, argument binding and signatures | 34.5 | |
| item 9, `peeled` as a loop | 25.7 | 3.08 M calls; 74.1 M over the whole run |
| item 11, line index | 21.3 | |
| item 18.2, effects in the statement result | 20.4 | |
| items 12.6 and 16, UTF-8 checks of names and keys | 18.5 | |
| item 4, declaration ids | 13.1 | |
| **E2, E3** (new) | 22.7 self, ≈103.7 inclusive | below |

The plan's items reach ≈746 M of it. What no item reaches is 456.3 M over 1,181 functions, none above 8.4 M; 220.4 M of it is in 59 functions of 2 M or more (`bind_assignment_target` 8.4, `check_deprecated_method_call` 8.3, `resolve_pseudo_at` 8.0, `as_object` 7.7, `type_check_call_outcome` 7.1, `forwards_static_binding`, `check_castable_array_arguments` and `check_static_call_context` 6.5 each). That is the rules' own logic: spread thin, with no single cost to remove. The report's "≈1.2 G of the rules' own logic" counts the items' share as well, and overstates it by ≈0.75 G.

**E2. `@var` targets computed for every statement.** `check_stmt_impl` walks each statement's assigned names (`statement_assigned_variable_names`, `checker/stmt.rs:628`; 43,445 calls, 12.0 M) and its defined names twice (`statement_defined_variable_names`, from `inline_var_annotations` at `:1562` and at `:1260`; 290,162 calls over 145,081 statements, 36.3 M inclusive). Only the `@var` targets read them, and `statement_var_tags_of` (`:1542`) returns nothing unless the statement's doc holds `var`: `var_tag_nodes` ran for 1,645 statements. Computing them only then saves ≈48 M, exactly.

**E3. A PHPUnit probe on every expression statement.** `apply_phpunit_assertion_narrowing` (`stmt.rs:704`) runs for each expression statement. `phpunit_method` (86,463 calls, 55.4 M inclusive; off: 57.2 M) infers the receiver (23.6 M), resolves a static target (11.1 M) and finds the method's owner (6.1 M) before it compares the owner with `PHPUnit\Framework\Assert`. Every name `assertion_check` reads starts with `assert`, so a method or static call whose written name doesn't is never an assertion it uses. Gating those two arms on the name leaves ≈6,080 calls in the analysed files: ≈ −50 M. The function-call arm stays: `resolve_function` is 3.6 M, and `use function … as` can give an assertion any name.

### N2. Already lean per call; the costs are counts, and four new cuts

**The relation.** `is_super_type_of`: 1,341,821 calls over the whole run, 470.3 M inclusive, 141.3 M self (≈105 per call).
- The shortcuts at its top (`super_type.rs:70-84`) answer 450,410 calls (34%). Their tests (`is_scalar_value`, `scalar_family`, the `mixed` and `never` matches) cost ≈46 M.
- Structural calls (no registry; `NoClasses`): 420,653. The memo answers 276,787 (66%); 143,866 are computed.
- Registry-backed calls: 470,758, all computed, with no memo. That is item 7's subject.
- `is_super_type_of_by_form`: 614,624 calls, 316.6 M inclusive, 77% of them registry-backed.
- The check's relation block is 504.7 M.

**The interner.** 1,676,428 inserts, 521.2 M inclusive (owned 1,335,586, borrowed 340,842); 126,748 new nodes (7.6%).
- Per insert ≈311: hashing ≈55 (`exact_hash` 92.9 M), comparing ≈83 (the front table 1,477,351 compares, 123.1 M; the shards 72,501, 6.3 M), the insert's own ≈105 (176.8 M).
- Dropping the owned values that hit: 1,255,518, 68.7 M.
- The hash is already shallow (each child's hash is read, not walked). The check's intern block is 369.9 M (insert 113.7, `exact_hash` 59.3, `exact_eq` 49.0, dropping `Repr` 34.8, `intern` 32.1, `is_form_of` 25.6).
- `Type::new`'s callers: decoding the stub cache 416,975 calls, 93.5 M (the warm run only; the off run decodes nothing); `Type::object` 170,696 (57.6 M); `int_literal` 71,550 (23.8 M); `bind_self` 64,671. Decoding as a whole: classes 137.7 M over 3,949, functions 301.3 M over 39,914.

**New cuts:**
- **E5.** Kind bits on the node, so the shortcuts and `is_form_of` read one byte instead of matching the `Repr`. ≈ −40-50 M, an estimate.
- **E6.** The identity test before the memo and the registry-backed forms. It sits at `super_type.rs:114`, after the memo's lookup and the stack guard. B7 counts how many questions it would answer.
- **E7.** A class's plain object type kept with its interned name, and a table of small integer nodes. B8 counts the repeats.
- **E8.** Each stub-cache file's types decoded once into a table its declarations index. ≈35-40 M is left of the decode after items 4 and 6.

### N3. The analysed files' declarations: ≈0.5-0.6 G, paid only because the per-file cache skips them

The per-file cache stores context files only: "An analysed file is always parsed, since checking needs its tree" (`project/file_cache.rs:7-8`); the session passes `cacheable` false for them (`project/session.rs:620-634`). Its products don't need the tree, though: only the check does.

Counted with PHP's tokenizer (`analysis/nn/decls.php`, `docs.php`):

| | files | MB | class-likes | methods | functions | closures | docs on declarations | other docs | body tokens |
|---|---|---|---|---|---|---|---|---|---|
| analysed | 2,792 | 19.07 | 1,956 | 9,357 | 1,144 | 6,561 | 6,668 (2.61 MB) | 35,377 (1.82 MB) | 1,384,375 |
| context (classmap) | 10,610 | 34.42 | 10,475 | 50,553 | 9,333 | | 41,253 (6.07 MB) | 2,573 | 2,599,131 (≈2.81 M over 11,459 files) |

The analysed files hold 13.9% of the declarations' docs by count, 30% by bytes, and 33% of the body tokens.
- `built_declaration` is 1,668.5 M (classes' `from_def` 1,517.1 M over 12,407; functions 75.4 M over 2,420). `from_def` as a whole is 1,519.7 M over 12,546, 580.7 M of it the docblock. The analysed share is ≈0.24 G by count, up to ≈0.32 G by doc bytes.
- The bodies' products (`BodySummary` 343.9, `InitFlow` 218.1, `GlobalFacts` 218.5; 780.5 M) at 33%: ≈0.26 G.
- Their line index: ≈80 M.

So a run with the per-file cache warm still pays ≈0.5-0.6 G for analysed files that did not change. Two things remove it: **E4**, their entries in the per-file cache, or the F2 result cache (`CRATE_SPLIT_PLAN.md:7005-7110`, not built), which skips unchanged files whole. A changed file's cost is inherent.

### N4. The regex scanner: 714 scans of 170 patterns

`scan_branch`'s 6,988 `format!` calls (3.7 M) run inside 714 `parse_capture_groups` calls, 11.7 M inclusive: 467 from `preg_matches_type`, 247 from `narrow_preg_outcome`. The analysed files hold 203 literal `preg_match` and `preg_match_all` sites, 170 distinct patterns (`analysis/nn/preg.php`), so each pattern is scanned ≈3.5 times (the type and each narrowing). F6f's memo (one analysis per distinct text) removes ≈75% of the 11.7 M. **E9** removes the formatting itself: `concat_literals` (`regex_groups.rs:390-401`) builds each product with `format!("{x}{y}")`.

**Reproduction.** The profiles are in `~/.cache/prs-profile/callgrind-20g-modes/` (`warm/warm.pkl`, `off/off.pkl`); `drill-scripts/q.py <pkl> find|callers|callees|lines|tree <pattern>` reads them. N1's split is `analysis/nn/n1split.py` (output `n1split-warm.txt`, `n1split-off.txt`); the counts of N3 and N4 come from the PHP scripts beside it, run over Piwigo's file lists.
