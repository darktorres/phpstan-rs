# Crate-split plan: cause lists

Appendices G–J of `CRATE_SPLIT_PLAN.md`: every nsrt-suite and rules-oracle
mismatch at `76e738b4`, sorted to the cause that owns it. The plan's items
name these causes by key ("Appendix H's `count-narrow`"); this file holds
the counts, descriptions and files behind each key. It is generated from
the oracle runs and edited only by regenerating it.

## Appendix G: built-in return types against the nsrt suite

The nsrt suite at `76e738b4` has 3,108 mismatches on a call to a function one of
PHPStan's `Type/Php/*ReturnTypeExtension`s types, spread over 94 extensions.
Every line was read against its fixture. 46 differ only in a display form the
gate's normalization covers, and 37 are another cause's, the extension being
right about what reaches it: Appendix H's `float-literal` (7, `abs.php`),
`range-union` (4, `abs.php`), `operator-ranges` (4, `bug-14654.php`),
`property-assign-narrow` (4, `bug-5129.php`), `closure-param-infer` (3,
`array-find-key.php`, `array-find.php`, `pipe-operator.php`),
`callable-value-call` (`bug-13296.php`), `coalesce-fold`
(`isset-coalesce-empty-type.php`), `count-narrow` (`bug-4499.php`),
`increment-types` (`bug-10187.php`), `is-fns-narrow` (`array_values.php`),
`short-ternary` (`do-not-remember-impure-functions.php`) and `union-normalize`
(`filter-var.php`); D3's nested-write (2, `bug-5316.php`), first-class callable
(`first-class-callables.php`) and `gettype()` comparison (`bug-9404.php`) items;
E2's spread-argument item (2, `array-intersect-key.php`); Appendix I's
`constant-condition-certainty` (`get-defined-vars.php`); and C9's
global-variable item: `global $global;` in a function is `'foo'` from a
file-scope assignment, where the function can run before it or after a
`$GLOBALS` write (`get-defined-vars.php`). Ten are phpstan-rs right where
PHPStan's value is wrong at runtime, approved deviations: `vsprintf('foo', [])`
is always `'foo'`, so its `non-falsy-string` stands where PHPStan has `string`
(`non-falsy-string.php`); `array_flip()` of an array holding `'foo' => 17` and
`'bar' => 17` is `array<int, string>` where PHPStan has `*NEVER*`, PHP returning
`[17 => 'bar']` (`array-flip.php:93`); `get_class($mixed)` is `string` where
PHPStan adds `false`, which PHP 8 never returns (`generics.php:1344`);
`array_fill(-2, 4, 'pear')` has the keys `-2` to `1` from PHP 8, where PHPStan
has PHP 7's `-2`, `0`, `1`, `2` (`array-functions.php:261`);
`ini_get('auto_prepend_file')` is `''` by default, which PHPStan's
`non-falsy-string` excludes (`bug-3382.php`); `filter_var(null,
FILTER_VALIDATE_BOOLEAN, FILTER_NULL_ON_FAILURE)` returns `false`, not PHPStan's
`null` (`filter-var.php:118`); `mb_substitute_character(null)` returns the
current setting, not PHPStan's `true` (`mb_substitute_character-php8.php:5`);
and `strval([])`, an invalid pattern in `preg_split()` and its `null` flags
return (`'Array'`, `false`, a list) where PHPStan has `*ERROR*`
(`strval.php:26`, `preg_split.php:32`, `:38`). The other 3,015, over 93
extensions, are the extensions' own, each read against its fixture and, where
the two tools disagree, php 8.5: 2,498 are wider than PHPStan's type and hold
every value it holds; 190 are calls PHP rejects (a `TypeError`, `ValueError` or
`Error`) typed as returning; 107 are in files that run before PHP 8 or 8.4 and
get PHP 8.5's return; and 220 miss a value the call can return. The rows whose
expression php can evaluate outside its fixture were run in php 8.5: that found
seven of the ten above and 20 of the 190, and for 396 of the 2,498 PHPStan's
value held. E2 names each group; one that stays is phpstan-rs being right at
runtime and is listed as an approved deviation. Each entry gives the extension,
the functions the suite asserts, the mismatch count and files, and one mismatch
from the run (phpstan-rs's type first). The extensions not listed have no
mismatches of their own; `ArrayFindKeyFunctionReturnTypeExtension`'s two differ
only in display.

- `FilterVarDynamicReturnTypeExtension` (`filter_var()`; 277, 3 files):
  `filter_var(17)` is `mixed` where PHPStan has `'17'`;
- `MinMaxFunctionReturnTypeExtension` (`max()`, `min()`; 217, 10 files):
  `max([1])` is `mixed` where PHPStan has `1`;
- `ArrayColumnFunctionReturnTypeExtension` (`array_column()`; 211, 5 files):
  `array_column($array, 'foo')` is `array` where PHPStan has `list`;
- `SprintfFunctionDynamicReturnTypeExtension` (`sprintf()`, `vsprintf()`; 135, 6
  files): `sprintf('%d', $constant)` is `string` where PHPStan has `'0'`;
- `BcMathStringOrNullReturnTypeExtension` (`bcdiv()`, `bcmod()`, `bcpowmod()`,
  `bcsqrt()`; 134, 2 files): `bcsqrt('-10')` is `string` where PHPStan has
  `null`;
- `CountFunctionReturnTypeExtension` (`count()`, `sizeof()`; 114, 16 files):
  `count($arr)` is `int<0, max>` where PHPStan has `1`;
- `ArraySliceFunctionReturnTypeExtension` (`array_slice()`; 101, 4 files):
  `array_slice($b, 1)` is `non-empty-list` where PHPStan has `list`;
- `ClassImplementsFunctionReturnTypeExtension` (`class_implements()`,
  `class_parents()`, `class_uses()`; 90, `class-implements.php`):
  `class_uses('thisIsNotAClass')` is `array<(int|string), string>|false` where
  PHPStan has `false`;
- `ExplodeFunctionDynamicReturnTypeExtension` (`explode()`; 66, 7 files):
  `explode('', $v)` is `non-empty-list<string>` where PHPStan has `false`;
- `ArraySearchFunctionDynamicReturnTypeExtension` (`array_search()`; 63, 8
  files): `array_search('b', $arr)` is `int|false` where PHPStan has `1`;
- `StrvalFamilyFunctionReturnTypeExtension` (`boolval()`, `floatval()`,
  `intval()`, `strval()`; 60, 3 files): `intval([])` is `int` where PHPStan has
  `0`;
- `HashFunctionsReturnTypeExtension` (`hash()`, `hash_file()`, `hash_hkdf()`,
  `hash_hmac()`, `hash_hmac_file()`, `hash_pbkdf2()`; 58, 3 files):
  `hash_hkdf('crc32', 'key')` is `string` where PHPStan has `false`;
- `RoundFunctionReturnTypeExtension` (`ceil()`, `floor()`, `round()`; 57, 3
  files): `ceil(array())` is `float` where PHPStan has `false`;
- `StrIncrementDecrementFunctionReturnTypeExtension` (`str_decrement()`,
  `str_increment()`; 56, 3 files): `str_decrement($s)` is `string` where PHPStan
  has `'a'`;
- `SubstrDynamicReturnTypeExtension` (`mb_substr()`, `substr()`; 55, 10 files):
  `substr('de_DE', 5, -5)` is `string` where PHPStan has `''`;
- `PregSplitDynamicReturnTypeExtension` (`preg_split()`; 42, `preg_split.php`):
  `preg_split('/-/', '')` is `list<string>|false` where PHPStan has
  `array{''}|false`;
- `ImplodeFunctionReturnTypeExtension` (`implode()`, `join()`; 42, 9 files):
  `implode(['12', '345'])` is `string` where PHPStan has `'12345'`;
- `PowFunctionReturnTypeExtension` (`pow()`; 42, `pow.php`): `pow($s, 0)` is
  `object|int|float` where PHPStan has `1`;
- `ArrayFillKeysFunctionReturnTypeExtension` (`array_fill_keys()`; 41, 7 files):
  `array_fill_keys($mixed, 'b')` is `array<'b'>` where PHPStan has `null`;
- `ArrayIntersectKeyFunctionReturnTypeExtension` (`array_intersect_key()`; 40, 7
  files): `array_intersect_key(['a' => 1])` is `array` where PHPStan has
  `array{a: 1}`;
- `ArrayCombineFunctionReturnTypeExtension` (`array_combine()`; 40, 6 files):
  `array_combine([1], [2])` is `array{0: 2}` where PHPStan has `array{1: 2}`;
- `RangeFunctionReturnTypeExtension` (`range()`; 40, 8 files): `range(2, 5, 2)`
  is `array` where PHPStan has `array{2, 4}`;
- `ArrayMergeFunctionDynamicReturnTypeExtension` (`array_merge()`; 38, 5 files):
  `array_merge([], ...[])` is `array` where PHPStan has `array{}`;
- `ArrayReplaceFunctionReturnTypeExtension` (`array_replace()`; 38, 4 files):
  `array_replace($array)` is `array` where PHPStan has `non-empty-array`;
- `MbSubstituteCharacterDynamicReturnTypeExtension`
  (`mb_substitute_character()`; 37, 2 files): `mb_substitute_character('')` is
  `bool|int|string` where PHPStan has `true`;
- `ArrayMapFunctionReturnTypeExtension` (`array_map()`; 36, 9 files):
  `array_map(null, [])` is `array` where PHPStan has `array{}`;
- `ArrayChunkFunctionReturnTypeExtension` (`array_chunk()`; 35, 5 files):
  `array_chunk($arr, 2)` is `array<array>` where PHPStan has
  `list<non-empty-list<mixed>>`;
- `GetClassDynamicReturnTypeExtension` (`get_class()`; 31, 10 files):
  `get_class($entity)` is `string` where PHPStan has `class-string`;
- `ArrayPadDynamicReturnTypeExtension` (`array_pad()`; 31, `array_pad.php`):
  `array_pad($listString, $int, 'foo')` is `array<string>` where PHPStan has
  `list<string>`;
- `ArrayKeysFunctionDynamicReturnTypeExtension` (`array_keys()`; 31, 9 files):
  `array_keys($arr)` is `list` where PHPStan has `array{}`;
- `MbStrlenFunctionReturnTypeExtension` (`mb_strlen()`; 30, 2 files):
  `mb_strlen('')` is `int` where PHPStan has `0`;
- `ArrayChangeKeyCaseFunctionReturnTypeExtension` (`array_change_key_case()`;
  29, 2 files): `array_change_key_case($arr2)` is `array<string, string>` where
  PHPStan has `array<lowercase-string, string>`;
- `ArrayReverseFunctionReturnTypeExtension` (`array_reverse()`; 29, 4 files):
  `array_reverse([])` is `array` where PHPStan has `array{}`;
- `GetParentClassDynamicFunctionReturnTypeExtension` (`get_parent_class()`; 29,
  3 files): `get_parent_class(self::class)` is `string|false` where PHPStan has
  `false`;
- `ArraySumFunctionDynamicReturnTypeExtension` (`array_sum()`; 26,
  `array-sum.php`): `array_sum($list)` is `int|float` where PHPStan has `0`;
- `NonEmptyStringFunctionsReturnTypeExtension` (`addcslashes()`, `addslashes()`,
  `escapeshellarg()`, `escapeshellcmd()`, `htmlentities()`,
  `htmlspecialchars()`, `preg_quote()`, `rawurldecode()`, `rawurlencode()`,
  `urldecode()`, `urlencode()`; 26, 2 files): `urldecode($nonEmpty)` is `string`
  where PHPStan has `non-empty-string`;
- `StrCaseFunctionsReturnTypeExtension` (`lcfirst()`, `mb_convert_case()`,
  `mb_convert_kana()`, `mb_strtolower()`, `mb_strtoupper()`, `strtoupper()`,
  `ucwords()`; 26, 3 files): `mb_strtolower('Ａｂｃ')` is `'Ａｂｃ'` where PHPStan has
  `'ａｂｃ'`;
- `AbsFunctionDynamicReturnTypeExtension` (`abs()`; 25, `abs.php`): `abs(null)`
  is `int|float` where PHPStan has `0`;
- `ArrayFilterFunctionReturnTypeExtension` (`array_filter()`; 25, 11 files):
  `array_filter($falsey)` is `array<int, never>` where PHPStan has `array{}`;
- `MbConvertEncodingFunctionReturnTypeExtension` (`mb_convert_encoding()`; 25, 3
  files): `mb_convert_encoding($arr)` is `array|string|false` where PHPStan has
  `array|false`;
- `ReplaceFunctionsDynamicReturnTypeExtension` (`preg_replace()`,
  `preg_replace_callback_array()`, `strtr()`; 25, 6 files): `preg_replace($s,
  $ls, $ls)` is `string|null` where PHPStan has `lowercase-string|null`;
- `StrlenFunctionReturnTypeExtension` (`strlen()`; 24, 5 files): `strlen('')` is
  `int<0, max>` where PHPStan has `0`;
- `DateFunctionReturnTypeExtension` (`date()`; 22, 5 files): `date('')` is
  `string` where PHPStan has `''`;
- `StrRepeatFunctionReturnTypeExtension` (`str_repeat()`; 22, 4 files):
  `str_repeat($nonEmpty, 1)` is `string` where PHPStan has `non-empty-string`;
- `TrimFunctionDynamicReturnTypeExtension` (`chop()`, `rtrim()`, `trim()`; 22, 4
  files): `trim($foo, $fooOrBar)` is `string` where PHPStan has `''|'foo'`;
- `ArrayFlipFunctionReturnTypeExtension` (`array_flip()`; 21, 7 files):
  `array_flip($unsealed)[1]` is `0` where PHPStan has `int`;
- `StrPadFunctionReturnTypeExtension` (`str_pad()`; 21, 5 files): `str_pad($s,
  1)` is `string` where PHPStan has `non-empty-string`;
- `ArrayValuesFunctionDynamicReturnTypeExtension` (`array_values()`; 19, 7
  files): `array_values($arr)` is `list` where PHPStan has `array{}`;
- `ArrayFirstLastDynamicReturnTypeExtension` (`array_first()`,
  `array_key_first()`, `array_key_last()`, `array_last()`; 18, 2 files):
  `array_last($mixed)` is `mixed|null` where PHPStan has `mixed`;
- `RandomIntFunctionReturnTypeExtension` (`mt_rand()`, `rand()`, `random_int()`;
  17, 3 files): `rand(5, 1)` is `int<5, 1>` where PHPStan has `int<1, 5>`;
- `CountCharsFunctionDynamicReturnTypeExtension` (`count_chars()`; 16, 2 files):
  `count_chars(self::ABC, 3)` is `array<(int|string), int>|string` where PHPStan
  has `string`;
- `FilterInputDynamicReturnTypeExtension` (`filter_input()`; 13, 4 files):
  `filter_input(-1, 'foo', FILTER_VALIDATE_INT)` is `int|null|false` where
  PHPStan has `null`;
- `ArrayRandFunctionReturnTypeExtension` (`array_rand()`; 12,
  `array-functions.php`): `array_rand(["a" => 1, 2 => "b"])` is `string|2` where
  PHPStan has `2|'a'`;
- `DateTimeDynamicReturnTypeExtension` (`date_create_from_format()`,
  `date_create_immutable_from_format()`; 12, `DateTimeDynamicReturnTypes.php`):
  `date_create_from_format('Y-m-d', '2022/02/20')` is `DateTime|false` where
  PHPStan has `false`;
- `LtrimFunctionReturnTypeExtension` (`ltrim()`; 12, 5 files): `ltrim($foo,
  $fooOrBar)` is `string` where PHPStan has `''|'foo'`;
- `VersionCompareFunctionDynamicReturnTypeExtension` (`version_compare()`; 12, 2
  files): `\version_compare('Foo','Bar','<')` is `int|bool` where PHPStan has
  `false`;
- `ArrayFillFunctionReturnTypeExtension` (`array_fill()`; 11, 3 files):
  `array_fill(2, $integer, new \stdClass())` is `array<stdClass>` where PHPStan
  has `array<int, stdClass>`;
- `OpensslCipherFunctionsReturnTypeExtension` (`openssl_cipher_iv_length()`,
  `openssl_cipher_key_length()`; 11, 3 files):
  `openssl_cipher_iv_length('unknown')` is `int|false` where PHPStan has
  `false`;
- `ParseUrlFunctionDynamicReturnTypeExtension` (`parse_url()`; 11, 2 files):
  `parse_url('abc.def', PHP_URL_SCHEME)` is `string|false|null` where PHPStan
  has `null`;
- `DateTimeCreateDynamicReturnTypeExtension` (`date_create()`,
  `date_create_immutable()`; 10, `DateTimeCreateDynamicReturnTypes.php`):
  `date_create('202/20')` is `DateTime|false` where PHPStan has `false`;
- `ConstantFunctionReturnTypeExtension` (`constant()`; 9, `constant.php`):
  `constant('FOO')` is `mixed` where PHPStan has `'foo'`;
- `IteratorToArrayFunctionReturnTypeExtension` (`iterator_to_array()`; 9,
  `iterator_to_array.php`): `iterator_to_array($foo)` is `array` where PHPStan
  has `array<string, int>`;
- `HighlightStringDynamicReturnTypeExtension` (`highlight_string()`; 8, 2
  files): `highlight_string($string)` is `string|true` where PHPStan has `true`;
- `PregFilterFunctionReturnTypeExtension` (`preg_filter()`; 8,
  `preg_filter.php`): `preg_filter($pattern)` is `string|array<(int|string),
  string>|null` where PHPStan has `list<string>|string|null`;
- `StrShuffleFunctionReturnTypeExtension` (`str_shuffle()`; 8,
  `str-shuffle.php`): `str_shuffle($nonES)` is `string` where PHPStan has
  `non-empty-string`;
- `StrSplitFunctionReturnTypeExtension` (`mb_str_split()`; 8, 2 files):
  `mb_str_split('', 1)` is `array` where PHPStan has `array{}`;
- `IniGetReturnTypeExtension` (`ini_get()`; 7, `ini-get.php`):
  `ini_get("memory_limit")` is `string|false` where PHPStan has `string`;
- `PgLastNoticeDynamicReturnTypeExtension` (`pg_last_notice()`; 7,
  `pg-last-notice.php`): `pg_last_notice($connection)` is `array|string|bool`
  where PHPStan has `string`;
- `TriggerErrorDynamicReturnTypeExtension` (`trigger_error()`; 7, 3 files):
  `trigger_error('bar', $errorLevels[4])` is `true` where PHPStan has `false`;
- `DateFormatFunctionReturnTypeExtension` (`date_format()`; 6, 2 files):
  `date_format($dt, '')` is `string` where PHPStan has `''`;
- `JsonThrowOnErrorDynamicReturnTypeExtension` (`json_encode()`; 6, 2 files):
  `json_encode($mixed)` is `string|false` where PHPStan has
  `non-empty-string|false`;
- `StrrevFunctionReturnTypeExtension` (`strrev()`; 6, `strrev.php`):
  `strrev($constantStrings)` is `string` where PHPStan has `'oof'|'rab'`;
- `CompactFunctionReturnTypeExtension` (`compact()`; 5, `compact.php`):
  `compact($dolor)` is `array` where PHPStan has `array<string, mixed>`;
- `GetDebugTypeFunctionReturnTypeExtension` (`get_debug_type()`; 5,
  `get-debug-type.php`): `get_debug_type($o)` is `string` where PHPStan has
  `'stdClass'`;
- `CurlGetinfoFunctionDynamicReturnTypeExtension` (`curl_getinfo()`; 5,
  `curl_getinfo.php`): `curl_getinfo($handle, PHP_EOL)` is `mixed` where PHPStan
  has `false`;
- `LocaltimeFunctionDynamicReturnTypeExtension` (`localtime()`; 5,
  `localeconv.php`): `localtime()` is `array` where PHPStan has `array{int<0,
  59>, int<0, 59>, int<0, 23>, int<1, 31>, int<0, 11>, int, int<0, 6>, int<0,
  365>, int}`;
- `PathinfoFunctionDynamicReturnTypeExtension` (`pathinfo()`; 5, 2 files):
  `pathinfo($s, $flag)` is `array|string` where PHPStan has `string`;
- `ArrayCurrentDynamicReturnTypeExtension` (`current()`; 4, 2 files):
  `current($a)` is `string|false` where PHPStan has `string`;
- `IdateFunctionReturnTypeExtension` (`idate()`; 4, `idate.php`): `idate('Y')`
  is `int|false` where PHPStan has `int`;
- `GetDefinedVarsFunctionReturnTypeExtension` (`get_defined_vars()`; 3,
  `get-defined-vars.php`): `get_defined_vars()` is `array{global: 'foo'}` where
  PHPStan has `array<string, mixed>`;
- `ArrayShiftFunctionReturnTypeExtension` (`array_shift()`; 3, 3 files):
  `array_shift($arr)` is `mixed` where PHPStan has `1|null`;
- `GettypeFunctionReturnTypeExtension` (`gettype()`; 3, `gettype.php`):
  `gettype($r)` is `'resource'` where PHPStan has `'resource'|'resource
  (closed)'`;
- `ArrayFindFunctionReturnTypeExtension` (`array_find()`; 3, `array-find.php`):
  `array_find($array, 'is_int')` is `mixed` where PHPStan has `int|null`;
- `NumberFormatFunctionDynamicReturnTypeExtension` (`number_format()`; 3,
  `number_format.php`): `number_format(1002.7, 3, '', '')` is `string` where
  PHPStan has `numeric-string`;
- `PgResultStatusDynamicReturnTypeExtension` (`pg_result_status()`; 3,
  `pg-result-status.php`): `pg_result_status($result)` is `string|int` where
  PHPStan has `int`;
- `SscanfFunctionDynamicReturnTypeExtension` (`sscanf()`; 3, `sscanf.php`):
  `sscanf($s, $s)` is `array|int|null` where PHPStan has `array|null`;
- `StrTokFunctionReturnTypeExtension` (`strtok()`; 3, `bug-3981.php`):
  `strtok('', ' ')` is `string|false` where PHPStan has `false`;
- `ArrayPopFunctionReturnTypeExtension` (`array_pop()`; 2, 2 files):
  `array_pop($arr)` is `0|1|2` where PHPStan has `0|1|2|null`;
- `Base64DecodeDynamicFunctionReturnTypeExtension` (`base64_decode()`; 2,
  `base64_decode.php`): `base64_decode($string)` is `string|false` where PHPStan
  has `string`;
- `DateIntervalFormatFunctionReturnTypeExtension` (`date_interval_format()`; 2,
  `bug-1452.php`): `date_interval_format($interval, '%a')` is `string` where
  PHPStan has `lowercase-string&non-empty-string`;
- `GetCalledClassDynamicReturnTypeExtension` (`get_called_class()`; 2,
  `bug-14951.php`): `get_called_class()` is `class-string<Bug14951\A>` where
  PHPStan has `class-string<static(Bug14951\A)>`;
- `ArrayCountValuesDynamicReturnTypeExtension` (`array_count_values()`; 1,
  `array-count-values.php`): `array_count_values(returnsStringOrObjectArray())`
  is `array<int>` where PHPStan has `array<string, int<1, max>>`;
- `ArrayKeyDynamicReturnTypeExtension` (`key()`; 1, `array-functions.php`):
  `key($generalStringKeys)` is `int|string|null` where PHPStan has
  `string|null`.

## Appendix H: the nsrt suite's uncited files by cause

At `76e738b4`, 1,066 of the 1,360 files with mismatches are cited nowhere else
in this plan. Of their 5,188 mismatches that aren't on a call Appendix G covers,
5,137 are traced here, line by line, to 306 causes; the other 51 differ only in
a display form the gate's normalization covers (`never` against `*NEVER*`, a
nested union's order, a list shape's explicit keys, `array<(int|string), T>`).
Every line was read against its fixture, the 325 files with a single mismatch
included: a line sits under its file's main cause unless it belongs to another,
and 473 lines in 142 files do. `gates/nsrt/owners.toml` takes these lines as its
first draft. Each entry gives the cause's key, its mismatch count and files,
what holds, and the files, most mismatches first, with a file's count where the
cause has only some of its lines. Every entry is a phase item: the phase is done
with it when its lines have no mismatches, or each remaining line is an approved
deviation that names its runtime reason. `ext:` keys are E2's extensions of the
same name; their calls' own mismatches are in Appendix G, and these are the
values that flow on from them.

**B3** (6 mismatches, 3 files, 2 causes):

- `case-insensitive-names` (4, 1 file): `PARENT::` is `parent::`;
  `case-insensitive-parent.php`.
- `php8-precedence` (2, 2 files): from PHP 8, `+`/`-` bind tighter than `.`, so
  `'x' . microtime(true) - $f` concatenates the difference (`non-falsy-string`);
  before PHP 8 it subtracts from a non-numeric string (*ERROR*); `float` today
  either way (B3's precedence item); `bug-5168-php7.php`, `bug-5168-php8.php`.

**B4** (947 mismatches, 272 files, 58 causes):

- `type-print` (127, 42 files): display forms the nsrt normalization doesn't
  cover yet (`callable` as `callable(): mixed`, a template with its bound,
  `array<mixed, mixed>`, anonymous class names, `iterable` not split into
  `array|Traversable`); B4's printer or the gate's normalization;
  `bug-15060.php` (13), `callable-object.php`, `callable-type-predicate.php`
  (7), `callable-string.php`, `generic-callables.php`,
  `more-type-strings-php8.php`, `bug-12355.php`, `bug-4803.php`, `bug-7987.php`,
  `bug-13809.php`, `bug-3266.php`, `more-type-strings.php`,
  `anonymous-class-name-same-line.php`, `array-filter-string-callables.php` (3),
  `benevolent-union-math.php`, `bug-5961.php`, `bug-8242.php`,
  `type-aliases.php` (3), `anonymous-class-name.php` (2),
  `array-filter-arrow-functions.php` (2), `array-filter-callables.php` (2),
  `bug-12891.php`, `bug-13577.php`, `bug-14545.php`, `bug-3474.php`,
  `bug-6170.php`, `bug-8268.php`, `bug2574.php`,
  `specifiedTypesUsingIsFunctions.php` (2), `template-constant-bound.php`,
  `typehints-anonymous-function.php` (2), `bug-1233.php` (1), `bug-12989.php`,
  `bug-14630.php` (1), `bug-3446.php`, `bug-4205.php`, `bug-5785.php`,
  `bug-6120.php`, `bug-8886.php`, `list-type.php` (1), `typehints.php` (1),
  `var-annotations.php` (1).
- `error-type` (65, 22 files): an invalid expression is `*ERROR*` rather than
  `mixed`: a missing member, an unsupported operand, a division by zero, a
  negative shift, a PHPDoc for a missing parameter, `$this` in a static closure;
  `bug-6624.php`, `array-destructuring.php` (9), `gmp-operators.php`,
  `div-by-zero.php`, `string-array-access.php`, `union-intersection.php` (4),
  `bug-5223.php`, `bug-7490.php`, `static-late-binding.php` (2),
  `anonymous-function.php` (1), `arrow-functions-inside.php`,
  `bug-10302-trait-extends.php`, `bug-12447-bis.php`, `bug-13270a.php` (1),
  `functionPhpDocs-phanPrefix.php` (1), `functionPhpDocs-phpstanPrefix.php` (1),
  `functionPhpDocs-psalmPrefix.php` (1), `functionPhpDocs.php` (1),
  `initializer-expr-type-resolver.php` (1), `literal-arrays.php`,
  `property-array.php`, `static-closure.php`.
- `static-type` (64, 17 files): `static` and `$this(Foo)` survive returns,
  loops, `try` and constructors, and `get_called_class()` is
  `class-string<static>`; `static-late-binding.php` (10),
  `method-phpDocs-inheritdoc-without-curly-braces.php` (8),
  `methodPhpDocs-phanPrefix.php` (8), `methodPhpDocs-phpstanPrefix.php` (8),
  `methodPhpDocs-psalmPrefix.php` (8), `bug-14804.php`, `bug-7550.php`,
  `bug-9341.php`, `bug-10477.php`, `bug-2231.php`, `bug-4707.php`,
  `this-subtractable.php` (2), `bug-6308.php`, `deducted-types.php` (1),
  `negated-instanceof.php` (1), `object-shape.php` (1), `var-annotations.php`
  (1).
- `conditional-return` (57, 12 files): a conditional return type resolves on the
  argument's type: arrays by emptiness, templates, nested conditions, `$this`,
  class types, and against the argument's value before a reassignment;
  `conditional-types.php` (20), `conditional-type-for-parameter-template.php`,
  `bug-7279.php`, `bug-11565.php`, `bug-15296.php`, `bug-11894.php`,
  `bug-9000.php`, `conditional-type-for-parameter-template-bound.php`,
  `bug-12562.php`, `bug-3853.php`, `bug-9360.php`, `fizz-buzz.php`.
- `generic-new-infer` (54, 14 files): `new` infers the class's templates from
  constructor arguments (`WeakMap`, `ArrayObject`, `ReflectionClass`,
  `DatePeriod`, `SensitiveParameterValue`), `never` with none; `bug-11844.php`,
  `bug-15032.php`, `bug-12473-types.php`, `bug-13144.php`,
  `date-period-return-types.php` (5), `sensitive-parameter-value.php`,
  `bug-13783.php`, `bug-6505.php`, `native-reflection-default-values.php`,
  `bug-4725.php`, `bug-9084.php`, `generics-do-not-generalize.php` (2),
  `bug-6993.php`, `bug-7078.php`.
- `subtracted` (54, 12 files): subtracted types: `mixed~null`, `mixed~false`,
  `$this(Foo~Bar)` and `Enum~Case` after negative checks; `mixed-subtract.php`,
  `this-subtractable.php` (11), `falsy-isset.php` (5), `bug-pr-339.php`,
  `mixed-to-number.php` (4), `bug-15060.php` (3), `negated-instanceof.php` (3),
  `subtracted.php`, `allowed-subtypes-enum.php`, `bug-1233.php` (2),
  `array_values.php` (1), `bug-12107.php` (1).
- `union-normalize` (54, 29 files): unions aren't normalized: duplicate members
  (`Dog|Dog`, `int|int|string`), `never` members (`true|never`), and
  `bool|int|false` for `bool|int`; `bug-15268.php`, `cast-to-numeric-string.php`
  (5), `bug-13192.php`, `bug-14891.php`, `has-offset-type-bug.php` (3),
  `phpunit-integration.php` (3), `bug-10871.php`, `bug-7877.php` (2),
  `non-empty-string-str-containing-fns.php` (2), `bug-10938.php`,
  `bug-13810.php` (1), `bug-14047.php` (1), `bug-14455.php` (1),
  `bug-14610.php`, `bug-15151.php`, `bug-2735.php` (1), `bug-7913.php` (1),
  `bug-8467b.php` (1), `bug-9634.php`, `closure-passed-by-reference-return.php`,
  `closure-passed-by-reference.php` (1), `is-a.php` (1), `key-of-generic.php`
  (1), `multi-assign.php` (1), `pr-5379.php`, `pr-5445.php`, `tagged-unions.php`
  (1), `throw-expr.php`, `union-intersection.php` (1).
- `shape-union` (39, 9 files): branches with different shapes stay a union of
  shapes rather than one optional-key shape; `bug11480.php`, `bug-13561.php`
  (6), `narrow-tagged-union.php` (3), `bug-9908.php`,
  `array-merge-const-non-const.php` (1), `array-replace-const-non-const.php`
  (1), `bug-10834.php`, `bug-14938.php` (1), `narrow-cast.php` (1).
- `callable-template-infer` (35, 12 files): a callable argument (closure, arrow
  function, first-class or string callable, `uksort`'s callback) infers a
  template from its parameters and return; `bug-12691.php`, `uksort-bug.php`,
  `bug-7281.php`, `smaller-than-benevolent.php`, `closure-passed-to-type.php`,
  `bug-14203.php`, `bug-5675.php`, `bug-9086.php`, `bug-9472.php`,
  `bug-5508.php`, `bug-6196.php`, `bug-8257.php`.
- `variadic-list` (27, 8 files): variadics and `func_get_args()` are lists
  before PHP 8 and named-key arrays from 8; `bug-2600-php8.php`, `bug-2600.php`,
  `variadic-parameter-php8.php`, `bug-2600-php-version-scope.php`,
  `anonymous-function.php` (1), `bug-5698-php7.php`, `bug-5698-php8.php`,
  `typehints.php` (1).
- `template-bound-generic` (23, 3 files): templates bounded by generics of other
  templates unwrap; `nested-generic-types.php`,
  `nested-generic-types-unwrapping-covariant.php`,
  `nested-generic-types-unwrapping.php`.
- `template-narrow` (21, 7 files): narrowing a template-typed value keeps `T` in
  the intersection; `bug-6591.php`, `bug-10083.php`, `bug-4498.php`,
  `bug-9961.php`, `bug-14348.php`, `bug-2580.php`, `bug-7688.php`.
- `template-generalize` (19, 8 files): inferred template arguments generalize
  (`Foo<1>` is `Foo<int>`) unless the argument is a class-string or the bound is
  nullable; `bug-3226.php`, `generic-generalization.php`,
  `generics-do-not-generalize.php` (4), `assert-class-type.php`, `self-out.php`
  (2), `bug-11430.php`, `bug-7423.php`, `template-null-bound.php`.
- `class-constant` (18, 6 files): `$this::class` in a final class is the
  literal, `$x::class` is `class-string<X>&literal-string`, and `::class` in an
  array literal stays a literal; `class-constant-narrowing.php`, `bug-5843.php`,
  `bug-14683.php` (2), `bug-2677.php`, `bug-4875.php`, `bug-6864.php` (1).
- `conditional-param` (17, 3 files): a conditional parameter type (`$k is 'a' ?
  A : B`) resolves when the other parameter narrows; `bug-7915.php`,
  `bug-10055.php`, `pr-5108.php`.
- `decimal-int-string` (15, 1 file): `decimal-int-string` and
  `non-decimal-int-string` types, and `[$s => 1]` keyed by them;
  `decimal-int-string.php` (15).
- `template-bound-union` (15, 3 files): a template bounded by a union stays `T`
  when returned; `bug-6566-types.php`, `bug-9939.php`, `bug-5000.php`.
- `callable-array` (14, 2 files): `is_array()` on a callable is
  `non-empty-list&callable(): mixed`, and `[$object, 'method']` is
  `list&callable`; `bug-3842.php`, `bug-4510.php`.
- `shape-intersect` (14, 2 files): intersecting two shapes, or an array with a
  shape, folds into one shape; `bug-11234.php`, `constant-array-intersect.php`.
- `template-never` (14, 2 files): a `never` or empty-array argument infers a
  template as `never`; `never-into-union-template.php`,
  `generics-empty-array.php`.
- `generic-static` (13, 1 file): `static(Foo<T, U>)` returned from a generic
  method keeps `static` and resolves the arguments; `generic-static.php`.
- `array-key-cast` (12, 3 files): literal and numeric-string array keys cast as
  PHP casts them (`'1'` is `1`, a `numeric-string` key is `int|numeric-string`);
  `literal-arrays-keys.php`, `bug-8592.php`, `list-type.php` (1).
- `generic-method-infer` (11, 6 files): a generic method's templates infer from
  arguments and resolve through returned generics; `bug-9456.php`,
  `bug-4545.php`, `bug-8127.php`, `collected-data.php`,
  `complex-generics-example.php`, `discussion-8447.php`.
- `intersection-static` (10, 2 files): a `static` return on an intersection
  receiver keeps the intersection; `intersection-static.php`, `bug-6462.php`.
- `template-return` (10, 6 files): a template parameter's return is the
  argument's type; `bug-6609-83.php`, `bug-6609.php`, `bug-4209-2.php`,
  `bug-4209.php`, `bug-6917.php`, `bug-9123.php`.
- `spread-string-keys` (9, 3 files): spreading string-keyed arrays keeps the
  string keys, which take no implicit index; `bug-15247-php81.php`,
  `bug-15248-string-key-unpacking.php`, `array-unpacking-string-keys.php` (1).
- `generic-this` (8, 2 files): a generic fluent method returning `$this` keeps
  `$this(Foo<T>)&static`; `bug-12575.php`, `new-in-initializers.php`.
- `void-null` (8, 4 files): a void call used as a value is `null`; `void.php`,
  `self-out.php` (2), `template-default.php` (2), `emptyiterator.php`.
- `class-constant-keys` (7, 1 file): `self::class` as an array key builds a
  shape; `array-shape-class-constant-key.php`.
- `conditional-param-out` (7, 2 files): a conditional `@param-out` type
  resolves, defaulted parameters included; `bug-10699.php`,
  `param-out-default.php`.
- `generic-closure-infer` (7, 1 file): a generic method's templates infer from
  closures and shapes; `bug-10254.php`.
- `object-shape` (7, 1 file): `object{…}` shapes: a missing property is
  `*ERROR*`, `(object) []` is `object{}&stdClass`; `object-shape.php` (7).
- `static-closure` (7, 4 files): a static closure is `static-Closure(...)`;
  `bug-15167.php` (2), `closure-static-type.php` (2), `degrade-closures.php`,
  `bug-9764.php` (1).
- `enum-name-value` (6, 2 files): `->name` and `->value` of an enum union or
  subtracted enum are the remaining cases' literals; `bug-14839.php`,
  `array-key-exists-on-subtracted.php` (1).
- `spread-list` (6, 2 files): `[...$arr]` is `list<mixed>`; `bug-5287.php`,
  `bug-5287-php81.php`.
- `implicit-nullable` (5, 2 files): a parameter defaulting to `null` is
  nullable; `nullable-closure-parameter.php`, `typehints-anonymous-function.php`
  (1).
- `template-infer-union` (5, 3 files): a template inferred from several
  arguments unions them; `generics-reduce-types-first.php`, `bug-3321.php`,
  `bug2577.php`.
- `template-subtract` (5, 1 file): `$items[$k] ?? null` on a generic list is
  `T~null|null`; `bug-4117.php`.
- `template-variadic` (5, 3 files): a variadic template parameter unions every
  argument; `bug-10092.php`, `bug-7068.php`, `bug-9881.php`.
- `clone-type` (4, 1 file): `clone` of `?object` is `object`; `clone.php`.
- `float-literal` (4, 1 file): float literals are constant floats (`1.0`);
  `deducted-types.php` (4).
- `magic-constants` (4, 1 file): `__CLASS__` is `class-string&literal-string`;
  `bug-3019.php`.
- `shape-dynamic-key` (4, 2 files): `[$k => $v]` is non-empty, with optional
  keys for a literal-union key; `bug-5219.php`, `bug-9907.php`.
- `spread-shapes` (4, 2 files): spreading shapes merges their keys, keeping
  required keys required; `bug-14708.php`, `bug-7776.php`.
- `template-in-shape` (4, 1 file): a template-typed property inside a shape
  keeps `T`; `bug-10445.php`.
- `new-dynamic` (3, 1 file): `new $class()` is `object`; `bug-4579.php`.
- `template-class-string` (3, 3 files): `class-string<T>` arguments resolve `T`,
  `static::class` included; `bug-2612.php`, `bug-4247.php`, `bug-7141.php`.
- `template-key` (3, 2 files): a template key survives iteration and offsets;
  `bug-5140.php`, `bug-7788.php`.
- `allowed-subtypes` (2, 2 files): a sealed hierarchy minus a member is the
  remaining member: `Throwable` without `Exception` is `Error`,
  `DateTimeInterface` without `DateTime` is `DateTimeImmutable`;
  `allowed-subtypes-datetime.php`, `allowed-subtypes-throwable.php`.
- `enum-this` (2, 1 file): `$this` in an enum narrowed by `match` is
  `$this(Enum)&Enum::A`; `bug-14478.php`.
- `enum-value-template` (2, 1 file): a static method returning `value-of` for an
  enum case argument folds to the value; `bug-13782.php`.
- `instanceof-expr` (2, 1 file): `$a instanceof $b` with an object on the right
  is `bool`; `bug-12107.php` (2).
- `range-union` (2, 1 file): adjacent integer ranges merge; `bug-14213.php` (2).
- `shape-from-array` (2, 1 file): `array<1, T>` is `array{1?: T}`;
  `array-shape-from-general-array-with-single-finite-key.php`.
- `template-bound` (2, 2 files): a template iterates and reads as its bound;
  `bug-6748.php` (1), `bug-7301.php`.
- `template-class-constant` (2, 1 file): `$this->model::class` on a
  template-typed property is `class-string<TModel>`; `bug-6864.php` (2).
- `template-default-param` (2, 1 file): a template parameter defaulting to
  `null` infers `null`; `bug-6584.php`.
- `template-invoke` (2, 1 file): `__invoke()` of a generic callable object
  returns the template; `bug-12894.php`.

**C4** (143 mismatches, 48 files, 19 causes):

- `generic-parent-params` (21, 5 files): a child's templates substitute into the
  parent's, so an inherited or overriding method's parameters take the generic
  types; `bug-9867.php`, `bug-2735.php` (5), `bug-4423.php`,
  `generic-parent.php`, `bug-12386.php`.
- `iterable-intersect` (19, 9 files): `iterable<T>&Collection` PHPDoc types keep
  the intersection and its offset type; `bug-2676.php`, `array-accessable.php`,
  `functionPhpDocs-phanPrefix.php` (2), `functionPhpDocs-phpstanPrefix.php` (2),
  `functionPhpDocs-psalmPrefix.php` (2), `functionPhpDocs.php` (2),
  `methodPhpDocs-phanPrefix.php` (2), `methodPhpDocs-phpstanPrefix.php` (2),
  `methodPhpDocs-psalmPrefix.php` (2).
- `define-constants` (16, 4 files): constants from `define()` and `const` inside
  functions, with a cast initializer and the namespaced fallback;
  `const-in-functions-namespaced.php`, `const-in-functions.php`,
  `cast-in-initializer-define.php`, `cast-in-initializer.php`.
- `inheritdoc` (12, 7 files): PHPDoc is inherited without `{@inheritdoc}`
  braces, from traits' abstract methods and with templates;
  `method-phpDocs-inheritdoc-without-curly-braces.php` (6),
  `abstract-generic-trait-method-implicit-phpdoc-inheritance.php`,
  `bug-9045.php`, `inherit-abstract-trait-method-phpdoc.php`,
  `inherit-phpdoc-merging-template.php`, `inheritdoc-from-trait.php`,
  `inheritdoc-without-curly-braces-from-trait.php`.
- `trait-templates` (11, 1 file): trait templates resolve through `@use` and
  their bounds; `generic-traits.php`.
- `class-constants` (10, 5 files): class constants resolve through `::class`
  values and spreads of other constants; `bug-7391.php`,
  `initializer-expr-type-resolver.php` (3), `bug-4950.php`, `bug-3379.php`,
  `bug-5458.php`.
- `generic-self-out` (10, 3 files): `@phpstan-self-out` on generic methods
  rewrites the receiver's arguments; `rule-error-builder.php`, `bug-13374.php`,
  `bug-3866.php`.
- `static-return` (8, 1 file): a `static` return stays `static(Foo)` inside the
  class; `methodPhpDocs.php`.
- `private-property-infer` (6, 1 file): an untyped private property takes its
  constructor-assigned type; `infer-private-property-type-from-constructor.php`.
- `promoted-phpdoc` (6, 1 file): a promoted property's PHPDoc from `@param` and
  `@var`; `promoted-properties-types.php`.
- `self-out` (6, 1 file): `@phpstan-self-out` rewrites the receiver;
  `self-out.php` (6).
- `no-named-arguments` (4, 1 file): `@no-named-arguments` makes a variadic a
  list; `no-named-arguments.php`.
- `property-tags` (4, 2 files): `@phpstan-property` overrides `@property`, and
  `@property` applies to a private property outside the class;
  `classPhpDocs-phpstanPropertyPrefix.php`,
  `property-phpdoc-tag-private-property.php`.
- `method-tags` (3, 2 files): `@method` tags type their returns and override a
  native method; `classPhpDocs.php`, `bug-3142.php`.
- `param-default-phpdoc` (2, 1 file): a parameter whose PHPDoc is a union of
  class constants keeps it with a constant default; `bug-5530.php`.
- `trait-method-phpdoc` (2, 1 file): a method from traits takes the right PHPDoc
  (the last trait's, or the class's own); `traits/traits.php`.
- `generic-property` (1, 1 file): a generic property returned through a method;
  `bug-8008.php`.
- `mixin` (1, 1 file): `@mixin` of a generic class forwards static calls;
  `model-mixin.php`.
- `trait-param-phpdoc` (1, 1 file): a trait method's `@param` applies inside the
  trait; `bug-9733.php`.

**C6** (173 mismatches, 48 files, 6 causes):

- `stub-return` (111, 33 files): method and function returns from the stubs
  (Redis, Ds, SPL, SQLite3, Imagick, SNMP, xdebug and others), `mixed` or wider
  today; `bug-9748.php`, `recursive-iterators.php`,
  `adapter-reflection-enum-return-types.php`, `ext-ds.php`, `bug-14736.php`,
  `bug-7056.php` (5), `imagick-pixel.php`, `image-size.php`,
  `password-get-info.php`, `sqlite3-result-fetch-all-php85.php`,
  `sqlite3-result-fetch-array.php`, `bug-13365.php`, `fpm-get-status.php`,
  `get_headers.php`, `ibm_db2.php`, `bug-11073.php`, `bug-7639.php`,
  `ds-copy.php`, `getopt.php`, `impure-connection-fns.php`, `memcache-get.php`,
  `redis-mget.php`, `snmp.php`, `bug-1519.php`, `bug-7341.php`, `bug-9714.php`,
  `die-74.php`, `error-get-last-php85.php`, `fgetcsv-php7.php`,
  `fgetcsv-php8.php`, `native-types-ftp-connect-resource.php`,
  `session-get-cookie-params-php85.php` (1), `ssh2-functions.php`.
- `stub-asserts` (39, 2 files): stub `@phpstan-assert` tags narrow
  (`is_readable`, `dir`, `chdir`, `chroot`);
  `non-empty-string-file-functions.php`, `bug-14829.php`.
- `arrayaccess-generic` (8, 4 files): offsets of `SplDoublyLinkedList`,
  `SplFixedArray`, `WeakMap` and `SplObjectStorage` return their generic value
  type (`mixed` today); `bug-9274.php`, `splfixedarray-iterator-types.php`,
  `bug-4436.php`, `weakMap.php`.
- `stub-byref` (8, 4 files): by-reference out parameters from the stubs
  (`socket_select`, `proc_open`'s pipes); `discussion-10285-php8.php`,
  `discussion-10285.php`, `bug-13197.php`, `bug-13197b.php`.
- `generic-reflection` (6, 5 files): `ReflectionClass<T>`'s `getName()`, `$name`
  and `$class` are `class-string<T>`; `bug-2927.php`, `bug-6889.php`,
  `bug-7056.php` (1), `isset-coalesce-empty-type-post-81.php`,
  `isset-coalesce-empty-type-pre-81.php`.
- `stub-property` (1, 1 file): `DateInterval::$days` is `int|false` after an
  impure call; `bug-14428.php`.

**C7** (106 mismatches, 36 files, 15 causes):

- `generic-defaults` (18, 2 files): a generic without arguments is `Foo<mixed>`
  and unions with `Foo<123>`; `bug-15198.php`, `bug-4504.php` (1).
- `unresolvable-phpdoc` (13, 11 files): an unresolvable PHPDoc type falls back
  to the native type; `psalm-prefix-unresolvable.php`, `unresolvable-types.php`,
  `count-recursive.php` (1), `functionPhpDocs-phanPrefix.php` (1),
  `functionPhpDocs-phpstanPrefix.php` (1), `functionPhpDocs-psalmPrefix.php`
  (1), `functionPhpDocs.php` (1),
  `method-phpDocs-inheritdoc-without-curly-braces.php` (1),
  `methodPhpDocs-phanPrefix.php` (1), `methodPhpDocs-phpstanPrefix.php` (1),
  `methodPhpDocs-psalmPrefix.php` (1).
- `phpdoc-types` (12, 5 files): PHPDoc keyword types (`pure-callable`,
  `callable-array`, `lowercase-string`/`uppercase-string` in shapes), and an
  invalid `non-empty-list` falls back to `array`; `more-types.php`,
  `lowercase-string-parse.php`, `non-empty-array.php`, `pure-callable.php`,
  `uppercase-string-parse.php`.
- `phpdoc-class-override` (11, 3 files): a class named like a PHPDoc pseudo-type
  wins when imported or in its namespace; `phpdoc-pseudotype-override.php`,
  `phpdoc-pseudotype-namespace.php`, `phpdoc-pseudotype-global.php`.
- `template-default` (9, 3 files): `@template T = X` defaults fill missing
  arguments; `template-default.php` (5), `generics-default.php`,
  `template-default-referring-other.php`.
- `shape-const-keys` (8, 1 file): `array{static::KEY: T}` shape keys resolve
  class constants; `bug-6989.php`.
- `offset-access-type` (7, 2 files): `T[K]` offset-access types and template
  keys in shapes; `offset-access.php`, `bug-15145.php`.
- `int-mask` (5, 1 file): `int-mask<…>` and `int-mask-of<…>` types;
  `int-mask.php`.
- `list-shape` (5, 1 file): `list{…}` shapes with optional elements;
  `array-shape-list-optional.php`.
- `phpdoc-const-expr` (4, 1 file): PHPDoc constant expressions keep float
  literals (`1.0`); `const-expr-phpdoc-type.php`.
- `type-alias-circular` (4, 1 file): circular type-alias imports are `*ERROR*`;
  `type-aliases.php` (4).
- `type-alias-import` (4, 2 files): `@phpstan-import-type` aliases in enums;
  `enums-import-alias.php`, `type-aliases.php` (2).
- `value-of` (3, 2 files): `value-of<T>` on constants and generics;
  `value-of-generic.php`, `value-of.php`.
- `type-alias-invalid` (2, 1 file): an invalid type alias resolves to `mixed`;
  `invalid-type-aliases.php`.
- `key-of` (1, 1 file): `key-of<…>` on a constant array; `key-of.php`.

**C8** (2 mismatches, 1 file, 1 cause):

- `type-alias-global` (2, 1 file): a global type alias from the `typeAliases`
  setting resolves (`int|string`); it is `never` or a class of that name today;
  `type-aliases.php` (2).

**D2** (2 mismatches, 2 files, 1 cause):

- `var-tags` (2, 2 files): `@var` placement: above a `use` statement, and
  without a variable name above a `foreach`;
  `foreach/type-in-comment-no-variable-2.php`, `var-above-use.php`.

**D3** (1,669 mismatches, 318 files, 89 causes):

- `count-narrow` (217, 27 files): `count()` compared with a constant, a range or
  another `count()` narrows the array: a list to an N-element shape, a union of
  shapes to its matching members, a maybe-countable to its countable part, and
  to non-empty; `list-count.php` (79), `bug-11488.php`, `list-count2.php`,
  `narrow-tagged-union.php` (16), `count-maybe.php`, `count-type.php`,
  `bug-14301.php`, `bug-13705.php` (6), `bug-14314.php` (6), `bug-2142.php`,
  `bug-10264.php` (4), `bug-4558.php`, `bug-4700.php`, `sizeof-php8.php`,
  `sizeof.php`, `bug-10952.php`, `bug-11642.php`, `bug-13675.php`,
  `bug-13749.php` (2), `reset-after-non-empty-loop.php` (2),
  `strlen-int-range.php` (2), `array-append-count.php`, `bug-11716.php` (1),
  `bug-13111.php`, `bug-1861.php`, `bug-2648.php`, `bug-4499.php`.
- `list-write` (114, 15 files): offset writes and appends keep `list`,
  non-emptiness and `hasOffsetValue` (`$l[count($l) - 1] = v`,
  `$g[array_key_last($g)][] = v`, nested writes), and an append to `mixed` stays
  `mixed`; `bug-14245.php`, `bug-12274.php`, `bug-15245.php`, `list-type.php`
  (15), `bug-13857.php`, `array-keys-branches.php` (4), `bug-4016.php`,
  `bug-9131.php`, `assign-nested-arrays.php`, `bug-12447.php`, `bug-13789.php`,
  `bug-2112.php`, `bug-8087.php`, `has-offset-type-bug.php` (2),
  `bug-13270a.php` (1).
- `operator-ranges` (87, 11 files): arithmetic, `%` and bitwise operators on
  ranges, constants and numeric strings give PHPStan's ranges;
  `modulo-operator.php`, `bug-14654.php`, `bug-15243.php`, `bug-8803.php` (8),
  `bug-9384.php`, `mixed-to-number.php` (6), `bug-9724.php`, `bitwise-not.php`,
  `bug-4843.php`, `decimal-int-string.php` (1),
  `integer-range-finite-types-php-int-max.php`.
- `loose-eq-narrow` (78, 4 files): `==`/`!=` narrow strings and unions, keeping
  accessories; `equal-narrow.php`, `bug-12312.php`, `equal.php` (11),
  `bug-14047.php` (7).
- `closure-param-infer` (77, 13 files): an untyped closure parameter takes its
  type from the callable parameter it is passed to (a union of callables, a
  variadic, a named argument, a constructor, an attribute, an immediately
  invoked closure's argument); `closure-passed-to-union-of-callables.php`,
  `closure-argument-type.php`, `bug-9240-php7.php`, `bug-9240.php`,
  `closure-passed-to-type-node-callback-scope-php81.php`,
  `arrow-function-argument-type.php`, `bug-13253.php`, `bug-10671.php`,
  `bug-10671b.php`, `callable-in-union.php`, `closure-in-attribute.php`,
  `bug-9601.php`, `closure-inferred-typehint.php`.
- `strlen-narrow` (59, 7 files): `strlen()`/`mb_strlen()` compared, stored or
  matched against another `strlen()` narrow the string; `bug-13688.php`,
  `strlen-int-range.php` (20), `bug-13749.php` (6), `bug-10952b.php`,
  `non-falsy-string.php`, `bug-11035.php`, `bug-7949.php`.
- `assert-tags` (58, 19 files): `@phpstan-assert`, `-if-true` and `-if-false` on
  methods, static methods, constructors, first-class and string callables,
  inherited from a parent or interface, and with conditional, template,
  intersection, `key-of` and `empty` types; `assert-inheritance.php`,
  `bug-14249.php`, `bug-12376.php`, `bug-14705.php` (5),
  `assert-conditional.php`, `assert-methods.php`, `assert-this.php`,
  `bug-10040.php`, `assert-empty.php`, `assert-invariant.php`,
  `assert-method.php`, `assert-constructor.php`, `assert-intersected.php`,
  `bug-10037.php`, `bug-11441.php`, `bug-11472.php`, `bug-13282.php`,
  `bug-14108.php`, `bug-14108c.php`.
- `conditional-holders` (57, 28 files): a variable holding a condition's result,
  or assigned under a condition, narrows its sources when it is checked later
  (PHPStan's conditional expression holders); one is never applied outside its
  condition (`bug-14891.php`); `bug-10090.php`, `bug-4733.php`, `bug-7948.php`,
  `bug-10088.php` (4), `conditional-expr-narrowing-through-variable.php` (4),
  `bug-13054.php`, `bug-14178.php`, `bug-14211.php`, `bug-2816-2.php`,
  `bug-2816.php`, `bug-3024.php`, `bug-9519.php`, `bug-9752.php`,
  `dependent-variables-type-guard-same-as-type.php` (2), `bug-10482.php`,
  `bug-10843.php`, `bug-11870.php`, `bug-13591.php`, `bug-14455.php` (1),
  `bug-14774.php`, `bug-14878.php`, `bug-15226.php`, `bug-2980.php`,
  `bug-4215.php`, `bug-4326.php`, `bug-7008.php`,
  `conditional-non-empty-array.php`, `disjunction-holder-projection.php`.
- `string-fn-narrow` (52, 7 files): a string function's result compared with a
  constant narrows its argument (`strtolower($s) === 'x'`, `substr`, `strstr`,
  `strrchr`, `trim($s) !== ''`); `non-empty-string-strcasing-specifying.php`,
  `non-empty-string-strstr-specifying.php`,
  `non-empty-string-substr-specifying.php`, `bug-12973.php`,
  `non-empty-string-strrchr-specifying.php`, `bug-13709.php`, `bug-7607.php`.
- `casts` (44, 6 files): casts keep what the value is: `(object)` of an object
  keeps its class, `(string)` of an int range is a union of literals and of a
  positive int non-falsy, `(array)` of a non-empty list stays one; `casts.php`,
  `cast-to-numeric-string.php` (13), `bug-14753.php`, `bug-9208.php`,
  `range-to-string.php`, `deducted-types.php` (1).
- `callable-predicate` (42, 4 files): calling a closure or callable variable
  whose return is conditional on its parameter narrows the argument, and the
  call's truthiness is remembered; `callable-type-predicate.php` (39),
  `bug-12686.php`, `bug-2822.php`, `bug-3770.php`.
- `offset-narrow-memory` (32, 10 files): a narrowed offset is remembered
  (`isset($a['k']) && !is_string($a['k'])` throwing, then `$a['k'] ?? ''` is
  `string`); `bug-10644.php`, `negated-boolean-and-conditional-holders.php`,
  `boolean-and-conditional-holders-mixed-context.php`, `bug-15060.php` (3),
  `bug-7501.php`, `bug-10473.php`, `bug-14319.php`, `bug-6202.php`,
  `composer-array-bug.php` (1), `composer-non-empty-array-after-unset.php`.
- `concat-accessories` (30, 3 files): concatenation keeps the operands' case and
  non-falsy accessories; `bug-11129.php`, `bug-10863.php`, `bug-8568.php`.
- `mixed-narrow` (29, 1 file): `is_string()` with `&&`/`||` narrowing of
  `mixed`; `bug-6329.php`.
- `byref-closure-use` (27, 4 files): a by-reference `use` variable merges the
  closure's writes into the outer scope, with the exact shape and without
  `never`; `bug-11561.php`, `closure-passed-by-reference.php` (8),
  `bug-13810.php` (5), `closure-passed-by-reference-in-call.php`.
- `isset-narrow` (26, 9 files): `isset()` makes an array non-empty, an offset
  `mixed~null`, a key `int<0, max>`, and a maybe-defined variable certain;
  `falsy-isset.php` (7), `has-offset-type-bug.php` (5), `bug-12933.php`,
  `bug-13270b-php8.php`, `bug-13674b.php` (3), `bug-8467b.php` (2),
  `bug-13270b.php`, `list-type.php` (1), `root-scope-maybe-defined.php`.
- `tagged-union-narrow` (26, 5 files): checking one offset narrows a union of
  shapes and their sibling offsets; `tagged-unions.php` (16), `bug-6728.php`,
  `bug-7921.php`, `bug-10544.php`, `bug-6383.php`.
- `array-plus` (22, 9 files): `+` and `+=` on shapes keep the known keys, their
  narrowed values and non-emptiness; `bug-4565.php`, `bug-10373.php`,
  `bug-14799.php`, `bug-6936-limit.php`, `constant-array-optional-set.php` (2),
  `bug-11912.php`, `bug-5584.php`, `bug-7492.php`, `count-recursive.php` (1).
- `identical-fold` (22, 2 files): `[1] === [1]` folds;
  `constant-array-type-identical.php`, `bug-4650.php` (2).
- `coalesce-narrow` (21, 9 files): `??` narrows its left side and the variable's
  certainty, on undefined variables, offsets, class constants and typed
  properties; `falsey-coalesce.php`, `bug-12125.php`, `bug-2420.php`,
  `bug-560.php`, `bug-7711.php`, `bug-10786.php`, `bug-11918.php`,
  `bug-5304.php`, `isset-property-default-value.php`.
- `or-narrow` (20, 6 files): `||` narrows each side in the other's false context
  and remembers both; `bug-12517.php`, `bug-8361.php`, `bug-7259.php`,
  `bug-3385.php`, `bug-9995.php`, `phpunit-integration.php` (1).
- `destructure` (19, 1 file): destructuring reads each element's type: nested
  (`[[$x]] = $a`), in a foreach, with a dynamic key and from an iterable
  (`mixed` today), and an offset target on an object makes it `ArrayAccess&…`;
  `array-destructuring.php` (19).
- `int-compare-narrow` (19, 5 files): integer comparisons narrow to ranges
  across `||` and later branches; `bug-6672.php`, `bug-10088.php` (4),
  `bug-4695.php`, `bug-1014.php`, `bug-9400.php`.
- `static-call-union` (17, 5 files): a static call on a union of classes or
  class-strings merges the returns; `union-intersection.php` (11),
  `bug-11687.php`, `method-call-return-type-fallback.php`,
  `static-late-binding.php` (1), `union-methods.php`.
- `unset-shape` (17, 7 files): `unset()` of a shape key removes it, a
  conditional or non-constant one makes keys optional;
  `filter-var-dynamic-return-type-extension-regression.php`,
  `array-is-list-unset.php` (2), `array-offset-unset.php`, `bug-8225.php`,
  `list-type.php` (2), `unset-conditional-expressions.php`,
  `native-expressions.php`.
- `isset-key-narrow` (16, 1 file): `isset($a[$k])` narrows the key: `mixed` to
  `mixed~(array|object|resource)`, a `string` on an int-keyed array to
  `decimal-int-string`, and against a constant array to its keys as PHP casts
  them (`1|2|'1'|'2'|float|true`); `bug-11716.php` (16).
- `ternary-narrow` (16, 4 files): a ternary condition narrows its arms, and a
  ternary in a condition narrows like `&&`; `ternary-specified-types.php`,
  `bug-15307.php`, `bug-987.php`, `bug-6174.php`.
- `string-offsets` (15, 1 file): `$s[$i]` on a constant string is the union of
  its characters; `string-offsets.php`.
- `assign-chain-narrow` (14, 1 file): `$a = $b = $c` links the three, so
  checking one narrows all; `multi-assign.php` (14).
- `cast-narrow` (14, 3 files): `(bool)`, `(int)` and `(string)` of an expression
  in a condition narrow the operand; `narrow-cast.php` (12), `bug-10528.php`,
  `bug-7685.php`.
- `fcc-static` (14, 1 file): first-class callables of static methods, and static
  calls through unions; `static-late-binding.php` (14).
- `offset-write-unknown-key` (14, 2 files): a write with a non-constant key into
  a constant array keeps the known elements; `constant-array-type-set.php`,
  `array-search-offset-argument-count.php`.
- `array-param-narrow` (13, 1 file): `is_string()` over an array's keys narrows
  it to `array<string, mixed>`; `bug-7076.php`.
- `arrayaccess-offset` (12, 3 files): an `ArrayAccess` offset reads
  `offsetGet()`'s return, and `isset` on it adds `hasOffset`; `bug-15021.php`
  (7), `bug-6399.php`, `bug-13204.php`.
- `closure-return-infer` (12, 5 files): a closure's return is inferred: `never`
  when it always throws, `void` without a `return`, and its return type when a
  variable holding it is called; `closure-return-type.php` (5), `bug-15167.php`
  (3), `missing-closure-native-return-typehint.php` (2), `bug-10283.php`,
  `mixed-typehint.php`.
- `enum-this-narrow` (12, 1 file): `$this !== self::A` narrows an enum's
  `$this`, and a conditional return follows it; `bug-12244.php`.
- `fcc-return` (12, 3 files): calling a first-class callable returns the
  method's return type and keeps its signature;
  `native-types-first-class-callables.php`, `bug-14362.php`,
  `intersection-template-mixed-method.php`.
- `increment-types` (12, 4 files): `++`/`--` on `null`, strings and floats give
  PHP's result types; `bug-10122.php`, `bug-1870.php`, `constantTypes.php` (1),
  `mixed-to-number.php` (1).
- `loose-eq-fold` (11, 3 files): PHP 8 loose comparisons between ints and
  non-numeric strings fold to `false`; `loose-comparisons-php8.php`,
  `bug-14793.php`, `equal.php` (2).
- `match-back-narrow` (11, 1 file): a `match` result compared back narrows the
  subject to the arms that produce it; `bug-14772.php`.
- `nullsafe-narrow` (11, 2 files): a truthy or compared nullsafe call makes the
  receiver non-null; `bug-4757.php`, `nullsafe-vs-scalar.php`.
- `and-narrow` (10, 2 files): `$a && $b` narrows both operands in either order
  and later null checks in the chain; `bug-14787.php`, `bug-8366.php`.
- `check-fold` (10, 3 files): a type check inside its own narrowing folds
  (`is_int($x)` is `true` in `if (is_int($x))`); `bug-8249.php`, `bug-12107.php`
  (1), `bug-2906.php`.
- `closure-use-narrow` (10, 1 file): the narrowed properties of a `use` variable
  carry into the closure; `specified-types-closure-use.php`.
- `float-constants` (10, 2 files): `-1 * INF` is `-INF`; `bug-12225.php` (7),
  `bug-13097.php`.
- `interpolation` (10, 3 files): interpolation and concatenation of constant
  unions and ranges give literal unions, `"$int"` is `decimal-int-string`;
  `constant-string-unions.php`, `bug-13378.php`, `bug-8635.php`.
- `is-fns-narrow` (10, 4 files): the `is_*` functions on `mixed`, unions and
  PHPDoc-typed strings; `bug-3133.php`, `specifiedTypesUsingIsFunctions.php`
  (3), `has-offset-type-bug.php` (2), `bug-1233.php` (1).
- `literal-key-writes` (10, 5 files): a write with a literal-union key builds a
  shape with optional keys; `bug-11716.php` (4), `array-keys-branches.php` (2),
  `bug-13805.php`, `set-constant-union-offset-on-constant-array.php`,
  `set-union-offset-preserve-constant-array.php`.
- `instanceof-intersect` (9, 2 files): `instanceof` of a subclass drops the
  redundant parent and narrows the variable an alias came from; `pr-5596.php`,
  `bug-6663.php`.
- `typed-property-coerce` (9, 3 files): a value assigned to a typed property is
  coerced (`1` into `float` is `1.0`, a `BcMath\Number` into `string`);
  `bug-12393.php`, `bug-12393-php84.php`, `bug-12393b-php84.php`.
- `unset-narrow` (9, 1 file): `unset()` or an overwrite of a key removes the old
  member from the union; `bug-14566.php`.
- `append-shape` (8, 4 files): appends in a branch to a constant list give an
  optional key or a union of shapes (`array{0: 'foo', 1: 'bar', 2?: 'baz'}`),
  and to `[]` the exact shape; `array-is-list-unset.php` (3),
  `array-keys-branches.php` (2), `constant-array-optional-set.php` (2),
  `count-recursive.php` (1).
- `float-narrow` (8, 1 file): `$f === 0.0` and `$f == 0` narrow a float, and
  `(string) 0.0` is `'-0'|'0'`; `bug-12225.php` (8).
- `match-narrow` (8, 5 files): a `match` arm narrows the subject (and an
  assigned subject variable) in its result, `match (true)` arms included;
  `bug-10084.php`, `bug-13385.php`, `bug-13385b.php`, `bug-8724.php`,
  `match-expression-inference.php`.
- `property-fetch-dynamic-name` (8, 2 files): `$obj->{'n'}` is the same
  expression as `$obj->n`; `bug-14847.php`, `bug-12398.php` (1).
- `reference-alias` (8, 2 files): a write through a reference updates the
  aliased variable; `bug-14275.php`, `bug-8056.php`.
- `isset-fold` (7, 4 files): `isset()` of an always-defined non-null variable is
  `true`; `bug-13561.php` (4), `array-pop-shift-has-offset.php` (1),
  `bug-13674b.php` (1), `isset-coalesce-empty-type-root.php` (1).
- `closure-generator` (6, 3 files): a closure or arrow function with `yield`
  returns `Generator<K, V, S, R>`; `closure-return-type.php` (3),
  `bug-7031.php`, `missing-closure-native-return-typehint.php` (1).
- `else-narrow` (6, 3 files): the else branch of a check removes the matched
  member (`?Foo` is `null`, a union of shapes loses the one with the key);
  `type-elimination.php`, `bug-12401.php`, `bug-5128.php`.
- `property-assign-narrow` (6, 3 files): a property just assigned keeps the
  assigned type, with and without `strict_types`; `bug-5129.php`,
  `remember-nullable-property-non-strict.php`,
  `remember-non-nullable-property-non-strict.php`.
- `self-identical` (6, 2 files): `$a !== $a` is `never` for a non-float type;
  `identical.php` (4), `equal.php` (2).
- `short-ternary` (6, 2 files): `$x ?: new A()` is the truthy part of `$x` or
  `A`; `bug-14705.php` (5), `bug-1897.php`.
- `coalesce-assign` (5, 2 files): `??=` on nested offsets writes the shape;
  `bug-15021.php` (4), `bug-optionalArrayKeyInMultiDimArrayLoop.php`.
- `dynamic-method-call` (5, 1 file): `$a->{$method}()` on a union resolves when
  every member has the method; `bug-12875.php`.
- `identical-transitive` (5, 1 file): `$c === $a && $c === $b` narrows `$a` and
  `$b` through `$c`; `constant-types-duplicate-condition.php`.
- `property-narrow` (5, 2 files): a property narrowed by an early return keeps
  its narrowing; `bug-6991.php`, `bug-12866.php`.
- `ternary-throw` (5, 1 file): a ternary with a `throw` arm narrows the other
  arm; `bug-6870.php`.
- `assert-ternary` (4, 1 file): `assert($c ? $x instanceof A : $x instanceof B)`
  narrows `$x` to `A|B`; `bug-14100.php`.
- `coalesce-throw` (4, 2 files): `$x ?? throw` makes `$x` non-null and an
  offset's key required after it; `bug-4885.php`, `bug-5351.php`.
- `constant-offset-fold` (4, 1 file): offsets of a class-constant array fold to
  exact shapes; `bug-7621-1.php`.
- `falsy-narrow` (4, 3 files): a falsy check narrows to the falsy members
  (`array{}|null`, `null`); `bug-2977.php`, `bug-10338.php`, `bug-3991.php` (1).
- `identical-narrow` (4, 3 files): `===`/`!==` against a constant narrows (`'0'
  !== $s` makes a non-empty string non-falsy); `identical.php` (2),
  `bug-3106.php`, `bug-7877.php` (1).
- `finite-types` (3, 1 file): branches over booleans keep a union of shapes;
  `finite-types.php`.
- `isset-constant-key` (3, 1 file): `assert(isset(self::KEYS[$k]))` narrows `$k`
  to the constant's keys; `bug-8559.php`.
- `property-isset-shape` (3, 2 files): `isset($o->p)` makes an object-shape
  property required; `object-shape.php` (2), `bug-15047.php`.
- `anonymous-class-this` (2, 1 file): `$this` inside an anonymous class's method
  is that class (`class@anonymous` today), so its PHPDoc-typed members resolve;
  `anonymous-class-name.php` (2).
- `coalesce-fold` (2, 1 file): `??` folds on a known left side (`'abc' ?? false`
  is `'abc'`) and removes `null` from a maybe-null `mixed`;
  `isset-coalesce-empty-type-root.php` (2).
- `destructure-key-assign` (2, 1 file): an assignment inside a destructuring key
  is evaluated (`[($a = 'foo') => $b] = …`); `bug-14019.php`.
- `empty-narrow` (2, 2 files): `!empty()` makes a union of shapes non-empty and
  subtracts `array{}` and `null` from `mixed`; `bug-3993.php`, `bug-9985.php`.
- `get-class-narrow` (2, 1 file): `get_class($o) === static::class` narrows `$o`
  to `static`; `get-class-static-class.php`.
- `nullsafe-static` (2, 1 file): `$a?->b::get()` is the return or `null`;
  `bug-nullsafe-prop-static-access.php`.
- `assert-or` (1, 1 file): `assert($s === null || $s !== '')` narrows `$s` to
  `non-empty-string|null`; `bug-6696.php`.
- `callable-offset-call` (1, 1 file): calling a closure read from a shape offset
  returns its return type; `bug-6633.php`.
- `callable-value-call` (1, 1 file): calling a value returns the callee's
  return: a closure variable's declared return, an object with `__invoke`,
  `[$this, 'm']`; `['C', 'm']` and `'C::m'` naming a non-static method are
  *ERROR* (`mixed` today); `bug-9764.php` (1).
- `constant-empty-fold` (1, 1 file): `!empty()` of an empty constant array is
  `never`; `bug-7913.php` (1).
- `float-const-fold` (1, 1 file): `$x = 10; $x /= 2` folds to `5`;
  `constantTypes.php` (1).
- `instanceof-narrow` (1, 1 file): `instanceof` on a union of interfaces removes
  the matched class in the else; `bug-13358.php`.
- `literal-narrow` (1, 1 file): a chain of `===` on a string builds a literal
  union, and removing a literal keeps the rest; `bug-10717.php`.
- `variable-variables` (1, 1 file): `$$name` with a constant name resolves;
  `bug-12398.php` (1).

**D5** (585 mismatches, 152 files, 25 causes):

- `loop-generalize` (208, 70 files): loop generalization: counters widen to
  ranges instead of unrolled constants, conditional appends make optional-key
  lists, nested writes keep non-empty optional-key shapes, and a loop's exit
  narrows; `bug-5309.php`, `for-loop-i-type.php` (11), `bug-12163.php`,
  `constant-array-optional-set.php` (9), `has-offset-type-bug.php` (8),
  `bug-14551.php`, `bug-8924.php`, `array-keys-branches.php` (6),
  `bug-14552.php`, `bug-2835.php`, `bug-9753.php`, `constantTypes.php` (6),
  `do-while-loop-variables.php`, `for-loop-variables.php` (5),
  `prestashop-breakdowns-empty-array.php`, `weird-array_key_exists-issue.php`,
  `bug-10089.php` (3), `bug-12078.php`, `bug-13786.php`, `bug-14297.php`,
  `bug-5017.php` (3), `bug-7115.php`, `bug-7978.php`, `bug-7996.php`,
  `bug-8015.php`, `issue-bot-array-regression.php`,
  `shopware-connection-profiler.php`, `while-loop-variables.php` (3),
  `bug-10025.php`, `bug-1021.php`, `bug-10438.php`, `bug-10650.php`,
  `bug-12665.php`, `bug-13637.php`, `bug-13647.php`, `bug-14084.php`,
  `bug-1516.php`, `bug-3558.php`, `bug-6173.php`, `bug-8520.php`,
  `bug-8827.php`, `bug-9784.php`,
  `dependent-variables-type-guard-same-as-type.php` (2),
  `myers-diff-loop-widening.php`, `pr-4390.php`, `bug-10640.php`,
  `bug-11854.php`, `bug-13039.php`, `bug-13509.php`, `bug-13705.php` (1),
  `bug-13730.php`, `bug-14083.php`, `bug-14553.php`, `bug-14814.php`,
  `bug-14980.php`, `bug-3269.php`, `bug-3915.php`, `bug-4907.php`,
  `bug-6070.php`, `bug-7000.php`, `bug-7153.php`, `bug-9004.php`,
  `bug-9332.php`, `bug-9904.php`, `bug7856.php`, `for-loop-expr.php`,
  `foreach-array-rewrite.php`, `generalize-scope-recursive.php`,
  `generalize-scope.php`, `loop-fixpoint-conditional-expressions.php`.
- `guards` (54, 8 files): a check on an impure-looking built-in
  (`function_exists`, `is_file`, `is_dir`, `file_exists`, `ftell`,
  `openssl_error_string`) is remembered until an impure call, closures included;
  `bug-13608.php`, `closure-retain-expression-types.php`,
  `clear-stat-cache.php`, `specified-function-call.php`, `bug-4816.php`,
  `bug-7106.php`, `bug-4970.php`, `filesystem-functions.php`.
- `ob-level` (47, 1 file): `ob_start()` and `ob_end_*()` move `ob_get_level()`;
  `output-buffering.php`.
- `impure-invalidate` (42, 9 files): an impure call invalidates what it could
  change: property narrowing after a void or impure method, remembered calls on
  an object passed to a call, superglobals; `impure-method.php`,
  `impure-constructor.php`, `do-not-remember-impure-functions.php`,
  `invalidate-object-argument-function.php`,
  `invalidate-object-argument-static.php`, `invalidate-object-argument.php`,
  `bug-12799.php`, `bug-4351.php`, `bug-8543.php`.
- `method-call-memory` (29, 11 files): a pure method call's narrowed result is
  remembered until an impure call on the object (`hasAttributes()` then
  `->attributes`); `bug-11200.php`,
  `remember-possibly-impure-function-values.php`, `bug-4177.php`,
  `bug-4231.php`, `bug-5501.php`, `DOMLegacyNamedNodeMap.php`,
  `bug-11200-lt8.php`, `bug-13076.php`, `bug-7210.php`,
  `enum-reflection-php82.php`, `specified-types-closure-edge.php`.
- `readonly-remember` (25, 5 files): a readonly property narrowed or assigned a
  constant in the constructor keeps it in other methods and hooks;
  `remember-readonly-constructor-narrowed.php`, `bug-12902.php`,
  `bug-12902-non-strict.php`,
  `remember-readonly-constructor-narrowed-hooks.php`, `bug-13321b.php`.
- `finally-scope` (21, 5 files): `finally` sees only the scopes that reach it,
  and an assignment that can't throw has completed before it;
  `finally-scope.php`, `finally-exit-point-scope.php`,
  `throw-points/assign-op.php`, `throw-points/assign.php`,
  `finally-with-early-termination.php`.
- `foreach-destructure` (20, 1 file): `foreach ($list as [$x, $y])` over shapes
  gives the elements' types (`mixed` today);
  `foreach-destructure-tagged-union.php`.
- `goto` (20, 6 files): a `goto` target merges the scopes that jump to it, and
  loops built from it widen; `goto-label-stabilization.php`,
  `goto-property-hook.php`, `bug-14660-top-level.php`, `bug-14660.php`,
  `bug-4674.php`, `bug-7734.php`.
- `switch-true` (18, 6 files): `switch (true)`/`switch (false)` cases narrow
  like `if`, with fallthrough, exits and unreachable cases; `bug-13211.php`,
  `bug-11064.php`, `switch-instanceof-truthy.php`, `switch-instanceof.php`,
  `switch-instanceof-fallthrough.php`, `switch-instanceof-not.php`.
- `iterator-key` (16, 9 files): foreach over an `Iterator`, `iterable<K, V>` or
  `SplObjectStorage` takes the key and value types; `bug-13985.php`,
  `foreach/object-type.php`,
  `foreach/foreach-iterable-with-specified-key-type.php`, `bug-4504.php` (1),
  `bug-4789.php`, `bug-6576.php`, `class-reflection-interfaces.php`,
  `date-period-return-types.php` (1),
  `foreach/foreach-iterable-with-complex-value-type.php`.
- `superglobals` (16, 2 files): `$argc`, `$argv` and `$GLOBALS` types;
  `cli-globals.php`, `superglobals.php`.
- `loop-body-narrow` (15, 6 files): inside `foreach`/`for` over an array, the
  array is non-empty; `bug-13312.php`, `bug-13270a.php` (3), `bug-5017.php` (1),
  `bug-5058.php`, `composer-treatPhpDocTypesAsCertainBug.php`,
  `foreach/foreach-with-specified-key-type.php`.
- `catch-scope` (10, 3 files): a `catch` block merges the scopes of every throw
  point in the `try`, a caught variable after the `try` is the union of the
  caught types, and a catch without a variable defines none;
  `throw-points/try-catch.php`, `bug-778.php`, `catch-without-variable.php`.
- `closure-invalidate` (10, 1 file): a closure that calls an impure method
  invalidates property narrowing where it is called; `bug-10566.php`.
- `foreach-narrow-all` (7, 1 file): returning on `!is_string($x)` for every
  element narrows the list to `list<string>`; `bug-5755.php`.
- `for-cond-entry` (6, 2 files): a `for` condition is evaluated on the initial
  values: false there makes the body unreachable, true there runs it at least
  once, so what it assigns is defined after the loop; `for-loop-i-type.php` (3),
  `for-loop-variables.php` (3).
- `foreach-key-value-link` (6, 2 files): narrowing a foreach key over a shape
  narrows the value, and the reverse; `foreach-dependent-key-value.php`,
  `bug-6108.php`.
- `foreach-iterable-part` (3, 1 file): `foreach` over a union with a
  non-iterable member iterates the iterable part;
  `foreach-partially-non-iterable.php`.
- `oversized-array` (3, 2 files): appends keep exact shapes up to the size
  limit; `oversized-array-stages.php`, `bug-yield-oversized-self-rejection.php`.
- `short-circuit-increment` (3, 1 file): `$i++ < 10` on the right of `&&` in a
  `while` condition counts, so `$i` is `int<1, 10>` in the body (`0` today);
  `while-loop-variables.php` (3).
- `foreach-cast-shape` (2, 1 file): `foreach` over `(array)` of a shape offset,
  narrowed by `isset` and `===`; `bug-13061.php`.
- `switch-get-class` (2, 1 file): `switch (get_class($x))` cases narrow `$x`;
  `switch-get-class.php`.
- `exit-narrow` (1, 1 file): `die()` on `null` makes the value non-null after;
  `bug-10422.php`.
- `switch-narrow` (1, 1 file): `switch` cases on a call narrow the value in
  later cases; `switch-type-elimination.php`.

**E2** (1,498 mismatches, 274 files, 87 causes):

- `byref-array-fns` (210, 24 files): the by-reference array functions
  (`array_pop`, `array_shift`, `array_push`, `array_unshift`, `array_splice`,
  the sort family, `usort`) rewrite their argument's type, keeping `list`,
  non-emptiness, `hasOffsetValue` facts, exact shapes and a template's `T&`
  bound; `array_splice.php`, `array-pop-shift-has-offset.php` (16),
  `array-push.php`, `array-unshift.php`, `bug-14633.php`, `bug-14631.php`,
  `bug-13510.php`, `bug-11846.php`, `bug-5017.php` (6), `bug-14472.php`,
  `scope-generalization.php`, `bug-14037.php`, `bug-4743.php`,
  `discussion-13395.php`, `bug-10627.php` (1), `bug-11322.php`, `bug-11917.php`,
  `bug-12954.php`, `bug-3548.php`, `bug-3875.php`, `bug-5783.php`,
  `constant-array-optional-set.php` (1), `constant-array-union-unshift.php`,
  `count-const-array-2.php`.
- `preg-shapes` (155, 24 files): the preg `$matches` shapes: `preg_match_all`,
  flags (`/n`, offset capture, unmatched as null), group 0 non-falsy when the
  pattern has literals, `> 0`/`=== 1` making groups required, patterns from
  constants, named and optional groups; `preg_match_all_shapes.php`,
  `bug-11311.php`, `bug-14314.php` (12), `preg_match_shapes_php82.php`,
  `bug-11293.php`, `bug-14820.php`, `bug-14575.php`,
  `preg_replace_callback_shapes.php`, `bug-11580.php`, `bug-12210.php`,
  `bug-14809.php`, `preg_match_shapes_php80.php`, `bug-12126.php`,
  `bug-11699.php`, `bug-13088.php`, `by-ref-writeback-native-type.php`,
  `bug-11820.php`, `bug-12211.php`, `bug-12242.php`, `bug-14605.php`,
  `bug-14791.php`, `bug11384.php`,
  `conditional-expr-narrowing-through-variable.php` (1),
  `preg_replace_callback_shapes-php72.php`.
- `key-exists-fold` (153, 29 files): `array_key_exists()`/`key_exists()` fold on
  constant arrays, narrow `mixed` to an array, make an optional key required,
  remove a key from the key type in the else, and make the array non-empty;
  `array-key-exists.php`, `key-exists.php`, `bug-13301-php8.php`,
  `bug-9461.php`, `bug-11276.php`, `bug-12620.php`, `conditional-vars-php8.php`,
  `conditional-vars.php`, `bug-2001-php8.php`, `bug-2001.php`, `bug-7805.php`,
  `bug-3009.php`, `bug-11518-types-php7.php`, `bug-11518-types.php`,
  `composer-array-bug.php` (3), `bug-11276b.php`, `bug-11724.php`,
  `bug-13301.php`, `bug-14874.php`, `bug-8917.php` (2), `bug13674a.php`,
  `array-key-exists-on-subtracted.php` (1), `bug-14705.php` (1), `bug-2869.php`,
  `bug-4099-php8.php` (1), `bug-4099.php` (1),
  `conditional-array-key-exists.php`, `list-type.php` (1),
  `session-get-cookie-params-php85.php` (1).
- `in-array-haystack` (65, 11 files): `in_array()` narrows the haystack
  (non-empty, a subtracted value, enum cases removed) and, loose, the needle to
  the haystack's values; `bug-9662.php`, `bug-9662-enums.php`, `bug-3013.php`,
  `enum-in-array.php` (7), `in_array_loose.php`, `in-array-enum.php` (3),
  `bug-8864.php`, `enum-vs-in-array.php`, `in-array-non-empty.php`,
  `bug-10201.php`, `bug-14407.php`.
- `ext:FilterVarArray` (60, 2 files): `filter_var_array()` and
  `filter_input_array()` with a definition give a shape, never `null` on PHP
  8.6; `filter-var-array.php`, `filter-var-array-dynamic-php86.php`.
- `ext:ArrayMap` (42, 14 files): `array_map()` gives the callback each element
  type (several arrays, enum cases, `[class, method]` callables) and keeps the
  keys, list and shape; `bug-14649.php`, `arrow-function-types.php`,
  `bug-4587.php`, `closure-types.php` (5), `array-map.php`,
  `array_map_multiple.php`, `bug-10264.php` (3), `bug-11281.php`,
  `bug-11861.php`, `bug-4213.php`, `bug-10442.php`, `bug-11656.php`,
  `bug-14844.php`, `bug-5293.php`.
- `ext:Randomizer` (40, 3 files): `Random\Randomizer`'s methods keep
  non-emptiness and element types; `bug-15256.php`, `bug-15256-83.php`,
  `bug-15256-shadowed-functions.php`.
- `ext:PgDml` (35, 2 files): `pg_insert()`, `pg_update()`, `pg_delete()` and
  `pg_select()` return types (no extension today); `pg-dml-resource.php`,
  `pg-dml.php`.
- `ext:VarExport` (30, 2 files): `var_export()`, `print_r()` and `highlight_*()`
  without `return: true` are `null`/`true`; `pr-1244-php-84.php`, `pr-1244.php`.
- `array-is-list` (29, 6 files): `array_is_list()` narrows `mixed` and arrays to
  `list`, folds on constant arrays, and is `false` on a shape with a required
  string key; `bug-9734.php` (10), `array-is-list-unset.php` (6),
  `array-is-list-type-specifying.php` (5), `bug-14938.php` (5),
  `array-is-list-offset.php`, `bug-10627.php` (1).
- `ext:CallUserFunc` (28, 4 files): `call_user_func()` and
  `call_user_func_array()` resolve the callee's return, generic and with named
  arguments; `call-user-func-php8.php`, `call-user-func-array-php8.php`,
  `call-user-func_array.php`, `call-user-func-php7.php`.
- `str-contains` (27, 1 file): `str_contains()`/`str_starts_with()` with a
  non-empty needle narrow the haystack;
  `non-empty-string-str-containing-fns.php` (27).
- `ctype-narrow` (26, 4 files): `ctype_*()` narrows a string to non-falsy and
  `ctype_digit()` to `numeric-string` (not `decimal-int-string`), an int to its
  ranges, and the operand of a `(string)` cast; `ctype.php`,
  `callsite-cast-narrowing.php`, `ctype-digit.php`, `bug-14792.php`.
- `ext:FilterVar` (25, 2 files): `filter_var()` through a variable and with
  `FILTER_REQUIRE_ARRAY`; `filter-var-returns-non-empty-string.php`,
  `discussion-9134.php`.
- `ext:JsonDecode` (25, 4 files): `json_decode()` of a constant folds, an
  invalid constant is `null`, and `JSON_OBJECT_AS_ARRAY` removes `object`;
  `json-decode/narrow_type.php`, `json-decode/narrow_type_with_force_array.php`,
  `json-decode/invalid_type.php`, `json-decode/json_object_as_array.php`.
- `is-callable-fold` (25, 3 files): `is_callable()` of a first-class callable
  and of a constant `[class, method]` folds; `bug-13872.php`,
  `bug-5782b-php7.php`, `bug-5782b-php8.php`.
- `callback-params` (24, 1 file): `array_udiff()`/`array_uintersect()` callbacks
  get the arrays' value types; `array_diff_intersect_callbacks.php`.
- `ext:ArrayFilter` (23, 12 files): `array_filter()` with a function-name,
  closure or arrow-function predicate narrows the values, makes keys optional or
  the result `array{}`, and without a callback removes falsy values;
  `array-filter-php7.php`, `array-filter-arrow-functions.php` (3),
  `array-filter-callables.php` (3), `array-filter-php8.php`, `bug-3132.php`,
  `array-filter-string-callables.php` (1), `bug-10189.php`, `bug-13421.php`,
  `bug-2273.php`, `bug-6006.php`, `bug-7909.php`, `bug-8373.php`.
- `ext:ClosureBind` (22, 4 files): `Closure::bind()` and `bindTo()` keep the
  closure's signature, and a `null` scope makes `$this` `*ERROR*`;
  `bug-5009.php`, `closure-static-type.php` (8), `closure-types.php` (2),
  `closure-bind-nullable-this.php`.
- `ext:ReflectionGetAttributes` (22, 2 files): `getAttributes()` is a
  `list<ReflectionAttribute<T>>`; `reflectionclass-issue-5511-php8.php`,
  `bug-14466.php`.
- `ext:StrReplace` (22, 4 files): `str_replace()` keeps the subject's non-empty
  and case accessories and a list subject; `bug-9870.php`,
  `non-empty-string-replace-functions.php`, `lowercase-string-replace.php`,
  `uppercase-string-replace.php`.
- `is-subclass-narrow` (20, 5 files): `is_subclass_of()` narrows to
  `class-string<T>` and is `never` on a final class;
  `specifiedTypesUsingIsFunctions.php` (14), `bug-6687.php`, `bug-6305.php`,
  `bug-6698.php`, `is-subclass-of.php`.
- `ext:ArrayKeys` (19, 5 files): `array_keys()` of a constant array is a shape,
  with a search value keeps literals, and after `array_key_exists()` is
  non-empty; `bug-11928.php`, `bug-13377.php`, `array_keys-php7.php`,
  `array_keys.php`, `bug-6859.php`.
- `preg-narrow` (19, 2 files): a truthy `preg_match()` narrows the subject to
  non-empty or non-falsy; `bug-14710.php`, `bug-14784.php`.
- `in-array-fold` (18, 4 files): `in_array()` folds to `true` or `false` on
  constant needles and haystacks of other types; `bug-14873.php`,
  `in-array-enum.php` (4), `in-array.php`, `bug-5668.php`.
- `array-search-narrow` (17, 5 files): `array_search() !== false` and
  `array_find_key() !== null` make the haystack non-empty, a truthy result makes
  `mixed` an array, and the else branch subtracts it;
  `array-search-type-specifying.php`, `array-search-existing.php`,
  `array-find-key-existing.php`, `array-search-php7.php`,
  `array-search-php8.php`.
- `key-first-last-fold` (15, 3 files): `array_key_first()`/`array_key_last()`
  `!== null` narrow emptiness, and a write at the last key keeps the list;
  `bug-13546.php`, `array-key-last-existing.php`, `bug-7905.php`.
- `ext:ArrayMerge` (14, 4 files): `array_merge()` of shapes and arrays keeps the
  known keys, non-empty-string keys and spread shapes; `bug-14526.php`,
  `bug-6927.php`, `array-merge-const-non-const.php` (3), `array-merge.php`.
- `ext:DateFunctionReturn` (14, 2 files): `date()` with a constant format is
  `''`, non-falsy or `numeric-string`; `date-format.php`, `bug-6613.php`.
- `ext:FilterInputArray` (13, 1 file): `filter_input_array()` with a definition
  array returns a shape; `filter-input-array.php`.
- `ext:BackedEnumFrom` (11, 1 file): `from()`/`tryFrom()` with a constant
  resolve to the case or `null`; `enum-from.php`.
- `ext:MbFunctions` (10, 2 files): `mb_detect_order()`'s return by its
  arguments; `bug-8922-php74.php`, `bug-8922.php`.
- `ext:OpensslEncryptParameterOut` (10, 1 file): `openssl_encrypt()`'s tag
  out-parameter depends on the cipher; `openssl-encrypt.php`.
- `ext:StrCaseFunctions` (10, 1 file): `str_replace()` and `preg_replace()` keep
  the subject's case and non-empty accessories; `replaceFunctions.php`.
- `byref-callback` (9, 1 file): `array_walk()`'s by-reference callback rewrites
  the element types; `bug-14525.php`.
- `ext:Count` (9, 1 file): `count()` and `sizeof()` of non-empty arrays and with
  `COUNT_RECURSIVE`; `count-recursive.php` (9).
- `ext:DateIntervalFormat` (9, 1 file): `DateInterval::format()` takes its
  accessories from the format; `date-interval-format.php`.
- `ext:Getenv` (9, 3 files): `getenv(null)` is `array<string, string>`,
  `string|false` before PHP 8; `getenv-php74.php`, `getenv-php80.php`,
  `bug-4538.php`.
- `ext:JsonLastError` (9, 1 file): `json_last_error()` is a literal range, `0`
  after a successful decode; `bug-2863.php`.
- `is-a-narrow` (9, 3 files): `is_a()` with `allow_string` narrows a string to
  `class-string<A>` (an object today) and removes it in the else; `is-a.php`
  (5), `bug-6404.php`, `bug-6704.php`.
- `simplexml` (9, 3 files): `SimpleXMLElement` iteration and offset access;
  `simplexml.php`, `has-offset-type-bug.php` (2), `bug-9575.php`.
- `ext:DateIntervalCreateFromDateString` (8, 2 files):
  `DateInterval::createFromDateString()` of a constant folds, and an invalid one
  is `never` on PHP 8.3; `bug-8442.php`, `bug-8442-83.php`.
- `ext:DateTimeModify` (8, 2 files): `modify()` is `DateTime|false` for a
  non-constant string and `never` for an invalid constant on PHP 8.3;
  `DateTimeModifyReturnTypes.php`, `DateTimeModifyReturnTypes83.php`.
- `get-parent-class-narrow` (8, 1 file): `get_parent_class($x) === A::class`
  narrows `$x` to `A|class-string<A>`; `bug-5552.php`.
- `ext:DomCreateElement` (7, 1 file): `createElement()` with a valid constant
  name can't be `false`; `dom-document-create-element.php`.
- `ext:Explode` (7, 5 files): `explode()` with constant arguments folds, and
  with a limit gives a shape; `array-destructuring.php` (3), `bug-13629.php`,
  `bug-14336.php`, `bug-14651.php`, `explode.php`.
- `ext:PdoConnect` (7, 1 file): `PDO::connect()` returns the driver's subclass;
  `pdo-connect-php84.php`.
- `ext:Sprintf` (7, 1 file): `sprintf('%s', $x)` keeps the argument's
  accessories; `bug-11201.php`.
- `enum-cases` (6, 3 files): `::cases()` on an enum class-string or template is
  a list of its cases; `bug-7162.php`, `bug-11233.php`, `bug-6695.php`.
- `ext:ArrayValues` (6, 3 files): `array_values()` keeps element types, shapes
  and non-emptiness; `bug-14489.php`, `array_values-php7.php`,
  `array_values.php` (1).
- `settype` (6, 1 file): `settype($x, $type)` rewrites the variable's type;
  `bug-3250.php`.
- `ext:ArrayColumn` (5, 4 files): `array_column()` of objects is a list, of a
  missing key `array{}`, and with a `null` value keeps the rows; `bug-8917.php`
  (2), `bug-6497.php`, `bug-6682.php`, `bug-9204.php`.
- `ext:ClosureFromCallable` (5, 1 file): `Closure::fromCallable()` keeps the
  callable's signature; `closure-return-type-extensions.php`.
- `method-exists-narrow` (5, 3 files): `method_exists()` narrows a literal
  string to `class-string&hasMethod(…)`; `this-subtractable.php` (3),
  `bug-8752.php`, `object-shape.php` (1).
- `enum-exists-narrow` (4, 2 files): `enum_exists()` narrows a string to
  `class-string<UnitEnum>`; `enum_exists.php`, `generic-enum-class-string.php`.
- `ext:ArrayIntersectKey` (4, 4 files): `array_intersect_key()` keeps the first
  array's key type and, with a constant array, gives an optional-key shape;
  `array-intersect-key-constant.php`, `array-intersect-key-php7.php`,
  `array-intersect-key-php8.php`, `bug-14747.php`.
- `ext:ArrayReplace` (4, 2 files): `array_replace()` of shapes keeps the known
  keys; `array-replace-const-non-const.php` (3), `bug-12828.php`.
- `ext:MysqliFetchObject` (4, 1 file): `fetch_object()` without a class is
  `stdClass`; `mysqli_fetch_object.php`.
- `ext:PregMatch` (4, 2 files): `preg_match()` returns `0|1|false`;
  `preg_match_php7.php`, `preg_match_php8.php`.
- `ext:Strtotime` (4, 1 file): `strtotime()` of a constant folds;
  `strtotime-return-type-extensions.php`.
- `class-exists-fold` (3, 1 file): `class_exists()` on an interface's name is
  `false`; `bug-14683.php` (3).
- `ext:DateFormat` (3, 2 files): `DateTime::format()` with a constant format
  gives literal unions and numeric accessories; `bug-10893.php`, `bug-8803.php`
  (1).
- `ext:PregReplace` (3, 1 file): `preg_replace()` on a shape keeps its keys;
  `bug-11547.php`.
- `property-exists-narrow` (3, 2 files): `property_exists()` narrows to
  `object&hasProperty(…)`; `object-shape.php` (2), `bug-13304.php`.
- `closure-get-current` (2, 1 file): `Closure::getCurrent()` outside a closure
  is `never`; `closure-get-current.php`.
- `constant-string-fold` (2, 1 file): a method returning a template string from
  a `match` folds to its literals; `class-name-usage-location.php`.
- `ext:ArrayChunk` (2, 1 file): `array_chunk()` of a constant array with a large
  size is one shape; `bug-8956.php`.
- `ext:ArrayCountValues` (2, 1 file): `array_count_values()` is `array<key,
  int<1, max>>`; `bug-14223.php`.
- `ext:ArrayFlip` (2, 2 files): `array_flip()` of `mixed` narrowed to an array;
  `array-flip-php7.php`, `array-flip-php8.php`.
- `ext:GetClass` (2, 2 files): `get_class()` is `class-string`, `|false` on
  `mixed`; `bug-3158.php`, `bug-4741.php`.
- `ext:ParseUrl` (2, 1 file): `parse_url()` with a non-constant component;
  `bug-4754.php`.
- `ext:PdoFetchObject` (2, 1 file): `fetchObject(Bar::class)` returns
  `Bar|false`; `PDOStatement.php`.
- `ext:Reset` (2, 2 files): `reset()` on a possibly empty array can be `false`;
  `bug-7928.php`, `reset-after-non-empty-loop.php` (1).
- `ext:StrIncrement` (2, 1 file): `str_increment()` of an invalid constant is
  `never`; `bug-13481.php`.
- `is-a-fold` (2, 1 file): `is_a()` between class-strings that can't relate is
  `false`; `bug-4371.php`.
- `is-callable-narrow` (2, 1 file): `is_callable([$x, 'm'])` narrows `$x` to an
  object or class-string; `bug-4573.php`.
- `strpos-fold` (2, 1 file): `strpos(…) === -1` is `false`;
  `extra-int-types.php`.
- `ext:ArrayCombine` (1, 1 file): `array_combine()` of constant arrays builds a
  shape; `array-combine-php8.php`.
- `ext:ArrayFillKeys` (1, 1 file): `array_fill_keys()` with object keys casts
  them to `string`; `bug-2471.php`.
- `ext:End` (1, 1 file): `end()` appended to a nested array;
  `array-destructuring-types.php`.
- `ext:GetParentClass` (1, 1 file): `get_parent_class()` without arguments
  folds; `get-parent-class.php`.
- `ext:IniGet` (1, 1 file): `ini_get()` of a known key is `string`;
  `ini-get.php`.
- `ext:IteratorToArray` (1, 1 file): `iterator_to_array()` keeps the value type;
  `bug-7185.php`.
- `ext:MinMax` (1, 1 file): `min()`/`max()` of a range and a constant, and after
  `count() <= 0`; `bug-10089.php` (1).
- `ext:TriggerError` (1, 1 file): `trigger_error()` with `E_USER_ERROR` is
  `never`; `bug-5992.php`.
- `http-response-header` (1, 1 file): `file_get_contents()` on a URL defines
  `$http_response_header`; `http-response-header.php`.
- `json-validate` (1, 1 file): `json_validate()` narrows its argument to
  `non-empty-string`; `json-validate.php`.

**Right at runtime** (6 mismatches, 4 files, 3 causes):

- `array-union-merge` (4, 2 files): phpstan-rs keeps a union of arrays unmerged
  (`array{a: int, b: string}|array{a: string, b: string}`,
  `array{}|non-empty-list<…>`), the same values as PHPStan's merged type; an
  approved deviation; `bug-14558.php`, `tagged-unions.php` (2).
- `coalesce-null-right` (1, 1 file): after `assert(($a[1] ?? null) === null)`,
  `$a[1]` reads `null` whether or not the key exists (a missing key reads `null`
  with a warning); PHPStan keeps `object|null`; an approved deviation;
  `bug-13214.php`.
- `shadowed-trait-right` (1, 1 file): a trait method the class shadows is
  analysed anyway, so `$a` is `1`; PHPStan skips the body and the fixture
  asserts a sentinel; an approved deviation; `shadowed-trait-methods.php`.

## Appendix I: the rules oracle's unexplained phpstan-rs-only findings by cause

At `76e738b4`, the rules oracle's 2,588 phpstan-rs-only findings split three
ways: 1,125 are explained by the plan item that cites their file, 1,019 are in
410 files the plan cites nowhere else, and 444 are in 105 cited files whose
citing item doesn't explain them (each cited file's findings were checked
against its item's text). The last two sets are listed here. Each finding was
read against its fixture, the phar's findings on the same line and, where the
two tools disagree on runtime behaviour, php 8.5; they trace to 255 causes. By
verdict: 860 are wrong today (a false positive, or a wrong type that leads to
one), 188 are a finding PHPStan reports under another identifier or phpstan-rs
reports twice, 70 are `phpstanRs.assertType` mismatches, 124 are right at
runtime and approved, 43 come from bleeding-edge toggles C1 turns into settings,
and 178 are the oracle's setup (php-parser classes, an unused trait, the
measured run's single-int `phpVersion`) or an approved rename. Each entry gives
the cause's key, its findings and files, its verdict, what holds, and the files,
most findings first; the phase is done with it when its files have no
phpstan-rs-only findings left, or each remaining one is approved.

**Oracle setup** (167 findings, 19 files, 3 causes):

- `phpparser-classes` (154 findings, 14 files; the oracle setup or an approved
  rename): php-parser and PHPStan classes the phar loads and phpstan-rs doesn't
  (the gate's excluded files); `self-contradiction.php`,
  `TestMethodTypeSpecifyingExtensions.php`, `rule-error-signature.php`,
  `return-list-rule.php`, `return-rule-error.php`,
  `TestTypeOverwriteSpecifyingExtensions.php`,
  `class-implements-out-of-phpstan.php`, `node-connecting-visitor.php`,
  `method-signature.php`, `class-implements-in-phpstan.php`,
  `instanceof-type.php`, `old-php-parser-4-class.php`,
  `runtime-reflection-function.php`, `runtime-reflection-instantiation.php`.
- `oracle-phpversion` (11 findings, 3 files; the oracle setup or an approved
  rename): the measured run gave the phar `phpVersion: 80500` as one int (gate
  12's spec gives the range), which leaves `PHP_VERSION_ID` unbounded;
  `final-private-method-phpversions.php`, `named-arguments-phpversion.php`,
  `disallow-named-arguments-php-version-scope.php`.
- `unused-trait-body` (2 findings, 2 files; the oracle setup or an approved
  rename): PHPStan doesn't analyse a trait no class uses; phpstan-rs does
  (approved); `ignore-parse-error-trait.php`, `bug-5143.php`.

**B3** (11 findings, 5 files, 2 causes):

- `assign-in-coalesce` (7 findings, 1 file; wrong today): `$a ?? $x = null`
  parses as `$a ?? ($x = null)`; `assign.invalidExpr`, `variable.undefined` and
  `return.type` today (B3's assignment-as-operand item);
  `unnecessary-null-coalesce-side-effects.php`.
- `shebang-declare` (4 findings, 4 files; wrong today): a shebang line before
  `<?php` doesn't make `declare(strict_types=1)` not first;
  `declare-shebang.php`, `declare-shebang2.php`, `declare-shebang3.php`,
  `bug-6252.php`.

**B4** (105 findings, 41 files, 19 causes):

- `template-iterable` (27 findings, 5 files; wrong today): `foreach` over
  `iterable<T>` gives `T` (`mixed` today, B4's `iterable<K, V>` item);
  `bug-7511.php`, `bug-7460.php`, `bug-8936.php`, `bug-10297.php`, `yield.php`.
- `assert:print-keys` (15 findings, 5 files; an assertType mismatch): `array<V>`
  prints without its `(int|string)` key, the nsrt normalization's form;
  `bug-13921.php`, `file-asserts.php`, `discussion-7004.php`, `bug-5372_2.php`,
  `inherit-phpdoc-return-type-with-narrower-native-return-type.php`.
- `template-property` (14 findings, 7 files; wrong today): a property holding a
  `T` (assigned, promoted, or `@var E|null` with `?? throw`) returns `T`;
  `generic-subtype.php`, `bug-13440.php`, `yield.php`, `bug-4590.php`,
  `template-string-bound.php`, `bug-14138.php`, `bug-13190.php`.
- `template-infer-keys` (7 findings, 2 files; wrong today; an assertType
  mismatch): `array<K, V>` arguments infer `K`, and `new Collection([0 => …])`
  infers `Collection<int, …>`; `bug-3769.php`, `bug-11041.php`.
- `template-generalize` (6 findings, 1 file; an assertType mismatch): `new
  Collection(['foo', 'bar'])` generalizes to `Collection<int, string>`;
  `bug-5372.php`.
- `template-offset` (6 findings, 3 files; wrong today): an offset read on
  `array<TKey, T>` or `non-empty-list<T>` is `T`; `bug-6158.php`,
  `bug-4117.php`, `bug-7600.php`.
- `assert:print-template-bound` (5 findings, 2 files; an assertType mismatch):
  templates print with their bound (`T of X (…)`), nested generics included;
  `bug-9708.php`, `bug-5562.php`.
- `callable-template-infer` (5 findings, 5 files; wrong today; an assertType
  mismatch): a callable argument infers a template from its parameters and
  return: `Closure(): T`, `__invoke` with a callable template,
  `callable(callable(): T): T`, a templated callable property called through
  `($this->cb)($x)`; `bug-14844-class-const.php`, `bug-7984.php`,
  `bug-9167.php`, `varying-acceptor.php`, `bug-13876.php`.
- `template-bound-template` (5 findings, 1 file; wrong today): a method template
  bounded by a class template (`F of E`) accepts the bound; `bug-4643.php`.
- `assert:iterable-template` (2 findings, 1 file; an assertType mismatch):
  `iterable<array<T>>` from a generic helper prints as `iterable`, `never` for
  an empty array; `bug-5757.php`.
- `conditional-return` (2 findings, 1 file; an assertType mismatch): conditional
  returns over callable templates resolve; `conditional-complex-templates.php`.
- `conditional-return-callable` (2 findings, 1 file; wrong today): a first-class
  callable of a method with a conditional return resolves it per element in
  `array_map()`; `bug-10715.php`.
- `mixed-accepts` (2 findings, 1 file; wrong today): `mixed|null` accepts
  `mixed` (`return.type` today); `?mixed` is reported once;
  `invalid-union-with-mixed.php`.
- `sealed-generic` (2 findings, 1 file; wrong today): `match` on `$x::class`
  over a `@phpstan-sealed` generic class's subclasses is exhaustive;
  `bug-14412.php`.
- `allowed-subtypes` (1 finding, 1 file; wrong today): `DateTimeInterface` after
  `!instanceof DateTimeImmutable` is `DateTime`; `bug-5259.php`.
- `shape-dynamic-key` (1 finding, 1 file; wrong today): `[$name => $value]` is
  `non-empty-array<string, mixed>`; `bug-4844.php`.
- `shape-optional-accepts` (1 finding, 1 file; wrong today): a constant array
  missing an optional key is accepted by `array{url?: …, title: …}`
  (`classConstant.phpDocType` today); `bug-7273b.php`.
- `stringable-to-string` (1 finding, 1 file; wrong today): without
  `strict_types`, a `Stringable` object is accepted by a `string` parameter;
  `stringable.php`.
- `template-narrow` (1 finding, 1 file; wrong today): `!instanceof C` on `T of
  A|B|C` leaves `A|B`, which has `$name`; `bug-6566.php`.

**C4** (65 findings, 31 files, 22 causes):

- `method-tag-static-trait` (8 findings, 1 file; wrong today): static `@method`
  tags declared on a trait or parent resolve on the class;
  `bug-11591-method-tag.php`.
- `trait-duplicate-method` (8 findings, 2 files; another identifier or a second
  report): methods differing only in case are
  `class.duplicateMethod`/`trait.duplicateMethod`, not `method.nameCase`;
  `bug-14250.php`, `duplicate-declarations.php`.
- `enum-interface-methods` (6 findings, 1 file; wrong today): an enum implements
  `cases`/`from`/`tryFrom` natively, so an interface declaring them is satisfied
  (`class.abstractMethodNotImplemented` today); `bug-11592.php`.
- `mixin-trait` (5 findings, 3 files; wrong today): `@mixin` on a trait applies
  to the classes using it (as well as being `mixin.trait`); `trait-mixin.php`,
  `trait-mixin.php`, `mixin-trait-use.php`.
- `require-extends-members` (5 findings, 1 file; wrong today):
  `@phpstan-require-extends` on an interface makes the required class's members
  accessible through it; `require-extends.php`.
- `define-constant-ctor` (4 findings, 1 file; wrong today): a constant
  `define()`d in the constructor is known in the class's other methods, with its
  value's type (`constant.notFound` on each read today);
  `remembered-constructor-scope.php`.
- `trait-templates` (4 findings, 3 files; wrong today): `@use
  Trait<X>`/`@phpstan-use` resolves a trait's method `@return` and property
  `@var` templates; `php-82-dynamic-properties-allow.php`, `bug-9657.php`,
  `php-82-dynamic-properties.php`.
- `inherited-property-phpdoc` (3 findings, 2 files; wrong today): a property
  redeclared without PHPDoc keeps the parent's type; `bug-7240.php`,
  `bug-1216.php`.
- `mixin-callstatic` (3 findings, 2 files; wrong today): with `@mixin` and
  `__callStatic`, `Class::method()` of an instance method goes through
  `__callStatic`; `bug-3641.php`, `bug-5536.php`.
- `trait-alias` (3 findings, 3 files; wrong today): trait adaptations: `foo as
  public` changes visibility and `foo as bar` adds `bar`; `bug-3465.php`,
  `bug-3514.php`, `bug-4516.php`.
- `define-constant-type` (2 findings, 1 file; wrong today): a constant
  `define()`d in the constructor has its value's type in other methods (`mixed`
  today); `remembered-constructor-scope.php`.
- `define-constants` (2 findings, 1 file; wrong today): a constant `define()`d
  in the file is known everywhere in it; `constants.php`.
- `trait-abstract-impl` (2 findings, 1 file; wrong today): an abstract method
  from one trait implemented by another trait in the same class is implemented;
  `bug-3406_2.php`.
- `trait-method-shadowed` (2 findings, 1 file; wrong today): a trait constructor
  the class overrides isn't checked in that class;
  `incompatible-default-parameter-type-trait-crash.php`.
- `abstract-trait-method-overridden` (1 finding, 1 file; wrong today): an
  abstract trait method the class implements isn't `missingType.return`;
  `bug-4214.php`.
- `const-expr-enum-value` (1 finding, 1 file; an assertType mismatch):
  `A::X->value` in a constant expression is the case's value (`mixed` today);
  `bug-8957.php`.
- `generic-parent-params` (1 finding, 1 file; wrong today): an implementing
  method's untyped parameter takes `@implements IProcessor<string>`'s type;
  `bug-8880.php`.
- `inherited-param-phpdoc` (1 finding, 1 file; wrong today): an untyped
  parameter inherits the parent's `@param`, so it isn't `missingType.parameter`;
  `method-signature-trait.php`.
- `inherited-stub-param` (1 finding, 1 file; another identifier or a second
  report): an overriding method's untyped parameter inherits the stub's `array`;
  PHPStan reports `missingType.parameter`; `memcache-pool-get.php`.
- `property-tag-inside-class` (1 finding, 1 file; wrong today): inside the
  class, a native property wins over a child's `@property` tag; `bug-13537.php`.
- `property-tag-private` (1 finding, 1 file; wrong today): a `@property` tag
  over a private property makes it readable outside (through `__get`);
  `private-property-with-allowed-property-tag-is-public.php`.
- `self-const-value` (1 finding, 1 file; wrong today): `self::LOREM` reads the
  constant's value (`self` isn't late-bound), so `@var int` on `IPSUM =
  self::LOREM` holds where phpstan-rs uses `LOREM`'s `@var string|int`;
  `value-assigned-to-class-constant.php`.

**C6** (144 findings, 23 files, 12 causes):

- `printf-values-dup` (47 findings, 2 files; wrong today; another identifier or
  a second report): `printf`'s `...$values` is `bool|float|int|string|null` with
  `__stringAndStringable` (functionMap), so the stub's `string|int|float` adds a
  second `argument.type` per value; `printf-param-types.php`,
  `unpack-operator.php`.
- `curl-init-benevolent` (41 findings, 7 files; wrong today): `curl_init()`'s
  `CurlHandle|false` is benevolent in PHPStan; `curl_setopt.php`,
  `curl_setopt_callback.php`, `bug-13862.php`, `curl-setopt-array.php`,
  `curl_setopt_prereq.php`, `bug-13862b.php`, `bug-15185.php`.
- `functionmap-variants` (26 findings, 3 files; wrong today): named arguments
  select a functionMap variant (`setcookie`'s `$expires_or_options`,
  `session_set_save_handler`'s handler, `stream_context_set_option`'s options),
  and an override of `PDO::query()` is checked against the native signature, not
  a variant's `$ctorargs`; `call-to-function-named-params-multivariant.php`,
  `bug-10171.php`, `bug-5436.php`.
- `iterator-key-type` (5 findings, 1 file; an assertType mismatch):
  `ArrayIterator::key()` and friends over a typed array are its key type;
  `infer-array-key.php`.
- `datePeriod-ctor-variants` (4 findings, 1 file; wrong today): `DatePeriod`'s
  constructor has two signatures; named `start`/`interval`/`end` pick the first
  (`argument.missing` `$isostr` today); `bug-5553.php`.
- `file-return` (4 findings, 1 file; wrong today): `file()` is
  `list<string>|false`; after the `false` check, `foreach` gives `string`;
  `bug-9328.php`.
- `pdo-benevolent` (4 findings, 1 file; wrong today): `PDO::prepare()`/`query()`
  are benevolent `PDOStatement|false` in PHPStan; `bug-14719.php`.
- `stub-signature` (4 findings, 2 files; wrong today): stub parameters narrower
  than PHP's: `strpos`'s `$offset` is `int` (negative counts from the end),
  `time_nanosleep`'s are `int`; `bug-8097.php`,
  `function-call-statement-no-side-effects-error-throws.php`.
- `dom-benevolent` (3 findings, 2 files; wrong today):
  `DOMDocument::createElement()` and `DOMElement::getAttributeNode()` are
  benevolent `…|false` in PHPStan; `dom-legacy-ext-template-nodes.php`,
  `bug-6213.php`.
- `stream-get-contents-benevolent` (3 findings, 1 file; wrong today):
  `stream_get_contents()` is benevolent `string|false` in PHPStan;
  `bug-3396.php`.
- `reflection-enum-cases` (2 findings, 1 file; wrong today):
  `ReflectionEnum<BackedEnum>::getCases()` gives `ReflectionEnumBackedCase` with
  `getBackingValue()`; `unused-method-false-positive-with-trait.php`.
- `static-return-stub` (1 finding, 1 file; wrong today):
  `CustomDateTimeImmutable::createFromFormat()` is `static|false`;
  `bug-7225.php`.

**C7** (96 findings, 20 files, 14 causes):

- `benevolent-keyword` (22 findings, 3 files; wrong today): `__benevolent<…>` in
  a PHPDoc is a type, not a class name; `binary-op-benevolent-union.php`,
  `invalid-inc-dec-union.php`, `unary-union.php`.
- `phpdoc-hyphen-key` (13 findings, 2 files; wrong today): a shape key with a
  hyphen (`psr-4?:`) is `phpDoc.parseError` today (C7's parser item), so the
  `@var` or alias is lost and `foreach.emptyArray`, `offsetAccess.notFound`,
  `class.notFound` follow; `bug-6000.php`, `bug-8467a.php`.
- `type-alias-circular` (13 findings, 2 files; another identifier or a second
  report): a circular or invalid type alias is
  `typeAlias.circular`/`typeAlias.invalidType`, not `class.notFound` on its
  name; `local-type-aliases.php`, `local-type-trait-aliases.php`.
- `class-string-param-type` (11 findings, 1 file; wrong today):
  `class-string<UnitEnum>` in a namespace resolves `UnitEnum` globally, so
  `$enum::from()` is the enum; `bug-14557-function.php`.
- `pure-callable` (11 findings, 2 files; wrong today): `pure-callable` and
  `pure-Closure` are PHPDoc types, not class names; `pure-callable-accepts.php`,
  `missing-method-parameter-typehint.php`.
- `array-union-phpdoc` (7 findings, 2 files; wrong today): `array|int[]` and
  `self[]|Collection|array` give `array<int>`/`array<self>`, so the value type
  is specified; `properties-assigned-types.php`,
  `missing-function-parameter-typehint.php`.
- `template-type-keyword` (5 findings, 1 file; wrong today): `template-type<T,
  Interface, 'TValue'>` is a PHPDoc type, not a class name; `bug-13474.php`.
- `class-named-like-keyword` (3 findings, 1 file; wrong today): a class named
  `The` or `Scalar` resolves as the class; `scalar-class-name.php`.
- `iterable-phpdoc` (3 findings, 1 file; wrong today): `iterable<Foo>` on a
  native `iterable` is its value type, and `array<Bar>|Collection` is a subtype
  of `iterable`; `call-methods-iterable.php`.
- `key-of-const` (2 findings, 1 file; wrong today): `key-of<self::ALL>` over a
  constant keyed by class constants resolves (`mixed` today; C7's `key-of`
  item); `call-methods.php`.
- `phpdoc-keywords` (2 findings, 1 file; wrong today): `non-decimal-int-string`
  is a PHPDoc type, not a class name; `unsealed-array-shapes-has-offset.php`.
- `phpdoc-precedence` (2 findings, 1 file; wrong today): a PHPDoc union
  containing an intersection parses with `&` binding tighter; `bug-6243.php`.
- `const-type-in-shape` (1 finding, 1 file; wrong today): `Foo::TEST` inside a
  `@psalm-param` shape resolves; `bug-3723.php`.
- `value-of-template` (1 finding, 1 file; wrong today): `?value-of<T>` for `T of
  BackedEnum` is `int|string|null`, not `mixed`; `bug-13638.php`.

**C9** (46 findings, 18 files, 4 causes):

- `user-function-purity` (15 findings, 5 files; wrong today): a call to a user
  function or method is judged by its body, its purity tags (a class's
  `@phpstan-all-methods-impure` included) and its callback arguments (named or
  shuffled included), and a call to a closure with a by-reference parameter is
  impure: a pure one is silent, or `*.resultUnused` when discarded, and an
  impure one `impure.functionCall`/`impure.methodCall`, where today each is
  `possiblyImpure.*` "unknown"; `pure-unless-callable-is-impure-named-arg.php`,
  `pure-unless-callable-is-impure.php`, `pure-function.php`,
  `all-methods-are-pure.php`, `bug-11361-pure.php`.
- `builtin-purity` (14 findings, 7 files; wrong today): built-ins and enum
  methods are pure: `is_int`, `is_string`, `is_numeric`, `is_array`, `empty`,
  `array_key_exists`, `tryFrom()` (functionMetadata); `bug-11207.php`,
  `bug-12224.php`, `pure-method.php`, `bug-12224.php`, `bug-13201.php`,
  `bug-14511-method.php`, `bug-14511.php`.
- `ctor-purity` (10 findings, 4 files; wrong today): a user class's constructor
  purity is known (promoted-only constructors are pure); `possiblyImpure.new`
  "unknown class" today; `pure-method.php`,
  `pure-unless-callable-is-impure.php`, `possibly-impure-instanceof-tip.php`,
  `bug-14138-pure.php`.
- `method-purity` (7 findings, 2 files; wrong today): a method calling an impure
  function or method is impure (`method.resultUnused` today);
  `call-to-static-method-without-impure-points.php`,
  `call-to-method-without-impure-points.php`.

**D2** (11 findings, 5 files, 1 cause):

- `var-tags` (11 findings, 5 files; wrong today): an `@var` naming a variable
  the next statement uses types or defines it (`$x = $v`, `$a->{$foo} = …`);
  `unset-false-key.php`, `bug-8280.php`, `bug-7531.php`, `bug-10861.php`,
  `discussion-13274.php`.

**D3** (298 findings, 143 files, 52 causes):

- `byref-out-undefined` (73 findings, 23 files; wrong today): an undefined
  variable passed to an out parameter is defined by the call and not checked
  against the parameter first: `preg_match`'s `$matches` (13 files),
  `preg_match_all`, `preg_replace_callback`'s `$count`, `parse_str`'s `$result`,
  user `&$out = null` parameters and static methods; `defined-variables.php`,
  `bug-6551.php`, `constant-parameter-check.php`, `bug-9503.php`,
  `boolean-op-truthy-falsey.php`, `bug-7806.php`, `bug-11276.php`,
  `bug-8356.php`, `bug-10418.php`, `bug-5326.php`, `bug-8142.php`,
  `bug-14097.php`, `bug-5365.php`, `bug-11602.php`, `bug-12406b.php`,
  `bug-14270.php`, `always-true-preg-match.php`, `bug-11908.php`,
  `bug-7491.php`, `bug-12397.php`, `bug-12015.php`, `bug-11617.php`,
  `preg-match-offset-capture-isset.php`.
- `const-offset-nonliteral` (24 findings, 11 files; wrong today; an assertType
  mismatch): a constant or literal array read with a non-literal key is the
  union of its values: class constants, `array_rand()` keys, `'a_' . $d`, `??
  null` on a literal-typed static array (`mixed` today); `bug-6633.php`,
  `bug-10577.php`, `bug-12981.php`, `bug-4747.php`, `nonexistent-offset.php`,
  `bug-7898.php`, `slevomat-cs-in-array.php`, `bug-10610.php`, `bug-3920.php`,
  `bug-3530.php`, `bug-7933.php`.
- `new-class-string` (18 findings, 10 files; wrong today): `new $c()` for
  `class-string<T>` is `T`, `static` for `static::class`, and after
  `is_subclass_of($c, A::class)` it is `A` (`mixed` today); `bug-4806.php`,
  `bug-6370.php`, `bug-4552.php`, `bug-7574.php`, `bug-5369.php`,
  `bug-13190-template-generic.php`, `bug-12940.php`, `bug-3120.php`,
  `bug-4795.php`, `bug-5591.php`.
- `conditional-holders` (16 findings, 6 files; wrong today): a variable defined
  under a condition is certain when a correlated condition holds (`!$b` then
  `!$a && !$b`, `in_array($t, [A, B])` then `$t === A`, `count($row) === N`, the
  same superglobal offset); `bug-8430.php`, `bug-8430b.php`, `bug-12992.php`,
  `bug-12597.php`, `bug-4173.php`, `bug-5805.php`.
- `infer-expr-gaps` (13 findings, 9 files; wrong today): `infer_expr` has no
  `instanceof` arm and gives `mixed` for a short ternary (`$s ?: ''`), so
  `return $o instanceof X`, `match ($x instanceof Y)` and `$a ?: [...]` are
  `mixed`; `bug-9991.php`, `bug-12827-instanceof.php`, `bug-6349.php`,
  `bug-10214.php`, `bug-6910.php`, `bug-4188.php`, `returnTypes.php`,
  `bug-5164.php`, `bug-5382.php`.
- `magic-constants` (13 findings, 6 files; wrong today; an assertType mismatch):
  `__FILE__` is `mixed` today; `__FUNCTION__`/`__METHOD__` in a closure are
  `'{closure}'`; `file.php`, `bug-anonymous-function-method-constant.php`,
  `compiler-halt-offset-const-defined.php`,
  `compiler-halt-offset-const-not-defined.php`, `bug-12850.php`,
  `recursive-iterator-iterator.php`.
- `match-exhaustive` (13 findings, 8 files; wrong today): `match (true)` is
  exhaustive when its arms' narrowings cover the subject: `instanceof` over a
  union, `===` over literals or enum cases, `is_string`/`is_array`, every
  combination of two bools; `bug-8042.php`, `bug-13303.php`,
  `strict-comparison-last-match-arm.php`, `bug-7721.php`, `bug-8937.php`,
  `bug-unhandled-true-with-complex-condition.php`,
  `match-always-true-last-arm.php`, `bug-13210.php`.
- `assert:offset-writes` (9 findings, 1 file; an assertType mismatch): nested
  offset writes and `isset` narrowing on `array<array<mixed>>`; `bug-8113.php`.
- `append-byref-arg` (8 findings, 3 files; wrong today): an offset or append
  passed by reference creates it (`hi($array['hi'])`, `set($array[])`), where
  today it is `offsetAccess.notFound` or `mixed`; `bug-5290.php`,
  `bug-2313.php`, `bug-2634.php`.
- `loop-offset-narrow` (8 findings, 1 file; wrong today): narrowing
  `$record[$col]` over a constant-key loop proves each key `int`;
  `bug-13643.php`.
- `offset-write-scalar-union` (8 findings, 2 files; wrong today): an offset
  write on a union with an array keeps only the array (and `false`, which
  autovivifies), not `offsetAssign.dimType`; `bug-3391.php`, `bug-7417.php`.
- `assert:mixed-offset` (6 findings, 1 file; wrong today): offsets of a
  PHPDoc-typed `array<int, SassValue>` argument are `mixed` today;
  `bug-10822.php`.
- `class-const-narrow` (6 findings, 1 file; wrong today): `$x::class ===
  Final::class` narrows `$x` to the final class (`method.notFound` today), and
  `!instanceof` leaves `Interface~Class`; `bug-9542.php`.
- `list-write` (6 findings, 3 files; wrong today): offset writes over foreach
  keys keep nested lists, and conditional appends then `!$a` returning make
  `$a[0]` exist; `bug-13797.php`, `bug-14124.php`, `bug-14124b.php`.
- `closure-generator` (5 findings, 4 files; wrong today): a closure or arrow
  function with `yield` is a `Generator`, called or invoked at once (`mixed`
  today); `bug-1517.php`, `arrow-functions-return-type.php`, `bug-6464.php`,
  `tooWideArrowFunctionReturnType.php`.
- `iife-byref-use` (5 findings, 2 files; wrong today): an immediately invoked
  closure's by-reference and `$this` writes reach the caller (`if.alwaysFalse`
  today); `bug-8926.php`, `bug-4864.php`.
- `offset-narrow-dynamic-key` (5 findings, 4 files; wrong today): a narrowed or
  assigned offset with variable keys stays narrowed (`is_null($a[$n - 1])` then
  an assignment, `!isset(self::$s[$id][$k])` throwing); `bug-1903.php`,
  `bug-9494.php`, `deep-dim-fetch.php`, `bug-6398.php`.
- `coalesce-assign` (4 findings, 4 files; wrong today): `$x ??= new X()` is `X`,
  as an argument, on a property and in a get hook (`mixed` today);
  `bug-3636.php`, `bug-12735.php`, `bug-4525.php`, `bug-12553.php`.
- `coalesce-assign-static` (4 findings, 2 files; wrong today): `self::$p ??= …`
  is the property's type (`mixed` today); `bug-10924.php`, `bug-11949.php`.
- `count-array-shift` (4 findings, 1 file; wrong today): after `2 !==
  count(explode(…))` throws, `array_shift()` twice gives `string` (`string|null`
  today); `bug-12675.php`.
- `closure-param-infer` (3 findings, 3 files; wrong today): a callback's native
  `array` parameter is refined by the callable's parameter type
  (`preg_replace_callback`, `mb_ereg_replace_callback`); `bug-9240.php`,
  `mb_ereg_replace_callback.php`, `preg_replace_callback.php`.
- `elseif-exhausted` (3 findings, 3 files; wrong today): an `elseif` repeating
  checks the `if` already exhausted is on `*NEVER*` (`instanceof.alwaysFalse`),
  not `instanceof.alwaysTrue` on the intersection; `boolean-and.php`,
  `boolean-logical-and.php`, `if-condition.php`.
- `maybe-defined-type` (3 findings, 1 file; wrong today): a maybe-defined
  variable keeps its type (`Foo|null`), so `$a?->foo` isn't on `mixed`;
  `bug-7291.php`.
- `static-call-on-expr` (3 findings, 1 file; wrong today):
  `(static::class)::m()` resolves the static method (`mixed` today);
  `bug-12558.php`.
- `string-offset-read` (3 findings, 2 files; wrong today): `$s[$i]` on a string
  is a string (`mixed` today); `non-empty-string-impossible-type.php`,
  `bug-11506.php`.
- `anon-class-parent` (2 findings, 1 file; wrong today): `parent::m()` in an
  anonymous class resolves (`mixed` today); `bug-7640.php`.
- `assert-tags` (2 findings, 2 files; wrong today; an assertType mismatch):
  `@phpstan-assert` through a parent call (`parent::equals($other)`) and an
  impossible assertion making the argument `never`; `bug-8169.php`,
  `bug-11289.php`.
- `byref-arg-type` (2 findings, 2 files; wrong today): a defined variable passed
  by reference is checked with its value, not `mixed` (`$a = 'hello'; foo($a)`
  for `string &$i`; `$matches = []`); `bug-1953.php`, `bug-11708.php`.
- `coalesce-maybe-defined` (2 findings, 2 files; wrong today): `$x ?? null` on a
  maybe-defined `$x` is its type or `null` (`mixed` today); `bug-12179.php`,
  `bug-4337.php`.
- `get-class-match-narrow` (2 findings, 1 file; wrong today): `match
  (get_class($obj))` arms narrow `$obj` to that class; `bug-8536.php`.
- `isset-mixed-offset` (2 findings, 1 file; wrong today): `isset($config[$key])`
  on `mixed` narrows `$config` to an offset-accessible type; `bug-7229.php`.
- `key-exists-offset` (2 findings, 2 files; wrong today): after
  `array_key_exists($k, $a)` with a non-literal key, `$a[$k]` is the union of
  the values (`mixed` today); `bug-13526.php`, `bug-7763.php`.
- `string-offset-write` (2 findings, 1 file; wrong today): `$string[$k] = $v` on
  `''` in a loop; the string isn't `''` after it; `bug-3803.php`.
- `array-literal-key-order` (1 finding, 1 file; wrong today): array-literal keys
  are evaluated in order: `[$idx++ => 'test', $idx++ => 'presto']` has keys 0
  and 1 (`array.duplicateKey` "2" today); `duplicate-keys.php`.
- `arrayaccess-offset-set` (1 finding, 1 file; approved, right at runtime):
  `$data['k'] = 1` against `offsetSet()`'s `@param string`, PHPDoc-only, which
  PHPStan doesn't check; `bug-2268.php`.
- `byref-out-input` (1 finding, 1 file; wrong today): `openssl_sign()`'s
  `$signature` is an out parameter; its input isn't checked;
  `incorrect-call-to-function.php`.
- `byref-static-call-classstring` (1 finding, 1 file; wrong today): a
  by-reference argument to `$classString::m()` is written (`if.alwaysFalse`
  today); `bug-5020.php`.
- `callable-offset-call` (1 finding, 1 file; wrong today): calling a closure
  read from an array offset after `isset` returns its return type;
  `bug-9614.php`.
- `casts` (1 finding, 1 file; wrong today): `(object)` of an array is `stdClass`
  (`mixed` today); `array_walk.php`.
- `count-narrow` (1 finding, 1 file; wrong today): `count($row) !== 1` on
  `array{mixed}|array{mixed, string|null, mixed}` leaves the three-element shape
  to destructure; `bug-11488.php`.
- `enum-case-subtract` (1 finding, 1 file; wrong today): `$foo === Foo::ONE`
  throwing leaves `Foo::TWO|Foo::THREE`; `bug-8846.php`.
- `function-exists-var` (1 finding, 1 file; wrong today): a variable holding
  `function_exists('f')` guards `f()` when it is checked; `bug-14384.php`.
- `implicit-mixed-property` (1 finding, 1 file; approved, right at runtime):
  `foreach` over an untyped public property's value is `mixed` (level 10's
  implicit-mixed check); `bug-6698.php`.
- `instanceof-class-string` (1 finding, 1 file; wrong today): `assert($x
  instanceof $className)` with `class-string<T>` makes `$x` `T`;
  `template-type-native-type-object.php`.
- `is-resource-closed` (1 finding, 1 file; wrong today): `is_resource()` on a
  `resource` is never always true (D3's soundness item);
  `check-type-function-call.php`.
- `isset-superglobal` (1 finding, 1 file; an assertType mismatch):
  `isset($_SERVER['X'])` narrows `$_SERVER` to `non-empty-array&hasOffset`;
  `narrow-superglobal.php`.
- `isset-union-key` (1 finding, 1 file; wrong today): `isset($a[$k])` with a
  literal-union key narrows `$a[$k]` for the same key; `bug-6508.php`.
- `match-result-correlation` (1 finding, 1 file; wrong today): `(match
  (get_class($x)) {…})->add($x)`: each arm's receiver pairs with the narrowed
  `$x`; `bug-7593.php`.
- `property-truthy-narrow` (1 finding, 1 file; wrong today): `if ($this->only)`
  narrows an untyped `?string` property to `non-empty-string` inside;
  `bug-6486.php`.
- `reference-alias` (1 finding, 1 file; wrong today): `$tmp = &$array; $tmp[] =
  'foo'` makes `$array` non-empty (`foreach.emptyArray` today); `bug-8056.php`.
- `template-null-narrow` (1 finding, 1 file; wrong today): after `if ($foo !==
  null && !$foo instanceof Foo) throw`, a template `$foo` may still be `null`,
  so `?->` isn't `nullsafe.neverNull`; `nullsafe-method-call-template-type.php`.
- `trait-property-exists` (1 finding, 1 file; wrong today):
  `property_exists($this, 'p')` in a trait makes `$this->p` `mixed`, not the
  property's type in one host; `bug-9095.php`.

**D5** (57 findings, 32 files, 19 causes):

- `switch-exhaustive` (13 findings, 7 files; wrong today): a `switch` over every
  value of a constant union, a PHPDoc union or an enum (fallthrough included)
  covers every path: variables are defined after it and `return.missing` isn't
  reported; `bug-8719.php`, `bug-12722.php`, `bug-1016b.php`, `bug-3488-2.php`,
  `bug-3488.php`, `bug-1016.php`, `defined-variables-enum.php`.
- `foreach-nonempty-defines` (12 findings, 2 files; wrong today): a `foreach`
  over a non-empty array defines its value variable after the loop;
  `foreach.php`, `bug-8467c.php`.
- `catch-scope` (3 findings, 1 file; wrong today): an assignment inside a
  constructor argument happens before the constructor can throw, so the `catch`
  sees it; `bug-14323.php`.
- `class-exists-remember` (3 findings, 2 files; wrong today):
  `class_exists()`/`function_exists()` checked in the constructor guard uses in
  other methods; `remember-class-exists-from-constructor.php`,
  `remember-function-exists-from-constructor.php`.
- `closure-invalidate` (3 findings, 2 files; wrong today): a closure passed to a
  call that may invoke it invalidates `$this->conn`'s narrowing;
  `bug-11417.php`, `bug-10903.php`.
- `constant-condition-certainty` (3 findings, 1 file; wrong today): a variable
  defined under `if (true)` is certain after it; `dead-branches.php`.
- `goto` (3 findings, 2 files; wrong today): a `goto` back to a label in a `try`
  runs `$attempt++` again, so `$attempt < 4` isn't always true; `bug-14638.php`,
  `bug-12167.php`.
- `loop-generalize` (3 findings, 3 files; wrong today; an assertType mismatch):
  loop generalization: optional-key lists, literal-keyed writes, and
  `array_shift` in a `foreach` widening a constant array; `bug-14308.php`,
  `bug-2851b.php`, `bug-12653.php`.
- `exhaustive-if-chain` (2 findings, 2 files; wrong today): an `if`/`elseif`
  chain over every member of a union (null, `instanceof A`, `instanceof B`;
  every `self::SORT_*`) covers every path; `bug-2875.php`, `bug-5266.php`.
- `infinite-loop-exit` (2 findings, 1 file; wrong today): `while (true)`/`for
  (;;)` left only by `break`: the break scopes define the variable;
  `bug-11545.php`.
- `loop-first-iteration` (2 findings, 1 file; wrong today): a variable defined
  on the loop's first iteration is defined on later ones; `bug-11218.php`.
- `catch-byref` (1 finding, 1 file; wrong today): a catch variable passed to a
  by-reference parameter with `@param-out int`;
  `parameter-out-catch-variable.php`.
- `dead-branch-analysed` (1 finding, 1 file; wrong today): a branch the
  condition makes unreachable (`if (!empty(X))` for `X = []`) isn't analysed;
  `bug-7913.php`.
- `dead-catch-return-missing` (1 finding, 1 file; wrong today): `try { return
  $this->i; } catch (…) {}` in a get hook: the backing read can't throw, so
  every path returns; `unthrown-exception-property-hooks.php`.
- `dead-loop-body` (1 finding, 1 file; approved, right at runtime): a call in a
  loop body that never runs (`for ($i = 5; $i < 4; …)`) names an undefined
  function; PHPStan skips unreachable code; `bug-3277.php`.
- `for-runs-once` (1 finding, 1 file; wrong today): a `for` loop whose first
  condition holds runs once, defining its variables;
  `defined-variables-anonymous-function-use.php`.
- `foreach-unroll` (1 finding, 1 file; wrong today): `foreach` over a constant
  shape writing `$r[$key]` builds `array{a: '1', b: '2'}`; `bug-13000.php`.
- `impure-invalidate` (1 finding, 1 file; wrong today): `$this->z()` may change
  `self::$x`, so `self::$x = []` no longer holds after it; `bug-3747.php`.
- `superglobals` (1 finding, 1 file; another identifier or a second report):
  top-level `$argv` is `variable.undefined` ("might not be defined") in PHPStan;
  `bug-4070.php`.

**E1** (74 findings, 41 files, 12 causes):

- `property-write-forms` (33 findings, 18 files; wrong today): offset writes,
  appends, `unset()` of an offset, by-reference arguments (`uksort`, `exec`,
  `proc_open`), references (`$a = &$this->p`, `$this->p = &$v`) are writes;
  `unused-private-property.php`, `passed-by-reference.php`, `uksort_arrow.php`,
  `readonly-assign-ref-phpdoc.php`, `offset-access-value-assignment.php`,
  `readonly-assign-ref.php`, `uninitialized-property.php`, `bug-2689.php`,
  `bug-8648.php`, `slevomat-foreach-unset-bug.php`, `bug-5935.php`,
  `bug-7251.php`, `bug-8781.php`, `bug-9361.php`, `param-out.php`,
  `bug-11777.php`, `bug-6356.php`, `readonly-assign-ref-phpdoc-and-native.php`.
- `private-set-never-written` (12 findings, 4 files; approved, right at
  runtime): a `private(set)` property never written (approved); `bug-12964.php`,
  `overriding-final-property.php`, `property-assign-ref-asymmetric.php`,
  `read-asymmetric-visibility.php`.
- `untyped-implicit-null` (12 findings, 5 files; wrong today): an untyped
  property is `null` until assigned, so its `@var`'s `null` isn't
  `property.unusedType`; `returnTypes.php`, `too-wide-property-type.php`,
  `bug-6230.php`, `bug-4526.php`, `bug-4680.php`.
- `duplicate-promoted` (4 findings, 2 files; another identifier or a second
  report): a property declared twice (a property and a promoted parameter) is
  the redeclaration error, not
  `property.unused`/`onlyWritten`/`missingType.property` on it;
  `duplicate-promoted-property.php`, `default-value-for-promoted-property.php`.
- `property-read-forms` (4 findings, 3 files; wrong today): `$this?->p` and a
  read after `is_null()` are reads; `strict-comparison.php`,
  `nullsafe-unused-private-property.php`, `access-properties-after-isnull.php`.
- `trait-member-reads` (3 findings, 3 files; wrong today): a used trait's method
  reading the host's property is a read; `bug-8850.php`,
  `private-property-trait.php`, `bug-6175.php`.
- `byref-hook-writes` (1 finding, 1 file; wrong today): a by-reference get hook
  lets callers write any type, so the property's type isn't too wide;
  `return-type-after-finally-property-hooks.php`.
- `const-use-invalid-access` (1 finding, 1 file; wrong today):
  `Bar::PRIVATE_BAR` read from the parent is a use, even though the read is
  `classConstant.private`; `class-constant-visibility.php`.
- `hook-property-unused` (1 finding, 1 file; another identifier or a second
  report): a hooked property never read is `property.neverRead`, not
  `property.unused`;
  `too-wide-throw-type-always-check-final-property-hooks.php`.
- `hook-property-virtual` (1 finding, 1 file; wrong today): a get-only hooked
  property has no backing store, so it isn't `property.onlyRead`;
  `bug-12702.php`.
- `promoted-only-written` (1 finding, 1 file; approved, right at runtime): a
  promoted property never read is write-only; right at runtime; `bug-6260.php`.
- `unused-ctor-param-shadow` (1 finding, 1 file; approved, right at runtime): a
  constructor parameter shadowed by a closure's parameter is unused; PHPStan
  counts the name; `unused-constructor-parameters.php`.

**E2** (52 findings, 27 files, 23 causes):

- `closure-bind-scope` (11 findings, 4 files; wrong today): `Closure::bind()`,
  `bindTo()` and `->call()` give the closure the new `$this` and the scope
  class's private access, arrow functions included; `closure-bind.php`,
  `arrow-function-bind.php`, `bug-4808.php`, `bug-4906.php`.
- `preg-byref-offset` (8 findings, 1 file; wrong today): `preg_match()` writing
  into `$matches[2]` (an offset out-parameter) extends the shape;
  `bug-11655.php`.
- `preg-callback-flags` (5 findings, 1 file; wrong today):
  `preg_replace_callback()`'s callback gets the `PREG_OFFSET_CAPTURE` shape and
  `$count` is defined; `bug-8284.php`.
- `array-combine-walk` (2 findings, 1 file; an assertType mismatch):
  `array_combine()` of a constant key list is non-empty with those keys, and
  `array_walk()` by reference rewrites an offset; `bug-7469.php`.
- `array-map-generator` (2 findings, 1 file; approved, right at runtime):
  `array_map()` over generator closures returned as `string[]`; right at
  runtime, PHPStan is silent; `bug-3891.php`.
- `byref-array-fns` (2 findings, 2 files; wrong today; an assertType mismatch):
  `array_push($a, ...$b)` pushes `$b`'s elements, and in a loop gives
  `list<int<0, max>>`; `bug-8449.php`, `bug-9403.php`.
- `ext:ArrayChangeKeyCase` (2 findings, 1 file; wrong today):
  `array_change_key_case()` on a shape changes the keys' case; today the keys
  stay, a wrong value; `bug-10960.php`.
- `ext:ArrayFilter` (2 findings, 1 file; wrong today): `array_filter()` with a
  first-class-callable predicate that asserts the value narrows the result;
  `bug-11337.php`.
- `ext:CallUserFunc` (2 findings, 1 file; wrong today): `call_user_func()` of a
  closure returns its return type; `vsprintf()`'s `$values` is reported twice;
  `bug-7057.php`.
- `key-exists-fold` (2 findings, 1 file; an assertType mismatch):
  `array_key_exists()` and `is_string()` narrow to
  `non-empty-array&hasOffsetValue`; `bug-7156.php`.
- `method-exists-invoke` (2 findings, 1 file; wrong today): `method_exists($o,
  '__invoke')` makes `$o` callable; `callables.php`.
- `array-search-narrow` (1 finding, 1 file; wrong today): `array_search(…) !==
  false` on `mixed` narrows it to an array; `bug-14312b.php`.
- `dom-create-element-validator` (1 finding, 1 file; approved, right at
  runtime): `createElement('')` throws at runtime (phpstan-rs's own validator);
  `bug-13792.php`.
- `ext:ArrayMerge` (1 finding, 1 file; wrong today): `array_merge($a, ['expr' =>
  …])` has the `'expr'` key; `bug-8438.php`.
- `ext:ArraySlice` (1 finding, 1 file; wrong today): `array_slice()` without
  `preserve_keys` is a list; `bug-12880.php`.
- `ext:End` (1 finding, 1 file; wrong today): `end()` of an array property after
  an append is its value type; `bug-10686.php`.
- `ext:FilterVar` (1 finding, 1 file; wrong today): `filter_var()` with a
  non-constant flags argument that may hold `FILTER_NULL_ON_FAILURE` returns
  `bool|null`; `bool` today, a wrong value; `bug-11485.php`.
- `ext:GetParentClass` (1 finding, 1 file; wrong today): `get_parent_class()` of
  an interface-typed object may be a class name; always false today, a wrong
  value; `bug-4302.php`.
- `ext:Sprintf` (1 finding, 1 file; wrong today): `sprintf()` with non-empty
  arguments is non-empty; `bug-11491.php`.
- `ext:Substr` (1 finding, 1 file; wrong today): `substr($s, 0, $pos + 1)` with
  `$pos` `int<0, max>` is non-empty; `bug-15258.php`.
- `key-exists-const` (1 finding, 1 file; wrong today): `array_key_exists('k',
  self::CONST)` guards `self::CONST['k']`, per host in a trait; `bug-4570.php`.
- `property-exists-guard` (1 finding, 1 file; wrong today):
  `property_exists(static::class, 'p')` guards `static::$p = …`;
  `bug-2861-assign.php`.
- `simplexml` (1 finding, 1 file; wrong today): `foreach` over
  `SimpleXMLElement::children()` gives `SimpleXMLElement`; `bug-3947.php`.

**E3** (328 findings, 114 files, 71 causes):

- `static-prop-statement` (79 findings, 6 files; approved, right at runtime): a
  static property read as a statement is `expr.resultUnused` (approved);
  `static::$instanceProp` is `property.staticAccess` in PHPStan;
  `access-static-properties.php`, `access-private-property-static.php`,
  `bug-8333.php`, `restricted-property.php`,
  `static-property-access-on-internal-subclass.php`, `bug-13042.php`.
- `fcc-purity` (19 findings, 1 file; wrong today): creating a first-class
  callable is pure, and calling one takes the callee's purity
  (`possiblyImpure.*` "unknown" today);
  `first-class-callable-pure-function.php`.
- `use-superglobal-cascade` (18 findings, 1 file; another identifier or a second
  report): a superglobal in a closure's `use` list is `closure.useSuperGlobal`
  (a compile error); the `argument.type` "mixed given" on reading it follows;
  `invalid-lexical-variables-in-closure-use.php`.
- `castable-to-number` (14 findings, 4 files; approved, right at runtime; a
  bleeding-edge toggle): `array_sum()`/`array_product()` of arrays or objects
  (`checkParameterCastableToNumberFunctions`, approved);
  `param-castable-to-number-functions.php`, `bug-13775-bis.php`,
  `bug-13775.php`, `param-castable-to-number-functions-named-args.php`.
- `invalid-binary-dup` (10 findings, 3 files; another identifier or a second
  report): an operation PHPStan reports as `binaryOp.invalid` is
  `expr.duplicateOperands`, `division.byZero`/`modulo.byZero`,
  `deadCode.unreachable` or `expr.resultUnused` in phpstan-rs;
  `invalid-binary.php`, `bug-5169.php`, `bug-7538.php`.
- `unresolvable-native` (10 findings, 5 files; another identifier or a second
  report): `int&mixed` is `*.unresolvableNativeType` once per declaration, not
  `intersectionType.int` per occurrence;
  `arrow-function-intersection-types.php`, `closure-intersection-types.php`,
  `intersection-types.php`, `intersection-types.php`, `intersection-types.php`.
- `finite-types-haystack` (9 findings, 1 file; a bleeding-edge toggle):
  `finiteTypesInHaystack`; `impossible-in-array-finite-types.php`.
- `abstract-rename` (8 findings, 7 files; the oracle setup or an approved
  rename): `class.abstractMethodNotImplemented` for `method.abstract` and
  `enum.duplicateConstant` for `enum.duplicateEnumCase`, the approved renames;
  `overriding-method.php`, `duplicate-enum-cases.php`,
  `invalid-promoted-properties.php`, `abstract-method.php`,
  `method-in-enum-without-body.php`, `missing-method-impl-enum.php`,
  `missing-method-impl.php`.
- `parent-outside-class` (8 findings, 3 files; another identifier or a second
  report): `parent` outside a class is `class.notFound`,
  `return.unresolvableType` in phpstan-rs; `class.nameCase` reported once more;
  `typehints.php`, `typehints.php`, `typehintsWithoutNamespace.php`.
- `unused-label` (8 findings, 2 files; a bleeding-edge toggle): `unusedLabel`;
  `goto-undefined-label.php`, `unused-label.php`.
- `abstract-with-wrong-parent` (7 findings, 2 files; another identifier or a
  second report): a class whose `@implements`/`@extends` names itself:
  `generics.wrongParent` for `generics.notCompatible` and one
  `class.abstractMethodNotImplemented` for PHPStan's `method.abstract` per
  method (approved renames); `class-ancestors-implements.php`,
  `class-ancestors-extends.php`.
- `new-non-object` (7 findings, 1 file; a bleeding-edge toggle):
  `newOnNonObject`; `instantiation-non-object.php`.
- `var-tag-native` (7 findings, 2 files; another identifier or a second report):
  an inline `@var` against the native type is `varTag.nativeType`; one above a
  destructuring or `foreach` isn't checked per target;
  `wrong-var-native-type.php`, `var-tag-destructuring-native.php`.
- `attribute-not-found-dup` (6 findings, 1 file; another identifier or a second
  report): `attribute.notFound` once per attribute, not twice;
  `class-constant-attribute.php`.
- `generic-iterable-classes` (6 findings, 1 file; wrong today):
  `missingType.generics` skips generic `Traversable` subtypes unless
  `checkGenericIterableClasses` (off by default); `bug-9981.php`.
- `date-interval-ctor` (5 findings, 1 file; a bleeding-edge toggle):
  `checkDateIntervalConstructor`; `date-interval-instantiation.php`.
- `static-access-instance` (5 findings, 2 files; another identifier or a second
  report): `self::$instanceProp` is `property.staticAccess` in PHPStan,
  `staticProperty.notFound` today; `bug-8668-bis.php`, `invalid-assign-var.php`.
- `undefined-var-dup` (5 findings, 3 files; another identifier or a second
  report): an undefined variable's offset read is `variable.undefined` only;
  `bug-1746.php`, `readonly-assign-phpdoc.php`, `readonly-assign.php`.
- `unset-rules` (5 findings, 1 file; another identifier or a second report):
  `unset()` on a scalar offset or an undefined variable is
  `unset.offset`/`unset.variable`; `unset.php`.
- `var-tag-unknown-class` (5 findings, 2 files; approved, right at runtime): an
  `@var` naming unknown classes above `$x ??= …`; right, PHPStan doesn't check
  that `@var`; `invalid-phpdoc.php`, `invalid-phpdoc-assign-operator.php`.
- `phpdoc-void-union` (4 findings, 1 file; wrong today): `@return int|void`
  allows a bare `return` and a missing return; `return null` is a type error
  only natively; `bug-11980-function.php`.
- `property-assign-by-ref` (4 findings, 1 file; another identifier or a second
  report): `&$o->private` is `property.assignByRef` where phpstan-rs says
  `property.private`; a readonly property only assigned by reference counts as
  written; `property-assign-ref.php`.
- `require-constant-path` (4 findings, 2 files; approved, right at runtime):
  `require_once` of a constant path that doesn't exist; right at runtime;
  `bug-12203.php`, `unused-constructor-parameters-include.php`.
- `unknown-class-cascade` (4 findings, 4 files; another identifier or a second
  report): an unknown class is `class.notFound` once; its object isn't also
  `cast.string` or `property.phpDocType`; `invalid-cast.php`, `bug-2676.php`,
  `method-assert.php`, `bug-6571.php`.
- `ctor-validators` (3 findings, 1 file; a bleeding-edge toggle): the
  `DateInterval` and `SimpleXMLElement` constructor validators
  (`checkDateIntervalConstructor`, and phpstan-rs's own XML check);
  `unthrown-exception.php`.
- `impossible-vs-if` (3 findings, 3 files; another identifier or a second
  report): a condition check PHPStan reports as
  `if.alwaysTrue`/`if.alwaysFalse`/`elseif.alwaysFalse` is
  `function.impossibleType`/`alreadyNarrowedType` in phpstan-rs;
  `bug-10394.php`, `bug-6947.php`, `call-condition-cross-file-marker-b.php`.
- `new-static-abstract` (3 findings, 3 files; a bleeding-edge toggle):
  `newStaticInAbstractClassStaticMethod`, a bleeding-edge toggle (PHPStan says
  `new.static`); `new-static-in-abstract-class-static-method.php`,
  `bug-11900.php`, `bug-12137.php`.
- `return-never-rename` (3 findings, 2 files; another identifier or a second
  report): a `never` function that returns is `return.never`, not `return.type`;
  `arrow-function-typehints-nodiscard.php`, `arrow-function-never-return.php`.
- `trait-protected-access` (3 findings, 1 file; wrong today): in a trait,
  `$this` narrowed to the host's parent class may read its protected properties;
  `bug-9386.php`.
- `assert-both` (2 findings, 1 file; the oracle setup or an approved rename):
  both tools report the mismatch (`phpstan.type` in PHPStan);
  `generic-type-override.php`.
- `catch-already-caught` (2 findings, 1 file; approved, right at runtime): a
  `catch (\Throwable)` after one catching `Throwable` never runs; right at
  runtime; `unthrown-exception-multi.php`.
- `conditional-return-check` (2 findings, 1 file; wrong today): `return $value
  === null` satisfies `@return ($value is null ? true : false)`;
  `bug-12087b.php`.
- `enum-case-const-value` (2 findings, 1 file; wrong today): `case A =
  self::PREFIX . 'x'` is a string value; `enum.caseType` compares it wrongly
  ("string does not match the string type"); `bug-9005.php`.
- `enum-duplicate-value-line` (2 findings, 1 file; another identifier or a
  second report): `enum.duplicateValue` on the enum, naming both cases, not on
  the later case; `enum-sanity.php`.
- `generics-no-parent-dup` (2 findings, 1 file; another identifier or a second
  report): `generics.noParent` once per `@implements` tag;
  `interface-ancestors-implements.php`.
- `impure-marked-pure-final` (2 findings, 2 files; wrong today):
  `impureMethod.pure` only for a final method or a method of a final class,
  since a child may be impure; `noop-impure-points.php`, `bug-12382.php`.
- `interface-conflict-enum` (2 findings, 1 file; approved, right at runtime): an
  enum whose two interfaces give `Traversable` conflicting arguments; a real
  conflict PHPStan misses; `enum-ancestors.php`.
- `last-condition-always-true` (2 findings, 2 files; wrong today): the last
  `elseif`'s always-true condition isn't reported;
  `loose-comparison-report-always-true-last-condition.php`,
  `loose-comparison.php`.
- `require-rename` (2 findings, 2 files; another identifier or a second report;
  the oracle setup or an approved rename): `requireExtends.nonObject` for
  `requireExtends.interface` (approved) and `requireImplements.class` for
  `requireImplements.onClass`; `incompatible-require-extends.php`,
  `incompatible-require-implements.php`.
- `required-after-optional-nullable` (2 findings, 2 files; wrong today): a typed
  parameter defaulting to `null` before a required one is implicitly nullable,
  not PHP 8.0's required-after-optional deprecation;
  `required-parameter-after-optional.php`,
  `required-parameter-after-optional.php`.
- `string-arg-not-callable` (2 findings, 1 file; wrong today): a string literal
  argument naming a function (`'random_int'`) isn't an impure call;
  `bug-12119.php`.
- `trait-use-line` (2 findings, 1 file; another identifier or a second report):
  a trait using a class or an unknown trait is reported on the `use`, not the
  trait's line; `trait-use-error.php`.
- `variance-private-method` (2 findings, 2 files; wrong today): variance isn't
  checked on a private method, which can't be called through the parent type;
  `method-signature-variance-contravariant.php`,
  `method-signature-variance-covariant.php`.
- `assignop-offset-dup` (1 finding, 1 file; another identifier or a second
  report): `$s['k'] += null` on a string is `offsetAccess.notFound` and
  `assignOp.invalid`, not `offsetAssign.dimType`; `offset-access-assignop.php`.
- `bcmath-comparison` (1 finding, 1 file; wrong today): `stdClass ==
  BcMath\Number` is `false` at runtime (php 8.5), not `equal.invalid`;
  `bug-13001.php`.
- `const-static-rename` (1 finding, 1 file; another identifier or a second
  report): `const static FOO` is `classConstant.value` in PHPStan;
  `return-type-class-constant.php`.
- `empty-always-falsy` (1 finding, 1 file; approved, right at runtime):
  `empty()` of `array{}|null` is always true; right at runtime;
  `foreach-iterable.php`.
- `impossible-vs-boolean` (1 finding, 1 file; another identifier or a second
  report): a check inside `&&` that the other side decides is
  `booleanNot`/`booleanAnd` in PHPStan, `function.alreadyNarrowedType` in
  phpstan-rs; `bug-4969.php`.
- `interface-not-found-rename` (1 finding, 1 file; another identifier or a
  second report): an unknown interface in `extends` is `interface.notFound`, not
  `class.notFound`; `interface-extends-error.php`.
- `interface-visibility-rename` (1 finding, 1 file; another identifier or a
  second report): a private method in an interface is `method.visibility`, not
  `method.abstractPrivate`; `visibility-in-interace.php`.
- `invalid-offset-rename` (1 finding, 1 file; another identifier or a second
  report): an object key is `offsetAccess.invalidOffset`, not
  `offsetAccess.notFound`; `invalid-key-array-dim-fetch.php`.
- `match-statement-unused` (1 finding, 1 file; wrong today): a `match` statement
  that may throw `UnhandledMatchError` isn't `expr.resultUnused`;
  `last-match-arm-always-true.php`.
- `no-parent-rename` (1 finding, 1 file; another identifier or a second report):
  `parent::` in a class without a parent is `class.noParent`, not
  `class.notFound`; `bug-12272.php`.
- `nullsafe-byref-dup` (1 finding, 1 file; another identifier or a second
  report): `$a = &$b?->foo` is `nullsafe.byRef`, not `property.nonObject` on
  `mixed`; `invalid-assign-var.php`.
- `offset-not-found-dup` (1 finding, 1 file; another identifier or a second
  report): `offsetAccess.notFound` once per offset chain; `bug-5743.php`.
- `override-magic-stdclass` (1 finding, 1 file; approved, right at runtime):
  `#[\Override]` on a trait's `__get` in a `stdClass` subclass is a PHP fatal
  error (checked with php 8.5); `bug-10149.php`.
- `param-closure-this-callable` (1 finding, 1 file; wrong today):
  `@param-closure-this` on a `callable` parameter is allowed (`callable` accepts
  a `Closure`); `function-call-param-closure-this.php`.
- `phpdoc-default-null` (1 finding, 1 file; wrong today): an untyped parameter
  with `@param bool` defaulting to `null` isn't `parameter.defaultValue`;
  `incompatible-default-parameter-type-methods.php`.
- `property-tag-rename` (1 finding, 1 file; another identifier or a second
  report): `@property-read` over a private property is `assign.propertyReadOnly`
  in PHPStan; `private-property-tag-read.php`.
- `readonly-phpdoc-hook` (1 finding, 1 file; wrong today): a `@readonly`
  property assigned in its own set hook isn't assigned outside the constructor;
  `property-hooks-readonly-by-phpdoc-assign.php`.
- `static-this-rename` (1 finding, 1 file; another identifier or a second
  report): `$this` in a static arrow function is `mixed` (`property.nonObject`),
  not `property.notFound`; `defined-variables-arrow-functions.php`.
- `tentative-return-rename` (1 finding, 1 file; another identifier or a second
  report): an untyped `offsetGet()` overriding `ArrayAccess`'s tentative `mixed`
  is `method.tentativeReturnType`, not `missingType.return`;
  `array-destructuring.php`.
- `too-wide-trait-method` (1 finding, 1 file; approved, right at runtime): a
  private trait method that never returns `null` in its only host; PHPStan skips
  trait methods; `tooWideMethodReturnType-private.php`.
- `trait-condition-hosts` (1 finding, 1 file; wrong today): `$this instanceof
  Foo` in a trait is reported only when it holds in every host;
  `impossible-instanceof.php`.
- `trait-dir-include` (1 finding, 1 file; approved, right at runtime): `__DIR__`
  in a trait is the trait's directory; the include fails at runtime;
  `Bug15015Trait.php`.
- `trait-duplicate-property` (1 finding, 1 file; another identifier or a second
  report): a trait declaring a property twice is the duplicate, not
  `missingType.property`; `bug-14250-promoted-properties.php`.
- `unreachable-nop` (1 finding, 1 file; wrong today): an empty statement after
  `return` isn't `deadCode.unreachable`; `unreachable.php`.
- `use-const-not-found` (1 finding, 1 file; wrong today): an imported constant
  that doesn't exist is reported where it is used, not on the `use`;
  `group-uses.php`.
- `var-tag-return` (1 finding, 1 file; approved, right at runtime): an `@var
  string` above `return $this->foo` (`int`) is wrong; PHPStan doesn't check a
  variable-less `@var` on a return; `var-tag-changed-expr-type.php`.
- `var-tag-subtype` (1 finding, 1 file; approved, right at runtime): an `@var`
  shape over a literal missing keys added later; PHPStan reports it only with
  `reportWrongPhpDocTypeInVarTag`; `bug-12222.php`.
- `yield-from-key-value` (1 finding, 1 file; wrong today): `yield from` checks
  the inner generator's values as `generator.valueType`; phpstan-rs reports them
  as `generator.keyType`; `yield-from.php`.

## Appendix J: the nsrt suite's cited files by cause

At `76e738b4`, the 169 cited files that aren't wholly an extension's have 3,445
lines with an nsrt mismatch the earlier passes left unsorted. Each line was read
against its fixture, its citing item's text and the rest of the plan, and where
the two tools disagree on runtime behaviour, php 8.5. 1,076 are what the citing
item describes, 635 are calls Appendix G counts, and 23 differ only in a display
form the gate's normalization covers (a union nested in a generic, `never`, a
backslash; the measurement sorted top-level unions only). Of the other 1,711:
664 are a cause another item or Appendix G names (38 causes, listed with the
item so `gates/nsrt/owners.toml` can take the lines), 871 are an Appendix H
cause in a cited file (80 causes), 146 are causes no item describes (15, each a
phase item), 27 are phpstan-rs right at runtime (12, approved deviations) and 3
are the suite's setup. Each entry gives the cause's key, its mismatch count and
files, and the files, most mismatches first; the phase is done with it when its
lines have no mismatches, or each remaining one is an approved deviation that
names its runtime reason. `ext:` keys are E2's extensions; the calls' own
mismatches are in Appendix G, and these are the values that flow on from them.

**Named elsewhere** (664 mismatches, 27 files, 38 causes):

- `bcmath-invalid-ops` (141, 1 file): `BcMath\Number` with a `bool`, `null`,
  array, float or non-numeric operand, and under every shift and bitwise
  operator, is *ERROR* (`int` or `float` today); named by D3 Error type, and E2
  (the BcMath operators, `bcmath-*.php` 304); `bcmath-number.php`.
- `loose-eq-fold` (107, 1 file): `==` folds by PHP 8's table where every pair of
  members has one answer: an array against a scalar or a shape, an int range
  against constants, `true`/`false`/`null`, case-accessory strings against
  literals of the other case, numeric strings against non-numeric ones; named by
  D3 loose comparison by each type's `looseCompare`; `loose-comparisons.php`.
- `preg-mixed` (31, 2 files): `$matches` stays `mixed` where the pattern is
  still analysable: a non-constant flags argument, `(?J)` duplicate names, a
  pattern in a variable, a `(?|…)` branch reset, a `preg_quote()` concatenation;
  a pattern it can't parse gives `array<string>`; named by D3 By-reference out
  types of built-in functions (`preg_match` with a non-constant pattern);
  `preg_match_shapes.php`, `passed-by-reference.php`.
- `for-cond-entry` (30, 2 files): a `for` condition is evaluated on the initial
  values: false there (`$i = 1; $i < count([])`) makes the body unreachable,
  every expression in it `*NEVER*`; true there runs the body at least once, so
  what it assigns is defined after the loop; named by D5 For (a first condition
  `true` runs the body, `false` makes it unreachable); `math.php`, `if.php`.
- `shift-ranges` (30, 1 file): `<<` and `>>` on ranges and constants give ranges
  and literals, and a negative shift, an array or a non-numeric string is
  *ERROR* (`int` today); named by D3 Arithmetic (a shift covers every operand)
  and Error type; `integer-range-types.php`.
- `coalesce-fold` (26, 1 file): `??` folds: a non-nullable left is the left
  (`$this->string ?? false` is `string`), a never-null right drops `null`, and
  an always-null or undefined left is the right; named by D3 isset/`empty`/`??`
  through one issetability descriptor (nsrt `isset-coalesce-empty-type.php`);
  `isset-coalesce-empty-type.php`.
- `float-const-fold` (26, 3 files): constant arithmetic folds to its value (`1 /
  2` is `0.5`, `1 / 1` is `1`, `$w = 1; $w *= 2.0` is `2.0`), `/ 0` and `% 0`
  are *ERROR*, `%` of floats is an int, and unary `+`/`-` of a constant folds;
  named by D3 Arithmetic (`InitializerExprTypeResolver`, a float literal types
  as its value); `binary.php`, `node-callback-scope.php`, `if.php`.
- `pow-ranges` (26, 1 file): `**` on ranges and constants gives ranges (`int<4,
  81>`) and literals (`'5' ** '4'` is `625`, `x ** 0` is `1` or `1.0`); named by
  D3 Arithmetic (`int ** int`, nsrt `pow.php`); `pow.php`.
- `enum-loose-fold` (25, 1 file): `==`/`!=` with an enum fold: identity against
  enums, `true` against `true`, `false` against `false`, `null`, strings and
  ints; named by D3 loose comparison by each type's `looseCompare` (an enum
  against another enum's case); `enums.php`.
- `pow-error` (20, 1 file): `**` with an array, an object or a non-numeric
  string is *ERROR*; named by D3 Error type; `pow.php`.
- `static-call-classstring` (17, 1 file): a static call, a static property or
  `new` through a `class-string<T>` variable or an object's `::class` is
  `T`/`static` (`mixed` today); named by D3 A class-string expression as a
  static receiver, and `new $c()`; `generics.php`.
- `static-closure-this` (17, 1 file): `$this` in a static closure or arrow
  function, or in a closure outside a class, is *ERROR* (`mixed` today); a
  static closure passed to a `@param-closure-this` parameter isn't bound, so it
  gets the bound class today; named by D3 Error type (`$this` in a static
  closure) and the static-closure `$this` item; `param-closure-this.php`.
- `bcmath-compare-fold` (16, 1 file): comparisons and `&&`/`||` of a
  `BcMath\Number` against constants fold (`> null` is `true`, `&& false` is
  `false`); named by E2 (the `BcMath\Number` operators) and D3 Comparisons
  between constants fold; `bcmath-number.php`.
- `int-truthy-narrow` (15, 1 file): truthiness and `== 0` on an int narrow it to
  `0` or `int<min, -1>|int<1, max>`, and a loop counter is `int<0, max>`; named
  by D3 Narrowing gaps (`!empty($v)` on an `int`, loose `==` with a constant);
  `node-callback-scope.php`.
- `isset-fold` (15, 1 file): `isset()` and `empty()` of variables and offsets
  whose definedness and nullability are known fold to `true`/`false`; named by
  D3 isset/`empty`/`??` through one issetability descriptor; `binary.php`.
- `maybe-defined-type` (13, 2 files): a maybe-defined variable keeps its type
  (`mixed` today); named by D5 Certainty (a maybe-defined variable's reads type
  as `mixed` today); `if.php`, `bug-7291.php`.
- `multiply-zero` (12, 1 file): `$x * 0`, `* false`, `* '0'` and `$x *= 0` fold
  to `0` on an int and `0.0` on a float; named by D3 Arithmetic
  (`InitializerExprTypeResolver`); `math.php`.
- `bcmath-concat` (10, 1 file): a `BcMath\Number` concatenated is a non-empty
  numeric string, non-falsy with a non-empty other side; named by E2 (the
  `BcMath\Number` operators, `bcmath-*.php` 304); `bcmath-number.php`.
- `foreach-constant-unroll` (10, 2 files): a foreach over a constant array runs
  once per element: a value changed each pass stays a finite union
  (`1|10|100|null`), appends build `list{0: 1|2|3, 1?: …, 2?: …}`, and after the
  loop the last key and value are the last element's (`'b'`, `string`); named by
  D5 Foreach (constant arrays of up to 16 keys are unrolled);
  `foreach-loop-variables.php`, `bug-15250.php`.
- `template-property` (10, 1 file): a property typed with a class template reads
  as `T` (`mixed` today); named by D3 `$this` carries the class's own templates,
  so a property typed `T` reads as `T`; `generics.php`.
- `assert-array-mixed` (9, 1 file): `@phpstan-assert string[] $arr` on a
  `mixed[]` argument doesn't narrow it, and the declaration is
  `assert.alreadyNarrowedType` "does not narrow down the type", a false
  positive: `array<mixed>` is taken as already `array<string>`, where a scalar
  assertion (`int` on `mixed`) and `list<int>` on `array` narrow; named by E an
  array assertion over `mixed` values narrows; `assert-docblock.php`.
- `intersect-empty` (7, 1 file): `array_intersect*`/`array_uintersect*` against
  `[]` are `array{}`; today they keep `$a`, as `array_intersect_key` does; named
  by E2 `array_intersect_key($a, [])` is `$a` instead of `array{}`, and the
  other intersect functions; `array-functions.php`.
- `plus-invalid` (7, 1 file): `+` with an array and a non-array, and on
  non-numeric strings, is *ERROR*; named by D3 Error type; `binary.php`.
- `append-shape` (6, 1 file): appends to an undefined or `null` variable, and to
  a constant list, build the exact shape (`$a[] = 'one'` is `array{'one'}`);
  named by D3 Array literals and appends (appending keeps the shape); `if.php`.
- `ext:Sscanf` (6, 1 file): `sscanf($s, '%s %d %d', $month, $day, $year)` types
  the by-reference outputs by their conversions (`string|null`, `int|null`),
  `mixed` today; named by D3 By-reference out types of built-in functions;
  `sscanf.php`.
- `foreach-overwrite-value` (4, 1 file): writing `$a[$k]` for the loop's own key
  on every path through a foreach over `$a` replaces its value type: after
  `foreach ($a as $k => $v) { $a[$k] = 1; }` on `array<int, string>` it is
  `array<int, 1>` (`array<int, 1|string>` today), a property included; named by
  D5 Foreach (`OriginalForeachKey`, the iteratee rewritten from the loop
  variables); `overwritten-arrays.php`.
- `int-float-sum-static` (4, 1 file): a sum with a `static::` constant is
  `float|int`, since a subclass may override it; named by D3 Arithmetic and
  `static::C` of an overridable constant; `binary.php`.
- `string-offset-write` (4, 1 file): `$s[0] = '1'` makes a string non-empty;
  writing a general string at an offset of a `literal-string` drops
  `literal-string`, and of a non-falsy one leaves it non-empty; named by D3 a
  string offset write changes the string; `bug-3981.php`.
- `ext:ArrayFill` (3, 1 file): `array_fill()`/`array_fill_keys()` with constant
  arguments build the shape; named by Appendix G; `array-functions.php`.
- `string-offset-read` (3, 1 file): `$s[0]` on a string is a `string`
  (`non-empty-string` in PHPStan), `mixed` today; named by D3 Offset reads
  (string offsets are typed); `bug-3981.php`.
- `unresolvable-phpdoc` (3, 3 files): `\\Test\Bar` (a doubled leading backslash)
  in a `@param` or `@var` is an invalid type, so the parameter or property is
  `mixed`; named by C what phpdoc-parser rejects (`\\Foo` with a doubled
  backslash) is dropped; `method-phpDocs-inheritdoc.php`,
  `methodPhpDocs-implicitInheritance.php`, `properties.php`.
- `ext:ArraySlice` (2, 1 file): `array_slice()` of a constant array, then an
  append, keeps the shape; named by Appendix G; `array-functions.php`.
- `ext:ArraySum` (2, 1 file): `array_sum()` of floats is `0|float`, and of a
  non-empty float array `float`; named by Appendix G; `array-sum.php`.
- `switch-nested-break` (2, 1 file): a `break` inside an `if` in a switch case
  carries that path's assignments past the switch, fallthrough included; named
  by D5 Switch (a `break` nested in a case's `if`, nsrt `if.php`); `if.php`.
- `ternary-const-cond` (2, 1 file): a ternary on a constant condition is its
  taken arm (`true ? 1 : 2` is `1`); named by D5 dead branches pruned (a
  ternary's dead side); `binary.php`.
- `ext:ArrayFindKey` (1, 1 file): `array_find_key()` over a shape is the union
  of its keys or `null`; named by E2's `ArrayFindKeyFunctionReturnTypeExtension`
  (its own calls in the suite differ only in display, so Appendix G has no entry
  for it); `array-find-key.php`.
- `spaceship-range` (1, 1 file): `$a <=> $b` is `int<-1, 1>` (`int` today);
  named by D3 `<=>` is `-1|0|1`; `math.php`.
- `string-truthy-narrow` (1, 1 file): a truthy string is `non-empty-string`;
  named by D3 Narrowing gaps (`if ($this->only)` on `?string`);
  `node-callback-scope.php`.

**Appendix H's causes** (871 mismatches, 68 files, 80 causes):

- `preg-shapes` (174, 1 file): group 0 of a pattern that must match something is
  non-empty, and non-falsy with a literal or a quantified group (`string`
  today); the false branch of `preg_match()` is `array{}`, so the merge is
  `array{}|shape`, not one shape with every key optional; a capturing group with
  a literal or a `+` quantifier is non-falsy (`non-empty-string` today); a group
  of alternated literals or a class of literal characters is their union
  (`'bar'|'baz'`, `']'|'a'`); `PREG_UNMATCHED_AS_NULL` and optional trailing
  groups give a `list{…}` shape; `PREG_OFFSET_CAPTURE` offsets are `int<-1,
  max>` (`int` today); an empty alternative (`'~|(a)~'`) makes the group
  optional in one shape instead of a union of shapes; `preg_match_shapes.php`.
- `settype` (156, 1 file): `settype($x, 'int')` and each other constant type
  name rewrites the variable to the cast type, and an invalid name is *ERROR*;
  the variable is `mixed` today (`SetTypeFunctionTypeSpecifyingExtension`);
  `settype($a['x'], 'string')` rewrites that offset of the shape; it is
  unchanged today, a wrong value; `set-type-type-specifying.php`.
- `subtracted` (44, 6 files): subtracted types: truthiness, `!=`/`!==`, `is_*`
  and `instanceof` negations on `mixed` or `object` give `mixed~(…)`/`object~X`,
  a negated `@phpstan-assert !null` gives `mixed~null`, a `while (($f = …) ===
  null)` exit leaves `mixed~null`, and a `(string)` or `(int)` of the result
  keeps what it proves; `node-callback-scope.php`, `non-empty-string.php`,
  `integer-range-types.php`, `bug-15039.php`, `if.php`, `assert-docblock.php`.
- `closure-param-infer` (42, 3 files): a closure takes its parameters from where
  it is used: on the right of `|>` or invoked at once, the argument's type;
  passed where a callable type gives `never` (`array_map` over `list<never>`,
  `callable(never...)`) or `true`, that type; `pipe-operator.php`,
  `bug-15285.php`, `bug-2572.php`.
- `static-type` (42, 7 files): `static` and `$this` survive: `$this` in a method
  is `$this(Foo)`, `@var static[]` and `@return static[]` are
  `array<static(Foo)>` through `$this->`, `self::` and `static::`, an inherited
  `@param static` is `static(Child)`, `parent::m()` returning `static`/`$this`
  keeps `static(Child)`/`$this(Child)`, and `@param-closure-this static` binds
  `static(Bar)`; `method-phpDocs-inheritdoc.php`,
  `methodPhpDocs-implicitInheritance.php`, `static-methods.php`,
  `static-properties.php`, `param-closure-this.php`, `native-types.php`,
  `properties.php`.
- `byref-array-fns` (37, 4 files): `array_pop`, `array_shift`, `shuffle` and the
  sort family rewrite the argument: a shape loses the key, non-emptiness and a
  template's `T&list<…>` stay, `mixed` becomes a `list` (or `mixed~array`), a
  non-array is *ERROR*, and a shape whose keys are all optional shuffles to a
  possibly empty `list` (`non-empty-list` today, a wrong value); `array` today;
  `array-pop.php`, `array-shift.php`, `sort.php`, `shuffle.php`.
- `operator-ranges` (34, 3 files): `+`, `-`, `*` and `/` on ranges and range
  unions give ranges (`int<2, 13>`, `float|int<1, 10>`), a constant minus
  `count()` is bounded, `/ -1` of a range flips it and stays `int`, and
  `PHP_INT_MIN / -1` is the float alone (`9223372036854775807|float` today);
  `integer-range-types.php`, `math.php`, `bug-15242.php`.
- `array-is-list` (25, 1 file): `array_is_list()` folds on list shapes: `true`
  on a `list{…}`, `false` after unsetting a required key or a non-constant key,
  and `true` again after appending; `bug-14177.php`.
- `type-print` (24, 5 files): display forms beyond the gate's normalization: a
  template with its bound (`T of stdClass (class …, argument)`), a newline in a
  constant string (`"\n"`, two raw line breaks today), and an explicit `mixed`
  value (`list<mixed>`); `generics.php`, `iterable.php`, `shuffle.php`,
  `predefined-constants.php`, `property-hooks.php`.
- `list-write` (15, 4 files): an offset write makes the array non-empty (`$a[$s]
  = 2`, `$this->p[$s] = $v`) and keeps a `string` key type
  (`(int|string)|string` today); `bug-4708.php`, `foreach-loop-variables.php`,
  `non-empty-string.php`, `bug-2911.php`.
- `new-dynamic` (13, 1 file): `new $a()` is `object` on a string or `mixed`, and
  the class-string's class after narrowing (`mixed` today);
  `generic-class-string.php`.
- `generic-new-infer` (12, 3 files): `new` infers the class's templates: without
  arguments `C<mixed>` (stored in a `C<int>` property, `C<int>`), from a static
  factory, and through a stub (`new IteratorIterator($it)` gives
  `getInnerIterator()` `Iterator<int, string>`); it prints without arguments
  today; `generics.php`, `bug-8441.php`, `iterator-iterator.php`.
- `loop-generalize` (12, 3 files): loop counters and values changed each pass
  widen to ranges (`int<0, 4>`, `int<10, max>`); unrolled constants or `int`
  today; `integer-range-types.php`, `foreach-loop-variables.php`, `if.php`.
- `is-subclass-narrow` (11, 1 file): `is_subclass_of($a, X)` narrows `mixed`,
  `string`, `object` and a class-string to `class-string<X>|X`, and `$a::f()`
  after it returns `f()`'s type; `generic-class-string.php`.
- `shape-union` (11, 3 files): branches with different shapes stay a union
  (`array{}|array{'one', 'two'}`, preg_match's `array{}|array{string}`), and
  `isset`/`array_key_exists` drop the members without the key; one shape with
  optional keys today; `isset.php`, `if.php`, `bug-14081.php`.
- `key-first-last-fold` (10, 1 file): `($key = array_key_first($l))` truthy or
  `!== null` makes the list non-empty and `$key` `int<1, max>`/`int<0, max>`,
  and on a list the key is `int<0, max>|null`; `bug-14081.php`.
- `iterator-key` (9, 2 files): foreach and `getIterator()` over a generic
  `IteratorAggregate`, `Iterator`, `Generator<Foo>` or `ArrayObject<int,
  string>` give its key and value types; `generics.php`, `iterable.php`.
- `class-constant` (8, 3 files): `X::class` is the literal name whether or not
  `X` exists, `static::class` is `class-string<static>` and `parent::class` a
  `class-string`; `$o::class` is `class-string<X>&literal-string` (the literal
  on a final class or an enum), `null` on a nullable object, and `'Foo'::class`
  is `'Foo'`; `binary.php`, `class-constant-on-expr.php`, `enums.php`.
- `callable-template-infer` (7, 2 files): `@param T|(Closure(): T)` given a
  closure or an invokable object binds `T` from its return, not to the closure
  itself; `generic-unions.php`, `bug-2572.php`.
- `ext:CallUserFunc` (7, 1 file): `call_user_func()` with a function name, a
  spread or a `[class, method]` array returns the callee's return, a template
  resolved; `call-user-func.php`.
- `ext:DateIntervalFormat` (7, 1 file): `DateInterval::format('%a')` is a
  lowercase non-empty numeric string, so `* 60` on it is `float|int`;
  `bug-1452.php`.
- `generic-reflection` (7, 2 files): `ReflectionClass<T>`'s `getName()`,
  `->name` and `newInstanceWithoutConstructor()` carry `T`/`static`, `new
  ReflectionClass(stdClass::class)` included, and so does a first-class callable
  of `getName`; `generics.php`, `first-class-callables.php`.
- `isset-narrow` (7, 4 files): `isset()` narrows: an offset makes the array
  non-empty and `mixed` `mixed~null`, and a maybe-defined variable becomes
  defined with its type (`Foo`, `1`); `if.php`, `isset.php`,
  `array-functions.php`, `falsey-isset-certainty.php`.
- `list-shape` (7, 1 file): `array_key_exists(3, $l)` on `list{0: s, 1: s, 2?:
  s, 3?: s}` makes every lower key required, the else drops key 3, and a
  `list{…}` prints as one; `bug-14177.php`.
- `assert-tags` (6, 1 file): a templated `@phpstan-assert T $o` with
  `class-string<T> $class`, and a method's `-if-true` on its argument, inherited
  through `parent::`; `assert-docblock.php`.
- `casts` (6, 1 file): casts of constants fold: `(int) '1'` is `1`, `(array)
  '1'` is `array{'1'}`, `(object)` is `stdClass`, `(bool) true` is `true`;
  `node-callback-scope.php`.
- `error-type` (6, 5 files): an invalid expression is *ERROR*: a missing enum
  case, an undefined constant or property, an `unset()` variable,
  `$string::class` (PHP throws `Error`); `class-constant-on-expr.php`,
  `constants.php`, `enums.php`, `native-types.php`, `properties.php`.
- `ext:ArrayCountValues` (6, 1 file): `array_count_values()` keeps the value
  literals as keys with `int<1, max>` counts, and is `array{}` for objects and
  scalars that aren't int or string; `array-count-values.php`.
- `instanceof-intersect` (6, 1 file): `instanceof` narrows to the intersection
  with an interface (`$this(Foo)&BarInterface`), keeps `static(Foo)`, narrows a
  class-string subject, and is *ERROR* for an invalid right side;
  `instanceof.php`.
- `array-plus` (5, 1 file): `+` on shapes is a shape keyed by the left then the
  right (`array{int, int, int}`); `binary.php`.
- `check-fold` (5, 1 file): `is_int`, `is_numeric` and `in_array` on known
  values fold; `binary.php`.
- `empty-narrow` (5, 1 file): truthiness and `count()` on an array narrow it to
  `array{}` or non-empty, and an impossible branch is `never`;
  `node-callback-scope.php`.
- `ext:ArrayMap` (5, 1 file): `array_map()` with a typed closure keeps the keys
  and the closure's return; `array-functions.php`.
- `ext:ArrayMerge` (5, 2 files): `array_merge()` of int arrays is `array<int>`,
  and of a shape with `array<string, mixed>` keeps the tail's key type
  (`...<string, mixed>`; `...` today); `bug-2911.php`, `array-functions.php`.
- `key-exists-fold` (5, 2 files): `array_key_exists('a', $a)` makes `mixed` or
  `array<string, int>` `non-empty-array&hasOffset('a')`, and a comparison of the
  offset after it adds `hasOffsetValue`; `array-flip.php`, `isset.php`.
- `offset-narrow-memory` (5, 1 file): `!is_string($s['k'])`, `!in_array($s['k'],
  […], true)` and `!is_numeric($s['k']) || $s['k'] < 1` throwing narrow the
  offset of an unsealed shape; `bug-2911.php`.
- `var-tags` (5, 1 file): `@var` on `global`: a nameless one types the single
  global, a named one only the variable it names (`@var int $baz` above `global
  $lorem` leaves `$baz` alone, where it is `int` today), and one above a foreach
  destructuring types the destructured variable; `bug-4500.php`.
- `callable-value-call` (4, 1 file): calling a variable returns the callee's
  return: an object with `__invoke`, `[$this, 'm']`; `['C', 'm']` and `'C::m'`
  naming a non-static method are *ERROR* (`mixed` today); `callables.php`.
- `closure-generator` (4, 1 file): a closure with `yield` returns `Generator<K,
  V, …>`, so `new FooFilterIterator($gen())` infers its templates and
  `key()`/`current()` are `bool` (`mixed` today); `bug-7519.php`.
- `ext:ClosureFromCallable` (4, 2 files): `Closure::fromCallable()`, of a `[$o,
  'm']` array too, keeps the callable's signature; `first-class-callables.php`,
  `node-callback-scope.php`.
- `literal-narrow` (4, 1 file): `$s === FinalCar::class` on
  `class-string<FinalCar|FinalBike|FinalBoat>` gives the literal, and the else
  removes `FinalCar`; `bug-9534.php`.
- `template-default-param` (4, 2 files): a template parameter whose argument is
  omitted binds from the default: `@param T $t = null` makes `new C()` a
  `C<null>` and a call `3|null`; `bug-8441.php`, `generic-unions.php`.
- `coalesce-narrow` (3, 1 file): `??` with a non-nullable left is the left
  (`$string ?? $int` is `string`), and on a maybe-defined variable the union;
  `binary.php`.
- `enum-name-value` (3, 1 file): `->name` of an enum with cases removed is the
  remaining cases' names; `bug-8486.php`.
- `inheritdoc` (3, 1 file): a property's PHPDoc is inherited through
  `{@inheritdoc}`, `@inheritdoc` without braces and implicitly;
  `properties.php`.
- `loop-body-narrow` (3, 2 files): inside a foreach over an array, the array is
  non-empty; `native-types.php`, `iterable.php`.
- `nullsafe-narrow` (3, 1 file): the arguments of `$e?->m(…)` see `$e` non-null,
  and a truthy `$s?->p` makes `$s` non-null; `nullsafe.php`.
- `ext:ArrayFillKeys` (2, 1 file): `array_fill_keys(['foo', …], true)` is a
  shape, so `$l['derp'] ?? false` is `false`; `isset.php`.
- `ext:ArrayFlip` (2, 1 file): `array_flip()` of `mixed[]` is
  `array<(int|string)>`, and of a list `array<V, int<0, max>>`;
  `array-flip.php`.
- `ext:Explode` (2, 1 file): `[$x, $y] = explode(':::', 'App/Service::foo')`
  folds, so `$y` past the end is *ERROR*; `bug-15013.php`.
- `ext:MinMax` (2, 1 file): a variable holding `min(30, $i)` is `int<min, 30>`;
  `math.php`.
- `finally-scope` (2, 1 file): `finally` sees the merged `try` and `catch`
  scopes (`1|'foo'`, the caught exceptions or `null`); `finally.php`.
- `foreach-narrow-all` (2, 1 file): a foreach throwing when `$key === 'a'`
  leaves `array<'b', int>` and later keys `'b'`; `bug-15250.php`.
- `generic-defaults` (2, 1 file): a generic missing arguments fills them:
  `Generator<Foo>` is `Generator<mixed, Foo, mixed, mixed>`, and a bare
  `ReflectionClass` is `ReflectionClass<object>`, so `->name` is
  `class-string<object>`; `nullsafe.php`.
- `mixin` (2, 2 files): `@mixin` forwards method calls, on an enum and through a
  stub's `@mixin TIterator` (`IteratorIterator::getArrayCopy()`); `enums.php`,
  `iterator-iterator.php`.
- `readonly-remember` (2, 1 file): a readonly property assigned a constant in
  the constructor keeps it in other methods (`'foo'`; `?? null` stays `'foo'`);
  `bug-15056.php`.
- `simplexml` (2, 1 file): `SimpleXMLElement::asXML()` and `xpath()`;
  `binary.php`.
- `static-closure` (2, 1 file): a static closure prints `static-Closure():
  never`; `bug-15285.php`.
- `unset-shape` (2, 1 file): `unset()` with a non-constant key makes every key
  optional; `binary.php`.
- `catch-scope` (1, 1 file): a `catch (\Exception $e)` after `catch
  (InvalidArgumentException)` is `Exception~InvalidArgumentException`;
  `native-types.php`.
- `conditional-return` (1, 1 file): a conditional return on a template resolves
  per member of a `class-string<CustomResponse>|null` argument
  (`CustomResponse|null`); `bug-8048.php`.
- `count-narrow` (1, 1 file): `count($a) >= 5` on an unsealed list makes the
  first five entries required; `unsealed-derivations.php`.
- `else-narrow` (1, 1 file): the else of an always-true `instanceof` on the
  PHPDoc type is `*NEVER*`; `native-types.php`.
- `ext:ArrayReplace` (1, 1 file): `array_replace([1, 2, 3], [4, 5, 6])` is
  `array{4, 5, 6}`; `native-types.php`.
- `ext:ClosureBind` (1, 1 file): `(function () {…})->call($o)` binds `$this` in
  the body to `$o`'s type; `native-types.php`.
- `ext:FilterVarArray` (1, 1 file): `filter_var_array()` of an unsealed shape
  applies the filter to the tail; `unsealed-derivations.php`.
- `falsy-narrow` (1, 1 file): the else of `if ($s)` on a string is `''|'0'`;
  `non-empty-string.php`.
- `float-literal` (1, 1 file): `1.0` merged from a `catch` stays the literal
  (`1|1.0`); `if.php`.
- `identical-fold` (1, 1 file): `'1e2' === '1E2'` folds to `false`;
  `loose-comparisons.php`.
- `identical-narrow` (1, 1 file): `$foo === Foo::TWO` on `Foo::ONE` is
  `*NEVER*`; `enums.php`.
- `impure-invalidate` (1, 1 file): an immediately-invoked callable calling
  `$s->voidMethod()` forgets `$s->prop`'s narrowing; `param-closure-this.php`.
- `increment-types` (1, 1 file): `$s++` on a `string` is `float|int|string`;
  `literal-string.php`.
- `offset-write-unknown-key` (1, 1 file): a foreach writing `$r[$field]` under
  `array_key_exists($field, $r)` keeps the other `hasOffsetValue` facts;
  `bug-4708.php`.
- `property-exists-narrow` (1, 1 file): `property_exists()` on
  `object|class-string` keeps `class-string` (`object|string` today);
  `bug-2861.php`.
- `property-tags` (1, 1 file): a native property from an ancestor
  (`DOMDocument::$documentElement`) wins over the class's `@property-read
  string`; `properties.php`.
- `spread-list` (1, 1 file): `[...$generator, &$x]` over `Generator<int, int>`
  is a `non-empty-list<int>`; `bug-15247.php`.
- `stub-return` (1, 1 file): `Throwable::getCode()` on an `Exception` is
  `int|string`; `generics.php`.
- `template-bound` (1, 1 file): an unbound template `U of T` reads as its bound,
  `int` after `new Foo(1)` binds `T` (`mixed` today);
  `nested-generic-incomplete-constructor.php`.
- `template-generalize` (1, 1 file): `new FooGeneric(5)` generalizes to
  `FooGeneric<int>`; `node-callback-scope.php`.
- `union-normalize` (1, 1 file): an `exit()` arm leaves `never|2|3`
  unnormalized; `match-expr.php`.

**B4** (45 mismatches, 5 files, 5 causes):

- `unsealed-shape-ops` (35, 2 files): unsealed shapes (`array{a: int, ...<K,
  V>}`) print their tail key only when it isn't `int|string`, become general
  arrays when every key is optional, keep `hasOffsetValue` and non-emptiness
  from writes, and keep the tail through `array_unshift`, `array_pop`,
  `array_shift`, `shuffle` and `array_splice` (`array` today);
  `unsealed-array-shapes.php`, `unsealed-derivations.php`.
- `template-infer` (5, 1 file): a template inferred from an unsealed shape's
  values includes the tail; `unsealed-array-shapes.php`.
- `untyped-property-mixed` (3, 1 file): an untyped property is `mixed`, not the
  type of its assignments; `integer-range-types.php`.
- `static-call-generic` (1, 1 file): `$s::genericStatic(1)` on a class-string
  resolves the template; `node-callback-scope.php`.
- `template-float-bound` (1, 1 file): a template bounded by `float` keeps the
  literal `1.0`; `generics.php`.

**C6** (7 mismatches, 1 file, 1 cause):

- `env-constants` (7, 1 file): a constant whose value depends on the build or
  the machine (`PHP_EXTRA_VERSION`, `DEFAULT_INCLUDE_PATH`, `PEAR_INSTALL_DIR`,
  `PEAR_EXTENSION_DIR`, `PHP_CONFIG_FILE_SCAN_DIR`, `ZEND_DEBUG_BUILD`,
  `ZEND_THREAD_SAFE`) is its general type; today it is phpstorm-stubs' sample
  value (`'/usr/lib/php5/20090626'`), wrong on every PHP 8 install;
  `predefined-constants.php`.

**C7** (3 mismatches, 2 files, 2 causes):

- `hook-method-template` (2, 1 file): a constructor's `@template T` resolves in
  a promoted property's hook; it is a class named `T` today;
  `property-hooks.php`.
- `class-string-rule` (1, 1 file): `Rule<T>::getNodeType()` returns
  `class-string<T>`; `generics.php`.

**C8** (2 mismatches, 1 file, 1 cause):

- `type-alias-enum` (2, 1 file): `@phpstan-type` declared on an enum resolves in
  its methods; `enums.php`.

**D3** (18 mismatches, 4 files, 4 causes):

- `assert-named-args` (10, 1 file): `@phpstan-assert` on a parameter passed as a
  named argument out of order (`trueCheck(allow_null: false, input: $a)`) isn't
  applied, so `$a` stays `mixed`; `bug-9435.php`.
- `clone-with` (5, 1 file): PHP 8.5 `clone($o, […])` is `object` for a `?object`
  or `mixed` `$o` (cloning `null` throws; `object|null` and `mixed` today),
  keeps `static` for `new $this`/`new static`, and adds `hasProperty()` for each
  assigned name; `clone-with.php`.
- `nullsafe-on-null` (2, 1 file): `$null?->foo` and `$null?->m()` on `null` are
  `null` (`mixed` today); `nullsafe.php`.
- `class-exists-narrow` (1, 1 file): `class_exists()` on a literal class name
  keeps the literal (`'stdClass'`; `class-string<stdClass>` today);
  `generic-class-string.php`.

**E2** (71 mismatches, 2 files, 2 causes):

- `ext:function-values` (65, 1 file): values assigned from `microtime()`,
  `version_compare()`, the `mb_*` encoding functions, `gettimeofday()`,
  `parse_url()`, `stat()` and its variants, `base64_decode()` and
  `str_word_count()` keep the extensions' types; `functions.php`.
- `ext:ArrayReduce` (6, 1 file): `array_reduce()` is the callback's return on a
  non-empty array, the initial value on an empty one and their union otherwise
  (`mixed|null` today); `array-functions.php`.

**Right at runtime** (27 mismatches, 12 files, 12 causes):

- `diff-empty-right` (8, 1 file): `array_diff*($a, [])` returns `$a` unchanged,
  so phpstan-rs's exact shape is right where PHPStan widens;
  `array-functions.php`.
- `match-unhandled-narrow` (4, 1 file): after `match ($i) { 1 => … }` without
  `default`, an unmatched `$i` has thrown `UnhandledMatchError`, so `$i` is `1`;
  PHPStan keeps `1|2|3|4`; `match-expr.php`.
- `array-union-merge` (3, 2 files): phpstan-rs keeps a union of arrays unmerged
  (`list<int>|non-empty-list<string>` in the else of `count() >= 2`,
  `array{}|non-empty-list<1|2|3>`), the same values as PHPStan's merged
  `list<…>`; `bug-13747.php`, `foreach-loop-variables.php`.
- `bcmath-strict` (3, 1 file): `BcMath\Number == null` is `false` and `* null`
  throws at runtime (php 8.5); PHPStan's `bool` and `0` are its own gaps;
  `bcmath-number.php`.
- `iterator-aggregate-right` (2, 1 file): foreach over an `IteratorAggregate`
  whose `getIterator()` is typed gives its key and value (`int`, `string`);
  PHPStan has `mixed` and notes it should be these; `bug-4415.php`.
- `array-flip-mixed-right` (1, 1 file): `array_flip($mixed)` is
  `array<int|string>`, as right as PHPStan's `array`: the values are the old
  keys; `array-flip.php`.
- `array-unique-shape` (1, 1 file): `array_unique([1, 2, 3])` stays `array{0: 1,
  1: 2, 2: 3}`, which PHP returns; `array-functions.php`.
- `catch-unknown-throwable` (1, 1 file): a caught unknown class is `X&Throwable`
  in PHPStan; the plain class holds the same values; `if.php`.
- `duplicate-var-right` (1, 1 file): a property with two `@var int` tags is
  `int`; PHPStan drops both and has `mixed`; `properties.php`.
- `e-all-right` (1, 1 file): `E_ALL` is `30719` from PHP 8.4, which drops
  `E_STRICT`; PHPStan has `int`; `predefined-constants.php`.
- `loose-strict` (1, 1 file): a non-falsy string `== true` is `true` (PHP casts
  it to bool); PHPStan's `bool` is imprecise; `loose-comparisons.php`.
- `nullsafe-else-right` (1, 1 file): in the else of `$s?->p !== null`, `$s->p`
  is `null` either way (a `null` `$s` reads `null` with a warning); PHPStan
  keeps `Foo|null`; `nullsafe.php`.

**Suite setup** (3 mismatches, 3 files, 2 causes):

- `suite-context` (2, 2 files): passes analysed alone; in the suite project a
  global constant defined elsewhere too (`data/bug-8526.php` has `define('FOO',
  true)`) is `mixed`, and the gate analyses each file in its own project or
  scopes `define()` per file; `constant.php`, `constants.php`.
- `env-phpversion` (1, 1 file): `PHP_VERSION_ID` asserted over PHPStan's whole
  supported range; the pinned version gives `int<80500, 80599>`, an approved
  deviation as for `predefined-constants.php`; `node-callback-scope.php`.
