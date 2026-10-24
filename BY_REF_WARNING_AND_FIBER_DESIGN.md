# By-reference state at control transfers: error handlers and fibers

Two deferred fixtures in `checker/by_ref_capture` need the checker to see that
control leaves the function and that other code writes a by-reference capture
before the caller reads it:

- `a_by_ref_capture_written_by_an_error_handler_is_read_after_the_warning`
- `d_by_ref_capture_of_a_global_sees_a_script_write_after_a_fiber_suspend`

Both are the same problem: a point where unknown-to-the-caller code runs (a
warning calls the error handler; a suspend returns control and a later resume
runs the rest of the body). This document states the facts found in the code,
then the proper model for each, then the decisions that remain.

## Facts

Checked against the tree at `4c713b638`.

- **Error handlers are not modelled.** `set_error_handler` and
  `restore_error_handler` appear nowhere in the checker. The only mention is
  the callable-takers list in `must_invoke.rs`.
- **There is no warning event.** Every diagnostic goes through `Checker::report`
  (`checker.rs:1576`), which is a sink for findings. It is not a runtime event:
  it is dropped while `reports_are_dropped()` (dynamic variable constructs), and
  the same identifiers cover static findings and runtime warnings.
- **Warning-raising identifiers.** `variable.undefined` (18 sites),
  `argument.type` (26 sites; covers both user-function TypeErrors, which are
  exceptions and do not reach a handler, and builtin null-to-scalar
  deprecations, which do), `function.deprecated` (17), `offsetAccess.notFound`
  (2, plus `report_missing_offset`).
- **`offsetAccess.notFound` is a definite finding.** It is reported when
  `container.has_offset_value_type(dim).no()`, that is when the key is known to
  be absent. A definite warning can therefore be a strong update.
- **Fibers are not special-cased.** The `Fiber` class comes from the phpstorm
  stubs (`vendor/phpstorm-stubs/Core/Core_c.php`): `start(mixed ...$args)`,
  `resume(mixed $value = null)`, `throw`, `isStarted`, `getReturn`, and
  `static suspend(mixed $value = null)`. A closure passed to `new Fiber(...)` is
  an argument of a `New`, so its by-reference captures are read as escapes
  (`closure_flow.rs`, `escape_uses`), and the literal is never followed.
- **Current output on both fixtures is a false positive.**
  - Fiber: `6:12 argument.type ... expects string, null|'str' given.` The `null`
    part is impossible: the fiber resumes only after the script's `$x = 'str'`.
  - Handler: `4:43 argument.type ... expects string, 'str'|null given.` The
    `null` part is impossible: the warning on line 6 always runs the handler
    before `$f()` on line 7.
- **PHP truth.** Both fixtures run clean apart from the offset warning (handler)
  and nothing (fiber). Verified with `php -d error_reporting=E_ALL`.
- **Reusable machinery.** Followed closures already have call-mode checking with
  by-reference write-back (`check_tracked_closure_calls`, `closure_flow.rs`), and
  a call's writes to globals come from `global_writes.rs`. Both designs reuse
  these; neither needs a new evaluator.

## Error handler

### Model

1. **Handler state is a flow fact.** `set_error_handler(h, mask)` pushes `h` and
   its level mask; `restore_error_handler()` pops. Inside a function body the
   handler installed at call time is not known, so it is the union of every
   handler the program can install (and "no change", since the default handler
   has no effect on locals). A non-literal argument makes the set unknown.
2. **Two kinds of warning point.**
   - *Definite*: a finding the checker is certain PHP raises (the
     `offsetAccess.notFound` case, a read of a variable known to be undefined).
     The handler's writes apply as a **strong update**, because the warning
     certainly runs the handler.
   - *Possible*: any other operation that may warn. The post-state is the union
     of "the handler ran" and "it did not". This is where the sound model needs
     a choice (decision 1 below).
3. **The handler's effect is its call-mode check.** At a warning point, each
   candidate handler is checked with `check_tracked_closure_calls` and the
   arguments typed from its declared parameters (`int $n`, `string $s` gives
   `int` and `string`). Its by-reference write-back is the effect. The mask
   filters candidates by level; an unknown mask is a possible call.
4. **Literal handler is followed.** The handler literal is read as a handler read
   (new `Read::Handler`), settled when every installation is a modelled
   `set_error_handler`. Otherwise it escapes, as today.

### What the fixture needs

Line 6, `$v = $arr['k']`, is a definite warning. The handler's effect (`$x =
'str'`) is applied as a strong update there, so line 7's `strlen($x)` sees
`'str'` only.

### Decisions

- **D1. Possible warnings.** Every operation that may warn merges the handler's
  effect into its post-state (sound; touches every expression), or only the
  modelled sites do (cheap; misses warnings the checker does not model).
  **Recommendation:** merge at every operation that may warn, with the merge
  limited to variables the handler can write, so the cost is proportional to the
  handler's write set. The cheap option is unsound: a warning the checker does
  not model would leave a stale value.
- **D2. Mask and restore.** Model both (recommended), since a handler that is
  not active at a warning must not run there.

## Fiber

### Model

1. **Fiber literal.** `new Fiber(<closure>)` where the argument is a closure
   literal, or a variable whose writes are all that literal. The literal is
   followed when every use of the holder is `start`, `resume`, `throw`,
   `isStarted` or `getReturn` on that fiber, and no other escape. Otherwise it
   escapes, as today.
2. **Suspend points.** A call to `Fiber::suspend()` is a suspend point. A call
   to a function or method that may suspend is too. Computing "may suspend" is
   an interprocedural summary, like `Invocations::summary`: a body may suspend
   if it calls `Fiber::suspend` or calls a function that may suspend. Recursion
   is conservative (may suspend). A dynamic or unresolved call may suspend.
3. **Segments.** The body splits at its suspend points into segments. Segment
   0 runs at `start(...)`. Segment `k` runs at the `k`-th `resume(...)` on that
   fiber, in the state the resume call sees. Fiber-local variables carry over
   from the suspend point, as the fiber's own frame. Globals and by-reference
   captures come from the caller's state at the resume call.
4. **Caps.** More resume sites than a small bound, or a fiber that escapes, makes
   the shared variables read after a suspend `mixed` (sound, imprecise).

### What the fixture needs

Segment 0 is empty up to `Fiber::suspend()`. Segment 1 (`strlen($x)`) runs at
`$fiber->resume()` on line 10, where `$x` is `'str'`, so no diagnostic.

### Decisions

- **D3. Suspend precision.** Segment model (recommended, precise) or havoc shared
  variables after every suspend point (sound, imprecise). The havoc option would
  pass this fixture only because it makes `$x` `mixed`, which is a weaker claim
  than the fixture's.
- **D4. Interprocedural suspend.** Include the may-suspend summary (recommended).
  Without it, a suspend inside a helper function is invisible, which is unsound.

## Shared mechanism

Both designs need the same thing: a point where code the caller does not see may
run and write by-reference state. Warning points (handler) and suspend points
(fiber) are two producers of it. The proper implementation is one summary per
function, *may run other code* (covers handlers and suspends), and one rule at
each call or operation that has it. Build the summary first; the two models then
differ only in what they write.

## Proposed order

1. Shared "may run other code" summary, with tests that exercise both kinds.
2. Error handler: definite warnings (strong update), then possible warnings under
   D1, then the mask and restore.
3. Fiber: literal recognition and escape rule, then the may-suspend summary
   (already in 1), then segments.

The fixtures change as follows. Handler: the expectation is the offset finding
alone, as committed. Fiber: no diagnostics, as committed.

## What I have not checked

- How many operations would have to merge the handler's effect under D1, and
  whether the checker already visits each one through a single path. The
  `argument.type` sites alone are 26, so this is the largest cost.
- Whether `Read::Handler` interacts with `Read::Passed`/`Invoked` when a handler
  literal is also passed to a must-invoke parameter.
- Whether fibers started in one function and resumed in another need the
  cross-function state to be the global state, and how `getReturn` should
  type.

## Adversarial validation

Each claim above was tested against the checker (probes via `check-project
--level 9`) or against PHP 8.5 (`php -d error_reporting=E_ALL`). Scratch files
are in `~/.cache/phpstan-rs-scratch/adv/`.

### Confirmed

- No `Fiber` or `set_error_handler` / `restore_error_handler` handling in the
  checker. `Fiber` appears only as a name in the `ReflectionFiber` list
  (`phpstan_api.rs:345`); `set_error_handler` only in the callable-takers list
  (`must_invoke.rs`).
- `offsetAccess.notFound` is a read-side finding. Its only caller is the rvalue
  `Index` arm of `check_expr` (`expr.rs:3060`). Writes to missing keys (`$a['k']
  = 1`, `$a['j']['x'] = 2`, `$a['m'][] = 3`) report nothing, and PHP warns for
  none of them.
- `isset`, `??` and `empty` get their own identifiers (`isset.variable`,
  `isset.offset`, `nullCoalesce.variable`, `nullCoalesce.offset`, `empty.variable`).
  They are not reported as `variable.undefined` or `offsetAccess.notFound`. So the
  definite-warning identifiers do not fire where PHP is silent.
- Warning classification is needed. A user function given `null` throws
  `TypeError`, which the handler never sees. A builtin given `null` raises
  `E_DEPRECATED` (8192), which the handler does see. The checker uses the same
  identifier, `argument.type`, for both.
- `@` does not stop the handler (PHP calls it). The mask filters by level. A
  handler replaced by a later `set_error_handler` stops being called.
- Both fixtures are false positives today. The fiber probe reports
  `null|'str'`, and the handler probe reports `'str'|null`.

### Refuted or corrected

- **Fiber state source.** The design says by-reference captures come from "the
  caller's state at the resume call". That is wrong for a capture shared with
  the creator's frame. In PHP the write comes through a second closure created
  in `make()`:

  ```php
  function make(): array {
      $x = 'a';
      $set = function (string $v) use (&$x): void { $x = $v; };
      $fiber = new Fiber(function () use (&$x): void {
          Fiber::suspend();
          echo strlen($x), "\n";
      });
      $fiber->start();
      return [$fiber, $set];
  }
  [$fiber, $set] = make();
  $set('hello');
  $fiber->resume();   // prints 5
  ```

  The resume caller never touches `$x`. The model must follow the reference
  through the creator's frame, not through the resume site.
- **Fiber literal reads.** The design says the by-reference captures of a closure
  passed to `new Fiber` are read as escapes. They are read as `Read::Captured`
  for the literal, which never settles because the literal is never followed.
  The net effect is the same, but the description should say so.
- **Handler cost.** The design says the cost of merging the handler is
  "proportional to the handler's write set". The call-mode check runs the whole
  handler body for diagnostics, so one check per operation costs the body size
  times the number of operations. That is reasoning, not a measurement. The
  write effects need their own summary, separate from the diagnostic check.
- **Shared summary.** The design says one "may run other code" summary serves
  both producers. For warnings it does not help: nearly every operation may warn,
  so the summary is trivially true. The handler needs which handlers can be
  installed and what they write. The fiber needs which calls may suspend. These
  are different summaries.
- **Coupling with creation diagnostics.** Making the handler followed drops its
  creation-mode diagnostics as soon as one call is checked. Today the handler body
  is reported (probe: `strlen(null)` in a handler that never runs). Under
  "modelled sites only" that would silently lose the report, which makes that
  option unsound. This ties decision D1 to whether the handler is followed.

### New gaps

1. **Installation outside the checked set.** The design takes "every handler the
   program can install" as a closed set. Framework or vendor code loaded but not
   checked can call `set_error_handler`. In a real project that is the common case,
   so the unknown-handler fallback decides most results. This needs an explicit
   decision (decision 5 below).
2. **`trigger_error` and `user_error`.** `trigger_error` is not modelled
   (`grep` finds it only in the stubs). PHP runs the handler from it with
   `E_USER_WARNING`. It is a definite warning site with a known message and
   belongs in the model.
3. **Handler that throws.** A common pattern is a handler that throws
   `ErrorException`. PHP probe: `f()` returns `-1` through the `catch`. The call-
   mode check returns the closure's type (`check_closure_literal` returns `Type`),
   so it cannot say the body exits. The warning point then needs a "may throw"
   edge that ends the path.
4. **Chained handlers.** `set_error_handler` returns the previous handler. If the
   handler calls a stored previous handler, that is an unknown call inside the
   handler. Documented PHP behaviour; I did not probe it.
5. **Fiber `throw()`.** `$fiber->throw(e)` resumes the body at its suspend point
   with an exception. PHP probe: the `catch` inside the body runs, then the rest
   (`caught`, `rest`). The segment model needs a throw-resume edge.
6. **Unresumed fibers.** A fiber started and never resumed never runs its later
   segments. The design says nothing about what state to check those segments
   with, and checking them in the creation state could report code PHP never
   reaches.
7. **Resume count.** "Segment `k` runs at the `k`-th resume" counts resume
   sites. A single `resume()` inside a loop or a branch runs an unknown number of
   segments. The model needs the set of segments each site may run, not a
   count, and the cap rule does not cover one site in a loop.
8. **`getReturn`.** Still open.

### Revised decisions

- **D1 (possible warnings), revised.** Merge the handler's write effects at every
  operation that may warn, using the separate write-effect summary. The
  diagnostic check then runs only where the handler is followed. This is the only
  sound option, and it fixes the creation-diagnostic coupling.
- **D2 (mask and restore).** Keep. Also model `@` as not suppressing the handler.
- **D3 (suspend precision).** Keep the segment model, with the creator-frame state
  (correction above), a throw-resume edge (gap 5), and a per-site segment set
  (gap 7).
- **D4 (interprocedural suspend).** Keep. Without it a suspend in a helper is
  invisible.
- **D5 (new): scope of installation.** Choose one of:
  - *Closed world:* only installations in the checked files count. Precise, but
    unsound for code outside the set.
  - *Open world:* any warning may run any handler installed outside the set, so
    every by-reference-shared variable is havocked at each warning point. Sound,
    and imprecise in real projects.

  **Recommendation:** open world, stated as a limit of the checked set, with the
  closed-world result offered as an opt-in for projects that check everything they
  load. The fixtures are closed-world, so both options pass them.

### Verdict

The design is not sound as written. The corrections are: the creator-frame state
for fibers, the separate write-effect summary, the coupling with creation
diagnostics, the throw edges, the per-site segment sets, and the open-world
decision. The two fixtures remain achievable under either scope option, because
their programs are closed.

## Open-world cost, measured

Scratch binaries and outputs: `~/.cache/phpstan-rs-scratch/adv/` (`bin/`, `corpus-*.json`,
`diffjson.py`). Corpus: piwigo17-frozen with its `phpstan-rs.toml`, 2792 files,
baseline 586 diagnostics (release build at `4c713b638`). Experiments were
uncommitted and are reverted; the tree is clean.

- **X1, mix at non-builtin calls, function imports and top-level variables:**
  586 to 586, no change. (Probe confirmed the path runs: a `--scan-dir` callee
  that writes a global removes the baseline false positive.)
- **X3, mix before every statement, `any_variable_may_exist` set at top level:**
  586 to 612: 1 lost, 27 gained. All 27 gained were `foreach.valueOverwrite` /
  `keyOverwrite` / `for.variableOverwrite`, an artifact of `insert` making
  maybe-undefined variables defined.
- **X4, same, but maybe-undefined marks kept:** 586 to 583: 3 lost, 0 gained.
  - `cast.useless` (`tools/triggers_list.php:1083`): the cast depends on a
    top-level variable that outside code may change between statements. The
    removal is the open-world semantics working.
  - `foreach.keyOverwrite` / `valueOverwrite` (`tests/Browser/ConsoleCleanTest.php:48`):
    removed. I first assumed `any_variable_may_exist` caused this.
- **X5, X4 without `any_variable_may_exist`:** identical to X4. The hypothesis is
  refuted. The mechanism for the two `foreach` losses is unexplained; the leading
  guess is that the statement-level mixing changes loop convergence (not traced).

### What these numbers do not cover

1. **Closure bodies.** `check_closure_literal` runs closure bodies with
   `global_scope = Other`, so the experiments never mixed a closure's by-reference
   captures. The handler fixture is exactly that case, so the measured cost
   understates the open world.
2. **Escaped references.** The mixing covers globals and top-level variables, not
   references passed to unchecked code.
3. **Corpus shape.** One codebase, mostly functions and classes. Function bodies see
   only `global` imports, which this corpus uses rarely, so the top-level scope
   carries most of the effect. Script-heavy code is unmeasured and would likely
   cost more.
4. **Handler precision.** X3 and X4 are the coarse ceiling. The designed handler
   model (strong update at definite warnings, merge at possible ones, write-effect
   summary) would cost less than havocking every variable at every statement.

### Reading the result

On this corpus the open-world ceiling for globals costs 3 of 586 findings and adds
none, so the sound option is cheap where it was measured. The closed-world option
is also unsound where it was probed (indirect calls: `call_user_func` and
`array_map` callbacks that write globals). The open-world cost for by-reference
captures, the part the handler fixture needs, is still unmeasured.

### Next measurement

Mix by-reference captures of every closure body (the closure's own uses, plus the
captures of any closure it may be called through), and re-run on this corpus. A
corpus with top-level scripts would also be worth adding.

## Open-world cost, measured (X6, X7)

Experiments mix variables to `mixed` before statements, with maybe-undefined marks kept. The experimental code is reverted; binaries are in `~/.cache/phpstan-rs-scratch/adv/bin/`.

- X6: mix every visible variable before every statement (top-level variables, `global` imports, closure by-reference captures).
- X7: as X6, but only before a statement that can run code (`global_writes::may_change` with an empty name: calls, includes, eval, yield, unset, global, reference assignment).

Piwigo (`corpus-*.json`, real project): baseline 586; X4/X5/X6 583; X7 583. Lost 3 (`cast.useless` `tools/triggers_list.php:1083`, `foreach.keyOverwrite` and `foreach.valueOverwrite` `tests/Browser/ConsoleCleanTest.php:48`), gained 0 in all.

Mago analyzer snippets (`mago-*.json`, not a real project): baseline 5150; X6 4933 (257 lost, 40 gained); X7 4980 (209 lost, 39 gained).

Findings on the mago losses and gains:

- `method.notFound` (88 lost under X6 and X7): baseline false positives. Eight snippets declare a `Collection` class with different shapes, and `check-project` loads them as one project, so the baseline resolves `$foo->get()` against the wrong class. Mixing hides the type. Mago is not a reliable measure of open-world cost for this reason.
- `argument.type` gained (36 under X6 and X7): a baseline bug, not caused by mixing. `Type::binary_operation` returns `int` for `Add` when neither operand is known to be an array, float, or int-bounded, so `mixed + mixed` becomes `int`. Reproduced on the unmodified binary: `function g(mixed $m) { takes_array($m + $m); }` reports `expects array, int given`, and PHP runs the file cleanly. Mixing makes more operands `mixed`, so the bug surfaces more often.
- `match.unhandled` gained (`mixed~(Unit::C1|Unit::C2)`): the open-world mixing of the match subject. Under open world the subject may be any value, so the report is a precision cost of mixing, not a false positive.

Conclusion: the real-project cost is small (3 lost, 0 gained on Piwigo). The mago numbers mix that cost with baseline false positives and the `mixed + mixed` bug. Granularity matters: X7 removed 48 of X6's losses without changing Piwigo.
