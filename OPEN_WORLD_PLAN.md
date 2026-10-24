# Plan: the open-world model, the type fixes behind it, and the open/closed decision

## Context

The checker assumes a closed world for variables that code outside a function can write
(globals, `global` imports, by-reference captures). That assumption is unsound: a call
that reaches code it cannot see (`call_user_func('setIt')`, an `array_map` closure that
declares `global $y`, a function in another file) leaves the caller's stale types in
place, and it reports findings off them. The measurements below give what a sound model
costs and the shape of the right design. This plan turns them into one build, and settles
the open/closed decision with stated criteria.

What is already committed on `master` (all with fixtures): the `mixed + mixed` arithmetic
fallback (`07adbd097`), `??` on an absent element (`9715b34c6`), the clippy fix
(`d5de8b100`), PHPStan's function-level stub PHPDoc (`2c189db92`), the trait-method scope
fix (`0566039d1`). The status of every item below, the decisions it waits on (Q1, Q3) and
the measurement rows are in `OPEN_WORLD_LEDGER.md`; `WORK_STACK.md` level 10 points there.
This file holds the design, `BY_REF_WARNING_AND_FIBER_DESIGN.md` the open-world and handler designs,
and `OVERFLOW_MEASUREMENT.md` the overflow measurement.

## What the measurements settled

- The cost is small on libraries and real on top-level scripts. Statement-level mixing
  (experiment X7) with the trait artifact removed, on `0566039d1`: Piwigo 583 to 580
  (3 lost, 0 gained); monolog, commonmark, guzzle, doctrine-collections, rector-src,
  doctrine-orm, psalm, flysystem and Symfony Console lose nothing; reactphp 4, laravel 1,
  drupal 28 (of about 124,000); WordPress 342 (about 2%, 8 gained).
- The experiments over-mixed. `file_exists()`, `fwrite()`, `array_shift()` cannot write the
  variable they appear near, and the statement mixed it anyway. These are artifacts of the
  experiment, not the cost of the model.
- `global_writes.rs` knows only the writes of functions declared in the same file. A
  function declared in another file that writes `global $x` is invisible at the call.
- A project-wide, demand-driven, memoized fact already exists for the same job on
  properties: `registry/property_writes.rs` (`PropertyWrites`: Nothing, OwnObject,
  Anything), fed by per-body `WriteFacts`/`CallWrites`/`Callee` in `registry/summaries.rs`
  and answered through `registry/demand.rs`. It follows `$this`, `self::`, `static::`,
  `parent::`, named statics, `new` and function names, and treats any other call as
  "may write anything". A builtin writes nothing unless a parameter takes something that
  runs code (`CODE_RUNNING_PARAMETER_TYPES`). The checker side is
  `forget_members_a_call_may_change` (`checker/expr.rs:183`, hooked once at `:371`).
- The globals model should be the sibling of that fact, not a second mechanism.

## The worlds, defined

One setting, one question: what does a call whose body the checker cannot see write?
Calls whose body it can see (project functions and methods, builtins) are resolved by
the project-wide fact in both modes, which is more precise than today.

- `world = "open"`: an unresolved callee (dynamic call, unknown receiver, a function outside
  the project and `scan_paths`, a callback-taking builtin) may write any global the
  scope can see. `include`/`eval`/`extract` keep their existing havoc.
- `world = "closed"`: an unresolved callee writes no global. This is today's behavior for
  every such call.

The setting lives in `EngineSettings` (`checker/settings.rs`, TOML top level, parallel to
`dynamic_constant_names`), reaches the checker through `CheckSettings`, and can be set
per fixture in the `X.toml` sidecar (the harness already reads `level = 9` there).

## Work items

Each item is one commit with its fixtures. Per project rules: `cargo fmt -p phpstan-rs`
first; check, clippy and the touched module's tests per item; the full suites at the end of
each lettered group, not per item.

### A. Type fixes with no world dependency

- **A1. `number` and the missing PHPDoc keywords.** `number` (PHPStan: `int|float`) is
  unsupported: a `@param number` gives `class.notFound` and a false `argument.type`
  (reproduced on `0566039d1`; ReactPHP `Transaction::applyTimeout`). Add it to the keyword
  table in `crates/types/src/parse.rs` (`type_keywords!`), then audit all of PHPStan's
  `TypeNodeResolver` keywords against it. Candidates absent from both tables, to verify
  one by one: `associative-array`, `callable-array`, `callable-object`, `empty`,
  `empty-scalar`, `non-empty-literal-string`, `non-empty-mixed`, `non-empty-scalar`,
  `static-closure`, `static-pure-closure`. Each gets a fixture checked by the runtime
  oracle.
- **A2. Unary operators on `mixed`.** `-$m`, `+$m` get the same `int|float` fallback as the
  binary operators (`checker/operators.rs`, `Type::binary_operation` in
  `crates/types/src/arithmetic.rs`); `~$m` is `int|string`.
- **A3. (dropped)** `count()` of a `Countable` stays `int<0, max>`: PHP passes a negative
  through, but both stubs declare `Countable::count()` non-negative, and an implementation
  returning a negative is reported at its `return`. Same model as every PHPDoc contract;
  fixture and corrected comment in `72854633c`.
- **A4. `abs()`, unary minus, `++` and `--` at the limits.** `abs(PHP_INT_MIN)`,
  `-PHP_INT_MIN` and `PHP_INT_MAX++` are floats (PHP 8.5 checked); today `abs(int)` is
  `int<0, max>`, `-$n` and `$n++` are `int`. These become part of group O's overflow
  results, not a separate float union.

### O. Integer overflow (before B; independent of the world setting)

Measured on the corpora with a blunt experiment (any unbounded end unions `float`):
Piwigo +138, vendors about 764 new-location findings, all read and classified in
`OVERFLOW_MEASUREMENT.md`. Real: 7 request-driven
`TypeError`s in Piwigo (`$page * $perPage` from `(int)` of query values), WordPress
`addTwoNumbers` (XML-RPC `<int>` parsed with `(int)`, default method table), Laravel
`AbstractPaginator::firstItem()` (needs a request-controlled `perPage`), monolog and Psalm
memory-shorthand parsing (config). Everything else came from operands bounded by memory,
the clock, step count or a guard. The design keeps the first set and drops the second.

The definition the group implements: a finding is true when some execution overflows
there, and an execution is chosen by the environment: request data, CLI arguments,
environment variables, files, database contents within the deployed driver's column
bounds, the clock within OS limits, hardware limits, and a run shorter than a stated
length. Checking an application, its own entry points are the environment and library
code is judged by what the application can feed it. Checking a library alone, its public
API has no environment, so what it needs from callers is reported as a contract, not as
a finding. Validation of this design (PHP 8.5, MySQL, PostgreSQL 18, SQLite, monolog run
at the limit) is in `OVERFLOW_MEASUREMENT.md`, last section.

- **O1. Exact overflow detection.** Range arithmetic in `crates/types/src/arithmetic.rs`
  computes in i128 with `PHP_INT_MIN`/`PHP_INT_MAX` as the ends of an unbounded range, and
  overflow is possible only when the exact result leaves i64. `int<0,max> - int<0,max>`
  is exact; `count($x) - 1` is exact. Covers `+ - *`, `**`, unary minus, `++`, `--`,
  compound assignment, `abs`. Shifts wrap and never give float (PHP 8.5: `1 << 63` is
  `PHP_INT_MIN`); `intdiv(PHP_INT_MIN, -1)` throws `ArithmeticError`; `PHP_INT_MIN % -1`
  is `0`; `array_sum`/`array_product` already return `int|float`.
- **O2. Runtime limits as return ranges.** Applied: `count()` of an array is at most
  `HT_MAX_SIZE` (2^30), a list's key below it (`bbdd45af9`); a `[deployment]` setting
  (architectures, operating systems; default any) bounds string measures by the largest
  listed address space and `time()` by the listed clocks, and an unbounded platform in the
  list bounds nothing (`793bbd50f`, decided 2026-10-22). What was checked:
  - `count($array)` without `COUNT_RECURSIVE`: at most `HT_MAX_SIZE`, `0x40000000` on
    64-bit (`Zend/zend_types.h:470`). Not `COUNT_RECURSIVE` (PHP 8.5: 4,295,032,832 for
    65536 copies of a 65536-element array) and not `Countable` (any int). Keys of a list
    get the same bound.
  - String lengths and offsets: PHP does not bound them (`ZSTR_MAX_LEN` is
    `SIZE_MAX - overhead`, `Zend/zend_string.h:108`), and neither does the address
    space on every platform PHP runs on: x86-64 (LA57) and riscv64 (Sv57) stop below
    2^56, AArch64 at 2^52, ppc64 at 4 PB (`asm/task_size_64.h`), but s390x gives user
    space the full 64 bits (`TASK_SIZE_MAX (-PAGE_SIZE)`,
    `arch/s390/include/asm/processor.h:121`). No universal bound; a bound needs the
    deployment's architectures.
  - `time()`: no universal bound either. Linux rejects a clock at or past
    `TIME_SETTOD_SEC_MAX` (`include/linux/time64.h:118`); FreeBSD at 8000 years unless
    `debug.allow_insane_settime` (`sys/kern/kern_time.c:425`); macOS accepts any
    non-negative second (`bsd/kern/kern_time.c:216`); Windows' `SYSTEMTIME` stops at
    year 30827. A bound needs the deployment's operating systems.
  - Not bounded (they are widening sources): `filesize`, `ftell`, `fstat` sizes (sparse
    files reach the filesystem limit), `DateTime::getTimestamp()` and `strtotime()`
    (PHP 8.5: `+100000000000 years` gives 3155695201792645701), `mktime` (wraps),
    `hrtime(true)`.
  Loop narrowing already carries a bound into the index (`$i < $c` with
  `$c: int<0, 1000>` gives `int<0, 999>`, probed on `0566039d1`), so O2 reaches loop
  indices without more work.
  - Database values: bounded by the column type only for drivers that enforce it. Checked
    locally: MySQL strict mode rejects 2^40 in `INT` (error 1264), non-strict clamps it to
    2147483647; PostgreSQL 18 rejects it; SQLite stores 2^62 in `INT` and `PHP_INT_MAX` in
    `SMALLINT`. So the bound needs the deployment's driver set (config) and the schema
    (migrations, ORM mapping, install SQL); SQLite gives none. Aggregates are not bounded
    by the column (`SUM` of two `INT` maxima is 4294967294).
- **O3. Magnitude: Small, Counter, Unknown, Large.** Counter applied to loop locals
  (`30c24ab03`): written only by literal steps from a bounded start, widened to
  start ± steps × iterations, iterations = 2^34/s × `[deployment] max_run_seconds`
  (a year unless set; decided 2026-10-22). Still to do: statics, private properties
  and by-ref captures updated only by steps; array elements (`$counts[$k]++`).
  First prototype measured (`OVERFLOW_MEASUREMENT.md`, last section): 863 gained vs 892,
  all eleven true positives kept; the rest is Unknown magnitude.
  Original text: Small is a bounded range (O2).
  Counter starts Small and changes only by a constant step of at most 2^10 per execution of
  the statement (the run-length limit of the environment); Counter plus Counter is not a
  Counter. Unknown is a value whose producer is a caller or writer outside what is
  resolved. Large is a value the environment can make arbitrarily big. The join is Large
  over Unknown over Counter over Small. The probe shows today's loop counter
  (`$d = $d + 1` in a `while`) widens to plain `int`, so the Counter rule is required.
- **O4. Provenance of strings, arrays and mixed values.** `(int)` of a string is only as
  big as the string can be: Large when the string comes from the environment, Unknown when
  it comes from a parameter, Small when it is a literal or built from Small values. So the
  model tracks what a string, an array element or a `mixed` value can contain (environment,
  parameter, bounded), through array access, `??`, ternaries, casts, `max`/`min`, string
  functions and properties. Validation showed the shortcuts "every `(int)` is Large" and
  "an int narrowed from `mixed` is Large" contradict the definition.
- **O5. Environment sources.** Superglobals; PSR-7 request getters; framework-injected
  controller, route and console parameters (checked: `"9223372036854775807"` reaches a
  typed `int` parameter as `PHP_INT_MAX`; `FILTER_VALIDATE_INT` accepts it); `argv`;
  `getenv`; `ini_get`; file reads and `unpack`; database fetches with the O2 bounds; ORM
  hydration with the mapped column type; `unserialize` of storage the app writes, as the
  join of its writes; dispatch tables such as WordPress's XML-RPC method map. A catalogue
  per framework, extensible from config.
- **O6. Overflow result, kept truthful.** An operation that can overflow keeps `float` in
  its result as a marked member. Narrowing treats it as a float (`is_int`, `is_float`,
  `gettype`), so a defensive `if (is_float($r))` is neither always-false nor reported. A
  sink that rejects the float (typed parameter, return, property, string offset, a
  callable's parameter) reports `arithmetic.intOverflow` instead of `argument.type` and
  the like, once per operation, naming the operation, the sink and the source chain. As an
  array key the marked float stays an int key.
- **O7. Contexts across calls.** A callee is re-run with each caller's argument
  magnitudes and provenance (the existing call-mode body re-run), so an operation in a
  library is judged by what the application feeds it; no reverse call graph and no
  registry-time receiver types are needed. A finding in a vendor body is reported at the
  first project frame. Preconditions must relate parameters to the object's properties:
  monolog's `normalize($data, $depth + 1)` throws TypeError only when
  `maxNormalizeDepth` is also `PHP_INT_MAX` (run at the limit; the guard at line 209 stops
  it otherwise).
- **O4/O5/O7 revised and validated (2026-10-22).** Bodies are re-run at their calls
  only for closures (`ClosureMode::Call`), never for named functions or methods, so
  O7 needs its own mechanism. The design below survived the attacks listed after it.
  - **Sinks.** Every place PHP turns the overflow float into an `int` is a finding.
    That includes typed `int` parameters, returns and properties (TypeError, also in
    coercive mode), and builtins' `int` parameters (`intdiv`, `chr`). It also includes
    the conversions PHP 8.5 warns about ("The float ... is not representable as an
    int, cast occurred") and that yield `PHP_INT_MIN`: array keys, `%`, bitwise
    operators, shifts, an explicit `(int)`, `sprintf('%d')`, string offsets and
    `settype`. `intdiv(PHP_INT_MIN, -1)` (ArithmeticError) is a site of its own.
    Printing and float comparisons lose precision silently and are not findings.
  - **Carrier.** Provenance is a set of data-flow nodes carried in types on `int`
    ranges (bounded ones too), `int`, `string`, `mixed`, and through array element
    types; literals carry none. This is Psalm's design (`Union::$parent_nodes`,
    `Internal/DataFlow`). The nodes are part of type identity, so a loop whose types
    agree but whose provenance grew runs another pass.
  - **Derivation.** A derived expression's result gets its operands' nodes. That
    happens where the checker infers the expression (arithmetic, casts, catalogued
    builtins, array reads, `??`, ternary, match, calls), not inside type operations.
    The types crate only has to keep nodes through union, intersection (narrowing),
    widening and generalizing.
  - **Nodes.** Sources (O5 catalogue: superglobals, `getenv`, `ini_get`, `argv`,
    request getters, framework-injected parameters, database fetches, file reads,
    `unserialize` of stored data). Also the current function's parameters, call
    results (one node per call site), and properties (one node per class property).
    Sizes are counts and lengths of what the process holds: bounded by the
    deployment's `memory_limit`, unbounded when it is `-1`. Counters are the bands
    of O3. Unknown is an unresolved edge, counted (O10).
  - **Facts and the project pass.** Each file's check records, for each overflow
    site, its operands as symbolic terms over nodes (`+ - *` of nodes and
    constants), with the site's own ranges. It also records the edges: call
    arguments to parameters with the caller's argument ranges, returns to call
    nodes, and writes to properties. A project pass follows the precedent of
    `property_assign_facts` (`project.rs:2490`, re-running the checker on unchecked
    files for their facts). It evaluates each site's terms with interval arithmetic
    under each context that reaches it from a source. The precondition is
    direction-aware and covers several parameters at once, which fixes the "is this
    parameter Large" shortcut (`$x + 1` overflows only at the top end, `$x - 1` only
    at the bottom). The finding is reported at the site with the path from the
    source, or at the first project frame for a site in vendor code.
  - **Dispatch.** A method call's facts point at every implementation the class
    hierarchy allows. A callback that is not resolved is an Unknown edge.
  - **Modes.** Application mode reports paths from sources. Library mode (O9) reports
    each public API site whose precondition no caller in the checked set discharges.
  - Attacks that failed: label loss in about 670 type constructions (derivation
    happens at the expression level, as in Psalm). Loops converging before
    provenance does (identity includes nodes). Single-level contexts (symbolic
    terms compose across calls).
  - Attacks that changed the design: sinks wider than typed parameters (above).
    Bounded ranges need provenance too: an environment value narrowed to
    `int<0, 10^12>` still overflows in a product. Preconditions must be
    direction-aware intervals. String lengths from the environment are sizes,
    bounded by `memory_limit`, not environment magnitudes.
  - Decided 2026-10-22:
    - Sizes are bounded by `[deployment] memory_limit` when set. Otherwise they are
      bounded by a stated hardware assumption: no process holds 2^56 bytes, so a
      size is below 2^56. It goes in the docs next to the one-year run.
    - A `mode` setting (`application` or `library`). When unset, composer.json
      `type: library` means library, anything else (or no composer.json) means
      application.

- **As built (O4, O5, O7).**
  - O4: types carry data-flow node sets (`types/src/flow.rs`), kept through union,
    intersection, narrowing, widening and loop convergence; the checker derives them at
    expressions (`checker/flow_derivation.rs`). Nodes (`checker/flow_nodes.rs`): request
    superglobals and `$argc` (environment), parameters, property reads, call results,
    globals and statics, and an overflow site carried by its float. A site is real when its
    operands come from the environment or have no source and a bounded range; an unbounded
    value with no source counts as unknown. Sinks (`checker/overflow_sinks.rs`) report
    `arithmetic.intOverflow` for real sites: native `int` parameters, returns and
    properties, builtin `int` parameters, array keys, `%`, bitwise operators, shifts, `~`,
    `(int)`, `intval`, `settype`, `printf` integer placeholders; and `intdiv(PHP_INT_MIN,
    -1)`.
  - O5: environment sources (`checker/flow_sources.rs`): file, stream, socket, terminal,
    environment, ini, database and stat builtins, `unserialize`, `filter_input`, `$argv`,
    and the request and result methods of PSR-7, Symfony, Laravel, mysqli, PDO, SQLite3 and
    `SplFileObject`; `[sources] functions` and `methods` name more. A counter stepped once
    an iteration of a by-value `foreach` over an array is bounded by the array's size (at
    most 2^30); a by-reference `foreach`, a `Traversable`, a step in a nested loop and a
    body with `goto` keep the run's bound.
  - O7: each check records where values go (`checker/flow_facts.rs`), and
    `checker/flow_eval.rs` judges overflow sinks from every file's facts. A node of a
    function has a value per call of the function: a parameter holds what that call passes,
    joined over the caller's calls; a call on a new object or on `$this` runs once per
    context of its caller (to depth 3), so a constructor writes each object's own arguments.
    An object is the `new` that made it in one context of its function; an object made where
    the checked code doesn't show is one more object, whose properties hold their defaults
    and what is written through receivers that can be it. Each operation is judged per
    context, refined through chains of up to four calls. A cycle that adds or subtracts
    amounts from outside it is a counter, moved by them as many times as its loops or the
    run let it run; a cycle the environment feeds holds what it chooses; any other runs its
    operations that many times (128 at most, then what the last run added is carried on for
    the rest) and narrows after, so `%`, `>>` and `&` hold it. `x - ((x >> k) << k)` and `x
    - (((x + c) >> k) << k)` keep what is below bit `k`. `(int)` of a float is at most 2^63
    - 1024. The float reaching a use is judged as PHP computes it, in doubles from the
    operands converted to doubles and through the steps it goes on through (`+ - *`, `/` by
    an `int`, unary minus, `abs()`, and `max()`/`min()` beside an `int`, which keep it only
    past that `int`; past four steps, or through another operation, by its exact result): a
    `strict_types` typed target rejects every float, any other use only one a double doesn't
    convert to an `int` from, so an overflow just below `PHP_INT_MIN` that rounds to `-2^63`
    is taken. `$_SERVER['REQUEST_TIME']` and `['REQUEST_TIME_FLOAT']` are the clock, not the
    environment's choice. An overflow the environment drives is `arithmetic.intOverflow`;
    one where values grow with nothing in the environment choosing them is
    `arithmetic.intOverflowGrowth`, reported only with `[ext.extras]
    check_int_overflow_growth` until O8 bounds relations.
- **O8. Relational domain.** Guards must clear findings: `$maxAge > PHP_INT_MAX - $now`
  (guzzle), `$depth > $this->maxNormalizeDepth` (monolog), `$pos <= strlen($s)`, and
  invariants an object keeps across its public calls (the WordPress tag processor's cursor
  within its document). Design under validation (not yet approved for implementation).
  Encodings and per-query verdict files live in `~/.cache/phpstan-rs-scratch/spike/`.
  - **What it has to clear.** With the growth check on (`1fffa1ffe`), 337 findings have no
    environment source (list: `~/.cache/phpstan-rs-scratch/design/o8-growth-findings.txt`):
    - Positions within a string (about 95): the WordPress html-api tag processor (14),
      decoder (8), SimplePie `Gzdecode` (7), HTTP parser (5), commonmark `Cursor` (24) and
      parsers (about 10), Psalm `FileManipulation` (10), Drupal sqlite `Schema` (5), Console
      section output (3), Piwigo's stream wrapper. The invariant is `offset <= strlen(s)`
      plus a constant; it needs builtin contracts relative to an offset
      (`strspn($s, _, $o) <= strlen($s) - $o`, `strpos` results below `strlen`), three-term
      relations (`pos + r <= len`), cancellation (`cp + min(n, L - cp) <= L`), and
      invariants of properties proved over every write.
    - Heap algebra (about 240): sodium_compat limbs (211) kept small by reductions between
      few additions, and Psalm literal folding through `TLiteralInt::$value`/`TIntRange`
      bounds (about 25). The bound depends on which objects reach which call, not on guards.
    - Loop-carried relations (few): Drupal `LockBackendAbstract` (`sleep <= delay` from
      `min()`), commonmark `DelimiterStack`/`RegexHelper`, `ProgressBarTest`.
  - **Domain.** A relational product inside `Locals`, keyed by `LocalKey`, not a separate
    IR: octagons per pack of variables that occur together, polyhedra per pack where a
    site needs them. Joins through `merge_scopes`, conditions through
    `apply_narrowing`/`compare_operands`, widening in `converge_loop_body`. Every write goes
    through `Locals` (declare and insert: 60 direct sites plus the binder), and loop passes
    are rewound with flow facts, so the clauses come from one recorded pass per statement.
  - **Heap.** Strong updates only for a declared, hook-free, never-referenced property of a
    must-alias object, with coercion modelled; weak otherwise (PHP 8.5 probes: set hooks,
    coercion, references, `__set`, unset all break strong updates).
  - **CHC per remaining finding.** The same pass emits Horn clauses (entry, summary and
    class-invariant predicates; call depth below 2^50; loop counters, `foreach` at most
    2^30 iterations, others 2^59; a public-call counter at most 2^59). Each finding the
    domain does not clear is solved: a proof drops it; a counterexample is a witness only
    when replayed against exact semantics; otherwise it is reported as unproven under its
    own identifier. Zero unknowns is not reachable (the tag processor's bookmarks need a
    quantified invariant over a sorted, disjoint list).
  - **Established by the spikes:**
    - Builtins need exact PHP 8 length models (`substr` with negative length or offset
      past the end, `strspn`/`strcspn` offsets); a faithful model turns unsolvable
      encodings into proofs of seconds.
    - Query clauses come from the same transition as the code, never written apart: a
      query missing a bound the transition had gave a false witness.
    - What the domain proves enters the clauses as background facts: a property moved
      only by constant steps is bounded by the run length (2733 `++$this->seek_count` is
      unprovable from clauses alone, proved in 8 s with the fact).
    - A constructor that only assigns its own properties writes no other object (the
      `property_writes` fact); property defaults hold at construction; `throw` ends a path;
      the call counter and object ids are written by no code; array keys may be negative.
    - A class invariant covers the class hierarchy: `WP_HTML_Processor` writes the parent's
      protected `bookmarks`, also on another instance.
    - Collections of value objects: one tracked object per value class (id, fields), its
      membership and key per collection, ids from a counter, null as id 0, other objects
      read through a second instance of the same point (cell morphing). Correct on safe
      and unsafe toys, but slow: the safe `end_of` takes 114 s unsliced and 227 s sliced
      at `rlimit` 3e8 (load about 10), and `sorted` (a `usort` then a `foreach`) is unknown
      after 2,746 s. Slicing each query to its cone of influence keeps every answer and
      saves little; the earlier 10 s came from the slicer's dropped clauses. Element
      invariants are proved where elements are
      created, not assumed where they are read.
    - Solvers: no single engine solves every case. Z3 Spacer with
      `fp.spacer.global` proves linear invariants the default engine misses; Golem `kind`
      proves the sodium limb loop; Eldarica alone handles guarded products (nonlinear).
      Z3 `rlimit` is deterministic per build but wall time per unit varies about 20x; Z3
      5.1.0 can reach an internal UNREACHABLE (`spacer_global_generalizer.cpp:337`), which
      `Z3_mk_context_rc` turns into unknown instead of exiting the process (code 114). Built
      from source, Z3 takes 24 minutes (`nice -n 19`, load about 4) and is a 33.5 MB binary.
    - Product guards (`$a > intdiv(PHP_INT_MAX, $b)`) keep a dedicated rule; products of
      environment values need no refutation (they are the true positives).
  - **Adversarial validation of the design:**

    | attack | evidence | verdict |
    |---|---|---|
    | strong updates unsound | PHP 8.5: set hook 10 vs 5, coercion "7"->7, reference alias 99, `__set`/`__get` 101, unset -> Error | holds with the conditions in Heap |
    | zero unknowns | tag processor: public `WP_HTML_Span` fields, sibling-instance writes, bookmarks shifted by prefix sums | fails: "unproven" stays a visible outcome |
    | separate IR | the checker already interprets the AST (`Locals`, `merge_scopes`, widening, narrowing; 108 files, 72.6k lines) | fails: a second semantics must agree forever; product inside `Locals` |
    | counterexample = witness | the encoding over-approximates (unknown edges, summaries, builtin rules) | fails: witness only after exact replay |
    | Spacer on products | nonlinear integer arithmetic is undecidable | partial: environment products are true positives; guarded products keep their rule |
    | full polyhedra scale | WordPress vars+props per function: median 3, p90 11, p99 32, max 168 | fails: packs, octagons per pack, polyhedra on demand |
    | replaces O9/O10 | | fails: contracts are a reporting mode; unknown edges are still counted |
    | O7 unchanged | provenance answers "chosen by the environment", relations answer "feasible" | O7 stays the provenance layer |
    | LSP latency | not measured | CHC only in the project pass, cached by body hash |
    | Z3 across platforms, determinism | Linux only; Windows (MinGW, wine) and macOS not built | requirement: a pinned, bundled Z3, a fresh context per query, `rlimit` not time, a fixed query order; verified on every platform before it ships |

  - **Solver matrix** (hand encodings in `~/.cache/phpstan-rs-scratch/spike/chc/`):

    | encoding | z3 default | z3 GSpacer | golem (best) | eldarica | truth |
    |---|---|---|---|---|---|
    | wp_cursor | safe 0.10 s | safe 0.11 s | spacer safe 0.10 s | safe 2.5 s | safe |
    | wp_rewrite_sorted | safe 0.28 s | safe 0.64 s | pdkind safe 5.8 s | safe 5.7 s | safe |
    | wp_rewrite_unsorted (length = len + sum of shifts; unfaithful model) | unknown | no answer in 10 min | all capped | unknown | artifact of the model |
    | wp_rewrite_exact (PHP 8 `substr` lengths) | unknown 14 s | safe 2.3 s | spacer safe 2.3 s | safe 7.5 s | safe |
    | wp_bookmarks (element predicate keyed by length) | safe 2.4 s | safe 1.0 s | | | safe |
    | monolog_size | unknown 104 s | safe 0.07 s | all capped | safe 3.3 s | safe |
    | monolog_nobound | unknown | unknown | capped | unknown | needs the size bound |
    | sodium_mul (size 26) | unknown | unknown | kind safe 0.41 s | unknown | safe |
    | sodium_mul63 | unknown | unknown | spacer cex 0.49 s | unknown | unsafe |
    | guarded_product (nonlinear) | unknown | unknown | unsupported | safe 2.9 s | safe |
    | limb_products (nonlinear, bounded) | safe | safe | unsupported | safe 2.1 s | safe |

    wp_rewrite_exact and wp_bookmarks assume every update lies within the document; the
    automatic heap model is what proves that.

  - **Automatic translation of the tag processor** (`spike/emit/emit.php`, 233 sites, Z3
    GSpacer, rlimit 3e7): 184 safe, 31 counterexamples, 18 unknown before the translator
    fixes listed above. The counterexamples were the translator defects above and reads
    of `lexical_updates`/`bookmarks` elements (`apply_attributes_updates` 2624–2683,
    `sort_start_ascending`, `seek`), which need the heap model. The unknowns are
    `skip_rcdata`, `skip_script_data`, `parse_next_tag` and `parse_next_attribute`
    cursor arithmetic. Line 2213 is proved by the exact `strspn` model. The entry and
    invariant predicates are reachable, so the proofs are not vacuous.

  - **Tag processor after the translator fixes, without the heap model** (`spike/emit/wp-tag4`,
    stopped at 168 of 233 sites): 151 safe, 17 unknown, no counterexamples. The
    counterexamples at 1037, 1148 and 1762 are now unknown. The sites from 2192 onward
    (element reads in `apply_attributes_updates`, `sort_start_ascending`, `seek`, and the
    later setters) were not reached.
  - **Slicing must keep every clause sound.** A predicate left with no arguments has to be
    written as a constant (`(|p|)` is rejected by Z3, which drops the clause), and a
    constraint over no variable (`false` where a `throw` ends a path) has to be kept.
    Dropping either makes paths reachable or unreachable that are not: with `false`
    dropped, `__wakeup` (which only throws) returned, and its `new \LogicException`
    reset the heap and produced the counterexamples at 2624–2660. A solver error voids
    the verdict. Counterexamples are read from Z3's refutation (`produce-proofs`, with
    `fp.xform.inline_*` and `coalesce_rules` off so each method shows;
    `spike/emit/prooftree.py`).
  - **Tag processor with the heap model** (`spike/emit/wp-tag7`, unsliced, Z3 GSpacer,
    `rlimit` 3e7, 69 minutes of solving): 198 of 233 sites safe, 35 unknown, no
    counterexamples. The encoding states every string shorter than the size bound wherever a
    clause starts at a predicate (loop heads, joins, call summaries, second instances), not
    only where public methods are entered; that alone proved 14 sites, among them the local
    positions of `skip_rcdata`, `skip_script_data`, `parse_next_tag` and
    `parse_next_attribute`. A sort's comparator is entered at the sort with two members of
    the collection, and protected methods are entries. Slicing each query to its cone of
    influence kept about 80% of the arguments and saved little, while slicing a 17 MB
    encoding in Python took tens of seconds per query.
  - **The 35 unknowns:**
    - Cursor fields across calls (16): `base_class_next_token` 1037/1146/1148,
      `parse_next_tag` 1762/2192, `parse_next_attribute` 2288/2337/2352,
      `get_full_comment_text` 3680, `subdivide_text_appropriately` 3734/3745,
      `set_modifiable_text` 4044/4048 (two), `set_attribute` 4731, `get_updated_html` 4953.
      They need the cursors bounded across public calls. Every write is a position within
      the document plus at most 9, except `bytes_already_parsed += $shift` in
      `apply_attributes_updates`.
    - The shift sums of `apply_attributes_updates` (13, 2624–2683). Their bound needs the
      pending replacements pairwise disjoint and within the document: a property of pairs
      of elements, which one tracked element cannot state.
    - The comparator's field differences (2, 2762/2776): the elements' positions come from
      the cursors.
    - Local loop relations in `skip_rcdata` (3, 1440/1456/1464): unknown at `rlimit` 3e8
      too; `$tag_length = strlen($tag_name)` must be carried through two loops. The
      domain's own facts are what the clauses receive for this. `skip_script_data` 1558
      is proved at 3e8 (18 s).
  - **What the remaining unknowns need** (diagnostics on the 35, `spike/emit/wp-tag8`,
    `wp-tag9`; each assumes, without proof, that every int field of the object and of the
    tracked elements stays within 2^57):
    - Assumed where public methods are entered only: 6 of the 35 are proved (public methods
      that read the fields they were entered with). Z3 does not carry the bound through
      the 175 internal predicates within the budget.
    - Assumed wherever a clause starts at a predicate: 21 are proved, among them 13 of the
      16 cursor sites, both comparator sites and 7 of the shift-sum sites. So the domain's
      facts have to reach the clauses at every predicate, not only at entries, and the
      domain has to prove those bounds itself: the cursors are positions in the document,
      and `bytes_already_parsed += $shift` runs only while the replacement starts before
      the cursor, so the cursor stays above `start - length`.
    - Still unknown: 7 sums over the collection (`$accumulated_shift_for_given_point`, 2633;
      `head_delta`/`tail_delta` and the bookmark positions, 2674–2683;
      `$before_current_tag`, 4953), which no bound on one element bounds, and 7 relations
      between locals (1146, 2337, 2352 and the `skip_rcdata`/`skip_script_data` loops).
  - **The pairwise invariant over `lexical_updates` is false in WordPress.**
    `remove_attribute()` appends a removal for every duplicate attribute each time it is
    called, with no deduplication (also in trunk, 7.2-alpha). Under PHP 8.5 with the real
    classes (`spike/wpdup/`): on `<div x=1 x=22 x=333>`, two calls queue 5 replacements and
    produce `<div   x=333>after</ >after<>after</div>` (49 bytes from 40); a bookmark on a
    later tag moves to -103 after 10 calls, -11983 after 1,000, and -99193 after 100 calls
    with a 1,002-byte duplicate: linearly in the number of calls. Tracking two elements
    cannot prove what is not true, so the shift sums stay unproven. Whether they reach
    2^63 depends on how fast the corrupted document grows against the memory bound, which
    is not settled.
  - **Open before the design is settled:** the ledger's rows O8.1 to O8.7 (the tag processor
    rerun, the pairwise collection invariant, the subclass's writes, the sodium ladder, the
    `WP_Query::get_posts` scale test, the solver portfolio, Windows and macOS).
- **O9. Library mode: contracts.** Checking a library alone, what a public API needs
  from its callers to avoid overflow is reported as a contract (opt-in report), not a
  finding. Laravel's `firstItem()` is such a contract in Laravel and a finding in an app
  that passes a request-derived `perPage`.
- **O10. Coverage.** An unresolved call edge (callback, string dispatch, event
  dispatcher, DI) leaves a callee's context Unknown, which loses a true positive if the
  environment reaches it that way. The run reports the number of unresolved edges that
  carry an undischarged overflow condition, so the gap is visible instead of silent.

Order: O6's marked float first, accepted by every sink and seen as a float by narrowing,
so overflow can enter the types without findings; then O1 and A4 emit it (exact
detection, the limits); then O2 and O3, with sinks reporting by magnitude, as the first
prototype, measured on Piwigo and the vendor packages with every remaining finding read;
then O4 and O5, O7, O8, O9, O10, measuring after each. The eleven true positives found so
far are the regression set; in library-only corpora, Laravel's becomes a contract.

Limits stated in the docs: 64-bit PHP only (on 32-bit, `strlen + 1` can overflow); the
Counter rule assumes runs shorter than a century at the step bound; database bounds hold
for the configured drivers and the code's schema, not for a drifted deployment; string
length and clock bounds hold only for the deployment platforms configured.

### B. The global-write fact (the effect model)

- **B1. Summary facts.** Extend `WriteFacts` (`registry/summaries.rs`) or add a sibling
  struct with: names from `global` statements, `$GLOBALS['literal']` writes, a flag for
  dynamic global names/`$GLOBALS[$x]`, and the "exact literal on every exit" refinement
  that `checker/global_writes.rs::exact_exit_writes` computes per file today. Reuse
  `Callee`/`CallWrites` for the calls a body inherits writes from.
- **B2. Demand answer.** A new registry fact beside `property_writes.rs`:
  `GlobalWrites { Nothing, Names(set), Anything }`, settled the same way (monotonic search,
  a cycle keeps the conservative verdict, memoized in `registry/demand.rs`, a new `Answer`
  variant). Queries: `function_global_writes`, `method_global_writes`.
- **B3. Builtin verdict.** Share `CODE_RUNNING_PARAMETER_TYPES` with `property_writes.rs`
  (one constant, not two). A builtin with none of those parameter types writes no global;
  its by-reference arguments are already written at the call by
  `apply_param_out_types` (`checker/expr.rs`, near the old `apply_global_writes` call at
  `:1570`).
- **B4. Checker hook.** Replace `GlobalWrites`/`apply_global_writes`
  (`checker/global_writes.rs`) with a hook beside `forget_members_a_call_may_change`
  covering `Call`, `MethodCall`, `StaticCall`, `New` and closure invocation. Keep `Scope`
  (`Top`, `Function(imports)`, `Other`) as the map from "which locals are globals".
  The `must_invoke` and `closure_flow` modules keep reading their own per-file facts until
  B6.
- **B5. The `world` setting** as defined above, default per decision Q1 (in the ledger), with a docs entry
  and sidecar support. Fixtures run in both modes where they differ.
- **B6. By-reference captures and closure scope.** A closure's by-reference captures are
  shared state: calls inside the body mix them under the same rules, replacing the
  experiment's `experiment_byref` stack. Unify `MustInvoke`'s per-file global facts with B2.
- **B7. Includes.** `include`/`require` currently havoc every variable. Resolve the include
  target (`checker/includes.rs`) and use a summary of the included file's top-level writes
  when it is in the project; keep the havoc for unresolved targets. This is where the
  WordPress cost goes down.

### C. Error handlers and fibers (decision Q3 in the ledger)

The design is in `BY_REF_WARNING_AND_FIBER_DESIGN.md`,
revised decisions D1 to D5. It builds on B: one "may run other code" summary per function,
one rule at each call or operation that has it, covering handler warnings and fiber
suspends. D5 (scope of handler installation) is the same `world` setting. It closes the
two fixtures that fail today:
`checker/by_ref_capture/a_by_ref_capture_written_by_an_error_handler_is_read_after_the_warning`
and `.../d_by_ref_capture_of_a_global_sees_a_script_write_after_a_fiber_suspend`.

## Validation

- **Fixtures** (`crates/phpstan-rs/tests/fixtures/checker/`, `global_vars/` and
  `by_ref_capture/`): one per resolved case and per world. Same-file, other-file and
  scan-path function that writes `global $x`; a function that writes one named global and
  not another; a callback-taking builtin; a builtin with no callback; method, static call
  and `new`; a closure by-reference capture; a trait method; `include`, `eval`,
  `$GLOBALS`; `call_user_func('setIt')`; an `array_map` closure with `global`. Every
  `assertType` is run under PHP by `tests/runtime_oracle.rs`.
- **Corpora**, `check-project --no-composer --format json` on each, baseline and candidate
  built from the same commit:
  Piwigo (`/home/torres/piwigo17-frozen`, `phpstan-rs.toml`), and `vendor/` `src`
  directories of monolog, league-commonmark, guzzle, league-flysystem,
  doctrine-collections, doctrine-orm, rector-src, psalm, reactphp-http, symfony Console,
  laravel, plus `vendor/wordpress` and `vendor/drupal/core`. Diff with
  `~/.cache/phpstan-rs-scratch/adv/diffjson.py`. Drop `mago/crates/analyzer/tests` as a
  precision corpus: eight snippets declare `Collection`, so results mix name collisions
  with the effect. Keep it for crash and timing checks only.
- **Classification protocol.** Every lost or gained finding goes into one of: legitimate
  uncertainty (the fact depended on code outside the checked set), a baseline false
  positive the change hides, a true finding lost (a defect in the model), an artifact
  (mixing something the call cannot write). Artifacts and lost true findings must be zero
  before cost is reported.
- **Timing (M4).** `hyperfine -N` under `nice -n 19`, warm cache, baseline and candidate
  alternating, `/proc/loadavg` noted: Piwigo and laravel `src`.
- **Suites.** `cargo test -p phpstan-rs --test fixtures`, `--lib`, `cargo test -p
  phpstan-rs-types`, `--test runtime_oracle`, `cargo clippy -p phpstan-rs --all-targets`.
  The two by-reference fixtures stay failing until C.

## The open or closed decision

Settled with the corpus results, against criteria stated now:

1. Zero artifacts and zero lost true findings after classification.
2. Lost findings from the open model are at most 1% of a package's diagnostics, except
   top-level-script-heavy code (WordPress), which is reported with its number and its B7
   improvement.
3. Wall time at most 5% over closed on Piwigo and laravel.
4. Gained findings: zero.

If all four hold the default is the recommendation of decision Q1. If any fails, `open`
ships as opt-in and the failing criterion is recorded in `WORK_STACK.md`.

## Housekeeping

- Update `WORK_STACK.md` level 10 per item as items finish. Fold finished rows into its Done
  prose (project rule).
- Record the integer-overflow and error-handler decisions in the level 10 section.
- Findings and decisions go into this plan and the ledger as they land. Bulky data stays in
  `~/.cache/phpstan-rs-scratch/` (corpus outputs, the `design/overflow-*-new.txt` and
  `design/o8-growth-findings.txt` lists, CHC encodings and verdict files), and these files
  point at it.
- Scratch binaries in `~/.cache/phpstan-rs-scratch/adv/bin/` stay until the decision is
  recorded, then are removed by full path.

## Critical files

- `crates/types/src/parse.rs` (keywords), `crates/types/src/arithmetic.rs`,
  `crates/phpstan-rs/src/checker/operators.rs` (A)
- `crates/phpstan-rs/src/registry/summaries.rs`, `property_writes.rs`, `demand.rs`,
  `bodies.rs`, new `registry/global_writes.rs` (B1 to B3)
- `crates/phpstan-rs/src/checker/expr.rs` (hook beside `:371`; old call at `:1570`),
  `checker/global_writes.rs` (`Scope`), `checker/function_decl.rs`, `checker/closure_flow.rs`,
  `checker/must_invoke.rs`, `checker/includes.rs` (B4 to B7)
- `crates/phpstan-rs/src/checker/settings.rs` (`EngineSettings`) (B5)
- `crates/phpstan-rs/tests/fixtures/checker/` and `tests/runtime_oracle.rs` (validation)

## Risks

- B2 adds a registry answer that persists in the stub/project caches; its key must change
  with the summary format, or a stale cache returns old verdicts.
- B7 changes `include` semantics that many fixtures cover; run the whole fixture suite at
  the end of B, not the touched module only.
- Method resolution for B4 depends on the receiver type, which the registry-time fact does
  not have (the same limit `purity` documents). Receivers it cannot resolve are
  `Anything`, so the model can only lose precision there, never soundness.
- C is large; it ships only after B validates, and may be split into its own plan.
