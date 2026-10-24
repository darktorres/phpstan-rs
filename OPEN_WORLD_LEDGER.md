# Open-world ledger

The status of every item of `OPEN_WORLD_PLAN.md`, and of the measurements and decisions it
rests on. The plan holds the design; this file holds what is done, with its evidence, and
what is next. `WORK_STACK.md` level 10 points here.

Design and measurements: `BY_REF_WARNING_AND_FIBER_DESIGN.md` (section "Open-world cost,
measured") and `OVERFLOW_MEASUREMENT.md`. Corpus outputs and binaries:
`~/.cache/phpstan-rs-scratch/adv/`; O8 encodings and verdict files:
`~/.cache/phpstan-rs-scratch/spike/`.

Status values: OPEN (not started), IN PROGRESS, DONE (the row says where the evidence is),
DECISION (needs the user).

Baseline note: the Piwigo and mago numbers recorded before commit `07adbd097` used the old
arithmetic fallback. Any comparison made before that commit's rerun is stale for the baseline.

## Decisions

| ID | Question | Status | Evidence so far | Next step |
|----|----------|--------|-----------------|-----------|
| Q1 | The default world: open, or closed with open as opt-in (D5 of the design) | DECISION | Piwigo: 3 lost, 0 gained; vendored packages measured (M3); mago unreliable (M2) | Decide against the plan's four criteria after B and M4, M5 |
| Q3 | Error handlers, fibers, `trigger_error`: modelled now (group C) or later | DECISION | `BY_REF_WARNING_AND_FIBER_DESIGN.md`, decisions D1 to D5; the two failing by-reference fixtures need it | Decide before C |
| Q4 | Unbounded `int + int`, `int * int` give `float` | DONE | Decided as group O (2026-10-22); was ledger row P3 | None |
| Q5 | Size bound and `mode` default | DONE | Decided 2026-10-22: `[deployment] memory_limit`, else below 2^56 bytes; `mode` from composer `type: library`, else application (plan, O7 revised) | None |

## Group A: type fixes with no world dependency

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| A1 | `number` and the PHPDoc keywords PHPStan has and the parser lacks | OPEN | `number` gives `class.notFound` and a false `argument.type` (ReactPHP `Transaction::applyTimeout`, reproduced on `0566039d1`) | Add to `type_keywords!`, audit PHPStan's `TypeNodeResolver` list one by one, a fixture each |
| A2 | Unary `-`, `+` on `mixed` give `int\|float`, `~` gives `int\|string` | OPEN | probe `takes_array(-$m)` reports nothing (was ledger row P4) | Type them like the binary fallback |
| A3 | `count()` of a `Countable` | DONE (dropped) | `72854633c`: the interface's non-negative contract holds, a negative is reported at the implementation's return | None |
| A4 | `abs()`, unary minus, `++`, `--` at the limits | DONE | `64dafa626` | None |

## Group O: integer overflow

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| O1 | Exact detection in i128 | DONE | `64dafa626`; right shifts `0e8dea09b`; string comparisons within 32 bits `18b86baef`; `intdiv`, `min`/`max` `7fbcfa490` | None |
| O2 | Runtime limits as ranges | DONE, except database bounds | `count()` `bbdd45af9`; `[deployment]` platforms `793bbd50f`; string sizes `84e5f7838` | Database bounds: O7.7 |
| O3 | Counters | IN PROGRESS | loop locals `30c24ab03`; statics `94560a44b`; one step per execution `1fffa1ffe` | Private properties, by-reference captures, array elements (`$counts[$k]++`) |
| O4 | Provenance: data-flow nodes in types | DONE | `c6c9c2a07`, `5f76bcc3e`, `2e1979285`, `82ce8aeb9`, `46d9b86fa`, `af84c4085` | None |
| O5 | Environment sources | IN PROGRESS | `bab1509d0` (files, streams, environment, ini, databases, `$argv`, PSR-7, Symfony, Laravel, `[sources]`); `foreach` bound `35009ddbb`; measured against `c6c9c2a07`: 10 gained, 28 lost, every loss a false positive | Framework-injected controller, route and console parameters; dispatch tables (WordPress XML-RPC `addTwoNumbers`) |
| O6 | The marked overflow float and its sinks | DONE | `64dafa626`, `3ce4367a2`, `7fbcfa490` | None |
| O7 | Contexts across calls, objects and files | DONE, with the gaps O7.1 to O7.8 | `1d09e2dc1`, `1fffa1ffe`; measured: 23 gained, each read, all true but O7.5's two | The gaps |
| O8 | Relational domain | IN PROGRESS (design) | The plan's O8 item: the design, its validation, the solver matrix and the spike results | O8.1 to O8.7, then the design for approval |
| O9 | Library mode: contracts | OPEN | | After O8; the `mode` setting comes with it |
| O10 | Coverage: unresolved edges counted | OPEN | | After O9 |

### Overflow measurements

- O4 and O5, against `c6c9c2a07` (`adv/compare-src.py`): 10 findings gained and 28 lost,
  every loss a false positive removed. Gained true positives: six request-driven Piwigo
  `TypeError`s and WordPress `widgets-form.php:196`. The Piwigo `Menu/BlockManager.php:101`
  false positive is fixed by the `foreach` bound. Still missed: Piwigo
  `ImageGetController.php:150` (a request value joined with a property is deferred, as flows
  are per type family, not per path) and `HistorySearchController.php:206`, Laravel's
  `firstItem()`, monolog and Psalm memory shorthand (all need O7), and WordPress
  `addTwoNumbers` (needs the XML-RPC dispatch table).
- O7, against `35009ddbb` (`fe`), Piwigo and the 13 vendor packages: the first pass gained
  864 findings and lost none; 812 of them named no source, from the cycle rule (loops
  without an iteration bound taken as unbounded, shift and mask carries widened to the whole
  range), `(int)` of a float, and the heap model (one object per `new`, one class-wide value
  for any object the code doesn't show). Callgrind on Piwigo: 35.95 G at `fe`, 20.47 G with
  the interner fix (`af84c4085`); the serial thread 0.20 G at `fe`, 0.71 G. With contexts,
  objects, the growth identifier and the sink rules: 20.86 G, serial thread 0.82 G; 23
  findings gained (Piwigo 5, Psalm 1, Laravel 1, WordPress 10, Drupal 5), each read: all
  true but getID3's two (gaps below); lost the WordPress `widgets-form.php:196` false
  positive (`-1` of `PHP_INT_MIN` rounds to `-2^63`, which `array_splice()` takes) and a
  Symfony test's growth finding, now opt-in.

### O7 gaps

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| O7.1 | A site in vendor code is not reported at the first project frame | OPEN | vendor files are not checked, so they record no facts | Record facts for vendor bodies the project reaches |
| O7.2 | In the LSP, a caller's edit does not recheck the callee file whose finding it changes | OPEN | callers are not their callees' dependents | Make the sink files dependents of the facts that reach them |
| O7.3 | The `mode` setting | OPEN | | With O9 |
| O7.4 | An operation with no sourced operand is not a node: `$x = 1; $x *= 1 << 40` in a loop is not judged; a loop count the environment chooses is not taken as the environment's | OPEN | | |
| O7.5 | A property written under a test of the same object's state takes the other state's values | OPEN | WordPress `getid3.php:2259`, `:2352`, both false | Keep values per state of a boolean property of `$this` between calls that don't change it |
| O7.6 | The path a finding names can pass through a merged value | OPEN | Laravel `Pluralizer.php:48` names test fakes; the real path is `ClearCommand`'s `clear()` count | Name the path that runs |
| O7.7 | Database values are unbounded: no schema or driver bound; `COUNT(*)` not known non-negative | OPEN | Piwigo tests that add to a fetched count or a `smallint` column | Schema and driver bounds (O2's database part) |
| O7.8 | A value bounded only by its relation to another grows as the run allows | OPEN | commonmark `Cursor`, a stream wrapper's `$position += strlen($chunk)`, tree counts; reported as `arithmetic.intOverflowGrowth`, off by default | O8 |

### O8 spikes (Linux)

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| O8.1 | Tag processor through the automatic translator, heap model and slicing, all 233 sites | IN PROGRESS | Two slicer defects found and fixed (plan, "Slicing must keep every clause sound"); the run with them was void. Translator fixes since: string lengths bounded at every predicate, `usort` calls its comparator with two members, protected methods are entries | Rerun all 233 sites. The heap toy under the fixed slicer: `H::end_of` safe in 227 s, `H::sorted` unknown at 3e8, `G::end_of` a counterexample (correct) |
| O8.2 | The cursor fields' bound needs a pairwise invariant over `lexical_updates` (no two replacements overlap, all within the document) | DECISION | `bytes_already_parsed += $shift` in `apply_attributes_updates` is the one write not within the document; one tracked element cannot state a pairwise property | Decide whether the domain carries pairwise collection invariants (two tracked elements) or these stay unproven |
| O8.3 | `WP_HTML_Processor`'s writes to the parent's `bookmarks` (`new WP_HTML_Span(0, 0)`, `(here->start, 0)`, also on another instance) | OPEN | not in the encoding | Model the subclass's writes as entries of the hierarchy |
| O8.4 | sodium X25519 ladder (bit-vector and integer encodings, XOR-mask swaps) | OPEN | `spike/ladder/ladder_bv.smt2`, `ladder_lia.smt2` | Run on Z3, Golem, Eldarica |
| O8.5 | Scale: `WP_Query::get_posts` (168 variables) | OPEN | | Translate and solve |
| O8.6 | Solver portfolio and budget | OPEN | the solver matrix in the plan | Settle after O8.1, O8.4, O8.5 |
| O8.7 | Windows and macOS: Z3 build and determinism | OPEN | Linux only | After Linux |

## Group B: the global-write fact

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| B1 | Summary facts for global writes | OPEN | | |
| B2 | `GlobalWrites` demand answer | OPEN | | |
| B3 | Builtin verdict, one shared `CODE_RUNNING_PARAMETER_TYPES` | OPEN | | |
| B4 | The checker hook beside `forget_members_a_call_may_change` | OPEN | | |
| B5 | The `world` setting | OPEN | default per Q1 | |
| B6 | By-reference captures and closure scope | OPEN | | |
| B7 | Includes resolved, top-level writes summarized | OPEN | | |

B may be taken before O8's implementation.

## Group C: error handlers and fibers

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| C | One "may run other code" summary per function; handler warnings and fiber suspends | OPEN | Design D1 to D5 in `BY_REF_WARNING_AND_FIBER_DESIGN.md`; closes `checker/by_ref_capture/a_by_ref_capture_written_by_an_error_handler_is_read_after_the_warning` and `…/d_by_ref_capture_of_a_global_sees_a_script_write_after_a_fiber_suspend` (`aff0b8a31`, `3544f1096`) | After B and Q3 |

## Measurements of the open-world cost

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| M1 | Stale baselines: Piwigo (586) and mago (5150) predate the `mixed + mixed` fix | DONE | Rerun on HEAD (2026-10-21): Piwigo 592, X7 589, lost 3, gained 0; mago 5159, X7 4994, lost 209, gained 44 (G rows) | None |
| M2 | Mago snippets are not a valid precision corpus: eight files declare `class Collection` with different shapes, and `check-project` loads them as one project | OPEN | `method.notFound` losses sampled (`static_return_generic_override.php:82`, `default_parameter_inference.php`) | Run each snippet alone, or drop the colliding files |
| M3 | Real corpora beyond Piwigo | DONE | On `0566039d1`, `check-project --no-composer` on each `src`: Piwigo 583 to 580 (3 lost, 0 gained); monolog, commonmark, guzzle, doctrine-collections, rector-src, doctrine-orm, psalm, flysystem, Symfony Console unchanged; reactphp 4 lost; laravel 1 lost; drupal 28 lost; WordPress 342 lost, 8 gained (top-level variables mixed before calls into other files) | B7 is where the WordPress cost goes down |
| M4 | Runtime cost of mixing | OPEN | none | `hyperfine -N` base against X7 on Piwigo and laravel, warm, `nice -n 19`, loadavg noted |
| M5 | Fixture suite under open world | OPEN | none | Apply X7 in a scratch worktree; list the expectations that change |
| M6 | Two Piwigo `foreach` overwrite losses at `tests/Browser/ConsoleCleanTest.php:48` have no traced mechanism | OPEN | suspected loop convergence | Trace the loop convergence for those two lines |
| M7 | Piwigo `cast.useless` loss at `tools/triggers_list.php:1083` | DONE | legitimate uncertainty (design doc) | None |
| M8 | Flysystem and Symfony losses | DONE | an artifact: trait method bodies inherited the top-level scope, so X7 mixed every local; fixed in `0566039d1`; Drupal fell from 2928 lost to 28 | None |
| M9 | PHPStan's stub PHPDoc overlay | DONE | `2c189db92`: Piwigo 592 to 583, lost 9 false positives, gained 0; mago 5159 to 5158, lost 8 (3 false positives, 5 message-only), gained 7 (2 new: `is_a` on a string, `preg_replace`'s `return.unusedType`; 5 message-only) | None |
| N1 | The fix's six Piwigo findings | DONE | three fixed in `9715b34c6` (`??` on an absent element); three in `2c189db92` (`file()` as `list<string>\|false`); Piwigo 583 = old baseline, 0 gained | None |

## Mago loss and gain classification

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| G1 | `method.notFound` lost (88 under X6 and X7) | OPEN | Sampled: cross-file `Collection` name collision, so the baseline is wrong | Classify all 88 after M2 |
| G2 | `argument.type` lost (67 under X7) | OPEN | not classified | Separate real uncertainty from baseline false positives |
| G3 | `arguments.count` lost (10 under X7) | OPEN | not classified | Classify |
| G4 | `booleanAnd`, `greaterOrEqual`, `smallerOrEqual` `alwaysTrue` lost (15 under X7) | OPEN | not classified | Classify |
| G5 | `property.notFound`, `assign.propertyReadOnly`, `assign.propertyType`, `isset.variable`, `method.resultUnused`, `notIdentical.alwaysTrue`, `property.private`, `property.writeOnly`, `return.type` lost (small counts under X7) | OPEN | not classified | Classify in one pass after M2 |
| G6 | `argument.type` gained (36 under X6 and X7) | DONE | the `mixed + mixed` → `int` bug, fixed in `07adbd097` | None |
| G7 | `variable.possiblyUndefined` gained (2 under X6 and X7) | OPEN | not examined | Find the two lines and trace them |
| G8 | `match.unhandled` gained (1 under X7; `mixed~(Unit::C1\|Unit::C2)`) | DONE | mixing makes the subject `mixed`: the open-world precision cost | None |

## Other open-world rows

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| P5 | Closed-world false positives: `call_user_func('setIt')`, and `array_map` with a closure that declares `global $y` | OPEN | confirmed on base; not counted on any corpus | Count occurrences on Piwigo and the vendored packages |
| R1 | `mixed + mixed` → `int` fallback in `Type::binary_operation` | DONE | `07adbd097`; fixtures in `int_range_arithmetic` | None |
| R2 | `WORK_STACK.md` lists the deferred by-reference and fiber items | DONE | level 10, "Queued, after the O8 design is settled" | None |
