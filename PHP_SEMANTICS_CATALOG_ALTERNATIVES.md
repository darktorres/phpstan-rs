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

## Experiment 5: CodeQL, with and without field-based dispatch

**Question.** Can a mature, scalable (but unsound) C analyzer find H1 and H2 from the builtin's
entry, and what does its call graph give as a reachability baseline?

**Licence.** The CodeQL CLI terms allow analysis of an open-source codebase under an
OSI-approved licence; php-src 8.5.4 is under the PHP License 3.01 (OSI-approved, May 2020) and
hosted on GitHub. CodeQL itself is not redistributed.

**Method.** CodeQL 2.27.2. A database of php-src 8.5.4 built as the minimal CLI
(`--disable-all --enable-cli --enable-session`, clang-21 `-O0`): 453 s, 248 MB. Queries in
`tools/php-catalog/experiments/alternatives/codeql/` compute, for every builtin function and method
(1,518 roots), which engine functions that dispatch to user code it can reach, stopping at user
code itself (`zend_call_function`, `execute_ex`, `zend_execute`). Two call graphs:

- CodeQL's own (`viableCallable`: static calls plus its function-pointer and lambda tracking);
- the same plus field-based resolution (`FieldResolution.qll`, about 40 lines): a call through a
  struct field reaches every function stored in a field of that name; a call through a variable,
  every function assigned to it; a call through a parameter, every function passed there by a
  direct caller.

Each was scored against experiment 3's observations of the same builtins (357 observed
builtin-to-mechanism pairs) with `score_reach.py`.

**Numbers.**

| Call graph | Indirect calls resolved | Recall on observed pairs | Builtin × mechanism pairs ruled out |
|---|---:|---:|---:|
| CodeQL's own | 0 of 2,358 | 0.711 (103 missed, including H2) | 36,433 of 39,468 |
| CodeQL + field-based | 1,910 of 2,380 | 1.000 | 13,602 of 47,058 |

Its own dispatch resolves none of php-src's calls through handler tables
(`Z_OBJ_HT_P(zv)->read_property`, `wrapper->wops->rename`), so every builtin "reaches" only
`zend_call_function` (through error reporting) and `zend_lazy_object_init`, and H1 and H2 are not
found. With field-based resolution both are found, and nothing observed is missed, but 22 of the
31 mechanisms are reached by 1,517 of the 1,518 builtins, `abs()` and `is_string()` included:
`__get`, `__set`, `__isset`, `__toString`, `__debugInfo`, `__destruct`, iterators, lazy
initializers and every user-stream operation. The pairs ruled out are almost all for narrow
mechanisms: wrapper `rename`/`mkdir`/`rmdir`/`unlink` (3 builtins reach them), `metadata` (8),
`stat` (16), `opendir` (21), `Serializable` (22), `__unset` (3).

**Meaning.** Unsound dispatch is unusable here: it misses what matters. Sound function-level
reachability is cheap (two minutes) but says "everything may run every magic method", because
every builtin reaches the engine core (error objects, releasing values, handler tables) whatever
its arguments. "Never" facts worth having for the common mechanisms need to know *which object*
reaches the handler call: argument-sensitive analysis, or the per-parameter bound plus
observation. Function-level reachability earns its keep only for narrow mechanisms and as the
generator of the mechanism list itself.

## Experiment 1: reusing sound or scalable C analyzers

**Question.** Does any existing analyzer produce a call graph or points-to result for php-src
that fits the plan's 10 GiB envelope, is sound against what PHP was seen doing, and tells
builtins apart (which builtin can reach which user-code mechanism)?

**Method.** Input: the minimal CLI used throughout (`--disable-all --enable-cli --enable-session`,
PHP 8.5.4), 523 C objects, 14,142 functions, 3,727,445 LLVM instructions. Each tool ran in a
10 GiB cgroup (no swap) or container memory limit, with a time box, one at a time under
`nice -n 19`. Results were scored with `score_reach.py` against experiment 3's 357 observed
builtin-to-mechanism pairs, over 31 mechanisms (each magic method, iterators, lazy
initialisers, `Serializable`, each user-wrapper and user-stream operation, and any user code).
Tools needing LLVM 14 got a clang-14 rebuild of the same configuration through wllvm.

**Numbers.**

| Tool | Configuration | Outcome | Time | Peak |
|---|---|---|---:|---:|
| Own baseline B0 | every indirect call may reach every address-taken function | completes; all 31 mechanisms reached by 1,517/1,518 builtins | 205 s | 1.6 GB |
| Own baseline B1 | indirect calls by LLVM function type | identical to B0 (opaque pointers make the types uninformative) | (same run) | |
| CodeQL 2.27.2 + field-based (experiment 5) | struct-field, variable and parameter resolution | completes; recall 1.000; 22/31 mechanisms reached by 1,517 builtins | 93 s query | |
| Phasar (development, LLVM 21) | OTF call graph, CFL-Steensgaard | completes; recall 1.000; all 31 reached by 1,516/1,517 builtins (62 of 47,058 pairs ruled out) | 1,129 s | 2.6 GB |
| Phasar | OTF call graph, union-find aliases | killed at 10 GiB | 1,080 s | 10 GiB |
| SVF 3.4 | Steensgaard | killed at 10 GiB | 147 s | 10 GiB |
| SVF 3.4, compact layouts | Steensgaard | killed at 10 GiB | 154 s | 10 GiB |
| SVF 3.4 | type-based call graph | killed at 10 GiB | 169 s | 10 GiB |
| SVF 3.4, compact layouts | Steensgaard on mem2reg'd bitcode | killed at 10 GiB | 163 s | 10 GiB |
| sea-dsa (SeaHorn, LLVM 14) | context-sensitive + complete call graph | killed at 10 GiB | 137 s | 10 GiB |
| sea-dsa | context-insensitive + complete call graph, mem2reg'd | killed at 10 GiB | 139 s | 10 GiB |
| sea-dsa | context-insensitive, no call graph | segmentation fault, no result | 526 s | 1.8 GB |
| Yapall 0.1.2 (LLVM 14) | unification | killed at 10 GiB (after a rebuild without `asm goto` and `ifunc`, which its LLVM reader rejects) | 188 s | 10 GiB |
| Yapall 0.1.2 | subset (Andersen) | killed at 10 GiB | 205 s | 10 GiB |
| Goblint 2.7.1 (CIL) | from `main` | front end cannot parse PHP 8.5's configured C; stopped after 8 successive incompatibilities | < 1 min each | 3 GB |
| IKOS 3.5 (LLVM 14) | interval domain, from `main` | preprocessor crashes on AVX-512 vector selects in `php_base64_encode_avx512` (its select-lowering pass is mandatory, also with `--opt=none`) | 13 s | 0.9 GB |

The plan's own earlier results belong in the same table: the custom whole-program pass of
October 4–5 exceeded 10 GiB on PHP 8.4, and SVF's demand-driven analysis (stock and compact) was
killed at 10 GiB on this same CLI. cclyzer++ was not run: it is tested only up to LLVM 10, and its
authors' successor, Yapall, was run instead.

**What it means.**

- Nothing that tells builtins apart fits. Every analysis that tracks which object reaches which
  call (subset or unification points-to: SVF, sea-dsa, Yapall, Phasar's union-find) is killed at
  10 GiB, crashes, or rejects the module, on the smallest PHP build there is. Production builds
  with every extension are several times larger.
- What completes is sound but says nothing per builtin. B0/B1, CodeQL with field resolution and
  Phasar with CFL-Steensgaard all keep every observed pair (recall 1.000) and all conclude that
  essentially every builtin may run every magic method: whole-program reachability goes through
  the engine core (errors, releases, handler tables), whatever the arguments.
- Fitting a mature tool to php-src costs one fix after another: Yapall needed a rebuild without
  `asm goto` and `ifunc`; Goblint needed eight successive workarounds and still failed; IKOS
  cannot get past AVX-512 code.

So reuse does not rescue the native-analysis route, and a custom engine would face the same
scale with less infrastructure. The sound bound that survives is the cheap one: reachability says
which mechanisms exist and which builtins could ever reach the narrow ones (wrapper
`rename`/`mkdir`/`rmdir`/`unlink`), and per-argument precision has to come from the parameter
bound plus observation, not from whole-program pointer analysis.
