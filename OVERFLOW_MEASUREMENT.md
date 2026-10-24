# Integer overflow: flood measurement (2026-10-22)

Experiment: `int` op `int` for `+`, `-`, `*` unions `float` whenever the result
has an unbounded end (`int`, `int<1, max>`, `int<min, -1>`...). Binary:
`~/.cache/phpstan-rs-scratch/adv/bin/overflow`. Baseline: `bin/headfix`
(HEAD `0566039d1`). Diff: `normdiff.py` (absolute vendor keys normalized to
package-relative paths; the earlier `diffjson.py` total-mismatch on vendor
packages was a key-format artifact and is not a result).

## Net diagnostics (headfix -> overflow)

| corpus | base | overflow | gained | lost |
|---|---|---|---|---|
| Piwigo | 583 | 719 | 138 | 2 |
| monolog | 246 | 275 | 29 | 0 |
| guzzle | 1354 | 1370 | 16 | 0 |
| league-commonmark | 302 | 338 | 36 | 0 |
| doctrine-collections | 6 | 6 | 0 | 0 |
| doctrine-orm | 2120 | 2135 | 15 | 0 |
| league-flysystem | 1469 | 1470 | 2 | 1 |
| psalm | 4381 | 4635 | 262 | 8 |
| rector-src | 3861 | 3876 | 17 | 2 |
| reactphp-http | 769 | 771 | 2 | 0 |
| laravel | 11437 | 11473 | 114 | 78 (message churn) |
| symfony Console | 6108 | 6114 | 84 | 78 (message churn) |
| wordpress | 16939 | 17159 | 239 | 19 |
| drupal core | 124584 | 124715 | 169 | 38 |

Laravel, Symfony and Drupal show equal lost/gained counts on the same identifier
(`missingType.return`, `missingType.parameter`...): these are changed messages
(an inferred type inside the text), not new findings.

## Gained findings, classified

- Piwigo gained `argument.type` 118 of 138: 114 have a given type with `int|float`
  or `int<a, max>|float`; 4 are other shapes. Smaller: `return.type` 11,
  `assign.propertyType` 5, `parameterByRef.type` 3.
- Every gained `int|float` finding is runtime-possible: PHP promotes overflowing
  `int` arithmetic to `float`. None is reached by a value the corpus can show
  overflowing; they are false positives under "reachable in practice", true under
  "possible at runtime".

## Lost findings (Piwigo, 2)

- `cast.useless` at `src/Piwigo/Admin/PiwigoInfosSender.php:478`: now correct to
  drop, since `(int)` of `int|float` is a real conversion.
- `function.alreadyNarrowedType` (`is_numeric()` with int always true) at the same
  line: a true finding lost. `is_numeric(int|float)` is still always true, so the
  checker's `is_numeric` narrowing should treat `int|float` as numeric. That is a
  gap to fix if the overflow model ships.

## Reading

The flood is the cost of `int|float` on every unbounded int operation. It is about
24% of Piwigo's diagnostics and 1-3% of the vendor packages. Under the project's
"runtime truth" rule the findings are sound; under "report what happens in practice"
they are noise. Recommended default: `int` (no overflow union), with an opt-in setting
for the runtime-exact model; the `is_numeric` gap must be closed before the opt-in ships.

## Corrections (after reading every Piwigo gained finding with its source line)

Source dump: `overflow-piwigo-gained.txt` (138 findings, all read).

- Earlier "every gained int|float is runtime-possible" was an overclaim. Most
  gains are offsets and counters that cannot overflow in practice: `strpos + 1`,
  `strlen - n`, `count - 1`, loop indexes, `time() ± n` passed to `touch`/`date`,
  sums of DB counts. The model cannot see that these are bounded by string or
  array size, so each is a false positive.
- Real, runtime-reachable case: `src/Piwigo/Controller/Api/Images/ImageSearchController.php:80`,
  `array_slice($searchItems, $page * $perPage, $perPage)`. `$page` and `$perPage`
  are `(int)` casts of query-string values with no bound (lines 45, 47 of that file).
  PHP 8.5 confirms `(int)"9223372036854775807" * 2` becomes a float, and with
  `declare(strict_types=1)` `array_slice` throws `TypeError`. So a request such as
  `perPage=9223372036854775807&page=2` yields a 500. The baseline did not report it.
  Other request-driven products in the list (`FavoriteListController:75`,
  `TagImagesController:81`, `CategoryImagesController:118`, `UserListController:108`,
  `ImageGetController:150`, `HistorySearchController:206`, `ImageSetRankController:87`)
  are the same shape and are not yet traced to their inputs.
- Lost findings on Piwigo are not 2 true losses. `PiwigoInfosSender.php:478` is
  still reported: `function.alreadyNarrowedType` now says `int|float` instead of
  `int` (a message change). The only real loss is `cast.useless` at the same line,
  which is correct, since `(int)` of `int|float` converts.
- The vendor percentages above were wrong: league-commonmark gained 36 on 302 (about
  12%), not 1-3%. The vendor gained findings (about 1000 net across packages) have
  not been read at all.

## Trace of the request-driven gained findings (Piwigo)

Real: the product comes from an unclamped `(int)` of a query parameter, and the
float reaches an `int` parameter in a `strict_types=1` file (TypeError, PHP 8.5 checked
for `array_slice` and the product):
- `Api/Images/ImageSearchController.php:80` (`perPage`, `page`, lines 45 and 47)
- `Api/Session/FavoriteListController.php:75` (`perPage`, `page`, lines 43 and 44)
- `Api/Tags/TagImagesController.php:81` (`perPage`, `page`, lines 55 and 56)
- `Api/Categories/CategoryImagesController.php:118` (`perPage`, `page`, lines 52 and 53)
- `Api/Users/UserListController.php:108` (`perPage`, `page`, lines 91 and 92; into `UserRowFetcher::page(int $offset)`)
- `Api/Images/ImageGetController.php:150` (`commentsPerPage`, `commentsPage`, lines 65 and 66)
- `Api/History/HistorySearchController.php:206` (`pageNumber` from the query, `HistorySearchInput::fromArray`; `pageNumber * 300`; PHP checked at 1e17 -> TypeError)

False in practice (the float comes from a value the request does not control):
- `Api/Images/ImageSetRankController.php:87-88`: the float is `$maxRank + 1`, with `$maxRank` from the database; the request `rank` is compared, not multiplied.
- `Group/GroupRepository.php:177`: every caller of `GroupService::getListWithMemberCounts()` uses the defaults `perPage = 9999999`, `page = 0`, so the product is `0`.
- `Admin/ElementSetRanksPageRenderer.php:178` (`current_rank * 10`): a stored rank, not request input; unreachable in practice.

Count: 7 real, 3 false. The 7 are bugs in Piwigo, found only because the opt-in model
treats unbounded ints as possibly overflowing. The general point for the checker is
that the model is right about these operands and wrong about the offsets listed above.

## Vendor gained findings (all new-location gains read)

Source dumps: `overflow-vendor-new-all.txt` (728: psalm, wordpress, drupal, the eight small
packages) and `overflow-laravel` (36, read in the terminal). The "changed-message" gains
(laravel 78, symfony 78, drupal 36, psalm 7, rector 2) are an artifact of the comparison:
the absolute vendor path inside anonymous-class names is not normalized, so the same
finding looks changed. The few real changes at an existing location are the same
overflow pattern (`FileInputHelper.php:209`, `Condition.php:96`).

Heuristic tally of the 728 (keyword match on the source line; approximate):
offset/count/depth 476, counter/size 69, time 38, other 143 (reviewed by reading: parser
positions, `strlen`/`count` differences, setters of offsets).

Request-reachable candidates and their status:
- `wordpress/wp-includes/class-wp-xmlrpc-server.php:283` `addTwoNumbers`: `$number1 + $number2`
  on two `is_int` values. `<int>` is parsed with `(int)` (`IXR/class-IXR-message.php:180`),
  so 64-bit values pass. The method is in WordPress's default XML-RPC table
  (`class-wp-xmlrpc-server.php:162`). Effect: a float where the docblock says `int`; no
  declared return type, so no TypeError. Real, benign.
- `laravel/src/Illuminate/Pagination/AbstractPaginator.php:340` `firstItem()`:
  `($currentPage - 1) * $perPage + 1`. `currentPage` comes from the request's `page`
  (`PaginationState.php:19-27`, `FILTER_VALIDATE_INT`, so up to PHP_INT_MAX). Overflow needs
  a request-controlled `perPage` as well (apps often pass `per_page` from the request; not
  verified for any specific app). No declared return type, so a float in the output.
  Real, benign, conditional.
- `monolog/src/Monolog/Utils.php:279-306` `expandIniShorthandBytes`: `$val *= 1024` three
  times from `(int)` of an ini value. Overflow needs a huge config value. Returns `int|false`
  by docblock; no native return type. Config-only.
- `psalm/src/Psalm/Internal/Fork/PsalmRestarter.php:148`: the same memory-shorthand shape.
  Config-only.
- `guzzle/src/Cookie/SetCookie.php:530` `maxAgeToExpires`: guarded by
  `$maxAge > PHP_INT_MAX - $now` (line 526). False positive that the checker cannot see
  (no relational reasoning).
- `drupal/lib/Drupal/Component/Utility/Rectangle.php:120`: `abs((int)$a) + abs((int)$b)`.
  PHP confirms `abs(PHP_INT_MIN)` is a float (`php -r`). Needs a `PHP_INT_MIN` image
  dimension. Unreachable in practice.

Everything else is false in practice: positions and lengths, counts, `time()`/timestamp
arithmetic, version numbers, test fixtures, and the sodium_compat 32-bit helpers (loop
indices and 0/1 values).

Net picture for the decision: 7 real request-driven 500s in Piwigo, 2 benign wrong-type
outputs in WordPress and Laravel (one conditional), 2 config-only overflows. Against roughly
764 new-location findings, the overflow default would be mostly noise. The model's
useful output is the small set of request-driven cases.

## Adversarial validation of the whole-program design (2026-10-22)

Checked against PHP 8.5, MySQL (local, strict and non-strict), PostgreSQL 18, SQLite:
- Framework-injected `int` parameters are a real large-value source: `"9223372036854775807"`
  passed to `f(int $x)` through `call_user_func_array` is `PHP_INT_MAX`; one more is a
  TypeError. `FILTER_VALIDATE_INT` accepts `PHP_INT_MAX`.
- Schema bounds hold per driver: MySQL strict rejects 2^40 in `INT` (error 1264), non-strict
  clamps to 2147483647; PostgreSQL rejects (`integer out of range`); SQLite stores 2^62 in
  `INT` and `PHP_INT_MAX` in `SMALLINT` (type affinity). Aggregates leave the column bound:
  `SUM` of two `INT` maxima is 4294967294.
- Monolog `NormalizerFormatter::normalize()` with `setMaxNormalizeDepth(PHP_INT_MAX)` and
  depth `PHP_INT_MAX` throws TypeError at line 223; with the default 9 the guard at line 209
  stops it. So a precondition must relate a parameter to an object property.

Design consequences:
1. "`(int)` of any string is Large" and "an int narrowed from mixed is Large" are
   heuristics that contradict the definition. Under the whole-program definition a cast of
   a parameter string is Unknown until the callers say otherwise. Large requires the
   string's provenance to be the environment. So the model needs provenance on strings,
   arrays and mixed values (what a value can contain), not on ints alone.
2. Unresolved call edges (callbacks, string dispatch tables such as WordPress XML-RPC,
   event dispatchers, DI) leave preconditions undischarged. Treating them as Unknown loses
   true positives (WordPress `addTwoNumbers` needs the XML-RPC table modelled); treating them
   as environment brings false positives back. The honest output is a coverage count of
   unresolved edges next to the findings.
3. Forward contexts beat backward preconditions: re-running a callee with its callers'
   argument magnitudes (the existing call-mode body re-run) needs no reverse call graph and
   no registry-time receiver types. Findings in vendor bodies are reported at the first
   project frame.
4. A library checked alone has no environment for its public API, so the Laravel
   `firstItem()` case is a contract fact there, not a finding; it becomes a finding in an
   app that passes a request-derived `perPage`.
5. Schema bounds need the driver set of the deployment (config), and SQLite gives none.

## First prototype (O6, O1, A4, O2, O3 Counter) — 2026-10-22

Binaries `adv/bin/proto-base` (HEAD `30c24ab03`, overflow float accepted at int
sinks) and `adv/bin/proto` (same, int sinks reject it); script `adv/run-proto.sh`,
comparison `adv/compare-proto.py` (`--dump` in `adv/proto-dump.txt`). Loadavg 5-9
during the runs (no timing taken).

Gained findings, prototype vs the earlier blunt experiment:
piwigo 132 (137), orm 16 (15), monolog 22 (29), guzzle 15 (16), commonmark 44 (36),
doctrine-collections 1 (0), flysystem 1 (1), psalm 245 (250), rector 14 (14),
reactphp 1 (2), laravel 29 (36), symfony console 6 (6), wordpress 218 (220),
drupal 119 (130). Total 863 (892).

All eleven true positives are still reported.

Gone (Piwigo 26, read): strlen/count differences now exact (`strlen($a) -
strlen($b)`, `count($x) - 1`, `random_int(0, count($x) - 1)`), loop counters
(`$i + 1` in loops, `$current_rank * 10`), `25 - strlen($riff)`. All were false
positives.

New (read, every package):
- A4 points the earlier experiment didn't model: `$x++`, `-$x`, `abs($x)`.
- `-strcmp(...)`: strcmp returns -1/0/1 since PHP 8.2; the stub says int. A
  version-dependent return range is missing.
- `-($result >> 1)`: right-shift ranges were lost; fixed in `0e8dea09b`.
- Counters outside loops: `static $counter; return $counter++`,
  `self::$messageIndex++`, `++$cache`, a closure's `$calls++`, Piwigo's
  `$next_element_id++` (starts from a DB max, so Unknown start). The Counter rule
  covers loop locals only; statics, private properties and by-ref captures
  updated only by steps are the same argument.
- `abs((int) $x)` in WordPress `absint()` (`load.php:1469`, `pluggable.php:3080`):
  `(int) "-9223372036854775808"` is PHP_INT_MIN and abs of it is a float, against
  the documented `@return int`. A contract violation reachable from request data
  (absint is applied to request values throughout WordPress): a true positive.
- The rest is Unknown-magnitude arithmetic on parameters and properties (psalm's
  `TLiteralInt($x + $sum)`, commonmark's Cursor offsets, sodium_compat).

Conclusion: the bounds and the Counter rule remove the false positives that came
from the checker's own imprecision, but the flood stays (863 of 892), because it is
Unknown magnitude: values from parameters and properties. That is what O4
(provenance), O5 (sources) and O7 (contexts) are for; nothing in O1-O3 can remove it.

## Provenance, sinks and sources (O4, O5) — 2026-10-22

Binaries `adv/bin/sink-base` (`c6c9c2a07`: flow carrier, sinks silent), `adv/bin/sink`
(`a6a24b7a0`: provenance and sinks) and `adv/bin/src` (`bab1509d0`: plus the source
catalogue); comparisons `adv/compare-sink.py`, `adv/compare-src.py` (`--dump` in
`sink-dump.txt`, `src-dump.txt`). Loadavg 6-12 (no timing taken).

Without the catalogue (sink): 4 gained, 27 lost. With it (src): 10 gained, 28 lost.
The prototype gained 863.

Gained with the catalogue, all read:
- Piwigo, request-driven, real (TypeError at `array_slice` or an `int` parameter):
  `CategoryImagesController:118`, `ImageSearchController:80`, `FavoriteListController:75`,
  `TagImagesController:81`, `UserListController:108`, and `CommentListController:122`
  (same shape, not in the earlier list).
- WordPress `wp-admin/widgets-form.php:196`: `(int) $_POST[...] - 1` at `PHP_INT_MIN`
  into `array_splice`; real.
- Piwigo `tests/Unit/Lang/TranslatorTest.php:885`: `filemtime() + 1000` into `touch`;
  real by the definition (a filesystem with 64-bit seconds stores any mtime), in a test.
- Piwigo `Menu/BlockManager.php:101`: `$idx * 50`, `$idx` stepped once per iteration of a
  `foreach` over an array. False: a foreach over an array runs at most 2^30 times; the
  counter band uses the run length only.
- Symfony `Tests/Helper/ProgressBarTest.php:1023`: a closure's static `$i++`, times 100000.
  Within the definition (2^34 steps a second), but a step there is a call, not a cycle.

Lost (28), all false positives removed: `strpos`/`mb_strpos` offsets and `return.type`
`int<0, max>` results now proven by the 2^56-byte string bound and `min`/`max` ranges,
Psalm's `max(1, ...)` thread counts, Symfony's `$argv` now `list<string>`.

True positives still not reported:
- Piwigo `ImageGetController:150`: `$commentsPerPage` is the request's value or
  `$this->currentConfig->nbCommentPage`; the union carries an environment node and a
  property node, so the site waits for the property's facts (O7). Sources are kept per
  kind of value, not per path, so the request path's own range is not separable.
- Piwigo `HistorySearchController:206`: through `HistorySearchInput::fromArray` (O7).
- Laravel `firstItem()`, monolog and Psalm memory shorthand: parameters (O7).
- WordPress `addTwoNumbers`: the XML-RPC dispatch table.

Regression found and fixed on the way: Psalm `FileDiffer.php:262` `return.type`, caused by
the bounded string length making a shape widen in more passes; `null|array{...}` now
widens as arrays beside other values, and `+ - * **` keep the overflow float a float
(`82ce8aeb9`).

### The foreach bound (`35009ddbb`), against `bab1509d0` — 2026-10-22

Binary `adv/bin/fe`, `adv/fe-vs-src.txt`. One finding lost, none gained, on Piwigo and the thirteen vendor packages: Piwigo `Menu/BlockManager.php:101` (`$idx * 50`, `$idx` stepped once per element of a `foreach` over an array), the false positive it targets.
