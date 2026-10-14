# Off-run drill ledger

Tracks what `PROFILE_DRILL_PIWIGO_OFF.md` leaves open: the decisions it
leaves to the user, and the questions it could not settle. The proposals
themselves are in the report (§10). This file lists only what still needs a
decision or a look before they can be built or judged.

Status: OPEN, DONE (hash or answer here), SKIP (reason).

Section numbers (§) and proposal numbers are the report's. "Item n" is an
item of the 20 G plan (`PROFILE_DRILL_PIWIGO_20G.md`, Part II).

Every item was checked again on 2026-10-14 against the code, the profiles,
the probe outputs on disk and fresh runs of the scripts, and corrected in
place. The checks and what each found are in
`~/.cache/prs-probe/validate/notes.md`; E14 and E15 came from them.

## Decisions

| # | status | decision | options | gates |
|---|---|---|---|---|
| D1 | DONE (D1 below) | Syntax errors inside a skimmed body (§10.8). Today a context file that fails to parse is skipped with a warning and its declarations are lost. | 1. Load the file anyway, and report the error only when something reads the body. 2. Check skimmed bodies with a pass that builds only what its checks read: today's behaviour, with ≈1.3-1.5 G of the 3.8 G saved, and more once each file's verdict is cached. | proposal 8 |
| D2 | DONE (D2 below) | Where a skimmed body's text comes from when it is read later (§10.8). | Keep it in memory (≈18 MB of body text for Piwigo's context files), or read it again from disk with a hash check. | proposal 8 |
| D3 | DONE (D3 below) | How a binary carries the stub data (§10.6). | 1. Built at compile time by a build script, and embedded. 2. Appended to the binary by the release step. 3. Packaged beside the binary. | proposal 6 |
| D4 | DONE (D4 below) | What a binary built from source (`cargo install`, `cargo build`) does, since it carries no cache unless its build makes one (§10.6). | | proposal 6 |
| D5 | DONE (D5 below) | A cache directory that exists but can't be written (§10.2). | Leave the gap, where the encoding is still paid, or detect it. | proposal 2 |
| D6 | DONE (D6 below) | What a context file that fails to parse contributes (§10.8). Today it loses every declaration. That is right when the error is real and wrong when it is a gap in our parser, and the analyser cannot tell which. | 1. Keep today's policy: its declarations are absent. 2. Keep every possibility: what the file declares is kept, and what failed is unknown. Recommended: option 2, with what parses recovered, validity by the configured PHP version, and one rule for the batch run and the language server. | none: it holds whether or not proposal 8 is built |
| D7 | DONE (D7 below) | When to build proposals 1-5 (§10, §10.6). Once proposal 6 builds the stub data at compile time, they speed only the off mode and the reflection crate's own tests, and proposal 4 also the build script. Proposal 6 waits on the crate split's reflection layer. | 1. Build them now: every cold run gains until the crate split lands. 2. Build only proposal 4 and E1's fix, and leave the rest to the off mode. Recommended: option 1, in the order E1's fix (with 1 and 5), D5's gate (with 2), 3, 4. | proposal 6's place in `WORK_STACK.md` |

### D1. Option 2, with each file's verdict cached

- **Decided** 2026-10-13. Every skimmed body is checked, by a pass that builds only what its checks read. A cache keyed by the file's bytes and the `syntax` crate holds each file's verdict, so a file is checked once per parser version (§10.8).
- **Why not option 1.** PHP refuses the whole file for one bad body, so no code after its `include` runs (php 8.5.4, `~/.cache/prs-probe/syntax-in-body/` and `validate/d1/`):
  - **A parse error** throws a catchable `ParseError`, and nothing the file declares exists.
  - **A compile-time error** ("Cannot mix keyed and unkeyed array entries", a `goto` to a missing label) is fatal and cannot be caught: the process ends at the `include`. The classes and functions compiled before the error are bound, but only a shutdown function runs to see them. The modifier checks (`zend_compile.c:946-1044`, "Multiple final modifiers are not allowed") are the exception: they throw `CompileError`, which is caught as a `ParseError` is.

  With option 1, whether a file loads would depend on whether an optimization skipped one of its bodies. Option 2 keeps one policy for every file. That policy itself is D6.
- **Every error is the parser's or the lexer's.** `syntax::parse_watching` is the only step of a load that fails (`project.rs:2892`), and a lexer error anywhere in the file outranks the parser's first error (`syntax/src/parse.rs:116-133`). No later pass over the tree reports one.
- **Corrections the adversarial pass made:**
  - **The pass cannot build nothing.** The parser checks destructuring after it builds the array, with sets keyed by node id (`syntax/src/parser/expr.rs:402-497`: `long_arrays`, `pattern_only`). It keeps array shapes and node ids.
  - **It saves less than first estimated:** ≈1.3-1.5 G before the plan and ≈0.6-0.9 G after it, not 1.5-2 G. Tree building is at most ≈0.53 G of the parse job's 3.65 G parser block, and the token pipeline (≈1.05 G) and the grammar stay in the pass. The report's §10.8 has the table.
  - **A body that is read is checked and then parsed.** So without a cached verdict, the saving is S − F × d, not S × (1 − d). After the plan, that is nothing from d ≈ 0.26-0.45. With the verdict cached, it is F × (1 − d): ≈2.0-2.3 G × (1 − d) after the plan.
- **The verdict cache's key** is sound (C6).

### D2. Kept, copied at load from the bytes the load reads

- **Decided** 2026-10-13 (§10.8).
- **No read of its own.** Every load reads the file's bytes, if only for the per-file cache's key (`session.rs:631-637`). The copy is taken from them, so a body parsed later reads what the load checked.
- **Lexing a body alone.** `Lexer::fragment(src, base)` (`syntax/src/lexer.rs:461-473`) lexes a piece with spans absolute in its file. It needs a bytes variant for files that are not UTF-8.
- **Memory.** About half of each context file is body text: B1's probe counts 17.7 MB in 49,533 bodies of Piwigo's 37.1 MB (§5's estimate was ≈19 MB). The peak is 866 MB, of which ≈420 MB are the context files' trees (the 20 G report's M1). The language server already keeps every file's text (`lsp.rs:294`).
- **Rejected.**
  - Reading the file again on demand: it may have changed since the load, and a hash check only detects that.
  - Mapping the file: the same problem, and it needs `unsafe`.

### D3. Option 1: the stub data built at compile time, into every binary

- **Decided** 2026-10-13 (§10.6). It is the only option that reaches every binary:
  - release binaries;
  - binaries built from source (D4);
  - test binaries;
  - every way the VS Code extension finds one (`serverPath`, `PATH`, a download).

  Options 2 and 3 reach release archives only. The data is a function of the source alone: the build reads no setting, and `switch_base` applies the PHP versions after the load. The only environment variables it reads are the cache's location (`stub_cache.rs:33-44`) and the pools' sizes (`PHPSTAN_RS_THREADS`, `project.rs:364`, and rayon's own), which B10 shows do not change the data.
- **Conditions found by the adversarial pass:**
  - **The crate split's reflection layer.** A build script cannot call the crate it builds, and must run the very code the runtime runs. A stub base is built through `Project::finish_loading` → `run_finishing_passes` (`project.rs:1387-1399`), and the registry uses the checker (`registry/init_flow.rs:13`). So proposal 6 lands with level 2 of `WORK_STACK.md`.
    - Today's bases also run purity and global-variable inference, which the module graph assigns to `analyser` (`gates/module-graph.txt:78-79`) and which the split's C9 moves out of reflection, before C10 extracts it. So a reflection build script could not build today's bases either (C8).
  - **The key misses an input today (E15).** The build's source hash leaves out `vendor/phpstan-src/stubs`, whose `.stub` files the stub build reads. Embedded data has no key, but the stub cache it would replace does.
  - **A test on every release target that the embedded data equals a runtime build.** Two release legs cross-compile (`release.yml`), and constant folding of `**` calls the platform's `pow` (`types/src/arithmetic.rs:715-717`). `folded_hash` (`syntax/src/name.rs:997`) is the same everywhere. B10: no stub folds `**` today, so the data is equal on every leg, and a host-side test that fails when a fold uses `powf` keeps it so.
  - **Build settings.** A `[profile.*.build-override]` opt-level for the build-dependency (the test profile's comment: 4-6× faster at opt-level 1). The 31 MB `include_bytes!` goes in a small crate of its own, so the main crate's incremental builds do not embed it again.
  - **A borrowed `Reader`**, since `Reader::new` takes an owned `Vec<u8>`.
- **Correction.** The data is not read in place from a file: it is a static. `.rodata` grows from 18.2 MB to ≈50 MB, in the release binary and in each test binary that links it. A run loads only the pages it touches. The compile-time cost is B9.
- **What it changes elsewhere.**
  - A run builds the stubs only in the off mode (`PHPSTAN_RS_NO_STUB_CACHE`, which comes to mean "build at run time") and in the reflection crate's own tests.
  - E1's hazard narrows to those runs and to the build script, which runs on a thread of no pool. It does not go away.
  - Proposals 1-5 lose most of their reach (D7).
- **Against the crate split (C8): the form does not survive, and D3 is decided again after C5's version key.**
  - C4 and C5 leave no bases to embed. What is left is each stub file's declarations before resolution, read when the file is reached. C9 also takes the purity and global-variable passes, which today's bases run, out of reflection, and stubs get no global-variable summaries after it.
  - The saving falls from 5.05 G to ≈0.2-1.5 G per cold run (≈2 G at the upper bounds), while B9's compile time, B10's test and the `.rodata` stay.
  - The per-file cache, keyed over the crates the declarations depend on, does most of the job with no build script.
  - Checked-in data, as the index is, is unfit: it is binary and changes with every edit to the declaration model.
  - Proposal 6 is not built in today's form.

### D4. Settled by D3

- The crate is not published (`publish = false`), and the README's route for a binary other than a release one is building from source.
- Once D3 lands, every binary built that way carries the data.
- Until then, such a binary pays one cold run per build, as today.
- **Reopened with D3 (C8).** If D3 is decided again for the per-file cache over embedding, a binary built from source pays one cold run per change to the crates the stub declarations depend on, not per build.

### D5. Detect it: one check, at the first store, apart from reading

- **Decided** 2026-10-13 (§10.2).
- **The check.** It runs lazily, at the first store, and is kept in a `OnceLock` of its own. A fully warm run stores nothing and checks nothing.
- **Reading stays on.** A directory that cannot be written still serves what it holds.
- **The gate comes before `CachedFile::of`**, which builds the entry before it is encoded (`session.rs:765-771`), so a file that will not be stored pays neither.
- **Reports.**
  - One warning when the cache cannot be written.
  - One more on a later write failure: entries sit in 256 shard directories (`file_cache.rs:177`), so one write can fail after the check passed.
  - A load refreshes an entry's mtime after 12 h (`file_cache.rs:190-196`), and a failure there passes silently today.
- **Which caches:** the per-file cache, the stub cache until proposal 6 or the crate split's C5 removes it, and D1's verdict cache.
- **E13's shard directories** are made by this check too, so a run that stores nothing touches nothing on disk.
- **Stating intent.** A cache that is read-only on purpose (one restored in CI, say) would get a note on every run. An explicit read-only setting states the intent and silences it.

### D6. Option 2: keep what the file declares, and recover what parses

- **Recommended** 2026-10-13, from the evidence below.
- **What PHP does with a file it refuses.** php 8.5.4, `~/.cache/prs-probe/d6/kinds.php` and `main.php`, autoloading `Broken.php`, one of whose bodies does not parse:
  - `new`, a static call and `class_exists()` autoload the file and throw `ParseError`. They throw again on every later attempt, since a failed autoload is not remembered.
  - `instanceof`, `::class`, `class_exists($n, false)`, a parameter type and a `catch` clause do not autoload. They behave as if the class were not declared: `false`, the name, `false`, a `TypeError` for an object (`null` passes a nullable type), the next `catch`.
  - A function the file declares is undefined: calling it throws `Error`.
  - **A compile-time error is fatal instead** (D1's probe): the autoload ends the process with no exception to catch. 257 of the 272 files below fail that way; the other 15 throw (13 parse errors, and 2 modifier errors as `CompileError`).
- **Neither world gives "not found" at a use the analysis continues past.** Where the error is real, a use that needs the class or the function throws or ends the process, so no code after it runs. Where the error is a gap of our parser, the declarations exist. So the code after a use sees the declarations in both worlds, and keeping them is the union of the two. `instanceof` and `class_exists($n, false)` are false in the first world and either value in the second; keeping the declarations allows both.
- **What fails in real code.** `php -l` ran over every PHP file under `vendor/`: 111,836 files in the 18 trees that hold PHP files (two of the 20 directories are empty), 67,252 of them installed dependencies (`~/.cache/prs-probe/d6/php-l-fatal.tsv`, `php-l-class.tsv`). 272 fail.

  | kind | files | reaches the analyser as context |
  |---|---:|---|
  | phpstorm-stubs: 25 built-in functions redeclared, 35 declarations repeated for version gates (18 parameters, 12 methods, 5 functions), `function exit` (`Core.php:1193`), 11 other compile errors | 72 | no: the stub loader reads them |
  | test fixtures | 198 | through `paths` or a scan directory only |
  | rector's `stubs/` | 1 | yes: rector's `phpstan.neon` scans `stubs` |
  | an example | 1 | no |

  - **Installed dependencies: none.** 3 of the 67,252 fail: a test script of `php-console`, a fixture of `phpcs-gitlab` and an example of `tracy`. All three lie outside their package's `autoload` directories.
  - **A root's own `autoload-dev` is never context.** The vendor discovery keeps only paths under the project's `vendor/` (`composer/src/vendor.rs:17-23`). phpstan-src has 191 failing files:
    - 184 are in its `autoload-dev` classmap (`tests/PHPStan`): 171 under `tests/*/data/` and 13 under `tests/PHPStan/Analyser/nsrt/`. They reach the analyser only through `paths`, and PHPStan excludes both there (`build/phpstan.neon:30`, `:33`).
    - 7 are in neither the classmap nor PHPStan's paths: 5 under `turbo-ext/tests/parser-fixtures/` and 2 under `e2e/`.
  - **Kinds of error.** 13 of the 272 are parse errors. The other 259 are compile errors, most of them of C7's 238 kinds, which the parser accepts. The parser was not run over them.
- **What our parser refuses in real code, which PHP accepts.** B1's Laravel run warned about two context files that PHP 8.5 lints clean, `aws-sdk-php`'s `AbstractConfigurationProvider.php:41` and `phpseclib`'s `Math/BigInteger.php:177`. Both hold `$x instanceof static::$name` or `self::$name`, a class named by a static property.
  - The parser reads a name after `instanceof` and leaves `::$name` unread (`syntax/src/parser/expr.rs:106-117`). PHP's grammar takes the same `class_name_reference` there as after `new`, which `parse_new_class_reference` already reads (`primary.rs:83`).
  - A tokenizer scan of all 111,836 files finds 6 such files, in the installed dependencies of laravel, league-flysystem and monolog (`validate/d6/instanceof_sp.tsv`). Each loses every declaration as context today. E14 is the fix.
  - The parser has not been run over the whole corpus, so other gaps of this kind are not counted.
- **Three causes, and what each needs.**
  - **A file broken on purpose,** a test fixture. Nothing refers to what it declares except the test that expects the error.
  - **A file written for analysers only.** `rector-src/stubs/Symfony/Component/Form/FormBuilderInterface.php` declares a class whose method has no body.
    - PHP refuses it at compile time ("Non-abstract method … must contain body"), so the `interface_exists` guard at its top never runs.
    - Rector scans the directory so that its analysis sees the class when `symfony/form` is absent.
    - The parser accepts it today (C7's run: `class A { function f(); }`), so the file loads.
    - Matching PHP exactly, with today's policy plus C7's 238 checks on context files, would drop the one file the project wants read.
  - **A gap in the parser,** for every version (`instanceof static::$name`, above) or by PHP version.
    - `rector-src/build/target-repository/e2e/parse-php7-code/src/Foo.php` reads `$bar{2}`. That is valid in PHP 7 and refused by PHP 8 (`curly.php`).
    - The parser has one grammar for every version, and its postfix loop has no arm for `{` (`syntax/src/parser/postfix.rs:16-241`). So a project on PHP 7.4 loses a valid file.
    - The gap also runs the other way: the parser accepts `(real)` and `(unset)` (`syntax/src/ast.rs:1303`, `:1311`), which PHP 8 refuses.
- **The design.**
  - **Recover what parses.** The declarations that parse are kept.
    - **Prior art.** Psalm does this: php-parser's `Collecting` handler returns the statements it recovers, and each error becomes one issue (`StatementsProvider.php:367-424`, Psalm 6.20.0, the latest stable; 7.0 is at its second release candidate; in `~/.cache/prs-probe/d6/upstream/`). PHPStan drops the file (`FileNodesFetcher.php:32-36`, unchanged in 2.3.1, the latest release).
    - **A body that fails** keeps its function or method, with an unknown summary. With proposal 8 this needs nothing new, since the skimmer finds a body's end without parsing it.
    - **A member or class header that fails** leaves the class declared with an unknown shape. That is a new kind of registry entry, on which no rule reports.
    - **Without proposal 8** the parser needs recovery points: the end of a member, at class depth, and the end of a top-level statement. That is new.
  - **One report per file.** It is the warning a context file gets today, naming the error. A use site gets no report: the analyser cannot tell a real error from a gap of ours, so a use-site report would be false for every gap. Once C7's gap is closed and validity follows the version, a use-site report ("this autoload throws `ParseError`") becomes exact.
  - **Validity follows the configured PHP version.**
    - The parser accepts every version's syntax and records what a file used. Master's `SyntaxMarks` does that for newer syntax, 7.1 to 8.1 (`syntax/src/lexer.rs:250-283`). It is extended to removed syntax: `$s{0}`, `(real)`, `(unset)`.
    - The version is applied after the parse. A file that every version in the configured range accepts is valid. One that some version in the range refuses gets the report, and still contributes what it declares.
    - The parse stays a function of the bytes and `syntax`, so C6's verdict key stays exact: the cache holds the marks, and each run applies its own version.
  - **One rule for the batch run and the language server.** On master, the batch run drops a failed context file's declarations (`context_failures`, `project/session.rs:324`, `:2143-2174`). The language server keeps the last version that parsed (`lsp.rs:752`, `parse_failed`). With recovery, both use what the current text declares. A header that fails leaves its class with an unknown shape, not gone, so the server no longer needs the last good version for it.
  - **D1's verdict cache** stores the recovered regions with the verdict.
- **Why not option 1.** It is wrong for every gap of the parser, and both kinds of gap are in real code. It also ties the parser's strictness to what context files contribute: closing C7's gap in the parser would drop the files written for analysers.
- **Cost.** Recovery in the parser (none for bodies once proposal 8 is in), a registry entry with an unknown shape, and marks for removed syntax.

### D7. Option 1: build proposals 1-5 now, in four steps

- **Recommended** 2026-10-13. The order:
  1. **E1's fix.** It fixes a deadlock that is in today's code (C2), so it is needed whatever proposal 6 does. It carries proposals 1 and 5: its S4 chooses the base at `expect_files`, and its S2 builds each layer on a thread of its own.
  2. **D5's gate.** Proposal 2 is that gate applied to the stub cache.
  3. **Proposal 3.**
  4. **Proposal 4.**

  Each step is measured with paired cold runs. B4 gives them their wall-time weight.
- **Proposal 6 is far off.**
  - It needs the crate split's reflection crate, stage C10. That comes after C4 (per-class memoized resolution) and C5 (the version-keyed stub layer).
  - The split has finished A0-A2, B1-B9 and C1-C3 (`gates/module-graph.txt:228` on master).
- **Until then every cold run pays the build.**
  - The stub cache is keyed by the build's source hash, so the first run after every rebuild is cold: the development loop pays it once per edit.
  - CI's cold jobs pay it.
  - So does the first language-server start. `tests/lsp_startup.rs` on master holds that start to 10 s, from an empty stub cache as well as a warm one.
- **The crate split deletes what they speed (C8).** C4 makes stub resolution lazy, and C5's stub layer, with a per-file parse memo, "replaces the per-project stub clone and the per-process stub parse" (`CRATE_SPLIT_PLAN.md`, C5's gate). No base is built, no corpus is merged and nothing is stored, so the code of proposals 1-5 goes with the eager build.
  - Their whole value is the cold runs between now and C5. Building them now is the only time they pay.
  - They are small and local, measured against today's eager build.
  - E1's fix is needed until then too, since the deadlock is in today's code.
  - D5's gate outlives them: it guards the per-file cache and D1's verdict cache, which stay. Only its part for the stub cache, proposal 2, goes.
  - Until C5, C4 keeps an eager pass over each stub file (`CRATE_SPLIT_PLAN.md`, C4: "the stub pass still resolves each stub file's classes within that file (until C5's stub layer)").

## Investigations that need a build

None of these was run for the report.

| # | status | question | settles |
|---|---|---|---|
| B1 | DONE (B1 below) | What proposal 9's on-demand search reaches, on Piwigo and on a general tree: the distinct callees the no-op rules and the property-write reads ask about, and the nodes the searches visit (§10.9). This also gives d, the share of bodies read. §11 assumes d ≈ 0.2. Measured on Piwigo and Laravel: each search visits 0.3-3.0 k nodes, against 69-114 k declarations the eager pass classifies, and no answer differs from it; d ≈ 0.005-0.006, and the bodies the skimmer parses in full for a trigger bring the share of body tokens parsed to ≈0.01-0.02. | proposal 9's saving; proposal 8's saving: ≈2.0-2.3 G × (1 − d) after the plan with each file's verdict cached, and without it whether proposal 8 pays at all, since it saves nothing from d ≈ 0.26-0.45 (D1) |
| B2 | DONE (B2 below) | Proposal 7's real saving. The estimate counts every tag, not every distinct tag, so the memos may lower it (§10.7). Measured per lexer run, memos included: ≈0.37 G off and ≈0.08 G warm, and ≈0.04 G once proposal 6 and item 14 are in. | proposal 7 |
| B3 | DONE (B3 below) | How many reports the loops' convergence passes format and then discard (`diagnostics.truncate`: `checker.rs:1278`, `:1807`, `function_decl.rs:1209`; §10.11). The loops discard 270 on Piwigo and 304 on Laravel. The `extract()` truncation discards 31,660 on Piwigo, ≈35 M (E10). | how far proposal 11a reaches |
| B4 | DONE (B4 below) | Wall time. Nothing is timed, including the 1.6 G of serial stub work on a cold run's critical path, with the other cores idle (§12). Medians of 8 interleaved runs on a quiet machine: off 2.68 s, stub cache cold 2.69 s, stub cache warm 2.31 s, both caches warm 1.60 s. The stub build is 0.37 s, 14% of an off run, and ≈0.4 s of its stretches run on one core. The finishing passes are ≈40% of the wall time with both caches warm. | the wall-time weight of proposals 1-5 |
| B5 | DONE (B5 below) | A run with the per-file cache warm. All three runs had it off, so its floor and `from_def`'s cost with it on are unknown (§11, §12). Its floor is 14.34 G, against 20.57 G with the stub cache warm alone. `from_def` falls to 202.1 M, the analysed files' classes only. The run also shows two costs only the warm per-file cache pays (E11, E12). Its wall time is B4's. | the floor of the per-file-cache-warm mode |
| B6 | DONE (B6 below) | Disk time of the cold run's cache write. Its 58 M is callgrind's count only (§12). The stub cache's write is ≈10 ms (4 files, 31.4 MB) and does not show in wall time. A cold per-file cache's first write costs ≈0.7 s of CPU, mostly in the kernel, and 0.10 s of wall (E13). | the cold run's full cost |
| B7 | DONE (B7 below) | How many of `is_super_type_of`'s 1,341,821 questions ask about one node against itself. The profile counts calls, not their operands (N2). A probe run on Piwigo makes 1,479,785 calls. 203,478 (13.8%) are one node against itself, and 164,144 of those pass the shortcuts: 67,631 structural, 96,513 registry-backed. Equal nodes that are not one are 0.6%. E6 nets ≈ −10 M. | E6's saving |
| B8 | DONE (E7 below) | How many of `Type::object`'s 170,696 calls build a class's plain object type, with no arguments, and how many of `int_literal`'s 71,550 build a small integer (N2). Settled from the code and the tokens: 125,364 (73%) are plain, and 92% of the integer literals are 0-255. | whether E7 is worth building |
| B9 | DONE (B9 below) | What building the stub data at compile time adds to a build: the reflection crate compiled again for the host, and the stub build in the build script at the `build-override` opt-level, for a clean build and for an incremental one after an edit to the reflection crate (§10.6, §12). Cargo compiles the crate once when no `--target` is passed and the override equals the profile, so only `release.yml`'s legs pay a second compile: ≈160-215 s of units at opt-level 1. The stub build takes 3.67 s at opt-level 0, 0.90 s at 1 and 0.59 s at 3, with byte-identical output, and the embedding ≈0.4 s. An edit to reflection loses up to ≈20 s of pipelining, unless only the binary crate takes the data. | proposal 6's cost to development (D3) |
| B10 | DONE (B10 below) | Whether the stub data built for a release target equals the data that target builds at run time, on the two cross-compiled legs (`aarch64-unknown-linux-gnu` on x86_64, `x86_64-apple-darwin` on arm64). Constant folding of `**` calls the platform's `pow` (`types/src/arithmetic.rs:715-717`). This becomes the test that D3 requires, run natively or under emulation (§10.6, §12). Equal today. The stub build calls `powf` 0 times: the stub sources' four `**` sit in a phpstan-src method body, on operands that are not literals. Nothing else in the build reads the platform, and the data is byte-identical across thread counts, one CPU and opt-levels 0, 1 and 3. A host-side test that fails when a fold uses `powf`, or a pow written in Rust, keeps it equal. | whether the cross-compiled legs can embed what their build script makes |

### How B1-B3 were measured

- **A probe build.** A patch at `2a7befd3` that is not for commit (`~/.cache/prs-profile/off-ledger-b/probe.patch`) does four things:
  - it answers every purity and property-write question of the check a second time, by a memoized depth-first search from the declaration asked about, and compares each answer with the eager pass's;
  - it records every body read through `BodySource::summary` and `init_flow` during the check;
  - it counts, for every run of the tag lexer, what was lexed and what a pulling parser would have lexed;
  - it records what `rewind_to` and the two dynamic-variable truncations drop.
- **The runs.** All runs are in the off mode, with outputs in `~/.cache/prs-profile/off-ledger-b/b1/`.
  - **Piwigo**, as in the profile. Its `out.json` is byte-identical to the profile's.
  - **Laravel**: `vendor/laravel`, `check-project src`, with its own `vendor/` as context.

### B1. The searches on demand reach 0.3-4% of the graph, and d ≈ 0.005-0.006

| | Piwigo | Laravel |
|---|---:|---:|
| context files, analysed files | 11,459, 2,792 | 12,404, 1,700 |
| context bodies | 49,533 (17.7 MB) | 38,146 (13.3 MB) |
| purity questions (function, method, static, `new`) | 39,880, 5,120 of them on stubs | 5,184, 404 on stubs |
| distinct non-stub callees asked about | 1,572 | 1,930 |
| of them pure | 0 | 0 |
| nodes the purity search visits | 1,590 | 1,939 |
| property-write questions | 26,189, 7,850 on stubs | 2,130, 1,375 on stubs |
| distinct non-stub callees asked about (nothing, own object, anything) | 2,489 (1,025, 89, 1,375) | 258 (82, 38, 138) |
| nodes the property-write search visits | 3,007 | 321 |
| declarations the eager property-write pass classifies | 69,479 | 113,812 |
| cycles met (purity, property writes) | 0, 14 | 0, 0 |
| answers that differ from the eager pass | 0 | 0 |
| context bodies read, all readers | 313 (102 KB) | 192 (71 KB) |
| **d, by count and by bytes** | **0.0063, 0.0058** | **0.0050, 0.0054** |

- **The search is exact on both trees.** It gives the eager pass's answer for every callee asked about, cycles included.
- **No non-stub callee the no-op rules asked about is pure**, on either tree. Each search stops at the first impure node, often at the root's gate before any body is read. On Piwigo, the purity search read 123 context bodies and 258 analysed ones, for roots whose own bodies are 227 and 1,288.
- **The checker itself read no context body.** The uninitialized-property rules are off by default (`rules.check_uninitialized_properties`). The parent-call rule (`project/class_shape.rs:343`) reads only for the methods it is configured with. With those on, the ancestors' bodies they read add to d. That is not measured.
- **The skimmer's own full parses.** Proposal 8 also parses in full every body that holds a trigger (§10.8). Counted with `token_get_all` over each run's context files (`b1/triggers.php` over `b1/*/context-files.txt`):

  | | bodies flagged | share of body tokens | by trigger |
  |---|---:|---:|---|
  | Piwigo | 210 of 49,496 (0.42%) | 1.34% | include 151, `new class` 34, a nested `function` 33, a `PHPStan` name 16, `class` 3, `define` 2 |
  | Laravel | 81 of 38,236 (0.21%) | 0.62% | include 53, `new class` 21, a nested `function` 13, `define` 6 |

  With the bodies read, ≈1-2% of the context body tokens are parsed in full.
- **Proposal 9.** "Less what is asked for" (§10.9) is 0.3-4.3% of the declarations per search (Piwigo 2.3% and 4.3%, Laravel 1.7% and 0.3%), so §10.9's saving stands: ≈0.3-0.4 G beyond item 4, and most of the passes' 0.19 s on the finishing thread.
- **Proposal 8, at d ≈ 0.02 by tokens:**
  - **With the verdict cached:** F × (1 − d) ≈ 2.0-2.3 G after the plan, against §11's 1.6-1.85 G at d ≈ 0.2. §11's warm column then leaves ≈9.9-10.2 G, not 10.3-10.6.
  - **Without it:** S − F × d ≈ 0.55-0.85 G after the plan, against ≈0.15-0.5 G at d ≈ 0.2. Proposal 8 pays on every run, far below the break-even d ≈ 0.26-0.45. A run with no cached verdict leaves ≈11.3-11.6 G, not 11.6-12.0.
  - **D1's verdict cache** is worth F − S ≈ 1.1-1.7 G on a run whose files have verdicts. It no longer decides whether proposal 8 pays.

### B2. Proposal 7: ≈0.37 G off and ≈0.08 G warm

- **What was counted.** For each run of the tag lexer: the tokens and bytes it lexed, and those a pulling parser would have lexed. The parser's share runs up to the last token it looked at before a description, and the `[optional]` test counts as one token. Runs are counted, not tags, so the memos are in the count. Piwigo's 61,615 runs equal `tokenize`'s call count in the profile.

  | phase | lexer runs | bytes lexed | not needed: bytes, tokens | description tokens |
  |---|---:|---:|---:|---:|
  | stub build (the same on both trees) | 21,840 | 2,197,804 | 78.9%, 81.2% | 414,584 |
  | parse, Piwigo | 23,135 | 1,222,438 | 36.4%, 36.3% | 98,541 |
  | check, Piwigo | 16,640 | 571,023 | 7.4%, 8.1% | 10,638 |
  | parse, Laravel | 55,377 | 3,698,324 | 21.1%, 18.6% | 64,391 |
  | check, Laravel | 27,756 | 580,709 | 1.4%, 2.0% | 1,815 |

- **Against §10.7's shares.** For the stubs, §10.7 estimated 84.4% and the count gives 78.9%. For the parse, it estimated 56.5% and the count gives 36.4%. Laravel's parse share is lower still: its `@method` tags are 36,859 of the runs, and 83% of their bytes are needed. The check reads few descriptions.
- **In Ir, on Piwigo.** The tag lexer's inclusive cost (308.5 M off, 132.7 M warm) is split by phase in proportion to the lexer's self cost per phase. That gives:
  - off: stub build 175.9 M, parse 90.6 M, check 42.0 M;
  - warm: parse 84.8 M, check 47.9 M.

  At the measured shares, not lexing those bytes saves 138.8 + 33.0 + 3.1 = 174.9 M off, and 30.9 + 3.5 = 34.4 M warm. `remaining_description` goes entirely: 197.9 M off and 47.5 M warm.
- **The saving.** ≈0.37 G off and ≈0.08 G warm, against §10.7's 0.34 and 0.1 G.
  - Four fifths of the off figure is the stub build, which proposal 6 removes, so after proposal 6 both modes save ≈0.08 G.
  - Item 14's borrowed tokens halve that again: ≈0.04 G, against §11's ≈0.05.
  - A tree whose doc comments are mostly signatures, such as Laravel's, saves less.

### B3. The loops discard almost nothing; an `extract()` body discards everything

| site | Piwigo | Laravel |
|---|---:|---:|
| `rewind_to` from `control_flow.rs:242` | 214 (133 KB) | 285 (27 KB) |
| `rewind_to` from `loops.rs:105` | 8 | 12 |
| `rewind_to` from `loops.rs:382` | 48 | 5 |
| `rewind_to` from `stmt.rs:271` | 0 | 2 |
| `check_function`'s dynamic-variable truncation (`function_decl.rs:1208`) | 31,660 (1.81 MB) | 2 |
| `check_program`'s (`checker.rs:1806`) | 0 | 0 |
| reports the check keeps | 651 | 9,800 |

- **The loops** discard 270 reports on Piwigo and 304 on Laravel. Proposal 11a saves well under 1 M on them.
- **The `extract()` truncation is where the waste is.** 31,481 of the 31,660 reports are `varTag.variableNotFound`, from the compiled Latte templates in Piwigo's `_analysis/phpstan-latte/`, which hold 34,570 `@var` tags. Each template's function calls `extract($ʟ_args)` and then names every variable with a `@var` tag.
  - `check_statement_var_tag_names` (`checker/stmt.rs:433`, `:529`) does not count `extract()` as defining a name, so it formats a report for each tag.
  - `check_function` then drops every report of the body.
- **The cost, warm run.**
  - The messages: ≈11 M, at the mean cost of `format_inner`'s 66,295 calls from `check_stmt_impl`.
  - `report::<&str>` from `check_stmt_impl`: 23.4 M over exactly 31,481 calls. Over all of `report`'s 32,395 calls, `DiagnosticBuilder::build` is 16.1 M (14.8 M of it `LineIndex::range`) and the vector's growth 4.6 M, so nearly all of it is these.
  - Together ≈35 M.
- **Proposal 11a** would save only the formatting, ≈11 M, since the report would still be built. E10 saves all of it, and it applies wherever compiled templates call `extract()`.

### B5. With both caches warm: 14.34 G, and two costs only the warm per-file cache pays

- **The run.** The profile's binary (`fp/dhat`, source hash `558a2b69…`) ran under callgrind on Piwigo, with a cache directory that two earlier runs had filled (`~/.cache/prs-profile/off-ledger-b/b5/`).
  - The stub cache and the per-file cache were both warm: 11,459 entries, one per context file, 48.3 MB.
  - The output is byte-identical to the profile's `out.json`.
  - The run wrote nothing: every cache file's mtime and size are unchanged.
  - Wall time is B4's.

  | phase | stub cache warm, per-file cache off (the profile's warm run) | both warm | change |
  |---|---:|---:|---:|
  | parse | 9,983.0 M | 3,833.7 M | −6,149.3 M |
  | check | 6,774.7 M | 6,704.6 M | −70.1 M |
  | finishing | 2,475.0 M | 2,617.3 M | +142.3 M |
  | insert (main thread, serial) | 751.2 M | 582.7 M | −168.5 M |
  | vendor discovery | 321.3 M | 321.2 M | 0 |
  | session and main | 267.0 M | 278.8 M | +11.8 M |
  | **total** | **20,572.2 M** | **14,338.3 M** | **−6,233.9 M** |

- **The floor is 14.34 G.**
- **What the parse phase keeps (3.83 G):**
  - **The 2,792 analysed files: ≈2.52 G.** Their parse is 1,961.2 M (`parse_with_ignores`). Their products cost 548.0 M:
    - `file_declarations::collect`, 262.4 M, of which `from_def` is 200.4 M;
    - `FileBodies::collect`'s walk, 141.3 M;
    - `recorded_references`, 79.5 M;
    - `GlobalFacts::of`, 64.8 M.
  - **The 11,459 cache hits: ≈1.12 G**, ≈98 K per entry, ≈23 per stored byte:
    - decoding, 985.6 M: `BuiltDecl` 452.7 M, `FileBodies` 338.4 M, the references 86.3 M plus 30.0 M to make their sets, `LineIndex` 51.0 M, `DeclaredClass` 29.8 M, the rest 27.4 M;
    - the checksum, 23.7 M;
    - the key's content hash, 19.7 M;
    - the reads, 29.1 M;
    - the entry's path, 30.8 M (`format!` and `Path::join`, twice per hit).
- **`from_def`** falls from 1,519.7 M (12,546 calls) to 202.1 M (2,005 calls): the analysed files' 1,932 classes and 73 anonymous ones.
- **E4, now measured.** The analysed files' products cost 548 M, which matches E4's ≈0.6 G gross. Decoding their 2,792 entries at the context entries' rate would cost ≈0.25-0.3 G, so E4 nets ≈0.25-0.3 G. More once E12 leaves the methods lazy.
- **Two costs the warm per-file cache adds:**
  - **E11, +116 M in the finishing passes.** The purity and property-write classifiers read 103,921 summaries from cached files, and each read hashes the file's path (`HashMap<PathBuf, FileId>::get`, ≈1.1 K Ir each). The same 121,031 reads cost 11.1 M in all with the cache off.
  - **E12, ≈0.4 G in the serial insert.** `record_assert_tagged_names` decodes 51,750 lazily stored methods of cached classes, 419.2 M. All of them, to read whether their assert list is empty. Without that, the insert would be ≈0.18 G: what the cache-off insert pays besides the tree drops.

### B4. Wall time: the stub build is 0.37 s of a 2.68 s off run

- **How.** The gate binary of `4db8f996`, whose source hash (`558a2b69…`) is the profile's, ran on Piwigo in six cache modes, round-robin for 8 rounds (`~/.cache/prs-profile/off-ledger-b/b4/scripts/interleave.py`). Each round started at the next mode.
  - The machine was otherwise idle. The load average, 0.9-6, is the runs' own.
  - Two earlier hyperfine passes ran at a load of 16-22 and disagreed by a factor of two. They are not used.

  | mode | median wall, s | min-max | user CPU, s | system CPU, s |
  |---|---:|---:|---:|---:|
  | off | 2.681 | 2.48-3.52 | 9.54 | 1.92 |
  | stub cache cold (the profile's cold run) | 2.693 | 2.52-3.54 | 9.41 | 1.97 |
  | stub cache warm (the profile's warm run) | 2.308 | 2.11-2.50 | 8.28 | 1.94 |
  | stub cache warm, per-file cache cold | 2.405 | 2.29-2.93 | 8.46 | 2.45 |
  | both warm | 1.595 | 1.53-1.95 | 5.67 | 1.03 |
  | both cold | 2.859 | 2.74-3.18 | 10.02 | 2.45 |

- **The stub build costs 0.37 s** (off against stub-warm). That is 14% of the off run's wall time, against 20% of its Ir.
- **Where the phases sit.** The profile's `fp/dhat` binary was recorded with perf, frame pointers, at 1,999 Hz (`b4/perf/`). Under perf a run takes ≈10% longer. A thread that runs all the time samples as 0.5-0.9 cores, so the core counts read about a quarter low.

  | stretch of the off run (2.95 s under perf) | s | cores busy |
  |---|---|---:|
  | start-up and vendor discovery | 0.00-0.15 | ≈1 |
  | stub sources, on the global pool | 0.15-0.35 | ≈4 |
  | stub corpus and plain base | 0.35-0.60 | 1 |
  | parse | 0.60-1.40 | ≈4 |
  | PHPStan's base, at `expect_files` | 1.43-1.60 | 1 |
  | insert | 1.60-1.80 | 1 |
  | finishing passes | 1.80-2.50 | ≈1-3 |
  | check | 2.50-2.90 | ≈5 |

  - **The stub stretches** take ≈0.6 s under perf, ≈0.4 s of it on one core.
  - **Proposals 1-5** cut into these stretches, so their wall-time ceiling is the 0.37 s the warm stub cache saves today. Proposal 5 (the corpus beside the parse) would hide the first serial stretch, ≈0.25 s, behind the parse. The second, PHPStan's base at ≈0.17 s, stays on the critical path.
  - **Proposal 6** takes all 0.37 s.
- **The other modes.**
  - Stub-warm (2.65 s under perf): parse 0.1-1.0 s, insert 1.0-1.2 s, finishing 1.2-2.1 s, check 2.1-2.6 s.
  - Both warm (1.65 s): parse 0.1-0.45 s, insert 0.45-0.55 s (nearly all E12's decoding), finishing 0.55-1.2 s, check 1.2-1.6 s.
- **What the wall time shows that Ir does not:**
  - **The finishing passes are the longest stretch after the parse.** They run 0.65-0.9 s at 1-3 cores in every mode. With both caches warm they are ≈40% of the wall time, against 18% of the Ir (2.62 of 14.34 G).
  - **The insert is serial.** It takes ≈0.2 s with the per-file cache off, mostly the tree drops, and ≈0.1 s with it warm, mostly E12.
  - **The per-file cache saves 0.71 s** of the stub-warm run's 2.31 s (31%), against 30% of the Ir.

### B6. The cache writes: ≈10 ms for the stub cache, ≈0.1 s of wall for a cold per-file cache

- **The stub cache.** A cold run writes 4 files, 31.4 MB, each through a temporary and a rename (`stub_cache.rs:73-90`). The writes take ≈10 ms under strace, and all of the stub cache's system calls ≈19 ms.
  - The wall cost is below the noise: cold 2.693 s against off 2.681 s, with a spread of ±0.3 s.
  - With the 58 M of encoding (≈15 ms), the cold run costs what the off run does.
- **The per-file cache.** The profile's runs had it off. The first run with it on writes 11,459 entries, 48.3 MB.
  - Per entry, `store` (`file_cache.rs:223-261`) calls `create_dir_all` (a `mkdir`; 11,473 in all), then `getpid` for the temporary's name, then `openat` with `O_CREAT`, `write`, `close` and `rename`.
  - When the shard exists, `create_dir_all`'s `mkdir` fails with `EEXIST` and it stats the shard: 11,208 `statx` on the workers.
  - Then `trim` lists the directory and stats every entry once: 11,459 `statx`, on the main thread (`session.rs:786`).
- **Its cost.** Perf, one run each, against the stub-warm recording (`b6/classify.py` over `b6/filescold.data` and `b4/perf/stubwarm.data`):

  | | CPU, ms |
  |---|---:|
  | `openat` | +315 |
  | `rename` | +111 |
  | `mkdir` | +80 |
  | `write` | +76 |
  | `statx`, `getdents64`, `close` | +47 |
  | postcard encoding | +191 |

  The interleaved runs give +0.51 s of system CPU, +0.18 s of user CPU and +0.10 s of wall (2.405 s against 2.308 s). The parse workers share the cost. Single perf runs are noisy: the encoding alone is more than the interleaved runs' user CPU.
- **E13** removes the `mkdir`s and the shard `statx`s.
- **Strace's per-call times are not usable here.** Under `-f` it serializes the threads, so the 11,459 writes show as 3.1 s. Its call counts are exact.

### B7. 13.8% of the questions are one node against itself; 164,144 pass the shortcuts, and E6 nets ≈ −10 M

- **Method.** A probe build (worktree at `2a7befd3`, gate profile) counts at the top of `is_super_type_of` (`super_type.rs:70`):
  - whether the operands are one node (`Arc::ptr_eq`) or equal nodes;
  - whether the hierarchy is structural;
  - where an equal pair is answered: the scalar-value shortcut, the `mixed`/`never` shortcut, or past them, where E6's test would sit.

  The run is Piwigo, off mode, once. The data is in `~/.cache/prs-profile/off-ledger-b/b7/`.
- **The counts.** 1,479,785 calls, 1.9% more than the off profile's 1,451,853 (the warm run's is 1,341,821). The code is the same: the probe counts every entry, inlined or not, while callgrind counts only the calls into the function's own code, in a build of another profile. The check asks 1,156,025 of them.

  | | calls | of all |
  |---|---:|---:|
  | one node against itself | 203,478 | 13.8% |
  | … answered by the shortcuts | 39,334 | 2.7% |
  | … past them, structural (the memo) | 67,631 | 4.6% |
  | … past them, registry-backed | 96,513 | 6.5% |
  | equal nodes that are not one | 8,713 | 0.6% |
  | … past the shortcuts | 820 | 0.06% |

- **Identity is the right test.** Interning makes equal types one node in all but 0.6% of the calls. Testing `==` past the shortcuts would catch 820 more pairs, and on equal hashes it compares the forms, which costs more than it saves.
- **E6's worth.** At E6's per-pair savings:
  - structural: 67,631 × ≈35-40 = ≈2.4-2.7 M;
  - registry-backed: 96,513 × ≈100-110 = ≈9.7-10.6 M;
  - in all ≈12-13 M.

  The test itself is a pointer compare on each call that passes the shortcuts, ≈0.9-1.0 M calls (N2: 891,411 in the profile) at ≈2 each, ≈2 M. Net ≈ −10 M, as E6 estimated at 10%.

### B9. The compile-time cost: no second compile for native builds when the override copies the profile; 0.6-0.9 s of stub build; up to ≈20 s of lost pipelining per edit to the reflection crate

- **Method.** The reflection crate does not exist yet, so today's `phpstan-rs` library stands in for it, which gives an upper bound. Builds ran from a worktree at `3324bacd`, under `nice -n 19` on the quiet 8-core machine. Each was the only job, and `/proc/loadavg` was logged before it. Data is in `~/.cache/prs-profile/off-ledger-b/b9/`.
  - **The opt-level-1 build** stands in for `[profile.release.build-override] opt-level = 1`. It is release with `opt-level = 1`, `codegen-units = 256` (the override's default), no LTO, and incremental. It ran with `cargo build --timings`.
  - **The stub build** is timed on a project of two files, one of them naming a `PHPStan\` class, so all four containers are built (31.35 MB). Each figure is the off run (`PHPSTAN_RS_NO_STUB_CACHE`) minus the stub-warm run: wall from hyperfine's medians of 5, CPU from its means.
  - **Cargo's unit sharing** was tested on a three-crate toy workspace (`b9/share/`). A crate is a normal and a build dependency of a second crate, whose build script writes the data that its `include_bytes!` embeds.
- **Is the crate compiled twice?** Cargo builds a crate that is both a normal and a build dependency once, if no `--target` is passed and the override's settings equal the profile's. Counted from `cargo build -v`:

  | profile | override | `refl` compiles |
  |---|---|---:|
  | test | none | 2 |
  | test | `opt-level = 1`, `debug = true` (equal to the profile) | 1 |
  | test | the same, plus `codegen-units = 256` | 2 |
  | release (thin LTO) | none | 2 |
  | release (thin LTO) | `opt-level = 3`, `codegen-units = 1` | 1 |
  | gate | `opt-level = 3`, `codegen-units = 16` | 1 |
  | test, with `--target` | equal | 2 |

  - **Shared:** `cargo test`, which is the development loop and `rust-ci.yml`'s, the gate profile, and a native `cargo build --release`.
    - Each gets an override equal to its profile, so the build script runs at that profile's opt-level.
    - An unset field must stay unset. The test profile passes no `-C codegen-units`, so an override that sets 256 is a different unit.
  - **Not shared:** every leg of `release.yml`, the native ones too, since each passes `--target` (`release.yml:101`). Those compile the reflection crate a second time for the host.
- **A clean build.** The opt-level-1 host build of the whole library takes 2 min 44 s of wall and 994 s of user CPU, alone on 8 cores. Its units sum to 721 s.
  - **The reflection crate's share.** The closure of `syntax`, `types`, `phpdoc`, `diagnostics`, `stub-sources` and the outside crates the registry uses (rayon, imbl, hashbrown, postcard, twox-hash, crc32fast, rustc-hash, serde, memchr, arrayvec) is 86 units and 139 s. The module graph also lets reflection depend on `composer` and `config` (`gates/module-graph.txt`), whose closure adds 15 units and 23 s (toml, serde_json and theirs): 101 units and 162 s.
    - With at most the main crate's 54 s, that is ≈160-215 s, or 22-30% of the units.
    - The closure's last unit ends at 80 s, so its critical path is ≈80 s plus the reflection crate itself.
    - None of `sqlparser`, the SQL drivers, aws-lc, lsp-types or clap is in the closure. Those are most of the rest.
  - **On `release.yml`'s legs,** that ≈160-215 s of units is added, at the override's opt-level and on the runner's cores. It runs beside the target build, which on this machine takes 9 min 36 s for the gate profile (`~/.cache/prs-b/gate-build.log`, load not logged).
- **The stub build in the build script.** All four containers:

  | opt-level | wall | CPU (user + sys) |
  |---|---:|---:|
  | 0 (the override's default) | 3.67 s | 12.2 s |
  | 1 | 0.90 s | 2.5 s |
  | 3 (gate) | 0.59 s | 1.8 s |

  - Opt-level 1 is 4.1× faster than 0, within the test profile's 4-6×, and 1.5× slower than 3.
  - **The output is the same at every level.** The four files compare equal byte for byte at opt-levels 0, 1 and 3.
  - Under C8's form, the build script parses every stub file and builds no base: by §4.1's split, ≈3.50 G of today's 5.05 G. Not measured.
- **The embedding.** A crate holding the four files through `include_bytes!` takes 0.3-1.0 s to compile in release: 0.97 s the first time, +0.3-0.4 s on a rebuild. A thin-LTO relink of a binary that uses it takes 2.79 s, against 2.73 s for one that does not. These timings, and the edit's codegen share below, were read off the console and not kept; the unit sharing above is also visible in `b9/share/target-*`, one `refl` rlib per compile.
- **An edit to the reflection crate.** At opt-level 1, the host rebuild of today's library takes 26 s of wall and 89 s of CPU after a one-line edit to `registry/index.rs`, 87% of it codegen. A comment added in `types` takes 30 s.
  - **With the unit shared,** nothing is compiled twice. A serial stretch is added instead:
    - the build script needs the reflection crate's rlib, not just its metadata;
    - the crate that takes the data cannot be below reflection (C8: cargo rejects the cycle), so it waits for reflection's codegen, which was 22 s here for today's whole library;
    - then come the build script's link, the stub build (0.6-0.9 s) and the data crate (≈0.4 s).

    Without the build script, that crate starts on reflection's metadata, and its compile overlaps reflection's codegen. The cost is about the smaller of the two compiles, plus ≈1.5 s and the link.
  - **The mitigation.** Only the binary crate, its tests and the crates that test the data take the data crate, and they hand it down. The libraries above reflection then compile as they do today.
  - **With the unit not shared** (`--target`), the host rebuild runs beside the target one, at opt-level 1 against 3, so it ends first.
  - **An edit to any other crate** reruns nothing.
- **For D3.** The override should equal the profile in test, gate and release. Dev's opt-level is 0, so it chooses between 3.7 s of stub build per rerun (equal) and a second compile at opt-level 1. The compile cost is then:
  - a second compile only on the release legs, ≈2.7-3.6 min of units;
  - 0.6-0.9 s of stub build and ≈0.4 s of embedding per edit to reflection or below;
  - the lost pipelining, unless the data enters at the binary crate.

### B10. The stub build folds no `**` and reads nothing of the platform: the cross-compiled legs embed what their targets build

- **Method.**
  - B7's probe build also logs every `powf` call of constant folding (`arithmetic.rs:715`, `:717`), with its operands, its result, whether the result is used, and the phase. It ran in the off mode on B9's project of two files, one naming a `PHPStan\` class, so all four containers are built. It also ran over Piwigo (B7's run).
  - The stub sources (`phpstorm-stubs`, `phpstan-src/src`, `phpstan-src/stubs`) were searched for `**` and `**=` with PHP's tokenizer.
  - The crates were searched for platform-dependent code: libm calls, `cfg` on the target, `MAIN_SEPARATOR`, native-endian bytes, the pointer width.
  - The gate binary built the four files with `PHPSTAN_RS_THREADS` at 1, 2, 3 and 8, with `RAYON_NUM_THREADS=1`, and on one CPU (`taskset -c 0`). B9 built them at opt-levels 0, 1 and 3. Data is in `~/.cache/prs-profile/off-ledger-b/b10/`.
  - Nothing ran on aarch64 or macOS. This machine has no arm64 emulation registered, and registering qemu's binfmt handlers changes the host.
- **`powf` is never called.** The two-file run makes 0 calls, the stub build included, and so does Piwigo's whole off run.
  - The stub sources hold four `**`, all in `IntegerRangeType`'s exponentiation (`phpstan-src/src/Type/IntegerRangeType.php:684-704`).
  - Their operands are method results, which have no literal value to fold.
- **Nothing else reads the platform.**
  - Folding's other float operations are `+`, `-`, `*`, `/` and comparisons, which IEEE 754 makes exact on every target.
  - `%` folds on integers (`arithmetic.rs`'s `Mod` arm), and `php_float_to_int`'s `rem_euclid` (`coercion.rs:329`) is exact. No other libm function is called.
  - Floats are parsed (`str::parse`) and formatted (`format_php_float`) by Rust's code, not the C library's.
  - `folded_hash` is fixed (D3), and all five release targets are 64-bit and little-endian.
  - The one target-dependent value is `MAIN_SEPARATOR`, for `DIRECTORY_SEPARATOR` in include paths (`checker/includes.rs:214`). The Windows leg is native.
- **Nor scheduling, nor optimization.** The four files are byte-identical across 1, 2, 3 and 8 threads, one rayon thread, one CPU, and opt-levels 0, 1 and 3. The merge runs in file order (`registry/stubs.rs:1479-1480`).
- **The answer.** With today's sources, the data that the `aarch64-unknown-linux-gnu` and `x86_64-apple-darwin` build scripts write equals what those targets build at run time.
- **What would break it: a stub that folds a float power.** Examples are a constant such as `2 ** 0.5`, or an integer power that overflows into a float.
  - The data would then hold a result of the build host's `pow`.
  - Rust documents `powf`'s precision as varying by platform.
- **The test D3 requires.**
  - **A host-side guard.** A test builds the stubs and fails if a fold used a `powf` result. It runs in `cargo test` with no emulation, and it fails the day a stub brings a float power.
  - **Or no platform `pow` at all.** Fold `**` with a pow written in Rust from basic operations, such as the `libm` crate's (to be checked for arch-specific paths). The equality then holds by construction, at run time too.
  - **An equality test on the target**, as a backstop:
    - the aarch64 leg can run its binary under qemu-user on the x86_64 runner;
    - the `x86_64-apple-darwin` leg can run its binary under Rosetta 2, where the arm64 runner has it.

    Each compares a runtime build (`PHPSTAN_RS_NO_STUB_CACHE`) with the embedded data.

## Investigations in the code or the data, no build

| # | status | question | settles |
|---|---|---|---|
| C1 | DONE (C1 below) | The tag lexer (`phpdoc/src/grammar/lexer.rs`): does any lexer state cross the point where the parser would stop at a description? If it does, a fresh lexer started at the offset gives different tokens (§10.7). | whether proposal 7's design holds |
| C2 | DONE (C2 below) | Deadlock through `install` (§10.5): can a stolen job wait on the stub corpus's own `LazyLock` when `install` is called from a third pool's thread, such as the one the test at `incremental_updates.rs:1155-1175` makes? | whether proposal 5 is safe as designed |
| C3 | DONE (C3 below) | Why encoding costs differ per byte between the two stub bases (§10.2). The file sizes (5.27 MB against 15.33 MB) would put the plain base's stores at about a quarter of 814.9 M, below the 0.26-0.36 G bound the code gives. Unexplained. | the overlap of proposals 1 and 2 |
| C4 | DONE (C4 below) | Which stub base each finishing run served (§10.1). One run cost 117.2 M on its calling thread; the profile can't say which base it was. | proposal 1's saving on the pool |
| C5 | DONE (C5 below) | The ≈211 M for the two stub bases' finishing runs (§4.4) is an estimate, because stolen jobs move cost between blocks and the profile has no calling context (§12). | proposals 1 and 3's share of the finishing passes |
| C6 | DONE (C6 below) | Whether a file's parse verdict depends on anything besides its bytes and the `syntax` crate (§10.8, D1). If it does, a verdict cache keyed by those two can go stale. | whether D1's verdict cache is exact |
| C7 | DONE (C7 below) | Which of PHP's compile-time errors the parser does not report. A `goto` to a label that does not exist is one: PHP refuses the file (`~/.cache/prs-probe/syntax-in-body/goto.php`), but the parser only records labels (`GotoScope`, `syntax/src/parser.rs:103`), so the file loads. The list of PHP's compile-time errors (`zend_compile.c`) against the parser's errors gives the rest. | how far a failed parse matches PHP's, with or without proposal 8 (D6) |
| C8 | DONE (C8 below) | Whether D3's embedded form holds once the crate split's stub layer lands (§10.6). D3 assumes data that is a function of the source alone, with the PHP versions applied after the load, embedded by a build script that runs the reflection layer. The crate split's C5 keys the stub layer by PHP version and parses per version, its C4 resolves stub classes lazily, and its Appendix F makes the stub index a checked-in file that an xtask generates and a regeneration test verifies. Read from the plan and the index, with the names ten projects reference: no bases are left to embed, only each stub file's declarations; a run reaches 6-43% of the stub bytes (10-58% at the upper bounds), so the saving falls to ≈0.2-1.5 G per cold run; the version key should keep the gates as data, or the data is no longer a function of the source; checked-in binary data is unfit. | proposal 6's embedded form and saving; whether D3 stands: re-decided after C5's version key |

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
  - **On worker 09,** that closure ran 10,485 = 3 × 3,495 times. So did the other per-class and per-function closures of that run: `resolve_deferred_value_of` 3,495, and `classify_purity` 3,495 (classes) and 5,282 (functions). These are PHPStan's base's 3,495 classes and 5,282 functions. (`drop_doc_members_excluded_by_native` runs one closure per signature, methods and functions alike, `resolve.rs:1742`: 34,969 calls on worker 09, so it does not count functions alone.)
  - **On the other workers,** `classify_purity`'s class closure ran 5,448 times on worker 02 and 11,720 times on worker 04: 17,168 = 1,542 + 15,626. That is the plain base and the project.
- **So** worker 09 ran PHPStan's base's finishing run, and the plain base's run shared worker 02 with the project's run. Proposal 1, which removes the plain base, removes the run that is not the 117.2 M one (C5).

### C5. The two stub bases' finishing runs cost ≈0.15-0.17 G, not ≈0.21 G

- **PHPStan's base: 117.2 M, measured.** Nothing of its run ran on another worker: every closure counted in C4 ran on worker 09 exactly once per class or function of that base. The main thread was blocked in `install` and the pool had no other work, so everything under that frame on worker 09 is that run's.
- **The plain base: ≈35-52 M, estimated.** Its run shares worker 02 with the project's run, and the counts cannot separate their costs.
  - Its classes and functions are a subset of PHPStan's base (`builtin_stubs` filters out `entry.phpstan`, `stubs.rs:937-944`), so it costs at most about 117.2 M.
  - The passes' work is mostly per class. Scaled by class count (1,542 against 3,495), it is ≈52 M. Scaled by the class tables' bytes (4.40 against 14.69 MB), it is ≈35 M.
- **Together, ≈150-170 M** against §4.4's ≈211 M. The ≈40-60 M left over in §4.4's block differences is cost that stolen jobs moved between blocks, not stub-base work.
- **Proposal 1's saving on the pool** is the plain base's run: **≈35-50 M**, not the 117.2 M that §10.1 names.
- **Proposal 3** is unaffected. Its bisection (193.6 M in the finishing passes) was carved out of the blocks before the ≈211 M was summed.

### C6. A file's parse verdict depends only on its bytes and the `syntax` crate

- **The only step of a load that fails** is `syntax::parse_watching` (`project.rs:2892`, through `parse_with_ignores`). What `parse_file_with_names` does after it, the declarations and the references, returns no error (`project.rs:3003-3071`).
- **The watch list** is the one input from outside `syntax`, and it sets only bits of `facts.mentions` (`syntax/src/lexer.rs:416-419`), never an error.
- **No stub mode.** `is_stub` is read only after the parse, by `file_declarations::collect`.
- **No feature changes the parse.** `syntax`'s only feature, `fuzzing`, derives `Arbitrary` (`php_string.rs:13`, `name.rs:263`).
- **So the key** is the file's content hash and a hash of `crates/syntax` (its sources and manifest), the workspace manifest (the profiles), `Cargo.lock` (its dependencies) and the target, computed as `build.rs` computes the build's source hash.
- **A panic is not a verdict.** A parse that panics leaves the file unread (`session.rs:661-666`), and nothing caches it.

### C7. The parser rejects 30 of the 268 compile-time errors PHP raises; the other 238 files load

- **Method.**
  - **PHP's side.** Every site in php-src 8.5.4 (`~/.cache/php-src/src-8.5.4`, the installed php's version) that raises a compile error:
    - `E_COMPILE_ERROR`;
    - the `CompileError` and `ParseError` exceptions;
    - the `E_ERROR` that attributes raise while compiling.

    That is 229 sites in `zend_compile.c`, 54 in `zend_inheritance.c`, and the rest in `zend_enum.c`, `zend_API.c`, `zend_opcode.c`, `zend_attributes.c` and the scanner.
  - **At least one trigger file per site,** 299 files in `~/.cache/prs-probe/c7/cases/`: `:7639` has four, and 16 sites whose first trigger hit another message have a second (`r2`). Each file is run twice:
    - with `php -l`;
    - with `echo "RAN\n";` as its first statement. An error that comes before `RAN` is a compile-time refusal: nothing in the file runs, and no code after its `include` runs (a fatal error ends the process; D1).

    `scripts/run.py` makes both runs. `verdicts.tsv` holds each file's result.
  - **Our side: run.** `phpstan-rs check`, which parses only, ran over the 299 files. The binary was the one saved for `4db8f996` (`.corpus-runs/gate/bin/4db8f996/gate/`), whose `crates/syntax` is HEAD's. Its result for each file is in `verdicts_run.tsv`.
  - **A reading of `crates/syntax` came first**, and the run settled three cases it got wrong:
    - the grammar rejects `f as static g` and `f as abstract g` in a trait adaptation;
    - it accepts `namespace namespace;`.
- **Result.** 268 sites refuse the file at compile time from a trigger that hits their exact message.
  - **The parser rejects 30 of them.**
    - **20 by checks of its own, with PHP's message:**
      - destructuring (7);
      - the `foreach` key (2);
      - empty array elements (2);
      - `match` with two defaults;
      - nested ternaries (3; all three forms get the message of `a ? b : c ? d : e`);
      - `final` or `abstract` on an anonymous class (2);
      - an invalid numeric literal, and the `\u{}` escapes (3).
    - **10 by its grammar, with another message:**
      - `list()` outside a destructuring;
      - `__halt_compiler()` inside a block;
      - a braced namespace inside another one;
      - an unmatched `)`;
      - four hook forms: `get()`, `set` with two parameters, a variadic one, and one with a default;
      - `static` or `abstract` on a trait alias (2).
  - **It accepts the other 238.** PHP refuses those files, while we load their declarations. By the function that raises the error (the groups were assigned by hand and add to 236; two sites are in none):

| group | sites | examples |
|---|---|---|
| class members | 53 | an abstract method in a class that is not abstract, a property hook with no body, `readonly` without a type, a promoted property outside the constructor, an enum case with no value, `case` outside an enum |
| variables | 26 | `$this` as a parameter or a `global`, a parameter declared twice, `use ($a, $a)`, `foreach ($a as $this)` |
| constant expressions | 24 | `const X = $a;`, `static::` in a class constant, `new` in a class constant, `[...1]` |
| writes | 21 | `f() = 1;`, `$a?->b = 1;`, `$x = $a[];`, `$GLOBALS = [];`, `[$a + 1] = $b;` |
| types | 18 | `int\|int`, `?mixed`, `void\|int`, `int&A`, `\int`, `function f(void $a)`, a bad default for a typed parameter |
| control flow | 18 | `break 2;` with one loop, `break;` outside any loop, `goto` to a missing label or into a loop, `return 1;` in a `void` function, `yield` outside a function |
| calls and arguments | 17 | a positional argument after a named one, `new A(...)`, `self::` in a function, `parent::` with no parent |
| names, imports, namespaces | 16 | two functions with one name, `use X\B; use Y\B;`, code after a braced namespace, `namespace` after a statement, `namespace namespace;` |
| modifiers | 11 | `public private`, `static static`, `final abstract class` |
| attributes | 10 | `#[A(...$a)]`, `#[Attribute]` on a trait, `#[\NoDiscard]` on a `void` function |
| magic methods | 8 | `__get()` with no parameter, `static function __get`, a return type on `__construct` |
| declare | 5 | `strict_types` not first, `declare(strict_types=1) { }` |
| inheritance in one file | 5 | a final method overridden by a class declared later in the same file |
| lexer | 4 | `(real)`, an unterminated `/*`, a heredoc whose indentation mixes tabs with spaces or is too shallow |

- **When the parser fails, PHP refuses too:** that held for all 30, and the parser rejected none of the triggers PHP accepts at compile time. The gap runs the other way: PHP refuses many files that we load. Real code shows the converse failing too: the parser refuses `$x instanceof static::$name`, which PHP 8.5 accepts (D6, E14).
- **Almost all 238 are local to the file.** A pass over the tree after the parse can decide them. The checker already reports some of them as diagnostics, for instance `case` outside an enum and `goto` to a missing label (`checker/keywords.rs:180`). It reports them only in analysed files, so a context file with one of these errors keeps its declarations. Which of the 238 the checker reports was not counted.
- **What is not a compile-time refusal.** 19 triggers fail only when the declaration runs, after the statements before it have run:
  - a class declared twice;
  - the enum checks: magic methods, properties from a trait, `cases()`, `Serializable`;
  - a trait that does not exist;
  - an interface implemented twice.

  `zend_inheritance.c`'s checks refuse the file at compile time only when the parent and the interfaces are declared earlier in the same file (early binding). Otherwise they fail at declaration time. 13 of its 54 sites were triggered: the 11 compile-time ones counted above, which include the hooked-property checks and `#[\Override]` that need no other class, and the 2 declaration-time ones. The other 41 are linking checks of the same kind, and were not triggered one by one.
- **Not reached from source:**
  - PHP itself raises an earlier error first: `namespace\self` at `:1153`, `readonly` on a method at `:8253` (the modifier check), `__halt_compiler()` in a braced namespace at `:9810` (an unclosed brace).
  - The value is folded to a string first: `:11120`.
  - No input reaches them: `:11523`, `:11542`.
  - The environment raises them: the stack limit, and the scanner's encoding errors.
- **For D6.** Today's policy, where a context file that fails to parse loses its declarations, matches PHP on 30 of the 268 errors. On the other 238, the analyser declares what PHP never would. Proposal 8 does not change this split, since its pass runs the same parser.
- **For PHP 8.5 only.** The triggers and verdicts are 8.5.4's, and the parser has one grammar for every version.
  - It refuses `$s{0}`, which PHP 7 accepts. D6's lint found it in real code: rector's `build/target-repository/e2e/parse-php7-code/src/Foo.php`.
  - It accepts `(real)` and `(unset)`, which only PHP 7 accepts.

  So for a project on PHP 7 the parser refuses a valid file, and "when the parser fails, PHP refuses too" does not hold. D6 makes validity follow the configured version. With `instanceof static::$name` it does not hold on PHP 8.5 either.

### C8. After the crate split there are no bases to embed; what is left is each stub file's declarations, worth ≈0.2-1.5 G per cold run

- **Method.** No build. Scripts and outputs are in `~/.cache/prs-probe/c8/`.
  - **The plan:** `CRATE_SPLIT_PLAN.md` on master (§2's stub layer, §3.4, C4, C5, C6, Appendix F), and master's `stub-sources` crate.
  - **The names each project's files reference** (`scripts/names.php`), by PHP's tokenizer: calls, class names in code and in PHPDoc, and constants, resolved against each file's namespace and imports.
  - **Against the checked-in index** (`crates/stub-sources/index.tsv` on master; `scripts/reach.py`):
    - the reached symbols are closed over the index's ancestors (the lower bound), or also over every class name the reached stub files mention, to a fixed point (the upper bound);
    - each reached symbol brings in its declaring files, as a per-file parse memo parses them.
  - **The files a run loads:** the package's own autoload directories as the analysed files, and the context files that `composer/src/vendor.rs` discovers (`scripts/context.php`). Piwigo's analysed files are those `phpstan-rs.toml` names.
    - `context.php` also walks PSR-0 directories, which `vendor.rs` skips when `autoload_static.php` exists, as it does in all ten. That adds 32 `doctrine/couchdb` files to monolog's list; its row below is recomputed without them (`validate/c8/`). Psalm's two PSR-0 files are in its classmap anyway.
    - It leaves out `ClassLoader.php` and Piwigo's two scan-path stubs, and keeps Laravel's two files the parser rejects (D6), so each list is within three files of what the analyser loads.
  - **The phpstan-src corpus counts** when the files name a `PHPStan\` class it declares.
- **What the crate split plans.**
  - **No bases.**
    - C4 resolves each class lazily through five memo levels. Its gate: "no stub class resolves unless something reaches it".
    - C5's stub layer is process-wide and keyed by PHP version. It holds C4's memos for stub classes, and parses each file once per version, when the file is reached: `names(kind, layer)` comes from the index "without parsing".
    - Today's `stub-base` and `stub-base-phpstan`, 20.6 of the 31.35 MB, are registries after the finishing passes over every stub class. Nothing builds them once C4 lands. C4 keeps an eager pass over each stub file until C5, and restricts purity classification to analysed and context declarations.
    - C9 takes purity and global-variable inference out of reflection: "Stubs get no global-variable summaries. Builtin purity comes from `functionMetadata`." Today's bases run both, so a build script on the reflection crate of C10 could not build them either.
  - **No corpus merge.** The checked-in index answers which file declares a name. It is the crate split's stage B9, done on master: 1.66 MB of text, 17,051 symbols, written by `cargo xtask stub-index` and verified by a regeneration test (`crates/phpstan-rs/tests/stub_index.rs`). Master does not read it at run time yet: its run-time index is still built from the sources or loaded from the stub cache (`registry/stubs.rs:1482-1500`).
  - **No word on the stub cache.** It came on 2026-10-01 (`d1fe66ca`), and the plan never mentions it.
  - **The version key** is "decided first" in C5, and Appendix F has no entry for it yet.
- **What a run reaches.** All stub sources are 16.34 MB: phpstorm-stubs 9.41 MB and phpstan-src's `src` 6.93 MB.

  | project | analysed files | context files | phpstan-src | analysed files alone, lower | with context, lower | with context, upper |
  |---|---:|---:|---|---:|---:|---:|
  | amphp | 26 | 26 | no | 0.51 MB (3.1%) | 0.98 MB (6.0%) | 1.58 MB (9.6%) |
  | guzzle | 70 | 99 | no | 1.46 MB (8.9%) | 1.47 MB (9.0%) | 1.66 MB (10.1%) |
  | monolog | 125 | 7,651 | yes | 1.84 MB (11.3%) | 3.93 MB (24.1%) | 7.28 MB (44.5%) |
  | league-flysystem | 173 | 8,975 | yes | 1.13 MB (6.9%) | 4.23 MB (25.9%) | 6.60 MB (40.4%) |
  | doctrine-orm | 471 | 3,336 | yes | 1.37 MB (8.4%) | 4.13 MB (25.3%) | 6.96 MB (42.6%) |
  | composer | 308 | 658 | yes | 2.24 MB (13.7%) | 3.51 MB (21.5%) | 6.22 MB (38.1%) |
  | laravel | 1,700 | 12,405 | yes | 2.61 MB (16.0%) | 5.15 MB (31.5%) | 8.05 MB (49.3%) |
  | symfony | 11,748 | 7,386 | yes | 4.57 MB (27.9%) | 5.47 MB (33.4%) | 5.67 MB (34.7%) |
  | psalm | 1,196 | 1,361 | yes | 1.90 MB (11.6%) | 3.29 MB (20.2%) | 5.65 MB (34.6%) |
  | Piwigo | 2,792 | 11,456 | yes | 3.11 MB (19.0%) | 7.07 MB (43.2%) | 9.40 MB (57.5%) |

  - **Bounds.**
    - C5's composer locator loads context classes on demand, so a run reaches less than "with context" and more than "analysed files alone".
    - phpstan-src's files are real source with bodies, which name classes their signatures do not. So its upper bound is loose.
  - **Bytes run ahead of classes.** At the lower bounds the reach is 2-18% of the classes but 3-43% of the bytes. A phpstorm-stubs file holds a whole extension: one call to `strlen()` parses `Core/Core.php`, and the standard extension's 552 functions sit in twelve `standard_*.php` files (0.45 MB), eleven of which even amphp's run reaches.
  - **Eight of the ten need phpstan-src**, through a PHPStan extension their dependencies autoload; Piwigo's own files name PHPStan classes too.
  - **The rate.** ≈0.21 G per MB averages both corpora; phpstan-src's files are mostly bodies and phpstorm-stubs' mostly doc comments, so a run that reaches mostly one of them may cost more or less per MB.
- **What proposal 6 still saves after C4 and C5.**
  - **The parse of the reached files.** Today's average is ≈0.21 G per MB of stub source (3.50 G for 16.34 MB, §4.1). That gives ≈0.2-1.5 G per cold run at the lower bounds with context (0.1-1.0 G for the analysed files alone) and ≈0.3-2.0 G at the upper bounds, against 5.05 G today. On Piwigo, B4's stretch of the stub sources (3.50 G in ≈0.2 s on ≈4 cores, under perf) puts 1.5 G at ≈0.1 s of wall if it is parsed as widely, against the 0.37 s the stub build costs today. A file parsed on demand is parsed by the thread that reaches it, so it may be less parallel.
  - **Not resolution.** Every run pays C4's memos in memory, embedded or not, because the embedded form holds no resolved class.
  - **Not the merge and the stores** (0.62 G). They go with the bases and the index, with or without proposal 6.
- **What would be embedded: each stub file's declarations, before resolution, decoded when the file is first reached.** That is what the per-file cache stores for a context file today: its declarations as built, names, references and summaries (`project/file_cache.rs`). Its key already has a stub mode (`is_stub`).
- **The version key decides whether those declarations are a function of the source.**
  - **Parsed per version, as C5 is written.**
    - 90 phpstorm-stubs files carry version gates, 3.45 MB: 712 `PhpStormStubsElementAvailable` and 2,492 `LanguageLevelTypeAware` attributes (`validate/c8/gates.php`). The count resolves the aliases `stub_attributes` resolves (`LanguageAware`, `PhpVersionAware`, `Available`, `ElementAvailable`, in `intl.php` and `ldap.php`). Without them it is 88 files, as the plan counts.
    - Over the 15 versions the analyser knows (`types::KNOWN_PHP_VERSIONS`, 5.3-8.5), those files have 388 distinct readings, 2 to 10 per file.
    - Embedding every reading parses 19.27 MB of source instead of 3.45 MB, and the phpstorm-stubs part grows ≈2.7×. A key per range, with readings merged, has more keys still.
  - **Gates kept as data.**
    - The lexer reads the attributes as conditions on the declaration, and resolution picks by the configured range.
    - One parse per file, and a range is answered with every reading in it, which keeps every possibility.
    - The data stays a function of the source, as D3 needs, and the per-file cache needs no version in its key.
    - The index's format already gives each declaration a gate (`@from-to`), though no gate is written yet.
  - **Gates as data is the more proper form.** It is C5's version-key decision, and it should be made with this in view.
- **How it would be carried.**
  - **Checked in, as the index is: unfit.**
    - The index is text, and it changes only with the stubs or its generator.
    - The declarations are binary, and they change with every edit to the declaration model or its encoding. Each such edit would add a regenerated multi-MB file to the history.
  - **A build script, as D3 decided.**
    - It is still the only form that reaches every binary.
    - The crate that embeds the data cannot be a dependency of the reflection crate, whose code its build script runs: cargo rejects the cycle ("cyclic package dependency", `cargo metadata` on a two-crate workspace, `~/.cache/prs-probe/c8/cyc/`). So a crate above reflection hands the data in, as D3's note that reflection's own tests cannot use the data already implies.
  - **Not embedded.** The stub files go through the per-file cache, as context files do.
    - A warm run decodes only the files it reaches, and a cold run parses them, the ≈0.2-1.5 G above.
    - The cache's key holds the build's source hash, so in development every rebuild makes the next run cold.
    - After the split, a key over the crates the declarations depend on (`reflection` and the workspace crates the module graph lets it depend on: `syntax`, `types`, `phpdoc`, `stub-sources`, `composer`, `config`, `diagnostics`, `semantics`; with the manifest, `Cargo.lock` and the target, as C6 keys the verdict) keeps the entries across other edits. §10.12 rejected a narrower key only because today's stub build reads most of the main crate.
- **For D3.** Its decision, the data built at compile time into every binary, can still stand. Its form and its saving do not:
  - the four files and the eager build go, since C4 and C5 leave per-file declarations, read lazily;
  - the saving falls from 5.05 G to ≈0.2-1.5 G per cold run (≈2 G at the upper bounds);
  - its costs stay: B9's compile time, B10's cross-target test, and the declarations in `.rodata`;
  - the per-file cache, with a key over the crates the declarations depend on, does most of its job with no build script.

  So D3 is decided again once C5 sets the version key, on these numbers and B9's. Proposal 6 is not built in today's form.
- **For D7.** C4 and C5 delete the eager build that proposals 1-5 speed. Their code goes with it, so their whole value is the cold runs between now and C5, which makes building them now the only time they pay. E1's fix is needed until then too.

## Found by the investigations

| # | status | item | gates |
|---|---|---|---|
| E1 | DONE (E1 below) | The deadlock of C2, present in today's code. The fix (S1-S4 below) and its test wait on a decision. The reproduction was not run. | proposal 5 |
| E2 | DONE (E2 below) | Walk a statement's assigned and defined names only when it has `@var` tags. `check_stmt_impl` computes them for every statement, and only the `@var` targets read them. ≈ −50 M, exact. | — |
| E3 | DONE (E3 below) | Probe a method or static call for a PHPUnit assertion only when its written name starts with `assert`, ignoring case. ≈ −35 M today, ≈ −12 M once item 18.1 infers each node once. Exact. | — |
| E4 | DONE (E4 below) | Store the analysed files in the per-file cache too: their products from the entry, their tree and ignore comments from a parse. ≈0.6 G gross on a run with that cache warm, less the entries' decoding. | B5, which measures the decoding; the F2 result cache, which would remove the same cost and more |
| E5 | DONE (E5 below) | A kind code in the node's 5 free flag bits (scalar family, plain `mixed`, `never`, one scalar value), so the relation's shortcuts read one word per type. ≈ −30 M, an estimate. | — |
| E6 | DONE (E6 below) | Answer a pair of identical nodes (`Arc::ptr_eq`) with Yes before the structural memo and the registry-backed forms. ≈35-110 per pair. B7 counts 164,144 such pairs past the shortcuts: ≈ −10 M net. | B7 |
| E7 | DONE (E7 below) | A class's plain object type kept with its declaration, the constant class types as statics, and a table of the integers −1..=255. ≈ −55 M. | item 4 for the declaration's slot; item 16, which makes the same calls cheaper |
| E8 | DONE (E8 below) | A cache file's types stored once, in a table its records index, as item 6 step 3 stores names. ≈ −65 M of decoding today, ≈ −25 M after item 6 step 1. | item 6, steps 3 and 5 (the decode context and the format bump) |
| E9 | SKIP (E9 below): F6f's rewrite of the scanner covers it, and ≈1 M is left after F6f's memo | The regex scanner's literal concatenation: `format!` per product (`regex_groups.rs:390-401`). | — |
| E10 | DONE (E10 below) | Report nothing while an enclosing body has called `extract()` or `eval()`: the truncation at its end drops all of it. ≈ −35 M on Piwigo, exact. | — |
| E11 | DONE (E11 below) | Decode a cached file's origins as the one `Arc<Path>` the file table binds, so its declarations' summary reads find the file by address. ≈ −116 M with the per-file cache warm, exact. | E8's decode context (item 6, step 3) |
| E12 | DONE (E12 below) | Carry each class's assert-tagged method names in its `BuiltDecl`, so inserting a cached class decodes none of its lazily stored methods. ≈ −0.35-0.4 G with the per-file cache warm, on the serial insert, exact. | — |
| E13 | DONE (E13 below) | Make the per-file cache's 256 shard directories once, at D5's first-store check, instead of `create_dir_all` per entry, and read the process id once. ≈ −80 ms of kernel CPU in `mkdir`, and the shards' `statx`, on a run that writes the cache, ≈ −10 ms of wall. Exact. | D5 |
| E14 | DONE (E14 below) | The parser refuses `$x instanceof static::$name` (or `self::`, a class name), valid PHP: it reads a name after `instanceof` and stops before `::$name`. Six files in the installed dependencies of three trees lose every declaration as context. Read `instanceof`'s right side as `new`'s class reference. | D6 |
| E15 | DONE (E15 below) | The build's source hash leaves out `vendor/phpstan-src/stubs`, whose `.stub` files the stub build reads: an edit to them alone keeps the stub cache's key, and the cache is read stale. Hash the directory too. | — |

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
- **What D3 changes.** Once proposal 6 embeds the stub data, a run reads it with serial code only: `Registry::load` and `StubIndex::load` decode in place, with no parallel work (`registry.rs:741-751`, `stubs.rs:1237-1249`). Serial code cannot steal a job, so a lazy value over embedded data cannot deadlock. Builds remain in three places: the off mode, the reflection crate's own tests, and the build script. The stub cache on disk goes away. The fix below holds before proposal 6 and after it.
- **S1. The stubs are a value, owned by the program's entry point and handed down. Production code has no static.**
  - Each entry point makes one `Stubs` value from a `StubSource`, chosen there from the environment, and hands it to the sessions it makes. The entry points are `check-project`'s command, the language server's `Server`, `incremental-bench` and the fixtures binary's `main`. Today `cache_dir()` reads `PHPSTAN_RS_NO_STUB_CACHE` and `PHPSTAN_RS_CACHE_DIR` on every call, deep in the code (`stub_cache.rs:33-44`).
  - **`StubSource`:**
    - `Cached { dir }`: the default until proposal 6.
    - `Embedded`: the default after it.
    - `Build`: the off mode, and the reflection crate's tests.
  - **Two layers, so that nothing is circular.**
    - `Corpus` holds the stub index and data. `Corpus::build(pool)` builds it.
    - A `Base` is built by `Base::build(&Corpus, kind, pool)`, the kind being plain or with PHPStan's source.
    - A base's own build project (`project.rs:597-599`), whose finishing passes read the stub index, is given the corpus that base is built from. It never asks for the value that is still being built.
    - The build script calls the two builds directly, on its own thread. It needs no `Stubs`.
  - **The free functions of `stubs.rs` become methods of `Corpus`:** `stub_index`, `generated_stub_data`, `builtin_function_extension`, `builtin_class_extension`, `stub_file_closure`, `stub_dependent_files`, `stub_file_names`, `phpstan_stub_names` and the `generated_stub*_named` family. The `Registry` keeps an `Arc<Corpus>` (`#[serde(skip)]`).
  - **No project or registry stores the `Stubs` value.** They hold only built values: the `Arc<Corpus>` and the base.
    - The two calls that may need a base take `&Stubs` as an argument: `expect_files` (S4), and `finish_loading` for its late switch (`sync_phpstan_stubs`, `project.rs:1180-1188`). That switch is for a project that is not finished and that gains a file mentioning `PHPStan\` after `expect_files`.
    - Their production callers are the entry points and the session, which hold the value: `session.rs:791`, `:871`, `lsp.rs:428` (through `Project::load_files`, whose `expect_files` is at `project.rs:1157`), `lsp.rs:569` and `incremental_bench.rs:47`.
  - **Pool jobs cannot start or wait for a build.** They are given a `&Project` or a `&Registry`:
    - the parse (`session.rs:748-781`, `project.rs:1148-1155`);
    - the checker (`builtin_calls.rs:36`, through `Checker::registry`, `checker.rs:277`);
    - `closure_scope`, `composer_hygiene` and `architecture`.

    Neither type holds a `Stubs` value, so the type system enforces this.
  - **The partial-cache case goes away:** whatever the cache lacks is built when the entry point makes the value.
  - **The stub cache becomes testable in one process**, until proposal 6 removes it. A test makes a fresh `Stubs` with a temporary cache directory. Today a test has to spawn the binary with environment variables (`tests/file_cache.rs:26-35`), because the statics are process-wide and are forced once.
- **S2. Each layer is built on a thread of its own, and waiting blocks in the OS.**
  - Making the value only starts the corpus's build thread. A base's build thread starts when a session first asks for that kind of base.
  - `wait()` blocks in the OS until the build thread publishes. So every waiter, main or a worker of any pool, blocks without taking jobs, and re-entry is impossible anywhere. No lock is held while anything parallel runs.
  - A panic in a build is carried to every waiter (`resume_unwind`).
  - The entry point makes the value before it lists the files. The corpus then builds beside the listing and the parse. This is proposal 5, and it replaces `warm_stubs`' threads (`project.rs:549-561`).
  - **This overlap holds only because the project gets its stubs at `expect_files`.** Today the session makes its project (`session.rs:733`) before the parse (`:748`). A project that held a built corpus from its creation would wait there, so the parse would never overlap the build.
  - The parse needs no stubs, only the settings and the composer packages:
    - `index_use_sites` → `class_fn_refs_of`;
    - `note_owner` → `packages.owner` (`project.rs:1125-1136`).
- **S3. The builds' parallel work runs on the pool the entry point hands in.**
  - That is the session's pool (`worker_pool()` for `check-project`, the suite's pool for the fixtures), and `PHPSTAN_RS_THREADS` and the stack size apply to it (§10.5, §10.12). The corpus's `par_iter` (`stubs.rs:1482`) runs inside its `install`.
  - Starvation would need every worker of that pool to wait for a build. In production, only the entry points and the session hold a `Stubs` value (S1), and none of them runs on a worker of the pool.
  - The test value (below) is the one place left where a pool job can wait. `wait()` `debug_assert!`s that its caller is not a worker of the build's pool.
- **S4. The base is chosen where its files say which one, and a project cannot exist without one.**
  - A project being loaded is its own type. It holds the settings, the composer packages and the file numbering, which is all the parse reads. It has no registry.
  - `expect_files(parsed, &Stubs)` turns it into a `Project`. That step takes the corpus, and the base its files call for through `Stubs::base(kind)`, on the session's thread (proposal 1).
  - Checking a project with no base cannot compile.
  - The language server's `Server::new` (`lsp.rs:289`) holds the loading state until its first `load_files`.
- **Tests share one value.**
  - A shared `Stubs` for the hundreds of tests that make a project lives behind a `test-support` cargo feature. The crate's dev-dependency on itself enables the feature, so production code cannot reach it.
  - Its source is `Embedded` once proposal 6 lands, except in the reflection crate's own tests, where it is `Build`.
  - `Project::new()` becomes that feature's shorthand for a project made from the test value, with the plain base.
- **The test at `incremental_updates.rs:1155`:** its groups get the shared value, and they block in the OS while a build runs.
- **The regression test** for the reproduction above makes its own fresh `Stubs` with the source `Build`, in-process. It needs neither a separate binary nor environment variables.
- **Rejected:**
  - **The first proposal**, in three parts:
    1. Force the stub values where the project is made. This is a convention that each new entry point must follow, and S1's types enforce it instead.
    2. Move the build onto a plain thread only when the caller is a rayon worker. This is a conditional hop around the hidden statics, and S2's unconditional build thread removes the need for it.
    3. Assert in debug builds that no `worker_pool` thread waits. S3 narrows this to the test value.
  - **The first revision of S1-S4**, for two reasons:
    - It kept a process-wide `Stubs::shared()` and the environment reads inside `cache_dir()`. Any code could still reach the global, and the stub cache stayed testable only through a child process.
    - It left the base's own build project needing the value under construction.
  - **The second revision**, for three reasons:
    - It had a project hold its corpus from its creation. That makes the session wait before the parse, which undoes S2's overlap.
    - It let a `Project` reach the `Stubs` value. Every pool job is given a `&Project`, so the rule that no pool job waits for a build rested on S3's `debug_assert!`.
    - It predated D3. Its options named only a cache directory, and its build used `worker_pool()` itself, where the build script and the fixtures need another pool.
  - **Waiting with `rayon::ThreadPool::install` from a worker of another pool**: it steals while it waits (`in_worker_cross`).
  - **Waiting with `rayon::yield_now` in a loop**: the jobs it runs sit above the build's frame on the same stack, so the build cannot resume until they return.
  - **Building outside the lock and then publishing**: two threads could both build the 5 G corpus.
  - **A once-cell that panics on re-entry**: it turns the hang into a crash, but it doesn't fix the cause.
- **After the crate split.** C4's memo levels are per-class `OnceLock`s whose re-entry panics and leaves the cell retryable (`CRATE_SPLIT_PLAN.md`, C4), and C5's per-file parse memo is forced from pool jobs. E1's rule applies to both: nothing parallel runs under a lock a pool job can wait on. A per-file parse is serial; whether the index build uses `rayon` is still to be decided (C5).


### E2. The `@var` targets: exact, ≈ −50 M

- **What runs.** For every statement (145,081), `check_stmt_impl` calls `inline_var_annotations` (`checker/stmt.rs:624`), which calls `statement_defined_variable_names` (`:1562`). It then calls `statement_assigned_variable_names` (`:628`), and `statement_defined_variable_names` again (`:1260`).
- **What reads them.** Only the two loops over `inline_var`, the statement's `@var` assertions (`:629-636`, `:1261-1266`). The inlay-hint test and `check_foreach` (`:999`) read `inline_var` itself, which is empty either way when the statement has no tag. `statement_var_tags_of` (`:1542-1555`) returns nothing unless the statement has a doc comment holding `var`. `var_tag_nodes` ran for 1,645 statements.
- **The cost.** `statement_defined_variable_names`: 290,162 calls, 36.3 M. The direct `statement_assigned_variable_names::collect`: 43,445 calls, 12.0 M. Each name is made a `String` through `Name`'s `Display` (18.6 M of the 35.9 M in `collect`) and pushed into a fresh `Vec` (11.6 M of growth). The frees come on top, about 4 M of `check_stmt_impl`'s `mi_free`.
- **The change.** `inline_var_annotations` returns at once when `tags` is empty. `assigned` and `defined` are computed only when `inline_var` is not empty. Nothing else reads them, so the output cannot change.

### E3. The PHPUnit probe: exact, ≈ −35 M

- **What runs.** `apply_phpunit_assertion_narrowing` (`stmt.rs:704`) runs for every expression statement. `phpunit_method` (`checker/narrowing/phpunit.rs:86-116`): 86,463 calls, 55.4 M.
  - Method calls: the receiver is inferred (20,186 calls, 23.6 M), then its owner is found (6.1 M).
  - Static calls: the target class is resolved (8,324 calls, 11.1 M, ≈1,330 each), then the method is looked up (1.9 M).
  - Function calls: `resolve_function`, 12,674 calls, 3.6 M.
  - Then the owner is compared with `PHPUnit\Framework\Assert`.
- **Why the name is enough.** The name `assertion_check` reads is the written one (`method.to_ascii_lowercase()`), and every name it matches starts with `assert`: the 55 names of its arms, and the `assertis`/`assertisnot` prefixes (`:127-258`). For any other written name it returns `None`. Gating the two arms on `method`'s first six bytes (`eq_ignore_ascii_case("assert")`) cannot change the result. A dynamic name is `DynamicMethodCall` or `DynamicStaticCall`, never probed today.
- **What stays.** The function-call arm, since `use function … as` can rename an assertion. And the real assertions: Piwigo's tests make 4,553 `self::assert*` calls and 890 method calls named `assert*`, out of 33,777 static and 49,421 method calls in the analysed files (`analysis/ee/asserts.php`). Most of the static calls that statements make are therefore real assertions, and they keep their 11.1 M.
- **The saving.** ≈28 M of the method arm, ≈5 M of the static arm, and their allocations: ≈ −35 M.
- **Item 18.1 shrinks it.** The receiver's inference repeats the statement's own (`check_expr` ran it just before). Once one inference per node is built, those 23.6 M become a lookup, and E3 saves ≈12 M. It is still one condition.

### E4. The analysed files in the per-file cache: ≈0.6 G gross

- **Why it is skipped today.** `read_and_parse_file` takes `cacheable = index < context_files.len()` (`project/session.rs:762`). On a hit, `CachedFile::into_parsed` gives `ast: None` and no ignore comments (`file_cache.rs:125-153`), and checking needs both.
- **What a hit would do.** Read the entry, then parse the file with `parse_with_ignores` (`project.rs:2887-2914`) for the tree and the ignore comments, and skip the rest of `parse_file_with_names` (`:3003-3071`):
  - `file_declarations::collect` (1,799.8 M over all 14,251 files);
  - `file_references` (`deps::recorded_references`, 247.3 M);
  - `FileBodies::of_file` (`BodySummary` 343.9 M, `InitFlow` 218.4 M, and their walk);
  - `GlobalFacts::of` (218.5 M) and `PestFacts::of` (6.2 M).
- **The analysed share.** The declarations: 14-16% by count, 30% of the doc bytes, so ≈0.3 G. The bodies' products and walks: 33% of the body tokens, ≈0.2 G. Global facts and references: ≈0.14 G. Together ≈0.6 G.
- **What it does not save.** The parse, the stub attributes and the anonymous classes' names, which change the tree. The line index (≈80 M), because `syntax::parse_watching` builds it with the tokens, unless the parse is changed to take the cached one (item 11 changes how it is built anyway).
- **What it adds.**
  - Decoding 2,792 entries on a hit. Unmeasured: all three runs had the cache off (B5). At the stub cache's rates (≈35 K per class, ≈7.5 K per function decoded), the analysed files' 1,956 class-likes and 1,144 functions would cost ≈80 M, and their bodies' products more.
  - On a miss, the copy into `CachedFile::of`, the encoding and the write, as for a context file.
- **The key.** It needs nothing new. It already covers the bytes, the path, `is_stub` and `index_use_sites` (`file_cache.rs:164-178`). `class_fn_refs` also depends on `retain_source`, which a key doesn't hold, but only the language server sets it, and the server loads through `Project::load_files` (`project.rs:1142`), which never touches the per-file cache. A hit can take `class_fn_refs` from the entry, or `index_use_sites` can index the tree it has, as it does now.
- **The check.** A debug build also builds a hit's products from the tree and asserts they encode to the entry's bytes, as `store` already asserts that an entry reads back (`file_cache.rs:228-237`).
- **Against F2.** The result cache would skip an unchanged file's check as well, and with it this cost. E4 is the part that needs no reasoning about dependencies, and it holds for the files F2 must recheck.

### E5. A kind code on the node: ≈ −30 M

- **Where the cost is.** In `is_super_type_of`'s own lines: `scalar_family` (`super_type.rs:646`) 22.4 M, `is_scalar_value` (`:667`) 12.5 M, the `mixed` and `never` tests (`:77`) 7.3 M, `:81` 1.8 M, `:73` 1.6 M. Together 45.6 M, ≈34 per call. Each test loads the node's `Repr` again and matches it.
- **Room.** The node's state word keeps 16 bits of flags, and 11 are used (`HAS_SELF` … `HAS_PARAM_CONDITIONAL`, `types/src/lib.rs:154-173`). The 5 free bits hold the code: 4 bits for the family or the form (none, int, float, string, bool, null, array, object, resource, plain `mixed`, `never`: 11 values), and 1 bit for "one scalar value" (`NAN` excluded). It is set in `Node::new` from the `Repr`, once per new node (126,748 in the warm run).
- **The shortcuts then** read one word of each node and test bits: ≈10 per call instead of ≈34.
- **Not covered.** `is_form_of` (25.6 M in the check's intern block) compares the stored form with the one being interned. Kind bits do not help it. N2 said they would.
- **Other readers** of the code: `union.rs:436-437`, `super_type.rs:48`, `:170`, `:178`, `:583`, `:738`, `:759`.

### E6. Identity first: ≈ −10 M (B7)

- `Type`'s `==` is `Arc::ptr_eq`, then, on equal hashes, `repr_eq` (`types/src/lib.rs:587-593`). The relation tests `sup == sub` only at `super_type.rs:114`, inside `is_super_type_of_by_form`, after the shortcuts, the memo (structural calls) or the stack guard and the 91-per-call dispatch of `:96` (registry-backed calls), and after the `Error`, subtracted-`mixed` and NaN arms.
- **Per identical pair** it saves ≈35-40 on a structural call (the thread-local memo's lookup) and ≈100-110 on a registry-backed one. B7 counts 67,631 structural and 96,513 registry-backed identical pairs past the shortcuts, 11% of the calls: ≈12-13 M, less ≈2 M for the test on every call that reaches it.
- **Is Yes always right for one node against itself?** The arms before `:114` answer Yes for `Error` and for two `NAN`s. A subtracted `mixed~S` against itself goes through `disjoint(S, mixed~S)`, which may give Maybe today. Yes is the right answer there, so the change can only make it more exact.

### E7. Plain object types and small integers: ≈ −55 M

- **Plain object types (B8).** Of `Type::object`'s 170,696 calls, 125,364 (73%) build a class's type with no arguments:
  - `resolve_name_expr`, 95,861 calls, 33.1 M (`checker/resolve.rs:13-28`): `new Foo`, `instanceof Foo`, `Foo::` written in code;
  - `record_call_throw_point`, 16,133 calls, 5.5 M: the same `Throwable` type each time (`checker/throws.rs:106`);
  - `instance_of_any`, 13,370 calls, 4.5 M: one type per name of a fixed list, at each call (`checker/operators.rs:701-705`).

  Each call is ≈340, all in `Type::new`: hashing, the front table's compare, dropping the owned form.
- **The fix.**
  - The two fixed ones become statics: ≈10 M.
  - `resolve_name_expr`'s type is kept in the class's declaration, as a `OnceLock<Type>` beside the entry item 4's ids index, built on its first read. An unknown class still builds it. ≈ −28 M, less the slot's read.
  - `resolve_class_name`'s `String` and the `Name` made from it are item 15's.
- **Small integers (B8).** `int_literal`'s 71,550 calls (23.8 M) come from code literals (`infer_expr_unguarded` 27,993, `check_expr_impl` 22,767) and from constants' values (`infer_const_type_with` 17,329). The analysed files hold 23,463 integer literals, 357 distinct values, and 21,578 (92%) of them are 0-255 (`analysis/ee/ints.php`). A static table of −1..=255, made on first use, answers those with a clone: ≈ −20 M.
- **Item 16 overlaps.** Whatever makes an interner hit cheaper makes E7 smaller.

### E8. Types once per cache file: ≈ −65 M today, ≈ −25 M after item 6

- **Today.** `Type`'s `Deserialize` is `Repr::deserialize(..).map(Type::new)` (`types/src/lib.rs:348-351`), so decoding a record interns every node of every type in it. In the warm run that is 416,975 calls of `Type::new`, 93.5 M, ≈224 each. The decoders: `FunctionReflection`'s 213,295 calls (49.2 M), `Vec<Type>`'s 111,634 (18.4 M), and the nested types', ≈92 K.
- **At most 30% of those nodes are distinct.** The whole warm run made 126,748 new nodes, and every distinct node a decode returns was made once in the process. So at least 70% of the decode's interning finds a node that already exists.
- **The design.** A cache file holds a table of its distinct type nodes, children before parents, each entry its form with child types as table indices. A record holds an index where it holds a type today. Decoding runs under item 6 step 5's decode context, which holds the table as it holds the name table, with a `OnceLock<Type>` per entry. A type is then interned once per file, and every other occurrence is an `Arc` clone.
- **The saving.** ≥70% of the 93.5 M today, ≈ −65 M. Item 6 step 1 stops decoding 22,746 stub methods, which leaves ≈35-40 M of this decoding, so ≈ −25 M on top of item 6.
- **The per-file cache** can share the format, and that cuts E4's decoding too.

### E9. The regex scanner's concatenation: folded into F6f

- `scan_branch` starts its accumulator at `[""]` and calls `concat_literals` once per atom (`regex_groups.rs:778`). That makes a new `Vec` and one `format!` per product (`:390-401`): 6,988 calls, 3.7 M, within `parse_capture_groups`' 11.7 M over 714 scans.
- F6f's memo scans each of the 170 distinct patterns once instead of ≈3.5 times, which leaves ≈1 M. F6f's rewrite of the scanner ("scanning bytes and keeping slices", 20 G plan, `PROFILE_DRILL_PIWIGO_20G.md:1705`) replaces these `String`s anyway. Extending the left string in place when the right side is one literal belongs in that rewrite, not in a change of its own.

### E10. Reports made inside an `extract()` or `eval()` body: ≈ −35 M, exact

- **What happens.** A body that calls `extract()` or `eval()` sets `saw_dynamic_var_construct`, and every report made after its start is dropped at its end. That happens in `check_function` (`function_decl.rs:1208`) and in `check_program` (`checker.rs:1806`).
  - The flag only turns on within a body.
  - A nested function or closure saves the flag, clears it and restores it (`function_decl.rs:885-886`, `:1211`). Its reports land after the enclosing body's start, so the enclosing truncation drops them too.
  - So a report made while any enclosing body has the flag set is always dropped.
- **The change.**
  - The checker keeps a count of the enclosing bodies with the flag set.
  - `report` and `report_builder` return at once while it is not zero.
  - `check_statement_var_tag_names` also returns before it formats anything.
- **Why the output cannot change.** `report` only pushes (`checker.rs:1487-1495`), and everything it would have pushed is truncated. A debug build reports as now and asserts that the truncation drops exactly what the count skipped.
- **The saving.** ≈35 M on Piwigo (B3). It applies to any tree that analyses templates compiled with `extract()`: Latte, Blade, plain PHP views.
- **What it leaves.** The rest of such a body's check still runs, because it records facts that outlive the body, such as property assignments and class-level sites. Which of those facts a dropped body needs is not traced.

### E11. A cached file's declarations miss the origin's address: ≈ −116 M with the per-file cache warm

- **The fast path.** `FileTable::get_by_origin` (`project/file_table.rs:108-114`) finds a file by the address of the `Arc<Path>` its declarations name it by. `insert_parsed_file` binds that one `Arc` (`bind_origin`, `project.rs:1055`). A parsed file's declarations all share it (`project.rs:3017-3018`), so a summary read hashes nothing.
- **A cached file misses it.**
  - Each decoded `Origin::File` is a new `Arc<Path>`: postcard decodes every occurrence on its own (`registry/provenance.rs:63`, `:129`).
  - `CachedFile::into_parsed` binds yet another `Arc::from(path)` (`project/file_cache.rs:146`).

  So every read through a cached declaration's provenance falls back to `self.get(path)`. That is `Path`'s component-wise hash and compare: 103,921 reads, 116.2 M (B5). The entries also store and decode the path once per provenance.
- **The change.** Decode a per-file entry's file origins as the entry's own path:
  - the entry stores a marker for "this file";
  - the decode context (E8's, item 6 step 3) hands back the one `Arc` that `into_parsed` binds.

  That covers the lazily stored methods if their decode gets the same context. Otherwise, `FileTable` keyed by the path's bytes would bring the fallback down from ≈1.1 K to ≈0.1 K per lookup.
- **Exact.** The origin is the same path, and only its sharing changes. A debug build asserts that each decoded origin spells the entry's path.

### E12. Inserting a cached class decodes all its methods: ≈ −0.35-0.4 G with the per-file cache warm, on the serial insert

- **What runs.** `insert_built_decl` calls `record_assert_tagged_names` (`registry/index.rs:153`, `:461-487`). For a class, it walks `info.methods.iter()` and reads `sig.assert`. A cached class's methods are stored lazily (`name_table::Lazy`), and that read forces each one's decode: 51,750 methods, 419.2 M, on the main thread (B5).
  - With the cache off, the methods are already built, and the walk costs little.
  - Which of the 51,750 a later step would read is hidden, because the insert decodes them first. The later decodes number 35,397, against 35,398 with the cache off: the stub bases' own.
  - The check asks about 1,572 purity callees and 2,489 property-write callees (B1), so at ≈8.1 K per method, the methods it reads cost tens of M at most.
- **The change.** `BuiltDecl::Class` carries the names of its methods that have assert tags, computed when the declaration is built. The cache stores it, and `record_assert_tagged_names` reads it without touching the methods.
- **The saving.** ≈0.35-0.4 G, less any of those methods a later step decodes, on the main thread's serial insert. E12 is also a wall-time cut (B4).
- **Exact.** `names_with_assert_tags` gets the same names. A debug build compares them with the walk's.

### E13. The per-file cache's directories, made once

- **What runs.** `store` calls `fs::create_dir_all` on the entry's shard for every entry (`file_cache.rs:240`). That is 11,473 `mkdir`s on a cold Piwigo run, all but the first in each of the 256 shards failing with `EEXIST`, after which `create_dir_all` stats the shard (11,208 `statx`): ≈80 ms of kernel time in `mkdir` alone (B6). Each temporary's name also calls `std::process::id()` (`:245`), one `getpid` per entry.
- **The change.**
  - The build's directory and its 256 shards are made once, by D5's check at the first store. Making them when `directory()` (`:62`) first finds the cache on would cost every run 257 `mkdir`s, a fully warm one and a read-only cache included.
  - The process id is read once.
  - If a shard disappears during the run, the temporary's open fails and `store` skips that entry, as it does for any failed write.
- **The saving.** ≈80 ms of CPU and the shards' `statx`, and ≈10 ms of wall, on runs that write entries.
- **What stays.** The `openat`, `write` and `rename` per entry, ≈0.5 s of CPU. Packing entries into fewer files would remove them, but that changes the cache's format and its trim.

### E14. `instanceof` with a static property as the class

- **What fails.** B1's Laravel run warned twice (`b1/laravel/stderr.txt`): "parse error: expected RParen, found DoubleColon" at `aws/aws-sdk-php/src/AbstractConfigurationProvider.php:41:41` (`$found instanceof static::$interfaceClass`) and `phpseclib/phpseclib/phpseclib/Math/BigInteger.php:177:31` (`$x instanceof self::$mainEngine`). `php -l` accepts both.
- **Why.** After `instanceof`, the parser takes an expression only when a variable or `(` follows, and otherwise a name (`syntax/src/parser/expr.rs:106-117`); the `::$name` after `static` or `self` is left for the enclosing rule. PHP's grammar takes a `class_name_reference` after `instanceof`, as after `new`: a name, or a `new_variable`, which includes `Name::$prop`, `$a->b`, `$a['k']` and `$a::$b`. `parse_new_class_reference` already reads that (`primary.rs:83-188`).
- **Reach.** A tokenizer scan of every file under `vendor/` finds six, the same two files in the installed dependencies of laravel, league-flysystem and monolog (`validate/d6/instanceof_sp.tsv`). Each one loses every declaration as context today, so `phpseclib\Math\BigInteger` and `Aws\AbstractConfigurationProvider` are unknown classes in those projects.
- **The change.** Read `instanceof`'s right side with `parse_new_class_reference`: a plain name stays `InstanceofTarget::Name`, and anything else becomes `InstanceofTarget::Expr`, as a variable target does now. A parser test per form, and the two files as fixtures.
- **What it does not settle.** The parser has never been run over the whole corpus. One `phpstan-rs check` over the 111,836 files under `vendor/`, compared with D6's `php -l` verdicts, lists every file PHP accepts and the parser refuses. That is a run of the binary and was not made.

### E15. The source hash misses PHPStan's `.stub` files

- **What is hashed.** `crates/phpstan-rs/build.rs` hashes `vendor/phpstorm-stubs`, `vendor/phpstan-src/src`, `Cargo.lock`, `Cargo.toml`, every crate's `src`, `Cargo.toml`, `build.rs` and `index.tsv`, and the target. The stub cache and the per-file cache are keyed by it.
- **What is read.** `crates/stub-sources/build.rs` also embeds `vendor/phpstan-src/stubs/*.stub` (41 files, 74 KB) as `PHPSTAN_STUB_SOURCES`, and reruns when they change. `patch_phpstan_stub_classes` (`registry/stubs.rs:735`) merges their templates, variances, bounds and property types into the stub classes the stub cache stores.
- **So** an edit to a `.stub` file alone, or a `phpstan-src` bump that changes only `stubs/`, rebuilds the binary with the same `PHPSTAN_RS_SOURCE_HASH`, and its runs read the old stub cache with no error. Master's `build.rs` has the same list (`:205-206`).
- **The change.** Add `vendor/phpstan-src/stubs` to the hashed inputs. A test that the hash's inputs cover every path `stub-sources`' build script reads keeps them in step.
- **For §10.6 and D3.** "The cache's key is the build's source hash alone, so what a cold run writes is a function of the build" holds once this is in. §10.12's reason to reject a narrower key, a missed input read stale with no error, is this case.

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

**E3. A PHPUnit probe on every expression statement.** `apply_phpunit_assertion_narrowing` (`stmt.rs:704`) runs for each expression statement. `phpunit_method` (86,463 calls, 55.4 M inclusive; off: 57.2 M) infers the receiver (23.6 M), resolves a static target (11.1 M) and finds the method's owner (6.1 M) before it compares the owner with `PHPUnit\Framework\Assert`. Every name `assertion_check` reads starts with `assert`, so a method or static call whose written name doesn't is never an assertion it uses. Gating those two arms on the name saves ≈35 M (E3 below). The function-call arm stays: `resolve_function` is 3.6 M, and `use function … as` can give an assertion any name.

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
- **E5.** A kind code on the node, so the shortcuts read one word per type instead of matching the `Repr`. ≈ −30 M (E5 below).
- **E6.** The identity test before the memo and the registry-backed forms. It sits at `super_type.rs:114`, after the memo's lookup and the stack guard. B7 counts 164,144 questions it would answer, ≈ −10 M net.
- **E7.** A class's plain object type kept with its interned name, and a table of small integer nodes. B8 counts the repeats.
- **E8.** Each stub-cache file's types decoded once into a table its declarations index. ≈35-40 M is left of the decode after items 4 and 6.

### N3. The analysed files' declarations: ≈0.5-0.6 G, paid only because the per-file cache skips them

The per-file cache stores context files only: "An analysed file is always parsed, since checking needs its tree" (`project/file_cache.rs:7-8`); the session passes `cacheable` false for them (`project/session.rs:762`). Its products don't need the tree, though: only the check does.

Counted with PHP's tokenizer (`analysis/nn/decls.php`, `docs.php`):

| | files | MB | class-likes | methods | functions | closures | docs on declarations | other docs | body tokens |
|---|---|---|---|---|---|---|---|---|---|
| analysed | 2,792 | 19.07 | 1,956 | 9,357 | 1,144 | 6,561 | 6,668 (2.61 MB) | 35,377 (1.82 MB) | 1,384,375 |
| context (classmap) | 10,610 | 34.42 | 10,475 | 50,553 | 9,333 | | 41,253 (6.07 MB) | 2,573 | 2,599,131 (≈2.81 M over 11,459 files) |

The analysed files hold 13.9% of the declarations' docs by count, 30% by bytes, and 33% of the body tokens.
- `built_declaration` is 1,668.5 M (classes' `from_def` 1,517.1 M over 12,407; functions 75.4 M over 2,420). `from_def` as a whole is 1,519.7 M over 12,546, 580.7 M of it the docblock. The analysed share is ≈0.24 G by count, up to ≈0.32 G by doc bytes.
- The bodies' products (`BodySummary` 343.9, `InitFlow` 218.1, `GlobalFacts` 218.5; 780.5 M) at 33%: ≈0.26 G.
- Their line index: ≈80 M. E4 does not remove it: the parse builds the index (E4 below).

So a run with the per-file cache warm still pays ≈0.5-0.6 G for analysed files that did not change. Two things remove it: **E4**, their entries in the per-file cache, or the F2 result cache (`CRATE_SPLIT_PLAN.md:7005-7110`, not built), which skips unchanged files whole. A changed file's cost is inherent.

### N4. The regex scanner: 714 scans of 170 patterns

`scan_branch`'s 6,988 `format!` calls (3.7 M) run inside 714 `parse_capture_groups` calls, 11.7 M inclusive: 467 from `preg_matches_type`, 247 from `narrow_preg_outcome`. The analysed files hold 203 literal `preg_match` and `preg_match_all` sites, 170 distinct patterns (`analysis/nn/preg.php`), so each pattern is scanned ≈3.5 times (the type and each narrowing). F6f's memo (one analysis per distinct text) removes ≈75% of the 11.7 M. `concat_literals` (`regex_groups.rs:390-401`) builds each product with `format!("{x}{y}")`; E9, which would remove that, is folded into F6f's rewrite of the scanner.

**Reproduction.** The profiles are in `~/.cache/prs-profile/callgrind-20g-modes/` (`warm/warm.pkl`, `off/off.pkl`); `~/.cache/prs-profile/drill-scripts/q.py <pkl> self|incl|find|callers|callees|lines|tree|thread <pattern>` reads them. N1's split is `analysis/nn/n1split.py` (output `n1split-warm.txt`, `n1split-off.txt`); the counts of N3 and N4 come from the PHP scripts beside it, run over Piwigo's file lists. E3's and E7's counts come from `analysis/ee/asserts.php` and `ints.php`, run over `analysis/n5/analysed.txt`.
