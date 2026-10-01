# Start-up and fixture-listing cost: plan

What the one-fixture `callgrind` profile found, and every change it
calls for. Nothing is left out for being small or for churn: each cost
the profile showed has a step, the stored registry's format is replaced
rather than patched, and the steps that need a measurement first say so.
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
  `.expect` file, and `run_fixture` calls it again for the one that runs.
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
5.6 M. The other 5.2 M of `run_finishing_passes` is not attributed yet.

### 1.4 What the profile does not show

The fixture's own analysis is under 1 M, so per-fixture cost is not
visible. A fixture is one of 3,807 in a process that pays the load once.
Step 0 adds the measurement that shows it.

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

### S0. Measurement

- **S0a. A one-command profile.** `cargo xtask profile-one [<fixture>]`:
  builds the frame-pointer `dhat` test binary in a target directory of its
  own, runs the fixture once to warm the cache, runs it under callgrind,
  and prints the instructions per thread and the named inclusive costs of
  section 1 (`collect`, `fixture_config`, `resolved_stubs`,
  `Registry::deserialize` by field, `memcpy`, `Interner::name`,
  `stub_cache::load`), writing them to
  `.corpus-runs/gate/profile/<commit>.tsv`. The CLAUDE.md recipe, as a
  command, with the `--exact` name as its argument.
- **S0b. The per-fixture baseline.** The same under callgrind for a
  filter of a few hundred fixtures in one process (`checker/narrowing`,
  284; `project/`, 223 multi-file), once on the base commit. Its
  inclusive costs beyond `resolved_stubs` and `collect` are the missing
  half of the picture; each cost over 5% of the run gets a row in
  section 5 of this file before S9 is planned.
- **S0c. `check-project` start-up.** The same profile for the CLI's
  workload (CLAUDE.md: one file in a directory, `check-project
  --no-composer .`): which of these costs the CLI pays, and what it waits
  on while the registry loads.

### S1. Overlap the registry load with listing the fixtures

- `phpstan_rs::project` exposes a call that starts the stub registry's
  load (`Project::new::BASE`) and the `StubIndex`'s on threads of their
  own and returns at once; `fixtures::main` calls it before `collect`.
- Today the load (123 M) waits for the listing (89 M); after this the
  wall time of that stretch is the larger of the two.
- Both loads start together, so the test thread never waits for
  `closure_scope` to read the index.
- The `LazyLock` stays the one owner; the thread only forces it.
- Applies equally to every binary that loads a project before it has
  anything to do with it: S0c says where (`check-project` reading the
  config and composer manifest, the LSP's `initialize`).

### S2. Fixture listing

- **S2a.** `app::config_schema()` is built once (`OnceLock`) instead of
  per call, which it is today in `fixture_config`.
- **S2b.** `collect` stops parsing configs. A fixture's `.toml` is read
  only for the trials it ignores: `collect` checks the file's text for
  `requires_feature` and parses only those, as that key is the only
  thing `collect` uses. A file that fails to parse today is not ignored
  (the `Err(_) => false` arm); that stays, since the failure then
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
  and doesn't decode says so.
- **S3b. Tables as records.** `NameTable`'s frozen base becomes a view
  over a section: a key array of `(name_off, name_len, entry_off,
  entry_len)` in little-endian `u32`s sorted by the folded name, a string
  pool, an entry pool. Lookup by `function_key` is a bisection on the
  folded text, with no `Folded` and no hash map built at load; this
  removes the string decode, the intern, the hashing and the inserts of
  1.2 (about 90 M with 3c to 3f). Each slot has a `OnceLock<Name>` for
  the places that hand out `&Name` (`get_with_name`, iteration): a zeroed
  array at load, filled on demand.
  - **Iteration order.** The base was a hash map, so its iteration order
    was hash order and is now key order. Every `for (name, _) in &table`
    and `.iter()` over a `NameTable` is audited: where the result is
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
  member-owner tables and `built_in_stub_classes` index nearly the same
  names as `classes`. Each is a column over one shared key array (one
  `Arc<KeySet>` in the stored registry; each table keeps its own offset
  column and its own overlay). Names are stored once; the 15.4 M and
  3.5 M of those tables' name decodes go.
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
  `property_writes`: each is audited for what a run reads. A set read
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
  hash it. The hash itself: four independent 64-bit lanes over 32-byte
  blocks, folded at the end, rather than `FxHasher::write` over the whole
  buffer (5.9 M, one dependent chain). It only has to tell a damaged
  file from a whole one. The truncated and corrupted-file tests run
  against every section.
- **S3i. How the file is read.** `fs::read` copies the whole file
  (23 MB; 57 MB with PHPStan's source) before anything is decoded; the
  kernel's copy is not in these counts. Measure it (S0a's wall time
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
  from `&str` straight into the interner with no `String`. `Type`'s
  `Repr` impl and `syntax::stack::guard` stay: stack depth, not a
  format's limit, bounds recursion. Decide by S0b's profile: the
  entries a few hundred fixtures read, decoded.
- Expected for the fixture thread: 123 M to about 15 M (the lookups, the
  few hundred entries a fixture touches, and the first `Name`s).

### S4. The rest of the worker's start-up

After S3g, the `StubIndex` load is small; what is left of
`run_finishing_passes` (5.2 M, unattributed) gets a profile of its own:
its inclusive tree, then a step per cost over 1% of the run. If it is
independent of the registry load it moves onto the thread S1 starts.

### S5. Interning

`Interner::name` hashes the text (`text_hash`), takes a shard and
`get_or_insert`s, allocating `Name::new` on a miss. After S3 the stored
registry stops being its biggest caller; what remains is read by whoever
interns names in bulk: the lexer and parser for every file, and decoded
entries. Measure that on the S0b profile, then: reserve the shards' capacity
from the stub index's name count, intern from the borrowed text of S3j
without an intermediate `String`, and look at the shard count against the
rayon pool's threads.

### S6. Per-fixture cost

Everything S0b finds above 5% that is not the registry's load: the step
list is written then, one row each, into section 5. Candidates the code
suggests, none measured yet: `Project::new`'s clone of the base and what
`finish_loading` redoes for a project of one file, the worker pool each
`Session` is given, and `check_program`'s per-file setup.

### S7. `check-project`

What S0c shows the CLI pays: the same load, so the format of S3 serves
it; the overlap of S1 for the stretch between process start and the
first file read; and whatever it does serially that the profile of the
corpus runs showed before (`GATE_SPEED_PLAN.md` section 2).

### S8. Gates and docs

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

The one-fixture profile (S0a) is run on the base commit and after:

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

## 5. Per-fixture findings

Filled in from S0b and S0c, one row per cost, each with the step that
takes it.

| cost | Ir | step |
|---|---|---|
