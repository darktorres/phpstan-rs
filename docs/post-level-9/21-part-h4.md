## H4. The pipeline, persistence and the Session (Part F 15)

REG-16.4, CACHE-13.2 and the crate split's F1 and F2 are one design: F2's own design was superseded by
decision 3, and building F1 and then REG-16.4 would build the Session twice. What follows is that one design.

### H4.1 The batch run

1. `Session::resolve(config)`: the database (the corpus `Arc` as its constant, the pool), the inputs (cold: all
   created; warm: restored first, H4.6), the settings inputs, `Packages` from the Composer load (vendor discovery
   still runs beside the manifest load, LOAD-9). Until STUB-11 embeds it the corpus is built beside the walk and
   the products (STUB-3), before the first query.
2. The walk (LOAD-2, as built) numbers each path and makes its `File`; `FileSet` is set once. **The stamp is new
   work** (CACHE-13.1's, W3 step 5): the walk takes no `stat` today (`Listed` holds path, role, stub flag and
   owner), so the loader takes the stamp from the open descriptor, before the read, for the files it reads anyway;
   a lazily reached file has none (H2.2's `file_stamp`). Steps 1 and 2 overlap as they do now (LOAD-8.10, 65 ms warm): the analysed and
   stub walks start at once, vendor discovery runs beside them, the context listing starts when discovery ends, and
   `Packages` and `FileSet` are written when all of them are done; no query runs before that. LOAD-2.4 hands each
   listed directory's files to the pool while the walk goes on, and a write per directory would cancel the queries
   running, so that overlap is kept outside the queries:
   the pool runs the pure function `products_of(bytes, role)`, and its results wait in the load's cache keyed by
   (path, stamp): a query that had to know the digest to look an entry up would read and hash the bytes the pool
   already read. `file_products(file)` finds its entry there, or calls the function, so it stays a function of the
   stamp it read. Whether the overlap matters is S-3's to measure (LOAD-2.4's wall effect was never measured), by
   S-3's stated rule; otherwise the plain order, walk and then products, stands. Input creation is confined to the driver by a private constructor and an `xtask arch`
   rule (salsa does not enforce that a query may not create an input).
3. Load: `file_products(file)` for every file of the **eager set** (the analysed files, scan and stub paths, and
   Composer's `files` entries; functions and constants cannot be autoloaded), in parallel on the pool, each file
   read once and parsed once, its products made: an analysed file's tree is its `parse_analysed` memo, kept for the
   check, and a context file's is built in the worker's scratch builder and cleared (H2.9). Then `decl_index`.
   Context *classes* are not in the eager set:
   - Vendor discovery lists the autoload directories' files as it does today and each gets a `File` (a query may
     not create an input, H2.2, and a located path needs one), but none is read or parsed until a name asks for it.
     When no unguarded eager declaration answers a name, `declarations_named` asks `locate(ClassKey)` and reads the
     located file's products. A guarded eager declaration is a candidate beside the located file (decision 11).
   - `locate` is Composer's `ClassLoader::findFile` over `Packages` and `FileSet`: the classmap entry; else the
     longest PSR-4 prefix and the first of its directories that holds the file; else PSR-0 (STUB-7.5; decision 11's
     "an autoloaded class in the file Composer's loader maps it to"). The directory test is a lookup in `FileSet`,
     so no filesystem read. The index is keyed through the root's case rule (level 9's R7, probed once per root and
     exact on Linux), so a candidate spelled in another case than the disk's finds its file where the volume folds
     case; where the rule's fold is not the volume's the lookup asks the system as discovery does (R9) through
     the tracked `resolve(PathKey) -> Option<resolved spelling>` of H4.5, whose answer is then looked up in
     `FileSet`'s index (a boolean probe yields no `File`). A candidate spelled through a link below an
     autoload directory, which Composer's `file_exists` finds, is not an index entry (LOAD-1.1 resolves each link
     once and `FileSet` holds resolved paths, where eager loading lists those files): the remaining misses are
     decision 14's delta and S-11 tests them with a link below a PSR-4 directory. **Unicode normalisation:** APFS
     and HFS+ look names up insensitively to normalisation while listing the stored spelling, so R7's probe is
     extended with a normalisation test (an entry with a non-ASCII letter looked up in its other normalisation),
     `FileSet` and `shown` are keyed through the rule, and NFC and NFD names are tested on the macOS leg.
   - This is the crate split's "context classes are read on demand through the locators" and the larger win of the
     engine for a big `vendor/`: a vendor file nothing reaches is never read or parsed. **Decided: W3 builds it,**
     so the loader is not written eagerly first and rewritten at W4, and STUB-7.5 moves from W4 to W3.
   - Its prerequisite is not built: the generated-file reader keeps paths only (`GeneratedEntries` drops the
     classmap's class names and the PSR prefixes) and `Autoload` has no `psr-0`, so the maps first keep their keys
     and gain PSR-0. That addition to `composer` is A7, ahead of W3: it changes no output, and is tested against the
     generated files' own arrays on `php` (the lookup that reads them is `locate`, W3's, tested against
     `ClassLoader::findFile`).
   - The delta (a vendor class that no autoload mapping reaches becomes unknown) is decision 14's, and **if S-11
     fails for a package the case returns to you as a proposed change to decision 14; the eager fallback for
     that package ships only after that ruling.** A lazily reached context file is stat-ed by `file_stamp`
     (H2.2). S-11 measures the listing and the read set.
4. Check: the analysed files, largest first, in parallel on the pool: `file_diagnostics(file)` runs `check_file`;
   levels resolve on demand on whichever thread asks first.
5. The whole-program queries, then `analysis(db)`: the finalizer.
6. With persistence on, the database is serialized with no handle alive, and only if something changed (H4.6).

What this removes from `Session::load`: the serial `insert_numbered_file` merge (the declaration index replaces
it), `finish_loading`'s passes (levels on demand), `complete_skipped_references`, and the file table's per-file
bookkeeping that is now `File` fields. The serial stretches the fixes plan measured (the finishing passes,
1.16 to 1.52 s of the off run at 2 to 3 of 8 cores) become the `decl_index` pass plus on-demand work spread over
the pool: REG-7's wall-time prize.

### H4.2 The server

The initial load is the batch load. After it: `didOpen` and `didChange` set the overlay in the `FileSource` and
bump the file's `stamp` (a nonce, which is never a disk stamp); `didClose` removes the overlay and stats the file;
watched-file events stamp, create or drop `File`s and set `FileSet`, **one notification being one batch** (the
stamps, creations and removals are collected, `FileSet` and its indexes are rebuilt once, each input is written
once, and queries restart once; a test creates 30,000 files in one notification). A configuration change writes
the settings inputs. **`reconfigure` runs on the full trigger set** of the crate split's F1, each with the
inputs it writes: a changed `scan_paths`, `exclude_paths`, `stub_paths` or `no_composer` (roles and the eager set
change: `FileSet`, `Packages`); a change to `composer.json`, `composer.lock` or `vendor/composer/installed.json`
and a regenerated `vendor/composer/autoload_*.php` (discovery reads them: `Packages`; with `locate` lazy a stale
`Packages` gives wrong lookups); a change to the baseline or a file it includes (`OutputInputs`); and the
watcher registrations are extended to those files (the watcher registers `**/*.php` only today, so they are
unwatched), so "reload the window" is gone. Then H2.6: `file_diagnostics` in priority order, published as each
completes and only when it differs.

### H4.3 F1, reconciled

| F1 item | on the engine |
|---|---|
| `AnalysisResult` owns its data | `analysis(db)`'s value: file and not-file errors, internal errors (the records `check_file` catches at its own boundary: H2.8; a file holding one is not persisted), stats, warnings, the inline-ignore records (from `file_products.ignores`, so a persisted result finalizes without parsing its file); "N more file(s) loaded for context" counts the files the load listed, a function of the inputs |
| the finalizer | a function of `file_diagnostics`, the whole-program results and `OutputInputs`; stale baseline and unmatched-ignore entries are its output |
| entry points | `Session::run`, `update(FileDelta, &CancellationToken)`, `reconfigure(Config)`, `generate_baseline`, `apply_fixes`, `snapshot()` (a database handle for one request); `Project`'s ~15 entry points collapse into them |
| `Project` folds in | the file table is `File` and `FileSet` plus the VFS; the registry is `Provider` over the engine; `dependents` is gone; `use_sites` and the escape sets are `file_products.use_sites` and a project query; `pest_config` is a query; `sql` is an input |
| the VFS | contents from `FileSource` (overlay first); membership decided once, by `classify` (LOAD-1.2, built), for the walk, a file the editor opens and a watched-file event alike; identity is the canonical path (LOAD-1.1, built; one canonical form on every platform, level 9's R3); URIs are level 9's R5 (RFC 8089 on each platform, an unopened file named in the spelling its client gave the folder), lines and file events F1's text unchanged |
| panic isolation on the pool | H2.8 |
| cancellation and the event loop | **changed.** F1 has requests run "against the last published snapshot while an update builds the next". salsa has one current revision: a write cancels the other handles' queries. A request is answered from its handle, and a request cancelled by a write answers `ContentModified` (or is run again by the loop) — rust-analyzer's model. F1's "publish the edited file first, then the open documents, then the rest" is H2.6's order of queries |
| last good AST | **gone with recovery.** The engine needs `parse` to be infallible, so REG-16.4 needs SYN-1.9 (batch 3), which the sequence already has first |
| `reconfigure` | settings, `FileSet` and `Packages` writes (H4.2) |
| one Session per root | one `ProjectDb` per root, sharing the pool, the interner and the corpus `Arc` (each database's corpus constant is the same `Arc`) |
| files outside every folder, untitled buffers | a database rooted at the nearest config or `composer.json`, else for the file alone; an overlay-only file has a `File` with `overlay` and no stat |
| the workspace load runs as the CLI's | `Session::load` (H4.1) |
| LSP parity | the project-level rules are queries, so they run on every update by construction, their per-file data kept as products |

### H4.4 F2, reconciled

| F2 item | on the engine |
|---|---|
| resolved dependencies, absent names included, includes, trait hosts | the recorded reads: an absent name is a `declarations_named` read; an included file is a `file_products` read of it; a trait's host reads the trait's levels. `deps.rs`, `Project::dependents` and `signature_dependents`, `register_references`, `transitive_affected_files*` and `bare_name` are deleted, and with them the whole of PROD-5.4, PROD-5.7 and LOAD-4.7's file-references half (Part E 35) |
| fingerprints per exported name | early cutoff on `class_final`, `function_final` and the constants, and per member on `class_member` (H2.4): a "fingerprint" is the equality of the value a reader read |
| incremental collectors | per-file facts as `file_products` fields and queries, and the whole-program queries over them |
| the persistent cache per analysed file | the database's memos: `FileCheck` (raw errors before the finalizer), the products, the levels, the verdicts, with the dependency graph |
| the key | the file is named by (build identity, project root, config key). The config key hashes `ReflectionInputs` and `CheckInputs`; these are *also* inputs, so a change inside a session invalidates exactly its readers; `OutputInputs` are in neither. Alternating configurations keep their own files (F2's entry sets), the four most recently used kept |
| location, mode `0700`, `CACHEDIR.TAG`, atomic write under a lock, `--no-result-cache`, `clear-result-cache` | unchanged in what they ask, and built for every platform (level 9's R11): the directory is `dirs::cache_dir()` joined with `phpstan-rs` (`PHPSTAN_RS_CACHE_DIR` overrides), so `~/Library/Caches` and `%LOCALAPPDATA%` as well as `$XDG_CACHE_HOME`; mode `0700` is Unix's, and a Windows directory under `%LOCALAPPDATA%` is the user's by inheritance, with ownership and not-a-link checks before an existing directory is read or written; the write is a temporary file renamed over the target, as the two caches do today, which replaces an existing file on Windows too unless another process holds it open, so a reader takes no lock and sees the old file or the new; **the lock is a dedicated `.lock` file** (`File::lock`, portable; the data file is never the lock target, since Windows' lock is mandatory against reads by other handles), taken by the writer around the temporary write and the rename, and a lock failure runs the batch uncached with one warning; retention is the four most recently used config keys per project root (F2's entry sets) under a byte cap (`PHPSTAN_RS_RESULT_CACHE_MB`, default 2048 as the file cache's), last use refreshed on a read at most every 12 hours (`REFRESH_AFTER`); the per-file cache (`file_cache.rs`) and its switches keep serving until CACHE-13.2 replaces them and then go, with `--no-result-cache` taking their place; a result file made by one platform's build is never read by another's, since the build is part of its key, and whether `debug_assertions` belongs in the key is decided in the commit that writes it |
| the LSP writes back only results whose overlay matches the disk | a result whose reads include an overlay's stamp is dropped from the write (H4.6), since an overlay's stamp is never a disk stamp; the server serializes at shutdown or after an idle period, not "at any quiet moment" |

### H4.5 Probes outside the file set

`include_target_exists` (`checker/includes.rs:283`) calls `is_file()` on the live filesystem. A query may not,
and a query that reported an untracked read would run again every revision, which for the many files that
`include` something would cancel the point of a warm run (a minority: 242 of Piwigo's 2,798 non-vendor files name
`include` or `require` by a loose count). The probe is a query: `path_is_file(db, IncluderKey, PathKey)`, a
tracked function that stats the path, **keyed by the includer** and reading its stamp, over a singleton input
`ProbeEpoch` that the driver bumps when the filesystem may have changed: once when a persisted database is
restored, on every watched-file event of any extension (the watcher registers `**/*.php` only, so a created
include target of another extension, or outside the watched folders, would move no epoch: watchers are
registered for the directories probed), and on `didSave`. Keying by the includer means an edit to the includer
re-probes its own paths, as every recheck of an includer calls `is_file` afresh today (creating the target and
then editing the includer clears `include.fileNotFound`). Each probe re-stats when the epoch moves, and an
unchanged answer stops there. The probes' paths are interned keys, so a restored database knows which to ask
again. `resolve(PathKey) -> Option<resolved spelling>` is the same design for `locate`'s fallback (H4.1). S-10
counts the distinct probes on the corpora, requires zero stat calls without a bump and a stated ms per bump.

### H4.6 Persistence

- **Format:** salsa's `persistence` feature, with the heavy values `Lazy`, in one whole in-memory buffer (salsa
  reads each memo key as a borrowed `&str`, which needs it; the project's `postcard` is such a format) inside an
  envelope of magic, format version, length and checksum, as `file_cache.rs` has. A tracked function's memo is
  written by its result type's own serde impls (`persist` on a tracked function takes no functions of its own; on
  an input, an interned struct and a tracked struct it may, `salsa-macros/src/options.rs:318-333` with
  `tracked_fn.rs:66` against `input.rs:72`, `interned.rs:72`, `tracked_struct.rs:68`). So the levels' and the
  check's results are types whose `Serialize` writes the bytes `Lazy` already holds and whose `Deserialize` reads
  them back as bytes, decoded on first `get` (CACHE-12's `Lazy`, whose equality by bytes and file number is
  already what cutoff needs). Restore is then a copy plus the graph, not a decode of every value. S-4 decides
  whether this beats the cache's own container (CACHE-9 and CACHE-10): if restore is slower than today's
  per-file decode in wall time, the container carries the values and salsa carries only the graph.
- **Persisted: the whole chain below every persisted memo, down to the base inputs.** salsa does not flatten
  onto the persisted queries: it replaces a non-persisted dependency by the base-input leaves beneath it, and
  its nested walk descends through every edge and inserts only input fields and persisted collectable interned
  structs (H1.0 10). If `check_file` were persisted and `declarations_named`, `decl_index`, the levels,
  `class_member` and `path_is_file` were not, every restored memo would hold edges to every eager file's stamp,
  `FileSet` and `Packages` instead of to the values it read: cutoff would exist only in the session that built the
  memos, one changed stamp or one created file would rerun every `check_file` that read a name after a restore,
  and each memo's edge list would be the whole leaf set. So persisted are: **every input a persisted memo reads**
  (`File`, `FileSet`, the settings, `Packages`, `ProbeEpoch`, `Extensions`, `OutputInputs`, `SqlSchema`: salsa
  asserts at serialization that each such input is `persist` and panics otherwise, so an `xtask arch` rule says
  every salsa input is `persist`, and an engine test serializes a database after one `check_file` and one
  `analysis` with every singleton present); the interned keys; `file_digest`; `file_products`' fields;
  `decl_index`, `declarations_named` and `hierarchy_cycles`; the five levels and `class_member`;
  `function_final`; the constants; `path_is_file`, `file_stamp` and `sql_columns`; `check_file`, the verdicts,
  `file_diagnostics`; and the whole-program results, with `Lazy` values that do not duplicate shared `Arc`
  structure. **Not persisted:** the trees (`parse_analysed`; a context tree is no memo), the accessors' tables,
  and `Builtin`, which is a constant of the database (the process-wide corpus, H2.2) and so a persisted memo
  needs no input for it. The alternative, if the chain is too large (S-4 measures its bytes per memo and per
  edge), is to keep the dependency graph in our own file. A path is persisted as a Unix name's bytes
  (`OsStrExt::as_bytes` and `from_bytes`) or a Windows name's UTF-16 code units (`OsStrExt::encode_wide` and
  `OsStringExt::from_wide`), as one pair of functions added to `path_bytes` (which cuts components and has no
  such API today), not through serde's `PathBuf`, which refuses a name that is not UTF-8.
- **Restore: two paths, never mixed.** salsa deserializes into a database that has no input (a singleton that
  exists already panics, and input slots are re-allocated in the saved order under an assertion; a
  never-changing input cannot be set again), and `deserialize` changes the live database in place and is not
  atomic. *Cold:* `Session::resolve` creates every input. *Warm:* the file is read whole and its envelope
  verified before any byte reaches `deserialize`; `deserialize` runs under `catch_unwind` into a database made
  for the restore; then each singleton is fetched (`try_get`), those the file lacked are created, and the
  others are set only when they differ (an input's setter is a change whatever it is given). **On any error or
  panic that database is dropped, one warning is printed, the run is cold, and the file is replaced at the end;
  a database that saw a failed restore is never reused** (a truncated stream leaves salsa's ingredient `Vec`
  one element short, and an out-of-range index panics in `Vec::remove`; without this a corrupt file, one from a
  skewed build, or one written by another local user into a shared cache directory would fail every run the
  same way until someone cleared the cache). The driver then reconciles: each walked path finds its persisted
  `File` through the persisted path index, **keyed by the exact canonical spelling** (a case-only rename is a
  delete and a create; the index covers every `File`, dead ones included), and its stamp is set if the `stat`
  differs or the racily-clean rule (CACHE-13.1) distrusts it, and on Windows or a file system the rule does not
  know the file is read and hashed instead (decision 13); new paths get new `File`s; paths that are gone leave
  `FileSet`; the settings and `Packages` are compared; `ProbeEpoch` is bumped. A lazily reached context file's
  stamp is `file_stamp` (H2.2), so nothing is "the files a persisted memo names". Queries then verify, and run
  only where a read changed. Nothing the engine sorts is ordered by `FileId` (H2.3), so a restored database's
  numbering (old numbers kept, new paths given the next free one) cannot change a result.
- **Write only when something changed.** The database is serialized at the end of a batch run only if a query
  executed or an input changed since the restore (an event-callback counter), the report is printed before the
  write, and a no-change warm run rewrites nothing (the project's rule for its caches). `as_serialize` cancels
  other workers and waits for their handles, so the server serializes at shutdown or after an idle period,
  never while a request runs, and S-4 bounds serialize time. Memos derived from a dirty buffer are not written:
  an overlay's stamp is never a disk stamp, and a result whose reads include one is dropped from the write, not
  trusted (F2 writes back "the results of files whose overlay matches the disk; an overlay's results are never
  cached").
- **Compaction (Part E 38, ruled 2026-10-18):** interned keys and `File`s of things that no longer exist stay in
  a persisted database, and an immortal `DeclKey` also keeps the memos of a dead declaration. The rule is a size
  ratio: when the keys, dead `File`s included, outnumber the live declarations several times over, the database is
  rebuilt. *At the start of a run* that means the run starts cold and writes a fresh file. *In a server* it means an
  in-process rebuild, because a server that runs for days has no next start: a new database is loaded from the live
  file set at idle (cold, or restored from the last file written while the ratio was fine) and replaces the old one
  once it has caught up, and the old one is dropped with its last handle. S-4 picks the ratio and measures the
  rebuild from a series of edit-replay runs, and S-3 measures what an immortal key retains before it.
- **Sequence:** CACHE-13.1's stat stamps first (they feed `File.stamp`), then the persistence, with the cache
  closure gate: a sequence of edits applied through cached runs gives, at every step, a cold run's result; a
  warm run after no change equals the cold run and executes no `check_file` (counted from the event callback),
  **even when `ProbeEpoch` is bumped**; after a restore, one body edit and one file creation rerun exactly the
  `check_file` set the live session reruns; a panic-injected file is in the warm-run gate; and the restore is
  tested with a truncated file, a flipped byte behind a recomputed checksum, an out-of-range ingredient index
  and a file from another build key.

### H4.7 W3's order

1. The file source and `File` inputs are built (LOAD-1.4). `file_digest`, `parse_analysed` and `file_products` become
   queries, and `file_decls` flips from W2's input to a field (H3.6).
2. `check_file`, `file_diagnostics`, `property_type_verdicts`, the whole-program queries and `analysis`.
3. The server's update path moves to H2.6; `update_file*`, `walk_affected`, `closure_scope`, `dependents` go.
4. The batch driver (H4.1); `finish_loading` (94 calls, 116 mentions at `ffa7cccb`) and its callers become `Session`.
5. CACHE-13.1, then CACHE-13.2 and the cache closure gate.
6. TYPE-4 on the revision, REG-16.5, LOAD-3.1, LOAD-5.2, LOAD-5.1's use-site index, REG-17 and CHECK-7, CHECK-8.
7. At every step the edit replay (G5's tool 7) equals a fresh load.

### H4.8 Interim work the engine deletes

`deps::Collector`, `recorded_references`, `RefKey` and the dependents indexes exist only to decide which files to
recheck (`deps.rs` says so: "used purely to decide which files might need re-checking"). PROD-5.7 (references as
one vector per file), LOAD-4.7's file-references half and PROD-5.4's rewrite are work on them; REG-16.4 deletes the
machinery whole. PROD-5.8 (the record of written names) stays: its readers are rules, not the dependents index.
Part E 35.
