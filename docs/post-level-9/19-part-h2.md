## H2. The engine (Part F 13)

### H2.1 The contract, and what it is built on

What the code above the engine may rely on, whichever engine is below:

1. **Inputs change only by a driver write.** A query is a pure function of inputs and other queries: no clock,
   no environment, no filesystem except through the file source, no state that depends on the thread or the
   order.
2. **Reads are recorded:** every input and query a query called, an absent name included.
3. **Early cutoff.** A new result that is equal (H1.5b) to the old one stops the change there. salsa backdates
   only when the new memo has no cycle heads, is not provisional and did not drop in durability, and only when
   the old value is still there (`function/backdate.rs`): so every member of a recursion SCC (purity, property
   writes, inference, constants on a cycle) re-runs its readers whenever anything in the SCC re-executes
   (cheap for constants and purity, relevant for inference), and an evicted `parse_analysed` cannot be
   compared; eviction costs less than it looks, since a re-executed memo's `changed_at` is the maximum of its
   inputs', so an evicted tree re-parsed from unchanged inputs invalidates no reader, and one whose inputs
   changed could not have been backdated anyway.
4. **A write cancels** the queries running on other handles; a cancelled unit is retried or dropped, and no
   partial result is ever visible.
5. **A cycle** is resolved by its rule (H3.3) or is a bug, reported as one.
6. **Results are `'static` values.** A borrowed view lives as long as a database borrow.
7. **Determinism:** the same inputs give the same results at any thread count and in any order (B8's
   equalities, tested through G5's dumps).

Built on salsa 0.28.5 in the engine module (Part E 33; a crate from G1). It uses `#[salsa::db]`, `input`,
`interned`, `tracked` (with `returns(ref)`, `lru`, `cycle_result`, `cycle_fn` and `cycle_initial`, `heap_size`),
`Durability`, `Cancelled`, `cancellation_token` and `persist`, and one tracked struct, `FileProducts` (H2.3),
whose fields are tracked one by one (`tracked_struct.rs`: `update_fields` moves a field's revision only if its
value changed; `tests/expect_reuse_field_x_of_a_tracked_struct_changes_but_fn_depends_on_field_y.rs` is the
pattern). It does not use accumulators: a diagnostic gathered in an accumulator is outside the result, so
outside cutoff and outside the persisted value. **The feature set is pinned:** `salsa` with
`default-features = false` and `macros`, `inventory`, `persistence` and `salsa_unstable` (`heap_size` needs the
last; `rayon` is used by salsa only for an `Either` impl and `accumulator` is unused), and `triomphe` without its
default features; `persistence` adds `serde`, `erased-serde` and `thin-vec/serde`. The crates new to `Cargo.lock`
are `salsa`, `salsa-macros`, `salsa-macro-rules`, `boxcar`, `intrusive-collections`, `inventory`,
`portable-atomic`, `thin-vec`, `tracing`, `tracing-core`, `typeid`, `erased-serde` and `triomphe`. The repository
runs `cargo deny check` (`deny.toml`, `rust-ci.yml`), so S-1's and S-2's commits pass it (advisories, yanks,
licences), and S-3 adds the clean and incremental build times and the release binary size to its measured
list, since the project has a build-time gate.

### H2.2 Inputs

| input | grain | holds | durability |
|---|---|---|---|
| `ReflectionInputs` | singleton | reflection settings, the configured PHP version range (decision 6; when set it overrides every owner's), assertion tables, `stub_paths` | high |
| `CheckInputs` | singleton | the rule settings and the level | high |
| `OutputInputs` | singleton | `ignore_errors`, the baseline, the formatters' settings; read only by `analysis` | high |
| `File` | one per path ever seen | `path` (set once), `shown` (the path as reports spell it, R6's `/` form relative to the report root, set with the path and by the driver when the root changes), `number: FileId` (an opaque handle: nothing is ordered by it, E1-10), `role`, `analysed`, `stamp` (analysed and eager files; a lazily reached context file's is the query `file_stamp`, below), `overlay` | analysed: low; context: medium; configured stubs and the installed PHPStan's files: high |
| `FileSet` | singleton | the sorted `Vec<File>` that exist now, with the path index that `locate` and the include resolution read, and the index by `shown` that an anonymous class's name is resolved through (H1.4) | low |
| `Packages` | singleton | Composer roots, autoload maps with their keys (classmap entries, PSR-4 and PSR-0 prefixes with their directories, files), owners, vendor directories, and per Composer root its declared PHP range (`config.platform.php`, else `require.php`) and its required packages (Part E 42) | medium |
| (`Builtin`) | **not an input** | the stub corpus (`registry::stubs::Corpus`, an `Arc` shared by every database in the process) is a constant of the database: the build identity in a persisted file's key covers any change of it, and a never-changing singleton could not be persisted without a custom codec (salsa asserts that every input a persisted memo reads is `persist`, and a custom `deserialize` gets no database argument) or set again after a restore | – |
| `SqlSchema` | singleton | the schema file's digest, or a live reflector's fingerprint | medium |
| `ProbeEpoch` | singleton | a counter the driver bumps when the filesystem, or a live SQL schema, may have changed (H4.5) | low |
| `Extensions` | singleton | the registered extension implementors and a fingerprint of the set and its configuration (J2) | high |

- Splitting the settings in three is the grain at which a change should reach: a formatter or baseline change
  must not re-resolve a class, and a rule change must not either. Within an input the grain is the field: a salsa
  input field has its own revision and durability, and a read records that field alone (`input.rs:230`). So each
  setting is a field of its input, and the checker reads settings through a view that fetches a field from the
  database the first time a check uses it and keeps it for the rest of that check (one `OnceCell` per field).
  The recorded reads are then the settings that file's check consulted, and toggling a rule reruns the files in
  which that rule's setting was consulted, not every file. Copying every field into `Checker` when it is built
  would make each file depend on all of them, so the checker's `self.settings.x` reads become calls on the view
  (mechanical: 114 of them in 34 files of `checker/` today, reaching the leaf flags of the extensions'
  `…Settings<bool>` structs, which are the fields).
- **The range of a file is its owner's (Part E 42, ruled 2026-10-18).** The owner of a file is the nearest
  ancestor `composer.json`, a fact of each directory's listing: A8's scan finds a degrade root's nested manifests before
  the walk (COMP-8) and `Packages` holds those directories (A1 builds the vendor fact beside it and not this one); a file
  with no owner uses the project root's manifest. `range_of(File)` is a
  tracked projection of `Packages`: the configured range if one is set, else the owner's declared range (Composer's
  grammar by interval arithmetic, COMP-2), else decision 18's default (open above, ruled 2026-10-18: Part E 40, M5d). It compares equal when the owner's
  range does, so a manifest edit reruns only the files it owns. A declaration's validity is a `VersionSet` that
  does not depend on the range, and the reader intersects it with its own file's range (J1.2); a builtin reading is
  selected with that range (`Readings::over`). A level's memo is keyed by the range only where something it reads
  differs across versions (J1.2), so a class with no version-sensitive ancestor has one memo whatever the owner's
  range, and a project with one range pays nothing. Activation is any owner's: it only turns checks on. A hull of the owners' ranges would judge Symfony's 202
  manifests at `>=8.4.1` against versions they cannot run on, and the root's alone would judge its 8 manifests at
  `>=8.1` as if they needed 8.4.
- `stamp` says whether the file's content may differ, and what it holds follows decision 13 (CACHE-13.1). On a
  file system known to keep a change time that no program sets (ext4, xfs, btrfs, APFS and the like) it is the
  `stat` tuple, trusted under the racily-clean rule, whose reference time is the clock reading taken at the file's
  own stat (Part E 15; decision 13's "the index's own write time" does not fit a database written after the whole
  check); on any other, and on Windows, it is the content hash, so
  the file is read and hashed. Either way the driver bumps it when the content may differ (an overlay changes, or
  the rule distrusts the tuple). The walk takes no `stat` today, so the stamp is new work: for the analysed and
  the eager files, which are all read anyway, the loader takes it from the open descriptor before the read. A
  **lazily reached context file** (H4.1) has no input stamp, because a query may not write an input: its stamp is
  the tracked function `file_stamp(File)` over `ProbeEpoch`, statted by the query that first needs it before it
  reads the bytes (the design of `path_is_file`, H4.5), so a first read is tracked, a later edit is found when
  the epoch moves, and a restore does not turn "none" into "some". The file *source* is not an input; it is
  a handle in the database (the overlay map and the disk), read only by `file_digest(File)` and
  `parse_analysed`. Because the driver bumps `stamp` whenever the content may differ, a read is always
  consistent with it.
- `shown` is a field because the checker takes it (`set_current_file`, level 9's R18) for the path
  rules (`in_test_code`, the restricted symbols' path rules) and the finalizer matches `ignoreErrors` paths and
  baselines against it (R6); an anonymous class's name carries it. It is a function of the path and the report root,
  so it is a field of the input the driver sets and not a query; a change of root rewrites it, and only the files
  that read it rerun.
- **Honesty rule:** an input is changed by the driver and by nothing else, and a query reads nothing but inputs
  and queries. The checker has two live reads, and both are made queries over the epoch input to keep it:
  `include_target_exists` (`path_is_file`, H4.5), and a live SQL reflector, whose `PREPARE` per distinct SQL text
  (`resolve_sql_row_type` falls through to `context.reflect(sql)`, cached by text in `SqlContext`'s own map) is
  the tracked function `sql_columns(SqlText)` over `SqlSchema` and `ProbeEpoch`: the driver bumps the epoch at
  each run start and when drift detection fires, so a persisted answer is asked again once per run and an
  unchanged answer cuts off, and `SqlContext`'s own cache goes (the memo is the cache). A closure-gate case
  changes the test database's schema between two runs: the warm run's diagnostics equal a cold run's.

### H2.3 Queries

`returns(ref)` throughout: a reader gets `&'db T` from the memo, no clone. Equality is H1.5b's.

| layer | query | key | result | notes |
|---|---|---|---|---|
| file | `file_digest` | `File` | the content digest, nothing else | **analysed files only.** Depends on the stamp (`File.stamp`, or `file_stamp` for a lazily reached file) and is persisted (digest only). The text is not a result: `parse_analysed` reads it from the `FileSource` handle under the stamp's protection, so a restored `file_digest` memo that is valid by stamp never hands out a missing text (H4.6). A context file keeps no text (LOAD-3.4): its `file_products` reads the stamp and the bytes once, in the worker's reusable buffer, and the products are the cutoff, so a touched but identical file stops there; its skimmed bodies' text is a field of its products, below |
| file | `parse_analysed` | `File` | `Arc<Parsed>`: the tree, errors, marks | a tracked function for **analysed files only**, which need their tree twice (their products, then their check). Kept for the whole run in the batch (no `lru`) and under `lru` in the server (H2.9). A context file has no tree query: its tree is built inside `file_products` (H2.9). Infallible (G3) |
| file | `file_products` | `File` | a tracked struct with one tracked field per product: `names` (PROD-5.8's record), `decls` (`FileDecls`, positionless, H1.5a), `decl_names` (the file's `DeclRef`s alone, anonymous classes left out, which `decl_index` reads), `decl_edges` (each class's `extends`, `implements` and `use` names as written, positionless, which `hierarchy_cycles` reads), `sites` (the positions), `facts` (mentions flags, `has_declaration_in_body` and the rest, and the file's validity `VersionSet` from its marks, G3.5), `globals` (H3.7), `bodies` (a context file's skimmed function bodies, each copied out by ordinal as PROD-4.2 does today, so that `decls` holds no body text and a body edit leaves it equal; an analysed file's bodies are in its tree), `doc_blocks` (DOC-2.9: each doc comment parsed once and found by its offset, read by the checker and by no level), `ignores`, `lines` (`LineStarts` or `LineIndex`, SYN-3.1), `use_sites` | made in one pass over the tree, so no intermediate is a memo: an analysed file reads its `parse_analysed` memo, and a context file's tree is built in the worker's scratch builder and cleared once the products exist (LOAD-3.5); **a reader depends on the field it reads:** a body edit leaves `decls` equal, a member edit changes `decls` and leaves `decl_names` equal, a line inserted above a class changes `sites` and nothing else |
| index | `decl_index` | none | names to `[DeclRef]`, over every eager file's `decl_names` | O(all declarations); see below |
| index | `declarations_named` | `ClassKey` / `FunctionKey` / `ConstKey` (generic over the key) | `Arc<[Candidate]>`, sorted by **the files' path order** (the `FileSet` order), never by `FileId`: a `DeclRef` (or `ConstDeclRef`) and its validity `VersionSet` (the `Availability` over a range is the reader's, H2.2) | **the recorded read of a name.** An absent name is an empty slice. A file's declarations exist only where its syntax holds (decision 15, Q-10): the availability of a project or context declaration is its file's validity `VersionSet` (G3.5's marks, a field of `facts`), which the reader meets with its own file's range (`range_of`, H2.2), and a file valid nowhere in that range gives that reader no candidate, as PHP refuses it there. A file number is first-seen order (`file_table.rs`: a new path takes the next free number), so a database edited into its state and a fresh load number the same paths differently; sorting by path order (which `FileTable::rank_of` already is) is what keeps candidate order, the `global_site_index` lists and every accumulation, and with them a union's member order and "previously declared at", equal between the two (a test builds one file set by two histories and compares `registry-dump`, candidate order included) |
| index | `hierarchy_cycles` | none | the edges dropped | one pass over the declared edges (`decl_edges`) of the project's and context's classes (H3.3); a member or signature edit reruns nothing in it |
| class | `class_ancestors`, `class_own`, `class_inherited`, `class_members`, `class_final` | `DeclKey` | REG-7's five levels | H3 |
| class | `class_member` | `MemberKey` (`DeclKey`, `MemberName`) | `Option<Arc<Member>>`: one member of `class_final`, absent included | a projection, so its readers rerun only when that member changed (H2.4) |
| function | `function_final` | `DeclKey` | REG-7.12 | |
| constant | `global_constant` | `ConstDeclRef`, interned | type and throws | keyed by a declaration like the levels; the name-level combination of candidates is the `Provider`'s (H3.4) |
| constant | `class_constant_value` | (`DeclKey`, name) | value type and throws (decision 1) | `cycle_result` |
| constant | `class_constant_value_only` | (`DeclKey`, name) | the type without the constant's PHPDoc, and whether the PHPDoc was dropped | the fallback of the PHPDoc case in H3.3(2); no PHPDoc edge enters it |
| body | `body_summary` | (`DeclKey`, member ordinal) | `BodySummary`, `InitFlow` | on demand, from the declaration's body text |
| body | `purity`, `property_writes` | `DeclKey` / member | verdicts (REG-1) | `cycle_result` |
| body | `receiver_types` | (`DeclKey`, member ordinal) | the inference in a no-effects mode, for the receivers that stage (H3.8) | may not read `purity` or `property_writes`; read by both |
| body | `infer_body`, `inferred_return`, `inferred_throws` | `DeclKey` / member | REG-17 | fixpoint with widening (H3.3); `infer_body` reads `purity`, `property_writes` and `receiver_types` |
| body | `effective_return`, `effective_property` | `DeclKey` / member | the declared type, else the inferred one (H3.8) | the inference family; the `Provider` reads signatures from `class_member` and these |
| check | `check_file` | `File` | `FileCheck`, a tracked struct with one tracked field per fact (diagnostics before finishing, member refs, escaped enums and classes, named-argument facts, property-assign facts, property-type sites), so each pass depends on the fact it reads | the type dump is a request, not a memo |
| check | `property_type_verdicts` | the class group (the declaring `DeclKey` of the writer scope) | `judge_property_types`'s verdicts for that group | `judge_property_types` merges the facts of every writer in path order, expands them through used traits and re-runs the checker on the other files declaring the class "for their writes alone" (`project.rs`), so the verdicts are keyed by group over an index query of writers built from the check facts; a keyless query would make the first worker to arrive compute every `check_file` while the others block |
| check | `file_diagnostics` | `File` | `check_file`'s diagnostics with the per-file project rules added (duplicates, ignore parse errors) and the verdicts of this file's own property-type sites (`check_too_wide_property_types`), sorted | the batch phases stay explicit: all `check_file`s first, then the verdict-reading diagnostics and the passes |
| project | `global_sites` | `File` | the file's `FileSites`: its global-variable facts resolved through the provider | H3.7 |
| project | `global_scope_files`, `global_site_index` | none | the files whose top level runs in global scope; each name to the files that assign it | H3.7 |
| project | `global_variable` | `GlobalKey` (a `Name`, exact) | one global's accumulated type | H3.7; the provider's `global_variable(&Name)` |
| project | the whole-program passes | none | dead code, class shapes, architecture, banned constructs, Composer and mock hygiene, unmatched ignores | each reads per-file facts, not trees (LOAD-5.1) |
| project | `analysis` | none | `AnalysisResult`: the finalizer over `file_diagnostics`, the passes and `OutputInputs` | |

- **Notation.** `file_decls(file)`, `file_sites(file)`, `file_names(file)` and `line_index(file)` in the text below
  are the `decls`, `sites`, `names` and `lines` fields of `file_products(file)`, each read with its own
  dependency.
- **The declaration index.** One tracked query over every eager file's `decl_names` (the `DeclRef`s alone),
  O(all declarations), run again when a declaration is added, removed or renamed and not when a member or a body
  changes: those change `decls` and leave `decl_names` equal, which is why the two are separate fields. It
  beats the alternative designs on what an edit costs: per-name input slots maintained by the driver (the first
  design considered) put derived data back into inputs and need a second revision per edit; a whole-index query
  is plain, and the re-running readers (`declarations_named` of the names asked so far, a hash probe each,
  about 374 Ir by REG-16.1's figure) back out at once when their slice is equal. For Piwigo's size that is of
  the order of 10 to 50 ms per edit that adds, removes or renames a declaration, and nothing for any other
  edit. S-3 measures it; the alternative is the slot design, and the switch is local to these two queries.
- **Why `declarations_named` returns declarations and not one:** decision 11. A name binds to the
  declarations PHP may bind it to; most names have one, a guarded polyfill has two, and each is its own
  entry. `Candidates` is `One(DeclRef)` or `Many`, and the level queries are per declaration; the provider
  combines candidates the way decision 6 combines a builtin's readings (H3.4).
- **`Builtin` declarations** are not in `decl_index`: `declarations_named` adds them from the corpus where they
  are available in the version range (J1.2), and their levels are queries keyed by `DeclKey` too, built from the
  corpus records. The corpus never changes, so queries that read nothing else are never re-verified.

### H2.4 The `Provider`: the accessors, and how a name is read

`Provider<'db>` is the only way the checker, the rules and the LSP read reflection. It holds `&'db dyn Db` and a
per-provider table (`FoldedName` to the answer), is made per check or per request, and is not `Sync` (a handle per
thread, as the database is). Its methods are REG-4.12's accessors (about 280 sites outside `registry/` become
calls on it): `class(&Name)`, `function(&Name)`, `constant(&ConstName)`, `method(&ClassView, &FoldedName)`,
`is_subclass_of`, `ancestors`, `class_constant`, `purity`, and the whole-registry iterations REG-7.10 names.
**The checker, the rules and the LSP do not name `Provider`: the checker-facing reader is a trait in
`reflection`** (these accessors; generic or `&dyn` in `Checker<'a>`), with `engine::Provider` as its implementor.
`analyser` sits below the engine, and a `Checker` that named `engine::Provider` would make `analyser` depend on
its own consumer, which `engine` already is (it calls `check_program`), a cycle the module graph forbids; the
trait keeps `File`, `DeclKey` and the other keys out, as H1.6 intends. H3.1's `LevelReader` is the smaller trait
on the level side.

- A read of a name: probe the provider's table by the name's stored hash (67 Ir, REG-16.1's figure for "one query
  per name per file, then a table of the file's own"); on a miss, `declarations_named(ClassKey::new(db, name))`
  (`FunctionKey` for a function, `ConstKey` for a constant) and the candidates' `class_final`. The provider keeps
  the answer, an absent one too. Inside a tracked function (`check_file`) the reads are recorded; outside one (an
  LSP request, a test) they are plain.
- **Granularity: a read goes to the query that answers it.** Whether a name exists reads `declarations_named`
  and nothing else; a subclass test reads `class_ancestors` (level 1), which an edit to a method or a property leaves equal; a
  member (a method, property, class constant or enum case, present or absent) reads `class_member`, a
  projection of `class_final` that returns that one member and backdates when the member is equal; only a
  whole-class question (abstract-method and interface-completeness checks, member listings) reads `class_final`.
  So editing one method's signature reruns the files that read that method, where today's `dependents` and
  `signature_dependents` stop at the class and rerun every file that reads it at all. `MemberKey` is the engine's
  interned (`DeclKey`, `MemberName`: H1.4); a method is asked by the `FoldedName` it was written as, so one
  method has one memo and one edge whatever spelling is used, and a property, class constant or case by its
  exact `Name`; an absent member is a recorded read (`None`), so a member added later
  reaches the readers that looked for it, and the `__call` and `__get` fallbacks are member reads of their own.
  When `class_final(C)` changes, the projections of C that were read rerun (a lookup and an equality each).
  **Cost, unmeasured:** REG-16.1 counted names (107,068 (file, name) pairs), not members, so the edge count at
  this grain is S-3's: it counts the distinct (file, class, member) reads and what their edges cost (a
  `QueryEdge` is 12 bytes and a packed one 8 in salsa's source, `zalsa_local.rs`, besides the memo's own
  overhead, H2.9), against the criteria H5 gives. The finer grain is the design; reading `class_final` for
  members is the fallback if S-3 kills it, a `Provider` setting that changes no caller.
- **Signature against body.** `class_final` holds no body, summary or position. What a body shows (purity,
  property writes, an inferred return) is its own query, asked when a rule asks, so a file that only calls a
  method depends on the method's signature and not on its body.
- **REG-4.12 first, on the old storage.** The accessors land before the engine, over the existing maps; the
  engine then replaces what is behind them. That is W2's order (H3.6).

### H2.5 The check

`check_file(db, File)` builds a `Provider` and a `Checker` (G1), runs `check_program` over
`parse_analysed(file)`'s root, and returns `FileCheck`. It reads: the file's `parse_analysed`, `file_decls`, `file_sites`, `file_names`, `line_index`; the
`CheckInputs` fields it consults (H2.2), `ReflectionInputs`, `Packages` (the owner); everything its provider reads;
and an included file's `file_digest` (an analysed file) or stamp (a context file's `file_stamp`), whose tree `includes.rs`
takes from `parse_analysed` or, for a context file, parses unmemoized, so editing an included file rechecks its includers, which today is the dependents index's job. **The statement-level poll `db.unwind_if_revision_cancelled()` is new work:** today `should_abort()` is
polled once per affected file, between `check_file` calls (`project.rs`), and no poll exists inside `checker.rs`
or `checker/*.rs`, so a cancelled write waits for the file being checked. The polls inside a file are added per
statement and, where one statement can be huge, per item of an array or call-argument list, per member when a
union is built, per declaration in the products pass and per size step in a parse over a stated size (S-5's accept
includes a 50 MB array-literal statement). Requests that need more than the memo (inlay hints, semantic tokens, hover,
completion probes, find-references' member collection) run the checker again, unmemoized, on a `Provider` outside any
query, as `checker_for` does today; E6's observers (Stage D) later replace the re-run.

### H2.6 Revisions

- **The batch run** writes every input before any query runs, and no write follows. salsa starts a revision
  at each input setter (`ingredient_mut` calls `new_revision`), so N writes are N + 1 revisions; that is
  harmless here (no handle exists to cancel, and the results do not depend on the count), and the cost of a
  `cancel_others` per write is paid only in the server. Nothing in the batch run is evicted by `lru`, since
  trees are not memos (H2.9).
- **The server** makes a revision per write, and **applies a notification's events as one batch** (today
  `handle_watched_files_changed` takes the whole `Vec<FileEvent>` and coalesces it to the last event per path;
  one `FileSet` write per event would cancel and wait once for each): `didOpen`, `didChange` (the overlay and the
  `stamp`), `didClose`, watched-file events, configuration. After the writes the server asks for `file_diagnostics` of each analysed
  file in priority order (the edited file, the open files, the rest), and publishes each as it completes and
  only if it differs from what was published. A file whose reads did not change verifies in a few
  microseconds (REG-16.1: all 2,792 in 5 to 10 ms); a file that read something that changed runs its check.
  This replaces `walk_affected`, `closure_scope`, `dependents` and `signature_dependents` wholesale.
- **TYPE-4's memo** carries the revision it was filled in and is cleared by the first access in a later one. A
  write cancels running queries, so no thread reads the memo at an older revision after a newer one began.

### H2.7 Concurrency and determinism

One database handle per worker, on the existing pool (`worker_pool`, 64 MiB stacks). **How the handles are
made:** the database is `Send` and not `Sync` (its `ZalsaLocal` holds a `RefCell`), so a closure that captures
`&db` cannot be shared; and rayon's `for_each_init` calls `init` once per rayon job, not once per thread, so it
would make a handle per job split. The driver therefore makes N handles (`db.clone()` on the driver thread, or a
`StorageHandle` and `into_storage` on each worker) and puts one in a slot per pool thread (indexed by
`rayon::current_thread_index`), or hands one to each of N long-lived worker tasks that pull files from a queue.
A write waits until every other handle is dropped (`Storage::cancel_others`, `storage.rs`, with no timeout, which
warns that a worker holding two handles deadlocks it), so a thread holds at most one, none survives into a write,
the driver stops dispatching once a write is pending (a worker that starts a unit after the flag is set only
makes a handle to drop it), and the server's request handlers take a handle per request and drop it before they
return. **The server applies writes on a thread other than the one that answers `$/cancelRequest` and
`shutdown`,** since the writer waits for every handle and an unbounded wait must not hold up those.
**Queries do not use `rayon` inside, and no closure inside a query touches a database handle:** a tracked query
body reads everything it needs on its own thread first, then fans plain data out to rayon (a handle cloned on
a worker would record its reads on its own query stack, not the pass's, and a same-thread cycle could be
reported when rayon runs a job inside a wait); the whole-program passes read the per-file facts sequentially
(about 2,800 reads) and parallelise only the pure merge, or are plain driver functions over query results. A
query that is already running on another thread is waited for by the asker (salsa blocks, with deadlock
detection for cross-thread cycles). **A stack guard:** a chain of tracked wrappers recurses on the stack (a
`class_ancestors(C)` calls `class_ancestors(parent)`, a hundred thousand deep for a hundred-thousand-class
chain, and constants that name each other likewise), and salsa has no stack growth, so the engine's wrappers call
`syntax::stack::guard`, and S-5 and S-6 add a 100,000-deep hierarchy and constant chain on a 256 KiB thread (F20's
depth test, extended to the engine). Determinism: no query reads the order of a hash map's
iteration (REG-4.14); a union's member order does not depend on scheduling (the rule `judge_property_types`
already follows: facts merge in path order); and nothing the engine sorts is ordered by `FileId` (H2.3). S-9
measures contention on shared levels, which REG-16.1 did not.

### H2.8 Cancellation and panics

- **Cancellation** is salsa's: a write unwinds other handles' queries with `Cancelled::PendingWrite`, and
  `Database::cancellation_token()` cancels one handle's (`Cancelled::Local`); both unwind by `resume_unwind`,
  without the panic hook. `$/cancelRequest` calls the request's token; F1's `CancellationToken` is that token.
  A cancelled request answers `ContentModified` or `RequestCancelled` (Part E 36 says which methods the client
  retries); a cancelled analysis unit stops and the next revision starts it again. **A local cancel is ignored
  while the thread executes a query with `cycle_result` or `cycle_fn`, and everything nested under it** (salsa
  disables local cancellation there, `function/execute.rs`, because a local cancel would poison the cycle; the
  token is also reset when the outermost attach scope ends), so a hover or completion that reaches a constant,
  purity or inference query cannot be interrupted by `$/cancelRequest` until that query returns, and only the
  pending-write flag still works. The inference passes get statement-level polls of the global flag, and S-5 has
  a case: `$/cancelRequest` during a long inference completes the request, and the server drops the answer
  instead of interrupting it.
- **Isolation, in two modes.** Each per-file unit and each request runs under `crash_context::isolated` (F1, the
  crash reporting plan). *At the driver, outside any query,* it first separates `Cancelled` (not an error:
  `Cancelled::catch`'s downcast) from any other payload (an internal error for that unit) and returns
  `Err(Cancelled)` for the unit. *Inside a tracked query* (the crash plan runs each whole-program pass as
  `isolated(Unit::Pass(name), ..)`, and `carry` catches a worker's panic and resumes with a `CarriedPanic`
  payload) it re-raises a `Cancelled` payload unchanged with `resume_unwind` and records only real panics, and
  `carry` passes `Cancelled` payloads through untouched: a `catch_unwind` that swallowed a `Cancelled` would let
  the query return, be memoized and survive into the next revision with a partial result. A unit that got
  `Cancelled::PropagatedPanic` (another thread's query panicked while this one waited on it) is run once more;
  if the same query panics again it is this unit's internal error, as it is for every file that reads a query
  that cannot complete. A test: a write arriving while a pass runs inside `analysis` leaves no `analysis` memo,
  and a real panic in one pass leaves the others' diagnostics.
- **Internal errors are not a query value.** A panic leaves its query unexecuted, so `analysis` and every
  whole-program query that reads a panicking `check_file` would panic too, and no `AnalysisResult` would exist
  to carry the report. So `check_file` catches the panic at its own boundary into a value, `FileCheck` with an
  internal-error record (the panic's payload text and the unit), which compares and persists like any result;
  `analysis` carries those records into `AnalysisResult` (exit status 2 and "a file with an internal error isn't
  cached" are F2's and follow from it: a `FileCheck` holding a record is not persisted). The "N more file(s)
  loaded for context" count of `check_project.rs` is a count of what this run read, which a lazy and persisted
  load changes between warm and cold, so it is reported from the files the load listed, not the files it read.
- **Cancellation by unwinding depends on the stack guard restoring its limit on unwind.** `syntax::stack::check`
  stores the outer limit, calls `stacker::grow` and restores `LIMIT` only when `grow` returns normally; stacker's
  `_grow` catches a panic inside the new segment, drops its own guard and resumes the unwind on the old stack,
  which skips the restore, so a later guarded call on that thread could compare against a freed segment's address
  (the case exists before this plan: `session.rs` wraps the parse in `catch_unwind`). F20's new `stack` crate
  restores `LIMIT` from a drop guard so that it also runs on unwind, with the test F20 lacks: a panic from inside
  a grown segment, caught, then a 100,000-level recursion on the same thread; S-5 repeats it with a `Cancelled`
  unwind.
- **A panic in a query** leaves it unexecuted; the next ask runs it again (the property REG-7.9 asks of
  `OnceLock`) for an ordinary query. **A query with a cycle strategy (`cycle_result`, `cycle_fn`) is poisoned
  instead:** salsa installs a poison guard for each execution (`execute.rs`), a panic leaves a valueless
  provisional memo, and a retry in the same revision throws `PropagatedPanic` again, so a constant, purity or
  inference query that panicked cannot be re-run until the next revision; the unit's internal error is built
  from the first panic's record, kept in the engine by query key. A panic-injection test over a shared level, a
  constant, a purity and an inference query, and a parallel-resolution test over cyclic fixtures, stay REG-7.9's
  tests.

### H2.9 Memory

- **Which trees are memos.** A tracked `parse` with `lru` for every file, evicted by `trigger_lru_eviction`, was the
  first cut. It frees every evicted tree on the one thread that evicts, which is the cost LOAD-3.3 measured and
  removed (−138.6 M Ir and −214 MiB peak when a context tree's drop moved to the worker that built it), and most
  trees are context trees. So a context file's tree is built in the worker's scratch builder inside `file_products`
  and cleared once the products are made (LOAD-3.5): it is freed, if at all, where it was allocated, and it is
  never a memo. A context file's tree wanted later (navigation into a vendor file) is parsed unmemoized. An
  analysed file's tree is a memo, `parse_analysed`, because its products and its check both read it. In the
  batch run it is kept for the run and dropped at exit, as the analysed trees are today; LOAD-5.2 (free each
  tree when its check ends) is memory only, and item 9 shrinks what it would free from 151 MB (MEAS-2; the
  analysed trees at exit) to the flat tree's size, so whether it is still wanted is S-7's to say, with the cost
  of freeing on another thread; **until S-7 says, W3 builds it as the ledger has it** (LOAD-5.2 needs LOAD-5.1,
  whose facts W3 builds, and LOAD-3.1; Part E 39), and S-7 chooses between freeing on the checking worker and a
  bounded eviction, or drops it explicitly in Part D. In the server
  `parse_analysed` has an `lru` (`set_lru_capacity`), so the batch and the server hold trees alike (Part E 28) and
  differ only in the capacity; a tree evicted is parsed again from the overlay or the disk, whose digest
  `file_digest` holds (LOAD-5.3). Eviction never loses cutoff for unchanged inputs (H2.1 3).
- **Memos.** A class's levels share structure (member maps behind `Arc`; a level stores what it changes), so a
  class costs about its final reflection plus the ancestors' set. `heap_size` is set on the heavy queries so the
  memo bytes are measured, not guessed. The dependency graph is one edge per distinct read of each query: about
  107,000 for Piwigo's checks at the grain of names (REG-16.1's count), 12 bytes each (`QueryEdge`; 8 packed),
  plus the member reads of H2.4 and the interned-key edges of H1.4, which S-3 counts. **Retained memos (Part E 38):**
  an immortal `DeclKey` keeps the memos of a dead declaration until the database is rebuilt, so `heap_size` also
  measures them over an edit replay, and H4.6's in-process compaction bounds them.
- **The kill criterion** is REG-12's: peak RSS at or below the phase start.
