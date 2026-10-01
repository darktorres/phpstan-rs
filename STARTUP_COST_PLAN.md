# Start-up and fixture-listing cost: plan

What the one-fixture `callgrind` profile found, and every change it
calls for. Nothing is left out for being small or for churn: each cost
the profile showed has a step, the stored registry's format is replaced
rather than patched.
It sits beside `PROJECT_BUILD_COST_INVESTIGATION.md` and
`BUILD_COST_LEDGER.md`, and continues that work: the stored registry
loaded in 40 ms there, and this is what is left of it.

## 1. What was measured

One fixture, `checker/narrowing/a_while_loop_excluding_iterator_aggregate_narrows_traversable_to_iterator`,
run with `--exact` from the `fixtures` test binary built with the `dhat`
profile and frame pointers, warm stub cache, under `valgrind
--tool=callgrind --separate-threads=yes` at `4a8d8f38`. 231.5 M
instructions; 0.05 s user. Sampling gave a few hundred samples and a
report step that took minutes; instruction counts are exact and the run
takes about a minute.

Instructions are not time. The decode is memory-bound, so its share of
wall time is probably larger than its share of instructions; the targets
below are in instructions and are estimates until measured.

The threads, and what depends on what: `main` lists every fixture and
only then starts libtest's test thread, so those two are a serial chain
(212 M of the 231 M). The rayon worker runs beside the test thread.

| thread | Ir | what it does |
|---|---|---|
| 1 (`main`) | 89.2 M | `fixtures::collect` 83.3 M |
| 15 (the fixture) | 123.3 M | `Project::resolved_stubs`, the stored registry's load, 122.4 M; the fixture's own analysis is under 1 M |
| 3 (rayon worker) | 17.6 M | `run_finishing_passes`; `closure_scope` loads the `StubIndex` 12.3 M |

### 1.1 `main`: `fixtures::collect` (89.2 M)

- Runs before any filter applies, for all 3,807 fixtures, even for one.
- `gate::fixture_config`: 36.8 M. `ConfigFile::load` 14.7 M (`parse`
  13.6 M, of which `toml::from_str` 7.9 M; `validate` 13.7 M), and
  `app::config_schema` rebuilt on each call, 6.1 M. It is called for each
  `.expect` file, but does that work only for the 755 fixtures that have a
  `.toml` (the other 3,052 cost one `is_file`); only 2 of the 755 use
  `requires_feature`, the one key `collect` reads. `run_fixture` parses
  the config again for the fixture that runs.
- `Path` work: about 25 M. `strip_prefix` 13.7 M, `with_extension`
  10.9 M, `Components::next_back` and `parse_next_component_back`
  underneath both. Per entry it also does `is_dir()` and
  `with_extension("expect").is_file()`, two more `stat`s.
- The sort of the trials: 8.0 M.

### 1.2 The fixture thread (123.3 M): loading the stored registry

All of it is inside `Project::new::BASE`, a `LazyLock` run by the first
`Session::load`. 95.6 M is `Registry::deserialize`:

| field | Ir |
|---|---|
| `global_constants`, `global_variables` (`FxHashMap<Name, Type>`) | 23.5 M |
| `classes` (`NameTable<ClassReflection>`) | 18.0 M |
| `functions` | 16.1 M |
| five member-owner tables (`NameTable<FxHashMap<Name, Name>>`) | 15.4 M |
| the raw-declaration diffs (`Option<Diff>`, two) | 10.4 M |
| three `NameTable<FxHashSet<Name>>` (`ancestor_names`, `subtype_children`, `trait_users`) | 3.5 M |
| other eager sets and maps (`FxHashSet<Name>` fields) | about 13 M |

Outside that: 5.9 M of `rustc-hash` (the cache's checksum, which runs
over the whole file before decoding) and about 3.5 M of hash-map
inserts, building each table's `Folded -> Lazy` map.

By kind of work, across the thread:

- **Copying.** `memcpy` is 21.1 M self (17%): 10.6 M through `realloc`
  (a `Vec` growing as `Bytes` is read), 5.0 M in `Bytes::deserialize`
  (`visit_bytes` does `to_vec` on bytes ciborium already copied), 1.4 M
  in the decoder's `pull`, 1.1 M in `String::deserialize`.
- **Names.** `Name::from(Cow<str>)` 18.9 M, `Interner::name` 16.4 M. Each
  stored name is decoded to a `String` (32.7 M), then hashed and interned,
  though a run looks up a few hundred of them.
- **Decoder.** `ciborium_ll::Decoder::pull` 10.5 M self, header parse
  4.4 M, `from_utf8` 3.7 M.

The tables already decode their entries lazily; what is paid at load is
everything around the entries: every name, every `Bytes` as its own
`Vec`, and every key hashed into a map.

### 1.3 The rayon worker (17.6 M)

`closure_scope` initialises the `StubIndex` (`stub_cache::load`, 12.3 M).
`packed::Records` stores `offsets: Vec<u32>` and ciborium reads it one
`u32` at a time: `next_element::<u32>` 7.4 M, `Deserializer::integer`
5.6 M. The other 5.2 M of `run_finishing_passes` is four passes over a scope of
one file: `resolve_transitive_purity` 1.85 M, `resolve_deferred_value_of`
1.80 M, `canonicalize_class_name_case` 1.11 M, `resolve_method_tags`
0.39 M. Their callees are a few thousand instructions, so the cost is in
their own bodies.

## 2. Rules

- Behaviour-neutral except where a step says otherwise: fixture
  expectations, corpus, type and baseline snapshots stay byte-identical.
  A step that has to change output lands that change first, as its own
  commit with an approved delta, as in `GATE_SPEED_PLAN.md`.
- One commit per step, checked cheaply before the next: fmt, clippy and
  the touched module's tests (`registry::`, the stored-registry round
  trip, `lexer` and `parser` where touched). No full suite and no corpus
  run per step.
- Instruction counts of the one-fixture profile are taken at the
  checkpoints named in section 4 (a minute each), not per step.
- At the end, once: lib suite, fixtures (`cargo test --test fixtures`,
  which `cargo test --workspace` skips), runtime oracle, differential
  tests, determinism, type snapshots byte-identical, corpus, and the
  gate timings against `gatetime`.
- No `unsafe`: the workspace denies it (`UNSAFE_PLAN.md`), which rules out
  `mmap`; the cache is read with `read` or `read_at`.
- A change to the stored format needs no migration: files are keyed by the
  build's source hash, so a new build never reads an old file.

## 3. Steps

### S1. Overlap the registry load with listing the fixtures

- `phpstan_rs::project` exposes a call that starts the stub registry's
  load (`Project::new::BASE`) and the `StubIndex`'s on threads of their
  own and returns at once; `fixtures::main` calls it before `collect`.
- Today the load (123 M) waits for the listing (89 M); after this the
  wall time of that stretch is the larger of the two.
- Both loads start together, so the test thread never waits for
  `closure_scope` to read the index.
- The `LazyLock` stays the one owner; the thread only forces it. Two
  statics exist: `Project::new::BASE` (plain stubs) and `phpstan_base()`
  (stubs plus PHPStan's source, 57 MB, needed only by a project whose
  files mention PHPStan). The call warms the plain base and
  `stubs::stub_index()` (a process-wide `OnceLock`); it does not load the
  PHPStan base, which most fixtures never use.

### S2. Fixture listing

- **S2a.** `app::config_schema()` is built once (`OnceLock`) instead of
  per call, which it is today in `fixture_config`.
- **S2b.** `collect` stops loading each config through the schema and
  `validate`. It needs one thing from a `.toml`, the `requires_feature`
  list, so `gate` gets a function that parses the file with the plain TOML
  parser and reads that top-level key (a real parse, not a text search:
  a key can be quoted or escaped). It runs for the 755 files with a
  `.toml`, in parallel (S2d). A file that fails to parse today is not
  ignored (the `Err(_) => false` arm); that stays, since the failure then
  surfaces when the fixture runs.
- **S2c.** One `read_dir` pass per directory using `DirEntry::file_type`
  (no `is_dir` `stat`), names built from the parent's relative name
  (`format!` once, no `strip_prefix`, no `with_extension("")` per entry),
  and a directory's `.expect` sibling known from the listing already in
  hand instead of a second `is_file`.
- **S2d.** The directory walk runs on the worker pool, one task per
  directory, the results joined and sorted once by name (a sort of
  strings already in hand, 8 M today).
- **S2e.** `run_fixture` and `fixture_config` still parse a config when a
  fixture runs; nothing else reads it twice.
- Expected: `collect` 83 M to under 8 M.

### S3. The stored registry: a new format

The largest step, in parts, each its own commit. The file stops being
one ciborium value. The shape that comes out of the profile:

- one byte buffer read once and shared (`Arc<[u8]>`), never copied into
  per-entry `Vec`s;
- tables stored as sorted records addressed by position, looked up by
  bisection on the folded name, the way `packed::Packed` already stores
  the stub index;
- names turned into `Name`s, and so interned, only when a lookup or an
  iteration reaches them;
- entries decoded on first read, as now.

- **S3a. The container.** A header (magic, format version, section
  table: name, offset, length, checksum) and the sections' bytes, written
  by `stub_cache::store` and read by `load` into one buffer. `load`
  returns section views, not a decoded value; the stored `Registry`
  becomes a set of sections (resolved declarations, derived tables,
  scalar fields, raw diffs, the `StubIndex`, the PHPStan overlay).
  `name_table::Bytes` becomes a `(Arc<[u8]>, Range<u32>)` slice with no
  copy. The truncated-file and damaged-file behaviour of `load` stays:
  the checksum is checked, a mismatch rebuilds, a file that passes it
  and doesn't decode says so. `store` writes the sections the same way
  on a cold cache; the first run after a build is timed once against the
  base commit's, so the cold path does not regress.
- **S3b. Tables as records.** `NameTable`'s frozen base becomes a view
  over a section: a key array of `(name_off, name_len, entry_off,
  entry_len)` in little-endian `u32`s sorted by the folded name, a string
  pool, an entry pool. Both the folded key and the declared name are
  stored (the declared name is what `&Name` returns; they differ only in
  ASCII case, as `function_key` folds with `to_ascii_lowercase`), and the
  array is sorted by the folded bytes. Lookup by `function_key` is a
  bisection on them, with no `Folded` and no hash map built at load; this
  removes the string decode, the intern, the hashing and the inserts of
  1.2 (the bulk of the 95.6 M, with S3c to S3f). Each slot has a
  `OnceLock<Name>` for the places that hand out `&Name`
  (`get_with_name`, iteration): a zeroed array at load, filled on demand.
  Bisection is about 13 comparisons at the 8,000 entries of `classes`,
  and a run does hundreds of lookups; a hash index on disk, a perfect
  hash or an FST would save comparisons a run doesn't make and cost
  either a rebuilt map at load or a dependency, and a sorted layout also
  gives one stable iteration order.
  - **Iteration order.** The base was a hash map, so its iteration order
    was hash order and is now key order. Every `for (name, _) in &table`
    and `.iter()`, `.keys()` and `.values()` over a `NameTable` (35 sites
    outside tests, in `scope.rs`, `resolve.rs`, `hierarchy.rs`,
    `project.rs`, the LSP's `workspace_symbol` and `completion`, and
    `main/demo.rs`) is audited: where the result is
    sorted or counted afterwards, nothing changes; where it is not, the
    consumer sorts explicitly (that is a behaviour fix first, as its
    own commit, with the approved delta if any output moves). The
    determinism gate (`--emission-order`) and the type snapshots are the
    check.
  - `hidden` and the overlay `entries` map are unchanged: they hold what
    a project adds or removes over the base.
  - `freeze`, `Clone` and `diff_against` keep their meaning; clones share
    the view by `Arc`, as they share the base now.
- **S3c. The raw declarations.** Stored as what differs, positionally:
  the raw table has the resolved table's key array (shared, not stored
  again), a bitmap of the entries that differ and their bytes, plus two
  short lists for entries only one side has. `from_diff`'s loop over
  every name disappears; the 10.4 M and the second decode of every
  class and function name go with it. A fixture that adds a declaration
  reads raw entries through the same view.
- **S3d. Shared keys for the derived tables.** `ancestor_names`,
  `subtype_children`, `trait_users`, `recorded_parents`, the five
  member-owner tables and `built_in_stub_classes` are keyed by class
  names, but whether each key set equals `classes`' is not verified. The
  first commit of this step checks it on the stored registry (a test that
  compares every table's keys with `classes`'). The design does not need
  them equal: one shared key array is the union, and each table is a
  column over it (its own offset column, with an absent marker, and its
  own overlay; one `Arc<KeySet>` in the stored registry). Names are
  stored once; the 15.4 M and 3.5 M of those tables' name decodes go.
- **S3e. `global_constants` and `global_variables`.** Both are
  `FxHashMap<Name, Type>` decoded whole (23.5 M) though a run reads a few
  entries. They become `NameTable<Type>`, which keeps the `pub` API in
  one place: `constant_type(name)`, `variable_type(name)`, an iterator
  for the places that walk them, and `changed_since`'s comparisons.
  Callers of the fields (`global_constants.get`, `.iter`, `.insert`) move
  to those. `NameTable` folds case in its key function, which these two
  maps don't; the key each needs (exact, or folded in part for a
  namespaced constant) is taken from how the maps are looked up today,
  and `NameTable` gains a key mode for it.
- **S3f. The other eager fields.** `unconditionally_declared_classes`,
  `unconditionally_declared_functions`, `stub_provided_functions`,
  `names_with_assert_tags`, `function_decl_sites`, `phpstan_class_files`,
  `property_writes`: each is audited for what a run reads. The profile
  puts about 13 M on `FxHashSet<Name>` fields without saying which; the
  first commit attributes it by field (the callee tree of `visit_map`),
  so each change is checked against its own share. A set read
  by `contains` becomes a sorted-key view with an overlay for the
  project's additions (a `NameSet`, a `NameTable<()>`); a map read by key
  becomes a `NameTable`; one read whole or recomputable is recomputed on
  first use instead of stored. Each gets a test that the view and the old
  map agree on every key of the stored registry and on keys it lacks.
- **S3g. The `StubIndex` records.** `packed::Records` keeps its offsets
  as one little-endian byte array read with `u32::from_le_bytes` at the
  position, not as a `Vec<u32>` decoded element by element (7.4 M). It
  is a section of the container: the worker reading it neither decodes
  nor copies.
- **S3h. Checksums.** One per section, checked when the section is read,
  so a run that never reads the PHPStan overlay (the 57 MB file) doesn't
  hash it. The hash itself is a vetted one (`xxhash-rust`'s XXH3, or
  `crc32fast`; exact-pinned, through `cargo deny`), not `FxHasher::write`
  over the whole buffer (5.9 M, one dependent chain) and not one written
  here. Neither is for security: it only has to tell a damaged file from
  a whole one, which the rename on write already makes rare. The truncated and corrupted-file tests run
  against every section.
- **S3i. How the file is read.** `fs::read` copies the whole file
  (23 MB; 57 MB with PHPStan's source) before anything is decoded; the
  kernel's copy is not in these counts. Measure it (wall time
  beside the instruction counts, `strace -c`), then, if it matters, read
  the header and the sections a run needs with positional reads
  (`FileExt::read_at`) instead of the whole file. Keep the whole read
  where the measurement says it is cheaper.
- **S3j. The entry codec.** Entries and scalar fields are CBOR through
  ciborium, with a recursion limit of 65,536 because `Type`s nest deeply.
  `Decoder::pull` (10.5 M self), header parsing (4.4 M), `from_utf8` and
  `String` allocation are the cost of a self-describing format, paid by
  every entry a run touches. Replace it with a compact non-self-describing
  serde format that borrows strings (`postcard`, exact-pinned like the
  other dependencies and through `cargo deny`), so `Name` deserialises
  from `&str` straight into the interner with no `String`. No stored type uses
  `skip_serializing_if`, `untagged`, `flatten`, `with` or
  `deserialize_any` (checked by search of `registry`, `types` and
  `syntax`), the attributes a non-self-describing format can't take.
  `Type`'s `Repr` impl and `syntax::stack::guard` stay: stack depth, not
  a format's limit, bounds recursion. Considered and not chosen: keeping
  CBOR (the cost above), and a zero-copy archive such as `rkyv`, which
  needs an archived form of every stored type (`Type` is interned, `Name`
  is an `Arc<str>`) and makes consumers read archived forms: a rewrite of
  the registry's API, not of the format. Decide by the one-fixture profile
  after S3a to S3i: what remains of the entries decoded, and of
  `Decoder::pull`.
- Expected for the fixture thread: 123 M to about 15 M (the lookups, the
  few hundred entries a fixture touches, and the first `Name`s).

### S4. The rest of the worker's start-up

After S3g the `StubIndex` load is small. What is left of
`run_finishing_passes` is the four passes of 1.3 (5.2 M), each over a
scope of one file and spending its time in its own body. Each is read for
work that grows with the stubs, not with the scope (a walk over every
class or function, an entry decoded that the scope doesn't name), and
bounded by the scope. If they are independent of the registry load they
run on the thread S1 starts.

### S5. Interning

In the fixture thread every call of `Interner::name` (16.4 M) comes from
`Name::from(Cow<str>)`, which is how stored names are decoded; S3b and
S3j remove them. What the interner costs after that is read from the
one-fixture profile taken after S3, and only a cost it shows gets a
change.

### S6. Gates and docs

- `gates/module-graph.txt` gets a line for each new module (the
  container, the records view, the key set).
- `PROJECT_BUILD_COST_INVESTIGATION.md` section 12 and
  `BUILD_COST_LEDGER.md` record the measurements from each checkpoint,
  with the commands that produced them.
- `CLAUDE.md`'s profiling section gains the instruction-count recipe:
  callgrind for runs under about a second, sampling for the rest.
- The stored-registry round-trip test (`registry/tests/stub_cache_round_trip.rs`)
  compares every entry as before, and gains: lookups by bisection against a
  hash-map reference on the real stored tables, plus keys that differ
  only in case and keys that are absent; the overlay and `hidden`
  over a view; a section that fails its checksum, a truncated header,
  a wrong version; and iteration order on equal tables.

## 4. Checkpoints

The one-fixture profile (the recipe in `CLAUDE.md`, with `callgrind`) was
taken at `4a8d8f38`; it is run again after:

| after | what moved |
|---|---|
| S1, S2 | the serial chain: `main` 89 M to under 10 M, hidden behind the load |
| S3a, S3b | name tables, `Bytes`, hashing |
| S3c to S3f | raw diffs, derived tables, eager maps and sets |
| S3g, S3h, S3i | the worker, the checksum, the read |
| S3j | the entry codec |
| the end | everything, with the whole-suite timings |

Targets, to be replaced by measurements: `main` under 10 M, the fixture
thread about 15 M, the worker about 2 M; about 30 M of 231 M. Anything
that lands far from them is traced before the next step, not carried.

## 5. Outcome

Measured on the same fixture, with the same recipe (the `dhat` profile with
frame pointers, a warm cache, `callgrind --separate-threads=yes`), after
each of the steps below. Wall time is `hyperfine -N` on the same binaries,
alternating, under a load average of about 2.4 at `nice -n 19`.

| after | instructions | main | registry thread |
|---|---|---|---|
| base (`4a8d8f38`) | 231.5 M | 89.2 M | 123.3 M |
| the container and record tables | 123.1 M | 6.0 M | 65.1 M |
| the stub index in sections | 83.6 M | 6.0 M | 32.8 M |
| CRC, bounds-only validation, the declared-names view | 66.4 M | 6.0 M | 30.5 M |
| scoped lookups in the finishing passes | 53.4 M | 6.0 M | 17.0 M |
| the final listing changes | 50.9 M | under 3.6 M | 17.6 M |

Warm, one fixture: 113-118 ms before, 26-30 ms after (about 4 times).
Cold cache: 0.74-0.94 s before, 0.62-0.66 s after, so the first run after
a build does not regress. The whole fixture suite, 3,807 fixtures, passes
and runs in 2.63 s on the `gate` profile; the lib suite (981), the CLI,
stub-index, scaling, LSP exit code and version tests pass; the type
output of every fixture is byte-identical to the base's snapshot
(compared as the gate records it, in byte order of the dump names).

What was done, by step, and where it differs from the text above:

- **S1** `93e5d4a7`: `Project::warm_stubs()` starts the registry and the
  stub index loading on threads of their own; `fixtures::main` calls it.
- **S2** `216e8ef1`, `3436a023`, `fc8d6875`: one `read_dir` pass per
  directory, listed in parallel; a config is parsed (by the plain TOML
  parser, for `requires_feature`) only where the listing shows one, and
  read in full only where it sets the key; the schema is built once; the
  names are sorted before the trials are made, in parallel. `strace`
  showed the other cost the instruction counts hid: 3,985 `openat` calls,
  3,060 of them failing, for configs that were not there; it is now 931.
  The listing is the same, name for name.
- **S3a-c** `5a13c35a`: the container (`registry/container.rs`), the
  tables as records read in place (`name_table.rs`), the declarations as
  built stored as records pointing at the resolved entries' bytes. The
  raw declarations needed no diff: entries are written once by content, so
  an entry equal in both tables is one set of bytes. Iteration of a stored
  base is in lowercased name order; no consumer's output moved (the
  fixtures, the type dumps and the lib suite are identical).
- **S3e, S3f** `7b183f91`: `NameMap` and `NameSet` (`name_map.rs`) over the
  same base, exact-name: `global_constants`, `function_decl_sites`,
  `phpstan_class_files` and the four sets. `global_variables` and
  `property_writes` stay eager: after these steps the whole of
  `Registry::load` is 2.9 M instructions, so there is nothing left in
  them to save.
- **S3g** `864607fe`: `Records` and `Packed` read their offsets and data in
  place; the stub index is sections; PHPStan's own names are records read
  when asked for.
- **S3h** `739abc65`: one CRC over the file (`crc32fast`, already in the
  lockfile; it takes 10 M instructions of SIMD, which is the largest item
  left on the registry thread, and about 1.5 ms). Not per section: the
  PHPStan registry is a file of its own, so a run never hashes what it
  doesn't read. The records' bounds are checked on every load; their text
  and order only in a debug build, as only a bug in the writer could break
  them and the round-trip tests read every table of the real registry.
- **S3d** (shared keys): dropped. The writer stores each name once, and a
  table is sixteen bytes a name read in place, so there is nothing left to
  share; a column design would add indirection to every lookup.
- **S3i** (how the file is read): measured, kept as it is. Reading the
  22.7 MB file takes 4.3 ms and its CRC about 1.5 ms. Positional reads of
  the entries a run touches would save most of the copy but need either
  the whole-file check gone or one per entry, and a damaged file would
  then fail mid-run instead of being rebuilt. The read runs on the thread
  S1 starts, beside the listing.
- **S3j** (the entry codec): not done. In the final profile no entry
  decode is among the costs of a one-fixture run, so the profile does not
  support the change; it needs a profile of a run that touches many stub
  classes first.
- **S4** `739abc65`, `d12af24e`: the finishing passes read the names in
  their scope. `canonicalize_class_name_case` asks a read-only view of the
  table's names (`declared_names`) for the names it is given instead of
  building a map of every class; `iter_mut_scoped` and
  `resolve_property_writes` look up the scope's own keys instead of
  filtering every stored name through the scope.
- **S5**: nothing to do. Every `Interner::name` call in the fixture thread
  came from decoding stored names, and no stored name is decoded to a
  string now.
- **S6**: `gates/module-graph.txt` needs no line: a module inherits its
  parent's crate, and `cargo xtask modgraph` passes with `container` and
  `name_map` in it. The stored-registry tests (`stub_cache_round_trip.rs`,
  `stubs/tests/stored_index.rs`, and unit tests in `container.rs`,
  `name_table.rs`, `name_map.rs`, `packed.rs`) compare every table, map,
  set and record with the registry they were stored from, and cover
  bisection in any case, changes over a stored base, a frozen table, damaged
  and truncated files and a file of another version.

What is left on the one-fixture profile (50.9 M): the CRC (10 M), the
listing's TOML parse of the 755 configs (about 4 M) and its tasks, the
finishing passes over a scope of one file (a few million), and the fixture
itself (about 2 M).
