# 20 G plan ledger

Tracks building the plan of `PROFILE_DRILL_PIWIGO_20G.md`: items 0-19 (§10), each closed by its validation (§11), then O3. One row per task. The design of each row is in the plan, at the step or bullet the row names; this file holds only the order and the state. The plan was reviewed in full at `d6e43bf7` (§13's review table).

Building started on 2026-10-03 (`WORK_STACK.md`, level 8).

**Status:** OPEN, DONE (hash), PART (what is left), SKIP (reason).

**Kind:**
- **perf**: exact; the output does not change.
- **sem**: a semantic change §11 lists.
- **check**: a new diagnostic.
- **infra**: an oracle, a dump or a test.
- **val**: a validation run.

## How the rows are worked

- **In order.** Item 0 comes first, and the rows run top to bottom; an item starts after the previous item's validation row; within an item they are in an order that builds, which is not always the plan's step order (the *plan* column gives the step). The *needs* column names a row in another item that must be done first. Within item 0, the dumps (0.4-0.10) come before the oracle run (0.2), whose edit replay records them, and the deletions (0.3) after it. A row that needs a later item's row sits right after it and is validated with that item: 3.4 after 9.2, and 6.2-6.4 after 9.5, all with 9.V, whose comparison then also admits item 6's step-2 changes.
- **Per row:** `cargo fmt`, then `cargo check` and clippy, then a commit with an explicit pathspec, since another session may share the checkout. A row may take several commits; its status takes the last one's hash.
- **Validation at the end of each item.** Each item's last row, *N*.V, runs §11: the builds, the tests, the fixtures, item 0's dumps with the `gate` and `oracle` builds, O2 and the equalities, compared with the previous item's, plus what the item adds. Nothing of it runs between an item's rows. Item 0's own rows also read the starting state and run the debug oracles; 0.11 is its validation and the baseline. O3 runs once, in the V rows, after 19.V.
- **A design that turns out wrong** is fixed in the plan first, in its own commit, and then built.
- **A new dependency** is added in the row that first uses it (§13's table). `twox-hash` leaves in row 17.2.
- **Heavy jobs** (builds of other profiles, the corpus, profilers) run one at a time, under `nice -n 19`, with no `timeout`.
- **Nothing kept in `target/` or `/tmp`.** `cargo clean` empties `target/` and a reboot empties `/tmp`. Every binary a later row or validation compares against is copied to `.corpus-runs/gate/bin/<commit>/<profile>/` as soon as it is built; runs and dumps go to `.corpus-runs/gate/<artifact>/<commit>.<ext>`; profiles to `~/.cache/prs-profile/<run>/` with their binary and a `README` (`.corpus-runs/README.md`). A worktree of another commit builds into a target directory of its own: two checkouts of one workspace in one target directory share the workspace crates' artifacts.

## Tasks

| # | plan | task | needs | kind | status |
|---|---|---|---|---|---|
| **0** | **item 0** | **Baselines; the debug oracles retired** | | | |
| 0.1 | step 1 | Read the starting state: at HEAD, the vendor-tree report against `1892b477`'s, and the all-on and unscoped reports against `7a3f3f99`'s (where the six changes show), each difference read and recorded | — | val | DONE (003f308f) |
| 0.1a | fix, step 7 | `array_reduce` by its stub's generics: `$carry` the initial value joined with the callback's return until stable; the callback's return for a never-empty array, the initial value's for an always-empty one, their union otherwise | 0.1 | sem | DONE (396240db) |
| 0.1b | fix, steps 1-4 | Effects by storage: object properties by field and declaring class, the callee's own `$this` apart; static properties by storage (`static::` over descendants, traits per user); globals; static variables by the configured versions; captures by reference. The summary's walk records writes with their bases; the fixed point settles effects, overridable and interface calls over every loaded implementation, callbacks by their target | 0.1 | sem | DONE (72de29b9) |
| 0.1c | fix, step 4 | Builtins by what they do: by-reference parameters, callbacks, the methods they call on an argument (by the hierarchy), and stream functions, each calling the wrapper methods the recording-wrapper run in `php` 8.5 found, on the registered wrapper classes | 0.1b | sem | DONE (24353080) |
| 0.1d | fix, step 5 | The checker forgets by effect: a property's facts on values whose type may hold the written object, the receiver's aliases for an own-object write, objects created here and never escaped kept; static storages; globals (every variable at a file's top level); static variables on re-entry; captures; references to written storage | 0.1b | sem | DONE (14988657) |
| 0.1e | fix, step 6 | A capture by reference entered with every type written while shared: the closure body checked until its entry stops growing, and again after a later write by the outer function | 0.1 | sem | DONE (1e0e1c52) |
| 0.1f | fix, oracle | Fixtures for each storage, binding, call and builtin case, field sensitivity, escape, captures and `array_reduce`, each checked against `php` (7.4 through `docker` for static variables) | 0.1a-0.1e | infra | DONE (f185ce48, 3e0d321c) |
| 0.4 | step 3 | One canonical type printer in `types` over all of `Repr`, with no interning. Every printer destructures without `..` and matches without `_` | — | infra | DONE (0fa1723c) |
| 0.5 | step 3 | O1, `--dump-front-end` through `parse_file`. It covers: the tree after the stub rules and anonymous-class names; facts; ignore marks; warnings and errors; positions at every token boundary; doc sites with their `DocBlock`s; references; built declarations; duplicate sites; use sites. It also records every type node with the type today's round trip gives it, and every union built, with its operands and result | 0.4 | infra | DONE (64ddd7e3, 11f3f8ba, c965722c, 0e9479ec) |
| 0.6 | step 3 | The full `registry-dump`, after the finishing passes | 0.4 | infra | DONE (a4acabd2) |
| 0.7 | step 3 | The checker's records: the PHPDoc resolution record and the expression-type record | 0.4 | infra | DONE (ba5cb5e1) |
| 0.8 | step 3 | O2 as the gate's whole artifact set (`cargo xtask corpus --full`), with every diagnostic field and every cache state, and a configuration with architecture rules and banned types | — | infra | DONE (f273b394) |
| 0.9 | step 3 | The server: an in-process session over Piwigo and two vendor projects, and the edit replay | 0.5, 0.6 | infra | DONE (9d30b919, 6f3ed002, da254e6a) |
| 0.10 | step 3 | Memory: the dhat profile by holder, with the heap at the start of the check and at the check's own maximum, and a profile with untrimmed backtraces | — | infra | DONE (149a2256) |
| 0.2 | step 2 | The `oracle` profile (`gate` with `debug-assertions` and `overflow-checks`, per-file cache off). Run every debug oracle over Piwigo's three configurations, each vendor project with every rule on, and the edit replay; count coverage with an uncommitted counter; fix each failure in its own commit | 0.1, 0.9 | infra | DONE (4c6c0646; fixes cf03d4e7, 21597ad7; runs in `.corpus-runs/gate/oracle-runs/`) |
| 0.3 | step 2 | Delete the debug oracles: `FileBodies`' statements, `BodySource::body`, the debug-only re-exports, the test oracles whose second implementation exists only for them | 0.2 | infra | DONE (693c5d58) |
| 0.11a | step 3 | The registry dump prints the registry as its difference from the stub base it names, for every table, map and set; the stub bases whole in a dump of their own (`stub-bases`) | — | infra | DONE (6d8b8205) |
| 0.11b | step 3 | The runner writes every artifact of one analysis: the front end's dump, the registry dump, the stub bases, the records, the types, the report and its views | 0.11a | infra | DONE (bf16335c) |
| 0.11c | step 3, §11 | One pass (`cargo xtask corpus --full`, `dumps` folded in): every configuration in each of its states with every artifact compared, the sessions, the replays, the vendor projects and the fixture suite, on the worker pool, with its progress; the `oracle` build through the same pass | 0.11b | infra | DONE (dc4da9a2) |
| 0.11d | fast validation, step 1 | The whole-program passes take candidates from the names the analysed files declare, read resolved, and relations from every file; dead abstract classes ask the descendants index; namespace cycles over every project file's classes, reported at the first analysed class contributing; a name declared twice reported at each declaration | 0.11c | sem | OPEN |
| 0.11e | fast validation, step 2 | A builtin method's overrides: the receiver's resolution of each method and the classes below it declaring it with a body (`methods_by_name`), not a lookup in every subclass | 0.11c | perf | OPEN |
| 0.11f | fast validation, step 4 | The registry dump's difference from the base read from the layers when a table shares the base's storage; a test that both paths print the same | 0.11c | infra | OPEN |
| 0.11g | fast validation, step 5 | The canonical printer writes into an `io::Write` sink; runs write their dumps as text with a manifest of digests per entity, the front end's texts included; the replay and the pass compare manifests; the store is a git repository (SHA-256 object format) fed by `git fast-import`, entity texts as blobs whose ids the runs compute as they write, a validated commit's snapshot a commit under a ref named by its hash, the front end's text pool and every existing snapshot imported; later states and the one-thread run given the first run's manifest write only the entities that differ; `compare` is `git diff` between snapshots | 0.11f | infra | OPEN |
| 0.11h | fast validation, step 6 | The canonical printer copies runs of plain bytes when quoting and writing names | 0.11g | infra | OPEN |
| 0.11i | fast validation, step 7 | The edit replay checks every step against a fresh load, the whole-program diagnostics included; its script exercises every dependency kind the edit path tracks, where the project has one, then a seeded random sequence of them | 0.11g | infra | OPEN |
| 0.11j | fast validation, step 8 | The edit path rechecks its dependents in one `check_files` over the worker pool, the property types judged once for all, then reindexes their use sites in one pass | 0.11c | perf | OPEN |
| 0.11k | PHP catalog, step 1 | The tool on clang's AST of php-src, configured in each version's image with every bundled extension, the other extensions the stubs declare read from their source: for every builtin function and method, everything it may do that user code observes (user code it runs on each argument and its own object, storages it writes, diagnostics with their levels and words, class lookups, output, releases, what it returns and throws on each path, code it keeps and kept code it runs, the registrations), each fact under the condition the source tests (type tags, the parameter parsing's coercions by typing mode, classes, constant bits, a URL's scheme, ini settings held open unless configured and never changed); code compiled from a string running anything; the dispatch points modelled (object handlers, class callbacks, stream wrappers and filters, callables, releases to destructors and the cycle collector, external libraries' callbacks, the fiber switch); an unmodelled pointer call, or a builtin no source covers, runs anything | 0.11j | infra | OPEN |
| 0.11l | PHP catalog, step 1 | The same facts for every opcode handler in every specialization, the VM generated with a function per handler, with the operator, conversion, object-handler and class-lookup functions it reaches and its interrupt checks; constructs mapped to opcodes by compiling each AST node kind, with each kind of operand, on each image before optimization | 0.11k | infra | OPEN |
| 0.11m | PHP catalog, step 2 | The recorder as the catalog's oracle on every version image; each construct form run over operands of every type tag, in both typing modes, with an error handler, magic methods, hooks and destructors that record their calls | 0.11l | infra | OPEN |
| 0.11n | PHP catalog, step 3; fast validation, step 3 | The effects read the catalog, the union over the configured versions, built at compile time by `build.rs` into static sorted arrays in place of the table, a malformed entry a build error; each fact where the argument's type may meet its condition: in a summary, a condition on a parameter's value carried to each call, any other value typed from the declarations without flow; a builtin method reaches the overrides of what it may call on its own object; its writes forgotten | 0.11m | sem | OPEN |
| 0.12a | implicit calls, Design | Magic and interface methods, property hooks and lazy objects' initializers as calls, from the catalog's operations under their operand conditions, by the class hierarchy over the value's type; a property magic also where the declared one may have been unset; a hooked access through every class redeclaring the property with a hook; lazy initializers on the classes the program may make lazy | 0.1d, 0.11n | sem | OPEN |
| 0.12b | implicit calls, Design | Destructors at every release the catalog gives, temporaries included, through what the released value holds, transitively; the cycle collector at every release that may leave a value collectable, running every loaded class's destructor; a dead object's writes to its own `$this` not seen; no destructor at a release of a value proven held by a place that outlives it | 0.12a | sem | OPEN |
| 0.12c | implicit calls, Design | Autoloaders (`__autoload` before 8.0 too), the error handler (calls to `#[\Deprecated]` callees and the compilation of included or autoloaded files among its sites), output-buffer callbacks, tick and signal handlers, and the code builtins keep to run later, by the program's registrations and the catalog's sites, conditions and kept code (session handlers, SQLite and XSLT functions, curl and XML callbacks, fetch and node classes among them) | 0.12a | sem | OPEN |
| 0.12d | implicit calls, Design | Generators: a generator function's call writes nothing of its body; each resumption (`current`, `key`, `next`, `send`, `throw`, `valid`, `rewind`, a `foreach` step) writes what the body may write between yields, over the generator functions the receiver may come from; a generator's release runs the `finally` blocks around its `yield`s; a fiber's `start`, `resume` and `throw` write what its callables may write, `Fiber::suspend` anything | 0.12a | sem | OPEN |
| 0.12e | implicit calls, Design | Included files: an `include` or `require` whose path the include check resolves writes what the file's top level writes; an unresolved one writes anything | 0.12a | sem | OPEN |
| 0.12f | implicit calls, Oracle | Fixtures for each kind of site, checked against `php` on every version image that has it | 0.12e | infra | OPEN |
| 0.13a | assumed types, ints, steps 1-2 | An open end of an int range is its limit, one form per set; every int operation exact in `i128`, with the floats PHP gives past a limit, as literals where few (`+`, `-`, `*`, `**`, unary `-`, `++`, `--`, `abs`, `array_sum`, `array_product`); shifts stay ints | 0.12f | sem | OPEN |
| 0.13b | assumed types, ints, steps 3-4 | Int comparisons narrow in integers; `count()` of an array and a list's keys within `HT_MAX_SIZE` by the configured versions | 0.13a | sem | OPEN |
| 0.13c | assumed types, ints, step 5 | `intdiv(PHP_INT_MIN, -1)`'s `ArithmeticError`; appending after `PHP_INT_MAX` (`Error` from 8.0, a warning and no write before); a float key outside the int range, with its deprecation (8.1) and warning (8.5) | 0.13a | sem | OPEN |
| 0.13d | assumed types, keys and properties, step 6 | A read at a key not proven present gives `null` too and is reported with PHP's words and level by version, from the catalog, at reads, compound assignments, `++`/`--`, destructuring and nested reads; a nested write there makes the element from nothing; string offsets the same; a property read where the type leaves the property's presence unknown the same, with "Undefined property", unless the class has `__get`; `report_possibly_nonexistent_general_array_offset` goes | 0.12f | sem | OPEN |
| 0.13e | assumed types, keys and properties, step 7 | Presence proven: the fact that a value is a key of a container place, its sources (`isset`, `array_key_exists`, writes, `foreach` keys, `array_keys`, `array_key_first`/`last`, `key`, `array_search`, `array_rand`, list and string bounds), a property's (`isset`, a write, `property_exists` for a dynamic one), and what drops it | 0.13d | sem | OPEN |
| 0.13f | assumed types, builtins, step 8 | Every builtin's return type joined with the catalog's outcomes on paths its arguments may take that its stub leaves out, its throw points the catalog's exceptions, each changed builtin listed; the iterators' outcomes off a valid position (`current()`, `key()`, `send()`, `getReturn()`); the recorder of a few states as their oracle | 0.11n | infra | OPEN |
| 0.13g | assumed types, iterators, step 9 | At a position not proven valid, the stub's type joined with the catalog's outcomes off one and its exceptions as throw points; interfaces over every loaded implementation | 0.13f | sem | OPEN |
| 0.13h | assumed types, iterators, step 10 | The position as a storage of the object, written by the moving methods and a wrapped iterator's moves, carried by item 0's effects; `valid()` true and an `ArrayIterator` over a non-empty array prove it | 0.13g | sem | OPEN |
| 0.13i | assumed types, throws, step 11 | Each declaration's throws settled with its effects over the call graph: its `throw`s, its calls' (builtins' from the catalog), its operations', its return's and parameters' `TypeError`s, less what its `try` blocks surely catch; a call's throw point its callee's set; `@throws` checked against the set, no longer taken in its place | 0.13h | sem | OPEN |
| 0.13j | assumed types, throws, step 12 | Operations throw what the catalog's opcodes may throw under conditions their operands' types meet (`TypeError`, the `Error`s, `DivisionByZeroError` only where the divisor may be zero, `ArithmeticError`, `ValueError`); each `catch` entered from every point that may throw what it catches | 0.13i | sem | OPEN |
| 0.13k | assumed types, Oracle | Fixtures for each part on every version image, each builtin whose type the catalog changes and each `catch` reached only through an undocumented or understated throw among them; the seven opted-out fixtures take the calls again, expectations made PHP's and names corrected | 0.13j | infra | OPEN |
| 0.11o | fast validation, step 12 | The runtime oracle reaches every asserting fixture: closures invoked, calls retried with the pool's other values, values made for every type, traits through an anonymous class; a fixture reaching none fails unless it opts out | 0.13k | infra | OPEN |
| 0.11p | fast validation, step 9 | `cargo xtask profile <kind> [<project>]`: the frame-pointer build, a warm cache, callgrind and the profile with its binaries and README; then a vendor project's run and a replay profiled again, peak memory, and the pass's wall time, with the catalog in | 0.11o | val | OPEN |
| 0.11q | fast validation, step 10 | Profile a configuration's state, a level, a session and the fixture suite the same way, and plan from them | 0.11p | val | OPEN |
| 0.11r | fast validation, step 11 | `cargo xtask validate`: §11 whole as one command on the worker pool, with its progress and the summary the V row records; the `oracle` pass in every state, the per-file cache included | 0.11q | infra | OPEN |
| 0.11 | step 4, §11 | Item 0's validation and the baseline, from item 0's last commit: the tests and the fixtures; the reports against `5fb858c9`'s, where only item 0's semantic changes (§11) may differ, each difference read; the implicit calls' and the assumed types' fixtures, and the runtime oracle with generated calls, on every version image; the recorder and the construct runs as the PHP catalog's oracle; every dump with the `gate` build, identical at one thread and the default, and with the `oracle` build, with no assertion; the equalities; saved under `.corpus-runs/gate/<artifact>/<commit>.<ext>` with the starting binaries | 0.11r | val | OPEN |
| **1** | **item 1** | **One session** | | | |
| 1.1 | One identity | A file is its resolved path: each link resolved once by `realpath`; cycles found against ancestors' identity; on Windows, identity through `same-file` (now a direct dependency); bind mounts and hard links stay distinct | — | sem | OPEN |
| 1.2 | What the identity decides | Spelling, `__FILE__`, `__DIR__` and owner from the resolved path. The role is the strongest root that contains it. `Session::classify` serves the load and every edit entry point, with an alias table, so a file an edit adds (one under `vendor/` included) takes the role a load gives it, and a deleted directory removes the files under it, which the server keeps today. A file's load order is its place in the walk, an added file's too | 1.1 | sem | OPEN |
| 1.3 | One configuration | `Session::resolve` serves the server too, with an error-policy parameter. A reload applies the difference in file sets. `workspace_config::resolve` is deleted | — | sem | OPEN |
| 1.4 | One load | `Session::load` through a file source with an overlay. The server, tests, `incremental-bench` and benches load through it. The old loaders are deleted. One byte reader. `FileState` keeps each file's content hash. `update_file` builds a file's products through the load's own `parse_file`, so the five walks it runs today go, and with them `project/tests/fused_declarations.rs` | 1.2 | sem | OPEN |
| 1.5 | One analysis | `finalize` by value. The server's first publish is `analyse` plus `finalize`. An edit rechecks through the same check. Whole-program passes run on save, debounced. `defer_inline_ignores` goes | 1.4 | sem | OPEN |
| 1.6 | What the server keeps | `built_once` split. Context trees come from `ast_of`. Only open documents' text is kept. The use-site index becomes a per-file product | 1.4 | perf | OPEN |
| 1.7 | Folders | One session and project per workspace folder; a document belongs to the deepest folder | 1.4 | sem | OPEN |
| 1.8 | Off the message loop | The load runs on the pool with `$/progress`; edits arriving meanwhile are queued as overlays | 1.4 | perf | OPEN |
| 1.V | §11 | Validation against item 0's dumps; the server's first dumps against the CLI's; item 0's edit replay through the server's entry points, every section equal to a fresh load's, load order included, with a link's spelling, a non-UTF-8 file, a watched-file delete, a folder added and removed, and a reload that changes the file set | 1.8 | val | OPEN |
| **2** | **item 2** | **The parallel walk and path data** | | | |
| 2.1 | One listing per directory | The vendor resolver's listings become the walk's table. Classmap and `files` entries under PSR-4 directories are dropped. Stub directories are walked once | — | perf | OPEN |
| 2.2 | One task per directory | Entries sorted by name bytes; kinds from the dirent. Item 1's link rules. Stronger roots and promoted packages pruned. `collect_php_files_excluding` and the name exclusions deleted | 1.2 | sem | OPEN |
| 2.3 | Spellings, path data | Spellings built, not respelled. Resolved path data from the walk; `owner_of_dir` once per directory, in one implementation | 2.2 | perf | OPEN |
| 2.4 | Parsing as listed | Each directory's PHP files go to the pool as one task, with the thread's name cache owned by the load | 2.2 | perf | OPEN |
| 2.5 | Order without comparisons | Keyed by root index and entry index path; explicit files placed by binary search | 2.2 | perf | OPEN |
| 2.6 | A path stored once | `FileTable` as a `HashTable<FileId>` using the walk's hash | 2.3 | perf | OPEN |
| 2.7 | Allocations | `installed.json` and manifests deserialized typed; `eval_concat` into one buffer; `ignore_errors` matched by the walk's text | — | perf | OPEN |
| 2.V | §11 | Validation against item 1's dumps | 2.7 | val | OPEN |
| **3** | **item 3** | **Context trees dropped on the worker** | | | |
| 3.1 | keep / drop | `keeps_tree(role, open)` in the load and in `update_file`; one state shape for a context file, the cached one | 1.4 | perf | OPEN |
| 3.2 | drop, read buffer | The drop runs on the allocating worker. Context files are read into the worker's reusable buffer | 3.1 | perf | OPEN |
| 3.3 | `ast_of` | `ast_of` reads the overlay before the disk | 1.4 | sem | OPEN |
| 3.V | §11 | Validation against item 2's dumps; 3.4 is validated with item 9 | 3.3 | val | OPEN |
| **4** | **item 4** | **The registry by declaration ids** | | | |
| 4.1 | review row 4 | Make the registry's fields private first, so readers go through its API | — | perf | OPEN |
| 4.2 | Three ranges | Each store's ids fall in three ranges, one per source; accessors dispatch by range | 4.1 | perf | OPEN |
| 4.3 | `FileDecls` | A file's declarations stored together in `la-arena`'s typed arenas (new dependency); members and parameters as ranges | 4.2 | perf | OPEN |
| 4.4 | Generations | Ids carry a generation; the id space has one too; a debug build checks it on read | 4.2 | perf | OPEN |
| 4.5 | Member lists | One entry per member a class has, traits' and aliases included; `MethodRef` is (class, slot) | 4.3 | sem | OPEN |
| 4.6 | Never changed | Declarations never change after they are built; every pass result is a table by id | 4.3 | perf | OPEN |
| 4.7 | Parallel passes | Each pass is a parallel map by its relation: hierarchy levels, references between classes, the global-variable include graph, and the purity and property-write worklists | 4.6 | perf | OPEN |
| 4.8 | Scope, snapshots | The scope is a bitset over ids; snapshots and change detection are by id | 4.6 | perf | OPEN |
| 4.V | §11 | Validation against item 3's dumps | 4.8 | val | OPEN |
| **5** | **item 5** | **The merge in parallel; files by id** | | | |
| 5.1 | Ids before the merge | Contiguous ranges by prefix sum of per-file counts, in load order | 4.2 | perf | OPEN |
| 5.2 | Stable ids | A file parsed again on the edit path keeps its ids | 5.1 | perf | OPEN |
| 5.3 | Sharded merge | Over every table the insert writes, each sharded by its own key | 5.1 | perf | OPEN |
| 5.4 | Path rank | Whole-program merges go by path rank; named-argument facts are recorded only when their rule is on | 5.3 | perf | OPEN |
| 5.5 | Keys stay keys | Duplicate sites and origins by `FileId` | 5.3 | perf | OPEN |
| 5.6 | Frontier from ids | `closure_scope`'s frontier starts from item 4's changed ids | 4.8 | perf | OPEN |
| 5.7 | Posting lists | Reverse indexes hold posting lists; a file's references are slices in content order; the per-file cache stores them sorted | 5.3 | perf | OPEN |
| 5.V | §11 | Validation against item 4's dumps | 5.7 | val | OPEN |
| **6** | **item 6** | **The stub scope and the caches' container** | | | |
| 6.1 | step 1 | Deferred types resolved on demand; the eager pass and `class_has_deferred_type`/`function_has_deferred_type` go | 4.6 | perf | OPEN |
| 6.5 | step 3 | Names stored once per cache file; records refer to them by index | — | perf | OPEN |
| 6.6 | step 4 | F2's count as a debug-only diagnostic | — | infra | OPEN |
| 6.7 | step 5 | Records read with `read_at` under a decode context; per-record CRCs; recoverable failures; eager sections; stub index keys read in place; `u64` offsets | 6.5 | perf | OPEN |
| 6.8 | step 6 | The caches' key covers every embedded input: `stub_sources::SOURCES_HASH` from the same list as the embeds; `container.rs`'s `VERSION` goes | — | perf | OPEN |
| 6.9 | Oracle | Tests written: a file replaced while read, a corrupt record, `SOURCES_HASH` against the embedded files | 6.7, 6.8 | infra | OPEN |
| 6.V | §11 | Validation against item 5's dumps; 6.2-6.4 are validated with item 9; item 6's tests (6.9); the platforms job with the file-replacement test | 6.9 | val | OPEN |
| **7** | **item 7** | **One memo for the type relation** | | | |
| 7.1 | read first | The relation's code read for any input beyond the registry's content (thread-locals, the current class, contextual statics) | — | perf | OPEN |
| 7.2 | generation | `ClassHierarchy::generation()`, the content generation; bumped by every finishing run and base switch; read-only levels get one of their own | 4.6 | perf | OPEN |
| 7.3 | one memo | One memo replaces `structural_memo` and `KEEP_DECISIONS` | 7.2 | perf | OPEN |
| 7.4 | final answers | Answers are stored only when final; cycles are resolved per component; constants on a cycle read `never` and are reported | 7.3 | sem | OPEN |
| 7.5 | bound, switch | Two segments per thread; a runtime off switch (`PHPSTAN_RS_NO_RELATION_MEMO`) | 7.3 | perf | OPEN |
| 7.V | §11 | Validation against item 6's dumps; the relation memo on against off | 7.5 | val | OPEN |
| **8** | **item 8** | **Declarations carry only what every reader needs** | | | |
| 8.1 | fields and readers | Every declaration field listed with its readers | 4.3 | perf | OPEN |
| 8.2 | case references | Computed by the checker from what it checks; the six fields leave the declarations | 8.1 | sem | OPEN |
| 8.3 | duplicate params | Resolved as parameters are pushed into the file's parameter vector | 4.3 | perf | OPEN |
| 8.4 | `from_decl`, `reconcile_with_native` | Parameters passed on; a lone member refined and returned with no `Vec` | — | perf | OPEN |
| 8.V | §11 | Validation against item 7's dumps | 8.4 | val | OPEN |
| **9** | **item 9** | **Trees in typed vectors, nodes by index** | | | |
| 9.1 | step 1 | `Expr::peeled` becomes a loop, with no guard, alone | — | perf | OPEN |
| 9.2 | step 2 | `ParsedFile` of typed arenas (`la-arena`): source bytes, `names`, `exprs`/`stmts` with spans, large variants and list elements in their own arenas; per-worker scratch store | 4.3 | perf | OPEN |
| 3.4 | scratch store | A tree that is not kept is parsed into the worker's scratch store | 9.2 | perf | OPEN |
| 9.3 | step 3 | Nodes by index (`ExprId`, `StmtId`): parse functions return nodes committed by their callers; `ParseError` boxed; matching and `syntax::visit` over ids; the inference cache by `ExprId` | 9.2 | perf | OPEN |
| 9.4 | step 4 | Payloads as byte ranges; escaped literals decoded once into the file's byte vector | 9.2 | perf | OPEN |
| 9.5 | step 5 | Nothing changes a tree after its parse. The stub rules apply while a stub is parsed, to stubs only, recording versions. Anonymous-class names go in a side table. Synthetic nodes go in a per-file scratch store. Completion and Composer's reader parse their own `ParsedFile`. `VisitorMut` goes | 9.3 | sem | OPEN |
| 6.2 | step 2 | The stub index records each declaration's, parameter's and per-version type's version range, and each declaration's extension | 9.5 | sem | OPEN |
| 6.3 | step 2 | Guards decided on run-time evidence only (extensions PHP cannot lack, Composer's `platform_check.php`, the configured runtime); an unknown guard keeps both declarations; readers answer for the configured versions | 6.2 | sem | OPEN |
| 6.4 | step 2 | New check: a polyfill's signature against its native one | 6.3 | check | OPEN |
| 9.6 | step 6 | Holders keep ids: disjunction pairs, `Initializers`, alias maps, both query-builder chains | 9.3 | perf | OPEN |
| 9.7 | step 7 | Lists committed together from scratch as `IdxRange`s; kept trees' arenas moved out at exact size | 9.3 | perf | OPEN |
| 9.8 | step 8 | Small nodes; statements and parameters without doc fields (doc sites by offset); attributes stay in their node | 9.3 | perf | OPEN |
| 9.V | §11 | Validation against item 8's dumps, with 3.4 and 6.2-6.4 and item 6's step-2 changes | 9.8 | val | OPEN |
| **10** | **item 10** | **Tokens in flat arrays** | | | |
| 10.1 | Design | Whole-file lexing into a `TokenStream`: three cursors; per-worker buffers; `Token`, `Tok`, `Supply`, the `VecDeque` and `LexEnd`'s second pass go | 9.2 | perf | OPEN |
| 10.2 | Names, literals | The lexer's name table maps bytes to `NameId`; escaped literals go into the file's byte vector | 10.1 | perf | OPEN |
| 10.3 | halt | `__halt_compiler` ends the stream after its three tokens, with the offset recorded; the parser reports what PHP's grammar rejects | 10.1 | sem | OPEN |
| 10.4 | halt offset | `__COMPILER_HALT_OFFSET__` typed as its file's offset; new check of its use in a file without a halt | 10.3 | check | OPEN |
| 10.5 | interpolation | String parts lexed inline; `Lexer::fragment`, the sub-parser, the context clone, the recorder fork and the scratch streams go | 10.1 | perf | OPEN |
| 10.6 | first error | Lexer errors recorded with offsets; the file's error is the first one in it | 10.1 | sem | OPEN |
| 10.7 | docs | A declaration's doc by PHP's rule (only `}` clears it); statements keep adjacency; new check of a doc given away | 10.1 | check | OPEN |
| 10.8 | versions | The lexer reads the configured versions; new check of constructs they lex differently | 10.1 | check | OPEN |
| 10.9 | token readers | Comments recorded only when asked; the stub rules' type strings use a scratch stream; completion uses its own stream; `Parser::new(TokenStream)` | 10.1 | perf | OPEN |
| 10.V | §11 | Validation against item 9's dumps; the versions check's 7.x readings through `docker` | 10.9 | val | OPEN |
| **11** | **item 11** | **Line starts, columns, offsets** | | | |
| 11.1 | step 1 | `LineStarts` for every file, `LineIndex` for analysed files and open documents; `enum Lines` in `FileState` | — | perf | OPEN |
| 11.2 | step 1 | Lines end at `\n` and at a lone `\r`; line comments end at a lone `\r` | 11.1 | sem | OPEN |
| 11.3 | step 2 | The block pass on `wide::u8x16` (a direct dependency now); no AVX2 path | 11.1 | perf | OPEN |
| 11.4 | step 1 | Lexer warnings and parse failures record line and column on the worker; fragment lexers build no index | 11.1 | perf | OPEN |
| 11.5 | step 3 | The server indexes context files on demand: the overlay first, the hash check otherwise | 1.4 | perf | OPEN |
| 11.6 | step 4 | The per-file cache stores `LineStarts`; no `Arc<LineIndex>` | 11.1 | perf | OPEN |
| 11.7 | step 5 | Nothing in the front end reads lines: ignore marks, member spans and doc lines as offsets; anonymous-class lines from `LineStarts` | 11.1 | perf | OPEN |
| 11.V | §11 | Validation against item 10's dumps | 11.7 | val | OPEN |
| **12** | **item 12** | **Lexer details** | | | |
| 12.1 | step 1 | The identifier lookup inlined; watch bits cached with each name in the cross-file cache; stub-attribute spellings go | 10.2 | perf | OPEN |
| 12.2 | step 2 | Keywords compared by word | — | perf | OPEN |
| 12.3 | step 3 | Blank runs: one byte first, the `wide` mask for longer runs | 11.3 | perf | OPEN |
| 12.4 | step 4 | Operators without a call; casts as PHP's single tokens (the parser's lookahead goes) | 10.1 | sem | OPEN |
| 12.5 | step 5 | `RefKey::Constant` keeps namespace and name apart | — | perf | OPEN |
| 12.6 | step 6 | `UseMap::get` stops re-validating UTF-8 | — | perf | OPEN |
| 12.7 | step 7 | Native type names matched without a lowercase copy | 12.2 | perf | OPEN |
| 12.8 | step 8 | A qualified name is one token; the spaced form is a syntax error on 8.x | 10.1 | sem | OPEN |
| 12.9 | steps 9-10 | Numbers parsed skipping `_`; `strip_leading_backslash` returns a slice | — | perf | OPEN |
| 12.10 | step 11 | A doc comment only where whitespace follows `/**` | — | sem | OPEN |
| 12.11 | step 12 | `?>` and heredoc lines end at a lone `\r` too | 11.2 | sem | OPEN |
| 12.12 | step 13 | Heredoc indentation errors reported as PHP's | — | check | OPEN |
| 12.13 | step 14 | Open tags as PHP's: `<?php` only before whitespace; `<?` by `short_open_tag`, reported where the setting is unknown | 6.3 | sem | OPEN |
| 12.V | §11 | Validation against item 11's dumps; the spaced names' 7.x readings through `docker` | 12.13 | val | OPEN |
| **13** | **item 13** | **A file's products: the declaration list and one walk** | | | |
| 13.1 | step 1 | The parser lists declarations. Guards follow PHP's binding: exits include `exit`, `die`, `throw` and `__halt_compiler`; a parent from another file means possibly declared. Entries are reserved in source order. Docs follow item 10's rule, resolved in the entry's scope. A marker records constants a file defines by a computed name | 9.7, 10.7 | sem | OPEN |
| 13.2 | step 1 | New check: a run-time-bound declaration after an unconditional exit is dead | 13.1 | check | OPEN |
| 13.3 | step 2 | `ImportScope`s published once | 13.1 | perf | OPEN |
| 13.4 | step 3 | One walk for `BodySummary`, `InitFlow`, `FileSites` and registered families, in PHP's order of evaluation | 13.1 | sem | OPEN |
| 13.5 | step 4 | The old collector goes; the stub rules record their types' references | 9.5 | perf | OPEN |
| 13.6 | step 5 | A file's bodies as one slice, reached through the declaration's id | 13.4, 4.3 | perf | OPEN |
| 13.7 | step 6 | Builders gather in scratch and emit exact-size slices | 13.4 | perf | OPEN |
| 13.8 | step 7 | References in one vector per file, sorted once; doc sites hold the interned `ImportScope`; `Declaration::name` is a `Name` | 13.3, 5.7 | perf | OPEN |
| 13.V | §11 | Validation against item 12's dumps | 13.8 | val | OPEN |
| **14** | **item 14** | **PHPDoc finished** | | | |
| 14.1 | step 8 | `syntax::memo::Memo` becomes the fingerprint-keyed memo: keyed SipHash-1-3-128 (`siphasher`, now direct) bounded by `quick_cache`'s weighted cache (new dependency); shard clearing goes | — | perf | OPEN |
| 14.2 | step 3 | `parse_tag(tag, rest) -> Arc<TagEffect>`; `apply(block, written, effect)` makes the decisions that depend on the spelling | — | perf | OPEN |
| 14.3 | step 4 | The tag memo keyed by the pair's fingerprint | 14.1 | perf | OPEN |
| 14.4 | step 1 | Eager lowering in `parse_tag`, once per distinct tag line; the statement `@var` memo stays; `impl WrittenType for TypeNode` goes | 14.2 | perf | OPEN |
| 14.5 | step 1 | The text grammar's callers by kind: configured text through `phpdoc::parse_type`; texts in our source as `static` tables; class names from code as `Type::object` (the `Integer::MAX` fix); keyword tests through `types::type_keyword`; `mentions_mixed` from the node; the rest of `parse.rs` deleted | 14.4 | sem | OPEN |
| 14.6 | step 2 | The resolution memo keyed by (written `Type`, context id); the context holds whole `TemplateParam`s, aliases and `self` names | 14.4 | perf | OPEN |
| 14.7 | step 5 | Tag tokens borrow byte ranges; the doc lexed as bytes with PHP's classes; wrapped tags read as logical text | 9.4 | sem | OPEN |
| 14.8 | step 6 | One parse per doc (the grammar's own splitter deleted); one winner rule per tag (`tag_type_nodes`, `select_var_tag_nodes` and `var_type`/`var_name` go); two memos; `DocNames` goes | 14.2, 14.7 | sem | OPEN |
| 14.9 | step 7 | Nodes in their owner's `la-arena`; `DocType` as (owner, `Idx`, `Type`); a serialized `DocType` holds its subtree and type | 14.8 | perf | OPEN |
| 14.V | §11 | Validation against item 13's dumps; the lowering oracle from item 0's recorded type nodes | 14.9 | val | OPEN |
| **15** | **item 15** | **Names held as names, as bytes** | | | |
| 15.1 | C3 step 7 | The checker's method memo and `class_memo` deleted | — | perf | OPEN |
| 15.2 | C3 step 6 | Every `String`-keyed map and identifier value takes PHP's identity (`FoldedName`, `Name`, `ConstName`, `ArrayKey`), in the plan's order of cost, then by module | 15.1 | sem | OPEN |
| 15.3 | end state | `TableKey for str` and `FoldKey for str` deleted; outside text interned where it enters | 15.2 | perf | OPEN |
| 15.4 | bytes | `Name` holds bytes and `Deref<Target = str>` goes; the `\x{NN}` escape wherever a name's text is shown; `parse_display` decodes it | 9.4 | sem | OPEN |
| 15.5 | fixes | `DiagnosticFix` holds bytes; `apply_fixes` and `resolve_span` work on bytes | 15.4 | sem | OPEN |
| 15.6 | serde | `Name` serializes its bytes | 15.4 | perf | OPEN |
| 15.7 | run-time strings | `syntax::name::from_runtime` by PHP's rules; the `trim_start_matches('\\')` and `as_str()?` sites go | 15.4 | sem | OPEN |
| 15.8 | folding | ASCII-only folding at the eight sites that use Unicode lowercasing | 15.4 | sem | OPEN |
| 15.9 | documents | Each open document's encoding found with `encoding_rs` (new dependency); overlays encoded back; fixes offered only when encodable | 15.4, 1.4 | sem | OPEN |
| 15.10 | short names | `NameData.last_segment`; a short name is a slice | 15.4 | perf | OPEN |
| 15.11 | two sites | Name resolution hashes the parts without building text; a callee's folded name reused | 15.4 | perf | OPEN |
| 15.V | §11 | Validation against item 14's dumps; a Latin-1 document in the server session | 15.11 | val | OPEN |
| **16** | **item 16** | **Interned values** | | | |
| 16.1 | name | `Name` as `triomphe::ThinArc<Header, u8>` (new dependency), one allocation, with `last_segment` in the header | 15.10 | perf | OPEN |
| 16.2 | tables | Strong tables: removal at a count of two, re-checked under the shard lock; a sweep at twice the last size; drop after unlock. Type nodes become `triomphe::Arc` | 16.1 | perf | OPEN |
| 16.3 | `PhpString` | Interned as a `ThinArc`, shared by literal types and shape keys; `Interner::str` and `WeakTable` go | 16.2 | perf | OPEN |
| 16.4 | holders | Every holder holds the handle: `FRONT`, memos' kept arguments, the lexer's tables | 16.2 | perf | OPEN |
| 16.5 | memos | The type memos bounded by two segments; no generation | 7.5 | perf | OPEN |
| 16.6 | Oracle | The `loom` model of drop, lookup and sweep (`loom`, test-only); the leak test after the edit replay | 16.2 | infra | OPEN |
| 16.V | §11 | Validation against item 15's dumps; the `loom` tests and the leak test (16.6) | 16.6 | val | OPEN |
| **17** | **item 17** | **Warm runs** | | | |
| 17.1 | step 1 | The stat index: per-platform fields; the racy rule applied where the file was read; `stat` trusted only on filesystems identified as local and fine (`rustix` direct, `sysinfo` new) | 1.4 | perf | OPEN |
| 17.2 | step 1 | Every content hash is BLAKE3 with full digests (`blake3`, new), the build's hash and `SOURCES_HASH` included; `twox-hash` removed | 6.8 | perf | OPEN |
| 17.3 | step 1 | The server writes neither the index nor the base from overlays | 17.1 | perf | OPEN |
| 17.4 | step 2 | The per-file key adds the configured versions and `short_open_tag` | 10.8, 12.13 | perf | OPEN |
| 17.5 | step 3 | The context base: its contents and file list; keyed by a destructured hash of the tracked settings; stored in item 6's container; the cold and warm paths; the server loads from it | 6.7, 17.2 | perf | OPEN |
| 17.6 | Oracle | Tests written: the `stat` cases, the edit replay across two runs, the server loading from the base | 17.5 | infra | OPEN |
| 17.V | §11 | Validation against item 16's dumps; the `stat` cases here and on the platforms job; the cache across configurations; the base absent, written and read; edits across two runs; a warm run leaves the cache directory unchanged | 17.6 | val | OPEN |
| **18** | **item 18** | **The check phase's remainder** | | | |
| 18.1 | step 1 | §12.10: observers and one inference per node; the PHPStan-API walker as a family; literal types once per node; `process_expr` in PHP's order of evaluation | 13.4 | sem | OPEN |
| 18.2 | step 1 | The member-usage family counts a member used when any access may reach it | 18.1 | sem | OPEN |
| 18.3 | step 2 | C1 step 5's tail: effects carried in the statement result; `has_yield` and `collect_throws` go | 18.1 | sem | OPEN |
| 18.4 | step 3 | Variables as slots; the computed-name forms marked exactly; per-body key tables | 9.3 | perf | OPEN |
| 18.5 | step 3 | Scopes as radix arrays, with `IntMap`-style merges and diffs; locals listed by name | 18.4, 16.2 | perf | OPEN |
| 18.6 | step 4 | C2 step 2: conditional subjects by parameter position | — | perf | OPEN |
| 18.7 | step 5 | §12.11's rest: the `MemberIndex` and the normalized-operand fast path | — | sem | OPEN |
| 18.8 | step 6 | C8 step 2's rest: interning from borrowed parts; copy on first change; memos probed borrowed; shared values borrowed; `Display` into the formatter; `concat` through a byte scratch | 16.3 | perf | OPEN |
| 18.9 | step 7 | One argument binding per call | 18.1 | perf | OPEN |
| 18.10 | step 8 | Per-function state reused | — | perf | OPEN |
| 18.11 | step 9 | Generic signatures substituted per type | 8.2 | perf | OPEN |
| 18.12 | step 10, F1 | Messages formatted only when reported; built-in identifiers as `&'static str` | — | perf | OPEN |
| 18.13 | step 10, F2 | Constant types built once; two-operand unions with no `Vec` | — | perf | OPEN |
| 18.14 | step 10, F6a | Borrow, iterate or use scratch | — | perf | OPEN |
| 18.15 | step 10, F6c | `Locals` behind one `Rc`; throw, catch and loop frames as scratch | — | perf | OPEN |
| 18.16 | step 10, F6d-F6e | Narrowing facts in the radix arrays; the separate walks' vectors go with them | 18.5, 18.1 | perf | OPEN |
| 18.17 | step 10, F6f, F4 | Per-text analyses once per distinct text (item 14's memo); SQL parsed once per text; one reused regex `Captures` | 14.1 | perf | OPEN |
| 18.18 | step 10 | One checker per worker | — | perf | OPEN |
| 18.V | §11 | Validation against item 17's dumps; the union oracle from item 0's recorded unions; the replay and merge-over-diff assertions; the 7.x readings through `docker`; types with locals by name | 18.18 | val | OPEN |
| **19** | **item 19** | **Each tree until its last reader** | | | |
| 19.1 | passes | The project-wide passes as families: architecture, banned constructs, class shapes (with parent-call facts), Composer hygiene, mock hygiene, unmatched ignores, the use-site index | 13.4, 18.1 | perf | OPEN |
| 19.2 | retention | `keeps_tree(role, open, mode, checked)`; a batch frees each tree when its check ends | 3.1 | perf | OPEN |
| 19.3 | schedule | Each worker takes the largest file left (`scope_fifo` or an atomic cursor) | — | perf | OPEN |
| 19.4 | judgment | The private-property judgment keeps writes from bound closures and reflection; outside files run only the property-write family | 18.1 | sem | OPEN |
| 19.5 | server | The tree cache: `quick_cache` weighted by bytes, keyed by (`FileId`, content hash); a hash mismatch is an edit | 14.1, 17.2 | perf | OPEN |
| 19.6 | readers | `FileState::ast` goes; every reader asks `ast_of` | 19.2 | perf | OPEN |
| 19.V | §11 | Validation against item 18's dumps; edit replays at a tree budget of zero; a disk change with no event | 19.6 | val | OPEN |
| **V** | **§11** | **O3, once** | | | |
| V.1 | O3 | The frame-pointer `dhat` build; callgrind (`--separate-threads=yes`); `perf` of a warm run; `hyperfine` warm and cold against the starting commit; dhat by holder, the check's live and resident maxima, a second dhat run from the check's start; the server's heap | 19.V | val | OPEN |
| V.2 | results | The measured figures into the plan's §12 and into `WORK_STACK.md` | V.1 | val | OPEN |
