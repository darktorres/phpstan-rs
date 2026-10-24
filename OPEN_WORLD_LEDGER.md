# Open-world measurement ledger

Tracks the gaps listed in the answer of 2026-10-21 about the open-world cost measurement.
Design and measurements: `BY_REF_WARNING_AND_FIBER_DESIGN.md`
(section "Open-world cost, measured"). Corpus outputs and binaries: `~/.cache/phpstan-rs-scratch/adv/`.

Status values: OPEN (not started), IN PROGRESS, DONE (investigated; the row says where the write-up is), DECISION (needs the user).

Baseline note: the Piwigo and mago numbers recorded before commit `07adbd097` used the old arithmetic
fallback. Any comparison made before that commit's rerun is stale for the baseline.

## Measurement gaps

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| M1 | Stale baselines: Piwigo (586) and mago (5150) predate the `mixed + mixed` fix | DONE (rerun on HEAD: Piwigo 592, X7 589 lost 3 gained 0; mago 5159, X7 4994, lost 209 gained 44; see N1) | `corpus-base.json`, `mago-base.json` | Rebuild base and X7 from HEAD; rerun Piwigo and mago; recompute the diff |
| M2 | Mago snippets are not a valid precision corpus: eight files declare `class Collection` with different shapes, and `check-project` loads them as one project | OPEN | `method.notFound` losses sampled (`static_return_generic_override.php:82`, `default_parameter_inference.php`); no standalone repro on base | Run each snippet in isolation, or exclude files that share class names |
| M3 | Only one real project measured (Piwigo). No script-heavy corpus | OPEN | none | Find a second real project; add a script-heavy corpus |
| M4 | Runtime cost of mixing not measured | OPEN | none | `hyperfine -N` on base vs X7, warm cache, per the project timing rules (`nice -n 19`, note loadavg) |
| M5 | Fixture suite not run under open world | OPEN | none | Apply the X7 change in a scratch worktree; run the fixture suite; list the expectations that change |
| M6 | Two Piwigo `foreach` overwrite losses at `tests/Browser/ConsoleCleanTest.php:48` have no traced mechanism | OPEN | design doc: "mechanism untraced" (suspected loop convergence) | Trace the loop convergence for those two lines |
| M7 | Piwigo `cast.useless` loss at `tools/triggers_list.php:1083` | DONE | design doc classifies it as legitimate uncertainty | Re-verify once M1 reruns |

## Mago loss and gain classification

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| G1 | `method.notFound` lost (88 under X6 and X7) | OPEN | Sampled: cross-file `Collection` name collision, so the baseline is wrong | Classify all 88 after M2 |
| G2 | `argument.type` lost (67 under X7) | OPEN | not classified | Classify; separate real uncertainty from baseline false positives |
| G3 | `arguments.count` lost (10 under X7) | OPEN | not classified | Classify |
| G4 | `booleanAnd`, `greaterOrEqual`, `smallerOrEqual` `alwaysTrue` lost (15 under X7) | OPEN | not classified | Classify |
| G5 | `property.notFound`, `assign.propertyReadOnly`, `assign.propertyType`, `isset.variable`, `method.resultUnused`, `notIdentical.alwaysTrue`, `property.private`, `property.writeOnly`, `return.type` lost (small counts under X7) | OPEN | not classified | Classify in one pass after M2 |
| G6 | `argument.type` gained (36 under X6 and X7) | DONE | Traced to the `mixed + mixed` → `int` bug in `Type::binary_operation`; fixed in `07adbd097`; reproduced on base with `takes_array($m + $m)` | Confirm the 36 disappear after M1 |
| G7 | `variable.possiblyUndefined` gained (2 under X6 and X7) | OPEN | not examined | Find the two lines and trace them |
| G8 | `match.unhandled` gained (1 under X7; `mixed~(Unit::C1|Unit::C2)`) | DONE | Mixing makes the subject `mixed`, so exhaustiveness fails; this is the open-world precision cost | None beyond M1 rerun |

## Unmodelled channels and policy

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| P1 | Error handlers, fibers, `trigger_error`: not modelled | DECISION | design doc, deferred by-ref and fiber designs | Decide whether these are in scope now |
| P2 | Open world as default vs closed world as opt-in (D5) | DECISION | Piwigo: 3 lost, 0 gained; mago: unreliable (M2) | Decide after M1 to M4 |
| P3 | Int overflow ignored for unbounded `int + int` and `int * int` (stays `int`, PHP gives `float`) | DECISION | `numeric_result` keeps `int` for two ints, as before the fix | Decide whether unbounded int arithmetic should union in `float` |
| P4 | Unary minus on `mixed` returns no finding (conservative) | OPEN | probe `takes_array(-$m)` reports nothing | Decide whether to type unary minus like the binary fallback |
| P5 | Closed-world false positives: `call_user_func('setIt')`, and `array_map` with a closure that declares `global $y` | OPEN | design doc: confirmed on base; not counted on any corpus | Count occurrences on Piwigo and mago |

## Related, outside #2

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| R1 | `mixed + mixed` → `int` fallback in `Type::binary_operation` | DONE | commit `07adbd097`; fixtures in `int_range_arithmetic` | None |
| R2 | `WORK_STACK.md` does not list the deferred by-ref and fiber items | DECISION | project CLAUDE.md asks for paused fixes to be recorded there | Decide whether to add them |

## New from the rerun

| ID | Item | Status | Evidence so far | Next step |
|----|------|--------|-----------------|-----------|
| N1 | The fix's six Piwigo findings | DONE | three fixed in `9715b34c6` (`??` on an absent element); three fixed in `2c189db92` (`file()` now `list<string>|false` via PHPStan's function-level stub PHPDoc). Piwigo 583 = old baseline, 0 gained | None |
