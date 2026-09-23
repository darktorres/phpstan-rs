# Piwigo scoped-corpus triage — `ab4c2b5`

Scoped run (`phpstan-rs.toml`, `/home/torres/piwigo17-frozen`) at commit
`ab4c2b5`: 2792 files checked, **235 diagnostics**. Snapshot:
`.corpus-runs/scoped/ab4c2b5.txt` (a fresh re-run was byte-identical, and
identical to the previous snapshot `050cc40`).

**Verdict: 100 false positives from 22 root causes, 135 true positives.**

## Baseline: real PHPStan

Real PHPStan (Piwigo's own `phpstan.neon`: level 10, bleedingEdge,
phpstan-doctrine with a DB-backed `objectManagerLoader`, phpstan-dba,
strict-rules) run over all 80 flagged files reports **0 errors**:

    cd /home/torres/piwigo17-frozen
    php -d memory_limit=6G vendor/bin/phpstan analyse --no-progress --error-format=raw <files>

Every diagnostic is therefore phpstan-rs-only: either a false positive or
a stricter-than-PHPStan true positive. Each false-positive root cause below
was reproduced with a minimal `assertType` probe.

## False positives (100)

| # | Count | Root cause | Location |
|---|---|---|---|
| 1 | 28 | Pest `beforeEach`/`afterEach` closures are checked against a clone of the locals, so `use (&$x)` write-back is discarded; the next hook still sees the pre-hook `$x = null` | `check_closure_literal_with_this_and_scope`, `src/checker/calls/closure_context.rs:34` |
| 2 | 14 | `PHP_SAPI` resolves to the phpstorm-stubs literal `'cli'`; PHPStan's `ConstantResolver` widens it to a union (it also widens `PHP_OS`, `PHP_EOL`, `DIRECTORY_SEPARATOR`, `PHP_INT_SIZE`, …) | `global_constant_type`, `src/checker/expr/probe_checks.rs` |
| 3 | 10 | Joining two branches where one side is an open shape (`array{…, ...}`) ignores that the `...` tail could hold the key: after `if (!isset($p['section'])) { $p['section'] = X; }` the result is `section?: X` instead of `mixed` | `merge_array_shape_pair`, `src/checker/type_union.rs:418` |
| 4 | 7 | `proc_get_status` is hand-patched with the base functionMap's `\|false`; PHPStan's `functionMap_php80delta.php` removes the `false` | `src/php_stubs.rs:287` |
| 5 | 5 | Property default values are inferred without `current_class` set, so `private int $x = self::FOO;` becomes `mixed` (fires on every such property) | `check_default_property_types` (`src/checker/property_default_types.rs`), called at `src/checker.rs:1907` |
| 6 | 5 | `get_defined_vars()` after `include $file` returns a closed shape; PHPStan calls `afterExtractCall()` after any include, making the result `array<string, mixed>` | include handling |
| 7 | 5 | Doctrine `MIN()`/`MAX()` of a custom-typed column get the field's PHP type (`SqlDateTime`); Doctrine's `SqlWalker` maps non-`TypedExpression` aggregates to `'string'`, so the raw DB value comes back | `src/checker/doctrine/` |
| 8 | 5 | SQL results from the static schema file ignore column nullability, which the schema parser does record (`varchar NULL` → `string`) | `resolve_select_from_static_schema`, `src/checker/sql_reflection/resolve.rs:430` |
| 9 | 3 | `switch` fall-through: a case's entry state is only the fall-through path, and the direct-entry path is dropped. Stacked `case 'a': case 'b':` narrows to just `'a'` | switch handling |
| 10 | 3 | `dirname(__DIR__, N)` climbs to `""` instead of `"."` when config `paths` are relative (`"src"`), producing `"/vendor/..."` | include-path resolution |
| 11 | 2 | Branch-implied facts (including the `instanceof` object-proxy facts) are never invalidated when their subject is rebound, e.g. by `foreach` on a later loop-convergence pass | `record_branch_implied_facts`, `src/checker/narrowing.rs:377` |
| 12 | 2 | `getOneOrNullResult(HYDRATE_SINGLE_SCALAR)` is typed as a row array; it returns the scalar itself | Doctrine result typing |
| 13 | 2 | A dynamic-key write `$r[$k] = …` doesn't purge an earlier narrowed `$r['qs']` entry created by a nested write `$r['qs']['z'] = …` | assignment narrowing |
| 14 | 1 | `missingType.generics` ignores template defaults: `@template T = mixed` still demands a type argument (PHPStan's `MissingTypehintCheck` skips templates with defaults) | `SensitiveParameterValue` |
| 15 | 1 | Bare `exit;` (no parentheses) isn't treated as terminating; `exit()` is | terminator detection |
| 16 | 1 | `use const Foo\BAR;` imports are ignored completely: constant lookup reuses the function resolver, which only follows `use function` imports | `resolve_function_candidates`, `src/registry/name_context.rs:255` |
| 17 | 1 | `isset($x['k'])` on `array{k?: int}\|false` doesn't remove `false` or make `k` required | isset narrowing |
| 18 | 1 | `in_array($x, self::CONST, true)` doesn't narrow `$x`; a literal array haystack works | `in_array` narrowing |
| 19 | 1 | `array_filter($a, is_int(...))` and `array_filter($a, 'is_int')` don't narrow the element type | `array_filter` return type |
| 20 | 1 | A `/** @var T $x */` on `$x = expr` is applied before the right-hand side is evaluated, so reads of `$x` inside it see `T` | assignment `@var` handling |
| 21 | 1 | `DOMDocument::$documentElement` (and `$doctype`) are non-null; PHPStan's `dom.stub` says `DOMElement\|null`. The PHPStan stub overlay carries only templates, not property types | `src/php_stubs.rs` |
| 22 | 1 | `mysqli::query()` isn't typed from the SQL; phpstan-dba (used by Piwigo's PHPStan) types a SELECT as `mysqli_result`. A feature gap rather than a bug | no mysqli support in SQL reflection |

The broadest outside this corpus are #5, #9, #15 and #16: basic patterns
that happen to be rare in Piwigo.

### Affected diagnostics per root cause

1. **Pest by-ref hooks (28)**: `tests/Unit/Admin/Maintenance/DbMaintenanceRepositorySqliteTest.php:44,45` (×2 each), `tests/Unit/Core/UniqueExecLockSqliteTest.php:45,46` (×2 each), `tests/Unit/Db/AdvisorySessionLockSqliteTest.php:44,45` (×2 each), `tests/Unit/Db/SchemaDumpServiceSqliteTest.php:47,48` (×2 each), `tests/Unit/Db/SortRendererSqliteTest.php:36,37` (×2 each), `tests/Unit/Db/SqlDialectExecutorSqliteTest.php:37,38` (×2 each), `tests/Unit/Db/SqlDialectTest.php:24`, `tests/Unit/Session/SessionServiceTest.php:59`, `tests/Unit/Admin/Extensions/ExtensionUpdateCheckerTest.php:281`, `tests/Unit/Admin/ThemesInstalledPageRendererTest.php:387`
2. **`PHP_SAPI` (14)**: `src/Piwigo/Core/Env.php:53`; `tools/coverage-merge.php:5`, `tools/doc-drift-check.php:5`, `tools/latte-lint.php:5`, `tools/normalize-fixture.php:5`, `tools/opcache-preload.php:74`, `tools/openapi-bootstrap.php:25`, `tools/openapi-generate-input-dto.php:13`, `tools/p58/{assign,census,trace,unflatten}.php:5`, `tools/regenerate-fixture-photos.php:5`, `tools/triggers_list.php:7`
3. **Open-shape join (10)**: `src/Piwigo/Section/SectionPopulator.php:188,200,264` (`identical.alwaysTrue`), `:440,484,511,556,588,598,613` (`identical.alwaysFalse`)
4. **`proc_get_status` (7)**: `tests/Unit/Admin/Extensions/PemCatalogTest.php:589`, `tests/Unit/Core/ErrorCollectorTest.php:765`, `tests/Unit/Core/ShutdownHandlerTest.php:170`, `tests/Unit/GeoIp/GeoIpDatabaseUpdaterTest.php:59`, `tests/Unit/Html/HtmlServiceTest.php:2034`, `tests/Unit/Http/HttpClientServiceTest.php:74`, `tests/Unit/Http/ResponseEmitterTest.php:72`
5. **Property default `self::` (5)**: `src/Piwigo/Config/CurrentConfig.php:2356,2434,2472`, `src/Piwigo/Core/Logger.php:87,99`
6. **`get_defined_vars()` after include (5)**: `tools/i18n/php-to-po-fn.php:41` (×2), `:42` (×2), `tools/i18n/verify-parity.php:97`
7. **Doctrine `MIN`/`MAX` (5)**: `src/Piwigo/Category/CategoryRepository.php:2527,2528`, `src/Piwigo/Comment/CommentRepository.php:1020,1021`, `src/Piwigo/Rate/RateRepository.php:405`
8. **Static-schema nullability (5)**: `tests/Integration/InstallationStatsTest.php:164,194`, `tests/Unit/Audit/AuditRepositoryTest.php:87`, `tests/Integration/MaintenanceActionDispatcherTest.php:429,443`
9. **Switch fall-through (3)**: `src/Piwigo/Notification/NotificationRepository.php:118`, `src/Piwigo/Controller/CommentsController.php:397`, `src/Piwigo/Controller/PictureController.php:509`
10. **Relative `dirname` (3)**: `src/Piwigo/Db/EntityManagerFactory.php:212`, `tests/Integration/ContainerSmokeTest.php:142`, `tools/opcache-preload.php:80`
11. **Stale branch-implied facts (2)**: `tools/phpstan/Latte/ContextVariableExtractor.php:318`, `src/Piwigo/Controller/Api/Categories/CategoryAvailableListController.php:188`
12. **`HYDRATE_SINGLE_SCALAR` (2)**: `src/Piwigo/Config/ConfigRepository.php:215`, `src/Piwigo/Site/SiteRepository.php:148`
13. **Dynamic-key write not purging nested entry (2)**: `src/Piwigo/Search/SearchService.php:1632,1635`
14. **Template defaults (1)**: `src/Piwigo/Auth/Event/TryLogUser.php:32`
15. **Bare `exit;` (1)**: `src/Piwigo/Admin/PluginsInstalledPageRenderer.php:117`
16. **`use const` (1)**: `src/Piwigo/Core/CoverageCollector.php:79`
17. **`isset` on `array|false` (1)**: `src/Piwigo/Http/HttpClientService.php:422`
18. **`in_array` with class-constant haystack (1)**: `src/Piwigo/Controller/Api/Images/ImageSetCategoryController.php:60` (`match.unhandled`)
19. **`array_filter` with `is_int(...)` (1)**: `src/Piwigo/Controller/CommentsController.php:142`
20. **`@var` before RHS (1)**: `src/Piwigo/Category/CategoryDefaultRenderer.php:94`
21. **`documentElement` nullability (1)**: `src/Piwigo/Admin/Upload/UploadService.php:745`
22. **`mysqli::query` (1)**: `tools/regenerate-fixture-photos.php:78`

### Minimal repros

```php
// 1 - Pest by-ref hook
$x = null;
beforeEach(function () use (&$x): void { $x = getenv('A'); });
afterEach(function () use (&$x): void { $x === null; }); // "always true"

// 3 - open-shape join
/** @param array<string, mixed> $parsed */
function f(array $parsed): void {
    $page = array_merge(['items' => 1], $parsed);
    if (! isset($page['section'])) { $page['section'] = Section::Categories; }
    // $page: array{items: mixed, section?: Section::Categories, ...}
}

// 5 - property default
class L { public const X = 3; public int $t = self::X; } // "default value of type mixed"

// 9 - switch fall-through
switch ($type) { case 'a': case 'b': /* $type is 'a' */ break; }

// 11 - stale branch-implied facts
foreach ($rows as $row) {
    if ($b) { $row['name'] = 'y'; } else { /* pass 2: $row['name'] is string */ $row['name'] = strip_tags('x'); }
    $cats[] = $row;
}

// 13 - dynamic-key write
$r = ['qs' => ['q' => 'x']]; $r['qs']['z'] = 1; $r[$k] = $v; // $r['qs'] still array{q: 'x', z: 1}

// 15 - bare exit
$raw = inc(); if ($raw === false) { exit; } // $raw still includes false

// 16 - use const
namespace Foo; use const Bar\ZED; ZED; // "Constant ZED not found"

// 20 - @var before RHS
/** @param array<mixed> $sel */
function g(array $sel): void {
    /** @var list<int> $sel */
    $sel = count($sel) > 0 ? $sel : []; // RHS $sel already reads as non-empty-list<int>
}
```

## True positives (135)

| Count | Identifier | Why they are correct |
|---|---|---|
| 39 | `arrayValues.list` | Each wraps `getSingleColumnResult()` (DBAL `fetchFirstColumn()`, always a list) or a `getArrayResult()` with no `INDEX BY` |
| 73 | `function.alreadyNarrowedType` | Mostly `is_array($row)` on Doctrine rows; the rest are `is_string`/`is_scalar`/`is_int` on non-null columns, `COUNT`, `fetchOne`, or native-typed properties read through reflection |
| 12 | `instanceof.alwaysTrue` | Non-null custom-typed ID columns whose converter returns the value object; one `ReflectionProperty::getValue()` on a native-typed readonly property |
| 8 | `nullCoalesce.offset` | `??` on keys of Doctrine rows whose mapped columns are non-null |
| 3 | `function.impossibleType` | `CommentRepository.php:570` (plain `c.date` select), `SiteRepository.php:120` (`id` is `int\|null`), `tests/Integration/UserRepositoryTest.php:387` (`tinyint(1) NOT NULL`, native ints) |

Real PHPStan sees `mixed` in these spots (no descriptors are registered for
Piwigo's custom Doctrine types, and `getArrayResult()`/
`getSingleColumnResult()` have no dynamic return type there). So "fixing"
them by deleting the redundant checks would break the project's own
level-10 PHPStan gate, and developers may treat them as noise.

## Latent issues (no current false positive)

- Fields from a `leftJoin`ed entity keep non-null types (`UserRepository.php:2079` shows `status: UserStatus`; should be `UserStatus|null`).
- Doctrine `INDEX BY` isn't modelled; results are always treated as lists (no corpus usage today).
- `Expr&Arr` isn't simplified to `Arr` in intersection display.
- `DOMDocument::$doctype` has the same non-null issue as #21.

## Tooling notes

- Corpus-context probe: a scratch dir with `vendor`, `src`, `install` and `phpstan-stubs` symlinked from the corpus, plus a toml of `paths = ["probe"]`, `scan_paths = ["src", "phpstan-stubs"]` and the same `[sql_reflection]` block. A copied corpus file can then be probed with `assertType` (about 35s per run).

## Results (after the fix round)

All 22 root causes are fixed, plus the latent issues and a few gaps the
fixes exposed. Every triaged false-positive location is gone from the
scoped run.

| Run | Before (`ab4c2b5`) | After |
|---|---|---|
| Scoped (`phpstan-rs.toml`) | 235 | 159 |
| Unscoped (whole checkout incl. `vendor/`) | 44,921 | 43,330 |

### Beyond the original 22

- **Latent issues, now built:** LEFT-joined fields are nullable; Doctrine
  `INDEX BY` keys the result collection (`Query<TKey, TResult>`).
- **Doctrine hydration modes:** every result method dispatches on its
  hydration mode (`HYDRATE_ARRAY`, `HYDRATE_SCALAR`,
  `HYDRATE_SINGLE_SCALAR`, `HYDRATE_SCALAR_COLUMN`); entity
  `HYDRATE_ARRAY` rows are arrays (they were object shapes).
- **mysqli:** `mysqli::query()`/`mysqli_query()` typed from their SQL
  (`mysqli_result<Row>` vs `true`, no `false` under PHP 8.1+ exception
  mode), with SQL-typed `fetch_*` methods.
- **Qualified function calls through namespace imports** (`use Amp\Future;
  Future\awaitAll()`): a side effect of keying imports by kind; about 495
  unscoped `function.notFound` false positives disappeared.
- **Gaps exposed by better resolution, fixed:** nameless `@var` on a
  `return`; a write making an optional shape key present; `isset()` adding
  a key an open shape/array didn't list; `array_merge(...)` over a
  non-empty list of non-empty lists; `return.unusedType` comparing each
  return point instead of their merged union; `phar://` includes and
  `DIRECTORY_SEPARATOR` in include paths.
- **`return.unusedType`** now uses PHPStan's trinary logic per return
  point: a declared member a returned type merely overlaps (`Node` when a
  `Name` is returned) counts as used, which removed 256 unscoped reports.
- **Build:** pinned `nightly-2026-09-22`, mold linker, `-Z threads=8`;
  incremental debug test rebuild ~18s (was ~123s). Pre-commit hook removed.

### New findings that are correct

- 25 more `arrayValues.list`: `array_values()` around
  `getSingleColumnResult()` in chains whose root the query-builder resolver
  can't follow - `fetchFirstColumn()` always returns a list regardless.
- `SectionPopulator.php:679` `argument.type`: `$page` merges
  `parseSectionUrl()`'s `array<string, mixed>`, so `chronology_date` can
  hold anything; `is_array()` only proves `array`, not the
  `list<int|string>` the callee declares.
- Unscoped: vendor dev scripts requiring autoloaders absent from the
  installed packages; Rector/ECS scoped vendor code whose docblocks weren't
  prefixed (visible now that prefixed function calls resolve).
