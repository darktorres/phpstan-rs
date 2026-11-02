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
- the extractor script;
- the oracle php's `php -v`;
- the inputs' content (the submodule commit, and the digests in `parity/pins.toml`).

A cache hit skips stage 1. The cache is never committed.

## Oracle pins

`parity/pins.toml` holds every input, each a released version fetched by digest (PL-3):
- the phar: 2.3.4, sha256 pinned;
- `vendor/phpstan-src`: the 2.3.4 commit;
- phpstan-src's composer dependencies, through its `composer.lock`. nikic/php-parser v5.9.0, phpdoc-parser 2.4.0 and PHPUnit 11.5.57 come from there;
- nikic/php-parser's source at v5.9.0, for its test fixtures, which composer's dist leaves out;
- php-src at 8.5.4, for the `.phpt` corpus;
- **the oracle php: 8.5.4.** Locally it is the installed php, and the extractor refuses any other version. CI uses the `php:8.5.4-cli` image, pinned by digest.

**Which nsrt files PHPStan runs depends on `PHP_VERSION_ID`** (`// lint` headers, and the conditions in `NodeScopeResolverTest::findTestFiles`). The scoreboard therefore measures exactly "PHPStan's suite as it runs under PHP 8.5.4".

## Suites

### lexer
- **Oracle:** nikic/php-parser's `Lexer::tokenize`, with a collecting error handler.
  - PHPStan lexes with nikic's lexers (`conf/parsers.neon`, `LexerFactory`): `Lexer\Emulative` for the newest version, and `Lexer` (or `Emulative` at the configured version) for analysed files.
  - nikic v5.9.0's newest supported version is 8.5, the oracle php's version, so no emulator is active and both lexers give `Lexer::tokenize`'s output. That is `PhpToken::tokenize` plus nikic's post-processing: the two `&` token kinds, the sentinel token and the bad-character and unterminated-comment errors.
- **Inputs:** the parser suite's inputs 1 and 5 (`tests/` and `vendor/` of phpstan-src), listed by xtask in byte order of their `/`-spelled paths. Inputs 2–4 join when they are pinned.
- **Line breaks:** each input runs as it is, and with every `\r\n`, `\r` and `\n` rewritten to `\r\n` and to a lone `\r` (CP-6). Case ids are the path, then `#crlf` or `#cr` for the rewrites.
- **Comparison:** each case's token stream, written one token per line as `kind pos length line` (kind is the `T_*` name, the character itself, or `EOF` for the sentinel), is compared by its SHA-256 and token count, and its errors (message, byte offset) by equality. Digests keep the cases file small; `--show <id>` reruns both sides for one case and prints the first differing token.
- **Unported:** until M1, `phpstan-rs debug lexer` answers that the lexer is not ported, and every case is `fail` with that reason.

### parser
- **Oracle:** nikic/php-parser v5.9.0's `--json-dump`, with the `startFilePos`/`endFilePos` attributes kept and the token positions dropped.
- **Inputs:**
  1. every `.php` file under `vendor/phpstan-src/tests` (data files included);
  2. nikic's `test/code/parser` fixtures, including the error-recovery ones;
  3. php-src 8.5.4's `.phpt` `--FILE--` sections (F11). `--INI--` lines become `-d` flags; the rules for `--FILE_EXTERNAL--` and `--REDIRECTTEST--` are per F11;
  4. a generated operator-pair file (D25-2);
  5. `vendor/phpstan-src/vendor`, as the large vendor tree.
- **Comparison:** normalized AST JSON equality.
- **Files that fail to parse:** the same first error message and line as nikic. `php -l` is also recorded, informationally, using F11's result classes.
- **Exit bar:** zero false refusals over input 5.

### phpdoc
- **Oracle:** phpdoc-parser 2.4.0, run from PHP over every doc comment of the parser suite's inputs, plus phpdoc-parser's own test data.
- **Comparison:** the printed node tree (`PhpDocNode::__toString` and a structural dump).

### types (nsrt)
- **Extraction:**
  - The file list is what `NodeScopeResolverTest`'s data provider yields under the oracle php. That is computed by calling `findTestFiles()` and the lint-skip check, with no analysis.
  - The file list comes from running the 52 `TypeInferenceTestCase` classes under `parity/extract/bootstrap.php`, whose stand-in `gatherAssertTypes()` records each file and analyses nothing.
  - `parity/extract/asserts.php` then reads each `assertType`, `assertNativeType`, `assertSuperType` and `assertVariableCertainty` call with nikic: the expected string or certainty, the line and column, the asserted expression's source text, and its kind (EV-3).
  - PHPStan reads a non-literal expected argument through the scope as a constant string. The extractor folds the forms the suite uses: `X::class` (with `self`, `static` and `parent` taken from the enclosing class), constants declared in the file, concatenation, `sprintf` over constants, an assignment's value, and a variable assigned once in the enclosing function. Every one of the 21,299 asserts resolves.
- **Case id:** `file:line:column:kind`. Nine lines hold two asserts, so the line alone is not unique.
- **phpstan-rs side:** `debug nsrt <file>` analyses the file as its own project (CA-4). It prints `describe(precise)` of the asserted expression, or the certainty, at each assert.
- **Comparison:** string equality. `describe()` aims at PHPStan's exact strings (CA-5), so no normalization is planned. If one is ever needed, it is listed in this doc with its reason.

### rules
Extraction **runs PHPStan's own test methods without running PHPStan's analysis.**
- `parity/extract/bootstrap.php`, a PHPUnit bootstrap, puts recording `PHPStan\Testing\RuleTestCase` and `TypeInferenceTestCase` classes on the autoloader before phpstan-src's own. They are generated from the real classes by rewriting the methods that would analyse.
- It then runs the `tests/PHPStan/Rules/**/*RuleTest.php` classes through PHPUnit. Data providers run, and `PHP_VERSION_ID` branches resolve under the oracle php.
- The recording class records, for each `analyse($files, $expectedErrors)`:
  - the test class and method (and the data-set name);
  - the files;
  - the expected `[message, line, tip?]` list;
  - the test's own property values at that moment (its toggles: `checkNullables`, `reportMaybes` and the like);
  - `getAdditionalConfigFiles()`.
- `getRule()` is never called, so no PHPStan service is built. `fix()` is recorded the same way, with its expected file.

Why execute instead of parsing: 196 of 3,284 `analyse()` calls pass expectations that are built in code, and 154 methods branch on `PHP_VERSION_ID`. Static parsing would get these wrong.

- **phpstan-rs side:** `debug rule <case>` analyses the case's files with only the mapped rule enabled, configured by the recorded toggles.
  - The map from PHPStan test class to our rule configuration lives in the `testing` crate, beside each rule's port.
  - A case whose test class has no mapping yet is `fail` (reason: "rule not ported"), not `error`.
- **Comparison:** the multiset of `(line, message, tip)`, as `RuleTestCase::analyse` compares them.

### level oracle
- **Oracle:** the phar, run on `tests/PHPStan/Rules/**/data` and, later, on corpus files, at every level 0–10. It uses FR-5's recipe:
  - run outside any `vendor/`, with `phpVersion` as a range;
  - each file is its own project (CA-4, OWL-2);
  - bleeding-edge toggles as each test sets them;
  - findings matched by identifier: same file and line, then within 3 lines, then within 60 lines for the listed identifiers;
  - each finding used once;
  - "in context of class" copies collapsed.
- **Comparison:** per identifier, the counts of PHPStan-only, matched and ours-only findings.
- **When it runs:** the phar runs are expensive, so this suite runs at milestone ends and not in per-push CI.

### end-to-end
- **Oracle:** the phar, against Piwigo (local only), monolog and symfony/console, each pinned by commit in `pins.toml`. It runs at every level, on the same `phpstan.neon`.
- **Comparison:** the JSON output, after making paths project-relative (OM-2).
- **When it runs:** from M7.

## Output

`parity/score.json` is committed. It holds per-suite totals, plus counts per group (the test directory or milestone owner). It stays small enough to review in a diff.

The per-case results go to `~/.cache/phpstan-rs/parity/results/<commit>.jsonl`. A run of some suites, or one with `--filter`, writes `<commit>.partial.jsonl` instead. `score.json` is rewritten only for the suites a run covers, and not at all under `--filter`. `--diff <commit>` compares two of these case by case and classifies each change (SB-6):
- for most suites: pass↔fail;
- for types: equal, narrower, wider or incomparable.

The one-line summary is `lexer 0/N parser 0/N phpdoc 0/N types 0/N rules 0/N`.

## CI

- **Every push:** the lexer, parser, phpdoc and types suites on a fixed `--filter` subset, in the release build, failing on any regression against the committed `score.json`.
- **Milestone ends:** the full scoreboard runs locally and its `score.json` is committed. CI checks that the committed file matches a full run, on a schedule.

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
  - **Result (2026-11-01): replaced.** No reflection call is needed. The stand-in `TypeInferenceTestCase::gatherAssertTypes()` records the file each data provider passes it, so PHPUnit's own run gives the list: 1,986 files from 49 classes.
    - 45 PHPUnit errors are expected. They are tests whose data provider is built from `gatherAssertTypes()`, which the stand-in leaves empty.
    - `LegacyNodeScopeResolverTest`, `Generics\AnalysisContextTest` and `ScopePhpVersionTest` assert through their own `processFile` callbacks, not through assert calls. Their few cases each inspect the scope in a callback, so they need a case-by-case extractor; until then they are a "Later" line in `STATUS.md`. They are not exclusions: none of the exclusion kinds applies.
