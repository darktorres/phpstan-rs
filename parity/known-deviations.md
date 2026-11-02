# Known deviations: places where PHPStan is wrong at runtime

The port matches PHPStan in every case listed here, and the scoreboard tests pass that way. Phase 4 decides what to do with each entry.

Each row has a key, the PHPStan behaviour, what `php` does, and its source. The source is a finding id in `RESTART_FINDINGS.md` at `legacy-final`, which names the legacy file and section with the full evidence.

**Seeded 2026-11-01 from the legacy.** A seeded row is a candidate until it is re-checked against the frozen `vendor/phpstan-src` and `php`. When the port reaches a row, it is confirmed or deleted in that commit.

## Builtin return values (CRATE_SPLIT_CAUSES Appendix G, CA-3)

| key | PHPStan | runtime | source |
|---|---|---|---|
| `vsprintf-empty-args` | `vsprintf('foo', [])` typed as failing | returns `'foo'` (php 8.5) | CA-3 |
| `array-flip-duplicates` | `array_flip` shape keeps duplicate values as distinct keys | later keys overwrite earlier ones | CA-3 |
| `get-class-never-false` | `get_class()` can return `false` | never `false` on PHP 8 | CA-3 |
| `array-fill-negative-start` | negative start index follows the PHP 7 rule | keys `-5, -4, -3` for `array_fill(-5, 3, 0)` (php 8.5) | CA-3 |
| `ini-get-auto-prepend` | `ini_get('auto_prepend_file')` type | real value type differs | CA-3 |
| `filter-var-null-bool` | `filter_var(null, FILTER_VALIDATE_BOOLEAN, FILTER_NULL_ON_FAILURE)` | returns `false` (php 8.5) | CA-3 |
| `mb-substitute-character-null` | `mb_substitute_character(null)` type | real return differs | CA-3 |
| `strval-array` | `strval([])` type | returns `"Array"`, with an "Array to string conversion" warning (php 8.5) | CA-3 |
| `preg-split-invalid-pattern` | invalid pattern and null flags typed as a list | returns `false` | CA-3 |

## Analyser and narrowing (Appendices H and J, EV-4)

| key | PHPStan | runtime | source |
|---|---|---|---|
| `array-diff-empty` | `array_diff*($a, [])` loses `$a`'s precision | returns `$a` unchanged | CA-3 |
| `match-no-default-after` | no narrowing after a `match` without `default` | an unmatched value throws, so code after it is narrowed | CA-3 |
| `bcmath-number-eq-null` | `BcMath\Number == null` | runtime comparison result differs | CA-3 |
| `e-all-84` | `E_ALL` value | 30719 from PHP 8.4 (php 8.5 prints 30719) | CA-3 |
| `non-falsy-eq-true` | a non-falsy string `== true` not always true | always true | CA-3 |
| `assert-assignment-maybe` | an assignment inside `assert()` defines its variable | Maybe: `zend.assertions` can skip it | EV-4 |
| `byref-use-sees-all-writes` | a by-reference `use` sees only some enclosing writes | it sees every enclosing write | EV-4 |
| `eval-defines-variables` | `eval` does not define variables | it can define any of them | EV-4 |

## Rules (Appendix I's 124 runtime-approved findings; CA-3)

| key | PHPStan | runtime | source |
|---|---|---|---|
| `private-set-never-written` | no report | the property can never be written | CA-3 |
| `catch-throwable-after-throwable` | no report | the second catch is dead | CA-3 |
| `var-above-return` | `@var` above `return` accepted | the tag has no effect | CA-3 |

The other ~120 approved rows are listed in `CRATE_SPLIT_CAUSES.md` Appendix I at `legacy-final`. Add them here as the rules port reaches each one.

## Types and PHPDoc (SB-4, L9-9)

| key | PHPStan | runtime | source |
|---|---|---|---|
| `template-bound-any-type` | `callable` and `class-string` template bounds rejected | any value type is a valid bound | SB-4 |
| `phpdoc-number-lexeme` | `0x1F` and `017` in PHPDoc read via `(int)` casts as 0 and 17 | PHP source reads them as 31 and 15 | L9-9 |
| `global-summary-null` | a global read in a function is `mixed` | it includes `null` (the function may run before the assignment) | EV-2 |

## Locator (PO-1)

| key | PHPStan | runtime | source |
|---|---|---|---|
| `class-not-in-autoloader` | a class its locators can't find is `class.notFound` | the class may exist (a second class in a shared file, or a file the autoloader doesn't list) | PO-1 |

## Parser

PHPStan never finishes on these inputs, which a port can't match. The port reports the parse error `php` gives, and the generated parser input leaves them out, because no extractor survives an endless loop.

| key | PHPStan | runtime | source |
|---|---|---|---|
| `prefix-incdec-new-hangs` | `++new A;` and `--new A;` never finish parsing: nikic/php-parser v5.9.0's error recovery (`ErrorHandler\Collecting`, which `RichParser` and `SimpleParser` use) loops forever; without a collecting handler it throws at once. `phpstan analyse` 2.3.4 on a two-line file ran past 60 s with no output. `++(new A)` and `--(new A)` finish | `php -l`: "syntax error, unexpected token ";", expecting "("" (php 8.5.4) | probed 2026-11-01 |
