# PHP catalog: experiments on the alternatives

Five experiments run on 2026-10-15 to choose between the catalog plan's approach (a sound
analysis of php-src's native code) and the alternatives the review found. Each section states
the question, the method, the numbers, and what they mean for the direction. Scratch outputs
are under `~/.cache/php-alt-exp/`; tools and results that later work needs are checked in under
`tools/php-catalog/experiments/alternatives/`.

Known holes used as ground truth, each confirmed under PHP 8.5.4:

- H1: `rename('wr://a', 'wr://b')` runs the user wrapper's `rename()`. The builtin-runs table
  has no row for it, and the type-derived bound (`derived_runs`) has no `rename` method and no
  `from`/`to` path parameter names.
- H2: `array_column([$o], 'p')` runs `__isset`, then `__get` when `__isset` returns true. The
  table has only `__isset`, because the recorder's object always answers `false`.
- H3: a lazy object's initializer runs on first access, for example inside `get_object_vars()`.

## Experiment 3: tracing what real workloads run

**Question.** Does observing real workloads find user code that builtins run and the table
misses?

**Method.** A PHP extension using the observer API (`tools/php-catalog/experiments/alternatives/tracer/`)
records, per internal call, the user code that runs directly inside it and where the user
object or closure sits among the call's arguments. It forces real calls for frameless and
special-cased builtins. The workload was php-src 8.5.4's own `.phpt` suite on the host PHP 8.5.4,
with the configured extensions loaded and the profilers left out.

**Numbers.** 21,836 tests in 519 s (17,741 passed, 191 failed, 3,896 skipped; the failures are
mostly opcache optimizer tests whose opcode dumps change when the shortcuts are disabled). The
trace has 1,183 distinct observations. Of those expressible as table rows, 217 are missing from
the table: 182 method runs, 23 wrapper runs and 12 callback runs.

**What it found.**

- H1 and H2, from php-src's existing tests (`ext/standard/tests/file/userwrapper.phpt`,
  `ext/standard/tests/array/array_column_property_visibility.phpt`).
- Property hooks run by `get_object_vars`, `json_encode`, `var_export`,
  `ReflectionProperty::getValue`/`setValue`, and `Exception::__construct` (a subclass's
  `$code::set` hook).
- `ReflectionClass::resetAsLazyGhost`/`resetAsLazyProxy` run the argument's `__destruct`.
- Callbacks in places the parameter types don't show: inside `filter_var`'s options array, in
  `preg_replace_callback_array`'s array, `PDOStatement::fetchAll` with `PDO::FETCH_FUNC`, and the
  variadic last argument of `array_intersect_uassoc`, `array_intersect_ukey` and the
  `array_uintersect` family.
- Wrapper methods: `fopen` seeking, `readdir`, `stream_select` and `posix_isatty` casting,
  `stream_set_blocking`, `SplFileObject` opening, `PDOStatement::execute` and
  `SQLite3Stmt::execute` reading stream parameters, `exif_read_data`,
  `DOM\HTMLDocument::saveHtmlFile`, `ZipArchive::extractTo`.
- `var_dump` running `__debugInfo` at variadic positions 2 to 7; `msg_send`/`shm_put_var` running
  `__serialize` at position 2.
- 48 `__toString` runs at string parameters that the table lacks (`strlen` has no `__tostring`
  row at all); the type-derived bound covers these.

Two kinds of entries are not holes: callable arrays (`[$obj, 'method']`) show as a method run at
`N[]`, and overrides of internal template methods called on the receiver (`SplHeap::compare`,
`SplObjectStorage::getHash`, `RecursiveIteratorIterator` hooks, `FilterIterator::accept`) are
covered by the checker's rule for methods a class extending a builtin declares.

**Meaning.** Ten minutes of tracing an existing test suite found both known holes and several
dozen more, across hooks, destructors, callbacks and stream wrappers. The table's synthetic
probes miss whole mechanisms, and the type-derived bound misses method names (`__get`, hooks,
`__destruct`, wrapper `rename`) and callback places. Observing workloads belongs in the
evidence; it finds what neither the probes nor the hand-written bound name.
