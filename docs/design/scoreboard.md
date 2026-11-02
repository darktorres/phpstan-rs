# Design: the parity scoreboard (`cargo xtask parity`)

The scoreboard is the finish line of every milestone. It runs suites that compare phpstan-rs with PHPStan 2.3.4. The expected side comes from PHPStan's own tests and tools, never from our code. It writes `parity/score.json` and prints a one-line summary.

Inputs: the findings C-1, FR-5, CA-4, CA-5, EV-3, SB-6, PL-3 and F11 in `RESTART_FINDINGS.md` at `legacy-final`.

## Shape

```
cargo xtask parity [--suite <name>]... [--filter <glob>] [--diff <commit>]
```

Each suite has three stages:
1. **Extract** the expected cases into a JSON-lines file.
   - The extractors are PHP scripts in `parity/extract/`. They run under the oracle php, against `vendor/phpstan-src` and its composer dependencies.
   - Each case has a stable id, an input, and the expected output.
2. **Run** each case's input through phpstan-rs.
   - It goes through the real binary, using a hidden `phpstan-rs debug <suite>` subcommand that is built in release.
   - The binary prints its side in the same JSON-lines shape.
   - This keeps one load path (D20-8): the scoreboard measures what ships.
3. **Compare** the two per case, by the suite's rule below.
   - Each case gets one result: `pass`, `fail` (with a short diff), `excluded` (listed in `parity/exclusions.md`) or `error` (phpstan-rs crashed or printed nothing for it).

Extraction is deterministic from pinned inputs. Its output is cached under `~/.cache/phpstan-rs/parity/<suite>/<key>/`. The key hashes:
- every extractor script the run reads;
- the php flags every extractor runs with: `-d memory_limit=-1 -d short_open_tag=1 -d error_reporting=-1 -d display_errors=stderr`. They make the local php.ini and the image's (which has none) give the same run: `short_open_tag` changes how `<?` lexes (the compiled default is on, a distribution's php.ini turns it off), and the CLI's compiled default prints diagnostics to stdout, into the output;
- the extractor's arguments and its stdin;
- the oracle php's `php -v`;
- the inputs' content (the submodule commit, phpstan-src's installed dependencies, and `parity/pins.toml`).

The pinned inputs from outside the repository (nikic's repository, php-src's tarball) are fetched once into `~/.cache/phpstan-rs/parity/pins/<name>-<commit or sha256>`, checked against their pin, and renamed into place when complete.

A cache hit skips stage 1. The cache is never committed. A full run (every suite, no filter) then removes what it did not use: the other entries of each extraction directory, stored inputs no list names, and pins no longer pinned. That keeps a cache restored from an older run, as CI's is, from growing.

## Oracle pins

`parity/pins.toml` holds every input, each a released version fetched by digest (PL-3):
- the phar: 2.3.4, sha256 pinned;
- `vendor/phpstan-src`: the 2.3.4 commit;
- phpstan-src's composer dependencies, through its `composer.lock`. nikic/php-parser v5.9.0, phpdoc-parser 2.4.0 and PHPUnit 11.5.57 come from there;
- nikic/php-parser's source at v5.9.0 and phpdoc-parser's at 2.4.0, for their test data, which composer's dist leaves out;
- php-src at 8.5.4, for the `.phpt` corpus;
- **the oracle php: 8.5.4.** Locally it is the installed php, and the scoreboard refuses any other version. CI uses the `php:8.5.4-cli` image, pinned by digest;
- composer 2.9.5, pinned by digest, which installs phpstan-src's dependencies.

**Which nsrt files PHPStan runs depends on `PHP_VERSION_ID`** (`// lint` headers, and the conditions in `NodeScopeResolverTest::findTestFiles`). The scoreboard therefore measures exactly "PHPStan's suite as it runs under PHP 8.5.4".

## Suites

### lexer
- **Oracle:** nikic/php-parser's `Lexer::tokenize`, with a collecting error handler.
  - PHPStan lexes with nikic's lexers (`conf/parsers.neon`, `LexerFactory`): `Lexer\Emulative` for the newest version, and `Lexer` (or `Emulative` at the configured version) for analysed files.
  - nikic v5.9.0's newest supported version is 8.5, the oracle php's version, so no emulator is active and both lexers give `Lexer::tokenize`'s output. That is `PhpToken::tokenize` plus nikic's post-processing: the two `&` token kinds, the sentinel token and the bad-character and unterminated-comment errors.
- **Inputs:** the parser suite's inputs.
- **Line breaks:** each input runs as it is, and with every `\r\n`, `\r` and `\n` rewritten to `\r\n` and to a lone `\r` (CP-6). Case ids are the path, then `#crlf` or `#cr` for the rewrites.
- **Comparison:** each case's token stream, written one token per line as `kind pos length line` (kind is the `T_*` name, the character itself, or `EOF` for the sentinel), is compared by its SHA-256 and token count, and its errors (message, byte offset) by equality. Digests keep the cases file small; `--show <id>` reruns both sides for one case and prints the first differing token.
- **Unported:** until M1, `phpstan-rs debug lexer` answers that the lexer is not ported, and every case is `fail` with that reason.

### parser
- **Oracle:** the AST PHPStan gets when it parses a file (`SimpleParser`, `RichParser`):
  - nikic v5.9.0's `Php8` parser over `Lexer\Emulative`, with a collecting error handler. Any collected error rejects the file;
  - then nikic's `NameResolver` with `preserveOriginalNames`, as `conf/services.neon` configures it. An `Error` it throws (a conflicting `use`, an invalid class name) is a parse error too;
  - each error is what `FileAnalyser` reports as `phpstan.parse`: `getMessage()` and the start line.

  `RichParser`'s other visitors annotate the tree for the analyser, so they belong to M5a's tests, not to this suite.
- **Inputs:**
  1. every `.php` file under `vendor/phpstan-src/tests` (data files included); the id is the `/`-spelled path, `tests/…`;
  2. the code sections of nikic's `test/code/parser` fixtures, including the error-recovery ones, read as its `CodeTestParser` reads them (canonicalized, `@@{expr}@@` evaluated). The id is `nikic/test/code/parser/<fixture>#<n>`, the fixture's n-th section from 0;
  3. php-src 8.5.4's `.phpt` `--FILE--` sections (F11), read as `run-tests.php` reads them: `===DONE===` ends a section, `--FILEEOF--` drops its trailing line breaks, `--FILE_EXTERNAL--` names a file beside the test, and `--REDIRECTTEST--` tests have no input. `--INI--` sections are not applied: every input runs under the same php flags. The id is `php-src/<test path>`;
  4. the generated operator pairs (D25-2): every ordered pair of about 70 operator forms (binary, assignment, ternary, prefix, postfix, casts and the operand-taking keywords), the inner form in the outer one's first and last operand, bare and parenthesized, each returned from a function. The id is `operators/<expression>`. `++new $a` and `--new $a` are left out: PHPStan never finishes parsing them (`parity/known-deviations.md`);
  5. `vendor/phpstan-src/vendor`, as the large vendor tree, ids `vendor/…`.

  `inputs.php` writes inputs 2–4 to a content-addressed store, `~/.cache/phpstan-rs/parity/inputs/store/<sha256>.php`, and prints a manifest; a cached manifest naming a missing file is made again. Both sides read the inputs as one `<case id>\t<file>` line each on stdin, in byte order, and `--show` takes `<case id> <file>`. A case's group is its id's first component: `tests`, `nikic`, `php-src`, `operators` or `vendor`.
- **Canonical AST:** nikic's JSON shape, made total and stable. A node is `nodeType`, then its sub-nodes in nikic's declared order, then `attributes`, with keys sorted and the token positions left out. A string that is not UTF-8 is `{"bytes": "<hex>"}`, and a float is `{"float": "<var_export>"}`, which covers `INF` and `NAN`.
- **Comparison:** the errors by equality, then the node count, then the SHA-256 of the compact canonical JSON. `--show <id>` prints both sides pretty-printed and the first differing line.
- **Exit bar:** zero false refusals over input 5. `php -l` is recorded informationally, using F11's result classes.

### phpdoc
- **Oracle:** phpdoc-parser 2.4.0, parsing as PHPStan does, with its `Lexer` and the `ParserConfig` of `conf/services.neon` (the `lines` attributes only):
  - a doc comment as `PhpDocStringResolver` does: `PhpDocParser`, then `consumeTokenType(TOKEN_END)`;
  - a type string as `TypeStringResolver` does (the function map's signatures, for one): `TypeParser`, then `consumeTokenType(TOKEN_END)`.
- **Inputs:** `phpdoc.php --list` prints one `<case id>\t<kind>\t<file>\t<offset>\t<length>` line per case, with kind `doc` or `type`. Both sides read those lines, and `--show` takes the same five values.
  - every distinct doc comment (`T_DOC_COMMENT`) of the parser suite's inputs, 41,290, at its first occurrence in input order. They are found with `PhpToken::tokenize`, so the suite doesn't depend on our lexer. The id is the input's id, `@` and the byte offset; the group is the input's;
  - the inputs of phpdoc-parser's own parser tests, 618: the `$input` or `$phpDoc` argument of every data set of `PhpDocParserTest`'s providers (docs) and `TypeParserTest`'s (types), run from phpdoc-parser's repository at the commit composer installed (pinned, and checked against `installed.json`). `testParseTagValue`'s bare tag values are left out, since PHPStan never parses one alone, and so is `ConstExprParserTest`, since PHPStan never calls `ConstExprParser` alone. They go to the inputs store as `<sha256>.txt`; the id is `phpdoc-parser/<test>::<provider>#<key>`.
- **Canonical tree:** a node is `node` (its class below `PHPStan\PhpDocParser\Ast\`), its public properties in declared order, then `attributes` with keys sorted. An `InvalidTagValueNode` also has `exception`, its parser exception's message. Bytes and floats are written as in the parser suite.
- **Comparison:** the exception `consumeTokenType` throws (its message), then the node count, then the SHA-256 of the compact canonical tree, then the SHA-256 of the printed tree (`(string) $phpDocNode`, which PHPStan's messages quote).

### Recording PHPStan's tests (types and rules)
Both suites come from **running PHPStan's own test methods without running PHPStan's analysis** (`parity/extract/tests.php`).
- The tests are every non-abstract `*Test.php` class under `tests/PHPStan` that descends from `RuleTestCase` or `TypeInferenceTestCase`, found by reading the class declarations: 509 classes, 457 of them rule tests, some outside `Rules/` (`Node`, `Analyser`, `Build`).
- PHPUnit runs them in the default order, with phpstan-src's own `phpunit.xml` settings and `parity/extract/bootstrap.php` as the bootstrap. That bootstrap puts recording `RuleTestCase` and `TypeInferenceTestCase` classes on the autoloader before phpstan-src's own. They are generated from the real classes by rewriting the methods that would analyse, so data providers run and `PHP_VERSION_ID` branches resolve under the oracle php.
- The records are written sorted; two runs give the same bytes.
- PHPUnit reports failures and errors that the stand-ins cause on purpose. An empty assert list fails, and a data provider built from `gatherAssertTypes()` errs. The extraction accepts PHPUnit's exit codes 0–2, as long as the summary line is there.

Why execute instead of parsing: 196 of 3,284 `analyse()` calls in `Rules/` pass expectations that are built in code, and 154 methods branch on `PHP_VERSION_ID`. Static parsing would get these wrong.

### types (nsrt)
- **Extraction:**
  - The stand-in `gatherAssertTypes()` records each file a type-inference test passes it, and returns no asserts: 1,986 records over 1,984 files.
  - `parity/extract/asserts.php` then reads each `assertType`, `assertNativeType`, `assertSuperType` and `assertVariableCertainty` call with nikic: the expected string or certainty, the line and column, the asserted expression's source text, and its kind (EV-3).
  - PHPStan reads a non-literal expected argument through the scope as a constant string. The extractor folds the forms the suite uses: `X::class` (with `self`, `static` and `parent` taken from the enclosing class), constants declared in the file, concatenation, `sprintf` over constants, an assignment's value, and a variable assigned once in the enclosing function. Every one of the 21,299 asserts resolves.
- **Case id:** `file:line:column:kind`. Nine lines hold two asserts, so the line alone is not unique. Cases are grouped by kind.
- **phpstan-rs side:** `debug types` reads the asserts on stdin. It analyses each file as its own project (CA-4), and prints `{"id", "actual"}` for each assert: `describe(precise)` of the asserted expression, or the certainty.
- **Comparison:** string equality. `describe()` aims at PHPStan's exact strings (CA-5), so no normalization is planned. If one is ever needed, it is listed in this doc with its reason.

### rules
- **Extraction:** the stand-in `RuleTestCase` records, for each `analyse($files, $expectedErrors)`:
  - the test class and method, the data-set name, and the call's ordinal within the test (a test can call `analyse()` more than once);
  - the files;
  - the expected `[message, line, tip?]` list;
  - the test's own property values at that moment (its toggles: `checkNullables`, `reportMaybes` and the like);
  - `getAdditionalConfigFiles()`.

  `getRule()` is never called, so no PHPStan service is built. `fix()` is recorded the same way, with its expected file. That gives 3,609 cases: 3,594 `analyse` and 15 `fix`.
- **Case id:** `Class::method#dataSet@call`, without `#dataSet` when the test has no data provider. Cases are grouped by the test's namespace below `PHPStan\`.
- **phpstan-rs side:** `debug rules` reads the records on stdin. It analyses each case's files with only the mapped rule enabled, configured by the recorded toggles, and prints `{"id", "errors"}` (or `{"id", "fixed"}` for a `fix` case).
  - The map from PHPStan test class to our rule configuration lives in the `testing` crate, beside each rule's port.
  - A case whose test class has no mapping yet is `fail` (reason: "rule not ported"), not `error`.
- **Comparison:** the multiset of `[message, line, tip]`, as `RuleTestCase::analyse` compares them; for `fix`, the fixed file's text.

### level oracle
- **Oracle:** the pinned phar from the `phpstan/phpstan` package, with the turbo extension beside it, run on every `.php` file under a `data` directory of `tests/PHPStan/Rules` (3,122 files) at every level 0–10 (`parity/extract/levels.php`). Each run is a project of its own (CA-4, OWL-2): the file and a `phpstan.neon` with the level and `phpVersion` as the range 70100–80599 (CA-4: never one version). It runs outside any `vendor/`, with no bleeding edge and no other configuration (FR-5).
  - **How it runs** (spikes S3, S4 and S5): worker processes, one per CPU, each take a batch of 500 runs. Each worker slot has one project directory holding a `level-<n>.neon` per level (with no `paths`), its `tmpDir`, and the current run's data file. A worker loads the phar once, as its `bin/phpstan` does, then runs PHPStan's own console application and `AnalyseCommand` per run, as `phpstan analyse --debug --no-progress --error-format=json -c level-<n>.neon <file>` would in that directory. Since the file is named on the command line, the compiled DI container depends only on the level, so a slot compiles 11 and reuses them; `--debug` never uses the result cache. php's opcache file cache keeps the phar's compiled code between workers. Workers load the turbo extension and activate it as the package's `bin/phpstan` does. The records equal one phar process per run with the file in `phpstan.neon`'s `paths`, byte for byte.
  - **The work directory** is the cache key's own, and `levels.php` empties it when it starts: compiled containers and opcache's code made under one php setup (turbo or not, a profiler extension or not) crash under another, and the key sees `php -v`, not the extensions.
  - A worker that dies keeps its finished runs; its run is retried alone and the rest are queued again. `levels.log` in the work directory has a line per finished batch, per death, and per run that printed no JSON.
  - A run where PHPStan itself crashes (it does on `Keywords/data/declare-position-nested.php`, `ShouldNotHappenException` in `MissingReturnRule`) records its exit code and the uncaught exception's message line.
- **Records:** one per file and level, id `<file>@<level>`, group the rule family (the directory under `Rules`): the exit code, the findings (line, identifier, message, tip, ignorable; sorted by line, identifier and message) and PHPStan's other errors. Paths are project-relative (OM-2).
- **phpstan-rs side:** `debug levels` reads the same `<id>\t<file>` lines and prints the same records, as `phpstan-rs analyse` on the same projects would report.
- **Comparison:** exact. A case passes when the exit code, the findings and the other errors are equal. A failing case's reason counts, per identifier, the findings only PHPStan reported and those only phpstan-rs did. (FR-5's near-line matching was for the legacy, whose lines were often off; a port is held to the same lines.)
- **When it runs:** on every push, like every suite, from its cached extraction, which reruns only when its own inputs change (the phar, phpstan-src, `levels.php`, the oracle php). A cold extraction is 34,342 runs, about 0.9 CPU-hours (spike S5).

### end-to-end
- **Projects:** monolog 3.12.1 (`src`) and symfony/console v8.1.10 (its root, without `vendor` and `Tests`), each pinned by commit in `pins.toml` with its paths. Their dependencies come from a lock committed in `parity/e2e/<name>/`, written once by the pinned composer for that release (root version the release, `prefer-stable`) and installed without dev packages. Composer reads a copy of the project's `composer.json` with `config.lock` on, since monolog turns the lock off; PHPStan reads the project's own. Piwigo is not a suite: it is not public, and its config needs extensions (Phase 3b); it stays the milestone-end regression check.
- **Oracle:** the phar as a user runs it, in the project's directory, so it finds the project's `composer.json` and autoloader: `analyse --debug --no-progress --error-format=json -c level-<n>.neon`, at every level, the config holding only the level, the paths, the excluded paths and a `tmpDir` (`parity/extract/e2e.php`). With no `phpVersion`, PHPStan takes the PHP versions from `composer.json`. `--debug` prints each analysed file's path, which names the cases, and changes no finding: the totals equal a plain run's.
- **Cases:** one per project and level (`console@7`: the exit code and the errors of no file) and one per analysed file and level (`console/Application.php@7`: its findings, sorted, a trait's with the class it was analysed in). 3,311 cases; a cold extraction is about a minute here.
- **phpstan-rs side:** `debug e2e` reads the same project lines until M7 ports the `analyse` command, which then runs on the same configs.
- **Comparison:** exact, with paths relative to the project (OM-2). A failing file case counts per identifier the findings only one side reported.

## Output

`parity/score.json` is committed. It holds per-suite totals, counts per group (the input tree, the assert kind or the test's namespace), and a digest of the passing case ids. It stays small enough to review in a diff.

The per-case results go to `~/.cache/phpstan-rs/parity/results/<commit>.jsonl`. A run of some suites, or one with `--filter`, writes `<commit>.partial.jsonl` instead. `score.json` is rewritten only for the suites a run covers, and not at all under `--filter`. `--diff <commit>` compares this run's results with `<commit>`'s, case by case, over the suites and `--filter` of this run (SB-6). It reads that commit's clean full run if there is one, then a run from a dirty tree at it, then a partial run; a suite the earlier run did not cover is named and skipped. Each case that differs is:
- **fixed** (passes now), **broken** (passed before), **changed** (failed both times, with another outcome or detail), **added** or **removed**;
- for a types case whose answer changed (not `assertVariableCertainty`), also classified by the new type's relation to the old one, as PHPStan's own types judge it (`parity/extract/relation.php`: both strings through the `TypeStringResolver` of the container PHPStan's tests use, then `isSuperTypeOf` both ways): **equal**, **narrower**, **wider**, **incomparable**, or **unparsed** for a string that is not a type. A passing case answered what it expected.

It prints a count per suite and class and the first ten cases of each class, and writes every case to `results/diff-<earlier>-<this>.txt`. It changes no result and no exit status: CI's gate stays `score.json`.

The one-line summary is `lexer 0/N parser 0/N phpdoc 0/N types 0/N rules 0/N levels 0/N e2e 0/N`.

## CI

- **Every push runs the whole scoreboard** in the `parity` job, inside the pinned `php:8.5.4-cli` image. phpstan-src's dependencies are installed by the pinned composer, and the extraction cache is kept between runs by `actions/cache`.
  - The image has far fewer extensions than the local php. All four extractions are byte-identical under both, checked on 2026-11-01.
  - A full run is cheap while the extractions are cached: each reruns only when its own key changes. The `actions/cache` key names the whole set of extractors and pins, and any older entry is restored (`restore-keys`), so a change to one extractor reruns only the extractions that read it.
- **The committed `score.json` must equal the run** (`git diff --exit-code`). Each suite's entry holds a SHA-256 of its passing case ids, so a change in which cases pass fails CI even when the totals stay equal. A commit that changes the score commits the new `score.json` with it.

## Spot checks (Phase 2 exit, 2026-11-02)

One case per suite, its expected side checked against the real tool, apart from the extractor:
- **lexer:** `tests/PHPStan/Rules/Classes/data/instantiation.php` dumped by php's own `PhpToken::tokenize`: the same 1,115 tokens, kinds, offsets, lengths and lines; nikic's end-of-file sentinel adds the 1,116th.
- **parser:** the phar on `Analyser/data/bug-10358.php` (a `NameResolver` error) and `Analyser/data/multipleParseErrors.php` (two syntax errors) reports exactly the extracted errors as `phpstan.parse`, messages and lines.
- **phpdoc:** the parser exceptions in the 19 invalid tags of `PhpDoc/data/invalid-phpdoc.php`'s doc trees are word for word the phar's 18 `phpDoc.parseError` messages; the 19th is a `@psalm-param` tag, which `InvalidPhpDocTagValueRule` skips (`:77`).
- **types:** `nsrt/array-is-list-type-specifying.php` with each `assertType('X', $e)` rewritten to `dumpType($e)`, run by the phar with bleedingEdge as `NodeScopeResolverTest` runs it: all 13 dumped types equal the extracted expectations.
- **rules:** the phar at level 0 with bleedingEdge on the data files of `CallToNonExistentFunctionRuleTest::testCallToRemovedFunctionsOnPhp8` and `::testBug3576`: its `function.notFound` errors are the extracted 9 and 6, messages and lines.
- **levels, e2e:** the phar's own output; the records equal one phar process per run (S4) and a plain run's totals (end-to-end).

## Spikes

- **S1: the recording `RuleTestCase`.**
  - Question: does PHPUnit run every Rules test class with the stand-in class, and does any test reach into PHPStan's container outside `getRule()`?
  - Method: run the whole Rules directory with the stand-in and count recorded cases, errors and skips.
  - Accept: at least 3,284 `analyse` calls recorded, and every error listed by name.
  - Fallback: static extraction for the literal 94%, with the rest listed as exclusions until each is handled.
  - **Result (2026-11-01): accepted.**
    - PHPUnit ran all 3,617 Rules tests with the stand-in: no errors, 39 skipped, 3,548 cases recorded (3,544 `analyse`, 4 `fix`), 8,172 expected errors, 46 distinct toggles.
    - Two runs gave byte-identical records once sorted.
    - The 39 skips are `RequiresPhp` conditions that exclude those tests under php 8.5, so they are outside the oracle by definition.
    - 40 tests recorded nothing:
      - `ApiRuleHelperTest`, `DirectRegistryTest`, `DefaultExceptionTypeResolverTest` and `RuleErrorBuilderTest` are unit tests of PHPStan helpers, not rule tests;
      - `InvalidThrowsPhpDocValueRuleTest::testMergeInheritedPhpDocs` and `AbstractMethodInNonAbstractClassRuleTest::testBug3406ReflectionCheck` assert on reflection directly.

      They are outside the rules suite; reflection cases like the last two belong to M4's tests.
- **S2: the nsrt file list.**
  - Question: does calling `findTestFiles()` through reflection give the data provider's exact list without analysing anything?
  - Method: compare its result with a PHPUnit `--list-tests` run.
  - Accept: the lists are identical.
  - Fallback: copy the conditions into the extractor, with a test that diffs it against `--list-tests`.
  - **Result (2026-11-01): replaced.** No reflection call is needed. The stand-in `TypeInferenceTestCase::gatherAssertTypes()` records the file each data provider passes it, so PHPUnit's own run gives the list: 1,986 records from 49 classes.
    - 45 PHPUnit errors are expected. They are tests whose data provider is built from `gatherAssertTypes()`, which the stand-in leaves empty.
    - `LegacyNodeScopeResolverTest`, `Generics\AnalysisContextTest` and `ScopePhpVersionTest` assert through their own `processFile` callbacks, not through assert calls. Their few cases each inspect the scope in a callback, so they need a case-by-case extractor; until then they are a "Later" line in `STATUS.md`. They are not exclusions: none of the exclusion kinds applies.
- **S3: the cost of a phar run, for the level oracle.**
  - Question: how long does one phar run on one rule data file take, and what can be cut without changing its output?
  - Method: 30 data files, one in every 104 in byte order, each its own project, at levels 0 and 10, run one at a time under `nice -n 19`.
  - Accept: a cold level-oracle extraction that fits a CI job (6 hours).
  - Fallback: shard the extraction over a CI matrix.
  - **Result (2026-11-02): accepted, at 1.2 s a run.**
    - The plain run took 5.2 s. Of that, 2 s is loading the phar (`--version` alone), and a plain `analyse` also starts parallel workers, each of which loads the phar again.
    - A warm `tmpDir`, per project or shared, saved nothing (4.6–6.3 s).
    - `--debug` analyses in the one process: 2.8 s. With php's opcache file cache as well (`opcache.enable_cli`, `opcache.file_cache`, `file_cache_only`), 1.2 s from the second run on.
    - The findings were identical every way; `--debug` only prints each file's path first.
    - So a cold extraction is about 34,000 × 1.2 s ≈ 11.4 CPU-hours: about 3 hours on a 4-vCPU runner, 1.5–2 hours here.
- **S4: running the phar without a process per run.**
  - Question: can the level oracle's runs share processes and compiled state without changing a single record?
  - Method: a worker that loads the phar once and runs PHPStan's console application per run, first with a project per run, then with a fixed project per worker and the file on the command line. Each compared byte for byte with one phar process per run on 121 runs (11 files × 11 levels), then two full extractions compared with each other.
  - Accept: identical records, and a cold extraction that fits a CI job.
  - Fallback: one process per run with the opcache file cache (S3), sharded over a CI matrix.
  - **Result (2026-11-02): accepted.**
    - With a project per run, one at a time: 0.33 s a run, against 0.85 s for a process per run. The rest was mostly the DI container, compiled anew for each project: an empty file cost 0.71 s of a real file's 0.82 under the same load. Memory grew about 2 MB a run, with each run's container class.
    - With a fixed project per worker and the file on the command line: 0.10 s a run, 11 containers per worker. Identical records.
    - Two things made records differ until fixed: stdout and stderr must stay separate, as the console's `ConsoleOutput` keeps them (PHPStan prints notes to stderr), and php's `short_open_tag` has to be set, as for the extractors.
    - Full extraction, 8 workers here: 14 min 38 s wall, 6,644 CPU-s. Two full runs gave identical records, except for 484 records of a first run that shared its work directory with a killed earlier run; the second, in a fresh directory, had no worker deaths and no failures but PHPStan's own crash on one file.
- **S5: what else a run's cost can lose.**
  - Question: with S4's workers, which of a run's costs can go without changing a record?
  - Method: one worker on one batch of 506 runs (46 files spread over the inputs, × 11 levels), profiled with Excimer, then variants timed against each other in alternating rounds, every variant's records compared with the others'. Garbage collection timed by `gc_status()`'s own counters, since wall time here varies by up to 50% between identical runs.
  - Accept: a gain beyond the rounds' spread, with identical records.
  - Fallback: S4's workers as they are.
  - **Result (2026-11-02):**
    - A run is about 60 ms. About a quarter is building the container from the config, about 28% parsing stubs again (each run's container starts with empty memos), about 18% the collector freeing the run's 12,000 objects. Sharing a container between files would leak one file's classes into the next, so these stay.
    - Collecting every 10, 50 or 100 runs instead of after each: 2–5% less collector time, for up to 3× the memory. Not taken.
    - The turbo extension: about a fifth off a run's CPU time (median 0.80 of plain over five rounds); php's tracing JIT: about the same (0.83). php refuses the JIT beside any extension that overrides `zend_execute_ex`, the turbo extension included, so it is one or the other; the turbo extension is taken, as PHPStan's own package runs it. Both gave records identical to plain.
    - Profiler extensions (SPX, xhprof, pcov) override `zend_execute_ex` too and made plain php about 40% slower here; the CLI here no longer loads them, as the CI image never does.
    - Full extraction with the turbo extension, 8 workers here: 6 min 48 s wall, 3,095 CPU-s, against S4's 14 min 38 s and 6,644. Its records equal S4's second run's, except the crashing file's 11 records, whose message now names PHPStan's own exception.
