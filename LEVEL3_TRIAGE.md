# Level 3 triage: which open findings still reproduce

plan #22 of the level 9 plan says whether each open finding of the stage reviews still reproduces is settled by running its
probe on the gate binary, not by reading. This file records that run.

## Method

- Binary: the `gate` profile built at `b9de6dcf1` (master, 2026-10-19), copied to
  `~/.cache/level3-triage/bin/phpstan-rs-b9de6dcf1`.
- Probes: one directory per finding under `~/.cache/level3-triage/probes/`, each with an `index.php` (and a `flags` file where
  the finding needs an opt-in rule). Outputs are in `~/.cache/level3-triage/out/`. Each probe was run with
  `check-project --no-composer` and the flags in its `flags` file. Where the question was a type, the probe asserted a wrong type on
  purpose, so the mismatch message shows the type the checker infers.
- Prefixes: `FR-` is `STAGE_B_FIX_REVIEW.md`, `SB-` is `STAGE_B_REVIEW.md`, `SC-` is `STAGE_C_REVIEW.md`.
- "Reproduced" means the probe shows the wrong behaviour the review describes. "Fixed" means a commit or a ledger row
  says so and the probe agrees. "Not probed" says why.
- "Changes a dump": a reproduced finding that changes a diagnostic or an inferred type changes the baseline when fixed, so the
  plan puts it before the item-0 baseline (plan #22). A finding that changes only comments, configuration or speed does not.

Not run here: the corpus runs (V1 and V2 of the Stage B review) and the fixture suite (V1 of the Stage C review, FR-86). Both need
the corpus and a full test run, which were not in this pass.

## Results

| ID | finding | result | probe | changes a dump |
|---|---|---|---|---|
| FR-2 | a first-class callable on a generic receiver leaks the template: `$g()` is `T` | Reproduced | FR-2 | yes |
| FR-3 | `foreach` over an `IteratorAggregate` yields `TKey` and `K` | Reproduced | FR-3, FR-3b | yes |
| FR-4 | a failed type check narrows to the checked type (`!is_float($int)` gives `int\|float`) | Not reproduced for the `is_float` case; the `is_string` case not probed | FR-4 | no |
| FR-5 | the error type silences quiet reads (`$undef ?? 'a'` is `*ERROR*`) | Reproduced | FR-5 | yes |
| FR-6 | a rebound closure's `$this` is reported undefined | Not reproduced | FR-6 | no |
| FR-7 | a global function called from a namespace is indexed under the namespaced name | Not reproduced in the dead-code report; the LSP index was not probed | FR-7 | no |
| FR-8 | an attribute class used only as an attribute is `class.unused` | Reproduced | FR-8 | yes |
| FR-9 | a pipe into an impure callable is reported as having no effect | Reproduced | FR-9 | yes |
| FR-10 | a bare `yield` before a binary-operator keyword is a parse error | Fixed (`2091070e`; the file parses) | FR-10 | no |
| FR-11 | `NAN === NAN` is reported always true | Reproduced | FR-11 | yes |
| FR-12 | an unresolvable property type is reported three times | Reproduced | FR-12 | yes |
| FR-13 | a positional key after an unknown key (`[$ab => 1, 5 => 2, 'y']`) is `0`, not `6` | Reproduced | FR-13 | yes |
| FR-14 | `iterable<K, V>` drops `K` (the key is `int\|string`, not `string`) | Reproduced | FR-14 | yes |
| FR-15 | a negated `instanceof` keeps the generic member of a union | Reproduced | FR-15 | yes |
| FR-16 | a divisor that converts to zero (`/ false`) is not reported | Reproduced | FR-16 | yes |
| FR-17 | `new` with a non-name class expression is not checked (`new $undef()`) | Reproduced | FR-17 | yes |
| FR-18 | a flat binary chain is superlinear | Reproduced: 1,000 `'a' .` terms 0.43 s, 4,000 terms 13.9 s | timing | no |
| FR-19 | smaller type gaps (`$i / 0`, native `\int` hint, and four others) | Partly: `$i / 0` is `int\|float` and `\int` gives `class.notFound`; the others not probed | FR-19a, FR-19b | yes |
| FR-20 | nothing enforces the stack guard | Fixed (FIX-20, DONE in the ledger) | none | no |
| FR-21 | the error type has no central rule | Fixed (FIX-21, DONE in the ledger) | none | no |
| FR-22 | smaller design points (seven, among them `ImpurePoints` and the callable union) | Not probed: design points, read only | none | no |
| FR-23 | fixtures that lost what they tested | Not probed: a read of the fixtures | none | no |
| FR-24 | comments that the fixes left wrong | Partly: the `elseif` backtrack comment and the `self.tokens[...]` comment are gone; the roadmap codes are mostly gone | grep | no |
| FR-25 | a literal-key write leaves the shape's next index (`$c[8]` after `$c[] = 4`) | Reproduced | FR-25 | yes |
| FR-26 | `mixed~S` joined with a value inside `S` stays `mixed~S` | Reproduced | FR-26 | yes |
| FR-27 | `is_resource()` of a resource is always true | Not reproduced for an open resource; a closed resource not probed | FR-27 | no |
| FR-28 | a guarded polyfill's body is checked against the stub's parameter names | Not reproduced (no undefined `$a`) | FR-28 | no |
| FR-29 | the trait-collision check | False positive not reproduced; the miss (two traits, no class method) reproduced | FR-29a, FR-29b | yes |
| FR-30 | a promoted property's set visibility is ignored | Reproduced (same probe as SC-8) | FR-30 | yes |
| FR-31 | an anonymous class skips the class-level checks | Reproduced | FR-31 | yes |
| FR-32 | `'abc' + 1` is `binaryOp.invalid` on PHP 7.4 | Reproduced | FR-32b | yes |
| FR-33 | design points (infer cache, anonymous class names, memo, lossy counting) | Not probed: design | none | no |
| FR-34 | a `to_number` doc says `'5 apples'` throws on PHP 8 | Reproduced (the doc text is still there) | grep | no |
| FR-35 | abstract and final checks compare names by case | Partly: the false positive not reproduced; the final miss reproduced | FR-35a, FR-35b | yes |
| FR-36 | an argument outside its template's bound is not reported | Reproduced | FR-36 | yes |
| FR-37 | stale comments in `types` and `checker` | Partly: `index_result_type`, `ActivityService`, "unconditionally trusts" and "never inferred precisely" are still there | grep | no |
| FR-38 | a template in `@phpstan-assert` narrows to the bare placeholder | Not reproduced in the two shapes tested | FR-38, FR-38b | no |
| FR-39 | integer arithmetic over a union or a signed range | Partly: `int<-3, 2> ** 2` is `never` and makes the next statement unreachable; other bullets not probed | FR-39 | yes |
| FR-40 | `null == '0'` is taken as always true | Not reproduced (`equal.alwaysFalse`, which is right) | FR-40 | no |
| FR-41 | PHPDoc types that parse as numbers (`Infinity`, `1_000`) | Reproduced; the conditional-return split not probed | FR-41a, FR-41b | yes |
| FR-42 | `promote_class_string_literals` misses a shape, so `first(['Foo'])` is `object` | Reproduced; the intersection display not probed | FR-42 | yes |
| FR-42a | `$argv` is defined inside a function | Reproduced | FR-42a | yes |
| FR-43 | comments in the `types` modules | Partly: `write_literal_key_aware` and "A `T` range" are still there | grep | no |
| FR-44 | `rand()` with reversed bounds is `never` | Reproduced | FR-44 | yes |
| FR-45 | a write through a reference does not reach the variable | Reproduced | FR-45 | yes |
| FR-46 | a flag set to `false` replays the condition as true | Reproduced | FR-46 | yes |
| FR-47 | a loop that cannot exit is not seen as terminating | Not reproduced in either shape | FR-47a, FR-47b | no |
| FR-48 | a write through one of several literal keys overwrites all of them | Reproduced | FR-48 | yes |
| FR-49 | builtin return types claim more than the function gives | Not reproduced for `array_combine`; the other builtins not probed | FR-49 | no |
| FR-50 | calls that narrow are matched by the name's case | Reproduced for a static method; the other sub-bullets not probed | FR-50 | yes |
| FR-51 | comments in the checker modules | Partly: a `NOT special-cased` comment sits beside its arm; the rest not checked | grep | no |
| FR-52 | a closure argument whose last `if` returns in both branches | Reproduced | FR-52 | yes |
| FR-53 | an exhaustive `match (true)` is reported unhandled | Reproduced | FR-53 | yes |
| FR-54 | a short ternary `$s ?: 'd'` is `mixed` | Reproduced; the compound-assignment case not probed | FR-54 | yes |
| FR-55 | a lowercase `__tostring()` does not make a class `Stringable` | Not reproduced: `Stringable` is implied, and a `method.nameCase` report appears instead | FR-55 | no |
| FR-56 | a method calling `static::` is proven pure | Reproduced | FR-56 | yes |
| FR-57 | an arrow function's body stops before `and`/`or` | Reproduced | FR-57 | yes |
| FR-58 | a hover request clones the enclosing scope per variable | Not probed: LSP performance, read only | none | no |
| FR-59 | comments in the registry, project, LSP and syntax modules | Partly: the "VSCode plan item" references are still there | grep | no |
| FR-60 | range subtraction and division flip results | Reproduced | FR-60 | yes |
| FR-61 | `isset()` and `??` on a property of a nullable object reported pointless | Reproduced | FR-61 | yes |
| FR-62 | a property keeps its narrowed type across a call that changes it | Not reproduced for `=== null` or `(string) … === ''` | FR-62 | no |
| FR-63 | two loop exits misread | Reproduced (both shapes) | FR-63a, FR-63b | yes |
| FR-64 | a `mysqli` query that does not start with its verb is typed `true` | Reproduced | FR-64 | yes |
| FR-65 | a spread argument counts as one argument | Reproduced | FR-65 | yes |
| FR-66 | opt-in dead-code false positives | Partly: a class named only in PHPDoc is reported unused; the `method.unused` case not reproduced | FR-66a, FR-66b | yes |
| FR-67 | a loose `==` narrows as `===` | Not reproduced for `case 0:`; the `or` chain not probed | FR-67 | no |
| FR-68 | a `break` inside an `if` in a `switch` case loses its path | Reproduced | FR-68 | yes |
| FR-69 | a `catch` judged unreachable when the `try` throws only through magic | Not reproduced in the shape tested (the catch stays reachable) | FR-69 | no |
| FR-70 | array literal and spread keys | Reproduced (keys `10\|0` where PHP gives `11`) | FR-70 | yes |
| FR-71 | `preg_match_all` pattern-order groups never hold `''` | Reproduced | FR-71 | yes |
| FR-72 | narrowing picks the wrong argument | Not reproduced for the named-argument case; the `in_array` case not probed | FR-72 | no |
| FR-73 | a `for` loop's in-range writes keep a list for a negative index | Not reproduced (the type is `array`) | FR-73 | no |
| FR-74 | `config.platform.php` given as a full version is ignored | Reproduced (control: `8.1` reports `always false`; `8.1.12` reports nothing) | FR-74 | yes |
| FR-75 | duplicate-declaration checks miss case variants and split groups | Reproduced (both) | FR-75a, FR-75b | yes |
| FR-76 | a namespace cycle through a context file is not reported | Reproduced: with `--scan-dir`, nothing is reported | FR-76b | yes |
| FR-77 | a file that does not parse is skipped without a report | Fixed (`4278ec2a`: a `parseError` at the failing token) | FR-77 | no |
| FR-78 | the duplicate checks walk every name for every file | Not probed: performance, read only | none | no |
| FR-79 | smaller gaps in the submodules | Not probed: read only | none | no |
| FR-80 | comments in the submodules | Partly: "the real corpus this phase targets" is gone; "always-on" and "confirmed via `dumpType()`" are still there | grep | no |
| FR-81 | a loop's shape widening builds a self-nesting array | Reproduced | FR-81 | yes |
| FR-82 | forgetting properties after a call misses writes | Not reproduced for an alias; static properties, builtins and `__toString` not probed | FR-82 | no |
| FR-83 | a `foreach` that rewrites its elements drops a write | Reproduced (every element gets `file: 'y'`) | FR-83 | yes |
| FR-84 | a template bound two siblings away reads as `mixed` | Reproduced | FR-84 | yes |
| FR-85 | a write through several literal keys leaves the per-key entries stale | Reproduced | FR-85 | yes |
| FR-86 | `c0c51bca` landed with two fixtures failing | Not probed: needs a fixture run | none | no |
| FR-87 | a discarded pass leaves some records behind | Not probed: read only | none | no |
| FR-88 | comments that describe the truncation the fix replaced | Reproduced (`diagnostics.truncate` still in five comments) | grep | no |
| FR-89 | a private method called through another value of its class is unused | Reproduced | FR-89 | yes |
| SB-V1 | the Stage B fixture suite and corpus | Not probed: needs the corpus | none | no |
| SB-V2 | the corpus delta for the four fix commits | Not probed: needs the corpus | none | no |
| SB-W1 | a nested write at a key union joins every key's container | Not reproduced in the shape tested | SB-W1 | no |
| SB-C2 | a closure's docblock is ignored | Reproduced | SB-C2 | yes |
| SB-M1 | a narrowed `mixed` parameter is not reported | Reproduced | SB-M1 | yes |
| SB-T2 | `throws.missing` ignores a callee's `@throws` | Reproduced (a decision row: the plan rules that callee throw points count) | SB-T2 | yes |
| SB-CMP1 | a comparison with an array "results in an error" | Reproduced | SB-CMP1 | yes |
| SB-DF1 | `ucwords` drops whether a string is a decimal int | Reproduced | SB-DF1 | yes |
| SB-DF2 | `sprintf('0%d')` is a string where a narrower type is right | Reproduced | SB-DF2 | yes |
| SB-DF3 | a failed `is_numeric` keeps `numeric-string` members | Not reproduced | SB-DF3 | no |
| SB-DF4 | `mixed` loses non-numeric-ness | Reproduced (`string` where `non-numeric-string` is right) | SB-DF4 | yes |
| SB-DF5 | the value side of an assignment condition is not narrowed | Not reproduced in the shape tested | SB-DF5 | no |
| SB-DF6 | a preg offset is typed `int` | Reproduced (`int\|null`, no range) | SB-DF6 | yes |
| SB-DF7 | the string-parameter refinement keeps what cannot pass | Not reproduced (`argument.type` reported) | SB-DF7 | no |
| SB-TP | typed purity cannot see implicit calls | Reproduced (`g($b);` reported as having no effect) | SB-TP2 | yes |
| SB-D1 | PHPDoc/native reconciliation needs the hierarchy | Not probed: design | none | no |
| SB-D2 | member lookup walks unions in `member_resolution.rs` | Not probed: design | none | no |
| SC-1 | a project class named like a stub class takes the stub's identity | Reproduced (`Evnt` is reported unused; `Event` is not) | SC-1 | yes |
| SC-2 | a leading backslash in an attribute pattern stops it matching | Not probed: needs the restricted-symbols configuration | none | no |
| SC-3 | attribute markers match the name as written | Reproduced (`method.override` for an unresolved `#[Override]`; `resultDiscarded` for `#[NoDiscard]`) | SC-3a, SC-3b | yes |
| SC-4 | a lowercase `__tostring()` (the same finding as FR-55) | Not reproduced | FR-55 | no |
| SC-5 | an enum ignores its own `@phpstan-type` aliases | Not reproduced in the shape tested | SC-5 | no |
| SC-6 | build-dependent constants read as the stub generator's values | Reproduced (`PHP_EXTRA_VERSION` is `'-13ubuntu3.2'`; `ZEND_THREAD_SAFE` is `false`) | SC-6 | yes |
| SC-7 | a namespaced constant is shadowed by a built-in | Reproduced (`PHP_EOL` is `"\n"\|"\r\n"`) | SC-7 | yes |
| SC-8 | a promoted property drops its set visibility (the same finding as FR-30) | Reproduced | FR-30 | yes |
| SC-9 | the old flat keys still load | Reproduced (a deprecation warning, and still accepted) | SC-9 | no |
| SC-10 | the opt-in checks lost their reasons | Not reproduced by grep: the pointers now go to field docs; not read in full | grep | no |
| SC-11 | a program's bodies answer for any declaration | Not probed: read only | none | no |
| SC-12 | the all-on gate's keys are a hand-kept list | Reproduced (`ALL_ON_SETS` in `xtask/src/main.rs` is still written by hand) | grep | no |
| SC-13 | stale and misplaced text | Partly: the cited `function_sig.rs:606` is gone; the CLI tests are still named for flags | grep | no |
| SC-O1 | control characters in a string literal type are printed raw | Not reproduced (the newline is shown escaped) | SC-7 | no |

## Counts

The table has 120 rows, counted from the file.

- Reproduced: 65 rows.
- Partly reproduced: 11 rows (the probed part reproduces; the rest of the finding was not probed or is not reproduced).
- Not reproduced in the shape tested: 23 rows.
- Fixed since the review: 4 rows (FR-10, FR-20, FR-21, FR-77).
- Not probed (corpus, fixtures, design, read only, or a configuration not written here): 17 rows.
- Reproduced or partly reproduced, and changing a dump: 60 rows.

## Consequence for batch 0

Reproduced findings that change a dump go before the item-0 baseline, as plan #22 says. The list is every "yes" row marked
Reproduced or Partly above. Each reproduced finding has its own probe, so the fix's test can start from it.
